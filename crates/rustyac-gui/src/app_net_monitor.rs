// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! `NetMonitorForm` (0x1e0), the developers' "Net monitor", made online only: the speeds of the
//! first two cars in a graph, the second car's error vector and the ping to the server.
//!
//! `NetMonitorForm::NetMonitorForm(Sim*)` 0x140303400, `render` 0x1403037c0.

use std::any::Any;

use crate::acos::format_g;
use crate::apps::App;
use crate::control::Id;
use crate::gui::Gui;
use crate::painter::Painter;

pub struct NetMonitorApp {
    pub graph: Id,
    pub error_vector: Id,
    pub ping: Id,
}

impl Gui {
    /// `NetMonitorForm::NetMonitorForm` 0x140303400: 500 x 400.
    pub fn new_net_monitor_app(&mut self, painter: &mut dyn Painter) -> Id {
        let form = self.new_form(painter, "NET_MONITOR", false);
        self.form_set_title(form, "Net monitor");
        let graph = self.new_graph(painter, "VELOCITY");
        self.set_size(graph, 400.0, 200.0);
        self.set_position(graph, 10.0, 30.0);
        self.graph_add_serie(graph, [1.0, 1.0, 1.0]);
        self.graph_add_serie(graph, [1.0, 0.0, 1.0]);
        {
            let g = self.graph_mut(graph);
            g.max_values_count = 400;
            g.max_y = 300.0;
        }
        self.control_add_control(form, graph);
        let error_vector = self.new_label(painter, "ERROR_VECTOR");
        self.set_position(error_vector, 10.0, 30.0);
        self.control_add_control(form, error_vector);
        let ping = self.new_label(painter, "PING");
        self.set_position(ping, 10.0, 60.0);
        self.control_add_control(form, ping);
        self.form_set_size(form, 500.0, 400.0);
        self.set_app(form, Box::new(NetMonitorApp { graph, error_vector, ping }));
        form
    }
}

impl App for NetMonitorApp {
    fn as_any(&mut self) -> &mut dyn Any {
        self
    }

    /// `NetMonitorForm::render` 0x1403037c0: km/h (`speed * 3.6f`); with a second car its
    /// `NetCarStateProvider::errorVector` length (a stream of precision 2) and
    /// `ACClient::getPing(car 0)` (the client's own ping).
    fn render(&mut self, gui: &mut Gui, id: Id, dt: f32, painter: &mut dyn Painter) {
        let speed = |gui: &Gui, i: usize| gui.apps.cars.get(i).map_or(0.0, |c| c.state.speed) * 3.6f32;
        let v = speed(gui, 0);
        gui.graph_add_value(self.graph, 0, v);
        if gui.apps.cars.len() > 1 {
            let v = speed(gui, 1);
            gui.graph_add_value(self.graph, 1, v);
            let [x, y, z] = gui.apps.net_error_vector;
            let q = x * x + y * y + z * z;
            let length = if q == 0.0 { 0.0 } else { q.sqrt() };
            gui.set_text(self.error_vector, &format!("EV: {}", format_g(length as f64, 2)));
            let ping = gui.apps.ping;
            gui.set_text(self.ping, &format!("P: {ping}"));
        }
        gui.control_render(id, dt, painter);
    }
}
