// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! `TyresApp` (0x4f0 bytes), the app "Tyres": every tyre's inner / middle / outer and core
//! temperature, its pressure, its wear and the compound, coloured against the compound's
//! ideal values. It shows the first car (`Sim::getCar(0)`), whichever car is focused.
//!
//! `TyresApp::TyresApp` 0x140338860, `render` 0x14033a6d0; `system/cfg/tyres_app.ini`.

use std::any::Any;

use rustyac_physics::car_physics_state::CarPhysicsState;
use rustyac_physics::data::ini::IniReader;

use crate::apps::App;
use crate::control::Id;
use crate::font::{Font, FontType};
use crate::gui::Gui;
use crate::hud::translate;
use crate::painter::{Painter, ALIGN_CENTER, GRAPHICS_GL};

const FWIDTH: f32 = 237.0;
const FHEIGHT: f32 = 257.0;
const IMO_WIDTH: f32 = 111.0;
const IMO_HEIGHT: f32 = 15.0;
const TOP_MARGIN: f32 = 32.0;
const LEFT_MARGIN: f32 = 0.0;
const IMO_SEP: f32 = 15.0;
const FRONT_REAR_SEP: f32 = 180.0;
const WEAR_WIDTH: f32 = 5.0;
const WEAR_HEIGHT: f32 = 35.0;
const TYRE_V_SPACE: f32 = 5.0;
const NEAR_LIMIT: f32 = f32::from_bits(0x3f7d_70a4);
const FAR_LIMIT: f32 = f32::from_bits(0x3f7a_e148);
const CORE_TEXT: &str = "Core  ";

pub struct Tyres {
    pub lbl_imo: [[Id; 3]; 4],
    pub lbl_temp_imo: [[Id; 3]; 4],
    pub prg_wear: [Id; 4],
    pub ctrl_tyres: [Id; 4],
    pub lbl_pressure: [Id; 4],
    pub lbl_core: [Id; 4],
    pub lbl_compound: Id,
    pub back_color: [f32; 4],
    pub far_cold_color: [f32; 3],
    pub far_hot_color: [f32; 3],
    pub near_cold_color: [f32; 3],
    pub near_hot_color: [f32; 3],
    pub optimal_color: [f32; 3],
    pub good_color: [f32; 3],
    pub used_color: [f32; 3],
    pub bad_color: [f32; 3],
    /// `backgroundRect`: left, right, top, bottom
    pub background_rect: [f32; 4],
    pub current_compound: String,
    pub near_temp_limit: f32,
    pub far_temp_limit: f32,
    pub high_near_pressure_limit: [f32; 4],
    pub high_far_pressure_limit: [f32; 4],
    pub low_near_pressure_limit: [f32; 4],
    pub low_far_pressure_limit: [f32; 4],
    pub high_near_pressure_saturation: [f32; 4],
    pub high_far_pressure_saturation: [f32; 4],
    pub low_near_pressure_saturation: [f32; 4],
    pub low_far_pressure_saturation: [f32; 4],
    pub use_temp_in_f: bool,
}

/// The tail of a name from the `n`-th character on (the game adds an index to the literal's
/// pointer).
fn tail(name: &str, n: usize) -> String {
    name.chars().skip(n).collect()
}

