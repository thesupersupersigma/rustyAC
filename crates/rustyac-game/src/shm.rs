// SPDX-License-Identifier: GPL-3.0-or-later

//! The game's shared memory, published by rustyAC: `Local\acpmf_physics`, `acpmf_graphics`
//! and `acpmf_static` with AC's own layouts (`docs/map/telemetry.md`), so `ac_telemetry.py`
//! and dashboard apps read rustyAC the way they read AC.
//!
//! The physics page is the car's own ([`PhysicsPageWriter`](rustyac_physics::car::PhysicsPageWriter),
//! bit-exact with the game's, including its 300 steps of warm-up) and is copied out after
//! every step. The graphics page is the game's `SharedMemoryWriter::update`, ported in
//! [`rustyac_physics::race::shm`], once per frame of the main thread.
//!
//! The names are AC's, so the two cannot publish at once: rustyAC refuses to start its writer
//! while `acs.exe` runs.

use windows::core::PCWSTR;
use windows::Win32::Foundation::{CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, HANDLE, INVALID_HANDLE_VALUE};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::Memory::{
    CreateFileMappingW, MapViewOfFile, UnmapViewOfFile, FILE_MAP_ALL_ACCESS, MEMORY_MAPPED_VIEW_ADDRESS, PAGE_READWRITE,
};
use windows::Win32::System::Threading::GetCurrentProcessId;

use rustyac_physics::car::telemetry::PAGE_SIZE;

use crate::input_file::StepInput;
use crate::physics_thread::StepSink;
use crate::sim::GameSim;

use rustyac_physics::race::flags::Flag;
use rustyac_physics::race::shm::{ac_version, AC_REPLAY, referee_frame, static_page, GraphicsFrame, GraphicsWriter, OnlineStatic, StaticFacts};
pub use rustyac_physics::race::shm::{Kind, Page, AC_LIVE, AC_OFF, AC_PAUSE, GRAPHICS_FIELDS, GRAPHICS_SIZE, STATIC_FIELDS, STATIC_SIZE};


/// `DrivingAssistManager::idealLine`: whether the ideal line shows (the window's Ctrl+I and
/// `assists.ini` set it; the graphics page reports it).
pub static IDEAL_LINE_ON: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Task 44c: `ReplayManager::isInReplaymode` and its `timeMult` (bits), as the desktop's replay
/// has them.
pub static IN_REPLAY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
pub static REPLAY_TIME_MULT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0x3f80_0000);


/// Is the real game running? Looks at the names of the running processes, nothing more.
/// Also reports another `rustyac.exe` (two writers on one page would garble it).
pub fn other_writer_running() -> Option<String> {
    // SAFETY: a process snapshot is read through the documented calls with a correctly sized entry.
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0).ok()?;
        let mut entry = PROCESSENTRY32W { dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
        let own = GetCurrentProcessId();
        let mut found = None;
        let mut more = Process32FirstW(snapshot, &mut entry).is_ok();
        while more {
            let length = entry.szExeFile.iter().position(|c| *c == 0).unwrap_or(entry.szExeFile.len());
            let name = String::from_utf16_lossy(&entry.szExeFile[..length]).to_ascii_lowercase();
            let ac = name == "acs.exe" || name == "acs_x86.exe";
            if ac || (name == "rustyac.exe" && entry.th32ProcessID != own) {
                found = Some(if ac { format!("Assetto Corsa ({name})") } else { "another rustyac.exe".to_string() });
                break;
            }
            more = Process32NextW(snapshot, &mut entry).is_ok();
        }
        let _ = CloseHandle(snapshot);
        found
    }
}

struct Mapping {
    handle: HANDLE,
    view: MEMORY_MAPPED_VIEW_ADDRESS,
    size: usize,
    /// The object was there already (a reader of an earlier session still holds it).
    existed: bool,
}

impl Mapping {
    fn create(name: &str, size: usize) -> Result<Mapping, String> {
        let wide: Vec<u16> = name.encode_utf16().chain([0]).collect();
        // SAFETY: `wide` is a terminated string that outlives the call; the view is mapped
        // for `size` bytes and only ever written within them.
        unsafe {
            let handle = CreateFileMappingW(INVALID_HANDLE_VALUE, None, PAGE_READWRITE, 0, size as u32, PCWSTR(wide.as_ptr()))
                .map_err(|e| format!("CreateFileMapping {name}: {e}"))?;
            let existed = GetLastError() == ERROR_ALREADY_EXISTS;
            let view = MapViewOfFile(handle, FILE_MAP_ALL_ACCESS, 0, 0, size);
            if view.Value.is_null() {
                let error = windows::core::Error::from_thread();
                let _ = CloseHandle(handle);
                return Err(format!("MapViewOfFile {name}: {error}"));
            }
            Ok(Mapping { handle, view, size, existed })
        }
    }

