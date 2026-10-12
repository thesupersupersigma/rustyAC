// SPDX-License-Identifier: GPL-3.0-or-later

//! Recordings of several cars (`.mcrec`) and their replay through [`PhysicsEngine`].
//!
//! `tools/car_oracle multi` lets the game's own `PhysicsEngine` drive 2 to 6 cars and writes,
//! for every step, what each car's device reported and what the game's state was after the
//! step: every body of every car, the damage, the tyres, the engine, the collision switches,
//! and the contact joints of the world. [`replay`] builds the same cars in a
//! [`PhysicsEngine`], feeds it nothing but the recorded controls from step 0 on, and holds
//! every value of every step against the recording, bit for bit.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::{CarSlot, DeviceRef, PhysicsEngine};
use crate::car::colliders::ColliderMesh;
use crate::car::{ChassisEnvironment, RollingChassis, ScriptedDevice};
use crate::ode::joint::JOINT_REVERSE;
use crate::ode::JointKind;
use crate::track::{Track, TrackGround};
use crate::vecmath::Vec3f;

pub const MAGIC: &[u8; 8] = b"RYMCREC1";

/// The values of the world in a row, before the cars'.
pub const WORLD_COLUMNS: [&str; 5] = ["world.contactJoints", "world.contactHash", "world.meshBounceVel", "world.noCollisionCounter", "world.currentFrame"];

/// Task 28: the referee's state after the step, at the end of a row: a hash over every field
/// of the game's `RaceManager` and `RaceTimingServices` and of what its jobs leave in the cars.
pub const REFEREE_COLUMNS: [&str; 2] = ["world.referee.lo", "world.referee.hi"];

/// What a car's device reported in a step, in a row.
pub const INPUT_COLUMNS: [&str; 7] = ["in.gas", "in.brake", "in.steer", "in.clutch", "in.handBrake", "in.buttons", "in.requestedGear"];

/// One car of a recording.
#[derive(Clone, Debug, PartialEq)]
pub struct CarMeta {
    /// The car's folder name under `cardata/`.
    pub name: String,
    /// How many rigid bodies the car has.
    pub bodies: usize,
    /// The state of the C runtime's `rand()` when the game built the car.
    pub seed: u32,
    pub position: [f32; 3],
    pub tail: [f32; 3],
    pub auto_clutch: bool,
    pub auto_shifter: bool,
    /// The car has its own collider mesh (the game's `collider.kn5`).
    pub collider_mesh: bool,
    /// Task 27: the car is driven by the game's `AIDriver` (and here by the port's): its
    /// inputs in a row are what the driver left in `Car::controls`, and its row has the
    /// driver's members ([`crate::ai::oracle`]).
    pub ai: Option<AiMeta>,
    /// Task 27: `Car::pitPosition` (the car's pit box), row by row
    pub pit: Option<[f32; 16]>,
    /// Task 27: the car gets its AI driver during the run (`ai_handover`: the recording's
    /// first car): its row has the driver's columns from the start, zero until then
    pub handover: bool,
    /// Task 27: the car is driven by the game's `AIDriverDrag`: its `push` and the
    /// milliseconds from the clock's start to the time `setStartTime` was called with
    pub drag: Option<(f32, f32)>,
}

/// An AI driver of a recording.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AiMeta {
    /// `AIDriver::basePush`, `AIDriver::aggression` as they were set after the constructor
    pub base_push: f32,
    pub aggression: f32,
    /// `Car::expectedFuelPerLap` as the game's `Car::Car` worked it out
    pub expected_fuel_per_lap: f32,
}

/// A car's columns after its inputs: its state, and an AI driver's members.
pub fn car_columns(car: &CarMeta) -> Vec<String> {
    let mut out = state_columns(car.bodies);
    if car.ai.is_some() || car.handover {
        out.extend(crate::ai::oracle::column_names());
    }
    out
}

/// A recording: the header's keys, the cars, and one row of 32-bit words per step.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Recording {
    pub header: Vec<(String, String)>,
    pub cars: Vec<CarMeta>,
    pub rows: Vec<Vec<u32>>,
}

fn hex3(words: &[f32; 3]) -> String {
    words.iter().map(|x| format!("{:08x}", x.to_bits())).collect::<Vec<_>>().join(",")
}

fn parse_hex3(text: &str) -> Result<[f32; 3], String> {
    let mut out = [0.0f32; 3];
    let parts: Vec<&str> = text.split(',').collect();
    if parts.len() != 3 {
        return Err(format!("{text:?}: not three values"));
    }
    for (slot, part) in out.iter_mut().zip(parts) {
        *slot = f32::from_bits(u32::from_str_radix(part, 16).map_err(|e| format!("{part:?}: {e}"))?);
    }
    Ok(out)
}

/// The names of a car's state columns: `bodies` rigid bodies, then the car's own values.
pub fn state_columns(bodies: usize) -> Vec<String> {
    let mut out = Vec::new();
    for body in 0..bodies {
        for name in ["pos.x", "pos.y", "pos.z", "q.0", "q.1", "q.2", "q.3", "lvel.x", "lvel.y", "lvel.z", "avel.x", "avel.y", "avel.z"] {
            out.push(format!("body{body}.{name}"));
        }
    }
    for zone in 0..5 {
        out.push(format!("damage.{zone}"));
    }
    for wheel in 0..4 {
        out.push(format!("sus{wheel}.damage"));
    }
    for wheel in 0..4 {
        for name in ["angularVelocity", "load", "ndSlip", "slipRatio"] {
            out.push(format!("tyre{wheel}.{name}"));
        }
    }
    for name in [
        "engine.velocity.lo",
        "engine.velocity.hi",
        "gear",
        "fuel.lo",
        "fuel.hi",
        "speed",
        "meshCollideBits",
        "collisionOffForPits",
        "lastCollisionTime.lo",
        "lastCollisionTime.hi",
        "lastCollisionWithCarTime.lo",
        "lastCollisionWithCarTime.hi",
        "lapCount",
        "npos",
    ] {
        out.push(name.to_string());
    }
    out
}

/// One contact joint of the world for [`contact_words`].
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ContactNote {
    pub pos: [f32; 3],
    pub normal: [f32; 3],
    pub depth: f32,
    /// The car a body of the joint belongs to; -1 the track (no body), -2 a loose object.
    pub car1: i32,
    pub car2: i32,
    pub reversed: bool,
    pub mode: i32,
    pub mu: f32,
    pub bounce: f32,
    /// `surface.bounce_vel`, the field the game never writes.
    pub bounce_vel: f32,
    pub soft_cfm: f32,
}

