// SPDX-License-Identifier: GPL-3.0-or-later

//! The game side: AC's own `PhysicsEngine`, `Track` and `Car` built in this process, the
//! fakes they talk to (ground, controls device) and the hooks that record what they do.
//!
//! Addresses are Ghidra addresses of acs.exe 1.16.4; member offsets are from acs.pdb
//! (`re/types/*.txt`), ODE offsets from the disassembly of ODE's own accessors.

pub mod sus_micro;

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
const VA_TRACK_ADD_SURFACE: usize = 0x1_4027_7e50; // Track::addSurface(const wstring&, float* vertices, int, u16* indices, int, const SurfaceDef&, uint subSpace)
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
const VA_PHYSICS_ENGINE_SET_WIND: usize = 0x1_4026_45a0; // PhysicsEngine::setWind(Speed, float)
// Task 15: the session's conditions and a saved setup
const VA_KS_RAND_RANGE: usize = 0x1_4003_3770; // float ksRand(float min, float max)
const VA_SPEED_FROM_KMH: usize = 0x1_4023_9970; // static Speed Speed::fromKMH(float)
const VA_GENERATE_WIND_JOB: usize = 0x1_4013_3ac0; // the job RaceManager::generateWind queues
const VA_CAR_SET_BALLAST_KG: usize = 0x1_4027_59e0; // Car::setBallastKG(float)
const VA_CAR_SET_RESTRICTOR: usize = 0x1_4027_5d10; // Car::setRestrictor(float)
const VA_CAR_SET_REQUESTED_FUEL: usize = 0x1_4027_5cb0; // Car::setRequestedFuel(float, bool)
const VA_SETUP_MANAGER_LOAD: usize = 0x1_4028_cc90; // SetupManager::load(const wstring& path)
const VA_TC_CYCLE_MODE: usize = 0x1_4028_f8e0; // TractionControl::cycleMode(int dir)
const VA_TC_GET_CURRENT_MODE: usize = 0x1_4028_f9b0; // pair<uint, uint> TractionControl::getCurrentMode()
// RaceManager: windSettings (baseSpeed km/h, baseDirection deg) and the way to the engine
const RM_WIND_BASE_SPEED: usize = 0xb8;
const RM_WIND_BASE_DIRECTION: usize = 0xbc;
const RM_SIM: usize = 0x168;
const SIM_PHYSICS_AVATAR: usize = 0x1b8;
const PHYSICS_AVATAR_ENGINE: usize = 0x58;
const VA_CAR_LOCK_CONTROLS: usize = 0x1_4027_45f0; // Car::lockControls(bool)
const VA_CAR_LOCK_CONTROLS_UNTIL: usize = 0x1_4027_4600; // Car::lockControlsUntil(double, double)
const VA_CAR_ADD_PENALTY: usize = 0x1_4026_f6a0; // Car::addPenalty(double)
const CAR_IS_GENTLE_STOPPING: usize = 0x3a81;
const PE_PENALTY_MODE: usize = 0xd4;
const PE_ALLOWED_TYRES_OUT: usize = 0xd0;
const VA_TRACK_ADD_TIME_LINE: usize = 0x1_4027_8040; // Track::addTimeLine(const vec3f& p0, const vec3f& p1, int type)
const CAR_TRANSPONDER: usize = 0x1f8;
const CAR_SPLINE_LOCATOR_DATA: usize = 0x3ad0;
const CAR_SPLINE_LOCATOR: usize = 0x3e38;
const VA_CAR_RESET_SUSPENSION_DAMAGE: usize = 0x1_4027_5970; // Car::resetSuspensionDamageLevel()
const VA_TYRE_SET_COMPOUND: usize = 0x1_4028_34e0; // bool Tyre::setCompound(int index)
const VA_SETUP_MANAGER_STEP: usize = 0x1_4028_d090; // SetupManager::step(float dt)
const VA_BRAKE_SYSTEM_SET_MANUAL_FRONT_BIAS: usize = 0x1_4028_e5d0; // BrakeSystem::setManualFrontBias(int)
// --- globals ---------------------------------------------------------------------------------
const VA_INIREADERDOCUMENTS_INITIALIZED: usize = 0x1_4155_a588; // static bool INIReaderDocuments::initialized
const VA_IS_USING_QPT: usize = 0x1_4151_d140; // bool isUsingQPT (ksTimer)
const VA_TIMER_START: usize = 0x1_4155_a590; // LARGE_INTEGER startTime (ksTimer)
const VA_TIMER_FREQUENCY: usize = 0x1_4155_a598; // LARGE_INTEGER frequency (ksTimer)
const VA_IS_TEST_MODE: usize = 0x1_4155_a770; // static bool PhysicsEngine::isTestMode
// --- vtables ---------------------------------------------------------------------------------
const VA_KEYBOARD_CONTROL_VTABLE: usize = 0x1_404c_5d50; // KeyboardCarControl (a real ICarControlsProvider)
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
const PE_TYRE_CONSUMPTION_RATE: usize = 0xcc;
const PE_AMBIENT_TEMPERATURE: usize = 0x100;
const PE_ROAD_TEMPERATURE: usize = 0x104;
const PE_MECHANICAL_DAMAGE_RATE: usize = 0x108;
const PE_WIND: usize = 0x158;
const PE_STEP_COUNTER: usize = 0x1a8;
const TRACK_SIZE: usize = 0x148;
const TRACK_DYNAMIC_GRIP_LEVEL: usize = 0x128;
// DynamicTrackData: isExternal +0, enabled +1
const TRACK_DYNAMIC_TRACK: usize = 0xb0;
const SURFACE_SIZE: usize = 0xc8;
const SD_GRIP_MOD: usize = 0x90;
const SD_COLLISION_CATEGORY: usize = 0x9c;
const SD_IS_VALID_TRACK: usize = 0xa0;
const SD_IS_PITLANE: usize = 0xb0;

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
const CAR_SETUP_MANAGER: usize = 0x35d0;
// SetupItem (0x88 bytes): one adjustable value of the car
const SETUP_ITEM_SIZE: usize = 0x88;
const SI_NAME: usize = 0x8;
const SI_CONNECTED_FLOAT: usize = 0x28;
const SI_MULTIPLIER: usize = 0x50;
const SI_NEW_VALUE: usize = 0x54;
const SI_ATTACHED: usize = 0x58;
const SI_LABEL_MULTIPLIER: usize = 0x80;
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
const CC_DRS: usize = 0x2;
const CC_KERS: usize = 0x3;
/// `Car::kers` (0xf8 bytes) and `Car::ers` (0x2b0 bytes)
const CAR_KERS: usize = 0x3670;
const CAR_ERS: usize = 0x3768;
/// `ERS::setPowerController(int)`, `Engine::setCoastSettings(int)`
const VA_ERS_SET_POWER_CONTROLLER: usize = 0x140292fc0;
const VA_ENGINE_SET_COAST_SETTINGS: usize = 0x140288010;
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

// ISuspension (the members every suspension class shares)
const SUS_BUMP_STOP_UP: usize = 0x1c;
const SUS_BUMP_STOP_DN: usize = 0x20;
/// `Car::rigidAxle`: the one body of a rigid rear axle (null without one).
const CAR_RIGID_AXLE: usize = 0x128;

/// Where one of the game's four suspension classes keeps what the recorder reads.
struct SusClass {
    name: &'static str,
    vtable: usize,
    /// `hub` (the axle class has none: its body is `Car::rigidAxle`)
    hub: Option<usize>,
    /// `strutBody`
    strut_body: Option<usize>,
    /// `status` (travel, damperSpeedMS)
    status: usize,
    steer_torque: Option<usize>,
    steer_angle: Option<usize>,
    joints: SusJoints,
}

