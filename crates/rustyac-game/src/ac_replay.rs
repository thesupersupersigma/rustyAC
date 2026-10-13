// SPDX-License-Identifier: GPL-3.0-or-later

//! AC's replays in the game: the port of `ReplayManager` / `ReplayRecorder`
//! (`rustyac_physics::replay`) fed from the simulation, its files, its keys.
//!
//! While a session runs, the manager takes a frame of every car after the physics steps at
//! the rate of `replay.ini`, as the game's does from `PhysicsAvatar::evOnStepCompleted`. A new
//! session autosaves the old one's replay into `replay/temp/`, a key saves into `replay/`:
//! rustyAC's own folders, never `Documents\Assetto Corsa\replay`. A file (rustyAC's or the
//! game's, version 15 or 16) is loaded as a session of its own: the file names the track and
//! the cars, the physics stands still, and every car's state comes from its recorder.
//!
//! This is not the `.ryin` recording (the inputs of a drive, simulated again); both exist.

use std::path::{Path, PathBuf};

use rustyac_physics::car_physics_state::CarPhysicsState;
use rustyac_physics::data::ini::IniReader;
use rustyac_physics::replay::{self, AvatarRecord, CarHeader, LeaderboardEntry, LocalTime, PlaybackAvatar, ReplayEvent, ReplayLoader, ReplayManager, ReplayRecorder, SaveInfo};
use rustyac_physics::track::Track;
use rustyac_physics::vecmath::Mat44f;

use crate::sim::GameSim;
use crate::view::CarView;

/// The extension of a replay file. (The game's own code adds none; the files of its replay
/// folder have this one.)
pub const EXTENSION: &str = "acreplay";

/// A car's state for the recorder: what the picture's state has, and the lap's numbers.
pub fn state_of(view: &CarView) -> CarPhysicsState {
    let mut state = view.physics_state();
    state.lap_time = view.lap.current_ms;
    state.last_lap = view.lap.last_ms;
    state.best_lap = view.lap.best_ms;
    state.lap_count = view.lap.laps;
    state.normalized_spline_position = view.lap.position;
    state
}

/// Is this file one of AC's replays (and not a `.ryin` recording, which is text)? The file
/// starts with its version as a 32-bit number.
pub fn is_ac_replay(path: &Path) -> bool {
    let mut head = [0u8; 4];
    let read = std::fs::File::open(path).and_then(|mut file| std::io::Read::read_exact(&mut file, &mut head));
    read.is_ok() && (1..=64).contains(&i32::from_le_bytes(head))
}

/// What a replay file says before its frames: the session it needs.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ReplayHeader {
    pub version: i32,
    pub record_interval_ms: f64,
    pub weather: String,
    pub track: String,
    pub layout: String,
    pub cars: Vec<CarHeader>,
    pub frames: i32,
}

/// A loader that builds nothing: it keeps what the file names.
struct HeaderLoader {
    header: ReplayHeader,
}

impl ReplayLoader for HeaderLoader {
    fn load_track(&mut self, track: &str, layout: &str) {
        self.header.track = track.to_string();
        self.header.layout = layout.to_string();
    }
    fn add_car(&mut self, car: &CarHeader) -> ReplayRecorder {
        self.header.cars.push(car.clone());
        ReplayRecorder::new(9999, 0)
    }
    fn apply_custom_weather(&mut self, weather: &str) {
        self.header.weather = weather.to_string();
    }
}

/// Reads a replay file's header (the whole file is read once to find every car).
pub fn read_header(bytes: &[u8]) -> Result<ReplayHeader, String> {
    let version = bytes.get(0..4).map(|b| i32::from_le_bytes(b.try_into().unwrap())).ok_or("the file is empty")?;
    let mut loader = HeaderLoader { header: ReplayHeader { version, ..ReplayHeader::default() } };
    let mut manager = ReplayManager::new(None, 30.0, Vec::new(), None, 0);
    manager.load(bytes, &mut loader)?;
    loader.header.record_interval_ms = manager.record_interval_ms;
    loader.header.frames = manager.recorded_frames;
    Ok(loader.header)
}

