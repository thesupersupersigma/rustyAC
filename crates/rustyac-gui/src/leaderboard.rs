// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! `ACStandardLeaderboard` (0x2e8 bytes): the table of drivers the game shows on TAB, at the
//! end of a session, in the "Race control" page and in the Leaderboard app. Two faces: the
//! entry list (car, tyre, laps, time, gap) and the time table (last lap, sectors, delta, best
//! lap, gap).
//!
//! `ACStandardLeaderboard::ACStandardLeaderboard` 0x1402d10f0, `render` 0x1402d4b70,
//! `printLBCar` 0x1402d2e20, `retrieveRowInfo` 0x1402d6830, `onMouseDown` 0x1402d2c20,
//! `onVisibleChanged` 0x1402d2d90, `setLastSession` 0x1402d6990, its seven handlers
//! (0x1402d2900 … 0x1402d2a60); `timeToString` 0x140053110, `timeToDiffString` 0x140099b70,
//! `timeToSectorString` 0x140099e60.
//!
//! The game's table reads the session through `RaceManager`; here it reads [`RaceLink`]
//! (`Gui::sim.race`), which whoever runs the GUI fills from the referee every frame.

use std::rc::Rc;

use crate::control::{Clicked, Id, Kind, MouseEvent};
use crate::font::{Font, FontType};
use crate::gui::Gui;
use crate::hud::{time_to_string3, translate};
use crate::menus::{Menu, SimAction};
use crate::painter::{Painter, Rect, Texture, ALIGN_CENTER, ALIGN_LEFT, ALIGN_RIGHT, GRAPHICS_GL};

/// `colTitleBar`, `colDarkBar`, `colDarkTransp` of the file.
const COL_TITLE_BAR: [f32; 4] = [0.74, 0.0, 0.0, 1.0];
const COL_DARK_BAR: [f32; 4] = [0.21, 0.21, 0.21, 1.0];
const COL_DARK_TRANSP: [f32; 4] = [0.1, 0.1, 0.1, 0.7];

/// `LeaderboardMode`.
pub const MODE_ENTRY_LIST: i32 = 0;
pub const MODE_TIME_TABLE: i32 = 1;
/// `LeaderboardUI`.
pub const UI_RACE_CONTROL: i32 = 0;
pub const UI_APP: i32 = 1;
pub const UI_TAB: i32 = 2;

/// `Lap`: its time (ms), its sector times, whether it counts.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RaceLap {
    pub time: u32,
    pub splits: Vec<u32>,
    pub is_valid: bool,
}

/// What the tables read of one car (`CarAvatar`, and `RaceManager`'s answers about it).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RaceCar {
    /// `driverInfo.name`, `driverInfo.nationCode`
    pub driver_name: String,
    pub nation_code: String,
    /// `unixName` (the car's folder), `currentSkin`, `guiShortName`
    pub unix_name: String,
    pub skin: String,
    pub gui_short_name: String,
    /// `isConnected()`, `isBlackFlagged`, `isInPitlane()`
    pub connected: bool,
    pub is_black_flagged: bool,
    pub in_pitlane: bool,
    /// `getCurrentCompoundShortName()`
    pub compound_short_name: String,
    /// `getBallastKG()`, `getRestrictor()`
    pub ballast_kg: f32,
    pub restrictor: f32,
    /// `ACClient::getPing(car)`
    pub ping: i32,
    /// `physicsState.performanceMeter`
    pub performance_meter: f32,
    /// `RaceManager::getLastLap(car)`, `getBestLap(car).time`, `getHasCompletedFlag(car)`
    pub last_lap: RaceLap,
    pub best_lap: u32,
    pub has_completed_flag: bool,
    /// `RaceManager::getCurrentLap(car).splits`
    pub current_splits: Vec<u32>,
    /// the car's own best sector times (`getBestSplit(i, false, car)`)
    pub personal_best_splits: Vec<u32>,
}

/// `LeaderboardEntry`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RaceEntry {
    /// the car's index in [`RaceLink::cars`] (its `guid`)
    pub car: usize,
    pub total_time: f64,
    pub best_lap: f64,
    pub laps: i32,
    pub has_completed_last_lap: bool,
}

/// `Session`, as far as the tables read it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RaceSession {
    pub session_type: i32,
    pub is_timed_race: bool,
    pub name: String,
}

/// What the tables read of the session (`RaceManager`, `ACClient`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RaceLink {
    pub cars: Vec<RaceCar>,
    /// `RaceManager::getSessionInfo(i)` of every session
    pub sessions: Vec<RaceSession>,
    /// `RaceManager::getCurrentSessionIndex`
    pub current_session_index: i32,
    /// `RaceManager::getLeaderboard()`: the running session's
    pub leaderboard: Vec<RaceEntry>,
    /// `RaceManager::leaderboardHistory`: the sessions that are over
    pub leaderboard_history: Vec<Vec<RaceEntry>>,
    /// `RaceManager::getBestLap()`: the best lap of anybody
    pub global_best_lap: u32,
    /// the best sector times of anybody (`getBestSplit(i, true, …)`)
    pub best_splits: Vec<u32>,
}

impl RaceLink {
    /// `RaceManager::getSessionInfo` 0x140139d70: a session, or an empty one.
    pub fn session_info(&self, index: i32) -> RaceSession {
        if index >= 0 {
            self.sessions.get(index as usize).cloned().unwrap_or_default()
        } else {
            RaceSession::default()
        }
    }

    /// `RaceManager::getLeaderboardFromSession` 0x140138b90.
    pub fn leaderboard_from_session(&self, index: i32) -> Vec<RaceEntry> {
        if self.current_session_index <= index {
            self.leaderboard.clone()
        } else if index >= 0 {
            self.leaderboard_history.get(index as usize).cloned().unwrap_or_default()
        } else {
            Vec::new()
        }
    }

    /// `RaceManager::isBestSplit` 0x14013eaa0.
    fn is_best_split(&self, index: usize, time: i32, global: bool, car: &RaceCar) -> bool {
        if time == 0 {
            return false;
        }
        let best = if global { self.best_splits.get(index) } else { car.personal_best_splits.get(index) };
        best.is_some_and(|best| *best == time as u32)
    }
}

