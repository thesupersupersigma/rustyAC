// SPDX-License-Identifier: GPL-3.0-or-later

//! `web_pack`: builds the browser preview's content pack.
//!
//! ```text
//! web_pack [--config <file>] [--out dist-web/preview] [--ac <Assetto Corsa folder>] [--dry-run]
//!          [--delta <the live pack's manifest.json>]
//! ```
//!
//! Reads which cars and tracks are wanted: from `--config`, else from `web/my_pack.toml` when
//! that file exists (your own list, ignored by git), else from `web/preview.toml` (two cars,
//! two tracks). `"*"` in a list stands for everything in the install, `"<track>/*"` for every
//! layout of a track.
//!
//! Every car is loaded once on the flat road and every track once (with the first car that
//! loads) with the file log on; exactly the files those loads read are copied from the game's
//! folder into the output folder, laid out as in the game. `manifest.json` lists them with
//! sizes and SHA-256 hashes, per car and per track. A car or track that cannot be used (not
//! installed, made for Custom Shaders Patch only, encrypted, files missing) is left out with
//! one line that says why; nothing is ever decrypted. `--dry-run` prints the sizes and what is
//! left out, and writes nothing. The game's folder is only read.
//!
//! `--delta <manifest.json>` (the manifest of the pack already on a server): builds the same
//! pack but writes only the files that are new or changed against that manifest (same path,
//! size and SHA-256 is unchanged), plus the new `manifest.json`, into an empty `--out` folder,
//! and prints their count and size and the files the new pack no longer has. That folder
//! copied over the old pack (and those files deleted) is the new pack, file for file.
//!
//! The pack is the user's own copy of Kunos' (and mod makers') files. `dist-web/` is ignored by
//! git; nothing in it belongs in the repository or in a release.

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use rustyac_physics::track::catalog::{self, Maker};
use rustyac_web::content;
use rustyac_web::pack::{Manifest, PackFile, PackItem, Sha256, MANY};

/// `key = ["a", "b"]` of the config file (the only TOML it uses).
fn string_list(text: &str, key: &str) -> Result<Vec<String>, String> {
    for line in text.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        let Some((name, value)) = line.split_once('=') else { continue };
        if name.trim() != key {
            continue;
        }
        let value = value.trim();
        let inner = value.strip_prefix('[').and_then(|v| v.strip_suffix(']')).ok_or_else(|| format!("{key}: expected a list in [ ] on one line"))?;
        let mut out = Vec::new();
        for item in inner.split(',').map(str::trim).filter(|item| !item.is_empty()) {
            let name = item.strip_prefix('"').and_then(|v| v.strip_suffix('"')).ok_or_else(|| format!("{key}: {item} is not a text in double quotes"))?;
            out.push(name.to_string());
        }
        return Ok(out);
    }
    Err(format!("no `{key} = [...]` line"))
}

/// The list of a config with its stars filled in: `"*"` is everything of `all`, `"x/*"` is
/// `x` and every `x/...` of `all`. The order is the list's (what is named first comes first,
/// what a star adds comes sorted as `all` is) and nothing comes twice.
fn expand(list: &[String], all: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let add = |name: &str, out: &mut Vec<String>| {
        if !out.iter().any(|have| have.eq_ignore_ascii_case(name)) {
            out.push(name.to_string());
        }
    };
    for item in list {
        if item == "*" {
            all.iter().for_each(|name| add(name, &mut out));
        } else if let Some(folder) = item.strip_suffix("/*") {
            let prefix = format!("{}/", folder.to_ascii_lowercase());
            all.iter().filter(|name| name.eq_ignore_ascii_case(folder) || name.to_ascii_lowercase().starts_with(&prefix)).for_each(|name| add(name, &mut out));
        } else {
            add(item, &mut out);
        }
    }
    out
}

fn megabytes(bytes: u64) -> String {
    format!("{:.1} MB", bytes as f64 / 1e6)
}

/// A refusal as one line of moderate length, without the game's folder in front of its paths.
fn one_line(why: &str, root: &Path) -> String {
    let root = root.to_string_lossy();
    let line = why.lines().map(str::trim).filter(|l| !l.is_empty()).collect::<Vec<_>>().join(" ");
    let line = line.replace(&format!("{root}\\"), "").replace(&format!("{root}/"), "");
    if line.chars().count() > 420 {
        format!("{} ...", line.chars().take(420).collect::<String>())
    } else {
        line
    }
}

/// The cars the game's own sound table names (`content/sfx/GUIDs.txt`, `event:/cars/<car>/`):
/// Kunos' cars. A mod brings its own sounds and is not in it. `None` without the table.
fn stock_cars(root: &Path) -> Option<Vec<String>> {
    let text = std::fs::read(root.join("content").join("sfx").join("GUIDs.txt")).ok()?;
    let text = String::from_utf8_lossy(&text).to_ascii_lowercase();
    let mut cars: Vec<String> = text.match_indices("event:/cars/").filter_map(|(at, marker)| text[at + marker.len()..].split('/').next().map(str::to_string)).collect();
    cars.sort();
    cars.dedup();
    (!cars.is_empty()).then_some(cars)
}

