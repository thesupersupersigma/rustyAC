// SPDX-License-Identifier: GPL-3.0-or-later

//! `rustyac.exe`: see `docs/game/first_drive.md`.

use std::path::Path;
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
use rustyac_game::render::scene::{CarShape, DrivingCamera};
use rustyac_game::render::ac::{AcOptions, AcRenderer};
use rustyac_game::render::{write_png, DebugRenderer, Picture};
use rustyac_game::shm::{SharedMemory, ShmSink};
use rustyac_game::sim::{DriverSource, GameSim, NobodySource, ReplaySource, SpawnSequence};
use rustyac_game::view::CarView;
use rustyac_game::window::{Event, Window};
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
    // the device the bindings name decides one aid (a pad has no clutch pedal)
    let input_method = Bindings::load(options, &mut Vec::new()).input_method;
    for note in rustyac_game::conditions::apply(options, &mut setup, &input_method)? {
        println!("{note}");
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
fn make_ac(options: &Options, width: u32, height: u32, info: &rustyac_game::sim::CarInfo) -> Result<AcRenderer, String> {
    let game = rustyac_content::install::ac_root().ok_or_else(rustyac_content::install::not_found_hint)?;
    // the sun and the weather of race.ini; without one, midday and clear
    let (mut sun_angle, mut weather, mut skin) = (-16.0f32, "3_clear".to_string(), options.skin.clone());
    let race = match (&options.race_ini_file, options.race_ini) {
        (Some(file), _) => Some(file.clone()),
        (None, Some(false)) => None,
        (None, _) => rustyac_game::conditions::race_ini_path().filter(|p| p.is_file()),
    };
    if let Some(ini) = race.and_then(|p| rustyac_physics::data::ini::IniReader::load(&p).ok()) {
        if ini.has_section("LIGHTING") {
            sun_angle = ini.get_float("LIGHTING", "SUN_ANGLE").unwrap_or(sun_angle);
        }
        weather = if ini.has_section("WEATHER") { ini.get_string("WEATHER", "NAME") } else { String::new() };
        if skin.is_none() {
            let name = ini.get_string("CAR_0", "SKIN");
            if !name.is_empty() {
                skin = Some(name);
            }
        }
    }
    if !weather.is_empty() && weather != "3_clear" && game.join("content/weather").join(&weather).join("weather.ini").is_file() {
        println!("weather {weather}: its fog and colours are used; its clouds come in Task 21");
    }
    let mut renderer = AcRenderer::new(width, height, AcOptions { warp: options.warp, gpu_log: options.gpu_log.clone(), game: game.clone(), sun_angle, weather, skin: None, video_exact: options.video_ini_exact, cube_faces: options.cube_faces })?;
    for note in &renderer.notes {
        println!("{note}");
    }
    if let Some(folder) = &info.track_folder {
        println!("{}", renderer.load_track(folder, &info.track_layout)?);
    }
    // the car's folder in the game (its data may come from elsewhere)
    let car_folder = rustyac_game::sim::find_car_model(&info.name, &info.data_path).and_then(|kn5| kn5.parent().map(|p| p.to_path_buf()));
    match car_folder {
        Some(folder) => {
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
            println!("{}", renderer.load_car(&folder, steer_lock, tyre_width)?);
        }
        None => println!("no 3D model of {} in Assetto Corsa's folder: no car is drawn", info.name),
    }
    renderer.finish_loading()?;
    Ok(renderer)
}

/// The picture: AC's renderer, or the debug view when it was asked for or when AC's renderer
/// cannot start (no Assetto Corsa folder, no Direct3D 11 …).
fn make_picture(options: &Options, width: u32, height: u32, info: &rustyac_game::sim::CarInfo) -> Result<Picture, String> {
    if !options.debug_view {
        match make_ac(options, width, height, info) {
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
fn start_audio(options: &Options, feed: &Option<Arc<Mutex<Feed>>>, info: &rustyac_game::sim::CarInfo, shape: &CarShape) -> Option<GameAudio> {
    let feed = feed.as_ref()?;
    // the car's folder name is its name in the game
    let name = Path::new(&info.name).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| info.name.clone());
    let track = info.track_folder.as_deref().map(|folder| (folder, info.track_layout.as_str()));
    GameAudio::start(&options.audio, Arc::clone(feed), &name, &info.data_path, track, shape)
}

fn sinks(options: &Options, setup: &SimSetup, feed: &Option<Arc<Mutex<Feed>>>) -> Result<Vec<Box<dyn StepSink>>, String> {
    let mut sinks: Vec<Box<dyn StepSink>> = Vec::new();
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
) -> Result<std::thread::JoinHandle<Outcome>, String> {
    let shared = Arc::clone(shared);
    std::thread::Builder::new()
        .name("physics".to_string())
        .spawn(move || {
            let outcome = GameSim::new(setup, make_source()).and_then(|sim| physics_thread::run(sim, &shared, sinks, config));
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
    let (setup, replay) = drive(options)?;
    let feed = audio_feed(options, true);
    let sinks = sinks(options, &setup, &feed)?;
    let shared = Shared::new();
    let config = LoopConfig {
        replay: replay.clone(),
        duration: options.duration.map(Duration::from_secs_f64),
        max_steps: None,
        pause_unfocused: false,
        auto_reset: true,
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
    let thread = start_physics(setup, source, &shared, sinks, config)?;
    let mut frames = 0u64;
    let mut frame_max = Duration::ZERO;
    let started = Instant::now();
    let mut headless_audio: Option<GameAudio> = None;
    let mut bench_stats = None;
    if options.bench_render {
        let mut renderer: Option<Picture> = None;
        let mut camera = None;
        let mut shape = None;
        let mut last = Instant::now();
        while !shared.finished.load(Ordering::Relaxed) && !thread.is_finished() && !STOP.load(Ordering::Relaxed) {
            if shape.is_none() {
                let info = shared.car_info.lock().unwrap().clone();
                if let Some(info) = info {
                    match make_picture(options, options.width, options.height, &info) {
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
                    let car_shape = CarShape::of(&info);
                    camera = Some(DrivingCamera::from_name(&options.camera, &car_shape).unwrap_or_else(|_| DrivingCamera::chase()));
                    shape = Some(car_shape);
                    // the loading is not part of the timing
                    last = Instant::now();
                }
            }
            let (Some(shape), Some(renderer), Some(camera)) = (&shape, &mut renderer, &mut camera) else {
                std::thread::sleep(Duration::from_millis(2));
                continue;
            };
            let now = Instant::now();
            let view = shared.frames().at(now);
            let dt = (now - last).as_secs_f32();
            let frame = camera.update(&view, shape, view.acc_g, dt);
            let info = HudInfo { fps: 0.0, timing: shared.timing(), camera: camera.name(), replay: replaying, ..HudInfo::default() };
            renderer.draw(&view, shape, camera, &frame, &info, dt);
            renderer.finish();
            bench_stats = renderer.frame_stats();
            frames += 1;
            if frames > 10 {
                frame_max = frame_max.max(now - last);
            }
            last = now;
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
                    let shape = CarShape::of(&info);
                    let camera = DrivingCamera::from_name(&options.camera, &shape).unwrap_or_else(|_| DrivingCamera::chase());
                    match start_audio(options, &feed, &info, &shape) {
                        Some(sound) => audio = Some((sound, shape, camera)),
                        None => failed = true,
                    }
                    last = Instant::now();
                }
            }
            if let Some((sound, shape, camera)) = audio.as_mut() {
                let now = Instant::now();
                let view = shared.frames().at(now);
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
    let seconds = started.elapsed().as_secs_f64();
    shared.quit.store(true, Ordering::Relaxed);
    let (view, timing) = thread.join().map_err(|_| "the physics thread panicked".to_string())??;
    report(&view, &timing);
    if let Some(sound) = headless_audio {
        println!("{}", sound.finish().report());
    }
    if options.bench_render {
        println!(
            "render (off screen, not waiting for a display): {frames} frames in {seconds:.1} s = {:.0} FPS, slowest frame {:.1} ms",
            frames as f64 / seconds,
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
    let (mut setup, replay) = drive(options)?;
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
    let sinks = sinks(options, &setup, &feed)?;
    let shared = Shared::new();
    // the window says when it has the keyboard (and Windows may not have given it)
    shared.focused.store(false, Ordering::Relaxed);
    let config = LoopConfig { replay: replay.clone(), duration: None, max_steps: None, pause_unfocused: !options.no_focus, auto_reset: true };
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
    let thread = start_physics(setup, source, &shared, sinks, config)?;

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
    let shape = CarShape::of(&car_info);
    let (width, height) = window.client_size();
    let graphics = make_picture(options, width, height, &car_info).and_then(|renderer| {
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
    let mut audio = start_audio(options, &feed, &car_info, &shape);
    let mut camera = match DrivingCamera::from_name(&options.camera, &shape) {
        Ok(camera) => camera,
        Err(message) => {
            // (as above: the physics thread runs already)
            shared.quit.store(true, Ordering::Relaxed);
            let _ = thread.join();
            return Err(message);
        }
    };
    let mut camera_toggles = 0;
    let started = Instant::now();
    let mut last = Instant::now();
    let (mut frames, mut fps, mut fps_frames, mut fps_since) = (0u64, 0.0f32, 0u32, Instant::now());
    let mut frame_max = Duration::ZERO;
    let mut result = Ok(());
    'frames: loop {
        for event in window.pump() {
            match event {
                Event::Close | Event::Key { key: 0x1b, .. } => break 'frames,
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
                    if ctrl && key == bindings.key_traction_control {
                        request(if shift { event::TC_DN } else { event::TC_UP });
                    } else if ctrl && key == bindings.key_abs {
                        request(if shift { event::ABS_DN } else { event::ABS_UP });
                    } else if ctrl && key == bindings.key_auto_shifter {
                        request(event::AUTO_SHIFTER);
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
                            0x75 => camera.f6(shape.car_cameras.len()),
                            0x50 | 0x13 => {
                                shared.paused.fetch_xor(true, Ordering::Relaxed);
                            }
                            0x52 => request(if shift { event::TO_TRACK } else { event::RESET }),
                            0x4e => request(event::REBUILD),
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
        let view = shared.frames().at(now);
        let frame = camera.update(&view, &shape, view.acc_g, dt.as_secs_f32());
        let replay_over = replay.is_some() && shared.finished.load(Ordering::Relaxed);
        let unfocused = !options.no_focus && !shared.focused.load(Ordering::Relaxed);
        let mut info = HudInfo {
            fps,
            timing: shared.timing(),
            paused: shared.paused.load(Ordering::Relaxed) || unfocused,
            camera: camera.name(),
            replay: replay.is_some(),
            notes: Vec::new(),
        };
        if replay_over {
            info.notes.push("the replay is over (Esc)".to_string());
        } else if unfocused {
            info.notes.push("click the window to drive".to_string());
        }
        // Game::renderAudio and AudioEngine::update: after the cameras, before the picture is shown
        if let Some(sound) = audio.as_mut() {
            if options.audio.clockless() {
                sound.grid_frames(|_| (camera, frame.matrix));
            } else {
                sound.frame(&camera, &frame.matrix, dt.as_secs_f64(), info.paused);
            }
        }
        renderer.draw(&view, &shape, &camera, &frame, &info, dt.as_secs_f32());
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
        if fps_since.elapsed() >= Duration::from_millis(500) {
            fps = fps_frames as f32 / fps_since.elapsed().as_secs_f32();
            fps_frames = 0;
            fps_since = Instant::now();
        }
    }
    let seconds = started.elapsed().as_secs_f64();
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
            for _ in 0..wanted.unwrap_or(1000) {
                if sim.car.device.source.in_spawn_sequence() {
                    drive_start = sim.steps + 1;
                }
                previous = CarView::capture(&sim, 0.0);
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
    let view = CarView::capture(&sim, sim.steps.saturating_sub(drive_start) as f64 * 0.003);
    if let Some(pose) = &options.pose_out {
        std::fs::write(pose, view.physics_state().to_bytes()).map_err(|e| format!("{}: {e}", pose.display()))?;
        println!("{}: the car's state after {} steps", pose.display(), sim.steps);
    }
    let shape = CarShape::of(&sim.car_info());
    let mut renderer = make_picture(options, options.width, options.height, &sim.car_info())?;
    let mut camera = DrivingCamera::from_name(&options.camera, &shape)?;
    // two frames, so that the chase camera has leaned into the car's acceleration
    camera.update(&previous, &shape, previous.acc_g, 1.0 / 60.0);
    let frame = camera.update(&view, &shape, view.acc_g, 1.0);
    let info = HudInfo { fps: 0.0, camera: camera.name(), replay: steps.is_some(), ..HudInfo::default() };
    // two frames: the second is a frame as the game draws them one after the other (the first
    // after loading still uploads and binds everything)
    renderer.draw(&view, &shape, &camera, &frame, &info, 0.0);
    renderer.draw(&view, &shape, &camera, &frame, &info, 0.0);
    let pixels = renderer.read_pixels()?;
    let (width, height) = renderer.size();
    write_png(path, width, height, &pixels)?;
    println!("{}: {width} x {height}, drawn by {} after {} steps ({:.1} km/h, gear {})", path.display(), renderer.describe(), sim.steps, view.speed_kmh, view.gear - 1);
    if let Some(stats) = renderer.frame_stats() {
        println!("  {stats}");
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
    let shape = CarShape::of(&info);
    let feed = Some(Arc::new(Mutex::new(Feed::with_grid())));
    let mut audio = start_audio(options, &feed, &info, &shape).ok_or("there is no sound to write")?;
    let mut camera = DrivingCamera::from_name(&options.camera, &shape)?;
    let mut sticky = Default::default();
    let started = Instant::now();
    for step in &file.steps {
        sim.step_recorded(step)?;
        let waiting = {
            let mut feed = feed.as_ref().unwrap().lock().unwrap();
            feed.push(&sim, &mut sticky);
            !feed.grid.is_empty()
        };
        if waiting {
            let view = CarView::capture(&sim, 0.0);
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
            let steps = rustyac_game::run_replay_headless(replay, options.dump_states.as_deref())?;
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
