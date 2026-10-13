// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! `SetupTyresManager` (0x1d8 bytes), the setup editor's page "TYRES": a spinner that picks
//! the compound; the pressures are ordinary setup items the car's `setup.ini` puts here.
//!
//! Constructor 0x140184650, the spinner's handler 0x140184bb0 with
//! `CarAvatar::setTyreCompound` 0x1400dabd0, `getStringToSave` 0x140184ce0, `loadFromINI`
//! 0x140184eb0, `resetToDefaultValues` 0x14016fa20 (the fuel page's too).

use std::rc::Rc;

use rustyac_physics::car::setup::SetupJob;
use rustyac_physics::data::ini::IniReader;

use crate::control::Id;
use crate::gui::Gui;
use crate::hud::translate;
use crate::painter::Painter;
use crate::setup_tab::TabKind;
use crate::widgets::WidgetEvent;

/// `SetupTyresManager`'s own members.
pub struct SetupTyres {
    pub tyre_spinner: Id,
    pub default_compound_index: i32,
}

impl Gui {
    /// `SetupTyresManager::SetupTyresManager` 0x140184650.
    pub fn new_setup_tyres(&mut self, painter: &mut dyn Painter) -> Id {
        let id = self.new_setup_tab(painter, &translate("TYRES"));
        self.control_set_size(id, 450.0, 400.0);
        let sp = self.new_spinner(painter, &translate("Compound"), true, false);
        let mut default_compound_index = 0;
        let mut names = Vec::new();
        if let Some(car) = &self.setup_car {
            let ini = IniReader::load(&car.data_path().join("tyres.ini")).unwrap_or_default();
            if ini.ready && ini.has_section("COMPOUND_DEFAULT") {
                default_compound_index = ini.get_int("COMPOUND_DEFAULT", "INDEX").unwrap_or(0);
            }
            names = car.tyre_compound_names();
        }
        for name in &names {
            self.spinner_add_item(sp, name);
        }
        self.setup_tab_mut(id).kind = TabKind::Tyres(Box::new(SetupTyres { tyre_spinner: sp, default_compound_index }));
        // the spinner's handler (0x140184bb0): the compound of that name, on every wheel
        self.spinner_mut(sp).handlers.push(Rc::new(move |gui: &mut Gui, ev: &WidgetEvent| {
            if let WidgetEvent::SpinnerValueChanged { spinner, .. } = ev {
                for tyre in 0..4 {
                    let name = gui.spinner_current_item(*spinner);
                    gui.setup_car_set_tyre_compound(tyre, &name);
                }
            }
        }));
        self.spinner_set_draw_arrows(painter, sp, true);
        self.set_size(sp, 350.0, 21.0);
        let (tab, own) = (self.controls[id].rect, self.controls[sp].rect);
        let x = (tab.right - tab.left) * 0.5 - (own.right - own.left) * 0.5;
        self.spinner_set_position(sp, x, 100.0);
        self.add_control(id, sp);
        id
    }

    /// `CarAvatar::setTyreCompound` 0x1400dabd0: a job for the physics thread for every
    /// compound of that name.
    pub fn setup_car_set_tyre_compound(&mut self, tyre: usize, name: &str) {
        let Some(car) = &mut self.setup_car else { return };
        for (index, compound) in car.tyre_compound_names().iter().enumerate() {
            if compound == name {
                car.job(SetupJob::SetTyreCompound { tyre, index: index as i32 });
            }
        }
    }

    fn setup_tyres(&self, id: Id) -> &SetupTyres {
        match &self.setup_tab(id).kind {
            TabKind::Tyres(tyres) => tyres,
            _ => panic!("control {id} is not the tyres page"),
        }
    }

    /// `SetupTyresManager::getStringToSave` 0x140184ce0.
    pub(crate) fn setup_tyres_string_to_save(&self, id: Id) -> String {
        format!("[TYRES]\nVALUE={}\n{}", self.spinner(self.setup_tyres(id).tyre_spinner).value, self.setup_tab_string_to_save_base(id))
    }

    /// `SetupTyresManager::loadFromINI` 0x140184eb0.
    pub(crate) fn setup_tyres_load_from_ini(&mut self, id: Id, ini: &IniReader, fixed: bool) {
        let sp = self.setup_tyres(id).tyre_spinner;
        if ini.has_section("TYRES") {
            self.setup_tab_set_value_to_spinner(sp, fixed, ini.get_int("TYRES", "VALUE").unwrap_or(0));
        } else {
            self.spinner_mut(sp).is_read_only = false;
        }
        self.setup_tab_load_from_ini_base(id, ini, fixed);
    }

    /// `resetToDefaultValues` 0x14016fa20 of the tyres' and of the fuel's page.
    pub(crate) fn setup_tyres_fuel_reset_to_default_values(&mut self, id: Id) {
        let (sp, value) = match &self.setup_tab(id).kind {
            TabKind::Tyres(tyres) => (tyres.tyre_spinner, tyres.default_compound_index),
            TabKind::Fuel(fuel) => (fuel.fuel_spinner, fuel.default_value),
            _ => panic!("control {id} is not the tyres or the fuel page"),
        };
        if !self.spinner(sp).is_read_only {
            self.spinner_set_value(sp, value);
        }
        self.setup_tab_reset_to_default_values_base(id);
    }

    pub(crate) fn setup_tyres_render(&mut self, id: Id, dt: f32, painter: &mut dyn Painter) {
        self.setup_tab_render_base(id, dt, painter);
    }
}