/// The first three world columns from the contact joints that exist after a step (newest
/// first, the order of ODE's joint list): their number, a hash over everything of every
/// joint, and the `bounce_vel` of the newest joint that bounces (a mesh contact).
pub fn contact_words(contacts: &[ContactNote]) -> [u32; 3] {
    let mut hash: u32 = 0x811c_9dc5;
    let mut bounce_vel = 0u32;
    let mut found = false;
    for c in contacts {
        let bounces = c.bounce != 0.0;
        if bounces && !found {
            bounce_vel = c.bounce_vel.to_bits();
            found = true;
        }
        let words = [
            c.pos[0].to_bits(),
            c.pos[1].to_bits(),
            c.pos[2].to_bits(),
            c.normal[0].to_bits(),
            c.normal[1].to_bits(),
            c.normal[2].to_bits(),
            c.depth.to_bits(),
            c.car1 as u32,
            c.car2 as u32,
            c.reversed as u32,
            c.mode as u32,
            c.mu.to_bits(),
            c.bounce.to_bits(),
            // only a contact that bounces reads the field
            if bounces { c.bounce_vel.to_bits() } else { 0 },
            c.soft_cfm.to_bits(),
        ];
        for word in words {
            for byte in word.to_le_bytes() {
                hash ^= byte as u32;
                hash = hash.wrapping_mul(0x0100_0193);
            }
        }
    }
    [contacts.len() as u32, hash, bounce_vel]
}

impl Recording {
    pub fn get(&self, key: &str) -> Option<&str> {
        self.header.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
    }

    fn need(&self, key: &str) -> Result<&str, String> {
        self.get(key).ok_or_else(|| format!("the recording has no header key {key:?}"))
    }

    pub fn set(&mut self, key: &str, value: impl Into<String>) {
        let value = value.into();
        match self.header.iter_mut().find(|(k, _)| k == key) {
            Some(entry) => entry.1 = value,
            None => self.header.push((key.to_string(), value)),
        }
    }

    /// Writes the cars into the header (`cars`, `car.<n>.*`).
    pub fn set_cars(&mut self, cars: &[CarMeta]) {
        self.cars = cars.to_vec();
        self.set("cars", cars.len().to_string());
        for (n, car) in cars.iter().enumerate() {
            self.set(&format!("car.{n}.name"), car.name.clone());
            self.set(&format!("car.{n}.bodies"), car.bodies.to_string());
            self.set(&format!("car.{n}.seed"), car.seed.to_string());
            self.set(&format!("car.{n}.position"), hex3(&car.position));
            self.set(&format!("car.{n}.tail"), hex3(&car.tail));
            self.set(&format!("car.{n}.auto_clutch"), (car.auto_clutch as u8).to_string());
            self.set(&format!("car.{n}.auto_shifter"), (car.auto_shifter as u8).to_string());
            self.set(&format!("car.{n}.collider_mesh"), (car.collider_mesh as u8).to_string());
            if let Some(ai) = &car.ai {
                self.set(&format!("car.{n}.ai"), hex3(&[ai.base_push, ai.aggression, ai.expected_fuel_per_lap]));
            }
            if let Some((push, start)) = car.drag {
                self.set(&format!("car.{n}.drag"), format!("{:08x},{:08x}", push.to_bits(), start.to_bits()));
            }
            if let Some(pit) = &car.pit {
                self.set(&format!("car.{n}.pit"), pit.iter().map(|x| format!("{:08x}", x.to_bits())).collect::<Vec<_>>().join(","));
            }
        }
    }

    fn cars_from_header(&self) -> Result<Vec<CarMeta>, String> {
        let count: usize = self.need("cars")?.parse().map_err(|e| format!("cars: {e}"))?;
        let mut cars = Vec::new();
        for n in 0..count {
            let key = |name: &str| format!("car.{n}.{name}");
            let flag = |name: &str| -> Result<bool, String> { Ok(self.need(&key(name))? == "1") };
            cars.push(CarMeta {
                name: self.need(&key("name"))?.to_string(),
                bodies: self.need(&key("bodies"))?.parse().map_err(|e| format!("bodies: {e}"))?,
                seed: self.need(&key("seed"))?.parse().map_err(|e| format!("seed: {e}"))?,
                position: parse_hex3(self.need(&key("position"))?)?,
                tail: parse_hex3(self.need(&key("tail"))?)?,
                auto_clutch: flag("auto_clutch")?,
                auto_shifter: flag("auto_shifter")?,
                collider_mesh: flag("collider_mesh")?,
                ai: match self.get(&key("ai")) {
                    Some(text) => {
                        let v = parse_hex3(text)?;
                        Some(AiMeta { base_push: v[0], aggression: v[1], expected_fuel_per_lap: v[2] })
                    }
                    None => None,
                },
                pit: match self.get(&key("pit")) {
                    Some(text) => {
                        let mut pit = [0.0f32; 16];
                        let parts: Vec<&str> = text.split(',').collect();
                        if parts.len() != 16 {
                            return Err(format!("car.{n}.pit: sixteen values"));
                        }
                        for (slot, part) in pit.iter_mut().zip(parts) {
                            *slot = f32::from_bits(u32::from_str_radix(part, 16).map_err(|e| format!("car.{n}.pit: {e}"))?);
                        }
                        Some(pit)
                    }
                    None => None,
                },
                handover: n == 0 && self.get("ai_handover").is_some(),
                drag: match self.get(&key("drag")) {
                    Some(text) => {
                        let (push, start) = text.split_once(',').ok_or("car.drag: two values")?;
                        let bits = |t: &str| u32::from_str_radix(t, 16).map(f32::from_bits).map_err(|e| format!("car.drag: {e}"));
                        Some((bits(push)?, bits(start)?))
                    }
                    None => None,
                },
            });
        }
        Ok(cars)
    }

    /// Words in a row.
    pub fn row_words(&self) -> usize {
        WORLD_COLUMNS.len() + self.cars.iter().map(|car| INPUT_COLUMNS.len() + car_columns(car).len()).sum::<usize>() + if self.get("referee").is_some() { REFEREE_COLUMNS.len() } else { 0 }
    }

    /// Task 28: the referee of the recording (header key `referee`:
    /// `<session list>;<penalties>;<jump start mode or ->;<OVER_TIME_MULT>;<first start
    /// delay ms>;<next start delay ms>;<seed>`, the two floats as bits).
    pub fn referee_setup(&self) -> Result<Option<crate::race::RefereeSetup>, String> {
        let Some(text) = self.get("referee") else {
            return Ok(None);
        };
        let wrong = || format!("referee: seven values expected, got {text:?}");
        let parts: Vec<&str> = text.split(';').collect();
        let [list, penalties, jump, mult, first, next, seed] = parts[..] else { return Err(wrong()) };
        let float = |text: &str| u32::from_str_radix(text, 16).map(f32::from_bits).map_err(|_| wrong());
        let cars = self.cars.len();
        let sessions = crate::race::SessionsIni::from_list(list, cars, penalties == "1", jump.parse().ok(), 3)?;
        Ok(Some(crate::race::RefereeSetup {
            sessions,
            over_time_mult: float(mult)?,
            start_index: (0..cars as i32).collect(),
            pit_index: (0..cars as i32).collect(),
            players: self.cars.iter().enumerate().map(|(n, car)| crate::race::output::Player { name: format!("Driver {n}"), car: car.name.clone(), skin: format!("skin_{n}") }).collect(),
            seed: seed.parse().map_err(|_| wrong())?,
            first_start_delay_ms: float(first)?,
            next_start_delay_ms: float(next)?,
            yellow_flag_distance: None,
            layout: self.get("track_layout").unwrap_or("").to_string(),
        }))
    }

