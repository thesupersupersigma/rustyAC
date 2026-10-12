// SPDX-License-Identifier: GPL-3.0-or-later

//! Task 28: the referee in the game. `rustyac_physics::race` is AC's race rules; here they
//! meet the game's cars (`GameSim`) and its display: the session, the flags, the penalty
//! messages and the results in words, until AC's own HUD is ported.

use rustyac_physics::ai::{SessionInfo, SoloAi};
use rustyac_physics::car::{RollingChassis, VanillaCar};
use rustyac_physics::engine::{CarSlot, DeviceRef, PhysicsEngine};
use rustyac_physics::race::flags::Flag;
use rustyac_physics::race::hud;
use rustyac_physics::race::manager::{SESSION_PRACTICE, SESSION_QUALIFY, SESSION_RACE};
use rustyac_physics::race::referee::{Referee, World};
use rustyac_physics::session::{generate_wind, WindSettings};
use rustyac_physics::track::timing::time_to_string;

use crate::input_file::SimSetup;
use crate::sim::{Driver, OpponentCar};

/// The game's cars as the referee reaches them.
pub struct SimWorld<'a> {
    pub car: &'a mut VanillaCar<Driver>,
    pub opponents: &'a mut Vec<OpponentCar>,
    pub engine: &'a mut Option<PhysicsEngine>,
    pub solo_ai: &'a mut SoloAi,
    pub setup: &'a mut SimSetup,
    pub session_index: &'a mut i32,
}

impl SimWorld<'_> {
    fn each_env(&mut self, mut f: impl FnMut(&mut rustyac_physics::car::ChassisEnvironment)) {
        if let Some(engine) = self.engine.as_mut() {
            f(&mut engine.env);
        }
        f(&mut self.car.car.env);
        for opponent in self.opponents.iter_mut() {
            f(&mut opponent.car.car.env);
        }
    }
}

impl World for SimWorld<'_> {
    fn car_count(&self) -> usize {
        1 + self.opponents.len()
    }

    fn car(&self, index: usize) -> &RollingChassis {
        if index == 0 {
            &self.car.car
        } else {
            &self.opponents[index - 1].car.car
        }
    }

    fn car_mut(&mut self, index: usize) -> &mut RollingChassis {
        if index == 0 {
            &mut self.car.car
        } else {
            &mut self.opponents[index - 1].car.car
        }
    }

    fn set_session_info(&mut self, info: SessionInfo) {
        // a later session starts from the track as the one before left it (the handler the
        // track hangs on the engine's new-session event, lambda @ 0x140277740)
        if info.index > 0 && self.setup.session_transfer {
            let track = match &self.engine {
                Some(engine) => engine.dynamic_track,
                None => self.car.car.dynamic_track,
            };
            if let Some(mut track) = track {
                let mut rand = rustyac_physics::session::MsvcRand(self.setup.seed.wrapping_add(info.index as u32));
                track.on_new_session(info.index, rand.next());
                track.step(0);
                let grip = track.dynamic_grip_level;
                self.each_env(|env| env.dynamic_grip_level = grip);
                self.setup.env.dynamic_grip_level = grip;
                self.setup.session.dynamic_track = Some(track);
                match self.engine.as_mut() {
                    Some(engine) => engine.dynamic_track = Some(track),
                    None => self.car.car.dynamic_track = Some(track),
                }
            }
        }
        *self.session_index = info.index;
        self.setup.env.session_type = info.session_type;
        self.setup.env.session_start_time_ms = info.start_time_ms;
        self.setup.ai.laps = info.laps;
        self.setup.ai.time_secs = info.time_secs;
        match self.engine.as_mut() {
            Some(engine) => {
                let mut slots = Vec::with_capacity(1 + self.opponents.len());
                slots.push(CarSlot { car: &mut self.car.car, device: DeviceRef::Here(&mut self.car.device) });
                for opponent in self.opponents.iter_mut() {
                    slots.push(CarSlot { car: &mut opponent.car.car, device: DeviceRef::Anywhere(&mut opponent.car.device) });
                }
                engine.set_session(info, &mut slots);
                engine.reset_collisions_for_new_session();
            }
            None => {
                let car = &mut self.car.car;
                car.env.session_type = info.session_type;
                car.env.session_start_time_ms = info.start_time_ms;
                car.is_retired = false;
                if car.black_flagged {
                    car.set_black_flag_for(false, 0);
                }
                car.fuel_lap_evaluator.on_new_session();
                self.solo_ai.session = info;
                if self.car.device.ai.is_some() {
                    use rustyac_physics::car::ControlsProvider;
                    self.solo_ai.give(&mut self.car.car);
                    self.car.device.on_new_session(&mut self.car.car, &info);
                    self.solo_ai.take(&mut self.car.car);
                }
                self.car.car.reset_collisions_for_new_session();
            }
        }
    }

    fn set_session_start_time(&mut self, ms: f64) {
        self.each_env(|env| env.session_start_time_ms = ms);
        self.setup.env.session_start_time_ms = ms;
        if let Some(engine) = self.engine.as_mut() {
            engine.session.start_time_ms = ms;
        }
        self.solo_ai.session.start_time_ms = ms;
    }

    fn set_penalty_mode(&mut self, mode: i32) {
        self.each_env(|env| env.penalty_mode = mode);
        self.setup.env.penalty_mode = mode;
    }

    fn generate_wind(&mut self) {
        let Some((base_speed, base_direction)) = self.setup.referee.as_ref().and_then(|referee| referee.wind_base) else {
            return;
        };
        // (a job of the physics thread: its rand(), the AI drivers' one)
        let mut rand = match &self.engine {
            Some(engine) => engine.ai_rand,
            None => self.solo_ai.rand,
        };
        let wind = generate_wind(&WindSettings { base_speed, base_direction }, &mut || rand.next());
        match self.engine.as_mut() {
            Some(engine) => engine.ai_rand = rand,
            None => self.solo_ai.rand = rand,
        }
        if let Some((speed, direction)) = wind {
            self.each_env(|env| env.set_wind(speed, direction));
            self.setup.session.wind_speed = speed;
            self.setup.session.wind_direction_deg = direction;
        }
    }
}

