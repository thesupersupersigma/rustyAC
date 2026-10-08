// SPDX-License-Identifier: GPL-3.0-or-later

//! A real track for the game's own physics, in this process.
//!
//! The game's `Track` (`Track::Track`), its surface table (`SurfacesManager`, name matching
//! through `TrackAvatar::getSurfaceDescFromMeshName`), its collision meshes
//! (`Track::addSurface` -> `PhysicsCore::createCollisionMesh` -> ODE's triangle mesh and
//! OPCODE's tree), its AI line (`Track::initAISpline`) and its rays (`Track::rayCast`) all run
//! as the game's code. The one thing that is not the game's is the reading of the kn5 files:
//! the game's loader creates Direct3D objects for every material and texture, so the
//! vertices, indices and names come from `rustyac-content`, the same reader the Rust track
//! uses (the game copies the vertices bit for bit, so there is nothing to compute in between).
//!
//! `car_oracle rays` casts a million rays through the game's ODE and the Rust port and
//! compares every answer bit for bit.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use rustyac_physics::track::{load_track, Track, TrackLoadReport};
use rustyac_physics::tyre::SurfaceDef;
use rustyac_physics::vecmath::Vec3f;

use crate::acs::Acs;
use crate::game::{rd, wr};

const VA_TRACK_CTOR: usize = 0x1_4027_7100; // Track::Track(PhysicsEngine*, const wstring& name, const wstring& config)
const VA_TRACK_INIT_AI_SPLINE: usize = 0x1_4027_82a0; // Track::initAISpline()
const VA_TRACK_ADD_SURFACE: usize = 0x1_4027_7e50; // Track::addSurface(const wstring&, float*, int, u16*, int, const SurfaceDef&, uint)
const VA_TRACK_RAY_CAST: usize = 0x1_4027_8bb0; // bool Track::rayCast(const vec3f& org, const vec3f& dir, RayCastResult*, float length)
const VA_SURFACES_MANAGER_CTOR: usize = 0x1_401a_e9b0; // SurfacesManager::SurfacesManager(TrackAvatar*)
const VA_GET_SURFACE_DESC: usize = 0x1_401c_8300; // SurfaceDef TrackAvatar::getSurfaceDescFromMeshName(std::wstring)
const TRACK_SIZE: usize = 0x148;
const TRACK_AVATAR_SIZE: usize = 0x308;
const TA_NAME: usize = 0x10;
const TA_CONFIG: usize = 0xa0;
const TA_SURFACES_MANAGER: usize = 0x250;
pub const SURFACE_SIZE: usize = 0xc8;
const SD_WAV_PITCH_SPEED: usize = 0x80;
const SD_GRIP_MOD: usize = 0x90;
const SD_SECTOR_ID: usize = 0x94;
const SD_DIRT_ADDITIVE_K: usize = 0x98;
const SD_COLLISION_CATEGORY: usize = 0x9c;
const SD_IS_VALID_TRACK: usize = 0xa0;
const SD_BLACK_FLAG_TIME: usize = 0xa4;
const SD_SIN_HEIGHT: usize = 0xa8;
const SD_SIN_LENGTH: usize = 0xac;
const SD_IS_PITLANE: usize = 0xb0;
const SD_DAMPING: usize = 0xb4;
const SD_GRANULARITY: usize = 0xb8;
const SD_VIBRATION_GAIN: usize = 0xbc;
const SD_VIBRATION_LENGTH: usize = 0xc0;
/// `CollisionMeshODE::userPointer`: the mesh's own copy of its `SurfaceDef`.
const CM_USER_POINTER: usize = 0x28;

type V3 = [f32; 3];

const VA_PHYSICS_OBJECT_CTOR: usize = 0x1_402a_c8a0; // PhysicsObject::PhysicsObject(PhysicsEngine&, const PhysicsObjectDesc&, BufferedChannel<mat44f>&)
const VA_MATRIX_QUEUE_VFTABLE: usize = 0x1_404d_f0f8; // Concurrency::concurrent_queue<mat44f>::`vftable'
const VA_PHYSICS_OBJECT_SET_WORLD_MATRIX: usize = 0x1_402a_cb90; // PhysicsObject::setWorldMatrix(const mat44f&)