    /// The name of every word of a row.
    pub fn columns(&self) -> Vec<String> {
        let mut out: Vec<String> = WORLD_COLUMNS.iter().map(|s| s.to_string()).collect();
        for (n, car) in self.cars.iter().enumerate() {
            out.extend(INPUT_COLUMNS.iter().map(|name| format!("car{n}.{name}")));
            out.extend(car_columns(car).into_iter().map(|name| format!("car{n}.{name}")));
        }
        if self.get("referee").is_some() {
            out.extend(REFEREE_COLUMNS.iter().map(|s| s.to_string()));
        }
        out
    }

    /// Where car `n`'s inputs start in a row.
    pub fn input_offset(&self, n: usize) -> usize {
        WORLD_COLUMNS.len() + self.cars[..n].iter().map(|car| INPUT_COLUMNS.len() + car_columns(car).len()).sum::<usize>()
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let text: String = self.header.iter().map(|(k, v)| format!("{k}={v}\n")).collect();
        let words = self.row_words();
        let mut out = Vec::with_capacity(24 + text.len() + self.rows.len() * words * 4);
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(&(text.len() as u32).to_le_bytes());
        out.extend_from_slice(text.as_bytes());
        out.extend_from_slice(&(words as u32).to_le_bytes());
        out.extend_from_slice(&(self.rows.len() as u32).to_le_bytes());
        for row in &self.rows {
            assert_eq!(row.len(), words, "a row of the wrong length");
            for word in row {
                out.extend_from_slice(&word.to_le_bytes());
            }
        }
        out
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Recording, String> {
        let short = || "the recording is cut short".to_string();
        if bytes.len() < 12 || &bytes[..8] != MAGIC {
            return Err("not a several-car recording (.mcrec)".to_string());
        }
        let u32_at = |at: usize| -> Result<usize, String> { Ok(u32::from_le_bytes(bytes.get(at..at + 4).ok_or_else(short)?.try_into().unwrap()) as usize) };
        let text_len = u32_at(8)?;
        let text = std::str::from_utf8(bytes.get(12..12 + text_len).ok_or_else(short)?).map_err(|e| e.to_string())?;
        let mut recording = Recording::default();
        for line in text.lines() {
            if let Some((key, value)) = line.split_once('=') {
                recording.header.push((key.to_string(), value.to_string()));
            }
        }
        recording.cars = recording.cars_from_header()?;
        let mut at = 12 + text_len;
        let words = u32_at(at)?;
        let count = u32_at(at + 4)?;
        at += 8;
        if words != recording.row_words() {
            return Err(format!("the rows have {words} words, the header's cars make {}", recording.row_words()));
        }
        let data = bytes.get(at..at + words * count * 4).ok_or_else(short)?;
        recording.rows = data.chunks_exact(words * 4).map(|row| row.chunks_exact(4).map(|w| u32::from_le_bytes(w.try_into().unwrap())).collect()).collect();
        Ok(recording)
    }

    pub fn load(path: &Path) -> Result<Recording, String> {
        Recording::from_bytes(&std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?)
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        std::fs::write(path, self.to_bytes()).map_err(|e| format!("{}: {e}", path.display()))
    }

    /// Steps `from..from + count` as a recording of their own is not possible (a replay
    /// starts at step 0); the first `count` steps are.
    pub fn truncated(&self, count: usize) -> Recording {
        let mut out = self.clone();
        out.rows.truncate(count);
        out.set("steps", out.rows.len().to_string());
        out
    }
}

/// Where a replay finds what the recording names.
#[derive(Clone, Debug, Default)]
pub struct ReplayPaths {
    /// The folder with the extracted car data (`cardata/`).
    pub cardata: PathBuf,
    /// Assetto Corsa's folder (the cars' collider meshes; the track unless `track_folder`
    /// of the header exists as it is).
    pub game_root: Option<PathBuf>,
}

/// The first value of a replay that is not the recording's.
#[derive(Clone, Debug, PartialEq)]
pub struct Mismatch {
    pub step: usize,
    pub column: String,
    pub game: u32,
    pub port: u32,
}

impl std::fmt::Display for Mismatch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "step {} {}: game {:08x} ({:e}), port {:08x} ({:e})",
            self.step,
            self.column,
            self.game,
            f32::from_bits(self.game),
            self.port,
            f32::from_bits(self.port)
        )
    }
}

/// What a replay found.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ReplayReport {
    pub steps: usize,
    pub values: u64,
    /// The first difference (the replay stops there).
    pub mismatch: Option<Mismatch>,
    /// More values of the same step that differ.
    pub also: Vec<String>,
    /// Steps with at least one contact joint, the most contact joints in a step, steps with
    /// a contact joint between two cars.
    pub contact_steps: usize,
    pub max_contacts: u32,
    /// FNV-1a over every row the port produced.
    pub hash: u64,
    /// Per car: the largest damage of any zone, the largest suspension damage.
    pub damage: Vec<(f32, f32)>,
    /// Per car: steps with the pit-lane ghosting on.
    pub ghost_steps: Vec<usize>,
    /// Seconds the replay's steps took.
    pub seconds: f64,
    /// Task 28: `race_out.json` and `laps.ini` as the referee leaves them at the end
    pub referee_files: Option<(Vec<u8>, Vec<u8>)>,
    /// and its state in words when the replay stopped (for a look at a difference)
    pub referee_dump: Vec<(String, String)>,
}

/// A replay being built: the engine and its cars.
pub struct ReplayWorld {
    pub engine: PhysicsEngine,
    pub cars: Vec<RollingChassis>,
    pub devices: Vec<ScriptedDevice>,
    /// Task 27: the cars' AI drivers (a car without one is driven by its scripted device)
    pub ai: Vec<Option<Box<crate::ai::AiDriver>>>,
    /// Task 27: the cars' drag drivers
    pub drag: Vec<Option<Box<crate::ai::AiDriverDrag>>>,
    /// Task 27: the step before which the first car gets its AI driver
    pub handover: Option<usize>,
    pub clock_start_ms: f64,
    pub steps: usize,
    /// Task 28: the referee of a recording that has one
    pub referee: Option<Box<crate::race::Referee>>,
    /// Task 28b: (car, step): before that step the car's AI driver is called in
    /// (`AIDriver::requestPitStop`; header key `ai_pit_calls`)
    pub pit_calls: Vec<(usize, usize)>,
    /// Task 28b: the step before which the first car is damaged, the five zone levels, the
    /// engine's life left and the hit that bends the second suspension (header key
    /// `pit_damage`)
    pub pit_damage: Option<(usize, [f32; 5], f64, f32)>,
}

