// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! STUB (Task 31, being ported).

use rustyac_physics::data::ini::IniReader;

use crate::control::Id;
use crate::gui::Gui;
use crate::hud::translate;
use crate::painter::Painter;
use crate::setup_tab::TabKind;

pub struct SetupFuel {}

impl Gui {
    pub fn new_setup_fuel(&mut self, painter: &mut dyn Painter) -> Id {
        let id = self.new_setup_tab(painter, &translate("FUEL"));
        self.setup_tab_mut(id).kind = TabKind::Fuel(Box::new(SetupFuel {}));
        id
    }

    pub(crate) fn setup_fuel_string_to_save(&self, id: Id) -> String {
        self.setup_tab_string_to_save_base(id)
    }

    pub(crate) fn setup_fuel_load_from_ini(&mut self, id: Id, ini: &IniReader, fixed: bool) {
        self.setup_tab_load_from_ini_base(id, ini, fixed);
    }

    pub(crate) fn setup_fuel_render(&mut self, id: Id, dt: f32, painter: &mut dyn Painter) {
        self.setup_tab_render_base(id, dt, painter);
    }

    pub(crate) fn setup_fuel_on_visible_changed(&mut self, _id: Id, _visible: bool) {}

    pub fn setup_fuel_get_setup_fuel(&self, _id: Id) -> f32 {
        0.0
    }

    pub fn setup_fuel_is_fixed(&self, _id: Id) -> bool {
        false
    }
}
