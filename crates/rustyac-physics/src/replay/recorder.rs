// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! `ReplayRecorder` (0x110 bytes): one car's ring of 272-byte `ReplayFrame`s.
//!
//! `recordFrame` 0x1401609d0 packs a `CarPhysicsState` into a frame (positions as floats,
//! orientations as the three angles of `mat44f::toEuler` 0x140053340 in 16-bit floats, levels
//! as bytes, flags as bits); `getPhysicsState` 0x14015d850 rebuilds a `CarPhysicsState` from
//! two neighbouring frames; `save` 0x1401622a0 and `load` 0x140160220 are the car's block of
//! a replay file (`convertReplayFormat` 0x14015d4b0 for version 15); `fetchLapFrames`
//! 0x14015d760, `getWingState` 0x14015ff00, `setFrameSize` 0x140162420, `clearFrames`
//! 0x14015d4a0.

use rustyac_math::{asinf, atan2f, sqrtf};

use super::half::{float_to_half, half_to_float};
use crate::car_physics_state::{CarPhysicsState, SurfaceDef};
use crate::dxmath::mat44f_lerp;
use crate::vecmath::{create_from_euler, xm_matrix_inverse, Mat44f, Vec3f};

/// The size of a `ReplayFrame`.
pub const FRAME_SIZE: usize = 0x110;

// the offsets of `ReplayFrame`'s members
const WORLD_TRANSLATION: usize = 0x14;
const WORLD_ORIENTATION: usize = 0x20;
const SUS_TRANSLATION: usize = 0x28;
const SUS_ORIENTATION: usize = 0x58;
const TYRE_TRANSLATION: usize = 0x70;
const TYRE_ORIENTATION: usize = 0xa0;
const VELOCITY: usize = 0xb8;
const ENGINE_RPM: usize = 0xbe;
const WHEEL_ANGULAR_SPEED: usize = 0xc0;
const TYRE_SLIP_ANGLE: usize = 0xc8;
const SLIP_RATIO: usize = 0xd0;
const ND_SLIP: usize = 0xd8;
const LOAD: usize = 0xe0;
const STEER: usize = 0xe8;
const BODYWORK_VOLUME: usize = 0xea;
const DRIVETRAIN_SPEED: usize = 0xec;
const LAP_TIME: usize = 0xf0;
const LAST_LAP: usize = 0xf4;
const BEST_LAP: usize = 0xf8;
const FUEL: usize = 0xfc;
const FUEL_LAPS: usize = 0xfd;
const GEAR: usize = 0xfe;
const TYRE_DIRTY_LEVEL: usize = 0xff;
const DAMAGE_ZONE_LEVEL: usize = 0x103;
const GAS: usize = 0x108;
const BRAKE: usize = 0x109;
const LAP_COUNT: usize = 0x10a;
const STATUS: usize = 0x10c;

/// The frame's lap marker: the first frame of a new `lapCount`.
pub const STATUS_LAP: u32 = 0x800;

/// `ReplayFrame` (0x110 bytes), as the bytes the game holds and writes into a file. Bytes
/// 0x00 to 0x13 (`bodyTranslation`, `bodyOrientation`), 0xee, 0xef and 0x10b (`boh`) are
/// never written by the game's code.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ReplayFrame(pub [u8; FRAME_SIZE]);

impl Default for ReplayFrame {
    fn default() -> ReplayFrame {
        ReplayFrame([0; FRAME_SIZE])
    }
}

impl ReplayFrame {
    fn put_f32(&mut self, at: usize, v: f32) {
        self.0[at..at + 4].copy_from_slice(&v.to_le_bytes());
    }
    fn f32_at(&self, at: usize) -> f32 {
        f32::from_le_bytes(self.0[at..at + 4].try_into().unwrap())
    }
    fn put_half(&mut self, at: usize, v: f32) {
        self.0[at..at + 2].copy_from_slice(&float_to_half(v).to_le_bytes());
    }
    fn half(&self, at: usize) -> f32 {
        half_to_float(u16::from_le_bytes(self.0[at..at + 2].try_into().unwrap()))
    }
    fn put_u32(&mut self, at: usize, v: u32) {
        self.0[at..at + 4].copy_from_slice(&v.to_le_bytes());
    }
    fn u32_at(&self, at: usize) -> u32 {
        u32::from_le_bytes(self.0[at..at + 4].try_into().unwrap())
    }
    fn vec3(&self, at: usize) -> [f32; 3] {
        [self.f32_at(at), self.f32_at(at + 4), self.f32_at(at + 8)]
    }
    fn halves3(&self, at: usize) -> [f32; 3] {
        [self.half(at), self.half(at + 2), self.half(at + 4)]
    }
    /// `status` (a `std::bitset<14>` in one 32-bit word)
    pub fn status(&self) -> u32 {
        self.u32_at(STATUS)
    }
    /// `lapCount`
    pub fn lap_count(&self) -> u8 {
        self.0[LAP_COUNT]
    }
    /// `lapTime`
    pub fn lap_time(&self) -> u32 {
        self.u32_at(LAP_TIME)
    }
    /// `worldTranslation`
    pub fn world_translation(&self) -> [f32; 3] {
        self.vec3(WORLD_TRANSLATION)
    }
}

