// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! The car's 3D model and its moving nodes: `CarAvatar::init3D` 0x1400d3b90,
//! `CarAvatar::makeBodyMatrix` 0x1400d8ec0, the steering wheel of `CarAvatar::update`
//! 0x1400db830, `CarLodManager` (`loadLod` 0x1400e4370, `update` 0x1400e57c0,
//! `updateLodVisibility` 0x1400e5810), `SuspensionAvatar` (`addModel` 0x1401b32b0, `update`
//! 0x1401b3840). Cars with animated suspensions are in [`crate::animator`].
//!
//! Not here yet (Task 21): the driver, lights, blurred rims, brake-disc glow, damage, dirt,
//! mirrors. Their meshes are drawn as the model file has them.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use rustyac_physics::data::ini::IniReader;
use rustyac_physics::vecmath::{xm_matrix_multiply, Mat44f, Vec3f};

use crate::animator::SuspensionAnimator;
use crate::blur::{BlurredObjects, TyreBlur};
use crate::camera::DEG_TO_RAD;
use crate::cockpit::{AnalogInstruments, CarAnimations, DriverShiftPositions, GearShiftShake, RotatingObjects};
use crate::constrained::ConstrainedObjectsManager;
use crate::damage::VisualDamageManager;
use crate::digital::{DigitalFrame, DigitalInstruments};
use crate::driver::{DriverFrame, DriverModel};
use crate::fake_shadow::CarFakeShadow;
use crate::graphics::Graphics;
use crate::lights::{AnimatedLights, BrakeDiscGraphics, CarBrakeLights, DynamicCarEffects};
use crate::model::Kn5Io;
use crate::scene::{NodeId, NodeKind, Scene};
use crate::flames::{BackfireParams, Flames};
use crate::particles::{EngineSmoke, SmokeFrame, TyreSmoke};
use crate::skid::CarSkidMarks;

pub use crate::state::CarPhysicsState;

pub const WHEEL_NAMES: [&str; 4] = ["WHEEL_LF", "WHEEL_RF", "WHEEL_LR", "WHEEL_RR"];
pub const SUS_NAMES: [&str; 4] = ["SUSP_LF", "SUSP_RF", "SUSP_LR", "SUSP_RR"];
pub const DISC_NAMES: [&str; 4] = ["DISC_LF", "DISC_RF", "DISC_LR", "DISC_RR"];

/// `CarLodDef` (0x68 bytes).
pub struct CarLodDef {
    pub filename: String,
    pub lod_in: f32,
    pub lod_out: f32,
    pub index: i32,
    /// the nodes switched with the level: its root and what was moved out from under it
    pub nodes: Vec<NodeId>,
    pub cockpit_hr: Option<NodeId>,
    pub cockpit_lr: Option<NodeId>,
    pub steer_hr: Option<NodeId>,
    pub steer_lr: Option<NodeId>,
}

/// `SuspensionAvatar` (0xe0 bytes): wheels, hubs and discs take world matrices from the physics.
pub struct SuspensionAvatar {
    pub wheel_transforms: [NodeId; 4],
    pub sus_transforms: [NodeId; 4],
    pub disc_transforms: [NodeId; 4],
    pub rear_axle: Option<NodeId>,
}

pub enum Suspension {
    Avatar(SuspensionAvatar),
    Animator(Box<SuspensionAnimator>),
}

/// What the car's objects read of the camera manager and the session every frame.
#[derive(Clone, Copy, Debug)]
pub struct ViewState {
    /// `ACCameraManager::mode`: 0 cockpit, 2 drivable (chase, bonnet …), 3 track side
    pub camera_mode: i32,
    /// `CameraDrivableManager::currentMode`
    pub drivable_mode: i32,
    /// `Sim::focusedCarIndex`
    pub focused_car_index: i32,
    /// the scene camera's position
    pub camera_position: [f32; 3],
    /// the scene camera's matrix (`Sim::sceneCamera`)
    pub camera_matrix: Mat44f,
    /// `Sim::useProView`
    pub use_pro_view: bool,
}

impl Default for ViewState {
    fn default() -> ViewState {
        ViewState { camera_mode: 2, drivable_mode: 0, focused_car_index: 0, camera_position: [0.0; 3], camera_matrix: Mat44f::IDENTITY, use_pro_view: false }
    }
}

/// The nodes of `Sim::initSceneGraph` 0x140199d70 the car's objects hang things on.
#[derive(Clone, Copy, Debug)]
pub struct SimNodes {
    pub root: NodeId,
    pub cars: NodeId,
    pub skid_marks: NodeId,
    pub particles: NodeId,
    pub car_shadows: NodeId,
    pub before_cars: NodeId,
    pub render_finished: NodeId,
}

