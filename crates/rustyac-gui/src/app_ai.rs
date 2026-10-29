// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! `FormAI` (0x2b0), the developers' "AI": the AI line recorder's buttons, the focused car's AI
//! driver's values (a graph, labels, spinners that change them, "SAVE ai" into the car's
//! `ai.ini`) and red debug text in the screen's corner.
//!
//! `FormAI::FormAI(Sim*)` 0x1402dcf80, `render` 0x1402e1480, `onBeginPitlaneClicked`
//! 0x1402e12d0, the start-recording lambda 0x1402def10, the magic lambda 0x1402df740, the save
//! cons lambda 0x1402ded20, the SAVE ai lambda 0x1402df8b0.

use std::any::Any;
use std::cell::RefCell;
use std::rc::Rc;

use crate::acos::format_g6;
use crate::apps::{App, AppAction};
use crate::control::{Clicked, Id};
use crate::gui::Gui;
use crate::hud::translate;
use crate::painter::{Painter, ALIGN_LEFT};
use crate::widgets::WidgetEvent;
use crate::widgets2::{format_fixed, Connected};

/// `CarAvatar::aiState` (`Car::getAIState` @ 0x140270240), only while the car's controls are
/// an `AIDriver`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct AiState {
    pub steer_target: [f32; 3],
    pub gas_brake_target: f32,
    pub target_speed: f32,
    pub target_lateral_offset: f32,
    pub brake_target_speed: f32,
    pub brake_target_dist: f32,
    pub current_normalized_spline_position: f32,
    pub outside_offset: f32,
    pub proj_dn_rpm: f32,
    pub understeer_factor: f32,
    pub current_push: f32,
    /// `getAISpline()->spline.splineToWorld(currentNormalizedSplinePosition)`
    pub spline_at_npos: [f32; 3],
}

/// The `AIDriver` values the app reads.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct AiDriverValues {
    pub steer_min_lookahead: f32,
    pub gas_brake_lookahead: f32,
    pub steer_gain: f32,
    pub brake_hint_base: f32,
    pub change_up_rpm: f32,
    pub change_dn_perc: f32,
    pub aero_hint: f32,
    pub tyres_hint: f32,
    pub ultra_grip: f32,
    pub next_car_modifier: f32,
    pub dist_to_next_corner: f32,
    /// `|steerTarget - the car's body position|`
    pub distance_from_target: f32,
}

/// What the app reads of the session (filled on the physics side).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AiAppView {
    /// the focused car's `AIDriver` (none: its controls are not one)
    pub driver: Option<AiDriverValues>,
    pub state: Option<AiState>,
    /// the track's line: `pointsCount`, `interpolationMode`, `isGridOn`
    pub points_count: u32,
    pub interpolation_mode: usize,
    pub grid_on: bool,
    /// `splineToWorld(0)`: the target steer distance before any AI state
    pub spline_at_zero: [f32; 3],
    /// `payloadAtPosition(worldToSpline(the focused car))`'s sides (left, right)
    pub sides: [f32; 2],
    /// the car's fuel lap evaluator: `totalM * 0.001 / totalLiters`
    pub km_per_liter: f32,
    /// `content/cars/<car>/data/car.ini` is a loose file (save cons writes only then)
    pub loose: bool,
}

/// `interpolationModeName` (0x1400126e0)
pub const INTERPOLATION_MODE_NAMES: [&str; 6] = ["eLinear", "eBezier", "eCatmulRom", "eBSpline", "eNThBezier", "eCubicSplines"];

/// The app's recorder and session jobs (`gui.apps.actions`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AiJob {
    StartRecording,
    PitLane { begin: bool },
    /// `AIDriver` value `which` (as `SetupJob::SetAi`) of the focused car
    Set { which: usize, value: f32 },
}

