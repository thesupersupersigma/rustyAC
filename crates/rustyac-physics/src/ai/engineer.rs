// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! `RaceEngineer` (RaceEngineer.obj of ksPhysicsAC.lib) as far as the AI drivers ask it:
//! estimates of what the car can do, from its state and its data.

// (the comparisons and the order of the operations are the machine code's)
#![allow(clippy::neg_cmp_op_on_partial_ord, clippy::assign_op_pattern, clippy::manual_range_contains, clippy::needless_range_loop, clippy::field_reassign_with_default, clippy::if_same_then_else, clippy::too_many_arguments)]

use crate::car::{RollingChassis, TractionType};
use crate::data::tyres_ini::{load_sens_exp_d, load_sens_linear_d};
use crate::session::MsvcRand;
use crate::tyre::TyreCompoundDef;

use super::{sat, RAND_UNIT};

/// `FindTyreCompoundLogic`
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompoundChoice {
    Fastest,
    Random,
}

/// 1 / 9.806 as the game has it (0x3dd0da0b)
const N_TO_KG: f32 = f32::from_bits(0x3dd0_da0b);

/// `RaceEngineer::getLeftDrivenTyre` @ 0x14027c1c0
pub fn left_driven_tyre(car: &RollingChassis) -> usize {
    if traction(car) == TractionType::Fwd {
        0
    } else {
        2
    }
}

/// `RaceEngineer::getRightDrivenTyre` @ 0x14027ca30
pub fn right_driven_tyre(car: &RollingChassis) -> usize {
    if traction(car) == TractionType::Fwd {
        1
    } else {
        3
    }
}

fn traction(car: &RollingChassis) -> TractionType {
    car.drivetrain.as_ref().map_or(TractionType::Rwd, |d| d.base().traction_type)
}

/// `RaceEngineer::getDrivingTyresSlip` @ 0x14027bc20
pub fn get_driving_tyres_slip(car: &RollingChassis) -> f32 {
    let (a, b) = if traction(car) == TractionType::Fwd { (0, 1) } else { (2, 3) };
    let (a, b) = (car.tyres[a].status.nd_slip, car.tyres[b].status.nd_slip);
    if a <= b {
        b
    } else {
        a
    }
}

/// `RaceEngineer::getWeightDistribution` @ 0x14027cb90: the front axle's share.
pub fn get_weight_distribution(car: &RollingChassis) -> f32 {
    let front = car.suspensions[0].get_base_position().z;
    let rear = car.suspensions[2].get_base_position().z;
    1.0 - front.abs() / (front - rear).abs()
}

/// `|basePosition.x * 2|` of an axle's left suspension: its track width.
fn track_width(car: &RollingChassis, suspension: usize) -> f32 {
    (car.suspensions[suspension].get_base_position().x * 2.0).abs()
}

/// `RaceEngineer::projectWingsLift` @ 0x14027cdc0: the wings' downforce at a speed, N.
pub fn project_wings_lift(car: &RollingChassis, speed: f32) -> f32 {
    let Some(aero) = &car.aero else { return 0.0 };
    let base = aero.base();
    let flow = (speed * speed) * base.air_density;
    let mut lift = 0.0f32;
    for wing in &base.wings {
        let cl = wing.data.lut_aoa_cl.get_value(wing.status.angle + wing.status.aoa);
        lift += ((wing.data.area * 0.5) * flow) * (cl * wing.data.cl_gain);
    }
    lift
}

/// `RaceEngineer::projectWingsDrag` @ 0x14027ccf0
pub fn project_wings_drag(car: &RollingChassis, speed: f32) -> f32 {
    let Some(aero) = &car.aero else { return 0.0 };
    let base = aero.base();
    let mut drag = 0.0f32;
    for wing in &base.wings {
        let cd = wing.data.lut_aoa_cd.get_value(wing.status.angle + wing.status.aoa);
        drag += (((base.air_density * wing.data.area) * 0.5) * (speed * speed)) * (cd * wing.data.cd_gain);
    }
    drag
}

