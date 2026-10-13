// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! The HUD objects that follow the cars: `DriverNameDisplayer` (0x138 bytes) with its
//! `CarLabel`s (0x230, a control each), `ProximityIndicator` (0x60) and `DamageDisplayer`
//! (0xb0).
//!
//! `DriverNameDisplayer::DriverNameDisplayer` 0x14008c7e0, `projectPoint` 0x14008d550,
//! `renderHUD` 0x14008d900, `setActive` 0x14008e070, `update` 0x14008e2c0;
//! `CarLabel::CarLabel` 0x140069840, `resetRects` 0x14006a660, `updatePilotData`
//! 0x14006a8a0; `Sim::getNationFlag` 0x140199350; `ProximityIndicator::ProximityIndicator`
//! 0x140123880, `renderHUD` 0x140123a80, `Node::worldToLocal` 0x14020e730;
//! `DamageDisplayer::DamageDisplayer` 0x140078c10, `renderHUD` 0x14007ae80, `show`
//! 0x14007b7f0, its 18 readers 0x14007a120..0x14007a4a0.
//!
//! The triple-screen path of the name tags (three view matrices of `CameraTripleScreen`) is
//! not here: rustyAC has no such camera.

use std::collections::BTreeMap;

use rustyac_physics::data::ini::IniReader;
use rustyac_physics::vecmath::{xm_matrix_inverse, xm_matrix_multiply, Mat44f, Vec3f};

use crate::control::{Id, NO_CONTROL};
use crate::font::{Font, FontType};
use crate::gui::Gui;
use crate::hud::{HudCar, HudInput};
use crate::painter::{Painter, Texture, ALIGN_CENTER, ALIGN_LEFT, GL_TRIANGLES, GRAPHICS_GL};

/// `eLabelType`.
pub const LABEL_SIMPLE: i32 = 0;
pub const LABEL_WITHOUT_FLAG: i32 = 1;
pub const LABEL_FLAG: i32 = 2;

/// `CarLabel::CarData`.
#[derive(Clone, Debug, PartialEq)]
pub struct CarData {
    pub name: String,
    pub position: i32,
    pub distance: f32,
    pub ahead: i32,
    pub ping: i32,
    pub texture: Option<Texture>,
    pub nation_code: String,
}

impl Default for CarData {
    /// `CarData::CarData` 0x1400697b0.
    fn default() -> CarData {
        CarData { name: String::new(), position: -1, distance: 0.0, ahead: -1, ping: -1, texture: None, nation_code: String::new() }
    }
}

/// `CarLabel`: the tag over a car.
pub struct CarLabel {
    pub control: Id,
    pub nation_code: String,
    pub label_type: i32,
    pub max_distance: f32,
    pub old_data: CarData,
    pub name_parent: Id,
    pub pos_parent: Id,
    pub name: Id,
    pub position: Id,
    pub flag: Id,
    pub arrow: Id,
}

