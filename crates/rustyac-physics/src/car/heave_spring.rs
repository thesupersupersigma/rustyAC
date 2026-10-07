// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni). Not covered by rustyAC's licenses — see LICENSING.md.

//! AC's `HeaveSpring`: the third spring of an axle (Formula cars). It acts on the average
//! height of the left and right hub, pushes each hub with the **full** force and the body
//! twice, and may pull as well as push.

use std::path::Path;

use super::body::{ForceSource, PhysicsCore, RigidBody};
use super::suspension::{Damper, SuspensionModel};
use crate::data::ini::IniReader;
use crate::vecmath::Vec3f;

/// AC's `HeaveSpring` (0x58 bytes). The two `Suspension*` and the `Car*` of the original are
/// handed to [`HeaveSpring::step`] instead.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HeaveSpring {
    /// `isPresent`: the car's file has the section.
    pub is_present: bool,
    /// `rodLength`, m. Replaced every step by the mean of the two wheels' rod lengths unless
    /// both wheel springs are zero.
    pub rod_length: f32,
    /// `status.travel`, m
    pub travel: f32,
    /// `k`, N/m. Zero switches the spring off.
    pub k: f32,
    /// `progressiveK`
    pub progressive_k: f32,
    /// `packerRange`, m
    pub packer_range: f32,
    /// `bumpStopRate`: the packer's rate, N/m
    pub bump_stop_rate: f32,
    /// `bumpStopUp`, m
    pub bump_stop_up: f32,
    /// `bumpStopDn`, m (negative)
    pub bump_stop_dn: f32,
    pub damper: Damper,
    /// `isFront`
    pub is_front: bool,
}

impl Default for HeaveSpring {
    /// `HeaveSpring::HeaveSpring` @ 0x14026daf0.
    fn default() -> HeaveSpring {
        HeaveSpring {
            is_present: false,
            rod_length: 0.0,
            travel: 0.0,
            k: 0.0,
            progressive_k: 0.0,
            packer_range: 0.0,
            bump_stop_rate: 0.0,
            bump_stop_up: 0.0,
            bump_stop_dn: 0.0,
            damper: Damper::default(),
            is_front: false,
        }
    }
}

/// `ucomiss x, 0` + `je`: true for an ordered value that is not zero.
#[allow(clippy::double_comparisons)]
fn ordered_nonzero(x: f32) -> bool {
    x < 0.0 || x > 0.0
}

impl HeaveSpring {
    /// `HeaveSpring::init` @ 0x1402b3310 and `HeaveSpring::initData` @ 0x1402b3340:
    /// `[HEAVE_FRONT]` / `[HEAVE_REAR]` of suspensions.ini.
    pub fn init(&mut self, data_path: &Path, is_front: bool) -> Result<(), String> {
        self.is_front = is_front;
        self.rod_length = 0.0;
        self.k = 0.0;
        self.progressive_k = 0.0;
        let ini = IniReader::load(&data_path.join("suspensions.ini"))?;
        if !ini.ready {
            return Ok(());
        }
        let section = if is_front { "HEAVE_FRONT" } else { "HEAVE_REAR" };
        if !ini.has_section(section) {
            self.is_present = false;
            return Ok(());
        }
        self.is_present = true;
        self.bump_stop_up = ini.get_float(section, "BUMPSTOP_UP")?;
        self.bump_stop_dn = -ini.get_float(section, "BUMPSTOP_DN")?;
        self.rod_length = ini.get_float(section, "ROD_LENGTH")?;
        self.k = ini.get_float(section, "SPRING_RATE")?;
        self.progressive_k = ini.get_float(section, "PROGRESSIVE_SPRING_RATE")?;
        self.damper = Damper::load(&ini, section)?;
        self.bump_stop_rate = ini.get_float(section, "BUMP_STOP_RATE")?;
        if !ordered_nonzero(self.bump_stop_rate) {
            self.bump_stop_rate = 500000.0;
        }
        self.packer_range = ini.get_float(section, "PACKER_RANGE")?;
        Ok(())
    }

