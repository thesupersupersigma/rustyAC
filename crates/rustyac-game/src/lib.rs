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
pub mod autodrive;
#[cfg(windows)]
pub mod cli;
#[cfg(windows)]
pub mod conditions;
pub mod crt;
pub mod dump;
pub mod input;
pub mod input_file;
#[cfg(windows)]
pub mod physics_thread;
#[cfg(windows)]
pub mod render;
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
pub fn run_replay_headless(replay: &Path, dump_states: Option<&Path>) -> Result<u64, String> {
    let file = input_file::InputFile::read(replay)?;
    let mut sim = sim::GameSim::new(file.setup.clone(), Box::new(sim::ReplaySource::default()))?;
    let mut writer = match dump_states {
        Some(path) => Some(dump::DumpWriter::create(path)?),
        None => None,
    };
    for step in &file.steps {
        sim.step_recorded(step)?;
        if let Some(writer) = &mut writer {
            writer.push(&dump::StepDump::capture(&sim.car.car))?;
        }
    }
    if let Some(writer) = writer {
        writer.finish()?;
    }
    Ok(sim.steps)
}
