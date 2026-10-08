// SPDX-License-Identifier: GPL-3.0-or-later

//! `track_survey [--list]`: every track and layout installed under the game's
//! `content/tracks`, loaded the way the game does, as one markdown table: who made it, its
//! model files, physics meshes, surfaces, spawn sets, timing lines, AI line, pit boxes, loose
//! objects, DRS zones, and whatever is unusual about it (or why it is refused).
//!
//! `--list` prints only `<track> <layout>` per line ("-" for no layout), for scripts.

use std::collections::BTreeSet;
use std::path::Path;

use rustyac_content::kn5::Kn5;
use rustyac_physics::track::catalog::{self, Maker, TrackEntry};
use rustyac_physics::track::load_track;

fn megabytes(bytes: u64) -> String {
    format!("{:.0}", bytes as f64 / 1_048_576.0)
}

/// The first number of an AI file: its version.
fn ai_version(path: &Path) -> Option<i32> {
    let bytes = std::fs::read(path).ok()?;
    Some(i32::from_le_bytes(bytes.get(..4)?.try_into().ok()?))
}

fn row(entry: &TrackEntry) -> String {
    let name = if entry.layout.is_empty() { entry.track.clone() } else { format!("{} / {}", entry.track, entry.layout) };
    let maker = match entry.maker {
        Maker::Kunos => "Kunos",
        Maker::Mod => "mod",
    };
    if let Err(refusal) = catalog::check(entry) {
        return format!("| `{name}` | {maker} | | | | | | | | | | **refused**: {refusal} |");
    }
    let (track, report) = match load_track(&entry.folder, &entry.layout) {
        Ok(loaded) => loaded,
        Err(message) => return format!("| `{name}` | {maker} | | | | | | | | | | **does not load**: {} |", message.replace('|', "/")),
    };
    let mut track = track;
    rustyac_physics::track::init_respawn_position_set(&mut track, "HOTLAP_START");
    let size: u64 = report.files.iter().filter_map(|f| std::fs::metadata(f).ok()).map(|m| m.len()).sum();
    let mut versions = BTreeSet::new();
    for file in &report.files {
        if let Ok(kn5) = Kn5::open(file) {
            versions.insert(kn5.version);
        }
    }
    let surfaces: Vec<String> = report.per_key.iter().map(|(key, count)| format!("{} {count}", if key.is_empty() { "(none)" } else { key })).collect();
    let spawns: Vec<String> = track.spawn_positions.iter().map(|(set, slots)| format!("{set} {}", slots.len())).collect();
    let lines = track.time_lines.iter().filter(|l| l.line_type == 0).count();
    let timing = if track.is_open { format!("{lines} + A to B: **point-to-point**") } else { format!("{lines}") };
    let layout_base = if entry.layout.is_empty() { entry.folder.clone() } else { entry.folder.join(&entry.layout) };
    let ai = match (&track.ai_spline, ai_version(&layout_base.join("ai").join("fast_lane.ai"))) {
        (Some(spline), Some(version)) => format!(
            "v{version}, {} points, {:.0} m{}{}",
            spline.point_count(),
            spline.length(),
            if spline.grid_built { ", grid built at load" } else if spline.grid_missing { ", no grid" } else { "" },
            if track.pit_lane_spline.is_some() { "; pit lane line" } else { "" }
        ),
        (None, Some(version)) => format!("v{version}, no points"),
        _ => "none".to_string(),
    };
    let objects = track.objects.len();
    let mut unusual: Vec<String> = Vec::new();
    unusual.extend(catalog::notes(entry));
    if report.rotated_models > 0 {
        unusual.push(format!("`ROTATION` on {} model(s)", report.rotated_models));
    }
    if report.skipped_models > 0 {
        unusual.push(format!("{} `MODEL_n` file(s) missing", report.skipped_models));
    }
    if track.world.direct_meshes() > 0 {
        unusual.push("a mesh in sub-space 0".to_string());
    }
    // a node matrix with a scale: the game's body has none (ODE makes the rotation orthonormal)
    let scaled = track
        .objects
        .iter()
        .filter(|o| {
            let row = o.matrix.m[0];
            let length = (row[0] * row[0] + row[1] * row[1] + row[2] * row[2]).sqrt();
            (length - 1.0).abs() > 1e-3
        })
        .count();
    if scaled > 0 {
        unusual.push(format!("{scaled} loose objects with a scaled node"));
    }
    // AISplineRecorder::load: side csv files newer than the AI line make the game work the
    // track limits out again (and rewrite the line)
    let data = layout_base.join("data");
    let modified = |path: &Path| std::fs::metadata(path).and_then(|m| m.modified()).ok();
    if let (Some(left), Some(right), Some(line)) = (modified(&data.join("side_l.csv")), modified(&data.join("side_r.csv")), modified(&layout_base.join("ai").join("fast_lane.ai"))) {
        if left > line || right > line {
            unusual.push("`side_l/r.csv` newer than the AI line: the game recomputes the track limits at load (not ported; here it gives the stored ones again)".to_string());
        } else {
            unusual.push("has `side_l/r.csv` (older than the AI line: not used)".to_string());
        }
    }
    if versions.iter().any(|v| *v < 5) {
        unusual.push(format!("kn5 version {versions:?}"));
    }
    if track.ai_spline.is_none() {
        unusual.push("no AI line".to_string());
    }
    if track.spawn_slots("PIT") == 0 {
        unusual.push("no pit".to_string());
    }
    if track.spawn_slots("HOTLAP_START") == 0 {
        unusual.push("no hot-lap start".to_string());
    }
    if !track.starting_bounds.is_empty() {
        unusual.push(format!("{} starting bound(s)", track.starting_bounds.len()));
    }
    format!(
        "| `{name}` | {maker} | {} / {} MB | {} ({} tris, {} spaces) | {} | {} | {timing} | {ai} | {} | {objects} | {} | {} |",
        report.files.len(),
        megabytes(size),
        report.objects,
        report.tris,
        report.spaces.len(),
        surfaces.join(", "),
        spawns.join(", "),
        track.spawn_slots("PIT"),
        track.drs_zones.len(),
        if unusual.is_empty() { String::new() } else { unusual.join("; ") },
    )
}

fn main() {
    let list_only = std::env::args().any(|a| a == "--list");
    let Some(root) = rustyac_content::install::ac_root() else {
        eprintln!("{}", rustyac_content::install::not_found_hint());
        std::process::exit(1);
    };
    let entries = catalog::installed(&root);
    if list_only {
        for entry in &entries {
            println!("{} {}", entry.track, if entry.layout.is_empty() { "-" } else { &entry.layout });
        }
        return;
    }
    println!("| Track / layout | By | kn5 files / size | Physics meshes | Surfaces (meshes per `KEY`) | Spawn sets | Timing lines | AI line | Pit boxes | Loose objects | DRS zones | Unusual |");
    println!("|---|---|---|---|---|---|---|---|---|---|---|---|");
    for entry in &entries {
        println!("{}", row(entry));
    }
}
