// SPDX-License-Identifier: BSD-3-Clause

//! Triangle mesh against triangle mesh: `ode/src/collision_trimesh_trimesh_new.cpp`
//! (`dCollideTTL`) as compiled into `acs.exe`.

use crate::contact::ContactGeom;
use crate::geom::{GeomRef, MeshPose};

/// `dCollideTTL` @ 0x14038c0d0: the contacts of two meshes, appended to `out`.
pub fn collide_ttl(_m1: &MeshPose, _m2: &MeshPose, _flags: u32, _g1: GeomRef, _g2: GeomRef, _out: &mut Vec<ContactGeom>) {
    unimplemented!("dCollideTTL")
}
