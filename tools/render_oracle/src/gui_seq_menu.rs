// SPDX-License-Identifier: GPL-3.0-or-later

//! The GUI oracle's scripts for the game's menus (Task 30), and what both sides share of them:
//! the script's own state of the session, and the list of calls a menu made on the session
//! (the game's side writes them down in stand-ins for `Sim`'s functions, the port's side reads
//! them from `Gui::sim.actions`), which is compared like any other state.

use std::sync::Mutex;

use crate::gui_seq::Op;

/// What a script says of the session that the HUD's own state does not hold.
#[derive(Clone, Debug)]
pub struct MenuScript {
    /// `RaceManager::getSessionCount`
    pub session_count: u32,
    /// the game object "REPLAY_MANAGER" is there
    pub replay_manager: bool,
    /// the player's car as the pit menu reads it
    pub pit: PitScript,
}

/// What a script says of the player's car for the pit menu.
#[derive(Clone, Debug)]
pub struct PitScript {
    /// `physicsInfo.maxFuel`
    pub max_fuel: f32,
    /// the compounds' names (`CarAvatar::getTyreCompound`)
    pub compounds: Vec<String>,
    /// `CarAvatar::getCurrentCompoundStaticPressure` of the four wheels
    pub pressure: [i32; 4],
    /// `CarAvatar::getWingAngle` by the wing's number
    pub wing: [f32; 8],
    pub in_pitlane: bool,
    pub fuel_per_lap: f32,
    /// `physicsState.normalizedSplinePosition`
    pub spline: f32,
    /// `RaceManager::getSessionTotalTime(0)`, and the session's `isTimedRace`
    pub total_time: i32,
    pub timed: bool,
    /// `PitStop`: its state, and pitTimer, maxPitTimer, tyreTime, repairTime, fuelTime
    pub state: i32,
    pub times: [f32; 5],
}

static SCRIPT: Mutex<MenuScript> = Mutex::new(MenuScript {
    session_count: 1,
    replay_manager: false,
    pit: PitScript { max_fuel: 0.0, compounds: Vec::new(), pressure: [0; 4], wing: [0.0; 8], in_pitlane: false, fuel_per_lap: 0.0, spline: 0.0, total_time: 0, timed: false, state: 0, times: [0.0; 5] },
});

/// The car's `data/setup.ini` of the pit menu's scripts (made up: no car of the game has it).
pub const PIT_SETUP_INI: &str = "[TYRES]
NAME=Tyres

[PRESSURE_LF]
SHOW_CLICKS=0
TAB=TYRES
NAME=Pressure LF
MIN=10
MAX=40
STEP=1

[PRESSURE_RF]
NAME=Pressure RF
MIN=10
MAX=40
STEP=1

[PRESSURE_LR]
NAME=Pressure LR
MIN=12
MAX=38
STEP=1

[PRESSURE_RR]
NAME=Pressure RR
MIN=12
MAX=38
STEP=1

[WING_1]
NAME=Front wing
MIN=0
MAX=10
STEP=1
PITSTOP=3.5

[WING_2]
NAME=Rear wing (no stop)
MIN=0
MAX=20
STEP=1

[WING_3]
NAME=Rear wing
MIN=2
MAX=14
STEP=1
PITSTOP=4

";
/// … of a car with fewer parts: two pressures, no wing.
pub const PIT_SETUP_INI_SMALL: &str = "[PRESSURE_RF]
NAME=front right
MIN=10
MAX=40

[PRESSURE_LR]
NAME=rear left
MIN=12
MAX=38

";
static ACTIONS: Mutex<Vec<String>> = Mutex::new(Vec::new());

pub fn script() -> MenuScript {
    let mut script = SCRIPT.lock().unwrap().clone();
    // (sessions the script named count before `menu.sessions`)
    let named = race_sessions();
    if named > 0 {
        script.session_count = named as u32;
    }
    script
}

/// A script's change of the menus' session state (true: it was one).
pub fn menu_input(what: &str, number: f32, _text: &str) -> bool {
    if what.starts_with("race.") {
        return race_input(what, number, _text);
    }
    let mut s = SCRIPT.lock().unwrap();
    match what {
        "menu.sessions" => s.session_count = number as u32,
        "menu.replayManager" => s.replay_manager = number != 0.0,
        "pit.maxFuel" => s.pit.max_fuel = number,
        "pit.compounds" => s.pit.compounds = _text.split('|').filter(|c| !c.is_empty()).map(str::to_string).collect(),
        "pit.inPitlane" => s.pit.in_pitlane = number != 0.0,
        "pit.fuelPerLap" => s.pit.fuel_per_lap = number,
        "pit.spline" => s.pit.spline = number,
        "pit.totalTime" => s.pit.total_time = number as i32,
        "pit.timed" => s.pit.timed = number != 0.0,
        "pit.state" => s.pit.state = number as i32,
        "pit.times" => {
            for (slot, word) in s.pit.times.iter_mut().zip(_text.split(',')) {
                *slot = word.trim().parse().unwrap_or(0.0);
            }
        }
        _ if what.starts_with("pit.pressure") => {
            if let Some(slot) = what[12..].parse::<usize>().ok().and_then(|k| s.pit.pressure.get_mut(k)) {
                *slot = number as i32;
            }
        }
        _ if what.starts_with("pit.wing") => {
            if let Some(slot) = what[8..].parse::<usize>().ok().and_then(|k| s.pit.wing.get_mut(k)) {
                *slot = number;
            }
        }
        _ => return false,
    }
    true
}

/// A call on the session, written down.
pub fn action(text: String) {
    ACTIONS.lock().unwrap().push(text);
}

