//! The game side: AC's own `PhysicsEngine`, `Track` and `Car` built in this process, the
//! fakes they talk to (ground, controls device) and the hooks that record what they do.
//!
//! Addresses are Ghidra addresses of acs.exe 1.16.4; member offsets are from acs.pdb
//! (`re/types/*.txt`), ODE offsets from the disassembly of ODE's own accessors.

use std::cell::UnsafeCell;
use std::path::Path;

use rustyac_physics::tyre::rig::{self, StepInput};

use crate::acs::Acs;
use crate::record::{Call, Row};
use crate::scenario::{CarView, Controls, Driver, Ground, Scenario, DT};

pub type V3 = [f32; 3];

// --- functions -------------------------------------------------------------------------------
const VA_PHYSICS_ENGINE_CTOR: usize = 0x1_4026_2430; // PhysicsEngine::PhysicsEngine()
const VA_PHYSICS_ENGINE_STEP: usize = 0x1_4026_4760; // PhysicsEngine::step(float dt, double physicsTime, double gameTime)
const VA_TRACK_CTOR: usize = 0x1_4027_7100; // Track::Track(PhysicsEngine*, const wstring& name, const wstring& config)
const VA_TRACK_INIT_AI_SPLINE: usize = 0x1_4027_82a0; // Track::initAISpline()
const VA_CAR_CTOR: usize = 0x1_4026_bf00; // Car::Car(PhysicsEngine*, const wstring& unixName, const wstring& config)
const VA_CAR_INIT_COLLIDER_MESH: usize = 0x1_4027_3b20; // Car::initColliderMesh(Mesh*, const mat44f&)
const VA_CAR_FORCE_POSITION: usize = 0x1_4026_fe10; // Car::forcePosition(const vec3f&, bool)
const VA_CAR_FORCE_ROTATION: usize = 0x1_4027_0040; // Car::forceRotation(const vec3f&)
const VA_CAR_GET_PHYSICS_STATE: usize = 0x1_4027_0d70; // Car::getPhysicsState(CarPhysicsState*)
const VA_SHARED_MEMORY_UPDATE_PHYSICS: usize = 0x1_4018_6ef0; // SharedMemoryWriter::updatePhysics(const double&)
const VA_TYRE_STEP: usize = 0x1_4028_3800; // Tyre::step(float dt)
const VA_DWORLDSTEP: usize = 0x1_4034_04c0; // dWorldStep(dWorldID, float)
const VA_STEP_MEMORY_ESTIMATE: usize = 0x1_4035_0000; // dxEstimateStepMemoryRequirements
const VA_STEP_STAGE0_JOINTS: usize = 0x1_4035_0b80; // dxStepIsland_Stage0_Joints (runs right after gravity and the gyroscopic torque were added)
const VA_CAR_SET_DAMAGE_LEVEL: usize = 0x1_4027_5b20; // Car::setDamageLevel(float)
const VA_CAR_RESET_SUSPENSION_DAMAGE: usize = 0x1_4027_5970; // Car::resetSuspensionDamageLevel()
// --- globals ---------------------------------------------------------------------------------
const VA_INIREADERDOCUMENTS_INITIALIZED: usize = 0x1_4155_a588; // static bool INIReaderDocuments::initialized
const VA_IS_USING_QPT: usize = 0x1_4151_d140; // bool isUsingQPT (ksTimer)
const VA_TIMER_START: usize = 0x1_4155_a590; // LARGE_INTEGER startTime (ksTimer)
const VA_TIMER_FREQUENCY: usize = 0x1_4155_a598; // LARGE_INTEGER frequency (ksTimer)
const VA_IS_TEST_MODE: usize = 0x1_4155_a770; // static bool PhysicsEngine::isTestMode
// --- vtables ---------------------------------------------------------------------------------
const VA_KEYBOARD_CONTROL_VTABLE: usize = 0x1_404c_5d50; // KeyboardCarControl (a real ICarControlsProvider)
const VA_SUSPENSION_VTABLE: usize = 0x1_404f_f870; // Suspension (double wishbone)
const VA_RIGID_BODY_VTABLE: usize = 0x1_4050_09c0; // RigidBodyODE
const RIGID_BODY_SLOTS: usize = 0x158 / 8;
const SUSPENSION_SLOTS: usize = 0xc8 / 8;

// --- object sizes and members ----------------------------------------------------------------
const PE_SIZE: usize = 0x278;
const PE_PHYSICS_TIME: usize = 0x20;
const PE_GAME_TIME: usize = 0xf8;
const PE_CORE: usize = 0x190;
const CORE_WORLD: usize = 0x8;
const CORE_CONTACT_POINTS: usize = 0x30;
const WORLD_BODY_COUNT: usize = 0x30;
const WORLD_JOINT_COUNT: usize = 0x34;
const WORLD_GRAVITY: usize = 0x38;
const PE_ALLOW_TYRE_BLANKETS: usize = 0xb8;
const PE_FUEL_CONSUMPTION_RATE: usize = 0xc8;
const PE_TYRE_CONSUMPTION_RATE: usize = 0xcc;
const PE_AMBIENT_TEMPERATURE: usize = 0x100;
const PE_ROAD_TEMPERATURE: usize = 0x104;
const PE_MECHANICAL_DAMAGE_RATE: usize = 0x108;
const PE_WIND: usize = 0x158;
const PE_STEP_COUNTER: usize = 0x1a8;
const TRACK_SIZE: usize = 0x148;
const TRACK_DYNAMIC_GRIP_LEVEL: usize = 0x128;
const SURFACE_SIZE: usize = 0xc8;
const SD_GRIP_MOD: usize = 0x90;
const SD_COLLISION_CATEGORY: usize = 0x9c;
const SD_IS_VALID_TRACK: usize = 0xa0;

pub const CAR_SIZE: usize = 0x3ea0;
const CAR_FINAL_STEER_ANGLE_SIGNAL: usize = 0x8;
const CAR_BODY: usize = 0x118;
const CAR_FUEL_TANK_BODY: usize = 0x120;
const CAR_FUEL_TANK_JOINT: usize = 0x130;
const CAR_CONTROLS: usize = 0x140;
const CAR_STEER_LOCK: usize = 0x174;
const CAR_STEER_RATIO: usize = 0x178;
const CAR_MASS: usize = 0x1e0;
const CAR_ACC_G: usize = 0x1e8;
const CAR_DRIVETRAIN: usize = 0x310;
const CAR_ABS: usize = 0x958;
const CAR_TRACTION_CONTROL: usize = 0xa00;
const CAR_SPEED_LIMITER: usize = 0xaa8;
const CAR_AERO_MAP: usize = 0xab8;
pub const CAR_TYRES: usize = 0xb28;
const CAR_SUSPENSIONS: usize = 0x2c88;
const CAR_BRAKE_SYSTEM: usize = 0x2ca0;
const CAR_AUTOCLUTCH: usize = 0x3090;
const CAR_PHYSICS_GUID: usize = 0x3238;
const CAR_TELEMETRY_IS_ENABLED: usize = 0x3258;
const CAR_PERFORMANCE_METER_IS_ENABLED: usize = 0x3560;
const CAR_AUTO_BLIP: usize = 0x3340;
const CAR_AUTO_SHIFT: usize = 0x33e8;
const CAR_EDL: usize = 0x3428;
const CAR_ANTIROLL_BARS: usize = 0x3460;
const CAR_STABILITY_CONTROL: usize = 0x34f0;
const CAR_DRS: usize = 0x3640;
const CAR_HEAVE_SPRINGS: usize = 0x3b40;
const CAR_LAST_FF: usize = 0x3c48;
const CAR_WATER: usize = 0x3c50;
const CAR_TORQUE_MODE_EX: usize = 0x3d10;
const CAR_IS_CONTROLS_LOCKED: usize = 0x3d14;
const CAR_BLACK_FLAGGED: usize = 0x3d20;
const CAR_PENALTY_TIME: usize = 0x3d30;
const CAR_CONTROLS_PROVIDER: usize = 0x3d80;
const CAR_SLEEPING_FRAMES: usize = 0x3d94;
const CAR_MZ_CURRENT: usize = 0x3d98;
const CAR_FUEL: usize = 0x3db8;
const CAR_DAMAGE_ZONE_LEVEL: usize = 0x3de0;
const CAR_FRAMES_TO_SLEEP: usize = 0x3e28;
const CAR_BALLAST_KG: usize = 0x3e30;
const CAR_VALUE_CACHE_SPEED: usize = 0x3e90;

// CarControls
const CC_GEAR_UP: usize = 0x0;
const CC_GEAR_DN: usize = 0x1;
const CC_REQUESTED_GEAR_INDEX: usize = 0x8;
const CC_HAND_BRAKE: usize = 0x10;
const CC_GAS: usize = 0x24;
const CC_BRAKE: usize = 0x28;
const CC_STEER: usize = 0x2c;
const CC_CLUTCH: usize = 0x30;

// Tyre
pub const TYRE_SIZE: usize = 0x858;
const T_INPUTS: usize = 0x0;
const T_STATUS: usize = 0x2e8;
const T_SURFACE_DEF: usize = 0x410;
const T_ABS_OVERRIDE: usize = 0x41c;
const T_THERMAL: usize = 0x420;
const T_AI_MULT: usize = 0x538;
const T_EXTERNAL_INPUTS: usize = 0x578;
const T_DRIVEN: usize = 0x5a1;
const T_LOCAL_WHEEL_ROTATION: usize = 0x5b8;
const T_RAY_CASTER: usize = 0x620;
const T_CURRENT_COMPOUND_INDEX: usize = 0x648;
const T_BLANKETS_ON: usize = 0x64c;
const S_ANGULAR_VELOCITY: usize = 0x14;
const TH_PATCHES: usize = 0x8;
const PATCH_SIZE: usize = 0x28;

// Suspension (double wishbone)
const SUS_BUMP_STOP_UP: usize = 0x1c;
const SUS_BUMP_STOP_DN: usize = 0x20;
const SUS_HUB: usize = 0x40;
const SUS_JOINTS: usize = 0x68;
const SUS_STATUS: usize = 0x1c0;
const SUS_STEER_TORQUE: usize = 0x1dc;
const SUS_STEER_ANGLE: usize = 0x1ec;
/// `Suspension::joints[0..5]` in creation order (`Suspension::attach`): each rod runs from a
/// point on the car body (anchor 1) to a point on the hub (anchor 2).
const SUS_JOINT_NAMES: [&str; 5] = ["top_rear", "top_front", "bottom_rear", "bottom_front", "steer_rod"];

