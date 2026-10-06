//! The rolling chassis: AC's `Car` reduced to the parts that are ported (rigid bodies and
//! their masses, fuel, the four suspensions with their tyres, heave springs, anti-roll bars,
//! steering, force feedback), wired together and stepped in the order of `Car::step` and
//! `Car::stepComponents`. Everything else comes in through a [`ChassisFeed`].

use std::cell::RefCell;
use std::path::{Path, PathBuf};

use super::antiroll_bar::AntirollBar;
use super::body::{FixedJoint, ForceSource, PhysicsCore, RigidBody};
use super::feed::{CarControls, ChassisFeed};
use super::heave_spring::HeaveSpring;
use super::setup::SetupManager;
use super::suspension::{SuspensionModel, VanillaDwb};
use crate::data::ini::IniReader;
use crate::math::{fdtest_inf_or_nan, powf, sqrtf};
use crate::tyre::rig;
use crate::tyre::{RayCastResult, RayTrackCollisionProvider, Suspension, TorqueModeEx, TyreCar, VanillaTyre};
use crate::vecmath::{Mat44f, Vec3f};

/// What the chassis reads from AC's `PhysicsEngine`, its `Track` and the driver's device:
/// session settings and a few constants of `system/cfg/assetto_corsa.ini`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ChassisEnvironment {
    /// `PhysicsEngine::ambientTemperature`, deg C
    pub ambient_temperature: f32,
    /// `PhysicsEngine::roadTemperature`, deg C
    pub road_temperature: f32,
    /// `Track::dynamicGripLevel`
    pub dynamic_grip_level: f32,
    /// `PhysicsEngine::tyreConsumptionRate`
    pub tyre_consumption_rate: f32,
    /// `PhysicsEngine::mechanicalDamageRate`
    pub mechanical_damage_rate: f32,
    /// `PhysicsEngine::fuelConsumptionRate`
    pub fuel_consumption_rate: f32,
    /// `PhysicsEngine::allowTyreBlankets`
    pub allow_tyre_blankets: bool,
    /// `PhysicsEngine::flatSpotFFGain` (0.05 in the constructor)
    pub flat_spot_ff_gain: f32,
    /// `PhysicsEngine::gyroWheelGain`: 0.004 with `[FF_EXPERIMENTAL] ENABLE_GYRO`, else 0
    pub gyro_wheel_gain: f32,
    /// `PhysicsEngine::mzLowSpeedReduction.speedKMH` (`[LOW_SPEED_FF] SPEED_KMH`)
    pub mz_low_speed_reduction_speed_kmh: f32,
    /// `PhysicsEngine::mzLowSpeedReduction.minValue` (`[LOW_SPEED_FF] MIN_VALUE`)
    pub mz_low_speed_reduction_min_value: f32,
    /// `ICarControlsProvider::ffFilter` of the driver's device (0 = no filter)
    pub ff_filter: f32,
    /// `ICarControlsProvider::useFakeUndersteerFF`
    pub use_fake_understeer_ff: bool,
    /// The car is the first one created (`Car::physicsGUID == 0`): only that car switches its
    /// joints' ERP with speed.
    pub is_first_car: bool,
}

impl Default for ChassisEnvironment {
    /// The `PhysicsEngine` constructor's values with the game's shipped `assetto_corsa.ini`
    /// (26 deg C air, 30 deg C road as in `tools/car_oracle`), a plain device, the player's car.
    fn default() -> ChassisEnvironment {
        ChassisEnvironment {
            ambient_temperature: 26.0,
            road_temperature: 30.0,
            dynamic_grip_level: 1.0,
            tyre_consumption_rate: 1.0,
            mechanical_damage_rate: 1.0,
            fuel_consumption_rate: 1.0,
            allow_tyre_blankets: true,
            flat_spot_ff_gain: 0.05,
            gyro_wheel_gain: 0.0,
            mz_low_speed_reduction_speed_kmh: 3.0,
            mz_low_speed_reduction_min_value: 0.01,
            ff_filter: 0.0,
            use_fake_understeer_ff: false,
            is_first_car: true,
        }
    }
}

/// AC's `SteeringSystem` (0x40 bytes) without the four-wheel-steer controller (`ctrl_4ws.ini`,
/// 2 of the 113 cars; needs `DynamicController`, a later task).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SteeringSystem {
    /// `linearRatio`: metres of steering-rod shift per degree of `finalSteerAngleSignal`
    /// (`car.ini [CONTROLS] LINEAR_STEER_ROD_RATIO`).
    pub linear_ratio: f32,
}

/// What one tyre was told and did during its step (only kept while tracing).
#[derive(Clone, Debug)]
pub struct TyreTrace {
    /// The inputs of this `Tyre::step` in the layout of the single-wheel rig.
    pub input: rig::StepInput,
    /// `localWheelRotation` on entry.
    pub wheel_rotation_in: Mat44f,
    /// The calls the tyre made on its hub and on the car body.
    pub calls: Vec<rig::Call>,
    /// [`rig::snapshot`] right after the step.
    pub output: Vec<u64>,
}

/// One body as `dWorldStep` finds it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BodyTrace {
    pub mass: f32,
    pub inertia: [f32; 3],
    pub pos: [f32; 3],
    pub q: [f32; 4],
    pub r: [f32; 9],
    pub lvel: [f32; 3],
    pub avel: [f32; 3],
    pub facc: [f32; 3],
    pub tacc: [f32; 3],
}

/// What a step looked like from the inside, for the comparison with the game's recordings.
#[derive(Clone, Debug, Default)]
pub struct StepTrace {
    pub tyres: Vec<TyreTrace>,
    /// The bodies at the entry of `dWorldStep`, in creation order.
    pub pre: Vec<BodyTrace>,
}

