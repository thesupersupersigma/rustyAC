// SPDX-License-Identifier: MIT OR Apache-2.0

//! `kn5_info <file.kn5 | track folder> [--nodes] [--materials] [--textures]`: what a kn5
//! (or every model of a track) holds, and how long reading its structure took.

use std::path::Path;
use std::time::Instant;

use rustyac_content::kn5::{Kn5, NodeClass};
use rustyac_content::track_files::TrackFiles;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(target) = args.iter().find(|a| !a.starts_with("--")) else {
        eprintln!("usage: kn5_info <file.kn5 | track folder> [--nodes] [--materials] [--textures]");
        std::process::exit(2);
    };
    let flag = |name: &str| args.iter().any(|a| a == name);
    let path = Path::new(target);
    let files: Vec<_> = if path.is_dir() {
        match TrackFiles::find(path, "") {
            Ok(track) => track.models.into_iter().map(|m| m.file).collect(),
            Err(e) => {
                eprintln!("{e}");
                std::process::exit(1);
            }
        }
    } else {
        vec![path.to_path_buf()]
    };
    for file in files {
        let start = Instant::now();
        let kn5 = match Kn5::open(&file) {
            Ok(kn5) => kn5,
            Err(e) => {
                eprintln!("{}: {e}", file.display());
                std::process::exit(1);
            }
        };
        let took = start.elapsed();
        let meshes: Vec<_> = kn5.nodes.iter().filter_map(|n| n.mesh.as_ref()).collect();
        let vertices: u64 = meshes.iter().map(|m| m.vertex_count as u64).sum();
        let triangles: u64 = meshes.iter().map(|m| m.index_count as u64 / 3).sum();
        let texture_bytes: u64 = kn5.textures.iter().map(|t| t.size as u64).sum();
        println!(
            "{}: version {}, {} textures ({:.1} MB), {} materials, {} nodes, {} meshes ({} skinned), {} vertices, {} triangles, read in {:.1} ms",
            file.display(),
            kn5.version,
            kn5.textures.len(),
            texture_bytes as f64 / 1048576.0,
            kn5.materials.len(),
            kn5.nodes.len(),
            meshes.len(),
            kn5.nodes.iter().filter(|n| n.class == NodeClass::SkinnedMesh).count(),
            vertices,
            triangles,
            took.as_secs_f64() * 1000.0
        );
        if flag("--textures") {
            for t in &kn5.textures {
                println!("  texture {:?} {} bytes active {}", t.name, t.size, t.active);
            }
        }
        if flag("--materials") {
            for (i, m) in kn5.materials.iter().enumerate() {
                println!("  material {i} {:?} shader {:?} blend {} alpha-test {} depth {} diffuse {:?}", m.name, m.shader, m.alpha_blend_mode, m.alpha_tested, m.depth_mode, m.diffuse());
            }
        }
        if flag("--nodes") {
            for (i, n) in kn5.nodes.iter().enumerate() {
                let depth = std::iter::successors(n.parent, |&p| kn5.nodes[p].parent).count();
                match &n.mesh {
                    Some(m) => println!(
                        "  {i:5} {}{:?} mesh v {} i {} material {} visible {} renderable {} transparent {} active {} lod {:?}..{:?}",
                        "  ".repeat(depth),
                        n.name,
                        m.vertex_count,
                        m.index_count,
                        m.material_id,
                        m.is_visible,
                        m.is_renderable,
                        m.is_transparent,
                        n.active,
                        m.lod_in,
                        m.lod_out
                    ),
                    None => println!("  {i:5} {}{:?} node active {} matrix {:?}", "  ".repeat(depth), n.name, n.active, n.matrix),
                }
            }
        }
    }
}
