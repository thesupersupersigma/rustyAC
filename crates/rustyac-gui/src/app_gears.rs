// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! `FormGear` (0x3d8 bytes), the app "Gears": the gear and the speed in the middle of round
//! gauges for the revs, the turbo and the KERS / ERS, drawn with the game's `ksCircularRPM`
//! shader.
//!
//! `FormGear::FormGear` 0x1402e8280, `render` 0x1402e9ac0; its static `gears` (initialiser
//! 0x140012b60).

use std::any::Any;

use rustyac_physics::data::ini::IniReader;

use crate::app_essentials::cut_f;
use crate::apps::App;
use crate::control::Id;
use crate::font::{Font, FontType};
use crate::gui::Gui;
use crate::hud::translate;
use crate::painter::{Painter, Texture, ALIGN_CENTER, GRAPHICS_GL};

const GEARS: [&str; 12] = ["R", "N", "1", "2", "3", "4", "5", "6", "7", "8", "9", "10"];

pub struct Gears {
    pub lab_gear: Id,
    pub lab_small_speed: Id,
    pub lab_speed: Id,
    pub rpm_shader: Option<usize>,
    pub rpm_angle_var: Option<usize>,
    pub kers_var: Option<usize>,
    pub off_texture: Texture,
    pub on_texture: Texture,
    pub off_texture_turbo: Texture,
    pub on_texture_turbo: Texture,
    pub kers_left_off_texture: Texture,
    pub kers_right_off_texture: Texture,
    pub kers_on_texture: Texture,
    pub battery_on_texture: Texture,
    pub kers_load_on_texture: Texture,
    pub kers_inner_right_off_texture: Texture,
    /// per car of the session when the app was made
    pub kers_on: Vec<bool>,
    pub current_rpm: f32,
    pub max_turbos: Vec<f32>,
    pub has_p2p: bool,
    pub p2p_label: Option<Id>,
    pub p2p_value: Option<Id>,
}

/// The sum of a car's `[TURBO_n] DISPLAY_MAX_BOOST` (its `data/engine.ini`), as `FormGear`'s
/// constructor adds them up.
pub fn max_turbo(engine: Option<&IniReader>) -> f32 {
    let mut sum = 0.0f32;
    let Some(ini) = engine else { return sum };
    let mut n = 0;
    while ini.has_section(&format!("TURBO_{n}")) {
        sum = ini.get_float(&format!("TURBO_{n}"), "DISPLAY_MAX_BOOST").unwrap_or(0.0) + sum;
        n += 1;
    }
    sum
}

