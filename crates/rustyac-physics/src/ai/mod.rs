// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! AC's AI drivers: `AIDriver` (the circuit driver) and `AIDriverDrag`.
//!
//! An AI driver is a car's controls provider. `Car::pollControls` asks it once per physics
//! step ([`AiDriver::acquire`] is `AIDriver::acquireControls` @ 0x14029baa0); it reads its
//! own car, the track's lines and hints, the session, and the other cars as they are at
//! that moment, and it draws from the C runtime's `rand()`.
//!
//! # The other cars and the random numbers
//!
//! The game steps its cars one after the other when it has no thread pool, so the driver
//! of car `n` sees the cars before it already through their `Car::step` of this step and
//! the ones after it not yet, and all drivers draw from one `rand()` stream in that order.
//! [`crate::engine::PhysicsEngine`] does exactly that whenever a car has an AI driver: the
//! cars are stepped in creation order on the calling thread, each one is handed
//! ([`AiStepInput`]) a view of the others taken right before its step and the one random
//! stream, which it hands back. The result therefore cannot depend on the number of worker
//! threads. (With a pool the game's drivers draw from whichever thread's `rand()` they run
//! on and read cars other threads are stepping: it is not repeatable there.)

pub mod awareness;
pub mod car_ext;
pub mod drag;
pub mod driver;
pub mod engineer;
pub mod oracle;

pub use drag::AiDriverDrag;
pub use driver::AiDriver;

use crate::car::RollingChassis;
use crate::session::MsvcRand;
use crate::vecmath::Vec3f;

/// `SessionType`
pub const SESSION_UNDEFINED: i32 = 0;
pub const SESSION_PRACTICE: i32 = 1;
pub const SESSION_QUALIFY: i32 = 2;
pub const SESSION_RACE: i32 = 3;

/// AC's `SessionInfo` (0x20 bytes): `PhysicsEngine::sessionInfo`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SessionInfo {
    /// `type`: 0 undefined (the engine before its first session), 1 practice, 2 qualifying,
    /// 3 race, 4 hot lap, 5 time attack, 6 drift, 7 drag
    pub session_type: i32,
    /// `startTimeMS`: the physics clock at the green light
    pub start_time_ms: f64,
    /// `timeSecs`: the session's length (0 for a session counted in laps)
    pub time_secs: f64,
    /// `laps`
    pub laps: i32,
    /// `index`
    pub index: i32,
}

impl Default for SessionInfo {
    /// A practice session that started at time 0.
    fn default() -> SessionInfo {
        SessionInfo { session_type: SESSION_PRACTICE, start_time_ms: 0.0, time_secs: 0.0, laps: 0, index: 0 }
    }
}

/// What a driver reads of a car of the engine (its own included), taken right before the
/// driver's car is stepped.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CarView {
    /// `car->body->getPosition()`
    pub position: Vec3f,
    /// The body's rotation (ODE's 3x4 matrix), for `worldToLocal` of another car's position
    pub rotation: [f32; 12],
    /// `car->body->getVelocity()`
    pub velocity: Vec3f,
    /// `Car::getSpeed()` (`valueCache.speed`), m/s
    pub speed: f32,
    /// `splineLocatorData`
    pub npos: f32,
    pub lateral_offset: f32,
    pub sides_from_il: [f32; 2],
    pub spline_length: f32,
    pub is_outside_track_limits: bool,
    /// `Car::isInPitLane()`
    pub is_in_pit_lane: bool,
    /// `powerClassIndex`
    pub power_class_index: f32,
}

impl CarView {
    /// The view of `car` as it is now.
    pub fn of(car: &RollingChassis) -> CarView {
        let body = car.core.world.body(car.body.id);
        let mut rotation = [0.0f32; 12];
        rotation.copy_from_slice(&body.r[..12]);
        let d = &car.spline_locator_data;
        CarView {
            position: car.core.get_position(car.body),
            rotation,
            velocity: car.core.get_velocity(car.body),
            speed: car.speed,
            npos: d.npos,
            lateral_offset: d.lateral_offset,
            sides_from_il: d.sides_from_il,
            spline_length: d.spline_length,
            is_outside_track_limits: d.is_outside_track_limits,
            is_in_pit_lane: car.is_in_pit_lane(),
            power_class_index: car.power_class_index,
        }
    }

    /// `body->worldToLocal(p)`: `dBodyGetPosRelPoint`.
    pub fn world_to_local(&self, p: &Vec3f) -> Vec3f {
        let r = &self.rotation;
        let d = [p.x - self.position.x, p.y - self.position.y, p.z - self.position.z];
        // dMultiply1_331: the transposed rotation times the difference
        Vec3f::new(
            r[0] * d[0] + r[4] * d[1] + r[8] * d[2],
            r[1] * d[0] + r[5] * d[1] + r[9] * d[2],
            r[2] * d[0] + r[6] * d[1] + r[10] * d[2],
        )
    }
}

