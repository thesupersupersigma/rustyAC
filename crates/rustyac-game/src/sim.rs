// SPDX-License-Identifier: GPL-3.0-or-later

//! The simulation the game runs: one [`VanillaCar`], built and spawned the way the oracle
//! builds and spawns the game's car, and stepped with nothing but what a driver's device
//! reports. Live driving, `--replay` and the tests all go through [`GameSim::step`]; there is
//! no other way the game touches the car.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use rustyac_content::vfs::PathExt;
use rustyac_physics::car::replay::Ground;
use rustyac_physics::track::{load_track, LapDb, Track, TrackGround};
use rustyac_physics::tyre::RayTrackCollisionProvider;
use rustyac_physics::car::{CarControls, CarControlsInput, ControlsProvider, VanillaCar, VibrationDef};
use rustyac_physics::vecmath::Vec3f;

use crate::input_file::{event, SimSetup, StepInput};

/// AC's physics step, s (333.33 Hz).
pub const DT: f32 = 0.003;
/// The same in milliseconds, as the physics clock counts.
pub const DT_MS: f64 = 3.0;

/// Whoever drives: the live devices, or a recorded drive.
pub trait DriverSource: Send {
    /// A recorded step is about to run (replay only).
    fn load(&mut self, _step: &StepInput) {}

    /// Commands the driver gave since the last step ([`event`] bits, brake-bias clicks).
    fn take_events(&mut self) -> (u32, i32) {
        (0, 0)
    }

    /// AC's `acquireControls`: writes the device's fields of `Car::controls`.
    fn acquire(&mut self, controls: &mut CarControls, dt: f32, input: &CarControlsInput);

    /// The headlight switch (`getAction(4)`).
    fn headlights(&mut self) -> bool {
        false
    }

    fn send_ff(&mut self, _ff: f32, _damper: f32, _user_gain: f32) {}

    fn set_vibrations(&mut self, _def: &VibrationDef) {}

    fn set_engine_rpm(&mut self, _rpm: f32, _low: f32, _high: f32) {}

    /// Which device drove in the last `acquire` (a note in the input file).
    fn device_id(&self) -> u32 {
        0
    }

    /// The next step belongs to the spawn sequence (the rest after the drop, the shift into
    /// first gear): nobody is driving yet, so the physics thread runs it without waiting for
    /// the clock.
    fn in_spawn_sequence(&self) -> bool {
        false
    }

    /// The game itself asks for a command before the next step (a car that fell over is put
    /// back). A recorded drive has its own commands and ignores this.
    fn request(&mut self, _events: u32) {}

    /// Clicks of the cockpit's brake-bias control asked for from outside the device (a key of
    /// the window or of the page), before the next step. A recorded drive ignores this.
    fn request_bias(&mut self, _clicks: i32) {}

    /// Called now and then while the simulation is paused (devices can be looked after).
    fn idle(&mut self) {}

    /// Does the device look at the car (AC's keyboard class does)?
    fn wants_probe(&self) -> bool {
        false
    }

    /// What the device may know of the car before the step, if it [wants](Self::wants_probe) it.
    fn set_probe(&mut self, _probe: &CarProbe) {}

    /// The track the car is on, once it is loaded (a driver that follows its line wants it).
    fn set_track(&mut self, _track: &Arc<Track>) {}
}

/// What AC's keyboard class reads off its car: values the last step left behind.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CarProbe {
    /// `status.ndSlip` of the left and right driven tyre (slip over the slip at peak grip).
    pub driven_left_slip: f32,
    pub driven_right_slip: f32,
    /// `RaceEngineer::getOptimalBrake`: the pedal at which the first axle reaches its grip.
    pub optimal_brake: f32,
    /// Where the car is and how it points (the body's position, forward and left axes), its
    /// place along the track's AI line after the last step (-1: not known) and beside it, the
    /// front wheels' angle at full lock (rad) and the wheelbase (m): what a driver sees.
    pub position: [f32; 3],
    pub forward: [f32; 3],
    pub left: [f32; 3],
    pub npos: f32,
    pub offset: f32,
    pub max_wheel_angle: f32,
    pub wheelbase: f32,
}

impl CarProbe {
    /// Reads the car; changes nothing in it.
    pub fn capture(car: &mut rustyac_physics::car::RollingChassis) -> CarProbe {
        // `RaceEngineer::getLeftDrivenTyre` / `getRightDrivenTyre`: the front pair of a
        // front-wheel-drive car, else the rear pair
        let front = car.drivetrain.as_ref().is_some_and(|d| d.base().traction_type == rustyac_physics::car::TractionType::Fwd);
        let (left, right) = if front { (0, 1) } else { (2, 3) };
        let m = car.core.get_world_matrix(car.body).m;
        let wheelbase = (car.suspensions[0].get_base_position().z - car.suspensions[2].get_base_position().z).abs();
        CarProbe {
            driven_left_slip: car.tyres[left].status.nd_slip,
            driven_right_slip: car.tyres[right].status.nd_slip,
            optimal_brake: rustyac_physics::car::aids::get_optimal_brake(car),
            position: [m[3][0], m[3][1], m[3][2]],
            forward: [m[2][0], m[2][1], m[2][2]],
            left: [m[0][0], m[0][1], m[0][2]],
            npos: if car.spline_locator.normalized_pos >= 0.0 { car.spline_locator_data.npos } else { -1.0 },
            offset: car.spline_locator.offset,
            max_wheel_angle: if car.steer_ratio != 0.0 { (car.steer_lock / car.steer_ratio).to_radians() } else { 0.3 },
            wheelbase,
        }
    }
}

/// `StepInput::device` of a step of the spawn sequence.
pub const DEVICE_SPAWN: u32 = 4;