pub fn dump_actions(out: &mut String) {
    for action in ACTIONS.lock().unwrap().drain(..) {
        out.push_str(&format!("action {action}\n"));
    }
}

const DT: f32 = 1.0 / 60.0;

fn hud(what: &'static str, number: f32, text: &'static str) -> Op {
    Op::Hud(what, number, text)
}

/// ESC, and the frames in which the menu finds its place.
fn esc(o: &mut Vec<Op>) {
    o.extend([hud("menu.esc", 0.0, ""), Op::Frame(DT), Op::Frame(DT)]);
}

fn click(o: &mut Vec<Op>, button: &'static str) {
    o.extend([hud("menu.hover", 0.0, button), Op::Frame(DT), hud("menu.click", 0.0, button), Op::Frame(DT), Op::Frame(DT)]);
}

/// The pause menu. `kind`: 0 a weekend's practice offline (all six buttons), 1 online (three),
/// 2 a single race offline (five), 3 the menu without a replay manager and without `fades.ini`.
pub fn pause_menu(kind: u32) -> Vec<Op> {
    let mut o = vec![hud("create", 2.0, ""), hud("car.name", 0.0, "Tazio Nuvolari"), hud("car.name", 1.0, "Rudolf Caracciola")];
    match kind {
        0 => o.extend([hud("session", 1.0, ""), hud("pqText", 0.0, "Practice"), hud("menu.sessions", 3.0, ""), hud("menu.replayManager", 1.0, "")]),
        1 => o.extend([hud("session", 2.0, ""), hud("pqText", 0.0, "Qualify"), hud("menu.sessions", 3.0, ""), hud("online", 1.0, ""), hud("menu.replayManager", 1.0, "")]),
        2 => o.extend([hud("session", 3.0, ""), hud("raceText", 0.0, "Race  -  Lap 1/5"), hud("menu.sessions", 1.0, ""), hud("menu.replayManager", 1.0, "")]),
        _ => o.extend([hud("session", 1.0, ""), hud("pqText", 0.0, "Practice"), hud("menu.sessions", 2.0, "")]),
    }
    o.extend([hud("menu.pause", 0.0, ""), Op::Frame(DT), hud("message", 0.0, "ABS|Set on level : 2/3"), Op::Frame(DT)]);
    // ESC: the menu comes up (and the message goes); the mouse over every button, and off them
    esc(&mut o);
    for button in ["RESUME", "PITS", "SKIP", "RESTART", "REPLAY", "EXIT"] {
        o.extend([hud("menu.hover", 0.0, button), Op::Frame(DT)]);
    }
    o.extend([Op::MouseMove(5, 5), Op::Frame(DT)]);
    click(&mut o, "RESUME");
    // back to the pits: the fade, then the car goes to its box and the ESC menu comes
    esc(&mut o);
    click(&mut o, "PITS");
    o.extend([hud("menu.fadeDone", 0.0, ""), Op::Frame(DT), Op::Frame(DT)]);
    esc(&mut o);
    click(&mut o, "SKIP");
    esc(&mut o);
    click(&mut o, "RESTART");
    // the replay: the fade; a second click while it runs ends the first fade there and then
    esc(&mut o);
    click(&mut o, "REPLAY");
    esc(&mut o);
    click(&mut o, "PITS");
    o.extend([hud("menu.fadeDone", 0.0, ""), Op::Frame(DT)]);
    // buttons greyed out do nothing
    esc(&mut o);
    for which in 0..6 {
        o.push(hud("menu.blank", which as f32, "1"));
    }
    o.push(Op::Frame(DT));
    for button in ["RESUME", "PITS", "SKIP", "RESTART", "REPLAY", "EXIT"] {
        click(&mut o, button);
    }
    o.extend([hud("menu.fadeDone", 0.0, ""), Op::Frame(DT)]);
    esc(&mut o);
    for which in 0..6 {
        o.push(hud("menu.blank", which as f32, "0"));
    }
    o.push(Op::Frame(DT));
    // the race comes: no skipping, and no way to the pits before the start
    o.extend([hud("session", 3.0, ""), hud("raceText", 0.0, "Race  -  Lap 1/5"), hud("tts", 5000.0, ""), hud("menu.resume", 0.0, ""), Op::Frame(DT)]);
    esc(&mut o);
    click(&mut o, "PITS");
    click(&mut o, "SKIP");
    esc(&mut o);
    o.push(hud("tts", -1000.0, ""));
    click(&mut o, "PITS");
    o.extend([hud("menu.fadeDone", 0.0, ""), Op::Frame(DT)]);
    esc(&mut o);
    click(&mut o, "EXIT");
    o.push(Op::Frame(DT));
    o
}

/// What both sides write down of the pit menu: floats as their bits.
pub fn pit_line(visible: bool, timer: f32, index: i32, preset: i32, fuel_index: i32, presets: &[Vec<f32>]) -> String {
    let presets: Vec<String> = presets.iter().map(|p| p.iter().map(|v| format!("{:08x}", v.to_bits())).collect::<Vec<_>>().join(",")).collect();
    format!("pit visible={} timer={:08x} index={index} preset={preset} fuel={fuel_index} values={}\n", visible as u8, timer.to_bits(), presets.join("/"))
}

const LEFT: f32 = 37.0;
const UP: f32 = 38.0;
const RIGHT: f32 = 39.0;
const DOWN: f32 = 40.0;

fn keys(o: &mut Vec<Op>, key: f32, count: usize, frame_every: usize) {
    for n in 0..count {
        o.push(hud("menu.pitKey", key, ""));
        if (n + 1) % frame_every == 0 || n + 1 == count {
            o.push(Op::Frame(DT));
        }
    }
}