    fn write(&self, bytes: &[u8]) {
        let count = bytes.len().min(self.size);
        // SAFETY: the view is `self.size` bytes long. Readers copy the page without a lock, as
        // they do with the game's (which copies it 16 bytes at a time, lowest address first).
        unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), self.view.Value as *mut u8, count) }
    }
}

impl Drop for Mapping {
    fn drop(&mut self) {
        // SAFETY: view and handle were created in `create` and are released once.
        unsafe {
            let _ = UnmapViewOfFile(self.view);
            let _ = CloseHandle(self.handle);
        }
    }
}

// SAFETY: the mapping is owned by whoever holds the struct; the raw view is plain memory.
unsafe impl Send for Mapping {}

/// The three pages, created like the game creates them.
pub struct SharedMemory {
    physics: Mapping,
    graphics: Mapping,
    statics: Mapping,
}

impl SharedMemory {
    /// Creates the pages; refuses while the real game (or another rustyAC) is running.
    pub fn create() -> Result<SharedMemory, String> {
        if let Some(who) = other_writer_running() {
            return Err(format!(
                "{who} is running and owns the shared memory pages of that name: rustyAC does not publish its own \
                 (close it, or start rustyac with --no-shm to silence this)"
            ));
        }
        // The game creates all three with the physics page's size and relies on Windows
        // rounding a mapping up to whole 4096-byte pages for the longer static page; here
        // the static page asks for its own size.
        Ok(SharedMemory {
            statics: Mapping::create("Local\\acpmf_static", STATIC_SIZE)?,
            graphics: Mapping::create("Local\\acpmf_graphics", PAGE_SIZE)?,
            physics: Mapping::create("Local\\acpmf_physics", PAGE_SIZE)?,
        })
    }

    /// Did the pages exist already (left open by a reader of an earlier session)?
    pub fn reused(&self) -> bool {
        self.physics.existed || self.graphics.existed || self.statics.existed
    }
}

/// Publishes the physics page after every step and the graphics page every frame of the
/// main thread (every referee frame, every third step).
pub struct ShmSink {
    memory: SharedMemory,
    graphics: Page,
    writer: GraphicsWriter,
    status: i32,
    static_written: bool,
    /// the referee frame (or step) the graphics page was last written for
    last_frame: u64,
}

impl ShmSink {
    pub fn new(memory: SharedMemory) -> ShmSink {
        ShmSink { memory, graphics: Page::graphics(), writer: GraphicsWriter::default(), status: AC_LIVE, static_written: false, last_frame: 0 }
    }

    /// The static page of this car (the game writes it once, with the first frame).
    pub fn static_page(sim: &GameSim) -> Page {
        let (track, track_config, spline_length, sector_count) = match &sim.track {
            Some(track) => (track.name.as_str(), track.config.as_str(), track.length(), track.sectors_normalized_positions.len() as i32),
            None => ("rustyac_flat", "", 0.0, 1),
        };
        let referee = sim.referee.as_deref();
        // driverInfo.name: the game's three name fields all hold it
        let name = sim.setup.referee.as_ref().and_then(|referee| referee.driver_names.first()).filter(|name| !name.is_empty()).map_or("Player", |name| name.as_str());
        let online = sim.online.as_ref().map(|session| {
            let accepted = &session.net.client.accepted;
            // the race manager's sessions in turn: the last race session's isTimedRace
            let is_timed_race = referee.and_then(|r| r.manager.sessions.iter().rfind(|s| s.session_type == rustyac_physics::race::manager::SESSION_RACE)).is_some_and(|s| s.is_timed_race);
            OnlineStatic { is_timed_race, has_extra_lap: accepted.has_extra_lap, reversed_grid_positions: accepted.inverted_grid_positions as i32, pit_window: (accepted.pit_window_start as i32, accepted.pit_window_end as i32) }
        });
        static_page(
            &sim.car.car,
            &StaticFacts {
                ac_version: &ac_version(),
                sessions: referee.map_or(1, |referee| referee.manager.sessions.len() as i32),
                cars: (1 + sim.opponents.len() + sim.remote_count()) as i32,
                // the car's folder name, however `--car` named it
                car_model: &crate::sim::car_name(&sim.data_path),
                track,
                track_config,
                spline_length,
                sector_count,
                // (no session list: penalties on a track, as before)
                penalties: referee.map_or(sim.track.is_some(), |referee| referee.manager.penalties_enabled),
                player_name: name,
                skin: sim.setup.player_skin.as_deref().unwrap_or_default(),
                max_power_w: sim.physics_info.max_power_w,
                max_turbo_boost: sim.physics_info.max_turbo_boost,
                online,
            },
        )
    }

