// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! `TyreTesterForm` (0x1f8), the developers' "Tyre tester": every frame the chosen tyre of the
//! player's car is swept over slip angle and slip ratio at the car's load and speed, and the
//! curves are drawn (Fy, Fx, Mz, DY over load, Fy at 9 degrees, cambered DY), with the peaks
//! in a line of text and the friction circle at 9 degrees as a cyan line.
//!
//! `TyreTesterForm::TyreTesterForm(CarAvatar*, GUI*)` 0x14033b520, `render` 0x14033bc30.

use std::any::Any;
use std::path::Path;

use crate::apps::App;
use crate::control::Id;
use crate::gui::Gui;
use crate::hud::translate;
use crate::painter::{GlId, Painter, GL_LINE_STRIP};
use crate::widgets2::format_fixed;
use crate::widgets3::GraphReferenceAxis;
use rustyac_physics::tyre::tester::TyreTester;

/// What the app reads of the player's car: `physicsState.speed` (m/s), `physicsState.load`, and
/// each wheel's `CarAvatar::getTyreCompoundIndex`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TyreTesterCar {
    pub speed: f32,
    pub load: [f32; 4],
    pub compound: [i32; 4],
}

pub struct TyreTesterApp {
    testers: Vec<TyreTester>,
    graph: Id,
    label: Id,
    spinner: Id,
    /// `glTyreTester`: made at the first frame, 2000 vertices
    gl: Option<GlId>,
}

impl Gui {
    /// `TyreTesterForm::TyreTesterForm` 0x14033b520: four testers of the car's tyres
    /// (`content/cars/<car>/data/`), a graph of six series, the text line and the tyre spinner.
    pub fn new_tyre_tester_app(&mut self, painter: &mut dyn Painter, data_path: &Path) -> Result<Id, String> {
        let testers = (0..4).map(|i| TyreTester::new(data_path, i)).collect::<Result<Vec<_>, _>>()?;
        let form = self.new_form(painter, "TYRE_TESTER", false);
        self.form_set_title(form, &translate("Tyre tester"));
        self.form_set_size(form, 560.0, 560.0);
        let graph = self.new_graph(painter, "SLIPGRAPH");
        for color in [[1.0, 1.0, 1.0], [1.0, 0.0, 1.0], [0.0, 1.0, 0.0], [1.0, 1.0, 0.0], [0.0, 1.0, 1.0], [1.0, 0.5, 0.5]] {
            self.graph_add_serie(graph, color);
        }
        self.set_position(graph, 30.0, 30.0);
        self.set_size(graph, 500.0, 500.0);
        self.graph_mut(graph).min_y = -2.5;
        self.graph_mut(graph).max_y = 2.5;
        for is_vertical in [true, false] {
            self.graph_mut(graph).axes.push(GraphReferenceAxis { color: [0.2, 0.2, 0.2], ref_value: 0.5, is_vertical });
        }
        self.control_add_control(form, graph);
        let label = self.new_label(painter, &translate("DATA"));
        self.set_position(label, 10.0, 550.0);
        self.set_size(label, 500.0, 20.0);
        self.control_add_control(form, label);
        let spinner = self.new_spinner(painter, &translate("SPINNER_TYRE"), false, false);
        self.spinner_set_position(spinner, 250.0, 10.0);
        self.spinner_set_size(spinner, 100.0, 30.0);
        self.spinner_mut(spinner).min_value = 0;
        self.spinner_mut(spinner).max_value = 3;
        self.control_add_control(form, spinner);
        self.set_app(form, Box::new(TyreTesterApp { testers, graph, label, spinner, gl: None }));
        Ok(form)
    }
}

impl App for TyreTesterApp {
    fn as_any(&mut self) -> &mut dyn Any {
        self
    }