/// `cvttsd2si`: a double cut to an `int`, the "indefinite" value when it does not fit.
fn cut(x: f64) -> i32 {
    if x > -2147483649.0 && x < 2147483648.0 {
        x as i32
    } else {
        i32::MIN
    }
}

/// `timeToDiffString` 0x140099b70.
pub fn time_to_diff_string(ms: i32, decimals: i32) -> String {
    let secs = ms / 1000;
    let rest = ms - secs * 1000;
    if ms == 0 {
        return match decimals {
            3 => "---",
            2 => "--",
            1 => "-",
            _ => "",
        }
        .to_string();
    }
    let sign = if ms < 0 { "-" } else { "+" };
    match decimals {
        3 => format!("{sign}{}.{:03}", secs.abs(), rest.abs()),
        2 => format!("{sign}{}.{:02}", secs.abs(), (rest / 10).abs()),
        1 => format!("{sign}{}.{:01}", secs.abs(), (rest / 100).abs()),
        _ => sign.to_string(),
    }
}

/// `timeToSectorString` 0x140099e60.
pub fn time_to_sector_string(ms: i32, decimals: i32) -> String {
    let secs = ms / 1000;
    let rest = ms - secs * 1000;
    match decimals {
        3 => format!("{secs}.{rest:03}"),
        2 => format!("{secs}.{:02}", rest / 10),
        1 => format!("{secs}.{:01}", rest / 100),
        _ => String::new(),
    }
}

/// The first `count` UTF-16 units of a text (`std::wstring::substr(0, count)`).
fn head(text: &str, count: usize) -> String {
    let units: Vec<u16> = text.encode_utf16().collect();
    String::from_utf16_lossy(&units[..units.len().min(count)])
}

/// `ACStandardLeaderboard`'s own members.
pub struct Leaderboard {
    pub session_id: i32,
    pub focus_on_player: bool,
    pub rows: i32,
    pub current_page: i32,
    pub selected_car_index: i32,
    pub show_mode_buttons: bool,
    /// `printedCars`: the cars of the rows drawn last
    pub printed_cars: Vec<usize>,
    pub bt_next_page: Id,
    pub bt_previous_page: Id,
    pub bt_kick: Option<Id>,
    pub bt_sessions: Vec<Id>,
    pub bt_entry_list: Id,
    pub bt_time_table: Id,
    pub mode: i32,
    pub last_mode: i32,
    title_font: usize,
    data_font: usize,
    black_font: usize,
    /// `avatarToBeKicked`
    pub avatar_to_be_kicked: Option<usize>,
    /// `skinTextures`: every car's `livery.png`
    skin_textures: Vec<Texture>,
    tx_mini_finish: Texture,
    pub ui: i32,
    pub is_loaded: bool,
    entry_name_x: f32,
    entry_car_x: f32,
    entry_tyres_x: f32,
    entry_ballast_x: f32,
    entry_laps_x: f32,
    entry_time_x: f32,
    entry_gap_x: f32,
    entry_ping_x: f32,
    entry_kick_x: f32,
    entry_flag_x: f32,
    time_name_x: f32,
    time_car_x: f32,
    time_lastlap_x: f32,
    time_split_x: [f32; 3],
    time_delta_x: f32,
    time_time_x: f32,
    time_gap_x: f32,
    time_ping_x: f32,
    time_kick_x: f32,
    time_flag_x: f32,
    wtop1: f32,
}

impl Gui {
    pub fn leaderboard(&self, id: Id) -> &Leaderboard {
        match &self.controls[id].kind {
            Kind::Menu(menu) => match &**menu {
                Menu::Leaderboard(l) => l,
                _ => panic!("control {id} is not a leaderboard"),
            },
            _ => panic!("control {id} is not a leaderboard"),
        }
    }

    pub fn leaderboard_mut(&mut self, id: Id) -> &mut Leaderboard {
        match &mut self.controls[id].kind {
            Kind::Menu(menu) => match &mut **menu {
                Menu::Leaderboard(l) => l,
                _ => panic!("control {id} is not a leaderboard"),
            },
            _ => panic!("control {id} is not a leaderboard"),
        }
    }

    /// `Sim::getNationFlag` 0x140199350 (`Sim::flagMapCache`).
    pub fn sim_nation_flag(&mut self, painter: &mut dyn Painter, code: &str) -> Texture {
        if code.is_empty() {
            return self.sim.flag_cache.entry("AC".to_string()).or_default().clone();
        }
        if let Some(t) = self.sim.flag_cache.get(code) {
            return t.clone();
        }
        let t = painter.get_texture(&format!("content/gui/NationFlags/{code}.png"));
        if t.kid.is_some() {
            self.sim.flag_cache.insert(code.to_string(), t.clone());
            return t;
        }
        self.sim.flag_cache.entry("AC".to_string()).or_default().clone()
    }

