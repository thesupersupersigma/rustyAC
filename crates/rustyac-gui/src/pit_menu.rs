// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! The pit stop on the screen: the looks of `QuickMenuPitstop` (0x190 bytes, the menu the arrow
//! keys drive) and the panel `PitStop::renderHUD` shows while the stop is worked on.
//!
//! `QuickMenuPitstop::QuickMenuPitstop` 0x1400aca70 (the looks' keys of `system/cfg/
//! pitstop.ini`), `buildControls` 0x1400ae7b0, `buildItem` 0x1400aff20, `addHeader`
//! 0x1400ae6f0, `QuickMenuPitstopItem::setTop` 0x1400b20d0, `updateUI` 0x1400b2140, `setLabels`
//! 0x1400b16d0, `renderHUD` 0x1400b1230; `PitStop::renderHUD` 0x1400aa300 (its "operating"
//! state), `getPitStopTyrePercentage` 0x1400a3070.
//!
//! What the menu holds and does (its items, the presets' values, the keys) is the port of Task
//! 28b: `rustyac_physics::race::pitstop::QuickMenu`. This file gives it the game's labels.

use rustyac_physics::data::ini::IniReader;
use rustyac_physics::race::pitstop::{QuickMenu, ITEM_FUEL, ITEM_REPAIR, ITEM_TYRE_COMPOUND, ITEM_TYRE_PRESSURE, ITEM_WING};

use crate::control::Id;
use crate::font::{Font, FontType};
use crate::gui::Gui;
use crate::painter::{Painter, Texture, ALIGN_CENTER, GRAPHICS_GL};

/// The keys of `system/cfg/pitstop.ini [SETTINGS]` that are the menu's looks.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PitMenuLooks {
    /// the file was there (without it the menu has no fonts of its own)
    pub ready: bool,
    pub size_mult: f32,
    pub header_height: f32,
    pub header_font: f32,
    /// `INTERLINE`: never read from the file
    pub interline: f32,
    pub item_height: f32,
    pub item_title_width: f32,
    pub item_value_width: f32,
    pub object_pos_x: f32,
    pub object_pos_y: f32,
    pub form_alignment_h: i32,
    pub form_alignment_v: i32,
    pub foreground_unselected: [f32; 4],
    pub background_unselected: [f32; 4],
    pub foreground_selected: [f32; 4],
    pub background_selected: [f32; 4],
    pub foreground_header: [f32; 4],
    pub background_header: [f32; 4],
    pub font_is_bold: bool,
}

impl PitMenuLooks {
    /// The constructor's values, then the file's: once the file is there every key is read,
    /// and a missing one is 0.
    pub fn from_ini(ini: Option<&IniReader>) -> PitMenuLooks {
        let mut looks = PitMenuLooks {
            ready: false,
            size_mult: 1.0,
            header_height: 15.0,
            header_font: 10.0,
            interline: 1.0,
            item_height: 20.0,
            item_title_width: 100.0,
            item_value_width: 100.0,
            object_pos_x: 0.0,
            object_pos_y: 0.0,
            form_alignment_h: 2,
            form_alignment_v: 1,
            foreground_unselected: [0.0; 4],
            background_unselected: [0.0; 4],
            foreground_selected: [0.0; 4],
            background_selected: [0.0; 4],
            foreground_header: [0.0; 4],
            background_header: [0.0; 4],
            font_is_bold: false,
        };
        let Some(ini) = ini.filter(|ini| ini.ready) else { return looks };
        let float = |key: &str| ini.get_float("SETTINGS", key).unwrap_or(0.0);
        let int = |key: &str| ini.get_int("SETTINGS", key).unwrap_or(0);
        let four = |key: &str| ini.get_float4("SETTINGS", key).unwrap_or([0.0; 4]);
        looks.ready = true;
        looks.size_mult = float("SIZE_MULT");
        looks.header_height = float("HEADER_HEIGHT");
        looks.header_font = float("HEADER_FONT");
        looks.object_pos_x = float("OBJECT_POS_X");
        looks.object_pos_y = float("OBJECT_POS_Y");
        looks.item_height = float("ITEM_HEIGHT") * looks.size_mult;
        looks.item_title_width = float("ITEM_TITLE_WIDTH") * looks.size_mult;
        looks.item_value_width = float("ITEM_VALUE_WIDTH") * looks.size_mult;
        looks.foreground_unselected = four("ITEM_FOREGROUND_UNSELECTED");
        looks.background_unselected = four("ITEM_BACKGROUND_UNSELECTED");
        looks.foreground_selected = four("ITEM_FOREGROUND_SELECTED");
        looks.background_selected = four("ITEM_BACKGROUND_SELECTED");
        looks.foreground_header = four("ITEM_FOREGROUND_HEADER");
        looks.background_header = four("ITEM_BACKGROUND_HEADER");
        looks.form_alignment_h = int("FORM_ALIGNMENT_H");
        looks.form_alignment_v = int("FORM_ALIGNMENT_V");
        looks.font_is_bold = int("FONT_IS_BOLD") != 0;
        looks
    }
}