/// What the referee says, in words, for the display.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RefereeHud {
    /// the session's name and number
    pub session: String,
    /// the banner at the top of the screen (the time or the laps left, FINAL LAP, ...)
    pub banner: String,
    /// position, laps, last and best lap, sectors
    pub lines: Vec<String>,
    /// the flags that are shown, in words
    pub flags: Vec<String>,
    /// the caption under a black flag
    pub caption: String,
    /// the lines in the middle of the screen (the penalty messages, "Session Over")
    pub centre: Vec<String>,
    /// the results table when the panel is up
    pub results: Vec<String>,
    /// ms to the green light of a race (negative: it is on); `None` outside a race
    pub time_to_start_ms: Option<f64>,
    /// the picture is fading to black: 0 none .. 1 black
    pub fade: f32,
}

fn session_name(kind: i32) -> &'static str {
    match kind {
        SESSION_PRACTICE => "PRACTICE",
        SESSION_QUALIFY => "QUALIFY",
        SESSION_RACE => "RACE",
        4 => "HOTLAP",
        5 => "TIME ATTACK",
        6 => "DRIFT",
        7 => "DRAG",
        _ => "SESSION",
    }
}

/// The results of a session as a table.
pub fn results_table(referee: &Referee, event: &rustyac_physics::race::OnSessionEndEvent) -> Vec<String> {
    let mut lines = vec![format!("{} - RESULTS", if event.current_session.name.is_empty() { session_name(event.current_session.session_type) } else { &event.current_session.name })];
    let race = event.current_session.session_type == SESSION_RACE;
    for (place, car) in event.result.positions.iter().enumerate() {
        let car = *car as usize;
        let Some(player) = referee.players.get(car) else { continue };
        let laps = event.result.laps.get(car).map_or(0, Vec::len);
        let best = event.result.laps.get(car).and_then(|laps| laps.iter().filter(|lap| lap.is_valid && lap.time != 0).map(|lap| lap.time).min()).unwrap_or(0);
        let detail = if race {
            format!("{laps} laps  total {}", time_to_string(event.result.total.get(car).copied().unwrap_or(0.0) as i32))
        } else {
            format!("{laps} laps")
        };
        lines.push(format!("{:>2}  {:<24} {:<22} best {}  {detail}", place + 1, player.name, player.car, time_to_string(best as i32)));
    }
    lines
}

