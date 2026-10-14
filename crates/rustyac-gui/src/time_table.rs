// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! `EscMenuTimeTable` (0x360 bytes), the ESC menu's page "Time": the laps of the focused car
//! in one session (the one the page "Race control" shows), with the best of that driver, of
//! the player and of the session above them, and "PRINT" (the laps as a CSV file).
//!
//! Constructor 0x140094230, the page buttons' handlers 0x1400967c0 and 0x140096820, PRINT
//! 0x1400959c0, `onVisibleChanged` 0x140096ef0, `refreshSession` 0x1400972f0, `refreshPlayer`
//! 0x140096f60, `render` 0x140097660; `timeToString` 0x140053110.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::rc::Rc;

use crate::control::{Clicked, Id, Kind};
use crate::font::{Font, FontType};
use crate::gui::Gui;
use crate::hud::{time_to_string3, translate};
use crate::leaderboard::{time_to_diff_string, time_to_sector_string};
use crate::menus::Menu;
use crate::painter::{Painter, Rect, Texture, ALIGN_CENTER, ALIGN_LEFT, ALIGN_RIGHT, GRAPHICS_GL};
use crate::setup_tab::cut;

const COL_TITLE_BAR: [f32; 4] = [0.74, 0.0, 0.0, 1.0];
const COL_DARK_BAR: [f32; 4] = [0.21, 0.21, 0.21, 1.0];
const COL_DARK_TRANSP: [f32; 4] = [0.1, 0.1, 0.1, 0.7];

/// `Lap` (0x48 bytes).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TimeLap {
    pub time: u32,
    /// every sector's own time
    pub splits: Vec<u32>,
    pub cuts: u32,
    pub is_valid: bool,
    pub compound: String,
}

/// `SessionResult` of a session that is over (`RaceManager::getSessionHistory`): every car's
/// laps and best lap.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TimeHistory {
    pub laps: Vec<Vec<TimeLap>>,
    pub best_laps: Vec<TimeLap>,
}

/// What the page reads of the session besides [`crate::leaderboard::RaceLink`].
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TimeLink {
    /// `RaceManager::getLaps(car, false)` of every car: the running session's laps
    pub laps: Vec<Vec<TimeLap>>,
    /// `RaceManager::sessionHistoryList`
    pub history: Vec<TimeHistory>,
    /// `Sim::getFocusedCarIndex`
    pub focused_car: usize,
    /// how many sectors the track has
    pub sector_count: usize,
    /// the track's layout, and where PRINT writes (rustyAC's own `out` folder)
    pub track_config: String,
    pub out_dir: PathBuf,
    /// "DDMMYY-HHMMSS" of now, for PRINT's file name (whoever runs the GUI sets it)
    pub time_stamp: String,
}

/// `EscMenuTimeTable`'s own members (the ones the game's code reads).
pub struct TimeTable {
    pub title_font: usize,
    pub data_font: usize,
    pub black_font: usize,
    pub big_font: usize,
    pub bt_lap_next_page: Id,
    pub bt_lap_previous_page: Id,
    pub bt_print: Id,
    pub last_driver_index: i32,
    pub current_lap_page: i32,
    pub session_id: i32,
    pub update_driver: bool,
    pub max_rows: i32,
    pub laps_buttons_top: f32,
    pub table_width: f32,
    pub update_selected_best: bool,
    pub selected_laps: Vec<TimeLap>,
    pub selected_lap_count: i32,
    pub selected_best_splits: [u32; 3],
    pub selected_bestlap: u32,
    pub best_splits: [u32; 3],
    pub bestlap: u32,
    pub player_splits: [u32; 3],
    pub player_bestlap: u32,
    /// by the car's number (the game: by its address)
    pub skin_textures: BTreeMap<usize, Texture>,
    pub name_x: f32,
    pub car_x: f32,
    pub bestlap_x: f32,
    pub split_x: [f32; 3],
    pub lapcount_x: f32,
    pub compound_x: f32,
    pub laptime_x: f32,
    pub lap_split_x: [f32; 3],
    pub delta_x: f32,
}

