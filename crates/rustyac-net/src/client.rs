// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! AC's `ACClient`: the online session of one player, over any [`Transport`].
//!
//! ```text
//! Join::start             ACClient::handshakeTCP        the handshake (question and answer)
//! Join (the car list)     ACClient::getRemoteCarList    the cars, ten per page
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

// Comparisons are spelled the way the original branches so NaN takes the same path.
#![allow(clippy::neg_cmp_op_on_partial_ord, clippy::double_comparisons, clippy::manual_clamp, clippy::needless_range_loop)]

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

/// Where a [`Join`] is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JoinStage {
    /// The request is out; the server's answer has not come.
    Handshake,
    /// The server let the player in; the car list is being asked for, ten cars a page.
    CarList,
    /// The client and the car list are ready ([`Join::finish`]).
    Done,
}

enum JoinPhase<T: Transport> {
    Handshake(T),
    CarList { client: Box<Client<T>>, cars: Vec<CarEntry>, index: u8, strays: u32 },
    Done { client: Box<Client<T>>, cars: Vec<CarEntry> },
    /// It failed (or was finished): nothing is left.
    Over,
}

/// The join: `ACClient::handshakeTCP` @ 0x140048b50 and `ACClient::getRemoteCarList`
/// @ 0x140046f80 as a state machine. The game sends a question and blocks for the answer,
/// five to fifteen times in a row; a browser cannot block, so here every answer is a
/// message that is handed in ([`Join::on_message`]) and moves the join one stage on. The
/// messages on the wire and their order are the game's.
///
/// * the desktop drives it with [`Join::wait`], which blocks in the transport;
/// * the browser calls [`Join::poll`] once a frame until it says [`JoinStage::Done`].
pub struct Join<T: Transport> {
    phase: JoinPhase<T>,
}

impl<T: Transport> Join<T> {
    /// Sends the join request.
    pub fn start(mut transport: T, request: &JoinRequest) -> Result<Join<T>, JoinError> {
        transport.send_reliable(&request.encode()).map_err(JoinError::Transport)?;
        Ok(Join { phase: JoinPhase::Handshake(transport) })
    }

    pub fn stage(&self) -> JoinStage {
        match self.phase {
            JoinPhase::Handshake(_) | JoinPhase::Over => JoinStage::Handshake,
            JoinPhase::CarList { .. } => JoinStage::CarList,
            JoinPhase::Done { .. } => JoinStage::Done,
        }
    }

    /// The server's answer to the handshake, once it let the player in.
    pub fn accepted(&self) -> Option<&Accepted> {
        match &self.phase {
            JoinPhase::CarList { client, .. } | JoinPhase::Done { client, .. } => Some(&client.accepted),
            _ => None,
        }
    }

    /// Every car of the server, once the list is whole.
    pub fn cars(&self) -> Option<&[CarEntry]> {
        match &self.phase {
            JoinPhase::Done { cars, .. } => Some(cars),
            _ => None,
        }
    }

    /// One message of the reliable channel. After an error the join is over (a player the
    /// server had let in has said goodbye).
    pub fn on_message(&mut self, message: &[u8]) -> Result<JoinStage, JoinError> {
        match std::mem::replace(&mut self.phase, JoinPhase::Over) {
            JoinPhase::Handshake(mut transport) => {
                let accepted = match HandshakeReply::decode(message) {
                    HandshakeReply::Accepted(accepted) => *accepted,
                    HandshakeReply::Refused(refusal) => return Err(JoinError::Refused(refusal)),
                };
                // serverIP = IPAddress(SERVER_IP, the reply's UDP port)
                transport.set_datagram_port(accepted.udp_port);
                let mut client = Box::new(Client::new(transport, accepted));
                // getRemoteCarList: the first page
                if let Err(why) = client.transport.send_reliable(&car_list_request(0)) {
                    client.shutdown();
                    return Err(JoinError::Transport(why));
                }
                self.phase = JoinPhase::CarList { client, cars: Vec::new(), index: 0, strays: 0 };
            }
            JoinPhase::CarList { mut client, mut cars, mut index, mut strays } => {
                let fail = |mut client: Box<Client<T>>, why: String| {
                    client.shutdown();
                    Err(JoinError::Transport(why))
                };
                match decode_car_list(message, index) {
                    Ok(entries) => {
                        let count = entries.len();
                        cars.extend(entries);
                        // (a slot is one byte: a server cannot have more cars than that)
                        if cars.len() > 256 {
                            return fail(client, "the server's car list does not end".to_string());
                        }
                        // a full page: there may be more (a page that is not full ends the list)
                        if count != 10 {
                            self.phase = JoinPhase::Done { client, cars };
                            return Ok(JoinStage::Done);
                        }
                        index = index.wrapping_add(10);
                    }
                    // another message came first: the game drops it and asks again; here it
                    // is handled like any message of the session
                    Err(_) if strays < 32 => {
                        strays += 1;
                        client.on_reliable(message);
                    }
                    Err(why) => return fail(client, why),
                }
                if let Err(why) = client.transport.send_reliable(&car_list_request(index)) {
                    return fail(client, why);
                }
                self.phase = JoinPhase::CarList { client, cars, index, strays };
            }
            done @ JoinPhase::Done { .. } => self.phase = done,
            JoinPhase::Over => return Err(JoinError::Transport("the join is over".to_string())),
        }
        Ok(self.stage())
    }

