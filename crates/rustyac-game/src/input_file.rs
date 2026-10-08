// SPDX-License-Identifier: GPL-3.0-or-later

//! The input file of `--record` / `--replay`: how the car was set up, then what the driver's
//! device reported in every physics step. Replaying it steps the same car through the same
//! steps, with no window, no clock and no hardware.
//!
//! Layout: a text header (`RUSTYAC-INPUT 1`, `key=value` lines, `end`), then one 40-byte record
//! per step until the end of the file.
//!
//! This file needs nothing but `rustyac-physics`: `tools/chassis_compare` includes it by path
//! to turn an oracle recording's controls into an input file.

use std::io::Write;
use std::path::Path;

use rustyac_physics::car::replay::{Ground, RunSetup};
use rustyac_physics::track::DynamicTrack;
use rustyac_physics::car::{CarControls, ChassisEnvironment};

pub const MAGIC: &str = "RUSTYAC-INPUT 1";
/// Bytes of one step's record.
pub const RECORD_SIZE: usize = 40;

/// Commands that run before a step, the way the game's main thread queues work for its
/// physics thread ([`StepInput::events`]).
pub mod event {
    /// Back to the spawn point the game's way: `Car::forceRotation`, `Car::forcePosition`.
    pub const RESET: u32 = 1 << 0;
    /// A new car (everything cold and unworn), at the spawn point.
    pub const REBUILD: u32 = 1 << 1;
    /// `TractionControl::cycleMode(1)` / `(-1)`.
    pub const TC_UP: u32 = 1 << 2;
    pub const TC_DN: u32 = 1 << 3;
    /// `ABS::cycleMode(1)` / `(-1)`.
    pub const ABS_UP: u32 = 1 << 4;
    pub const ABS_DN: u32 = 1 << 5;
    /// The automatic gearbox aid on / off.
    pub const AUTO_SHIFTER: u32 = 1 << 6;
    /// The driver holds the clutch himself in this step: the automatic clutch aid stands
    /// back (it would overwrite the pedal), and takes over again in the first step without
    /// this bit.
    pub const MANUAL_CLUTCH: u32 = 1 << 7;
    /// Back onto the track: the car is put on the nearest point of the AI line, facing along it.
    pub const TO_TRACK: u32 = 1 << 8;
    /// The cockpit's engine-brake setting one up / down (`CarAvatar::cycleEngineBrake`).
    pub const ENGINE_BRAKE_UP: u32 = 1 << 9;
    pub const ENGINE_BRAKE_DN: u32 = 1 << 10;
    /// The MGU-K delivery profile, next / previous (`CarAvatar::cycleERSPower`).
    pub const MGUK_DELIVERY_UP: u32 = 1 << 11;
    pub const MGUK_DELIVERY_DN: u32 = 1 << 12;
    /// The MGU-K recovery level up / down (`CarAvatar::cycleERSRecovery`).
    pub const MGUK_RECOVERY_UP: u32 = 1 << 13;
    pub const MGUK_RECOVERY_DN: u32 = 1 << 14;
    /// The MGU-H between battery and motor (`CarAvatar::cycleERSHeatCharging`).
    pub const MGUH_MODE: u32 = 1 << 15;

    pub const NAMES: [(u32, &str); 16] = [
        (RESET, "reset"),
        (REBUILD, "rebuild"),
        (TC_UP, "tc+"),
        (TC_DN, "tc-"),
        (ABS_UP, "abs+"),
        (ABS_DN, "abs-"),
        (AUTO_SHIFTER, "auto-shifter"),
        (MANUAL_CLUTCH, "manual clutch"),
        (TO_TRACK, "to track"),
        (ENGINE_BRAKE_UP, "engine brake+"),
        (ENGINE_BRAKE_DN, "engine brake-"),
        (MGUK_DELIVERY_UP, "mgu-k delivery+"),
        (MGUK_DELIVERY_DN, "mgu-k delivery-"),
        (MGUK_RECOVERY_UP, "mgu-k recovery+"),
        (MGUK_RECOVERY_DN, "mgu-k recovery-"),
        (MGUH_MODE, "mgu-h mode"),
    ];
}

