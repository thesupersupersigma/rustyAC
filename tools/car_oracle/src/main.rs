// SPDX-License-Identifier: GPL-3.0-or-later

//! Whole-car test oracle: Assetto Corsa's own `PhysicsEngine` and one `Car` (constructed,
//! loaded and stepped by the game's code, mapped from acs.exe; the game itself never starts)
//! on a fake flat track, driven by scripted controls, with every force handed to a rigid
//! body and the full body / joint / tyre state recorded each step.
//!
//! car_oracle list
//! car_oracle run --scenario <name> [--out <dir>] [--steps <n>] [--floor] [--collide] [--no-joint-forces] [--hash-only]
//!                [--car <folder under cardata/>]
//!     One scenario in this process. Prints `<name> steps=… bytes=… hash=…`. `--floor` adds a
//!     collision mesh under the car (file `<name>_floor.carrec`).
//! car_oracle all [--out <dir>] [--only <name,name>] [--steps <n>]
//!     Every scenario twice (fresh process each time), compares the two runs byte for byte,
//!     replays the tyres through the Rust port, and writes `<out>/results.md`.
//! car_oracle check <recording>
//!     Tyre cross-check and headline numbers of one recording.
//! car_oracle setup-check [--car <name>]
//!     Builds the car, does what the game's setup screen does at a session start and lets the
//!     game's own code print every value that changes ("Setup change for Car: …"), for
//!     comparing with a real game's log.txt.
//! car_oracle diff <recording> <recording> [--only <prefix,prefix>] [--ignore <part,part>]
//!     Value-by-value comparison of two recordings (fields and force tape).
//! car_oracle csv <recording> [--table steps|tape|telemetry] [--from <step>] [--to <step>]
//!                [--every <n>] [--only <prefix,prefix>] [--csv-out <file>]
//!     Full-precision CSV of a recording.
//! car_oracle rays --track <track folder> [--count <n>] [--seed <n>]
//!     The ray micro-oracle: the track's meshes in the game's own ODE and in the Rust port,
//!     `count` rays (default 1,000,000) through both, every answer compared bit for bit.
//!     Writes `oracle/track/rays_results.md`.
//! car_oracle collide --track <track folder> [--count <n>] [--mesh-count <n>] [--seed <n>] [--kind <n>]
//!     The collision micro-oracle: the car's floor boxes (`count` poses, default 200,000) and
//!     its collider mesh (`mesh-count` poses, default 50,000) over the track, in the game's
//!     own ODE / OPCODE / PhysicsCore and in the Rust port; every contact compared bit for
//!     bit. Writes `oracle/collide/poses_results.md`.
//! car_oracle collide-worlds [--steps <n>] [--seed <n>] [--only <world,world>] [--hybrid]
//!     Small worlds with contacts, stepped on both sides (default 1,500 steps). Writes
//!     `oracle/collide/worlds_results.md`.

mod acs;
mod check;
mod collide;
mod game;
mod record;
mod scenario;
mod sites;
mod track;
mod track_driver;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use record::{CsvOptions, Recording, Writer};

const DEFAULT_ACS: &str = r"C:\Program Files (x86)\Steam\steamapps\common\assettocorsa\acs.exe";

fn usage() -> String {
    "usage: car_oracle list\n       \
     car_oracle run --scenario <name> [--track <track folder or name>] [--out <dir>] [--steps <n>] [--floor] [--no-joint-forces] [--hash-only] [--setup <saved setup .ini>]\n       \
     car_oracle all [--out <dir>] [--only <name,name>] [--steps <n>]\n       \
     car_oracle check <recording>\n       \
     car_oracle setup-check [--car <name>]\n       \
     car_oracle diff <recording> <recording> [--only <prefix,prefix>] [--ignore <part,part>]\n       \
     car_oracle csv <recording> [--table steps|tape|telemetry] [--from <step>] [--to <step>] [--every <n>] \
     [--only <prefix,prefix>] [--csv-out <file>]\n       \
     car_oracle rays --track <track folder or name> [--count <n>] [--seed <n>]\n       \
     car_oracle collide --track <track folder or name> [--count <n>] [--mesh-count <n>] [--seed <n>] [--kind <n>]\n       \
     car_oracle collide-worlds [--steps <n>] [--seed <n>] [--only <world,world>]\n       \
     car_oracle multi --scenario <mc_...> --track <track> [--cars <car,car,...>] [--steps <n>] [--out <dir>] [--game-threads <n>] [--threads <n,n>]\n       \
     car_oracle net <tape.nettape> --scenario net_watch|net_block|net_drive|net_pit --track <track> [--steps <n>] [--fault <step>:<kind>] [--csv-out <golden>]\n       \
     car_oracle multi-check <recording.mcrec> [--threads <n,n,...>] [--fault <step>:<car>]\n       \
     car_oracle multi-list\n       \
     common: [--acs <path to acs.exe>] [--root <scratch game folder>] [--car <folder under cardata/>] [--verbose]"
        .to_string()
}

fn repo_root() -> PathBuf {
    // tools/car_oracle -> the repository (no canonicalize: it would turn the path into a \\?\ one)
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().and_then(Path::parent).expect("the repository folder").to_path_buf()
}

struct Args {
    command: String,
    file: Option<PathBuf>,
    /// `race-ini --pit-slots <n>`: what `TrackAvatar::getMaxSlotsAvailable("PIT")` answers
    pit_slots: i32,
    file2: Option<PathBuf>,
    scenario: String,
    out: PathBuf,
    steps: Option<usize>,
    joint_forces: bool,
    floor: bool,
    /// `--damage f,r,l,r,c`: the damage levels of the bodywork the car starts with (km/h of
    /// closing speed per zone: front, rear, left, right, centre), instead of the scenario's.
    damage: Option<[f32; 5]>,
    hash_only: bool,
    /// Task 19: also write `<scenario>.audiotape` (the game's own `CarPhysicsState` at 60
    /// frames a second and every physics event), the input of `tools/audio_oracle`.
    audio_tape: bool,
    /// Task 28c: `multi --replay`: the game's replay code records beside the port's, then
    /// both save, load and play (`--replay-frames <n>`: the ring's size, `--replay-level
    /// <0..4>`: replay.ini's quality level)
    replay: bool,
    /// Task 28c: `multi --ghost`: the game's GhostCarRecorder beside the port's on the first car
    ghost: bool,
    /// Task 44: `multi --ai-record`: the game's AISplineRecorder beside the port's on the first car
    ai_record: bool,
    /// Task 28c: `multi --telemetry`: the game's ComparativeTelemetryRecorder beside the port's
    telemetry: bool,
    replay_frames: Option<i32>,
    replay_level: Option<i32>,
    only: Vec<String>,
    ignore: Vec<String>,
    table: String,
    from: usize,
    to: usize,
    every: usize,
    csv_out: Option<PathBuf>,
    acs: PathBuf,
    root: PathBuf,
    car: String,
    verbose: bool,
    track: Option<PathBuf>,
    /// `--layout <name>`: the track's layout ("" for a track without layouts).
    layout: String,
    /// `rays --space0 <n>`: every n-th mesh in sub-space id 0. `rays --late <percent>`: the
    /// last so many percent of the meshes are added after a first ray.
    space0: usize,
    late: Option<usize>,
    /// `--setup <file>`: the saved setup a Task 15 scenario loads.
    setup: Option<PathBuf>,
    count: usize,
    count_given: bool,
    mesh_count: usize,
    kind: Option<usize>,
    hybrid: bool,
    boxes_only: bool,
    collide: bool,
    seed: u64,
    /// Task 24, `multi`: the cars (`--cars a,b,c`), the worker threads of the port's replays
    /// (`--threads 0,4`), `[PHYSICS_THREADING] THREADS` of the game's own engine
    /// (`--game-threads`), a one-bit fault (`--fault <step>:<car>`).
    cars: Vec<String>,
    threads: Vec<usize>,
    game_threads: i32,
    /// Task 27, `multi --ai-line <file.ai>`: this file as the track's `ai/fast_lane.ai` (a line
    /// of an old version: the game rebuilds it in the oracle's scratch folder, the port in memory)
    ai_line: Option<PathBuf>,
    fault: Option<(usize, usize)>,
}

fn parse_args() -> Result<Args, String> {
    let mut it = std::env::args().skip(1);
    let repo = repo_root();
    let mut a = Args {
        command: it.next().ok_or_else(usage)?,
        file: None,
        pit_slots: 24,
        file2: None,
        scenario: String::new(),
        out: repo.join("oracle/car"),
        steps: None,
        joint_forces: true,
        floor: false,
        damage: None,
        hash_only: false,
        audio_tape: false,
        replay: false,
        ghost: false,
        ai_record: false,
        telemetry: false,
        replay_frames: None,
        replay_level: None,
        only: Vec::new(),
        ignore: Vec::new(),
        table: "steps".into(),
        from: 0,
        to: usize::MAX,
        every: 1,
        csv_out: None,
        acs: PathBuf::from(DEFAULT_ACS),
        root: repo.join("re/scratch/car_oracle/root"),
        car: game::DEFAULT_CAR.to_string(),
        verbose: false,
        track: None,
        layout: String::new(),
        space0: 0,
        late: None,
        setup: None,
        count: 1_000_000,
        count_given: false,
        mesh_count: 50_000,
        kind: None,
        hybrid: false,
        boxes_only: false,
        collide: false,
        seed: 12,
        cars: Vec::new(),
        threads: Vec::new(),
        game_threads: 0,
        ai_line: None,
        fault: None,
    };
    while let Some(flag) = it.next() {
        let mut value = || it.next().ok_or_else(|| format!("{flag} needs a value\n{}", usage()));
        let number = |text: String| text.parse::<usize>().map_err(|_| format!("{text}: not a number"));
        match flag.as_str() {
            "--scenario" => a.scenario = value()?,
            "--out" => a.out = PathBuf::from(value()?),
            "--steps" => a.steps = Some(number(value()?)?),
            "--no-joint-forces" => a.joint_forces = false,
            "--floor" => a.floor = true,
            "--damage" => {
                let text = value()?;
                let levels: Vec<f32> = text.split(',').filter_map(|part| part.trim().parse().ok()).collect();
                a.damage = Some(levels.try_into().map_err(|_| format!("--damage wants five numbers, got {text:?}"))?);
            }
            "--pop" => {
                // Task 22b: <wheel 0..3 (LF RF LR RR)>@<seconds>
                let text = value()?;
                let parsed = text.split_once('@').and_then(|(w, t)| Some((w.parse::<i64>().ok()?, t.parse::<f64>().ok()?)));
                let (wheel, seconds) = parsed.filter(|(w, t)| (0..4).contains(w) && *t >= 0.0).ok_or(format!("--pop <wheel 0..3>@<seconds>, got {text:?}"))?;
                game::POP.store(wheel << 32 | (seconds * 333.0).round() as i64, std::sync::atomic::Ordering::Relaxed);
            }
            "--hash-only" => a.hash_only = true,
            "--audio-tape" => a.audio_tape = true,
            "--replay" => a.replay = true,
            "--ghost" => a.ghost = true,
            "--ai-record" => a.ai_record = true,
            "--telemetry" => a.telemetry = true,
            "--replay-frames" => a.replay_frames = Some(value()?.parse().map_err(|_| "--replay-frames needs a number")?),
            "--replay-level" => a.replay_level = Some(value()?.parse().map_err(|_| "--replay-level needs 0..4")?),
            "--only" => a.only = value()?.split(',').map(str::to_string).collect(),
            "--ignore" => a.ignore = value()?.split(',').map(str::to_string).collect(),
            "--table" => a.table = value()?,
            "--from" => a.from = number(value()?)?,
            "--to" => a.to = number(value()?)?,
            "--every" => a.every = number(value()?)?,
            "--csv-out" => a.csv_out = Some(PathBuf::from(value()?)),
            "--acs" => a.acs = PathBuf::from(value()?),
            "--root" => a.root = PathBuf::from(value()?),
            "--car" => a.car = value()?,
            "--verbose" => a.verbose = true,
            "--track" => a.track = Some(PathBuf::from(value()?)),
            "--layout" => {
                a.layout = value()?;
                if a.layout == "-" {
                    a.layout.clear();
                }
            }
            "--space0" => a.space0 = number(value()?)?,
            "--late" => a.late = Some(number(value()?)?),
            "--setup" => a.setup = Some(std::path::absolute(PathBuf::from(value()?)).map_err(|e| e.to_string())?),
            "--count" => {
                a.count = number(value()?)?;
                a.count_given = true;
            }
            "--mesh-count" => a.mesh_count = number(value()?)?,
            "--kind" => a.kind = Some(number(value()?)?),
            "--hybrid" => a.hybrid = true,
            "--boxes-only" => a.boxes_only = true,
            "--collide" => a.collide = true,
            "--seed" => a.seed = number(value()?)? as u64,
            "--pit-slots" => a.pit_slots = number(value()?)? as i32,
            "--cars" => a.cars = value()?.split(',').map(str::to_string).collect(),
            "--threads" => {
                let text = value()?;
                a.threads = text.split(',').map(|part| part.parse::<usize>().map_err(|_| format!("--threads {text}: not numbers"))).collect::<Result<_, _>>()?;
            }
            "--ai-line" => {
                a.ai_line = Some(PathBuf::from(value()?));
            }
            "--game-threads" => {
                let text = value()?;
                a.game_threads = text.parse().map_err(|_| format!("--game-threads {text}: not a number"))?;
            }
            "--fault" => {
                let text = value()?;
                a.fault = Some(text.split_once(':').and_then(|(s, c)| Some((s.parse().ok()?, c.parse().ok()?))).ok_or(format!("--fault <step>:<car>, got {text:?}"))?);
            }
            other if !other.starts_with("--") && a.file.is_none() => a.file = Some(PathBuf::from(other)),
            other if !other.starts_with("--") && a.file2.is_none() => a.file2 = Some(PathBuf::from(other)),
            _ => return Err(format!("unknown argument {flag}\n{}", usage())),
        }
    }
    Ok(a)
}

