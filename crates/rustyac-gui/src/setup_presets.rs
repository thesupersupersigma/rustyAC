// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! STUB (Task 31, being ported).

use std::path::Path;

use rustyac_physics::data::ini::IniReader;

use crate::control::Id;
use crate::gui::Gui;
use crate::hud::translate;
use crate::painter::Painter;
use crate::setup_tab::TabKind;

pub struct SetupPresets {
    pub quick_menu: Option<Id>,
}

impl Gui {
    pub fn new_setup_presets(&mut self, painter: &mut dyn Painter, quick_menu: Option<Id>) -> Id {
        let id = self.new_setup_tab(painter, &translate("PITSTOP STRATEGY"));
        self.setup_tab_mut(id).kind = TabKind::Presets(Box::new(SetupPresets { quick_menu }));
        id
    }

    pub(crate) fn setup_presets_render(&mut self, id: Id, dt: f32, painter: &mut dyn Painter) {
        self.setup_tab_render_base(id, dt, painter);
    }

    pub(crate) fn setup_presets_on_visible_changed(&mut self, _id: Id, _visible: bool) {}

    pub fn setup_presets_save_text(&self, _id: Id) -> String {
        String::new()
    }

    pub fn setup_presets_load_fixed(&mut self, _id: Id, _path: &Path) {}

    pub fn setup_presets_load_presets(&mut self, _id: Id, _reader: &IniReader) {}
}
