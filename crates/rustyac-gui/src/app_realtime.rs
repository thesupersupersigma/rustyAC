// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! `FormRealTimeTiming` (0x318 bytes), the app "Realtime": the three cars ahead of the player
//! and the three behind with the gaps on the track, the last laps anybody finished, the
//! track's state and the wind.
//!
//! `FormRealTimeTiming::FormRealTimeTiming` 0x14032b8a0, `render` 0x14032cda0, its handlers of
//! `RaceManager::evOnLapCompleted` (0x14032c7b0) and `Sim::evOnNewSession` (0x14032cca0);
//! `CarAvatar::getDistancesOnSpline` 0x1400d3330 and `getTimeDifferenceOnSpline` 0x1400d3860.

use std::any::Any;

use crate::app_essentials::cut_f;
use crate::app_laps::cut;
use crate::apps::{App, AppCar, AppLink};
use crate::control::Id;
use crate::font::{Font, FontType};
use crate::gui::Gui;
use crate::hud::{time_to_string3, translate};
use crate::overlay_leaderboard::time_to_diff_short;
use crate::painter::{Painter, Texture, ALIGN_LEFT, ALIGN_RIGHT, GL_LINES, GRAPHICS_GL};

const BLUE: [f32; 4] = [f32::from_bits(0x3e0f_5c29), f32::from_bits(0x3e75_c28f), 1.0, 1.0];
const GREEN: [f32; 4] = [0.0, 1.0, 0.0, 1.0];
const WHITE: [f32; 4] = [1.0; 4];

pub struct Realtime {
    pub names: [Id; 7],
    pub times: [Id; 7],
    pub positions: [Id; 7],
    pub lap_times_names: [Id; 3],
    pub lap_times_times: [Id; 3],
    pub lap_times_timestamps: [f64; 3],
    pub lap_times_colors: [[f32; 4]; 3],
    pub current_label: i32,
    pub best_lap: u32,
    pub track_status: Id,
    pub tx_wind_indicator: Texture,
}

impl Gui {
    /// `FormRealTimeTiming::FormRealTimeTiming(Sim&)` 0x14032b8a0.
    pub fn new_realtime_app(&mut self, painter: &mut dyn Painter) -> Id {
        let form = self.new_form(painter, "REAL_TIME_APP", true);
        self.form_set_title(form, &translate("Realtime"));
        self.form_data_mut(form).dev_app = false;
        self.form_data_mut(form).autohide = true;
        self.controls[form].back_color = [0.0, 0.0, 0.0, f32::from_bits(0x3f4c_cccd)];
        let label = |gui: &mut Gui, painter: &mut dyn Painter, name: String, text: &str, align: i32, w: f32, x: f32, y: f32| {
            let id = gui.new_label(painter, &name);
            gui.set_text(id, text);
            gui.controls[id].font_align = align;
            gui.set_size(id, w, 20.0);
            gui.set_position(id, x, y);
            gui.fonts.push(Font::new(painter, "", FontType::Monospaced, 16.0, false, false));
            gui.controls[id].font = Some(gui.fonts.len() - 1);
            gui.controls[id].fore_color = WHITE;
            gui.add_control(form, id);
            id
        };
        let (mut names, mut times, mut positions) = ([0; 7], [0; 7], [0; 7]);
        for i in 0..7 {
            let y = i as f32 * 20.0 + 30.0;
            positions[i] = label(self, painter, format!("LABEL_POS_D{i}"), "", ALIGN_LEFT, 30.0, 5.0, y);
            names[i] = label(self, painter, format!("LABEL_NAME_D{i}"), &format!("DRIVER{i}"), ALIGN_LEFT, 150.0, 35.0, y);
            times[i] = label(self, painter, format!("LABEL_TIME_D{i}"), "-12.4", ALIGN_RIGHT, 40.0, 240.0, y);
        }
        for id in [positions[3], names[3], times[3]] {
            self.controls[id].fore_color = [1.0, 1.0, 0.0, 1.0];
        }
        let (mut lap_times_names, mut lap_times_times) = ([0; 3], [0; 3]);
        for k in 0..3 {
            let y = k as f32 * 20.0 + 170.0 + 40.0;
            lap_times_names[k] = label(self, painter, format!("LABEL_NAME_L{k}"), "", ALIGN_LEFT, 150.0, 5.0, y);
            lap_times_times[k] = label(self, painter, format!("LABEL_TIME_L{k}"), "", ALIGN_RIGHT, 40.0, 240.0, y);
        }
        let track_status = self.new_label(painter, "TRACK_STATUS");
        self.set_position(track_status, 5.0, 273.0);
        let r = self.controls[form].rect;
        self.set_size(track_status, (r.right - r.left) - 10.0, 20.0);
        self.controls[track_status].back_color = [1.0, 0.0, 0.0, 1.0];
        self.fonts.push(Font::new(painter, "", FontType::Proportional, 12.0, false, false));
        self.controls[track_status].font = Some(self.fonts.len() - 1);
        self.add_control(form, track_status);
        self.set_size(form, 285.0, 300.0);
        let tx_wind_indicator = painter.get_texture("content/gui/wind_indicator.png");
        let m = self.controls[form].scale_mult;
        self.scale_by_mult(form, m);
        self.set_app(
            form,
            Box::new(Realtime {
                names,
                times,
                positions,
                lap_times_names,
                lap_times_times,
                lap_times_timestamps: [0.0; 3],
                lap_times_colors: [[0.0; 4]; 3],
                current_label: 0,
                best_lap: 0,
                track_status,
                tx_wind_indicator,
            }),
        );
        form
    }
}