/// The buttons' tags (an app is out of its form while a click runs).
#[derive(Default)]
struct Tags {
    recording: bool,
    pits: bool,
    /// `FormAI::aiDriver` is set (it stays once it is)
    driver: bool,
    magic: bool,
}

pub struct AiApp {
    graph: Id,
    but_start_recording: Id,
    lat_off: Option<Id>,
    br_target: Option<Id>,
    nsp: Id,
    radius: Id,
    spinners: [Id; 8],
    tags: Rc<RefCell<Tags>>,
    state: AiState,
    max_outside_offset: f32,
}

/// The spinners in the order of [`AiJob::Set`]'s `which` 3.. (and `aiPush` = 1, `Steer LA` = 2,
/// `Steer gain` = 0): (name, x, y, min, max, step, value, which, scale).
const SPINNERS: [(&str, f32, f32, i32, i32, i32, i32, usize, f32); 8] = [
    ("Steer LA", 5.0, 440.0, 0, 500, 1, 0, 2, 0.1),
    ("Steer gain", 5.0, 500.0, 0, 400, 1, 0, 0, 0.01),
    ("rpm gear up", 5.0, 560.0, 0, 50000, 50, 0, 3, 1.0),
    ("gear down %", 200.0, 560.0, 50, 100, 1, 0, 4, 0.01),
    ("brake hint", 5.0, 620.0, 0, 130, 1, 90, 5, 0.01),
    ("tyre hint", 200.0, 620.0, 0, 150, 1, 120, 6, 0.01),
    ("aero hint", 200.0, 500.0, 0, 150, 1, 100, 7, 0.01),
    ("aiPush", 200.0, 380.0, 1, 120, 1, 100, 1, 0.01),
];

