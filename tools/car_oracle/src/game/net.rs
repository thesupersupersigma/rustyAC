// SPDX-License-Identifier: GPL-3.0-or-later

//! Task 25: the game's own online client and remote cars in-process, fed from a tape.
//!
//! The local car is a [`multi::MultiWorld`] of one car (the game's `PhysicsEngine`, `Track`
//! and `Car`). Around it this module puts by hand what `RaceManager::initOnline` builds:
//!
//! * an `ACClient` (0x10818 bytes) with its two sockets, its car, and the listeners
//!   `beginUpdateMode` installs. Its constructor is not run (it wants the whole `Sim`): the
//!   members `ACClient::onPhysicsStep` @ 0x14004f1d0 and what it calls read are set as the
//!   constructor and the handshake set them;
//! * one `NetCarStateProvider` (0x15b8 bytes) per other slot of the tape's car list: the
//!   constructor's values, the game's own `initPhysicsValues`, the body
//!   (`createRigidBody`, `setMassBox`) and the collider mesh through the game's own
//!   `RigidBodyODE` functions, `SlipStream::init`;
//! * the handler `ACClient` hangs on the local car's collision event.
//!
//! The game's winsock imports are stand-ins: `recvfrom` and `recv` hand out the tape's
//! incoming messages of the step, `sendto` and `send` keep what the game sends. Nothing
//! touches the network.
//!
//! `Car::postStep` is detoured: `ACClient::onPhysicsStep` runs right before it (the client's
//! handler was the first on the engine's step-completed event) and every remote car's
//! `NetCarStateProvider::step` @ 0x14011bd00 right after it (they were the last).

use std::collections::VecDeque;
use std::sync::Mutex;

use rustyac_net::handshake::CarEntry;
use rustyac_net::net_car::NetCarStateProvider as PortCar;
use rustyac_net::replay::remote_words;
use rustyac_net::transport::{Framer, Lane, Tape};

use super::multi::MultiWorld;
use super::*;

const VA_ACCLIENT_ON_PHYSICS_STEP: usize = 0x1_4004_f1d0; // ACClient::onPhysicsStep(double)
const VA_ACCLIENT_ASK_P2P: usize = 0x1_4004_4470; // ACClient::askForP2Pvalue()
const VA_ONMESSAGE_DO_CALL: usize = 0x1_4003_f9e0; // the UDP listener's std::function body
const VA_ONMESSAGE_TCP_DO_CALL: usize = 0x1_4003_f920; // the TCP listener's
const VA_COLLISION_DO_CALL: usize = 0x1_4003_fb60; // the collision handler's
const VA_SIM_EXECUTE_ON_MAIN_THREAD: usize = 0x1_4019_9240;
const VA_NETCAR_STEP: usize = 0x1_4011_bd00; // NetCarStateProvider::step(double)
const VA_NETCAR_INIT_PHYSICS_VALUES: usize = 0x1_4011_9fe0;
const VA_CAR_PHYSICS_STATE_CTOR: usize = 0x1_400b_ee60;
const VA_SLIP_STREAM_INIT: usize = 0x1_402a_adc0; // SlipStream::init(PhysicsEngine*)
const VA_CAR_POST_STEP: usize = 0x1_4027_5430;

const CLIENT_SIZE: usize = 0x10818;
const NETCAR_SIZE: usize = 0x15b8;

/// What the stand-in sockets hold: the step's incoming messages and what the game sent.
#[derive(Default)]
struct Wire {
    datagrams: VecDeque<Vec<u8>>,
    stream: VecDeque<u8>,
    sent: Vec<(Lane, Vec<u8>)>,
}

static WIRE: Mutex<Option<Wire>> = Mutex::new(None);

fn wire<R>(f: impl FnOnce(&mut Wire) -> R) -> R {
    f(WIRE.lock().unwrap().get_or_insert_with(Wire::default))
}

extern "system" fn net_recvfrom(_socket: usize, buffer: *mut u8, length: i32, _flags: i32, from: *mut u8, _from_length: *mut i32) -> i32 {
    wire(|w| match w.datagrams.pop_front() {
        Some(datagram) => unsafe {
            let count = datagram.len().min(length as usize);
            std::ptr::copy_nonoverlapping(datagram.as_ptr(), buffer, count);
            if !from.is_null() {
                // a sockaddr_in: AF_INET, the server's port, 127.0.0.1
                std::ptr::copy_nonoverlapping([2u8, 0, 0x25, 0x80, 127, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0].as_ptr(), from, 16);
            }
            count as i32
        },
        None => -1,
    })
}

