// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! `AIDriverDrag` (AIDriverDrag.obj of ksPhysicsAC.lib, 0x118 bytes): the drag-race driver.
//!
//! It knows no track line: when its car is put on its place it lays a straight line of its
//! own through that place, 5 km long, and steers along it. Before its start time it holds
//! the car on a little brake and brings the revs up in the last second; from then on it is
//! flat out (half throttle while the tyres slip past its limit) and shifts by revs.

// (the comparisons and the order of the operations are the machine code's)
#![allow(clippy::neg_cmp_op_on_partial_ord, clippy::assign_op_pattern)]

use super::{sat, PidController, RAND_UNIT};
use crate::car::{CarControls, CarControlsInput, ControlsProvider, RollingChassis};
use crate::data::ini::IniReader;
use crate::math::fdtest_inf_or_nan;
use crate::session::MsvcRand;
use crate::track::spline::{InterpolatingSpline, SplinePoint};
use crate::vecmath::Vec3f;

/// AC's `AIDriverDrag`.
#[derive(Clone, Debug)]
pub struct AiDriverDrag {
    /// `push`: the car's `AI_LEVEL` of race.ini as written (0..100)
    pub push: f32,
    pub spline: InterpolatingSpline,
    pub pid_steer: PidController,
    pub pid_brake: PidController,
    pub pid_gas: PidController,
    pub steer_signal: f32,
    pub gas_signal: f32,
    pub brake_signal: f32,
    pub change_up_rpm: i32,
    pub change_dn_rpm: i32,
    pub steer_gain: f32,
    pub steer_min_lookahead: f32,
    pub gas_brake_lookahead: f32,
    /// `startTime`: the physics clock at which the driver goes, ms
    pub start_time: f64,
    pub normalized_spline_position: f32,
    pub target_offset: f32,
    pub old_steer: f32,
    pub understeer_steer_correction: f32,
    pub max_push: f32,
    pub slip_ratio_limit: f32,
}

impl AiDriverDrag {
    /// `AIDriverDrag::AIDriverDrag` @ 0x1402ab740
    pub fn new(car: &mut RollingChassis) -> Result<AiDriverDrag, String> {
        let mut ai = AiDriverDrag {
            push: 0.0,
            spline: InterpolatingSpline::default(),
            pid_steer: PidController::default(),
            pid_brake: PidController::default(),
            pid_gas: PidController::default(),
            steer_signal: 0.0,
            gas_signal: 0.0,
            brake_signal: 0.0,
            change_up_rpm: 8000,
            change_dn_rpm: 4000,
            steer_gain: 0.05,
            steer_min_lookahead: 12.0,
            gas_brake_lookahead: 3.0,
            start_time: f64::MAX,
            normalized_spline_position: 0.0,
            target_offset: 0.0,
            old_steer: 0.0,
            understeer_steer_correction: 0.0,
            max_push: 100.0,
            slip_ratio_limit: 10.0,
        };
        let drivetrain = car.drivetrain.as_ref().ok_or("an AI driver needs the car's drivetrain")?;
        ai.change_dn_rpm = crate::car::drivetrain::cvttss2si(drivetrain.engine().get_max_torque_rpm());
        let path = car.data_path.join("ai.ini");
        let ini = if crate::data::exists(&path) { IniReader::load(&path)? } else { IniReader::default() };
        if ini.ready {
            ai.change_up_rpm = ini.get_int("GEARS", "UP").unwrap_or(0);
            ai.change_dn_rpm = ini.get_int("GEARS", "DOWN").unwrap_or(0);
            if ini.has_section("STEER") {
                ai.steer_gain = ini.get_float("STEER", "STEER_GAIN").unwrap_or(0.0);
            }
            if ini.has_section("LOOKAHEAD") {
                ai.steer_min_lookahead = ini.get_float("LOOKAHEAD", "BASE").unwrap_or(0.0);
                ai.gas_brake_lookahead = ini.get_float("LOOKAHEAD", "GAS_BRAKE_LOOKAHEAD").unwrap_or(0.0);
            }
            if ini.has_section("DRAG") {
                ai.slip_ratio_limit = ini.get_float("DRAG", "SLIP_RATIO_LIMIT").unwrap_or(0.0);
            }
        }
        if let Some(aids) = &mut car.aids {
            aids.base_mut().traction_control.is_active = false;
        }
        ai.pid_steer = PidController { p: 0.3, i: 0.0, d: 0.0, ..PidController::default() };
        ai.pid_brake = PidController { p: 1.0, i: 0.01, d: 0.0, ..PidController::default() };
        ai.pid_gas = PidController { p: 0.3, i: 0.0, d: 0.001, ..PidController::default() };
        if ai.slip_ratio_limit == 0.0 {
            ai.slip_ratio_limit = 10.0;
        }
        Ok(ai)
    }

