// SPDX-License-Identifier: GPL-3.0-or-later

//! rustyAC's renderer: a 1:1 port of the core of Assetto Corsa's Direct3D 11 renderer (the
//! plain `CameraForward` path), using the game's own compiled shaders and textures from the
//! player's install.
//!
//! * [`gpulog`]: the command log both this port and the game's own code are held against.

// The ported code keeps the game's operations as the machine code has them: a product by -1 or
// by 0 is a product, comparisons are spelled so that a NaN takes the game's branch, loops over
// matrix cells index several arrays.
#![allow(clippy::neg_multiply, clippy::erasing_op, clippy::neg_cmp_op_on_partial_ord, clippy::needless_range_loop, clippy::too_many_arguments, clippy::assign_op_pattern, clippy::eq_op)]

pub mod animator;
pub mod blur;
pub mod camera;
pub mod car;
pub mod cockpit;
pub mod constrained;
pub mod crowds;
pub mod cubemap;
pub mod damage;
pub mod digital;
pub mod driver;
pub mod fake_shadow;
pub mod forward;
#[cfg(all(not(windows), not(feature = "backend-wgpu")))]
compile_error!("rustyac-render off Windows needs the feature backend-wgpu (Direct3D 11 is Windows only)");
#[cfg(not(feature = "backend-wgpu"))]
pub mod fw1;
mod fw1_atlas;
// the glyphs of Direct3D 11's text are DirectWrite's: there only the layout tests use it
#[cfg_attr(not(feature = "backend-wgpu"), allow(dead_code))]
mod ttf;
#[cfg(feature = "backend-wgpu")]
#[path = "fw1_wgpu.rs"]
pub mod fw1;
pub mod flames;
pub mod ghost_car;
pub mod gl;
#[cfg(windows)]
pub mod gpulog;
pub mod gui_paint;
pub mod graphics;
pub mod ideal_line;
#[cfg(not(feature = "backend-wgpu"))]
pub mod kgl;
#[cfg(feature = "backend-wgpu")]
#[path = "kgl_wgpu.rs"]
pub mod kgl;
pub mod kgl_log;
/// (the page gives kgl its device: the same wgpu as here)
#[cfg(feature = "wgpu")]
pub use wgpu;
pub mod lighting;
pub mod lights;
pub mod material;
pub mod mirror;
pub mod model;
pub mod panels;
pub mod particles;
pub mod pit_crew;
pub mod post;
pub mod scene;
pub mod shader;
pub mod skid;
pub mod sky;
pub mod start_lights;
/// The game's `CarPhysicsState`, which lives with the physics (the replays rebuild it
/// where there is no Direct3D).
pub mod state {
    pub use rustyac_physics::car_physics_state::*;
}
pub mod suspension_graphics;
pub mod texture;
pub mod time_attack;
pub mod tyre_crew;
pub mod wings;
pub mod texture_fallback;
