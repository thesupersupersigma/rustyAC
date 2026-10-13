// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! AC's replays: the picture replay of the game (`.acreplay` files, format version 16), as
//! `ReplayManager` and `ReplayRecorder` record, save, load and play it.
//!
//! This is not rustyAC's own `.ryin` recording (the inputs of a drive, simulated again): a
//! replay here is a ring of compact snapshots of every car, taken from the physics at a fixed
//! rate, from which the cars' whole states are rebuilt for the picture, the sound and the HUD
//! while the physics stands still. The code has no file, window or sound in it, so it runs
//! in the browser as it runs on the desktop.

pub mod half;
pub mod manager;
pub mod recorder;

pub use manager::*;
pub use recorder::*;
