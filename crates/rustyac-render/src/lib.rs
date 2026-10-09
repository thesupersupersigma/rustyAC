// SPDX-License-Identifier: GPL-3.0-or-later

//! rustyAC's renderer: a 1:1 port of the core of Assetto Corsa's Direct3D 11 renderer (the
//! plain `CameraForward` path), using the game's own compiled shaders and textures from the
//! player's install.
//!
//! * [`gpulog`]: the command log both this port and the game's own code are held against.

pub mod gpulog;
