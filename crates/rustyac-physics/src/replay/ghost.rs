// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! `GhostCarRecorder` (0x160 bytes): the ghost of the player's best lap.
//!
//! Every 129 ms of physics time, once the start line was crossed, the recorder keeps the
//! car body's, the four wheels' and the four suspensions' world matrices
//! (`onStepComplete` 0x1401093f0). A finished lap that is valid, without a cut and quicker
//! than the ghost's becomes the ghost (`onLapCompleted` 0x1401091e0). The ghost is played by
//! the player's own lap time: the frame is the time over 129 ms, the blend what is left
//! (`GhostCar::update` 0x140106380, `setCurrentFrame` 0x140109c30, `shouldDisplayGhostCar`
//! 0x140109ca0, `getBodyMatrix` 0x140107cd0, `getTyreMatrix` 0x140108590,
//! `getSuspensionMatrix` 0x140108520). The file: `saveGhostCarRecording` 0x1401097b0,
//! `loadGhostCarRecording` 0x140108c10, its name `getGhostCarFileName` 0x140107d20.

use crate::dxmath::mat44f_lerp;
use crate::vecmath::Mat44f;

/// `GhostCarRecorder::recordIntervalMS`.
pub const RECORD_INTERVAL_MS: f64 = 129.0;

/// `GhostCarFrame` (0x240 bytes): the body, the wheels, the suspensions.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GhostCarFrame {
    pub body: Mat44f,
    pub wheels: [Mat44f; 4],
    pub suspensions: [Mat44f; 4],
}

impl GhostCarFrame {
    /// The frame as the file has it: nine matrices of sixteen floats.
    pub fn to_bytes(&self, out: &mut Vec<u8>) {
        for m in std::iter::once(&self.body).chain(&self.wheels).chain(&self.suspensions) {
            for v in m.m.iter().flatten() {
                out.extend_from_slice(&v.to_le_bytes());
            }
        }
    }

    pub fn from_bytes(bytes: &[u8]) -> GhostCarFrame {
        let matrix = |k: usize| {
            let mut m = Mat44f::IDENTITY;
            for i in 0..16 {
                let at = 0x40 * k + 4 * i;
                m.m[i / 4][i % 4] = f32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
            }
            m
        };
        GhostCarFrame { body: matrix(0), wheels: [matrix(1), matrix(2), matrix(3), matrix(4)], suspensions: [matrix(5), matrix(6), matrix(7), matrix(8)] }
    }
}

/// What the recorder reads of the car's `TimeTransponder` in a step.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct GhostTransponder {
    /// `t`: the running lap's time, ms
    pub t: u32,
    /// `lastLap`
    pub last_lap: u32,
    /// `lapCount`
    pub lap_count: u32,
    /// `finishLinePassed`: the line was crossed in this step
    pub finish_line_passed: bool,
    /// `wasLastLapValid`
    pub was_last_lap_valid: bool,
    /// `getCuts()` of the lap that just ended
    pub cuts: i32,
}

/// The names a ghost lap is kept under.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GhostNames {
    pub driver: String,
    pub track: String,
    pub config: String,
    pub car: String,
}

/// `GhostCarRecorder`.
#[derive(Clone, Debug, PartialEq)]
pub struct GhostCarRecorder {
    pub blend: f32,
    pub recording_buffer: Vec<GhostCarFrame>,
    pub playing_buffer: Vec<GhostCarFrame>,
    /// `ghostDriverName`, `trackName`, `trackConfig`, `carUnixName`: of the ghost lap
    pub names: GhostNames,
    pub advantage_time_ms: f64,
    pub ghost_car_lap: u32,
    pub line_passed_for_the_first_time: bool,
    pub is_ghost_car_lap_valid: bool,
    pub current_frame: u32,
    pub last_record_time: f64,
    pub need_to_save: bool,
    pub recording_switch: bool,
    pub playing_switch: bool,
    pub current_lap: i32,
    pub version: i32,
    pub suspension_wheels_enabled: i32,
}

fn read_i32(bytes: &[u8], at: &mut usize) -> Option<i32> {
    let v = i32::from_le_bytes(bytes.get(*at..*at + 4)?.try_into().ok()?);
    *at += 4;
    Some(v)
}

fn put_string(out: &mut Vec<u8>, text: &str) {
    let bytes = super::manager::encode_utf8(text);
    out.extend_from_slice(&(bytes.len() as i32).to_le_bytes());
    out.extend_from_slice(&bytes);
}

impl GhostCarRecorder {
    /// `GhostCarRecorder::GhostCarRecorder` 0x140106b90 (`recording`, `playing`: race.ini
    /// `[GHOST_CAR] RECORDING`, `PLAYING`; `driver`: the player's name).
    pub fn new(driver: &str, recording: bool, playing: bool) -> GhostCarRecorder {
        GhostCarRecorder {
            blend: 0.0,
            recording_buffer: Vec::new(),
            playing_buffer: Vec::new(),
            names: GhostNames { driver: driver.to_string(), ..GhostNames::default() },
            advantage_time_ms: 0.0,
            ghost_car_lap: 0xffff_ffff,
            line_passed_for_the_first_time: false,
            is_ghost_car_lap_valid: true,
            current_frame: 0,
            last_record_time: 0.0,
            need_to_save: false,
            recording_switch: recording,
            playing_switch: playing,
            current_lap: 0,
            version: 0,
            suspension_wheels_enabled: 1,
        }
    }

