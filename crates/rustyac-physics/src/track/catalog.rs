// SPDX-License-Identifier: GPL-3.0-or-later

//! Which tracks and layouts are installed, finding one by name, and saying clearly why a
//! track cannot be driven (not installed, made for Custom Shaders Patch only, encrypted).
//!
//! The game itself has none of this: its launcher writes a folder name and a layout into
//! `race.ini` and `acs.exe` loads what is there or stops. Nothing here is read by the physics.

use std::path::{Path, PathBuf};

use rustyac_content::kn5::Kn5;
use rustyac_content::vfs::{self, PathExt};

use super::surfaces::SurfacesManager;

/// Who made a track or layout.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Maker {
    Kunos,
    Mod,
}

/// One drivable thing of `content/tracks`: a track without layouts, or one layout of a track.
#[derive(Clone, Debug, PartialEq)]
pub struct TrackEntry {
    /// The folder's name: the game's name of the track.
    pub track: String,
    /// The layout (`CONFIG_TRACK`), empty for a track without layouts.
    pub layout: String,
    /// `content/tracks/<track>`
    pub folder: PathBuf,
    /// `"name"` of its `ui_track.json`, or empty.
    pub ui_name: String,
    pub maker: Maker,
}

/// Why a track or layout is not loaded.
#[derive(Clone, Debug, PartialEq)]
pub enum Refusal {
    /// Only its menu entry is there (a DLC that is not owned, or a layout whose track is not).
    NotInstalled(String),
    /// Its files only work with Custom Shaders Patch.
    CspOnly(String),
    /// A model file is encrypted or cannot be read.
    Encrypted(String),
    /// A file of the track is one plain Assetto Corsa cannot read (for another reason).
    Unreadable(String),
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Refusal::NotInstalled(why) => write!(f, "not installed: {why}"),
            Refusal::CspOnly(why) => write!(f, "made for Custom Shaders Patch only (rustyAC is plain Assetto Corsa): {why}"),
            Refusal::Encrypted(why) => write!(f, "encrypted or unreadable (rustyAC never decrypts content): {why}"),
            Refusal::Unreadable(why) => write!(f, "acs.exe could not load it: {why}"),
        }
    }
}

