// SPDX-License-Identifier: GPL-3.0-or-later

//! Test oracle for `rustyac-audio`.
//!
//! audio_oracle run --side ac|port --tape <file.audiotape> [--camera cockpit|chase|trackside]
//!                  [--out <dir>] [--frames <n>] [--no-track]
//!     One side of a drive, in this process: the game's own sound code (`ac`, acs.exe mapped
//!     and called by address) or the Rust port. Writes `<drive>_<camera>_<side>.log` (every
//!     FMOD call) and `.wav` (FMOD's non-real-time writer; nothing reaches the speakers).
//! audio_oracle compare --tape <file> [--camera ...] [--frames <n>] [--out <dir>]
//!     Runs the game's side twice and the port once (three processes) and compares the logs
//!     line by line and the WAVs byte by byte.
//! audio_oracle dsp [--count <n>] [--seed <n>]
//!     The two DSP plug-ins: the game's own callbacks against the port's on random buffers.
//! audio_oracle survey [--out <file.md>]
//!     Every installed car and track: is there a bank, does FMOD 1.08.12 load it, which
//!     events does the sound find.
//!
//! common: [--acs <path to acs.exe>] [--root <scratch game folder>]

#[allow(dead_code)]
#[path = "../../car_oracle/src/acs.rs"]
mod acs;
mod drive;
mod dsp_check;
mod harness;
mod survey;

use std::path::{Path, PathBuf};

use drive::{Camera, Drive, BLOCK, DT, SAMPLE_RATE};
use rustyac_audio::car::{CarInfo, SimView};
use rustyac_audio::engine::{AudioEngine, EngineFiles};
use rustyac_audio::fmod::{log, raw};
use rustyac_audio::sim::{AudioWorld, CarSound, FrameInput, PhysicsEvent};
use rustyac_audio::track::{cache_surface_sounds, Scene, TrackAudio};

const DEFAULT_ACS: &str = r"C:\Program Files (x86)\Steam\steamapps\common\assettocorsa\acs.exe";

pub fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().and_then(Path::parent).expect("the repository folder").to_path_buf()
}

pub struct Args {
    command: String,
    side: String,
    tape: Option<PathBuf>,
    camera: Camera,
    out: PathBuf,
    frames: Option<usize>,
    no_track: bool,
    pub acs: PathBuf,
    root: PathBuf,
    pub count: usize,
    pub seed: u64,
    pub out_file: Option<PathBuf>,
    pub only: Option<String>,
    /// `run --script <log>`: the port is told what that run was told about its events' states.
    script: Option<PathBuf>,
}

fn parse() -> Result<Args, String> {
    let mut it = std::env::args().skip(1);
    let command = it.next().ok_or("usage: audio_oracle run|compare|dsp|survey ... (see the top of src/main.rs)")?;
    let repo = repo_root();
    let mut a = Args {
        command,
        side: "port".to_string(),
        tape: None,
        camera: Camera::Cockpit,
        out: repo.join("oracle/audio"),
        frames: None,
        no_track: false,
        acs: PathBuf::from(DEFAULT_ACS),
        root: repo.join("re/scratch/task19/root"),
        count: 200,
        seed: 1,
        out_file: None,
        only: None,
        script: None,
    };
    while let Some(arg) = it.next() {
        let mut value = || it.next().ok_or_else(|| format!("{arg} needs a value"));
        match arg.as_str() {
            "--side" => a.side = value()?,
            "--tape" => a.tape = Some(PathBuf::from(value()?)),
            "--camera" => a.camera = Camera::parse(&value()?)?,
            "--out" => a.out = PathBuf::from(value()?),
            "--frames" => a.frames = Some(value()?.parse().map_err(|_| "--frames needs a number")?),
            "--no-track" => a.no_track = true,
            "--acs" => a.acs = PathBuf::from(value()?),
            "--root" => a.root = PathBuf::from(value()?),
            "--count" => a.count = value()?.parse().map_err(|_| "--count needs a number")?,
            "--seed" => a.seed = value()?.parse().map_err(|_| "--seed needs a number")?,
            "--out-file" => a.out_file = Some(PathBuf::from(value()?)),
            "--only" => a.only = Some(value()?),
            "--script" => a.script = Some(PathBuf::from(value()?)),
            other => return Err(format!("unknown option {other}")),
        }
    }
    Ok(a)
}

