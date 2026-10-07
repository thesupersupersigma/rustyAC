// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni). Not covered by rustyAC's licenses — see LICENSING.md.

//! Scripted single-wheel scenarios for the rig: thousands of 0.003 s steps each.
//!
//! The scripts only need to be *plausible* and to visit the branches of `Tyre::step`; they
//! are inputs, not physics. They are deterministic (fixed seed, plain f32 arithmetic), and
//! the oracle stores the inputs it used in its recording anyway.

use super::{StepInput, DT};
use crate::data::tyres_ini::Axle;

/// Every scenario, in the order the oracle runs them.
pub const SCENARIOS: [&str; 12] = [
    "warmup",
    "brake_lockup",
    "wheelspin",
    "cornering",
    "liftoff",
    "blankets",
    "bump",
    "overheat",
    "random",
    "tester",
    "nan_inputs",
    "golden_mix",
];

/// The few tyre numbers the scripts need to place the hub sensibly.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Wheel {
    /// Unloaded radius, m.
    pub radius: f32,
    /// Rim radius, m.
    pub rim_radius: f32,
    /// Vertical spring rate, N/m.
    pub rate: f32,
    /// Static load, N.
    pub static_load: f32,
}

impl Default for Wheel {
    fn default() -> Wheel {
        Wheel {
            radius: 0.33,
            rim_radius: 0.165,
            rate: 250000.0,
            static_load: 3000.0,
        }
    }
}

/// Which compound index a scenario runs on (clamped to what the car has by the caller).
pub fn compound_for(scenario: &str) -> i32 {
    match scenario {
        "cornering" | "blankets" => 1,
        "liftoff" => 2,
        _ => 0,
    }
}

/// splitmix64: small, well mixed, and the same on every machine.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    /// Uniform in [0, 1).
    fn unit(&mut self) -> f32 {
        (self.next() >> 40) as f32 / (1u64 << 24) as f32
    }

    fn range(&mut self, low: f32, high: f32) -> f32 {
        low + (high - low) * self.unit()
    }

    fn chance(&mut self, probability: f32) -> bool {
        self.unit() < probability
    }
}

/// Linear ramp from `a` (at step `i0`) to `b` (at step `i1`), held outside.
fn ramp(i: usize, i0: usize, i1: usize, a: f32, b: f32) -> f32 {
    if i <= i0 {
        a
    } else if i >= i1 {
        b
    } else {
        a + (b - a) * ((i - i0) as f32 / (i1 - i0) as f32)
    }
}

struct Script<'a> {
    wheel: &'a Wheel,
    steps: Vec<StepInput>,
    x: f32,
    z: f32,
}

