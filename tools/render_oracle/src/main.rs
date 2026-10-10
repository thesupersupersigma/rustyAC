// SPDX-License-Identifier: GPL-3.0-or-later

//! Render oracle: Assetto Corsa's own renderer (run from acs.exe in this process, on a WARP
//! device, off screen) against the Rust port in `crates/rustyac-render`.
//! See `docs/port/renderer_core.md`.

#[allow(dead_code)]
#[path = "../../car_oracle/src/acs.rs"]
mod acs;
mod ac;
mod ac_car;
mod compare;
mod frames;
mod port;
mod root;

use std::path::PathBuf;

const USAGE: &str = "usage:
  render_oracle run --side ac|port [frame options]   render one frame with one side; writes
                                                     <out>/<frame>.<side>.gpulog / .init.gpulog / .png
  render_oracle compare [frame options] [--loose] [--own-textures]    both sides (a process each), then the logs line
                                                     by line and the pictures byte by byte
     frame options: [--track <folder name>] [--layout <l>] [--view chase|cockpit|free|eyes|sun|far|side|rear|front] [--capture <n>]
                    [--car <folder name> --pose <file> [--skin <folder name>]]
                    a sequence: --car <c> --tape <file.audiotape> [--tape-from <frame>] [--frames <count>] (every
                    frame from --capture on is compared; --dump <i,j> also writes those frames whole)
                    [--set name=value[@frame],…] writes over the car's state (lights flash brake gas gear rpm kmh
                    damage=f:r:l:r:c pit kers dirt fuel turbo water limiter)
                    other video.ini values than the Task 20 profile's: [--shadow-size <n>] [--cubemap-size <n>]
                    [--world-detail <n>] [--anisotropic <n>] [--cubemap-faces <0..6>] [--cubemap-far <m>]
                    [--sun <SUN_ANGLE, degrees; default -16>] [--weather <folder of content/weather; default 3_clear>]
     common: [--acs <path to acs.exe>] [--root <scratch game folder>] [--out <folder>] [--size WxH] [--verbose]";

pub fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().expect("the repository root")
}

pub struct Args {
    pub command: String,
    pub acs: PathBuf,
    pub game: PathBuf,
    pub root: PathBuf,
    pub out: PathBuf,
    pub width: u32,
    pub height: u32,
    pub verbose: bool,
    pub side: String,
    pub track: Option<String>,
    pub car: Option<String>,
    pub skin: Option<String>,
    pub pose: Option<PathBuf>,
    /// a tape of `car_oracle run --audio-tape`: the game's own car states, 60 a second
    pub tape: Option<PathBuf>,
    pub tape_from: usize,
    /// how many frames of the tape
    pub frames: usize,
    /// frames of a sequence whose whole log and picture are written (not only their hashes)
    pub dump: Vec<usize>,
    /// a word for the frame's name (to tell runs with different inputs apart)
    pub label: String,
    /// `--set name=value[@frame],…`: values written over the car's state of every frame (from
    /// `frame` on), on both sides alike
    pub set: Vec<(String, String, usize)>,
    pub layout: String,
    pub view: String,
    pub capture: usize,
    pub sun_angle: f32,
    pub weather: String,
    pub loose: bool,
    /// `compare` runs the two sides one after the other
    pub serial: bool,
    /// the port reads textures with its own reader, not with d3dx11_43.dll
    pub own_textures: bool,
    /// the options of the frame, to hand on to the two child processes of `compare`
    pub frame_options: Vec<String>,
}