    /// `ACStandardLeaderboard::ACStandardLeaderboard` 0x1402d10f0.
    pub fn new_leaderboard(&mut self, painter: &mut dyn Painter, rows: i32, focus_on_player: bool) -> Id {
        let id = self.new_control(painter, "RACE_CONTROL_KICK_PLAYER");
        let mut font = |gui: &mut Gui, italic: bool| {
            gui.fonts.push(Font::new(painter, "", FontType::Monospaced, 14.0, italic, true));
            gui.fonts.len() - 1
        };
        let title_font = font(self, true);
        let buttons_font = font(self, true);
        let data_font = font(self, false);
        let black_font = font(self, false);
        self.fonts[black_font].set_color(0.0, 0.0, 0.0, 1.0);
        let skin_textures: Vec<Texture> = self.sim.race.cars.iter().map(|car| painter.get_texture(&format!("content/cars/{}/skins/{}/livery.png", car.unix_name, car.skin))).collect();
        let tx_mini_finish = painter.get_texture("content/texture/mini_finish.png");
        let menu = id;

        let page_button = |gui: &mut Gui, painter: &mut dyn Painter, texture: &str, next: bool| {
            let button = gui.new_active_button(painter, "BUT_VOTE_NEXT_PAGE");
            gui.set_multiple_texture(painter, button, texture);
            gui.set_text(button, "");
            gui.set_size(button, 40.0, 40.0);
            gui.controls[button].ev_clicked.push(Rc::new(move |gui: &mut Gui, _: &Clicked| {
                let cars = gui.sim.race.cars.len() as u32;
                let l = gui.leaderboard_mut(menu);
                if next {
                    // lambda 0x1402d2960
                    if (l.current_page as u32) < cars / l.rows as u32 {
                        l.current_page += 1;
                    }
                } else {
                    // lambda 0x1402d2930
                    l.current_page -= 1;
                    if l.current_page < 0 {
                        l.current_page = 0;
                    }
                }
            }));
            gui.set_visible(button, false);
            gui.add_control(menu, button);
            button
        };
        let bt_next_page = page_button(self, painter, "content/gui/raceControl/right_icon", true);
        let bt_previous_page = page_button(self, painter, "content/gui/raceControl/left_icon", false);
        let mut bt_sessions = Vec::new();
        let mut i = 0usize;
        while i < self.sim.race.sessions.len() {
            let button = self.new_active_button(painter, &format!("BUT_SESSION_{i}"));
            self.set_multiple_texture(painter, button, &format!("content/gui/raceControl/session_{i}"));
            self.set_text(button, "");
            self.set_size(button, 40.0, 40.0);
            // lambda 0x1402d2a40
            self.controls[button].ev_clicked.push(Rc::new(move |gui: &mut Gui, _: &Clicked| {
                let l = gui.leaderboard_mut(menu);
                l.session_id = i as i32;
                l.mode = l.last_mode;
            }));
            self.set_visible(button, false);
            bt_sessions.push(button);
            self.add_control(menu, button);
            i += 1;
        }
        let tab = |gui: &mut Gui, painter: &mut dyn Painter, name: &str, text: &str, alpha: f32| {
            let control = gui.new_control(painter, name);
            gui.controls[control].border_color = [1.0; 4];
            gui.controls[control].back_color = [COL_TITLE_BAR[0], COL_TITLE_BAR[1], COL_TITLE_BAR[2], alpha];
            gui.set_text(control, &translate(text));
            gui.set_size(control, 100.0, 18.0);
            gui.controls[control].font = Some(buttons_font);
            gui.controls[control].font_align = ALIGN_CENTER;
            gui.set_visible(control, false);
            gui.add_control(menu, control);
            control
        };
        let bt_entry_list = tab(self, painter, "LEADERBOARD_ENTRY_LIST", "ENTRY LIST", COL_TITLE_BAR[3]);
        let bt_time_table = tab(self, painter, "LEADERBOARD_TIME_TABLE", "TIME TABLE", 0.0);
        // lambdas 0x1402d2a60 (the entry list) and 0x1402d29a0 (the time table)
        for (control, mode) in [(bt_entry_list, MODE_ENTRY_LIST), (bt_time_table, MODE_TIME_TABLE)] {
            self.controls[control].ev_clicked.push(Rc::new(move |gui: &mut Gui, _: &Clicked| {
                let l = gui.leaderboard_mut(menu);
                l.mode = mode;
                l.last_mode = mode;
                let (entry, time) = (l.bt_entry_list, l.bt_time_table);
                gui.controls[entry].back_color[3] = if mode == MODE_ENTRY_LIST { COL_TITLE_BAR[3] } else { 0.0 };
                gui.controls[time].back_color[3] = if mode == MODE_TIME_TABLE { COL_TITLE_BAR[3] } else { 0.0 };
            }));
        }
        let mut bt_kick = None;
        if self.sim.online {
            let button = self.new_active_button(painter, "BUT_KICK_USER");
            self.set_multiple_texture(painter, button, "content/gui/raceControl/kick_icon");
            self.set_text(button, "");
            self.set_size(button, 30.0, 30.0);
            self.set_visible(button, false);
            // lambda 0x1402d29f0
            self.controls[button].ev_clicked.push(Rc::new(move |gui: &mut Gui, _: &Clicked| {
                if let Some(car) = gui.leaderboard(menu).avatar_to_be_kicked {
                    gui.sim.actions.push(SimAction::SendVote { kind: 2, value: true, guid: car as i32 });
                }
            }));
            self.add_control(menu, button);
            bt_kick = Some(button);
        }
        self.control_set_size(id, 780.0, 325.0);
        self.controls[id].draw_border = false;
        self.controls[id].draw_background = false;
        self.controls[id].kind = Kind::Menu(Box::new(Menu::Leaderboard(Box::new(Leaderboard {
            session_id: 0,
            focus_on_player,
            rows,
            current_page: 0,
            selected_car_index: -1,
            show_mode_buttons: false,
            printed_cars: Vec::new(),
            bt_next_page,
            bt_previous_page,
            bt_kick,
            bt_sessions,
            bt_entry_list,
            bt_time_table,
            mode: MODE_ENTRY_LIST,
            last_mode: MODE_ENTRY_LIST,
            title_font,
            data_font,
            black_font,
            avatar_to_be_kicked: None,
            skin_textures,
            tx_mini_finish,
            ui: UI_RACE_CONTROL,
            is_loaded: false,
            entry_name_x: 0.0,
            entry_car_x: 0.0,
            entry_tyres_x: 0.0,
            entry_ballast_x: 0.0,
            entry_laps_x: 0.0,
            entry_time_x: 0.0,
            entry_gap_x: 0.0,
            entry_ping_x: 0.0,
            entry_kick_x: 0.0,
            entry_flag_x: 0.0,
            time_name_x: 0.0,
            time_car_x: 0.0,
            time_lastlap_x: 0.0,
            time_split_x: [0.0; 3],
            time_delta_x: 0.0,
            time_time_x: 0.0,
            time_gap_x: 0.0,
            time_ping_x: 0.0,
            time_kick_x: 0.0,
            time_flag_x: 0.0,
            wtop1: 0.0,
        }))));
        // (the handler of Sim::evOnNewSession, 0x1402d2900: see `sim_on_new_session`)
        self.sim.leaderboards.push(id);
        id
    }