/// A recorded drive: every step it reports what the file says.
#[derive(Clone, Copy, Debug, Default)]
pub struct ReplaySource {
    pub step: StepInput,
}

impl DriverSource for ReplaySource {
    fn load(&mut self, step: &StepInput) {
        self.step = *step;
    }

    fn take_events(&mut self) -> (u32, i32) {
        (self.step.events, self.step.bias_clicks)
    }

    fn acquire(&mut self, controls: &mut CarControls, _dt: f32, _input: &CarControlsInput) {
        // the whole of `Car::controls` as the recorded device left it: whatever that device
        // did not write had the same value before, because the car is in the same state
        *controls = self.step.controls;
    }

    fn headlights(&mut self) -> bool {
        self.step.headlights
    }

    fn device_id(&self) -> u32 {
        self.step.device
    }

    fn in_spawn_sequence(&self) -> bool {
        self.step.device == DEVICE_SPAWN
    }
}

/// Nobody at the controls: pedals up, wheel straight. With the spawn sequence around it
/// ([`SpawnSequence`]) this is the driver of the unattended checks.
#[derive(Clone, Copy, Debug, Default)]
pub struct NobodySource;

impl DriverSource for NobodySource {
    fn acquire(&mut self, controls: &mut CarControls, _dt: f32, _input: &CarControlsInput) {
        *controls = CarControls { clutch: 1.0, ..CarControls::default() };
    }
}

/// Nobody steers and the throttle is held to the floor (`--flat-out`): a second of revving
/// with the clutch pedal down, then the clutch is dropped. A launch with all the wheelspin
/// the car has, for pictures of smoke and skid marks.
#[derive(Clone, Copy, Debug, Default)]
pub struct FlatOutSource {
    steps: u32,
}

impl DriverSource for FlatOutSource {
    fn acquire(&mut self, controls: &mut CarControls, _dt: f32, _input: &CarControlsInput) {
        self.steps += 1;
        let clutch = if self.steps < 333 { 0.0 } else { 1.0 };
        *controls = CarControls { clutch, gas: 1.0, ..CarControls::default() };
    }
}

/// Steps of rest after a spawn, as the oracle's scenarios wait (1.2 s): the car drops onto
/// its wheels and comes to rest.
pub const SPAWN_REST_STEPS: u32 = 400;
/// Then the up-shift paddle is held this long: first gear.
pub const SPAWN_SHIFT_STEPS: u32 = 10;
/// And a moment for the gearbox to finish the change before the driver gets the car.
pub const SPAWN_SETTLE_STEPS: u32 = 60;

/// Wraps the live devices: after every spawn (the start, a reset, a new car) the car is left
/// alone to drop and rest, then put into first gear with the up-shift paddle, the way a
/// driver would; only then do the devices drive. Everything it does goes through
/// `Car::controls` like any driver's input, and is recorded as such.
pub struct SpawnSequence<S: DriverSource> {
    pub inner: S,
    /// Steps of the sequence done; past the end the devices drive.
    pub at: u32,
    /// Commands asked for by the game itself ([`DriverSource::request`]).
    requested: u32,
    /// Brake-bias clicks asked for from outside ([`DriverSource::request_bias`]).
    requested_bias: i32,
    /// Shift into first gear at the end of the sequence.
    pub first_gear: bool,
    /// The last `acquire` was a step of the sequence.
    last_was_sequence: bool,
}

impl<S: DriverSource> SpawnSequence<S> {
    pub fn new(inner: S, first_gear: bool) -> SpawnSequence<S> {
        SpawnSequence { inner, at: 0, requested: 0, requested_bias: 0, first_gear, last_was_sequence: false }
    }

    fn length(&self) -> u32 {
        SPAWN_REST_STEPS + if self.first_gear { SPAWN_SHIFT_STEPS + SPAWN_SETTLE_STEPS } else { 0 }
    }
}

impl<S: DriverSource> DriverSource for SpawnSequence<S> {
    fn take_events(&mut self) -> (u32, i32) {
        let (mut events, bias_clicks) = self.inner.take_events();
        events |= std::mem::take(&mut self.requested);
        if events & (event::RESET | event::REBUILD) != 0 {
            self.at = 0;
        }
        (events, bias_clicks + std::mem::take(&mut self.requested_bias))
    }

    fn acquire(&mut self, controls: &mut CarControls, dt: f32, input: &CarControlsInput) {
        self.last_was_sequence = self.at < self.length();
        if !self.last_was_sequence {
            self.inner.acquire(controls, dt, input);
            return;
        }
        *controls = CarControls { clutch: 1.0, ..CarControls::default() };
        controls.gear_up = self.first_gear && (SPAWN_REST_STEPS..SPAWN_REST_STEPS + SPAWN_SHIFT_STEPS).contains(&self.at);
        self.at += 1;
    }

    fn headlights(&mut self) -> bool {
        self.at >= self.length() && self.inner.headlights()
    }

    fn send_ff(&mut self, ff: f32, damper: f32, user_gain: f32) {
        self.inner.send_ff(ff, damper, user_gain);
    }

    fn set_vibrations(&mut self, def: &VibrationDef) {
        self.inner.set_vibrations(def);
    }

    fn set_engine_rpm(&mut self, rpm: f32, low: f32, high: f32) {
        self.inner.set_engine_rpm(rpm, low, high);
    }

    fn device_id(&self) -> u32 {
        if self.last_was_sequence {
            DEVICE_SPAWN
        } else {
            self.inner.device_id()
        }
    }

    fn in_spawn_sequence(&self) -> bool {
        self.at < self.length()
    }

    fn request(&mut self, events: u32) {
        self.requested |= events;
    }