/// float to byte through a `Range` (the same sequence at every site of `recordFrame`, e.g.
/// 0x140160b0a): the value held to `[0, max]` as a float (a NaN becomes 0), then in doubles
/// its part of the range times 255, cut off towards zero.
#[allow(clippy::neg_cmp_op_on_partial_ord)] // `comiss; jae`
fn to_byte(x: f32, max: f32) -> u8 {
    let mut v = x;
    if !(v >= 0.0) {
        v = 0.0;
    }
    if v > max {
        v = max;
    }
    let q = ((v as f64) - 0.0) / ((max as f64) - 0.0);
    let m = (255.0 - 0.0) * q;
    let m = m + 0.0;
    (m as i64) as u8
}

/// byte to float through a `Range` (`getPhysicsState`, e.g. 0x14015d9f4), in doubles.
fn scale(b: u8, max: f64) -> f32 {
    let mut t = (b as f64) - 0.0;
    t /= 255.0 - 0.0;
    t *= max - 0.0;
    t += 0.0f32 as f64;
    t as f32
}

/// `cvttss2si` into a 64-bit register: what does not fit (and a NaN) is 0x8000000000000000.
fn truncate_i64(x: f32) -> i64 {
    if x.is_nan() || x >= 9.223_372e18 || x < -9.223_372e18 {
        i64::MIN
    } else {
        x as i64
    }
}

/// `mat44f::toEuler` 0x140053340 (and its copies inside `recordFrame` and
/// `ReplayManager::onStepCompleted`): heading, pitch, roll.
#[allow(clippy::neg_cmp_op_on_partial_ord)]
pub fn to_euler(m: &Mat44f) -> [f32; 3] {
    let m = &m.m;
    let mut x = atan2f(-m[2][0], m[2][2]);
    let mut t = m[2][1];
    if t > 1.0 {
        t = 1.0;
    } else if !(t >= -1.0) {
        t = -1.0;
    }
    let y = asinf(t);
    // (`ucomiss; jne` without a parity test: a NaN counts as zero)
    let zero = |v: f32| v == 0.0 || v.is_nan();
    let z = if zero(m[0][1]) && zero(m[1][1]) {
        x = 0.0;
        atan2f(m[1][0], m[0][0])
    } else {
        atan2f(-m[0][1], m[1][1])
    };
    [x, y, z]
}

/// `mat44f::createFromEulerSafe` 0x140155c30: each angle held to minus pi .. pi (a NaN
/// becomes minus pi), then `mat44f::createFromEuler` 0x140118ef0.
#[allow(clippy::neg_cmp_op_on_partial_ord)]
pub fn create_from_euler_safe(rot: [f32; 3], pos: [f32; 3]) -> Mat44f {
    let pi = f32::from_bits(0x4049_0fdb);
    let mut r = rot;
    for c in &mut r {
        if !(*c >= -pi) {
            *c = -pi;
        } else if *c > pi {
            *c = pi;
        }
    }
    create_from_euler(r, pos)
}

/// What `recordFrame` reads of the car's `CarAvatar` (not of the state it is handed).
#[derive(Clone, Copy, Debug)]
pub struct AvatarRecord<'a> {
    /// `CarAvatar::physicsState.actionsState`, `statusBytes`, `kersIsCharging`: the avatar's
    /// own copy of the state (what the picture last took), where the flags come from
    pub actions_state: u32,
    pub status_bytes: u32,
    pub kers_is_charging: bool,
    /// `CarAvatar::physicsInfo.maxFuel`
    pub max_fuel: f64,
    /// `DynamicCarEffects::currentDirtLevel` (`None`: the car has no such object)
    pub car_dirt: Option<f32>,
    /// `CarAvatar::isConnected`: the car's node is active
    pub connected: bool,
    /// `WingState::angle` of every wing (`ICarPhysicsStateProvider::getWingState`)
    pub wing_angles: &'a [f32],
}

