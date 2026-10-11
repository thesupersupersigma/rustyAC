// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! AC's `ACClient`: the online session of one player, over any [`Transport`].
//!
//! ```text
//! Client::join            ACClient::handshakeTCP        the handshake (question and answer)
//! Client::car_list        ACClient::getRemoteCarList    the cars, ten per page
//! Client::set_checksums   (handshake + initOnline)      the digests the server asked for
//! Client::begin_update_mode   ACClient::beginUpdateMode + askForP2Pvalue
//! Client::on_physics_step ACClient::onPhysicsStep       after every physics step: the car
//!                                                       update when the send interval is
//!                                                       over, then up to ten datagrams
//!                                                       and ten reliable messages
//! Client::update          ACClient::update              the checksum answer (once), the
//!                                                       session check every two seconds
//! Client::shutdown        ACClient::shutdown            the disconnect message
//! ```
//!
//! What arrives is turned into [`Event`]s for the game; the states of the other cars are
//! handed out per step ([`Client::take_car_states`]) for their
//! [`crate::net_car::NetCarStateProvider`]s.

use crate::car_update::{self, LocalCarState};
use crate::datagram::{self, Incoming};
use crate::handshake::{car_list_request, decode_car_list, Accepted, CarEntry, HandshakeReply, JoinRequest, Refusal, SessionInfo};
use crate::messages::{self, ClientEvent, Message};
use crate::net_car::NetCarState;
use crate::transport::Transport;

/// What the game hears of the session.
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    /// A message of the reliable channel, taken apart.
    Message(Message),
    /// The server knows this client's datagram address: car updates flow from now on.
    Associated,
    /// `ACP_RACE_START_UPDATE` was fresh: the race starts at this time of the physics clock.
    RaceStart(f64),
    /// No ping from the server for 20 seconds ("The server seems to be unavailable.").
    ServerSilent,
    /// The connection is gone.
    ConnectionLost(String),
}

/// How the join ended when it did not work.
#[derive(Clone, Debug, PartialEq)]
pub enum JoinError {
    Refused(Refusal),
    Transport(String),
}

impl std::fmt::Display for JoinError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            JoinError::Refused(refusal) => write!(f, "{}", refusal.message()),
            JoinError::Transport(message) => write!(f, "{message}"),
        }
    }
}

/// A collision of the local car, waiting for the next report (message 0x82).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CollisionReport {
    /// The slot of the car that was hit; `None`: the track.
    pub other_slot: Option<u8>,
    /// Closing speed, km/h.
    pub speed: f32,
    pub world_pos: [f32; 3],
    pub rel_pos: [f32; 3],
}

/// AC's `ACClient`.
pub struct Client<T: Transport> {
    pub transport: T,
    /// The handshake's answer: what to load and the server's rules.
    pub accepted: Accepted,
    /// The player's slot.
    pub session_id: u8,
    /// `sendInterval`, ms.
    pub send_interval: f64,
    pub last_send_time: f64,
    pub pak_sequence_index: u8,
    pub is_associated: bool,
    /// `endSession`: the session is over, nothing is sent.
    pub end_session: bool,
    /// `remoteFactor`: over how many ms a remote car's jump is faded out.
    pub remote_factor: f32,
    /// The session the server is in, and its start on the physics clock, ms.
    pub current_session: SessionInfo,
    pub session_start_time: f64,
    /// `lastSessionCheckTime`
    last_session_check_time: f64,
    /// `checksumResults` with the car's digest last; sent once by [`Client::update`].
    checksums: Option<Vec<[u8; 16]>>,
    /// `UDPSocket::ping`, `lastPingTime`
    pub ping: u16,
    pub last_ping_time: f64,
    silent_reported: bool,
    /// Collisions since the last report, and when that was sent.
    collisions: Vec<CollisionReport>,
    last_collision_report: f64,
    /// `lastChatTime`: at most one chat message a second.
    last_chat_time: f64,
    /// A headlight flash goes out with the next car update.
    pub flash_requested: bool,
    car_states: Vec<(u8, NetCarState)>,
    events: Vec<Event>,
    lost: bool,
    cars_in_session: usize,
    /// Datagrams and messages counted, for the displays.
    pub datagrams_in: u64,
    pub datagrams_out: u64,
    pub messages_in: u64,
}