impl Gui {
    pub fn time_table(&self, id: Id) -> &TimeTable {
        match &self.controls[id].kind {
            Kind::Menu(menu) => match &**menu {
                Menu::TimeTable(table) => table,
                _ => panic!("control {id} is not the time table"),
            },
            _ => panic!("control {id} is not the time table"),
        }
    }

    pub fn time_table_mut(&mut self, id: Id) -> &mut TimeTable {
        match &mut self.controls[id].kind {
            Kind::Menu(menu) => match &mut **menu {
                Menu::TimeTable(table) => table,
                _ => panic!("control {id} is not the time table"),
            },
            _ => panic!("control {id} is not the time table"),
        }
    }

    fn time_table_page_button(&mut self, painter: &mut dyn Painter, id: Id, name: &str, texture: &str, next: bool) -> Id {
        let button = self.new_active_button(painter, name);
        self.set_multiple_texture(painter, button, texture);
        self.set_text(button, "");
        self.set_size(button, 40.0, 40.0);
        self.controls[button].ev_clicked.push(Rc::new(move |gui: &mut Gui, _ev: &Clicked| {
            let table = gui.time_table_mut(id);
            if next {
                // (the page after the last full one can be reached: it is empty)
                let pages = table.selected_laps.len() as u64 / table.max_rows.max(1) as u32 as u64;
                if (table.current_lap_page as i64 as u64) < pages {
                    table.current_lap_page += 1;
                }
            } else {
                table.current_lap_page = (table.current_lap_page - 1).max(0);
            }
        }));
        self.set_visible(button, false);
        self.add_control(id, button);
        button
    }

    /// `EscMenuTimeTable::EscMenuTimeTable` 0x140094230.
    pub fn new_time_table(&mut self, painter: &mut dyn Painter) -> Id {
        let id = self.new_control(painter, "ESC_MENU_TIME_TABLE");
        let mut font = |gui: &mut Gui, scale: f32, italic: bool| {
            gui.fonts.push(Font::new(painter, "", FontType::Monospaced, scale, italic, true));
            gui.fonts.len() - 1
        };
        let title_font = font(self, 14.0, true);
        let data_font = font(self, 14.0, false);
        let black_font = font(self, 14.0, false);
        let big_font = font(self, 25.0, true);
        self.fonts[black_font].set_color(0.0, 0.0, 0.0, 1.0);
        self.controls[id].kind = Kind::Menu(Box::new(Menu::TimeTable(Box::new(TimeTable {
            title_font,
            data_font,
            black_font,
            big_font,
            bt_lap_next_page: id,
            bt_lap_previous_page: id,
            bt_print: id,
            last_driver_index: -1,
            current_lap_page: 0,
            session_id: 0,
            update_driver: false,
            max_rows: 16,
            laps_buttons_top: 0.0,
            table_width: 486.0,
            update_selected_best: false,
            selected_laps: Vec::new(),
            selected_lap_count: 0,
            selected_best_splits: [0; 3],
            selected_bestlap: 0,
            best_splits: [0; 3],
            bestlap: 0,
            player_splits: [0; 3],
            player_bestlap: 0,
            skin_textures: BTreeMap::new(),
            name_x: 0.0,
            car_x: 0.0,
            bestlap_x: 0.0,
            split_x: [0.0; 3],
            lapcount_x: 0.0,
            compound_x: 0.0,
            laptime_x: 0.0,
            lap_split_x: [0.0; 3],
            delta_x: 0.0,
        }))));
        let next = self.time_table_page_button(painter, id, "BUT_LAP_NEXT_PAGE", "content/gui/raceControl/right_icon", true);
        let previous = self.time_table_page_button(painter, id, "BUT_LAP_PREV_PAGE", "content/gui/raceControl/left_icon", false);
        let skins: Vec<(usize, String)> = self.sim.race.cars.iter().enumerate().map(|(i, car)| (i, format!("content/cars/{}/skins/{}/livery.png", car.unix_name, car.skin))).collect();
        for (car, path) in skins {
            let texture = painter.get_texture(&path);
            self.time_table_mut(id).skin_textures.insert(car, texture);
        }
        let print = self.new_control(painter, "BUT_PRINT");
        self.set_text(print, &translate("PRINT"));
        self.controls[print].font = Some(title_font);
        self.controls[print].font_align = ALIGN_CENTER;
        self.set_size(print, 80.0, 18.0);
        self.controls[print].back_color = COL_TITLE_BAR;
        self.add_control(id, print);
        self.controls[print].ev_clicked.push(Rc::new(move |gui: &mut Gui, _ev: &Clicked| {
            gui.time_table_print(id);
        }));
        let table = self.time_table_mut(id);
        table.bt_lap_next_page = next;
        table.bt_lap_previous_page = previous;
        table.bt_print = print;
        id
    }