// SharedMemoryWriter (0x220 bytes) and the CarAvatar (0x12a8 bytes) it reads a few values from
const SMW_SIZE: usize = 0x220;
const SMW_CAR: usize = 0x60;
const SMW_AVATAR: usize = 0x68;
const SMW_LAP_INVALIDATOR: usize = 0x70;
/// `sharedMemories[2]`, the physics page: mapped pointer, packet id, warm-up counter.
const SMW_PHYSICS_BUFFER: usize = 0x80 + 2 * 0x18 + 0x8;
const SMW_PHYSICS_PACKET_ID: usize = 0x80 + 2 * 0x18 + 0x10;
const SMW_PHYSICS_NULL_COUNTS: usize = 0x80 + 2 * 0x18 + 0x14;
/// `physicsInfo.bumpStopsUp` / `bumpStopsDn` (CarPhysicsInfo is at +0xc8).
const SMW_BUMP_STOPS_UP: usize = 0xc8 + 0x68;
const SMW_BUMP_STOPS_DN: usize = 0xc8 + 0x78;
const AVATAR_SIZE: usize = 0x12a8;
const AVATAR_PHYSICS_STATE: usize = 0x268;
const CAR_LAP_INVALIDATOR: usize = 0x3a18;

// RigidBodyODE / IJoint wrappers
const RB_ODE_BODY: usize = 0x8;
const JOINT_WRAPPER_ODE_JOINT: usize = 0x8;

// dxBody (single precision ODE 0.13.1)
const B_TAG: usize = 0x20;
const B_FIRST_JOINT: usize = 0x30;
const B_MASS: usize = 0x48;
const B_INERTIA: usize = 0x5c;
const B_POS: usize = 0xc0;
const B_R: usize = 0xd0;
const B_Q: usize = 0x100;
const B_LVEL: usize = 0x110;
const B_AVEL: usize = 0x120;
const B_FACC: usize = 0x130;
const B_TACC: usize = 0x140;
// dxJoint
const J_TAG: usize = 0x20;
const J_BODY0: usize = 0x40;
const J_BODY1: usize = 0x58;
const J_FEEDBACK: usize = 0x68;
const J_PARAMS: usize = 0x88;
const JOINT_DBALL: u32 = 15;
const JOINT_FIXED: u32 = 7;
/// Type-specific joint members recorded each step, as `(name, offset from the joint's own
/// data at +0x88)`; all f32.
const DBALL_PARAMS: [(&str, usize); 9] = [
    ("anchor1.x", 0x00),
    ("anchor1.y", 0x04),
    ("anchor1.z", 0x08),
    ("anchor2.x", 0x10),
    ("anchor2.y", 0x14),
    ("anchor2.z", 0x18),
    ("erp", 0x20),
    ("cfm", 0x24),
    ("distance", 0x28),
];
const FIXED_PARAMS: [(&str, usize); 9] = [
    ("qrel.w", 0x00),
    ("qrel.x", 0x04),
    ("qrel.y", 0x08),
    ("qrel.z", 0x0c),
    ("offset.x", 0x10),
    ("offset.y", 0x14),
    ("offset.z", 0x18),
    ("erp", 0x20),
    ("cfm", 0x24),
];
const JOINT_PARAM_WORDS: usize = 9;

fn joint_params(kind: u32) -> &'static [(&'static str, usize); JOINT_PARAM_WORDS] {
    match kind {
        JOINT_DBALL => &DBALL_PARAMS,
        JOINT_FIXED => &FIXED_PARAMS,
        other => panic!("the car has a joint of ODE type {other}, which the oracle does not know"),
    }
}

pub unsafe fn wr<T>(base: *mut u8, offset: usize, value: T) {
    base.add(offset).cast::<T>().write_unaligned(value);
}

pub unsafe fn rd<T: Copy>(base: *const u8, offset: usize) -> T {
    base.add(offset).cast::<T>().read_unaligned()
}

fn leak<T>(value: Vec<T>) -> *mut T {
    Box::leak(value.into_boxed_slice()).as_mut_ptr()
}

/// Zeroed, 8-byte aligned memory that is never freed.
fn object(size: usize) -> *mut u8 {
    leak(vec![0u64; size.div_ceil(8)]).cast::<u8>()
}

unsafe fn v3(base: *const u8, offset: usize) -> V3 {
    rd(base, offset)
}

/// A `std::wstring` (VS2013 layout) in the game's heap.
fn wstring(acs: &Acs, text: &str) -> *mut u8 {
    let units: Vec<u16> = text.encode_utf16().collect();
    let string = acs.alloc(32);
    unsafe {
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
    string
}

// --- recorder state --------------------------------------------------------------------------

#[derive(Clone)]
pub struct BodyRef {
    pub name: String,
    /// The game's `RigidBodyODE`.
    wrapper: *mut u8,
    /// ODE's `dxBody`.
    ode: *mut u8,
}

#[derive(Clone)]
pub struct JointRef {
    pub name: String,
    /// ODE joint type number (1 ball, 3 slider, 4 contact, 7 fixed, 15 dball).
    pub kind: u32,
    ode: *mut u8,
    pub body0: usize,
    pub body1: usize,
    feedback: *mut [f32; 16],
}

/// A body as ODE holds it.
#[derive(Clone, Copy, Default)]
struct BodyState {
    pos: V3,
    q: [f32; 4],
    r: [f32; 9],
    lvel: V3,
    avel: V3,
    facc: V3,
    tacc: V3,
    mass: f32,
    inertia: V3,
}

unsafe fn body_state(ode: *const u8) -> BodyState {
    let r12: [f32; 12] = rd(ode, B_R);
    let inertia: [f32; 12] = rd(ode, B_INERTIA);
    BodyState {
        pos: v3(ode, B_POS),
        q: rd(ode, B_Q),
        // ODE keeps a 3x4 matrix; the fourth column is padding
        r: [r12[0], r12[1], r12[2], r12[4], r12[5], r12[6], r12[8], r12[9], r12[10]],
        lvel: v3(ode, B_LVEL),
        avel: v3(ode, B_AVEL),
        facc: v3(ode, B_FACC),
        tacc: v3(ode, B_TACC),
        mass: rd(ode, B_MASS),
        inertia: [inertia[0], inertia[5], inertia[10]],
    }
}

/// What one `Tyre::step` call saw and did.
#[derive(Clone, Default)]
struct TyreCapture {
    input: Option<StepInput>,
    /// `Tyre::localWheelRotation` on entry: the drivetrain turns the driven wheels' matrix
    /// between two tyre steps, so for the tyre it is an input.
    wheel_rotation_in: [f32; 16],
    calls: Vec<rig::Call>,
    output: Vec<u64>,
    /// How often the tyre asked its hub for each thing, and the ground for a hit.
    asked: [u32; 4],
    /// Set if the hub gave two different answers to the same question within one step
    /// (the single-wheel rig can only replay one).
    ambiguous: bool,
}

struct State {
    car: *mut u8,
    engine: *mut u8,
    track: *mut u8,
    surface: *mut u8,
    ground: Ground,
    bodies: Vec<BodyRef>,
    joints: Vec<JointRef>,
    body_vtable: *const usize,
    suspensions: [*mut u8; 4],
    suspension_vtables: [*const usize; 4],
    tyre_step_original: usize,
    world_step_original: usize,
    /// What the fake controls device reports this step.
    controls: Controls,
    /// How many times the game polled the device this step.
    polled: u32,
    tape: Vec<Call>,
    outer_site: u32,
    /// The wheel whose `Tyre::step` is running.
    tyre: Option<usize>,
    tyres: [TyreCapture; 4],
    pre: Vec<BodyState>,
    post: Vec<BodyState>,
    /// Force and torque accumulators as the solver used them: the game's forces plus what
    /// `dWorldStep` adds itself (gravity, gyroscopic torque).
    solver: Vec<(V3, V3)>,
    joint_params: Vec<[f32; JOINT_PARAM_WORDS]>,
    stage0_original: usize,
    world_steps: u32,
    image_base: usize,
}

struct Global(UnsafeCell<Option<State>>);
// the oracle is single-threaded; the game code it calls runs on the calling thread
unsafe impl Sync for Global {}
static STATE: Global = Global(UnsafeCell::new(None));

fn state() -> &'static mut State {
    unsafe { (*STATE.0.get()).as_mut().expect("the game called a hook before the world was built") }
}

/// Return address of the game code that called the hook being entered (set by the thunks).
static mut RETURN_ADDRESS: usize = 0;

/// `$thunk` is what goes into a vtable: it notes where the call came from, then runs `$handler`
/// with the registers untouched.
macro_rules! thunk {
    ($thunk:ident => $handler:ident) => {
        #[unsafe(naked)]
        extern "C" fn $thunk() {
            core::arch::naked_asm!(
                "mov rax, [rsp]",
                "mov [rip + {ret}], rax",
                "jmp {handler}",
                ret = sym RETURN_ADDRESS,
                handler = sym $handler,
            );
        }
    };
}

impl State {
    fn site(&self, return_address: usize) -> u32 {
        return_address.wrapping_sub(self.image_base) as u32
    }

    fn body_index(&self, wrapper: *mut u8) -> usize {
        self.bodies
            .iter()
            .position(|b| b.wrapper == wrapper)
            .expect("a hooked call on a rigid body the oracle does not know")
    }

    fn record(&mut self, wrapper: *mut u8, kind: u32, return_address: usize, a: V3, b: V3) {
        let body = self.body_index(wrapper);
        let ode = self.bodies[body].ode;
        let call = Call {
            body: body as u32,
            kind,
            site: self.site(return_address),
            outer_site: self.outer_site,
            a,
            b,
            facc: unsafe { v3(ode, B_FACC) },
            tacc: unsafe { v3(ode, B_TACC) },
        };
        self.tape.push(call);
        // the tyre's own push on the car body (surface drag), as the single-wheel rig records it
        if let (Some(wheel), 2, 0) = (self.tyre, kind, body) {
            self.tyres[wheel].calls.push(rig::Call { kind: 4, a, b, ..rig::Call::default() });
        }
    }
}

// --- IRigidBody hooks ------------------------------------------------------------------------

