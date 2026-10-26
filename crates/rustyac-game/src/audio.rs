// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! The game's sound: what the sound reads of the car after a physics step (the audio part of
//! AC's `Car::getPhysicsState` @ 0x140270d70, which fills the `CarPhysicsState` the main thread
//! copies), handed from the physics thread to the display thread, and the display thread's
//! side: AC's sound engine with the track's and the car's sounds (`rustyac-audio`), driven
//! once per picture frame.
//!
//! The sound only reads. Nothing here writes to the car, and the physics thread's share is a
//! copy of a few numbers after the step.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;
#[cfg(not(target_arch = "wasm32"))]
use std::time::Instant;

use rustyac_audio::car::{CameraView, CarFrame, CarInfo as SoundInfo, SimView};
use rustyac_audio::engine::{AudioEngine, EngineFiles};
use rustyac_audio::fmod::{log, raw};
use rustyac_audio::sim::{frame_dt, AudioWorld, CarSound, FrameInput, PhysicsEvent};
use rustyac_audio::tape::OracleCamera;
use rustyac_audio::track::{cache_surface_sounds, Scene, TrackAudio};
use rustyac_physics::data::ini::{append_path, IniReader};
use rustyac_physics::vecmath::xm_matrix_multiply;

#[cfg(windows)]
use crate::input_file::StepInput;
#[cfg(windows)]
use crate::physics_thread::StepSink;
use crate::render::scene::{CameraMode, CarShape, DrivingCamera};
use crate::sim::GameSim;
use crate::view::Mat;

/// The sample rate and block of a WAV written without a clock: one block is one frame of 1/60 s.
pub const NRT_RATE: i32 = 48000;
pub const NRT_BLOCK: u32 = 800;
/// The frame time of the frames of a WAV written without a clock.
pub const NRT_DT: f32 = 1.0 / 60.0;

/// The surface a tyre touched last: `Car::getPhysicsState` copies a tyre's surface only while
/// the tyre has one, so the snapshot keeps the last.
#[derive(Clone, Debug, Default)]
pub struct StickySurface {
    wav: String,
    grip_mod: f32,
}

/// The sound's inputs after a step: the audio part of `Car::getPhysicsState` @ 0x140270d70
/// and `CarAvatar::isTcInAction`. `sticky` carries the tyres' last surfaces from step to step.
#[allow(clippy::needless_range_loop)] // one loop over the wheels, as the game's
pub fn capture(sim: &GameSim, sticky: &mut [StickySurface; 4]) -> CarFrame {
    capture_car(sim, &sim.car.car, sticky)
}

/// The same of every car of the session, the player's first. `sticky` grows to one set of
/// surfaces per car.
pub fn capture_all(sim: &GameSim, sticky: &mut Vec<[StickySurface; 4]>) -> Vec<CarFrame> {
    sticky.resize_with(1 + sim.opponents.len(), Default::default);
    let mut frames: Vec<CarFrame> = sim.cars().zip(sticky.iter_mut()).map(|(car, sticky)| capture_car(sim, car, sticky)).collect();
    frames.extend((0..sim.remote_count()).map(|index| capture_remote(sim, index)));
    frames
}

/// The sound's inputs of another player's car: the audio part of the `CarPhysicsState` its
/// `NetCarStateProvider` makes up. A car that is not connected is silent (`node_active`).
fn capture_remote(sim: &GameSim, index: usize) -> CarFrame {
    let flat = |m: &Mat| -> [f32; 16] { std::array::from_fn(|i| m[i / 4][i % 4]) };
    let provider = &sim.online.as_ref().expect("a remote car has a session").net.cars[index];
    let state = &provider.state;
    let world = &state.world_matrix.m;
    let mut frame = CarFrame {
        world_matrix: flat(world),
        tyre_matrix: std::array::from_fn(|i| flat(&state.tyre_matrix[i].m)),
        engine_rpm: if state.engine_rpm >= 0.0 { state.engine_rpm } else { 0.0 },
        is_engine_limiter_on: false,
        wheel_angular_speed: state.wheel_angular_speed,
        gas: state.gas,
        brake: state.brake,
        gear: state.gear,
        speed: state.speed,
        velocity: state.velocity,
        slip_ratio: state.slip_ratio,
        nd_slip: state.nd_slip,
        // (NetCarStateProvider::getPhysicsState: 100 N on every tyre, 20 l of fuel)
        load: [100.0; 4],
        tyre_dirty_level: [0.0; 4],
        surface_wav: Default::default(),
        surface_grip_mod: [0.0; 4],
        drivetrain_speed: state.drivetrain_speed,
        turbo_boost: 0.0,
        is_gear_grinding: false,
        body_work_volume: 0.0,
        air_density: sim.car.car.air_density,
        fuel: 20.0,
        engine_life_left: 1000.0,
        turbo_bov: 0.0,
        actions_state: state.actions_state,
        tyre_inflation: [1.0; 4],
        sus_damage: [0.0; 4],
        node_active: !provider.is_disconnected,
        tc_in_action: false,
        body_position: [world[3][0], world[3][1], world[3][2]],
        pit_position: [provider.pit_position.m[3][0], provider.pit_position.m[3][1], provider.pit_position.m[3][2]],
    };
    for wheel in 0..4 {
        if let Some(surface) = &state.tyre_surface_def[wheel] {
            frame.surface_grip_mod[wheel] = surface.grip_mod;
            frame.surface_wav[wheel] = match sim.track.as_ref().and_then(|track| track.surfaces.get(surface.user_pointer as usize)) {
                Some(track_surface) => track_surface.wav.clone(),
                None => String::new(),
            };
        }
    }
    frame
}