/// What one physics step is given: nothing but what a driver does.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct StepInput {
    /// `Car::controls` as the device left it in `acquireControls`.
    pub controls: CarControls,
    /// The device's headlight switch is held (`getAction(4)`).
    pub headlights: bool,
    /// [`event`] bits: commands that ran before this step.
    pub events: u32,
    /// Clicks of the cockpit brake-bias control before this step.
    pub bias_clicks: i32,
    /// Which device drove (0 scripted, 1 keyboard, 2 Xbox pad, 3 DirectInput). Only a note.
    pub device: u32,
}

impl StepInput {
    pub fn to_bytes(&self) -> [u8; RECORD_SIZE] {
        let c = &self.controls;
        let flags = c.gear_up as u32 | (c.gear_dn as u32) << 1 | (c.drs as u32) << 2 | (c.kers as u32) << 3 | (self.headlights as u32) << 4;
        let words = [
            c.gas.to_bits(),
            c.brake.to_bits(),
            c.steer.to_bits(),
            c.clutch.to_bits(),
            c.hand_brake.to_bits(),
            c.requested_gear_index as u32,
            flags,
            self.events,
            self.bias_clicks as u32,
            self.device,
        ];
        let mut out = [0; RECORD_SIZE];
        for (chunk, word) in out.chunks_exact_mut(4).zip(words) {
            chunk.copy_from_slice(&word.to_le_bytes());
        }
        out
    }

    pub fn from_bytes(bytes: &[u8]) -> StepInput {
        let word = |k: usize| u32::from_le_bytes(bytes[k * 4..k * 4 + 4].try_into().unwrap());
        let flags = word(6);
        StepInput {
            controls: CarControls {
                gas: f32::from_bits(word(0)),
                brake: f32::from_bits(word(1)),
                steer: f32::from_bits(word(2)),
                clutch: f32::from_bits(word(3)),
                hand_brake: f32::from_bits(word(4)),
                requested_gear_index: word(5) as i32,
                gear_up: flags & 1 != 0,
                gear_dn: flags & 2 != 0,
                drs: flags & 4 != 0,
                kers: flags & 8 != 0,
            },
            headlights: flags & 16 != 0,
            events: word(7),
            bias_clicks: word(8) as i32,
            device: word(9),
        }
    }
}

/// A car set up the way `tools/chassis_compare` sets one up for a recording of the game:
/// the session values of that recording, and the instruments the comparison reads.
#[derive(Clone, Debug, PartialEq)]
pub struct OracleSetup {
    pub scenario: String,
    pub ground: Ground,
    pub pitlane: bool,
    pub stability_gain: f32,
    pub wind_speed: f32,
    pub wind_direction_deg: f32,
    pub damage: [f32; 5],
    /// The recording was made on a real track (`car_oracle run --track`).
    pub track: Option<OracleTrack>,
    /// The recording was made with collisions (`car_oracle run --collide`).
    pub collide: Option<OracleCollide>,
    /// The recording's session conditions and saved setup (Task 15).
    pub conditions: rustyac_physics::car::replay::Conditions,
}

/// How the car of an oracle recording touched things.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OracleCollide {
    /// The car's own `collider.kn5` (from the game's folder), not the older recordings' stand-in.
    pub collider_mesh: bool,
    /// `bounce_vel` of the game's mesh contact joints in that run, as bits (the game never
    /// writes it: it is whatever its stack held).
    pub mesh_bounce_vel: u32,
    /// `PhysicsCore::setNoCollisionSteps` at the start.
    pub no_collision_steps: i32,
    /// The oracle's flat test floor as a real mesh.
    pub floor: bool,
}