    /// `RaceManager::getLaps(car, false)`: the running session's laps of a car.
    fn time_table_live_laps(&self, car: usize) -> Vec<TimeLap> {
        self.sim.time.laps.get(car).cloned().unwrap_or_default()
    }

    /// `EscMenuTimeTable::refreshSession` 0x1400972f0: the bests of a session that is over.
    pub fn time_table_refresh_session(&mut self, id: Id) {
        let session_id = self.time_table(id).session_id;
        let history = usize::try_from(session_id).ok().and_then(|i| self.sim.time.history.get(i)).cloned().unwrap_or_default();
        let current = self.sim.race.current_session_index;
        let table = self.time_table_mut(id);
        table.best_splits = [0; 3];
        table.bestlap = 0;
        table.update_driver = true;
        table.selected_lap_count = 0;
        table.update_selected_best = true;
        table.selected_best_splits = [0; 3];
        table.player_splits = [0; 3];
        table.selected_bestlap = 0;
        table.player_bestlap = 0;
        table.current_lap_page = 0;
        if session_id != current {
            for (i, laps) in history.laps.iter().enumerate() {
                if let Some(best) = history.best_laps.get(i) {
                    if (table.bestlap == 0 || best.time < table.bestlap) && best.is_valid {
                        table.bestlap = best.time;
                        if i == 0 {
                            table.player_bestlap = table.bestlap;
                        }
                    }
                }
                for lap in laps.iter().filter(|lap| lap.is_valid) {
                    // (a track of more than three sectors writes past the three in the game)
                    for (s, split) in lap.splits.iter().enumerate().take(3) {
                        if table.best_splits[s] == 0 || *split < table.best_splits[s] {
                            table.best_splits[s] = *split;
                            if i == 0 {
                                table.player_splits[s] = *split;
                            }
                        }
                    }
                }
            }
        }
    }

    /// `EscMenuTimeTable::refreshPlayer` 0x140096f60: the player's bests (only when the page
    /// or the menu comes into view).
    pub fn time_table_refresh_player(&mut self, id: Id) {
        let session_id = self.time_table(id).session_id;
        let laps = if session_id == self.sim.race.current_session_index {
            self.time_table_live_laps(0)
        } else {
            usize::try_from(session_id).ok().and_then(|i| self.sim.time.history.get(i)).and_then(|h| h.laps.first()).cloned().unwrap_or_default()
        };
        let table = self.time_table_mut(id);
        for lap in &laps {
            if (table.player_bestlap == 0 || lap.time < table.player_bestlap) && lap.is_valid {
                table.player_bestlap = lap.time;
            }
            for s in 0..3 {
                if s < lap.splits.len() && (table.player_splits[s] == 0 || lap.splits[s] < table.player_splits[s]) && lap.is_valid {
                    table.player_splits[s] = lap.splits[s];
                }
            }
        }
    }