#[allow(clippy::needless_range_loop)] // one loop over the wheels, as the game's
fn capture_car(sim: &GameSim, car: &rustyac_physics::car::RollingChassis, sticky: &mut [StickySurface; 4]) -> CarFrame {
    let flat = |m: &Mat| -> [f32; 16] { std::array::from_fn(|i| m[i / 4][i % 4]) };
    let world = car.core.get_world_matrix(car.body);
    let body_velocity = car.core.get_velocity(car.body);
    let mut frame = CarFrame {
        world_matrix: flat(&world.m),
        tyre_matrix: [flat(&world.m); 4],
        engine_rpm: 0.0,
        is_engine_limiter_on: false,
        wheel_angular_speed: [0.0; 4],
        gas: car.controls.gas,
        brake: car.controls.brake,
        gear: 1,
        speed: car.speed,
        velocity: [body_velocity.x, body_velocity.y, body_velocity.z],
        slip_ratio: [0.0; 4],
        nd_slip: [0.0; 4],
        load: [0.0; 4],
        tyre_dirty_level: [0.0; 4],
        surface_wav: Default::default(),
        surface_grip_mod: [0.0; 4],
        drivetrain_speed: 0.0,
        turbo_boost: 0.0,
        is_gear_grinding: false,
        body_work_volume: 0.0,
        air_density: car.air_density,
        fuel: car.fuel as f32,
        engine_life_left: 1000.0,
        turbo_bov: 0.0,
        // the horn (action 6) has no key in rustyAC yet
        actions_state: 0,
        tyre_inflation: [0.0; 4],
        sus_damage: [0.0; 4],
        node_active: true,
        tc_in_action: false,
        body_position: [world.m[3][0], world.m[3][1], world.m[3][2]],
        pit_position: [car.pit_position.m[3][0], car.pit_position.m[3][1], car.pit_position.m[3][2]],
    };
    if let Some(drivetrain) = &car.drivetrain {
        let engine = drivetrain.engine();
        frame.turbo_bov = engine.base().bov;
        frame.engine_life_left = engine.base().life_left as f32;
        frame.drivetrain_speed = (drivetrain.base().drive.velocity as f32).abs();
        frame.turbo_boost = engine.base().status.turbo_boost;
        // negative or not a number: 0
        let rpm = drivetrain.get_engine_rpm();
        frame.engine_rpm = if rpm >= 0.0 { rpm } else { 0.0 };
        frame.is_gear_grinding = drivetrain.base().is_gear_grinding;
        frame.gear = drivetrain.base().current_gear;
        frame.is_engine_limiter_on = engine.is_limiter_on();
    }
    let asleep = car.sleeping_frames > car.frames_to_sleep;
    for i in 0..4.min(car.tyres.len()) {
        let tyre = &car.tyres[i];
        frame.sus_damage[i] = car.suspensions[i].get_damage();
        frame.tyre_inflation[i] = tyre.status.inflation;
        // the wheel's matrix: its spin times the hub matrix the tyre read in its step, at the
        // hub's place now
        let hub = car.suspensions[i].get_hub_world_matrix(&car.core);
        let mut m = xm_matrix_multiply(&tyre.local_wheel_rotation, &tyre.world_rotation);
        m.m[3][0] = hub.m[3][0];
        m.m[3][1] = hub.m[3][1];
        m.m[3][2] = hub.m[3][2];
        frame.tyre_matrix[i] = flat(&m.m);
        frame.wheel_angular_speed[i] = if !tyre.status.is_locked && !asleep { tyre.status.angular_velocity } else { 0.0 };
        frame.nd_slip[i] = tyre.status.nd_slip;
        frame.load[i] = tyre.status.load;
        frame.slip_ratio[i] = tyre.status.slip_ratio;
        if let Some(surface) = &tyre.surface_def {
            sticky[i].grip_mod = surface.grip_mod;
            sticky[i].wav = match sim.track.as_ref().and_then(|track| track.surfaces.get(surface.user_pointer as usize)) {
                Some(track_surface) => track_surface.wav.clone(),
                None => String::new(),
            };
        }
        frame.surface_wav[i] = sticky[i].wav.clone();
        frame.surface_grip_mod[i] = sticky[i].grip_mod;
        frame.tyre_dirty_level[i] = tyre.status.dirty_level;
    }
    // bodyWorkVolume (0x140271922): how fast the hubs move against the body along the body's
    // up axis, summed over the wheels
    for i in 0..4.min(car.suspensions.len()) {
        let w = car.core.get_world_matrix(car.body);
        let (upz, upy, upx) = (w.m[1][2], w.m[1][1], w.m[1][0]);
        let sv = car.suspensions[i].get_velocity(&car.core);
        let bv = car.core.get_velocity(car.body);
        let dz = bv.z - sv.z;
        let dy = bv.y - sv.y;
        let dx = bv.x - sv.x;
        let t = ((dy * upy) + (dx * upx)) + (dz * upz);
        frame.body_work_volume += t.abs();
    }
    if let Some(aids) = &car.aids {
        frame.tc_in_action = aids.base().traction_control.is_in_action;
    }
    frame
}

/// The physics thread's `ACPhysicsEvent`s for the sound: every car's (`param1` names the car
/// that was hit; `Sim::stepPhysicsEvent` hands each to that car's sound), car after car.
fn events_of(sim: &GameSim, out: &mut Vec<PhysicsEvent>) {
    for e in sim.cars().flat_map(|car| &car.physics_events) {
        out.push(PhysicsEvent {
            kind: e.kind,
            param1: e.param1,
            param2: e.param2,
            param3: e.param3,
            param4: e.param4,
            v_param1: [e.v_param1.x, e.v_param1.y, e.v_param1.z],
            v_param2: [e.v_param2.x, e.v_param2.y, e.v_param2.z],
            ul_param0: e.ul_param0 as u64,
        });
    }
}

