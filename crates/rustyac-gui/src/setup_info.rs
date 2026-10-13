// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! STUB (Task 31, being ported).

use crate::control::Id;
use crate::gui::Gui;
use crate::painter::Painter;

impl Gui {
    pub fn new_setup_information(&mut self, painter: &mut dyn Painter) -> Id {
        self.new_control(painter, "SETUP_INFO")
    }
}
