//! The ODE micro-oracle: synthetic worlds built and stepped in Assetto Corsa's own ODE and in
//! the Rust port, compared bit for bit after every step.
//!
//! A world is a list of operations made from a seed (bodies, masses, poses, joints) plus,
//! for every step, another list (forces through all seven `dBodyAdd…` functions, now and
//! then a softness change, a rod reseat or a "stop" as the game does them). Both engines
//! get exactly the same operations.

use crate::engine::{BodyState, Engine, JointState, Kind, MassSpec, MassState};
use rustyac_ode::{PARAM_CFM, PARAM_ERP};

/// AC's step.
pub const H: f32 = 0.003;
const GRAVITY: [f32; 3] = [0.0, -9.806, 0.0];
const ERP: f32 = 0.3;
const CFM: f32 = 1e-7;

/// splitmix64
#[derive(Clone)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Rng {
        Rng(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) ^ 0x1234_5678_9abc_def0)
    }
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }
    /// Uniform in [0, 1).
    fn unit(&mut self) -> f32 {
        (self.next() >> 40) as f32 / (1u64 << 24) as f32
    }
    pub fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.unit()
    }
    fn below(&mut self, n: u32) -> u32 {
        (self.next() % n as u64) as u32
    }
    fn chance(&mut self, p: f32) -> bool {
        self.unit() < p
    }
    fn vec(&mut self, size: f32) -> [f32; 3] {
        [self.range(-size, size), self.range(-size, size), self.range(-size, size)]
    }
}

#[derive(Clone, Debug)]
pub enum Op {
    Body,
    Mass(usize, MassSpec),
    Position(usize, [f32; 3]),
    Rotation(usize, [f32; 12]),
    LinearVel(usize, [f32; 3]),
    AngularVel(usize, [f32; 3]),
    FiniteRotation(usize, bool, [f32; 3]),
    Damping(usize, f32, f32),
    AutoDisable(usize, bool),
    Enabled(usize, bool),
    Add(usize, u32, [f32; 3], [f32; 3]),
    /// What `RigidBodyODE::stop` does: velocities and accumulators to zero.
    Stop(usize),
    /// `dBodyDestroy`: the body's joints stay in the world, attached to nothing.
    Destroy(usize),
    /// Body flags set / cleared and `max_angular_speed`, written directly.
    Poke(usize, u32, u32, f32),
    Joint(Kind),
    Attach(usize, Option<usize>, Option<usize>),
    BallAnchor(usize, [f32; 3]),
    DBallAnchor(usize, u32, [f32; 3]),
    DBallDistance(usize, f32),
    Fixed(usize),
    SliderAxis(usize, [f32; 3]),
    Param(usize, i32, f32),
    /// What `PhysicsCore::reseatDistanceJointLocal` does: both anchors given in body
    /// coordinates, converted with `dBodyGetRelPointPos`, then the rod length measured when
    /// the joint was created is put back.
    Reseat { joint: usize, b1: usize, b2: usize, local1: [f32; 3], local2: [f32; 3] },
}

/// Rod lengths remembered at creation (the wrapper `DistanceJointODE` keeps them).
#[derive(Default)]
pub struct Driver {
    rod_length: Vec<Option<f32>>,
}

impl Driver {
    pub fn apply<E: Engine>(&mut self, e: &mut E, op: &Op) {
        match *op {
            Op::Body => {
                e.body_create();
            }
            Op::Mass(b, ref m) => e.body_set_mass(b, m),
            Op::Position(b, p) => e.body_set_position(b, p),
            Op::Rotation(b, ref r) => e.body_set_rotation(b, r),
            Op::LinearVel(b, v) => e.body_set_linear_vel(b, v),
            Op::AngularVel(b, v) => e.body_set_angular_vel(b, v),
            Op::FiniteRotation(b, mode, axis) => e.body_set_finite_rotation(b, mode, axis),
            Op::Damping(b, l, a) => e.body_set_damping(b, l, a),
            Op::AutoDisable(b, on) => e.body_set_auto_disable(b, on),
            Op::Enabled(b, on) => e.body_set_enabled(b, on),
            Op::Add(b, kind, a, p) => e.body_add(b, kind, a, p),
            Op::Destroy(b) => e.body_destroy(b),
            Op::Poke(b, set, clear, max_angular_speed) => e.body_poke(b, set, clear, max_angular_speed),
            Op::Stop(b) => {
                e.body_set_linear_vel(b, [0.0; 3]);
                e.body_set_angular_vel(b, [0.0; 3]);
                e.body_set_accumulators(b, [0.0; 3], [0.0; 3]);
            }
            Op::Joint(kind) => {
                e.joint_create(kind);
                self.rod_length.push(None);
            }
            Op::Attach(j, b1, b2) => e.joint_attach(j, b1, b2),
            Op::BallAnchor(j, p) => e.joint_set_ball_anchor(j, p),
            Op::DBallAnchor(j, which, p) => {
                e.joint_set_dball_anchor(j, which, p);
                if which == 1 {
                    // createDistanceJoint: anchor 1, anchor 2, then remember the length
                    self.rod_length[j] = Some(e.joint_get_dball_distance(j));
                }
            }
            Op::DBallDistance(j, d) => e.joint_set_dball_distance(j, d),
            Op::Fixed(j) => e.joint_set_fixed(j),
            Op::SliderAxis(j, axis) => e.joint_set_slider_axis(j, axis),
            Op::Param(j, parameter, value) => e.joint_set_param(j, parameter, value),
            Op::Reseat { joint, b1, b2, local1, local2 } => {
                let p1 = e.body_get_rel_point_pos(b1, local1);
                let p2 = e.body_get_rel_point_pos(b2, local2);
                e.joint_set_dball_anchor(joint, 0, p1);
                e.joint_set_dball_anchor(joint, 1, p2);
                if let Some(length) = self.rod_length[joint] {
                    e.joint_set_dball_distance(joint, length);
                }
            }
        }
    }
}

