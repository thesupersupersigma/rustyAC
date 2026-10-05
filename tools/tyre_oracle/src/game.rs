//! AC's own `Tyre` on the rig: fake `ISuspension`, `IRayTrackCollisionProvider`, `Car`,
//! `PhysicsEngine`, `Track` and `IRigidBody` objects for it to talk to, and readers for the
//! members that get recorded. Offsets are from acs.pdb (see re/tyre/types/*.txt).

use std::cell::UnsafeCell;

use rustyac_physics::tyre::rig::{self, Call, StepInput, DT, MAX_CALLS};
use rustyac_physics::tyre::{TyreModelInput, TyreModelOutput};

use crate::acs::{
    Acs, RVA_SCTM_SOLVE, RVA_TYRE_CTOR, RVA_TYRE_INIT, RVA_TYRE_SET_COMPOUND, RVA_TYRE_STEP,
};

const TYRE_SIZE: usize = 0x858;

// Tyre members
const T_INPUTS: usize = 0x0;
const T_STATUS: usize = 0x2e8;
const T_HUB: usize = 0x3a0;
const T_SURFACE_DEF: usize = 0x410;
const T_ABS_OVERRIDE: usize = 0x41c;
const T_THERMAL: usize = 0x420;
pub const T_COMPOUND_DEFS: usize = 0x500;
const T_AI_MULT: usize = 0x538;
const T_EXTERNAL_INPUTS: usize = 0x578;
const T_DRIVEN: usize = 0x5a1;
const T_LOCAL_WHEEL_ROTATION: usize = 0x5b8;
const T_CAR: usize = 0x640;
const T_BLANKETS_ON: usize = 0x64c;
pub const T_SCTM: usize = 0x660;
// TyreStatus / TyreThermalModel members
const S_ANGULAR_VELOCITY: usize = 0x14;
const TH_PATCHES: usize = 0x8;
const TH_CAR: usize = 0xd0;
const PATCH_SIZE: usize = 0x28;
// Car / PhysicsEngine / Track / SurfaceDef members
const CAR_SIZE: usize = 0x3ea0;
const CAR_BODY: usize = 0x118;
const CAR_KS_PHYSICS: usize = 0x138;
const CAR_TORQUE_MODE_EX: usize = 0x3d10;
const CAR_SLEEPING_FRAMES: usize = 0x3d94;
const CAR_FRAMES_TO_SLEEP: usize = 0x3e28;
const CAR_VALUE_CACHE_SPEED: usize = 0x3e90;
const PE_SIZE: usize = 0x278;
const PE_ALLOW_TYRE_BLANKETS: usize = 0xb8;
const PE_TYRE_CONSUMPTION_RATE: usize = 0xcc;
const PE_AMBIENT_TEMPERATURE: usize = 0x100;
const PE_ROAD_TEMPERATURE: usize = 0x104;
const PE_MECHANICAL_DAMAGE_RATE: usize = 0x108;
const PE_TRACK: usize = 0x198;
const TRACK_SIZE: usize = 0x148;
const TRACK_DYNAMIC_GRIP_LEVEL: usize = 0x128;
const SURFACE_SIZE: usize = 0xc8;
const SD_GRIP_MOD: usize = 0x90;
const SD_DIRT_ADDITIVE_K: usize = 0x98;
const SD_IS_VALID_TRACK: usize = 0xa0;
const SD_SIN_HEIGHT: usize = 0xa8;
const SD_SIN_LENGTH: usize = 0xac;
const SD_DAMPING: usize = 0xb4;
const SD_GRANULARITY: usize = 0xb8;

/// What the fake objects answer with and what they were told, for the step in progress.
struct RigState {
    input: Option<StepInput>,
    calls: Vec<Call>,
    surface: *mut u8,
}

struct Global(UnsafeCell<RigState>);
// the oracle is single-threaded; the game code it calls runs on the calling thread
unsafe impl Sync for Global {}
static STATE: Global = Global(UnsafeCell::new(RigState {
    input: None,
    calls: Vec::new(),
    surface: std::ptr::null_mut(),
}));