/// The pit menu of the arrow keys. `small`: a car whose `setup.ini` has two pressures and no
/// wing. `lane`: the car is on the pit lane when the session begins.
pub fn pit_menu(small: bool, lane: bool) -> Vec<Op> {
    let mut o = vec![hud("create", 2.0, ""), hud("car.name", 0.0, "Tazio Nuvolari"), hud("session", 1.0, ""), hud("pqText", 0.0, "Practice")];
    o.extend([hud("pit.maxFuel", 60.0, ""), hud("pit.compounds", 0.0, "Soft|Medium|Hard"), hud("pit.pressure0", 26.0, ""), hud("pit.pressure1", 26.0, ""), hud("pit.pressure2", 24.0, ""), hud("pit.pressure3", 39.0, ""), hud("pit.wing1", 3.0, ""), hud("pit.wing3", 13.0, "")]);
    o.extend([hud("menu.pit", 0.0, if small { "small" } else { "" }), Op::Frame(DT), Op::Frame(DT)]);
    if lane {
        // on the pit lane from the start: the menu comes by itself, its cursor on the fuel
        o.extend([hud("pit.inPitlane", 1.0, ""), Op::Frame(DT), Op::Frame(DT)]);
        keys(&mut o, RIGHT, 2, 1);
        keys(&mut o, DOWN, 1, 1);
        keys(&mut o, RIGHT, 1, 1);
        keys(&mut o, UP, 2, 1);
        keys(&mut o, RIGHT, 1, 1);
        o.extend([hud("pit.inPitlane", 0.0, ""), Op::Frame(DT), Op::Frame(6.0), Op::Frame(DT), Op::Frame(DT)]);
    }
    // the first key only shows the menu; then down the rows, every one changed both ways
    keys(&mut o, DOWN, 1, 1);
    keys(&mut o, UP, 1, 1);
    keys(&mut o, DOWN, 1, 1);
    keys(&mut o, RIGHT, 3, 1);
    keys(&mut o, LEFT, 5, 2);
    keys(&mut o, RIGHT, 2, 2);
    keys(&mut o, DOWN, 1, 1);
    keys(&mut o, RIGHT, 5, 1);
    keys(&mut o, LEFT, 6, 2);
    keys(&mut o, RIGHT, 2, 2);
    for _ in 0..4 {
        keys(&mut o, DOWN, 1, 1);
        keys(&mut o, RIGHT, 3, 3);
        keys(&mut o, LEFT, 22, 11);
        keys(&mut o, RIGHT, 1, 1);
    }
    for _ in 0..5 {
        keys(&mut o, DOWN, 1, 1);
        keys(&mut o, RIGHT, 2, 2);
        keys(&mut o, LEFT, 1, 1);
    }
    keys(&mut o, DOWN, 3, 3);
    // back up to the presets: the second and the third, each with values of its own
    keys(&mut o, UP, 20, 5);
    keys(&mut o, RIGHT, 1, 1);
    keys(&mut o, DOWN, 1, 1);
    keys(&mut o, RIGHT, 7, 7);
    keys(&mut o, UP, 1, 1);
    keys(&mut o, RIGHT, 3, 1);
    keys(&mut o, DOWN, 2, 2);
    keys(&mut o, RIGHT, 2, 2);
    keys(&mut o, UP, 2, 2);
    keys(&mut o, LEFT, 1, 1);
    keys(&mut o, LEFT, 2, 1);
    // how far the fuel goes: in laps, in the minutes of a timed race
    o.extend([hud("pit.fuelPerLap", 2.4, ""), Op::Frame(DT), hud("menu.pitKey", DOWN, ""), hud("menu.pitKey", RIGHT, ""), Op::Frame(DT)]);
    o.extend([hud("session", 3.0, ""), hud("raceText", 0.0, "Race"), hud("pit.timed", 1.0, ""), hud("pit.totalTime", 1_200_000.0, ""), Op::Frame(DT), hud("car.laps", 4.0, "0"), hud("pit.spline", 0.37, ""), Op::Frame(DT), hud("pit.timed", 0.0, ""), Op::Frame(DT)]);
    // nobody touches it: it goes after its time; on the pit lane it comes by itself and stays
    o.extend([Op::Frame(2.5), Op::Frame(2.5), Op::Frame(DT), Op::Frame(DT), hud("pit.inPitlane", 1.0, ""), Op::Frame(DT), Op::Frame(4.0), Op::Frame(4.0), hud("menu.pitKey", DOWN, ""), Op::Frame(DT), hud("pit.inPitlane", 0.0, ""), Op::Frame(3.0), Op::Frame(3.0), Op::Frame(DT), Op::Frame(DT)]);
    // what hides it at once
    let show = |o: &mut Vec<Op>| o.extend([hud("menu.pitKey", UP, ""), Op::Frame(DT)]);
    for (what, on, off, text) in [("pause", 1.0, 0.0, ""), ("esc", 1.0, 0.0, "300"), ("replay", 1.0, 0.0, ""), ("ended", 1.0, 0.0, ""), ("cameraMode", 1.0, 2.0, ""), ("cameraMode", 3.0, 0.0, ""), ("focus", 1.0, 0.0, ""), ("session", 4.0, 2.0, "")] {
        show(&mut o);
        o.extend([hud(what, on, text), Op::Frame(DT), hud(what, off, text), Op::Frame(DT)]);
    }
    show(&mut o);
    o.extend([Op::Set(0, "screen.appInteractionEnabled", crate::gui_seq::Val::B(false)), Op::Frame(DT), Op::Set(0, "screen.appInteractionEnabled", crate::gui_seq::Val::B(true)), Op::Frame(DT)]);
    show(&mut o);
    o.push(Op::Frame(DT));
    o
}