impl Gui {
    /// `FormGear::FormGear(CarAvatar*)` 0x1402e8280. `engines` is every car's `data/engine.ini`
    /// (by the car's place in the session).
    pub fn new_gears_app(&mut self, painter: &mut dyn Painter, engines: &[Option<IniReader>]) -> Id {
        let form = self.new_form(painter, "FORM_GEAR", true);
        self.form_set_title(form, &translate("Gears"));
        self.form_data_mut(form).dev_app = false;
        self.set_size(form, 196.0, 206.0);
        self.set_auto_hide_mode(form, true);
        let font = |gui: &mut Gui, painter: &mut dyn Painter, size: f32| {
            let mut f = Font::new(painter, "", FontType::Monospaced, size, false, false);
            f.shadowed = true;
            gui.fonts.push(f);
            gui.fonts.len() - 1
        };
        let f = font(self, painter, 120.0);
        self.controls[form].font = Some(f);
        let label = |gui: &mut Gui, painter: &mut dyn Painter, name: &str, y: f32, size: f32, fore_first: bool| {
            let id = gui.new_label(painter, name);
            gui.set_position(id, 0.0, y);
            gui.set_size(id, 156.0, 32.0);
            let f = font(gui, painter, size);
            gui.controls[id].font = Some(f);
            let _ = fore_first;
            gui.controls[id].fore_color = [1.0; 4];
            gui.controls[id].font_align = ALIGN_CENTER;
            id
        };
        let lab_gear = label(self, painter, "N", 60.0, 98.0, true);
        self.add_control(form, lab_gear);
        let lab_speed = label(self, painter, "145", 155.0, 36.0, false);
        self.add_control(form, lab_speed);
        let lab_small_speed = label(self, painter, "kmh", 190.0, 12.0, false);
        if self.apps.use_mph {
            self.set_text(lab_small_speed, "mph");
        }
        self.add_control(form, lab_small_speed);
        let rpm_shader = painter.get_shader("ksCircularRPM");
        let rpm_angle_var = rpm_shader.and_then(|s| painter.shader_var(s, "angleRPM"));
        let kers_var = rpm_shader.and_then(|s| painter.shader_var(s, "kersType"));
        let mut tex = |name: &str| painter.get_texture(&format!("content/texture/{name}.png"));
        let off_texture = tex("rpm_off");
        let on_texture = tex("rpm_on");
        let off_texture_turbo = tex("turbo_off");
        let on_texture_turbo = tex("turbo_on");
        let kers_left_off_texture = tex("kers_left_off");
        let kers_right_off_texture = tex("kers_right_off");
        let battery_on_texture = tex("kersmotor_on");
        let kers_on_texture = tex("battery_on");
        let kers_load_on_texture = tex("kersload_on");
        let kers_inner_right_off_texture = tex("kers_inner_right_off");
        let mut max_turbos = Vec::new();
        let mut kers_on = Vec::new();
        for (i, car) in self.apps.cars.clone().iter().enumerate() {
            max_turbos.push(max_turbo(engines.get(i).and_then(|e| e.as_ref())));
            kers_on.push(car.has_kers || car.has_ers);
        }
        let has_p2p = self.apps.has_p2p;
        let (mut p2p_label, mut p2p_value) = (None, None);
        if has_p2p {
            let id = self.new_label(painter, "P2PLBL");
            self.set_position(id, 146.0, 47.0);
            self.set_size(id, 30.0, 32.0);
            let f = font(self, painter, 15.0);
            self.controls[id].font = Some(f);
            self.controls[id].fore_color = [1.0; 4];
            self.controls[id].font_align = ALIGN_CENTER;
            self.set_text(id, "P2P");
            self.add_control(form, id);
            p2p_label = Some(id);
            let id = self.new_label(painter, "P2PVAL");
            self.set_position(id, 146.0, 59.0);
            self.set_size(id, 30.0, 32.0);
            let f = font(self, painter, 25.0);
            self.controls[id].font = Some(f);
            self.controls[id].fore_color = [1.0; 4];
            self.controls[id].font_align = ALIGN_CENTER;
            self.set_text(id, "15");
            self.add_control(form, id);
            p2p_value = Some(id);
        }
        let m = self.controls[form].scale_mult;
        self.scale_by_mult(form, m);
        self.set_app(
            form,
            Box::new(Gears {
                lab_gear,
                lab_small_speed,
                lab_speed,
                rpm_shader,
                rpm_angle_var,
                kers_var,
                off_texture,
                on_texture,
                off_texture_turbo,
                on_texture_turbo,
                kers_left_off_texture,
                kers_right_off_texture,
                kers_on_texture,
                battery_on_texture,
                kers_load_on_texture,
                kers_inner_right_off_texture,
                kers_on,
                current_rpm: 0.0,
                max_turbos,
                has_p2p,
                p2p_label,
                p2p_value,
            }),
        );
        form
    }
}

fn clamp01(x: f32) -> f32 {
    if x > 1.0 {
        1.0
    } else if x >= 0.0 {
        x
    } else {
        0.0
    }
}

impl Gears {
    /// `ShaderVariable::set` of the angle (and the gauge's kind), then `CBuffer::commit` of each.
    fn set(&self, painter: &mut dyn Painter, angle: f32, kers: Option<u32>) {
        let Some(shader) = self.rpm_shader else { return };
        if let Some(var) = self.rpm_angle_var {
            painter.set_shader_var(shader, var, &angle.to_le_bytes());
        }
        if let (Some(var), Some(bits)) = (self.kers_var, kers) {
            painter.set_shader_var(shader, var, &bits.to_le_bytes());
        }
        if let Some(var) = self.rpm_angle_var {
            painter.commit_shader_var(shader, var);
        }
        if let (Some(var), Some(_)) = (self.kers_var, kers) {
            painter.commit_shader_var(shader, var);
        }
    }

    fn draw(&self, painter: &mut dyn Painter, t0: &Texture, t1: &Texture, rect: [f32; 3]) {
        painter.set_texture(0, t0);
        painter.set_texture(1, t1);
        painter.gl_color3f(GRAPHICS_GL, 1.0, 1.0, 1.0);
        match self.rpm_shader {
            Some(shader) => painter.gl_quad_shader(GRAPHICS_GL, rect[0], rect[1], rect[2], rect[2], true, shader),
            None => painter.gl_quad(GRAPHICS_GL, rect[0], rect[1], rect[2], rect[2], true),
        }
    }
}

