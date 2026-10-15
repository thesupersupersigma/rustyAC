// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! `FormTelemetry` (0x2b8 bytes), the developers' "Telemetry": four graphs of the player's car,
//! each showing a channel picked from about a hundred, and a presets spinner
//! (`system/cfg/telemetry_presets.ini`; the last preset is kept in `cfg/telemetry_user.ini`).
//!
//! `FormTelemetry::FormTelemetry(CarAvatar&)` 0x14032f660, `render` 0x140335b50, `~FormTelemetry`
//! 0x1403306a0, `initChannels` 0x140332ff0, `applyPreset` 0x1403327d0, `getValuesFromPhysics`
//! 0x140332ef0, `getValuesFromGraphics` 0x140332e10; the handlers' `_Do_call`s 0x1403317f0
//! (a graph's spinner), 0x140331740 (presets), 0x1403316e0 (`OnPhysicsStepCompleted`).
//!
//! The values come from the host as [`TelemetryFeed`]: each channel's raw value (before its
//! scale) in [`channels`]' order, once for the frame and once per physics step.

use std::any::Any;
use std::cell::RefCell;
use std::collections::{BTreeMap, VecDeque};
use std::rc::Rc;

use rustyac_physics::data::ini::IniReader;

use crate::apps::{App, AppAction};
use crate::control::Id;
use crate::gui::Gui;
use crate::hud::translate;
use crate::painter::Painter;
use crate::widgets::WidgetEvent;
use crate::widgets3::GraphReferenceAxis;

/// What `initChannels` reads of the player's `Car` once: `Engine::getLimiterRPM`,
/// `Car::clutchMaxTorque`, `tractionType == 3` (AWD2), the front and rear dynamic anti-roll bar
/// controllers' `isReady`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TelemetryCar {
    pub limiter_rpm: i32,
    pub clutch_max_torque: f64,
    pub awd2: bool,
    pub arb_ready: [bool; 2],
}

/// The raw channel values (index = [`channels`]' order): `graphics` read in this frame's
/// `render`, `steps` one per physics step since the last frame (`PhysicsEngine::physicsTime` in
/// ms, the values after that step).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TelemetryFeed {
    pub graphics: Vec<f32>,
    pub steps: Vec<(f64, Vec<f32>)>,
}

/// `TelemetryChannelDef` (0x48): `min_value`, `max_value`, `scale`, `isReadOnPhysicsThread`,
/// `jumpFrames`, `jumpedFrameCounter`, `auto_range`.
#[derive(Clone, Debug, PartialEq)]
pub struct ChannelDef {
    pub name: String,
    pub min: f32,
    pub max: f32,
    pub scale: f32,
    pub physics: bool,
    pub jump: i32,
    pub counter: i32,
    pub auto_range: bool,
}