/// What the menu's labels say of the car and the session besides the menu's own values.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PitMenuCar {
    /// `CarAvatar::raceEngineer` is there (the car has its physics)
    pub has_race_engineer: bool,
    /// `CarAvatar::getTyreCompound(index, false)` of every compound
    pub compounds: Vec<String>,
    /// `CarAvatar::getWingAngle` by the wing's number (`componentIndex`)
    pub wing_angle: Vec<f32>,
    /// `FuelLapEvaluator::getFuelPerLap`
    pub fuel_per_lap: f32,
    /// `RaceManager::getLapCount` of the car
    pub lap_count: u32,
    /// `physicsState.normalizedSplinePosition`
    pub normalized_spline_position: f32,
    /// `RaceManager::getCurrentSession()`: its type, `isTimedRace`
    pub session_type: i32,
    pub is_timed_race: bool,
    /// `RaceManager::getSessionTotalTime(0)`, ms
    pub session_total_time: i32,
}

/// What `QuickMenuPitstop::renderHUD` asks before it shows the menu.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PitMenuScene {
    pub esc_menu_visible: bool,
    pub pause_menu_visible: bool,
    pub in_replay: bool,
    /// `Sim::endSessionDisplayer` is there and active
    pub end_session_active: bool,
    /// `ACCameraManager::mode`
    pub camera_mode: i32,
    /// `GameScreen::appInteractionEnabled`
    pub app_interaction_enabled: bool,
    pub focused_car_index: i32,
    /// `CarAvatar::isInPitlane` of the player's car
    pub in_pitlane: bool,
}

/// The labels of one item.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PitMenuItemLabels {
    pub title: Id,
    pub value: Id,
    pub header: Option<Id>,
}

/// The looks of `QuickMenuPitstop`: `root` and everything on it.
pub struct PitMenu {
    pub looks: PitMenuLooks,
    pub root: Id,
    pub preset_header: Option<Id>,
    pub presets: Vec<Id>,
    pub items: Vec<PitMenuItemLabels>,
    font: Option<usize>,
    header_font: Option<usize>,
}

/// `toupper` of every unit, in the C locale: only a to z change.
fn upper(text: &str) -> String {
    text.chars().map(|c| c.to_ascii_uppercase()).collect()
}

