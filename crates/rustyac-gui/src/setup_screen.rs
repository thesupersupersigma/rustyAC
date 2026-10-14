// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! `SetupScreen` (0x1f8 bytes), the ESC menu's page "Setup": a bar of tabs, the pages (save /
//! load, gears, tyres, fuel, electronics, the pages made from the car's `data/setup.ini`, the
//! pit strategy) and the car's status beside them.
//!
//! `SetupScreen::SetupScreen` 0x14017c010, the tab bar's handler 0x14017cfd0, `getSetupTab`
//! 0x14017d650, `loadINI` 0x14017d950, `render` 0x14017f7c0, `switchVisibility` 0x14017fcd0,
//! `resetTabsValueToDefault` 0x14017f950, `needToSave` 0x14017f500, `getFixedFuel`
//! 0x14017d640, `isFixedFuel` 0x14017d940, `saveSetupAbsolutePath` 0x14017f9c0,
//! `loadSetupAbsolutePath` 0x14017f2c0; `ESCMenu::onShutdown` 0x1400fdf80.
//!
//! The car is what [`crate::setup_car::SetupCar`] says of it; what the game hands the physics
//! thread (`PhysicsAvatar::executeOnPhysicsThread`) are the [`SetupJob`]s given to it.

use std::path::Path;
use std::rc::Rc;

use rustyac_physics::car::setup::SetupJob;
use rustyac_physics::data::ini::IniReader;

use crate::control::{Id, Kind};
use crate::gui::Gui;
use crate::hud::translate;
use crate::menus::Menu;
use crate::painter::Painter;
use crate::setup_tab::SetupItemDef;
use crate::widgets::{msvc_floor, WidgetEvent};

/// `SetupScreen`'s own members.
pub struct SetupScreen {
    pub car_name: String,
    pub tab_bar: Id,
    pub tabs: Vec<Id>,
    pub setup_info: Id,
    pub io_manager: Id,
    pub gear_manager: Id,
    pub tyres_manager: Id,
    pub fuel_manager: Id,
    pub electronics_manager: Id,
    pub pitstop_manager: Option<Id>,
}

/// A setup file's text as the game's text-mode `wofstream` writes it: every line break CR LF,
/// every character one byte (the stream stops at the first that is none).
pub fn text_file_bytes(text: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len() + text.len() / 8);
    for c in text.chars() {
        let code = c as u32;
        if code > 0xff {
            break;
        }
        if c == '\n' {
            out.push(b'\r');
        }
        out.push(code as u8);
    }
    out
}

/// The file as the stream's open and close leave it (a folder that is missing: no file).
fn write_file(path: &Path, bytes: &[u8]) -> bool {
    std::fs::write(path, bytes).is_ok()
}

impl Gui {
    pub fn setup_screen(&self, id: Id) -> &SetupScreen {
        match &self.controls[id].kind {
            Kind::Menu(menu) => match &**menu {
                Menu::SetupScreen(screen) => screen,
                _ => panic!("control {id} is not the setup screen"),
            },
            _ => panic!("control {id} is not the setup screen"),
        }
    }

    pub fn setup_screen_mut(&mut self, id: Id) -> &mut SetupScreen {
        match &mut self.controls[id].kind {
            Kind::Menu(menu) => match &mut **menu {
                Menu::SetupScreen(screen) => screen,
                _ => panic!("control {id} is not the setup screen"),
            },
            _ => panic!("control {id} is not the setup screen"),
        }
    }

    /// A page beside the tab bar's left end, and in the bar.
    fn setup_screen_place_tab(&mut self, painter: &mut dyn Painter, tab_bar: Id, tab: Id) {
        let left = msvc_floor(self.controls[tab_bar].rect.left);
        self.set_position(tab, left, 65.0);
        let name = self.controls[tab].name.clone();
        self.tab_bar_add_tab(painter, tab_bar, &name);
    }