fn parse() -> Result<Args, String> {
    let mut list = std::env::args().skip(1);
    let command = list.next().ok_or(USAGE)?;
    let repo = repo_root();
    let game = rustyac_content::install::ac_root().ok_or_else(rustyac_content::install::not_found_hint)?;
    let mut a = Args {
        command,
        acs: game.join("acs.exe"),
        game,
        root: repo.join("re/scratch/render_oracle/root"),
        out: repo.join("oracle/render"),
        width: 1280,
        height: 720,
        verbose: false,
        side: String::new(),
        track: None,
        car: None,
        skin: None,
        pose: None,
        tape: None,
        tape_from: 0,
        frames: 60,
        dump: Vec::new(),
        label: String::new(),
        set: Vec::new(),
        layout: String::new(),
        view: "chase".into(),
        capture: 1,
        sun_angle: -16.0,
        weather: "3_clear".into(),
        loose: false,
        serial: false,
        own_textures: false,
        frame_options: Vec::new(),
    };
    let mut profile = root::TASK_20;
    while let Some(flag) = list.next() {
        let mut value = || list.next().ok_or(format!("{flag} needs a value"));
        let mut frame_option = true;
        let mut text = String::new();
        match flag.as_str() {
            "--acs" => {
                a.acs = PathBuf::from(value()?);
                frame_option = false;
            }
            "--root" => {
                a.root = PathBuf::from(value()?);
                frame_option = false;
            }
            "--out" => {
                // absolute: the game's side changes the current directory
                let given = PathBuf::from(value()?);
                let absolute = if given.is_absolute() { given } else { std::env::current_dir().map_err(|e| e.to_string())?.join(given) };
                text = absolute.to_string_lossy().into_owned();
                a.out = absolute;
            }
            "--size" => {
                text = value()?;
                let (w, h) = text.split_once('x').ok_or("--size WxH")?;
                a.width = w.parse().map_err(|_| "--size WxH")?;
                a.height = h.parse().map_err(|_| "--size WxH")?;
            }
            "--side" => {
                a.side = value()?;
                frame_option = false;
            }
            "--track" => {
                text = value()?;
                a.track = Some(text.clone());
            }
            "--car" => {
                text = value()?;
                a.car = Some(text.clone());
            }
            "--skin" => {
                text = value()?;
                a.skin = Some(text.clone());
            }
            "--pose" => {
                let given = PathBuf::from(value()?);
                let absolute = if given.is_absolute() { given } else { std::env::current_dir().map_err(|e| e.to_string())?.join(given) };
                text = absolute.to_string_lossy().into_owned();
                a.pose = Some(absolute);
            }
            "--tape" => {
                let given = PathBuf::from(value()?);
                let absolute = if given.is_absolute() { given } else { std::env::current_dir().map_err(|e| e.to_string())?.join(given) };
                text = absolute.to_string_lossy().into_owned();
                a.tape = Some(absolute);
            }
            "--tape-from" => {
                text = value()?;
                a.tape_from = text.parse().map_err(|_| "--tape-from <frame>")?;
            }
            "--frames" => {
                text = value()?;
                a.frames = text.parse().map_err(|_| "--frames <count>")?;
            }
            "--label" => {
                text = value()?;
                a.label = text.clone();
            }
            "--set" => {
                text = value()?;
                for item in text.split(',').filter(|s| !s.is_empty()) {
                    let (item, from) = match item.split_once('@') {
                        Some((item, from)) => (item, from.parse().map_err(|_| format!("--set {item}@<frame>"))?),
                        None => (item, 0usize),
                    };
                    let (name, value) = item.split_once('=').ok_or("--set name=value[@frame],…")?;
                    a.set.push((name.to_string(), value.to_string(), from));
                }
            }
            "--dump" => {
                let list = value()?;
                a.dump = list.split(',').filter(|s| !s.is_empty()).map(|s| s.parse().map_err(|_| "--dump <frame,frame,...>".to_string())).collect::<Result<_, _>>()?;
                frame_option = false;
            }
            "--layout" => {
                text = value()?;
                a.layout = if text == "-" { String::new() } else { text.clone() };
            }
            "--view" => {
                text = value()?;
                a.view = text.clone();
            }
            "--capture" => {
                text = value()?;
                a.capture = text.parse().map_err(|_| "--capture <n>")?;
            }
            "--shadow-size" | "--cubemap-size" | "--world-detail" | "--anisotropic" | "--cubemap-faces" | "--smoke" | "--mirror" => {
                text = value()?;
                let number: i32 = text.parse().map_err(|_| format!("{flag} <number>"))?;
                match flag.as_str() {
                    "--shadow-size" => profile.shadow_map_size = number,
                    "--cubemap-size" => profile.cubemap_size = number,
                    "--world-detail" => profile.world_detail = number,
                    "--smoke" => profile.smoke = number,
                    "--mirror" => profile.mirror_size = number,
                    // Sim::initCubemaps 0x1401997a0: 0 to 6
                    "--cubemap-faces" => profile.cubemap_faces_per_frame = number.clamp(0, 6),
                    _ => profile.anisotropic = number,
                }
            }
            "--cubemap-far" => {
                text = value()?;
                profile.cubemap_far_plane = text.parse().map_err(|_| "--cubemap-far <metres>")?;
            }
            "--sun" => {
                text = value()?;
                a.sun_angle = text.parse().map_err(|_| "--sun <degrees>")?;
            }
            "--weather" => {
                text = value()?;
                a.weather = text.clone();
            }
            "--own-textures" => {
                a.own_textures = true;
            }
            "--loose" => {
                a.loose = true;
                frame_option = false;
            }
            "--mirror-smoke" => {
                profile.mirror_smoke = true;
            }
            "--virtual-mirror" => {
                profile.virtual_mirror = true;
            }
            "--serial" => {
                a.serial = true;
                frame_option = false;
            }
            "--verbose" => {
                a.verbose = true;
                frame_option = false;
            }
            other => return Err(format!("unknown option {other}\n{USAGE}")),
        }
        if frame_option {
            a.frame_options.push(flag.clone());
            if !text.is_empty() {
                a.frame_options.push(text);
            }
        }
    }
    root::set_profile(profile);
    Ok(a)
}

