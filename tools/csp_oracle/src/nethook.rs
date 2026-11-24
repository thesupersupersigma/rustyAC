// SPDX-License-Identifier: GPL-3.0-or-later

//! Task NH-5 part 1: CSP's hook on the remote-state add (`dwrite.dll` @ 0x18061ec60, with the
//! game's own `NetCarStateProvider::onRemoteStateReceived` @ 0x14011b1e0 behind it) against
//! `rustyac_net::csp_motion` on the same arrivals, every add compared as bits.
//!
//! `--net-hook <file>`: a rustyAC network tape (`.nettape`: `0x48` / `0x46` / `AB 03` datagrams
//! in, the clock 3 ms a tick) or a relay capture (`udp.log` of `re/scratch/taskNH7`: the
//! server's datagrams, the clock its seconds), then made-up edge states. Four runs: the
//! settings `RESET` and raw interval off and on. Each run also counts the slot jumps (a
//! state 50 m or more from the car's last one) and how many of them snapped.

use crate::acs::Acs;
use crate::game::{rd, wr};
use rustyac_net::csp_motion::{CspMotion, TELEPORT_BIT};
use rustyac_net::net_car::{NetCarGeometry, NetCarState, NetCarStateProvider};
use rustyac_physics::car::PhysicsCore;
use rustyac_physics::vecmath::Mat44f;

const DLL_BASE: usize = 0x1_8000_0000;
const HOOK: usize = 0x1_8061_ec60;
const FILTER: usize = 0x1_8061_e750;
/// The original add CSP calls (`qword_182814C60`), the physics engine it reads the clock of
/// (`qword_182814E40`, `+0x20` `physicsTime`), the settings and the measured intervals.
const ORIGINAL: usize = 0x1_8281_4c60;
const ENGINE: usize = 0x1_8281_4e40;
const RESET: usize = 0x1_827f_9388;
const RACE_INI_RAN: usize = 0x1_827f_9495;
const RAW: usize = 0x1_8280_3261;
const MEASURED: usize = 0x1_8281_3040;
const VA_ADD: usize = 0x1_4011_b1e0;
const NETCAR_SIZE: usize = 0x15b8;
const SEND_INTERVAL: f64 = 54.0;

/// An arrival: the clock, the slot, the state as decoded.
type Arrival = (f64, u8, NetCarState);

fn arrivals(file: &str) -> Vec<Arrival> {
    let bytes = std::fs::read(file).unwrap_or_else(|e| panic!("{file}: {e}"));
    // the capture's server is AssettoServer pre49: its format text
    let custom = std::cell::RefCell::new(rustyac_net::custom_update::Reader::new(rustyac_net::custom_update::Format::parse(rustyac_net::custom_update::ASSETTO_SERVER).expect("AssettoServer's format")));
    let decode = |d: &[u8]| if d.starts_with(&rustyac_net::datagram::CUSTOM_UPDATE) { custom.borrow_mut().decode(d) } else { rustyac_net::datagram::decode(d) };
    let states = |t: f64, d: &[u8]| decode(d).into_iter().filter_map(move |i| if let rustyac_net::datagram::Incoming::CarState { slot, state } = i { Some((t, slot, state)) } else { None });
    if file.ends_with(".nettape") {
        let tape = rustyac_net::transport::Tape::from_bytes(&bytes).expect("a network tape");
        tape.of(rustyac_net::transport::Lane::DatagramIn).flat_map(|e| states(e.tick as f64 * 3.0, &e.data).collect::<Vec<_>>()).collect()
    } else {
        let text = String::from_utf8_lossy(&bytes);
        let mut t0 = None;
        let mut out = Vec::new();
        for line in text.lines() {
            let p: Vec<&str> = line.split_whitespace().collect();
            if p.len() != 4 || p[2] != "S>C" {
                continue;
            }
            let (Ok(t), Some(d)) = (p[0].parse::<f64>(), (0..p[3].len()).step_by(2).map(|i| u8::from_str_radix(&p[3][i..i + 2], 16).ok()).collect::<Option<Vec<u8>>>()) else { continue };
            let t = (t - *t0.get_or_insert(t)) * 1000.0;
            out.extend(states(t, &d));
        }
        out
    }
}