impl CarLabel {
    /// `CarLabel::CarLabel` 0x140069840. (The game names its controls `L"CAR_LABEL" + index`,
    /// `L"Name" + index` and so on: a pointer that many characters into the word, so car 1's
    /// are "AR_LABEL" and "ame"; past a word it reads whatever follows in the game's data,
    /// here nothing.)
    pub fn new(gui: &mut Gui, painter: &mut dyn Painter, index: usize, max_distance: f32, label_type: i32) -> CarLabel {
        let cut = |word: &'static str| word.get(index..).unwrap_or("");
        let plain = |gui: &mut Gui, painter: &mut dyn Painter, name: &str| {
            let c = gui.new_control(painter, name);
            gui.controls[c].draw_background = false;
            gui.controls[c].draw_border = false;
            c
        };
        let orbitron = |gui: &mut Gui, painter: &mut dyn Painter, control: Id, size: f32, bold: bool| {
            let mut font = Font::new(painter, "Orbitron", FontType::Custom, size, false, bold);
            font.shadowed = false;
            gui.fonts.push(font);
            gui.controls[control].font = Some(gui.fonts.len() - 1);
        };
        let control = plain(gui, painter, cut("CAR_LABEL"));
        let mut l = CarLabel { control, nation_code: ".".to_string(), label_type, max_distance, old_data: CarData::default(), name_parent: NO_CONTROL, pos_parent: NO_CONTROL, name: NO_CONTROL, position: NO_CONTROL, flag: NO_CONTROL, arrow: NO_CONTROL };
        l.name = plain(gui, painter, cut("Name"));
        gui.set_text(l.name, "Name");
        gui.set_visible(l.name, true);
        orbitron(gui, painter, l.name, 12.0, false);
        gui.controls[l.name].back_texture_color = [0.0, 0.0, 0.0, 1.0];
        if label_type == LABEL_SIMPLE {
            gui.controls[l.name].font_align = ALIGN_CENTER;
            l.arrow = plain(gui, painter, cut("Arrow"));
            gui.controls[l.arrow].back_texture = painter.get_texture("content/gui/driverNameDisplayer/down.png");
            gui.set_visible(l.arrow, true);
            gui.controls[l.arrow].back_texture_color = [0.0, 0.0, 0.0, 0.9];
            gui.add_control(control, l.name);
            gui.add_control(control, l.arrow);
        } else {
            gui.controls[l.name].font_align = ALIGN_LEFT;
            l.name_parent = gui.new_control(painter, cut("NameParent"));
            gui.controls[l.name_parent].draw_background = true;
            gui.controls[l.name_parent].draw_border = false;
            gui.set_visible(l.name_parent, true);
            gui.controls[l.name_parent].back_texture_color = [0.0, 0.0, 0.0, 0.9];
            gui.add_control(l.name_parent, l.name);
            if label_type == LABEL_FLAG {
                l.flag = plain(gui, painter, cut("Flag"));
                gui.set_visible(l.flag, true);
                gui.controls[l.flag].back_texture_color = [0.9, 0.9, 0.9, 0.9];
                gui.add_control(l.name_parent, l.flag);
            }
            l.position = plain(gui, painter, cut("Position"));
            gui.set_text(l.position, "-");
            gui.set_visible(l.position, true);
            gui.controls[l.position].font_align = ALIGN_CENTER;
            orbitron(gui, painter, l.position, 18.0, true);
            gui.controls[l.position].back_texture_color = [0.9, 0.0, 0.0, 1.0];
            l.arrow = plain(gui, painter, cut("Arrow"));
            gui.controls[l.arrow].back_texture = painter.get_texture("content/gui/driverNameDisplayer/arrow.png");
            gui.set_visible(l.arrow, true);
            gui.controls[l.arrow].back_texture_color = [0.0, 0.0, 0.0, 0.9];
            l.pos_parent = gui.new_control(painter, cut("PosParent"));
            gui.controls[l.pos_parent].draw_background = true;
            gui.controls[l.pos_parent].draw_border = false;
            gui.set_visible(l.pos_parent, true);
            gui.add_control(l.pos_parent, l.position);
            gui.add_control(control, l.name_parent);
            gui.add_control(control, l.pos_parent);
            gui.add_control(control, l.arrow);
            l.reset_rects(gui, 0x3c);
        }
        l
    }

