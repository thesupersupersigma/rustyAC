// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! `SetupInformation` (0x1d8 bytes), the panel "Car Status" beside the setup editor's pages:
//! camber, caster, pressures, temperatures, toe and grain / blister of every wheel and the
//! two ride heights with the car's minimum, read from the car's state every frame.
//!
//! Constructor 0x14017fcf0, `setFrontValues` 0x1401820e0, `setRearValues` 0x140182860,
//! `render` 0x140180660, `cutDecimals` 0x140180510, `getTOE` 0x140180580, `tempToString`
//! 0x140182eb0; `Node::worldToLocalNormal` 0x14020e8d0.

use rustyac_physics::vecmath::xm_matrix_inverse;

use crate::control::{Id, Kind};
use crate::font::{Font, FontType};
use crate::gui::Gui;
use crate::hud::translate;
use crate::menus::Menu;
use crate::painter::{Painter, Texture, ALIGN_CENTER, ALIGN_LEFT};
use crate::setup_car::SetupCarState;
use crate::setup_tab::cut;
use crate::widgets::msvc_floor;

/// `CarInfoFront` / `CarInfoRear`: the labels of one wheel (a rear wheel has no caster).
#[derive(Clone, Copy, Debug, Default)]
pub struct CarInfo {
    pub temperature: Id,
    pub camber: Id,
    pub caster: Option<Id>,
    pub pressure: Id,
    pub toe: Id,
    pub grain_blister: Id,
}

/// `SetupInformation`'s own members.
pub struct SetupInformation {
    /// loaded and never drawn
    pub car_view: Texture,
    /// LF, RF, LR, RR
    pub wheels: [CarInfo; 4],
    pub front_height: Id,
    pub rear_height: Id,
}

/// `std::to_wstring(float)`.
fn to_wstring(v: f32) -> String {
    format!("{:.6}", v as f64)
}

/// `SetupInformation::cutDecimals` 0x140180510: cut after `n` decimals (0: the point goes too).
pub fn cut_decimals(text: &str, n: i32) -> String {
    let units: Vec<u16> = text.encode_utf16().collect();
    let mut p = units.iter().rposition(|u| *u == '.' as u16).map_or(-1, |i| i as i32);
    if n != 0 {
        p = p + 1 + n;
    }
    if p < 0 {
        return text.to_string();
    }
    String::from_utf16_lossy(&units[..(p as usize).min(units.len())])
}

/// `tempToString` 0x140182eb0.
fn temp_to_string(imo: &[f32; 3]) -> String {
    // (`comiss`: a number that is none is "NA" too)
    if !(imo[0] > -50.0) {
        "NA".to_string()
    } else {
        format!("{}/{}/{}", cut(imo[0]), cut(imo[1]), cut(imo[2]))
    }
}

/// `SetupInformation::getTOE` 0x140180580: a wheel's toe in degrees, from where its
/// suspension points in the car's body.
pub fn toe_deg(state: &SetupCarState, wheel: usize) -> f32 {
    let m = &state.suspension_matrix[wheel].m;
    let v = [-m[2][0], -m[2][1], -m[2][2]];
    // Node::worldToLocalNormal
    let i = xm_matrix_inverse(&state.body_matrix).m;
    let x = (v[1] * i[1][0] + v[0] * i[0][0]) + v[2] * i[2][0];
    let z = (v[1] * i[1][2] + v[0] * i[0][2]) + v[2] * i[2][2];
    let l = (z * z + x * x).sqrt();
    let x = if l != 0.0 { x / l } else { x };
    rustyac_physics::math::asinf(x) * f32::from_bits(0x4265_2ee1)
}

impl Gui {
    fn setup_information(&self, id: Id) -> &SetupInformation {
        match &self.controls[id].kind {
            Kind::Menu(menu) => match &**menu {
                Menu::SetupInfo(info) => info,
                _ => panic!("control {id} is not the car status panel"),
            },
            _ => panic!("control {id} is not the car status panel"),
        }
    }

