// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! The wheel guns of the pit crew: `PitCrew::PitCrew` 0x1400a0b80 makes two events for each
//! of the four mechanics (`event:/common/screw`, then `event:/common/unscrew`, with no reverb),
//! and `PitCrew::updateCurrentPosition` 0x1400a4500 starts one at its wheel whenever it is not
//! playing while the mechanic unscrews or screws, and stops it when he is done. The timeline
//! that decides when is the picture's (`rustyac_render::tyre_crew`); this is the sound's end.

use crate::engine::{AudioEngine, EventId, Mat, REVERB_OFF};

/// The eight events, by wheel (0 LF, 1 RF, 2 LR, 3 RR).
pub struct CrewGuns {
    pub screw: [EventId; 4],
    pub unscrew: [EventId; 4],
}

impl CrewGuns {
    /// The events in the order the constructor makes them: each mechanic's screw event, then
    /// his unscrew event.
    pub fn new(engine: &mut AudioEngine) -> CrewGuns {
        let mut screw = [0; 4];
        let mut unscrew = [0; 4];
        for k in 0..4 {
            screw[k] = engine.create_event("event:/common/screw", REVERB_OFF);
            unscrew[k] = engine.create_event("event:/common/unscrew", REVERB_OFF);
        }
        CrewGuns { screw, unscrew }
    }

    fn event(&self, wheel: usize, screw: bool) -> EventId {
        if screw {
            self.screw[wheel]
        } else {
            self.unscrew[wheel]
        }
    }

    /// `AudioEvent::isPlaying` 0x1401fa170.
    pub fn is_playing(&self, engine: &AudioEngine, wheel: usize, screw: bool) -> bool {
        engine.event_is_playing(self.event(wheel, screw))
    }

    /// `AudioEvent::set3DAttributes` 0x1401fb370 at the wheel's matrix with no velocity, then
    /// `AudioEvent::start` 0x1401fbf40.
    pub fn start(&self, engine: &AudioEngine, wheel: usize, screw: bool, matrix: &Mat) {
        let e = self.event(wheel, screw);
        engine.event_set_3d(e, matrix, &[0.0, 0.0, 0.0]);
        engine.event_start(e);
    }

    /// `AudioEvent::stop` 0x1401fc040.
    pub fn stop(&self, engine: &AudioEngine, wheel: usize, screw: bool) {
        engine.event_stop(self.event(wheel, screw));
    }
}
