// SPDX-License-Identifier: MIT OR Apache-2.0
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! The session's mode object (`Sim::Sim` makes one by the first session's type): the
//! special event's goals every mode shares (`GameMode`) and the modes that judge them.
//!
//! * [`GameMode`]: `GameMode::GameMode` @ 0x1400bc810, `setTier` @ 0x1400be990, and the
//!   `special_event` entry of `JsonOutputFile::addExtraSessionInfo` @ 0x14010c5e0.
//! * [`Mode::Hotlap`]: `HotlapMode` (constructor @ 0x1400beac0, its lap handler
//!   @ 0x1400beb80).
//!
//! `evOnTierCompleted` and `evOnLeaderboardScore` have only Steam's achievements and
//! leaderboards behind them in the game, which rustyAC leaves out.

use super::drift::DriftMode;
use super::manager::{SESSION_DRIFT, SESSION_HOTLAP, SESSION_TIME_ATTACK};
use super::time_attack::TimeAttack;
use super::output::Node;
use crate::data::ini::IniReader;
use crate::track::timing::OnLapCompletedEvent;

/// `ConditionDef`: one goal of a special event.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Condition {
    /// `[CONDITION_n] TYPE`
    pub kind: String,
    /// the medal: BRONZE, SILVER, GOLD, GOLDEN GLORY, then MEDAL
    pub name: String,
    /// `[CONDITION_n] OBJECTIVE`
    pub objective: i32,
    pub achieved: bool,
}

/// What every mode keeps of the special event.
#[derive(Clone, Debug, PartialEq)]
pub struct GameMode {
    /// `[SPECIAL_EVENT] GUID`, as text ("" without one)
    pub guid: String,
    pub conditions: Vec<Condition>,
    /// the last goal reached, -1: none
    pub current_tier: i32,
    /// `RaceManager::isOfficialSpecialEvent`: race.ini named one of the install's encrypted
    /// events, which replaced it
    pub official: bool,
    /// not the game's `GameMode`: `cfg/gameplay.ini [TIME_ATTACK] BASE_TIME_MULT` (1 without
    /// the section), which `TimeAttack`'s constructor reads
    pub time_attack_mult: f32,
}

impl Default for GameMode {
    fn default() -> GameMode {
        GameMode { guid: String::new(), conditions: Vec::new(), current_tier: -1, official: false, time_attack_mult: 1.0 }
    }
}

impl GameMode {
    /// The constructor: `race` is the session file as read (the official event's when there
    /// is one, [`IniReader::load_race`]), `documents` the player's own `cfg/race.ini`, where
    /// the goals already reached are (`[CONDITION_n] ACHIEVED`).
    pub fn new(race: &IniReader, documents: &IniReader, official: bool) -> GameMode {
        let mut mode = GameMode { official, ..GameMode::default() };
        if !race.ready || !race.has_section("SPECIAL_EVENT") {
            return mode;
        }
        // the ctor decrypts without looking for the file: a missing one reads "NOT FOUND",
        // whose GUID 0 matches a GUID of 0 (or none) and replaces race.ini with nothing
        let guid = race.get_int("SPECIAL_EVENT", "GUID").unwrap_or(0);
        if !official && guid == 0 && rustyac_content::acd::special_event(0).is_none() {
            return mode;
        }
        mode.guid = race.get_string("SPECIAL_EVENT", "GUID");
        while race.has_section(&format!("CONDITION_{}", mode.conditions.len())) {
            let i = mode.conditions.len();
            let section = format!("CONDITION_{i}");
            mode.conditions.push(Condition {
                kind: race.get_string(&section, "TYPE"),
                name: ["BRONZE", "SILVER", "GOLD", "GOLDEN GLORY"].get(i).unwrap_or(&"MEDAL").to_string(),
                objective: race.get_int(&section, "OBJECTIVE").unwrap_or(0),
                achieved: documents.get_int(&section, "ACHIEVED").unwrap_or(0) > 0,
            });
        }
        mode
    }

    /// The `verifyConditions` of TimeAttack and DriftMode: every goal `score` reaches is
    /// achieved, `title` its message's text.
    pub fn verify(&mut self, title: &str, score: f32, message: &mut dyn FnMut(&str, &str)) {
        for i in 0..self.conditions.len() {
            let c = &mut self.conditions[i];
            if c.objective as f32 <= score && !c.achieved {
                message(title, &format!("{} Achieved", c.name));
                c.achieved = true;
                self.current_tier = i as i32;
            }
        }
    }

