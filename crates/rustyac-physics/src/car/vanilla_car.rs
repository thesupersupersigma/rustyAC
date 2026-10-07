// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni). Not covered by rustyAC's licenses — see LICENSING.md.

//! [`VanillaCar`]: the whole car. A [`RollingChassis`] with every slot filled (aero, brakes,
//! engine and drivetrain, aids, telemetry writer) and the driver's device, stepped exactly as
//! AC's `PhysicsEngine::step` steps one `Car`. Nothing is fed any more: what goes in is what
//! the driver's device reports ([`ControlsProvider`]) and what the session and the track
//! decide ([`ChassisEnvironment`], the road under the wheels).

use std::path::Path;

use super::chassis::{ChassisEnvironment, RollingChassis};
use super::feed::{CarControls, ChassisFeed, EngineFeed, VibrationDef};
use super::telemetry::PhysicsPage;
use crate::tyre::RayTrackCollisionProvider;

/// `CarControlsInput`: what `Car::pollControls` tells the device (a wheel uses the lock for
/// its soft stops, the keyboard steers less at speed).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CarControlsInput {
    /// `steerLock`: the car's steering-wheel lock, degrees
    pub steer_lock: f32,
    /// `speed`: the car's speed at the start of the step, m/s
    pub speed: f32,
}

/// AC's `ICarControlsProvider`, the driver's device (a wheel, a pad, the keyboard, an AI
/// driver), as far as `Car` calls it.
///
/// A device writes **its own** fields of `Car::controls` and leaves the others as they are:
/// the game's keyboard device, for one, never touches the clutch or the H-shifter request.
/// Whatever smoothing the game applies to raw input (steering speed, dead zones, gamma,
/// the keyboard's throttle ramp) happens inside the device classes, before this point;
/// `Car::pollControls` itself passes the values through unchanged.
pub trait ControlsProvider {
    /// `acquireControls` (+0x08): once per step, unless the car's controls are locked or it
    /// is black-flagged.
    fn acquire_controls(&mut self, controls: &mut CarControls, dt: f32, input: &CarControlsInput);

    /// `getAction` (+0x10): is a button held? `Car::step` asks for 4, the headlight switch.
    fn get_action(&mut self, _action: i32) -> bool {
        false
    }

    /// `sendFF` (+0x18): the steering force, the damper level, the user's gain.
    fn send_ff(&mut self, _ff: f32, _damper: f32, _user_gain: f32) {}

    /// `getFFGlobalGain` (+0x28): the device's own force-feedback gain; 1 for everything but
    /// a wheel.
    fn get_ff_global_gain(&mut self) -> f32 {
        1.0
    }

    /// `setVibrations` (+0x48)
    fn set_vibrations(&mut self, _def: &VibrationDef) {}

    /// `setEngineRPM` (+0x50): the revs and two marks at 75 % and 95 % of the limiter.
    fn set_engine_rpm(&mut self, _rpm: f32, _low: f32, _high: f32) {}
}

/// A device that reports whatever was last put into it: a scripted driver, a replayed
/// recording, a test. Like the game's devices it writes only what it has: pedals, wheel,
/// paddles, handbrake, H-shifter lever and the DRS button.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ScriptedDevice {
    /// What the device reports. `kers` is not written (the scripted device has no such button).
    pub controls: CarControls,
    /// The headlight switch (`getAction(4)`).
    pub headlights: bool,
    /// What the car sent back in the last step.
    pub last_ff: f32,
    pub last_damper: f32,
    pub last_vibrations: VibrationDef,
    pub last_engine_rpm: [f32; 3],
    /// How often the car asked for the controls.
    pub polls: u64,
}

impl ControlsProvider for ScriptedDevice {
    fn acquire_controls(&mut self, controls: &mut CarControls, _dt: f32, _input: &CarControlsInput) {
        let c = &self.controls;
        controls.gear_up = c.gear_up;
        controls.gear_dn = c.gear_dn;
        controls.drs = c.drs;
        controls.hand_brake = c.hand_brake;
        controls.requested_gear_index = c.requested_gear_index;
        controls.gas = c.gas;
        controls.brake = c.brake;
        controls.steer = c.steer;
        controls.clutch = c.clutch;
        self.polls += 1;
    }

    fn get_action(&mut self, action: i32) -> bool {
        action == 4 && self.headlights
    }

    fn send_ff(&mut self, ff: f32, damper: f32, _user_gain: f32) {
        self.last_ff = ff;
        self.last_damper = damper;
    }

    fn set_vibrations(&mut self, def: &VibrationDef) {
        self.last_vibrations = *def;
    }

    fn set_engine_rpm(&mut self, rpm: f32, low: f32, high: f32) {
        self.last_engine_rpm = [rpm, low, high];
    }
}

/// The feed of a whole car: only the device is left. Every other hook belongs to a system
/// the car has itself, so being asked is a bug.
struct DeviceFeed<'a> {
    device: &'a mut dyn ControlsProvider,
    dt: f32,
}

