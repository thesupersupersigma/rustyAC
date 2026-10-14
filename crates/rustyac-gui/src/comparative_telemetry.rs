// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! `ComparativeTelemetry` (0x258 bytes), the ESC menu's page "Telemetry": the player's best
//! and last laps (the `ComparativeTelemetryRecorder`'s, Task 28c) and an opponent's lap from a
//! `.tc` file as four graphs (brake, speed, gas, gear) over the lap, with the values under the
//! mouse.
//!
//! Constructor 0x14006dff0, `initOpponentSelectionForm` 0x140070600, `addToGraph`
//! 0x140070570, `updateBestPlayerData` 0x140072a40, `updateLastPlayerData` 0x140072d20,
//! `onVisibleChanged` 0x140070cb0, `onMouseMove` 0x140070bb0, `render` 0x1400710f0,
//! `renderPlayerStats` 0x140071f20; the handlers 0x14006fa20, 0x14006f990, 0x14006f970,
//! 0x14006f4c0, 0x14006f390; `ComparativeTelemetryRecorder::getCTOpponents` 0x1400741a0 and
//! `loadCTOpponent` 0x140075730.

use std::any::Any;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use rustyac_physics::replay::telemetry::CtData;

use crate::acos::format_g6;
use crate::apps::App;
use crate::control::{Clicked, Id, MouseEvent};
use crate::font::{Font, FontType};
use crate::gui::Gui;
use crate::hud::{time_to_string3, translate};
use crate::painter::{Painter, ALIGN_LEFT, GL_LINES, GRAPHICS_GL};
use crate::widgets::WidgetEvent;

/// `serieColors` 0x141513e40: the best lap, the last lap, the opponent's.
const SERIE_COLORS: [[f32; 3]; 3] = [[1.0, 0.0, 0.0], [1.0, 1.0, 1.0], [0.0, 1.0, 1.0]];

/// What the page reads of the session: the recorder (`ComparativeTelemetryRecorder`, a game
/// object of `Sim`), the player's car, where the opponents' files are.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TelemetryLink {
    /// a recorder is there (`Sim::gameObjects` has one)
    pub has_recorder: bool,
    /// `getBestTime` (u32::MAX: none), `getBestTimeData`, `getLastTime`, `getLastTimeData`
    pub best_time: u32,
    pub best: Rc<Vec<CtData>>,
    pub last_time: u32,
    pub last: Rc<Vec<CtData>>,
    /// `Documents\Assetto Corsa\ctelemetry\opponents\` (rustyAC: its own `ctelemetry\opponents`)
    pub opponents_folder: Option<PathBuf>,
    /// `TrackAvatar::unixName`, `getConfig`; the recorder's car's `unixName`
    pub track: String,
    pub track_config: String,
    pub car: String,
    /// `cars[0]->driverInfo.name`, `physicsInfo.maxGear`
    pub player_name: String,
    pub max_gear: i32,
    /// `Spline::length` of the track's AI line (m)
    pub ai_spline_length: f32,
    /// `Speed::useMPH`
    pub use_mph: bool,
}

/// `CTDriver`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CtDriver {
    pub driver_name: String,
    pub file_path: PathBuf,
    pub time: u32,
}

/// The header of a `.tc` file: the four names and the time (none: not one).
fn read_header(bytes: &[u8]) -> Option<([String; 4], u32, usize)> {
    let mut at = 4usize;
    let mut take = |n: usize| -> Option<&[u8]> {
        let s = bytes.get(at..at + n)?;
        at += n;
        Some(s)
    };
    take(0)?;
    let mut names: [String; 4] = Default::default();
    for name in &mut names {
        let len = i32::from_le_bytes(take(4)?.try_into().ok()?);
        *name = String::from_utf8_lossy(take(len.max(0) as usize)?).into_owned();
    }
    let time = u32::from_le_bytes(take(4)?.try_into().ok()?);
    Some((names, time, at))
}