fn state() -> &'static mut RigState {
    unsafe { &mut *STATE.0.get() }
}

fn input() -> &'static StepInput {
    state()
        .input
        .as_ref()
        .expect("the game called the rig before any input was set")
}

type V3 = [f32; 3];

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

// --- ISuspension -------------------------------------------------------------------------

extern "C" fn hub_get_hub_world_matrix(_this: *mut u8, out: *mut [f32; 16]) -> *mut [f32; 16] {
    unsafe { *out = input().hub_matrix };
    out
}

extern "C" fn hub_get_point_velocity(_this: *mut u8, out: *mut V3, _p: *const V3) -> *mut V3 {
    unsafe { *out = input().hub_velocity };
    out
}

extern "C" fn hub_add_force_at_pos(
    _this: *mut u8,
    force: *const V3,
    pos: *const V3,
    driven: usize,
    add_to_steer_torque: usize,
) {
    // bools arrive in the low byte only
    let flags = (driven & 0xff != 0) as u32 | ((add_to_steer_torque & 0xff != 0) as u32) << 1;
    state().calls.push(Call {
        kind: 1,
        a: unsafe { *force },
        b: unsafe { *pos },
        flags,
        ..Call::default()
    });
}

extern "C" fn hub_add_torque(_this: *mut u8, torque: *const V3) {
    state().calls.push(Call {
        kind: 2,
        a: unsafe { *torque },
        ..Call::default()
    });
}

extern "C" fn hub_get_hub_angular_velocity(_this: *mut u8, out: *mut V3) -> *mut V3 {
    unsafe { *out = input().hub_angular_velocity };
    out
}

extern "C" fn hub_add_local_force_and_torque(
    _this: *mut u8,
    force: *const V3,
    torque: *const V3,
    drive_torque: *const V3,
) {
    state().calls.push(Call {
        kind: 3,
        a: unsafe { *force },
        b: unsafe { *torque },
        c: unsafe { *drive_torque },
        ..Call::default()
    });
}

// --- IRayTrackCollisionProvider ------------------------------------------------------------

extern "C" fn rcp_ray_cast(
    _this: *mut u8,
    org: *const V3,
    _dir: *const V3,
    result: *mut RayCastResult,
    _length: f32,
) -> u8 {
    let input = input();
    let org = unsafe { *org };
    unsafe {
        (*result).has_hit = input.has_hit as u8;
        if input.has_hit {
            (*result).surface_def = state().surface;
            (*result).pos = [org[0], input.ground_y, org[2]];
            (*result).normal = input.ground_normal;
        }
    }
    input.has_hit as u8
}

/// No per-wheel ray caster: `Tyre::step` then calls `rayCast` above.
extern "C" fn rcp_create_ray_caster(_this: *mut u8, _length: f32) -> *mut u8 {
    std::ptr::null_mut()
}

// --- IRigidBody (Car::body) ----------------------------------------------------------------

extern "C" fn body_get_mass(_this: *mut u8) -> f32 {
    input().body_mass
}

extern "C" fn body_get_velocity(_this: *mut u8, out: *mut V3) -> *mut V3 {
    unsafe { *out = input().body_velocity };
    out
}

extern "C" fn body_add_force_at_local_pos(_this: *mut u8, f: *const V3, p: *const V3) {
    state().calls.push(Call {
        kind: 4,
        a: unsafe { *f },
        b: unsafe { *p },
        ..Call::default()
    });
}

/// Any other virtual function of the fake objects: the tyre is not supposed to call it.
extern "C" fn unexpected<const TABLE: usize, const SLOT: usize>() {
    let table = ["ISuspension", "IRayTrackCollisionProvider", "IRigidBody"][TABLE];
    eprintln!(
        "the game called {table} vtable slot {SLOT} (+{:#x}), which the rig does not provide",
        SLOT * 8
    );
    std::process::abort();
}

macro_rules! vtable {
    ($table:expr; $($slot:expr),*) => {
        vec![$(unexpected::<$table, $slot> as extern "C" fn() as usize),*]
    };
}

fn leak<T>(value: Vec<T>) -> *mut T {
    Box::leak(value.into_boxed_slice()).as_mut_ptr()
}

