//! rustyAC physics.
//!
//! Every physics system is a slot behind a trait with a **Vanilla** implementation (a 1:1
//! port of Assetto Corsa's code, bit-exact where that can be shown) and, later, an
//! **Enhanced** one. Ported functions carry their original name and address in `acs.exe`.

pub mod curve;
pub mod data;
pub mod math;
pub mod tyre;
pub mod vecmath;
