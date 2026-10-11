// SPDX-License-Identifier: GPL-3.0-or-later

//! An online session inside the game: the client of a server ([`rustyac_net::client`]),
//! the other players' cars ([`rustyac_net::net_car`]) as bodies of the physics engine, and
//! what the displays show of both.
//!
//! Everything here runs on the physics thread, where Assetto Corsa runs it: the client's
//! work after every step (`ACClient::onPhysicsStep`) before the local car's `Car::postStep`,
//! the remote cars' steps (`NetCarStateProvider::step`) after it. See
//! [`rustyac_physics::engine::StepHooks`].
//!
//! No platform code: the transport is a trait object (TCP and UDP on the desktop, a
//! WebSocket in the browser build to come).

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Arc;

use rustyac_net::car_update::LocalCarState;
use rustyac_net::client::{Client, CollisionReport, Event};
use rustyac_net::handshake::{session_type, Accepted, CarEntry};
use rustyac_net::messages::Message;
use rustyac_net::net_car::{NetCarGeometry, NetCarStateProvider, StepWorld};
use rustyac_net::transport::Transport;
use rustyac_physics::car::{PhysicsCore, RollingChassis};
use rustyac_physics::engine::{CarSlot, StepHooks};
use rustyac_physics::track::{Track, TrackGround};
use rustyac_physics::vecmath::{Mat44f, Vec3f};

/// The transport of a session: any, behind a box.
pub type BoxedTransport = Box<dyn Transport + Send>;

/// What the server dictates of the physics (the handshake's reply), as the game applies it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ServerRules {
    /// -1: no limit.
    pub allowed_tyres_out: i32,
    pub tyre_blankets: bool,
    /// 0 off, 1 the car's own, 2 for everybody.
    pub tc: i32,
    pub abs: i32,
    pub stability: bool,
    pub auto_clutch: bool,
    pub jump_start_mode: i32,
    pub damage_rate: f32,
    pub fuel_rate: f32,
    pub tyre_wear_rate: f32,
    /// The session: its kind (1 practice, 2 qualifying, 3 race) and the track's grip.
    pub session_type: i32,
    pub grip: f32,
    pub gas_penalty_disabled: bool,
}

impl ServerRules {
    pub fn of(accepted: &Accepted) -> ServerRules {
        ServerRules {
            allowed_tyres_out: accepted.allowed_tyres_out as i32,
            tyre_blankets: accepted.tyre_blankets,
            tc: accepted.tc as i32,
            abs: accepted.abs as i32,
            stability: accepted.stability,
            auto_clutch: accepted.auto_clutch,
            jump_start_mode: accepted.jump_start_mode as i32,
            damage_rate: accepted.damage_rate,
            fuel_rate: accepted.fuel_rate,
            tyre_wear_rate: accepted.tyre_wear_rate,
            session_type: accepted.current_session.kind as i32,
            grip: accepted.current_session.grip,
            gas_penalty_disabled: accepted.gas_penalty_disabled,
        }
    }

    /// `engine.penaltyMode` of a session (the job `ACClient::onNewSession` queues): losing
    /// the lap in practice and qualifying; in a race the cut-the-throttle penalty, or only
    /// the detection when the server switched that off.
    pub fn penalty_mode(&self) -> i32 {
        if self.session_type == session_type::RACE as i32 {
            if self.gas_penalty_disabled {
                4
            } else {
                0
            }
        } else {
            1
        }
    }
}

/// One slot of the server, for the player list.
#[derive(Clone, Debug, PartialEq)]
pub struct RosterEntry {
    pub slot: u8,
    pub model: String,
    pub driver: String,
    /// The player's own slot.
    pub me: bool,
    pub connected: bool,
    pub ping: i32,
    pub laps: u32,
    pub last_lap_ms: u32,
    pub best_lap_ms: u32,
}

/// Another player's car.
pub struct RemoteCar {
    pub entry: CarEntry,
    pub data_path: PathBuf,
    pub provider: NetCarStateProvider,
    /// `Car::damageZoneLevel` as the server tells it.
    pub damage: [f32; 5],
}

