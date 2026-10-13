// SPDX-License-Identifier: GPL-3.0-or-later

//! `rustyac.exe`: see `docs/game/first_drive.md`.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rustyac_game::autodrive::AutoDriver;
use rustyac_game::cli::Options;
use rustyac_game::input::bindings::{Bindings, KEY_PLAIN_ABS, KEY_PLAIN_BIAS_DN, KEY_PLAIN_BIAS_UP, KEY_PLAIN_TC};
use rustyac_game::input::keyboard::KeyboardCarControl;
use rustyac_game::input::pad::XInput;
use rustyac_game::input::wheel::WheelDevice;
use rustyac_game::input::LiveSource;
use rustyac_game::input_file::{event, InputFile, InputWriter, SimSetup, StepInput};
use rustyac_game::audio::{AudioSink, Feed, GameAudio};
use rustyac_game::physics_thread::{self, LoopConfig, Recorder, Shared, StepSink, Timing};
use rustyac_game::render::hud::HudInfo;
use rustyac_game::render::models::ModelOptions;
use rustyac_game::render::scene::{free_camera, CameraFrame, CarShape, DrivingCamera};
use rustyac_game::render::ac::{AcOptions, AcRenderer, GraphicsPreset};
use rustyac_game::ac_replay::{ReplayKey, ReplayRig};
use rustyac_game::ghost::{GhostOptions, GhostRig};
use rustyac_game::telemetry::TelemetryRig;
use rustyac_physics::replay::ReplayEvent;
use rustyac_game::render::{box_downscale, write_png, DebugRenderer, Picture};
use rustyac_game::shm::{SharedMemory, ShmSink};
use rustyac_game::sim::{DriverSource, GameSim, NobodySource, ReplaySource, SpawnSequence};
use rustyac_game::focus::{Focus, FocusCommand};
use rustyac_game::view::CarView;
use rustyac_game::window::{Event, MouseAction, Window};

/// The game's default keys of the commands "Hide Show Apps", "Cycle Virtual Desktop", "Driver
/// Names", "Show Damage" and "Hide Damage" (`CommandManager::CommandManager` 0x1400e8b70), and
/// rustyAC's own key for its own display.
const KEY_HIDE_APPS: i32 = 0x48;
const KEY_CYCLE_DESKTOP: i32 = 0x55;
const KEY_DRIVER_NAMES: i32 = 0x4c;
const KEY_SHOW_DAMAGE: i32 = 0x4a;
const KEY_HIDE_DAMAGE: i32 = 0x51;
const KEY_DEBUG_HUD: i32 = 0x79;
use windows::Win32::System::Console::SetConsoleCtrlHandler;
use windows::Win32::System::Power::{SetThreadExecutionState, ES_CONTINUOUS, ES_DISPLAY_REQUIRED};

/// Ctrl+C was pressed or the console is being closed: every loop ends, so that the force
/// feedback is taken off, the recording is finished and the shared memory says "off".
static STOP: AtomicBool = AtomicBool::new(false);
/// The program has finished its work (the console handler waits for this before it lets
/// Windows end the process).
static DONE: AtomicBool = AtomicBool::new(false);

unsafe extern "system" fn console_handler(_kind: u32) -> windows::core::BOOL {
    STOP.store(true, Ordering::Relaxed);
    // when the console is closed the process dies as soon as this returns: give the loops
    // a moment to end tidily
    for _ in 0..300 {
        if DONE.load(Ordering::Relaxed) {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    true.into()
}

/// The car and session of a live drive: from the command line, and the conditions of the
/// game's own last session (race.ini) unless that is switched off.
fn live_setup(options: &Options) -> Result<SimSetup, String> {
    // the track by the name the user gave it, the layout by --layout or race.ini's CONFIG_TRACK
    let (mut track, mut layout) = (String::new(), String::new());
    if let Some(name) = &options.track {
        let race = rustyac_game::conditions::race_track(options);
        let found = rustyac_game::sim::resolve_track(name, options.layout.as_deref(), race.as_ref())?;
        for note in &found.notes {
            println!("{note}");
        }
        // by name when it is one of the game's own folder (a recording then replays on
        // another PC), by path otherwise
        let in_game = rustyac_content::install::ac_root().is_some_and(|root| root.join("content").join("tracks").join(&found.entry.track) == found.entry.folder);
        track = if in_game { found.entry.track.clone() } else { found.entry.folder.display().to_string() };
        layout = found.entry.layout;
    }
    let mut setup = SimSetup {
        car: options.car.clone(),
        // (the aids: `conditions::apply` below)
        auto_clutch: true,
        track,
        layout,
        track_objects: true,
        session_transfer: true,
        spawn: options.spawn.clone(),
        session_starts_at_spawn: true,
        drs_zones: true,
        ..SimSetup::default()
    };
    // normal play: the picture's rand() starts from the clock, as the game's does (Sim::Sim
    // and CameraFacing seed it from the time); pictures and unattended runs keep a fixed start
    if let Some(seed) = options.render_seed {
        setup.render_seed = seed;
    } else if options.screenshot.is_none() && !options.headless {
        setup.render_seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u32).unwrap_or(1);
    }
    // Task 24: the other cars
    if let Some(count) = options.cars {
        let drive = if setup.track.is_empty() { rustyac_game::input_file::OpponentDrive::Parked } else { rustyac_game::input_file::OpponentDrive::Auto };
        for _ in 1..count {
            setup.opponents.push(rustyac_game::input_file::Opponent { car: options.car.clone(), skin: String::new(), drive: drive.clone(), place: None });
        }
    }
    // Task 27: AC's AI drivers. `--ai-level` / `--ai-aggression` as race.ini's AI_LEVEL /
    // AI_AGGRESSION become a driver's basePush and aggression
    let ai_push = (options.ai_level.unwrap_or(100.0) * 0.01).clamp(0.0, 1.0);
    let ai_aggression = (options.ai_aggression.unwrap_or(0.0) * 0.01).clamp(0.0, 1.0) * 0.95;
    if let Some(count) = options.ai_cars {
        for _ in 1..count {
            setup.opponents.push(rustyac_game::input_file::Opponent { car: options.car.clone(), skin: String::new(), drive: rustyac_game::input_file::OpponentDrive::Ai { push: ai_push, aggression: ai_aggression }, place: None });
        }
    }
    for text in &options.opponents {
        let mut opponent = rustyac_game::input_file::Opponent::parse(text)?;
        if text.to_ascii_lowercase().ends_with(":ai") {
            opponent.drive = rustyac_game::input_file::OpponentDrive::Ai { push: ai_push, aggression: ai_aggression };
        }
        if let rustyac_game::input_file::OpponentDrive::Recorded(file) = &mut opponent.drive {
            // (the whole path goes into a recording's header: a replay finds the file from anywhere)
            if !file.is_file() {
                return Err(format!("--opponent {text}: {} is not a file", file.display()));
            }
            *file = std::path::absolute(&*file).map_err(|e| e.to_string())?;
        }
        setup.opponents.push(opponent);
    }
    setup.physics_threads = options.physics_threads;
    // the device the bindings name decides one aid (a pad has no clutch pedal)
    let input_method = Bindings::load(options, &mut Vec::new()).input_method;
    let notes = rustyac_game::conditions::apply(options, &mut setup, &input_method)?;
    // Task 27: the AI drivers' session
    let ai_cars = setup.opponents.iter().any(|opponent| matches!(opponent.drive, rustyac_game::input_file::OpponentDrive::Ai { .. }));
    if ai_cars && setup.session.session_type.is_none() && !setup.track.is_empty() {
        // no session of race.ini: a race from the grid, or practice from the pit boxes
        if !options.spawn_given {
            setup.spawn = "start".to_string();
        }
        setup.session.session_type = Some(if setup.spawn == "pit" { 1 } else { 3 });
    }
    if let Some(laps) = options.ai_laps {
        setup.ai.laps = laps;
    }
    if let Some(seconds) = options.race_start_in {
        setup.ai.start_delay_ms = seconds * 1000.0;
    }
    if let Some(seconds) = options.ai_at {
        setup.ai.hand_over_step = (seconds / 0.003).round() as i64;
    }
    // Task 28: the referee. race.ini's sessions (conditions::apply), or --sessions, or a race
    // of --ai-laps laps when AI drivers race without a race.ini session
    if options.no_referee {
        setup.referee = None;
    } else if !setup.track.is_empty() {
        let cars = 1 + setup.opponents.len();
        let penalties = options.penalties.unwrap_or(setup.session.penalties);
        let made = match &options.sessions {
            Some(list) => Some(rustyac_game::conditions::sessions_from_list(list, cars, penalties, options.jump_start, setup.env.base_pit_penalty_laps)?),
            None if setup.referee.is_none() && ai_cars && setup.session.session_type == Some(3) => Some(rustyac_game::conditions::sessions_from_list(&format!("r{}", setup.ai.laps.max(1)), cars, penalties, options.jump_start, setup.env.base_pit_penalty_laps)?),
            None => None,
        };
        if let Some(sessions) = made {
            let first = &sessions.sessions[0];
            setup.session.session_type = Some(first.session_type);
            if !options.spawn_given {
                setup.spawn = if first.spawn_set == "START" { "start" } else { "pit" }.to_string();
            }
            if let Some(mode) = sessions.jump_start_penalty_mode {
                setup.env.jump_start_penalty_mode = mode;
            }
            setup.env.allowed_tyres_out = sessions.allowed_tyres_out();
            let wind_base = setup.referee.as_ref().and_then(|referee| referee.wind_base);
            setup.referee = Some(rustyac_game::input_file::RefereeHeader { sessions, over_time_mult: rustyac_game::conditions::over_time_mult(), first_start_delay_ms: 20000.0, next_start_delay_ms: 10000.0, seed: 0, wind_base, driver_names: Vec::new(), pit: (!options.no_pit_stop).then(|| rustyac_game::conditions::pit_header((0, 0), setup.session.setup_file.as_deref())) });
        }
        // Task 28b: --pit-window <open>,<close>
        if let (Some(window), Some(pit)) = (options.pit_window, setup.referee.as_mut().and_then(|referee| referee.pit.as_mut())) {
            pit.window = window;
        }
        if let Some(referee) = &mut setup.referee {
            if let Some(penalties) = options.penalties {
                referee.sessions.penalties_enabled = penalties;
                setup.env.allowed_tyres_out = referee.sessions.allowed_tyres_out();
            }
            if let Some(seconds) = options.race_start_in {
                referee.first_start_delay_ms = (seconds * 1000.0) as f32;
                referee.next_start_delay_ms = (seconds * 1000.0) as f32;
            }
            // (a live drive draws anew every time; pictures and unattended runs do not)
            referee.seed = match options.ai_seed {
                Some(seed) => seed.wrapping_add(28_000),
                None if options.screenshot.is_none() && !options.headless => std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u32).unwrap_or(0),
                None => 28_000,
            };
        }
    }
    // (a live drive draws anew every time, as the game's rand() starts from the clock)
    setup.ai.seed = match options.ai_seed {
        Some(seed) => seed,
        None if (ai_cars || setup.ai.player || options.ai_at.is_some()) && options.screenshot.is_none() && !options.headless => {
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u32).unwrap_or(0)
        }
        None => 0,
    };
    for note in notes {
        println!("{note}");
    }
    if !setup.opponents.is_empty() {
        println!(
            "{} other car(s), drawn and heard as the game draws and plays them. Ctrl+{} / Ctrl+{}: the camera to the car ahead / behind on the track, Ctrl+{} or F4: back to yours",
            setup.opponents.len(),
            rustyac_game::input::bindings::key_name(0x63),
            rustyac_game::input::bindings::key_name(0x61),
            rustyac_game::input::bindings::key_name(0x62)
        );
    }
    Ok(setup)
}

