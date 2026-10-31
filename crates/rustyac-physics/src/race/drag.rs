// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! `DragMode` (0x2f0 bytes, constructor @ 0x140084a20): a drag session's matches, best of
//! `[SESSION_0] MATCHES`, between the player's car and one AI car. The game's code is for
//! two cars; so is this.

use super::game_mode::GameMode;
use super::output::Node;
use super::timing::Lap;
use crate::math::sqrtf;
use crate::track::spline::InterpolatingSpline;

/// `eDragModeStatus`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DragState {
    GameOver = 0,
    RaceOn = 1,
    Suspended = 2,
    #[default]
    Still = 3,
}

/// `DragMode::DragStatus` (0x48 bytes).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DragStatus {
    pub false_start: bool,
    pub disqualified: bool,
    pub wins: u32,
    pub reaction_time: f32,
    pub player_time: f32,
    pub final_speed: i32,
    pub max_speed: i32,
    pub spline_side: f32,
    pub lowest_reaction_time: f32,
    pub spawn_position: [f32; 3],
    pub still_position: [f32; 3],
}

/// What `DragMode` asks of the cars, the menus and the camera.
pub trait DragHost {
    /// the CRT `rand()` the game shares with the AI drivers
    fn rand(&mut self) -> i32;
    fn is_ai(&self, car: usize) -> bool;
    /// `AIDriverDrag::setStartTime` (one `rand()`)
    fn ai_start_time(&mut self, car: usize, time: f64);
    fn gentle_stop(&mut self, car: usize, on: bool);
    /// `CarAvatar::goToSpawnPosition("START")` and `resetTimeTransponder`
    fn go_to_spawn(&mut self, car: usize);
    /// `resetMatch` brings the ESC menu back
    fn show_esc_menu(&mut self);
    /// `ACCameraManager::fadeIn`: `true` when a fade began; its end is
    /// [`DragMode::reset_match`] (`results`: false) or the end screen
    fn fade_in(&mut self, seconds: f32, results: bool) -> bool;
}

/// What one frame gives the mode: the clock (ms) and the cars' places.
#[derive(Clone, Debug, Default)]
pub struct DragFrame {
    pub game_time: f64,
    pub paused: bool,
    pub replay: bool,
    /// `CarAvatar::bodyMatrix`'s translation (+0x254)
    pub positions: Vec<[f32; 3]>,
    /// `CarAvatar::physicsState`'s position (+0x29c), for the progress bar
    pub state_positions: Vec<[f32; 3]>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DragMode {
    pub base: GameMode,
    pub lap_times: Vec<Vec<Lap>>,
    pub cars: Vec<DragStatus>,
    pub status: DragState,
    pub match_number: u32,
    pub start_time: f64,
    pub null_counter: f32,
    /// the track's semaphore was found (the first `Suspended` frame)
    pub semaphore: bool,
    /// `StartingTrackSemaphore::disqualifiedOn` and `customStart` (0: the session's clock)
    pub disqualified_on: bool,
    pub custom_start: f64,
    pub spline: InterpolatingSpline,
    pub player_perc: f32,
    pub ai_perc: f32,
    pub count_down: f32,
}

/// `rand() * 3.0518509e-05f` as the game has it.
fn rand_unit(host: &mut dyn DragHost) -> f32 {
    host.rand() as f32 * f32::from_bits(0x3800_0100)
}

fn side(d: f32) -> f32 {
    if d > 0.0 {
        1.0
    } else if d < 0.0 {
        -1.0
    } else {
        0.0
    }
}

impl DragMode {
    /// The constructor: `start` the `START` slots 0 and 1, `finish` the nodes
    /// `AC_OPEN_FINISH_L`/`_R`, `spawns` every car's `START` place; `matches` `[SESSION_0]
    /// MATCHES` (0 without the key, 5 without a readable race.ini).
    pub fn new(base: GameMode, matches: u32, start: [[f32; 3]; 2], finish: [[f32; 3]; 2], spawns: &[[f32; 3]], game_time: f64, host: &mut dyn DragHost) -> DragMode {
        let s = [0, 1, 2].map(|k| (start[0][k] + start[1][k]) * 0.5);
        let d = [0, 1, 2].map(|k| (finish[1][k] + finish[0][k]) * 0.5 - s[k]);
        let mut spline = InterpolatingSpline::default();
        spline.linear = true;
        for i in 0..=100u32 {
            let t = i as f32 * f32::from_bits(0x3c23_d70a);
            spline.add_point([d[0] * t + s[0], d[1] * t + s[1], d[2] * t + s[2]], 0);
        }
        spline.compute_spline_length();
        let cars = spawns
            .iter()
            .map(|&pos| {
                let n = spline.world_to_spline(&pos, -1);
                DragStatus { false_start: false, disqualified: false, wins: 0, reaction_time: 0.0, player_time: 0.0, final_speed: 0, max_speed: 0, spline_side: side(spline.get_signed_distance_from_spline(&pos, n)), lowest_reaction_time: f32::MAX, spawn_position: pos, still_position: [0.0; 3] }
            })
            .collect::<Vec<_>>();
        let mut mode = DragMode {
            base,
            lap_times: vec![Vec::new(); cars.len()],
            cars,
            status: DragState::Still,
            match_number: matches,
            start_time: 0.0,
            null_counter: 0.0,
            semaphore: false,
            disqualified_on: false,
            custom_start: 0.0,
            spline,
            player_perc: 0.0,
            ai_perc: 0.0,
            count_down: 0.0,
        };
        mode.start_time = mode.generate_start_time(game_time, host);
        mode
    }

