// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! Two developers' apps: `ACDriverPerformance` (0x2a8 bytes, "Driver performances": 0 -> 50
//! -> 0 ... 0 -> 160 -> 0 km/h and 100 m ... 1 km timed, with their records in
//! `savedData\driverPerformances\<track>\<layout>\<car>.ini`) and `RaceFuelMonitor` (0x1c0
//! bytes, "Race Fuel Monitor": every car's fuel and laps of fuel).
//!
//! `ACDriverPerformance::ACDriverPerformance` 0x140303bd0, `render` 0x140305d90, `updateData`
//! 0x140307650, `renderOnVideo` 0x140305e00, `fromFloatToString` 0x140305120,
//! `loadDriverRecords` 0x1403052e0, `saveDriverRecords` 0x140306df0 (its destructor: here
//! [`App::shutdown`]); `RaceFuelMonitor::RaceFuelMonitor` 0x140338250, `render` 0x140338460.

use std::any::Any;

use crate::acos::format_g6;
use crate::apps::{App, AppAction};
use crate::control::Id;
use crate::font::{Font, FontType};
use crate::gui::Gui;
use crate::hud::translate;
use crate::painter::{Painter, ALIGN_LEFT};
use crate::widgets2::format_fixed;
use rustyac_physics::data::ini::IniReader;

/// `perf` (0x60 bytes).
#[derive(Clone, Debug, PartialEq)]
pub struct Perf {
    pub name: String,
    pub started_to_brake_position: [f32; 3],
    /// 0 a distance, 1 a speed (up and down)
    pub kind: i32,
    pub objective: f32,
    pub seconds: f32,
    pub seconds_breaking: f32,
    pub record: f32,
    pub record_breaking: f32,
    pub last_seconds: f32,
    pub last_breaking_seconds: f32,
    pub speed_at_checkpoint: f32,
    pub distance_breaking: f32,
    pub last_speed: f32,
    pub count_down_active: bool,
    pub beaten: bool,
    pub started_to_brake: bool,
}

impl Perf {
    /// `perf::perf(name, objective, type)` 0x140304b60.
    pub fn new(name: &str, objective: f32, kind: i32) -> Perf {
        Perf { name: name.to_string(), started_to_brake_position: [0.0; 3], kind, objective, seconds: 0.0, seconds_breaking: 0.0, record: f32::MAX, record_breaking: f32::MAX, last_seconds: 0.0, last_breaking_seconds: 0.0, speed_at_checkpoint: 0.0, distance_breaking: 0.0, last_speed: 0.0, count_down_active: false, beaten: false, started_to_brake: false }
    }
}

/// `ACDriverPerformance`'s own members.
pub struct DriverPerformance {
    pub perfs: Vec<Perf>,
    pub current_distance: f32,
    pub stop_limit: f32,
    pub previous_car_position: [f32; 3],
    pub brake_threshold: f32,
    pub time_to_wait: f32,
    pub waiting_time: f32,
    pub version: i32,
    pub car_name: String,
    /// the record file's path under `Documents\Assetto Corsa\`
    pub file_name: String,
    pub string_font_size: f32,
    pub seconds_font_size: f32,
    pub font: usize,
}

/// `cvttss2si` into 32 bits.
#[allow(clippy::manual_range_contains)]
fn cvtt(x: f32) -> i32 {
    if x.is_nan() || x >= 2_147_483_648.0 || x < -2_147_483_648.0 {
        i32::MIN
    } else {
        x as i32
    }
}

/// `ACDriverPerformance::fromFloatToString` 0x140305120: cut toward minus infinity to `digits`
/// decimals, `%g`; nothing ("-") for 0.
pub fn from_float_to_string(value: f32, digits: f32) -> String {
    let p = rustyac_math::powf(10.0, digits);
    let mut x = p * value;
    let t = cvtt(x);
    if t != i32::MIN && t as f32 != x {
        x = (t - x.is_sign_negative() as i32) as f32;
    }
    let r = x / p;
    if r == 0.0 || r.is_nan() {
        "-".to_string()
    } else {
        format_g6(r)
    }
}

