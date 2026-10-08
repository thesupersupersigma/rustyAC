// SPDX-License-Identifier: MIT OR Apache-2.0

//! `kn5_material <file.kn5> [material name]`: every property and texture slot of a material,
//! the header of every texture, and where the meshes are.

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let kn5 = rustyac_content::Kn5::open(std::path::Path::new(&args[0])).expect("a kn5 file");
    let mut reader = kn5.reader().expect("the file again");
    for material in kn5.materials.iter().filter(|m| args.get(1).is_some_and(|name| m.name == name.as_str())) {
        println!("{} ({})", material.name, material.shader);
        for p in &material.properties {
            println!("  {} = {} {:?} {:?}", p.name, p.value, p.value3, p.value4);
        }
        for t in &material.textures {
            println!("  {} [{}] = {}", t.name, t.slot, t.texture);
        }
    }
    if args.iter().any(|a| a == "--textures") {
        for texture in &kn5.textures {
            let head = reader.texture_head(texture, 128).expect("a texture header");
            let word = |at: usize| u32::from_le_bytes([head[at], head[at + 1], head[at + 2], head[at + 3]]);
            println!(
                "{:32} {:9} bytes {}x{} mips {} flags {:#x} fourcc {:?} bits {} masks {:#x} {:#x} {:#x} {:#x}",
                texture.name,
                texture.size,
                word(16),
                word(12),
                word(28),
                word(80),
                String::from_utf8_lossy(&head[84..88]),
                word(88),
                word(92),
                word(96),
                word(100),
                word(104)
            );
        }
    }
    if args.iter().any(|a| a == "--meshes") {
        for (index, node) in kn5.nodes.iter().enumerate() {
            if let Some(mesh) = &node.mesh {
                let world = kn5.world_matrix(index);
                let c = mesh.bounding_centre;
                let y = c[0] * world[0][1] + c[1] * world[1][1] + c[2] * world[2][1] + world[3][1];
                let positions = reader.positions(mesh).expect("vertices");
                let (mut low, mut high) = ([f32::MAX; 3], [f32::MIN; 3]);
                for p in &positions {
                    for k in 0..3 {
                        let w = p[0] * world[0][k] + p[1] * world[1][k] + p[2] * world[2][k] + world[3][k];
                        low[k] = low[k].min(w);
                        high[k] = high[k].max(w);
                    }
                }
                println!("{:5} {:28} material {:2} {:22} radius {:6.3} height {:6.3} active {} box {:.2?} .. {:.2?}", index, node.name, mesh.material_id, kn5.materials[mesh.material_id as usize].name, mesh.bounding_radius, y, node.active, low, high);
            }
        }
    }
}