/// The rolling chassis of one car.
pub struct RollingChassis {
    /// The rigid-body world with this car's bodies and joints.
    pub core: PhysicsCore,
    pub env: ChassisEnvironment,
    /// The road under the wheels (`Track` as an `IRayTrackCollisionProvider`).
    pub ground: Box<dyn RayTrackCollisionProvider>,
    /// `Car::body`
    pub body: RigidBody,
    /// `Car::fuelTankBody`
    pub fuel_tank_body: RigidBody,
    /// `Car::fuelTankJoint`
    pub fuel_tank_joint: FixedJoint,
    /// `Car::suspensions`: LF, RF, LR, RR. The suspension slot.
    pub suspensions: Vec<Box<dyn SuspensionModel>>,
    /// `Car::tyres`
    pub tyres: Vec<VanillaTyre>,
    /// `Car::heaveSprings`: front, rear
    pub heave_springs: [HeaveSpring; 2],
    /// `Car::antirollBars`: front, rear
    pub antiroll_bars: [AntirollBar; 2],
    /// `Car::steeringSystem`
    pub steering_system: SteeringSystem,
    /// `Car::setupManager`
    pub setup_manager: SetupManager,
    /// `Car::controls`
    pub controls: CarControls,
    /// `Car::finalSteerAngleSignal`, degrees at the road wheels
    pub final_steer_angle_signal: f32,
    /// `Car::mass` (`[BASIC] TOTALMASS`), kg
    pub mass: f32,
    /// `Car::ffMult`
    pub ff_mult: f32,
    /// `Car::accG`: body acceleration in body axes, g
    pub acc_g: Vec3f,
    /// `Car::steerLock`, degrees
    pub steer_lock: f32,
    /// `Car::steerRatio`
    pub steer_ratio: f32,
    /// `Car::steerAssist`
    pub steer_assist: f32,
    /// `Car::torqueModeEx`
    pub torque_mode_ex: TorqueModeEx,
    /// `Car::lastVelocity`
    pub last_velocity: Vec3f,
    /// `Car::sleepingFrames`
    pub sleeping_frames: i32,
    /// `Car::framesToSleep`
    pub frames_to_sleep: i32,
    /// `Car::mzCurrent`
    pub mz_current: f32,
    /// `Car::lastFF`: the force-feedback number handed to the driver's device
    pub last_ff: f32,
    /// `Car::lastPureMZFF`
    pub last_pure_mz_ff: f32,
    /// `Car::lastGyroFF`
    pub last_gyro_ff: f32,
    /// `Car::lastSteerPosition`
    pub last_steer_position: f32,
    /// `Car::flatSpotPhase`
    pub flat_spot_phase: f32,
    /// `Car::bodyInertia`: the box of `[BASIC] INERTIA`, m
    pub body_inertia: Vec3f,
    /// `Car::fuel`, litres
    pub fuel: f64,
    /// `Car::fuelConsumptionK`
    pub fuel_consumption_k: f64,
    /// `Car::maxFuel`
    pub max_fuel: f64,
    /// `Car::requestedFuel`
    pub requested_fuel: f32,
    /// `Car::fuelKG`: kg per litre
    pub fuel_kg: f32,
    /// `Car::lastBodyMassUpdateTime`, ms
    pub last_body_mass_update_time: f64,
    /// `Car::fuelTankPos`
    pub fuel_tank_pos: Vec3f,
    /// `Car::ballastKG`
    pub ballast_kg: f32,
    /// `Car::valueCache.speed`: |body velocity| at the start of the step, m/s
    pub speed: f32,
    /// `Engine::fuelPressure` as the fuel burn leaves it (1, or 0 with an empty tank): an
    /// output towards the engine.
    pub fuel_pressure: f32,
    /// `PhysicsEngine::physicsTime` of the current step, ms.
    pub physics_time: f64,
    /// Filled during a step when it is `Some` on entry.
    pub trace: Option<StepTrace>,
    /// The car's data folder.
    pub data_path: PathBuf,
}

/// The C runtime's `rand()` (MSVCR120): `Suspension::Suspension` draws one number each.
struct MsvcRand(u32);

impl MsvcRand {
    fn next(&mut self) -> i32 {
        self.0 = self.0.wrapping_mul(214013).wrapping_add(2531011);
        ((self.0 >> 16) & 0x7fff) as i32
    }
}

/// `ucomiss x, 0` + `je`: true for an ordered value that is not zero.
#[allow(clippy::double_comparisons)]
fn ordered_nonzero(x: f32) -> bool {
    x < 0.0 || x > 0.0
}

/// `ksSawToothWave` @ 0x14022d2a0.
#[allow(clippy::manual_range_contains)]
pub fn ks_saw_tooth_wave(x: f32, length: f32) -> f32 {
    let half = length * 0.5;
    // cvttss2si: out of range (and NaN) gives i32::MIN
    let quotient = x / length;
    let whole = if quotient.is_nan() || quotient >= 2_147_483_648.0 || quotient < -2_147_483_648.0 {
        i32::MIN
    } else {
        quotient as i32
    };
    ((x - whole as f32 * length) - half) / half
}

/// The tyre's view of its hub: the suspension, with the rigid-body world it needs.
struct HubPort<'a, 'b> {
    core: &'a RefCell<&'b mut PhysicsCore>,
    suspension: &'a mut dyn SuspensionModel,
    trace: Option<&'a RefCell<TyreTrace>>,
    asked: [bool; 3],
}

fn arr(v: &Vec3f) -> [f32; 3] {
    [v.x, v.y, v.z]
}

impl Suspension for HubPort<'_, '_> {
    fn get_hub_world_matrix(&mut self) -> Mat44f {
        let matrix = self.suspension.get_hub_world_matrix(&self.core.borrow());
        if let (Some(trace), false) = (self.trace, self.asked[0]) {
            let mut flat = [0.0; 16];
            for (i, value) in flat.iter_mut().enumerate() {
                *value = matrix.m[i / 4][i % 4];
            }
            trace.borrow_mut().input.hub_matrix = flat;
        }
        self.asked[0] = true;
        matrix
    }