    fn request_bias(&mut self, clicks: i32) {
        self.requested_bias += clicks;
    }

    fn idle(&mut self) {
        self.inner.idle();
    }

    fn wants_probe(&self) -> bool {
        self.inner.wants_probe()
    }

    fn set_probe(&mut self, probe: &CarProbe) {
        self.inner.set_probe(probe);
    }

    fn set_track(&mut self, track: &Arc<Track>) {
        self.inner.set_track(track);
    }
}

/// The car's `ICarControlsProvider`: hands the car's calls to the [`DriverSource`] and keeps
/// what the device reported, for the input file and the display.
pub struct Driver {
    pub source: Box<dyn DriverSource>,
    /// `getFFGlobalGain`
    pub ff_gain: f32,
    /// `Car::controls` as the device left it in this step's `acquireControls`.
    pub reported: CarControls,
    /// What `getAction(4)` answered in this step.
    pub headlights: bool,
    /// The car asked for the controls in this step (it does not while they are locked).
    pub polled: bool,
    /// What the car sent back in the last step.
    pub last_ff: f32,
    pub last_damper: f32,
    pub last_vibrations: VibrationDef,
}

impl Driver {
    pub fn new(source: Box<dyn DriverSource>, ff_gain: f32) -> Driver {
        Driver {
            source,
            ff_gain,
            reported: CarControls::default(),
            headlights: false,
            polled: false,
            last_ff: 0.0,
            last_damper: 0.0,
            last_vibrations: VibrationDef::default(),
        }
    }
}

impl ControlsProvider for Driver {
    fn acquire_controls(&mut self, controls: &mut CarControls, dt: f32, input: &CarControlsInput) {
        self.source.acquire(controls, dt, input);
        self.reported = *controls;
        self.polled = true;
    }

    fn get_action(&mut self, action: i32) -> bool {
        if action == 4 {
            self.headlights = self.source.headlights();
            self.headlights
        } else {
            false
        }
    }

    fn send_ff(&mut self, ff: f32, damper: f32, user_gain: f32) {
        self.last_ff = ff;
        self.last_damper = damper;
        self.source.send_ff(ff, damper, user_gain);
    }

    fn get_ff_global_gain(&mut self) -> f32 {
        self.ff_gain
    }

    fn set_vibrations(&mut self, def: &VibrationDef) {
        self.last_vibrations = *def;
        self.source.set_vibrations(def);
    }

    fn set_engine_rpm(&mut self, rpm: f32, low: f32, high: f32) {
        self.source.set_engine_rpm(rpm, low, high);
    }
}

/// Finds a car's data folder, in this order:
/// 1. `car` as a path: a data folder (`car.ini` in it), or a car folder (with `data.acd` or
///    a `data` folder in it);
/// 2. `car` as a name in the game's own folder: `content/cars/<car>/data`, which is read out
///    of `data.acd` in memory when that is there (as the game does), else from the plain
///    `data` folder (the SDK's cars, unpacked mods);
/// 3. `car` as a name under a `cardata` folder next to the working directory or above the
///    program (extracted files: the test cars of the oracles).
pub fn find_car_data(car: &str) -> Result<PathBuf, String> {
    // an archive that is there but cannot be used (packed under another folder name, cut
    // short) is not a car; why is said if nothing else is found
    let mut unusable: Vec<String> = Vec::new();
    let mut has_car = |data: &Path| {
        unusable.extend(rustyac_content::acd::archive_error(data));
        rustyac_physics::data::exists(&data.join("car.ini"))
    };
    let mut tried = Vec::new();
    let direct = PathBuf::from(car);
    for folder in [direct.clone(), direct.join("data")] {
        if has_car(&folder) {
            return Ok(folder);
        }
    }
    tried.push(direct);
    if let Some(root) = ac_root() {
        let folder = root.join("content").join("cars").join(car).join("data");
        if has_car(&folder) {
            return Ok(folder);
        }
        tried.push(folder);
    }
    let mut roots = vec![PathBuf::from(".")];
    if let Ok(exe) = std::env::current_exe() {
        roots.extend(exe.ancestors().skip(1).map(Path::to_path_buf));
    }
    roots.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.."));
    for root in roots {
        let folder = root.join("cardata").join(car);
        if has_car(&folder) {
            return Ok(folder);
        }
        tried.push(folder);
    }
    let mut hint = if ac_root().is_none() { format!("; {}", rustyac_content::install::not_found_hint()) } else { String::new() };
    for message in &unusable {
        hint.push_str(&format!("; {message}"));
    }
    Err(format!(
        "the car {car:?} was not found: no car.ini in {}{hint}",
        tried.iter().take(3).map(|p| p.display().to_string()).collect::<Vec<_>>().join(", ")
    ))
}

/// Assetto Corsa's own folder: `AC_ROOT` if that is set, else Steam's usual place, else any
/// Steam library (see `rustyac_content::install`). Read only.
pub fn ac_root() -> Option<PathBuf> {
    rustyac_content::install::ac_root()
}

/// Finds a track's folder: a path to it, or its name under the game's `content/tracks`.
pub fn find_track(track: &str) -> Result<PathBuf, String> {
    let direct = PathBuf::from(track);
    if direct.vfs_is_dir() {
        return Ok(direct);
    }
    if let Some(root) = ac_root() {
        let folder = root.join("content").join("tracks").join(track);
        if folder.vfs_is_dir() {
            return Ok(folder);
        }
    }
    Err(format!(
        "the track {track:?} was not found: it is neither a folder nor a name under Assetto Corsa's content/tracks (set AC_ROOT if the game is not in a Steam library)"
    ))
}

