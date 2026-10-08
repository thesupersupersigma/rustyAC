// SPDX-License-Identifier: GPL-3.0-or-later

//! A whole session of the sound as the oracle runs it ([`run_session`]: what `tools/audio_oracle`
//! does on the port's side), and the golden file a test replays it from ([`Golden`]): a short
//! recorded drive (the sound's inputs per frame, as the game's own physics produced them), the
//! answers the game's run got to its "is this event playing" questions, and the line count and
//! hash of the FMOD call log the game's own sound code wrote for it.

use std::path::{Path, PathBuf};

use crate::car::{CameraView, CarFrame, CarInfo, SimView};
use crate::engine::{AudioEngine, EngineFiles, Mat, Vec3};
use crate::fmod::log;
use crate::sim::{AudioWorld, CarSound, FrameInput, PhysicsEvent};
use crate::tape::OracleCamera;
use crate::track::{cache_surface_sounds, Scene, TrackAudio};

/// One frame of a session: the car, the physics events since the frame before, the listener.
pub struct SessionFrame<'a> {
    pub car: &'a CarFrame,
    pub events: &'a [PhysicsEvent],
    pub listener: (Mat, Vec3),
}

/// What a session is set up from.
pub struct Session<'a> {
    pub files: EngineFiles,
    /// `audio.ini [LEVELS] MASTER`, which `Game::Game` hands to the engine.
    pub master: f32,
    /// The track's scene and the folder its `data/audio_sources.ini` is in.
    pub track: Option<(&'a Scene, PathBuf)>,
    /// The track's surfaces: (KEY, WAV).
    pub surfaces: &'a [(String, String)],
    pub car: CarInfo,
    pub camera: CameraView,
    /// The frame time.
    pub dt: f32,
}

/// Sets the session up, runs its frames and tears it down, with the marks the oracle's logs
/// carry (`setup`, `frame <n>`, `teardown`).
pub fn run_session<'a>(session: Session, frames: impl Iterator<Item = SessionFrame<'a>>) -> Result<(), String> {
    log::mark("setup");
    let mut engine = AudioEngine::new(session.files)?;
    engine.set_volume(session.master);
    let track = match session.track {
        Some((scene, data_folder)) => Some(TrackAudio::new(&mut engine, scene, &data_folder)?),
        None => None,
    };
    cache_surface_sounds(&mut engine, session.surfaces);
    let car = CarSound::new(&mut engine, session.car)?;
    let mut world = AudioWorld::new(engine, track, vec![car]);
    let sim = SimView { focused_car_index: 0, camera: Some(session.camera) };
    // the session's camera is set up: ACCameraManager::setAudioDistanceScale
    world.set_audio_distance_scale(&sim, 1.0);
    for (i, frame) in frames.enumerate() {
        log::mark(&format!("frame {i}"));
        world.frame(&FrameInput { cars: std::slice::from_ref(frame.car), events: frame.events, sim, listener: Some(frame.listener), dt: session.dt });
    }
    log::mark("teardown");
    world.destroy();
    Ok(())
}

/// The levels the oracle's runs and the golden test use (the game reads them from Documents
/// `cfg/audio.ini`).
pub const AUDIO_INI: &str = "[LEVELS]\nBRAKES=0.8\nDIRT_BOTTOM=1\nENGINE=1\nMASTER=1\nOPPONENTS=0.8\nSURFACES=0.9\nTYRES=1\nWIND=0.3\nTRANSMISSION=0.33333334\n\n[SETTINGS]\nDRIVER_NAME=no such device\n\n[SKIDS]\nENTRY_POINT=100\n";

pub struct GoldenFrame {
    pub car: CarFrame,
    pub events: Vec<PhysicsEvent>,
}

/// A golden drive.
pub struct Golden {
    pub car: String,
    /// The track's folder name ("" for the flat road) and layout.
    pub track: String,
    pub layout: String,
    pub camera: OracleCamera,
    pub frames: Vec<GoldenFrame>,
    /// What the game's run was told by `getPlaybackState` / `getPaused`, in the order asked.
    pub answers: Vec<u32>,
    /// The game's log: its lines and the FNV-1a hash of its bytes.
    pub lines: u64,
    pub hash: u64,
}

const MAGIC: &[u8; 8] = b"AUGOLD01";

struct Writer(Vec<u8>);

