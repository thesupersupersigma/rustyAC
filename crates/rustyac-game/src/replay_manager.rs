// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! The replay's transport: the state `ReplayManager` keeps between the keys and the frames.
//! `play` 0x1401591f0, `pause` 0x140159070, `stop` 0x14015b6f0, `fastForward` 0x140156780,
//! `slowMotion` 0x14015b3b0, and the speed's part of `updatePlayState` 0x14015b9a0.
//!
//! The game plays recorded frames (one every 30 ms); a rustyAC replay is the drive simulated
//! again from its recorded inputs, so the speed here paces the physics. What needs frames to
//! go back over (rewind, the lap jumps, an instant replay while driving) is not here.

// The comparisons and limits are written as the machine code has them (what a NaN does).
#![allow(clippy::neg_cmp_op_on_partial_ord, clippy::manual_clamp)]

/// `ReplayManager::status`.
pub const PLAY: i32 = 0;
pub const PAUSE: i32 = 1;
pub const STOP: i32 = 2;
pub const FAST_FORWARD: i32 = 4;
pub const SLOW_MOTION: i32 = 5;

/// What `Sim::evOnReplayStatusChanged` carries.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StatusEvent {
    pub status: i32,
    pub time_mult: f32,
    pub slow_mo_level: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ReplayManager {
    pub status: i32,
    pub time_mult: f32,
    /// seconds since slow motion was asked for: the speed comes down over 0.4 s
    pub slow_mo_counter: f32,
    /// the game's "SlowMotion Level" spinner (2 to 12): the speed is one over it
    pub slow_mo_level: f32,
    /// the events since they were last taken, in order
    pub events: Vec<StatusEvent>,
}

impl Default for ReplayManager {
    fn default() -> ReplayManager {
        ReplayManager::new()
    }
}

impl ReplayManager {
    /// As `ReplayManager::startReplayMode` leaves it: playing.
    pub fn new() -> ReplayManager {
        ReplayManager { status: PLAY, time_mult: 1.0, slow_mo_counter: 0.0, slow_mo_level: 2.0, events: Vec::new() }
    }

    fn event(&mut self, time_mult: f32, slow_mo_level: f32) {
        self.events.push(StatusEvent { status: self.status, time_mult, slow_mo_level });
    }

    /// `ReplayManager::play` 0x1401591f0.
    pub fn play(&mut self) {
        self.time_mult = 1.0;
        self.status = PLAY;
        self.event(1.0, self.slow_mo_level);
    }

    /// `ReplayManager::pause` 0x140159070.
    pub fn pause(&mut self) {
        self.status = PAUSE;
        self.time_mult = 0.0;
        self.event(0.0, self.slow_mo_level);
    }

    /// `ReplayManager::stop` 0x14015b6f0 (the caller goes back to the first frame).
    pub fn stop(&mut self) {
        self.time_mult = 0.0;
        self.status = STOP;
        self.event(0.0, self.slow_mo_level);
    }

    /// `ReplayManager::fastForward(1, false)` 0x140156780: one faster, from 2 up; the event
    /// says a slow-motion level of 1.
    pub fn fast_forward(&mut self) {
        if !(1.0 < self.time_mult) {
            self.time_mult = 1.0;
        }
        self.status = FAST_FORWARD;
        let v = 1.0 + self.time_mult;
        self.time_mult = if v > 20.0 {
            20.0
        } else if v < 0.0 {
            0.0
        } else {
            v
        };
        self.event(self.time_mult, 1.0);
    }

    /// `ReplayManager::slowMotion` 0x14015b3b0; the event says a speed of 1.
    pub fn slow_motion(&mut self, level: f32) {
        self.slow_mo_level = level;
        if self.status == SLOW_MOTION {
            self.time_mult = 1.0 / level;
        } else {
            self.slow_mo_counter = 0.0;
        }
        self.status = SLOW_MOTION;
        self.event(1.0, level);
    }

    /// The key of `[SLOWMO]` (`Sim::onKeyDown` 0x14019cf43): slow motion on, or back to play.
    pub fn key_slow_motion(&mut self) {
        if self.status == SLOW_MOTION {
            self.play();
        } else {
            self.slow_motion(self.slow_mo_level);
        }
    }