/// The distance between two points as `updateData` adds it (y, x, z).
fn distance(a: [f32; 3], b: [f32; 3]) -> f32 {
    let dy = a[1] - b[1];
    let dx = a[0] - b[0];
    let dz = a[2] - b[2];
    let d2 = dy * dy + dx * dx;
    let d2 = d2 + dz * dz;
    if d2 != 0.0 && !d2.is_nan() {
        d2.sqrt()
    } else {
        0.0
    }
}

impl DriverPerformance {
    /// `ACDriverPerformance::loadDriverRecords` 0x1403052e0 on the file's text (false: the
    /// built-in measures).
    fn load(&mut self, text: Option<&str>, unix: &str) -> bool {
        let Some(text) = text else { return false };
        let r = IniReader::from_text(std::path::Path::new(&self.file_name), text);
        if !r.ready || r.get_int("MAIN", "VERSION").unwrap_or(0) < self.version {
            return false;
        }
        self.car_name = r.get_string("MAIN", "CAR_NAME");
        if self.car_name != unix {
            return false;
        }
        let n = r.get_int("MAIN", "PERF_SIZE").unwrap_or(0) as u32;
        self.perfs.clear();
        for i in 0..n {
            let sec = format!("PERF_{i}");
            let mut p = Perf::new(&r.get_string(&sec, "NAME"), r.get_float(&sec, "OBJECTIVE").unwrap_or(0.0), r.get_int(&sec, "TYPE").unwrap_or(0));
            p.seconds = 0.0;
            p.record = r.get_float(&sec, "RECORD").unwrap_or(0.0);
            p.record_breaking = r.get_float(&sec, "RECORD_COUNT_DOWN").unwrap_or(0.0);
            p.speed_at_checkpoint = r.get_float(&sec, "CP_SPEED").unwrap_or(0.0);
            if p.record == 0.0 {
                p.record = f32::MAX;
            }
            if p.record_breaking == 0.0 {
                p.record_breaking = f32::MAX;
            }
            self.perfs.push(p);
        }
        true
    }

    /// `ACDriverPerformance::saveDriverRecords` 0x140306df0: the file's bytes (a wide
    /// `ofstream` in text mode: CRLF).
    pub fn save_bytes(&self) -> Vec<u8> {
        let mut s = String::new();
        let mut line = |t: String| {
            s.push_str(&t);
            s.push_str("\r\n");
        };
        line("[MAIN]".into());
        line(format!("VERSION={}", self.version));
        line(format!("CAR_NAME={}", self.car_name));
        line(format!("PERF_SIZE={}", self.perfs.len()));
        for (i, p) in self.perfs.iter().enumerate() {
            line(format!("[PERF_{i}]"));
            line(format!("NAME={}", p.name));
            line(format!("TYPE={}", p.kind));
            line(format!("OBJECTIVE={}", format_g6(p.objective)));
            line(format!("CP_SPEED={}", format_g6(p.speed_at_checkpoint)));
            line(format!("RECORD={}", if p.record == f32::MAX { "0".to_string() } else { format_g6(p.record) }));
            line(format!("RECORD_COUNT_DOWN={}", if p.record_breaking == f32::MAX { "0".to_string() } else { format_g6(p.record_breaking) }));
        }
        s.into_bytes()
    }