/// A synthetic world: how to build it and what to do before each step.
pub struct Scene {
    pub name: String,
    pub setup: Vec<Op>,
    pub bodies: usize,
    pub joints: usize,
    /// (joint, kind, body 1, body 2) for the per-step generator
    joint_list: Vec<(usize, Kind, Option<usize>, Option<usize>)>,
    masses: Vec<f32>,
    rng: Rng,
    /// how eventful the per-step script is
    force_chance: f32,
    /// bodies have ODE's auto-disable on: the script also wakes and disables bodies by hand
    sleepy: bool,
    car: Option<CarLayout>,
    /// bodies that were destroyed on the way (random worlds only)
    pub dead: Vec<bool>,
    /// the step before which one body is destroyed, if any
    destroy_at: Option<usize>,
}

struct CarLayout {
    hubs: [usize; 4],
    body: usize,
    rest: [f32; 4],
    /// the two ends of each front steering rod in the frames of the car body and the hub
    steer: [([f32; 3], [f32; 3]); 2],
}

fn random_rotation(rng: &mut Rng) -> [f32; 12] {
    // a random unit quaternion turned into a matrix here (not through ODE), then left a
    // little off orthonormal so that dBodySetRotation's clean-up has something to do
    let mut q = [rng.range(-1.0, 1.0), rng.range(-1.0, 1.0), rng.range(-1.0, 1.0), rng.range(-1.0, 1.0)];
    let n = (q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]).sqrt().max(1e-3);
    for v in q.iter_mut() {
        *v /= n;
    }
    let (w, x, y, z) = (q[0], q[1], q[2], q[3]);
    let mut r = [
        1.0 - 2.0 * (y * y + z * z),
        2.0 * (x * y - w * z),
        2.0 * (x * z + w * y),
        0.0,
        2.0 * (x * y + w * z),
        1.0 - 2.0 * (x * x + z * z),
        2.0 * (y * z - w * x),
        0.0,
        2.0 * (x * z - w * y),
        2.0 * (y * z + w * x),
        1.0 - 2.0 * (x * x + y * y),
        0.0,
    ];
    if rng.chance(0.5) {
        for k in [0usize, 1, 2, 4, 5, 6, 8, 9, 10] {
            r[k] += rng.range(-1e-3, 1e-3);
        }
    }
    r
}

fn random_mass(rng: &mut Rng) -> MassSpec {
    let mass = rng.range(0.5, 600.0);
    let roll = rng.unit();
    if roll < 0.70 {
        MassSpec::Box { mass, lx: rng.range(0.1, 2.5), ly: rng.range(0.1, 2.5), lz: rng.range(0.1, 4.0) }
    } else if roll < 0.95 {
        let d = [mass * rng.range(0.02, 1.5), mass * rng.range(0.02, 1.5), mass * rng.range(0.02, 1.5)];
        let off = |rng: &mut Rng, a: f32, b: f32| rng.range(-0.3, 0.3) * (a * b).sqrt();
        let i12 = off(rng, d[0], d[1]);
        let i13 = off(rng, d[0], d[2]);
        let i23 = off(rng, d[1], d[2]);
        MassSpec::Parameters { mass, i: [d[0], d[1], d[2], i12, i13, i23] }
    } else {
        MassSpec::ExplicitBug { mass, i: [rng.range(1.0, 500.0), rng.range(1.0, 500.0), rng.range(1.0, 500.0)] }
    }
}

struct BodyPlan {
    position: [f32; 3],
}

/// Adds one body with random properties to the set-up list; `ac_like` gives it exactly what
/// `RigidBodyODE::RigidBodyODE` gives every body of the game.
fn add_body(setup: &mut Vec<Op>, rng: &mut Rng, index: usize, spread: f32, ac_like: bool, masses: &mut Vec<f32>) -> BodyPlan {
    setup.push(Op::Body);
    let roll = rng.unit();
    if ac_like || roll < 0.70 {
        setup.push(Op::FiniteRotation(index, true, [0.0; 3]));
    } else if roll < 0.85 {
        setup.push(Op::FiniteRotation(index, true, rng.vec(1.0)));
    }
    if ac_like || rng.chance(0.9) {
        setup.push(Op::Damping(index, 0.0, 0.0));
    } else {
        setup.push(Op::Damping(index, rng.range(0.0, 0.05), rng.range(0.0, 0.05)));
    }
    let mass = if ac_like {
        MassSpec::Box { mass: rng.range(0.5, 600.0), lx: rng.range(0.1, 2.5), ly: rng.range(0.1, 2.5), lz: rng.range(0.1, 4.0) }
    } else {
        random_mass(rng)
    };
    masses.push(match mass {
        MassSpec::Box { mass, .. } | MassSpec::Parameters { mass, .. } | MassSpec::ExplicitBug { mass, .. } => mass,
    });
    setup.push(Op::Mass(index, mass));
    let position = rng.vec(spread);
    setup.push(Op::Position(index, position));
    setup.push(Op::Rotation(index, random_rotation(rng)));
    setup.push(Op::LinearVel(index, rng.vec(3.0)));
    setup.push(Op::AngularVel(index, rng.vec(4.0)));
    BodyPlan { position }
}

/// Adds one joint between two bodies (or a body and the world) the way the game's factories
/// do it: create, attach, set the anchors.
#[allow(clippy::too_many_arguments)]
fn add_joint(
    setup: &mut Vec<Op>,
    rng: &mut Rng,
    joints: &mut Vec<(usize, Kind, Option<usize>, Option<usize>)>,
    kind: Kind,
    b1: Option<usize>,
    b2: Option<usize>,
    p1: [f32; 3],
    p2: [f32; 3],
    plain: bool,
) {
    let j = joints.len();
    setup.push(Op::Joint(kind));
    setup.push(Op::Attach(j, b1, b2));
    let near = |rng: &mut Rng, p: [f32; 3]| {
        let d = rng.vec(0.6);
        [p[0] + d[0], p[1] + d[1], p[2] + d[2]]
    };
    match kind {
        Kind::DBall => {
            setup.push(Op::DBallAnchor(j, 0, near(rng, p1)));
            setup.push(Op::DBallAnchor(j, 1, near(rng, p2)));
            if !plain && rng.chance(0.15) {
                setup.push(Op::DBallDistance(j, rng.range(0.2, 1.5)));
            }
        }
        Kind::Ball => {
            let mid = [(p1[0] + p2[0]) * 0.5, (p1[1] + p2[1]) * 0.5, (p1[2] + p2[2]) * 0.5];
            setup.push(Op::BallAnchor(j, near(rng, mid)));
        }
        Kind::Fixed => setup.push(Op::Fixed(j)),
        Kind::Slider => {
            let mut axis = rng.vec(1.0);
            if axis.iter().all(|v| v.abs() < 0.05) {
                axis[1] = 1.0;
            }
            setup.push(Op::SliderAxis(j, axis));
        }
    }
    if !plain {
        let roll = rng.unit();
        if roll < 0.3 {
            // what the game's setERPCFM does
            setup.push(Op::Param(j, PARAM_ERP, 0.3));
            setup.push(Op::Param(j, PARAM_CFM, 1e-7));
        } else if roll < 0.45 {
            setup.push(Op::Param(j, PARAM_ERP, rng.range(0.05, 0.95)));
            setup.push(Op::Param(j, PARAM_CFM, 10f32.powf(rng.range(-9.0, -4.0))));
        }
    }
    joints.push((j, kind, b1, b2));
}