enum SusJoints {
    /// `IJoint* joints[5]` at this offset, with these names in creation order.
    Array(usize, [&'static str; 5]),
    /// `std::vector` of records at this offset: record size, offset of the `IJoint*` in a
    /// record, name prefix. (For the axle only the Left instance creates the links.)
    Vector(usize, usize, usize, &'static str),
}

/// `Suspension` (double wishbone), `SuspensionStrut`, `SuspensionAxle`, `SuspensionML`.
const SUS_CLASSES: [SusClass; 4] = [
    SusClass {
        name: "DWB",
        vtable: 0x1_404f_f870,
        hub: Some(0x40),
        strut_body: None,
        status: 0x1c0,
        steer_torque: Some(0x1dc),
        steer_angle: Some(0x1ec),
        // each rod runs from a point on the car body (anchor 1) to a point on the hub (anchor 2)
        joints: SusJoints::Array(0x68, ["top_rear", "top_front", "bottom_rear", "bottom_front", "steer_rod"]),
    },
    SusClass {
        name: "STRUT",
        vtable: 0x1_404f_fc80,
        hub: Some(0x40),
        strut_body: Some(0x1b8),
        status: 0x180,
        steer_torque: Some(0x19c),
        steer_angle: Some(0x1ac),
        joints: SusJoints::Array(0x58, ["bottom_rear", "bottom_front", "steer_rod", "slider", "ball"]),
    },
    SusClass {
        name: "AXLE",
        vtable: 0x1_404f_fe90,
        hub: None,
        strut_body: None,
        status: 0x68,
        steer_torque: None,
        steer_angle: None,
        joints: SusJoints::Vector(0x88, 0x40, 0x38, "link"),
    },
    SusClass {
        name: "ML",
        vtable: 0x1_4050_01a0,
        hub: Some(0x38),
        strut_body: None,
        status: 0x40,
        steer_torque: Some(0x98),
        steer_angle: None,
        joints: SusJoints::Vector(0x70, 0x38, 0x30, "joint"),
    },
];

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
const JOINT_BALL: u32 = 1;
const JOINT_SLIDER: u32 = 3;
/// `dxJointBall`: the two anchors, erp, cfm.
const BALL_PARAMS: [(&str, usize); 8] = [
    ("anchor1.x", 0x00),
    ("anchor1.y", 0x04),
    ("anchor1.z", 0x08),
    ("anchor2.x", 0x10),
    ("anchor2.y", 0x14),
    ("anchor2.z", 0x18),
    ("erp", 0x20),
    ("cfm", 0x24),
];
/// `dxJointSlider`: axis1, qrel, offset, then the limit-motor block at +0x30 (its
/// `normal_cfm` is what `SliderJointODE::setERPCFM` writes).
const SLIDER_PARAMS: [(&str, usize); 11] = [
    ("axis1.x", 0x00),
    ("axis1.y", 0x04),
    ("axis1.z", 0x08),
    ("qrel.w", 0x10),
    ("qrel.x", 0x14),
    ("qrel.y", 0x18),
    ("qrel.z", 0x1c),
    ("offset.x", 0x20),
    ("offset.y", 0x24),
    ("offset.z", 0x28),
    ("cfm", 0x30 + 0x14),
];
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
fn joint_params(kind: u32) -> &'static [(&'static str, usize)] {
    match kind {
        JOINT_DBALL => &DBALL_PARAMS,
        JOINT_FIXED => &FIXED_PARAMS,
        JOINT_BALL => &BALL_PARAMS,
        JOINT_SLIDER => &SLIDER_PARAMS,
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
    /// Index into `SUS_CLASSES` of each wheel's suspension.
    suspension_classes: [usize; 4],
    suspension_vtables: [*const usize; 4],
    tyre_step_original: usize,
    world_step_original: usize,
    /// What the fake controls device reports this step.
    controls: Controls,
    /// How many times the game polled the device this step.
    polled: u32,
    /// A whole-car scenario: the device also reports the DRS button.
    whole: bool,
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
    joint_params: Vec<Vec<f32>>,
    /// `Car::controls` when `Car::step` had finished (read as `dWorldStep` starts).
    applied_controls: [u8; 0x34],
    stage0_original: usize,
    world_steps: u32,
    /// How often ODE started on an island (one island holds the whole car).
    islands: u32,
    /// `islands` when the current `dWorldStep` began.
    islands_before_step: u32,
    image_base: usize,
    /// The real track of a track scenario, and what each tyre's ray found in this step.
    game_track: Option<&'static crate::track::GameTrack>,
    ray_hits: [RayRecord; 4],
}

/// What a tyre's own ray caster answered in a step.
#[derive(Clone, Copy, Default)]
struct RayRecord {
    hit: bool,
    pos: V3,
    normal: V3,
    /// Index of the mesh that was hit, -1 for none.
    mesh: i32,
}

/// The game's `RayCastHit` (0x28 bytes).
#[repr(C)]
struct RayCastHit {
    pos: V3,
    normal: V3,
    collision_object: *mut u8,
    has_contact: u8,
}

/// `Track::createRayCaster` of the game, kept when the track's vtable entry is replaced.
static mut CREATE_RAY_CASTER_ORIGINAL: usize = 0;

/// `IRayTrackCollisionProvider::createRayCaster` of a real track: the game's own ray caster
/// behind a wrapper that notes what every ray finds.
extern "C" fn wrapped_create_ray_caster(this: *mut u8, length: f32) -> *mut u8 {
    unsafe {
        let original: extern "C" fn(*mut u8, f32) -> *mut u8 = std::mem::transmute(CREATE_RAY_CASTER_ORIGINAL);
        let real = original(this, length);
        // IRayCaster: +0x00 destructor, +0x08 rayCast, +0x10 release
        let table: Vec<usize> =
            vec![device_destructor as *const () as usize, wrapped_ray_cast as *const () as usize, wrapped_release as *const () as usize];
        let wrapper = object(0x18);
        wr(wrapper, 0, leak(table));
        wr(wrapper, 8, real);
        wrapper
    }
}

extern "C" fn wrapped_release(_this: *mut u8) {}

/// `IRayCaster::rayCast`: the game's `RayCaster::rayCast` (its ODE ray against the track's
/// meshes), with the answer noted for the tyre that asked.
extern "C" fn wrapped_ray_cast(this: *mut u8, out: *mut RayCastHit, org: *const V3, dir: *const V3) -> *mut RayCastHit {
    unsafe {
        let real: *mut u8 = rd(this, 8);
        let table: *const usize = rd(real, 0);
        let ray_cast: extern "C" fn(*mut u8, *mut RayCastHit, *const V3, *const V3) -> *mut RayCastHit = std::mem::transmute(*table.add(1));
        let result = ray_cast(real, out, org, dir);
        let st = state();
        if let (Some(wheel), Some(track)) = (st.tyre, st.game_track) {
            let hit = (*out).has_contact != 0;
            let mut record = RayRecord { hit, mesh: -1, ..RayRecord::default() };
            let capture = &mut st.tyres[wheel];
            capture.asked[3] += 1;
            let input = capture.input.as_mut().unwrap();
            input.has_hit = hit;
            if hit {
                record.pos = (*out).pos;
                record.normal = (*out).normal;
                record.mesh = track.index_of.get(&((*out).collision_object as usize)).map(|i| *i as i32).unwrap_or(-2);
                input.ground_y = (*out).pos[1];
                input.ground_normal = (*out).normal;
                // the surface the tyre will read through the hit object's user pointer
                let surface = crate::track::read_surface_def(track.surface_of((*out).collision_object));
                input.grip_mod = surface.grip_mod;
                input.dirt_additive_k = surface.dirt_additive_k;
                input.sin_height = surface.sin_height;
                input.sin_length = surface.sin_length;
                input.damping = surface.damping;
                input.granularity = surface.granularity;
            }
            st.ray_hits[wheel] = record;
        }
        result
    }
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
// setRotation takes a matrix; its first row is what gets recorded as the vector
body_force_hook!(body_set_rotation, body_set_rotation_h, 0x98, 12, one);

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

fn note<const N: usize>(capture: &mut TyreCapture, which: usize, slot: &mut [f32; N], value: [f32; N]) {
    // bits, not values: +0.0 and -0.0 are different answers
    if capture.asked[which] > 0 && slot.map(f32::to_bits) != value.map(f32::to_bits) {
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
        if st.whole {
            wr(controls, CC_DRS, c.drs as u8);
            wr(controls, CC_KERS, c.kers as u8);
        }
        wr(controls, CC_HAND_BRAKE, c.hand_brake);
        // -1 (no H-shifter) is also what the constructor left there
        wr(controls, CC_REQUESTED_GEAR_INDEX, c.requested_gear);
        wr(controls, CC_GAS, c.gas);
        wr(controls, CC_BRAKE, c.brake);
        wr(controls, CC_STEER, c.steer);
        wr(controls, CC_CLUTCH, c.clutch);
    }
}

/// `getAction(action)`: only the headlight switch (4) of a whole-car script is ever pressed.
extern "C" fn device_get_action(_this: *mut u8, action: i32) -> u8 {
    let st = state();
    (st.whole && action == 4 && st.controls.headlights) as u8
}
extern "C" fn device_send_ff(_this: *mut u8, _force: f32, _damper: f32, _gain: f32) {}
extern "C" fn device_get_ff_global_gain(_this: *mut u8) -> f32 {
    1.0
}
extern "C" fn device_get_name(_this: *mut u8) -> *const u8 {
    c"car_oracle script".as_ptr().cast()
}
extern "C" fn device_on_auto_shifter_changed(_this: *mut u8, _on: u8) {}
extern "C" fn device_false(_this: *mut u8) -> u8 {
    0
}
extern "C" fn device_set_vibrations(_this: *mut u8, _def: *const u8) {}
extern "C" fn device_set_engine_rpm(_this: *mut u8, _rpm: f32, _limiter: f32) {}

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
        // what the tyre itself would get from car->body->getVelocity() (+0x78) and getMass() (+0x28)
        let get_velocity: extern "C" fn(*mut u8, *mut V3) -> *mut V3 =
            std::mem::transmute(*st.body_vtable.add(0x78 / 8));
        let get_mass: extern "C" fn(*mut u8) -> f32 = std::mem::transmute(*st.body_vtable.add(0x28 / 8));
        let mut body_velocity = [0f32; 3];
        get_velocity(body.wrapper, &mut body_velocity);
        let body_mass = get_mass(body.wrapper);
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
            body_velocity,
            body_mass,
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
        st.applied_controls = rd(st.car, CAR_CONTROLS);
        st.joint_params = st
            .joints
            .iter()
            .map(|j| {
                joint_params(j.kind).iter().map(|(_, offset)| rd::<f32>(j.ode, J_PARAMS + offset)).collect()
            })
            .collect();
        for joint in &st.joints {
            if !joint.feedback.is_null() {
                *joint.feedback = [0.0; 16];
            }
        }
    }
    state().islands_before_step = state().islands;
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
    // the island of the car is the first one of a step (its bodies are at the head of the
    // list of the world; a loose object that flies on its own is an island after it)
    if st.islands == st.islands_before_step {
        st.solver = st.bodies.iter().map(|body| unsafe { (v3(body.ode, B_FACC), v3(body.ode, B_TACC)) }).collect();
    }
    st.islands += 1;
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
pub const DEFAULT_CAR: &str = "ks_ferrari_f2004";
/// Fixed weather: the same values the game's `PhysicsEngine` constructor starts with.
/// The physics clock at the first step, ms. The game's clock counts from program start, so a
/// session never begins at 0; members that start at time 0 (the auto-blip's "last blip") must
/// lie in the past here too.
pub const CLOCK_START_MS: f64 = 60_000.0;
pub const AMBIENT_TEMPERATURE: f32 = 26.0;
pub const ROAD_TEMPERATURE: f32 = 30.0;

pub struct Options {
    /// Ask ODE for the force each joint applies (`dJointSetFeedback`). The game never does;
    /// see the report for what it changes.
    pub joint_feedback: bool,
    /// Folder name of the car under `cardata/`.
    pub car: String,
    /// Only build the car, apply the session-start setup and let the game's own
    /// `SetupManager::step` report what changes (for comparing with a real game log).
    pub setup_check: bool,
    /// A track folder: the game's own track with its collision meshes instead of the flat road.
    pub track: Option<std::path::PathBuf>,
    /// The track's layout ("" for none).
    pub layout: String,
    /// Task 13: the body touches things. The track's meshes keep their real collision
    /// categories, the car gets its own collider mesh (`colliders`), and the contact joints
    /// are recorded.
    pub collide: bool,
    /// The car's colliders as the port reads them (the mesh of `collider.kn5`; the game's own
    /// loader needs Direct3D).
    pub colliders: Option<rustyac_physics::car::colliders::CarColliders>,
    /// The file the collider mesh came from (for the recording's header).
    pub collider_kn5: Option<std::path::PathBuf>,
    /// Task 15: a saved setup (`--setup <file>`), for the scenarios that load one.
    pub setup: Option<std::path::PathBuf>,
}

/// One car on the fake track, ready to be stepped.
pub struct World<'a> {
    acs: &'a Acs,
    pub engine: *mut u8,
    pub car: *mut u8,
    driver: Driver,
    pub step_index: usize,
    /// A `SharedMemoryWriter` laid out by hand, its fake `CarAvatar`, and the page it fills.
    telemetry_writer: *mut u8,
    avatar: *mut u8,
    page: *mut u8,
    /// `name:old->new` of every setup value the session start changes (applied in step 0).
    pub setup_changes: Vec<String>,
    /// The scenario is one of the powertrain scenarios: more inputs and values are recorded.
    powertrain: bool,
    /// The scenario is one of the whole-car scenarios: the DRS button and more values of the
    /// wings and the aids are recorded.
    whole: bool,
    /// Header lines of a track scenario (the track, the spawn).
    pub track_meta: Vec<(String, String)>,
    /// Task 15: header keys of the session's conditions (none for the older scenarios).
    pub conditions_meta: Vec<(String, String)>,
    /// Task 13: the contact joints are recorded; the game's geoms by address (a track mesh is
    /// its index, floor box k of the car 1000 + k, the car's collider mesh 2000).
    collide: bool,
    geom_names: std::collections::HashMap<usize, i32>,
    /// The track's loose objects (the game's `PhysicsObject`s), in creation order.
    game_objects: Vec<crate::track::GameObject>,
}

/// Builds the small game folder the engine, track and car read their files from.
pub fn prepare_root(repo: &Path, root: &Path, car: &str) -> Result<u64, String> {
    let io = |e: std::io::Error| e.to_string();
    std::fs::create_dir_all(root.join("system/cfg")).map_err(io)?;
    // THREADS=0: no thread pool, the whole step runs on the calling thread. The other keys
    // only feed the force-feedback numbers and have the values of the game's own file.
    std::fs::write(
        root.join("system/cfg/assetto_corsa.ini"),
        "[PHYSICS_THREADING]\nTHREADS=0\n[FF_EXPERIMENTAL]\nENABLE_GYRO=0\nDAMPER_MIN_LEVEL=0\nDAMPER_GAIN=1\n\
         [LOW_SPEED_FF]\nSPEED_KMH=3\nMIN_VALUE=0.01\n",
    )
    .map_err(io)?;
    // the track "flat" has no files at all: no AI line, no surfaces, no DRS zones
    let _ = std::fs::remove_dir_all(root.join("content/tracks"));
    let _ = std::fs::remove_file(root.join("system/cfg/tyre_smoke.ini"));
    let from = repo.join("cardata").join(car);
    let to = root.join("content/cars").join(car).join("data");
    std::fs::create_dir_all(&to).map_err(io)?;
    let mut names = Vec::new();
    for entry in std::fs::read_dir(&from).map_err(|e| format!("{}: {e}", from.display()))? {
        let entry = entry.map_err(io)?;
        if entry.file_type().map_err(io)?.is_file() {
            names.push(entry.file_name());
        }
    }
    // a hash of the car's data (names and contents, in name order) for the recording's header
    names.sort();
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for name in names {
        let bytes = std::fs::read(from.join(&name)).map_err(io)?;
        crate::record::fnv1a(&mut hash, name.to_string_lossy().as_bytes());
        crate::record::fnv1a(&mut hash, &bytes);
        std::fs::write(to.join(&name), &bytes).map_err(io)?;
    }
    Ok(hash)
}

/// What the game's setup screen does to the car when a session starts with the default setup.
///
/// Every `SetupManager` item that has a section in the car's `setup.ini` is shown as a
/// spinner holding an integer; on entry the screen sets every spinner to its default and
/// writes the spinner's value back to the item. A value that is not on the spinner's grid
/// (e.g. a camber of 3.0 degrees stored as radians and shown in tenths of a degree) comes
/// back changed. Mirrors `SetupScreen::loadINI` 0x14017d950, `SetupTab::addItem` 0x140183850
/// and the spinner job 0x140183620; the game's own `SetupManager::step` then applies the
/// values in the first step. Returns `name:old->new` for every value that will change.
unsafe fn apply_setup_screen_defaults(car: *mut u8, setup: &rustyac_physics::data::ini::IniReader) -> Vec<String> {
    let manager = car.add(CAR_SETUP_MANAGER);
    let mut item: *mut u8 = rd(manager, 0);
    let end: *mut u8 = rd(manager, 8);
    let clicks = setup.has_section("DISPLAY_METHOD");
    let mut changes = Vec::new();
    while item < end {
        let name = read_wstring(item.add(SI_NAME));
        if setup.has_section(&name) {
            // a missing key reads as 0, as in the game's INIReader
            let float = |key: &str| setup.get_float(&name, key).unwrap_or(0.0);
            let label = rd::<f32>(item, SI_LABEL_MULTIPLIER).abs();
            let min = (float("MIN") / label) as f64;
            let max = (float("MAX") / label) as f64;
            let step = setup.get_int(&name, "STEP").unwrap_or(0) as f64;
            let mode = if clicks { setup.get_int(&name, "SHOW_CLICKS").unwrap_or(0) } else { 0 };
            let value: f32 = rd(item, SI_NEW_VALUE);
            // the spinner's range and position (C casts: toward zero)
            let (low, high, position) = match mode {
                1 => ((min / step) as i32, (max / step) as i32, (value as f64 / step + 0.5) as i32),
                2 => (0, ((max - min) / step) as i32, ((value - min as f32) / step as f32 + 0.5f32) as i32),
                _ => (min as i32, max as i32, value as i32),
            };
            let position = if position > high { high } else { position.max(low) };
            let back: f32 = match mode {
                1 => step as f32 * position as f32,
                2 => step as f32 * position as f32 + min as f32,
                _ => position as f32,
            };
            wr(item, SI_NEW_VALUE, back);
            wr(item, SI_ATTACHED, 1u8);
            let connected: *const f32 = rd(item, SI_CONNECTED_FLOAT);
            let applied = rd::<f32>(item, SI_MULTIPLIER) * back;
            if !connected.is_null() && applied.to_bits() != (*connected).to_bits() && applied != *connected {
                changes.push(format!("{name}:{:?}->{:?}", *connected, applied));
            }
        }
        item = item.add(SETUP_ITEM_SIZE);
    }
    changes
}

/// The game's `PhysicsEngine`, built the way every scenario builds it. The current directory
/// must be the folder made by [`prepare_root`].
pub fn new_engine(acs: &Acs, seed: u32) -> *mut u8 {
    unsafe {
        // the game's rand() and the oracle's clock start from the same point every run
        let srand: extern "C" fn(u32) = std::mem::transmute(acs.crt_function(c"srand"));
        srand(seed);
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
        wr(engine, PE_PHYSICS_TIME, CLOCK_START_MS);
        wr(engine, PE_GAME_TIME, CLOCK_START_MS);
        wr(engine, PE_AMBIENT_TEMPERATURE, AMBIENT_TEMPERATURE);
        wr(engine, PE_ROAD_TEMPERATURE, ROAD_TEMPERATURE);
        engine
    }
}

impl<'a> World<'a> {
    /// The current directory must be the folder made by [`prepare_root`].
    pub fn build(acs: &'a Acs, scenario: &Scenario, options: &Options) -> World<'a> {
        unsafe {
            let engine = new_engine(acs, scenario.seed);
            // Task 15: the session's conditions. The temperatures are two plain stores in the
            // game (RaceManager::initOffline, before the car exists).
            let conditions = scenario.whole.conditions;
            let hex = |values: &[f32]| values.iter().map(|x| format!("{:08x}", x.to_bits())).collect::<Vec<_>>().join(",");
            let mut conditions_meta: Vec<(String, String)> = Vec::new();
            if let Some((air, road)) = conditions.temperature {
                wr(engine, PE_AMBIENT_TEMPERATURE, air);
                wr(engine, PE_ROAD_TEMPERATURE, road);
            }
            // [DYNAMIC_TRACK]: read by the game's own Track::initDynamicTrack (called from the
            // Track constructor) out of cfg/race.ini in the working directory
            let _ = std::fs::remove_file("cfg/race.ini");
            if let Some([start, randomness, gain, transfer]) = conditions.dynamic_track {
                std::fs::create_dir_all("cfg").expect("the cfg folder of the scratch root");
                let text = format!("[DYNAMIC_TRACK]\nSESSION_START={start}\nSESSION_TRANSFER={transfer}\nRANDOMNESS={randomness}\nLAP_GAIN={gain}\n");
                std::fs::write("cfg/race.ini", text).expect("cfg/race.ini of the scratch root");
                conditions_meta.push(("dynamic_track".to_string(), hex(&[start, randomness, gain, transfer])));
            }
            // [WIND], the part in RaceManager::initOffline (0x14013bd75..): the two limits kept
            // within 0..40 km/h, the game's own ksRand between them and Speed::fromKMH; the
            // rest of that function needs the whole game, so its few lines are done here
            let wind_settings = conditions.wind.map(|[min, max, direction]| {
                let clamp = |x: f32| if x > 40.0 { 40.0 } else if 0.0 > x { 0.0 } else { x };
                let ks_rand: extern "C" fn(f32, f32) -> f32 = std::mem::transmute(acs.va(VA_KS_RAND_RANGE));
                let from_kmh: extern "C" fn(*mut f32, f32) -> *mut f32 = std::mem::transmute(acs.va(VA_SPEED_FROM_KMH));
                let kmh = ks_rand(clamp(min), clamp(max));
                let mut speed = 0.0f32;
                from_kmh(&mut speed, kmh);
                conditions_meta.push(("wind_ini".to_string(), hex(&[min, max, direction])));
                let mut direction = direction;
                if direction < 0.0 {
                    direction = ks_rand(0.0, 360.0);
                }
                // Speed::kmh @ 0x140058f50
                (speed * f32::from_bits(0x4066_6666), direction)
            });

            let mut game_track: Option<&'static crate::track::GameTrack> = None;
            let mut floor_object: *mut u8 = std::ptr::null_mut();
            let mut spawn: (V3, V3) = ([0.0, 0.0, 0.0], [0.0, 0.0, -1.0]);
            let mut armed = false;
            let mut track_meta: Vec<(String, String)> = Vec::new();
            let mut game_objects: Vec<crate::track::GameObject> = Vec::new();
            let mut driver = scenario.driver();
            driver.follower.grip = crate::track_driver::Grip::of(&options.car);
            let (track, surface) = if let Some(folder) = &options.track {
                let kind = scenario.track_kind().expect("--track needs one of the track scenarios (spa_...)");
                // the game's own Track, surfaces, collision meshes and AI line; every mesh a
                // ghost to the car's body
                let mut built = crate::track::GameTrack::build(acs, engine, folder, &options.layout, !options.collide).expect("the track");
                assert!(built.surface_mismatches.is_empty(), "the game's surfaces differ from the port's: {:?}", built.surface_mismatches);
                // TrackAvatar::initTimeLines: the gates between the nodes AC_TIME_n_L / _R, from
                // the nodes' own matrices
                let place = |name: &str| -> Option<V3> {
                    let m = built.rust.helper_nodes.iter().find(|h| h.name == name)?.local.m;
                    Some([m[3][0], m[3][1], m[3][2]])
                };
                let add_time_line: extern "C" fn(*mut u8, *const V3, *const V3, i32) = std::mem::transmute(acs.va(VA_TRACK_ADD_TIME_LINE));
                let mut lines = 0;
                while let (Some(left), Some(right)) = (place(&format!("AC_TIME_{lines}_L")), place(&format!("AC_TIME_{lines}_R"))) {
                    add_time_line(built.track, &left, &right, 0);
                    lines += 1;
                }
                // a point-to-point track: the start and the finish gate (types 1 and 2), and
                // the finish line of a drag strip (an ordinary line again)
                let ab = ["AC_AB_START_L", "AC_AB_START_R", "AC_AB_FINISH_L", "AC_AB_FINISH_R"].map(place);
                if let [Some(sl), Some(sr), Some(fl), Some(fr)] = ab {
                    add_time_line(built.track, &sl, &sr, 1);
                    add_time_line(built.track, &fl, &fr, 2);
                    lines += 2;
                }
                if let (Some(left), Some(right)) = (place("AC_OPEN_FINISH_L"), place("AC_OPEN_FINISH_R")) {
                    add_time_line(built.track, &left, &right, 0);
                    lines += 1;
                }
                assert_eq!(lines, built.rust.time_lines.len(), "the port found other timing lines");
                // a session that is not a race, with penalties on: leaving the track with more
                // than two tyres costs the lap (RaceManager::setCurrentSession / initOffline)
                wr(engine, PE_PENALTY_MODE, 1i32);
                wr(engine, PE_ALLOWED_TYRES_OUT, 2i32);
                let (position, tail, is_armed, note) = crate::track_driver::spawn(kind, &mut built.rust).expect("the scenario's spawn point");
                spawn = ([position.x, position.y, position.z], [tail.x, tail.y, tail.z]);
                armed = is_armed;
                // every tyre's ray caster is the game's, behind a wrapper that notes the hits
                let original: *const usize = rd(built.track, 0);
                CREATE_RAY_CASTER_ORIGINAL = *original.add(3);
                let mut table: Vec<usize> = (0..5).map(|i| *original.offset(i as isize - 1)).collect();
                table[1 + 3] = wrapped_create_ray_caster as *const () as usize;
                wr(built.track, 0, leak(table).add(1));
                // the track's loose objects (cones, marker boards): the game makes them while
                // it loads the track, before any car. Only in a run with collisions.
                if options.collide {
                    game_objects = crate::track::create_objects(acs, engine, &built.rust);
                }
                let hex = |v: &V3| v.map(|x| format!("{:08x}", x.to_bits())).join(",");
                track_meta = vec![
                    ("track".to_string(), built.rust.name.clone()),
                    ("track_folder".to_string(), folder.display().to_string()),
                    ("track_layout".to_string(), options.layout.clone()),
                    ("track_objects".to_string(), (options.collide as u8).to_string()),
                    ("track_object_count".to_string(), game_objects.len().to_string()),
                    ("spawn".to_string(), note),
                    ("spawn_position".to_string(), hex(&spawn.0)),
                    ("spawn_tail".to_string(), hex(&spawn.1)),
                    ("armed".to_string(), (armed as u8).to_string()),
                    ("allowed_tyres_out".to_string(), "2".to_string()),
                    ("track_meshes".to_string(), built.rust.surfaces.len().to_string()),
                    ("track_ai_length".to_string(), format!("{:08x}", built.rust.length().to_bits())),
                ];
                driver.track = Some(std::sync::Arc::new(built.rust.clone()));
                // what a tyre's capture starts from in a step without a hit
                let surface = object(SURFACE_SIZE);
                wr(surface, SD_GRIP_MOD, 1.0f32);
                wr(surface, SD_IS_VALID_TRACK, 1u8);
                let built: &'static crate::track::GameTrack = Box::leak(Box::new(built));
                game_track = Some(built);
                (built.track, surface)
            } else {
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
            if scenario.whole.pitlane {
                wr(surface, SD_IS_PITLANE, 1u8);
            }
            if scenario.whole.penalty_cut_gas {
                wr(engine, PE_PENALTY_MODE, 0i32);
            }
            if scenario.whole.wind_speed != 0.0 {
                // the game's own PhysicsEngine::setWind(Speed, float): the speed is a one-float
                // struct passed by value (in edx), the direction is the third argument
                let set_wind: extern "C" fn(*mut u8, u32, f32) = std::mem::transmute(acs.va(VA_PHYSICS_ENGINE_SET_WIND));
                set_wind(engine, scenario.whole.wind_speed.to_bits(), scenario.whole.wind_direction_deg);
            }

            if scenario.floor {
                // one big flat quad at y = 0 as a real ODE triangle mesh (two triangles, seen
                // from above counter-clockwise), in static sub-space 1 like the game's tracks
                const HALF: f32 = 3000.0;
                let vertices: [f32; 12] =
                    [-HALF, 0.0, -HALF, -HALF, 0.0, HALF, HALF, 0.0, HALF, HALF, 0.0, -HALF];
                let indices: [u16; 6] = [0, 1, 2, 0, 2, 3];
                let add_surface: extern "C" fn(*mut u8, *mut u8, *const f32, i32, *const u16, i32, *const u8, u32) -> *mut u8 =
                    std::mem::transmute(acs.va(VA_TRACK_ADD_SURFACE));
                floor_object = add_surface(track, wstring(acs, "road"), vertices.as_ptr(), 4, indices.as_ptr(), 6, surface, 1);
            }

            // the track answers the tyres' rays itself: a copy of its vtable (with the RTTI
            // pointer in front) in which rayCast and createRayCaster are ours
            let original: *const usize = rd(track, 0);
            let mut table: Vec<usize> = (0..5).map(|i| *original.offset(i as isize - 1)).collect();
            table[1 + 1] = track_ray_cast as *const () as usize;
            table[1 + 3] = track_create_ray_caster as *const () as usize;
            wr(track, 0, leak(table).add(1));
            (track, surface)
            };

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
                suspension_classes: [0; 4],
                suspension_vtables: [std::ptr::null(); 4],
                tyre_step_original: 0,
                world_step_original: 0,
                controls: Controls { clutch: 1.0, ..Controls::default() },
                polled: 0,
                whole: scenario.whole.on,
                tape: Vec::new(),
                outer_site: 0,
                tyre: None,
                tyres: Default::default(),
                pre: Vec::new(),
                post: Vec::new(),
                solver: Vec::new(),
                joint_params: Vec::new(),
                applied_controls: [0; 0x34],
                stage0_original: 0,
                world_steps: 0,
                islands: 0,
                islands_before_step: 0,
                image_base: acs.va(crate::acs::GHIDRA_BASE),
                game_track,
                ray_hits: [RayRecord::default(); 4],
            });

            if conditions.dynamic_track.is_some() {
                assert_eq!(rd::<u8>(track, TRACK_DYNAMIC_TRACK + 1), 1, "the game's track did not read [DYNAMIC_TRACK] of cfg/race.ini");
            }
            if let Some((base_speed, base_direction)) = wind_settings {
                // the game's own wind job (lambda @ 0x140133ac0: 80..120 % of the base speed,
                // within 20 degrees of the base direction, then PhysicsEngine::setWind). It
                // reads a RaceManager: two numbers and the way to the engine, laid out by hand.
                let manager = object(0x200);
                let sim = object(0x200);
                wr(manager, RM_WIND_BASE_SPEED, base_speed);
                wr(manager, RM_WIND_BASE_DIRECTION, base_direction);
                wr(manager, RM_SIM, sim);
                wr(sim, SIM_PHYSICS_AVATAR, engine.sub(PHYSICS_AVATAR_ENGINE));
                let closure = object(8);
                wr(closure, 0, manager);
                let job: extern "C" fn(*mut u8) = std::mem::transmute(acs.va(VA_GENERATE_WIND_JOB));
                job(closure);
                // Wind: vector, speed (m/s), directionDeg
                let wind = engine.add(PE_WIND);
                conditions_meta.push(("game_wind".to_string(), hex(&[rd::<f32>(wind, 0xc), rd::<f32>(wind, 0x10)])));
            }

            let car = acs.alloc(CAR_SIZE);
            let ctor: extern "C" fn(*mut u8, *mut u8, *mut u8, *mut u8) -> *mut u8 =
                std::mem::transmute(acs.va(VA_CAR_CTOR));
            ctor(car, engine, wstring(acs, &options.car), wstring(acs, ""));
            // [CAR_0] BALLAST / RESTRICTOR: the game's own setters (CarAvatar queues them)
            if conditions.ballast_kg > 0.0 {
                let set: extern "C" fn(*mut u8, f32) = std::mem::transmute(acs.va(VA_CAR_SET_BALLAST_KG));
                set(car, conditions.ballast_kg);
                conditions_meta.push(("ballast_kg".to_string(), hex(&[conditions.ballast_kg])));
            }
            if conditions.restrictor > 0.0 {
                let set: extern "C" fn(*mut u8, f32) = std::mem::transmute(acs.va(VA_CAR_SET_RESTRICTOR));
                set(car, conditions.restrictor);
                conditions_meta.push(("restrictor".to_string(), hex(&[conditions.restrictor])));
            }
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
            let mut indices: Vec<u16> =
                vec![0, 1, 2, 1, 3, 2, 4, 6, 5, 5, 6, 7, 0, 4, 1, 1, 4, 5, 2, 3, 6, 3, 7, 6, 0, 2, 4, 2, 6, 4, 1, 5, 3, 3, 5, 7];
            let mut identity: [f32; 16] = [1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.];
            // with collisions: the car's own collider mesh (the first mesh of collider.kn5, as
            // CarAvatar::initPhysics picks it) and CarAvatar::makeBodyMatrix of the identity
            let real_mesh = options.colliders.as_ref().and_then(|c| c.mesh.as_ref()).filter(|_| options.collide);
            if let Some(real) = real_mesh {
                vertices = vec![0f32; real.vertices.len() * 11];
                for (i, p) in real.vertices.iter().enumerate() {
                    vertices[i * 11..i * 11 + 3].copy_from_slice(p);
                }
                indices = real.indices.clone();
                for r in 0..4 {
                    for c in 0..4 {
                        identity[4 * r + c] = real.matrix.m[r][c];
                    }
                }
            }
            let vertex_count = vertices.len() / 11;
            let vp = leak(vertices).cast::<u8>();
            wr(mesh, 0x108, vp);
            wr(mesh, 0x110, vp.add(vertex_count * 0x2c));
            wr(mesh, 0x118, vp.add(vertex_count * 0x2c));
            let n = indices.len();
            let ip = leak(indices).cast::<u8>();
            wr(mesh, 0x120, ip);
            wr(mesh, 0x128, ip.add(n * 2));
            wr(mesh, 0x130, ip.add(n * 2));
            let init_mesh: extern "C" fn(*mut u8, *mut u8, *const [f32; 16]) =
                std::mem::transmute(acs.va(VA_CAR_INIT_COLLIDER_MESH));
            init_mesh(car, mesh, &identity);

            // the game's geoms by address, for the contacts of the recording
            let mut geom_names = std::collections::HashMap::new();
            let mut collide_meta: Vec<(String, String)> = Vec::new();
            if options.collide {
                let body: *const u8 = rd(car, CAR_BODY);
                let (begin, end): (*const usize, *const usize) = (rd(body, 0x10), rd(body, 0x18));
                for k in 0..end.offset_from(begin) as usize {
                    geom_names.insert(*begin.add(k), 1000 + k as i32);
                }
                // collisionMeshes[0] (a shared_ptr<BodyCollisionMesh>) -> geomID
                let meshes: *const *const u8 = rd(body, 0x30);
                let mesh_geom: usize = rd(*meshes, 0x10);
                geom_names.insert(mesh_geom, 2000);
                if let Some(track) = game_track {
                    for (i, &object) in track.objects.iter().enumerate() {
                        geom_names.insert(rd::<usize>(object, 0x10), i as i32);
                    }
                }
                if !floor_object.is_null() {
                    geom_names.insert(rd::<usize>(floor_object, 0x10), 0);
                }
                for (i, object) in game_objects.iter().enumerate() {
                    geom_names.insert(object.geom() as usize, 3000 + i as i32);
                }
                // PhysicsEngine::setSessionInfo: no contacts for the first 250 steps (the lap
                // scenario starts the way a session does; the others collide from step 0)
                use crate::track_driver::TrackKind;
                let session_start = matches!(scenario.track_kind(), Some(TrackKind::Lap | TrackKind::FullLap | TrackKind::Run | TrackKind::RunFull));
                let no_collision_steps: i32 = if session_start { 250 } else { 0 };
                let core: *mut u8 = rd(engine, PE_CORE);
                wr(core, 0x60, no_collision_steps);
                collide_meta = vec![
                    ("collide".to_string(), "1".to_string()),
                    ("collider_mesh".to_string(), if real_mesh.is_some() { "real" } else { "box" }.to_string()),
                    ("collider_kn5".to_string(), options.collider_kn5.as_ref().map(|p| p.display().to_string()).unwrap_or_default()),
                    // what the game's mesh contact joints get as bounce_vel: the upper half of
                    // the address of the car's mesh geom
                    ("mesh_bounce_vel".to_string(), format!("{:08x}", (mesh_geom >> 32) as u32)),
                    ("no_collision_steps".to_string(), no_collision_steps.to_string()),
                    ("floor_boxes".to_string(), (end.offset_from(begin)).to_string()),
                ];
            }

            // the controls device: 12 virtual functions, with the RTTI pointer of a real device
            // in front so that the game's dynamic_cast<AIDriver*> answers "not an AI"
            let keyboard = acs.va(VA_KEYBOARD_CONTROL_VTABLE) as *const usize;
            let table: Vec<usize> = vec![
                *keyboard.offset(-1),
                device_destructor as *const () as usize,              // +0x00
                device_acquire_controls as *const () as usize,        // +0x08
                device_get_action as *const () as usize,              // +0x10
                device_send_ff as *const () as usize,                 // +0x18
                device_get_name as *const () as usize,                // +0x20
                device_get_ff_global_gain as *const () as usize,      // +0x28
                device_false as *const () as usize,                   // +0x30 isDeviceConnected (0 in every real device)
                device_on_auto_shifter_changed as *const () as usize, // +0x38
                device_false as *const () as usize,                   // +0x40 IsKeyboardControl
                device_set_vibrations as *const () as usize,          // +0x48
                device_set_engine_rpm as *const () as usize,          // +0x50
                device_false as *const () as usize,                   // +0x58 shouldDelete
            ];
            let device = object(0x40);
            wr(device, 0, leak(table).add(1));
            wr(car, CAR_CONTROLS_PROVIDER, device);

            // two bookkeeping helpers of the player's car that only fill buffers: off
            wr(car, CAR_PERFORMANCE_METER_IS_ENABLED, 0u8);
            wr(car, CAR_TELEMETRY_IS_ENABLED, 0u8);
            // driving aids: the automatic clutch is the scenario's choice, set the way the game's
            // assist option sets it (CarAvatar::setAutoClutchEnabled): "on start" follows the
            // option, "on gear change" is forced on with it and otherwise stays what the car's
            // own drivetrain.ini says
            wr(car, CAR_AUTOCLUTCH + 0xc, scenario.auto_clutch as u8); // useAutoOnStart
            if scenario.auto_clutch {
                wr(car, CAR_AUTOCLUTCH + 0xd, 1u8); // useAutoOnChange
            }
            // the automatic gearbox aid (what ACPlugin::setAutoShift / the assist option write)
            if scenario.auto_shifter {
                wr(car, CAR_AUTO_SHIFT, 1u8); // AutoShifter::isActive
            }
            // the stability aid of the game's options (DrivingAssistManager writes the gain)
            if scenario.whole.stability_gain != 0.0 {
                wr(car, CAR_STABILITY_CONTROL, scenario.whole.stability_gain);
            }

            let mut world = World {
                acs,
                engine,
                car,
                driver,
                step_index: 0,
                telemetry_writer: object(SMW_SIZE),
                avatar: object(AVATAR_SIZE),
                page: object(0x1000),
                setup_changes: Vec::new(),
                powertrain: scenario.powertrain,
                whole: scenario.whole.on,
                track_meta,
                conditions_meta,
                collide: options.collide,
                geom_names,
                game_objects,
            };
            world.track_meta.extend(collide_meta);
            if armed {
                // RaceManager::initOffline -> CarAvatar::armFirstLap in a hot-lap session
                wr(car, CAR_TRANSPONDER + 0x80, 1u8);
            }
            if options.setup_check {
                // no recording: just what the session start does to this car's setup, reported
                // by the game's own SetupManager::step on its (visible) standard output
                world.session_start_setup(&options.car);
                let setup_step: extern "C" fn(*mut u8, f32) = std::mem::transmute(acs.va(VA_SETUP_MANAGER_STEP));
                setup_step(car.add(CAR_SETUP_MANAGER), DT);
                return world;
            }
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
                // `CarAvatar::initPhysics` @ 0x1400d7660: `dynamic_cast<Suspension*>`, so the
                // bump stops of a wheel that is not on a double wishbone stay 0
                let suspension = state().suspensions[i];
                let dwb = SUS_CLASSES[state().suspension_classes[i]].name == "DWB";
                wr(writer, SMW_BUMP_STOPS_UP + i * 4, if dwb { rd::<f32>(suspension, SUS_BUMP_STOP_UP) } else { 0.0 });
                wr(writer, SMW_BUMP_STOPS_DN + i * 4, if dwb { rd::<f32>(suspension, SUS_BUMP_STOP_DN) } else { 0.0 });
            }