    /// `TyreTesterForm::render` 0x14033bc30.
    fn render(&mut self, gui: &mut Gui, id: Id, dt: f32, painter: &mut dyn Painter) {
        // (in a replay: nothing at all)
        if gui.apps.in_replay {
            return;
        }
        let car = gui.apps.tyre_tester.unwrap_or_default();
        let i = gui.spinner(self.spinner).value.clamp(0, 3) as usize;
        let t = &mut self.testers[i];
        t.set_compound(car.compound[i]);
        // (a NaN speed takes 15 too)
        t.velocity = if car.speed > 30.0 { car.speed } else { 15.0 };
        let load = car.load[i];
        let g = self.graph;

        let f0 = t.slip_angle_sweep(-20.0, 20.0, load, 200);
        for s in 0..3 {
            gui.graph_clear_serie(g, s);
        }
        gui.graph_mut(g).max_values_count = 200;
        let (mut max_fy, mut max_mz, mut sa_at_max_fy, mut sa_at_max_mz) = (0.0f32, 0.0f32, 0.0f32, 0.0f32);
        for f in &f0 {
            gui.graph_add_value(g, 0, f.fy / t.load);
            gui.graph_add_value(g, 2, (f.mz / (load * 0.0002)) * 0.01);
            if f.mz > max_mz {
                max_mz = f.mz;
                sa_at_max_mz = f.slip_angle;
            }
            if f.fy > max_fy {
                max_fy = f.fy;
                sa_at_max_fy = f.slip_angle;
            }
        }
        let f1 = t.slip_ratio_sweep(-1.0, 1.0, load, 200);
        let (mut max_fx, mut sr_at_max_fx) = (0.0f32, 0.0f32);
        for f in &f1 {
            gui.graph_add_value(g, 1, f.fx / load);
            if f.fx > max_fx {
                max_fx = f.fx;
                sr_at_max_fx = f.slip_ratio;
            }
        }
        let f2 = t.sr_sa_sweep(9.0, -1.0, 1.0, load, 200);
        gui.graph_clear_serie(g, 4);
        for f in &f2 {
            gui.graph_add_value(g, 4, f.fy / load);
        }
        gui.graph_clear_serie(g, 3);
        let mut x = 0.0f32;
        loop {
            gui.graph_add_value(g, 3, t.tyre.get_dy(x * 9.806));
            x += 10.0;
            if x >= 2000.0 {
                break;
            }
        }
        gui.graph_clear_serie(g, 5);
        let mut a = -20.0f32;
        loop {
            gui.graph_add_value(g, 5, (t.tyre.get_cambered_dy(a * 0.017453, 1.0) - 1.0) * 5.0);
            a += 0.2;
            if a > 20.0 {
                break;
            }
        }
        let d3 = t.tyre.get_dy(19612.0);
        let d2 = t.tyre.get_dy(9806.0);
        let d1 = t.tyre.get_dy(4903.0);
        let k = 1.0 / load;
        let f = |v: f32, n: usize| format_fixed(v as f64, n);
        let text = format!(
            "FY:{} @ {}   FX:{} @ {} MZ:{} @ {} DY @ 0.5k:{} 1k:{} 2k:{}",
            f(k * max_fy, 2),
            f(sa_at_max_fy, 1),
            f(k * max_fx, 2),
            f(sr_at_max_fx, 2),
            f(max_mz, 1),
            f(sa_at_max_mz, 1),
            f(d1, 1),
            f(d2, 1),
            f(d3, 1)
        );
        gui.set_text(self.label, &text);
        gui.control_render(id, dt, painter);
        // the friction circle at 9 degrees, a quarter of the form's size around its centre
        let gl = *self.gl.get_or_insert_with(|| painter.create_gl(2000));
        painter.gl_begin(gl, GL_LINE_STRIP);
        painter.gl_color3f(gl, 0.0, 1.0, 1.0);
        let r = gui.controls[id].rect;
        let (w, h) = (r.right - r.left, r.bottom - r.top);
        let (cx, cy) = (w * 0.5 + r.left, h * 0.5 + r.top);
        for p in &f2 {
            painter.gl_vertex3f(gl, ((k * p.fx) * w) * 0.25 + cx, ((k * p.fy) * h) * 0.25 + cy, 0.0);
        }
        painter.gl_end(gl);
    }
}