extern "system" fn net_recv(_socket: usize, buffer: *mut u8, length: i32, _flags: i32) -> i32 {
    wire(|w| {
        let count = w.stream.len().min(length.max(0) as usize);
        if count == 0 {
            return -1;
        }
        for k in 0..count {
            unsafe { *buffer.add(k) = w.stream.pop_front().unwrap() };
        }
        count as i32
    })
}

extern "system" fn net_sendto(_socket: usize, buffer: *const u8, length: i32, _flags: i32, _to: *const u8, _to_length: i32) -> i32 {
    let data = unsafe { std::slice::from_raw_parts(buffer, length.max(0) as usize) }.to_vec();
    wire(|w| w.sent.push((Lane::DatagramOut, data)));
    length
}

extern "system" fn net_send(_socket: usize, buffer: *const u8, length: i32, _flags: i32) -> i32 {
    // the frame: the length in front is the transport's, not the message's
    let data = unsafe { std::slice::from_raw_parts(buffer, length.max(0) as usize) };
    wire(|w| w.sent.push((Lane::ReliableOut, data[2.min(data.len())..].to_vec())));
    length
}

/// The stand-ins for the game's winsock imports (by ordinal, from either winsock DLL). Set
/// before the game is loaded.
pub fn socket_overrides() -> Vec<(&'static str, &'static str, usize)> {
    let mut list = Vec::new();
    for dll in ["wsock32.dll", "ws2_32.dll"] {
        list.push((dll, "#16", net_recv as *const () as usize));
        list.push((dll, "#17", net_recvfrom as *const () as usize));
        list.push((dll, "#19", net_send as *const () as usize));
        list.push((dll, "#20", net_sendto as *const () as usize));
    }
    list
}

/// The messages of the reliable channel the game's own handlers are given. The others'
/// handlers post work to the main thread's `Sim` or walk avatars, which are not there;
/// the port takes every message.
const GAME_TAKES: [u8; 3] = [0x4d, 0x47, 0x4a];

struct Hook {
    client: *mut u8,
    car: *mut u8,
    engine: *mut u8,
    netcars: Vec<*mut u8>,
    on_physics_step: usize,
    netcar_step: usize,
    post_step_original: usize,
}

unsafe impl Send for Hook {}

static HOOK: Mutex<Option<Hook>> = Mutex::new(None);

/// `Car::postStep`, with the client's handler before it and the remote cars' after it.
extern "C" fn post_step_hook(car: *mut u8, dt: f32) {
    let (client, own, engine, netcars, on_physics_step, netcar_step, original) = {
        let guard = HOOK.lock().unwrap();
        let hook = guard.as_ref().expect("the hook is set before the first step");
        (hook.client, hook.car, hook.engine, hook.netcars.clone(), hook.on_physics_step, hook.netcar_step, hook.post_step_original)
    };
    unsafe {
        let time: f64 = rd(engine, PE_PHYSICS_TIME);
        let post_step: extern "C" fn(*mut u8, f32) = std::mem::transmute(original);
        if car != own {
            return post_step(car, dt);
        }
        let on_step: extern "C" fn(*mut u8, f64) = std::mem::transmute(on_physics_step);
        on_step(client, time);
        post_step(car, dt);
        let step: extern "C" fn(*mut u8, f64) = std::mem::transmute(netcar_step);
        for netcar in netcars {
            step(netcar, time);
        }
    }
}

/// A `std::function` body (`_Func_impl`) whose call is the game's own function `do_call`
/// with `captured` as the lambda's one capture.
unsafe fn function_impl(acs: &Acs, do_call: usize, captured: usize) -> *mut u8 {
    // the vtable: _Copy, _Move, _Do_call, _Target_type, _Get, _Delete_this
    let table = leak(vec![0usize, 0, acs.va(do_call), 0, 0, 0]);
    let object = object(0x20);
    wr(object, 0, table);
    wr(object, 8, captured);
    object
}

/// The game's side of the session.
pub struct NetGame<'a> {
    pub world: MultiWorld<'a>,
    acs: &'a Acs,
    client: *mut u8,
    netcars: Vec<*mut u8>,
    /// Each remote car's ODE body and mesh geom.
    bodies: Vec<(*mut u8, *mut u8)>,
    tape: Tape,
    next_datagram: usize,
    next_reliable: usize,
    /// How many reliable messages of the tape the game's handlers were given.
    pub game_reliable: usize,
}

