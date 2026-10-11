// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! The car update a client sends (`ACClient::sendCarPosition` @ 0x140052230), datagram
//! 0x46, 62 bytes:
//!
//! | offset | type | what |
//! |---|---|---|
//! | 0 | `u8` | 0x46 |
//! | 1 | `u8` | sequence number (one up per update, wraps) |
//! | 2 | `u32` | the physics clock, whole ms |
//! | 6 | 3 x `f32` | the body's position |
//! | 18 | 3 x `f32` | heading, pitch, roll (radians) from the body's matrix |
//! | 30 | 3 x `f32` | the body's velocity |
//! | 42 | 4 x `u8` | wheel speeds: `round(log10(|w| + 1) * 20) * sign + 100` |
//! | 46 | `u8` | steering wheel, degrees + 127 |
//! | 47 | `u8` | front wheel angle, half degrees + 127 |
//! | 48 | `u16` | engine rpm (1 to 65000) |
//! | 50 | `u8` | gear (0 reverse, 1 neutral, 2 first) |
//! | 51 | `u32` | status bits ([`crate::net_car::status`]) |
//! | 55 | `i16` | performance delta, ms (within 30 s) |
//! | 57 | `u8` | throttle x 255 |
//! | 58 | `f32` | the place round the lap, 0..1 |
//!
//! The server sends the other clients the first 51 bytes' worth of it in its mega packets
//! (with the car's slot in front and its own idea of each client's clock).

// Comparisons are spelled the way the original branches so NaN takes the same path.
#![allow(clippy::neg_cmp_op_on_partial_ord)]

use rustyac_math::{asinf, atan2f, log10f};
use rustyac_physics::vecmath::Mat44f;

use crate::datagram::id;
use crate::net_car::status;
use crate::packet::Writer;

/// What `sendCarPosition` reads of the local car, after `dWorldStep` and before the car's
/// `Car::postStep`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LocalCarState {
    /// `car->body->getWorldMatrix`
    pub body_matrix: Mat44f,
    /// `car->body->getVelocity`
    pub velocity: [f32; 3],
    /// `tyres[i].status.angularVelocity`, `tyres[i].status.slipRatio`
    pub wheel_angular_velocity: [f32; 4],
    pub wheel_slip_ratio: [f32; 4],
    /// `Car::getSpeed`, m/s
    pub speed: f32,
    /// `controls.steer` and `Car::steerLock`
    pub steer: f32,
    pub steer_lock: f32,
    /// `Car::getFrontWheelAngle`, radians
    pub front_wheel_angle: f32,
    /// `Drivetrain::getEngineRPM`
    pub engine_rpm: f32,
    /// `drivetrain.currentGear`
    pub current_gear: i32,
    pub gas: f32,
    pub brake: f32,
    pub lights_on: bool,
    /// The driver's horn (action 6 of the controls provider).
    pub horn: bool,
    /// `Car::getPenaltyTime`
    pub penalty_time: f64,
    /// A headlight flash was asked for since the last update.
    pub flash: bool,
    /// `ers.isCharging`
    pub ers_is_charging: bool,
    /// `drs.isPresent && drs.isActive`
    pub drs_open: bool,
    /// `PerformanceMeter::getCurrentSplit`, seconds
    pub performance_split: f64,
    /// `splineLocatorData.npos`
    pub normalized_position: f32,
}

/// `mat44f::toEuler` as `sendCarPosition` has it inline: heading, pitch, roll.
pub fn euler_of(m: &Mat44f) -> [f32; 3] {
    let m = &m.m;
    let mut heading = atan2f(-m[2][0], m[2][2]);
    let mut sine = m[2][1];
    if sine > 1.0 {
        sine = 1.0;
    } else if !(sine >= -1.0) {
        sine = -1.0;
    }
    let pitch = asinf(sine);
    // (ucomiss + jne: a NaN counts as zero here)
    let zero = |x: f32| !(x < 0.0 || x > 0.0);
    let roll = if zero(m[0][1]) && zero(m[1][1]) {
        // straight up or down: all of the turn about the vertical goes into the roll
        heading = 0.0;
        atan2f(m[1][0], m[0][0])
    } else {
        atan2f(-m[0][1], m[1][1])
    };
    [heading, pitch, roll]
}

/// One wheel's speed as its byte.
pub fn encode_wheel_speed(speed: f32) -> u8 {
    let magnitude = speed.abs();
    let mut x = speed;
    if magnitude > f32::from_bits(0x3dcc_cccd) {
        let sign = if speed > 0.0 {
            1.0f32
        } else if speed == 0.0 {
            0.0
        } else {
            -1.0
        };
        x = (log10f(magnitude + 1.0) * 20.0).round() * sign;
    }
    // (a wheel turning at 0.1 rad/s or less goes in as it is)
    if x > 154.0 {
        x = 154.0;
    } else if !(x >= -100.0) {
        x = -100.0;
    }
    (x + 100.0) as i64 as u8
}