    /// `AIDriverDrag::setStartTime` @ 0x1402ac3d0: the start, later by up to 50 ms for every
    /// point the driver's level is below 100. Draws one `rand()`.
    pub fn set_start_time(&mut self, time: f64, rand: &mut MsvcRand) {
        let late = (rand.next() as f32 * RAND_UNIT) * 50.0;
        self.start_time = ((self.max_push - self.push) * late) as f64 + time;
    }

    /// The driver's handler of `Car::evOnForcedPositionCompleted` (lambda @ 0x1402abf80): a
    /// line of 201 points 25 m apart through the place the car was put on, from 200 m before
    /// it. (It runs along the first two numbers of the body matrix's third row at the
    /// height of the third: the game's own mix of axes.)
    fn lay_line(&mut self, car: &RollingChassis) {
        self.spline = InterpolatingSpline::default();
        self.spline.linear = true;
        let place = car.forced_position;
        let m = car.core.get_world_matrix(car.body).m;
        let x0 = -m[2][0] * 200.0 + place.x;
        let y0 = -m[2][1] * 200.0 + place.y;
        for i in 0..=200u32 {
            let m = car.core.get_world_matrix(car.body).m;
            let point = [(m[2][0] * 25.0) * i as f32 + x0, (m[2][1] * 25.0) * i as f32 + y0, place.z];
            // Spline::addPoint: the running length along the points
            let point_length = match self.spline.points.last() {
                Some(last) => {
                    let d = [point[0] - last.point[0], point[1] - last.point[1], point[2] - last.point[2]];
                    let squared = (d[1] * d[1] + d[0] * d[0]) + d[2] * d[2];
                    last.point_length + if squared == 0.0 { 0.0 } else { crate::math::sqrtf(squared) }
                }
                None => 0.0,
            };
            self.spline.m_length = point_length;
            self.spline.points.push(SplinePoint { point, point_length, tag: 0 });
        }
        self.spline.build_grid();
        self.spline.compute_spline_length();
    }

    /// `AIDriverDrag::getSteerAtDistance` @ 0x1402ac350
    fn get_steer_at_distance(&self, car: &RollingChassis, distance: f32) -> f32 {
        let length = self.spline.length();
        let point = self.spline.spline_to_world(distance / length + self.normalized_spline_position);
        let local = car.core.world_to_local(car.body, &Vec3f::new(point[0], point[1], point[2]));
        local.x - self.target_offset
    }

    /// `AIDriverDrag::stepSteer` @ 0x1402ac790
    fn step_steer(&mut self, car: &RollingChassis) {
        let mut steer = (self.get_steer_at_distance(car, 25.0) * -0.05 - self.old_steer) + self.old_steer;
        steer = if fdtest_inf_or_nan(steer) { 0.0 } else { sat(steer, -0.9, 0.9) };
        if 0.0 < self.understeer_steer_correction {
            self.understeer_steer_correction -= 1.0;
            steer = (steer - self.old_steer) * 0.1 + self.old_steer;
        }
        let steer = if 0.9 < steer {
            0.9
        } else if steer < -0.9 {
            -0.9
        } else {
            steer
        };
        self.steer_signal = steer;
        self.old_steer = steer;
    }

