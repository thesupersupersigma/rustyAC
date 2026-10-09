// SPDX-License-Identifier: GPL-3.0-or-later

//! rustyAC's renderer: a 1:1 port of the core of Assetto Corsa's Direct3D 11 renderer (the
//! plain `CameraForward` path), using the game's own compiled shaders and textures from the
//! player's install.
//!
//! * [`gpulog`]: the command log both this port and the game's own code are held against.

// The ported code keeps the game's operations as the machine code has them: a product by -1 or
// by 0 is a product, comparisons are spelled so that a NaN takes the game's branch, loops over
// matrix cells index several arrays.
#![allow(clippy::neg_multiply, clippy::erasing_op, clippy::neg_cmp_op_on_partial_ord, clippy::needless_range_loop, clippy::too_many_arguments)]

pub mod camera;
pub mod gpulog;
pub mod graphics;
pub mod kgl;
pub mod lighting;
pub mod material;
pub mod model;
pub mod scene;
pub mod shader;
pub mod sky;
pub mod texture;
pub mod texture_fallback;
