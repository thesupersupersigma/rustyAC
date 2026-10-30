// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! `DriftMode` (0x1248 bytes, constructor @ 0x140089610): the drift session's score, levels
//! and clock, worked out in its `renderHUD` @ 0x14008b7c0 (the game has no `update` for it)
//! from the player's car's drift points (`crate::car::drift`, through `CarPhysicsState`).
//! [`DriftMode::hud`] makes every change one `renderHUD` makes and returns what it draws.

use super::game_mode::GameMode;
use super::output::Node;

/// `eDriftModeStatus`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DriftStatus {
    Over = 0,
    Drifting = 1,
    #[default]
    Still = 2,
}

/// A rising "+n" text (`BonusInfo`): `x` from the middle of the screen, `value` the
/// seconds (`+n"`) or the points (`+n pt`) its text shows.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Bonus {
    pub x: f32,
    pub y: f32,
    pub value: i32,
}

/// What `DriftMode` reads of the player's car (its `CarPhysicsState` copy) and the menus.
#[derive(Clone, Copy, Debug, Default)]
pub struct DriftFrame {
    pub drift_points: f32,
    pub instant_drift: f32,
    pub combo: i32,
    pub bonus_on: bool,
    /// m/s
    pub speed: f32,
    pub esc_open: bool,
    pub paused: bool,
    pub replay: bool,
}

/// What one `renderHUD` draws.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DriftDraw {
    Start,
    Over,
    Drifting {
        /// the rising texts before this frame moved them (time, then points)
        bonuses: [Option<Bonus>; 2],
        level: i32,
        current_points_per_level: f32,
        points_per_level: f32,
        level_up: bool,
        seconds: i32,
        session_points: i32,
        instant_drift: f32,
        combo: i32,
        bonus_on: bool,
    },
}

/// What a `renderHUD` asks of the car and the session.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct DriftOut {
    pub gentle_stop: Option<bool>,
    /// the clock ran out: a fade of 2.5 s, then the results
    pub fade_to_results: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DriftMode {
    pub base: GameMode,
    pub mode: DriftStatus,
    pub bonus_time: Bonus,
    pub bonus_points: Bonus,
    pub last_time_left: i32,
    pub max_session_combo_count: f32,
    pub level_up_time: f32,
    pub old_points: f32,
    pub seconds_left: f32,
    pub current_level: i32,
    pub session_points: f32,
    pub max_points: i32,
    pub current_points_per_level: f32,
    pub last_points_achieved: i32,
    pub last_seconds_achieved: i32,
    /// not the game's: what the last `hud` drew, for the renderer
    pub drawn: Option<DriftDraw>,
}

/// `pointsPerLevel[level]`: 200, 300, ... (1000 of them).
fn points_per_level(level: i32) -> f32 {
    (200 + 100 * level) as f32
}

impl DriftMode {
    pub fn new(base: GameMode) -> DriftMode {
        DriftMode {
            base,
            mode: DriftStatus::Still,
            bonus_time: Bonus::default(),
            bonus_points: Bonus::default(),
            last_time_left: 0,
            max_session_combo_count: 0.0,
            level_up_time: 0.0,
            old_points: 0.0,
            seconds_left: 60.0,
            current_level: 0,
            session_points: 0.0,
            max_points: 0,
            current_points_per_level: 0.0,
            last_points_achieved: 0,
            last_seconds_achieved: 0,
            drawn: None,
        }
    }

    /// The handler on `Sim::evOnNewSession` (0x14008a040): the rest is kept.
    pub fn new_session(&mut self) {
        self.mode = DriftStatus::Still;
        self.current_level = 0;
        self.session_points = 0.0;
        self.current_points_per_level = 0.0;
        self.max_session_combo_count = 0.0;
    }

    /// `renderHUD` @ 0x14008b7c0. `message`: `SystemMessage::addMessage(text, description)`.
    pub fn hud(&mut self, dt: f32, f: &DriftFrame, message: &mut dyn FnMut(&str, &str), out: &mut DriftOut) -> Option<DriftDraw> {
        self.base.verify("Drift challenge", self.session_points, message);
        if f.esc_open || f.replay || f.paused {
            return None;
        }
        match self.mode {
            DriftStatus::Over => {
                // modeGameOver
                self.base.verify("Drift challenge", self.session_points, message);
                out.gentle_stop = Some(true);
                Some(DriftDraw::Over)
            }
            DriftStatus::Drifting => self.drifting(dt, f, out),
            DriftStatus::Still => {
                out.gentle_stop = Some(false);
                if f.speed * 3.6 > 15.0 {
                    self.mode = DriftStatus::Drifting;
                    self.seconds_left = 60.0;
                }
                Some(DriftDraw::Start)
            }
        }
    }