macro_rules! body_force_hook {
    ($thunk:ident, $handler:ident, $slot:expr, $kind:expr, two) => {
        extern "C" fn $handler(this: *mut u8, a: *const V3, b: *const V3) {
            let from = unsafe { RETURN_ADDRESS };
            let st = state();
            let original: extern "C" fn(*mut u8, *const V3, *const V3) =
                unsafe { std::mem::transmute(*st.body_vtable.add($slot / 8)) };
            let (va, vb) = unsafe { (*a, *b) };
            original(this, a, b);
            st.record(this, $kind, from, va, vb);
        }
        thunk!($thunk => $handler);
    };
    ($thunk:ident, $handler:ident, $slot:expr, $kind:expr, one) => {
        extern "C" fn $handler(this: *mut u8, a: *const V3) {
            let from = unsafe { RETURN_ADDRESS };
            let st = state();
            let original: extern "C" fn(*mut u8, *const V3) =
                unsafe { std::mem::transmute(*st.body_vtable.add($slot / 8)) };
            let va = unsafe { *a };
            original(this, a);
            st.record(this, $kind, from, va, [0.0; 3]);
        }
        thunk!($thunk => $handler);
    };
}

body_force_hook!(body_add_force_at_pos, body_add_force_at_pos_h, 0x110, 1, two);
body_force_hook!(body_add_force_at_local_pos, body_add_force_at_local_pos_h, 0xf8, 2, two);
body_force_hook!(body_add_local_force, body_add_local_force_h, 0xd8, 3, one);
body_force_hook!(body_add_local_force_at_pos, body_add_local_force_at_pos_h, 0xe8, 4, two);
body_force_hook!(body_add_local_force_at_local_pos, body_add_local_force_at_local_pos_h, 0xf0, 5, two);
body_force_hook!(body_add_torque, body_add_torque_h, 0x118, 6, one);
body_force_hook!(body_add_local_torque, body_add_local_torque_h, 0xe0, 7, one);
body_force_hook!(body_set_velocity, body_set_velocity_h, 0x80, 9, one);
body_force_hook!(body_set_angular_velocity, body_set_angular_velocity_h, 0x88, 10, one);
body_force_hook!(body_set_position, body_set_position_h, 0x90, 11, one);

extern "C" fn body_stop_h(this: *mut u8, amount: f32) {
    let from = unsafe { RETURN_ADDRESS };
    let st = state();
    let original: extern "C" fn(*mut u8, f32) = unsafe { std::mem::transmute(*st.body_vtable.add(0x70 / 8)) };
    original(this, amount);
    st.record(this, 8, from, [0.0; 3], [0.0; 3]);
}
thunk!(body_stop => body_stop_h);

// --- ISuspension hooks -----------------------------------------------------------------------

fn suspension_index(st: &State, this: *mut u8) -> usize {
    st.suspensions.iter().position(|&s| s == this).expect("a hooked call on an unknown suspension")
}

fn note<T: PartialEq + Copy>(capture: &mut TyreCapture, which: usize, slot: &mut T, value: T) {
    if capture.asked[which] > 0 && *slot != value {
        capture.ambiguous = true;
    }
    capture.asked[which] += 1;
    *slot = value;
}

extern "C" fn sus_get_hub_world_matrix(this: *mut u8, out: *mut [f32; 16]) -> *mut [f32; 16] {
    let st = state();
    let wheel = suspension_index(st, this);
    let original: extern "C" fn(*mut u8, *mut [f32; 16]) -> *mut [f32; 16] =
        unsafe { std::mem::transmute(*st.suspension_vtables[wheel].add(1)) };
    let result = original(this, out);
    if st.tyre == Some(wheel) {
        let capture = &mut st.tyres[wheel];
        let mut slot = capture.input.as_ref().unwrap().hub_matrix;
        note(capture, 0, &mut slot, unsafe { *out });
        capture.input.as_mut().unwrap().hub_matrix = slot;
    }
    result
}

extern "C" fn sus_get_point_velocity(this: *mut u8, out: *mut V3, point: *const V3) -> *mut V3 {
    let st = state();
    let wheel = suspension_index(st, this);
    let original: extern "C" fn(*mut u8, *mut V3, *const V3) -> *mut V3 =
        unsafe { std::mem::transmute(*st.suspension_vtables[wheel].add(2)) };
    let result = original(this, out, point);
    if st.tyre == Some(wheel) {
        let capture = &mut st.tyres[wheel];
        let mut slot = capture.input.as_ref().unwrap().hub_velocity;
        note(capture, 1, &mut slot, unsafe { *out });
        capture.input.as_mut().unwrap().hub_velocity = slot;
    }
    result
}

extern "C" fn sus_get_hub_angular_velocity(this: *mut u8, out: *mut V3) -> *mut V3 {
    let st = state();
    let wheel = suspension_index(st, this);
    let original: extern "C" fn(*mut u8, *mut V3) -> *mut V3 =
        unsafe { std::mem::transmute(*st.suspension_vtables[wheel].add(7)) };
    let result = original(this, out);
    if st.tyre == Some(wheel) {
        let capture = &mut st.tyres[wheel];
        let mut slot = capture.input.as_ref().unwrap().hub_angular_velocity;
        note(capture, 2, &mut slot, unsafe { *out });
        capture.input.as_mut().unwrap().hub_angular_velocity = slot;
    }
    result
}

extern "C" fn sus_add_force_at_pos_h(this: *mut u8, force: *const V3, pos: *const V3, driven: usize, steer: usize) {
    let from = unsafe { RETURN_ADDRESS };
    let st = state();
    let wheel = suspension_index(st, this);
    let original: extern "C" fn(*mut u8, *const V3, *const V3, usize, usize) =
        unsafe { std::mem::transmute(*st.suspension_vtables[wheel].add(3)) };
    if st.tyre == Some(wheel) {
        // bools arrive in the low byte only
        let flags = (driven & 0xff != 0) as u32 | ((steer & 0xff != 0) as u32) << 1;
        let (a, b) = unsafe { (*force, *pos) };
        st.tyres[wheel].calls.push(rig::Call { kind: 1, a, b, flags, ..rig::Call::default() });
    }
    let site = st.site(from);
    let outer = std::mem::replace(&mut st.outer_site, site);
    original(this, force, pos, driven, steer);
    state().outer_site = outer;
}
thunk!(sus_add_force_at_pos => sus_add_force_at_pos_h);

extern "C" fn sus_add_torque_h(this: *mut u8, torque: *const V3) {
    let from = unsafe { RETURN_ADDRESS };
    let st = state();
    let wheel = suspension_index(st, this);
    let original: extern "C" fn(*mut u8, *const V3) =
        unsafe { std::mem::transmute(*st.suspension_vtables[wheel].add(4)) };
    if st.tyre == Some(wheel) {
        st.tyres[wheel].calls.push(rig::Call { kind: 2, a: unsafe { *torque }, ..rig::Call::default() });
    }
    let site = st.site(from);
    let outer = std::mem::replace(&mut st.outer_site, site);
    original(this, torque);
    state().outer_site = outer;
}
thunk!(sus_add_torque => sus_add_torque_h);

extern "C" fn sus_add_local_force_and_torque_h(this: *mut u8, force: *const V3, torque: *const V3, drive: *const V3) {
    let from = unsafe { RETURN_ADDRESS };
    let st = state();
    let wheel = suspension_index(st, this);
    let original: extern "C" fn(*mut u8, *const V3, *const V3, *const V3) =
        unsafe { std::mem::transmute(*st.suspension_vtables[wheel].add(24)) };
    if st.tyre == Some(wheel) {
        let (a, b, c) = unsafe { (*force, *torque, *drive) };
        st.tyres[wheel].calls.push(rig::Call { kind: 3, a, b, c, ..rig::Call::default() });
    }
    let site = st.site(from);
    let outer = std::mem::replace(&mut st.outer_site, site);
    original(this, force, torque, drive);
    state().outer_site = outer;
}
thunk!(sus_add_local_force_and_torque => sus_add_local_force_and_torque_h);

// --- the ground ------------------------------------------------------------------------------

/// AC's `RayCastResult`.
#[repr(C)]
struct RayCastResult {
    surface_def: *mut u8,
    pos: V3,
    normal: V3,
    has_hit: u8,
    collision_object: *mut u8,
}
const _: () = assert!(std::mem::size_of::<RayCastResult>() == 0x30);

/// The game's own per-wheel ray is 3 m long and starts 2 m above the wheel centre.
const RAY_LENGTH: f32 = 3.0;

/// `IRayTrackCollisionProvider::rayCast` of the track: the analytic road.
extern "C" fn track_ray_cast(
    _this: *mut u8,
    org: *const V3,
    _dir: *const V3,
    result: *mut RayCastResult,
    _length: f32,
) -> u8 {
    let st = state();
    let org = unsafe { *org };
    let height = st.ground.height(org[0], org[2]);
    let hit = org[1] >= height && org[1] - height <= RAY_LENGTH;
    unsafe {
        (*result).has_hit = hit as u8;
        if hit {
            (*result).surface_def = st.surface;
            (*result).pos = [org[0], height, org[2]];
            (*result).normal = [0.0, 1.0, 0.0];
            (*result).collision_object = std::ptr::null_mut();
        }
    }
    if let Some(wheel) = st.tyre {
        let capture = &mut st.tyres[wheel];
        capture.asked[3] += 1;
        let input = capture.input.as_mut().unwrap();
        input.has_hit = hit;
        input.ground_y = height;
        input.ground_normal = [0.0, 1.0, 0.0];
    }
    hit as u8
}

/// No ODE ray per wheel: `Tyre::step` then asks `rayCast` above.
extern "C" fn track_create_ray_caster(_this: *mut u8, _length: f32) -> *mut u8 {
    std::ptr::null_mut()
}

// --- the controls device ---------------------------------------------------------------------

extern "C" fn device_destructor(_this: *mut u8, _flags: u32) -> *mut u8 {
    std::ptr::null_mut()
}