impl PitMenu {
    /// The constructor's fonts and `buildControls` 0x1400ae7b0 for a menu Task 28b's
    /// `QuickMenu::build` made of the same `setup.ini`: the controls in the game's order, laid
    /// out and put where `FORM_ALIGNMENT_H` / `_V` say on a screen of the painter's size.
    pub fn build(gui: &mut Gui, painter: &mut dyn Painter, looks: PitMenuLooks, menu: &QuickMenu, setup: &IniReader, car: &PitMenuCar) -> PitMenu {
        let (font, header_font) = if looks.ready {
            gui.fonts.push(Font::new(painter, "", FontType::Proportional, 12.0, false, looks.font_is_bold));
            gui.fonts.push(Font::new(painter, "", FontType::Proportional, looks.header_font, false, looks.font_is_bold));
            (Some(gui.fonts.len() - 2), Some(gui.fonts.len() - 1))
        } else {
            (None, None)
        };
        let root = gui.new_control(painter, "FORM_PITSTOP");
        let mut pit = PitMenu { looks, root, preset_header: None, presets: Vec::new(), items: Vec::new(), font, header_font };
        let l = looks;
        if setup.ready {
            let header = gui.new_label(painter, "QM_HEADER_PRESET");
            pit.preset_header = Some(header);
            pit.add_header(gui, header, " PRESETS");
            let r = gui.controls[header].rect;
            let cell = (r.right - r.left) / menu.presets_count as f32;
            for i in 0..menu.presets_count.max(0) {
                let label = gui.new_label(painter, &format!("QM_PRESET_{i}"));
                pit.presets.push(label);
                gui.set_size(label, cell, l.item_height);
                gui.set_position(label, i as f32 * cell, l.interline + l.header_height);
                let c = &mut gui.controls[label];
                c.font = font;
                c.font_align = ALIGN_CENTER;
                c.font_scale = l.size_mult;
                gui.set_text(label, &(i + 1).to_string());
                gui.add_control(root, label);
            }
            // the name the repairs' labels carry: the first wing the file does not have
            let missing_wing = (1..).map(|n| format!("WING_{n}")).find(|section| !setup.has_section(section)).unwrap_or_default();
            let mut first_wing = true;
            for item in &menu.items {
                let (name, title, header) = match item.kind {
                    ITEM_FUEL => (String::new(), Some(" ADD LITERS"), Some(("QM_HEADER_GENERAL", " GENERAL"))),
                    ITEM_TYRE_COMPOUND => (String::new(), Some(" COMPOUND"), Some(("QM_HEADER_TYRE", " TYRES"))),
                    ITEM_TYRE_PRESSURE => (item.setup_name.clone(), None, None),
                    ITEM_WING => (item.setup_name.clone(), None, std::mem::take(&mut first_wing).then_some(("QM_HEADER_WINGS", " WINGS"))),
                    _ => (missing_wing.clone(), Some([" BODY", " SUSPENSION", " ENGINE"][item.component_index.clamp(0, 2) as usize]), (item.component_index == 0).then_some(("QM_HEADER_REPAIR", " REPAIR"))),
                };
                let mut labels = pit.build_item(gui, painter, setup, &name, title.is_none());
                if let Some(title) = title {
                    gui.set_text(labels.title, title);
                }
                if let Some((name, text)) = header {
                    let header = gui.new_label(painter, name);
                    labels.header = Some(header);
                    pit.add_header(gui, header, text);
                }
                pit.items.push(labels);
            }
        }
        // the rows, top to bottom: an item's header stands over it
        let mut y = ((l.interline + l.header_height) + l.item_height) + l.interline;
        for labels in &pit.items {
            if let Some(header) = labels.header {
                gui.set_position(header, 0.0, y);
                y = y + (l.interline + l.header_height);
            }
            for label in [labels.title, labels.value] {
                let left = gui.controls[label].rect.left;
                gui.set_position(label, left, y);
            }
            y = y + (l.item_height + l.interline);
        }
        y = y - (l.item_height + l.interline);
        gui.set_size(root, l.item_value_width + l.item_title_width, y);
        gui.controls[root].draw_border = false;
        gui.controls[root].draw_background = false;
        // where the menu stands on the screen
        let (screen_w, screen_h) = painter.screen();
        let r = gui.controls[root].rect;
        let (width, height) = (r.right - r.left, r.bottom - r.top);
        let fx = match l.form_alignment_h {
            0 => l.object_pos_x,
            1 => (screen_w / 2) as f32 - width * 0.5,
            2 => (screen_w as f32 - l.object_pos_x) - width,
            _ => 0.0,
        };
        let fy = match l.form_alignment_v {
            0 => l.object_pos_y,
            1 => (screen_h / 2) as f32 - height * 0.5,
            2 => (screen_h as f32 - l.object_pos_y) - height,
            _ => 0.0,
        };
        gui.set_position(root, fx, fy);
        pit.update_ui(gui, menu, car);
        pit
    }