    /// `Sim::evOnNewSession` as the tables hear it (the lambda 0x1402d2900 of every one).
    pub fn sim_on_new_session(&mut self) {
        let current = self.sim.race.current_session_index;
        for id in self.sim.leaderboards.clone() {
            self.leaderboard_mut(id).session_id = current;
        }
    }

    /// `ACStandardLeaderboard::setLastSession` 0x1402d6990.
    pub fn leaderboard_set_last_session(&mut self, id: Id) {
        self.leaderboard_mut(id).session_id = (self.sim.race.sessions.len() as u32).wrapping_sub(1) as i32;
    }

    /// `ACStandardLeaderboard::onVisibleChanged` 0x1402d2d90.
    pub(crate) fn leaderboard_on_visible_changed(&mut self, id: Id, visible: bool) {
        for child in self.controls[id].controls.clone() {
            self.on_visible_changed(child, visible);
        }
        if visible {
            self.leaderboard_mut(id).session_id = self.sim.race.current_session_index;
        }
    }

    /// `ACStandardLeaderboard::onMouseDown` 0x1402d2c20: a click on a row picks its car (and,
    /// when it is connected, turns the camera to it).
    pub(crate) fn leaderboard_on_mouse_down(&mut self, id: Id, e: &MouseEvent) -> bool {
        self.control_on_mouse_down(id, e);
        if !self.leaderboard(id).focus_on_player {
            let mut r = Rect { left: 0.0, right: 1.0, top: 0.0, bottom: 1.0 };
            self.get_world_rect(id, &mut r);
            let own = self.controls[id].rect;
            let left = r.left + own.left;
            let top = r.top + own.top;
            let height = own.bottom - own.top;
            let top20 = top + 20.0;
            let bottom = height + top;
            let (x, y) = (e.x as f32, e.y as f32);
            if x > left && x < (own.right - own.left) + left && y > top20 && y < bottom {
                let rows = self.leaderboard(id).rows;
                let t = ((y - top20) / (bottom - top20)) * rows as f32;
                let index = t as i32;
                self.leaderboard_mut(id).selected_car_index = index;
                if (index as i64 as u64) < self.leaderboard(id).printed_cars.len() as u64 {
                    let car = self.leaderboard(id).printed_cars[index as usize];
                    self.leaderboard_mut(id).avatar_to_be_kicked = Some(car);
                    if self.sim.race.cars.get(car).is_some_and(|c| c.connected) {
                        // Sim::setFocusedCarIndex 0x14019e6b0
                        if car < self.sim.race.cars.len() {
                            self.sim.actions.push(SimAction::SetFocusedCarIndex(car as u32));
                        }
                    }
                }
            }
        }
        true
    }

    /// `ACStandardLeaderboard::retrieveRowInfo` 0x1402d6830: a row's time and its gap (ms; -1:
    /// no gap, the first row).
    fn leaderboard_row_info(&self, id: Id, entries: &[RaceEntry], index: usize) -> (i32, i32) {
        let is_race = self.sim.race.session_info(self.leaderboard(id).session_id).session_type == 3;
        let e = &entries[index];
        let (time, mut gap);
        if is_race {
            time = cut(e.total_time);
            gap = 0;
            if index >= 1 && time != 0 && entries[index - 1].laps == e.laps {
                gap = cut(time as f64 - entries[0].total_time);
            }
        } else {
            let mut t = cut(e.best_lap);
            if t < 0 {
                t = 0;
            }
            time = t;
            gap = cut(t as f64 - entries[0].best_lap);
        }
        if index == 0 {
            gap = -1;
        }
        (time, gap)
    }