/// The track of an oracle recording and where its car was put.
#[derive(Clone, Debug, PartialEq)]
pub struct OracleTrack {
    /// The track's folder.
    pub folder: String,
    /// The spawn: a point on the road and the direction of the car's tail.
    pub position: [f32; 3],
    pub tail: [f32; 3],
    /// The first lap is armed (a hot-lap start).
    pub armed: bool,
    /// `PhysicsEngine::allowedTyresOut`
    pub allowed_tyres_out: i32,
}

fn hex3(values: &[f32; 3]) -> String {
    values.map(|v| format!("{:08x}", v.to_bits())).join(",")
}

fn parse_hex3(text: &str) -> Result<[f32; 3], String> {
    let words: Vec<f32> = text.split(',').filter_map(|w| u32::from_str_radix(w, 16).ok()).map(f32::from_bits).collect();
    <[f32; 3]>::try_from(words).map_err(|_| format!("three hexadecimal words expected, got {text:?}"))
}

/// What the game's session files (`cfg/race.ini`, `cfg/assists.ini`), a saved setup and the
/// command line add to a live drive (Task 15). The default is the drive of before: no wind, a
/// fixed grip (`env.dynamic_grip_level`), the car's own aids, the default setup.
#[derive(Clone, Debug, PartialEq)]
pub struct Session {
    /// The wind handed to `PhysicsEngine::setWind`: m/s (0: none) and degrees.
    pub wind_speed: f32,
    pub wind_direction_deg: f32,
    /// The track's grip of the session (`[DYNAMIC_TRACK]` of race.ini, already drawn); `None`:
    /// the grip is the fixed number in `env`.
    pub dynamic_track: Option<DynamicTrack>,
    /// `[CAR_0] BALLAST` (kg), `RESTRICTOR`; 0: none.
    pub ballast_kg: f32,
    pub restrictor: f32,
    /// `[RACE] PENALTIES`: on a track, two tyres may leave it; off: any number.
    pub penalties: bool,
    /// assists.ini `ABS`, `TRACTION_CONTROL` (0 off, 1 as the car has it, 2 on) and
    /// `STABILITY_CONTROL` (percent); `None`: the car's own files decide.
    pub assists: Option<(i32, i32, f32)>,
    /// A saved setup, loaded after the default one.
    pub setup_file: Option<std::path::PathBuf>,
    /// `[SESSION_0] TYPE` of race.ini (1 practice ... 4 hot-lap); `None`: a hot-lap session
    /// when the car starts at the hot-lap start, else practice.
    pub session_type: Option<i32>,
    /// race.ini has a hot-lap session (any `[SESSION_n] TYPE=4`): the first lap is armed.
    /// `None`: armed when the session's type is 4.
    pub arm_first_lap: Option<bool>,
}

impl Default for Session {
    fn default() -> Session {
        Session { wind_speed: 0.0, wind_direction_deg: 0.0, dynamic_track: None, ballast_kg: 0.0, restrictor: 0.0, penalties: true, assists: None, setup_file: None, session_type: None, arm_first_lap: None }
    }
}

/// Numbers of a header line written as bit patterns (`41200000,43020000`).
fn hex_floats(key: &str, text: &str) -> Result<Vec<f32>, String> {
    rustyac_physics::car::replay::Conditions::parse_hex(text).map_err(|e| format!("{key}: {e}"))
}

