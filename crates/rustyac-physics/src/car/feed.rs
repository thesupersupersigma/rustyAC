// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni). Not covered by rustyAC's licenses — see LICENSING.md.

//! The "feed": everything that crosses into the car from the systems that are not ported yet
//! (the driver, aero, the aids, hybrid systems) and, for a chassis that runs without its own
//! brakes or drivetrain, from those too.
//!
//! [`RollingChassis::step`](super::RollingChassis::step) calls the hooks of a [`ChassisFeed`]
//! at the places where `Car::step` / `Car::stepComponents` run the real systems. In the tests
//! the hooks are filled from a recording of the game ([`super::replay::RecordedFeed`]).
//!
//! Brakes, engine, clutch, gearbox and differential are ported
//! ([`BrakeModel`](super::BrakeModel), [`DrivetrainModel`](super::DrivetrainModel) with its
//! [`EngineModel`](super::EngineModel)). A chassis that has them
//! ([`RollingChassis::install_brakes`](super::RollingChassis::install_brakes),
//! [`RollingChassis::install_drivetrain`](super::RollingChassis::install_drivetrain)) no longer
//! calls the hooks marked "only without ..." below.
//!
//! Only inputs travel through here. Nothing the car computes itself is ever fed.

use super::RollingChassis;

/// The part of AC's `CarControls` (`Car::controls`, 0x34 bytes) the ported systems read.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CarControls {
    /// `gearUp` (+0x00): the up-shift paddle is held
    pub gear_up: bool,
    /// `gearDn` (+0x01)
    pub gear_dn: bool,
    /// `drs` (+0x02): the DRS button
    pub drs: bool,
    /// `kers` (+0x03): the hybrid / push-to-pass button
    pub kers: bool,
    /// `requestedGearIndex` (+0x08): the H-shifter's gear (0 reverse, 1 neutral, 2 first ...),
    /// or -1 for a car driven with paddles
    pub requested_gear_index: i32,
    /// `handBrake` (+0x10), 0..1
    pub hand_brake: f32,
    /// `gas` (+0x24), 0..1
    pub gas: f32,
    /// `brake` (+0x28), 0..1
    pub brake: f32,
    /// `steer` (+0x2c), -1..1, positive = right
    pub steer: f32,
    /// `clutch` (+0x30), 1 = pedal up (engaged)
    pub clutch: f32,
}

impl Default for CarControls {
    /// Everything released; no H-shifter (`requestedGearIndex` -1, as `Car::Car` leaves it).
    fn default() -> CarControls {
        CarControls {
            gear_up: false,
            gear_dn: false,
            drs: false,
            kers: false,
            requested_gear_index: -1,
            hand_brake: 0.0,
            gas: 0.0,
            brake: 0.0,
            steer: 0.0,
            clutch: 0.0,
        }
    }
}

/// AC's `VibrationDef`: what `Car::pollControls` hands to the driver's device every step.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct VibrationDef {
    /// `curbs`: kerb rumble
    pub curbs: f32,
    /// `gforce`
    pub gforce: f32,
    /// `slips`: tyre slip
    pub slips: f32,
    /// `engine`: revs over the limiter's revs, 0..1
    pub engine: f32,
    /// `abs`: the ABS pulsing
    pub abs: f32,
}

/// What the fuel burn at the top of `Car::step` reads from the engine: values the engine
/// left at the end of the previous step.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct EngineFeed {
    /// `Drivetrain::getEngineRPM()`
    pub rpm: f32,
    /// `Engine::gasUsage`: the throttle the engine really used.
    pub gas_usage: f32,
    /// `Engine::status.turboBoost`
    pub turbo_boost: f32,
}

/// The not-yet-ported systems, as hooks in the order `Car::step` runs them.
///
/// Every hook gets the chassis, so a real system can read what it needs (tyre feedback
/// torques, wheel speeds, body velocity, ride heights) and write its outputs where the game
/// writes them (`Tyre::inputs`, wheel speeds, forces on the car body).
pub trait ChassisFeed {
    /// `Car::pollControls` @ 0x140274e70: the driver's device (or the AI) fills
    /// `Car::controls`. Called once per step, before anything else reads the controls.
    fn poll_controls(&mut self, chassis: &mut RollingChassis);

