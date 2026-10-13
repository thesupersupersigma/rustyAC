// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! `PitStop` (PitStop.obj, 0x240 bytes): the player's pit stop. A small state machine that
//! waits for the car in its pit box, reads what the pit menu asks for, counts the stop's
//! time down with the controls locked, and then hands the choices to the physics thread
//! (new tyres, pressures, wings, fuel, repairs).
//!
//! As with [`super::RaceManager`], the game's object is a main-thread `GameObject` tied to
//! `Sim`; here it is a state machine that is told what it would read of the player's
//! `CarAvatar` ([`PitCarView`]) and answers with what it would do to the rest of the game
//! ([`PitAction`]), in the game's order. The menu it reads is the game's quick menu
//! (`QuickMenuPitstop`, the one a wheel or pad driver gets): its items and presets are
//! [`QuickMenu`], whatever draws them. The old mouse menu (`Sim::useMousePitstop`) is not
//! ported.

// (a NaN falls through the game's `comiss` + jump as it does here)
#![allow(clippy::neg_cmp_op_on_partial_ord)]

use crate::ai::car_ext::PitStopTime;
use crate::car::RollingChassis;
use crate::data::ini::IniReader;
use crate::math::sqrtf;
use crate::session::MsvcRand;

/// `PitStopState`
pub const STATE_IDLE: i32 = 0;
pub const STATE_WAITING_FOR_DRIVER: i32 = 1;
pub const STATE_CHOOSING: i32 = 2;
pub const STATE_OPERATING: i32 = 3;
pub const STATE_FINISHED: i32 = 4;
pub const STATE_UNDEFINED: i32 = 5;

/// `EQuickMenuPitstopType`
pub const ITEM_FUEL: i32 = 0;
pub const ITEM_WING: i32 = 1;
pub const ITEM_TYRE_COMPOUND: i32 = 2;
pub const ITEM_TYRE_PRESSURE: i32 = 3;
pub const ITEM_REPAIR: i32 = 4;

/// `QuickMenuPitstopItem` (0x78 bytes) without its labels.
#[derive(Clone, Debug, PartialEq)]
pub struct QuickMenuItem {
    /// `type`
    pub kind: i32,
    /// `index`: the item's place in a preset's values
    pub index: i32,
    /// `minValue`, `maxValue`
    pub min_value: f32,
    pub max_value: f32,
    /// `setupName`: the setup item behind it (`PRESSURE_LF`, `WING_1`, `FUEL`, ...)
    pub setup_name: String,
    /// `componentIndex`: the wing, the wheel, or the repair (0 body, 1 suspension, 2 engine)
    pub component_index: i32,
    /// `isFixed`: a fixed setup holds the value
    pub is_fixed: bool,
    /// `pitstopTime`: a wing's seconds
    pub pitstop_time: f32,
    /// what the menu's left column says
    pub title: String,
}

/// `QuickMenuPitstop` (0x190 bytes) without its looks: the items and the presets' values.
#[derive(Clone, Debug, PartialEq)]
pub struct QuickMenu {
    /// `items`
    pub items: Vec<QuickMenuItem>,
    /// `STAY_IN_CAR`
    pub stay_in_car: bool,
    /// `pitstopPresets`: every preset's value of every item
    pub presets: Vec<Vec<f32>>,
    /// `fuelIndex`: where the fuel item is in `items`
    pub fuel_index: i32,
    /// `currentIndex`: the line the cursor is on
    pub current_index: i32,
    /// `currentPreset`
    pub current_preset: i32,
    /// `PRESETS_COUNT`
    pub presets_count: i32,
    /// the compounds' names, for the menu's text
    pub compounds: Vec<String>,
    /// `AUTO_APP_ON_PITLANE`, `VISIBILITY_MAX_TIME`
    pub auto_app_on_pitlane: bool,
    pub visibility_max_time: f32,
    /// `isVisible`, `timer`
    pub is_visible: bool,
    pub timer: f32,
}

/// The keys of `system/cfg/pitstop.ini [SETTINGS]` that change what the menu does (the
/// others are its looks).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct QuickMenuSettings {
    pub stay_in_car: bool,
    pub auto_app_on_pitlane: bool,
    pub presets_count: i32,
    pub visibility_max_time: f32,
}

impl Default for QuickMenuSettings {
    /// The constructor's values, which stay when the file is missing.
    fn default() -> QuickMenuSettings {
        QuickMenuSettings { stay_in_car: false, auto_app_on_pitlane: false, presets_count: 3, visibility_max_time: 3.0 }
    }
}

impl QuickMenuSettings {
    /// `system/cfg/pitstop.ini` as the constructor reads it: once the file is there every
    /// key is read, and a missing one is 0.
    pub fn from_ini(ini: &IniReader) -> QuickMenuSettings {
        if !ini.ready {
            return QuickMenuSettings::default();
        }
        QuickMenuSettings {
            stay_in_car: ini.get_int("SETTINGS", "STAY_IN_CAR").unwrap_or(0) != 0,
            auto_app_on_pitlane: ini.get_int("SETTINGS", "AUTO_APP_ON_PITLANE").unwrap_or(0) != 0,
            presets_count: ini.get_int("SETTINGS", "PRESETS_COUNT").unwrap_or(0),
            visibility_max_time: ini.get_float("SETTINGS", "VISIBILITY_MAX_TIME").unwrap_or(0.0),
        }
    }
}

/// What the menu's item functions read of the player's car.
#[derive(Clone, Debug, PartialEq)]
pub struct QuickMenuCar {
    /// `physicsInfo.maxFuel`
    pub max_fuel: f32,
    /// how many compounds the car has
    pub compounds: usize,
    /// `CarAvatar::getCurrentCompoundStaticPressure` of every wheel
    pub static_pressure: [i32; 4],
    /// `CarAvatar::getWingAngle` of every wing
    pub wing_angle: Vec<f32>,
}

