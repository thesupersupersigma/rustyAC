// SPDX-License-Identifier: MIT OR Apache-2.0

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

    pub const NAMES: [(u32, &str); 9] = [
        (RESET, "reset"),
        (REBUILD, "rebuild"),
        (TC_UP, "tc+"),
        (TC_DN, "tc-"),
        (ABS_UP, "abs+"),
        (ABS_DN, "abs-"),
        (AUTO_SHIFTER, "auto-shifter"),
        (MANUAL_CLUTCH, "manual clutch"),
        (TO_TRACK, "to track"),
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

/// Everything that decides what car is built and how its session starts.
#[derive(Clone, Debug, PartialEq)]
pub struct SimSetup {
    /// The car's folder under `cardata/`.
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
    /// `getFFGlobalGain` of the driver's device (a wheel's force-feedback gain; else 1).
    pub ff_gain: f32,
    pub oracle: Option<OracleSetup>,
    /// The track: a folder, or a name under the game's `content/tracks`. Empty: the endless
    /// flat road.
    pub track: String,
    /// Where on the track the car starts: `hotlap`, `pit` or `start`.
    pub spawn: String,
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
            ff_gain: 1.0,
            oracle: None,
            track: String::new(),
            spawn: "hotlap".to_string(),
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
        put("ff_gain", format!("{:?}", self.ff_gain));
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
            if let Some(track) = &o.track {
                put("oracle_track_folder", track.folder.clone());
                put("oracle_spawn_position", hex3(&track.position));
                put("oracle_spawn_tail", hex3(&track.tail));
                put("oracle_armed", (track.armed as u32).to_string());
                put("oracle_allowed_tyres_out", track.allowed_tyres_out.to_string());
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
                track: None,
            };
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
                "ff_gain" => setup.ff_gain = float()?,
                "track" => setup.track = value.to_string(),
                "spawn" => setup.spawn = value.to_string(),
                "oracle_scenario" => oracle.get_or_insert_with(blank).scenario = value.to_string(),
                "oracle_ground" => {
                    oracle.get_or_insert_with(blank).ground = Ground::parse(value).ok_or(format!("oracle_ground: {value:?}"))?
                }
                "oracle_pitlane" => oracle.get_or_insert_with(blank).pitlane = flag(),
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