    fn get_point_velocity(&mut self, p: &Vec3f) -> Vec3f {
        let velocity = self.suspension.get_point_velocity(&self.core.borrow(), p);
        if let (Some(trace), false) = (self.trace, self.asked[1]) {
            trace.borrow_mut().input.hub_velocity = arr(&velocity);
        }
        self.asked[1] = true;
        velocity
    }

    fn add_force_at_pos(&mut self, force: &Vec3f, pos: &Vec3f, driven: bool, add_to_steer_torque: bool) {
        if let Some(trace) = self.trace {
            trace.borrow_mut().calls.push(rig::Call {
                kind: 1,
                a: arr(force),
                b: arr(pos),
                flags: driven as u32 | (add_to_steer_torque as u32) << 1,
                ..rig::Call::default()
            });
        }
        self.suspension.add_force_at_pos(&mut self.core.borrow_mut(), force, pos, driven, add_to_steer_torque);
    }

    fn add_torque(&mut self, torque: &Vec3f) {
        if let Some(trace) = self.trace {
            trace.borrow_mut().calls.push(rig::Call { kind: 2, a: arr(torque), ..rig::Call::default() });
        }
        self.suspension.add_torque(&mut self.core.borrow_mut(), torque);
    }

    fn get_hub_angular_velocity(&mut self) -> Vec3f {
        let velocity = self.suspension.get_hub_angular_velocity(&self.core.borrow());
        if let (Some(trace), false) = (self.trace, self.asked[2]) {
            trace.borrow_mut().input.hub_angular_velocity = arr(&velocity);
        }
        self.asked[2] = true;
        velocity
    }

    fn add_local_force_and_torque(&mut self, force: &Vec3f, torque: &Vec3f, drive_torque: &Vec3f) {
        if let Some(trace) = self.trace {
            trace.borrow_mut().calls.push(rig::Call {
                kind: 3,
                a: arr(force),
                b: arr(torque),
                c: arr(drive_torque),
                ..rig::Call::default()
            });
        }
        self.suspension.add_local_force_and_torque(&mut self.core.borrow_mut(), force, torque, drive_torque);
    }
}

/// The tyre's view of the car.
struct CarPort<'a, 'b> {
    core: &'a RefCell<&'b mut PhysicsCore>,
    body: RigidBody,
    env: &'a ChassisEnvironment,
    torque_mode_ex: TorqueModeEx,
    sleeping: bool,
    speed: f32,
    /// The surface-drag call on the car body, for the trace.
    calls: Vec<rig::Call>,
}

impl TyreCar for CarPort<'_, '_> {
    fn torque_mode_ex(&self) -> TorqueModeEx {
        self.torque_mode_ex
    }
    fn is_sleeping(&self) -> bool {
        self.sleeping
    }
    fn get_speed(&self) -> f32 {
        self.speed
    }
    fn dynamic_grip_level(&self) -> f32 {
        self.env.dynamic_grip_level
    }
    fn tyre_consumption_rate(&self) -> f32 {
        self.env.tyre_consumption_rate
    }
    fn mechanical_damage_rate(&self) -> f32 {
        self.env.mechanical_damage_rate
    }
    fn ambient_temperature(&self) -> f32 {
        self.env.ambient_temperature
    }
    fn road_temperature(&self) -> f32 {
        self.env.road_temperature
    }
    fn allow_tyre_blankets(&self) -> bool {
        self.env.allow_tyre_blankets
    }
    fn body_get_velocity(&mut self) -> Vec3f {
        self.core.borrow().get_velocity(self.body)
    }
    fn body_get_mass(&mut self) -> f32 {
        self.core.borrow().get_mass(self.body)
    }
    fn body_add_force_at_local_pos(&mut self, f: &Vec3f, p: &Vec3f) {
        self.calls.push(rig::Call { kind: 4, a: arr(f), b: arr(p), ..rig::Call::default() });
        let mut core = self.core.borrow_mut();
        let previous = core.source;
        core.source = ForceSource::Surface;
        core.add_force_at_local_pos(self.body, f, p);
        core.source = previous;
    }
}

/// The road, with what it answered kept for the trace.
struct GroundPort<'a> {
    ground: &'a dyn RayTrackCollisionProvider,
    trace: Option<&'a RefCell<TyreTrace>>,
}

impl RayTrackCollisionProvider for GroundPort<'_> {
    fn ray_cast(&self, org: &Vec3f, dir: &Vec3f, length: f32) -> Option<RayCastResult> {
        let hit = self.ground.ray_cast(org, dir, length);
        if let Some(trace) = self.trace {
            let input = &mut trace.borrow_mut().input;
            input.has_hit = hit.is_some();
            if let Some(hit) = &hit {
                input.ground_y = hit.pos.y;
                input.ground_normal = arr(&hit.normal);
                let s = hit.surface_def;
                input.grip_mod = s.grip_mod;
                input.dirt_additive_k = s.dirt_additive_k;
                input.sin_height = s.sin_height;
                input.sin_length = s.sin_length;
                input.damping = s.damping;
                input.granularity = s.granularity;
            }
        }
        hit
    }
}

