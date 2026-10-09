// SPDX-License-Identifier: GPL-3.0-or-later

//! Render oracle: Assetto Corsa's own renderer (run from acs.exe in this process, on a WARP
//! device, off screen) against the Rust port in `crates/rustyac-render`.
//! See `docs/port/renderer_core.md`.

#[path = "../../car_oracle/src/acs.rs"]
mod acs;
mod ac;
mod root;

use std::path::PathBuf;

const USAGE: &str = "usage:
  render_oracle probe [--verbose]            build the game's GraphicsManager on WARP and stop
     common: [--acs <path to acs.exe>] [--root <scratch game folder>] [--size WxH]";

pub fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().expect("the repository root")
}

pub struct Args {
    pub command: String,
    pub acs: PathBuf,
    pub game: PathBuf,
    pub root: PathBuf,
    pub width: u32,
    pub height: u32,
    pub verbose: bool,
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
        width: 1280,
        height: 720,
        verbose: false,
    };
    while let Some(flag) = list.next() {
        let mut value = || list.next().ok_or(format!("{flag} needs a value"));
        match flag.as_str() {
            "--acs" => a.acs = PathBuf::from(value()?),
            "--root" => a.root = PathBuf::from(value()?),
            "--size" => {
                let v = value()?;
                let (w, h) = v.split_once('x').ok_or("--size WxH")?;
                a.width = w.parse().map_err(|_| "--size WxH")?;
                a.height = h.parse().map_err(|_| "--size WxH")?;
            }
            "--verbose" => a.verbose = true,
            other => return Err(format!("unknown option {other}\n{USAGE}")),
        }
    }
    Ok(a)
}

fn run() -> Result<(), String> {
    let args = parse()?;
    match args.command.as_str() {
        "probe" => {
            root::prepare(&args)?;
            std::env::set_current_dir(&args.root).map_err(|e| format!("{}: {e}", args.root.display()))?;
            let game = ac::Game::start(&args)?;
            println!("GraphicsManager at {:p}", game.graphics);
            Ok(())
        }
        _ => Err(USAGE.into()),
    }
}

fn main() {
    if let Err(message) = run() {
        eprintln!("{message}");
        std::process::exit(1);
    }
}
