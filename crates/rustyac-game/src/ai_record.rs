// SPDX-License-Identifier: GPL-3.0-or-later

//! AC's AI line recorder in the game (`--ai-record`): the port of `AISplineRecorder`'s
//! recording (`rustyac_physics::track::ai_line`) on the player's car, as FormAI's "start
//! recording" leaves it. The engine records ([`rustyac_physics::engine::PhysicsEngine::
//! start_ai_recording`], `SimSetup::ai_record`): every lap the car finishes is the AI line of
//! every car from then on. This sink saves the lines at the end.
//!
//! The game writes `ai/fast_lane.ai.candidate` (and `pit_lane.ai.candidate`) into the track's
//! own folder when the track goes; rustyAC writes them into the folder `--ai-record` names.

use std::path::PathBuf;
use std::sync::Arc;

use rustyac_physics::track::ai_line::AiSplineRecorder;
use rustyac_physics::track::Track;

use crate::input_file::StepInput;
use crate::physics_thread::StepSink;
use crate::sim::GameSim;

pub struct AiRecordSink {
    folder: PathBuf,
    track: Option<Arc<Track>>,
}

impl AiRecordSink {
    pub fn new(folder: PathBuf) -> AiRecordSink {
        AiRecordSink { folder, track: None }
    }
}

impl StepSink for AiRecordSink {
    fn after_step(&mut self, sim: &GameSim, _input: &StepInput) -> Result<(), String> {
        if self.track.is_none() {
            self.track = sim.track.clone();
        }
        Ok(())
    }

    /// `AISplineRecorder::~AISplineRecorder` @ 0x140293d20: the save, into rustyAC's folder.
    fn finish(&mut self) -> Result<(), String> {
        let Some(track) = self.track.take() else { return Ok(()) };
        // the lines the recording left (the track's own until a lap was finished)
        let mut recorder = track.ai().cloned().unwrap_or_else(|| AiSplineRecorder { is_active: true, recording_step: 1.5, pit_lane_spline_attach_point: -1.0, ..AiSplineRecorder::default() });
        recorder.start_recording();
        let files = recorder.save(&track, &track.data_folder.join("data"))?;
        if files.is_empty() {
            println!("AI line: nothing recorded");
        }
        for (name, bytes) in files {
            let path = self.folder.join(name);
            match rustyac_content::guard::create_dir_all(&self.folder).and_then(|()| rustyac_content::guard::write(&path, &bytes)) {
                Ok(()) => println!("AI line: {} ({} bytes; the game would write it into the track's ai folder)", path.display(), bytes.len()),
                Err(e) => eprintln!("WARNING: the AI line was not saved: {}: {e}", path.display()),
            }
        }
        Ok(())
    }
}