    fn setup_information_label(&mut self, painter: &mut dyn Painter, name: &str, key: &str, x: f32, y: f32) -> Id {
        let label = self.new_label(painter, name);
        self.set_text(label, &format!("{}:", translate(key)));
        self.controls[label].font_align = ALIGN_LEFT;
        self.set_position(label, x, y);
        label
    }

    /// `SetupInformation::SetupInformation` 0x14017fcf0.
    pub fn new_setup_information(&mut self, painter: &mut dyn Painter) -> Id {
        let id = self.new_control(painter, "SETUP_INFORMATION");
        self.control_set_size(id, 470.0, 600.0);
        self.fonts.push(Font::new(painter, "", FontType::Proportional, 14.0, false, false));
        self.controls[id].font = Some(self.fonts.len() - 1);
        self.controls[id].draw_border = true;
        self.controls[id].draw_background = true;
        self.controls[id].back_color = [0.1, 0.1, 0.1, 0.6];
        self.controls[id].border_color = [0.2, 0.2, 0.2, 0.8];
        let car_view = painter.get_texture("content/gui/setupApps/carView.png");
        let mut wheels = [CarInfo::default(); 4];
        for (w, (x, y)) in [(20.0, 50.0), (250.0, 50.0), (20.0, 480.0), (250.0, 480.0)].into_iter().enumerate() {
            let info = if w < 2 {
                // setFrontValues 0x1401820e0
                let camber = self.setup_information_label(painter, "FRONT_CAMBER", "Camber", x, y);
                let mut yy = y + 20.0;
                let caster = self.setup_information_label(painter, "FRONT_CASTER", "Caster", x, yy);
                yy += 20.0;
                let pressure = self.setup_information_label(painter, "FRONT_PRESSURE", "Pressure", x, yy);
                yy += 20.0;
                let temperature = self.setup_information_label(painter, "FRONT_TEMPERATURE", "Temp", x, yy);
                yy += 20.0;
                let toe = self.setup_information_label(painter, "FRONT_TOE", "TOE", x, yy);
                yy += 20.0;
                let grain_blister = self.setup_information_label(painter, "GRAIN_BLISTER", "Grain/Blister", x, yy);
                for label in [camber, caster, pressure, temperature, toe, grain_blister] {
                    self.add_control(id, label);
                }
                CarInfo { temperature, camber, caster: Some(caster), pressure, toe, grain_blister }
            } else {
                // setRearValues 0x140182860
                let camber = self.setup_information_label(painter, "REAR_CAMBER", "Camber", x, y);
                let mut yy = y + 20.0;
                let pressure = self.setup_information_label(painter, "REAR_PRESSURE", "Pressure", x, yy);
                yy += 20.0;
                let temperature = self.setup_information_label(painter, "REAR_TEMPERATURE", "Temp", x, yy);
                yy += 20.0;
                let toe = self.setup_information_label(painter, "REAR_TOE", "TOE", x, yy);
                yy += 20.0;
                let grain_blister = self.setup_information_label(painter, "GRAIN_BLISTER", "Grain/Blister", x, yy);
                for label in [camber, pressure, temperature, toe, grain_blister] {
                    self.add_control(id, label);
                }
                CarInfo { temperature, camber, caster: None, pressure, toe, grain_blister }
            };
            wheels[w] = info;
        }
        let front_height = self.setup_information_label(painter, "FRONT_HEIGHT", "Front Height", 90.0, 180.0);
        self.add_control(id, front_height);
        let rear_height = self.setup_information_label(painter, "REAR_HEIGHT", "Rear Height", 90.0, 420.0);
        self.add_control(id, rear_height);
        self.controls[id].kind = Kind::Menu(Box::new(Menu::SetupInfo(Box::new(SetupInformation { car_view, wheels, front_height, rear_height }))));
        id
    }