/// `x < 0.0` taken as the game's `comiss x, 0; jae` does: a NaN counts as below.
fn wrap(x: f32) -> f32 {
    if x >= 0.0 {
        x
    } else {
        x + 1.0
    }
}

/// `CarAvatar::getDistancesOnSpline` 0x1400d3330: the way forward to `c`, and back.
fn distances_on_spline(me: &AppCar, c: &AppCar, length: f32) -> (f32, f32) {
    let a = wrap(c.state.normalized_spline_position - me.state.normalized_spline_position);
    let b = wrap(me.state.normalized_spline_position - c.state.normalized_spline_position);
    (length * a, length * b)
}

/// `CarAvatar::getTimeDifferenceOnSpline` 0x1400d3860 (ms; positive: `c` is ahead).
fn time_difference_on_spline(me: &AppCar, c: &AppCar, length: f32) -> f64 {
    let a = wrap(c.state.normalized_spline_position - me.state.normalized_spline_position) * length;
    let b = wrap(me.state.normalized_spline_position - c.state.normalized_spline_position) * length;
    let floor = |v: f32| if v <= 0.1 || v.is_nan() { f32::from_bits(0x3dcc_cccd) } else { v };
    if b > a {
        let v = floor(me.state.speed);
        ((a / v) * 1000.0) as f64
    } else {
        let v = floor(c.state.speed);
        (-(b / v) * 1000.0) as f64
    }
}

/// The first 20 characters of a name (`substr(0, 0x14)`).
fn short(name: &str) -> String {
    let units: Vec<u16> = name.encode_utf16().take(20).collect();
    String::from_utf16_lossy(&units)
}

fn place(link: &AppLink, c: &AppCar) -> String {
    if link.session_type == 3 {
        c.realtime_position.wrapping_add(1).to_string()
    } else {
        c.leaderboard_position.to_string()
    }
}

impl Realtime {
    fn row(&self, gui: &mut Gui, link: &AppLink, row: usize, me: &AppCar, c: &AppCar) {
        gui.set_text(self.positions[row], &place(link, c));
        gui.set_text(self.names[row], &short(&c.driver_name));
        let d = time_difference_on_spline(me, c, link.ai_spline_length);
        gui.set_text(self.times[row], &time_to_diff_short(cut(d)));
    }

    fn paint(&self, gui: &mut Gui, row: usize, colour: [f32; 4]) {
        gui.controls[self.names[row]].fore_color = colour;
        gui.controls[self.times[row]].fore_color = colour;
        gui.controls[self.positions[row]].fore_color = colour;
    }
}