/// `ICarControlsProvider::acquireControls(CarControls&, float dt, CarControlsInput&)`. Like the
/// game's keyboard device it writes only what it has; the rest of `Car::controls` keeps the
/// values the constructor gave it (no H-shifter request, no handbrake …).
extern "C" fn device_acquire_controls(_this: *mut u8, controls: *mut u8, _dt: f32, _input: *const f32) {
    let st = state();
    let c = st.controls;
    st.polled += 1;
    unsafe {
        wr(controls, CC_GEAR_UP, c.gear_up as u8);
        wr(controls, CC_GEAR_DN, c.gear_dn as u8);
        wr(controls, CC_HAND_BRAKE, 0.0f32);
        wr(controls, CC_GAS, c.gas);
        wr(controls, CC_BRAKE, c.brake);
        wr(controls, CC_STEER, c.steer);
        wr(controls, CC_CLUTCH, c.clutch);
    }
}

/// `getAction(action)`: no button is ever pressed.
extern "C" fn device_get_action(_this: *mut u8, _action: i32) -> u8 {
    0
}
extern "C" fn device_send_ff(_this: *mut u8, _force: f32, _damper: f32, _gain: f32) {}
extern "C" fn device_get_ff_global_gain(_this: *mut u8) -> f32 {
    1.0
}
extern "C" fn device_true(_this: *mut u8) -> u8 {
    1
}
extern "C" fn device_false(_this: *mut u8) -> u8 {
    0
}
extern "C" fn device_set_vibrations(_this: *mut u8, _def: *const u8) {}
extern "C" fn device_set_engine_rpm(_this: *mut u8, _rpm: f32, _limiter: f32) {}
extern "C" fn device_unexpected<const SLOT: usize>() {
    eprintln!("the game called controls-device slot +{:#x}, which the oracle does not provide", SLOT * 8);
    std::process::exit(6);
}

// --- function wrappers -----------------------------------------------------------------------

extern "C" fn tyre_step_hook(tyre: *mut u8, dt: f32) {
    let st = state();
    let original: extern "C" fn(*mut u8, f32) = unsafe { std::mem::transmute(st.tyre_step_original) };
    let offset = (tyre as usize).wrapping_sub(st.car as usize + CAR_TYRES);
    if offset % TYRE_SIZE != 0 || offset / TYRE_SIZE >= 4 {
        return original(tyre, dt);
    }
    let wheel = offset / TYRE_SIZE;
    unsafe {
        let car = st.car;
        let body = &st.bodies[0];
        let input = StepInput {
            hub_matrix: [0.0; 16],
            hub_velocity: [0.0; 3],
            hub_angular_velocity: [0.0; 3],
            brake_torque: rd(tyre, T_INPUTS),
            hand_brake_torque: rd(tyre, T_INPUTS + 4),
            electric_torque: rd(tyre, T_INPUTS + 8),
            abs_override: rd(tyre, T_ABS_OVERRIDE),
            ai_mult: rd(tyre, T_AI_MULT),
            driven: rd::<u8>(tyre, T_DRIVEN) != 0,
            // whatever the drivetrain left in the wheel's spin since the last tyre step
            set_angular_velocity: Some(rd(tyre, T_STATUS + S_ANGULAR_VELOCITY)),
            set_blankets: Some(rd::<u8>(tyre, T_BLANKETS_ON) != 0),
            ext_active: rd::<u8>(tyre, T_EXTERNAL_INPUTS) != 0,
            ext_load: rd(tyre, T_EXTERNAL_INPUTS + 4),
            ext_slip_angle: rd(tyre, T_EXTERNAL_INPUTS + 8),
            ext_slip_ratio: rd(tyre, T_EXTERNAL_INPUTS + 0xc),
            has_hit: false,
            ground_y: 0.0,
            ground_normal: [0.0; 3],
            grip_mod: rd(st.surface, SD_GRIP_MOD),
            dirt_additive_k: rd(st.surface, 0x98),
            sin_height: rd(st.surface, 0xa8),
            sin_length: rd(st.surface, 0xac),
            damping: rd(st.surface, 0xb4),
            granularity: rd(st.surface, 0xb8),
            has_car: true,
            torque_mode: rd(car, CAR_TORQUE_MODE_EX),
            car_speed: rd(car, CAR_VALUE_CACHE_SPEED),
            car_sleeping: rd::<i32>(car, CAR_SLEEPING_FRAMES) > rd::<i32>(car, CAR_FRAMES_TO_SLEEP),
            dynamic_grip_level: rd(st.track, TRACK_DYNAMIC_GRIP_LEVEL),
            tyre_consumption_rate: rd(st.engine, PE_TYRE_CONSUMPTION_RATE),
            mechanical_damage_rate: rd(st.engine, PE_MECHANICAL_DAMAGE_RATE),
            ambient_temperature: rd(st.engine, PE_AMBIENT_TEMPERATURE),
            road_temperature: rd(st.engine, PE_ROAD_TEMPERATURE),
            allow_tyre_blankets: rd::<u8>(st.engine, PE_ALLOW_TYRE_BLANKETS) != 0,
            body_velocity: v3(body.ode, B_LVEL),
            body_mass: rd(body.ode, B_MASS),
        };
        st.tyres[wheel] = TyreCapture {
            input: Some(input),
            wheel_rotation_in: rd(tyre, T_LOCAL_WHEEL_ROTATION),
            ..TyreCapture::default()
        };
    }
    st.tyre = Some(wheel);
    original(tyre, dt);
    let st = state();
    st.tyre = None;
    st.tyres[wheel].output = tyre_snapshot(tyre, &st.tyres[wheel].calls);
}

extern "C" fn world_step_hook(world: *mut u8, step: f32) -> i32 {
    let st = state();
    let original: extern "C" fn(*mut u8, f32) -> i32 = unsafe { std::mem::transmute(st.world_step_original) };
    unsafe {
        st.pre = st.bodies.iter().map(|b| body_state(b.ode)).collect();
        st.joint_params = st
            .joints
            .iter()
            .map(|j| {
                let mut out = [0f32; JOINT_PARAM_WORDS];
                for (slot, (_, offset)) in out.iter_mut().zip(joint_params(j.kind)) {
                    *slot = rd(j.ode, J_PARAMS + offset);
                }
                out
            })
            .collect();
        for joint in &st.joints {
            if !joint.feedback.is_null() {
                *joint.feedback = [0.0; 16];
            }
        }
    }
    let result = original(world, step);
    let st = state();
    st.post = st.bodies.iter().map(|b| unsafe { body_state(b.ode) }).collect();
    st.world_steps += 1;
    result
}

/// `dxStepIsland_Stage0_Joints`: entered once per island right after the stepper has added
/// gravity and the gyroscopic torque to the accumulators.
extern "C" fn stage0_joints_hook(a: usize, b: usize, c: usize, d: usize) -> usize {
    let st = state();
    let original: extern "C" fn(usize, usize, usize, usize) -> usize =
        unsafe { std::mem::transmute(st.stage0_original) };
    st.solver = st.bodies.iter().map(|body| unsafe { (v3(body.ode, B_FACC), v3(body.ode, B_TACC)) }).collect();
    original(a, b, c, d)
}

// --- reading a tyre the way the single-wheel rig does -----------------------------------------

/// The game-side twin of `rig::snapshot`, in `rig::output_fields()` order.
fn tyre_snapshot(p: *const u8, calls: &[rig::Call]) -> Vec<u64> {
    let f32_at = |offset: usize| unsafe { rd::<u32>(p, offset) } as u64;
    let patches: *const u8 = unsafe { rd(p, T_THERMAL + TH_PATCHES) };
    rig::output_fields()
        .iter()
        .map(|name| {
            let name = name.as_str();
            if let Some(rest) = name.strip_prefix("call") {
                if rest == "s" {
                    return calls.len() as u64;
                }
                let (index, member) = rest.split_once('.').unwrap();
                let call = calls.get(index.parse::<usize>().unwrap()).copied().unwrap_or_default();
                let axis = |v: [f32; 3]| v["xyz".find(&member[1..]).unwrap()].to_bits() as u64;
                return match member {
                    "kind" => call.kind as u64,
                    "flags" => call.flags as u64,
                    _ if member.starts_with('a') => axis(call.a),
                    _ if member.starts_with('b') => axis(call.b),
                    _ => axis(call.c),
                };
            }
            if let Some(member) = name.strip_prefix("status.") {
                let (offset, kind) = status_member(member);
                return match kind {
                    'd' => unsafe { rd::<u64>(p, T_STATUS + offset) },
                    'b' => (unsafe { rd::<u8>(p, T_STATUS + offset) }) as u64,
                    _ => f32_at(T_STATUS + offset),
                };
            }
            if let Some(cell) = name.strip_prefix("localWheelRotation.M") {
                let row = cell.as_bytes()[0] as usize - b'1' as usize;
                let col = cell.as_bytes()[1] as usize - b'1' as usize;
                return f32_at(T_LOCAL_WHEEL_ROTATION + (row * 4 + col) * 4);
            }
            if let Some(rest) = name.strip_prefix("thermal.patch") {
                let (index, member) = rest.split_once('.').unwrap();
                let offset = index.parse::<usize>().unwrap() * PATCH_SIZE + if member == "T" { 0x18 } else { 0x1c };
                return unsafe { rd::<u32>(patches, offset) } as u64;
            }
            match name {
                "thermal.phase" => unsafe { rd::<u64>(p, T_THERMAL + 0x20) },
                "thermal.coreTemp" => f32_at(T_THERMAL + 0x3c),
                "thermal.thermalMultD" => f32_at(T_THERMAL + 0xc4),
                "thermal.practicalTemp" => f32_at(T_THERMAL + 0xc8),
                "thermal.coreTInput" => f32_at(T_THERMAL + 0xd8),
                "hasSurfaceDef" => (unsafe { rd::<usize>(p, T_SURFACE_DEF) } != 0) as u64,
                "tyreBlanketsOn" => (unsafe { rd::<u8>(p, T_BLANKETS_ON) }) as u64,
                _ => f32_at(tyre_member(name)),
            }
        })
        .collect()
}