/// The day and time for a file's name: the local time on Windows, UTC elsewhere.
pub fn local_time() -> LocalTime {
    #[cfg(windows)]
    {
        #[link(name = "kernel32")]
        extern "system" {
            fn GetLocalTime(time: *mut [u16; 8]);
        }
        let mut t = [0u16; 8];
        // SAFETY: the function fills the 16 bytes of a SYSTEMTIME.
        unsafe { GetLocalTime(&mut t) };
        LocalTime { day: t[3] as u32, month: t[1] as u32, year: t[0] as u32 % 100, hour: t[4] as u32, minute: t[5] as u32, second: t[6] as u32 }
    }
    #[cfg(not(windows))]
    {
        let seconds = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
        let (days, rest) = (seconds / 86400, seconds % 86400);
        // (days since 1970 to a date: Howard Hinnant's civil_from_days)
        let z = days as i64 + 719_468;
        let era = z.div_euclid(146_097);
        let doe = z.rem_euclid(146_097);
        let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let day = doy - (153 * mp + 2) / 5 + 1;
        let month = if mp < 10 { mp + 3 } else { mp - 9 };
        let year = yoe + era * 400 + if month <= 2 { 1 } else { 0 };
        LocalTime { day: day as u32, month: month as u32, year: (year % 100) as u32, hour: (rest / 3600) as u32, minute: (rest % 3600 / 60) as u32, second: (rest % 60) as u32 }
    }
}

use rustyac_physics::replay::track_env::NoTrack;

/// The replay's keys (`Sim::onKeyDown` 0x14019a940: the commands `Pause Replay`, `FFWD`,
/// `REV`, `SLOWMO`, `Next Lap`, `Previous Lap`; the GUI's buttons for the rest).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ReplayKey {
    PlayPause,
    FastForward,
    Rewind,
    SlowMotion,
    NextFrame,
    NextLap,
    PreviousLap,
    Stop,
}

/// The replay of a session: the manager, and what it needs of the cars.
pub struct ReplayRig {
    pub manager: Option<ReplayManager>,
    /// `cfg/replay.ini`, when there is one
    ini: Option<IniReader>,
    /// rustyAC's replay folder
    pub folder: PathBuf,
    pub info: SaveInfo,
    max_fuel: Vec<f64>,
    lap_counts: Vec<u32>,
    new_sessions: u32,
    /// the lighting's sun angle, for the scene's frames
    pub sun_angle: f32,
    session_type: i32,
    /// the cars' long-lived states of the playback, and what goes with them
    pub states: Vec<CarPhysicsState>,
    avatars: Vec<PlaybackAvatar>,
    /// the loose objects' matrices of the last playback frame
    pub objects: Vec<Mat44f>,
    /// the slow-motion level of the `SLOWMO` key (the GUI's spinner: 2 to 12)
    pub slow_motion_level: f32,
    /// lines for the console: what was saved, what went wrong
    pub messages: Vec<String>,
    /// files written, in order
    pub saved: Vec<PathBuf>,
    /// the focused car, for the lap keys
    pub focused_car: usize,
}

impl ReplayRig {
    pub fn new(folder: PathBuf, ini: Option<IniReader>, sun_angle: f32, session_type: i32) -> ReplayRig {
        ReplayRig {
            manager: None,
            ini,
            folder,
            info: SaveInfo::default(),
            max_fuel: Vec::new(),
            lap_counts: Vec::new(),
            new_sessions: 0,
            sun_angle,
            session_type,
            states: Vec::new(),
            avatars: Vec::new(),
            objects: Vec::new(),
            slow_motion_level: 2.0,
            messages: Vec::new(),
            saved: Vec::new(),
            focused_car: 0,
        }
    }

    /// `engine.ini [ENGINE_DATA] LIMITER` of a car (9999 when the file cannot be read), as
    /// `ReplayRecorder::ReplayRecorder` reads it.
    fn limiter(data: &Path) -> i32 {
        match IniReader::load(&data.join("engine.ini")) {
            Ok(ini) => ini.get_int("ENGINE_DATA", "LIMITER").unwrap_or(0),
            Err(_) => 9999,
        }
    }