/// The online session of a [`crate::sim::GameSim`].
pub struct OnlineSession {
    pub client: Client<BoxedTransport>,
    /// The other slots' cars in the order of the server's list (spectators left out).
    pub remote: Vec<RemoteCar>,
    pub roster: Vec<RosterEntry>,
    /// The last lines of the chat and of the server's notices, newest last.
    pub chat: VecDeque<String>,
    /// Lines for the console since they were last taken.
    pub console: Vec<String>,
    /// What the game has to act on, in arrival order.
    pub pending: Vec<Event>,
    pub track: Option<Arc<Track>>,
    /// The cars that become remote cars when the world is built (then empty).
    pub remote_entries: Vec<CarEntry>,
    /// The driver's horn (sent with the car update).
    pub horn: bool,
    /// Steps so far (the tape's clock).
    steps: u64,
}

/// How many chat lines are kept.
const CHAT_LINES: usize = 6;

/// `Car::getFrontWheelAngle` @ 0x140270990: the mean of the two front hubs' turn about the
/// body's vertical, radians.
#[allow(clippy::neg_cmp_op_on_partial_ord)]
pub fn front_wheel_angle(car: &RollingChassis) -> f32 {
    if car.suspensions.len() < 2 {
        return 0.0;
    }
    let turn = |wheel: usize| -> (f32, f32) {
        let hub = car.suspensions[wheel].get_hub_world_matrix(&car.core);
        let forward = Vec3f::new(-hub.m[2][0], -hub.m[2][1], -hub.m[2][2]);
        let local = car.core.world_to_local_normal(car.body, &forward);
        let (mut x, mut z) = (local.x, local.z);
        let length = rustyac_math::sqrtf(x * x + z * z);
        if length < 0.0 || length > 0.0 {
            let inv = 1.0f32 / length;
            x *= inv;
            z *= inv;
        }
        (x, z)
    };
    let angle = |x: f32, z: f32| -> f32 {
        let cosine = -z;
        let angle = if cosine > -1.0 && cosine < 1.0 { rustyac_math::acosf(cosine) } else { 0.0 };
        let sign = if x > 0.0 {
            1.0f32
        } else if x == 0.0 {
            0.0
        } else {
            -1.0
        };
        sign * angle
    };
    let (x0, z0) = turn(0);
    let (x1, z1) = turn(1);
    let first = angle(x0, z0);
    (angle(x1, z1) + first) * 0.5
}

/// What `ACClient::sendCarPosition` reads of the local car.
pub fn local_car_state(car: &RollingChassis, horn: bool) -> LocalCarState {
    let velocity = car.core.get_velocity(car.body);
    let mut state = LocalCarState {
        body_matrix: car.core.get_world_matrix(car.body),
        velocity: [velocity.x, velocity.y, velocity.z],
        speed: car.speed,
        steer: car.controls.steer,
        steer_lock: car.steer_lock,
        front_wheel_angle: front_wheel_angle(car),
        gas: car.controls.gas,
        brake: car.controls.brake,
        lights_on: car.lights_on,
        horn,
        penalty_time: car.get_penalty_time(),
        ers_is_charging: car.ers.as_ref().is_some_and(|ers| ers.is_charging),
        drs_open: car.aero.as_ref().is_some_and(|aero| aero.base().drs.is_present && aero.base().drs.is_active),
        performance_split: car.performance_split,
        normalized_position: car.spline_locator_data.npos,
        ..LocalCarState::default()
    };
    for (wheel, tyre) in car.tyres.iter().take(4).enumerate() {
        state.wheel_angular_velocity[wheel] = tyre.status.angular_velocity;
        state.wheel_slip_ratio[wheel] = tyre.status.slip_ratio;
    }
    if let Some(drivetrain) = &car.drivetrain {
        state.engine_rpm = drivetrain.get_engine_rpm();
        state.current_gear = drivetrain.base().current_gear;
    }
    state
}

