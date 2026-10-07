// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni). Not covered by rustyAC's licenses — see LICENSING.md.

//! What a car collides with: the floor boxes of `data/colliders.ini`
//! (`CarColliderManager::loadINI` @ 0x1402a37a0) and the collider mesh of
//! `content/cars/<name>/collider.kn5` (`CarAvatar::initPhysics` @ 0x1400d7660,
//! `Car::initColliderMesh` @ 0x140273b20).
//!
//! Both hang on the car's body, in the car's own sub-space of the dynamic space (number
//! `physicsGUID + 1`), with category 4 (a car of this machine):
//!
//! * a **box** collides with category 1 only (a track surface): the boxes are the car's
//!   floor, and the game drops every box contact that does not push up in the body's frame;
//! * the **mesh** collides with walls (2), other cars (4, 8) and loose objects (0x10), and
//!   with track surfaces only while the car lies on its side or roof
//!   (`Car::updateColliderStatus`).

use std::path::{Path, PathBuf};

use crate::data::ini::{append_path, IniReader};
use crate::vecmath::{xm_matrix_multiply, Mat44f, Vec3f};

/// One `[COLLIDER_n]` of `colliders.ini`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BoxColliderDef {
    pub centre: Vec3f,
    pub size: Vec3f,
}

/// `Car::bounds` (`CarBounds`): the box around the collider mesh, moved (not turned) by the
/// mesh's matrix.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CarBounds {
    pub min: Vec3f,
    pub max: Vec3f,
    pub length: f32,
    pub width: f32,
    pub length_front: f32,
    pub length_rear: f32,
}

/// The collider mesh of a car as the game hands it to `Car::initColliderMesh`.
#[derive(Clone, Debug, PartialEq)]
pub struct ColliderMesh {
    /// The positions of the mesh's vertices, in the model's space, exactly as stored.
    pub vertices: Vec<[f32; 3]>,
    pub indices: Vec<u16>,
    /// `CarAvatar::makeBodyMatrix` of the identity: where the model sits in the body's frame.
    pub matrix: Mat44f,
}

/// Everything the car's body collides with.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CarColliders {
    pub boxes: Vec<BoxColliderDef>,
    pub mesh: Option<ColliderMesh>,
}

/// `CarColliderManager::loadINI` @ 0x1402a37a0: `[COLLIDER_0]`, `[COLLIDER_1]` … until a
/// number has no section; of each the `CENTRE` and the `SIZE`.
pub fn load_boxes(data_folder: &Path) -> Result<Vec<BoxColliderDef>, String> {
    let path = append_path(data_folder, "colliders.ini");
    if !path.is_file() {
        return Ok(Vec::new());
    }
    let ini = IniReader::load(&path)?;
    let mut boxes = Vec::new();
    loop {
        let section = format!("COLLIDER_{}", boxes.len());
        if !ini.has_section(&section) {
            break;
        }
        let size = ini.get_float3(&section, "SIZE")?;
        let centre = ini.get_float3(&section, "CENTRE")?;
        boxes.push(BoxColliderDef { centre: Vec3f::new(centre[0], centre[1], centre[2]), size: Vec3f::new(size[0], size[1], size[2]) });
    }
    Ok(boxes)
}

/// `CarAvatar::makeBodyMatrix` @ 0x1400d8ec0 of the identity matrix: the translation by
/// `car.ini [BASIC] GRAPHICS_OFFSET`, then, when `GRAPHICS_PITCH_ROTATION` is not zero, the
/// pitch about the x axis.
pub fn make_body_matrix(graphics_offset: &Vec3f, graphics_pitch_rotation: f32) -> Mat44f {
    let mut m = Mat44f::IDENTITY;
    let (x, y, z) = (graphics_offset.x, graphics_offset.y, graphics_offset.z);
    let s = m.m;
    m.m[3][0] = ((y * s[1][0] + x * s[0][0]) + z * s[2][0]) + s[3][0];
    m.m[3][1] = ((x * s[0][1] + y * s[1][1]) + z * s[2][1]) + s[3][1];
    m.m[3][2] = ((x * s[0][2] + y * s[1][2]) + z * s[2][2]) + s[3][2];
    // ucomiss + je: a pitch of zero (or a NaN) leaves the matrix alone
    if !(graphics_pitch_rotation == 0.0 || graphics_pitch_rotation.is_nan()) {
        let rotation = Mat44f::create_from_axis_angle(&Vec3f::new(1.0, 0.0, 0.0), graphics_pitch_rotation);
        m = xm_matrix_multiply(&rotation, &m);
    }
    m
}