/// Everything that decides what car is built and how its session starts.
#[derive(Clone, Debug, PartialEq)]
pub struct SimSetup {
    /// The car: its folder name in the game's `content/cars` (or under `cardata/`), or a path.
    pub car: String,
    /// `srand` seed of the C runtime when the car is built.
    pub seed: u32,
    /// The physics clock before the first step, ms; step `n` runs at `clock + 3 * (n + 1)`.
    pub clock_start_ms: f64,
    /// The session's values. Only the ones written to the header can differ from the default.
    pub env: ChassisEnvironment,
    /// The "automatic clutch" driving aid.
    pub auto_clutch: bool,
    /// The "automatic gearbox" driving aid.
    pub auto_shifter: bool,
    /// The "automatic throttle blip" driving aid (`assists.ini AUTO_BLIP` ->
    /// `AutoBlip::isActive`); `None`: as the car's loader leaves it (on).
    pub auto_blip: Option<bool>,
    /// The session starts when the car is put down (`PhysicsEngine::sessionInfo.startTimeMS` =
    /// the clock then, as `RaceManager::setCurrentSession` sets it); false: at clock 0, as in
    /// files recorded before this was known.
    pub session_starts_at_spawn: bool,
    /// `getFFGlobalGain` of the driver's device (a wheel's force-feedback gain; else 1).
    pub ff_gain: f32,
    pub oracle: Option<OracleSetup>,
    /// The track: a folder, or a name under the game's `content/tracks`. Empty: the endless
    /// flat road.
    pub track: String,
    /// Where on the track the car starts: `hotlap`, `pit` or `start`.
    pub spawn: String,
    /// The session's wind, track grip, ballast, aids and saved setup.
    pub session: Session,
}

impl Default for SimSetup {
    /// The F2004 with the oracle's defaults: 26 deg C air, 30 deg C road, grip 1, no wind,
    /// seed 1, the clock at one minute, the automatic clutch on.
    fn default() -> SimSetup {
        SimSetup {
            car: "ks_ferrari_f2004".to_string(),
            seed: 1,
            clock_start_ms: 60_000.0,
            env: ChassisEnvironment::default(),
            auto_clutch: true,
            auto_shifter: false,
            auto_blip: None,
            session_starts_at_spawn: false,
            ff_gain: 1.0,
            oracle: None,
            track: String::new(),
            spawn: "hotlap".to_string(),
            session: Session::default(),
        }
    }
}

impl SimSetup {
    /// The physics clock of a step, ms.
    pub fn time_of_step(&self, step: u64) -> f64 {
        self.clock_start_ms + (step as f64 + 1.0) * 3.0
    }

    /// The same car as `tools/chassis_compare` builds it for a recording (oracle set-ups only).
    pub fn run_setup(&self) -> Option<RunSetup> {
        let oracle = self.oracle.as_ref()?;
        Some(RunSetup {
            scenario: oracle.scenario.clone(),
            ground: oracle.ground,
            seed: self.seed,
            clock_start_ms: self.clock_start_ms,
            env: self.env,
            rust_brakes: true,
            rust_drivetrain: true,
            rust_aero: true,
            rust_aids: true,
            telemetry: true,
            pitlane: oracle.pitlane,
            stability_gain: oracle.stability_gain,
            wind_speed: oracle.wind_speed,
            wind_direction_deg: oracle.wind_direction_deg,
            damage: oracle.damage,
            auto_clutch: self.auto_clutch,
            auto_shifter: self.auto_shifter,
            track: None,
            // (the collider mesh is the game's file: `spawn_car` reads it)
            collide: match &oracle.collide {
                Some(c) => rustyac_physics::car::replay::CollideRun {
                    on: true,
                    mesh: None,
                    mesh_bounce_vel: f32::from_bits(c.mesh_bounce_vel),
                    no_collision_steps: c.no_collision_steps,
                    floor: c.floor,
                },
                None => Default::default(),
            },
            conditions: oracle.conditions.clone(),
        })
    }