    /// `ACDriverPerformance::updateData` 0x140307650.
    fn update_data(&mut self, dt: f32, pos: [f32; 3], speed_ms: f32, brake: f32) {
        let d = distance(self.previous_car_position, pos);
        self.current_distance = d + self.current_distance;
        self.previous_car_position = pos;
        for i in 0..self.perfs.len() {
            let v = speed_ms * f32::from_bits(0x4066_6666);
            let stop_limit = self.stop_limit;
            let p = &mut self.perfs[i];
            if !(v >= stop_limit) {
                self.current_distance = 0.0;
                p.beaten = false;
                p.seconds = 0.0;
            } else if !p.beaten {
                p.seconds = dt + p.seconds;
                match p.kind {
                    0 => {
                        if !(p.objective > self.current_distance) {
                            p.last_speed = v;
                            p.last_seconds = p.seconds;
                            p.beaten = true;
                            if p.seconds < p.record {
                                p.speed_at_checkpoint = v;
                                p.record = p.seconds;
                            }
                        }
                    }
                    1 => {
                        if !(v < p.objective) {
                            p.last_seconds = p.seconds;
                            p.count_down_active = true;
                            p.beaten = true;
                            if p.seconds < p.record {
                                p.record = p.seconds;
                            }
                        }
                    }
                    _ => {}
                }
            }
            // C: the braking half of a speed measure
            if p.kind == 1 && p.count_down_active {
                if self.brake_threshold >= brake {
                    if !p.started_to_brake {
                        continue;
                    }
                } else if !p.started_to_brake {
                    p.started_to_brake = true;
                    p.started_to_brake_position = pos;
                }
                p.distance_breaking = distance(p.started_to_brake_position, pos);
                if v > stop_limit {
                    if !(v > p.objective) {
                        p.seconds_breaking = dt + p.seconds_breaking;
                        continue;
                    }
                } else {
                    p.last_breaking_seconds = p.seconds_breaking;
                    if p.seconds_breaking < p.record_breaking {
                        p.record_breaking = p.seconds_breaking;
                    }
                    p.count_down_active = false;
                }
                if v > p.objective {
                    p.distance_breaking = 0.0;
                }
                p.seconds_breaking = 0.0;
                p.started_to_brake = false;
            }
        }
    }
}

/// `RaceFuelMonitor`'s own members.
pub struct RaceFuelMonitor {
    pub font: usize,
}

impl Gui {
    /// `ACDriverPerformance::ACDriverPerformance(CarAvatar*)` 0x140303bd0: `ini` the game's
    /// `system/cfg/driver_performances.ini`, `records` the record file's text (none: there is
    /// none).
    pub fn new_driver_performance_app(&mut self, painter: &mut dyn Painter, ini: Option<&IniReader>, records: Option<&str>) -> Id {
        let form = self.new_form(painter, "ACDriverPerformance", false);
        self.form_set_title(form, &translate("Driver performances"));
        let car = self.apps.cars.first().cloned().unwrap_or_default();
        let config = if self.apps.track_config.is_empty() { String::new() } else { format!("{}/", self.apps.track_config) };
        let file_name = format!("savedData/driverPerformances/{}/{}{}.ini", self.apps.track_name, config, car.unix_name);
        let m = &car.body_matrix.m;
        self.set_size(form, 550.0, 570.0);
        self.fonts.push(Font::new(painter, "", FontType::Monospaced, 10.0, false, false));
        let font = self.fonts.len() - 1;
        self.controls[form].font = Some(font);
        self.fonts[font].set_color(1.0, 1.0, 1.0, 1.0);
        let mut app = DriverPerformance {
            perfs: Vec::new(),
            current_distance: 0.0,
            stop_limit: 1.0,
            previous_car_position: [m[3][0], m[3][1], m[3][2]],
            brake_threshold: f32::from_bits(0x3dcc_cccd),
            time_to_wait: 5.0,
            waiting_time: 0.0,
            version: 2,
            car_name: car.unix_name.clone(),
            file_name,
            string_font_size: 15.0,
            seconds_font_size: 24.0,
            font,
        };
        if let Some(ini) = ini.filter(|i| i.ready) {
            let f = |k: &str| ini.get_float("MAIN", k).unwrap_or(0.0);
            app.string_font_size = f("STRING_FONT_SIZE");
            app.seconds_font_size = f("SECONDS_FONT_SIZE");
            app.stop_limit = f("STOP_LIMIT_KMH");
            app.time_to_wait = f("TIME_TO_WAIT_SEC");
            app.brake_threshold = f("BRAKE_THRESHOLD_PERCENTAGE") * f32::from_bits(0x3c23_d70a);
        }
        if !app.load(records, &car.unix_name) {
            for (name, objective, kind) in [("0 -> 50 -> 0 (Km/h)", 50.0, 1), ("0 -> 100 -> 0 (Km/h)", 100.0, 1), ("0 -> 120 -> 0 (Km/h)", 120.0, 1), ("0 -> 160 -> 0 (Km/h)", 160.0, 1), ("Dist. 100 m", 100.0, 0), ("Dist. 120 m", 120.0, 0), ("Dist. 160 m", 160.0, 0), ("Dist. 400 m", 400.0, 0), ("Dist. 1 Km", 1000.0, 0)] {
                app.perfs.push(Perf::new(name, objective, kind));
            }
        }
        self.set_app(form, Box::new(app));
        form
    }