impl<'a> NetGame<'a> {
    /// `world`: one local car on the track. `remote`: the port's remote cars of the same
    /// tape (their car-list entries and what the port made of the car's files), in order.
    /// `kind`: the session's kind; `slot`, `send_interval`: the handshake's.
    pub fn build(acs: &'a Acs, world: MultiWorld<'a>, tape: &Tape, remote: &[(CarEntry, PortCar, Option<rustyac_physics::car::ColliderMesh>)], slot: u8, send_interval: f64, kind: u8) -> Result<NetGame<'a>, String> {
        unsafe {
            let engine = world.engine;
            let car = world.car_ptr(0);
            let core: *mut u8 = rd(engine, PE_CORE);
            // Sim::executeOnMainThread: nothing is there to run it (ret)
            acs.patch(acs.va(VA_SIM_EXECUTE_ON_MAIN_THREAD), &[0xc3]);

            // the Sim as far as it is read: its cars (the local avatar: raceEngineer -> car),
            // its physics avatar (the engine sits 0x58 inside it)
            let sim = object(0x400);
            let engineer = object(0x40);
            wr(engineer, 8, car);
            let local_avatar = object(0x12a8);
            wr(local_avatar, 0xf80, engineer);
            wr(local_avatar, 0x130, sim);
            let avatars = leak(vec![local_avatar]);
            wr(sim, 0x208, avatars);
            wr(sim, 0x210, avatars.add(1));
            wr(sim, 0x218, avatars.add(1));
            wr(sim, 0x1b8, engine.sub(0x58));

            let client = object(CLIENT_SIZE);
            wr(client, 0x120, [76.0f32, 25.5, 250.0]);
            wr(client, 0x3f0, sim);
            wr(client, 0x3f8, car);
            wr(client, 0x418, send_interval);
            wr(client, 0x430, [2u8, 0, 0x25, 0x80, 127, 0, 0, 1]);
            wr(client, 0x440, slot);
            wr(client, 0x10510, kind as i32);
            // the UDP socket and its listener (ACClient::onMessage)
            let sok = client.add(0x3a0);
            wr(sok, 0, 0x1234usize);
            let listener = object(0x20);
            wr(listener, 0x18, function_impl(acs, VA_ONMESSAGE_DO_CALL, client as usize));
            wr(sok, 0x10, listener);
            wr(sok, 0x18, listener.add(0x20));
            wr(sok, 0x20, listener.add(0x20));
            // the TCP socket: its receive buffer and its listener (ACClient::onMessageTCP)
            let tcp = client.add(0x478);
            wr(tcp, 0x58, object(0x10000));
            wr(tcp, 0x60, 0x1235usize);
            let listener = object(0x20);
            wr(listener, 0x18, function_impl(acs, VA_ONMESSAGE_TCP_DO_CALL, client as usize));
            wr(tcp, 0x28, listener);
            wr(tcp, 0x30, listener.add(0x20));
            wr(tcp, 0x38, listener.add(0x20));
            // the handler on the local car's collision event (the lambda holds a reference to
            // the client pointer)
            let cell = leak(vec![client as usize]);
            let handler = object(0x28);
            wr(handler, 0, client);
            wr(handler, 0x20, function_impl(acs, VA_COLLISION_DO_CALL, cell as usize));
            wr(car, 0xa0, handler);
            wr(car, 0xa8, handler.add(0x28));
            wr(car, 0xb0, handler.add(0x28));

            let ray_provider: *mut u8 = rd(car, CAR_TYRES + 0x5a8);
            let state_ctor: extern "C" fn(*mut u8) -> *mut u8 = std::mem::transmute(acs.va(VA_CAR_PHYSICS_STATE_CTOR));
            let init_values: extern "C" fn(*mut u8) = std::mem::transmute(acs.va(VA_NETCAR_INIT_PHYSICS_VALUES));
            let slip_init: extern "C" fn(*mut u8, *mut u8) = std::mem::transmute(acs.va(VA_SLIP_STREAM_INIT));
            let mut netcars = Vec::new();
            let mut bodies = Vec::new();
            let identity: [f32; 16] = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0];
            for (entry, port, mesh) in remote {
                let p = object(NETCAR_SIZE);
                state_ctor(p.add(0x6e8));
                wr(p, 0x78, entry.session_id);
                wr(p, 0x10c, port.guid as i32);
                wr(p, 0x1a8, client);
                wr(p, 0x6b8, ray_provider);
                std::ptr::copy_nonoverlapping(wstring(acs, &entry.model), p.add(0x698), 32);
                wr(p, 0x1b0, identity);
                for wheel in 0..4 {
                    wr(p, 0x1f0 + 0x40 * wheel, identity);
                    wr(p, 0x2f0 + 0x40 * wheel, identity);
                    wr(p, 0x1458 + 0x40 * wheel, identity);
                }
                wr(p, 0x13b8, 1u8);
                wr(p, 0x1580, -1i32);
                wr(p, 0x6e8, port.guid as u8);
                let remote_avatar = object(0x12a8);
                wr(remote_avatar, 0x130, sim);
                wr(p, 0x1558, remote_avatar);
                init_values(p);
                // what the port read of the same files must be what the game read
                let wheels: [[f32; 16]; 4] = rd(p, 0x594);
                let radius: [f32; 4] = rd(p, 0x1438);
                for wheel in 0..4 {
                    let ours: Vec<u32> = port.geometry.wheel_base_pos_ls[wheel].m.iter().flatten().map(|x| x.to_bits()).collect();
                    let theirs: Vec<u32> = wheels[wheel].iter().map(|x| x.to_bits()).collect();
                    if ours != theirs || radius[wheel].to_bits() != port.geometry.tyre_radius[wheel].to_bits() {
                        return Err(format!("{}: the game's initPhysicsValues gives wheel {wheel} {:?} radius {}, the port {:?} radius {}", entry.model, &wheels[wheel][12..15], radius[wheel], &port.geometry.wheel_base_pos_ls[wheel].m[3][..3], port.geometry.tyre_radius[wheel]));
                    }
                }
                // the pit box as the constructor lifts it (only shown, never the body's place)
                let pit: Vec<f32> = port.pit_position.m.iter().flatten().copied().collect();
                wr(p, 0x1290, <[f32; 16]>::try_from(pit).unwrap());

                // the body: createRigidBody, setMassBox(1000, 1.4, 1.0, 4.0), the collider mesh
                let core_vtable: *const usize = rd(core, 0);
                let create_body: extern "C" fn(*mut u8) -> *mut u8 = std::mem::transmute(*core_vtable.add(3));
                let body = create_body(core);
                wr(p, 0x1560, body);
                let body_vtable: *const usize = rd(body, 0);
                let set_mass_box: extern "C" fn(*mut u8, f32, f32, f32, f32) = std::mem::transmute(*body_vtable.add(0x38 / 8));
                set_mass_box(body, 1000.0, f32::from_bits(0x3fb3_3333), 1.0, 4.0);
                let mut mesh_geom: *mut u8 = std::ptr::null_mut();
                if let Some(mesh) = mesh {
                    let vertices: Vec<f32> = mesh.vertices.iter().flatten().copied().collect();
                    let matrix: Vec<f32> = mesh.matrix.m.iter().flatten().copied().collect();
                    let add_mesh: extern "C" fn(*mut u8, *const f32, i32, *const u16, i32, *const f32, u32, u32, u32) = std::mem::transmute(*body_vtable.add(0x120 / 8));
                    let mask = if entry.model == "spectator" { 0 } else { 4 };
                    add_mesh(body, leak(vertices), mesh.vertices.len() as i32, leak(mesh.indices.clone()), mesh.indices.len() as i32, leak(matrix), 8, mask, port.guid + 1);
                    let meshes: *const *const u8 = rd(body, 0x30);
                    mesh_geom = rd(*meshes, 0x10);
                }
                // SlipStream: the constructor's values, then init (which also lists it in the engine)
                let slip = p.add(0x12d0);
                wr(slip, 0x40, [1.0f32, 1.0]);
                wr(slip, 0x64, 0.25f32);
                slip_init(slip, engine);
                // a new remote car is disconnected: a ghost
                if !mesh_geom.is_null() {
                    let set_category: extern "C" fn(*mut u8, u32, u32) = std::mem::transmute(*body_vtable.add(0x128 / 8));
                    set_category(body, 0, 0);
                }
                bodies.push((rd::<*mut u8>(body, RB_ODE_BODY), mesh_geom));
                netcars.push(p);
            }
            let list = leak(netcars.clone());
            wr(client, 0x448, list);
            wr(client, 0x450, list.add(netcars.len()));
            wr(client, 0x458, list.add(netcars.len()));