    /// `CarLabel::resetRects` 0x14006a660.
    pub fn reset_rects(&mut self, gui: &mut Gui, w: i32) {
        gui.reset_base_rect(self.control);
        let s = w + 10;
        if self.label_type == LABEL_SIMPLE {
            gui.set_size(self.control, s as f32, 36.0);
            gui.set_size(self.name, s as f32, 19.0);
            gui.set_size(self.arrow, 18.0, 18.0);
            gui.set_position(self.arrow, -9.0, 22.0);
            gui.set_position(self.name, (-(s / 2)) as f32, 3.0);
            return;
        }
        let f = if self.label_type == LABEL_FLAG { 25 } else { 0 };
        let b = f + s;
        gui.set_size(self.control, (b + 49) as f32, 36.0);
        gui.set_size(self.name_parent, b as f32, 22.0);
        gui.set_size(self.name, s as f32, 19.0);
        gui.set_size(self.arrow, 49.0, 13.0);
        gui.set_size(self.pos_parent, 39.0, 22.0);
        gui.set_size(self.position, 39.0, 22.0);
        if self.label_type == LABEL_FLAG {
            gui.set_size(self.flag, 22.0, 22.0);
            gui.set_position(self.flag, 3.0, 0.0);
        }
        gui.set_position(self.arrow, 0.0, 23.0);
        gui.set_position(self.pos_parent, 10.0, 0.0);
        gui.set_position(self.position, -1.0, -1.0);
        gui.set_position(self.name_parent, 49.0, 0.0);
        gui.set_position(self.name, (f + 10) as f32, 3.0);
    }

    /// `CarLabel::updatePilotData` 0x14006a8a0.
    pub fn update_pilot_data(&mut self, gui: &mut Gui, d: &CarData, scale: f32) {
        let fade = 1.0 - (d.distance / self.max_distance);
        let half = fade * 0.5;
        gui.controls[self.arrow].back_texture_color = [0.1, 0.1, 0.1, half];
        if d.name != self.old_data.name {
            gui.set_text(self.name, &d.name);
            let len = d.name.encode_utf16().count();
            let w = (len as f32 * 8.5).round() as i32;
            self.reset_rects(gui, w);
            gui.scale_by_mult(self.control, scale);
        }
        let (g, b) = if d.ping == -1 {
            (1.0, 1.0)
        } else if d.ping > 250 {
            (0.0, 0.0)
        } else if d.ping > 100 {
            (1.0, 0.0)
        } else {
            (1.0, 1.0)
        };
        gui.controls[self.name].fore_color = [1.0, g, b, fade];
        if self.label_type != LABEL_SIMPLE {
            gui.controls[self.name_parent].back_color = [0.1, 0.1, 0.1, half];
            gui.controls[self.position].fore_color = [1.0, 1.0, 1.0, fade];
            if self.label_type == LABEL_FLAG {
                gui.controls[self.flag].fore_color = [1.0, 1.0, 1.0, fade];
                gui.controls[self.flag].back_texture_color = [1.0, 1.0, 1.0, fade];
                if let Some(texture) = &d.texture {
                    gui.controls[self.flag].back_texture = texture.clone();
                }
            }
            if d.position != self.old_data.position {
                gui.set_text(self.position, &d.position.to_string());
            }
            gui.controls[self.pos_parent].back_color = match d.ahead {
                1 => [0.0, 1.0, 0.0, fade * 0.8],
                0 => [0.1, 0.0, 0.9, fade * 0.8],
                _ => [0.74, 0.0, 0.0, fade * 0.8],
            };
        }
        self.old_data = d.clone();
        self.nation_code = d.nation_code.clone();
    }
}

/// `transform` 0x1400243c0 with its divide.
fn transform(p: [f32; 3], m: &Mat44f) -> [f32; 3] {
    let m = &m.m;
    let x = ((p[1] * m[1][0] + p[0] * m[0][0]) + p[2] * m[2][0]) + m[3][0];
    let y = ((p[0] * m[0][1] + p[1] * m[1][1]) + p[2] * m[2][1]) + m[3][1];
    let z = ((p[0] * m[0][2] + p[1] * m[1][2]) + p[2] * m[2][2]) + m[3][2];
    let w = ((p[0] * m[0][3] + p[1] * m[1][3]) + p[2] * m[2][3]) + m[3][3];
    [x / w, y / w, z / w]
}

