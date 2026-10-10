// SPDX-License-Identifier: GPL-3.0-or-later

//! The game's side of the car: acs.exe's own `CarLodManager` (its constructor loads, compiles
//! and attaches every level of detail and re-parents the steering wheel; `update`;
//! `updateLodVisibility`), `SuspensionAvatar` or `SuspensionAnimator` (constructor, `addModel`,
//! the per-frame update), `CarAvatar::makeBodyMatrix`, `NodeBoundingSphere` and
//! `makeTyresDoubleFacedShadows`, called by address against a `CarAvatar`, a `Sim` and a `Game`
//! that are zeroed buffers with the few fields filled in that this code reads.
//!
//! What the harness does by hand in place of `CarAvatar::init3D` and `CarAvatar::update`
//! (which need the whole game): the order of the calls, the two transform nodes, taking the
//! unworn seat belt out of the tree, and the steering wheel's matrix (the game's own
//! `mat44f::createFromAxisAngle` gives the rotation; the product with the wheel's rest matrix
//! is the port's `XMMatrixMultiply`, which the track frames prove on thousands of nodes).
//! Task 21 added the car's other objects, each built by the game's own constructor on the same
//! fake blocks and updated in the game's order: `ConstrainedObjectsManager`,
//! `VisualDamageManager`, `CarBrakeLights`, `TyreBlur`, `BlurredObjects`, `BrakeDiscGraphics`,
//! `DynamicCarEffects`, `CarFakeShadow`.

use std::path::Path;

use rustyac_physics::vecmath::{xm_matrix_multiply, Mat44f};

use crate::ac::{rd, wr, wstring, write_wstring, Game};
use crate::frames::CarSpec;

const VA_FONT_CTOR: usize = 0x1_4020_0bb0; // Font::Font(eFontType, float, bool, bool): DirectWrite, not part of the 3D frame
const VA_CONSOLE_SINGLETON: usize = 0x1_4155_9c08; // static Console* Console::_singleton
const VA_CONSTRAINED_CTOR: usize = 0x1_4007_6d10; // ConstrainedObjectsManager::ConstrainedObjectsManager(CarAvatar*)
const VA_CONSTRAINED_UPDATE_CONSTRAINTS: usize = 0x1_4007_7320; // ConstrainedObjectsManager::updateConstraints(float)
const VA_NODE_BOUNDING_SPHERE_CTOR: usize = 0x1_4021_8a30; // NodeBoundingSphere::NodeBoundingSphere(std::wstring, float)
const VA_NODE_BOUNDING_SPHERE_APPLY_NO_CULL: usize = 0x1_4021_8b50; // NodeBoundingSphere::applyNoCull(Node*)
const VA_SUSPENSION_AVATAR_CTOR: usize = 0x1_401b_2fe0; // SuspensionAvatar::SuspensionAvatar(CarAvatar*)
const VA_SUSPENSION_AVATAR_UPDATE: usize = 0x1_401b_3840; // SuspensionAvatar::update(float)
const VA_SUSPENSION_ANIMATOR_CTOR: usize = 0x1_401b_0770; // SuspensionAnimator::SuspensionAnimator(CarAvatar*)
const VA_SUSPENSION_ANIMATOR_UPDATE: usize = 0x1_401b_25a0; // SuspensionAnimator::suspensionAnimatorUpdate(float)
const VA_CAR_LOD_MANAGER_CTOR: usize = 0x1_400e_2f80; // CarLodManager::CarLodManager(CarAvatar*)
const VA_CAR_LOD_MANAGER_UPDATE: usize = 0x1_400e_57c0; // CarLodManager::update(float)
const VA_CAR_LOD_MANAGER_UPDATE_LOD_VISIBILITY: usize = 0x1_400e_5810; // CarLodManager::updateLodVisibility()
const VA_CAR_AVATAR_MAKE_BODY_MATRIX: usize = 0x1_400d_8ec0; // CarAvatar::makeBodyMatrix(const mat44f&, mat44f&)
const VA_MAKE_TYRES_DOUBLE_FACED_SHADOWS: usize = 0x1_400d_9020; // makeTyresDoubleFacedShadows(Node*)
const VA_CREATE_FROM_AXIS_ANGLE: usize = 0x1_4005_71a0; // static mat44f mat44f::createFromAxisAngle(const vec3f&, float)
const VA_DIGITAL_INSTRUMENTS_CTOR: usize = 0x1_400e_b030; // DigitalInstruments::DigitalInstruments(CarAvatar*)
const VA_DIGITAL_INSTRUMENTS_UPDATE: usize = 0x1_400f_0520; // DigitalInstruments::update(float)
const VA_GEARS_INITIALISER: usize = 0x1_4000_7cd0; // the gear letters R N 1 … 9 of DigitalItem.obj
const VA_ROTATING_OBJECTS_CTOR: usize = 0x1_400b_a560; // RotatingObjects::RotatingObjects(CarAvatar*)
const VA_ROTATING_OBJECTS_UPDATE: usize = 0x1_400b_ae20; // RotatingObjects::update(float)
const VA_CAR_ANIMATIONS_CTOR: usize = 0x1_4006_0ba0; // CarAnimations::CarAnimations(CarAvatar*)
const VA_CAR_ANIMATIONS_UPDATE: usize = 0x1_4006_2160; // CarAnimations::update(float)
const VA_GEAR_SHIFT_SHAKE_CTOR: usize = 0x1_4010_48f0; // GearShiftShake::GearShiftShake(CarAvatar*)
const VA_GEAR_SHIFT_SHAKE_UPDATE: usize = 0x1_4010_4ca0; // GearShiftShake::update(float)
const VA_ANALOG_INSTRUMENTS_CTOR: usize = 0x1_4005_6480; // AnalogInstruments::AnalogInstruments(CarAvatar*)
const VA_ANALOG_INSTRUMENTS_UPDATE: usize = 0x1_4005_9770; // AnalogInstruments::update(float)
const VA_CAR_AVATAR_INIT_DRIVER: usize = 0x1_400d_7380; // CarAvatar::initDriver()
const VA_DRIVER_MODEL_UPDATE: usize = 0x1_400f_b5d0; // DriverModel::update(float)
const VA_TYRE_SMOKE_CTOR: usize = 0x1_401d_0000; // TyreSmoke::TyreSmoke(CarAvatar*, int)
const VA_TYRE_SMOKE_UPDATE: usize = 0x1_401d_0b80; // TyreSmoke::update(float)
const VA_ENGINE_SMOKE_CTOR: usize = 0x1_4009_2de0; // EngineSmoke::EngineSmoke(CarAvatar*)
const VA_ENGINE_SMOKE_UPDATE: usize = 0x1_4009_35e0; // EngineSmoke::update(float)
const VA_BACKFIRE_PARAMS_CTOR: usize = 0x1_400c_ccb0; // BackfireParams::BackfireParams(CarAvatar*)
const VA_BACKFIRE_PARAMS_CHECK: usize = 0x1_400d_26b0; // BackfireParams::checkBackfire(float)
const VA_FLAMES_CTOR: usize = 0x1_400f_f990; // Flames::Flames(CarAvatar*)
const VA_FLAMES_UPDATE: usize = 0x1_4010_46a0; // Flames::update(float)
const VA_MIRROR_TEXTURE_RENDERER_CTOR: usize = 0x1_4011_3c40; // MirrorTextureRenderer::MirrorTextureRenderer(Sim*)
const VA_MIRROR_TEXTURE_RENDERER_RENDER: usize = 0x1_4011_4410; // MirrorTextureRenderer::render(float)
const VA_CAR_MIRROR_MANAGER_CTOR: usize = 0x1_400e_5f20; // CarMirrorManager::CarMirrorManager(CarAvatar*, Texture*)
const VA_CAR_MIRROR_MANAGER_UPDATE: usize = 0x1_400e_6fc0; // CarMirrorManager::update(float)
const VA_VIRTUAL_MIRROR_CTOR: usize = 0x1_401d_1ab0; // VirtualMirrorRenderer::VirtualMirrorRenderer(Sim*)
const VA_VIRTUAL_MIRROR_RENDER: usize = 0x1_401d_1ea0; // VirtualMirrorRenderer::renderVirtualMirror(float)
const VA_SKID_MARK_BUFFER_CTOR: usize = 0x1_4018_f2d0; // SkidMarkBuffer::SkidMarkBuffer(GraphicsManager*, unsigned int)
const VA_CAR_AVATAR_UPDATE_SKID_MARKS: usize = 0x1_400d_d780; // CarAvatar::updateSkidMarks(float)
const VA_KS_RANDOMIZE: usize = 0x1_4004_b290; // ksRandomize(unsigned int): srand
const VA_CAR_BRAKE_LIGHTS_CTOR: usize = 0x1_400d_dbb0; // CarBrakeLights::CarBrakeLights(CarAvatar*)
const VA_CAR_BRAKE_LIGHTS_UPDATE: usize = 0x1_400e_0450; // CarBrakeLights::update(float)
const VA_ANIMATED_LIGHTS_CTOR: usize = 0x1_4005_a850; // AnimatedLights::AnimatedLights(CarBrakeLights&)
const VA_ANIMATED_LIGHTS_UPDATE: usize = 0x1_4005_aef0; // AnimatedLights::update(float)
const VA_CAR_AVATAR_VTABLE: usize = 0x1_404b_e5e8; // const CarAvatar::`vftable' (for the game's dynamic_cast of an object's parent)
const VA_BRAKE_DISC_GRAPHICS_CTOR: usize = 0x1_4005_ca40; // BrakeDiscGraphics::BrakeDiscGraphics(CarAvatar&)
const VA_BRAKE_DISC_GRAPHICS_UPDATE: usize = 0x1_4005_d930; // BrakeDiscGraphics::update(float)
const VA_DYNAMIC_CAR_EFFECTS_CTOR: usize = 0x1_4009_1230; // DynamicCarEffects::DynamicCarEffects(CarAvatar*)
const VA_DYNAMIC_CAR_EFFECTS_UPDATE: usize = 0x1_4009_1ac0; // DynamicCarEffects::update(float)
const VA_TYRE_BLUR_CTOR: usize = 0x1_401c_f760; // TyreBlur::TyreBlur(CarAvatar*)
const VA_TYRE_BLUR_UPDATE: usize = 0x1_401c_fdb0; // TyreBlur::update(float)
const VA_BLURRED_OBJECTS_CTOR: usize = 0x1_400c_35d0; // BlurredObjects::BlurredObjects(CarAvatar*)
const VA_BLURRED_OBJECTS_UPDATE: usize = 0x1_400c_43b0; // BlurredObjects::update(float)
const VA_CAR_FAKE_SHADOW_CTOR: usize = 0x1_400e_0d80; // CarFakeShadow::CarFakeShadow(CarAvatar*)
const VA_KN5IO_ADD_DLC_KEY: usize = 0x1_4021_4de0; // static void KN5IO::addDLCKey(unsigned int)
const VA_DIGITAL_PANELS_CTOR: usize = 0x1_4008_20f0; // DigitalPanels::DigitalPanels(CarAvatar*)
const VA_DIGITAL_PANELS_UPDATE: usize = 0x1_4008_3f70; // DigitalPanels::update(float)