/// Made-up states after the input's last one of `slot`: every drop case of the test, the
/// teleport bit, a wrapped and a large sequence step, a late time stamp, a reconnection
/// (`None`: mark the car disconnected first).
fn edges(last: &Arrival) -> Vec<Option<Arrival>> {
    let (mut t, slot, base) = *last;
    let mut state = base;
    let mut out = Vec::new();
    let mut push = |s: NetCarState, out: &mut Vec<Option<Arrival>>| {
        t += SEND_INTERVAL;
        out.push(Some((t, slot, s)));
    };
    for k in 0..9 {
        for bad in [f32::INFINITY, f32::NEG_INFINITY, f32::NAN, f32::MAX, -f32::MAX] {
            let mut s = state;
            s.pak_sequence_id = s.pak_sequence_id.wrapping_add(1);
            s.time_stamp += SEND_INTERVAL;
            match k {
                0..=2 => s.pos[k] = bad,
                3..=5 => s.rotation[k - 3] = bad,
                _ => s.velocity[k - 6] = bad,
            }
            push(s, &mut out);
            // a NaN heading, -inf or -MAX is taken: carry on from a sane one
            state.pak_sequence_id = s.pak_sequence_id;
            state.time_stamp = s.time_stamp;
            push(state, &mut out);
        }
    }
    let mut step = |state: &mut NetCarState, seq: u8, dt: f64, bits: u32, dx: f32, out: &mut Vec<Option<Arrival>>| {
        state.pak_sequence_id = state.pak_sequence_id.wrapping_add(seq);
        state.time_stamp += dt;
        state.status_bytes = base.status_bytes | bits;
        state.pos[0] += dx;
        push(*state, out);
    };
    step(&mut state, 1, SEND_INTERVAL, TELEPORT_BIT, 0.0, &mut out);
    step(&mut state, 1, SEND_INTERVAL, 0, 60.0, &mut out);
    step(&mut state, 20, SEND_INTERVAL * 20.0, 0, 0.0, &mut out);
    step(&mut state, 1, 2000.0, 0, 0.0, &mut out);
    step(&mut state, 250, SEND_INTERVAL, 0, 0.0, &mut out);
    out.push(None);
    step(&mut state, 1, 6000.0, 0, 0.0, &mut out);
    step(&mut state, 1, SEND_INTERVAL, 0, 0.0, &mut out);
    out
}

/// The game's `NetCarState` (0x88 bytes) of a port state; `acceleration` (0x24) is never set.
unsafe fn write_state(p: *mut u8, s: &NetCarState) {
    std::ptr::write_bytes(p, 0, 0x88);
    wr(p, 0, s.pos);
    wr(p, 0xc, s.rotation);
    wr(p, 0x18, s.velocity);
    wr(p, 0x30, s.time_stamp);
    wr(p, 0x38, s.rcv_time);
    wr(p, 0x40, s.pak_sequence_id);
    wr(p, 0x44, s.target_height);
    wr(p, 0x48, s.tyre_angular_speed);
    wr(p, 0x58, s.engine_rpm);
    wr(p, 0x5c, s.steer_angle);
    wr(p, 0x60, s.wheel_angle);
    wr(p, 0x64, s.gear_index);
    wr(p, 0x68, s.status_bytes);
    wr(p, 0x6c, s.aoa);
    wr(p, 0x70, s.aoa_axis);
    wr(p, 0x7c, s.ping);
    wr(p, 0x80, s.performance_delta);
    wr(p, 0x84, s.gas);
}

/// The compared words of a state: the game's 0x88 bytes (padding 0x41..0x44 left out).
fn state_words(s: &NetCarState) -> Vec<u32> {
    let mut b = [0u8; 0x88];
    unsafe { write_state(b.as_mut_ptr(), s) };
    words(&b)
}

fn words(b: &[u8]) -> Vec<u32> {
    b.chunks(4).enumerate().map(|(i, c)| if i == 0x40 / 4 { c[0] as u32 } else { u32::from_le_bytes([c[0], c[1], c[2], c[3]]) }).collect()
}

