// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! Joining a server: the handshake (`ACClient::handshakeTCP` @ 0x140048b50) and the car
//! list (`ACClient::getRemoteCarList` @ 0x140046f80), as messages built and parsed. Who
//! sends and waits is [`crate::client`].

use crate::packet::{Reader, Writer};

/// The protocol version `acs.exe` 1.16 sends and Kunos' `acServer` 1.15 wants.
pub const PROTOCOL_VERSION: u16 = 202;

/// Message ids of the join phase.
pub mod id {
    pub const BLACKLISTED: u8 = 0x3b;
    pub const WRONG_PASSWORD: u8 = 0x3c;
    pub const HANDSHAKE: u8 = 0x3d;
    pub const ACCEPTED: u8 = 0x3e;
    pub const CAR_LIST_REQUEST: u8 = 0x3f;
    pub const CAR_LIST: u8 = 0x40;
    pub const STILL_BOOKING: u8 = 0x41;
    pub const UNSUPPORTED_PROTOCOL: u8 = 0x42;
    pub const NO_SLOTS: u8 = 0x45;
    pub const SESSION_CLOSED: u8 = 0x6e;
    pub const AUTH_FAILED: u8 = 0x6f;
}

/// What the player sends to join. The game takes the GUID from Steam and the rest from
/// race.ini (`[REMOTE] NAME, TEAM, REQUESTED_CAR, PASSWORD`, `[CAR_0] NATION_CODE`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct JoinRequest {
    pub guid: String,
    pub name: String,
    pub team: String,
    pub nation_code: String,
    pub car: String,
    pub password: String,
}

impl JoinRequest {
    /// Message 0x3d: the id, the protocol version, then the GUID (narrow), the name (the
    /// one wide string), team, nation, car and password (narrow).
    pub fn encode(&self) -> Vec<u8> {
        let mut w = Writer::with_id(id::HANDSHAKE);
        w.u16(PROTOCOL_VERSION);
        w.ansi(&self.guid).wide(&self.name).ansi(&self.team).ansi(&self.nation_code).ansi(&self.car).ansi(&self.password);
        w.finish()
    }
}

/// Why a server did not let the player in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    Blacklisted,
    WrongPassword,
    /// The server is still in its booking phase; the value as the server sent it (the
    /// game shows it as minutes:seconds of milliseconds).
    StillBooking(i32),
    UnsupportedProtocol,
    NoSlots,
    SessionClosed,
    AuthFailed(String),
    /// A message that is no handshake answer.
    Unexpected(u8),
    /// The connection gave nothing.
    NoResponse,
}

impl Refusal {
    /// The game's own words for it (its splash messages).
    pub fn message(&self) -> String {
        match self {
            Refusal::Blacklisted => "Cannot join the server : You have been blacklisted from this server".to_string(),
            Refusal::WrongPassword => "ERROR :: Password not accepted".to_string(),
            Refusal::StillBooking(ms) => format!("ERROR :: Server still booking ( {} s ) left", ms / 1000),
            Refusal::UnsupportedProtocol => "ERROR :: Server is running a different protocol".to_string(),
            Refusal::NoSlots => "Cannot join the server : no available slots".to_string(),
            Refusal::SessionClosed => "ERROR :: Session is closed to new connections".to_string(),
            Refusal::AuthFailed(reason) => format!("SERVER AUTH FAILED:{reason}"),
            Refusal::Unexpected(id) => format!("Server error code : {id}"),
            Refusal::NoResponse => "Server error : No response from the server".to_string(),
        }
    }
}

/// A session's kind (`SessionType`).
pub mod session_type {
    pub const PRACTICE: u8 = 1;
    pub const QUALIFY: u8 = 2;
    pub const RACE: u8 = 3;

    pub fn name(kind: u8) -> &'static str {
        match kind {
            QUALIFY => "Qualify",
            RACE => "Race",
            _ => "Practice",
        }
    }
}

/// One entry of the server's session list (`RemoteSession` as the handshake fills it).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SessionEntry {
    /// 1 practice, 2 qualifying, 3 race (anything else counts as practice).
    pub kind: u8,
    pub laps: u16,
    /// Minutes.
    pub time: u16,
}

impl SessionEntry {
    pub fn is_timed_race(&self) -> bool {
        self.kind == session_type::RACE && self.laps == 0 && self.time > 0
    }
}

/// The session a server is in: the body of `ACClient::onNewSession` @ 0x14004e4a0, which
/// the handshake's reply carries and message 0x4a is.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SessionInfo {
    pub name: String,
    pub index: u8,
    /// Raw: 1 practice, 2 qualifying, 3 race.
    pub kind: u8,
    /// Minutes.
    pub time: u16,
    pub laps: u16,
    /// The track's grip, 0..1.
    pub grip: f32,
    /// In the handshake: the player's place on the grid (`handshakeCarPosition`).
    pub handshake_position: Option<u8>,
    /// In message 0x4a: the slots in grid order, one per car of the session.
    pub grid: Vec<u8>,
    /// In the handshake: how long the session has run, ms. In message 0x4a: the session's
    /// start on the clock the server and the clients share.
    pub start: i64,
}

