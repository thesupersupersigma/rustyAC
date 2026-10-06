//! The "feed": everything that crosses into the rolling chassis from the systems that are
//! not ported yet (driver, engine, clutch, gearbox, differential, brakes, aero, aids).
//!
//! [`RollingChassis::step`](super::RollingChassis::step) calls the hooks of a [`ChassisFeed`]
//! at the places where `Car::step` / `Car::stepComponents` run the real systems. Today the
//! hooks are filled from a recording of the game ([`super::replay::RecordedFeed`]); a later
//! task replaces a hook by the ported system without touching the chassis.
//!
//! Only inputs travel through here. Nothing the chassis computes itself (body states,
//! suspension forces, tyre forces, steering, force feedback) is ever fed.

use super::RollingChassis;

/// The part of AC's `CarControls` (`Car::controls`, 0x34 bytes) the chassis reads.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CarControls {
    /// `gas`, 0..1
    pub gas: f32,
    /// `brake`, 0..1
    pub brake: f32,
    /// `steer`, -1..1, positive = right
    pub steer: f32,
    /// `clutch`, 1 = pedal up (engaged)
    pub clutch: f32,
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

    /// What the fuel burn reads from the engine (`Car::step`, right after `pollControls`).
    fn engine(&mut self, chassis: &RollingChassis) -> EngineFeed;

    /// `Autoclutch::step` @ 0x1402b9590: may rewrite `controls.clutch`. Called where
    /// `Car::step` calls it, before the sleeping rule reads the clutch.
    fn autoclutch(&mut self, chassis: &mut RollingChassis);

    /// `Drivetrain::currentGear` as the sleeping rule reads it (1 = neutral).
    fn current_gear(&mut self, chassis: &RollingChassis) -> i32;

    /// `Car::stepComponents` positions 1 and 2, `BrakeSystem::step` @ 0x14028e640 and
    /// `EDL::step` @ 0x1402bb460: the brake torques in `Tyre::inputs`. Runs before the
    /// suspensions and the tyres.
    fn brakes(&mut self, chassis: &mut RollingChassis);

    /// Positions 6 to 9, `DRS::step`, `AeroMap::step`, `Kers::step`, `ERS::step`: the wings
    /// push on the car body. Runs after the heave springs.
    fn aero(&mut self, chassis: &mut RollingChassis);

    /// Positions 11 to 14, `AutoBlip::step`, `AutoShifter::step`, `GearChanger::step`,
    /// `Drivetrain::step` @ 0x14026b130: the speed (and spin matrix) of the driven wheels.
    /// Runs after the steering, before the anti-roll bars.
    fn drivetrain(&mut self, chassis: &mut RollingChassis);

    /// Positions 16 to 29, `ABS::step`, `TractionControl::step`, `SpeedLimiter::step`, …,
    /// `StabilityControl::step`: the aids. Runs after the anti-roll bars.
    fn aids(&mut self, chassis: &mut RollingChassis);
}