    /// The manager of a live session, made at its first step: a recorder for every car.
    fn start(&mut self, sim: &GameSim, views: &[CarView]) {
        let infos: Vec<crate::sim::CarInfo> = std::iter::once(sim.car_info()).chain(sim.opponent_infos()).chain(sim.remote_infos()).collect();
        let mut recorders = Vec::new();
        self.info = SaveInfo { weather: "3_clear".to_string(), track: sim.setup.track.clone(), layout: sim.setup.layout.clone(), ..SaveInfo::default() };
        let names: Vec<String> = sim.setup.referee.as_ref().map(|referee| referee.driver_names.clone()).unwrap_or_default();
        for (i, view) in views.iter().enumerate() {
            let info = infos.get(i);
            let folder = info.map(|info| Path::new(&info.name).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| info.name.clone())).unwrap_or_default();
            recorders.push(ReplayRecorder::new(info.map_or(9999, |info| Self::limiter(&info.data_path)), view.wing_count as usize));
            let skin = match (i, info.and_then(|info| info.skin.clone())) {
                (0, _) => sim.setup.player_skin.clone().unwrap_or_default(),
                (_, skin) => skin.unwrap_or_default(),
            };
            self.info.cars.push(CarHeader { unix_name: folder, driver_name: names.get(i).cloned().unwrap_or_else(|| if i == 0 { "Player".to_string() } else { format!("Driver {i}") }), nation_code: String::new(), team: String::new(), skin });
            self.info.wings.push(view.wing_count as u32);
            self.info.guids.push(i as i32);
            self.max_fuel.push(view.max_fuel);
        }
        self.lap_counts = views.iter().map(|view| view.lap.laps).collect();
        let objects = sim.world_core().track_objects.len();
        let manager = ReplayManager::new(self.ini.as_ref(), 30.0, recorders, Some(objects), self.session_type);
        self.messages.extend(manager.log.iter().cloned());
        self.manager = Some(manager);
        self.new_sessions = sim.new_sessions;
    }

    fn leaderboard(&self, views: &[CarView], now: f64) -> Vec<LeaderboardEntry> {
        views.iter().enumerate().map(|(i, view)| LeaderboardEntry { car: i as i32, total_time: now, best_lap: view.lap.best_ms as f64, laps: view.lap.laps as i32, is_race_mode: self.session_type == 3, is_black_flagged: false, has_completed_last_lap: false }).collect()
    }

    /// After a physics step (`ReplayManager::onStepCompleted`, the laps' handlers, and the
    /// autosave of a new session).
    pub fn after_step(&mut self, sim: &GameSim) {
        let pt = sim.clock_ms();
        if let Some(manager) = &self.manager {
            if manager.has_loaded_replay {
                return;
            }
            // a new session: the old one's replay is saved and the recording starts again
            if sim.new_sessions != self.new_sessions {
                self.new_sessions = sim.new_sessions;
                self.autosave();
                if let Some(manager) = &mut self.manager {
                    manager.on_new_session(self.session_type);
                }
                self.lap_counts.iter_mut().for_each(|count| *count = 0);
            }
        }
        if let Some(manager) = &self.manager {
            let dt = pt - manager.last_record_time;
            if dt < manager.record_interval_ms {
                return;
            }
        }
        let views = CarView::capture_world(sim, sim.sim_seconds());
        if self.manager.is_none() {
            self.start(sim, &views);
        }
        let board = self.leaderboard(&views, pt);
        let Some(manager) = &mut self.manager else { return };
        if manager.wants_first_leaderboard() {
            manager.on_lap_leaderboard(board.clone());
        }
        for (i, view) in views.iter().enumerate() {
            if self.lap_counts.len() <= i {
                self.lap_counts.push(0);
            }
            if view.lap.laps > self.lap_counts[i] {
                self.lap_counts[i] = view.lap.laps;
                manager.on_lap_completed(i, view.lap.last_ms, view.lap.laps, view.lap.last_valid);
                manager.on_lap_leaderboard(board.clone());
            }
        }
        let states: Vec<CarPhysicsState> = views.iter().map(state_of).collect();
        // (the car's own copy of its state is the state itself here, and the body's dirt, which
        // lives in the picture at the picture's pace, is not recorded: a recording must not
        // depend on how fast the pictures come)
        let avatars: Vec<AvatarRecord> = views
            .iter()
            .zip(&states)
            .enumerate()
            .map(|(i, (view, state))| AvatarRecord {
                actions_state: state.actions_state as u32,
                status_bytes: state.status_bytes,
                kers_is_charging: state.kers_is_charging,
                max_fuel: self.max_fuel.get(i).copied().unwrap_or(view.max_fuel),
                car_dirt: None,
                connected: !view.disconnected,
                wing_angles: &view.wing_angles[..view.wing_count as usize],
            })
            .collect();
        let world = sim.world_core();
        let objects: Vec<Option<Mat44f>> = world.track_objects.iter().map(|object| Some(world.get_world_matrix(object.body))).collect();
        manager.on_step_completed(pt, &states, &avatars, self.sun_angle, &objects);
    }

    /// The file name of a save by hand: `AC_DDMMYY-hhmmss_<car>_<track>_<layout>` with the
    /// session's letter, as the game's autosave names its files.
    pub fn file_name(&self, suffix: &str) -> String {
        let car = self.info.cars.first().map_or("", |car| car.unix_name.as_str());
        replay::get_replay_file_name(&local_time(), suffix, car, &self.info.track, &self.info.layout)
    }

    /// `ReplayManager::getAutoFileName`: the name the replay bar's save box proposes.
    pub fn auto_file_name(&self) -> String {
        let car = self.info.cars.first().map_or("", |car| car.unix_name.as_str());
        replay::get_auto_file_name(&local_time(), car, &self.info.track, &self.info.layout)
    }

    /// `ReplayManager::save`: into rustyAC's replay folder (`temp/` below it for an
    /// autosave), never into a folder of the game.
    pub fn save(&mut self, name: &str, temp: bool) -> Result<PathBuf, String> {
        let manager = self.manager.as_mut().ok_or("there is no replay yet")?;
        let text = self.folder.to_string_lossy().to_lowercase().replace('\\', "/");
        if text.contains("documents/assetto corsa") || text.contains("/assettocorsa/") {
            return Err(format!("{} is one of the game's folders: rustyAC saves its replays in a folder of its own", self.folder.display()));
        }
        let relative = manager.save_name(name, temp).ok_or_else(|| format!("{name:?} is not a file name"))?;
        let bytes = manager.save_bytes(&self.info)?;
        let path = self.folder.join(format!("{relative}.{EXTENSION}"));
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
        }
        std::fs::write(&path, &bytes).map_err(|e| format!("{}: {e}", path.display()))?;
        self.saved.push(path.clone());
        Ok(path)
    }

    /// The replay as a file of the caller's choice (for checks: `--save-replay`).
    pub fn save_to(&mut self, path: &Path) -> Result<usize, String> {
        let manager = self.manager.as_mut().ok_or("there is no replay yet")?;
        let bytes = manager.save_bytes(&self.info)?;
        if let Some(parent) = path.parent().filter(|parent| !parent.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
        }
        std::fs::write(path, &bytes).map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(bytes.len())
    }

    /// `ReplayManager::shutdown` 0x14015b1c0: with autosave on, the session's replay goes to
    /// `temp/`; with it off, the game writes the file `cr` (the "current replay") at every
    /// exit.
    pub fn shutdown(&mut self) {
        let Some(manager) = self.manager.as_ref().filter(|manager| !manager.has_loaded_replay && manager.recorded_frames > 0) else { return };
        if manager.auto_save_enabled {
            self.autosave();
        } else {
            match self.save("cr", false) {
                Ok(path) => self.messages.push(format!("Saving replay: {}", path.display())),
                Err(message) => self.messages.push(format!("the replay was not saved: {message}")),
            }
        }
    }

    /// The autosave of the closure 0x1401546d0 and of `ReplayManager::shutdown`: the file of
    /// the session that ends, after the oldest file of its kind made room.
    pub fn autosave(&mut self) {
        let Some((short_name, size)) = self.manager.as_ref().and_then(|manager| manager.autosave()).map(|data| (data.short_name.clone(), data.size)) else { return };
        let temp = self.folder.join("temp");
        let files: Vec<String> = std::fs::read_dir(&temp).map(|dir| dir.filter_map(|e| e.ok()).filter(|e| e.path().is_file()).map(|e| e.file_name().to_string_lossy().into_owned()).filter(|name| replay::autosave_pattern_matches(name, &short_name)).collect()).unwrap_or_default();
        if let Some(old) = replay::clear_saved(files, size) {
            let _ = std::fs::remove_file(temp.join(old));
        }
        let name = self.file_name(&short_name);
        match self.save(&name, true) {
            Ok(path) => self.messages.push(format!("Saving replay: {}", path.display())),
            Err(message) => self.messages.push(format!("the replay was not saved: {message}")),
        }
    }

    /// A file as a session: the manager of `RaceManager::initReplay`, filled by
    /// `ReplayManager::load`. `cars`: each car's data folder (for its limiter), wing count
    /// and tank size, in the file's order.
    pub fn load(&mut self, bytes: &[u8], cars: &[(PathBuf, usize, f64)]) -> Result<ReplayHeader, String> {
        struct Loader<'a> {
            cars: &'a [(PathBuf, usize, f64)],
            header: ReplayHeader,
        }
        impl ReplayLoader for Loader<'_> {
            fn load_track(&mut self, track: &str, layout: &str) {
                self.header.track = track.to_string();
                self.header.layout = layout.to_string();
            }
            fn add_car(&mut self, car: &CarHeader) -> ReplayRecorder {
                let made = self.cars.get(self.header.cars.len());
                self.header.cars.push(car.clone());
                ReplayRecorder::new(made.map_or(9999, |car| ReplayRig::limiter(&car.0)), made.map_or(0, |car| car.1))
            }
            fn apply_custom_weather(&mut self, weather: &str) {
                self.header.weather = weather.to_string();
            }
        }
        let version = bytes.get(0..4).map(|b| i32::from_le_bytes(b.try_into().unwrap())).ok_or("the file is empty")?;
        let mut loader = Loader { cars, header: ReplayHeader { version, ..ReplayHeader::default() } };
        // (no cars yet: the ring's size is that of one car, as in the game; nothing records)
        let mut manager = ReplayManager::new(self.ini.as_ref(), 30.0, Vec::new(), None, 0);
        manager.load(bytes, &mut loader)?;
        let mut header = loader.header;
        header.record_interval_ms = manager.record_interval_ms;
        header.frames = manager.recorded_frames;
        self.info = SaveInfo { weather: header.weather.clone(), track: header.track.clone(), layout: header.layout.clone(), cars: header.cars.clone(), wings: cars.iter().map(|car| car.1 as u32).collect(), guids: (0..header.cars.len() as i32).collect() };
        self.max_fuel = (0..header.cars.len()).map(|i| cars.get(i).map_or(1.0, |car| car.2)).collect();
        self.manager = Some(manager);
        Ok(header)
    }

    /// Is the replay showing?
    pub fn in_replay_mode(&self) -> bool {
        self.manager.as_ref().is_some_and(|manager| manager.replay_mode)
    }

    /// `ReplayManager::startReplayMode` (0: from the first frame).
    pub fn start_replay_mode(&mut self) -> bool {
        let Some(manager) = &mut self.manager else { return false };
        if manager.recorded_frames < 3 {
            return false;
        }
        manager.start_replay_mode(0);
        let cars = manager.recorders.len();
        self.states = vec![CarPhysicsState::at_origin(); cars];
        self.avatars = (0..cars).map(|i| PlaybackAvatar { max_fuel: self.max_fuel.get(i).copied().unwrap_or(1.0), has_dynamic_effects: true, dirt_level: None, car_node_active: true }).collect();
        true
    }

    /// `ReplayManager::stopReplayMode` (a loaded replay has nothing to go back to).
    pub fn stop_replay_mode(&mut self) {
        if let Some(manager) = self.manager.as_mut().filter(|manager| !manager.has_loaded_replay) {
            manager.stop_replay_mode();
        }
    }

    /// A key of the replay.
    pub fn key(&mut self, key: ReplayKey) {
        let (focused, level) = (self.focused_car, self.slow_motion_level);
        let Some(manager) = self.manager.as_mut().filter(|manager| manager.replay_mode) else { return };
        match key {
            ReplayKey::PlayPause => {
                if manager.status == replay::PLAY || manager.status == replay::SLOW_MOTION {
                    manager.pause();
                } else {
                    manager.play();
                }
            }
            ReplayKey::FastForward => {
                if manager.time_mult < 6.0 {
                    manager.fast_forward(1, false);
                }
            }
            ReplayKey::Rewind => {
                if manager.time_mult > -6.0 {
                    manager.rewind(1, false);
                }
            }
            ReplayKey::SlowMotion => {
                if manager.status == replay::SLOW_MOTION {
                    manager.play();
                } else {
                    manager.slow_motion(level);
                }
            }
            ReplayKey::NextFrame => manager.next_frame(),
            ReplayKey::NextLap => manager.next_lap(focused),
            ReplayKey::PreviousLap => manager.previous_lap(focused),
            ReplayKey::Stop => manager.stop(),
        }
    }

    /// One picture frame of the replay (`ReplayManager::updatePlayState`, then every car's
    /// state from its recorder): the states are in [`ReplayRig::states`], the loose objects'
    /// matrices in [`ReplayRig::objects`]. The answer: the manager's events of the frame.
    pub fn frame(&mut self, dt: f32, track: Option<&Track>) -> Vec<ReplayEvent> {
        let Some(manager) = self.manager.as_mut().filter(|manager| manager.is_active) else { return Vec::new() };
        self.objects = manager.update_play_state(dt);
        for car in 0..manager.recorders.len().min(self.states.len()) {
            match track {
                Some(track) => manager.get_physics_state(car, &mut self.states[car], &mut self.avatars[car], track),
                None => manager.get_physics_state(car, &mut self.states[car], &mut self.avatars[car], &NoTrack),
            }
        }
        std::mem::take(&mut manager.events)
    }

    /// Every car's wing angles at the cursor, and whether the car shows (`connected`).
    pub fn wings(&self, car: usize) -> Vec<f32> {
        self.manager.as_ref().and_then(|manager| manager.recorders.get(car)).map(|recorder| recorder.get_wing_state()).unwrap_or_default()
    }

    pub fn car_shows(&self, car: usize) -> bool {
        self.avatars.get(car).is_none_or(|avatar| avatar.car_node_active)
    }

    /// The body's dirt of a car at the cursor (`DynamicCarEffects::setDirtLevel`).
    pub fn dirt_level(&self, car: usize) -> Option<f32> {
        self.avatars.get(car).and_then(|avatar| avatar.dirt_level)
    }

    /// The line of text about the replay: where it is, how fast it runs.
    pub fn status_line(&self) -> String {
        let Some(manager) = &self.manager else { return String::new() };
        let seconds = |frames: i32| frames as f64 * manager.record_interval_ms * 0.001;
        let status = match manager.status {
            replay::PLAY => "play".to_string(),
            replay::PAUSE => "pause".to_string(),
            replay::STOP => "stop".to_string(),
            replay::REWIND => format!("rewind x{:.0}", -manager.time_mult),
            replay::FAST_FORWARD => format!("fast forward x{:.0}", manager.time_mult),
            replay::SLOW_MOTION => format!("slow motion 1/{:.0}", manager.slow_mo_level),
            replay::SINGLE_FRAME => "frame by frame".to_string(),
            other => format!("status {other}"),
        };
        format!("REPLAY {:.1} s / {:.1} s  {status}  (frame {} of {})", seconds(manager.current_frame.rem_euclid(manager.recorded_frames.max(1))), seconds(manager.recorded_frames), manager.current_frame.rem_euclid(manager.recorded_frames.max(1)), manager.recorded_frames)
    }
}

/// A car's view for the camera, the HUD and the sound's listener, from a replay's state: the
/// body, the wheels, the speed, the engine, the pedals, the lap. (The picture itself is drawn
/// from the state.)
pub fn view_of(base: &CarView, state: &CarPhysicsState) -> CarView {
    let mut view = *base;
    view.body = state.world_matrix.m;
    for i in 0..4 {
        view.hubs[i] = state.suspension_matrix[i].m;
        view.tyre_matrix[i] = state.tyre_matrix[i].m;
    }
    view.wheel_angular_speed = state.wheel_angular_speed;
    view.steer_deg = state.steer;
    view.rpm = state.engine_rpm;
    view.gas = state.gas;
    view.brake = state.brake;
    view.clutch = state.clutch;
    view.gear = state.gear;
    view.speed_ms = state.speed;
    view.speed_kmh = state.speed * 3.6;
    view.fuel = state.fuel;
    view.damage = state.damage_zone_level;
    view.acc_g = state.acc_g;
    view.render.velocity = state.velocity;
    view.lap.current_ms = state.lap_time;
    view.lap.last_ms = state.last_lap;
    view.lap.best_ms = state.best_lap;
    view.lap.laps = state.lap_count;
    view.lap.position = state.normalized_spline_position;
    view
}
