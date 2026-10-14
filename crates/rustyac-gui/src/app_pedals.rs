// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! `CarPedalsForm` (0x1f0 bytes), the app "Pedals": gas, brake, clutch and force feedback as
//! bars; and `ElectronicAidsForm` (0x1e0 bytes), the app "Electronics": ABS, TC, EDL and DRS
//! lights.
//!
//! `CarPedalsForm::CarPedalsForm` 0x1402dbc80, `render` 0x1402dc4d0;
//! `ElectronicAidsForm::ElectronicAidsForm` 0x1402ea500, `render` 0x1402eae30.

use std::any::Any;

use rustyac_physics::data::ini::IniReader;

use crate::apps::App;
use crate::control::Id;
use crate::gui::Gui;
use crate::hud::translate;
use crate::painter::Painter;

const GREY: [f32; 4] = [0.5, 0.5, 0.5, 1.0];
const RED: [f32; 4] = [1.0, 0.0, 0.0, 1.0];
const GREEN: [f32; 4] = [0.0, 1.0, 0.0, 1.0];
const BLUE: [f32; 4] = [0.0, 0.0, 1.0, 1.0];
const WHITE: [f32; 4] = [1.0; 4];
const G7: [f32; 4] = [f32::from_bits(0x3f33_3333), f32::from_bits(0x3f33_3333), f32::from_bits(0x3f33_3333), 1.0];
const ORANGE: [f32; 4] = [2.0, 1.0, 0.0, 1.0];
const YELLOW: [f32; 4] = [1.0, 1.0, 0.0, 1.0];

pub struct Pedals {
    pub prg_gas: Id,
    pub prg_brake: Id,
    pub prg_clutch: Id,
    pub prg_ff: Id,
    pub is_gas_ok: bool,
    /// `ffMult`: `controls.ini [STEER] FF_GAIN` with a wheel; never read again
    pub ff_mult: f32,
    pub gas_led: Id,
}

pub struct Electronics {
    pub control_abs: Id,
    pub control_tc: Id,
    pub control_drs: Id,
    pub control_edl: Id,
}

impl Gui {
    /// `CarPedalsForm::CarPedalsForm(CarAvatar*)` 0x1402dbc80; `controls` is `cfg/controls.ini`.
    pub fn new_pedals_app(&mut self, painter: &mut dyn Painter, controls: Option<&IniReader>) -> Id {
        let form = self.new_form(painter, "PEDALS", true);
        let mut ff_mult = 1.0;
        self.form_set_title(form, &translate("Pedals"));
        self.form_data_mut(form).dev_app = false;
        if let Some(ini) = controls.filter(|ini| ini.ready) {
            if ini.get_string("HEADER", "INPUT_METHOD") == "WHEEL" {
                ff_mult = ini.get_float("STEER", "FF_GAIN").unwrap_or(0.0);
            }
        }
        self.set_size(form, 130.0, 100.0);
        let back = self.new_control(painter, "BACK");
        self.set_size(back, 80.0, 60.0);
        self.set_position(back, 25.0, 32.0);
        self.controls[back].back_color = [f32::from_bits(0x3e4c_cccd), f32::from_bits(0x3e4c_cccd), f32::from_bits(0x3e4c_cccd), f32::from_bits(0x3f66_6666)];
        self.controls[back].border_color = WHITE;
        self.add_control(form, back);
        self.set_auto_hide_mode(form, true);
        let bar = |gui: &mut Gui, painter: &mut dyn Painter, name: &str, x: f32| {
            let id = gui.new_progress_bar(painter, name);
            gui.set_size(id, 10.0, 50.0);
            gui.set_position(id, x, 40.0);
            id
        };
        let prg_gas = bar(self, painter, "GAS", 65.0);
        self.controls[prg_gas].fore_color = GREEN;
        self.controls[prg_gas].border_color = WHITE;
        self.add_control(form, prg_gas);
        let gas_led = self.new_control(painter, "GAS");
        self.set_size(gas_led, 10.0, 5.0);
        self.set_position(gas_led, 65.0, 33.0);
        self.controls[gas_led].border_color = WHITE;
        self.controls[gas_led].back_color = RED;
        self.add_control(form, gas_led);
        let prg_brake = bar(self, painter, "BRAKE", 50.0);
        self.controls[prg_brake].fore_color = RED;
        self.add_control(form, prg_brake);
        let prg_clutch = bar(self, painter, "CLUTCH", 35.0);
        self.controls[prg_clutch].fore_color = BLUE;
        self.add_control(form, prg_clutch);
        let prg_ff = bar(self, painter, "FF", 80.0);
        {
            let p = self.progress_bar_mut(prg_ff);
            p.is_vertical = true;
            p.min_value = 0.0;
            p.max_value = 1.0;
        }
        self.add_control(form, prg_ff);
        let m = self.controls[form].scale_mult;
        self.scale_by_mult(form, m);
        self.set_app(form, Box::new(Pedals { prg_gas, prg_brake, prg_clutch, prg_ff, is_gas_ok: false, ff_mult, gas_led }));
        form
    }

