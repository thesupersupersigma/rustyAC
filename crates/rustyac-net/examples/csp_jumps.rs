// SPDX-License-Identifier: GPL-3.0-or-later

//! Task NH-5 part 2: AssettoServer's AI slot jumps, played again on rustyAC's remote cars.
//!
//! `csp_jumps <golden.netgold> [--tape <file.nettape>] [--csp] [--cardata <folder>] [--root <AC folder>]`
//!
//! The golden's local car (its recorded controls) on the golden's tape or on `--tape` (`--csp`
//! joins as CSP, which a tape of `AB 03` updates needs). Three runs: the game's add, CSP's
//! hook with AssettoServer's settings (no `RESET`), CSP's hook with `RESET`. A jump is a new
//! state 50 m or more from the car's state before. At each jump, after the step it arrived
//! in: where the car is drawn (it should be at the new state, nothing in between), where its
//! collider is (with the drawn car), how far its heading is from the state's. A car is stuck
//! when its states moved 20 m or more in five seconds and the drawn car less than 1 m.

use rustyac_net::client::Client;
use rustyac_net::csp_motion::CspMotion;
use rustyac_net::replay::{replay_request, NetGolden, NetReplay};
use rustyac_net::transport::{Tape, TapePlayer};
use rustyac_physics::engine::record::{load_track, Golden, ReplayPaths, INPUT_COLUMNS};
use rustyac_physics::vecmath::create_from_euler;
use std::path::PathBuf;

fn dist(a: [f32; 3], b: [f32; 3]) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

#[derive(Default)]
struct Count {
    jumps: u32,
    /// The state history snapped (the two older states are the new one).
    history_snapped: u32,
    /// Drawn more than 10 m from the new state after its step.
    between: u32,
    /// The collider more than 1 m from the drawn car after the step.
    collider_apart: u32,
    /// Heading off by more than 5 degrees after the step.
    heading_off: u32,
    worst_heading_deg: f32,
    /// The farthest a drawn car was from its new state, and how many of those drawn
    /// between were nearer the old place than the new one (across the map).
    worst_between_m: f32,
    nearer_old: u32,
    stuck: u32,
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let value = |name: &str| args.iter().position(|a| a == name).and_then(|at| args.get(at + 1)).cloned();
    let Some(path) = args.first().filter(|a| !a.starts_with("--")) else {
        eprintln!("csp_jumps <golden.netgold> [--tape <file.nettape>] [--csp] [--cardata <folder>] [--root <AC folder>]");
        std::process::exit(2);
    };
    let golden = NetGolden::from_bytes(&std::fs::read(path).expect("the golden")).expect("a network golden");
    let tape = match value("--tape") {
        Some(file) => Tape::from_bytes(&std::fs::read(&file).expect("the tape")).expect("a network tape"),
        None => golden.tape.clone(),
    };
    let csp = args.iter().any(|a| a == "--csp");
    let paths = ReplayPaths { cardata: PathBuf::from(value("--cardata").unwrap_or_else(|| "cardata".to_string())), game_root: value("--root").map(PathBuf::from).or_else(rustyac_content::install::ac_root) };
    let local = Golden::from_bytes(&golden.local).expect("the golden's local car");
    let recording = local.recording().expect("its recording");
    let width = INPUT_COLUMNS.len();
    for mode in ["the game's add", "CSP's hook, AssettoServer's settings", "CSP's hook with RESET"] {
        let track = load_track(&recording, &paths).expect("the track");
        let mut request = replay_request();
        if csp {
            request.features = Some(rustyac_net::csp::FEATURES.to_string());
        }
        let (client, cars) = Client::join(TapePlayer::new(&tape), &request).expect("the tape's join");
        let mut replay = NetReplay::build_on(&recording, &paths, track, client, cars).expect("the replay");
        if mode != "the game's add" {
            let mut motion = CspMotion::new(2651);
            motion.read_settings(&[]);
            motion.reset = mode.ends_with("RESET");
            replay.session.csp_motion = Some(motion);
        }
        let n = replay.session.cars.len();
        let mut count = Count::default();
        // per car: the newest state seen (sequence, time stamp, place); five-second windows
        let mut newest: Vec<Option<(u8, u64, [f32; 3])>> = vec![None; n];
        let mut window: Vec<([f32; 3], [f32; 3])> = vec![([0.0; 3], [0.0; 3]); n];
        let mut states = 0u64;
        let last = local.inputs.last().cloned().unwrap_or_default();
        let steps = local.inputs.len().max(tape.entries.last().map_or(0, |e| e.tick as usize + 1));
        for step in 0..steps {
            let row = local.inputs.get(step).unwrap_or(&last);
            let inputs: Vec<&[u32]> = row.chunks(width).collect();
            replay.step(&inputs);
            let core = &replay.world.engine.core;
            for (i, car) in replay.session.cars.iter().enumerate() {
                let s = &car.net_states[0];
                let drawn = [car.body_matrix.m[3][0], car.body_matrix.m[3][1], car.body_matrix.m[3][2]];
                let key = (s.pak_sequence_id, s.time_stamp.to_bits(), s.pos);
                if newest[i].is_some_and(|(q, t, _)| (q, t) == (key.0, key.1)) || car.is_disconnected {
                    continue;
                }
                states += 1;
                if let Some((_, _, before)) = newest[i] {
                    if dist(s.pos, before) >= 50.0 {
                        count.jumps += 1;
                        count.history_snapped += (car.net_states[1].pos == s.pos && car.net_states[2].pos == s.pos) as u32;
                        count.between += (dist(drawn, s.pos) > 10.0) as u32;
                        count.worst_between_m = count.worst_between_m.max(dist(drawn, s.pos));
                        count.nearer_old += (dist(drawn, before) < dist(drawn, s.pos)) as u32;
                        let collider = core.body_state(car.body.id).pos;
                        count.collider_apart += (dist(collider, drawn) > 1.0) as u32;
                        let want = create_from_euler(s.rotation, s.pos);
                        let f = |m: &[f32; 4]| m[0] * m[0] + m[1] * m[1] + m[2] * m[2];
                        let (a, b) = (&car.body_matrix.m[2], &want.m[2]);
                        let cos = (a[0] * b[0] + a[1] * b[1] + a[2] * b[2]) / (f(a) * f(b)).sqrt();
                        let deg = cos.clamp(-1.0, 1.0).acos().to_degrees();
                        count.heading_off += (deg > 5.0) as u32;
                        count.worst_heading_deg = count.worst_heading_deg.max(deg);
                    }
                }
                newest[i] = Some(key);
            }
            if step % 1667 == 1666 {
                for (i, car) in replay.session.cars.iter().enumerate() {
                    let (state, drawn) = (car.net_states[0].pos, [car.body_matrix.m[3][0], car.body_matrix.m[3][1], car.body_matrix.m[3][2]]);
                    let (state0, drawn0) = window[i];
                    if step > 1666 && !car.is_disconnected && dist(state, state0) >= 20.0 && dist(drawn, drawn0) < 1.0 {
                        count.stuck += 1;
                    }
                    window[i] = (state, drawn);
                }
            }
        }
        println!(
            "{mode}: {steps} steps, {n} remote cars, {states} new states; {} jumps of 50 m or more: history snapped {}, drawn over 10 m from it {} (worst {:.1} m, nearer the old place {}), collider apart {}, heading off over 5 deg {} (worst {:.1} deg); stuck {}",
            count.jumps, count.history_snapped, count.between, count.worst_between_m, count.nearer_old, count.collider_apart, count.heading_off, count.worst_heading_deg, count.stuck
        );
    }
}