impl crate::race::World for ReplayWorld {
    fn car_count(&self) -> usize {
        self.cars.len()
    }

    fn car(&self, index: usize) -> &RollingChassis {
        &self.cars[index]
    }

    fn car_mut(&mut self, index: usize) -> &mut RollingChassis {
        &mut self.cars[index]
    }

    fn set_session_info(&mut self, info: crate::ai::SessionInfo) {
        let mut slots = slots_of(&mut self.cars, &mut self.devices, &mut self.ai);
        self.engine.set_session(info, &mut slots);
        self.engine.reset_collisions_for_new_session();
    }

    fn set_session_start_time(&mut self, ms: f64) {
        self.engine.env.session_start_time_ms = ms;
        self.engine.session.start_time_ms = ms;
        for car in &mut self.cars {
            car.env.session_start_time_ms = ms;
        }
    }

    fn set_penalty_mode(&mut self, mode: i32) {
        self.engine.env.penalty_mode = mode;
        for car in &mut self.cars {
            car.env.penalty_mode = mode;
        }
    }

    fn generate_wind(&mut self) {}
}

/// The track of a recording.
pub fn load_track(recording: &Recording, paths: &ReplayPaths) -> Result<Arc<Track>, String> {
    let folder = PathBuf::from(recording.need("track_folder")?);
    let layout = recording.get("track_layout").unwrap_or("");
    let folder = if folder.is_dir() {
        folder
    } else {
        // recorded on another machine: the same track of this machine's game (the folder's
        // name is what follows the last slash of either kind: a Windows path read on a
        // system that only knows forward slashes is one long name otherwise)
        let text = folder.to_string_lossy().into_owned();
        let name = text.rsplit(['/', '\\']).find(|part| !part.is_empty()).ok_or("track_folder has no name")?;
        paths.game_root.as_ref().ok_or("the recording's track folder is not here and no game folder was given")?.join("content").join("tracks").join(name)
    };
    let (mut track, _report) = crate::track::load_track(&folder, layout)?;
    if let Some(ai_folder) = recording.get("ai_folder") {
        // Task 27: the run had another fast lane (a line of an old version)
        for message in track.replace_ai_line(Path::new(ai_folder))? {
            eprintln!("{message}");
        }
    }
    if recording.cars.iter().any(|car| car.ai.is_some() || car.handover) {
        // the lines as the AI drivers need them
        track.init_ai()?;
    }
    Ok(Arc::new(track))
}

impl ReplayWorld {
    /// Builds the recording's cars the way `tools/car_oracle multi` built the game's:
    /// `Car::Car`, the collider mesh, the aids' switches, the spawn, the session start.
    pub fn build(recording: &Recording, paths: &ReplayPaths, track: Arc<Track>, threads: usize) -> Result<ReplayWorld, String> {
        let number = |key: &str| -> Result<f64, String> { recording.need(key)?.parse::<f64>().map_err(|e| format!("{key}: {e}")) };
        let clock_start_ms = number("clock_start_ms")?;
        let env = ChassisEnvironment {
            ambient_temperature: number("ambient")? as f32,
            road_temperature: number("road")? as f32,
            penalty_mode: number("penalty_mode")? as i32,
            // Task 28: the rules a referee's session list brings (absent in older recordings)
            jump_start_penalty_mode: match recording.get("jump_start_penalty_mode") {
                Some(text) => text.parse().map_err(|e| format!("jump_start_penalty_mode: {e}"))?,
                None => ChassisEnvironment::default().jump_start_penalty_mode,
            },
            lock_gearbox_at_start_time_ms: match recording.get("lock_gearbox_ms") {
                Some(text) => text.parse().map_err(|e| format!("lock_gearbox_ms: {e}"))?,
                None => ChassisEnvironment::default().lock_gearbox_at_start_time_ms,
            },
            allowed_tyres_out: number("allowed_tyres_out")? as i32,
            ..ChassisEnvironment::default()
        };
        let mut engine = PhysicsEngine::new(env, threads);
        engine.set_track(Arc::clone(&track), recording.get("track_objects") == Some("1"));
        engine.core.mesh_bounce_vel = f32::from_bits(u32::from_str_radix(recording.need("mesh_bounce_vel")?, 16).map_err(|e| e.to_string())?);
        let mut cars = Vec::new();
        for meta in &recording.cars {
            let data_path = paths.cardata.join(&meta.name);
            let mesh: Option<ColliderMesh> = if meta.collider_mesh {
                let loaded = crate::car::colliders::load(&data_path, paths.game_root.as_deref(), &meta.name)?;
                Some(loaded.mesh.ok_or_else(|| format!("no collider.kn5 for {} (it is in Assetto Corsa's folder)", meta.name))?)
            } else {
                None
            };
            let car_env = engine.car_env();
            let ground = Box::new(TrackGround(Arc::clone(&track)));
            let mut car = engine.build_car(|core, guid| {
                let mut car = RollingChassis::new_in_core(core, guid, &data_path, car_env, ground, meta.seed, clock_start_ms)?;
                // the collider mesh comes right after `Car::Car` (CarAvatar::initPhysics)
                if let Some(mesh) = mesh {
                    car.init_collider_mesh(mesh);
                }
                car.install_aero()?;
                car.install_brakes()?;
                car.install_drivetrain()?;
                car.install_aids()?;
                Ok(car)
            })?;
            car.autoclutch.use_auto_on_start = meta.auto_clutch;
            if meta.auto_clutch {
                car.autoclutch.use_auto_on_change = true;
            }
            car.auto_shifter.is_active = meta.auto_shifter;
            car.set_track(Arc::clone(&track));
            car.force_rotation(&Vec3f::new(meta.tail[0], meta.tail[1], meta.tail[2]));
            car.force_position(&Vec3f::new(meta.position[0], meta.position[1], meta.position[2]));
            car.set_damage_level(0.0);
            car.reset_suspension_damage_level();
            car.session_start()?;
            cars.push(car);
        }
        engine.core.set_no_collision_steps(number("no_collision_steps")? as i32);
        let devices = vec![ScriptedDevice::default(); cars.len()];
        let handover = match recording.get("ai_handover") {
            Some(text) => Some(text.parse::<usize>().map_err(|e| format!("ai_handover: {e}"))?),
            None => None,
        };
        let mut pit_calls = Vec::new();
        for call in recording.get("ai_pit_calls").unwrap_or("").split(',').filter(|call| !call.is_empty()) {
            let wrong = || format!("ai_pit_calls: <car>:<step> expected, got {call:?}");
            let (car, step) = call.split_once(':').ok_or_else(wrong)?;
            pit_calls.push((car.parse().map_err(|_| wrong())?, step.parse().map_err(|_| wrong())?));
        }
        let pit_damage = match recording.get("pit_damage") {
            Some(text) => {
                let wrong = || format!("pit_damage: <step>;<five zones>;<life>;<hit> expected, got {text:?}");
                let parts: Vec<&str> = text.split(';').collect();
                let [step, zones, life, hit] = parts[..] else { return Err(wrong()) };
                let zones: Vec<f32> = zones.split(',').filter_map(|z| u32::from_str_radix(z, 16).ok()).map(f32::from_bits).collect();
                let zones: [f32; 5] = zones.try_into().map_err(|_| wrong())?;
                Some((step.parse().map_err(|_| wrong())?, zones, f64::from_bits(u64::from_str_radix(life, 16).map_err(|_| wrong())?), f32::from_bits(u32::from_str_radix(hit, 16).map_err(|_| wrong())?)))
            }
            None => None,
        };
        let mut world = ReplayWorld { engine, cars, devices, ai: Vec::new(), drag: Vec::new(), handover, clock_start_ms, steps: 0, referee: None, pit_calls, pit_damage };
        world.ai.resize_with(world.cars.len(), || None);
        world.drag.resize_with(world.cars.len(), || None);
        if recording.cars.iter().any(|meta| meta.drag.is_some()) {
            world.build_drag(recording, clock_start_ms)?;
        }
        if recording.cars.iter().any(|meta| meta.ai.is_some()) || handover.is_some() {
            world.build_ai(recording, clock_start_ms)?;
        }
        // Task 28: the referee, made once every car and driver is there
        if let Some(setup) = recording.referee_setup()? {
            let track = world.cars[0].track.clone().ok_or("a referee needs a track")?;
            let mut referee = crate::race::Referee::new(setup, track, &mut world, clock_start_ms);
            // Task 28b: the player's pit stop, with the menu as the recording has it (header
            // key `pit`: `<window start>;<window end>;<the menu's values>`)
            if let Some(text) = recording.get("pit") {
                let wrong = || format!("pit: <window start>;<window end>;<menu> expected, got {text:?}");
                let mut parts = text.splitn(3, ';');
                let start: u16 = parts.next().and_then(|v| v.parse().ok()).ok_or_else(wrong)?;
                let end: u16 = parts.next().and_then(|v| v.parse().ok()).ok_or_else(wrong)?;
                let mut menu = crate::race::QuickMenu::for_car(&world.cars[0], &crate::race::QuickMenuSettings::default())?;
                if !menu.set_values_text(parts.next().ok_or_else(wrong)?) {
                    return Err(format!("pit: the recording's menu is not this car's ({} items here)", menu.items.len()));
                }
                referee.enable_pit_stop(menu, &world);
                referee.pit_window = (start, end);
            }
            referee.hash_frames = true;
            world.referee = Some(Box::new(referee));
        }
        Ok(world)
    }

