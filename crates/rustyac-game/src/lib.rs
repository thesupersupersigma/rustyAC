//! rustyAC, the game: everything around the physics car that makes it drivable.
//!
//! * [`sim`]: one [`VanillaCar`](rustyac_physics::car::VanillaCar) on the endless flat road,
//!   built, spawned and stepped. The only place the game touches the car.
//! * [`input_file`], [`dump`]: the files of `--record` / `--replay` / `--dump-states`.
//! * [`cli`]: the command line.
//!
//! The physics is `rustyac-physics` and nothing else: this crate feeds it the driver's controls
//! and the session's values and reads its state.

pub mod cli;
pub mod crt;
pub mod dump;
pub mod input;
pub mod input_file;
pub mod physics_thread;
pub mod shm;
pub mod sim;
pub mod timer;
pub mod view;

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