/// A track and layout as the user names them (`--track`, `--layout`): a folder, a folder
/// name, the menu's name or a part of one; `race` is race.ini's `TRACK` and `CONFIG_TRACK`,
/// whose layout is taken when no layout is asked for and the track is the same. A track
/// that is unknown, not installed, made for Custom Shaders Patch only or encrypted is an
/// error that says which.
pub fn resolve_track(track: &str, layout: Option<&str>, race: Option<&(String, String)>) -> Result<rustyac_physics::track::catalog::Found, String> {
    let root = ac_root();
    let preferred = |name: &str| race.filter(|(race_track, _)| race_track.eq_ignore_ascii_case(name)).map(|(_, config)| config.clone());
    rustyac_physics::track::catalog::find(root.as_deref(), track, layout, &preferred)
}

/// Loads a track that was already resolved: the folder (or name) and the layout exactly.
fn load_resolved(track: &str, layout: &str) -> Result<(PathBuf, Track, rustyac_physics::track::TrackLoadReport), String> {
    let folder = find_track(track)?;
    let name = folder.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let entry = rustyac_physics::track::catalog::entry_of(&folder, &name, layout);
    if let Err(refusal) = rustyac_physics::track::catalog::check(&entry) {
        let what = if layout.is_empty() { name } else { format!("{name} / {layout}") };
        return Err(format!("the track {what} is refused: {refusal}"));
    }
    let (loaded, report) = load_track(&folder, layout)?;
    Ok((folder, loaded, report))
}

/// The car's folder name (the game's `unixName`) from its data folder: the folder's own name,
/// or its parent's when the folder is a car's `data` (`content/cars/<car>/data`).
pub fn car_name(data_path: &Path) -> String {
    let name = |path: &Path| path.file_name().map(|n| n.to_string_lossy().into_owned());
    match name(data_path) {
        Some(folder) if folder.eq_ignore_ascii_case("data") => data_path.parent().and_then(name).unwrap_or(folder),
        Some(folder) => folder,
        None => String::new(),
    }
}

/// The car's 3D model in the game's folder: `content/cars/<car>/<LOD_0 of data/lods.ini>`.
pub fn find_car_model(_car: &str, data_path: &Path) -> Option<PathBuf> {
    let name = car_name(data_path);
    let folder = ac_root()?.join("content").join("cars").join(&name);
    let lods = rustyac_physics::data::read(&data_path.join("lods.ini")).ok().flatten().map(|bytes| String::from_utf8_lossy(&bytes).into_owned()).unwrap_or_default();
    let mut in_first = false;
    for line in lods.lines() {
        let line = line.split(';').next().unwrap_or("").trim();
        if line.starts_with('[') {
            in_first = line == "[LOD_0]";
        } else if let (true, Some(("FILE", file))) = (in_first, line.split_once('=').map(|(k, v)| (k.trim(), v.trim()))) {
            let path = folder.join(file);
            if path.vfs_is_file() {
                return Some(path);
            }
        }
    }
    // no lods.ini: the model named like the folder
    let path = folder.join(format!("{name}.kn5"));
    path.vfs_is_file().then_some(path)
}

/// Where a car is put down: a point on the road and the direction its tail points
/// (`Car::forceRotation` takes the tail).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpawnPose {
    pub position: Vec3f,
    pub tail: Vec3f,
}

/// The car's unchanging facts, for the display.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CarInfo {
    pub data_path: PathBuf,
    pub name: String,
    /// The wheels' design positions in body axes (LF, RF, LR, RR).
    pub wheel_positions: [[f32; 3]; 4],
    pub tyre_width: [f32; 4],
    pub tyre_radius: [f32; 4],
    /// The track's folder, when the car is on one.
    pub track_folder: Option<PathBuf>,
    /// Its layout ("" for none).
    pub track_layout: String,
}

/// What the game reads of a car once, right after building it (`CarAvatar::initPhysics`, its
/// `CarPhysicsInfo`): from the first step on a turbo's controller has rewritten the boost
/// these two come from.
#[derive(Clone, Copy, Debug, Default)]
pub struct PhysicsInfo {
    pub max_power_w: f32,
    pub max_turbo_boost: f32,
}

impl PhysicsInfo {
    fn of(car: &VanillaCar<Driver>) -> PhysicsInfo {
        match &car.car.drivetrain {
            Some(drivetrain) => {
                let engine = drivetrain.engine();
                PhysicsInfo { max_power_w: engine.get_max_power_w(), max_turbo_boost: engine.get_max_turbo_boost() }
            }
            None => PhysicsInfo::default(),
        }
    }
}

/// One car on the endless flat road or on a track, and the count of its steps.
pub struct GameSim {
    pub setup: SimSetup,
    pub data_path: PathBuf,
    pub car: VanillaCar<Driver>,
    /// The car's numbers of the static shared-memory page, taken before its first step.
    pub physics_info: PhysicsInfo,
    /// Steps run so far; the next one runs at `setup.time_of_step(steps)`.
    pub steps: u64,
    /// The track, and its folder.
    pub track: Option<Arc<Track>>,
    pub track_folder: Option<PathBuf>,
    /// Where the car starts and where a reset puts it.
    pub spawn: SpawnPose,
    /// What loading the track said (for the console).
    pub track_summary: String,
    /// The lap list (the game's `RaceTimingServices` for this one car): what the lap
    /// displays and the shared memory show.
    pub lap_db: LapDb,
    /// Which session this is (0 at the start; a new car starts the next one).
    pub session_index: i32,
    /// The last change of an aid or of the brake bias, for the displays. Never read by the
    /// physics and not part of any dump.
    pub aid_note: AidNote,
}