/// How long the join waits for an answer, ms (the game waits for ever).
pub const JOIN_TIMEOUT_MS: u32 = 10_000;

impl<T: Transport> Client<T> {
    /// `ACClient::handshakeTCP` @ 0x140048b50: sends the request and reads the answer.
    pub fn join(mut transport: T, request: &JoinRequest) -> Result<Client<T>, JoinError> {
        transport.send_reliable(&request.encode()).map_err(JoinError::Transport)?;
        let reply = transport.wait_reliable(JOIN_TIMEOUT_MS).map_err(JoinError::Transport)?;
        let accepted = match reply.as_deref().map(HandshakeReply::decode) {
            Some(HandshakeReply::Accepted(accepted)) => *accepted,
            Some(HandshakeReply::Refused(refusal)) => return Err(JoinError::Refused(refusal)),
            None => return Err(JoinError::Refused(Refusal::NoResponse)),
        };
        // serverIP = IPAddress(SERVER_IP, the reply's UDP port)
        transport.set_datagram_port(accepted.udp_port);
        Ok(Client {
            transport,
            session_id: accepted.session_id,
            send_interval: accepted.send_interval_ms(),
            last_send_time: 0.0,
            pak_sequence_index: 0,
            is_associated: false,
            end_session: false,
            remote_factor: 250.0,
            current_session: accepted.current_session.clone(),
            session_start_time: 0.0,
            last_session_check_time: -1.0,
            checksums: None,
            ping: 0,
            last_ping_time: 0.0,
            silent_reported: false,
            collisions: Vec::new(),
            last_collision_report: 0.0,
            last_chat_time: f64::NEG_INFINITY,
            flash_requested: false,
            car_states: Vec::new(),
            events: Vec::new(),
            lost: false,
            cars_in_session: 1,
            datagrams_in: 0,
            datagrams_out: 0,
            messages_in: 0,
            accepted,
        })
    }

    /// `ACClient::getRemoteCarList` @ 0x140046f80: every car of the server, the player's
    /// own slot and spectators included (the game makes remote cars of the others).
    /// Messages of other kinds that arrive meanwhile are kept for later.
    pub fn car_list(&mut self) -> Result<Vec<CarEntry>, String> {
        let mut cars = Vec::new();
        let mut index = 0u8;
        let mut strays = 0;
        loop {
            self.transport.send_reliable(&car_list_request(index))?;
            let page = self.transport.wait_reliable(JOIN_TIMEOUT_MS)?.ok_or("the server did not send its car list")?;
            let entries = match decode_car_list(&page, index) {
                Ok(entries) => entries,
                // another message came first: the game drops it and asks again; here it is
                // handled like any message of the session
                Err(_) if strays < 32 => {
                    strays += 1;
                    self.on_reliable(&page);
                    continue;
                }
                Err(message) => return Err(message),
            };
            let count = entries.len();
            cars.extend(entries);
            // a full page: there may be more (a page that is not full ends the list)
            if count != 10 {
                return Ok(cars);
            }
            index = index.wrapping_add(10);
        }
    }

    /// The digests of the files the handshake listed, in its order, and the digest of the
    /// player's car data (`ACClient::getCarMD5`). [`Client::update`] sends them once.
    pub fn set_checksums(&mut self, mut files: Vec<[u8; 16]>, car: [u8; 16]) {
        files.push(car);
        self.checksums = Some(files);
    }