fn main() {
    let result = parse_args().and_then(|args| match args.command.as_str() {
        "list" => {
            for s in scenario::all().into_iter().chain(scenario::powertrain()).chain(scenario::whole()).chain(scenario::hybrid()).chain(scenario::track()) {
                println!("{:24} {:6.1} s  {}", s.name, (s.steps - 1) as f32 * scenario::DT, s.about);
            }
            Ok(())
        }
        "run" => run(&args),
        "all" => all(&args),
        "check" => check_one(&args),
        "setup-check" => setup_check(&args),
        "diff" => diff(&args),
        "csv" => csv(&args),
        "rays" => rays(&args),
        "collide" => collide_poses(&args),
        "collide-worlds" => collide_worlds(&args),
        "sus-micro" => sus_micro(&args),
        "multi" => multi(&args),
        "net" => net(&args),
        "multi-check" => multi_check(&args),
        "replay-file" => replay_file(&args),
        "race-ini" => race_ini(&args),
        "race-micro" => race_micro(&args),
        "mode-micro" => mode_micro(&args),
        "mouse-micro" => mouse_micro(&args),
        "steer-micro" => steer_micro(&args),
        "pad-micro" => pad_micro(&args),
        "start-camera-micro" => start_camera_micro(&args),
        "multi-list" => {
            for s in game::multi::scenarios() {
                println!("{:18} {:6.1} s  {} cars  {}", s.name, s.seconds, s.default_cars.len(), s.about);
            }
            Ok(())
        }
        _ => Err(usage()),
    });
    if let Err(message) = result {
        eprintln!("{message}");
        std::process::exit(1);
    }
}

/// A track's folder: a path, or a name under the game's `content/tracks`.
fn track_folder(args: &Args, track: &Path) -> PathBuf {
    if track.is_dir() {
        return std::path::absolute(track).unwrap_or_else(|_| track.to_path_buf());
    }
    let game = args.acs.parent().unwrap_or(Path::new("."));
    game.join("content").join("tracks").join(track)
}

/// Task 24b: `car_oracle race-ini <file.ini> [--pit-slots <n>] [--seed <n>] [--root <dir>]`: the
/// cars of a race.ini as the game's own `RaceManager::initOffline` makes them (its callees that
/// need the rest of the game write down what they are given) against
/// `rustyac_physics::session::race_grid`.
fn race_ini(args: &Args) -> Result<(), String> {
    use rustyac_physics::data::ini::IniReader;
    use rustyac_physics::session;
    let file = args.file.as_ref().ok_or("race-ini <file.ini>")?;
    let file = std::path::absolute(file).map_err(|e| e.to_string())?;
    let game_folder = args.acs.parent().unwrap_or(Path::new(".")).to_path_buf();
    let root = std::path::absolute(&args.root).map_err(|e| e.to_string())?;
    if root.join("acs.exe").is_file() {
        return Err(format!("{} is a game folder: the oracle writes only into a scratch folder of its own", root.display()));
    }
    // the skins of every car the file names: empty folders with the game's names, which is
    // all `isSkinAvailable` and `getModelSkins` look at
    let ini = IniReader::load(&file)?;
    let mut models: Vec<String> = vec![ini.get_string("RACE", "MODEL")];
    let mut n = 0;
    while n < 256 {
        if ini.has_section(&format!("CAR_{n}")) {
            models.push(ini.get_string(&format!("CAR_{n}"), "MODEL"));
        }
        n += 1;
    }
    models.sort();
    models.dedup();
    let _ = std::fs::remove_dir_all(root.join("content/cars"));
    for model in models.iter().filter(|m| !m.is_empty() && m.as_str() != "-") {
        std::fs::create_dir_all(root.join("content/cars").join(model).join("skins")).map_err(|e| e.to_string())?;
        for skin in session::model_skins(&game_folder.join("content/cars"), model) {
            std::fs::create_dir_all(root.join("content/cars").join(model).join("skins").join(skin)).map_err(|e| e.to_string())?;
        }
    }
    std::fs::create_dir_all(root.join("cfg")).map_err(|e| e.to_string())?;
    let copy = root.join("cfg/race_under_test.ini");
    std::fs::copy(&file, &copy).map_err(|e| e.to_string())?;
    std::env::set_current_dir(&root).map_err(|e| format!("{}: {e}", root.display()))?;
    let acs = acs::Acs::load(&args.acs)?;
    if args.verbose {
        acs.unbuffer_game_stdout();
    } else {
        acs.silence_game_stdout();
    }
    let seed = args.seed as u32;
    let game = unsafe { game::race_ini::read(&acs, &copy, args.pit_slots, seed)? };

    // the port, on the same file, the same skin folders and the same start of rand()
    let mut rand = session::MsvcRand(seed);
    let mut draw = || rand.next();
    session::draws_before_the_cars(&ini, &mut draw);
    let cars_folder = root.join("content/cars");
    let port = session::race_grid(&ini, args.pit_slots.max(0) as usize, &|model| session::model_skins(&cars_folder, model), &|model, skin| session::skin_available(&cars_folder, model, skin), &mut draw);

    let bits = |x: f32| format!("{x} ({:08x})", x.to_bits());
    let mut game_lines = vec![format!("cars to be loaded: {}", game.cars_to_be_loaded)];
    game_lines.push(format!("first session: type {}, forced position {}", game.sessions.first().map_or(1, |s| s.0), game.sessions.first().map_or(-1, |s| s.1)));
    for (n, car) in game.cars.iter().enumerate() {
        let ai = match car.ai {
            // (a drag session: AIDriverDrag, whose push is the car's level as read)
            Some((level, race, _)) if race.is_nan() => format!("drag driver, push {}", bits(level)),
            Some((level, race, aggression)) => format!("AI level {} of {}, aggression {}", bits(level), bits(race), bits(aggression)),
            None => "no AI".to_string(),
        };
        game_lines.push(format!("car {n}: {} | config {:?} | skin {} | ballast {} | restrictor {} | driver {:?} | nation {:?} | {ai} | pit box {n} | grid slot {}", car.model, car.config, car.skin, bits(car.ballast_kg), bits(car.restrictor), car.driver_name, car.nation_code, car.start_index));
    }
    let mut port_lines = Vec::new();
    match &port {
        Ok(grid) => {
            port_lines.push(format!("cars to be loaded: {}", grid.cars_to_be_loaded));
            port_lines.push(format!("first session: type {}, forced position {}", grid.first_session_type, grid.forced_position));
            for car in &grid.cars {
                let ai = match (car.section > 0, grid.first_session_type) {
                    (false, _) => "no AI".to_string(),
                    (true, 7) => format!("drag driver, push {}", bits(car.ai_level)),
                    (true, _) => format!("AI level {} of {}, aggression {}", bits(car.ai_level), bits(grid.ai_level), bits(car.ai_aggression)),
                };
                port_lines.push(format!(
                    "car {}: {} | config {:?} | skin {} | ballast {} | restrictor {} | driver {:?} | nation {:?} | {ai} | pit box {} | grid slot {}",
                    car.index,
                    car.model,
                    car.config,
                    car.skin,
                    bits(car.ballast_kg),
                    bits(car.restrictor),
                    car.driver_name,
                    car.nation_code.clone().unwrap_or_default(),
                    car.index,
                    car.start_index
                ));
            }
        }
        Err(message) => {
            port_lines.push("cars to be loaded: 0".to_string());
            port_lines.push(format!("first session: type {}, forced position {}", game.sessions.first().map_or(1, |s| s.0), game.sessions.first().map_or(-1, |s| s.1)));
            println!("the port: {message}");
        }
    }
    // Task 28: the session list and the rules
    game_lines.extend(game.session_lines.iter().cloned());
    {
        let sessions = rustyac_physics::race::SessionsIni::from_ini(&ini, 3);
        for s in &sessions.sessions {
            port_lines.push(format!("session: type {} timed {} laps {} minutes {:08x} overtime {:08x} spawn {:?} name {:?} forced {}", s.session_type, s.is_timed_race as u8, s.laps, s.duration_minutes.to_bits(), s.overtime_ms.to_bits(), s.spawn_set, s.name, s.forced_position));
        }
        port_lines.push(format!(
            "rules: penalties {} fixed setup {} additional lap {} allowed tyres out {} jump start mode {}",
            sessions.penalties_enabled as u8,
            sessions.fixed_setup as u8,
            sessions.has_additional_lap as u8,
            sessions.allowed_tyres_out(),
            sessions.jump_start_penalty_mode.unwrap_or(1)
        ));
    }
    println!("{} (pit slots {}, seed {seed}):", file.display(), args.pit_slots);
    for note in &game.notes {
        println!("  the game: {note}");
    }
    for line in &game_lines {
        println!("  {line}");
    }
    if game_lines == port_lines {
        println!("race.ini reading: IDENTICAL ({} cars, {} of them with a skin the game picked)", game.cars.len(), port.as_ref().map_or(0, |g| g.cars.iter().filter(|c| c.skin_was_picked).count()));
        Ok(())
    } else {
        println!("race.ini reading: DIFFERENT; the port's:");
        for line in &port_lines {
            println!("  {line}");
        }
        Err("the game and the port read the file differently".to_string())
    }
}

/// Task 34: `car_oracle mouse-micro [--seed <n>] [--steps <calls>] [--root <dir>]` (tape: `<root>/mouse.tape`):
/// the game's own mouse steering (`KeyboardCarControl`) on made-up cursors and windows, as a
/// tape for the port's `mouse_steering_check` example.
fn mouse_micro(args: &Args) -> Result<(), String> {
    let root = std::path::absolute(&args.root).map_err(|e| e.to_string())?;
    if root.join("acs.exe").is_file() {
        return Err(format!("{} is a game folder: the oracle writes only into a scratch folder of its own", root.display()));
    }
    let out = root.join("mouse.tape");
    std::fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    std::env::set_current_dir(&root).map_err(|e| format!("{}: {e}", root.display()))?;
    acs::set_extra_overrides(game::mouse::overrides());
    let acs = acs::Acs::load(&args.acs)?;
    acs.silence_game_stdout();
    let tape = unsafe { game::mouse::micro(&acs, args.seed as u32, args.steps.unwrap_or(20_000))? };
    std::fs::write(&out, &tape).map_err(|e| format!("{}: {e}", out.display()))?;
    println!("mouse-micro seed {}: {} lines of the game into {}", args.seed, tape.lines().count(), out.display());
    Ok(())
}