    /// `ACStandardLeaderboard::render` 0x1402d4b70.
    pub(crate) fn leaderboard_render(&mut self, id: Id, dt: f32, painter: &mut dyn Painter) {
        let gl = GRAPHICS_GL;
        let own = self.controls[id].rect;
        let (w, h) = (own.right - own.left, own.bottom - own.top);
        let session_id = self.leaderboard(id).session_id;
        let session_type = self.sim.race.session_info(session_id).session_type;
        self.leaderboard_mut(id).printed_cars.clear();
        let (bt_time_table, bt_entry_list, bt_next, bt_previous) = {
            let l = self.leaderboard(id);
            (l.bt_time_table, l.bt_entry_list, l.bt_next_page, l.bt_previous_page)
        };
        // the two faces' tabs
        if (session_type.wrapping_sub(1) as u32) <= 2 {
            self.set_visible(bt_time_table, true);
            self.set_visible(bt_entry_list, true);
            let width = |gui: &Gui, c: Id| gui.controls[c].rect.right - gui.controls[c].rect.left;
            let x = w - (width(self, bt_time_table) + 5.0) * 2.0;
            self.set_position(bt_time_table, x, h + 10.0);
            let x = w - (width(self, bt_entry_list) + 5.0) * 3.0;
            self.set_position(bt_entry_list, x, h + 10.0);
        } else {
            self.leaderboard_mut(id).show_mode_buttons = false;
        }
        // the pages' arrows
        if self.sim.race.cars.len() as u32 > self.leaderboard(id).rows as u32 && !self.leaderboard(id).focus_on_player {
            let next_width = self.controls[bt_next].rect.right - self.controls[bt_next].rect.left;
            self.set_visible(bt_next, true);
            self.set_position(bt_next, w - next_width, h);
            self.set_visible(bt_previous, true);
            let previous_width = self.controls[bt_previous].rect.right - self.controls[bt_previous].rect.left;
            self.set_position(bt_previous, ((w - next_width) - previous_width) - 20.0, h);
        }
        // the sessions' buttons
        let current = self.sim.race.current_session_index;
        for (i, button) in self.leaderboard(id).bt_sessions.clone().into_iter().enumerate() {
            let visible = !((current as u32) < i as u32) && current > 0;
            self.set_visible(button, visible);
            self.set_position(button, (150 + 75 * i as u32) as u64 as f32, h);
            if self.leaderboard(id).session_id == i as i32 {
                self.active_button_mut(button).selected = true;
                if self.leaderboard(id).show_mode_buttons {
                    self.set_visible(bt_time_table, i as i32 == current);
                    let shown = self.controls[bt_time_table].visible;
                    self.set_visible(bt_entry_list, shown);
                    if !shown {
                        self.leaderboard_mut(id).mode = MODE_ENTRY_LIST;
                    }
                }
            } else {
                self.active_button_mut(button).selected = false;
            }
        }
        // where the table is on the screen; the columns, set once (or every frame in the app)
        let mut r = Rect { left: 0.0, right: 1.0, top: 0.0, bottom: 1.0 };
        self.get_world_rect(id, &mut r);
        let left = r.left + own.left;
        let mut top = r.top + own.top;
        let bottom = (own.bottom - own.top) + top;
        let right = r.right;
        if !self.leaderboard(id).is_loaded {
            let l = self.leaderboard_mut(id);
            if l.ui == UI_RACE_CONTROL || l.ui == UI_TAB || l.ui == UI_APP {
                let x = left + 10.0;
                l.entry_name_x = x;
                l.entry_flag_x = x + 23.0;
                l.entry_car_x = x + 180.0;
                l.entry_tyres_x = l.entry_car_x + 195.0;
                l.entry_ballast_x = l.entry_tyres_x + 65.0;
                l.entry_laps_x = l.entry_ballast_x + 65.0;
                l.entry_time_x = l.entry_laps_x + 110.0;
                l.entry_gap_x = l.entry_time_x + 75.0;
                l.entry_ping_x = l.entry_gap_x + 45.0;
                l.entry_kick_x = l.entry_ping_x + 10.0;
                l.time_name_x = x;
                l.time_flag_x = x + 23.0;
                l.time_car_x = x + 180.0;
                l.time_lastlap_x = l.time_car_x + 125.0;
                let last;
                if l.ui == UI_APP {
                    l.time_split_x[0] = l.time_lastlap_x + 50.0;
                    l.time_split_x[1] = l.time_split_x[0] + 50.0;
                    l.time_split_x[2] = l.time_split_x[1] + 50.0;
                    l.time_delta_x = l.time_split_x[2] + 60.0;
                    l.is_loaded = false;
                    last = l.time_delta_x;
                } else {
                    l.time_delta_x = l.time_lastlap_x + 70.0;
                    l.time_split_x[0] = l.time_delta_x + 50.0;
                    l.time_split_x[1] = l.time_split_x[0] + 50.0;
                    l.time_split_x[2] = l.time_split_x[1] + 50.0;
                    l.is_loaded = true;
                    last = l.time_split_x[2];
                }
                l.time_time_x = last + 90.0;
                l.time_gap_x = l.time_time_x + 75.0;
                l.time_ping_x = l.time_gap_x + 45.0;
                l.time_kick_x = l.time_ping_x + 10.0;
            }
            l.wtop1 = top + 1.0;
        }
        // the header and the strip under the table
        let mode = self.leaderboard(id).mode;
        let online = self.sim.online;
        if mode == MODE_ENTRY_LIST || mode == MODE_TIME_TABLE {
            painter.gl_color4f(gl, COL_TITLE_BAR[0], COL_TITLE_BAR[1], COL_TITLE_BAR[2], COL_TITLE_BAR[3]);
            painter.gl_quad(gl, left, top, w, 20.0, false);
            let (font, y) = (self.leaderboard(id).title_font, self.leaderboard(id).wtop1);
            self.fonts[font].set_color(1.0, 1.0, 1.0, 1.0);
            let l = self.leaderboard(id);
            let mut texts: Vec<(String, f32, i32)> = Vec::new();
            if mode == MODE_ENTRY_LIST {
                texts.push((translate("DRIVER"), l.entry_name_x, ALIGN_LEFT));
                texts.push((translate("CAR"), l.entry_car_x, ALIGN_LEFT));
                texts.push((translate("TYRE"), l.entry_tyres_x - 5.0, ALIGN_LEFT));
                texts.push((translate("LAPS"), l.entry_laps_x + 15.0, ALIGN_RIGHT));
                texts.push((translate(if self.sim.race.session_info(session_id).session_type == 3 { "TOTAL" } else { "LAPTIME" }), l.entry_time_x - 5.0, ALIGN_RIGHT));
                texts.push((translate("GAP"), l.entry_gap_x - 5.0, ALIGN_RIGHT));
                if online {
                    texts.push((translate("P"), l.entry_ping_x - 5.0, ALIGN_RIGHT));
                    texts.push((translate("BoP"), l.entry_ballast_x + 10.0, ALIGN_RIGHT));
                    texts.push((String::new(), l.entry_kick_x, ALIGN_LEFT));
                }
            } else {
                texts.push((translate("DRIVER"), l.time_name_x, ALIGN_LEFT));
                texts.push((translate("CAR"), l.time_car_x, ALIGN_LEFT));
                texts.push((translate("LASTLAP"), l.time_lastlap_x - 5.0, ALIGN_RIGHT));
                for i in 1..=3 {
                    texts.push((translate(&format!("S{i}")), l.time_split_x[i - 1] - 5.0, ALIGN_RIGHT));
                }
                texts.push(("  DELTA".to_string(), l.time_delta_x - 5.0, ALIGN_RIGHT));
                texts.push((translate("BESTLAP"), l.time_time_x - 5.0, ALIGN_RIGHT));
                texts.push((translate("GAP"), l.time_gap_x - 5.0, ALIGN_RIGHT));
                if online {
                    texts.push((translate("P"), l.time_ping_x - 5.0, ALIGN_RIGHT));
                    texts.push((String::new(), l.time_kick_x, ALIGN_LEFT));
                }
            }
            for (text, x, align) in texts {
                self.fonts[font].blit_string(painter, x, y, &text, 1.0, align);
            }
            painter.gl_color4f(gl, COL_DARK_TRANSP[0], COL_DARK_TRANSP[1], COL_DARK_TRANSP[2], COL_DARK_TRANSP[3]);
            painter.gl_quad(gl, left, h + top, w, 40.0, false);
            top = top + 20.0;
        }
        if let Some(kick) = self.leaderboard(id).bt_kick {
            self.set_visible(kick, false);
        }
        // the rows
        let entries = self.sim.race.leaderboard_from_session(session_id);
        let rows = self.leaderboard(id).rows;
        let mut printed = 0i32;
        let has_name = |gui: &Gui, entry: &RaceEntry| gui.sim.race.cars.get(entry.car).is_some_and(|car| !car.driver_name.is_empty());
        if !self.leaderboard(id).focus_on_player {
            let page = self.leaderboard(id).current_page;
            for i in 0..entries.len() {
                if (i as i32) >= page * rows && (i as i32) < (page + 1) * rows && has_name(self, &entries[i]) {
                    let (time, gap) = self.leaderboard_row_info(id, &entries, i);
                    self.leaderboard_print_car(id, painter, page * rows + printed, &entries[i], time, gap, Rect { left, right, top, bottom });
                    self.leaderboard_mut(id).printed_cars.push(entries[i].car);
                    top = top + 30.5;
                    printed += 1;
                }
            }
        } else {
            let size = entries.len() as i64;
            let p = entries.iter().position(|entry| entry.car == 0).map_or(size, |p| p as i64);
            self.leaderboard_mut(id).current_page = (p as i32) / rows;
            let mut start = p - 3;
            if start < 0 {
                start = 0;
            }
            let mut end = start + rows as i64;
            if end > size - 1 {
                end = size - 1;
            } else if end < 0 {
                end = 0;
            }
            if end - start < rows as i64 {
                let s2 = end - rows as i64 + 1;
                start = size - 1;
                if s2 < size {
                    start = s2;
                    if s2 < 0 {
                        start = 0;
                    }
                }
            }
            let mut k = start;
            while k <= end {
                let i = k as usize;
                if has_name(self, &entries[i]) {
                    let (time, gap) = self.leaderboard_row_info(id, &entries, i);
                    self.leaderboard_print_car(id, painter, k as i32, &entries[i], time, gap, Rect { left, right, top, bottom });
                    self.leaderboard_mut(id).printed_cars.push(entries[i].car);
                    top = top + 30.5;
                    printed += 1;
                }
                if printed >= rows {
                    break;
                }
                k += 1;
            }
        }
        // the session's name, the rows that are empty, the buttons
        let (font, x) = (self.leaderboard(id).data_font, self.leaderboard(id).entry_name_x);
        self.fonts[font].set_color(1.0, 1.0, 1.0, 1.0);
        let name = translate(&self.sim.race.session_info(session_id).name);
        self.fonts[font].blit_string(painter, x, bottom + 10.0, &name, 1.0, ALIGN_LEFT);
        if printed != rows {
            painter.gl_color4f(gl, 0.1, 0.1, 0.1, 0.7);
            painter.gl_quad(gl, left, top, w, (rows - printed) as f32 * 30.5, false);
        }
        self.control_render(id, dt, painter);
    }