/// `file` with every folder spelled as the first path of the pack that named it: Windows folds
/// case, the Linux server doesn't (a skin.ini can name a skin folder in other letters than the
/// install has). What `out` already has in other letters (an older pack's) takes that spelling.
fn spell(folders: &mut BTreeMap<String, String>, out: &Path, file: &str) -> Result<String, String> {
    let mut path = String::new();
    for part in file.split('/') {
        let next = if path.is_empty() { part.to_string() } else { format!("{path}/{part}") };
        let key = next.to_ascii_lowercase();
        if !folders.contains_key(&key) {
            if let Ok(entries) = std::fs::read_dir(out.join(&path)) {
                for entry in entries.flatten() {
                    let name = entry.file_name().to_string_lossy().into_owned();
                    if name != part && name.eq_ignore_ascii_case(part) {
                        std::fs::rename(entry.path(), out.join(&next)).map_err(|e| format!("{}: {e}", entry.path().display()))?;
                    }
                }
            }
            folders.insert(key.clone(), next);
        }
        path = folders[&key].clone();
    }
    Ok(path)
}

/// An error when two paths of the pack differ only in letter case, in a folder or the file name.
fn one_spelling<'a>(paths: impl Iterator<Item = &'a str>) -> Result<(), String> {
    let mut seen = BTreeMap::new();
    for path in paths {
        for end in path.match_indices('/').map(|(at, _)| at).chain([path.len()]) {
            let part = &path[..end];
            if let Some(other) = seen.insert(part.to_ascii_lowercase(), part) {
                if other != part {
                    return Err(format!("{other} and {part} differ only in letter case: the server would not find one of them"));
                }
            }
        }
    }
    Ok(())
}

/// Copies a file and returns its size and SHA-256.
/// Copies `from` to `to` (or only reads it, without `to`): its size and SHA-256.
fn copy_hashed(from: &Path, to: Option<&Path>) -> Result<(u64, String), String> {
    let mut input = std::fs::File::open(from).map_err(|e| format!("{}: {e}", from.display()))?;
    let mut output = match to {
        Some(to) => {
            if let Some(parent) = to.parent() {
                std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
            }
            Some((std::fs::File::create(to).map_err(|e| format!("{}: {e}", to.display()))?, to))
        }
        None => None,
    };
    let mut hash = Sha256::new();
    let mut buffer = vec![0u8; 1 << 20];
    let mut size = 0u64;
    loop {
        let n = input.read(&mut buffer).map_err(|e| format!("{}: {e}", from.display()))?;
        if n == 0 {
            break;
        }
        hash.update(&buffer[..n]);
        size += n as u64;
        if let Some((output, to)) = &mut output {
            output.write_all(&buffer[..n]).map_err(|e| format!("{}: {e}", to.display()))?;
        }
    }
    Ok((size, hash.finish()))
}

/// The files a `manifest.json` of this tool lists (one `{"path": …, "bytes": …, "sha256": …}`
/// per line, as [`Manifest::to_json`] writes them): path → (size, SHA-256).
fn manifest_files(text: &str) -> Result<BTreeMap<String, (u64, String)>, String> {
    if !text.contains(rustyac_web::pack::FORMAT) {
        return Err(format!("not a web_pack manifest.json (no \"{}\" in it)", rustyac_web::pack::FORMAT));
    }
    let mut files = BTreeMap::new();
    for line in text.lines() {
        let Some(rest) = line.trim().strip_prefix("{\"path\": \"") else { continue };
        let cut = || format!("a line it cannot read: {line}");
        // the path, with the JSON escapes undone
        let mut path = String::new();
        let mut chars = rest.chars();
        loop {
            match chars.next().ok_or_else(cut)? {
                '"' => break,
                '\\' => match chars.next().ok_or_else(cut)? {
                    'u' => {
                        let hex: String = chars.by_ref().take(4).collect();
                        path.push(u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32).ok_or_else(cut)?);
                    }
                    c => path.push(c),
                },
                c => path.push(c),
            }
        }
        let rest = chars.as_str();
        let bytes = rest.split("\"bytes\": ").nth(1).and_then(|s| s.split(|c: char| !c.is_ascii_digit()).next()).and_then(|s| s.parse().ok());
        let sha256 = rest.split("\"sha256\": \"").nth(1).and_then(|s| s.split('"').next()).filter(|s| s.len() == 64);
        match (bytes, sha256) {
            (Some(bytes), Some(sha256)) => files.insert(path, (bytes, sha256.to_string())),
            _ => return Err(cut()),
        };
    }
    if files.is_empty() {
        return Err("no files in it".to_string());
    }
    Ok(files)
}

