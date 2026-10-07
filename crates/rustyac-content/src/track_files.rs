// SPDX-License-Identifier: MIT OR Apache-2.0

//! Which files make up a track.
//!
//! `content/tracks/<track>/` holds either one `<track>.kn5`, or a `models.ini` that lists
//! several (`[MODEL_0]`, `[MODEL_1]` ... each with `FILE`, `POSITION`, `ROTATION`). A track
//! with layouts has `models_<layout>.ini` and a folder `<layout>/` with its own `data` and
//! `ai`; without a layout those two folders sit in the track folder itself.

use std::path::{Path, PathBuf};

/// One `[MODEL_n]` of a models file.
#[derive(Clone, Debug, PartialEq)]
pub struct ModelEntry {
    pub file: PathBuf,
    pub position: [f32; 3],
    /// Euler angles as written in the file.
    pub rotation: [f32; 3],
}

#[derive(Clone, Debug, PartialEq)]
pub struct TrackFiles {
    /// `content/tracks/<track>`
    pub folder: PathBuf,
    /// The folder's name: the track's name in the game.
    pub name: String,
    /// The layout, or empty.
    pub layout: String,
    /// The models in the order the game loads them.
    pub models: Vec<ModelEntry>,
    /// `<folder>[/<layout>]/data`
    pub data: PathBuf,
    /// `<folder>[/<layout>]/ai`
    pub ai: PathBuf,
}

fn three(text: &str) -> [f32; 3] {
    let mut out = [0.0f32; 3];
    for (slot, part) in out.iter_mut().zip(text.split(',')) {
        *slot = part.trim().parse().unwrap_or(0.0);
    }
    out
}

/// The `[MODEL_n]` sections of a models file, n = 0, 1, 2 ... up to the first missing one.
fn read_models(ini: &Path, folder: &Path) -> Result<Vec<ModelEntry>, String> {
    let text = std::fs::read(ini).map_err(|e| format!("{}: {e}", ini.display()))?;
    let text = String::from_utf8_lossy(&text);
    let mut sections: Vec<(String, Vec<(String, String)>)> = Vec::new();
    for line in text.lines() {
        let line = line.split(';').next().unwrap_or("").trim();
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            sections.push((name.to_string(), Vec::new()));
        } else if let (Some((key, value)), Some(section)) = (line.split_once('='), sections.last_mut()) {
            section.1.push((key.trim().to_string(), value.trim().to_string()));
        }
    }
    let mut models = Vec::new();
    for n in 0.. {
        let name = format!("MODEL_{n}");
        let Some((_, keys)) = sections.iter().find(|(section, _)| *section == name) else { break };
        let get = |key: &str| keys.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str()).unwrap_or("");
        models.push(ModelEntry { file: folder.join(get("FILE")), position: three(get("POSITION")), rotation: three(get("ROTATION")) });
    }
    Ok(models)
}

impl TrackFiles {
    /// `folder` is `content/tracks/<track>`; `layout` is empty for a track without layouts.
    pub fn find(folder: &Path, layout: &str) -> Result<TrackFiles, String> {
        if !folder.is_dir() {
            return Err(format!("the track folder {} does not exist", folder.display()));
        }
        let name = folder.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let ini = if layout.is_empty() { folder.join("models.ini") } else { folder.join(format!("models_{layout}.ini")) };
        let models = if ini.is_file() {
            read_models(&ini, folder)?
        } else if layout.is_empty() {
            vec![ModelEntry { file: folder.join(format!("{name}.kn5")), position: [0.0; 3], rotation: [0.0; 3] }]
        } else {
            return Err(format!("the layout {layout:?} needs {}", ini.display()));
        };
        if models.is_empty() {
            return Err(format!("{} lists no model", ini.display()));
        }
        for model in &models {
            if !model.file.is_file() {
                return Err(format!("the track model {} is missing", model.file.display()));
            }
        }
        let base = if layout.is_empty() { folder.to_path_buf() } else { folder.join(layout) };
        Ok(TrackFiles { folder: folder.to_path_buf(), name, layout: layout.to_string(), models, data: base.join("data"), ai: base.join("ai") })
    }
}
