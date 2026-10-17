// SPDX-License-Identifier: GPL-3.0-or-later

//! Task 37b: plays a tape of the game's own gamepad car control (`car_oracle pad-micro`:
//! `JoypadCarControl`'s constructor on made-up controls.ini files, `acquireControls` on
//! made-up XInput states and keys) through the port's `JoypadCarControl`, and says whether
//! every value is the same, bit for bit: what the constructor made of the settings (with
//! `[X360] SPEED_SENSITIVITY` at the controls page's default 0.5, at 0, at AC's preset 0.1, at
//! 1 and missing), and every call's steering, pedals, buttons and the class's own fields.
//!
//! `cargo run --release -p rustyac-game --example pad_steering_check -- <tape>
//!  [--workers <n>] [--fault]`
//! - `--workers n`: the tape played n times at once on n threads (0: on this thread)
//! - `--fault`: `SPEED_SENSITIVITY` one bit off after the reading, which must be caught

use rustyac_game::input::ini::ControlsIni;
use rustyac_game::input::pad::{JoypadCarControl, PadState};
use rustyac_game::input::Extra;
use rustyac_physics::car::feed::CarControls;
use rustyac_physics::car::CarControlsInput;

/// The port's buttons (in the order the constructor reads the sections) in the order the
/// game's class keeps them: glance back before glance right, and the up button of brake
/// balance, ABS, TC and turbo before its down button.
const GAME_ORDER: [usize; 27] = [0, 1, 2, 3, 5, 4, 6, 7, 8, 10, 9, 11, 12, 13, 15, 14, 17, 16, 19, 18, 20, 21, 22, 23, 24, 25, 26];

fn hex(s: &str) -> f32 {
    f32::from_bits(u32::from_str_radix(s, 16).expect("hex"))
}

/// `KEY` 0x41..0x58 held: bit (key - 0x41) of the step's keys, as the oracle's stand-in.
fn key_down(keys: u32, key: i32) -> bool {
    (0x41..=0x58).contains(&key) && keys & (1 << (key - 0x41)) != 0
}

/// The tape through the port: (calls, groups, the first line that differs).
fn play(tape: &str, fault: bool) -> (usize, usize, Option<usize>) {
    let mut pad: Option<JoypadCarControl> = None;
    let (mut calls, mut groups) = (0, 0);
    for (n, text) in tape.lines().enumerate() {
        let (kind, rest) = text.split_once(' ').unwrap_or((text, ""));
        match kind {
            "pad" => {
                groups += 1;
                let (input, game) = rest.split_once(" -> ").expect("a pad line");
                let (legacy, ini) = input.split_once(' ').expect("legacy and the ini");
                let mut p = JoypadCarControl::from_ini(&ControlsIni::parse(&ini.replace('|', "\r\n")), legacy == "1");
                let mut port = format!(
                    "{:08x} {:08x} {:08x} {:08x} {:08x} {:08x} {} {}",
                    p.speed_sensitivity.to_bits(),
                    p.steer_gamma.to_bits(),
                    p.steer_filter.to_bits(),
                    p.dead_zone.to_bits(),
                    p.steer_speed.to_bits(),
                    p.rumble_intensity.to_bits(),
                    p.use_legacy_code as u8,
                    p.is_steer_with_right as u8
                );
                for b in GAME_ORDER.iter().map(|i| &p.buttons[*i]) {
                    port.push_str(&format!(" {:x}/{}/{}", 1u32 << b.button, b.key, b.allow_keyboard_input as u8));
                }
                if port != game {
                    eprintln!("line {}: game {game}\n         port {port}", n + 1);
                    return (calls, groups, Some(n + 1));
                }
                if fault {
                    p.speed_sensitivity = f32::from_bits(p.speed_sensitivity.to_bits() ^ 1);
                }
                pad = Some(p);
            }
            "step" => {
                calls += 1;
                let p = pad.as_mut().expect("a pad line first");
                let (input, game) = rest.split_once(" -> ").expect("a step");
                let v: Vec<&str> = input.split(' ').collect();
                let int = |i: usize| v[i].parse::<i32>().expect("a number");
                let state = PadState {
                    buttons: u16::from_str_radix(v[0], 16).expect("buttons"),
                    left_trigger: int(1) as u8,
                    right_trigger: int(2) as u8,
                    thumb_lx: int(3) as i16,
                    thumb_ly: int(4) as i16,
                    thumb_rx: int(5) as i16,
                    thumb_ry: int(6) as i16,
                };
                let keys = u32::from_str_radix(v[7], 16).expect("keys");
                let input = CarControlsInput { steer_lock: hex(v[8]), speed: hex(v[9]) };
                // (v[10] is dt: the game does not use it, nor does the port)
                let mut controls = CarControls { steer: hex(v[11]), ..CarControls::default() };
                let mut extra = Extra::default();
                p.acquire_controls(&state, &mut controls, &mut extra, &input, &|k| key_down(keys, k));
                let flags: String = [
                    controls.gear_up,
                    controls.gear_dn,
                    controls.drs,
                    controls.kers,
                    extra.brake_balance_up,
                    extra.brake_balance_dn,
                    extra.abs_up,
                    extra.abs_dn,
                    extra.tc_up,
                    extra.tc_dn,
                    extra.engine_brake_up,
                    extra.engine_brake_dn,
                    extra.mguk_delivery_up,
                    extra.mguk_delivery_dn,
                    extra.mguk_recovery_up,
                    extra.mguk_recovery_dn,
                    extra.mguh_mode,
                ]
                .iter()
                .map(|b| format!("{:02x}", *b as u8))
                .collect();
                let port = format!(
                    "{:08x} {:08x} {:08x} {:08x} {:08x} {flags} {:08x} {:08x} {:08x}",
                    controls.steer.to_bits(),
                    controls.gas.to_bits(),
                    controls.brake.to_bits(),
                    controls.clutch.to_bits(),
                    controls.hand_brake.to_bits(),
                    p.last_max_steer.to_bits(),
                    p.last_raw.to_bits(),
                    p.last_normalized.to_bits()
                );
                if port != game {
                    eprintln!("call {calls} (line {}): game {game}\n         port {port}", n + 1);
                    return (calls, groups, Some(n + 1));
                }
            }
            _ => {}
        }
    }
    (calls, groups, None)
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let path = args.get(1).expect("usage: pad_steering_check <tape> [--workers n] [--fault]");
    let value = |name: &str| args.iter().position(|a| a == name).map(|i| args[i + 1].clone());
    let fault = args.iter().any(|a| a == "--fault");
    let workers: usize = value("--workers").map(|n| n.parse().expect("workers")).unwrap_or(0);
    let tape = std::fs::read_to_string(path).expect("the tape");
    let results: Vec<(usize, usize, Option<usize>)> = if workers == 0 {
        vec![play(&tape, fault)]
    } else {
        std::thread::scope(|s| {
            let runs: Vec<_> = (0..workers).map(|_| s.spawn(|| play(&tape, fault))).collect();
            runs.into_iter().map(|r| r.join().expect("a worker")).collect()
        })
    };
    if results.iter().any(|r| *r != results[0]) {
        println!("pad steering: the {} runs differ from each other", results.len());
        std::process::exit(1);
    }
    let (calls, groups, wrong) = results[0];
    match wrong {
        None => println!("pad steering: IDENTICAL ({calls} calls of the game in {groups} settings, {} run(s), workers {workers})", results.len()),
        Some(line) => {
            println!("pad steering: DIFFERENT at line {line} (call {calls}, settings {groups})");
            std::process::exit(1);
        }
    }
}