/// The panel of a pit stop that is being worked on: the three bars as the clock runs down.
pub fn pit_stop() -> Vec<Op> {
    let mut o = vec![hud("create", 2.0, ""), hud("car.name", 0.0, "Tazio Nuvolari"), hud("session", 3.0, ""), hud("raceText", 0.0, "Race"), hud("menu.pitStop", 0.0, ""), Op::Frame(DT)];
    o.extend([hud("pit.state", 3.0, ""), hud("menu.pitState", 0.0, "")]);
    for times in ["30,30,8,12,20", "27.5,30,8,12,20", "22,30,8,12,20", "19.3,30,8,12,20", "12,30,8,12,20", "9.9,30,8,12,20", "4.2,30,8,12,20", "0.1,30,8,12,20", "0,30,8,12,20", "-1,30,8,12,20", "5,10,0,0,10", "5,10,10,0,0", "7,7,3,7,5"] {
        o.extend([hud("pit.times", 0.0, times), Op::Frame(DT)]);
    }
    // not in a menu, not in a replay, not after the race; locked controls end it
    for (what, text) in [("pause", ""), ("esc", "300"), ("replay", ""), ("raceOver", "")] {
        o.extend([hud(what, 1.0, text), Op::Frame(DT), hud(what, 0.0, text), Op::Frame(DT)]);
    }
    for state in [0.0, 1.0, 4.0, 5.0, 3.0] {
        o.extend([hud("pit.state", state, ""), hud("menu.pitState", 0.0, ""), Op::Frame(DT)]);
    }
    o.extend([hud("car.lock", 4000.0, "0"), Op::Frame(DT), hud("car.lock", 0.0, "0"), Op::Frame(DT), hud("pit.state", 3.0, ""), hud("menu.pitState", 0.0, ""), Op::Frame(DT)]);
    o
}

// ----- the standings (the tables of `ACStandardLeaderboard` and what hosts it) -----

use rustyac_gui::hud::HudInput;
use rustyac_gui::leaderboard::{RaceCar, RaceEntry, RaceLink, RaceSession};

static RACE: Mutex<Option<RaceLink>> = Mutex::new(None);

fn numbers(text: &str) -> Vec<u32> {
    text.split(',').filter_map(|word| word.trim().parse().ok()).collect()
}

fn entries(text: &str) -> Vec<RaceEntry> {
    text.split(';')
        .filter(|entry| !entry.is_empty())
        .map(|entry| {
            let v: Vec<f64> = entry.split(':').map(|word| word.parse().unwrap_or(0.0)).collect();
            RaceEntry { car: v[0] as usize, total_time: v[1], best_lap: v[2], laps: v[3] as i32, has_completed_last_lap: v[4] != 0.0 }
        })
        .collect()
}

/// A script's change of the standings (true: it was one). A car's own values: the number is
/// the value and the text the car, or the text is the value and the number the car.
pub fn race_input(what: &str, number: f32, text: &str) -> bool {
    let mut guard = RACE.lock().unwrap();
    let race = guard.get_or_insert_with(RaceLink::default);
    let car = |race: &mut RaceLink, index: usize| -> Option<*mut RaceCar> { race.cars.get_mut(index).map(|car| car as *mut RaceCar) };
    match what {
        "race.cars" => race.cars = vec![RaceCar { performance_meter: 0.0, ..RaceCar::default() }; number as usize],
        "race.sessions" => {
            race.sessions = text
                .split('|')
                .filter(|session| !session.is_empty())
                .map(|session| {
                    let mut parts = session.splitn(3, ':');
                    RaceSession { session_type: parts.next().and_then(|t| t.parse().ok()).unwrap_or(0), is_timed_race: parts.next() == Some("1"), name: parts.next().unwrap_or("").to_string() }
                })
                .collect()
        }
        "race.current" => race.current_session_index = number as i32,
        "race.board" => race.leaderboard = entries(text),
        "race.history" => {
            let index = number as usize;
            if race.leaderboard_history.len() <= index {
                race.leaderboard_history.resize(index + 1, Vec::new());
            }
            race.leaderboard_history[index] = entries(text);
        }
        "race.globalBest" => race.global_best_lap = number as u32,
        "race.bestSplits" => race.best_splits = numbers(text),
        // the text is the value, the number the car
        "race.car.skin" | "race.car.shortName" | "race.car.compound" | "race.car.lastSplits" | "race.car.curSplits" | "race.car.pbSplits" => {
            let Some(car) = car(race, number as usize) else { return true };
            // SAFETY: the pointer is into `race`, which is held for the whole function.
            let car = unsafe { &mut *car };
            match what {
                "race.car.skin" => car.skin = text.to_string(),
                "race.car.shortName" => car.gui_short_name = text.to_string(),
                "race.car.compound" => car.compound_short_name = text.to_string(),
                "race.car.lastSplits" => car.last_lap.splits = numbers(text),
                "race.car.curSplits" => car.current_splits = numbers(text),
                _ => car.personal_best_splits = numbers(text),
            }
        }
        _ if what.starts_with("race.car.") => {
            let Some(car) = text.parse::<usize>().ok().and_then(|index| car(race, index)) else { return true };
            // SAFETY: as above.
            let car = unsafe { &mut *car };
            match what {
                "race.car.black" => car.is_black_flagged = number != 0.0,
                "race.car.pit" => car.in_pitlane = number != 0.0,
                "race.car.ballast" => car.ballast_kg = number,
                "race.car.restrictor" => car.restrictor = number,
                "race.car.perf" => car.performance_meter = number,
                "race.car.last" => car.last_lap.time = number as u32,
                "race.car.lastValid" => car.last_lap.is_valid = number != 0.0,
                "race.car.best" => car.best_lap = number as u32,
                "race.car.done" => car.has_completed_flag = number != 0.0,
                _ => return false,
            }
        }
        _ => return false,
    }
    true
}