impl Scene {
    /// One free body: gravity, forces and torques, the gyroscopic term, the rotation update.
    pub fn free_body(seed: u64) -> Scene {
        let mut rng = Rng::new(seed ^ 0x0f00);
        let mut setup = Vec::new();
        let mut masses = Vec::new();
        add_body(&mut setup, &mut rng, 0, 2.0, seed % 2 == 0, &mut masses);
        Scene {
            name: format!("free_body#{seed}"),
            setup,
            bodies: 1,
            joints: 0,
            joint_list: Vec::new(),
            masses,
            rng,
            force_chance: 0.6,
            sleepy: false,
            car: None,
            dead: Vec::new(),
            destroy_at: None,
        }
    }

    /// Two to five bodies with ODE's auto-disable on (the game uses it for movable track
    /// objects): a damped chain hanging from the world comes to rest and falls asleep, the
    /// script wakes and disables bodies by hand now and then.
    pub fn auto_disable(seed: u64) -> Scene {
        let mut rng = Rng::new(seed ^ 0xa070_0000);
        let mut setup = Vec::new();
        let mut masses = Vec::new();
        let mut joint_list = Vec::new();
        let nb = 2 + rng.below(4) as usize;
        let mut plans = Vec::new();
        for i in 0..nb {
            plans.push(add_body(&mut setup, &mut rng, i, 1.0, true, &mut masses));
            setup.push(Op::LinearVel(i, rng.vec(0.3)));
            setup.push(Op::AngularVel(i, rng.vec(0.3)));
            setup.push(Op::Damping(i, rng.range(0.02, 0.3), rng.range(0.02, 0.3)));
            setup.push(Op::AutoDisable(i, true));
        }
        // hung from the world, then a chain
        let anchor = plans[0].position;
        let kind = if rng.chance(0.5) { Kind::Ball } else { Kind::Fixed };
        add_joint(&mut setup, &mut rng, &mut joint_list, kind, Some(0), None, anchor, anchor, true);
        for i in 1..nb {
            let kind = match rng.below(3) {
                0 => Kind::Ball,
                1 => Kind::Fixed,
                _ => Kind::DBall,
            };
            let (p1, p2) = (plans[i - 1].position, plans[i].position);
            add_joint(&mut setup, &mut rng, &mut joint_list, kind, Some(i - 1), Some(i), p1, p2, true);
        }
        if rng.chance(0.3) {
            setup.push(Op::Enabled(rng.below(nb as u32) as usize, false));
        }
        let joints = joint_list.len();
        Scene {
            name: format!("auto_disable#{seed}"),
            setup,
            bodies: nb,
            joints,
            joint_list,
            masses,
            rng,
            force_chance: 0.01,
            sleepy: true,
            car: None,
            dead: Vec::new(),
            destroy_at: None,
        }
    }

    /// A rod of length zero: both anchors of a DBall at the same point, so the direction of
    /// the constraint row cannot come from the anchors. ODE then takes the direction of the
    /// anchors' relative velocity, and (1, 0, 0) if that is zero too (even seeds start at
    /// rest in free fall, so the very first steps use that last resort).
    pub fn dball_zero(seed: u64) -> Scene {
        let mut rng = Rng::new(seed ^ 0xdba1_0000);
        let mut setup = Vec::new();
        let mut masses = Vec::new();
        let mut joint_list = Vec::new();
        let a = add_body(&mut setup, &mut rng, 0, 0.5, true, &mut masses);
        let to_world = seed % 4 == 3;
        let mut bodies = 1;
        if !to_world {
            add_body(&mut setup, &mut rng, 1, 0.5, true, &mut masses);
            bodies = 2;
        }
        if seed % 2 == 0 {
            for b in 0..bodies {
                setup.push(Op::LinearVel(b, [0.0; 3]));
                setup.push(Op::AngularVel(b, [0.0; 3]));
                setup.push(Op::Mass(b, MassSpec::Box { mass: 10.0, lx: 0.5, ly: 0.5, lz: 0.5 }));
            }
        }
        let point = [a.position[0] + 0.2, a.position[1], a.position[2] - 0.1];
        setup.push(Op::Joint(Kind::DBall));
        setup.push(Op::Attach(0, Some(0), if to_world { None } else { Some(1) }));
        setup.push(Op::DBallAnchor(0, 0, point));
        setup.push(Op::DBallAnchor(0, 1, point));
        joint_list.push((0, Kind::DBall, Some(0), if to_world { None } else { Some(1) }));
        Scene {
            name: format!("dball_zero#{seed}"),
            setup,
            bodies,
            joints: 1,
            joint_list,
            masses,
            rng,
            force_chance: if seed % 2 == 0 { 0.0 } else { 0.3 },
            sleepy: false,
            car: None,
            dead: Vec::new(),
            destroy_at: None,
        }
    }