/// Puts the track's and the car's 3D models on the graphics card, where there are any. A
/// model that cannot be loaded is a line on the console, not an end: the grid and the boxes
/// are still there.
fn load_models(renderer: &mut DebugRenderer, options: &Options, info: &rustyac_game::sim::CarInfo) {
    let model_options = ModelOptions { texture_size: options.texture_size, flat: options.no_textures, ..ModelOptions::default() };
    let megabytes = |bytes: u64| bytes as f64 / 1_048_576.0;
    if let Some(folder) = &info.track_folder {
        let loaded = rustyac_content::TrackFiles::find_lenient(folder, &info.track_layout).and_then(|files| {
            // (a model file that is not there is left out, as in the physics)
            let models: Vec<_> = files.models.iter().filter(|m| m.file.is_file()).collect();
            let paths: Vec<_> = models.iter().map(|m| m.file.clone()).collect();
            let placements: Vec<_> = models.iter().map(|m| rustyac_game::render::models::Placement { position: m.position, rotation: m.rotation }).collect();
            renderer.load_track(&paths, &placements, &model_options)
        });
        match loaded {
            Ok(stats) => {
                println!(
                    "track models: {} files, {} meshes, {} triangles ({:.0} MB), {} textures ({:.0} MB, longest side {}), {} materials in flat colour, loaded in {:.2} s",
                    stats.files,
                    stats.meshes,
                    stats.triangles,
                    megabytes(stats.geometry_bytes),
                    stats.textures,
                    megabytes(stats.texture_bytes),
                    if stats.texture_size == 0 { "as stored".to_string() } else { format!("{} px", stats.texture_size) },
                    stats.flat_materials,
                    stats.load_seconds
                );
                for note in &stats.notes {
                    println!("  {note}");
                }
            }
            Err(message) => eprintln!("WARNING: the track's models were not loaded ({message}): the grid is drawn instead"),
        }
    }
    renderer.boxes = options.boxes;
    if !options.boxes {
        match rustyac_game::sim::find_car_model(&info.name, &info.data_path) {
            Some(file) => match renderer.load_car(&file, &model_options) {
                Ok(stats) => {
                    println!(
                        "car model {}: {} meshes, {} triangles, {} textures ({:.0} MB), {} materials in flat colour, loaded in {:.2} s",
                        file.display(),
                        stats.meshes,
                        stats.triangles,
                        stats.textures,
                        megabytes(stats.texture_bytes),
                        stats.flat_materials,
                        stats.load_seconds
                    );
                    for note in &stats.notes {
                        println!("  {note}");
                    }
                }
                Err(message) => eprintln!("WARNING: the car's model was not loaded ({message}): boxes are drawn instead"),
            },
            None => println!("no 3D model of {} in Assetto Corsa's folder: the car is drawn as boxes", info.name),
        }
    }
}

/// AC's own renderer with the track and the car loaded.
fn make_ac(options: &Options, width: u32, height: u32, info: &rustyac_game::sim::CarInfo, others: &[rustyac_game::sim::CarInfo]) -> Result<AcRenderer, String> {
    let game = rustyac_content::install::ac_root().ok_or_else(rustyac_content::install::not_found_hint)?;
    // the sun and the weather of race.ini; without one, midday and clear
    let (mut sun_angle, mut weather, mut skin) = (-16.0f32, "3_clear".to_string(), options.skin.clone());
    // without a race.ini: the time runs at its own pace and the clouds drift
    let mut sun_animation = Some((1.0f32, 0.2f32));
    let mut groove = None;
    let mut race_session = false;
    let mut session_type = None;
    let race = match (&options.race_ini_file, options.race_ini) {
        (Some(file), _) => Some(file.clone()),
        (None, Some(false)) => None,
        (None, _) => rustyac_game::conditions::race_ini_path().filter(|p| p.is_file()),
    };
    let assists_file = race.as_ref().map(|p| p.with_file_name("assists.ini")).filter(|p| p.is_file());
    let race_reader = race.as_ref().and_then(|p| rustyac_physics::data::ini::IniReader::load(p).ok());
    if let Some(ini) = race.and_then(|p| rustyac_physics::data::ini::IniReader::load(&p).ok()) {
        if ini.has_section("LIGHTING") {
            sun_angle = ini.get_float("LIGHTING", "SUN_ANGLE").unwrap_or(sun_angle);
            sun_animation = Some((ini.get_float("LIGHTING", "TIME_MULT").unwrap_or(0.0), ini.get_float("LIGHTING", "CLOUD_SPEED").unwrap_or(0.0)));
        } else {
            sun_animation = None;
        }
        race_session = ini.get_int("SESSION_0", "TYPE").unwrap_or(0) == 3;
        session_type = ini.get_int("SESSION_0", "TYPE").ok().filter(|t| *t > 0);
        weather = if ini.has_section("WEATHER") { ini.get_string("WEATHER", "NAME") } else { String::new() };
        if ini.has_section("GROOVE") {
            groove = Some((ini.get_float("GROOVE", "MAX_LAPS").unwrap_or(0.0), ini.get_float("GROOVE", "STARTING_LAPS").unwrap_or(0.0)));
        }
        if skin.is_none() {
            let name = ini.get_string("CAR_0", "SKIN");
            if !name.is_empty() {
                skin = Some(name);
            }
        }
    }
    // DrivingAssistManager::DrivingAssistManager 0x1400fbd90: assists.ini [ASSISTS] IDEAL_LINE,
    // beside the race.ini (not with --no-race-ini); --ideal-line / --no-ideal-line come first
    let mut ideal_line = false;
    if let Some(assists) = assists_file.and_then(|p| rustyac_physics::data::ini::IniReader::load(&p).ok()) {
        ideal_line = assists.get_int("ASSISTS", "IDEAL_LINE").unwrap_or(0) != 0;
    }
    if let Some(on) = options.ideal_line {
        ideal_line = on;
    }
    rustyac_game::shm::IDEAL_LINE_ON.store(ideal_line, Ordering::Relaxed);
    if let Some(sun) = options.sun {
        sun_angle = sun;
    }
    // a session of race.ini with other cars: the player's skin as the game makes it
    if options.skin.is_none() && info.skin.is_some() {
        skin = info.skin.clone();
    }
    // a race start: --start-lights <seconds>, or the 20 s of RaceManager::startRace when
    // race.ini's session is a race
    let start_lights = options.start_lights.or(race_session.then_some(20.0));
    if let Some(name) = &options.weather {
        weather = name.clone();
    }
    if let (Some(mult), Some((time_mult, _))) = (options.time_mult, &mut sun_animation) {
        *time_mult = mult;
    }
    // --no-a2c (NOT Assetto Corsa): before the device makes its blend states
    rustyac_render::kgl::EXPERIMENT_NO_ALPHA_TO_COVERAGE.store(options.no_a2c, Ordering::Relaxed);
    let mut renderer = AcRenderer::new(width, height, AcOptions { warp: options.warp, gpu_log: options.gpu_log.clone(), game: game.clone(), sun_angle, weather, skin: None, video_exact: options.video_ini_exact, cube_faces: options.cube_faces, mirror_size: options.mirror_size, sun_animation, groove, mirror_hq: options.mirror_hq, world_detail: options.world_detail, render_seed: info.render_seed, cube_size: options.cube_size, ideal_line, start_lights, session_type: session_type.unwrap_or(if options.spawn == "hotlap" { 4 } else { 1 }), video_ini: options.video_ini.clone(), preset: options.graphics.as_deref().map(GraphicsPreset::from_name).transpose()?, aa_samples: options.aa_samples, post: options.post, fxaa: options.fxaa, motion_blur: options.motion_blur, hdr_levels: options.hdr_levels })?;
    for note in &renderer.notes {
        println!("{note}");
    }
    if let Some(folder) = &info.track_folder {
        println!("{}", renderer.load_track(folder, &info.track_layout)?);
    }
    // RUSTYAC_DIAG_DRAW_ONE_CAR=1 (a diagnosis switch, NOT Assetto Corsa): the other cars are in
    // the physics but not in the picture, to tell what their physics costs the frame rate from
    // what drawing them costs
    let others: &[rustyac_game::sim::CarInfo] = if std::env::var_os("RUSTYAC_DIAG_DRAW_ONE_CAR").is_some_and(|v| v == "1") {
        println!("RUSTYAC_DIAG_DRAW_ONE_CAR=1: NOT Assetto Corsa, a diagnosis switch: only your car is drawn");
        &[]
    } else {
        others
    };
    // RaceManager::carsToBeLoaded, before the first car is made
    renderer.set_cars_to_be_loaded(1 + others.len());
    // Sim::addCar for every car in the order of the physics: the player's first. A car's folder
    // in the game holds its models (its data may come from elsewhere).
    let load = |renderer: &mut AcRenderer, info: &rustyac_game::sim::CarInfo, skin: Option<String>| -> Result<bool, String> {
        let Some(folder) = rustyac_game::sim::find_car_model(&info.name, &info.data_path).and_then(|kn5| kn5.parent().map(|p| p.to_path_buf())) else {
            return Ok(false);
        };
        // a skin that the car does not have: the first one
        let skin = skin.filter(|s| folder.join("skins").join(s).is_dir());
        renderer.set_skin(skin);
        let steer_lock = rustyac_physics::data::ini::IniReader::load(&info.data_path.join("car.ini")).ok().and_then(|ini| ini.get_float("CONTROLS", "STEER_LOCK").ok()).unwrap_or(0.0);
        // CarPhysicsInfo::tyreWidth
        let mut tyre_width = [0.0f32; 4];
        if let Ok(tyres) = rustyac_physics::data::ini::IniReader::load(&info.data_path.join("tyres.ini")) {
            let front = tyres.get_float("FRONT", "WIDTH").unwrap_or(0.0);
            let rear = tyres.get_float("REAR", "WIDTH").unwrap_or(0.0);
            tyre_width = [front, front, rear, rear];
        }
        // CarPhysicsInfo::maxGear
        let max_gear = rustyac_physics::data::ini::IniReader::load(&info.data_path.join("drivetrain.ini")).ok().and_then(|ini| ini.get_int("GEARS", "COUNT").ok()).unwrap_or(0);
        println!("{}", renderer.load_car(&folder, steer_lock, tyre_width, max_gear)?);
        Ok(true)
    };
    if load(&mut renderer, info, skin)? {
        for (number, other) in others.iter().enumerate() {
            // (the cars of the picture are numbered as the cars of the physics: one that cannot
            // be drawn would put every later one on the wrong car)
            if !load(&mut renderer, other, other.skin.clone())? {
                return Err(format!("no 3D model of {} (car {}) in Assetto Corsa's folder", other.name, number + 1));
            }
        }
        let crew = renderer.init_pit_crew();
        if !crew.is_empty() {
            println!("{crew}");
        }
    } else {
        println!("no 3D model of {} in Assetto Corsa's folder: no car is drawn", info.name);
    }
    renderer.finish_loading()?;
    // Task 29: AC's GUI and HUD (Sim::Sim makes them once the cars are there)
    if !options.no_gui && !options.no_hud {
        let ini = |car: &rustyac_game::sim::CarInfo, section: &str, key: &str| rustyac_physics::data::ini::IniReader::load(&car.data_path.join("car.ini")).ok().and_then(|ini| ini.get_float(section, key).ok()).unwrap_or(0.0);
        let cars: Vec<&rustyac_game::sim::CarInfo> = std::iter::once(info).chain(others).collect();
        let folders: Vec<String> = cars.iter().map(|car| car.name.clone()).collect();
        let setup = rustyac_game::render::gui::GuiSetup {
            cars: rustyac_game::render::gui::car_names(&game, race_reader.as_ref(), &folders),
            steer_locks: cars.iter().map(|car| ini(car, "CONTROLS", "STEER_LOCK")).collect(),
            fuel_light_min_liters: ini(info, "GRAPHICS", "FUEL_LIGHT_MIN_LITERS"),
            session_type: session_type.unwrap_or(if options.spawn == "hotlap" { 4 } else { 1 }),
            penalties: race_reader.as_ref().is_some_and(|ini| ini.get_int("RACE", "PENALTIES").unwrap_or(0) != 0),
            cfg_dir: options.cfg_dir.clone().or_else(|| std::env::current_exe().ok().and_then(|exe| exe.parent().map(|folder| folder.join("cfg")))).unwrap_or_else(|| PathBuf::from("cfg")),
            documents_cfg: rustyac_game::conditions::documents_folder().map(|folder| folder.join("cfg")),
            game: game.clone(),
        };
        if let Err(message) = renderer.enable_gui(setup) {
            eprintln!("WARNING: AC's GUI could not be made ({message}); rustyAC's own display is drawn instead");
        }
    }
    Ok(renderer)
}

/// The picture: AC's renderer, or the debug view when it was asked for or when AC's renderer
/// cannot start (no Assetto Corsa folder, no Direct3D 11 …).
fn make_picture(options: &Options, width: u32, height: u32, info: &rustyac_game::sim::CarInfo, others: &[rustyac_game::sim::CarInfo]) -> Result<Picture, String> {
    if !options.debug_view {
        match make_ac(options, width, height, info, others) {
            Ok(renderer) => return Ok(Picture::Ac(Box::new(renderer))),
            Err(message) => eprintln!("WARNING: Assetto Corsa's renderer could not start ({message}); the debug view is drawn instead"),
        }
    }
    let mut renderer = DebugRenderer::new(width, height)?;
    load_models(&mut renderer, options, info);
    Ok(Picture::Debug(Box::new(renderer)))
}

/// The recorder and the shared memory, as asked for.
/// The physics thread's hand-over to the sound, if this run has sound: always with a window
/// (unless `--no-audio`); without one (`--headless`) only when the sound is asked for by one
/// of its own options, so that a headless run stays silent.
fn audio_feed(options: &Options, headless: bool) -> Option<Arc<Mutex<Feed>>> {
    let audio = &options.audio;
    if audio.off || (headless && audio.wav.is_none() && audio.log.is_none() && !audio.null) {
        return None;
    }
    Some(Arc::new(Mutex::new(if audio.clockless() { Feed::with_grid() } else { Feed::default() })))
}

