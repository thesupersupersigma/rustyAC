// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! `track_info <track folder> [layout]`: loads a track the way the game does and says what
//! it found (meshes, surfaces, sub-spaces, timing lines, spawn points, load time). The folder
//! may also be a name under the game's `content/tracks` (`track_info spa`).
//!
//! `track_info <track folder> [layout] --telemetry <csv>` also lays a recording of the real
//! game on that track (a CSV of `ac_telemetry.py`) over the port's track: is the road where
//! the game's car drove, is the car where the game says along the lap, is the start line
//! where the game's lap clock restarts. It checks what the oracle cannot (the oracle hands
//! the game the meshes this port read from the files).

use std::path::PathBuf;

use rustyac_physics::track::{load_track, Track};
use rustyac_physics::tyre::RayTrackCollisionProvider;
use rustyac_physics::vecmath::Vec3f;

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let mut recording = None;
    if let Some(at) = args.iter().position(|a| a == "--telemetry") {
        if at + 1 >= args.len() {
            eprintln!("--telemetry needs a CSV file");
            std::process::exit(2);
        }
        recording = Some(args.remove(at + 1));
        args.remove(at);
    }
    let Some(folder) = args.first() else {
        eprintln!("usage: track_info <track folder> [layout] [--telemetry <csv of ac_telemetry.py>]");
        std::process::exit(2);
    };
    // a name instead of a folder: under the game's content/tracks (AC_ROOT, else Steam's usual place)
    let mut folder = PathBuf::from(folder);
    if !folder.is_dir() {
        if let Some(named) = rustyac_content::install::ac_root().map(|root| root.join("content").join("tracks").join(&folder)).filter(|named| named.is_dir()) {
            folder = named;
        }
    }
    let (track, report) = match load_track(&folder, args.get(1).map(String::as_str).unwrap_or("")) {
        Ok(loaded) => loaded,
        Err(message) => {
            eprintln!("{message}");
            std::process::exit(1);
        }
    };
    println!("{}", report.summary(&track));
    println!("sub-spaces in the order they were made: {:?}", report.spaces);
    for message in &report.messages {
        println!("{message}");
    }
    let mut track = track;
    rustyac_physics::track::init_respawn_position_set(&mut track, "HOTLAP_START");
    for (name, slots) in &track.spawn_positions {
        if let Some((position, tail)) = track.spawn_pose(name, 0) {
            println!("spawn {name}_0 ({} slots) at {position:?}, tail towards {tail:?}", slots.len());
        }
    }
    for (line, position) in track.time_lines.iter().zip(&track.sectors_normalized_positions) {
        println!("timing line {}: {:?} to {:?}, at {position:.6} of the lap", line.id, line.points[0], line.points[1]);
    }
    // the ground under every helper node, as a first look at the rays
    let mut kinds: std::collections::BTreeMap<String, (u32, u32, f32)> = std::collections::BTreeMap::new();
    for helper in &report.helpers {
        let p = helper.world.m[3];
        let kind: String = helper.name.trim_start_matches("AC_").chars().take_while(|c| !c.is_ascii_digit()).collect();
        let entry = kinds.entry(kind).or_insert((0, 0, 0.0));
        entry.0 += 1;
        if let Some(hit) = track.ray_cast(&Vec3f::new(p[0], p[1] + 10.0, p[2]), &Vec3f::new(0.0, -1.0, 0.0), 100.0) {
            entry.1 += 1;
            // how far the node floats above the ground it is dropped onto
            entry.2 = entry.2.max((p[1] - hit.pos.y).abs());
        }
    }
    for (kind, (count, hits, drop)) in &kinds {
        println!("helper nodes AC_{kind}: {count}, {hits} with ground within 90 m below (largest drop {drop:.3} m)");
    }
    if let Some(recording) = recording {
        if let Err(message) = telemetry(&track, &recording) {
            eprintln!("{message}");
            std::process::exit(1);
        }
    }
}

/// The middle, the 99th hundredth and the largest of some sizes.
fn spread(values: &mut [f32]) -> (f32, f32, f32) {
    if values.is_empty() {
        return (0.0, 0.0, 0.0);
    }
    values.sort_by(|a, b| a.total_cmp(b));
    (values[values.len() / 2], values[values.len() * 99 / 100], values[values.len() - 1])
}

