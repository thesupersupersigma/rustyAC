// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! Cars with animated suspensions (`car.ini [GRAPHICS] USE_ANIMATED_SUSPENSIONS=1`, the
//! F2004 among them): `SuspensionAnimator` (`addModel` 0x1401b14f0,
//! `suspensionAnimatorUpdate` 0x1401b25a0), `Animation::load` 0x140207b30, `AnimationPlayer`
//! (`setCurrentPos` 0x140209040), `quatpos::lerp` 0x140208d30 with DirectXMath's
//! `XMQuaternionSlerpV` 0x140107300, `XMVectorATan2` 0x140107420, `XMVectorATan` 0x140107670
//! and `XMVectorSin` 0x140107790 as they are compiled into the game, lane by lane.
//!
//! The wheels stay inside the body's hierarchy: arms and hubs are posed by the car's
//! `animations/car_susp_XX.ksanim` from the height of the physics hub, the front hubs get a
//! toe turn, and the spin of the wheels is summed up here from the wheels' speed.

use std::path::Path;

use rustyac_math::asinf;
use rustyac_physics::curve::Curve;
use rustyac_physics::vecmath::{xm_matrix_inverse, xm_matrix_multiply, Mat44f, Vec3f};

use crate::car::{CarPhysicsState, DISC_NAMES, WHEEL_NAMES};
use crate::scene::{NodeId, Scene};

const FILE_NAMES: [&str; 4] = ["SUSP_LF", "SUSP_RF", "SUSP_LR", "SUSP_RR"];
/// The "hub" of a rear wheel is its `SUSP_` node.
const HUB_NAMES: [&str; 4] = ["HUB_LF", "HUB_RF", "SUSP_LR", "SUSP_RR"];

/// `quatpos` (0x28 bytes): a rotation, a place and a scale.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct QuatPos {
    pub quat: [f32; 4],
    pub pos: [f32; 3],
    pub scale: [f32; 3],
}

/// `DirectX::XMVectorATan` 0x140107670, one lane.
fn xm_atan(v: f32) -> f32 {
    const TC0: [u32; 4] = [0xbeaa_aa6c, 0x3e4c_bbe5, 0xbe11_7fc7, 0x3dda_3d83];
    const TC1: [u32; 4] = [0xbd9a_3174, 0x3d2f_c1fe, 0xbc84_6e02, 0x3b3b_d74a];
    let f = f32::from_bits;
    let neg = 0.0f32 - v;
    // maxps(neg, v): the second operand when they are equal or unordered
    let abs_v = if neg > v { neg } else { v };
    let mut sign = if 1.0 < v { 1.0f32 } else { -1.0 };
    let comp = abs_v <= 1.0;
    if comp {
        sign = 0.0;
    }
    let inv = 1.0f32 / v;
    let x = if comp { v } else { inv };
    let x2 = x * x;
    let mut r = f(TC1[3]) * x2;
    r = (r + f(TC1[2])) * x2;
    r = (r + f(TC1[1])) * x2;
    r = (r + f(TC1[0])) * x2;
    r = (r + f(TC0[3])) * x2;
    r = (r + f(TC0[2])) * x2;
    r = (r + f(TC0[1])) * x2;
    r = (r + f(TC0[0])) * x2;
    r += 1.0;
    r *= x;
    let r1 = (f32::from_bits(0x3fc9_0fdb) * sign) - r;
    if sign == 0.0 {
        r
    } else {
        r1
    }
}

/// `DirectX::XMVectorATan2` 0x140107420, one lane.
fn xm_atan2(y: f32, x: f32) -> f32 {
    const PI: u32 = 0x4049_0fdb;
    const PI_OVER_TWO: u32 = 0x3fc9_0fdb;
    const PI_OVER_FOUR: u32 = 0x3f49_0fdb;
    const THREE_PI_OVER_FOUR: u32 = 0x4016_cbe4;
    let y_sign = y.to_bits() & 0x8000_0000;
    let x_is_pos = x.to_bits() & 0x8000_0000 == 0;
    let y_is_zero = y == 0.0;
    let x_is_zero = x == 0.0;
    let x_is_inf = x.abs() == f32::INFINITY;
    let y_is_inf = y.abs() == f32::INFINITY;
    let with_sign = |bits: u32| f32::from_bits(bits | y_sign);
    let special = if y_is_inf {
        Some(if x_is_inf {
            if x_is_pos {
                with_sign(PI_OVER_FOUR)
            } else {
                with_sign(THREE_PI_OVER_FOUR)
            }
        } else {
            with_sign(PI_OVER_TWO)
        })
    } else if y_is_zero {
        Some(if x_is_pos { f32::from_bits(y_sign) } else { with_sign(PI) })
    } else if x_is_zero {
        Some(with_sign(PI_OVER_TWO))
    } else {
        None
    };
    let v = y / x;
    let r0 = xm_atan(v);
    let r1 = if x_is_pos { -0.0f32 } else { with_sign(PI) };
    let r2 = r0 + r1;
    special.unwrap_or(r2)
}

