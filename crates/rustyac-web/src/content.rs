// SPDX-License-Identifier: GPL-3.0-or-later

//! Cars and tracks for the browser build: what is there, what loads, which files a drive
//! needs.
//!
//! Everything goes through `rustyac_content::vfs`, so the same code answers for a folder on
//! disk (the pack tool, the tests) and for the page's files in memory. The refusals are the
//! desktop's own: a car is built by the same loader, a track is checked by the same
//! `track::catalog`.

use std::path::{Path, PathBuf};

use rustyac_content::track_files::TrackFiles;
use rustyac_content::vfs::{self, PathExt};
use rustyac_game::input_file::SimSetup;
use rustyac_game::sim::{self, GameSim, NobodySource};
use rustyac_physics::track::catalog;

use crate::model::Placement;

/// A car folder of `content/cars`.
#[derive(Clone, Debug, PartialEq)]
pub struct CarEntry {
    /// The folder's name: the game's name of the car.
    pub id: String,
    /// The menu's name (`ui/ui_car.json`), or the folder's.
    pub name: String,
    /// Why it does not load, in the loader's words.
    pub refused: Option<String>,
}

/// A track or one layout of a track.
#[derive(Clone, Debug, PartialEq)]
pub struct TrackEntry {
    pub track: String,
    pub layout: String,
    pub name: String,
    pub refused: Option<String>,
}

/// The text of `"key": "value"` in one of the game's ui files, read leniently (they are
/// often not valid json).
fn json_string(text: &str, key: &str) -> Option<String> {
    let at = text.find(&format!("\"{key}\""))?;
    let rest = &text[at + key.len() + 2..];
    let rest = rest.trim_start().strip_prefix(':')?.trim_start().strip_prefix('"')?;
    let mut out = String::new();
    let mut chars = rest.chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => return Some(out),
            '\\' => out.extend(chars.next()),
            c => out.push(c),
        }
    }
    None
}

fn car_folder(root: &Path, car: &str) -> PathBuf {
    root.join("content").join("cars").join(car)
}

fn track_folder(root: &Path, track: &str) -> PathBuf {
    root.join("content").join("tracks").join(track)
}

/// The folder names under `content/cars` that hold a car's data, sorted.
pub fn car_ids(root: &Path) -> Vec<String> {
    let cars = root.join("content").join("cars");
    let mut ids: Vec<String> = vfs::read_dir(&cars)
        .unwrap_or_default()
        .into_iter()
        .filter(|(name, is_dir)| *is_dir && (cars.join(name).join("data.acd").vfs_is_file() || cars.join(name).join("data").join("car.ini").vfs_is_file()))
        .map(|(name, _)| name)
        .collect();
    ids.sort_by_key(|id| id.to_ascii_lowercase());
    ids
}

/// The menu's name of a car, when its `ui/ui_car.json` is at hand.
pub fn car_name(root: &Path, car: &str) -> String {
    vfs::read(&car_folder(root, car).join("ui").join("ui_car.json"))
        .ok()
        .and_then(|bytes| json_string(&String::from_utf8_lossy(&bytes), "name"))
        .filter(|name| !name.trim().is_empty())
        .unwrap_or_else(|| car.to_string())
}

/// Does the car load? It is built on the flat road by the loader that will build it for the
/// drive, and thrown away.
pub fn check_car(car: &str) -> Result<(), String> {
    let setup = SimSetup { car: car.to_string(), ..SimSetup::default() };
    GameSim::new(setup, Box::new(NobodySource)).map(|_| ())
}

/// Every car folder with its name and, when `check`, whether it loads.
pub fn cars(root: &Path, check: bool) -> Vec<CarEntry> {
    car_ids(root)
        .into_iter()
        .map(|id| CarEntry { name: car_name(root, &id), refused: if check { check_car(&id).err() } else { None }, id })
        .collect()
}

/// Every track and layout under `content/tracks`, checked without opening the models (the
/// full check runs when one is loaded).
pub fn tracks(root: &Path) -> Vec<TrackEntry> {
    catalog::installed(root)
        .into_iter()
        .map(|entry| TrackEntry {
            refused: catalog::check_without_models(&entry).err().map(|refusal| refusal.to_string()),
            name: if entry.ui_name.is_empty() { entry.track.clone() } else { entry.ui_name.clone() },
            track: entry.track,
            layout: entry.layout,
        })
        .collect()
}

/// The session of a browser drive: what `rustyac.exe --no-race-ini` gives (26 C air, 30 C
/// road, grip 100 %, no wind), with the automatic clutch a pad or the keyboard force and the
/// automatic gearbox as asked. `spawn`: `pit` (the pit box; where R puts the car back),
/// `hotlap` or `start`.
pub fn setup(car: &str, track: &str, layout: &str, auto_shifter: bool, spawn: &str) -> SimSetup {
    SimSetup {
        car: car.to_string(),
        auto_clutch: true,
        auto_shifter,
        track: track.to_string(),
        layout: layout.to_string(),
        track_objects: true,
        session_transfer: true,
        spawn: spawn.to_string(),
        session_starts_at_spawn: true,
        drs_zones: true,
        ..SimSetup::default()
    }
}

