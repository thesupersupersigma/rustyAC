// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! `ACSuspension` (the developers' Suspensions app): per wheel the slip bar, the slip angle
//! line and camber / toe texts, the caster, KPI and scrub, the rear bump-steer plot, the tyres'
//! temperature grids and how far each wheel's node sits from its tyre. Its 3D lines are
//! `rustyac_render::suspensions`.

use std::any::Any;
use std::rc::Rc;

use rustyac_physics::vecmath::{xm_matrix_inverse, Mat44f, Vec3f};

use crate::acos::format_g6;
use crate::app_wings::cvtt;
use crate::apps::{App, AppAction};
use crate::control::{Clicked, Id};
use crate::gui::Gui;
use crate::hud::translate;
use crate::painter::{GlId, Painter, ALIGN_CENTER, ALIGN_LEFT, GL_LINES, GL_LINE_STRIP, GL_QUADS};
use crate::widgets2::format_fixed;

/// The player's car facts `CarAvatar::initPhysics` copies into `physicsInfo`, and the car.ini
/// fuel tank the constructor reads.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SuspensionFacts {
    /// `caster`: `RaceEngineer::getFrontCasterRAD` in degrees
    pub caster: f32,
    /// `kpi.angleRAD`, `kpi.scrubRadius`
    pub kpi_rad: f32,
    pub scrub: f32,
    /// `susBasePos`
    pub sus_base_pos: [[f32; 3]; 4],
    /// `ridePickupPoint`
    pub ride_pickup_point: [[f32; 3]; 2],
    /// `tyreWidth`, the tyres' `data.radius`
    pub tyre_width: [f32; 4],
    pub tyre_radius: [f32; 4],
    /// car.ini `[FUELTANK] POSITION`
    pub fuel_tank_position: [f32; 3],
}

/// What the app reads in a frame besides the player's `AppCar`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SuspensionLink {
    pub facts: SuspensionFacts,
    /// `SuspensionAvatar::getWheelTransform(i)`'s world translation
    pub wheel_nodes: [[f32; 3]; 4],
}

pub struct SuspensionApp {
    bars: [Id; 4],
    rear_bump_steer: Vec<f32>,
    /// `susGL`
    gl: GlId,
}

impl Gui {
    /// `ACSuspension::ACSuspension(CarAvatar*)` 0x1402e3fd0: 500 x 700, the TempRun button and
    /// four slip bars.
    pub fn new_suspension_app(&mut self, painter: &mut dyn Painter) -> Id {
        let form = self.new_form(painter, "ACSuspension", false);
        // shortcut: the game's susGL is one static for every instance; one app is ever made
        let gl = painter.create_gl(0x200);
        self.form_set_title(form, &translate("Suspensions"));
        self.form_set_size(form, 500.0, 700.0);
        let button = self.new_control(painter, "TEMP_RUN");
        self.set_size(button, 200.0, 20.0);
        self.set_text(button, "Start TempRun");
        self.set_position(button, 10.0, 680.0);
        self.control_add_control(form, button);
        // (lambda 0x1402e4770: the job on the physics thread, then the text)
        let recording = std::cell::Cell::new(false);
        self.controls[button].ev_clicked.push(Rc::new(move |gui: &mut Gui, _: &Clicked| {
            gui.apps.actions.push(AppAction::TempRun(!recording.get()));
            recording.set(!recording.get());
            gui.set_text(button, if recording.get() { "STOP TempRun" } else { "Start TempRun" });
        }));
        let bars = [(20.0, 240.0), (420.0, 240.0), (20.0, 260.0), (420.0, 260.0)].map(|(x, y)| {
            let bar = self.new_progress_bar(painter, "PRG1");
            self.control_add_control(form, bar);
            let p = self.progress_bar_mut(bar);
            p.min_value = 0.0;
            p.max_value = 1.0;
            self.set_size(bar, 60.0, 10.0);
            self.progress_bar_mut(bar).is_vertical = false;
            self.set_position(bar, x, y);
            bar
        });
        self.set_app(form, Box::new(SuspensionApp { bars, rear_bump_steer: vec![0.0; 400], gl }));
        form
    }
}

/// `Node::worldToLocal` 0x14020e730.
fn world_to_local(inverse: &Mat44f, v: [f32; 3]) -> [f32; 3] {
    let m = &inverse.m;
    let [x, y, z] = v;
    [(((y * m[1][0]) + (x * m[0][0])) + (z * m[2][0])) + m[3][0], (((x * m[0][1]) + (y * m[1][1])) + (z * m[2][1])) + m[3][1], (((x * m[0][2]) + (y * m[1][2])) + (z * m[2][2])) + m[3][2]]
}