impl Writer {
    fn u32(&mut self, v: u32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn f32(&mut self, v: f32) {
        self.u32(v.to_bits());
    }
    fn floats(&mut self, v: &[f32]) {
        for x in v {
            self.f32(*x);
        }
    }
    fn text(&mut self, v: &str) {
        self.u32(v.len() as u32);
        self.0.extend_from_slice(v.as_bytes());
    }
}

struct Reader<'a>(&'a [u8], usize);

impl Reader<'_> {
    fn u32(&mut self) -> Result<u32, String> {
        let b = self.0.get(self.1..self.1 + 4).ok_or("the golden file ends early")?;
        self.1 += 4;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }
    fn f32(&mut self) -> Result<f32, String> {
        Ok(f32::from_bits(self.u32()?))
    }
    fn floats<const N: usize>(&mut self) -> Result<[f32; N], String> {
        let mut out = [0.0f32; N];
        for x in out.iter_mut() {
            *x = self.f32()?;
        }
        Ok(out)
    }
    fn text(&mut self) -> Result<String, String> {
        let n = self.u32()? as usize;
        let b = self.0.get(self.1..self.1 + n).ok_or("the golden file ends early")?;
        self.1 += n;
        Ok(String::from_utf8_lossy(b).into_owned())
    }
}

fn write_frame(w: &mut Writer, f: &CarFrame) {
    w.floats(&f.world_matrix);
    for m in &f.tyre_matrix {
        w.floats(m);
    }
    w.floats(&[f.engine_rpm, f.gas, f.brake, f.speed]);
    w.u32(f.is_engine_limiter_on as u32 | (f.is_gear_grinding as u32) << 1 | (f.node_active as u32) << 2 | (f.tc_in_action as u32) << 3);
    w.u32(f.gear as u32);
    w.u32(f.actions_state);
    w.floats(&f.wheel_angular_speed);
    w.floats(&f.velocity);
    w.floats(&f.slip_ratio);
    w.floats(&f.nd_slip);
    w.floats(&f.load);
    w.floats(&f.tyre_dirty_level);
    for wav in &f.surface_wav {
        w.text(wav);
    }
    w.floats(&f.surface_grip_mod);
    w.floats(&[f.drivetrain_speed, f.turbo_boost, f.body_work_volume, f.air_density, f.fuel, f.engine_life_left, f.turbo_bov]);
    w.floats(&f.tyre_inflation);
    w.floats(&f.sus_damage);
    w.floats(&f.body_position);
    w.floats(&f.pit_position);
}

fn read_frame(r: &mut Reader) -> Result<CarFrame, String> {
    let world_matrix = r.floats::<16>()?;
    let tyre_matrix = [r.floats::<16>()?, r.floats::<16>()?, r.floats::<16>()?, r.floats::<16>()?];
    let [engine_rpm, gas, brake, speed] = r.floats::<4>()?;
    let flags = r.u32()?;
    let gear = r.u32()? as i32;
    let actions_state = r.u32()?;
    let wheel_angular_speed = r.floats::<4>()?;
    let velocity = r.floats::<3>()?;
    let slip_ratio = r.floats::<4>()?;
    let nd_slip = r.floats::<4>()?;
    let load = r.floats::<4>()?;
    let tyre_dirty_level = r.floats::<4>()?;
    let surface_wav = [r.text()?, r.text()?, r.text()?, r.text()?];
    let surface_grip_mod = r.floats::<4>()?;
    let [drivetrain_speed, turbo_boost, body_work_volume, air_density, fuel, engine_life_left, turbo_bov] = r.floats::<7>()?;
    Ok(CarFrame {
        world_matrix,
        tyre_matrix,
        engine_rpm,
        is_engine_limiter_on: flags & 1 != 0,
        wheel_angular_speed,
        gas,
        brake,
        gear,
        speed,
        velocity,
        slip_ratio,
        nd_slip,
        load,
        tyre_dirty_level,
        surface_wav,
        surface_grip_mod,
        drivetrain_speed,
        turbo_boost,
        is_gear_grinding: flags & 2 != 0,
        body_work_volume,
        air_density,
        fuel,
        engine_life_left,
        turbo_bov,
        actions_state,
        tyre_inflation: r.floats::<4>()?,
        sus_damage: r.floats::<4>()?,
        node_active: flags & 4 != 0,
        tc_in_action: flags & 8 != 0,
        body_position: r.floats::<3>()?,
        pit_position: r.floats::<3>()?,
    })
}