impl RollingChassis {
    /// `Car::Car` @ 0x14026bf00, the ported part, in the constructor's order: car body and
    /// fuel tank, `Car::initCarData`, per wheel the suspension and its tyre, heave springs,
    /// steering, anti-roll bars, the first body-mass refresh, the setup items.
    ///
    /// `data_path` is the car's data folder (unpacked ini files). `rand_seed` is the state of
    /// the C runtime's `rand()` when the car is built (`srand(seed)`): each suspension draws
    /// one number for the side its steering rod bends to when damaged. `physics_time` is
    /// `PhysicsEngine::physicsTime` at that moment, ms.
    pub fn new(
        data_path: &Path,
        env: ChassisEnvironment,
        ground: Box<dyn RayTrackCollisionProvider>,
        rand_seed: u32,
        physics_time: f64,
    ) -> Result<RollingChassis, String> {
        let mut core = PhysicsCore::new();
        let body = core.create_rigid_body();
        let fuel_tank_body = core.create_rigid_body();

        // Car::initCarData @ 0x140272b30
        let ini = IniReader::load(&data_path.join("car.ini"))?;
        let mass = ini.get_float("BASIC", "TOTALMASS")?;
        if ini.has_section("EXPLICIT_INERTIA") {
            return Err(format!(
                "{}: [EXPLICIT_INERTIA] is not ported (no car in cardata has it)",
                data_path.join("car.ini").display()
            ));
        }
        let inertia = ini.get_float3("BASIC", "INERTIA")?;
        let body_inertia = Vec3f::new(inertia[0], inertia[1], inertia[2]);
        // `Car::updateBodyMass` takes its "explicit inertia" path whenever the box is (0,0,0)
        // (three `ucomiss` + `jne`: also for NaNs), with or without that section
        if !ordered_nonzero(body_inertia.x) && !ordered_nonzero(body_inertia.y) && !ordered_nonzero(body_inertia.z) {
            return Err(format!(
                "{}: [BASIC] INERTIA is missing or all zero: the game then treats the car like one with \
                 [EXPLICIT_INERTIA], which is not ported",
                data_path.join("car.ini").display()
            ));
        }
        core.set_mass_box(body, mass, body_inertia.x, body_inertia.y, body_inertia.z);
        let mut fuel_kg = 0.74;
        if ini.has_section("FUEL_EXT") {
            fuel_kg = ini.get_float("FUEL_EXT", "KG_PER_LITER")?;
        }
        let ff_mult = ini.get_float("CONTROLS", "FFMULT")? * 0.001;
        let steer_lock = ini.get_float("CONTROLS", "STEER_LOCK")?;
        let steer_ratio = ini.get_float("CONTROLS", "STEER_RATIO")?;
        let mut steer_linear_ratio = ini.get_float("CONTROLS", "LINEAR_STEER_ROD_RATIO")?;
        if !ordered_nonzero(steer_linear_ratio) {
            steer_linear_ratio = 0.003;
        }
        let mut steer_assist = ini.get_float("CONTROLS", "STEER_ASSIST")?;
        if !ordered_nonzero(steer_assist) {
            steer_assist = 1.0;
        }
        let fuel_consumption_k = ini.get_float("FUEL", "CONSUMPTION")? as f64;
        let mut fuel = ini.get_float("FUEL", "FUEL")? as f64;
        let mut max_fuel = ini.get_float("FUEL", "MAX_FUEL")? as f64;
        if max_fuel == 0.0 {
            max_fuel = 30.0;
        }
        let requested_fuel;
        if fuel == 0.0 {
            fuel = 30.0;
            requested_fuel = 30.0;
        } else {
            requested_fuel = fuel as f32;
        }
        let tank = ini.get_float3("FUELTANK", "POSITION")?;
        let fuel_tank_pos = Vec3f::new(tank[0], tank[1], tank[2]);
        core.set_mass_box(fuel_tank_body, 1.0, 0.5, 0.5, 0.5);
        core.set_position(fuel_tank_body, &fuel_tank_pos);
        let fuel_tank_joint = core.create_fixed_joint(fuel_tank_body, body);

        let mut chassis = RollingChassis {
            core,
            env,
            ground,
            body,
            fuel_tank_body,
            fuel_tank_joint,
            suspensions: Vec::new(),
            tyres: Vec::new(),
            heave_springs: [HeaveSpring::default(); 2],
            antiroll_bars: [AntirollBar::default(); 2],
            steering_system: SteeringSystem { linear_ratio: steer_linear_ratio },
            setup_manager: SetupManager::default(),
            controls: CarControls::default(),
            final_steer_angle_signal: 0.0,
            mass,
            ff_mult,
            acc_g: Vec3f::default(),
            steer_lock,
            steer_ratio,
            steer_assist,
            torque_mode_ex: TorqueModeEx::Original,
            last_velocity: Vec3f::default(),
            sleeping_frames: 0,
            frames_to_sleep: 50,
            mz_current: 0.0,
            last_ff: 0.0,
            last_pure_mz_ff: 0.0,
            last_gyro_ff: 0.0,
            last_steer_position: 0.0,
            flat_spot_phase: 0.0,
            body_inertia,
            fuel,
            fuel_consumption_k,
            max_fuel,
            requested_fuel,
            fuel_kg,
            last_body_mass_update_time: -1.0e8,
            fuel_tank_pos,
            ballast_kg: 0.0,
            speed: 0.0,
            fuel_pressure: 0.0,
            physics_time,
            trace: None,
            data_path: data_path.to_path_buf(),
        };

        // the wheels: suspension, then its tyre
        let suspensions_ini = IniReader::load(&data_path.join("suspensions.ini"))?;
        let mut rand = MsvcRand(rand_seed);
        for index in 0..4 {
            let section = if index < 2 { "FRONT" } else { "REAR" };
            let kind = suspensions_ini.get_string(section, "TYPE");
            if kind != "DWB" {
                return Err(format!(
                    "{}: [{section}] TYPE={kind}: only the double-wishbone suspension (DWB) is ported",
                    data_path.join("suspensions.ini").display()
                ));
            }
            let suspension = VanillaDwb::new(&mut chassis.core, body, data_path, index, rand.next())?;
            chassis.suspensions.push(Box::new(suspension));
            let mut tyre = VanillaTyre::new();
            let (result, _) =
                chassis.with_tyre_ports(index as usize, None, |hub, _ground, car| tyre.init(hub, data_path, index, Some(&*car)));
            result?;
            chassis.tyres.push(tyre);
        }

        // Car::initHeaveSprings @ 0x140273f40 (all four wheels are double wishbone here)
        chassis.heave_springs[0].init(data_path, true)?;
        chassis.heave_springs[1].init(data_path, false)?;
        // Car::buildARBS @ 0x14026f750
        chassis.antiroll_bars[0].k = suspensions_ini.get_float("ARB", "FRONT")?;
        chassis.antiroll_bars[1].k = suspensions_ini.get_float("ARB", "REAR")?;
        for name in ["ctrl_arb_front.ini", "ctrl_arb_rear.ini", "ctrl_4ws.ini"] {
            if data_path.join(name).is_file() {
                return Err(format!("{}: controllers (DynamicController) are not ported", data_path.join(name).display()));
            }
        }
        chassis.sleeping_frames = 0;
        chassis.update_body_mass();
        chassis.setup_manager = SetupManager::init(&chassis, data_path)?;
        Ok(chassis)
    }

