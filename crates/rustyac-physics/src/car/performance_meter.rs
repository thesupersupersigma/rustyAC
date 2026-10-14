// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! `PerformanceMeter` (`Car+0x3560`, 0x70 bytes): the player's car's lap against the meter's
//! best lap, a sample of the lap timer and the speed about every thousandth of the lap by the
//! place on the spline, and the difference of the newest sample to the best lap's sample of
//! the same number (the "Performance Delta" app's delta and bars, `CarPhysicsState
//! performanceMeter` / `performanceMeterSpeedDiffMS`).
//!
//! It writes nothing but its own members: the car's physics never reads it in the game as
//! shipped (only the "recover time" penalty mode does, which nothing sets).

/// `PerformancePair`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PerformancePair {
    /// the lap timer (`TimeTransponder::t`, ms)
    pub t: u32,
    /// `Car::getSpeed`, m/s
    pub speed_ms: f32,
}

/// What `PerformanceMeter::step` reads of its car.
#[derive(Clone, Copy, Debug, Default)]
pub struct PerformanceMeterInput {
    /// `transponder.finishLinePassed`, `wasLastLapValid`, `lastLap`, `t`
    pub finish_line_passed: bool,
    pub was_last_lap_valid: bool,
    pub last_lap: u32,
    pub t: u32,
    /// `splineLocatorData.npos`
    pub npos: f32,
    /// `Car::getSpeed` (`valueCache.speed`, m/s)
    pub speed: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PerformanceMeter {
    pub is_enabled: bool,
    pub current_lap: Vec<PerformancePair>,
    pub best_lap: Vec<PerformancePair>,
    /// ms; -1: none
    pub best_lap_time: f64,
    pub last_lap_index: i32,
    pub current_distance: f64,
    pub last_recorded_distance: f64,
    /// seconds, + = slower
    pub current_diff: f64,
    /// m/s, + = faster
    pub current_speed_diff_ms: f32,
}

impl Default for PerformanceMeter {
    /// `Car::Car` 0x14026bf00's members before `init`.
    fn default() -> PerformanceMeter {
        PerformanceMeter { is_enabled: false, current_lap: Vec::new(), best_lap: Vec::new(), best_lap_time: 0.0, last_lap_index: 0, current_distance: 0.0, last_recorded_distance: 0.0, current_diff: 0.0, current_speed_diff_ms: 0.0 }
    }
}

/// `cvttsd2si` into 32 bits: NaN and what does not fit is `i32::MIN`.
#[allow(clippy::manual_range_contains)]
fn cvttsd2si32(x: f64) -> i32 {
    if x.is_nan() || x >= 2_147_483_648.0 || x <= -2_147_483_649.0 {
        i32::MIN
    } else {
        x as i32
    }
}

impl PerformanceMeter {
    /// `PerformanceMeter::init` 0x14026b7c0 (the vectors and the outputs are left as they are).
    pub fn init(&mut self) {
        self.is_enabled = true;
        self.best_lap_time = f64::from_bits(0xbff0_0000_0000_0000);
        self.last_lap_index = 0;
        self.current_distance = 0.0;
        self.last_recorded_distance = 0.0;
    }

    /// `PerformanceMeter::reset` 0x14026b880 (`Car::onNewSession`): the outputs stay.
    pub fn reset(&mut self) {
        self.best_lap_time = f64::from_bits(0xbff0_0000_0000_0000);
        self.last_lap_index = 0;
        self.current_distance = 0.0;
        self.last_recorded_distance = 0.0;
        self.current_lap.clear();
        self.best_lap.clear();
    }

    /// `PerformanceMeter::hasData` 0x14026b7a0.
    pub fn has_data(&self) -> bool {
        !self.best_lap.is_empty()
    }

    /// `PerformanceMeter::getCurrentSplit` 0x14026b700: (seconds, m/s); nothing on an open
    /// track that has not started (`TimeTransponder::isInOpenTrackTransition`).
    pub fn get_current_split(&self, in_open_track_transition: bool) -> (f64, f32) {
        if in_open_track_transition {
            (0.0, 0.0)
        } else {
            (self.current_diff, self.current_speed_diff_ms)
        }
    }

    /// `PerformanceMeter::step` 0x14026b8e0 (`Car::stepComponents`, the player's car only).
    pub fn step(&mut self, input: &PerformanceMeterInput) {
        if !self.is_enabled {
            return;
        }
        if !input.finish_line_passed {
            let npos = input.npos as f64;
            self.current_distance = npos;
            let e = cvttsd2si32(npos * f64::from_bits(0xc08f_3fff_e719_0014));
            let idx = (-1i32).wrapping_sub(e);
            if idx < 0 || (idx as i64 as u64) < self.current_lap.len() as u64 {
                return;
            }
            self.current_lap.push(PerformancePair { t: input.t, speed_ms: input.speed });
            self.last_recorded_distance = self.current_distance;
            let i = (self.current_lap.len() as i32).wrapping_sub(1);
            if i < 0 || i as u64 >= self.best_lap.len() as u64 {
                self.current_diff = 0.0;
                self.current_speed_diff_ms = 0.0;
            } else {
                let best = self.best_lap[i as usize];
                self.current_diff = (input.t as f64 - best.t as f64) * f64::from_bits(0x3f50_624d_d2f1_a9fc);
                self.current_speed_diff_ms = input.speed - best.speed_ms;
            }
        } else {
            if input.was_last_lap_valid && (self.best_lap.is_empty() || (input.last_lap as f64) < self.best_lap_time) {
                self.best_lap = self.current_lap.clone();
                self.best_lap_time = input.last_lap as f64;
            }
            self.current_distance = 0.0;
            self.last_recorded_distance = 0.0;
            self.current_lap.clear();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_faster_lap_against_the_best() {
        let mut m = PerformanceMeter::default();
        m.init();
        let lap = |m: &mut PerformanceMeter, ms_per_thousandth: u32, speed: f32| {
            for k in 1..=999u32 {
                m.step(&PerformanceMeterInput { npos: k as f32 / 999.0, t: k * ms_per_thousandth, speed, ..Default::default() });
            }
            m.step(&PerformanceMeterInput { finish_line_passed: true, was_last_lap_valid: true, last_lap: 1000 * ms_per_thousandth, ..Default::default() });
        };
        lap(&mut m, 100, 50.0);
        assert_eq!(m.best_lap.len(), 999);
        assert!(m.has_data());
        // the next lap 10 ms a thousandth faster: half way, 5 s ahead
        for k in 1..=500u32 {
            m.step(&PerformanceMeterInput { npos: k as f32 / 999.0, t: k * 90, speed: 52.0, ..Default::default() });
        }
        let (t, s) = m.get_current_split(false);
        assert!((t + 5.0).abs() < 0.2, "{t}");
        assert_eq!(s, 2.0);
        assert_eq!(m.get_current_split(true), (0.0, 0.0));
        m.reset();
        assert!(!m.has_data());
        // (the outputs stay until the next sample)
        assert_eq!(m.get_current_split(false).1, 2.0);
    }
}