fn main() {
    let result = parse().and_then(|args| match args.command.as_str() {
        "run" => run(&args),
        "compare" => compare(&args),
        "dsp" => dsp_check::run(&args),
        "survey" => survey::run(&args),
        other => Err(format!("unknown command {other}")),
    });
    if let Err(message) = result {
        eprintln!("audio_oracle: {message}");
        std::process::exit(1);
    }
}

pub fn ac_folder(args: &Args) -> PathBuf {
    args.acs.parent().unwrap_or(Path::new(".")).to_path_buf()
}

fn file_stem(drive: &Drive, side: &str) -> String {
    format!("{}_{}_{}_{side}", drive.car, drive.name, drive.camera.name())
}

/// One side of a drive.
fn run(args: &Args) -> Result<(), String> {
    let tape = args.tape.as_ref().ok_or("run needs --tape <file.audiotape>")?;
    let tape = std::path::absolute(tape).map_err(|e| e.to_string())?;
    let mut drive = Drive::load(&tape, args.camera, args.frames)?;
    if args.no_track {
        drive.track_folder = None;
    }
    let out = std::path::absolute(&args.out).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
    let ac = ac_folder(args);
    let repo = repo_root();
    let root = std::path::absolute(&args.root).map_err(|e| e.to_string())?;
    drive::prepare_root(&root, &repo, &ac, &drive)?;
    let stem = file_stem(&drive, if args.script.is_some() { "scripted" } else { &args.side });
    let wav = out.join(format!("{stem}.wav"));
    let log_file = out.join(format!("{stem}.log"));
    let scene = match &drive.track_folder {
        Some(folder) => Some(Scene::load(folder, &drive.layout)?),
        None => None,
    };
    let surfaces = drive::surfaces(&ac, &drive)?;
    let master = rustyac_physics::data::ini::IniReader::load(&root.join("cfg/audio.ini"))?.get_float("LEVELS", "MASTER")?;

    match args.side.as_str() {
        "ac" => {
            // the game's code reads its files by relative path
            std::env::set_current_dir(&root).map_err(|e| format!("{}: {e}", root.display()))?;
            let imports: Vec<(&'static str, &'static str, usize)> = raw::imports()
                .into_iter()
                .map(|(symbol, function)| (if symbol.contains("@Studio@") { "fmodstudio64.dll" } else { "fmod64.dll" }, symbol, function))
                .collect();
            acs::set_extra_overrides(imports);
            let acs = acs::Acs::load(&args.acs)?;
            acs.silence_game_stdout();
            raw::load(&ac, raw::Output::WavNrt { file: wav.clone(), rate: SAMPLE_RATE, block: BLOCK })?;
            log::start(Some(&log_file))?;
            harness::run(&acs, &ac, &drive, scene.as_ref(), &drive::surface_cache_paths(&surfaces), master);
        }
        "port" => {
            raw::load(&ac, raw::Output::WavNrt { file: wav.clone(), rate: SAMPLE_RATE, block: BLOCK })?;
            match &args.script {
                Some(script) => log::start_scripted(Some(&log_file), script)?,
                None => log::start(Some(&log_file))?,
            }
            run_port(&ac, &root, &drive, scene.as_ref(), &surfaces, master)?;
        }
        other => return Err(format!("--side is ac or port, not {other}")),
    }
    let summary = log::finish().ok_or("no log")?;
    println!("{stem} frames={} lines={} hash={:016x}", drive.frames.len(), summary.lines, summary.hash);
    Ok(())
}