    /// Runs `f` with the three things a tyre function needs: its hub, the road and the car.
    fn with_tyre_ports<R>(
        &mut self,
        index: usize,
        trace: Option<&RefCell<TyreTrace>>,
        f: impl FnOnce(&mut dyn Suspension, &dyn RayTrackCollisionProvider, &mut dyn TyreCar) -> R,
    ) -> (R, Vec<rig::Call>) {
        let sleeping = self.sleeping_frames > self.frames_to_sleep;
        let cell = RefCell::new(&mut self.core);
        let mut hub = HubPort { core: &cell, suspension: self.suspensions[index].as_mut(), trace, asked: [false; 3] };
        let mut car = CarPort {
            core: &cell,
            body: self.body,
            env: &self.env,
            torque_mode_ex: self.torque_mode_ex,
            sleeping,
            speed: self.speed,
            calls: Vec::new(),
        };
        let ground = GroundPort { ground: self.ground.as_ref(), trace };
        let result = f(&mut hub, &ground, &mut car);
        (result, car.calls)
    }

    /// `Car::isSleeping` @ 0x1402745e0.
    pub fn is_sleeping(&self) -> bool {
        self.sleeping_frames > self.frames_to_sleep
    }

    /// `Car::calcBodyMass` @ 0x14026fb70.
    pub fn calc_body_mass(&self) -> f32 {
        let mut hubs = 0.0f32;
        for suspension in &self.suspensions {
            hubs += suspension.get_mass(&self.core);
        }
        (self.mass - hubs) + self.ballast_kg
    }

    /// `Car::updateBodyMass` @ 0x140276c70: once per second of physics time the body gets
    /// `TOTALMASS` minus the hubs plus ballast, and the tank the mass of the fuel in it.
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    pub fn update_body_mass(&mut self) {
        if !(self.physics_time - self.last_body_mass_update_time > 1000.0) {
            return;
        }
        let mass = self.calc_body_mass();
        self.core.set_mass_box(self.body, mass, self.body_inertia.x, self.body_inertia.y, self.body_inertia.z);
        let fuel_mass = (self.fuel_kg as f64 * self.fuel) as f32;
        // `comiss` + `cmovbe`: 0.1 unless the fuel weighs more
        let tank = if fuel_mass > 0.1 { fuel_mass } else { 0.1 };
        self.core.set_mass_box(self.fuel_tank_body, tank, 0.5, 0.5, 0.5);
        self.last_body_mass_update_time = self.physics_time;
    }

    /// `Car::getTotalMass` @ 0x140272570.
    pub fn get_total_mass(&self, with_fuel: bool) -> f32 {
        let mut total = self.core.get_mass(self.body);
        for suspension in &self.suspensions {
            total += suspension.get_mass(&self.core);
        }
        if with_fuel {
            total += self.fuel as f32 * self.fuel_kg;
        }
        total
    }

    /// `RaceEngineer::getBaseCarHeight` @ 0x14027ba00: how high the body origin sits above
    /// the road with the wheels at their design position.
    pub fn get_base_car_height(&self) -> f32 {
        let front = -(self.suspensions[0].get_base_position().y - self.tyres[0].data.radius);
        let rear = -(self.suspensions[2].get_base_position().y - self.tyres[2].data.radius);
        if front > rear {
            front
        } else {
            rear
        }
    }

    /// `Car::forceRotation` @ 0x140270040. `heading` is the direction the car's tail points.
    pub fn force_rotation(&mut self, heading: &Vec3f) {
        let h = heading;
        let mut z = h.x - h.y * 0.0;
        let mut x = h.y * 0.0 - h.z;
        let mut y = h.z * 0.0 - h.x * 0.0;
        let length = sqrtf((y * y + x * x) + z * z);
        if ordered_nonzero(length) {
            let inverse = 1.0 / length;
            x *= inverse;
            y *= inverse;
            z *= inverse;
        }
        let matrix = Mat44f { m: [[x, y, z, 0.0], [0.0, 1.0, 0.0, 0.0], [-h.x, -h.y, -h.z, 0.0], [0.0, 0.0, 0.0, 1.0]] };
        self.core.set_rotation(self.body, &matrix);
        self.core.set_rotation(self.fuel_tank_body, &matrix);
        for suspension in &mut self.suspensions {
            suspension.attach(&mut self.core);
        }
        let previous = std::mem::replace(&mut self.core.source, ForceSource::Teleport);
        self.core.stop(self.body);
        self.core.stop(self.fuel_tank_body);
        self.core.source = previous;
    }