    /// One joint of one type: between two bodies, or (every third seed) between a body and
    /// the world, attached as first or as second body.
    pub fn single_joint(kind: Kind, seed: u64) -> Scene {
        let mut rng = Rng::new(seed ^ 0x1000 ^ ((kind as u64) << 20));
        let mut setup = Vec::new();
        let mut masses = Vec::new();
        let mut joint_list = Vec::new();
        let ac_like = seed % 2 == 0;
        let a = add_body(&mut setup, &mut rng, 0, 1.0, ac_like, &mut masses);
        let to_world = seed % 3 == 2;
        let mut bodies = 1;
        if to_world {
            let anchor = rng.vec(1.0);
            let (b1, b2) = if seed % 2 == 0 { (Some(0), None) } else { (None, Some(0)) };
            add_joint(&mut setup, &mut rng, &mut joint_list, kind, b1, b2, a.position, anchor, ac_like);
        } else {
            let b = add_body(&mut setup, &mut rng, 1, 1.0, ac_like, &mut masses);
            bodies = 2;
            add_joint(&mut setup, &mut rng, &mut joint_list, kind, Some(0), Some(1), a.position, b.position, ac_like);
        }
        let name = format!("{kind:?}#{seed}").to_lowercase();
        Scene { name, setup, bodies, joints: 1, joint_list, masses, rng, force_chance: 0.6, sleepy: false, car: None, dead: Vec::new(), destroy_at: None }
    }

    /// 1 to 8 bodies joined by random chains of the four joint types, with extra rods that
    /// close loops, a few joints to the world, and usually more than one island.
    pub fn random(seed: u64) -> Scene {
        let mut rng = Rng::new(seed ^ 0x5eed_0000);
        let mut setup = Vec::new();
        let mut masses = Vec::new();
        let mut joint_list = Vec::new();
        let nb = 1 + rng.below(8) as usize;
        let ac_like = rng.chance(0.5);
        let mut plans = Vec::new();
        for i in 0..nb {
            plans.push(add_body(&mut setup, &mut rng, i, 2.0, ac_like, &mut masses));
        }
        let pick_kind = |rng: &mut Rng| match rng.unit() {
            x if x < 0.5 => Kind::DBall,
            x if x < 0.7 => Kind::Ball,
            x if x < 0.85 => Kind::Fixed,
            _ => Kind::Slider,
        };
        // a tree (or a forest: some bodies stay on their own)
        for i in 1..nb {
            if rng.chance(0.8) {
                let other = rng.below(i as u32) as usize;
                let kind = pick_kind(&mut rng);
                let (b1, b2) = if rng.chance(0.5) { (i, other) } else { (other, i) };
                let (p1, p2) = (plans[b1].position, plans[b2].position);
                add_joint(&mut setup, &mut rng, &mut joint_list, kind, Some(b1), Some(b2), p1, p2, ac_like);
            }
        }
        // extra rods (several between the same pair is what a wishbone suspension is)
        if nb >= 2 {
            for _ in 0..rng.below(5) {
                let b1 = rng.below(nb as u32) as usize;
                let mut b2 = rng.below(nb as u32) as usize;
                if b1 == b2 {
                    b2 = (b1 + 1) % nb;
                }
                let kind = if rng.chance(0.85) { Kind::DBall } else { Kind::Ball };
                let (p1, p2) = (plans[b1].position, plans[b2].position);
                add_joint(&mut setup, &mut rng, &mut joint_list, kind, Some(b1), Some(b2), p1, p2, ac_like);
            }
        }
        // a joint to the world now and then
        if rng.chance(0.15) {
            let b = rng.below(nb as u32) as usize;
            let kind = pick_kind(&mut rng);
            let anchor = rng.vec(2.0);
            let (b1, b2) = if rng.chance(0.5) { (Some(b), None) } else { (None, Some(b)) };
            let p = plans[b].position;
            add_joint(&mut setup, &mut rng, &mut joint_list, kind, b1, b2, p, anchor, ac_like);
        }
        // body states no linked function can produce, but which the stepper has code for:
        // no gravity, no gyroscopic term, a cap on the angular speed
        if !ac_like {
            for i in 0..nb {
                let roll = rng.unit();
                if roll < 0.06 {
                    setup.push(Op::Poke(i, 8, 0, f32::INFINITY));
                } else if roll < 0.12 {
                    setup.push(Op::Poke(i, 0, 0x100, f32::INFINITY));
                } else if roll < 0.18 {
                    setup.push(Op::Poke(i, 0x80, 0, rng.range(0.5, 3.0)));
                }
            }
        }
        // one world in eight has ODE's auto-disable on, with damping so that bodies can
        // come to rest; and a body may start disabled
        let sleepy = rng.chance(0.125);
        for i in 0..nb {
            if sleepy {
                setup.push(Op::Damping(i, rng.range(0.02, 0.3), rng.range(0.02, 0.3)));
                setup.push(Op::AutoDisable(i, true));
            }
            if rng.chance(0.04) {
                setup.push(Op::Enabled(i, false));
            }
        }
        let joints = joint_list.len();
        Scene {
            name: format!("random#{seed}"),
            setup,
            bodies: nb,
            joints,
            joint_list,
            masses,
            rng,
            force_chance: if sleepy { 0.02 } else { 0.4 },
            sleepy,
            car: None,
            dead: Vec::new(),
            destroy_at: None,
        }
        .with_destruction(seed)
    }