fn write_png(path: &std::path::Path, width: u32, height: u32, rgba: &[u8]) -> Result<(), String> {
    let file = std::fs::File::create(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().map_err(|e| e.to_string())?;
    writer.write_image_data(rgba).map_err(|e| e.to_string())
}

fn run_side(args: &Args) -> Result<(), String> {
    root::set_documents(&args.root);
    let frame = frames::build(args)?;
    std::fs::create_dir_all(&args.out).map_err(|e| e.to_string())?;
    let (init, rendered) = match args.side.as_str() {
        "ac" => {
            root::prepare(args)?;
            if let Some(car) = &frame.car {
                ac_car::prepare_root(&args.root, &args.game, &repo_root(), car)?;
            }
            std::env::set_current_dir(&args.root).map_err(|e| format!("{}: {e}", args.root.display()))?;
            let game = ac::Game::start(args)?;
            let rendered = unsafe { game.render(&frame, &args.dump)? };
            (game.init_log.clone(), rendered)
        }
        "port" => {
            rustyac_render::texture::force_fallback(args.own_textures);
            port::render(args, &frame)?
        }
        other => return Err(format!("--side {other:?} is not ac or port")),
    };
    let file = |kind: &str| args.out.join(format!("{}.{}.{kind}", frame.name, args.side));
    std::fs::write(file("init.gpulog"), &init).map_err(|e| e.to_string())?;
    std::fs::write(file("cube.gpulog"), &rendered.cube_log).map_err(|e| e.to_string())?;
    if frame.sequence {
        // one line per frame, and the whole of the frames asked for
        let mut list = String::new();
        for f in &rendered.frames {
            list.push_str(&format!("{} calls={} draws={} log={:016x} pixels={:016x}\n", f.index, f.calls, f.draws, f.log_hash, f.pixels_hash));
            if let (Some(log), Some(pixels)) = (&f.log, &f.pixels) {
                std::fs::write(file(&format!("f{}.gpulog", f.index)), log).map_err(|e| e.to_string())?;
                std::fs::write(file(&format!("f{}.rgba", f.index)), pixels).map_err(|e| e.to_string())?;
                write_png(&file(&format!("f{}.png", f.index)), rendered.width, rendered.height, pixels)?;
            }
        }
        std::fs::write(file("seq"), list).map_err(|e| e.to_string())?;
        println!("{}: {} frames", file("seq").display(), rendered.frames.len());
        return Ok(());
    }
    let f = rendered.frames.last().ok_or("no frame was captured")?;
    let (log, pixels) = (f.log.as_ref().ok_or("no log")?, f.pixels.as_ref().ok_or("no pixels")?);
    std::fs::write(file("gpulog"), log).map_err(|e| e.to_string())?;
    std::fs::write(file("rgba"), pixels).map_err(|e| e.to_string())?;
    write_png(&file("png"), rendered.width, rendered.height, pixels)?;
    println!("{}: {} draw calls, {} log lines, {}x{}", file("").display(), f.draws, f.calls, rendered.width, rendered.height);
    Ok(())
}

fn run() -> Result<(), String> {
    let args = parse()?;
    match args.command.as_str() {
        "run" => run_side(&args),
        "compare" => compare::compare(&args),
        _ => Err(USAGE.into()),
    }
}

fn main() {
    if let Err(message) = run() {
        eprintln!("{message}");
        std::process::exit(1);
    }
}