/// `DriverNameDisplayer`.
pub struct DriverNames {
    /// `GameObject::isActive`: the "Driver Names" key, kept in `gameplay.ini`
    pub is_active: bool,
    pub font_color: [f32; 3],
    pub max_distance: f32,
    pub scale: f32,
    /// `cars`: a car's index and its distance from the camera, as the last frame sorted them
    pub cars: Vec<(usize, f32)>,
    pub car_displayers: Vec<CarLabel>,
    pub label_type: i32,
    pub draw_focused_car: bool,
    pub current_camera_matrix: Mat44f,
    pub current_view_projection_matrix: Mat44f,
    /// `Sim::flagMapCache`
    pub flag_cache: BTreeMap<String, Texture>,
}

impl DriverNames {
    /// `DriverNameDisplayer::DriverNameDisplayer` 0x14008c7e0. `ini` is
    /// `system/cfg/name_displayer.ini`, `active` `gameplay.ini [DRIVER_NAME_DISPLAYER]
    /// IS_ACTIVE` when the section is there.
    pub fn new(gui: &mut Gui, painter: &mut dyn Painter, ini: Option<&IniReader>, active: Option<bool>, cars: usize) -> DriverNames {
        let mut d = DriverNames {
            is_active: true,
            font_color: [0.0; 3],
            max_distance: 0.0,
            scale: 1.0,
            cars: Vec::new(),
            car_displayers: Vec::new(),
            label_type: LABEL_FLAG,
            draw_focused_car: false,
            current_camera_matrix: Mat44f { m: [[0.0; 4]; 4] },
            current_view_projection_matrix: Mat44f { m: [[0.0; 4]; 4] },
            flag_cache: BTreeMap::new(),
        };
        if let Some(ini) = ini.filter(|ini| ini.ready) {
            if ini.has_key("MAIN", "SIMPLE_LABEL") && ini.get_int("MAIN", "SIMPLE_LABEL").unwrap_or(0) > 0 {
                d.label_type = LABEL_SIMPLE;
            } else if ini.has_key("MAIN", "RENDER_FLAG") && ini.get_int("MAIN", "RENDER_FLAG").unwrap_or(0) <= 0 {
                d.label_type = LABEL_WITHOUT_FLAG;
            }
            if ini.has_key("MAIN", "SCALE") {
                let f = ini.get_float("MAIN", "SCALE").unwrap_or(0.0);
                d.scale = if f > 4.0 {
                    4.0
                } else if !(f >= 0.2) {
                    0.2
                } else {
                    f
                };
            }
            d.font_color = ini.get_float3("MAIN", "FONT_COLOR").unwrap_or([0.0; 3]);
            d.max_distance = ini.get_float("MAIN", "MAX_DISTANCE").unwrap_or(0.0);
            if ini.has_key("MAIN", "DRAW_FOCUSEDCAR") {
                d.draw_focused_car = ini.get_int("MAIN", "DRAW_FOCUSEDCAR").unwrap_or(0) != 0;
            }
        }
        if let Some(active) = active {
            d.is_active = active;
        }
        for i in 0..cars {
            let label = CarLabel::new(gui, painter, i, d.max_distance, d.label_type);
            gui.set_visible(label.control, false);
            d.car_displayers.push(label);
        }
        d
    }

    /// `DriverNameDisplayer::update` 0x14008e2c0: the camera's matrices of this frame.
    pub fn update(&mut self, camera_matrix: &Mat44f, view: &Mat44f, perspective: &Mat44f) {
        self.current_camera_matrix = *camera_matrix;
        self.current_view_projection_matrix = xm_matrix_multiply(view, perspective);
    }