    /// Task 27: the drag drivers, made as `tools/car_oracle multi` made the game's: after
    /// all cars, in creation order; then every car is put on its place once more (the
    /// driver lays its line) and every driver is given its start time, from the `rand()`
    /// state `ai_seed`.
    fn build_drag(&mut self, recording: &Recording, clock_start_ms: f64) -> Result<(), String> {
        let seed: u32 = recording.need("ai_seed")?.parse().map_err(|e| format!("ai_seed: {e}"))?;
        self.engine.ai_rand = crate::session::MsvcRand(seed);
        for (n, meta) in recording.cars.iter().enumerate() {
            let Some((push, _)) = meta.drag else { continue };
            let mut driver = crate::ai::AiDriverDrag::new(&mut self.cars[n])?;
            driver.push = push;
            self.drag[n] = Some(Box::new(driver));
        }
        for (n, meta) in recording.cars.iter().enumerate() {
            let car = &mut self.cars[n];
            car.forced_position_events = 0;
            car.force_rotation(&Vec3f::new(meta.tail[0], meta.tail[1], meta.tail[2]));
            car.force_position(&Vec3f::new(meta.position[0], meta.position[1], meta.position[2]));
            if let Some(driver) = &mut self.drag[n] {
                use crate::car::ControlsProvider as _;
                driver.on_forced_position(car);
            }
            car.forced_position_events = 0;
        }
        for (n, meta) in recording.cars.iter().enumerate() {
            if let (Some((_, start)), Some(driver)) = (meta.drag, &mut self.drag[n]) {
                let mut rand = self.engine.ai_rand;
                driver.set_start_time(clock_start_ms + start as f64, &mut rand);
                self.engine.ai_rand = rand;
            }
        }
        Ok(())
    }

    /// Task 27: the AI drivers, made as `tools/car_oracle multi` made the game's: after all
    /// cars, in creation order, from the `rand()` state `ai_seed`; then the session
    /// (`ai_session`: type, milliseconds from the clock's start to the green light, seconds,
    /// laps), which every driver is told, and every car put on its place once more.
    fn build_ai(&mut self, recording: &Recording, clock_start_ms: f64) -> Result<(), String> {
        let seed: u32 = recording.need("ai_seed")?.parse().map_err(|e| format!("ai_seed: {e}"))?;
        self.engine.ai_rand = crate::session::MsvcRand(seed);
        let session = self.engine.session;
        for (n, meta) in recording.cars.iter().enumerate() {
            let car = &mut self.cars[n];
            car.forced_position_events = 0;
            car.init_pitstop_timings()?;
            if let Some(pit) = &meta.pit {
                for r in 0..4 {
                    car.pit_position.m[r].copy_from_slice(&pit[4 * r..4 * r + 4]);
                }
            }
            if meta.handover {
                // (the driver comes later; the car's estimate is checked now)
                car.init_expected_fuel_per_lap()?;
                let game = u32::from_str_radix(recording.need("ai_handover_expected_fuel")?, 16).map_err(|e| e.to_string())?;
                if car.expected_fuel_per_lap.to_bits() != game {
                    return Err(format!("car {n}: the fuel a lap is expected to take is {} here and {} in the game", car.expected_fuel_per_lap, f32::from_bits(game)));
                }
            }
            let Some(ai) = &meta.ai else { continue };
            // Car::Car's estimate of the fuel a lap takes: the game's own number is in the recording
            car.init_expected_fuel_per_lap()?;
            if car.expected_fuel_per_lap.to_bits() != ai.expected_fuel_per_lap.to_bits() {
                return Err(format!("car {n}: the fuel a lap is expected to take is {} here and {} in the game", car.expected_fuel_per_lap, ai.expected_fuel_per_lap));
            }
            let mut rand = self.engine.ai_rand;
            car.ai_input.session = session;
            let mut driver = crate::ai::AiDriver::new(car, &session, &mut rand, &crate::ai::driver::AiFiles::default())?;
            self.engine.ai_rand = rand;
            driver.base_push = ai.base_push;
            driver.aggression = ai.aggression;
            self.ai[n] = Some(Box::new(driver));
        }
        if let Some(text) = recording.get("ai_session") {
            let parts: Vec<f64> = text.split(',').map(|part| part.parse::<f64>().map_err(|e| format!("ai_session: {e}"))).collect::<Result<_, _>>()?;
            if parts.len() != 4 {
                return Err("ai_session: four numbers".to_string());
            }
            let info = crate::ai::SessionInfo { session_type: parts[0] as i32, start_time_ms: clock_start_ms + parts[1], time_secs: parts[2], laps: parts[3] as i32, index: 0 };
            let mut slots = slots_of(&mut self.cars, &mut self.devices, &mut self.ai);
            self.engine.set_session(info, &mut slots);
            for n in 0..recording.cars.len() {
                let meta = &recording.cars[n];
                slots[n].car.force_rotation(&Vec3f::new(meta.tail[0], meta.tail[1], meta.tail[2]));
                slots[n].car.force_position(&Vec3f::new(meta.position[0], meta.position[1], meta.position[2]));
                self.engine.deliver_forced_positions(&mut slots);
            }
        }
        Ok(())
    }

