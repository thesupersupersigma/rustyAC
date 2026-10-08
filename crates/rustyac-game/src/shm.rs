// SPDX-License-Identifier: GPL-3.0-or-later

//! The game's shared memory, published by rustyAC: `Local\acpmf_physics`, `acpmf_graphics`
//! and `acpmf_static` with AC's own layouts (`docs/map/telemetry.md`), so `ac_telemetry.py`
//! and dashboard apps read rustyAC the way they read AC.
//!
//! The physics page is the car's own ([`PhysicsPageWriter`](rustyac_physics::car::PhysicsPageWriter),
//! bit-exact with the game's, including its 300 steps of warm-up) and is copied out after
//! every step. The graphics and static pages hold what a lone car on an endless road has to
//! say; fields about laps, sectors, opponents and the track are constant.
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

/// `sizeof(SPageFileGraphic)`, `sizeof(SPageFileStatic)`.
pub const GRAPHICS_SIZE: usize = 0x130;
pub const STATIC_SIZE: usize = 0x2ac;

/// `AC_STATUS`
pub const AC_OFF: i32 = 0;
pub const AC_LIVE: i32 = 2;
pub const AC_PAUSE: i32 = 3;

/// A member of a page: 32-bit integer, float, floats, or a `wchar_t` string of so many
/// characters (with its terminator).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    I,
    F,
    Fs(usize),
    W(usize),
}

/// `SPageFileGraphic` (4-byte packing).
pub const GRAPHICS_FIELDS: &[(&str, Kind)] = &[
    ("packetId", Kind::I),
    ("status", Kind::I),
    ("session", Kind::I),
    ("currentTime", Kind::W(15)),
    ("lastTime", Kind::W(15)),
    ("bestTime", Kind::W(15)),
    ("split", Kind::W(15)),
    ("completedLaps", Kind::I),
    ("position", Kind::I),
    ("iCurrentTime", Kind::I),
    ("iLastTime", Kind::I),
    ("iBestTime", Kind::I),
    ("sessionTimeLeft", Kind::F),
    ("distanceTraveled", Kind::F),
    ("isInPit", Kind::I),
    ("currentSectorIndex", Kind::I),
    ("lastSectorTime", Kind::I),
    ("numberOfLaps", Kind::I),
    ("tyreCompound", Kind::W(33)),
    ("replayTimeMultiplier", Kind::F),
    ("normalizedCarPosition", Kind::F),
    ("carCoordinates", Kind::Fs(3)),
    ("penaltyTime", Kind::F),
    ("flag", Kind::I),
    ("idealLineOn", Kind::I),
    ("isInPitLane", Kind::I),
    ("surfaceGrip", Kind::F),
    ("mandatoryPitDone", Kind::I),
    ("windSpeed", Kind::F),
    ("windDirection", Kind::F),
    ("timeLimitSessionLeft", Kind::I),
    ("isEscMenuVisible", Kind::I),
];

/// `SPageFileStatic` (4-byte packing).
pub const STATIC_FIELDS: &[(&str, Kind)] = &[
    ("smVersion", Kind::W(15)),
    ("acVersion", Kind::W(15)),
    ("numberOfSessions", Kind::I),
    ("numCars", Kind::I),
    ("carModel", Kind::W(33)),
    ("track", Kind::W(33)),
    ("playerName", Kind::W(33)),
    ("playerSurname", Kind::W(33)),
    ("playerNick", Kind::W(33)),
    ("sectorCount", Kind::I),
    ("maxTorque", Kind::F),
    ("maxPower", Kind::F),
    ("maxRpm", Kind::I),
    ("maxFuel", Kind::F),
    ("suspensionMaxTravel", Kind::Fs(4)),
    ("tyreRadius", Kind::Fs(4)),
    ("maxTurboBoost", Kind::F),
    ("deprecated_1", Kind::F),
    ("deprecated_2", Kind::F),
    ("penaltiesEnabled", Kind::I),
    ("aidFuelRate", Kind::F),
    ("aidTireRate", Kind::F),
    ("aidMechanicalDamage", Kind::F),
    ("aidAllowTyreBlankets", Kind::I),
    ("aidStability", Kind::F),
    ("aidAutoClutch", Kind::I),
    ("aidAutoBlip", Kind::I),
    ("hasDRS", Kind::I),
    ("hasERS", Kind::I),
    ("hasKERS", Kind::I),
    ("kersMaxJ", Kind::F),
    ("engineBrakeSettingsCount", Kind::I),
    ("ersPowerControllerCount", Kind::I),
    ("trackSPlineLength", Kind::F),
    ("trackConfiguration", Kind::W(33)),
    ("ersMaxJ", Kind::F),
    ("isTimedRace", Kind::I),
    ("hasExtraLap", Kind::I),
    ("carSkin", Kind::W(33)),
    ("reversedGridPositions", Kind::I),
    ("PitWindowStart", Kind::I),
    ("PitWindowEnd", Kind::I),
];

