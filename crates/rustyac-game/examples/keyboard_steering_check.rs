// SPDX-License-Identifier: GPL-3.0-or-later

//! Task 37: plays a tape of the game's own keyboard steering (`car_oracle steer-micro`:
//! `KeyboardCarControl::stepSteer` and `getKeyboardSteering` on a game body put somewhere new
//! every call, with a track's AI line or without) through the port's `KeyboardCarControl`,
//! and says whether every value is the same, bit for bit.
//!
//! `cargo run --release -p rustyac-game --example keyboard_steering_check -- <tape>
//!  [--line <fast_lane.ai>] [--workers <n>] [--sensitivity-key] [--fault]`
//! - `--line`: the line the oracle gave the game (the port reads it itself; its points must
//!   be the game's)
//! - `--workers n`: the tape played n times at once on n threads (0: on this thread)
//! - `--sensitivity-key`: the keyboard read from a controls.ini with
//!   `RUSTYAC_SPEED_SENSITIVITY=1` instead of the constructor's values
//! - `--fault`: `STEER_GAIN` one bit off, which must be caught

use std::sync::Arc;

use rustyac_game::input::ini::ControlsIni;
use rustyac_game::input::keyboard::KeyboardCarControl;
use rustyac_game::input::CarProbe;
use rustyac_physics::car::CarControlsInput;
use rustyac_physics::track::spline::{AiSpline, InterpolatingSpline};

fn hex(s: &str) -> f32 {
    f32::from_bits(u32::from_str_radix(s, 16).expect("hex"))
}

struct Options {
    sensitivity_key: bool,
    fault: bool,
}

/// The tape through the port: (calls, the first call that differs).
fn play(tape: &str, line: Option<&Arc<InterpolatingSpline>>, options: &Options) -> (usize, Option<usize>) {
    let mut k = KeyboardCarControl::defaults();
    let mut calls = 0;
    for text in tape.lines() {
        let (kind, rest) = text.split_once(' ').unwrap_or((text, ""));
        match kind {
            "keyboard" => {
                let s: Vec<f32> = rest.split(' ').map(hex).collect();
                let base = if options.sensitivity_key {
                    KeyboardCarControl::from_ini(&ControlsIni::parse("[KEYBOARD]\r\nRUSTYAC_SPEED_SENSITIVITY=1\r\n"))
                } else {
                    KeyboardCarControl::defaults()
                };
                k = KeyboardCarControl {
                    steer_speed: s[0],
                    steer_opposite_direction_factor: s[1],
                    steer_reset_factor: s[2],
                    steer_gain: if options.fault { f32::from_bits(s[3].to_bits() ^ 1) } else { s[3] },
                    look_ahead: s[4],
                    line: line.cloned(),
                    ..base
                };
            }
            "step" => {
                calls += 1;
                let (input, output) = rest.split_once(" -> ").expect("a step");
                let v: Vec<&str> = input.split(' ').collect();
                let f: Vec<f32> = v[..20].iter().map(|s| hex(s)).collect();
                let keys: i32 = v[20].parse().expect("keys");
                let probe = CarProbe { position: [f[12], f[13], f[14]], left: [f[0], f[1], f[2]], ..CarProbe::default() };
                let (speed, dt) = (f[16], f[17]);
                k.old_steer = f[19];
                let before = k.int_steer.to_bits() == f[18].to_bits();
                let wish = k.step_steer(&probe);
                let r = k.get_keyboard_steering(dt, &CarControlsInput { steer_lock: 0.0, speed }, &probe, keys & 1 != 0, keys & 2 != 0);
                let port = format!(
                    "{:08x} {:08x} {:08x} {:08x} {:08x} {:08x} {:08x} {}",
                    wish.to_bits(),
                    r.to_bits(),
                    k.int_steer.to_bits(),
                    k.min_steering.to_bits(),
                    k.steer_decelerator.to_bits(),
                    k.speed_decelering_factor.to_bits(),
                    k.turn_decelering_factor.to_bits(),
                    k.current_spline_pos
                );
                if !before || port != output {
                    eprintln!("call {calls}: game {output}\n         port {port}");
                    return (calls, Some(calls));
                }
            }
            _ => {}
        }
    }
    (calls, None)
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let path = args.get(1).expect("usage: keyboard_steering_check <tape> [--line <fast_lane.ai>] [--workers n] [--sensitivity-key] [--fault]");
    let value = |name: &str| args.iter().position(|a| a == name).map(|i| args[i + 1].clone());
    let options = Options { sensitivity_key: args.iter().any(|a| a == "--sensitivity-key"), fault: args.iter().any(|a| a == "--fault") };
    let workers: usize = value("--workers").map(|n| n.parse().expect("workers")).unwrap_or(0);
    let tape = std::fs::read_to_string(path).expect("the tape");
    let count: usize = tape.lines().next().and_then(|l| l.strip_prefix("line ")).expect("the line's size").parse().expect("a count");
    // the game's line, point by point, against the port's own reading of the file
    let line = if count > 0 {
        let file = value("--line").expect("the tape has a line: --line <fast_lane.ai>");
        let ai = AiSpline::load(std::path::Path::new(&file)).expect("the line");
        let game: Vec<[u32; 3]> = tape.lines().filter_map(|l| l.strip_prefix("point ")).map(|p| {
            let v: Vec<u32> = p.split(' ').map(|s| u32::from_str_radix(s, 16).expect("hex")).collect();
            [v[0], v[1], v[2]]
        }).collect();
        let port: Vec<[u32; 3]> = ai.spline.points.iter().map(|p| p.point.map(f32::to_bits)).collect();
        if game != port {
            println!("keyboard steering: DIFFERENT line ({} points in the game, {} in the port)", game.len(), port.len());
            std::process::exit(1);
        }
        Some(Arc::new(ai.spline))
    } else {
        None
    };
    let results: Vec<(usize, Option<usize>)> = if workers == 0 {
        vec![play(&tape, line.as_ref(), &options)]
    } else {
        std::thread::scope(|s| {
            let runs: Vec<_> = (0..workers).map(|_| s.spawn(|| play(&tape, line.as_ref(), &options))).collect();
            runs.into_iter().map(|r| r.join().expect("a worker")).collect()
        })
    };
    let (calls, wrong) = results[0];
    let points = if count > 0 { format!("on a line of {count} points") } else { "without a line".to_string() };
    if results.iter().any(|r| *r != results[0]) {
        println!("keyboard steering: the {} runs differ from each other", results.len());
        std::process::exit(1);
    }
    match wrong {
        None => println!("keyboard steering: IDENTICAL ({calls} calls of the game {points}, {} run(s), workers {workers})", results.len()),
        Some(call) => {
            println!("keyboard steering: DIFFERENT at call {call} of {calls} {points}");
            std::process::exit(1);
        }
    }
}