    /// One step with these controls (a car's [`INPUT_COLUMNS`] words each).
    pub fn step(&mut self, inputs: &[&[u32]]) {
        self.step_with(inputs, &mut super::NoHooks);
    }

    /// [`ReplayWorld::step`] with the game's other step-completed handlers (an online
    /// session: its client and the other players' cars).
    pub fn step_with(&mut self, inputs: &[&[u32]], hooks: &mut dyn super::StepHooks) {
        for (device, words) in self.devices.iter_mut().zip(inputs) {
            let c = &mut device.controls;
            c.gas = f32::from_bits(words[0]);
            c.brake = f32::from_bits(words[1]);
            c.steer = f32::from_bits(words[2]);
            c.clutch = f32::from_bits(words[3]);
            c.hand_brake = f32::from_bits(words[4]);
            c.gear_up = words[5] & 1 != 0;
            c.gear_dn = words[5] & 2 != 0;
            c.drs = words[5] & 4 != 0;
            c.kers = words[5] & 8 != 0;
            c.requested_gear_index = words[6] as i32;
        }
        let time = self.clock_start_ms + (self.steps as f64 + 1.0) * 3.0;
        if self.handover == Some(self.steps) {
            // the game's "AI drives my car": a new driver for the first car, as it is now
            let session = self.engine.session;
            let mut rand = self.engine.ai_rand;
            self.cars[0].ai_input.session = session;
            match crate::ai::AiDriver::new(&mut self.cars[0], &session, &mut rand, &crate::ai::driver::AiFiles::default()) {
                Ok(driver) => self.ai[0] = Some(Box::new(driver)),
                Err(message) => panic!("the hand-over: {message}"),
            }
            self.engine.ai_rand = rand;
        }
        // Task 28b: the damage a repair stop starts from, and drivers that are called in
        if let Some((step, zones, life, hit)) = self.pit_damage {
            if step == self.steps {
                let car = &mut self.cars[0];
                car.damage_zone_level = zones;
                if let Some(drivetrain) = &mut car.drivetrain {
                    drivetrain.engine_mut().base_mut().life_left = life;
                }
                car.suspensions[1].set_damage(hit);
            }
        }
        for &(car, step) in &self.pit_calls {
            if step == self.steps {
                if let Some(driver) = self.ai.get_mut(car).and_then(|driver| driver.as_mut()) {
                    driver.request_pit_stop = true;
                }
            }
        }
        // Task 28: the engine's jobs the referee posted
        if let Some(mut referee) = self.referee.take() {
            referee.before_step(self);
            self.referee = Some(referee);
        }
        let mut slots = slots_of(&mut self.cars, &mut self.devices, &mut self.ai);
        for (slot, drag) in slots.iter_mut().zip(self.drag.iter_mut()) {
            if let Some(driver) = drag {
                slot.device = DeviceRef::Anywhere(&mut **driver);
            }
        }
        self.engine.step_with(0.003, time, &mut slots, hooks);
        assert_eq!(self.engine.pit_lane_mispredictions, 0, "a car's pit-lane flag was not what the AI drivers were shown (step {})", self.steps);
        drop(slots);
        self.steps += 1;
        // Task 28: the cars' lap events, and the referee's frame every third step
        if let Some(mut referee) = self.referee.take() {
            for index in 0..self.cars.len() {
                let (laps, splits) = self.cars[index].transponder.take_events();
                referee.events(index, &laps, &splits);
            }
            referee.after_step(self, time);
            self.referee = Some(referee);
        }
    }

    /// The contact joints of the world, newest first.
    pub fn contacts(&self) -> Vec<ContactNote> {
        let core = &self.engine.core;
        let car_of = |body: Option<crate::ode::BodyId>| -> i32 {
            match body {
                None => -1,
                Some(id) => (0..self.cars.len()).find(|&n| self.engine.bodies_of(n).contains(&id)).map_or(-2, |n| n as i32),
            }
        };
        core.contact_joints()
            .into_iter()
            .map(|id| {
                let joint = core.world.joint(id);
                let JointKind::Contact { contact, .. } = &joint.kind else { unreachable!("contact_joints lists contact joints") };
                ContactNote {
                    pos: contact.geom.pos,
                    normal: contact.geom.normal,
                    depth: contact.geom.depth,
                    car1: car_of(joint.node[0].body),
                    car2: car_of(joint.node[1].body),
                    reversed: joint.flags & JOINT_REVERSE != 0,
                    mode: contact.surface.mode,
                    mu: contact.surface.mu,
                    bounce: contact.surface.bounce,
                    bounce_vel: contact.surface.bounce_vel,
                    soft_cfm: contact.surface.soft_cfm,
                }
            })
            .collect()
    }