/// `RaceEngineer::evalLateralGFromLoads` @ 0x14027a260: the lateral g the four tyres give
/// under these loads, limited by the axle that gives up first and by roll-over.
pub fn eval_lateral_g_from_loads(car: &RollingChassis, speed: f32, aero_factor: f32, mass: f32, loads: &[f32; 4]) -> f32 {
    let cg = car.get_cg_height();
    let front_share = get_weight_distribution(car);
    let front_track = track_width(car, 0);
    let rear_track = track_width(car, 2);
    let grip_level = car.track_grip_level();
    let mut force = [0.0f32; 4];
    for wheel in 0..4 {
        let tyre = &car.tyres[wheel];
        let d = tyre.get_dy(loads[wheel]);
        let d = tyre.peek_corrected_d(d);
        let d = (d / ((speed * tyre.model_data.speed_sensitivity) * tyre.slip_provider.max_slip + 1.0)) * grip_level;
        force[wheel] = d * loads[wheel];
    }
    let mut front = force[0] + force[1];
    let mut rear = force[2] + force[3];
    let by_rear = (rear * front_share) / (1.0 - front_share);
    let by_front = ((1.0 - front_share) * front) / front_share;
    if by_rear <= front {
        front = by_rear;
    }
    if by_front <= rear {
        rear = by_front;
    }
    let track = if rear_track <= front_track { rear_track } else { front_track };
    let g = ((rear + front) / mass) * N_TO_KG;
    let roll = ((track * 0.5) / cg) * aero_factor;
    if roll < g {
        roll
    } else {
        g
    }
}

/// `RaceEngineer::evalLateralGrip` @ 0x14027a510: the lateral g the car can hold at a
/// speed. `loads`: the tyres' loads as they are (else worked out from the mass, the wings
/// and the load transfer); `lift_kg`: the downforce as it is, kg (0 or less: from the wings'
/// tables at that speed, times `aero_hint`).
pub fn eval_lateral_grip(car: &RollingChassis, speed: f32, mass: f32, aero_hint: f32, loads: Option<&[f32; 4]>, mut lift_kg: f32) -> f32 {
    let front_share = get_weight_distribution(car);
    if lift_kg <= 0.0 {
        lift_kg = project_wings_lift(car, speed) * aero_hint;
    }
    let aero_factor = (lift_kg / mass) * N_TO_KG + 1.0;
    let loads = match loads {
        Some(loads) => *loads,
        None => {
            let front_weight = (mass * 9.806) * front_share;
            let grip = (car.tyres[0].get_dy((front_weight * 0.5) * aero_factor) * 0.9) * aero_factor;
            let front_track = track_width(car, 0);
            let rear_track = track_width(car, 2);
            let cg = car.get_cg_height();
            let front_transfer = (cg / front_track) * grip;
            let front_left = sat(front_transfer + 0.5, 0.0, 1.0);
            let front_right = sat(0.5 - front_transfer, 0.0, 1.0);
            let rear_transfer = (cg / rear_track) * grip;
            let rear_left = sat(rear_transfer + 0.5, 0.0, 1.0);
            let rear_right = sat(0.5 - rear_transfer, 0.0, 1.0);
            let rear_weight = (1.0 - front_share) * (mass * 9.806);
            let rear_lift = ((1.0 - front_share) * lift_kg) * 0.5;
            let front_lift = (lift_kg * front_share) * 0.5;
            let raw = [front_weight * front_left + front_lift, front_weight * front_right + front_lift, rear_weight * rear_left + rear_lift, rear_weight * rear_right + rear_lift];
            // `maxps` with the register that still holds the rear load transfer in its first
            // lane and zeros in the others: no load below that
            [if rear_transfer > raw[0] { rear_transfer } else { raw[0] }, if 0.0 > raw[1] { 0.0 } else { raw[1] }, if 0.0 > raw[2] { 0.0 } else { raw[2] }, if 0.0 > raw[3] { 0.0 } else { raw[3] }]
        }
    };
    eval_lateral_g_from_loads(car, speed, aero_factor, mass, &loads)
}