/// One of the game's `PhysicsObject`s (a loose track object) and the queue it pushes its
/// matrix on after every step.
#[derive(Clone, Copy)]
pub struct GameObject {
    /// `PhysicsObject` (0x20 bytes): +0x10 is its `RigidBodyODE`.
    pub object: *mut u8,
    pub queue: *mut u8,
    /// `TrackObject::orgMatrix`: the node's own matrix.
    pub org_matrix: [[f32; 4]; 4],
}

impl GameObject {
    /// The game's `RigidBodyODE` of the object.
    pub unsafe fn wrapper(&self) -> *mut u8 {
        rd(self.object, 0x10)
    }

    /// ODE's `dxBody`.
    pub unsafe fn body(&self) -> *mut u8 {
        rd(self.wrapper(), 0x8)
    }

    /// ODE's geom of the object's mesh (`collisionMeshes[0]->geomID`).
    pub unsafe fn geom(&self) -> *mut u8 {
        let meshes: *const *const u8 = rd(self.wrapper(), 0x30);
        rd(*meshes, 0x10)
    }
}

/// Makes the track's loose objects with the game's own `PhysicsObject::PhysicsObject`, in the
/// order of the port's loader (the game's order: models in file order, nodes in tree order),
/// each from a `PhysicsObjectDesc` as `TrackObject::TrackObject` fills it: the node's own
/// matrix, the first child mesh's positions and indices, mass 1. Must run before the car is
/// made, as the game loads its track before its cars.
pub unsafe fn create_objects(acs: &Acs, engine: *mut u8, track: &Track) -> Vec<GameObject> {
    let queue_ctor: unsafe extern "system" fn(*mut u8, usize) -> *mut u8 =
        std::mem::transmute(acs.msvcp_function(c"??0_Concurrent_queue_base_v4@details@Concurrency@@IEAA@_K@Z"));
    let ctor: extern "C" fn(*mut u8, *mut u8, *const u8, *mut u8) -> *mut u8 = std::mem::transmute(acs.va(VA_PHYSICS_OBJECT_CTOR));
    let mut out = Vec::with_capacity(track.objects.len());
    for def in &track.objects {
        // BufferedChannel<mat44f>: a concurrent_queue of 0x40-byte items (TrackObject::chIn)
        let queue = acs.alloc(0x28);
        queue_ctor(queue, 0x40);
        wr(queue, 0, acs.va(VA_MATRIX_QUEUE_VFTABLE));
        // PhysicsObjectDesc (0x80 bytes)
        let desc = acs.alloc(0x80);
        desc.write_bytes(0, 0x80);
        wstring_at(acs, desc, &def.name);
        let vertices = acs.alloc(def.vertices.len().max(1) * 12);
        std::ptr::copy_nonoverlapping(def.vertices.as_ptr().cast::<u8>(), vertices, def.vertices.len() * 12);
        let indices = acs.alloc(def.indices.len().max(1) * 2);
        std::ptr::copy_nonoverlapping(def.indices.as_ptr().cast::<u8>(), indices, def.indices.len() * 2);
        wr(desc, 0x20, vertices);
        wr(desc, 0x28, def.vertices.len() as i32);
        wr(desc, 0x30, indices);
        wr(desc, 0x38, def.indices.len() as i32);
        wr(desc, 0x3c, def.matrix.m);
        wr(desc, 0x7c, 1.0f32);
        let object = acs.alloc(0x20);
        ctor(object, engine, desc, queue);
        out.push(GameObject { object, queue, org_matrix: def.matrix.m });
    }
    out
}

/// What the job of a new session does (lambda @ 0x1401c6f40): `TrackObject::resetOrgMatrix`
/// @ 0x1401cf6d0 for every object in order, which is `PhysicsObject::setWorldMatrix` with
/// the matrix the object was made with.
pub unsafe fn reset_objects(acs: &Acs, objects: &[GameObject]) {
    let set_world_matrix: extern "C" fn(*mut u8, *const [[f32; 4]; 4]) = std::mem::transmute(acs.va(VA_PHYSICS_OBJECT_SET_WORLD_MATRIX));
    for object in objects {
        set_world_matrix(object.object, &object.org_matrix);
    }
}

