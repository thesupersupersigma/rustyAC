// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! `ReplayManager` (0x198 bytes): the replay's timeline.
//!
//! The constructor 0x140153130 (`cfg/replay.ini`: the rate, the memory budget, autosave),
//! `onStepCompleted` 0x140158b50 (a frame of every car and of the scene whenever the
//! recording interval has passed), `onLapCompleted` 0x140158a00, the leaderboard snapshots,
//! `save` 0x140159d10 and `load` 0x140157680 (format version 16; 15 is converted; below 15
//! is refused), the file names (`getReplayFileName` 0x140157260, `getAutoFileName`
//! 0x1401568b0), autosave (the closure 0x1401546d0, `clearSaved` 0x1401559f0, `shutdown`
//! 0x14015b1c0, `resetReplay` 0x1401597d0), and the playback: `startReplayMode`
//! 0x14015b480, `stopReplayMode` 0x14015b800, `play` 0x1401591f0, `pause` 0x140159070,
//! `stop` 0x14015b6f0, `rewind` 0x140159b40, `fastForward` 0x140156780, `slowMotion`
//! 0x14015b3b0, `nextFrame` 0x140158900, `nextLap` 0x140158930, `previousLap` 0x1401592a0,
//! `photoMode` 0x140159120, `setNormalizedPosition` 0x14015b0b0, `setCutInPerc` 0x14015af30,
//! `setCutOutPerc` 0x14015aff0, `updatePlayState` 0x14015b9a0, `updateReplayRecorders`
//! 0x14015bc90, `updateReplayAudio` 0x14015bc00.
//!
//! Nothing here touches a disk, a camera or a sound engine: the files are byte vectors, and
//! what the game tells its other parts is a list of [`ReplayEvent`]s for the host to act on.
//! The game's quirks are kept (the scene frames of a file are not rotated or cut like the
//! cars' frames; `rewind` and `fastForward` by frames clamp to 0..1; and so on).

// The comparisons and limits are written as the machine code has them (what a NaN does).
#![allow(clippy::neg_cmp_op_on_partial_ord, clippy::manual_clamp, clippy::needless_range_loop, clippy::too_many_arguments)]

use super::half::{float_to_half, half_to_float};
use super::recorder::{create_from_euler_safe, AvatarRecord, PlaybackAvatar, PlaybackTrack, ReplayRecorder};
use crate::car_physics_state::CarPhysicsState;
use crate::data::ini::IniReader;
use crate::dxmath::mat44f_lerp;
use crate::vecmath::Mat44f;

/// `eReplayStatus`.
pub const PLAY: i32 = 0;
pub const PAUSE: i32 = 1;
pub const STOP: i32 = 2;
pub const REWIND: i32 = 3;
pub const FAST_FORWARD: i32 = 4;
pub const SLOW_MOTION: i32 = 5;
pub const MODE_ENTER: i32 = 6;
pub const MODE_EXIT: i32 = 7;
pub const POSITION_SET: i32 = 9;
pub const PHOTO_MODE: i32 = 11;
pub const SINGLE_FRAME: i32 = 12;

/// What the manager tells the rest of the game, in order.
#[derive(Clone, Debug, PartialEq)]
pub enum ReplayEvent {
    /// `Sim::evOnReplayStatusChanged` (`OnReplayStatusChanged`)
    StatusChanged { status: i32, time_mult: f32, slow_motion_level: f32 },
    /// `evOnReplayRewind`
    Rewind(i32),
    /// `evOnReplayStarted`, `evOnReplayStopped`
    Started,
    Stopped,
    /// `evDidFinishedSavingReplay`
    Saved(bool),
    /// `updateReplayAudio`: `AudioEngine::start` (true) or `stop`
    Audio(bool),
}

/// `CompressedTransform` (12 bytes): three angles and a position as 16-bit floats.
pub type CompressedTransform = [u16; 6];

/// `FrameInformation`: the scene's part of a frame.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FrameInformation {
    /// the sun's angle as a 16-bit float
    pub sun_angle: u16,
    /// the newest leaderboard snapshot when the frame was taken
    pub lb_index: i16,
    pub track_objects: Vec<CompressedTransform>,
}

/// `LeaderboardEntry`. `car` is the car's number (the game holds its `CarAvatar`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LeaderboardEntry {
    pub car: i32,
    pub total_time: f64,
    pub best_lap: f64,
    pub laps: i32,
    pub is_race_mode: bool,
    pub is_black_flagged: bool,
    pub has_completed_last_lap: bool,
}

/// `ReplayLap`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ReplayLap {
    pub time: u32,
    pub is_valid: bool,
}

/// `ReplayAutosave`: how many files of a session type are kept (-1: all, 0: none are made)
/// and the letter in their names.
#[derive(Clone, Debug, PartialEq)]
pub struct ReplayAutosave {
    pub size: i32,
    pub short_name: String,
}

/// A car's lines in a replay file.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CarHeader {
    pub unix_name: String,
    pub driver_name: String,
    pub nation_code: String,
    pub team: String,
    pub skin: String,
}

/// What `save` reads of the session.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SaveInfo {
    /// `WeatherManager::getLastWeatherName`
    pub weather: String,
    /// the track's name and `TrackAvatar::getConfig`
    pub track: String,
    pub layout: String,
    pub cars: Vec<CarHeader>,
    /// each car's wing count (`CarAvatar::wingsStatus`) and `CarAvatar::guid`
    pub wings: Vec<u32>,
    pub guids: Vec<i32>,
}