/// One frame for the sound, cut on the grid of a clock-less WAV.
pub struct GridFrame {
    /// every car's state, the player's first
    pub state: Vec<CarFrame>,
    pub events: Vec<PhysicsEvent>,
}

/// What the physics thread leaves for the display thread.
#[derive(Default)]
pub struct Feed {
    /// The newest step's state of every car, the player's first (the game's
    /// `PhysicsAvatar::update` copies the newest finished step, unblended).
    pub state: Option<Vec<CarFrame>>,
    /// The physics events since the display thread last looked.
    pub events: Vec<PhysicsEvent>,
    /// Clock-less mode: a frame after every `floor(n * 50 / 9)` steps (60 a second of 3 ms steps).
    pub grid: std::collections::VecDeque<GridFrame>,
    grid_on: bool,
    steps: u64,
    next_frame: u64,
    pending: Vec<PhysicsEvent>,
}

impl Feed {
    /// The frames of the grid are collected (for a WAV written without a clock).
    pub fn with_grid() -> Feed {
        Feed { grid_on: true, next_frame: 1, ..Feed::default() }
    }

    /// After a physics step.
    pub fn push(&mut self, sim: &GameSim, sticky: &mut Vec<[StickySurface; 4]>) {
        let state = capture_all(sim, sticky);
        self.steps += 1;
        if self.grid_on {
            events_of(sim, &mut self.pending);
            if self.next_frame * 50 / 9 <= self.steps {
                self.next_frame += 1;
                self.grid.push_back(GridFrame { state: state.clone(), events: std::mem::take(&mut self.pending) });
            }
        } else {
            events_of(sim, &mut self.events);
        }
        self.state = Some(state);
    }
}

/// The physics thread's end: after every step the sound's inputs go into the feed.
#[cfg(windows)]
pub struct AudioSink {
    feed: Arc<Mutex<Feed>>,
    sticky: Vec<[StickySurface; 4]>,
}

#[cfg(windows)]
impl AudioSink {
    pub fn new(feed: Arc<Mutex<Feed>>) -> AudioSink {
        AudioSink { feed, sticky: Default::default() }
    }
}

#[cfg(windows)]
impl StepSink for AudioSink {
    fn after_step(&mut self, sim: &GameSim, _input: &StepInput) -> Result<(), String> {
        // the copy is made outside the lock; the lock is held for a move
        let mut local = Feed::default();
        let state = capture_all(sim, &mut self.sticky);
        events_of(sim, &mut local.events);
        let mut feed = self.feed.lock().unwrap();
        feed.steps += 1;
        if feed.grid_on {
            feed.pending.append(&mut local.events);
            if feed.next_frame * 50 / 9 <= feed.steps {
                feed.next_frame += 1;
                let events = std::mem::take(&mut feed.pending);
                feed.grid.push_back(GridFrame { state: state.clone(), events });
            }
        } else {
            feed.events.append(&mut local.events);
        }
        feed.state = Some(state);
        Ok(())
    }
}

/// What the command line says about the sound.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AudioOptions {
    /// `--no-audio`
    pub off: bool,
    /// `--volume <0..1>`: instead of `audio.ini [LEVELS] MASTER`
    pub volume: Option<f32>,
    /// `--audio-wav <file>`: the mix goes to a WAV through FMOD's non-real-time writer
    pub wav: Option<PathBuf>,
    /// `--audio-log <file>`: every FMOD call
    pub log: Option<PathBuf>,
    /// `--audio-null`: FMOD's no-sound output in real time (for timing runs)
    pub null: bool,
    /// `--audio-ini <file>`: instead of Documents `cfg/audio.ini`
    pub ini: Option<PathBuf>,
    /// `--audio-oracle-camera <cockpit|chase|trackside>`: the listener of `tools/audio_oracle`
    pub oracle_camera: Option<String>,
    /// `--audio-script <log>`: answers of another run to the event state queries (see
    /// `rustyac_audio::fmod::log::start_scripted`)
    pub script: Option<PathBuf>,
    /// rustyAC's own `cfg` folder: its `audio.ini` and `camera_manager.ini` (the Audio app's)
    /// come before Documents'
    pub cfg_dir: Option<PathBuf>,
    /// Task 41b: keep a sound tape of the frames (see [`GameAudio::take_tape`])
    pub tape: bool,
}

impl AudioOptions {
    /// Is the mix written without a clock (one block per 1/60 s frame of simulated time)?
    pub fn clockless(&self) -> bool {
        self.wav.is_some()
    }
}

/// How long the sound took on the display thread.
#[derive(Clone, Copy, Debug, Default)]
pub struct AudioTiming {
    pub frames: u64,
    pub sum: Duration,
    pub max: Duration,
    /// FMOD's own figures at the end, percent of one core: mixer, streams, geometry, update,
    /// Studio (its own threads when a sound card or the no-sound output paces it).
    pub fmod: [f32; 5],
}

/// The display thread's end: the engine, the track's sounds, the car's.
pub struct GameAudio {
    world: AudioWorld,
    feed: Arc<Mutex<Feed>>,
    last: Option<Vec<CarFrame>>,
    camera: Option<(i32, i32)>,
    /// the sound cards' names (`AudioEngine::getDriverInfo`)
    #[cfg_attr(feature = "wgpu", allow(dead_code))]
    drivers: Vec<String>,
    bonnet_external: bool,
    bumper_external: bool,
    paused: bool,
    /// `car.ini [BASIC] GRAPHICS_OFFSET` of every car
    graphics_offsets: Vec<[f32; 3]>,
    oracle: Option<(OracleCamera, Option<CarFrame>)>,
    pub timing: AudioTiming,
    logging: bool,
    frame_number: u64,
    focus: usize,
    /// the pit crew's wheel guns, made when the first pit stop needs them (the game makes them
    /// with the session; here a session without a stop keeps the calls it always made)
    crew: Option<rustyac_audio::pit_crew::CrewGuns>,
    /// Task 41b: the sound tape kept so far (`AudioOptions::tape`)
    tape: Option<Vec<u8>>,
}