impl QuickMenuCar {
    pub fn of(car: &RollingChassis) -> QuickMenuCar {
        let mut static_pressure = [0; 4];
        for (wheel, slot) in static_pressure.iter_mut().enumerate() {
            if let Some(tyre) = car.tyres.get(wheel) {
                // CarAvatar::getCurrentCompoundStaticPressure @ 0x1400d32b0
                if let Some(def) = (tyre.current_compound_index >= 0).then(|| tyre.compound_defs.get(tyre.current_compound_index as usize)).flatten() {
                    *slot = def.pressure_static as i32;
                }
            }
        }
        let wings = car.aero.as_ref().map_or(0, |aero| aero.base().wings.len());
        QuickMenuCar { max_fuel: car.max_fuel as f32, compounds: car.tyres.first().map_or(0, |tyre| tyre.compound_defs.len()), static_pressure, wing_angle: (0..wings).map(|wing| wing_angle(car, wing as i32)).collect() }
    }
}

/// `CarAvatar::getWingAngle` @ 0x1400d3aa0: a wing with a controller answers with the
/// angle it was given, not the one the controller set.
pub fn wing_angle(car: &RollingChassis, wing: i32) -> f32 {
    let Some(aero) = &car.aero else { return 0.0 };
    aero.base().wings.get(wing.max(0) as usize).map_or(0.0, |w| if w.data.has_controller { w.status.input_angle } else { w.status.angle })
}

impl QuickMenu {
    /// `QuickMenuPitstop::getValue` @ 0x1400b03e0
    pub fn get_value(&self, item: &QuickMenuItem) -> f32 {
        self.presets.get(self.current_preset.max(0) as usize).and_then(|preset| preset.get(item.index.max(0) as usize)).copied().unwrap_or(0.0)
    }

    /// `QuickMenuPitstop::getMaxFuel` @ 0x1400b03c0: the fuel item's `maxValue`
    pub fn get_max_fuel(&self) -> f32 {
        self.items.get(self.fuel_index.max(0) as usize).map_or(0.0, |item| item.max_value)
    }

    /// `QuickMenuPitstop::getWingPitstopTime` @ 0x1400b0400
    pub fn get_wing_pitstop_time(&self, wing: i32) -> f32 {
        self.items.iter().find(|item| item.kind == ITEM_WING && item.component_index == wing).map_or(0.0, |item| item.pitstop_time)
    }

    /// `QuickMenuPitstop::resetControls` @ 0x1400b1420: one item's value in every preset
    pub fn reset_controls(&mut self, index: i32, value: f32) {
        let count = self.presets_count.max(0) as usize;
        for preset in self.presets.iter_mut().take(count) {
            if let Some(slot) = preset.get_mut(index.max(0) as usize) {
                *slot = value;
            }
        }
    }

    /// `QuickMenuPitstop::QuickMenuPitstop` @ 0x1400aca70 (the keys of
    /// `system/cfg/pitstop.ini [SETTINGS]` that change what the menu does) and
    /// `buildControls` @ 0x1400ae7b0: the items a car has, from its `data/setup.ini`, and
    /// every preset at the values the menu starts with.
    pub fn build(car: &QuickMenuCar, compounds: Vec<String>, setup_ini: &IniReader, settings: &QuickMenuSettings) -> QuickMenu {
        let mut menu = QuickMenu {
            items: Vec::new(),
            stay_in_car: settings.stay_in_car,
            presets: Vec::new(),
            fuel_index: 0,
            // (the constructor leaves 0: a menu that came up by itself on the pit lane has its
            // cursor on the fuel; only a key that wakes the menu puts it on the presets' row)
            current_index: 0,
            current_preset: 0,
            presets_count: settings.presets_count,
            compounds,
            auto_app_on_pitlane: settings.auto_app_on_pitlane,
            visibility_max_time: settings.visibility_max_time,
            is_visible: false,
            timer: 0.0,
        };
        let add = |menu: &mut QuickMenu, kind: i32, component_index: i32, setup_name: &str, title: String, min_value: f32, max_value: f32, pitstop_time: f32| {
            let index = menu.items.len() as i32;
            menu.items.push(QuickMenuItem { kind, index, min_value, max_value, setup_name: setup_name.to_string(), component_index, is_fixed: false, pitstop_time, title });
        };
        // QuickMenuPitstop::buildItem @ 0x1400aff20: the name and the limits of a setup.ini section
        let limits = |section: &str| (setup_ini.get_string(section, "NAME").to_uppercase(), setup_ini.get_float(section, "MIN").unwrap_or(0.0), setup_ini.get_float(section, "MAX").unwrap_or(0.0));
        add(&mut menu, ITEM_FUEL, -1, "FUEL", "ADD LITERS".to_string(), 0.0, 0.0, 0.0);
        add(&mut menu, ITEM_TYRE_COMPOUND, -1, "COMPOUND", "COMPOUND".to_string(), -1.0, car.compounds as f32, 0.0);
        for (wheel, section) in ["PRESSURE_LF", "PRESSURE_RF", "PRESSURE_LR", "PRESSURE_RR"].iter().enumerate() {
            if setup_ini.has_section(section) {
                let (name, min, max) = limits(section);
                add(&mut menu, ITEM_TYRE_PRESSURE, wheel as i32, section, name, min, max, 0.0);
            }
        }
        let mut wing = 1;
        loop {
            let section = format!("WING_{wing}");
            if !setup_ini.has_section(&section) {
                break;
            }
            if setup_ini.has_key(&section, "PITSTOP") {
                let (name, min, max) = limits(&section);
                add(&mut menu, ITEM_WING, wing, &section, name, min, max, setup_ini.get_float(&section, "PITSTOP").unwrap_or(0.0));
            }
            wing += 1;
        }
        for (component, title) in ["BODY", "SUSPENSION", "ENGINE"].iter().enumerate() {
            add(&mut menu, ITEM_REPAIR, component as i32, "", title.to_string(), 0.0, 0.0, 0.0);
        }
        for preset in 0..menu.presets_count.max(0) {
            menu.presets.push(vec![0.0; menu.items.len()]);
            menu.current_preset = preset;
            for index in 0..menu.items.len() {
                menu.item_function(0, index, car);
            }
        }
        menu.current_preset = 0;
        menu
    }

