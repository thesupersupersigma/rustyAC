// SPDX-License-Identifier: BSD-3-Clause

//! Box against triangle mesh: `ode/src/collision_trimesh_box.cpp` (`dCollideBTL`) as compiled
//! into `acs.exe`.

use crate::contact::ContactGeom;
use crate::geom::{BoxPose, GeomRef, MeshPose};

/// `dCollideBTL` @ 0x14038a3d0: the contacts of the box with the mesh, appended to `out`
/// (`g1` is the mesh, `g2` the box).
pub fn collide_btl(_mesh: &MeshPose, _box: &BoxPose, _flags: u32, _g1: GeomRef, _g2: GeomRef, _out: &mut Vec<ContactGeom>) {
    unimplemented!("dCollideBTL")
}