    /// `SetupScreen::SetupScreen` 0x14017c010. The car is [`Gui::setup_car`] (none: only the
    /// pages every car has are made, as in the game for a car without a race engineer);
    /// `Sim::useMousePitstop` is [`crate::setup_car::SetupLink::use_mouse_pitstop`]. The pit
    /// menu the game makes here is made by `pit_menu`, at the same moment.
    pub fn new_setup_screen(&mut self, painter: &mut dyn Painter, pit_menu: Option<crate::esc_menu::PageMaker>) -> Id {
        let id = self.new_control(painter, "SETUP_SCREEN");
        self.controls[id].back_color = [0.0; 4];
        self.set_position(id, 100.0, 0.0);
        self.controls[id].visible = false;
        self.controls[id].draw_border = false;
        self.controls[id].draw_background = false;

        let tab_bar = self.new_tab_bar(painter, "TAB BAR");
        self.add_control(id, tab_bar);
        // the tab bar's handler (0x14017cfd0)
        self.tab_bar_mut(tab_bar).handlers.push(Rc::new(move |gui: &mut Gui, ev: &WidgetEvent| {
            if let WidgetEvent::TabSelected { old_index, new_index, .. } = ev {
                let tabs = gui.setup_screen(id).tabs.clone();
                if let (Some(old), Some(new)) = (tabs.get(*old_index as usize), tabs.get(*new_index as usize)) {
                    gui.set_visible(*old, false);
                    gui.set_visible(*new, true);
                }
            }
        }));
        let mut tabs = Vec::new();
        let io_manager = self.new_setup_io(painter, id);
        self.setup_screen_place_tab(painter, tab_bar, io_manager);
        tabs.push(io_manager);
        let track = self.setup.track_name.clone();
        self.setup_io_set_selected_track(io_manager, &track);
        let gear_manager = self.new_setup_gears(painter);
        self.setup_screen_place_tab(painter, tab_bar, gear_manager);
        tabs.push(gear_manager);
        self.set_visible(gear_manager, false);
        let tyres_manager = self.new_setup_tyres(painter);
        self.setup_screen_place_tab(painter, tab_bar, tyres_manager);
        tabs.push(tyres_manager);
        self.set_visible(tyres_manager, false);
        let fuel_manager = self.new_setup_fuel(painter);
        self.setup_screen_place_tab(painter, tab_bar, fuel_manager);
        tabs.push(fuel_manager);
        self.set_visible(fuel_manager, false);
        let electronics_manager = self.new_setup_electronics(painter);
        if self.setup_electronics_has_spinners(electronics_manager) {
            self.setup_screen_place_tab(painter, tab_bar, electronics_manager);
            tabs.push(electronics_manager);
        }
        self.set_visible(electronics_manager, false);
        let setup_info = self.new_setup_information(painter);
        let x = msvc_floor(self.controls[io_manager].rect.right + 100.0);
        self.set_position(setup_info, x, 65.0);
        for child in [setup_info, tyres_manager, gear_manager, io_manager, fuel_manager, electronics_manager] {
            self.add_control(id, child);
        }
        self.controls[id].kind = Kind::Menu(Box::new(Menu::SetupScreen(Box::new(SetupScreen {
            car_name: String::new(),
            tab_bar,
            tabs,
            setup_info,
            io_manager,
            gear_manager,
            tyres_manager,
            fuel_manager,
            electronics_manager,
            pitstop_manager: None,
        }))));
        if self.setup_car.is_none() {
            return id;
        }
        let quick_menu = if self.setup.use_mouse_pitstop { None } else { pit_menu.map(|make| make(self, painter)) };
        self.setup_screen_load_ini(painter, id, quick_menu);
        self.setup_screen_reset_tabs_value_to_default(id);
        if !self.setup.use_mouse_pitstop {
            let presets = self.new_setup_presets(painter, quick_menu);
            self.setup_screen_place_tab(painter, tab_bar, presets);
            self.setup_screen_mut(id).tabs.push(presets);
            self.set_visible(presets, false);
            self.add_control(id, presets);
            self.setup_screen_mut(id).pitstop_manager = Some(presets);
        }
        // (the game ends here with a check of the car's data: a car with a heave spring and
        // ROD_LENGTH_HF / HR in its setup.ini beside a wheel's own makes it stop)
        id
    }

    /// `SetupScreen::getSetupTab` 0x14017d650: the page of that name, made if there is none.
    fn setup_screen_get_setup_tab(&mut self, painter: &mut dyn Painter, id: Id, name: &str) -> Id {
        for tab in &self.setup_screen(id).tabs {
            if self.controls[*tab].name == name {
                return *tab;
            }
        }
        let tab = self.new_setup_tab(painter, name);
        self.set_visible(tab, false);
        let tab_bar = self.setup_screen(id).tab_bar;
        self.tab_bar_add_tab(painter, tab_bar, name);
        self.setup_screen_mut(id).tabs.push(tab);
        let left = msvc_floor(self.controls[tab_bar].rect.left);
        self.set_position(tab, left, 65.0);
        self.add_control(id, tab);
        tab
    }