    /// The menu of a car, from the `setup.ini` in its data folder.
    pub fn for_car(car: &RollingChassis, settings: &QuickMenuSettings) -> Result<QuickMenu, String> {
        let setup_ini = IniReader::load(&car.data_path.join("setup.ini"))?;
        let compounds = car.tyres.first().map(|tyre| tyre.compound_defs.iter().map(|def| def.name.clone()).collect()).unwrap_or_default();
        Ok(QuickMenu::build(&QuickMenuCar::of(car), compounds, &setup_ini, settings))
    }

    /// An item's own function (`itemFuel` @ 0x1400b0570, `itemCompound` @ 0x1400b04b0,
    /// `itemPressure` @ 0x1400b0660, `itemWing` @ 0x1400b0750, the repairs' lambdas):
    /// `direction` -1 or +1 is a step, 0 the item's starting value.
    pub fn item_function(&mut self, direction: i32, index: usize, car: &QuickMenuCar) {
        let Some(item) = self.items.get(index).cloned() else { return };
        let preset = self.current_preset.max(0) as usize;
        let Some(value) = self.presets.get(preset).and_then(|preset| preset.get(index)).copied() else { return };
        let new = match item.kind {
            ITEM_FUEL => {
                self.fuel_index = index as i32;
                if direction == 0 {
                    self.items[index].min_value = 0.0;
                    self.items[index].max_value = car.max_fuel;
                    0.0
                } else {
                    let mut n = value as i32;
                    if direction > 0 && !(self.items[index].max_value - 1.0 < n as f32) {
                        n += 1;
                    } else if direction < 0 && n > 0 {
                        n -= 1;
                    }
                    n as f32
                }
            }
            ITEM_TYRE_COMPOUND => {
                let mut n = if direction == 0 { -1 } else { value as i32 };
                if direction > 0 && (n as i64) < car.compounds as i64 - 1 {
                    n += 1;
                } else if direction < 0 && n > -1 {
                    n -= 1;
                }
                n as f32
            }
            ITEM_TYRE_PRESSURE => {
                if direction == 0 {
                    car.static_pressure.get(item.component_index.max(0) as usize).copied().unwrap_or(0) as f32
                } else if item.is_fixed {
                    return;
                } else {
                    let mut x = (value as i32) as f32;
                    if direction > 0 && x < item.max_value {
                        x += 1.0;
                    } else if direction < 0 && item.min_value <= x && x != item.min_value {
                        x -= 1.0;
                    }
                    x
                }
            }
            ITEM_WING => {
                if direction == 0 {
                    0.0
                } else if item.is_fixed {
                    return;
                } else {
                    let angle = car.wing_angle.get(item.component_index.max(0) as usize).copied().unwrap_or(0.0);
                    let mut x = (value as i32) as f32;
                    if direction > 0 && x + angle < item.max_value {
                        x += 1.0;
                    } else if direction < 0 && x + angle > item.min_value {
                        x -= 1.0;
                    }
                    x
                }
            }
            _ => {
                if direction == 0 || value as i32 != 0 {
                    0.0
                } else {
                    1.0
                }
            }
        };
        self.presets[preset][index] = new;
    }

    /// `QuickMenuPitstop::sendInput` @ 0x1400b1460: an arrow key (the virtual key codes
    /// 0x25 left, 0x26 up, 0x27 right, 0x28 down). The first press only shows the menu; on
    /// the top line left and right change the preset.
    pub fn send_input(&mut self, key: u32, car: &QuickMenuCar) {
        self.timer = self.visibility_max_time;
        if !self.is_visible {
            self.is_visible = true;
            self.current_index = -1;
            return;
        }
        if self.current_index < 0 {
            match key {
                0x25 if self.current_preset > 0 => self.current_preset -= 1,
                0x27 if (self.current_preset as i64) < self.presets_count as i64 - 1 => self.current_preset += 1,
                0x28 if (self.current_index as i64) < self.items.len() as i64 - 1 => self.current_index += 1,
                _ => {}
            }
        } else {
            let index = self.current_index as usize;
            match key {
                0x25 => self.item_function(-1, index, car),
                0x27 => self.item_function(1, index, car),
                0x26 => self.current_index -= 1,
                0x28 if (self.current_index as i64) < self.items.len() as i64 - 1 => self.current_index += 1,
                _ => {}
            }
        }
    }

    /// What `QuickMenuPitstop::renderHUD` @ 0x1400b1230 does of the menu's coming and
    /// going: on the pit lane it stays up (`AUTO_APP_ON_PITLANE`), else it goes
    /// `VISIBILITY_MAX_TIME` seconds after the last key.
    pub fn frame(&mut self, dt: f32, in_pitlane: bool, allowed: bool) {
        if !allowed {
            self.is_visible = false;
            self.timer = 0.0;
            return;
        }
        if self.auto_app_on_pitlane && in_pitlane {
            self.is_visible = true;
            self.timer = self.visibility_max_time;
        }
        if self.timer > 0.0 {
            self.timer -= dt;
        } else {
            self.is_visible = false;
        }
    }

    /// `SetupPitstopPresets::loadPresets` @ 0x14017a570: a setup's `.sp` file
    /// (`[PRESET_n]` with a key per item: `FUEL`, `COMPOUND`, `PRESSURE_LF`.., `WING_n`),
    /// every value kept between the item's limits.
    pub fn load_presets(&mut self, ini: &IniReader) {
        for preset in 0..self.presets_count.max(0) as usize {
            let section = format!("PRESET_{preset}");
            if !ini.has_section(&section) {
                continue;
            }
            for index in 0..self.items.len() {
                let item = &self.items[index];
                if item.setup_name.is_empty() || !ini.has_key(&section, &item.setup_name) {
                    continue;
                }
                let x = ini.get_float(&section, &item.setup_name).unwrap_or(0.0);
                let value = if !(x <= item.max_value) {
                    item.max_value
                } else if x < item.min_value {
                    item.min_value
                } else {
                    x
                };
                if let Some(slot) = self.presets.get_mut(preset).and_then(|preset| preset.get_mut(index)) {
                    *slot = value;
                }
            }
        }
    }