    /// `RaceFuelMonitor::RaceFuelMonitor(Sim*)` 0x140338250.
    pub fn new_race_fuel_monitor_app(&mut self, painter: &mut dyn Painter) -> Id {
        let form = self.new_form(painter, "RACE_FUEL_MONITOR", false);
        self.form_set_title(form, &translate("Race Fuel Monitor"));
        self.fonts.push(Font::new(painter, "", FontType::Monospaced, 14.0, true, true));
        let font = self.fonts.len() - 1;
        self.controls[form].font = Some(font);
        self.set_size(form, 400.0, 800.0);
        self.form_data_mut(form).dev_app = true;
        self.set_app(form, Box::new(RaceFuelMonitor { font }));
        form
    }
}

impl App for DriverPerformance {
    fn as_any(&mut self) -> &mut dyn Any {
        self
    }

    /// `ACDriverPerformance::render` 0x140305d90 with `renderOnVideo` 0x140305e00.
    fn render(&mut self, gui: &mut Gui, id: Id, dt: f32, painter: &mut dyn Painter) {
        gui.control_render(id, dt, painter);
        if !(self.waiting_time >= self.time_to_wait) {
            self.waiting_time += dt;
        } else {
            let car = gui.apps.cars.first().cloned().unwrap_or_default();
            let m = car.body_matrix.m;
            self.update_data(dt, [m[3][0], m[3][1], m[3][2]], car.state.speed, car.state.brake);
        }
        let font = self.font;
        if self.time_to_wait > self.waiting_time {
            let pos = gui.local_to_world(id, [15.0, 15.0]);
            gui.fonts[font].scale = self.string_font_size * 5.0;
            gui.fonts[font].set_color(1.0, 0.0, 0.0, 1.0);
            let text = translate("HEATING UP...");
            gui.fonts[font].blit_string(painter, pos[0], pos[1], &text, 1.0, ALIGN_LEFT);
            return;
        }
        let step = cvtt(self.seconds_font_size) as f32;
        let mut y = 25.0f32;
        for p in self.perfs.clone() {
            gui.fonts[font].set_color(1.0, 1.0, 1.0, 1.0);
            let pos1 = gui.local_to_world(id, [150.0, y]);
            gui.fonts[font].scale = self.string_font_size;
            gui.fonts[font].blit_string(painter, pos1[0], pos1[1], &p.name, 1.0, ALIGN_LEFT);
            y += step;
            let pos2 = gui.local_to_world(id, [150.0, y]);
            let s = match p.kind {
                1 => {
                    if p.seconds_breaking != 0.0 {
                        gui.fonts[font].set_color(1.0, 0.0, 0.0, 1.0);
                        Some(from_float_to_string(p.seconds_breaking, 2.0))
                    } else if p.seconds != 0.0 && !p.beaten {
                        gui.fonts[font].set_color(0.0, 1.0, 0.0, 1.0);
                        Some(from_float_to_string(p.seconds, 2.0))
                    } else {
                        gui.fonts[font].set_color(1.0, 1.0, 1.0, 1.0);
                        Some(format!("{}|{}", from_float_to_string(p.last_seconds, 2.0), from_float_to_string(p.last_breaking_seconds, 2.0)))
                    }
                }
                0 => {
                    gui.fonts[font].set_color(1.0, 1.0, 1.0, 1.0);
                    Some(from_float_to_string(p.seconds, 2.0))
                }
                _ => None,
            };
            if let Some(s) = s {
                gui.fonts[font].scale = self.seconds_font_size;
                gui.fonts[font].blit_string(painter, pos2[0], pos2[1], &format!("{s}s"), 1.0, ALIGN_LEFT);
                if p.record != f32::MAX {
                    gui.fonts[font].set_color(0.0, 1.0, 0.0, 1.0);
                    let pos3 = gui.local_to_world(id, [0.0, y]);
                    let text = format!("{}/{}", from_float_to_string(p.last_seconds, 2.0), from_float_to_string(p.record, 2.0));
                    gui.fonts[font].blit_string(painter, pos3[0], if p.kind == 1 { pos2[1] } else { pos3[1] }, &text, 1.0, ALIGN_LEFT);
                    if p.kind == 0 {
                        gui.fonts[font].set_color(1.0, 1.0, 0.0, 1.0);
                        let pos4 = gui.local_to_world(id, [300.0, y]);
                        let text = format!(" Speed: {} Km/h", from_float_to_string(p.speed_at_checkpoint, 0.0));
                        gui.fonts[font].blit_string(painter, pos4[0], pos4[1], &text, 1.0, ALIGN_LEFT);
                    }
                }
                if p.kind == 1 && p.record_breaking != f32::MAX {
                    gui.fonts[font].set_color(1.0, 0.0, 0.0, 1.0);
                    let pos5 = gui.local_to_world(id, [300.0, y]);
                    let text = format!("{}/{}", from_float_to_string(p.last_breaking_seconds, 2.0), from_float_to_string(p.record_breaking, 2.0));
                    gui.fonts[font].blit_string(painter, pos5[0], pos5[1], &text, 1.0, ALIGN_LEFT);
                    let text = format!("M : {}", from_float_to_string(p.distance_breaking, 2.0));
                    gui.fonts[font].blit_string(painter, pos5[0] + 150.0, pos2[1], &text, 1.0, ALIGN_LEFT);
                }
            }
            y += cvtt(self.seconds_font_size).wrapping_mul(2) as f32;
        }
    }

