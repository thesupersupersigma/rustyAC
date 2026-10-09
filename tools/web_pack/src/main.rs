// SPDX-License-Identifier: GPL-3.0-or-later

//! `web_pack`: builds the browser preview's content pack.
//!
//! ```text
//! web_pack [--config web/preview.toml] [--out dist-web/preview] [--ac <Assetto Corsa folder>]
//!          [--dry-run]
//! ```
//!
//! Reads which cars and tracks are wanted (at most two of each), loads every car on every
//! track once with the file log on, and copies exactly the files those loads read from the
//! game's folder into the output folder, laid out as in the game. `manifest.json` lists them
//! with sizes and SHA-256 hashes, per car and per track. The game's folder is only read.
//!
//! The pack is the user's own copy of Kunos' files. `dist-web/` is ignored by git; nothing in
//! it belongs in the repository or in a release.

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use rustyac_physics::track::catalog;
use rustyac_web::content;
use rustyac_web::pack::{Manifest, PackFile, PackItem, Sha256, MAX_CARS, MAX_TRACKS};

/// `key = ["a", "b"]` of the config file (the only TOML it uses).
fn string_list(text: &str, key: &str) -> Result<Vec<String>, String> {
    for line in text.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        let Some((name, value)) = line.split_once('=') else { continue };
        if name.trim() != key {
            continue;
        }
        let value = value.trim();
        let inner = value.strip_prefix('[').and_then(|v| v.strip_suffix(']')).ok_or_else(|| format!("{key}: expected a list in [ ]"))?;
        let mut out = Vec::new();
        for item in inner.split(',').map(str::trim).filter(|item| !item.is_empty()) {
            let name = item.strip_prefix('"').and_then(|v| v.strip_suffix('"')).ok_or_else(|| format!("{key}: {item} is not a text in double quotes"))?;
            out.push(name.to_string());
        }
        return Ok(out);
    }
    Err(format!("no `{key} = [...]` line"))
}

fn megabytes(bytes: u64) -> String {
    format!("{:.1} MB", bytes as f64 / 1e6)
}

