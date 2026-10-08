// SPDX-License-Identifier: GPL-3.0-or-later

//! What both sides of the audio oracle are run on: a recorded drive (the audio tape of
//! `car_oracle run --audio-tape`), a listener pose per frame, and the small game folder the
//! game's code reads its ini and GUID files from.

use std::path::{Path, PathBuf};

use rustyac_audio::car::CarFrame;
use rustyac_audio::engine::{Mat, Vec3};
use rustyac_audio::tape::Tape;

/// The frame time both sides are given: the tape's frames are 1/60 s apart.
pub const DT: f32 = 1.0 / 60.0;
pub const SAMPLE_RATE: i32 = 48000;
/// One mixer block per frame: 48000 / 60 samples.
pub const BLOCK: u32 = 800;

/// The listener of a drive: `rustyac.exe --audio-oracle-camera` has the same three.
pub use rustyac_audio::tape::OracleCamera as Camera;

/// One frame of a drive.
pub struct Frame {
    pub state: Vec<u8>,
    pub events: Vec<[u8; 0x48]>,
    pub car: CarFrame,
    pub listener: (Mat, Vec3),
}

/// A drive and everything about its session.
pub struct Drive {
    pub name: String,
    pub car: String,
    pub track_folder: Option<PathBuf>,
    pub layout: String,
    pub camera: Camera,
    pub frames: Vec<Frame>,
}

impl Drive {
    pub fn load(tape: &Path, camera: Camera, limit: Option<usize>) -> Result<Drive, String> {
        let t = Tape::read(tape)?;
        let car = t.get("car").ok_or("the tape names no car")?.to_string();
        let track_folder = t.get("track_folder").map(PathBuf::from);
        let layout = t.get("track_layout").unwrap_or("").to_string();
        let name = t.get("scenario").unwrap_or("drive").to_string();
        let mut frames = Vec::new();
        let mut first: Option<CarFrame> = None;
        for f in t.frames.into_iter().take(limit.unwrap_or(usize::MAX)) {
            let mut frame = CarFrame::from_physics_state(&f.state);
            frame.tc_in_action = f.tc_in_action;
            let first = first.get_or_insert_with(|| frame.clone());
            let listener = camera.listener(&frame, first);
            frames.push(Frame { state: f.state, events: f.events, car: frame, listener });
        }
        Ok(Drive { name, car, track_folder, layout, camera, frames })
    }

    /// The track's name and where its `data` folder is under `content/tracks`.
    pub fn track_data_relative(&self) -> Option<String> {
        let folder = self.track_folder.as_ref()?;
        let name = folder.file_name()?.to_string_lossy().into_owned();
        Some(if self.layout.is_empty() { format!("content/tracks/{name}") } else { format!("content/tracks/{name}/{}", self.layout) })
    }
}

fn copy(from: &Path, to: &Path) -> Result<(), String> {
    std::fs::create_dir_all(to.parent().unwrap()).map_err(|e| e.to_string())?;
    std::fs::copy(from, to).map_err(|e| format!("{} -> {}: {e}", from.display(), to.display()))?;
    Ok(())
}

/// The levels both sides run with (the game reads them from Documents `cfg/audio.ini`).
pub const AUDIO_INI: &str = "[LEVELS]\nBRAKES=0.8\nDIRT_BOTTOM=1\nENGINE=1\nMASTER=1\nOPPONENTS=0.8\nSURFACES=0.9\nTYRES=1\nWIND=0.3\nTRANSMISSION=0.33333334\n\n[SETTINGS]\nDRIVER_NAME=no such device\n\n[SKIDS]\nENTRY_POINT=100\n";

/// Builds the small game folder: the ini and GUID files the game's sound code reads by
/// relative path. The banks stay where they are (the FMOD layer finds them in the game folder).
pub fn prepare_root(root: &Path, repo: &Path, ac: &Path, drive: &Drive) -> Result<(), String> {
    if root.join("acs.exe").is_file() || root.join("AssettoCorsa.exe").is_file() {
        return Err(format!("{} looks like a game folder: the oracle's root has to be a scratch folder of its own", root.display()));
    }
    let io = |e: std::io::Error| e.to_string();
    std::fs::create_dir_all(root.join("cfg")).map_err(io)?;
    std::fs::write(root.join("cfg/audio.ini"), AUDIO_INI).map_err(io)?;
    copy(&ac.join("system/cfg/audio_engine.ini"), &root.join("system/cfg/audio_engine.ini"))?;
    copy(&ac.join("content/sfx/GUIDs.txt"), &root.join("content/sfx/GUIDs.txt"))?;
    let car = &drive.car;
    let car_root = root.join("content/cars").join(car);
    let _ = std::fs::remove_dir_all(&car_root);
    let car_guids = ac.join("content/cars").join(car).join("sfx/GUIDs.txt");
    if car_guids.is_file() {
        copy(&car_guids, &car_root.join("sfx/GUIDs.txt"))?;
    }
    // the car's data: the extracted folder of the repository, else out of the game's data.acd
    for name in ["engine.ini", "sounds.ini"] {
        let extracted = repo.join("cardata").join(car).join(name);
        let to = car_root.join("data").join(name);
        if extracted.is_file() {
            copy(&extracted, &to)?;
        } else if let Some(bytes) = rustyac_physics::data::read(&ac.join("content/cars").join(car).join("data").join(name))? {
            std::fs::create_dir_all(to.parent().unwrap()).map_err(io)?;
            std::fs::write(&to, bytes).map_err(io)?;
        }
    }
    if let (Some(folder), Some(relative)) = (&drive.track_folder, drive.track_data_relative()) {
        let from = if drive.layout.is_empty() { folder.clone() } else { folder.join(&drive.layout) };
        let to = root.join(&relative).join("data/audio_sources.ini");
        let _ = std::fs::remove_file(&to);
        let source = from.join("data/audio_sources.ini");
        if source.is_file() {
            copy(&source, &to)?;
        }
    }
    Ok(())
}

/// The surfaces of the drive's track as `Sim::loadTrack` walks them: (KEY, WAV).
pub fn surfaces(ac: &Path, drive: &Drive) -> Result<Vec<(String, String)>, String> {
    let Some(folder) = &drive.track_folder else {
        return Ok(Vec::new());
    };
    let data = if drive.layout.is_empty() { folder.join("data") } else { folder.join(&drive.layout).join("data") };
    let manager = rustyac_physics::track::surfaces::SurfacesManager::new(&ac.join("system/data/surfaces.ini"), &data.join("surfaces.ini"))?;
    Ok(manager.surfaces.iter().map(|(key, surface)| (key.clone(), surface.wav.clone())).collect())
}

/// The paths `Sim::loadTrack` hands to `AudioEngine::addCache`, in its order.
pub fn surface_cache_paths(surfaces: &[(String, String)]) -> Vec<String> {
    let mut sorted: Vec<&(String, String)> = surfaces.iter().collect();
    sorted.sort_by(|a, b| a.0.encode_utf16().cmp(b.0.encode_utf16()));
    let mut out = Vec::new();
    for (_, wav) in sorted {
        if !wav.is_empty() {
            let count = wav.chars().count();
            out.push(format!("event:/surfaces/{}", wav.chars().take(count.saturating_sub(4)).collect::<String>()));
        }
    }
    out
}