/// The start-up initialisers of the tables of node names (`WHEEL_LF` …) of CarLodManager.obj,
/// SuspensionAvatar.obj and SuspensionAnimator.obj.
const VA_NAME_TABLE_INITIALISERS: [usize; 12] = [VA_GEARS_INITIALISER, 0x1_4000_75c0, 0x1_4000_74b0, 0x1_4000_7220, 0x1_4000_e2e0, 0x1_4000_e1d0, 0x1_4000_e0c0, 0x1_4000_dc10, 0x1_4000_dd20, 0x1_4000_dfb0, 0x1_4000_db00, VA_GLASS_DAMAGE_NAMES_INITIALISER];
const VA_GLASS_DAMAGE_NAMES_INITIALISER: usize = 0x1_4000_fc00; // the names DAMAGE_GLASS_FRONT … of VisualDamageManager.obj
const VA_VISUAL_DAMAGE_CTOR: usize = 0x1_401d_2890; // VisualDamageManager::VisualDamageManager(CarAvatar*)
const VA_VISUAL_DAMAGE_UPDATE: usize = 0x1_401d_56f0; // VisualDamageManager::update(float)

const DEG: f32 = f32::from_bits(0x3c8e_f998);

fn copy_file(from: &Path, to: &Path) -> Result<(), String> {
    if let Some(parent) = to.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    let same = match (std::fs::metadata(from), std::fs::metadata(to)) {
        (Ok(a), Ok(b)) => a.len() == b.len(),
        _ => false,
    };
    if !same {
        std::fs::copy(from, to).map_err(|e| format!("{} -> {}: {e}", from.display(), to.display()))?;
    }
    Ok(())
}

fn copy_folder(from: &Path, to: &Path) -> Result<(), String> {
    for entry in std::fs::read_dir(from).map_err(|e| format!("{}: {e}", from.display()))?.filter_map(|e| e.ok()) {
        if entry.path().is_file() {
            copy_file(&entry.path(), &to.join(entry.file_name()))?;
        }
    }
    Ok(())
}

/// The data files the car's objects read.
const DATA_FILES: [&str; 25] = [
    "flames.ini",
    "flame_presets.ini",
    "sounds.ini",
    "car.ini",
    "lods.ini",
    "suspensions.ini",
    "ambient_shadows.ini",
    "blurred_objects.ini",
    "lights.ini",
    "brakes.ini",
    "damage.ini",
    "mirrors.ini",
    "driver3d.ini",
    "analog_instruments.ini",
    "analog_speed_curve.lut",
    "digital_instruments.ini",
    "digital_panels.ini",
    "flames.ini",
    "flame_presets.ini",
    "extra_animations.ini",
    "wing_animations.ini",
    "suspension_graphics.ini",
    "engine.ini",
    "drivetrain.ini",
    "tyres.ini",
];
/// Loose files of the car's folder the car's objects read.
const LOOSE_FILES: [&str; 6] = ["body_shadow.png", "tyre_0_shadow.png", "tyre_1_shadow.png", "tyre_2_shadow.png", "tyre_3_shadow.png", "driver_base_pos.knh"];