impl Gui {
    /// `FormAI::FormAI` 0x1402dcf80: 400 x 740. `LAT_OFF` and `BR_TARGET` only when the session
    /// has a car (always here).
    pub fn new_ai_app(&mut self, painter: &mut dyn Painter) -> Id {
        let form = self.new_form(painter, "FORM_AI", false);
        self.form_set_title(form, "AI");
        let tags = Rc::new(RefCell::new(Tags { driver: self.apps.ai.driver.is_some(), magic: true, ..Tags::default() }));
        let graph = self.new_graph(painter, "SPEED_GRAPH");
        self.set_size(graph, 290.0, 100.0);
        self.set_position(graph, 5.0, 30.0);
        self.control_add_control(form, graph);
        for color in [[0.0, 1.0, 0.0], [1.0, 0.0, 0.0], [1.0, 0.0, 1.0], [1.0, 1.0, 0.0], [1.0, 1.0, 1.0]] {
            self.graph_add_serie(graph, color);
        }
        let g = self.graph_mut(graph);
        g.min_y = 0.0;
        g.max_y = 60.0;
        g.max_values_count = 300;
        // the start-recording lambda 0x1402def10: once
        let rec = self.new_control(painter, "BEGIN_FAST");
        self.set_size(rec, 200.0, 25.0);
        self.set_position(rec, 50.0, 140.0);
        self.set_text(rec, &translate("start recording"));
        self.control_add_control(form, rec);
        let t = Rc::clone(&tags);
        self.controls[rec].ev_clicked.push(Rc::new(move |gui: &mut Gui, _: &Clicked| {
            if !t.borrow().recording {
                gui.set_text(rec, &translate("recording"));
                t.borrow_mut().recording = true;
                gui.apps.actions.push(AppAction::Ai(AiJob::StartRecording));
            }
        }));
        // onBeginPitlaneClicked 0x1402e12d0
        let pits = self.new_control(painter, "BEGIN_PITS");
        self.set_size(pits, 200.0, 25.0);
        self.set_position(pits, 50.0, 170.0);
        let t = Rc::clone(&tags);
        self.controls[pits].ev_clicked.push(Rc::new(move |gui: &mut Gui, _: &Clicked| {
            let begin = !t.borrow().pits;
            gui.set_text(pits, &translate(if begin { "recording" } else { "start recording" }));
            t.borrow_mut().pits = begin;
            gui.apps.actions.push(AppAction::Ai(AiJob::PitLane { begin }));
        }));
        self.set_text(pits, &translate("start pits"));
        self.control_add_control(form, pits);
        // the save cons lambda 0x1402ded20 (on the physics thread): only a loose car.ini's car
        let cons = self.new_control(painter, "SAVE_CONS");
        self.set_size(cons, 200.0, 25.0);
        self.set_position(cons, 50.0, 200.0);
        self.controls[cons].ev_clicked.push(Rc::new(move |gui: &mut Gui, _: &Clicked| {
            let (Some(car), true) = (gui.apps.cars.get(gui.apps.focused_car), gui.apps.ai.loose) else { return };
            let file = format!("content/cars/{}/data/fuel_cons.ini", car.unix_name);
            // std::to_wstring(float): "%f"
            let value = format!("{:.6}", gui.apps.ai.km_per_liter);
            gui.apps.actions.push(profile(&file, "FUEL_EVAL", "KM_PER_LITER", value));
            gui.apps.actions.push(AppAction::AddMessage { title: "Fuel cons saved to fuel_cons.ini".into(), text: String::new() });
        }));
        self.set_text(cons, &translate("save cons"));
        self.control_add_control(form, cons);
        self.form_set_size(form, 400.0, 740.0);
        let lat_off = self.new_connected_label(painter, "LAT_OFF", Connected::Float(0.0));
        self.set_size(lat_off, 200.0, 25.0);
        self.set_position(lat_off, 50.0, 240.0);
        self.control_add_control(form, lat_off);
        let br_target = self.new_label(painter, "BR_TARGET");
        self.set_size(br_target, 200.0, 25.0);
        self.set_position(br_target, 50.0, 265.0);
        self.control_add_control(form, br_target);
        let nsp = self.new_label(painter, "LABEL_NSP");
        self.set_size(nsp, 200.0, 25.0);
        self.set_position(nsp, 300.0, 295.0);
        self.control_add_control(form, nsp);
        // shortcut: the 3D lines of the render-finished handler (the line, the pit lane, the sides) are not drawn, port FormAI::renderSplines if wanted
        // the magic lambda 0x1402df740: AIDriver::useMagicForces
        let magic = self.new_control(painter, "magic");
        self.set_size(magic, 100.0, 25.0);
        self.set_position(magic, 5.0, 330.0);
        self.set_text(magic, "Magic: ON");
        self.control_add_control(form, magic);
        let t = Rc::clone(&tags);
        // (by the button's text, on the car's controls of now)
        self.controls[magic].ev_clicked.push(Rc::new(move |gui: &mut Gui, _: &Clicked| {
            if gui.apps.ai.driver.is_none() {
                return;
            }
            let on = !t.borrow().magic;
            t.borrow_mut().magic = on;
            gui.set_text(magic, if on { "Magic: ON" } else { "Magic: OFF" });
            gui.apps.actions.push(AppAction::Ai(AiJob::Set { which: 8, value: on as u8 as f32 }));
        }));
        let mut spinners = [0; 8];
        for (k, &(name, x, y, min, max, step, value, which, scale)) in SPINNERS.iter().enumerate() {
            let sp = self.new_spinner(painter, &translate(name), false, false);
            let (w, interval) = if name == "aiPush" { (100.0, None) } else { (175.0, Some(0.016)) };
            self.spinner_set_size(sp, w, 25.0);
            self.spinner_set_position(sp, x, y);
            if let Some(v) = interval {
                self.spinner_set_repeat_interval(sp, v);
            }
            // (before the handler is hooked: nobody hears of it). The first four get their
            // value before their limits: "gear down %" starts at 0 under its minimum 50.
            if k < 4 {
                self.spinner_set_value(sp, value);
            }
            let s = self.spinner_mut(sp);
            s.min_value = min;
            s.max_value = max;
            s.step = step;
            if k >= 4 {
                self.spinner_set_value(sp, value);
            }
            self.control_add_control(form, sp);
            let t = Rc::clone(&tags);
            self.spinner_mut(sp).handlers.push(Rc::new(move |gui: &mut Gui, ev: &WidgetEvent| {
                let WidgetEvent::SpinnerValueChanged { value, .. } = ev else { return };
                if t.borrow().driver {
                    let v = *value as f32 * scale;
                    gui.apps.actions.push(AppAction::Ai(AiJob::Set { which, value: v }));
                    // (the game writes the driver at once: the text of this frame shows it)
                    if let Some(d) = gui.apps.ai.driver.as_mut() {
                        match which {
                            0 => d.steer_gain = v,
                            2 => d.steer_min_lookahead = v,
                            3 => d.change_up_rpm = v,
                            4 => d.change_dn_perc = v,
                            5 => d.brake_hint_base = v,
                            6 => d.tyres_hint = v,
                            7 => d.aero_hint = v,
                            _ => {}
                        }
                    }
                }
            }));
            spinners[k] = sp;
            if name == "aero hint" {
                let gear_down = spinners[3];
                // the SAVE ai lambda 0x1402df8b0
                let save = self.new_control(painter, "SAVE AI");
                self.set_size(save, 175.0, 25.0);
                self.set_position(save, 200.0, 680.0);
                self.set_text(save, &translate("SAVE ai"));
                self.control_add_control(form, save);
                let t = Rc::clone(&tags);
                self.controls[save].ev_clicked.push(Rc::new(move |gui: &mut Gui, _: &Clicked| {
                    if t.borrow().driver {
                        if let (Some(d), Some(car)) = (gui.apps.ai.driver, gui.apps.cars.get(gui.apps.focused_car)) {
                            let gear_down = gui.spinner(gear_down).value;
                            for (section, key, value) in save_ai(&d, gear_down) {
                                gui.apps.actions.push(profile(&format!("content/cars/{}/data/ai.ini", car.unix_name), section, key, value));
                            }
                        }
                    }
                    gui.apps.actions.push(AppAction::AddMessage { title: translate("ai.ini saved"), text: String::new() });
                }));
            }
        }
        let radius = self.new_control(painter, "RADIUS");
        self.set_size(radius, 100.0, 25.0);
        self.set_position(radius, 5.0, 360.0);
        self.control_add_control(form, radius);
        self.set_app(form, Box::new(AiApp { graph, but_start_recording: rec, lat_off: Some(lat_off), br_target: Some(br_target), nsp, radius, spinners, tags, state: AiState::default(), max_outside_offset: 0.0 }));
        form
    }
}

