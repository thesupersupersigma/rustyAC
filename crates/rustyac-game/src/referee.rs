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
    /// Task 28b: the pit menu while it is up, line by line (the cursor's line starts with `>`)
    pub pit: Vec<String>,
    /// Task 29: the same moment as AC's HUD objects read it
    pub gui: RefereeGui,
    /// Task 30: the pit menu and the stop, for AC's own labels and panel
    pub pit_gui: Option<PitGui>,
    /// Task 30: the standings as AC's tables read them (`RaceManager`'s answers; the cars'
    /// names and looks are the picture's side's to add)
    pub race: Option<rustyac_gui::leaderboard::RaceLink>,
    /// Task 31: every car's laps of the running session and of the ones that are over, for
    /// the ESC menu's page "Time"
    pub time: Option<rustyac_gui::time_table::TimeLink>,
    /// Task 30: what the ESC menu's "Race control" says of the day: the track's grip, the
    /// air's and the road's temperature, the wind's speed (m/s) and direction (degrees)
    pub env: [f32; 5],
}

/// What AC's pit menu (`QuickMenuPitstop`) and `PitStop::renderHUD` show: the menu as it is,
/// what its labels read of the car and the session, and the stop's state and clocks
/// (`pitTimer`, `maxPitTimer`, `tyreTime`, `repairTime`, `fuelTime`).
#[derive(Clone, Debug, PartialEq)]
pub struct PitGui {
    pub menu: rustyac_physics::race::QuickMenu,
    pub car: rustyac_gui::pit_menu::PitMenuCar,
    pub in_pitlane: bool,
    pub state: i32,
    pub times: [f32; 5],
}

/// What AC's HUD objects read of the session through `Sim` (see `rustyac_gui::hud::HudInput`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RefereeGui {
    pub session_type: i32,
    /// `RaceManager::getPracticeQualifyText`, `getRaceText`
    pub practice_qualify_text: String,
    pub race_text: String,
    /// `RaceManager::getTimeToSessionStart`, ms
    pub time_to_session_start: f64,
    /// `RaceManager::isRaceOver(0)`
    pub race_over: bool,
    /// `RaceManager::getSessionTimeLeft`, ms
    pub session_time_left: f64,
    /// the results are up (`Sim::isDisplayingResults`)
    pub results_displayed: bool,
    /// the flags `FlagManager::update` shows: blue, black, yellow, penalty, chequered
    pub flags: [bool; 5],
    /// the caption under the black flag
    pub caption: String,
    /// the player's car: `hasPenalty()`, `disableMinSpeedPenaltyClear`, the ms its controls
    /// stay locked, `pitPenaltyLaps`, `isCurrentLapValid()`, `isBlackFlagged`
    pub penalty_time: f64,
    pub min_speed_penalty_clear_disabled: bool,
    pub lock_controls_time: f32,
    pub pit_penalty_laps: i16,
    pub current_lap_valid: bool,
    pub is_black_flagged: bool,
    /// how much the player's car points against the road (over 0.4: the wrong way)
    pub against_road: f32,
    /// every car: its place on the leaderboard, its place on the road, its laps, its last lap (ms)
    pub cars: Vec<(i32, i32, i32, u32)>,
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
    // Task 28b: the player's pit stops, and the mandatory one
    if let Some(referee) = sim.referee.as_ref().filter(|referee| referee.pit.is_some()) {
        let stops: Vec<&str> = referee.log.iter().filter_map(|line| line.strip_prefix("Pit stop: ")).collect();
        let car = &sim.car.car;
        let compound = car.tyres.first().and_then(|tyre| tyre.compound_defs.get(tyre.current_compound_index.max(0) as usize)).map(|def| def.short_name.clone()).unwrap_or_default();
        lines.push(format!("pit stops of your car: {}{}; now {:.1} l of fuel, tyres {compound}", stops.len(), if stops.is_empty() { String::new() } else { format!(" ({})", stops.join(", ")) }, car.fuel));
        if let Some(menu) = &referee.menu {
            let items: Vec<String> = menu.items.iter().map(|item| format!("{} {}", item.title.to_lowercase(), menu.value_text(item))).collect();
            lines.push(format!("pit menu (preset {} of {}): {}", menu.current_preset + 1, menu.presets_count, items.join(", ")));
        }
        if referee.pit_window.1 != 0 {
            lines.push(format!("mandatory pit stop: {}", if referee.pit_avatar.mandatory_pitstop_done { "done" } else if car.black_flagged { "not done: black flag" } else { "not done" }));
        }
    }
    if let Some(out) = out {
        let written = std::fs::create_dir_all(out).and_then(|()| std::fs::write(out.join("race_out.json"), &json)).and_then(|()| std::fs::write(out.join("laps.ini"), &laps));
        match written {
            Ok(()) => lines.push(format!("referee: wrote {} and {}", out.join("race_out.json").display(), out.join("laps.ini").display())),
            Err(error) => lines.push(format!("referee: could not write into {}: {error}", out.display())),
        }
    }
    Some(lines)
}