    /// The layout of an F2004: car body (510 kg box), fuel tank bolted to it, four hubs, each
    /// held by five rods (two wishbones and a steering rod), created in the game's order.
    /// The per-step script holds it up with simple spring, damper and "tyre" forces computed
    /// from the reference engine's state and turns the front steering rods.
    pub fn car(seed: u64) -> Scene {
        let mut rng = Rng::new(seed ^ 0xca70_0000);
        let mut setup = Vec::new();
        let mut joint_list = Vec::new();
        let jitter = |rng: &mut Rng, v: f32| if seed == 0 { v } else { v * rng.range(0.9, 1.1) };
        let body_y = 0.33;
        // body 0: the car body, body 1: the fuel tank
        let masses = vec![jitter(&mut rng, 510.0), 59.2, 19.0, 19.0, 28.5, 28.5];
        for (i, &mass) in masses.iter().enumerate().take(2) {
            setup.push(Op::Body);
            setup.push(Op::FiniteRotation(i, true, [0.0; 3]));
            setup.push(Op::Damping(i, 0.0, 0.0));
            let spec = if i == 0 {
                MassSpec::Box { mass, lx: 1.4, ly: 0.75, lz: 3.3 }
            } else {
                MassSpec::Box { mass, lx: 0.5, ly: 0.5, lz: 0.5 }
            };
            setup.push(Op::Mass(i, spec));
        }
        setup.push(Op::Position(0, [0.0, body_y, 0.0]));
        setup.push(Op::Position(1, [0.0, body_y - 0.1, -0.35]));
        // joint 0: the fuel tank bolt, body1 = tank, body2 = car body
        setup.push(Op::Joint(Kind::Fixed));
        setup.push(Op::Attach(0, Some(1), Some(0)));
        setup.push(Op::Fixed(0));
        joint_list.push((0, Kind::Fixed, Some(1), Some(0)));
        // hubs: LF, RF, LR, RR (+x is the car's left)
        let corners = [(0.75f32, 1.65f32, 0.33f32), (-0.75, 1.65, 0.33), (0.7, -1.4, 0.34), (-0.7, -1.4, 0.34)];
        let mut hubs = [0usize; 4];
        let mut steer = [([0.0f32; 3], [0.0f32; 3]); 2];
        for (w, &(x, z, radius)) in corners.iter().enumerate() {
            let hub = 2 + w;
            hubs[w] = hub;
            setup.push(Op::Body);
            setup.push(Op::FiniteRotation(hub, true, [0.0; 3]));
            setup.push(Op::Damping(hub, 0.0, 0.0));
            setup.push(Op::Mass(hub, MassSpec::Box { mass: masses[hub], lx: 0.2, ly: 0.6, lz: 0.6 }));
            let hub_pos = [x, radius, z];
            setup.push(Op::Position(hub, hub_pos));
            let side = x.signum();
            let inner = x - side * jitter(&mut rng, 0.45);
            // car-side point, hub-side point: top rear, top front, bottom rear, bottom front, steer
            let rods = [
                ([inner, radius + 0.14, z - 0.25], [x - side * 0.08, radius + 0.13, z - 0.01]),
                ([inner, radius + 0.15, z + 0.22], [x - side * 0.08, radius + 0.13, z - 0.01]),
                ([inner - side * 0.12, radius - 0.13, z - 0.3], [x - side * 0.06, radius - 0.14, z]),
                ([inner - side * 0.12, radius - 0.12, z + 0.28], [x - side * 0.06, radius - 0.14, z]),
                ([inner, radius - 0.02, z + 0.12], [x - side * 0.07, radius - 0.01, z + 0.11]),
            ];
            if w < 2 {
                let (c, h) = rods[4];
                steer[w] = ([c[0], c[1] - body_y, c[2]], [h[0] - hub_pos[0], h[1] - hub_pos[1], h[2] - hub_pos[2]]);
            }
            for (car_point, hub_point) in rods {
                let j = joint_list.len();
                setup.push(Op::Joint(Kind::DBall));
                setup.push(Op::Attach(j, Some(0), Some(hub)));
                setup.push(Op::DBallAnchor(j, 0, car_point));
                setup.push(Op::DBallAnchor(j, 1, hub_point));
                joint_list.push((j, Kind::DBall, Some(0), Some(hub)));
            }
            // Suspension::Suspension ends with setERPCFM(0.3, baseCFM) on its five joints
            for j in joint_list.len() - 5..joint_list.len() {
                setup.push(Op::Param(j, PARAM_ERP, 0.3));
                setup.push(Op::Param(j, PARAM_CFM, 1e-7));
            }
        }
        if seed != 0 {
            setup.push(Op::LinearVel(0, [rng.range(-1.0, 1.0), 0.0, rng.range(0.0, 40.0)]));
        }
        let joints = joint_list.len();
        let name = format!("car#{seed}");
        let car = CarLayout { hubs, body: 0, rest: [0.0; 4], steer };
        Scene { name, setup, bodies: 6, joints, joint_list, masses, rng, force_chance: 0.0, sleepy: false, car: Some(car), dead: Vec::new(), destroy_at: None }
    }

