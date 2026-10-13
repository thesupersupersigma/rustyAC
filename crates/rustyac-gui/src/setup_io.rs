// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! STUB (Task 31, being ported).

use crate::control::Id;
use crate::gui::Gui;
use crate::painter::Painter;
use crate::setup_tab::TabKind;

pub struct SetupIo {
    pub screen: Id,
}

impl Gui {
    pub fn new_setup_io(&mut self, painter: &mut dyn Painter, screen: Id) -> Id {
        let id = self.new_setup_tab(painter, "Setup I/O");
        self.setup_tab_mut(id).kind = TabKind::Io(Box::new(SetupIo { screen }));
        id
    }

    pub fn setup_io_set_selected_track(&mut self, _id: Id, _track: &str) {}

    pub(crate) fn setup_io_render(&mut self, id: Id, dt: f32, painter: &mut dyn Painter) {
        self.setup_tab_render_base(id, dt, painter);
    }

    pub(crate) fn setup_io_on_visible_changed(&mut self, _id: Id, _visible: bool) {}
}