    /// `Sim::getNationFlag` 0x140199350.
    pub fn nation_flag(&mut self, painter: &mut dyn Painter, code: &str) -> Texture {
        if code.is_empty() {
            return self.flag_cache.entry("AC".to_string()).or_default().clone();
        }
        if let Some(t) = self.flag_cache.get(code) {
            return t.clone();
        }
        let t = painter.get_texture(&format!("content/gui/NationFlags/{code}.png"));
        if t.kid.is_some() {
            self.flag_cache.insert(code.to_string(), t.clone());
            return t;
        }
        self.flag_cache.entry("AC".to_string()).or_default().clone()
    }

    /// `DriverNameDisplayer::projectPoint` 0x14008d550 for one screen: the pixel of a point, or
    /// (-1, -1) behind the camera.
    pub fn project_point(&self, screen: (i32, i32), p: [f32; 3]) -> [f32; 2] {
        let c = transform(p, &self.current_view_projection_matrix);
        let (w, h) = (screen.0 as f32, screen.1 as f32);
        let cam = &self.current_camera_matrix.m;
        let mut d = Vec3f::new(p[0] - cam[3][0], p[1] - cam[3][1], p[2] - cam[3][2]);
        d.normalize();
        let dot = ((-cam[2][1] * d.y) + (-cam[2][0] * d.x)) + (-cam[2][2] * d.z);
        if dot.acos() < f32::from_bits(0x3fc9_0fd0) {
            [((c[0] + 1.0) * 0.5) * w, ((1.0 - c[1]) * 0.5) * h]
        } else {
            [-1.0, -1.0]
        }
    }

    /// `DriverNameDisplayer::renderHUD` 0x14008d900: the tags are placed and drawn, the farthest
    /// first.
    pub fn render_hud(&mut self, gui: &mut Gui, painter: &mut dyn Painter, dt: f32, input: &HudInput) {
        if input.pause_menu_visible || input.esc_menu_visible || !self.is_active || input.triple_screen {
            return;
        }
        if self.cars.len() != input.cars.len() {
            self.cars = (0..input.cars.len()).map(|i| (i, 0.0)).collect();
        }
        let cam = input.camera_matrix.m[3];
        for e in &mut self.cars {
            let a = input.cars[e.0].world_position;
            let (dz, dy, dx) = (a[2] - cam[2], a[1] - cam[1], a[0] - cam[0]);
            let mut s = dy * dy;
            s += dx * dx;
            s += dz * dz;
            e.1 = if s == 0.0 { 0.0 } else { s.sqrt() };
        }
        // std::sort with `a.distance > b.distance`: an insertion sort up to 32 cars
        for i in 1..self.cars.len() {
            let v = self.cars[i];
            let mut j = i;
            while j > 0 && v.1 > self.cars[j - 1].1 {
                self.cars[j] = self.cars[j - 1];
                j -= 1;
            }
            self.cars[j] = v;
        }
        let session_type = input.session_type;
        let focused = input.focused_car_index;
        let my_laps = input.cars.get(focused as usize).map(|c| c.lap_count).unwrap_or(0);
        let (w, h) = gui.screen;
        for k in 0..self.cars.len() {
            let (guid, distance) = self.cars[k];
            let car: &HudCar = &input.cars[guid];
            let control = self.car_displayers[guid].control;
            gui.set_visible(control, false);
            if !car.connected {
                continue;
            }
            if (input.camera_mode == 1 || input.camera_mode == 2) && guid as i32 == focused {
                continue;
            }
            let m = &car.body_matrix.m;
            let p = [m[3][0], m[3][1] + 1.5, m[3][2]];
            let s = self.project_point(gui.screen, p);
            if s[0] == -1.0 && s[1] == -1.0 {
                continue;
            }
            if !(s[0] > 0.0 && s[1] > 0.0) {
                continue;
            }
            if !(s[0] < w as f32 && s[1] < h as f32) {
                continue;
            }
            if !(self.max_distance > distance) {
                continue;
            }
            if !(self.draw_focused_car || guid as i32 != focused) {
                continue;
            }
            let mut data = CarData { name: car.driver_name.clone(), nation_code: car.nation_code.clone(), ..CarData::default() };
            data.position = if session_type != 3 { car.leaderboard_position } else { car.real_time_position + 1 };
            data.distance = distance;
            if self.label_type == LABEL_FLAG && self.car_displayers[guid].nation_code != car.nation_code {
                data.texture = Some(self.nation_flag(painter, &car.nation_code));
            }
            if session_type == 3 {
                let laps = car.lap_count;
                if my_laps < laps {
                    data.ahead = 1;
                } else if laps < my_laps {
                    data.ahead = 0;
                }
            }
            if input.online {
                data.ping = car.ping;
            }
            let scale = self.scale;
            self.car_displayers[guid].update_pilot_data(gui, &data, scale);
            gui.set_position(control, (s[0] as i32) as f32, ((s[1] as i32) - 0x2d) as f32);
            gui.set_visible(control, true);
            gui.render(control, dt, painter);
        }
    }
}