impl SessionInfo {
    /// `grid_slots`: `None` in the handshake, else the number of cars (`netCars + 1`).
    pub fn decode(r: &mut Reader<'_>, grid_slots: Option<usize>) -> SessionInfo {
        let mut info = SessionInfo { name: r.string(), index: r.u8(), kind: r.u8(), time: r.u16(), laps: r.u16(), grip: r.f32(), ..SessionInfo::default() };
        match grid_slots {
            None => info.handshake_position = Some(r.u8()),
            Some(count) => {
                for _ in 0..count {
                    info.grid.push(r.u8());
                }
            }
        }
        info.start = r.i64();
        info
    }

    pub fn is_timed_race(&self) -> bool {
        self.kind == session_type::RACE && self.laps == 0 && self.time > 0
    }
}

/// The accepted reply (0x3e): the server's name, where and how often to send, what to
/// load, and the server's rules.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Accepted {
    pub server_name: String,
    pub udp_port: u16,
    /// `CLIENT_SEND_INTERVAL_HZ`
    pub send_hz: u8,
    /// Lower-cased, as the game does on receipt.
    pub track: String,
    pub track_config: String,
    /// The car the server gave the player (lower-cased) and its skin (as sent).
    pub model: String,
    pub skin: String,
    pub sun_angle: f32,
    /// -1: any number of tyres may leave the track.
    pub allowed_tyres_out: i16,
    pub tyre_blankets: bool,
    /// 0 off, 1 the car's own, 2 everybody.
    pub tc: u8,
    pub abs: u8,
    pub stability: bool,
    pub auto_clutch: bool,
    /// 0 locked on the grid, 1 teleport to the pits, 2 drive-through.
    pub jump_start_mode: u8,
    /// Rates: 1 = 100 %.
    pub damage_rate: f32,
    pub fuel_rate: f32,
    pub tyre_wear_rate: f32,
    pub force_virtual_mirror: bool,
    /// 0: no wrecker protection.
    pub max_contacts_per_km: u8,
    pub race_over_time_ms: u32,
    pub result_screen_time_ms: u32,
    pub has_extra_lap: bool,
    pub gas_penalty_disabled: bool,
    pub pit_window_start: u16,
    pub pit_window_end: u16,
    /// Negative: the whole grid is inverted.
    pub inverted_grid_positions: i16,
    /// The player's slot.
    pub session_id: u8,
    pub sessions: Vec<SessionEntry>,
    pub current_session: SessionInfo,
    /// Files whose MD5 the server wants (paths from the game's folder).
    pub checksum_files: Vec<String>,
    /// `;`-separated short names; empty: every tyre is legal.
    pub legal_tyres: String,
    pub random_seed: u32,
}

impl Accepted {
    /// `ACClient::sendInterval`: `((1000 / Hz) / 3) * 3` in whole milliseconds.
    pub fn send_interval_ms(&self) -> f64 {
        send_interval_ms(self.send_hz)
    }
}

/// The send interval of a rate in Hz, as the game computes it (a multiple of the 3 ms
/// step). A rate of 0, on which the game divides by zero, gives the game's start value.
pub fn send_interval_ms(hz: u8) -> f64 {
    if hz == 0 {
        return 96.0;
    }
    (((1000 / hz as i32) / 3) * 3) as f64
}

/// The answer to a handshake.
#[derive(Clone, Debug, PartialEq)]
pub enum HandshakeReply {
    Accepted(Box<Accepted>),
    Refused(Refusal),
}

