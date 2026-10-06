//! AC's `AntirollBar`: pushes the left and right hub of an axle towards the same height.

use super::body::{ForceSource, PhysicsCore, RigidBody};
use super::suspension::SuspensionModel;
use crate::math::sqrtf;
use crate::vecmath::Vec3f;

/// AC's `AntirollBar` (0x48 bytes) without its optional `DynamicController` (`ctrl`, from
/// `ctrl_arb_front.ini` / `ctrl_arb_rear.ini`: 2 of the 113 cars; not ported, a later task).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct AntirollBar {
    /// `k`: N per metre of height difference between the two hubs.
    pub k: f32,
}

/// `ucomiss x, 0` + `je`: true for an ordered value that is not zero.
#[allow(clippy::double_comparisons)]
fn ordered_nonzero(x: f32) -> bool {
    x < 0.0 || x > 0.0
}

impl AntirollBar {
    /// `AntirollBar::step` @ 0x1402bb640. `left` and `right` are `hubs[0]` and `hubs[1]`.
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    pub fn step(
        &mut self,
        core: &mut PhysicsCore,
        body: RigidBody,
        left: &mut dyn SuspensionModel,
        right: &mut dyn SuspensionModel,
        _dt: f32,
    ) {
        // `comiss 0, k` + `jae`: skipped when 0 >= k (a NaN goes on)
        if 0.0 >= self.k {
            return;
        }
        let m0 = left.get_hub_world_matrix(core);
        let p0 = Vec3f::new(m0.m[3][0], m0.m[3][1], m0.m[3][2]);
        let m1 = right.get_hub_world_matrix(core);
        let p1 = Vec3f::new(m1.m[3][0], m1.m[3][1], m1.m[3][2]);
        let l0 = core.world_to_local(body, &p0);
        let l1 = core.world_to_local(body, &p1);
        let delta = l1.y - l0.y;
        let matrix = core.get_world_matrix(body);
        let mut n = Vec3f::new(matrix.m[1][0], matrix.m[1][1], matrix.m[1][2]);
        let length = sqrtf((n.y * n.y + n.x * n.x) + n.z * n.z);
        if ordered_nonzero(length) {
            let inverse = 1.0 / length;
            n = Vec3f::new(n.x * inverse, n.y * inverse, n.z * inverse);
        }
        let k = self.k;
        let previous = core.source;
        core.source = ForceSource::Arb;
        let force = Vec3f::new((n.x * delta) * k, (n.y * delta) * k, (n.z * delta) * k);
        left.add_force_at_pos(core, &force, &p0, false, false);
        core.add_local_force_at_local_pos(body, &Vec3f::new(0.0, -(delta * k), 0.0), &l0);
        let negative = -delta;
        let force = Vec3f::new((n.x * negative) * k, (n.y * negative) * k, (n.z * negative) * k);
        right.add_force_at_pos(core, &force, &p1, false, false);
        core.add_local_force_at_local_pos(body, &Vec3f::new(0.0, delta * k, 0.0), &l1);
        core.source = previous;
    }
}