/// Task 37b: `car_oracle pad-micro [--seed <n>] [--steps <calls>] [--root <dir>]` (tape:
/// `<root>/pad.tape`): the game's own gamepad car control (`JoypadCarControl`) read from
/// made-up controls.ini files and fed made-up XInput states, for the port's
/// `pad_steering_check` example.
fn pad_micro(args: &Args) -> Result<(), String> {
    let root = std::path::absolute(&args.root).map_err(|e| e.to_string())?;
    if root.join("acs.exe").is_file() {
        return Err(format!("{} is a game folder: the oracle writes only into a scratch folder of its own", root.display()));
    }
    let out = root.join("pad.tape");
    std::fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    std::env::set_current_dir(&root).map_err(|e| format!("{}: {e}", root.display()))?;
    acs::set_extra_overrides(game::pad::overrides());
    let acs = acs::Acs::load(&args.acs)?;
    acs.silence_game_stdout();
    let tape = unsafe { game::pad::micro(&acs, args.seed as u32, args.steps.unwrap_or(20_000))? };
    std::fs::write(&out, &tape).map_err(|e| format!("{}: {e}", out.display()))?;
    println!("pad-micro seed {}: {} lines of the game into {}", args.seed, tape.lines().count(), out.display());
    Ok(())
}

/// Task 37: `car_oracle steer-micro [--seed <n>] [--steps <calls>] [--root <dir>]` (tapes:
/// `<root>/steer_line.tape` on Laguna Seca's AI line, `<root>/steer_none.tape` without one): the
/// game's own keyboard steering look-ahead (`KeyboardCarControl::stepSteer`) on a game body put
/// somewhere new every call, for the port's `keyboard_steering_check` example.
fn steer_micro(args: &Args) -> Result<(), String> {
    let root = std::path::absolute(&args.root).map_err(|e| e.to_string())?;
    if root.join("acs.exe").is_file() {
        return Err(format!("{} is a game folder: the oracle writes only into a scratch folder of its own", root.display()));
    }
    std::fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    // the game's AISpline::load may write the line again: it reads a copy in the oracle's folder
    let ac = args.acs.parent().ok_or("no AC folder")?;
    let line = root.join("fast_lane.ai");
    std::fs::copy(ac.join("content/tracks/ks_laguna_seca/ai/fast_lane.ai"), &line).map_err(|e| format!("fast_lane.ai: {e}"))?;
    std::env::set_current_dir(&root).map_err(|e| format!("{}: {e}", root.display()))?;
    acs::set_extra_overrides(game::steer::overrides());
    let acs = acs::Acs::load(&args.acs)?;
    acs.silence_game_stdout();
    let steps = args.steps.unwrap_or(20_000);
    for (name, file) in [("steer_line.tape", Some(line.to_string_lossy().into_owned())), ("steer_none.tape", None)] {
        let tape = unsafe { game::steer::micro(&acs, args.seed as u32, steps, file.as_deref())? };
        let out = root.join(name);
        std::fs::write(&out, &tape).map_err(|e| format!("{}: {e}", out.display()))?;
        println!("steer-micro seed {}: {} lines of the game into {}", args.seed, tape.lines().count(), out.display());
    }
    Ok(())
}

/// Task 34: `car_oracle start-camera-micro [--seed <n>] [--steps <frames>] [--root <dir>]`
/// (tapes: `<root>/start_camera.tape`, `<root>/free_camera.tape`): the game's own
/// session-start camera on made-up cars and F7's free camera on made-up keys and cursors, for
/// the port's `start_camera_check` example.
fn start_camera_micro(args: &Args) -> Result<(), String> {
    let root = std::path::absolute(&args.root).map_err(|e| e.to_string())?;
    if root.join("acs.exe").is_file() {
        return Err(format!("{} is a game folder: the oracle writes only into a scratch folder of its own", root.display()));
    }
    std::fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    let out = root.join("start_camera.tape");
    acs::set_extra_overrides(game::camera::overrides());
    let acs = acs::Acs::load(&args.acs)?;
    acs.silence_game_stdout();
    let tape = unsafe { game::camera::micro(&acs, args.seed as u32, args.steps.unwrap_or(20_000))? };
    std::fs::write(&out, &tape).map_err(|e| format!("{}: {e}", out.display()))?;
    println!("start-camera-micro seed {}: {} lines of the game into {}", args.seed, tape.lines().count(), out.display());
    let out = root.join("free_camera.tape");
    let tape = unsafe { game::camera::free_micro(&acs, args.seed as u32, args.steps.unwrap_or(20_000))? };
    std::fs::write(&out, &tape).map_err(|e| format!("{}: {e}", out.display()))?;
    println!("free camera: {} lines of the game into {}", tape.lines().count(), out.display());
    Ok(())
}

/// Task 45: `car_oracle mode-micro [--seed <n>] [--steps <laps per race.ini>]`: the game's
/// special-event decryption, goals and hotlap judging against the port's.
fn mode_micro(args: &Args) -> Result<(), String> {
    let root = std::path::absolute(&args.root).map_err(|e| e.to_string())?;
    let acs_path = std::path::absolute(&args.acs).map_err(|e| e.to_string())?;
    let ac = acs_path.parent().ok_or("--acs has no folder")?.to_path_buf();
    if root.join("acs.exe").is_file() {
        return Err(format!("{} is a game folder: the oracle writes only into a scratch folder of its own", root.display()));
    }
    std::fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    std::env::set_current_dir(&root).map_err(|e| format!("{}: {e}", root.display()))?;
    acs::set_extra_overrides(vec![("shell32.dll", "SHGetFolderPathW", game::multi::sh_get_folder_path_w as *const () as usize)]);
    let acs = acs::Acs::load(&acs_path)?;
    if !args.verbose {
        acs.silence_game_stdout();
    }
    let r = unsafe { game::modes::micro(&acs, &root, &ac, args.seed as u32, args.steps.unwrap_or(40))? };
    println!("mode-micro seed {}: {} event files ({} bytes) decrypted, {} race.ini read ({} goals), {} hotlap laps judged ({} messages), race_out.json {} bytes, {} time attack sessions ({} frames, {} checkpoints, {} messages), {} drift steps ({} drifts, {} combos, {:.0} points), {} drag sessions ({} frames, {} matches, {} penalties, {} messages)", args.seed, r.files, r.bytes, r.inis, r.conditions, r.laps, r.messages, r.json_bytes, r.ta_sessions, r.ta_frames, r.ta_checkpoints, r.ta_messages, r.drift_steps, r.drifts, r.drift_combos, r.drift_points, r.drag_sessions, r.drag_frames, r.drag_matches, r.drag_penalties, r.drag_messages);
    match r.mismatch {
        None => {
            println!("modes: IDENTICAL");
            Ok(())
        }
        Some(what) => Err(format!("modes: DIFFERENT at {what}")),
    }
}

/// Task 28: `car_oracle race-micro [--seed <n>] [--kind <session list 0..7>] [--count <cars>]
/// [--steps <frames>] [--fault 1:0] [--root <dir>]`: the game's own referee and the port's on
/// the same made-up drive, compared after every frame (`game::race`). One drive per process.
fn race_micro(args: &Args) -> Result<(), String> {
    let root = std::path::absolute(&args.root).map_err(|e| e.to_string())?;
    if root.join("acs.exe").is_file() {
        return Err(format!("{} is a game folder: the oracle writes only into a scratch folder of its own", root.display()));
    }
    std::fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    std::env::set_current_dir(&root).map_err(|e| format!("{}: {e}", root.display()))?;
    acs::set_extra_overrides(vec![("shell32.dll", "SHGetFolderPathW", game::multi::sh_get_folder_path_w as *const () as usize)]);
    let acs = acs::Acs::load(&args.acs)?;
    if args.verbose {
        acs.unbuffer_game_stdout();
    } else {
        acs.silence_game_stdout();
    }
    let variant = args.kind.unwrap_or(0);
    let cars = if args.count_given { args.count } else { 6 };
    let frames = args.steps.unwrap_or(40_000);
    let report = unsafe { game::race::micro(&acs, args.seed as u32, variant, cars, frames, args.fault.is_some(), None)? };
    let jobs: Vec<String> = report.jobs_seen.iter().map(|(address, name)| format!("{name} @ {:#x}", 0x1_4000_0000usize + address)).collect();
    println!("race-micro seed {} list {variant} cars {cars}: {} frames, {} laps, {} sessions ended, {} values compared, race_out.json {} bytes", args.seed, report.frames, report.laps, report.sessions_ended, report.values, report.json_bytes);
    println!("  jobs for the physics thread: {}", jobs.join(", "));
    println!("  frames by flag (none, blue, black, chequered, yellow, white, penalty): {:?}", report.flag_frames);
    println!("  convertRemoteSession: {} made-up server sessions, all the same", report.remote_sessions);
    println!("  golden: ({}, {variant}, {cars}, {frames}, {:#018x}, {:#018x}),", args.seed, report.state_hash, report.json_hash);
    match report.mismatch {
        None => {
            println!("referee: IDENTICAL");
            Ok(())
        }
        Some(what) => {
            println!("referee: DIFFERENT at {what}");
            Err("the game's referee and the port's differ".to_string())
        }
    }
}