/// `DirectX::XMVectorSin` 0x140107790, one lane.
fn xm_sin(v: f32) -> f32 {
    let f = f32::from_bits;
    let mut q = f(0x3e22_f983) * v;
    // XMVectorRound: the sum with 2^23 and back does the rounding
    if q.abs() <= 8_388_608.0 {
        let magic = f(0x4b00_0000 | (q.to_bits() & 0x8000_0000));
        q = (magic + q) - magic;
    }
    let mut x = v - (q * f(0x40c9_0fdb));
    let sgn = x.to_bits() & 0x8000_0000;
    let c = f(0x4049_0fdb | sgn);
    let abs_x = f(x.to_bits() & 0x7fff_ffff);
    let reflected = c - x;
    if !(abs_x <= f(0x3fc9_0fdb)) {
        x = reflected;
    }
    let x2 = x * x;
    let mut r = f(0xb2cd_365b) * x2;
    r = (r + f(0x3638_b88e)) * x2;
    r = (r + f(0xb950_0bf1)) * x2;
    r = (r + f(0x3c08_8886)) * x2;
    r = (r + f(0xbe2a_aaab)) * x2;
    r += 1.0;
    r * x
}

/// `DirectX::XMQuaternionSlerpV` 0x140107300. The result is not normalised again.
fn xm_quaternion_slerp(q0: &[f32; 4], q1: &[f32; 4], t: f32) -> [f32; 4] {
    let p: [f32; 4] = std::array::from_fn(|k| q0[k] * q1[k]);
    let mut cos = (p[0] + p[2]) + (p[1] + p[3]);
    let v0 = (-t) + 1.0;
    let v1 = t + 0.0;
    let sign = if cos < 0.0 { -1.0f32 } else { 1.0 };
    cos *= sign;
    let linear = !(cos < f32::from_bits(0x3f7f_ff58));
    let sin_omega = (1.0f32 - (cos * cos)).sqrt();
    let omega = xm_atan2(sin_omega, cos);
    let mut s0 = xm_sin(omega * v0) / sin_omega;
    let mut s1 = xm_sin(omega * v1) / sin_omega;
    if linear {
        s0 = v0;
        s1 = v1;
    }
    s1 *= sign;
    std::array::from_fn(|k| (s1 * q1[k]) + (s0 * q0[k]))
}

/// `mat44f::setScale` 0x140209210.
fn set_scale(m: &mut Mat44f, s: [f32; 3]) {
    let row = |x: f32, y: f32, z: f32, k: f32| {
        let (mut x, mut y, mut z) = (x, y, z);
        let len = (((x * x) + (y * y)) + (z * z)).sqrt();
        if len != 0.0 && !len.is_nan() {
            let inv = 1.0f32 / len;
            x = inv * x;
            y *= inv;
            z *= inv;
        }
        (x * k, y * k, z * k)
    };
    let r1 = row(m.m[1][0], m.m[1][1], m.m[1][2], s[1]);
    let r2 = row(-m.m[2][0], -m.m[2][1], -m.m[2][2], s[2]);
    let r0 = row(m.m[0][0], m.m[0][1], m.m[0][2], s[0]);
    (m.m[1][0], m.m[1][1], m.m[1][2]) = r1;
    (m.m[2][0], m.m[2][1], m.m[2][2]) = (-r2.0, -r2.1, -r2.2);
    (m.m[0][0], m.m[0][1], m.m[0][2]) = r0;
}