    /// `HeaveSpring::step` @ 0x1402b3960. `left` and `right` are the axle's two suspensions
    /// (double wishbone in the game), `body` is the car body.
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    pub fn step(
        &mut self,
        core: &mut PhysicsCore,
        body: RigidBody,
        left: &mut dyn SuspensionModel,
        right: &mut dyn SuspensionModel,
        _dt: f32,
    ) {
        let l0 = core.world_to_local(body, &core.get_position(left.hub()));
        let l1 = core.world_to_local(body, &core.get_position(right.hub()));
        let ref0 = left.get_base_position();
        let ref1 = right.get_base_position();
        if ordered_nonzero(left.base().k) || ordered_nonzero(right.base().k) {
            self.rod_length = (right.base().rod_length + left.base().rod_length) * 0.5;
        }
        let average_y = (l1.y + l0.y) * 0.5;
        let travel = (average_y - ref0.y) + self.rod_length;
        self.travel = travel;
        let mut force = (travel * self.progressive_k + self.k) * travel;
        if ordered_nonzero(self.packer_range) && travel > self.packer_range {
            force = force + (travel - self.packer_range) * self.bump_stop_rate;
        }
        let matrix = core.get_world_matrix(body);
        let up = Vec3f::new(matrix.m[1][0], matrix.m[1][1], matrix.m[1][2]);

        let previous = core.source;
        core.source = ForceSource::HeaveSpring;
        Self::push(core, body, left, right, &up, force, &ref0, &ref1);

        core.source = ForceSource::HeaveBumpstop;
        let d = average_y - ref0.y;
        if ordered_nonzero(self.bump_stop_up) && d > self.bump_stop_up {
            let force = (d - self.bump_stop_up) * 500000.0;
            Self::push(core, body, left, right, &up, force, &ref0, &ref1);
        }
        if ordered_nonzero(self.bump_stop_dn) && !(d >= self.bump_stop_dn) {
            let force = (d - self.bump_stop_dn) * 500000.0;
            Self::push(core, body, left, right, &up, force, &ref0, &ref1);
        }

        core.source = ForceSource::HeaveDamper;
        let v1 = core.get_velocity(right.hub());
        let v0 = core.get_velocity(left.hub());
        let hub_x = (v1.x + v0.x) * 0.5;
        let hub_y = (v0.y + v1.y) * 0.5;
        let hub_z = (v0.z + v1.z) * 0.5;
        let p1 = core.get_local_point_velocity(body, &ref1);
        let p0 = core.get_local_point_velocity(body, &ref0);
        let body_y = (p0.y + p1.y) * 0.5;
        let body_x = (p0.x + p1.x) * 0.5;
        let body_z = (p0.z + p1.z) * 0.5;
        let speed = ((hub_y - body_y) * up.y + (hub_x - body_x) * up.x) + (hub_z - body_z) * up.z;
        let f = self.damper.get_force(speed);
        let on_hub = Vec3f::new(up.x * f, up.y * f, up.z * f);
        let pos = core.get_position(left.hub());
        left.add_force_at_pos(core, &on_hub, &pos, false, false);
        let pos = core.get_position(right.hub());
        right.add_force_at_pos(core, &on_hub, &pos, false, false);
        let on_body = Vec3f::new(on_hub.x * -1.0, on_hub.y * -1.0, on_hub.z * -1.0);
        core.add_force_at_local_pos(body, &on_body, &ref0);
        core.add_force_at_local_pos(body, &on_body, &ref1);
        core.source = previous;
    }

    /// One spring-like force: `-force` along the body's up axis on each hub, `(0, force, 0)`
    /// in body axes on the body at both reference points.
    #[allow(clippy::too_many_arguments)]
    fn push(
        core: &mut PhysicsCore,
        body: RigidBody,
        left: &mut dyn SuspensionModel,
        right: &mut dyn SuspensionModel,
        up: &Vec3f,
        force: f32,
        ref0: &Vec3f,
        ref1: &Vec3f,
    ) {
        let n = -force;
        let on_hub = Vec3f::new(up.x * n, up.y * n, up.z * n);
        let pos = core.get_position(left.hub());
        left.add_force_at_pos(core, &on_hub, &pos, false, false);
        let pos = core.get_position(right.hub());
        right.add_force_at_pos(core, &on_hub, &pos, false, false);
        let on_body = Vec3f::new(0.0, force, 0.0);
        core.add_local_force_at_local_pos(body, &on_body, ref0);
        core.add_local_force_at_local_pos(body, &on_body, ref1);
    }
}