/// Task 24: several cars in the game's own engine, recorded; then the port's replays.
fn multi(args: &Args) -> Result<(), String> {
    let scenarios = game::multi::scenarios();
    let scenario = scenarios.iter().find(|s| s.name == args.scenario).ok_or_else(|| format!("no several-car scenario {:?} (see `car_oracle multi-list`)", args.scenario))?;
    let repo = repo_root();
    let out = std::path::absolute(if args.out == repo.join("oracle/car") { repo.join("oracle/multi") } else { args.out.clone() }).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
    let track = args.track.as_ref().ok_or("multi needs --track <folder or name> (for example --track spa)")?;
    let folder = track_folder(args, track);
    let names: Vec<String> = if args.cars.is_empty() { scenario.default_cars.iter().map(|s| s.to_string()).collect() } else { args.cars.clone() };
    for name in &names {
        game::prepare_root(&repo, &args.root, name)?;
    }
    track::prepare_root(&args.root, &folder, &args.layout)?;
    // Task 27: another fast lane for this run. The game reads (and, for an old version,
    // rewrites) `ai/fast_lane.ai` of the scratch root; the port reads the untouched copy
    let mut ai_folder = None;
    if let Some(line) = &args.ai_line {
        let name = folder.file_name().ok_or("the track folder has no name")?;
        let mut base = args.root.join("content/tracks").join(name);
        if !args.layout.is_empty() {
            base = base.join(&args.layout);
        }
        let port = base.join("ai_port");
        let _ = std::fs::remove_dir_all(&port);
        std::fs::create_dir_all(&port).map_err(|e| e.to_string())?;
        std::fs::copy(line, base.join("ai/fast_lane.ai")).map_err(|e| format!("{}: {e}", line.display()))?;
        std::fs::copy(line, port.join("fast_lane.ai")).map_err(|e| e.to_string())?;
        if base.join("ai/pit_lane.ai").is_file() {
            std::fs::copy(base.join("ai/pit_lane.ai"), port.join("pit_lane.ai")).map_err(|e| e.to_string())?;
        }
        ai_folder = Some(std::path::absolute(&port).map_err(|e| e.to_string())?);
    }
    if args.game_threads != 0 {
        // the game's own thread pool (the default file of the scratch root has THREADS=0)
        let ini = args.root.join("system/cfg/assetto_corsa.ini");
        let text = std::fs::read_to_string(&ini).map_err(|e| e.to_string())?;
        std::fs::write(&ini, text.replace("THREADS=0", &format!("THREADS={}", args.game_threads))).map_err(|e| e.to_string())?;
    }
    std::env::set_current_dir(&args.root).map_err(|e| format!("{}: {e}", args.root.display()))?;
    // Task 27: the one import of the AI drivers this loader does not bind
    let mut overrides = vec![("shell32.dll", "SHGetFolderPathW", game::multi::sh_get_folder_path_w as *const () as usize)];
    if game::multi::referee_of(scenario.name).is_some() {
        // Task 28: the referee's code draws from a rand() of its own (the main thread's)
        overrides.push(("msvcr120.dll", "rand", game::race::game_rand as *const () as usize));
    }
    acs::set_extra_overrides(overrides);
    let acs = acs::Acs::load(&args.acs)?;
    game::race::init_rand(&acs);
    if args.verbose {
        acs.unbuffer_game_stdout();
    } else {
        acs.silence_game_stdout();
    }
    let game_root = args.acs.parent().map(Path::to_path_buf);
    let cardata = repo.join("cardata");
    let colliders = |name: &str| rustyac_physics::car::colliders::load(&cardata.join(name), game_root.as_deref(), name);
    let mut world = game::multi::MultiWorld::build(&acs, scenario, &args.cars, &folder, &args.layout, &colliders, args.seed as u32)?;
    let steps = args.steps.unwrap_or((scenario.seconds / scenario::DT) as usize);
    let start = std::time::Instant::now();
    // Task 24b: a tape per car (`<scenario>.car<n>.audiotape`, the format of `run --audio-tape`):
    // the game's own `CarPhysicsState` of every car 60 times a second. The engine's physics
    // events (every car's, in the engine's order; `param1` of a collision is the car) are in
    // the first car's tape.
    let mut tapes = Vec::new();
    if args.audio_tape {
        use std::io::Write as _;
        world.taping = true;
        for (index, name) in world.names.clone().iter().enumerate() {
            let tape_path = out.join(format!("{}.car{index}.audiotape", scenario.name));
            let mut file = std::io::BufWriter::new(std::fs::File::create(&tape_path).map_err(|e| format!("{}: {e}", tape_path.display()))?);
            let header = format!("audiotape=1\nscenario={}\ncar={name}\nframe_hz=60\nstep_ms=3\ncars={}\ncar_index={index}\ntrack={}\ntrack_layout={}\n\n", scenario.name, world.names.len(), track.display(), args.layout);
            file.write_all(header.as_bytes()).map_err(|e| e.to_string())?;
            tapes.push(file);
        }
    }
    // Task 28c: the replays
    let mut replay = None;
    if args.replay {
        let ini_text = format!("[QUALITY]\nLEVEL={}\n\n[REPLAY]\nMAX_SIZE_MB=26\n\n[AUTOSAVE]\nENABLED=1\nRACE=3\nQUALIFY=2\nOTHERS=1\nMIN_TIME_SECONDS=30\n", args.replay_level.unwrap_or(3));
        let ini = rustyac_physics::data::ini::IniReader::from_text(std::path::Path::new("replay.ini"), &ini_text);
        let cars: Vec<*mut u8> = (0..world.names.len()).map(|i| world.car_ptr(i)).collect();
        replay = Some(unsafe { game::replay::ReplayProof::build(&acs, &cars, &world.names, world.game_track, world.port_track(), &world.physics_objects, 0, Some(&ini), args.replay_frames, true) });
    }
    if args.ghost {
        let port_track = world.port_track();
        unsafe { game::ghost::install(&acs, world.car_ptr(0), &world.names[0], &port_track.name, &port_track.config) };
    }
    if args.ai_record {
        unsafe { game::ai_record::install(&acs, world.engine, world.game_track, world.car_ptr(0), &world.port_track())? };
    }
    if args.telemetry {
        let port_track = world.port_track();
        unsafe { game::telemetry::install(&acs, world.car_ptr(0), &world.names[0], &port_track.name, &port_track.config) };
    }
    // Task 44c: ACSuspension's TempRun on car 0 (CAR_ORACLE_TEMP_RUN=<on>:<off> steps): the
    // game's BrakeSystem::activateTempRunFile @ 0x14028d400 (Car+0x2ca0), its file in the root's
    // TempRuns (the working folder by now)
    let temp_run: Option<(usize, usize)> = std::env::var("CAR_ORACLE_TEMP_RUN").ok().and_then(|t| t.split_once(':').and_then(|(a, b)| Some((a.parse().ok()?, b.parse().ok()?))));
    if let Some((on, off)) = temp_run {
        std::fs::create_dir_all("TempRuns").map_err(|e| e.to_string())?;
        world.recording.set("temp_run", format!("{on}:{off}"));
    }
    let mut next_frame = 1usize;
    for i in 0..steps {
        if let Some((on, off)) = temp_run {
            if i == on || i == off {
                let activate: extern "C" fn(*mut u8, bool) = unsafe { std::mem::transmute(acs.va(0x1_4028_d400)) };
                activate(unsafe { world.car_ptr(0).add(0x2ca0) }, i == on);
            }
        }
        world.step();
        if args.ai_record {
            unsafe { game::ai_record::after_step() };
        }
        if let Some(replay) = &mut replay {
            unsafe { replay.after_step(world.time_ms(), &|index| world.physics_state(index).0)? };
        }
        if !tapes.is_empty() {
            use std::io::Write as _;
            for event in world.tape_events.drain(..) {
                tapes[0].write_all(&[1]).and_then(|_| tapes[0].write_all(&event)).map_err(|e| e.to_string())?;
            }
            if next_frame * 50 / 9 <= i + 1 {
                next_frame += 1;
                for (index, file) in tapes.iter_mut().enumerate() {
                    let (state, tc) = world.physics_state(index);
                    file.write_all(&[2]).and_then(|_| file.write_all(&(i as u32 + 1).to_le_bytes())).and_then(|_| file.write_all(&state)).and_then(|_| file.write_all(&[tc])).map_err(|e| e.to_string())?;
                }
            }
        }
    }
    for mut file in tapes {
        use std::io::Write as _;
        file.flush().map_err(|e| e.to_string())?;
    }
    let seconds = start.elapsed().as_secs_f64();
    world.recording.set("steps", steps.to_string());
    if let Some(ai_folder) = &ai_folder {
        world.recording.set("ai_folder", ai_folder.display().to_string());
    }
    world.recording.set("game_threads", args.game_threads.to_string());
    if args.ai_record {
        world.recording.set("ai_record", game::ai_record::header());
    }
    let suffix = if args.game_threads != 0 { format!("_gt{}", args.game_threads) } else { String::new() };
    let path = out.join(format!("{}{suffix}.mcrec", scenario.name));
    world.recording.save(&path)?;
    println!("{}: {} cars, {steps} steps in {seconds:.1} s ({:.1}x real time), {} loose objects, {} values a step", scenario.name, world.names.len(), steps as f64 * 0.003 / seconds, world.object_count(), world.recording.row_words());
    for line in world.summary() {
        println!("  {line}");
    }
    println!("wrote {} ({} MB)", path.display(), std::fs::metadata(&path).map(|m| m.len() / 1_000_000).unwrap_or(0));
    if args.ghost {
        for line in unsafe { game::ghost::finish(&repo, scenario.name)? } {
            println!("{line}");
        }
    }
    if args.ai_record {
        for line in unsafe { game::ai_record::finish(&repo, scenario.name, world.game_track, &world.port_track())? } {
            println!("{line}");
        }
    }
    if args.telemetry {
        for line in unsafe { game::telemetry::finish(&repo, scenario.name)? } {
            println!("{line}");
        }
    }
    if let Some(replay) = &mut replay {
        replay_proof(replay, scenario.name, &out, &repo, &world.names, &track.display().to_string(), &args.layout)?;
    }
    let recording = world.recording.clone();
    // Task 28: the end of the program for the game's referee: its results, its race_out.json
    let mut game_file = None;
    if let Some(referee) = &mut world.referee {
        let file = unsafe { referee.shutdown()? };
        let state: Vec<String> = unsafe { referee.dump() }.into_iter().map(|(key, value)| format!("{key}: {value}")).collect();
        let log: Vec<String> = referee.log.iter().map(|(frame, line)| format!("frame {frame}: {line}")).collect();
        let _ = std::fs::write(path.with_extension("race_out.json"), &file);
        let _ = std::fs::write(path.with_extension("referee.txt"), format!("{}\n\n{}\n", log.join("\n"), state.join("\n")));
        println!("the game's referee: {} frames, race_out.json {} bytes; what it said is in {}", referee.frames, file.len(), path.with_extension("referee.txt").display());
        println!("  the first car: {} frames with a time penalty, {} with a drive-through pending, {} with its controls locked for a time, {} black-flagged; most cuts in a lap {}", referee.seen[0], referee.seen[1], referee.seen[2], referee.seen[3], referee.seen[4]);
        if referee.pit.is_some() {
            println!("  the first car's pit stop: {} begun, {} finished (the choices applied), {} frames counting down, {} waiting in the box; {} frames with the mandatory stop done", referee.pit_seen[2], referee.pit_seen[3], referee.pit_seen[0], referee.pit_seen[4], referee.pit_seen[1]);
            if referee.pit_seen[5..].iter().any(|n| *n > 0) {
                println!("  the mouse menu: {} frames shown, {} with new tyres chosen, {} with fuel asked for, {} with a repair chosen", referee.pit_seen[5], referee.pit_seen[6], referee.pit_seen[7], referee.pit_seen[8]);
            }
        }
        for (frame, line) in referee.log.iter().filter(|(_, line)| !line.starts_with("SessionEnd")) {
            println!("  frame {frame} ({:.1} s): {line}", *frame as f64 * 0.009);
        }
        game_file = Some(file);
    }
    multi_compare(args, &recording, &path, game_file.as_deref())
}

