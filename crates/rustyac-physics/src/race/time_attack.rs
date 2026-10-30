// SPDX-License-Identifier: MIT OR Apache-2.0
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! `TimeAttack` (session type 5): a time budget per sector, points at each checkpoint (a
//! split of the race manager's current lap), penalties, the special event's goals.
//!
//! * [`TimeAttack::new_session`]: the constructor's and `evOnNewSession`'s budget
//!   (@ 0x1401b7e20, lambda @ 0x1401b91d0), `TimeAttackGameStatus::reset` @ 0x1401bdcd0.
//! * [`TimeAttack::update`]: `update` @ 0x1401be0b0, `updateGameOver` @ 0x1401be280,
//!   `updateTimeAttack` @ 0x1401be570, `updatePenalties` @ 0x1401be2c0, `verifyConditions`
//!   @ 0x1401beb10.
//! * [`TimeAttack::hud_tick`]: what `renderHUD` @ 0x1401bda40 changes (the panels' timers of
//!   `renderGuiStatus` @ 0x1401bd120 and `renderGuiCheckpointBreak` @ 0x1401bc4c0); the
//!   drawing itself is the GUI's.
//!
//! The game runs it once per drawn frame with the frame's time; rustyAC runs it in the
//! referee's frame (shortcut: a 9 ms frame instead of the screen's, upgrade if points must
//! match a recorded run of the game frame for frame).

#![allow(clippy::neg_cmp_op_on_partial_ord)]

use super::game_mode::GameMode;
use super::output::Node;

/// `eTimeAttackStatus`
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TaMode {
    Over = 0,
    On = 1,
    #[default]
    Still = 2,
}

/// `TimeAttackGameStatus` (what is read of it), `TimeAttackTarget` and `TimeAttackTempStatus`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TimeAttack {
    pub base: GameMode,
    /// `cfg/gameplay.ini [TIME_ATTACK] BASE_TIME_MULT`
    pub time_mult: f32,
    /// `target.sectors`: the budget of each sector
    pub sectors: Vec<f32>,
    pub sector_increments: Vec<f32>,
    pub sector_index: usize,
    pub time_left: f32,
    pub points: f32,
    pub car_damage: f32,
    pub game_over_time: f32,
    pub slow_penalty_timer: f32,
    pub reset_position: [f32; 3],
    pub sector_times: Vec<f32>,
    pub mode: TaMode,
    pub score_label_timer: f32,
    pub penalty_points_accumulated: f32,
    pub penalty_timer: f32,
    pub last_gained_points: f32,
    pub last_gained_seconds: f32,
    pub last_car_damage: f32,
    /// the HUD's checkpoint panel shows this frame (what `hud_tick` found)
    pub checkpoint_shown: bool,
}

/// What a frame of the mode reads of the session, the car and the screen.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TaFrame {
    /// the track's sector count (`sectorsNormalizedPositions`)
    pub track_sectors: usize,
    /// the pause menu, the ESC menu, a replay
    pub paused: bool,
    pub esc_open: bool,
    pub replay: bool,
    /// the car's body position
    pub position: [f32; 3],
    /// `RaceManager::getCurrentLap(car).splits.size()`, `getLapCount(car)`
    pub splits: usize,
    pub lap_count: u32,
    pub damage_zone_level: [f32; 5],
    /// tyres on a surface that is not `IS_VALID_TRACK`
    pub tyres_off: usize,
    /// `RaceEngineer::getMaxSpeedFromGear(0)`, m/s
    pub first_gear_max_speed: f32,
    /// m/s
    pub speed: f32,
    /// `CarAvatar::hasPenalty`
    pub penalty_time: f32,
}

/// What a frame asks of the rest of the game.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TaOut {
    /// `CarAvatar::setGentleStop(car, value)`
    pub gentle_stop: Option<bool>,
    /// the time ran out: the 2.5 s camera fade, then `EndSessionDisplayer::displayResults`
    pub fade_to_results: bool,
}

impl TimeAttack {
    pub fn new(base: GameMode, time_mult: f32) -> TimeAttack {
        TimeAttack { base, time_mult, slow_penalty_timer: 10.0, score_label_timer: 1000.0, ..TimeAttack::default() }
    }

