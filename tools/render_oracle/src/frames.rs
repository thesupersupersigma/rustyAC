// SPDX-License-Identifier: GPL-3.0-or-later

//! What a frame of the proof is: which track, where the camera stands, what its lens and its
//! shadow ranges are. Both sides get the very same numbers (the same code computes them in
//! each process).

use std::path::Path;

use rustyac_physics::track::loader;
use rustyac_physics::vecmath::Mat44f;

use crate::Args;

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

pub struct Frame {
    pub name: String,
    pub track: Option<TrackSpec>,
    pub camera: CameraSpec,
    /// which frame is logged and read back (0 = the first one rendered)
    pub capture: usize,
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
    let at = |ahead: f32, up: f32| [place[0] + forward[0] * ahead, place[1] + up, place[2] + forward[2] * ahead];
    // the lens and the shadow ranges the game cameras use (CameraDrivableManager::updateChase
    // 0x1400c7120, ::updateDash 0x1400c7b50, CameraOnBoardFree::update 0x140115db0)
    let camera = match args.view.as_str() {
        "chase" => CameraSpec { matrix: look_from(at(-6.0, 2.0), at(0.0, 1.0)), fov: 60.0, near: 1.0, far: None, splits: [10.0, 50.0, 150.0, 500.0] },
        "cockpit" => CameraSpec { matrix: look_from(at(0.0, 1.0), at(10.0, 0.9)), fov: 56.0, near: 0.05, far: None, splits: [f32::from_bits(0x3fa6_6666), 80.0, 250.0, 500.0] },
        "free" => CameraSpec { matrix: look_from(at(-30.0, 15.0), at(20.0, 0.0)), fov: 60.0, near: 0.1, far: None, splits: [10.0, 80.0, 300.0, 1500.0] },
        other => return Err(format!("the view {other:?} is not one of chase, cockpit, free")),
    };
    let name = match &args.track {
        Some(t) => format!("{t}_{}", args.view),
        None => format!("empty_{}", args.view),
    };
    let name = if args.sun_angle == -16.0 { name } else { format!("{name}_sun{}", args.sun_angle) };
    let mut cubemap_model = format!("{}/content/objects3D/cubemap_model.kn5", path_text(&args.game));
    if let Some(track) = &track {
        let own = format!("{}/cubemap_model.kn5", track.folder);
        if Path::new(&own).is_file() {
            cubemap_model = own;
        }
    }
    Ok(Frame { name, track, camera, capture: args.capture, sun_angle: args.sun_angle, weather: args.weather.clone(), cubemap_model })
}

/// A path as text with forward slashes.
pub fn path_text(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}