/// Task 28c: `car_oracle replay-file <file.acreplay> --track <track> [--layout l]`: one of the
/// player's own replays (read only) loaded by the game's `ReplayManager::load` and by the
/// port's, the recorders and scene frames compared, then played by both through a script of
/// the transport's functions, every car's rebuilt `CarPhysicsState` compared. The cars are
/// stand-ins (no car's data is read: wings as the file counts them, a tank of 100 l).
fn replay_file(args: &Args) -> Result<(), String> {
    use game::replay::Act;
    use rustyac_physics::replay::{CarHeader, ReplayLoader, ReplayManager, ReplayRecorder};
    let file = std::path::absolute(args.file.as_ref().ok_or("replay-file needs a file")?).map_err(|e| e.to_string())?;
    let bytes = std::fs::read(&file).map_err(|e| format!("{}: {e}", file.display()))?;
    // the file's cars and their wing counts, by the port's reader
    struct Peek(Vec<String>);
    impl ReplayLoader for Peek {
        fn load_track(&mut self, _track: &str, _layout: &str) {}
        fn add_car(&mut self, car: &CarHeader) -> ReplayRecorder {
            self.0.push(car.unix_name.clone());
            ReplayRecorder::new(9999, 0)
        }
        fn apply_custom_weather(&mut self, _weather: &str) {}
    }
    let mut peek = Peek(Vec::new());
    let mut manager = ReplayManager::new(None, 30.0, Vec::new(), None, 0);
    manager.load(&bytes, &mut peek)?;
    let wings: Vec<usize> = manager.recorders.iter().map(|recorder| recorder.meta_data.1 as usize).collect();
    let objects = manager.frame_information.first().map_or(0, |frame| frame.track_objects.len());
    let version = i32::from_le_bytes(bytes[0..4].try_into().unwrap());
    let repo = repo_root();
    let name = file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    if args.fault.is_some() || std::env::var_os("REPLAY_PORT_ONLY").is_some() {
        // REPLAY_PORT_ONLY=1: the port alone (a file the game's own loader cannot read)
        use rustyac_physics::replay::{track_env::NoTrack, PlaybackAvatar};
        manager.start_replay_mode(0);
        let cars = manager.recorders.len();
        let mut outs = vec![rustyac_physics::car_physics_state::CarPhysicsState::at_origin(); cars];
        let mut fx = vec![PlaybackAvatar { max_fuel: 100.0, has_dynamic_effects: true, dirt_level: None, car_node_active: true }; cars];
        let mut frames = 0;
        for step in 0..1500 {
            match step {
                300 => manager.fast_forward(2, false),
                700 => manager.rewind(3, false),
                1000 => manager.set_normalized_position(0.9),
                1001 => manager.play(),
                _ => {}
            }
            manager.update_play_state(1.0 / 60.0);
            for car in 0..cars {
                manager.get_physics_state(car, &mut outs[car], &mut fx[car], &NoTrack);
            }
            frames += 1;
        }
        println!("replay-file | {name} | version {version}, {} bytes, {} cars, {} frames, {objects} loose objects | the port alone: loaded, {frames} frames played to frame {} | {}", bytes.len(), wings.len(), manager.recorded_frames, manager.current_frame, manager.log.last().cloned().unwrap_or_default());
        return Ok(());
    }
    let track = args.track.as_ref().ok_or("replay-file needs --track <folder or name>")?;
    let folder = track_folder(args, track);
    track::prepare_root(&args.root, &folder, &args.layout)?;
    std::env::set_current_dir(&args.root).map_err(|e| format!("{}: {e}", args.root.display()))?;
    acs::set_extra_overrides(vec![("shell32.dll", "SHGetFolderPathW", game::multi::sh_get_folder_path_w as *const () as usize)]);
    let acs = acs::Acs::load(&args.acs)?;
    acs.silence_game_stdout();
    let (game_track, port_track) = unsafe { game::multi::track_for_replay(&acs, &folder, &args.layout)? };
    let mut cars: Vec<String> = peek.0.clone();
    cars.sort();
    cars.dedup();
    let mut proof = unsafe { game::replay::ReplayProof::build(&acs, &[], &[], game_track, port_track, &[], objects, None, Some(manager.recorded_frames.max(1)), true) };
    let script: Vec<(Act, usize)> = vec![
        (Act::Play, 90),
        (Act::SlowMotion(4.0), 60),
        (Act::Play, 10),
        (Act::FastForward(1, false), 60),
        (Act::Rewind(1, false), 60),
        (Act::Pause, 4),
        (Act::NextFrame, 12),
        (Act::Position(0.5), 3),
        (Act::Play, 40),
        (Act::NextLap, 20),
        (Act::PreviousLap, 20),
        (Act::Position(0.97), 2),
        (Act::Play, 100),
        (Act::CutIn(0.3), 2),
        (Act::CutOut(0.6), 2),
        (Act::Play, 60),
        (Act::Rewind(4, false), 150),
        (Act::Stop, 3),
        (Act::Play, 30),
    ];
    let report = unsafe { proof.load_and_play(&file.to_string_lossy(), &bytes, &script, Some(&wings))? };
    let ok = report.differences.is_empty() && report.bad_states == 0 && report.bad_objects == 0;
    println!(
        "replay-file | {name} | version {version}, {} bytes, {} cars ({}), {} frames, {} loose objects, {} {} | loaded by both; playback {} frames: {} car states and {} object matrices compared, {} and {} differ | {}",
        bytes.len(),
        wings.len(),
        cars.join(" "),
        report.recorded,
        objects,
        track.display(),
        args.layout,
        report.frames,
        report.states,
        report.objects,
        report.bad_states,
        report.bad_objects,
        if ok { "IDENTICAL" } else { "DIFFERENT" }
    );
    for line in &report.differences {
        println!("    {line}");
    }
    let _ = repo;
    Ok(())
}

