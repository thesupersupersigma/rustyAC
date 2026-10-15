// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! `ACDCarEngineer` (0x1f0), the developers' "Car engineer": the player's car's masses, ride
//! frequencies and damping ratios, from `RaceEngineer`.
//!
//! `ACDCarEngineer::ACDCarEngineer(Sim*)` 0x14031a0f0, `render` 0x14031a750;
//! `RaceEngineer::getFrontNaturalFrequencyHZ` 0x14027bf20, `getRearNaturalFrequencyHZ`
//! 0x14027c8f0, `getFrontDampingRatio` 0x14027bd60, `getRearDampingRatio` 0x14027c6d0,
//! `getWeightDistribution` 0x14027cb90.

use std::any::Any;

use crate::acos::{format_g, format_g6};
use crate::apps::App;
use crate::control::Id;
use crate::gui::Gui;
use crate::hud::translate;
use crate::painter::Painter;

/// What the app reads of the player's car: `physicsInfo.totalMass`, `bodyMass`,
/// `unsprungWeights` (set once at the start), and for `RaceEngineer` suspensions 0 and 2's `getK`,
/// `getBasePosition().z`, `getDamper()`'s `reboundSlow` and `bumpSlow`, the body's `getMass`,
/// front heave spring's `k`, `reboundSlow`, `bumpSlow` (None: not present).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CarEngineer {
    pub total_mass: f32,
    pub body_mass: f32,
    pub unsprung: [f32; 4],
    pub k: [f32; 2],
    pub z: [f32; 2],
    pub mass: f32,
    pub dampers: [[f32; 2]; 2],
    pub heave: Option<[f32; 3]>,
}

const INV_2PI: u32 = 0x3e23_67ef;

/// `ucomiss x, 0.0; jne`: zero or NaN.
fn is_zero(x: f32) -> bool {
    x == 0.0 || x.is_nan()
}

impl CarEngineer {
    fn ratio(&self) -> f32 {
        self.z[0].abs() / (self.z[0] - self.z[1]).abs()
    }

    /// `getFrontNaturalFrequencyHZ` 0x14027bf20.
    pub fn front_frequency(&self) -> f32 {
        let mut k = self.k[0];
        if let (true, Some(h)) = (is_zero(k), self.heave) {
            k = h[0];
        }
        let a = 1.0 - self.ratio();
        let t = (a * self.mass) * 0.5;
        (k / t).sqrt() * f32::from_bits(INV_2PI)
    }

    /// `getRearNaturalFrequencyHZ` 0x14027c8f0.
    pub fn rear_frequency(&self) -> f32 {
        let k = self.k[1];
        let a = 1.0 - (1.0 - self.ratio());
        let t = (a * self.mass) * 0.5;
        (k / t).sqrt() * f32::from_bits(INV_2PI)
    }

    /// `getFrontDampingRatio` 0x14027bd60: (rebound, bump).
    pub fn front_damping(&self) -> [f32; 2] {
        let (mut k, mut bump, mut rebound) = (self.k[0], self.dampers[0][1], self.dampers[0][0]);
        if let (true, Some(h)) = (is_zero(k), self.heave) {
            (k, rebound, bump) = (h[0], h[1], h[2]);
        }
        let r = self.ratio();
        let a = (((1.0 - r) * self.mass) * 0.5) * k;
        // getWeightDistribution
        let wd = 1.0 - self.ratio();
        let b = ((self.mass * wd) * 0.5) * k;
        [rebound / (b.sqrt() * 2.0), bump / (a.sqrt() * 2.0)]
    }

    /// `getRearDampingRatio` 0x14027c6d0: (rebound, bump).
    pub fn rear_damping(&self) -> [f32; 2] {
        let (k, bump, rebound) = (self.k[1], self.dampers[1][1], self.dampers[1][0]);
        let a1 = (((1.0 - (1.0 - self.ratio())) * self.mass) * 0.5) * k;
        let a2 = (((1.0 - (1.0 - self.ratio())) * self.mass) * 0.5) * k;
        [rebound / (a2.sqrt() * 2.0), bump / (a1.sqrt() * 2.0)]
    }
}

pub struct CarEngineerApp {
    lines: [Id; 6],
}

impl Gui {
    /// `ACDCarEngineer::ACDCarEngineer(Sim*)` 0x14031a0f0: six plain controls.
    pub fn new_car_engineer_app(&mut self, painter: &mut dyn Painter) -> Id {
        let form = self.new_form(painter, "ACDCarEngineer", false);
        self.form_set_title(form, &translate("Car engineer"));
        self.form_data_mut(form).dev_app = true;
        let mut lines = [0; 6];
        for (i, (name, y)) in [("CAR_MASS", 50.0), ("SPRUNG_WEIGHT", 75.0), ("UNSPRUNG_WEIGHT", 100.0), ("FREQUENCY", 125.0), ("DAMP F", 150.0), ("DAMP R", 175.0)].into_iter().enumerate() {
            let c = self.new_control(painter, name);
            self.set_position(c, 10.0, y);
            self.set_size(c, 450.0, 20.0);
            if i == 0 {
                self.set_text(c, "CAR MASS:");
            }
            self.control_add_control(form, c);
            lines[i] = c;
        }
        self.form_set_size(form, 500.0, 205.0);
        self.set_app(form, Box::new(CarEngineerApp { lines }));
        form
    }
}

impl App for CarEngineerApp {
    fn as_any(&mut self) -> &mut dyn Any {
        self
    }

    /// `ACDCarEngineer::render` 0x14031a750: drawn first, so the texts are a frame old.
    fn render(&mut self, gui: &mut Gui, id: Id, dt: f32, painter: &mut dyn Painter) {
        gui.control_render(id, dt, painter);
        let e = gui.apps.car_engineer;
        let l = self.lines;
        gui.set_text(l[0], &format!("{}:{} Kg", translate("CAR MASS"), format_g6(e.total_mass)));
        gui.set_text(l[1], &format!("{}:{} Kg", translate("SPRUNG"), format_g6(e.body_mass)));
        let u = ((e.unsprung[1] + e.unsprung[0]) + e.unsprung[2]) + e.unsprung[3];
        gui.set_text(l[2], &format!("{}:{} Kg", translate("UNSPRUNG"), format_g6(u)));
        let g2 = |v: f32| format_g(v as f64, 2);
        let rear = e.rear_frequency();
        let front = e.front_frequency();
        gui.set_text(l[3], &format!("{} F:{} Hz R:{} Hz", translate("FREQUENCY"), g2(front), g2(rear)));
        let fd = e.front_damping();
        let rd = e.rear_damping();
        gui.set_text(l[4], &format!("{}:{}    {}:{}", translate("DAMP FRONT BUMP"), g2(fd[1]), translate("REBOUND"), g2(fd[0])));
        gui.set_text(l[5], &format!("{}:{}    {}:{}", translate("DAMP REAR BUMP"), g2(rd[1]), translate("REBOUND"), g2(rd[0])));
    }
}
