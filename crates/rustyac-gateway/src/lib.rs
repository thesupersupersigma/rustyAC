// SPDX-License-Identifier: MIT OR Apache-2.0

//! The rustyAC WebSocket gateway.
//!
//! Assetto Corsa's online protocol is one TCP connection and UDP datagrams to an
//! `acServer`. A browser can open neither, and an HTTP-only tunnel (a Cloudflare Tunnel)
//! carries neither. The gateway stands next to the server and carries one client per
//! WebSocket:
//!
//! ```text
//! client  <== one WebSocket, binary messages ==>  gateway  <== TCP ==>  acServer
//!             [0][one TCP message]                         <== UDP ==>
//!             [1][one UDP datagram]
//! ```
//!
//! * a message with lane byte 0 ([`ws::LANE_RELIABLE`]) is one message of the TCP stream;
//!   the gateway adds or removes the 16-bit length the stream has in front of each;
//! * a message with lane byte 1 ([`ws::LANE_DATAGRAM`]) is one UDP datagram, from a socket
//!   the gateway opens for this client alone (the server tells clients apart by it).
//!
//! The gateway does not read what it carries, with one exception: when a WebSocket ends
//! and the client has joined but not said goodbye (a browser tab that was closed), the
//! gateway sends the protocol's one-byte goodbye (`0x43`) for it, so that the server frees
//! the slot and tells the other players at once.
//!
//! [`ws`] also holds the client side of the WebSocket, which `rustyac.exe --join ws://...`
//! uses. Plain `std`: no dependency, no SIMD, nothing a processor without AVX cannot run.

pub mod gateway;
pub mod sha1;
pub mod ws;
