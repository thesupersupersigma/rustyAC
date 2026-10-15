// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! `TyreWearingDebug` (0x1c0), the developers' "Tyre debug": the wear distance, the wear curve's
//! value, the core temperature and the thermal model's state of the player's four tyres.
//!
//! `TyreWearingDebug::TyreWearingDebug(Sim*)` 0x140319960, `render` 0x140319b10.

use std::any::Any;

use crate::acos::format_g6;
use crate::apps::App;
use crate::control::Id;
use crate::gui::Gui;
use crate::hud::translate;
use crate::painter::{Painter, ALIGN_LEFT};

/// One tyre of `PhysicsEngine::cars[0]`: `status.virtualKM`, `modelData.wearCurve.getValue(
/// (float)virtualKM)`, `thermalModel.coreTemp`, `thermalModel.isActive`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TyreDebugRow {
    pub virtual_km: f64,
    pub wear_value: f32,
    pub core_temp: f32,
    pub active: bool,
}

pub struct TyreDebug;

impl Gui {
    /// `TyreWearingDebug::TyreWearingDebug(Sim*)` 0x140319960: no children; it draws with the
    /// GUI's own default font.
    pub fn new_tyre_debug_app(&mut self, painter: &mut dyn Painter) -> Id {
        let form = self.new_form(painter, "TYRE_WEARING_DEBUG", false);
        self.form_set_title(form, &translate("Tyre debug"));
        self.form_set_size(form, 250.0, 450.0);
        self.set_app(form, Box::new(TyreDebug));
        form
    }
}

impl App for TyreDebug {
    fn as_any(&mut self) -> &mut dyn Any {
        self
    }

    /// `TyreWearingDebug::render` 0x140319b10.
    fn render(&mut self, gui: &mut Gui, id: Id, dt: f32, painter: &mut dyn Painter) {
        gui.control_render(id, dt, painter);
        let mut y = 30.0f32;
        // the GUI's shared default font: every control drawn with it is 15 from now on
        let font = gui.font;
        gui.fonts[font].scale = 15.0;
        // (focused car's physicsStateProvider, the player's car: None when either is missing)
        let Some(rows) = gui.apps.tyre_debug else { return };
        let r = gui.controls[id].rect;
        for (i, t) in rows.iter().enumerate() {
            let text = format!(
                "{} : {i}\n{} : {}\n{} : {}\n{} : {}\n{} : {}\n",
                translate("- TYRE"),
                translate("v.KM"),
                crate::app_track_map::format_g6(t.virtual_km),
                translate("value"),
                format_g6(t.wear_value * 100.0),
                translate("coreTemp"),
                format_g6(t.core_temp),
                translate("active"),
                t.active as u8
            );
            gui.fonts[font].blit_string(painter, r.left + 30.0, y + r.top, &text, 1.0, ALIGN_LEFT);
            y += 100.0;
        }
    }
}
