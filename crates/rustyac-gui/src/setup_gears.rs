// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! `SetupGearManager` (0x208 bytes), the setup editor's page "GEARS": a spinner per gear that
//! picks a line of the gear's `.rto` file (and one for the final ratio), or one spinner that
//! picks a whole gear set; beside them the top speed of every gear.
//!
//! Constructor 0x140170c80, `addSetupItem` 0x140171f60, `populateSpinner` 0x140173810,
//! `addGearSet` 0x140171d00, the spinners' handlers (a gear 0x140171560, the gear set
//! 0x1401713e0), `render` 0x1401743a0, `getStringMaxSpeed` 0x140172de0, `getStringToSave`
//! 0x1401730d0, `loadFromINI` 0x1401734a0, `resetToDefaultValues` 0x1401749c0;
//! `RaceEngineer::getMaxSpeedFromGear` 0x14027c240; the gears' part of `SetupScreen::loadINI`
//! 0x14017d950.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::rc::Rc;

use rustyac_physics::car::setup::{load_gear_ratios, SetupJob};
use rustyac_physics::data::ini::{append_path, IniReader};

use crate::acos::format_g6;
use crate::control::Id;
use crate::font::{Font, FontType};
use crate::gui::Gui;
use crate::hud::translate;
use crate::painter::{Painter, ALIGN_LEFT};
use crate::setup_tab::TabKind;
use crate::widgets::{msvc_floor, WidgetEvent};

/// `GearDef`: a setup item's name and the ratios its spinner picks from.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GearDef {
    pub setup_manager_name: String,
    pub values: Vec<f32>,
}

/// `SetupItemText`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SetupItemText {
    pub display_name: String,
    pub setup_manager_var_name: String,
    pub text_file: PathBuf,
    pub help_key: String,
}

/// `SetupGearManager`'s own members.
#[derive(Default)]
pub struct SetupGears {
    pub gear_sets: BTreeMap<String, Vec<GearDef>>,
    pub gears: BTreeMap<Id, GearDef>,
    pub default_values: Vec<i32>,
    pub gear_set_spinner: Option<Id>,
}

impl Gui {
    fn setup_gears(&self, id: Id) -> &SetupGears {
        match &self.setup_tab(id).kind {
            TabKind::Gears(gears) => gears,
            _ => panic!("control {id} is not the gears page"),
        }
    }

    fn setup_gears_mut(&mut self, id: Id) -> &mut SetupGears {
        match &mut self.setup_tab_mut(id).kind {
            TabKind::Gears(gears) => gears,
            _ => panic!("control {id} is not the gears page"),
        }
    }

    /// `SetupGearManager::SetupGearManager` 0x140170c80.
    pub fn new_setup_gears(&mut self, painter: &mut dyn Painter) -> Id {
        let id = self.new_setup_tab(painter, &translate("GEARS"));
        self.setup_tab_mut(id).kind = TabKind::Gears(Box::default());
        self.control_set_size(id, 450.0, 600.0);
        self.fonts.push(Font::new(painter, "", FontType::Proportional, 12.0, false, false));
        self.controls[id].font = Some(self.fonts.len() - 1);
        id
    }

    /// A setup item's `newValue`, written at once (the gears' handlers do not go through the
    /// physics thread's queue).
    fn setup_gears_write(&mut self, name: &str, value: f32) {
        let Some(car) = &mut self.setup_car else { return };
        if let Some(item) = car.items().iter().position(|item| item.name == name) {
            car.job(SetupJob::SetNewValue { item, value });
        }
    }

    /// The handler of a gear's spinner (0x140171560): the ratio of the spinner's line.
    fn setup_gears_gear_changed(&mut self, id: Id, spinner: Id) {
        self.setup_tab_mut(id).modified_values = true;
        let def = self.setup_gears(id).gears.get(&spinner).cloned().unwrap_or_default();
        let line = self.spinner(spinner).value;
        if let Some(value) = usize::try_from(line).ok().and_then(|line| def.values.get(line)) {
            self.setup_gears_write(&def.setup_manager_name, *value);
        }
    }