/// The graphics side of `CarAvatar` with its `CarLodManager`.
pub struct CarAvatar {
    /// the car's folder: `<game>/content/cars/<car>`
    pub folder: PathBuf,
    pub car_node: NodeId,
    pub body_transform: NodeId,
    pub steer_transform_hr: NodeId,
    pub org_steer_matrix: Mat44f,
    pub body_matrix: Mat44f,
    pub graphics_offset: [f32; 3],
    /// radians
    pub graphics_pitch_rotation: f32,
    pub graphic_steer_lock_degrees: f32,
    /// `Car::steerLock`; `None` for a car without physics
    pub steer_lock: Option<f32>,
    pub lods: Vec<CarLodDef>,
    pub current_lod: i32,
    pub cockpit_hr_distance: f32,
    pub driver_hr_distance: f32,
    pub pro_view_nodes: Vec<NodeId>,
    pub suspension: Suspension,
    /// `driverModel_HR` and `driverModel_LR`
    pub driver_hr: Option<DriverModel>,
    pub driver_lr: Option<DriverModel>,
    /// `isDriverHR`: written after the updates, read by the next frame's
    pub is_driver_hr: bool,
    /// 0: the player's car
    pub guid: i32,
    /// `video.ini [ASSETTOCORSA] LOCK_STEER`, `HIDE_ARMS`, `HIDE_STEER`
    pub lock_virtual_steer: bool,
    pub hide_arms_in_cockpit: bool,
    pub hide_steer: bool,
    /// `CarPhysicsInfo::maxGear`
    pub max_gear: i32,
    /// what the car's objects read of the cameras and the session in a frame
    pub view: ViewState,
    /// `ConstrainedObjectsManager`
    pub constrained: ConstrainedObjectsManager,
    /// `VisualDamageManager`
    pub damage: Option<VisualDamageManager>,
    /// the pause menu shows (the damage's parts stop shaking)
    pub pause_menu: bool,
    /// `RotatingObjects`, `CarAnimations`, `GearShiftShake`, `AnalogInstruments`
    pub rotating_objects: RotatingObjects,
    pub car_animations: CarAnimations,
    pub gear_shift_shake: Option<GearShiftShake>,
    pub analog_instruments: AnalogInstruments,
    /// `DigitalInstruments`
    pub digital_instruments: DigitalInstruments,
    /// `Game::gameTime.now`, milliseconds: the clock the dashboard's lights blink by
    pub game_time_ms: f64,
    /// the aids' levels as the displays show them, the air's temperature, miles for km
    pub tc_level: u32,
    pub abs_level: u32,
    pub ambient_temperature: f32,
    pub use_mph: bool,
    /// `car.ini [GRAPHICS] DRIVEREYES`
    pub driver_eyes_position: [f32; 3],
    /// `CarAvatar::wingsStatus[i].angle`: each wing's angle, degrees
    pub wing_angles: Vec<f32>,
    /// the skid marks: `CarAvatar::skidMarkBuffers`
    pub skid_marks: CarSkidMarks,
    /// `TyreSmoke` of each wheel, `EngineSmoke`
    pub tyre_smoke: Vec<TyreSmoke>,
    pub engine_smoke: Option<EngineSmoke>,
    /// `RaceManager::carsToBeLoaded`
    pub cars_to_be_loaded: i32,
    /// `car.ini [GRAPHICS] MIRROR_POSITION`, `CarMirrorManager`
    pub mirror_position: [f32; 3],
    pub mirrors: crate::mirror::CarMirrorManager,
    /// `CarAvatar::backfireParams`, `fuelInExhaust`, the `Flames` object, `aiState.isActive`
    pub backfire: Option<BackfireParams>,
    pub fuel_in_exhaust: f32,
    pub flames: Option<Rc<RefCell<Flames>>>,
    pub ai_active: bool,
    /// a backfire fired in the last `update` (`evOnBackfireTriggered`)
    pub backfire_triggered: bool,
    /// `CarBrakeLights`, `BrakeDiscGraphics`, `DynamicCarEffects`
    pub brake_lights: CarBrakeLights,
    pub animated_lights: AnimatedLights,
    pub brake_discs: BrakeDiscGraphics,
    pub dynamic_effects: DynamicCarEffects,
    /// `CarAvatar::inPitlane`: a tyre stands on a pit-lane surface
    pub in_pitlane: bool,
    /// a replay plays
    pub replay_mode: bool,
    /// `TyreBlur` and `BlurredObjects`, made by [`CarAvatar::init_common_post_physics`]
    pub tyre_blur: TyreBlur,
    pub blurred_objects: BlurredObjects,
    /// `ReplayManager::timeMult` while a replay plays, else 1
    pub replay_scale: f32,
    /// `CarFakeShadow`, made by [`CarAvatar::on_post_load`]
    pub fake_shadow: Option<Rc<RefCell<CarFakeShadow>>>,
    /// the car's folder as its textures are named by
    pub folder_text: String,
}

fn ini(folder: &Path, name: &str) -> Option<IniReader> {
    IniReader::load(&folder.join("data").join(name)).ok()
}

fn get_float(ini: &Option<IniReader>, section: &str, key: &str) -> f32 {
    ini.as_ref().and_then(|i| i.get_float(section, key).ok()).unwrap_or(0.0)
}

