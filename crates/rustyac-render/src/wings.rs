// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! `FormWings::onRenderNode` 0x1402edc30: while the developers' Wings app shows, every wing's
//! outline (green, a fin's yellow) and its lift vector (purple) over the player's car, drawn
//! at the render-finished node in the opaque pass with the depth test off. Its outline lambda
//! is 0x1402ed330. The app's table is `rustyac_gui::app_wings`.

use crate::gl::{GL_LINES, GL_LINE_STRIP};
use crate::graphics::{Graphics, DEPTH_NORMAL, DEPTH_OFF};
use crate::scene::{NodeEventHandler, OnNodeRenderEvent, Scene};

/// One wing as the handler reads it: `wingData[i]`'s `position`, `span`, `chord`, `isVertical`
/// and `wingsStatus[i].liftVector`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct WingOutline {
    pub position: [f32; 3],
    pub span: f32,
    pub chord: f32,
    pub is_vertical: bool,
    pub lift_vector: [f32; 3],
}

/// What a frame draws: the player's car's `physicsState.worldMatrix` and its wings. `None`:
/// the app is hidden (or the car has no wing) and nothing is drawn.
#[derive(Default)]
pub struct WingsOutlines {
    pub frame: Option<([[f32; 4]; 4], Vec<WingOutline>)>,
}

/// The game's point times the world matrix: `((x M1 + y M2) + z M3) + M4`.
fn transform(m: &[[f32; 4]; 4], p: [f32; 3]) -> [f32; 3] {
    std::array::from_fn(|c| ((p[0] * m[0][c] + p[1] * m[1][c]) + p[2] * m[2][c]) + m[3][c])
}

impl NodeEventHandler for WingsOutlines {
    fn on_node_render(&mut self, _scene: &mut Scene, graphics: &mut Graphics, event: &OnNodeRenderEvent) {
        let Some((m, wings)) = &self.frame else { return };
        if wings.is_empty() || event.pass_id != 0 {
            return;
        }
        let mut gl = graphics.gl.take().expect("the graphics manager's GL renderer");
        graphics.set_depth_mode(DEPTH_OFF);
        gl.color3f(0.0, 1.0, 0.0);
        for w in wings {
            let [x, y, z] = w.position;
            let (s0, s1, c0, c1) = (w.span * -0.5, w.span * 0.5, w.chord * -0.5, w.chord * 0.5);
            let corners = if w.is_vertical {
                gl.color3f(4.0, 4.0, 0.0);
                [[x, s0 + y, c0 + z], [x, s1 + y, c0 + z], [x, s1 + y, c1 + z], [x, s0 + y, c1 + z], [x, s0 + y, c0 + z]]
            } else {
                gl.color3f(0.0, 4.0, 0.0);
                [[s0 + x, y, c0 + z], [s1 + x, y, c0 + z], [s1 + x, y, c1 + z], [s0 + x, y, c1 + z], [s0 + x, y, c0 + z]]
            };
            gl.begin(GL_LINE_STRIP, None);
            for p in corners {
                gl.vertex3fv(&transform(m, p));
            }
            gl.end(graphics);
        }
        gl.begin(GL_LINES, None);
        gl.color3f(5.0, 0.0, 5.0);
        for w in wings {
            gl.vertex3fv(&transform(m, w.position));
            let tip: [f32; 3] = std::array::from_fn(|k| w.lift_vector[k] * 0.001 + w.position[k]);
            gl.vertex3fv(&transform(m, tip));
        }
        gl.end(graphics);
        graphics.gl = Some(gl);
        graphics.set_depth_mode(DEPTH_NORMAL);
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_transform_is_a_row_vector_times_the_matrix() {
        let mut m = [[0.0f32; 4]; 4];
        m[0][1] = 1.0; // x -> y
        m[1][2] = 1.0; // y -> z
        m[2][0] = 1.0; // z -> x
        m[3] = [10.0, 20.0, 30.0, 1.0];
        assert_eq!(super::transform(&m, [1.0, 2.0, 3.0]), [13.0, 21.0, 32.0]);
    }
}
