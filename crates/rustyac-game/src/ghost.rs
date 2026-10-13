// SPDX-License-Identifier: GPL-3.0-or-later

//! AC's ghost car in the game: the port of `GhostCarRecorder`
//! (`rustyac_physics::replay::ghost`) fed from the player's car after every physics step, as
//! the game's is from `Car::evOnStepComplete` and `Car::evOnLapCompleted`, and its file.
//!
//! The game keeps the files in `Documents\Assetto Corsa\GhostCar\<track>\`; rustyAC keeps its
//! own in `GhostCar\<track>\` beside the program and never writes into the game's folders. A
//! file of the game can be played by naming it (`--ghost-file`, or race.ini's `FILE`).
//!
//! As in the game: the player's car only, never online; the ghost is the best valid lap
//! without a cut; the file is written when the program ends, and read at the start only when
//! `LOAD` asks for it.

use std::path::{Path, PathBuf};

use rustyac_physics::replay::ghost::{GhostCarFrame, GhostCarRecorder, GhostNames, GhostTransponder};
use rustyac_physics::vecmath::Mat44f;

use crate::sim::GameSim;
use crate::view::CarView;

/// race.ini's `[GHOST_CAR]` section (`RaceManager::initGhostcar` 0x14013a210 and
/// `GhostCarRecorder::loadGhostCarRecorderCustomParameters` 0x140108880).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GhostOptions {
    /// `RECORDING`
    pub recording: bool,
    /// `PLAYING`
    pub playing: bool,
    /// `LOAD`: the ghost of an earlier run is read at the start
    pub load: bool,
    /// `FILE`: that file first (a full path); the folder's own file when it does not fit
    pub file: Option<PathBuf>,
    /// `SECONDS_ADVANTAGE`: the ghost runs this much ahead
    pub seconds_advantage: f32,
}

impl GhostOptions {
    /// A ghost car is wanted at all.
    pub fn wanted(&self) -> bool {
        self.recording || self.playing
    }
}

/// The ghost car's recorder with the things of the game around it.
pub struct GhostRig {
    pub recorder: GhostCarRecorder,
    options: GhostOptions,
    folder: PathBuf,
    names: Option<GhostNames>,
    new_sessions: u32,
    /// Lines for the console (the game's messages), taken by the caller.
    pub messages: Vec<String>,
}

impl GhostRig {
    /// `folder`: where the ghost files are kept (rustyAC's own `GhostCar` folder).
    pub fn new(folder: PathBuf, options: GhostOptions) -> GhostRig {
        // (the driver's name comes with the first step, when the session is known)
        let recorder = GhostCarRecorder::new("", options.recording, options.playing);
        GhostRig { recorder, options, folder, names: None, new_sessions: 0, messages: Vec::new() }
    }

    /// The session's names as the game's recorder takes them from the avatar and the track.
    fn names_of(sim: &GameSim) -> GhostNames {
        let driver = sim.setup.referee.as_ref().and_then(|referee| referee.driver_names.first().cloned()).filter(|name| !name.is_empty()).unwrap_or_else(|| "Player".to_string());
        let folder_name = |text: &str| Path::new(text).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| text.to_string());
        GhostNames { driver, track: folder_name(&sim.setup.track), config: sim.setup.layout.clone(), car: folder_name(&sim.setup.car) }
    }

    /// The constructor's end, once the session is known: the driver's name and
    /// `loadGhostCarRecorderCustomParameters`.
    fn start(&mut self, sim: &GameSim) {
        let names = GhostRig::names_of(sim);
        self.recorder.names.driver = names.driver.clone();
        self.new_sessions = sim.new_sessions;
        if self.options.load {
            let own = self.folder.join(GhostCarRecorder::file_name(&names));
            let mut loaded = false;
            if let Some(file) = self.options.file.clone() {
                loaded = self.load(&file, &names);
            }
            if !loaded {
                loaded = self.load(&own, &names);
            }
            if loaded {
                self.messages.push(format!("ghost car: a lap of {} ms by {} ({} frames)", self.recorder.ghost_car_lap, self.recorder.names.driver, self.recorder.playing_buffer.len()));
            }
        }
        self.recorder.set_seconds_advantage(self.options.seconds_advantage);
        self.recorder.current_frame = 0;
        self.names = Some(names);
    }

    /// `GhostCarRecorder::loadGhostCarRecording` 0x140108c10 on a file.
    fn load(&mut self, file: &Path, names: &GhostNames) -> bool {
        match std::fs::read(file) {
            Ok(bytes) => self.recorder.load_bytes(&bytes, names),
            Err(_) => {
                self.messages.push("ERROR: GHOST CAR :: couldn't open File".to_string());
                false
            }
        }
    }

    /// After a physics step: `Car::evOnLapCompleted`'s handler for the laps this step
    /// finished, then `Car::evOnStepComplete`'s.
    pub fn after_step(&mut self, sim: &GameSim) {
        if self.names.is_none() {
            self.start(sim);
        }
        if sim.new_sessions != self.new_sessions {
            self.new_sessions = sim.new_sessions;
            self.recorder.on_new_session();
        }
        let Some(names) = &self.names else { return };
        let tp = &sim.car.car.transponder;
        let mut t = GhostTransponder { t: tp.t, last_lap: tp.last_lap, lap_count: tp.lap_count, finish_line_passed: tp.finish_line_passed, was_last_lap_valid: tp.was_last_lap_valid, cuts: tp.cuts };
        for lap in &sim.step_laps {
            // (the handler runs inside the transponder's step: the cuts are the finished lap's)
            let at_event = GhostTransponder { cuts: lap.cuts, ..t };
            if self.recorder.on_lap_completed(&at_event, names) {
                self.messages.push(format!("New Ghost Car best: {}", lap_text(self.recorder.ghost_car_lap)));
            }
        }
        t.cuts = tp.cuts;
        self.recorder.on_step_complete(sim.clock_ms(), &t, &mut || {
            let state = CarView::capture(sim, sim.sim_seconds()).physics_state();
            let m = |m: &Mat44f| *m;
            GhostCarFrame { body: m(&state.world_matrix), wheels: state.tyre_matrix, suspensions: state.suspension_matrix }
        });
    }

    /// `GhostCarRecorder::~GhostCarRecorder` 0x140106de0: the one place the file is written.
    pub fn shutdown(&mut self) {
        if !self.recorder.need_to_save {
            return;
        }
        self.recorder.need_to_save = false;
        let file = self.folder.join(GhostCarRecorder::file_name(&self.recorder.names));
        let written = file.parent().map_or(Ok(()), std::fs::create_dir_all).and_then(|()| std::fs::write(&file, self.recorder.save_bytes()));
        match written {
            Ok(()) => self.messages.push(format!("ghost car: {} ({} ms, {} frames)", file.display(), self.recorder.ghost_car_lap, self.recorder.playing_buffer.len())),
            Err(e) => self.messages.push(format!("WARNING: the ghost car was not saved: {}: {e}", file.display())),
        }
    }
}

/// A lap time as the game's message has it (`timeToString` with thousandths).
fn lap_text(ms: u32) -> String {
    format!("{}:{:02}.{:03}", ms / 60_000, ms / 1000 % 60, ms % 1000)
}