unsafe fn wr<T>(base: *mut u8, offset: usize, value: T) {
    base.add(offset).cast::<T>().write_unaligned(value);
}

unsafe fn rd<T: Copy>(base: *const u8, offset: usize) -> T {
    base.add(offset).cast::<T>().read_unaligned()
}

/// The fake world and the game functions.
pub struct Game<'a> {
    acs: &'a Acs,
    hub: *mut u8,
    rcp: *mut u8,
    car: *mut u8,
    physics: *mut u8,
    track: *mut u8,
}

impl<'a> Game<'a> {
    pub fn new(acs: &'a Acs) -> Game<'a> {
        let mut hub_vtable = vtable![0; 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24];
        hub_vtable[1] = hub_get_hub_world_matrix as *const () as usize;
        hub_vtable[2] = hub_get_point_velocity as *const () as usize;
        hub_vtable[3] = hub_add_force_at_pos as *const () as usize;
        hub_vtable[4] = hub_add_torque as *const () as usize;
        hub_vtable[7] = hub_get_hub_angular_velocity as *const () as usize;
        hub_vtable[24] = hub_add_local_force_and_torque as *const () as usize;
        let mut rcp_vtable = vtable![1; 0, 1, 2, 3];
        rcp_vtable[1] = rcp_ray_cast as *const () as usize;
        rcp_vtable[3] = rcp_create_ray_caster as *const () as usize;
        let mut body_vtable = vtable![2; 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20,
            21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39];
        body_vtable[0x28 / 8] = body_get_mass as *const () as usize;
        body_vtable[0x78 / 8] = body_get_velocity as *const () as usize;
        body_vtable[0xf8 / 8] = body_add_force_at_local_pos as *const () as usize;

        // objects are 8-byte aligned, zeroed and never freed
        let object = |size: usize| leak(vec![0u64; size.div_ceil(8)]).cast::<u8>();
        let hub = object(0x40);
        let rcp = object(0x10);
        let body = object(0x10);
        let car = object(CAR_SIZE);
        let physics = object(PE_SIZE);
        let track = object(TRACK_SIZE);
        let surface = object(SURFACE_SIZE);
        unsafe {
            wr(hub, 0, leak(hub_vtable));
            wr(rcp, 0, leak(rcp_vtable));
            wr(body, 0, leak(body_vtable));
            wr(car, CAR_BODY, body);
            wr(car, CAR_KS_PHYSICS, physics);
            wr(physics, PE_TRACK, track);
            wr(surface, SD_IS_VALID_TRACK, 1u8);
        }
        state().surface = surface;
        Game {
            acs,
            hub,
            rcp,
            car,
            physics,
            track,
        }
    }

    /// Makes `input` what the fake objects answer with (hub, road, car).
    pub fn set_world(&self, input: &StepInput) {
        let st = state();
        st.input = Some(*input);
        unsafe {
            wr(st.surface, SD_GRIP_MOD, input.grip_mod);
            wr(st.surface, SD_DIRT_ADDITIVE_K, input.dirt_additive_k);
            wr(st.surface, SD_SIN_HEIGHT, input.sin_height);
            wr(st.surface, SD_SIN_LENGTH, input.sin_length);
            wr(st.surface, SD_DAMPING, input.damping);
            wr(st.surface, SD_GRANULARITY, input.granularity);
            wr(self.car, CAR_TORQUE_MODE_EX, input.torque_mode);
            wr(self.car, CAR_VALUE_CACHE_SPEED, input.car_speed);
            // Car::isSleeping() is sleepingFrames > framesToSleep
            wr(self.car, CAR_SLEEPING_FRAMES, input.car_sleeping as i32);
            wr(self.car, CAR_FRAMES_TO_SLEEP, 0i32);
            wr(
                self.physics,
                PE_ALLOW_TYRE_BLANKETS,
                input.allow_tyre_blankets as u8,
            );
            wr(
                self.physics,
                PE_TYRE_CONSUMPTION_RATE,
                input.tyre_consumption_rate,
            );
            wr(
                self.physics,
                PE_AMBIENT_TEMPERATURE,
                input.ambient_temperature,
            );
            wr(self.physics, PE_ROAD_TEMPERATURE, input.road_temperature);
            wr(
                self.physics,
                PE_MECHANICAL_DAMAGE_RATE,
                input.mechanical_damage_rate,
            );
            wr(
                self.track,
                TRACK_DYNAMIC_GRIP_LEVEL,
                input.dynamic_grip_level,
            );
        }
    }

    /// `new Tyre()` + `Tyre::init(hub, rcp, dataPath, index, 0, nullptr)`: the game reads
    /// `<data_path>tyres.ini` itself, exactly as its tyre test bench does. `set_world` must
    /// have been called (init asks the hub for its matrix).
    pub fn new_tyre(&self, data_path: &str, index: i32) -> GameTyre<'_> {
        let ptr = self.acs.alloc(TYRE_SIZE);
        let ctor: extern "C" fn(*mut u8) -> *mut u8 =
            unsafe { std::mem::transmute(self.acs.addr(RVA_TYRE_CTOR)) };
        ctor(ptr);
        // std::wstring by value: the callee destroys it, so its buffer comes from the
        // game's allocator
        let units: Vec<u16> = data_path.encode_utf16().collect();
        let string = self.acs.alloc(32);
        unsafe {
            if units.len() < 8 {
                std::ptr::copy_nonoverlapping(units.as_ptr(), string.cast::<u16>(), units.len());
                wr(string, 0x18, 7usize);
            } else {
                let buffer = self.acs.alloc((units.len() + 1) * 2);
                std::ptr::copy_nonoverlapping(units.as_ptr(), buffer.cast::<u16>(), units.len());
                wr(string, 0, buffer);
                wr(string, 0x18, units.len());
            }
            wr(string, 0x10, units.len());
        }
        let init: extern "C" fn(*mut u8, *mut u8, *mut u8, *mut u8, i32, i32, *mut u8) =
            unsafe { std::mem::transmute(self.acs.addr(RVA_TYRE_INIT)) };
        init(
            ptr,
            self.hub,
            self.rcp,
            string,
            index,
            0,
            std::ptr::null_mut(),
        );
        assert_eq!(
            unsafe { rd::<*mut u8>(ptr, T_HUB) },
            self.hub,
            "Tyre::init did not store the hub"
        );
        GameTyre { game: self, ptr }
    }
}

