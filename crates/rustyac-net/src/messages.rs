// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! The messages of the reliable channel after the join: what a server sends
//! (`ACClient::onMessageTCP` @ 0x14004c840) and what a client sends.
//!
//! A message is one id byte and its fields. The game reads the id once and then runs the
//! id's handler again and again while bytes are left, so a message may hold several
//! records of its kind one after the other; [`decode`] does the same.
//!
//! Some records have no count on the wire: the lap message (0x49), the race-over message
//! (0x4b) and the new-session message (0x4a) list one row per car of the session, and
//! "the session's cars" is what the client itself made of the car list (every slot that is
//! no spectator's).

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

    /// Sub-types of [`CLIENT_EVENT`].
    pub const EVENT_COLLISION_WITH_CAR: u8 = 0x0a;
    pub const EVENT_COLLISION_WITH_ENV: u8 = 0x0b;
    pub const EVENT_JUMP_START: u8 = 0x0c;
}

/// The time the server sends for "no lap yet".
pub const NO_TIME: u32 = 999_999_999;

/// One row of the standings a lap message carries.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LeaderboardRow {
    pub slot: u8,
    /// Best lap in practice and qualifying, total time in a race, ms ([`NO_TIME`]: none).
    pub time: u32,
    pub laps: u16,
    pub has_finished: bool,
}

/// A message from the server.
#[derive(Clone, Debug, PartialEq)]
pub enum Message {
    /// 0x0d: push-to-pass of a car (`count` -1: the answer to the client's question).
    P2p { slot: u8, count: i16, activate: bool },
    /// 0x0e: a car did its mandatory pit stop.
    MandatoryPit { slot: u8, done: bool },
    /// 0x47: somebody said something (slot 0xff: the server).
    Chat { slot: u8, text: String },
    /// 0x49: a car completed a lap; the standings; the track's grip now.
    LapCompleted { slot: u8, lap_time: u32, cuts: u8, standings: Vec<LeaderboardRow>, grip: f32 },
    /// 0x4a: the server went on to another session.
    NewSession(SessionInfo),
    /// 0x4b: the race is over (the rows: slot, time, laps); the flag: the server goes on
    /// with this client (false: the connection ends after the result screen).
    RaceOver { standings: Vec<LeaderboardRow>, stay: bool },
    /// 0x4d: a driver left.
    ClientDisconnected { slot: u8 },
    /// 0x50: a car changed its tyres (the compound's short name).
    TyreCompound { slot: u8, compound: String },
    /// 0x51, type 0: the server's welcome text.
    WelcomeMessage(String),
    /// 0x52: the setup the server fixes (`[NAME] VALUE=` pairs).
    Setup { fixed: u8, values: Vec<(String, f32)> },
    /// 0x53: the track's DRS zones (start and end as places round the lap).
    DrsZones(Vec<(f32, f32)>),
    /// 0x54: the sun, degrees.
    SunAngle(f32),
    /// 0x56: a car's damage (front, rear, left, right, centre).
    Damage { slot: u8, levels: [f32; 5] },
    /// 0x58: a car's sector time.
    SectorSplit { slot: u8, sector: u8, time: u32, cuts: u8 },
    /// 0x5a: a driver took a slot (the slot's damage starts from nothing).
    DriverConnected { slot: u8, name: String, nation_code: String },
    /// 0x5b: the driver names of slots.
    DriverNames(Vec<(u8, String)>),
    /// 0x64 / 0x65 / 0x66: a vote is running. `kind`: 0 next session, 1 restart, 2 kick
    /// (`target`: the slot to kick).
    Vote { kind: u8, target: u8, quorum: u8, votes: u8, time_left_ms: u32, last_voter: u8, last_vote: bool },
    /// 0x67: the vote did not pass.
    VoteNotPassed,
    /// 0x68: a driver was kicked (0 voted out, 1 banned until restart, 2 blacklisted,
    /// 3 checksum failed, other: kicked by the admin).
    Kicked { slot: u8, reason: u8 },
    /// 0x70: ballast (kg) and restrictor per slot.
    Bop(Vec<(u8, f32, f32)>),
    /// 0x78: the weather.
    Weather { ambient: u8, road: u8, graphics: String, wind_speed_kmh: i16, wind_direction_deg: i16 },
    /// 0x82, sub-type 0x0c: a car jumped the start.
    JumpStart { slot: u8 },
    /// A message this client does not take apart: its id and its bytes after the id.
    Other { id: u8, data: Vec<u8> },
}