    fn write_graphics(&mut self, sim: &GameSim) {
        let car = &sim.car.car;
        let db = &sim.lap_db;
        let last = db.laps.last().cloned().unwrap_or_default();
        let mut frame = match &sim.referee {
            Some(referee) => referee_frame(referee),
            // (no session list: one endless practice)
            None => GraphicsFrame {
                replay: false,
                paused: false,
                esc_menu: false,
                session_type: car.env.session_type,
                session_laps: 0,
                current_splits: &db.current_splits,
                last_lap_time: last.time,
                last_lap_last_split: last.splits.last().copied().unwrap_or(0),
                best_lap_time: db.best_lap.time,
                lap_count: db.laps.len() as i32,
                position: 1,
                session_time_left: 0.0,
                flag: Flag::None,
                ideal_line: false,
                replay_time_multiplier: 1.0,
                in_pitlane: car.is_in_pit_lane(),
                mandatory_pit_done: false,
                time_limit_left: None,
            },
        };
        frame.paused = self.status == AC_PAUSE;
        frame.esc_menu = frame.paused;
        frame.ideal_line = IDEAL_LINE_ON.load(std::sync::atomic::Ordering::Relaxed);
        frame.mandatory_pit_done = sim.mandatory_pit_done();
        // (online the physics goes on during a replay)
        if IN_REPLAY.load(std::sync::atomic::Ordering::Relaxed) {
            frame.in_replay(f32::from_bits(REPLAY_TIME_MULT.load(std::sync::atomic::Ordering::Relaxed)), car.transponder.last_lap, sim.online.is_some());
        }
        if self.writer.update(&mut self.graphics, car, &frame) {
            self.memory.graphics.write(&self.graphics.bytes);
        }
    }
}

impl StepSink for ShmSink {
    fn after_step(&mut self, sim: &GameSim, input: &StepInput) -> Result<(), String> {
        if !self.static_written || input.events & crate::input_file::event::REBUILD != 0 {
            self.memory.statics.write(&ShmSink::static_page(sim).bytes);
            self.static_written = true;
        }
        // what the game's physics thread does right after the step
        if let Some(page) = sim.car.physics_page() {
            let mut bytes = [0u8; PAGE_SIZE];
            for (chunk, word) in bytes.chunks_exact_mut(4).zip(&page.words) {
                chunk.copy_from_slice(&word.to_le_bytes());
            }
            self.memory.physics.write(&bytes);
        }
        // SharedMemoryWriter::update runs once per drawn frame of the main thread
        let frame = sim.referee.as_ref().map_or(sim.steps / 3, |referee| referee.frames);
        if frame != self.last_frame {
            self.last_frame = frame;
            self.write_graphics(sim);
        }
        Ok(())
    }

    fn set_paused(&mut self, paused: bool) {
        let replay = IN_REPLAY.load(std::sync::atomic::Ordering::Relaxed);
        self.status = if paused { AC_PAUSE } else { AC_LIVE };
        let g = &mut self.graphics;
        if replay && paused {
            // an offline replay stands the physics still: the page as GraphicsFrame::in_replay
            // makes it (shortcut: written once as the replay starts, not every frame; the
            // later time multiplier and the replayed laps do not reach it)
            g.set_i("status", AC_REPLAY);
            g.set_i("position", -1);
            g.set_f("sessionTimeLeft", -1.0);
            g.set_f("replayTimeMultiplier", f32::from_bits(REPLAY_TIME_MULT.load(std::sync::atomic::Ordering::Relaxed)));
            g.set_i("isEscMenuVisible", 0);
            self.memory.graphics.write(&g.bytes);
            return;
        }
        g.set_i("status", self.status);
        g.set_i("isEscMenuVisible", paused as i32);
        self.memory.graphics.write(&g.bytes);
    }

    fn finish(&mut self) -> Result<(), String> {
        // like the game's shutdown: status off, so readers stop counting
        let g = &mut self.graphics;
        g.set_i("status", AC_OFF);
        g.set_i("session", 0);
        g.set_i("timeLimitSessionLeft", -1);
        g.set_i("isEscMenuVisible", 0);
        self.memory.graphics.write(&g.bytes);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strings_are_cut_and_terminated() {
        let mut g = Page::graphics();
        g.set_w("currentTime", "0123456789abcdefgh");
        let at = g.offset("currentTime");
        let units: Vec<u16> = g.bytes[at..at + 30].chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
        assert_eq!(String::from_utf16_lossy(&units[..14]), "0123456789abcd");
        assert_eq!(units[14], 0);
        // the neighbour is untouched
        assert_eq!(g.get_i("session"), 0);
        g.set_w("currentTime", "1:02:003");
        assert_eq!(g.bytes[at + 16..at + 30], [0; 14]);
    }
}
