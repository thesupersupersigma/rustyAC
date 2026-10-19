// SPDX-License-Identifier: MIT OR Apache-2.0

//! Translates every shader pair in a folder (`<name>_vs.fxo` + `<name>_ps.fxo`, AC's
//! `system/shaders/win`) and checks each WGSL module with naga, wgpu's own validator.
//! Prints one table row per pair; the translated text is never kept unless `--dump <dir>`.
//!
//! `cargo run -p rustyac-dxbc --example shader_table -- <folder> [--dump <dir>]`

use std::path::PathBuf;

fn validate(text: &str) -> Result<(), String> {
    let module = naga::front::wgsl::parse_str(text).map_err(|e| e.emit_to_string(text))?;
    naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::default())
        .validate(&module)
        .map_err(|e| e.emit_to_string(text))?;
    Ok(())
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let folder = PathBuf::from(args.first().expect("usage: shader_table <folder> [--dump <dir>]"));
    let dump = args.iter().position(|a| a == "--dump").map(|i| PathBuf::from(&args[i + 1]));
    let mut names: Vec<String> = std::fs::read_dir(&folder)
        .expect("the shader folder")
        .filter_map(|e| e.ok()?.file_name().into_string().ok()?.strip_suffix("_vs.fxo").map(str::to_string))
        .collect();
    names.sort();
    let (mut ok, mut failed) = (0, 0);
    println!("| shader | VS instructions | PS instructions | translated | naga VS | naga PS |");
    println!("|---|---|---|---|---|---|");
    for name in &names {
        let read = |stage: &str| std::fs::read(folder.join(format!("{name}_{stage}.fxo"))).map_err(|e| e.to_string()).and_then(|b| rustyac_dxbc::parse(&b));
        let (vs, ps) = match (read("vs"), read("ps")) {
            (Ok(vs), Ok(ps)) => (vs, ps),
            (Err(e), _) | (_, Err(e)) => {
                failed += 1;
                println!("| {name} | | | read failed: {e} | | |");
                continue;
            }
        };
        let (nv, np) = (vs.program.instructions.len(), ps.program.instructions.len());
        let t = match rustyac_dxbc::translate_pair(&vs, &ps) {
            Ok(t) => t,
            Err(e) => {
                failed += 1;
                println!("| {name} | {nv} | {np} | failed: {e} | | |");
                continue;
            }
        };
        if let Some(dir) = &dump {
            std::fs::create_dir_all(dir).unwrap();
            std::fs::write(dir.join(format!("{name}_vs.wgsl")), &t.vs).unwrap();
            std::fs::write(dir.join(format!("{name}_ps.wgsl")), &t.ps).unwrap();
        }
        let (v, p) = (validate(&t.vs), validate(&t.ps));
        let cell = |r: &Result<(), String>| match r {
            Ok(()) => "valid".to_string(),
            Err(e) => format!("**invalid**: {}", e.lines().take(3).collect::<Vec<_>>().join(" ").replace('|', "/")),
        };
        if v.is_ok() && p.is_ok() {
            ok += 1;
        } else {
            failed += 1;
        }
        println!("| {name} | {nv} | {np} | yes | {} | {} |", cell(&v), cell(&p));
    }
    println!("\n{ok} of {} pairs translate and validate, {failed} fail", names.len());
}
