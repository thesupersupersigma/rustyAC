// SPDX-License-Identifier: GPL-3.0-or-later

//! An online session played again from a tape, for the proofs: the engine with the local
//! car of a recording ([`rustyac_physics::engine::record`]), the client fed by a
//! [`TapePlayer`], and the other players' cars. `tools/car_oracle net` runs the game's own
//! `ACClient::onPhysicsStep` and `NetCarStateProvider::step` beside this and compares every
//! value of [`NetReplay::remote_row`] and every outgoing message after every step; the
//! golden test replays a stored excerpt.
//!
//! Two kinds of outgoing messages are not held against the game's ([`comparable`]):
//!
//! * the session check (`4f`): the game sends it from its frame loop, by the wall clock;
//! * the time in the answer to a ping (bytes 5..9 of `f8`): the wall clock.

use std::sync::Arc;

use rustyac_physics::car::colliders;
use rustyac_physics::engine::record::{Recording, ReplayPaths, ReplayWorld};
use rustyac_physics::track::Track;
use rustyac_physics::vecmath::Mat44f;

use crate::client::Client;
use crate::handshake::{CarEntry, JoinRequest};
use crate::net_car::{NetCarGeometry, NetCarStateProvider};
use crate::session::NetSession;
use crate::transport::{Lane, Tape, TapePlayer};

/// The words of a remote car that are compared, in order, as (name, words).
pub const REMOTE_FIELDS: [(&str, usize); 25] = [
    ("body.pos", 3),
    ("body.q", 4),
    ("body.lvel", 3),
    ("body.avel", 3),
    ("meshCategoryBits", 1),
    ("bodyMatrix", 16),
    ("errorVector", 3),
    ("instantVelocity", 3),
    ("smoothVelocity", 3),
    ("hasCollisionInThisStep", 1),
    ("isDisconnected", 1),
    ("netStates0.timeStamp", 2),
    ("netStates0.targetHeight", 1),
    ("state.speed", 1),
    ("state.wheelAngularSpeed", 4),
    ("state.tyreContactPoint", 12),
    ("suspMatrix", 64),
    ("tyreLocalRotation", 64),
    ("state.accG", 3),
    ("state.drivetrainSpeed", 1),
    ("state.gas", 1),
    ("state.brake", 1),
    ("state.steer", 1),
    ("state.engineRPM", 1),
    ("state.gear.statusBits.ping", 3),
];

/// How many words [`NetReplay::remote_row`] has per remote car.
pub fn remote_words() -> usize {
    REMOTE_FIELDS.iter().map(|field| field.1).sum()
}

/// The names of a row's words.
pub fn remote_columns(cars: &[u8]) -> Vec<String> {
    let mut out = Vec::new();
    for slot in cars {
        for (name, count) in REMOTE_FIELDS {
            for k in 0..count {
                out.push(if count == 1 { format!("slot{slot}.{name}") } else { format!("slot{slot}.{name}[{k}]") });
            }
        }
    }
    out
}

/// An outgoing message as it is compared with the game's: `None` for the kinds that are
/// not compared, the time of a ping's answer blanked.
pub fn comparable(lane: Lane, data: &[u8]) -> Option<Vec<u8>> {
    match (lane, data.first()) {
        (Lane::DatagramOut, Some(0x4f)) => None,
        (Lane::DatagramOut, Some(0xf8)) => {
            let mut out = data.to_vec();
            for byte in out.iter_mut().skip(5).take(4) {
                *byte = 0;
            }
            Some(out)
        }
        _ => Some(data.to_vec()),
    }
}

/// The session of a tape on the local car of a recording.
pub struct NetReplay {
    pub world: ReplayWorld,
    pub session: NetSession<TapePlayer>,
    /// The server's car list as the tape has it.
    pub cars: Vec<CarEntry>,
    sent_seen: usize,
}