    /// `SetupScreen::loadINI` 0x14017d950: the car's `data/setup.ini`. The car's setup items
    /// lead (in their own order): each that has a section gets a spinner on the page the
    /// section names, and is attached.
    fn setup_screen_load_ini(&mut self, painter: &mut dyn Painter, id: Id, quick_menu: Option<Id>) {
        let Some(car) = &self.setup_car else { return };
        let (unix_name, data_path, items, max_gear) = (car.unix_name(), car.data_path(), car.items(), car.max_gear());
        self.setup_screen_mut(id).car_name = unix_name;
        let reader = IniReader::load(&data_path.join("setup.ini")).unwrap_or_default();
        if reader.ready {
            if let Some(menu) = quick_menu {
                self.setup.quick_menu_build_controls.push(menu);
            }
            let use_gearset = reader.has_section("GEARS") && reader.get_int("GEARS", "USE_GEARSET").unwrap_or(0) > 0;
            let float = |section: &str, key: &str| reader.get_float(section, key).unwrap_or(0.0);
            for (index, item) in items.iter().enumerate() {
                let name = item.name.as_str();
                if !reader.has_section(name) {
                    continue;
                }
                let raw_tab = reader.get_string(name, "TAB");
                let mut tab_name = self.locale.get_string(&raw_tab);
                if tab_name.is_empty() {
                    tab_name = raw_tab;
                }
                let tab = self.setup_screen_get_setup_tab(painter, id, &tab_name);
                let mut def = SetupItemDef { name: self.locale.get_string(name), ..SetupItemDef::default() };
                if def.name.is_empty() {
                    def.name = reader.get_string(name, "NAME");
                }
                def.connection = index;
                if let Some(car) = &mut self.setup_car {
                    car.job(SetupJob::Attach { item: index });
                }
                def.px = float(name, "POS_X");
                def.py = float(name, "POS_Y");
                let label = item.label_multiplier.abs();
                def.min_value = (float(name, "MIN") / label) as f64;
                def.max_value = (float(name, "MAX") / label) as f64;
                def.step = reader.get_int(name, "STEP").unwrap_or(0) as f64;
                def.help_key = reader.get_string(name, "HELP");
                def.show_clicks_mode = if reader.has_section("DISPLAY_METHOD") { reader.get_int(name, "SHOW_CLICKS").unwrap_or(0) } else { 0 };
                self.setup_tab_add_item(painter, tab, def);
            }
            let gears = self.setup_screen(id).gear_manager;
            self.setup_gears_load_ini(painter, gears, &reader, use_gearset, max_gear);
        }
        // the pages after the fixed ones, by name
        let mut first = 4usize;
        let electronics = translate("ELECTRONICS");
        if self.setup_screen(id).tabs.iter().any(|tab| self.controls[*tab].name == electronics) {
            first = 5;
        }
        let tab_bar = self.setup_screen(id).tab_bar;
        let count = self.tab_bar(tab_bar).elements.len();
        self.tab_bar_order_alphabetically(tab_bar, first, count);
        let mut tabs = self.setup_screen(id).tabs.clone();
        if first < tabs.len() {
            let key = |tab: &Id| self.controls[*tab].name.encode_utf16().collect::<Vec<u16>>();
            rustyac_physics::msvc_sort::sort(&mut tabs[first..], &|a: &Id, b: &Id| key(a) < key(b));
        }
        self.setup_screen_mut(id).tabs = tabs;
    }

    /// `SetupScreen::render` 0x14017f7c0.
    pub(crate) fn setup_screen_render(&mut self, id: Id, dt: f32, painter: &mut dyn Painter) {
        let (width, height) = painter.screen();
        let mut w = width as f32;
        if self.sim.triple_screen {
            w *= f32::from_bits(0x3eaa_aaab);
        }
        let left = self.controls[id].rect.left;
        self.set_size(id, w - left, height as f32);
        let (tab_bar, tabs, info) = (self.setup_screen(id).tab_bar, self.setup_screen(id).tabs.clone(), self.setup_screen(id).setup_info);
        self.tab_bar_update_position(tab_bar);
        if !tabs.is_empty() {
            let tab = tabs[self.tab_bar(tab_bar).top_index as usize];
            let left = msvc_floor(self.controls[tab_bar].rect.left);
            self.set_position(tab, left, 65.0);
            let x = msvc_floor(self.controls[tab].rect.right + 100.0);
            self.set_position(info, x, 65.0);
        }
        self.set_visible(tab_bar, !tabs.is_empty());
        self.control_render(id, dt, painter);
    }