/// A page being filled: the members' offsets worked out from the layout.
#[derive(Clone, Debug)]
pub struct Page {
    offsets: Vec<(&'static str, Kind, usize)>,
    pub bytes: Vec<u8>,
}

impl Page {
    pub fn new(fields: &[(&'static str, Kind)]) -> Page {
        let mut offsets = Vec::new();
        let mut at = 0usize;
        for (name, kind) in fields {
            let (align, size) = match kind {
                Kind::I | Kind::F => (4, 4),
                Kind::Fs(count) => (4, 4 * count),
                Kind::W(count) => (2, 2 * count),
            };
            at = at.div_ceil(align) * align;
            offsets.push((*name, *kind, at));
            at += size;
        }
        Page { offsets, bytes: vec![0; at.div_ceil(4) * 4] }
    }

    pub fn graphics() -> Page {
        Page::new(GRAPHICS_FIELDS)
    }

    pub fn statics() -> Page {
        Page::new(STATIC_FIELDS)
    }

    pub fn offset(&self, name: &str) -> usize {
        self.offsets.iter().find(|(n, _, _)| *n == name).unwrap_or_else(|| panic!("the page has no member {name}")).2
    }

    fn member(&self, name: &str) -> (Kind, usize) {
        let (_, kind, offset) = self.offsets.iter().find(|(n, _, _)| *n == name).unwrap_or_else(|| panic!("the page has no member {name}"));
        (*kind, *offset)
    }

    pub fn set_i(&mut self, name: &str, value: i32) {
        let (kind, at) = self.member(name);
        assert_eq!(kind, Kind::I, "{name}");
        self.bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
    }

    pub fn set_f(&mut self, name: &str, value: f32) {
        let (kind, at) = self.member(name);
        assert_eq!(kind, Kind::F, "{name}");
        self.bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
    }

    pub fn set_fs(&mut self, name: &str, values: &[f32]) {
        let (kind, at) = self.member(name);
        assert_eq!(kind, Kind::Fs(values.len()), "{name}");
        for (k, value) in values.iter().enumerate() {
            self.bytes[at + 4 * k..at + 4 * k + 4].copy_from_slice(&value.to_le_bytes());
        }
    }

    /// A string: UTF-16, cut to fit, always terminated.
    pub fn set_w(&mut self, name: &str, text: &str) {
        let (kind, at) = self.member(name);
        let Kind::W(count) = kind else { panic!("{name} is not a string") };
        self.bytes[at..at + 2 * count].fill(0);
        for (k, unit) in text.encode_utf16().take(count - 1).enumerate() {
            self.bytes[at + 2 * k..at + 2 * k + 2].copy_from_slice(&unit.to_le_bytes());
        }
    }

    pub fn get_i(&self, name: &str) -> i32 {
        let at = self.offset(name);
        i32::from_le_bytes(self.bytes[at..at + 4].try_into().unwrap())
    }

    pub fn get_f(&self, name: &str) -> f32 {
        let at = self.offset(name);
        f32::from_le_bytes(self.bytes[at..at + 4].try_into().unwrap())
    }
}

/// A lap time the way AC's pages show it: `m:ss:mmm`.
pub fn time_string(ms: i32) -> String {
    let ms = ms.max(0);
    format!("{}:{:02}:{:03}", ms / 60_000, ms / 1000 % 60, ms % 1000)
}

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

/// Publishes after every step.
pub struct ShmSink {
    memory: SharedMemory,
    graphics: Page,
    /// `sharedMemories[1].packetId`
    graphics_packet: i32,
    status: i32,
    distance: f32,
    last_position: Option<[f32; 3]>,
    static_written: bool,
    drive_start_steps: u64,
}

impl ShmSink {
    pub fn new(memory: SharedMemory) -> ShmSink {
        ShmSink {
            memory,
            graphics: Page::graphics(),
            graphics_packet: 0,
            status: AC_LIVE,
            distance: 0.0,
            last_position: None,
            static_written: false,
            drive_start_steps: 0,
        }
    }

    /// The static page of this car (the game writes it once, with the first frame).
    pub fn static_page(sim: &GameSim) -> Page {
        let car = &sim.car.car;
        let mut page = Page::statics();
        page.set_w("smVersion", "1.7");
        page.set_w("acVersion", "rustyAC 0.1");
        page.set_i("numberOfSessions", 1);
        page.set_i("numCars", 1);
        // the car's folder name, however `--car` named it
        let model = crate::sim::car_name(&sim.data_path);
        page.set_w("carModel", &model);
        match &sim.track {
            Some(track) => {
                page.set_w("track", &track.name);
                // `Spline::length` of the AI line
                page.set_f("trackSPlineLength", track.length());
                page.set_i("penaltiesEnabled", 1);
            }
            None => page.set_w("track", "rustyac_flat"),
        }
        page.set_w("playerName", "Player");
        page.set_w("playerSurname", "");
        page.set_w("playerNick", "rustyAC");
        // the track's lap / sector lines (`Track::sectorsNormalizedPositions`)
        page.set_i("sectorCount", sim.track.as_ref().map(|t| t.sectors_normalized_positions.len() as i32).unwrap_or(1));
        if let Some(drivetrain) = &car.drivetrain {
            let engine = drivetrain.engine();
            // `Engine::maxTorqueNM`: the torque curve at the revs of its peak
            let ini = rustyac_physics::data::ini::IniReader::load(&sim.data_path.join("engine.ini"));
            if let Ok(curve) = ini.and_then(|ini| ini.get_curve("HEADER", "POWER_CURVE")) {
                page.set_f("maxTorque", curve.get_value(engine.get_max_torque_rpm()));
            }
            page.set_f("maxPower", engine.get_max_power_w());
            // `acEngine.defaultEngineLimiter`: nothing has moved the limiter when the page is written
            page.set_i("maxRpm", engine.get_limiter_rpm());
        }
        page.set_f("maxFuel", car.max_fuel as f32);
        let mut travel = [0.0; 4];
        let mut radius = [0.0; 4];
        for index in 0..4.min(car.tyres.len()) {
            let base = car.suspensions[index].base();
            travel[index] = base.bump_stop_up - base.bump_stop_dn;
            radius[index] = car.tyres[index].data.radius;
        }
        page.set_fs("suspensionMaxTravel", &travel);
        page.set_fs("tyreRadius", &radius);
        page.set_f("aidFuelRate", car.env.fuel_consumption_rate);
        page.set_f("aidTireRate", car.env.tyre_consumption_rate);
        page.set_f("aidMechanicalDamage", car.env.mechanical_damage_rate);
        page.set_i("aidAllowTyreBlankets", car.env.allow_tyre_blankets as i32);
        if let Some(aids) = &car.aids {
            page.set_f("aidStability", aids.base().stability_control.gain);
        }
        page.set_i("aidAutoClutch", car.autoclutch.use_auto_on_change as i32);
        page.set_i("aidAutoBlip", car.auto_blip.is_active as i32);
        if let Some(aero) = &car.aero {
            page.set_i("hasDRS", aero.base().drs.is_present as i32);
        }
        page.set_w("trackConfiguration", "");
        page.set_w("carSkin", "");
        page
    }

    fn write_graphics(&mut self, sim: &GameSim) {
        let car = &sim.car.car;
        let g = &mut self.graphics;
        // the game's counter goes up twice per page: the page shows 1, 3, 5 ...
        self.graphics_packet += 1;
        g.set_i("packetId", self.graphics_packet);
        self.graphics_packet += 1;
        g.set_i("status", self.status);
        if sim.track.is_some() {
            // SharedMemoryWriter::update's lap fields: the strings and the lap count from the
            // lap list (a lap with a cut is listed and is never the best), the integer times
            // straight from the car's transponder (whose best lap does not look at cuts)
            use rustyac_physics::track::timing::time_to_string;
            let tp = &car.transponder;
            let db = &sim.lap_db;
            let last = db.last_lap();
            // AC_PRACTICE 0, AC_HOTLAP 3
            g.set_i("session", if sim.setup.spawn == "hotlap" { 3 } else { 0 });
            g.set_w("currentTime", &time_to_string(tp.t as i32));
            g.set_w("lastTime", &time_to_string(last.time as i32));
            g.set_w("bestTime", &time_to_string(db.best_lap.time as i32));
            match db.current_splits.last() {
                // timeToSectorString: seconds and tenths
                Some(split) => g.set_w("split", &format!("{}.{}", split / 1000, split % 1000 / 100)),
                None => g.set_w("split", " "),
            }
            g.set_i("completedLaps", db.laps.len() as i32);
            g.set_i("iCurrentTime", tp.t as i32);
            g.set_i("iLastTime", tp.last_lap as i32);
            g.set_i("iBestTime", tp.best_lap as i32);
            g.set_i("currentSectorIndex", db.current_splits.len() as i32);
            let last_sector = match db.current_splits.last() {
                Some(split) => *split,
                None if last.time == 0 => 0,
                None => last.splits.last().copied().unwrap_or(0),
            };
            g.set_i("lastSectorTime", last_sector as i32);
            g.set_f("normalizedCarPosition", car.spline_locator_data.npos);
            g.set_i("isInPitLane", car.is_in_pit_lane() as i32);
            g.set_i("isInPit", car.is_in_pits() as i32);
        } else {
            g.set_i("session", 0);
            let ms = ((sim.steps - self.drive_start_steps) as f64 * 3.0) as i32;
            g.set_w("currentTime", &time_string(ms));
            g.set_w("lastTime", "-:--:---");
            g.set_w("bestTime", "-:--:---");
            g.set_w("split", "");
            g.set_i("iCurrentTime", ms);
        }
        g.set_i("position", 1);
        g.set_f("sessionTimeLeft", -1.0);
        let position = car.core.get_position(car.body);
        let position = [position.x, position.y, position.z];
        if let Some(last) = self.last_position {
            let d = [position[0] - last[0], position[1] - last[1], position[2] - last[2]];
            let moved = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
            // a teleport is not driving
            if moved.is_finite() && moved < 2.0 {
                self.distance += moved;
            }
        }
        self.last_position = Some(position);
        g.set_f("distanceTraveled", self.distance);
        if let Some(tyre) = car.tyres.first() {
            if let Some(compound) = tyre.compound_defs.get(tyre.current_compound_index as usize) {
                g.set_w("tyreCompound", &compound.name);
            }
        }
        g.set_f("replayTimeMultiplier", 1.0);
        g.set_fs("carCoordinates", &position);
        g.set_f("penaltyTime", car.get_penalty_time() as f32);
        g.set_f("surfaceGrip", car.env.dynamic_grip_level);
        g.set_f("windSpeed", car.env.wind_speed * 3.6);
        g.set_f("windDirection", car.env.wind_direction_deg);
        g.set_i("timeLimitSessionLeft", -1);
        g.set_i("isEscMenuVisible", (self.status == AC_PAUSE) as i32);
        self.memory.graphics.write(&g.bytes);
    }
}

impl StepSink for ShmSink {
    fn after_step(&mut self, sim: &GameSim, input: &StepInput) -> Result<(), String> {
        if !self.static_written || input.events & crate::input_file::event::REBUILD != 0 {
            self.memory.statics.write(&ShmSink::static_page(sim).bytes);
            self.static_written = true;
        }
        if sim.car.device.source.in_spawn_sequence() || input.device == crate::sim::DEVICE_SPAWN {
            // the clock of the drive starts when the driver gets the car
            self.drive_start_steps = sim.steps;
        }
        // what the game's physics thread does right after the step
        if let Some(page) = sim.car.physics_page() {
            let mut bytes = [0u8; PAGE_SIZE];
            for (chunk, word) in bytes.chunks_exact_mut(4).zip(&page.words) {
                chunk.copy_from_slice(&word.to_le_bytes());
            }
            self.memory.physics.write(&bytes);
        }
        self.write_graphics(sim);
        Ok(())
    }

    fn set_paused(&mut self, paused: bool) {
        self.status = if paused { AC_PAUSE } else { AC_LIVE };
        let g = &mut self.graphics;
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
    fn the_pages_have_the_game_s_layout() {
        // sizes and offsets from acs.pdb (docs/map/telemetry.md 5.7, 5.8)
        let g = Page::graphics();
        assert_eq!(g.bytes.len(), GRAPHICS_SIZE);
        for (name, offset) in [
            ("packetId", 0x000),
            ("session", 0x008),
            ("currentTime", 0x00c),
            ("lastTime", 0x02a),
            ("bestTime", 0x048),
            ("split", 0x066),
            ("position", 0x088),
            ("iBestTime", 0x094),
            ("sessionTimeLeft", 0x098),
            ("distanceTraveled", 0x09c),
            ("isInPit", 0x0a0),
            ("numberOfLaps", 0x0ac),
            ("tyreCompound", 0x0b0),
            ("replayTimeMultiplier", 0x0f4),
            ("carCoordinates", 0x0fc),
            ("penaltyTime", 0x108),
            ("flag", 0x10c),
            ("surfaceGrip", 0x118),
            ("windSpeed", 0x120),
            ("timeLimitSessionLeft", 0x128),
            ("isEscMenuVisible", 0x12c),
        ] {
            assert_eq!(g.offset(name), offset, "{name}");
        }
        let s = Page::statics();
        assert_eq!(s.bytes.len(), STATIC_SIZE);
        for (name, offset) in [
            ("smVersion", 0x000),
            ("acVersion", 0x01e),
            ("numberOfSessions", 0x03c),
            ("numCars", 0x040),
            ("carModel", 0x044),
            ("track", 0x086),
            ("playerName", 0x0c8),
            ("playerSurname", 0x10a),
            ("playerNick", 0x14c),
            ("sectorCount", 0x190),
            ("maxTorque", 0x194),
            ("maxPower", 0x198),
            ("maxRpm", 0x19c),
            ("maxFuel", 0x1a0),
            ("suspensionMaxTravel", 0x1a4),
            ("tyreRadius", 0x1b4),
            ("maxTurboBoost", 0x1c4),
            ("penaltiesEnabled", 0x1d0),
            ("aidFuelRate", 0x1d4),
            ("aidAllowTyreBlankets", 0x1e0),
            ("aidStability", 0x1e4),
            ("aidAutoClutch", 0x1e8),
            ("aidAutoBlip", 0x1ec),
            ("hasDRS", 0x1f0),
            ("kersMaxJ", 0x1fc),
            ("engineBrakeSettingsCount", 0x200),
            ("trackSPlineLength", 0x208),
            ("trackConfiguration", 0x20c),
            ("ersMaxJ", 0x250),
            ("isTimedRace", 0x254),
            ("carSkin", 0x25c),
            ("reversedGridPositions", 0x2a0),
            ("PitWindowEnd", 0x2a8),
        ] {
            assert_eq!(s.offset(name), offset, "{name}");
        }
    }

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
        assert_eq!(time_string(62_003), "1:02:003");
    }
}