/// The session a file is loaded into: `Sim::loadTrack`, `Sim::addCar` with the car's
/// recorder as `CarAvatar::initCommonPostPhysics` makes it, `Sim::applyCustomWeather`.
pub trait ReplayLoader {
    fn load_track(&mut self, track: &str, layout: &str);
    fn add_car(&mut self, car: &CarHeader) -> ReplayRecorder;
    fn apply_custom_weather(&mut self, weather: &str);
    /// `CarAvatar::isBlackFlagged` of a car
    fn is_black_flagged(&self, _car: i32) -> bool {
        false
    }
}

/// The day and time in a file's name (`localtime`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LocalTime {
    pub day: u32,
    pub month: u32,
    /// the year's last two digits
    pub year: u32,
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
}

/// `ReplayManager::getReplayFileName` 0x140157260: `AC_DDMMYY-hhmmss_<suffix>_<car>_<track>_<layout>`.
pub fn get_replay_file_name(t: &LocalTime, suffix: &str, car: &str, track: &str, layout: &str) -> String {
    format!("AC_{:02}{:02}{:02}-{:02}{:02}{:02}_{suffix}_{car}_{track}_{layout}", t.day, t.month, t.year, t.hour, t.minute, t.second)
}

/// `ReplayManager::getAutoFileName` 0x1401568b0: `<car>_<track>_<layout>_DDMMYY-hhmmss`.
pub fn get_auto_file_name(t: &LocalTime, car: &str, track: &str, layout: &str) -> String {
    format!("{car}_{track}_{layout}_{:02}{:02}{:02}-{:02}{:02}{:02}", t.day, t.month, t.year, t.hour, t.minute, t.second)
}

/// `Path::doesFilenameContainsIllegalChar` 0x140230520.
pub fn has_illegal_char(name: &str) -> bool {
    name.chars().any(|c| "\\/:?\"<>|*%$".contains(c))
}

/// The pattern `clearSaved` lists `replay/temp` with: `AC_*<short name>_*.*`.
pub fn autosave_pattern_matches(file: &str, short_name: &str) -> bool {
    fn wild(pattern: &[char], text: &[char]) -> bool {
        match pattern.split_first() {
            None => text.is_empty(),
            Some(('*', rest)) => (0..=text.len()).any(|skip| wild(rest, &text[skip..])),
            Some((c, rest)) => text.first().is_some_and(|t| t.eq_ignore_ascii_case(c)) && wild(rest, &text[1..]),
        }
    }
    let pattern: Vec<char> = format!("AC_*{short_name}_*.*").chars().collect();
    let text: Vec<char> = file.chars().collect();
    wild(&pattern, &text)
}

/// `ReplayManager::clearSaved` 0x1401559f0: of the files that match a session type's
/// pattern, the one that is deleted before a new one is saved (the first by name, when
/// there are as many as may be kept).
pub fn clear_saved(mut files: Vec<String>, keep: i32) -> Option<String> {
    if keep < 0 || files.is_empty() || (keep as usize) > files.len() {
        return None;
    }
    files.sort();
    files.into_iter().next()
}

/// `ksEncodeUtf8` 0x140238f10: UTF-8 of each 16-bit unit by itself.
fn encode_utf8(text: &str) -> Vec<u8> {
    let mut out = Vec::new();
    for unit in text.encode_utf16() {
        let c = unit as u32;
        if c < 0x80 {
            out.push(c as u8);
        } else if c < 0x800 {
            out.extend([0xc0 | (c >> 6) as u8, 0x80 | (c & 0x3f) as u8]);
        } else {
            out.extend([0xe0 | (c >> 12) as u8, 0x80 | ((c >> 6) & 0x3f) as u8, 0x80 | (c & 0x3f) as u8]);
        }
    }
    out
}

/// `ksDecodeUtf8`.
fn decode_utf8(bytes: &[u8]) -> String {
    let mut units: Vec<u16> = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i] as u32;
        let (len, mut c) = if b < 0x80 {
            (1, b)
        } else if b >> 5 == 0x6 {
            (2, b & 0x1f)
        } else if b >> 4 == 0xe {
            (3, b & 0x0f)
        } else if b >> 3 == 0x1e {
            (4, b & 0x07)
        } else {
            (1, 0xfffd)
        };
        for k in 1..len {
            c = (c << 6) | (bytes.get(i + k).copied().unwrap_or(0x80) as u32 & 0x3f);
        }
        i += len;
        if c >= 0x1_0000 {
            let c = c - 0x1_0000;
            units.push(0xd800 | (c >> 10) as u16);
            units.push(0xdc00 | (c & 0x3ff) as u16);
        } else {
            units.push(c as u16);
        }
    }
    String::from_utf16_lossy(&units)
}