    /// `QuickMenuPitstop::addHeader` 0x1400ae6f0.
    fn add_header(&self, gui: &mut Gui, header: Id, text: &str) {
        let l = &self.looks;
        gui.set_size(header, l.item_value_width + l.item_title_width, l.header_height);
        gui.set_position(header, 0.0, 0.0);
        let c = &mut gui.controls[header];
        c.font = self.header_font;
        c.font_scale = l.size_mult;
        c.fore_color = l.foreground_header;
        c.back_color = l.background_header;
        gui.set_text(header, text);
        gui.add_control(self.root, header);
    }

    /// `QuickMenuPitstop::buildItem` 0x1400aff20. `named`: the title is the `NAME` of the
    /// item's section of `setup.ini` (a pressure, a wing).
    fn build_item(&self, gui: &mut Gui, painter: &mut dyn Painter, setup: &IniReader, name: &str, named: bool) -> PitMenuItemLabels {
        let l = &self.looks;
        let title = gui.new_label(painter, &format!("QM_TITLE_{name}"));
        let value = gui.new_label(painter, &format!("QM_VALUE_{name}"));
        gui.set_size(title, l.item_title_width, l.item_height);
        gui.set_position(title, 0.0, 0.0);
        gui.controls[title].font = self.font;
        gui.controls[title].font_scale = l.size_mult;
        if named {
            gui.set_text(title, &upper(&format!(" {}", setup.get_string(name, "NAME"))));
        }
        gui.set_size(value, l.item_value_width, l.item_height);
        gui.controls[value].font_align = ALIGN_CENTER;
        gui.set_position(value, l.item_title_width, 0.0);
        gui.controls[value].font = self.font;
        gui.controls[value].font_scale = l.size_mult;
        gui.add_control(self.root, title);
        gui.add_control(self.root, value);
        PitMenuItemLabels { title, value, header: None }
    }

    /// `QuickMenuPitstop::updateUI` 0x1400b2140: the colours of the preset and the row the
    /// cursor is on, and every row's text.
    pub fn update_ui(&self, gui: &mut Gui, menu: &QuickMenu, car: &PitMenuCar) {
        let l = &self.looks;
        for (j, label) in self.presets.iter().enumerate() {
            let selected = j as i32 == menu.current_preset;
            let c = &mut gui.controls[*label];
            c.back_color = if selected { l.background_selected } else { l.background_unselected };
            c.fore_color = if selected { l.foreground_selected } else { l.foreground_unselected };
        }
        for (k, labels) in self.items.iter().enumerate() {
            let selected = k as i32 == menu.current_index;
            for label in [labels.title, labels.value] {
                let c = &mut gui.controls[label];
                c.back_color = if selected { l.background_selected } else { l.background_unselected };
                c.fore_color = if selected { l.foreground_selected } else { l.foreground_unselected };
            }
        }
        for k in 0..self.items.len() {
            self.set_labels(gui, menu, k, car);
        }
    }