    /// `generateStartTime` @ 0x140086ea0: 10 to 20 s from now.
    fn generate_start_time(&self, game_time: f64, host: &mut dyn DragHost) -> f64 {
        rand_unit(host) as f64 * 10000.0 + (game_time + 10000.0)
    }

    /// The handler of `ESCMenu::evOnDriveMenuPressed`.
    pub fn drive_pressed(&mut self) {
        self.status = DragState::RaceOn;
    }

    /// The handler of `Sim::evOnNewSession`.
    pub fn new_session(&mut self, game_time: f64, host: &mut dyn DragHost) {
        self.start_time = f64::MAX;
        self.null_counter = 0.0;
        self.lap_times.iter_mut().for_each(Vec::clear);
        self.cars.iter_mut().for_each(|c| c.wins = 0);
        self.new_match(1.0, game_time, host);
    }

    /// `newMatch` @ 0x140086fc0
    fn new_match(&mut self, fade: f32, game_time: f64, host: &mut dyn DragHost) {
        if !host.fade_in(fade, false) {
            self.reset_match(game_time, host);
        }
    }

    /// `resetMatch` @ 0x1400889e0 (also the end of `newMatch`'s fade).
    pub fn reset_match(&mut self, game_time: f64, host: &mut dyn DragHost) {
        self.null_counter = 0.0;
        self.start_time = self.generate_start_time(game_time, host);
        if self.semaphore {
            self.disqualified_on = false;
        }
        // DragStatus::reset is given the start as a float
        let start = self.start_time as f32 as f64;
        for (i, c) in self.cars.iter_mut().enumerate() {
            host.go_to_spawn(i);
            if host.is_ai(i) {
                host.ai_start_time(i, start);
            }
            host.gentle_stop(i, false);
            c.reaction_time = 0.0;
            c.player_time = 0.0;
            c.false_start = false;
            c.disqualified = false;
            c.final_speed = 0;
        }
        host.show_esc_menu();
        self.status = DragState::Still;
    }