impl CarAvatar {
    /// `CarAvatar::init3D` 0x1400d3b90 with what `initCommon` 0x1400d56e0 read before it.
    /// `folder` is the car's folder as text (its texture keys are made of it) and on disk.
    pub fn init_3d(graphics: &mut Graphics, scene: &mut Scene, cars_node: NodeId, folder_text: &str, folder: &Path, skin: &str, steer_lock: Option<f32>, guid: i32) -> Result<CarAvatar, String> {
        let car_ini = ini(folder, "car.ini");
        // initCommon: [BASIC] GRAPHICS_OFFSET, GRAPHICS_PITCH_ROTATION
        let graphics_offset = car_ini.as_ref().and_then(|i| i.get_float3("BASIC", "GRAPHICS_OFFSET").ok()).unwrap_or([0.0; 3]);
        let graphics_pitch_rotation = get_float(&car_ini, "BASIC", "GRAPHICS_PITCH_ROTATION") * DEG_TO_RAD;

        let car_node = scene.bounding_sphere("CARNODE", 4.0);
        scene.add_child(cars_node, car_node);
        let body_transform = scene.node("BODYTR");
        // CarAvatar::initDriver 0x1400d7380: the two driver models, the first children of BODYTR
        let (mut driver_hr, mut driver_lr) = (None, None);
        if folder.join("data/driver3d.ini").is_file() || rustyac_physics::data::ini::IniReader::load(&folder.join("data/driver3d.ini")).is_ok() {
            driver_hr = DriverModel::new(graphics, scene, folder_text, folder, skin, guid, true)?;
            if let Some(d) = &driver_hr {
                scene.add_child(body_transform, d.driver_root);
            }
            driver_lr = DriverModel::new(graphics, scene, folder_text, folder, skin, guid, false)?;
            if let Some(d) = &driver_lr {
                scene.add_child(body_transform, d.driver_root);
            }
        }
        let animated = car_ini.as_ref().and_then(|i| i.get_int("GRAPHICS", "USE_ANIMATED_SUSPENSIONS").ok()).unwrap_or(0) != 0;
        let suspension = if animated {
            println!("USING ANIMATED SUSPENSIONS");
            Suspension::Animator(Box::new(SuspensionAnimator::new(folder)))
        } else {
            println!("USING PHYSICS DRIVEN SUSPENSIONS");
            // SuspensionAvatar::SuspensionAvatar 0x1401b2fe0: its twelve nodes, before BODYTR
            let mut avatar = SuspensionAvatar { wheel_transforms: [0; 4], sus_transforms: [0; 4], disc_transforms: [0; 4], rear_axle: None };
            for i in 0..4 {
                avatar.wheel_transforms[i] = scene.node("WHEEL_TR");
                scene.add_child(car_node, avatar.wheel_transforms[i]);
                avatar.sus_transforms[i] = scene.node("SUS_TRANSFORM");
                scene.add_child(car_node, avatar.sus_transforms[i]);
                avatar.disc_transforms[i] = scene.node("DISC_TRANSFORM");
                scene.add_child(car_node, avatar.disc_transforms[i]);
            }
            Suspension::Avatar(avatar)
        };
        let steer_transform_hr = scene.node("STEER_TRANSFORM");
        scene.add_child(body_transform, steer_transform_hr);
        scene.add_child(car_node, body_transform);

        let mut car = CarAvatar {
            folder: folder.to_path_buf(),
            car_node,
            body_transform,
            steer_transform_hr,
            org_steer_matrix: Mat44f::default(),
            body_matrix: Mat44f::default(),
            graphics_offset,
            graphics_pitch_rotation,
            graphic_steer_lock_degrees: steer_lock.unwrap_or(0.0),
            steer_lock,
            lods: Vec::new(),
            current_lod: 0,
            cockpit_hr_distance: 2.0,
            driver_hr_distance: 15.0,
            pro_view_nodes: Vec::new(),
            suspension,
            driver_hr,
            driver_lr,
            is_driver_hr: true,
            guid,
            lock_virtual_steer: false,
            hide_arms_in_cockpit: false,
            hide_steer: false,
            max_gear: 0,
            view: ViewState::default(),
            constrained: ConstrainedObjectsManager::default(),
            damage: None,
            pause_menu: false,
            rotating_objects: RotatingObjects::default(),
            car_animations: CarAnimations::default(),
            gear_shift_shake: None,
            analog_instruments: AnalogInstruments::default(),
            digital_instruments: DigitalInstruments::default(),
            game_time_ms: 0.0,
            tc_level: 0,
            abs_level: 0,
            ambient_temperature: 0.0,
            use_mph: false,
            driver_eyes_position: car_ini.as_ref().and_then(|i| i.get_float3("GRAPHICS", "DRIVEREYES").ok()).unwrap_or([0.0; 3]),
            wing_angles: Vec::new(),
            skid_marks: CarSkidMarks::default(),
            tyre_smoke: Vec::new(),
            engine_smoke: None,
            cars_to_be_loaded: 1,
            mirror_position: car_ini.as_ref().and_then(|i| i.get_float3("GRAPHICS", "MIRROR_POSITION").ok()).unwrap_or([0.0; 3]),
            mirrors: crate::mirror::CarMirrorManager::default(),
            backfire: None,
            fuel_in_exhaust: 0.0,
            flames: None,
            ai_active: false,
            backfire_triggered: false,
            brake_lights: CarBrakeLights::default(),
            animated_lights: AnimatedLights::default(),
            brake_discs: BrakeDiscGraphics::default(),
            dynamic_effects: DynamicCarEffects::default(),
            in_pitlane: false,
            replay_mode: false,
            tyre_blur: TyreBlur::default(),
            blurred_objects: BlurredObjects::default(),
            replay_scale: 1.0,
            fake_shadow: None,
            folder_text: folder_text.to_string(),
        };

        // CarLodManager::CarLodManager 0x1400e2f80
        let lods_ini = ini(folder, "lods.ini");
        car.cockpit_hr_distance = get_float(&lods_ini, "COCKPIT_HR", "DISTANCE_SWITCH");
        if car.cockpit_hr_distance == 0.0 {
            car.cockpit_hr_distance = 2.0;
        }
        if lods_ini.as_ref().is_some_and(|i| i.has_key("DRIVER_HR", "DISTANCE_SWITCH")) {
            car.driver_hr_distance = get_float(&lods_ini, "DRIVER_HR", "DISTANCE_SWITCH");
        }
        let mut index = 0;
        while lods_ini.as_ref().is_some_and(|i| i.has_section(&format!("LOD_{index}"))) {
            let section = format!("LOD_{index}");
            let filename = lods_ini.as_ref().map(|i| i.get_string(&section, "FILE")).unwrap_or_default();
            car.lods.push(CarLodDef {
                filename,
                lod_in: get_float(&lods_ini, &section, "IN"),
                lod_out: get_float(&lods_ini, &section, "OUT"),
                index,
                nodes: Vec::new(),
                cockpit_hr: None,
                cockpit_lr: None,
                steer_hr: None,
                steer_lr: None,
            });
            index += 1;
        }
        for i in 0..car.lods.len() {
            car.load_lod(graphics, scene, i, folder_text, skin)?;
        }

        // the rest of init3D
        if let NodeKind::BoundingSphere { delegate } = &mut scene.nodes[car_node].kind {
            *delegate = Some(body_transform);
        }
        scene.apply_no_cull(car_node);
        // the seat belt that is not worn: taken out of the tree for good
        if let Some(n) = scene.find_child_by_name(car_node, "CINTURE_OFF", true) {
            scene.nodes[n].is_active = false;
            if let Some(parent) = scene.nodes[n].parent {
                scene.remove_child(parent, n);
            }
        }
        car.damage = Some(VisualDamageManager::new(graphics, scene, folder, body_transform)?);
        car.rotating_objects = RotatingObjects::new(scene, folder, car_node);
        car.backfire = Some(BackfireParams::new(folder));
        car.make_tyres_double_faced_shadows(graphics, scene, car_node);
        Ok(car)
    }

