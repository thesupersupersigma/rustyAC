// SPDX-License-Identifier: GPL-3.0-or-later

//! rustyAC, the game: everything around the physics car that makes it drivable.
//!
//! * [`sim`]: one [`VanillaCar`](rustyac_physics::car::VanillaCar) on the endless flat road,
//!   built, spawned and stepped. The only place the game touches the car.
//! * [`input_file`], [`dump`]: the files of `--record` / `--replay` / `--dump-states`.
//! * [`cli`]: the command line.
//! * [`audio`]: the sound (AC's own, through `rustyac-audio`).
//! * [`conditions`]: the session of a live drive from the game's own race.ini and assists.ini.
//!
//! The physics is `rustyac-physics` and nothing else: this crate feeds it the driver's controls
//! and the session's values and reads its state.

// The window, Direct3D, the devices, FMOD, the physics thread and the shared memory are
// Windows. What is left on another target (wasm: the browser build `rustyac-web`, and the
// replay check under WASI) is the simulation, the replay files, AC's keyboard and pad classes
// and the scene maths.
#[cfg(windows)]
pub mod audio;
pub mod ac_replay;
pub mod ghost;
pub mod telemetry;
pub mod autodrive;
#[cfg(windows)]
pub mod cli;
#[cfg(windows)]
pub mod conditions;
pub mod crt;
pub mod dump;
pub mod focus;
#[cfg(windows)]
pub mod gateway;
#[cfg(windows)]
pub mod identity;
pub mod input;
pub mod input_file;
#[cfg(windows)]
pub mod join;
pub mod online;
#[cfg(windows)]
pub mod mouse_hider;
#[cfg(windows)]
pub mod physics_thread;
pub mod referee;
#[cfg(windows)]
pub mod render;
pub mod replay_manager;
/// The part of the debug view that is plain maths and file parsing (no Direct3D).
#[cfg(not(windows))]
pub mod render {
    pub mod dds;
    pub mod scene;
}
#[cfg(windows)]
pub mod shm;
pub mod sim;
#[cfg(windows)]
pub mod timer;
pub mod view;
#[cfg(windows)]
pub mod window;

use std::path::Path;

/// `--replay <file> --headless`: runs a recorded drive as fast as it goes, optionally writing
/// the car's state after every step. Returns the number of steps.
///
/// `state_hash`: also print a hash over every car's rigid-body state of every step and the
/// time a step took (stderr). `physics_threads`: the worker threads of a drive with several
/// cars (`None`: as the game decides).
pub fn run_replay_headless(replay: &Path, dump_states: Option<&Path>, state_hash: bool, physics_threads: Option<usize>) -> Result<u64, String> {
    run_replay_headless_saving(replay, dump_states, state_hash, physics_threads, None, None, None)
}

/// [`run_replay_headless`] that also records AC's replay of the drive (`ReplayManager`, at
/// its default 33 frames a second) and writes it to `save_replay` at the end, and that runs
/// AC's ghost car recorder and its telemetry recorder beside the drive (`ghost`, `telemetry`;
/// their files are written at the end).
pub fn run_replay_headless_saving(replay: &Path, dump_states: Option<&Path>, state_hash: bool, physics_threads: Option<usize>, save_replay: Option<&Path>, mut ghost: Option<&mut ghost::GhostRig>, mut telemetry: Option<&mut telemetry::TelemetryRig>) -> Result<u64, String> {
    let mut rig = save_replay.map(|path| ac_replay::ReplayRig::new(path.parent().map(Path::to_path_buf).unwrap_or_default(), None, -16.0, 1));
    let file = input_file::InputFile::read(replay)?;
    let mut setup = file.setup.clone();
    setup.physics_threads = physics_threads;
    let mut sim = sim::GameSim::new(setup, Box::new(sim::ReplaySource::default()))?;
    let mut writer = match dump_states {
        Some(path) => Some(dump::DumpWriter::create(path)?),
        None => None,
    };
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    #[cfg(not(target_arch = "wasm32"))]
    let started = std::time::Instant::now();
    for step in &file.steps {
        sim.step_recorded(step)?;
        if let Some(rig) = &mut rig {
            rig.after_step(&sim);
        }
        if let Some(ghost) = &mut ghost {
            ghost.after_step(&sim);
        }
        if let Some(telemetry) = &mut telemetry {
            telemetry.after_step(&sim);
        }
        if let Some(writer) = &mut writer {
            writer.push(&dump::StepDump::capture(&sim.car.car))?;
        }
        if state_hash {
            hash = sim.state_hash(hash);
        }
    }
    if let Some(writer) = writer {
        writer.finish()?;
    }
    if let (Some(rig), Some(path)) = (&mut rig, save_replay) {
        let bytes = rig.save_to(path)?;
        eprintln!("replay: {} ({bytes} bytes, {} frames)", path.display(), rig.manager.as_ref().map_or(0, |m| m.recorded_frames));
    }
    if let Some(ghost) = &mut ghost {
        ghost.shutdown();
        for line in ghost.messages.drain(..) {
            eprintln!("{line}");
        }
    }
    if let Some(telemetry) = &mut telemetry {
        telemetry.shutdown();
        for line in telemetry.messages.drain(..) {
            eprintln!("{line}");
        }
    }
    // Task 28: the referee's files of a replay are not written, their hashes are said
    if let Some(lines) = referee::finish(&mut sim, None) {
        for line in lines {
            eprintln!("{line}");
        }
    }
    if state_hash {
        #[cfg(not(target_arch = "wasm32"))]
        let per_step = started.elapsed().as_secs_f64() * 1000.0 / sim.steps.max(1) as f64;
        #[cfg(target_arch = "wasm32")]
        let per_step = 0.0;
        eprintln!(
            "state hash {hash:016x}: {} cars, {} steps, {} worker threads, {per_step:.4} ms a step ({:.0} % of the 3 ms a step lasts)",
            1 + sim.opponents.len(),
            sim.steps,
            sim.engine.as_ref().map_or(0, |engine| engine.threads()),
            per_step / 3.0 * 100.0
        );
        if let Some(engine) = &sim.engine {
            let ms = |seconds: f64| seconds * 1000.0 / sim.steps.max(1) as f64;
            eprintln!(
                "  of a step: the cars {:.4} ms, the collision pass {:.4} ms, dWorldStep {:.4} ms, the rest {:.4} ms",
                ms(engine.seconds[0]),
                ms(engine.seconds[1]),
                ms(engine.seconds[2]),
                ms(engine.seconds[3])
            );
        }
    }
    Ok(sim.steps)
}