/// `ComparativeTelemetryRecorder::getCTOpponents` 0x1400741a0: the first 20 files of the
/// folder (directories skipped), those of this track, layout and car.
pub fn get_ct_opponents(link: &TelemetryLink) -> Vec<CtDriver> {
    let Some(folder) = &link.opponents_folder else { return Vec::new() };
    let Ok(entries) = std::fs::read_dir(folder) else { return Vec::new() };
    let mut names: Vec<String> = entries.filter_map(|e| e.ok()).filter(|e| !e.path().is_dir()).map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    // (_wfindfirst / _wfindnext: the file system's order, by name on NTFS)
    names.sort_by_key(|n| n.to_uppercase());
    let mut out = Vec::new();
    for name in names.iter().take(20) {
        let path = folder.join(name);
        let Ok(bytes) = std::fs::read(&path) else { continue };
        let Some(([driver, track, car, config], time, _)) = read_header(&bytes) else { continue };
        if track == link.track && car == link.car && config == link.track_config {
            out.push(CtDriver { driver_name: driver, file_path: path, time });
        }
    }
    out
}

/// `ComparativeTelemetryRecorder::loadCTOpponent` 0x140075730.
pub fn load_ct_opponent(path: &Path) -> Vec<CtData> {
    let Ok(bytes) = std::fs::read(path) else { return Vec::new() };
    let Some((_, _, at)) = read_header(&bytes) else { return Vec::new() };
    let Some(count) = bytes.get(at..at + 4).map(|b| i32::from_le_bytes([b[0], b[1], b[2], b[3]])) else { return Vec::new() };
    let mut out = Vec::new();
    for k in 0..count.max(0) as usize {
        match bytes.get(at + 4 + k * 20..at + 4 + k * 20 + 20) {
            Some(b) => out.push(CtData::from_bytes(b)),
            // (the game's read past the end leaves the rest of the vector as it was: zeroes)
            None => out.push(CtData::default()),
        }
    }
    out
}

/// `cvttss2si` into 32 bits: NaN and what does not fit is `i32::MIN`.
#[allow(clippy::manual_range_contains)]
fn cvtt(x: f32) -> i32 {
    if x.is_nan() || x >= 2_147_483_648.0 || x < -2_147_483_648.0 {
        i32::MIN
    } else {
        x as i32
    }
}

/// `cvttsd2si` into 32 bits.
#[allow(clippy::manual_range_contains)]
fn cvtt_d(x: f64) -> i32 {
    if x.is_nan() || x >= 2_147_483_648.0 || x <= -2_147_483_649.0 {
        i32::MIN
    } else {
        x as i32
    }
}

/// The compiler's `floorf` of render: the truncation, one lower for a negative number that is
/// not whole; what does not fit stays as it is.
fn floor_emitted(x: f32) -> f32 {
    let t = cvtt(x);
    if t != i32::MIN && t as f32 != x {
        (t - x.is_sign_negative() as i32) as f32
    } else {
        x
    }
}

/// `ComparativeTelemetry`'s own members.
pub struct ComparativeTelemetry {
    /// `seriesTime`: the best lap's, the last lap's, the opponent's (ms)
    pub series_time: [u32; 3],
    pub graph_speed: Id,
    pub graph_gas: Id,
    pub graph_brake: Id,
    pub graph_gear: Id,
    pub os_form: Id,
    pub current_perc: f32,
    pub cb_player_best: Id,
    pub cb_player_last_best: Id,
    pub but_opponent: Id,
    pub opponents: Vec<CtDriver>,
    /// the form's own font (Control+0x90)
    pub font: usize,
}

impl Gui {
    fn ct(&mut self, id: Id) -> &mut ComparativeTelemetry {
        self.app_mut::<ComparativeTelemetry>(id).expect("the page is a ComparativeTelemetry")
    }