/// The port's side: the same session through `rustyac_audio`.
fn run_port(ac: &Path, root: &Path, drive: &Drive, scene: Option<&Scene>, surfaces: &[(String, String)], master: f32) -> Result<(), String> {
    log::mark("setup");
    let files = EngineFiles { content_root: ac.to_path_buf(), audio_engine_ini: root.join("system/cfg/audio_engine.ini"), audio_ini: root.join("cfg/audio.ini") };
    let mut engine = AudioEngine::new(files)?;
    engine.set_volume(master);
    let track = match (scene, drive.track_data_relative()) {
        (Some(scene), Some(relative)) => Some(TrackAudio::new(&mut engine, scene, &root.join(relative))?),
        _ => None,
    };
    cache_surface_sounds(&mut engine, surfaces);
    let info = CarInfo { unix_name: drive.car.clone(), guid: 0, data_folder: root.join("content/cars").join(&drive.car).join("data"), car_cameras_external_sound: Vec::new() };
    let car = CarSound::new(&mut engine, info)?;
    let mut world = AudioWorld { engine, track, cars: vec![car] };
    let sim = SimView { focused_car_index: 0, camera: Some(drive.camera.view()) };
    for (i, frame) in drive.frames.iter().enumerate() {
        log::mark(&format!("frame {i}"));
        let events: Vec<PhysicsEvent> = frame.events.iter().map(|raw| PhysicsEvent::from_bytes(raw)).collect();
        world.frame(&FrameInput { cars: std::slice::from_ref(&frame.car), events: &events, sim, listener: Some(frame.listener), dt: DT });
    }
    log::mark("teardown");
    let AudioWorld { mut engine, track, cars } = world;
    for car in cars {
        if let Some(audio) = car.audio {
            audio.destroy(&mut engine);
        }
    }
    if let Some(track) = track {
        track.destroy(&mut engine);
    }
    drop(engine);
    Ok(())
}

fn child(args: &Args, side: &str, out: &Path, script: Option<&Path>) -> Result<String, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let mut command = std::process::Command::new(exe);
    command.arg("run").arg("--side").arg(side).arg("--tape").arg(args.tape.as_ref().unwrap());
    command.arg("--camera").arg(args.camera.name()).arg("--out").arg(out).arg("--acs").arg(&args.acs);
    command.arg("--root").arg(&args.root);
    if let Some(frames) = args.frames {
        command.arg("--frames").arg(frames.to_string());
    }
    if args.no_track {
        command.arg("--no-track");
    }
    if let Some(script) = script {
        command.arg("--script").arg(script);
    }
    let output = command.output().map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err(format!("the {side} side failed ({}):\n{}", output.status, String::from_utf8_lossy(&output.stderr)));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// First line where two logs differ, with some lines around it.
fn first_difference(a: &str, b: &str) -> Option<String> {
    let (la, lb): (Vec<&str>, Vec<&str>) = (a.lines().collect(), b.lines().collect());
    let n = la.len().min(lb.len());
    let at = (0..n).find(|&i| la[i] != lb[i]).or((la.len() != lb.len()).then_some(n))?;
    let mark = (0..at.min(la.len())).rev().find(|&i| la[i].starts_with('#')).map(|i| la[i]).unwrap_or("");
    let mut text = format!("line {} (after \"{mark}\"; {} against {} lines)\n", at + 1, la.len(), lb.len());
    for i in at.saturating_sub(3)..(at + 3).min(la.len().max(lb.len())) {
        let (x, y) = (la.get(i).copied().unwrap_or("<end>"), lb.get(i).copied().unwrap_or("<end>"));
        if x == y {
            text.push_str(&format!("      {x}\n"));
        } else {
            text.push_str(&format!("  ac: {x}\nport: {y}\n"));
        }
    }
    Some(text)
}

/// The 16-bit samples of a WAV file.
fn samples(wav: &[u8]) -> Vec<i16> {
    let start = wav.windows(4).position(|w| w == b"data").map(|at| at + 8).unwrap_or(wav.len());
    wav[start..].chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]])).collect()
}

/// How far two mixes are apart: the largest difference of their levels over windows of a
/// second (dB, only where one of them is above -60 dB), and the level of their difference
/// relative to the first one (dB; minus infinity for equal files).
fn apart(a: &[i16], b: &[i16]) -> (f64, f64) {
    let window = (drive::SAMPLE_RATE as usize) * 2;
    let level = |x: &[i16]| -> f64 {
        let sum: f64 = x.iter().map(|s| (*s as f64) * (*s as f64)).sum();
        10.0 * (sum / x.len().max(1) as f64 / (32768.0 * 32768.0)).max(1e-12).log10()
    };
    let n = a.len().min(b.len());
    let mut worst = 0.0f64;
    for start in (0..n).step_by(window) {
        let end = (start + window).min(n);
        let (la, lb) = (level(&a[start..end]), level(&b[start..end]));
        if la.max(lb) > -60.0 {
            worst = worst.max((la - lb).abs());
        }
    }
    let difference: f64 = a[..n].iter().zip(&b[..n]).map(|(x, y)| ((*x as f64) - (*y as f64)).powi(2)).sum();
    let signal: f64 = a[..n].iter().map(|x| (*x as f64).powi(2)).sum();
    (worst, 10.0 * (difference / signal.max(1.0)).max(1e-12).log10())
}