/// Task 28c: the end of a `multi --replay` run: what was recorded is compared, three files
/// are saved by the game and by the port and compared, each is loaded by both and played
/// through a script of the transport's functions.
fn replay_proof(replay: &mut game::replay::ReplayProof, scenario: &str, out: &Path, repo: &Path, names: &[String], track: &str, layout: &str) -> Result<(), String> {
    use game::replay::Act;
    let verdict = |differences: &[String]| if differences.is_empty() { "IDENTICAL".to_string() } else { format!("DIFFERENT ({})", differences.len()) };
    let mut all_ok = true;
    let (frames, differences) = unsafe { replay.compare_recording() };
    println!("replay | {scenario} | recording | {} frames recorded, {} laps, {frames} ring slots of the cars compared | {}", replay.recorded, replay.laps, verdict(&differences));
    for line in &differences {
        println!("    {line}");
    }
    all_ok &= differences.is_empty();
    let long: Vec<(Act, usize)> = vec![
        (Act::Play, 90),
        (Act::SlowMotion(4.0), 70),
        (Act::SlowMotion(8.0), 20),
        (Act::Play, 10),
        (Act::FastForward(1, false), 40),
        (Act::FastForward(1, false), 40),
        (Act::Rewind(1, false), 60),
        (Act::Rewind(1, false), 30),
        (Act::Pause, 5),
        (Act::NextFrame, 12),
        (Act::NextFrame, 12),
        (Act::Position(0.5), 3),
        (Act::Play, 40),
        (Act::NextLap, 20),
        (Act::PreviousLap, 20),
        (Act::Position(0.97), 2),
        (Act::Play, 120),
        (Act::Rewind(3, false), 80),
        (Act::CutIn(0.3), 2),
        (Act::CutOut(0.6), 2),
        (Act::Play, 60),
        (Act::FastForward(4, false), 200),
        (Act::Rewind(4, false), 300),
        (Act::Stop, 3),
        (Act::Play, 30),
        (Act::FastForward(1, true), 2),
        (Act::Rewind(1, true), 2),
    ];
    let short: Vec<(Act, usize)> = vec![(Act::Play, 60), (Act::Rewind(2, false), 60), (Act::SlowMotion(2.0), 40), (Act::Position(0.9), 2), (Act::Play, 80)];
    let cuts: [(&str, Option<(f32, f32)>, bool); 3] = [("as an autosave writes it (no cut set)", None, false), ("after the replay was entered (cut 0 to the last frame)", None, true), ("cut from 25% to 75%", Some((0.25, 0.75)), true)];
    for (index, (label, cut, entered)) in cuts.iter().enumerate() {
        unsafe { replay.set_cuts(*cut, *entered) };
        let name = format!("{scenario}_{index}");
        let (game_file, port_file) = unsafe { replay.save(&name, repo)? };
        let differences = replay.compare_files(&game_file, &port_file);
        println!("replay | {scenario} | file {index}: {label} | {} bytes of the game, {} of the port | {}", game_file.len(), port_file.len(), verdict(&differences));
        for line in &differences {
            println!("    {line}");
        }
        all_ok &= differences.is_empty();
        let _ = std::fs::write(out.join(format!("{name}.game.acreplay")), &game_file);
        let _ = std::fs::write(out.join(format!("{name}.port.acreplay")), &port_file);
        replay.keep_tape = index == 1;
        let report = unsafe { replay.load_and_play(&format!("oracle_no_documents/Assetto Corsa/replay/{name}"), &port_file, if index == 1 { &long } else { &short }, None)? };
        if index == 1 {
            // the game's rebuilt states of that playback as tapes (the format of --audio-tape),
            // one frame a picture: for the picture's and the sound's oracles
            for (car, tape) in replay.tapes.iter().enumerate() {
                let mut bytes = format!("audiotape=1
scenario={scenario}_replay
car={}
frame_hz=60
step_ms=3
cars={}
car_index={car}
track={track}
track_layout={layout}

", names.get(car).map_or("", String::as_str), replay.tapes.len()).into_bytes();
                for (frame, state) in tape.chunks(0xb70).enumerate() {
                    bytes.push(2);
                    bytes.extend_from_slice(&(frame as u32 * 50 / 9).to_le_bytes());
                    bytes.extend_from_slice(state);
                    bytes.push(0);
                }
                let _ = std::fs::write(out.join(format!("{scenario}_replay.car{car}.audiotape")), bytes);
            }
        }
        println!("replay | {scenario} | playback of file {index} | {} frames of {} recorded, the cursor ends at {}; {} car states and {} object matrices compared, {} and {} differ | {}", report.frames, report.recorded, report.last_frame, report.states, report.objects, report.bad_states, report.bad_objects, verdict(&report.differences));
        for line in &report.differences {
            println!("    {line}");
        }
        if index == 0 {
            for line in &report.told {
                println!("    load: {line}");
            }
        }
        all_ok &= report.differences.is_empty() && report.bad_states == 0 && report.bad_objects == 0;
    }
    println!("replay | {scenario} | all | {}", if all_ok { "IDENTICAL" } else { "DIFFERENT" });
    Ok(())
}

/// Task 25: `car_oracle net <tape> --scenario net_... --track <track> [--steps n]
/// [--csv-out <golden file> --count <steps>]`: the game's own `ACClient::onPhysicsStep` and
/// `NetCarStateProvider::step` against the port, both fed the tape's incoming messages.
fn net(args: &Args) -> Result<(), String> {
    use rustyac_net::handshake::HandshakeReply;
    use rustyac_net::replay::{comparable, remote_columns, step_hash, NetGolden, NetReplay};
    use rustyac_net::transport::{Lane, Tape, TapeEntry};
    use rustyac_physics::engine::record::{ReplayPaths, INPUT_COLUMNS};

    let tape_path = args.file.as_ref().ok_or("net needs a tape: car_oracle net <file.nettape> --scenario net_watch --track <track>")?;
    let tape_path = std::path::absolute(tape_path).map_err(|e| e.to_string())?;
    let tape = Tape::from_bytes(&std::fs::read(&tape_path).map_err(|e| format!("{}: {e}", tape_path.display()))?)?;
    let scenarios = game::multi::scenarios();
    let scenario = scenarios.iter().find(|s| s.name == args.scenario).ok_or_else(|| format!("no scenario {:?}: net_watch, net_block, net_drive, net_pit", args.scenario))?;
    let repo = repo_root();
    let out = std::path::absolute(if args.out == repo.join("oracle/car") { repo.join("oracle/net") } else { args.out.clone() }).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
    let track_name = args.track.as_ref().ok_or("net needs --track <folder or name> (the tape's track)")?;
    let folder = track_folder(args, track_name);
    // the tape's handshake: the slot, the send interval, the session's kind; and its car list
    let accepted = match tape.of(Lane::ReliableIn).next().map(|entry| HandshakeReply::decode(&entry.data)) {
        Some(HandshakeReply::Accepted(accepted)) => accepted,
        _ => return Err("the tape does not start with an accepted handshake".to_string()),
    };
    let names: Vec<String> = if args.cars.is_empty() { scenario.default_cars.iter().map(|s| s.to_string()).collect() } else { args.cars.clone() };
    let mut models: Vec<String> = names.clone();
    for entry in tape.of(Lane::ReliableIn).filter(|entry| entry.data.first() == Some(&0x40)) {
        for car in rustyac_net::handshake::decode_car_list(&entry.data, entry.data[1])? {
            models.push(car.model);
        }
    }
    models.sort();
    models.dedup();
    for name in &models {
        game::prepare_root(&repo, &args.root, name)?;
    }
    track::prepare_root(&args.root, &folder, &args.layout)?;
    std::env::set_current_dir(&args.root).map_err(|e| format!("{}: {e}", args.root.display()))?;
    acs::set_extra_overrides(game::net::socket_overrides());
    let acs = acs::Acs::load(&args.acs)?;
    if args.verbose {
        acs.unbuffer_game_stdout();
    } else {
        acs.silence_game_stdout();
    }
    let game_root = args.acs.parent().map(Path::to_path_buf);
    let cardata = repo.join("cardata");
    let colliders = |name: &str| rustyac_physics::car::colliders::load(&cardata.join(name), game_root.as_deref(), name);
    let world = game::multi::MultiWorld::build(&acs, scenario, &args.cars, &folder, &args.layout, &colliders, args.seed as u32)?;

    // the port: the same local car, the tape's session
    let paths = ReplayPaths { cardata: cardata.clone(), game_root: game_root.clone() };
    let recording = world.recording.clone();
    let track = rustyac_physics::engine::record::load_track(&recording, &paths)?;
    // `--fault <step>:<kind>`: the port alone gets a damaged tape, to show that the compare can
    // fail. Kind 0: one bit of the first car state arriving from that step on; 1: the first
    // ping from that step on is lost; 2: the first car state from that step on is lost.
    let mut port_tape = tape.clone();
    if let Some((step, kind)) = args.fault {
        let wanted: &[u8] = if kind == 1 { &[0xf9] } else { &[0x46, 0x48] };
        let at = port_tape
            .entries
            .iter()
            .position(|entry| entry.lane == Lane::DatagramIn && entry.tick >= step as u64 && entry.data.first().is_some_and(|id| wanted.contains(id)) && (kind != 0 || entry.data.len() > 12))
            .ok_or(format!("--fault {step}:{kind}: the tape has no such message from step {step} on"))?;
        println!("  fault {kind}: the port's incoming message {:#04x} of step {} is {}", port_tape.entries[at].data[0], port_tape.entries[at].tick, if kind == 0 { "changed by one bit" } else { "lost" });
        if kind == 0 {
            port_tape.entries[at].data[12] ^= 1;
        } else {
            port_tape.entries.remove(at);
        }
    }
    let mut port = NetReplay::build(&recording, &paths, track, &port_tape)?;
    let slots = port.remote_slots();
    let columns = remote_columns(&slots);
    let mut remote = Vec::new();
    for (entry, car) in port.cars.iter().filter(|car| slots.contains(&car.session_id)).zip(&port.session.cars) {
        remote.push((entry.clone(), car.clone(), colliders(&entry.model)?.mesh));
    }
    let mut game = game::net::NetGame::build(&acs, world, &tape, &remote, accepted.session_id, accepted.send_interval_ms(), accepted.current_session.kind)?;
    println!(
        "{}: the tape of slot {} at {} ({} messages), {} remote cars, send interval {} ms; local car {}",
        scenario.name,
        accepted.session_id,
        accepted.track,
        tape.entries.len(),
        slots.len(),
        accepted.send_interval_ms(),
        names[0]
    );

    let mut counts = std::collections::BTreeMap::<(u8, u8), u64>::new();
    let same_messages = |game_sent: &[(Lane, Vec<u8>)], port_sent: &[(Lane, Vec<u8>)], step: usize, counts: &mut std::collections::BTreeMap<(u8, u8), u64>| -> Result<(), String> {
        let list = |sent: &[(Lane, Vec<u8>)]| -> Vec<(Lane, Vec<u8>)> { sent.iter().filter_map(|(lane, data)| comparable(*lane, data).map(|data| (*lane, data))).collect() };
        let (g, p) = (list(game_sent), list(port_sent));
        if g != p {
            let show = |sent: &[(Lane, Vec<u8>)]| sent.iter().map(|(lane, data)| format!("{lane:?} {data:02x?}")).collect::<Vec<_>>().join("\n      ");
            return Err(format!("step {step}: the outgoing messages differ\n  game: {}\n  port: {}", show(&g), show(&p)));
        }
        for (lane, data) in &g {
            *counts.entry((*lane as u8, data.first().copied().unwrap_or(0))).or_default() += 1;
        }
        Ok(())
    };
    // before the first step: the push-to-pass question (the join's own messages are the port's)
    let before: Vec<(Lane, Vec<u8>)> = port.take_sent().into_iter().filter(|(_, data)| !matches!(data.first(), Some(0x3d | 0x3f))).collect();
    same_messages(&game.ask_p2p(), &before, 0, &mut counts)?;
    // Task 43: every chat line and vote rustyAC sent on the tape, sent again by the game's own
    // sendChat / sendVote (no step sends them: the player does)
    let mut said = 0;
    for entry in tape.of(Lane::ReliableOut) {
        if let Some(game_sent) = game.send_as_game(&entry.data) {
            same_messages(&game_sent, &[(Lane::ReliableOut, entry.data.clone())], entry.tick as usize, &mut counts).map_err(|e| format!("{}: chat or vote: {e}", scenario.name))?;
            said += 1;
        }
    }
    if said > 0 {
        println!("  chat lines and votes rustyAC sent, sent again by the game's sendChat / sendVote: {said}, the same bytes");
    }

    let steps = args.steps.unwrap_or((scenario.seconds / scenario::DT) as usize);
    let width = INPUT_COLUMNS.len();
    let input_at = recording.input_offset(0);
    let local_columns = recording.columns();
    let mut values = 0u64;
    let mut hashes = Vec::new();
    // what happened, for the table: per remote car steps solid / ghost / gone / hit
    let words = rustyac_net::replay::remote_words();
    let mut seen = vec![[0u64; 5]; slots.len()];
    let mut contact_steps = 0u64;
    let mut failure = None;
    let start = std::time::Instant::now();
    let mut zones_seen: Vec<Vec<[u32; 3]>> = Vec::new();
    'steps: for step in 0..steps {
        let game_sent = game.step();
        let game_local = game.world.recording.rows.last().expect("a step leaves a row").clone();
        let inputs: Vec<&[u32]> = vec![&game_local[input_at..input_at + width]];
        let port_sent = port.step(&inputs);
        let port_local = port.world.row(&inputs);
        let (game_remote, port_remote) = (game.remote_row(), port.remote_row());
        let same = |a: u32, b: u32| a == b || (f32::from_bits(a).is_nan() && f32::from_bits(b).is_nan());
        for (i, (a, b)) in game_local.iter().zip(&port_local).enumerate() {
            if !same(*a, *b) {
                failure = Some(format!("step {step}: the local car's {} differs: game {a:08x} ({:e}), port {b:08x} ({:e})", local_columns[i], f32::from_bits(*a), f32::from_bits(*b)));
                break 'steps;
            }
        }
        for (i, (a, b)) in game_remote.iter().zip(&port_remote).enumerate() {
            if !same(*a, *b) {
                let also: Vec<String> = game_remote.iter().zip(&port_remote).enumerate().filter(|(_, (a, b))| !same(**a, **b)).skip(1).take(8).map(|(i, _)| columns[i].clone()).collect();
                failure = Some(format!("step {step}: {} differs: game {a:08x} ({:e}), port {b:08x} ({:e}); also {also:?}", columns[i], f32::from_bits(*a), f32::from_bits(*b)));
                break 'steps;
            }
        }
        // Task 44c: the server's DRS zones (0x53) take the file's place in both
        let port_zones: Vec<[u32; 3]> = port.world.engine.track.as_ref().map_or(Vec::new(), |t| t.server_drs_zones.get().unwrap_or_else(|| t.drs_zones.clone())).iter().map(|z| [z.detection.to_bits(), z.start.to_bits(), z.end.to_bits()]).collect();
        let game_zones = game.drs_zones();
        if game_zones != port_zones {
            failure = Some(format!("step {step}: the DRS zones differ: game {game_zones:x?}, port {port_zones:x?}"));
            break;
        }
        if zones_seen.last() != Some(&game_zones) {
            zones_seen.push(game_zones);
        }
        if let Err(message) = same_messages(&game_sent, &port_sent, step, &mut counts) {
            failure = Some(message);
            break;
        }
        values += (game_local.len() + game_remote.len()) as u64;
        hashes.push(step_hash(&game_local, &game_remote, &game_sent));
        contact_steps += (game_local[0] > 0) as u64;
        for (car, row) in game_remote.chunks(words).enumerate() {
            let (category, k, gone) = (row[13], f32::from_bits(row[39]), row[40]);
            seen[car][0] += (category == 8) as u64;
            seen[car][1] += (category == 0 && gone == 0) as u64;
            seen[car][2] += (gone != 0) as u64;
            seen[car][3] += (k > 0.0) as u64;
            seen[car][4] += (f32::from_bits(row[44]) > 0.0) as u64;
        }
    }
    let seconds = start.elapsed().as_secs_f64();
    let done = hashes.len();
    println!("  {done} steps in {seconds:.1} s; {values} values compared ({} of the local car + {} of the remote cars a step)", local_columns.len(), columns.len());
    for (car, slot) in slots.iter().enumerate() {
        let s = seen[car];
        if s[0] + s[1] > 0 {
            println!("  slot {slot:2} ({}): solid {} steps, ghost {} steps, gone {} steps, moving {} steps, pushed by the local car (velocity blend) {} steps", remote[car].0.model, s[0], s[1], s[2], s[4], s[3]);
        }
    }
    println!("  the track's DRS zones (start, end) in turn, the same in both: {:?}", zones_seen.iter().map(|z| z.iter().map(|w| (f32::from_bits(w[1]), f32::from_bits(w[2]))).collect::<Vec<_>>()).collect::<Vec<_>>());
    println!("  steps with contact joints in the world: {contact_steps}; reliable messages given to the game's own handlers: {}", game.game_reliable);
    for ((lane, id), count) in &counts {
        println!("  outgoing {} id {id:#04x}: {count} messages identical", if *lane == Lane::ReliableOut as u8 { "TCP" } else { "UDP" });
    }
    let verdict = match &failure {
        None => format!("{}: IDENTICAL over {done} steps ({values} values, {} outgoing messages)", scenario.name, counts.values().sum::<u64>()),
        Some(message) => format!("{}: DIFFERENT\n  {message}", scenario.name),
    };
    println!("{verdict}");
    std::fs::write(out.join(format!("{}.result.txt", scenario.name)), format!("{verdict}\n")).map_err(|e| e.to_string())?;
    if let (Some(path), None) = (&args.csv_out, &failure) {
        // a golden excerpt: the first --count steps
        let count = if args.count_given { args.count.min(done) } else { done.min(1500) };
        let entries: Vec<TapeEntry> = tape.entries.iter().filter(|entry| entry.tick < count as u64 || matches!(entry.data.first(), Some(0x3e | 0x40))).cloned().map(|mut entry| {
            // (tapes of before the blanking: no player id, no password in a golden file)
            entry.data = rustyac_net::handshake::without_secrets(&entry.data);
            entry
        }).collect();
        let golden = NetGolden { local: game.world.recording.golden(count).to_bytes(), tape: Tape { entries }, hashes: hashes[..count].to_vec() };
        std::fs::write(path, golden.to_bytes()).map_err(|e| format!("{}: {e}", path.display()))?;
        println!("wrote the golden excerpt {} ({count} steps, {} bytes)", path.display(), golden.to_bytes().len());
    }
    match failure {
        None => Ok(()),
        Some(message) => Err(message),
    }
}

fn multi_check(args: &Args) -> Result<(), String> {
    let path = args.file.as_ref().ok_or("multi-check <recording.mcrec>")?;
    let recording = rustyac_physics::engine::record::Recording::load(path)?;
    if let (Some(steps), Some(out)) = (args.steps, args.csv_out.as_ref()) {
        // `multi-check <recording> --steps <n> --csv-out <file.mcgold>`: the golden excerpt
        let golden = recording.golden(steps);
        std::fs::write(out, golden.to_bytes()).map_err(|e| format!("{}: {e}", out.display()))?;
        println!("wrote {} ({} steps)", out.display(), golden.hashes.len());
        return Ok(());
    }
    // Task 28: the game's race_out.json of a recording with a referee lies beside it
    let game_file = std::fs::read(path.with_extension("race_out.json")).ok();
    multi_compare(args, &recording, path, game_file.as_deref())
}