    /// The nodes `ISuspensionAvatar::getWheelTransform` gives for a wheel: the transform that
    /// holds every level's wheel, or (animated suspensions) each level's wheel node.
    fn wheel_transforms(&self, wheel: usize) -> Vec<NodeId> {
        match &self.suspension {
            Suspension::Avatar(avatar) => vec![avatar.wheel_transforms[wheel]],
            Suspension::Animator(animator) => animator.wheel_transforms(wheel),
        }
    }

    /// `CarAvatar::initMirrorMaterials` 0x1400d75f0: only when the game has a mirror texture.
    pub fn init_mirror_materials(&mut self, graphics: &mut Graphics, scene: &mut Scene, texture: &crate::texture::Texture) -> Result<(), String> {
        self.mirrors = crate::mirror::CarMirrorManager::new(graphics, scene, &self.folder, self.body_transform, texture)?;
        Ok(())
    }

    /// The nodes `CarAvatar::setVisible` 0x1400dade0 switches besides the body's: the wheels'
    /// and the suspensions'.
    pub fn visibility_nodes(&self) -> Vec<NodeId> {
        match &self.suspension {
            Suspension::Avatar(avatar) => avatar.wheel_transforms.iter().chain(&avatar.sus_transforms).copied().collect(),
            Suspension::Animator(animator) => (0..4).filter_map(|i| animator.wheel_transform(i)).collect(),
        }
    }

    /// `CarAvatar::initCommonPostPhysics` 0x1400d6190, the objects of the picture, in the
    /// game's order.
    pub fn init_common_post_physics(&mut self, graphics: &mut Graphics, scene: &mut Scene, sim: &SimNodes, tyre_width: [f32; 4]) -> Result<(), String> {
        self.skid_marks.tyre_width = tyre_width;
        self.tyre_smoke.clear();
        let smoke = graphics.video.smoke;
        for i in 0..4 {
            self.skid_marks.make_buffer(graphics, scene, sim.skid_marks, i)?;
            self.tyre_smoke.push(TyreSmoke::new(graphics, scene, sim.particles, i, smoke, self.cars_to_be_loaded));
        }
        self.engine_smoke = Some(EngineSmoke::new(graphics, scene, sim.particles, smoke));
        let wheels: [Vec<NodeId>; 4] = std::array::from_fn(|w| self.wheel_transforms(w));
        let wheel_nodes = |w: usize| wheels[w].clone();
        self.car_animations = CarAnimations::new(scene, &self.folder, self.body_transform, self.driver_eyes_position[0]);
        self.gear_shift_shake = Some(GearShiftShake::new(scene, self.car_node));
        self.analog_instruments = AnalogInstruments::new(scene, &self.folder, self.car_node, self.body_transform)?;
        self.brake_lights = CarBrakeLights::new(graphics, scene, &self.folder, self.body_transform)?;
        self.animated_lights = AnimatedLights::new(scene, &self.folder, self.body_transform);
        self.tyre_blur = TyreBlur::new(graphics, scene, &wheel_nodes)?;
        self.blurred_objects = BlurredObjects::new(scene, &self.folder, &wheel_nodes)?;
        self.digital_instruments = DigitalInstruments::new(graphics, scene, &self.folder, self.body_transform)?;
        // (the game stops on a car whose flames have no textures; here it has no flames)
        self.flames = match Flames::new(graphics, scene, &self.folder, self.body_transform, sim.render_finished) {
            Ok(flames) => Some(flames),
            Err(e) => {
                println!("WARNING: {e}");
                None
            }
        };
        self.brake_discs = BrakeDiscGraphics::new(graphics, scene, &self.folder, self.car_node)?;
        self.dynamic_effects = DynamicCarEffects::new(graphics, scene, self.body_transform);
        Ok(())
    }

