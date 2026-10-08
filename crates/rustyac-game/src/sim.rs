// SPDX-License-Identifier: GPL-3.0-or-later

//! The simulation the game runs: one [`VanillaCar`], built and spawned the way the oracle
//! builds and spawns the game's car, and stepped with nothing but what a driver's device
//! reports. Live driving, `--replay` and the tests all go through [`GameSim::step`]; there is
//! no other way the game touches the car.

use std::path::{Path, PathBuf};
use std::sync::Arc;

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
    /// Shift into first gear at the end of the sequence.
    pub first_gear: bool,
    /// The last `acquire` was a step of the sequence.
    last_was_sequence: bool,
}

impl<S: DriverSource> SpawnSequence<S> {
    pub fn new(inner: S, first_gear: bool) -> SpawnSequence<S> {
        SpawnSequence { inner, at: 0, requested: 0, first_gear, last_was_sequence: false }
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
        (events, bias_clicks)
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

/// Finds a car's data folder: a path to it, or its name under a `cardata` folder next to the
/// working directory or above the program.
pub fn find_car_data(car: &str) -> Result<PathBuf, String> {
    let mut tried = Vec::new();
    let direct = PathBuf::from(car);
    if direct.join("car.ini").is_file() {
        return Ok(direct);
    }
    tried.push(direct);
    let mut roots = vec![PathBuf::from(".")];
    if let Ok(exe) = std::env::current_exe() {
        roots.extend(exe.ancestors().skip(1).map(Path::to_path_buf));
    }
    roots.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.."));
    for root in roots {
        let folder = root.join("cardata").join(car);
        if folder.join("car.ini").is_file() {
            return Ok(folder);
        }
        tried.push(folder);
    }
    Err(format!(
        "the car {car:?} was not found: no car.ini in {} (extracted car data goes into cardata/<car>)",
        tried.iter().take(3).map(|p| p.display().to_string()).collect::<Vec<_>>().join(", ")
    ))
}

/// Assetto Corsa's own folder: `AC_ROOT` if that is set, else Steam's usual place. Read only.
pub fn ac_root() -> Option<PathBuf> {
    let root = match std::env::var_os("AC_ROOT") {
        Some(root) => PathBuf::from(root),
        None => PathBuf::from(r"C:\Program Files (x86)\Steam\steamapps\common\assettocorsa"),
    };
    root.join("content").is_dir().then_some(root)
}

/// Finds a track's folder: a path to it, or its name under the game's `content/tracks`.
pub fn find_track(track: &str) -> Result<PathBuf, String> {
    let direct = PathBuf::from(track);
    if direct.is_dir() {
        return Ok(direct);
    }
    if let Some(root) = ac_root() {
        let folder = root.join("content").join("tracks").join(track);
        if folder.is_dir() {
            return Ok(folder);
        }
    }
    Err(format!(
        "the track {track:?} was not found: it is neither a folder nor a name under Assetto Corsa's content/tracks (set AC_ROOT if the game is not in Steam's usual place)"
    ))
}

/// The car's 3D model in the game's folder: `content/cars/<car>/<LOD_0 of data/lods.ini>`.
pub fn find_car_model(car: &str, data_path: &Path) -> Option<PathBuf> {
    let name = Path::new(car).file_name()?.to_string_lossy().into_owned();
    let folder = ac_root()?.join("content").join("cars").join(&name);
    let lods = std::fs::read_to_string(data_path.join("lods.ini")).unwrap_or_default();
    let mut in_first = false;
    for line in lods.lines() {
        let line = line.split(';').next().unwrap_or("").trim();
        if line.starts_with('[') {
            in_first = line == "[LOD_0]";
        } else if let (true, Some(("FILE", file))) = (in_first, line.split_once('=').map(|(k, v)| (k.trim(), v.trim()))) {
            let path = folder.join(file);
            if path.is_file() {
                return Some(path);
            }
        }
    }
    // no lods.ini: the model named like the folder
    let path = folder.join(format!("{name}.kn5"));
    path.is_file().then_some(path)
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
}

/// One car on the endless flat road or on a track, and the count of its steps.
pub struct GameSim {
    pub setup: SimSetup,
    pub data_path: PathBuf,
    pub car: VanillaCar<Driver>,
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
            });
        }
        if setup.oracle.as_ref().and_then(|o| o.collide).is_some_and(|c| c.collider_mesh) {
            // the recording's car had its own collider mesh: the game's file, read as the
            // oracle read it
            let name = data_path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
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
    if track.is_some() {
        // RaceManager::setCurrentSession for a session that is not a race: leaving the track
        // costs the lap (penalty mode 1); `[RACE] PENALTIES` on: two tyres may be off
        env.penalty_mode = 1;
        env.allowed_tyres_out = 2;
        env.session_type = if setup.spawn == "hotlap" { 4 } else { 1 };
    }
    let mut car = VanillaCar::new(data_path, env, ground, setup.seed, physics_time, driver)?;
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
        // RaceManager::initOffline -> CarAvatar::armFirstLap in a hot-lap session
        if setup.spawn == "hotlap" {
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
    car.car.force_rotation(&spawn.tail);
    car.car.force_position(&spawn.position);
    car.car.session_start()?;
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
            let folder = find_track(&setup.track)?;
            let (mut loaded, report) = load_track(&folder, "")?;
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
            let folder = find_track(&oracle.folder)?;
            let (loaded, report) = load_track(&folder, "")?;
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
        Ok(GameSim { setup, data_path, car, steps: 0, track, track_folder, spawn, track_summary, lap_db })
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
            CarInfo { data_path: self.data_path.clone(), name: self.setup.car.clone(), track_folder: self.track_folder.clone(), ..CarInfo::default() };
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
        let placeholder = Driver::new(Box::new(ReplaySource::default()), self.setup.ff_gain);
        let mut car = build_car(&self.setup, &self.data_path, self.clock_ms(), placeholder, self.track.as_ref(), &self.spawn)?;
        std::mem::swap(&mut car.device, &mut self.car.device);
        // the engine's step count goes on (lap times carry its remainder of three)
        car.car.step_counter = self.car.car.step_counter;
        self.car = car;
        self.lap_db = LapDb::new(self.lap_db.sector_count);
        Ok(())
    }

    fn apply(&mut self, events: u32, bias_clicks: i32) -> Result<(), String> {
        if events & event::REBUILD != 0 {
            self.rebuild()?;
        }
        if events & event::RESET != 0 {
            // a teleport is a job of the game's main thread: it runs at the start of the step
            let spawn = self.spawn;
            let armed = self.track.is_some() && self.setup.spawn == "hotlap";
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
        if bias_clicks != 0 {
            if let Some(brakes) = &mut self.car.car.brake_system {
                brakes.set_manual_front_bias(bias_clicks);
            }
        }
        if let Some(aids) = &mut self.car.car.aids {
            let aids = aids.base_mut();
            for (bit, direction) in [(event::TC_UP, 1), (event::TC_DN, -1)] {
                if events & bit != 0 {
                    aids.traction_control.cycle_mode(direction);
                }
            }
            for (bit, direction) in [(event::ABS_UP, 1), (event::ABS_DN, -1)] {
                if events & bit != 0 {
                    aids.abs.cycle_mode(direction);
                }
            }
        }
        if events & event::AUTO_SHIFTER != 0 {
            self.car.car.auto_shifter.is_active = !self.car.car.auto_shifter.is_active;
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