    /// `SetupPitstopPresets::savePresets` @ 0x14017a8b0: the `.sp` file's text.
    pub fn save_presets(&self) -> String {
        let mut out = String::new();
        for (n, preset) in self.presets.iter().enumerate().take(self.presets_count.max(0) as usize) {
            out.push_str(&format!("[PRESET_{n}]\n"));
            for item in self.items.iter().filter(|item| item.kind != ITEM_REPAIR) {
                out.push_str(&format!("{}={}\n", item.setup_name, super::output::stream_float(preset.get(item.index.max(0) as usize).copied().unwrap_or(0.0))));
            }
            out.push('\n');
        }
        out
    }

    /// One item's value as the menu words it.
    pub fn value_text(&self, item: &QuickMenuItem) -> String {
        let value = self.get_value(item);
        match item.kind {
            ITEM_FUEL => format!("+{} L", value as i32),
            ITEM_WING => format!("{:+}", value as i32),
            ITEM_TYRE_COMPOUND => {
                if value as i32 == -1 {
                    "No change".to_string()
                } else {
                    self.compounds.get(value as i32 as usize).cloned().unwrap_or_else(|| format!("compound {}", value as i32))
                }
            }
            ITEM_TYRE_PRESSURE => format!("{} psi", value as i32),
            _ => if value != 0.0 { "Yes" } else { "No" }.to_string(),
        }
    }

    /// The menu as lines of text: the preset, then every item, the cursor's marked.
    pub fn lines(&self) -> Vec<String> {
        let cursor = if self.current_index < 0 { ">" } else { " " };
        let mut out = vec![format!("{cursor} PIT STOP PRESET {} / {}", self.current_preset + 1, self.presets_count)];
        for (n, item) in self.items.iter().enumerate() {
            let cursor = if n as i32 == self.current_index { ">" } else { " " };
            let fixed = if item.is_fixed { "  (fixed)" } else { "" };
            out.push(format!("{cursor} {:<22} {}{fixed}", item.title, self.value_text(item)));
        }
        out
    }

    /// The presets' values in one line of text (a recording carries the menu like this).
    pub fn values_text(&self) -> String {
        let presets: Vec<String> = self.presets.iter().map(|preset| preset.iter().map(|v| format!("{:08x}", v.to_bits())).collect::<Vec<_>>().join(",")).collect();
        format!("{};{}", self.current_preset, presets.join("/"))
    }

    /// [`QuickMenu::values_text`] back into the menu. Values of items the menu does not
    /// have are left out; a text that is not one changes nothing.
    pub fn set_values_text(&mut self, text: &str) -> bool {
        let Some((preset, values)) = text.split_once(';') else { return false };
        let Ok(preset) = preset.parse::<i32>() else { return false };
        let mut presets = Vec::new();
        for part in values.split('/') {
            let mut row = Vec::new();
            for word in part.split(',').filter(|word| !word.is_empty()) {
                let Ok(bits) = u32::from_str_radix(word, 16) else { return false };
                row.push(f32::from_bits(bits));
            }
            presets.push(row);
        }
        if presets.len() != self.presets.len() || presets.iter().zip(&self.presets).any(|(a, b)| a.len() != b.len()) {
            return false;
        }
        self.presets = presets;
        self.current_preset = preset.clamp(0, (self.presets_count - 1).max(0));
        true
    }
}

/// `PitStopStart`: what `evPitStopCommit` tells (the pit crew listens).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PitStopStart {
    pub tyre_change_time: f32,
    pub total_time: f32,
    pub force_change_camera: bool,
}

/// What the physics thread is handed when the countdown ends (the capture of the lambda
/// @ 0x1400a7da0 and what it reads of `PitStop`).
#[derive(Clone, Debug, PartialEq)]
pub struct PitApply {
    pub new_compound: i32,
    pub psi: Vec<f32>,
    pub wings: Vec<(i32, f32)>,
    pub final_fuel: f32,
    pub repair_body: bool,
    pub repair_suspension: bool,
    pub repair_engine: bool,
}

/// What the stop does to the rest of the game.
#[derive(Clone, Debug, PartialEq)]
pub enum PitAction {
    /// `CarAvatar::setControlsLock` of the player's car (a job on the physics thread)
    ControlsLock(bool),
    /// `evPitStopCommit`: the stop begins
    Commit(PitStopStart),
    /// `evPitStopOver`
    Over,
    /// the choices, for the physics thread ([`apply`])
    Apply(PitApply),
}

/// What `PitStop` reads of the player's `CarAvatar` in a frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PitCarView {
    /// `CarAvatar::isBlackFlagged`
    pub is_black_flagged: bool,
    /// `CarAvatar::isInPitlane()`: a tyre on a pit-lane surface
    pub is_in_pitlane: bool,
    /// `bodyMatrix`'s place
    pub position: [f32; 3],
    /// `physicsState.speed`, m/s
    pub speed: f32,
    /// `physicsState.fuel`, litres
    pub fuel: f32,
}

impl PitCarView {
    pub fn of(car: &RollingChassis) -> PitCarView {
        let p = car.core.get_position(car.body);
        PitCarView { is_black_flagged: car.black_flagged, is_in_pitlane: car.is_in_pit_lane(), position: [p.x, p.y, p.z], speed: car.speed, fuel: car.fuel as f32 }
    }
}

/// What else of the game a frame of the stop looks at.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PitFrame {
    /// the frame's length, s
    pub dt: f32,
    /// `RaceManager::getCurrentSessionType()`
    pub session_type: i32,
    /// `RaceManager::isRaceOver(0)`
    pub race_over: bool,
    /// `ReplayManager::isInReplaymode()`
    pub in_replay: bool,
    /// the pause menu is up
    pub pause_menu: bool,
    /// the ESC menu is up
    pub esc_menu: bool,
}