/// The car's own things that `getPhysicsState` reads and writes outside the state.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PlaybackAvatar {
    /// `CarAvatar::physicsInfo.maxFuel`
    pub max_fuel: f64,
    /// the car has a `DynamicCarEffects`
    pub has_dynamic_effects: bool,
    /// out: `DynamicCarEffects::setDirtLevel` was called with this
    pub dirt_level: Option<f32>,
    /// out: `carNode->isActive`
    pub car_node_active: bool,
}

/// The track as `getPhysicsState` asks it.
pub trait PlaybackTrack {
    /// `InterpolatingSpline::worldToSpline(pos, -1)` 0x1401f3bf0 on the track's best-lap AI
    /// spline (`TrackAvatar::getAISpline` 0x1401c8010).
    fn world_to_spline(&self, pos: &[f32; 3]) -> f32;
    /// `Track::rayCast` 0x140278bb0: the surface, the point and the normal of a hit.
    fn ray_cast(&self, origin: &[f32; 3], direction: &[f32; 3], length: f32) -> Option<(SurfaceDef, [f32; 3], [f32; 3])>;
}

/// `ReplayRecorder`.
#[derive(Clone, PartialEq)]
pub struct ReplayRecorder {
    pub frame_blend: f32,
    pub is_rewinding: bool,
    pub frames: Vec<ReplayFrame>,
    /// `ReplayFrameAdditionalInfo`: carDirt, engineLife, turboBoost, connected
    pub frames_additional_info: Vec<[u8; 4]>,
    /// `ReplayFrameWingStatus` of every wing: the angle and three bytes never written
    pub frames_wings: Vec<Vec<[u8; 4]>>,
    pub version: i32,
    /// `metaData`: frameCount, numberOfWings
    pub meta_data: (u32, u32),
    pub recorded_frames: i32,
    pub lap_frame_index_cache: Vec<u32>,
    pub lap_cache_index: i32,
    pub playing_pos: i32,
    pub current_recording_cue: i32,
    /// `engine.ini [ENGINE_DATA] LIMITER` (9999 when the file cannot be read)
    pub std_engine_limiter: i32,
    /// how many wings the car's state provider tells of (the size of a frame's wing list)
    pub wing_count: usize,
}

impl ReplayRecorder {
    /// `ReplayRecorder::ReplayRecorder` 0x14015c9e0.
    pub fn new(std_engine_limiter: i32, wing_count: usize) -> ReplayRecorder {
        ReplayRecorder {
            frame_blend: 0.0,
            is_rewinding: false,
            frames: Vec::new(),
            frames_additional_info: Vec::new(),
            frames_wings: Vec::new(),
            version: 16,
            meta_data: (0, 0),
            recorded_frames: 0,
            lap_frame_index_cache: vec![0xffff_ffff; 999],
            lap_cache_index: 0,
            playing_pos: 0,
            current_recording_cue: 0,
            std_engine_limiter,
            wing_count,
        }
    }

    /// `ReplayRecorder::setFrameSize` 0x140162420.
    pub fn set_frame_size(&mut self, n: usize) {
        self.frames.resize(n, ReplayFrame::default());
        self.frames_wings.resize(n, Vec::new());
        self.frames_additional_info.resize(n, [0; 4]);
        for wings in &mut self.frames_wings {
            wings.resize(self.wing_count, [0; 4]);
        }
    }

    /// `ReplayRecorder::clearFrames` 0x14015d4a0 (the lap cache and the frames stay).
    pub fn clear_frames(&mut self) {
        self.current_recording_cue = 0;
        self.recorded_frames = 0;
    }