    /// `ComparativeTelemetry::ComparativeTelemetry(Sim*)` 0x14006dff0.
    pub fn new_comparative_telemetry(&mut self, painter: &mut dyn Painter) -> Id {
        let form = self.new_form(painter, "COMPARATIVE_TELEMETRY", false);
        self.fonts.push(Font::new(painter, "", FontType::Monospaced, 16.0, false, false));
        let font = self.fonts.len() - 1;
        self.controls[form].font = Some(font);
        self.controls[form].back_color = [0.0, 0.0, 0.0, f32::from_bits(0x3f4c_cccd)];
        let tenth = f32::from_bits(0x3dcc_cccd);
        self.controls[form].border_color = [tenth, tenth, tenth, 1.0];
        self.controls[form].text = String::new();
        if let Some(title) = self.form_data(form).form_title {
            self.set_visible(title, false);
        }
        let graph = |gui: &mut Gui, painter: &mut dyn Painter, name: &str, max_y: f32, min_y: f32| {
            let g = gui.new_graph(painter, name);
            {
                let d = gui.graph_mut(g);
                d.max_y = max_y;
                d.min_y = min_y;
                d.max_values_count = 0;
            }
            gui.controls[g].back_color = [0.0; 4];
            gui.controls[g].border_color = [0.0; 4];
            for c in SERIE_COLORS {
                gui.graph_add_serie(g, c);
            }
            g
        };
        let (one_05, minus_05) = (f32::from_bits(0x3f86_6666), f32::from_bits(0xbd4c_cccd));
        // (the brake graph is added before it is set up, the others after)
        let graph_brake = self.new_graph(painter, "BRAKE_GRAPH");
        self.control_add_control(form, graph_brake);
        {
            let d = self.graph_mut(graph_brake);
            d.max_y = one_05;
            d.min_y = minus_05;
            d.max_values_count = 0;
        }
        self.controls[graph_brake].back_color = [0.0; 4];
        self.controls[graph_brake].border_color = [0.0; 4];
        for c in SERIE_COLORS {
            self.graph_add_serie(graph_brake, c);
        }
        let graph_speed = self.new_graph(painter, "SPEED_GRAPH");
        {
            let d = self.graph_mut(graph_speed);
            d.auto_adjust_max_value = true;
            d.max_y = 220.0;
            d.min_y = -5.0;
            d.max_values_count = 0;
        }
        self.controls[graph_speed].back_color = [0.0; 4];
        self.controls[graph_speed].border_color = [0.0; 4];
        for c in SERIE_COLORS {
            self.graph_add_serie(graph_speed, c);
        }
        self.control_add_control(form, graph_speed);
        let graph_gas = graph(self, painter, "GAS_GRAPH", one_05, minus_05);
        self.control_add_control(form, graph_gas);
        let max_gear = self.sim.telemetry.max_gear;
        let graph_gear = graph(self, painter, "GEAR_GRAPH", max_gear as f32 + 2.0, -0.5);
        self.control_add_control(form, graph_gear);

        let player = self.sim.telemetry.player_name.clone();
        let cb_player_best = self.new_check_box(painter, "CB_PLAYER_BEST");
        let best = translate("Best");
        self.set_text(cb_player_best, &format!("{player} {best}"));
        self.set_check(cb_player_best, true);
        self.check_box_mut(cb_player_best).handlers.push(Rc::new(move |gui: &mut Gui, ev: &WidgetEvent| {
            if let WidgetEvent::CheckBoxChanged { value, .. } = ev {
                if *value {
                    gui.ct_update_best_player_data(form, true);
                } else {
                    gui.ct_clear_serie(form, 0);
                }
            }
        }));
        self.controls[cb_player_best].fore_color = [1.0, 0.0, 0.0, 1.0];
        self.control_add_control(form, cb_player_best);

        let cb_player_last_best = self.new_check_box(painter, "CB_PLAYER_CURRENT_BEST");
        let last = translate("Last");
        self.set_text(cb_player_last_best, &format!("{player} {last}"));
        self.set_check(cb_player_last_best, true);
        self.check_box_mut(cb_player_last_best).handlers.push(Rc::new(move |gui: &mut Gui, ev: &WidgetEvent| {
            if let WidgetEvent::CheckBoxChanged { value, .. } = ev {
                if *value {
                    gui.ct_update_last_player_data(form, true);
                } else {
                    gui.ct_clear_serie(form, 1);
                }
            }
        }));
        self.controls[cb_player_last_best].fore_color = [1.0, 1.0, 1.0, 1.0];
        self.control_add_control(form, cb_player_last_best);

        let but_opponent = self.new_control(painter, "SELECT_OPPONENT");
        self.set_text(but_opponent, &translate("Select Opponent"));
        self.control_add_control(form, but_opponent);
        self.controls[but_opponent].ev_clicked.push(Rc::new(move |gui: &mut Gui, _ev: &Clicked| {
            let os = gui.ct(form).os_form;
            let shown = gui.controls[os].visible;
            gui.set_visible(os, !shown);
        }));
        self.controls[but_opponent].fore_color = [0.0, 1.0, 1.0, 1.0];

        self.set_app(
            form,
            Box::new(ComparativeTelemetry {
                series_time: [0; 3],
                graph_speed,
                graph_gas,
                graph_brake,
                graph_gear,
                os_form: form,
                current_perc: f32::from_bits(0xbf80_0000),
                cb_player_best,
                cb_player_last_best,
                but_opponent,
                opponents: Vec::new(),
                font,
            }),
        );
        self.ct_update_best_player_data(form, false);
        self.ct_update_last_player_data(form, false);
        self.ct_init_opponent_selection_form(painter, form);
        form
    }

