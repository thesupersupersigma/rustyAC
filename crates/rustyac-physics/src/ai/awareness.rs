// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! `AIDriver::stepCarsAwareness2` and its helpers: what the driver makes of the other cars.
//!
//! Every fifth step the driver lists the cars that are not on the pit lane, each with where
//! it is (ahead and behind along the lap, beside), how fast, and how long until the driver
//! reaches it. A car beside it ("engaged") pushes the driver's line away; the nearest slower
//! car ahead is either passed on the side with more room, when there is room, time and no
//! corner in the way, or becomes the "brake obstacle" the look-ahead braking slows for.

// (the comparisons and the order of the operations are the machine code's)
#![allow(clippy::neg_cmp_op_on_partial_ord, clippy::assign_op_pattern, clippy::manual_range_contains, clippy::needless_range_loop, clippy::field_reassign_with_default, clippy::if_same_then_else, clippy::too_many_arguments)]

use super::driver::{AiBrakeObstacle, AiDriver, AiOpponentData};
use super::AiStepInput;
use crate::car::RollingChassis;
use crate::math::{asinf, sqrtf};
use crate::track::ai_line::AiSplineRecorder;
use crate::vecmath::Vec3f;

fn sign(x: f32) -> f32 {
    if x > 0.0 {
        1.0
    } else if x >= 0.0 {
        0.0
    } else {
        -1.0
    }
}

impl AiDriver {
    /// `AIDriver::calcMinForwardSides` @ 0x14029c4f0: the least room to each edge over the
    /// next metres.
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    fn calc_min_forward_sides(&mut self, recorder: &AiSplineRecorder, distance: f32) {
        let spline = self.spline(recorder);
        let mut n = self.loc_npos;
        let mut done = 0.0f32;
        let length = spline.length();
        self.min_forward_sides = [10000.0; 2];
        if 0.0 < distance {
            loop {
                let sides = spline.get_sides_at_pos(n);
                done += 10.0;
                n += 10.0 / length;
                for k in 0..2 {
                    if !(self.min_forward_sides[k] <= sides[k]) {
                        self.min_forward_sides[k] = sides[k];
                    }
                }
                if !(done < distance) {
                    break;
                }
            }
        }
    }

    /// `AIDriver::getDistanceToApex` @ 0x14029d130: metres to where the track is too narrow
    /// for a line `offset` to the side (1000: nowhere within 500 m).
    fn get_distance_to_apex(&self, car: &RollingChassis, recorder: &AiSplineRecorder, offset: f32, step: f32) -> f32 {
        let spline = self.spline(recorder);
        let mut n = self.loc_npos;
        let mut distance = 0.0f32;
        let length = spline.length();
        loop {
            let sides = spline.get_sides_at_pos(n);
            let half_width = car.car_half_width;
            if sides[0] + offset < half_width {
                return distance;
            }
            if sides[1] - offset < half_width {
                return distance;
            }
            distance += step;
            n = spline.spline.wrap_position(step / length + n);
            if 500.0 <= distance {
                return 1000.0;
            }
        }
    }

    /// `AIDriver::getEngagedDesiredOffset` @ 0x14029d3e0: the lateral offset that keeps the
    /// car clear of the ones beside it. `left` / `right`: the distance to the nearest car on
    /// each side (0 or less: none), `lateral`: where the car is, the track's free room ahead
    /// and how fast the two cars close in sideways.
    #[allow(clippy::too_many_arguments)]
    fn get_engaged_desired_offset(&self, car: &RollingChassis, left: f32, right: f32, lateral: f32, room_left: f32, room_right: f32, left_closing: f32, right_closing: f32) -> f32 {
        let closing = if right_closing <= left_closing { left_closing } else { right_closing };
        let closing = if closing <= 0.0 { 0.0 } else { closing };
        let gap = (closing * 0.03 + 1.0) * (car.car_half_width * 3.0);
        if !(right > 0.0) && left <= 0.0 {
            return 0.0;
        }
        if left > 0.0 {
            if right > 0.0 {
                return ((left + right) * 0.5 - left) + lateral;
            }
            let limit = gap * 1.5 - room_left;
            if !(left >= gap) {
                let wanted = (gap - left) + lateral;
                return if wanted <= limit { limit } else { wanted };
            }
            if limit > lateral {
                return limit;
            }
        }
        if right > 0.0 {
            let limit = -(gap * 1.5 - room_right);
            if !(right >= gap) {
                let wanted = lateral - (gap - right);
                return if limit <= wanted { limit } else { wanted };
            }
            if limit < lateral {
                return limit;
            }
        }
        if lateral > 0.0 {
            if right <= 0.0 {
                return 0.0;
            }
            let room = right - gap;
            lateral - if room <= lateral { room } else { lateral }
        } else {
            if left <= 0.0 {
                return 0.0;
            }
            let room = left - gap;
            let back = -lateral;
            lateral + if room <= back { room } else { back }
        }
    }

