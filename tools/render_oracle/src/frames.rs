// SPDX-License-Identifier: GPL-3.0-or-later

//! What a frame of the proof is: which track, where the camera stands, what its lens and its
//! shadow ranges are. Both sides get the very same numbers (the same code computes them in
//! each process).

use std::path::Path;

use rustyac_physics::track::loader;
use rustyac_physics::vecmath::Mat44f;

use crate::Args;

/// What both sides seed the C runtime's `rand()` with before the car is loaded.
pub const RAND_SEED: u32 = 21;

/// The frame time both sides run with.
pub const DT: f32 = 1.0 / 60.0;

pub struct ModelFile {
    /// the path the model is opened by, on both sides
    pub filename: String,
    pub position: [f32; 3],
    pub rotation: [f32; 3],
}

pub struct TrackSpec {
    pub name: String,
    /// the folder of the track's models, as text (its `texture` folder holds loose images that
    /// replace embedded ones)
    pub folder: String,
    pub models: Vec<ModelFile>,
    /// `data/lighting.ini` of the track (or of its layout): the sun's pitch and heading
    pub sun_pitch: Option<f32>,
    pub sun_heading: Option<f32>,
}

pub struct CarSpec {
    /// the car's folder name under `content/cars`
    pub name: String,
    /// the car's folder as text (the models are opened by `<folder>/<file>`)
    pub folder: String,
    pub skin: String,
    /// `car.ini [CONTROLS] STEER_LOCK`
    pub steer_lock: f32,
}

#[derive(Clone, Copy)]
pub struct CameraSpec {
    /// `Camera::matrix`: rows right, up, back, position
    pub matrix: [f32; 16],
    pub fov: f32,
    pub near: f32,
    /// what the game camera writes; `None` leaves what `Sim::Sim` set (40000)
    pub far: Option<f32>,
    /// the arguments of `setShadowMapsSplits`
    pub splits: [f32; 4],
}

/// One frame of a run: the car's state of that frame and where the camera stands.
pub struct Step {
    pub state: rustyac_render::car::CarPhysicsState,
    pub camera: CameraSpec,
}

pub struct Frame {
    pub name: String,
    pub track: Option<TrackSpec>,
    pub car: Option<CarSpec>,
    /// the frames, in order; every one is updated and rendered
    pub steps: Vec<Step>,
    /// the first frame that is logged and read back (0 = the first one rendered)
    pub capture: usize,
    /// a sequence from a tape: every frame from `capture` on is compared
    pub sequence: bool,
    /// race.ini `[LIGHTING] SUN_ANGLE`
    pub sun_angle: f32,
    /// race.ini `[WEATHER] NAME`
    pub weather: String,
    /// the model `Sim::initStaticCubemap` draws into the reflection cube map
    pub cubemap_model: String,
}