/// Every file below a folder (not into `skip`), as paths.
fn files_below(folder: &Path, skip: &[&str], depth: usize, out: &mut Vec<PathBuf>) {
    for (name, is_dir) in vfs::read_dir(folder).unwrap_or_default() {
        let path = folder.join(&name);
        if !is_dir {
            out.push(path);
        } else if depth > 0 && !skip.iter().any(|s| name.eq_ignore_ascii_case(s)) {
            files_below(&path, skip, depth - 1, out);
        }
    }
}

/// The 3D model files of a drive: the car's first level of detail, and the track's models
/// with where its models file puts them.
pub fn model_files(root: &Path, car: &str, track: &str, layout: &str) -> (Option<PathBuf>, Vec<(PathBuf, Placement)>) {
    let folder = car_folder(root, car);
    let data = folder.join("data");
    let car_model = sim::find_car_model(car, &data);
    let track_models = TrackFiles::find_lenient(&track_folder(root, track), layout)
        .map(|files| files.models.into_iter().filter(|m| m.file.vfs_is_file()).map(|m| (m.file, Placement { position: m.position, rotation: m.rotation })).collect())
        .unwrap_or_default();
    (car_model, track_models)
}

/// The files a drive of this car on this track reads, as far as can be told from what is at
/// hand: the car's data (the archive as it is, or the plain folder), its collider and model,
/// the track's models file, models, `data` and `ai` folders, the game's own tables under
/// `system`. A second call after the first files are there can name more (the model a
/// `lods.ini` or a `models.ini` points to).
pub fn files_of_drive(root: &Path, car: &str, track: &str, layout: &str) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let folder = car_folder(root, car);
    if folder.join("data.acd").vfs_is_file() {
        out.push(folder.join("data.acd"));
    } else {
        files_below(&folder.join("data"), &[], 0, &mut out);
    }
    out.push(folder.join("collider.kn5"));
    out.push(folder.join("ui").join("ui_car.json"));
    let track_base = track_folder(root, track);
    let layout_base = if layout.is_empty() { track_base.clone() } else { track_base.join(layout) };
    out.push(if layout.is_empty() { track_base.join("models.ini") } else { track_base.join(format!("models_{layout}.ini")) });
    for base in [&track_base, &layout_base] {
        files_below(&base.join("data"), &[], 0, &mut out);
        files_below(&base.join("ai"), &[], 0, &mut out);
        out.push(base.join("ui").join("ui_track.json"));
    }
    out.push(track_base.join("ui").join(layout).join("ui_track.json"));
    let (car_model, track_models) = model_files(root, car, track, layout);
    out.extend(car_model);
    out.extend(track_models.into_iter().map(|(file, _)| file));
    files_below(&root.join("system").join("data"), &[], 0, &mut out);
    out.retain(|path| path.vfs_is_file());
    out.sort();
    out.dedup();
    out
}

/// Loads the drive once with the file log on and returns every file that was read: what a
/// preview pack has to hold. (`files_of_drive` is the page's guess before a load; this is the
/// proof after one.)
pub fn files_read_by_drive(root: &Path, car: &str, track: &str, layout: &str) -> Result<Vec<PathBuf>, String> {
    vfs::set_log(true);
    let result = (|| -> Result<(), String> {
        let sim = GameSim::new(setup(car, track, layout, true, "pit"), Box::new(NobodySource))?;
        // and what the picture reads: the car's shape, the model files (whole)
        let info = sim.car_info();
        let _ = rustyac_game::render::scene::CarShape::of(&info);
        let (car_model, track_models) = model_files(root, car, track, layout);
        for file in car_model.into_iter().chain(track_models.into_iter().map(|(file, _)| file)) {
            let _ = file.vfs_is_file();
        }
        let _ = car_name(root, car);
        Ok(())
    })();
    let log = vfs::take_log();
    vfs::set_log(false);
    result?;
    let mut files: Vec<PathBuf> = log.into_iter().filter(|(_, found)| *found).map(|(path, _)| path).collect();
    // a data file read out of the archive is the archive
    files.sort();
    files.dedup();
    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_menu_name_is_read_leniently() {
        assert_eq!(json_string("{\n \"name\": \"BMW Z4 GT3\",\n \"brand\": \"BMW\"}", "name").as_deref(), Some("BMW Z4 GT3"));
        assert_eq!(json_string("{\"name\" : \"A \\\"B\\\"\"}", "name").as_deref(), Some("A \"B\""));
        assert_eq!(json_string("{}", "name"), None);
    }
}