    /// `QuickMenuPitstop::setLabels` 0x1400b16d0: what a row's value says.
    pub fn set_labels(&self, gui: &mut Gui, menu: &QuickMenu, i: usize, car: &PitMenuCar) {
        let Some(preset) = (menu.current_preset >= 0).then(|| menu.presets.get(menu.current_preset as usize)).flatten() else { return };
        let (Some(item), Some(labels), Some(v)) = (menu.items.get(i), self.items.get(i), preset.get(i).copied()) else { return };
        let text = match item.kind {
            ITEM_FUEL => {
                let mut tail = String::new();
                if car.has_race_engineer && (car.session_type.wrapping_sub(1) as u32) <= 2 {
                    let fuel_per_lap = car.fuel_per_lap;
                    if fuel_per_lap > 0.0 {
                        let x = if v > item.max_value {
                            item.max_value
                        } else if v >= 0.0 {
                            v
                        } else {
                            0.0
                        };
                        if car.session_type == 3 && car.is_timed_race && car.lap_count != 0 {
                            let minutes = (car.session_total_time as f32 * f32::from_bits(0x3a83_126f)) * f32::from_bits(0x3c88_8889);
                            let laps = car.lap_count as f32 + car.normalized_spline_position;
                            let per_lap = minutes / laps;
                            let n = ((x / fuel_per_lap) * per_lap) as i32;
                            tail = format!(" (+{n}Mins)");
                        } else {
                            let n = (x / fuel_per_lap) as i32;
                            tail = format!(" (+{n}Laps)");
                        }
                    }
                }
                format!("+{}{tail}", v as i32)
            }
            ITEM_WING => {
                let angle = car.wing_angle.get(item.component_index.max(0) as usize).copied().unwrap_or(0.0) as i32;
                let delta = v as i32;
                if v >= 0.0 {
                    format!("+{delta} ({})", angle + delta)
                } else {
                    format!("{delta} ({})", angle + delta)
                }
            }
            ITEM_TYRE_COMPOUND => {
                if v >= 0.0 {
                    car.compounds.get(v as i32 as usize).cloned().unwrap_or_default()
                } else {
                    "NO CHANGE".to_string()
                }
            }
            ITEM_TYRE_PRESSURE => (v as i32).to_string(),
            ITEM_REPAIR => if v as i32 == 0 { "NO" } else { "YES" }.to_string(),
            _ => return,
        };
        gui.set_text(labels.value, &text);
    }

    /// `QuickMenuPitstop::renderHUD` 0x1400b1230, whole: whether the menu may show, its coming
    /// up by itself on the pit lane, its drawing, and its timer.
    pub fn render_hud(&self, gui: &mut Gui, painter: &mut dyn Painter, dt: f32, menu: &mut QuickMenu, car: &PitMenuCar, scene: &PitMenuScene) {
        let session = car.session_type;
        if scene.esc_menu_visible || scene.pause_menu_visible || scene.in_replay || scene.end_session_active || !(session == 3 || session == 2 || session == 1) || (scene.camera_mode as u32 & 0xffff_fffd) != 0 || !scene.app_interaction_enabled || scene.focused_car_index != 0 {
            menu.timer = 0.0;
            menu.is_visible = false;
            return;
        }
        if menu.auto_app_on_pitlane && scene.in_pitlane {
            menu.is_visible = true;
            menu.timer = menu.visibility_max_time;
            self.update_ui(gui, menu, car);
        }
        if menu.is_visible {
            self.draw(gui, painter, dt, menu, car);
        }
        if menu.timer > 0.0 {
            menu.timer = menu.timer - dt;
        } else {
            menu.is_visible = false;
        }
    }

    /// The drawing of `renderHUD` alone (for whoever keeps the menu's timer elsewhere): the
    /// fuel row's text again, then the labels.
    pub fn draw(&self, gui: &mut Gui, painter: &mut dyn Painter, dt: f32, menu: &QuickMenu, car: &PitMenuCar) {
        if menu.fuel_index >= 0 && (menu.fuel_index as usize) < self.items.len() && gui.controls[self.items[menu.fuel_index as usize].value].visible {
            self.set_labels(gui, menu, menu.fuel_index as usize, car);
        }
        gui.render(self.root, dt, painter);
    }
}

/// `PitStop::colTitleBar`, `PitStop::colDarkTransp`.
pub const COL_TITLE_BAR: [f32; 4] = [0.74, 0.0, 0.0, 1.0];
pub const COL_DARK_TRANSP: [f32; 4] = [0.1, 0.1, 0.1, 0.7];

/// The pictures of `PitStop::renderHUD`.
pub struct PitStopHud {
    pub tx_repairing: Texture,
    pub tx_fueling: Texture,
    pub tx_tyre_changing: Texture,
}