    /// `[GHOST_CAR] SECONDS_ADVANTAGE` of race.ini
    /// (`loadGhostCarRecorderCustomParameters` 0x140108880): the ghost runs that far ahead.
    pub fn set_seconds_advantage(&mut self, seconds: f32) {
        self.advantage_time_ms = (seconds * 1000.0) as f64;
    }

    /// The handler of `Sim::evOnNewSession` (0x140107980). The ghost itself stays.
    pub fn on_new_session(&mut self) {
        self.is_ghost_car_lap_valid = true;
        self.line_passed_for_the_first_time = false;
        self.current_frame = 0;
        self.last_record_time = 0.0;
        self.current_lap = 0;
        self.need_to_save = false;
        self.recording_buffer.clear();
    }

    /// `GhostCarRecorder::onStepComplete` 0x1401093f0, after the car's step at the time
    /// `physics_time` (ms). `frame` makes the car's matrices of this step (the body's world
    /// matrix, `Car::getTyreMatrix`, `Car::getSuspensionMatrix`); it is only asked when a
    /// frame is kept. The answer: a frame was kept.
    pub fn on_step_complete(&mut self, physics_time: f64, t: &GhostTransponder, frame: &mut dyn FnMut() -> GhostCarFrame) -> bool {
        let mut kept = false;
        if t.finish_line_passed {
            self.line_passed_for_the_first_time = true;
        }
        let dt = physics_time - self.last_record_time;
        if dt >= RECORD_INTERVAL_MS {
            if self.recording_switch && self.line_passed_for_the_first_time && self.is_ghost_car_lap_valid {
                // a lap already slower than the ghost's is given up (this frame is still kept)
                if self.ghost_car_lap < t.t {
                    self.is_ghost_car_lap_valid = false;
                }
                self.recording_buffer.push(frame());
                kept = true;
            }
            self.last_record_time = physics_time;
        }
        if self.playing_switch && self.line_passed_for_the_first_time && !self.playing_buffer.is_empty() && t.finish_line_passed {
            self.current_frame = 0;
        }
        self.current_lap = t.lap_count as i32;
        kept
    }

    /// `GhostCarRecorder::onLapCompleted` 0x1401091e0 (raised before the same step's
    /// `onStepComplete`). `current`: the names of the session. The answer: the lap is the
    /// new ghost (the game says "New Ghost Car best").
    pub fn on_lap_completed(&mut self, t: &GhostTransponder, current: &GhostNames) -> bool {
        let mut new_best = false;
        if t.was_last_lap_valid && self.is_ghost_car_lap_valid && t.last_lap < self.ghost_car_lap {
            if !self.recording_switch {
                self.is_ghost_car_lap_valid = true;
                return false;
            }
            if t.cuts == 0 {
                self.names = current.clone();
                self.ghost_car_lap = t.last_lap;
                self.playing_buffer = self.recording_buffer.clone();
                self.version = 4;
                self.need_to_save = true;
                new_best = true;
            }
        }
        if self.recording_switch {
            self.recording_buffer.clear();
        }
        self.is_ghost_car_lap_valid = true;
        new_best
    }

    /// `GhostCarRecorder::getGhostCarFileName` 0x140107d20: the file below the ghost folder,
    /// `<track>/GHOST_CAR_<driver>_<car>[_<layout>].ghost`.
    pub fn file_name(names: &GhostNames) -> String {
        if names.config.is_empty() {
            format!("{}/GHOST_CAR_{}_{}.ghost", names.track, names.driver, names.car)
        } else {
            format!("{}/GHOST_CAR_{}_{}_{}.ghost", names.track, names.driver, names.car, names.config)
        }
    }

