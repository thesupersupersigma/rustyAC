// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! `ACSuspension`'s 3D lines (its handler of the render-finished node, lambda 0x1402e4980 =
//! `ACSuspension::renderWireframe` 0x1402e6da0): while the Suspensions app shows, in the
//! opaque pass, the suspensions' debug lines with the car's centre, the ride pickup points,
//! the tyres' circles the keypad picks and the fuel tank. Backspace, Ctrl and keypad 1-4 switch
//! the parts, each on its key's press.

use rustyac_physics::vecmath::Mat44f;

use crate::gl::{GlRenderer, GL_LINES, GL_LINE_STRIP, GL_QUADS};
use crate::graphics::{Graphics, CULL_FRONT, CULL_NONE, DEPTH_NORMAL, DEPTH_OFF};
use crate::scene::{NodeEventHandler, OnNodeRenderEvent, Scene};

/// A suspension's `DebugLine`: from, to, colour.
pub type Line = ([f32; 3], [f32; 3], [f32; 4]);

/// What the handler draws in a frame, the player's car's.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SuspensionFrame {
    /// `physicsState.worldMatrix`
    pub world: Mat44f,
    /// `physicsState.tyreMatrix`
    pub tyre_matrix: [Mat44f; 4],
    /// `CarAvatar::getSuspensionDebugLines(w)`, already in the world
    pub lines: [Vec<Line>; 4],
    /// `physicsInfo.ridePickupPoint`, car coordinates
    pub ride_pickup_point: [[f32; 3]; 2],
    /// `physicsInfo.tyreWidth`
    pub tyre_width: [f32; 4],
    /// the tyres' `data.radius`
    pub tyre_radius: [f32; 4],
    /// car.ini `[FUELTANK] POSITION`
    pub fuel_tank_position: [f32; 3],
    /// the wheel nodes' world translations (`SuspensionAvatar::getWheelTransform`)
    pub wheel_nodes: [[f32; 3]; 4],
    /// held: Backspace, Ctrl, keypad 1-4
    pub keys: [bool; 6],
}

/// The handler: its switches persist, the frame is the app's of this frame (None: hidden).
pub struct SuspensionWireframe {
    pub frame: Option<SuspensionFrame>,
    pub render_suspensions: bool,
    pub render_fuel_tank_position: bool,
    pub render_wheels: [bool; 4],
    /// `Trigger::ignoreSubsequentTrue`'s last input per key
    held: [bool; 6],
    /// `susGL`: 0x200 vertices, made with the app
    gl: GlRenderer,
}

impl SuspensionWireframe {
    /// `ACSuspension::ACSuspension`'s part: the switches' defaults and `susGL`.
    pub fn new(graphics: &Graphics) -> Self {
        SuspensionWireframe { frame: None, render_suspensions: true, render_fuel_tank_position: true, render_wheels: [false; 4], held: [false; 6], gl: GlRenderer::new(graphics, 0x200) }
    }
}

fn translation(m: &Mat44f) -> [f32; 3] {
    [m.m[3][0], m.m[3][1], m.m[3][2]]
}

