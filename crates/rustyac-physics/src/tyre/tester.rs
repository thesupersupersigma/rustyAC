// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! `TyreTester` (0x128): one tyre of the car on a stand-in hub and ground, swept over slip
//! angle and slip ratio for the developers' "Tyre tester" app. The hub turns by the slip angle
//! and moves at `velocity` along z; the ground sits where the tyre's spring holds `load`.
//!
//! `TyreTester::TyreTester` 0x14044ee40, `slipAngleSweep` 0x14044f300, `slipRatioSweep`
//! 0x14044f710, `srSASweep` 0x14044fb00, the hub's slots 0x14044f160 / 0x14044f250 /
//! 0x14044f260, the ground's `rayCast` 0x14044f270.

use std::path::Path;

use rustyac_math::cosf;

use super::{RayCastResult, RayTrackCollisionProvider, SurfaceDef, Suspension, VanillaTyre};
use crate::vecmath::{xm_matrix_multiply, Mat44f, Vec3f};

/// The step every sweep value gets (`Tyre::step(0.003)`).
const DT: f32 = 0.003;
const DEG_TO_RAD: f32 = 0.017453;

/// `TyreTestFrame`: the tyre's forces after one step, the slip angle in degrees.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TyreTestFrame {
    pub fy: f32,
    pub fx: f32,
    pub mz: f32,
    pub slip_angle: f32,
    pub slip_ratio: f32,
}

/// The tester as the tyre's `ISuspension`.
struct TesterHub {
    slip_angle: f32,
    velocity: f32,
}

impl Suspension for TesterHub {
    /// 0x14044f160: a turn about y by the slip angle, no translation.
    fn get_hub_world_matrix(&mut self) -> Mat44f {
        xm_matrix_multiply(&Mat44f::create_from_axis_angle(&Vec3f::new(0.0, 1.0, 0.0), self.slip_angle * DEG_TO_RAD), &Mat44f::IDENTITY)
    }
    /// 0x14044f250
    fn get_point_velocity(&mut self, _p: &Vec3f) -> Vec3f {
        Vec3f::new(0.0, 0.0, self.velocity)
    }
    fn add_force_at_pos(&mut self, _force: &Vec3f, _pos: &Vec3f, _driven: bool, _add_to_steer_torque: bool) {}
    fn add_torque(&mut self, _torque: &Vec3f) {}
    fn get_hub_angular_velocity(&mut self) -> Vec3f {
        Vec3f::new(0.0, 0.0, 0.0)
    }
    fn add_local_force_and_torque(&mut self, _force: &Vec3f, _torque: &Vec3f, _drive_torque: &Vec3f) {}
}

/// The tester as the tyre's ground (0x14044f270), with the tyre's values of before the step
/// (the step writes `pressureDynamic` only after its ray cast).
struct TesterGround {
    pressure_dynamic: f32,
    ideal_pressure: f32,
    spring_gain: f32,
    k: f32,
    radius: f32,
    load: f32,
    surf: SurfaceDef,
}

impl RayTrackCollisionProvider for TesterGround {
    fn ray_cast(&self, _org: &Vec3f, _dir: &Vec3f, _length: f32) -> Option<RayCastResult> {
        let d = (self.pressure_dynamic - self.ideal_pressure) * self.spring_gain + self.k;
        let y = self.load / d - self.radius;
        Some(RayCastResult { surface_def: self.surf, pos: Vec3f::new(0.0, y, 0.0), normal: Vec3f::new(0.0, 1.0, 0.0) })
    }
}

pub struct TyreTester {
    pub tyre: VanillaTyre,
    /// degrees
    pub slip_angle: f32,
    /// m/s
    pub velocity: f32,
    pub load: f32,
    surf: SurfaceDef,
}

impl TyreTester {
    /// `TyreTester(path, index)` 0x14044ee40: the car's tyre `index` from `data_path`
    /// (`content/cars/<car>/data/`), 20 m/s, no load.
    pub fn new(data_path: &Path, index: i32) -> Result<TyreTester, String> {
        let surf = SurfaceDef { vibration_length: 1.5, grip_mod: 1.0, dirt_additive_k: 0.0, ..SurfaceDef::default() };
        let mut tester = TyreTester { tyre: VanillaTyre::new(), slip_angle: 0.0, velocity: 20.0, load: 0.0, surf };
        let mut hub = tester.hub();
        tester.tyre.init(&mut hub, data_path, index, None)?;
        Ok(tester)
    }