    /// `CarAvatar::onPostLoad` 0x1400d92b0: the flat ground shadows, drawn when the scene's
    /// `CAR_SHADOWS` node is reached.
    pub fn on_post_load(&mut self, graphics: &mut Graphics, scene: &mut Scene, car_shadows_node: NodeId) {
        let shadow = Rc::new(RefCell::new(CarFakeShadow::new(graphics, &self.folder_text, &self.folder, self.car_node, self.body_transform)));
        scene.add_event_handler(car_shadows_node, shadow.clone());
        self.fake_shadow = Some(shadow);
    }

    /// `CarAvatar::makeTyresDoubleFacedShadows` 0x1400d9020: the tyres' material casts its
    /// shadow from both faces.
    fn make_tyres_double_faced_shadows(&self, graphics: &Graphics, scene: &mut Scene, node: NodeId) {
        if let NodeKind::Mesh(mesh) = &scene.nodes[node].kind {
            if let Some(material) = mesh.material {
                let m = &mut scene.materials[material.0 as usize];
                if m.shader.is_some_and(|s| graphics.shaders.get(s).name == "ksTyres") {
                    m.double_face_shadow = true;
                }
            }
        }
        let children = scene.nodes[node].children.clone();
        for child in children {
            self.make_tyres_double_faced_shadows(graphics, scene, child);
        }
    }

    /// `CarLodManager::loadLod` 0x1400e4370.
    fn load_lod(&mut self, graphics: &mut Graphics, scene: &mut Scene, lod_index: usize, folder_text: &str, skin: &str) -> Result<(), String> {
        let filename = self.lods[lod_index].filename.clone();
        let mut io = Kn5Io::new();
        io.skin_override_path.push(format!("{folder_text}/skins/{skin}"));
        let name = format!("{folder_text}/{filename}");
        let root = io.load(graphics, scene, &name, &self.folder.join(&filename))?;
        scene.compile(graphics, root);
        self.lods[lod_index].nodes.push(root);
        scene.add_child(self.body_transform, root);
        println!("INIT LOD:{}", self.lods[lod_index].index);
        for i in 0..4 {
            for table in [&WHEEL_NAMES, &SUS_NAMES, &DISC_NAMES] {
                if let Some(n) = scene.find_child_by_name(root, table[i], true) {
                    self.lods[lod_index].nodes.push(n);
                }
            }
        }
        if let Some(n) = scene.find_child_by_name(root, "REAR_AXLE", true) {
            self.lods[lod_index].nodes.push(n);
        }
        if self.lods[lod_index].index == 0 {
            self.init_no_body_nodes(scene);
            if let Some(n) = scene.find_child_by_name(root, "STEER_HR", true) {
                let world = scene.get_world_matrix(n);
                scene.nodes[self.steer_transform_hr].matrix = world;
                self.org_steer_matrix = scene.get_world_matrix(n);
                scene.nodes[n].matrix = Mat44f::IDENTITY;
                self.lods[lod_index].nodes.push(n);
                scene.add_child(self.steer_transform_hr, n);
                self.lods[lod_index].steer_hr = Some(n);
            } else {
                println!("ERROR: CANNOT FIND STEER_HR IN FILE: {name}");
            }
        }
        if let Some(n) = scene.find_child_by_name(root, "STEER_LR", true) {
            scene.nodes[n].matrix = Mat44f::IDENTITY;
            self.lods[lod_index].nodes.push(n);
            scene.add_child(self.steer_transform_hr, n);
            self.lods[lod_index].steer_lr = Some(n);
        }
        self.lods[lod_index].cockpit_hr = scene.find_child_by_name(root, "COCKPIT_HR", true);
        self.lods[lod_index].cockpit_lr = scene.find_child_by_name(root, "COCKPIT_LR", true);
        self.constrained.add_model(scene, root);
        let folder = self.folder.clone();
        let car_node = self.car_node;
        match &mut self.suspension {
            Suspension::Avatar(avatar) => avatar.add_model(scene, &folder, car_node, root),
            Suspension::Animator(animator) => animator.add_model(scene, root),
        }
        Ok(())
    }

    /// `CarLodManager::initNoBodyNodes` 0x1400e3fa0: the nodes of the first level that are not
    /// drawable things, minus the ones `data/proview_nodes.ini` marks with 1. The game writes
    /// that file when it is missing or older than the model; this never writes it.
    fn init_no_body_nodes(&mut self, scene: &Scene) {
        let Some(root) = self.lods.first().and_then(|l| l.nodes.first().copied()) else {
            return;
        };
        let kn5 = self.folder.join(&self.lods[0].filename);
        let ini_path = self.folder.join("data/proview_nodes.ini");
        let modified = |p: &Path| std::fs::metadata(p).and_then(|m| m.modified()).ok();
        let fresh = match (modified(&ini_path), modified(&kn5)) {
            (Some(a), Some(b)) => a > b,
            _ => false,
        };
        let reader = if fresh { IniReader::load(&ini_path).ok() } else { None };
        if !fresh {
            println!("CREATING proview_nodes.ini");
        }
        fn walk(scene: &Scene, reader: &Option<IniReader>, list: &mut Vec<NodeId>, n: NodeId, depth: usize) {
            let node = &scene.nodes[n];
            match reader {
                Some(r) => {
                    let key = format!("{}{}", "____".repeat(depth), node.name);
                    if node.renderable.is_none() && r.get_int("NODES", &key).unwrap_or(0) != 1 {
                        list.push(n);
                    }
                    for &c in &node.children {
                        walk(scene, reader, list, c, depth + 1);
                    }
                }
                None => {
                    // writeNode: a drawable node ends the walk of its branch
                    if node.renderable.is_none() {
                        list.push(n);
                        for &c in &node.children {
                            walk(scene, reader, list, c, depth + 1);
                        }
                    }
                }
            }
        }
        for &c in &scene.nodes[root].children {
            walk(scene, &reader, &mut self.pro_view_nodes, c, 0);
        }
    }