/// The sound of the car that was built (the engine, the track's sounds, the car's).
fn start_audio(options: &Options, feed: &Option<Arc<Mutex<Feed>>>, info: &rustyac_game::sim::CarInfo, others: &[rustyac_game::sim::CarInfo]) -> Option<GameAudio> {
    let feed = feed.as_ref()?;
    // a car's folder name is its name in the game
    let cars: Vec<(String, PathBuf, CarShape)> = std::iter::once(info)
        .chain(others)
        .map(|info| (Path::new(&info.name).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| info.name.clone()), info.data_path.clone(), CarShape::of(info)))
        .collect();
    let track = info.track_folder.as_deref().map(|folder| (folder, info.track_layout.as_str()));
    let mut audio = GameAudio::start(&options.audio, Arc::clone(feed), &cars, track)?;
    audio.set_focus(options.focus.min(others.len()));
    Some(audio)
}

/// Task 28c: the session's replay (AC's `ReplayManager`), shared by the physics thread, which
/// records, and the window, which plays. A window always has one; a run without a window
/// only when it is asked to save one. Replays go into rustyAC's own folder: `--replay-dir`,
/// else `replay` beside rustyac.exe (never one of Assetto Corsa's folders).
fn replay_rig(options: &Options, window: bool) -> Option<Arc<Mutex<ReplayRig>>> {
    if options.no_ac_replay || !(window || options.save_replay.is_some()) {
        return None;
    }
    let folder = options.replay_dir.clone().or_else(|| std::env::current_exe().ok().and_then(|exe| exe.parent().map(|folder| folder.join("replay")))).unwrap_or_else(|| PathBuf::from("replay"));
    // cfg/replay.ini: the player's (read only), else the game's own; none with --no-race-ini,
    // so that a check does not depend on the player's files (10 minutes at 33 frames a second)
    let mut ini = None;
    if options.race_ini != Some(false) {
        let mut files = Vec::new();
        if let Some(home) = std::env::var_os("USERPROFILE") {
            files.push(PathBuf::from(home).join("Documents/Assetto Corsa/cfg/replay.ini"));
        }
        if let Some(game) = rustyac_content::install::ac_root() {
            files.push(game.join("cfg/replay.ini"));
        }
        ini = files.iter().find(|file| file.is_file()).and_then(|file| rustyac_physics::data::ini::IniReader::load(file).ok());
    }
    Some(Arc::new(Mutex::new(ReplayRig::new(folder, ini, -16.0, 1))))
}

/// AC's ghost car, when race.ini's `[GHOST_CAR]` or `--ghost` asks for one
/// (`RaceManager::initGhostcar`: offline only). Its files live in rustyAC's own folder.
fn ghost_rig(options: &Options) -> Option<Arc<Mutex<GhostRig>>> {
    // (a recorded drive played again has a ghost only when the command line asks for one: what
    // it does must not depend on the player's files)
    let asked = options.ghost.is_some() || options.ghost_file.is_some();
    if options.join.is_some() || options.ac_replay.is_some() || (options.replay.is_some() && !asked) {
        return None;
    }
    let mut ghost = GhostOptions::default();
    let race = match (&options.race_ini_file, options.race_ini) {
        _ if options.replay.is_some() => None,
        (Some(file), _) => Some(file.clone()),
        (None, Some(false)) => None,
        (None, _) => rustyac_game::conditions::race_ini_path().filter(|p| p.is_file()),
    };
    if let Some(ini) = race.and_then(|p| rustyac_physics::data::ini::IniReader::load(&p).ok()) {
        let on = |key: &str| ini.get_int("GHOST_CAR", key).unwrap_or(0) != 0;
        ghost.recording = on("RECORDING");
        ghost.playing = on("PLAYING");
        ghost.load = on("LOAD");
        ghost.file = Some(ini.get_string("GHOST_CAR", "FILE")).filter(|file| !file.is_empty()).map(PathBuf::from);
        ghost.seconds_advantage = ini.get_float("GHOST_CAR", "SECONDS_ADVANTAGE").unwrap_or(0.0);
    }
    match options.ghost.as_deref() {
        Some("off") => return None,
        Some("record") => ghost.recording = true,
        Some("play") => (ghost.playing, ghost.load) = (true, true),
        Some("both") => (ghost.recording, ghost.playing, ghost.load) = (true, true, true),
        _ => {}
    }
    if let Some(file) = &options.ghost_file {
        ghost.file = Some(file.clone());
        ghost.load = true;
    }
    if let Some(seconds) = options.ghost_advantage {
        ghost.seconds_advantage = seconds;
    }
    if !ghost.wanted() {
        return None;
    }
    let folder = options.ghost_dir.clone().or_else(|| std::env::current_exe().ok().and_then(|exe| exe.parent().map(|folder| folder.join("GhostCar")))).unwrap_or_else(|| PathBuf::from("GhostCar"));
    Some(Arc::new(Mutex::new(GhostRig::new(folder, ghost))))
}

/// AC's comparative telemetry (`Sim::Sim` makes its recorder for the player's car, offline
/// or not; rustyAC: never on a server): in a window, and elsewhere when a folder is named.
fn telemetry_rig(options: &Options, window: bool) -> Option<Arc<Mutex<TelemetryRig>>> {
    let asked = options.telemetry_dir.is_some();
    if options.no_telemetry || options.join.is_some() || options.ac_replay.is_some() || ((options.replay.is_some() || !window) && !asked) {
        return None;
    }
    let folder = options.telemetry_dir.clone().or_else(|| std::env::current_exe().ok().and_then(|exe| exe.parent().map(|folder| folder.join("ctelemetry")))).unwrap_or_else(|| PathBuf::from("ctelemetry"));
    Some(Arc::new(Mutex::new(TelemetryRig::new(folder))))
}

/// The physics thread's end of the telemetry: a sample after every step, the file at the end.
struct TelemetrySink {
    rig: Arc<Mutex<TelemetryRig>>,
}

impl StepSink for TelemetrySink {
    fn after_step(&mut self, sim: &rustyac_game::sim::GameSim, _input: &StepInput) -> Result<(), String> {
        let mut rig = self.rig.lock().unwrap();
        if sim.online.is_none() {
            rig.after_step(sim);
        }
        for line in rig.messages.drain(..) {
            println!("{line}");
        }
        Ok(())
    }

    fn finish(&mut self) -> Result<(), String> {
        let mut rig = self.rig.lock().unwrap();
        rig.shutdown();
        for line in rig.messages.drain(..) {
            println!("{line}");
        }
        Ok(())
    }
}

/// The physics thread's end of the ghost car: the recorder's handlers after every step, and
/// the file when the run ends.
struct GhostSink {
    rig: Arc<Mutex<GhostRig>>,
}

impl StepSink for GhostSink {
    fn after_step(&mut self, sim: &rustyac_game::sim::GameSim, _input: &StepInput) -> Result<(), String> {
        let mut rig = self.rig.lock().unwrap();
        if sim.online.is_none() {
            rig.after_step(sim);
        }
        for line in rig.messages.drain(..) {
            println!("{line}");
        }
        Ok(())
    }

    fn finish(&mut self) -> Result<(), String> {
        let mut rig = self.rig.lock().unwrap();
        rig.shutdown();
        for line in rig.messages.drain(..) {
            println!("{line}");
        }
        Ok(())
    }
}

/// The physics thread's end of the replay: a frame of every car whenever the recording
/// interval has passed (`ReplayManager::onStepCompleted`), and the save at the program's end.
struct ReplaySink {
    rig: Arc<Mutex<ReplayRig>>,
    save: Option<PathBuf>,
}

impl StepSink for ReplaySink {
    fn after_step(&mut self, sim: &rustyac_game::sim::GameSim, _input: &StepInput) -> Result<(), String> {
        let mut rig = self.rig.lock().unwrap();
        if !rig.in_replay_mode() {
            rig.after_step(sim);
        }
        Ok(())
    }

    fn finish(&mut self) -> Result<(), String> {
        let mut rig = self.rig.lock().unwrap();
        match &self.save {
            Some(path) => match rig.save_to(path) {
                Ok(bytes) => println!("replay: {} ({bytes} bytes, {} frames)", path.display(), rig.manager.as_ref().map_or(0, |m| m.recorded_frames)),
                Err(message) => eprintln!("WARNING: the replay was not saved: {message}"),
            },
            None => rig.shutdown(),
        }
        for line in rig.messages.drain(..) {
            println!("{line}");
        }
        Ok(())
    }
}

/// A key of a replay that shows. The answer: the key was the replay's.
#[allow(clippy::too_many_arguments)]
fn ac_replay_key(rig: &Arc<Mutex<ReplayRig>>, shared: &Shared, bindings: &Bindings, key: i32, ctrl: bool, alt: bool, online: bool) -> bool {
    let mut rig = rig.lock().unwrap();
    let command = if ctrl {
        match key {
            _ if key == bindings.key_ffwd => Some(ReplayKey::FastForward),
            _ if key == bindings.key_slowmo => Some(ReplayKey::SlowMotion),
            _ if key == bindings.key_pause_replay => Some(ReplayKey::PlayPause),
            _ if key == bindings.key_rev => Some(ReplayKey::Rewind),
            _ if key == bindings.key_next_lap => Some(ReplayKey::NextLap),
            _ if key == bindings.key_previous_lap => Some(ReplayKey::PreviousLap),
            _ if key == bindings.key_start_replay => {
                // back to the driving (a loaded replay has none)
                leave_replay(&mut rig, shared, online);
                return true;
            }
            _ => None,
        }
    } else if alt {
        None
    } else {
        match key {
            0x50 | 0x13 => Some(ReplayKey::PlayPause),
            0x24 => Some(ReplayKey::Stop),
            0xbe => Some(ReplayKey::NextFrame),
            0x21 => Some(ReplayKey::NextLap),
            0x22 => Some(ReplayKey::PreviousLap),
            0x79 => {
                save_replay(&mut rig);
                return true;
            }
            _ => None,
        }
    };
    match command {
        Some(command) => {
            rig.key(command);
            true
        }
        None => false,
    }
}

/// F10: the replay into rustyAC's replay folder, under the name the game's autosave would
/// give it (with an `M` for "by hand" where the session's letter goes).
fn save_replay(rig: &mut ReplayRig) {
    let name = rig.file_name("M");
    match rig.save(&name, false) {
        Ok(path) => println!("Saving replay: {}", path.display()),
        Err(message) => eprintln!("WARNING: the replay was not saved: {message}"),
    }
}

/// `ReplayManager::stopReplayMode`: the driving goes on (offline the physics was standing).
fn leave_replay(rig: &mut ReplayRig, shared: &Shared, online: bool) {
    if rig.manager.as_ref().is_some_and(|manager| manager.has_loaded_replay) {
        return;
    }
    rig.stop_replay_mode();
    if !online {
        shared.paused.store(false, Ordering::Relaxed);
    }
    println!("REPLAY: back to the session");
}

/// `ReplayManager::startReplayMode`: the replay of the session so far, from its first frame.
/// Offline the physics stands still meanwhile (`Sim::onReplayModeChanged`).
fn enter_replay(rig: &mut ReplayRig, shared: &Shared, online: bool) {
    if rig.start_replay_mode() {
        if !online {
            shared.paused.store(true, Ordering::Relaxed);
        }
        println!("{}", rig.status_line());
    } else {
        println!("REPLAY: nothing is recorded yet");
    }
}

fn sinks(options: &Options, setup: &SimSetup, feed: &Option<Arc<Mutex<Feed>>>, rig: &Option<Arc<Mutex<ReplayRig>>>, ghost: &Option<Arc<Mutex<GhostRig>>>, telemetry: &Option<Arc<Mutex<TelemetryRig>>>) -> Result<Vec<Box<dyn StepSink>>, String> {
    let mut sinks: Vec<Box<dyn StepSink>> = Vec::new();
    if let Some(ghost) = ghost {
        sinks.push(Box::new(GhostSink { rig: Arc::clone(ghost) }));
    }
    if let Some(rig) = telemetry {
        sinks.push(Box::new(TelemetrySink { rig: Arc::clone(rig) }));
    }
    if let Some(rig) = rig {
        sinks.push(Box::new(ReplaySink { rig: Arc::clone(rig), save: options.save_replay.clone() }));
    }
    if let Some(feed) = feed {
        sinks.push(Box::new(AudioSink::new(Arc::clone(feed))));
    }
    if let Some(path) = &options.record {
        sinks.push(Box::new(Recorder(Some(InputWriter::create(path, setup)?))));
        println!("recording the inputs of every step to {}", path.display());
    }
    if !options.no_shm {
        match SharedMemory::create() {
            Ok(memory) => {
                println!(
                    "shared memory: publishing Local\\acpmf_physics, acpmf_graphics, acpmf_static{}",
                    if memory.reused() { " (the pages were still held open by a reader of an earlier session)" } else { "" }
                );
                sinks.push(Box::new(ShmSink::new(memory)));
            }
            // the car drives all the same
            Err(message) => eprintln!("WARNING: no shared memory: {message}"),
        }
    }
    Ok(sinks)
}