/// The standings as both sides read them: the script's, with the names, nations, folders,
/// pings and "is connected" of the HUD's cars.
pub fn race_link(input: &HudInput) -> RaceLink {
    let mut race = RACE.lock().unwrap().clone().unwrap_or_default();
    for (car, hud) in race.cars.iter_mut().zip(&input.cars) {
        car.driver_name = hud.driver_name.clone();
        car.nation_code = hud.nation_code.clone();
        car.unix_name = hud.unix_name.clone();
        car.connected = hud.connected;
        car.ping = hud.ping;
    }
    race
}

/// The sessions the script named (0: none, and `menu.sessions` counts).
pub fn race_sessions() -> usize {
    RACE.lock().unwrap().as_ref().map_or(0, |race| race.sessions.len())
}

fn leak(text: String) -> &'static str {
    Box::leak(text.into_boxed_str())
}

const DRIVERS: [&str; 24] = [
    "Tazio Nuvolari", "Rudolf Caracciola", "Bernd Rosemeyer", "Achille Varzi", "Louis Chiron", "Hans Stuck", "Luigi Fagioli", "Manfred von Brauchitsch", "Raymond Sommer", "Jean-Pierre Wimille", "Giuseppe Farina", "Hermann Lang", "Richard Seaman", "Rene Dreyfus", "Piero Taruffi", "Carlo Felice Trossi", "Philippe Etancelin", "Goffredo Zehender", "Earl Howe", "Whitney Straight", "A Driver With A Very Long Name Indeed", "", "Clemente Biondetti", "Christian Kautz",
];
const NATIONS: [&str; 8] = ["ITA", "DEU", "", "FRA", "GBR", "ZZZ", "MCO", "CHE"];
const SKINS: [&str; 4] = ["00_official", "01_ferrari_1", "02_ferrari_2", "no_such_skin"];

/// The cars of the tables' scripts: names, nations, liveries, tyres.
fn field(o: &mut Vec<Op>, cars: usize) {
    o.extend([hud("create", cars as f32, ""), hud("race.cars", cars as f32, ""), hud("session", 1.0, ""), hud("pqText", 0.0, "Practice")]);
    for i in 0..cars {
        let index = leak(i.to_string());
        o.extend([
            hud("car.name", i as f32, DRIVERS[i % DRIVERS.len()]),
            hud("car.nation", i as f32, NATIONS[i % NATIONS.len()]),
            hud("car.unixName", i as f32, if i % 5 == 4 { "no_such_car" } else { "ks_ferrari_f2004" }),
            hud("car.connected", 1.0, index),
            hud("race.car.skin", i as f32, SKINS[i % SKINS.len()]),
            hud("race.car.shortName", i as f32, if i % 3 == 0 { "Ferrari F2004 of the year 2004" } else { "F2004" }),
            hud("race.car.compound", i as f32, ["S", "M", "H", ""][i % 4]),
        ]);
    }
}

/// A board of `cars` entries: `race` by laps and total time, else by best lap; some cars have
/// no time yet. `seed` moves the numbers.
fn board(cars: usize, race: bool, seed: usize) -> &'static str {
    let mut out = Vec::new();
    for place in 0..cars {
        let car = (place * 7 + seed) % cars;
        let laps = if race { 5usize.saturating_sub(place / 5) } else { (place + seed) % 6 };
        let best = if place + 2 >= cars && !race { 0.0 } else { 83456.0 + place as f64 * 417.5 + seed as f64 * 13.0 };
        let total = if race && laps > 0 { laps as f64 * 84000.0 + place as f64 * 1234.5 } else { 0.0 };
        let done = race && place < 3 && seed % 2 == 1;
        out.push(format!("{car}:{total}:{best}:{laps}:{}", done as u8));
    }
    leak(out.join(";"))
}