    /// `CarAvatar::makeBodyMatrix` 0x1400d8ec0: the physics body's matrix moved by the
    /// model's offset (in the car's axes) and pitched.
    pub fn make_body_matrix(&self, w: &Mat44f) -> Mat44f {
        let o = self.graphics_offset;
        let mut b = *w;
        let m = &w.m;
        b.m[3][0] = ((o[1] * m[1][0] + o[0] * m[0][0]) + o[2] * m[2][0]) + m[3][0];
        b.m[3][1] = ((o[0] * m[0][1] + o[1] * m[1][1]) + o[2] * m[2][1]) + m[3][1];
        b.m[3][2] = ((o[0] * m[0][2] + o[1] * m[1][2]) + o[2] * m[2][2]) + m[3][2];
        if self.graphics_pitch_rotation != 0.0 && !self.graphics_pitch_rotation.is_nan() {
            b = xm_matrix_multiply(&Mat44f::create_from_axis_angle(&Vec3f::new(1.0, 0.0, 0.0), self.graphics_pitch_rotation), &b);
        }
        b
    }

    /// What the frame's `update`s do to the nodes, in the game's order: `CarAvatar::update`
    /// (the body, the steering wheel), `SuspensionAvatar::update`, `CarLodManager::update`.
    pub fn update(&mut self, graphics: &mut Graphics, scene: &mut Scene, state: &CarPhysicsState, dt: f32) {
        // CarAvatar::updateInPitlaneState 0x1400dd6d0
        self.in_pitlane = state.tyre_surface_def.iter().any(|s| s.is_pitlane);
        // the backfire test and the handlers of evOnBackfireTriggered
        self.backfire_triggered = false;
        if let Some(backfire) = &mut self.backfire {
            if 0.0 < state.engine_life_left && backfire.check(state, &mut self.fuel_in_exhaust, self.ai_active, dt) {
                self.backfire_triggered = true;
                if let Some(flames) = &self.flames {
                    flames.borrow_mut().on_backfire(self.fuel_in_exhaust, &mut graphics.crt_rand);
                }
            }
        }
        let replay_dt = if self.replay_mode { dt * self.replay_scale } else { dt };
        self.body_matrix = self.make_body_matrix(&state.world_matrix);
        scene.nodes[self.body_transform].matrix = self.body_matrix;
        if let Some(shadow) = &self.fake_shadow {
            shadow.borrow_mut().state = *state;
        }
        // ACCameraManager::isCameraOnBoard 0x1400336f0
        let on_board = self.guid == self.view.focused_car_index && (self.view.camera_mode == 0 || (self.view.camera_mode == 2 && self.view.drivable_mode == 4));
        // the steering wheel
        scene.nodes[self.steer_transform_hr].matrix = self.org_steer_matrix;
        // CarAvatar::getGraphicSteerDeg 0x1400d34b0
        let mut graphic_steer_deg = state.steer;
        if let Some(lock) = self.steer_lock {
            graphic_steer_deg = (graphic_steer_deg / lock) * self.graphic_steer_lock_degrees;
        }
        if !(self.lock_virtual_steer && on_board) {
            let r = Mat44f::create_from_axis_angle(&Vec3f::new(0.0, 0.0, 1.0), graphic_steer_deg * DEG_TO_RAD);
            scene.nodes[self.steer_transform_hr].matrix = xm_matrix_multiply(&r, &self.org_steer_matrix);
        }
        // the driver: which model shows, and whether it shows at all
        if self.lock_virtual_steer {
            for driver in [&mut self.driver_hr, &mut self.driver_lr].into_iter().flatten() {
                driver.lock_animation = on_board;
                if on_board {
                    driver.set_locked_position(scene);
                }
            }
        }
        if let Some(d) = &self.driver_hr {
            scene.nodes[d.driver_root].is_active = self.is_driver_hr;
        }
        if let Some(d) = &self.driver_lr {
            scene.nodes[d.driver_root].is_active = !self.is_driver_hr;
        }
        let shown = if self.is_driver_hr { &self.driver_hr } else { &self.driver_lr };
        if self.guid == 0 && self.view.camera_mode == 9 {
            if let Some(d) = shown {
                d.set_visible(scene, false);
            }
        } else {
            let special = self.view.camera_mode == 2 && self.view.drivable_mode == 4 && self.view.focused_car_index == self.guid;
            if let Some(d) = shown {
                d.set_visible(scene, !special);
                scene.nodes[self.steer_transform_hr].is_active = !special;
                if self.hide_arms_in_cockpit {
                    d.set_visible(scene, !on_board);
                }
                if self.hide_steer {
                    d.set_visible(scene, !on_board);
                    scene.nodes[self.steer_transform_hr].is_active = !on_board;
                }
            }
        }

        // CarAvatar::updateSkidMarks 0x1400dd780, before the car's objects move the wheels
        {
            let wheel_node = |i: usize| match &self.suspension {
                Suspension::Avatar(avatar) => Some(avatar.wheel_transforms[i]),
                Suspension::Animator(animator) => animator.wheel_transform(i),
            };
            let nodes: [Option<NodeId>; 4] = std::array::from_fn(wheel_node);
            let scene_ref: &Scene = scene;
            let active = scene_ref.nodes[self.car_node].is_active;
            self.skid_marks.update(state, active, &|i| nodes[i].map(|n| scene_ref.get_world_matrix(n)).unwrap_or(Mat44f::IDENTITY));
        }
        // the car's objects, in the order they were added: the two drivers first
        let frame = DriverFrame {
            dt,
            dt_scaled: replay_dt,
            camera_position: self.view.camera_position,
            on_board,
            use_pro_view: self.view.use_pro_view,
            graphic_steer_deg,
            max_gear: self.max_gear,
        };
        for driver in [&mut self.driver_hr, &mut self.driver_lr].into_iter().flatten() {
            driver.update(scene, state, &frame);
        }
        if let Suspension::Avatar(avatar) = &self.suspension {
            avatar.update(scene, state);
        }
        // CarLodManager::update 0x1400e57c0
        for &n in &self.pro_view_nodes {
            scene.nodes[n].is_active = true;
        }
        if let Some(damage) = &mut self.damage {
            self.mirrors.update(scene, self.guid == self.view.focused_car_index && crate::mirror::is_camera_on_board(self.view.camera_mode, self.view.drivable_mode));
            damage.update(graphics, scene, state, replay_dt, self.pause_menu);
        }
        self.rotating_objects.update(scene, state, replay_dt);
        {
            let frame = SmokeFrame {
                dt,
                now_ms: self.game_time_ms,
                connected: scene.nodes[self.car_node].is_active,
                replay_time_mult: self.replay_mode.then_some(self.replay_scale),
            };
            for smoke in &mut self.tyre_smoke {
                smoke.update(scene, state, &frame, &mut graphics.crt_rand);
            }
            if let Some(smoke) = &mut self.engine_smoke {
                smoke.update(state, &frame, &mut graphics.crt_rand);
            }
        }
        {
            // CarAvatar::getActiveDriverModel 0x1400d30b0
            let driver = if self.is_driver_hr { &self.driver_hr } else { &self.driver_lr };
            let positions = driver.as_ref().map(|d| DriverShiftPositions {
                shift: d.shift_player.as_ref().map(|p| p.get_current_pos()).unwrap_or(0.0),
                shift_down: d.shift_down_player.as_ref().map(|p| p.get_current_pos()).unwrap_or(0.0),
            });
            self.car_animations.door_animation_target = if self.guid == 0 && self.view.camera_mode == 9 { 1.0 } else { 0.0 };
            self.car_animations.update(scene, state, dt, self.steer_lock.unwrap_or(0.0), &self.wing_angles, positions);
        }
        if let Some(shake) = &mut self.gear_shift_shake {
            shake.update(scene, state, dt, &mut graphics.crt_rand);
        }
        self.analog_instruments.update(scene, state, dt, self.guid == self.view.focused_car_index);
        self.brake_lights.update(scene, state, replay_dt, self.in_pitlane);
        self.animated_lights.update(scene, self.brake_lights.front_lights_on, dt);
        let scale = if self.replay_mode && self.replay_scale != 0.0 { self.replay_scale } else { 1.0 };
        self.tyre_blur.update(scene, state, scale);
        self.blurred_objects.update(scene, state, scale);
        self.digital_instruments.update(
            scene,
            state,
            &DigitalFrame {
                dt,
                game_time_ms: self.game_time_ms,
                focused: self.guid == self.view.focused_car_index,
                pause_menu: self.pause_menu,
                tc_level: self.tc_level,
                abs_level: self.abs_level,
                ambient_temperature: self.ambient_temperature,
                lap_count: state.lap_count,
                sun_angle: graphics.lighting.angle,
                use_mph: self.use_mph,
            },
        );
        if let Some(flames) = &self.flames {
            let mut flames = flames.borrow_mut();
            flames.scene_camera = self.view.camera_matrix;
            let connected = scene.nodes[self.car_node].is_active;
            if flames.update(dt, dt, connected, self.replay_mode.then_some(self.replay_scale)) {
                self.fuel_in_exhaust = 0.0;
            }
        }
        self.brake_discs.update(scene, state, dt, self.replay_mode.then_some(self.replay_scale));
        if !self.replay_mode {
            self.dynamic_effects.update(scene, state);
        }
    }