    /// `ACClient::beginUpdateMode` @ 0x1400446e0 and `askForP2Pvalue` @ 0x140044470: the
    /// session starts for this client. `session_elapsed_clock`: the physics clock, ms, the
    /// session started at (`ksGetTime() - elapsed` at the handshake).
    pub fn begin_update_mode(&mut self, session_start_time: f64) -> Result<(), String> {
        self.session_start_time = session_start_time;
        self.transport.send_reliable(&messages::ask_p2p())
    }

    /// `ACClient::onPhysicsStep` @ 0x14004f1d0. `physics_time`: the engine's clock of the
    /// step, ms; `clock_ms`: the wall clock on the same scale (the game's `ksGetTime`), for
    /// the ping's answer; `car`: the local car as it is after `dWorldStep` (asked only when
    /// an update goes out).
    pub fn on_physics_step(&mut self, physics_time: f64, clock_ms: f64, car: &mut dyn FnMut() -> LocalCarState) {
        if self.lost {
            return;
        }
        self.send_car_position(physics_time, car);
        // UDPSocket::receive(10): ten datagrams at most, the server's pings not counted
        let mut taken = 0;
        let mut pings = 0;
        while taken < 10 && pings < 64 {
            match self.transport.poll_datagram() {
                Ok(Some(datagram)) => {
                    if datagram.first() == Some(&datagram::id::PING) {
                        pings += 1;
                    } else {
                        taken += 1;
                    }
                    self.on_datagram(&datagram, physics_time, clock_ms);
                }
                Ok(None) => break,
                Err(message) => {
                    self.lose(message);
                    return;
                }
            }
        }
        // TCPSocket::receive(10)
        for _ in 0..10 {
            match self.transport.poll_reliable() {
                Ok(Some(message)) => self.on_reliable(&message),
                Ok(None) => break,
                Err(message) => {
                    self.lose(message);
                    return;
                }
            }
        }
        // the collisions of the last five seconds
        if physics_time - self.last_collision_report > 5000.0 {
            if !self.collisions.is_empty() {
                let report = messages::collision_report(&self.collisions);
                self.collisions.clear();
                self.send(&report);
            }
            self.last_collision_report = physics_time;
        }
    }

    /// `ACClient::sendCarPosition` @ 0x140052230.
    fn send_car_position(&mut self, physics_time: f64, car: &mut dyn FnMut() -> LocalCarState) {
        // comisd + jb: not before the interval is over
        if !(physics_time - self.last_send_time >= self.send_interval) || self.end_session {
            return;
        }
        if !self.is_associated {
            // ACClient::associate, until the server echoes it
            self.send_datagram(&datagram::association(self.session_id));
            self.last_send_time = physics_time;
            return;
        }
        let mut state = car();
        state.flash = self.flash_requested;
        self.flash_requested = false;
        let update = car_update::encode(self.pak_sequence_index, physics_time, &state);
        self.pak_sequence_index = self.pak_sequence_index.wrapping_add(1);
        self.send_datagram(&update);
        self.last_send_time = physics_time;
    }

    fn on_datagram(&mut self, datagram: &[u8], physics_time: f64, clock_ms: f64) {
        self.datagrams_in += 1;
        for incoming in datagram::decode(datagram) {
            match incoming {
                Incoming::Ping { ping, datagram } => {
                    // UDPSocket::receive answers the ping itself
                    self.ping = ping;
                    self.last_ping_time = clock_ms;
                    self.silent_reported = false;
                    self.send_datagram(&datagram::pong(&datagram, clock_ms));
                }
                Incoming::Associated => {
                    if !self.is_associated {
                        self.events.push(Event::Associated);
                    }
                    self.is_associated = true;
                }
                Incoming::CarState { slot, state } => self.car_states.push((slot, state)),
                Incoming::RaceStart { start, real, ping } => {
                    // fresh when the server's idea of this client's clock is within two pings
                    let fresh = (physics_time - real as f64).abs() < (2 * ping as i32) as f64;
                    if fresh {
                        self.session_start_time = start as f64;
                        self.events.push(Event::RaceStart(start as f64));
                    }
                }
                Incoming::Unknown(_) | Incoming::Truncated(_) => {}
            }
        }
    }