    /// The four graphs, in the order the game names them: brake, speed, gas, gear.
    fn ct_graphs(&mut self, id: Id) -> [Id; 4] {
        let c = self.ct(id);
        [c.graph_brake, c.graph_speed, c.graph_gas, c.graph_gear]
    }

    fn ct_clear_serie(&mut self, id: Id, serie: u32) {
        for g in self.ct_graphs(id) {
            self.graph_clear_serie(g, serie);
        }
    }

    fn ct_set_max_values(&mut self, id: Id, n: u32) {
        for g in self.ct_graphs(id) {
            self.graph_mut(g).max_values_count = n;
        }
    }

    /// `ComparativeTelemetry::initOpponentSelectionForm` 0x140070600.
    fn ct_init_opponent_selection_form(&mut self, painter: &mut dyn Painter, id: Id) {
        let os = self.new_form(painter, "OPPONENT_SELECTION_FORM", false);
        self.ct(id).os_form = os;
        self.control_add_control(id, os);
        self.form_set_icon_visible(os, false);
        self.set_visible(os, false);
        self.set_size(os, 300.0, 250.0);
        self.set_position(os, 0.0, 0.0);
        if let Some(title) = self.form_data(os).form_title {
            self.set_text(title, &translate("Select the record"));
        }
        self.form_set_icon(painter, os, "content/gui/ctelemetry/close.png");
        self.form_set_icon_size(os, 23.0, 23.0);
        self.controls[os].back_color = [0.0, 0.0, 0.0, 1.0];
        let list = self.new_list_box(painter, "OS_LISTBOX", 10, 1);
        if self.sim.telemetry.has_recorder {
            let opponents = get_ct_opponents(&self.sim.telemetry);
            self.ct(id).opponents = opponents;
        }
        let title_row = self.list_box(list).title_row;
        self.set_visible(title_row, false);
        self.list_box_draw_cells_backgrounds(list, false);
        for o in self.ct(id).opponents.clone() {
            let mut row = crate::setup_io::row_data(&o.driver_name);
            row.columns.push(time_to_string3(o.time as i32));
            row.forecolors.push([1.0; 4]);
            self.list_box_add_item(list, &row);
        }
        self.set_position(list, 0.0, 50.0);
        let r = self.controls[os].rect;
        self.set_size(list, r.right - r.left, (r.bottom - r.top) - 50.0);
        self.list_box_mut(list).allow_deselection = true;
        self.list_box_mut(list).handlers.push(Rc::new(move |gui: &mut Gui, ev: &WidgetEvent| match ev {
            // lambda_e458b8 0x14006f4c0: an opponent chosen
            WidgetEvent::ListBoxRow { row, kind: 0, .. } => {
                let Some(o) = gui.ct(id).opponents.get(*row as usize).cloned() else { return };
                gui.ct(id).series_time[2] = o.time;
                let data = load_ct_opponent(&o.file_path);
                gui.ct_clear_serie(id, 2);
                gui.ct_set_max_values(id, data.len() as u32);
                let [brake, speed, gas, gear] = gui.ct_graphs(id);
                for d in &data {
                    gui.graph_add_value(brake, 2, d.brake);
                    gui.graph_add_value(speed, 2, d.speed);
                    gui.graph_add_value(gas, 2, d.gas);
                    gui.graph_add_value(gear, 2, d.gear as f32);
                }
                let button = gui.ct(id).but_opponent;
                gui.set_text(button, &o.driver_name);
            }
            // lambda_6e1f2f 0x14006f390: the opponent let go
            WidgetEvent::ListBoxRow { kind: 1, .. } => {
                gui.ct_clear_serie(id, 2);
                gui.ct(id).series_time[2] = 0;
                let button = gui.ct(id).but_opponent;
                gui.set_text(button, &translate("Select Opponent"));
            }
            _ => {}
        }));
        self.control_add_control(os, list);
        self.list_box_set_items_font_size(list, 15.0);
        self.controls[list].back_color = [0.0, 0.0, 0.0, 1.0];
        self.controls[list].border_color = [0.0, 0.0, 0.0, 1.0];
    }