/// A short on-screen note about the traction control, the ABS or the brake bias: what the
/// driver just set (AC shows a system message for the same presses).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AidNote {
    bytes: [u8; 40],
    /// The step count ([`GameSim::steps`]) when it was made.
    pub step: u64,
}

impl Default for AidNote {
    fn default() -> AidNote {
        AidNote { bytes: [0; 40], step: 0 }
    }
}

impl AidNote {
    /// How long a note is shown: 2.5 s of physics steps.
    pub const STEPS: u64 = 833;

    pub fn new(text: &str, step: u64) -> AidNote {
        let mut bytes = [0u8; 40];
        for (slot, byte) in bytes.iter_mut().zip(text.bytes().filter(u8::is_ascii)) {
            *slot = byte;
        }
        AidNote { bytes, step }
    }

    pub fn as_str(&self) -> &str {
        let end = self.bytes.iter().position(|b| *b == 0).unwrap_or(self.bytes.len());
        std::str::from_utf8(&self.bytes[..end]).unwrap_or("")
    }

    /// The text while the note is fresh at step count `steps`, else nothing.
    pub fn shown_at(&self, steps: u64) -> Option<&str> {
        (!self.as_str().is_empty() && steps.saturating_sub(self.step) < AidNote::STEPS).then(|| self.as_str())
    }
}

/// A level of the traction control or the ABS as the displays word it: `2/3`, `off`, or
/// `not fitted`. `mode` is `getCurrentMode`'s pair (level from 1, number of levels; (0, 0)
/// while switched off), which is what AC's own message shows.
pub fn aid_level_text(present: bool, mode: (u32, u32)) -> String {
    if !present {
        "not fitted".to_string()
    } else if mode.0 == 0 {
        "off".to_string()
    } else {
        format!("{}/{}", mode.0, mode.1)
    }
}

/// The brake bias as AC's message and its apps show it: `BrakeSystem::getFrontBias` (the
/// value of `acpmf_physics.brakeBias`) times 100 with one decimal, `58.0 %`.
pub fn bias_text(front_bias: f32) -> String {
    format!("{:.1} %", front_bias * 100.0)
}

/// Where the car is spawned: on the road at the origin, the nose towards +z (so the tail,
/// which is what `Car::forceRotation` takes, towards -z). The oracle's spawn.
const SPAWN_POSITION: Vec3f = Vec3f { x: 0.0, y: 0.0, z: 0.0 };
const SPAWN_TAIL: Vec3f = Vec3f { x: 0.0, y: 0.0, z: -1.0 };
const FLAT_SPAWN: SpawnPose = SpawnPose { position: SPAWN_POSITION, tail: SPAWN_TAIL };

/// The spawn point a set-up asks for on a track: `hotlap` (the track's hot-lap start, or the
/// first grid slot where it has none), `pit` (the first pit box) or `start` (the first grid slot).
fn spawn_on(track: &Track, spawn: &str) -> Result<SpawnPose, String> {
    let sets: &[&str] = match spawn {
        "hotlap" => &["HOTLAP_START", "START", "PIT"],
        "pit" => &["PIT"],
        "start" => &["START"],
        other => return Err(format!("the spawn point {other:?} is not one of hotlap, pit, start")),
    };
    for set in sets {
        if let Some((position, tail)) = track.spawn_pose(set, 0) {
            return Ok(SpawnPose { position, tail });
        }
    }
    Err(format!("the track {} has no spawn point for {spawn:?} (no AC_{}_0 node)", track.name, sets[0]))
}