/// Offset inside `TyreStatus` and kind (`f` f32, `d` f64, `b` bool) of a recorded member.
fn status_member(name: &str) -> (usize, char) {
    match name {
        "depth" => (0x0, 'f'),
        "load" => (0x4, 'f'),
        "camberRAD" => (0x8, 'f'),
        "slipAngleRAD" => (0xc, 'f'),
        "slipRatio" => (0x10, 'f'),
        "angularVelocity" => (0x14, 'f'),
        "Fy" => (0x18, 'f'),
        "Fx" => (0x1c, 'f'),
        "Mz" => (0x20, 'f'),
        "isLocked" => (0x24, 'b'),
        "slipFactor" => (0x28, 'f'),
        "ndSlip" => (0x2c, 'f'),
        "distToGround" => (0x30, 'f'),
        "Dy" => (0x34, 'f'),
        "Dx" => (0x38, 'f'),
        "D" => (0x3c, 'f'),
        "dirtyLevel" => (0x40, 'f'),
        "rollingResistence" => (0x44, 'f'),
        "thermalInput" => (0x48, 'f'),
        "feedbackTorque" => (0x4c, 'f'),
        "loadedRadius" => (0x50, 'f'),
        "effectiveRadius" => (0x54, 'f'),
        "liveRadius" => (0x58, 'f'),
        "pressureStatic" => (0x5c, 'f'),
        "pressureDynamic" => (0x60, 'f'),
        "virtualKM" => (0x68, 'd'),
        "lastTempIMO0" => (0x70, 'f'),
        "lastTempIMO1" => (0x74, 'f'),
        "lastTempIMO2" => (0x78, 'f'),
        "peakSA" => (0x7c, 'f'),
        "grain" => (0x80, 'd'),
        "blister" => (0x88, 'd'),
        "inflation" => (0x90, 'f'),
        "flatSpot" => (0x98, 'd'),
        "lastGrain" => (0xa0, 'f'),
        "lastBlister" => (0xa4, 'f'),
        "normalizedSlideX" => (0xa8, 'f'),
        "normalizedSlideY" => (0xac, 'f'),
        "finalDY" => (0xb0, 'f'),
        "wearMult" => (0xb4, 'f'),
        other => panic!("no offset for status.{other}"),
    }
}

/// Offset inside `Tyre` of a recorded f32 member.
fn tyre_member(name: &str) -> usize {
    let (member, axis) = match name.split_once('.') {
        Some((member, axis)) => (member, "xyz".find(axis).unwrap() * 4),
        None => (name, 0),
    };
    axis + match member {
        "worldPosition" => 0x5f8,
        "unmodifiedContactPoint" => 0x3e8,
        "contactPoint" => 0x3f4,
        "contactNormal" => 0x400,
        "roadRight" => 0x588,
        "roadHeading" => 0x594,
        "absOverride" => T_ABS_OVERRIDE,
        "oldAngularVelocity" => 0x5b0,
        "totalSlideVelocity" => 0x5b4,
        "slidingVelocityY" => 0x604,
        "slidingVelocityX" => 0x608,
        "roadVelocityX" => 0x60c,
        "roadVelocityY" => 0x610,
        "totalHubVelocity" => 0x614,
        "rSlidingVelocityX" => 0x618,
        "rSlidingVelocityY" => 0x61c,
        "localMX" => 0x854,
        other => panic!("no offset for {other}"),
    }
}

// --- the world -------------------------------------------------------------------------------

pub const WHEELS: [&str; 4] = ["lf", "rf", "lr", "rr"];
pub const CAR_NAME: &str = "ks_ferrari_f2004";
/// Fixed weather: the same values the game's `PhysicsEngine` constructor starts with.
pub const AMBIENT_TEMPERATURE: f32 = 26.0;
pub const ROAD_TEMPERATURE: f32 = 30.0;

pub struct Options {
    /// Ask ODE for the force each joint applies (`dJointSetFeedback`). The game never does;
    /// see the report for what it changes.
    pub joint_feedback: bool,
}

/// One car on the fake track, ready to be stepped.
pub struct World<'a> {
    acs: &'a Acs,
    pub engine: *mut u8,
    pub track: *mut u8,
    pub car: *mut u8,
    driver: Driver,
    pub step_index: usize,
    /// A `SharedMemoryWriter` laid out by hand, its fake `CarAvatar`, and the page it fills.
    telemetry_writer: *mut u8,
    avatar: *mut u8,
    page: *mut u8,
    pub joint_feedback: bool,
}

/// Builds the small game folder the engine, track and car read their files from.
pub fn prepare_root(repo: &Path, root: &Path) -> Result<(), String> {
    let io = |e: std::io::Error| e.to_string();
    std::fs::create_dir_all(root.join("system/cfg")).map_err(io)?;
    // THREADS=0: no thread pool, the whole step runs on the calling thread. The other keys
    // only feed the force-feedback numbers.
    std::fs::write(
        root.join("system/cfg/assetto_corsa.ini"),
        "[PHYSICS_THREADING]\nTHREADS=0\n[FF_EXPERIMENTAL]\nENABLE_GYRO=0\nDAMPER_MIN_LEVEL=0\nDAMPER_GAIN=1\n",
    )
    .map_err(io)?;
    // the track "flat" has no files at all: no AI line, no surfaces, no DRS zones
    let _ = std::fs::remove_dir_all(root.join("content/tracks"));
    let _ = std::fs::remove_file(root.join("system/cfg/tyre_smoke.ini"));
    let from = repo.join("cardata").join(CAR_NAME);
    let to = root.join("content/cars").join(CAR_NAME).join("data");
    std::fs::create_dir_all(&to).map_err(io)?;
    for entry in std::fs::read_dir(&from).map_err(|e| format!("{}: {e}", from.display()))? {
        let entry = entry.map_err(io)?;
        if entry.file_type().map_err(io)?.is_file() {
            std::fs::copy(entry.path(), to.join(entry.file_name())).map_err(io)?;
        }
    }
    Ok(())
}