/// One of the game's `Tyre` objects.
pub struct GameTyre<'a> {
    game: &'a Game<'a>,
    pub ptr: *mut u8,
}

impl GameTyre<'_> {
    /// `tyre->car` and `tyre->thermalModel.car`.
    pub fn set_car(&self, present: bool) {
        let car = if present {
            self.game.car
        } else {
            std::ptr::null_mut()
        };
        unsafe {
            wr(self.ptr, T_CAR, car);
            wr(self.ptr, T_THERMAL + TH_CAR, car);
        }
    }

    /// `Tyre::setCompound(cindex)`
    pub fn set_compound(&self, cindex: i32) -> bool {
        let f: extern "C" fn(*mut u8, i32) -> u8 =
            unsafe { std::mem::transmute(self.game.acs.addr(RVA_TYRE_SET_COMPOUND)) };
        f(self.ptr, cindex) & 0xff != 0
    }

    pub fn compound_count(&self) -> usize {
        let (begin, end): (usize, usize) = unsafe {
            (
                rd(self.ptr, T_COMPOUND_DEFS),
                rd(self.ptr, T_COMPOUND_DEFS + 8),
            )
        };
        (end - begin) / 0x3f0
    }

    /// `SCTM::solve` on the tyre's own `scTM`.
    pub fn sctm_solve(&self, input: &SctmInput) -> SctmOutput {
        let f: extern "C" fn(*mut u8, *mut SctmOutput, *const SctmInput) -> *mut SctmOutput =
            unsafe { std::mem::transmute(self.game.acs.addr(RVA_SCTM_SOLVE)) };
        let mut out = SctmOutput::default();
        f(unsafe { self.ptr.add(T_SCTM) }, &mut out, input);
        out
    }

    /// The rig's step: apply the inputs, `Tyre::step(0.003)`, read everything back.
    pub fn step(&self, input: &StepInput) -> Vec<u64> {
        self.game.set_world(input);
        self.set_car(input.has_car);
        let p = self.ptr;
        unsafe {
            wr(p, T_INPUTS, input.brake_torque);
            wr(p, T_INPUTS + 4, input.hand_brake_torque);
            wr(p, T_INPUTS + 8, input.electric_torque);
            wr(p, T_ABS_OVERRIDE, input.abs_override);
            wr(p, T_AI_MULT, input.ai_mult);
            wr(p, T_DRIVEN, input.driven as u8);
            wr(p, T_EXTERNAL_INPUTS, input.ext_active as u8);
            wr(p, T_EXTERNAL_INPUTS + 4, input.ext_load);
            wr(p, T_EXTERNAL_INPUTS + 8, input.ext_slip_angle);
            wr(p, T_EXTERNAL_INPUTS + 0xc, input.ext_slip_ratio);
            if let Some(value) = input.set_angular_velocity {
                wr(p, T_STATUS + S_ANGULAR_VELOCITY, value);
            }
            if let Some(on) = input.set_blankets {
                wr(p, T_BLANKETS_ON, on as u8);
            }
        }
        state().calls.clear();
        let step: extern "C" fn(*mut u8, f32) =
            unsafe { std::mem::transmute(self.game.acs.addr(RVA_TYRE_STEP)) };
        step(p, DT);
        self.snapshot()
    }

    /// The game-side twin of `rig::snapshot`, in `rig::output_fields()` order.
    pub fn snapshot(&self) -> Vec<u64> {
        let calls = &state().calls;
        assert!(
            calls.len() <= MAX_CALLS,
            "{} hub calls in one step",
            calls.len()
        );
        let p = self.ptr;
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
                    let call = calls
                        .get(index.parse::<usize>().unwrap())
                        .copied()
                        .unwrap_or_default();
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
                    let offset = index.parse::<usize>().unwrap() * PATCH_SIZE
                        + if member == "T" { 0x18 } else { 0x1c };
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

/// AC's `TyreModelInput` (0x30 bytes).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct SctmInput {
    pub load: f32,
    pub slip_angle_rad: f32,
    pub slip_ratio: f32,
    pub camber_rad: f32,
    pub speed: f32,
    pub u: f32,
    pub tyre_index: i32,
    pub cp_length: f32,
    pub grain: f32,
    pub blister: f32,
    pub pressure_ratio: f32,
    pub use_simple_model: bool,
}