    /// `addExtraSessionInfo`'s `special_event` entry (every mode has one, goals or not).
    pub fn extra(&self) -> Node {
        Node::Object(vec![
            ("name".to_string(), Node::Text("special_event".to_string())),
            ("guid".to_string(), Node::escaped(&self.guid)),
            ("max".to_string(), Node::int(self.conditions.len() as i64)),
            ("tier".to_string(), Node::int(self.current_tier as i64)),
        ])
    }
}

/// The session's mode object.
#[derive(Clone, Debug, PartialEq)]
pub enum Mode {
    Hotlap(GameMode),
    TimeAttack(Box<TimeAttack>),
    Drift(Box<DriftMode>),
}

impl Mode {
    /// `Sim::Sim`'s choice by the first session's type; `None`: no mode object.
    pub fn of(session_type: i32, base: &GameMode) -> Option<Mode> {
        match session_type {
            SESSION_HOTLAP => Some(Mode::Hotlap(base.clone())),
            SESSION_TIME_ATTACK => Some(Mode::TimeAttack(Box::new(TimeAttack::new(base.clone(), base.time_attack_mult)))),
            SESSION_DRIFT => Some(Mode::Drift(Box::new(DriftMode::new(base.clone())))),
            _ => None,
        }
    }

    pub fn base(&self) -> &GameMode {
        match self {
            Mode::Hotlap(base) => base,
            Mode::TimeAttack(ta) => &ta.base,
            Mode::Drift(drift) => &drift.base,
        }
    }

    /// The player's car finished a lap (the mode's handler runs after the race manager's).
    /// `message`: `SystemMessage::addMessage(text, description)`.
    pub fn on_lap_completed(&mut self, lap: &OnLapCompletedEvent, message: &mut dyn FnMut(&str, &str)) {
        let Mode::Hotlap(base) = self else { return };
        if lap.lap_time == 0 || !lap.is_valid || lap.cuts != 0 {
            return;
        }
        for i in 0..base.conditions.len() {
            let condition = &mut base.conditions[i];
            // an unsigned compare in the game
            if lap.lap_time < condition.objective as u32 && !condition.achieved {
                message("Hotlap challenge", &format!("{} Achieved", condition.name));
                condition.achieved = true;
                base.current_tier = i as i32;
            }
        }
    }

    /// The entries of `race_out.json`'s `extras` before `bestlap`.
    pub fn extras(&self) -> Vec<Node> {
        match self {
            Mode::Hotlap(base) => vec![base.extra()],
            Mode::TimeAttack(ta) => vec![ta.extra(), ta.base.extra()],
            Mode::Drift(drift) => vec![drift.extra(), drift.base.extra()],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn ini(text: &str) -> IniReader {
        IniReader::from_text(Path::new("race.ini"), text)
    }

    #[test]
    fn hotlap_goals_are_judged_like_the_game() {
        let race = ini("[SPECIAL_EVENT]\nGUID=1\n[CONDITION_0]\nTYPE=TIME\nOBJECTIVE=90000\n[CONDITION_1]\nTYPE=TIME\nOBJECTIVE=87000\n[CONDITION_2]\nTYPE=TIME\nOBJECTIVE=85000\n");
        let documents = ini("[CONDITION_1]\nACHIEVED=1\n");
        let base = GameMode::new(&race, &documents, true);
        assert_eq!(base.conditions.iter().map(|c| (c.name.as_str(), c.achieved)).collect::<Vec<_>>(), [("BRONZE", false), ("SILVER", true), ("GOLD", false)]);
        let mut mode = Mode::of(SESSION_HOTLAP, &base).unwrap();
        let mut said = Vec::new();
        let mut lap = OnLapCompletedEvent { lap_time: 86000, is_valid: true, ..Default::default() };
        lap.cuts = 1;
        mode.on_lap_completed(&lap, &mut |a, b| said.push(format!("{a} {b}")));
        lap.cuts = 0;
        mode.on_lap_completed(&lap, &mut |a, b| said.push(format!("{a} {b}")));
        // SILVER was reached before: only BRONZE is new; the tier is the last new one
        assert_eq!(said, ["Hotlap challenge BRONZE Achieved"]);
        assert_eq!(mode.base().current_tier, 0);
        assert!(Mode::of(1, &base).is_none());
        assert_eq!(GameMode::new(&ini("[RACE]\n"), &ini(""), false), GameMode::default());
    }
}
