//! `rustyac.exe`: see `docs/game/first_drive.md`.

use std::path::Path;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant};

use rustyac_game::cli::Options;
use rustyac_game::input::bindings::Bindings;
use rustyac_game::input::pad::XInput;
use rustyac_game::input::wheel::WheelDevice;
use rustyac_game::input::LiveSource;
use rustyac_game::input_file::{event, InputFile, InputWriter, SimSetup, StepInput};
use rustyac_game::physics_thread::{self, LoopConfig, Recorder, Shared, StepSink, Timing};
use rustyac_game::render::hud::HudInfo;
use rustyac_game::render::scene::{CameraMode, CarShape, DrivingCamera};
use rustyac_game::render::{write_png, DebugRenderer};
use rustyac_game::shm::{SharedMemory, ShmSink};
use rustyac_game::sim::{DriverSource, GameSim, NobodySource, ReplaySource, SpawnSequence};
use rustyac_game::view::CarView;
use rustyac_game::window::{Event, Window};

/// The car and session of a live drive, from the command line.
fn live_setup(options: &Options) -> SimSetup {
    SimSetup { car: options.car.clone(), auto_clutch: !options.no_auto_clutch, auto_shifter: options.auto_shifter, ..SimSetup::default() }
}

/// The recorder and the shared memory, as asked for.
fn sinks(options: &Options, setup: &SimSetup) -> Result<Vec<Box<dyn StepSink>>, String> {
    let mut sinks: Vec<Box<dyn StepSink>> = Vec::new();
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
        None => Ok((live_setup(options), None)),
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

fn report(view: &CarView, timing: &Timing) {
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
}

/// `--headless` without a window: the car runs in real time, with a recorded drive or with
/// nobody at the controls, until `--duration` is over or the drive ends. With
/// `--bench-render` frames are drawn off screen meanwhile, as fast as the card goes.
fn run_headless(options: &Options) -> Result<(), String> {
    let (setup, replay) = drive(options)?;
    let sinks = sinks(options, &setup)?;
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
    let source = move || -> Box<dyn DriverSource> {
        if replaying {
            Box::new(ReplaySource::default())
        } else {
            Box::new(SpawnSequence::new(NobodySource, true))
        }
    };
    let thread = start_physics(setup, source, &shared, sinks, config)?;
    let mut frames = 0u64;
    let mut frame_max = Duration::ZERO;
    let started = Instant::now();
    if options.bench_render {
        let mut renderer = DebugRenderer::new(options.width, options.height)?;
        println!(
            "drawing {} x {} off screen with {} samples per pixel on {}{}",
            options.width,
            options.height,
            renderer.samples(),
            renderer.adapter,
            if renderer.software { " (software rasteriser)" } else { "" }
        );
        let mut camera = DrivingCamera::new(CameraMode::Chase);
        let mut shape = None;
        let mut last = Instant::now();
        while !shared.finished.load(Ordering::Relaxed) {
            if shape.is_none() {
                shape = shared.car_info.lock().unwrap().as_ref().map(CarShape::of);
            }
            let Some(shape) = &shape else {
                std::thread::sleep(Duration::from_millis(2));
                continue;
            };
            let now = Instant::now();
            let view = shared.frames().at(now);
            let frame = camera.update(&view, shape, view.acc_g, (now - last).as_secs_f32());
            let info = HudInfo { fps: 0.0, timing: shared.timing(), camera: camera.mode.name(), replay: replaying, ..HudInfo::default() };
            renderer.draw(&view, shape, &frame, &info);
            renderer.finish();
            frames += 1;
            if frames > 10 {
                frame_max = frame_max.max(now - last);
            }
            last = now;
        }
    } else {
        // a replay ends by itself; so does a run with a duration
        while !shared.finished.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_millis(20));
        }
    }
    let seconds = started.elapsed().as_secs_f64();
    shared.quit.store(true, Ordering::Relaxed);
    let (view, timing) = thread.join().map_err(|_| "the physics thread panicked".to_string())??;
    report(&view, &timing);
    if options.bench_render {
        println!(
            "render (off screen, not waiting for a display): {frames} frames in {seconds:.1} s = {:.0} FPS, slowest frame {:.1} ms",
            frames as f64 / seconds,
            frame_max.as_secs_f64() * 1000.0
        );
    }
    Ok(())
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
    if replay.is_none() {
        print!("{}", bindings.describe());
        match bindings.write_if_missing() {
            Ok(Some(path)) => println!("bindings written to {} (edit it to change them)", path.display()),
            Ok(None) => {}
            Err(message) => eprintln!("WARNING: {message}"),
        }
    }
    let sinks = sinks(options, &setup)?;
    let shared = Shared::new();
    shared.focused.store(!options.no_focus, Ordering::Relaxed);
    let config = LoopConfig { replay: replay.clone(), duration: None, max_steps: None, pause_unfocused: !options.no_focus, auto_reset: true };
    let source = {
        let (bindings, shared, rumble, ffb, live) = (bindings.clone(), Arc::clone(&shared), !options.no_rumble, options.ffb, replay.is_none());
        move || -> Box<dyn DriverSource> {
            if !live {
                return Box::new(ReplaySource::default());
            }
            let wheel = WheelDevice::open(&bindings, window_handle, ffb);
            print!("{}", list_devices(&bindings, &wheel));
            Box::new(SpawnSequence::new(LiveSource::new(bindings, shared, rumble, wheel), true))
        }
    };
    let thread = start_physics(setup, source, &shared, sinks, config)?;

    // the car is built on the physics thread: wait for it (or for its refusal)
    let shape = loop {
        if let Some(info) = shared.car_info.lock().unwrap().as_ref() {
            break CarShape::of(info);
        }
        if thread.is_finished() {
            drop(window);
            return thread.join().map_err(|_| "the physics thread panicked".to_string())?.map(|_| ());
        }
        std::thread::sleep(Duration::from_millis(2));
    };
    let (width, height) = window.client_size();
    let mut renderer = DebugRenderer::new(width, height)?;
    let chain = renderer.swap_chain(window.handle)?;
    println!(
        "window {width} x {height}, {} samples per pixel, {}{}",
        renderer.samples(),
        renderer.adapter,
        if renderer.software { " (software rasteriser)" } else { "" }
    );
    let mut camera = DrivingCamera::new(if options.camera == "cockpit" { CameraMode::Cockpit } else { CameraMode::Chase });
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
                Event::Key { key, ctrl, shift } => {
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
                    } else if !ctrl {
                        match key {
                            // C: camera, P or Pause: pause, R: reset (Shift+R: a new car)
                            0x43 => camera.mode = camera.mode.next(),
                            0x50 | 0x13 => {
                                shared.paused.fetch_xor(true, Ordering::Relaxed);
                            }
                            0x52 => request(if shift { event::REBUILD } else { event::RESET }),
                            _ => {}
                        }
                    }
                }
            }
        }
        let toggles = shared.camera_toggles.load(Ordering::Relaxed);
        if toggles != camera_toggles {
            camera_toggles = toggles;
            camera.mode = camera.mode.next();
        }
        if thread.is_finished() || options.duration.is_some_and(|d| started.elapsed().as_secs_f64() >= d) {
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
            camera: camera.mode.name(),
            replay: replay.is_some(),
            notes: Vec::new(),
        };
        if replay_over {
            info.notes.push("the replay is over (Esc)".to_string());
        } else if unfocused {
            info.notes.push("click the window to drive".to_string());
        }
        renderer.draw(&view, &shape, &frame, &info);
        if let Err(message) = renderer.present(&chain, options.vsync) {
            result = Err(message);
            break;
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
    let (view, timing) = outcome?;
    report(&view, &timing);
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
    let source: Box<dyn DriverSource> = if steps.is_some() { Box::new(ReplaySource::default()) } else { Box::new(SpawnSequence::new(NobodySource, true)) };
    let mut sim = GameSim::new(setup, source)?;
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
                sim.step()?;
            }
        }
    }
    let view = CarView::capture(&sim, sim.steps.saturating_sub(drive_start) as f64 * 0.003);
    let shape = CarShape::of(&sim.car_info());
    let mut renderer = DebugRenderer::new(options.width, options.height)?;
    let mode = if options.camera == "cockpit" { CameraMode::Cockpit } else { CameraMode::Chase };
    let mut camera = DrivingCamera::new(mode);
    // two frames, so that the chase camera has leaned into the car's acceleration
    camera.update(&previous, &shape, previous.acc_g, 1.0 / 60.0);
    let frame = camera.update(&view, &shape, view.acc_g, 1.0);
    let info = HudInfo { fps: 0.0, camera: mode.name(), replay: steps.is_some(), ..HudInfo::default() };
    renderer.draw(&view, &shape, &frame, &info);
    let pixels = renderer.read_pixels()?;
    let (width, height) = renderer.size();
    write_png(path, width, height, &pixels)?;
    println!(
        "{}: {width} x {height}, {} samples per pixel, drawn by {}{} after {} steps ({:.1} km/h, gear {})",
        path.display(),
        renderer.samples(),
        renderer.adapter,
        if renderer.software { " (software rasteriser)" } else { "" },
        sim.steps,
        view.speed_kmh,
        view.gear - 1
    );
    Ok(())
}

fn run(options: &Options) -> Result<(), String> {
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
    if let Some(path) = &options.screenshot {
        return run_screenshot(options, path);
    }
    if options.headless {
        if let (Some(replay), false) = (&options.replay, options.realtime) {
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

fn main() {
    let options = match Options::parse(std::env::args().skip(1)) {
        Ok(options) => options,
        Err(message) => {
            eprintln!("{message}");
            std::process::exit(2);
        }
    };
    if let Err(message) = run(&options) {
        eprintln!("rustyac: {message}");
        std::process::exit(1);
    }
}