/// The car's set-up, and the recorded drive if one is to be played.
/// Task 28: where the referee's files go: `--out-dir`, else `out` beside rustyac.exe (never
/// one of Assetto Corsa's folders).
fn referee_out(options: &Options) -> Option<PathBuf> {
    if let Some(folder) = &options.out_dir {
        return Some(folder.clone());
    }
    std::env::current_exe().ok().and_then(|exe| exe.parent().map(|folder| folder.join("out")))
}

/// Task 28: the referee's buttons pressed at given seconds of the run.
fn session_script(options: &Options) -> Vec<(f64, u32)> {
    options.next_session_at.iter().map(|at| (*at, event::NEXT_SESSION)).chain(options.restart_session_at.iter().map(|at| (*at, event::RESTART_SESSION))).chain(options.pit_keys.iter().copied()).chain(options.ai_pit_at.iter().map(|at| (*at, event::AI_PIT))).collect()
}

fn drive(options: &Options) -> Result<(SimSetup, Option<Vec<StepInput>>), String> {
    match &options.replay {
        Some(path) => {
            let file = InputFile::read(path)?;
            println!("replaying {} ({} steps, {:.1} s)", path.display(), file.steps.len(), file.steps.len() as f64 * 0.003);
            Ok((file.setup, Some(file.steps)))
        }
        None => Ok((live_setup(options)?, None)),
    }
}

/// Task 25: joins the server the command line or race.ini names, if any. Returns the
/// session and the options of the drive on it: the server's car, skin and track, the pit
/// box of the player's slot.
fn join_server(options: &Options) -> Result<Option<(Options, rustyac_game::online::OnlineSession)>, String> {
    let Some(target) = rustyac_game::join::target(options)? else {
        return Ok(None);
    };
    if options.join.is_none() {
        println!("race.ini has [REMOTE] ACTIVE=1 and the command line names no car and no track: joining that server (--offline: drive alone)");
    }
    let joined = rustyac_game::join::join(options, &target)?;
    for note in &joined.notes {
        println!("{note}");
    }
    let accepted = &joined.session.net.client.accepted;
    let mut online = options.clone();
    online.car = accepted.model.clone();
    online.skin = Some(accepted.skin.clone());
    online.track = Some(accepted.track.clone());
    online.layout = Some(if accepted.track_config.is_empty() { "-".to_string() } else { accepted.track_config.clone() });
    // (--spawn start puts the car on the grid slot instead: for checks with the line
    // follower, which does not know the way out of the pits)
    if !options.spawn_given {
        online.spawn = "pit".to_string();
        online.spawn_given = true;
    }
    online.no_race_cars = true;
    online.cars = None;
    online.opponents.clear();
    if online.sun.is_none() {
        online.sun = Some(accepted.sun_angle);
    }
    Ok(Some((online, joined.session)))
}

/// The set-up of a drive on a server: [`live_setup`] of the server's car and track, in the
/// pit box of the player's slot.
fn online_setup(options: &Options, session: &rustyac_game::online::OnlineSession) -> Result<SimSetup, String> {
    let mut setup = live_setup(options)?;
    let accepted = &session.net.client.accepted;
    setup.opponents.clear();
    setup.player_skin = Some(accepted.skin.clone());
    // CarAvatar::setSpawnPositionIndex("PIT", sessionID) / ("START", handshakeCarPosition)
    setup.player_place = Some(rustyac_game::input_file::GridPlace { pit: accepted.session_id as usize, start: accepted.current_session.handshake_position.map_or(-1, |p| p as i32), ballast_kg: 0.0, restrictor: 0.0 });
    // the server's weather comes with its first weather message; its grip with the session
    setup.session.dynamic_track = None;
    setup.session.ballast_kg = 0.0;
    setup.session.restrictor = 0.0;
    Ok(setup)
}

/// What the physics thread hands back when it ends.
type Outcome = Result<(CarView, Timing), String>;

/// Starts the physics thread. The car is built on it (the car is not `Send`), with the
/// driver `make_source` creates there (device handles belong to the thread that polls them).
fn start_physics(
    setup: SimSetup,
    make_source: impl FnOnce() -> Box<dyn DriverSource> + Send + 'static,
    shared: &Arc<Shared>,
    sinks: Vec<Box<dyn StepSink>>,
    config: LoopConfig,
    online: Option<rustyac_game::online::OnlineSession>,
) -> Result<std::thread::JoinHandle<Outcome>, String> {
    let shared = Arc::clone(shared);
    std::thread::Builder::new()
        .name("physics".to_string())
        .spawn(move || {
            let built = match online {
                Some(session) => GameSim::new_online(setup, make_source(), session),
                None => GameSim::new(setup, make_source()),
            };
            let outcome = built.and_then(|sim| physics_thread::run(sim, &shared, sinks, config));
            shared.finished.store(true, Ordering::Relaxed);
            outcome.map(|(sim, timing)| (CarView::capture(&sim, 0.0), timing))
        })
        .map_err(|e| e.to_string())
}

/// The lap timer's numbers at the end of a run on a track.
fn lap_report(view: &CarView) {
    let lap = &view.lap;
    if lap.on_track {
        use rustyac_game::render::hud::lap_clock;
        println!(
            "laps: {} listed, last {}{}, best {}; the running lap {} ({}), {:.1} % round, sector {} of {}",
            lap.laps,
            lap_clock(lap.last_ms),
            if lap.last_ms != 0 && !lap.last_valid { " (cut)" } else { "" },
            lap_clock(lap.best_ms),
            lap_clock(lap.current_ms),
            if lap.valid { "valid".to_string() } else { format!("{} cuts", lap.cuts) },
            lap.position.clamp(0.0, 1.0) * 100.0,
            lap.sector + 1,
            lap.sector_count
        );
    }
}

fn report(view: &CarView, timing: &Timing) {
    lap_report(view);
    println!("{}", timing.report());
    println!(
        "the car after {} steps: {:.1} km/h, gear {}, {:.0} rpm, at x {:.2} y {:.3} z {:.2}",
        view.steps,
        view.speed_kmh,
        view.gear - 1,
        view.rpm,
        view.body[3][0],
        view.body[3][1],
        view.body[3][2]
    );
    println!(
        "its controls at the end: gas {:.3}, brake {:.3}, steer {:.4}, clutch {:.2} (driver: {})",
        view.gas,
        view.brake,
        view.steer,
        view.clutch,
        rustyac_game::input::device_name(view.device)
    );
}

/// `--headless` without a window: the car runs in real time, with a recorded drive or with
/// nobody at the controls, until `--duration` is over or the drive ends. With
/// `--bench-render` frames are drawn off screen meanwhile, as fast as the card goes.
fn run_headless(options: &Options) -> Result<(), String> {
    let joined = join_server(options)?;
    let (options, online) = match joined {
        Some((online_options, session)) => (online_options, Some(session)),
        None => (options.clone(), None),
    };
    let options = &options;
    let (setup, replay) = match &online {
        Some(session) => (online_setup(options, session)?, None),
        None => drive(options)?,
    };
    let feed = audio_feed(options, true);
    let sinks = sinks(options, &setup, &feed, &replay_rig(options, false), &ghost_rig(options), &telemetry_rig(options, false))?;
    let shared = Shared::new();
    let config = LoopConfig {
        replay: replay.clone(),
        duration: options.duration.map(Duration::from_secs_f64),
        max_steps: None,
        pause_unfocused: false,
        auto_reset: true,
        net_tape: options.net_tape.clone(),
        chat: options.chat.clone(),
        referee_out: referee_out(options),
        session_script: session_script(options),
    };
    if options.duration.is_none() && options.replay.is_none() {
        println!("no --duration: running until Ctrl+C");
    }
    let replaying = replay.is_some();
    let autodrive = options.autodrive;
    let source = move || -> Box<dyn DriverSource> {
        if replaying {
            Box::new(ReplaySource::default())
        } else if autodrive {
            Box::new(SpawnSequence::new(AutoDriver::new(), true))
        } else {
            Box::new(SpawnSequence::new(NobodySource, true))
        }
    };
    let thread = start_physics(setup, source, &shared, sinks, config, online)?;
    let mut frames = 0u64;
    let mut frame_max = Duration::ZERO;
    let mut headless_audio: Option<GameAudio> = None;
    let mut bench_stats = None;
    // --bench-render: the frames are counted from the first one (the loading of the picture,
    // which a preset can make long, is not part of the rate)
    let mut bench_from: Option<Instant> = None;
    let mut bench_seconds = 0.0f64;
    if options.bench_render {
        let mut renderer: Option<Picture> = None;
        let mut camera = None;
        let mut shape = None;
        let mut last = Instant::now();
        while !shared.finished.load(Ordering::Relaxed) && !thread.is_finished() && !STOP.load(Ordering::Relaxed) {
            if shape.is_none() {
                let info = shared.car_info.lock().unwrap().clone();
                if let Some(info) = info {
                    let others = shared.other_infos.lock().unwrap().clone();
                    // the camera's car (--focus)
                    let focused = if options.focus == 0 { info.clone() } else { others.get(options.focus - 1).cloned().unwrap_or_else(|| info.clone()) };
                    match make_picture(options, options.width, options.height, &info, &others) {
                        Ok(picture) => {
                            println!("drawing {} x {} off screen: {}", options.width, options.height, picture.describe());
                            renderer = Some(picture);
                        }
                        Err(message) => {
                            // the physics thread runs already: end it tidily first
                            shared.quit.store(true, Ordering::Relaxed);
                            let _ = thread.join();
                            return Err(message);
                        }
                    }
                    let car_shape = CarShape::of(&focused);
                    camera = Some(DrivingCamera::from_name(&options.camera, &car_shape).unwrap_or_else(|_| DrivingCamera::chase()));
                    shape = Some(car_shape);
                    // the loading is not part of the timing
                    last = Instant::now();
                    bench_from = Some(last);
                }
            }
            let (Some(shape), Some(renderer), Some(camera)) = (&shape, &mut renderer, &mut camera) else {
                std::thread::sleep(Duration::from_millis(2));
                continue;
            };
            let now = Instant::now();
            let world = shared.world_at(now);
            let focus = options.focus.min(world.len() - 1);
            let view = world[focus];
            let dt = (now - last).as_secs_f32();
            let frame = camera.update(&view, shape, view.acc_g, dt);
            let info = HudInfo { fps: 0.0, timing: shared.timing(), camera: camera.name(), replay: replaying, notes: shared.online_hud.lock().unwrap().clone(), referee: shared.referee_hud.lock().unwrap().clone(), ..HudInfo::default() };
            renderer.draw_world(&world, focus, shape, camera, &frame, &info, dt);
            renderer.finish();
            bench_stats = renderer.frame_stats();
            frames += 1;
            bench_seconds = bench_from.map_or(0.0, |from| from.elapsed().as_secs_f64());
            if frames > 10 {
                frame_max = frame_max.max(now - last);
            }
            last = now;
        }
        // --bench-shot: the picture as it is at the end (an online session cannot be redone
        // for a --screenshot: this is its picture)
        if let (Some(path), Some(shape), Some(renderer), Some(camera)) = (&options.bench_shot, &shape, &mut renderer, &mut camera) {
            let world = shared.world_at(Instant::now());
            let focus = options.focus.min(world.len() - 1);
            let view = world[focus];
            let frame = camera.update(&view, shape, view.acc_g, 1.0 / 60.0);
            let info = HudInfo { fps: 0.0, timing: shared.timing(), camera: camera.name(), replay: replaying, notes: shared.online_hud.lock().unwrap().clone(), referee: shared.referee_hud.lock().unwrap().clone(), ..HudInfo::default() };
            renderer.draw_world(&world, focus, shape, camera, &frame, &info, 1.0 / 60.0);
            let pixels = renderer.read_pixels()?;
            let (width, height) = renderer.size();
            write_png(path, width, height, &pixels)?;
            println!("{}: {width} x {height}, the last frame drawn off screen", path.display());
        }
    } else if feed.is_some() {
        // no picture, but sound: its frames at about 60 a second, the listener at the camera
        let mut audio: Option<(GameAudio, CarShape, DrivingCamera)> = None;
        let mut failed = false;
        let mut last = Instant::now();
        while !shared.finished.load(Ordering::Relaxed) && !thread.is_finished() && !STOP.load(Ordering::Relaxed) {
            if audio.is_none() && !failed {
                let info = shared.car_info.lock().unwrap().clone();
                if let Some(info) = info {
                    let others = shared.other_infos.lock().unwrap().clone();
                    let focused = if options.focus == 0 { info.clone() } else { others.get(options.focus - 1).cloned().unwrap_or_else(|| info.clone()) };
                    let shape = CarShape::of(&focused);
                    let camera = DrivingCamera::from_name(&options.camera, &shape).unwrap_or_else(|_| DrivingCamera::chase());
                    match start_audio(options, &feed, &info, &others) {
                        Some(sound) => audio = Some((sound, shape, camera)),
                        None => failed = true,
                    }
                    last = Instant::now();
                }
            }
            if let Some((sound, shape, camera)) = audio.as_mut() {
                let now = Instant::now();
                let world = shared.world_at(now);
                let view = world[options.focus.min(world.len() - 1)];
                let frame = camera.update(&view, shape, view.acc_g, (now - last).as_secs_f32());
                if options.audio.clockless() {
                    sound.grid_frames(|_| (*camera, frame.matrix));
                } else {
                    sound.frame(camera, &frame.matrix, (now - last).as_secs_f64(), false);
                }
                last = now;
                std::thread::sleep(Duration::from_millis(if options.audio.clockless() { 4 } else { 15 }));
            } else {
                std::thread::sleep(Duration::from_millis(5));
            }
        }
        headless_audio = audio.map(|(sound, _, _)| sound);
    } else {
        // a replay ends by itself; so does a run with a duration
        while !shared.finished.load(Ordering::Relaxed) && !thread.is_finished() && !STOP.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_millis(20));
        }
    }
    shared.quit.store(true, Ordering::Relaxed);
    let (view, timing) = thread.join().map_err(|_| "the physics thread panicked".to_string())??;
    report(&view, &timing);
    if let Some(sound) = headless_audio {
        println!("{}", sound.finish().report());
    }
    if options.bench_render {
        println!(
            "render (off screen, not waiting for a display): {frames} frames in {bench_seconds:.1} s = {:.0} FPS (average frame {:.2} ms), slowest frame {:.1} ms",
            frames as f64 / bench_seconds.max(1e-9),
            bench_seconds * 1000.0 / (frames.max(1)) as f64,
            frame_max.as_secs_f64() * 1000.0
        );
        if let Some(stats) = bench_stats {
            println!("  {stats}");
        }
    }
    Ok(())
}