/// What an add leaves (not its return: the game's add is `void`, CSP's hook hands back
/// whatever is in `al`; a dropped state shows as states unchanged): the measured interval, the body, the error, the ping, the QoS,
/// disconnected, where the picture was, the three states.
fn port_row(measured: f32, c: &NetCarStateProvider) -> Vec<u32> {
    let mut row = vec![measured.to_bits()];
    row.extend(c.body_matrix.m.iter().flatten().map(|x| x.to_bits()));
    row.extend(c.error_vector.map(f32::to_bits));
    row.extend([c.ping as u32, c.qos_good_packets, c.qos_bad_packets, c.is_disconnected as u32]);
    row.extend(c.last_screen_pos.map(f32::to_bits));
    for s in &c.net_states {
        row.extend(state_words(s));
    }
    row
}

unsafe fn game_row(measured: f32, p: *const u8) -> Vec<u32> {
    let mut row = vec![measured.to_bits()];
    row.extend(rd::<[u32; 16]>(p, 0x1b0));
    row.extend(rd::<[u32; 3]>(p, 0x100));
    row.extend([rd::<u32>(p, 0x110), rd(p, 0x19c), rd(p, 0x1a0), rd::<u8>(p, 0x13b8) as u32]);
    row.extend(rd::<[u32; 3]>(p, 0x588));
    for k in 0..3 {
        row.extend(words(std::slice::from_raw_parts(p.add(0x3f0 + 0x88 * k), 0x88)));
    }
    row
}

fn leak(size: usize) -> *mut u8 {
    Box::leak(vec![0u8; size].into_boxed_slice()).as_mut_ptr()
}