/// AC's `TyreModelOutput` (0x1c bytes).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct SctmOutput {
    pub fy: f32,
    pub fx: f32,
    pub mz: f32,
    pub trail: f32,
    pub nd_slip: f32,
    pub dy: f32,
    pub dx: f32,
}

const _: () = assert!(std::mem::size_of::<SctmInput>() == 0x30);
const _: () = assert!(std::mem::size_of::<SctmOutput>() == 0x1c);

impl SctmInput {
    pub fn to_port(self) -> TyreModelInput {
        TyreModelInput {
            load: self.load,
            slip_angle_rad: self.slip_angle_rad,
            slip_ratio: self.slip_ratio,
            camber_rad: self.camber_rad,
            speed: self.speed,
            u: self.u,
            tyre_index: self.tyre_index,
            cp_length: self.cp_length,
            grain: self.grain,
            blister: self.blister,
            pressure_ratio: self.pressure_ratio,
            use_simple_model: self.use_simple_model,
        }
    }
}

impl SctmOutput {
    pub fn bits(&self) -> [u32; 7] {
        [
            self.fy,
            self.fx,
            self.mz,
            self.trail,
            self.nd_slip,
            self.dy,
            self.dx,
        ]
        .map(f32::to_bits)
    }
}

pub fn port_bits(out: &TyreModelOutput) -> [u32; 7] {
    [
        out.fy,
        out.fx,
        out.mz,
        out.trail,
        out.nd_slip,
        out.dy,
        out.dx,
    ]
    .map(f32::to_bits)
}