fn build_car(
    setup: &SimSetup,
    data_path: &Path,
    physics_time: f64,
    driver: Driver,
    track: Option<&Arc<Track>>,
    spawn: &SpawnPose,
) -> Result<VanillaCar<Driver>, String> {
    if let Some(run) = setup.run_setup() {
        // exactly the car `tools/chassis_compare` holds against a recording of the game
        let mut run = run;
        run.clock_start_ms = physics_time;
        if let (Some(track), Some(oracle)) = (track, setup.oracle.as_ref().and_then(|o| o.track.as_ref())) {
            run.track = Some(rustyac_physics::car::replay::TrackRun {
                track: Arc::clone(track),
                position: spawn.position,
                tail: spawn.tail,
                armed: oracle.armed,
                allowed_tyres_out: oracle.allowed_tyres_out,
                objects: oracle.objects,
            });
        }
        if setup.oracle.as_ref().and_then(|o| o.collide).is_some_and(|c| c.collider_mesh) {
            // the recording's car had its own collider mesh: the game's file, read as the
            // oracle read it
            let name = car_name(data_path);
            let root = ac_root().ok_or("the recording was made with the car's collider mesh, which is in Assetto Corsa's folder (set AC_ROOT)")?;
            let colliders = rustyac_physics::car::colliders::load(data_path, Some(&root), &name)?;
            run.collide.mesh = Some(colliders.mesh.ok_or(format!("no collider.kn5 for {name} in {}", root.display()))?);
        }
        return VanillaCar::from_chassis(run.build(data_path)?, driver);
    }
    // `Car::Car`, the spawn (the game's own, with its drop onto the wheels), the session start
    let ground: Box<dyn RayTrackCollisionProvider> = match track {
        Some(track) => Box::new(TrackGround(Arc::clone(track))),
        None => Box::new(Ground::Flat),
    };
    let mut env = setup.env;
    let session = &setup.session;
    if session.wind_speed != 0.0 {
        // RaceManager::generateWind's job: PhysicsEngine::setWind
        env.set_wind(session.wind_speed, session.wind_direction_deg);
    }
    if track.is_some() {
        // RaceManager::setCurrentSession for a session that is not a race: leaving the track
        // costs the lap (penalty mode 1); `[RACE] PENALTIES` on: two tyres may be off, off:
        // any number (-1)
        env.penalty_mode = 1;
        env.allowed_tyres_out = if session.penalties { 2 } else { -1 };
        // the session of race.ini; without one, a hot-lap session from the hot-lap start
        env.session_type = session.session_type.unwrap_or(if setup.spawn == "hotlap" { 4 } else { 1 });
    }
    // the track's DRS zones (files recorded before they were ported have the wing free everywhere)
    env.track_drs_zones = setup.drs_zones;
    if setup.session_starts_at_spawn {
        // RaceManager::setCurrentSession: the session starts now (the wind's slow swing and
        // the automatic gearbox's 300 ms count from here)
        env.session_start_time_ms = physics_time;
    }
    // RaceManager::initOffline: a file with a hot-lap session arms the first lap
    let arm_first_lap = session.arm_first_lap.unwrap_or(env.session_type == 4);
    // the track's loose objects are made with the track, before the car
    let with_objects = setup.track_objects || setup.oracle.as_ref().and_then(|o| o.track.as_ref()).is_some_and(|t| t.objects);
    let objects: &[rustyac_physics::track::TrackObjectDef] = match track {
        Some(track) if with_objects => &track.objects,
        _ => &[],
    };
    let mut car = VanillaCar::new_with_objects(data_path, env, ground, setup.seed, physics_time, driver, objects)?;
    // the session of race.ini and assists.ini: the track's grip, ballast and restrictor
    // (CarAvatar::setBallastKG, setRestrictor), the aids (DrivingAssistManager)
    car.car.dynamic_track = session.dynamic_track;
    if session.ballast_kg > 0.0 {
        car.car.ballast_kg = session.ballast_kg;
    }
    if session.restrictor > 0.0 {
        car.car.set_restrictor(session.restrictor);
    }
    if let (Some((abs, traction_control, stability)), Some(aids)) = (session.assists, &mut car.car.aids) {
        aids.base_mut().apply_driving_assists(abs, traction_control, stability);
    }
    // CarAvatar::initPhysics: the car's collider mesh, when the game's folder has one for it
    // (without it the car has only its floor boxes: walls do not stop it)
    if let Some(root) = ac_root() {
        if let Err(message) = car.car.load_collider_mesh(&root) {
            eprintln!("collider mesh: {message}");
        }
    }
    if let Some(track) = track {
        // CarAvatar::setSpawnPositionIndex("PIT", 0): the car's pit box
        if let Some(&node) = track.spawn_positions.get("PIT").and_then(|slots| slots.first()) {
            car.car.pit_position = track.helper_nodes[node].local;
        }
        car.car.set_track(Arc::clone(track));
        // RaceManager::initOffline -> CarAvatar::armFirstLap, for a hot-lap session only: the
        // clock starts again when the car first crosses the line (lap 1 is line to line). In
        // any other session it keeps running from the session's start and the first lap
        // includes the run-up from the spawn point
        if arm_first_lap {
            car.car.transponder.arm_first_lap();
        }
    }
    // `CarAvatar::setAutoClutchEnabled`: the aid switches the automatic clutch at the start,
    // and with it the one on shifts
    car.car.autoclutch.use_auto_on_start = setup.auto_clutch;
    if setup.auto_clutch {
        car.car.autoclutch.use_auto_on_change = true;
    }
    car.car.auto_shifter.is_active = setup.auto_shifter;
    // `CarAvatar::setAutoBlip` (assists.ini AUTO_BLIP): the flag of `AutoBlip`; a car with an
    // electronic blip blips whatever it says
    if let Some(blip) = setup.auto_blip {
        car.car.auto_blip.is_active = blip;
        // the same job (0x1400d07a0), with the aid off: a car with an H-pattern gearbox also
        // stops cutting the ignition on up-shifts (switching the aid on does not restore it)
        if !blip {
            if let Some(drivetrain) = &mut car.car.drivetrain {
                let base = drivetrain.base_mut();
                if base.is_shifter_supported {
                    base.auto_cut_off_time = 0.0;
                }
            }
        }
    }
    car.car.force_rotation(&spawn.tail);
    car.car.force_position(&spawn.position);
    car.car.session_start()?;
    // a saved setup, loaded as the setup screen's "Load" does it, before the first step
    if let Some(file) = &session.setup_file {
        let saved = rustyac_physics::data::ini::IniReader::load(file)?;
        let name = file.file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        car.car.load_setup(&saved, &name).map_err(|e| format!("{}: {e}", file.display()))?;
    }
    // PhysicsEngine::setSessionInfo: no contacts are looked for during the first 0.75 s
    car.car.reset_collisions_for_new_session();
    Ok(car)
}