/// The game's `RayCastResult` (0x30 bytes).
#[repr(C)]
struct RayCastResult {
    surface_def: *mut u8,
    pos: V3,
    normal: V3,
    has_hit: u8,
    collision_object: *mut u8,
}

/// A `std::wstring` (VS2013 layout) written into `string` (32 bytes), buffer in the game's heap.
unsafe fn wstring_at(acs: &Acs, string: *mut u8, text: &str) {
    let units: Vec<u16> = text.encode_utf16().collect();
    string.write_bytes(0, 32);
    if units.len() < 8 {
        std::ptr::copy_nonoverlapping(units.as_ptr(), string.cast::<u16>(), units.len());
        wr(string, 0x18, 7usize);
    } else {
        let buffer = acs.alloc((units.len() + 1) * 2);
        std::ptr::copy_nonoverlapping(units.as_ptr(), buffer.cast::<u16>(), units.len());
        wr(string, 0, buffer);
        wr(string, 0x18, units.len());
    }
    wr(string, 0x10, units.len());
}

fn wstring(acs: &Acs, text: &str) -> *mut u8 {
    let string = acs.alloc(32);
    unsafe { wstring_at(acs, string, text) };
    string
}

/// Puts the track's small files where the game looks for them: `content/tracks/<name>/data`
/// and `ai` under the oracle's root, and the game's own `system/data/surfaces.ini`. The kn5
/// models stay where they are.
pub fn prepare_root(root: &Path, track_folder: &Path, layout: &str) -> Result<String, String> {
    let io = |e: std::io::Error| e.to_string();
    let name = track_folder.file_name().ok_or("the track folder has no name")?.to_string_lossy().into_owned();
    // a layout keeps its own data and ai in a folder of its name
    let (to, base) = if layout.is_empty() {
        (root.join("content/tracks").join(&name), track_folder.to_path_buf())
    } else {
        (root.join("content/tracks").join(&name).join(layout), track_folder.join(layout))
    };
    // the scratch root is rewritten below: it must never be a game folder
    if root.join("acs.exe").is_file() || root.join("content").join("cars").is_dir() && root.join("system").join("cfg").is_dir() {
        return Err(format!("{} looks like a game folder: the oracle's root has to be a scratch folder of its own", root.display()));
    }
    for sub in ["data", "ai"] {
        // what an earlier run on another layout or track version left behind must not be read
        let _ = std::fs::remove_dir_all(to.join(sub));
        let from = base.join(sub);
        std::fs::create_dir_all(to.join(sub)).map_err(io)?;
        let Ok(entries) = std::fs::read_dir(&from) else { continue };
        for entry in entries {
            let entry = entry.map_err(io)?;
            if entry.file_type().map_err(io)?.is_file() {
                std::fs::copy(entry.path(), to.join(sub).join(entry.file_name())).map_err(io)?;
            }
        }
    }
    let game = rustyac_physics::track::loader::game_root(track_folder).ok_or_else(|| format!("{} is not inside a game folder", track_folder.display()))?;
    std::fs::create_dir_all(root.join("system/data")).map_err(io)?;
    std::fs::copy(game.join("system/data/surfaces.ini"), root.join("system/data/surfaces.ini")).map_err(io)?;
    Ok(name)
}

/// The game's `SurfaceDef` as the port's.
pub unsafe fn read_surface_def(sd: *const u8) -> SurfaceDef {
    SurfaceDef {
        grip_mod: rd(sd, SD_GRIP_MOD),
        dirt_additive_k: rd(sd, SD_DIRT_ADDITIVE_K),
        sin_height: rd(sd, SD_SIN_HEIGHT),
        sin_length: rd(sd, SD_SIN_LENGTH),
        damping: rd(sd, SD_DAMPING),
        granularity: rd(sd, SD_GRANULARITY),
        is_valid_track: rd::<u8>(sd, SD_IS_VALID_TRACK) != 0,
        is_pitlane: rd::<u8>(sd, SD_IS_PITLANE) != 0,
        vibration_gain: rd(sd, SD_VIBRATION_GAIN),
        vibration_length: rd(sd, SD_VIBRATION_LENGTH),
        wav_pitch_speed: rd(sd, SD_WAV_PITCH_SPEED),
        black_flag_time: rd(sd, SD_BLACK_FLAG_TIME),
        collision_category: rd(sd, SD_COLLISION_CATEGORY),
        sector_id: rd(sd, SD_SECTOR_ID),
        user_pointer: u32::MAX,
    }
}