    /// `Car::forcePosition` @ 0x14026fe10 (with `Car::reset` @ 0x1402758e0): puts the car on
    /// the road at `pos` (a point on the ground). The parts of the original that belong to
    /// systems not ported yet (`Drivetrain::reset`, `BrakeSystem::reset`, the gear) are the
    /// feed's business.
    pub fn force_position(&mut self, pos: &Vec3f) {
        let mut pos = *pos;
        pos.y += self.get_base_car_height() + 0.01;
        // Car::reset
        self.frames_to_sleep = 50;
        self.fuel = self.requested_fuel as f64;
        let previous = std::mem::replace(&mut self.core.source, ForceSource::Teleport);
        self.core.stop(self.body);
        self.core.set_position(self.body, &pos);
        let tank = self.core.local_to_world(self.body, &self.fuel_tank_pos);
        self.core.set_position(self.fuel_tank_body, &tank);
        for suspension in &mut self.suspensions {
            suspension.stop(&mut self.core);
            suspension.attach(&mut self.core);
        }
        for index in 0..self.tyres.len() {
            let mut tyre = std::mem::take(&mut self.tyres[index]);
            self.with_tyre_ports(index, None, |hub, _ground, car| tyre.reset(hub, Some(&*car)));
            self.tyres[index] = tyre;
        }
        self.core.stop(self.body);
        self.core.stop(self.fuel_tank_body);
        self.core.source = previous;
        self.frames_to_sleep = 50;
    }

    /// `Tyre::setCompound` @ 0x1402834e0 on the four tyres: what the game's setup screen does
    /// with `tyres.ini [COMPOUND_DEFAULT] INDEX` when a session starts.
    pub fn set_compound(&mut self, compound: i32) -> Result<(), String> {
        for index in 0..self.tyres.len() {
            let mut tyre = std::mem::take(&mut self.tyres[index]);
            let (found, _) = self.with_tyre_ports(index, None, |hub, _ground, car| tyre.set_compound(compound, hub, Some(&*car)));
            self.tyres[index] = tyre;
            if !found {
                return Err(format!("compound index {compound} does not exist"));
            }
        }
        Ok(())
    }

    /// The session start of the player's car as the game's setup screen does it with the
    /// default setup: the default tyre compound, and every setup value pushed through the
    /// screen's whole-number spinners (applied by [`SetupManager::step`] in the first step).
    pub fn session_start(&mut self) -> Result<(), String> {
        let tyres = IniReader::load(&self.data_path.join("tyres.ini"))?;
        let compound = tyres.get_int("COMPOUND_DEFAULT", "INDEX")?;
        // an index that does not exist changes nothing in the game (`setCompound` says no and
        // nobody looks)
        let _ = self.set_compound(compound);
        let setup = IniReader::load(&self.data_path.join("setup.ini"))?;
        if setup.ready {
            let mut manager = std::mem::take(&mut self.setup_manager);
            manager.apply_setup_screen_defaults(self, &setup);
            self.setup_manager = manager;
        }
        Ok(())
    }