impl QuatPos {
    /// `quatpos::lerp` 0x140208d30: the matrix between `self` and `b`.
    pub fn lerp(&self, b: &QuatPos, t: f32) -> Mat44f {
        let a = self;
        let q = xm_quaternion_slerp(&a.quat, &b.quat, t);
        // XMMatrixRotationQuaternion, as it is inlined
        let (x, y, z, w) = (q[0], q[1], q[2], q[3]);
        let (x2, y2, z2) = (x + x, y + y, z + z);
        let (xx, yy, zz) = (x2 * x, y2 * y, z2 * z);
        let mut m = Mat44f {
            m: [
                [(1.0 - yy) - zz, (z2 * w) + (y2 * x), (z2 * x) - (y2 * w), 0.0],
                [(y2 * x) - (z2 * w), (1.0 - xx) - zz, (x2 * w) + (z2 * y), 0.0],
                [(y2 * w) + (z2 * x), (z2 * y) - (x2 * w), (1.0 - xx) - yy, 0.0],
                [0.0, 0.0, 0.0, 1.0],
            ],
        };
        for k in 0..3 {
            m.m[3][k] = ((b.pos[k] - a.pos[k]) * t) + a.pos[k];
        }
        let s: [f32; 3] = std::array::from_fn(|k| ((b.scale[k] - a.scale[k]) * t) + a.scale[k]);
        set_scale(&mut m, s);
        m
    }
}

/// `AnimationSet`: the frames of one node.
pub struct AnimationSet {
    pub target_name: String,
    pub frames: Vec<QuatPos>,
    pub is_animated: bool,
}

/// `Animation`: a `.ksanim` file.
#[derive(Default)]
pub struct Animation {
    pub sets: Vec<AnimationSet>,
}

impl Animation {
    /// `Animation::load` 0x140207b30. A file that is not there gives no sets. Version 1
    /// files (matrices in place of rotations) are not read: no stock car has one.
    pub fn load(path: &Path) -> Animation {
        println!("LOADING ANIMATION: {}", path.display());
        let mut animation = Animation::default();
        let Ok(bytes) = std::fs::read(path) else {
            println!("VERSION: 1");
            return animation;
        };
        let i32_at = |at: usize| bytes.get(at..at + 4).map(|b| i32::from_le_bytes([b[0], b[1], b[2], b[3]]));
        let f32_at = |at: usize| f32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]);
        let version = i32_at(0).unwrap_or(1);
        println!("VERSION: {version}");
        if version < 2 {
            println!("ERROR: {} is a version {version} animation, which is not read", path.display());
            return animation;
        }
        let count = i32_at(4).unwrap_or(0);
        let mut at = 8usize;
        for _ in 0..count {
            let Some(name_len) = i32_at(at) else { break };
            at += 4;
            let Some(name) = bytes.get(at..at + name_len as usize) else { break };
            at += name_len as usize;
            let Some(frame_count) = i32_at(at) else { break };
            at += 4;
            let Some(block) = bytes.get(at..at + frame_count as usize * 40) else { break };
            let base = at;
            at += block.len();
            let frames: Vec<QuatPos> = (0..frame_count as usize)
                .map(|k| {
                    let o = base + k * 40;
                    QuatPos { quat: [f32_at(o), f32_at(o + 4), f32_at(o + 8), f32_at(o + 12)], pos: [f32_at(o + 16), f32_at(o + 20), f32_at(o + 24)], scale: [f32_at(o + 28), f32_at(o + 32), f32_at(o + 36)] }
                })
                .collect();
            // a set moves when any frame differs from the first, byte for byte
            let is_animated = block.chunks_exact(40).any(|frame| frame != &block[..40]);
            animation.sets.push(AnimationSet { target_name: name.iter().map(|b| *b as char).collect(), frames, is_animated });
        }
        animation
    }
}

struct AnimationPlayerSet {
    target: NodeId,
    frames: Vec<QuatPos>,
    is_active: bool,
}

/// `AnimationPlayer` (0x28 bytes).
pub struct AnimationPlayer {
    sets: Vec<AnimationPlayerSet>,
    current_pos: f32,
}

impl AnimationPlayer {
    /// `AnimationPlayer::AnimationPlayer` 0x140208570, with only the animated sets.
    fn new(animation: &Animation, scene: &Scene, root: NodeId) -> AnimationPlayer {
        let mut sets = Vec::new();
        for set in &animation.sets {
            let mut found = Vec::new();
            scene.find_children_by_name(root, &set.target_name, &mut found);
            for node in found {
                if set.is_animated {
                    sets.push(AnimationPlayerSet { target: node, frames: set.frames.clone(), is_active: true });
                }
            }
        }
        AnimationPlayer { sets, current_pos: -1.0 }
    }

