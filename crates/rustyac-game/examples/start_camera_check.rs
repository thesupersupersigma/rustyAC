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
//! With a fourth (`track_camera.tape`) and the AC folder, F3's track cameras: every track's
//! camera sets as the game's `CameraManager` reads them against the port's `TrackCameras::load`
//! (every field, the distances to the AI line, the paths' points), and the game's
//! `CameraTrack::update` against the port's (matrix, field of view, planes, exposures, depth
//! of field).
//!
//! `cargo run --release -p rustyac-game --example start_camera_check -- <start_camera.tape> [<free_camera.tape> [<driving_camera.tape> [<track_camera.tape> <AC folder>]]]`

use rustyac_game::render::track_camera::{CameraSet, RandomCamera, TrackCamera, TrackCameras};
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
    if let (Some(path), Some(ac)) = (std::env::args().nth(4), std::env::args().nth(5)) {
        failed |= !track(&std::fs::read_to_string(&path).expect("the track cameras' tape"), std::path::Path::new(&ac));
    }
    if let (Some(path), Some(seed)) = (std::env::args().nth(6), std::env::args().nth(7)) {
        failed |= !random(&std::fs::read_to_string(&path).expect("the random camera's tape"), seed.parse().expect("the seed"));
    }
    if failed {
        std::process::exit(1);
    }
}