    /// `SetupInformation::render` 0x140180660. The title is written before the frame's first
    /// colour: it has the colour the frame before left (the rear height's).
    pub(crate) fn setup_information_render(&mut self, id: Id, dt: f32, painter: &mut dyn Painter) {
        self.control_render(id, dt, painter);
        let mut r = self.controls[id].rect;
        self.get_world_rect(id, &mut r);
        let font = self.controls[id].font.unwrap_or(self.font);
        let top = self.controls[id].rect.top;
        self.fonts[font].blit_string(painter, (r.right - r.left) * 0.5 + r.left, top + 12.0, &translate("Car Status"), f32::from_bits(0x3fb3_3333), ALIGN_CENTER);
        self.fonts[font].set_color(1.0, 1.0, 1.0, 1.0);
        let state = self.setup_car.as_ref().map(|car| car.state()).unwrap_or_default();
        let info = self.setup_information(id);
        let (wheels, front_height, rear_height) = (info.wheels, info.front_height, info.rear_height);
        let hundredth = f32::from_bits(0x3c23_d70a);
        let mut value = |gui: &mut Gui, label: Id, text: &str| {
            let l = gui.controls[label].rect;
            gui.fonts[font].blit_string(painter, (r.left + l.right) + 100.0, (r.top + l.top) + 2.0, text, 1.0, ALIGN_LEFT);
        };
        for (w, wheel) in wheels.iter().enumerate() {
            let v = msvc_floor((state.camber_rad[w] * f32::from_bits(0x4265_2ee1)) * 100.0) * hundredth;
            value(self, wheel.camber, &cut_decimals(&to_wstring(v), 2));
        }
        for (w, wheel) in wheels.iter().enumerate() {
            let (blister, grain) = (cut(state.tyres[w].last_blister), cut(state.tyres[w].last_grain));
            value(self, wheel.grain_blister, &format!("{grain}% / {blister}%"));
        }
        for wheel in &wheels[..2] {
            let v = msvc_floor(state.caster * 100.0) * hundredth;
            if let Some(caster) = wheel.caster {
                value(self, caster, &cut_decimals(&to_wstring(v), 2));
            }
        }
        for (w, wheel) in wheels.iter().enumerate() {
            let (hot, cold) = (cut(state.tyres[w].dynamic_pressure), cut(state.tyres[w].static_pressure));
            value(self, wheel.pressure, &format!("Cold:{cold} Hot:{hot}"));
        }
        for (w, wheel) in wheels.iter().enumerate() {
            value(self, wheel.temperature, &temp_to_string(&state.tyres[w].last_set_imo));
        }
        for (w, wheel) in wheels.iter().enumerate() {
            let m = if w % 2 == 0 { hundredth } else { -hundredth };
            let v = msvc_floor(toe_deg(&state, w) * 100.0) * m;
            value(self, wheel.toe, &cut_decimals(&to_wstring(v), 2));
        }
        // the heights: white, red under the car's minimum, yellow on it
        let (mn, h0, h1) = (state.min_height_m, state.ride_height[0], state.ride_height[1]);
        let c = if h0 >= mn { 1.0 } else { 0.0 };
        self.fonts[font].set_color(1.0, c, c, 1.0);
        let (mm0, mm1, mm_min) = (msvc_floor(h0 * 1000.0), msvc_floor(h1 * 1000.0), msvc_floor(mn * 1000.0));
        let min_text = if 0.0 <= mm_min { to_wstring(mm_min) } else { "--".to_string() };
        if mm0 == mm_min {
            self.fonts[font].set_color(1.0, 1.0, 0.0, 1.0);
        }
        let text = format!("~{} mm - min: {}", cut_decimals(&to_wstring(mm0), 0), cut_decimals(&min_text, 0));
        let l = self.controls[front_height].rect;
        self.fonts[font].blit_string(painter, (r.left + l.right) + 170.0, (r.top + l.top) + 2.0, &text, 1.0, ALIGN_CENTER);
        let c = if mm1 >= mm_min { 1.0 } else { 0.0 };
        self.fonts[font].set_color(1.0, c, c, 1.0);
        if mm1 == mm_min {
            self.fonts[font].set_color(1.0, 1.0, 0.0, 1.0);
        }
        let text = format!("~{} mm - min: {}", cut_decimals(&to_wstring(mm1), 0), cut_decimals(&min_text, 0));
        let l = self.controls[rear_height].rect;
        self.fonts[font].blit_string(painter, (r.left + l.right) + 170.0, (r.top + l.top) + 2.0, &text, 1.0, ALIGN_CENTER);
    }
}