    /// `ComparativeTelemetry::addToGraph` 0x140070570.
    fn ct_add_to_graph(&mut self, id: Id, serie: u32, d: &CtData) {
        let [brake, speed, gas, gear] = self.ct_graphs(id);
        self.graph_add_value(brake, serie, d.brake);
        self.graph_add_value(speed, serie, d.speed);
        self.graph_add_value(gas, serie, d.gas);
        self.graph_add_value(gear, serie, d.gear as f32);
    }

    /// `ComparativeTelemetry::updateBestPlayerData` 0x140072a40.
    pub fn ct_update_best_player_data(&mut self, id: Id, force: bool) {
        let link = &self.sim.telemetry;
        if !link.has_recorder || link.best_time == u32::MAX {
            return;
        }
        let (time, data) = (link.best_time, link.best.clone());
        if !(self.ct(id).series_time[0] != time || force) {
            return;
        }
        self.ct(id).series_time[0] = time;
        let n = data.len() as u32;
        self.ct_set_max_values(id, n);
        self.ct_clear_serie(id, 0);
        self.ct_set_max_values(id, n);
        for d in data.iter() {
            self.ct_add_to_graph(id, 0, d);
        }
    }

    /// `ComparativeTelemetry::updateLastPlayerData` 0x140072d20.
    pub fn ct_update_last_player_data(&mut self, id: Id, force: bool) {
        let link = &self.sim.telemetry;
        if !link.has_recorder || link.last_time == u32::MAX {
            return;
        }
        let (time, data) = (link.last_time, link.last.clone());
        if !(self.ct(id).series_time[1] != time || force) {
            return;
        }
        self.ct(id).series_time[1] = time;
        let n = data.len() as u32;
        self.ct_clear_serie(id, 1);
        self.ct_set_max_values(id, n);
        for d in data.iter() {
            self.ct_add_to_graph(id, 1, d);
        }
    }