fn by_count(p: u32, o: u32) -> [f32; 4] {
    if p > o {
        BLUE
    } else if p < o {
        GREEN
    } else {
        WHITE
    }
}

impl App for Realtime {
    fn as_any(&mut self) -> &mut dyn Any {
        self
    }

    /// `FormRealTimeTiming::render` 0x14032cda0.
    fn render(&mut self, gui: &mut Gui, id: Id, dt: f32, painter: &mut dyn Painter) {
        let link = gui.apps.clone();
        let gl = GRAPHICS_GL;
        let s = gui.controls[id].scale_mult;
        let row_h = s * 20.0;
        let [x0, y0] = gui.local_to_world(id, [0.0, 0.0]);
        let w = {
            let r = gui.controls[id].rect;
            r.right - r.left
        };
        let white = [1.0, 1.0, 1.0, f32::from_bits(0x3e99_999a)];
        let dark = [0.0, 0.0, 0.0, f32::from_bits(0x3f4c_cccd)];
        let line = |painter: &mut dyn Painter, y: f32| {
            painter.gl_color4f(gl, white[0], white[1], white[2], white[3]);
            painter.gl_begin(gl, GL_LINES);
            painter.gl_vertex3f(gl, x0, y, 0.0);
            painter.gl_vertex3f(gl, w + x0, y, 0.0);
            painter.gl_end(gl);
        };
        let mut y = y0 + s * 30.0;
        line(painter, y);
        painter.gl_color4f(gl, dark[0], dark[1], dark[2], dark[3]);
        let h1 = row_h * 7.0;
        painter.gl_quad(gl, x0, y + s, w, h1, false);
        y = y + (s * 2.0 + h1);
        line(painter, y);
        y = y + s * 38.0;
        line(painter, y);
        painter.gl_color4f(gl, dark[0], dark[1], dark[2], dark[3]);
        let h2 = row_h * 3.0;
        painter.gl_quad(gl, x0, y + s, w, h2, false);
        y = y + (s * 2.0 + h2);
        line(painter, y);

        for i in 0..7 {
            gui.set_text(self.positions[i], "");
            gui.set_text(self.names[i], "");
            gui.set_text(self.times[i], "");
        }
        let me = link.cars.first().cloned().unwrap_or_default();
        let race = link.session_type == 3;
        let (mut ahead, mut behind): (Vec<(usize, f32)>, Vec<(usize, f32)>) = (Vec::new(), Vec::new());
        for k in 1..link.cars.len() {
            let c = &link.cars[k];
            if c.connected && !c.in_pitlane {
                let (a, b) = distances_on_spline(&me, c, link.ai_spline_length);
                if !(a > b) {
                    ahead.push((k, a));
                } else {
                    behind.push((k, b));
                }
            }
        }
        let order = |x: &(usize, f32), y: &(usize, f32)| if x.1 < y.1 { std::cmp::Ordering::Less } else if y.1 < x.1 { std::cmp::Ordering::Greater } else { std::cmp::Ordering::Equal };
        ahead.sort_by(order);
        behind.sort_by(order);
        gui.set_text(self.positions[3], &place(&link, &me));
        gui.set_text(self.names[3], &short(&me.driver_name));
        gui.set_text(self.times[3], "0.0");
        for (n, (k, _)) in ahead.iter().take(3).enumerate() {
            let row = 2 - n;
            let c = &link.cars[*k];
            self.row(gui, &link, row, &me, c);
            let colour = if race {
                by_count(me.lap_count, c.lap_count)
            } else {
                let (p, o) = (me.leaderboard_position, c.leaderboard_position);
                if p > o {
                    BLUE
                } else if p < o {
                    GREEN
                } else {
                    WHITE
                }
            };
            self.paint(gui, row, colour);
        }
        for (n, (k, _)) in behind.iter().take(3).enumerate() {
            let row = 4 + n;
            let c = &link.cars[*k];
            self.row(gui, &link, row, &me, c);
            self.paint(gui, row, by_count(me.lap_count, c.lap_count));
        }
        for k in 0..3 {
            let v = (link.game_time - self.lap_times_timestamps[k]) * 0.0001;
            let v = if v > 1.0 {
                1.0
            } else if v >= 0.0 {
                v
            } else {
                0.0
            };
            let f = 1.0 - (v as f32) * f32::from_bits(0x3e99_999a);
            let c = self.lap_times_colors[k];
            let col = [f * c[0], f * c[1], f * c[2], 1.0];
            gui.controls[self.lap_times_names[k]].fore_color = col;
            gui.controls[self.lap_times_times[k]].fore_color = col;
        }
        let status = format!("ROAD: {}C AIR: {}C GRIP: {}% WIND: {}kmh", cut_f(link.road_temperature), cut_f(link.ambient_temperature), cut_f(link.grip * 100.0), cut_f(link.wind_speed * f32::from_bits(0x4066_6666)));
        gui.set_text(self.track_status, &status);
        gui.control_render(id, dt, painter);

        // the wind's arrow
        painter.set_texture(0, &self.tx_wind_indicator);
        let m = me.state.world_matrix.m;
        let mut z = -m[2][2];
        let mut x = -m[2][0];
        let nonzero = |v: f32| !(v == 0.0 || v.is_nan());
        if nonzero(x) || nonzero(z) {
            let len = rustyac_math::sqrtf(x * x + z * z);
            if nonzero(len) {
                let inv = 1.0 / len;
                x = x * inv;
                z = inv * z;
            }
            let a = rustyac_math::atan2f(x, z);
            let angle = -(a * f32::from_bits(0x4265_2ee1));
            painter.gl_color3f(gl, 1.0, 1.0, 1.0);
            let (tw, th) = if self.tx_wind_indicator.kid.is_some() { painter.texture_size(&self.tx_wind_indicator) } else { (0, 0) };
            let (tw, th) = (tw as f32, th as f32);
            let r = gui.controls[id].rect;
            let px = (r.right - tw * 0.5) - 1.0;
            let py = (gui.controls[self.track_status].rect.top + r.top) + th * 0.5;
            let rot = (link.wind_direction_deg - angle) * f32::from_bits(0x3c8e_f998);
            painter.gl_quad_centred_rotated(gl, px, py, tw, th, rot, true);
        }
    }

