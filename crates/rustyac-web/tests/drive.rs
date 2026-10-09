// SPDX-License-Identifier: GPL-3.0-or-later

//! The browser's way of loading, on the desktop: the files of a drive are copied into
//! memory, mounted in place of the disk, and the drive must be the one the disk gives.
//!
//! Needs the game's folder (`AC_ROOT`, else Steam's usual place); prints NOT TESTED without.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use rustyac_content::vfs::{self, MemFs};
use rustyac_web::content;
use rustyac_web::model::{Model, ModelOptions, Sink};
use rustyac_web::session::Session;

/// The car's whole state as one number.
fn state_hash(session: &Session) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for word in session.sim.car.car.save_state() {
        for byte in word.to_le_bytes() {
            hash = (hash ^ byte as u64).wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    hash
}

/// A drive of `frames` frames of 1/60 s with the line follower at the controls.
fn drive(car: &str, track: &str, layout: &str, frames: usize) -> Result<(u64, u64, f32), String> {
    // (from the hot-lap start: the line follower does not find its way out of a pit box)
    let mut session = Session::new(car, track, layout, true, "hotlap")?;
    session.input.lock().unwrap().autodrive = true;
    for _ in 0..frames {
        session.advance(1.0 / 60.0)?;
    }
    Ok((session.total_steps, state_hash(&session), session.view().speed_kmh))
}

#[derive(Default)]
struct Counting {
    textures: usize,
    meshes: usize,
    texture_bytes: usize,
}

impl Sink for Counting {
    fn texture(&mut self, image: &rustyac_game::render::dds::Image) -> Result<usize, String> {
        self.textures += 1;
        self.texture_bytes += image.bytes();
        Ok(self.textures - 1)
    }

    fn mesh(&mut self, _vertices: &[rustyac_content::kn5::Vertex], _indices: &[u16]) -> Result<usize, String> {
        self.meshes += 1;
        Ok(self.meshes - 1)
    }
}

fn relative(root: &Path, file: &Path) -> String {
    file.strip_prefix(root).unwrap_or(file).to_string_lossy().replace('\\', "/")
}

#[test]
fn a_drive_from_memory_is_the_drive_from_disk() {
    let Some(root) = rustyac_content::install::ac_root() else {
        println!("NOT TESTED: {}", rustyac_content::install::not_found_hint());
        return;
    };
    for (car, track, layout) in [("bmw_z4_gt3", "ks_laguna_seca", ""), ("ks_ferrari_f2004", "magione", "")] {
        if !root.join("content/cars").join(car).is_dir() || !root.join("content/tracks").join(track).is_dir() {
            println!("NOT TESTED: {car} or {track} is not installed");
            continue;
        }
        // from the disk
        let disk = drive(car, track, layout, 300).expect("the drive from disk");
        let read: Vec<PathBuf> = content::files_read_by_drive(&root, car, track, layout).expect("the logged load");
        let planned = content::files_of_drive(&root, car, track, layout);
        let in_root = |file: &&PathBuf| file.starts_with(&root);
        let missed: Vec<String> = read.iter().filter(in_root).filter(|f| !planned.contains(f)).map(|f| relative(&root, f)).collect();
        assert!(missed.is_empty(), "the load of {car} on {track} read files the page would not have fetched: {missed:?}");
        let bytes: u64 = planned.iter().filter_map(|f| std::fs::metadata(f).ok()).map(|m| m.len()).sum();
        println!("{car} on {track}: {} files read, {} planned ({:.1} MB)", read.iter().filter(in_root).count(), planned.len(), bytes as f64 / 1_048_576.0);

        // from memory: only the planned files, under the name the page mounts them at
        let fs = Arc::new(MemFs::new());
        for file in &planned {
            fs.insert(&format!("ac/{}", relative(&root, file)), std::fs::read(file).unwrap().into());
        }
        vfs::mount(Some(fs.clone()));
        rustyac_content::install::set_ac_root(Some(PathBuf::from("ac")));
        let mounted_root = PathBuf::from("ac");
        let cars = content::cars(&mounted_root, true);
        let tracks = content::tracks(&mounted_root);
        let memory = drive(car, track, layout, 300);
        // the models, as the picture loads them
        let (car_model, track_models) = content::model_files(&mounted_root, car, track, layout);
        let options = ModelOptions { texture_size: 256, texture_budget_mb: 256, unpack_bc: true };
        let mut sink = Counting::default();
        let files: Vec<PathBuf> = track_models.iter().map(|(file, _)| file.clone()).collect();
        let places: Vec<_> = track_models.iter().map(|(_, place)| *place).collect();
        let track_model = Model::load(&files, &places, &options, &mut sink);
        let car_model = car_model.map(|file| Model::load(&[file], &[], &options, &mut sink));
        vfs::mount(None);
        rustyac_content::install::set_ac_root(None);

        assert_eq!(cars.len(), 1, "{cars:?}");
        assert_eq!((cars[0].id.as_str(), &cars[0].refused), (car, &None), "{cars:?}");
        assert!(tracks.iter().any(|t| t.track == track && t.layout == layout && t.refused.is_none()), "{tracks:?}");
        assert_eq!(memory.expect("the drive from memory"), disk, "{car} on {track}: memory against disk (steps, state hash, km/h)");
        assert!(disk.0 == 1666 && disk.2 > 20.0, "the line follower drove: {disk:?}");
        let track_model = track_model.expect("the track's models");
        let car_model = car_model.expect("a car model").expect("the car's model");
        assert!(track_model.meshes.len() > 50 && car_model.meshes.len() > 10 && sink.textures > 20, "{} + {} meshes, {} textures", track_model.meshes.len(), car_model.meshes.len(), sink.textures);
        println!(
            "  from memory: same {} steps, state {:016x}, {:.0} km/h; picture: {} + {} meshes, {} textures ({:.0} MB unpacked at 256 px), notes {:?}",
            disk.0,
            disk.1,
            disk.2,
            track_model.meshes.len(),
            car_model.meshes.len(),
            sink.textures,
            sink.texture_bytes as f64 / 1_048_576.0,
            track_model.stats.notes
        );
    }
}