impl HandshakeReply {
    pub fn decode(message: &[u8]) -> HandshakeReply {
        let mut r = Reader::new(message);
        if message.is_empty() {
            return HandshakeReply::Refused(Refusal::NoResponse);
        }
        match r.u8() {
            id::ACCEPTED => {}
            id::BLACKLISTED => return HandshakeReply::Refused(Refusal::Blacklisted),
            id::WRONG_PASSWORD => return HandshakeReply::Refused(Refusal::WrongPassword),
            id::STILL_BOOKING => return HandshakeReply::Refused(Refusal::StillBooking(r.i32())),
            id::UNSUPPORTED_PROTOCOL => return HandshakeReply::Refused(Refusal::UnsupportedProtocol),
            id::NO_SLOTS => return HandshakeReply::Refused(Refusal::NoSlots),
            id::SESSION_CLOSED => return HandshakeReply::Refused(Refusal::SessionClosed),
            id::AUTH_FAILED => return HandshakeReply::Refused(Refusal::AuthFailed(r.wide())),
            other => return HandshakeReply::Refused(Refusal::Unexpected(other)),
        }
        let mut a = Accepted { server_name: r.wide(), udp_port: r.u16(), send_hz: r.u8(), ..Accepted::default() };
        a.track = r.string().to_lowercase();
        a.track_config = r.string().to_lowercase();
        a.model = r.string().to_lowercase();
        a.skin = r.string();
        a.sun_angle = r.f32();
        a.allowed_tyres_out = r.i16();
        a.tyre_blankets = r.bool();
        a.tc = r.u8();
        a.abs = r.u8();
        a.stability = r.bool();
        a.auto_clutch = r.bool();
        a.jump_start_mode = r.u8();
        a.damage_rate = r.f32();
        a.fuel_rate = r.f32();
        a.tyre_wear_rate = r.f32();
        a.force_virtual_mirror = r.bool();
        a.max_contacts_per_km = r.u8();
        a.race_over_time_ms = r.u32();
        a.result_screen_time_ms = r.u32();
        a.has_extra_lap = r.bool();
        a.gas_penalty_disabled = r.bool();
        a.pit_window_start = r.u16();
        a.pit_window_end = r.u16();
        a.inverted_grid_positions = r.i16();
        a.session_id = r.u8();
        let sessions = r.u8();
        for _ in 0..sessions {
            let kind = r.u8();
            // (the list has the laps before the time; message 0x4a has them the other way)
            let laps = r.u16();
            let time = r.u16();
            a.sessions.push(SessionEntry { kind: if (1..=3).contains(&kind) { kind } else { 1 }, laps, time });
        }
        if !a.sessions.iter().any(|s| s.kind == session_type::RACE) {
            // no race in the list: no pit window
            a.pit_window_start = 0;
            a.pit_window_end = 0;
        }
        a.current_session = SessionInfo::decode(&mut r, None);
        let files = r.u8();
        for _ in 0..files {
            a.checksum_files.push(r.string());
        }
        a.legal_tyres = r.string();
        a.random_seed = r.u32();
        HandshakeReply::Accepted(Box::new(a))
    }
}

/// One car of the server's list (`ClientRemoteCarDef`, 0x40's entry).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CarEntry {
    /// The slot.
    pub session_id: u8,
    /// Lower-cased, as the game does on receipt.
    pub model: String,
    pub skin: String,
    pub driver_name: String,
    pub team: String,
    pub nation_code: String,
    pub is_spectator: bool,
    /// Front, rear, left, right, centre.
    pub damage_zone_level: [f32; 5],
}

/// Message 0x3f: the list from `index` on (ten cars per answer).
pub fn car_list_request(index: u8) -> Vec<u8> {
    let mut w = Writer::with_id(id::CAR_LIST_REQUEST);
    w.u8(index);
    w.finish()
}

/// Message 0x40. `Err`: not the page that was asked for (the game asks again).
pub fn decode_car_list(message: &[u8], index: u8) -> Result<Vec<CarEntry>, String> {
    let mut r = Reader::new(message);
    let id = r.u8();
    if id != id::CAR_LIST {
        return Err(format!("message {id:#04x} instead of the car list"));
    }
    let echoed = r.u8();
    if echoed != index {
        return Err(format!("the car list from {echoed} instead of from {index}"));
    }
    let count = r.u8();
    let mut cars = Vec::with_capacity(count as usize);
    for _ in 0..count {
        let mut car = CarEntry { session_id: r.u8(), model: r.string().to_lowercase(), skin: r.string(), driver_name: r.string(), team: r.string(), nation_code: r.string(), is_spectator: r.bool(), ..CarEntry::default() };
        for level in &mut car.damage_zone_level {
            *level = r.f32();
        }
        cars.push(car);
    }
    if r.overrun {
        return Err("the car list is cut short".to_string());
    }
    Ok(cars)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handshake_request_layout() {
        let request = JoinRequest { guid: "81234567890123456".into(), name: "ab".into(), team: "t".into(), nation_code: "ITA".into(), car: "bmw_z4_gt3".into(), password: String::new() };
        let bytes = request.encode();
        assert_eq!(&bytes[..3], &[0x3d, 0xca, 0x00]);
        assert_eq!(bytes[3], 17);
        assert_eq!(&bytes[4..21], b"81234567890123456");
        // the name: two characters of four bytes
        assert_eq!(&bytes[21..30], &[2, b'a', 0, 0, 0, b'b', 0, 0, 0]);
        assert_eq!(&bytes[30..32], &[1, b't']);
        assert_eq!(*bytes.last().unwrap(), 0);
        assert_eq!(send_interval_ms(18), 54.0);
        assert_eq!(send_interval_ms(20), 48.0);
        assert_eq!(send_interval_ms(60), 15.0);
    }

    #[test]
    fn refusals() {
        assert_eq!(HandshakeReply::decode(&[0x3c]), HandshakeReply::Refused(Refusal::WrongPassword));
        assert_eq!(HandshakeReply::decode(&[0x45]), HandshakeReply::Refused(Refusal::NoSlots));
        assert_eq!(HandshakeReply::decode(&[]), HandshakeReply::Refused(Refusal::NoResponse));
        assert_eq!(HandshakeReply::decode(&[0x6f, 1, b'x', 0, 0, 0]), HandshakeReply::Refused(Refusal::AuthFailed("x".into())));
    }
}