/// The car's files in the oracle's scratch folder: the game's code opens them by relative
/// paths (`content/cars/<car>/…`). The data files are plain ones (from `cardata/` or unpacked
/// from the car's `data.acd` in memory), the models, the skin and the animations are copies.
pub fn prepare_root(root: &Path, game: &Path, repo: &Path, spec: &CarSpec, car_data: Option<&Path>) -> Result<(), String> {
    let source = game.join("content/cars").join(&spec.name);
    let target = root.join("content/cars").join(&spec.name);
    if target.join("data.acd").exists() {
        return Err(format!("{} holds a data.acd: the oracle wants the plain data files only", target.display()));
    }
    let data = target.join("data");
    std::fs::create_dir_all(&data).map_err(|e| e.to_string())?;
    // the list of nodes the game writes into the data folder: gone, so that every run makes it anew
    let _ = std::fs::remove_file(data.join("proview_nodes.ini"));
    let plain = repo.join("cardata").join(&spec.name);
    let unpacked = source.join("data");
    for name in DATA_FILES {
        let bytes = if plain.join(name).is_file() {
            std::fs::read(plain.join(name)).map_err(|e| e.to_string())?
        } else {
            match rustyac_physics::data::read(&unpacked.join(name))? {
                Some(bytes) => bytes,
                None => continue,
            }
        };
        let same = std::fs::read(data.join(name)).is_ok_and(|old| old == bytes);
        if !same {
            std::fs::write(data.join(name), bytes).map_err(|e| e.to_string())?;
        }
    }
    let lods = rustyac_physics::data::ini::IniReader::load(&data.join("lods.ini"))?;
    let mut index = 0;
    while lods.has_section(&format!("LOD_{index}")) {
        let file = lods.get_string(&format!("LOD_{index}"), "FILE");
        copy_file(&source.join(&file), &target.join(&file))?;
        index += 1;
    }
    for name in LOOSE_FILES {
        if source.join(name).is_file() {
            copy_file(&source.join(name), &target.join(name))?;
        }
    }
    // the textures of the displays' graphs (digital_instruments.ini TEXTURE_BASE / TEXTURE_TOP)
    if let Ok(text) = std::fs::read_to_string(data.join("digital_instruments.ini")) {
        for line in text.lines() {
            let Some((key, value)) = line.split_once('=') else { continue };
            if matches!(key.trim(), "TEXTURE_BASE" | "TEXTURE_TOP") {
                let name = value.split(';').next().unwrap_or("").trim();
                if !name.is_empty() && source.join("texture").join(name).is_file() {
                    copy_file(&source.join("texture").join(name), &target.join("texture").join(name))?;
                }
            }
        }
    }
    // the digits of the panels
    if source.join("texture/display_panel").is_dir() {
        copy_folder(&source.join("texture/display_panel"), &target.join("texture/display_panel"))?;
    }
    // a made-up test car: files laid over the real car's (and the leftovers of an earlier one gone)
    let marker = target.join("car_data_overlay.txt");
    if marker.is_file() {
        for line in std::fs::read_to_string(&marker).unwrap_or_default().lines() {
            let _ = std::fs::remove_file(target.join(line));
        }
        let _ = std::fs::remove_file(&marker);
        return prepare_root(root, game, repo, spec, car_data);
    }
    if let Some(overlay) = car_data {
        let mut laid = Vec::new();
        for entry in std::fs::read_dir(overlay).map_err(|e| format!("{}: {e}", overlay.display()))?.filter_map(|e| e.ok()).filter(|e| e.path().is_file()) {
            let name = entry.file_name().to_string_lossy().into_owned();
            let loose = name.ends_with(".png") || name.ends_with(".knh");
            let relative = if loose { name.clone() } else { format!("data/{name}") };
            let bytes = std::fs::read(entry.path()).map_err(|e| e.to_string())?;
            if bytes.is_empty() {
                let _ = std::fs::remove_file(target.join(&relative));
            } else {
                std::fs::write(target.join(&relative), bytes).map_err(|e| e.to_string())?;
            }
            laid.push(relative);
        }
        fn walk(from: &Path, to: &Path, relative: &str, laid: &mut Vec<String>) -> Result<(), String> {
            for entry in std::fs::read_dir(from).map_err(|e| format!("{}: {e}", from.display()))?.filter_map(|e| e.ok()) {
                let name = entry.file_name().to_string_lossy().into_owned();
                let r = if relative.is_empty() { name.clone() } else { format!("{relative}/{name}") };
                if entry.path().is_dir() {
                    walk(&entry.path(), to, &r, laid)?;
                } else {
                    std::fs::create_dir_all(to.join(&r).parent().unwrap()).map_err(|e| e.to_string())?;
                    std::fs::copy(entry.path(), to.join(&r)).map_err(|e| e.to_string())?;
                    laid.push(r);
                }
            }
            Ok(())
        }
        if overlay.join("root").is_dir() {
            walk(&overlay.join("root"), &target, "", &mut laid)?;
        }
        std::fs::write(&marker, laid.join("\n")).map_err(|e| e.to_string())?;
    }
    // the fonts of the dashboard's displays
    copy_folder(&game.join("content/fonts"), &root.join("content/fonts"))?;
    // the driver: its two models, and the texture folders the skin names
    if let Ok(driver) = rustyac_physics::data::ini::IniReader::load(&data.join("driver3d.ini")) {
        let model = driver.get_string("MODEL", "NAME");
        for file in [format!("content/driver/{model}.kn5"), format!("content/driver/{model}_B.kn5")] {
            if game.join(&file).is_file() {
                copy_file(&game.join(&file), &root.join(&file))?;
            }
        }
        if let Ok(skin_ini) = rustyac_physics::data::ini::IniReader::load(&source.join("skins").join(&spec.skin).join("skin.ini")) {
            for part in ["SUIT", "GLOVES", "HELMET"] {
                if !skin_ini.has_key(&model, part) {
                    continue;
                }
                let value = skin_ini.get_string(&model, part).replace('\\', "/");
                let colour = format!("content/texture/driver_{}{value}", part.to_lowercase());
                let normal = match colour.rfind('/') {
                    Some(at) => format!("{}/_nm", &colour[..at]),
                    None => String::new(),
                };
                for folder in [colour, normal] {
                    if !folder.is_empty() && game.join(&folder).is_dir() {
                        copy_folder(&game.join(&folder), &root.join(&folder))?;
                    }
                }
            }
        }
    }
    // textures the car's objects open by a path under the game's folder
    for name in ["content/texture/DAMAGE_GLASS.dds", "content/texture/skids.dds", "content/texture/smoke_0.png", "content/texture/grass.png"] {
        if game.join(name).is_file() {
            copy_file(&game.join(name), &root.join(name))?;
        }
    }
    // the flames' textures
    let flames = format!("content/cars/{}/texture/flames", spec.name);
    if game.join(&flames).is_dir() {
        copy_folder(&game.join(&flames), &root.join(&flames))?;
    }
    let skin = source.join("skins").join(&spec.skin);
    if let Ok(entries) = std::fs::read_dir(&skin) {
        for entry in entries.filter_map(|e| e.ok()).filter(|e| e.path().is_file()) {
            copy_file(&entry.path(), &target.join("skins").join(&spec.skin).join(entry.file_name()))?;
        }
    }
    if let Ok(entries) = std::fs::read_dir(source.join("animations")) {
        for entry in entries.filter_map(|e| e.ok()).filter(|e| e.path().extension().is_some_and(|x| x.eq_ignore_ascii_case("ksanim"))) {
            copy_file(&entry.path(), &target.join("animations").join(entry.file_name()))?;
        }
    }
    Ok(())
}

