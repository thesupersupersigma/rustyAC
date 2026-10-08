// SPDX-License-Identifier: GPL-3.0-or-later

//! The bank survey: every installed car and track through the port with the player's FMOD
//! 1.08.12 (no-sound output). For a car: is there a bank, does FMOD load it, does the sound
//! find its events. For a track: emitters, reverb zones, occluders, surface sounds.

use std::fmt::Write as _;
use std::path::Path;

use rustyac_audio::car::CarInfo;
use rustyac_audio::engine::{AudioEngine, EngineFiles};
use rustyac_audio::fmod::raw;
use rustyac_audio::sim::CarSound;
use rustyac_audio::track::{Scene, TrackAudio};

pub fn run(args: &crate::Args) -> Result<(), String> {
    let ac = crate::ac_folder(args);
    let repo = crate::repo_root();
    let root = std::path::absolute(&args.root).map_err(|e| e.to_string())?;
    // only the two ini files of the engine are needed from the small game folder
    std::fs::create_dir_all(root.join("cfg")).map_err(|e| e.to_string())?;
    std::fs::write(root.join("cfg/audio.ini"), crate::drive::AUDIO_INI).map_err(|e| e.to_string())?;
    raw::load(&ac, raw::Output::NoSoundNrt { rate: crate::drive::SAMPLE_RATE, block: crate::drive::BLOCK })?;
    let files = EngineFiles { content_root: ac.clone(), audio_engine_ini: ac.join("system/cfg/audio_engine.ini"), audio_ini: root.join("cfg/audio.ini") };
    let mut engine = AudioEngine::new(files)?;
    let mut text = format!("FMOD version found: {:#x} (the game requires 0x10812).\n\n", engine.version);

    // --- cars ---
    let mut cars: Vec<String> = std::fs::read_dir(ac.join("content/cars"))
        .map_err(|e| e.to_string())?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    cars.sort();
    if let Some(only) = &args.only {
        cars.retain(|c| c.contains(only.as_str()));
    }
    let mut rows = String::new();
    let (mut plays, mut no_bank, mut refused, mut partial) = (0, 0, 0, 0);
    let mut no_bank_names = Vec::new();
    for car in &cars {
        let folder = ac.join("content/cars").join(car);
        let bank = format!("content/cars/{car}/sfx/{car}.bank");
        let has_bank = ac.join(&bank).is_file();
        let own_guids = folder.join("sfx/GUIDs.txt").is_file();
        if !has_bank {
            no_bank += 1;
            no_bank_names.push(car.clone());
            continue;
        }
        let info = CarInfo { unix_name: car.clone(), guid: 0, data_folder: folder.join("data"), car_cameras_external_sound: Vec::new() };
        let result = match CarSound::new(&mut engine, info) {
            Ok(sound) => {
                let audio = sound.audio.expect("a car with a bank has a sound object");
                let loaded = engine.bank_loaded(&bank);
                let core = audio.core_events(&engine);
                let missing: Vec<&str> = core.iter().filter(|c| !c.1).map(|c| c.0).collect();
                let optional = audio.optional_events().join(", ");
                audio.destroy(&mut engine);
                if !loaded {
                    refused += 1;
                    "**no sound**: FMOD 1.08.12 does not load the bank (made with another FMOD Studio version, or not a bank)".to_string()
                } else if missing.is_empty() {
                    plays += 1;
                    format!("plays; optional events: {}", if optional.is_empty() { "none" } else { &optional })
                } else {
                    partial += 1;
                    format!("**partly**: the bank loads, but these events are not in it: {}; optional events: {}", missing.join(", "), if optional.is_empty() { "none" } else { &optional })
                }
            }
            Err(error) => {
                refused += 1;
                format!("**no sound**: {error}")
            }
        };
        let _ = writeln!(rows, "| `{car}` | {} | {result} |", if own_guids { "own" } else { "the game's" });
    }
    let _ = writeln!(
        text,
        "### Cars\n\n{} car folders: {plays} play, {partial} play partly, {refused} have a bank that gives no sound, {no_bank} have no bank (nothing to play: the game makes no sound object for them either).\n\n| Car | GUIDs.txt | Result |\n|---|---|---|\n{rows}",
        cars.len()
    );
    let _ = writeln!(text, "Without a bank (`content/cars/<car>/sfx/<car>.bank` is not there; DLC cars that are not installed have only their menu entry): {}\n", no_bank_names.iter().map(|n| format!("`{n}`")).collect::<Vec<_>>().join(", "));

    // --- tracks ---
    let mut rows = String::new();
    let entries = rustyac_physics::track::catalog::installed(&ac);
    for entry in &entries {
        if let Some(only) = &args.only {
            if !entry.track.contains(only.as_str()) {
                continue;
            }
        }
        let name = if entry.layout.is_empty() { entry.track.clone() } else { format!("{} / {}", entry.track, entry.layout) };
        if let Err(refusal) = rustyac_physics::track::catalog::check(entry) {
            let _ = writeln!(rows, "| `{name}` | - | - | - | - | not loaded by rustyAC: {refusal:?} |");
            continue;
        }
        let line = match track_row(&mut engine, &ac, &entry.folder, &entry.layout) {
            Ok(line) => line,
            Err(error) => format!("- | - | - | - | **{error}**"),
        };
        let _ = writeln!(rows, "| `{name}` | {line} |");
    }
    let _ = writeln!(
        text,
        "### Tracks\n\n| Track / layout | `AC_AUDIO_*` emitters | Reverb zones | Occluders | Surface sounds (pool of 12 each) | Result |\n|---|---|---|---|---|---|\n{rows}"
    );
    drop(engine);
    let path = args.out_file.clone().unwrap_or_else(|| repo.join("oracle/audio/survey.md"));
    std::fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
    std::fs::write(&path, &text).map_err(|e| e.to_string())?;
    println!("{} cars: {plays} play, {partial} partly, {refused} refused, {no_bank} without a bank; {} tracks / layouts", cars.len(), entries.len());
    println!("written to {}", path.display());
    Ok(())
}

fn track_row(engine: &mut AudioEngine, ac: &Path, folder: &Path, layout: &str) -> Result<String, String> {
    let scene = Scene::load(folder, layout)?;
    let data = if layout.is_empty() { folder.to_path_buf() } else { folder.join(layout) };
    let track = TrackAudio::new(engine, &scene, &data)?;
    let manager = rustyac_physics::track::surfaces::SurfacesManager::new(&ac.join("system/data/surfaces.ini"), &data.join("data/surfaces.ini"))?;
    let mut sounds = Vec::new();
    let mut unknown = Vec::new();
    for surface in manager.surfaces.values() {
        if surface.wav.is_empty() {
            continue;
        }
        let count = surface.wav.chars().count();
        let stem: String = surface.wav.chars().take(count.saturating_sub(4)).collect();
        if sounds.contains(&stem) {
            continue;
        }
        if !engine.has_event(&format!("event:/surfaces/{stem}")) {
            unknown.push(stem.clone());
        }
        sounds.push(stem);
    }
    let line = format!(
        "{} | {} | {} | {} | {} |",
        track.emitters(),
        track.reverbs(),
        track.occluders(),
        sounds.join(", "),
        if unknown.is_empty() { "plays".to_string() } else { format!("plays; no such surface sound in the game's bank (silent, as in the game): {}", unknown.join(", ")) }
    );
    track.destroy(engine);
    Ok(line.trim_end_matches(" |").to_string())
}
