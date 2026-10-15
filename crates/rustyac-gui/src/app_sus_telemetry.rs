// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! `TelemetryApp` (0x1e0 bytes), the developers' "SusTelemetry": each wheel's suspension travel
//! (red) against its packer range (yellow) as four graphs, one point a frame, of the player's
//! car.
//!
//! `TelemetryApp::TelemetryApp` 0x140302180, `render` 0x1403024a0; `CarAvatar::getPackerRange`
//! 0x1400d3540.

use std::any::Any;

use crate::apps::App;
use crate::control::Id;
use crate::gui::Gui;
use crate::hud::translate;
use crate::painter::Painter;

/// `TelemetryApp`'s own members: `grSusTravel`.
pub struct SusTelemetry {
    pub graphs: [Id; 4],
}

impl Gui {
    /// `TelemetryApp::TelemetryApp(CarAvatar*)` 0x140302180.
    pub fn new_sus_telemetry_app(&mut self, painter: &mut dyn Painter) -> Id {
        let form = self.new_form(painter, "SUS_TELEMETRY", false);
        self.form_set_title(form, &translate("SusTelemetry"));
        self.form_data_mut(form).dev_app = true;
        self.set_size(form, 600.0, 400.0);
        let x = [10.0, 300.0, 10.0, 300.0];
        let y = [40.0, 40.0, 140.0, 140.0];
        let mut graphs = [0; 4];
        for k in 0..4 {
            let g = self.new_graph(painter, "SUS_TRAVEL");
            self.set_position(g, x[k], y[k]);
            self.set_size(g, 250.0, 80.0);
            self.graph_add_serie(g, [1.0, 1.0, 0.0]);
            self.graph_add_serie(g, [1.0, 0.0, 0.0]);
            {
                let d = self.graph_mut(g);
                d.max_values_count = 100;
                d.max_y = f32::from_bits(0x3e4c_cccd);
                d.min_y = f32::from_bits(0xbe4c_cccd);
            }
            self.control_add_control(form, g);
            graphs[k] = g;
        }
        self.set_app(form, Box::new(SusTelemetry { graphs }));
        form
    }
}

impl App for SusTelemetry {
    fn as_any(&mut self) -> &mut dyn Any {
        self
    }

    /// `TelemetryApp::render` 0x1403024a0: the player's car (`cars[0]`), whatever the focus.
    fn render(&mut self, gui: &mut Gui, id: Id, dt: f32, painter: &mut dyn Painter) {
        let car = gui.apps.cars.first().cloned().unwrap_or_default();
        for k in 0..4 {
            let mut r = car.packer_range.map_or(0.0, |p| p[k]);
            if r == 0.0 {
                r = f32::from_bits(0x3dcc_cccd);
            }
            gui.graph_add_value(self.graphs[k], 0, r);
            gui.graph_add_value(self.graphs[k], 1, car.state.suspension_travel[k]);
        }
        gui.control_render(id, dt, painter);
    }
}