    /// One physics step: `Car::stepPreCacheValues` @ 0x1402768c0, `Car::step` @ 0x140275da0
    /// (with `Car::stepComponents` @ 0x1402764d0) and the rigid-body step
    /// (`PhysicsCore::step` @ 0x1402cd690), as `PhysicsEngine::step` @ 0x140264760 runs them
    /// for one car.
    ///
    /// `physics_time` is the engine's clock for this step, ms (it advances by `dt * 1000`).
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    pub fn step(&mut self, dt: f32, physics_time: f64, feed: &mut dyn ChassisFeed) {
        self.physics_time = physics_time;
        if let Some(tape) = &mut self.core.tape {
            tape.clear();
        }
        if let Some(trace) = &mut self.trace {
            trace.tyres.clear();
            trace.pre.clear();
        }

        // Car::stepPreCacheValues
        let v = self.core.get_velocity(self.body);
        let squared = (v.x * v.x + v.y * v.y) + v.z * v.z;
        self.speed = if ordered_nonzero(squared) { sqrtf(squared) } else { 0.0 };

        // --- Car::step ---------------------------------------------------------------------
        // the first car softens its joints' error correction below 1 m/s
        if self.env.is_first_car {
            let v = self.core.get_velocity(self.body);
            let squared = (v.x * v.x + v.y * v.y) + v.z * v.z;
            let erp;
            if squared >= 1.0 {
                erp = 0.3;
                for suspension in &mut self.suspensions {
                    let cfm = suspension.base().base_cfm;
                    suspension.set_erp_cfm(&mut self.core, 0.3, cfm);
                }
            } else {
                erp = 0.9;
                for suspension in &mut self.suspensions {
                    suspension.set_erp_cfm(&mut self.core, 0.9, 1.0e-7);
                }
            }
            self.core.joint_set_erp_cfm(self.fuel_tank_joint.id, erp, -1.0);
        }

        feed.poll_controls(self);

        // fuel burn
        let engine = feed.engine(self);
        let boost = if engine.turbo_boost >= 0.0 { engine.turbo_boost as f64 } else { 0.0 };
        let burnt = engine.rpm.abs() * dt * engine.gas_usage;
        self.fuel -= burnt as f64 * (boost + 1.0) * self.fuel_consumption_k * 0.001 * self.env.fuel_consumption_rate as f64;
        if self.fuel > 0.0 {
            self.fuel_pressure = 1.0;
        } else {
            self.fuel = 0.0;
            self.fuel_pressure = 0.0;
        }
        self.update_body_mass();

        // the steering wheel as a road-wheel angle
        let mut signal = self.steer_lock * self.controls.steer / self.steer_ratio;
        if fdtest_inf_or_nan(signal) {
            signal = 0.0;
        }
        self.final_steer_angle_signal = signal;

        let mut all_loaded = true;
        for tyre in &self.tyres {
            if 0.0 >= tyre.status.load {
                all_loaded = false;
                break;
            }
        }
        feed.autoclutch(self);

        // the sleeping rule
        let mut asleep = false;
        if self.speed >= 0.5 {
            self.sleeping_frames = 0;
        } else {
            let w = self.core.get_angular_velocity(self.body);
            if (w.x * w.x + w.y * w.y) + w.z * w.z >= 1.0 {
                self.sleeping_frames = 0;
            } else {
                let driving = !(0.01 >= self.controls.gas) && !(0.01 >= self.controls.clutch) && feed.current_gear(self) != 1;
                if !driving && all_loaded {
                    self.sleeping_frames = self.sleeping_frames.wrapping_add(1);
                } else {
                    self.sleeping_frames = 0;
                }
                asleep = self.sleeping_frames > self.frames_to_sleep;
            }
        }
        if asleep {
            let previous = std::mem::replace(&mut self.core.source, ForceSource::Sleep);
            self.core.stop(self.body);
            self.core.stop(self.fuel_tank_body);
            self.core.source = previous;
        }

        // body acceleration in g, body axes
        let v = self.core.get_velocity(self.body);
        let inverse_dt = 1.0 / dt;
        let acc = Vec3f::new(
            ((v.x - self.last_velocity.x) * inverse_dt) * 0.101_978_384,
            ((v.y - self.last_velocity.y) * inverse_dt) * 0.101_978_384,
            ((v.z - self.last_velocity.z) * inverse_dt) * 0.101_978_384,
        );
        self.last_velocity = v;
        self.acc_g = self.core.world_to_local_normal(self.body, &acc);

        // --- Car::stepComponents -----------------------------------------------------------
        // 1, 2: brakes, EDL
        feed.brakes(self);
        // 3: suspensions
        for suspension in &mut self.suspensions {
            suspension.step(&mut self.core, dt);
        }
        // 4: tyres; force feedback right after the fourth
        for index in 0..self.tyres.len() {
            self.step_tyre(index, dt);
        }
        self.on_tyres_step_completed();
        // 5: heave springs
        for (axle, first) in [(0usize, 0usize), (1, 2)] {
            if ordered_nonzero(self.heave_springs[axle].k) {
                let (left, right) = self.suspensions[first..first + 2].split_at_mut(1);
                self.heave_springs[axle].step(&mut self.core, self.body, left[0].as_mut(), right[0].as_mut(), dt);
            }
        }
        // 6 to 9: DRS, aero, KERS, ERS
        feed.aero(self);
        // 10: steering: the rods move now, the solver turns the wheels at the end of this
        // step, the tyres see it in the next
        let offset = -(self.final_steer_angle_signal * self.steering_system.linear_ratio);
        self.suspensions[0].set_steer_length_offset(&mut self.core, offset);
        let offset = -(self.final_steer_angle_signal * self.steering_system.linear_ratio);
        self.suspensions[1].set_steer_length_offset(&mut self.core, offset);
        // 11 to 14: auto-blip, auto-shifter, gear changer, drivetrain
        feed.drivetrain(self);
        // 15: anti-roll bars
        for (axle, first) in [(0usize, 0usize), (1, 2)] {
            let (left, right) = self.suspensions[first..first + 2].split_at_mut(1);
            self.antiroll_bars[axle].step(&mut self.core, self.body, left[0].as_mut(), right[0].as_mut(), dt);
        }
        // 16 to 18: ABS, traction control, speed limiter
        feed.aids(self);
        // 20: setup values that changed reach the car here
        let mut manager = std::mem::take(&mut self.setup_manager);
        manager.step(self);
        self.setup_manager = manager;
        // 21 to 29: telemetry, lap timing, stability control
        feed.stability(self);

        // --- PhysicsCore::step ---------------------------------------------------------------
        let pre: Vec<BodyTrace> = match self.trace {
            Some(_) => self.core.bodies().map(|body| self.body_trace(body)).collect(),
            None => Vec::new(),
        };
        if let Some(trace) = &mut self.trace {
            trace.pre = pre;
        }
        self.core.step(dt);
    }

    /// The state of a body as the comparison with the recordings wants it.
    pub fn body_trace(&self, body: RigidBody) -> BodyTrace {
        let b = self.core.world.body(body.id);
        let mut r = [0.0; 9];
        for row in 0..3 {
            for col in 0..3 {
                r[3 * row + col] = b.r[4 * row + col];
            }
        }
        BodyTrace {
            mass: b.mass.mass,
            inertia: [b.mass.i[0], b.mass.i[5], b.mass.i[10]],
            pos: [b.pos[0], b.pos[1], b.pos[2]],
            q: b.q,
            r,
            lvel: [b.lvel[0], b.lvel[1], b.lvel[2]],
            avel: [b.avel[0], b.avel[1], b.avel[2]],
            facc: [b.facc[0], b.facc[1], b.facc[2]],
            tacc: [b.tacc[0], b.tacc[1], b.tacc[2]],
        }
    }