/// `RaceEngineer::evalTyreLoad` @ 0x14027a9e0: (lf, rf, lr, rr) at a speed under a lateral
/// and a longitudinal g.
fn eval_tyre_load(car: &RollingChassis, speed: f32, lateral_g: f32, longitudinal_g: f32) -> [f32; 4] {
    let front_share = get_weight_distribution(car);
    let lift = project_wings_lift(car, speed);
    let mass = car.get_total_mass(true);
    let cg = car.get_cg_height();
    let front_z = car.suspensions[0].get_base_position().z;
    let rear_z = car.suspensions[2].get_base_position().z;
    let share = sat(-((cg / (front_z - rear_z)) * longitudinal_g) + front_share, 0.0, 1.0);
    let front_transfer = (cg / track_width(car, 0)) * lateral_g;
    let front_weight = share * (mass * 9.806);
    let mut lf = (front_transfer + 0.5) * front_weight;
    let mut rf = (0.5 - front_transfer) * front_weight;
    let rear_transfer = (cg / track_width(car, 2)) * lateral_g;
    let rear_weight = (1.0 - share) * (mass * 9.806);
    let front_lift = (lift * front_share) * 0.5;
    let rear_lift = ((1.0 - front_share) * lift) * 0.5;
    rf += front_lift;
    lf += front_lift;
    let rr = (0.5 - rear_transfer) * rear_weight + rear_lift;
    let lr = (rear_transfer + 0.5) * rear_weight + rear_lift;
    [lf, rf, lr, rr]
}

/// `RaceEngineer::evalAvailableBrake` @ 0x140279f50: the braking g the front tyres leave at
/// a speed in a corner of `lateral_g`.
pub fn eval_available_brake(car: &RollingChassis, speed: f32, lateral_g: f32, mass: f32) -> f32 {
    let lift = project_wings_lift(car, speed);
    let grip_level = car.track_grip_level();
    let loads = eval_tyre_load(car, speed, lateral_g, -(((lift * (1.0 / mass)) * N_TO_KG + 1.0) * car.tyres[0].model_data.dx0));
    let left = car.tyres[0].peek_corrected_d(car.tyres[0].get_dx(loads[0]));
    let right = car.tyres[1].peek_corrected_d(car.tyres[1].get_dx(loads[1]));
    let t = &car.tyres;
    let mut left = ((left * loads[0]) * grip_level) / ((speed * t[0].model_data.speed_sensitivity) * t[0].slip_provider.max_slip + 1.0);
    let right = ((right * loads[1]) * grip_level) / ((speed * t[1].model_data.speed_sensitivity) * t[1].slip_provider.max_slip + 1.0);
    if right <= left {
        left = right;
    }
    ((left * 4.0) * (1.0 / mass)) * N_TO_KG
}

/// `RaceEngineer::getMaxBrakingForce` @ 0x14027c1f0
pub fn get_max_braking_force(car: &RollingChassis) -> f32 {
    let Some(brakes) = &car.brake_system else { return 0.0 };
    let power = brakes.get_brake_power();
    let front = power * brakes.base().front_bias;
    ((power - front) / car.tyres[2].data.radius + front / car.tyres[0].data.radius) * 2.1
}

/// `RaceEngineer::getOptimalBrake` @ 0x14027c320: the pedal at which the first axle locks
/// with the loads as they are.
pub fn get_optimal_brake(car: &RollingChassis) -> f32 {
    let Some(brakes) = &car.brake_system else { return 0.0 };
    let t = &car.tyres;
    let d: [f32; 4] = [0, 1, 2, 3].map(|w| t[w].peek_corrected_d(t[w].get_dx(t[w].status.load)));
    let load: [f32; 4] = [0, 1, 2, 3].map(|w| t[w].status.load);
    let front_radius = t[0].status.loaded_radius;
    let rear_radius = t[2].status.loaded_radius;
    let bias = brakes.base().front_bias;
    let front = (((d[1] * load[1] + d[0] * load[0]) * 0.5) * front_radius) / (brakes.get_brake_power() * bias);
    let rear = (((d[3] * load[3] + d[2] * load[2]) * 0.5) * rear_radius) / (brakes.get_brake_power() * (1.0 - bias));
    if front < rear {
        front
    } else {
        rear
    }
}