/// The status bits of the update.
pub fn status_bits(car: &LocalCarState) -> u32 {
    let mut bits = 0u32;
    for wheel in 0..4 {
        if car.wheel_slip_ratio[wheel].abs() > f32::from_bits(0x3f4c_cccd) && car.speed > 1.0 {
            bits |= status::SKID_LF << wheel;
        }
    }
    if car.brake as f64 > f64::from_bits(0x3fb9_9999_9999_999a) {
        bits |= status::BRAKING;
    }
    if car.lights_on {
        bits |= status::LIGHTS;
    }
    if car.horn {
        bits |= status::HORN;
    }
    if car.penalty_time > 0.0 {
        bits |= status::PENALTY;
    }
    if car.flash {
        bits |= status::FLASH;
    }
    if car.ers_is_charging {
        bits |= status::KERS_CHARGING;
    }
    if car.drs_open {
        bits |= status::DRS;
    }
    bits
}

/// Datagram 0x46 for the car at `physics_time` (ms) with sequence number `sequence`.
pub fn encode(sequence: u8, physics_time: f64, car: &LocalCarState) -> Vec<u8> {
    let mut w = Writer::with_id(id::CAR_UPDATE);
    w.u8(sequence);
    w.u32(physics_time as i64 as u32);
    let m = &car.body_matrix.m;
    w.vec3([m[3][0], m[3][1], m[3][2]]);
    w.vec3(euler_of(&car.body_matrix));
    w.vec3(car.velocity);
    for wheel in 0..4 {
        w.u8(encode_wheel_speed(car.wheel_angular_velocity[wheel]));
    }
    let mut steer = car.steer * car.steer_lock;
    if steer > 127.0 {
        steer = 127.0;
    } else if !(steer >= -127.0) {
        steer = -127.0;
    }
    w.u8((steer + 127.0) as i64 as u8);
    let angle = (car.front_wheel_angle * 2.0) * f32::from_bits(0x4265_2ee1);
    let angle = if angle > 127.0 {
        127.0
    } else if !(angle < -127.0) && !angle.is_nan() {
        angle
    } else {
        -127.0
    };
    w.u8((angle + 127.0) as i64 as u8);
    let rpm = if car.engine_rpm > 65000.0 {
        65000.0
    } else if car.engine_rpm >= 1.0 {
        car.engine_rpm
    } else {
        1.0
    };
    w.u16(rpm as i64 as u16);
    w.u8(car.current_gear as u8);
    w.u32(status_bits(car));
    let split = car.performance_split;
    let delta = if split.abs() > 30.0 {
        let sign = if split > 0.0 { 1.0 } else { -1.0 };
        (sign * 30000.0) as i32
    } else {
        (split * 1000.0) as i32
    };
    w.i16(delta as i16);
    w.u8((car.gas * 255.0) as i64 as u8);
    w.f32(car.normalized_position);
    w.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::datagram::decode_wheel_speed;

    #[test]
    fn update_is_62_bytes_and_wheels_round_trip() {
        let car = LocalCarState { body_matrix: Mat44f::IDENTITY, steer_lock: 180.0, steer: 0.5, engine_rpm: 7000.0, current_gear: 3, gas: 1.0, brake: 0.2, wheel_angular_velocity: [150.0, -150.0, 0.05, 0.0], ..LocalCarState::default() };
        let bytes = encode(9, 123_456.9, &car);
        assert_eq!(bytes.len(), 62);
        assert_eq!(&bytes[..6], &[0x46, 9, 0x40, 0xe2, 0x01, 0x00]);
        // the wheels: back within a few percent
        for (byte, speed) in bytes[42..46].iter().zip([150.0f32, -150.0, 0.0, 0.0]) {
            let back = decode_wheel_speed(*byte);
            assert!((back - speed).abs() <= speed.abs() * 0.07 + 1e-6, "{back} vs {speed}");
        }
        assert_eq!(bytes[46], 127 + 90);
        assert_eq!(u16::from_le_bytes([bytes[48], bytes[49]]), 7000);
        assert_eq!(bytes[50], 3);
        assert_eq!(u32::from_le_bytes([bytes[51], bytes[52], bytes[53], bytes[54]]), status::BRAKING);
        assert_eq!(bytes[57], 255);
        // level and pointing along z: no angles
        assert_eq!(euler_of(&Mat44f::IDENTITY), [0.0, 0.0, 0.0]);
    }
}