    /// `ComparativeTelemetry::renderPlayerStats` 0x140071f20.
    fn ct_render_player_stats(&mut self, id: Id, painter: &mut dyn Painter) {
        let c = self.ct(id);
        let (perc, font, cb_best, cb_last, speed, gas, brake, gear) = (c.current_perc, c.font, c.cb_player_best, c.cb_player_last_best, c.graph_speed, c.graph_gas, c.graph_brake, c.graph_gear);
        if perc == -1.0 || perc.is_nan() {
            return;
        }
        let mut y = self.controls[cb_best].rect.top;
        let r = self.controls[id].rect;
        let w = r.right - r.left;
        let x1 = w * f32::from_bits(0x3eb3_3333);
        let x2 = w * f32::from_bits(0x3ef0_a3d7);
        let x3 = w * f32::from_bits(0x3f17_0a3d);
        let x4 = w * f32::from_bits(0x3f38_51ec);
        let x5 = w * f32::from_bits(0x3f57_0a3d);
        self.fonts[font].set_color(1.0, 1.0, 1.0, 1.0);
        let mut i = 0usize;
        while i < self.graph(speed).series.len() {
            let c = SERIE_COLORS[i.min(2)];
            self.fonts[font].set_color(c[0], c[1], c[2], 1.0);
            let p = self.local_to_world(id, [x1, y]);
            let len = self.graph(speed).series[i].values.len() as u32;
            if len == 0 {
                y += 25.0;
                let show = match i {
                    0 => self.check_box_mut(cb_best).box_value,
                    1 => self.check_box_mut(cb_last).box_value,
                    _ => true,
                };
                if show {
                    let text = translate("NO RECORD FOUND");
                    self.fonts[font].blit_string(painter, p[0], p[1], &text, 1.0, ALIGN_LEFT);
                }
            } else {
                let time = self.ct(id).series_time[i.min(2)];
                self.fonts[font].blit_string(painter, p[0], p[1], &time_to_string3(time as i32), 1.0, ALIGN_LEFT);
                let v = self.graph_value_at_percentage(speed, i, perc).unwrap_or(0.0);
                let text = if !self.sim.telemetry.use_mph { format!("{}Km/h", cvtt(v)) } else { format!("{}Mph", cvtt_d(v as f64 * f64::from_bits(0x3fe3_e249_13d1_2e9e))) };
                let q = self.local_to_world(id, [x2, y]);
                self.fonts[font].blit_string(painter, q[0], q[1], &text, 1.0, ALIGN_LEFT);
                let g = self.graph_value_at_percentage(gas, i, perc).unwrap_or(0.0);
                let q = self.local_to_world(id, [x3, y]);
                self.fonts[font].blit_string(painter, q[0], q[1], &format!("T : {} %", cvtt(g * 100.0)), 1.0, ALIGN_LEFT);
                let b = self.graph_value_at_percentage(brake, i, perc).unwrap_or(0.0);
                let q = self.local_to_world(id, [x4, y]);
                self.fonts[font].blit_string(painter, q[0], q[1], &format!("B : {} %", cvtt(b * 100.0)), 1.0, ALIGN_LEFT);
                let gr = self.graph_value_at_percentage(gear, i, perc).unwrap_or(0.0);
                let text = if gr > 1.0 { format!("G :{}", format_g6(gr - 1.0)) } else { "G :N".to_string() };
                let q = self.local_to_world(id, [x5, y]);
                self.fonts[font].blit_string(painter, q[0], q[1], &text, 1.0, ALIGN_LEFT);
                y += 25.0;
            }
            i += 1;
        }
    }