/// `ProximityIndicator`: a red triangle at the edge of the screen towards each car within ten
/// metres.
pub struct ProximityIndicator {
    pub is_active: bool,
}

impl ProximityIndicator {
    /// `ProximityIndicator::ProximityIndicator` 0x140123880; `ini` is
    /// `system/cfg/proximity_indicator.ini`.
    pub fn new(ini: Option<&IniReader>) -> ProximityIndicator {
        let mut is_active = true;
        if let Some(ini) = ini.filter(|ini| ini.ready) {
            is_active = ini.get_int("SETTINGS", "HIDE").unwrap_or(0) != 1;
        }
        ProximityIndicator { is_active }
    }

    /// `ProximityIndicator::renderHUD` 0x140123a80.
    pub fn render_hud(&mut self, gui: &Gui, painter: &mut dyn Painter, input: &HudInput) {
        if input.esc_menu_visible || !gui.is_active || input.pause_menu_visible || (input.camera_mode & !2) != 0 {
            return;
        }
        let fi = input.focused_car_index as usize;
        let Some(me) = input.cars.get(fi) else { return };
        let p = me.world_position;
        let (h, w) = (gui.screen.1 as f32, gui.screen.0 as f32);
        let s = (h * f32::from_bits(0x3aaa_aaab)) * 20.0;
        for (b, o) in input.cars.iter().enumerate() {
            if b == fi {
                continue;
            }
            let mo = o.body_matrix.m[3];
            let (dy, dx, dz) = (p[1] - mo[1], p[0] - mo[0], p[2] - mo[2]);
            let mut d2 = dy * dy;
            d2 += dx * dx;
            d2 += dz * dz;
            if d2 >= 100.0 {
                continue;
            }
            if !o.connected {
                continue;
            }
            // Node::worldToLocal 0x14020e730
            let mi = xm_matrix_inverse(&me.body_matrix).m;
            let tx = ((mo[1] * mi[1][0] + mo[0] * mi[0][0]) + mo[2] * mi[2][0]) + mi[3][0];
            let tz = ((mo[1] * mi[1][2] + mo[0] * mi[0][2]) + mo[2] * mi[2][2]) + mi[3][2];
            let (mut ax, mut az) = (tx * -1.0, tz * -1.0);
            let len = (ax * ax + az * az).sqrt();
            if len != 0.0 {
                let inv = 1.0 / len;
                ax = ax * inv;
                az = az * inv;
            }
            let (cx, cy) = (((ax + 1.0) * w) * 0.5, ((az + 1.0) * h) * 0.5);
            let nx2 = -ax;
            let u = ax * 0.0 - az * 0.0;
            painter.gl_color4f(GRAPHICS_GL, 1.0, 0.0, 0.0, 0.6);
            painter.gl_begin(GRAPHICS_GL, GL_TRIANGLES);
            painter.gl_vertex3f(GRAPHICS_GL, cx, cy, 0.0);
            let (ex, ez, z0, px, pz) = (ax * s, az * s, s * 0.0, nx2 * s, u * s);
            painter.gl_vertex3f(GRAPHICS_GL, ez + (cx - ex), px + (cy - ez), pz + (0.0 - z0));
            painter.gl_vertex3f(GRAPHICS_GL, (cx - ex) - ez, (cy - ez) - px, (0.0 - z0) - pz);
            painter.gl_end(GRAPHICS_GL);
        }
    }
}