/// Runs the look at one car or track; a loader that gives up anywhere on the way (a mod's
/// file it cannot make sense of) is a reason to leave that one out, not the end of the build.
fn guarded<T>(look: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(look)) {
        Ok(result) => result,
        Err(panic) => {
            let why = panic.downcast_ref::<String>().cloned().or_else(|| panic.downcast_ref::<&str>().map(|s| s.to_string())).unwrap_or_default();
            Err(format!("the loader gave up on it: {why}"))
        }
    }
}

/// A car or track that goes into the pack: what a load of it read.
struct Item {
    /// The car's folder, or the track's.
    id: String,
    layout: String,
    name: String,
    is_mod: bool,
    /// Named in the config itself, in a list that has a star too.
    pinned: bool,
    /// Its own files, relative to the game's folder, with their sizes.
    files: Vec<(String, u64)>,
}

impl Item {
    fn label(&self) -> String {
        if self.layout.is_empty() {
            self.id.clone()
        } else {
            format!("{}/{}", self.id, self.layout)
        }
    }

    fn bytes(&self) -> u64 {
        self.files.iter().map(|(_, bytes)| bytes).sum()
    }
}

/// Every file under `folder` (relative to `root`, `/` between names), the ones `keep` takes.
fn files_under(root: &Path, folder: &str, keep: &dyn Fn(&str) -> bool, out: &mut BTreeMap<String, u64>) {
    let Ok(entries) = std::fs::read_dir(root.join(folder)) else { return };
    let mut entries: Vec<_> = entries.filter_map(|e| e.ok()).collect();
    entries.sort_by_key(|e| e.file_name().to_string_lossy().to_ascii_lowercase());
    for entry in entries {
        let name = entry.file_name().to_string_lossy().into_owned();
        let path = if folder.is_empty() { name } else { format!("{folder}/{name}") };
        match entry.file_type() {
            Ok(kind) if kind.is_dir() => files_under(root, &path, keep, out),
            Ok(kind) if kind.is_file() && keep(&path.to_ascii_lowercase()) => {
                out.insert(path, entry.metadata().map(|m| m.len()).unwrap_or(0));
            }
            _ => {}
        }
    }
}

/// AC's launcher in the page: its default theme and the shared scripts the launcher puts into
/// it, the game's `cfg` (the player's settings start as copies of it), the weather list, the
/// special events' event.ini files, and
/// for every car and track in the pack what the launcher shows of it: menu entries, brand
/// badges, skins' previews and liveries, track previews and outlines. Videos are left out.
fn launcher_files(root: &Path, cars: &[String], tracks: &[String]) -> BTreeMap<String, u64> {
    let mut out = BTreeMap::new();
    let all = |_: &str| true;
    files_under(root, "cfg", &all, &mut out);
    // the official post-processing filters the Video options list (ui.js listfiles ... root=true)
    files_under(root, "system/cfg", &all, &mut out);
    files_under(root, "launcher/themes/.base", &all, &mut out);
    files_under(root, "launcher/themes/default", &|p: &str| !p.ends_with(".ogv"), &mut out);
    files_under(root, "content/weather", &|p: &str| p.ends_with(".ini"), &mut out);
    // shortcut: the Special Events list without its pictures (144 MB), add them if the preview should show them
    files_under(root, "content/specialevents", &|p: &str| p.ends_with(".ini"), &mut out);
    for car in cars {
        files_under(root, &format!("content/cars/{car}/ui"), &all, &mut out);
        let skin = |p: &str| p.ends_with("/ui_skin.json") || p.ends_with("/preview.jpg") || p.ends_with("/livery.png");
        files_under(root, &format!("content/cars/{car}/skins"), &skin, &mut out);
    }
    for track in tracks {
        files_under(root, &format!("content/tracks/{track}/ui"), &|p: &str| !p.ends_with(".ogv"), &mut out);
    }
    out
}

/// FMOD Studio's release build for the page (the logging build is twice the size).
const FMOD_FILES: [&str; 2] = ["fmodstudio.js", "fmodstudio.wasm"];