/// `WritePrivateProfileStringW` into a game file (the GUI's runner puts `content/` into its own folder).
fn profile(file: &str, section: &str, key: &str, value: String) -> AppAction {
    AppAction::WriteProfileString { file: file.into(), section: section.into(), key: key.into(), value }
}

/// The SAVE ai lambda's `WritePrivateProfileString`s (floats as a default `std::wostream`:
/// `%g` of six digits). shortcut: drivetrain.ini's AUTO_SHIFTER copy for a loose unencrypted
/// car is not done, port it if a loose car's AI should save it.
pub fn save_ai(d: &AiDriverValues, gear_down: i32) -> Vec<(&'static str, &'static str, String)> {
    let g = |v: f32| format_g6(v);
    vec![
        ("HEADER", "VERSION", "4".into()),
        ("GEARS", "UP", g(d.change_up_rpm)),
        ("GEARS", "DOWN", gear_down.to_string()),
        ("LOOKAHEAD", "BASE", g(d.steer_min_lookahead)),
        ("LOOKAHEAD", "GAS_BRAKE_LOOKAHEAD", g(d.gas_brake_lookahead)),
        ("STEER", "STEER_GAIN", g(d.steer_gain)),
        ("PEDALS", "BRAKE_HINT", g(d.brake_hint_base)),
        ("PEDALS", "TRAIL_HINT", "1".into()),
        ("ULTRA_GRIP", "VALUE", g(d.ultra_grip)),
        ("PHYSICS_HINTS", "AERO_HINT", g(d.aero_hint)),
        ("TYRES", "HINT", g(d.tyres_hint)),
    ]
}