/// `--list-tracks`: every installed track and layout, and why some cannot be driven.
fn list_tracks() -> String {
    use rustyac_physics::track::catalog;
    let Some(root) = rustyac_content::install::ac_root() else {
        return format!("{}\n", rustyac_content::install::not_found_hint());
    };
    let mut out = String::new();
    for entry in catalog::installed(&root) {
        let layout = if entry.layout.is_empty() { String::new() } else { format!(" --layout {}", entry.layout) };
        let name = if entry.ui_name.is_empty() { String::new() } else { format!("  ({})", entry.ui_name) };
        match catalog::check(&entry) {
            Ok(()) => out.push_str(&format!("--track {}{layout}{name}\n", entry.track)),
            Err(refusal) => out.push_str(&format!("   refused: {}{}{name}: {refusal}\n", entry.track, if entry.layout.is_empty() { String::new() } else { format!(" / {}", entry.layout) })),
        }
    }
    out
}

/// The devices found, for the console.
fn list_devices(bindings: &Bindings, wheel: &Option<WheelDevice>) -> String {
    let mut out = String::from("devices:\n  keyboard (always there)\n");
    let pads = XInput::new().scan();
    if pads.is_empty() {
        out.push_str("  no Xbox pad on XInput (it is looked for again every second; plug it in any time)\n");
    }
    for index in &pads {
        out.push_str(&format!("  Xbox pad on XInput slot {index}{}\n", if Some(index) == pads.first() { " (in use)" } else { "" }));
    }
    out.push_str(&rustyac_game::input::dinput::describe_devices(wheel));
    out.push_str(&format!("  input method of the bindings: {}; whichever device is touched last drives\n", bindings.input_method));
    out
}