/// Replays a several-car recording through the port, once per thread count, and holds every
/// value against the game's.
fn multi_compare(args: &Args, recording: &rustyac_physics::engine::record::Recording, path: &Path, game_file: Option<&[u8]>) -> Result<(), String> {
    use rustyac_physics::engine::record::{load_track, replay_on, ReplayPaths};
    let repo = repo_root();
    let paths = ReplayPaths { cardata: repo.join("cardata"), game_root: args.acs.parent().map(Path::to_path_buf) };
    let track = load_track(recording, &paths)?;
    let cpus = std::thread::available_parallelism().map_or(1, |n| n.get());
    let threads = if args.threads.is_empty() { vec![0, rustyac_physics::engine::worker_threads(-1, cpus).max(2)] } else { args.threads.clone() };
    let mut ok = true;
    let mut hashes = Vec::new();
    let mut lines = Vec::new();
    for &count in &threads {
        let report = replay_on(recording, &paths, std::sync::Arc::clone(&track), count, args.fault)?;
        let verdict = match &report.mismatch {
            None => "IDENTICAL".to_string(),
            Some(m) => format!("DIFFERENT at {m}"),
        };
        let line = format!(
            "{} threads {count}: {verdict}; {} steps, {} values, contact steps {}, most contacts {}, damage {:?}, pit ghost steps {:?}, hash {:016x}, {:.1} s",
            recording.get("scenario").unwrap_or("?"),
            report.steps,
            report.values,
            report.contact_steps,
            report.max_contacts,
            report.damage,
            report.ghost_steps,
            report.hash,
            report.seconds
        );
        println!("{line}");
        for also in &report.also {
            println!("    also {also}");
        }
        // Task 44c: the TempRun file
        if let Some(port) = &report.temp_run {
            let name = format!("TempRuns/{}@{}.trun", recording.cars.first().map_or("", |c| c.name.as_str()), recording.get("track").unwrap_or(""));
            let game = std::fs::read(&name).unwrap_or_default();
            let same = game == *port;
            ok &= same;
            println!("    temp_run {name}: {} ({} bytes in the game's, {} in the port's)", if same { "IDENTICAL" } else { "DIFFERENT" }, game.len(), port.len());
        }
        // Task 28: the referee's files at the end
        if let (Some((json, laps_ini)), Some(game)) = (&report.referee_files, game_file) {
            let same = json.as_slice() == game;
            println!("    race_out.json: {} ({} bytes in the game's, {} in the port's); laps.ini {} bytes", if same { "IDENTICAL" } else { "DIFFERENT" }, game.len(), json.len(), laps_ini.len());
            if !same {
                let at = json.iter().zip(game).position(|(a, b)| a != b).unwrap_or(json.len().min(game.len()));
                let from = at.saturating_sub(80);
                println!("      the game: {:?}", String::from_utf8_lossy(&game[from..(at + 80).min(game.len())]));
                println!("      the port: {:?}", String::from_utf8_lossy(&json[from..(at + 80).min(json.len())]));
                let _ = std::fs::write(path.with_extension("port_race_out.json"), json);
            }
            let _ = std::fs::write(path.with_extension("laps.ini"), laps_ini);
            ok &= same;
        }
        if report.mismatch.as_ref().is_some_and(|m| m.column.starts_with("world.referee")) || args.verbose {
            let state: Vec<String> = report.referee_dump.iter().map(|(key, value)| format!("{key}: {value}")).collect();
            let _ = std::fs::write(path.with_extension(format!("port_referee_t{count}.txt")), state.join("\n") + "\n");
            println!("    the port's referee at that step: {}", path.with_extension(format!("port_referee_t{count}.txt")).display());
        }
        ok &= report.mismatch.is_none();
        hashes.push(report.hash);
        lines.push(line);
    }
    let same = hashes.windows(2).all(|w| w[0] == w[1]);
    println!("thread counts {threads:?}: {}", if same { "the port's rows are the same for all" } else { "THE PORT'S ROWS DIFFER BETWEEN THREAD COUNTS" });
    let _ = std::fs::write(path.with_extension("result.txt"), lines.join("\n") + &format!("\nthreads {threads:?} same: {same}\n"));
    if ok && same && args.fault.is_none() {
        Ok(())
    } else if args.fault.is_some() && !ok {
        println!("(a fault was injected: the difference is the expected result)");
        Ok(())
    } else {
        Err("the game and the port do not agree".to_string())
    }
}

/// The ray micro-oracle.
fn rays(args: &Args) -> Result<(), String> {
    let track = args.track.as_ref().ok_or("rays needs --track <track folder>")?;
    let folder = track_folder(args, track);
    let repo = repo_root();
    game::prepare_root(&repo, &args.root, &args.car)?;
    track::prepare_root(&args.root, &folder, &args.layout)?;
    std::env::set_current_dir(&args.root).map_err(|e| format!("{}: {e}", args.root.display()))?;
    let acs = acs::Acs::load(&args.acs)?;
    if args.verbose {
        acs.unbuffer_game_stdout();
    } else {
        acs.silence_game_stdout();
    }
    let engine = game::new_engine(&acs, 1);
    // --late <percent>: that share of the meshes, the last ones, come after a first ray
    let late_from = match args.late {
        Some(percent) => {
            let meshes = rustyac_physics::track::load_track(&folder, &args.layout)?.0.surfaces.len();
            Some(meshes - meshes * percent.min(100) / 100)
        }
        None => None,
    };
    let plan = track::BuildPlan { space0_every: args.space0, late_from };
    let (report, ok) = track::rays(&acs, engine, &folder, &args.layout, args.count, args.seed, &plan)?;
    let name = folder.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let path = track::results_path(&repo, &name, &args.layout, &plan);
    std::fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
    let mut layout = if args.layout.is_empty() { String::new() } else { format!(" --layout {}", args.layout) };
    if args.space0 != 0 {
        layout.push_str(&format!(" --space0 {}", args.space0));
    }
    if let Some(percent) = args.late {
        layout.push_str(&format!(" --late {percent}"));
    }
    std::fs::write(&path, format!("# Ray micro-oracle: the game's ODE / OPCODE against rustyac-ode\n\n`car_oracle rays --track {}{layout} --count {} --seed {}`\n\n{report}", track.display(), args.count, args.seed))
        .map_err(|e| e.to_string())?;
    println!("{report}");
    println!("written to {}", path.display());
    if ok {
        Ok(())
    } else {
        Err("the game and the port do not agree".to_string())
    }
}

/// The car's colliders as the port loads them: the boxes of `cardata/<car>/colliders.ini`
/// and the mesh of the game's `content/cars/<car>/collider.kn5`.
fn car_colliders(args: &Args) -> Result<rustyac_physics::car::colliders::CarColliders, String> {
    let data = repo_root().join("cardata").join(&args.car);
    rustyac_physics::car::colliders::load(&data, args.acs.parent(), &args.car)
}

fn write_results(name: &str, title: &str, command: &str, report: &str) -> Result<(), String> {
    let path = repo_root().join("oracle/collide").join(name);
    std::fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
    std::fs::write(&path, format!("# {title}\n\n`{command}`\n\n{report}")).map_err(|e| e.to_string())?;
    println!("{report}");
    println!("written to {}", path.display());
    Ok(())
}

/// The collision micro-oracle over a track.
fn collide_poses(args: &Args) -> Result<(), String> {
    let track = args.track.as_ref().ok_or("collide needs --track <track folder>")?;
    let folder = track_folder(args, track);
    let repo = repo_root();
    let colliders = car_colliders(args)?;
    game::prepare_root(&repo, &args.root, &args.car)?;
    track::prepare_root(&args.root, &folder, &args.layout)?;
    std::env::set_current_dir(&args.root).map_err(|e| format!("{}: {e}", args.root.display()))?;
    let acs = acs::Acs::load(&args.acs)?;
    if args.verbose {
        acs.unbuffer_game_stdout();
    } else {
        acs.silence_game_stdout();
    }
    let engine = game::new_engine(&acs, 1);
    let game_track = track::GameTrack::build(&acs, engine, &folder, &args.layout, false)?;
    let count = if args.count_given { args.count } else { 200_000 };
    let (report, ok) = collide::collide(&acs, engine, &game_track, &colliders, count, args.mesh_count, args.seed, args.kind, args.boxes_only)?;
    let suffix = if args.kind.is_some() || args.count_given || args.boxes_only { "poses_partial.md" } else { "poses_results.md" };
    write_results(
        suffix,
        "Collision micro-oracle: the game's ODE / OPCODE / PhysicsCore against the port, poses over a track",
        &format!("car_oracle collide --track {} --count {count} --mesh-count {} --seed {}", track.display(), args.mesh_count, args.seed),
        &report,
    )?;
    if ok {
        Ok(())
    } else {
        Err("the game and the port do not agree".to_string())
    }
}

/// The collision micro-oracle: small worlds that are stepped.
fn collide_worlds(args: &Args) -> Result<(), String> {
    let repo = repo_root();
    let colliders = car_colliders(args)?;
    game::prepare_root(&repo, &args.root, &args.car)?;
    std::env::set_current_dir(&args.root).map_err(|e| format!("{}: {e}", args.root.display()))?;
    let acs = acs::Acs::load(&args.acs)?;
    if args.verbose {
        acs.unbuffer_game_stdout();
    } else {
        acs.silence_game_stdout();
    }
    let engine = game::new_engine(&acs, 1);
    let steps = args.steps.unwrap_or(1500);
    let (report, ok) = collide::worlds(&acs, engine, &colliders, steps, args.seed, &args.only, args.hybrid)?;
    write_results(
        if args.hybrid {
            "worlds_hybrid.md"
        } else if args.only.is_empty() && args.steps.is_none() {
            "worlds_results.md"
        } else {
            "worlds_partial.md"
        },
        "Collision micro-oracle: small worlds with contacts, stepped in the game's PhysicsCore and in the port",
        &format!("car_oracle collide-worlds --steps {steps} --seed {}{}", args.seed, if args.hybrid { " --hybrid" } else { "" }),
        &report,
    )?;
    if ok {
        Ok(())
    } else {
        Err("the game and the port do not agree".to_string())
    }
}

fn recording_path(out: &Path, name: &str) -> PathBuf {
    out.join(format!("{name}.carrec"))
}

/// The suspension micro-oracle on one car: the game's own suspension objects against the
/// port's on random states (see `game/sus_micro.rs`). One car per process.
fn sus_micro(args: &Args) -> Result<(), String> {
    let scenario = scenario::find("wc_stops").expect("the scenario exists");
    let repo = repo_root();
    game::prepare_root(&repo, &args.root, &args.car)?;
    std::env::set_current_dir(&args.root).map_err(|e| format!("{}: {e}", args.root.display()))?;
    let acs = acs::Acs::load(&args.acs)?;
    acs.silence_game_stdout();
    let options = game::Options {
        joint_feedback: false,
        car: args.car.clone(),
        setup_check: false,
        track: None,
        layout: String::new(),
        collide: false,
        colliders: None,
        collider_kn5: None,
        setup: None,
    };
    let mut world = game::World::build(&acs, &scenario, &options);
    let count = if args.count == 1_000_000 { 20_000 } else { args.count };
    let data = repo.join("cardata").join(&args.car);
    let report = world.sus_micro(&data, scenario.seed, count, args.seed)?;
    let text = format!(
        "# Suspension micro-oracle: `{}`

`car_oracle sus-micro --car {} --count {count} --seed {}`

{}",
        args.car, args.car, args.seed, report.text
    );
    println!("{}", report.text);
    let dir = repo.join("oracle/sus_micro");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join(format!("{}_results.md", args.car));
    std::fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))?;
    println!("{}", path.display());
    if report.ok {
        Ok(())
    } else {
        Err("the game and the port do not agree".to_string())
    }
}