    /// `ReplayRecorder::recordFrame` 0x1401609d0.
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    pub fn record_frame(&mut self, s: &CarPhysicsState, a: &AvatarRecord) {
        let n = self.frames.len();
        if n == 0 {
            return;
        }
        if (self.recorded_frames as i64 as u64) < n as u64 {
            self.recorded_frames += 1;
        }
        if self.current_recording_cue as i64 as u64 >= n as u64 {
            self.current_recording_cue = 0;
        }
        let cue = self.current_recording_cue as usize;
        let previous_lap_count = (cue > 0).then(|| self.frames[cue - 1].lap_count());
        let f = &mut self.frames[cue];
        f.put_half(ENGINE_RPM, s.engine_rpm);
        f.0[GAS] = to_byte(s.gas, 1.0);
        f.0[BRAKE] = to_byte(s.brake, 1.0);
        f.0[GEAR] = s.gear as u8;
        f.put_half(STEER, s.steer);
        for k in 0..3 {
            f.put_f32(WORLD_TRANSLATION + 4 * k, s.world_matrix.m[3][k]);
        }
        let e = to_euler(&s.world_matrix);
        for k in 0..3 {
            f.put_half(WORLD_ORIENTATION + 2 * k, e[k]);
            f.put_half(VELOCITY + 2 * k, s.velocity[k]);
        }
        f.put_half(BODYWORK_VOLUME, s.body_work_volume);
        f.put_u32(LAP_TIME, s.lap_time);
        f.put_u32(LAST_LAP, s.last_lap);
        f.put_u32(BEST_LAP, s.best_lap);
        f.0[LAP_COUNT] = s.lap_count as u8;
        f.put_half(DRIVETRAIN_SPEED, s.drivetrain_speed);
        // the fuel as a part of the tank; a tank that is not empty never reads 0
        let mut fuel = to_byte(s.fuel / (a.max_fuel as f32), 1.0);
        if fuel == 0 && !(0.0 >= s.fuel) {
            fuel = 1;
        }
        f.0[FUEL] = fuel;
        f.0[FUEL_LAPS] = truncate_i64(s.fuel_laps) as u8;
        let info = &mut self.frames_additional_info[cue];
        info[2] = to_byte(s.turbo_boost, 5.0);
        info[1] = to_byte(s.engine_life_left, 1000.0);
        info[0] = a.car_dirt.map_or(0, |dirt| to_byte(dirt, 1.0));
        info[3] = a.connected as u8;
        for i in 0..4 {
            f.put_half(WHEEL_ANGULAR_SPEED + 2 * i, s.wheel_angular_speed[i]);
            // (the member is called tyreSlipAngle; what goes into it is `tyreSlip`)
            f.put_half(TYRE_SLIP_ANGLE + 2 * i, s.tyre_slip[i]);
            f.0[TYRE_DIRTY_LEVEL + i] = to_byte(s.tyre_dirty_level[i], 20.0);
            f.put_half(ND_SLIP + 2 * i, s.nd_slip[i]);
            f.put_half(LOAD + 2 * i, s.load[i]);
            f.put_half(SLIP_RATIO + 2 * i, s.slip_ratio[i]);
            let sus = to_euler(&s.suspension_matrix[i]);
            let tyre = to_euler(&s.tyre_matrix[i]);
            for k in 0..3 {
                f.put_f32(SUS_TRANSLATION + 12 * i + 4 * k, s.suspension_matrix[i].m[3][k]);
                f.put_half(SUS_ORIENTATION + 6 * i + 2 * k, sus[k]);
                f.put_f32(TYRE_TRANSLATION + 12 * i + 4 * k, s.tyre_matrix[i].m[3][k]);
                f.put_half(TYRE_ORIENTATION + 6 * i + 2 * k, tyre[k]);
            }
            f.0[DAMAGE_ZONE_LEVEL + i] = to_byte(s.damage_zone_level[i], 255.0);
        }
        f.0[DAMAGE_ZONE_LEVEL + 4] = to_byte(s.damage_zone_level[4], 255.0);
        let wings = &mut self.frames_wings[cue];
        for (j, angle) in a.wing_angles.iter().enumerate() {
            if j < wings.len() {
                wings[j][0] = to_byte(*angle, 100.0);
            }
        }
        // the flags: 9 and 10 from the state, the others from the avatar's own copy
        let mut status = f.status();
        let mut bit = |mask: u32, on: bool| {
            if on {
                status |= mask;
            } else {
                status &= !mask;
            }
        };
        bit(0x200, s.is_gear_grinding);
        bit(0x400, !s.is_drift_valid);
        bit(0x1, a.actions_state & 0x10_0000 != 0);
        bit(0x2, a.status_bytes & 4 != 0);
        bit(0x4, a.status_bytes & 1 != 0);
        bit(0x8, a.actions_state & 0x40 != 0);
        bit(0x10, a.actions_state & 0x1 != 0);
        bit(0x20, a.actions_state & 0x2 != 0);
        bit(0x40, a.actions_state & 0x100 != 0);
        bit(0x80, a.actions_state & 0x80 != 0);
        bit(0x100, false);
        bit(0x1000, a.actions_state & 0x400 != 0);
        bit(0x2000, a.kers_is_charging);
        // the lap marker and the cache of the frames that carry it
        let new_lap = previous_lap_count.is_some_and(|previous| previous != f.lap_count());
        bit(STATUS_LAP, new_lap);
        f.put_u32(STATUS, status);
        if new_lap && !self.lap_frame_index_cache.is_empty() {
            let at = self.lap_cache_index as usize % self.lap_frame_index_cache.len();
            self.lap_frame_index_cache[at] = cue as u32;
            self.lap_cache_index = ((self.lap_cache_index + 1) as i64 as u64 % self.lap_frame_index_cache.len() as u64) as i32;
        }
        self.current_recording_cue += 1;
    }