/// The nodes of `Sim::initSceneGraph` the car's objects hang things on.
pub struct SimNodes {
    pub root: *mut u8,
    pub cars: *mut u8,
    pub skid_marks: *mut u8,
    pub particles: *mut u8,
    pub car_shadows: *mut u8,
    pub before_cars: *mut u8,
    pub render_finished: *mut u8,
    pub blurred: *mut u8,
    pub unblurred: *mut u8,
}

/// The game's objects of one car.
pub struct Car {
    /// the fake `CarAvatar`
    car: *mut u8,
    /// the fake `Game` (its dt)
    game: *mut u8,
    /// `SuspensionAvatar` or `SuspensionAnimator`
    suspension: *mut u8,
    animated: bool,
    lod_manager: *mut u8,
    body_transform: *mut u8,
    steer_transform: *mut u8,
    steer_lock: f32,
    constrained: *mut u8,
    visual_damage: *mut u8,
    rotating_objects: *mut u8,
    car_animations: *mut u8,
    gear_shift_shake: *mut u8,
    analog_instruments: *mut u8,
    brake_lights: *mut u8,
    animated_lights: *mut u8,
    tyre_blur: *mut u8,
    blurred_objects: *mut u8,
    digital_instruments: *mut u8,
    flames: *mut u8,
    /// `MirrorTextureRenderer`, `VirtualMirrorRenderer`, `CarMirrorManager`: null without mirrors
    mirror: *mut u8,
    virtual_mirror: *mut u8,
    mirror_manager: *mut u8,
    /// the four `TyreSmoke` and the `EngineSmoke`, each with its `update`
    smokes: Vec<(*mut u8, usize)>,
    brake_discs: *mut u8,
    dynamic_effects: *mut u8,
    digital_panels: *mut u8,
    race_manager: *mut u8,
    /// stand-ins for the other cars of the real-time order and of the leaderboard
    other_cars: *mut u8,
    wings: *mut u8,
    /// the pause and the replay status of the last frame
    last_pause: std::cell::Cell<bool>,
    last_status: std::cell::Cell<i32>,
}

impl Car {
    /// The fake `CarAvatar`.
    pub fn avatar(&self) -> *mut u8 {
        self.car
    }
}

/// An 8-bit PNG as RGBA bytes.
fn png_rgba(path: &Path) -> Result<Vec<u8>, String> {
    let decoder = png::Decoder::new(std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?);
    let mut reader = decoder.read_info().map_err(|e| e.to_string())?;
    let mut buffer = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buffer).map_err(|e| e.to_string())?;
    buffer.truncate(info.buffer_size());
    Ok(match info.color_type {
        png::ColorType::Rgba => buffer,
        png::ColorType::Rgb => buffer.chunks_exact(3).flat_map(|p| [p[0], p[1], p[2], 255]).collect(),
        other => return Err(format!("{}: a PNG of colour type {other:?}", path.display())),
    })
}