    pub fn header(&self) -> String {
        let e = &self.env;
        let mut out = format!("{MAGIC}\n");
        let mut put = |key: &str, value: String| {
            out.push_str(key);
            out.push('=');
            out.push_str(&value);
            out.push('\n');
        };
        put("car", self.car.clone());
        put("dt", "0.003".to_string());
        put("seed", self.seed.to_string());
        put("clock_start_ms", format!("{:?}", self.clock_start_ms));
        put("ambient_temperature", format!("{:?}", e.ambient_temperature));
        put("road_temperature", format!("{:?}", e.road_temperature));
        put("dynamic_grip_level", format!("{:?}", e.dynamic_grip_level));
        put("tyre_consumption_rate", format!("{:?}", e.tyre_consumption_rate));
        put("mechanical_damage_rate", format!("{:?}", e.mechanical_damage_rate));
        put("fuel_consumption_rate", format!("{:?}", e.fuel_consumption_rate));
        put("allow_tyre_blankets", (e.allow_tyre_blankets as u32).to_string());
        put("penalty_mode", e.penalty_mode.to_string());
        put("ff_filter", format!("{:?}", e.ff_filter));
        put("use_fake_understeer_ff", (e.use_fake_understeer_ff as u32).to_string());
        put("damper_min_value", format!("{:?}", e.damper_min_value));
        put("damper_gain", format!("{:?}", e.damper_gain));
        put("auto_clutch", (self.auto_clutch as u32).to_string());
        put("auto_shifter", (self.auto_shifter as u32).to_string());
        if let Some(blip) = self.auto_blip {
            put("auto_blip", (blip as u32).to_string());
        }
        if self.session_starts_at_spawn {
            put("session_starts_at_spawn", "1".to_string());
        }
        put("ff_gain", format!("{:?}", self.ff_gain));
        // the session's additions, each only when it is not the default (older files have none)
        let s = &self.session;
        let hex = |values: &[f32]| values.iter().map(|x| format!("{:08x}", x.to_bits())).collect::<Vec<_>>().join(",");
        if s.wind_speed != 0.0 {
            put("wind", hex(&[s.wind_speed, s.wind_direction_deg]));
        }
        if let Some(density) = e.air_density_override {
            put("air_density_override", hex(&[density]));
        }
        if let Some((at_26, per_degree)) = e.experiment_tyre_pressure_law {
            put("experiment_tyre_pressure_law", hex(&[at_26, per_degree]));
        }
        if let Some(t) = &s.dynamic_track {
            put(
                "dynamic_track",
                format!("{},{},{}", t.is_external as u32, t.enabled as u32, hex(&[t.session_start_grip, t.base_grip, t.random_grip, t.grip_per_lap, t.session_transfer, t.dynamic_grip_level])),
            );
        }
        if s.ballast_kg != 0.0 {
            put("ballast_kg", hex(&[s.ballast_kg]));
        }
        if s.restrictor != 0.0 {
            put("restrictor", hex(&[s.restrictor]));
        }
        if !s.penalties {
            put("penalties", "0".to_string());
        }
        if let Some((abs, traction_control, stability)) = s.assists {
            put("assists", format!("{abs},{traction_control},{}", hex(&[stability])));
        }
        if let Some(file) = &s.setup_file {
            put("setup_file", file.display().to_string());
        }
        if let Some(session_type) = s.session_type {
            put("session_type", session_type.to_string());
        }
        if let Some(armed) = s.arm_first_lap {
            put("arm_first_lap", (armed as u32).to_string());
        }
        if !self.track.is_empty() {
            put("track", self.track.clone());
            put("spawn", self.spawn.clone());
        }
        if let Some(o) = &self.oracle {
            put("oracle_scenario", o.scenario.clone());
            put("oracle_ground", o.ground.describe());
            put("oracle_pitlane", (o.pitlane as u32).to_string());
            put("oracle_stability_gain", format!("{:?}", o.stability_gain));
            put("oracle_wind_speed", format!("{:?}", o.wind_speed));
            put("oracle_wind_direction_deg", format!("{:?}", o.wind_direction_deg));
            put("oracle_damage", o.damage.map(|d| format!("{d:?}")).join(","));
            if o.conditions != Default::default() {
                put("oracle_conditions", o.conditions.encode());
            }
            if let Some(track) = &o.track {
                put("oracle_track_folder", track.folder.clone());
                put("oracle_spawn_position", hex3(&track.position));
                put("oracle_spawn_tail", hex3(&track.tail));
                put("oracle_armed", (track.armed as u32).to_string());
                put("oracle_allowed_tyres_out", track.allowed_tyres_out.to_string());
            }
            if let Some(c) = &o.collide {
                put("oracle_collide", "1".to_string());
                put("oracle_collider_mesh", (c.collider_mesh as u32).to_string());
                put("oracle_mesh_bounce_vel", format!("{:08x}", c.mesh_bounce_vel));
                put("oracle_no_collision_steps", c.no_collision_steps.to_string());
                put("oracle_floor", (c.floor as u32).to_string());
            }
        }
        out.push_str("end\n");
        out
    }