impl NetReplay {
    /// The recording's car in its engine; the tape's handshake and car list; a remote car
    /// for every other slot of the list, as `Sim::addNetCar` makes them.
    pub fn build(recording: &Recording, paths: &ReplayPaths, track: Arc<Track>, tape: &Tape) -> Result<NetReplay, String> {
        let mut world = ReplayWorld::build(recording, paths, Arc::clone(&track), 0)?;
        // (whatever is asked: the tape answers what it answered then)
        let request = JoinRequest { guid: "0".to_string(), name: "replay".to_string(), ..JoinRequest::default() };
        let (client, cars) = Client::join(TapePlayer::new(tape), &request).map_err(|e| e.to_string())?;
        let me = client.session_id;
        let mut session = NetSession::new(client);
        session.track = Some(Arc::clone(&track));
        let local_cars = world.cars.len();
        for (number, entry) in cars.iter().filter(|car| car.session_id != me && !car.is_spectator).enumerate() {
            let data_path = paths.cardata.join(&entry.model);
            let mesh = colliders::load(&data_path, paths.game_root.as_deref(), &entry.model)?.mesh;
            let pit_box = track.spawn_positions.get("PIT").and_then(|slots| slots.get(entry.session_id as usize).or(slots.first())).map_or(Mat44f::IDENTITY, |node| track.helper_nodes[*node].local);
            let geometry = NetCarGeometry::load(&data_path);
            let provider = NetCarStateProvider::new(&mut world.engine.core, entry.session_id, (local_cars + number) as u32, &entry.model, geometry, mesh.as_ref(), pit_box);
            session.cars.push(provider);
        }
        session.client.set_cars_in_session(1 + session.cars.len());
        let started = world.clock_start_ms - session.client.accepted.current_session.start as f64;
        session.client.begin_update_mode(started)?;
        Ok(NetReplay { world, session, cars, sent_seen: 0 })
    }

    /// What the client sent before the first step (the handshake, the car list requests,
    /// the push-to-pass question).
    pub fn take_sent(&mut self) -> Vec<(Lane, Vec<u8>)> {
        let sent = &self.session.client.transport.sent.entries;
        let new: Vec<(Lane, Vec<u8>)> = sent[self.sent_seen..].iter().map(|entry| (entry.lane, entry.data.clone())).collect();
        self.sent_seen = sent.len();
        new
    }

    /// One step with these controls of the local car; returns what the client sent in it.
    pub fn step(&mut self, inputs: &[&[u32]]) -> Vec<(Lane, Vec<u8>)> {
        self.world.step_with(inputs, &mut self.session);
        // PhysicsEngine::slipStreams: every remote car's wake, whether or not it is there
        self.world.engine.extra_wakes.clear();
        self.world.engine.extra_wakes.extend(self.session.cars.iter().map(|car| car.slip_stream));
        self.take_sent()
    }

    /// The compared words of every remote car after the last step ([`REMOTE_FIELDS`]).
    pub fn remote_row(&self) -> Vec<u32> {
        let core = &self.world.engine.core;
        let mut row = Vec::with_capacity(self.session.cars.len() * remote_words());
        for car in &self.session.cars {
            let body = core.world.body(car.body.id);
            row.extend(body.pos[..3].iter().map(|x| x.to_bits()));
            row.extend(body.q.iter().map(|x| x.to_bits()));
            row.extend(body.lvel[..3].iter().map(|x| x.to_bits()));
            row.extend(body.avel[..3].iter().map(|x| x.to_bits()));
            row.push(core.get_mesh_collide_category(car.body, 0).unwrap_or(0));
            let matrix = |m: &Mat44f, row: &mut Vec<u32>| row.extend(m.m.iter().flatten().map(|x| x.to_bits()));
            matrix(&car.body_matrix, &mut row);
            row.extend(car.error_vector.map(f32::to_bits));
            row.extend(car.instant_velocity.map(f32::to_bits));
            row.extend(car.smooth_velocity.map(f32::to_bits));
            row.push(car.has_collision_in_this_step.to_bits());
            row.push(car.is_disconnected as u32);
            let stamp = car.net_states[0].time_stamp.to_bits();
            row.extend([stamp as u32, (stamp >> 32) as u32]);
            row.push(car.net_states[0].target_height.to_bits());
            let state = &car.state;
            row.push(state.speed.to_bits());
            row.extend(state.wheel_angular_speed.map(f32::to_bits));
            row.extend(state.tyre_contact_point.iter().flatten().map(|x| x.to_bits()));
            for m in &car.susp_matrix {
                matrix(m, &mut row);
            }
            for m in &car.tyre_local_rotation {
                matrix(m, &mut row);
            }
            row.extend(state.acc_g.map(f32::to_bits));
            row.extend([state.drivetrain_speed, state.gas, state.brake, state.steer, state.engine_rpm].map(f32::to_bits));
            row.extend([state.gear as u32, state.status_bytes, car.ping as u32]);
        }
        row
    }