/// `DamageItem`.
pub struct DamageItem {
    pub texture: Texture,
    pub health: f32,
    pub blink_time: f32,
}

/// The pictures of `DamageDisplayer` in the order it makes (and draws) them.
pub const DAMAGE_PICTURES: [&str; 18] = ["engine", "gearbox", "front", "rear", "left", "right", "tyre_fl", "tyre_fr", "tyre_rl", "tyre_rr", "sus_fl", "sus_fr", "sus_rl", "sus_rr", "flatspot_fl", "flatspot_fr", "flatspot_rl", "flatspot_rr"];

/// `DamageDisplayer`: the car seen from above, part by part, white to yellow to red.
pub struct DamageDisplayer {
    /// the "Hide Damage" key, kept in `gameplay.ini`; off while a replay shows
    pub enabled: bool,
    pub items: Vec<DamageItem>,
    pub old_car_index: i32,
    pub initialized: bool,
    pub render_timer: f32,
    pub max_render_timer: f32,
    pub max_damage_kmh: f32,
    pub font: Option<Font>,
    pub print_values: bool,
    pub x_off: f32,
    pub y_off: f32,
}

impl DamageDisplayer {
    /// `DamageDisplayer::DamageDisplayer` 0x140078c10. `ini` is
    /// `system/cfg/damage_displayer.ini`, `active` `gameplay.ini [DAMAGE_DISPLAYER] IS_ACTIVE`
    /// when the section is there.
    pub fn new(painter: &mut dyn Painter, ini: Option<&IniReader>, active: Option<i32>, focused_car_index: i32) -> DamageDisplayer {
        let mut d = DamageDisplayer { enabled: true, items: Vec::new(), old_car_index: focused_car_index, initialized: false, render_timer: 0.0, max_render_timer: 4.0, max_damage_kmh: 60.0, font: None, print_values: false, x_off: 0.0, y_off: 0.0 };
        if let Some(ini) = ini.filter(|ini| ini.ready) {
            d.max_render_timer = ini.get_float("MAIN", "TIME").unwrap_or(0.0);
            d.max_damage_kmh = ini.get_float("MAIN", "MAX_DAMAGE_KMH").unwrap_or(0.0);
            d.x_off = ini.get_float("MAIN", "POSITION_X").unwrap_or(0.0);
            d.y_off = ini.get_float("MAIN", "DISTANCE_FROM_CENTER_Y").unwrap_or(0.0);
            d.print_values = ini.get_int("MAIN", "PRINT_VALUES").unwrap_or(0) == 1;
        }
        if d.print_values {
            d.font = Some(Font::new(painter, "", FontType::Proportional, 15.0, false, false));
        }
        if let Some(active) = active {
            d.enabled = 0 < active;
        }
        for name in DAMAGE_PICTURES {
            d.items.push(DamageItem { texture: painter.get_texture(&format!("content/texture/damage/{name}.png")), health: 1.0, blink_time: 0.0 });
        }
        d
    }

    /// `DamageDisplayer::show` 0x14007b7f0 (the "Show Damage" key).
    pub fn show(&mut self) {
        self.render_timer = self.max_render_timer;
    }