    /// Reads a header; returns the set-up and the number of bytes the header takes.
    pub fn parse_header(bytes: &[u8]) -> Result<(SimSetup, usize), String> {
        let mut setup = SimSetup::default();
        let mut oracle: Option<OracleSetup> = None;
        let mut at = 0;
        let mut first = true;
        loop {
            let end = bytes[at..].iter().position(|b| *b == b'\n').ok_or("the header has no end line")? + at;
            let line = std::str::from_utf8(&bytes[at..end]).map_err(|e| format!("the header is not text: {e}"))?;
            let line = line.trim_end_matches('\r');
            at = end + 1;
            if first {
                if line != MAGIC {
                    return Err(format!("not a rustyAC input file (first line {line:?}, expected {MAGIC:?})"));
                }
                first = false;
                continue;
            }
            if line == "end" {
                break;
            }
            let (key, value) = line.split_once('=').ok_or(format!("header line without '=': {line:?}"))?;
            let float = || value.parse::<f32>().map_err(|e| format!("{key}: {e}"));
            let flag = || value != "0";
            let e = &mut setup.env;
            let blank = || OracleSetup {
                scenario: String::new(),
                ground: Ground::Flat,
                pitlane: false,
                stability_gain: 0.0,
                wind_speed: 0.0,
                wind_direction_deg: 0.0,
                damage: [0.0; 5],
                conditions: Default::default(),
                track: None,
                collide: None,
            };
            let blank_collide = || OracleCollide { collider_mesh: false, mesh_bounce_vel: 0, no_collision_steps: 0, floor: false };
            let blank_track = || OracleTrack { folder: String::new(), position: [0.0; 3], tail: [0.0, 0.0, -1.0], armed: false, allowed_tyres_out: -1 };
            match key {
                "car" => setup.car = value.to_string(),
                "dt" => {
                    if value != "0.003" {
                        return Err(format!("dt={value}: only AC's 0.003 s step is supported"));
                    }
                }
                "seed" => setup.seed = value.parse().map_err(|e| format!("seed: {e}"))?,
                "clock_start_ms" => setup.clock_start_ms = value.parse().map_err(|e| format!("clock_start_ms: {e}"))?,
                "ambient_temperature" => e.ambient_temperature = float()?,
                "road_temperature" => e.road_temperature = float()?,
                "dynamic_grip_level" => e.dynamic_grip_level = float()?,
                "tyre_consumption_rate" => e.tyre_consumption_rate = float()?,
                "mechanical_damage_rate" => e.mechanical_damage_rate = float()?,
                "fuel_consumption_rate" => e.fuel_consumption_rate = float()?,
                "allow_tyre_blankets" => e.allow_tyre_blankets = flag(),
                "penalty_mode" => e.penalty_mode = value.parse().map_err(|e| format!("penalty_mode: {e}"))?,
                "ff_filter" => e.ff_filter = float()?,
                "use_fake_understeer_ff" => e.use_fake_understeer_ff = flag(),
                "damper_min_value" => e.damper_min_value = float()?,
                "damper_gain" => e.damper_gain = float()?,
                "auto_clutch" => setup.auto_clutch = flag(),
                "auto_shifter" => setup.auto_shifter = flag(),
                "auto_blip" => setup.auto_blip = Some(flag()),
                "session_starts_at_spawn" => setup.session_starts_at_spawn = flag(),
                "session_type" => setup.session.session_type = Some(value.parse().map_err(|e| format!("session_type: {e}"))?),
                "arm_first_lap" => setup.session.arm_first_lap = Some(flag()),
                "ff_gain" => setup.ff_gain = float()?,
                "track" => setup.track = value.to_string(),
                "spawn" => setup.spawn = value.to_string(),
                "wind" => {
                    let [speed, direction] = hex_floats(key, value)?[..] else { return Err(format!("wind: two numbers expected, got {value:?}")) };
                    setup.session.wind_speed = speed;
                    setup.session.wind_direction_deg = direction;
                }
                "dynamic_track" => {
                    let parts: Vec<&str> = value.splitn(3, ',').collect();
                    let [external, enabled, numbers] = parts[..] else { return Err(format!("dynamic_track: {value:?}")) };
                    let [session_start_grip, base_grip, random_grip, grip_per_lap, session_transfer, dynamic_grip_level] = hex_floats(key, numbers)?[..] else {
                        return Err(format!("dynamic_track: six numbers expected, got {numbers:?}"));
                    };
                    setup.session.dynamic_track = Some(DynamicTrack {
                        is_external: external != "0",
                        enabled: enabled != "0",
                        session_start_grip,
                        base_grip,
                        random_grip,
                        grip_per_lap,
                        session_transfer,
                        dynamic_grip_level,
                    });
                }
                "ballast_kg" => setup.session.ballast_kg = hex_floats(key, value)?.first().copied().unwrap_or(0.0),
                "restrictor" => setup.session.restrictor = hex_floats(key, value)?.first().copied().unwrap_or(0.0),
                "penalties" => setup.session.penalties = flag(),
                "air_density_override" => e.air_density_override = hex_floats(key, value)?.first().copied(),
                "experiment_tyre_pressure_law" => {
                    let [at_26, per_degree] = hex_floats(key, value)?[..] else { return Err(format!("experiment_tyre_pressure_law: two numbers expected, got {value:?}")) };
                    e.experiment_tyre_pressure_law = Some((at_26, per_degree));
                }
                "assists" => {
                    let parts: Vec<&str> = value.split(',').collect();
                    let [abs, traction_control, stability] = parts[..] else { return Err(format!("assists: {value:?}")) };
                    let int = |text: &str| text.parse::<i32>().map_err(|e| format!("assists: {e}"));
                    setup.session.assists = Some((int(abs)?, int(traction_control)?, hex_floats(key, stability)?.first().copied().unwrap_or(0.0)));
                }
                "setup_file" => setup.session.setup_file = Some(std::path::PathBuf::from(value)),
                "oracle_scenario" => oracle.get_or_insert_with(blank).scenario = value.to_string(),
                "oracle_collide" => {
                    oracle.get_or_insert_with(blank).collide.get_or_insert_with(blank_collide);
                }
                "oracle_collider_mesh" => oracle.get_or_insert_with(blank).collide.get_or_insert_with(blank_collide).collider_mesh = flag(),
                "oracle_mesh_bounce_vel" => {
                    oracle.get_or_insert_with(blank).collide.get_or_insert_with(blank_collide).mesh_bounce_vel =
                        u32::from_str_radix(value, 16).map_err(|e| format!("oracle_mesh_bounce_vel: {e}"))?
                }
                "oracle_no_collision_steps" => {
                    oracle.get_or_insert_with(blank).collide.get_or_insert_with(blank_collide).no_collision_steps =
                        value.parse().map_err(|e| format!("oracle_no_collision_steps: {e}"))?
                }
                "oracle_floor" => oracle.get_or_insert_with(blank).collide.get_or_insert_with(blank_collide).floor = flag(),
                "oracle_ground" => {
                    oracle.get_or_insert_with(blank).ground = Ground::parse(value).ok_or(format!("oracle_ground: {value:?}"))?
                }
                "oracle_pitlane" => oracle.get_or_insert_with(blank).pitlane = flag(),
                "oracle_conditions" => oracle.get_or_insert_with(blank).conditions = rustyac_physics::car::replay::Conditions::decode(value)?,
                "oracle_stability_gain" => oracle.get_or_insert_with(blank).stability_gain = float()?,
                "oracle_wind_speed" => oracle.get_or_insert_with(blank).wind_speed = float()?,
                "oracle_wind_direction_deg" => oracle.get_or_insert_with(blank).wind_direction_deg = float()?,
                "oracle_damage" => {
                    let mut damage = [0.0; 5];
                    let parts: Vec<&str> = value.split(',').collect();
                    if parts.len() != 5 {
                        return Err(format!("oracle_damage: five numbers expected, got {value:?}"));
                    }
                    for (d, part) in damage.iter_mut().zip(parts) {
                        *d = part.parse().map_err(|e| format!("oracle_damage: {e}"))?;
                    }
                    oracle.get_or_insert_with(blank).damage = damage;
                }
                "oracle_track_folder" => oracle.get_or_insert_with(blank).track.get_or_insert_with(blank_track).folder = value.to_string(),
                "oracle_spawn_position" => oracle.get_or_insert_with(blank).track.get_or_insert_with(blank_track).position = parse_hex3(value)?,
                "oracle_spawn_tail" => oracle.get_or_insert_with(blank).track.get_or_insert_with(blank_track).tail = parse_hex3(value)?,
                "oracle_armed" => oracle.get_or_insert_with(blank).track.get_or_insert_with(blank_track).armed = flag(),
                "oracle_allowed_tyres_out" => {
                    oracle.get_or_insert_with(blank).track.get_or_insert_with(blank_track).allowed_tyres_out =
                        value.parse().map_err(|e| format!("oracle_allowed_tyres_out: {e}"))?
                }
                // a newer writer's key: an input file must never be half understood
                other => return Err(format!("unknown header key {other:?}")),
            }
        }
        setup.oracle = oracle;
        Ok((setup, at))
    }
}

