// SPDX-License-Identifier: GPL-3.0-or-later

//! The simulation the game runs: one [`VanillaCar`], built and spawned the way the oracle
//! builds and spawns the game's car, and stepped with nothing but what a driver's device
//! reports. Live driving, `--replay` and the tests all go through [`GameSim::step`]; there is
//! no other way the game touches the car.

use rustyac_physics::car::setup::SetupJob;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use rustyac_content::vfs::PathExt;
use rustyac_physics::car::replay::Ground;
use rustyac_net::net_car::NetCarStateProvider;
use rustyac_physics::track::{load_track, LapDb, Track, TrackGround};

use crate::online::{OnlineHooks, OnlineSession, ServerRules};
use rustyac_physics::tyre::RayTrackCollisionProvider;
use rustyac_physics::car::{CarControls, CarControlsInput, ControlsProvider, VanillaCar, VibrationDef};
use rustyac_physics::vecmath::Vec3f;

use crate::input_file::{event, Opponent, OpponentDrive, SimSetup, StepInput};
use rustyac_physics::engine::{CarSlot, DeviceRef, PhysicsEngine};

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

    /// A gain of the wheel's `DICarControl::ffUpgrades` (0 curbs, 1 road, 2 slips, 3 ABS): the
    /// force feedback app's spinners.
    fn set_ff_upgrade(&mut self, _which: usize, _value: f32) {}

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

/// A parked car: on its brakes (Task 24).
pub struct ParkedSource;

impl DriverSource for ParkedSource {
    fn acquire(&mut self, controls: &mut CarControls, _dt: f32, _input: &CarControlsInput) {
        *controls = CarControls { brake: 1.0, clutch: 1.0, ..CarControls::default() };
    }
}

/// The inputs of a recorded drive, one step of the file per step of the session; on the
/// brakes when the file ends (Task 24).
pub struct RecordedSource {
    pub steps: Vec<StepInput>,
    pub at: usize,
    current: Option<StepInput>,
}

impl RecordedSource {
    pub fn new(steps: Vec<StepInput>) -> RecordedSource {
        RecordedSource { steps, at: 0, current: None }
    }
}

impl DriverSource for RecordedSource {
    fn take_events(&mut self) -> (u32, i32) {
        // (the file's teleports and new sessions were its own session's: not this one's)
        self.current = self.steps.get(self.at).copied();
        self.at += 1;
        (0, 0)
    }

    fn acquire(&mut self, controls: &mut CarControls, _dt: f32, _input: &CarControlsInput) {
        match &self.current {
            Some(step) => *controls = step.controls,
            None => *controls = CarControls { brake: 1.0, clutch: 1.0, ..CarControls::default() },
        }
    }