fn run(args: &[String]) -> Result<(), String> {
    let value = |name: &str| args.iter().position(|a| a == name).and_then(|at| args.get(at + 1)).cloned();
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).parent().and_then(Path::parent).map(Path::to_path_buf).unwrap_or_default();
    let config = value("--config").map(PathBuf::from).unwrap_or_else(|| {
        let mine = repo.join("web").join("my_pack.toml");
        if mine.is_file() {
            mine
        } else {
            repo.join("web").join("preview.toml")
        }
    });
    let out = value("--out").map(PathBuf::from).unwrap_or_else(|| repo.join("dist-web/preview"));
    let dry = args.iter().any(|a| a == "--dry-run");
    let old = match value("--delta") {
        Some(file) => Some(std::fs::read_to_string(&file).map_err(|e| format!("{file}: {e}")).and_then(|text| manifest_files(&text).map_err(|e| format!("{file}: {e}")))?),
        None => None,
    };
    // a delta folder holds the upload and nothing else
    if old.is_some() && !dry && std::fs::read_dir(&out).is_ok_and(|mut entries| entries.next().is_some()) {
        return Err(format!("--delta writes into an empty folder: {} has files in it", out.display()));
    }
    let root = match value("--ac") {
        Some(folder) => PathBuf::from(folder),
        None => rustyac_content::install::ac_root().ok_or_else(rustyac_content::install::not_found_hint)?,
    };
    if !rustyac_content::install::is_ac_root(&root) {
        return Err(format!("{} is not Assetto Corsa's folder (no content and system folders in it)", root.display()));
    }
    // the loaders find a car by its name in this folder
    rustyac_content::install::set_ac_root(Some(root.clone()));
    // the pack is written next to nothing of the game: never into its folder (a file would be
    // truncated before it is read), never around it
    let whole = |path: &Path| std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    let (whole_out, whole_root) = (whole(&out), whole(&root));
    if whole_out.starts_with(&whole_root) || whole_root.starts_with(&whole_out) {
        return Err(format!("the pack's folder {} and the game's folder {} must not lie inside one another", out.display(), root.display()));
    }
    let text = std::fs::read_to_string(&config).map_err(|e| format!("{}: {e}", config.display()))?;
    let cars = string_list(&text, "cars").map_err(|e| format!("{}: {e}", config.display()))?;
    let tracks = string_list(&text, "tracks").map_err(|e| format!("{}: {e}", config.display()))?;

    // what the install has: every folder under content/cars, every track and layout
    let cars_folder = root.join("content").join("cars");
    let mut all_cars: Vec<String> = std::fs::read_dir(&cars_folder)
        .map_err(|e| format!("{}: {e}", cars_folder.display()))?
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.path().is_dir())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    all_cars.sort_by_key(|name| name.to_ascii_lowercase());
    let all_tracks: Vec<String> = catalog::installed(&root).into_iter().map(|entry| if entry.layout.is_empty() { entry.track } else { format!("{}/{}", entry.track, entry.layout) }).collect();
    // what the config names itself comes first on the page, before what a star adds
    let named = |list: &[String]| -> Vec<String> { if list.iter().any(|item| item.ends_with('*')) { list.iter().filter(|item| !item.ends_with('*')).map(|item| item.to_ascii_lowercase()).collect() } else { Vec::new() } };
    let (named_cars, named_tracks) = (named(&cars), named(&tracks));
    let stock = stock_cars(&root);
    let cars = expand(&cars, &all_cars);
    let tracks: Vec<(String, String)> = expand(&tracks, &all_tracks).iter().map(|t| t.split_once('/').map_or((t.clone(), String::new()), |(track, layout)| (track.to_string(), layout.to_string()))).collect();
    if cars.is_empty() || tracks.is_empty() {
        return Err(format!("{}: at least one car and one track are needed", config.display()));
    }
    println!("config: {}", config.display());
    println!("Assetto Corsa: {} (read only)", root.display());
    println!("pack: {}{}", out.display(), if dry { " (dry run: nothing is written)" } else { "" });
    println!("asked for: {} cars, {} tracks and layouts", cars.len(), tracks.len());

    // (a path that climbs out of the game's folder with `..` is not copied anywhere)
    let relative = |file: &Path| file.strip_prefix(&root).ok().filter(|rest| rest.components().all(|part| matches!(part, std::path::Component::Normal(_)))).map(|rest| rest.to_string_lossy().replace('\\', "/"));
    let sized = |file: String| -> Result<(String, u64), String> {
        let bytes = std::fs::metadata(root.join(&file)).map_err(|e| format!("{file}: {e}"))?.len();
        Ok((file, bytes))
    };
    // a loader that gives up on a mod's file must not fill the screen: its words become the reason
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let mut shared: BTreeMap<String, u64> = BTreeMap::new();
    let mut skipped: Vec<(&str, String, String)> = Vec::new();

    // every car, alone on the flat road
    let mut car_items: Vec<Item> = Vec::new();
    for (n, car) in cars.iter().enumerate() {
        eprint!("\rchecking car {} of {}: {car:40}", n + 1, cars.len());
        let folder = cars_folder.join(car);
        let probe = guarded(|| -> Result<Item, String> {
            if !folder.is_dir() {
                return Err("not installed: no such folder under content/cars".to_string());
            }
            if !folder.join("data.acd").is_file() && !folder.join("data").join("car.ini").is_file() {
                return Err("not installed: the folder has neither data.acd nor a data folder (a DLC that is not owned, or a mod without its data)".to_string());
            }
            let mut read = content::files_read_by_drive(&root, car, "", "")?;
            content::check_car_model(&root, car)?;
            // and what Full mode (AC's renderer) reads: shaders, sky, pits, the car's skin and driver
            read.extend(content::files_of_picture(&root, car, "", ""));
            // Task 43: every skin's skin.ini and the suits it names, for a car in any skin
            for skin in std::fs::read_dir(folder.join("skins")).into_iter().flatten().flatten().filter(|e| e.path().is_dir()) {
                read.extend(content::suit_files(&root, car, &skin.file_name().to_string_lossy()));
            }
            // (the first skin's are in files_of_picture already: a file counts once)
            read.sort();
            read.dedup();
            let own = format!("content/cars/{}/", car.to_ascii_lowercase());
            let mut files = Vec::new();
            // the sound reads its bank and GUIDs.txt only once FMOD runs, and sounds.ini from a data folder
            let mut sound = BTreeMap::new();
            files_under(&root, &format!("content/cars/{car}/sfx"), &|p: &str| p.ends_with(".bank") || p.ends_with("/guids.txt"), &mut sound);
            files_under(&root, &format!("content/cars/{car}/data"), &|p: &str| p.ends_with("/sounds.ini"), &mut sound);
            files.extend(sound.into_iter().filter(|(file, _)| !read.iter().any(|r| relative(r).as_deref() == Some(file.as_str()))));
            for file in read.iter().filter_map(|file| relative(file)) {
                let (file, bytes) = sized(file)?;
                if file.to_ascii_lowercase().starts_with(&own) {
                    files.push((file, bytes));
                } else {
                    shared.insert(file, bytes);
                }
            }
            let is_mod = match &stock {
                Some(stock) => !stock.contains(&car.to_ascii_lowercase()),
                None => content::car_is_mod(&root, car),
            };
            Ok(Item { id: car.clone(), layout: String::new(), name: content::car_name(&root, car), is_mod, pinned: named_cars.contains(&car.to_ascii_lowercase()), files })
        });
        match probe {
            Ok(item) => car_items.push(item),
            Err(why) => skipped.push(("car", car.clone(), one_line(&why, &root))),
        }
    }
    eprintln!();
    // the sound's own tables (no .strings.bank: every event is found by GUID)
    for file in ["content/sfx/GUIDs.txt", "content/sfx/common.bank"] {
        if let Ok(sized) = sized(file.to_string()) {
            shared.insert(sized.0, sized.1);
        }
    }
    let Some(probe_car) = car_items.first().map(|item| item.id.clone()) else {
        std::panic::set_hook(hook);
        for (kind, name, why) in &skipped {
            println!("skipped {kind} {name}: {why}");
        }
        return Err("none of the cars can be used: a pack needs at least one".to_string());
    };

    // every track, with the first car that loads
    let mut track_items: Vec<Item> = Vec::new();
    for (n, (track, layout)) in tracks.iter().enumerate() {
        let label = if layout.is_empty() { track.clone() } else { format!("{track}/{layout}") };
        eprint!("\rchecking track {} of {}: {label:50}", n + 1, tracks.len());
        let probe = guarded(|| -> Result<Item, String> {
            let entry = catalog::entry_of(&root.join("content").join("tracks").join(track), track, layout);
            catalog::check(&entry).map_err(|refusal| refusal.to_string())?;
            let mut read = content::files_read_by_drive(&root, &probe_car, track, layout)?;
            read.extend(content::files_of_picture(&root, &probe_car, track, layout));
            let own = format!("content/tracks/{}/", track.to_ascii_lowercase());
            let car = format!("content/cars/{}/", probe_car.to_ascii_lowercase());
            let mut files = Vec::new();
            // the track's reverbs and occluders (read by the sound only)
            let mut sound = BTreeMap::new();
            let data = if layout.is_empty() { format!("content/tracks/{track}/data") } else { format!("content/tracks/{track}/{layout}/data") };
            files_under(&root, &data, &|p: &str| p.ends_with("/audio_sources.ini"), &mut sound);
            files.extend(sound);
            for file in read.iter().filter_map(|file| relative(file)) {
                let lower = file.to_ascii_lowercase();
                if lower.starts_with(&car) {
                    continue;
                }
                let (file, bytes) = sized(file)?;
                if lower.starts_with(&own) {
                    files.push((file, bytes));
                } else {
                    shared.insert(file, bytes);
                }
            }
            let name = if entry.ui_name.is_empty() { track.clone() } else { entry.ui_name.clone() };
            Ok(Item { id: track.clone(), layout: layout.clone(), name, is_mod: entry.maker == Maker::Mod, pinned: named_tracks.contains(&label.to_ascii_lowercase()), files })
        });
        match probe {
            Ok(item) => track_items.push(item),
            Err(why) => skipped.push(("track", label, one_line(&why, &root))),
        }
    }
    eprintln!();
    std::panic::set_hook(hook);

    // the sizes, before anything is copied (a file two layouts share counts once in the total)
    let print_sizes = |car_items: &[Item], track_items: &[Item]| {
        let mut every: BTreeMap<String, u64> = shared.iter().map(|(file, bytes)| (file.to_ascii_lowercase(), *bytes)).collect();
        println!();
        for (kind, items) in [("car", car_items), ("track", track_items)] {
            for item in items {
                let largest = item.files.iter().max_by_key(|(_, bytes)| *bytes);
                println!(
                    "{kind:5} {:44} {:>10}  {:3} files{}{}",
                    item.label(),
                    megabytes(item.bytes()),
                    item.files.len(),
                    largest.map(|(file, bytes)| format!("; the largest: {} ({})", file.rsplit('/').next().unwrap_or(""), megabytes(*bytes))).unwrap_or_default(),
                    if item.is_mod { "   [mod]" } else { "" }
                );
                every.extend(item.files.iter().map(|(file, bytes)| (file.to_ascii_lowercase(), *bytes)));
            }
        }
        println!("{:5} {:44} {:>10}  {:3} files", "", "system (shared)", megabytes(shared.values().sum()), shared.len());
        let of = |items: &[Item]| {
            let own: BTreeMap<String, u64> = items.iter().flat_map(|item| item.files.iter().map(|(file, bytes)| (file.to_ascii_lowercase(), *bytes))).collect();
            megabytes(own.values().sum())
        };
        println!("total {:44} {:>10}  {:3} files   ({} cars: {}; {} tracks and layouts: {})", "", megabytes(every.values().sum()), every.len(), car_items.len(), of(car_items), track_items.len(), of(track_items));
    };
    print_sizes(&car_items, &track_items);

    // what is left out, and why: the same reason once, with every name it holds for
    let print_skipped = |skipped: &[(&str, String, String)]| {
        if skipped.is_empty() {
            return;
        }
        println!("\nleft out ({}):", skipped.len());
        let mut by_reason: Vec<(String, Vec<String>)> = Vec::new();
        for (kind, name, why) in skipped {
            let key = format!("{kind}: {why}");
            match by_reason.iter_mut().find(|(reason, _)| *reason == key) {
                Some((_, names)) => names.push(name.clone()),
                None => by_reason.push((key, vec![name.clone()])),
            }
        }
        for (reason, names) in by_reason {
            let (kind, why) = reason.split_once(": ").unwrap_or(("", &reason));
            println!("  {kind} {}: {why}", names.join(", "));
        }
    };
    print_skipped(&skipped);
    let mods = |items: &[Item]| items.iter().filter(|item| item.is_mod).map(Item::label).collect::<Vec<_>>();
    let (car_mods, track_mods) = (mods(&car_items), mods(&track_items));
    let listed = |names: &[String]| if names.is_empty() { String::new() } else { format!(" ({})", names.join(", ")) };
    println!("\nmods in the pack: {} cars{}; {} tracks and layouts{}", car_mods.len(), listed(&car_mods), track_mods.len(), listed(&track_mods));
    if car_items.len() > MANY || track_items.len() > MANY {
        println!(
            "\nnote: {} cars and {} tracks and layouts is a large pack (more than {MANY} cars or tracks). That is fine on a server only you can reach;\n      it is Kunos' and the mod makers' content, so keep the site behind a login (Cloudflare Access).",
            car_items.len(),
            track_items.len()
        );
    }
    if track_items.is_empty() {
        return Err("none of the tracks can be used: a pack needs at least one".to_string());
    }
    if dry {
        return Ok(());
    }

    // copy and hash; a car or track whose file cannot be copied after all is left out too
    let mut manifest = Manifest::default();
    let mut late: Vec<(&str, String, String)> = Vec::new();
    // a file the live pack already has (same path, size and hash) is only read, not written
    let put = |from: &Path, file: &str| -> Result<(u64, String), String> {
        if let Some(old) = &old {
            let read = copy_hashed(from, None)?;
            if old.get(file) == Some(&read) {
                return Ok(read);
            }
        }
        copy_hashed(from, Some(&out.join(file)))
    };
    let folders = std::cell::RefCell::new(BTreeMap::new());
    let add = |manifest: &mut Manifest, files: &[(String, u64)]| -> Result<Vec<usize>, String> {
        let mut numbers = Vec::new();
        for (from, _) in files {
            let file = &spell(&mut folders.borrow_mut(), &out, from)?;
            // (a file two layouts share is copied once)
            if let Some(index) = manifest.files.iter().position(|f| f.path.eq_ignore_ascii_case(file)) {
                numbers.push(index);
                continue;
            }
            let (bytes, sha256) = put(&root.join(from), file)?;
            numbers.push(manifest.file(PackFile { path: file.clone(), bytes, sha256 }));
        }
        Ok(numbers)
    };
    let total = car_items.len() + track_items.len();
    for (n, item) in car_items.iter().chain(&track_items).enumerate() {
        let kind = if n < car_items.len() { "car" } else { "track" };
        eprint!("\rcopying {} of {total}: {:50}", n + 1, item.label());
        match add(&mut manifest, &item.files) {
            Ok(files) => {
                let entry = PackItem { id: item.id.clone(), layout: item.layout.clone(), name: item.name.clone(), pinned: item.pinned, files };
                if kind == "car" {
                    manifest.cars.push(entry);
                } else {
                    manifest.tracks.push(entry);
                }
            }
            Err(why) => late.push((kind, item.label(), one_line(&why, &root))),
        }
    }
    eprintln!();
    let shared_files: Vec<(String, u64)> = shared.iter().map(|(file, bytes)| (file.clone(), *bytes)).collect();
    manifest.shared = add(&mut manifest, &shared_files)?;
    let pack_cars: Vec<String> = manifest.cars.iter().map(|c| c.id.clone()).collect();
    let mut pack_tracks: Vec<String> = manifest.tracks.iter().map(|t| t.id.clone()).collect();
    pack_tracks.dedup();
    let launcher: Vec<(String, u64)> = launcher_files(&root, &pack_cars, &pack_tracks).into_iter().collect();
    manifest.launcher = add(&mut manifest, &launcher)?;
    println!("AC's launcher: {} in {} files (its theme, cfg, and the cars' and tracks' menu pictures)", megabytes(manifest.bytes_of(&manifest.launcher)), manifest.launcher.len());
    // FMOD's browser build (Firelight's: never in the repo or a release, only in a pack), from ignored/fmod
    let fmod = repo.join("ignored/fmod/fmodstudioapi20315html5/fmodstudioapi20315html5/api/studio/lib/wasm");
    for name in FMOD_FILES {
        match put(&fmod.join(name), &format!("fmod/{name}")) {
            Ok((bytes, sha256)) => {
                manifest.file(PackFile { path: format!("fmod/{name}"), bytes, sha256 });
            }
            Err(why) => println!("no sound in the page: FMOD Studio 2.03.15 HTML5 is not unpacked under ignored/fmod ({why})"),
        }
    }
    if manifest.cars.is_empty() || manifest.tracks.is_empty() {
        return Err("no car or no track could be copied: no pack was written".to_string());
    }
    one_spelling(manifest.files.iter().map(|f| f.path.as_str()))?;
    std::fs::create_dir_all(&out).map_err(|e| format!("{}: {e}", out.display()))?;
    std::fs::write(out.join("manifest.json"), manifest.to_json()).map_err(|e| format!("manifest.json: {e}"))?;
    print_skipped(&late);
    println!(
        "\nwrote {}: {} cars, {} tracks and layouts, {} in {} files\n(keep it out of git and out of releases: it is Kunos' and the mod makers' content)",
        out.join("manifest.json").display(),
        manifest.cars.len(),
        manifest.tracks.len(),
        megabytes(manifest.total_bytes()),
        manifest.files.len()
    );
    if let Some(old) = &old {
        let new: Vec<&PackFile> = manifest.files.iter().filter(|f| old.get(&f.path) != Some(&(f.bytes, f.sha256.clone()))).collect();
        let kept: std::collections::BTreeSet<&str> = manifest.files.iter().map(|f| f.path.as_str()).collect();
        let gone: Vec<(&String, u64)> = old.iter().filter(|(path, _)| !kept.contains(path.as_str())).map(|(path, (bytes, _))| (path, *bytes)).collect();
        println!(
            "\ndelta: {} new or changed files, {} (plus manifest.json) in {}: upload these over the live pack",
            new.len(),
            megabytes(new.iter().map(|f| f.bytes).sum()),
            out.display()
        );
        println!("the new pack no longer has {} files of the live pack ({}){}", gone.len(), megabytes(gone.iter().map(|(_, bytes)| bytes).sum()), if gone.is_empty() { "" } else { ": delete them there" });
        for (path, _) in gone {
            println!("  gone: {path}");
        }
    }
    Ok(())
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("usage: web_pack [--config <file>] [--out dist-web/preview] [--ac <Assetto Corsa folder>] [--dry-run] [--delta <live manifest.json>]");
        println!("--delta: write only the files that are new or changed against that manifest, plus the new manifest.json, into an empty --out");
        println!("the config: --config, else web/my_pack.toml when it exists, else web/preview.toml");
        return;
    }
    if let Err(message) = run(&args) {
        eprintln!("web_pack: {message}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_config_is_two_lists() {
        let text = "# cars\ncars = [\"a\", \"b\"]  # two\ntracks = [ \"spa\" ,\"ring/layout_gp\" ]\n";
        assert_eq!(string_list(text, "cars").unwrap(), vec!["a", "b"]);
        assert_eq!(string_list(text, "tracks").unwrap(), vec!["spa", "ring/layout_gp"]);
        assert!(string_list(text, "bikes").is_err());
        assert!(string_list("cars = \"a\"", "cars").is_err());
    }

    #[test]
    fn every_folder_has_one_spelling_on_disk_too() {
        let out = std::env::temp_dir().join(format!("web_pack_case_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&out);
        // an older pack's folder in the install's letters
        std::fs::create_dir_all(out.join("content/cars/c/skins/ARROW_RG7")).unwrap();
        std::fs::write(out.join("content/cars/c/skins/ARROW_RG7/Old.dds"), b"x").unwrap();
        let mut folders = BTreeMap::new();
        let mut spelled = |file: &str| spell(&mut folders, &out, file).unwrap();
        assert_eq!(spelled("content/cars/c/skins/arrow_rg7/ui_skin.json"), "content/cars/c/skins/arrow_rg7/ui_skin.json");
        assert_eq!(spelled("content/cars/c/skins/ARROW_RG7/livery.png"), "content/cars/c/skins/arrow_rg7/livery.png");
        assert_eq!(spelled("content/cars/C/skins/arrow_rg7/old.dds"), "content/cars/c/skins/arrow_rg7/old.dds");
        let names = |dir: &str| std::fs::read_dir(out.join(dir)).unwrap().map(|e| e.unwrap().file_name().into_string().unwrap()).collect::<Vec<_>>();
        assert_eq!(names("content/cars/c/skins"), ["arrow_rg7"]);
        assert_eq!(names("content/cars/c/skins/arrow_rg7"), ["old.dds"]);
        std::fs::remove_dir_all(&out).unwrap();
        assert!(one_spelling(["a/B/c.ini", "a/B/d.ini", "a/e.ini"].into_iter()).is_ok());
        assert!(one_spelling(["a/B/c.ini", "a/b/d.ini"].into_iter()).is_err());
        assert!(one_spelling(["a/c.ini", "a/C.INI"].into_iter()).is_err());
    }

    #[test]
    fn a_manifest_reads_back() {
        let mut manifest = Manifest::default();
        let paths = ["content/cars/a\"b\\c/data.acd", "content/tracks/\u{1}é/x.kn5"];
        for (n, path) in paths.iter().enumerate() {
            manifest.file(PackFile { path: path.to_string(), bytes: n as u64 * 7, sha256: format!("{n:064x}") });
        }
        manifest.shared = vec![0, 1];
        let files = manifest_files(&manifest.to_json()).unwrap();
        assert_eq!(files.len(), 2);
        assert_eq!(files[paths[0]], (0, format!("{:064x}", 0)));
        assert_eq!(files[paths[1]], (7, format!("{:064x}", 1)));
        assert!(manifest_files("{}").is_err());
        assert!(manifest_files(&format!("{{\"format\": \"{}\"}}", rustyac_web::pack::FORMAT)).is_err());
    }

    #[test]
    fn a_star_is_everything_after_what_is_named_first() {
        let all: Vec<String> = ["abarth500", "bmw_z4_gt3", "ks_ferrari_f2004"].iter().map(|s| s.to_string()).collect();
        let list = |items: &[&str]| items.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(expand(&list(&["ks_ferrari_f2004", "bmw_z4_gt3", "*"]), &all), list(&["ks_ferrari_f2004", "bmw_z4_gt3", "abarth500"]));
        assert_eq!(expand(&list(&["*"]), &all), all);
        // a name that is not in the install stays (it is left out later, with the reason), twice is once
        assert_eq!(expand(&list(&["nope", "BMW_Z4_GT3", "bmw_z4_gt3"]), &all), list(&["nope", "BMW_Z4_GT3"]));
        let tracks = list(&["imola", "ks_nurburgring/layout_gp_a", "ks_nurburgring/layout_sprint_a", "spa"]);
        assert_eq!(expand(&list(&["spa", "ks_nurburgring/*"]), &tracks), list(&["spa", "ks_nurburgring/layout_gp_a", "ks_nurburgring/layout_sprint_a"]));
        assert_eq!(expand(&list(&["imola/*"]), &tracks), list(&["imola"]));
        assert_eq!(megabytes(1_500_000), "1.5 MB");
        assert_eq!(one_line("C:\\ac\\content\\cars\\a\\data\\x.ini: bad\n  value\n", Path::new("C:\\ac")), "content\\cars\\a\\data\\x.ini: bad value");
    }
}
