// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! `CarPhysicsForm` (0x1f8), the developers' "Car physics": 35 numbers of the player's car
//! (`cars[0]`), its tyre compound and a graph of the force feedback.
//!
//! `CarPhysicsForm::CarPhysicsForm(CarAvatar*)` 0x1402d7680, `render` 0x1402d9500,
//! `RaceEngineer::getDynamicIndex` 0x14027bc60.

use std::any::Any;

use crate::apps::App;
use crate::control::Id;
use crate::font::{Font, FontType};
use crate::gui::Gui;
use crate::hud::translate;
use crate::painter::Painter;
use crate::widgets2::Connected;
use crate::widgets3::GraphReferenceAxis;

/// What the app reads beside the physics state: for `RaceEngineer::getDynamicIndex` the
/// `getBasePosition().z` of suspensions 0 and 2 and the body's `getMass` and
/// `getLocalInertia().y`; `CarAvatar::getKmPerLiter`; `CarAvatar::filteredSpeed`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CarPhysicsExtra {
    pub base_z: [f32; 2],
    pub mass: f32,
    pub iyy: f32,
    pub km_per_liter: f32,
    pub filtered_speed: f32,
}

/// `RaceEngineer::getDynamicIndex` 0x14027bc60, in the machine's order.
pub fn dynamic_index(e: &CarPhysicsExtra) -> f32 {
    let z2 = e.base_z[1].abs();
    let z0 = e.base_z[0].abs();
    e.iyy / ((z0 * z2) * e.mass)
}

const ROWS: [&str; 35] = [
    "CGH", "DY F", "DY R", "LD0", "LD1", "LD2", "LD3", "DISTR", "SA LF", "SA LR", "AVEL_Y", "GLAT", "GLON", "DI", "DSPEED", "BOOST", "VKM F", "VKM R", "AP", "ELIFE", "BOV", "DYN_TRK", "BOOST", "GRAIN F", "GRAIN R", "BLISTER F", "BLISTER R", "FLAT L", "FLAT R", "KERS", "KERS_KJ", "CONS", "NCP", "RPM", "FSPEED",
];

pub struct CarPhysics {
    tyre_name: Id,
    labels: [Id; 35],
    ff_graph: Id,
}

impl Gui {
    /// `CarPhysicsForm::CarPhysicsForm(CarAvatar*)` 0x1402d7680.
    pub fn new_car_physics_app(&mut self, painter: &mut dyn Painter) -> Id {
        let form = self.new_form(painter, "CAR_STATS", false);
        self.form_set_title(form, &translate("Car physics"));
        let tyre_name = self.new_label(painter, "TYRE:");
        self.set_position(tyre_name, 5.0, 32.0);
        self.set_size(tyre_name, 50.0, 32.0);
        self.control_add_control(form, tyre_name);
        let mut labels = [0; 35];
        for (i, name) in ROWS.iter().enumerate() {
            let l = self.new_connected_label(painter, name, Connected::Float(0.0));
            self.set_position(l, 5.0, 52.0 + 20.0 * i as f32);
            self.set_size(l, 50.0, 32.0);
            self.control_add_control(form, l);
            labels[i] = l;
        }
        let ff_graph = self.new_graph(painter, "FF");
        self.set_position(ff_graph, 5.0, 772.0);
        self.set_size(ff_graph, 160.0, 100.0);
        self.control_add_control(form, ff_graph);
        self.graph_add_serie(ff_graph, [0.0, 0.0, 1.0]);
        self.graph_mut(ff_graph).min_y = 0.0;
        self.graph_mut(ff_graph).max_y = 1.0;
        self.graph_mut(ff_graph).axes.push(GraphReferenceAxis { color: [1.0; 3], ref_value: 0.1, is_vertical: false });
        self.form_set_size(form, 170.0, 880.0);
        // (the form draws no text of its own: no visible effect)
        self.fonts.push(Font::new(painter, "", FontType::Monospaced, 20.0, false, false));
        self.controls[form].font = Some(self.fonts.len() - 1);
        self.set_app(form, Box::new(CarPhysics { tyre_name, labels, ff_graph }));
        form
    }
}

impl App for CarPhysics {
    fn as_any(&mut self) -> &mut dyn Any {
        self
    }

    /// `CarPhysicsForm::render` 0x1402d9500.
    fn render(&mut self, gui: &mut Gui, id: Id, dt: f32, painter: &mut dyn Painter) {
        let e = gui.apps.car_physics;
        let grip = gui.apps.grip;
        let (s, compound) = gui.apps.cars.first().map(|c| (c.state, c.compound_short_name.clone())).unwrap_or_default();
        let di = dynamic_index(&e);
        gui.graph_add_value(self.ff_graph, 0, s.last_ff_pure.abs());
        gui.set_text(self.tyre_name, &compound);
        let t = s.load[1] + s.load[0];
        let u = t + s.load[2] + s.load[3];
        let distr = (t / u) * 100.0;
        let values: [f32; 35] = [
            s.cg_height,
            s.dy[0],
            s.dy[2],
            s.load[0],
            s.load[1],
            s.load[2],
            s.load[3],
            distr,
            s.slip_angle[0],
            s.slip_angle[2],
            s.local_angular_velocity[1],
            s.acc_g[0],
            s.acc_g[2],
            di,
            s.drivetrain_speed,
            s.turbo_boost,
            s.tyre_virtual_km[0],
            s.tyre_virtual_km[2],
            s.air_density,
            s.engine_life_left,
            s.turbo_bov,
            grip,
            s.turbo_boost_level,
            s.tyre_grain[0],
            s.tyre_grain[2],
            s.tyre_blister[0],
            s.tyre_blister[2],
            s.tyre_flat_spot[0],
            s.tyre_flat_spot[1],
            s.kers_charge,
            s.kers_current_kj,
            e.km_per_liter,
            s.normalized_spline_position,
            s.engine_rpm,
            e.filtered_speed,
        ];
        for (l, v) in self.labels.iter().zip(values) {
            gui.connected_label_mut(*l).value = Connected::Float(v);
        }
        gui.control_render(id, dt, painter);
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn dynamic_index_of_point_masses_is_one() {
        let e = super::CarPhysicsExtra { base_z: [1.5, -1.0], mass: 1000.0, iyy: 1500.0, ..Default::default() };
        assert_eq!(super::dynamic_index(&e), 1.0);
    }
}