    fn headlights(&mut self) -> bool {
        self.current.is_some_and(|step| step.headlights)
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
    /// The brake pedal, held as well (`--flat-out-brake`): the front wheels hold the car
    /// while the engine spins the rear ones, a burnout on the spot. 0: a launch.
    pub brake: f32,
}

impl FlatOutSource {
    /// With the brake pedal held at `brake` (0..1) as well.
    pub fn with_brake(brake: f32) -> FlatOutSource {
        FlatOutSource { steps: 0, brake }
    }
}

impl DriverSource for FlatOutSource {
    fn acquire(&mut self, controls: &mut CarControls, _dt: f32, _input: &CarControlsInput) {
        self.steps += 1;
        let clutch = if self.steps < 333 { 0.0 } else { 1.0 };
        *controls = CarControls { clutch, gas: 1.0, brake: self.brake, ..CarControls::default() };
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

    fn set_ff_upgrade(&mut self, which: usize, value: f32) {
        self.inner.set_ff_upgrade(which, value);
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
    /// Task 27: AC's AI driver of the car. While there is one the source is not asked for
    /// the controls: the driver writes them.
    pub ai: Option<Box<rustyac_physics::ai::AiDriver>>,
}

impl Driver {
    pub fn new(source: Box<dyn DriverSource>, ff_gain: f32) -> Driver {
        Driver {
            ai: None,
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

    fn acquire_controls_car(&mut self, car: &mut rustyac_physics::car::RollingChassis, dt: f32) {
        match &mut self.ai {
            Some(ai) => {
                ai.acquire(car, dt);
                self.reported = car.controls;
                self.polled = true;
            }
            None => {
                let input = CarControlsInput { steer_lock: car.steer_lock, speed: car.speed };
                self.acquire_controls(&mut car.controls, dt, &input);
            }
        }
    }

    fn is_ai(&self) -> bool {
        self.ai.is_some()
    }

    fn on_forced_position(&mut self, car: &mut rustyac_physics::car::RollingChassis) {
        if let Some(ai) = &mut self.ai {
            ControlsProvider::on_forced_position(&mut **ai, car);
        }
    }

    fn on_lap_completed(&mut self, valid: bool) {
        if let Some(ai) = &mut self.ai {
            ControlsProvider::on_lap_completed(&mut **ai, valid);
        }
    }

    fn on_new_session(&mut self, car: &mut rustyac_physics::car::RollingChassis, info: &rustyac_physics::ai::SessionInfo) {
        if let Some(ai) = &mut self.ai {
            ControlsProvider::on_new_session(&mut **ai, car, info);
        }
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
            // a car given by its folder (`--car <folder>`): the models beside its data
            if let Some(own) = data_path.parent().map(|p| p.join(file)).filter(|p| data_path.file_name().is_some_and(|n| n == "data") && p.vfs_is_file()) {
                return Some(own);
            }
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
    /// The setup's `render_seed`.
    pub render_seed: u32,
    /// Task 31: the player's car as the setup editor sees it when the session starts
    pub setup: Option<Box<rustyac_gui::setup_car::SetupSnapshot>>,
    /// Another car of the session: the skin it was asked for (`None`: the car's first)
    pub skin: Option<String>,
    /// How many cars the session has (`RaceManager::carsToBeLoaded`)
    pub cars: usize,
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
    /// New sessions so far (N: a new car at the spawn point): the picture's handlers of the
    /// game's new-session event run when it changes
    pub new_sessions: u32,
    /// The laps the player's car finished in the last step (`Car::evOnLapCompleted`), for the
    /// handlers that run after the step (the ghost car's recorder)
    pub step_laps: Vec<rustyac_physics::track::timing::OnLapCompletedEvent>,
    /// `Car::fuelLapEvaluator`: the fuel a lap takes, the distance per litre (for the displays)
    pub fuel_evaluator: rustyac_physics::car::fuel_lap_evaluator::FuelLapEvaluator,
    /// The last change of an aid or of the brake bias, for the displays. Never read by the
    /// physics and not part of any dump.
    pub aid_note: AidNote,
    /// Task 24: the other cars of the session, in creation order (car 1, 2, ...).
    pub opponents: Vec<OpponentCar>,
    /// The engine all cars are stepped by; `None` with one car, which steps itself exactly
    /// as it did before there were several.
    pub engine: Option<PhysicsEngine>,
    /// Task 25: the online session (the client of a server and the other players' cars).
    pub online: Option<Box<OnlineSession>>,
    /// The server's rules of that session.
    pub rules: Option<ServerRules>,
    /// Task 27: the AI drivers' random stream and session while one car is stepped alone
    /// (with other cars the engine has both).
    pub solo_ai: rustyac_physics::ai::SoloAi,
    /// Task 28: the referee (AC's `RaceManager` and what hangs on it); `None`: no race rules.
    pub referee: Option<Box<rustyac_physics::race::Referee>>,
    /// Task 31: the setup editor's jobs for the player's car (the game's
    /// `PhysicsAvatar::commandQueue`), done before the next step
    pub setup_jobs: Vec<SetupJob>,
    /// the jobs of a recording: each with the step it is done before
    pub setup_schedule: std::collections::VecDeque<(u64, SetupJob)>,
    /// every job done so far, with its step (a recording keeps them beside its input file)
    pub setup_log: Vec<(u64, SetupJob)>,
}

/// Another car of the session with its driver.
pub struct OpponentCar {
    pub car: VanillaCar<Driver>,
    pub setup: Opponent,
    pub data_path: PathBuf,
    pub spawn: SpawnPose,
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
    spawn_slot(track, spawn, 0)
}

/// [`spawn_on`] for car `index` of the session: the slot with its number (`AC_PIT_<n>`,
/// `AC_START_<n>`). A hot-lap start has one slot: the other cars stand on the grid's.
fn spawn_slot(track: &Track, spawn: &str, index: usize) -> Result<SpawnPose, String> {
    if index > 0 {
        let sets: &[&str] = match spawn {
            "pit" => &["PIT"],
            _ => &["START", "PIT"],
        };
        for set in sets {
            if let Some((position, tail)) = track.spawn_pose(set, index) {
                return Ok(SpawnPose { position, tail });
            }
        }
        return Err(format!("the track {} has no slot {index} for another car (it has {} grid and {} pit slots)", track.name, track.spawn_slots("START"), track.spawn_slots("PIT")));
    }
    spawn_first(track, spawn)
}

/// Where a car of race.ini stands (`CarAvatar::goToSpawnPosition` of the session's spawn set):
/// the slot of its own number of the set (`place.pit` in the pits, `place.start` on the grid;
/// the hot-lap start has one slot, on which the game puts every car). A number the set has no
/// slot for: the game takes a node again when the set was listed twice and else puts the car
/// at the origin of the world; here the slots go round again, with a line on the console.
fn spawn_place(track: &Track, spawn: &str, place: &crate::input_file::GridPlace, car: &str) -> Result<SpawnPose, String> {
    let (set, index) = match spawn {
        "pit" => ("PIT", place.pit as i64),
        "start" => ("START", place.start as i64),
        "hotlap" => ("HOTLAP_START", 0),
        other => return Err(format!("the spawn point {other:?} is not one of hotlap, pit, start")),
    };
    let slots = track.spawn_slots(set);
    if slots == 0 && spawn == "hotlap" {
        // (a track without a hot-lap start: the grid, as for a session of one car)
        return spawn_first(track, spawn);
    }
    if slots == 0 {
        return Err(format!("the track {} has no AC_{set}_0 node: no place for {car}", track.name));
    }
    let wrapped = index.rem_euclid(slots as i64) as usize;
    if wrapped as i64 != index {
        eprintln!("WARNING: {car} has slot {index} of {set}, the track has {slots}: it stands on slot {wrapped} (Assetto Corsa would put it at the origin of the world, or on a slot that is taken)");
    }
    let (position, tail) = track.spawn_pose(set, wrapped).ok_or_else(|| format!("no slot {wrapped} of {set}"))?;
    Ok(SpawnPose { position, tail })
}

fn spawn_first(track: &Track, spawn: &str) -> Result<SpawnPose, String> {
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

/// The settings of a session: of its one car, or of the engine and every car in it.
fn session_env(setup: &SimSetup, physics_time: f64, on_track: bool, rules: Option<&ServerRules>) -> rustyac_physics::car::ChassisEnvironment {
    let mut env = setup.env;
    let session = &setup.session;
    if session.wind_speed != 0.0 {
        // RaceManager::generateWind's job: PhysicsEngine::setWind
        env.set_wind(session.wind_speed, session.wind_direction_deg);
    }
    if on_track {
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
        // the automatic gearbox's 300 ms count from here); a race with AI drivers may have
        // its green light a little later (`--race-start-in`)
        env.session_start_time_ms = physics_time + setup.ai.start_delay_ms;
    }
    if let Some(rules) = rules {
        // an online session: the server's rules (ACClient::handshakeTCP writes them into the
        // engine), the session's kind, and the grip the server gives the track
        env.allowed_tyres_out = rules.allowed_tyres_out;
        env.allow_tyre_blankets = rules.tyre_blankets;
        if (0..=2).contains(&rules.jump_start_mode) {
            env.jump_start_penalty_mode = rules.jump_start_mode;
        }
        env.mechanical_damage_rate = rules.damage_rate;
        env.fuel_consumption_rate = rules.fuel_rate;
        env.tyre_consumption_rate = rules.tyre_wear_rate;
        env.session_type = rules.session_type;
        env.penalty_mode = rules.penalty_mode();
        env.dynamic_grip_level = rules.grip;
        // RaceManager::initOnline: the gearbox is locked from 12 s before a start
        env.lock_gearbox_at_start_time_ms = 12000.0;
    }
    env
}

#[allow(clippy::too_many_arguments)]
fn build_car(
    setup: &SimSetup,
    data_path: &Path,
    physics_time: f64,
    driver: Driver,
    track: Option<&Arc<Track>>,
    spawn: &SpawnPose,
    mut engine: Option<&mut PhysicsEngine>,
    slot: usize,
    rules: Option<&ServerRules>,
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
    let ground: Box<dyn RayTrackCollisionProvider + Send> = match track {
        Some(track) => Box::new(TrackGround(Arc::clone(track))),
        None => Box::new(Ground::Flat),
    };
    // the session's settings (the wind, the rules of a session on a track, the DRS zones, the
    // session's start): `session_env`
    let env = session_env(setup, physics_time, track.is_some(), rules);
    let session = &setup.session;
    // RaceManager::initOffline: a file with a hot-lap session arms the first lap
    let arm_first_lap = session.arm_first_lap.unwrap_or(env.session_type == 4);
    // the track's loose objects are made with the track, before the car
    let with_objects = setup.track_objects || setup.oracle.as_ref().and_then(|o| o.track.as_ref()).is_some_and(|t| t.objects);
    let objects: &[rustyac_physics::track::TrackObjectDef] = match track {
        Some(track) if with_objects => &track.objects,
        _ => &[],
    };
    let mut car = match engine.as_deref_mut() {
        None => VanillaCar::new_with_objects(data_path, env, ground, setup.seed, physics_time, driver, objects)?,
        Some(engine) => {
            // several cars: built into the engine's one world (which holds the track's
            // objects already), everything that makes bodies or shapes in one go
            let env = rustyac_physics::car::ChassisEnvironment { is_first_car: engine.car_count() == 0, ..env };
            let root = ac_root();
            let chassis = engine.build_car(|core, guid| {
                let mut chassis = rustyac_physics::car::RollingChassis::new_in_core(core, guid, data_path, env, ground, setup.seed, physics_time)?;
                chassis.install_aero()?;
                chassis.install_brakes()?;
                chassis.install_drivetrain()?;
                chassis.install_aids()?;
                chassis.install_telemetry()?;
                if let Some(root) = &root {
                    if let Err(message) = chassis.load_collider_mesh(root) {
                        eprintln!("collider mesh: {message}");
                    }
                }
                Ok(chassis)
            })?;
            VanillaCar::from_chassis(chassis, driver)?
        }
    };
    // the session of race.ini and assists.ini: the track's grip, ballast and restrictor
    // (CarAvatar::setBallastKG, setRestrictor), the aids (DrivingAssistManager)
    match engine.as_deref_mut() {
        None => car.car.dynamic_track = session.dynamic_track,
        // (online the server gives the grip: Track::setGripLevelExternal)
        Some(_) if rules.is_some() => {}
        // (the track's grip is the engine's: the laps of all cars count)
        Some(engine) if slot == 0 => engine.dynamic_track = session.dynamic_track,
        Some(_) => {}
    }
    if session.ballast_kg > 0.0 {
        car.car.ballast_kg = session.ballast_kg;
    }
    if session.restrictor > 0.0 {
        car.car.set_restrictor(session.restrictor);
    }
    if let (Some((abs, traction_control, stability)), Some(aids)) = (session.assists, &mut car.car.aids) {
        aids.base_mut().apply_driving_assists(abs, traction_control, stability);
    }
    if let (Some(rules), Some(aids)) = (rules, &mut car.car.aids) {
        // DrivingAssistManager with a client: the server's ABS and traction control, and
        // no stability control where the server does not allow it
        let stability = if rules.stability { session.assists.map_or(0.0, |assists| assists.2) } else { 0.0 };
        aids.base_mut().apply_driving_assists(rules.abs, rules.tc, stability);
    }
    // CarAvatar::initPhysics: the car's collider mesh, when the game's folder has one for it
    // (without it the car has only its floor boxes: walls do not stop it)
    if engine.is_none() {
        if let Some(root) = ac_root() {
            if let Err(message) = car.car.load_collider_mesh(&root) {
                eprintln!("collider mesh: {message}");
            }
        }
    }
    if let Some(track) = track {
        // CarAvatar::setSpawnPositionIndex("PIT", n): the car's pit box
        if let Some(&node) = track.spawn_positions.get("PIT").and_then(|slots| slots.get(slot).or(slots.first())) {
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
    // ("AUTO CLUTCH TURNED OFF BY THE SERVER")
    let auto_clutch = setup.auto_clutch && rules.is_none_or(|rules| rules.auto_clutch);
    car.car.autoclutch.use_auto_on_start = auto_clutch;
    if auto_clutch {
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
    match engine {
        None => car.car.reset_collisions_for_new_session(),
        Some(engine) => engine.reset_collisions_for_new_session(),
    }
    Ok(car)
}

/// A remote car's collider mesh from the game's folder (`content/cars/<model>/collider.kn5`).
fn remote_collider(data_path: &Path, root: Option<&Path>, model: &str) -> Option<rustyac_physics::car::ColliderMesh> {
    match rustyac_physics::car::colliders::load(data_path, root, model) {
        Ok(colliders) => colliders.mesh,
        Err(message) => {
            eprintln!("collider mesh of {model}: {message}");
            None
        }
    }
}

/// `Sim::addNetCar` for every other slot of the server, in the order of its car list: a body
/// with the car's collider mesh in the engine's world, a ghost until its first state arrives.
/// `first_guid`: the number of the first remote car among the session's cars.
fn add_remote_cars(session: &mut OnlineSession, engine: &mut PhysicsEngine, track: Option<&Arc<Track>>, first_guid: usize) -> Result<(), String> {
    let root = ac_root();
    let entries = std::mem::take(&mut session.remote_entries);
    for (number, entry) in entries.into_iter().enumerate() {
        let data_path = find_car_data(&entry.model).map_err(|e| format!("the server has a {} (slot {}): {e}", entry.model, entry.session_id))?;
        let mesh = remote_collider(&data_path, root.as_deref(), &entry.model);
        // TrackAvatar::getSpawnPosition("PIT", slot): the slot's pit box
        let pit_box = track
            .and_then(|track| track.spawn_positions.get("PIT").and_then(|slots| slots.get(entry.session_id as usize).or(slots.first())).map(|node| track.helper_nodes[*node].local))
            .unwrap_or(rustyac_physics::vecmath::Mat44f::IDENTITY);
        session.add_remote_car(&mut engine.core, entry, data_path, (first_guid + number) as u32, mesh.as_ref(), pit_box);
    }
    Ok(())
}

/// The driver of another car.
fn opponent_driver(opponent: &Opponent, track: Option<&Arc<Track>>) -> Result<Driver, String> {
    let mut source: Box<dyn DriverSource> = match &opponent.drive {
        OpponentDrive::Parked => Box::new(ParkedSource),
        OpponentDrive::Auto => {
            if track.is_none() {
                return Err(format!("the opponent {}: the line follower needs a track", opponent.car));
            }
            Box::new(SpawnSequence::new(crate::autodrive::AutoDriver::new(), true))
        }
        OpponentDrive::Recorded(file) => Box::new(RecordedSource::new(crate::input_file::InputFile::read(file)?.steps)),
        // (the AI driver is made once all cars are there: `GameSim::start_ai`)
        OpponentDrive::Ai { .. } => Box::new(ParkedSource),
    };
    if let Some(track) = track {
        source.set_track(track);
    }
    Ok(Driver::new(source, 1.0))
}

/// `new AIDriver(car)` for a car of the session: its pit-stop times and the fuel a lap is
/// expected to take first (`Car::Car` works both out for every car; only a driver reads them).
fn make_ai_driver(car: &mut rustyac_physics::car::RollingChassis, session: &rustyac_physics::ai::SessionInfo, rand: &mut rustyac_physics::session::MsvcRand, setup_file: Option<&Path>) -> Result<rustyac_physics::ai::AiDriver, String> {
    car.init_pitstop_timings()?;
    car.init_expected_fuel_per_lap()?;
    car.ai_input.session = *session;
    rustyac_physics::ai::AiDriver::new(car, session, rand, &rustyac_physics::ai::driver::AiFiles { ai_setup: setup_file })
}

/// The session's cars: the driver's, and with other cars the engine that steps them all.
#[allow(clippy::type_complexity)]
fn build_world(
    setup: &SimSetup,
    data_path: &Path,
    physics_time: f64,
    driver: Driver,
    track: Option<&Arc<Track>>,
    spawn: &SpawnPose,
    rules: Option<&ServerRules>,
) -> Result<(VanillaCar<Driver>, Vec<OpponentCar>, Option<PhysicsEngine>), String> {
    // (an online session always has the engine: the other players' cars are bodies in it)
    if setup.opponents.is_empty() && rules.is_none() {
        return Ok((build_car(setup, data_path, physics_time, driver, track, spawn, None, 0, None)?, Vec::new(), None));
    }
    if setup.oracle.is_some() {
        return Err("an oracle set-up has one car".to_string());
    }
    if ac_root().is_none() {
        eprintln!("WARNING: Assetto Corsa's folder was not found (set AC_ROOT): without their collider meshes the cars drive through each other");
    }
    let cpus = std::thread::available_parallelism().map_or(1, |n| n.get());
    let threads = setup.physics_threads.unwrap_or_else(|| rustyac_physics::engine::worker_threads(physics_threading_key(), cpus));
    let mut engine = PhysicsEngine::new(session_env(setup, physics_time, track.is_some(), rules), threads);
    if let Some(track) = track {
        engine.set_track(Arc::clone(track), setup.track_objects);
    }
    let car = build_car(setup, data_path, physics_time, driver, track, spawn, Some(&mut engine), setup.player_place.map_or(0, |place| place.pit), rules)?;
    let mut opponents = Vec::new();
    for (index, opponent) in setup.opponents.iter().enumerate() {
        let slot = index + 1;
        let opponent_data = find_car_data(&opponent.car)?;
        let pose = match (track, &opponent.place) {
            // a car of race.ini: the slot the game gives it
            (Some(track), Some(place)) => spawn_place(track, &setup.spawn, place, &opponent.car)?,
            (Some(track), None) => spawn_slot(track, &setup.spawn, slot)?,
            // the flat road: in a row behind the driver's car
            (None, _) => SpawnPose { position: Vec3f::new(0.0, 0.0, -8.0 * slot as f32), tail: SPAWN_TAIL },
        };
        // the other cars have the session's weather and rules, not the driver's own ballast,
        // restrictor or setup; until they have drivers of their own (AI) the gearbox and the
        // clutch work by themselves
        let mut other = setup.clone();
        other.car = opponent.car.clone();
        other.seed = setup.seed.wrapping_add(1000 * slot as u32);
        other.auto_clutch = true;
        other.auto_shifter = true;
        other.auto_blip = None;
        // (a car of race.ini has the ballast and the restrictor of its own section)
        other.session.ballast_kg = opponent.place.map_or(0.0, |place| place.ballast_kg);
        other.session.restrictor = opponent.place.map_or(0.0, |place| place.restrictor);
        other.session.setup_file = None;
        // (assists.ini is the player's: DrivingAssistManager changes car 0 only; the others
        // keep the ABS and traction control of their own files)
        other.session.assists = None;
        if let OpponentDrive::Recorded(file) = &opponent.drive {
            // a recorded drive is replayed with the aids it was driven with
            let recorded = crate::input_file::InputFile::read(file)?.setup;
            other.auto_clutch = recorded.auto_clutch;
            other.auto_shifter = recorded.auto_shifter;
            other.auto_blip = recorded.auto_blip;
        }
        let pit_box = opponent.place.map_or(slot, |place| place.pit);
        let built = build_car(&other, &opponent_data, physics_time, opponent_driver(opponent, track)?, track, &pose, Some(&mut engine), pit_box, None)?;
        opponents.push(OpponentCar { car: built, setup: opponent.clone(), data_path: opponent_data, spawn: pose });
    }
    Ok((car, opponents, Some(engine)))
}

/// `[PHYSICS_THREADING] THREADS` of the game's own `system/cfg/assetto_corsa.ini` (-1 when
/// the game's folder or the file is not there, 0 when the section is there without the key:
/// the game's reading).
pub fn physics_threading_key() -> i32 {
    let Some(file) = ac_root().map(|root| root.join("system").join("cfg").join("assetto_corsa.ini")) else {
        return -1;
    };
    if !file.is_file() {
        return -1;
    }
    match rustyac_physics::data::ini::IniReader::load(&file) {
        Ok(ini) if ini.has_section("PHYSICS_THREADING") => ini.get_int("PHYSICS_THREADING", "THREADS").unwrap_or(0),
        _ => -1,
    }
}

impl GameSim {
    pub fn new(setup: SimSetup, source: Box<dyn DriverSource>) -> Result<GameSim, String> {
        GameSim::build(setup, source, None)
    }

    /// A session on a server: `setup` as the server's handshake has it (its car, its track,
    /// the pit box of the player's slot), `online` the client after the car list.
    pub fn new_online(setup: SimSetup, source: Box<dyn DriverSource>, online: OnlineSession) -> Result<GameSim, String> {
        GameSim::build(setup, source, Some(online))
    }

    fn build(setup: SimSetup, source: Box<dyn DriverSource>, online: Option<OnlineSession>) -> Result<GameSim, String> {
        let rules = online.as_ref().map(|session| ServerRules::of(&session.net.client.accepted));
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
            spawn = match &setup.player_place {
                // a session of race.ini with other cars: the slot of [CAR_0]
                Some(place) => spawn_place(&loaded, &setup.spawn, place, &setup.car)?,
                None => spawn_on(&loaded, &setup.spawn)?,
            };
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
        let (car, opponents, mut engine) = build_world(&setup, &data_path, setup.clock_start_ms, driver, track.as_ref(), &spawn, rules.as_ref())?;
        let lap_db = LapDb::new(track.as_ref().map(|t| t.sectors_normalized_positions.len()).unwrap_or(0));
        let physics_info = PhysicsInfo::of(&car);
        let mut online = online.map(Box::new);
        if let (Some(session), Some(engine)) = (&mut online, &mut engine) {
            session.net.track = track.clone();
            add_remote_cars(session, engine, track.as_ref(), 1 + opponents.len())?;
            session.net.client.set_cars_in_session(1 + session.remote.len());
            // the session has run for a while already: its start on this machine's clock
            let started = setup.clock_start_ms - session.net.client.accepted.current_session.start as f64;
            session.net.client.begin_update_mode(started)?;
        }
        let mut sim = GameSim { setup, data_path, car, physics_info, steps: 0, track, track_folder, spawn, track_summary, lap_db, session_index: 0, new_sessions: 0, step_laps: Vec::new(), fuel_evaluator: rustyac_physics::car::fuel_lap_evaluator::FuelLapEvaluator::new(), aid_note: AidNote::default(), opponents, engine, online, rules, solo_ai: rustyac_physics::ai::SoloAi { rand: rustyac_physics::session::MsvcRand(1), session: rustyac_physics::ai::SessionInfo::default() }, referee: None, setup_jobs: Vec::new(), setup_schedule: std::collections::VecDeque::new(), setup_log: Vec::new() };
        sim.setup_schedule = sim.setup.setup_jobs.iter().cloned().collect();
        // the session begins in the ESC menu: without a referee the car is held here
        if sim.setup.esc_start && sim.setup.referee.is_none() {
            sim.car.car.lock_controls(true);
        }
        sim.start_ai()?;
        sim.start_referee();
        Ok(sim)
    }

    /// The world the track's loose objects and every contact live in: the engine's with
    /// several cars, else the car's own.
    pub fn world_core(&self) -> &rustyac_physics::car::PhysicsCore {
        match &self.engine {
            Some(engine) => &engine.core,
            None => &self.car.car.core,
        }
    }

    /// The track's grip of the session (`Track::dynamicTrack`), when race.ini has the section:
    /// the engine's with several cars, else the car's own.
    pub fn dynamic_track(&self) -> Option<rustyac_physics::track::DynamicTrack> {
        match &self.engine {
            Some(engine) => engine.dynamic_track,
            None => self.car.car.dynamic_track,
        }
    }

    /// All cars of the session in creation order: the driver's first.
    pub fn cars(&self) -> impl Iterator<Item = &rustyac_physics::car::RollingChassis> {
        std::iter::once(&self.car.car).chain(self.opponents.iter().map(|o| &o.car.car))
    }

    /// FNV-1a over the rigid-body state of every car (for checks that two runs are the same).
    pub fn state_hash(&self, mut hash: u64) -> u64 {
        for car in self.cars() {
            for body in car.own_bodies() {
                let b = car.core.world.body(body);
                for word in b.pos[..3].iter().chain(&b.q).chain(&b.lvel[..3]).chain(&b.avel[..3]) {
                    for byte in word.to_bits().to_le_bytes() {
                        hash ^= byte as u64;
                        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
                    }
                }
            }
        }
        hash
    }

    /// The physics clock after the last step, ms.
    pub fn clock_ms(&self) -> f64 {
        if self.steps == 0 {
            self.setup.clock_start_ms
        } else {
            self.setup.time_of_step(self.steps - 1)
        }
    }

    /// The car's folder name, as the setups' folders have it (`CarAvatar::unixName`): the
    /// folder above `data` when the car was named by its data folder.
    pub fn setup_car_name(&self) -> String {
        if !self.car.car.unix_name.is_empty() {
            return self.car.car.unix_name.clone();
        }
        car_name(&self.data_path)
    }

    /// What the display needs to know of the car once: where its data is and where its
    /// wheels sit.
    pub fn car_info(&self) -> CarInfo {
        let info =
            CarInfo { setup: None, data_path: self.data_path.clone(), name: self.setup.car.clone(), render_seed: self.setup.render_seed, track_folder: self.track_folder.clone(), track_layout: self.track.as_ref().map(|t| t.config.clone()).unwrap_or_default(), cars: 1 + self.opponents.len() + self.remote_count(), skin: self.setup.player_skin.clone(), ..CarInfo::default() };
        let mut info = Self::with_wheels(info, &self.car.car);
        info.setup = Some(Box::new(rustyac_gui::setup_car::SetupSnapshot::of(&self.car.car, &self.setup_car_name())));
        info
    }

    /// The same of the session's other cars, in creation order.
    pub fn opponent_infos(&self) -> Vec<CarInfo> {
        let cars = 1 + self.opponents.len() + self.remote_count();
        self.opponents
            .iter()
            .map(|other| {
                let info = CarInfo { setup: None, data_path: other.data_path.clone(), name: other.setup.car.clone(), skin: (!other.setup.skin.is_empty()).then(|| other.setup.skin.clone()), render_seed: self.setup.render_seed, track_folder: self.track_folder.clone(), track_layout: self.track.as_ref().map(|t| t.config.clone()).unwrap_or_default(), cars, ..CarInfo::default() };
                Self::with_wheels(info, &other.car.car)
            })
            .chain(self.remote_infos())
            .collect()
    }

    /// How many cars of other players the session has (their slots, taken or not).
    pub fn remote_count(&self) -> usize {
        self.online.as_ref().map_or(0, |session| session.remote.len())
    }

    /// The unchanging facts of the other players' cars, in their order.
    pub fn remote_infos(&self) -> Vec<CarInfo> {
        let Some(session) = &self.online else {
            return Vec::new();
        };
        let cars = 1 + self.opponents.len() + session.remote.len();
        session
            .remote
            .iter()
            .enumerate()
            .map(|(number, remote)| {
                let geometry = &session.net.cars[number].geometry;
                let mut info = CarInfo {
                    data_path: remote.data_path.clone(),
                    name: remote.entry.model.clone(),
                    skin: (!remote.entry.skin.is_empty()).then(|| remote.entry.skin.clone()),
                    render_seed: self.setup.render_seed,
                    track_folder: self.track_folder.clone(),
                    track_layout: self.track.as_ref().map(|t| t.config.clone()).unwrap_or_default(),
                    cars,
                    ..CarInfo::default()
                };
                for wheel in 0..4 {
                    let p = geometry.wheel_base_pos_ls[wheel].m[3];
                    info.wheel_positions[wheel] = [p[0], p[1], p[2]];
                    info.tyre_width[wheel] = 0.2;
                    info.tyre_radius[wheel] = geometry.tyre_radius[wheel];
                }
                info
            })
            .collect()
    }

    fn with_wheels(mut info: CarInfo, car: &rustyac_physics::car::RollingChassis) -> CarInfo {
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
            if let Some(mut track) = self.dynamic_track() {
                self.session_index += 1;
                let mut rand = rustyac_physics::session::MsvcRand(self.setup.seed.wrapping_add(self.session_index as u32));
                track.on_new_session(self.session_index, rand.next());
                track.step(0);
                self.setup.env.dynamic_grip_level = track.dynamic_grip_level;
                self.setup.session.dynamic_track = Some(track);
            }
        }
        let placeholder = Driver::new(Box::new(ReplaySource::default()), self.setup.ff_gain);
        // (with other cars: all of them anew, in a new engine; their drivers start again)
        let (mut car, opponents, mut engine) = build_world(&self.setup, &self.data_path, self.clock_ms(), placeholder, self.track.as_ref(), &self.spawn, self.rules.as_ref())?;
        if let (Some(session), Some(engine)) = (&mut self.online, &mut engine) {
            // the other players' cars get bodies in the new world; what is known of them stays
            let root = ac_root();
            for (remote, car) in session.remote.iter().zip(&mut session.net.cars) {
                let mesh = remote_collider(&remote.data_path, root.as_deref(), &remote.entry.model);
                car.body = NetCarStateProvider::create_body(&mut engine.core, car.guid, &remote.entry.model, mesh.as_ref());
            }
        }
        self.opponents = opponents;
        self.engine = engine;
        std::mem::swap(&mut car.device, &mut self.car.device);
        // the engine's step count goes on (lap times carry its remainder of three)
        car.car.step_counter = self.car.car.step_counter;
        if let Some(engine) = &mut self.engine {
            engine.step_counter = self.car.car.step_counter;
        }
        self.physics_info = PhysicsInfo::of(&car);
        self.car = car;
        self.lap_db = LapDb::new(self.lap_db.sector_count);
        self.new_sessions += 1;
        // (a new Car: a new evaluator)
        self.fuel_evaluator = rustyac_physics::car::fuel_lap_evaluator::FuelLapEvaluator::new();
        // (new cars: new AI drivers; a driver the player's car had goes with the old car)
        self.car.device.ai = None;
        self.start_ai()?;
        // (and a new referee: the session list from its start)
        self.referee = None;
        self.start_referee();
        Ok(())
    }

    /// Task 28: the referee of the session list, when the set-up has one: the manager with
    /// its first session, as `RaceManager::initOffline` leaves it, and `Sim::Sim`'s
    /// `startRace`.
    fn start_referee(&mut self) {
        let (Some(header), Some(track)) = (self.setup.referee.clone(), self.track.clone()) else {
            return;
        };
        if self.online.is_some() || self.setup.oracle.is_some() {
            return;
        }
        let cars = 1 + self.opponents.len();
        let places: Vec<Option<crate::input_file::GridPlace>> = std::iter::once(self.setup.player_place).chain(self.opponents.iter().map(|opponent| opponent.setup.place)).collect();
        let skins: Vec<String> = std::iter::once(self.setup.player_skin.clone().unwrap_or_default()).chain(self.opponents.iter().map(|opponent| opponent.setup.skin.clone())).collect();
        let players = (0..cars)
            .map(|index| rustyac_physics::race::output::Player {
                name: header.driver_names.get(index).filter(|name| !name.is_empty()).cloned().unwrap_or_else(|| if index == 0 { "Player".to_string() } else { format!("Driver {index}") }),
                car: if index == 0 { self.car.car.unix_name.clone() } else { self.opponents[index - 1].car.car.unix_name.clone() },
                skin: skins[index].clone(),
            })
            .collect();
        let setup = rustyac_physics::race::RefereeSetup {
            sessions: header.sessions.clone(),
            over_time_mult: header.over_time_mult,
            start_index: places.iter().enumerate().map(|(index, place)| place.map_or(index as i32, |place| place.start)).collect(),
            pit_index: places.iter().enumerate().map(|(index, place)| place.map_or(index as i32, |place| place.pit as i32)).collect(),
            players,
            seed: header.seed,
            first_start_delay_ms: header.first_start_delay_ms,
            next_start_delay_ms: header.next_start_delay_ms,
            yellow_flag_distance: None,
            layout: self.setup.layout.clone(),
        };
        let now = self.clock_ms();
        // Task 28b: the pit menu of the player's car, with the presets of its setup
        let mut menu = None;
        if let Some(pit) = &header.pit {
            match rustyac_physics::race::QuickMenu::for_car(&self.car.car, &pit.settings) {
                Ok(mut made) => {
                    if let Some(text) = &pit.presets_ini {
                        made.load_presets(&rustyac_physics::data::ini::IniReader::from_text(std::path::Path::new("presets.sp"), text));
                    }
                    if let Some(values) = &pit.values {
                        made.set_values_text(values);
                    }
                    menu = Some((made, pit.window));
                }
                Err(message) => eprintln!("no pit stop: {message}"),
            }
        }
        let mut world = crate::referee::SimWorld { car: &mut self.car, opponents: &mut self.opponents, engine: &mut self.engine, solo_ai: &mut self.solo_ai, setup: &mut self.setup, session_index: &mut self.session_index };
        let mut referee = rustyac_physics::race::Referee::new(setup, track, &mut world, now);
        if let Some((menu, window)) = menu {
            referee.enable_pit_stop(menu, &world);
            referee.pit_window = window;
        }
        self.referee = Some(Box::new(referee));
    }

    /// Task 28b: the player's pit stop for the picture: `PitStop::state` and the marker's
    /// visibility (`None`: no pit stop in this session).
    pub fn pit_stop_view(&self) -> Option<crate::view::PitStopView> {
        let pit = match (&self.referee, &self.online) {
            (Some(referee), _) => referee.pit.as_ref(),
            (None, Some(session)) => session.pit.as_ref().map(|pit| &pit.pit),
            (None, None) => None,
        };
        pit.map(|pit| crate::view::PitStopView {
            state: pit.state,
            visible: pit.indicator_visible,
            pit_timer: pit.pit_timer,
            max_pit_timer: pit.max_pit_timer,
            tyre_time: pit.tyre_time,
            commits: pit.commits,
            overs: pit.overs,
            tyre_change_time: pit.last_start.tyre_change_time,
            total_time: pit.last_start.total_time,
            force_change_camera: pit.last_start.force_change_camera,
            changing_tyres: pit.last_changing_tyres,
        })
    }

    /// Task 28b: the mandatory pit window (0, 0: none) and whether the player's stop was counted.
    pub fn pit_window(&self) -> (u16, u16) {
        match (&self.referee, &self.online) {
            (Some(referee), _) => referee.pit_window,
            (None, Some(session)) => (session.net.client.accepted.pit_window_start, session.net.client.accepted.pit_window_end),
            (None, None) => (0, 0),
        }
    }

    pub fn mandatory_pit_done(&self) -> bool {
        match (&self.referee, &self.online) {
            (Some(referee), _) => referee.pit_avatar.mandatory_pitstop_done,
            (None, Some(session)) => session.pit.as_ref().is_some_and(|pit| pit.avatar.mandatory_pitstop_done),
            (None, None) => false,
        }
    }

    /// Task 28: what the referee says, for the display.
    pub fn referee_hud(&self) -> Option<crate::referee::RefereeHud> {
        self.referee.as_ref().map(|referee| {
            let mut hud = crate::referee::hud_of(referee, &self.car.car);
            hud.new_sessions = (self.new_sessions << 16) | (referee.new_session_count & 0xffff);
            hud
        })
    }

    /// Task 28: the end of the program (`RaceManager::shutdown`, `Sim::writeOutputJson`,
    /// `RaceTimingServices::shutdown`): the last session's results are made, and the bytes
    /// of `race_out.json` and `laps.ini` are returned with the results as a table.
    pub fn finish_referee(&mut self) -> Option<(Vec<u8>, Vec<u8>, Vec<String>)> {
        let mut referee = self.referee.take()?;
        let mut world = crate::referee::SimWorld { car: &mut self.car, opponents: &mut self.opponents, engine: &mut self.engine, solo_ai: &mut self.solo_ai, setup: &mut self.setup, session_index: &mut self.session_index };
        let (json, laps) = referee.shutdown(&mut world);
        let table = referee.last_results.as_ref().map(|event| crate::referee::results_table(&referee, event)).unwrap_or_default();
        self.referee = Some(referee);
        Some((json, laps, table))
    }

    /// The session as the AI drivers are told it (`PhysicsEngine::sessionInfo`).
    fn ai_session(&self) -> rustyac_physics::ai::SessionInfo {
        let env = &self.car.car.env;
        rustyac_physics::ai::SessionInfo { session_type: env.session_type, start_time_ms: env.session_start_time_ms, time_secs: self.setup.ai.time_secs, laps: self.setup.ai.laps, index: self.session_index }
    }

    /// Where AC looks for an AI setup of a car on the track (`AIDriver::initSetup`
    /// @ 0x14029e250): the user's `setups/<car>/<track>/ai_default.ini`, else the game's
    /// `system/setups/<car>/<track>/ai_default.ini`. Both are only read.
    fn ai_setup_file(&self, car: &rustyac_physics::car::RollingChassis) -> Option<PathBuf> {
        let track = self.track.as_ref()?;
        let relative = Path::new(&car.unix_name).join(&track.name).join("ai_default.ini");
        #[cfg(windows)]
        let own = crate::conditions::documents_folder().map(|folder| folder.join("setups").join(&relative)).filter(|file| file.is_file());
        #[cfg(not(windows))]
        let own: Option<PathBuf> = None;
        own.or_else(|| ac_root().map(|root| root.join("system").join("setups").join(&relative)).filter(|file| file.is_file()))
    }

    /// Task 27: makes the session's AI drivers as the game does once all cars are there, in
    /// the cars' order: `new AIDriver(car)` each (the player's when `INPUT_METHOD=AI`), then
    /// the session, which every driver is told, and every car's place on the grid or in its
    /// pit box (the game's teleport at a session's start, which a driver hears of too).
    fn start_ai(&mut self) -> Result<(), String> {
        // (new for every session of a run, and written into recordings with the seed)
        let seed = rustyac_physics::session::MsvcRand(self.setup.seed.wrapping_add(27_000).wrapping_add(self.setup.ai.seed).wrapping_add(self.new_sessions));
        self.solo_ai = rustyac_physics::ai::SoloAi { rand: seed, session: rustyac_physics::ai::SessionInfo { session_type: rustyac_physics::ai::SESSION_UNDEFINED, start_time_ms: 0.0, time_secs: 0.0, laps: 0, index: 0 } };
        if let Some(engine) = &mut self.engine {
            engine.ai_rand = seed;
        }
        let wanted: Vec<Option<(f32, f32)>> = std::iter::once(self.setup.ai.player.then_some((1.0, 0.0)))
            .chain(self.opponents.iter().map(|opponent| match opponent.setup.drive {
                OpponentDrive::Ai { push, aggression } => Some((push, aggression)),
                _ => None,
            }))
            .collect();
        if wanted.iter().all(Option::is_none) {
            return Ok(());
        }
        if self.track.is_none() {
            return Err("an AI driver needs a track (--track)".to_string());
        }
        for (index, wanted) in wanted.iter().enumerate() {
            let Some((push, aggression)) = *wanted else { continue };
            let setup_file = {
                let car = if index == 0 { &self.car.car } else { &self.opponents[index - 1].car.car };
                self.ai_setup_file(car)
            };
            let mut rand = match &self.engine {
                Some(engine) => engine.ai_rand,
                None => self.solo_ai.rand,
            };
            let session = self.solo_ai.session;
            let slot = if index == 0 { &mut self.car } else { &mut self.opponents[index - 1].car };
            slot.car.forced_position_events = 0;
            let mut driver = make_ai_driver(&mut slot.car, &session, &mut rand, setup_file.as_deref())?;
            driver.base_push = push;
            driver.aggression = aggression;
            slot.device.ai = Some(Box::new(driver));
            match &mut self.engine {
                Some(engine) => engine.ai_rand = rand,
                None => self.solo_ai.rand = rand,
            }
        }
        // the session, and the cars on their places
        let info = self.ai_session();
        match &mut self.engine {
            Some(engine) => {
                let mut slots = Vec::with_capacity(1 + self.opponents.len());
                slots.push(CarSlot { car: &mut self.car.car, device: DeviceRef::Here(&mut self.car.device) });
                for opponent in &mut self.opponents {
                    slots.push(CarSlot { car: &mut opponent.car.car, device: DeviceRef::Anywhere(&mut opponent.car.device) });
                }
                engine.set_session(info, &mut slots);
                for index in 0..slots.len() {
                    slots[index].car.forced_position_events = wanted[index].is_some() as u32;
                    engine.deliver_forced_positions(&mut slots);
                }
            }
            None => {
                self.solo_ai.session = info;
                self.solo_ai.give(&mut self.car.car);
                self.car.device.on_new_session(&mut self.car.car, &info);
                self.car.car.forced_position_events = 1;
                self.car.device.on_forced_position(&mut self.car.car);
                self.car.car.forced_position_events = 0;
                self.solo_ai.take(&mut self.car.car);
            }
        }
        Ok(())
    }

    /// Task 27: AC's "Activate AI" (`Sim::onKeyDown` -> the closure @ 0x140195b40): a new
    /// `AIDriver` for the player's car as it is now, with the constructor's level (100 %)
    /// and no aggression. Nothing happens when the car has one already.
    fn hand_over(&mut self) -> Result<(), String> {
        if self.car.device.ai.is_some() {
            return Ok(());
        }
        if self.track.is_none() {
            return Err("an AI driver needs a track".to_string());
        }
        let setup_file = self.ai_setup_file(&self.car.car);
        let info = self.ai_session();
        let (mut rand, session) = match &self.engine {
            Some(engine) => (engine.ai_rand, engine.session),
            None => {
                self.solo_ai.session = info;
                (self.solo_ai.rand, info)
            }
        };
        let events = self.car.car.forced_position_events;
        let driver = make_ai_driver(&mut self.car.car, &session, &mut rand, setup_file.as_deref())?;
        self.car.car.forced_position_events = events;
        self.car.device.ai = Some(Box::new(driver));
        match &mut self.engine {
            Some(engine) => engine.ai_rand = rand,
            None => self.solo_ai.rand = rand,
        }
        self.aid_note = AidNote::new("AI DRIVER: ON", self.steps);
        Ok(())
    }

    fn apply(&mut self, events: u32, bias_clicks: i32) -> Result<(), String> {
        if events & event::REBUILD != 0 {
            self.rebuild()?;
        }
        if events & event::AI_ON != 0 {
            self.hand_over()?;
        }
        // Task 28b: the AI driver of the player's car is called in
        if events & event::AI_PIT != 0 {
            if let Some(driver) = &mut self.car.device.ai {
                driver.request_pit_stop = true;
            }
        }
        // a new session's jobs in the order the game queues them: the track's handler (the
        // loose objects go home) was registered before the car's
        if events & (event::OBJECTS_HOME | event::RESET) != 0 {
            match &mut self.engine {
                // (the job runs at the start of the step; no car reads the objects before then)
                Some(engine) => engine.core.reset_track_objects(),
                None if !self.car.car.core.track_objects.is_empty() => self.car.car.queue(|car| car.core.reset_track_objects()),
                None => {}
            }
        }
        if events & event::RESET != 0 {
            // CarAvatar::goToSpawnPosition 0x1400d3ae0: the lap under way does not count for the fuel
            self.fuel_evaluator.ignore_lap = true;
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
        // PhysicsAvatar::stepCommandQueue: the setup editor's jobs
        while self.setup_schedule.front().is_some_and(|(step, _)| *step <= self.steps) {
            if let Some((_, job)) = self.setup_schedule.pop_front() {
                self.setup_jobs.push(job);
            }
        }
        for job in std::mem::take(&mut self.setup_jobs) {
            if let SetupJob::SetFfUpgrade { which, value } = job {
                self.car.device.source.set_ff_upgrade(which, value);
            }
            // (the job's own line, `_Do_call` @ 0x1400d05a0)
            if let SetupJob::SetBallastKg { kg } = job {
                println!("Setting ballast for car 0 to {:.6} Kg", kg as f64);
            }
            self.car.car.apply_setup_job(&job);
            // (the lap a refuel falls in is no measure of the fuel a lap takes: the HUD's own
            // evaluator hears it too)
            if matches!(job, SetupJob::SetRequestedFuel { .. }) {
                self.fuel_evaluator.ignore_lap = true;
            }
            self.setup_log.push((self.steps, job));
        }
        let (mut events, bias_clicks) = self.car.device.source.take_events();
        if self.setup.ai.hand_over_step == self.steps as i64 {
            events |= event::AI_ON;
        }
        self.apply(events, bias_clicks)?;
        if self.car.device.source.wants_probe() {
            let probe = CarProbe::capture(&mut self.car.car);
            self.car.device.source.set_probe(&probe);
        }
        self.car.device.polled = false;
        // Task 28: the engine's jobs the referee posted (the new session, the start time)
        if let Some(mut referee) = self.referee.take() {
            let mut world = crate::referee::SimWorld { car: &mut self.car, opponents: &mut self.opponents, engine: &mut self.engine, solo_ai: &mut self.solo_ai, setup: &mut self.setup, session_index: &mut self.session_index };
            referee.before_step(&mut world);
            self.referee = Some(referee);
        }
        let time = self.setup.time_of_step(self.steps);
        match &mut self.engine {
            None if self.car.device.ai.is_some() => {
                // one car with an AI driver: what the engine hands such a car
                self.solo_ai.give(&mut self.car.car);
                self.car.step(DT, time);
                if self.car.car.forced_position_events != 0 {
                    self.car.device.on_forced_position(&mut self.car.car);
                    self.car.car.forced_position_events = 0;
                }
                self.solo_ai.take(&mut self.car.car);
            }
            None => self.car.step(DT, time),
            Some(engine) => {
                for opponent in &mut self.opponents {
                    let _ = opponent.car.device.source.take_events();
                    if opponent.car.device.source.wants_probe() {
                        let probe = CarProbe::capture(&mut opponent.car.car);
                        opponent.car.device.source.set_probe(&probe);
                    }
                    opponent.car.device.polled = false;
                }
                // the driver's device stays on this thread (a real wheel is polled here)
                let mut slots = Vec::with_capacity(1 + self.opponents.len());
                slots.push(CarSlot { car: &mut self.car.car, device: DeviceRef::Here(&mut self.car.device) });
                for opponent in &mut self.opponents {
                    slots.push(CarSlot { car: &mut opponent.car.car, device: DeviceRef::Anywhere(&mut opponent.car.device) });
                }
                match &mut self.online {
                    Some(session) => {
                        engine.step_with(DT, time, &mut slots, &mut OnlineHooks { session });
                        // the other players' wakes, for the next step's slipstream
                        engine.extra_wakes.clear();
                        // (PhysicsEngine::slipStreams lists every remote car's, there or not)
                        engine.extra_wakes.extend(session.net.cars.iter().map(|car| car.slip_stream));
                    }
                    None => engine.step(DT, time, &mut slots),
                }
                // the other cars' laps: the referee's lists (without one their timers' events
                // are dropped)
                for (index, opponent) in self.opponents.iter_mut().enumerate() {
                    let (laps, splits) = opponent.car.car.transponder.take_events();
                    if let Some(referee) = &mut self.referee {
                        referee.events(index + 1, &laps, &splits);
                    }
                }
            }
        }
        self.steps += 1;
        // FuelLapEvaluator::step (the end of Car::stepComponents)
        let (speed, fuel, in_pit_lane) = (self.car.car.speed, self.car.car.fuel, self.car.car.is_in_pit_lane());
        self.fuel_evaluator.step(DT, speed, fuel, in_pit_lane);
        // Car::evOnSectorSplit / evOnLapCompleted -> the lap list
        self.step_laps.clear();
        if self.track.is_some() {
            let (laps, splits) = self.car.car.transponder.take_events();
            self.step_laps.extend(laps.iter().cloned());
            if let Some(session) = &mut self.online {
                session.report_local_car(&self.car.car, &laps, &splits, time);
            }
            if let Some(referee) = &mut self.referee {
                referee.events(0, &laps, &splits);
            }
            for split in &splits {
                self.lap_db.on_sector_split(split);
            }
            for lap in &laps {
                self.lap_db.on_lap_completed(lap);
                self.fuel_evaluator.on_lap_completed(fuel, in_pit_lane);
            }
        }
        if self.online.is_some() {
            self.apply_server_events();
        }
        // Task 28b: the player's pit stop on a server (the menu's keys, then its frame)
        if let Some(session) = &mut self.online {
            if session.pit.is_none() && !session.pit_failed {
                let window = (session.net.client.accepted.pit_window_start, session.net.client.accepted.pit_window_end);
                match crate::online::OnlinePit::new(&self.car.car, &crate::online::PIT_SETTINGS, window) {
                    Ok(pit) => {
                        if window.1 != 0 {
                            session.console.push(format!("online: the server has a mandatory pit window: opens at {}, closes at {} (the leader's laps; minutes in a timed race)", window.0, window.1));
                        }
                        session.pit = Some(pit);
                    }
                    Err(message) => {
                        session.console.push(format!("online: no pit stop: {message}"));
                        session.pit_failed = true;
                    }
                }
            }
            if let Some(pit) = &mut session.pit {
                for (bit, key) in [(event::PIT_LEFT, 0x25), (event::PIT_UP, 0x26), (event::PIT_RIGHT, 0x27), (event::PIT_DOWN, 0x28)] {
                    if events & bit != 0 {
                        pit.menu.send_input(key, &rustyac_physics::race::QuickMenuCar::of(&self.car.car));
                    }
                }
            }
            session.pit_frame(&mut self.car.car, time);
        }
        // Task 30: "Back to pits" without a referee is the plain way back to the spawn point
        if events & event::BACK_TO_PITS != 0 && self.referee.is_none() {
            self.car.device.source.request(event::RESET);
        }
        // Task 30: the ESC menu's lock of the controls, and its "Drive", without a referee
        if self.referee.is_none() {
            if events & event::PITS_LOCK != 0 {
                self.car.car.lock_controls(true);
            }
            if events & event::PITS_DRIVE != 0 {
                self.car.car.lock_controls(false);
            }
        }
        // Task 28: the referee's frame (the game's main thread), every third step
        if let Some(mut referee) = self.referee.take() {
            let mut world = crate::referee::SimWorld { car: &mut self.car, opponents: &mut self.opponents, engine: &mut self.engine, solo_ai: &mut self.solo_ai, setup: &mut self.setup, session_index: &mut self.session_index };
            if events & event::NEXT_SESSION != 0 {
                referee.skip_session(&mut world);
            }
            if events & event::RESTART_SESSION != 0 {
                referee.restart_session(&mut world);
            }
            if events & event::BACK_TO_PITS != 0 {
                referee.back_to_pits(&mut world, time);
            }
            if events & event::PITS_LOCK != 0 {
                referee.pits_lock(&mut world);
            }
            if events & event::PITS_DRIVE != 0 {
                referee.drive(&mut world);
            }
            // Task 28b: the pit menu's keys
            for (bit, key) in [(event::PIT_LEFT, 0x25), (event::PIT_UP, 0x26), (event::PIT_RIGHT, 0x27), (event::PIT_DOWN, 0x28)] {
                if events & bit != 0 {
                    referee.pit_key(key, rustyac_physics::race::World::car(&world, 0));
                }
            }
            let sessions_before = referee.manager.current_session_index;
            referee.after_step(&mut world, time);
            if referee.manager.current_session_index != sessions_before {
                // the picture's handlers of the game's new-session event
                self.new_sessions += 1;
                self.lap_db = LapDb::new(self.lap_db.sector_count);
            }
            self.referee = Some(referee);
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

    /// What the server said that changes the physics: the weather, the track's grip, a new
    /// session, ballast and restrictor, the race's start.
    fn apply_server_events(&mut self) {
        use rustyac_net::client::Event;
        use rustyac_net::messages::Message;
        let Some(session) = &mut self.online else {
            return;
        };
        let me = session.net.client.session_id;
        for event in std::mem::take(&mut session.pending) {
            match event {
                Event::Message(Message::Weather { ambient, road, wind_speed_kmh, wind_direction_deg, .. }) => {
                    // ACP_WEATHER_UPDATE: the temperatures as whole degrees; the wind at most 40 km/h
                    let kmh = (wind_speed_kmh as f32).clamp(0.0, 40.0);
                    let speed = rustyac_physics::session::wind_from_kmh(kmh);
                    for env in self.engine.iter_mut().map(|engine| &mut engine.env).chain([&mut self.car.car.env, &mut self.setup.env]) {
                        env.ambient_temperature = ambient as f32;
                        env.road_temperature = road as f32;
                    }
                    if let Some(engine) = &mut self.engine {
                        engine.set_wind(speed, wind_direction_deg as f32);
                    }
                    self.setup.session.wind_speed = speed;
                    self.setup.session.wind_direction_deg = wind_direction_deg as f32;
                }
                // Track::setGripLevelExternal: every lap message carries the grip
                Event::Message(Message::LapCompleted { grip, .. }) => self.set_server_grip(grip),
                Event::Message(Message::NewSession(info)) => {
                    // the job ACClient::onNewSession queues for the physics thread, and
                    // PhysicsEngine::setSessionInfo
                    if let Some(rules) = &mut self.rules {
                        rules.session_type = info.kind as i32;
                        rules.grip = info.grip;
                        let (kind, mode) = (rules.session_type, rules.penalty_mode());
                        for env in self.engine.iter_mut().map(|engine| &mut engine.env).chain([&mut self.car.car.env]) {
                            env.session_type = kind;
                            env.penalty_mode = mode;
                            env.session_start_time_ms = info.start as f64;
                        }
                    }
                    self.set_server_grip(info.grip);
                    if let Some(engine) = &mut self.engine {
                        engine.reset_collisions_for_new_session();
                    }
                    // the grid of a race: the slot the server gives this car
                    if info.kind == rustyac_net::handshake::session_type::RACE {
                        if let (Some(place), Some(track)) = (info.grid.iter().position(|slot| *slot == me), &self.track) {
                            if let Some((position, tail)) = track.spawn_pose("START", place) {
                                self.spawn = SpawnPose { position, tail };
                            }
                        }
                    }
                    self.new_sessions += 1;
                    self.car.device.source.request(event::RESET);
                }
                Event::Message(Message::Bop(entries)) => {
                    for (slot, ballast, restrictor) in entries {
                        // (kilograms and a restrictor that are numbers, as race.ini's)
                        if slot == me && ballast.is_finite() && restrictor.is_finite() && ballast >= 0.0 && restrictor >= 0.0 {
                            // Car::setBallastKG / setRestrictor, when the value is another
                            if self.car.car.ballast_kg != ballast {
                                self.car.car.ballast_kg = ballast;
                            }
                            self.car.car.set_restrictor(restrictor);
                        }
                    }
                }
                Event::RaceStart(start) => {
                    // PhysicsEngine::setCurrentSessionStartTime
                    for env in self.engine.iter_mut().map(|engine| &mut engine.env).chain([&mut self.car.car.env]) {
                        env.session_start_time_ms = start;
                    }
                }
                _ => {}
            }
        }
    }

    fn set_server_grip(&mut self, grip: f32) {
        // (a grip that is no number, or none at all, is not taken)
        if !(grip.is_finite() && grip > 0.0) {
            return;
        }
        for env in self.engine.iter_mut().map(|engine| &mut engine.env).chain([&mut self.car.car.env, &mut self.setup.env]) {
            env.dynamic_grip_level = grip;
        }
    }

    /// One step of a recorded drive.
    pub fn step_recorded(&mut self, step: &StepInput) -> Result<(), String> {
        self.car.device.source.load(step);
        self.step().map(|_| ())
    }
}