    /// The handler of `RaceManager::evOnLapCompleted` 0x14032c7b0.
    fn on_lap_completed(&mut self, gui: &mut Gui, _id: Id, car: u32, time: u32) {
        let link = gui.apps.clone();
        let c = link.cars.get(car as usize).cloned().unwrap_or_default();
        let colour = |me: &mut Realtime| {
            if c.best_lap.time == time {
                [0.0, 1.0, 0.0, 1.0]
            } else {
                let _ = me;
                [1.0, 0.0, 0.0, 1.0]
            }
        };
        let row = if self.current_label <= 2 {
            let k = self.current_label as usize;
            gui.set_text(self.lap_times_times[k], &time_to_string3(time as i32));
            gui.set_text(self.lap_times_names[k], &short(&c.driver_name));
            self.current_label += 1;
            k
        } else {
            for k in 0..2 {
                let t = gui.controls[self.lap_times_times[k + 1]].text.clone();
                gui.set_text(self.lap_times_times[k], &t);
                let n = gui.controls[self.lap_times_names[k + 1]].text.clone();
                gui.set_text(self.lap_times_names[k], &n);
                self.lap_times_timestamps[k] = self.lap_times_timestamps[k + 1];
                self.lap_times_colors[k] = self.lap_times_colors[k + 1];
            }
            gui.set_text(self.lap_times_times[2], &time_to_string3(time as i32));
            gui.set_text(self.lap_times_names[2], &c.driver_name);
            2
        };
        self.lap_times_timestamps[row] = link.game_time;
        self.lap_times_colors[row] = colour(self);
        if time < self.best_lap || self.best_lap == 0 {
            self.lap_times_colors[row] = [1.0, 0.0, 1.0, 1.0];
            self.best_lap = time;
        }
    }

    /// The handler of `Sim::evOnNewSession` 0x14032cca0.
    fn on_new_session(&mut self, _gui: &mut Gui, _id: Id) {
        self.best_lap = 0;
    }
}