    /// `GhostCarRecorder::saveGhostCarRecording` 0x1401097b0: the file's bytes (the ghost,
    /// never the lap that is being recorded).
    pub fn save_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&4i32.to_le_bytes());
        out.extend_from_slice(&1i32.to_le_bytes());
        put_string(&mut out, &self.names.driver);
        put_string(&mut out, &self.names.track);
        put_string(&mut out, &self.names.config);
        put_string(&mut out, &self.names.car);
        out.extend_from_slice(&self.ghost_car_lap.to_le_bytes());
        out.extend_from_slice(&(self.playing_buffer.len() as i32).to_le_bytes());
        for frame in &self.playing_buffer {
            frame.to_bytes(&mut out);
        }
        out
    }

    /// `GhostCarRecorder::loadGhostCarRecording` 0x140108c10: a file's ghost, when it is of
    /// the session's track, layout and car (`current`; the driver's name is not compared).
    /// As in the game the recorder's names are the file's even when it is refused.
    pub fn load_bytes(&mut self, bytes: &[u8], current: &GhostNames) -> bool {
        let mut at = 0usize;
        let Some(version) = read_i32(bytes, &mut at) else { return false };
        self.version = version;
        if version < 3 {
            return false;
        }
        let Some(enabled) = read_i32(bytes, &mut at) else { return false };
        self.suspension_wheels_enabled = enabled;
        let mut names = Vec::new();
        for _ in 0..4 {
            let Some(len) = read_i32(bytes, &mut at).and_then(|len| usize::try_from(len).ok()) else { return false };
            let Some(text) = bytes.get(at..at.saturating_add(len)) else { return false };
            at += len;
            names.push(super::manager::decode_utf8(text));
        }
        self.names = GhostNames { driver: names[0].clone(), track: names[1].clone(), config: names[2].clone(), car: names[3].clone() };
        if self.names.track != current.track || self.names.car != current.car || self.names.config != current.config {
            return false;
        }
        let Some(lap) = read_i32(bytes, &mut at) else { return false };
        self.ghost_car_lap = lap as u32;
        let Some(count) = read_i32(bytes, &mut at) else { return false };
        let mut frames = Vec::new();
        for _ in 0..count.max(0) {
            let Some(raw) = bytes.get(at..at + 0x240) else { return false };
            at += 0x240;
            frames.push(GhostCarFrame::from_bytes(raw));
        }
        self.playing_buffer = frames;
        true
    }

    /// `GhostCarRecorder::shouldDisplayGhostCar` 0x140109ca0.
    pub fn should_display_ghost_car(&self) -> bool {
        let n = self.playing_buffer.len() as u64;
        !self.playing_buffer.is_empty() && (self.current_frame as u64) < n.wrapping_sub(2) && self.playing_switch && self.line_passed_for_the_first_time
    }

    /// `GhostCarRecorder::setCurrentFrame` 0x140109c30.
    pub fn set_current_frame(&mut self, frame: u32) {
        let n = self.playing_buffer.len() as u64;
        self.current_frame = if (frame as u64) < n.wrapping_sub(2) { frame } else { n.wrapping_sub(2) as u32 };
    }

    /// The timing of `GhostCar::update` 0x140106380: the frame and the blend for the
    /// player's running lap time (`CarPhysicsState::lapTime`, ms). Call it only while
    /// [`GhostCarRecorder::should_display_ghost_car`] says so, as the game does.
    pub fn set_lap_time(&mut self, lap_time: u32) {
        let t = lap_time as f64 + self.advantage_time_ms;
        let inv = 1.0 / RECORD_INTERVAL_MS;
        let x = inv * t;
        // (`cvttsd2si` into 32 bits: what does not fit is 0x80000000)
        let idx: i32 = if x.is_nan() || x >= 2_147_483_648.0 || x <= -2_147_483_649.0 { i32::MIN } else { x as i32 };
        self.blend = ((t - idx as f64 * RECORD_INTERVAL_MS) * inv) as f32;
        self.set_current_frame(idx as u32);
    }

    fn pair(&self) -> Option<(&GhostCarFrame, &GhostCarFrame)> {
        let c = self.current_frame as usize;
        Some((self.playing_buffer.get(c)?, self.playing_buffer.get(c + 1)?))
    }

    /// `GhostCarRecorder::getBodyMatrix` 0x140107cd0 (the physics body: the picture puts the
    /// car's graphics offset on it for a file of version 4).
    pub fn get_body_matrix(&self) -> Option<Mat44f> {
        self.pair().map(|(a, b)| mat44f_lerp(&a.body, &b.body, self.blend))
    }

    /// `GhostCarRecorder::getTyreMatrix` 0x140108590.
    pub fn get_tyre_matrix(&self, wheel: usize) -> Option<Mat44f> {
        self.pair().map(|(a, b)| mat44f_lerp(&a.wheels[wheel], &b.wheels[wheel], self.blend))
    }

    /// `GhostCarRecorder::getSuspensionMatrix` 0x140108520.
    pub fn get_suspension_matrix(&self, wheel: usize) -> Option<Mat44f> {
        self.pair().map(|(a, b)| mat44f_lerp(&a.suspensions[wheel], &b.suspensions[wheel], self.blend))
    }
}

/// `GhostCar::getGhostCarOpacity` 0x140105480: nothing within `min_distance` of the player's
/// car, then more with the distance up to `max_opacity` at `max_distance`.
pub fn ghost_car_opacity(ghost: &[f32; 3], player: &[f32; 3], min_distance: f32, max_distance: f32, max_opacity: f32) -> f32 {
    let (dx, dy, dz) = (ghost[0] - player[0], ghost[1] - player[1], ghost[2] - player[2]);
    let s = (dy * dy + dx * dx) + dz * dz;
    let d = if s == 0.0 || s.is_nan() { 0.0 } else { rustyac_math::sqrtf(s) };
    if d > min_distance {
        let o = ((d - min_distance) / (max_distance - min_distance)) * max_opacity;
        return if o > max_opacity { max_opacity } else { o };
    }
    0.0
}