    /// `SetupScreen::resetTabsValueToDefault` 0x14017f950.
    pub fn setup_screen_reset_tabs_value_to_default(&mut self, id: Id) {
        for tab in self.setup_screen(id).tabs.clone() {
            self.setup_tab_reset_to_default_values(tab);
        }
    }

    /// `SetupScreen::needToSave` 0x14017f500.
    pub fn setup_screen_need_to_save(&self, id: Id) -> bool {
        self.setup_screen(id).tabs.iter().any(|tab| self.setup_tab(*tab).modified_values)
    }

    /// `SetupScreen::getFixedFuel` 0x14017d640.
    pub fn setup_screen_get_fixed_fuel(&self, id: Id) -> f32 {
        self.setup_fuel_get_setup_fuel(self.setup_screen(id).fuel_manager)
    }

    /// `SetupScreen::isFixedFuel` 0x14017d940.
    pub fn setup_screen_is_fixed_fuel(&self, id: Id) -> bool {
        self.setup_fuel_is_fixed(self.setup_screen(id).fuel_manager)
    }

    /// The text `SetupScreen::saveSetupAbsolutePath` 0x14017f9c0 writes (line breaks as the
    /// streams have them: one character), and every page's changes are forgotten.
    pub fn setup_screen_setup_text(&mut self, id: Id) -> String {
        let mut out = String::new();
        for tab in self.setup_screen(id).tabs.clone() {
            out.push_str(&self.setup_tab_string_to_save(tab));
            out.push('\n');
            self.setup_tab_mut(tab).modified_values = false;
        }
        out.push_str(&format!("[CAR]\nMODEL={}\n", self.setup_screen(id).car_name));
        out
    }

    /// `SetupScreen::saveSetupAbsolutePath` 0x14017f9c0: the setup's file and, beside it, the
    /// pit strategy's (`.sp`).
    pub fn setup_screen_save_setup_absolute_path(&mut self, id: Id, path: &Path) -> bool {
        if path.as_os_str().is_empty() {
            return false;
        }
        let text = self.setup_screen_setup_text(id);
        write_file(path, &text_file_bytes(&text));
        if let Some(presets) = self.setup_screen(id).pitstop_manager {
            let text = self.setup_presets_save_text(presets);
            write_file(&sp_path(path), &text_file_bytes(&text));
        }
        true
    }

    /// `SetupScreen::loadSetupAbsolutePath` 0x14017f2c0.
    pub fn setup_screen_load_setup_absolute_path(&mut self, id: Id, path: &Path, fixed: bool) -> bool {
        let mut ok = false;
        if path.is_file() {
            let reader = IniReader::load(path).unwrap_or_default();
            if reader.ready {
                for tab in self.setup_screen(id).tabs.clone() {
                    self.setup_tab_load_from_ini(tab, &reader, fixed);
                    self.setup_tab_mut(tab).modified_values = false;
                }
                ok = true;
            }
        }
        if let Some(presets) = self.setup_screen(id).pitstop_manager {
            if fixed {
                self.setup_presets_load_fixed(presets, path);
            }
            let sp = sp_path(path);
            if sp.is_file() {
                let reader = IniReader::load(&sp).unwrap_or_default();
                self.setup_presets_load_presets(presets, &reader);
            }
        }
        ok
    }

    /// `ESCMenu::onShutdown` 0x1400fdf80: the setup as it is goes to
    /// `<car>\generic\last.ini` under the `setups` folder (the game's is in `Documents\Assetto
    /// Corsa`; rustyAC's own is beside its exe).
    pub fn esc_menu_on_shutdown(&mut self, menu: Id, setups: &Path) {
        let Some(screen) = self.esc_menu(menu).setup_screen else { return };
        if !matches!(&self.controls[screen].kind, Kind::Menu(m) if matches!(**m, Menu::SetupScreen(_))) {
            return;
        }
        let Some(car) = &self.setup_car else { return };
        if self.sim.replay.has_loaded_replay {
            return;
        }
        let generic = setups.join(car.unix_name()).join("generic");
        let _ = std::fs::create_dir_all(&generic);
        self.setup_screen_save_setup_absolute_path(screen, &generic.join("last.ini"));
    }
}

/// `path.substr(0, size - 4) + L".sp"`.
pub fn sp_path(path: &Path) -> std::path::PathBuf {
    let text = path.to_string_lossy();
    let cut: String = text.chars().take(text.chars().count().saturating_sub(4)).collect();
    std::path::PathBuf::from(format!("{cut}.sp"))
}