/// `TrackAvatar::init3D` placing a model's top node (`models.ini` ROTATION / POSITION).
pub fn placed(matrix: &[f32; 16], entry: &ModelFile) -> [f32; 16] {
    let mut m = Mat44f::default();
    for r in 0..4 {
        for c in 0..4 {
            m.m[r][c] = matrix[r * 4 + c];
        }
    }
    let out = loader::top_node_matrix(&m, entry.position, entry.rotation);
    let mut flat = [0.0f32; 16];
    for r in 0..4 {
        for c in 0..4 {
            flat[r * 4 + c] = out.m[r][c];
        }
    }
    flat
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

fn normalized(a: [f32; 3]) -> [f32; 3] {
    let l = (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt();
    [a[0] / l, a[1] / l, a[2] / l]
}

/// A camera world matrix that stands at `eye` and looks at `target`.
pub fn look_from(eye: [f32; 3], target: [f32; 3]) -> [f32; 16] {
    let z = normalized(sub(eye, target));
    let x = normalized(cross([0.0, 1.0, 0.0], z));
    let y = cross(z, x);
    [x[0], x[1], x[2], 0.0, y[0], y[1], y[2], 0.0, z[0], z[1], z[2], 0.0, eye[0], eye[1], eye[2], 1.0]
}

/// The frame the command line asks for.
pub fn build(args: &Args) -> Result<Frame, String> {
    let mut track = None;
    // where the car would stand: the first grid slot, or the origin without a track
    let mut place = [0.0f32, 0.0, 0.0];
    let mut forward = [0.0f32, 0.0, 1.0];
    if let Some(name) = &args.track {
        let folder = args.game.join("content/tracks").join(name);
        if !folder.is_dir() {
            return Err(format!("no track folder {}", folder.display()));
        }
        let models = loader::track_models(&folder, &args.layout)?;
        let mut files = Vec::new();
        for model in &models {
            files.push(ModelFile { filename: path_text(&model.file), position: model.position, rotation: model.rotation });
        }
        // TrackAvatar::TrackAvatar: `<data folder>/data/lighting.ini`
        let data = if args.layout.is_empty() { folder.clone() } else { folder.join(&args.layout) };
        let (mut sun_pitch, mut sun_heading) = (None, None);
        if let Ok(ini) = rustyac_physics::data::ini::IniReader::load(&data.join("data/lighting.ini")) {
            sun_pitch = Some(ini.get_float("LIGHTING", "SUN_PITCH_ANGLE").unwrap_or(0.0));
            sun_heading = Some(ini.get_float("LIGHTING", "SUN_HEADING_ANGLE").unwrap_or(0.0));
        }
        track = Some(TrackSpec { name: name.clone(), folder: path_text(&folder), models: files, sun_pitch, sun_heading });
        let (physical, _) = loader::load_track(&folder, &args.layout)?;
        let pose = ["START", "HOTLAP_START", "PIT"].iter().find_map(|set| physical.spawn_pose(set, 0));
        if let Some((position, tail)) = pose {
            place = [position.x, position.y, position.z];
            forward = [-tail.x, -tail.y, -tail.z];
        }
    }
    // the car, where its physics state has it
    let mut car = None;
    let mut states: Vec<rustyac_render::car::CarPhysicsState> = Vec::new();
    let mut eyes = [0.0f32, 1.0, 0.0];
    if let Some(name) = &args.car {
        let folder = args.game.join("content/cars").join(name);
        if !folder.is_dir() {
            return Err(format!("no car folder {}", folder.display()));
        }
        states = match (&args.tape, &args.pose) {
            (Some(tape), _) => read_tape(tape, args.tape_from, args.frames)?,
            (None, Some(file)) => vec![rustyac_render::car::CarPhysicsState::from_bytes(&std::fs::read(file).map_err(|e| format!("{}: {e}", file.display()))?)?; args.capture + 1],
            (None, None) => return Err("--car needs --pose <file> (rustyac.exe --screenshot … --pose-out <file> writes one) or --tape <file>".into()),
        };
        if states.is_empty() {
            return Err("the tape has no frame in that range".into());
        }
        let car_ini = rustyac_physics::data::ini::IniReader::load(&folder.join("data/car.ini"))?;
        let steer_lock = car_ini.get_float("CONTROLS", "STEER_LOCK").unwrap_or(0.0);
        eyes = car_ini.get_float3("GRAPHICS", "DRIVEREYES").unwrap_or(eyes);
        let skin = match &args.skin {
            Some(skin) => skin.clone(),
            None => {
                // the first skin folder by name
                let mut skins: Vec<String> = std::fs::read_dir(folder.join("skins")).map_err(|e| e.to_string())?.filter_map(|e| e.ok()).filter(|e| e.path().is_dir()).map(|e| e.file_name().to_string_lossy().into_owned()).collect();
                skins.sort();
                skins.first().cloned().unwrap_or_default()
            }
        };
        car = Some(CarSpec { name: name.clone(), folder: path_text(&folder), skin, steer_lock });
    }
    // where the sun is seen from the car (for the view into the sun): the same angles the
    // lighting uses, to the precision a camera needs
    let sun_from = {
        let (pitch, heading) = match &track {
            Some(t) => (t.sun_pitch.unwrap_or(45.0), t.sun_heading.unwrap_or(0.0)),
            None => (45.0, 0.0),
        };
        let (p, a, h) = (pitch.to_radians(), args.sun_angle.to_radians(), heading.to_radians());
        // (0, -1, 0) through Rx(pitch) Rz(angle) Ry(heading), row vectors
        let v1 = [0.0, -p.cos(), -p.sin()];
        let v2 = [v1[0] * a.cos() - v1[1] * a.sin(), v1[0] * a.sin() + v1[1] * a.cos(), v1[2]];
        let light = [v2[0] * h.cos() + v2[2] * h.sin(), v2[1], -v2[0] * h.sin() + v2[2] * h.cos()];
        [-light[0], -light[1], -light[2]]
    };
    // the camera of a frame: it follows the car's state of that frame
    let camera_of = |state: Option<&rustyac_render::car::CarPhysicsState>| -> Result<CameraSpec, String> {
        let (mut place, mut forward) = (place, forward);
        if let Some(s) = state {
            let w = s.world_matrix.m;
            place = [w[3][0], w[3][1], w[3][2]];
            forward = [w[2][0], w[2][1], w[2][2]];
        }
        let at = |ahead: f32, up: f32| [place[0] + forward[0] * ahead, place[1] + up, place[2] + forward[2] * ahead];
        // the driver's eyes: car axes are +x left, +y up, +z forward
        let eye_point = |ahead: f32| match state {
            Some(s) => {
                let w = s.world_matrix.m;
                let p = [eyes[0], eyes[1], eyes[2] + ahead];
                [p[0] * w[0][0] + p[1] * w[1][0] + p[2] * w[2][0] + w[3][0], p[0] * w[0][1] + p[1] * w[1][1] + p[2] * w[2][1] + w[3][1], p[0] * w[0][2] + p[1] * w[1][2] + p[2] * w[2][2] + w[3][2]]
            }
            None => at(ahead, 1.0),
        };
        camera_for(&args.view, &at, &eye_point, sun_from, state)
    };
    for (index, state) in states.iter_mut().enumerate() {
        for (name, value, from) in &args.set {
            if index >= *from {
                apply_set(state, name, value)?;
            }
        }
    }
    let steps: Vec<Step> = if states.is_empty() {
        let camera = camera_of(None)?;
        (0..=args.capture).map(|_| Step { state: rustyac_render::car::CarPhysicsState::at_origin(), camera }).collect()
    } else {
        let mut steps = Vec::with_capacity(states.len());
        for state in &states {
            steps.push(Step { state: *state, camera: camera_of(Some(state))? });
        }
        steps
    };
    let name = match &args.track {
        Some(t) => format!("{t}_{}", args.view),
        None => format!("empty_{}", args.view),
    };
    let name = match (&car, &args.tape, &args.pose) {
        (Some(c), Some(tape), _) => format!("{name}_{}_{}_{}_{}", c.name, tape.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default(), args.tape_from, args.frames),
        (Some(c), None, Some(pose)) => format!("{name}_{}_{}", c.name, pose.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()),
        _ => name,
    };
    let name = if args.label.is_empty() { name } else { format!("{name}_{}", args.label) };
    let name = if args.sun_angle == -16.0 { name } else { format!("{name}_sun{}", args.sun_angle) };
    let p = crate::root::profile();
    let t = crate::root::TASK_20;
    let name = if (p.shadow_map_size, p.cubemap_size, p.world_detail, p.anisotropic) == (t.shadow_map_size, t.cubemap_size, t.world_detail, t.anisotropic) { name } else { format!("{name}_s{}c{}w{}a{}", p.shadow_map_size, p.cubemap_size, p.world_detail, p.anisotropic) };
    let name = if p.cubemap_faces_per_frame == 0 && p.cubemap_far_plane == 0.0 { name } else { format!("{name}_f{}far{}", p.cubemap_faces_per_frame, p.cubemap_far_plane) };
    let mut cubemap_model = format!("{}/content/objects3D/cubemap_model.kn5", path_text(&args.game));
    if let Some(track) = &track {
        let own = format!("{}/cubemap_model.kn5", track.folder);
        if Path::new(&own).is_file() {
            cubemap_model = own;
        }
    }
    let sequence = args.tape.is_some();
    let capture = if sequence { args.capture.min(steps.len() - 1) } else { args.capture };
    Ok(Frame { name, track, car, steps, capture, sequence, sun_angle: args.sun_angle, weather: args.weather.clone(), cubemap_model })
}

/// The picture frames `from .. from + count` of a tape written by `car_oracle run
/// --audio-tape`: a header of `key=value` lines and an empty line, then records: 1 + 0x48
/// bytes (a physics event) or 2 + a step count (u32) + the game's `CarPhysicsState` (0xb70
/// bytes) + 1 byte.
pub fn read_tape(path: &Path, from: usize, count: usize) -> Result<Vec<rustyac_render::car::CarPhysicsState>, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let head = bytes.windows(2).position(|w| w == [10u8, 10u8]).ok_or("the tape has no header")?;
    let mut at = head + 2;
    let mut index = 0usize;
    let mut out = Vec::new();
    while at < bytes.len() && out.len() < count {
        match bytes[at] {
            1 => at += 1 + 0x48,
            2 => {
                let state = bytes.get(at + 5..at + 5 + rustyac_render::state::GAME_SIZE).ok_or("the tape ends inside a frame")?;
                if index >= from {
                    out.push(rustyac_render::car::CarPhysicsState::from_game_bytes(state)?);
                }
                index += 1;
                at += 5 + rustyac_render::state::GAME_SIZE + 1;
            }
            other => return Err(format!("{}: a record of kind {other} at byte {at}", path.display())),
        }
    }
    Ok(out)
}

/// One `--set name=value`.
fn apply_set(s: &mut rustyac_render::car::CarPhysicsState, name: &str, value: &str) -> Result<(), String> {
    let number = |v: &str| v.parse::<f32>().map_err(|_| format!("--set {name}={v}: not a number"));
    let on = |v: &str| -> Result<bool, String> { Ok(number(v)? != 0.0) };
    match name {
        "lights" => s.status_bytes = (s.status_bytes & !1) | on(value)? as u32,
        "flash" => s.actions_state = (s.actions_state & !0x400) | if on(value)? { 0x400 } else { 0 },
        "brake" => s.brake = number(value)?,
        "gas" => s.gas = number(value)?,
        "gear" => s.gear = number(value)? as i32,
        "rpm" => s.engine_rpm = number(value)?,
        "limiter" => s.limiter_rpm = number(value)? as i32,
        "kmh" => s.speed = number(value)? / 3.6,
        "fuel" => s.fuel = number(value)?,
        "turbo" => s.turbo_boost = number(value)?,
        "water" => s.water = number(value)?,
        "kers" => s.kers_is_charging = on(value)?,
        "pit" => s.tyre_surface_def.iter_mut().for_each(|d| d.is_pitlane = value != "0"),
        "dirt" => {
            let k = number(value)?;
            s.tyre_surface_def.iter_mut().for_each(|d| d.dirt_additive_k = k);
        }
        "damage" => {
            let parts: Vec<&str> = value.split(':').collect();
            if parts.len() != 5 {
                return Err("--set damage=front:rear:left:right:centre".into());
            }
            for (slot, part) in s.damage_zone_level.iter_mut().zip(parts) {
                *slot = number(part)?;
            }
        }
        other => return Err(format!("--set {other}: not one of lights flash brake gas gear rpm limiter kmh fuel turbo water kers pit dirt damage")),
    }
    Ok(())
}

type Point<'a> = &'a dyn Fn(f32, f32) -> [f32; 3];

/// The lens and the shadow ranges the game cameras use (CameraDrivableManager::updateChase
/// 0x1400c7120, ::updateDash 0x1400c7b50, CameraOnBoardFree::update 0x140115db0).
fn camera_for(view: &str, at: Point, eye_point: &dyn Fn(f32) -> [f32; 3], sun_from: [f32; 3], state: Option<&rustyac_render::car::CarPhysicsState>) -> Result<CameraSpec, String> {
    let camera = match view {
        "chase" => CameraSpec { matrix: look_from(at(-6.0, 2.0), at(0.0, 1.0)), fov: 60.0, near: 1.0, far: None, splits: [10.0, 50.0, 150.0, 500.0] },
        "cockpit" => CameraSpec { matrix: look_from(at(0.0, 1.0), at(10.0, 0.9)), fov: 56.0, near: 0.05, far: None, splits: [f32::from_bits(0x3fa6_6666), 80.0, 250.0, 500.0] },
        "free" => CameraSpec { matrix: look_from(at(-30.0, 15.0), at(20.0, 0.0)), fov: 60.0, near: 0.1, far: None, splits: [10.0, 80.0, 300.0, 1500.0] },
        // from the driver's eyes
        "eyes" => CameraSpec { matrix: look_from(eye_point(0.0), eye_point(10.0)), fov: 56.0, near: 0.05, far: None, splits: [f32::from_bits(0x3fa6_6666), 80.0, 250.0, 500.0] },
        // a chase camera on the far side of the car from the sun, looking at it over the car
        "sun" => {
            let target = at(0.0, 1.0);
            let eye = [target[0] - sun_from[0] * 7.0, target[1] + 0.6, target[2] - sun_from[2] * 7.0];
            let look = [eye[0] + sun_from[0], eye[1] + sun_from[1], eye[2] + sun_from[2]];
            CameraSpec { matrix: look_from(eye, look), fov: 60.0, near: 1.0, far: None, splits: [10.0, 50.0, 150.0, 500.0] }
        }
        // far enough for the second level of detail
        "far" => CameraSpec { matrix: look_from(at(-25.0, 4.0), at(0.0, 1.0)), fov: 60.0, near: 1.0, far: None, splits: [10.0, 50.0, 150.0, 500.0] },
        // beside the car, low: the wheels, the discs, the ground under the car
        "side" => {
            let w = state.map(|s| s.world_matrix.m).unwrap_or(Mat44f::IDENTITY.m);
            let target = at(0.0, 0.5);
            let ahead = at(1.5, 0.5);
            let eye = [ahead[0] + w[0][0] * 4.5, target[1] + 0.6, ahead[2] + w[0][2] * 4.5];
            CameraSpec { matrix: look_from(eye, target), fov: 60.0, near: 1.0, far: None, splits: [10.0, 50.0, 150.0, 500.0] }
        }
        // behind the car, close: the rear lights, the exhausts
        "rear" => CameraSpec { matrix: look_from(at(-4.5, 1.2), at(0.0, 0.6)), fov: 60.0, near: 1.0, far: None, splits: [10.0, 50.0, 150.0, 500.0] },
        // in front of the car, close: the head lights, the windscreen, the driver
        "front" => CameraSpec { matrix: look_from(at(4.5, 1.2), at(0.0, 0.6)), fov: 60.0, near: 1.0, far: None, splits: [10.0, 50.0, 150.0, 500.0] },
        other => return Err(format!("the view {other:?} is not one of chase, cockpit, free, eyes, sun, far, side, rear, front")),
    };
    Ok(camera)
}

/// A path as text with forward slashes.
pub fn path_text(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}