impl OnlineSession {
    /// A session after the handshake and the car list (`cars`: every entry of the list).
    pub fn new(client: Client<BoxedTransport>, cars: &[CarEntry]) -> OnlineSession {
        let me = client.session_id;
        let roster = cars
            .iter()
            .map(|car| RosterEntry {
                slot: car.session_id,
                model: car.model.clone(),
                driver: car.driver_name.clone(),
                me: car.session_id == me,
                connected: car.session_id == me || !car.driver_name.is_empty(),
                ping: 0,
                laps: 0,
                last_lap_ms: 0,
                best_lap_ms: 0,
            })
            .collect();
        let remote_entries = OnlineSession::remote_entries(cars, me);
        OnlineSession { client, remote: Vec::new(), roster, chat: VecDeque::new(), console: Vec::new(), pending: Vec::new(), track: None, remote_entries, horn: false, steps: 0 }
    }

    /// The cars the game makes remote cars of: every slot but the player's and the
    /// spectators', in the list's order (`ACClient::getRemoteCarList`).
    pub fn remote_entries(cars: &[CarEntry], me: u8) -> Vec<CarEntry> {
        cars.iter().filter(|car| car.session_id != me && !car.is_spectator).cloned().collect()
    }

    /// `Sim::addNetCar` @ 0x140197c30, the physics side: the body of the car in `entry`'s
    /// slot in the engine's world. `guid`: its number among the session's cars (1 for the
    /// first remote car). `mesh`: the car's collider.
    pub fn add_remote_car(&mut self, core: &mut PhysicsCore, entry: CarEntry, data_path: PathBuf, guid: u32, mesh: Option<&rustyac_physics::car::ColliderMesh>, pit_box: Mat44f) {
        let geometry = NetCarGeometry::load(&data_path);
        let provider = NetCarStateProvider::new(core, entry.session_id, guid, &entry.model, geometry, mesh, pit_box);
        let damage = entry.damage_zone_level;
        self.remote.push(RemoteCar { entry, data_path, provider, damage });
    }

    fn say(&mut self, line: String) {
        self.console.push(line.clone());
        self.chat.push_back(line);
        while self.chat.len() > CHAT_LINES {
            self.chat.pop_front();
        }
    }

    fn driver_of(&self, slot: u8) -> String {
        match self.roster.iter().find(|entry| entry.slot == slot) {
            Some(entry) if !entry.driver.is_empty() => entry.driver.clone(),
            _ => format!("car {slot}"),
        }
    }

    /// What arrived, for the displays; what the game must act on is left in `pending`.
    fn digest(&mut self) {
        for event in self.client.take_events() {
            match &event {
                Event::Message(Message::Chat { slot, text }) => {
                    let who = self.driver_of(*slot);
                    self.say(format!("{who}: {text}"));
                }
                Event::Message(Message::NewSession(info)) => {
                    self.say(format!("New Session : {}", info.name));
                    for entry in &mut self.roster {
                        entry.laps = 0;
                        entry.last_lap_ms = 0;
                        entry.best_lap_ms = 0;
                    }
                }
                Event::Associated => self.console.push("online: the server answers on UDP, car updates flow".to_string()),
                Event::ServerSilent => self.say("The server seems to be unavailable.".to_string()),
                Event::ConnectionLost(message) => self.say(format!("Connection lost: {message}")),
                _ => {}
            }
            self.pending.push(event);
        }
    }

    /// What the session saw of every driver, for the console at the end.
    pub fn summary(&self) -> Vec<String> {
        let mut lines = Vec::new();
        for entry in self.roster.iter().filter(|entry| entry.connected) {
            let remote = self.remote.iter().find(|remote| remote.provider.session_id == entry.slot);
            let packets = match remote {
                Some(remote) => format!(
                    ", states taken {} in order + {} out of order, ping {} ms, {}",
                    remote.provider.qos_good_packets,
                    remote.provider.qos_bad_packets,
                    remote.provider.ping,
                    if remote.provider.is_disconnected { "not there at the end" } else { "there at the end" }
                ),
                None => String::new(),
            };
            lines.push(format!(
                "online:   slot {:2} {:<20} {:<18} laps {} (best {}){}{}",
                entry.slot,
                if entry.driver.is_empty() { "?" } else { &entry.driver },
                entry.model,
                entry.laps,
                if entry.best_lap_ms > 0 { lap_text(entry.best_lap_ms) } else { "-".to_string() },
                if entry.me { "  <- you" } else { "" },
                packets
            ));
        }
        lines
    }

