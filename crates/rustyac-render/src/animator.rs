// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! Cars with animated suspensions (`car.ini [GRAPHICS] USE_ANIMATED_SUSPENSIONS=1`):
//! placeholder until the port of `SuspensionAnimator`.

use std::path::Path;

use crate::car::CarPhysicsState;
use crate::scene::{NodeId, Scene};

pub struct SuspensionAnimator {}

impl SuspensionAnimator {
    pub fn new(_folder: &Path) -> SuspensionAnimator {
        SuspensionAnimator {}
    }

    pub fn add_model(&mut self, _scene: &mut Scene, _root: NodeId) {}

    pub fn update(&mut self, _scene: &mut Scene, _body: NodeId, _state: &CarPhysicsState, _dt: f32) {}
}