fn same_bits(a: &SurfaceDef, b: &SurfaceDef) -> bool {
    let floats = |s: &SurfaceDef| {
        [s.grip_mod, s.dirt_additive_k, s.sin_height, s.sin_length, s.damping, s.granularity, s.vibration_gain, s.vibration_length, s.wav_pitch_speed, s.black_flag_time]
            .map(f32::to_bits)
    };
    floats(a) == floats(b)
        && a.is_valid_track == b.is_valid_track
        && a.is_pitlane == b.is_pitlane
        && a.collision_category == b.collision_category
        && a.sector_id == b.sector_id
}

/// Two ways of building a track that no real track uses, for the ray micro-oracle: both
/// sides are built the same odd way.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BuildPlan {
    /// Every n-th mesh goes into sub-space id 0, the static space itself (0: none).
    pub space0_every: usize,
    /// The meshes from this index on are added after a first ray was cast.
    pub late_from: Option<usize>,
}

impl BuildPlan {
    pub fn active(&self) -> bool {
        self.space0_every != 0 || self.late_from.is_some()
    }

    fn space(&self, index: usize, space: u32) -> u32 {
        if self.space0_every != 0 && index % self.space0_every == self.space0_every / 2 {
            0
        } else {
            space
        }
    }

    /// The Rust track built again mesh by mesh under this plan.
    fn rebuild(&self, track: &Track) -> Track {
        let mut out = track.clone();
        out.world = Default::default();
        out.surfaces.clear();
        for (index, (mesh, surface)) in track.world.meshes.iter().zip(&track.surfaces).enumerate() {
            if Some(index) == self.late_from {
                // the first ray: what exists is cleaned
                out.world.clean();
            }
            let data = &mesh.data.mesh;
            out.add_surface(&surface.name, &surface.key, &surface.wav, data.vertices.clone(), data.indices.clone(), &surface.surface_def, self.space(index, mesh.space_id));
        }
        out.world.clean();
        out
    }
}

/// The game's track with Spa (or any track) in it, and the Rust track next to it.
pub struct GameTrack {
    pub track: *mut u8,
    /// The game's collision objects in creation order (`CollisionMeshODE*`), and back.
    pub objects: Vec<*mut u8>,
    pub index_of: HashMap<usize, usize>,
    /// The game's own copy of each mesh's `SurfaceDef` -> the mesh's index.
    pub surface_index_of: HashMap<usize, usize>,
    /// The Rust port of the same track.
    pub rust: Track,
    pub report: TrackLoadReport,
    /// Meshes whose surface the game and the port see differently (should be none).
    pub surface_mismatches: Vec<String>,
    pub seconds_game: f64,
}

/// What a ray of the game found.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GameHit {
    pub pos: V3,
    pub normal: V3,
    /// Index of the mesh in creation order.
    pub mesh: usize,
}

impl GameTrack {
    /// Builds the game's `Track` for the track in `folder` on `engine`. The current
    /// directory must be the oracle's root, prepared with [`prepare_root`].
    ///
    /// `ghost`: every mesh gets collision category 0, which nothing but a ray meets, so the
    /// car's body passes through the track (and the walls) as it does in the Rust port so
    /// far; the sub-space of a mesh is still chosen from its real category.
    pub fn build(acs: &Acs, engine: *mut u8, folder: &Path, layout: &str, ghost: bool) -> Result<GameTrack, String> {
        GameTrack::build_with(acs, engine, folder, layout, ghost, &BuildPlan::default())
    }