    /// A strut (MacPherson) car as the game builds one: car body, fuel tank, and per corner a
    /// hub and a strut body, a slider between strut and hub, a ball joint between car body and
    /// strut top, and three rods (two for the lower wishbone, one for the steering).
    /// 10 bodies, 21 joints, 50 constraint rows. Driven like [`Scene::car`].
    pub fn strut_car(seed: u64) -> Scene {
        let mut rng = Rng::new(seed ^ 0x5707_0000);
        let mut setup = Vec::new();
        let mut joint_list = Vec::new();
        let jitter = |rng: &mut Rng, v: f32| if seed == 0 { v } else { v * rng.range(0.9, 1.1) };
        let body_y = 0.4;
        let mut masses = vec![jitter(&mut rng, 1100.0), 45.0];
        for (i, &mass) in masses.iter().enumerate() {
            setup.push(Op::Body);
            setup.push(Op::FiniteRotation(i, true, [0.0; 3]));
            setup.push(Op::Damping(i, 0.0, 0.0));
            let spec = if i == 0 {
                MassSpec::Box { mass, lx: 1.6, ly: 1.2, lz: 4.0 }
            } else {
                MassSpec::Box { mass, lx: 0.5, ly: 0.5, lz: 0.5 }
            };
            setup.push(Op::Mass(i, spec));
        }
        setup.push(Op::Position(0, [0.0, body_y, 0.0]));
        setup.push(Op::Position(1, [0.0, body_y - 0.1, -1.2]));
        setup.push(Op::Joint(Kind::Fixed));
        setup.push(Op::Attach(0, Some(1), Some(0)));
        setup.push(Op::Fixed(0));
        joint_list.push((0, Kind::Fixed, Some(1), Some(0)));
        let corners = [(0.76f32, 1.3f32, 0.31f32), (-0.76, 1.3, 0.31), (0.75, -1.35, 0.31), (-0.75, -1.35, 0.31)];
        let mut hubs = [0usize; 4];
        let mut steer = [([0.0f32; 3], [0.0f32; 3]); 2];
        for (w, &(x, z, radius)) in corners.iter().enumerate() {
            let hub = 2 + 2 * w;
            let strut = hub + 1;
            hubs[w] = hub;
            let side = x.signum();
            let hub_pos = [x, radius, z];
            let strut_pos = [x - side * 0.06, radius + 0.33, z - 0.01];
            for (body, mass, pos, size) in
                [(hub, 30.0f32, hub_pos, [0.2f32, 0.5, 0.5]), (strut, jitter(&mut rng, 6.0), strut_pos, [0.1, 0.4, 0.1])]
            {
                setup.push(Op::Body);
                setup.push(Op::FiniteRotation(body, true, [0.0; 3]));
                setup.push(Op::Damping(body, 0.0, 0.0));
                setup.push(Op::Mass(body, MassSpec::Box { mass, lx: size[0], ly: size[1], lz: size[2] }));
                setup.push(Op::Position(body, pos));
                masses.push(mass);
            }
            // the slider between strut and hub, along the strut
            let j = joint_list.len();
            setup.push(Op::Joint(Kind::Slider));
            setup.push(Op::Attach(j, Some(strut), Some(hub)));
            setup.push(Op::SliderAxis(j, [-side * 0.18, 1.0, -0.03]));
            joint_list.push((j, Kind::Slider, Some(strut), Some(hub)));
            // the ball joint at the strut top
            let j = joint_list.len();
            setup.push(Op::Joint(Kind::Ball));
            setup.push(Op::Attach(j, Some(0), Some(strut)));
            setup.push(Op::BallAnchor(j, [x - side * 0.1, radius + 0.56, z - 0.02]));
            joint_list.push((j, Kind::Ball, Some(0), Some(strut)));
            // lower wishbone (two rods) and the steering rod: car-side point, hub-side point
            let inner = x - side * jitter(&mut rng, 0.42);
            let rods = [
                ([inner, radius - 0.12, z - 0.3], [x - side * 0.06, radius - 0.13, z]),
                ([inner, radius - 0.12, z + 0.26], [x - side * 0.06, radius - 0.13, z]),
                ([inner, radius + 0.02, z + 0.13], [x - side * 0.07, radius + 0.03, z + 0.12]),
            ];
            if w < 2 {
                let (c, h) = rods[2];
                steer[w] = ([c[0], c[1] - body_y, c[2]], [h[0] - hub_pos[0], h[1] - hub_pos[1], h[2] - hub_pos[2]]);
            }
            for (car_point, hub_point) in rods {
                let j = joint_list.len();
                setup.push(Op::Joint(Kind::DBall));
                setup.push(Op::Attach(j, Some(0), Some(hub)));
                setup.push(Op::DBallAnchor(j, 0, car_point));
                setup.push(Op::DBallAnchor(j, 1, hub_point));
                joint_list.push((j, Kind::DBall, Some(0), Some(hub)));
            }
        }
        if seed != 0 {
            setup.push(Op::LinearVel(0, [rng.range(-1.0, 1.0), 0.0, rng.range(0.0, 40.0)]));
        }
        let joints = joint_list.len();
        let name = format!("strut_car#{seed}");
        let car = CarLayout { hubs, body: 0, rest: [0.0; 4], steer };
        Scene { name, setup, bodies: 10, joints, joint_list, masses, rng, force_chance: 0.0, sleepy: false, car: Some(car), dead: Vec::new(), destroy_at: None }
    }

    /// One random world in ten has a body destroyed after a few hundred steps (the game
    /// destroys the bodies of a car that leaves).
    fn with_destruction(mut self, seed: u64) -> Scene {
        if seed % 10 == 3 && self.bodies >= 2 {
            self.destroy_at = Some(50 + self.rng.below(400) as usize);
        }
        self
    }

    fn is_dead(&self, b: usize) -> bool {
        self.dead.get(b).copied().unwrap_or(false)
    }

