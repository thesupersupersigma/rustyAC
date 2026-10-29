// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! `SetupDebug` (0x1c8), the developers' "Setup debug": a red text dump of the focused car's
//! physics state and a few of its `Car`'s members (tyre pressures, wing angles, rod lengths,
//! gear ratios).
//!
//! `SetupDebug::SetupDebug(Sim*)` 0x140317a80, `render` 0x140318010, `printString` 0x140317cb0,
//! `printStringList` 0x140317d90, `vec3f::toString` 0x1400533f0.

use std::any::Any;

use crate::acos::format_g6;
use crate::apps::App;
use crate::control::Id;
use crate::font::{Font, FontType};
use crate::gui::Gui;
use crate::painter::{Painter, ALIGN_LEFT};

/// What the app reads of the focused car's `Car` (`CarAvatar::raceEngineer->car`) and its
/// avatar's `physicsInfo`: `tyres[i].status.pressureStatic`, `aeroMap.wings[w].status.angle`,
/// `suspensions[i]->rodLength`, `drivetrain.gears[k].ratio` (all of them: R, N, 1st, ...),
/// `drivetrain.finalRatio`, `physicsInfo.maxGear`, `getTyreCompound(0, true)`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SetupDebugCar {
    pub pressure_static: [f32; 4],
    pub wing_angles: Vec<f32>,
    pub rod_lengths: [f32; 4],
    pub gear_ratios: Vec<f64>,
    pub final_ratio: f32,
    pub max_gear: i32,
    pub tyre_compound: String,
}

pub struct SetupDebug {
    y: f32,
}

impl Gui {
    /// `SetupDebug::SetupDebug(Sim*)` 0x140317a80: the raw title, its own red Consolas 20.
    pub fn new_setup_debug_app(&mut self, painter: &mut dyn Painter) -> Id {
        let form = self.new_form(painter, "SETUP_DEBUG_APP", false);
        self.form_set_title(form, "Setup debug");
        self.form_set_size(form, 400.0, 400.0);
        self.fonts.push(Font::new(painter, "", FontType::Monospaced, 20.0, false, false));
        let font = self.fonts.len() - 1;
        self.controls[form].font = Some(font);
        self.fonts[font].set_color(1.0, 0.0, 0.0, 1.0);
        self.form_set_size(form, 500.0, 1000.0);
        self.set_app(form, Box::new(SetupDebug { y: 0.0 }));
        form
    }
}

/// `printStringList`'s rounding: `value * powf(10, 4)` floored (truncated, one less when
/// negative and not whole), divided back.
pub(crate) fn floor4(value: f32) -> f32 {
    let p = 10000.0f32;
    let mut v = value * p;
    let n = v as i32;
    if n != i32::MIN && n as f32 != v {
        v = (n - v.is_sign_negative() as i32) as f32;
    }
    v / p
}

/// `vec3f::toString` 0x1400533f0.
fn vec3(v: [f32; 3]) -> String {
    format!("{} ,{} ,{}", format_g6(v[0]), format_g6(v[1]), format_g6(v[2]))
}

impl SetupDebug {
    /// `printString` 0x140317cb0.
    fn print_string(&mut self, gui: &mut Gui, id: Id, painter: &mut dyn Painter, s: &str) {
        let p = gui.local_to_world(id, [0.0, self.y]);
        let font = gui.controls[id].font.unwrap_or(gui.font);
        gui.fonts[font].blit_string(painter, p[0], p[1], s, 1.0, ALIGN_LEFT);
        self.y += 20.0;
    }

    /// `printStringList(prefix, value, 4.0)` 0x140317d90: floored to four decimals.
    fn print_value(&mut self, gui: &mut Gui, id: Id, painter: &mut dyn Painter, prefix: &str, value: f32) {
        self.print_string(gui, id, painter, &format!("{prefix}{}", format_g6(floor4(value))));
    }
}

impl App for SetupDebug {
    fn as_any(&mut self) -> &mut dyn Any {
        self
    }