impl App for Gears {
    fn as_any(&mut self) -> &mut dyn Any {
        self
    }

    /// `FormGear::render` 0x1402e9ac0.
    fn render(&mut self, gui: &mut Gui, id: Id, dt: f32, painter: &mut dyn Painter) {
        let link = gui.apps.clone();
        let index = link.focused_car;
        let car = link.focused();
        let st = &car.state;
        let max_turbo = self.max_turbos.get(index).copied().unwrap_or(0.0);
        let kers_on = self.kers_on.get(index).copied().unwrap_or(false);
        let g = st.gear;
        gui.set_text(self.lab_gear, GEARS[if (g as u32) <= 10 { g as usize } else { 10 }]);
        let k = (dt * f32::from_bits(0x4279_ffff)) * f32::from_bits(0x3e4c_cccd);
        self.current_rpm = (st.engine_rpm - self.current_rpm) * k + self.current_rpm;
        let limit = if st.limiter_rpm < car.engine_damage_rpm { st.limiter_rpm } else { car.engine_damage_rpm };
        let limit_f = limit as f32;
        gui.controls[self.lab_gear].fore_color = if self.current_rpm > limit_f * f32::from_bits(0x3f7d_70a4) { [1.0, 0.0, 0.0, 1.0] } else { [1.0; 4] };
        let v = st.speed * if link.use_mph { f32::from_bits(0x400f_29f7) } else { f32::from_bits(0x4066_6666) };
        let v = v + 0.5;
        gui.set_text(self.lab_speed, &cut_f(v).to_string());
        gui.control_render(id, dt, painter);

        let s = gui.controls[id].scale_mult;
        let size = s * 156.0;
        let r = gui.controls[id].rect;
        let rect = [r.left, s * 40.0 + r.top, size];
        let a084 = f32::from_bits(0x3f57_0a3e);
        let a008 = f32::from_bits(0x3da3_d70a);
        let a057 = f32::from_bits(0x3f11_eb85);
        let a063 = f32::from_bits(0x3f21_47ae);
        // the revs
        let angle = clamp01(self.current_rpm / limit_f) * a084 + a008;
        self.set(painter, angle, Some(0));
        self.draw(painter, &self.off_texture, &self.on_texture, rect);
        // the turbo
        let mut angle = 0.0;
        if !(max_turbo == 0.0 || max_turbo.is_nan()) {
            angle = clamp01(st.turbo_boost / max_turbo) * a084 + a008;
        }
        self.set(painter, angle, None);
        self.draw(painter, &self.off_texture_turbo, &self.on_texture_turbo, rect);
        // what the KERS / ERS gives
        let mut angle = 0.0;
        if kers_on {
            angle = st.kers_input * a057 + f32::from_bits(0x3e2e_147b);
        }
        self.set(painter, angle, Some(0x3f80_0000));
        self.draw(painter, &self.kers_left_off_texture, &self.kers_on_texture, rect);
        // its charge
        let mut angle = f32::from_bits(0x3f6c_cccc);
        if kers_on {
            angle = (((1.0 - st.kers_charge) * a057) * 0.5) + a063;
        }
        self.set(painter, angle, Some(0x4000_0000));
        self.draw(painter, &self.battery_on_texture, &self.kers_right_off_texture, rect);
        // the energy used of the lap's (KERS, then ERS)
        for max in [car.kers_max_j, car.ers_max_j] {
            if !(0.0 >= max) {
                let x = clamp01((st.kers_current_kj / max) * 1000.0);
                let angle = ((x * a057) * 0.5) + a063;
                self.set(painter, angle, Some(0x4040_0000));
                self.draw(painter, &self.kers_load_on_texture, &self.kers_inner_right_off_texture, rect);
            }
        }
        if self.has_p2p {
            if let Some(value) = self.p2p_value {
                match st.p2p_status {
                    1 => gui.controls[value].fore_color = [1.0, 0.0, 0.0, 1.0],
                    2 => gui.controls[value].fore_color = [1.0; 4],
                    3 => gui.controls[value].fore_color = [0.0, 0.0, 1.0, 1.0],
                    _ => {}
                }
                gui.set_text(value, &(st.p2p_activations as u32).to_string());
            }
        }
        let _ = (self.lab_small_speed, self.p2p_label);
    }
}
