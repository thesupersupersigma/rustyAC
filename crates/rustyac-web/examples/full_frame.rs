// SPDX-License-Identifier: GPL-3.0-or-later

//! `cargo run --release -p rustyac-web --example full_frame [--features wgpu] -- --car <id>
//! --track <id> [--layout <l>] [--spawn pit] [--size 1280x720] [--frames 3] [--warp]
//! --out <folder> --name <name>`: Full mode's fixed frame (`rustyac_web::full::fixed_frame`),
//! the one the page draws with `Game::full_proof`, natively from the installed game. Without
//! the feature on Direct3D 11 (`--warp`: WARP), with it on kgl's wgpu backend (`--warp`: Direct3D
//! 12 WARP, else RUSTYAC_WGPU_ADAPTER=gpu or swiftshader). Writes `<name>.kgllog` (the kgl call
//! log), `<name>.png` (the picture) and `<name>.files` (every file read, relative to the game's
//! folder: what a pack needs for this drive in Full mode).

fn run() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let (mut car, mut track, mut layout, mut spawn) = (None, None, String::new(), "pit".to_string());
    let (mut size, mut frames, mut warp, mut out, mut name) = ((1280u32, 720u32), 3u32, false, None, None);
    while let Some(arg) = args.next() {
        let mut value = || args.next().ok_or(format!("{arg} needs a value"));
        match arg.as_str() {
            "--car" => car = Some(value()?),
            "--track" => track = Some(value()?),
            "--layout" => layout = value()?,
            "--spawn" => spawn = value()?,
            "--size" => {
                let text = value()?;
                let (w, h) = text.split_once('x').ok_or("--size <width>x<height>")?;
                size = (w.parse().map_err(|e| format!("--size: {e}"))?, h.parse().map_err(|e| format!("--size: {e}"))?);
            }
            "--frames" => frames = value()?.parse().map_err(|e| format!("--frames: {e}"))?,
            "--warp" => warp = true,
            "--out" => out = Some(std::path::PathBuf::from(value()?)),
            "--name" => name = Some(value()?),
            other => return Err(format!("unknown option {other}")),
        }
    }
    let (car, track) = (car.ok_or("--car <id> is needed")?, track.ok_or("--track <id> is needed")?);
    let (out, name) = (out.ok_or("--out <folder> is needed")?, name.ok_or("--name <name> is needed")?);
    let root = rustyac_content::install::ac_root().ok_or_else(rustyac_content::install::not_found_hint)?;
    // the page has no Documents folder: no video.ini, gameplay.ini or controls of the player's
    rustyac_content::install::set_documents(Some(out.join("no_documents")));
    // every file the drive and the renderer read (a pack must hold them): <name>.files
    rustyac_content::vfs::set_log(true);
    let mut session = rustyac_web::session::Session::new(&car, &track, &layout, true, &spawn)?;
    let mut notes = Vec::new();
    let (mut renderer, log) = rustyac_web::full::fixed_frame(&root, &mut session, size.0, size.1, frames, warp, &mut notes)?;
    let pixels = renderer.read_pixels()?;
    let mut read: Vec<String> = rustyac_content::vfs::take_log().into_iter().filter(|(_, found)| *found).filter_map(|(path, _)| path.strip_prefix(&root).ok().map(|p| p.to_string_lossy().replace('\\', "/"))).collect();
    rustyac_content::vfs::set_log(false);
    read.sort();
    read.dedup();
    std::fs::create_dir_all(&out).map_err(|e| format!("{}: {e}", out.display()))?;
    std::fs::write(out.join(format!("{name}.kgllog")), &log).map_err(|e| e.to_string())?;
    std::fs::write(out.join(format!("{name}.files")), read.join("\n") + "\n").map_err(|e| e.to_string())?;
    let file = std::fs::File::create(out.join(format!("{name}.png"))).map_err(|e| e.to_string())?;
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), size.0, size.1);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header().and_then(|mut w| w.write_image_data(&pixels)).map_err(|e| e.to_string())?;
    println!("{name}: {} x {}, {frames} frames, {} kgl log lines, {}", size.0, size.1, log.lines().count(), notes.join(" / "));
    Ok(())
}

fn main() {
    if let Err(message) = run() {
        eprintln!("full_frame: {message}");
        std::process::exit(1);
    }
}