/// The table. `kind`: 0 as "Race control" has it (pages, tabs, the sessions' buttons), 1 the
/// Leaderboard app's (5 rows around the player), 2 the one on TAB (10 rows around the player),
/// 3 online (ping, ballast, the kick button). `cars`: how many drive.
pub fn leaderboard(kind: u32, cars: usize) -> Vec<Op> {
    let mut o = Vec::new();
    field(&mut o, cars);
    o.extend([hud("race.sessions", 0.0, "1:0:Practice|2:0:Qualifying|3:0:Quick Race"), hud("race.current", 0.0, ""), hud("race.board", 0.0, board(cars, false, 0))]);
    if kind == 3 {
        o.push(hud("online", 1.0, ""));
        for i in 0..cars {
            let index = leak(i.to_string());
            o.extend([hud("car.ping", (17 * i % 230) as f32, index), hud("race.car.ballast", if i % 3 == 0 { 0.0 } else { 5.0 * i as f32 }, index), hud("race.car.restrictor", if i % 4 == 0 { 0.0 } else { 2.0 * i as f32 }, index)]);
        }
    }
    let (rows, focus) = match kind {
        1 => (5.0, "focus"),
        2 => (10.0, "focus"),
        _ => (10.0, ""),
    };
    o.extend([hud("menu.lb", rows, focus), hud("menu.lbPos", 0.0, "250,60")]);
    match kind {
        1 => o.extend([hud("menu.lbSize", 0.0, "780,172.5"), hud("menu.lbSet", 1.0, "ui"), hud("menu.lbSet", 0.0, "loaded")]),
        2 => o.extend([hud("menu.lbSet", 2.0, "ui"), hud("menu.lbSet", 0.0, "loaded")]),
        _ => o.push(hud("menu.lbSet", 1.0, "show")),
    }
    o.extend([Op::Frame(DT), Op::Frame(DT)]);
    // the time table: last laps (one that does not count), sectors, the best of all, deltas
    let click = |o: &mut Vec<Op>, x: i32, y: i32| o.extend([Op::MouseMove(x, y), Op::MouseDown(x, y), Op::MouseUp(x, y), Op::Frame(DT)]);
    let (tab_time, tab_entry) = ((250 + 570 + 50, 60 + 335 + 9), (250 + 465 + 50, 60 + 335 + 9));
    let tabs_y = if kind == 1 { 60 + 172 + 10 + 9 } else { tab_time.1 };
    click(&mut o, tab_time.0, tabs_y);
    o.extend([hud("race.globalBest", 83456.0, ""), hud("race.bestSplits", 0.0, "27100,30200,26000")]);
    for i in 0..cars.min(8) {
        let index = leak(i.to_string());
        o.extend([
            hud("race.car.last", 84000.0 + 731.0 * i as f32, index),
            hud("race.car.lastValid", (i % 3 != 1) as u8 as f32, index),
            hud("race.car.lastSplits", i as f32, leak(format!("{},{},{}", 27100 + 40 * i, 30200 + 55 * i, 26700 + 636 * i))),
            hud("race.car.best", 83456.0 + 417.0 * i as f32, index),
            hud("race.car.pbSplits", i as f32, leak(format!("{},{},{}", 27100 + 40 * i, 30200 + 20 * i, 26000 + 300 * i))),
            hud("race.car.perf", [0.0, -0.35, 1.25, 100.0, -100.0, 12.0, -0.04, 0.5][i], index),
        ]);
    }
    o.push(Op::Frame(DT));
    // a lap under way: sectors as they come, the driver's best in green, the best of all purple
    o.extend([hud("race.car.curSplits", 0.0, "27100"), hud("race.car.curSplits", 1.0, "27140,30200"), hud("race.car.curSplits", 2.0, "1000000,2,26000"), hud("race.car.pit", 1.0, "1"), Op::Frame(DT)]);
    o.extend([hud("race.car.done", 1.0, "0"), hud("race.car.done", 1.0, "1"), hud("race.car.done", 1.0, "3"), hud("race.car.black", 1.0, "2"), hud("car.connected", 0.0, "1"), Op::Frame(DT)]);
    click(&mut o, tab_entry.0, tabs_y);
    // a row is picked (the camera goes to its car); pages
    click(&mut o, 400, 60 + 20 + 45);
    click(&mut o, 400, 60 + 20 + 15);
    click(&mut o, 400, 60 + 20 + 280);
    click(&mut o, 250 + 760, 60 + 345);
    click(&mut o, 250 + 760, 60 + 345);
    click(&mut o, 250 + 760, 60 + 345);
    click(&mut o, 250 + 700, 60 + 345);
    click(&mut o, 400, 60 + 20 + 75);
    if kind == 3 {
        // the kick button of the picked row
        click(&mut o, 250 + 755 + 15, 60 + 20 + 61 + 15);
    }
    click(&mut o, 250 + 700, 60 + 345);
    click(&mut o, 250 + 700, 60 + 345);
    // qualifying begins: the practice becomes history, its button comes
    o.extend([hud("race.history", 0.0, board(cars, false, 0)), hud("race.current", 1.0, ""), hud("race.board", 0.0, board(cars, false, 3)), hud("session", 2.0, ""), hud("menu.newSession", 0.0, ""), Op::Frame(DT)]);
    click(&mut o, 250 + 150 + 20, 60 + 345);
    click(&mut o, tab_time.0, tabs_y);
    click(&mut o, 250 + 225 + 20, 60 + 345);
    click(&mut o, tab_time.0, tabs_y);
    click(&mut o, 250 + 150 + 20, 60 + 345);
    click(&mut o, 250 + 225 + 20, 60 + 345);
    // the race: laps and totals, cars that are a lap down, the flag of who has finished
    o.extend([hud("race.history", 1.0, board(cars, false, 3)), hud("race.current", 2.0, ""), hud("race.board", 0.0, board(cars, true, 0)), hud("session", 3.0, ""), hud("raceText", 0.0, "Race"), hud("menu.newSession", 0.0, ""), Op::Frame(DT)]);
    click(&mut o, tab_entry.0, tabs_y);
    o.extend([hud("race.board", 0.0, board(cars, true, 1)), Op::Frame(DT), hud("race.sessions", 0.0, "1:0:Practice|2:0:Qualifying|3:1:Quick Race"), Op::Frame(DT)]);
    click(&mut o, 250 + 300 + 20, 60 + 345);
    click(&mut o, 250 + 150 + 20, 60 + 345);
    o.extend([hud("menu.lbLast", 0.0, ""), Op::Frame(DT), hud("menu.lbVisible", 0.0, ""), Op::Frame(DT), hud("race.current", 1.0, ""), hud("menu.lbVisible", 1.0, ""), Op::Frame(DT)]);
    // the table moves: its columns stay where they were first drawn (but for the app's)
    o.extend([hud("menu.lbPos", 0.0, "120,200"), Op::Frame(DT), hud("race.board", 0.0, ""), Op::Frame(DT)]);
    o
}