/// One scenario, in this process (the game's objects cannot be torn down and rebuilt).
fn run(args: &Args) -> Result<(), String> {
    let mut found =
        scenario::find(&args.scenario).ok_or_else(|| format!("no scenario {:?} (see `car_oracle list`)", args.scenario))?;
    let scenario = &mut found;
    let name = if args.floor && !scenario.floor { format!("{}_floor", scenario.name) } else { scenario.name.to_string() };
    scenario.floor |= args.floor;
    if let Some(damage) = args.damage {
        if !scenario.whole.on {
            return Err(format!("--damage needs a whole-car scenario; {} is not one", scenario.name));
        }
        scenario.whole.damage = damage;
    }
    let scenario = &*scenario;
    let repo = repo_root();
    // relative paths are the caller's; the game needs its own working directory
    let out = std::path::absolute(&args.out).map_err(|e| e.to_string())?;
    let data_hash = game::prepare_root(&repo, &args.root, &args.car)?;
    let track_folder = match (&args.track, scenario.track_kind()) {
        (Some(track), Some(_)) => {
            let folder = track_folder(args, track);
            track::prepare_root(&args.root, &folder, &args.layout)?;
            Some(folder)
        }
        (None, Some(_)) => return Err(format!("the scenario {} runs on a track: add --track <folder> (for example --track spa)", scenario.name)),
        (Some(_), None) => return Err(format!("the scenario {} runs on the flat road; the track scenarios are spa_...", scenario.name)),
        (None, None) => None,
    };
    std::env::set_current_dir(&args.root).map_err(|e| format!("{}: {e}", args.root.display()))?;

    let acs = acs::Acs::load(&args.acs)?;
    if args.verbose {
        acs.unbuffer_game_stdout();
    } else {
        acs.silence_game_stdout();
    }
    let colliders = if args.collide { Some(car_colliders(args)?) } else { None };
    let options = game::Options {
        joint_feedback: args.joint_forces,
        car: args.car.clone(),
        setup_check: false,
        track: track_folder,
        layout: args.layout.clone(),
        collide: args.collide,
        colliders,
        collider_kn5: args.acs.parent().map(|game| rustyac_physics::car::colliders::collider_kn5_path(game, &args.car)),
        setup: args.setup.clone(),
    };
    let mut world = game::World::build(&acs, scenario, &options);
    let steps = args.steps.unwrap_or(scenario.steps);
    let mut meta = vec![
        ("scenario".to_string(), name.clone()),
        ("about".to_string(), scenario.about.to_string()),
        ("car".to_string(), args.car.clone()),
        ("car_data_hash".to_string(), format!("{data_hash:016x}")),
        ("dt".to_string(), format!("{:?}", scenario::DT)),
        ("clock_start_ms".to_string(), format!("{:?}", game::CLOCK_START_MS)),
        ("seed".to_string(), scenario.seed.to_string()),
        ("auto_clutch".to_string(), (scenario.auto_clutch as u8).to_string()),
        ("auto_shifter".to_string(), (scenario.auto_shifter as u8).to_string()),
        ("ground".to_string(), scenario.ground.describe()),
        ("floor_mesh".to_string(), (scenario.floor as u8).to_string()),
        ("joint_forces".to_string(), (args.joint_forces as u8).to_string()),
        ("wheels".to_string(), game::WHEELS.join(",")),
    ];
    meta.extend(world.facts());
    if scenario.whole.on {
        // only the whole-car scenarios: the older recordings keep their header (and hashes)
        meta.push(("whole".to_string(), "1".to_string()));
        meta.push(("pitlane".to_string(), (scenario.whole.pitlane as u8).to_string()));
        meta.push(("stability_gain".to_string(), format!("{:?}", scenario.whole.stability_gain)));
        meta.push(("wind_speed".to_string(), format!("{:?}", scenario.whole.wind_speed)));
        meta.push(("wind_direction_deg".to_string(), format!("{:?}", scenario.whole.wind_direction_deg)));
        meta.push(("damage".to_string(), scenario.whole.damage.map(|d| format!("{d:?}")).join(",")));
        let penalty_mode = if scenario.track_kind().is_some() {
            "1"
        } else if scenario.whole.penalty_cut_gas {
            "0"
        } else {
            "3"
        };
        meta.push(("penalty_mode".to_string(), penalty_mode.to_string()));
    }
    meta.extend(world.track_meta.clone());
    meta.extend(world.conditions_meta.clone());
    let path = recording_path(&out, &name);
    let mut writer = Writer::new((!args.hash_only).then_some(path.as_path()), meta).map_err(|e| e.to_string())?;
    let mut sites = BTreeSet::new();
    let mut steps = steps;
    // Task 19: the audio tape. A header of `key=value` lines and an empty line, then records:
    // 1 + 0x48 bytes = a physics event (in the order the engine queued them), 2 + u32 step
    // count + 0xb70 bytes + 1 byte = a picture frame with the car's state after that many steps
    // and whether its traction control is acting.
    // Frame n comes after floor(n * 50 / 9) steps: 60 frames a second of 3 ms steps.
    let mut tape = if args.audio_tape {
        use std::io::Write as _;
        world.audio_events = Some(std::cell::RefCell::new(Vec::new()));
        let tape_path = out.join(format!("{name}.audiotape"));
        std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
        let mut file = std::io::BufWriter::new(std::fs::File::create(&tape_path).map_err(|e| format!("{}: {e}", tape_path.display()))?);
        let mut header = format!("audiotape=1\nscenario={name}\ncar={}\nframe_hz=60\nstep_ms=3\n", args.car);
        for (key, value) in world.track_meta.iter() {
            header.push_str(&format!("{key}={value}\n"));
        }
        header.push('\n');
        file.write_all(header.as_bytes()).map_err(|e| e.to_string())?;
        Some(file)
    } else {
        None
    };
    let mut next_frame = 1usize;
    for i in 0..steps {
        if world.ended() {
            // the car left the track for good: its body would be in a wall
            steps = i;
            break;
        }
        let (row, calls) = world.step(i == 0);
        for call in &calls {
            sites.insert(call.site);
            if call.outer_site != 0 {
                sites.insert(call.outer_site);
            }
        }
        writer.step(&row, &calls).map_err(|e| e.to_string())?;
        if let Some(file) = tape.as_mut() {
            use std::io::Write as _;
            world.pop_audio_events();
            for event in world.audio_events.as_ref().unwrap().borrow_mut().drain(..) {
                file.write_all(&[1]).and_then(|_| file.write_all(&event)).map_err(|e| e.to_string())?;
            }
            if next_frame * 50 / 9 <= i + 1 {
                next_frame += 1;
                file.write_all(&[2])
                    .and_then(|_| file.write_all(&(i as u32 + 1).to_le_bytes()))
                    .and_then(|_| file.write_all(world.physics_state()))
                    .and_then(|_| file.write_all(&[world.tc_in_action()]))
                    .map_err(|e| e.to_string())?;
            }
        }
    }
    if let Some(mut file) = tape.take() {
        use std::io::Write as _;
        file.flush().map_err(|e| e.to_string())?;
    }
    let mut trailer = String::new();
    for site in sites {
        trailer.push_str(&sites::describe(site));
        trailer.push('\n');
    }
    let (hash, bytes) = writer.finish(&trailer).map_err(|e| e.to_string())?;
    println!("{name} steps={steps} bytes={bytes} hash={hash:016x}");
    Ok(())
}

/// Runs `car_oracle run` for one scenario in a child process and returns its hash.
fn run_child(args: &Args, name: &str, hash_only: bool) -> Result<String, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let mut command = std::process::Command::new(exe);
    command.arg("run").arg("--scenario").arg(name).arg("--out").arg(&args.out);
    command.arg("--acs").arg(&args.acs).arg("--root").arg(&args.root).arg("--car").arg(&args.car);
    if let Some(steps) = args.steps {
        command.arg("--steps").arg(steps.to_string());
    }
    if hash_only {
        command.arg("--hash-only");
    }
    if !args.joint_forces {
        command.arg("--no-joint-forces");
    }
    let output = command.output().map_err(|e| e.to_string())?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    if !output.status.success() {
        return Err(format!(
            "{name}: the run failed ({})\n{}{}",
            output.status,
            stdout,
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    stdout
        .lines()
        .find_map(|line| line.strip_prefix(name)?.split("hash=").nth(1).map(str::to_string))
        .ok_or_else(|| format!("{name}: no hash in the run's output:\n{stdout}"))
}

fn all(args: &Args) -> Result<(), String> {
    let repo = repo_root();
    let data = repo.join("cardata").join(&args.car);
    let mut table = String::from(
        "| scenario | steps | two runs identical | tyre forces = Rust tyre | all tyre values = Rust tyre | \
         tape closes | tape calls explained | tyre calls on tape | headline numbers |\n\
         |---|---|---|---|---|---|---|---|---|\n",
    );
    let mut notes = String::new();
    let mut failed = false;
    // the powertrain scenarios only run when they are named
    for scenario in scenario::all().into_iter().chain(scenario::powertrain()).chain(scenario::whole()).chain(scenario::hybrid()) {
        if scenario.powertrain && args.only.is_empty() {
            continue;
        }
        if !args.only.is_empty() && !args.only.iter().any(|n| n == scenario.name) {
            continue;
        }
        let first = run_child(args, scenario.name, false)?;
        let second = run_child(args, scenario.name, true)?;
        let identical = first == second;
        let recording = Recording::read(&recording_path(&args.out, scenario.name))?;
        let report = check::check(&recording, &data)?;
        failed |= !identical
            || report.force_steps_matching != report.wheel_steps
            || report.full_steps_matching != report.wheel_steps
            || report.tape_steps_closed != recording.steps.len()
            || report.tape_calls_explained != report.tape_calls
            || report.tape_tyre_links != report.wheel_steps;
        let line = format!(
            "| `{}` | {} | {} | {} | {} | {} | {} | {} | {} |",
            scenario.name,
            recording.steps.len(),
            if identical { format!("yes (`{first}`)") } else { format!("**NO** (`{first}` / `{second}`)") },
            check::percent(report.force_steps_matching, report.wheel_steps),
            check::percent(report.full_steps_matching, report.wheel_steps),
            check::percent(report.tape_steps_closed, recording.steps.len()),
            check::percent(report.tape_calls_explained, report.tape_calls),
            check::percent(report.tape_tyre_links, report.wheel_steps),
            report.headline.join("; "),
        );
        println!("{line}");
        for note in &report.notes {
            println!("    {note}");
            notes.push_str(&format!("- `{}`: {note}\n", scenario.name));
        }
        table.push_str(&line);
        table.push('\n');
    }
    if !notes.is_empty() {
        table.push_str("\nNotes:\n\n");
        table.push_str(&notes);
    }
    let path = args.out.join("results.md");
    std::fs::write(&path, &table).map_err(|e| format!("{}: {e}", path.display()))?;
    println!("wrote {}", path.display());
    if failed {
        return Err("at least one check of at least one scenario is below 100 %".into());
    }
    Ok(())
}

fn check_one(args: &Args) -> Result<(), String> {
    let file = args.file.as_ref().ok_or_else(usage)?;
    let recording = Recording::read(file)?;
    let car = recording.get("car").unwrap_or(game::DEFAULT_CAR);
    let report = check::check(&recording, &repo_root().join("cardata").join(car))?;
    println!(
        "{}: {} steps; tyre forces match in {} of wheel-steps, every tyre value in {}",
        recording.get("scenario").unwrap_or("?"),
        recording.steps.len(),
        check::percent(report.force_steps_matching, report.wheel_steps),
        check::percent(report.full_steps_matching, report.wheel_steps),
    );
    println!(
        "  force tape: {} of calls explain the accumulators bit for bit; nothing after the last call in {} of \
         steps; the tyre's hub calls equal the tape's in {} of wheel-steps",
        check::percent(report.tape_calls_explained, report.tape_calls),
        check::percent(report.tape_steps_closed, recording.steps.len()),
        check::percent(report.tape_tyre_links, report.wheel_steps),
    );
    for line in report.headline.iter().chain(&report.notes) {
        println!("  {line}");
    }
    Ok(())
}

/// What the session start does to a car's setup, printed by the game's own code.
fn setup_check(args: &Args) -> Result<(), String> {
    let repo = repo_root();
    game::prepare_root(&repo, &args.root, &args.car)?;
    std::env::set_current_dir(&args.root).map_err(|e| format!("{}: {e}", args.root.display()))?;
    let acs = acs::Acs::load(&args.acs)?;
    acs.unbuffer_game_stdout();
    let scenarios = scenario::all();
    let options = game::Options { joint_feedback: false, car: args.car.clone(), setup_check: true, track: None, layout: String::new(), collide: false, colliders: None, collider_kn5: None, setup: None };
    let world = game::World::build(&acs, &scenarios[0], &options);
    println!("{} values change: {}", world.setup_changes.len(), world.setup_changes.join(", "));
    Ok(())
}

/// Compares two recordings value by value (`--only` limits the fields by prefix; fields whose
/// name contains one of `--ignore`'s parts are skipped).
fn diff(args: &Args) -> Result<(), String> {
    let (a, b) = match (&args.file, &args.file2) {
        (Some(a), Some(b)) => (Recording::read(a)?, Recording::read(b)?),
        _ => return Err(usage()),
    };
    let report = record::diff(&a, &b, &args.only, &args.ignore);
    for line in &report {
        println!("{line}");
    }
    Ok(())
}

fn csv(args: &Args) -> Result<(), String> {
    let file = args.file.as_ref().ok_or_else(usage)?;
    let recording = Recording::read(file)?;
    let out = args.csv_out.clone().unwrap_or_else(|| file.with_extension(format!("{}.csv", args.table)));
    let options = CsvOptions {
        table: args.table.clone(),
        from: args.from,
        to: args.to,
        every: args.every,
        only: args.only.clone(),
    };
    let rows = record::to_csv(&recording, &options, &out)?;
    println!("{rows} rows -> {}", out.display());
    Ok(())
}
