// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! The messages of the reliable channel after the join: what a server sends
//! (`ACClient::onMessageTCP` @ 0x14004c840) and what a client sends.
//!
//! A message is one id byte and its fields. The game walks a message in a loop: while
//! bytes are left it reads another record of the same id.

use crate::client::CollisionReport;
use crate::handshake::SessionInfo;
use crate::packet::{Reader, Writer};

pub mod id {
    pub const P2P: u8 = 0x0d;
    pub const MANDATORY_PIT: u8 = 0x0e;
    pub const BLACKLIST_REQUEST: u8 = 0x3b;
    pub const DISCONNECT: u8 = 0x43;
    pub const CHECKSUM: u8 = 0x44;
    pub const CHAT: u8 = 0x47;
    pub const LAP_COMPLETED: u8 = 0x49;
    pub const NEW_SESSION: u8 = 0x4a;
    pub const RACE_OVER: u8 = 0x4b;
    pub const CLIENT_DISCONNECTED: u8 = 0x4d;
    pub const TYRE_COMPOUND: u8 = 0x50;
    pub const TEXT_FILE: u8 = 0x51;
    pub const SETUP: u8 = 0x52;
    pub const DRS_ZONES: u8 = 0x53;
    pub const SUN_ANGLE: u8 = 0x54;
    pub const DAMAGE: u8 = 0x56;
    pub const SECTOR_SPLIT: u8 = 0x58;
    pub const DRIVER_CONNECTED: u8 = 0x5a;
    pub const DRIVER_NAMES: u8 = 0x5b;
    pub const VOTE_NEXT_SESSION: u8 = 0x64;
    pub const VOTE_RESTART_SESSION: u8 = 0x65;
    pub const VOTE_KICK: u8 = 0x66;
    pub const VOTE_NOT_PASSED: u8 = 0x67;
    pub const KICKED: u8 = 0x68;
    pub const BOP: u8 = 0x70;
    pub const WEATHER: u8 = 0x78;
    pub const CLIENT_EVENT: u8 = 0x82;
}

/// A message from the server.
#[derive(Clone, Debug, PartialEq)]
pub enum Message {
    /// 0x47: somebody said something (slot 0xff... the server itself uses the sender's slot).
    Chat { slot: u8, text: String },
    /// 0x4a: the server went on to another session.
    NewSession(SessionInfo),
    /// 0x53: the track's DRS zones (start and end as places round the lap).
    DrsZones(Vec<(f32, f32)>),
    /// 0x54: the sun.
    SunAngle(f32),
    /// 0x70: ballast (kg) and restrictor per slot.
    Bop(Vec<(u8, f32, f32)>),
    /// 0x78: the weather.
    Weather { ambient: u8, road: u8, graphics: String, wind_speed_kmh: i16, wind_direction_deg: i16 },
    /// A message this client does not take apart: its id and its bytes after the id.
    Other { id: u8, data: Vec<u8> },
}

/// What the local car did that the server is told at once.
#[derive(Clone, Debug, PartialEq)]
pub enum ClientEvent {
    /// A message built elsewhere, sent as it is.
    Raw(Vec<u8>),
}

/// Takes a message from the server apart. `cars`: how many cars the session has (message
/// 0x4a lists a slot for each).
pub fn decode(message: &[u8], cars: usize) -> Vec<Message> {
    let mut out = Vec::new();
    let Some(&first) = message.first() else {
        return out;
    };
    let mut r = Reader::new(message);
    r.u8();
    match first {
        id::CHAT => out.push(Message::Chat { slot: r.u8(), text: r.wide() }),
        id::NEW_SESSION => out.push(Message::NewSession(SessionInfo::decode(&mut r, Some(cars)))),
        id::DRS_ZONES => {
            let count = r.u8();
            out.push(Message::DrsZones((0..count).map(|_| (r.f32(), r.f32())).collect()));
        }
        id::SUN_ANGLE => out.push(Message::SunAngle(r.f32())),
        id::BOP => {
            let count = r.u8();
            out.push(Message::Bop((0..count).map(|_| (r.u8(), r.f32(), r.f32())).collect()));
        }
        id::WEATHER => out.push(Message::Weather { ambient: r.u8(), road: r.u8(), graphics: r.wide(), wind_speed_kmh: r.i16(), wind_direction_deg: r.i16() }),
        other => out.push(Message::Other { id: other, data: message[1..].to_vec() }),
    }
    if r.overrun {
        // a message that ends inside a field: not taken apart
        return vec![Message::Other { id: first, data: message[1..].to_vec() }];
    }
    out
}

/// A client's message for `event`.
pub fn encode(_slot: u8, event: &ClientEvent) -> Vec<u8> {
    match event {
        ClientEvent::Raw(bytes) => bytes.clone(),
    }
}

/// `ACClient::askForP2Pvalue` @ 0x140044470: `0d ff ff 00`.
pub fn ask_p2p() -> Vec<u8> {
    vec![id::P2P, 0xff, 0xff, 0x00]
}

/// `ACClient::shutdown` @ 0x140052d30.
pub fn disconnect() -> Vec<u8> {
    vec![id::DISCONNECT]
}

/// Message 0x44: the digests one after the other, the car's last.
pub fn checksum_answer(digests: &[[u8; 16]]) -> Vec<u8> {
    let mut w = Writer::with_id(id::CHECKSUM);
    for digest in digests {
        w.bytes(digest);
    }
    w.finish()
}

/// `ACClient::sendChat` @ 0x140052a90.
pub fn chat(slot: u8, text: &str) -> Vec<u8> {
    let mut w = Writer::with_id(id::CHAT);
    w.u8(slot).wide(text);
    w.finish()
}

/// The collisions of the last five seconds (message 0x82), built by
/// `ACClient::onPhysicsStep` @ 0x14004f1d0.
pub fn collision_report(collisions: &[CollisionReport]) -> Vec<u8> {
    let mut w = Writer::with_id(id::CLIENT_EVENT);
    w.u16(collisions.len() as u16);
    for collision in collisions {
        match collision.other_slot {
            Some(slot) => {
                w.u8(0x0a).u8(slot);
            }
            None => {
                w.u8(0x0b);
            }
        }
        w.f32(collision.speed).vec3(collision.world_pos).vec3(collision.rel_pos);
    }
    w.finish()
}