/// `Node::worldToLocalNormal` 0x14020e8d0.
fn world_to_local_normal(inverse: &Mat44f, v: [f32; 3]) -> [f32; 3] {
    let m = &inverse.m;
    let [x, y, z] = v;
    std::array::from_fn(|c| ((y * m[1][c]) + (x * m[0][c])) + (z * m[2][c]))
}

impl SuspensionApp {
    /// `ACSuspension::renderCamber` 0x1402e5f90.
    fn render_camber(&mut self, gui: &mut Gui, font: usize, painter: &mut dyn Painter, c: [f32; 2], i: usize, facts: &SuspensionFacts) {
        let gl = self.gl;
        let car = &gui.apps.cars[0];
        let s = car.state;
        let body_inverse = xm_matrix_inverse(&car.body_matrix);
        let bar = gui.progress_bar_mut(self.bars[i]);
        bar.value = s.nd_slip[i];
        let fore = if 1.0 > bar.value { [1.0, 1.0, 0.0, 1.0] } else { [1.0, 0.0, 0.0, 1.0] };
        gui.controls[self.bars[i]].fore_color = fore;
        painter.gl_begin(gl, GL_LINES);
        painter.gl_color3f(gl, 1.0, 1.0, 1.0);
        painter.gl_vertex3f(gl, c[0] - 50.0, c[1], 0.0);
        painter.gl_vertex3f(gl, c[0] + 50.0, c[1], 0.0);
        painter.gl_color3f(gl, 1.0, 0.0, 1.0);
        let r = Mat44f::create_from_axis_angle(&Vec3f::new(0.0, 0.0, 1.0), -s.slip_angle[i] * 0.017453).m;
        painter.gl_vertex3f(gl, c[0], c[1], 0.0);
        let up: [f32; 3] = std::array::from_fn(|k| ((r[0][k] * 0.0 + r[1][k]) + r[2][k] * 0.0) + r[3][k]);
        painter.gl_vertex3f(gl, c[0] - up[0] * 100.0, c[1] - up[1] * 100.0, 0.0 - up[2] * 100.0);
        painter.gl_end(gl);
        let f = &mut gui.fonts[font];
        f.set_color(1.0, 0.0, 1.0, 1.0);
        f.blit_string(painter, c[0], c[1] + 20.0, &format!("SA: {}", format_fixed(s.slip_angle[i] as f64, 1)), 1.0, ALIGN_CENTER);
        f.set_color(0.0, 1.0, 0.0, 1.0);
        f.blit_string(painter, c[0], c[1] + 40.0, &format!("SR: {}", format_fixed((s.slip_ratio[i] * 100.0) as f64, 1)), 1.0, ALIGN_CENTER);
        let sm = &s.suspension_matrix[i].m;
        let n = world_to_local_normal(&body_inverse, [-sm[2][0], -sm[2][1], -sm[2][2]]);
        let (mut x, z) = (n[0], n[2]);
        let len = (z * z + x * x).sqrt();
        if len != 0.0 {
            x *= 1.0 / len;
        }
        let mut toe = rustyac_math::asinf(x) * 57.29578;
        if i & 1 != 0 {
            toe = -toe;
        }
        let p = world_to_local(&body_inverse, [sm[3][0], sm[3][1], sm[3][2]]);
        if i == 2 {
            let half = (self.rear_bump_steer.len() / 2) as f32;
            let idx = cvtt(half + (p[1] - facts.sus_base_pos[i][1]) * 300.0);
            if idx >= 0 && (idx as usize) < self.rear_bump_steer.len() {
                self.rear_bump_steer[idx as usize] = toe;
            }
        }
        f.set_color(0.0, 1.0, 1.0, 1.0);
        let mut text = format!("C: {} T:{}", format_fixed((s.camber_rad[i] * 57.29578) as f64, 1), format_fixed(toe as f64, 1));
        if i < 2 {
            text += &format!(" [STR:{}]", format_fixed((s.steer / toe) as f64, 1));
        }
        f.blit_string(painter, c[0], c[1] + 60.0, &text, 1.0, ALIGN_CENTER);
    }

