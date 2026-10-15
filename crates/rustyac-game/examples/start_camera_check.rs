// SPDX-License-Identifier: GPL-3.0-or-later

//! Task 34: plays a tape of the game's own session-start camera (`car_oracle
//! start-camera-micro`: `CameraModeStart::updateCar` on made-up cars, `rand()` from seed 1)
//! through the port's `StartCamera`, and says whether every camera matrix, field of view and
//! near plane is the same, bit for bit (and the far plane 15000 and the exposure 30 the port's
//! renderer has of its own).
//!
//! With a second tape (`free_camera.tape`), F7's free camera too: the game's
//! `CameraMouseControlBase::update` against the port's `MouseCamera`.
//!
//! `cargo run --release -p rustyac-game --example start_camera_check -- <start_camera.tape> [<free_camera.tape>]`

use rustyac_game::render::scene::{CameraFrame, MouseCamera, MouseCameraInput, StartCamera};
use rustyac_game::view::CarView;

fn float(s: &str) -> f32 {
    f32::from_bits(u32::from_str_radix(s, 16).expect("hex"))
}

fn main() {
    let path = std::env::args().nth(1).expect("usage: start_camera_check <start_camera.tape>");
    let tape = std::fs::read_to_string(&path).expect("the tape");
    let mut camera = StartCamera::new(-1.0);
    let mut car = CarView::default();
    let (mut lines, mut wrong) = (0usize, 0usize);
    for line in tape.lines() {
        lines += 1;
        if let Some(crew) = line.strip_prefix("crew ") {
            // (the game's rand() goes on from camera to camera)
            let rand = camera.rand;
            camera = StartCamera::new(float(crew));
            camera.rand = rand;
            continue;
        }
        let (input, output) = line.split_once(" -> ").expect("a line of the tape");
        let v: Vec<f32> = input.split(' ').skip(1).map(float).collect();
        let out: Vec<f32> = output.split(' ').map(float).collect();
        for (i, value) in v[1..17].iter().enumerate() {
            car.body[i / 4][i % 4] = *value;
        }
        for (i, value) in v[17..29].iter().enumerate() {
            car.render.contact_point[i / 3][i % 3] = *value;
        }
        car.pit_position = [v[29], v[30], v[31]];
        let frame = camera.update(&car, v[0], v[32]);
        let mut port: Vec<u32> = frame.matrix.iter().flatten().map(|x| x.to_bits()).collect();
        port.extend([frame.fov.to_bits(), frame.near.to_bits(), 15000.0f32.to_bits(), 30.0f32.to_bits()]);
        let game: Vec<u32> = out.iter().map(|x| x.to_bits()).collect();
        if port != game {
            wrong += 1;
            if wrong <= 5 {
                println!("DIFFERENT line {lines}:\n  game {game:08x?}\n  port {port:08x?}");
            }
        }
    }
    let mut failed = wrong != 0;
    if wrong == 0 {
        println!("start camera: IDENTICAL ({lines} lines of the game)");
    } else {
        println!("start camera: DIFFERENT in {wrong} of {lines} lines");
    }
    if let Some(path) = std::env::args().nth(2) {
        let tape = std::fs::read_to_string(&path).expect("the free camera's tape");
        let (mut lines, mut wrong) = (0usize, 0usize);
        let mut free: Option<MouseCamera> = None;
        for line in tape.lines() {
            lines += 1;
            if let Some(start) = line.strip_prefix("free ") {
                let v: Vec<f32> = start.split(' ').map(float).collect();
                let mut matrix = [[0.0f32; 4]; 4];
                for (i, value) in v[..16].iter().enumerate() {
                    matrix[i / 4][i % 4] = *value;
                }
                free = Some(MouseCamera::new(&CameraFrame { matrix, fov: v[16], near: 0.1 }));
                continue;
            }
            let (input, output) = line.split_once(" -> ").expect("a line of the tape");
            let v: Vec<&str> = input.split(' ').collect();
            let keys = u32::from_str_radix(v[1], 16).expect("keys");
            let held = |bit: u32| keys & (1 << bit) != 0;
            let input = MouseCameraInput { shift: held(0), ctrl: held(1), up: held(2), down: held(3), left: held(4), right: held(5), look: held(6), cursor: (v[2].parse().expect("x"), v[3].parse().expect("y")) };
            let frame = free.as_mut().expect("a free camera").update(&input, float(v[4]));
            let port: Vec<u32> = frame.matrix.iter().flatten().map(|x| x.to_bits()).collect();
            let game: Vec<u32> = output.split(' ').map(|x| float(x).to_bits()).collect();
            if port != game {
                wrong += 1;
                if wrong <= 5 {
                    println!("DIFFERENT free line {lines}:\n  game {game:08x?}\n  port {port:08x?}");
                }
            }
        }
        if wrong == 0 {
            println!("free camera: IDENTICAL ({lines} lines of the game)");
        } else {
            println!("free camera: DIFFERENT in {wrong} of {lines} lines");
            failed = true;
        }
    }
    if failed {
        std::process::exit(1);
    }
}