    /// `ReplayRecorder::save` 0x1401622a0: the car's block of a replay file, oldest frame
    /// first from `cut_in` to `cut_out` (both counted from the ring's start `idx`).
    /// `number_of_wings`: the size of the avatar's wing list.
    pub fn save(&mut self, out: &mut Vec<u8>, cut_in: i32, cut_out: i32, frame_count: i32, idx: i32, number_of_wings: u32) -> Result<(), String> {
        self.meta_data = (frame_count as u32, number_of_wings);
        out.extend_from_slice(&self.meta_data.0.to_le_bytes());
        out.extend_from_slice(&self.meta_data.1.to_le_bytes());
        let r = self.recorded_frames;
        if r <= 0 {
            return Err("a recorder without frames cannot be saved".to_string());
        }
        let stop = (cut_out.wrapping_add(idx)) % r;
        let mut p = (cut_in.wrapping_add(idx)) % r;
        loop {
            let at = usize::try_from(p).map_err(|_| "a cut before the first frame")?;
            out.extend_from_slice(&self.frames[at].0);
            out.extend_from_slice(&self.frames_additional_info[at]);
            if number_of_wings != 0 {
                for wing in &self.frames_wings[at] {
                    out.extend_from_slice(wing);
                }
            }
            p = (p + 1) % r;
            if p == stop {
                break;
            }
        }
        Ok(())
    }

    /// `ReplayRecorder::load` 0x140160220: the car's block of a file of `version`. The answer
    /// is the block's frame count. (The game crashes on a version below 15 and reads past a
    /// file that ends early; both are errors here.)
    pub fn load(&mut self, bytes: &[u8], at: &mut usize, version: i32) -> Result<i32, String> {
        self.version = version;
        if version < 15 {
            return Err("Loading replay with version <15 is no longer supported".to_string());
        }
        let mut take = |n: usize| -> Result<&[u8], String> {
            let piece = bytes.get(*at..*at + n).ok_or("the replay file ends inside a car's frames")?;
            *at += n;
            Ok(piece)
        };
        let head = take(8)?;
        self.meta_data = (u32::from_le_bytes(head[0..4].try_into().unwrap()), u32::from_le_bytes(head[4..8].try_into().unwrap()));
        let count = self.meta_data.0 as usize;
        self.recorded_frames = self.meta_data.0 as i32;
        if self.frames.len() != count {
            self.frames.resize(count, ReplayFrame::default());
            self.frames_wings.resize(count, Vec::new());
            self.frames_additional_info.resize(count, [0; 4]);
        }
        self.lap_frame_index_cache.clear();
        let wings_in_file = self.meta_data.1 as usize;
        for wings in &mut self.frames_wings {
            // (the game sizes the list by the car's wings and writes the file's count into it)
            wings.resize(self.wing_count.max(wings_in_file), [0; 4]);
        }
        for i in 0..count {
            let raw: [u8; FRAME_SIZE] = take(FRAME_SIZE)?.try_into().unwrap();
            self.frames[i] = if version < 16 { convert_replay_format(&raw) } else { ReplayFrame(raw) };
            self.frames_additional_info[i] = take(4)?.try_into().unwrap();
            if (self.frames[i].status() >> 11) & 1 != 0 {
                self.lap_frame_index_cache.push(i as u32);
            }
            for j in 0..wings_in_file {
                self.frames_wings[i][j] = take(4)?.try_into().unwrap();
            }
        }
        Ok(self.meta_data.0 as i32)
    }