/// `PitStop::getPitStopTyrePercentage` 0x1400a3070 (and its two twins): the share of a job
/// that is still to do, as the stop's clock runs down.
pub fn pit_stop_percentage(pit_timer: f32, max_pit_timer: f32, job_time: f32) -> f32 {
    let x = (pit_timer - (max_pit_timer - job_time)) / job_time;
    if x > 1.0 {
        return 0.0;
    }
    let c = if x >= 0.0 { x } else { 0.0 };
    1.0 - c
}

impl PitStopHud {
    /// The three pictures, as `PitStop::PitStop` 0x1400a55d0 loads them.
    pub fn new(painter: &mut dyn Painter) -> PitStopHud {
        let tx_repairing = painter.get_texture("content/gui/pitstop/operating_repair.png");
        let tx_fueling = painter.get_texture("content/gui/pitstop/operating_fuel.png");
        let tx_tyre_changing = painter.get_texture("content/gui/pitstop/operating_tyres.png");
        PitStopHud { tx_repairing, tx_fueling, tx_tyre_changing }
    }

    /// `PitStop::renderHUD` 0x1400aa300 (without the old mouse menu of its state "choosing"):
    /// nothing while a menu or a replay shows or once the race is over; locked controls leave
    /// the stop's state undefined (5); while the stop is worked on (state 3) the panel is
    /// drawn. `times`: `pitTimer`, `maxPitTimer`, `tyreTime`, `repairTime`, `fuelTime`.
    pub fn render_hud(&self, painter: &mut dyn Painter, scene: &PitMenuScene, race_over: bool, lock_controls_time: f32, state: &mut i32, times: &[f32; 5]) {
        if scene.pause_menu_visible || scene.in_replay || scene.esc_menu_visible || race_over {
            return;
        }
        if 0.0 < lock_controls_time {
            *state = 5;
        }
        if *state == 3 {
            let share = |job: f32| pit_stop_percentage(times[0], times[1], job);
            self.render_operating(painter, share(times[2]), share(times[3]), share(times[4]));
        }
    }

    /// `PitStop::renderHUD` 0x1400aa300 in the state "operating": a dark panel in the middle
    /// of the screen with a picture and a bar for the tyres, the repairs and the fuel.
    /// `tyres`, `repair`, `fuel` are the three percentages.
    pub fn render_operating(&self, painter: &mut dyn Painter, tyres: f32, repair: f32, fuel: f32) {
        let (w, h) = painter.screen();
        let gl = GRAPHICS_GL;
        let size = |painter: &dyn Painter, texture: &Texture| if texture.kid.is_some() { painter.texture_size(texture) } else { (0, 0) };
        painter.gl_color4f(gl, COL_DARK_TRANSP[0], COL_DARK_TRANSP[1], COL_DARK_TRANSP[2], COL_DARK_TRANSP[3]);
        painter.gl_quad(gl, (w / 2 - 0x4b) as f32, (h / 2 - 0x6e) as f32, 150.0, 220.0, false);
        let x1 = (w / 2 - 0x49) as f32;
        let tyre_width = size(painter, &self.tx_tyre_changing).0;
        let bar_x = ((w / 2 - 0x3f).wrapping_add(tyre_width as i32)) as u32 as i64 as f32;
        let mut y = (h / 2 - 0x64) as f32;
        for (i, (texture, share)) in [(&self.tx_tyre_changing, tyres), (&self.tx_repairing, repair), (&self.tx_fueling, fuel)].into_iter().enumerate() {
            if i > 0 {
                y = y + 75.0;
            }
            painter.set_texture(0, texture);
            painter.gl_color4f(gl, 1.0, 1.0, 1.0, 1.0);
            let (tw, th) = size(painter, texture);
            painter.gl_quad(gl, x1, y, tw as f32, th as f32, true);
            painter.gl_color4f(gl, COL_TITLE_BAR[0], COL_TITLE_BAR[1], COL_TITLE_BAR[2], COL_TITLE_BAR[3]);
            painter.gl_quad(gl, bar_x, y + 12.0, (1.0 - share) * 80.0, 20.0, false);
        }
    }
}