impl Gui {
    /// `TyresApp::TyresApp(CarAvatar*)` 0x140338860; `ini` is `system/cfg/tyres_app.ini`.
    pub fn new_tyres_app(&mut self, painter: &mut dyn Painter, ini: Option<&IniReader>) -> Id {
        let form = self.new_form(painter, "TYRES_APP", true);
        let imo_color = [1.0f32; 4];
        let imo_dark_color = [0.0, f32::from_bits(0x3d4c_cccd), f32::from_bits(0x3da3_d70a), f32::from_bits(0x3ea8_f5c3)];
        let imo_font_color = [0.0, 0.0, 0.0, 1.0];
        let background_rect = [LEFT_MARGIN, LEFT_MARGIN + FWIDTH, TOP_MARGIN, TOP_MARGIN + (FHEIGHT - TOP_MARGIN)];
        self.set_size(form, FWIDTH, FHEIGHT);
        self.form_data_mut(form).dev_app = false;
        self.form_data_mut(form).autohide = true;
        self.fonts.push(Font::new(painter, "", FontType::Proportional, 10.0, false, false));
        let font = self.fonts.len() - 1;
        self.form_set_title(form, &translate("Tyres"));
        let pos0 = [LEFT_MARGIN, TOP_MARGIN];
        let pos1 = [(IMO_WIDTH + LEFT_MARGIN) + IMO_SEP, TOP_MARGIN];
        let pos = [pos0, pos1, pos0, pos1];
        let third = f32::from_bits(0x3eaa_aaab);
        let (mut lbl_imo, mut lbl_temp_imo) = ([[0; 3]; 4], [[0; 3]; 4]);
        let (mut prg_wear, mut ctrl_tyres, mut lbl_pressure, mut lbl_core) = ([0; 4], [0; 4], [0; 4], [0; 4]);
        let mut fy = 0.0f32;
        for i in 0..4 {
            if i >= 2 {
                fy = FRONT_REAR_SEP;
            }
            let core = self.new_label(painter, &tail("CORE", i));
            self.controls[core].font = Some(font);
            self.set_text(core, &format!("{CORE_TEXT}--- \u{b0}C"));
            self.controls[core].font_align = ALIGN_CENTER;
            self.controls[core].font_scale = f32::from_bits(0x3f99_999a);
            self.controls[core].back_color = imo_color;
            self.controls[core].fore_color = imo_font_color;
            self.set_size(core, IMO_WIDTH, IMO_HEIGHT);
            self.set_position(core, pos[i][0], fy + pos[i][1]);
            self.add_control(form, core);
            lbl_core[i] = core;
            for k in 0..3 {
                let l = self.new_label(painter, &tail("IMO_LBL", i + k));
                self.controls[l].font = Some(font);
                self.controls[l].font_scale = f32::from_bits(0x3f99_999a);
                self.controls[l].font_align = ALIGN_CENTER;
                self.controls[l].back_color = imo_dark_color;
                self.controls[l].fore_color = imo_color;
                self.set_size(l, IMO_WIDTH * third, IMO_HEIGHT);
                let t = self.new_label(painter, &tail("IMO_TEMP_LBL", i + k));
                self.controls[t].font = Some(font);
                self.controls[t].font_scale = f32::from_bits(0x3f99_999a);
                self.controls[t].font_align = ALIGN_CENTER;
                self.set_text(t, "---");
                self.controls[t].back_color = imo_color;
                self.controls[t].fore_color = imo_font_color;
                self.set_size(t, IMO_WIDTH * third, IMO_HEIGHT);
                let ly = (fy + pos[i][1]) + IMO_HEIGHT;
                let ty = (fy + pos[i][1]) + (IMO_HEIGHT * 2.0);
                let even = i % 2 == 0;
                let x = match k {
                    0 => {
                        self.set_text(l, "I");
                        if even {
                            ((IMO_WIDTH * third) * 2.0) + pos[i][0]
                        } else {
                            pos[i][0]
                        }
                    }
                    1 => {
                        self.set_text(l, "M");
                        (IMO_WIDTH * third) + pos[i][0]
                    }
                    _ => {
                        self.set_text(l, "O");
                        if even {
                            pos[i][0]
                        } else {
                            ((IMO_WIDTH * third) * 2.0) + pos[i][0]
                        }
                    }
                };
                self.set_position(l, x, ly);
                self.set_position(t, x, ty);
                self.add_control(form, l);
                self.add_control(form, t);
                lbl_imo[i][k] = l;
                lbl_temp_imo[i][k] = t;
            }
            if i >= 2 {
                fy = FRONT_REAR_SEP - (IMO_HEIGHT * 4.0);
            }
            let p = self.new_label(painter, &tail("PSI", i));
            self.controls[p].font = Some(font);
            self.set_text(p, "---psi");
            self.controls[p].font_align = ALIGN_CENTER;
            self.controls[p].back_color = imo_dark_color;
            self.controls[p].fore_color = imo_color;
            self.set_size(p, IMO_WIDTH * 0.5, IMO_HEIGHT);
            let tyre = self.new_control(painter, &tail("TYRE", i));
            let tex = painter.get_texture("content/gui/tyresapp/tyre_white_full.png");
            let (tw, th) = if tex.kid.is_some() { painter.texture_size(&tex) } else { (0, 0) };
            self.controls[tyre].back_texture = tex;
            let (tw, th) = (tw as f32, th as f32);
            self.set_size(tyre, tw, th);
            self.controls[tyre].draw_background = false;
            self.controls[tyre].draw_border = false;
            let ox = ((IMO_WIDTH * 0.5) - tw) * 0.5;
            let (mut a, mut b) = (-TYRE_V_SPACE, -(TYRE_V_SPACE + IMO_HEIGHT));
            if i >= 2 {
                a = (th - IMO_HEIGHT) + TYRE_V_SPACE;
                b = TYRE_V_SPACE + WEAR_HEIGHT;
            }
            let wear = self.new_progress_bar(painter, "WEAR");
            self.progress_bar_mut(wear).is_vertical = true;
            self.set_size(wear, WEAR_WIDTH, WEAR_HEIGHT);
            self.controls[wear].fore_color = [1.0; 4];
            let base = (fy + pos[i][1]) + (IMO_HEIGHT * 3.0);
            if i % 2 == 0 {
                self.set_position(p, pos[i][0], base);
                self.set_position(tyre, (IMO_WIDTH * 0.5) + (ox + pos[i][0]), base - a);
                self.set_position(wear, ((IMO_WIDTH * 0.5) + pos[i][0]) - WEAR_WIDTH, base - b);
            } else {
                self.set_position(p, (IMO_WIDTH * 0.5) + pos[i][0], base);
                self.set_position(tyre, ox + pos[i][0], base - a);
                self.set_position(wear, (IMO_WIDTH * 0.5) + pos[i][0], base - b);
            }
            self.add_control(form, p);
            self.add_control(form, tyre);
            self.add_control(form, wear);
            lbl_pressure[i] = p;
            ctrl_tyres[i] = tyre;
            prg_wear[i] = wear;
        }
        let near_temp_limit = 1.0 - NEAR_LIMIT;
        let far_temp_limit = NEAR_LIMIT - FAR_LIMIT;
        let lbl_compound = self.new_label(painter, "COMP");
        self.controls[lbl_compound].font = Some(font);
        self.set_text(lbl_compound, "");
        self.controls[lbl_compound].font_align = ALIGN_CENTER;
        self.controls[lbl_compound].back_color = imo_dark_color;
        self.controls[lbl_compound].fore_color = imo_color;
        self.set_size(lbl_compound, IMO_WIDTH * 0.5, IMO_HEIGHT);
        self.set_position(lbl_compound, ((IMO_WIDTH + LEFT_MARGIN) + IMO_SEP) + (IMO_WIDTH * 0.5), ((IMO_HEIGHT * 3.0) + 1.0) + (FRONT_REAR_SEP * 0.5));
        self.add_control(form, lbl_compound);
        let mut app = Tyres {
            lbl_imo,
            lbl_temp_imo,
            prg_wear,
            ctrl_tyres,
            lbl_pressure,
            lbl_core,
            lbl_compound,
            back_color: [0.0, 0.0, 0.0, f32::from_bits(0x3ecc_cccd)],
            far_cold_color: [0.0, 0.0, 1.0],
            far_hot_color: [1.0, 0.0, 0.0],
            near_cold_color: [0.0, 0.5, 1.0],
            near_hot_color: [1.0, 0.5, 0.0],
            optimal_color: [0.0, 1.0, 0.0],
            good_color: [0.0, 1.0, 0.0],
            used_color: [1.0, 1.0, 0.0],
            bad_color: [1.0, 0.0, 0.0],
            background_rect,
            current_compound: String::new(),
            near_temp_limit,
            far_temp_limit,
            high_near_pressure_limit: [1.0; 4],
            high_far_pressure_limit: [1.0; 4],
            low_near_pressure_limit: [1.0; 4],
            low_far_pressure_limit: [1.0; 4],
            high_near_pressure_saturation: [1.0; 4],
            high_far_pressure_saturation: [1.0; 4],
            low_near_pressure_saturation: [1.0; 4],
            low_far_pressure_saturation: [1.0; 4],
            use_temp_in_f: false,
        };
        if let Some(r) = ini.filter(|r| r.ready) {
            let f3 = |key: &str| r.get_float3("OPTIONS", key).unwrap_or([0.0; 3]);
            app.use_temp_in_f = r.get_int("OPTIONS", "USE_FAHRENHEIT").unwrap_or(0) != 0;
            app.far_cold_color = f3("FAR_COLD");
            app.far_hot_color = f3("FAR_HOT");
            app.near_cold_color = f3("NEAR_COLD");
            app.near_hot_color = f3("NEAR_HOT");
            app.optimal_color = f3("OPTIMAL");
            app.good_color = f3("WEAR1");
            app.used_color = f3("WEAR2");
            app.bad_color = f3("WEAR3");
        }
        let m = self.controls[form].scale_mult;
        self.scale_by_mult(form, m);
        self.set_app(form, Box::new(app));
        form
    }
}