    /// `Tyre::step` @ 0x140283800 of one wheel, with its hub, the road and the car.
    fn step_tyre(&mut self, index: usize, dt: f32) {
        let mut tyre = std::mem::take(&mut self.tyres[index]);
        let trace = self.trace.is_some().then(|| {
            let body_velocity = self.core.get_velocity(self.body);
            RefCell::new(TyreTrace {
                input: rig::StepInput {
                    hub_matrix: [0.0; 16],
                    hub_velocity: [0.0; 3],
                    hub_angular_velocity: [0.0; 3],
                    brake_torque: tyre.inputs.brake_torque,
                    hand_brake_torque: tyre.inputs.hand_brake_torque,
                    electric_torque: tyre.inputs.electric_torque,
                    abs_override: tyre.abs_override,
                    ai_mult: tyre.ai_mult,
                    driven: tyre.driven,
                    set_angular_velocity: Some(tyre.status.angular_velocity),
                    set_blankets: Some(tyre.tyre_blankets_on),
                    ext_active: tyre.external_inputs.is_active,
                    ext_load: tyre.external_inputs.load,
                    ext_slip_angle: tyre.external_inputs.slip_angle,
                    ext_slip_ratio: tyre.external_inputs.slip_ratio,
                    has_hit: false,
                    ground_y: 0.0,
                    ground_normal: [0.0; 3],
                    grip_mod: 1.0,
                    dirt_additive_k: 0.0,
                    sin_height: 0.0,
                    sin_length: 0.0,
                    damping: 0.0,
                    granularity: 0.0,
                    has_car: true,
                    torque_mode: match self.torque_mode_ex {
                        TorqueModeEx::Original => 0,
                        TorqueModeEx::ReactionTorques => 1,
                        TorqueModeEx::DriveTorques => 2,
                        TorqueModeEx::Other => 3,
                    },
                    car_speed: self.speed,
                    car_sleeping: self.is_sleeping(),
                    dynamic_grip_level: self.env.dynamic_grip_level,
                    tyre_consumption_rate: self.env.tyre_consumption_rate,
                    mechanical_damage_rate: self.env.mechanical_damage_rate,
                    ambient_temperature: self.env.ambient_temperature,
                    road_temperature: self.env.road_temperature,
                    allow_tyre_blankets: self.env.allow_tyre_blankets,
                    body_velocity: arr(&body_velocity),
                    body_mass: self.core.get_mass(self.body),
                },
                wheel_rotation_in: tyre.local_wheel_rotation,
                calls: Vec::new(),
                output: Vec::new(),
            })
        });
        let previous = std::mem::replace(&mut self.core.source, ForceSource::Tyre);
        let ((), mut body_calls) =
            self.with_tyre_ports(index, trace.as_ref(), |hub, ground, car| tyre.step(dt, hub, Some(ground), Some(car)));
        self.core.source = previous;
        if let Some(trace) = trace {
            let mut trace = trace.into_inner();
            // the body call is recorded after the tyre's own hub calls, as in the rig
            trace.calls.append(&mut body_calls);
            trace.output = rig::snapshot(&tyre, &trace.calls);
            self.trace.as_mut().unwrap().tyres.push(trace);
        }
        self.tyres[index] = tyre;
    }

    /// `Car::getSteerFF` @ 0x140272180: the steering torque of the two front wheels as a
    /// force-feedback level.
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    pub fn get_steer_ff(&mut self) -> f32 {
        let steer_torque = self.suspensions[1].get_steer_torque() + self.suspensions[0].get_steer_torque();
        let position = self.steer_lock * self.controls.steer;
        let ratio = self.steer_ratio.abs();
        let mut gyro = position - self.last_steer_position;
        self.mz_current = (self.mz_current - steer_torque) * self.env.ff_filter + steer_torque;
        gyro = gyro * 333.333_34;
        gyro = gyro / ratio;
        let w0 = self.tyres[0].status.angular_velocity;
        let w1 = self.tyres[1].status.angular_velocity;
        self.last_pure_mz_ff = self.mz_current * 1.4;
        gyro = gyro * ((w1.abs() + w0.abs()) * self.tyres[0].data.angular_inertia);
        gyro = gyro * self.env.gyro_wheel_gain;
        self.last_steer_position = position;
        let mut ff = -(self.mz_current + gyro) * 1.4;

        // flat-spot shake (flat spots and blisters are doubles)
        let s = |i: usize| self.tyres[i].status;
        let mut flat_spot = if s(0).flat_spot > s(1).flat_spot { s(0).flat_spot } else { s(1).flat_spot };
        let front = if s(0).blister > s(1).blister { s(0).blister } else { s(1).blister };
        let rear = if s(2).blister > s(3).blister { s(2).blister } else { s(3).blister };
        let blister = (if front > rear { front } else { rear }) * 0.003;
        if !(flat_spot > blister) {
            flat_spot = blister;
        }
        let rotation = w0.abs() + w1.abs();
        self.flat_spot_phase = rotation * 0.003 + self.flat_spot_phase;
        if rotation > 7.0 {
            let mut shake = ks_saw_tooth_wave(self.flat_spot_phase, 12.566_36);
            shake = shake * flat_spot as f32;
            shake = shake * self.env.flat_spot_ff_gain;
            shake = shake * (s(1).load + s(0).load);
            shake = shake * 0.5;
            shake = shake + 1.0;
            ff = ff + shake;
        }

        // the car's gain and the steer-assist curve
        if self.steer_assist != 1.0 && !self.steer_assist.is_nan() {
            let level = powf((ff * self.ff_mult).abs(), self.steer_assist);
            ff = if ff > 0.0 {
                1.0 * level
            } else if ff >= 0.0 {
                0.0 * level
            } else {
                -1.0 * level
            };
        } else {
            ff = ff * self.ff_mult;
        }
        self.last_gyro_ff = gyro * 1.4;
        ff = -ff;

        // optional lightening when the front tyres slide
        if self.env.use_fake_understeer_ff {
            let slide = (s(1).nd_slip + s(0).nd_slip) * 0.5 - 1.0;
            let clamped = |x: f32| {
                if x > 1.0 {
                    1.0
                } else if !(x >= 0.0) {
                    0.0
                } else {
                    x
                }
            };
            let mut divisor = clamped(slide) * 5.5 + 1.0;
            if divisor > 2.5 {
                divisor = 2.5;
            }
            ff = ff / divisor;
        }
        ff
    }

    /// `Car::onTyresStepCompleted` @ 0x140274cd0: runs at the end of the fourth tyre's step.
    /// The call to the device (`sendFF`) is not ported; `last_ff` is what it would be sent.
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    pub fn on_tyres_step_completed(&mut self) {
        self.last_ff = self.get_steer_ff();
        let speed_kmh = self.env.mz_low_speed_reduction_speed_kmh;
        if ordered_nonzero(speed_kmh) {
            let mut k = (self.speed * 3.6) / speed_kmh;
            if k > 1.0 {
                k = 1.0;
            } else if !(k >= 0.0) {
                k = 0.0;
            }
            let min_value = self.env.mz_low_speed_reduction_min_value;
            self.last_ff = ((1.0 - min_value) * k + min_value) * self.last_ff;
        }
    }
}