    /// The slots of the remote cars, in their order.
    pub fn remote_slots(&self) -> Vec<u8> {
        self.session.cars.iter().map(|car| car.session_id).collect()
    }
}

fn fnv(hash: &mut u64, bytes: &[u8]) {
    for byte in bytes {
        *hash ^= *byte as u64;
        *hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
}

/// The hash of one step for a golden file: the local car's row, the remote cars' row and
/// the compared outgoing messages. Two NaNs of different bits would differ here: a golden
/// excerpt is cut from a stretch without any.
pub fn step_hash(local_row: &[u32], remote_row: &[u32], sent: &[(Lane, Vec<u8>)]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for word in local_row.iter().chain(remote_row) {
        fnv(&mut hash, &word.to_le_bytes());
    }
    for (lane, data) in sent {
        if let Some(data) = comparable(*lane, data) {
            fnv(&mut hash, &[*lane as u8]);
            fnv(&mut hash, &(data.len() as u32).to_le_bytes());
            fnv(&mut hash, &data);
        }
    }
    hash
}

const GOLDEN_MAGIC: &[u8; 8] = b"RYNGOLD1";

/// A golden excerpt of an oracle run: the local car's recording header and inputs
/// ([`rustyac_physics::engine::record::Golden`]), the tape, and per step the hash of what
/// the game's own code made of it ([`step_hash`]).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NetGolden {
    pub local: Vec<u8>,
    pub tape: Tape,
    pub hashes: Vec<u64>,
}

impl NetGolden {
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = GOLDEN_MAGIC.to_vec();
        let tape = self.tape.to_bytes();
        for block in [&self.local, &tape] {
            out.extend_from_slice(&(block.len() as u32).to_le_bytes());
            out.extend_from_slice(block);
        }
        out.extend_from_slice(&(self.hashes.len() as u32).to_le_bytes());
        for hash in &self.hashes {
            out.extend_from_slice(&hash.to_le_bytes());
        }
        out
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<NetGolden, String> {
        if bytes.len() < 8 || &bytes[..8] != GOLDEN_MAGIC {
            return Err("not a rustyAC network golden file".to_string());
        }
        let mut pos = 8;
        let block = |pos: &mut usize| -> Result<Vec<u8>, String> {
            let length = u32::from_le_bytes(bytes.get(*pos..*pos + 4).ok_or("the golden file is cut short")?.try_into().unwrap()) as usize;
            *pos += 4;
            let data = bytes.get(*pos..*pos + length).ok_or("the golden file is cut short")?.to_vec();
            *pos += length;
            Ok(data)
        };
        let local = block(&mut pos)?;
        let tape = Tape::from_bytes(&block(&mut pos)?)?;
        let count = u32::from_le_bytes(bytes.get(pos..pos + 4).ok_or("the golden file is cut short")?.try_into().unwrap()) as usize;
        pos += 4;
        let mut hashes = Vec::with_capacity(count);
        for k in 0..count {
            hashes.push(u64::from_le_bytes(bytes.get(pos + 8 * k..pos + 8 * k + 8).ok_or("the golden file is cut short")?.try_into().unwrap()));
        }
        Ok(NetGolden { local, tape, hashes })
    }

    /// Replays the excerpt; `Ok(None)`: every step's hash is the game's, `Ok(Some(step))`:
    /// the first step that differs.
    pub fn replay(&self, paths: &ReplayPaths) -> Result<Option<usize>, String> {
        let golden = rustyac_physics::engine::record::Golden::from_bytes(&self.local)?;
        let recording = golden.recording()?;
        let track = rustyac_physics::engine::record::load_track(&recording, paths)?;
        let mut replay = NetReplay::build(&recording, paths, track, &self.tape)?;
        // (the join's own messages are not part of a step)
        replay.take_sent();
        let width = rustyac_physics::engine::record::INPUT_COLUMNS.len();
        for (step, (inputs, expected)) in golden.inputs.iter().zip(&self.hashes).enumerate() {
            let inputs: Vec<&[u32]> = inputs.chunks(width).collect();
            let sent = replay.step(&inputs);
            let hash = step_hash(&replay.world.row(&inputs), &replay.remote_row(), &sent);
            if hash != *expected {
                return Ok(Some(step));
            }
        }
        Ok(None)
    }
}