    /// `AIDriver::stepCarsAwareness2` @ 0x14029f8d0
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    pub(super) fn step_cars_awareness2(&mut self, car: &RollingChassis, recorder: &AiSplineRecorder, input: &AiStepInput, dt: f32) {
        let aggression = self.aggression_hint * self.aggression;
        self.calc_min_forward_sides(recorder, 100.0);
        self.brake_obstacles.clear();
        self.next_car_modifier = 0.0;
        if car.is_in_pit_lane() {
            return;
        }
        let mut step = 3.0f32;
        let width = car.car_half_width * 2.5;
        self.desired_offset = 0.0;
        let mut near = 0i32;
        let (mut left, mut right) = (-1.0f32, -1.0f32);
        let (mut left_closing, mut right_closing) = (0.0f32, 0.0f32);
        let my_speed = car.speed;
        self.opponent_data.clear();
        let t = &car.tyres;
        let rear = if t[2].status.nd_slip < t[3].status.nd_slip { t[3].status.nd_slip } else { t[2].status.nd_slip };
        let front = if t[0].status.nd_slip < t[1].status.nd_slip { t[1].status.nd_slip } else { t[0].status.nd_slip };
        let worst = if front < rear { rear } else { front };
        let at_the_limit = aggression * 0.19999999 + 0.8 <= worst;
        let mut engaged = 0i32;
        let me = &input.cars[input.me];
        let my_velocity = car.core.get_velocity(car.body);
        for (index, other) in input.cars.iter().enumerate() {
            if index == input.me || other.is_in_pit_lane {
                continue;
            }
            let mut o = AiOpponentData { car: index, ..AiOpponentData::default() };
            let rel = car.core.world_to_local(car.body, &other.position);
            o.rel_pos = rel;
            let dv = Vec3f::new(my_velocity.x - other.velocity.x, my_velocity.y - other.velocity.y, my_velocity.z - other.velocity.z);
            let rel_v = car.core.world_to_local_normal(car.body, &dv);
            let closing = sign(rel.x) * rel_v.x;
            let length = sqrtf((rel.x * rel.x + rel.y * rel.y) + rel.z * rel.z);
            let nx = if length == 0.0 { rel.x } else { rel.x / length };
            o.rel_x_speed = closing;
            let angle = asinf(nx);
            let rel_angle = (sign(rel.z) * angle) * 57.29578;
            let mut ahead = other.npos - me.npos;
            let mut behind = me.npos - other.npos;
            if ahead < 0.0 {
                ahead += 1.0;
            }
            if behind < 0.0 {
                behind += 1.0;
            }
            let lap = car.spline_locator_data.spline_length;
            o.lateral_offset = other.lateral_offset;
            let ahead = ahead * lap;
            let behind = behind * lap;
            o.space_left = other.lateral_offset + other.sides_from_il[0];
            o.space_right = other.sides_from_il[1] - other.lateral_offset;
            o.dist_front = ahead;
            o.dist_back = behind;
            o.rel_angle = rel_angle;
            let v = other.velocity;
            let squared = (v.x * v.x + v.y * v.y) + v.z * v.z;
            o.speed_ms = if squared == 0.0 { 0.0 } else { sqrtf(squared) };
            if rel.z <= 0.0 || my_speed - o.speed_ms <= 0.0 {
                o.tti = 100.0;
                o.dist_to_pass = my_speed * 100.0;
            } else {
                let inv = 1.0 / (my_speed - o.speed_ms);
                let to_reach = if ahead - 6.0 <= 0.01 { 0.01 } else { ahead - 6.0 };
                o.tti = inv * to_reach;
                let to_pass = if ahead - 3.0 <= 0.01 { 0.01 } else { ahead - 3.0 };
                o.dist_to_pass = ((inv * to_pass) * my_speed) * 0.8;
            }
            if 8.0 <= rel.z.abs() || (15.0 <= ahead && 15.0 <= behind) || rel_angle.abs() <= 25.0 {
                o.is_engaged = false;
            } else {
                o.is_engaged = true;
                engaged += 1;
                if 0.0 < rel.x {
                    if left < 0.0 || rel.x < left {
                        left = rel.x;
                        left_closing = closing;
                    }
                } else if right < 0.0 || -rel.x < right {
                    right = -rel.x;
                    right_closing = closing;
                }
            }
            if ahead < (car.speed * 3.6) * 0.2 + 20.0 {
                near += 1;
            }
            self.opponent_data.push(o);
        }
        self.is_engaged = 0.0 < left || 0.0 < right;
        self.desired_offset = self.get_engaged_desired_offset(car, left, right, car.spline_locator_data.lateral_offset, self.min_forward_sides[0], self.min_forward_sides[1], left_closing, right_closing);
        if self.desired_offset != 0.0 {
            step = 4.0;
        }
        if left <= 0.0 || right <= 0.0 {
            self.engaged_push = 1.0;
            if 1 < engaged {
                self.engaged_push = 1.0 - (engaged as f32 - 1.0) * 0.1;
            }
        } else {
            self.engaged_push = 0.9;
        }
        let apex_left = self.get_distance_to_apex(car, recorder, car.car_half_width * 2.0, 10.0);
        let apex_right = self.get_distance_to_apex(car, recorder, car.car_half_width * -2.0, 10.0);
        if self.desired_offset.abs() < 0.2 {
            let apex = if apex_left <= apex_right { apex_right } else { apex_left };
            if apex < 200.0 {
                if 0.0 < left {
                    self.desired_offset = width * 1.5 + self.desired_offset;
                }
                if 0.0 < right {
                    self.desired_offset -= width * 1.5;
                }
            }
        }
        let my_lateral = car.spline_locator_data.lateral_offset;
        crate::msvc_sort::sort(&mut self.opponent_data, &|a: &AiOpponentData, b: &AiOpponentData| a.tti < b.tti);
        let mut moved = false;
        let near_f = near as f32;
        let range = (my_speed * 0.07 + 8.0) * (near_f * 0.05 + 1.0);
        for index in 0..self.opponent_data.len() {
            let o = self.opponent_data[index];
            if o.is_engaged {
                continue;
            }
            let other = &input.cars[o.car];
            let beside = (o.lateral_offset - car.spline_locator_data.lateral_offset).abs();
            if other.is_outside_track_limits && 20.0 > o.speed_ms {
                continue;
            }
            if beside >= 8.0 {
                continue;
            }
            let (other_left, other_right) = input.engagement(o.car, 8.0);
            let class = (car.power_class_index - other.power_class_index) * 0.01;
            let class = if class > 6.0 {
                6.0
            } else if class >= 0.0 {
                class
            } else {
                0.0
            };
            let patience = class + 1.5;
            let reach = class + range;
            let mut obstacle: Option<f32> = None;
            if !(o.dist_front < reach) && !(o.tti < patience) {
                // far ahead still
                if !(o.dist_front >= range) && (my_lateral - o.lateral_offset).abs() < width * 1.5 {
                    self.next_car_modifier += if my_speed >= o.speed_ms { 1.0 } else { 0.5 };
                    obstacle = Some(8.0);
                } else if o.tti < 3.0 {
                    obstacle = Some(8.0);
                } else {
                    continue;
                }
            } else {
                let (mut free_left, mut free_right) = (0.0f32, 0.0f32);
                if (left < 0.0 || left > width * 2.0) && (other_left < 0.0 || other_left > width * 2.0) {
                    free_left = o.space_left;
                }
                if (right < 0.0 || right > width * 2.0) && (other_right < 0.0 || other_right > width * 2.0) {
                    free_right = o.space_right;
                }
                let beside_gap = width * 1.5;
                let (target, apex, free) = if free_left > free_right { (o.lateral_offset - beside_gap, apex_right, free_left) } else { (beside_gap + o.lateral_offset, apex_left, free_right) };
                let time_to_move = (target - self.current_offset).abs() / step;
                let mut can_pass = apex > o.dist_to_pass || apex > 200.0;
                let pace = car.speed / self.target_speed;
                if !(pace >= aggression) && !(0.0 >= self.target_speed) && !at_the_limit {
                    can_pass = true;
                } else if 1.0 > o.speed_ms {
                    can_pass = true;
                }
                if !moved && free > (near_f * 0.2 + 1.0) * width && !(time_to_move >= o.tti) && can_pass {
                    self.desired_offset = target;
                    moved = true;
                    continue;
                }
                let mut follow = false;
                if reach > o.dist_front && (my_lateral - o.lateral_offset).abs() < beside_gap && o.speed_ms > 1.0 {
                    self.next_car_modifier += if my_speed >= o.speed_ms { 1.0 } else { 0.5 };
                    follow = true;
                }
                if 2.0 > o.tti || follow {
                    obstacle = Some(7.0);
                }
            }
            if let Some(margin) = obstacle {
                // (one obstacle, and the list is not looked at any further)
                self.brake_obstacles.push(AiBrakeObstacle { distance: o.dist_front - margin, speed_ms: o.speed_ms, odata: index });
                break;
            }
        }
        self.next_car_modifier = super::sat(self.next_car_modifier, 0.0, 1.0);
        self.dynamic_push = if near < 2 { 1.0 } else { 1.0 - near_f * 0.001 };
        let wanted = self.desired_offset;
        let rate = if wanted == 0.0 {
            if 0.0 <= left || 0.0 <= right {
                0.002
            } else {
                0.01
            }
        } else {
            step * dt
        };
        let current = self.current_offset;
        let mut next = wanted;
        if rate < (current - wanted).abs() {
            next = if wanted <= current { current - rate } else { current + rate };
        }
        let d = &car.spline_locator_data;
        let hi = d.sides_from_il[1] - car.car_half_width;
        let lo = car.car_half_width - d.sides_from_il[0];
        self.current_offset = super::sat(next, lo, hi);
    }
}