/// A sound tape: what the sound was given, frame by frame (the cars, the events, the camera),
/// for a run that cannot be repeated (online) to be heard again by [`GameAudio::play_tape`].
/// shortcut: the pit crew's guns and the Audio app's actions are not on it, tape them when a proof drives them.
const TAPE_MAGIC: &[u8; 8] = b"RACSTP01";

fn tape_u32(tape: &mut Vec<u8>, v: u32) {
    tape.extend_from_slice(&v.to_le_bytes());
}

fn tape_text(tape: &mut Vec<u8>, v: &str) {
    tape_u32(tape, v.len() as u32);
    tape.extend_from_slice(v.as_bytes());
}

struct TapeReader<'a>(&'a [u8], usize);

impl TapeReader<'_> {
    fn bytes(&mut self, n: usize) -> Result<&[u8], String> {
        let b = self.0.get(self.1..self.1 + n).ok_or("the sound tape ends early")?;
        self.1 += n;
        Ok(b)
    }
    fn u32(&mut self) -> Result<u32, String> {
        let b = self.bytes(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }
    fn f32(&mut self) -> Result<f32, String> {
        Ok(f32::from_bits(self.u32()?))
    }
    fn text(&mut self) -> Result<String, String> {
        let n = self.u32()? as usize;
        Ok(String::from_utf8_lossy(self.bytes(n)?).into_owned())
    }
}

/// Documents `Assetto Corsa`, where the game keeps `cfg/audio.ini`.
fn documents() -> Option<PathBuf> {
    rustyac_content::install::documents()
}

impl GameAudio {
    /// Starts the sound for a car on a track (or on the flat road). `Ok(None)` with a line on
    /// the console when there is none to be had: no Assetto Corsa folder, no FMOD in it.
    /// `cars`: every car of the session, the player's first: its name in the game, its data
    /// folder and its shape.
    pub fn start(options: &AudioOptions, feed: Arc<Mutex<Feed>>, cars: &[(String, PathBuf, CarShape)], track: Option<(&Path, &str)>) -> Option<GameAudio> {
        if options.off {
            return None;
        }
        match Self::try_start(options, feed, cars, track) {
            Ok(audio) => Some(audio),
            Err(message) => {
                println!("audio: no sound ({message})");
                None
            }
        }
    }