impl Game {
    /// `CarAvatar::onPostLoad` 0x1400d92b0: the flat ground shadows (the game's own
    /// `CarFakeShadow` constructor). A car without `body_shadow.png` gets its five pictures
    /// drawn and saved into the car's folder of the scratch root: the answer is a line with
    /// each one's hash, and the files are taken away again.
    pub unsafe fn post_load_car(&self, c: &Car, name: &str) -> Result<Vec<u8>, String> {
        let folder = std::path::PathBuf::from(format!("content/cars/{name}"));
        let generates = !folder.join("body_shadow.png").is_file();
        let shadow = self.acs.alloc(0x158);
        let ctor: extern "C" fn(*mut u8, *mut u8) -> *mut u8 = std::mem::transmute(self.acs.va(VA_CAR_FAKE_SHADOW_CTOR));
        ctor(shadow, c.car);
        let mut lines = Vec::new();
        if generates {
            let files: Vec<std::path::PathBuf> = (0..4).map(|i| folder.join(format!("tyre_{i}_shadow.png"))).chain([folder.join("body_shadow.png")]).collect();
            for (i, file) in files.iter().enumerate() {
                let picture = png_rgba(file)?;
                lines.extend_from_slice(format!("generated shadow {i}: {} bytes {:016x}
", picture.len(), crate::ac::hash(&picture)).as_bytes());
            }
            for file in files {
                let _ = std::fs::remove_file(file);
            }
        }
        Ok(lines)
    }
}

impl Game {
    /// What has to be there once before any car: patches, the tables of node names, a console.
    pub(crate) unsafe fn prepare_cars(&self) -> *mut u8 {
        let acs = &self.acs;
        // mov rax, rcx; ret: a Font that is never used (the level-of-detail manager's debug text)
        acs.patch(acs.va(VA_FONT_CTOR), &[0x48, 0x89, 0xc8, 0xc3]);
        for va in VA_NAME_TABLE_INITIALISERS {
            let init: extern "C" fn() = std::mem::transmute(acs.va(va));
            init();
        }
        // a Console that only ever gets a variable added
        let console = acs.alloc(0x128);
        wr(console, 0x30, 1u8);
        wr(console, 0x120, 1u8);
        wr(console, 0xe0, 7u64);
        wr(console, 0x118, 7u64);
        acs.set_global(VA_CONSOLE_SINGLETON, console);
        console
    }

    /// `CarAvatar::init3D` 0x1400d3b90 by hand around the game's own constructors.
    pub unsafe fn load_car(&self, spec: &CarSpec, nodes: &SimNodes, camera: *mut u8, console: *mut u8) -> Result<Car, String> {
        let cars_node = nodes.cars;
        let acs = &self.acs;
        // the C runtime's rand() of this thread: the same start as the port's
        let randomize: extern "C" fn(u32) = std::mem::transmute(acs.va(VA_KS_RANDOMIZE));
        randomize(crate::frames::RAND_SEED);
        let data = std::path::PathBuf::from(format!("content/cars/{}/data", spec.name));
        let car_ini = rustyac_physics::data::ini::IniReader::load(&data.join("car.ini"))?;

        let game = acs.alloc(0x278);
        wr(game, 0x138, self.graphics);
        let camera_manager = acs.alloc(0x198);
        let replay_manager = acs.alloc(0x198);
        let sim = acs.alloc(0x2d0);
        wr(sim, 0x08, game);
        wr(sim, 0x188, camera_manager);
        wr(camera_manager, 0x110, sim);
        let camera_drivable = acs.alloc(0x100);
        wr(camera_manager, 0xc0, camera_drivable);
        wr(sim, 0x1b0, replay_manager);
        wr(sim, 0x238, camera);
        wr(sim, 0x110, acs.alloc(0x200)); // pauseMenu: not visible
        // a RaceManager with no client and no timing service (the lap count is the car's own),
        // a PhysicsAvatar whose engine has an air temperature of 0
        let race_manager = acs.alloc(0x400);
        wr(race_manager, 0x168, sim);
        // the session started long ago; no RaceTimingServices (the leaderboard answers -1);
        // carsRealTimePosition (records of 0x10 bytes)
        wr(race_manager, 0x140, -1.0f64);
        wr(race_manager, 0x8, game);
        let other_cars = acs.alloc(0x12a8);
        wr(other_cars, 0x1158, 99i32);
        let real_time = acs.alloc(0x10 * 40);
        wr(race_manager, 0x1a0, real_time);
        wr(race_manager, 0x1a8, real_time.add(0x10 * 40));
        wr(sim, 0x1a8, race_manager);
        wr(sim, 0x1b8, acs.alloc(0x400));
        wr(sim, 0x228, console);
        wr(sim, 0x120, nodes.root);
        wr(sim, 0x130, nodes.cars);
        wr(sim, 0x138, nodes.skid_marks);
        wr(sim, 0x140, nodes.particles);
        wr(sim, 0x148, nodes.car_shadows);
        wr(sim, 0x158, nodes.blurred);
        wr(sim, 0x160, nodes.unblurred);
        wr(sim, 0x170, nodes.render_finished);
        wr(sim, 0x178, nodes.before_cars);
        let car = acs.alloc(0x12a8);
        // Sim::Sim: the mirror texture and the virtual mirror, when video.ini asks for mirrors.
        // A TrackAvatar of which only the ideal line's node is touched; the one car.
        let (mut mirror, mut virtual_mirror) = (std::ptr::null_mut::<u8>(), std::ptr::null_mut::<u8>());
        if crate::root::profile().mirror_size != 0 {
            let track = acs.alloc(0x200);
            wr(track, 0x58, acs.alloc(0xe0));
            wr(sim, 0x150, track);
            let cars = acs.alloc(0x10);
            wr(cars, 0, car);
            wr(sim, 0x208, cars);
            wr(sim, 0x210, cars.add(8));
            mirror = acs.alloc(0x68);
            let ctor: extern "C" fn(*mut u8, *mut u8) -> *mut u8 = std::mem::transmute(acs.va(VA_MIRROR_TEXTURE_RENDERER_CTOR));
            // (GraphicsManager::videoSettings.aaSamples, read here for the multisampled target of
            // [MIRROR] HQ only: the oracle's screen has one sample)
            let hq = crate::root::profile().mirror_hq;
            if hq > 0 {
                wr(self.graphics, 0x10, hq);
            }
            ctor(mirror, sim);
            wr(self.graphics, 0x10, 1i32);
            wr(sim, 0x180, mirror);
            virtual_mirror = acs.alloc(0x78);
            let ctor: extern "C" fn(*mut u8, *mut u8) -> *mut u8 = std::mem::transmute(acs.va(VA_VIRTUAL_MIRROR_CTOR));
            ctor(virtual_mirror, sim);
            wr(sim, 0x280, virtual_mirror);
        }
        wr(car, 0x0, acs.va(VA_CAR_AVATAR_VTABLE));
        wr(car, 0x8, game);
        wr(car, 0x130, sim);
        write_wstring(acs, car.add(0x138), &spec.name);
        wr(car, 0x1d0, [1.0f32, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0]);
        // initCommon: car.ini [BASIC]
        let offset = car_ini.get_float3("BASIC", "GRAPHICS_OFFSET").unwrap_or([0.0; 3]);
        wr(car, 0x104c, offset);
        wr(car, 0x1058, car_ini.get_float("BASIC", "GRAPHICS_PITCH_ROTATION").unwrap_or(0.0) * DEG);
        write_wstring(acs, car.add(0x1138), &spec.skin);
        wr(car, 0x1158, 0i32);

        let car_node = acs.alloc(0x110);
        let sphere_ctor: extern "C" fn(*mut u8, *mut u8, f32) -> *mut u8 = std::mem::transmute(acs.va(VA_NODE_BOUNDING_SPHERE_CTOR));
        sphere_ctor(car_node, wstring(acs, "CARNODE"), 4.0);
        self.add_child(cars_node, car_node);
        wr(car, 0x210, car_node);
        let body_transform = self.node("BODYTR");
        wr(car, 0x1b8, body_transform);
        // initCommon: the detailed driver shows until the first frame says otherwise
        wr(car, 0xfe0, 1u8);
        wr(car, 0x1060, spec.steer_lock);
        wr(car, 0xe44, spec.max_gear);
        wr(car, 0xe40, spec.steer_lock);
        wr(car, 0x218, car_ini.get_float3("GRAPHICS", "DRIVEREYES").unwrap_or([0.0; 3]));
        // CarAvatar::initDriver 0x1400d7380: the two driver models, the first children of BODYTR
        let init_driver: extern "C" fn(*mut u8) = std::mem::transmute(acs.va(VA_CAR_AVATAR_INIT_DRIVER));
        init_driver(car);
        let animated = car_ini.get_int("GRAPHICS", "USE_ANIMATED_SUSPENSIONS").unwrap_or(0) != 0;
        let suspension = if animated {
            let s = acs.alloc(0xf0);
            let ctor: extern "C" fn(*mut u8, *mut u8) -> *mut u8 = std::mem::transmute(acs.va(VA_SUSPENSION_ANIMATOR_CTOR));
            ctor(s, car)
        } else {
            let s = acs.alloc(0xe0);
            let ctor: extern "C" fn(*mut u8, *mut u8) -> *mut u8 = std::mem::transmute(acs.va(VA_SUSPENSION_AVATAR_CTOR));
            ctor(s, car)
        };
        wr(car, 0xff8, suspension.add(0x58));
        let steer_transform = self.node("STEER_TRANSFORM");
        wr(car, 0x1c0, steer_transform);
        self.add_child(body_transform, steer_transform);
        self.add_child(car_node, body_transform);
        let constrained = acs.alloc(0x78);
        let ctor: extern "C" fn(*mut u8, *mut u8) -> *mut u8 = std::mem::transmute(acs.va(VA_CONSTRAINED_CTOR));
        ctor(constrained, car);
        wr(car, 0x1018, constrained);

        // the models of every level, each with the number the game wants to have been told
        let lods = rustyac_physics::data::ini::IniReader::load(&data.join("lods.ini"))?;
        let mut index = 0;
        while lods.has_section(&format!("LOD_{index}")) {
            let file = format!("content/cars/{}/{}", spec.name, lods.get_string(&format!("LOD_{index}"), "FILE"));
            let mut head = [0u8; 14];
            let mut f = std::fs::File::open(&file).map_err(|e| format!("{file}: {e}"))?;
            if std::io::Read::read_exact(&mut f, &mut head).is_ok() && i32::from_le_bytes(head[6..10].try_into().unwrap()) >= 6 {
                let key = u32::from_le_bytes(head[10..14].try_into().unwrap());
                if key != 0 {
                    let add: extern "C" fn(u32) = std::mem::transmute(acs.va(VA_KN5IO_ADD_DLC_KEY));
                    add(key);
                }
            }
            index += 1;
        }
        let lod_manager = acs.alloc(0xb0);
        let ctor: extern "C" fn(*mut u8, *mut u8) -> *mut u8 = std::mem::transmute(acs.va(VA_CAR_LOD_MANAGER_CTOR));
        ctor(lod_manager, car);
        wr(car, 0x1010, lod_manager);

        wr(car_node, 0x108, body_transform); // delegateNode
        let apply_no_cull: extern "C" fn(*mut u8, *mut u8) = std::mem::transmute(acs.va(VA_NODE_BOUNDING_SPHERE_APPLY_NO_CULL));
        apply_no_cull(car_node, std::ptr::null_mut());
        // the seat belt that is not worn: off, and out of the tree
        {
            let vtable = rd::<*const usize>(car_node, 0);
            let find: extern "C" fn(*mut u8, *const u8, bool) -> *mut u8 = std::mem::transmute(*vtable.add(5));
            let n = find(car_node, wstring(acs, "CINTURE_OFF"), true);
            if !n.is_null() {
                wr(n, 0xd8, 0u8);
                let parent: *mut u8 = rd(n, 0xa8);
                if !parent.is_null() {
                    let parent_vtable = rd::<*const usize>(parent, 0);
                    let remove: extern "C" fn(*mut u8, *mut u8) = std::mem::transmute(*parent_vtable.add(4));
                    remove(parent, n);
                }
            }
        }
        // CarAvatar::initMirrorMaterials 0x1400d75f0
        wr(car, 0x120, car_ini.get_float3("GRAPHICS", "MIRROR_POSITION").unwrap_or([0.0; 3]));
        let mut mirror_manager = std::ptr::null_mut::<u8>();
        if !mirror.is_null() {
            mirror_manager = acs.alloc(0x78);
            let ctor: extern "C" fn(*mut u8, *mut u8, *mut u8) -> *mut u8 = std::mem::transmute(acs.va(VA_CAR_MIRROR_MANAGER_CTOR));
            ctor(mirror_manager, car, mirror);
        }
        let visual_damage = acs.alloc(0xd0);
        let ctor: extern "C" fn(*mut u8, *mut u8) -> *mut u8 = std::mem::transmute(acs.va(VA_VISUAL_DAMAGE_CTOR));
        ctor(visual_damage, car);
        let rotating_objects = acs.alloc(0x78);
        let ctor: extern "C" fn(*mut u8, *mut u8) -> *mut u8 = std::mem::transmute(acs.va(VA_ROTATING_OBJECTS_CTOR));
        ctor(rotating_objects, car);
        let double_faced: extern "C" fn(*mut u8) = std::mem::transmute(acs.va(VA_MAKE_TYRES_DOUBLE_FACED_SHADOWS));
        double_faced(car_node);
        // CarAvatar::initPhysics: a Car of which only the steering lock is ever read
        let physics = acs.alloc(0x4000);
        wr(physics, 0x174, spec.steer_lock);
        wr(car, 0x1168, physics);
        // CarAvatar::initCommonPostPhysics 0x1400d6190: the objects of the picture
        // (the skid marks: world detail above 0; 6000 vertices x QUANTITY_MULT, 12000 from detail 3)
        wr(car, 0xe5c, spec.tyre_width);
        let mut smokes: Vec<(*mut u8, usize)> = Vec::new();
        {
            let mut mult = 1.0f32;
            if let Ok(ini) = rustyac_physics::data::ini::IniReader::load(std::path::Path::new("system/cfg/skidmarks.ini")) {
                if ini.has_key("GRAPHICS", "QUANTITY_MULT") {
                    mult = ini.get_float("GRAPHICS", "QUANTITY_MULT").unwrap_or(0.0);
                }
            }
            let detail = crate::root::profile().world_detail;
            for i in 0..4 {
                let _ = &mut smokes;
                if detail > 0 && mult != 0.0 {
                    let size = if detail >= 3 { (mult * 12000.0) as i32 } else { (mult * 6000.0) as i32 };
                    let buffer = acs.alloc(0x198);
                    let ctor: extern "C" fn(*mut u8, *mut u8, u32) -> *mut u8 = std::mem::transmute(acs.va(VA_SKID_MARK_BUFFER_CTOR));
                    ctor(buffer, self.graphics, size as u32);
                    wr(car, 0xe20 + 8 * i, buffer);
                    self.add_child(nodes.skid_marks, buffer);
                }
                let smoke = acs.alloc(0xa8);
                let ctor: extern "C" fn(*mut u8, *mut u8, i32) -> *mut u8 = std::mem::transmute(acs.va(VA_TYRE_SMOKE_CTOR));
                smokes.push((ctor(smoke, car, i as i32), VA_TYRE_SMOKE_UPDATE));
            }
        }
        {
            let smoke = acs.alloc(0x90);
            let ctor: extern "C" fn(*mut u8, *mut u8) -> *mut u8 = std::mem::transmute(acs.va(VA_ENGINE_SMOKE_CTOR));
            smokes.push((ctor(smoke, car), VA_ENGINE_SMOKE_UPDATE));
        }
        let make = |size: usize, va: usize| -> *mut u8 {
            let object = acs.alloc(size);
            let ctor: extern "C" fn(*mut u8, *mut u8) -> *mut u8 = std::mem::transmute(acs.va(va));
            ctor(object, car)
        };
        let car_animations = make(0xa8, VA_CAR_ANIMATIONS_CTOR);
        wr(car, 0x1270, car_animations);
        let gear_shift_shake = make(0x128, VA_GEAR_SHIFT_SHAKE_CTOR);
        let analog_instruments = make(0x490, VA_ANALOG_INSTRUMENTS_CTOR);
        let brake_lights = make(0xf0, VA_CAR_BRAKE_LIGHTS_CTOR);
        // GameObject::addGameObject: the object's parent is the car
        wr(brake_lights, 0x38, car);
        let animated_lights = acs.alloc(0x88);
        let ctor: extern "C" fn(*mut u8, *mut u8) -> *mut u8 = std::mem::transmute(acs.va(VA_ANIMATED_LIGHTS_CTOR));
        ctor(animated_lights, brake_lights);
        let tyre_blur = acs.alloc(0x120);
        let ctor: extern "C" fn(*mut u8, *mut u8) -> *mut u8 = std::mem::transmute(acs.va(VA_TYRE_BLUR_CTOR));
        ctor(tyre_blur, car);
        let blurred_objects = acs.alloc(0x78);
        let ctor: extern "C" fn(*mut u8, *mut u8) -> *mut u8 = std::mem::transmute(acs.va(VA_BLURRED_OBJECTS_CTOR));
        ctor(blurred_objects, car);
        let digital_instruments = make(0xc8, VA_DIGITAL_INSTRUMENTS_CTOR);
        let digital_panels = make(0xd8, VA_DIGITAL_PANELS_CTOR);
        let backfire = make(0x28, VA_BACKFIRE_PARAMS_CTOR);
        wr(car, 0x1068, backfire);
        let flames = make(0x158, VA_FLAMES_CTOR);
        let brake_discs = make(0x148, VA_BRAKE_DISC_GRAPHICS_CTOR);
        let dynamic_effects = make(0x88, VA_DYNAMIC_CAR_EFFECTS_CTOR);
        Ok(Car { car, game, suspension, animated, lod_manager, body_transform, steer_transform, steer_lock: spec.steer_lock, constrained, visual_damage, rotating_objects, car_animations, gear_shift_shake, analog_instruments, brake_lights, animated_lights, tyre_blur, blurred_objects, digital_instruments, flames, brake_discs, dynamic_effects, smokes, mirror, virtual_mirror, mirror_manager, digital_panels, race_manager, other_cars, wings: acs.alloc(0x44 * 8), last_pause: std::cell::Cell::new(false), last_status: std::cell::Cell::new(-1) })
    }

    /// One frame of the car: what `Game::update` and the handlers of `evOnPostUpdate` do to its
    /// nodes. The camera of the frame must be set already.
    /// The mirror's part of `Sim::renderScene` 0x14019e570.
    pub unsafe fn render_mirror(&self, c: &Car, camera: &crate::frames::CameraSpec, dt: f32) {
        if c.mirror.is_null() {
            return;
        }
        let on_board = camera.mode == 0 || (camera.mode == 2 && camera.drivable_mode == 4);
        if !on_board && rd::<u8>(c.mirror, 0x29) != 0 {
            let node: *mut u8 = rd(c.virtual_mirror, 0x58);
            if rd::<u8>(node, 0xd8) == 0 || (camera.mode & !2) != 0 {
                return;
            }
        }
        let render: extern "C" fn(*mut u8, f32) = std::mem::transmute(self.acs.va(VA_MIRROR_TEXTURE_RENDERER_RENDER));
        render(c.mirror, dt);
    }

    /// The handler of `Game::evOnPreGUI` of the virtual mirror.
    pub unsafe fn render_virtual_mirror(&self, c: &Car, dt: f32) {
        if c.virtual_mirror.is_null() {
            return;
        }
        let node: *mut u8 = rd(c.virtual_mirror, 0x58);
        if rd::<u8>(node, 0xd8) != 0 {
            let render: extern "C" fn(*mut u8, f32) = std::mem::transmute(self.acs.va(VA_VIRTUAL_MIRROR_RENDER));
            render(c.virtual_mirror, dt);
        }
    }

    pub unsafe fn update_car(&self, c: &Car, s: &rustyac_render::car::CarPhysicsState, camera: &crate::frames::CameraSpec, extra: &crate::frames::Extra, dt: f32, now_ms: f64) {
        wr(c.game, 0x18, now_ms);
        // the session and where the car stands in it
        {
            wr(c.race_manager, 0xe8, extra.session_type);
            let real_time: *mut u8 = rd(c.race_manager, 0x1a0);
            let sim: *mut u8 = rd(c.car, 0x130);
            // ReplayManager: replayMode, timeMult, isActive, status
            let replay: *mut u8 = rd(sim, 0x1b0);
            wr(replay, 0x148, extra.replay as u8);
            wr(replay, 0x130, extra.replay_scale);
            wr(replay, 0x30, extra.replay as u8);
            // Sim::evOnPauseModeChanged (+0x90) and evOnReplayStatusChanged (+0x78): every
            // handler the car's objects registered (records of 0x28 bytes, the function object
            // at +0x20), in their order
            let fire = |event: usize, payload: *mut u8| {
                let (mut at, end): (*mut u8, *mut u8) = (rd(sim, event), rd(sim, event + 8));
                while at < end {
                    let object: *mut u8 = rd(at, 0x20);
                    let vtable: *const usize = rd(object, 0);
                    let call: extern "C" fn(*mut u8, *mut u8) = std::mem::transmute(*vtable.add(2));
                    call(object, payload);
                    at = at.add(0x28);
                }
            };
            if extra.pause != c.last_pause.get() {
                c.last_pause.set(extra.pause);
                let mut paused = extra.pause as u8;
                fire(0x90, &mut paused);
            }
            if extra.replay_status != c.last_status.get() {
                c.last_status.set(extra.replay_status);
                wr(replay, 0xbc, extra.replay_status);
                let mut payload = [0u8; 12];
                payload[0..4].copy_from_slice(&extra.replay_status.to_le_bytes());
                payload[4..8].copy_from_slice(&extra.replay_scale.to_le_bytes());
                payload[8..12].copy_from_slice(&2.0f32.to_le_bytes());
                fire(0x78, payload.as_mut_ptr());
            }
            // PauseMenu::visible, PhysicsAvatar::engine.ambientTemperature
            wr(rd::<*mut u8>(sim, 0x110), 0x150, extra.pause as u8);
            wr(rd::<*mut u8>(sim, 0x1b8), 0x158, extra.air);
            // CarAvatar::wingsStatus (WingState: 0x44 bytes, the angle at +0xc), physicsInfo
            match extra.wing {
                Some(angle) => {
                    for i in 0..8 {
                        wr(c.wings, 0x44 * i + 0xc, angle);
                    }
                    wr(c.car, 0xfa0, c.wings);
                    wr(c.car, 0xfa8, c.wings.add(0x44 * 8));
                }
                None => {
                    wr(c.car, 0xfa0, 0usize);
                    wr(c.car, 0xfa8, 0usize);
                }
            }
            wr(c.car, 0xf54, extra.kers_max);
            wr(c.car, 0xf58, extra.ers_max);
            wr(c.car, 0xf60, (extra.kers_max > 0.0) as u8);
            // Car::tractionControl (+0xa00) and Car::abs (+0x958): isActive, currentMode, a
            // level curve that is not empty (its vector's begin and end)
            let physics: *mut u8 = rd(c.car, 0x1168);
            for (aid, mode, curve, level) in [(0xa00usize, 0xa20usize, 0xa48usize, extra.tc), (0x958, 0x9f8, 0x998, extra.abs)] {
                wr(physics, aid + 1, (level != 0) as u8);
                wr(physics, mode, level.saturating_sub(1));
                wr(physics, curve, c.other_cars);
                wr(physics, curve + 8, c.other_cars.add(64));
            }
            for i in 0..40usize {
                let who = if i as i32 == extra.position { c.car } else { c.other_cars };
                wr(real_time, 0x10 * i, who);
            }
        }
        // the camera manager of the frame
        let sim: *mut u8 = rd(c.car, 0x130);
        let camera_manager: *mut u8 = rd(sim, 0x188);
        wr(camera_manager, 0x120, camera.mode);
        wr(rd::<*mut u8>(camera_manager, 0xc0), 0x58, camera.drivable_mode);
        let acs = &self.acs;
        let flat = |m: &Mat44f| -> [f32; 16] { std::array::from_fn(|i| m.m[i / 4][i % 4]) };
        // CarAvatar::setNewPhysicsState: the state the physics handed over
        let bytes = s.to_game_bytes();
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), c.car.add(0x268), bytes.len());
        // CarAvatar::updateInPitlaneState 0x1400dd6d0
        wr(c.car, 0x129c, s.tyre_surface_def.iter().any(|d| d.is_pitlane) as u8);
        // the head of CarAvatar::update 0x1400db830: the backfire test, then every handler of
        // evOnBackfireTriggered (car+0xd0: records of 0x28 bytes, the function object at +0x20)
        {
            let params: *mut u8 = rd(c.car, 0x1068);
            let check: extern "C" fn(*mut u8, f32) -> bool = std::mem::transmute(acs.va(VA_BACKFIRE_PARAMS_CHECK));
            if !params.is_null() && 0.0 < s.engine_life_left && check(params, dt) {
                let (mut at, end): (*mut u8, *mut u8) = (rd(c.car, 0xd0), rd(c.car, 0xd8));
                let mut argument = 0u8;
                while at < end {
                    let object: *mut u8 = rd(at, 0x20);
                    let vtable: *const usize = rd(object, 0);
                    let call: extern "C" fn(*mut u8, *mut u8) = std::mem::transmute(*vtable.add(2));
                    call(object, &mut argument);
                    at = at.add(0x28);
                }
            }
        }
        // CarAvatar::update 0x1400db830: the body …
        let make_body_matrix: extern "C" fn(*mut u8, *const u8, *mut u8) = std::mem::transmute(acs.va(VA_CAR_AVATAR_MAKE_BODY_MATRIX));
        make_body_matrix(c.car, c.car.add(0x26c), c.car.add(0x224));
        wr(c.body_transform, 8, rd::<[f32; 16]>(c.car, 0x224));
        // … and the steering wheel
        let org: [f32; 16] = rd(c.car, 0x1d0);
        let a = ((s.steer / c.steer_lock) * c.steer_lock) * DEG;
        let axis = [0.0f32, 0.0, 1.0];
        let mut r = [0.0f32; 16];
        let create_from_axis_angle: extern "C" fn(*mut [f32; 16], *const [f32; 3], f32) -> *mut [f32; 16] = std::mem::transmute(acs.va(VA_CREATE_FROM_AXIS_ANGLE));
        create_from_axis_angle(&mut r, &axis, a);
        let matrix = |f: &[f32; 16]| Mat44f { m: std::array::from_fn(|row| std::array::from_fn(|col| f[row * 4 + col])) };
        wr(c.steer_transform, 8, flat(&xm_matrix_multiply(&matrix(&r), &matrix(&org))));
        // … the driver block of CarAvatar::update (LOCK_STEER, HIDE_ARMS and HIDE_STEER are 0 in
        // the profile): which of the two models shows
        {
            let (hr, lr): (*mut u8, *mut u8) = (rd(c.car, 0xfe8), rd(c.car, 0xff0));
            let is_hr: u8 = rd(c.car, 0xfe0);
            let root_of = |d: *mut u8| rd::<*mut u8>(d, 0x58);
            if !hr.is_null() {
                wr(root_of(hr), 0xd8, is_hr);
            }
            if !lr.is_null() {
                wr(root_of(lr), 0xd8, (is_hr == 0) as u8);
            }
            let shown = if is_hr != 0 { hr } else { lr };
            let guid: i32 = rd(c.car, 0x1158);
            let special = camera.mode == 2 && camera.drivable_mode == 4 && rd::<i32>(sim, 0x220) == guid;
            if !shown.is_null() {
                // DriverModel::setVisible 0x1400fb5a0
                let force_hidden: u8 = rd(shown, 0xb8);
                wr(root_of(shown), 0xd8, (!special && force_hidden == 0) as u8);
                wr(c.steer_transform, 0xd8, !special as u8);
            }
        }
        // … and the skid marks, with the wheels where the last frame left them
        let update_skid_marks: extern "C" fn(*mut u8, f32) = std::mem::transmute(acs.va(VA_CAR_AVATAR_UPDATE_SKID_MARKS));
        update_skid_marks(c.car, dt);
        // the car's objects in the order they were added
        wr(c.game, 0x20, dt);
        for driver in [rd::<*mut u8>(c.car, 0xfe8), rd::<*mut u8>(c.car, 0xff0)] {
            if !driver.is_null() {
                let update: extern "C" fn(*mut u8, f32) = std::mem::transmute(acs.va(VA_DRIVER_MODEL_UPDATE));
                update(driver, dt);
            }
        }
        if !c.animated {
            let update: extern "C" fn(*mut u8, f32) = std::mem::transmute(acs.va(VA_SUSPENSION_AVATAR_UPDATE));
            update(c.suspension, dt);
        }
        let update: extern "C" fn(*mut u8, f32) = std::mem::transmute(acs.va(VA_CAR_LOD_MANAGER_UPDATE));
        update(c.lod_manager, dt);
        if !c.mirror_manager.is_null() {
            let update: extern "C" fn(*mut u8, f32) = std::mem::transmute(acs.va(VA_CAR_MIRROR_MANAGER_UPDATE));
            update(c.mirror_manager, dt);
        }
        for (object, va) in [(c.visual_damage, VA_VISUAL_DAMAGE_UPDATE), (c.rotating_objects, VA_ROTATING_OBJECTS_UPDATE)].into_iter().chain(c.smokes.iter().copied()) {
            // Game::update 0x140243010 leaves out an object whose isActive is off
            if rd::<u8>(object, 0x30) == 0 {
                continue;
            }
            let update: extern "C" fn(*mut u8, f32) = std::mem::transmute(acs.va(va));
            update(object, dt);
        }
        for (object, va) in [
            (c.car_animations, VA_CAR_ANIMATIONS_UPDATE),
            (c.gear_shift_shake, VA_GEAR_SHIFT_SHAKE_UPDATE),
            (c.analog_instruments, VA_ANALOG_INSTRUMENTS_UPDATE),
            (c.brake_lights, VA_CAR_BRAKE_LIGHTS_UPDATE),
            (c.animated_lights, VA_ANIMATED_LIGHTS_UPDATE),
            (c.tyre_blur, VA_TYRE_BLUR_UPDATE),
            (c.blurred_objects, VA_BLURRED_OBJECTS_UPDATE),
            (c.digital_instruments, VA_DIGITAL_INSTRUMENTS_UPDATE),
            (c.digital_panels, VA_DIGITAL_PANELS_UPDATE),
            (c.flames, VA_FLAMES_UPDATE),
            (c.brake_discs, VA_BRAKE_DISC_GRAPHICS_UPDATE),
            (c.dynamic_effects, VA_DYNAMIC_CAR_EFFECTS_UPDATE),
        ] {
            let update: extern "C" fn(*mut u8, f32) = std::mem::transmute(acs.va(va));
            update(object, dt);
        }
        // the handlers of evOnPostUpdate, in the order they were registered
        if c.animated && rd::<u8>(c.suspension, 0x30) != 0 {
            let update: extern "C" fn(*mut u8, f32) = std::mem::transmute(acs.va(VA_SUSPENSION_ANIMATOR_UPDATE));
            update(c.suspension, dt);
        }
        let update_constraints: extern "C" fn(*mut u8, f32) = std::mem::transmute(acs.va(VA_CONSTRAINED_UPDATE_CONSTRAINTS));
        update_constraints(c.constrained, dt);
        let update_lod_visibility: extern "C" fn(*mut u8) = std::mem::transmute(acs.va(VA_CAR_LOD_MANAGER_UPDATE_LOD_VISIBILITY));
        update_lod_visibility(c.lod_manager);
    }
}