    /// The handler of the gear sets' spinner (0x1401713e0): every gear of the chosen set.
    fn setup_gears_set_changed(&mut self, id: Id, spinner: Id) {
        self.setup_tab_mut(id).modified_values = true;
        let name = self.spinner_current_item(spinner);
        let set = self.setup_gears(id).gear_sets.get(&name).cloned().unwrap_or_default();
        for gear in set {
            if let Some(value) = gear.values.first() {
                self.setup_gears_write(&gear.setup_manager_name, *value);
            }
        }
    }

    /// `SetupGearManager::addSetupItem` 0x140171f60: `gear` is 2 for the first gear, or
    /// negative for the final ratio.
    pub fn setup_gears_add_setup_item(&mut self, painter: &mut dyn Painter, id: Id, gear: i32, text: &SetupItemText) {
        let Some(car) = &self.setup_car else { return };
        let max_gear = car.max_gear();
        let sp = self.new_spinner(painter, &text.display_name, true, false);
        self.spinner_set_label_text(sp, &text.display_name);
        self.fonts.push(Font::new(painter, "", FontType::Proportional, 12.0, false, false));
        self.controls[sp].font = Some(self.fonts.len() - 1);
        self.set_size(sp, 184.0, 21.0);
        let n = if gear < 0 { max_gear as u32 } else { (gear - 2) as u32 };
        let y = msvc_floor((n as f32 * 1.5) * 48.0 + 55.0);
        self.spinner_set_position(sp, 135.0, y);
        self.spinner_mut(sp).draw_underline = true;
        let help = self.locale.setup_help_text(&text.help_key);
        if help.is_empty() {
            self.spinner_mut(sp).display_popover = false;
        } else {
            let pop_over = self.spinner(sp).pop_over;
            self.pop_over_set_text(pop_over, &help);
            self.pop_over_set_label_title(pop_over, &text.display_name);
            self.spinner_mut(sp).display_popover = true;
        }
        // populateSpinner 0x140173810: the file's lines; the last that is the car's ratio
        let mut values = Vec::new();
        let current = self.setup_car.as_ref().map_or(0.0, |car| if gear >= 0 { car.gear_ratio(gear as usize) as f32 } else { car.final_ratio() });
        for (label, ratio) in load_gear_ratios(&text.text_file).unwrap_or_default() {
            self.spinner_add_item(sp, &label.replace("//", "/"));
            values.push(ratio);
            // (`ucomiss`: equal, or one of them not a number)
            if ratio == current || ratio.is_nan() || current.is_nan() {
                let line = self.spinner(sp).max_value;
                self.spinner_set_value(sp, line);
            }
        }
        self.setup_gears_mut(id).gears.insert(sp, GearDef { setup_manager_name: text.setup_manager_var_name.clone(), values });
        let value = self.spinner(sp).value;
        self.setup_gears_mut(id).default_values.push(value);
        self.spinner_mut(sp).handlers.push(Rc::new(move |gui: &mut Gui, ev: &WidgetEvent| {
            if let WidgetEvent::SpinnerValueChanged { spinner, .. } = ev {
                gui.setup_gears_gear_changed(id, *spinner);
            }
        }));
        self.add_control(id, sp);
        let tab = self.setup_tab_mut(id);
        tab.spinners.push(sp);
        tab.tags.push(None);
    }

    /// `SetupGearManager::addGearSet` 0x140171d00.
    pub fn setup_gears_add_gear_set(&mut self, painter: &mut dyn Painter, id: Id, name: &str, set: Vec<GearDef>) {
        let sp = match self.setup_gears(id).gear_set_spinner {
            Some(sp) => sp,
            None => {
                let sp = self.new_spinner(painter, "GEAR_SETS", true, false);
                self.setup_gears_mut(id).gear_set_spinner = Some(sp);
                self.add_control(id, sp);
                self.set_size(sp, 184.0, 21.0);
                let (tab, own) = (self.controls[id].rect, self.controls[sp].rect);
                let x = (tab.right - tab.left) * 0.5 - (own.right - own.left) * 0.5;
                self.spinner_set_position(sp, x, 100.0);
                self.spinner_mut(sp).draw_underline = true;
                self.spinner_mut(sp).handlers.push(Rc::new(move |gui: &mut Gui, ev: &WidgetEvent| {
                    if let WidgetEvent::SpinnerValueChanged { spinner, .. } = ev {
                        gui.setup_gears_set_changed(id, *spinner);
                    }
                }));
                sp
            }
        };
        self.spinner_add_item(sp, name);
        self.setup_gears_mut(id).gear_sets.insert(name.to_string(), set);
    }

