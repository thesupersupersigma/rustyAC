// SPDX-License-Identifier: GPL-3.0-or-later

//! `boundaries`: the game's `ModelBoundaries::computeAAModelBoundaries` (0x140218d60) on every
//! car's `collider.kn5`, loaded by the game's `KN5IO` under a `MESH_COLLIDER` node as
//! `CarAvatar::initPhysics` does, against the port's `model_boundaries`. The box gives the
//! cockpit's look-back point (`CameraOnBoard::CameraOnBoard`).

use crate::ac::{rd, Game};
use crate::Args;

const VA_COMPUTE_AA_MODEL_BOUNDARIES: usize = 0x1_4021_8d60;

pub fn run(args: &Args) -> Result<(), String> {
    let cars = args.game.join("content/cars");
    // the scratch game folder, for the shaders the materials ask for
    crate::root::set_documents(&args.root);
    crate::root::prepare(args)?;
    std::env::set_current_dir(&args.root).map_err(|e| format!("{}: {e}", args.root.display()))?;
    let game = Game::start(args)?;
    let mut names: Vec<String> = std::fs::read_dir(&cars).map_err(|e| format!("{}: {e}", cars.display()))?.filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    names.sort();
    let (mut same, mut different) = (0, 0);
    unsafe {
        let io = game.kn5io();
        let compute: extern "C" fn(*mut u8, *mut u8) -> *mut u8 = std::mem::transmute(game.acs.va(VA_COMPUTE_AA_MODEL_BOUNDARIES));
        for name in names {
            let path = cars.join(&name).join("collider.kn5");
            if !path.is_file() {
                continue;
            }
            let node = game.node("MESH_COLLIDER");
            let loaded = game.load_kn5(io, &path.to_string_lossy())?;
            game.add_child(node, loaded);
            let out = game.acs.alloc(24);
            compute(out, node);
            let ac: Vec<f32> = (0..6).map(|i| rd::<f32>(out, i * 4)).collect();
            let port = rustyac_physics::car::colliders::model_boundaries(&path)?.to_vec();
            if ac.iter().map(|x| x.to_bits()).eq(port.iter().map(|x| x.to_bits())) {
                same += 1;
            } else {
                different += 1;
                println!("DIFFERENT {name}: game {ac:?} port {port:?}");
            }
        }
    }
    let verdict = if different == 0 { "IDENTICAL" } else { "DIFFERENT" };
    println!("model boundaries: {verdict} ({same} cars the same, {different} different)");
    Ok(())
}