/// The file the game looks for: `content/cars/<name>/collider.kn5` under the game's folder.
pub fn collider_kn5_path(game_root: &Path, car_name: &str) -> PathBuf {
    game_root.join("content").join("cars").join(car_name).join("collider.kn5")
}

/// The mesh the game takes from a `collider.kn5`: the first child of the first child of the
/// file's root node, which has to be a mesh.
pub fn load_collider_kn5(path: &Path) -> Result<(Vec<[f32; 3]>, Vec<u16>), String> {
    let kn5 = rustyac_content::Kn5::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let root = kn5.nodes.first().ok_or_else(|| format!("{}: no nodes", path.display()))?;
    let first = *root.children.first().ok_or_else(|| format!("{}: the root node has no child", path.display()))?;
    let second = *kn5.nodes[first].children.first().ok_or_else(|| format!("{}: the first node has no child", path.display()))?;
    let mesh = kn5.nodes[second].mesh.as_ref().ok_or_else(|| format!("{}: the first node's first child is not a mesh", path.display()))?;
    let mut reader = kn5.reader().map_err(|e| format!("{}: {e}", path.display()))?;
    let vertices = reader.positions(mesh).map_err(|e| format!("{}: {e}", path.display()))?;
    let indices = reader.indices(mesh).map_err(|e| format!("{}: {e}", path.display()))?;
    if !rustyac_ode::collision::TriMeshData::indices_in_range(vertices.len(), &indices) {
        return Err(format!("{}: a triangle names a vertex the mesh does not have", path.display()));
    }
    Ok((vertices, indices))
}

/// The bounds part of `Car::initColliderMesh` @ 0x140273b20.
pub fn bounds(mesh: &ColliderMesh) -> CarBounds {
    let Some(first) = mesh.vertices.first() else {
        return CarBounds::default();
    };
    let (mut min, mut max) = (*first, *first);
    for v in &mesh.vertices {
        for k in 0..3 {
            if v[k] <= min[k] {
                min[k] = v[k];
            }
            // comiss max, v / ja skip
            if !(max[k] > v[k]) {
                max[k] = v[k];
            }
        }
    }
    let t = &mesh.matrix.m[3];
    let max = Vec3f::new(t[0] + max[0], t[1] + max[1], t[2] + max[2]);
    let min = Vec3f::new(t[0] + min[0], t[1] + min[1], t[2] + min[2]);
    CarBounds {
        min,
        max,
        length: (max.z - min.z).abs(),
        width: (max.x - min.x).abs(),
        length_front: max.z.abs(),
        length_rear: min.z.abs(),
    }
}

/// The colliders of the car whose extracted data is in `data_folder` (`car.ini`,
/// `colliders.ini`). The mesh needs the game's folder; without it (or without the file,
/// which the game allows too) the car has only its boxes.
pub fn load(data_folder: &Path, game_root: Option<&Path>, car_name: &str) -> Result<CarColliders, String> {
    let boxes = load_boxes(data_folder)?;
    let mut mesh = None;
    if let Some(root) = game_root {
        let path = collider_kn5_path(root, car_name);
        if path.is_file() {
            let car = IniReader::load(&append_path(data_folder, "car.ini"))?;
            let offset = car.get_float3("BASIC", "GRAPHICS_OFFSET")?;
            let pitch = car.get_float("BASIC", "GRAPHICS_PITCH_ROTATION")?;
            let (vertices, indices) = load_collider_kn5(&path)?;
            // CarAvatar::initCommon @ 0x1400d56e0 keeps the pitch in radians: the degrees of
            // `GRAPHICS_PITCH_ROTATION` times the literal 0.017453 (not pi / 180)
            let matrix = make_body_matrix(&Vec3f::new(offset[0], offset[1], offset[2]), pitch * f32::from_bits(0x3c8e_f998));
            mesh = Some(ColliderMesh { vertices, indices, matrix });
        }
    }
    Ok(CarColliders { boxes, mesh })
}
