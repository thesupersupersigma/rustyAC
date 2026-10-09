// SPDX-License-Identifier: GPL-3.0-or-later

//! rustyAC in the browser, a preview: the Vanilla physics as WebAssembly (bit for bit the
//! desktop's, see `docs/port/web.md`), a small picture, keyboard and gamepad. No sound, no
//! AC shaders, no shared memory.
//!
//! * [`content`]: which cars and tracks are there, which load, which files a drive needs.
//! * [`session`]: one drive: the simulation behind a fixed-step accumulator, its camera.
//! * [`driver`]: the page's keys and pad behind AC's own keyboard and pad classes.
//! * [`model`], [`bc`]: kn5 models and their textures, ready for a graphics card.
//! * [`pack`]: the preview pack's manifest (`tools/web_pack` writes it, the page reads it).
//! * `gpu`, `web` (wasm only): the wgpu picture and what the page calls.
//!
//! Everything but the last two is plain Rust and is tested on the desktop.

pub mod bc;
pub mod content;
pub mod driver;
pub mod model;
pub mod pack;
pub mod session;

#[cfg(target_arch = "wasm32")]
mod gpu;
#[cfg(target_arch = "wasm32")]
mod web;