    /// `ACStandardLeaderboard::printLBCar` 0x1402d2e20: one row.
    #[allow(clippy::too_many_arguments)]
    fn leaderboard_print_car(&mut self, id: Id, painter: &mut dyn Painter, index: i32, entry: &RaceEntry, time: i32, gap: i32, r: Rect) {
        let gl = GRAPHICS_GL;
        let Some(car) = self.sim.race.cars.get(entry.car).cloned() else { return };
        let guid = entry.car;
        let y = r.top + 5.0;
        let own = self.controls[id].rect;
        let w = own.right - own.left;
        let online = self.sim.online;
        let color = if index & 1 == 0 { COL_DARK_BAR } else { COL_DARK_TRANSP };
        painter.gl_color4f(gl, color[0], color[1], color[2], color[3]);
        painter.gl_quad(gl, r.left, r.top, w, 30.5, false);
        let n = index + 1;
        let text = format!("{n}{}{}", if n < 10 { ".    " } else { ".   " }, head(&car.driver_name, 20));
        // the picked row: a red bar at its left, and online the kick button
        if self.leaderboard(id).avatar_to_be_kicked == Some(guid) && car.connected {
            painter.gl_color4f(gl, COL_TITLE_BAR[0], COL_TITLE_BAR[1], COL_TITLE_BAR[2], COL_TITLE_BAR[3]);
            painter.gl_quad(gl, r.left, r.top, 5.0, 30.5, false);
            if let (Some(kick), true) = (self.leaderboard(id).bt_kick, guid != 0) {
                let q = self.world_to_local(id, [self.leaderboard(id).time_kick_x, r.top]);
                self.set_position(kick, q[0], q[1]);
                self.set_visible(kick, true);
            }
        }
        let name_color = if guid == 0 {
            [1.0, 1.0, 0.0, 1.0]
        } else if !car.connected {
            [0.7, 0.7, 0.7, 1.0]
        } else {
            [1.0; 4]
        };
        let (data, black) = (self.leaderboard(id).data_font, self.leaderboard(id).black_font);
        self.fonts[data].set_color4(&name_color);
        let l = self.leaderboard(id);
        let (time_name_x, time_car_x, time_flag_x, time_gap_x, entry_ping_x) = (l.time_name_x, l.time_car_x, l.time_flag_x, l.time_gap_x, l.entry_ping_x);
        self.fonts[data].blit_string(painter, time_name_x, y, &text, 1.0, ALIGN_LEFT);
        let livery = self.leaderboard(id).skin_textures.get(guid).cloned().unwrap_or_default();
        painter.set_texture(0, &livery);
        painter.gl_color4f(gl, 1.0, 1.0, 1.0, 1.0);
        painter.gl_quad(gl, time_car_x, r.top + 3.0, 21.0, 21.0, true);
        let short_name = head(&car.gui_short_name, 18);
        let flag = self.sim_nation_flag(painter, &car.nation_code);
        painter.set_texture(0, &flag);
        painter.gl_color4f(gl, 1.0, 1.0, 1.0, 1.0);
        painter.gl_quad(gl, time_flag_x, r.top + 4.0, 20.0, 20.0, true);
        if gap >= 0 {
            self.fonts[data].blit_string(painter, time_gap_x, y, &time_to_diff_string(gap, 3), 1.0, ALIGN_RIGHT);
        }
        if online {
            let text = if car.connected { car.ping.to_string() } else { "---".to_string() };
            self.fonts[data].blit_string(painter, entry_ping_x, y, &text, 1.0, ALIGN_RIGHT);
        }
        let l = self.leaderboard(id);
        let session_id = l.session_id;
        match l.mode {
            MODE_ENTRY_LIST => {
                let (entry_time_x, entry_car_x, entry_tyres_x, entry_laps_x, entry_ballast_x) = (l.entry_time_x, l.entry_car_x, l.entry_tyres_x, l.entry_laps_x, l.entry_ballast_x);
                let text = if car.is_black_flagged { translate("Retired") } else { time_to_string3(time) };
                self.fonts[data].blit_string(painter, entry_time_x, y, &text, 1.0, ALIGN_RIGHT);
                self.fonts[data].blit_string(painter, entry_car_x + 30.0, y, &short_name, 1.0, ALIGN_LEFT);
                self.fonts[data].blit_string(painter, entry_tyres_x + 5.0, y, &car.compound_short_name, 1.0, ALIGN_LEFT);
                let show_number = if !entry.has_completed_last_lap || session_id != self.sim.race.current_session_index {
                    true
                } else {
                    let session = self.sim.race.session_info(session_id);
                    session.session_type == 3 && session.is_timed_race
                };
                if show_number {
                    self.fonts[data].blit_string(painter, entry_laps_x, y, &entry.laps.to_string(), 1.0, ALIGN_RIGHT);
                } else {
                    let finish = self.leaderboard(id).tx_mini_finish.clone();
                    painter.set_texture(0, &finish);
                    painter.gl_color4f(gl, 1.0, 1.0, 1.0, 1.0);
                    painter.gl_quad(gl, entry_laps_x - 21.0, r.top + 3.0, 21.0, 21.0, true);
                }
                if online {
                    let (ballast, restrictor) = (car.ballast_kg as i32, car.restrictor as i32);
                    let mut text = if ballast == 0 { "0Kg/".to_string() } else { format!("+{ballast}Kg/") };
                    text.push_str(&if restrictor == 0 { "0%".to_string() } else { format!("{restrictor}%") });
                    self.fonts[data].blit_string(painter, entry_ballast_x + 20.0, y + 2.0, &text, 0.8, ALIGN_RIGHT);
                }
            }
            MODE_TIME_TABLE => {
                let (time_time_x, time_lastlap_x, time_split_x, time_delta_x) = (l.time_time_x, l.time_lastlap_x, l.time_split_x, l.time_delta_x);
                if car.is_black_flagged {
                    self.fonts[data].blit_string(painter, time_time_x, y, &translate("Retired"), 1.0, ALIGN_RIGHT);
                    return;
                }
                // the last lap: on red when it did not count
                let lap = &car.last_lap;
                if !lap.is_valid && lap.time != 0 {
                    painter.gl_color4f(gl, 1.0, 0.0, 0.0, 0.7);
                    painter.gl_quad(gl, time_lastlap_x - 94.0, y, 96.0, 18.0, false);
                    self.fonts[black].blit_string(painter, time_lastlap_x, y, &time_to_string3(lap.time as i32), 1.0, ALIGN_RIGHT);
                } else {
                    self.fonts[data].blit_string(painter, time_lastlap_x, y, &time_to_string3(lap.time as i32), 1.0, ALIGN_RIGHT);
                }
                // the three sectors: purple the best of all, green the driver's best
                for s in 0..3usize {
                    let xs = time_split_x[s];
                    let boxed = |painter: &mut dyn Painter, color: [f32; 4]| {
                        painter.gl_color4f(gl, color[0], color[1], color[2], color[3]);
                        painter.gl_quad(gl, xs - 44.0, y, 46.0, 18.0, false);
                    };
                    let race = &self.sim.race;
                    let mut v = 0.0f32;
                    if car.has_completed_flag {
                        if s < lap.splits.len() {
                            v = car.personal_best_splits.get(s).copied().unwrap_or(0) as u64 as f32;
                        }
                        let iv = v as i32;
                        if race.is_best_split(s, iv, true, &car) && v != 0.0 {
                            boxed(painter, [1.0, 0.0, 1.0, 0.7]);
                            self.fonts[black].blit_string(painter, xs, y, &time_to_sector_string(iv, 1), 1.0, ALIGN_RIGHT);
                        } else {
                            boxed(painter, [1.0; 4]);
                            let text = if v == 0.0 { "---".to_string() } else { time_to_sector_string(iv, 1) };
                            self.fonts[black].blit_string(painter, xs, y, &text, 1.0, ALIGN_RIGHT);
                        }
                    } else {
                        if car.current_splits.is_empty() {
                            let u = lap.splits.get(s).copied().unwrap_or(0);
                            v = (if u < 999_999 { u } else { 999_999 }) as u64 as f32;
                        } else if s < car.current_splits.len() {
                            let u = car.current_splits[s];
                            v = (if u < 999_999 { u } else { 999_999 }) as f32;
                        }
                        let iv = v as i32;
                        if race.is_best_split(s, iv, true, &car) && v != 0.0 {
                            boxed(painter, [1.0, 0.0, 1.0, 0.7]);
                            self.fonts[black].blit_string(painter, xs, y, &time_to_sector_string(iv, 1), 1.0, ALIGN_RIGHT);
                        } else if race.is_best_split(s, iv, false, &car) {
                            if v != 0.0 {
                                boxed(painter, [0.0, 1.0, 0.0, 0.7]);
                                self.fonts[black].blit_string(painter, xs, y, &time_to_sector_string(iv, 1), 1.0, ALIGN_RIGHT);
                            } else {
                                self.fonts[data].blit_string(painter, xs, y, "---", 1.0, ALIGN_RIGHT);
                            }
                        } else {
                            let text = if v == 0.0 { "---".to_string() } else { time_to_sector_string(iv, 1) };
                            self.fonts[data].blit_string(painter, xs, y, &text, 1.0, ALIGN_RIGHT);
                        }
                    }
                }
                // the best lap: in a box once the car has finished (purple: nobody was faster)
                let best = time_to_string3(car.best_lap as i32);
                if car.has_completed_flag {
                    let color = if self.sim.race.global_best_lap < car.best_lap { [1.0; 4] } else { [1.0, 0.0, 1.0, 0.7] };
                    painter.gl_color4f(gl, color[0], color[1], color[2], color[3]);
                    painter.gl_quad(gl, time_time_x - 84.0, y, 86.0, 18.0, false);
                    self.fonts[black].blit_string(painter, time_time_x, y, &best, 1.0, ALIGN_RIGHT);
                } else {
                    self.fonts[data].blit_string(painter, time_time_x, y, &best, 1.0, ALIGN_RIGHT);
                }
                // the delta to the driver's best lap
                let d = car.performance_meter;
                let text = if d > 99.0 || !(d >= -99.0) {
                    "---".to_string()
                } else if car.in_pitlane {
                    "Pit".to_string()
                } else {
                    time_to_diff_string((d * 1000.0) as i32, 1)
                };
                self.fonts[data].blit_string(painter, time_delta_x, y, &text, 1.0, ALIGN_RIGHT);
            }
            _ => {}
        }
    }
}

