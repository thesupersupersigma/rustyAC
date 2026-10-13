// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! STUB (Task 31, being ported).

use rustyac_physics::data::ini::IniReader;

use crate::control::Id;
use crate::gui::Gui;
use crate::hud::translate;
use crate::painter::Painter;
use crate::setup_tab::TabKind;

pub struct SetupElectronics {}

impl Gui {
    pub fn new_setup_electronics(&mut self, painter: &mut dyn Painter) -> Id {
        let id = self.new_setup_tab(painter, &translate("ELECTRONICS"));
        self.setup_tab_mut(id).kind = TabKind::Electronics(Box::new(SetupElectronics {}));
        id
    }

    pub(crate) fn setup_electronics_string_to_save(&self, id: Id) -> String {
        self.setup_tab_string_to_save_base(id)
    }

    pub(crate) fn setup_electronics_load_from_ini(&mut self, id: Id, ini: &IniReader, fixed: bool) {
        self.setup_tab_load_from_ini_base(id, ini, fixed);
    }

    pub(crate) fn setup_electronics_render(&mut self, id: Id, dt: f32, painter: &mut dyn Painter) {
        self.setup_tab_render_base(id, dt, painter);
    }

    pub(crate) fn setup_electronics_reset_to_default_values(&mut self, id: Id) {
        self.setup_tab_reset_to_default_values_base(id);
    }

    pub(crate) fn setup_electronics_on_visible_changed(&mut self, _id: Id, _visible: bool) {}

    pub(crate) fn setup_electronics_has_spinners(&self, _id: Id) -> bool {
        false
    }
}