    /// `EscMenuTimeTable::onVisibleChanged` 0x140096ef0.
    pub(crate) fn time_table_on_visible_changed(&mut self, id: Id, visible: bool) {
        if visible {
            self.time_table_refresh_session(id);
            self.time_table_refresh_player(id);
        }
        self.control_on_visible_changed(id, visible);
    }

    /// PRINT (0x1400959c0): every lap of the table as a CSV file, in rustyAC's own `out`
    /// folder (the game's is in Documents). Returns the file.
    pub fn time_table_print(&mut self, id: Id) -> Option<PathBuf> {
        let table = self.time_table(id);
        let focused = self.sim.time.focused_car;
        let session = self.sim.race.session_info(table.session_id).name;
        let unix_name = self.sim.race.cars.get(focused).map(|car| car.unix_name.clone()).unwrap_or_default();
        let name = format!("{session}_{unix_name}_{focused}_{}_{}_{}.csv", self.setup.track_name, self.sim.time.track_config, self.sim.time.time_stamp);
        let mut text = String::from("LAP;TYRES;LAPTIME;VALIDITY;SPLIT1;SPLIT2;SPLIT3\r\n");
        for (n, lap) in table.selected_laps.iter().enumerate() {
            text.push_str(&format!("{};{};{};{};", n + 1, lap.compound, lap.time, lap.is_valid as u8));
            for split in &lap.splits {
                text.push_str(&format!("{split};"));
            }
            text.push_str("\r\n");
        }
        let folder = self.sim.time.out_dir.clone();
        if folder.as_os_str().is_empty() {
            return None;
        }
        let _ = std::fs::create_dir_all(&folder);
        let path = folder.join(name);
        std::fs::write(&path, text).ok().map(|()| path)
    }

