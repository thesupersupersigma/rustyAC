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
    SCRIPT.lock().unwrap().clone()
}

/// A script's change of the menus' session state (true: it was one).
pub fn menu_input(what: &str, number: f32, _text: &str) -> bool {
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