fn clamp01(t: f32) -> f32 {
    if t > 1.0 {
        1.0
    } else if t >= 0.0 {
        t
    } else {
        0.0
    }
}

fn lerp(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 4] {
    [((b[0] - a[0]) * t) + a[0], ((b[1] - a[1]) * t) + a[1], ((b[2] - a[2]) * t) + a[2], 1.0]
}

/// `TyreThermalState::temps` as the game lays it out: element `e` of stripe `s` is float
/// `s + 3 * e` of the block (the port's `[[f32; 12]; 3]` holds the block's floats in order).
fn temp(state: &CarPhysicsState, tyre: usize, flat: usize) -> f32 {
    state.tyre_thermal_states[tyre].temps[flat / 12][flat % 12]
}

/// `std::to_wstring((int)x)` of a float (`cvttss2si`).
fn int_text(x: f32) -> String {
    crate::app_essentials::cut_f(x).to_string()
}

impl App for Tyres {
    fn as_any(&mut self) -> &mut dyn Any {
        self
    }

    /// `TyresApp::render` 0x14033a6d0.
    fn render(&mut self, gui: &mut Gui, id: Id, dt: f32, painter: &mut dyn Painter) {
        let link = gui.apps.clone();
        let car = link.cars.first().cloned().unwrap_or_default();
        let st = &car.state;
        let gl = GRAPHICS_GL;
        let [wx, wy] = gui.local_to_world(id, [0.0, 0.0]);
        let s = gui.controls[id].scale_mult;
        let c = self.back_color;
        painter.gl_color4f(gl, c[0], c[1], c[2], c[3]);
        let r = self.background_rect;
        painter.gl_quad(gl, (s * r[0]) + wx, (s * r[2]) + wy, (r[1] - r[0]) * s, (r[3] - r[2]) * s, false);
        painter.gl_end(gl);
        if !link.in_replay {
            gui.set_text(self.lbl_compound, &car.compound_short_name);
            let changed = self.current_compound != gui.controls[self.lbl_compound].text;
            self.current_compound = gui.controls[self.lbl_compound].text.clone();
            for i in 0..4 {
                let v = 1.0 - st.wear[i];
                gui.progress_bar_mut(self.prg_wear[i]).value = clamp01(v);
                let wm = st.wear_mult[i];
                let c = if wm > f32::from_bits(0x3f7a_e148) {
                    self.good_color
                } else if wm > f32::from_bits(0x3f75_c28f) {
                    self.used_color
                } else {
                    self.bad_color
                };
                gui.controls[self.prg_wear[i]].fore_color = [c[0], c[1], c[2], 1.0];
                let ts = &st.tyre_thermal_states[i];
                if crate::app_essentials::cut_f(st.tyre_inflation[i]) == 0 {
                    gui.set_text(self.lbl_pressure[i], "0psi");
                    gui.set_text(self.lbl_core[i], "");
                    for k in 0..3 {
                        gui.set_text(self.lbl_temp_imo[i][k], "");
                    }
                } else {
                    for k in 0..3 {
                        let mut a = 0.0f32;
                        for e in 0..12 {
                            a = a + temp(st, i, k + 3 * e);
                        }
                        a = a * f32::from_bits(0x3daa_aaab);
                        let text = if !self.use_temp_in_f { int_text(a) } else { int_text(((a * 9.0) * f32::from_bits(0x3e4c_cccd)) + 32.0) };
                        gui.set_text(self.lbl_temp_imo[i][k], &text);
                    }
                    let core = ts.core_temp;
                    if !self.use_temp_in_f {
                        gui.set_text(self.lbl_core[i], &format!("{CORE_TEXT}{}\u{b0}C", int_text(core)));
                    } else {
                        gui.set_text(self.lbl_core[i], &format!("{CORE_TEXT}{}\u{b0}F", int_text(((core * 9.0) * f32::from_bits(0x3e4c_cccd)) + 32.0)));
                    }
                    gui.set_text(self.lbl_pressure[i], &format!("{}psi", int_text(ts.dynamic_pressure)));
                }
                let m = ts.mult;
                let col = if !ts.is_hot {
                    if m > NEAR_LIMIT {
                        lerp(self.near_cold_color, self.optimal_color, clamp01((m - NEAR_LIMIT) / self.near_temp_limit))
                    } else {
                        lerp(self.far_cold_color, self.near_cold_color, clamp01((m - FAR_LIMIT) / self.far_temp_limit))
                    }
                } else if m > NEAR_LIMIT {
                    lerp(self.near_hot_color, self.optimal_color, clamp01((m - NEAR_LIMIT) / self.near_temp_limit))
                } else {
                    lerp(self.far_hot_color, self.near_hot_color, clamp01((m - FAR_LIMIT) / self.far_temp_limit))
                };
                gui.controls[self.ctrl_tyres[i]].back_texture_color = col;
                let ideal = car.ideal_pressure[i] as f32;
                if changed {
                    self.high_near_pressure_limit[i] = ideal * f32::from_bits(0x3f8c_cccd);
                    self.high_far_pressure_limit[i] = ideal * 1.5;
                    self.high_near_pressure_saturation[i] = self.high_near_pressure_limit[i] - ideal;
                    self.high_far_pressure_saturation[i] = self.high_far_pressure_limit[i] - self.high_near_pressure_limit[i];
                    self.low_near_pressure_limit[i] = ideal * f32::from_bits(0x3f66_6666);
                    self.low_far_pressure_limit[i] = ideal * 0.5;
                    self.low_near_pressure_saturation[i] = ideal - self.low_near_pressure_limit[i];
                    self.low_far_pressure_saturation[i] = self.low_near_pressure_limit[i] - self.low_far_pressure_limit[i];
                }
                let dp = ts.dynamic_pressure;
                let col = if dp > ideal {
                    if dp <= self.high_near_pressure_limit[i] {
                        lerp(self.near_hot_color, self.optimal_color, clamp01((self.high_near_pressure_limit[i] - dp) / self.high_near_pressure_saturation[i]))
                    } else {
                        lerp(self.far_hot_color, self.near_hot_color, clamp01((self.high_far_pressure_limit[i] - dp) / self.high_far_pressure_saturation[i]))
                    }
                } else if dp >= self.low_near_pressure_limit[i] {
                    lerp(self.near_cold_color, self.optimal_color, clamp01((dp - self.low_near_pressure_limit[i]) / self.low_near_pressure_saturation[i]))
                } else {
                    lerp(self.far_cold_color, self.near_cold_color, clamp01((dp - self.low_far_pressure_limit[i]) / self.low_far_pressure_saturation[i]))
                };
                gui.controls[self.lbl_pressure[i]].fore_color = col;
            }
        }
        gui.control_render(id, dt, painter);
        let _ = (self.lbl_imo, &self.current_compound);
    }
}