impl<'a> World<'a> {
    /// The current directory must be the folder made by [`prepare_root`].
    pub fn build(acs: &'a Acs, scenario: &Scenario, options: &Options) -> World<'a> {
        unsafe {
            // the game's rand() and the oracle's clock start from the same point every run
            let srand: extern "C" fn(u32) = std::mem::transmute(acs.crt_function(c"srand"));
            srand(scenario.seed);
            crate::acs::reset_clock();

            // globals the game's start-up would have set (see the report for each)
            assert_eq!(acs.global::<u8>(VA_IS_USING_QPT), 1, "ksTimer is not in QueryPerformanceCounter mode");
            assert_eq!(acs.global::<u8>(VA_IS_TEST_MODE), 0, "PhysicsEngine::isTestMode is set");
            acs.set_global(VA_TIMER_START, 0i64);
            acs.set_global(VA_TIMER_FREQUENCY, crate::acs::CLOCK_FREQUENCY as i64);
            // "already initialised" with an empty base path: cfg/race.ini is then looked for in
            // the working directory (where there is none) instead of the user's Documents folder
            acs.set_global(VA_INIREADERDOCUMENTS_INITIALIZED, 1u8);

            let engine = acs.alloc(PE_SIZE);
            let ctor: extern "C" fn(*mut u8) -> *mut u8 = std::mem::transmute(acs.va(VA_PHYSICS_ENGINE_CTOR));
            ctor(engine);
            // the constructor stores the wall clock here; the car's once-a-second mass refresh
            // is timed from it
            wr(engine, PE_PHYSICS_TIME, 0.0f64);
            wr(engine, PE_GAME_TIME, 0.0f64);
            wr(engine, PE_AMBIENT_TEMPERATURE, AMBIENT_TEMPERATURE);
            wr(engine, PE_ROAD_TEMPERATURE, ROAD_TEMPERATURE);

            let track = acs.alloc(TRACK_SIZE);
            let ctor: extern "C" fn(*mut u8, *mut u8, *mut u8, *mut u8) -> *mut u8 =
                std::mem::transmute(acs.va(VA_TRACK_CTOR));
            ctor(track, engine, wstring(acs, "flat"), wstring(acs, ""));
            let init_ai_spline: extern "C" fn(*mut u8) = std::mem::transmute(acs.va(VA_TRACK_INIT_AI_SPLINE));
            init_ai_spline(track);

            // one surface: full grip, valid track, nothing else
            let surface = object(SURFACE_SIZE);
            wr(surface, SD_GRIP_MOD, 1.0f32);
            wr(surface, SD_COLLISION_CATEGORY, 1u32);
            wr(surface, SD_IS_VALID_TRACK, 1u8);

            // the track answers the tyres' rays itself: a copy of its vtable (with the RTTI
            // pointer in front) in which rayCast and createRayCaster are ours
            let original: *const usize = rd(track, 0);
            let mut table: Vec<usize> = (0..5).map(|i| *original.offset(i as isize - 1)).collect();
            table[1 + 1] = track_ray_cast as *const () as usize;
            table[1 + 3] = track_create_ray_caster as *const () as usize;
            wr(track, 0, leak(table).add(1));

            *STATE.0.get() = Some(State {
                car: std::ptr::null_mut(),
                engine,
                track,
                surface,
                ground: scenario.ground,
                bodies: Vec::new(),
                joints: Vec::new(),
                body_vtable: acs.va(VA_RIGID_BODY_VTABLE) as *const usize,
                suspensions: [std::ptr::null_mut(); 4],
                suspension_vtables: [std::ptr::null(); 4],
                tyre_step_original: 0,
                world_step_original: 0,
                controls: Controls { clutch: 1.0, ..Controls::default() },
                polled: 0,
                tape: Vec::new(),
                outer_site: 0,
                tyre: None,
                tyres: Default::default(),
                pre: Vec::new(),
                post: Vec::new(),
                solver: Vec::new(),
                joint_params: Vec::new(),
                stage0_original: 0,
                world_steps: 0,
                image_base: acs.va(crate::acs::GHIDRA_BASE),
            });

            let car = acs.alloc(CAR_SIZE);
            let ctor: extern "C" fn(*mut u8, *mut u8, *mut u8, *mut u8) -> *mut u8 =
                std::mem::transmute(acs.va(VA_CAR_CTOR));
            ctor(car, engine, wstring(acs, CAR_NAME), wstring(acs, ""));
            assert_eq!(rd::<u32>(car, CAR_PHYSICS_GUID), 0, "the car is not car 0");
            let st = state();
            st.car = car;

            // a mesh collider is mandatory (Car::updateColliderStatus reads mesh 0 unchecked): a
            // small box inside the car, which never touches anything on a track without meshes
            let mesh = object(0x168);
            let mut vertices = vec![0f32; 8 * 11];
            for i in 0..8 {
                vertices[i * 11] = if i & 1 == 0 { -0.3 } else { 0.3 };
                vertices[i * 11 + 1] = if i & 2 == 0 { 0.3 } else { 0.5 };
                vertices[i * 11 + 2] = if i & 4 == 0 { -0.5 } else { 0.5 };
            }
            let indices: Vec<u16> =
                vec![0, 1, 2, 1, 3, 2, 4, 6, 5, 5, 6, 7, 0, 4, 1, 1, 4, 5, 2, 3, 6, 3, 7, 6, 0, 2, 4, 2, 6, 4, 1, 5, 3, 3, 5, 7];
            let vp = leak(vertices).cast::<u8>();
            wr(mesh, 0x108, vp);
            wr(mesh, 0x110, vp.add(8 * 0x2c));
            wr(mesh, 0x118, vp.add(8 * 0x2c));
            let n = indices.len();
            let ip = leak(indices).cast::<u8>();
            wr(mesh, 0x120, ip);
            wr(mesh, 0x128, ip.add(n * 2));
            wr(mesh, 0x130, ip.add(n * 2));
            let identity: [f32; 16] = [1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.];
            let init_mesh: extern "C" fn(*mut u8, *mut u8, *const [f32; 16]) =
                std::mem::transmute(acs.va(VA_CAR_INIT_COLLIDER_MESH));
            init_mesh(car, mesh, &identity);

            // the controls device: 12 virtual functions, with the RTTI pointer of a real device
            // in front so that the game's dynamic_cast<AIDriver*> answers "not an AI"
            let keyboard = acs.va(VA_KEYBOARD_CONTROL_VTABLE) as *const usize;
            let table: Vec<usize> = vec![
                *keyboard.offset(-1),
                device_destructor as *const () as usize,              // +0x00
                device_acquire_controls as *const () as usize,        // +0x08
                device_get_action as *const () as usize,              // +0x10
                device_send_ff as *const () as usize,                 // +0x18
                device_unexpected::<4> as *const () as usize,         // +0x20 getName
                device_get_ff_global_gain as *const () as usize,      // +0x28
                device_true as *const () as usize,                    // +0x30 isDeviceConnected
                device_unexpected::<7> as *const () as usize,         // +0x38
                device_false as *const () as usize,                   // +0x40 IsKeyboardControl
                device_set_vibrations as *const () as usize,          // +0x48
                device_set_engine_rpm as *const () as usize,          // +0x50
                device_false as *const () as usize,                   // +0x58
            ];
            let device = object(0x40);
            wr(device, 0, leak(table).add(1));
            wr(car, CAR_CONTROLS_PROVIDER, device);

            // two bookkeeping helpers of the player's car that only fill buffers: off
            wr(car, CAR_PERFORMANCE_METER_IS_ENABLED, 0u8);
            wr(car, CAR_TELEMETRY_IS_ENABLED, 0u8);
            // driving aids: the automatic clutch is the scenario's choice
            wr(car, CAR_AUTOCLUTCH + 0xc, scenario.auto_clutch as u8); // useAutoOnStart
            wr(car, CAR_AUTOCLUTCH + 0xd, scenario.auto_clutch as u8); // useAutoOnChange

            let world = World {
                acs,
                engine,
                track,
                car,
                driver: scenario.driver(),
                step_index: 0,
                telemetry_writer: object(SMW_SIZE),
                avatar: object(AVATAR_SIZE),
                page: object(0x1000),
                joint_feedback: options.joint_feedback,
            };
            world.find_bodies_and_joints(options);
            world.install_hooks();
            // the game's telemetry writer, pointed at this car, a zeroed avatar (it supplies the
            // cockpit settings: engine-brake and ERS levels, all 0 here) and our page; what
            // CarAvatar::initPhysics would have copied into its physicsInfo is filled in
            let writer = world.telemetry_writer;
            wr(writer, SMW_CAR, car);
            wr(writer, SMW_AVATAR, world.avatar);
            wr(writer, SMW_LAP_INVALIDATOR, car.add(CAR_LAP_INVALIDATOR));
            wr(writer, SMW_PHYSICS_BUFFER, world.page);
            wr(writer, SMW_PHYSICS_NULL_COUNTS, 300i32); // the 300-packet warm-up is over
            for i in 0..4 {
                let suspension = state().suspensions[i];
                wr(writer, SMW_BUMP_STOPS_UP + i * 4, rd::<f32>(suspension, SUS_BUMP_STOP_UP));
                wr(writer, SMW_BUMP_STOPS_DN + i * 4, rd::<f32>(suspension, SUS_BUMP_STOP_DN));
            }

            // on the road, facing +z (forceRotation takes the direction the car's tail points)
            let force_rotation: extern "C" fn(*mut u8, *const V3) = std::mem::transmute(acs.va(VA_CAR_FORCE_ROTATION));
            let force_position: extern "C" fn(*mut u8, *const V3, u8) =
                std::mem::transmute(acs.va(VA_CAR_FORCE_POSITION));
            // spawned the way the game spawns a car
            force_rotation(car, &[0.0, 0.0, -1.0]);
            force_position(car, &[0.0, 0.0, 0.0], 1);
            let set_damage_level: extern "C" fn(*mut u8, f32) = std::mem::transmute(acs.va(VA_CAR_SET_DAMAGE_LEVEL));
            set_damage_level(car, 0.0);
            let reset_suspension_damage: extern "C" fn(*mut u8) =
                std::mem::transmute(acs.va(VA_CAR_RESET_SUSPENSION_DAMAGE));
            reset_suspension_damage(car);
            state().tape.clear();
            world
        }
    }

    unsafe fn find_bodies_and_joints(&self, options: &Options) {
        let st = state();
        let car = self.car;
        let body_ref = |name: &str, wrapper: *mut u8| BodyRef {
            name: name.to_string(),
            wrapper,
            ode: rd(wrapper, RB_ODE_BODY),
        };
        st.bodies.push(body_ref("body", rd(car, CAR_BODY)));
        st.bodies.push(body_ref("fuel_tank", rd(car, CAR_FUEL_TANK_BODY)));
        let begin: *const *mut u8 = rd(car, CAR_SUSPENSIONS);
        let end: *const *mut u8 = rd(car, CAR_SUSPENSIONS + 8);
        assert_eq!(end.offset_from(begin), 4, "the car does not have four suspensions");
        for (i, wheel) in WHEELS.iter().enumerate() {
            let suspension = *begin.add(i);
            assert_eq!(
                rd::<usize>(suspension, 0),
                self.acs.va(VA_SUSPENSION_VTABLE),
                "wheel {wheel} is not a double-wishbone suspension (the only type the oracle knows)"
            );
            st.suspensions[i] = suspension;
            st.bodies.push(body_ref(&format!("hub_{wheel}"), rd(suspension, SUS_HUB)));
        }
        for body in &st.bodies {
            assert_eq!(rd::<usize>(body.wrapper, 0), st.body_vtable as usize, "{} is not a RigidBodyODE", body.name);
        }

        // every joint of the car hangs on the car body: walk ODE's list (newest first)
        let mut found = Vec::new();
        let mut node: *const u8 = rd(st.bodies[0].ode, B_FIRST_JOINT);
        while !node.is_null() {
            found.push(rd::<*mut u8>(node, 0));
            node = rd(node, 0x10);
        }
        found.reverse();
        let mut names: Vec<(usize, String)> = Vec::new();
        for (i, wheel) in WHEELS.iter().enumerate() {
            for (k, rod) in SUS_JOINT_NAMES.iter().enumerate() {
                let wrapper: *const u8 = rd(st.suspensions[i], SUS_JOINTS + k * 8);
                names.push((rd(wrapper, JOINT_WRAPPER_ODE_JOINT), format!("{wheel}.{rod}")));
            }
        }
        let tank: *const u8 = rd(car, CAR_FUEL_TANK_JOINT);
        names.push((rd(tank, JOINT_WRAPPER_ODE_JOINT), "fuel_tank".to_string()));
        for ode in found {
            let vtable: *const usize = rd(ode, 0);
            let type_of: extern "C" fn(*mut u8) -> u32 = std::mem::transmute(*vtable.add(4));
            let body_of = |offset: usize| {
                let body: *mut u8 = rd(ode, offset);
                st.bodies.iter().position(|b| b.ode == body).unwrap_or(usize::MAX)
            };
            let name = names
                .iter()
                .find(|(joint, _)| *joint == ode as usize)
                .map(|(_, name)| name.clone())
                .unwrap_or_else(|| format!("joint{}", st.joints.len()));
            let feedback = if options.joint_feedback {
                let block = object(64).cast::<[f32; 16]>();
                wr(ode, J_FEEDBACK, block);
                block
            } else {
                std::ptr::null_mut()
            };
            st.joints.push(JointRef {
                name,
                kind: type_of(ode),
                ode,
                body0: body_of(J_BODY0),
                body1: body_of(J_BODY1),
                feedback,
            });
        }
    }

    unsafe fn install_hooks(&self) {
        let st = state();
        // rigid bodies: one patched copy of the class vtable for the six bodies
        let mut table: Vec<usize> = (0..RIGID_BODY_SLOTS + 1).map(|i| *st.body_vtable.offset(i as isize - 1)).collect();
        let mut set = |offset: usize, f: extern "C" fn()| table[1 + offset / 8] = f as usize;
        set(0x110, body_add_force_at_pos);
        set(0xf8, body_add_force_at_local_pos);
        set(0xd8, body_add_local_force);
        set(0xe8, body_add_local_force_at_pos);
        set(0xf0, body_add_local_force_at_local_pos);
        set(0x118, body_add_torque);
        set(0xe0, body_add_local_torque);
        set(0x70, body_stop);
        set(0x80, body_set_velocity);
        set(0x88, body_set_angular_velocity);
        set(0x90, body_set_position);
        let table = leak(table).add(1);
        for body in &st.bodies {
            wr(body.wrapper, 0, table);
        }
        // suspensions: the six functions the tyre uses
        for i in 0..4 {
            let original: *const usize = rd(st.suspensions[i], 0);
            st.suspension_vtables[i] = original;
            let mut table: Vec<usize> = (0..SUSPENSION_SLOTS + 1).map(|k| *original.offset(k as isize - 1)).collect();
            table[1 + 1] = sus_get_hub_world_matrix as *const () as usize;
            table[1 + 2] = sus_get_point_velocity as *const () as usize;
            table[1 + 3] = sus_add_force_at_pos as *const () as usize;
            table[1 + 4] = sus_add_torque as *const () as usize;
            table[1 + 7] = sus_get_hub_angular_velocity as *const () as usize;
            table[1 + 24] = sus_add_local_force_and_torque as *const () as usize;
            wr(st.suspensions[i], 0, leak(table).add(1));
        }
        // Tyre::step: mov r11,rsp / push rbp / push rdi / lea rbp,[r11-0xe8]
        st.tyre_step_original = self.acs.detour(
            VA_TYRE_STEP,
            12,
            &[0x4c, 0x8b, 0xdc, 0x55, 0x57, 0x49, 0x8d, 0xab, 0x18, 0xff, 0xff, 0xff],
            tyre_step_hook as *const () as usize,
        );
        // dWorldStep: mov [rsp+8],rbx / push rdi / sub rsp,0x60 / lea r9,[rip+…]. The lea is
        // position-dependent, so the copy loads the same address as a constant instead.
        let mut copy = vec![0x48, 0x89, 0x5c, 0x24, 0x08, 0x57, 0x48, 0x83, 0xec, 0x60, 0x49, 0xb9];
        copy.extend((self.acs.va(VA_STEP_MEMORY_ESTIMATE) as u64).to_le_bytes());
        st.world_step_original = self.acs.detour_with(
            VA_DWORLDSTEP,
            17,
            &[0x48, 0x89, 0x5c, 0x24, 0x08, 0x57, 0x48, 0x83, 0xec, 0x60, 0x4c, 0x8d, 0x0d, 0x2f, 0xfb, 0x00, 0x00],
            &copy,
            world_step_hook as *const () as usize,
        );
        // dxStepIsland_Stage0_Joints: mov [rsp+8],rcx / push rbx, rbp, rsi, rdi, r12, r13, r14
        st.stage0_original = self.acs.detour(
            VA_STEP_STAGE0_JOINTS,
            15,
            &[0x48, 0x89, 0x4c, 0x24, 0x08, 0x53, 0x55, 0x56, 0x57, 0x41, 0x54, 0x41, 0x55, 0x41, 0x56],
            stage0_joints_hook as *const () as usize,
        );
    }

    pub fn bodies(&self) -> Vec<String> {
        state().bodies.iter().map(|b| b.name.clone()).collect()
    }

    pub fn joints(&self) -> Vec<JointRef> {
        state().joints.clone()
    }

    fn view(&self) -> CarView {
        unsafe {
            let car = self.car;
            let body = state().bodies[0].ode;
            // yaw rate about the body's own up axis (second column of R); the car's x axis
            // points to its left, so a turn to the right is a negative rotation about "up"
            let r: [f32; 12] = rd(body, B_R);
            let w = v3(body, B_AVEL);
            let up = [r[1], r[5], r[9]];
            let speed: f32 = rd(car, CAR_VALUE_CACHE_SPEED);
            // rear (driven) wheel radius: TyreStatus::effectiveRadius of the left rear tyre
            let radius: f32 = rd(car, CAR_TYRES + 2 * TYRE_SIZE + T_STATUS + 0x54);
            let ratio = rd::<f64>(car, CAR_DRIVETRAIN + 0xc0) as f32;
            CarView {
                step: self.step_index,
                speed,
                rpm: (rd::<f64>(car, CAR_DRIVETRAIN + 0x8) * 9.549296585513721) as f32,
                road_rpm: if radius > 0.0 { speed / radius * ratio.abs() * 9.549_296_6 } else { 0.0 },
                gear: rd(car, CAR_DRIVETRAIN + 0x584),
                yaw_rate: -(w[0] * up[0] + w[1] * up[1] + w[2] * up[2]),
            }
        }
    }

    /// One physics step: the script's controls, `PhysicsEngine::step`, and everything that
    /// was seen on the way.
    pub fn step(&mut self, naming: bool) -> (Row, Vec<Call>) {
        let view = self.view();
        let controls = self.driver.controls(&view);
        let st = state();
        st.controls = controls;
        st.polled = 0;
        st.tape.clear();
        for capture in &mut st.tyres {
            *capture = TyreCapture::default();
        }
        let before_world_steps = st.world_steps;
        let time_ms = (self.step_index + 1) as f64 * 3.0;
        let step: extern "C" fn(*mut u8, f32, f64, f64) =
            unsafe { std::mem::transmute(self.acs.va(VA_PHYSICS_ENGINE_STEP)) };
        step(self.engine, DT, time_ms, time_ms);
        let st = state();
        assert_eq!(st.world_steps, before_world_steps + 1, "dWorldStep did not run exactly once");
        assert_eq!(st.polled, 1, "the controls device was not polled exactly once");

        // what the game does after the step: the state snapshot for the main thread (the
        // telemetry writer reads ride heights and the limiter from the avatar's copy of it),
        // then the shared-memory physics page
        unsafe {
            let get_physics_state: extern "C" fn(*mut u8, *mut u8) =
                std::mem::transmute(self.acs.va(VA_CAR_GET_PHYSICS_STATE));
            get_physics_state(self.car, self.avatar.add(AVATAR_PHYSICS_STATE));
            let update_physics: extern "C" fn(*mut u8, *const f64) =
                std::mem::transmute(self.acs.va(VA_SHARED_MEMORY_UPDATE_PHYSICS));
            update_physics(self.telemetry_writer, &time_ms);
            assert_eq!(
                rd::<i32>(self.telemetry_writer, SMW_PHYSICS_PACKET_ID),
                self.step_index as i32 + 1,
                "the telemetry writer did not write a page"
            );
        }

        let mut row = Row::new(naming);
        row.i("step", self.step_index as i32);
        row.d("time_ms", time_ms);
        unsafe { self.emit(&mut row, &controls) };
        let calls = std::mem::take(&mut state().tape);
        self.step_index += 1;
        (row, calls)
    }

    unsafe fn emit(&self, row: &mut Row, script: &Controls) {
        let st = state();
        let car = self.car;
        // what the device reported
        row.f("script.steer", script.steer);
        row.f("script.gas", script.gas);
        row.f("script.brake", script.brake);
        row.f("script.clutch", script.clutch);
        row.i("script.gearUp", script.gear_up as i32);
        row.i("script.gearDn", script.gear_dn as i32);
        // Car::controls at the end of the step: after the game's own overrides and helpers
        let c = car.add(CAR_CONTROLS);
        row.f("controls.steer", rd(c, CC_STEER));
        row.f("controls.gas", rd(c, CC_GAS));
        row.f("controls.brake", rd(c, CC_BRAKE));
        row.f("controls.clutch", rd(c, CC_CLUTCH));
        row.f("controls.handBrake", rd(c, CC_HAND_BRAKE));
        row.i("controls.gearUp", rd::<u8>(c, CC_GEAR_UP) as i32);
        row.i("controls.gearDn", rd::<u8>(c, CC_GEAR_DN) as i32);
        row.i("controls.requestedGearIndex", rd(c, CC_REQUESTED_GEAR_INDEX));
        row.f("car.finalSteerAngleSignal", rd(car, CAR_FINAL_STEER_ANGLE_SIGNAL));

        // bodies: as dWorldStep found them (state the car's systems worked with, accumulators
        // full) and as it left them
        for (i, body) in st.bodies.iter().enumerate() {
            let (pre, post) = (&st.pre[i], &st.post[i]);
            let n = &body.name;
            row.f(&format!("{n}.mass"), pre.mass);
            row.v(&format!("{n}.inertia"), &pre.inertia);
            row.v(&format!("{n}.pre.pos"), &pre.pos);
            row.v(&format!("{n}.pre.q"), &pre.q);
            row.v(&format!("{n}.pre.R"), &pre.r);
            row.v(&format!("{n}.pre.lvel"), &pre.lvel);
            row.v(&format!("{n}.pre.avel"), &pre.avel);
            row.v(&format!("{n}.facc"), &pre.facc);
            row.v(&format!("{n}.tacc"), &pre.tacc);
            row.v(&format!("{n}.solver.facc"), &st.solver[i].0);
            row.v(&format!("{n}.solver.tacc"), &st.solver[i].1);
            row.i(&format!("{n}.tag"), rd(body.ode, B_TAG));
            row.v(&format!("{n}.post.pos"), &post.pos);
            row.v(&format!("{n}.post.q"), &post.q);
            row.v(&format!("{n}.post.R"), &post.r);
            row.v(&format!("{n}.post.lvel"), &post.lvel);
            row.v(&format!("{n}.post.avel"), &post.avel);
        }
        // joints: type-specific data as dWorldStep found it, and the forces it applied
        for (i, joint) in st.joints.iter().enumerate() {
            let n = format!("joint.{}", joint.name);
            for ((name, _), &value) in joint_params(joint.kind).iter().zip(&st.joint_params[i]) {
                row.f(&format!("{n}.{name}"), value);
            }
            row.i(&format!("{n}.tag"), rd(joint.ode, J_TAG));
            let feedback = if joint.feedback.is_null() { [f32::NAN; 16] } else { *joint.feedback };
            row.v(&format!("{n}.f1"), &feedback[0..3]);
            row.v(&format!("{n}.t1"), &feedback[4..7]);
            row.v(&format!("{n}.f2"), &feedback[8..11]);
            row.v(&format!("{n}.t2"), &feedback[12..15]);
        }
        // tyres: what each Tyre::step was given and what it left behind (single-wheel rig layout)
        let input_names = rig::input_fields();
        let output_names = rig::output_fields();
        for (i, wheel) in WHEELS.iter().enumerate() {
            let capture = &st.tyres[i];
            let input = capture.input.as_ref().expect("Tyre::step did not run for a wheel");
            for (name, word) in input_names.iter().zip(input.to_words()) {
                emit_rig_word(row, &format!("tyre.{wheel}.{name}"), name, word);
            }
            for (name, &word) in output_names.iter().zip(&capture.output) {
                emit_rig_word(row, &format!("tyre.{wheel}.{name}"), name, word);
            }
            for (k, &value) in capture.wheel_rotation_in.iter().enumerate() {
                row.f(&format!("tyre.{wheel}.in_localWheelRotation.M{}{}", k / 4 + 1, k % 4 + 1), value);
            }
            for (k, what) in ["hubMatrix", "pointVelocity", "hubAngularVelocity", "ray"].iter().enumerate() {
                row.i(&format!("tyre.{wheel}.asked.{what}"), capture.asked[k] as i32);
            }
            row.i(&format!("tyre.{wheel}.ambiguous"), capture.ambiguous as i32);
        }
        self.emit_components(row);
    }

    /// The main state values of the car's systems at the end of the step.
    unsafe fn emit_components(&self, row: &mut Row) {
        let st = state();
        let car = self.car;
        let engine = self.engine;
        row.f("car.speed", rd(car, CAR_VALUE_CACHE_SPEED));
        row.v("car.accG", &v3(car, CAR_ACC_G));
        row.d("car.fuel", rd(car, CAR_FUEL));
        row.f("car.mass", rd(car, CAR_MASS));
        row.f("car.lastFF", rd(car, CAR_LAST_FF));
        row.f("car.mzCurrent", rd(car, CAR_MZ_CURRENT));
        row.i("car.sleepingFrames", rd(car, CAR_SLEEPING_FRAMES));
        row.f("car.waterTemperature", rd(car, CAR_WATER + 0x18));
        row.i("car.isControlsLocked", rd::<u8>(car, CAR_IS_CONTROLS_LOCKED) as i32);
        row.i("car.blackFlagged", rd::<u8>(car, CAR_BLACK_FLAGGED) as i32);
        row.d("car.penaltyTime", rd(car, CAR_PENALTY_TIME));
        row.v("car.damageZoneLevel", &rd::<[f32; 5]>(car, CAR_DAMAGE_ZONE_LEVEL));
        row.f("engine.ambientTemperature", rd(engine, PE_AMBIENT_TEMPERATURE));
        row.f("engine.roadTemperature", rd(engine, PE_ROAD_TEMPERATURE));
        row.v("engine.wind", &v3(engine, PE_WIND));
        row.i("engine.stepCounter", rd(engine, PE_STEP_COUNTER));
        row.d("engine.physicsTime", rd(engine, PE_PHYSICS_TIME));
        row.f("track.dynamicGripLevel", rd(st.track, TRACK_DYNAMIC_GRIP_LEVEL));
        let core: *const u8 = rd(engine, PE_CORE);
        let world: *const u8 = rd(core, CORE_WORLD);
        row.i("world.bodies", rd(world, WORLD_BODY_COUNT));
        row.i("world.joints", rd(world, WORLD_JOINT_COUNT));
        row.i("world.contactPoints", rd(core, CORE_CONTACT_POINTS));
        row.v("world.gravity", &v3(world, WORLD_GRAVITY));

        let d = car.add(CAR_DRIVETRAIN);
        row.i("drivetrain.currentGear", rd(d, 0x584));
        row.d("drivetrain.engine.velocity", rd(d, 0x8));
        row.f("drivetrain.engineRPM", (rd::<f64>(d, 0x8) as f32) * 0.159_155_07 * 60.0);
        row.d("drivetrain.drive.velocity", rd(d, 0x20));
        row.d("drivetrain.outShaftL.velocity", rd(d, 0x38));
        row.d("drivetrain.outShaftR.velocity", rd(d, 0x50));
        row.d("drivetrain.rootVelocity", rd(d, 0xb0));
        row.i("drivetrain.clutchOpenState", rd::<u8>(d, 0xb8) as i32);
        row.d("drivetrain.ratio", rd(d, 0xc0));
        row.d("drivetrain.cutOff", rd(d, 0xd8));
        row.f("drivetrain.totalTorque", rd(d, 0x4d8));
        row.f("drivetrain.currentClutchTorque", rd(d, 0x538));
        row.f("drivetrain.locClutch", rd(d, 0x644));
        row.i("drivetrain.isGearGrinding", rd::<u8>(d, 0x0) as i32);
        let e = d.add(0xe0);
        row.d("engine.status.outTorque", rd(e, 0x130));
        row.d("engine.status.externalCoastTorque", rd(e, 0x138));
        row.f("engine.status.turboBoost", rd(e, 0x140));
        row.i("engine.status.isLimiterOn", rd::<u8>(e, 0x144) as i32);
        row.f("engine.fuelPressure", rd(e, 0x150));
        row.i("engine.limiterOn", rd(e, 0x1f4));
        row.f("engine.electronicOverride", rd(e, 0x1f8));
        row.d("engine.lifeLeft", rd(e, 0x320));
        row.f("engine.lastInput.gas", rd(e, 0x1dc));

        let b = car.add(CAR_BRAKE_SYSTEM);
        row.f("brakes.frontBias", rd(b, 0x0));
        row.f("brakes.brakePowerMultiplier", rd(b, 0x4));
        row.f("brakes.electronicOverride", rd(b, 0x8));
        row.f("brakes.brakePower", rd(b, 0x270));
        for (i, wheel) in WHEELS.iter().enumerate() {
            row.f(&format!("brakes.disc.{wheel}.t"), rd(b, 0x18 + i * 0x90));
        }

        for (i, wheel) in WHEELS.iter().enumerate() {
            let s = st.suspensions[i];
            row.f(&format!("suspension.{wheel}.travel"), rd(s, SUS_STATUS));
            row.f(&format!("suspension.{wheel}.damperSpeedMS"), rd(s, SUS_STATUS + 4));
            row.f(&format!("suspension.{wheel}.steerTorque"), rd(s, SUS_STEER_TORQUE));
            row.f(&format!("suspension.{wheel}.steerAngle"), rd(s, SUS_STEER_ANGLE));
            row.f(&format!("suspension.{wheel}.bumpStopDn"), rd(s, SUS_BUMP_STOP_DN));
        }
        for (i, axle) in ["front", "rear"].iter().enumerate() {
            row.f(&format!("heave.{axle}.travel"), rd(car, CAR_HEAVE_SPRINGS + i * 0x58 + 0x8));
            row.f(&format!("arb.{axle}.k"), rd(car, CAR_ANTIROLL_BARS + i * 0x48 + 0x40));
        }

        let a = car.add(CAR_AERO_MAP);
        row.f("aero.airDensity", rd(a, 0x30));
        row.f("aero.dynamicCD", rd(a, 0x28));
        row.f("aero.dynamicCL", rd(a, 0x2c));
        let wings: *const u8 = rd(a, 0x38);
        let wings_end: *const u8 = rd(a, 0x40);
        let count = (wings_end as usize - wings as usize) / 0x310;
        for k in 0..count {
            let w = wings.add(k * 0x310 + 0x250);
            for (name, offset) in [
                ("aoa", 0x0),
                ("cd", 0x4),
                ("cl", 0x8),
                ("angle", 0xc),
                ("groundHeight", 0x14),
                ("dragKG", 0x1c),
                ("liftKG", 0x20),
                ("yawAngle", 0x30),
            ] {
                row.f(&format!("wing{k}.{name}"), rd(w, offset));
            }
        }

        let tc = car.add(CAR_TRACTION_CONTROL);
        row.i("tc.isActive", rd::<u8>(tc, 0x1) as i32);
        row.i("tc.isInAction", rd::<u8>(tc, 0x8) as i32);
        row.f("tc.slipRatioLimit", rd(tc, 0x4));
        let abs = car.add(CAR_ABS);
        row.i("abs.isPresent", rd::<u8>(abs, 0x0) as i32);
        row.i("abs.isActive", rd::<u8>(abs, 0x1) as i32);
        row.f("abs.currentValue", rd(abs, 0xa4));
        row.f("edl.outLevel", rd(car, CAR_EDL + 0x1c));
        row.f("stability.gain", rd(car, CAR_STABILITY_CONTROL));
        row.i("speedLimiter.isLimiting", rd::<u8>(car, CAR_SPEED_LIMITER + 1) as i32);
        row.i("autoShift.isActive", rd::<u8>(car, CAR_AUTO_SHIFT) as i32);
        row.i("autoBlip.isActive", rd::<u8>(car, CAR_AUTO_BLIP) as i32);
        row.i("autoClutch.useAutoOnStart", rd::<u8>(car, CAR_AUTOCLUTCH + 0xc) as i32);
        row.i("autoClutch.useAutoOnChange", rd::<u8>(car, CAR_AUTOCLUTCH + 0xd) as i32);
        row.f("autoClutch.clutchValueSignal", rd(car, CAR_AUTOCLUTCH + 0x1a0));
        row.i("drs.isActive", rd::<u8>(car, CAR_DRS + 1) as i32);
        row.f("car.ballastKG", rd(car, CAR_BALLAST_KG));
        row.f("car.steerLock", rd(car, CAR_STEER_LOCK));
        row.f("car.steerRatio", rd(car, CAR_STEER_RATIO));
        // the shared-memory physics page the game's own writer produced for this step
        let mut at = 0;
        for (name, kind, count) in crate::record::PAGE_FIELDS {
            for i in 0..count {
                let field = if count == 1 { format!("page.{name}") } else { format!("page.{name}.{i}") };
                match kind {
                    'i' => row.i(&field, rd(self.page, at)),
                    _ => row.f(&field, rd(self.page, at)),
                }
                at += 4;
            }
        }
        assert_eq!(at, crate::record::PAGE_SIZE);
    }

    /// Facts about the car that do not change during a run, for the recording's header.
    pub fn facts(&self) -> Vec<(String, String)> {
        let st = state();
        let mut out = Vec::new();
        unsafe {
            let tyre = self.car.add(CAR_TYRES);
            out.push(("compound".to_string(), rd::<i32>(tyre, T_CURRENT_COMPOUND_INDEX).to_string()));
            out.push((
                "ray_casters".to_string(),
                (0..4).filter(|i| rd::<usize>(tyre.add(i * TYRE_SIZE), T_RAY_CASTER) != 0).count().to_string(),
            ));
        }
        out.push(("bodies".to_string(), self.bodies().join(",")));
        let joints: Vec<String> = st
            .joints
            .iter()
            .map(|j| {
                let body = |i: usize| st.bodies.get(i).map(|b| b.name.as_str()).unwrap_or("world");
                format!("{}:{}:{}:{}", j.name, j.kind, body(j.body0), body(j.body1))
            })
            .collect();
        out.push(("joints".to_string(), joints.join(",")));
        out
    }
}

fn emit_rig_word(row: &mut Row, name: &str, rig_name: &str, word: u64) {
    match rig::field_kind(rig_name) {
        'd' => row.d(name, f64::from_bits(word)),
        'i' => row.i(name, word as i32),
        _ => row.f(name, f32::from_bits(word as u32)),
    }
}
