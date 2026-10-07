// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni). Not covered by rustyAC's licenses — see LICENSING.md.

//! `track_info <track folder> [layout]`: loads a track the way the game does and says what
//! it found (meshes, surfaces, sub-spaces, timing lines, spawn points, load time).

use std::path::Path;

use rustyac_physics::track::load_track;
use rustyac_physics::tyre::RayTrackCollisionProvider;
use rustyac_physics::vecmath::Vec3f;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(folder) = args.first() else {
        eprintln!("usage: track_info <track folder> [layout]");
        std::process::exit(2);
    };
    let (track, report) = match load_track(Path::new(folder), args.get(1).map(String::as_str).unwrap_or("")) {
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
}