/// The end of the program: the last session's results are made, `race_out.json` and
/// `laps.ini` are written into `out` (rustyAC's own folder, never Assetto Corsa's), and what
/// happened comes back as lines for the console. `None` without a referee.
pub fn finish(sim: &mut crate::sim::GameSim, out: Option<&std::path::Path>) -> Option<Vec<String>> {
    let (json, laps, table) = sim.finish_referee()?;
    let mut lines = table;
    let hash = |bytes: &[u8]| bytes.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ *b as u64).wrapping_mul(0x0000_0100_0000_01b3));
    lines.push(format!("referee: race_out.json {} bytes (hash {:016x}), laps.ini {} bytes (hash {:016x})", json.len(), hash(&json), laps.len(), hash(&laps)));
    if let Some(out) = out {
        let written = std::fs::create_dir_all(out).and_then(|()| std::fs::write(out.join("race_out.json"), &json)).and_then(|()| std::fs::write(out.join("laps.ini"), &laps));
        match written {
            Ok(()) => lines.push(format!("referee: wrote {} and {}", out.join("race_out.json").display(), out.join("laps.ini").display())),
            Err(error) => lines.push(format!("referee: could not write into {}: {error}", out.display())),
        }
    }
    Some(lines)
}

/// The referee's words for this moment.
pub fn hud_of(referee: &Referee, player: &RollingChassis) -> RefereeHud {
    let manager = &referee.manager;
    let session = &manager.current_session;
    let state = Referee::player_state(player);
    let mut out = RefereeHud {
        session: format!("{} ({}/{})", if session.name.is_empty() { session_name(session.session_type) } else { &session.name }, manager.current_session_index + 1, manager.sessions.len()),
        banner: hud::session_banner(manager, &state, manager.results_displayed),
        ..RefereeHud::default()
    };
    if session.session_type == SESSION_RACE {
        out.time_to_start_ms = Some(manager.time_to_session_start());
    }
    let cars = manager.car_count();
    if cars > 0 && !referee.views.is_empty() {
        let place = if session.session_type == SESSION_RACE { manager.car_real_time_position(0, &referee.views) + 1 } else { manager.car_leaderboard_position(0, &referee.views) };
        let db = &manager.timing.lap_dbs[0];
        out.lines.push(format!("POS {place}/{cars}   LAPS {}", db.laps.len()));
        out.lines.push(format!("LAST {}   BEST {}", time_to_string(db.laps.last().map_or(0, |lap| lap.time) as i32), time_to_string(db.best_lap.time as i32)));
        let sectors: Vec<String> = db.current_splits.iter().enumerate().map(|(i, ms)| format!("S{} {}", i + 1, time_to_string(*ms as i32))).collect();
        if !sectors.is_empty() {
            out.lines.push(sectors.join("  "));
        }
    }
    for flag in referee.shown.list() {
        out.flags.push(flag.words().to_string());
        if flag == Flag::Black {
            out.caption = referee.flags.black_flag_caption.to_string();
        }
    }
    for message in &referee.message_lines {
        out.centre.push(message[0].clone());
        out.centre.push(message[1].clone());
    }
    out.centre.extend(hud::manager_lines(manager, &referee.views, &state, manager.results_displayed));
    if manager.results_displayed {
        if let Some(event) = referee.last_results.as_ref().filter(|_| manager.finalized) {
            out.results = results_table(referee, event);
        } else {
            // the race is over and its results are not made yet: the order on the track
            out.results.push(format!("{} - OVER", if session.name.is_empty() { session_name(session.session_type) } else { &session.name }));
            for (place, entry) in manager.timing.leaderboard.iter().enumerate() {
                if let Some(player) = referee.players.get(entry.car) {
                    out.results.push(format!("{:>2}  {:<24} {:<22} best {}  {} laps", place + 1, player.name, player.car, time_to_string(entry.best_lap as i32), entry.laps));
                }
            }
        }
    }
    if let Some((left, job)) = manager.fade {
        let length = if job == rustyac_physics::race::FadeJob::Restart { 1.0 } else { rustyac_physics::race::manager::FADE_SECONDS };
        out.fade = (1.0 - left / length).clamp(0.0, 1.0);
    }
    out
}
