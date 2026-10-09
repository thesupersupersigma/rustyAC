// SPDX-License-Identifier: GPL-3.0-or-later

//! Render oracle: Assetto Corsa's own renderer (run from acs.exe in this process, on a WARP
//! device, off screen) against the Rust port in `crates/rustyac-render`.
//! See `docs/port/renderer_core.md`.

#[path = "../../car_oracle/src/acs.rs"]
mod acs;
mod ac;
mod compare;
mod frames;
mod port;
mod root;

use std::path::PathBuf;

const USAGE: &str = "usage:
  render_oracle run --side ac|port [frame options]   render one frame with one side; writes
                                                     <out>/<frame>.<side>.gpulog / .init.gpulog / .png
  render_oracle compare [frame options] [--loose]    both sides (a process each), then the logs line
                                                     by line and the pictures byte by byte
     frame options: [--track <folder name>] [--layout <l>] [--view chase|cockpit|free] [--capture <n>]
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
    pub layout: String,
    pub view: String,
    pub capture: usize,
    pub loose: bool,
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
        layout: String::new(),
        view: "chase".into(),
        capture: 1,
        loose: false,
        frame_options: Vec::new(),
    };
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
            "--loose" => {
                a.loose = true;
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
    let frame = frames::build(args)?;
    std::fs::create_dir_all(&args.out).map_err(|e| e.to_string())?;
    let (init, rendered) = match args.side.as_str() {
        "ac" => {
            root::prepare(args)?;
            std::env::set_current_dir(&args.root).map_err(|e| format!("{}: {e}", args.root.display()))?;
            let game = ac::Game::start(args)?;
            let rendered = unsafe { game.render(&frame)? };
            (game.init_log.clone(), rendered)
        }
        "port" => port::render(args, &frame)?,
        other => return Err(format!("--side {other:?} is not ac or port")),
    };
    let base = args.out.join(format!("{}.{}", frame.name, args.side));
    std::fs::write(base.with_extension(format!("{}.gpulog", args.side)), &rendered.log).map_err(|e| e.to_string())?;
    std::fs::write(base.with_extension(format!("{}.init.gpulog", args.side)), &init).map_err(|e| e.to_string())?;
    std::fs::write(base.with_extension(format!("{}.rgba", args.side)), &rendered.pixels).map_err(|e| e.to_string())?;
    write_png(&base.with_extension(format!("{}.png", args.side)), rendered.width, rendered.height, &rendered.pixels)?;
    println!("{}: {} draw calls, {} log lines, {}x{}", base.display(), rendered.draws, rendered.log.iter().filter(|b| **b == b'\n').count(), rendered.width, rendered.height);
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