impl GameSim {
    pub fn new(setup: SimSetup, source: Box<dyn DriverSource>) -> Result<GameSim, String> {
        let data_path = find_car_data(&setup.car)?;
        let driver = Driver::new(source, setup.ff_gain);
        let (mut track, mut track_folder, mut spawn, mut track_summary) = (None, None, FLAT_SPAWN, String::new());
        if !setup.track.is_empty() {
            if setup.oracle.is_some() {
                return Err("an oracle set-up runs on the oracle's own road, not on a track".to_string());
            }
            let (folder, mut loaded, report) = load_resolved(&setup.track, &setup.layout)?;
            // RaceManager::initOffline: the spawn set of the session (a hot-lap session has
            // `HOTLAP_START`, practice `PIT`, a race `START`; the last two are then cast twice)
            let set = match setup.spawn.as_str() {
                "pit" => "PIT",
                "start" => "START",
                _ => "HOTLAP_START",
            };
            rustyac_physics::track::init_respawn_position_set(&mut loaded, set);
            spawn = spawn_on(&loaded, &setup.spawn)?;
            track_summary = report.summary(&loaded);
            track = Some(Arc::new(loaded));
            track_folder = Some(folder);
        }
        if let Some(oracle) = setup.oracle.as_ref().and_then(|o| o.track.as_ref()) {
            // a recording of the game on a track: the same track, the recording's spawn
            let (folder, loaded, report) = load_resolved(&oracle.folder, &oracle.layout)?;
            let v = |a: &[f32; 3]| Vec3f::new(a[0], a[1], a[2]);
            spawn = SpawnPose { position: v(&oracle.position), tail: v(&oracle.tail) };
            track_summary = report.summary(&loaded);
            track = Some(Arc::new(loaded));
            track_folder = Some(folder);
        }
        let mut driver = driver;
        if let Some(track) = &track {
            driver.source.set_track(track);
        }
        let car = build_car(&setup, &data_path, setup.clock_start_ms, driver, track.as_ref(), &spawn)?;
        let lap_db = LapDb::new(track.as_ref().map(|t| t.sectors_normalized_positions.len()).unwrap_or(0));
        let physics_info = PhysicsInfo::of(&car);
        Ok(GameSim { setup, data_path, car, physics_info, steps: 0, track, track_folder, spawn, track_summary, lap_db, session_index: 0, aid_note: AidNote::default() })
    }

    /// The physics clock after the last step, ms.
    pub fn clock_ms(&self) -> f64 {
        if self.steps == 0 {
            self.setup.clock_start_ms
        } else {
            self.setup.time_of_step(self.steps - 1)
        }
    }

    /// What the display needs to know of the car once: where its data is and where its
    /// wheels sit.
    pub fn car_info(&self) -> CarInfo {
        let car = &self.car.car;
        let mut info =
            CarInfo { data_path: self.data_path.clone(), name: self.setup.car.clone(), track_folder: self.track_folder.clone(), track_layout: self.track.as_ref().map(|t| t.config.clone()).unwrap_or_default(), ..CarInfo::default() };
        for index in 0..4.min(car.tyres.len()) {
            let p = car.suspensions[index].get_base_position();
            info.wheel_positions[index] = [p.x, p.y, p.z];
            info.tyre_width[index] = car.tyres[index].data.width;
            info.tyre_radius[index] = car.tyres[index].data.radius;
        }
        info
    }

    /// Seconds simulated so far.
    pub fn sim_seconds(&self) -> f64 {
        self.steps as f64 * DT as f64
    }

    /// A new car at the spawn point, built at the current clock. The driver stays.
    fn rebuild(&mut self) -> Result<(), String> {
        if self.setup.oracle.is_some() {
            return Err("an oracle set-up cannot be rebuilt mid-run".to_string());
        }
        if self.setup.session_transfer {
            // the handler `Track` hangs on a new session (lambda @ 0x140277740): the next
            // session starts from the session's start grip plus SESSION_TRANSFER of what
            // this one gained, with its own random part
            if let Some(mut track) = self.car.car.dynamic_track {
                self.session_index += 1;
                let mut rand = rustyac_physics::session::MsvcRand(self.setup.seed.wrapping_add(self.session_index as u32));
                track.on_new_session(self.session_index, rand.next());
                track.step(0);
                self.setup.env.dynamic_grip_level = track.dynamic_grip_level;
                self.setup.session.dynamic_track = Some(track);
            }
        }
        let placeholder = Driver::new(Box::new(ReplaySource::default()), self.setup.ff_gain);
        let mut car = build_car(&self.setup, &self.data_path, self.clock_ms(), placeholder, self.track.as_ref(), &self.spawn)?;
        std::mem::swap(&mut car.device, &mut self.car.device);
        // the engine's step count goes on (lap times carry its remainder of three)
        car.car.step_counter = self.car.car.step_counter;
        self.physics_info = PhysicsInfo::of(&car);
        self.car = car;
        self.lap_db = LapDb::new(self.lap_db.sector_count);
        Ok(())
    }