    /// [`GameTrack::build`] under a [`BuildPlan`].
    pub fn build_with(acs: &Acs, engine: *mut u8, folder: &Path, layout: &str, ghost: bool, plan: &BuildPlan) -> Result<GameTrack, String> {
        let (mut rust, report) = load_track(folder, layout)?;
        if plan.active() {
            rust = plan.rebuild(&rust);
        }
        let started = std::time::Instant::now();
        unsafe {
            let track = acs.alloc(TRACK_SIZE);
            let ctor: extern "C" fn(*mut u8, *mut u8, *mut u8, *mut u8) -> *mut u8 = std::mem::transmute(acs.va(VA_TRACK_CTOR));
            ctor(track, engine, wstring(acs, &rust.name), wstring(acs, layout));

            // a TrackAvatar of which only the name, the layout and the surfaces exist
            let avatar = acs.alloc(TRACK_AVATAR_SIZE);
            wstring_at(acs, avatar.add(TA_NAME), &rust.name);
            wstring_at(acs, avatar.add(TA_CONFIG), layout);
            let manager_ctor: extern "C" fn(*mut u8, *mut u8) -> *mut u8 = std::mem::transmute(acs.va(VA_SURFACES_MANAGER_CTOR));
            manager_ctor(avatar.add(TA_SURFACES_MANAGER), avatar);

            let get_surface: extern "C" fn(*mut u8, *mut u8, *mut u8) -> *mut u8 = std::mem::transmute(acs.va(VA_GET_SURFACE_DESC));
            let add_surface: extern "C" fn(*mut u8, *mut u8, *const f32, i32, *const u16, i32, *const u8, u32) -> *mut u8 =
                std::mem::transmute(acs.va(VA_TRACK_ADD_SURFACE));
            let mut objects = Vec::with_capacity(rust.surfaces.len());
            let mut index_of = HashMap::new();
            let mut surface_index_of = HashMap::new();
            let mut surface_mismatches = Vec::new();
            let surface = acs.alloc(SURFACE_SIZE);
            for (index, (mesh, ours)) in rust.world.meshes.iter().zip(&rust.surfaces).enumerate() {
                // the game's own surface for this name (the name is handed over by value:
                // the callee frees its buffer)
                surface.write_bytes(0, SURFACE_SIZE);
                get_surface(avatar, surface, wstring(acs, &ours.name));
                let theirs = read_surface_def(surface);
                if !same_bits(&theirs, &ours.surface_def) {
                    surface_mismatches.push(format!("{}: game {theirs:?}, port {:?}", ours.name, ours.surface_def));
                }
                if Some(index) == plan.late_from {
                    // a first ray before the rest of the meshes exist: the static space is
                    // cleaned, and what comes later meets ODE's rule for late geoms
                    let ray_cast: extern "C" fn(*mut u8, *const V3, *const V3, *mut RayCastResult, f32) -> u8 = std::mem::transmute(acs.va(VA_TRACK_RAY_CAST));
                    let mut result = RayCastResult { surface_def: std::ptr::null_mut(), pos: [0.0; 3], normal: [0.0; 3], has_hit: 0, collision_object: std::ptr::null_mut() };
                    ray_cast(track, &[0.0, 5000.0, 0.0], &[0.0, -1.0, 0.0], &mut result, 3.0);
                }
                // TrackAvatar::addPhysicsMesh: walls live in their own sub-spaces
                let id = theirs.sector_id as u32;
                let mut space = if theirs.collision_category == 2 { id.wrapping_add(10_000) } else { id };
                if plan.active() {
                    // (the plan's own sub-space, the same on both sides)
                    space = mesh.space_id;
                }
                if space != mesh.space_id {
                    surface_mismatches.push(format!("{}: sub-space {space} in the game, {} in the port", ours.name, mesh.space_id));
                }
                if ghost {
                    wr(surface, SD_COLLISION_CATEGORY, 0u32);
                }
                let vertices = &mesh.data.mesh.vertices;
                let indices = &mesh.data.mesh.indices;
                let object = add_surface(
                    track,
                    wstring(acs, &ours.name),
                    vertices.as_ptr().cast::<f32>(),
                    vertices.len() as i32,
                    indices.as_ptr(),
                    indices.len() as i32,
                    surface,
                    space,
                );
                index_of.insert(object as usize, index);
                surface_index_of.insert(rd::<usize>(object, CM_USER_POINTER), index);
                objects.push(object);
            }
            let init_ai_spline: extern "C" fn(*mut u8) = std::mem::transmute(acs.va(VA_TRACK_INIT_AI_SPLINE));
            init_ai_spline(track);
            Ok(GameTrack { track, objects, index_of, surface_index_of, rust, report, surface_mismatches, seconds_game: started.elapsed().as_secs_f64() })
        }
    }