            let expected = [0x48, 0x89, 0x5c, 0x24, 0x18, 0x56, 0x48, 0x81, 0xec, 0xd0, 0x00, 0x00, 0x00];
            let original = acs.detour(VA_CAR_POST_STEP, 13, &expected, post_step_hook as *const () as usize);
            *HOOK.lock().unwrap() =
                Some(Hook { client, car, engine, netcars: netcars.clone(), on_physics_step: acs.va(VA_ACCLIENT_ON_PHYSICS_STEP), netcar_step: acs.va(VA_NETCAR_STEP), post_step_original: original });
            wire(|w| *w = Wire::default());
            // the tape's reliable messages before the first step were the join's: skipped
            let first_reliable = tape.entries.iter().position(|e| e.lane == Lane::ReliableIn && !matches!(e.data.first(), Some(0x3e | 0x40))).unwrap_or(tape.entries.len());
            Ok(NetGame { world, acs, client, netcars, bodies, tape: tape.clone(), next_datagram: 0, next_reliable: first_reliable, game_reliable: 0 })
        }
    }

    /// `ACClient::askForP2Pvalue` (the end of `RaceManager::initOnline`); returns what it sent.
    pub fn ask_p2p(&mut self) -> Vec<(Lane, Vec<u8>)> {
        unsafe {
            let ask: extern "C" fn(*mut u8) = std::mem::transmute(self.acs.va(VA_ACCLIENT_ASK_P2P));
            ask(self.client);
        }
        wire(|w| std::mem::take(&mut w.sent))
    }

    /// One step: the tape's messages of this step go onto the stand-in sockets, the game's
    /// engine steps. Returns what the game sent.
    pub fn step(&mut self) -> Vec<(Lane, Vec<u8>)> {
        let tick = self.world.step_index as u64;
        wire(|w| {
            while self.next_datagram < self.tape.entries.len() {
                let entry = &self.tape.entries[self.next_datagram];
                if entry.lane == Lane::DatagramIn && entry.tick > tick {
                    break;
                }
                if entry.lane == Lane::DatagramIn {
                    w.datagrams.push_back(entry.data.clone());
                }
                self.next_datagram += 1;
            }
            while self.next_reliable < self.tape.entries.len() {
                let entry = &self.tape.entries[self.next_reliable];
                if entry.lane == Lane::ReliableIn && entry.tick > tick {
                    break;
                }
                if entry.lane == Lane::ReliableIn && entry.data.first().is_some_and(|id| GAME_TAKES.contains(id)) {
                    w.stream.extend(Framer::frame(&entry.data).expect("a taped message fits a frame"));
                    self.game_reliable += 1;
                }
                self.next_reliable += 1;
            }
        });
        self.world.step();
        wire(|w| {
            assert!(w.datagrams.is_empty(), "the game left datagrams of step {tick} unread");
            std::mem::take(&mut w.sent)
        })
    }

    /// The compared words of every remote car (`rustyac_net::replay::REMOTE_FIELDS`).
    pub fn remote_row(&self) -> Vec<u32> {
        let mut row = Vec::with_capacity(self.netcars.len() * remote_words());
        unsafe {
            for (&p, &(body, geom)) in self.netcars.iter().zip(&self.bodies) {
                row.extend(rd::<[u32; 3]>(body, B_POS));
                row.extend(rd::<[u32; 4]>(body, B_Q));
                row.extend(rd::<[u32; 3]>(body, B_LVEL));
                row.extend(rd::<[u32; 3]>(body, B_AVEL));
                row.push(if geom.is_null() { 0 } else { rd(geom, 0x78) });
                row.extend(rd::<[u32; 16]>(p, 0x1b0));
                row.extend(rd::<[u32; 3]>(p, 0x100));
                row.extend(rd::<[u32; 3]>(p, 0x6cc));
                row.extend(rd::<[u32; 3]>(p, 0x13e0));
                row.push(rd(p, 0x198));
                row.push(rd::<u8>(p, 0x13b8) as u32);
                row.extend(rd::<[u32; 2]>(p, 0x420));
                row.push(rd(p, 0x434));
                row.push(rd(p, 0x958));
                row.extend(rd::<[u32; 4]>(p, 0x934));
                row.extend(rd::<[u32; 12]>(p, 0xd64));
                row.extend(rd::<[u32; 64]>(p, 0x2f0));
                row.extend(rd::<[u32; 64]>(p, 0x1458));
                row.extend(rd::<[u32; 3]>(p, 0xd34));
                row.extend([rd::<u32>(p, 0xe1c), rd(p, 0x948), rd(p, 0x94c), rd(p, 0x944), rd(p, 0x92c)]);
                row.extend([rd::<u32>(p, 0x954), rd(p, 0x1240), rd(p, 0x110)]);
            }
        }
        row
    }
}