/// What both sides write down of a table besides its controls.
#[allow(clippy::too_many_arguments)]
pub fn board_extra(session: i32, page: i32, selected: i32, mode: i32, last_mode: i32, show: bool, loaded: bool, printed: &str, picked: i32) -> String {
    format!(" board={session},{page},{selected},{mode},{last_mode},{}{} printed={printed} picked={picked}", show as u8, loaded as u8)
}

/// The dump of a name tag's controls. The game names them by adding the car's number to a
/// pointer into a word ("CAR_LABEL", "Name" ...): past the fifth car that reads whatever
/// follows the word in the program's data, where the port gives an empty name (Task 29, open
/// question 6). The names are never drawn; from the sixth car on they are left out of the
/// comparison.
pub fn tag_names(car: usize, block: String) -> String {
    if car < 5 {
        return block;
    }
    let mut out = String::new();
    for line in block.lines() {
        match (line.find('"'), line.find("\" r=")) {
            (Some(from), Some(to)) if from < to => {
                out.push_str(&line[..from + 1]);
                out.push_str(&line[to..]);
            }
            _ => out.push_str(line),
        }
        out.push(char::from(10u8));
    }
    out
}

/// What both sides write down of the end-of-session displayer besides its controls.
pub fn end_line(active: bool, loaded: bool, old_mode: i32, gui_active: bool) -> String {
    format!("end active={} loaded={} oldMode={old_mode} gui={}\n", active as u8, loaded as u8, gui_active as u8)
}

/// The end of a session: the results over a dark screen, the three buttons. `kind`: the type
/// of the session that ends (1 practice, 2 qualifying, 3 a race; +10: online); `cars`: how
/// many drove. A screen of 1280 x 720 is taken for the mouse.
pub fn end_session(kind: u32, cars: usize) -> Vec<Op> {
    let (online, kind) = (kind >= 10, kind % 10);
    let mut o = Vec::new();
    field(&mut o, cars);
    let race = kind == 3;
    let sessions = match kind {
        1 => "1:0:Practice",
        2 => "1:0:Practice|2:0:Qualifying",
        _ => "1:0:Practice|2:0:Qualifying|3:0:Quick Race",
    };
    let current = (kind - 1) as f32;
    o.extend([hud("race.sessions", 0.0, sessions), hud("race.current", current, ""), hud("session", kind as f32, ""), hud("raceText", 0.0, "Race"), hud("pqText", 0.0, "Session"), hud("cameraMode", 2.0, "")]);
    for index in 0..current as usize {
        o.push(hud("race.history", index as f32, board(cars, false, index + 1)));
    }
    o.push(hud("race.board", 0.0, board(cars, race, 1)));
    for i in 0..cars.min(6) {
        let index = leak(i.to_string());
        o.extend([hud("race.car.last", 84000.0 + 731.0 * i as f32, index), hud("race.car.lastValid", (i % 3 != 1) as u8 as f32, index), hud("race.car.best", 83456.0 + 417.0 * i as f32, index), hud("race.car.done", 1.0, index)]);
    }
    if online {
        o.push(hud("online", 1.0, ""));
        for i in 0..cars {
            o.push(hud("car.ping", (23 * i % 190) as f32, leak(i.to_string())));
        }
    }
    o.extend([hud("menu.end", 0.0, ""), Op::Frame(DT), hud("message", 0.0, "Race|Over"), Op::Frame(DT)]);
    // the results come: the GUI goes, the camera turns, the controls are locked
    o.extend([hud("menu.endShow", 0.0, ""), Op::Frame(DT), Op::Frame(DT), hud("menu.endShow", 0.0, ""), Op::Frame(DT)]);
    let (restart, replay, exit) = ((470, 625), (640, 625), (810, 625));
    for at in [restart, replay, exit, (640, 300)] {
        o.extend([Op::MouseMove(at.0, at.1), Op::Frame(DT)]);
    }
    // a row of the table is picked; a page on, a page back (more than ten cars)
    let click = |o: &mut Vec<Op>, x: i32, y: i32| o.extend([Op::MouseMove(x, y), Op::MouseDown(x, y), Op::MouseUp(x, y), Op::Frame(DT)]);
    let (left, top) = (250, 197);
    click(&mut o, left + 300, top + 20 + 45);
    click(&mut o, left + 760, top + 345);
    click(&mut o, left + 700, top + 345);
    click(&mut o, left + 150 + 20, top + 345);
    click(&mut o, left + 225 + 20, top + 345);
    // Replay: the panel goes while the replay runs and is back after it
    click(&mut o, replay.0, replay.1);
    o.extend([hud("replay", 1.0, ""), Op::Frame(DT), Op::Frame(DT), hud("replay", 0.0, ""), Op::Frame(DT), Op::Frame(DT)]);
    // the pause menu's place is taken: nothing is drawn while it shows
    o.extend([hud("pause", 1.0, ""), Op::Frame(DT), hud("pause", 0.0, ""), Op::Frame(DT)]);
    // other kinds of session behind the same panel
    for other in [0.0, 4.0, 1.0, 2.0, 3.0] {
        o.extend([hud("session", other, ""), Op::Frame(DT)]);
    }
    o.extend([hud("session", kind as f32, ""), Op::Frame(DT)]);
    // Restart: the referee's; then the panel is taken down, and comes again
    click(&mut o, restart.0, restart.1);
    o.extend([hud("menu.endHide", 0.0, ""), Op::Frame(DT), hud("menu.endHide", 0.0, ""), Op::Frame(DT)]);
    click(&mut o, restart.0, restart.1);
    click(&mut o, replay.0, replay.1);
    o.extend([hud("cameraMode", 0.0, ""), hud("race.board", 0.0, board(cars, race, 2)), hud("menu.endShow", 0.0, ""), Op::Frame(DT), Op::Frame(DT)]);
    click(&mut o, exit.0, exit.1);
    o.push(Op::Frame(DT));
    o
}