/// `initChannels` 0x140332ff0: every channel in the order the game registers it.
pub fn channels(car: &TelemetryCar) -> Vec<ChannelDef> {
    let mut v = Vec::new();
    let mut add = |name: &str, min: f32, max: f32, scale: f32, physics: bool, jump: i32| {
        v.push(ChannelDef { name: name.to_string(), min, max, scale, physics, jump, counter: 0, auto_range: max == 0.0 && min == 0.0 });
    };
    const W: [&str; 4] = ["LF", "RF", "LR", "RR"];
    let deg = f32::from_bits(0x4265_2ee1);
    if car.awd2 {
        add("AWD2_LOCK", 0.0, 2000.0, 1.0, true, 0);
    }
    add("ENGINE_RPM", 0.0, car.limiter_rpm as f32 * f32::from_bits(0x3f99_999a), 1.0, true, 0);
    add("CLUTCH_TORQUE", (car.clutch_max_torque * -1.2000000476837158) as f32, (car.clutch_max_torque * 1.2000000476837158) as f32, 1.0, true, 0);
    add("DRV_ROOT_VEL", 0.0, 2000.0, 1.0, true, 0);
    add("YAW_RATE", -1.0, 1.0, 1.0, false, 10);
    let wheels = |add: &mut dyn FnMut(&str, f32, f32, f32, bool, i32), base: &str, min: f32, max: f32, scale: [f32; 4], physics: bool, jump: i32| {
        for (k, w) in W.iter().enumerate() {
            add(&format!("{base}_{w}"), min, max, scale[k], physics, jump);
        }
    };
    wheels(&mut add, "LOAD", 0.0, 1000.0, [f32::from_bits(0x3dd0_da0b); 4], true, 0);
    wheels(&mut add, "AVEL", -20.0, 300.0, [1.0; 4], true, 0);
    wheels(&mut add, "SA", -20.0, 20.0, [deg; 4], true, 0);
    wheels(&mut add, "SR", -1.0, 1.0, [1.0; 4], true, 0);
    wheels(&mut add, "MZ", -150.0, 150.0, [1.0; 4], true, 0);
    wheels(&mut add, "DY", 0.0, 2.0, [1.0; 4], true, 0);
    wheels(&mut add, "FY", -5000.0, 50000.0, [1.0; 4], true, 0);
    wheels(&mut add, "DX", 0.0, 2.0, [1.0; 4], true, 0);
    wheels(&mut add, "CAMBER_DEG", -10.0, 10.0, [deg, -deg, deg, -deg], true, 0);
    wheels(&mut add, "NDSLIP", 0.0, 1.0, [1.0; 4], true, 0);
    wheels(&mut add, "SUSTRVL", -200.0, 200.0, [1000.0; 4], true, 0);
    add("AI_PUSH", 0.0, 100.0, 1.0, true, 0);
    add("AI_LIVE_OFFSET", -3.0, 3.0, 1.0, true, 0);
    add("AI_DIST_CORNER", 0.0, 200.0, 1.0, true, 0);
    wheels(&mut add, "DAMP_VEL", -1.0, 1.0, [1.0; 4], true, 0);
    wheels(&mut add, "CORE_TEMP", 0.0, 180.0, [1.0; 4], true, 0);
    wheels(&mut add, "AVG_SURF_TEMP", 0.0, 180.0, [1.0; 4], true, 0);
    wheels(&mut add, "PRACT_TEMP", 0.0, 180.0, [1.0; 4], true, 0);
    wheels(&mut add, "TEMP_MULT", 0.0, 100.0, [100.0; 4], true, 0);
    wheels(&mut add, "BRAKE_PAD_INPUT", 0.0, 2500.0, [1.0; 4], true, 0);
    add("ERS_KINETIC_REC", 0.0, 100.0, 1000.0, true, 0);
    add("ERS_HEAT_REC", 0.0, 100.0, 1000.0, true, 0);
    add("ERS_CHARGE", 0.0, 100.0, 100.0, true, 0);
    add("ERS_TORQUE", 0.0, 200.0, 1.0, true, 0);
    add("AVG_REAR_TRAVEL", -200.0, 200.0, 1000.0, true, 0);
    add("OVERSTEER_FACTOR", -5.0, 5.0, 1.0, true, 0);
    wheels(&mut add, "STEER_DEG", -30.0, 20.0, [1.0; 4], true, 0);
    wheels(&mut add, "SKID_VOL", 0.0, 1.0, [1.0; 4], false, 0);
    wheels(&mut add, "SKID_PITCH", 0.0, 1.0, [1.0; 4], false, 0);
    wheels(&mut add, "BRAKE_DISC_T", 0.0, 1000.0, [1.0; 4], true, 10);
    add("FF_PURE", -1.0, 1.0, 1.0, false, 0);
    add("FF_FINAL", -1.0, 1.0, 1.0, false, 0);
    add("WATER_TEMP", 0.0, 150.0, 1.0, false, 0);
    add("RIDE_HEIGHT_F", 0.0, 200.0, 1000.0, false, 0);
    add("RIDE_HEIGHT_R", 0.0, 200.0, 1000.0, false, 0);
    add("ACC_LAT", -4.0, 4.0, 1.0, false, 0);
    add("ACC_LON", -5.0, 2.0, 1.0, false, 0);
    if car.arb_ready[0] {
        add("ARB_FRONT", 0.0, 0.0, 1.0, true, 0);
    }
    if car.arb_ready[1] {
        add("ARB_REAR", 0.0, 0.0, 1.0, true, 0);
    }
    v
}

/// `TelemetryGraph` (0x30): `graph`, `spinner`, `lblLive`, `lblSmooth`, `channel`,
/// `smoothValue`, `resetSmoothValue`; with its `liveValuesChannels` queue.
pub struct TelemetryGraph {
    pub graph: Id,
    pub spinner: Id,
    pub lbl_live: Id,
    pub lbl_smooth: Id,
    pub channel: Option<usize>,
    pub smooth: f32,
    pub reset_smooth: bool,
    pub queue: VecDeque<f32>,
}

/// `FormTelemetry`'s own members.
pub struct Telemetry {
    pub graphs: Vec<TelemetryGraph>,
    /// `channels` (a `std::map<std::wstring, TelemetryChannelDef>`: index into `defs`)
    pub map: BTreeMap<String, usize>,
    pub defs: Vec<ChannelDef>,
    pub sp_presets: Id,
    pub last_physics_rec_time: f64,
    /// what the spinners' handlers chose (graph, channel): the handlers run inside the form's
    /// render too, where the app is lent; the game reads the choice only from the next frame
    pub chosen: Rc<RefCell<Vec<(usize, Option<usize>)>>>,
}

