// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! rustyAC physics.
//!
//! Every physics system is a slot behind a trait with a **Vanilla** implementation (a 1:1
//! port of Assetto Corsa's code, bit-exact where that can be shown) and, later, an
//! **Enhanced** one. Ported functions carry their original name and address in `acs.exe`.

pub mod car;
pub mod curve;
pub mod data;
/// The C runtime functions with AC's exact results (shared crate `rustyac-math`).
pub use rustyac_math as math;
pub use rustyac_ode as ode;
pub mod track;
pub mod tyre;
pub mod vecmath;
