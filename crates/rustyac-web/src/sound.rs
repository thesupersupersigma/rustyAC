// SPDX-License-Identifier: GPL-3.0-or-later

//! The sound's proof in the page: `audio_oracle run --side port` on a recorded drive, with the
//! browser's FMOD under the port's sound code. Its call log is the desktop's line for line (what
//! FMOD 1.08 and 2.03 answer differently is translated in `web/sound.js`).

use std::path::{Path, PathBuf};

use rustyac_audio::car::{CarFrame, CarInfo};
use rustyac_audio::engine::EngineFiles;
use rustyac_audio::fmod::{log, raw};
use rustyac_audio::golden::{run_session, Session, SessionFrame, AUDIO_INI};
use rustyac_audio::sim::PhysicsEvent;
use rustyac_audio::tape::{OracleCamera, Tape};
use rustyac_audio::track::Scene;
use wasm_bindgen::prelude::*;

use crate::web::{files, mounted, ROOT};

/// The drive of `tape` (an `.audiotape` of `car_oracle run --audio-tape`, a path in the game's
/// folder as the page has it) heard from `camera` (cockpit, chase, trackside), as the desktop's
/// audio oracle runs it: 1/60 s frames, one 800-sample block of a 48 kHz mix per frame,
/// written to FMOD's file system as `/fmodoutput.wav` when `wav`. Returns the FMOD call log.
/// Once per page (FMOD is set up once).
#[wasm_bindgen]
pub fn sound_proof(tape: &str, camera: &str, wav: bool) -> Result<String, JsError> {
    proof(tape, camera, wav).map_err(|e| JsError::new(&e))
}

fn proof(tape: &str, camera: &str, wav: bool) -> Result<String, String> {
    let root = PathBuf::from(ROOT);
    let t = Tape::read(Path::new(&mounted(tape)))?;
    let camera = OracleCamera::parse(camera)?;
    let car = t.get("car").ok_or("the tape names no car")?.to_string();
    // the tape has the desktop's folder: the page's is the same name under content/tracks
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
    // the oracle's levels (its root's cfg/audio.ini)
    let audio_ini = PathBuf::from(mounted("proof/cfg/audio.ini"));
    files().insert(&audio_ini.to_string_lossy(), AUDIO_INI.as_bytes().into());
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
    let output = if wav { raw::Output::WavNrt { file: PathBuf::from("/fmodoutput.wav"), rate: 48000, block: 800 } } else { raw::Output::NoSoundNrt { rate: 48000, block: 800 } };
    raw::load(&root, output)?;
    log::start_kept(None);
    let files = EngineFiles { content_root: root.clone(), audio_engine_ini: root.join("system/cfg/audio_engine.ini"), audio_ini };
    let info = CarInfo { unix_name: car.clone(), guid: 0, data_folder: root.join("content/cars").join(&car).join("data"), car_cameras_external_sound: Vec::new() };
    let track = match (&scene, &track_folder) {
        (Some(scene), Some(folder)) => Some((scene, if layout.is_empty() { folder.clone() } else { folder.join(&layout) })),
        _ => None,
    };
    let session = Session { files, master: 1.0, track, surfaces: &surfaces, car: info, camera: camera.view(), dt: 1.0 / 60.0 };
    let result = run_session(session, frames.iter().map(|(car, events, listener)| SessionFrame { car, events, listener: *listener }));
    let (_, kept) = log::finish_kept().ok_or("no log")?;
    result?;
    String::from_utf8(kept).map_err(|e| e.to_string())
}

/// Task 41b: the sound of a whole drive from the launcher's files, every car of it (AI too),
/// as the desktop's `rustyac.exe --replay <tape> --headless --audio-wav … --audio-log …
/// --audio-ini <the oracle's audio.ini> --camera <camera> --focus <focus>` gives it: the setup
/// the page makes of `race_text` / `assists_text` / `seed` (see `race::replay_race`), driven
/// with the inputs of the desktop's recording `tape`; the mix in FMOD's file system as
/// `/fmodoutput.wav`. Returns the FMOD call log.
#[wasm_bindgen]
pub fn race_sound_proof(race_text: &str, assists_text: Option<String>, seed: u32, tape: &[u8], camera: &str, focus: usize) -> Result<String, JsError> {
    race_proof(race_text, assists_text.as_deref(), seed, tape, camera, focus).map_err(|e| JsError::new(&e))
}

fn race_proof(race_text: &str, assists_text: Option<&str>, seed: u32, tape: &[u8], camera: &str, focus: usize) -> Result<String, String> {
    use rustyac_game::audio::{AudioOptions, GameAudio};
    let file = rustyac_game::input_file::InputFile::parse(tape)?;
    let (setup, _) = crate::race::setup_of(race_text, assists_text, seed)?;
    let audio_ini = PathBuf::from(mounted("proof/cfg/audio.ini"));
    files().insert(&audio_ini.to_string_lossy(), AUDIO_INI.as_bytes().into());
    let options = AudioOptions { wav: Some(PathBuf::from("/fmodoutput.wav")), log: Some(PathBuf::from("kept")), ini: Some(audio_ini), ..AudioOptions::default() };
    let audio = GameAudio::replay(&options, setup, &file.steps, camera, focus)?;
    let log = audio.finish_kept().ok_or("no call log")?;
    String::from_utf8(log).map_err(|e| e.to_string())
}