    fn try_start(options: &AudioOptions, feed: Arc<Mutex<Feed>>, cars: &[(String, PathBuf, CarShape)], track: Option<(&Path, &str)>) -> Result<GameAudio, String> {
        let ac = rustyac_content::install::ac_root().ok_or("Assetto Corsa's folder was not found: its FMOD DLLs and sound banks are needed")?;
        // Game::Game makes the sound engine only when the game's own switch is on
        let switch = IniReader::load(&ac.join("system/cfg/audio_engine.ini"))?;
        if switch.ready && switch.get_int("SETTINGS", "ENABLE_AUDIO").unwrap_or(0) == 0 {
            return Err("system/cfg/audio_engine.ini has ENABLE_AUDIO=0".to_string());
        }
        let output = match (&options.wav, options.null) {
            // (the page's is a path in FMOD's own file system)
            (Some(file), _) if cfg!(target_arch = "wasm32") => raw::Output::WavNrt { file: file.clone(), rate: NRT_RATE, block: NRT_BLOCK },
            (Some(file), _) => raw::Output::WavNrt { file: std::path::absolute(file).map_err(|e| e.to_string())?, rate: NRT_RATE, block: NRT_BLOCK },
            (None, true) => raw::Output::NoSound,
            (None, false) => raw::Output::Device,
        };
        raw::load(&ac, output)?;
        let logging = options.log.is_some();
        match (&options.log, &options.script) {
            (Some(file), Some(script)) => log::start_scripted(Some(file), script)?,
            // (the page writes no files: its log is kept, for `finish_kept`)
            #[cfg(target_arch = "wasm32")]
            (Some(_), None) => log::start_kept(None),
            #[cfg(not(target_arch = "wasm32"))]
            (Some(file), None) => log::start(Some(file))?,
            _ => {}
        }
        log::mark("setup");
        let own = |name: &str| options.cfg_dir.as_ref().map(|dir| dir.join(name)).filter(|file| file.is_file());
        let audio_ini = match &options.ini {
            Some(file) => file.clone(),
            None => own("audio.ini").or_else(|| documents().map(|d| d.join("cfg/audio.ini"))).unwrap_or_default(),
        };
        let files = EngineFiles { content_root: ac.clone(), audio_engine_ini: ac.join("system/cfg/audio_engine.ini"), audio_ini: audio_ini.clone() };
        let mut engine = AudioEngine::new(files)?;
        if engine.version != rustyac_audio::fmod::types::FMOD_VERSION {
            // the game only complains and carries on; rustyAC's binding is written for 1.08.12
            return Err(format!("the FMOD DLLs in {} are version {:#x}; Assetto Corsa's own, 1.08.12 (0x10812), are needed", ac.display(), engine.version));
        }
        // Game::Game: the master level
        let levels = IniReader::load(&audio_ini)?;
        let master = match options.volume {
            Some(volume) => volume.clamp(0.0, 1.0),
            None if levels.ready => levels.get_float("LEVELS", "MASTER").unwrap_or(1.0),
            None => 1.0,
        };
        engine.set_volume(master);
        // TrackAvatar: the track's emitters, reverbs and occluders; Sim::loadTrack: the pool
        // of surface sounds
        let mut track_audio = None;
        if let Some((folder, layout)) = track {
            let scene = Scene::load(folder, layout)?;
            let data = if layout.is_empty() { folder.to_path_buf() } else { folder.join(layout) };
            track_audio = Some(TrackAudio::new(&mut engine, &scene, &data)?);
            let manager = rustyac_physics::track::surfaces::SurfacesManager::new(&ac.join("system/data/surfaces.ini"), &data.join("data/surfaces.ini"))?;
            let surfaces: Vec<(String, String)> = manager.surfaces.iter().map(|(key, surface)| (key.clone(), surface.wav.clone())).collect();
            cache_surface_sounds(&mut engine, &surfaces);
        }
        // Sim::addCar: the sound of every car in the order they are made (the number is the guid)
        let mut sounds = Vec::new();
        for (guid, (car_name, car_data, _)) in cars.iter().enumerate() {
            // CameraCarDefinition: `EXTERNAL_SOUND` of each of the car's own cameras (default: yes)
            let cameras = IniReader::load(&append_path(car_data, "cameras.ini"))?;
            let mut external = Vec::new();
            for n in 0.. {
                let section = format!("CAMERA_{n}");
                if !cameras.has_section(&section) {
                    break;
                }
                external.push(if cameras.has_key(&section, "EXTERNAL_SOUND") { cameras.get_int(&section, "EXTERNAL_SOUND").unwrap_or(0) != 0 } else { true });
            }
            let info = SoundInfo { unix_name: car_name.clone(), guid: guid as i32, data_folder: car_data.clone(), car_cameras_external_sound: external };
            sounds.push(CarSound::new(&mut engine, info)?);
        }
        let has_bank = sounds.first().is_some_and(|car| car.audio.is_some());
        let silent: Vec<&str> = cars.iter().zip(&sounds).skip(1).filter(|(_, sound)| sound.audio.is_none()).map(|(car, _)| car.0.as_str()).collect();
        // CameraDrivableManager: the bonnet's and the bumper's view may count as outside
        let manager = match own("camera_manager.ini").or_else(|| documents().map(|d| d.join("cfg/camera_manager.ini"))) {
            Some(file) => IniReader::load(&file)?,
            None => IniReader::default(),
        };
        let flag = |key: &str| manager.has_key("AUDIO", key) && manager.get_int("AUDIO", key).unwrap_or(0) != 0;
        let oracle = match &options.oracle_camera {
            Some(name) => Some((OracleCamera::parse(name)?, None)),
            None => None,
        };
        let drivers = if options.wav.is_none() && !options.null { engine.driver_names() } else { Vec::new() };
        let devices = drivers.len();
        println!(
            "audio: FMOD {:#x} from {}; {}; car bank {}; track: {}",
            engine.version,
            ac.display(),
            match (&options.wav, options.null) {
                (Some(file), _) => format!("writing {} (48 kHz, no sound card)", file.display()),
                (None, true) => "no-sound output".to_string(),
                (None, false) => format!("{devices} output device(s), master volume {master}"),
            },
            if has_bank { "loaded" } else { "missing: the car is silent" },
            match &track_audio {
                Some(t) => format!("{} ambience emitter(s), {} reverb zone(s), {} occluder(s)", t.emitters(), t.reverbs(), t.occluders()),
                None => "none".to_string(),
            }
        );
        if sounds.len() > 1 {
            println!(
                "audio: {} other car(s) with their own sounds, [LEVELS] OPPONENTS {}{}",
                sounds.len() - 1,
                sounds.iter().skip(1).find_map(|car| car.audio.as_ref()).map_or("-".to_string(), |audio| format!("{}", audio.mix_volumes.opponents)),
                if silent.is_empty() { String::new() } else { format!("; no sound bank for {}", silent.join(", ")) }
            );
        }
        Ok(GameAudio {
            world: AudioWorld::new(engine, track_audio, sounds),
            feed,
            last: None,
            camera: None,
            drivers,
            bonnet_external: flag("BONNET_EXTERNAL"),
            bumper_external: flag("BUMPER_EXTERNAL"),
            paused: false,
            graphics_offsets: cars.iter().map(|car| car.2.graphics_offset).collect(),
            oracle,
            timing: AudioTiming::default(),
            logging,
            frame_number: 0,
            focus: 0,
            crew: None,
            tape: options.tape.then(|| {
                let mut tape = TAPE_MAGIC.to_vec();
                tape_u32(&mut tape, cars.len() as u32);
                for (name, _, shape) in cars {
                    tape_text(&mut tape, name);
                    shape.graphics_offset.iter().for_each(|v| tape_u32(&mut tape, v.to_bits()));
                }
                let (folder, layout) = track.map_or((String::new(), ""), |(folder, layout)| (folder.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(), layout));
                tape_text(&mut tape, &folder);
                tape_text(&mut tape, layout);
                tape
            }),
        })
    }

    /// The camera manager as the sound sees it.
    fn camera_view(&self, camera: &DrivingCamera) -> CameraView {
        if let Some((oracle, _)) = &self.oracle {
            return oracle.view();
        }
        CameraView {
            mode: match camera.mode {
                CameraMode::Cockpit => 0,
                CameraMode::Car => 1,
                CameraMode::Drivable => 2,
            },
            car_camera_index: camera.car_index as i32,
            drivable_mode: camera.drivable as i32,
            bonnet_external_sound: self.bonnet_external,
            bumper_external_sound: self.bumper_external,
        }
    }

    /// `CarAvatar::makeBodyMatrix` @ 0x1400d8ec0, the position: the body's place with the
    /// model's offset (`car.ini [GRAPHICS] GRAPHICS_OFFSET`).
    fn body_position(&self, car: usize, w: &[f32; 16]) -> [f32; 3] {
        let g = self.graphics_offsets.get(car).copied().unwrap_or([0.0; 3]);
        [
            (((g[1] * w[4]) + (g[0] * w[0])) + (g[2] * w[8])) + w[12],
            (((g[0] * w[1]) + (g[1] * w[5])) + (g[2] * w[9])) + w[13],
            (((g[0] * w[2]) + (g[1] * w[6])) + (g[2] * w[10])) + w[14],
        ]
    }