    /// `ACSuspension::renderTyreTemp` 0x1402e6930.
    fn render_tyre_temp(&self, gui: &mut Gui, font: usize, painter: &mut dyn Painter, p: [f32; 2], i: usize) {
        let gl = self.gl;
        let t = gui.apps.cars[0].state.tyre_thermal_states[i];
        // the game reads the 36 temperatures flat: ring c's row r at 3c + r
        let temps = t.temps.as_flattened();
        let mut sums = [0.0f32; 3];
        painter.gl_begin(gl, GL_QUADS);
        for (r, sum) in sums.iter_mut().enumerate() {
            for c in 0..12 {
                let v = temps[3 * c + r];
                *sum += v;
                let k = (v - 26.0) * 0.01;
                painter.gl_color3f(gl, k, 0.0, 1.0 - k);
                painter.gl_quad(gl, c as f32 * 10.0 + p[0], r as f32 * 10.0 + p[1], 10.0, 10.0, false);
            }
        }
        painter.gl_end(gl);
        let k = |v: f32| cvtt(v * 0.083333336);
        let text = format!("IMO {} {} {} I:{} P:{} CP:{}", k(sums[0]), k(sums[1]), k(sums[2]), cvtt(t.thermal_input), cvtt(t.dynamic_pressure), cvtt(t.cp_temperature));
        gui.fonts[font].blit_string(painter, p[0], p[1] + 35.0, &text, 1.0, ALIGN_LEFT);
    }
}

impl App for SuspensionApp {
    fn as_any(&mut self) -> &mut dyn Any {
        self
    }

    /// `ACSuspension::render` 0x1402e4ad0.
    fn render(&mut self, gui: &mut Gui, id: Id, dt: f32, painter: &mut dyn Painter) {
        gui.control_render(id, dt, painter);
        let (Some(link), false) = (gui.apps.suspension, gui.apps.cars.is_empty()) else { return };
        let facts = link.facts;
        let gl = self.gl;
        let font = gui.controls[id].font.unwrap_or(gui.font);
        let (x, y) = (gui.controls[id].rect.left, gui.controls[id].rect.top);
        for (i, (dx, dy)) in [(50.0, 150.0), (450.0, 150.0), (50.0, 550.0), (450.0, 550.0)].into_iter().enumerate() {
            self.render_camber(gui, font, painter, [x + dx, y + dy], i, &facts);
        }
        let text = format!("{}: {}\nKPI:{}deg\nSCRUB:{}mm", translate("CASTER"), format_fixed(facts.caster as f64, 1), format_fixed((facts.kpi_rad * 57.29578) as f64, 1), format_fixed((facts.scrub * 1000.0) as f64, 1));
        gui.fonts[font].set_color(1.0, 1.0, 1.0, 1.0);
        gui.fonts[font].blit_string(painter, x + 250.0, y + 200.0, &text, 1.0, ALIGN_CENTER);
        painter.gl_color3f(gl, 1.0, 1.0, 1.0);
        painter.gl_begin(gl, GL_LINE_STRIP);
        for (i, v) in self.rear_bump_steer.iter().enumerate() {
            painter.gl_vertex3f(gl, (v * 100.0) + (x + 250.0), i as f32 + (y + 310.0), 0.0);
        }
        painter.gl_end(gl);
        for (i, (dx, dy)) in [(40.0, 290.0), (320.0, 290.0), (40.0, 350.0), (320.0, 350.0)].into_iter().enumerate() {
            self.render_tyre_temp(gui, font, painter, [x + dx, y + dy], i);
        }
        gui.fonts[font].set_color(1.0, 0.0, 0.0, 1.0);
        let tyre_matrix = gui.apps.cars[0].state.tyre_matrix;
        for (i, name) in ["  FL ", "  FR ", "  RL ", "  RR "].into_iter().enumerate() {
            let n = link.wheel_nodes[i];
            let t = &tyre_matrix[i].m[3];
            let (dx, dy, dz) = (n[0] - t[0], n[1] - t[1], n[2] - t[2]);
            let s = ((dy * dy) + (dx * dx)) + (dz * dz);
            let len = if s == 0.0 { 0.0 } else { s.sqrt() };
            let text = format!("{}{name}{} : x={} | y={} | z={}", translate("Tyre off"), format_g6(len), format_g6(dx), format_g6(dy), format_g6(dz));
            gui.fonts[font].blit_string(painter, 10.0, 10.0 + 20.0 * i as f32, &text, 1.0, ALIGN_LEFT);
        }
    }
}