    /// `AnimationPlayer::setCurrentPos` 0x140209040.
    fn set_current_pos(&mut self, scene: &mut Scene, pos: f32, force: bool) {
        if self.sets.is_empty() {
            return;
        }
        let mut pos = pos;
        if pos > 1.0 {
            pos = 1.0;
        } else if !(pos >= 0.0) {
            pos = 0.0;
        }
        if pos == self.current_pos && !force {
            return;
        }
        let count = self.sets[0].frames.len();
        let n = count as i32;
        let f = n as f32 * pos;
        let mut f0 = f as i32;
        let mut f1 = f0 + 1;
        let mut blend = f - f0 as f32;
        if blend > 1.0 {
            blend = 1.0;
        } else if !(blend >= 0.0) {
            blend = 0.0;
        }
        if (f0 as i64 as u64) >= (count as i64 - 1) as u64 {
            f0 = n - 1;
            f1 = n - 1;
            blend = 0.0;
        }
        if !(pos > 0.0) {
            f0 = 0;
            f1 = 1;
            blend = 0.0;
        }
        for set in &self.sets {
            if !set.is_active {
                continue;
            }
            let (Some(a), Some(b)) = (set.frames.get(f0 as usize), set.frames.get(f1 as usize)) else {
                continue;
            };
            let m = a.lerp(b, blend);
            if scene.nodes[set.target].is_active {
                scene.nodes[set.target].matrix = m;
            }
        }
        self.current_pos = pos;
    }

    /// `AnimationPlayer::isAnimatingNode` 0x140208d00.
    fn is_animating_node(&self, node: NodeId) -> bool {
        self.sets.iter().any(|s| s.target == node && s.is_active)
    }
}

struct TransmissionObject {
    node: NodeId,
    org_matrix: Mat44f,
    use_org_matrix: bool,
    is_left: bool,
}

/// `SuspensionAnimatorLodDef` (0x4b0 bytes): what the animator holds of one level of detail.
struct LodDef {
    players: Vec<AnimationPlayer>,
    hubs: [Option<NodeId>; 4],
    wheels: [Option<NodeId>; 4],
    discs: [Option<NodeId>; 4],
    org_wheel_matrix: [Mat44f; 4],
    rot_accumulator: [f32; 4],
    height_curves: [Curve; 4],
    transmission_objects: Vec<TransmissionObject>,
}

/// `SuspensionAnimator` (0xf0 bytes).
pub struct SuspensionAnimator {
    animations: [Animation; 4],
    lods: Vec<LodDef>,
}

impl SuspensionAnimator {
    /// `SuspensionAnimator::SuspensionAnimator` 0x1401b0770: the four animations of the car.
    pub fn new(folder: &Path) -> SuspensionAnimator {
        let animations = std::array::from_fn(|i| Animation::load(&folder.join(format!("animations/CAR_{}.ksanim", FILE_NAMES[i]))));
        SuspensionAnimator { animations, lods: Vec::new() }
    }

    /// `SuspensionAnimator::addModel` 0x1401b14f0, once per level of detail: the nodes, and
    /// for each wheel the curve from its height to the animation's position.
    /// `SuspensionAnimator::getWheelTransform` 0x1401b2470 for every level of detail.
    pub fn wheel_transforms(&self, wheel: usize) -> Vec<NodeId> {
        self.lods.iter().filter_map(|d| d.wheels[wheel]).collect()
    }