    /// The budget of a new session from `evaluateTimeFromTrackSpline`'s seconds per sector.
    pub fn new_session(&mut self, estimate: &[f32], track_sectors: usize) {
        self.slow_penalty_timer = 10.0;
        self.sectors = estimate.iter().map(|v| (v + 10.0) * self.time_mult).collect();
        self.sector_increments = vec![f32::from_bits(0x3c23_d70a); self.sectors.len()];
        self.mode = TaMode::Still;
        self.sector_index = 0;
        self.time_left = 0.0;
        self.points = 0.0;
        self.car_damage = 0.0;
        self.game_over_time = 0.0;
        self.sector_times = vec![0.0; track_sectors];
        self.score_label_timer = 1000.0;
        self.penalty_points_accumulated = 0.0;
        self.penalty_timer = 0.0;
        self.last_gained_points = 0.0;
        self.last_gained_seconds = 0.0;
        self.last_car_damage = 0.0;
    }

    /// `TimeAttack::update`. `message`: `SystemMessage::addMessage(text, description)`.
    pub fn update(&mut self, dt: f32, f: &TaFrame, message: &mut dyn FnMut(&str, &str)) -> TaOut {
        let mut out = TaOut::default();
        if f.track_sectors == 0 || f.paused || self.sector_index >= self.sectors.len() || self.sector_index >= self.sector_times.len() || f.replay {
            return out;
        }
        match self.mode {
            TaMode::Over => {
                self.game_over_time += dt;
                self.verify_conditions(message);
                out.gentle_stop = Some(true);
            }
            TaMode::On => out.fade_to_results = self.update_time_attack(dt, f, message),
            TaMode::Still => {
                out.gentle_stop = Some(false);
                if f.esc_open {
                    self.reset_position = f.position;
                    return out;
                }
                let r = self.reset_position;
                let (dx, dy, dz) = (f.position[0] - r[0], f.position[1] - r[1], f.position[2] - r[2]);
                let d2 = (dy * dy + dx * dx) + dz * dz;
                if d2 != 0.0 && crate::math::sqrtf(d2) > 3.0 {
                    self.mode = TaMode::On;
                }
            }
        }
        out
    }

    fn update_time_attack(&mut self, dt: f32, f: &TaFrame, message: &mut dyn FnMut(&str, &str)) -> bool {
        self.update_penalties(dt, f, message);
        if self.sector_index != f.splits {
            let si = self.sector_index;
            let rem = self.sectors[si] - self.sector_times[si];
            if rem < 0.0 {
                self.mode = TaMode::Over;
                return false;
            }
            let pts = rem * 10.0;
            let mut lap_bonus = ((f.lap_count as f32) * 100.0) * rem;
            lap_bonus = lap_bonus.clamp(0.0, 750.0);
            if f.lap_count == 1 && f.splits == 0 {
                lap_bonus = 0.0;
            }
            let gained = pts + lap_bonus;
            self.points += gained;
            let secs = rem * 0.5;
            let bonus = if secs >= 5.0 { 5.0 } else { secs };
            let new_bonus = bonus - self.sector_times[si] * self.sector_increments[si];
            self.sectors[si] = self.sector_times[si] + new_bonus;
            self.checkpoint_break(dt, gained, new_bonus);
            self.sector_times[si] = 0.0;
            self.sector_index = f.splits;
        }
        let si = self.sector_index;
        if si < self.sector_times.len() {
            self.sector_times[si] += dt;
            self.time_left = self.sectors[si] - self.sector_times[si];
            if self.time_left > 0.0 {
                return false;
            }
            self.mode = TaMode::Over;
            return true;
        }
        false
    }