impl Telemetry {
    /// `getValuesFromPhysics` (`physics`) / `getValuesFromGraphics`.
    fn sample(&mut self, raw: &[f32], physics: bool) {
        for g in &mut self.graphs {
            // (the game reads a graph without a channel at address 0x34)
            let Some(c) = g.channel else { continue };
            let d = &mut self.defs[c];
            if d.physics != physics {
                continue;
            }
            if physics {
                if d.counter < d.jump {
                    d.counter += 1;
                    continue;
                }
                d.counter = 0;
            }
            g.queue.push_back(d.scale * raw.get(c).copied().unwrap_or(0.0));
        }
    }
}

/// `applyPreset` 0x1403327d0: every preset of that name, in the file's order.
fn apply_preset(gui: &mut Gui, presets: &[(String, [String; 4])], spinners: &[Id], name: &str) {
    for p in presets.iter().filter(|p| p.0 == name) {
        for (spinner, channel) in spinners.iter().zip(p.1.iter()) {
            gui.spinner_set_value_by_item(*spinner, channel);
        }
    }
}

impl Gui {
    /// `FormTelemetry::FormTelemetry(CarAvatar&)` 0x14032f660: `presets` is the game's
    /// `system/cfg/telemetry_presets.ini`, `user` the Documents' `cfg/telemetry_user.ini`.
    pub fn new_telemetry_app(&mut self, painter: &mut dyn Painter, car: &TelemetryCar, presets: Option<&IniReader>, user: Option<&IniReader>) -> Id {
        let form = self.new_form(painter, "TELEMETRY", false);
        self.form_set_title(form, &translate("Telemetry"));
        let defs = channels(car);
        let map: BTreeMap<String, usize> = defs.iter().enumerate().map(|(i, d)| (d.name.clone(), i)).collect();
        let mut graphs = Vec::new();
        let chosen = Rc::new(RefCell::new(Vec::new()));
        for i in 0..4 {
            let g = self.new_graph(painter, "GRAPH");
            self.graph_mut(g).max_values_count = 500;
            self.graph_mut(g).axes.push(GraphReferenceAxis { color: [0.0; 3], ref_value: 0.5, is_vertical: false });
            self.control_add_control(form, g);
            let sp = self.new_spinner(painter, "SPINNER", false, false);
            self.spinner_set_label_text(sp, "");
            for name in map.keys() {
                self.spinner_add_item(sp, name);
            }
            let lbl_live = self.new_label(painter, "LIVE");
            self.control_add_control(form, lbl_live);
            let lbl_smooth = self.new_label(painter, "SMOOTH");
            self.control_add_control(form, lbl_smooth);
            let (names, chosen2) = (map.clone(), chosen.clone());
            self.spinner_mut(sp).handlers.push(Rc::new(move |gui: &mut Gui, ev: &WidgetEvent| {
                let WidgetEvent::SpinnerValueChanged { spinner, .. } = ev else { return };
                // (map operator[]: the items are all channels)
                chosen2.borrow_mut().push((i, names.get(&gui.spinner_current_item(*spinner)).copied()));
            }));
            self.control_add_control(form, sp);
            self.graph_add_serie(g, [1.0, 0.0, 0.0]);
            graphs.push(TelemetryGraph { graph: g, spinner: sp, lbl_live, lbl_smooth, channel: None, smooth: 0.0, reset_smooth: false, queue: VecDeque::new() });
        }
        let sp_presets = self.new_spinner(painter, "PRESETS", false, false);
        self.set_size(sp_presets, 400.0, 30.0);
        self.spinner_set_position(sp_presets, 2.0, 50.0);
        self.control_add_control(form, sp_presets);
        let mut list = Vec::new();
        if let Some(ini) = presets {
            let mut n = 0;
            loop {
                let sec = format!("PRESET_{n}");
                if !ini.has_section(&sec) {
                    break;
                }
                let name = ini.get_string(&sec, "NAME");
                let data = std::array::from_fn(|k| ini.get_string(&sec, &format!("DATA_{k}")));
                self.spinner_add_item(sp_presets, &name);
                list.push((name, data));
                n += 1;
            }
        }
        let spinners: Vec<Id> = graphs.iter().map(|g| g.spinner).collect();
        let (presets2, spinners2) = (list.clone(), spinners.clone());
        self.spinner_mut(sp_presets).handlers.push(Rc::new(move |gui: &mut Gui, ev: &WidgetEvent| {
            let WidgetEvent::SpinnerValueChanged { spinner, .. } = ev else { return };
            let name = gui.spinner_current_item(*spinner);
            apply_preset(gui, &presets2, &spinners2, &name);
        }));
        self.set_app(form, Box::new(Telemetry { graphs, map, defs, sp_presets, last_physics_rec_time: 0.0, chosen }));
        if let Some(first) = list.first() {
            match user {
                None => apply_preset(self, &list, &spinners, &first.0),
                Some(ini) => {
                    let name = ini.get_string("USER", "PRESET");
                    self.spinner_set_value_by_item(sp_presets, &name);
                }
            }
        }
        self.form_set_size(form, 600.0, 700.0);
        form
    }
}