/// What the local car did that the server is told at once.
#[derive(Clone, Debug, PartialEq)]
pub enum ClientEvent {
    /// `ACClient::onLapCompleted` @ 0x14004b830 (only valid laps of the local car).
    LapCompleted { physics_time: f64, lap_time: u32, splits: Vec<u32>, cuts: i32, lap_count: u8 },
    /// `ACClient::onSectorSplit` @ 0x14004ff70
    SectorSplit { sector: u8, time: u32, cuts: u32 },
    /// `ACClient::onTyreCompoundChanged` @ 0x1400503d0: the compound's short name.
    TyreCompound(String),
    /// `ACClient::updateDamageReport` @ 0x140054060
    Damage([f32; 5]),
    /// The jump-start handler of `ACClient::beginUpdateMode`.
    JumpStart,
    /// Push-to-pass was used.
    P2p { count: u16, active: bool },
    /// The mandatory pit stop is done.
    MandatoryPit(bool),
    /// `ACClient::sendVote` @ 0x140052ba0. `kind`: 0 next session, 1 restart, 2 kick
    /// (with the slot to kick).
    Vote { kind: u8, yes: bool, target: Option<u8> },
    /// The wrecker protection asks to be blacklisted (`ACClient::onCollisionWithCar`).
    BlacklistRequest,
    /// A message built elsewhere, sent as it is.
    Raw(Vec<u8>),
}

fn rows(r: &mut Reader<'_>, cars: usize, with_finished: bool) -> Vec<LeaderboardRow> {
    (0..cars)
        .map(|_| {
            let mut row = LeaderboardRow { slot: r.u8(), time: r.u32(), laps: r.u16(), has_finished: false };
            if with_finished {
                row.has_finished = r.bool();
            }
            row
        })
        .collect()
}