    /// What happens before step `step`, given the reference engine's state after the
    /// previous one.
    pub fn before_step(&mut self, step: usize, reference: &[BodyState]) -> Vec<Op> {
        let mut ops = Vec::new();
        if self.destroy_at == Some(step) {
            let b = self.rng.below(self.bodies as u32) as usize;
            self.dead = vec![false; self.bodies];
            self.dead[b] = true;
            ops.push(Op::Destroy(b));
        }
        if let Some(car) = &mut self.car {
            // springs and dampers between hub and body (the same world force with opposite
            // signs at the hub's position), a stiff "tyre" under each hub, steering
            let body = reference[car.body];
            let up = [0.0f32, 1.0, 0.0];
            for w in 0..4 {
                let hub = reference[car.hubs[w]];
                let travel = hub.pos[1] - body.pos[1];
                if step == 0 {
                    car.rest[w] = travel;
                }
                let k = if w < 2 { 180_000.0 } else { 160_000.0 };
                let preload = if w < 2 { 1700.0 } else { 2100.0 };
                let force = (travel - car.rest[w]) * k + preload + (hub.lvel[1] - body.lvel[1]) * 4000.0;
                let f = [up[0] * force, up[1] * force, up[2] * force];
                ops.push(Op::Add(car.body, 3, f, hub.pos));
                ops.push(Op::Add(car.hubs[w], 3, [-f[0], -f[1], -f[2]], hub.pos));
                let radius = if w < 2 { 0.33 } else { 0.34 };
                let squash = radius - hub.pos[1];
                if squash > 0.0 {
                    let load = squash * 250_000.0 - hub.lvel[1] * 500.0;
                    let side = -hub.lvel[0] * 300.0 + self.rng.range(-200.0, 200.0);
                    let contact = [hub.pos[0], hub.pos[1] - radius, hub.pos[2]];
                    ops.push(Op::Add(car.hubs[w], 3, [side, load, self.rng.range(-300.0, 300.0)], contact));
                    ops.push(Op::Add(car.hubs[w], 1, [0.0, self.rng.range(-30.0, 30.0), 0.0], [0.0; 3]));
                }
            }
            // aero: drag and downforce in the body frame at body-frame points
            let speed = body.lvel[2];
            ops.push(Op::Add(car.body, 6, [0.0, -1.2 * speed * speed, -0.6 * speed * speed], [0.0, 0.1, -0.4]));
            // the steering rods of the two front wheels move on the car side every step
            let steer = 0.01 * ((step as f32) * 0.01).sin();
            for w in 0..2 {
                let joint = 1 + 5 * w + 4;
                let (mut local1, local2) = car.steer[w];
                local1[0] += steer;
                ops.push(Op::Reseat { joint, b1: car.body, b2: car.hubs[w], local1, local2 });
            }
            // the game's softness switch below 1 m/s (car 0 only)
            let speed2 = body.lvel[0] * body.lvel[0] + body.lvel[1] * body.lvel[1] + body.lvel[2] * body.lvel[2];
            let erp = if speed2 >= 1.0 { 0.3 } else { 0.9 };
            for j in 1..self.joints {
                ops.push(Op::Param(j, PARAM_ERP, erp));
                ops.push(Op::Param(j, PARAM_CFM, 1e-7));
            }
            ops.push(Op::Param(0, PARAM_ERP, erp));
            return ops;
        }
        for b in 0..self.bodies {
            if self.is_dead(b) {
                continue;
            }
            if self.rng.chance(self.force_chance) {
                let kind = self.rng.below(7);
                let scale = self.masses[b] * if kind == 1 || kind == 2 { 6.0 } else { 25.0 };
                let a = self.rng.vec(scale);
                let p = self.rng.vec(1.5);
                let p = if kind == 3 || kind == 5 {
                    [reference[b].pos[0] + p[0], reference[b].pos[1] + p[1], reference[b].pos[2] + p[2]]
                } else {
                    p
                };
                ops.push(Op::Add(b, kind, a, p));
            }
            if self.rng.chance(0.002) {
                ops.push(Op::Stop(b));
            }
            if self.sleepy {
                if self.rng.chance(0.004) {
                    ops.push(Op::Enabled(b, true));
                    ops.push(Op::Add(b, 0, self.rng.vec(self.masses[b] * 40.0), [0.0; 3]));
                }
                if self.rng.chance(0.001) {
                    ops.push(Op::Enabled(b, false));
                }
                // dBodySetAutoDisableFlag off (which also wakes the body) and on again
                if self.rng.chance(0.001) {
                    ops.push(Op::AutoDisable(b, false));
                }
                if self.rng.chance(0.002) {
                    ops.push(Op::AutoDisable(b, true));
                }
            }
        }
        for k in 0..self.joint_list.len() {
            let (j, kind, b1, b2) = self.joint_list[k];
            // a joint that lost a body is attached to nothing: the game would not touch it
            if b1.is_some_and(|b| self.is_dead(b)) || b2.is_some_and(|b| self.is_dead(b)) {
                continue;
            }
            if self.rng.chance(0.01) {
                let erp = if self.rng.chance(0.5) { 0.9 } else { 0.3 };
                ops.push(Op::Param(j, PARAM_ERP, erp));
                // the fuel-tank variant passes -1 ("leave alone"), which the wrapper drops
                if self.rng.chance(0.5) {
                    ops.push(Op::Param(j, PARAM_CFM, 1e-7));
                }
            }
            if kind == Kind::DBall && self.rng.chance(0.02) {
                if let (Some(b1), Some(b2)) = (b1, b2) {
                    let local1 = self.rng.vec(0.5);
                    let local2 = self.rng.vec(0.5);
                    ops.push(Op::Reseat { joint: j, b1, b2, local1, local2 });
                }
            }
        }
        ops
    }
}

/// Result of one world.
#[derive(Default, Clone)]
pub struct Outcome {
    pub steps: usize,
    pub exact_steps: usize,
    pub setup_exact: bool,
    pub first: Option<String>,
    /// A value that is not finite turned up in the reference engine.
    pub not_finite: bool,
    /// FNV-1a 64 of every value the reference engine produced (the recorded states).
    pub hash: u64,
    pub max_rows: u32,
    /// Body-steps in which a body was disabled (asleep) in the reference engine.
    pub asleep: usize,
    /// Joint-steps in which a rod's two anchors were less than 1e-7 m apart in the reference
    /// engine, so that the DBall row took one of its fallback directions.
    pub short_rods: usize,
}