/// What the engine hands a car with an AI driver before its step, and takes back after it.
#[derive(Clone, Debug)]
pub struct AiStepInput {
    /// `PhysicsEngine::cars` as views, in creation order; this car is `cars[me]`.
    pub cars: Vec<CarView>,
    pub me: usize,
    /// The physics thread's `rand()`.
    pub rand: MsvcRand,
    /// `PhysicsEngine::sessionInfo`
    pub session: SessionInfo,
}

impl Default for AiStepInput {
    fn default() -> AiStepInput {
        AiStepInput { cars: Vec::new(), me: 0, rand: MsvcRand(1), session: SessionInfo::default() }
    }
}

impl AiStepInput {
    /// `Car::getEngagement(distance)` @ 0x140270680 of car `of`: the nearest car beside it
    /// on its left (`.0`) and on its right (`.1`), -1 where there is none.
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    pub fn engagement(&self, of: usize, distance: f32) -> (f32, f32) {
        let this = &self.cars[of];
        let (mut left, mut right) = (-1.0f32, -1.0f32);
        for (index, other) in self.cars.iter().enumerate() {
            if index == of || other.is_in_pit_lane {
                continue;
            }
            let rel = this.world_to_local(&other.position);
            let length = crate::math::sqrtf((rel.y * rel.y + rel.x * rel.x) + rel.z * rel.z);
            let mut x = rel.x;
            if length != 0.0 {
                x = rel.x / length;
            }
            let angle = crate::math::asinf(x);
            let sign = if rel.z > 0.0 {
                1.0f32
            } else if rel.z >= 0.0 {
                0.0
            } else {
                -1.0
            };
            let mut back = this.npos - other.npos;
            if back < 0.0 {
                back += 1.0;
            }
            let spline_length = this.spline_length;
            if rel.z.abs() < distance && (spline_length * 0.0 < 15.0 || spline_length * back < 15.0) && ((sign * angle) * 57.29578).abs() > 25.0 {
                if rel.x > 0.0 {
                    if left < 0.0 || rel.x < left {
                        left = rel.x;
                    }
                } else if right < 0.0 || -rel.x < right {
                    right = -rel.x;
                }
            }
        }
        (left, right)
    }
}

/// `ksClamp`-style saturation as the compiler wrote it: `hi` for a NaN.
#[inline]
pub(crate) fn sat(x: f32, lo: f32, hi: f32) -> f32 {
    if x <= hi {
        if x < lo {
            lo
        } else {
            x
        }
    } else {
        hi
    }
}

/// The game's factor from `rand()` to 0..1 (0x38000100).
pub(crate) const RAND_UNIT: f32 = f32::from_bits(0x3800_0100);

/// `ksSpeedFromRadius` @ 0x1402cb0f0
#[allow(clippy::neg_cmp_op_on_partial_ord)]
pub fn ks_speed_from_radius(radius: f32, grip: f32, camber: f32) -> f32 {
    let t = crate::math::tanf(camber);
    let sum = t + grip;
    let den = 1.0 - t * grip;
    if !(den >= 0.0) {
        return 200.0;
    }
    if !(sum >= 0.0) {
        return 0.0;
    }
    crate::math::sqrtf((sum / den) * (radius * f32::from_bits(0x411c_e560)))
}

/// `ksGFromRadius` @ 0x1402cb0b0
pub fn ks_g_from_radius(radius: f32, speed: f32, camber: f32) -> f32 {
    let g = ((speed * speed) / radius) * f32::from_bits(0x3dd0_da0b);
    g / (crate::math::tanf(camber) + 1.0)
}

/// AC's `PIDController` (0x14 bytes).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PidController {
    pub p: f32,
    pub i: f32,
    pub d: f32,
    pub current_error: f32,
    pub integral: f32,
}

impl PidController {
    /// `PIDController::eval` @ 0x14044e720
    pub fn eval(&mut self, target: f32, value: f32, dt: f32) -> f32 {
        let error = target - value;
        let derivative = (error - self.current_error) / dt;
        let integral = error * dt + self.integral;
        self.current_error = error;
        self.integral = integral;
        let out = derivative * self.d + (integral * self.i + error * self.p);
        if crate::math::fdtest_inf_or_nan(error) || crate::math::fdtest_inf_or_nan(out) || crate::math::fdtest_inf_or_nan(self.integral) {
            self.current_error = 0.0;
            self.integral = 0.0;
            return 0.0;
        }
        out
    }

    /// `PIDController::reset` @ 0x14044e7e0
    pub fn reset(&mut self) {
        self.current_error = 0.0;
        self.integral = 0.0;
    }
}