    /// `EscMenuTimeTable::render` 0x140097660.
    pub(crate) fn time_table_render(&mut self, id: Id, dt: f32, painter: &mut dyn Painter) {
        let gl = GRAPHICS_GL;
        let mut r = Rect { left: 0.0, right: 1.0, top: 0.0, bottom: 1.0 };
        self.get_world_rect(id, &mut r);
        let own = self.controls[id].rect;
        let (x0, y0) = (r.left + (own.left + 20.0), r.top + (own.top + 8.0));
        let first_render = self.time_table(id).name_x == 0.0;
        if first_render {
            self.time_table_mut(id).max_rows = cut(((own.bottom - own.top) - 287.5) * f32::from_bits(0x3d06_4b8a));
        }
        let current = self.sim.race.current_session_index;
        if current < self.time_table(id).session_id {
            self.time_table_mut(id).session_id = current;
            self.time_table_refresh_session(id);
        }
        let focused = self.sim.time.focused_car;
        let session_id = self.time_table(id).session_id;
        let live = session_id == current;
        let selected = if live {
            Some(self.time_table_live_laps(focused))
        } else {
            usize::try_from(session_id).ok().and_then(|i| self.sim.time.history.get(i)).and_then(|h| h.laps.get(focused)).cloned()
        };
        let (global_best, global_splits, sectors) = (self.sim.race.global_best_lap, self.sim.race.best_splits.clone(), self.sim.time.sector_count);
        let display_name = self.sim.race.cars.get(focused).map(|car| car.driver_name.clone()).unwrap_or_default();
        let player_name = self.sim.race.cars.first().map(|car| car.driver_name.clone()).unwrap_or_default();
        let session_name: String = self.sim.race.session_info(session_id).name.chars().map(|c| c.to_ascii_uppercase()).collect();
        {
            let table = self.time_table_mut(id);
            if table.last_driver_index != focused as i32 {
                table.update_driver = true;
                table.selected_lap_count = 0;
                table.update_selected_best = true;
                table.last_driver_index = focused as i32;
                table.selected_best_splits = [0; 3];
                table.selected_bestlap = 0;
                table.current_lap_page = 0;
            }
            if let Some(laps) = selected {
                table.selected_laps = laps;
            }
            // the selected driver's bests, from the laps that are new
            let n = table.selected_laps.len();
            if table.selected_lap_count as i64 != n as i64 {
                table.update_selected_best = true;
            }
            if table.update_driver || table.update_selected_best {
                for k in (table.selected_lap_count.max(0) as usize)..n {
                    let lap = table.selected_laps[k].clone();
                    if (table.selected_bestlap == 0 || lap.time < table.selected_bestlap) && lap.is_valid {
                        table.selected_bestlap = lap.time;
                    }
                    for s in 0..3 {
                        if s < lap.splits.len() && (table.selected_best_splits[s] == 0 || lap.splits[s] < table.selected_best_splits[s]) && lap.is_valid {
                            table.selected_best_splits[s] = lap.splits[s];
                        }
                    }
                }
                table.selected_lap_count = n as i32;
                table.update_driver = false;
                table.update_selected_best = false;
            }
            if live {
                table.bestlap = global_best;
                for i in 0..3 {
                    if i < sectors {
                        table.best_splits[i] = global_splits.get(i).copied().unwrap_or(0);
                    }
                }
            }
            if first_render {
                table.name_x = x0 + 10.0;
                table.car_x = table.name_x + 180.0;
                table.split_x[0] = table.car_x + 80.0;
                table.split_x[1] = table.split_x[0] + 60.0;
                table.split_x[2] = table.split_x[1] + 60.0;
                table.bestlap_x = table.split_x[2] + 80.0;
            }
        }
        let t = self.time_table(id);
        let (title_font, data_font, black_font, big_font, width) = (t.title_font, t.data_font, t.black_font, t.big_font, t.table_width);
        let (name_x, car_x, split_x, bestlap_x) = (t.name_x, t.car_x, t.split_x, t.bestlap_x);
        let color = |painter: &mut dyn Painter, c: [f32; 4]| painter.gl_color4f(gl, c[0], c[1], c[2], c[3]);
        // the title band
        color(painter, COL_TITLE_BAR);
        painter.gl_quad(gl, x0, y0, width, 35.0, false);
        self.fonts[big_font].blit_string(painter, x0 + 10.0, y0, &display_name, 1.0, ALIGN_LEFT);
        self.fonts[big_font].blit_string(painter, (x0 + width) - 10.0, y0, &translate(&session_name), 1.0, ALIGN_RIGHT);
        // the first header
        color(painter, COL_TITLE_BAR);
        painter.gl_quad(gl, x0, 55.0, width, 20.0, false);
        self.fonts[title_font].set_color(1.0, 1.0, 1.0, 1.0);
        for (text, x, align) in [("DRIVER", name_x, ALIGN_LEFT), ("CAR", car_x, ALIGN_LEFT), ("S1", split_x[0], ALIGN_RIGHT), ("S2", split_x[1], ALIGN_RIGHT), ("S3", split_x[2], ALIGN_RIGHT), ("BESTLAP", bestlap_x, ALIGN_RIGHT)] {
            self.fonts[title_font].blit_string(painter, x, 56.0, &translate(text), 1.0, align);
        }
        let mut y = 55.0f32 + 20.0;
        // the focused driver's, the player's and the session's bests
        for i in 0..3 {
            let text_y = y + 5.0;
            if i % 2 == 0 {
                color(painter, COL_DARK_BAR);
            }
            color(painter, COL_DARK_TRANSP);
            painter.gl_quad(gl, x0, y, width, 30.5, false);
            let t = self.time_table(id);
            let (row_color, font, name, car, splits, best) = match i {
                0 => (COL_DARK_TRANSP, data_font, Some(display_name.clone()), Some(focused), t.selected_best_splits, t.selected_bestlap),
                1 => (COL_DARK_BAR, data_font, Some(player_name.clone()), Some(0), t.player_splits, t.player_bestlap),
                _ => ([1.0, 0.0, 1.0, 0.7], black_font, None, None, t.best_splits, t.bestlap),
            };
            color(painter, row_color);
            painter.gl_quad(gl, x0, y, width, 30.5, false);
            if i == 1 {
                self.fonts[data_font].set_color(1.0, 1.0, 0.0, 1.0);
            }
            if let (Some(name), Some(car)) = (name, car) {
                self.fonts[font].blit_string(painter, name_x, text_y, &name, 1.0, ALIGN_LEFT);
                let texture = self.time_table_mut(id).skin_textures.entry(car).or_default().clone();
                painter.set_texture(0, &texture);
                painter.gl_color4f(gl, 1.0, 1.0, 1.0, 1.0);
                painter.gl_quad(gl, car_x, y + 3.0, 21.0, 21.0, true);
            }
            for k in 0..3 {
                self.fonts[font].blit_string(painter, split_x[k], text_y, &time_to_sector_string(splits[k] as i32, 3), 1.0, ALIGN_RIGHT);
            }
            self.fonts[font].blit_string(painter, bestlap_x, text_y, &time_to_string3(best as i32), 1.0, ALIGN_RIGHT);
            y += 30.5;
        }
        self.fonts[data_font].set_color(1.0, 1.0, 1.0, 1.0);
        y += 20.0;
        if first_render {
            let table = self.time_table_mut(id);
            table.lapcount_x = x0 + 30.0;
            table.compound_x = table.lapcount_x + 50.0;
            table.laptime_x = table.compound_x + 110.0;
            table.lap_split_x[0] = table.laptime_x + 80.0;
            table.lap_split_x[1] = table.lap_split_x[0] + 60.0;
            table.lap_split_x[2] = table.lap_split_x[1] + 60.0;
            table.delta_x = table.lap_split_x[2] + 80.0;
        }
        let t = self.time_table(id);
        let (lapcount_x, compound_x, laptime_x, lap_split_x, delta_x) = (t.lapcount_x, t.compound_x, t.laptime_x, t.lap_split_x, t.delta_x);
        let (laps, best_splits, bestlap, selected_best_splits, selected_bestlap) = (t.selected_laps.clone(), t.best_splits, t.bestlap, t.selected_best_splits, t.selected_bestlap);
        let (page, max_rows) = (t.current_lap_page, t.max_rows);
        let text_y = y + 1.0;
        color(painter, COL_TITLE_BAR);
        painter.gl_quad(gl, x0, y, width, 20.0, false);
        self.fonts[title_font].set_color(1.0, 1.0, 1.0, 1.0);
        for (text, x, align) in [("LAP", lapcount_x, ALIGN_RIGHT), ("TYRE", compound_x, ALIGN_CENTER), ("LAPTIME", laptime_x, ALIGN_RIGHT), ("S1", lap_split_x[0], ALIGN_RIGHT), ("S2", lap_split_x[1], ALIGN_RIGHT), ("S3", lap_split_x[2], ALIGN_RIGHT), ("Diff", delta_x, ALIGN_RIGHT)] {
            self.fonts[title_font].blit_string(painter, x, text_y, &translate(text), 1.0, align);
        }
        y += 20.0;
        // the laps of the page
        let first = page.wrapping_mul(max_rows);
        let end = (page + 1).wrapping_mul(max_rows);
        if (first as i64) < laps.len() as i64 && first >= 0 {
            let mut i = first;
            loop {
                let text_y = y + 5.0;
                if i == end {
                    break;
                }
                let lap = &laps[i as usize];
                color(painter, if i % 2 == 0 { COL_DARK_BAR } else { COL_DARK_TRANSP });
                painter.gl_quad(gl, x0, y, width, 30.5, false);
                self.fonts[data_font].blit_string(painter, lapcount_x, text_y, &(i + 1).to_string(), 1.0, ALIGN_RIGHT);
                self.fonts[data_font].blit_string(painter, compound_x, text_y, &lap.compound, 1.0, ALIGN_RIGHT);
                let time = time_to_string3(lap.time as i32);
                let mark = if !lap.is_valid && lap.time != 0 {
                    Some([1.0, 0.0, 0.0, 0.7])
                } else if bestlap == lap.time && bestlap != 0 {
                    Some([1.0, 0.0, 1.0, 0.7])
                } else if selected_bestlap == lap.time && selected_bestlap != 0 {
                    Some([0.0, 1.0, 0.0, 0.7])
                } else {
                    None
                };
                match mark {
                    Some(c) => {
                        color(painter, c);
                        painter.gl_quad(gl, laptime_x - 74.0, text_y, 76.0, 18.0, false);
                        self.fonts[black_font].blit_string(painter, laptime_x, text_y, &time, 1.0, ALIGN_RIGHT);
                    }
                    None => self.fonts[data_font].blit_string(painter, laptime_x, text_y, &time, 1.0, ALIGN_RIGHT),
                }
                for s in 0..3 {
                    let x = lap_split_x[s];
                    if s < lap.splits.len() {
                        let fv = lap.splits[s].min(999_999) as f32;
                        let mark = if best_splits[s] as f32 == fv && best_splits[s] != 0 {
                            Some([1.0, 0.0, 1.0, 0.7])
                        } else if selected_best_splits[s] as f32 == fv && selected_best_splits[s] != 0 {
                            Some([0.0, 1.0, 0.0, 0.7])
                        } else {
                            None
                        };
                        match mark {
                            Some(c) => {
                                color(painter, c);
                                painter.gl_quad(gl, x - 54.0, text_y, 56.0, 18.0, false);
                                self.fonts[black_font].blit_string(painter, x, text_y, &time_to_sector_string(cut(fv), 3), 1.0, ALIGN_RIGHT);
                            }
                            None if fv == 0.0 => self.fonts[data_font].blit_string(painter, x, text_y, "---", 1.0, ALIGN_RIGHT),
                            None => self.fonts[data_font].blit_string(painter, x, text_y, &time_to_sector_string(cut(fv), 3), 1.0, ALIGN_RIGHT),
                        }
                    } else {
                        self.fonts[data_font].blit_string(painter, x, text_y, "---", 1.0, ALIGN_RIGHT);
                    }
                }
                // against the valid lap before
                if i != 0 && lap.is_valid {
                    if let Some(before) = laps[..i as usize].iter().rev().find(|lap| lap.is_valid) {
                        self.fonts[data_font].blit_string(painter, delta_x, text_y, &time_to_diff_string(lap.time.wrapping_sub(before.time) as i32, 3), 1.0, ALIGN_RIGHT);
                    }
                }
                y += 30.5;
                i += 1;
                if i as i64 as u64 >= laps.len() as u64 {
                    break;
                }
            }
        }
        // the bar under the table, with the buttons
        let table = self.time_table_mut(id);
        table.laps_buttons_top = if y > table.laps_buttons_top { y } else { table.laps_buttons_top };
        let top = table.laps_buttons_top;
        let (next, previous, print) = (table.bt_lap_next_page, table.bt_lap_previous_page, table.bt_print);
        let pages = (max_rows as i64 as u64) < laps.len() as u64;
        self.set_visible(next, pages);
        self.set_visible(previous, pages);
        self.set_position(next, width - 20.0, top + 5.0);
        self.set_position(previous, ((width - 20.0) - 40.0) - 10.0, top + 5.0);
        color(painter, COL_DARK_TRANSP);
        painter.gl_quad(gl, x0, top, width, 50.0, false);
        let p = self.controls[print].rect;
        self.set_position(print, (width - (p.right - p.left)) * 0.5 + 20.0, top + 15.0);
        self.control_render(id, dt, painter);
    }
}
