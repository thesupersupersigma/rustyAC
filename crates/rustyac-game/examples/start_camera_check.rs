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
//! With a third (`driving_camera.tape`, Task 46), the cockpit, drivable and car cameras: the
//! game's `CameraOnBoard`, `CameraDrivableManager` and `CameraCarManager` against the port's
//! `DrivingCamera` (matrix, field of view, near plane).
//!
//! `cargo run --release -p rustyac-game --example start_camera_check -- <start_camera.tape> [<free_camera.tape> [<driving_camera.tape>]]`

use rustyac_game::render::scene::{CameraFrame, CameraMode, CarCamera, CarShape, DrivingCamera, MouseCamera, MouseCameraInput, StartCamera};
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
    if let Some(path) = std::env::args().nth(3) {
        failed |= !driving(&std::fs::read_to_string(&path).expect("the driving cameras' tape"));
    }
    if failed {
        std::process::exit(1);
    }
}

/// Task 46: the cockpit, drivable and car cameras (`driving_camera.tape`): the game's
/// `CameraOnBoard::update`, `CameraDrivableManager::update` and `CameraCarManager::update`
/// against the port's `DrivingCamera`. Lines that differ are counted by mode, view and features.
fn driving(tape: &str) -> bool {
    let (mut lines, mut wrong) = (0usize, 0usize);
    let mut camera = DrivingCamera::chase();
    let mut shape = CarShape::plain();
    let mut car = CarView::default();
    let mut what = String::new();
    let mut wrong_by: std::collections::BTreeMap<String, (usize, usize)> = Default::default();
    for line in tape.lines() {
        lines += 1;
        if let Some(seg) = line.strip_prefix("seg ") {
            let w: Vec<&str> = seg.split(' ').collect();
            let (mode, view, index, features): (u32, i32, usize, u32) = (w[0].parse().unwrap(), w[1].parse().unwrap(), w[2].parse().unwrap(), w[3].parse().unwrap());
            let c: Vec<f32> = w[4..].iter().map(|x| float(x)).collect();
            let rand = camera.cockpit.rand;
            camera = DrivingCamera::chase();
            camera.cockpit.rand = rand;
            camera.near = 0.0;
            camera.set_drivable(view);
            match mode {
                0 => camera.set_mode_kept(0),
                2 => camera.set_car_camera(index),
                _ => {}
            }
            let three = |i: usize| [c[i], c[i + 1], c[i + 2]];
            shape = CarShape {
                graphics_offset: three(0),
                graphics_pitch: c[3],
                eye_model: three(4),
                bumper_pos: three(7),
                bumper_pitch: c[10],
                bonnet_pos: three(11),
                bonnet_pitch: c[14],
                dash_pos: three(15),
                drivable_yaw: c[18],
                onboard_fov: c[19],
                eye_pitch: c[36],
                look_back: three(38),
                drivable_look_back: three(53),
                car_cameras: (0..c[59] as usize)
                    .map(|k| {
                        let at = 60 + k * 18;
                        let mut matrix = [[0.0; 4]; 4];
                        for (i, value) in c[at..at + 16].iter().enumerate() {
                            matrix[i / 4][i % 4] = *value;
                        }
                        CarCamera { matrix, fov: c[at + 16] }
                    })
                    .collect(),
                ..CarShape::plain()
            };
            let k = &mut camera.cockpit;
            k.g_force_mix = three(21);
            k.g_force_lag_mix = c[24];
            k.head_rotation_degrees = c[25];
            k.head_rotation_speed = c[26];
            k.shake_mul = c[27];
            k.shake_freq = three(28);
            k.shake_scale = three(31);
            k.still_camera = c[34] != 0.0;
            k.world_aligned = c[35] != 0.0;
            k.yaw = c[37];
            camera.chase = [(c[41], c[42], c[43]), (c[44], c[45], c[46])];
            (camera.acc_gx_mul, camera.acc_gz_mul, camera.chase_filter) = (c[47], c[48], c[49]);
            (camera.rotation_degrees, camera.rotation_speed) = (c[50], c[51]);
            what = format!("mode {mode} view {view} features {features}");
            continue;
        }
        let (input, output) = line.split_once(" -> ").expect("a line of the tape");
        let w: Vec<&str> = input.split(' ').collect();
        let v: Vec<f32> = w[3..].iter().map(|x| float(x)).collect();
        if w[1] == "1" {
            match camera.mode {
                CameraMode::Car => camera.f6(shape.car_cameras.len()),
                _ => camera.f1(),
            }
        }
        camera.actions = u32::from_str_radix(w[2], 16).expect("actions");
        for (i, value) in v[..16].iter().enumerate() {
            car.body[i / 4][i % 4] = *value;
        }
        car.hubs[2][3] = [v[16], v[17], v[18], 1.0];
        car.hubs[3][3] = [v[19], v[20], v[21], 1.0];
        car.speed_ms = v[22];
        car.speed_kmh = v[22] * 3.6;
        let frame = camera.update(&car, &shape, [v[23], v[24], v[25]], v[29]);
        let mut port: Vec<u32> = frame.matrix.iter().flatten().map(|x| x.to_bits()).collect();
        port.extend([frame.fov.to_bits(), frame.near.to_bits()]);
        let game: Vec<u32> = output.split(' ').take(18).map(|x| float(x).to_bits()).collect();
        let key = format!("{what} now {}", camera.name());
        let entry = wrong_by.entry(key).or_default();
        entry.1 += 1;
        if port != game {
            wrong += 1;
            entry.0 += 1;
            if wrong <= 5 {
                println!("DIFFERENT driving line {lines} ({what}, {}):
  game {game:08x?}
  port {port:08x?}", camera.name());
            }
        }
    }
    if wrong == 0 {
        println!("driving cameras: IDENTICAL ({lines} lines of the game)");
    } else {
        for (key, (bad, all)) in &wrong_by {
            if *bad != 0 {
                println!("  {key}: {bad} of {all} frames differ");
            }
        }
        println!("driving cameras: DIFFERENT in {wrong} of {lines} lines");
    }
    wrong == 0
}
