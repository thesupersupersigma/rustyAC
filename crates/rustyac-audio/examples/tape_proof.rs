// SPDX-License-Identifier: GPL-3.0-or-later

//! `tape_proof <AC folder> <drive.audiotape> <camera> <log out> [wav out]`: the port's side of
//! `audio_oracle run --side port` with whatever FMOD this platform loads (FMOD Engine 2.03 off
//! Windows), as the page's `sound_proof` (crates/rustyac-web/src/sound.rs) runs it, for
//! comparing call logs across platforms.

use std::path::{Path, PathBuf};

use rustyac_audio::car::{CarFrame, CarInfo};
use rustyac_audio::engine::EngineFiles;
use rustyac_audio::fmod::{log, raw};
use rustyac_audio::golden::{run_session, Session, SessionFrame, AUDIO_INI};
use rustyac_audio::sim::PhysicsEvent;
use rustyac_audio::tape::{OracleCamera, Tape};
use rustyac_audio::track::Scene;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 5 {
        eprintln!("tape_proof <AC folder> <drive.audiotape> <camera> <log out> [wav out]");
        std::process::exit(2);
    }
    if let Err(e) = run(Path::new(&args[1]), Path::new(&args[2]), &args[3], Path::new(&args[4]), args.get(5).map(PathBuf::from)) {
        eprintln!("tape_proof: {e}");
        std::process::exit(1);
    }
}

fn run(root: &Path, tape: &Path, camera: &str, log_out: &Path, wav: Option<PathBuf>) -> Result<(), String> {
    let t = Tape::read(tape)?;
    let camera = OracleCamera::parse(camera)?;
    let car = t.get("car").ok_or("the tape names no car")?.to_string();
    // the tape has the Windows folder: the same name under this folder's content/tracks
    let track = t.get("track_folder").and_then(|folder| Path::new(&folder.replace('\\', "/")).file_name().map(|n| n.to_string_lossy().into_owned()));
    let layout = t.get("track_layout").unwrap_or("").to_string();
    let mut frames = Vec::new();
    let mut first: Option<CarFrame> = None;
    for f in &t.frames {
        let mut frame = CarFrame::from_physics_state(&f.state);
        frame.tc_in_action = f.tc_in_action;
        let first = first.get_or_insert_with(|| frame.clone());
        let listener = camera.listener(&frame, first);
        let events: Vec<PhysicsEvent> = f.events.iter().map(|raw| PhysicsEvent::from_bytes(raw)).collect();
        frames.push((frame, events, listener));
    }
    // the oracle's levels (its root's cfg/audio.ini), beside the log
    let audio_ini = log_out.with_extension("audio.ini");
    std::fs::write(&audio_ini, AUDIO_INI).map_err(|e| format!("{}: {e}", audio_ini.display()))?;
    let track_folder = track.map(|name| root.join("content/tracks").join(name));
    let scene = match &track_folder {
        Some(folder) => Some(Scene::load(folder, &layout)?),
        None => None,
    };
    let surfaces: Vec<(String, String)> = match &track_folder {
        Some(folder) => {
            let data = if layout.is_empty() { folder.join("data") } else { folder.join(&layout).join("data") };
            let manager = rustyac_physics::track::surfaces::SurfacesManager::new(&root.join("system/data/surfaces.ini"), &data.join("surfaces.ini"))?;
            manager.surfaces.iter().map(|(key, surface)| (key.clone(), surface.wav.clone())).collect()
        }
        None => Vec::new(),
    };
    let output = match wav {
        Some(file) => raw::Output::WavNrt { file, rate: 48000, block: 800 },
        None => raw::Output::NoSoundNrt { rate: 48000, block: 800 },
    };
    raw::load(root, output)?;
    log::start(Some(log_out))?;
    let files = EngineFiles { content_root: root.to_path_buf(), audio_engine_ini: root.join("system/cfg/audio_engine.ini"), audio_ini };
    let info = CarInfo { unix_name: car.clone(), guid: 0, data_folder: root.join("content/cars").join(&car).join("data"), car_cameras_external_sound: Vec::new() };
    let track = match (&scene, &track_folder) {
        (Some(scene), Some(folder)) => Some((scene, if layout.is_empty() { folder.clone() } else { folder.join(&layout) })),
        _ => None,
    };
    let session = Session { files, master: 1.0, track, surfaces: &surfaces, car: info, camera: camera.view(), dt: 1.0 / 60.0 };
    let result = run_session(session, frames.iter().map(|(car, events, listener)| SessionFrame { car, events, listener: *listener }));
    let summary = log::finish().ok_or("no log")?;
    println!("tape_proof: {} frames, {} lines, hash {:016x}", frames.len(), summary.lines, summary.hash);
    result
}