    /// What runs after all updates and before the picture (the handlers of `evOnPostUpdate`):
    /// the animated suspensions, `ConstrainedObjectsManager::updateConstraints`, then
    /// `CarLodManager::updateLodVisibility` 0x1400e5810.
    /// `camera` is the scene camera's matrix; `track_camera` is the track-side camera mode
    /// (its distances count a tenth).
    pub fn post_update(&mut self, scene: &mut Scene, state: &CarPhysicsState, dt: f32, camera: &Mat44f, fov: f32, track_camera: bool) {
        if let Suspension::Animator(animator) = &mut self.suspension {
            animator.update(scene, self.body_transform, state, dt);
        }
        self.constrained.update_constraints(scene);
        let mut div = 1.0f32;
        if track_camera {
            div *= 10.0;
        }
        let dx = camera.m[3][0] - self.body_matrix.m[3][0];
        let dy = camera.m[3][1] - self.body_matrix.m[3][1];
        let dz = camera.m[3][2] - self.body_matrix.m[3][2];
        let sq = (dy * dy + dx * dx) + dz * dz;
        let mut d = if sq != 0.0 && !sq.is_nan() { rustyac_math::sqrtf(sq) } else { 0.0 };
        d *= fov;
        self.current_lod = -1;
        d *= f32::from_bits(0x3c88_8889);
        d /= div;
        for lod in &self.lods {
            let active = d >= lod.lod_in && d < lod.lod_out;
            if active {
                self.current_lod = lod.index;
            }
            for &n in &lod.nodes {
                scene.nodes[n].is_active = active;
            }
            self.is_driver_hr = !(d > self.driver_hr_distance);
            let mut set = |n: Option<NodeId>, on: bool| {
                if let Some(n) = n {
                    scene.nodes[n].is_active = on;
                }
            };
            if !(d > self.cockpit_hr_distance) {
                if lod.cockpit_hr.is_some() && lod.cockpit_lr.is_some() {
                    set(lod.cockpit_hr, true);
                    set(lod.cockpit_lr, false);
                } else {
                    set(lod.cockpit_hr, true);
                    set(lod.cockpit_lr, true);
                }
                if lod.steer_hr.is_some() && lod.steer_lr.is_some() {
                    set(lod.steer_hr, true);
                    set(lod.steer_lr, false);
                }
            } else {
                if lod.cockpit_hr.is_some() && lod.cockpit_lr.is_some() {
                    set(lod.cockpit_hr, false);
                    set(lod.cockpit_lr, true);
                } else if lod.cockpit_lr.is_some() {
                    set(lod.cockpit_lr, true);
                }
                if lod.steer_hr.is_some() && lod.steer_lr.is_some() {
                    set(lod.steer_hr, false);
                    set(lod.steer_lr, true);
                }
            }
        }
    }
}