impl App for Telemetry {
    fn as_any(&mut self) -> &mut dyn Any {
        self
    }

    /// The physics steps' handler (only while the form is visible, so only here), then
    /// `FormTelemetry::render` 0x140335b50.
    fn render(&mut self, gui: &mut Gui, id: Id, dt: f32, painter: &mut dyn Painter) {
        for (i, channel) in std::mem::take(&mut *self.chosen.borrow_mut()) {
            self.graphs[i].channel = channel;
            self.graphs[i].reset_smooth = true;
        }
        let feed = std::mem::take(&mut gui.apps.telemetry);
        for (pt, raw) in &feed.steps {
            if (*pt - self.last_physics_rec_time) > 12.0 {
                self.sample(raw, true);
                self.last_physics_rec_time = *pt;
            }
        }
        self.sample(&feed.graphics, false);
        gui.apps.telemetry = feed;
        let r = gui.controls[id].rect;
        let (w, hh) = (r.right - r.left, r.bottom - r.top);
        let mut y = 100.0f32;
        let h = (((hh - 100.0) / self.graphs.len() as f32) as i32) as f32;
        for g in &mut self.graphs {
            gui.spinner_set_position(g.spinner, (w - 200.0) * 0.5, y);
            gui.set_size(g.spinner, 200.0, 30.0);
            gui.set_position(g.lbl_live, 2.0, y);
            gui.set_size(g.lbl_live, 80.0, 30.0);
            gui.set_position(g.lbl_smooth, (w - 80.0) - 2.0, y);
            gui.set_size(g.lbl_smooth, 80.0, 30.0);
            gui.set_position(g.graph, 2.0, y + 35.0);
            gui.set_size(g.graph, w - 4.0, (h - 30.0) - 5.0);
            y += h;
            let Some(c) = g.channel else { continue };
            let d = &mut self.defs[c];
            gui.graph_mut(g.graph).min_y = d.min;
            gui.graph_mut(g.graph).max_y = d.max;
            let mut count = 0;
            let mut last = 0.0f32;
            while let Some(v) = g.queue.pop_front() {
                if d.auto_range {
                    d.min = if v >= d.min { d.min } else { v };
                    d.max = if v <= d.max { d.max } else { v };
                }
                gui.graph_add_value(g.graph, 0, v);
                if !g.reset_smooth {
                    g.smooth = (v - g.smooth) * f32::from_bits(0x3b44_9ba6) + g.smooth;
                } else {
                    g.smooth = v;
                    g.reset_smooth = false;
                }
                last = v;
                count += 1;
            }
            if count != 0 {
                gui.set_text(g.lbl_live, &crate::widgets2::format_fixed(last as f64, 2));
            }
            gui.set_text(g.lbl_smooth, &crate::widgets2::format_fixed(g.smooth as f64, 2));
        }
        gui.control_render(id, dt, painter);
    }

    /// `~FormTelemetry` 0x1403306a0: the presets spinner's item into `cfg/telemetry_user.ini`.
    fn shutdown(&mut self, gui: &mut Gui, _id: Id) {
        let text = format!("[USER]\r\nPRESET={}\r\n", gui.spinner_current_item(self.sp_presets));
        gui.apps.actions.push(AppAction::WriteFile { path: "cfg/telemetry_user.ini".to_string(), bytes: text.into_bytes() });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channel_count_and_limits() {
        let car = TelemetryCar { limiter_rpm: 7500, clutch_max_torque: 500.0, awd2: false, arb_ready: [false; 2] };
        let c = channels(&car);
        assert_eq!(c.len(), 104);
        assert_eq!(channels(&TelemetryCar { awd2: true, arb_ready: [true; 2], ..car }).len(), 107);
        assert_eq!(c[0].max, 9000.0);
        assert_eq!(c[1].min, -600.0000238418579f64 as f32);
    }
}
