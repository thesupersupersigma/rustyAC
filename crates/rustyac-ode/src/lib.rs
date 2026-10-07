// SPDX-License-Identifier: BSD-3-Clause

//! rustyAC rigid-body core, stage 1.
//!
//! A 1:1 port of the part of **ODE 0.13.1** (Open Dynamics Engine, single precision) that
//! Assetto Corsa's `acs.exe` links and uses for a car that touches nothing: world, bodies,
//! mass, the force accumulators, the four joint types the cars use (DBall, Ball, Slider,
//! Fixed), islands, and the equality-only path of `dWorldStep` (the "big matrix" stepper:
//! `A = J * M^-1 * J^T`, `L*D*L^T` solve, finite-rotation integrator). Contacts, the
//! pivoting LCP solver and collision detection are stages 2 and 3.
//!
//! The goal is the **same bits** as the game: where the code Visual C++ 2013 produced for
//! `acs.exe` differs from the ODE source (operation order, a multiplication by a reciprocal
//! in place of a division …) the compiled code wins. Functions carry their ODE name and
//! their address in `acs.exe` 1.16.4 in the doc comments.
//!
//! The API follows ODE's (`dBodyCreate` is [`World::body_create`], `dJointAttach` is
//! [`World::joint_attach`], `dWorldStep` is [`World::step`] …) with handles in place of
//! pointers.
//!
//! ODE is copyright (c) 2001-2007 Russell L. Smith and is used here under its BSD-style
//! licence; see `LICENSE-ODE` next to this crate's `Cargo.toml`.

// A transcription: loops index like the original, sums are written in the operand order of
// the machine code, and the float comparisons are the machine code's own (they decide what
// a NaN does: `x < 0.0 || x > 0.0` is not `x != 0.0`).
#![allow(
    clippy::needless_range_loop,
    clippy::manual_memcpy,
    clippy::assign_op_pattern,
    clippy::neg_cmp_op_on_partial_ord,
    clippy::double_comparisons,
    clippy::manual_range_contains,
    clippy::nonminimal_bool
)]

pub mod collide_btl;
pub mod collide_ttl;
pub mod collision;
pub mod common;
pub mod contact;
pub mod geom;
pub mod joint;
pub mod lcp;
pub mod mass;
pub mod matrix;
pub mod odemath;
pub mod opcode;
pub mod opcode_obb;
pub mod rotation;
pub mod step;
pub mod world;

pub use collision::{RayContact, StaticWorld};
pub use common::{Matrix3, Quaternion, Vector3};
pub use contact::{Contact, ContactGeom, SurfaceParameters};
pub use geom::{Collision, GeomId, GeomRef};
pub use joint::{Joint, JointFeedback, JointGroupId, JointKind, PARAM_CFM, PARAM_ERP};
pub use mass::Mass;
pub use step::StepStats;
pub use world::{Body, BodyId, JointId, World};