fn put_string(out: &mut Vec<u8>, text: &str) {
    let bytes = encode_utf8(text);
    out.extend_from_slice(&(bytes.len() as i32).to_le_bytes());
    out.extend_from_slice(&bytes);
}

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn take(&mut self, n: usize) -> Result<&[u8], String> {
        let piece = self.bytes.get(self.at..self.at.checked_add(n).ok_or("a length that is too large")?).ok_or("the replay file ends early")?;
        self.at += n;
        Ok(piece)
    }
    fn i32(&mut self) -> Result<i32, String> {
        Ok(i32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn u16(&mut self) -> Result<u16, String> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }
    fn f32(&mut self) -> Result<f32, String> {
        Ok(f32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn f64(&mut self) -> Result<f64, String> {
        Ok(f64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }
    fn string(&mut self) -> Result<String, String> {
        let n = self.i32()?;
        let n = usize::try_from(n).map_err(|_| "a text of negative length")?;
        Ok(decode_utf8(self.take(n)?))
    }
}

/// `ReplayManager`.
#[derive(Clone)]
pub struct ReplayManager {
    /// `GameObject::isActive`: `Sim::update` calls `updatePlayState` while it is set
    pub is_active: bool,
    pub has_loaded_replay: bool,
    pub status: i32,
    pub last_filename: String,
    /// one recorder per car, in the cars' order
    pub recorders: Vec<ReplayRecorder>,
    /// each car's laps (the game's map from the car to its list)
    pub replay_laps: Vec<Vec<ReplayLap>>,
    pub old_status: i32,
    pub time_mult: f32,
    pub slow_mo_counter: f32,
    pub slow_mo_level: f32,
    pub play_counter: f64,
    pub replay_mode: bool,
    pub recorded_frames: i32,
    pub current_recording_index: i32,
    pub last_record_time: f64,
    pub max_frames: i32,
    pub min_frames: i32,
    pub cut_out: i32,
    pub cut_in: i32,
    pub max_size_mb: f32,
    pub current_frame: i32,
    pub current_session_type: i32,
    /// by session type (1 practice, 2 qualify, 3 race, 4 hotlap, 5 time attack, 6 drift, 7 drag)
    pub auto_save_data: Vec<(i32, ReplayAutosave)>,
    pub auto_save_enabled: bool,
    /// `ReplayRecorder::recordIntervalMS` (a static of the game: 30 until a file or the ini
    /// says otherwise)
    pub record_interval_ms: f64,
    /// the global `frameInformation`
    pub frame_information: Vec<FrameInformation>,
    /// the global `replayLeaderboards`
    pub replay_leaderboards: Vec<Vec<LeaderboardEntry>>,
    /// `getPhysicsState`'s static: the interval in seconds as its first call found it
    pub interval_static: Option<f32>,
    pub events: Vec<ReplayEvent>,
    /// what the game prints
    pub log: Vec<String>,
    /// the places in the last saved file of bytes the game never sets (the three bytes after
    /// a lap's flag are whatever its stack held; they are 0 here)
    pub save_undefined: Vec<usize>,
}

impl ReplayManager {
    /// `ReplayManager::ReplayManager` 0x140153130. `ini`: `cfg/replay.ini` (`None`: it cannot
    /// be read); `record_interval_ms`: the static as it is (30 at the program's start);
    /// `recorders`: every car's; `track_objects`: how many loose objects the track has
    /// (`None`: no track yet); `session_type`: the current session's.
    pub fn new(ini: Option<&IniReader>, record_interval_ms: f64, recorders: Vec<ReplayRecorder>, track_objects: Option<usize>, session_type: i32) -> ReplayManager {
        let cars = recorders.len();
        let mut m = ReplayManager {
            is_active: false,
            has_loaded_replay: false,
            status: STOP,
            last_filename: String::new(),
            recorders,
            replay_laps: vec![Vec::new(); cars],
            old_status: STOP,
            time_mult: 1.0,
            slow_mo_counter: 0.0,
            slow_mo_level: 2.0,
            play_counter: 0.0,
            replay_mode: false,
            recorded_frames: 0,
            current_recording_index: 0,
            last_record_time: 0.0,
            max_frames: (record_interval_ms * 60.0 * 10.0) as i32,
            min_frames: 0,
            cut_out: 0,
            cut_in: 0,
            max_size_mb: 10.0,
            current_frame: 0,
            current_session_type: session_type,
            auto_save_data: Vec::new(),
            auto_save_enabled: false,
            record_interval_ms,
            frame_information: Vec::new(),
            replay_leaderboards: Vec::new(),
            interval_static: None,
            events: Vec::new(),
            log: Vec::new(),
            save_undefined: Vec::new(),
        };
        if let Some(ini) = ini {
            if ini.has_section("QUALITY") {
                let level = ini.get_int("QUALITY", "LEVEL").unwrap_or(0);
                match level {
                    0 => m.record_interval_ms = 120.0,
                    1 => m.record_interval_ms = 90.0,
                    2 => m.record_interval_ms = 60.0,
                    3 => m.record_interval_ms = 30.0,
                    4 => m.record_interval_ms = 15.0,
                    _ => m.log.push(format!("ERROR: Replay Quality Level {level} not supported, using default 30Hz")),
                }
                m.log.push(format!("REPLAY QUALITY AT {:.6} Hz", 1000.0 / m.record_interval_ms));
            }
            let mut car_bytes = ((cars as u64) * 0x110) as f32;
            if car_bytes == 0.0 {
                car_bytes = 272.0;
            }
            if ini.has_section("REPLAY") {
                let x = ini.get_float("REPLAY", "MAX_SIZE_MB").unwrap_or(0.0);
                m.max_size_mb = if x > 1000.0 {
                    1000.0
                } else if x >= 10.0 {
                    x
                } else {
                    10.0
                };
                m.log.push(format!("MAX SIZE MB {:.6}", m.max_size_mb));
                m.max_frames = ((m.max_size_mb * 1_000_000.0) / car_bytes) as i32;
            }
            let hz = 1000.0 / m.record_interval_ms;
            m.log.push(format!("ReplayManager MAX FRAMES COUNT {} WHICH IS {:.6} MINS", m.max_frames, m.max_frames as f64 / (hz * 60.0)));
            if ini.has_section("AUTOSAVE") {
                m.auto_save_enabled = ini.get_int("AUTOSAVE", "ENABLED").unwrap_or(0) != 0;
                if m.auto_save_enabled {
                    let size = |key: &str| ini.get_int("AUTOSAVE", key).unwrap_or(0);
                    let (race, qualify, others) = (size("RACE"), size("QUALIFY"), size("OTHERS"));
                    m.auto_save_data.push((3, ReplayAutosave { size: race, short_name: "R".to_string() }));
                    m.auto_save_data.push((2, ReplayAutosave { size: qualify, short_name: "Q".to_string() }));
                    for session in [1, 4, 5, 6, 7] {
                        m.auto_save_data.push((session, ReplayAutosave { size: others, short_name: "O".to_string() }));
                    }
                    let seconds = ini.get_float("AUTOSAVE", "MIN_TIME_SECONDS").unwrap_or(0.0);
                    let x = ((seconds as f64) * hz) as f32;
                    m.min_frames = x.ceil() as i32;
                    m.log.push(format!("Autosave replay options min frame: {} ", m.min_frames));
                }
            }
        }
        m.size_ring(track_objects);
        m.log.push(format!("ReplayManager initialized current type {}", m.current_session_type));
        m
    }

    /// The constructor's steps 4 and 5: every recorder's ring and the scene's frames get
    /// `max_frames` slots.
    pub fn size_ring(&mut self, track_objects: Option<usize>) {
        let frames = self.max_frames.max(0) as usize;
        for recorder in &mut self.recorders {
            recorder.set_frame_size(frames);
        }
        self.frame_information.resize(frames, FrameInformation::default());
        if let Some(count) = track_objects {
            for info in &mut self.frame_information {
                info.track_objects.resize(count, [0; 6]);
            }
        }
    }

    fn event(&mut self, status: i32, time_mult: f32, slow_motion_level: f32) {
        self.events.push(ReplayEvent::StatusChanged { status, time_mult, slow_motion_level });
    }

    /// `ReplayManager::onStepCompleted` 0x140158b50, after a physics step at the time `pt`
    /// (ms). `states`, `avatars`: every car's; `sun_angle`: the lighting's angle;
    /// `track_objects`: every loose object's world matrix (`None`: it has no body). The
    /// answer: a frame was recorded.
    pub fn on_step_completed(&mut self, pt: f64, states: &[CarPhysicsState], avatars: &[AvatarRecord], sun_angle: f32, track_objects: &[Option<Mat44f>]) -> bool {
        let dt = pt - self.last_record_time;
        if dt < self.record_interval_ms || dt.is_nan() {
            return false;
        }
        if self.has_loaded_replay {
            return false;
        }
        for (i, recorder) in self.recorders.iter_mut().enumerate() {
            recorder.record_frame(&states[i], &avatars[i]);
        }
        if let Some(info) = usize::try_from(self.current_recording_index).ok().and_then(|at| self.frame_information.get_mut(at)) {
            info.sun_angle = float_to_half(sun_angle);
            let lb = (self.replay_leaderboards.len() as i64 - 1).max(0);
            info.lb_index = lb as i16;
            for (k, matrix) in track_objects.iter().enumerate() {
                let (Some(m), Some(slot)) = (matrix, info.track_objects.get_mut(k)) else { continue };
                let e = super::recorder::to_euler(m);
                // (the game has `mat44f::toEuler` inline here)
                *slot = [float_to_half(e[0]), float_to_half(e[1]), float_to_half(e[2]), float_to_half(m.m[3][0]), float_to_half(m.m[3][1]), float_to_half(m.m[3][2])];
            }
        }
        if self.recorded_frames < self.max_frames {
            self.recorded_frames += 1;
        }
        self.current_recording_index += 1;
        if self.current_recording_index >= self.max_frames {
            self.current_recording_index = 0;
        }
        self.last_record_time = pt;
        true
    }

    /// `ReplayManager::onLapCompleted` 0x140158a00.
    pub fn on_lap_completed(&mut self, car: usize, lap_time: u32, lap_count: u32, is_valid: bool) {
        if lap_count != 0 {
            if self.replay_laps.len() <= car {
                self.replay_laps.resize(car + 1, Vec::new());
            }
            self.replay_laps[car].push(ReplayLap { time: lap_time, is_valid });
        }
    }

    /// The handler of every car's `evOnLapCompleted` that the constructor registers second
    /// (lambda 0x140154da0): a snapshot of `RaceManager::getLeaderboard`.
    pub fn on_lap_leaderboard(&mut self, leaderboard: Vec<LeaderboardEntry>) {
        self.replay_leaderboards.push(leaderboard);
    }

    /// The handler of `Game::evOnPostUpdate` (lambda 0x140154d20): the first snapshot.
    pub fn wants_first_leaderboard(&self) -> bool {
        !self.has_loaded_replay && self.replay_leaderboards.is_empty()
    }

    /// The physical ring slot of the frame under the cursor (`None` where the game would
    /// divide by zero or read before its frames).
    fn ring_index(&self) -> Option<usize> {
        if self.recorded_frames == 0 {
            return None;
        }
        usize::try_from(self.current_frame.wrapping_add(self.current_recording_index) % self.recorded_frames).ok()
    }

    /// `ReplayManager::getLeaderboard` 0x140157040.
    pub fn get_leaderboard(&self) -> Vec<LeaderboardEntry> {
        let Some(info) = self.ring_index().and_then(|at| self.frame_information.get(at)) else { return Vec::new() };
        usize::try_from(info.lb_index).ok().and_then(|at| self.replay_leaderboards.get(at)).cloned().unwrap_or_default()
    }

    /// `ReplayManager::getCurrentSunAngle` 0x140156d00.
    pub fn get_current_sun_angle(&self) -> f32 {
        self.ring_index().and_then(|at| self.frame_information.get(at)).map_or(0.0, |info| half_to_float(info.sun_angle))
    }

    /// `ReplayManager::getNormalizedPosition` 0x140157230.
    pub fn get_normalized_position(&self) -> f32 {
        self.current_frame as f32 / (self.recorded_frames - 2) as f32
    }

    /// `ReplayManager::getNormalizedLapChange` 0x1401570d0.
    pub fn get_normalized_lap_change(&self, car: usize) -> Vec<f32> {
        let Some(recorder) = self.recorders.get(car) else { return Vec::new() };
        let cri = self.current_recording_index as u32;
        recorder
            .fetch_lap_frames()
            .into_iter()
            .map(|f| {
                let u = if f < cri { f.wrapping_sub(cri).wrapping_add(self.recorded_frames as u32) } else { f.wrapping_sub(cri) };
                u as f32 / self.recorded_frames as f32
            })
            .collect()
    }

    /// `ReplayManager::getLaps` 0x140156d70: the laps of a car up to the lap count of its
    /// state (a lap the list does not have has no time).
    pub fn get_laps(&self, car: usize, lap_count: u32) -> Vec<ReplayLap> {
        let laps = self.replay_laps.get(car).cloned().unwrap_or_default();
        (0..lap_count as usize).map(|i| laps.get(i).copied().unwrap_or_default()).collect()
    }

    /// `ReplayManager::save` 0x140159d10, the file's name: `None` (and the event `false`) for
    /// a name with a character no file may have; else the path below the replay folder
    /// (`temp/<name>` for an autosave). The game adds no extension.
    pub fn save_name(&mut self, name: &str, temp_dir: bool) -> Option<String> {
        if has_illegal_char(name) {
            self.events.push(ReplayEvent::Saved(false));
            return None;
        }
        self.last_filename = name.to_string();
        Some(if temp_dir { format!("temp/{name}") } else { name.to_string() })
    }

    /// `ReplayManager::save`, the file's bytes.
    pub fn save_bytes(&mut self, info: &SaveInfo) -> Result<Vec<u8>, String> {
        let mut out = Vec::new();
        self.save_undefined.clear();
        out.extend_from_slice(&16i32.to_le_bytes());
        out.extend_from_slice(&self.record_interval_ms.to_le_bytes());
        put_string(&mut out, &info.weather);
        put_string(&mut out, &info.track);
        put_string(&mut out, &info.layout);
        out.extend_from_slice(&(info.cars.len() as i32).to_le_bytes());
        out.extend_from_slice(&self.current_recording_index.to_le_bytes());
        // the scene's frames as they lie in memory: neither turned to the ring's start nor cut
        let scene = self.frame_information.len().min(self.recorded_frames.max(0) as usize);
        out.extend_from_slice(&(scene as i32).to_le_bytes());
        let objects = self.frame_information.first().map_or(0, |f| f.track_objects.len());
        out.extend_from_slice(&(objects as u32).to_le_bytes());
        for info in &self.frame_information[..scene] {
            out.extend_from_slice(&info.sun_angle.to_le_bytes());
            out.extend_from_slice(&info.lb_index.to_le_bytes());
            for object in &info.track_objects {
                for half in object {
                    out.extend_from_slice(&half.to_le_bytes());
                }
            }
        }
        let rf = self.recorded_frames;
        if rf <= 0 {
            return Err("nothing is recorded".to_string());
        }
        let cri = self.current_recording_index;
        for (c, car) in info.cars.iter().enumerate() {
            put_string(&mut out, &car.unix_name);
            put_string(&mut out, &car.driver_name);
            put_string(&mut out, &car.nation_code);
            put_string(&mut out, &car.team);
            put_string(&mut out, &car.skin);
            let start = cri.wrapping_add(self.cut_in) % rf;
            let end = self.cut_out.wrapping_add(cri) % rf;
            let mut count = 0;
            let mut p = start;
            loop {
                p = (p + 1) % rf;
                count += 1;
                if p == end || count > rf {
                    break;
                }
            }
            let (cut_in, cut_out) = (self.cut_in, self.cut_out);
            let recorder = self.recorders.get_mut(c).ok_or("a car without a recorder")?;
            recorder.save(&mut out, cut_in, cut_out, count, cri, info.wings.get(c).copied().unwrap_or(0))?;
            let laps = self.replay_laps.get(c).map(Vec::as_slice).unwrap_or(&[]);
            out.extend_from_slice(&(laps.len() as i32).to_le_bytes());
            for lap in laps {
                out.extend_from_slice(&lap.time.to_le_bytes());
                // (the three bytes after the flag are whatever the game's stack held)
                out.extend_from_slice(&[lap.is_valid as u8, 0, 0, 0]);
                self.save_undefined.extend(out.len() - 3..out.len());
            }
        }
        out.extend_from_slice(&(self.replay_leaderboards.len() as i32).to_le_bytes());
        for snapshot in &self.replay_leaderboards {
            for entry in snapshot {
                let guid = usize::try_from(entry.car).ok().and_then(|car| info.guids.get(car)).copied().unwrap_or(entry.car);
                out.extend_from_slice(&guid.to_le_bytes());
                out.extend_from_slice(&(entry.best_lap as f32).to_le_bytes());
                out.extend_from_slice(&(entry.is_race_mode as i32).to_le_bytes());
                out.extend_from_slice(&entry.laps.to_le_bytes());
                out.extend_from_slice(&(entry.total_time as f32).to_le_bytes());
            }
        }
        self.events.push(ReplayEvent::Saved(true));
        Ok(out)
    }

    /// `ReplayManager::load` 0x140157680. The manager must be a fresh one of a session with
    /// no cars (as `RaceManager::initReplay` 0x14013e820 makes it).
    pub fn load(&mut self, bytes: &[u8], loader: &mut dyn ReplayLoader) -> Result<(), String> {
        let mut r = Reader { bytes, at: 0 };
        let version = r.i32()?;
        self.log.push(format!("Loading replay version {version}"));
        let mut weather = String::new();
        if version < 13 {
            self.record_interval_ms = 32.0;
        } else {
            self.record_interval_ms = r.f64()?;
            weather = r.string()?;
        }
        let track = r.string()?;
        let layout = if version > 13 { r.string()? } else { String::new() };
        loader.load_track(&track, &layout);
        let car_count = r.i32()?;
        if version > 1 {
            r.i32()?;
        }
        self.current_recording_index = 0;
        if version > 7 {
            let scene = r.i32()?;
            self.frame_information.clear();
            let objects = if version > 10 { r.i32()? } else { 0 };
            for _ in 0..scene.max(0) {
                let sun_angle = if version < 9 { float_to_half(r.f32()?) } else { r.u16()? };
                let lb_index = if version < 10 { 0 } else { r.u16()? as i16 };
                let mut track_objects = Vec::new();
                for _ in 0..objects.max(0) {
                    let mut object = [0u16; 6];
                    for half in &mut object {
                        *half = r.u16()?;
                    }
                    track_objects.push(object);
                }
                self.frame_information.push(FrameInformation { sun_angle, lb_index, track_objects });
            }
        }
        self.recorders.clear();
        self.replay_laps.clear();
        for _ in 0..car_count.max(0) {
            let mut header = CarHeader { unix_name: r.string()?, driver_name: r.string()?, nation_code: r.string()?, team: r.string()?, skin: r.string()? };
            // (a nation code of more than three letters becomes AC)
            if header.nation_code.encode_utf16().count() > 3 {
                header.nation_code = "AC".to_string();
            }
            let mut recorder = loader.add_car(&header);
            self.recorded_frames = recorder.load(bytes, &mut r.at, version)?;
            let laps = r.i32()? as u32;
            if laps >= 10000 {
                return Err("the replay file has a car with 10000 laps or more".to_string());
            }
            let mut list = Vec::new();
            for _ in 0..laps {
                let raw = r.take(8)?;
                list.push(ReplayLap { time: u32::from_le_bytes(raw[0..4].try_into().unwrap()), is_valid: raw[4] != 0 });
            }
            self.recorders.push(recorder);
            self.replay_laps.push(list);
        }
        if version > 9 {
            // The reader takes every snapshot to have an entry for every car; the writer
            // writes each snapshot as long as it was. A file whose snapshots are shorter is read
            // out of step from there on: the game then asks for a car that is not there (and
            // crashes on it). Here such a file plays without its leaderboards.
            let mut read = || -> Result<Vec<Vec<LeaderboardEntry>>, String> {
                let mut all = Vec::new();
                let snapshots = r.i32()?;
                for _ in 0..snapshots.max(0) {
                    let mut snapshot = Vec::new();
                    for _ in 0..car_count.max(0) {
                        let car = r.i32()?;
                        let best = r.f32()?;
                        let race = r.i32()?;
                        let laps = r.i32()?;
                        let total = r.f32()?;
                        if car < 0 || car >= car_count {
                            return Err(format!("a leaderboard entry names car {car} of {car_count}"));
                        }
                        snapshot.push(LeaderboardEntry { car, total_time: total as f64, best_lap: best as f64, laps, is_race_mode: race > 0, is_black_flagged: loader.is_black_flagged(car), has_completed_last_lap: false });
                    }
                    all.push(snapshot);
                }
                Ok(all)
            };
            match read() {
                Ok(all) => self.replay_leaderboards.extend(all),
                Err(message) => {
                    self.log.push(format!("the file's leaderboard snapshots cannot be read ({message}): the replay plays without them"));
                    for info in &mut self.frame_information {
                        info.lb_index = 0;
                    }
                }
            }
        }
        self.has_loaded_replay = true;
        if !weather.is_empty() {
            loader.apply_custom_weather(&weather);
        }
        Ok(())
    }

    /// The autosave of the closure 0x1401546d0 (a new session began) and of `shutdown`
    /// 0x14015b1c0: the letter of the file's name and how many files are kept, when the
    /// session that ends is saved.
    pub fn autosave(&self) -> Option<&ReplayAutosave> {
        if self.has_loaded_replay || !self.auto_save_enabled || self.min_frames > self.recorded_frames {
            return None;
        }
        self.auto_save_data.iter().find(|(session, _)| *session == self.current_session_type).map(|(_, data)| data).filter(|data| data.size != 0)
    }

    /// The end of the closure 0x1401546d0, after the autosave (if any) was written.
    pub fn on_new_session(&mut self, session_type: i32) {
        if !self.has_loaded_replay && self.auto_save_enabled {
            self.reset_replay();
        }
        self.current_session_type = session_type;
    }

    /// `ReplayManager::resetReplay` 0x1401597d0.
    pub fn reset_replay(&mut self) {
        for laps in &mut self.replay_laps {
            laps.clear();
        }
        for recorder in &mut self.recorders {
            recorder.clear_frames();
        }
        self.play_counter = 0.0;
        self.recorded_frames = 0;
        self.current_recording_index = 0;
        self.replay_leaderboards.clear();
    }

    /// `ReplayManager::updateReplayAudio` 0x14015bc00.
    pub fn update_replay_audio(&mut self) {
        let on = if self.status == SLOW_MOTION { self.slow_mo_level <= 2.0 } else { 0.5 <= self.time_mult && self.time_mult <= 2.0 };
        self.events.push(ReplayEvent::Audio(on));
    }

    /// `ReplayManager::play` 0x1401591f0.
    pub fn play(&mut self) {
        self.time_mult = 1.0;
        self.status = PLAY;
        self.event(PLAY, 1.0, self.slow_mo_level);
        self.update_replay_audio();
    }

    /// `ReplayManager::pause` 0x140159070.
    pub fn pause(&mut self) {
        self.status = PAUSE;
        self.time_mult = 0.0;
        self.event(PAUSE, 0.0, self.slow_mo_level);
        self.update_replay_audio();
    }

    /// `ReplayManager::stop` 0x14015b6f0.
    pub fn stop(&mut self) {
        self.time_mult = 0.0;
        self.status = STOP;
        self.current_frame = self.cut_in;
        self.events.push(ReplayEvent::Rewind(0));
        self.event(STOP, self.time_mult, self.slow_mo_level);
        self.update_replay_audio();
    }

    /// `ReplayManager::rewind` 0x140159b40.
    pub fn rewind(&mut self, n: i32, frame_step: bool) {
        if !frame_step {
            if self.time_mult > -1.0 {
                self.time_mult = -1.0;
            }
            self.status = REWIND;
            let t = self.time_mult - n as f32;
            self.time_mult = if t > 0.0 {
                0.0
            } else if t >= -20.0 {
                t
            } else {
                -20.0
            };
            self.events.push(ReplayEvent::Rewind(self.time_mult as i32));
            self.event(REWIND, self.time_mult, 1.0);
            self.events.push(ReplayEvent::Rewind(0));
            self.update_replay_audio();
        } else {
            let v = self.current_frame.wrapping_sub(n);
            self.current_frame = if v > 1 { 1 } else { v.max(0) };
        }
    }

    /// `ReplayManager::fastForward` 0x140156780.
    pub fn fast_forward(&mut self, n: i32, frame_step: bool) {
        if !frame_step {
            if self.time_mult <= 1.0 {
                self.time_mult = 1.0;
            }
            self.old_status = self.status;
            self.status = FAST_FORWARD;
            let t = n as f32 + self.time_mult;
            self.time_mult = if t > 20.0 {
                20.0
            } else if t >= 0.0 {
                t
            } else {
                0.0
            };
            self.event(FAST_FORWARD, self.time_mult, 1.0);
            self.update_replay_audio();
        } else {
            let v = self.current_frame.wrapping_add(n);
            self.current_frame = if v > 1 { 1 } else { v.max(0) };
        }
    }

    /// `ReplayManager::slowMotion` 0x14015b3b0.
    pub fn slow_motion(&mut self, level: f32) {
        self.slow_mo_level = level;
        if self.status == SLOW_MOTION {
            self.time_mult = 1.0 / level;
        } else {
            self.slow_mo_counter = 0.0;
        }
        self.status = SLOW_MOTION;
        self.event(SLOW_MOTION, 1.0, level);
        self.update_replay_audio();
    }

    /// `ReplayManager::nextFrame` 0x140158900.
    pub fn next_frame(&mut self) {
        self.status = SINGLE_FRAME;
        self.old_status = SINGLE_FRAME;
        self.time_mult = 0.49;
        self.update_replay_audio();
    }

    /// `ReplayManager::photoMode` 0x140159120.
    pub fn photo_mode(&mut self) {
        self.status = PHOTO_MODE;
        self.time_mult = 0.0;
        self.event(PHOTO_MODE, 0.0, self.slow_mo_level);
        self.update_replay_audio();
    }

    /// `ReplayManager::setNormalizedPosition` 0x14015b0b0.
    pub fn set_normalized_position(&mut self, p: f32) {
        let p = if p > 1.0 {
            1.0
        } else if p < 0.0 {
            0.0
        } else {
            p
        };
        self.current_frame = (self.recorded_frames as f32 * p) as i32;
        self.event(POSITION_SET, self.time_mult, self.slow_mo_level);
        self.events.push(ReplayEvent::Rewind(0));
    }

    fn clamp01(p: f32) -> f32 {
        if p > 1.0 {
            1.0
        } else if p < 0.0 {
            0.0
        } else {
            p
        }
    }

    /// `ReplayManager::setCutInPerc` 0x14015af30.
    pub fn set_cut_in_perc(&mut self, p: f32) -> f32 {
        let p = Self::clamp01(p);
        self.cut_in = ((self.recorded_frames - 1) as f32 * p) as i32;
        if self.current_frame < self.cut_in {
            self.current_frame = self.cut_in;
            self.update_replay_recorders();
        }
        Self::clamp01(self.cut_in as f32 / (self.recorded_frames - 2) as f32)
    }

    /// `ReplayManager::setCutOutPerc` 0x14015aff0.
    pub fn set_cut_out_perc(&mut self, p: f32) -> f32 {
        let p = Self::clamp01(p);
        self.cut_out = ((self.recorded_frames - 1) as f32 * p) as i32;
        if self.cut_out <= self.current_frame {
            self.current_frame = self.cut_out;
            self.update_replay_recorders();
        }
        Self::clamp01(self.cut_out as f32 / (self.recorded_frames - 2) as f32)
    }

    /// `ReplayManager::nextLap` 0x140158930: 60 frames past the first lap start after the cursor.
    pub fn next_lap(&mut self, focused_car: usize) {
        let Some(recorder) = self.recorders.get(focused_car) else { return };
        let mut target = 0u32;
        for f in recorder.fetch_lap_frames() {
            if (self.current_frame as u32) < f && target == 0 {
                target = f.wrapping_add(60);
            }
        }
        if target != 0 {
            self.current_frame = target as i32;
        }
    }

    /// `ReplayManager::previousLap` 0x1401592a0: 60 frames before the last lap start before
    /// the cursor.
    pub fn previous_lap(&mut self, focused_car: usize) {
        let Some(recorder) = self.recorders.get(focused_car) else { return };
        let mut target = 0u32;
        for f in recorder.fetch_lap_frames() {
            if f < self.current_frame as u32 {
                target = f.wrapping_sub(60);
            }
        }
        if target as i32 != 0 {
            self.current_frame = target as i32;
        }
    }

    /// `ReplayManager::startReplayMode` 0x14015b480, as far as it is the manager's own (the
    /// cameras, the sound and the focus are the host's: the events tell it).
    pub fn start_replay_mode(&mut self, seconds: u32) {
        self.is_active = true;
        self.cut_in = 0;
        self.current_frame = 0;
        self.cut_out = self.recorded_frames - 1;
        // (seconds times milliseconds a frame: the game's own slip)
        let back = seconds as f64 * self.record_interval_ms;
        if back < self.recorded_frames as f64 {
            self.current_frame = (self.recorded_frames as f64 - back) as i32;
        }
        self.replay_mode = true;
        self.events.push(ReplayEvent::Audio(true));
        self.events.push(ReplayEvent::Started);
        self.status = MODE_ENTER;
        self.event(MODE_ENTER, self.time_mult, self.slow_mo_level);
        self.play();
    }

    /// `ReplayManager::stopReplayMode` 0x14015b800.
    pub fn stop_replay_mode(&mut self) {
        if !self.replay_mode {
            return;
        }
        self.events.push(ReplayEvent::Audio(true));
        self.is_active = false;
        self.status = MODE_EXIT;
        self.replay_mode = false;
        self.time_mult = 1.0;
        self.events.push(ReplayEvent::Stopped);
        self.event(MODE_EXIT, self.time_mult, self.slow_mo_level);
    }

    /// `ReplayManager::updatePlayState` 0x14015b9a0, once a frame of `dt` seconds while the
    /// replay shows. The answer: the track objects' matrices of `updateReplayRecorders`.
    pub fn update_play_state(&mut self, dt: f32) -> Vec<Mat44f> {
        if 0.0 >= self.slow_mo_level {
            self.slow_mo_level = 1.0;
        }
        if self.status == SINGLE_FRAME {
            if self.old_status == SINGLE_FRAME {
                self.old_status = PAUSE;
            } else {
                self.time_mult = 0.0;
            }
        }
        if self.status == REWIND {
            self.events.push(ReplayEvent::Rewind(0));
        }
        if self.status == SLOW_MOTION {
            let c = self.slow_mo_counter;
            let d = if c >= f32::from_bits(0x3ecc_cccd) {
                self.slow_mo_level
            } else {
                self.slow_mo_counter = c + dt;
                1.0 - (c * 2.5) * (1.0 - self.slow_mo_level)
            };
            self.time_mult = 1.0 / d;
        }
        let mut t = self.time_mult;
        if t > 5.0 {
            t = 5.0;
        } else if !(t >= -5.0) {
            t = -5.0;
        }
        self.time_mult = t;
        self.play_counter += (t.abs() as f64) * ((dt as f64) * 1000.0);
        while self.play_counter >= self.record_interval_ms {
            let dir = if self.time_mult > 0.0 {
                1
            } else if self.time_mult < 0.0 {
                -1
            } else {
                0
            };
            if self.recorded_frames == 0 {
                break;
            }
            self.current_frame = (dir + self.current_frame) % self.recorded_frames;
            if self.current_frame <= self.cut_in {
                self.current_frame = self.cut_in;
                self.pause();
            }
            if self.current_frame >= self.cut_out {
                self.current_frame = self.cut_out - 1;
                self.pause();
            }
            self.play_counter -= self.record_interval_ms;
        }
        let objects = self.update_replay_recorders();
        if self.status != SINGLE_FRAME {
            self.old_status = self.status;
        }
        objects
    }

    /// `ReplayManager::updateReplayRecorders` 0x14015bc90: the track objects between the
    /// frame before the cursor's and the cursor's (the answer: their matrices), and every
    /// recorder's blend and position. The host then asks each recorder for its car's state
    /// ([`ReplayManager::get_physics_state`]) and wings.
    pub fn update_replay_recorders(&mut self) -> Vec<Mat44f> {
        let t = (self.play_counter as f32) / (self.record_interval_ms as f32);
        let Some(idx) = self.ring_index() else { return Vec::new() };
        let mut objects = Vec::new();
        if let Some(info) = self.frame_information.get(idx) {
            let previous = &self.frame_information[idx.saturating_sub(1)];
            let decode = |h: &CompressedTransform| create_from_euler_safe([half_to_float(h[0]), half_to_float(h[1]), half_to_float(h[2])], [half_to_float(h[3]), half_to_float(h[4]), half_to_float(h[5])]);
            for (k, object) in info.track_objects.iter().enumerate() {
                let cur = decode(object);
                let prev = previous.track_objects.get(k).map_or(cur, decode);
                objects.push(mat44f_lerp(&prev, &cur, t));
            }
        }
        for recorder in &mut self.recorders {
            if self.status == REWIND {
                recorder.is_rewinding = true;
                recorder.frame_blend = 1.0 - t;
            } else {
                recorder.frame_blend = t;
                recorder.is_rewinding = false;
            }
            if self.current_frame == self.recorded_frames - 1 {
                recorder.frame_blend = 0.0;
            }
            recorder.playing_pos = idx as i32;
        }
        objects
    }

    /// A car's state at the cursor (`ReplayRecorder::getPhysicsState` through the car's
    /// state provider, as `updateReplayRecorders` calls it).
    pub fn get_physics_state(&mut self, car: usize, out: &mut CarPhysicsState, avatar: &mut PlaybackAvatar, track: &dyn PlaybackTrack) {
        let interval = self.record_interval_ms;
        if let Some(recorder) = self.recorders.get(car) {
            recorder.get_physics_state(out, avatar, track, &mut self.interval_static, interval);
        }
    }
}