impl SuspensionAvatar {
    /// `SuspensionAvatar::addModel` 0x1401b32b0: the model's wheel, suspension and disc nodes
    /// lose their own matrices (but a side offset from `suspensions.ini`) and move under the
    /// transforms that follow the physics.
    fn add_model(&mut self, scene: &mut Scene, folder: &Path, car_node: NodeId, root: NodeId) {
        let sus_ini = ini(folder, "suspensions.ini");
        let place = |scene: &mut Scene, n: NodeId, offset: f32, parent: NodeId| {
            let mut m = Mat44f::IDENTITY;
            m.m[3][0] = offset;
            m.m[3][1] = 0.0;
            m.m[3][2] = 0.0;
            scene.nodes[n].matrix = m;
            scene.add_child(parent, n);
        };
        for i in 0..4 {
            if let Some(n) = scene.find_child_by_name(root, WHEEL_NAMES[i], true) {
                place(scene, n, get_float(&sus_ini, "GRAPHICS_OFFSETS", WHEEL_NAMES[i]), self.wheel_transforms[i]);
            }
            if let Some(n) = scene.find_child_by_name(root, SUS_NAMES[i], true) {
                place(scene, n, get_float(&sus_ini, "GRAPHICS_OFFSETS", SUS_NAMES[i]), self.sus_transforms[i]);
            }
            if let Some(n) = scene.find_child_by_name(root, DISC_NAMES[i], true) {
                // the wheel's key, not the disc's
                place(scene, n, get_float(&sus_ini, "GRAPHICS_OFFSETS", WHEEL_NAMES[i]), self.disc_transforms[i]);
            }
        }
        if let Some(n) = scene.find_child_by_name(root, "REAR_AXLE", true) {
            let axle = match self.rear_axle {
                Some(axle) => axle,
                None => {
                    let axle = scene.node("REAR_AXLE_TRANSFORM");
                    scene.add_child(car_node, axle);
                    self.rear_axle = Some(axle);
                    axle
                }
            };
            scene.nodes[n].matrix = Mat44f::IDENTITY;
            scene.add_child(axle, n);
        }
    }

    /// `SuspensionAvatar::update` 0x1401b3840: plain copies of world matrices.
    fn update(&self, scene: &mut Scene, s: &CarPhysicsState) {
        for i in 0..4 {
            scene.nodes[self.wheel_transforms[i]].matrix = s.tyre_matrix[i];
            scene.nodes[self.sus_transforms[i]].matrix = s.suspension_matrix[i];
            scene.nodes[self.disc_transforms[i]].matrix = s.tyre_matrix[i];
        }
        if let Some(axle) = self.rear_axle {
            let (a, b) = (&s.suspension_matrix[2].m, &s.suspension_matrix[3].m);
            let mx = (a[3][0] + b[3][0]) * 0.5;
            let my = (a[3][1] + b[3][1]) * 0.5;
            let mz = (a[3][2] + b[3][2]) * 0.5;
            let mut m = s.suspension_matrix[2];
            m.m[3][0] = mx;
            m.m[3][1] = my;
            m.m[3][2] = mz;
            scene.nodes[axle].matrix = m;
        }
    }
}