    /// `~ACDriverPerformance` 0x140304c50: `saveDriverRecords` (the game's end).
    fn shutdown(&mut self, gui: &mut Gui, _id: Id) {
        gui.apps.actions.push(AppAction::WriteFile { path: self.file_name.clone(), bytes: self.save_bytes() });
    }
}

impl App for RaceFuelMonitor {
    fn as_any(&mut self) -> &mut dyn Any {
        self
    }

    /// `RaceFuelMonitor::render` 0x140338460.
    fn render(&mut self, gui: &mut Gui, id: Id, dt: f32, painter: &mut dyn Painter) {
        gui.control_render(id, dt, painter);
        let r = gui.controls[id].rect;
        let x = r.left + 10.0;
        let mut y = r.top + 80.0;
        for car in gui.apps.cars.clone() {
            let s = &car.state;
            let fuel = cvtt(s.fuel);
            let fuel_laps = cvtt(s.fuel_laps);
            let text = format!(
                "{} F:{} FL:{} FxL:{} LTC:{} S:{}",
                car.driver_name,
                fuel,
                fuel_laps,
                format_fixed((s.fuel / s.fuel_laps) as f64, 2),
                car.ai_laps_to_complete.unwrap_or(-1),
                format_fixed(s.normalized_spline_position as f64, 2)
            );
            let pit = car.requesting_pit_stop;
            gui.fonts[self.font].set_color(1.0, if pit { 0.0 } else { 1.0 }, if pit { 0.0 } else { 1.0 }, 1.0);
            gui.fonts[self.font].blit_string(painter, x, y, &text, 1.0, ALIGN_LEFT);
            y += 30.0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::from_float_to_string;

    #[test]
    fn numbers() {
        assert_eq!(from_float_to_string(12.3456, 2.0), "12.34");
        assert_eq!(from_float_to_string(0.004, 2.0), "-");
        assert_eq!(from_float_to_string(100.0, 2.0), "100");
        assert_eq!(from_float_to_string(5.5, 2.0), "5.5");
        assert_eq!(from_float_to_string(187.9, 0.0), "187");
        assert_eq!(from_float_to_string(-0.5, 2.0), "-0.5");
    }
}