fn distance(a: [f32; 3], b: [f32; 3]) -> f32 {
    let d = [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
    let q = d[0] * d[0] + d[1] * d[1] + d[2] * d[2];
    if q == 0.0 {
        0.0
    } else {
        q.sqrt()
    }
}

impl App for AiApp {
    fn as_any(&mut self) -> &mut dyn Any {
        self
    }

    /// vtable +0x40: an empty function (a form taller than the screen stays where it is).
    fn on_visible_changed(&mut self, _gui: &mut Gui, _id: Id, _visible: bool) {}

    /// `FormAI::render` 0x1402e1480.
    fn render(&mut self, gui: &mut Gui, id: Id, dt: f32, painter: &mut dyn Painter) {
        let view = gui.apps.ai.clone();
        if !self.tags.borrow().driver {
            if let Some(d) = view.driver {
                self.tags.borrow_mut().driver = true;
                // (setValue tells the handlers: the driver gets the values back as the spinners hold them)
                let values = [d.steer_min_lookahead * 10.0, d.steer_gain * 100.0, d.change_up_rpm, (d.change_dn_perc * 100.0 + 0.5) as i32 as f32, d.brake_hint_base * 100.0, d.tyres_hint * 100.0, d.aero_hint * 100.0];
                for (sp, v) in self.spinners.iter().zip(values) {
                    gui.spinner_set_value(*sp, v as i32);
                }
            }
        }
        let view = gui.apps.ai.clone();
        if let Some(s) = view.state {
            self.state = s;
        }
        let car = gui.apps.cars.get(gui.apps.focused_car).cloned();
        let s = self.state;
        if let (Some(car), Some(_)) = (&car, view.state) {
            gui.graph_add_value(self.graph, 1, car.state.speed);
            gui.graph_add_value(self.graph, 0, car.state.gas * 50.0);
            gui.graph_add_value(self.graph, 2, car.state.brake * 50.0);
            gui.graph_add_value(self.graph, 3, s.target_speed);
            gui.graph_add_value(self.graph, 4, s.understeer_factor * 50.0);
            gui.set_text(self.radius, &format!("CP:{}", (s.current_push * 100.0) as i32));
            if s.brake_target_speed > 0.0 {
                let (i, sp) = ((s.brake_target_speed * 3.6) as i32, (car.state.speed * 3.6) as i32);
                if let Some(l) = self.br_target {
                    gui.set_text(l, &format!("{i} {sp} {} D:{}", sp - i, format_g6(s.brake_target_dist)));
                }
            }
            gui.set_text(self.nsp, &format_fixed(car.state.normalized_spline_position as f64, 3));
        }
        if let Some(l) = self.lat_off {
            gui.connected_label_mut(l).value = Connected::Float(s.target_lateral_offset);
        }
        gui.control_render(id, dt, painter);
        let font = gui.font;
        let f2 = |v: f32| format_fixed(v as f64, 2);
        let mut line = |gui: &mut Gui, y: f32, text: &str, scale: f32| gui.fonts[font].blit_string(painter, 20.0, y, text, scale, ALIGN_LEFT);
        let npos = car.as_ref().map_or(0.0, |c| c.state.normalized_spline_position);
        let red = |gui: &mut Gui| gui.fonts[font].set_color(1.0, 0.0, 0.0, 1.0);
        red(gui);
        line(gui, 20.0, &format!("{} : {}", translate("Normalized Pos"), f2(npos)), 1.0);
        red(gui);
        let at = if view.state.is_some() || self.state != AiState::default() { s.spline_at_npos } else { view.spline_at_zero };
        line(gui, 50.0, &format!("{} : {}", translate("Target Steer Distance"), f2(distance(at, s.steer_target))), 1.0);
        red(gui);
        line(gui, 80.0, &format!("{} : {}", translate("Points count"), view.points_count), 1.0);
        red(gui);
        line(gui, 110.0, &format!("{} : {}", translate("Interpolation Mode"), INTERPOLATION_MODE_NAMES.get(view.interpolation_mode).copied().unwrap_or("")), 1.0);
        let off = s.outside_offset.abs();
        let (r, g) = if off >= 2.0 { (1.0, 0.0) } else { (0.0, 1.0) };
        gui.fonts[font].set_color(r, g, 0.0, 1.0);
        if gui.apps.ctrl_down {
            self.max_outside_offset = 0.0;
        }
        self.max_outside_offset = off.max(self.max_outside_offset);
        line(gui, 140.0, &format!("{} : {}({})", translate("Outside offset"), f2(s.outside_offset), f2(self.max_outside_offset)), 2.0);
        red(gui);
        line(gui, 170.0, &format!("{} : {}\n", translate("Gas Brake Target"), f2(s.gas_brake_target)), 1.0);
        red(gui);
        line(gui, 200.0, &format!("{} : {}\n", translate("IS grid on"), view.grid_on as u8), 1.0);
        if !self.tags.borrow().driver {
            // shortcut: the recorder's side splines are empty here (the game fills them only when side_l/r.csv are newer than the line), so the indices are 0
            red(gui);
            line(gui, 230.0, &format!("{} : 0\n", translate("RIGHT INDEX")), 1.0);
            red(gui);
            line(gui, 260.0, &format!("{} : {}\n", translate("RIGHT DISTANCE"), f2(view.sides[1])), 1.0);
            red(gui);
            line(gui, 290.0, &format!("{} : 0\n", translate("LEFT  INDEX")), 1.0);
            red(gui);
            line(gui, 320.0, &format!("{} : {}\n", translate("LEFT DISTANCE"), f2(view.sides[0])), 1.0);
        } else if let Some(d) = view.driver {
            line(gui, 230.0, &format!("{} : {}\n", translate("Prj RPM"), f2(s.proj_dn_rpm)), 1.0);
            line(gui, 260.0, &format!("{} : {}\n", translate("Distance From target"), f2(d.distance_from_target)), 1.0);
            line(gui, 290.0, &format!("{} : {}\n", translate("BASE"), f2(d.steer_min_lookahead)), 1.0);
            line(gui, 350.0, &format!("NCM : {}\n", f2(d.next_car_modifier)), 1.0);
            line(gui, 380.0, &format!("DTNC : {}\n", f2(d.dist_to_next_corner)), 1.0);
        }
        // the recording button blinks red every 500 ms of the session
        if self.tags.borrow().recording {
            let t = gui.apps.game_time;
            let grey = ((t * 0.002) as i32) % 2 == 0;
            gui.controls[self.but_start_recording].back_color = if grey { [0.4, 0.4, 0.4, 1.0] } else { [1.0, 0.0, 0.0, 1.0] };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_ai_writes_the_games_keys() {
        let d = AiDriverValues { change_up_rpm: 7500.0, steer_min_lookahead: 12.5, steer_gain: 1.1, ..AiDriverValues::default() };
        let keys = save_ai(&d, 80);
        assert_eq!(keys[0], ("HEADER", "VERSION", "4".into()));
        assert_eq!(keys[1].2, "7500");
        assert_eq!(keys[2].2, "80");
        assert_eq!(keys[3].2, "12.5");
        assert_eq!(keys.len(), 11);
    }
}