    /// `update` @ 0x140088c30
    pub fn update(&mut self, dt: f32, f: &DragFrame, host: &mut dyn DragHost) {
        if f.replay || f.paused {
            if self.status == DragState::RaceOn && self.start_time - f.game_time > 0.0 {
                // (the game writes into the semaphore without asking whether it found it)
                self.start_time = self.generate_start_time(f.game_time, host);
                self.custom_start = self.start_time;
                self.ai_start_times(host);
            }
            return;
        }
        if self.count_down > 0.0 {
            self.count_down -= dt;
            if self.count_down <= 0.0 {
                if self.status != DragState::GameOver {
                    self.new_match(1.0, f.game_time, host);
                } else {
                    host.fade_in(2.5, true);
                }
            }
        }
        match self.status {
            DragState::GameOver => (0..self.cars.len()).for_each(|i| host.gentle_stop(i, true)),
            DragState::RaceOn => {
                (0..self.cars.len()).for_each(|i| host.gentle_stop(i, false));
                self.update_drag_race(dt, f);
            }
            // (Still waits for Drive with the cars held and the lights' time rolled again)
            DragState::Still => {
                self.start_time = self.generate_start_time(f.game_time, host);
                self.semaphore = true;
                self.custom_start = self.start_time;
                for i in 0..self.cars.len() {
                    host.gentle_stop(i, true);
                    self.cars[i].still_position = f.positions[i];
                    if host.is_ai(i) {
                        host.ai_start_time(i, self.start_time);
                    }
                }
            }
            DragState::Suspended => {}
        }
    }

    fn ai_start_times(&mut self, host: &mut dyn DragHost) {
        for i in 0..self.cars.len() {
            if host.is_ai(i) {
                host.ai_start_time(i, self.start_time);
            }
        }
    }

    /// A car's penalty: every other car wins the match.
    fn others_win(&mut self, offender: usize) {
        for j in 0..self.cars.len() {
            if j != offender {
                self.cars[j].wins += 1;
                self.compute_car_times();
                let over = self.is_game_over();
                self.count_down = 2.5;
                if over {
                    self.status = DragState::GameOver;
                }
            }
        }
    }

    /// `updateDragRace` @ 0x140088f90
    fn update_drag_race(&mut self, dt: f32, f: &DragFrame) {
        if self.start_time - f.game_time > 0.0 || self.status != DragState::RaceOn {
            if self.null_counter < 1.0 {
                self.null_counter += dt;
                return;
            }
            for i in 0..self.cars.len() {
                let (p, s) = (f.positions[i], self.cars[i].still_position);
                let (dx, dz) = (p[0] - s[0], p[2] - s[2]);
                let d2 = dz * dz + dx * dx;
                if d2 != 0.0 && sqrtf(d2) > f32::from_bits(0x3dcc_cccd) && self.status == DragState::RaceOn {
                    self.cars[i].false_start = true;
                    if self.semaphore {
                        self.disqualified_on = true;
                    }
                    self.status = DragState::Suspended;
                    self.others_win(i);
                }
            }
            return;
        }
        self.null_counter = 0.0;
        self.player_perc = self.spline.world_to_spline(&f.state_positions[0], -1);
        self.ai_perc = self.spline.world_to_spline(&f.state_positions[1], -1);
        for i in 0..self.cars.len() {
            let p = f.positions[i];
            let s = side(self.spline.get_signed_distance_from_spline(&p, 0.5));
            if s != self.cars[i].spline_side && self.cars[i].final_speed == 0 {
                self.status = DragState::Suspended;
                self.cars[i].disqualified = true;
                self.others_win(i);
            }
            let c = &mut self.cars[i];
            if c.final_speed == 0 {
                c.player_time += dt;
            }
            let q = c.still_position;
            let (dx, dy, dz) = (p[0] - q[0], p[1] - q[1], p[2] - q[2]);
            let d2 = (dy * dy + dx * dx) + dz * dz;
            if d2 != 0.0 && sqrtf(d2) > 2.0 && self.status == DragState::RaceOn && c.reaction_time == 0.0 {
                c.reaction_time = c.player_time;
                if c.player_time < c.lowest_reaction_time && !c.false_start {
                    c.lowest_reaction_time = c.player_time;
                }
            }
        }
    }