impl NodeEventHandler for SuspensionWireframe {
    fn on_node_render(&mut self, _scene: &mut Scene, graphics: &mut Graphics, event: &OnNodeRenderEvent) {
        let Some(f) = &self.frame else { return };
        if event.pass_id != 0 {
            return;
        }
        let gl = &mut self.gl;
        graphics.set_world_matrix(&Mat44f::IDENTITY);
        graphics.set_depth_mode(DEPTH_OFF);
        // Trigger::ignoreSubsequentTrue: a press switches once
        for (k, &down) in f.keys.iter().enumerate() {
            if down && !self.held[k] {
                match k {
                    0 => self.render_suspensions = !self.render_suspensions,
                    1 => self.render_fuel_tank_position = !self.render_fuel_tank_position,
                    w => self.render_wheels[w - 2] = !self.render_wheels[w - 2],
                }
            }
            self.held[k] = down;
        }
        if self.render_suspensions {
            gl.begin(GL_LINES, None);
            for (p0, p1, c) in f.lines.iter().flatten() {
                gl.color4f(c[0], c[1], c[2], c[3]);
                gl.vertex3fv(p0);
                gl.vertex3fv(p1);
            }
            gl.color3f(0.0, 1.0, 1.0);
            let [x, y, z] = translation(&f.world);
            for v in [[x, y + 0.5, z], [x, y - 0.5, z], [x + 0.5, y, z], [x - 0.5, y, z], [x, y, z + 0.5], [x, y, z - 0.5]] {
                gl.vertex3fv(&v);
            }
            gl.end(graphics);
            graphics.set_world_matrix(&f.world);
            let [r0, r1] = f.ride_pickup_point;
            gl.begin(GL_LINES, None);
            gl.color3f(1.0, 0.0, 1.0);
            gl.vertex3fv(&r0);
            gl.vertex3fv(&r1);
            gl.end(graphics);
            graphics.set_depth_mode(DEPTH_NORMAL);
            graphics.set_world_matrix(&f.world);
            graphics.set_cull_mode(CULL_NONE);
            gl.color3f(0.0, 1.0, 1.0);
            gl.begin(GL_QUADS, None);
            gl.vertex3f(r0[0] - 0.2, r0[1], r0[2]);
            gl.vertex3f(r1[0] - 0.2, r1[1], r1[2]);
            gl.vertex3f(r1[0] + 0.2, r1[1], r1[2]);
            gl.vertex3f(r0[0] + 0.2, r0[1], r0[2]);
            gl.end(graphics);
        }
        graphics.set_depth_mode(DEPTH_OFF);
        for w in 0..4 {
            if !self.render_wheels[w] {
                continue;
            }
            graphics.set_world_matrix(&f.tyre_matrix[w]);
            gl.color3f(0.0, 3.0, 0.0);
            let r = f.tyre_radius[w];
            let hw = f.tyre_width[w] * 0.5;
            for sx in [hw, -hw] {
                gl.begin(GL_LINE_STRIP, None);
                let mut a = 0.0f32;
                loop {
                    gl.vertex3f(sx, rustyac_math::cosf(a) * r, rustyac_math::sinf(a) * r);
                    a += 0.062831804;
                    #[allow(clippy::approx_constant)] // the game's literal, not TAU
                    if a >= 6.28318 {
                        break;
                    }
                }
                gl.end(graphics);
                gl.begin(GL_LINES, None);
                gl.vertex3f(sx, r, 0.0);
                gl.vertex3f(sx, -r, 0.0);
                gl.end(graphics);
                gl.begin(GL_LINES, None);
                gl.vertex3f(sx, 0.0, r);
                gl.vertex3f(sx, 0.0, -r);
                gl.end(graphics);
            }
            graphics.set_world_matrix(&Mat44f::IDENTITY);
            gl.color4f(1.0, 1.0, 1.0, 1.0);
            gl.begin(GL_LINES, None);
            gl.vertex3fv(&f.wheel_nodes[w]);
            gl.vertex3fv(&translation(&f.tyre_matrix[w]));
            gl.end(graphics);
        }
        if self.render_fuel_tank_position {
            gl.color4f(3.0, 0.0, 0.0, 1.0);
            graphics.set_world_matrix(&f.world);
            gl.begin(GL_LINES, None);
            let [x, y, z] = f.fuel_tank_position;
            for v in [[x, y - 0.5, z], [x, y + 0.5, z], [x - 0.5, y, z], [x + 0.5, y, z], [x, y, z - 0.5], [x, y, z + 0.5]] {
                gl.vertex3fv(&v);
            }
            gl.end(graphics);
        }
        graphics.set_cull_mode(CULL_FRONT);
        graphics.set_world_matrix(&Mat44f::IDENTITY);
        graphics.set_depth_mode(DEPTH_NORMAL);
    }
}
