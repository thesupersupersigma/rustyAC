// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! AC's `CarAudioFMOD` (CarAudioFMOD.obj): the sounds of one car. Built once per car; every
//! picture frame it starts and stops its events, sets their volumes, places them in 3D and
//! writes the car's state into their parameters (`renderAudio`), and it answers three events of
//! its car: a collision, a gear change, a backfire (plus the down-shift protection beep).
//!
//! Everything the game reads of the car is in [`CarFrame`] (the render-side `CarPhysicsState`
//! plus a few `CarAvatar` members) and [`SimView`] (which car has the focus, which camera).

use std::ffi::CStr;
use std::path::PathBuf;

use rustyac_physics::data::ini::IniReader;

use crate::engine::{fin, AudioEngine, EventId, Mat, Vec3, REVERB_OFF, REVERB_ON};

/// What the sound reads of its car in one frame.
#[derive(Clone, Debug)]
pub struct CarFrame {
    // --- CarPhysicsState (the main thread's copy) ---
    pub world_matrix: Mat,
    pub tyre_matrix: [Mat; 4],
    pub engine_rpm: f32,
    pub is_engine_limiter_on: bool,
    pub wheel_angular_speed: [f32; 4],
    pub gas: f32,
    pub brake: f32,
    /// 0 = reverse, 1 = neutral, 2 = first ...
    pub gear: i32,
    /// m/s
    pub speed: f32,
    pub velocity: Vec3,
    pub slip_ratio: [f32; 4],
    pub nd_slip: [f32; 4],
    pub load: [f32; 4],
    pub tyre_dirty_level: [f32; 4],
    /// `tyreSurfaceDef[i].wavString` ("grass.wav"; empty on plain tarmac)
    pub surface_wav: [String; 4],
    pub surface_grip_mod: [f32; 4],
    pub drivetrain_speed: f32,
    pub turbo_boost: f32,
    pub is_gear_grinding: bool,
    pub body_work_volume: f32,
    pub air_density: f32,
    pub fuel: f32,
    pub engine_life_left: f32,
    pub turbo_bov: f32,
    /// `actionsState`: bit 6 is the horn
    pub actions_state: u32,
    pub tyre_inflation: [f32; 4],
    pub sus_damage: [f32; 4],
    // --- CarAvatar ---
    /// `carNode->isActive`: the car is in the session (connected).
    pub node_active: bool,
    /// `CarAvatar::isTcInAction`: read straight from the physics car (`tractionControl.isInAction`).
    pub tc_in_action: bool,
    /// Row 3 of `bodyMatrix` (the drawn body), for the distance to the listener.
    pub body_position: Vec3,
    /// Row 3 of `pitPosition`.
    pub pit_position: Vec3,
}

impl CarFrame {
    /// From the bytes of a `CarPhysicsState` (0xb70 of them), as the game's own
    /// `Car::getPhysicsState` fills it; the `CarAvatar` members are set for a connected car
    /// far from its pit box.
    pub fn from_physics_state(b: &[u8]) -> CarFrame {
        assert!(b.len() >= 0xb70, "a CarPhysicsState is 0xb70 bytes");
        let f = |at: usize| f32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]]);
        let i = |at: usize| i32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]]);
        let f4 = |at: usize| [f(at), f(at + 4), f(at + 8), f(at + 12)];
        let mat = |at: usize| -> Mat { std::array::from_fn(|k| f(at + 4 * k)) };
        let wav = |at: usize| -> String {
            let mut units = Vec::new();
            for k in 0..64 {
                let u = u16::from_le_bytes([b[at + 2 * k], b[at + 2 * k + 1]]);
                if u == 0 {
                    break;
                }
                units.push(u);
            }
            String::from_utf16_lossy(&units)
        };
        let world_matrix = mat(0x4);
        CarFrame {
            world_matrix,
            tyre_matrix: std::array::from_fn(|w| mat(0x144 + 0x40 * w)),
            engine_rpm: f(0x244),
            is_engine_limiter_on: b[0x248] != 0,
            wheel_angular_speed: f4(0x24c),
            gas: f(0x260),
            brake: f(0x264),
            gear: i(0x26c),
            speed: f(0x270),
            velocity: [f(0x274), f(0x278), f(0x27c)],
            slip_ratio: f4(0x2b4),
            nd_slip: f4(0x2d4),
            load: f4(0x2e4),
            tyre_dirty_level: f4(0x314),
            surface_wav: std::array::from_fn(|w| wav(0x328 + 0xc8 * w)),
            surface_grip_mod: std::array::from_fn(|w| f(0x328 + 0xc8 * w + 0x90)),
            drivetrain_speed: f(0x734),
            turbo_boost: f(0x738),
            is_gear_grinding: b[0x744] != 0,
            body_work_volume: f(0x748),
            air_density: f(0x790),
            fuel: f(0x794),
            engine_life_left: f(0x7a8),
            turbo_bov: f(0x7ac),
            actions_state: i(0x7d4) as u32,
            tyre_inflation: f4(0x7dc),
            sus_damage: f4(0x7f8),
            node_active: true,
            tc_in_action: false,
            body_position: [world_matrix[12], world_matrix[13], world_matrix[14]],
            pit_position: [0.0, -1000.0, 0.0],
        }
    }
}

/// The camera, as far as the sound asks (`ACCameraManager` and what hangs off it).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CameraView {
    /// `ACCameraManager::mode`: 0 cockpit, 1 the car's own cameras (F6), 2 the "drivable"
    /// cameras (chase, bonnet, bumper, dash), 3 track, 5 on-board free, 6 free.
    pub mode: i32,
    /// `cameraCar->currentCameraIndex`
    pub car_camera_index: i32,
    /// `cameraDrivable->currentMode`: 0 and 1 chase, 2 bonnet, 3 bumper, 4 dash.
    pub drivable_mode: i32,
    /// The focused car's `bonnetExternalSound` / `bumperExternalSound` (cameras.ini).
    pub bonnet_external_sound: bool,
    pub bumper_external_sound: bool,
}

impl CameraView {
    pub const COCKPIT: CameraView = CameraView { mode: 0, car_camera_index: 0, drivable_mode: 0, bonnet_external_sound: false, bumper_external_sound: false };
    pub const CHASE: CameraView = CameraView { mode: 2, car_camera_index: 0, drivable_mode: 0, bonnet_external_sound: false, bumper_external_sound: false };
    pub const TRACK: CameraView = CameraView { mode: 3, car_camera_index: 0, drivable_mode: 0, bonnet_external_sound: false, bumper_external_sound: false };
    pub const FREE: CameraView = CameraView { mode: 6, car_camera_index: 0, drivable_mode: 0, bonnet_external_sound: false, bumper_external_sound: false };
}