    fn hub(&self) -> TesterHub {
        TesterHub { slip_angle: self.slip_angle, velocity: self.velocity }
    }

    /// `Tyre::setCompound` with the tester as the hub.
    pub fn set_compound(&mut self, index: i32) {
        let mut hub = self.hub();
        self.tyre.set_compound(index, &mut hub, None);
    }

    /// Every sweep's start: the load, `Tyre::reset`, the external inputs on with the load.
    fn begin(&mut self, load: f32) {
        self.load = load;
        let mut hub = self.hub();
        self.tyre.reset(&mut hub, None);
        self.tyre.external_inputs.is_active = true;
        self.tyre.external_inputs.load = self.load;
    }

    /// One `Tyre::step(0.003)` and its frame.
    fn step(&mut self) -> TyreTestFrame {
        let t = &self.tyre;
        let ground = TesterGround { pressure_dynamic: t.status.pressure_dynamic, ideal_pressure: t.model_data.ideal_pressure, spring_gain: t.model_data.pressure_spring_gain, k: t.data.k, radius: t.data.radius, load: self.load, surf: self.surf };
        let mut hub = self.hub();
        self.tyre.step(DT, &mut hub, Some(&ground), None);
        let s = &self.tyre.status;
        TyreTestFrame { fy: s.fy, fx: s.fx, mz: s.mz, slip_angle: s.slip_angle_rad * 57.29578, slip_ratio: s.slip_ratio }
    }

    /// `slipAngleSweep` 0x14044f300: `n` slip angles from `a` (degrees), no slip ratio.
    pub fn slip_angle_sweep(&mut self, a: f32, b: f32, load: f32, n: i32) -> Vec<TyreTestFrame> {
        self.begin(load);
        self.tyre.external_inputs.slip_ratio = 0.0;
        let step = (b - a) / n as f32;
        (0..n)
            .map(|i| {
                self.tyre.status.grain = 0.0;
                self.tyre.status.blister = 0.0;
                let sa = i as f32 * step + a;
                self.slip_angle = sa;
                self.tyre.external_inputs.slip_angle = sa * DEG_TO_RAD;
                self.tyre.status.angular_velocity = cosf(self.slip_angle * DEG_TO_RAD).abs() * (self.velocity / self.tyre.data.radius);
                self.step()
            })
            .collect()
    }

    /// `slipRatioSweep` 0x14044f710: `n` slip ratios from `a`, no slip angle; Fx negated.
    pub fn slip_ratio_sweep(&mut self, a: f32, b: f32, load: f32, n: i32) -> Vec<TyreTestFrame> {
        self.begin(load);
        self.tyre.external_inputs.slip_angle = 0.0;
        let step = (b - a) / n as f32;
        let a1 = a + 1.0;
        (0..n)
            .map(|i| {
                let s = i as f32 * step;
                self.slip_angle = 0.0;
                self.tyre.external_inputs.slip_ratio = s + a;
                self.tyre.status.angular_velocity = (self.velocity / self.tyre.data.radius) * (a1 + s);
                let f = self.step();
                TyreTestFrame { fx: -f.fx, ..f }
            })
            .collect()
    }

    /// `srSASweep` 0x14044fb00: `n` slip ratios from `a` at the slip angle `sa_deg`.
    pub fn sr_sa_sweep(&mut self, sa_deg: f32, a: f32, b: f32, load: f32, n: i32) -> Vec<TyreTestFrame> {
        self.begin(load);
        let sa_rad = sa_deg * DEG_TO_RAD;
        let step = (b - a) / n as f32;
        let a1 = a + 1.0;
        (0..n)
            .map(|i| {
                self.slip_angle = sa_deg;
                self.tyre.external_inputs.slip_angle = sa_rad;
                let s = i as f32 * step;
                self.tyre.external_inputs.slip_ratio = s + a;
                self.tyre.status.angular_velocity = cosf(self.slip_angle * DEG_TO_RAD).abs() * ((self.velocity / self.tyre.data.radius) * (a1 + s));
                self.step()
            })
            .collect()
    }
}