fn fnv(hash: &mut u64, words: &[u32]) {
    for w in words {
        for byte in w.to_le_bytes() {
            *hash ^= byte as u64;
            *hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
}

fn same(a: f32, b: f32) -> bool {
    a.to_bits() == b.to_bits() || (a.is_nan() && b.is_nan())
}

fn compare_slices(what: &str, a: &[f32], b: &[f32], out: &mut Vec<String>) -> bool {
    let mut ok = a.len() == b.len();
    for k in 0..a.len().min(b.len()) {
        if !same(a[k], b[k]) {
            ok = false;
            out.push(format!(
                "{what}[{k}]: acs.exe {:?} ({:#010x}) / Rust {:?} ({:#010x})",
                a[k],
                a[k].to_bits(),
                b[k],
                b[k].to_bits()
            ));
        }
    }
    ok
}

fn compare_body(index: usize, a: &BodyState, b: &BodyState, out: &mut Vec<String>) -> bool {
    let name = |field: &str| format!("body {index} {field}");
    let mut ok = true;
    ok &= compare_slices(&name("pos"), &a.pos, &b.pos, out);
    ok &= compare_slices(&name("q"), &a.q, &b.q, out);
    let rows = |r: &[f32; 12]| [r[0], r[1], r[2], r[4], r[5], r[6], r[8], r[9], r[10]];
    ok &= compare_slices(&name("R"), &rows(&a.r), &rows(&b.r), out);
    ok &= compare_slices(&name("lvel"), &a.lvel, &b.lvel, out);
    ok &= compare_slices(&name("avel"), &a.avel, &b.avel, out);
    ok &= compare_slices(&name("facc"), &a.facc, &b.facc, out);
    ok &= compare_slices(&name("tacc"), &a.tacc, &b.tacc, out);
    if a.tag != b.tag || a.flags != b.flags {
        ok = false;
        out.push(format!(
            "body {index}: tag {} / {}, flags {:#x} / {:#x} (acs.exe / Rust)",
            a.tag, b.tag, a.flags, b.flags
        ));
    }
    ok
}

fn compare_mass(index: usize, a: &MassState, b: &MassState, out: &mut Vec<String>) -> bool {
    let rows = |r: &[f32; 12]| [r[0], r[1], r[2], r[4], r[5], r[6], r[8], r[9], r[10]];
    let mut ok = compare_slices(&format!("body {index} mass"), &[a.mass, a.inv_mass], &[b.mass, b.inv_mass], out);
    ok &= compare_slices(&format!("body {index} I"), &rows(&a.i), &rows(&b.i), out);
    ok &= compare_slices(&format!("body {index} invI"), &rows(&a.inv_i), &rows(&b.inv_i), out);
    ok
}

fn compare_joint(index: usize, a: &JointState, b: &JointState, with_feedback: bool, out: &mut Vec<String>) -> bool {
    let mut ok = compare_slices(&format!("joint {index} parameters"), &a.params, &b.params, out);
    if with_feedback {
        ok &= compare_slices(&format!("joint {index} force"), &a.feedback, &b.feedback, out);
        if a.tag != b.tag {
            ok = false;
            out.push(format!("joint {index}: tag {} (acs.exe) / {} (Rust)", a.tag, b.tag));
        }
    }
    ok
}

/// Builds the scene in both engines, steps both `steps` times and compares everything after
/// every step. After a step that differs the Rust side is put back on the reference state,
/// so every step is judged on its own.
pub fn run<A: Engine, R: Engine>(scene: &mut Scene, ac: &mut A, rust: &mut R, steps: usize, record: Option<&mut Vec<u32>>) -> Outcome {
    let mut outcome = Outcome { hash: 0xcbf2_9ce4_8422_2325, setup_exact: true, ..Outcome::default() };
    let mut record = record;
    ac.set_world(GRAVITY, ERP, CFM);
    rust.set_world(GRAVITY, ERP, CFM);
    let (mut drive_ac, mut drive_rust) = (Driver::default(), Driver::default());
    for op in &scene.setup.clone() {
        drive_ac.apply(ac, op);
        drive_rust.apply(rust, op);
    }
    // after set-up: masses, poses and joint parameters must already agree
    let mut first = Vec::new();
    for b in 0..scene.bodies {
        outcome.setup_exact &= compare_mass(b, &ac.mass_state(b), &rust.mass_state(b), &mut first);
        outcome.setup_exact &= compare_body(b, &ac.body_state(b), &rust.body_state(b), &mut first);
    }
    for j in 0..scene.joints {
        outcome.setup_exact &= compare_joint(j, &ac.joint_state(j), &rust.joint_state(j), false, &mut first);
    }
    if let Some(text) = first.first() {
        outcome.first = Some(format!("after set-up: {text}"));
        // continue from identical states anyway
        for b in 0..scene.bodies {
            rust.body_write_state(b, &ac.body_state(b));
        }
        for j in 0..scene.joints {
            rust.joint_write_params(j, &ac.joint_state(j).params);
        }
    }

    let mut reference: Vec<BodyState> = (0..scene.bodies).map(|b| ac.body_state(b)).collect();
    for step in 0..steps {
        for op in scene.before_step(step, &reference) {
            drive_ac.apply(ac, &op);
            drive_rust.apply(rust, &op);
        }
        for k in 0..scene.joint_list.len() {
            let (j, kind, b1, b2) = scene.joint_list[k];
            let (Kind::DBall, Some(b1)) = (kind, b1) else { continue };
            if scene.is_dead(b1) || b2.is_some_and(|b| scene.is_dead(b)) {
                continue;
            }
            let p = ac.joint_state(j).params;
            let a1 = ac.body_get_rel_point_pos(b1, [p[0], p[1], p[2]]);
            let a2 = match b2 {
                Some(b2) => ac.body_get_rel_point_pos(b2, [p[3], p[4], p[5]]),
                None => [p[3], p[4], p[5]],
            };
            let d = [a1[0] - a2[0], a1[1] - a2[1], a1[2] - a2[2]];
            let length = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
            outcome.short_rods += (length < 1e-7) as usize;
        }
        ac.step(H);
        rust.step(H);
        outcome.max_rows = outcome.max_rows.max(rust.rows());
        let mut step_first: Vec<String> = Vec::new();
        let mut exact = true;
        let mut words: Vec<u32> = Vec::new();
        for b in 0..scene.bodies {
            if scene.is_dead(b) {
                continue;
            }
            let a = ac.body_state(b);
            exact &= compare_body(b, &a, &rust.body_state(b), &mut step_first);
            for v in a.pos.iter().chain(&a.q).chain(&a.r).chain(&a.lvel).chain(&a.avel) {
                words.push(v.to_bits());
                outcome.not_finite |= !v.is_finite();
            }
            words.push(a.tag as u32);
            outcome.asleep += (a.flags & 4 != 0) as usize;
            reference[b] = a;
            // the frame-conversion getters, for a point that changes every step
            let t = step as f32 * 0.37 + b as f32;
            let point = [1.3 * t.sin(), 0.7 * (1.7 * t).cos() + a.pos[1], 0.9 * (0.3 * t).sin() - 0.2];
            let (probe_a, probe_r) = (ac.body_probe(b, point), rust.body_probe(b, point));
            exact &= compare_slices(&format!("body {b} getters"), &probe_a, &probe_r, &mut step_first);
            words.extend(probe_a.iter().map(|v| v.to_bits()));
        }
        for j in 0..scene.joints {
            let a = ac.joint_state(j);
            exact &= compare_joint(j, &a, &rust.joint_state(j), true, &mut step_first);
            words.extend(a.feedback.iter().map(|v| v.to_bits()));
            words.push(a.tag as u32);
        }
        fnv(&mut outcome.hash, &words);
        if let Some(record) = record.as_deref_mut() {
            record.extend_from_slice(&words);
        }
        outcome.steps += 1;
        if exact {
            outcome.exact_steps += 1;
        } else {
            if outcome.first.is_none() {
                outcome.first = Some(format!("step {step}: {}", step_first.first().cloned().unwrap_or_default()));
                if std::env::var_os("ODE_ORACLE_DETAIL").is_some() {
                    eprintln!("{} step {step}: {} differences", scene.name, step_first.len());
                    for line in step_first.iter().take(60) {
                        eprintln!("    {line}");
                    }
                    for op in &scene.setup {
                        eprintln!("    setup {op:?}");
                    }
                }
            }
            for b in 0..scene.bodies {
                if !scene.is_dead(b) {
                    rust.body_write_state(b, &reference[b]);
                }
            }
            for j in 0..scene.joints {
                rust.joint_write_params(j, &ac.joint_state(j).params);
            }
        }
    }
    outcome
}