/// What the sound reads of `Sim`.
#[derive(Clone, Copy, Debug)]
pub struct SimView {
    /// `Sim::focusedCarIndex`
    pub focused_car_index: u32,
    /// `None` while the simulation has no camera manager.
    pub camera: Option<CameraView>,
}

/// What a car's sound is built from.
#[derive(Clone, Debug)]
pub struct CarInfo {
    /// `CarAvatar::unixName`: the car's folder name.
    pub unix_name: String,
    /// `CarAvatar::guid`: 0 is the player's car.
    pub guid: i32,
    /// The car's `data` folder (or where its `data.acd` is), for `engine.ini` and `sounds.ini`.
    pub data_folder: PathBuf,
    /// `externalSound` of each of the car's own cameras (`CameraCarDefinition`, car.ini).
    pub car_cameras_external_sound: Vec<bool>,
}

/// `MixVolumes` (0x24 bytes): the levels of Documents `cfg/audio.ini`.
#[derive(Clone, Copy, Debug)]
pub struct MixVolumes {
    pub wind: f32,
    pub tyres: f32,
    pub surfaces: f32,
    pub dirt: f32,
    pub engine: f32,
    pub transmission: f32,
    pub opponents: f32,
    /// 1.5 or 1.0, set by the camera (`CarAvatar::setMultVolume`).
    pub tyres_mult: f32,
    pub engine_mult: f32,
}

impl MixVolumes {
    /// `MixVolumes::MixVolumes` @ 0x140064a40
    #[allow(clippy::neg_cmp_op_on_partial_ord, clippy::float_cmp)]
    pub fn new(audio_ini: &IniReader) -> MixVolumes {
        let mut v = MixVolumes { wind: 1.0, tyres: 1.0, surfaces: 1.0, dirt: 1.0, engine: 1.0, transmission: -1.0, opponents: 1.0, tyres_mult: 1.5, engine_mult: 1.5 };
        if !audio_ini.ready {
            return v;
        }
        let clamp01 = |x: f32| {
            if x > 1.0 {
                1.0
            } else if x >= 0.0 {
                x
            } else {
                0.0
            }
        };
        let get = |key: &str| audio_ini.get_float("LEVELS", key).unwrap_or(0.0);
        v.wind = clamp01(get("WIND"));
        v.tyres = clamp01(get("TYRES"));
        v.surfaces = clamp01(get("SURFACES"));
        v.dirt = clamp01(get("DIRT_BOTTOM"));
        v.opponents = clamp01(get("OPPONENTS"));
        v.engine = clamp01(get("ENGINE"));
        if audio_ini.has_key("LEVELS", "TRANSMISSION") {
            let t = get("TRANSMISSION");
            v.transmission = if t > 1.0 {
                1.0
            } else if t >= -1.0 {
                t
            } else {
                -1.0
            };
        }
        let t = v.transmission;
        // ucomiss t,-1 ; jne: equal or unordered takes the engine's level
        v.transmission = if t < -1.0 || t > -1.0 { t * 3.0 } else { v.engine };
        v
    }
}

/// `SkidParams` (0x14 bytes)
#[derive(Clone, Copy, Debug)]
pub struct SkidParams {
    pub entry_point: f32,
    pub pitch_base: f32,
    pub pitch_gain: f32,
    pub volume_gain: f32,
    pub smooth_alpha: f32,
}

impl SkidParams {
    /// `SkidParams::SkidParams` @ 0x140065110
    pub fn new(audio_ini: &IniReader) -> SkidParams {
        let mut p = SkidParams { entry_point: 0.5, pitch_base: 0.75, pitch_gain: 0.8, volume_gain: 2.5, smooth_alpha: f32::from_bits(0x426f_ffff) };
        if audio_ini.ready && audio_ini.has_section("SKIDS") {
            p.entry_point = (audio_ini.get_float("SKIDS", "ENTRY_POINT").unwrap_or(0.0) * 0.5) * 0.01;
        }
        p
    }
}

/// `SmoothValue`: a value that follows its target at `alpha` per second.
#[derive(Clone, Copy, Debug, Default)]
pub struct SmoothValue {
    pub alpha: f32,
    pub value: f32,
}

impl SmoothValue {
    /// `SmoothValue::update` @ 0x140067eb0
    #[allow(clippy::float_cmp)]
    pub fn update(&mut self, target: f32, dt: f32) {
        if !fin(target) {
            return;
        }
        let a = self.alpha;
        // ucomiss a,0 ; je: zero or unordered
        if !(a < 0.0 || a > 0.0) || (a * dt) >= 1.0 {
            self.value = target;
        } else {
            self.value = (((target - self.value) * a) * dt) + self.value;
        }
    }
}

/// `x > 1 ? 1 : (x >= 0 ? x : 0)`; a NaN gives 0.
#[inline]
fn sat01(x: f32) -> f32 {
    if x > 1.0 {
        1.0
    } else if x >= 0.0 {
        x
    } else {
        0.0
    }
}

/// `SurfaceList`: the surface sound of one wheel.
#[derive(Clone, Debug, Default)]
struct SurfaceList {
    path: String,
    event: Option<EventId>,
}

const K75: f32 = 0.013_333_334;