    fn transport(&mut self) -> Option<&mut T> {
        match &mut self.phase {
            JoinPhase::Handshake(transport) => Some(transport),
            JoinPhase::CarList { client, .. } => Some(&mut client.transport),
            _ => None,
        }
    }

    fn lose(&mut self, why: String) -> JoinError {
        if let JoinPhase::CarList { mut client, .. } = std::mem::replace(&mut self.phase, JoinPhase::Over) {
            client.shutdown();
        }
        JoinError::Transport(why)
    }

    /// Takes the answers that have arrived, without waiting.
    pub fn poll(&mut self) -> Result<JoinStage, JoinError> {
        loop {
            let Some(transport) = self.transport() else {
                return if self.stage() == JoinStage::Done { Ok(JoinStage::Done) } else { Err(JoinError::Transport("the join is over".to_string())) };
            };
            match transport.poll_reliable() {
                Ok(Some(message)) => self.on_message(&message)?,
                Ok(None) => return Ok(self.stage()),
                Err(why) => return Err(self.lose(why)),
            };
        }
    }

    /// Runs the join to its end, waiting up to `timeout_ms` for each answer (the game
    /// waits for ever).
    pub fn wait(&mut self, timeout_ms: u32) -> Result<(), JoinError> {
        loop {
            let stage = self.stage();
            let Some(transport) = self.transport() else {
                return if stage == JoinStage::Done { Ok(()) } else { Err(JoinError::Transport("the join is over".to_string())) };
            };
            match transport.wait_reliable(timeout_ms) {
                Ok(Some(message)) => self.on_message(&message)?,
                Ok(None) if stage == JoinStage::Handshake => {
                    self.phase = JoinPhase::Over;
                    return Err(JoinError::Refused(Refusal::NoResponse));
                }
                Ok(None) => return Err(self.lose("the server did not send its car list".to_string())),
                Err(why) => return Err(self.lose(why)),
            };
        }
    }

    /// The client and every car of the server (the player's own slot and spectators
    /// included; the game makes remote cars of the others), once the join is done.
    pub fn finish(self) -> Option<(Client<T>, Vec<CarEntry>)> {
        match self.phase {
            JoinPhase::Done { client, cars } => Some((*client, cars)),
            _ => None,
        }
    }

    /// Gives the join up: a player the server had let in says goodbye.
    pub fn abandon(self) {
        if let JoinPhase::CarList { mut client, .. } | JoinPhase::Done { mut client, .. } = self.phase {
            client.shutdown();
        }
    }
}

impl<T: Transport> Client<T> {
    /// The client of a server that let the player in.
    fn new(transport: T, accepted: Accepted) -> Client<T> {
        Client {
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
        }
    }

    /// The whole join, waiting for each answer: [`Join`] run to its end.
    pub fn join(transport: T, request: &JoinRequest) -> Result<(Client<T>, Vec<CarEntry>), JoinError> {
        let mut join = Join::start(transport, request)?;
        join.wait(JOIN_TIMEOUT_MS)?;
        join.finish().ok_or(JoinError::Transport("the join did not end".to_string()))
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
                // (a state that is no number is dropped: the game would put it into the
                // world, where the local car can touch it)
                Incoming::CarState { slot, state } => {
                    if state.pos.iter().chain(&state.rotation).chain(&state.velocity).all(|v| v.is_finite()) {
                        self.car_states.push((slot, state));
                    }
                }
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
            // isConnectionFinished: the server goes on without this client
            if let Message::RaceOver { stay: false, .. } = &decoded {
                self.end_session = true;
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
        // (a vote without its target, an unknown vote: the game sends nothing)
        if !message.is_empty() {
            self.send(&message);
        }
    }

    /// `ACClient::sendChat` @ 0x140052a90: at most one message a second (`now`: the wall
    /// clock, ms). Returns whether it went out.
    pub fn send_chat(&mut self, text: &str, now: f64) -> bool {
        // (`/client_list` is the game's own command: the game prints its list and sends
        // nothing; the caller prints rustyAC's)
        if !(now - self.last_chat_time > 1000.0) || text.is_empty() || text == "/client_list" {
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