    /// The gears' part of `SetupScreen::loadINI` 0x14017d950: a spinner per `[GEAR_n]`, or the
    /// gear sets `[GEAR_SET_n]`; then `[FINAL_GEAR_RATIO]`.
    pub(crate) fn setup_gears_load_ini(&mut self, painter: &mut dyn Painter, id: Id, reader: &IniReader, use_gearset: bool, max_gear: i32) {
        let Some(car) = &self.setup_car else { return };
        let data_path = car.data_path();
        if !use_gearset {
            for g in 2..max_gear.max(0) + 2 {
                let section = format!("GEAR_{}", g - 1);
                if !reader.has_section(&section) {
                    continue;
                }
                let var = format!("INTERNAL_GEAR_{g}");
                let mut text = SetupItemText { display_name: self.locale.get_string(&var), ..SetupItemText::default() };
                if text.display_name.is_empty() {
                    text.display_name = reader.get_string(&section, "NAME");
                }
                text.help_key = reader.get_string(&section, "HELP");
                text.text_file = append_path(&data_path, &reader.get_string(&section, "RATIOS"));
                text.setup_manager_var_name = var;
                self.setup_gears_add_setup_item(painter, id, g, &text);
            }
        } else {
            let mut n = 0;
            loop {
                let set_name = format!("GEAR_SET_{n}");
                if !reader.has_section(&set_name) {
                    break;
                }
                let mut set = Vec::new();
                for g in 2..max_gear.max(0) + 2 {
                    let value = reader.get_float(&set_name, &format!("GEAR_{}", g - 1)).unwrap_or(0.0);
                    set.push(GearDef { setup_manager_name: format!("INTERNAL_GEAR_{g}"), values: vec![value] });
                }
                let name = reader.get_string(&set_name, "NAME");
                self.setup_gears_add_gear_set(painter, id, &name, set);
                n += 1;
            }
        }
        if reader.has_section("FINAL_GEAR_RATIO") {
            let mut text = SetupItemText { display_name: self.locale.get_string("INTERNAL_FINAL_RATIO"), ..SetupItemText::default() };
            if text.display_name.is_empty() {
                text.display_name = reader.get_string("FINAL_GEAR_RATIO", "NAME");
            }
            text.help_key = reader.get_string("FINAL_GEAR_RATIO", "HELP");
            text.text_file = append_path(&data_path, &reader.get_string("FINAL_GEAR_RATIO", "RATIOS"));
            text.setup_manager_var_name = "FINAL_RATIO".to_string();
            self.setup_gears_add_setup_item(painter, id, -1, &text);
        }
    }

    /// `RaceEngineer::getMaxSpeedFromGear` 0x14027c240 (m/s; `gear` 0 is the first).
    fn setup_gears_max_speed(&self, gear: usize) -> f32 {
        let Some(car) = &self.setup_car else { return 0.0 };
        let radius = if car.is_front_wheel_drive() { car.tyre_radius(0) } else { car.tyre_radius(2) };
        let r2 = radius * 2.0;
        let rpm = car.limiter_rpm();
        let den = (car.final_ratio() as f64 * car.gear_ratio(gear + 2)) * 336.0;
        let num = (rpm as f32 * r2) * 39.0;
        let mph = (num as f64 / den) as f32;
        mph * f32::from_bits(0x3ee4_e26d)
    }

    /// `SetupGearManager::getStringMaxSpeed` 0x140172de0.
    fn setup_gears_string_max_speed(&self, gear: usize) -> String {
        let speed = self.setup_gears_max_speed(gear);
        let name = translate("Max speed gear");
        if !self.setup.use_mph {
            format!("{name} {} KMH : {}", gear + 1, format_g6(msvc_floor(speed * f32::from_bits(0x4066_6666))))
        } else {
            format!("{name} {} MPH : {}", gear + 1, format_g6(msvc_floor(speed * f32::from_bits(0x400f_29f7))))
        }
    }