    fn apply(&mut self, events: u32, bias_clicks: i32) -> Result<(), String> {
        if events & event::REBUILD != 0 {
            self.rebuild()?;
        }
        // a new session's jobs in the order the game queues them: the track's handler (the
        // loose objects go home) was registered before the car's
        if events & (event::OBJECTS_HOME | event::RESET) != 0 && !self.car.car.core.track_objects.is_empty() {
            self.car.car.queue(|car| car.core.reset_track_objects());
        }
        if events & event::RESET != 0 {
            // a teleport is a job of the game's main thread: it runs at the start of the step
            let spawn = self.spawn;
            // (the game's restart leaves the armed flag of a hot-lap file as it is)
            let armed = self.track.is_some() && self.setup.session.arm_first_lap.unwrap_or(self.car.car.env.session_type == 4);
            self.car.car.queue(move |car| {
                car.force_rotation(&spawn.tail);
                car.force_position(&spawn.position);
                // the game's teleport repairs the car (CarAvatar::forcePosition's job: body
                // and suspension damage; the engine is renewed by Car::forcePosition itself)
                car.set_damage_level(0.0);
                car.reset_suspension_damage_level();
                // back at the spawn point the timer starts again, as at a session's start
                // (CarAvatar::onNewSession queues TimeTransponder::reset); the lap list stays
                car.transponder.reset();
                if armed {
                    car.transponder.arm_first_lap();
                }
            });
            self.lap_db.current_splits.clear();
        }
        if events & event::TO_TRACK != 0 {
            // the nearest point of the AI line, facing along it; without an AI line, the spawn
            let body = self.car.car.core.get_world_matrix(self.car.car.body).m[3];
            let here = Vec3f::new(body[0], body[1], body[2]);
            let pose = match self.track.as_ref().and_then(|track| track.pose_on_ai_line(&here)) {
                Some((position, tail)) => SpawnPose { position, tail },
                None => self.spawn,
            };
            self.car.car.queue(move |car| {
                car.force_rotation(&pose.tail);
                car.force_position(&pose.position);
                car.set_damage_level(0.0);
                car.reset_suspension_damage_level();
                // the game's teleport only forgets the timing lines crossed so far (a lap past
                // its first sector line then no longer counts); being lifted back onto the road
                // is help the game does not have, so the lap in progress is also marked as cut
                car.transponder.add_cut();
            });
        }
        // the notes below are for the displays only: they read what the calls left behind
        let steps = self.steps;
        if bias_clicks != 0 {
            if let Some(brakes) = &mut self.car.car.brake_system {
                let before = brakes.get_front_bias();
                brakes.set_manual_front_bias(bias_clicks);
                let after = brakes.get_front_bias();
                // (a car without a cockpit control, or the control at its stop)
                let same = if before.to_bits() == after.to_bits() { " (no change)" } else { "" };
                self.aid_note = AidNote::new(&format!("Brake bias {}{same}", bias_text(after)), steps);
            }
        }
        if let Some(aids) = &mut self.car.car.aids {
            let aids = aids.base_mut();
            for (bit, direction) in [(event::TC_UP, 1), (event::TC_DN, -1)] {
                if events & bit != 0 {
                    aids.traction_control.cycle_mode(direction);
                    let tc = &aids.traction_control;
                    self.aid_note = AidNote::new(&format!("TC {}", aid_level_text(tc.is_present, tc.get_current_mode())), steps);
                }
            }
            for (bit, direction) in [(event::ABS_UP, 1), (event::ABS_DN, -1)] {
                if events & bit != 0 {
                    aids.abs.cycle_mode(direction);
                    let abs = &aids.abs;
                    self.aid_note = AidNote::new(&format!("ABS {}", aid_level_text(abs.is_present, abs.get_current_mode())), steps);
                }
            }
        } else if events & (event::TC_UP | event::TC_DN) != 0 {
            self.aid_note = AidNote::new("TC not fitted", steps);
        } else if events & (event::ABS_UP | event::ABS_DN) != 0 {
            self.aid_note = AidNote::new("ABS not fitted", steps);
        }
        if events & event::AUTO_SHIFTER != 0 {
            self.car.car.auto_shifter.is_active = !self.car.car.auto_shifter.is_active;
        }
        // the cockpit of the hybrid system and the engine brake: what the game's notifiers
        // and key handler do on a press, with the game's messages on the console
        {
            let car = &mut self.car.car;
            let say = |title: &str, value: Option<String>| match value {
                Some(value) => println!("{title}: {value}"),
                None => println!("{title}: Not available"),
            };
            for (up, dn, which) in [
                (event::ENGINE_BRAKE_UP, event::ENGINE_BRAKE_DN, 0),
                (event::MGUK_DELIVERY_UP, event::MGUK_DELIVERY_DN, 1),
                (event::MGUK_RECOVERY_UP, event::MGUK_RECOVERY_DN, 2),
            ] {
                let dir = if events & dn != 0 {
                    -1
                } else if events & up != 0 {
                    1
                } else {
                    continue;
                };
                match which {
                    0 => {
                        let count = car.engine_brake_settings();
                        say("Engine Brake", car.cycle_engine_brake(dir).map(|index| format!("{}/{count}", index + 1)));
                    }
                    1 => {
                        let index = car.cycle_ers_power(dir);
                        let name = index.and_then(|i| car.ers.as_ref().and_then(|ers| ers.power_controllers.get(i as usize)).map(|c| c.name.clone()));
                        say("MGU-K Delivery", name);
                    }
                    _ => say("MGU-K Recovery", car.cycle_ers_recovery(dir).map(|level| format!("{}%", level * 10))),
                }
            }
            if events & event::MGUH_MODE != 0 {
                say("MGU-H Mode", car.cycle_ers_heat_charging().map(|battery| if battery { "Battery".to_string() } else { "Motor".to_string() }));
            }
        }
        // the automatic clutch aid is the session's setting, except while the driver holds
        // the clutch himself (the aid's last act in a step is to overwrite the pedal)
        self.car.car.autoclutch.use_auto_on_start = self.setup.auto_clutch && events & event::MANUAL_CLUTCH == 0;
        Ok(())
    }

    /// One physics step with whatever the driver's source says; returns what it said (the
    /// step's line in an input file).
    pub fn step(&mut self) -> Result<StepInput, String> {
        let (events, bias_clicks) = self.car.device.source.take_events();
        self.apply(events, bias_clicks)?;
        if self.car.device.source.wants_probe() {
            let probe = CarProbe::capture(&mut self.car.car);
            self.car.device.source.set_probe(&probe);
        }
        self.car.device.polled = false;
        self.car.step(DT, self.setup.time_of_step(self.steps));
        self.steps += 1;
        // Car::evOnSectorSplit / evOnLapCompleted -> the lap list
        if self.track.is_some() {
            let (laps, splits) = self.car.car.transponder.take_events();
            for split in &splits {
                self.lap_db.on_sector_split(split);
            }
            for lap in &laps {
                self.lap_db.on_lap_completed(lap);
            }
        }
        let device = &self.car.device;
        Ok(StepInput {
            controls: device.reported,
            headlights: device.headlights,
            events,
            bias_clicks,
            device: device.source.device_id(),
        })
    }

    /// One step of a recorded drive.
    pub fn step_recorded(&mut self, step: &StepInput) -> Result<(), String> {
        self.car.device.source.load(step);
        self.step().map(|_| ())
    }
}