/// AC's `CarAudioFMOD` (0x4b8 bytes).
pub struct CarAudio {
    info: CarInfo,
    /// `cameraManager` was fetched (the first `renderAudio` does that).
    camera_manager_known: bool,
    hit_events: [Option<EventId>; 3],
    scrape_events: [Option<EventId>; 3],
    scrape_decays: [f32; 3],
    dirt_event: EventId,
    bank_path: String,
    pub mix_volumes: MixVolumes,
    engine_ext_event: EventId,
    engine_int_event: EventId,
    gear_ext_event: EventId,
    gear_int_event: EventId,
    bodywork_event: EventId,
    wind_event: EventId,
    down_shift_event: Option<EventId>,
    horn_event: Option<EventId>,
    gear_grind_event: Option<EventId>,
    backfire_ext_event: Option<EventId>,
    backfire_int_event: Option<EventId>,
    traction_control_ext_event: Option<EventId>,
    traction_control_int_event: Option<EventId>,
    transmission_event: Option<EventId>,
    limiter_event: Option<EventId>,
    turbo_event: Option<EventId>,
    wheel_events: Vec<EventId>,
    skid_ext_events: Vec<EventId>,
    skid_int_events: Vec<EventId>,
    surface_events: [SurfaceList; 4],
    /// 0 unspecified, 1 front, 2 rear (`sounds.ini [ENGINE] POSITION`)
    engine_position: i32,
    max_turbo_boost: f32,
    pub skid_params: SkidParams,
    last_gear: i32,
    gas: SmoothValue,
    bodywork_volume: SmoothValue,
    skid_volumes: [SmoothValue; 4],
    skid_pitches: [SmoothValue; 4],
    traction_control_decay: f32,
    limiter_decay: f32,
    turbo_bov_decay: f32,
    /// Written by the simulation every frame: this car's rank by distance to the listener.
    pub listener_priority: i32,
    /// ... and the gap to the car before it in that ranking.
    pub listener_distance: f32,
    using_cache: bool,
}

impl CarAudio {
    /// The car's sound bank, relative to the game folder; a car without one has no sound object
    /// at all (`CarAvatar::initCommonPostPhysics` @ 0x1400d6190).
    pub fn bank_path(unix_name: &str) -> String {
        format!("content/cars/{unix_name}/sfx/{unix_name}.bank")
    }

    /// What `CarAvatar::initCommonPostPhysics` does for the sound: the car's own GUID file if it
    /// has one, then the sound object. `None` when the car has no bank.
    pub fn for_car(engine: &mut AudioEngine, info: CarInfo) -> Result<Option<CarAudio>, String> {
        let root = engine.files.content_root.clone();
        if !root.join(Self::bank_path(&info.unix_name)).is_file() {
            return Ok(None);
        }
        let guids = format!("content/cars/{}/sfx/GUIDs.txt", info.unix_name);
        if root.join(&guids).is_file() {
            engine.parse_guids(&guids)?;
        }
        Ok(Some(CarAudio::new(engine, info)?))
    }

    /// `CarAudioFMOD::CarAudioFMOD` @ 0x140062830
    pub fn new(engine: &mut AudioEngine, info: CarInfo) -> Result<CarAudio, String> {
        let unix = info.unix_name.clone();
        let car_event = |name: &str| format!("event:/cars/{unix}/{name}");
        // the dirt sound is in the common bank: made before the car's bank is taken
        let dirt_event = engine.create_event("event:/common/dirt", REVERB_OFF);
        let bank_path = Self::bank_path(&unix);
        engine.add_bank_ref(&bank_path);
        let audio_ini = IniReader::load(&engine.files.audio_ini)?;
        let mix_volumes = MixVolumes::new(&audio_ini);
        let engine_ext_event = engine.create_event(&car_event("engine_ext"), REVERB_ON);
        let engine_int_event = engine.create_event(&car_event("engine_int"), REVERB_OFF);
        let gear_ext_event = engine.create_event(&car_event("gear_ext"), REVERB_ON);
        let gear_int_event = engine.create_event(&car_event("gear_int"), REVERB_OFF);
        let bodywork_event = engine.create_event(&car_event("bodywork"), REVERB_OFF);
        let wind_event = engine.create_event(&car_event("wind"), REVERB_OFF);
        let skid_params = SkidParams::new(&IniReader::load(&engine.files.audio_ini)?);
        let mut a = CarAudio {
            info,
            camera_manager_known: false,
            hit_events: [None; 3],
            scrape_events: [None; 3],
            scrape_decays: [0.0; 3],
            dirt_event,
            bank_path,
            mix_volumes,
            engine_ext_event,
            engine_int_event,
            gear_ext_event,
            gear_int_event,
            bodywork_event,
            wind_event,
            down_shift_event: None,
            horn_event: None,
            gear_grind_event: None,
            backfire_ext_event: None,
            backfire_int_event: None,
            traction_control_ext_event: None,
            traction_control_int_event: None,
            transmission_event: None,
            limiter_event: None,
            turbo_event: None,
            wheel_events: Vec::new(),
            skid_ext_events: Vec::new(),
            skid_int_events: Vec::new(),
            surface_events: Default::default(),
            engine_position: 0,
            max_turbo_boost: 0.0,
            skid_params,
            last_gear: 0,
            gas: SmoothValue { alpha: 0.9, value: 0.0 },
            bodywork_volume: SmoothValue { alpha: 0.9, value: 0.0 },
            skid_volumes: [SmoothValue { alpha: 0.9, value: 0.0 }; 4],
            skid_pitches: [SmoothValue { alpha: 0.9, value: 0.0 }; 4],
            traction_control_decay: 0.0,
            limiter_decay: 10.0,
            turbo_bov_decay: 10.0,
            listener_priority: 0,
            listener_distance: 0.0,
            using_cache: false,
        };
        for (k, name) in ["car", "track", "object"].iter().enumerate() {
            let hit = format!("event:/collisions/{name}/hit");
            let scrape = format!("event:/collisions/{name}/scrape");
            if engine.has_event(&hit) {
                a.hit_events[k] = Some(engine.create_event(&hit, REVERB_OFF));
            }
            if engine.has_event(&scrape) {
                a.scrape_events[k] = Some(engine.create_event(&scrape, REVERB_OFF));
            }
            a.scrape_decays[k] = 0.0;
        }
        let optional = |engine: &mut AudioEngine, path: &str, reverb: i32| -> Option<EventId> { engine.has_event(path).then(|| engine.create_event(path, reverb)) };
        a.horn_event = optional(engine, &car_event("horn"), REVERB_OFF);
        a.down_shift_event = optional(engine, "event:/common/ds_protection", REVERB_OFF);
        a.gear_grind_event = optional(engine, &car_event("gear_grind"), REVERB_OFF);
        if engine.has_event(&car_event("backfire_ext")) && engine.has_event(&car_event("backfire_int")) {
            a.backfire_ext_event = Some(engine.create_event(&car_event("backfire_ext"), REVERB_ON));
            a.backfire_int_event = Some(engine.create_event(&car_event("backfire_int"), REVERB_OFF));
        }
        if engine.has_event(&car_event("tractioncontrol_ext")) && engine.has_event(&car_event("tractioncontrol_int")) {
            a.traction_control_ext_event = Some(engine.create_event(&car_event("tractioncontrol_ext"), REVERB_ON));
            a.traction_control_int_event = Some(engine.create_event(&car_event("tractioncontrol_int"), REVERB_OFF));
        }
        a.transmission_event = optional(engine, &car_event("transmission"), REVERB_OFF);
        a.limiter_event = optional(engine, &car_event("limiter"), REVERB_OFF);
        // the turbo: the boost of every [TURBO_n] of engine.ini added up
        {
            let ini = IniReader::load(&rustyac_physics::data::ini::append_path(&a.info.data_folder, "engine.ini"))?;
            let mut found = false;
            let mut i = 0;
            loop {
                let section = format!("TURBO_{i}");
                i += 1;
                if !ini.has_section(&section) {
                    break;
                }
                found = true;
                let v = ini.get_float(&section, "MAX_BOOST")?;
                a.max_turbo_boost += v;
            }
            if found && engine.has_event(&car_event("turbo")) {
                a.turbo_event = Some(engine.create_event(&car_event("turbo"), REVERB_OFF));
            }
        }
        for _ in 0..4 {
            a.wheel_events.push(engine.create_event(&car_event("wheel"), REVERB_OFF));
        }
        for _ in 0..4 {
            let e = engine.create_event(&car_event("skid_ext"), REVERB_ON);
            a.skid_ext_events.push(e);
            engine.event_set_base_volume(e, 0.0);
        }
        for _ in 0..4 {
            let e = engine.create_event(&car_event("skid_int"), REVERB_OFF);
            a.skid_int_events.push(e);
            engine.event_set_base_volume(e, 0.0);
        }
        {
            let ini = IniReader::load(&rustyac_physics::data::ini::append_path(&a.info.data_folder, "sounds.ini"))?;
            if ini.ready && ini.has_section("ENGINE") {
                let s: String = ini.get_string("ENGINE", "POSITION").chars().map(|c| c.to_ascii_lowercase()).collect();
                if s == "front" {
                    a.engine_position = 1;
                } else if s == "rear" {
                    a.engine_position = 2;
                }
            }
        }
        a.gas.alpha = if a.info.guid != 0 { 10.0 } else { 0.0 };
        a.bodywork_volume.alpha = f32::from_bits(0x404c_cccd);
        for i in 0..4 {
            a.skid_volumes[i].alpha = a.skid_params.smooth_alpha;
            a.skid_pitches[i].alpha = a.skid_params.smooth_alpha;
        }
        a.limiter_decay = 0.0;
        engine.update_properties();
        Ok(a)
    }

