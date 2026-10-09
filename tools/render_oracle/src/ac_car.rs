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
//! Not run, like in the port: the driver, `ConstrainedObjectsManager` (its `addModel` is
//! patched out), the damage, light and mirror helpers.

use std::path::Path;

use rustyac_physics::vecmath::{xm_matrix_multiply, Mat44f};

use crate::ac::{rd, wr, wstring, write_wstring, Game};
use crate::frames::CarSpec;

const VA_FONT_CTOR: usize = 0x1_4020_0bb0; // Font::Font(eFontType, float, bool, bool): DirectWrite, not part of the 3D frame
const VA_CONSOLE_SINGLETON: usize = 0x1_4155_9c08; // static Console* Console::_singleton
const VA_CONSTRAINED_ADD_MODEL: usize = 0x1_4007_7040; // ConstrainedObjectsManager::addModel(Node*)
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
const VA_KN5IO_ADD_DLC_KEY: usize = 0x1_4021_4de0; // static void KN5IO::addDLCKey(unsigned int)

/// The start-up initialisers of the tables of node names (`WHEEL_LF` …) of CarLodManager.obj,
/// SuspensionAvatar.obj and SuspensionAnimator.obj.
const VA_NAME_TABLE_INITIALISERS: [usize; 10] = [0x1_4000_75c0, 0x1_4000_74b0, 0x1_4000_7220, 0x1_4000_e2e0, 0x1_4000_e1d0, 0x1_4000_e0c0, 0x1_4000_dc10, 0x1_4000_dd20, 0x1_4000_dfb0, 0x1_4000_db00];

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

/// The car's files in the oracle's scratch folder: the game's code opens them by relative
/// paths (`content/cars/<car>/…`). The data files are plain ones (from `cardata/` or unpacked
/// from the car's `data.acd` in memory), the models, the skin and the animations are copies.
pub fn prepare_root(root: &Path, game: &Path, repo: &Path, spec: &CarSpec) -> Result<(), String> {
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
    for name in ["car.ini", "lods.ini", "suspensions.ini"] {
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
}

impl Game {
    /// What has to be there once before any car: patches, the tables of node names, a console.
    unsafe fn prepare_cars(&self) {
        let acs = &self.acs;
        // mov rax, rcx; ret: a Font that is never used (the level-of-detail manager's debug text)
        acs.patch(acs.va(VA_FONT_CTOR), &[0x48, 0x89, 0xc8, 0xc3]);
        // ret: the constrained objects (steering rods …) are a later task on both sides
        acs.patch(acs.va(VA_CONSTRAINED_ADD_MODEL), &[0xc3]);
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
    }

    /// `CarAvatar::init3D` 0x1400d3b90 by hand around the game's own constructors.
    pub unsafe fn load_car(&self, spec: &CarSpec, cars_node: *mut u8, camera: *mut u8) -> Result<Car, String> {
        let acs = &self.acs;
        self.prepare_cars();
        let data = std::path::PathBuf::from(format!("content/cars/{}/data", spec.name));
        let car_ini = rustyac_physics::data::ini::IniReader::load(&data.join("car.ini"))?;

        let game = acs.alloc(0x278);
        wr(game, 0x138, self.graphics);
        let camera_manager = acs.alloc(0x198);
        let replay_manager = acs.alloc(0x198);
        let sim = acs.alloc(0x2d0);
        wr(sim, 0x08, game);
        wr(sim, 0x188, camera_manager);
        wr(sim, 0x1b0, replay_manager);
        wr(sim, 0x238, camera);
        let car = acs.alloc(0x12a8);
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
        // a ConstrainedObjectsManager that is never looked into (its addModel is patched out)
        wr(car, 0x1018, acs.alloc(0x78));

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
        let double_faced: extern "C" fn(*mut u8) = std::mem::transmute(acs.va(VA_MAKE_TYRES_DOUBLE_FACED_SHADOWS));
        double_faced(car_node);
        Ok(Car { car, game, suspension, animated, lod_manager, body_transform, steer_transform, steer_lock: spec.steer_lock })
    }

    /// One frame of the car: what `Game::update` and the handlers of `evOnPostUpdate` do to its
    /// nodes. The camera of the frame must be set already.
    pub unsafe fn update_car(&self, c: &Car, spec: &CarSpec, dt: f32) {
        let acs = &self.acs;
        let s = &spec.pose;
        let flat = |m: &Mat44f| -> [f32; 16] { std::array::from_fn(|i| m.m[i / 4][i % 4]) };
        // CarAvatar::setNewPhysicsState: the state the physics handed over
        wr(c.car, 0x26c, flat(&s.world_matrix));
        for i in 0..4 {
            wr(c.car, 0x2ac + 0x40 * i, flat(&s.suspension_matrix[i]));
            wr(c.car, 0x3ac + 0x40 * i, flat(&s.tyre_matrix[i]));
            wr(c.car, 0x4b4 + 4 * i, s.wheel_angular_speed[i]);
        }
        wr(c.car, 0x4c4, s.steer);
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
        // the car's objects in the order they were added
        wr(c.game, 0x20, dt);
        if !c.animated {
            let update: extern "C" fn(*mut u8, f32) = std::mem::transmute(acs.va(VA_SUSPENSION_AVATAR_UPDATE));
            update(c.suspension, dt);
        }
        let update: extern "C" fn(*mut u8, f32) = std::mem::transmute(acs.va(VA_CAR_LOD_MANAGER_UPDATE));
        update(c.lod_manager, dt);
        // the handlers of evOnPostUpdate, in the order they were registered
        if c.animated {
            let update: extern "C" fn(*mut u8, f32) = std::mem::transmute(acs.va(VA_SUSPENSION_ANIMATOR_UPDATE));
            update(c.suspension, dt);
        }
        let update_lod_visibility: extern "C" fn(*mut u8) = std::mem::transmute(acs.va(VA_CAR_LOD_MANAGER_UPDATE_LOD_VISIBILITY));
        update_lod_visibility(c.lod_manager);
    }
}