    /// `AIDriverDrag::stepGasBrake` @ 0x1402ac430 with `stepGasBrakeStart` @ 0x1402ac520
    fn step_gas_brake(&mut self, car: &RollingChassis) {
        self.gas_signal = 0.0;
        self.brake_signal = 0.0;
        let to_start = (self.start_time - car.physics_time) as f32;
        if to_start <= 0.0 {
            self.gas_signal = 1.0;
            let t = &car.tyres;
            let rear = if t[2].status.nd_slip < t[3].status.nd_slip { t[3].status.nd_slip } else { t[2].status.nd_slip };
            let front = if t[0].status.nd_slip < t[1].status.nd_slip { t[1].status.nd_slip } else { t[0].status.nd_slip };
            let worst = if rear <= front { front } else { rear };
            if self.slip_ratio_limit < worst {
                self.gas_signal = 0.5;
            }
        } else {
            self.gas_signal = 0.0;
            if to_start < 1000.0 {
                let rpm = car.drivetrain.as_ref().map_or(0.0, |d| d.get_engine_rpm());
                self.gas_signal = sat((self.change_up_rpm as f32 - rpm) * 0.001 + self.gas_signal, 0.0, 1.0);
            }
            self.brake_signal = 0.05;
        }
    }

    /// `AIDriverDrag::stepGears` @ 0x1402ac5d0
    fn step_gears(&mut self, car: &mut RollingChassis) {
        let Some(drivetrain) = &car.drivetrain else { return };
        let base = drivetrain.base();
        let rpm = (base.engine.velocity * 0.15915507152579872) * 60.0;
        let rpm_whole = if rpm.is_nan() || !(-2147483648.0..2147483648.0).contains(&rpm) { i32::MIN } else { rpm as i32 };
        if self.change_up_rpm < rpm_whole
            && !car.controls.gear_up
            && base.cut_off <= 0.0
            && !drivetrain.is_changing_gear()
            && (car.speed < 1.0 || car.tyres[2].status.nd_slip < 1.1)
            && (0.99 < car.controls.clutch || base.current_gear == 1)
        {
            car.controls.gear_up = true;
            car.controls.gear_dn = false;
            return;
        }
        car.controls.gear_up = false;
        let t = &car.tyres;
        let rear = if t[2].status.nd_slip < t[3].status.nd_slip { t[3].status.nd_slip } else { t[2].status.nd_slip };
        car.controls.gear_dn = !(drivetrain.is_changing_gear() || car.controls.clutch <= 0.98 || 0.1 <= car.controls.gas || self.change_dn_rpm as f64 <= rpm || base.current_gear < 3 || 1.1 <= rear);
    }

    /// `AIDriverDrag::acquireControls` @ 0x1402ac1f0
    pub fn acquire(&mut self, car: &mut RollingChassis) {
        let m = car.core.get_world_matrix(car.body).m;
        self.normalized_spline_position = self.spline.world_to_spline(&[m[3][0], m[3][1], m[3][2]], -1);
        self.step_steer(car);
        self.step_gas_brake(car);
        self.step_gears(car);
        car.controls.steer = sat(self.steer_signal, -1.0, 1.0);
        car.controls.gas = sat(self.gas_signal, 0.0, 1.0);
        car.controls.brake = sat(self.brake_signal, 0.0, 1.0);
    }
}

impl ControlsProvider for AiDriverDrag {
    /// The driver needs its car: [`ControlsProvider::acquire_controls_car`] is its entry.
    fn acquire_controls(&mut self, _controls: &mut CarControls, _dt: f32, _input: &CarControlsInput) {}

    fn acquire_controls_car(&mut self, car: &mut RollingChassis, _dt: f32) {
        self.acquire(car);
    }

    fn is_ai(&self) -> bool {
        true
    }

    fn on_forced_position(&mut self, car: &mut RollingChassis) {
        while car.forced_position_events > 0 {
            car.forced_position_events -= 1;
            self.lay_line(car);
        }
    }
}