    /// `Track::rayCast` (the shared ray; `length` is set on every call).
    pub fn ray_cast(&self, acs: &Acs, org: &V3, dir: &V3, length: f32) -> Option<GameHit> {
        unsafe {
            let ray_cast: extern "C" fn(*mut u8, *const V3, *const V3, *mut RayCastResult, f32) -> u8 = std::mem::transmute(acs.va(VA_TRACK_RAY_CAST));
            let mut result = RayCastResult { surface_def: std::ptr::null_mut(), pos: [0.0; 3], normal: [0.0; 3], has_hit: 0, collision_object: std::ptr::null_mut() };
            if ray_cast(self.track, org, dir, &mut result, length) == 0 {
                return None;
            }
            let mesh = *self.index_of.get(&(result.collision_object as usize)).expect("a hit on a mesh the oracle did not make");
            debug_assert_eq!(rd::<*mut u8>(result.collision_object, CM_USER_POINTER), result.surface_def);
            Some(GameHit { pos: result.pos, normal: result.normal, mesh })
        }
    }

    /// The surface the game attached to a collision object.
    pub unsafe fn surface_of(&self, object: *const u8) -> *const u8 {
        rd(object, CM_USER_POINTER)
    }
}

/// xorshift64*, for rays that are the same on every run.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    /// Uniform in [0, 1).
    fn unit(&mut self) -> f32 {
        (self.next() >> 40) as f32 / 16_777_216.0
    }

    fn range(&mut self, low: f32, high: f32) -> f32 {
        low + (high - low) * self.unit()
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

/// The kinds of rays of the micro-oracle.
const KINDS: [&str; 10] = [
    "tyre ray (3 m straight down from above a random triangle)",
    "tyre ray aimed exactly at a mesh vertex",
    "tyre ray aimed exactly at the middle of a triangle's edge",
    "spawn ray (100 m straight down from 10 m above)",
    "ray in a random direction, 0.5 to 100 m long",
    "nearly flat ray along the road, 5 to 200 m long",
    "ray from below (the back of the road)",
    "tyre ray anywhere inside the track's bounding box",
    "ray without an end (length f32::MAX), straight down from up to 500 m above",
    "ray without an end (length f32::MAX), in a random direction",
];

/// One ray: origin, direction (not normalised: the game does that), length.
fn make_ray(rng: &mut Rng, track: &Track, kind: usize, bounds: &[f32; 6]) -> (V3, V3, f32) {
    let meshes = &track.world.meshes;
    // a random point of a random triangle (meshes weighted equally, so small kerbs and walls
    // get as many rays as the big road meshes)
    let mesh = &meshes[rng.below(meshes.len())].data.mesh;
    let triangle = rng.below(mesh.nb_tris.max(1) as usize) as u32;
    let [a, b, c] = mesh.triangle(triangle);
    let (mut u, mut v) = (rng.unit(), rng.unit());
    if u + v > 1.0 {
        u = 1.0 - u;
        v = 1.0 - v;
    }
    let point = [a[0] + (b[0] - a[0]) * u + (c[0] - a[0]) * v, a[1] + (b[1] - a[1]) * u + (c[1] - a[1]) * v, a[2] + (b[2] - a[2]) * u + (c[2] - a[2]) * v];
    let down = [0.0, -1.0, 0.0];
    match kind {
        0 => ([point[0], point[1] + rng.range(0.2, 2.9), point[2]], down, 3.0),
        1 => {
            let corner = [a, b, c][rng.below(3)];
            ([corner[0], corner[1] + rng.range(0.2, 2.9), corner[2]], down, 3.0)
        }
        2 => {
            let (p, q) = [(a, b), (b, c), (c, a)][rng.below(3)];
            ([(p[0] + q[0]) * 0.5, (p[1] + q[1]) * 0.5 + rng.range(0.2, 2.9), (p[2] + q[2]) * 0.5], down, 3.0)
        }
        3 => ([point[0], point[1] + 10.0, point[2]], down, 100.0),
        4 => {
            let dir = loop {
                let d = [rng.range(-1.0, 1.0), rng.range(-1.0, 1.0), rng.range(-1.0, 1.0)];
                let l = d[0] * d[0] + d[1] * d[1] + d[2] * d[2];
                if l > 0.01 && l <= 1.0 {
                    break d;
                }
            };
            let length = rng.range(0.5, 100.0);
            let back = rng.range(0.0, length);
            ([point[0] - dir[0] * back, point[1] - dir[1] * back + rng.range(0.0, 1.0), point[2] - dir[2] * back], dir, length)
        }
        5 => {
            let heading = rng.range(0.0, std::f32::consts::TAU);
            let dir = [heading.cos(), rng.range(-0.08, 0.02), heading.sin()];
            let length = rng.range(5.0, 200.0);
            let back = rng.range(0.0, length);
            ([point[0] - dir[0] * back, point[1] + rng.range(0.05, 1.5), point[2] - dir[2] * back], dir, length)
        }
        6 => ([point[0], point[1] - rng.range(0.2, 2.9), point[2]], [0.0, 1.0, 0.0], 3.0),
        8 => ([point[0], point[1] + rng.range(0.2, 500.0), point[2]], down, f32::MAX),
        9 => {
            let dir = loop {
                let d = [rng.range(-1.0, 1.0), rng.range(-1.0, 1.0), rng.range(-1.0, 1.0)];
                let l = d[0] * d[0] + d[1] * d[1] + d[2] * d[2];
                if l > 0.01 && l <= 1.0 {
                    break d;
                }
            };
            let back = rng.range(0.0, 300.0);
            ([point[0] - dir[0] * back, point[1] - dir[1] * back + rng.range(0.0, 1.0), point[2] - dir[2] * back], dir, f32::MAX)
        }
        _ => ([rng.range(bounds[0], bounds[1]), rng.range(bounds[2], bounds[3]), rng.range(bounds[4], bounds[5])], down, 3.0),
    }
}

/// `car_oracle rays`: `count` random rays through the game's ODE / OPCODE and through the
/// Rust port; every answer compared bit for bit. Returns the report (markdown) and whether
/// everything was identical.
pub fn rays(acs: &Acs, engine: *mut u8, folder: &Path, layout: &str, count: usize, seed: u64, plan: &BuildPlan) -> Result<(String, bool), String> {
    let game = GameTrack::build_with(acs, engine, folder, layout, false, plan)?;
    let track = &game.rust;
    let mut bounds = [f32::INFINITY, f32::NEG_INFINITY, f32::INFINITY, f32::NEG_INFINITY, f32::INFINITY, f32::NEG_INFINITY];
    for mesh in &track.world.meshes {
        for k in 0..3 {
            bounds[2 * k] = bounds[2 * k].min(mesh.aabb[2 * k]);
            bounds[2 * k + 1] = bounds[2 * k + 1].max(mesh.aabb[2 * k + 1]);
        }
    }
    let mut rng = Rng(seed | 1);
    // (rays, hits in the game, identical answers) per kind
    let mut tally = [(0u64, 0u64, 0u64); KINDS.len()];
    let mut first: Option<String> = None;
    let mut meshes_hit = vec![false; track.surfaces.len()];
    let (mut seconds_game, mut seconds_rust) = (0.0f64, 0.0f64);
    for i in 0..count {
        let kind = match i % 20 {
            0..=5 => 0,
            6 => 1,
            7 => 2,
            8 => 3,
            9..=11 => 4,
            12 | 13 => 5,
            14 => 6,
            15 => 7,
            16 | 17 => 8,
            _ => 9,
        };
        let (org, dir, length) = make_ray(&mut rng, track, kind, &bounds);
        let t0 = std::time::Instant::now();
        let theirs = game.ray_cast(acs, &org, &dir, length);
        let t1 = std::time::Instant::now();
        let ours = track.ray_cast_hit(&Vec3f::new(org[0], org[1], org[2]), &Vec3f::new(dir[0], dir[1], dir[2]), length);
        seconds_game += (t1 - t0).as_secs_f64();
        seconds_rust += t1.elapsed().as_secs_f64();
        let bits = |v: &V3| v.map(f32::to_bits);
        let same = match (&theirs, &ours) {
            (None, None) => true,
            (Some(t), Some(o)) => bits(&t.pos) == bits(&o.contact.pos) && bits(&t.normal) == bits(&o.contact.normal) && t.mesh == o.contact.mesh,
            _ => false,
        };
        let entry = &mut tally[kind];
        entry.0 += 1;
        entry.1 += theirs.is_some() as u64;
        entry.2 += same as u64;
        if let Some(hit) = &theirs {
            meshes_hit[hit.mesh] = true;
        }
        if !same && first.is_none() {
            first = Some(format!(
                "ray {i} ({}): org {org:?} dir {dir:?} length {length:?}\n  game: {theirs:?}\n  port: {:?}",
                KINDS[kind],
                ours.map(|o| (o.contact.pos, o.contact.normal, o.contact.mesh, o.contact.triangle, o.contact.depth))
            ));
        }
    }
    let total: u64 = tally.iter().map(|t| t.0).sum();
    let hits: u64 = tally.iter().map(|t| t.1).sum();
    let same: u64 = tally.iter().map(|t| t.2).sum();
    let mut out = String::new();
    out.push_str(&format!(
        "Track `{}`{}: {} physics meshes, {} triangles, {} sub-spaces; the game built its meshes and trees in {:.2} s, the port in {:.2} s.\n\n",
        track.name,
        if layout.is_empty() { String::new() } else { format!(" layout `{layout}`") },
        game.report.objects, game.report.tris, game.report.spaces.len(), game.seconds_game, game.report.seconds_trees
    ));
    if plan.active() {
        out.push_str(&format!(
            "Built in a way no real track is, the same on both sides: {}{}. Meshes that are direct members of the static space: {}.\n\n",
            if plan.space0_every != 0 { format!("every {}th mesh in sub-space id 0 (the static space itself)", plan.space0_every) } else { String::new() },
            match plan.late_from {
                Some(from) => format!("{}the meshes from number {from} on added after a first ray was cast", if plan.space0_every != 0 { "; " } else { "" }),
                None => String::new(),
            },
            track.world.direct_meshes(),
        ));
    }
    out.push_str(&format!(
        "Surfaces: the game's own `SurfaceDef` for each of the {} mesh names against the port's: **{} differ**.\n\n",
        track.surfaces.len(),
        game.surface_mismatches.len()
    ));
    for line in game.surface_mismatches.iter().take(5) {
        out.push_str(&format!("- {line}\n"));
    }
    out.push_str("| Kind of ray | Rays | Hits (game) | Identical answers | % |\n|---|---|---|---|---|\n");
    for (kind, (n, h, s)) in KINDS.iter().zip(tally) {
        out.push_str(&format!("| {kind} | {n} | {h} | {s} | {:.4} |\n", s as f64 * 100.0 / n.max(1) as f64));
    }
    out.push_str(&format!("| **all** | **{total}** | **{hits}** | **{same}** | **{:.4}** |\n\n", same as f64 * 100.0 / total.max(1) as f64));
    out.push_str(&format!(
        "An answer is identical when both say \"no hit\", or both hit the same mesh and the hit position and normal have the same bits (seed {seed}). \
         {} of the {} meshes were hit at least once. Time for the rays: game {seconds_game:.2} s, port {seconds_rust:.2} s.\n",
        meshes_hit.iter().filter(|h| **h).count(),
        meshes_hit.len()
    ));
    match &first {
        Some(first) => out.push_str(&format!("\nFirst difference:\n\n```\n{first}\n```\n")),
        None => out.push_str("\nNo difference.\n"),
    }
    let ok = same == total && game.surface_mismatches.is_empty();
    Ok((out, ok))
}

/// Where the results of `car_oracle rays` go.
pub fn results_path(repo: &Path, track: &str, layout: &str, plan: &BuildPlan) -> PathBuf {
    let layout = if layout.is_empty() { String::new() } else { format!("_{layout}") };
    let plan = format!("{}{}", if plan.space0_every != 0 { "_space0" } else { "" }, if plan.late_from.is_some() { "_late" } else { "" });
    repo.join(format!("oracle/track/rays_{track}{layout}{plan}.md"))
}