/// The normal way to run: a window, the live devices (or a recorded drive to watch).
fn run_window(options: &Options) -> Result<(), String> {
    let joined = join_server(options)?;
    let (options, online) = match joined {
        Some((online_options, session)) => (online_options, Some(session)),
        None => (options.clone(), None),
    };
    let options = &options;
    let (mut setup, replay) = match &online {
        Some(session) => (online_setup(options, session)?, None),
        None => drive(options)?,
    };
    let mut notes = Vec::new();
    let bindings = Bindings::load(options, &mut notes);
    for note in &notes {
        println!("bindings: {note}");
    }
    if replay.is_none() && bindings.input_method == "WHEEL" {
        // what the physics reads off AC's wheel class: the force's filter, the understeer
        // effect and the device's gain (the pad's and the keyboard's classes have 0, off, 1)
        let wheel = rustyac_game::input::wheel::DiCarControl::from_ini(&bindings.ini);
        setup.env.ff_filter = wheel.ff_filter;
        setup.env.use_fake_understeer_ff = wheel.use_fake_understeer_ff;
        setup.ff_gain = wheel.ff_gain;
    }
    let window = Window::create("rustyAC", options.width, options.height, options.windowed, options.no_focus)?;
    let window_handle = window.handle.0 as isize;
    // a pad does not count as "somebody is at the PC": keep the display on while the window is open
    // SAFETY: a plain request to the power manager, taken back at the end of this function.
    unsafe { SetThreadExecutionState(ES_CONTINUOUS | ES_DISPLAY_REQUIRED) };
    // a Ctrl key that is itself a driving key (AC's files put gear-down on Left Ctrl) does not
    // make a Ctrl+letter command: the other Ctrl key does, and Alt (`[RUSTYAC] COMMAND_MODIFIER`)
    let keyboard = KeyboardCarControl::from_ini(&bindings.ini);
    let ctrl_drives = [keyboard.drives_with(0xa2) || keyboard.drives_with(0x11), keyboard.drives_with(0xa3) || keyboard.drives_with(0x11)];
    // rustyAC's plain keys for the aids and the brake bias, where the bindings leave them free
    let plain_keys: Vec<i32> = [KEY_PLAIN_TC, KEY_PLAIN_ABS, KEY_PLAIN_BIAS_UP, KEY_PLAIN_BIAS_DN].into_iter().filter(|key| bindings.plain_key_free(*key)).collect();
    if replay.is_none() {
        print!("{}", bindings.describe());
        match bindings.write_if_missing() {
            Ok(Some(path)) => println!("bindings written to {} (edit it to change them)", path.display()),
            Ok(None) => {}
            Err(message) => eprintln!("WARNING: {message}"),
        }
    }
    let feed = audio_feed(options, false);
    let rig = replay_rig(options, true);
    let is_online = online.is_some();
    let ghost = ghost_rig(options).filter(|_| !is_online);
    let sinks = sinks(options, &setup, &feed, &rig, &ghost, &telemetry_rig(options, true).filter(|_| !is_online))?;
    let shared = Shared::new();
    // a replay waits for the picture: its first step runs when the first frame is drawn (the
    // frame loop sets the transport's speed)
    if replay.is_some() {
        shared.replay_speed.store(0.0f32.to_bits(), Ordering::Relaxed);
    }
    // the window says when it has the keyboard (and Windows may not have given it)
    shared.focused.store(false, Ordering::Relaxed);
    let config = LoopConfig { replay: replay.clone(), duration: None, max_steps: None, pause_unfocused: !options.no_focus, auto_reset: true, net_tape: options.net_tape.clone(), chat: options.chat.clone(), referee_out: referee_out(options), session_script: session_script(options) };
    let source = {
        let (bindings, shared, rumble, ffb, live) = (bindings.clone(), Arc::clone(&shared), !options.no_rumble, options.ffb, replay.is_none());
        let autodrive = options.autodrive;
        move || -> Box<dyn DriverSource> {
            if !live {
                return Box::new(ReplaySource::default());
            }
            if autodrive {
                return Box::new(SpawnSequence::new(AutoDriver::new(), true));
            }
            let wheel = WheelDevice::open(&bindings, window_handle, ffb);
            print!("{}", list_devices(&bindings, &wheel));
            Box::new(SpawnSequence::new(LiveSource::new(bindings, shared, rumble, wheel), true))
        }
    };
    let thread = start_physics(setup, source, &shared, sinks, config, online)?;

    // the car is built on the physics thread: wait for it (or for its refusal)
    let car_info = loop {
        if let Some(info) = shared.car_info.lock().unwrap().as_ref() {
            break info.clone();
        }
        if thread.is_finished() {
            drop(window);
            return thread.join().map_err(|_| "the physics thread panicked".to_string())?.map(|_| ());
        }
        std::thread::sleep(Duration::from_millis(2));
    };
    let other_infos = shared.other_infos.lock().unwrap().clone();
    // every car's shape (its eye point and its cameras), the player's first
    let shapes: Vec<CarShape> = std::iter::once(&car_info).chain(&other_infos).map(CarShape::of).collect();
    let car_names: Vec<String> = std::iter::once(&car_info).chain(&other_infos).map(|info| Path::new(&info.name).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| info.name.clone())).collect();
    let mut focus = Focus::new(options.focus.min(other_infos.len()));
    let mut focus_script = options.focus_script.clone();
    focus_script.sort_by(|a, b| a.0.total_cmp(&b.0));
    let shape = shapes[focus.index].clone();
    let (width, height) = window.client_size();
    let graphics = make_picture(options, width, height, &car_info, &other_infos).and_then(|mut renderer| {
        if let Some(ghost) = &ghost {
            renderer.set_ghost(Arc::clone(ghost));
        }
        let chain = renderer.swap_chain(window.handle)?;
        Ok((renderer, chain))
    });
    let (mut renderer, chain) = match graphics {
        Ok(graphics) => graphics,
        Err(message) => {
            // the physics thread runs already: end it tidily first (devices, recording, shared memory)
            shared.quit.store(true, Ordering::Relaxed);
            let _ = thread.join();
            return Err(message);
        }
    };
    println!("window {width} x {height}: {}", renderer.describe());
    if options.virtual_mirror {
        renderer.set_virtual_mirror(true);
    }
    let mut audio = start_audio(options, &feed, &car_info, &other_infos);
    let mut camera = match DrivingCamera::from_name(&options.camera, &shape) {
        Ok(camera) => camera,
        Err(message) => {
            // (as above: the physics thread runs already)
            shared.quit.store(true, Ordering::Relaxed);
            let _ = thread.join();
            return Err(message);
        }
    };
    // Task 28c: one of AC's replays as the session: the file's frames into the manager, and the
    // replay shows from its first frame (the physics stands still for good)
    if let (Some(file), Some(rig)) = (&options.ac_replay, &rig) {
        let loaded = std::fs::read(file).map_err(|e| format!("{}: {e}", file.display())).and_then(|bytes| {
            let views = shared.world_at(Instant::now());
            let cars: Vec<(PathBuf, usize, f64)> = std::iter::once(&car_info).chain(&other_infos).zip(&views).map(|(info, view)| (info.data_path.clone(), view.wing_count as usize, view.max_fuel)).collect();
            let mut rig = rig.lock().unwrap();
            let header = rig.load(&bytes, &cars)?;
            if !rig.start_replay_mode() {
                return Err(format!("{}: the replay has no frames to play", file.display()));
            }
            Ok(header)
        });
        match loaded {
            Ok(header) => {
                shared.paused.store(true, Ordering::Relaxed);
                println!("replay {}: version {}, {} frames every {} ms ({:.1} s), {} cars", file.display(), header.version, header.frames, header.record_interval_ms, header.frames as f64 * header.record_interval_ms * 0.001, header.cars.len());
            }
            Err(message) => {
                shared.quit.store(true, Ordering::Relaxed);
                let _ = thread.join();
                return Err(message);
            }
        }
    }
    let mut ac_replay_audio = true;
    let mut window_shot = false;
    let mut camera_toggles = 0;
    // the replay's transport (ReplayManager): a recorded drive starts playing
    let mut transport = replay.is_some().then(rustyac_game::replay_manager::ReplayManager::new);
    let mut script = options.replay_script.clone();
    let mut new_session_at = options.new_session_at;
    script.sort_by(|a, b| a.0.total_cmp(&b.0));
    let started = Instant::now();
    let mut last = Instant::now();
    let (mut frames, mut fps, mut fps_frames, mut fps_since) = (0u64, 0.0f32, 0u32, Instant::now());
    // Ctrl+F / --show-fps: the frame-rate counter; the frame times of the running half second
    let (mut fps_counter, mut frame_ms, mut frame_max_ms, mut running_max) = (options.show_fps, 0.0f32, 0.0f32, Duration::ZERO);
    let mut frame_max = Duration::ZERO;
    let mut result = Ok(());
    let mut world = shared.world_at(Instant::now());
    // rustyAC's own panels and numbers: --debug-hud, F10, or when AC's HUD is not there
    let mut debug_hud = options.debug_hud || renderer.gui().is_none();
    if let (Some(commands), Some(gui)) = (&options.gui_do, renderer.gui()) {
        gui.script(commands)?;
    }
    'frames: loop {
        for event in window.pump() {
            match event {
                Event::Close | Event::Key { key: 0x1b, .. } => break 'frames,
                // ksgui::GUI's listeners of the mouse and the keyboard
                Event::Mouse { x, y, action } => {
                    if let Some(gui) = renderer.gui() {
                        match action {
                            MouseAction::Move => gui.mouse_move(x, y),
                            MouseAction::Down(button) => gui.mouse_down(x, y, button),
                            MouseAction::Up(button) => gui.mouse_up(x, y, button),
                            MouseAction::Wheel(delta) => gui.mouse_wheel(x, y, delta as f32 / 120.0),
                        }
                    }
                }
                Event::Char(character) => {
                    if let Some(gui) = renderer.gui() {
                        gui.key_char(character);
                    }
                }
                Event::Focus(focused) => shared.focused.store(focused, Ordering::Relaxed),
                Event::Resize(width, height) => {
                    if let Err(message) = renderer.resize_swap_chain(&chain, width, height) {
                        result = Err(message);
                        break 'frames;
                    }
                }
                Event::Key { key, left_ctrl, right_ctrl, alt, shift } => {
                    let ctrl = bindings.command_modifier.held(left_ctrl, right_ctrl, alt, ctrl_drives);
                    let request = |bits: u32| {
                        shared.requests.fetch_or(bits, Ordering::Relaxed);
                    };
                    let key = key as i32;
                    if let Some(gui) = renderer.gui() {
                        gui.key_down(key as u32);
                    }
                    // Sim::onKeyDown 0x14019a940, the replay's commands: with Ctrl, whatever
                    // Shift does; they come before rustyAC's own Ctrl+F
                    let replay_keys = [bindings.key_ffwd, bindings.key_slowmo, bindings.key_pause_replay, bindings.key_rev, bindings.key_next_lap, bindings.key_previous_lap, bindings.key_start_replay];
                    let in_ac_replay = rig.as_ref().is_some_and(|rig| rig.lock().unwrap().in_replay_mode());
                    if let (true, Some(rig)) = (in_ac_replay && !(ctrl && [bindings.key_previous_car, bindings.key_player_car, bindings.key_next_car].contains(&key) && !replay_keys.contains(&key)), &rig) {
                        // Task 28c: a replay of recorded frames shows: its keys
                        if !ac_replay_key(rig, &shared, &bindings, key, ctrl, alt, is_online) && !ctrl && !alt {
                            match key {
                                0x70 | 0x43 => camera.f1(),
                                0x73 => focus.command(FocusCommand::Player, &world, &car_names),
                                0x75 => camera.f6(shapes[focus.index].car_cameras.len()),
                                _ => {}
                            }
                        }
                    } else if let (true, Some(rig), true) = (transport.is_none() && replay.is_none(), &rig, ctrl && key == bindings.key_start_replay) {
                        // Sim::onKeyDown, the command "Start Replay"
                        enter_replay(&mut rig.lock().unwrap(), &shared, is_online);
                    } else if let (Some(rm), true) = (transport.as_mut(), ctrl && replay_keys.contains(&key)) {
                        if key == bindings.key_ffwd {
                            rm.fast_forward();
                        } else if key == bindings.key_slowmo {
                            rm.key_slow_motion();
                        } else if key == bindings.key_pause_replay {
                            rm.key_pause();
                        } else {
                            println!("REPLAY: rewind, the lap jumps and the instant replay need recorded frames to go back over; a rustyAC replay is the drive run again (Home: back to the start)");
                        }
                    } else if let (Some(rm), true) = (transport.as_mut(), !ctrl && !alt && key == 0x24) {
                        // Home (rustyAC's own key; the game's Stop is a button of its replay bar):
                        // ReplayManager::stop, back to the first frame
                        rm.stop();
                        shared.replay_restart.store(true, Ordering::Relaxed);
                    } else if ctrl && (in_ac_replay || !shared.paused.load(Ordering::Relaxed)) && world.len() > 1 && [bindings.key_previous_car, bindings.key_player_car, bindings.key_next_car].contains(&key) {
                        // Sim::onKeyDown 0x14019a940: with Ctrl, not while the pause menu shows,
                        // only with more than one car; tested in the game's order
                        let command = if key == bindings.key_previous_car {
                            FocusCommand::Previous
                        } else if key == bindings.key_player_car {
                            FocusCommand::Player
                        } else {
                            FocusCommand::Next
                        };
                        focus.command(command, &world, &car_names);
                    } else if let (true, Some(gui)) = (ctrl && !shared.paused.load(Ordering::Relaxed) && [KEY_HIDE_APPS, KEY_CYCLE_DESKTOP, KEY_DRIVER_NAMES, KEY_SHOW_DAMAGE, KEY_HIDE_DAMAGE].contains(&key), renderer.gui()) {
                        // Sim::onKeyDown 0x14019a940, the commands of the GUI and the HUD (the
                        // game's default keys; with Ctrl, not while the pause menu shows)
                        match key {
                            KEY_HIDE_APPS => gui.hide_show_apps(),
                            KEY_CYCLE_DESKTOP => println!("GUI: desktop {}", gui.cycle_desktop() + 1),
                            KEY_DRIVER_NAMES => println!("DRIVER NAMES: {}", if gui.toggle_driver_names() { "ON" } else { "OFF" }),
                            KEY_SHOW_DAMAGE => gui.show_damage(),
                            _ => println!("DAMAGE DISPLAYER: {}", if gui.toggle_damage() { "Enabled" } else { "Disabled" }),
                        }
                    } else if !ctrl && !alt && key == KEY_DEBUG_HUD {
                        // F10 (rustyAC's own key): its own panels and numbers
                        debug_hud = !debug_hud;
                    } else if (ctrl && key == 0x46) || (!ctrl && !alt && key == 0x78) {
                        // Ctrl+F or F9: the frame-rate counter (rustyAC's own keys; AC has an
                        // app for it). F alone is the keyboard's second DRS key, hence F9 too
                        fps_counter = !fps_counter;
                    } else if ctrl && key == bindings.key_traction_control {
                        request(if shift { event::TC_DN } else { event::TC_UP });
                    } else if ctrl && key == bindings.key_abs {
                        request(if shift { event::ABS_DN } else { event::ABS_UP });
                    } else if ctrl && key == bindings.key_auto_shifter {
                        request(event::AUTO_SHIFTER);
                    } else if ctrl && key == bindings.key_activate_ai {
                        // Sim::onKeyDown 0x14019a940, the command "Activate AI": an AI driver
                        // takes the car
                        request(event::AI_ON);
                    } else if ctrl && key == bindings.key_ideal_line {
                        // Sim::onKeyDown 0x14019a940: no text on the screen in the game; a line here
                        match renderer.toggle_ideal_line() {
                            Some(on) => {
                                rustyac_game::shm::IDEAL_LINE_ON.store(on, Ordering::Relaxed);
                                println!("IDEAL LINE: {}", if on { "ON" } else { "OFF" });
                            }
                            None => println!("IDEAL LINE: this track has none (or the debug view is drawing)"),
                        }
                    } else if ctrl && key == rustyac_game::input::bindings::KEY_MGUK_RECOVERY {
                        // AC's fixed commands of the hybrid cockpit: the digits 1 to 4
                        request(if shift { event::MGUK_RECOVERY_DN } else { event::MGUK_RECOVERY_UP });
                    } else if ctrl && key == rustyac_game::input::bindings::KEY_MGUK_DELIVERY {
                        request(if shift { event::MGUK_DELIVERY_DN } else { event::MGUK_DELIVERY_UP });
                    } else if ctrl && key == rustyac_game::input::bindings::KEY_MGUH_MODE {
                        request(event::MGUH_MODE);
                    } else if ctrl && key == rustyac_game::input::bindings::KEY_ENGINE_BRAKE {
                        request(if shift { event::ENGINE_BRAKE_DN } else { event::ENGINE_BRAKE_UP });
                    } else if !ctrl && !alt && plain_keys.contains(&key) {
                        // rustyAC's plain keys, as in the browser: T / Y (Shift: down), ] / [.
                        // Not while paused: the presses would all arrive at once afterwards.
                        match key {
                            _ if shared.paused.load(Ordering::Relaxed) => {}
                            KEY_PLAIN_TC => request(if shift { event::TC_DN } else { event::TC_UP }),
                            KEY_PLAIN_ABS => request(if shift { event::ABS_DN } else { event::ABS_UP }),
                            KEY_PLAIN_BIAS_UP => {
                                shared.bias_requests.fetch_add(1, Ordering::Relaxed);
                            }
                            _ => {
                                shared.bias_requests.fetch_sub(1, Ordering::Relaxed);
                            }
                        }
                    } else if !ctrl {
                        match key {
                            // C: camera, P or Pause: pause, R: back to the spawn point,
                            // Shift+R: back onto the track where the car is, N: a new car
                            // F1 (and C, as before): AC's driving views in turn; F6: the
                            // cameras of the car's cameras.ini
                            0x70 | 0x43 => camera.f1(),
                            // F4: back to your car (with other cars only; through the fade)
                            0x73 => focus.command(FocusCommand::Player, &world, &car_names),
                            0x75 => camera.f6(shapes[focus.index].car_cameras.len()),
                            // F11: AC's virtual mirror
                            0x7a => match renderer.toggle_virtual_mirror() {
                                Some(on) => println!("VIRTUAL MIRROR: {}", if on { "ON" } else { "OFF" }),
                                None => println!("VIRTUAL MIRROR: video.ini has no mirrors ([MIRROR] SIZE=0)"),
                            },
                            0x50 | 0x13 => match transport.as_mut() {
                                // a replay: the transport's own pause
                                Some(rm) => rm.key_pause(),
                                None => {
                                    shared.paused.fetch_xor(true, Ordering::Relaxed);
                                }
                            },
                            0x52 => request(if shift { event::TO_TRACK } else { event::RESET }),
                            0x4e => request(event::REBUILD),
                            // Task 28: the referee's two buttons (F7 next session, F8 restart)
                            0x76 => request(event::NEXT_SESSION),
                            0x77 => request(event::RESTART_SESSION),
                            // Task 28b: the pit menu (the number pad's 4 8 6 2, or Alt with the arrow keys)
                            0x64 => request(event::PIT_LEFT),
                            0x68 => request(event::PIT_UP),
                            0x66 => request(event::PIT_RIGHT),
                            0x62 => request(event::PIT_DOWN),
                            0x25 if alt => request(event::PIT_LEFT),
                            0x26 if alt => request(event::PIT_UP),
                            0x27 if alt => request(event::PIT_RIGHT),
                            0x28 if alt => request(event::PIT_DOWN),
                            _ => {}
                        }
                    }
                }
            }
        }
        let toggles = shared.camera_toggles.load(Ordering::Relaxed);
        if toggles != camera_toggles {
            camera_toggles = toggles;
            // the pad's camera button is F1 in the game too
            camera.f1();
        }
        if thread.is_finished() || STOP.load(Ordering::Relaxed) || options.duration.is_some_and(|d| started.elapsed().as_secs_f64() >= d) {
            break;
        }
        let now = Instant::now();
        let dt = now - last;
        last = now;
        world = shared.world_at(now);
        // Task 28c: --replay-script for a replay of recorded frames; then the replay's frame
        // (ReplayManager::updatePlayState): every car's state from its recorder
        let (mut ac_replay_line, mut ac_replay_events, mut ac_replay_scale) = (None, Vec::new(), 1.0f32);
        if let Some(rig) = &rig {
            let mut rig = rig.lock().unwrap();
            rig.focused_car = focus.index;
            while let (true, Some(at)) = (transport.is_none(), script.first().map(|(at, _)| *at).filter(|at| started.elapsed().as_secs_f64() >= *at)) {
                let (_, name) = script.remove(0);
                match name.as_str() {
                    "replay" => enter_replay(&mut rig, &shared, is_online),
                    "exit" => leave_replay(&mut rig, &shared, is_online),
                    "save" => save_replay(&mut rig),
                    "pause" => rig.key(ReplayKey::PlayPause),
                    "slow" => rig.key(ReplayKey::SlowMotion),
                    "ffwd" => rig.key(ReplayKey::FastForward),
                    "rewind" => rig.key(ReplayKey::Rewind),
                    "frame" => rig.key(ReplayKey::NextFrame),
                    "nextlap" => rig.key(ReplayKey::NextLap),
                    "prevlap" => rig.key(ReplayKey::PreviousLap),
                    _ => rig.key(ReplayKey::Stop),
                }
                println!("replay script at {at:.2} s: {name} -> {}", rig.status_line());
            }
            if rig.in_replay_mode() {
                let track = shared.track.lock().unwrap().clone();
                for event in rig.frame(dt.as_secs_f32(), track.as_deref()) {
                    match event {
                        ReplayEvent::StatusChanged { status, time_mult, slow_motion_level } => ac_replay_events.push((status, time_mult, slow_motion_level)),
                        ReplayEvent::Audio(on) => ac_replay_audio = on,
                        _ => {}
                    }
                }
                for (i, view) in world.iter_mut().enumerate() {
                    let Some(state) = rig.states.get(i) else { continue };
                    let mut shown = rustyac_game::ac_replay::view_of(view, state);
                    let wings = rig.wings(i);
                    shown.wing_count = wings.len().min(8) as u8;
                    shown.wing_angles[..shown.wing_count as usize].copy_from_slice(&wings[..shown.wing_count as usize]);
                    shown.disconnected = !rig.car_shows(i);
                    *view = shown;
                }
                // the loose objects as the scene's frames have them (a file that counts them
                // differently than this track does was written with objects a patch added, or
                // is another track's: then they stay where they are)
                let same_objects = track.as_ref().is_some_and(|track| track.objects.len() == rig.objects.len());
                if let (true, Some(first)) = (same_objects, world.first_mut()) {
                    let count = rig.objects.len().min(rustyac_game::view::MOVED_OBJECTS);
                    for (number, matrix) in rig.objects.iter().take(count).enumerate() {
                        first.moved_objects[number] = (number as u16, matrix.m);
                    }
                    first.moved_object_count = count as u8;
                }
                renderer.set_replay_states(Some(rig.states.clone()));
                if let Some(sound) = audio.as_mut() {
                    sound.set_replay_states(&rig.states);
                }
                ac_replay_scale = rig.manager.as_ref().map_or(1.0, |manager| manager.time_mult);
                ac_replay_line = Some(rig.status_line());
            } else {
                renderer.set_replay_states(None);
            }
            for line in rig.messages.drain(..) {
                println!("{line}");
            }
        }
        // --focus-script: the focus keys by the clock
        while let Some(at) = focus_script.first().map(|(at, _)| *at).filter(|at| started.elapsed().as_secs_f64() >= *at) {
            let (_, name) = focus_script.remove(0);
            let command = match name.as_str() {
                "next" => FocusCommand::Next,
                "previous" => FocusCommand::Previous,
                _ => FocusCommand::Player,
            };
            println!("focus script at {at:.2} s: {name}");
            focus.command(command, &world, &car_names);
        }
        // the fade of a change of the focused car, as this frame shows it
        let focus_fade = focus.for_picture();
        if focus.index >= world.len() {
            focus.index = 0;
        }
        let view = world[focus.index];
        let frame = camera.update(&view, &shapes[focus.index], view.acc_g, dt.as_secs_f32());
        let replay_over = replay.is_some() && shared.finished.load(Ordering::Relaxed);
        let unfocused = !options.no_focus && !shared.focused.load(Ordering::Relaxed);
        // --new-session-at: N by the clock
        if new_session_at.is_some_and(|at| started.elapsed().as_secs_f64() >= at) {
            new_session_at = None;
            shared.requests.fetch_or(event::REBUILD, Ordering::Relaxed);
            println!("new session (N) at step {}", shared.steps.load(Ordering::Relaxed));
        }
        // --replay-script: the keys by the clock
        if let Some(rm) = transport.as_mut() {
            while let Some(at) = script.first().map(|(at, _)| *at).filter(|at| started.elapsed().as_secs_f64() >= *at) {
                let (_, name) = script.remove(0);
                match name.as_str() {
                    "pause" => rm.key_pause(),
                    "slow" => rm.key_slow_motion(),
                    "ffwd" => rm.fast_forward(),
                    _ => {
                        rm.stop();
                        shared.replay_restart.store(true, Ordering::Relaxed);
                    }
                }
                println!("replay script at {at:.2} s: {name} -> status {}, speed {} (step {})", rm.status, rm.time_mult, shared.steps.load(Ordering::Relaxed));
            }
        }
        // ReplayManager::updatePlayState, before the cars update: the speed of this frame
        let (mut replay_events, mut replay_scale, mut replay_state, mut replay_sound) = (Vec::new(), 1.0f32, String::new(), true);
        if let Some(rm) = transport.as_mut() {
            use rustyac_game::replay_manager::{FAST_FORWARD, PAUSE, SLOW_MOTION, STOP};
            // the replay pauses itself at its end
            if replay_over && rm.status != PAUSE && rm.status != STOP {
                rm.pause();
            }
            replay_scale = rm.update_play_state(dt.as_secs_f32());
            shared.replay_speed.store(replay_scale.max(0.0).to_bits(), Ordering::Relaxed);
            replay_events = rm.events.drain(..).map(|e| (e.status, e.time_mult, e.slow_mo_level)).collect();
            replay_sound = rm.audio_on();
            replay_state = match rm.status {
                PAUSE if !replay_over => "REPLAY PAUSED".to_string(),
                STOP => "REPLAY STOPPED (Ctrl+Space plays)".to_string(),
                FAST_FORWARD => format!("FAST FORWARD x{replay_scale:.0}"),
                SLOW_MOTION => format!("SLOW MOTION x{replay_scale:.2}"),
                _ => String::new(),
            };
        }
        let mut info = HudInfo {
            fps,
            fps_counter,
            frame_ms,
            frame_max_ms,
            timing: shared.timing(),
            paused: ac_replay_line.is_none() && (shared.paused.load(Ordering::Relaxed) || unfocused),
            camera: camera.name(),
            replay: replay.is_some() || ac_replay_line.is_some(),
            replay_over,
            replay_events: if ac_replay_line.is_some() { ac_replay_events } else { replay_events },
            replay_scale: if ac_replay_line.is_some() { ac_replay_scale } else { replay_scale },
            replay_transport: transport.is_some() || ac_replay_line.is_some(),
            replay_state: ac_replay_line.clone().unwrap_or(replay_state),
            notes: shared.online_hud.lock().unwrap().clone(),
            hidden: !debug_hud,
            focus_fade,
            referee: shared.referee_hud.lock().unwrap().clone(),
        };
        // Task 28: the start lights count down to the referee's race start
        if let Some(referee) = &info.referee {
            renderer.set_race_start_in(referee.time_to_start_ms);
        }
        if focus.index != 0 {
            info.notes.push(format!("camera on car {} ({}): Ctrl+{} is your car", focus.index, car_names[focus.index], rustyac_game::input::bindings::key_name(bindings.key_player_car)));
        }
        if replay_over {
            info.notes.push("the replay is over (Esc)".to_string());
        } else if unfocused {
            info.notes.push("click the window to drive".to_string());
        }
        // Game::update: the car's one backfire test (CarAvatar::update), whose event the sound
        // and the flames both hear; the flames empty its counter
        // the pit crew's wheel guns: what the crew will ask of them in this frame (the gun of
        // each mechanic who is unscrewing or screwing, when his car is the focused one)
        let guns = renderer.crew_guns_wanted();
        if let (Some(sound), Some(link)) = (audio.as_mut(), renderer.crew_link()) {
            for (wheel, wanted) in guns.iter().enumerate() {
                if let (Some(screw), true) = (*wanted, focus.index == 0) {
                    link.playing[wheel][screw as usize] = sound.crew_is_playing(wheel, screw);
                }
            }
        }
        renderer.draw_world(&world, focus.index, &shapes[focus.index], &camera, &frame, &info, dt.as_secs_f32());
        if let Some(link) = renderer.crew_link() {
            use rustyac_game::render::ac::CrewSound;
            let calls = std::mem::take(&mut link.sound);
            // (the camera's part: see docs/port/replays.md, "Not done")
            link.camera.clear();
            if let Some(sound) = audio.as_mut() {
                for call in calls {
                    match call {
                        CrewSound::Start { wheel, screw, matrix } => sound.crew_start(wheel, screw, &matrix.m),
                        CrewSound::Stop { wheel, screw } => sound.crew_stop(wheel, screw),
                    }
                }
            }
        }
        // Game::evOnPostGui: the focus changes at the fade's black frame
        focus.post_gui(dt.as_secs_f32(), &world, &car_names);
        // Game::renderAudio and AudioEngine::update: after the cameras, before the picture is shown
        if let Some(sound) = audio.as_mut() {
            sound.set_focus(focus.index);
            if options.audio.clockless() {
                sound.grid_frames(|_| (camera, frame.matrix));
            } else {
                sound.set_picture_backfires(&renderer.backfires_triggered());
                // (ReplayManager::updateReplayAudio: no sound outside half to double speed)
                sound.frame(&camera, &frame.matrix, dt.as_secs_f64(), info.paused || !replay_sound || (info.replay_transport && transport.is_none() && !ac_replay_audio));
            }
        }
        // --bench-shot in a window: the picture a quarter of a second before --duration ends
        if let (Some(path), false, true) = (&options.bench_shot, window_shot, options.duration.is_some_and(|d| started.elapsed().as_secs_f64() >= d - 0.25)) {
            window_shot = true;
            match renderer.read_pixels() {
                Ok(pixels) => {
                    let (width, height) = renderer.size();
                    match write_png(path, width, height, &pixels) {
                        Ok(()) => println!("{}: {width} x {height}, the window's picture", path.display()),
                        Err(message) => eprintln!("WARNING: {message}"),
                    }
                }
                Err(message) => eprintln!("WARNING: {message}"),
            }
        }
        match renderer.present(&chain, options.vsync) {
            // nothing is seen: no need to draw as fast as the card can
            Ok(false) => std::thread::sleep(Duration::from_millis(15)),
            Ok(true) => {}
            Err(message) => {
                result = Err(message);
                break;
            }
        }
        frames += 1;
        fps_frames += 1;
        if frames > 30 {
            frame_max = frame_max.max(dt);
        }
        running_max = running_max.max(dt);
        if fps_since.elapsed() >= Duration::from_millis(500) {
            fps = fps_frames as f32 / fps_since.elapsed().as_secs_f32();
            frame_ms = 1000.0 / fps.max(1e-3);
            frame_max_ms = running_max.as_secs_f32() * 1000.0;
            running_max = Duration::ZERO;
            fps_frames = 0;
            fps_since = Instant::now();
        }
    }
    let seconds = started.elapsed().as_secs_f64();
    // what the game writes at its end, into rustyAC's own cfg folder
    if let Some(gui) = renderer.gui() {
        match gui.shutdown() {
            Ok(files) => println!("GUI: wrote {}", files.iter().map(|f| f.display().to_string()).collect::<Vec<_>>().join(", ")),
            Err(message) => eprintln!("WARNING: the GUI's files were not written ({message})"),
        }
    }
    shared.quit.store(true, Ordering::Relaxed);
    let outcome = thread.join().map_err(|_| "the physics thread panicked".to_string())?;
    drop(chain);
    drop(window);
    // SAFETY: as above; the display may sleep again.
    unsafe { SetThreadExecutionState(ES_CONTINUOUS) };
    let audio_timing = audio.map(GameAudio::finish);
    let (view, timing) = outcome?;
    report(&view, &timing);
    if let Some(audio_timing) = audio_timing {
        println!("{}", audio_timing.report());
    }
    println!(
        "render: {frames} frames in {seconds:.1} s = {:.1} FPS ({}), slowest frame {:.1} ms",
        frames as f64 / seconds.max(1e-9),
        if options.vsync { "waiting for the display" } else { "not waiting for the display" },
        frame_max.as_secs_f64() * 1000.0
    );
    result
}