    /// `ReplayRecorder::fetchLapFrames` 0x14015d760: the ring slots that begin a lap, in the
    /// order they were recorded.
    pub fn fetch_lap_frames(&self) -> Vec<u32> {
        let mut out = Vec::new();
        for &v in &self.lap_frame_index_cache {
            if v == 0xffff_ffff {
                break;
            }
            if self.frames.get(v as usize).is_some_and(|f| (f.status() >> 11) & 1 != 0) {
                out.push(v);
            }
        }
        out
    }

    /// `ReplayRecorder::getWingState` 0x14015ff00: every wing's angle at the playing position.
    pub fn get_wing_state(&self) -> Vec<f32> {
        if self.frames_wings.is_empty() {
            return Vec::new();
        }
        let at = self.playing_pos as usize;
        let Some(wings) = self.frames_wings.get(at) else { return Vec::new() };
        let next = ((self.playing_pos + 1) as i64 as u64 % self.frames_wings.len() as u64) as usize;
        let dec = |b: u8| -> f32 { ((((b as f64) - 0.0) * (1.0 / (255.0 - 0.0))) * (100.0 - 0.0) + 0.0f32 as f64) as f32 };
        let mut out = Vec::with_capacity(wings.len());
        for (i, wing) in wings.iter().enumerate() {
            let mut a = dec(wing[0]);
            if !self.frames_wings[next].is_empty() {
                let mut t = self.frame_blend;
                if self.is_rewinding {
                    t = 1.0 - t;
                }
                let b = dec(self.frames_wings[next].get(i).map_or(0, |w| w[0]));
                a += (b - a) * t;
            }
            out.push(a);
        }
        out
    }

