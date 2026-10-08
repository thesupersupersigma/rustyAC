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

fn three(text: &[u8]) -> [f32; 3] {
    let mut out = [0.0f32; 3];
    for (slot, part) in out.iter_mut().zip(text.split(|b| *b == b',')) {
        *slot = std::str::from_utf8(part.trim_ascii()).ok().and_then(|number| number.parse().ok()).unwrap_or(0.0);
    }
    out
}

/// A file name out of an ini as a path. The file's bytes need not be UTF-8, but a path on
/// Windows is text: bytes that are not UTF-8 are taken one byte per character (Latin-1), which
/// loses nothing.
fn file_name(bytes: &[u8]) -> PathBuf {
    match std::str::from_utf8(bytes) {
        Ok(text) => PathBuf::from(text),
        Err(_) => PathBuf::from(bytes.iter().map(|b| *b as char).collect::<String>()),
    }
}

/// The `[MODEL_n]` sections of a models file, n = 0, 1, 2 ... up to the first missing one.
fn read_models(ini: &Path, folder: &Path) -> Result<Vec<ModelEntry>, String> {
    // the file is read as bytes: sections, keys and values are never decoded
    let text = std::fs::read(ini).map_err(|e| format!("{}: {e}", ini.display()))?;
    let text = text.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(&text);
    type Section<'a> = (&'a [u8], Vec<(&'a [u8], &'a [u8])>);
    let mut sections: Vec<Section> = Vec::new();
    for line in text.split(|b| *b == b'\n') {
        let line = line.split(|b| *b == b';').next().unwrap_or(&[]).trim_ascii();
        if let Some(name) = line.strip_prefix(b"[").and_then(|l| l.strip_suffix(b"]")) {
            sections.push((name, Vec::new()));
        } else if let (Some(at), Some(section)) = (line.iter().position(|b| *b == b'='), sections.last_mut()) {
            section.1.push((line[..at].trim_ascii(), line[at + 1..].trim_ascii()));
        }
    }
    let mut models = Vec::new();
    for n in 0.. {
        let name = format!("MODEL_{n}");
        let Some((_, keys)) = sections.iter().find(|(section, _)| *section == name.as_bytes()) else { break };
        let get = |key: &str| keys.iter().find(|(k, _)| *k == key.as_bytes()).map(|(_, v)| *v).unwrap_or(&[]);
        models.push(ModelEntry { file: folder.join(file_name(get("FILE"))), position: three(get("POSITION")), rotation: three(get("ROTATION")) });
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_models_file_is_read_as_bytes() {
        assert_eq!(three(b" 1.5, -2 ,x"), [1.5, -2.0, 0.0]);
        assert_eq!(file_name(b"spa.kn5"), PathBuf::from("spa.kn5"));
        // Latin-1 "é": not UTF-8, one byte per character
        assert_eq!(file_name(b"caf\xe9.kn5"), PathBuf::from("caf\u{e9}.kn5"));
    }
}