/// `PitStop` without its looks.
#[derive(Clone, Debug, PartialEq)]
pub struct PitStop {
    /// `state`
    pub state: i32,
    /// `displayingPitMenu`
    pub displaying_pit_menu: bool,
    /// `pitTimer`: seconds left
    pub pit_timer: f32,
    /// `maxPitTimer`: the stop's whole time
    pub max_pit_timer: f32,
    /// `fuelTime`, `repairTime`, `tyreTime`
    pub fuel_time: f32,
    pub repair_time: f32,
    pub tyre_time: f32,
    /// `indicatorOnlyInPitline` (`gameplay.ini [PIT_INDICATOR] ONLY_IN_PITLINE`)
    pub indicator_only_in_pitline: bool,
    /// the pit indicator's visibility byte
    pub indicator_visible: bool,
    /// `pitPosition`: the player's pit box
    pub pit_position: [f32; 3],
    /// `newCompound` (-1: the tyres stay)
    pub new_compound: i32,
    /// `wings`: (wing, clicks asked for)
    pub wings: Vec<(i32, f32)>,
    /// `psi`
    pub psi: Vec<f32>,
    /// `getFuel`: litres to add
    pub get_fuel: f32,
    /// `rbody`, `rsusp`, `rengine`
    pub rbody: bool,
    pub rsusp: bool,
    pub rengine: bool,
    /// `GameScreen::appInteractionEnabled`
    pub app_interaction_enabled: bool,
    /// `CarAvatar::lastPitstopTime` would be written this frame (the countdown ran)
    pub timer_ran: bool,
    /// `strategies`, `currentStrategyIndex`, `pitRandomModifier`: as the constructor leaves
    /// them. No function of the game reads them again (the random part of a stop's time is
    /// the one `rand()` of `Car::getPitstopTime`); they are here for the record.
    pub strategies: [PitStopStrategy; 2],
    pub current_strategy_index: i32,
    pub pit_random_modifier: f32,
    /// for the pit crew, which listens to `evPitStopCommit` and `evPitStopOver` from another
    /// thread: how often each was raised, what the last commit carried and whether the tyres
    /// are changed in that stop (`isChangingTyres` @ 0x1400a9870)
    pub commits: u32,
    pub overs: u32,
    pub last_start: PitStopStart,
    pub last_changing_tyres: bool,
}

/// `PitStopStrategy` (12 bytes; constructor @ 0x1400a7590).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PitStopStrategy {
    pub fuel: f32,
    pub tyre_compound: i32,
    pub fix_body: bool,
    pub fix_engine: bool,
    pub fix_susp: bool,
}

impl Default for PitStopStrategy {
    fn default() -> PitStopStrategy {
        PitStopStrategy { fuel: 30.0, tyre_compound: -1, fix_body: false, fix_engine: false, fix_susp: false }
    }
}

impl PitStop {
    /// `PitStop::PitStop` @ 0x1400a55d0, for the pit box at `pit_position`.
    pub fn new(pit_position: [f32; 3]) -> PitStop {
        PitStop {
            state: STATE_FINISHED,
            displaying_pit_menu: false,
            pit_timer: 0.0,
            max_pit_timer: 10.0,
            fuel_time: 0.0,
            repair_time: 0.0,
            tyre_time: 0.0,
            indicator_only_in_pitline: true,
            indicator_visible: true,
            pit_position,
            new_compound: -1,
            wings: Vec::new(),
            psi: Vec::new(),
            get_fuel: 0.0,
            rbody: false,
            rsusp: false,
            rengine: false,
            app_interaction_enabled: true,
            timer_ran: false,
            strategies: [PitStopStrategy::default(), PitStopStrategy { tyre_compound: 2, ..PitStopStrategy::default() }],
            current_strategy_index: 0,
            pit_random_modifier: 0.0,
            commits: 0,
            overs: 0,
            last_start: PitStopStart { tyre_change_time: 0.0, total_time: 0.0, force_change_camera: false },
            last_changing_tyres: false,
        }
    }

    /// The handler of `Sim::evOnNewSession` (lambda @ 0x1400a7ce0).
    pub fn on_new_session(&mut self, out: &mut Vec<PitAction>) {
        self.state = STATE_UNDEFINED;
        out.push(PitAction::ControlsLock(false));
        self.pit_timer = 0.0;
        self.app_interaction_enabled = true;
    }

    /// `PitStop::isInsidePitArea` @ 0x1400a9af0: less than 3 m from the pit box.
    pub fn is_inside_pit_area(&self, car: &PitCarView) -> bool {
        let dy = self.pit_position[1] - car.position[1];
        let dx = self.pit_position[0] - car.position[0];
        let dz = self.pit_position[2] - car.position[2];
        let squared = (dy * dy + dx * dx) + dz * dz;
        let distance = if squared == 0.0 { 0.0 } else { sqrtf(squared) };
        distance < 3.0
    }

    /// `PitStop::isCarOutOfPitArea` @ 0x1400a9840
    fn is_car_out_of_pit_area(&mut self, car: &PitCarView) -> bool {
        if self.is_inside_pit_area(car) {
            return false;
        }
        self.indicator_visible = true;
        true
    }

    /// `PitStop::isInPitPosition` @ 0x1400a98c0 (the quick menu's way): in the box and
    /// slower than 1.5 km/h; once there, until 2 km/h.
    pub fn is_in_pit_position(&mut self, car: &PitCarView) -> bool {
        let inside = self.is_inside_pit_area(car);
        let mut displaying = false;
        if inside && (car.speed * 3.6 < 1.5 || self.displaying_pit_menu) {
            displaying = true;
        }
        self.displaying_pit_menu = displaying;
        if displaying && !(car.speed * 3.6 < 2.0) {
            self.displaying_pit_menu = false;
        }
        self.displaying_pit_menu
    }