/// `--screenshot <png>`: drives for a while without a clock (a recorded drive, or nobody),
/// then draws one frame off screen and writes it as a PNG. No window is opened.
fn run_screenshot(options: &Options, path: &Path) -> Result<(), String> {
    let (setup, steps) = drive(options)?;
    let source: Box<dyn DriverSource> = if steps.is_some() {
        Box::new(ReplaySource::default())
    } else if options.autodrive {
        Box::new(SpawnSequence::new(AutoDriver::new(), true))
    } else if options.flat_out {
        Box::new(SpawnSequence::new(rustyac_game::sim::FlatOutSource::with_brake(options.flat_out_brake), true))
    } else {
        Box::new(SpawnSequence::new(NobodySource, true))
    };
    // with the line follower the drive can be kept: it runs without a clock, so a lap takes seconds
    let mut writer = match (&options.record, steps.is_none()) {
        (Some(path), true) => Some(InputWriter::create(path, &setup)?),
        _ => None,
    };
    let mut sim = GameSim::new(setup, source)?;
    if !sim.track_summary.is_empty() {
        println!("{}", sim.track_summary);
    }
    let wanted = options.at.map(|seconds| (seconds / 0.003).round() as usize);
    // --lead-in: the car as it was every 1/60 s over the last seconds
    let total = wanted.unwrap_or(1000);
    let lead_from = total.saturating_sub((options.lead_in / 0.003).round() as usize);
    let mut lead: Vec<Vec<CarView>> = Vec::new();
    let mut lead_due = 0.0f64;
    let mut drive_start = 0;
    let mut previous = CarView::capture(&sim, 0.0);
    match &steps {
        Some(steps) => {
            for step in steps.iter().take(wanted.unwrap_or(steps.len())) {
                previous = CarView::capture(&sim, 0.0);
                sim.step_recorded(step)?;
            }
        }
        None => {
            for i in 0..total {
                if sim.car.device.source.in_spawn_sequence() {
                    drive_start = sim.steps + 1;
                }
                previous = CarView::capture(&sim, 0.0);
                if options.lead_in > 0.0 && i >= lead_from {
                    lead_due += 0.003;
                    if lead_due >= 1.0 / 60.0 {
                        lead_due -= 1.0 / 60.0;
                        lead.push(CarView::capture_world(&sim, sim.steps.saturating_sub(drive_start) as f64 * 0.003));
                    }
                }
                let input = sim.step()?;
                if let Some(writer) = &mut writer {
                    writer.push(&input)?;
                }
            }
            if let Some(writer) = writer.take() {
                println!("{} steps of the drive written to {}", writer.finish()?, options.record.as_ref().unwrap().display());
            }
        }
    }
    let mut view = CarView::capture(&sim, sim.steps.saturating_sub(drive_start) as f64 * 0.003);
    // --show-lights, --show-damage: the picture's own switches
    let dress = |v: &mut CarView| {
        v.lights |= options.show_lights;
        if let Some(damage) = options.show_damage {
            v.damage = damage;
        }
    };
    dress(&mut view);
    lead.iter_mut().flatten().for_each(dress);
    if let Some(pose) = &options.pose_out {
        std::fs::write(pose, view.physics_state().to_bytes()).map_err(|e| format!("{}: {e}", pose.display()))?;
        println!("{}: the car's state after {} steps", pose.display(), sim.steps);
    }
    // the cars of the picture and the one the camera is on (--focus)
    let other_infos = sim.opponent_infos();
    let focus = options.focus.min(other_infos.len());
    let mut world = CarView::capture_world(&sim, sim.steps.saturating_sub(drive_start) as f64 * 0.003);
    world.iter_mut().for_each(dress);
    let previous = if focus == 0 { previous } else { world[focus] };
    let view = world[focus];
    let shape = CarShape::of(&if focus == 0 { sim.car_info() } else { other_infos[focus - 1].clone() });
    // --supersample: the picture is drawn larger and averaged down at the end
    let supersample = options.supersample.max(1);
    let mut renderer = make_picture(options, options.width * supersample, options.height * supersample, &sim.car_info(), &other_infos)?;
    let mut camera = DrivingCamera::from_name(&options.camera, &shape)?;
    // --free-camera: it stands where the car of the picture puts it, in the lead-in frames too
    let free = options.free_camera.map(|free| CameraFrame { matrix: free_camera(&view.body, free.eye, free.look), fov: free.fov, near: 0.1 });
    if options.virtual_mirror {
        renderer.set_virtual_mirror(true);
    }
    // --no-hud: the virtual mirror is part of the display (a preset may have switched it on)
    if options.no_hud {
        renderer.set_virtual_mirror(false);
    }
    // rustyAC's own panels: with --debug-hud, or when AC's HUD is not there
    let text_hidden = options.no_hud || !(options.debug_hud || renderer.gui().is_none());
    if let (Some(commands), Some(gui)) = (&options.gui_do, renderer.gui()) {
        gui.script(commands)?;
    }
    let lead_info = HudInfo { fps: 0.0, fps_counter: options.show_fps, camera: camera.name(), replay: steps.is_some(), hidden: text_hidden, referee: sim.referee_hud(), ..HudInfo::default() };
    for old_world in &lead {
        let old = &old_world[focus.min(old_world.len() - 1)];
        let frame = camera.update(old, &shape, old.acc_g, 1.0 / 60.0);
        let frame = free.unwrap_or(frame);
        renderer.draw_world(old_world, focus, &shape, &camera, &frame, &lead_info, 1.0 / 60.0);
    }
    // two frames, so that the chase camera has leaned into the car's acceleration
    camera.update(&previous, &shape, previous.acc_g, 1.0 / 60.0);
    let frame = camera.update(&view, &shape, view.acc_g, 1.0);
    let frame = free.unwrap_or(frame);
    let info = HudInfo { fps: 0.0, fps_counter: options.show_fps, camera: camera.name(), replay: steps.is_some(), hidden: text_hidden, referee: sim.referee_hud(), ..HudInfo::default() };
    // two frames: the second is a frame as the game draws them one after the other (the first
    // after loading still uploads and binds everything)
    let dt = if lead.is_empty() { 0.0 } else { 1.0 / 60.0 };
    // (the motion blur smears by the camera's movement since the frame before: the second draw
    // starts from the same "frame before" as the first)
    let blur = renderer.blur_history();
    renderer.draw_world(&world, focus, &shape, &camera, &frame, &info, dt);
    renderer.set_blur_history(blur);
    renderer.draw_world(&world, focus, &shape, &camera, &frame, &info, dt);
    let mut pixels = renderer.read_pixels()?;
    let (mut width, mut height) = renderer.size();
    if supersample > 1 {
        (width, height) = (width / supersample, height / supersample);
        pixels = box_downscale(&pixels, width, height, supersample);
    }
    write_png(path, width, height, &pixels)?;
    println!("{}: {width} x {height}, drawn by {} after {} steps ({:.1} km/h, gear {})", path.display(), renderer.describe(), sim.steps, view.speed_kmh, view.gear - 1);
    if let Some(stats) = renderer.frame_stats() {
        println!("  {stats}");
    }
    // the cars of the picture: where each is and how hard it has been hit
    if world.len() > 1 {
        for (number, car) in world.iter().enumerate() {
            println!(
                "  car {number}{}: at {:.1} {:.1} {:.1} heading {:.0} deg, {:.1} km/h, {:.1} % round the lap, damage front / rear / left / right {:.0} / {:.0} / {:.0} / {:.0}",
                if number == focus { " (the camera's)" } else { "" },
                car.body[3][0],
                car.body[3][1],
                car.body[3][2],
                car.body[2][0].atan2(car.body[2][2]).to_degrees(),
                car.speed_kmh,
                car.lap.position.clamp(0.0, 1.0) * 100.0,
                car.damage[0],
                car.damage[1],
                car.damage[2],
                car.damage[3]
            );
        }
    }
    lap_report(&view);
    Ok(())
}