    /// `ComparativeTelemetry::render` 0x1400710f0.
    fn ct_render(&mut self, id: Id, dt: f32, painter: &mut dyn Painter) {
        let c = self.ct(id);
        let (os, brake, speed, gas, gear, cb_best, cb_last, button, font) = (c.os_form, c.graph_brake, c.graph_speed, c.graph_gas, c.graph_gear, c.cb_player_best, c.cb_player_last_best, c.but_opponent, c.font);
        self.set_position(os, 0.0, 0.0);
        self.ct_update_best_player_data(id, false);
        self.ct_update_last_player_data(id, false);
        let size = |gui: &Gui| {
            let r = gui.controls[id].rect;
            (r.right - r.left, r.bottom - r.top)
        };
        let small = f32::from_bits(0x3e19_999a);
        let third = f32::from_bits(0x3eaa_aaab);
        let (w, h) = size(self);
        self.set_size(brake, w, h * small);
        let (w, h) = size(self);
        self.set_size(speed, w, h * f32::from_bits(0x3ecc_cccd));
        let (w, h) = size(self);
        self.set_size(gas, w, h * small);
        let (w, h) = size(self);
        self.set_size(gear, w, h * small);
        let (w, _) = size(self);
        self.set_size(cb_best, w * third, 13.0);
        let (w, _) = size(self);
        self.set_size(cb_last, w * third, 13.0);
        let (w, _) = size(self);
        self.set_size(button, w * third, 25.0);
        self.set_position(brake, 0.0, 0.0);
        let y = self.controls[brake].rect.bottom;
        self.set_position(speed, 0.0, y);
        let y = self.controls[speed].rect.bottom;
        self.set_position(gas, 0.0, y);
        let y = self.controls[gas].rect.bottom;
        self.set_position(gear, 0.0, y);
        let y = self.controls[gear].rect.bottom + 8.0;
        self.set_position(cb_best, 10.0, y);
        let y = self.controls[cb_best].rect.top + 25.0;
        self.set_position(cb_last, 10.0, y);
        let y = self.controls[cb_last].rect.top + 25.0;
        self.set_position(button, 10.0, y);
        {
            let (b, o) = (self.controls[button].rect, self.controls[os].rect);
            let y = b.top - (o.bottom - o.top);
            let x = ((b.right - b.left) * 0.5 + b.left) - (o.right - o.left) * 0.5;
            self.set_position(os, x, y);
        }
        self.control_render(id, dt, painter);
        let gl = GRAPHICS_GL;
        let perc = self.ct(id).current_perc;
        if perc >= 0.0 || perc.is_nan() {
            painter.gl_color4f(gl, 1.0, 1.0, 0.0, 1.0);
            painter.gl_begin(gl, GL_LINES);
            let (w, _) = size(self);
            let v1 = self.local_to_world(id, [w * perc, 0.0]);
            painter.gl_vertex3f(gl, v1[0], v1[1], 0.0);
            let (w, _) = size(self);
            let v2 = self.local_to_world(id, [w * perc, self.controls[gear].rect.bottom]);
            painter.gl_vertex3f(gl, v2[0], v2[1], 0.0);
            painter.gl_end(gl);
            self.fonts[font].set_color(1.0, 1.0, 0.0, 1.0);
            let len = self.sim.telemetry.ai_spline_length;
            let v = floor_emitted(len * perc);
            let v = (v * f32::from_bits(0x3c23_d70a)) * f32::from_bits(0x3dcc_cccd);
            let text = format!("Km : {}", format_g6(v));
            let (w, _) = size(self);
            let p = self.local_to_world(id, [w - 75.0, self.controls[cb_last].rect.top]);
            self.fonts[font].blit_string(painter, p[0] - 20.0, p[1], &text, 1.0, ALIGN_LEFT);
        }
        self.ct_render_player_stats(id, painter);
        self.fonts[font].set_color(0.5, 0.5, 0.5, 1.0);
        painter.gl_color4f(gl, 0.5, 0.5, 0.5, 1.0);
        for (g, name) in [(brake, "BRAKE"), (speed, "SPEED"), (gas, "GAS"), (gear, "GEAR")] {
            painter.gl_begin(gl, GL_LINES);
            let r = self.controls[g].rect;
            let v1 = self.local_to_world(id, [r.left, r.top]);
            painter.gl_vertex3f(gl, v1[0], v1[1], 0.0);
            let v2 = self.local_to_world(id, [r.right, r.top]);
            painter.gl_vertex3f(gl, v2[0], v2[1], 0.0);
            painter.gl_end(gl);
            let label = format!(" {}", translate(name));
            self.fonts[font].blit_string(painter, v1[0], (r.bottom - r.top) * 0.5 + v1[1], &label, 1.0, ALIGN_LEFT);
        }
        painter.gl_begin(gl, GL_LINES);
        let r = self.controls[gear].rect;
        let w1 = self.local_to_world(id, [r.left, r.bottom]);
        painter.gl_vertex3f(gl, w1[0], w1[1], 0.0);
        let w2 = self.local_to_world(id, [r.right, r.bottom]);
        painter.gl_vertex3f(gl, w2[0], w2[1], 0.0);
        painter.gl_end(gl);
        // (drawn a second time, over the lines and labels)
        if self.controls[os].visible {
            self.render(os, dt, painter);
        }
    }
}