    /// `SetupGearManager::render` 0x1401743a0.
    pub(crate) fn setup_gears_render(&mut self, id: Id, dt: f32, painter: &mut dyn Painter) {
        self.setup_tab_render_base(id, dt, painter);
        let Some(car) = &self.setup_car else { return };
        let (max_gear, final_ratio) = (car.max_gear(), car.final_ratio());
        let [ox, oy] = self.local_to_world(id, [0.0, 0.0]);
        let font = self.controls[id].font.unwrap_or(self.font);
        let set_mode = self.setup_gears(id).gear_set_spinner.is_some();
        for i in 0..max_gear.max(0) {
            let text = self.setup_gears_string_max_speed(i as usize);
            if !set_mode {
                let fy = msvc_floor((i as f32 * 1.5) * 48.0 + 55.0);
                let y = if max_gear <= 7 { (fy + oy) + 27.0 } else { (fy * 0.5 + oy) + 60.0 };
                self.fonts[font].blit_string(painter, ox + 127.0, y, &text, f32::from_bits(0x3fa6_6666), ALIGN_LEFT);
            } else {
                let y = i as f32 * 50.0 + (oy + 150.0);
                self.fonts[font].blit_string(painter, ox + 145.0, y, &text, 1.0, ALIGN_LEFT);
            }
        }
        if set_mode {
            let text = format!("{} : {}", translate("Final ratio"), format_g6(final_ratio));
            let y = max_gear as f32 * 60.0 + (oy + 150.0);
            self.fonts[font].blit_string(painter, ox + 170.0, y, &text, f32::from_bits(0x3fa6_6666), ALIGN_LEFT);
        }
    }

    /// `SetupGearManager::getStringToSave` 0x1401730d0: the spinners' lines, not ratios.
    pub(crate) fn setup_gears_string_to_save(&self, id: Id) -> String {
        let gears = self.setup_gears(id);
        let mut out = String::new();
        if let Some(sp) = gears.gear_set_spinner {
            out.push_str(&format!("[GEARSET]\nVALUE={}\n", self.spinner(sp).value));
        }
        for sp in &self.setup_tab(id).spinners {
            let name = gears.gears.get(sp).map_or("", |def| def.setup_manager_name.as_str());
            out.push_str(&format!("[{name}]\nVALUE={}\n", self.spinner(*sp).value));
        }
        out
    }

    /// `SetupGearManager::loadFromINI` 0x1401734a0.
    pub(crate) fn setup_gears_load_from_ini(&mut self, id: Id, ini: &IniReader, fixed: bool) {
        if let Some(sp) = self.setup_gears(id).gear_set_spinner {
            if ini.has_section("GEARSET") {
                self.setup_tab_set_value_to_spinner(sp, fixed, ini.get_int("GEARSET", "VALUE").unwrap_or(0));
            }
        }
        for sp in self.setup_tab(id).spinners.clone() {
            let name = self.setup_gears(id).gears.get(&sp).map(|def| def.setup_manager_name.clone()).unwrap_or_default();
            if ini.has_section(&name) {
                self.setup_tab_set_value_to_spinner(sp, fixed, ini.get_int(&name, "VALUE").unwrap_or(0));
            } else {
                self.spinner_mut(sp).is_read_only = false;
            }
        }
    }

    /// `SetupGearManager::resetToDefaultValues` 0x1401749c0.
    pub(crate) fn setup_gears_reset_to_default_values(&mut self, id: Id) {
        if let Some(sp) = self.setup_gears(id).gear_set_spinner {
            if !self.spinner(sp).is_read_only {
                self.spinner_set_value(sp, 0);
            }
        }
        let defaults = self.setup_gears(id).default_values.clone();
        for (sp, value) in self.setup_tab(id).spinners.clone().into_iter().zip(defaults) {
            if !self.spinner(sp).is_read_only {
                self.spinner_set_value(sp, value);
            }
        }
        self.setup_tab_mut(id).modified_values = false;
    }
}