    /// `ReplayRecorder::getPhysicsState` 0x14015d850: the state at `playingPos`, blended
    /// towards the next frame of the ring by `frameBlend`. `out` is the car's long-lived
    /// state: what is not written keeps its value. `interval`: the function's own static
    /// (the recording interval in seconds as it was at the first call of any recorder).
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    pub fn get_physics_state(&self, out: &mut CarPhysicsState, avatar: &mut PlaybackAvatar, track: &dyn PlaybackTrack, interval: &mut Option<f32>, record_interval_ms: f64) {
        let n = self.frames.len();
        if n == 0 || self.playing_pos < 0 || self.playing_pos as usize >= n {
            // (the game has no such test: it divides by zero or reads past its frames)
            return;
        }
        let a_index = self.playing_pos as usize;
        let b_index = ((self.playing_pos + 1) as i64 as u64 % n as u64) as usize;
        let (fa, fb) = (&self.frames[a_index], &self.frames[b_index]);
        let (ia, ib) = (self.frames_additional_info[a_index], self.frames_additional_info[b_index]);
        let t = self.frame_blend;
        out.limiter_rpm = self.std_engine_limiter;
        out.status_bytes = 0;
        out.time_stamp = 0.0;
        out.clutch = 1.0;
        out.is_engine_limiter_on = false;
        out.gear = fa.0[GEAR] as i32;
        out.lap_count = fa.0[LAP_COUNT] as u32;
        out.fuel_laps = fa.0[FUEL_LAPS] as f32;
        out.last_lap = fa.u32_at(LAST_LAP);
        out.best_lap = fa.u32_at(BEST_LAP);
        out.gas = scale(fa.0[GAS], 1.0);
        out.brake = scale(fa.0[BRAKE], 1.0);
        out.fuel = scale(fa.0[FUEL], 1.0) * (avatar.max_fuel as f32);
        out.engine_life_left = scale(ia[1], 1000.0);
        out.engine_rpm = fa.half(ENGINE_RPM);
        out.velocity = fa.halves3(VELOCITY);
        let [vx, vy, vz] = out.velocity;
        let squared = (vx * vx + vy * vy) + vz * vz;
        out.speed = if squared == 0.0 || squared.is_nan() { 0.0 } else { sqrtf(squared) };
        out.steer = fa.half(STEER);
        out.body_work_volume = fa.half(BODYWORK_VOLUME);
        out.drivetrain_speed = fa.half(DRIVETRAIN_SPEED);
        let world_a = create_from_euler_safe(fa.halves3(WORLD_ORIENTATION), fa.vec3(WORLD_TRANSLATION));
        out.world_matrix = world_a;
        let np_a = track.world_to_spline(&[world_a.m[3][0], world_a.m[3][1], world_a.m[3][2]]);
        out.normalized_spline_position = np_a;
        // the acceleration: the change of the velocity since the frame before, in the car's
        // axes of this frame, in g
        let previous = if self.playing_pos > 0 { self.frames[a_index - 1].halves3(VELOCITY) } else { [0.0; 3] };
        let step = *interval.get_or_insert((record_interval_ms * 0.001) as f32);
        let mut m0 = world_a;
        m0.m[3][0] = 0.0;
        m0.m[3][1] = 0.0;
        m0.m[3][2] = 0.0;
        let inv = xm_matrix_inverse(&m0).m;
        let xf = |v: &[f32; 3]| -> [f32; 3] {
            [
                ((v[1] * inv[1][0] + v[0] * inv[0][0]) + v[2] * inv[2][0]) + inv[3][0],
                ((v[1] * inv[1][1] + v[0] * inv[0][1]) + v[2] * inv[2][1]) + inv[3][1],
                ((v[1] * inv[1][2] + v[0] * inv[0][2]) + v[2] * inv[2][2]) + inv[3][2],
            ]
        };
        let (p, c) = (xf(&previous), xf(&out.velocity));
        let r = 1.0 / step;
        let k = f32::from_bits(0x3dd0_fac6);
        out.acc_g = [-(((p[0] - c[0]) * r) * k), ((p[1] - c[1]) * r) * k, -(((p[2] - c[2]) * r) * k)];
        if avatar.has_dynamic_effects {
            avatar.dirt_level = Some(scale(ia[0], 1.0));
        }
        let turbo_max = if self.version < 7 {
            avatar.car_node_active = true;
            0.5
        } else {
            avatar.car_node_active = ia[3] != 0;
            5.0
        };
        out.turbo_boost = scale(ia[2], turbo_max);
        let world_b = create_from_euler_safe(fb.halves3(WORLD_ORIENTATION), fb.vec3(WORLD_TRANSLATION));
        let np_b = track.world_to_spline(&[world_b.m[3][0], world_b.m[3][1], world_b.m[3][2]]);
        let (brake_b, gas_b) = (scale(fb.0[BRAKE], 1.0), scale(fb.0[GAS], 1.0));
        let (drivetrain_b, steer_b, rpm_b) = (fb.half(DRIVETRAIN_SPEED), fb.half(STEER), fb.half(ENGINE_RPM));
        let turbo_b = scale(ib[2], turbo_max);
        if !self.is_rewinding {
            out.brake = (brake_b - out.brake) * t + out.brake;
            out.gas = (gas_b - out.gas) * t + out.gas;
            out.drivetrain_speed = (drivetrain_b - out.drivetrain_speed) * t + out.drivetrain_speed;
            out.world_matrix = mat44f_lerp(&world_a, &world_b, t);
            out.steer = (steer_b - out.steer) * t + out.steer;
            out.engine_rpm = (rpm_b - out.engine_rpm) * t + out.engine_rpm;
            if np_b > np_a {
                out.normalized_spline_position = (np_b - np_a) * t + np_a;
            }
            out.turbo_boost = (turbo_b - out.turbo_boost) * t + out.turbo_boost;
            let (la, lb) = (fa.lap_time() as f32, fb.lap_time() as f32);
            out.lap_time = truncate_i64((lb - la) * t + la) as u32;
        } else {
            let u = 1.0 - t;
            out.brake = (out.brake - brake_b) * u + brake_b;
            out.gas = (out.gas - gas_b) * u + gas_b;
            out.drivetrain_speed = (out.drivetrain_speed - drivetrain_b) * u + drivetrain_b;
            out.world_matrix = mat44f_lerp(&world_b, &world_a, u);
            out.steer = (out.steer - steer_b) * u + steer_b;
            // (the engine speed alone keeps the forward formula)
            out.engine_rpm = (rpm_b - out.engine_rpm) * t + out.engine_rpm;
            if np_b > np_a {
                out.normalized_spline_position = (np_a - np_b) * u + np_b;
            }
            out.turbo_boost = (out.turbo_boost - turbo_b) * u + turbo_b;
            out.lap_time = fa.lap_time();
        }
        for i in 0..4 {
            out.wheel_angular_speed[i] = fa.half(WHEEL_ANGULAR_SPEED + 2 * i);
            out.tyre_slip[i] = fa.half(TYRE_SLIP_ANGLE + 2 * i);
            out.slip_ratio[i] = fa.half(SLIP_RATIO + 2 * i);
            out.nd_slip[i] = fa.half(ND_SLIP + 2 * i);
            out.load[i] = fa.half(LOAD + 2 * i);
            out.tyre_dirty_level[i] = scale(fa.0[TYRE_DIRTY_LEVEL + i], 20.0);
            out.damage_zone_level[i] = scale(fa.0[DAMAGE_ZONE_LEVEL + i], 255.0);
            out.tyre_inflation[i] = 1.0;
            let pose = |f: &ReplayFrame, orientation: usize, translation: usize| create_from_euler_safe(f.halves3(orientation + 6 * i), f.vec3(translation + 12 * i));
            out.suspension_matrix[i] = mat44f_lerp(&pose(fa, SUS_ORIENTATION, SUS_TRANSLATION), &pose(fb, SUS_ORIENTATION, SUS_TRANSLATION), t);
            out.tyre_matrix[i] = mat44f_lerp(&pose(fa, TYRE_ORIENTATION, TYRE_TRANSLATION), &pose(fb, TYRE_ORIENTATION, TYRE_TRANSLATION), t);
            // the ground under the tyre: 5 m down from 1 m above its centre
            let centre = out.tyre_matrix[i].m[3];
            if let Some((surface, pos, normal)) = track.ray_cast(&[centre[0], centre[1] + 1.0, centre[2]], &[0.0, -1.0, 0.0], 5.0) {
                out.tyre_surface_def[i] = surface;
                out.tyre_contact_point[i] = pos;
                out.tyre_contact_normal[i] = normal;
            }
        }
        out.damage_zone_level[4] = scale(fa.0[DAMAGE_ZONE_LEVEL + 4], 255.0);
        let point = |k: usize| Vec3f::new(out.tyre_contact_point[k][0], out.tyre_contact_point[k][1], out.tyre_contact_point[k][2]);
        out.ground_plane = crate::car::aero::plane4f(&point(0), &point(1), &point(2));
        // the flags, into the state and into the driver's actions
        let status = fa.status();
        let on = |b: u32| (status >> b) & 1 != 0;
        let mut actions = out.actions_state as u32;
        let mut action = |mask: u32, value: bool| {
            if value {
                actions |= mask;
            } else {
                actions &= !mask;
            }
        };
        out.is_gear_grinding = on(9);
        out.is_drift_valid = !on(10);
        action(0x10_0000, on(0));
        if on(1) {
            out.status_bytes |= 4;
        }
        if on(2) {
            out.status_bytes |= 1;
        }
        action(0x40, on(3));
        action(0x1, on(4));
        action(0x2, on(5));
        action(0x100, on(6));
        action(0x80, on(7));
        action(0x400, on(12));
        out.kers_is_charging = on(13);
        out.actions_state = actions as i32;
    }
}