    /// `PitStop::isChangingTyres` @ 0x1400a9870 (the quick menu's way)
    pub fn is_changing_tyres(&self) -> bool {
        self.new_compound > -1
    }

    /// `PitStop::getPitStopFuelingPercentage` @ 0x1400a9730
    pub fn get_fueling_percentage(&self) -> f32 {
        Self::percentage(self.pit_timer, self.max_pit_timer, self.fuel_time)
    }

    /// `PitStop::getPitStopRepairingPercentage` @ 0x1400a9780
    pub fn get_repairing_percentage(&self) -> f32 {
        Self::percentage(self.pit_timer, self.max_pit_timer, self.repair_time)
    }

    fn percentage(timer: f32, max: f32, part: f32) -> f32 {
        let x = (timer - (max - part)) / part;
        if x > 1.0 {
            return 0.0;
        }
        let x = if x >= 0.0 { x } else { 0.0 };
        1.0 - x
    }

    /// `PitStop::update` @ 0x1400ab190: one frame. `chassis` is the player's physics car
    /// (`CarAvatar::getPitstopTime` asks it), `rand` the main thread's `rand()`.
    pub fn update(&mut self, frame: &PitFrame, car: &PitCarView, menu: &mut QuickMenu, chassis: &RollingChassis, rand: &mut MsvcRand, out: &mut Vec<PitAction>) {
        self.timer_ran = false;
        if car.is_black_flagged {
            return;
        }
        if frame.in_replay || !(1..=3).contains(&frame.session_type) || frame.pause_menu {
            self.indicator_visible = false;
            return;
        }
        if frame.race_over || frame.esc_menu {
            self.state = STATE_UNDEFINED;
        }
        match self.state {
            STATE_IDLE => {
                if car.is_in_pitlane {
                    self.indicator_visible = true;
                    self.state = STATE_WAITING_FOR_DRIVER;
                }
            }
            STATE_WAITING_FOR_DRIVER => {
                if self.is_in_pit_position(car) {
                    self.read_menu(car, menu, chassis, rand, out);
                }
                // (the code of Choosing follows in the machine code)
                if !self.is_in_pit_position(car) {
                    self.state = STATE_WAITING_FOR_DRIVER;
                }
            }
            STATE_CHOOSING => {
                if !self.is_in_pit_position(car) {
                    self.state = STATE_WAITING_FOR_DRIVER;
                }
            }
            STATE_OPERATING => {
                if self.is_pit_stop_done(frame.dt, car, menu, out) {
                    out.push(PitAction::Over);
                    self.overs += 1;
                    self.displaying_pit_menu = false;
                    self.state = STATE_FINISHED;
                }
            }
            STATE_FINISHED => {
                self.indicator_visible = false;
                if self.is_car_out_of_pit_area(car) {
                    self.state = STATE_IDLE;
                }
            }
            STATE_UNDEFINED => {
                self.indicator_visible = false;
                self.pit_timer = 0.0;
                if self.is_car_out_of_pit_area(car) {
                    self.state = STATE_IDLE;
                }
            }
            _ => {
                self.indicator_visible = false;
                self.state = STATE_IDLE;
            }
        }
        if !car.is_in_pitlane && self.state != STATE_UNDEFINED {
            self.state = STATE_IDLE;
        }
    }

    /// The quick menu's branch of the state `WaitingForDriver`: the choices are read, the
    /// stop is timed, and it begins (or is over at once when it asks for nothing).
    fn read_menu(&mut self, car: &PitCarView, menu: &mut QuickMenu, chassis: &RollingChassis, rand: &mut MsvcRand, out: &mut Vec<PitAction>) {
        self.psi.clear();
        self.wings.clear();
        for n in 0..menu.items.len() {
            let item = menu.items[n].clone();
            match item.kind {
                ITEM_FUEL => {
                    let room = menu.get_max_fuel() - car.fuel;
                    let value = menu.get_value(&item);
                    self.get_fuel = if value <= room { value } else { room };
                }
                ITEM_WING => {
                    self.wings.push((item.component_index, menu.get_value(&item)));
                    menu.reset_controls(item.index, 0.0);
                }
                ITEM_TYRE_COMPOUND => self.new_compound = menu.get_value(&item) as i32,
                ITEM_TYRE_PRESSURE => self.psi.push(menu.get_value(&item)),
                ITEM_REPAIR => {
                    let wanted = menu.get_value(&item) != 0.0;
                    match item.component_index {
                        0 => self.rbody = wanted,
                        1 => self.rsusp = wanted,
                        2 => self.rengine = wanted,
                        _ => continue,
                    }
                    menu.reset_controls(item.index, 0.0);
                }
                _ => {}
            }
        }
        // CarAvatar::getPitstopTime @ 0x1400d3570
        let fuel = if self.get_fuel > 0.0 { self.get_fuel } else { 0.0 };
        let mut time: PitStopTime = chassis.get_pitstop_time(fuel, self.new_compound != -1, self.rbody || self.rsusp, self.rengine, self.rsusp, true, rand);
        for &(wing, value) in &self.wings {
            if value as i32 != 0 {
                let seconds = menu.get_wing_pitstop_time(wing);
                time.total += seconds;
                time.repair += seconds;
            }
        }
        self.tyre_time = time.tyres;
        self.repair_time = time.repair;
        self.fuel_time = time.fuel;
        self.pit_timer = time.total;
        self.max_pit_timer = time.total;
        if time.total <= 0.0 {
            self.state = STATE_FINISHED;
        } else {
            let start = PitStopStart { tyre_change_time: time.tyres, total_time: time.total, force_change_camera: !menu.stay_in_car };
            out.push(PitAction::Commit(start));
            self.commits += 1;
            self.last_start = start;
            self.last_changing_tyres = self.is_changing_tyres();
            self.state = STATE_OPERATING;
        }
    }