    /// The lines of the player list and the chat, for the HUD.
    pub fn hud_lines(&self) -> Vec<String> {
        let mut lines = Vec::new();
        let session = &self.client.current_session;
        lines.push(format!("ONLINE  {}  ({})  ping {} ms", self.client.accepted.server_name, session.name, self.client.ping));
        for entry in self.roster.iter().filter(|entry| entry.connected) {
            let best = if entry.best_lap_ms > 0 { crate::online::lap_text(entry.best_lap_ms) } else { "-:--.---".to_string() };
            let name = if entry.driver.is_empty() { "?" } else { &entry.driver };
            lines.push(format!("{} {:<20} {:<18} laps {:<3} best {}{}", if entry.me { ">" } else { " " }, name, entry.model, entry.laps, best, if entry.me { String::new() } else { format!("  {} ms", entry.ping) }));
        }
        lines.extend(self.chat.iter().cloned());
        lines
    }
}

/// A lap time as `m:ss.mmm`.
pub fn lap_text(ms: u32) -> String {
    format!("{}:{:02}.{:03}", ms / 60_000, ms / 1000 % 60, ms % 1000)
}

/// The session's work in the step-completed phase of a physics step.
pub struct OnlineHooks<'a> {
    pub session: &'a mut OnlineSession,
}

impl StepHooks for OnlineHooks<'_> {
    /// `ACClient::onPhysicsStep`: the client was the first to hang a handler on the engine.
    fn before_cars(&mut self, _core: &mut PhysicsCore, cars: &mut [CarSlot<'_>], physics_time: f64) {
        let session = &mut *self.session;
        session.client.transport.set_tick(session.steps);
        session.steps += 1;
        let Some(slot) = cars.first() else {
            return;
        };
        let car = &*slot.car;
        let horn = session.horn;
        // the wall clock of the pong and of the frame's work is the physics clock here: the
        // two run alike, and a session then repeats exactly from a tape
        session.client.on_physics_step(physics_time, physics_time, &mut || local_car_state(car, horn));
        session.client.update(physics_time);
        let send_interval = session.client.send_interval;
        for (slot, state) in session.client.take_car_states() {
            // (the first car of the list with that slot; the player's own slot has none)
            if let Some(remote) = session.remote.iter_mut().find(|remote| remote.provider.session_id == slot) {
                remote.provider.on_remote_state_received(state, physics_time, send_interval);
            }
        }
        session.digest();
    }

    /// `NetCarStateProvider::step` of every remote car, in the order they were made.
    fn after_cars(&mut self, core: &mut PhysicsCore, cars: &mut [CarSlot<'_>], physics_time: f64) {
        let session = &mut *self.session;
        let Some(slot) = cars.first() else {
            return;
        };
        let car = &*slot.car;
        // the handler ACClient hangs on the local car's collision event: the remote car that
        // was hit keeps part of the solver's velocity for a second, and the hit is reported
        for event in &car.collision_events {
            let other = event.body.and_then(|body| session.remote.iter_mut().find(|remote| remote.provider.body == body));
            let other_slot = match other {
                Some(remote) => {
                    remote.provider.on_local_collision();
                    Some(remote.provider.session_id)
                }
                None => None,
            };
            // (a hit of anything that is no remote car goes in as a hit of the surroundings)
            session.client.add_collision(CollisionReport {
                other_slot,
                speed: event.rel_speed,
                world_pos: [event.world_pos.x, event.world_pos.y, event.world_pos.z],
                rel_pos: [event.rel_pos.x, event.rel_pos.y, event.rel_pos.z],
            });
        }
        let Some(track) = session.track.clone() else {
            return;
        };
        let ground = TrackGround(track);
        let local_in_pit_lane = car.is_in_pit_lane();
        let local_body = Some(car.body);
        let remote_factor = session.client.remote_factor;
        for remote in &mut session.remote {
            let mut world = StepWorld { core, ground: &ground, remote_factor, local_in_pit_lane, local_body };
            remote.provider.step(physics_time, &mut world);
        }
        // the displays: who is there
        for entry in &mut session.roster {
            if let Some(remote) = session.remote.iter().find(|remote| remote.provider.session_id == entry.slot) {
                entry.ping = remote.provider.ping;
                if !remote.provider.is_disconnected {
                    entry.connected = true;
                }
            }
        }
    }
}