/// `convertReplayFormat` 0x14015d4b0: a frame of version 15 as a frame of version 16. The
/// only differences: version 15 has a `transmissionLoad` at 0xec (dropped) and its
/// `drivetrainSpeed` at 0xee; its flags stop at bit 11.
pub fn convert_replay_format(v15: &[u8; FRAME_SIZE]) -> ReplayFrame {
    let mut f = ReplayFrame::default();
    f.0[0x00..0x12].copy_from_slice(&v15[0x00..0x12]);
    f.0[0x14..0x26].copy_from_slice(&v15[0x14..0x26]);
    f.0[0x28..0x58].copy_from_slice(&v15[0x28..0x58]);
    f.0[0x58..0x70].copy_from_slice(&v15[0x58..0x70]);
    f.0[0x70..0xa0].copy_from_slice(&v15[0x70..0xa0]);
    f.0[0xa0..0xb8].copy_from_slice(&v15[0xa0..0xb8]);
    f.0[0xb8..0xec].copy_from_slice(&v15[0xb8..0xec]);
    f.0[0xec..0xee].copy_from_slice(&v15[0xee..0xf0]);
    f.0[0xf0..0x10b].copy_from_slice(&v15[0xf0..0x10b]);
    let status = u32::from_le_bytes(v15[STATUS..STATUS + 4].try_into().unwrap()) & 0xfff;
    f.put_u32(STATUS, status);
    f
}