impl ChassisFeed for DeviceFeed<'_> {
    fn poll_controls(&mut self, chassis: &mut RollingChassis) {
        let input = CarControlsInput { steer_lock: chassis.steer_lock, speed: chassis.speed };
        self.device.acquire_controls(&mut chassis.controls, self.dt, &input);
    }

    fn get_action(&mut self, action: i32) -> bool {
        self.device.get_action(action)
    }

    fn send_ff(&mut self, ff: f32, damper: f32, user_gain: f32) {
        self.device.send_ff(ff, damper, user_gain);
    }

    fn get_ff_global_gain(&mut self) -> f32 {
        self.device.get_ff_global_gain()
    }

    fn set_vibrations(&mut self, def: &VibrationDef) {
        self.device.set_vibrations(def);
    }

    fn set_engine_rpm(&mut self, rpm: f32, low: f32, high: f32) {
        self.device.set_engine_rpm(rpm, low, high);
    }

    fn engine(&mut self, _chassis: &RollingChassis) -> EngineFeed {
        unreachable!("a whole car has its own engine")
    }

    fn autoclutch(&mut self, _chassis: &mut RollingChassis) {
        unreachable!("a whole car has its own automatic clutch")
    }

    fn current_gear(&mut self, _chassis: &RollingChassis) -> i32 {
        unreachable!("a whole car has its own gearbox")
    }

    fn brakes(&mut self, _chassis: &mut RollingChassis) {
        unreachable!("a whole car has its own brakes")
    }

    fn edl(&mut self, _chassis: &mut RollingChassis) {
        unreachable!("a whole car has its own aids")
    }

    fn aero(&mut self, _chassis: &mut RollingChassis) {
        unreachable!("a whole car has its own wings")
    }

    fn drivetrain(&mut self, _chassis: &mut RollingChassis) {
        unreachable!("a whole car has its own drivetrain")
    }

    fn aids(&mut self, _chassis: &mut RollingChassis) {
        unreachable!("a whole car has its own aids")
    }

    fn stability(&mut self, _chassis: &mut RollingChassis) {
        unreachable!("a whole car has its own aids")
    }
}

/// One whole car with its driver's device.
pub struct VanillaCar<P: ControlsProvider> {
    /// AC's `Car`.
    pub car: RollingChassis,
    /// `Car::controlsProvider`
    pub device: P,
}

impl<P: ControlsProvider> VanillaCar<P> {
    /// `Car::Car` @ 0x14026bf00 with everything this port has: the rolling chassis, then
    /// aero, brakes, engine and drivetrain with the shift helpers, the aids, and a telemetry
    /// writer. Cars with systems that are not ported (four-wheel drive, KERS / ERS, rear
    /// steering, strut or axle suspensions) are refused with a message.
    ///
    /// `rand_seed` is the state of the C runtime's `rand()` and `physics_time` the engine's
    /// clock (ms) when the car is built; see [`RollingChassis::new`]. The car still has to be
    /// put on the road ([`RollingChassis::force_rotation`], [`RollingChassis::force_position`])
    /// and given its session start ([`RollingChassis::session_start`]).
    pub fn new(
        data_path: &Path,
        env: ChassisEnvironment,
        ground: Box<dyn RayTrackCollisionProvider>,
        rand_seed: u32,
        physics_time: f64,
        device: P,
    ) -> Result<VanillaCar<P>, String> {
        let mut car = RollingChassis::new(data_path, env, ground, rand_seed, physics_time)?;
        car.install_aero()?;
        car.install_brakes()?;
        car.install_drivetrain()?;
        car.install_aids()?;
        car.install_telemetry()?;
        Ok(VanillaCar { car, device })
    }

    /// Wraps a chassis that already has every system.
    pub fn from_chassis(car: RollingChassis, device: P) -> Result<VanillaCar<P>, String> {
        for (present, name) in [
            (car.aero.is_some(), "aero"),
            (car.brake_system.is_some(), "brakes"),
            (car.drivetrain.is_some(), "drivetrain"),
            (car.aids.is_some(), "aids"),
        ] {
            if !present {
                return Err(format!("the chassis has no {name}: not a whole car"));
            }
        }
        Ok(VanillaCar { car, device })
    }

    /// One physics step as `PhysicsEngine::step` @ 0x140264760 runs it for one car:
    /// `stepWind`, `Car::stepPreCacheValues`, `Car::step`, the rigid-body step,
    /// `Car::postStep`, and what the game's physics thread does right after (the telemetry
    /// page). `physics_time` is the clock of this step, ms; it advances by `dt * 1000`.
    ///
    /// What the game's main thread queues for the physics thread (a click of the cockpit
    /// brake bias, a setup change, a lock of the controls, a black flag) runs at the start of
    /// the step, when the clock already shows the new step's time: hand such a change to
    /// [`RollingChassis::queue`] between two calls. (A change that does not read the clock can
    /// as well be made directly.)
    pub fn step(&mut self, dt: f32, physics_time: f64) {
        self.car.env.step_wind(physics_time);
        let mut feed = DeviceFeed { device: &mut self.device, dt };
        self.car.step(dt, physics_time, &mut feed);
    }

    /// The `acpmf_physics` page of the step that just ran.
    pub fn physics_page(&self) -> Option<&PhysicsPage> {
        self.car.physics_page.as_ref()
    }
}
