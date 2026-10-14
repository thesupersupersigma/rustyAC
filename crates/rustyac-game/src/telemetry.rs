// SPDX-License-Identifier: GPL-3.0-or-later

//! AC's comparative telemetry in the game: the port of `ComparativeTelemetryRecorder`
//! (`rustyac_physics::replay::telemetry`) fed from the player's car after every physics step,
//! and the best lap's file (`.tc`).
//!
//! The game keeps the file in `Documents\Assetto Corsa\ctelemetry\player\`; rustyAC keeps its
//! own in `ctelemetry\player\` beside the program and never writes into the game's folders.
//!
//! One thing is done at a fixed pace here that the game does at the picture's pace: the game
//! takes the samples out of their queue once a picture, rustyAC after every physics step, so
//! that the recorded lap does not depend on how fast the pictures come. (In the game a sample
//! that waits in the queue across the finish line is filed under the new lap.)
//!
//! rustyAC has no display for these laps yet (the game's is an app); the recorder and its
//! file are here for it.

use std::path::{Path, PathBuf};

use rustyac_physics::replay::telemetry::{ComparativeTelemetryRecorder, TelemetryNames};

use crate::sim::GameSim;

/// The telemetry recorder with the things of the game around it.
pub struct TelemetryRig {
    pub recorder: Option<ComparativeTelemetryRecorder>,
    pub folder: PathBuf,
    /// `splineLocatorData.npos` as the last step left it: the game's sample is taken before
    /// the step's own value is copied there
    npos: f32,
    /// Lines for the console, taken by the caller.
    pub messages: Vec<String>,
}

impl TelemetryRig {
    /// `folder`: where the files are kept (rustyAC's own `ctelemetry` folder).
    pub fn new(folder: PathBuf) -> TelemetryRig {
        TelemetryRig { recorder: None, folder, npos: 0.0, messages: Vec::new() }
    }

    /// The constructor, once the session is known: the names and `loadBest`.
    fn start(&mut self, sim: &GameSim) {
        let driver = sim.setup.referee.as_ref().and_then(|referee| referee.driver_names.first().cloned()).filter(|name| !name.is_empty()).unwrap_or_else(|| "Player".to_string());
        let folder_name = |text: &str| Path::new(text).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| text.to_string());
        let names = TelemetryNames { driver, track: folder_name(&sim.setup.track), car: folder_name(&sim.setup.car), config: sim.setup.layout.clone() };
        let mut recorder = ComparativeTelemetryRecorder::new(names);
        let file = self.folder.join(ComparativeTelemetryRecorder::file_name(&recorder.names));
        if let Ok(bytes) = std::fs::read(&file) {
            if recorder.load_bytes(&bytes) {
                self.messages.push(format!("telemetry: the best lap of {} ms from {}", recorder.best_time, file.display()));
            }
        }
        self.recorder = Some(recorder);
    }

    /// After a physics step: the lap's handler for the laps this step finished, the step's
    /// sample, and the sample into its slot.
    pub fn after_step(&mut self, sim: &GameSim) {
        if self.recorder.is_none() {
            self.start(sim);
        }
        let Some(recorder) = &mut self.recorder else { return };
        for lap in &sim.step_laps {
            recorder.on_lap_completed(lap.lap_time, lap.lap_count);
        }
        let car = &sim.car.car;
        let gear = car.drivetrain.as_ref().map_or(1, |drivetrain| drivetrain.base().current_gear);
        recorder.on_step_complete(gear, self.npos, car.speed, car.controls.gas, car.controls.brake);
        self.npos = car.spline_locator_data.npos;
        recorder.update(car.transponder.lap_count);
    }

    /// `ComparativeTelemetryRecorder::~ComparativeTelemetryRecorder` 0x140073520: `saveBest`,
    /// the one place the file is written.
    pub fn shutdown(&mut self) {
        let Some(recorder) = &mut self.recorder else { return };
        let Some(bytes) = recorder.save_bytes() else { return };
        recorder.need_to_save = false;
        let file = self.folder.join(ComparativeTelemetryRecorder::file_name(&recorder.names));
        let written = file.parent().map_or(Ok(()), std::fs::create_dir_all).and_then(|()| std::fs::write(&file, bytes));
        match written {
            Ok(()) => self.messages.push(format!("telemetry: {} (the best lap, {} ms)", file.display(), recorder.best_time)),
            Err(e) => self.messages.push(format!("WARNING: the telemetry was not saved: {}: {e}", file.display())),
        }
    }
}