    /// `onFinishLanePassed` @ 0x1400870e0: a car crossed the finish (`speed` m/s).
    pub fn on_finish_lane_passed(&mut self, car: usize, speed: f32, use_mph: bool, host: &mut dyn DragHost, message: &mut dyn FnMut(&str, &str)) {
        let Some(c) = self.cars.get_mut(car) else { return };
        let v = speed * if use_mph { f32::from_bits(0x400f_29f7) } else { f32::from_bits(0x4066_6666) };
        c.final_speed = v as i32;
        c.max_speed = c.max_speed.max(c.final_speed);
        host.gentle_stop(car, true);
        let other = self.cars.iter().enumerate().any(|(j, c)| j != car && c.final_speed != 0);
        if !other || self.status != DragState::RaceOn {
            return;
        }
        // a tie goes to the second car
        if self.cars[1].player_time <= self.cars[0].player_time {
            self.cars[1].wins += 1;
        } else {
            self.cars[0].wins += 1;
        }
        self.compute_car_times();
        let over = self.is_game_over();
        self.count_down = 2.5;
        if over {
            self.status = DragState::GameOver;
        }
        // verifyConditions @ 0x140089460: the player's wins against the goals
        let wins = self.cars[0].wins;
        self.base.verify_by("Drag challenge", |objective| objective as u32 <= wins, message);
    }

    /// `computeCarTimes` @ 0x140086650: every car's match as a lap.
    fn compute_car_times(&mut self) {
        for (c, laps) in self.cars.iter().zip(self.lap_times.iter_mut()) {
            let time = (c.player_time * 1000.0) as i64 as u32;
            laps.push(Lap { time, splits: vec![time], cuts: 0, is_valid: !(c.disqualified && c.false_start), compound: String::new() });
        }
    }

    /// `isGameOver` @ 0x140086ef0
    pub fn is_game_over(&self) -> bool {
        let threshold = (self.match_number as f32 / self.cars.len() as f32) as i32 + 1;
        let mut best = 0;
        for (i, c) in self.cars.iter().enumerate() {
            if c.wins > self.cars[best].wins {
                best = i;
            }
        }
        self.cars.get(best).is_some_and(|c| c.wins >= threshold as u32)
    }

    /// `getNumberOfRuns` @ 0x14010b340
    pub fn runs(&self) -> u32 {
        match self.cars.as_slice() {
            [a, b, ..] => a.wins + b.wins,
            _ => 0,
        }
    }

    /// A proof's record of the mode after a drawn frame: everything but the constructor's
    /// spline and the race.ini values that never change.
    pub fn frame_state(&self) -> String {
        format!(
            "{:?} {:?} {:?} {} {:x} {:x} {} {} {:x} {:x} {:x} {:x} {:?} {}",
            self.lap_times,
            self.cars,
            self.status,
            self.match_number,
            self.start_time.to_bits(),
            self.null_counter.to_bits(),
            self.semaphore,
            self.disqualified_on,
            self.custom_start.to_bits(),
            self.player_perc.to_bits(),
            self.ai_perc.to_bits(),
            self.count_down.to_bits(),
            self.base.conditions,
            self.base.current_tier
        )
    }

    /// `SessionResultProcessor::processDragResults` @ 0x140166d70: the places (player first
    /// with more than half the matches) and every car's matches as laps.
    pub fn results(&self) -> (Vec<i32>, Vec<Vec<Lap>>) {
        let wins = self.cars.first().map_or(0, |c| c.wins);
        let positions = if (self.match_number as f32 + 0.5) * 0.5 < wins as f32 { vec![0, 1] } else { vec![1, 0] };
        (positions, self.lap_times.clone())
    }