    /// `modeDrifting` @ 0x14008a1f0.
    fn drifting(&mut self, dt: f32, f: &DriftFrame, out: &mut DriftOut) -> Option<DriftDraw> {
        let (mut bonus_secs, mut bonus_pts) = (0, 0);
        if f.combo as f32 > self.max_session_combo_count {
            self.max_session_combo_count = f.combo as f32;
        }
        if self.old_points as i32 != f.drift_points as i32 {
            bonus_pts = (f.drift_points - self.old_points) as i32;
            self.current_points_per_level += bonus_pts as f32;
            self.old_points = f.drift_points;
            self.last_points_achieved = bonus_pts;
        }
        self.session_points += bonus_pts as f32;
        if self.session_points > self.max_points as f32 {
            self.max_points = self.session_points as i32;
        }
        let level = self.current_level;
        if self.current_points_per_level > points_per_level(level) {
            self.current_points_per_level -= points_per_level(level);
            bonus_secs = (30.0 - level as f32).clamp(10.0, 30.0) as i32;
            self.last_seconds_achieved = bonus_secs;
            if level < 999 {
                self.current_level = level + 1;
            }
            self.seconds_left += bonus_secs as f32;
            self.level_up_time = 3.0;
        }
        if self.seconds_left <= 0.0 && f.instant_drift == 0.0 {
            self.mode = DriftStatus::Over;
            out.fade_to_results = true;
            return None; // nothing drawn in that frame
        }
        // animateBonuses: drawn where they are, then they rise
        let mut bonuses = [None, None];
        for (bonus, shown) in [&mut self.bonus_time, &mut self.bonus_points].into_iter().zip(&mut bonuses) {
            if bonus.y > 0.0 {
                *shown = Some(*bonus);
                bonus.y -= dt * 100.0;
            }
        }
        let draw = DriftDraw::Drifting {
            bonuses,
            level: self.current_level,
            current_points_per_level: self.current_points_per_level,
            points_per_level: points_per_level(self.current_level),
            level_up: self.level_up_time > 0.0,
            seconds: self.seconds_left as i32,
            session_points: self.session_points as i32,
            instant_drift: f.instant_drift,
            combo: f.combo,
            bonus_on: f.bonus_on,
        };
        if self.level_up_time > 0.0 {
            self.level_up_time -= dt;
        }
        if bonus_secs != 0 {
            self.bonus_time = Bonus { x: -10.0, y: 100.0, value: bonus_secs };
        }
        if bonus_pts != 0 {
            self.bonus_points = Bonus { x: 260.0, y: 100.0, value: bonus_pts };
        }
        if self.seconds_left > 0.0 || f.instant_drift <= 0.0 {
            self.seconds_left -= dt;
        } else {
            self.seconds_left = 0.0;
        }
        self.last_time_left = self.seconds_left as i32;
        Some(draw)
    }

    /// `race_out.json`'s `{"name":"drift","points","combos","levels"}`.
    pub fn extra(&self) -> Node {
        Node::Object(vec![
            ("name".to_string(), Node::Text("drift".to_string())),
            ("points".to_string(), Node::int(self.session_points as i32 as i64)),
            ("combos".to_string(), Node::int(self.max_session_combo_count as i32 as i64)),
            ("levels".to_string(), Node::int(self.current_level as i64 + 1)),
        ])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_banked_drift_levels_up_and_the_clock_ends_the_game() {
        let mut d = DriftMode::new(GameMode::default());
        let (mut out, mut said) = (DriftOut::default(), Vec::new());
        let mut f = DriftFrame { speed: 10.0, ..Default::default() };
        assert_eq!(d.hud(0.1, &f, &mut |a, b| said.push(format!("{a} {b}")), &mut out), Some(DriftDraw::Start));
        assert_eq!(d.mode, DriftStatus::Drifting);
        f.drift_points = 250.7;
        d.hud(0.5, &f, &mut |_, _| {}, &mut out);
        assert_eq!((d.session_points, d.current_level, d.seconds_left), (250.0, 1, 89.5));
        assert_eq!(d.bonus_points.value, 250);
        d.seconds_left = 0.01;
        d.hud(0.1, &f, &mut |_, _| {}, &mut out);
        d.hud(0.1, &f, &mut |_, _| {}, &mut out);
        assert!(out.fade_to_results && d.mode == DriftStatus::Over);
    }
}
