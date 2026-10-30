// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! `DriftModeComponent` (`Car+0x3510`, 0x50 bytes): the player's car's drift points, worked
//! out every physics step from the body's slip angle and speed (`step` 0x1402bff20). The
//! `DriftMode` of a drift session reads them through `CarPhysicsState` (`driftPoints`,
//! `instantDrift`, `isDriftValid`, `driftComboCounter`, `driftBonusOn`).

/// `DriftModeComponent::init` @ 0x1402bfda0 (in every `Car::Car`; `TyreSmoke::TyreSmoke` reads
/// the same): `system/cfg/tyre_smoke.ini [TRIGGERS] SLIP_LEVEL` of the install into the static
/// `triggerSlipLevel`, 0 without the file or the key.
pub fn trigger_slip_level(root: &std::path::Path) -> f32 {
    crate::data::ini::IniReader::load(&root.join("system/cfg/tyre_smoke.ini")).ok().map_or(0.0, |ini| ini.get_float("TRIGGERS", "SLIP_LEVEL").unwrap_or(0.0))
}

const BETA_START: f32 = f32::from_bits(0x3e06_09fe); // 7.5 degrees
const BETA_COMBO: f32 = f32::from_bits(0x3e86_09fe); // 15 degrees
const BETA_STRAIGHT: f32 = f32::from_bits(0x3d86_09fe); // 3.75 degrees
const PER_KMH: f32 = f32::from_bits(0x3c7c_0fc1); // 1/65

/// What the step reads of one tyre: its surface's `gripMod` and `dirtAdditiveK` (none: no
/// surface under it), and `TyreStatus` load, slip ratio and angular velocity.
#[derive(Clone, Copy, Debug, Default)]
pub struct DriftTyre {
    pub surface: Option<(f32, f32)>,
    pub load: f32,
    pub slip_ratio: f32,
    pub angular_velocity: f32,
}

/// What the step reads of the car.
#[derive(Clone, Copy, Debug, Default)]
pub struct DriftInput {
    /// the body's velocity in its own frame (`getLocalVelocity`)
    pub local_velocity: [f32; 3],
    /// `Car::speed`, m/s
    pub speed: f32,
    pub gear: i32,
    pub damage_zone_level: [f32; 5],
    pub tyres: [DriftTyre; 4],
    /// the static `triggerSlipLevel` ([`trigger_slip_level`])
    pub trigger_slip_level: f32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct DriftModeComponent {
    pub drift_straight_timer: f32,
    pub points: f64,
    pub instant_drift: f64,
    pub current_speed_multiplier: f64,
    pub current_drift_angle: f64,
    pub combo_counter: i32,
    pub last_drift_direction: i32,
    pub extreme_drifting: bool,
    pub drifting: bool,
    pub invalid: bool,
    pub old_damage_zones: [f32; 5],
}

/// `x > 0` 1, `x == 0` 0, else -1 (a NaN too), as the game's compares fall.
fn sign(x: f32) -> f32 {
    if x > 0.0 {
        1.0
    } else if x >= 0.0 {
        0.0
    } else {
        -1.0
    }
}

/// `ucomiss` + `je`: a NaN counts as equal.
fn differs(a: f32, b: f32) -> bool {
    a < b || a > b
}

/// `RaceEngineer::getBetaRAD` 0x14027ba90, made positive: the body's slip angle.
pub fn beta(v: [f32; 3]) -> f32 {
    let len = crate::math::sqrtf(v[1] * v[1] + v[0] * v[0] + v[2] * v[2]);
    let x = if len != 0.0 { v[0] / len } else { v[0] };
    let beta = if x > -1.0 && x < 1.0 { crate::math::asinf(x) } else { std::f32::consts::FRAC_PI_2 };
    beta.abs()
}

impl DriftModeComponent {
    /// `validateDrift` 0x1402c0270: sets `invalid` (never clears it).
    fn validate(&mut self, f: &DriftInput) {
        let dirt = f.tyres.iter().filter(|t| t.surface.is_some_and(|(_, dirt)| 0.001 < dirt)).count();
        let mut invalidate = true;
        if dirt <= 2 && f.speed * 3.6 >= 20.0 {
            let mut changed = false;
            for i in 0..4 {
                if differs(f.damage_zone_level[i], self.old_damage_zones[i]) {
                    self.old_damage_zones[i] = f.damage_zone_level[i];
                    changed = true;
                }
            }
            if differs(f.damage_zone_level[4], self.old_damage_zones[4]) {
                self.old_damage_zones[4] = f.damage_zone_level[4];
            } else if !changed && f.gear != 0 {
                invalidate = false;
            }
        }
        if invalidate {
            self.invalid = true;
        }
    }

    /// `checkExtremeDrifting` 0x1402bfc50: two tyres or more spinning hard on grip.
    fn extreme(f: &DriftInput) -> bool {
        let n = f.tyres.iter().filter(|t| {
            t.surface.is_some_and(|(grip, _)| t.angular_velocity.abs() > 4.0 && t.slip_ratio.abs() > f.trigger_slip_level && !(10.0 >= t.load) && !(0.9 > grip))
        });
        n.count() > 1
    }

    /// `step` 0x1402bff20, once a physics step.
    pub fn step(&mut self, dt: f32, f: &DriftInput) {
        self.validate(f);
        let beta = beta(f.local_velocity);
        let kmh = f.speed * 3.6;
        let x = f.local_velocity[0];
        if kmh > 20.0 && beta > BETA_START {
            if !self.drifting {
                self.last_drift_direction = sign(x) as i32;
                self.combo_counter = 1;
                self.invalid = false;
                self.instant_drift = 0.0;
            }
            self.current_drift_angle = (beta - BETA_START) as f64;
            let m = ((kmh - 20.0) * PER_KMH) as f64;
            self.current_speed_multiplier = if m > 2.0 {
                2.0
            } else if m >= 0.0 {
                m
            } else {
                0.0
            };
            self.extreme_drifting = Self::extreme(f);
            let mut t = self.current_speed_multiplier * self.current_drift_angle;
            if self.extreme_drifting {
                t *= 2.0;
            }
            self.instant_drift += t;
            if x.abs() > 4.0 && differs(self.last_drift_direction as f32, sign(x)) && beta > BETA_COMBO {
                self.instant_drift += 50.0;
                self.combo_counter += 1;
                self.last_drift_direction = sign(x) as i32;
            }
            self.drifting = true;
            self.drift_straight_timer = 0.0;
        }
        if self.drifting {
            self.drift_straight_timer = if kmh > 20.0 && !(beta >= BETA_STRAIGHT) { dt + self.drift_straight_timer } else { 0.0 };
            if !self.invalid && !(1.0 >= self.drift_straight_timer) {
                self.combo_counter = 0;
                self.points += self.instant_drift;
                self.drifting = false;
                self.instant_drift = 0.0;
            }
        }
        if self.invalid {
            self.current_drift_angle = 0.0;
            self.current_speed_multiplier = 0.0;
            self.extreme_drifting = false;
            self.drifting = false;
            self.instant_drift = 0.0;
            self.combo_counter = 0;
        }
    }
}