pub unsafe fn run(acs: &Acs, dll: usize, file: &str) {
    let at = |va: usize| dll + va - DLL_BASE;
    let input = arrivals(file);
    let mut slots: Vec<u8> = input.iter().map(|a| a.1).collect();
    slots.sort();
    slots.dedup();
    println!("net-hook: {file}: {} states of slots {slots:?}", input.len());
    let mut stream: Vec<Option<Arrival>> = input.iter().copied().map(Some).collect();
    if let Some(last) = input.iter().rev().find(|a| a.1 == slots[0]) {
        stream.extend(edges(last));
    }
    // rustyAC's client drops a state with a value that is no finite number before any add
    // (Client::on_datagram): those only have the drop test compared, against CSP's on a
    // scratch car (not disconnected, RESET off: the test changes nothing then)
    let finite = |s: &NetCarState| s.pos.iter().chain(&s.rotation).chain(&s.velocity).all(|v| v.is_finite());
    let filter: extern "C" fn(*mut u8, *const u8) -> u8 = std::mem::transmute(at(FILTER));
    let (scratch, state) = (leak(NETCAR_SIZE), leak(0x88));
    wr(at(RESET) as *mut u8, 0, 0u8);
    let (mut tested, mut same) = (0, 0);
    for (_, _, s) in stream.iter().flatten().filter(|a| !finite(&a.2)) {
        write_state(state, s);
        tested += 1;
        same += ((filter(scratch, state) == 0) == rustyac_net::csp_motion::drops(s)) as u32;
    }
    println!("net-hook: drop test on {tested} states that are no finite numbers: {same} the same as CSP's ({})", if same == tested { "IDENTICAL" } else { "DIFFERENT" });
    stream.retain(|a| a.is_none_or(|a| finite(&a.2)));
    // the game's add behind CSP's: what CSP's own load put there, else the game's function
    let original: usize = rd(at(ORIGINAL) as *const u8, 0);
    let add = acs.va(VA_ADD);
    println!("net-hook: CSP's original add {original:#x} (the game's {add:#x}), first bytes of the game's {:02x?}", std::slice::from_raw_parts(add as *const u8, 6));
    if original == 0 {
        wr(at(ORIGINAL) as *mut u8, 0, add);
    }
    let hook: extern "C" fn(*mut u8, *const u8) -> u8 = std::mem::transmute(at(HOOK));
    // the client as the add reads it: sim (0x3f0) -> physics avatar (0x1b8), whose engine
    // (at 0x58) has the clock at 0x20; the send interval (0x418)
    let avatar = leak(0x200);
    let sim = leak(0x400);
    wr(sim, 0x1b8, avatar);
    let client = leak(0x20000);
    wr(client, 0x3f0, sim);
    wr(client, 0x418, SEND_INTERVAL);
    wr(at(ENGINE) as *mut u8, 0, avatar.add(0x58));
    wr(at(RACE_INI_RAN) as *mut u8, 0, 1u8);
    let mut all_same = true;
    for (reset, raw) in [(false, false), (true, false), (false, true), (true, true)] {
        wr(at(RESET) as *mut u8, 0, reset as u8);
        wr(at(RAW) as *mut u8, 0, raw as u8);
        std::ptr::write_bytes(at(MEASURED) as *mut u8, 0, 0x400);
        let mut motion = CspMotion::new(0);
        (motion.reset, motion.raw_interval) = (reset, raw);
        let mut core = PhysicsCore::new();
        let mut cars: Vec<(u8, NetCarStateProvider, *mut u8)> = Vec::new();
        for (guid, &slot) in slots.iter().enumerate() {
            let port = NetCarStateProvider::new(&mut core, slot, guid as u32 + 1, "car", NetCarGeometry::from_inis(&|_| None), None, Mat44f::IDENTITY);
            let p = leak(NETCAR_SIZE);
            wr(p, 0x10c, guid as i32 + 1);
            wr(p, 0x1a8, client);
            wr(p, 0x1b0, Mat44f::IDENTITY.m);
            wr(p, 0x13b8, 1u8);
            cars.push((slot, port, p));
        }
        let (mut adds, mut different, mut jumps, mut snapped) = (0u64, 0u64, 0u64, 0u64);
        let mut last_pos: Vec<Option<[f32; 3]>> = vec![None; cars.len()];
        let mut reconnect_next = false;
        for arrival in &stream {
            let Some((t, slot, s)) = *arrival else {
                reconnect_next = true;
                continue;
            };
            let i = cars.iter().position(|c| c.0 == slot).unwrap();
            let (_, port, p) = &mut cars[i];
            if std::mem::take(&mut reconnect_next) {
                port.mark_disconnected();
                wr(*p, 0x13b8, 1u8);
            }
            wr(avatar, 0x78, t);
            write_state(state, &s);
            hook(*p, state);
            let taken_port = motion.add(port, s, t, SEND_INTERVAL);
            let measured_game: f32 = rd(at(MEASURED) as *const u8, 4 * port.guid as usize);
            let (a, b) = (game_row(measured_game, *p), port_row( motion.measured[port.guid as usize], port));
            adds += 1;
            if a != b {
                different += 1;
                if different <= 5 {
                    let first = a.iter().zip(&b).position(|(x, y)| x != y).unwrap_or(0);
                    println!("net-hook: add {adds} (slot {slot}, t {t}): word {first} csp {:#010x} port {:#010x}", a.get(first).unwrap_or(&0), b.get(first).unwrap_or(&0));
                }
            }
            // a slot jump: 50 m from the last state; snapped: the history is this state, 30 ms apart
            if let Some(q) = last_pos[i] {
                let d = (0..3).map(|k| (s.pos[k] - q[k]) * (s.pos[k] - q[k])).sum::<f32>();
                if d >= 2500.0 && taken_port {
                    jumps += 1;
                    let h = &port.net_states;
                    snapped += (h[1].pos == s.pos && h[2].pos == s.pos) as u64;
                }
            }
            if taken_port {
                last_pos[i] = Some(s.pos);
            }
        }
        all_same &= different == 0;
        println!(
            "net-hook: RESET {} raw interval {}: {adds} adds, {} ({different} different); {jumps} jumps of 50 m or more, {snapped} snapped",
            reset as u8,
            raw as u8,
            if different == 0 { "IDENTICAL" } else { "DIFFERENT" }
        );
    }
    println!("net-hook: {}", if all_same && same == tested { "IDENTICAL" } else { "DIFFERENT" });
}