/// One record of message `first` from `r`. `None`: the id is not one this client knows.
fn record(first: u8, r: &mut Reader<'_>, cars: usize) -> Option<Message> {
    Some(match first {
        id::P2P => Message::P2p { slot: r.u8(), count: r.i16(), activate: r.bool() },
        id::MANDATORY_PIT => Message::MandatoryPit { slot: r.u8(), done: r.bool() },
        id::CHAT => Message::Chat { slot: r.u8(), text: r.wide() },
        id::LAP_COMPLETED => {
            let (slot, lap_time, cuts) = (r.u8(), r.u32(), r.u8());
            // (a byte the game reads and drops: the number of rows the server sends)
            let sent = r.u8() as usize;
            // the game reads as many rows as it has cars; a server that counts otherwise
            // is followed by its own count as long as the message has the bytes
            let count = if sent != cars && r.remaining() == sent * 8 + 4 { sent } else { cars };
            Message::LapCompleted { slot, lap_time, cuts, standings: rows(r, count, true), grip: r.f32() }
        }
        id::NEW_SESSION => Message::NewSession(SessionInfo::decode(r, Some(cars))),
        id::RACE_OVER => Message::RaceOver { standings: rows(r, cars, false), stay: r.bool() },
        id::CLIENT_DISCONNECTED => Message::ClientDisconnected { slot: r.u8() },
        id::TYRE_COMPOUND => Message::TyreCompound { slot: r.u8(), compound: r.string() },
        id::TEXT_FILE => {
            let kind = r.u8();
            if kind != 0 {
                return None;
            }
            Message::WelcomeMessage(r.big_wide())
        }
        id::SETUP => {
            let fixed = r.u8();
            let count = r.u8();
            Message::Setup { fixed, values: (0..count).map(|_| (r.string(), r.f32())).collect() }
        }
        id::DRS_ZONES => {
            let count = r.u8();
            Message::DrsZones((0..count).map(|_| (r.f32(), r.f32())).collect())
        }
        id::SUN_ANGLE => Message::SunAngle(r.f32()),
        id::DAMAGE => {
            let slot = r.u8();
            let mut levels = [0.0f32; 5];
            for level in &mut levels {
                *level = r.f32();
            }
            Message::Damage { slot, levels }
        }
        id::SECTOR_SPLIT => Message::SectorSplit { slot: r.u8(), sector: r.u8(), time: r.u32(), cuts: r.u8() },
        id::DRIVER_CONNECTED => Message::DriverConnected { slot: r.u8(), name: r.string(), nation_code: r.string() },
        id::DRIVER_NAMES => {
            let count = r.u8();
            Message::DriverNames((0..count).map(|_| (r.u8(), r.wide())).collect())
        }
        id::VOTE_NEXT_SESSION | id::VOTE_RESTART_SESSION | id::VOTE_KICK => {
            let kind = first - id::VOTE_NEXT_SESSION;
            Message::Vote { kind, target: r.u8(), quorum: r.u8(), votes: r.u8(), time_left_ms: r.u32(), last_voter: r.u8(), last_vote: r.bool() }
        }
        id::VOTE_NOT_PASSED => {
            // (a number the game reads and does not use)
            r.i32();
            Message::VoteNotPassed
        }
        id::KICKED => Message::Kicked { slot: r.u8(), reason: r.u8() },
        id::BOP => {
            let count = r.u8();
            Message::Bop((0..count).map(|_| (r.u8(), r.f32(), r.f32())).collect())
        }
        id::WEATHER => Message::Weather { ambient: r.u8(), road: r.u8(), graphics: r.wide(), wind_speed_kmh: r.i16(), wind_direction_deg: r.i16() },
        id::CLIENT_EVENT => {
            // (the game reads nothing more of any other kind)
            if r.u8() != id::EVENT_JUMP_START {
                return None;
            }
            Message::JumpStart { slot: r.u8() }
        }
        _ => return None,
    })
}

/// Takes a message from the server apart. `cars`: how many cars the session has for this
/// client (every slot of the car list that is no spectator's).
pub fn decode(message: &[u8], cars: usize) -> Vec<Message> {
    let mut out = Vec::new();
    let Some(&first) = message.first() else {
        return out;
    };
    let other = || Message::Other { id: first, data: message[1..].to_vec() };
    let mut r = Reader::new(message);
    r.u8();
    // a message of the id alone is handled once; else record after record while bytes are left
    let mut once = message.len() == 1;
    while once || !r.is_eof() {
        once = false;
        match record(first, &mut r, cars) {
            Some(decoded) if !r.overrun => {
                let ends = matches!(decoded, Message::NewSession(_));
                out.push(decoded);
                // (the new-session handler leaves the message at once)
                if ends {
                    break;
                }
            }
            // an unknown id, or a record that ends inside a field: not taken apart
            _ => return vec![other()],
        }
    }
    out
}