/// Task 28b: the pit stop in words, in three groups: the mandatory pit window (a line for
/// the top of the screen), the stop's countdown with what is being done (the middle), and
/// the menu while it is up (AC's own menu comes with the UI).
pub struct PitLines {
    pub window: Vec<String>,
    pub countdown: Vec<String>,
    pub menu: Vec<String>,
}

impl PitLines {
    /// All of them, one after the other.
    pub fn all(self) -> Vec<String> {
        self.window.into_iter().chain(self.countdown).chain(self.menu).collect()
    }
}

pub fn pit_lines(referee: &Referee) -> PitLines {
    let (Some(pit), Some(menu)) = (&referee.pit, &referee.menu) else { return PitLines { window: Vec::new(), countdown: Vec::new(), menu: Vec::new() } };
    pit_lines_of(pit, menu, referee.pit_window, &referee.pit_avatar, &referee.manager)
}

/// [`pit_lines`] of a stop, its menu, the mandatory window and the manager that keeps the
/// window's times (online the manager is only that).
pub fn pit_lines_of(pit: &rustyac_physics::race::PitStop, menu: &rustyac_physics::race::QuickMenu, window: (u16, u16), avatar: &rustyac_physics::race::PitAvatar, manager: &rustyac_physics::race::RaceManager) -> PitLines {
    use rustyac_physics::race::pitstop::STATE_OPERATING;
    let mut lines = PitLines { window: Vec::new(), countdown: Vec::new(), menu: Vec::new() };
    let out = &mut lines.window;
    if window.1 != 0 && manager.current_session.session_type == SESSION_RACE {
        let unit = if manager.current_session.is_timed_race { "min" } else { "lap" };
        let state = if avatar.mandatory_pitstop_done {
            "DONE".to_string()
        } else if manager.pit_window_end_time != 0.0 {
            "CLOSED".to_string()
        } else if manager.pit_window_start_time > 0.0 {
            "OPEN".to_string()
        } else {
            format!("opens at {unit} {}", window.0)
        };
        out.push(format!("MANDATORY PIT: {state}  (PIT OPEN: {unit} {}  PIT CLOSE: {unit} {})", window.0, window.1));
    }
    if pit.state == STATE_OPERATING {
        let out = &mut lines.countdown;
        out.push(format!("PIT STOP  {:.1} s", pit.pit_timer.max(0.0)));
        let mut work = Vec::new();
        if pit.is_changing_tyres() {
            work.push("TYRES".to_string());
        }
        if pit.fuel_time > 0.0 {
            work.push(format!("FUEL {:.0}%", pit.get_fueling_percentage() * 100.0));
        }
        if pit.repair_time > 0.0 {
            work.push(format!("REPAIR {:.0}%", pit.get_repairing_percentage() * 100.0));
        }
        out.push(work.join("   "));
    } else if menu.is_visible {
        lines.menu.extend(menu.lines());
        lines.menu.push("number pad 8 2: line, 4 6: change (or Alt + arrows)".to_string());
        lines.menu.push("stop in your pit box to begin".to_string());
    }
    lines
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
    out.gui = RefereeGui {
        session_type: session.session_type,
        practice_qualify_text: hud::practice_qualify_text(manager),
        race_text: hud::race_text(manager),
        time_to_session_start: manager.time_to_session_start(),
        race_over: !referee.views.is_empty() && manager.is_race_over(0, &referee.views),
        session_time_left: manager.session_time_left(),
        results_displayed: manager.results_displayed,
        flags: [referee.shown.blue, referee.shown.black, referee.shown.yellow, referee.shown.penalty, referee.shown.chequered],
        caption: if referee.shown.black { referee.flags.black_flag_caption.to_string() } else { String::new() },
        penalty_time: state.penalty_time,
        min_speed_penalty_clear_disabled: state.min_speed_clear_disabled,
        lock_controls_time: state.lock_controls_time as f32,
        pit_penalty_laps: state.pit_penalty_laps,
        current_lap_valid: state.current_lap_valid,
        is_black_flagged: state.is_black_flagged,
        against_road: referee.player_against_road,
        cars: if referee.views.is_empty() {
            Vec::new()
        } else {
            (0..cars).map(|car| (manager.car_leaderboard_position(car, &referee.views), manager.car_real_time_position(car, &referee.views), manager.lap_count(car) as i32, manager.timing.lap_dbs.get(car).and_then(|db| db.laps.last()).map_or(0, |lap| lap.time))).collect()
        },
    };
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
    let mut pit = pit_lines(referee);
    if let (Some(menu), false) = (&referee.menu, pit.menu.is_empty()) {
        // (the estimate the game's mouse menu shows; the quick menu has none)
        let at = pit.menu.len() - 2;
        pit.menu.insert(at, format!("  ESTIMATED TIME         {:.1} s", menu.estimate(player)));
    }
    out.lines.extend(pit.window);
    out.centre.extend(pit.countdown);
    out.pit = pit.menu;
    {
        use rustyac_gui::leaderboard::{RaceCar, RaceEntry, RaceLap, RaceLink, RaceSession};
        let entry = |e: &rustyac_physics::race::timing::LeaderboardEntry| RaceEntry { car: e.car, total_time: e.total_time, best_lap: e.best_lap, laps: e.laps, has_completed_last_lap: e.has_completed_last_lap };
        out.race = Some(RaceLink {
            cars: manager
                .timing
                .lap_dbs
                .iter()
                .enumerate()
                .map(|(car, db)| RaceCar {
                    is_black_flagged: referee.views.get(car).is_some_and(|view| view.is_black_flagged),
                    // RaceTimingServices::getLastLap: the last lap of the list, or none
                    last_lap: db.laps.last().map(|lap| RaceLap { time: lap.time, splits: lap.splits.clone(), is_valid: lap.is_valid }).unwrap_or_default(),
                    best_lap: db.best_lap.time,
                    has_completed_flag: db.has_completed_last_lap,
                    current_splits: db.current_splits.clone(),
                    personal_best_splits: db.personal_best_splits.clone(),
                    // RaceManager::getLapCount
                    lap_count: db.laps.len() as u32,
                    ..RaceCar::default()
                })
                .collect(),
            sessions: manager
                .sessions
                .iter()
                .enumerate()
                .map(|(i, s)| {
                    // (the running session's start is the manager's own copy's)
                    let s = if i as i32 == manager.current_session_index { &manager.current_session } else { s };
                    RaceSession { session_type: s.session_type, is_timed_race: s.is_timed_race, name: s.name.clone(), laps: s.laps, duration_minutes: s.duration_minutes, start_time: s.start_time }
                })
                .collect(),
            current_session_index: manager.current_session_index,
            leaderboard: manager.timing.leaderboard.iter().map(entry).collect(),
            leaderboard_history: manager.leaderboard_history.iter().map(|board| board.iter().map(entry).collect()).collect(),
            global_best_lap: manager.timing.global_best_lap,
            best_splits: manager.timing.best_splits.clone(),
            ..RaceLink::default()
        });
        {
            use rustyac_gui::time_table::{TimeHistory, TimeLap, TimeLink};
            let lap = |lap: &rustyac_physics::race::timing::Lap| TimeLap { time: lap.time, splits: lap.splits.clone(), cuts: lap.cuts, is_valid: lap.is_valid, compound: lap.compound.clone() };
            out.time = Some(TimeLink {
                laps: manager.timing.lap_dbs.iter().map(|db| db.laps.iter().map(lap).collect()).collect(),
                history: manager.session_history_list.iter().map(|event| TimeHistory { laps: event.result.laps.iter().map(|laps| laps.iter().map(lap).collect()).collect(), best_laps: event.result.best_laps.iter().map(lap).collect() }).collect(),
                sector_count: manager.timing.sector_count,
                ..TimeLink::default()
            });
        }
        let env = &player.env;
        out.env = [env.dynamic_grip_level, env.ambient_temperature, env.road_temperature, env.wind_speed, env.wind_direction_deg];
    }
    if let (Some(stop), Some(menu)) = (&referee.pit, &referee.menu) {
        let car = rustyac_gui::pit_menu::PitMenuCar {
            has_race_engineer: true,
            // CarAvatar::getTyreCompound(index, false): the compounds' short names
            compounds: player.tyres.first().map(|tyre| tyre.compound_defs.iter().map(|def| def.short_name.clone()).collect()).unwrap_or_default(),
            wing_angle: rustyac_physics::race::QuickMenuCar::of(player).wing_angle,
            fuel_per_lap: player.fuel_lap_evaluator.fuel_per_lap(),
            lap_count: manager.lap_count(0),
            normalized_spline_position: player.spline_locator_data.npos,
            session_type: session.session_type,
            is_timed_race: session.is_timed_race,
            // RaceManager::getSessionTotalTime(0) offline: RaceTimingServices::getTotalTime
            session_total_time: manager.timing.lap_dbs.first().map_or(0, |db| db.total_time as i32),
        };
        out.pit_gui = Some(PitGui { menu: menu.clone(), car, in_pitlane: referee.pit_avatar.in_pitlane, state: stop.state, times: [stop.pit_timer, stop.max_pit_timer, stop.tyre_time, stop.repair_time, stop.fuel_time] });
    }
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