/// `RaceEngineer::projectCarForwardForceAtGas` @ 0x14027cc20: the drive force at the wheels
/// at the engine's speed now and a throttle, N.
pub fn project_car_forward_force_at_gas(car: &RollingChassis, gas: f32) -> f32 {
    let Some(drivetrain) = &car.drivetrain else { return 0.0 };
    let base = drivetrain.base();
    let engine = drivetrain.engine();
    let throttle = engine.throttle_response_gas(gas, 0.0);
    let rpm = ((base.engine.velocity * 0.15915507152579872) * 60.0) as f32;
    let torque = engine.get_torque_at_rpm(rpm, throttle);
    let boost = if torque <= 0.0 { 1.0 } else { engine.base().status.turbo_boost + 1.0 };
    let radius = car.tyres[left_driven_tyre(car)].data.radius;
    ((((boost * torque) as f64) * base.ratio) / radius as f64) as f32
}

/// `RaceEngineer::getTorqueToGripRatio` @ 0x14027ca60: the full-throttle drive force over
/// what the driven tyres can put down.
pub fn get_torque_to_grip_ratio(car: &RollingChassis) -> f32 {
    let force = project_car_forward_force_at_gas(car, 1.0);
    if 0.0 < force {
        let mut grip = 0.0f32;
        for tyre in &car.tyres {
            if tyre.driven {
                let d = tyre.peek_corrected_d(tyre.get_dx(tyre.status.load));
                grip += (d * tyre.data.radius) * tyre.status.load;
            }
        }
        if grip != 0.0 {
            return force / grip;
        }
    }
    1.0
}

/// `RaceEngineer::getAccelerationProfile` @ 0x14027b550: the throttle that just holds the
/// speed against the wings' drag, by five halvings.
#[allow(clippy::neg_cmp_op_on_partial_ord)]
pub fn get_acceleration_profile(car: &RollingChassis) -> f32 {
    let drag = car.aero.as_ref().map_or(0.0, |aero| aero.base().get_current_drag_kg());
    let (mut low, mut high) = (0.0f32, 1.0f32);
    for _ in 0..5 {
        let mid = (high + low) * 0.5;
        let force = project_car_forward_force_at_gas(car, mid);
        if force < drag * 9.806 {
            low = mid;
        } else {
            high = mid;
        }
    }
    (high + low) * 0.5
}

/// `RaceEngineer::getCompoundDY` @ 0x14027bbf0
pub fn get_compound_dy(def: &TyreCompoundDef, load: f32) -> f32 {
    let m = &def.model_data;
    if 4 < m.version {
        load_sens_exp_d(m.ls_exp_y, m.ls_mult_y, load)
    } else {
        load_sens_linear_d(m.dy0, m.dy1, load)
    }
}

/// `RaceEngineer::findTyreCompound` @ 0x14027b3a0
pub fn find_tyre_compound(car: &RollingChassis, choice: CompoundChoice, rand: &mut MsvcRand) -> i32 {
    let defs = &car.tyres[0].compound_defs;
    match choice {
        CompoundChoice::Random => {
            let draw = rand.next() as f32 * RAND_UNIT;
            crate::car::drivetrain::cvttss2si(draw * defs.len() as f32)
        }
        CompoundChoice::Fastest => {
            let mut winner = -1i32;
            let mut best = 0.0f32;
            for def in defs {
                let dy = get_compound_dy(def, 3000.0);
                if best < dy {
                    winner = def.index as i32;
                    best = dy;
                }
            }
            winner
        }
    }
}