/// `--replay <file> --headless --audio-wav <wav>`: a recorded drive as fast as it goes, its
/// sound written without a clock: one frame of the sound (one 1/60 s block of the WAV) after
/// every `floor(n * 50 / 9)` physics steps, the listener at the camera of `--camera`.
fn run_replay_audio(options: &Options, replay: &Path) -> Result<(), String> {
    let file = rustyac_game::input_file::InputFile::read(replay)?;
    let mut sim = GameSim::new(file.setup.clone(), Box::new(ReplaySource::default()))?;
    let info = sim.car_info();
    let others = sim.opponent_infos();
    let focus = options.focus.min(others.len());
    let shape = CarShape::of(if focus == 0 { &info } else { &others[focus - 1] });
    let feed = Some(Arc::new(Mutex::new(Feed::with_grid())));
    let mut audio = start_audio(options, &feed, &info, &others).ok_or("there is no sound to write")?;
    let mut camera = DrivingCamera::from_name(&options.camera, &shape)?;
    let mut sticky = Vec::new();
    let started = Instant::now();
    for step in &file.steps {
        sim.step_recorded(step)?;
        let waiting = {
            let mut feed = feed.as_ref().unwrap().lock().unwrap();
            feed.push(&sim, &mut sticky);
            !feed.grid.is_empty()
        };
        if waiting {
            let view = if focus == 0 { CarView::capture(&sim, 0.0) } else { CarView::capture_opponent(&sim, focus - 1, 0.0) };
            let frame = camera.update(&view, &shape, view.acc_g, rustyac_game::audio::NRT_DT);
            audio.grid_frames(|_| (camera, frame.matrix));
        }
    }
    let timing = audio.finish();
    println!("{}", timing.report());
    eprintln!("replayed {} steps ({:.1} s of driving) with sound in {:.2} s", sim.steps, sim.steps as f64 * 0.003, started.elapsed().as_secs_f64());
    Ok(())
}

fn run(options: &Options) -> Result<(), String> {
    if options.list_tracks {
        print!("{}", list_tracks());
        return Ok(());
    }
    if options.list_devices {
        let mut notes = Vec::new();
        let bindings = Bindings::load(options, &mut notes);
        for note in &notes {
            println!("bindings: {note}");
        }
        let wheel = WheelDevice::open(&bindings, 0, false);
        print!("{}{}", list_devices(&bindings, &wheel), bindings.describe());
        return Ok(());
    }
    // which maths this run computes with (on the error stream where the standard output may
    // be carrying a state dump)
    if options.dump_states.is_some() {
        eprintln!("{}", rustyac_math::describe());
    } else {
        println!("{}", rustyac_math::describe());
    }
    // Task 28c: --replay with one of AC's replays: the file names the track and the cars; they
    // are built as a session whose physics stands still, and the replay shows
    if let Some(file) = options.replay.clone().filter(|file| rustyac_game::ac_replay::is_ac_replay(file)) {
        let bytes = std::fs::read(&file).map_err(|e| format!("{}: {e}", file.display()))?;
        let header = rustyac_game::ac_replay::read_header(&bytes)?;
        let first = header.cars.first().ok_or("the replay has no car")?;
        let mut o = options.clone();
        o.ac_replay = Some(file);
        o.replay = None;
        o.headless = false;
        o.car = first.unix_name.clone();
        o.skin = Some(first.skin.clone()).filter(|skin| !skin.is_empty());
        o.track = Some(header.track.clone()).filter(|track| !track.is_empty());
        o.layout = Some(if header.layout.is_empty() { "-".to_string() } else { header.layout.clone() });
        o.opponents = header.cars[1..].iter().map(|car| if car.skin.is_empty() { format!("{}:parked", car.unix_name) } else { format!("{}:{}:parked", car.unix_name, car.skin) }).collect();
        o.cars = None;
        o.ai_cars = None;
        o.race_ini = Some(false);
        o.no_referee = true;
        o.sessions = None;
        o.no_shm = true;
        println!("{}: AC replay version {}: {} on {} {}", o.ac_replay.as_ref().map(|f| f.display().to_string()).unwrap_or_default(), header.version, header.cars.iter().map(|car| car.unix_name.as_str()).collect::<Vec<_>>().join(", "), header.track, header.layout);
        return run_window(&o);
    }
    if let Some(path) = &options.screenshot {
        return run_screenshot(options, path);
    }
    if options.headless {
        if let (Some(replay), false) = (&options.replay, options.realtime) {
            if options.audio.wav.is_some() && !options.audio.off {
                return run_replay_audio(options, replay);
            }
            // as fast as it goes
            let started = Instant::now();
            let ghost = ghost_rig(options);
            let mut ghost = ghost.as_ref().map(|ghost| ghost.lock().unwrap());
            let telemetry = telemetry_rig(options, false);
            let mut telemetry = telemetry.as_ref().map(|rig| rig.lock().unwrap());
            let steps = rustyac_game::run_replay_headless_saving(replay, options.dump_states.as_deref(), options.state_hash, options.physics_threads, options.save_replay.as_deref().filter(|_| !options.no_ac_replay), ghost.as_deref_mut(), telemetry.as_deref_mut())?;
            let seconds = started.elapsed().as_secs_f64();
            // the dump may be going to the standard output
            eprintln!(
                "replayed {steps} steps ({:.1} s of driving) in {seconds:.2} s ({:.1} x real time)",
                steps as f64 * 0.003,
                steps as f64 * 0.003 / seconds.max(1e-9)
            );
            return Ok(());
        }
        return run_headless(options);
    }
    run_window(options)
}

pub fn main() {
    let options = match Options::parse(std::env::args().skip(1)) {
        Ok(options) => options,
        Err(message) => {
            eprintln!("{message}");
            std::process::exit(2);
        }
    };
    // SAFETY: the handler only touches two atomics.
    unsafe {
        let _ = SetConsoleCtrlHandler(Some(console_handler), true);
    }
    let result = run(&options);
    DONE.store(true, Ordering::Relaxed);
    if let Err(message) = result {
        eprintln!("rustyac: {message}");
        std::process::exit(1);
    }
}