    fn one_frame(&mut self, mut states: Vec<CarFrame>, events: &[PhysicsEvent], view: CameraView, camera_matrix: &Mat, dt: f32) {
        if let Some(tape) = &mut self.tape {
            tape.push(b'F');
            rustyac_audio::golden::write_frames(tape, &states, events);
            for v in [view.mode, view.car_camera_index, view.drivable_mode, view.bonnet_external_sound as i32, view.bumper_external_sound as i32] {
                tape_u32(tape, v as u32);
            }
            camera_matrix.iter().flatten().chain([&dt]).for_each(|v| tape_u32(tape, v.to_bits()));
        }
        // (the page has no `Instant`: there the frames are counted, not timed)
        #[cfg(not(target_arch = "wasm32"))]
        let started = Instant::now();
        for (car, state) in states.iter_mut().enumerate() {
            state.body_position = self.body_position(car, &state.world_matrix);
        }
        // Sim::focusedCarIndex: the car the camera is on
        let focus = self.focus.min(states.len().saturating_sub(1));
        let state = &states[focus];
        let sim = SimView { focused_car_index: focus as u32, camera: Some(view) };
        // ACCameraManager::setMode ends in setAudioDistanceScale; F6 inside the car cameras
        // (only the index changes) does not
        if self.camera != Some((view.mode, view.drivable_mode)) {
            self.camera = Some((view.mode, view.drivable_mode));
            self.world.set_audio_distance_scale(&sim, 1.0);
        }
        let listener = match &mut self.oracle {
            Some((oracle, first)) => {
                let first = first.get_or_insert_with(|| state.clone());
                oracle.listener(state, first)
            }
            // the cameras of the focused car hand over their matrix and that car's velocity
            None => (std::array::from_fn(|i| camera_matrix[i / 4][i % 4]), state.velocity),
        };
        if self.logging {
            log::mark(&format!("frame {}", self.frame_number));
        }
        self.frame_number += 1;
        self.world.frame(&FrameInput { cars: &states, events, sim, listener: Some(listener), dt });
        self.last = Some(states);
        self.timing.frames += 1;
        #[cfg(not(target_arch = "wasm32"))]
        {
            let took = started.elapsed();
            self.timing.sum += took;
            self.timing.max = self.timing.max.max(took);
        }
    }

    /// Task 28c: a replay shows: the sound's cars take these states (the physics stands still,
    /// so nothing else writes them).
    pub fn set_replay_states(&mut self, states: &[rustyac_physics::car_physics_state::CarPhysicsState]) {
        let frames: Vec<CarFrame> = states.iter().map(|state| CarFrame::from_physics_state(&state.to_game_bytes())).collect();
        self.feed.lock().unwrap().state = Some(frames);
    }

    fn crew_guns(&mut self) -> &rustyac_audio::pit_crew::CrewGuns {
        if self.crew.is_none() {
            self.crew = Some(rustyac_audio::pit_crew::CrewGuns::new(&mut self.world.engine));
        }
        self.crew.as_ref().expect("just made")
    }

    /// `AudioEvent::isPlaying` of a wheel gun of the pit crew (`screw` or unscrew).
    pub fn crew_is_playing(&mut self, wheel: usize, screw: bool) -> bool {
        self.crew_guns();
        self.crew.as_ref().is_some_and(|guns| guns.is_playing(&self.world.engine, wheel, screw))
    }

    /// A wheel gun starts at its wheel (`AudioEvent::set3DAttributes`, `start`).
    pub fn crew_start(&mut self, wheel: usize, screw: bool, matrix: &Mat) {
        self.crew_guns();
        if let Some(guns) = &self.crew {
            let mut m = [0.0f32; 16];
            for (r, row) in matrix.iter().enumerate() {
                m[r * 4..r * 4 + 4].copy_from_slice(row);
            }
            guns.start(&self.world.engine, wheel, screw, &m);
        }
    }

    /// A wheel gun stops (`AudioEvent::stop`).
    pub fn crew_stop(&mut self, wheel: usize, screw: bool) {
        self.crew_guns();
        if let Some(guns) = &self.crew {
            guns.stop(&self.world.engine, wheel, screw);
        }
    }

    /// `CarAvatar::evOnBackfireTriggered` of the frame, when the picture's car ran the test
    /// (it owns `fuelInExhaust`, which its flames empty): the sound plays on that, as in the
    /// game, and does not test by itself. `None`: no such car (the sound tests by itself).
    pub fn set_picture_backfire(&mut self, fired: Option<bool>) {
        if let Some(tape) = &mut self.tape {
            tape.extend([b'b', fired.map_or(2, u8::from)]);
        }
        if let Some(car) = self.world.cars.first_mut() {
            car.picture_backfire = fired;
        }
    }

    /// The same for every car, in the cars' order.
    pub fn set_picture_backfires(&mut self, fired: &[Option<bool>]) {
        if let Some(tape) = &mut self.tape {
            tape.push(b'B');
            tape_u32(tape, fired.len() as u32);
            tape.extend(fired.iter().map(|f| f.map_or(2, u8::from)));
        }
        for (car, fired) in self.world.cars.iter_mut().zip(fired) {
            car.picture_backfire = *fired;
        }
    }

