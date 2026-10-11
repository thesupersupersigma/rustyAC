// SPDX-License-Identifier: GPL-3.0-or-later

//! rustyAC multiplayer: the client side of Assetto Corsa's online protocol (protocol
//! version 202, the one `acs.exe` 1.16 and Kunos' `acServer` 1.15 speak).
//!
//! * [`packet`]: AC's `UDPPacket`, the byte buffer of every message.
//! * [`transport`]: the two channels to a server as a trait, the reliable channel's
//!   framing, and tapes of what crossed. [`native`] is the desktop's TCP + UDP.
//! * [`md5`]: the checksums a server asks for.
//!
//! Nothing in this crate is platform code: it builds for `wasm32-unknown-unknown`, where
//! the transport will be a WebSocket to a gateway next to the server (M2).

pub mod md5;
#[cfg(not(target_arch = "wasm32"))]
pub mod native;
pub mod packet;
pub mod transport;