    /// `ICarControlsProvider::getAction` (+0x10): is a button of the device held? `Car::step`
    /// asks for action 4, the headlight switch.
    fn get_action(&mut self, _action: i32) -> bool {
        false
    }

    /// `ICarControlsProvider::sendFF` (+0x18): the force-feedback number, the damper level and
    /// the user's gain, from `Car::onTyresStepCompleted`.
    fn send_ff(&mut self, _ff: f32, _damper: f32, _user_gain: f32) {}

    /// `ICarControlsProvider::getFFGlobalGain` (+0x28): the device's own force-feedback gain
    /// (a wheel's setting; 1 for a keyboard, a pad or an AI driver). `Car::getFinalFF` reads it
    /// for the telemetry.
    fn get_ff_global_gain(&mut self) -> f32 {
        1.0
    }

    /// `ICarControlsProvider::setVibrations` (+0x48), from `Car::pollControls`.
    fn set_vibrations(&mut self, _def: &VibrationDef) {}

    /// `ICarControlsProvider::setEngineRPM` (+0x50): the engine's revs and two marks at 75 %
    /// and 95 % of the limiter (shift lights of a wheel), from `Car::pollControls`.
    fn set_engine_rpm(&mut self, _rpm: f32, _low: f32, _high: f32) {}

    /// Only without a drivetrain: what the fuel burn reads from the engine (`Car::step`,
    /// right after `pollControls`).
    fn engine(&mut self, chassis: &RollingChassis) -> EngineFeed;

    /// Only without a drivetrain: `Autoclutch::step` @ 0x1402b9590 may rewrite
    /// `controls.clutch`. Called where `Car::step` calls it, before the sleeping rule reads
    /// the clutch.
    fn autoclutch(&mut self, chassis: &mut RollingChassis);

    /// Only without a drivetrain: `Drivetrain::currentGear` as the sleeping rule reads it
    /// (1 = neutral).
    fn current_gear(&mut self, chassis: &RollingChassis) -> i32;

    /// Only without a brake system: `Car::stepComponents` position 1, `BrakeSystem::step`
    /// @ 0x14028e640: the brake and handbrake torques in `Tyre::inputs`.
    fn brakes(&mut self, chassis: &mut RollingChassis);

    /// Position 2, `EDL::step` @ 0x1402bb460: the electronic differential lock may add brake
    /// torque to the faster wheel of the driven pair. Runs after the brakes and right before
    /// the suspensions and the tyres, so it is also the last moment for whatever else an
    /// unported system leaves in a tyre (`absOverride`, `aiMult`, `inputs.electricTorque`).
    fn edl(&mut self, chassis: &mut RollingChassis);

    /// Only without an aero model: positions 6 to 9, `DRS::step`, `AeroMap::step`,
    /// `Kers::step`, `ERS::step`: the wings push on the car body. Runs after the heave springs.
    fn aero(&mut self, chassis: &mut RollingChassis);

    /// Only without a drivetrain: positions 11 to 14, `AutoBlip::step`, `AutoShifter::step`,
    /// `GearChanger::step`, `Drivetrain::step` @ 0x14026b130: the speed (and spin matrix) of
    /// the driven wheels. Runs after the steering, before the anti-roll bars.
    fn drivetrain(&mut self, chassis: &mut RollingChassis);

    /// Only without an aids model: positions 16 to 18, `ABS::step`, `TractionControl::step`, `SpeedLimiter::step`: the
    /// aids that act on the next step's brakes and engine (`Tyre::absOverride`,
    /// `Engine::electronicOverride`, `BrakeSystem::electronicOverride`). Runs after the
    /// anti-roll bars, before the setup items are written.
    fn aids(&mut self, chassis: &mut RollingChassis);

    /// Positions 21 to 29, after `SetupManager::step`: telemetry, lap timing and
    /// `StabilityControl::step` @ 0x1402bfa50, which puts a yaw torque on the car body. The
    /// last hook before the rigid-body step.
    fn stability(&mut self, chassis: &mut RollingChassis);
}