/// Task 46: F2's random camera (`random_camera.tape`): the game's `ACCameraManager::update`,
/// `stepCameraRandom` and `setMode` with F1, F2, F3 and F6 against the port's `RandomCamera`
/// and `DrivingCamera`, `rand()` from the tape's seed.
fn random(tape: &str, seed: u32) -> bool {
    let mut rand = rustyac_physics::session::MsvcRand(seed);
    let (mut lines, mut wrong, mut groups) = (0usize, 0usize, 0usize);
    let mut camera = DrivingCamera::chase();
    let mut track = TrackCameras::default();
    let mut random = RandomCamera::new(None, &mut rustyac_physics::session::MsvcRand(0));
    let (mut cameras, mut global) = (0usize, 0i32);
    for line in tape.lines() {
        lines += 1;
        if let Some(group) = line.strip_prefix("group ") {
            groups += 1;
            let w: Vec<&str> = group.split(' ').collect();
            let n: Vec<i64> = w[..7].iter().map(|x| x.parse().unwrap()).collect();
            cameras = n[1] as usize;
            camera = DrivingCamera::chase();
            camera.near = 0.1;
            camera.set_drivable(n[4] as i32);
            camera.mode = [CameraMode::Cockpit, CameraMode::Car, CameraMode::Drivable, CameraMode::Track][n[2] as usize];
            camera.set_last_driving(if n[3] == 0 { CameraMode::Cockpit } else { CameraMode::Drivable });
            camera.car_index = n[5] as usize;
            track = TrackCameras { sets: vec![CameraSet::default(); n[0] as usize], current_set: n[6] as usize, ..TrackCameras::default() };
            camera.track_set = track.current_set;
            let t: Vec<f32> = w[7..10].iter().map(|x| float(x)).collect();
            random = RandomCamera { min_time: t[0], max_time: t[1], time: t[2], old_time: 0.0, probabilities: [0, 1, 2, 3].map(|i| w[10 + i].parse().unwrap()), last: 3 };
            global = 0;
            continue;
        }
        let (input, output) = line.split_once(" -> ").expect("a line of the tape");
        // (the game's currentGlobalCameraIndex changes in setMode only: on a key or a draw)
        let set_mode = input.starts_with("key") || (camera.random && random.old_time >= random.time);
        match input.split_once(' ').expect("an event") {
            ("key", "1") => camera.f1(),
            ("key", "2") => {
                if camera.f3() {
                    track.next_set();
                }
                camera.random = true;
            }
            ("key", "3") => {
                if camera.f3() {
                    track.next_set();
                }
            }
            ("key", _) => camera.f6(cameras),
            (_, dt) => random.update(float(dt), &mut camera, cameras, &mut track, &mut rand),
        }
        camera.track_set = track.current_set;
        if let Some(g) = camera.global_index().filter(|_| set_mode) {
            global = g;
        }
        let mode = match camera.mode {
            CameraMode::Cockpit => 0,
            CameraMode::Car => 1,
            CameraMode::Drivable => 2,
            CameraMode::Track => 3,
        };
        let last = if camera.last_driving() == CameraMode::Cockpit { 0 } else { 2 };
        let mut drivable = camera;
        drivable.mode = CameraMode::Drivable;
        let view = drivable.global_index().expect("a drivable view") - 1;
        let port = format!(
            "{mode} {last} {view} {} {} {} {} {:08x} {:08x} {:08x} {global}",
            camera.car_index, track.current_set, camera.random as u8, random.last, random.time.to_bits(), random.old_time.to_bits(), camera.near.to_bits()
        );
        if port != output {
            wrong += 1;
            if wrong <= 5 {
                println!("DIFFERENT line {lines} ({input}):
  game {output}
  port {port}");
            }
        }
    }
    if wrong == 0 {
        println!("random camera: IDENTICAL ({groups} groups, {lines} lines of the game)");
    } else {
        println!("random camera: DIFFERENT in {wrong} of {lines} lines");
    }
    wrong == 0
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
            k.random_blend = c[60 + c[59] as usize * 18];
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

/// The fields of a track camera in the order of the tape's `cam` lines.
fn camera_fields(c: &TrackCamera) -> Vec<u32> {
    let mut v: Vec<f32> = c.matrix.iter().flatten().copied().collect();
    v.extend([c.max_fov, c.min_fov, c.in_point, c.out_point]);
    v.extend(c.shadow_splits);
    v.extend([c.near, c.far, c.min_exposure, c.max_exposure, c.dof_factor, c.dof_focus, c.dof_range, c.far_distance, c.near_distance, c.spline_rotation, c.fov_gamma, c.spline_animation_length]);
    let mut bits: Vec<u32> = v.iter().map(|x| x.to_bits()).collect();
    bits.extend([c.dof_manual as u32, c.is_fixed as u32, c.spline_file.encode_utf16().count() as u32]);
    if let Some(spline) = &c.spline {
        bits.extend(spline.points.iter().flat_map(|p| p.point).map(|x| x.to_bits()));
    }
    bits
}

/// Task 46: F3's track cameras (`track_camera.tape`): the sets as loaded, then the frames.
fn track(tape: &str, ac: &std::path::Path) -> bool {
    use rustyac_physics::track::spline::{InterpolatingSpline, SplinePoint};
    let (mut lines, mut wrong, mut frames, mut cameras) = (0usize, 0usize, 0usize, 0usize);
    let mut port = TrackCameras::default();
    let mut folder = String::new();
    let (mut set, mut camera) = (0usize, 0usize);
    let mut previous = rustyac_game::view::IDENTITY;
    let complain = |what: String, wrong: &mut usize| {
        *wrong += 1;
        if *wrong <= 5 {
            println!("DIFFERENT track {what}");
        }
    };
    for text in tape.lines() {
        lines += 1;
        let (head, rest) = text.split_once(' ').unwrap_or((text, ""));
        match head {
            "group" => {
                folder = rest.rsplit_once(' ').expect("group").0.to_string();
                (set, camera) = (0, 0);
                previous = rustyac_game::view::IDENTITY;
            }
            "line" => {
                let v: Vec<f32> = rest.split(' ').map(float).collect();
                let mut line = InterpolatingSpline::default();
                line.points = v.chunks(3).map(|p| SplinePoint { point: [p[0], p[1], p[2]], point_length: 0.0, tag: 0 }).collect();
                port = TrackCameras::load(&ac.join(&folder), &line, None).expect("the port's sets");
            }
            "cfg" => {
                let v: Vec<f32> = rest.split(' ').map(float).collect();
                (port.upward_offset, port.wobbling_speed, port.wobbling_strength) = (v[0], v[1], v[2]);
            }
            "set" => {
                let (count, name) = rest.split_once(' ').unwrap_or((rest, ""));
                let ours = port.sets.get(set);
                if ours.map(|s| (s.cameras.len(), s.name.as_str())) != Some((count.parse().expect("count"), name)) {
                    complain(format!("set {folder} #{set}: game {count} {name:?}, port {:?}", ours.map(|s| (s.cameras.len(), &s.name))), &mut wrong);
                }
                set += 1;
                camera = 0;
            }
            "cam" => {
                cameras += 1;
                let (fields, path) = rest.split_once(" |").expect("cam");
                let mut v: Vec<&str> = fields.split(' ').collect();
                let name_length: usize = v.remove(0).parse().expect("name");
                let mut game: Vec<u32> = v[..35].iter().map(|x| float(x).to_bits()).collect();
                game.extend(v[35..].iter().map(|x| x.parse::<u32>().expect("flag")));
                game.extend(path.split(' ').filter(|x| !x.is_empty()).map(|x| float(x).to_bits()));
                let ours = port.sets.get(set - 1).and_then(|s| s.cameras.get(camera));
                let same = ours.is_some_and(|c| camera_fields(c) == game && c.name.encode_utf16().count() == name_length);
                if !same {
                    complain(format!("camera {folder} set {} #{camera}:\n  game {game:08x?}\n  port {:08x?}", set - 1, ours.map(camera_fields)), &mut wrong);
                }
                camera += 1;
            }
            "f" => {
                frames += 1;
                let (input, output) = rest.split_once(" -> ").expect("a frame");
                let v: Vec<&str> = input.split(' ').collect();
                let f: Vec<f32> = v[3..].iter().map(|x| float(x)).collect();
                if v[0] == "1" {
                    port.next_set();
                }
                port.still = v[1] == "1";
                let mut body = rustyac_game::view::IDENTITY;
                for (i, value) in f[1..17].iter().enumerate() {
                    body[i / 4][i % 4] = *value;
                }
                let dt = if v[2] == "1" { f[22] * f[0] } else { f[22] };
                let frame = port.update(&previous, &body, [f[17], f[18], f[19]], f[20], f[21], dt).expect("a track camera");
                previous = frame.matrix;
                let mut ours: Vec<u32> = frame.matrix.iter().flatten().map(|x| x.to_bits()).collect();
                ours.extend([frame.fov, frame.near, frame.far, frame.min_exposure, frame.max_exposure, frame.dof_factor, frame.dof_focus, frame.dof_range, 30.0].map(f32::to_bits));
                let game: Vec<u32> = output.split(' ').map(|x| float(x).to_bits()).collect();
                if ours != game {
                    complain(format!("frame line {lines} ({folder}, set {}, {}):\n  game {game:08x?}\n  port {ours:08x?}", port.current_set, frame.name), &mut wrong);
                }
            }
            _ => panic!("line {lines}: {head}?"),
        }
    }
    if wrong == 0 {
        println!("track cameras: IDENTICAL ({cameras} cameras of {} sets, {frames} frames)", tape.lines().filter(|l| l.starts_with("set ")).count());
    } else {
        println!("track cameras: DIFFERENT in {wrong} of {lines} lines");
    }
    wrong == 0
}