/// A client's message for `event`. `slot`: the client's own.
pub fn encode(slot: u8, event: &ClientEvent) -> Vec<u8> {
    let mut w = Writer::new();
    match event {
        ClientEvent::LapCompleted { physics_time, lap_time, splits, cuts, lap_count } => {
            w.u8(id::LAP_COMPLETED).u32(*physics_time as i64 as u32).u32(*lap_time).u8(splits.len() as u8);
            for split in splits {
                w.u32(*split);
            }
            w.u8((*cuts).min(255) as u8).u8(*lap_count);
        }
        ClientEvent::SectorSplit { sector, time, cuts } => {
            w.u8(id::SECTOR_SPLIT).u8(*sector).u32(*time).u8((*cuts).min(255) as u8);
        }
        ClientEvent::TyreCompound(name) => {
            w.u8(id::TYRE_COMPOUND).ansi(name);
        }
        ClientEvent::Damage(levels) => {
            w.u8(id::DAMAGE);
            for level in levels {
                w.f32(*level);
            }
        }
        ClientEvent::JumpStart => {
            w.u8(id::CLIENT_EVENT).u8(id::EVENT_JUMP_START);
        }
        ClientEvent::P2p { count, active } => {
            w.u8(id::P2P).u16(*count).u8(*active as u8);
        }
        ClientEvent::MandatoryPit(done) => {
            w.u8(id::MANDATORY_PIT).u8(*done as u8);
        }
        ClientEvent::Vote { kind, yes, target } => match (kind, target) {
            (0, _) => {
                w.u8(id::VOTE_NEXT_SESSION).u8(*yes as u8);
            }
            (1, _) => {
                w.u8(id::VOTE_RESTART_SESSION).u8(*yes as u8);
            }
            (2, Some(target)) => {
                w.u8(id::VOTE_KICK).u8(*target).u8(*yes as u8);
            }
            _ => {}
        },
        ClientEvent::BlacklistRequest => {
            w.u8(id::BLACKLIST_REQUEST);
        }
        ClientEvent::Raw(bytes) => {
            w.bytes(bytes);
        }
    }
    let _ = slot;
    w.finish()
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
/// `ACClient::onPhysicsStep` @ 0x14004f1d0: a count, then per collision its kind (0x0a
/// with the other car's slot, 0x0b with anything else), the closing speed in km/h, where
/// it was in the world and on the car.
pub fn collision_report(collisions: &[CollisionReport]) -> Vec<u8> {
    let mut w = Writer::with_id(id::CLIENT_EVENT);
    w.u16(collisions.len() as u16);
    for collision in collisions {
        match collision.other_slot {
            Some(slot) => {
                w.u8(id::EVENT_COLLISION_WITH_CAR).u8(slot);
            }
            None => {
                w.u8(id::EVENT_COLLISION_WITH_ENV);
            }
        }
        w.f32(collision.speed).vec3(collision.world_pos).vec3(collision.rel_pos);
    }
    w.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lap_message_out_and_in() {
        let out = encode(1, &ClientEvent::LapCompleted { physics_time: 65_536.7, lap_time: 100_000, splits: vec![30_000, 30_000, 40_000], cuts: 300, lap_count: 2 });
        assert_eq!(out.len(), 12 + 12);
        assert_eq!(&out[..10], &[0x49, 0, 0, 1, 0, 0xa0, 0x86, 1, 0, 3]);
        assert_eq!(&out[22..], &[255, 2]);
        // the server's lap message for two cars
        let mut w = Writer::with_id(id::LAP_COMPLETED);
        w.u8(1).u32(100_000).u8(0).u8(2);
        w.u8(1).u32(100_000).u16(1).u8(0);
        w.u8(0).u32(NO_TIME).u16(0).u8(0);
        w.f32(0.97);
        let decoded = decode(&w.finish(), 2);
        let Message::LapCompleted { slot, lap_time, standings, grip, .. } = &decoded[0] else { panic!("{decoded:?}") };
        assert_eq!((*slot, *lap_time, standings.len(), *grip), (1, 100_000, 2, 0.97));
        assert_eq!(standings[1].time, NO_TIME);
    }

    #[test]
    fn records_repeat_and_unknown_ids_stay_whole() {
        let decoded = decode(&[0x4d, 3, 5], 2);
        assert_eq!(decoded, vec![Message::ClientDisconnected { slot: 3 }, Message::ClientDisconnected { slot: 5 }]);
        assert_eq!(decode(&[0x99, 1, 2], 2), vec![Message::Other { id: 0x99, data: vec![1, 2] }]);
        assert_eq!(decode(&[0x67, 0, 0, 0, 0], 2), vec![Message::VoteNotPassed]);
        assert_eq!(decode(&[0x82, 0x0c, 4], 2), vec![Message::JumpStart { slot: 4 }]);
        assert_eq!(decode(&[0x5a, 2, 1, b'x', 3, b'I', b'T', b'A'], 2), vec![Message::DriverConnected { slot: 2, name: "x".into(), nation_code: "ITA".into() }]);
        assert_eq!(collision_report(&[CollisionReport { other_slot: Some(4), speed: 1.0, world_pos: [0.0; 3], rel_pos: [0.0; 3] }]).len(), 3 + 2 + 28);
    }
}
