// SPDX-License-Identifier: GPL-3.0-or-later

//! The text HUD of the debug view: speed, gear, revs, pedals, aids, timer, frame rate.

use super::font::{FontBitmap, SOLID};
use crate::input::device_name;
use crate::physics_thread::Timing;
use crate::view::CarView;

/// A corner of a HUD rectangle: pixels from the top left, texture coordinates, colour.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
#[repr(C)]
pub struct HudVertex {
    pub position: [f32; 2],
    pub uv: [f32; 2],
    pub color: [f32; 4],
}

/// What the HUD shows besides the car.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct HudInfo {
    pub fps: f32,
    pub timing: Timing,
    pub paused: bool,
    pub camera: String,
    /// A recorded drive is playing.
    pub replay: bool,
    /// Extra lines (top left, under the numbers).
    pub notes: Vec<String>,
}

type Color = [f32; 4];
const WHITE: Color = [0.95, 0.95, 0.95, 1.0];
const DIM: Color = [0.62, 0.66, 0.70, 1.0];
const PANEL: Color = [0.03, 0.04, 0.05, 0.62];
const GREEN: Color = [0.20, 0.85, 0.30, 1.0];
const RED: Color = [0.92, 0.18, 0.15, 1.0];
const BLUE: Color = [0.25, 0.55, 0.95, 1.0];
const YELLOW: Color = [0.98, 0.82, 0.12, 1.0];
const OFF: Color = [0.25, 0.27, 0.30, 1.0];

pub struct Hud<'a> {
    font: &'a FontBitmap,
    pub vertices: Vec<HudVertex>,
}