    /// The row of the step that just ran, with `inputs` as they were given.
    pub fn row(&self, inputs: &[&[u32]]) -> Vec<u32> {
        let mut row = Vec::new();
        row.extend(contact_words(&self.contacts()));
        row.push(self.engine.core.no_collision_counter as u32);
        row.push(self.engine.core.current_frame);
        for (n, car) in self.cars.iter().enumerate() {
            match self.ai[n].as_ref().map(|_| ()).or(self.drag[n].as_ref().map(|_| ())) {
                // an AI driver's inputs: what it left in the car's controls
                Some(_) => {
                    let c = &car.controls;
                    row.extend([c.gas, c.brake, c.steer, c.clutch, c.hand_brake].map(f32::to_bits));
                    row.push(c.gear_up as u32 | (c.gear_dn as u32) << 1 | (c.drs as u32) << 2 | (c.kers as u32) << 3);
                    row.push(c.requested_gear_index as u32);
                }
                None => row.extend_from_slice(inputs[n]),
            }
            for &body in self.engine.bodies_of(n) {
                let b = car.core.world.body(body);
                row.extend(b.pos[..3].iter().map(|x| x.to_bits()));
                row.extend(b.q.iter().map(|x| x.to_bits()));
                row.extend(b.lvel[..3].iter().map(|x| x.to_bits()));
                row.extend(b.avel[..3].iter().map(|x| x.to_bits()));
            }
            row.extend(car.damage_zone_level.iter().map(|x| x.to_bits()));
            for suspension in &car.suspensions {
                row.push(suspension.get_damage().to_bits());
            }
            for tyre in &car.tyres {
                let s = &tyre.status;
                row.extend([s.angular_velocity, s.load, s.nd_slip, s.slip_ratio].map(f32::to_bits));
            }
            let (velocity, gear) = match &car.drivetrain {
                Some(drivetrain) => (drivetrain.base().engine.velocity, drivetrain.base().current_gear),
                None => (0.0, 0),
            };
            let double = |x: f64| [x.to_bits() as u32, (x.to_bits() >> 32) as u32];
            row.extend(double(velocity));
            row.push(gear as u32);
            row.extend(double(car.fuel));
            row.push(car.speed.to_bits());
            row.push(car.core.get_mesh_collide_mask(car.body, 0).unwrap_or(0));
            row.push(car.is_collision_off_for_pits as u32);
            row.extend(double(car.last_collision_time));
            row.extend(double(car.last_collision_with_car_time));
            row.push(car.transponder.lap_count);
            row.push(car.spline_locator_data.npos.to_bits());
            match &self.ai[n] {
                Some(driver) => row.extend(driver.oracle_words()),
                None if n == 0 && self.handover.is_some() => row.extend(std::iter::repeat_n(0u32, crate::ai::oracle::column_names().len())),
                None => {}
            }
        }
        if let Some(referee) = &self.referee {
            row.extend([referee.state_hash as u32, (referee.state_hash >> 32) as u32]);
        }
        row
    }
}

/// The cars with their devices as the engine takes them: a car's AI driver, else its
/// scripted device.
fn slots_of<'a>(cars: &'a mut [RollingChassis], devices: &'a mut [ScriptedDevice], ai: &'a mut [Option<Box<crate::ai::AiDriver>>]) -> Vec<CarSlot<'a>> {
    cars.iter_mut()
        .zip(devices.iter_mut())
        .zip(ai.iter_mut())
        .map(|((car, device), ai)| match ai {
            Some(driver) => CarSlot { car, device: DeviceRef::Anywhere(&mut **driver) },
            None => CarSlot { car, device: DeviceRef::Anywhere(device) },
        })
        .collect()
}

/// Two words are the same value: the same bits, or two NaNs (a NaN's sign and payload follow
/// the operand order the compiler chose).
fn same_word(a: u32, b: u32, float: bool) -> bool {
    a == b || (float && f32::from_bits(a).is_nan() && f32::from_bits(b).is_nan())
}

/// Columns that hold a whole number or half of a double, not a float.
fn is_float_column(name: &str) -> bool {
    !(name.ends_with(".lo")
        || name.ends_with(".hi")
        || name.ends_with("gear")
        || name.ends_with("Bits")
        || name.ends_with("ForPits")
        || name.ends_with("lapCount")
        || name.ends_with("buttons")
        || name.ends_with("requestedGear")
        || name.contains(".ai.i.")
        || name.starts_with("world."))
}

/// Replays a recording from step 0 with `threads` worker threads and compares every value of
/// every step. `fault`, for proving that the comparison can fail: (step, car, body): one bit
/// of that body's forward speed is flipped before that step.
pub fn replay(recording: &Recording, paths: &ReplayPaths, threads: usize, fault: Option<(usize, usize)>) -> Result<ReplayReport, String> {
    let track = load_track(recording, paths)?;
    replay_on(recording, paths, track, threads, fault)
}

/// [`replay`] on a track that is loaded already.
pub fn replay_on(recording: &Recording, paths: &ReplayPaths, track: Arc<Track>, threads: usize, fault: Option<(usize, usize)>) -> Result<ReplayReport, String> {
    if recording.rows.is_empty() {
        return Err("the recording has no steps".to_string());
    }
    if let Some((_, car)) = fault {
        if car >= recording.cars.len() {
            return Err(format!("the fault's car {car}: the recording has {} cars", recording.cars.len()));
        }
    }
    let mut world = ReplayWorld::build(recording, paths, track, threads)?;
    for (n, (car, meta)) in world.cars.iter().zip(&recording.cars).enumerate() {
        let bodies = world.engine.bodies_of(n).len();
        if bodies != meta.bodies {
            return Err(format!("car {n} ({}): {bodies} bodies here, {} in the recording", car.unix_name, meta.bodies));
        }
    }
    let columns = recording.columns();
    let floats: Vec<bool> = columns.iter().map(|name| is_float_column(name)).collect();
    let offsets: Vec<usize> = (0..recording.cars.len()).map(|n| recording.input_offset(n)).collect();
    let mut report = ReplayReport { damage: vec![(0.0, 0.0); recording.cars.len()], ghost_steps: vec![0; recording.cars.len()], ..ReplayReport::default() };
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    let start = std::time::Instant::now();
    let row_dump: Option<(usize, usize, Vec<String>)> = std::env::var("RUSTYAC_ROW_DUMP").ok().and_then(|text| {
        let mut parts = text.splitn(3, ':');
        Some((parts.next()?.parse().ok()?, parts.next()?.parse().ok()?, parts.next()?.split(',').map(str::to_string).collect()))
    });
    for (step, game) in recording.rows.iter().enumerate() {
        if let Some((at, car)) = fault {
            if at == step {
                let body = world.cars[car].body;
                let b = world.cars[car].core.world.body_mut(body.id);
                b.lvel[2] = f32::from_bits(b.lvel[2].to_bits() ^ 1);
            }
        }
        let inputs: Vec<&[u32]> = offsets.iter().map(|&at| &game[at..at + INPUT_COLUMNS.len()]).collect();
        world.step(&inputs);
        let port = world.row(&inputs);
        for word in &port {
            for byte in word.to_le_bytes() {
                hash ^= byte as u64;
                hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
            }
        }
        // RUSTYAC_ROW_DUMP=<from>:<to>:<part of a column's name>,...: the port's and the game's
        // values of those columns in those steps, for a look at a difference
        if let Some((from, to, parts)) = row_dump.as_ref() {
            if (*from..*to).contains(&step) {
                let line: Vec<String> = columns.iter().enumerate().filter(|(_, name)| parts.iter().any(|part| name.contains(part.as_str()))).map(|(i, name)| format!("{name} {:08x}/{:08x}", game[i], port[i])).collect();
                eprintln!("step {step} (game/port): {}", line.join("  "));
            }
        }
        report.steps = step + 1;
        report.values += port.len() as u64;
        if port[0] > 0 {
            report.contact_steps += 1;
        }
        report.max_contacts = report.max_contacts.max(port[0]);
        for (n, car) in world.cars.iter().enumerate() {
            let zone = car.damage_zone_level.iter().fold(0.0f32, |a, &b| a.max(b));
            let bent = car.suspensions.iter().fold(0.0f32, |a, s| a.max(s.get_damage()));
            report.damage[n] = (report.damage[n].0.max(zone), report.damage[n].1.max(bent));
            report.ghost_steps[n] += car.is_collision_off_for_pits as usize;
        }
        let mut differing = (0..port.len()).filter(|&i| !same_word(game[i], port[i], floats[i]));
        if let Some(first) = differing.next() {
            report.mismatch = Some(Mismatch { step, column: columns[first].clone(), game: game[first], port: port[first] });
            report.also = differing.take(std::env::var("RUSTYAC_ALSO").ok().and_then(|n| n.parse().ok()).unwrap_or(12)).map(|i| format!("{} game {:08x} port {:08x}", columns[i], game[i], port[i])).collect();
            break;
        }
    }
    report.seconds = start.elapsed().as_secs_f64();
    report.hash = hash;
    if let Some(mut referee) = world.referee.take() {
        report.referee_dump = referee.dump(&world);
        if report.mismatch.is_none() {
            report.referee_files = Some(referee.shutdown(&mut world));
        }
    }
    Ok(report)
}