    /// `CarAudioFMOD::~CarAudioFMOD` @ 0x1400653e0
    pub fn destroy(mut self, engine: &mut AudioEngine) {
        for i in 0..4 {
            if let Some(e) = self.surface_events[i].event {
                engine.release_cached_event(e);
            }
        }
        for e in self.skid_int_events.drain(..) {
            engine.destroy_event(e);
        }
        for e in self.skid_ext_events.drain(..) {
            engine.destroy_event(e);
        }
        for e in self.wheel_events.drain(..) {
            engine.destroy_event(e);
        }
        let optional = [
            self.turbo_event,
            self.limiter_event,
            self.transmission_event,
            self.traction_control_int_event,
            self.traction_control_ext_event,
            self.backfire_int_event,
            self.backfire_ext_event,
            self.gear_grind_event,
            self.horn_event,
            self.down_shift_event,
        ];
        for e in optional.into_iter().flatten() {
            engine.destroy_event(e);
        }
        for e in [self.wind_event, self.bodywork_event, self.gear_int_event, self.gear_ext_event, self.engine_int_event, self.engine_ext_event] {
            engine.destroy_event(e);
        }
        engine.remove_bank_ref(&self.bank_path);
        engine.destroy_event(self.dirt_event);
        for k in (0..3).rev() {
            if let Some(e) = self.scrape_events[k] {
                engine.destroy_event(e);
            }
        }
        for k in (0..3).rev() {
            if let Some(e) = self.hit_events[k] {
                engine.destroy_event(e);
            }
        }
    }

    pub fn info(&self) -> &CarInfo {
        &self.info
    }