impl<'a> Hud<'a> {
    pub fn new(font: &'a FontBitmap) -> Hud<'a> {
        Hud { font, vertices: Vec::with_capacity(4096) }
    }

    fn quad(&mut self, x: f32, y: f32, w: f32, h: f32, uv: [f32; 4], color: Color) {
        let corner = |px: f32, py: f32, u: f32, v: f32| HudVertex { position: [px, py], uv: [u, v], color };
        let (a, b, c, d) = (corner(x, y, uv[0], uv[1]), corner(x + w, y, uv[2], uv[1]), corner(x + w, y + h, uv[2], uv[3]), corner(x, y + h, uv[0], uv[3]));
        self.vertices.extend([a, b, c, a, c, d]);
    }

    pub fn rect(&mut self, x: f32, y: f32, w: f32, h: f32, color: Color) {
        let uv = self.font.uv(SOLID);
        self.quad(x, y, w, h, uv, color);
    }

    /// The width of a text at a scale.
    pub fn width(&self, text: &str, scale: f32) -> f32 {
        text.chars().count() as f32 * self.font.glyph().0 * scale
    }

    /// Draws a text with its top left corner at (x, y); returns its width.
    pub fn text(&mut self, x: f32, y: f32, scale: f32, color: Color, text: &str) -> f32 {
        let (gw, gh) = self.font.glyph();
        let mut at = x;
        for c in text.chars() {
            if c != ' ' {
                let uv = self.font.uv(c as u32);
                self.quad(at.round(), y.round(), gw * scale, gh * scale, uv, color);
            }
            at += gw * scale;
        }
        at - x
    }

    pub fn text_centred(&mut self, centre: f32, y: f32, scale: f32, color: Color, text: &str) {
        let width = self.width(text, scale);
        self.text(centre - width * 0.5, y, scale, color, text);
    }

    pub fn text_right(&mut self, right: f32, y: f32, scale: f32, color: Color, text: &str) {
        let width = self.width(text, scale);
        self.text(right - width, y, scale, color, text);
    }

    /// A vertical bar filled from the bottom by `value` (0..1), with a label under it.
    #[allow(clippy::too_many_arguments)]
    fn bar(&mut self, x: f32, bottom: f32, w: f32, h: f32, value: f32, color: Color, label: &str, s: f32) {
        self.rect(x, bottom - h, w, h, [0.10, 0.11, 0.12, 0.85]);
        let filled = h * value.clamp(0.0, 1.0);
        self.rect(x, bottom - filled, w, filled, color);
        self.text_centred(x + w * 0.5, bottom + 4.0 * s, 0.6 * s, DIM, label);
    }

    /// A lamp: a labelled box that is lit or not.
    #[allow(clippy::too_many_arguments)]
    fn lamp(&mut self, x: f32, y: f32, w: f32, h: f32, lit: bool, color: Color, label: &str, s: f32) {
        self.rect(x, y, w, h, if lit { color } else { OFF });
        let text_color = if lit { [0.02, 0.02, 0.02, 1.0] } else { DIM };
        let (_, gh) = self.font.glyph();
        self.text_centred(x + w * 0.5, y + (h - gh * 0.7 * s) * 0.5, 0.7 * s, text_color, label);
    }
}

/// `m:ss.mmm`
pub fn clock(seconds: f64) -> String {
    let ms = (seconds.max(0.0) * 1000.0).round() as u64;
    format!("{}:{:02}.{:03}", ms / 60_000, ms / 1000 % 60, ms % 1000)
}

/// A lap time as `m:ss.mmm`, or dashes for none.
pub fn lap_clock(ms: u32) -> String {
    if ms == 0 {
        "-:--.---".to_string()
    } else {
        format!("{}:{:02}.{:03}", ms / 60_000, ms / 1000 % 60, ms % 1000)
    }
}

/// The gear as a driver reads it.
pub fn gear_label(gear: i32) -> String {
    match gear {
        0 => "R".to_string(),
        1 => "N".to_string(),
        n => (n - 1).to_string(),
    }
}

/// The whole HUD for a picture of `width` x `height` pixels.
/// The lines of a hybrid car: battery, what the motor gives now, the lap's allowance, and
/// the cockpit's settings as the game's own messages name them. None for a car without a
/// hybrid system and without an engine-brake control.
fn hybrid_lines(view: &CarView) -> Vec<String> {
    let h = &view.hybrid;
    let mut lines = Vec::new();
    if h.has_kers || h.has_ers {
        let mut text = format!("{} {:.0} %  deploy {:.0} %", if h.has_ers { "ERS" } else { "KERS" }, h.charge * 100.0, h.input * 100.0);
        if h.max_kj > 0.0 && h.max_kj < 1.0e6 {
            text.push_str(&format!("  lap {:.0} / {:.0} kJ", h.used_kj, h.max_kj));
        } else {
            text.push_str(&format!("  lap {:.0} kJ", h.used_kj));
        }
        if h.charging {
            text.push_str("  CHG");
        }
        lines.push(text);
    }
    if h.has_ers {
        let mut text = format!("MGU-K {}/{} {}", h.power_index + 1, h.power_count, h.power_name.as_str());
        text.push_str(&format!("  rec {} %", h.recovery * 10));
        text.push_str(if h.heat_charging { "  MGU-H battery" } else { "  MGU-H motor" });
        lines.push(text);
    }
    if h.engine_brake_count > 1 {
        lines.push(format!("engine brake {}/{}", h.engine_brake + 1, h.engine_brake_count));
    }
    lines
}

pub fn build(font: &FontBitmap, view: &CarView, info: &HudInfo, width: f32, height: f32) -> Vec<HudVertex> {
    let mut hud = Hud::new(font);
    let s = (height / 720.0).max(0.5);
    let (_, gh) = font.glyph();
    let line = gh * 0.75 * s + 3.0 * s;

    // top left: timer, rates, who drives, the hybrid system
    let hybrid = hybrid_lines(view);
    let rows = 8 + info.notes.len() + hybrid.len();
    hud.rect(10.0 * s, 10.0 * s, 430.0 * s, line * rows as f32 + 16.0 * s, PANEL);
    let x = 20.0 * s;
    let mut y = 16.0 * s;
    hud.text(x, y, 1.0 * s, WHITE, &clock(view.drive_seconds));
    hud.text_right(430.0 * s, y + 4.0 * s, 0.75 * s, DIM, &format!("{:.0} FPS", info.fps));
    y += gh * s + 6.0 * s;
    let t = &info.timing;
    for text in [
        format!("physics {:.1} Hz  step {:.3} ms", t.rate_hz(), t.step_avg_us() / 1000.0),
        format!("late max {:.2} ms  over 1 ms: {}", t.late_max_us / 1000.0, t.late_over_1ms),
        format!("driver: {}", device_name(view.device)),
        format!("camera: {}  (F1, F6)", info.camera),
        format!("air {:.0} C  road {:.0} C  grip {:.1} %", view.air, view.road, view.grip * 100.0),
        if view.wind_kmh > 0.0 {
            format!("wind {:.1} km/h from {:.0} deg  air {:.3}", view.wind_kmh, view.wind_deg, view.air_density)
        } else {
            format!("no wind  air {:.3} kg/m3", view.air_density)
        },
        format!("tyres {}  setup {}", view.compound.as_str(), if view.setup.as_str().is_empty() { "default" } else { view.setup.as_str() }),
    ] {
        hud.text(x, y, 0.75 * s, DIM, &text);
        y += line;
    }
    for text in &hybrid {
        hud.text(x, y, 0.75 * s, WHITE, text);
        y += line;
    }
    for note in &info.notes {
        hud.text(x, y, 0.75 * s, YELLOW, note);
        y += line;
    }

    // under it: damage (the five zones in km/h of impact, the bend of each corner's
    // suspension, the engine), shown once there is any or while the body touches something
    let hurt = view.damage.iter().any(|d| *d > 0.0) || view.suspension_damage.iter().any(|d| *d > 0.0) || view.engine_life < 1000.0;
    if hurt || view.contacts > 0 {
        let top = 10.0 * s + line * rows as f32 + 16.0 * s + 8.0 * s;
        hud.rect(10.0 * s, top, 430.0 * s, line * 3.0 + 14.0 * s, PANEL);
        let mut y = top + 7.0 * s;
        let level = |d: f32| if d > 60.0 { RED } else if d > 0.0 { YELLOW } else { DIM };
        let mut tx = x + hud.text(x, y, 0.75 * s, if hurt { WHITE } else { DIM }, "DAMAGE") + 12.0 * s;
        for (label, d) in ["front", "rear", "left", "right", "most"].iter().zip(view.damage) {
            tx += hud.text(tx, y, 0.75 * s, level(d), &format!("{label} {d:.0}")) + 10.0 * s;
        }
        y += line;
        let mut tx = x + hud.text(x, y, 0.75 * s, DIM, "suspension") + 12.0 * s;
        for (label, d) in ["LF", "RF", "LR", "RR"].iter().zip(view.suspension_damage) {
            tx += hud.text(tx, y, 0.75 * s, if d > 0.5 { RED } else if d > 0.0 { YELLOW } else { DIM }, &format!("{label} {:.0}%", d * 100.0)) + 10.0 * s;
        }
        y += line;
        let life = view.engine_life;
        let engine = if life <= 0.0 { "engine BLOWN".to_string() } else { format!("engine {:.0}%", life * 0.1) };
        let w = hud.text(x, y, 0.75 * s, if life <= 0.0 { RED } else if life < 1000.0 { YELLOW } else { DIM }, &engine);
        if view.contacts > 0 {
            hud.text(x + w + 16.0 * s, y, 0.75 * s, YELLOW, &format!("touching: {} contact points", view.contacts));
        }
    }

    // bottom left: revs, gear, speed (the middle of the picture is the car's)
    // (wide enough for the lamps at their longest: two aids that are not fitted)
    let lamps: f32 = [view.tc_text(), view.abs_text(), "DRS".to_string(), "AUTO".to_string(), "LIGHTS".to_string()].iter().map(|label| hud.width(label, 0.7 * s) + 18.0 * s).sum();
    let panel_w = (470.0 * s).max(lamps + 26.0 * s);
    let panel_h = 150.0 * s;
    let px = 10.0 * s;
    let py = height - panel_h - 40.0 * s;
    hud.rect(px, py, panel_w, panel_h, PANEL);
    // the rev bar: green, yellow in the last 15 %, red in the last 5 %
    let limit = view.rpm_limit.max(1000.0);
    let revs = (view.rpm / limit).clamp(0.0, 1.0);
    let (bx, by, bw, bh) = (px + 16.0 * s, py + 12.0 * s, panel_w - 32.0 * s, 20.0 * s);
    hud.rect(bx, by, bw, bh, [0.10, 0.11, 0.12, 0.9]);
    let rev_color = if revs > 0.95 {
        RED
    } else if revs > 0.85 {
        YELLOW
    } else {
        GREEN
    };
    hud.rect(bx, by, bw * revs, bh, rev_color);
    hud.text(bx, by + bh + 4.0 * s, 0.7 * s, DIM, &format!("{:.0} rpm", view.rpm));
    // the brake bias, always: AC's own number (getFrontBias x 100, one decimal)
    hud.text_centred(px + panel_w * 0.5, by + bh + 4.0 * s, 0.7 * s, WHITE, &view.bias_text());
    hud.text_right(bx + bw, by + bh + 4.0 * s, 0.7 * s, DIM, &format!("{limit:.0}"));
    // gear and speed
    let centre = px + panel_w * 0.5;
    hud.text_centred(centre - 120.0 * s, py + 52.0 * s, 2.6 * s, WHITE, &gear_label(view.gear));
    hud.text_right(centre + 130.0 * s, py + 56.0 * s, 2.2 * s, WHITE, &format!("{:.0}", view.speed_kmh.abs()));
    hud.text(centre + 136.0 * s, py + 86.0 * s, 0.8 * s, DIM, "km/h");
    // lamps
    let lamp_y = py + panel_h - 34.0 * s;
    let mut lx = px + 16.0 * s;
    // the aids' lamps say their level all the time (`TC 2/3`, `ABS off`, `TC not fitted`) and
    // are lit while the aid acts
    for (lit, color, label) in [
        (view.tc_in_action, YELLOW, view.tc_text()),
        (view.abs_in_action, YELLOW, view.abs_text()),
        (view.drs, GREEN, "DRS".to_string()),
        (view.auto_shifter, BLUE, "AUTO".to_string()),
        (view.lights, WHITE, "LIGHTS".to_string()),
    ] {
        let w = hud.width(&label, 0.7 * s) + 12.0 * s;
        hud.lamp(lx, lamp_y, w, 24.0 * s, lit, color, &label, s);
        lx += w + 6.0 * s;
    }

    // bottom right: pedals and steering
    let (bar_w, bar_h) = (26.0 * s, 110.0 * s);
    let bottom = height - 44.0 * s;
    let right = width - 30.0 * s;
    hud.rect(right - 4.0 * bar_w - 3.0 * 12.0 * s - 14.0 * s, bottom - bar_h - 44.0 * s, 4.0 * bar_w + 3.0 * 12.0 * s + 28.0 * s, bar_h + 78.0 * s, PANEL);
    let column = |k: f32| right - (k + 1.0) * bar_w - k * 12.0 * s;
    hud.bar(column(0.0), bottom, bar_w, bar_h, view.gas, GREEN, "GAS", s);
    hud.bar(column(1.0), bottom, bar_w, bar_h, view.brake, RED, "BRK", s);
    // the clutch pedal's travel: 1 = pedal up
    hud.bar(column(2.0), bottom, bar_w, bar_h, 1.0 - view.clutch, BLUE, "CLT", s);
    hud.bar(column(3.0), bottom, bar_w, bar_h, view.hand_brake, YELLOW, "HB", s);
    // steering: a marker on a line, right = right
    let (sx, sw, sy) = (column(3.0), column(0.0) + bar_w - column(3.0), bottom - bar_h - 28.0 * s);
    hud.rect(sx, sy + 6.0 * s, sw, 3.0 * s, OFF);
    hud.rect(sx + sw * 0.5 - 1.0, sy, 2.0, 15.0 * s, DIM);
    let marker = sx + sw * (0.5 + 0.5 * view.steer.clamp(-1.0, 1.0));
    hud.rect(marker - 4.0 * s, sy, 8.0 * s, 15.0 * s, WHITE);

    // top right: the tyres' loads, coloured by slip (red = past the peak)
    let (tw, th) = (74.0 * s, 40.0 * s);
    let tx = width - 2.0 * tw - 36.0 * s;
    let ty = 16.0 * s;
    hud.rect(tx - 10.0 * s, ty - 6.0 * s, 2.0 * tw + 30.0 * s, 2.0 * th + 22.0 * s, PANEL);
    for wheel in 0..4 {
        // wheel 0 is the front left, which is on the left of a car seen from behind
        let (col, row) = ((wheel % 2) as f32, (wheel / 2) as f32);
        let slip = view.wheel_slip[wheel].clamp(0.0, 2.0);
        let color = if slip > 1.0 { [0.9, 0.25 + 0.2 * (2.0 - slip), 0.15, 1.0] } else { [0.15 + 0.3 * slip, 0.55, 0.25, 1.0] };
        let (x, y) = (tx + col * (tw + 10.0 * s), ty + row * (th + 10.0 * s));
        hud.rect(x, y, tw, th, color);
        let surface = view.surfaces[wheel].as_str();
        if surface.is_empty() {
            hud.text_centred(x + tw * 0.5, y + 8.0 * s, 0.7 * s, [0.02, 0.02, 0.02, 1.0], &format!("{:.0} N", view.wheel_load[wheel]));
        } else {
            // on a track: the load, and under it the surface the tyre stands on
            hud.text_centred(x + tw * 0.5, y + 2.0 * s, 0.62 * s, [0.02, 0.02, 0.02, 1.0], &format!("{:.0} N", view.wheel_load[wheel]));
            let short: String = surface.chars().take(14).collect();
            hud.text_centred(x + tw * 0.5, y + 21.0 * s, 0.4 * s, [0.02, 0.02, 0.02, 1.0], &short);
        }
    }

    // top centre, on a track: the lap
    let lap = &view.lap;
    if lap.on_track {
        let (pw, ph) = (470.0 * s, 96.0 * s);
        // beside the top left panel where the picture is too narrow for the middle
        let px = ((width - pw) * 0.5).max(450.0 * s);
        let py = 10.0 * s;
        hud.rect(px, py, pw, ph, PANEL);
        let lap_color = if lap.valid { WHITE } else { RED };
        hud.text(px + 14.0 * s, py + 8.0 * s, 1.15 * s, lap_color, &lap_clock(lap.current_ms));
        let state = if lap.valid { "VALID".to_string() } else { format!("INVALID ({} cut{})", lap.cuts, if lap.cuts == 1 { "" } else { "s" }) };
        hud.text(px + 190.0 * s, py + 14.0 * s, 0.7 * s, if lap.valid { GREEN } else { RED }, &state);
        hud.text_right(px + pw - 14.0 * s, py + 14.0 * s, 0.7 * s, DIM, &format!("lap {}   {:.1} %", lap.laps + 1, lap.position.clamp(0.0, 1.0) * 100.0));
        let row = py + 44.0 * s;
        hud.text(px + 14.0 * s, row, 0.7 * s, DIM, &format!("last {}{}", lap_clock(lap.last_ms), if lap.last_ms != 0 && !lap.last_valid { " cut" } else { "" }));
        hud.text(px + 250.0 * s, row, 0.7 * s, DIM, &format!("best {}", lap_clock(lap.best_ms)));
        // the sectors: done ones with their time, the running one marked
        let row = py + 68.0 * s;
        let mut sx = px + 14.0 * s;
        for k in 0..lap.sector_count.min(4) {
            let time = lap.sector_ms[k as usize];
            let (text, color) = if k < lap.sector {
                (format!("S{} {:.3}", k + 1, time as f64 / 1000.0), WHITE)
            } else if k == lap.sector {
                (format!("S{} ...", k + 1), YELLOW)
            } else {
                (format!("S{}", k + 1), DIM)
            };
            sx += hud.text(sx, row, 0.7 * s, color, &text) + 18.0 * s;
        }
        let mut notes = Vec::new();
        if lap.tyres_out > 0 {
            notes.push(format!("{} off", lap.tyres_out));
        }
        if lap.in_pit_lane {
            notes.push("PIT LANE".to_string());
        }
        hud.text_right(px + pw - 14.0 * s, row, 0.7 * s, if lap.tyres_out > 2 { RED } else { DIM }, &notes.join("  "));
    }

    if info.replay {
        // under the lap panel where there is one
        let y = if view.lap.on_track { 114.0 * s } else { 14.0 * s };
        hud.text_centred(width * 0.5, y, 1.0 * s, YELLOW, "REPLAY");
    }
    // a change of an aid or of the brake bias: a note for a moment, in the upper middle
    if let Some(note) = view.aid_note() {
        let w = hud.width(note, 1.2 * s) + 32.0 * s;
        let y = height * 0.24;
        hud.rect((width - w) * 0.5, y, w, gh * 1.2 * s + 14.0 * s, PANEL);
        hud.text_centred(width * 0.5, y + 7.0 * s, 1.2 * s, YELLOW, note);
    }
    if info.paused {
        let w = hud.width("PAUSED", 2.5 * s) + 40.0 * s;
        hud.rect((width - w) * 0.5, height * 0.36, w, gh * 2.5 * s + 20.0 * s, PANEL);
        hud.text_centred(width * 0.5, height * 0.36 + 10.0 * s, 2.5 * s, WHITE, "PAUSED");
    }
    let keys = if lap.on_track {
        "F1 view   F6 car cameras   R to the start   Shift+R back on track   N new car   T / Y TC / ABS (Shift: down)   ] [ brake bias   P pause   Esc quit"
    } else {
        "F1 view   F6 car cameras   R reset   N new car   T / Y TC / ABS (Shift: down)   ] [ brake bias   P pause   Esc quit"
    };
    hud.text(14.0 * s, height - 28.0 * s, 0.7 * s, DIM, keys);
    hud.vertices
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels() {
        assert_eq!(clock(0.0), "0:00.000");
        assert_eq!(clock(83.4567), "1:23.457");
        assert_eq!((gear_label(0), gear_label(1), gear_label(2), gear_label(8)), ("R".to_string(), "N".to_string(), "1".to_string(), "7".to_string()));
    }
}
