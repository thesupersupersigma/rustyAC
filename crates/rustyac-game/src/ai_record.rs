// SPDX-License-Identifier: GPL-3.0-or-later

//! AC's AI line recorder in the game (`--ai-record`): the port of `AISplineRecorder`'s
//! recording (`rustyac_physics::track::ai_line`) on the player's car, as FormAI's "start
//! recording" leaves it. The game steps its recorder in `Track::step`, before the cars step:
//! it sees the car as the previous step left it. Here that is the state kept from the
//! previous step's end; the step's finished laps follow.
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

/// The player's car as the recorder sees it: GUID, body position, speed, gas, brake.
type Seen = (u32, [f32; 3], f32, f32, f32);

pub struct AiRecordSink {
    folder: PathBuf,
    recorder: Option<(Arc<Track>, AiSplineRecorder)>,
    seen: Option<Seen>,
}

impl AiRecordSink {
    pub fn new(folder: PathBuf) -> AiRecordSink {
        AiRecordSink { folder, recorder: None, seen: None }
    }
}

impl StepSink for AiRecordSink {
    fn after_step(&mut self, sim: &GameSim, _input: &StepInput) -> Result<(), String> {
        if self.recorder.is_none() {
            let Some(track) = sim.track.clone() else { return Ok(()) };
            let mut recorder = track.ai().cloned().unwrap_or_else(|| AiSplineRecorder { is_active: true, recording_step: 1.5, pit_lane_spline_attach_point: -1.0, ..AiSplineRecorder::default() });
            recorder.start_recording();
            self.recorder = Some((track, recorder));
        }
        let Some((_, recorder)) = &mut self.recorder else { return Ok(()) };
        if let Some((guid, pos, speed, gas, brake)) = self.seen {
            recorder.step(guid, pos, speed, gas, brake);
        }
        for lap in &sim.step_laps {
            recorder.on_lap_completed(lap.car_index, lap.lap_time);
        }
        let car = &sim.car.car;
        let p = car.core.get_position(car.body);
        self.seen = Some((0, [p.x, p.y, p.z], car.speed, car.controls.gas, car.controls.brake));
        Ok(())
    }

    /// `AISplineRecorder::~AISplineRecorder` @ 0x140293d20: the save, into rustyAC's folder.
    fn finish(&mut self) -> Result<(), String> {
        let Some((track, mut recorder)) = self.recorder.take() else { return Ok(()) };
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