impl App for ComparativeTelemetry {
    fn as_any(&mut self) -> &mut dyn Any {
        self
    }

    fn render(&mut self, gui: &mut Gui, id: Id, dt: f32, painter: &mut dyn Painter) {
        lend(self, gui, id, |gui| gui.ct_render(id, dt, painter));
    }

    // (the children's handlers read the page's members: they are lent back to the form while
    // the mouse reaches them)
    fn on_mouse_down(&mut self, gui: &mut Gui, id: Id, e: &MouseEvent) -> bool {
        lend(self, gui, id, |gui| gui.control_on_mouse_down(id, e))
    }

    fn on_mouse_up(&mut self, gui: &mut Gui, id: Id, e: &MouseEvent) {
        lend(self, gui, id, |gui| gui.control_on_mouse_up(id, e));
    }

    fn on_mouse_wheel(&mut self, gui: &mut Gui, id: Id, e: &MouseEvent) {
        lend(self, gui, id, |gui| gui.control_on_mouse_wheel(id, e));
    }

    /// `ComparativeTelemetry::onVisibleChanged` 0x140070cb0: neither Form's nor Control's.
    fn on_visible_changed(&mut self, gui: &mut Gui, id: Id, visible: bool) {
        if visible {
            lend(self, gui, id, |gui| {
                gui.ct_update_best_player_data(id, true);
                gui.ct_update_last_player_data(id, true);
            });
        }
    }

    /// `ComparativeTelemetry::onMouseMove` 0x140070bb0.
    fn on_mouse_move(&mut self, gui: &mut Gui, id: Id, e: &MouseEvent) {
        let o = gui.local_to_world(id, [0.0, 0.0]);
        let (x, y) = (e.x as f32, e.y as f32);
        let r = gui.controls[id].rect;
        let (w, h) = (r.right - r.left, r.bottom - r.top);
        if !(x > o[0] && x < w + o[0] && y > o[1] && y < h + o[1]) {
            return;
        }
        let p = (x - o[0]) / w;
        self.current_perc = if p > 1.0 {
            1.0
        } else if p >= 0.0 {
            p
        } else {
            0.0
        };
        lend(self, gui, id, |gui| gui.form_on_mouse_move(id, e));
    }

    /// vtable +0xa8: nothing (the page is never dragged).
    fn on_title_clicked(&mut self, _gui: &mut Gui, _id: Id, _ev: &Clicked) {}
}

/// `f` with the page's members back on its form.
fn lend<R>(ct: &mut ComparativeTelemetry, gui: &mut Gui, id: Id, f: impl FnOnce(&mut Gui) -> R) -> R {
    gui.set_app(id, Box::new(std::mem::replace(ct, placeholder())));
    let r = f(gui);
    restore(gui, id, ct);
    r
}

/// An empty stand-in while the members are lent back to the form.
fn placeholder() -> ComparativeTelemetry {
    ComparativeTelemetry { series_time: [0; 3], graph_speed: 0, graph_gas: 0, graph_brake: 0, graph_gear: 0, os_form: 0, current_perc: 0.0, cb_player_best: 0, cb_player_last_best: 0, but_opponent: 0, opponents: Vec::new(), font: 0 }
}

/// The members back from the form after a call that needed them there.
fn restore(gui: &mut Gui, id: Id, into: &mut ComparativeTelemetry) {
    if let crate::control::Kind::Form(data) = &mut gui.controls[id].kind {
        if let Some(mut app) = data.app.take() {
            if let Some(ct) = app.as_any().downcast_mut::<ComparativeTelemetry>() {
                *into = std::mem::replace(ct, placeholder());
            }
        }
    }
}