    /// `Sim::focusedCarIndex`: the car the camera is on. Its sounds are the full set (inside
    /// or outside), the others' the opponents' set; the listener's velocity is its. As in the
    /// game, the louder tyres and engine of an outside view stay with the car that was focused
    /// when the camera mode last changed (`ACCameraManager::setAudioDistanceScale` is not
    /// called for a change of the focus).
    /// The sound as the Audio app reads it.
    #[cfg(not(feature = "wgpu"))]
    pub fn app_sound(&self) -> crate::render::gui_apps::AppSound {
        crate::render::gui_apps::AppSound { drivers: self.drivers.clone(), camera: [self.bonnet_external, self.bumper_external] }
    }

    /// The Audio app's calls: `AudioEngine::setVolume` 0x1401fbdf0, a level of every car's
    /// `mixVolumes` (the transmission's -1: the engine's level now, else times 3), the views'
    /// sounds (the game's are a car's `DrivableCameraDef`; rustyAC's camera has one pair).
    pub fn app_action(&mut self, action: &rustyac_gui::apps::AppAction) {
        use rustyac_gui::apps::{AppAction, MixLevel};
        match action {
            AppAction::SetMasterVolume(v) => self.world.engine.set_volume(*v),
            AppAction::SetMixLevel { level, value } => {
                for car in self.world.cars.iter_mut() {
                    let Some(audio) = car.audio.as_mut() else { continue };
                    let m = &mut audio.mix_volumes;
                    match level {
                        MixLevel::Wind => m.wind = *value,
                        MixLevel::Tyres => m.tyres = *value,
                        MixLevel::Surfaces => m.surfaces = *value,
                        MixLevel::Dirt => m.dirt = *value,
                        MixLevel::Engine => m.engine = *value,
                        MixLevel::Transmission => m.transmission = if *value == -1.0 { m.engine } else { *value * 3.0 },
                        MixLevel::Opponents => m.opponents = *value,
                    }
                }
            }
            AppAction::SetCameraSound { which, external, .. } => {
                if *which == 0 {
                    self.bonnet_external = *external;
                } else {
                    self.bumper_external = *external;
                }
            }
            _ => {}
        }
    }

    pub fn set_focus(&mut self, focus: usize) {
        if let Some(tape) = &mut self.tape {
            tape.push(b'S');
            tape_u32(tape, focus as u32);
        }
        self.focus = focus;
    }

    /// One picture frame against the clock: the newest physics state, the events since the
    /// last frame, the camera's matrix as the listener. `seconds` is the time since the last
    /// frame; a paused game keeps running its sound on the frozen state with the master volume
    /// at zero, as the game's pause menu does.
    pub fn frame(&mut self, camera: &DrivingCamera, camera_matrix: &Mat, seconds: f64, paused: bool) {
        if paused != self.paused {
            if let Some(tape) = &mut self.tape {
                tape.extend([b'P', paused as u8]);
            }
            self.paused = paused;
            if paused {
                self.world.engine.stop();
            } else {
                self.world.engine.start();
            }
        }
        // Game::onIdle does nothing in a frame without time: the events wait for the next one
        let dt = frame_dt(seconds);
        if dt.is_nan() || dt <= 0.0 {
            return;
        }
        let (state, events) = {
            let mut feed = self.feed.lock().unwrap();
            let Some(state) = feed.state.clone().or_else(|| self.last.clone()) else { return };
            (state, std::mem::take(&mut feed.events))
        };
        let view = self.camera_view(camera);
        self.one_frame(state, &events, view, camera_matrix, dt);
    }

    /// How many cars have sounds.
    pub fn car_count(&self) -> usize {
        self.world.cars.len()
    }

    /// The frames of the 60 Hz grid that are waiting (a WAV written without a clock): each is
    /// one block of the mix. `camera` gives each frame's listener from the frame's own state.
    pub fn grid_frames(&mut self, mut camera: impl FnMut(&CarFrame) -> (DrivingCamera, Mat)) {
        loop {
            let Some(frame) = self.feed.lock().unwrap().grid.pop_front() else { break };
            let Some(focused) = frame.state.get(self.focus.min(frame.state.len().saturating_sub(1))) else { continue };
            let (driving, matrix) = camera(focused);
            let view = self.camera_view(&driving);
            self.one_frame(frame.state, &frame.events, view, &matrix, NRT_DT);
        }
    }

    /// Ends the sound: the cars' sounds, the track's, the engine (which closes a WAV).
    pub fn finish(self) -> AudioTiming {
        let timing = self.teardown();
        if let Some(summary) = log::finish() {
            println!("audio log: {} lines, hash {:016x}", summary.lines, summary.hash);
        }
        timing
    }

    /// The sound tape so far (`AudioOptions::tape`); the tape goes on.
    pub fn take_tape(&self) -> Option<Vec<u8>> {
        self.tape.clone()
    }