impl<'a> Script<'a> {
    fn new(wheel: &'a Wheel) -> Script<'a> {
        Script {
            wheel,
            steps: Vec::new(),
            x: 0.0,
            z: 0.0,
        }
    }

    /// One step: the wheel travels along +z at `speed`, pointed `slip` rad away from its
    /// direction of travel, leaned by `camber` rad and pressed down for `load` N on a road
    /// at height 0. `edit` then changes whatever else the scenario wants.
    fn push(
        &mut self,
        speed: f32,
        slip: f32,
        camber: f32,
        load: f32,
        edit: impl FnOnce(&mut StepInput, &Wheel),
    ) {
        let (sin_yaw, cos_yaw) = slip.sin_cos();
        let (sin_roll, cos_roll) = camber.sin_cos();
        let hub_y = self.wheel.radius - load / self.wheel.rate;
        let mut input = StepInput {
            #[rustfmt::skip]
            hub_matrix: [
                cos_yaw * cos_roll, sin_roll, -sin_yaw * cos_roll, 0.0,
                -cos_yaw * sin_roll, cos_roll, sin_yaw * sin_roll, 0.0,
                sin_yaw, 0.0, cos_yaw, 0.0,
                self.x, hub_y, self.z, 1.0,
            ],
            hub_velocity: [0.0, 0.0, speed],
            hub_angular_velocity: [0.0; 3],
            brake_torque: 0.0,
            hand_brake_torque: 0.0,
            electric_torque: 0.0,
            abs_override: 1.0,
            ai_mult: 1.0,
            driven: false,
            set_angular_velocity: None,
            set_blankets: None,
            ext_active: false,
            ext_load: 0.0,
            ext_slip_angle: 0.0,
            ext_slip_ratio: 0.0,
            has_hit: true,
            ground_y: 0.0,
            ground_normal: [0.0, 1.0, 0.0],
            grip_mod: 1.0,
            dirt_additive_k: 0.0,
            sin_height: 0.0,
            sin_length: 0.0,
            damping: 0.0,
            granularity: 0.0,
            has_car: true,
            torque_mode: 0,
            car_speed: speed,
            car_sleeping: false,
            dynamic_grip_level: 1.0,
            tyre_consumption_rate: 1.0,
            mechanical_damage_rate: 1.0,
            ambient_temperature: 22.0,
            road_temperature: 30.0,
            allow_tyre_blankets: false,
            body_velocity: [0.0, 0.0, speed],
            body_mass: 600.0,
        };
        edit(&mut input, self.wheel);
        self.steps.push(input);
        self.z += input.hub_velocity[2] * DT;
        self.x += input.hub_velocity[0] * DT;
    }
}

/// The inputs of a scenario, or `None` for an unknown name.
pub fn scenario(name: &str, axle: Axle, wheel: &Wheel) -> Option<Vec<StepInput>> {
    let front = axle == Axle::Front;
    let load0 = wheel.static_load;
    let rolling = |speed: f32, slip_ratio: f32| speed * (1.0 + slip_ratio) / wheel.radius;
    let mut s = Script::new(wheel);
    match name {
        // cold tyre from a standstill, then laps' worth of weaving with the load moving
        "warmup" => {
            for i in 0..6000 {
                let t = i as f32;
                let speed = if i < 1500 {
                    ramp(i, 0, 1500, 0.0, 55.0)
                } else {
                    55.0 + 10.0 * (t * 0.004).sin()
                };
                let slip = if i < 1500 {
                    0.0
                } else {
                    0.06 * (t * 0.01).sin()
                };
                let load = if i < 1000 {
                    load0
                } else {
                    load0 * (1.0 + 0.3 * (t * 0.02).sin())
                };
                let camber = if front { -0.035 } else { -0.02 };
                s.push(speed, slip, camber, load, |_, _| {});
            }
        }
        // hard braking into a locked wheel, an ABS-like release, then the handbrake
        "brake_lockup" => {
            for i in 0..4000 {
                let speed = if i < 500 {
                    70.0
                } else if i < 2800 {
                    (70.0 - 12.0 * (i - 500) as f32 * DT).max(5.0)
                } else {
                    ramp(i, 2800, 4000, 5.0, 30.0)
                };
                let braking = ramp(i, 500, 700, 0.0, 1.0) - ramp(i, 2200, 2400, 0.0, 1.0);
                let transfer = if front { 0.4 } else { -0.4 };
                let load = load0 * (1.0 + transfer * braking);
                s.push(speed, 0.01, -0.03, load, |input, _| {
                    input.torque_mode = 1;
                    if i == 0 {
                        input.set_angular_velocity = Some(rolling(speed, 0.0));
                    }
                    input.brake_torque = ramp(i, 500, 900, 0.0, 1500.0)
                        + ramp(i, 900, 1300, 0.0, 3000.0)
                        - ramp(i, 2200, 2400, 0.0, 4500.0);
                    if (1500..1700).contains(&i) {
                        input.abs_override = 0.3;
                    }
                    if (3000..3200).contains(&i) {
                        input.hand_brake_torque = 2000.0;
                    }
                });
            }
        }
        // standing start with far more torque than grip
        "wheelspin" => {
            for i in 0..4000 {
                let speed = ramp(i, 300, 3000, 0.0, 40.0);
                s.push(speed, 0.0, -0.02, load0, |input, _| {
                    input.torque_mode = 2;
                    if front {
                        // not driven: an electric motor's torque spins it up
                        // a burst above what the tyre can hold, then back under it
                        input.electric_torque = ramp(i, 100, 200, 0.0, 1650.0)
                            - ramp(i, 350, 450, 0.0, 650.0)
                            - ramp(i, 2000, 2300, 0.0, 700.0);
                    } else {
                        // driven: the drivetrain dictates the wheel speed
                        input.driven = true;
                        let spin = ramp(i, 150, 300, 0.0, 9.0) - ramp(i, 1200, 2400, 0.0, 8.5);
                        input.set_angular_velocity = Some((speed + spin) / wheel.radius);
                    }
                });
            }
        }
        // a long, loaded corner at constant speed
        "cornering" => {
            for i in 0..4000 {
                let slip = ramp(i, 0, 600, 0.0, 0.09);
                s.push(50.0, slip, -0.05, load0 * 1.6, |input, _| {
                    input.torque_mode = 2;
                    input.dynamic_grip_level = 0.98;
                    input.hub_angular_velocity = [0.0, 0.5, 0.0];
                    if i == 0 {
                        input.set_angular_velocity = Some(rolling(50.0, 0.0));
                    }
                    if !front {
                        input.driven = true;
                        input.set_angular_velocity = Some(rolling(50.0, 0.01));
                    }
                });
            }
        }
        // power on through a corner, then a sudden lift: load and slip angle change
        "liftoff" => {
            for i in 0..4000 {
                let lift = ramp(i, 1500, 1700, 0.0, 1.0);
                let transfer = if front { 0.15 } else { -0.25 };
                let slide = ramp(i, 1600, 2200, 0.0, 0.06) - ramp(i, 2600, 3200, 0.0, 0.06);
                let speed = 45.0 - 6.0 * ramp(i, 1500, 3000, 0.0, 1.0);
                s.push(
                    speed,
                    0.05 + slide,
                    -0.04,
                    load0 * (1.3 + transfer * lift),
                    |input, _| {
                        input.torque_mode = 1;
                        if i == 0 {
                            input.set_angular_velocity = Some(rolling(speed, 0.0));
                        }
                        if front {
                            input.electric_torque = 300.0 - 450.0 * lift;
                        } else {
                            input.driven = true;
                            input.set_angular_velocity = Some(rolling(speed, 0.04 - 0.07 * lift));
                        }
                    },
                );
            }
        }
        // tyre blankets: on in the pits, off once rolling, put back on later
        "blankets" => {
            for i in 0..4000 {
                let speed = if i < 2800 {
                    ramp(i, 600, 1600, 0.0, 30.0)
                } else {
                    ramp(i, 2800, 3200, 30.0, 1.0)
                };
                s.push(speed, 0.02, -0.03, load0, |input, _| {
                    input.allow_tyre_blankets = i < 3600;
                    input.ambient_temperature = 15.0;
                    input.road_temperature = 18.0;
                    if i == 2400 || i == 3300 {
                        input.set_blankets = Some(true);
                    }
                    if i == 3800 {
                        input.set_blankets = Some(false);
                    }
                });
            }
        }
        // a step up in the road, a drop (airborne), a kerb face, the rim, rough surfaces
        "bump" => {
            for i in 0..4000 {
                s.push(40.0, 0.01, -0.03, load0, |input, wheel| {
                    if i == 0 {
                        input.set_angular_velocity = Some(rolling(40.0, 0.0));
                    }
                    if (1000..1500).contains(&i) {
                        input.ground_y = 0.03;
                    }
                    if (2000..2100).contains(&i) {
                        input.ground_y = -0.25;
                    }
                    if (2600..2900).contains(&i) {
                        // the road tilts sideways, past the 16 degrees the tyre accepts
                        let tilt = ramp(i, 2600, 2750, 0.0, 0.4) - ramp(i, 2750, 2900, 0.0, 0.4);
                        input.ground_normal = [tilt.sin(), tilt.cos(), 0.0];
                    }
                    if (3200..3300).contains(&i) {
                        // high enough to squash the tyre onto the rim
                        let hub_y = input.hub_matrix[13];
                        input.ground_y = hub_y - wheel.rim_radius + 0.004;
                    }
                    if (3400..3700).contains(&i) {
                        input.sin_height = 0.01;
                        input.sin_length = 3.0;
                    }
                    if (3700..3900).contains(&i) {
                        input.granularity = 1.0;
                        input.grip_mod = 0.6;
                        input.dirt_additive_k = 1.0;
                        input.damping = 0.05;
                    }
                });
            }
        }
        // a long burnout on a hot day: blistering, then a stripe passes the explosion
        // temperature and the tyre deflates; at the end the hub is lowered onto the rim
        "overheat" => {
            for i in 0..6000 {
                let drop = ramp(i, 5200, 5400, 0.0, 1.0);
                let load =
                    load0 * 1.5 + drop * (wheel.radius - wheel.rim_radius + 0.004) * wheel.rate;
                s.push(30.0, 0.12, -0.03, load, |input, _| {
                    input.ambient_temperature = 40.0;
                    input.road_temperature = 55.0;
                    if front {
                        input.electric_torque = ramp(i, 50, 300, 0.0, 9000.0);
                    } else {
                        input.driven = true;
                        input.set_angular_velocity =
                            Some(rolling(30.0, ramp(i, 50, 300, 0.0, 1.5)));
                    }
                });
            }
        }
        // everything at once, in 400-step segments with different settings
        "random" => {
            let mut rng = Rng(if front { 0x5eed_f00d } else { 0x5eed_beef });
            let (mut speed, mut slip, mut camber, mut load_factor) =
                (20.0f32, 0.0f32, 0.0f32, 1.0f32);
            let (mut brake, mut electric, mut slip_ratio) = (0.0f32, 0.0f32, 0.0f32);
            let mut target = [20.0f32, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0];
            let mut segment = s.steps.first().copied();
            for i in 0..8000 {
                if i % 400 == 0 {
                    // new segment: constant settings, taken from a template step
                    s.push(0.0, 0.0, 0.0, load0, |input, _| {
                        input.torque_mode = (rng.next() % 3) as i32;
                        input.driven = rng.chance(0.4);
                        input.has_car = !rng.chance(0.1);
                        input.ai_mult = [1.0, 1.0, 1.0, 1.0, 1.2, 0.9][(rng.next() % 6) as usize];
                        input.tyre_consumption_rate =
                            [1.0, 1.0, 30.0, 300.0, 0.0][(rng.next() % 5) as usize];
                        input.mechanical_damage_rate = [1.0, 1.0, 0.0][(rng.next() % 3) as usize];
                        input.allow_tyre_blankets = rng.chance(0.2);
                        input.ambient_temperature = rng.range(5.0, 40.0);
                        input.road_temperature = input.ambient_temperature + rng.range(0.0, 20.0);
                        input.dynamic_grip_level = rng.range(0.9, 1.0);
                        input.grip_mod = [1.0, 1.0, 1.0, 0.95, 0.9, 0.6][(rng.next() % 6) as usize];
                        input.dirt_additive_k = if rng.chance(0.25) {
                            rng.range(0.1, 2.0)
                        } else {
                            0.0
                        };
                        if rng.chance(0.2) {
                            input.sin_height = rng.range(0.001, 0.02);
                            input.sin_length = rng.range(0.5, 6.0);
                        }
                        input.granularity = if rng.chance(0.15) { 1.0 } else { 0.0 };
                        input.damping = if rng.chance(0.15) {
                            rng.range(0.01, 0.2)
                        } else {
                            0.0
                        };
                        input.ext_active = rng.chance(0.06);
                        input.car_sleeping = rng.chance(0.05);
                        input.body_mass = rng.range(500.0, 1500.0);
                    });
                    segment = s.steps.pop();
                    target = [
                        if rng.chance(0.15) {
                            rng.range(0.0, 3.0)
                        } else {
                            rng.range(0.0, 90.0)
                        },
                        rng.range(-0.25, 0.25) * rng.unit(),
                        rng.range(-0.12, 0.12),
                        if rng.chance(0.1) {
                            -0.5
                        } else {
                            rng.range(0.1, 2.5)
                        },
                        if rng.chance(0.3) {
                            rng.range(0.0, 5000.0)
                        } else {
                            0.0
                        },
                        if rng.chance(0.4) {
                            rng.range(-600.0, 900.0)
                        } else {
                            0.0
                        },
                        if rng.chance(0.5) {
                            rng.range(-0.3, 0.3)
                        } else {
                            rng.range(-1.2, 1.5) * rng.unit()
                        },
                    ];
                }
                let settings = segment.expect("the first step starts a segment");
                speed += (target[0] - speed) * 0.01 + rng.range(-0.05, 0.05);
                slip += (target[1] - slip) * 0.02 + rng.range(-0.002, 0.002);
                camber += (target[2] - camber) * 0.02 + rng.range(-0.001, 0.001);
                load_factor += (target[3] - load_factor) * 0.02 + rng.range(-0.02, 0.02);
                brake += (target[4] - brake) * 0.03;
                electric += (target[5] - electric) * 0.03;
                slip_ratio += (target[6] - slip_ratio) * 0.02 + rng.range(-0.003, 0.003);
                let speed_now = speed.max(0.0);
                let upside_down = i % 400 > 380 && (i / 400) % 7 == 3;
                let no_hit = rng.chance(0.01);
                let tilt = if rng.chance(0.03) {
                    rng.range(-0.45, 0.45)
                } else {
                    rng.range(-0.05, 0.05)
                };
                let lateral = rng.range(-0.3, 0.3);
                let vertical = rng.range(-0.2, 0.2);
                let avel = [
                    rng.range(-0.3, 0.3),
                    rng.range(-1.0, 1.0),
                    rng.range(-0.3, 0.3),
                ];
                let ground_y = if rng.chance(0.02) {
                    rng.range(-0.05, 0.05)
                } else {
                    0.0
                };
                let ext = [
                    rng.range(0.0, 8000.0),
                    rng.range(-0.3, 0.3),
                    rng.range(-1.0, 1.0),
                ];
                let set_blankets = if rng.chance(0.002) {
                    Some(rng.chance(0.5))
                } else {
                    None
                };
                let roll = if upside_down { 2.6 } else { camber };
                s.push(speed_now, slip, roll, load0 * load_factor, |input, _| {
                    let pose = (input.hub_matrix, input.hub_velocity, input.car_speed);
                    *input = settings;
                    input.hub_matrix = pose.0;
                    input.hub_velocity = [lateral, vertical, pose.1[2]];
                    input.car_speed = pose.2;
                    input.body_velocity = input.hub_velocity;
                    input.hub_angular_velocity = avel;
                    input.brake_torque = brake.max(0.0);
                    input.electric_torque = electric;
                    input.has_hit = !no_hit;
                    input.ground_y = ground_y;
                    input.ground_normal = [tilt.sin(), tilt.cos(), 0.0];
                    input.ext_load = ext[0];
                    input.ext_slip_angle = ext[1];
                    input.ext_slip_ratio = ext[2];
                    input.set_blankets = set_blankets;
                    input.set_angular_velocity = input
                        .driven
                        .then(|| speed_now * (1.0 + slip_ratio) / wheel.radius);
                });
            }
        }
        // like the game's own tyre test bench: no car, load and slip given directly
        "tester" => {
            for i in 0..2000 {
                s.push(20.0, 0.0, 0.0, load0, |input, _| {
                    input.has_car = false;
                    input.ext_active = true;
                    input.ext_load = 1000.0 + 3000.0 * (i / 500) as f32;
                    let sweep = (i % 500) as f32 / 500.0 * 2.0 - 1.0;
                    if (i / 250) % 2 == 0 {
                        input.ext_slip_angle = sweep * 0.35;
                    } else {
                        input.ext_slip_ratio = sweep;
                    }
                    if i % 700 == 0 {
                        input.set_angular_velocity = Some(rolling(20.0, 0.0));
                    }
                });
            }
        }
        // one-step NaN / infinity injections into each input in turn, with a short
        // airborne moment after each one (which is what clears a poisoned slip value)
        "nan_inputs" => {
            for i in 0..2400 {
                s.push(30.0, 0.03, -0.02, load0, |input, _| {
                    if i == 0 {
                        input.set_angular_velocity = Some(rolling(30.0, 0.0));
                    }
                    let phase = i % 200;
                    if phase == 150 || phase == 151 {
                        input.ground_y = -1.0;
                    }
                    if phase != 100 {
                        return;
                    }
                    match i / 200 {
                        0 => input.hub_velocity[2] = f32::NAN,
                        1 => input.hub_velocity[0] = f32::INFINITY,
                        2 => input.set_angular_velocity = Some(f32::NAN),
                        3 => input.hub_matrix[5] = f32::NAN,
                        4 => input.ground_normal = [f32::NAN, 1.0, 0.0],
                        5 => input.brake_torque = f32::NAN,
                        6 => input.ground_y = f32::NAN,
                        7 => input.hub_angular_velocity[0] = f32::NEG_INFINITY,
                        8 => input.electric_torque = f32::INFINITY,
                        9 => input.grip_mod = f32::NAN,
                        10 => input.hub_matrix[12] = f32::INFINITY,
                        _ => input.dynamic_grip_level = f32::NAN,
                    }
                });
            }
        }
        // a little of everything in 500 steps, for the checked-in golden test
        "golden_mix" => {
            for i in 0..500 {
                let speed = ramp(i, 30, 200, 0.0, 28.0) - ramp(i, 230, 300, 0.0, 14.0);
                let slip = ramp(i, 90, 160, 0.0, 0.07) - ramp(i, 300, 340, 0.0, 0.05);
                let load = load0 * (1.0 + 0.4 * ramp(i, 200, 240, 0.0, 1.0));
                s.push(speed, slip, -0.03, load, |input, wheel| {
                    input.torque_mode = [2, 1, 0, 2, 1][i / 100];
                    input.driven = !front && (100..300).contains(&i);
                    if input.driven {
                        input.set_angular_velocity = Some(rolling(speed, 0.06));
                    }
                    input.electric_torque =
                        ramp(i, 5, 40, 0.0, 1500.0) - ramp(i, 60, 90, 0.0, 1300.0);
                    input.brake_torque =
                        ramp(i, 215, 235, 0.0, 4000.0) - ramp(i, 280, 300, 0.0, 4000.0);
                    input.hub_angular_velocity = [0.0, 0.3, 0.0];
                    input.mechanical_damage_rate = 1.0;
                    input.tyre_consumption_rate = 25.0;
                    input.allow_tyre_blankets = (400..460).contains(&i);
                    if i == 410 {
                        input.set_blankets = Some(true);
                    }
                    if (320..340).contains(&i) {
                        input.ground_y = -0.2;
                    }
                    if (345..350).contains(&i) {
                        input.ground_y = input.hub_matrix[13] - wheel.rim_radius + 0.003;
                    }
                    if (350..375).contains(&i) {
                        let tilt = ramp(i, 350, 362, 0.0, 0.4) - ramp(i, 362, 375, 0.0, 0.4);
                        input.ground_normal = [tilt.sin(), tilt.cos(), 0.0];
                    }
                    if (380..430).contains(&i) {
                        input.grip_mod = 0.7;
                        input.dirt_additive_k = 1.5;
                        input.damping = 0.08;
                        input.granularity = 1.0;
                        input.sin_height = 0.008;
                        input.sin_length = 2.5;
                    }
                    if (440..455).contains(&i) {
                        input.ai_mult = 1.2;
                    }
                    if (460..475).contains(&i) {
                        input.ext_active = true;
                        input.ext_load = 5000.0;
                        input.ext_slip_angle = 0.1;
                        input.ext_slip_ratio = -0.2;
                    }
                    if (480..486).contains(&i) {
                        input.has_hit = false;
                    }
                    input.has_car = !(490..500).contains(&i);
                });
            }
        }
        _ => return None,
    }
    Some(s.steps)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scenarios_are_deterministic_and_long_enough() {
        let wheel = Wheel::default();
        for name in SCENARIOS {
            for axle in [Axle::Front, Axle::Rear] {
                let a = scenario(name, axle, &wheel).unwrap();
                let b = scenario(name, axle, &wheel).unwrap();
                assert!(a.len() >= 500, "{name}: {} steps", a.len());
                let words = |steps: &[StepInput]| -> Vec<Vec<u64>> {
                    steps.iter().map(StepInput::to_words).collect()
                };
                assert!(words(&a) == words(&b), "{name} is not deterministic");
            }
        }
        assert!(scenario("nope", Axle::Front, &wheel).is_none());
    }
}