    /// The 18 readers: a part's health from 1 (fine) to 0.
    fn fetch(&self, item: usize, car: &HudCar) -> f32 {
        match item {
            0 => car.engine_life_left * 0.001,
            1 => car.gearbox,
            2..=5 => {
                let (m, z) = (self.max_damage_kmh, car.damage_zone_level[item - 2]);
                let c = if z > m {
                    m
                } else if !(z >= 0.0) {
                    0.0
                } else {
                    z
                };
                1.0 - (c / m)
            }
            6..=9 => car.tyre_inflation[item - 6],
            10..=13 => 1.0 - car.sus_damage[item - 10],
            _ => 1.0 - car.tyre_flat_spot[item - 14],
        }
    }

    /// `DamageDisplayer::renderHUD` 0x14007ae80.
    pub fn render_hud(&mut self, gui: &Gui, painter: &mut dyn Painter, dt: f32, input: &HudInput) {
        if input.end_session_active || input.esc_menu_visible || !gui.is_active || input.pause_menu_visible || !self.enabled {
            return;
        }
        let Some(car) = input.cars.get(input.focused_car_index as usize) else { return };
        if !self.initialized || self.old_car_index != input.focused_car_index {
            for i in 0..self.items.len() {
                self.items[i].health = self.fetch(i, car);
            }
            self.initialized = true;
            return;
        }
        let mut show = 0.0 < self.render_timer;
        for i in 0..self.items.len() {
            let n = self.fetch(i, car);
            let item = &mut self.items[i];
            if n > item.health {
                item.health = n;
            } else if n < item.health || n <= 0.0 {
                item.health = n;
                item.blink_time = 1.0;
                show = true;
                self.render_timer = self.max_render_timer;
            }
        }
        if show {
            let y = (gui.screen.1 / 2) as f32 + self.y_off;
            let red_phase = ((input.game_time_now as i32) / 100) % 2 != 0;
            for item in &mut self.items {
                painter.gl_color4f(GRAPHICS_GL, 1.0, 1.0, 1.0, 1.0);
                if item.blink_time > 0.0 && red_phase {
                    painter.gl_color4f(GRAPHICS_GL, 1.0, 0.0, 0.0, 1.0);
                    item.blink_time = item.blink_time - (dt * 2.0);
                } else {
                    let h = item.health;
                    if h > 0.5 {
                        let u = (h - 0.5) * 2.0;
                        painter.gl_color4f(GRAPHICS_GL, (u * 0.0) + 1.0, (u * 0.0) + 1.0, u, 1.0);
                    } else {
                        let u = h * 2.0;
                        painter.gl_color4f(GRAPHICS_GL, (u * 0.0) + 1.0, u, u * 0.0, 1.0);
                    }
                }
                painter.set_texture(0, &item.texture);
                let (tw, th) = painter.texture_size(&item.texture);
                painter.gl_quad(GRAPHICS_GL, self.x_off, y, tw as f32, th as f32, true);
            }
            if let (true, Some(font), Some(car)) = (self.print_values, &mut self.font, input.cars.first()) {
                let lines = [
                    ("BODY FRONT : ", car.damage_zone_level[0]),
                    ("BODY REAR : ", car.damage_zone_level[1]),
                    ("BODY LEFT : ", car.damage_zone_level[2]),
                    ("BODY RIGHT : ", car.damage_zone_level[3]),
                    ("SUSPENSION FL : ", 1.0 - car.sus_damage[0]),
                    ("SUSPENSION FR : ", 1.0 - car.sus_damage[1]),
                    ("SUSPENSION RL : ", 1.0 - car.sus_damage[2]),
                    ("SUSPENSION RR : ", 1.0 - car.sus_damage[3]),
                    ("ENGINE LIFE : ", car.engine_life_left),
                    ("GEAR : ", car.gearbox),
                ];
                for (i, (prefix, value)) in lines.iter().enumerate() {
                    font.blit_string(painter, 0.0, (i * 25) as f32, &format!("{prefix}{:.6}", *value as f64), 1.0, ALIGN_LEFT);
                }
            }
        }
        self.render_timer = self.render_timer - dt;
    }
}