/// The standings of a weekend's qualifying, with a practice behind it.
fn weekend(o: &mut Vec<Op>, cars: usize, online: bool) {
    field(o, cars);
    o.extend([hud("race.sessions", 0.0, "1:0:Practice|2:0:Qualifying|3:0:Quick Race"), hud("race.history", 0.0, board(cars, false, 2)), hud("race.current", 1.0, ""), hud("race.board", 0.0, board(cars, false, 0)), hud("session", 2.0, ""), hud("pqText", 0.0, "Qualifying")]);
    for i in 0..cars.min(8) {
        let index = leak(i.to_string());
        o.extend([
            hud("race.car.last", 84000.0 + 731.0 * i as f32, index),
            hud("race.car.lastValid", (i % 3 != 1) as u8 as f32, index),
            hud("race.car.lastSplits", i as f32, leak(format!("{},{},{}", 27100 + 40 * i, 30200 + 55 * i, 26700 + 636 * i))),
            hud("race.car.best", 83456.0 + 417.0 * i as f32, index),
            hud("race.car.pbSplits", i as f32, leak(format!("{},{},{}", 27100 + 40 * i, 30200 + 20 * i, 26000 + 300 * i))),
        ]);
    }
    o.extend([hud("race.globalBest", 83456.0, ""), hud("race.bestSplits", 0.0, "27100,30200,26000")]);
    if online {
        o.push(hud("online", 1.0, ""));
        for i in 0..cars {
            o.push(hud("car.ping", (29 * i % 210) as f32, leak(i.to_string())));
        }
    }
}

/// The table held on TAB: it comes, shows the standings round the player, hears the mouse,
/// goes. A screen of 1280 x 720 is taken for the mouse.
pub fn tab_leaderboard(cars: usize, online: bool) -> Vec<Op> {
    let mut o = Vec::new();
    weekend(&mut o, cars, online);
    o.extend([hud("menu.tab", 0.0, ""), Op::Frame(DT), Op::Frame(DT)]);
    let click = |o: &mut Vec<Op>, x: i32, y: i32| o.extend([Op::MouseMove(x, y), Op::MouseDown(x, y), Op::MouseUp(x, y), Op::Frame(DT)]);
    let (left, top) = (250, 197);
    // hidden: the mouse is not heard
    click(&mut o, left + 570 + 50, top + 335 + 9);
    o.extend([hud("menu.tabShow", 1.0, ""), Op::Frame(DT), Op::Frame(DT)]);
    click(&mut o, left + 570 + 50, top + 335 + 9);
    click(&mut o, left + 300, top + 20 + 45);
    click(&mut o, left + 150 + 20, top + 345);
    click(&mut o, left + 225 + 20, top + 345);
    click(&mut o, left + 465 + 50, top + 335 + 9);
    // the player is further down the order; then the race
    o.extend([hud("race.board", 0.0, board(cars, false, 5)), Op::Frame(DT), hud("menu.tabShow", 0.0, ""), Op::Frame(DT)]);
    o.extend([hud("race.history", 1.0, board(cars, false, 5)), hud("race.current", 2.0, ""), hud("race.board", 0.0, board(cars, true, 3)), hud("session", 3.0, ""), hud("raceText", 0.0, "Race"), hud("menu.newSession", 0.0, ""), Op::Frame(DT)]);
    o.extend([hud("menu.tabShow", 1.0, ""), Op::Frame(DT), hud("race.board", 0.0, board(cars, true, 4)), Op::Frame(DT), hud("menu.tabShow", 1.0, ""), Op::Frame(DT), hud("menu.tabShow", 0.0, ""), Op::Frame(DT), hud("menu.tabShow", 0.0, ""), Op::Frame(DT)]);
    o
}

/// The Leaderboard app: a form of the app bar with five rows round the player.
pub fn leaderboard_app(cars: usize) -> Vec<Op> {
    let mut o = Vec::new();
    weekend(&mut o, cars, false);
    o.extend([hud("menu.app", 0.0, ""), Op::Frame(DT), hud("menu.appVisible", 1.0, ""), Op::Frame(DT), Op::Frame(DT)]);
    let click = |o: &mut Vec<Op>, x: i32, y: i32| o.extend([Op::MouseMove(x, y), Op::MouseDown(x, y), Op::MouseUp(x, y), Op::Frame(DT)]);
    // its tabs, the sessions' buttons; the form dragged by its title; hidden and back
    click(&mut o, 570 + 50, 80 + 30 + 172 + 10 + 9);
    click(&mut o, 150 + 20, 80 + 30 + 172 + 20);
    click(&mut o, 225 + 20, 80 + 30 + 172 + 20);
    click(&mut o, 465 + 50, 80 + 30 + 172 + 10 + 9);
    o.extend([Op::MouseMove(300, 90), Op::MouseDown(300, 90), Op::MouseMove(380, 260), Op::Frame(DT), Op::MouseUp(380, 260), Op::Frame(DT), Op::Frame(DT)]);
    o.extend([hud("race.board", 0.0, board(cars, false, 5)), Op::Frame(DT), hud("menu.appVisible", 0.0, ""), Op::Frame(DT), hud("menu.appVisible", 1.0, ""), Op::Frame(DT), Op::MouseMove(1279, 300), Op::Frame(DT), Op::Frame(DT), Op::MouseMove(600, 600), Op::Frame(DT)]);
    o
}