/// `SessionLeaderboard` (0x60 bytes): the table held on TAB, in the middle of a dimmed screen.
/// `SessionLeaderboard::SessionLeaderboard` 0x1400baf60, `renderHUD` 0x1400bb3c0, `setVisible`
/// 0x1400bb3e0, its three handlers of the window's mouse events (0x1400bb2b0 …);
/// `Sim::displayLeaderboard` 0x140044910, and what `Sim::onKeyDown` 0x14019a940 and
/// `Sim::renderHUD` 0x14019e4f0 do with the TAB key.
pub struct SessionLeaderboard {
    /// `GameObject::isActive`
    pub is_active: bool,
    pub parent_control: Id,
    pub leaderboard: Id,
}

impl SessionLeaderboard {
    /// The constructor (the game makes it with more than one car, or online), and
    /// `Sim::displayLeaderboard(false)` right after it.
    pub fn new(gui: &mut Gui, painter: &mut dyn Painter) -> SessionLeaderboard {
        let parent = gui.new_control(painter, "PARENT_CONTROL");
        let (w, h) = painter.screen();
        gui.set_size(parent, w as f32, h as f32);
        gui.set_position(parent, 0.0, 0.0);
        gui.controls[parent].draw_border = false;
        let leaderboard = gui.new_leaderboard(painter, 10, true);
        let (p, c) = (gui.controls[parent].rect, gui.controls[leaderboard].rect);
        let x = ((p.right - p.left) * 0.5) - ((c.right - c.left) * 0.5);
        let y = ((p.bottom - p.top) * 0.5) - ((c.bottom - c.top) * 0.5);
        gui.add_control(parent, leaderboard);
        gui.set_position(leaderboard, x, y);
        gui.leaderboard_mut(leaderboard).ui = UI_TAB;
        gui.leaderboard_mut(leaderboard).is_loaded = false;
        let mut board = SessionLeaderboard { is_active: true, parent_control: parent, leaderboard };
        board.display(gui, false);
        board
    }

