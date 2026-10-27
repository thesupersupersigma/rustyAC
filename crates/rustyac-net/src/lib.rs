// SPDX-License-Identifier: GPL-3.0-or-later

//! rustyAC multiplayer: the client side of Assetto Corsa's online protocol (protocol
//! version 202, the one `acs.exe` 1.16 and Kunos' `acServer` 1.15 speak).
//!
//! * [`packet`]: AC's `UDPPacket`, the byte buffer of every message.
//! * [`transport`]: the two channels to a server as a trait, the reliable channel's
//!   framing, and tapes of what crossed. [`native`] is the desktop's TCP + UDP.
//! * [`md5`]: the checksums a server asks for.
//!
//! * [`ws`]: both channels in one WebSocket to a gateway next to the server
//!   (`crates/rustyac-gateway`), for the browser and for a desktop behind an HTTP-only tunnel.
//!
//! Nothing in this crate is platform code: it builds for `wasm32-unknown-unknown`, where
//! the page's own `WebSocket` is the pipe under [`ws::WsTransport`].

pub mod car_update;
pub mod client;
pub mod datagram;
/// The matrix blend of the game (`lerp` 0x140108600), which lives with the other maths.
pub mod dxmath {
    pub use rustyac_physics::dxmath::*;
}
pub mod handshake;
pub mod laps;
pub mod md5;
pub mod messages;
#[cfg(not(target_arch = "wasm32"))]
pub mod native;
pub mod net_car;
pub mod packet;
pub mod replay;
pub mod session;
pub mod transport;
pub mod ws;