    pub fn add_model(&mut self, scene: &mut Scene, root: NodeId) {
        let mut d = LodDef {
            players: Vec::new(),
            hubs: [None; 4],
            wheels: [None; 4],
            discs: [None; 4],
            org_wheel_matrix: [Mat44f::default(); 4],
            rot_accumulator: [0.0; 4],
            height_curves: std::array::from_fn(|_| Curve::new()),
            transmission_objects: Vec::new(),
        };
        for i in 0..4 {
            d.players.push(AnimationPlayer::new(&self.animations[i], scene, root));
            d.hubs[i] = scene.find_child_by_name(root, HUB_NAMES[i], true);
            if d.hubs[i].is_none() {
                println!("ERROR: SUSPENSION NOT FOUND: {}", HUB_NAMES[i]);
            }
            d.wheels[i] = scene.find_child_by_name(root, WHEEL_NAMES[i], true);
            match d.wheels[i] {
                Some(wheel) => d.org_wheel_matrix[i] = scene.nodes[wheel].matrix,
                None => println!("ERROR:, WHEEL {} NOT FOUND", WHEEL_NAMES[i]),
            }
            d.discs[i] = scene.find_child_by_name(root, DISC_NAMES[i], true);
        }
        for i in 0..4 {
            let Some(wheel) = d.wheels[i] else {
                // the game does not look and would stop here
                continue;
            };
            let mut pos = 0.0f32;
            loop {
                d.players[i].set_current_pos(scene, pos, true);
                let p = scene.local_to_world(wheel, &Vec3f::new(0.0, 0.0, 0.0));
                d.height_curves[i].add_value(p.y, pos);
                pos += f32::from_bits(0x3d4c_cccd);
                if !(pos <= 1.0) {
                    break;
                }
            }
        }
        for (prefix, is_left) in [("TRANSMISSION_L_", true), ("TRANSMISSION_R_", false)] {
            let mut found = Vec::new();
            scene.find_children_by_prefix(root, prefix, &mut found);
            for node in found {
                let use_org_matrix = !d.players.iter().any(|p| p.is_animating_node(node));
                d.transmission_objects.push(TransmissionObject { node, org_matrix: scene.nodes[node].matrix, use_org_matrix, is_left });
            }
        }
        self.lods.push(d);
    }

    /// `SuspensionAnimator::suspensionAnimatorUpdate` 0x1401b25a0, every frame after all
    /// updates: every level of detail, seen or not.
    pub fn update(&mut self, scene: &mut Scene, body: NodeId, s: &CarPhysicsState, dt: f32) {
        // Node::worldToLocal 0x14020e730: through the inverse of the body's world matrix
        let inverse = xm_matrix_inverse(&scene.get_world_matrix(body));
        let i = &inverse.m;
        let to_local = |v: [f32; 3], translate: bool| -> [f32; 3] {
            std::array::from_fn(|j| {
                let r = ((v[1] * i[1][j]) + (v[0] * i[0][j])) + (v[2] * i[2][j]);
                if translate {
                    r + i[3][j]
                } else {
                    r
                }
            })
        };
        for d in &mut self.lods {
            for w in 0..4 {
                let h = &s.suspension_matrix[w].m;
                let p = to_local([h[3][0], h[3][1], h[3][2]], true);
                let pos = d.height_curves[w].get_value(p[1]);
                d.players[w].set_current_pos(scene, pos, true);
            }
            if let (Some(_), Some(_)) = (d.hubs[0], d.hubs[1]) {
                for w in 0..2 {
                    let h = &s.suspension_matrix[w].m;
                    let mut n = to_local([-h[2][0], -h[2][1], -h[2][2]], false);
                    let len = ((n[2] * n[2]) + (n[0] * n[0])).sqrt();
                    if len != 0.0 && !len.is_nan() {
                        let inv = 1.0f32 / len;
                        n[0] *= inv;
                        n[2] *= inv;
                    }
                    let a = asinf(n[0]);
                    let r = Mat44f::create_from_axis_angle(&Vec3f::new(0.0, 1.0, 0.0), -a);
                    let hub = d.hubs[w].expect("checked above");
                    scene.nodes[hub].matrix = xm_matrix_multiply(&r, &scene.nodes[hub].matrix);
                }
            }
            for w in 0..4 {
                d.rot_accumulator[w] = (dt * s.wheel_angular_speed[w]) + d.rot_accumulator[w];
                let r = Mat44f::create_from_axis_angle(&Vec3f::new(1.0, 0.0, 0.0), d.rot_accumulator[w]);
                if let Some(wheel) = d.wheels[w] {
                    scene.nodes[wheel].matrix = xm_matrix_multiply(&r, &d.org_wheel_matrix[w]);
                    if let Some(disc) = d.discs[w] {
                        scene.nodes[disc].matrix = scene.nodes[wheel].matrix;
                    }
                }
            }
            for t in &d.transmission_objects {
                let angle = d.rot_accumulator[if t.is_left { 2 } else { 3 }];
                let r = Mat44f::create_from_axis_angle(&Vec3f::new(1.0, 0.0, 0.0), angle);
                let m = if t.use_org_matrix { t.org_matrix } else { scene.nodes[t.node].matrix };
                scene.nodes[t.node].matrix = xm_matrix_multiply(&r, &m);
            }
        }
    }
}