    /// `rustyac.exe --sound-tape <file>`: the sound of a tape [`take_tape`](Self::take_tape)
    /// gave, its cars and track from this install, each frame as it was heard. The sound back,
    /// for [`finish`](Self::finish).
    pub fn play_tape(options: &AudioOptions, bytes: &[u8]) -> Result<GameAudio, String> {
        let ac = rustyac_content::install::ac_root().ok_or("Assetto Corsa's folder was not found")?;
        if bytes.get(..8) != Some(TAPE_MAGIC.as_slice()) {
            return Err("not a sound tape".to_string());
        }
        let mut r = TapeReader(bytes, 8);
        let mut cars = Vec::new();
        for _ in 0..r.u32()? {
            let name = r.text()?;
            let graphics_offset = [r.f32()?, r.f32()?, r.f32()?];
            let data = ac.join("content/cars").join(&name).join("data");
            cars.push((name, data, CarShape { graphics_offset, ..CarShape::plain() }));
        }
        let (folder, layout) = (r.text()?, r.text()?);
        let folder = (!folder.is_empty()).then(|| ac.join("content/tracks").join(folder));
        let feed = Arc::new(Mutex::new(Feed::default()));
        let mut audio = Self::try_start(options, feed, &cars, folder.as_deref().map(|f| (f, layout.as_str())))?;
        let fired = |b: u8| (b != 2).then_some(b == 1);
        while r.1 < bytes.len() {
            match r.bytes(1)?[0] {
                b'F' => {
                    let (states, events) = rustyac_audio::golden::read_frames(bytes, &mut r.1)?;
                    let [mode, car_camera_index, drivable_mode, bonnet, bumper] = [r.u32()?, r.u32()?, r.u32()?, r.u32()?, r.u32()?];
                    let view = CameraView { mode: mode as i32, car_camera_index: car_camera_index as i32, drivable_mode: drivable_mode as i32, bonnet_external_sound: bonnet != 0, bumper_external_sound: bumper != 0 };
                    let mut matrix = [[0.0f32; 4]; 4];
                    for v in matrix.iter_mut().flatten() {
                        *v = r.f32()?;
                    }
                    let dt = r.f32()?;
                    audio.one_frame(states, &events, view, &matrix, dt);
                }
                b'P' => {
                    audio.paused = r.bytes(1)?[0] != 0;
                    if audio.paused {
                        audio.world.engine.stop();
                    } else {
                        audio.world.engine.start();
                    }
                }
                b'b' => audio.set_picture_backfire(fired(r.bytes(1)?[0])),
                b'B' => {
                    let n = r.u32()? as usize;
                    let list: Vec<Option<bool>> = r.bytes(n)?.iter().map(|b| fired(*b)).collect();
                    audio.set_picture_backfires(&list);
                }
                b'S' => audio.set_focus(r.u32()? as usize),
                other => return Err(format!("the sound tape has a record {other:#x} it should not have")),
            }
        }
        Ok(audio)
    }

    /// As [`finish`](Self::finish), the call log back (the page's: it writes no files).
    pub fn finish_kept(self) -> Option<Vec<u8>> {
        self.teardown();
        log::finish_kept().map(|(_, kept)| kept)
    }

    fn teardown(self) -> AudioTiming {
        let mut timing = self.timing;
        timing.fmod = self.world.engine.cpu_usage();
        if self.logging {
            log::mark("teardown");
        }
        self.world.destroy();
        timing
    }

    /// The sound of a session's cars, the player's (`info`) first, then `others`
    /// (`GameSim::opponent_infos`), on its track.
    pub fn start_for(options: &AudioOptions, feed: Arc<Mutex<Feed>>, info: &crate::sim::CarInfo, others: &[crate::sim::CarInfo]) -> Option<GameAudio> {
        let cars = Self::cars_of(info, others);
        Self::start(options, feed, &cars, info.track_folder.as_deref().map(|folder| (folder, info.track_layout.as_str())))
    }

    fn cars_of(info: &crate::sim::CarInfo, others: &[crate::sim::CarInfo]) -> Vec<(String, PathBuf, CarShape)> {
        // a car's folder name is its name in the game
        std::iter::once(info)
            .chain(others)
            .map(|info| (Path::new(&info.name).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| info.name.clone()), info.data_path.clone(), CarShape::of(info)))
            .collect()
    }

    /// `rustyac.exe --replay <file> --headless --audio-wav`: a recorded drive (every car of it)
    /// as fast as it goes, its sound without a clock: one frame of the sound after every
    /// `floor(n * 50 / 9)` physics steps, the listener at `camera` of car `focus`. The sound
    /// back, for [`finish`](Self::finish).
    pub fn replay(options: &AudioOptions, setup: crate::input_file::SimSetup, steps: &[crate::input_file::StepInput], camera: &str, focus: usize) -> Result<GameAudio, String> {
        let mut sim = GameSim::new(setup, Box::new(crate::sim::ReplaySource::default()))?;
        let info = sim.car_info();
        let others = sim.opponent_infos();
        let focus = focus.min(others.len());
        let shape = CarShape::of(if focus == 0 { &info } else { &others[focus - 1] });
        let feed = Arc::new(Mutex::new(Feed::with_grid()));
        if options.off {
            return Err("the sound is off".to_string());
        }
        let mut audio = Self::try_start(options, Arc::clone(&feed), &Self::cars_of(&info, &others), info.track_folder.as_deref().map(|folder| (folder, info.track_layout.as_str())))?;
        audio.set_focus(focus);
        let mut camera = DrivingCamera::from_name(camera, &shape)?;
        let mut sticky = Vec::new();
        for step in steps {
            sim.step_recorded(step)?;
            let waiting = {
                let mut feed = feed.lock().unwrap();
                feed.push(&sim, &mut sticky);
                !feed.grid.is_empty()
            };
            if waiting {
                let view = if focus == 0 { crate::view::CarView::capture(&sim, 0.0) } else { crate::view::CarView::capture_opponent(&sim, focus - 1, 0.0) };
                let frame = camera.update(&view, &shape, view.acc_g, NRT_DT);
                audio.grid_frames(|_| (camera, frame.matrix));
            }
        }
        Ok(audio)
    }
}

impl AudioTiming {
    pub fn report(&self) -> String {
        if self.frames == 0 {
            return "audio: no frame".to_string();
        }
        format!(
            "audio: {} frames, {:.3} ms a frame on the display thread (slowest {:.3} ms); FMOD's own figures at the end: mixer {:.2} %, streams {:.2} %, geometry {:.2} %, update {:.2} %, studio {:.2} % of a core",
            self.frames,
            self.sum.as_secs_f64() * 1000.0 / self.frames as f64,
            self.max.as_secs_f64() * 1000.0,
            self.fmod[0],
            self.fmod[1],
            self.fmod[2],
            self.fmod[3],
            self.fmod[4]
        )
    }
}
