// SPDX-License-Identifier: GPL-3.0-or-later

//! Task 34: plays a tape of the game's own mouse steering (`car_oracle mouse-micro`:
//! `KeyboardCarControl::readFromIni`'s `MOUSE_*` keys, `setMouseSteering`, `getMouseSteering`
//! on made-up cursors and windows) through the port's `KeyboardCarControl`, and says whether
//! every value and every cursor put back is the same, bit for bit.
//!
//! `cargo run --release -p rustyac-game --example mouse_steering_check -- <mouse.tape>`

use rustyac_game::input::ini::ControlsIni;
use rustyac_game::input::keyboard::{KeyboardCarControl, MouseSteeringWindow};

struct Tape {
    x: i32,
    rect: [i32; 4],
    set: Vec<(i32, i32)>,
}

impl MouseSteeringWindow for Tape {
    fn cursor_x(&mut self) -> i32 {
        self.x
    }
    fn window_rect(&mut self) -> [i32; 4] {
        self.rect
    }
    fn set_cursor(&mut self, x: i32, y: i32) {
        self.set.push((x, y));
    }
}

fn hex(s: &str) -> u32 {
    u32::from_str_radix(s, 16).expect("hex")
}

fn main() {
    let path = std::env::args().nth(1).expect("usage: mouse_steering_check <mouse.tape>");
    let tape = std::fs::read_to_string(&path).expect("the tape");
    let mut k = KeyboardCarControl::defaults();
    let (mut lines, mut wrong) = (0usize, 0usize);
    for line in tape.lines() {
        lines += 1;
        let (input, output) = line.split_once(" -> ").expect("a line of the tape");
        let out: Vec<&str> = output.split(' ').collect();
        let same = if let Some(ini) = input.strip_prefix("ini ") {
            let port = KeyboardCarControl::from_ini(&ControlsIni::parse(&ini.replace('|', "\r\n")));
            format!("{} {} {:08x}", port.mouse_steering as u8, port.mouse_accelerator_brake as u8, port.mouse_speed.to_bits()) == output
        } else if let Some(set) = input.strip_prefix("set ") {
            k.set_mouse_steering(set == "1");
            k.mouse_speed = f32::from_bits(hex(out[1]));
            (k.mouse_steering as u8).to_string() == out[0]
        } else {
            let v: Vec<&str> = input.split(' ').collect();
            k.old_mouse_value = f32::from_bits(hex(v[1]));
            k.mouse_speed = f32::from_bits(hex(v[2]));
            let n = |i: usize| v[i].parse::<i32>().expect("a number");
            let mut window = Tape { x: n(3), rect: [n(4), n(5), n(6), n(7)], set: Vec::new() };
            let r = k.get_mouse_steering(&mut window);
            let (sx, sy) = window.set.first().copied().unwrap_or((0, 0));
            format!("{:08x} {:08x} {sx} {sy} {}", r.to_bits(), k.old_mouse_value.to_bits(), window.set.len()) == output
        };
        if !same {
            wrong += 1;
            if wrong <= 10 {
                println!("DIFFERENT: {line}");
            }
        }
    }
    if wrong == 0 {
        println!("mouse steering: IDENTICAL ({lines} lines of the game)");
    } else {
        println!("mouse steering: DIFFERENT in {wrong} of {lines} lines");
        std::process::exit(1);
    }
}