/// A recording of the real game on this track against the port's track.
fn telemetry(track: &Track, path: &str) -> Result<(), String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
    let mut lines = text.lines();
    let header: Vec<&str> = lines.next().unwrap_or("").split(',').collect();
    let column = |name: &str| header.iter().position(|h| *h == name).ok_or(format!("{path}: no column {name}"));
    let (lap_ms, track_pos, cg, speed) = (column("lapTimeMs")?, column("trackPos")?, column("cgHeight")?, column("speedKmh")?);
    let xyz = [column("posX")?, column("posY")?, column("posZ")?];
    let spline = track.ai_spline.as_ref();
    let length = track.length();
    let line = track.time_lines.first();

    let (mut frames, mut no_ground) = (0u32, 0u32);
    let (mut height, mut height_signed, mut along) = (Vec::new(), 0.0f64, Vec::new());
    let mut previous: Option<([f32; 3], f32)> = None;
    println!();
    println!("the real game's recording {path} over this track:");
    for row in lines {
        let cells: Vec<&str> = row.split(',').collect();
        let number = |i: usize| cells.get(i).and_then(|c| c.parse::<f32>().ok());
        let (Some(x), Some(y), Some(z)) = (number(xyz[0]), number(xyz[1]), number(xyz[2])) else { continue };
        let (Some(ms), Some(npos), Some(cg), Some(kmh)) = (number(lap_ms), number(track_pos), number(cg), number(speed)) else { continue };
        let pos = [x, y, z];
        // the position is the picture's: it changes once a frame, and its first row is the freshest
        let fresh = previous.is_none_or(|(p, _)| p != pos);
        if let (Some(line), Some((before, ms_before))) = (line, previous) {
            // the lap clock went back: the start line was crossed `ms` ago by the hub of the
            // left front wheel; how far past the port's line is the car's middle now?
            if ms < ms_before && ms_before > 1000.0 && fresh {
                let [p0, p1] = &line.points;
                let (lx, lz) = (p1.x - p0.x, p1.z - p0.z);
                let l = (lx * lx + lz * lz).sqrt();
                let (mut nx, mut nz) = (-lz / l, lx / l);
                if nx * (x - before[0]) + nz * (z - before[2]) < 0.0 {
                    (nx, nz) = (-nx, -nz);
                }
                let past = nx * (x - p0.x) + nz * (z - p0.z);
                let side = ((x - p0.x) * lx + (z - p0.z) * lz) / l;
                println!(
                    "  the lap clock restarted {ms:.0} ms before a row at {kmh:.0} km/h: the car's middle is {past:.2} m past the port's start line ({side:.1} m along its {l:.1} m), so the timed wheel hub was {:.2} m ahead of the car's middle, plus what the car moved between the picture's position and this row (at most a frame: {:.2} m)",
                    kmh / 3.6 * ms / 1000.0 - past,
                    kmh / 3.6 / 60.0
                );
            }
        }
        if fresh {
            frames += 1;
            // the game: cgHeight = the body's height minus the mean height of the four contact points
            match track.ray_cast(&Vec3f::new(x, y + 1.0, z), &Vec3f::new(0.0, -1.0, 0.0), 8.0) {
                Some(hit) => {
                    let d = (y - hit.pos.y) - cg;
                    height_signed += d as f64;
                    height.push(d.abs());
                }
                None => no_ground += 1,
            }
            if let Some(spline) = spline {
                let mut d = spline.spline.world_to_spline(&pos, -1) - npos;
                d -= d.round();
                along.push((d * length).abs());
            }
        }
        previous = Some((pos, ms));
    }
    let count = height.len().max(1) as f64;
    let (middle, most, largest) = spread(&mut height);
    println!(
        "  road height: {frames} positions of the game's car, {no_ground} with no road of the port under them; the port's road under the car's middle against the game's own (the car's height minus its cgHeight): mean difference {:+.4} m, half within {middle:.4} m, 99 % within {most:.4} m, largest {largest:.4} m",
        height_signed / count
    );
    if spline.is_some() {
        let (middle, most, largest) = spread(&mut along);
        println!("  place along the lap: the port's position on the AI line at the game's car against the game's trackPos: half within {middle:.3} m, 99 % within {most:.3} m, largest {largest:.3} m (of {length:.1} m)");
    }
    Ok(())
}