    /// Which optional events the car's bank has (for the survey).
    pub fn optional_events(&self) -> Vec<&'static str> {
        let mut names = Vec::new();
        let mut note = |on: bool, name: &'static str| {
            if on {
                names.push(name);
            }
        };
        note(self.horn_event.is_some(), "horn");
        note(self.gear_grind_event.is_some(), "gear_grind");
        note(self.backfire_ext_event.is_some(), "backfire");
        note(self.traction_control_ext_event.is_some(), "tractioncontrol");
        note(self.transmission_event.is_some(), "transmission");
        note(self.limiter_event.is_some(), "limiter");
        note(self.turbo_event.is_some(), "turbo");
        names
    }

    /// The events every car must have, with whether FMOD found them in the bank.
    pub fn core_events(&self, engine: &AudioEngine) -> Vec<(&'static str, bool)> {
        vec![
            ("engine_ext", engine.event_is_valid(self.engine_ext_event)),
            ("engine_int", engine.event_is_valid(self.engine_int_event)),
            ("gear_ext", engine.event_is_valid(self.gear_ext_event)),
            ("gear_int", engine.event_is_valid(self.gear_int_event)),
            ("bodywork", engine.event_is_valid(self.bodywork_event)),
            ("wind", engine.event_is_valid(self.wind_event)),
            ("wheel", engine.event_is_valid(self.wheel_events[0])),
            ("skid_ext", engine.event_is_valid(self.skid_ext_events[0])),
            ("skid_int", engine.event_is_valid(self.skid_int_events[0])),
        ]
    }

    fn focused(&self, sim: &SimView) -> bool {
        self.info.guid as u32 == sim.focused_car_index
    }

    fn camera<'a>(&self, sim: &'a SimView) -> Option<&'a CameraView> {
        if self.camera_manager_known {
            sim.camera.as_ref()
        } else {
            None
        }
    }

    /// `CarAudioFMOD::isExternalCamera` @ 0x1400663d0: the free or the track camera.
    fn is_external_camera(&self, sim: &SimView) -> bool {
        self.camera(sim).is_some_and(|cm| cm.mode == 6 || cm.mode == 3)
    }

    /// `CarAudioFMOD::shouldUseInternalSounds` @ 0x140067810
    pub fn should_use_internal_sounds(&self, sim: &SimView) -> bool {
        if !self.focused(sim) {
            return false;
        }
        let Some(cm) = self.camera(sim) else { return false };
        match cm.mode {
            0 => true,
            5 => false,
            // CameraDrivableManager::shouldUseInternalSounds @ 0x1400c69b0
            2 => match cm.drivable_mode {
                0 | 1 => false,
                2 => !cm.bonnet_external_sound,
                3 => !cm.bumper_external_sound,
                4 => true,
                _ => false,
            },
            1 => !self.info.car_cameras_external_sound.get(cm.car_camera_index as usize).copied().unwrap_or(false),
            _ => false,
        }
    }

    fn fade(&self) -> f32 {
        1.0 - (self.listener_distance * K75)
    }

    /// The start / resume / stop block `startStopEvents` repeats for every event.
    fn ss(engine: &AudioEngine, event: Option<EventId>, active: bool, condition: bool) {
        let Some(e) = event else { return };
        if active && condition {
            if engine.event_is_paused(e) {
                engine.event_resume(e, true);
            } else if !engine.event_is_playing(e) {
                engine.event_start(e);
            }
        } else {
            engine.event_stop(e);
        }
    }

    /// `CarAvatar::isInPit` @ 0x1400d8bd0: within 1.5 m of the pit box.
    #[allow(clippy::float_cmp)]
    pub fn is_in_pit(frame: &CarFrame) -> bool {
        let dx = frame.world_matrix[12] - frame.pit_position[0];
        let dy = frame.world_matrix[13] - frame.pit_position[1];
        let dz = frame.world_matrix[14] - frame.pit_position[2];
        let d2 = ((dy * dy) + (dx * dx)) + (dz * dz);
        let d = if d2 < 0.0 || d2 > 0.0 { d2.sqrt() } else { 0.0 };
        d < 1.5
    }

    /// `CarAudioFMOD::startStopEvents` @ 0x1400678c0
    #[allow(clippy::neg_cmp_op_on_partial_ord, clippy::float_cmp)]
    fn start_stop_events(&self, engine: &AudioEngine, frame: &CarFrame, sim: &SimView) {
        let active = frame.node_active;
        let focused = self.focused(sim);
        let internal = self.should_use_internal_sounds(sim);
        let dirt = &frame.tyre_dirty_level;
        let dirt_on = (((dirt[1] + dirt[0]) + dirt[2]) + dirt[3]) > 0.0;
        let engine_on = !(0.0 >= frame.fuel) && !(0.0 >= frame.engine_life_left);
        let in_pit = Self::is_in_pit(frame);
        let rv = engine.reverb_value;
        let rev = focused && internal && (rv < 0.0 || rv > 0.0);
        let near7 = self.listener_priority < 7;

        Self::ss(engine, Some(self.dirt_event), active, focused && internal && dirt_on);
        Self::ss(engine, Some(self.bodywork_event), active, focused && internal && !in_pit);
        Self::ss(engine, Some(self.wind_event), active, focused);
        let horn = (frame.actions_state >> 6) & 1 != 0 && self.horn_event.is_some_and(|e| engine.event_is_within_range(e)) && near7;
        Self::ss(engine, self.horn_event, active, horn);
        Self::ss(engine, self.gear_grind_event, active, focused && frame.is_gear_grinding);
        Self::ss(engine, self.traction_control_ext_event, active, !internal && (10.0 >= self.traction_control_decay) && near7);
        Self::ss(engine, self.traction_control_int_event, active, focused && internal && (10.0 >= self.traction_control_decay));
        Self::ss(engine, self.transmission_event, active, focused && internal);
        let engine_ext = engine_on && (!internal || rev) && engine.event_is_within_range(self.engine_ext_event) && near7;
        Self::ss(engine, Some(self.engine_ext_event), active, engine_ext);
        Self::ss(engine, Some(self.engine_int_event), active, focused && engine_on && internal);
        Self::ss(engine, self.turbo_event, active, self.listener_priority < 6);
        for &e in &self.wheel_events {
            Self::ss(engine, Some(e), active, focused);
        }
        Self::ss(engine, self.limiter_event, active, (10.0 >= self.limiter_decay) && near7);
    }

    /// `CarAudioFMOD::setEventVolumes` @ 0x140067330
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    fn set_event_volumes(&self, engine: &mut AudioEngine, sim: &SimView) {
        let mv = &self.mix_volumes;
        let prio = self.listener_priority;
        let u = |x: i32| (prio.wrapping_sub(x) as u32) <= 1;
        let focused = self.focused(sim);
        let ext = self.is_external_camera(sim);
        let m = if ext { 2.5 } else { mv.engine_mult };
        let e = (if !focused && !ext { mv.opponents } else { mv.engine }) * m;
        let mut horn_v = e;
        if !focused {
            horn_v *= mv.opponents;
        }
        if u(5) {
            horn_v *= self.fade();
        }
        let bf_v = if u(5) { self.fade() * e } else { e };
        let mut surf_v = if focused { mv.surfaces } else { mv.opponents * 0.3 };
        if ext {
            surf_v = mv.surfaces;
            if u(3) {
                surf_v *= self.fade();
            }
        }
        let mut hit_v = mv.surfaces;
        if u(5) {
            hit_v *= self.fade();
        }
        let trans_v = m * mv.transmission;
        let eng_v = if u(5) { self.fade() * e } else { e };
        let turbo_v = if u(4) { self.fade() * e } else { e };

        engine.event_set_base_volume(self.dirt_event, mv.dirt);
        if let Some(ev) = self.down_shift_event {
            engine.event_set_base_volume(ev, e);
        }
        let mut r = engine.reverb_value;
        if !((r as f64) >= 0.05) {
            r = 0.0;
        }
        let mut v = eng_v;
        if focused && self.should_use_internal_sounds(sim) && r >= 0.0 {
            v = eng_v * r;
        }
        engine.event_set_base_volume(self.engine_ext_event, v);
        engine.event_set_base_volume(self.engine_int_event, e);
        engine.event_set_base_volume(self.gear_ext_event, e);
        engine.event_set_base_volume(self.gear_int_event, e);
        engine.event_set_base_volume(self.bodywork_event, e);
        if let (Some(tc_ext), Some(tc_int)) = (self.traction_control_ext_event, self.traction_control_int_event) {
            engine.event_set_base_volume(tc_ext, e);
            engine.event_set_base_volume(tc_int, e);
        }
        engine.event_set_base_volume(self.wind_event, mv.wind);
        if let Some(ev) = self.horn_event {
            engine.event_set_base_volume(ev, horn_v);
        }
        if let (Some(bf_ext), Some(bf_int)) = (self.backfire_ext_event, self.backfire_int_event) {
            engine.event_set_base_volume(bf_ext, bf_v);
            engine.event_set_base_volume(bf_int, bf_v);
        }
        if let Some(ev) = self.transmission_event {
            engine.event_set_base_volume(ev, trans_v);
        }
        if let Some(ev) = self.limiter_event {
            engine.event_set_base_volume(ev, eng_v);
        }
        if let Some(ev) = self.turbo_event {
            engine.event_set_base_volume(ev, turbo_v);
        }
        for &ev in &self.wheel_events {
            engine.event_set_base_volume(ev, mv.tyres);
        }
        for surface in &self.surface_events {
            if let Some(ev) = surface.event {
                engine.event_set_base_volume(ev, surf_v);
            }
        }
        for i in 0..3 {
            let own = if focused { 1.0 } else { mv.opponents };
            if let Some(ev) = self.hit_events[i] {
                engine.event_set_base_volume(ev, own * hit_v);
            }
            if let Some(ev) = self.scrape_events[i] {
                engine.event_set_base_volume(ev, own * hit_v);
            }
        }
    }

    /// `CarAudioFMOD::computeTransforms` @ 0x140065f20: where the body, the engine, the exhaust
    /// and the wind sound are.
    fn compute_transforms(&self, engine: &AudioEngine, frame: &CarFrame, sim: &SimView) -> (Mat, Mat, Mat, Mat) {
        let cockpit = self.focused(sim) && self.camera(sim).is_some_and(|cm| cm.mode == 0);
        let body = frame.world_matrix;
        let fwd = [body[8], body[9], body[10]];
        let with_pos = |p: [f32; 3]| {
            let mut m = body;
            m[12] = p[0];
            m[13] = p[1];
            m[14] = p[2];
            m
        };
        if cockpit {
            let l = engine.listener_position();
            let ahead: [f32; 3] = std::array::from_fn(|c| (fwd[c] * 0.5) + l[c]);
            let behind: [f32; 3] = std::array::from_fn(|c| (fwd[c] * -0.5) + l[c]);
            let engine_m = match self.engine_position {
                1 => with_pos(ahead),
                2 => with_pos(behind),
                _ => with_pos(l),
            };
            (body, engine_m, with_pos(behind), with_pos(ahead))
        } else {
            let pos = |w: usize, c: usize| frame.tyre_matrix[w][12 + c];
            let f: [f32; 3] = std::array::from_fn(|c| (pos(0, c) + pos(1, c)) * 0.5);
            let r: [f32; 3] = std::array::from_fn(|c| (pos(2, c) + pos(3, c)) * 0.5);
            let engine_m = match self.engine_position {
                1 => with_pos(f),
                2 => with_pos(std::array::from_fn(|c| (fwd[c] * 0.5) + r[c])),
                _ => body,
            };
            let exhaust = with_pos(std::array::from_fn(|c| (fwd[c] * -0.5) + r[c]));
            // the wind is built from the rear axle too
            let wind = with_pos(std::array::from_fn(|c| (fwd[c] * 0.5) + r[c]));
            (body, engine_m, exhaust, wind)
        }
    }

    /// `CarAudioFMOD::renderAudio` @ 0x1400669b0
    #[allow(clippy::neg_cmp_op_on_partial_ord, clippy::float_cmp)]
    pub fn render_audio(&mut self, engine: &mut AudioEngine, frame: &CarFrame, sim: &SimView, dt: f32) {
        if !self.camera_manager_known {
            self.camera_manager_known = sim.camera.is_some();
        }
        self.start_stop_events(engine, frame, sim);
        self.set_event_volumes(engine, sim);
        if !frame.node_active {
            if self.using_cache {
                for i in 0..4 {
                    if let Some(e) = self.surface_events[i].event {
                        // the slot keeps the event: the game does not clear it here
                        engine.release_cached_event(e);
                    }
                }
                self.using_cache = false;
            }
            return;
        }
        let _listener = engine.listener_position();
        let mut vel = frame.velocity;
        if !fin(vel[0]) || !fin(vel[1]) || !fin(vel[2]) {
            vel = [0.0; 3];
        }
        let mut kmh = frame.speed * 3.6;
        if !fin(kmh) {
            kmh = 0.0;
        }
        let (t_body, t_engine, t_exhaust, t_wind) = self.compute_transforms(engine, frame, sim);
        self.gas.update(frame.gas, dt);
        self.traction_control_decay += dt;
        if frame.tc_in_action {
            self.traction_control_decay = 0.0;
        }
        for i in 0..3 {
            let Some(ev) = self.scrape_events[i] else { continue };
            let d = dt + self.scrape_decays[i];
            self.scrape_decays[i] = d;
            if d > 1.0 {
                engine.event_stop(ev);
            } else {
                engine.event_set_parameter(ev, c"decay", d);
                engine.event_set_parameter(ev, c"speed", kmh);
                if !engine.event_is_playing(ev) {
                    engine.event_start(ev);
                }
            }
        }
        let dl = &frame.tyre_dirty_level;
        let dirt = sat01((((dl[0] + dl[1]) + dl[2]) + dl[3]) * 0.1);
        engine.event_set_3d(self.dirt_event, &t_body, &vel);
        engine.event_set_parameter(self.dirt_event, c"dirtiness", dirt);
        engine.event_set_parameter(self.dirt_event, c"speed", kmh);

        // comiss 1.0,rpm ; cmovbe: a NaN is passed on (and refused by setParameter)
        let rpm = if !(1.0 > frame.engine_rpm) { frame.engine_rpm } else { 1.0 };
        engine.event_set_3d(self.engine_ext_event, &t_engine, &vel);
        engine.event_set_parameter(self.engine_ext_event, c"rpms", rpm);
        engine.event_set_parameter(self.engine_ext_event, c"throttle", self.gas.value);
        engine.event_set_3d(self.engine_int_event, &t_engine, &vel);
        engine.event_set_parameter(self.engine_int_event, c"rpms", rpm);
        engine.event_set_parameter(self.engine_int_event, c"throttle", self.gas.value);
        engine.event_set_3d(self.gear_int_event, &t_body, &vel);
        engine.event_set_3d(self.gear_ext_event, &t_body, &vel);

        self.bodywork_volume.update(frame.body_work_volume, dt);
        self.bodywork_volume.value = sat01(self.bodywork_volume.value);
        engine.event_set_3d(self.bodywork_event, &t_body, &vel);
        engine.event_set_parameter(self.bodywork_event, c"susp_travel_speed", self.bodywork_volume.value);

        engine.event_set_3d(self.wind_event, &t_wind, &vel);
        engine.event_set_parameter(self.wind_event, c"speed", kmh);
        let mut air = frame.air_density;
        if !(air < 0.0 || air > 0.0) {
            air = 1.2102;
        }
        engine.event_set_parameter(self.wind_event, c"air_pressure", air);

        if let Some(ev) = self.horn_event {
            engine.event_set_3d(ev, &t_body, &vel);
        }
        if let Some(ev) = self.gear_grind_event {
            engine.event_set_3d(ev, &t_body, &vel);
        }
        if let Some(ev) = self.down_shift_event {
            engine.event_set_3d_at_listener(ev);
        }
        if let (Some(bf_ext), Some(bf_int)) = (self.backfire_ext_event, self.backfire_int_event) {
            engine.event_set_3d(bf_int, &t_exhaust, &vel);
            engine.event_set_parameter(bf_int, c"throttle", self.gas.value);
            engine.event_set_3d(bf_ext, &t_exhaust, &vel);
            engine.event_set_parameter(bf_ext, c"throttle", self.gas.value);
        }
        if let (Some(tc_ext), Some(tc_int)) = (self.traction_control_ext_event, self.traction_control_int_event) {
            engine.event_set_3d(tc_ext, &t_engine, &vel);
            engine.event_set_parameter(tc_ext, c"decay", self.traction_control_decay);
            engine.event_set_3d(tc_int, &t_engine, &vel);
            engine.event_set_parameter(tc_int, c"decay", self.traction_control_decay);
        }
        if let Some(ev) = self.transmission_event {
            engine.event_set_3d(ev, &t_engine, &vel);
            engine.event_set_parameter(ev, c"drivetrain_speed", frame.drivetrain_speed);
            engine.event_set_parameter(ev, c"throttle", self.gas.value);
        }
        if let Some(ev) = self.limiter_event {
            self.limiter_decay += dt;
            if frame.is_engine_limiter_on {
                self.limiter_decay = 0.0;
            }
            engine.event_set_3d(ev, &t_engine, &vel);
            engine.event_set_parameter(ev, c"decay", self.limiter_decay);
        }
        if let Some(ev) = self.turbo_event {
            self.turbo_bov_decay += dt;
            if !(0.0 >= frame.turbo_bov) {
                self.turbo_bov_decay = 0.0;
            }
            engine.event_set_3d(ev, &t_engine, &vel);
            engine.event_set_parameter(ev, c"boost", frame.turbo_boost / self.max_turbo_boost);
            engine.event_set_parameter(ev, c"bov", frame.turbo_bov);
            engine.event_set_parameter(ev, c"bov_decay", self.turbo_bov_decay);
        }
        self.update_wheels(engine, frame, &vel);
        self.update_skids(engine, frame, sim, &vel, dt);
        self.update_surfaces(engine, frame, sim, &vel);
    }

    /// `CarAudioFMOD::updateWheels` @ 0x140068730
    fn update_wheels(&self, engine: &AudioEngine, frame: &CarFrame, vel: &Vec3) {
        for i in 0..4 {
            let ev = self.wheel_events[i];
            engine.event_set_3d(ev, &frame.tyre_matrix[i], vel);
            engine.event_set_parameter(ev, c"brake", frame.brake);
            let mut k = frame.speed * 3.6;
            if !fin(k) {
                k = 0.0;
            }
            engine.event_set_parameter(ev, c"speed", k);
            engine.event_set_parameter(ev, c"inflation", frame.tyre_inflation[i]);
            engine.event_set_parameter(ev, c"suspension_damage", frame.sus_damage[i]);
        }
    }

    /// `CarAudioFMOD::updateSkids` @ 0x140067f30: volume and pitch of the four skid sounds,
    /// computed here (the events have no parameter).
    #[allow(clippy::neg_cmp_op_on_partial_ord, clippy::float_cmp)]
    fn update_skids(&mut self, engine: &mut AudioEngine, frame: &CarFrame, sim: &SimView, vel: &Vec3, dt: f32) {
        let internal = self.should_use_internal_sounds(sim);
        let focused = self.focused(sim);
        let near4 = self.listener_priority < 4;
        let load = &frame.load;
        let mut avg = (((load[1] + load[0]) + load[2]) + load[3]) * 0.125;
        if !(avg < 0.0 || avg > 0.0) {
            avg = 0.01;
        }
        for i in 0..4 {
            let mut pitch = 0.0f32;
            let mut vol = 0.0f32;
            if !(0.9 > frame.surface_grip_mod[i]) {
                let s = frame.speed;
                let lock = (s > 1.0 || frame.wheel_angular_speed[i].abs() > 8.0) && frame.slip_ratio[i].abs() > 0.8;
                if lock {
                    pitch = 1.2;
                    vol = s * 3.6;
                    if !fin(vol) {
                        vol = 0.0;
                    }
                    vol = sat01(vol * 0.05);
                } else if s > 1.0 {
                    let nd = frame.nd_slip[i];
                    let ep = self.skid_params.entry_point;
                    if nd >= ep {
                        let t = sat01((nd - ep) * 0.166_666_67);
                        let lf = load[i] / avg;
                        let lfc = if !(lf > 0.2) { 0.2 } else { lf };
                        vol = sat01((t * self.skid_params.volume_gain) * lfc);
                        let p = (t * self.skid_params.pitch_gain) + self.skid_params.pitch_base;
                        pitch = if p > 1.2 {
                            1.2
                        } else if !(p >= self.skid_params.pitch_base) {
                            self.skid_params.pitch_base
                        } else {
                            p
                        };
                    }
                }
            }
            pitch = if !(pitch > 0.1) { 0.1 } else { pitch };
            self.skid_pitches[i].update(pitch, dt);
            self.skid_volumes[i].update(vol, dt);
            let ev = if internal { self.skid_int_events[i] } else { self.skid_ext_events[i] };
            if near4 {
                if !engine.event_is_playing(ev) {
                    engine.event_start(ev);
                }
            } else {
                self.skid_volumes[i].value = 0.0;
                engine.event_stop(ev);
            }
            let mut m = 0.6f32;
            if self.is_external_camera(sim) {
                m = 1.0;
            }
            if focused {
                m = 1.0;
            } else {
                m *= self.mix_volumes.opponents;
            }
            let mut v = (self.skid_volumes[i].value * self.mix_volumes.tyres) * m;
            if !internal {
                v *= self.mix_volumes.tyres_mult;
            }
            if (self.listener_priority.wrapping_sub(2) as u32) <= 1 {
                v = self.fade() * v;
            }
            engine.event_set_base_volume(ev, v);
            engine.event_set_base_pitch(ev, self.skid_pitches[i].value);
            engine.event_set_3d(ev, &frame.tyre_matrix[i], vel);
        }
        let other = if internal { &self.skid_ext_events } else { &self.skid_int_events };
        for &ev in other {
            engine.event_stop(ev);
        }
    }

    /// `CarAudioFMOD::updateSurfaces` @ 0x140068450: one pooled surface sound per wheel, named
    /// after the surface's wav.
    fn update_surfaces(&mut self, engine: &mut AudioEngine, frame: &CarFrame, sim: &SimView, vel: &Vec3) {
        let near3 = self.listener_priority < 3;
        let ext = self.is_external_camera(sim);
        let play = (!ext && self.focused(sim)) || near3;
        if !play {
            for i in 0..4 {
                if let Some(e) = self.surface_events[i].event.take() {
                    engine.release_cached_event(e);
                }
            }
            return;
        }
        for i in 0..4 {
            let name = &frame.surface_wav[i];
            if !name.is_empty() {
                // the last four characters (".wav") are dropped
                let count = name.chars().count();
                let stem: String = name.chars().take(count.saturating_sub(4)).collect();
                let path = format!("event:/surfaces/{stem}");
                self.play_surface(engine, frame, i, path, vel);
            } else if let Some(e) = self.surface_events[i].event.take() {
                engine.release_cached_event(e);
            }
        }
    }

    /// `CarAudioFMOD::playSurface` @ 0x140066700
    fn play_surface(&mut self, engine: &mut AudioEngine, frame: &CarFrame, i: usize, path: String, vel: &Vec3) {
        if self.surface_events[i].event.is_some() && self.surface_events[i].path != path {
            let e = self.surface_events[i].event.take().unwrap();
            engine.release_cached_event(e);
        }
        if self.surface_events[i].event.is_none() {
            self.surface_events[i].event = engine.get_cached_event(&path);
            self.surface_events[i].path = path;
            self.using_cache = true;
        }
        if let Some(e) = self.surface_events[i].event {
            if !engine.event_is_playing(e) {
                engine.event_start(e);
            }
            engine.event_set_3d(e, &frame.tyre_matrix[i], vel);
            let mut k = frame.speed * 3.6;
            if !fin(k) {
                k = 0.0;
            }
            engine.event_set_parameter(e, c"speed", k);
        }
    }

    /// `CarAudioFMOD::onCarHit` @ 0x140066480. `rel_speed` is what the physics event carries
    /// (already km/h; the game multiplies by 3.6 once more), `category` the collider's group.
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    pub fn on_car_hit(&mut self, engine: &AudioEngine, pos: &Vec3, rel_speed: f32, category: u64) {
        let speed = rel_speed * 3.6;
        if !(speed >= 2.0) {
            return;
        }
        let m: Mat = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, pos[0], pos[1], pos[2], 1.0];
        let vel = [0.0f32; 3];
        // an `unsigned long`: 32 bits
        let idx = match category as u32 {
            1 => 1,
            4 | 8 => 0,
            16 => 2,
            _ => 1,
        };
        let near = self.listener_priority < 7;
        if let Some(h) = self.hit_events[idx] {
            if !engine.event_is_playing(h) {
                engine.event_set_3d(h, &m, &vel);
                engine.event_set_parameter(h, c"impact_speed", speed);
                if !engine.event_is_playing(h) && near {
                    engine.event_start(h);
                }
            }
        }
        if let Some(s) = self.scrape_events[idx] {
            if !engine.event_is_playing(s) && near {
                engine.event_start(s);
            }
            if engine.event_is_paused(s) && near {
                engine.event_resume(s, true);
            }
            engine.event_set_3d(s, &m, &vel);
            self.scrape_decays[idx] = 0.0;
        }
    }

    /// The handler of the car's gear-changed event (lambda @ 0x140065910) with
    /// `CarAudioFMOD::onGearChanged` @ 0x140066670. `current` is the new gear index as a float.
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    pub fn on_gear_event(&mut self, engine: &AudioEngine, sim: &SimView, current: f32) {
        if current as i32 == 1 {
            // into neutral: nothing, and the last gear is not noted
            return;
        }
        let last = self.last_gear as f32;
        let is_down = !(current > last);
        let ev = if self.should_use_internal_sounds(sim) { self.gear_int_event } else { self.gear_ext_event };
        let near = self.listener_priority < 7;
        if !engine.event_is_playing(ev) && near {
            engine.event_stop(ev);
            let state: &CStr = c"state";
            engine.event_set_parameter(ev, state, if is_down { 0.0 } else { 1.0 });
            engine.event_start(ev);
        }
        self.last_gear = current as i32;
    }

    /// The handler of the car's backfire event (lambda @ 0x140065750).
    pub fn on_backfire(&self, engine: &AudioEngine, sim: &SimView) {
        let (Some(ext), Some(int)) = (self.backfire_ext_event, self.backfire_int_event) else { return };
        let near = self.listener_priority < 7;
        if engine.event_is_playing(ext) {
            return;
        }
        if engine.event_is_playing(int) {
            return;
        }
        if !near {
            return;
        }
        if self.should_use_internal_sounds(sim) {
            engine.event_start(int);
        } else {
            engine.event_start(ext);
        }
    }

    /// The handler of the down-shift protection event (lambda @ 0x1400658a0): a beep for the
    /// focused car. (The game does not test that the event exists.)
    pub fn on_downshift_protection(&self, engine: &AudioEngine, sim: &SimView) {
        let Some(ev) = self.down_shift_event else { return };
        let focused = self.focused(sim);
        if !engine.event_is_playing(ev) && focused {
            engine.event_start(ev);
        }
    }
}