// --- golden excerpts ---------------------------------------------------------------------------

pub const GOLDEN_MAGIC: &[u8; 8] = b"RYMCGLD1";

/// A small stand-in for a recording, for `cargo test`: the header, what every device
/// reported in every step, and one hash per step over the game's whole row.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Golden {
    pub header: Vec<(String, String)>,
    /// Per step: the cars' [`INPUT_COLUMNS`] words, car after car.
    pub inputs: Vec<Vec<u32>>,
    /// Per step: FNV-1a over the game's row.
    pub hashes: Vec<u64>,
}

fn row_hash(row: &[u32]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for word in row {
        for byte in word.to_le_bytes() {
            hash ^= byte as u64;
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    hash
}

impl Recording {
    /// The first `count` steps as a golden excerpt.
    pub fn golden(&self, count: usize) -> Golden {
        let offsets: Vec<usize> = (0..self.cars.len()).map(|n| self.input_offset(n)).collect();
        let rows = &self.rows[..count.min(self.rows.len())];
        let mut header = self.header.clone();
        if let Some(entry) = header.iter_mut().find(|(k, _)| k == "steps") {
            entry.1 = rows.len().to_string();
        }
        Golden {
            header,
            inputs: rows.iter().map(|row| offsets.iter().flat_map(|&at| row[at..at + INPUT_COLUMNS.len()].iter().copied()).collect()).collect(),
            hashes: rows.iter().map(|row| row_hash(row)).collect(),
        }
    }
}

impl Golden {
    pub fn to_bytes(&self) -> Vec<u8> {
        let text: String = self.header.iter().map(|(k, v)| format!("{k}={v}\n")).collect();
        let words = self.inputs.first().map_or(0, Vec::len);
        let mut out = Vec::new();
        out.extend_from_slice(GOLDEN_MAGIC);
        out.extend_from_slice(&(text.len() as u32).to_le_bytes());
        out.extend_from_slice(text.as_bytes());
        out.extend_from_slice(&(words as u32).to_le_bytes());
        out.extend_from_slice(&(self.inputs.len() as u32).to_le_bytes());
        for (inputs, hash) in self.inputs.iter().zip(&self.hashes) {
            for word in inputs {
                out.extend_from_slice(&word.to_le_bytes());
            }
            out.extend_from_slice(&hash.to_le_bytes());
        }
        out
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Golden, String> {
        let short = || "the golden file is cut short".to_string();
        if bytes.len() < 12 || &bytes[..8] != GOLDEN_MAGIC {
            return Err("not a several-car golden file".to_string());
        }
        let u32_at = |at: usize| -> Result<usize, String> { Ok(u32::from_le_bytes(bytes.get(at..at + 4).ok_or_else(short)?.try_into().unwrap()) as usize) };
        let text_len = u32_at(8)?;
        let text = std::str::from_utf8(bytes.get(12..12 + text_len).ok_or_else(short)?).map_err(|e| e.to_string())?;
        let mut golden = Golden::default();
        for line in text.lines() {
            if let Some((key, value)) = line.split_once('=') {
                golden.header.push((key.to_string(), value.to_string()));
            }
        }
        let mut at = 12 + text_len;
        let words = u32_at(at)?;
        let count = u32_at(at + 4)?;
        at += 8;
        for _ in 0..count {
            let row = bytes.get(at..at + words * 4 + 8).ok_or_else(short)?;
            golden.inputs.push(row[..words * 4].chunks_exact(4).map(|w| u32::from_le_bytes(w.try_into().unwrap())).collect());
            golden.hashes.push(u64::from_le_bytes(row[words * 4..].try_into().unwrap()));
            at += words * 4 + 8;
        }
        Ok(golden)
    }

    /// The recording's header and cars, without rows.
    pub fn recording(&self) -> Result<Recording, String> {
        let mut recording = Recording { header: self.header.clone(), ..Recording::default() };
        recording.cars = recording.cars_from_header()?;
        Ok(recording)
    }

    /// Replays the excerpt with `threads` worker threads. `Ok(None)`: every step's row has
    /// the game's hash; else the first step that does not.
    pub fn replay(&self, paths: &ReplayPaths, track: Arc<Track>, threads: usize) -> Result<Option<usize>, String> {
        let recording = self.recording()?;
        let width = INPUT_COLUMNS.len();
        if self.inputs.is_empty() || self.inputs.iter().any(|inputs| inputs.len() != width * recording.cars.len()) {
            return Err("the golden file has no steps, or its inputs do not fit its cars".to_string());
        }
        let mut world = ReplayWorld::build(&recording, paths, track, threads)?;
        for (step, (inputs, hash)) in self.inputs.iter().zip(&self.hashes).enumerate() {
            let per_car: Vec<&[u32]> = inputs.chunks_exact(width).collect();
            world.step(&per_car);
            if row_hash(&world.row(&per_car)) != *hash {
                return Ok(Some(step));
            }
        }
        Ok(None)
    }
}