    /// The key of `[PAUSE_REPLAY]` (0x14019cfc3): from playing it pauses, from anything else
    /// it plays.
    pub fn key_pause(&mut self) {
        if self.status != PLAY {
            self.play();
        } else {
            self.pause();
        }
    }

    /// The speed's part of `ReplayManager::updatePlayState` 0x14015b9a0, once a frame before
    /// the cars update: the slow motion's ramp and the limit of five times. The answer is the
    /// speed everything reads this frame.
    pub fn update_play_state(&mut self, dt: f32) -> f32 {
        if !(0.0 < self.slow_mo_level) {
            self.slow_mo_level = 1.0;
        }
        if self.status == SLOW_MOTION {
            let c = self.slow_mo_counter;
            if c >= f32::from_bits(0x3ecc_cccd) {
                self.time_mult = 1.0 / self.slow_mo_level;
            } else {
                let a = 1.0 - self.slow_mo_level;
                let mut b = c * 2.5;
                self.slow_mo_counter = c + dt;
                b *= a;
                self.time_mult = 1.0 / (1.0 - b);
            }
        }
        self.time_mult = if self.time_mult > 5.0 {
            5.0
        } else if self.time_mult < -5.0 {
            -5.0
        } else {
            self.time_mult
        };
        self.time_mult
    }

    /// `ReplayManager::updateReplayAudio` 0x14015bc00: whether the sound runs.
    pub fn audio_on(&self) -> bool {
        if self.status == SLOW_MOTION {
            2.0 >= self.slow_mo_level
        } else {
            0.5 <= self.time_mult && self.time_mult <= 2.0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fast_forward_climbs_from_two_and_is_read_at_five_at_most() {
        let mut rm = ReplayManager::new();
        let mut read = Vec::new();
        for _ in 0..6 {
            rm.fast_forward();
            read.push(rm.update_play_state(1.0 / 60.0));
        }
        assert_eq!(read, [2.0, 3.0, 4.0, 5.0, 5.0, 5.0]);
        assert_eq!(rm.events[0], StatusEvent { status: FAST_FORWARD, time_mult: 2.0, slow_mo_level: 1.0 });
        // without a frame between the presses the stored speed climbs to 20
        let mut rm = ReplayManager::new();
        for _ in 0..30 {
            rm.fast_forward();
        }
        assert_eq!(rm.time_mult, 20.0);
        assert_eq!(rm.update_play_state(0.0), 5.0);
    }

    #[test]
    fn slow_motion_ramps_down_over_four_tenths_of_a_second_and_toggles() {
        let mut rm = ReplayManager::new();
        rm.key_slow_motion();
        assert_eq!(rm.events[0], StatusEvent { status: SLOW_MOTION, time_mult: 1.0, slow_mo_level: 2.0 });
        // the first frame still plays at full speed: 1 / (1 - 0 * 2.5 * (1 - 2))
        assert_eq!(rm.update_play_state(0.1), 1.0);
        // 0.1 s in: 1 / (1 + 0.25)
        assert_eq!(rm.update_play_state(0.1), 1.0 / (1.0 - (0.1f32 * 2.5) * (1.0 - 2.0)));
        for _ in 0..10 {
            rm.update_play_state(0.1);
        }
        assert_eq!(rm.time_mult, 0.5);
        assert!(rm.audio_on());
        rm.key_slow_motion();
        assert_eq!((rm.status, rm.time_mult), (PLAY, 1.0));
    }

    #[test]
    fn the_pause_key_pauses_only_from_playing() {
        let mut rm = ReplayManager::new();
        rm.key_pause();
        assert_eq!((rm.status, rm.time_mult), (PAUSE, 0.0));
        assert!(!rm.audio_on());
        rm.key_pause();
        assert_eq!((rm.status, rm.time_mult), (PLAY, 1.0));
        rm.fast_forward();
        rm.fast_forward();
        assert!(!rm.audio_on());
        rm.key_pause();
        assert_eq!((rm.status, rm.time_mult), (PLAY, 1.0));
        rm.stop();
        assert_eq!(rm.events.last(), Some(&StatusEvent { status: STOP, time_mult: 0.0, slow_mo_level: 2.0 }));
    }
}