impl Golden {
    pub fn write(&self, path: &Path) -> Result<(), String> {
        let mut w = Writer(MAGIC.to_vec());
        w.text(&self.car);
        w.text(&self.track);
        w.text(&self.layout);
        w.text(self.camera.name());
        w.0.extend_from_slice(&self.lines.to_le_bytes());
        w.0.extend_from_slice(&self.hash.to_le_bytes());
        w.u32(self.answers.len() as u32);
        // an answer is a playback state (0..4) or a flag: a byte each
        w.0.extend(self.answers.iter().map(|a| *a as u8));
        w.u32(self.frames.len() as u32);
        for frame in &self.frames {
            write_frame(&mut w, &frame.car);
            w.u32(frame.events.len() as u32);
            for e in &frame.events {
                w.u32(e.kind as u32);
                w.floats(&[e.param1, e.param2, e.param3, e.param4]);
                w.floats(&e.v_param1);
                w.floats(&e.v_param2);
                w.u32(e.ul_param0 as u32);
            }
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        std::fs::write(path, w.0).map_err(|e| format!("{}: {e}", path.display()))
    }

    pub fn read(path: &Path) -> Result<Golden, String> {
        let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        if bytes.len() < 8 || &bytes[..8] != MAGIC {
            return Err(format!("{}: not an audio golden file", path.display()));
        }
        let mut r = Reader(&bytes, 8);
        let car = r.text()?;
        let track = r.text()?;
        let layout = r.text()?;
        let camera = OracleCamera::parse(&r.text()?)?;
        let lines = r.u32()? as u64 | (r.u32()? as u64) << 32;
        let hash = r.u32()? as u64 | (r.u32()? as u64) << 32;
        let count = r.u32()? as usize;
        let answers: Vec<u32> = r.0.get(r.1..r.1 + count).ok_or("the golden file ends early")?.iter().map(|b| *b as u32).collect();
        r.1 += count;
        let mut frames = Vec::new();
        for _ in 0..r.u32()? {
            let car = read_frame(&mut r)?;
            let mut events = Vec::new();
            for _ in 0..r.u32()? {
                let kind = r.u32()? as i32;
                let [param1, param2, param3, param4] = r.floats::<4>()?;
                events.push(PhysicsEvent { kind, param1, param2, param3, param4, v_param1: r.floats::<3>()?, v_param2: r.floats::<3>()?, ul_param0: r.u32()? as u64 });
            }
            frames.push(GoldenFrame { car, events });
        }
        Ok(Golden { car, track, layout, camera, frames, answers, lines, hash })
    }

    /// Runs the port on the golden drive (FMOD must be loaded and a log started with the
    /// golden's answers). `ac` is the game's folder, `audio_ini` a file with [`AUDIO_INI`].
    pub fn run(&self, ac: &Path, audio_ini: &Path) -> Result<(), String> {
        let files = EngineFiles { content_root: ac.to_path_buf(), audio_engine_ini: ac.join("system/cfg/audio_engine.ini"), audio_ini: audio_ini.to_path_buf() };
        let master = rustyac_physics::data::ini::IniReader::load(audio_ini)?.get_float("LEVELS", "MASTER")?;
        let mut scene = None;
        let mut surfaces = Vec::new();
        let mut data_folder = PathBuf::new();
        if !self.track.is_empty() {
            let folder = ac.join("content/tracks").join(&self.track);
            scene = Some(Scene::load(&folder, &self.layout)?);
            data_folder = if self.layout.is_empty() { folder } else { folder.join(&self.layout) };
            let manager = rustyac_physics::track::surfaces::SurfacesManager::new(&ac.join("system/data/surfaces.ini"), &data_folder.join("data/surfaces.ini"))?;
            surfaces = manager.surfaces.iter().map(|(key, surface)| (key.clone(), surface.wav.clone())).collect();
        }
        let car = CarInfo { unix_name: self.car.clone(), guid: 0, data_folder: ac.join("content/cars").join(&self.car).join("data"), car_cameras_external_sound: Vec::new() };
        let session = Session { files, master, track: scene.as_ref().map(|s| (s, data_folder.clone())), surfaces: &surfaces, car, camera: self.camera.view(), dt: 1.0 / 60.0 };
        let first = self.frames.first().map(|f| f.car.clone());
        let camera = self.camera;
        run_session(
            session,
            self.frames.iter().map(|f| SessionFrame { car: &f.car, events: &f.events, listener: camera.listener(&f.car, first.as_ref().unwrap_or(&f.car)) }),
        )
    }
}