    fn update_penalties(&mut self, dt: f32, f: &TaFrame, message: &mut dyn FnMut(&str, &str)) {
        let d = f.damage_zone_level;
        let damage = (((d[0] + d[1]) + d[2]) + d[3]) + d[4];
        if damage != self.car_damage {
            self.penalty_points_accumulated -= damage * 5.0;
            self.points -= damage * 5.0;
            self.car_damage = damage;
        }
        if f.tyres_off >= 3 {
            let x = dt * 100.0;
            self.penalty_points_accumulated -= x;
            self.points -= x;
        }
        let slow_below = (f.first_gear_max_speed * 3.6) * f32::from_bits(0x3f2a_aaab);
        if !(slow_below > f.speed * 3.6) {
            self.slow_penalty_timer = 10.0;
        } else {
            self.slow_penalty_timer -= dt;
            if self.slow_penalty_timer <= 0.0 {
                message("Warning", "You are too slow, gain some speed");
                let x = dt * 100.0;
                self.penalty_points_accumulated -= x;
                self.points -= x;
            }
        }
    }

    fn verify_conditions(&mut self, message: &mut dyn FnMut(&str, &str)) {
        for i in 0..self.base.conditions.len() {
            let c = &mut self.base.conditions[i];
            if c.objective as f32 <= self.points && !c.achieved {
                message("TimeAttack challenge", &format!("{} Achieved", c.name));
                c.achieved = true;
                self.base.current_tier = i as i32;
            }
        }
    }

    /// `renderGuiCheckpointBreak`'s timer: whether the checkpoint panel is drawn.
    pub fn checkpoint_break(&mut self, dt: f32, gained_points: f32, gained_seconds: f32) -> bool {
        if gained_points != 0.0 {
            self.score_label_timer = 0.0;
            self.last_gained_points = gained_points;
            self.last_gained_seconds = gained_seconds;
        }
        if self.score_label_timer < 5.0 {
            self.score_label_timer += dt;
            return true;
        }
        false
    }

    /// Whether `renderHUD` draws anything.
    pub fn hud_shown(&self, f: &TaFrame) -> bool {
        f.track_sectors != 0 && !self.sectors.is_empty() && !f.esc_open && !f.replay && !f.paused
    }

    /// What `renderHUD` changes. Returns whether the checkpoint panel is drawn.
    pub fn hud_tick(&mut self, dt: f32, f: &TaFrame) -> bool {
        self.checkpoint_shown = false;
        if !self.hud_shown(f) || self.mode != TaMode::On {
            return false;
        }
        if self.last_car_damage != self.car_damage {
            self.penalty_timer = 5.0;
            self.last_car_damage = self.car_damage;
        }
        if f.penalty_time > 0.0 {
            self.penalty_timer = 5.0;
            self.last_car_damage = self.car_damage;
        }
        if self.penalty_timer > 0.0 {
            self.penalty_timer -= dt;
            if self.penalty_timer <= 0.0 {
                self.penalty_points_accumulated = 0.0;
                self.penalty_timer = 0.0;
            }
        }
        self.checkpoint_shown = self.checkpoint_break(dt, 0.0, 0.0);
        self.checkpoint_shown
    }

    /// `race_out.json`'s `{"name":"timeattack","points"}`.
    pub fn extra(&self) -> Node {
        Node::Object(vec![("name".to_string(), Node::Text("timeattack".to_string())), ("points".to_string(), Node::int(self.points as i32 as i64))])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_checkpoint_banks_points_and_time() {
        let mut ta = TimeAttack::new(GameMode::default(), 1.0);
        ta.new_session(&[20.0, 30.0], 2);
        let mut f = TaFrame { track_sectors: 2, position: [0.0, 0.0, 5.0], speed: 50.0, first_gear_max_speed: 20.0, ..Default::default() };
        let mut said = Vec::new();
        ta.update(0.1, &f, &mut |a, b| said.push(format!("{a} {b}")));
        assert_eq!(ta.mode, TaMode::On);
        for _ in 0..100 {
            ta.update(0.1, &f, &mut |_, _| {});
        }
        // 30 s budget, 10 s spent: 20 s left gives 200 points, +5 s
        f.splits = 1;
        ta.update(0.1, &f, &mut |_, _| {});
        assert_eq!((ta.sector_index, ta.points as i32), (1, 199));
        assert!((ta.sectors[0] - (10.0 + 5.0 - 10.0 * 0.01)).abs() < 1e-3);
        assert!(said.is_empty());
    }
}