            // on the road, facing +z (forceRotation takes the direction the car's tail points)
            let force_rotation: extern "C" fn(*mut u8, *const V3) = std::mem::transmute(acs.va(VA_CAR_FORCE_ROTATION));
            let force_position: extern "C" fn(*mut u8, *const V3, u8) =
                std::mem::transmute(acs.va(VA_CAR_FORCE_POSITION));
            // spawned the way the game spawns a car
            force_rotation(car, &spawn.1);
            force_position(car, &spawn.0, 1);
            let set_damage_level: extern "C" fn(*mut u8, f32) = std::mem::transmute(acs.va(VA_CAR_SET_DAMAGE_LEVEL));
            set_damage_level(car, 0.0);
            let reset_suspension_damage: extern "C" fn(*mut u8) =
                std::mem::transmute(acs.va(VA_CAR_RESET_SUSPENSION_DAMAGE));
            reset_suspension_damage(car);
            if scenario.whole.damage != [0.0; 5] {
                // what collisions leave in Car::damageZoneLevel (only the wings read it here)
                wr(car, CAR_DAMAGE_ZONE_LEVEL, scenario.whole.damage);
            }
            world.session_start_setup(&options.car);
            if conditions.setup {
                let file = options.setup.as_ref().expect("this scenario loads a saved setup: --setup <file>");
                world.load_saved_setup(&options.car, file);
                world.conditions_meta.push(("setup_file".to_string(), file.display().to_string()));
            }
            state().tape.clear();
            world
        }
    }

    /// The two things the game's setup screen does to the player's car when a session starts
    /// with the default setup, in the game's order: the default tyre compound
    /// (`tyres.ini [COMPOUND_DEFAULT] INDEX`) through the game's `Tyre::setCompound`, and the
    /// setup items' round trip through the screen's spinners.
    unsafe fn session_start_setup(&mut self, car_name: &str) {
        use rustyac_physics::data::ini::IniReader;
        let data = Path::new("content/cars").join(car_name).join("data");
        let tyres = IniReader::load(&data.join("tyres.ini")).expect("the car's tyres.ini");
        // the game's INIReader gives 0 for a missing key
        let compound = tyres.get_int("COMPOUND_DEFAULT", "INDEX").unwrap_or(0);
        let set_compound: extern "C" fn(*mut u8, i32) -> u8 = std::mem::transmute(self.acs.va(VA_TYRE_SET_COMPOUND));
        for wheel in 0..4 {
            set_compound(self.car.add(CAR_TYRES + wheel * TYRE_SIZE), compound);
        }
        if let Ok(setup) = IniReader::load(&data.join("setup.ini")) {
            self.setup_changes = apply_setup_screen_defaults(self.car, &setup);
        }
    }

    /// Task 15: a saved setup, loaded after the default one and before the first step.
    ///
    /// The player's path is the setup screen (GUI code that cannot be built here). What it
    /// does is done with the game's own functions wherever one exists without the screen:
    /// * the generic items: `SetupManager::load` @ 0x14028cc90 (the loader the AI's setups go
    ///   through: for every item with a section in the file and in the car's setup.ini it
    ///   computes `newValue` from `VALUE`, `STEP`, `MIN` and `SHOW_CLICKS` and attaches the
    ///   item). It does not keep a value within the spinner's range and knows no display
    ///   factor: equal to the screen for a file whose values are in range;
    /// * the compound: `Tyre::setCompound`; the fuel: `Car::setRequestedFuel(v, true)`; the
    ///   traction-control level: `TractionControl::cycleMode`, as often as the screen's handler
    ///   would call it;
    /// * the gears: the screen writes the ratio of line `VALUE` of the gear's .rto file into
    ///   the item's `newValue` (lambda @ 0x140171560); written here the same way (the table
    ///   read by the port's reader). The game's `SetupManager::step` and
    ///   `Drivetrain::setGearRatio` do the rest in the first step.
    unsafe fn load_saved_setup(&mut self, car_name: &str, file: &Path) {
        use rustyac_physics::data::ini::IniReader;
        let saved = IniReader::load(file).expect("the saved setup");
        assert!(saved.ready, "{}: not found", file.display());
        let data = Path::new("content/cars").join(car_name).join("data");
        let setup = IniReader::load(&data.join("setup.ini")).expect("the car's setup.ini");
        let value = |name: &str| saved.get_int(name, "VALUE").unwrap_or(0);
        let clamp = |v: i32, low: i32, high: i32| if v > high { high } else { v.max(low) };
        let car = self.car;
        // the gear tab
        let manager = car.add(CAR_SETUP_MANAGER);
        let mut item: *mut u8 = rd(manager, 0);
        let end: *mut u8 = rd(manager, 8);
        while item < end {
            let name = read_wstring(item.add(SI_NAME));
            let section = match name.strip_prefix("INTERNAL_GEAR_").and_then(|g| g.parse::<usize>().ok()) {
                Some(gear) if gear >= 2 => Some(format!("GEAR_{}", gear - 1)),
                _ if name == "FINAL_RATIO" => Some("FINAL_GEAR_RATIO".to_string()),
                _ => None,
            };
            if let Some(section) = section.filter(|s| setup.has_section(s) && saved.has_section(&name)) {
                let ratios = rustyac_physics::car::setup::load_gear_ratios(&data.join(setup.get_string(&section, "RATIOS"))).expect("the gear's .rto");
                if !ratios.is_empty() {
                    wr(item, SI_NEW_VALUE, ratios[clamp(value(&name), 0, ratios.len() as i32 - 1) as usize].1);
                }
            }
            item = item.add(SETUP_ITEM_SIZE);
        }
        // the tyres tab
        if saved.has_section("TYRES") {
            let tyres = IniReader::load(&data.join("tyres.ini")).expect("the car's tyres.ini");
            let mut count = 1;
            while tyres.has_section(&format!("FRONT_{count}")) {
                count += 1;
            }
            let set_compound: extern "C" fn(*mut u8, i32) -> u8 = std::mem::transmute(self.acs.va(VA_TYRE_SET_COMPOUND));
            for wheel in 0..4 {
                set_compound(car.add(CAR_TYRES + wheel * TYRE_SIZE), clamp(value("TYRES"), 0, count - 1));
            }
        }
        // the fuel tab: whole litres, at most the tank
        if saved.has_section("FUEL") {
            let car_ini = IniReader::load(&data.join("car.ini")).expect("the car's car.ini");
            let mut max_fuel = car_ini.get_float("FUEL", "MAX_FUEL").unwrap_or(0.0);
            if max_fuel == 0.0 {
                max_fuel = 30.0;
            }
            let set_fuel: extern "C" fn(*mut u8, f32, u8) = std::mem::transmute(self.acs.va(VA_CAR_SET_REQUESTED_FUEL));
            set_fuel(car, clamp(value("FUEL"), 0, max_fuel as i32) as f32, 1);
        }
        // the electronics tab: the traction-control level
        if saved.has_section("TRACTION_CONTROL") {
            let tc = car.add(CAR_TRACTION_CONTROL);
            let mut mode = [0u32; 2];
            let get_mode: extern "C" fn(*mut u8, *mut [u32; 2]) -> *mut [u32; 2] = std::mem::transmute(self.acs.va(VA_TC_GET_CURRENT_MODE));
            get_mode(tc, &mut mode);
            // isPresent
            if rd::<u8>(tc, 0) != 0 && mode[1] > 0 {
                let steps = (clamp(value("TRACTION_CONTROL"), 0, mode[1] as i32) as f32 - mode[0] as f32) as i32;
                let cycle: extern "C" fn(*mut u8, i32) = std::mem::transmute(self.acs.va(VA_TC_CYCLE_MODE));
                for _ in 0..steps.abs() {
                    cycle(tc, if steps < 0 { -1 } else { 1 });
                }
            }
        }
        // the generic tabs: the game's own loader, from a copy of the file in the scratch root
        std::fs::create_dir_all("setups").expect("the setups folder of the scratch root");
        std::fs::copy(file, "setups/task15_setup.ini").expect("a copy of the saved setup");
        let load: extern "C" fn(*mut u8, *mut u8) = std::mem::transmute(self.acs.va(VA_SETUP_MANAGER_LOAD));
        load(manager, wstring(self.acs, "setups/task15_setup.ini"));
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
        // `Car::Car` creates the rigid axle before the wheels
        let axle: *mut u8 = rd(car, CAR_RIGID_AXLE);
        if !axle.is_null() {
            st.bodies.push(body_ref("axle", axle));
        }
        for (i, wheel) in WHEELS.iter().enumerate() {
            let suspension = *begin.add(i);
            let vtable = rd::<usize>(suspension, 0);
            let class = SUS_CLASSES
                .iter()
                .position(|class| self.acs.va(class.vtable) == vtable)
                .unwrap_or_else(|| panic!("wheel {wheel}: not one of the four suspension classes"));
            st.suspensions[i] = suspension;
            st.suspension_classes[i] = class;
            let class = &SUS_CLASSES[class];
            if class.name == "ML" {
                // `SuspensionML` never writes bumpStopRate, bumpStopUp, bumpStopDn and
                // packerRange (its step reads the first and the last): the game runs on
                // whatever the heap block held. The oracle gives them the port's value, 0.
                for offset in [0x10, SUS_BUMP_STOP_UP, SUS_BUMP_STOP_DN, 0x2c] {
                    wr(suspension, offset, 0.0f32);
                }
            }
            if let Some(hub) = class.hub {
                st.bodies.push(body_ref(&format!("hub_{wheel}"), rd(suspension, hub)));
            }
            if let Some(strut) = class.strut_body {
                st.bodies.push(body_ref(&format!("strut_{wheel}"), rd(suspension, strut)));
            }
        }
        println!(
            "suspensions: {}",
            st.suspension_classes.iter().map(|&class| SUS_CLASSES[class].name).collect::<Vec<_>>().join(" ")
        );
        for body in &st.bodies {
            assert_eq!(rd::<usize>(body.wrapper, 0), st.body_vtable as usize, "{} is not a RigidBodyODE", body.name);
        }

        // the joints in creation order: the fuel tank's, then each wheel's as its class keeps
        // them (a strut's slider joins the strut body and the hub, so walking the car body's
        // own list would miss it)
        let mut names: Vec<(*mut u8, String)> = Vec::new();
        let tank: *const u8 = rd(car, CAR_FUEL_TANK_JOINT);
        names.push((rd(tank, JOINT_WRAPPER_ODE_JOINT), "fuel_tank".to_string()));
        for (i, wheel) in WHEELS.iter().enumerate() {
            let s = st.suspensions[i];
            match SUS_CLASSES[st.suspension_classes[i]].joints {
                SusJoints::Array(offset, rods) => {
                    for (k, rod) in rods.iter().enumerate() {
                        let wrapper: *const u8 = rd(s, offset + k * 8);
                        names.push((rd(wrapper, JOINT_WRAPPER_ODE_JOINT), format!("{wheel}.{rod}")));
                    }
                }
                SusJoints::Vector(offset, size, joint, prefix) => {
                    let begin: *const u8 = rd(s, offset);
                    let end: *const u8 = rd(s, offset + 8);
                    let count = (end as usize - begin as usize) / size;
                    for k in 0..count {
                        let wrapper: *const u8 = rd(begin, k * size + joint);
                        names.push((rd(wrapper, JOINT_WRAPPER_ODE_JOINT), format!("{wheel}.{prefix}{k}")));
                    }
                }
            }
        }
        // every one of them hangs on a body of the car, and no body has a joint that is not listed
        for body in &st.bodies {
            let mut node: *const u8 = rd(body.ode, B_FIRST_JOINT);
            while !node.is_null() {
                let joint = rd::<*mut u8>(node, 0);
                assert!(names.iter().any(|(ode, _)| *ode == joint), "{} has a joint the oracle does not know", body.name);
                node = rd(node, 0x10);
            }
        }
        for (ode, name) in names {
            let vtable: *const usize = rd(ode, 0);
            let type_of: extern "C" fn(*mut u8) -> u32 = std::mem::transmute(*vtable.add(4));
            let body_of = |offset: usize| {
                let body: *mut u8 = rd(ode, offset);
                st.bodies.iter().position(|b| b.ode == body).unwrap_or(usize::MAX)
            };
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
        set(0x98, body_set_rotation);
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

    /// The scripted driver gave up: the car left the track for good.
    pub fn ended(&self) -> bool {
        self.driver.follower.ended
    }

    /// Task 13, what a recording with collisions holds on top: the contact joints that exist
    /// after the step (the ones `dWorldStep` just used), the collision switches of the core,
    /// and what the car's collision callback writes.
    unsafe fn emit_collisions(&self, row: &mut Row) {
        use rustyac_physics::car::replay::{contact_trace_values, ContactTrace};
        let car = self.car;
        let core: *const u8 = rd(self.engine, PE_CORE);
        let world: *const u8 = rd(core, CORE_WORLD);
        let vtable = self.acs.va(0x1_4051_2d50); // dxJointContact
        let mut contacts = Vec::new();
        // the world's joint list: the newest joint first
        let mut joint: *const u8 = rd(world, 0x28);
        while !joint.is_null() {
            if rd::<usize>(joint, 0) == vtable {
                let name = |geom: usize| self.geom_names.get(&geom).copied().unwrap_or(-1);
                let mode: i32 = rd(joint, 0x90);
                contacts.push(ContactTrace {
                    pos: rd(joint, 0xd0),
                    normal: rd(joint, 0xe0),
                    depth: rd(joint, 0xf0),
                    g1: name(rd(joint, 0xf8)),
                    g2: name(rd(joint, 0x100)),
                    side1: rd(joint, 0x108),
                    side2: rd(joint, 0x10c),
                    mode,
                    mu: rd(joint, 0x94),
                    bounce: rd(joint, 0xa8),
                    soft_erp: if mode & 8 != 0 { rd(joint, 0xb0) } else { 0.0 },
                    soft_cfm: rd(joint, 0xb4),
                    bodies: !rd::<*const u8>(joint, 0x40).is_null() as i32 | (!rd::<*const u8>(joint, 0x58).is_null() as i32) << 1 | ((rd::<u32>(joint, 0x30) & 2 != 0) as i32) << 2,
                });
            }
            joint = rd(joint, 0x10);
        }
        for value in contact_trace_values(&contacts) {
            match value.kind {
                'f' => row.f(&value.name, f32::from_bits(value.word as u32)),
                'd' => row.d(&value.name, f64::from_bits(value.word)),
                _ => row.i(&value.name, value.word as i32),
            }
        }
        // the collision events of this step: `PhysicsEngine::eventQueue` (a
        // `concurrent_queue<ACPhysicsEvent>` at +0x30) is emptied with the queue's own pop, as
        // the game's main thread does for its sounds (`Sim::stepPhysicsEvent`)
        {
            use rustyac_physics::car::chassis::PhysicsEvent;
            use rustyac_physics::vecmath::Vec3f;
            let pop: unsafe extern "system" fn(*mut u8, *mut u8) -> bool = std::mem::transmute(self.acs.msvcp_function(
                c"?_Internal_pop_if_present@_Concurrent_queue_base_v4@details@Concurrency@@IEAA_NPEAX@Z",
            ));
            let queue = (self.engine as *mut u8).add(0x30);
            let mut events = Vec::new();
            let mut raw = [0u8; 0x48];
            while pop(queue, raw.as_mut_ptr()) {
                let p = raw.as_ptr();
                let vector = |at: usize| -> Vec3f {
                    let v: [f32; 3] = rd(p, at);
                    Vec3f::new(v[0], v[1], v[2])
                };
                events.push(PhysicsEvent {
                    kind: rd(p, 0x00),
                    param1: rd(p, 0x04),
                    param2: rd(p, 0x08),
                    param3: rd(p, 0x0c),
                    param4: rd(p, 0x10),
                    v_param1: vector(0x14),
                    v_param2: vector(0x20),
                    ul_param0: rd(p, 0x40),
                });
            }
            for value in rustyac_physics::car::replay::event_trace_values(&events) {
                match value.kind {
                    'f' => row.f(&value.name, f32::from_bits(value.word as u32)),
                    _ => row.i(&value.name, value.word as i32),
                }
            }
        }
        // the loose objects: the matrices their step handlers queued for the picture are
        // thrown away (TrackObject::update pops them every frame), their bodies are noted
        if !self.game_objects.is_empty() {
            let pop: unsafe extern "system" fn(*mut u8, *mut u8) -> bool = std::mem::transmute(self.acs.msvcp_function(
                c"?_Internal_pop_if_present@_Concurrent_queue_base_v4@details@Concurrency@@IEAA_NPEAX@Z",
            ));
            let mut matrix = [0u8; 0x40];
            let mut traces = Vec::with_capacity(self.game_objects.len());
            for object in &self.game_objects {
                while pop(object.queue, matrix.as_mut_ptr()) {}
                let body = object.body();
                let state = body_state(body);
                traces.push(rustyac_physics::car::replay::ObjectTrace {
                    // dxBody::flags, dxBodyDisabled = 4
                    enabled: rd::<u32>(body, 0x38) & 4 == 0,
                    mask: rd(object.geom(), 0x7c),
                    pos: state.pos,
                    q: state.q,
                    lvel: state.lvel,
                    avel: state.avel,
                });
            }
            for value in rustyac_physics::car::replay::object_trace_values(&traces) {
                match value.kind {
                    'f' => row.f(&value.name, f32::from_bits(value.word as u32)),
                    _ => row.i(&value.name, value.word as i32),
                }
            }
        }
        row.i("collide.noCollisionCounter", rd(core, 0x60));
        row.i("collide.currentFrame", rd(core, 0xa0));
        row.d("car.lastCollisionTime", rd(car, 0x3508));
        row.d("car.lastCollisionWithCarTime", rd(car, 0x3c30));
        // the collide bits of the car's collider mesh (Car::updateColliderStatus)
        let body: *const u8 = rd(car, CAR_BODY);
        let meshes: *const *const u8 = rd(body, 0x30);
        let mesh_geom: *const u8 = rd(*meshes, 0x10);
        row.i("car.meshCollideMask", rd(mesh_geom, 0x7c));
        // ISuspension::getDamage: the bend of each corner as a fraction of its most
        for (w, wheel) in WHEELS.iter().enumerate() {
            // (through the class's own function: slot +0x88; an axle answers 0)
            let st = state();
            let get_damage: extern "C" fn(*mut u8) -> f32 = std::mem::transmute(*st.suspension_vtables[w].add(17));
            row.f(&format!("sus.{wheel}.damage"), get_damage(st.suspensions[w]));
        }
    }

    /// What a track scenario records on top: every tyre's ray, the lap timer, the lap
    /// invalidator, the place along the AI line.
    unsafe fn emit_track(&self, row: &mut Row, track: &crate::track::GameTrack) {
        let st = state();
        let car = self.car;
        for (w, wheel) in WHEELS.iter().enumerate() {
            let ray = st.ray_hits[w];
            row.i(&format!("ray.{wheel}.hit"), ray.hit as i32);
            row.v(&format!("ray.{wheel}.pos"), &ray.pos);
            row.v(&format!("ray.{wheel}.normal"), &ray.normal);
            row.i(&format!("ray.{wheel}.mesh"), ray.mesh);
            // the surface the tyre stands on after its step, as the mesh it belongs to
            let surface: *const u8 = rd(car, CAR_TYRES + w * TYRE_SIZE + T_SURFACE_DEF);
            let mesh = if surface.is_null() { -1 } else { track.surface_index_of.get(&(surface as usize)).map(|i| *i as i32).unwrap_or(-2) };
            row.i(&format!("tyre.{wheel}.surfaceMesh"), mesh);
        }
        let tp = car.add(CAR_TRANSPONDER);
        row.i("transponder.t", rd(tp, 0x00));
        row.i("transponder.lastLap", rd(tp, 0x04));
        row.i("transponder.bestLap", rd(tp, 0x08));
        row.i("transponder.lapCount", rd(tp, 0x0c));
        row.i("transponder.finishLinePassed", rd::<u8>(tp, 0x10) as i32);
        row.i("transponder.wasLastLapValid", rd::<u8>(tp, 0x11) as i32);
        row.i("transponder.cuts", rd(tp, 0x84));
        let status: *const u8 = rd(tp, 0x18);
        let current: *const u32 = rd(tp, 0x60);
        let last: *const u32 = rd(tp, 0x30);
        for k in 0..track.rust.time_lines.len() {
            row.i(&format!("transponder.status.{k}.isValid"), rd::<u8>(status, 12 * k) as i32);
            row.i(&format!("transponder.status.{k}.lastResponse"), rd(status, 12 * k + 4));
            row.i(&format!("transponder.status.{k}.lastTime"), rd(status, 12 * k + 8));
            row.i(&format!("transponder.currentSplits.{k}"), *current.add(k) as i32);
            row.i(&format!("transponder.lastLapSplits.{k}"), *last.add(k) as i32);
        }
        let invalidator = car.add(CAR_LAP_INVALIDATOR);
        row.i("lapInvalidator.currentTyresOut", rd(invalidator, 0x10));
        row.i("lapInvalidator.isInPenaltyZone", rd::<u8>(invalidator, 0x14) as i32);
        let locator = car.add(CAR_SPLINE_LOCATOR);
        row.i("splineLocator.currentIndex", rd(locator, 0x10));
        row.f("splineLocator.normalizedPos", rd(locator, 0x20));
        row.f("splineLocator.offset", rd(locator, 0x24));
        row.i("splineLocator.isOutsideLimits", rd::<u8>(locator, 0x28) as i32);
        let data = car.add(CAR_SPLINE_LOCATOR_DATA);
        row.f("splineData.npos", rd(data, 0x00));
        row.f("splineData.lateralOffset", rd(data, 0x08));
        row.f("splineData.splineLength", rd(data, 0x0c));
        row.f("splineData.sides.0", rd(data, 0x10));
        row.f("splineData.sides.1", rd(data, 0x14));
        row.f("splineData.sidesFromIL.0", rd(data, 0x18));
        row.f("splineData.sidesFromIL.1", rd(data, 0x1c));
        row.f("splineData.sideVelocity", rd(data, 0x20));
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
            let pos = v3(body, B_POS);
            let lock: f32 = rd(car, CAR_STEER_LOCK);
            let steer_ratio: f32 = rd(car, CAR_STEER_RATIO);
            CarView {
                position: pos,
                forward: [r[2], r[6], r[10]],
                left: [r[0], r[4], r[8]],
                npos: if self.step_index == 0 { -1.0 } else { rd(car, CAR_SPLINE_LOCATOR_DATA) },
                offset: rd(car, CAR_SPLINE_LOCATOR + 0x24),
                max_wheel_angle: if steer_ratio != 0.0 { (lock / steer_ratio).to_radians() } else { 0.3 },
                step: self.step_index,
                speed,
                road_rpm: if radius > 0.0 { speed / radius * ratio.abs() * 9.549_296_6 } else { 0.0 },
                gear: rd(car, CAR_DRIVETRAIN + 0x584),
                yaw_rate: -(w[0] * up[0] + w[1] * up[1] + w[2] * up[2]),
                laps: rd(car.add(CAR_TRANSPONDER), 0x0c),
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
        if controls.teleport != 0 {
            // what the game's main thread queues for a teleport (CarAvatar::forcePosition),
            // with or without spoiling the lap; its body calls are not part of the step's tape
            unsafe {
                let force_rotation: extern "C" fn(*mut u8, *const V3) = std::mem::transmute(self.acs.va(VA_CAR_FORCE_ROTATION));
                let force_position: extern "C" fn(*mut u8, *const V3, u8) = std::mem::transmute(self.acs.va(VA_CAR_FORCE_POSITION));
                if controls.teleport == 3 {
                    // put down in any attitude: RigidBodyODE::setRotation on the car body and
                    // the fuel tank (the function itself, past the recorder's wrappers)
                    let rows = &controls.teleport_rows;
                    let m: [f32; 16] = [
                        rows[0][0], rows[0][1], rows[0][2], 0.0, rows[1][0], rows[1][1], rows[1][2], 0.0, rows[2][0], rows[2][1], rows[2][2], 0.0, 0.0, 0.0, 0.0, 1.0,
                    ];
                    let set_rotation: extern "C" fn(*mut u8, *const [f32; 16]) = std::mem::transmute(self.acs.va(0x1_402c_ea00));
                    set_rotation(st.bodies[0].wrapper, &m);
                    set_rotation(st.bodies[1].wrapper, &m);
                    force_position(self.car, &controls.teleport_position, 0);
                } else {
                    force_rotation(self.car, &controls.teleport_tail);
                    force_position(self.car, &controls.teleport_position, (controls.teleport == 2) as u8);
                }
            }
        }
        if controls.reset_objects {
            // the job a new session queues for the start of the next step
            unsafe { crate::track::reset_objects(self.acs, &self.game_objects) };
        }
        st.tape.clear();
        st.ray_hits = [RayRecord::default(); 4];
        for capture in &mut st.tyres {
            *capture = TyreCapture::default();
        }
        let before_world_steps = st.world_steps;
        let before_islands = st.islands;
        let time_ms = CLOCK_START_MS + (self.step_index + 1) as f64 * 3.0;
        if controls.bias_clicks != 0 {
            // a click of the cockpit brake-bias control: in the game a command queued for the
            // physics thread and run before the step (PhysicsAvatar::stepCommandQueue)
            unsafe {
                let set_manual_front_bias: extern "C" fn(*mut u8, i32) =
                    std::mem::transmute(self.acs.va(VA_BRAKE_SYSTEM_SET_MANUAL_FRONT_BIAS));
                set_manual_front_bias(self.car.add(CAR_BRAKE_SYSTEM), controls.bias_clicks);
            }
        }
        // Task 16, the cockpit settings of a hybrid car: the jobs `CarAvatar::cycleERSPower`,
        // `cycleERSRecovery`, `cycleERSHeatCharging` and `cycleEngineBrake` queue
        let mut controls = controls;
        unsafe {
            let ers = self.car.add(CAR_ERS);
            let ers_present = rd::<u8>(ers, 0x10) != 0;
            // std::vector<ERSPowerController> (0x48 bytes each)
            let count = (rd::<usize>(ers, 0x30) - rd::<usize>(ers, 0x28)) / 0x48;
            if controls.ers_power >= 0 && ers_present && count > 0 {
                controls.ers_power %= count as i32;
                let set_power_controller: extern "C" fn(*mut u8, i32) = std::mem::transmute(self.acs.va(VA_ERS_SET_POWER_CONTROLLER));
                set_power_controller(ers, controls.ers_power);
            } else {
                controls.ers_power = -1;
            }
            if controls.ers_recovery >= 0 && ers_present {
                // lambda 0x1400d0570: `cvtdq2ps` ; `mulss 0.1f`
                wr(ers, 0x14, controls.ers_recovery as f32 * 0.1f32);
            } else {
                controls.ers_recovery = -1;
            }
            if controls.ers_heat != 0 && ers_present {
                wr(ers, 0x20, (controls.ers_heat > 0) as u8);
            } else {
                controls.ers_heat = 0;
            }
            // Engine::gasCoastOffsetCurve (Engine + 0x360; a Curve's `references` vector is at
            // +0x8): its points are the settings
            let engine = self.car.add(CAR_DRIVETRAIN + 0xe0);
            let settings = (rd::<usize>(engine, 0x360 + 0x10) - rd::<usize>(engine, 0x360 + 0x8)) / 4;
            if controls.engine_brake >= 0 && settings > 1 {
                controls.engine_brake = controls.engine_brake.min(settings as i32 - 1);
                let set_coast_settings: extern "C" fn(*mut u8, i32) = std::mem::transmute(self.acs.va(VA_ENGINE_SET_COAST_SETTINGS));
                set_coast_settings(engine, controls.engine_brake);
            } else {
                controls.engine_brake = -1;
            }
        }
        let controls = controls;
        // what the game's main thread would queue for the physics thread: the game's own
        // functions, called before the step (the engine's clock still shows the last step)
        unsafe {
            if controls.lock_ms != 0.0 {
                let lock_until: extern "C" fn(*mut u8, f64, f64) = std::mem::transmute(self.acs.va(VA_CAR_LOCK_CONTROLS_UNTIL));
                lock_until(self.car, controls.lock_ms as f64, rd::<f64>(self.engine, PE_PHYSICS_TIME));
            }
            if controls.set_locked != 0 {
                let lock: extern "C" fn(*mut u8, u8) = std::mem::transmute(self.acs.va(VA_CAR_LOCK_CONTROLS));
                lock(self.car, (controls.set_locked > 0) as u8);
            }
            if controls.gentle_stop != 0 {
                wr(self.car, CAR_IS_GENTLE_STOPPING, (controls.gentle_stop > 0) as u8);
            }
            if controls.add_penalty != 0.0 {
                let add_penalty: extern "C" fn(*mut u8, f64) = std::mem::transmute(self.acs.va(VA_CAR_ADD_PENALTY));
                add_penalty(self.car, controls.add_penalty as f64);
            }
        }
        let step: extern "C" fn(*mut u8, f32, f64, f64) =
            unsafe { std::mem::transmute(self.acs.va(VA_PHYSICS_ENGINE_STEP)) };
        step(self.engine, DT, time_ms, time_ms);
        let st = state();
        assert_eq!(st.world_steps, before_world_steps + 1, "dWorldStep did not run exactly once");
        // (one island: the car; more when loose objects are awake and not touching it)
        assert!(st.islands > before_islands, "ODE stepped no island");
        if self.game_objects.is_empty() {
            assert_eq!(st.islands, before_islands + 1, "ODE did not step exactly one island");
        }
        // a car whose controls are locked outright (or that is black-flagged) does not ask
        let locked = unsafe { rd::<u8>(self.car, CAR_IS_CONTROLS_LOCKED) != 0 || rd::<u8>(self.car, CAR_BLACK_FLAGGED) != 0 };
        assert_eq!(st.polled, if locked { 0 } else { 1 }, "the controls device was not polled as expected");

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
        unsafe {
            self.emit(&mut row, &controls);
            if let Some(track) = state().game_track {
                if !self.game_objects.is_empty() {
                    row.i("script.resetObjects", controls.reset_objects as i32);
                }
                row.i("script.teleport", controls.teleport);
                row.v("script.teleportPosition", &controls.teleport_position);
                row.v("script.teleportTail", &controls.teleport_tail);
                if self.collide {
                    for (k, axis) in controls.teleport_rows.iter().enumerate() {
                        row.v(&format!("script.teleportRow{k}"), axis);
                    }
                }
                self.emit_track(&mut row, track);
            }
            if self.collide {
                self.emit_collisions(&mut row);
            }
        }
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
        if self.powertrain {
            row.f("script.handBrake", script.hand_brake);
            row.i("script.requestedGear", script.requested_gear);
            row.i("script.biasClicks", script.bias_clicks);
        }
        // Task 16: a hybrid car's button and cockpit jobs (as they were done: see `step`)
        let (kers_present, ers_present) = unsafe { (rd::<u8>(car, CAR_KERS + 0x18) != 0, rd::<u8>(car, CAR_ERS + 0x10) != 0) };
        if self.whole && (kers_present || ers_present) {
            row.i("script.kers", script.kers as i32);
            row.i("script.ersPower", script.ers_power);
            row.i("script.ersRecovery", script.ers_recovery);
            row.i("script.ersHeat", script.ers_heat);
            row.i("script.engineBrake", script.engine_brake);
        }
        if self.whole {
            row.i("script.drs", script.drs as i32);
            row.i("script.headlights", script.headlights as i32);
            row.f("script.lockMs", script.lock_ms);
            row.i("script.setLocked", script.set_locked);
            row.i("script.gentleStop", script.gentle_stop);
            row.f("script.addPenalty", script.add_penalty);
        }
        // Car::controls as Car::step left it (read when dWorldStep starts): after the game's
        // own overrides and helpers (control lock, automatic clutch, automatic throttle blip)
        let c = st.applied_controls.as_ptr();
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
            emit_quaternion(row, &format!("{n}.pre.q"), &pre.q);
            row.v(&format!("{n}.pre.R"), &pre.r);
            row.v(&format!("{n}.pre.lvel"), &pre.lvel);
            row.v(&format!("{n}.pre.avel"), &pre.avel);
            row.v(&format!("{n}.facc"), &pre.facc);
            row.v(&format!("{n}.tacc"), &pre.tacc);
            row.v(&format!("{n}.solver.facc"), &st.solver[i].0);
            row.v(&format!("{n}.solver.tacc"), &st.solver[i].1);
            row.i(&format!("{n}.tag"), rd(body.ode, B_TAG));
            row.v(&format!("{n}.post.pos"), &post.pos);
            emit_quaternion(row, &format!("{n}.post.q"), &post.q);
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

    /// The main state values of the car's systems at the end of the step. Two are older by
    /// the game's own design: `car.speed` (cached before the step from the body's velocity)
    /// and `car.accG` (computed at the top of `Car::step`).
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
        row.f("physics.ambientTemperature", rd(engine, PE_AMBIENT_TEMPERATURE));
        row.f("physics.roadTemperature", rd(engine, PE_ROAD_TEMPERATURE));
        row.v("physics.wind", &v3(engine, PE_WIND));
        row.i("physics.stepCounter", rd(engine, PE_STEP_COUNTER));
        row.d("physics.physicsTime", rd(engine, PE_PHYSICS_TIME));
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
        row.f("drivetrain.diffPowerRamp", rd(d, 0xc8));
        row.f("drivetrain.diffCoastRamp", rd(d, 0xcc));
        row.f("drivetrain.diffPreLoad", rd(d, 0xd0));
        row.i("drivetrain.gearRequest.request", rd(d, 0x5a0));
        row.i("drivetrain.gearRequest.requestedGear", rd(d, 0x5a0 + 0x18));
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
        row.f("engine.gasUsage", rd(e, 0x31c));

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
            let class = &SUS_CLASSES[st.suspension_classes[i]];
            row.f(&format!("suspension.{wheel}.travel"), rd(s, class.status));
            row.f(&format!("suspension.{wheel}.damperSpeedMS"), rd(s, class.status + 4));
            // (a rigid axle keeps no steer torque: its `getSteerTorque` returns 0)
            row.f(&format!("suspension.{wheel}.steerTorque"), class.steer_torque.map_or(0.0, |offset| rd(s, offset)));
            row.f(&format!("suspension.{wheel}.steerAngle"), class.steer_angle.map_or(0.0, |offset| rd(s, offset)));
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
        row.i("stability.useBeta", rd::<u8>(car, CAR_STABILITY_CONTROL + 4) as i32);
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
        if self.powertrain {
            // more of the brakes, the engine and the drivetrain (offsets from the PDB's types)
            let d = car.add(CAR_DRIVETRAIN);
            row.d("drivetrain.gearRequest.timeAccumulator", rd(d, 0x5a0 + 0x8));
            row.d("drivetrain.gearRequest.timeout", rd(d, 0x5a0 + 0x10));
            row.d("drivetrain.validShiftRPMWindow", rd(d, 0x5d8));
            row.d("drivetrain.lastRatio", rd(d, 0x588));
            row.d("drivetrain.outShaftL.oldVelocity", rd(d, 0x38 + 0x10));
            row.d("drivetrain.outShaftR.oldVelocity", rd(d, 0x50 + 0x10));
            let e = d.add(0xe0);
            row.f("engine.bov", rd(e, 0x154));
            row.f("engine.maxPowerW_Dynamic", rd(e, 0x1fc));
            row.f("engine.gasCoastOffset", rd(e, 0x358));
            row.f("engine.limiterMultiplier", rd(e, 0x14c));
            // std::vector<Turbo> (0x24 bytes each): userSetting, rotation, maxBoost ... wastegate
            let first: *const u8 = rd(e, 0x158);
            let last: *const u8 = rd(e, 0x160);
            let count = (last as usize - first as usize) / 0x24;
            row.i("engine.turbos", count as i32);
            for k in 0..2 {
                let turbo = if k < count { first.add(k * 0x24) } else { std::ptr::null() };
                let value = |offset: usize| if turbo.is_null() { 0.0f32 } else { rd(turbo, offset) };
                row.f(&format!("engine.turbo{k}.userSetting"), value(0x0));
                row.f(&format!("engine.turbo{k}.rotation"), value(0x4));
                row.f(&format!("engine.turbo{k}.maxBoost"), value(0x8));
                row.f(&format!("engine.turbo{k}.wastegate"), value(0x8 + 0x14));
            }
            let b = car.add(CAR_BRAKE_SYSTEM);
            row.f("brakes.biasOverride", rd(b, 0x274));
            row.f("brakes.ebbInstant", rd(b, 0x10));
            row.f("brakes.rearCorrectionTorque", rd(b, 0x260));
            row.d("autoBlip.blipStartTime", rd(car, CAR_AUTO_BLIP + 0x90));
            row.f("autoShift.gasCutoff", rd(car, CAR_AUTO_SHIFT + 0x1c));
            row.i("autoShift.changeUpRpm", rd(car, CAR_AUTO_SHIFT + 0x4));
            row.i("autoShift.changeDnRpm", rd(car, CAR_AUTO_SHIFT + 0x8));
            row.i("autoClutch.isForced", rd::<u8>(car, CAR_AUTOCLUTCH + 0xe) as i32);
        }
        if self.whole {
            // more of the aids and the wings (offsets from the PDB's types)
            let tc = car.add(CAR_TRACTION_CONTROL);
            row.f("tc.timeAccumulator", rd(tc, 0x1c));
            row.i("tc.currentMode", rd(tc, 0x20));
            let abs = car.add(CAR_ABS);
            row.f("abs.timeAccumulator", rd(abs, 0x18));
            row.f("abs.slipRatioLimit", rd(abs, 0x4));
            row.i("abs.currentMode", rd(abs, 0xa0));
            row.f("edl.outBrakeTorque", rd(car, CAR_EDL + 0x20));
            row.f("edl.speedDiff", rd(car, CAR_EDL + 0x24));
            row.i("speedLimiter.shoudLimit", rd::<u8>(car, CAR_SPEED_LIMITER) as i32);
            row.i("drs.isPresent", rd::<u8>(car, CAR_DRS) as i32);
            row.i("drs.isAvailable", rd::<u8>(car, CAR_DRS + 2) as i32);
            row.i("drs.lastState", rd::<u8>(car, CAR_DRS + 0x28) as i32);
            let a = car.add(CAR_AERO_MAP);
            let wings: *const u8 = rd(a, 0x38);
            let wings_end: *const u8 = rd(a, 0x40);
            for k in 0..(wings_end as usize - wings as usize) / 0x310 {
                let w = wings.add(k * 0x310);
                row.f(&format!("wing{k}.inputAngle"), rd(w, 0x250 + 0x10));
                row.f(&format!("wing{k}.angleMult"), rd(w, 0x250 + 0x24));
                row.f(&format!("wing{k}.groundEffectLift"), rd(w, 0x250 + 0x28));
                row.f(&format!("wing{k}.groundEffectDrag"), rd(w, 0x250 + 0x2c));
                row.v(&format!("wing{k}.liftVector"), &v3(w, 0x250 + 0x38));
                row.i(&format!("wing{k}.overrideActive"), rd::<u8>(w, 0x300) as i32);
            }
            row.f("physics.wind.speed", rd(self.engine, PE_WIND + 0xc));
            // the car-level glue of Car::step / Car::postStep
            row.f("car.vibrationPhase", rd(car, 0x3e18));
            row.f("car.slipVibrationPhase", rd(car, 0x3e1c));
            row.i("car.lightsOn", rd::<u8>(car, 0x3c7c) as i32);
            row.i("car.isCollisionOffForPits", rd::<u8>(car, 0x3e68) as i32);
            row.i("car.hasGridPosition", rd::<u8>(car, 0x3e8c) as i32);
            row.v("car.gridPosition", &v3(car, 0x3e80));
            row.v("car.slipStream.tip", &v3(car, 0x3ca8 + 0x8));
            row.v("car.slipStream.dir", &v3(car, 0x3ca8 + 0x48));
            row.f("car.slipStream.length", rd(car, 0x3ca8 + 0x60));
            row.d("car.lockControlsTime", rd(car, 0x3d18));
            row.d("car.penaltyTimeAccumulator", rd(car, 0x3d28));
            row.i("car.disableMinSpeedPenaltyClear", rd::<u8>(car, 0x3e2c) as i32);
            row.i("car.isGentleStopping", rd::<u8>(car, CAR_IS_GENTLE_STOPPING) as i32);
            {
                // IRigidBody::getMeshCollideMask(0) (+0x140) of the car body
                let body: *mut u8 = rd(car, 0x118);
                let vtable: *const usize = rd(body, 0);
                let get_mask: extern "C" fn(*mut u8, u32) -> u64 = std::mem::transmute(*vtable.add(0x140 / 8));
                row.i("car.meshCollideMask", get_mask(body, 0) as i32);
            }
            for (i, wheel) in WHEELS.iter().enumerate() {
                // Tyre::absOverride at the end of the step (after ABS::step); the tyre block
                // above holds it as the tyre's own step found it
                row.f(&format!("abs.override.{wheel}"), rd(car, CAR_TYRES + i * TYRE_SIZE + 0x41c));
            }
        }
        // Task 16: the hybrid systems of a car that has one
        if rd::<u8>(car, CAR_KERS + 0x18) != 0 {
            let k = car.add(CAR_KERS);
            row.i("controls.kers", rd::<u8>(car, 0x143) as i32);
            row.f("kers.input", rd(k, 0x1c));
            row.f("kers.charge", rd(k, 0x24));
            row.f("kers.currentJ", rd(k, 0x3c));
            let first: *const u8 = rd(k, 0xd0);
            let last: *const u8 = rd(k, 0xd8);
            for n in 0..(last as usize - first as usize) / 0xa0 {
                row.f(&format!("kers.controller.{n}.currentValue"), rd(first.add(n * 0xa0), 0x94));
            }
        }
        if rd::<u8>(car, CAR_ERS + 0x10) != 0 {
            let e = car.add(CAR_ERS);
            row.i("controls.kers", rd::<u8>(car, 0x143) as i32);
            row.f("ers.kineticRecovery", rd(e, 0x14));
            row.f("ers.status.kineticRecovery", rd(e, 0x18));
            row.f("ers.status.heatRecovery", rd(e, 0x1c));
            row.i("ers.isHeatCharginBattery", rd::<u8>(e, 0x20) as i32);
            row.i("ers.isCharging", rd::<u8>(e, 0x5c) as i32);
            row.d("ers.charge", rd(e, 0x1d8));
            row.f("ers.currentJ", rd(e, 0x1e4));
            row.f("ers.input", rd(e, 0x1e8));
            // the delivery profile in use: the stages of `controller` (+0x188) and
            // `controllerFront` (+0x1b0), which `setPowerController` copies in
            for (name, offset) in [("controller", 0x188usize), ("controllerFront", 0x1b0)] {
                let first: *const u8 = rd(e, offset + 0x8);
                let last: *const u8 = rd(e, offset + 0x10);
                row.i(&format!("ers.{name}.stages"), ((last as usize - first as usize) / 0xa0) as i32);
                for n in 0..4 {
                    let value = if n < (last as usize - first as usize) / 0xa0 { rd(first.add(n * 0xa0), 0x94) } else { 0.0f32 };
                    row.f(&format!("ers.{name}.{n}.currentValue"), value);
                }
            }
            let b = car.add(CAR_BRAKE_SYSTEM);
            row.f("ers.rearCorrectionTorque", rd(b, 0x260));
            for (i, wheel) in WHEELS.iter().enumerate().take(2) {
                // Tyre::inputs.electricTorque (the front motors)
                row.f(&format!("ers.electricTorque.{wheel}"), rd(car, CAR_TYRES + i * TYRE_SIZE + 0x8));
            }
        }
        // Task 16: a four-wheel-drive car (TractionType AWD 2, AWD_NEW 3) also has its front
        // shafts, its three differentials or its coupling, and their controllers
        let d = car.add(CAR_DRIVETRAIN);
        let traction: i32 = rd(d, 0x580);
        if traction >= 2 {
            row.i("drivetrain.tractionType", traction);
            row.d("drivetrain.outShaftLF.velocity", rd(d, 0x68));
            row.d("drivetrain.outShaftRF.velocity", rd(d, 0x80));
            row.d("drivetrain.outShaftLF.oldVelocity", rd(d, 0x78));
            row.d("drivetrain.outShaftRF.oldVelocity", rd(d, 0x90));
            row.d("drivetrain.outShaftL.inertia", rd(d, 0x40));
            row.d("drivetrain.outShaftR.inertia", rd(d, 0x58));
            row.d("drivetrain.outShaftLF.inertia", rd(d, 0x70));
            row.d("drivetrain.outShaftRF.inertia", rd(d, 0x88));
            row.f("drivetrain.awdFrontShare", rd(d, 0x4dc));
            for (name, offset) in [("awdFrontDiff", 0x4e0), ("awdRearDiff", 0x4f0), ("awdCenterDiff", 0x500)] {
                row.f(&format!("drivetrain.{name}.power"), rd(d, offset));
                row.f(&format!("drivetrain.{name}.coast"), rd(d, offset + 4));
                row.f(&format!("drivetrain.{name}.preload"), rd(d, offset + 8));
            }
            row.d("drivetrain.awd2.ramp", rd(d, 0x520));
            row.d("drivetrain.awd2.maxTorque", rd(d, 0x528));
            row.f("drivetrain.awd2.currentLockTorque", rd(d, 0x530));
            // DrivetrainControllers: unique_ptr<DynamicController> each; the stages are 0xa0
            // bytes with `currentValue` at +0x94
            for (name, offset) in [("awdFrontShare", 0x620), ("awdCenterLock", 0x628), ("awd2", 0x638)] {
                let controller: *const u8 = rd(d, offset);
                if controller.is_null() {
                    continue;
                }
                let first: *const u8 = rd(controller, 0x8);
                let last: *const u8 = rd(controller, 0x10);
                for k in 0..(last as usize - first as usize) / 0xa0 {
                    row.f(&format!("drivetrain.ctrl.{name}.stage{k}.currentValue"), rd(first.add(k * 0xa0), 0x94));
                }
            }
        }
    }

    /// Facts about the car that do not change during a run, for the recording's header.
    pub fn facts(&self) -> Vec<(String, String)> {
        let st = state();
        let mut out = Vec::new();
        unsafe {
            let tyre = self.car.add(CAR_TYRES);
            out.push(("compound".to_string(), rd::<i32>(tyre, T_CURRENT_COMPOUND_INDEX).to_string()));
            out.push(("setup_changes".to_string(), self.setup_changes.join(",")));
            out.push((
                "ray_casters".to_string(),
                (0..4).filter(|i| rd::<usize>(tyre.add(i * TYRE_SIZE), T_RAY_CASTER) != 0).count().to_string(),
            ));
        }
        unsafe {
            // the wings in the order the game steps them (`wing0` … in the per-step fields)
            let aero = self.car.add(CAR_AERO_MAP);
            let wings: *const u8 = rd(aero, 0x38);
            let end: *const u8 = rd(aero, 0x40);
            let names: Vec<String> =
                (0..(end as usize - wings as usize) / 0x310).map(|k| read_wstring(wings.add(k * 0x310))).collect();
            out.push(("wings".to_string(), names.join(",")));
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

/// ODE's quaternion order is (w, x, y, z).
fn emit_quaternion(row: &mut Row, name: &str, q: &[f32; 4]) {
    for (axis, value) in ["w", "x", "y", "z"].iter().zip(q) {
        row.f(&format!("{name}.{axis}"), *value);
    }
}

/// A `std::wstring` of the game (VS2013 layout) as text.
unsafe fn read_wstring(string: *const u8) -> String {
    let len: usize = rd(string, 0x10);
    let capacity: usize = rd(string, 0x18);
    let data: *const u16 = if capacity < 8 { string.cast() } else { rd(string, 0) };
    String::from_utf16_lossy(std::slice::from_raw_parts(data, len))
}

fn emit_rig_word(row: &mut Row, name: &str, rig_name: &str, word: u64) {
    match rig::field_kind(rig_name) {
        'd' => row.d(name, f64::from_bits(word)),
        'i' => row.i(name, word as i32),
        _ => row.f(name, f32::from_bits(word as u32)),
    }
}