    /// `SetupDebug::render` 0x140318010.
    fn render(&mut self, gui: &mut Gui, id: Id, dt: f32, painter: &mut dyn Painter) {
        gui.control_render(id, dt, painter);
        // (the game reads a focused car without a race engineer at address 8)
        let Some(car) = gui.apps.setup_debug.clone() else { return };
        let s = gui.apps.cars.get(gui.apps.focused_car).map(|c| c.state).unwrap_or_default();
        self.y = 15.0;
        for i in 0..4 {
            self.print_value(gui, id, painter, &format!("AIR PRESSURE {i} : "), car.pressure_static[i]);
        }
        for i in 0..4 {
            self.print_value(gui, id, painter, &format!("camberRAD {i} : "), s.camber_rad[i]);
        }
        for i in 0..4 {
            self.print_value(gui, id, painter, &format!("camberDEG {i} : "), s.camber_rad[i] * f32::from_bits(0x4265_2ee1));
        }
        self.print_string(gui, id, painter, &format!("accG {}", vec3(s.acc_g)));
        self.print_value(gui, id, painter, "engineRPM ", s.engine_rpm);
        self.print_value(gui, id, painter, "limiterRPM ", s.limiter_rpm as f32);
        self.print_value(gui, id, painter, "isEngineLimiterOn ", s.is_engine_limiter_on as u8 as f32);
        self.print_value(gui, id, painter, "steer ", s.steer);
        self.print_value(gui, id, painter, "gas ", s.gas);
        self.print_value(gui, id, painter, "brake ", s.brake);
        self.print_value(gui, id, painter, "clutch ", s.clutch);
        self.print_value(gui, id, painter, "gear ", s.gear as f32);
        self.print_value(gui, id, painter, "cgHeight ", s.cg_height);
        let m = &s.world_matrix.m;
        self.print_string(gui, id, painter, &format!("worldPosition {}", vec3([m[3][0], m[3][1], m[3][2]])));
        self.print_string(gui, id, painter, &format!("velocity {}", vec3(s.velocity)));
        self.print_string(gui, id, painter, &format!("Tyre Comp.: {}", car.tyre_compound));
        self.print_value(gui, id, painter, "lapTime ", s.lap_time as f32);
        self.print_value(gui, id, painter, "lastLap ", s.last_lap as f32);
        self.print_value(gui, id, painter, "bestLap ", s.best_lap as f32);
        self.print_value(gui, id, painter, "lapCount ", s.lap_count as f32);
        self.print_value(gui, id, painter, "performanceMeter ", s.performance_meter);
        self.print_value(gui, id, painter, "FUEL  ", s.fuel);
        self.y += 20.0;
        for a in &car.wing_angles {
            self.print_value(gui, id, painter, "AEROMAP : ", *a);
        }
        for i in 0..4 {
            self.print_value(gui, id, painter, &format!("ROD LENGTH {i} : "), car.rod_lengths[i]);
        }
        if car.max_gear > 0 {
            for k in 1..=car.max_gear as usize {
                let ratio = car.gear_ratios.get(k + 1).copied().unwrap_or(0.0);
                self.print_value(gui, id, painter, &format!("GEARS {k}  : "), ratio as f32);
            }
        }
        self.print_value(gui, id, painter, "FINAL RATIO : ", car.final_ratio);
        let rows: [(&str, [f32; 4]); 7] = [
            ("wheelAngularSpeed", s.wheel_angular_speed),
            ("slipAngle", s.slip_angle),
            ("slipRatio", s.slip_ratio),
            ("load", s.load),
            ("tyreDirtyLevel", s.tyre_dirty_level),
            ("suspensionTravel", s.suspension_travel),
            ("ndSlip", s.nd_slip),
        ];
        for (name, values) in rows {
            for (i, v) in values.iter().enumerate() {
                self.print_value(gui, id, painter, &format!("{name} {i} : "), *v);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{floor4, format_g6};

    #[test]
    fn floors_to_four_decimals() {
        assert_eq!(format_g6(floor4(-1.23456)), "-1.2346");
        assert_eq!(format_g6(floor4(1.23456)), "1.2345");
        assert_eq!(format_g6(floor4(2.0)), "2");
    }
}