    /// `Sim::displayLeaderboard` 0x140044910.
    pub fn display(&mut self, gui: &mut Gui, show: bool) {
        gui.set_visible(self.parent_control, show);
        self.is_active = show;
    }

    /// `Sim::onKeyDown`, its first lines: TAB shows the table, unless a menu or the results show.
    pub fn on_tab_down(&mut self, gui: &mut Gui, esc_menu_visible: bool, pause_menu_visible: bool, results_displayed: bool) {
        if !esc_menu_visible && !pause_menu_visible && !results_displayed {
            self.display(gui, true);
        }
    }

    /// `Sim::renderHUD` 0x14019e4f0, once a frame: the table goes when TAB is let go (or the
    /// ESC menu or the results come).
    pub fn poll(&mut self, gui: &mut Gui, tab_held: bool, esc_menu_visible: bool, results_displayed: bool) {
        if gui.controls[self.parent_control].visible && (!tab_held || esc_menu_visible || results_displayed) {
            self.display(gui, false);
        }
    }

    /// `SessionLeaderboard::renderHUD` 0x1400bb3c0.
    pub fn render_hud(&self, gui: &mut Gui, painter: &mut dyn Painter, dt: f32) {
        if self.is_active && gui.controls[self.parent_control].visible {
            gui.render(self.parent_control, dt, painter);
        }
    }

    pub fn on_mouse_down(&self, gui: &mut Gui, e: &MouseEvent) {
        if self.is_active {
            gui.on_mouse_down(self.parent_control, e);
        }
    }

    pub fn on_mouse_up(&self, gui: &mut Gui, e: &MouseEvent) {
        if self.is_active {
            gui.on_mouse_up(self.parent_control, e);
        }
    }

    pub fn on_mouse_move(&self, gui: &mut Gui, e: &MouseEvent) {
        if self.is_active {
            gui.on_mouse_move(self.parent_control, e);
        }
    }
}

impl Gui {
    /// `LeaderboardApp::LeaderboardApp` 0x140329bf0: the app "Leaderboard", a form with a
    /// table of five rows around the player. `Sim::addForm` puts it on the game screen.
    pub fn new_leaderboard_app(&mut self, painter: &mut dyn Painter) -> Id {
        let form = self.new_form(painter, "LEADERBOARD_APP", false);
        self.form_set_title(form, &translate("Leaderboard"));
        self.form_data_mut(form).dev_app = false;
        let board = self.new_leaderboard(painter, 5, true);
        self.set_position(board, 0.0, 30.0);
        let r = self.controls[board].rect;
        self.set_size(board, r.right - r.left, 172.5);
        self.add_control(form, board);
        let r = self.controls[board].rect;
        self.set_size(form, r.right - r.left, ((r.bottom - r.top) + r.top) + 40.0);
        self.form_data_mut(form).autohide = true;
        self.leaderboard_mut(board).ui = UI_APP;
        self.leaderboard_mut(board).is_loaded = false;
        form
    }
}