/// A whole input file in memory.
#[derive(Clone, Debug, PartialEq)]
pub struct InputFile {
    pub setup: SimSetup,
    pub steps: Vec<StepInput>,
}

impl InputFile {
    pub fn read(path: &Path) -> Result<InputFile, String> {
        let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        InputFile::parse(&bytes).map_err(|e| format!("{}: {e}", path.display()))
    }

    pub fn parse(bytes: &[u8]) -> Result<InputFile, String> {
        let (setup, at) = SimSetup::parse_header(bytes)?;
        let body = &bytes[at..];
        if !body.len().is_multiple_of(RECORD_SIZE) {
            return Err(format!("{} bytes after the header are not a whole number of {RECORD_SIZE}-byte steps", body.len()));
        }
        Ok(InputFile { setup, steps: body.chunks_exact(RECORD_SIZE).map(StepInput::from_bytes).collect() })
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = self.setup.header().into_bytes();
        for step in &self.steps {
            out.extend(step.to_bytes());
        }
        out
    }

    pub fn write(&self, path: &Path) -> Result<(), String> {
        std::fs::write(path, self.to_bytes()).map_err(|e| format!("{}: {e}", path.display()))
    }
}

/// Writes an input file step by step while a drive runs.
pub struct InputWriter {
    file: std::io::BufWriter<std::fs::File>,
    pub steps: u64,
}

impl InputWriter {
    pub fn create(path: &Path, setup: &SimSetup) -> Result<InputWriter, String> {
        let file = std::fs::File::create(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let mut file = std::io::BufWriter::new(file);
        file.write_all(setup.header().as_bytes()).map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(InputWriter { file, steps: 0 })
    }

    pub fn push(&mut self, step: &StepInput) -> Result<(), String> {
        self.steps += 1;
        self.file.write_all(&step.to_bytes()).map_err(|e| format!("writing the input file: {e}"))
    }

    pub fn finish(mut self) -> Result<u64, String> {
        self.file.flush().map_err(|e| format!("writing the input file: {e}"))?;
        Ok(self.steps)
    }
}
