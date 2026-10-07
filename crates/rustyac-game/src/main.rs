//! `rustyac.exe`: see `docs/game/first_drive.md`.

use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use rustyac_game::cli::Options;
use rustyac_game::input_file::{InputFile, InputWriter, SimSetup};
use rustyac_game::physics_thread::{self, LoopConfig, Recorder, Shared, StepSink};
use rustyac_game::shm::{SharedMemory, ShmSink};
use rustyac_game::sim::{DriverSource, GameSim, NobodySource, ReplaySource, SpawnSequence};

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

/// `--headless` without a window: the car runs in real time, with a recorded drive or with
/// nobody at the controls, until `--duration` is over or the drive ends.
fn run_headless(options: &Options) -> Result<(), String> {
    let (setup, source, replay): (SimSetup, Box<dyn DriverSource>, _) = match &options.replay {
        Some(path) => {
            let file = InputFile::read(path)?;
            println!("replaying {} ({} steps, {:.1} s) in real time", path.display(), file.steps.len(), file.steps.len() as f64 * 0.003);
            (file.setup, Box::new(ReplaySource::default()), Some(file.steps))
        }
        None => (live_setup(options), Box::new(SpawnSequence::new(NobodySource, true)), None),
    };
    let sinks = sinks(options, &setup)?;
    let shared = Shared::new();
    let config = LoopConfig {
        replay,
        duration: options.duration.map(Duration::from_secs_f64),
        max_steps: None,
        pause_unfocused: false,
        auto_reset: true,
    };
    if options.duration.is_none() && options.replay.is_none() {
        println!("no --duration: running until Ctrl+C");
    }
    let thread_shared = Arc::clone(&shared);
    let thread = std::thread::Builder::new()
        .name("physics".to_string())
        .spawn(move || {
            // the car lives on the physics thread from its first moment (it is not `Send`)
            let outcome = GameSim::new(setup, source).and_then(|sim| physics_thread::run(sim, &thread_shared, sinks, config));
            thread_shared.finished.store(true, Ordering::Relaxed);
            outcome.map(|(sim, timing)| (rustyac_game::view::CarView::capture(&sim, 0.0), timing))
        })
        .map_err(|e| e.to_string())?;
    // a replay ends by itself; so does a run with a duration
    while !shared.finished.load(Ordering::Relaxed) {
        std::thread::sleep(Duration::from_millis(20));
    }
    shared.quit.store(true, Ordering::Relaxed);
    let (view, timing) = thread.join().map_err(|_| "the physics thread panicked".to_string())??;
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
    Ok(())
}

fn run(options: &Options) -> Result<(), String> {
    if options.headless {
        if let (Some(replay), None) = (&options.replay, &options.screenshot) {
            if !options.realtime {
                // as fast as it goes
                let started = std::time::Instant::now();
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
        }
        return run_headless(options);
    }
    Err("the window is not built yet: use --headless".to_string())
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