/// Both sides of a drive, compared.
fn compare(args: &Args) -> Result<(), String> {
    let tape = args.tape.as_ref().ok_or("compare needs --tape <file.audiotape>")?;
    let mut drive = Drive::load(tape, args.camera, args.frames)?;
    if args.no_track {
        drive.track_folder = None;
    }
    let out = std::path::absolute(&args.out).map_err(|e| e.to_string())?;
    let out2 = out.join("second");
    // the game's side twice (is FMOD the same from run to run?), then the port twice: left to
    // itself, and told what the game's first run was told about its events' states
    child(args, "ac", &out, None)?;
    child(args, "ac", &out2, None)?;
    child(args, "port", &out, None)?;
    let ac_log_file = out.join(format!("{}.log", file_stem(&drive, "ac")));
    child(args, "port", &out, Some(&ac_log_file))?;
    let read = |dir: &Path, side: &str, ext: &str| std::fs::read(dir.join(format!("{}.{ext}", file_stem(&drive, side)))).map_err(|e| e.to_string());
    let ac_log = String::from_utf8_lossy(&read(&out, "ac", "log")?).into_owned();
    let ac_log2 = String::from_utf8_lossy(&read(&out2, "ac", "log")?).into_owned();
    let port_log = String::from_utf8_lossy(&read(&out, "port", "log")?).into_owned();
    let scripted_log = String::from_utf8_lossy(&read(&out, "scripted", "log")?).into_owned();
    let ac_wav = read(&out, "ac", "wav")?;
    let ac_wav2 = read(&out2, "ac", "wav")?;
    let port_wav = read(&out, "port", "wav")?;
    let calls = ac_log.lines().filter(|l| !l.starts_with('#')).count();
    let log_same = ac_log == scripted_log;
    let free_same = ac_log == port_log;
    let log_repeat = ac_log == ac_log2;
    let wav_repeat = ac_wav == ac_wav2;
    let wav_same = ac_wav == port_wav;
    let (sa, sa2, sp) = (samples(&ac_wav), samples(&ac_wav2), samples(&port_wav));
    let describe = |same: bool, a: &[i16], b: &[i16]| {
        if same {
            "identical".to_string()
        } else {
            let (level, difference) = apart(a, b);
            format!("levels within {level:.2} dB, difference {difference:.1} dB")
        }
    };
    let wav_ok = wav_same || (!wav_repeat && ac_wav.len() == port_wav.len() && apart(&sa, &sp).0 <= (apart(&sa, &sa2).0 * 2.0).max(1.5));
    let word = |same: bool| if same { "identical" } else { "differs" };
    println!(
        "{} | {} | {} | {} | {} | {} | {} | {} | {} | {:.1} s | {} | {} | {}",
        drive.car,
        drive.track_folder.as_ref().and_then(|f| f.file_name()).map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "-".to_string()),
        drive.name,
        drive.camera.name(),
        drive.frames.len(),
        calls,
        if log_same { "identical" } else { "DIFFERENT" },
        word(log_repeat),
        word(free_same),
        sa.len() as f64 / 2.0 / drive::SAMPLE_RATE as f64,
        describe(wav_repeat, &sa, &sa2),
        describe(wav_same, &sa, &sp),
        if log_same && wav_ok { "ok" } else { "FAIL" },
    );
    if !log_same {
        if let Some(text) = first_difference(&ac_log, &scripted_log) {
            println!("{text}");
        }
    }
    // the recordings and logs are big: only a failing drive keeps its files
    if log_same && wav_ok && std::env::var_os("AUDIO_ORACLE_KEEP").is_none() {
        for side in ["ac", "port", "scripted"] {
            for ext in ["log", "wav"] {
                let _ = std::fs::remove_file(out.join(format!("{}.{ext}", file_stem(&drive, side))));
            }
        }
    }
    // the second run's files are only for the comparison
    let _ = std::fs::remove_dir_all(&out2);
    if log_same && wav_ok {
        Ok(())
    } else {
        Err("the game and the port do not agree".to_string())
    }
}