/// Copies a file and returns its size and SHA-256.
fn copy_hashed(from: &Path, to: Option<&Path>) -> Result<(u64, String), String> {
    let mut input = std::fs::File::open(from).map_err(|e| format!("{}: {e}", from.display()))?;
    let mut output = match to {
        Some(to) => {
            if let Some(parent) = to.parent() {
                std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
            }
            Some(std::fs::File::create(to).map_err(|e| format!("{}: {e}", to.display()))?)
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
        if let Some(output) = &mut output {
            output.write_all(&buffer[..n]).map_err(|e| e.to_string())?;
        }
    }
    Ok((size, hash.finish()))
}

fn run(args: &[String]) -> Result<(), String> {
    let value = |name: &str| args.iter().position(|a| a == name).and_then(|at| args.get(at + 1)).cloned();
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).parent().and_then(Path::parent).map(Path::to_path_buf).unwrap_or_default();
    let config = value("--config").map(PathBuf::from).unwrap_or_else(|| repo.join("web/preview.toml"));
    let out = value("--out").map(PathBuf::from).unwrap_or_else(|| repo.join("dist-web/preview"));
    let dry = args.iter().any(|a| a == "--dry-run");
    let root = match value("--ac") {
        Some(folder) => PathBuf::from(folder),
        None => rustyac_content::install::ac_root().ok_or_else(rustyac_content::install::not_found_hint)?,
    };
    if !rustyac_content::install::is_ac_root(&root) {
        return Err(format!("{} is not Assetto Corsa's folder (no content and system folders in it)", root.display()));
    }
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
    if cars.len() > MAX_CARS || tracks.len() > MAX_TRACKS {
        return Err(format!(
            "{} asks for {} cars and {} tracks: a preview pack holds at most {MAX_CARS} cars and {MAX_TRACKS} tracks",
            config.display(),
            cars.len(),
            tracks.len()
        ));
    }
    if cars.is_empty() || tracks.is_empty() {
        return Err(format!("{}: at least one car and one track are needed", config.display()));
    }
    let tracks: Vec<(String, String)> = tracks.iter().map(|t| t.split_once('/').map_or((t.clone(), String::new()), |(track, layout)| (track.to_string(), layout.to_string()))).collect();
    println!("Assetto Corsa: {} (read only)", root.display());
    println!("pack: {}{}", out.display(), if dry { " (dry run: nothing is written)" } else { "" });

    // every car on every track: what each load read, by the folder it belongs to
    // (a path that climbs out of the game's folder with `..` is not copied anywhere)
    let relative = |file: &Path| file.strip_prefix(&root).ok().filter(|rest| rest.components().all(|part| matches!(part, std::path::Component::Normal(_)))).map(|rest| rest.to_string_lossy().replace('\\', "/"));
    let mut car_files: BTreeMap<usize, Vec<String>> = BTreeMap::new();
    let mut track_files: BTreeMap<usize, Vec<String>> = BTreeMap::new();
    let mut shared: Vec<String> = Vec::new();
    for (c, car) in cars.iter().enumerate() {
        for (t, (track, layout)) in tracks.iter().enumerate() {
            let entry = catalog::entry_of(&root.join("content").join("tracks").join(track), track, layout);
            catalog::check(&entry).map_err(|refusal| format!("the track {track} {layout} is refused: {refusal}"))?;
            let read = content::files_read_by_drive(&root, car, track, layout).map_err(|e| format!("{car} on {track} {layout}: {e}"))?;
            for file in read.iter().filter_map(|file| relative(file)) {
                let lower = file.to_ascii_lowercase();
                let list = if lower.starts_with(&format!("content/cars/{}/", car.to_ascii_lowercase())) {
                    car_files.entry(c).or_default()
                } else if lower.starts_with(&format!("content/tracks/{}/", track.to_ascii_lowercase())) {
                    track_files.entry(t).or_default()
                } else {
                    &mut shared
                };
                if !list.iter().any(|f| f.eq_ignore_ascii_case(&file)) {
                    list.push(file);
                }
            }
            println!("  loaded {car} on {track}{}: {} files read", if layout.is_empty() { String::new() } else { format!(" ({layout})") }, read.len());
        }
    }

    let mut manifest = Manifest::default();
    let add = |manifest: &mut Manifest, files: &[String]| -> Result<Vec<usize>, String> {
        let mut numbers = Vec::new();
        for file in files {
            let target = (!dry).then(|| out.join(file));
            let (bytes, sha256) = copy_hashed(&root.join(file), target.as_deref())?;
            numbers.push(manifest.file(PackFile { path: file.clone(), bytes, sha256 }));
        }
        Ok(numbers)
    };
    for (c, car) in cars.iter().enumerate() {
        let files = add(&mut manifest, car_files.get(&c).map(Vec::as_slice).unwrap_or(&[]))?;
        manifest.cars.push(PackItem { id: car.clone(), layout: String::new(), name: content::car_name(&root, car), files });
    }
    for (t, (track, layout)) in tracks.iter().enumerate() {
        let files = add(&mut manifest, track_files.get(&t).map(Vec::as_slice).unwrap_or(&[]))?;
        let entry = catalog::entry_of(&root.join("content").join("tracks").join(track), track, layout);
        let name = if entry.ui_name.is_empty() { track.clone() } else { entry.ui_name };
        manifest.tracks.push(PackItem { id: track.clone(), layout: layout.clone(), name, files });
    }
    manifest.shared = add(&mut manifest, &shared)?;
    if !dry {
        std::fs::create_dir_all(&out).map_err(|e| format!("{}: {e}", out.display()))?;
        std::fs::write(out.join("manifest.json"), manifest.to_json()).map_err(|e| format!("manifest.json: {e}"))?;
    }

    println!();
    for (kind, items) in [("car", &manifest.cars), ("track", &manifest.tracks)] {
        for item in items {
            let largest = item.files.iter().map(|&i| &manifest.files[i]).max_by_key(|f| f.bytes);
            println!(
                "{kind:5} {:28} {:>10}  {} files{}",
                if item.layout.is_empty() { item.id.clone() } else { format!("{}/{}", item.id, item.layout) },
                megabytes(manifest.bytes_of(&item.files)),
                item.files.len(),
                largest.map(|f| format!("; the largest: {} ({})", f.path.rsplit('/').next().unwrap_or(""), megabytes(f.bytes))).unwrap_or_default()
            );
        }
    }
    println!("{:5} {:28} {:>10}  {} files", "", "system (shared)", megabytes(manifest.bytes_of(&manifest.shared)), manifest.shared.len());
    println!("total {:28} {:>10}  {} files", "", megabytes(manifest.total_bytes()), manifest.files.len());
    if !dry {
        println!("\nwrote {} (keep it out of git and out of releases: it is Kunos' content)", out.join("manifest.json").display());
    }
    Ok(())
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("usage: web_pack [--config web/preview.toml] [--out dist-web/preview] [--ac <Assetto Corsa folder>] [--dry-run]");
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
}