    /// `ElectronicAidsForm::ElectronicAidsForm(CarAvatar*)` 0x1402ea500.
    pub fn new_electronics_app(&mut self, painter: &mut dyn Painter) -> Id {
        let form = self.new_form(painter, "ELECTRONICS", true);
        self.form_set_title(form, &translate("Electronics"));
        self.form_data_mut(form).dev_app = false;
        self.set_size(form, 350.0, 115.0);
        let mut x = 35.0;
        let mut make = |gui: &mut Gui, painter: &mut dyn Painter, name: &str, file: &str| {
            let id = gui.new_control(painter, name);
            gui.set_position(id, x, 30.0);
            let tex = painter.get_texture(&format!("content/gui/electronicaids/{file}.png"));
            let (w, h) = if tex.kid.is_some() { painter.texture_size(&tex) } else { (0, 0) };
            gui.controls[id].back_texture = tex;
            gui.set_size(id, w as f32, h as f32);
            gui.controls[id].draw_background = false;
            gui.controls[id].draw_border = false;
            gui.add_control(form, id);
            x = gui.controls[id].rect.right + 10.0;
            id
        };
        let control_abs = make(self, painter, "ABS", "abs");
        let control_tc = make(self, painter, "TC", "tc");
        let control_edl = make(self, painter, "EDL", "edl");
        let control_drs = make(self, painter, "DRS", "drs");
        self.set_auto_hide_mode(form, true);
        let m = self.controls[form].scale_mult;
        self.scale_by_mult(form, m);
        self.set_app(form, Box::new(Electronics { control_abs, control_tc, control_drs, control_edl }));
        form
    }
}

impl App for Pedals {
    fn as_any(&mut self) -> &mut dyn Any {
        self
    }

    /// `CarPedalsForm::render` 0x1402dc4d0.
    fn render(&mut self, gui: &mut Gui, id: Id, dt: f32, painter: &mut dyn Painter) {
        let car = gui.apps.focused();
        let st = &car.state;
        gui.progress_bar_mut(self.prg_gas).value = st.gas;
        gui.progress_bar_mut(self.prg_brake).value = st.brake;
        let v = 1.0 - st.clutch;
        gui.progress_bar_mut(self.prg_clutch).value = if v > 1.0 {
            1.0
        } else if v >= 0.0 {
            v
        } else {
            0.0
        };
        let ffscale = car.user_ff_gain.unwrap_or(1.0);
        let mut ff = 0.0f32;
        if let Some(gain) = car.ff_global_gain {
            let a = st.last_ff_pure.abs();
            ff = gain * (a * ffscale);
            gui.progress_bar_mut(self.prg_ff).value = if ff > 1.0 {
                1.0
            } else if ff >= 0.0 {
                ff
            } else {
                0.0
            };
        }
        if 1.0 > st.gas {
            gui.controls[self.prg_gas].border_color = GREY;
        } else {
            gui.controls[self.prg_gas].border_color = GREEN;
            self.is_gas_ok = true;
        }
        gui.controls[self.gas_led].back_color = if self.is_gas_ok { GREEN } else { RED };
        gui.controls[self.prg_brake].border_color = if 0.0 >= st.brake { GREY } else { RED };
        gui.controls[self.prg_clutch].border_color = if 0.0 >= st.clutch { GREY } else { BLUE };
        gui.controls[self.prg_ff].fore_color = if car.ff_global_gain.is_some() && ff > 1.0 { RED } else { GREY };
        gui.control_render(id, dt, painter);
        let _ = self.ff_mult;
    }
}

impl App for Electronics {
    fn as_any(&mut self) -> &mut dyn Any {
        self
    }

    /// `ElectronicAidsForm::render` 0x1402eae30: the car it was made with (the first).
    fn render(&mut self, gui: &mut Gui, id: Id, dt: f32, painter: &mut dyn Painter) {
        gui.control_render(id, dt, painter);
        let car = gui.apps.cars.first().cloned().unwrap_or_default();
        let st = &car.state;
        if car.abs_enabled {
            gui.controls[self.control_abs].back_texture_color = if car.abs_in_action { ORANGE } else { G7 };
        }
        gui.set_visible(self.control_abs, car.abs_enabled);
        if car.tc_enabled {
            gui.controls[self.control_tc].back_texture_color = if car.tc_in_action { ORANGE } else { G7 };
        }
        gui.set_visible(self.control_tc, car.tc_enabled);
        if car.edl_enabled {
            let out = car.edl_out_level;
            gui.controls[self.control_edl].back_texture_color = if (out as f64) > 0.1 && !(1.0 >= st.speed) {
                let g = (((out as f64) * 0.3) as f32) + f32::from_bits(0x3f33_3333);
                let r = (((out as f64) * 1.3) as f32) + f32::from_bits(0x3f33_3333);
                [r, g, 0.0, 1.0]
            } else {
                G7
            };
        }
        gui.set_visible(self.control_edl, car.edl_enabled);
        if !car.has_drs {
            gui.set_visible(self.control_drs, false);
            return;
        }
        let sb = st.status_bytes;
        gui.controls[self.control_drs].back_texture_color = if sb & 4 != 0 {
            YELLOW
        } else if sb & 2 != 0 {
            GREEN
        } else {
            G7
        };
        gui.set_visible(self.control_drs, true);
    }
}