    fn on_reliable(&mut self, message: &[u8]) {
        self.messages_in += 1;
        let cars = self.cars_in_session;
        for decoded in messages::decode(message, cars) {
            if let Message::NewSession(info) = &decoded {
                self.current_session = info.clone();
                self.session_start_time = info.start as f64;
                self.end_session = false;
            }
            self.events.push(Event::Message(decoded));
        }
    }

    fn lose(&mut self, message: String) {
        if !self.lost {
            self.lost = true;
            self.events.push(Event::ConnectionLost(message));
        }
    }

    fn send(&mut self, message: &[u8]) {
        if self.lost {
            return;
        }
        if let Err(message) = self.transport.send_reliable(message) {
            self.lose(message);
        }
    }

    fn send_datagram(&mut self, datagram: &[u8]) {
        if self.lost {
            return;
        }
        self.datagrams_out += 1;
        if let Err(message) = self.transport.send_datagram(datagram) {
            self.lose(message);
        }
    }

    /// `ACClient::update` @ 0x140053b30, once a frame with the wall clock (`now`, ms): the
    /// checksum answer, the session check, the server's silence.
    pub fn update(&mut self, now: f64) {
        if self.lost {
            return;
        }
        if let Some(digests) = self.checksums.take() {
            let answer = messages::checksum_answer(&digests);
            self.send(&answer);
        }
        if self.last_session_check_time <= 0.0 {
            // the first check: five seconds from now, then every two
            self.last_session_check_time = now + 5000.0;
        } else if now - self.last_session_check_time > 2000.0 {
            self.send_datagram(&datagram::session_check(self.current_session.kind));
            self.last_session_check_time = now;
        }
        if self.is_associated && self.last_ping_time > 0.0 && now - self.last_ping_time > 20_000.0 && !self.silent_reported {
            self.silent_reported = true;
            self.events.push(Event::ServerSilent);
        }
    }

    /// The states of other cars that arrived in the last step, in arrival order.
    pub fn take_car_states(&mut self) -> Vec<(u8, NetCarState)> {
        std::mem::take(&mut self.car_states)
    }

    pub fn take_events(&mut self) -> Vec<Event> {
        std::mem::take(&mut self.events)
    }

    pub fn is_lost(&self) -> bool {
        self.lost
    }

    /// `ACClient::addCollisionEvent` @ 0x140043aa0: a collision of the local car, for the
    /// next report.
    pub fn add_collision(&mut self, report: CollisionReport) {
        // one entry per car that was hit (and one for everything else): the hardest hit
        match self.collisions.iter_mut().find(|known| known.other_slot == report.other_slot) {
            Some(known) => {
                if report.speed > known.speed {
                    *known = report;
                }
            }
            None => self.collisions.push(report),
        }
    }

    /// What the local car did that the server hears of at once (laps, splits, tyres ...).
    pub fn send_event(&mut self, event: &ClientEvent) {
        let message = messages::encode(self.session_id, event);
        self.send(&message);
    }

    /// `ACClient::sendChat` @ 0x140052a90: at most one message a second (`now`: the wall
    /// clock, ms). Returns whether it went out.
    pub fn send_chat(&mut self, text: &str, now: f64) -> bool {
        if now - self.last_chat_time < 1000.0 || text.is_empty() {
            return false;
        }
        self.last_chat_time = now;
        let message = messages::chat(self.session_id, text);
        self.send(&message);
        true
    }

    /// `ACClient::shutdown` @ 0x140052d30: tells the server the player is leaving.
    pub fn shutdown(&mut self) {
        self.send(&messages::disconnect());
        self.lost = true;
    }

    /// How many cars the session has (`netCars + 1`): message 0x4a lists a slot for each.
    pub fn set_cars_in_session(&mut self, cars: usize) {
        self.cars_in_session = cars;
    }
}