    /// `PitStop::isPitStopDone` @ 0x1400a9b80: the countdown, with the controls locked; at
    /// its end `commitQuickMenuChanges` @ 0x1400a94b0.
    fn is_pit_stop_done(&mut self, dt: f32, car: &PitCarView, menu: &QuickMenu, out: &mut Vec<PitAction>) -> bool {
        out.push(PitAction::ControlsLock(true));
        self.app_interaction_enabled = false;
        self.indicator_visible = false;
        self.timer_ran = true;
        if self.pit_timer > 0.0 {
            self.pit_timer -= dt;
            if self.pit_timer <= 0.0 {
                let max = menu.get_max_fuel();
                let sum = car.fuel + self.get_fuel;
                let final_fuel = if sum > max {
                    max
                } else if sum >= car.fuel {
                    sum
                } else {
                    car.fuel
                };
                out.push(PitAction::Apply(PitApply { new_compound: self.new_compound, psi: self.psi.clone(), wings: self.wings.clone(), final_fuel, repair_body: self.rbody, repair_suspension: self.rsusp, repair_engine: self.rengine }));
                self.app_interaction_enabled = true;
            }
        }
        0.0 >= self.pit_timer
    }

    /// The stop's state in words (what `tools/car_oracle` reads out of the game's own
    /// `PitStop` under the same names).
    pub fn text(&self) -> String {
        format!(
            "state {} menu {} timer {:08x} max {:08x} fuel {:08x} repair {:08x} tyre {:08x} indicator {} compound {} wings [{}] psi [{}] getFuel {:08x} repairs {}{}{} apps {}",
            self.state,
            self.displaying_pit_menu as u8,
            self.pit_timer.to_bits(),
            self.max_pit_timer.to_bits(),
            self.fuel_time.to_bits(),
            self.repair_time.to_bits(),
            self.tyre_time.to_bits(),
            self.indicator_visible as u8,
            self.new_compound,
            self.wings.iter().map(|(wing, value)| format!("{wing}:{:08x}", value.to_bits())).collect::<Vec<_>>().join(" "),
            self.psi.iter().map(|value| format!("{:08x}", value.to_bits())).collect::<Vec<_>>().join(" "),
            self.get_fuel.to_bits(),
            self.rbody as u8,
            self.rsusp as u8,
            self.rengine as u8,
            self.app_interaction_enabled as u8
        )
    }
}

/// `PitStop::getEstimatedTime` @ 0x1400a9600: the seconds a stop would take, without the
/// random part. It is the old mouse menu's (what its buttons ask for: the fuel as a share of
/// the tank, a tyre change, the three repairs; "repair suspension" includes the body).
pub fn get_estimated_time(chassis: &RollingChassis, max_fuel: f32, fuel_to_add_percentage: f32, tyres: bool, repair_body: bool, repair_engine: bool, repair_suspension: bool) -> f32 {
    let fuel = max_fuel * fuel_to_add_percentage;
    // CarAvatar::getPitstopTime @ 0x1400d3570
    let fuel = if fuel > 0.0 { fuel } else { 0.0 };
    chassis.get_pitstop_time(fuel, tyres, repair_body || repair_suspension, repair_engine, repair_suspension, false, &mut MsvcRand(0)).total
}

impl QuickMenu {
    /// The seconds the stop the menu asks for now would take, without the random part: the
    /// quick menu's choices through the same sum as `PitStop::update` makes when the car
    /// stops (and [`get_estimated_time`] makes for the mouse menu).
    pub fn estimate(&self, chassis: &RollingChassis) -> f32 {
        let (mut fuel, mut tyres, mut body, mut suspension, mut engine, mut wings) = (0.0f32, false, false, false, false, 0.0f32);
        for item in &self.items {
            let value = self.get_value(item);
            match item.kind {
                ITEM_FUEL => {
                    let room = self.get_max_fuel() - chassis.fuel as f32;
                    fuel = if value <= room { value } else { room };
                }
                ITEM_WING if value as i32 != 0 => wings += item.pitstop_time,
                ITEM_TYRE_COMPOUND => tyres = value as i32 != -1,
                ITEM_REPAIR => match item.component_index {
                    0 => body = value != 0.0,
                    1 => suspension = value != 0.0,
                    2 => engine = value != 0.0,
                    _ => {}
                },
                _ => {}
            }
        }
        let fuel = if fuel > 0.0 { fuel } else { 0.0 };
        chassis.get_pitstop_time(fuel, tyres, body || suspension, engine, suspension, false, &mut MsvcRand(0)).total + wings
    }
}

/// Made-up states for the pit stop's state machine, for proofs: what `PitStop::update` reads
/// of the player's car and of the menu, drawn frame by frame from a seed. `tools/car_oracle`
/// feeds the game's own `PitStop` from the same stream (the `pit_feed` scenarios).
#[derive(Clone, Debug)]
pub struct PitFeed {
    rand: MsvcRand,
    /// a second stream, for what `getEstimatedTime` is asked
    rand_estimate: MsvcRand,
    /// frames left of the current spell, and what it is: 0 in the box, 1 away, 2 anything
    left: u32,
    spell: u32,
    dt: f32,
    black: bool,
}

/// One frame of a [`PitFeed`].
#[derive(Clone, Debug, PartialEq)]
pub struct Fed {
    pub car: PitCarView,
    pub dt: f32,
    /// a new preset and its values, at the start of a spell
    pub menu: Option<(i32, Vec<f32>)>,
    /// new damage: the five zone levels and the engine's life left
    pub damage: Option<([f32; 5], f64)>,
    /// what the mouse menu's buttons say, for `getEstimatedTime`: the fuel to add as a share
    /// of the tank, a tyre change, repair body, repair engine, repair suspension
    pub estimate: (f32, bool, bool, bool, bool),
}

impl PitFeed {
    pub fn new(seed: u32) -> PitFeed {
        PitFeed { rand: MsvcRand(seed), rand_estimate: MsvcRand(seed ^ 0x2b28), left: 0, spell: 0, dt: 0.009, black: false }
    }

    fn unit(&mut self) -> f32 {
        self.rand.next() as f32 / 32767.0
    }