/// The text of `"key": "value"` in a json file, read leniently (the game's ui files are often
/// not valid json).
fn json_string(text: &str, key: &str) -> Option<String> {
    let at = text.find(&format!("\"{key}\""))?;
    let rest = &text[at + key.len() + 2..];
    let rest = rest.trim_start().strip_prefix(':')?.trim_start();
    let rest = rest.strip_prefix('"')?;
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

fn read_text(path: &Path) -> Option<String> {
    Some(String::from_utf8_lossy(&vfs::read(path).ok()?).into_owned())
}

/// The entry of a track folder and layout, whether it is installed or not.
pub fn entry_of(folder: &Path, track: &str, layout: &str) -> TrackEntry {
    entry(folder, track, layout)
}

fn entry(folder: &Path, track: &str, layout: &str) -> TrackEntry {
    let ui = if layout.is_empty() { folder.join("ui") } else { folder.join("ui").join(layout) };
    // a DLC that is not owned has only `dlc_ui_track.json`
    let json = read_text(&ui.join("ui_track.json")).or_else(|| read_text(&ui.join("dlc_ui_track.json"))).unwrap_or_default();
    let author = json_string(&json, "author").unwrap_or_default();
    let base = if layout.is_empty() { folder.to_path_buf() } else { folder.join(layout) };
    // a layout with Custom Shaders Patch's own settings folder was not made by Kunos
    let kunos = (author.is_empty() || author.to_ascii_lowercase().contains("kunos")) && !base.join("extension").vfs_is_dir() && !folder.join("extension").vfs_is_dir();
    TrackEntry {
        track: track.to_string(),
        layout: layout.to_string(),
        folder: folder.to_path_buf(),
        ui_name: json_string(&json, "name").unwrap_or_default(),
        maker: if kunos { Maker::Kunos } else { Maker::Mod },
    }
}

/// The layouts of a track folder, "" for the track itself: what has a menu entry
/// (`ui/ui_track.json`, `ui/<layout>/ui_track.json`) or a models file (`models.ini`,
/// `models_<layout>.ini`, `<track>.kn5` for a track without either).
pub fn layouts_of(folder: &Path) -> Vec<String> {
    let name = folder.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let mut layouts: Vec<String> = Vec::new();
    let mut add = |layout: String| {
        if !layouts.contains(&layout) {
            layouts.push(layout);
        }
    };
    let mut named: Vec<String> = Vec::new();
    let ui = folder.join("ui");
    if let Ok(entries) = vfs::read_dir(&ui) {
        for (entry, _) in entries {
            if ui.join(&entry).join("ui_track.json").vfs_is_file() || ui.join(&entry).join("dlc_ui_track.json").vfs_is_file() {
                named.push(entry);
            }
        }
    }
    if let Ok(entries) = vfs::read_dir(folder) {
        for (file, _) in entries {
            if let Some(layout) = file.strip_prefix("models_").and_then(|f| f.strip_suffix(".ini")) {
                if !layout.is_empty() {
                    named.push(layout.to_string());
                }
            }
        }
    }
    named.sort();
    let has_base_ui = folder.join("ui").join("ui_track.json").vfs_is_file() || folder.join("ui").join("dlc_ui_track.json").vfs_is_file();
    let has_base_models = folder.join("models.ini").vfs_is_file() || (named.is_empty() && folder.join(format!("{name}.kn5")).vfs_is_file());
    if has_base_ui || has_base_models {
        add(String::new());
    }
    for layout in named {
        add(layout);
    }
    layouts
}

/// Every track and layout under `<root>/content/tracks`, by folder name.
pub fn installed(root: &Path) -> Vec<TrackEntry> {
    let tracks = root.join("content").join("tracks");
    let mut folders: Vec<PathBuf> = vfs::read_dir(&tracks).map(|d| d.into_iter().filter(|(_, is_dir)| *is_dir).map(|(name, _)| tracks.join(name)).collect()).unwrap_or_default();
    folders.sort();
    let mut out = Vec::new();
    for folder in folders {
        let track = folder.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        for layout in layouts_of(&folder) {
            out.push(entry(&folder, &track, &layout));
        }
    }
    out
}

/// The models file of a layout and the model files it names (`[MODEL_n] FILE` for n = 0, 1,
/// 2 ... up to the first number that is missing, as the game reads it).
fn model_files(folder: &Path, layout: &str) -> Result<Vec<PathBuf>, Refusal> {
    let name = folder.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let ini = if layout.is_empty() { folder.join("models.ini") } else { folder.join(format!("models_{layout}.ini")) };
    if !ini.vfs_is_file() {
        let single = folder.join(format!("{name}.kn5"));
        if layout.is_empty() && single.vfs_is_file() {
            return Ok(vec![single]);
        }
        return Err(Refusal::NotInstalled(if layout.is_empty() {
            format!("the folder has neither models.ini nor {name}.kn5: only the menu entry of a track that is not owned (a DLC)")
        } else {
            format!("models_{layout}.ini is missing: only the menu entry of a layout that is not owned (a DLC)")
        }));
    }
    let text = read_text(&ini).unwrap_or_default();
    let mut found: Vec<(u32, PathBuf)> = Vec::new();
    let mut section: Option<u32> = None;
    for line in text.lines() {
        let line = line.split(';').next().unwrap_or("").trim();
        if line.starts_with('[') {
            section = line.strip_prefix("[MODEL_").and_then(|l| l.strip_suffix(']')).and_then(|n| n.parse().ok());
        } else if let Some(number) = section {
            if let Some(file) = line.strip_prefix("FILE").map(str::trim_start).and_then(|l| l.strip_prefix('=')) {
                if !found.iter().any(|(n, _)| *n == number) {
                    found.push((number, folder.join(file.trim())));
                }
            }
        }
    }
    let mut files = Vec::new();
    for number in 0.. {
        match found.iter().find(|(n, _)| *n == number) {
            Some((_, file)) => files.push(file.clone()),
            None => break,
        }
    }
    Ok(files)
}

/// Can this track / layout be loaded by plain Assetto Corsa, and so by rustyAC?
pub fn check(entry: &TrackEntry) -> Result<(), Refusal> {
    check_with(entry, true)
}

/// [`check`] without opening the model files: what can be said from the small files alone.
/// For a list of tracks where the models (hundreds of megabytes) are not at hand yet, as in
/// the browser build; the full [`check`] runs when one is loaded.
pub fn check_without_models(entry: &TrackEntry) -> Result<(), Refusal> {
    check_with(entry, false)
}

fn check_with(entry: &TrackEntry, open_models: bool) -> Result<(), Refusal> {
    let files = model_files(&entry.folder, &entry.layout)?;
    let missing: Vec<String> = files.iter().filter(|f| !f.vfs_is_file()).map(|f| f.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()).collect();
    if !files.is_empty() && missing.len() == files.len() {
        return Err(Refusal::NotInstalled(format!("none of its {} model files is there ({} ...): the track it is a layout of is not installed", files.len(), missing[0])));
    }
    if files.is_empty() {
        return Err(Refusal::NotInstalled("its models file names no model".to_string()));
    }
    // a model the reader cannot make sense of: damaged, or encrypted by its maker
    for file in files.iter().filter(|f| open_models && f.vfs_is_file()) {
        match Kn5::open(file) {
            Err(e) => return Err(Refusal::Encrypted(format!("{}: {e}", file.display()))),
            Ok(kn5) => {
                if let Some(marker) = encryption_marker(&kn5) {
                    return Err(Refusal::Encrypted(format!("{}: {marker}", file.display())));
                }
            }
        }
    }
    let base = if entry.layout.is_empty() { entry.folder.clone() } else { entry.folder.join(&entry.layout) };
    // a track whose data is packed into an archive keeps it from being read: it stays unread
    if let Some(archive) = packed_data(&base) {
        return Err(Refusal::Encrypted(format!("its data is packed in {} (a track's data folder is plain files in Assetto Corsa)", archive.display())));
    }
    // surfaces.ini with values only Custom Shaders Patch understands: acs.exe stops on them
    let surfaces = base.join("data").join("surfaces.ini");
    if surfaces.vfs_is_file() {
        // the game's own table is not needed to see whether the track's file can be read
        if let Err(message) = SurfacesManager::new(Path::new(""), &surfaces) {
            // without the long path in front
            let short = message.rsplit("surfaces.ini: ").next().unwrap_or(&message).to_string();
            // `extended-...` is how Custom Shaders Patch's extended physics marks a value
            return Err(if short.contains("extended") {
                Refusal::CspOnly(format!("acs.exe cannot read its data/surfaces.ini ({short}) and would stop there"))
            } else {
                Refusal::Unreadable(format!("its data/surfaces.ini has a value that is not a number ({short})"))
            });
        }
    }
    Ok(())
}

/// `<layout folder>/data.acd`, if there is one: an archive in the place of the plain `data`
/// folder. Assetto Corsa's tracks have none; one that has is refused and never opened.
pub fn packed_data(base: &Path) -> Option<PathBuf> {
    let archive = base.join("data.acd");
    archive.vfs_is_file().then_some(archive)
}

/// A name in a kn5 that says "encrypted". This is a guess: no encrypted track is installed
/// on the PC this was written on, so the names are the obvious ones and nothing more. A kn5
/// the reader cannot parse at all is refused in any case.
fn encryption_marker(kn5: &Kn5) -> Option<String> {
    let marked = |name: &str| {
        let lower = name.to_ascii_lowercase();
        lower.contains("__encrypt") || lower.contains("encrypted__") || lower.starts_with("__ac_shaders_patch")
    };
    if let Some(texture) = kn5.textures.iter().find(|t| marked(&t.name.display())) {
        return Some(format!("it carries the marker texture {:?}", texture.name.display()));
    }
    if let Some(node) = kn5.nodes.iter().find(|n| marked(&n.name.display())) {
        return Some(format!("it carries the marker node {:?}", node.name.display()));
    }
    None
}

/// What is worth knowing about an entry that loads: extras that are ignored.
pub fn notes(entry: &TrackEntry) -> Vec<String> {
    let mut notes = Vec::new();
    let base = if entry.layout.is_empty() { entry.folder.clone() } else { entry.folder.join(&entry.layout) };
    if base.join("extension").vfs_is_dir() || entry.folder.join("extension").vfs_is_dir() {
        notes.push("has a CSP `extension` folder (ignored)".to_string());
    }
    let has_patch = vfs::read_dir(&entry.folder).map(|d| d.iter().any(|(name, _)| name.ends_with(".vao-patch"))).unwrap_or(false);
    if has_patch {
        notes.push("CSP `.vao-patch` files (ignored)".to_string());
    }
    notes
}

/// Lower case, letters and digits only: how names are compared.
fn plain(name: &str) -> String {
    name.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect()
}

/// A track asked for by name and, maybe, layout.
#[derive(Clone, Debug, PartialEq)]
pub struct Found {
    pub entry: TrackEntry,
    /// What was assumed on the way, for the console.
    pub notes: Vec<String>,
}

/// Finds a track by what the user typed: a folder path, a folder name under `content/tracks`
/// (`ks_laguna_seca`), the name the game's menu shows (`Laguna Seca`), or a part of either
/// that fits one track only (`laguna`). `layout` is the layout asked for, if any;
/// `preferred_layout` is what to take when none was asked for and it exists (race.ini's
/// `CONFIG_TRACK` when its `TRACK` is this track).
///
/// The answer is checked: a track that is not installed, is made for Custom Shaders Patch
/// only or is encrypted comes back as an error that says so.
pub fn find(root: Option<&Path>, name: &str, layout: Option<&str>, preferred_layout: &dyn Fn(&str) -> Option<String>) -> Result<Found, String> {
    let mut notes = Vec::new();
    let direct = PathBuf::from(name);
    let has_separator = name.contains('/') || name.contains('\\');
    let (folder, track, mut layout_from_name) = if direct.vfs_is_dir() && (has_separator || root.is_none()) {
        let track = std::path::absolute(&direct).ok().and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned())).unwrap_or_default();
        (direct, track, None)
    } else {
        let Some(root) = root else {
            return Err(format!("the track {name:?} is not a folder, and Assetto Corsa was not found to look it up by name: {}", rustyac_content::install::not_found_hint()));
        };
        let all = installed(root);
        if all.is_empty() {
            return Err(format!("no track is installed under {}", root.join("content").join("tracks").display()));
        }
        let wanted = plain(name);
        let folders = || {
            let mut names: Vec<&str> = all.iter().map(|e| e.track.as_str()).collect();
            names.dedup();
            names
        };
        // 1. the folder's name, 2. a menu name, 3. a part of either that fits one track
        let by_folder: Vec<&TrackEntry> = all.iter().filter(|e| e.track.eq_ignore_ascii_case(name)).collect();
        let by_ui: Vec<&TrackEntry> = all.iter().filter(|e| !e.ui_name.is_empty() && plain(&e.ui_name) == wanted).collect();
        if let Some(first) = by_folder.first() {
            (first.folder.clone(), first.track.clone(), None)
        } else if let Some(first) = by_ui.first() {
            let same_track = by_ui.iter().all(|e| e.track == first.track);
            if !same_track {
                let which: Vec<String> = by_ui.iter().map(|e| e.track.clone()).collect();
                return Err(format!("the name {name:?} fits more than one track: {}; use the folder's name", which.join(", ")));
            }
            let layout = (by_ui.len() == 1 && !first.layout.is_empty()).then(|| first.layout.clone());
            (first.folder.clone(), first.track.clone(), layout)
        } else {
            let mut partial: Vec<&TrackEntry> = Vec::new();
            if !wanted.is_empty() {
                for e in &all {
                    if plain(&e.track).contains(&wanted) || plain(&e.ui_name).contains(&wanted) {
                        partial.push(e);
                    }
                }
            }
            let mut tracks: Vec<&str> = partial.iter().map(|e| e.track.as_str()).collect();
            tracks.dedup();
            match tracks.len() {
                0 => {
                    return Err(format!(
                        "the track {name:?} is unknown: it is neither a folder nor the name of a track under {}. Installed: {}",
                        root.join("content").join("tracks").display(),
                        folders().join(", ")
                    ))
                }
                1 => {
                    let first = partial[0];
                    notes.push(format!("track {name:?} taken as {}", first.track));
                    // a name that fits one layout's menu name only picks that layout
                    let by_layout: Vec<&&TrackEntry> = partial.iter().filter(|e| plain(&e.ui_name).contains(&wanted) && !e.layout.is_empty()).collect();
                    let layout = (by_layout.len() == 1 && !plain(&first.track).contains(&wanted)).then(|| by_layout[0].layout.clone());
                    (first.folder.clone(), first.track.clone(), layout)
                }
                _ => return Err(format!("the name {name:?} fits more than one track: {}; say which", tracks.join(", "))),
            }
        }
    };
    let layouts = layouts_of(&folder);
    let shown = |layouts: &[String]| layouts.iter().map(|l| if l.is_empty() { "(none)".to_string() } else { l.clone() }).collect::<Vec<_>>().join(", ");
    let layout = match layout {
        Some(asked) => {
            let asked = if asked == "-" { "" } else { asked };
            match layouts.iter().find(|l| l.eq_ignore_ascii_case(asked)) {
                Some(found) => found.clone(),
                None => {
                    // a part of a layout's name that fits one only
                    let fits: Vec<&String> = layouts.iter().filter(|l| !asked.is_empty() && plain(l).contains(&plain(asked))).collect();
                    match fits[..] {
                        [one] => {
                            notes.push(format!("layout {asked:?} taken as {one}"));
                            one.clone()
                        }
                        _ => return Err(format!("the track {track} has no layout {asked:?}; its layouts: {}", shown(&layouts))),
                    }
                }
            }
        }
        None => {
            let preferred = preferred_layout(&track).filter(|p| layouts.iter().any(|l| l == p));
            if let Some(layout) = layout_from_name.take() {
                layout
            } else if let Some(layout) = preferred {
                if !layout.is_empty() {
                    notes.push(format!("layout {layout} (race.ini's CONFIG_TRACK)"));
                }
                layout
            } else if layouts.iter().any(String::is_empty) || layouts.is_empty() {
                String::new()
            } else {
                // the first layout that can be driven
                let usable = layouts.iter().find(|l| check(&entry(&folder, &track, l)).is_ok()).unwrap_or(&layouts[0]).clone();
                notes.push(format!("the track {track} has layouts ({}); none was asked for with --layout, so {usable} it is", shown(&layouts)));
                usable
            }
        }
    };
    let found = entry(&folder, &track, &layout);
    if let Err(refusal) = check(&found) {
        let what = if layout.is_empty() { track.clone() } else { format!("{track} / {layout}") };
        return Err(format!("the track {what} is refused: {refusal}"));
    }
    notes.extend(self::notes(&found).into_iter().map(|n| format!("{}: {n}", found.track)));
    Ok(Found { entry: found, notes })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menu_files_are_read_leniently() {
        let text = "{\n\t\"name\": \"Drag 400m\",\n \"author\" : \"Some \\\"One\\\"\", trailing junk";
        assert_eq!(json_string(text, "name").as_deref(), Some("Drag 400m"));
        assert_eq!(json_string(text, "author").as_deref(), Some("Some \"One\""));
        assert_eq!(json_string(text, "version"), None);
        assert_eq!(plain("Laguna Seca"), "lagunaseca");
        assert_eq!(plain("ks_laguna_seca"), "kslagunaseca");
    }

    #[test]
    fn an_unknown_track_says_so() {
        let nowhere = Path::new("no_such_game_folder_for_this_test");
        let error = find(Some(nowhere), "spa", None, &|_| None).unwrap_err();
        assert!(error.contains("no track is installed"), "{error}");
        let error = find(None, "no_such_track_folder", None, &|_| None).unwrap_err();
        assert!(error.contains("is not a folder"), "{error}");
    }
}