    /// `race_out.json`'s `{"name":"drag","total","wins","runs","reaction_time","max_speed"}`.
    pub fn extra(&self) -> Node {
        let first = self.cars.first();
        let mut reaction = first.map_or(0.0, |c| c.lowest_reaction_time) * 1000.0;
        if reaction > 100000.0 {
            reaction = -1.0;
        }
        Node::Object(vec![
            ("name".to_string(), Node::Text("drag".to_string())),
            ("total".to_string(), Node::int(self.match_number as i32 as i64)),
            ("wins".to_string(), Node::int(first.map_or(0, |c| c.wins) as i32 as i64)),
            ("runs".to_string(), Node::int(self.runs() as i32 as i64)),
            ("reaction_time".to_string(), Node::float(reaction)),
            ("max_speed".to_string(), Node::int(first.map_or(0, |c| c.max_speed) as i64)),
        ])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Host {
        rand: crate::session::MsvcRand,
        calls: Vec<String>,
    }

    impl DragHost for Host {
        fn rand(&mut self) -> i32 {
            self.rand.next()
        }
        fn is_ai(&self, car: usize) -> bool {
            car == 1
        }
        fn ai_start_time(&mut self, car: usize, time: f64) {
            self.rand.next();
            self.calls.push(format!("ai {car} {time}"));
        }
        fn gentle_stop(&mut self, _: usize, _: bool) {}
        fn go_to_spawn(&mut self, car: usize) {
            self.calls.push(format!("spawn {car}"));
        }
        fn show_esc_menu(&mut self) {}
        fn fade_in(&mut self, _: f32, _: bool) -> bool {
            false
        }
    }

    #[test]
    fn the_first_to_the_line_wins_and_a_jump_start_loses() {
        let mut host = Host { rand: crate::session::MsvcRand(1), calls: Vec::new() };
        let spawns = [[-2.0, 0.0, 0.0], [2.0, 0.0, 0.0]];
        let mut d = DragMode::new(GameMode::default(), 3, [spawns[0], spawns[1]], [[-5.0, 0.0, 400.0], [5.0, 0.0, 400.0]], &spawns, 0.0, &mut host);
        assert_eq!(d.cars.iter().map(|c| c.spline_side).collect::<Vec<_>>(), [-1.0, 1.0]);
        d.new_session(0.0, &mut host);
        let mut f = DragFrame { game_time: 0.0, positions: spawns.to_vec(), state_positions: spawns.to_vec(), ..Default::default() };
        d.update(0.1, &f, &mut host);
        assert_eq!(d.cars[0].still_position, spawns[0]);
        d.drive_pressed();
        f.game_time = d.start_time + 1.0;
        for _ in 0..10 {
            d.update(0.1, &f, &mut host);
        }
        assert!((d.cars[0].player_time - 1.0).abs() < 1e-5);
        d.on_finish_lane_passed(0, 50.0, false, &mut host, &mut |_, _| {});
        d.update(0.1, &f, &mut host);
        d.on_finish_lane_passed(1, 50.0, false, &mut host, &mut |_, _| {});
        assert_eq!((d.cars[0].wins, d.cars[0].final_speed, d.lap_times[0][0].time), (1, 180, 1000));
        // the next match: the car moves before the green
        for _ in 0..30 {
            d.update(0.1, &f, &mut host);
        }
        assert_eq!(d.status, DragState::Still);
        d.update(0.1, &f, &mut host);
        d.drive_pressed();
        f.game_time = d.start_time - 5000.0;
        for _ in 0..11 {
            d.update(0.1, &f, &mut host);
        }
        assert!(!d.cars[0].false_start);
        f.positions[0][2] += 0.5;
        d.update(0.1, &f, &mut host);
        assert!(d.cars[0].false_start && d.cars[1].wins == 1 && d.status == DragState::Suspended);
    }
}