    fn pick(&mut self, count: u32) -> u32 {
        self.rand.next() as u32 % count.max(1)
    }

    /// The next frame's state, for the pit box at `pit_position` and this menu.
    pub fn next(&mut self, pit_position: [f32; 3], menu: &QuickMenu, max_fuel: f32) -> Fed {
        let mut fed_menu = None;
        let mut damage = None;
        if self.left == 0 {
            self.left = 40 + self.pick(700);
            self.spell = [0, 0, 0, 1, 2][self.pick(5) as usize];
            self.dt = [0.009, 0.009, 0.05, 0.25, 0.003, 1.5][self.pick(6) as usize];
            self.black = self.pick(40) == 0;
            // what the menu asks for
            let preset = self.pick(menu.presets_count.max(1) as u32) as i32;
            let mut values = Vec::new();
            for item in &menu.items {
                let value = match item.kind {
                    ITEM_FUEL => [0.0, 0.0, 1.0, 5.0, 30.0, 500.0][self.pick(6) as usize] + (self.pick(3) as f32) * self.unit() * item.max_value,
                    ITEM_TYRE_COMPOUND => (self.pick(item.max_value.max(0.0) as u32 + 2) as i32 - 1).min(item.max_value as i32 - 1) as f32,
                    ITEM_TYRE_PRESSURE => (item.min_value - 6.0 + self.unit() * (item.max_value - item.min_value + 12.0)).floor(),
                    ITEM_WING => (self.pick(7) as i32 - 3) as f32,
                    _ => (self.pick(3) == 0) as u32 as f32,
                };
                values.push(value);
            }
            fed_menu = Some((preset, values));
            if self.pick(2) == 0 {
                let mut zones = [0.0f32; 5];
                for zone in &mut zones {
                    *zone = [0.0, 0.0, 3.0, 40.0, 99.0, 250.0][self.pick(6) as usize] * self.unit();
                }
                let life = [1000.0, 1000.0, 999.0, 700.0, 0.0, -50.0][self.pick(6) as usize];
                damage = Some((zones, life));
            }
        }
        self.left -= 1;
        // where the car is, how fast, and the rest
        let reach = match self.spell {
            0 => [0.0, 0.5, 2.0, 2.99][self.pick(4) as usize],
            1 => [3.01, 4.0, 30.0][self.pick(3) as usize],
            _ => self.unit() * 6.0,
        };
        let turn = self.unit() * 6.283_185_5;
        let position = [pit_position[0] + reach * turn.cos(), pit_position[1] + 0.1 * (self.unit() - 0.5), pit_position[2] + reach * turn.sin()];
        let speed = match self.pick(if self.spell == 0 { 12 } else { 4 }) {
            0 => 0.41,
            1 => 0.42,
            2 => 0.55,
            3 => 0.56 + self.unit() * 20.0,
            _ => self.unit() * 0.4,
        };
        let fuel = [0.0, 1.0, 0.5 * max_fuel, max_fuel, max_fuel + 3.0][self.pick(5) as usize] + self.unit();
        let is_in_pitlane = self.pick(if self.spell == 1 { 3 } else { 60 }) != 0;
        let dt = if self.pick(25) == 0 { self.unit() * 0.1 } else { self.dt };
        let car = PitCarView { is_black_flagged: self.black && self.pick(3) == 0, is_in_pitlane, position, speed, fuel };
        let e = &mut self.rand_estimate;
        let share = [0.0, 0.0, 1.0, -0.2][e.next() as usize % 4] + e.next() as f32 / 32767.0 * 1.1;
        let estimate = (share, e.next() % 2 == 0, e.next() % 3 == 0, e.next() % 3 == 0, e.next() % 3 == 0);
        Fed { car, dt, menu: fed_menu, damage, estimate }
    }
}

/// The lambda @ 0x1400a7da0, on the physics thread: the stop's choices go into the car.
pub fn apply(car: &mut RollingChassis, choices: &PitApply) {
    if choices.new_compound != -1 {
        const PRESSURES: [&str; 4] = ["PRESSURE_LF", "PRESSURE_RF", "PRESSURE_LR", "PRESSURE_RR"];
        for (wheel, name) in PRESSURES.iter().enumerate() {
            if wheel < car.tyres.len() {
                car.set_tyre_compound(wheel, choices.new_compound);
            }
            // (the game indexes `psi` and the item unchecked: a car without the four
            // pressure items has no compound item either)
            let (Some(&wanted), Some(item)) = (choices.psi.get(wheel), car.setup_manager.get_setup_item(name)) else { continue };
            let item = &mut car.setup_manager.items[item];
            let now = item.new_value;
            let mut value = wanted;
            if wanted > now {
                if wanted - now > 4.0 {
                    value = now + 4.0;
                }
            } else if wanted < now && !(wanted - now >= -4.0) {
                value = now - 4.0;
            }
            item.new_value = value;
        }
    }
    for &(wing, value) in &choices.wings {
        if value as i32 != 0 {
            let name = format!("WING_{wing}");
            // CarAvatar::getWingAngle @ 0x1400d3aa0
            let angle = wing_angle(car, wing);
            if let Some(item) = car.setup_manager.get_setup_item(&name) {
                car.setup_manager.items[item].new_value = (angle as i32 + value as i32) as f32;
            }
        }
    }
    // Car::setRequestedFuel(fuel, false) @ 0x140275cb0
    car.fuel = choices.final_fuel as f64;
    car.fuel_lap_evaluator.ignore_lap = true;
    if choices.repair_engine {
        if let Some(drivetrain) = &mut car.drivetrain {
            drivetrain.engine_mut().reset();
        }
    }
    if choices.repair_body {
        car.set_damage_level(0.0);
    }
    if choices.repair_suspension {
        car.set_damage_level(0.0);
        car.reset_suspension_damage_level();
    }
    if car.penalty_manager.is_serving_pit_penalty {
        car.penalty_manager.rearm_pit_penalty = true;
    }
    car.lock_controls(false);
}
