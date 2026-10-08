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
use std::time::{Duration, Instant};

use rustyac_audio::car::{CameraView, CarFrame, CarInfo as SoundInfo, SimView};
use rustyac_audio::engine::{AudioEngine, EngineFiles};
use rustyac_audio::fmod::{log, raw};
use rustyac_audio::sim::{frame_dt, AudioWorld, CarSound, FrameInput, PhysicsEvent};
use rustyac_audio::tape::OracleCamera;
use rustyac_audio::track::{cache_surface_sounds, Scene, TrackAudio};
use rustyac_physics::data::ini::{append_path, IniReader};
use rustyac_physics::vecmath::xm_matrix_multiply;

use crate::input_file::StepInput;
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
    let car = &sim.car.car;
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

/// The physics thread's `ACPhysicsEvent`s for the sound.
fn events_of(sim: &GameSim, out: &mut Vec<PhysicsEvent>) {
    for e in &sim.car.car.physics_events {
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
    pub state: CarFrame,
    pub events: Vec<PhysicsEvent>,
}

/// What the physics thread leaves for the display thread.
#[derive(Default)]
pub struct Feed {
    /// The newest step's state (the game's `PhysicsAvatar::update` copies the newest finished
    /// step, unblended).
    pub state: Option<CarFrame>,
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
    pub fn push(&mut self, sim: &GameSim, sticky: &mut [StickySurface; 4]) {
        let state = capture(sim, sticky);
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
pub struct AudioSink {
    feed: Arc<Mutex<Feed>>,
    sticky: [StickySurface; 4],
}

impl AudioSink {
    pub fn new(feed: Arc<Mutex<Feed>>) -> AudioSink {
        AudioSink { feed, sticky: Default::default() }
    }
}

impl StepSink for AudioSink {
    fn after_step(&mut self, sim: &GameSim, _input: &StepInput) -> Result<(), String> {
        // the copy is made outside the lock; the lock is held for a move
        let mut local = Feed::default();
        let state = capture(sim, &mut self.sticky);
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
    last: Option<CarFrame>,
    camera: Option<(i32, i32)>,
    bonnet_external: bool,
    bumper_external: bool,
    paused: bool,
    graphics_offset: [f32; 3],
    oracle: Option<(OracleCamera, Option<CarFrame>)>,
    pub timing: AudioTiming,
    logging: bool,
    frame_number: u64,
}

/// Documents `Assetto Corsa`, where the game keeps `cfg/audio.ini`.
fn documents() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE").map(|home| PathBuf::from(home).join("Documents").join("Assetto Corsa"))
}

impl GameAudio {
    /// Starts the sound for a car on a track (or on the flat road). `Ok(None)` with a line on
    /// the console when there is none to be had: no Assetto Corsa folder, no FMOD in it.
    pub fn start(options: &AudioOptions, feed: Arc<Mutex<Feed>>, car_name: &str, car_data: &Path, track: Option<(&Path, &str)>, shape: &CarShape) -> Option<GameAudio> {
        if options.off {
            return None;
        }
        match Self::try_start(options, feed, car_name, car_data, track, shape) {
            Ok(audio) => Some(audio),
            Err(message) => {
                println!("audio: no sound ({message})");
                None
            }
        }
    }

    fn try_start(options: &AudioOptions, feed: Arc<Mutex<Feed>>, car_name: &str, car_data: &Path, track: Option<(&Path, &str)>, shape: &CarShape) -> Result<GameAudio, String> {
        let ac = rustyac_content::install::ac_root().ok_or("Assetto Corsa's folder was not found: its FMOD DLLs and sound banks are needed")?;
        // Game::Game makes the sound engine only when the game's own switch is on
        let switch = IniReader::load(&ac.join("system/cfg/audio_engine.ini"))?;
        if switch.ready && switch.get_int("SETTINGS", "ENABLE_AUDIO").unwrap_or(0) == 0 {
            return Err("system/cfg/audio_engine.ini has ENABLE_AUDIO=0".to_string());
        }
        let output = match (&options.wav, options.null) {
            (Some(file), _) => raw::Output::WavNrt { file: std::path::absolute(file).map_err(|e| e.to_string())?, rate: NRT_RATE, block: NRT_BLOCK },
            (None, true) => raw::Output::NoSound,
            (None, false) => raw::Output::Device,
        };
        raw::load(&ac, output)?;
        let logging = options.log.is_some();
        match (&options.log, &options.script) {
            (Some(file), Some(script)) => log::start_scripted(Some(file), script)?,
            (Some(file), None) => log::start(Some(file))?,
            _ => {}
        }
        log::mark("setup");
        let audio_ini = match &options.ini {
            Some(file) => file.clone(),
            None => documents().map(|d| d.join("cfg/audio.ini")).unwrap_or_default(),
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
        let info = SoundInfo { unix_name: car_name.to_string(), guid: 0, data_folder: car_data.to_path_buf(), car_cameras_external_sound: external };
        let car = CarSound::new(&mut engine, info)?;
        let has_bank = car.audio.is_some();
        // CameraDrivableManager: the bonnet's and the bumper's view may count as outside
        let manager = match documents() {
            Some(d) => IniReader::load(&d.join("cfg/camera_manager.ini"))?,
            None => IniReader::default(),
        };
        let flag = |key: &str| manager.has_key("AUDIO", key) && manager.get_int("AUDIO", key).unwrap_or(0) != 0;
        let oracle = match &options.oracle_camera {
            Some(name) => Some((OracleCamera::parse(name)?, None)),
            None => None,
        };
        let devices = if options.wav.is_none() && !options.null { engine.driver_names().len() } else { 0 };
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
        Ok(GameAudio {
            world: AudioWorld::new(engine, track_audio, vec![car]),
            feed,
            last: None,
            camera: None,
            bonnet_external: flag("BONNET_EXTERNAL"),
            bumper_external: flag("BUMPER_EXTERNAL"),
            paused: false,
            graphics_offset: shape.graphics_offset,
            oracle,
            timing: AudioTiming::default(),
            logging,
            frame_number: 0,
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
    fn body_position(&self, w: &[f32; 16]) -> [f32; 3] {
        let g = self.graphics_offset;
        [
            (((g[1] * w[4]) + (g[0] * w[0])) + (g[2] * w[8])) + w[12],
            (((g[0] * w[1]) + (g[1] * w[5])) + (g[2] * w[9])) + w[13],
            (((g[0] * w[2]) + (g[1] * w[6])) + (g[2] * w[10])) + w[14],
        ]
    }

    fn one_frame(&mut self, mut state: CarFrame, events: &[PhysicsEvent], camera: &DrivingCamera, camera_matrix: &Mat, dt: f32) {
        let started = Instant::now();
        state.body_position = self.body_position(&state.world_matrix);
        let view = self.camera_view(camera);
        let sim = SimView { focused_car_index: 0, camera: Some(view) };
        // ACCameraManager::setMode ends in setAudioDistanceScale; F6 inside the car cameras
        // (only the index changes) does not
        if self.camera != Some((view.mode, view.drivable_mode)) {
            self.camera = Some((view.mode, view.drivable_mode));
            self.world.set_audio_distance_scale(&sim, 1.0);
        }
        let listener = match &mut self.oracle {
            Some((oracle, first)) => {
                let first = first.get_or_insert_with(|| state.clone());
                oracle.listener(&state, first)
            }
            // the cameras of the driven car hand over their matrix and the body's velocity
            None => (std::array::from_fn(|i| camera_matrix[i / 4][i % 4]), state.velocity),
        };
        if self.logging {
            log::mark(&format!("frame {}", self.frame_number));
        }
        self.frame_number += 1;
        self.world.frame(&FrameInput { cars: std::slice::from_ref(&state), events, sim, listener: Some(listener), dt });
        self.last = Some(state);
        let took = started.elapsed();
        self.timing.frames += 1;
        self.timing.sum += took;
        self.timing.max = self.timing.max.max(took);
    }

    /// One picture frame against the clock: the newest physics state, the events since the
    /// last frame, the camera's matrix as the listener. `seconds` is the time since the last
    /// frame; a paused game keeps running its sound on the frozen state with the master volume
    /// at zero, as the game's pause menu does.
    pub fn frame(&mut self, camera: &DrivingCamera, camera_matrix: &Mat, seconds: f64, paused: bool) {
        if paused != self.paused {
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
        self.one_frame(state, &events, camera, camera_matrix, dt);
    }

    /// The frames of the 60 Hz grid that are waiting (a WAV written without a clock): each is
    /// one block of the mix. `camera` gives each frame's listener from the frame's own state.
    pub fn grid_frames(&mut self, mut camera: impl FnMut(&CarFrame) -> (DrivingCamera, Mat)) {
        loop {
            let Some(frame) = self.feed.lock().unwrap().grid.pop_front() else { break };
            let (driving, matrix) = camera(&frame.state);
            self.one_frame(frame.state, &frame.events, &driving, &matrix, NRT_DT);
        }
    }

    /// Ends the sound: the cars' sounds, the track's, the engine (which closes a WAV).
    pub fn finish(self) -> AudioTiming {
        let mut timing = self.timing;
        timing.fmod = self.world.engine.cpu_usage();
        if self.logging {
            log::mark("teardown");
        }
        self.world.destroy();
        if let Some(summary) = log::finish() {
            println!("audio log: {} lines, hash {:016x}", summary.lines, summary.hash);
        }
        timing
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
