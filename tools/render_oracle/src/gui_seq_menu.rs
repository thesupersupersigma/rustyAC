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
    /// what the ESC menu and its pages read
    pub esc: EscScript,
    /// what the replay's bar reads besides the manager's members
    pub replay: ReplayScript,
    /// an online session's own overlays: the ping to the server (`ACClient::getPing(null)`),
    /// the server's welcome message, the message of the countdown of a kicked driver ("":
    /// none runs)
    pub own_ping: i32,
    pub welcome: String,
    pub disconnect: String,
    /// what the online pages read of `ACClient` and of the server's rules (Task 34)
    pub online: OnlineScript,
}

#[derive(Clone, Debug)]
pub struct OnlineScript {
    /// `ACClient::serverName`, `serverDrivingAssists`, `invertedGridPositions`
    pub server_name: String,
    pub tc: i32,
    pub abs: i32,
    pub stability: bool,
    pub auto_clutch: bool,
    pub inverted_grid: i32,
    /// `PhysicsEngine`'s rates, tyre blankets and false start's penalty
    pub damage: f32,
    pub fuel: f32,
    pub tyre: f32,
    pub blankets: bool,
    pub jump_start: i32,
    /// `ACClientVotingManager::currentVote`; the target is a car's index (-1: none)
    pub vote_type: i32,
    pub has_voted: bool,
    pub time_left: f32,
    pub max_time: f32,
    pub target: i32,
    /// the client's `netCars`: (session id, car's index)
    pub net_cars: Vec<(u8, usize)>,
}

/// What a script says of the session for the ESC menu.
#[derive(Clone, Copy, Debug)]
pub struct EscScript {
    /// the player's car's `setupState`; `RaceManager::fixedSetup`
    pub setup_state: i32,
    pub fixed_setup: bool,
    /// `RaceManager::getTimetoWait`; `Game::gameTime.now`; `CarAvatar::isInPit`
    pub time_to_wait: f64,
    pub game_time: f64,
    pub in_pit: bool,
    /// the track's grip, the sun's angle, the air's and the road's temperature, the wind
    pub grip: f32,
    pub sun_angle: f32,
    pub ambient: f32,
    pub road: f32,
    pub wind_speed: f32,
    pub wind_direction: f32,
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
    /// Task 43: `PitStop::getEstimatedTime`, for the mouse menu (the tank: `car.fuel`)
    pub estimate: f32,
}

static SCRIPT: Mutex<MenuScript> = Mutex::new(MenuScript {
    session_count: 1,
    replay_manager: false,
    pit: PitScript { max_fuel: 0.0, compounds: Vec::new(), pressure: [0; 4], wing: [0.0; 8], in_pitlane: false, fuel_per_lap: 0.0, spline: 0.0, total_time: 0, timed: false, state: 0, times: [0.0; 5], estimate: 0.0 },
    own_ping: 0,
    welcome: String::new(),
    disconnect: String::new(),
    online: OnlineScript { server_name: String::new(), tc: 0, abs: 0, stability: false, auto_clutch: false, inverted_grid: 0, damage: 0.0, fuel: 0.0, tyre: 0.0, blankets: false, jump_start: 0, vote_type: 3, has_voted: false, time_left: 0.0, max_time: 0.0, target: -1, net_cars: Vec::new() },
    replay: ReplayScript { lap_marks: Vec::new(), camera_manager: false, camera_sets: Vec::new(), camera_index: 0 },
    esc: EscScript { setup_state: 1, fixed_setup: false, time_to_wait: 0.0, game_time: 0.0, in_pit: false, grip: 1.0, sun_angle: 0.0, ambient: 0.0, road: 0.0, wind_speed: 0.0, wind_direction: 0.0 },
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
        "menu.ping" => s.own_ping = number as i32,
        "menu.welcome" => s.welcome = _text.replace('|', "\n"),
        "menu.disconnect" => s.disconnect = _text.to_string(),
        "menu.replayLaps" => s.replay.lap_marks = _text.split(',').filter_map(|mark| mark.trim().parse().ok()).collect(),
        "menu.replayCamera" => s.replay.camera_manager = number != 0.0,
        "menu.cameraSets" => s.replay.camera_sets = _text.split('|').filter(|set| !set.is_empty()).map(str::to_string).collect(),
        "menu.cameraAt" => s.replay.camera_index = number as i32,
        "menu.env" => match _text {
            "setup" => s.esc.setup_state = number as i32,
            "fixedSetup" => s.esc.fixed_setup = number != 0.0,
            "wait" => s.esc.time_to_wait = number as f64,
            "now" => s.esc.game_time = number as f64,
            "inPit" => s.esc.in_pit = number != 0.0,
            "grip" => s.esc.grip = number,
            "sun" => s.esc.sun_angle = number,
            "ambient" => s.esc.ambient = number,
            "road" => s.esc.road = number,
            "wind" => s.esc.wind_speed = number,
            "windDirection" => s.esc.wind_direction = number,
            _ => return false,
        },
        "menu.serverName" => s.online.server_name = _text.to_string(),
        "menu.server" => {
            let o = &mut s.online;
            match _text {
                "tc" => o.tc = number as i32,
                "abs" => o.abs = number as i32,
                "stability" => o.stability = number != 0.0,
                "autoClutch" => o.auto_clutch = number != 0.0,
                "inverted" => o.inverted_grid = number as i32,
                "damage" => o.damage = number,
                "fuel" => o.fuel = number,
                "tyre" => o.tyre = number,
                "blankets" => o.blankets = number != 0.0,
                "jumpStart" => o.jump_start = number as i32,
                _ => return false,
            }
        }
        "menu.vote" => {
            let o = &mut s.online;
            match _text {
                "type" => o.vote_type = number as i32,
                "voted" => o.has_voted = number != 0.0,
                "left" => o.time_left = number,
                "max" => o.max_time = number,
                "target" => o.target = number as i32,
                _ => return false,
            }
        }
        "menu.netCar" => s.online.net_cars.push((number as u8, _text.parse().unwrap_or(0))),
        "pit.maxFuel" => s.pit.max_fuel = number,
        "pit.compounds" => s.pit.compounds = _text.split('|').filter(|c| !c.is_empty()).map(str::to_string).collect(),
        "pit.inPitlane" => s.pit.in_pitlane = number != 0.0,
        "pit.fuelPerLap" => s.pit.fuel_per_lap = number,
        "pit.spline" => s.pit.spline = number,
        "pit.totalTime" => s.pit.total_time = number as i32,
        "pit.timed" => s.pit.timed = number != 0.0,
        "pit.state" => s.pit.state = number as i32,
        "pit.estimate" => s.pit.estimate = number,
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

pub(crate) fn hud(what: &'static str, number: f32, text: &'static str) -> Op {
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
    let mut o = vec![hud("create", 2.0, ""), hud("car.name", 0.0, "Tazio Nuvolari"), hud("session", 3.0, ""), hud("raceText", 0.0, "Race"), hud("pit.compounds", 0.0, "Soft|Medium|Hard"), hud("menu.pitStop", 0.0, ""), Op::Frame(DT)];
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

/// Task 43: the old mouse menu of `PitStop` (a keyboard driver's): it opens in the box (the
/// state "choosing"), the mouse goes over it and clicks every kind of control but CONFIRM
/// (that one asks the car for the stop's time: proven by the car oracle's `pit_mouse`), the
/// tank's line and the estimate follow; it is not drawn in a menu or once the race is over.
/// A screen of 1280 x 720 is taken for the mouse (the panel's left edge is at 315).
pub fn pit_mouse() -> Vec<Op> {
    let mut o = vec![hud("create", 2.0, ""), hud("car.name", 0.0, "Tazio Nuvolari"), hud("session", 3.0, ""), hud("raceText", 0.0, "Race")];
    o.extend([hud("pit.maxFuel", 60.0, ""), hud("pit.compounds", 0.0, "Soft|Medium|Hard"), hud("car.fuel", 21.5, "0"), hud("pit.estimate", 12.7, ""), hud("menu.pitStop", 0.0, ""), Op::Frame(DT)]);
    // in the box: the menu opens with the tank's share, "NO CHANGE" chosen
    o.extend([hud("pit.state", 2.0, ""), hud("menu.pitState", 0.0, ""), hud("menu.pitMouse", 21.5 / 60.0, ""), Op::Frame(DT)]);
    let click = |o: &mut Vec<Op>, x: i32, y: i32| o.extend([Op::MouseMove(x, y), Op::Frame(DT), Op::MouseDown(x, y), Op::Frame(DT), Op::MouseUp(x, y), Op::Frame(DT)]);
    let (left, top) = (315, 75);
    // every compound, then "NO CHANGE" again, then the second compound
    for x in [200 + 28, 312 + 28, 424 + 28, 50 + 28, 312 + 28] {
        click(&mut o, left + x, top + 40 + 28);
    }
    // the fuel: plus three times, minus once, a click on the gauge past the tank, one before
    // it (nothing), fill up, minus
    for (x, y) in [(424, 224), (424, 224), (424, 224), (59, 224), (90 + 250, 215 + 9), (90 + 40, 215 + 9), (574, 224), (59, 224)] {
        click(&mut o, left + x, top + y);
    }
    // the three repairs on, the body off again
    for x in [66 + 32, 293 + 32, 536 + 32, 293 + 32] {
        click(&mut o, left + x, top + 360 + 32);
    }
    // beside the controls, and outside the panel: nothing
    click(&mut o, left + 600, top + 300);
    click(&mut o, 100, 400);
    o.extend([hud("pit.estimate", 31.2, ""), hud("car.fuel", 3.0, "0"), Op::Frame(DT)]);
    // not in a menu, not once the race is over; another state: no menu, and no clicks
    for (what, text) in [("pause", ""), ("esc", "300"), ("raceOver", "")] {
        o.extend([hud(what, 1.0, text), Op::Frame(DT), hud(what, 0.0, text), Op::Frame(DT)]);
    }
    o.extend([hud("pit.state", 4.0, ""), hud("menu.pitState", 0.0, ""), Op::Frame(DT)]);
    click(&mut o, left + 200 + 28, top + 40 + 28);
    o.extend([hud("pit.state", 2.0, ""), hud("menu.pitState", 0.0, ""), Op::Frame(DT)]);
    click(&mut o, left + 66 + 32, top + 360 + 32);
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
                    // type:timed:name, and then laps:minutes:start for who reads them
                    let mut parts = session.split(':');
                    RaceSession {
                        session_type: parts.next().and_then(|t| t.parse().ok()).unwrap_or(0),
                        is_timed_race: parts.next() == Some("1"),
                        name: parts.next().unwrap_or("").to_string(),
                        laps: parts.next().and_then(|t| t.parse().ok()).unwrap_or(0),
                        duration_minutes: parts.next().and_then(|t| t.parse().ok()).unwrap_or(0.0),
                        start_time: parts.next().and_then(|t| t.parse().ok()).unwrap_or(0.0),
                    }
                })
                .collect()
        }
        "race.current" => race.current_session_index = number as i32,
        "race.inverted" => race.inverted_grid_positions = number as i32,
        "race.pitWindow" => {
            let (start, end) = text.split_once(',').unwrap_or(("0", "0"));
            race.pit_window_start = start.parse().unwrap_or(0);
            race.pit_window_end = end.parse().unwrap_or(0);
        }
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
                "race.car.laps" => car.lap_count = number as u32,
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

pub(crate) fn leak(text: String) -> &'static str {
    Box::leak(text.into_boxed_str())
}

const DRIVERS: [&str; 24] = [
    "Tazio Nuvolari", "Rudolf Caracciola", "Bernd Rosemeyer", "Achille Varzi", "Louis Chiron", "Hans Stuck", "Luigi Fagioli", "Manfred von Brauchitsch", "Raymond Sommer", "Jean-Pierre Wimille", "Giuseppe Farina", "Hermann Lang", "Richard Seaman", "Rene Dreyfus", "Piero Taruffi", "Carlo Felice Trossi", "Philippe Etancelin", "Goffredo Zehender", "Earl Howe", "Whitney Straight", "A Driver With A Very Long Name Indeed", "", "Clemente Biondetti", "Christian Kautz",
];
const NATIONS: [&str; 8] = ["ITA", "DEU", "", "FRA", "GBR", "ZZZ", "MCO", "CHE"];
const SKINS: [&str; 4] = ["00_official", "01_ferrari_1", "02_ferrari_2", "no_such_skin"];

/// The cars of the tables' scripts: names, nations, liveries, tyres.
pub(crate) fn field(o: &mut Vec<Op>, cars: usize) {
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
pub(crate) fn board(cars: usize, race: bool, seed: usize) -> &'static str {
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

/// The state line of the overlay leaderboard.
pub fn overlay_line(mode: i32, vertical: bool, timer: f32, names: &[String]) -> String {
    format!("overlay mode={mode} vertical={} timer={:08x} names={}{}", vertical as u8, timer.to_bits(), names.join("|"), char::from(10u8))
}

/// The overlay leaderboard: the strip along the bottom (`vertical`: the column at the left),
/// through a practice and a race, every face of F9, the player's place moved through the
/// field, cars in the pits, retired and gone, a driver's name changed, the menus over it.
pub fn overlay(cars: usize, vertical: bool) -> Vec<Op> {
    let mut o = Vec::new();
    field(&mut o, cars);
    let side = if vertical { "v" } else { "" };
    let place = |o: &mut Vec<Op>, p: usize| o.extend([hud("car.pos", p as f32, "0"), Op::Frame(DT)]);
    o.extend([hud("race.sessions", 0.0, "1:0:Practice|3:0:Quick Race"), hud("race.current", 0.0, ""), hud("race.board", 0.0, board(cars, false, 0)), hud("car.pos", 1.0, "0")]);
    // off: nothing; then on, in a practice (the gaps are those of the best laps)
    o.extend([hud("menu.overlay", 0.0, ""), Op::Frame(DT), hud("menu.overlayMode", 1.0, side), Op::Frame(DT)]);
    for p in [1, 2, cars / 2, cars.saturating_sub(1), cars, cars + 3, 0] {
        place(&mut o, p);
    }
    // the race: gaps to the first, to the car ahead, the two in turn (ten seconds each)
    o.extend([hud("race.current", 1.0, ""), hud("race.board", 0.0, board(cars, true, 3)), hud("session", 3.0, ""), hud("raceText", 0.0, "Race"), Op::Frame(DT)]);
    for mode in [1.0, 2.0, 3.0] {
        o.extend([hud("menu.overlayMode", mode, side), Op::Frame(DT)]);
        place(&mut o, cars / 2 + 1);
        place(&mut o, 1);
    }
    o.extend([Op::Frame(6.0), Op::Frame(5.0), Op::Frame(6.0), Op::Frame(5.0), Op::Frame(DT)]);
    // cars in the pits, retired, gone; a driver's name changed (and one that makes the same
    // three letters as another's)
    let last = leak((cars - 1).to_string());
    o.extend([hud("race.car.pit", 1.0, "1"), hud("race.car.black", 1.0, "0"), Op::Frame(DT), hud("race.car.black", 1.0, last), hud("race.car.pit", 1.0, "0"), Op::Frame(DT), hud("car.connected", 0.0, "1"), Op::Frame(DT), hud("car.connected", 1.0, "1"), hud("race.car.black", 0.0, "0"), hud("race.car.pit", 0.0, "0")]);
    o.extend([hud("car.name", 0.0, "J. O'Neil"), Op::Frame(DT), hud("car.name", 1.0, "A. B. Nuvoletta"), Op::Frame(DT), hud("car.name", 0.0, "Xu"), Op::Frame(DT), hud("car.name", 1.0, "carlo nuvolari"), hud("car.name", 0.0, "Tazio Nuvolari"), Op::Frame(DT), Op::Frame(DT)]);
    // a wider gap than the cell can say; other kinds of session; the taskbar out (the mouse
    // at the right edge)
    o.extend([hud("race.board", 0.0, leak(format!("0:420000:83000:5:0;1:{}:84000:5:0", 420000.0 + 123456789.0))), Op::Frame(DT), hud("race.board", 0.0, board(cars, true, 4))]);
    for session in [2.0, 4.0, 0.0, 3.0] {
        o.extend([hud("session", session, ""), Op::Frame(DT)]);
    }
    o.extend([Op::MouseMove(1279, 300), Op::Frame(DT), Op::Frame(0.3), Op::Frame(0.3), Op::MouseMove(600, 300), Op::Frame(DT)]);
    // hidden by the menus, a replay, the results
    for what in ["pause", "esc", "replay", "ended"] {
        o.extend([hud(what, 1.0, "300"), Op::Frame(DT), hud(what, 0.0, ""), Op::Frame(DT)]);
    }
    // F9, eight times round: every face, with its message
    o.extend([hud("menu.overlayMode", 0.0, ""), Op::Frame(DT)]);
    for _ in 0..8 {
        o.extend([hud("menu.overlayF9", 0.0, ""), Op::Frame(DT), Op::Frame(0.5)]);
    }
    // a face the file can say and F9 cannot
    o.extend([hud("menu.overlayMode", 7.0, side), Op::Frame(DT), hud("menu.overlayF9", 0.0, ""), Op::Frame(DT)]);
    o
}

/// The ESC menu's own state in the dump: which child is the picked button, whether "Drive"
/// was ever pressed, the fade's seconds.
pub fn esc_extra(last: i32, drive_pressed: bool, fade: f32) -> String {
    format!(" esc={last},{},{:08x}", drive_pressed as u8, fade.to_bits())
}

/// The ESC menu (the menu of the pits) with its page "Race control". `kind`: 0 a practice
/// with two sessions after it, 1 the race of a weekend (laps, then by the clock), 2 one
/// session whose setup is fixed.
pub fn esc_menu(kind: u32) -> Vec<Op> {
    let mut o = Vec::new();
    let cars = if kind == 2 { 3 } else { 12 };
    field(&mut o, cars);
    let env = |o: &mut Vec<Op>, what: &'static str, value: f32| o.push(hud("menu.env", value, what));
    let sessions = match kind {
        0 => "1:0:Practice:0:30:0|2:0:Qualifying:0:15:0|3:0:Quick Race:5:0:0",
        1 => "1:0:Practice:0:90:0|3:0:Race:12:0:20000|3:1:endurance:0:125.5:20000",
        _ => "4:0:Hotlap:0:0:0",
    };
    o.extend([hud("race.sessions", 0.0, sessions), hud("race.current", if kind == 1 { 1.0 } else { 0.0 }, ""), hud("race.board", 0.0, board(cars, kind == 1, 0)), hud("session", [1.0, 3.0, 4.0][kind as usize], ""), hud("timeLeft", 1_234_567.0, "")]);
    if kind == 1 {
        o.push(hud("race.history", 0.0, board(cars, false, 2)));
    }
    for (what, value) in [("grip", 0.9734), ("sun", -32.5), ("ambient", 26.4), ("road", 31.9), ("wind", 3.3), ("windDirection", 135.0), ("now", 61_000.0), ("fixedSetup", (kind == 2) as u8 as f32)] {
        env(&mut o, what, value);
    }
    let click = |o: &mut Vec<Op>, x: i32, y: i32| o.extend([Op::MouseMove(x, y), Op::MouseDown(x, y), Op::MouseUp(x, y), Op::Frame(DT)]);
    let (drive, setup, time, telemetry, control, exit) = (125 + 48, 225 + 48, 325 + 48, 425 + 48, 525 + 48, 720 - 97 + 48);
    // made, hidden; shown: off line the page "Race control" opens by itself
    o.extend([hud("menu.escMenu", 0.0, ""), Op::Frame(DT), hud("menu.escShow", 0.0, ""), Op::Frame(DT), Op::Frame(DT)]);
    for y in [drive, setup, time, telemetry, control, exit] {
        o.extend([Op::MouseMove(50, y), Op::Frame(DT)]);
    }
    // the pages by their buttons: another page, the same button again, the fixed setup's message
    for y in [time, telemetry, setup, setup, telemetry, telemetry, control] {
        click(&mut o, 50, y);
    }
    o.push(Op::Frame(1.0));
    // the session's panel: the setup's state, a session not started, no time left, the clock
    for state in [0.0, 2.0, 5.0, 1.0] {
        env(&mut o, "setup", state);
        o.push(Op::Frame(DT));
    }
    o.extend([hud("tts", 3000.0, ""), Op::Frame(DT), hud("tts", 0.0, ""), hud("timeLeft", 0.0, ""), Op::Frame(DT), hud("timeLeft", 4_000_000.0, ""), Op::Frame(DT), hud("timeLeft", 59_999.0, ""), Op::Frame(DT)]);
    for (what, value) in [("sun", 80.0), ("sun", -200.5), ("ambient", -3.7), ("road", 0.4), ("wind", 27.9), ("windDirection", -45.0), ("grip", 1.0)] {
        env(&mut o, what, value);
        o.push(Op::Frame(DT));
    }
    if kind == 1 {
        // a race by laps: what the leader has left; the grid turned round; a pit window
        let leader = leak(((3) % cars).to_string());
        o.extend([hud("race.car.laps", 4.0, leader), Op::Frame(DT), hud("race.car.laps", 40.0, leader), Op::Frame(DT), hud("race.inverted", 1.0, ""), hud("race.pitWindow", 0.0, "3,9"), Op::Frame(DT)]);
        // the race by the clock that follows
        o.extend([hud("race.history", 1.0, board(cars, true, 3)), hud("race.current", 2.0, ""), hud("race.board", 0.0, board(cars, true, 5)), hud("menu.newSession", 0.0, ""), Op::Frame(DT), hud("race.pitWindow", 0.0, "0,0"), hud("race.inverted", 0.0, ""), Op::Frame(DT)]);
    } else if kind == 0 {
        o.extend([hud("race.history", 0.0, board(cars, false, 4)), hud("race.current", 1.0, ""), hud("race.board", 0.0, board(cars, false, 1)), hud("session", 2.0, ""), hud("menu.newSession", 0.0, ""), Op::Frame(DT)]);
    }
    // the table of the page hears the mouse; the three buttons of the session's panel
    click(&mut o, 425 + 150 + 20, 55 + 20 + 12);
    click(&mut o, 425 + 300, 55 + 20 + 45);
    for x in [45, 130, 215] {
        o.extend([Op::MouseMove(120 + x + 20, 273 + 30 + 20), Op::Frame(DT)]);
        click(&mut o, 120 + x + 20, 273 + 30 + 20);
    }
    // "Drive": the camera fades, then the car is let go and the game's screen is back
    click(&mut o, 50, drive);
    o.extend([Op::Frame(DT), hud("menu.fadeDone", 0.0, ""), Op::Frame(DT), Op::Frame(DT)]);
    // back in the pits: the page is as it was left; "Drive" forbidden, then allowed
    o.extend([hud("menu.escShow", 0.0, ""), Op::Frame(DT), hud("menu.driveEnabled", 0.0, ""), Op::Frame(DT)]);
    click(&mut o, 50, drive);
    o.extend([hud("menu.driveEnabled", 1.0, ""), Op::Frame(DT)]);
    click(&mut o, 50, drive);
    o.extend([hud("menu.fadeDone", 0.0, ""), Op::Frame(DT)]);
    // with no page picked the menu opens "Race control" again, but not from the free camera
    o.extend([hud("menu.escShow", 0.0, ""), Op::Frame(DT)]);
    click(&mut o, 50, control);
    o.extend([hud("menu.resume", 0.0, ""), Op::Frame(DT), hud("cameraMode", 5.0, ""), hud("menu.escShow", 0.0, ""), Op::Frame(DT), hud("menu.resume", 0.0, ""), hud("cameraMode", 0.0, ""), hud("menu.escShow", 0.0, ""), Op::Frame(DT)]);
    click(&mut o, 50, time);
    o.extend([hud("menu.raceControl", 0.0, ""), Op::Frame(DT), hud("menu.raceControl", 0.0, ""), Op::Frame(DT)]);
    // "Exit"
    click(&mut o, 50, exit);
    o.extend([hud("menu.resume", 0.0, ""), Op::Frame(DT)]);
    o
}

/// The ESC menu of an online session: its header, and the page "Race control" with the
/// server's rules, the votes and the chat (Task 34). The server's state is made up.
pub fn esc_menu_online() -> Vec<Op> {
    let mut o = Vec::new();
    let cars = 12;
    field(&mut o, cars);
    o.extend([hud("online", 1.0, ""), hud("race.sessions", 0.0, "1:0:Practice:0:30:0|2:0:Qualifying:0:15:0|3:0:Race:10:0:20000"), hud("race.current", 0.0, ""), hud("race.board", 0.0, board(cars, false, 0)), hud("session", 1.0, ""), hud("timeLeft", 1_234_567.0, "")]);
    for (what, value) in [("grip", 0.9734), ("sun", -32.5), ("ambient", 26.4), ("road", 31.9), ("wind", 3.3), ("windDirection", 135.0), ("now", 61_000.0)] {
        o.push(hud("menu.env", value, what));
    }
    o.push(hud("menu.serverName", 0.0, "rustyAC test server"));
    for (what, value) in [("tc", 1.0), ("abs", 2.0), ("stability", 0.0), ("autoClutch", 1.0), ("damage", 0.5), ("fuel", 1.0), ("tyre", 2.25), ("blankets", 1.0), ("jumpStart", 1.0)] {
        o.push(hud("menu.server", value, what));
    }
    for (sid, car) in [(3.0, "1"), (4.0, "2"), (7.0, "5")] {
        o.push(hud("menu.netCar", sid, car));
    }
    let click = |o: &mut Vec<Op>, x: i32, y: i32| o.extend([Op::MouseMove(x, y), Op::MouseDown(x, y), Op::MouseUp(x, y), Op::Frame(DT)]);
    // made, hidden; shown: online too the page "Race control" opens by itself
    o.extend([hud("menu.escMenu", 0.0, ""), Op::Frame(DT), hud("menu.escShow", 0.0, ""), Op::Frame(DT), Op::Frame(DT)]);
    // the server's rules, one by one; a long name; the grid turned round
    for (what, value) in [("tc", 0.0), ("abs", 1.0), ("stability", 1.0), ("autoClutch", 0.0), ("damage", 1.0), ("fuel", 0.0), ("tyre", 0.333), ("blankets", 0.0), ("jumpStart", 2.0), ("jumpStart", 0.0), ("tc", 2.0), ("abs", 0.0), ("jumpStart", 5.0)] {
        o.extend([hud("menu.server", value, what), Op::Frame(DT)]);
    }
    o.extend([hud("race.inverted", 8.0, ""), Op::Frame(DT), hud("menu.serverName", 0.0, "rustyAC test server number two | sprint races every hour"), Op::Frame(DT)]);
    // the chat: other drivers, the server, a session no car has (red from then on)
    for (sid, text) in [(3.0, "hello"), (255.0, "Welcome to the server"), (4.0, "gl hf"), (9.0, "my own line"), (7.0, "after the red one")] {
        o.extend([hud("menu.chat", sid, text), Op::Frame(DT)]);
    }
    for i in 0..20 {
        o.push(hud("menu.chat", 3.0, leak(format!("line {i} of a long chat"))));
    }
    o.push(Op::Frame(DT));
    // the input: a click gives it the keys, Enter sends; too soon after: kept
    // (the page is at x 100; the chat's input at (10, 363 - 40) in the chat at (325, 425))
    click(&mut o, 100 + 325 + 110, 425 + 323 + 12);
    for c in "hi all".chars() {
        o.push(Op::KeyChar(c as u32));
    }
    o.extend([Op::Frame(DT), Op::KeyChar(13), Op::Frame(DT)]);
    for c in "again".chars() {
        o.push(Op::KeyChar(c as u32));
    }
    o.extend([Op::KeyChar(13), Op::Frame(DT), hud("menu.env", 63_000.0, "now"), Op::Frame(DT), Op::KeyChar(13), Op::Frame(DT)]);
    // the votes: asked for (each request locks the two buttons longer), the server's, a kick
    let (vx, vy) = (100 + 20 + 45 + 20, 55 + 203 + 15 + 325 + 15 + 20 + 20);
    click(&mut o, vx, vy);
    o.extend([hud("menu.env", 100_000.0, "now"), Op::Frame(DT)]);
    click(&mut o, vx + 140, vy);
    o.extend([hud("menu.env", 125_000.0, "now"), Op::Frame(DT)]);
    click(&mut o, vx + 140, vy);
    o.extend([hud("menu.env", 300_000.0, "now"), Op::Frame(DT)]);
    click(&mut o, vx, vy);
    o.extend([hud("menu.vote", 1.0, "type"), hud("menu.vote", 15.0, "max"), hud("menu.vote", 12.5, "left"), Op::Frame(DT)]);
    click(&mut o, vx, vy + 80);
    o.extend([hud("menu.vote", 1.0, "voted"), hud("menu.vote", 7.25, "left"), Op::Frame(DT)]);
    o.extend([hud("menu.vote", 0.0, "voted"), hud("menu.vote", 0.0, "type"), Op::Frame(DT), hud("menu.vote", 1.0, "voted"), Op::Frame(DT)]);
    o.extend([hud("menu.vote", 0.0, "voted"), hud("menu.vote", 2.0, "type"), hud("menu.vote", 4.0, "target"), hud("menu.vote", 3.0, "left"), Op::Frame(DT)]);
    click(&mut o, vx + 140, vy + 80);
    o.extend([hud("menu.vote", 1.0, "voted"), Op::Frame(DT), hud("menu.vote", 7.0, "type"), hud("menu.vote", 0.0, "voted"), Op::Frame(DT), hud("menu.vote", 0.0, "left"), hud("menu.vote", 3.0, "type"), Op::Frame(DT)]);
    // a new session lets the requests go; the votes go in the last 30 s of a practice
    o.extend([hud("menu.newSession", 0.0, ""), Op::Frame(DT), hud("timeLeft", 29_000.0, ""), Op::Frame(DT), hud("session", 3.0, ""), Op::Frame(DT)]);
    // the header: waiting, a countdown
    o.extend([hud("menu.env", 70_000.0, "wait"), Op::Frame(DT), hud("menu.env", 0.0, "wait"), hud("tts", 8000.0, ""), Op::Frame(DT), hud("tts", 0.0, ""), Op::Frame(DT)]);
    o.extend([hud("menu.resume", 0.0, ""), Op::Frame(DT)]);
    o
}

/// What a script says for the replay's bar that the manager's own members do not hold.
#[derive(Clone, Debug)]
pub struct ReplayScript {
    /// `ReplayManager::getNormalizedLapChange` of the focused car
    pub lap_marks: Vec<f32>,
    /// the game object `ACCameraManager` is there; the track's camera sets; the camera's number
    pub camera_manager: bool,
    pub camera_sets: Vec<String>,
    pub camera_index: i32,
}

/// `ReplayManager::getAutoFileName` in the scripts (made up).
pub const REPLAY_AUTO_NAME: &str = "ks_ferrari_f2004_magione__131026-142501";

/// The members of `ReplayManager` the bar reads, and the rules its transport calls change
/// them by in the scripts (the manager itself is Task 28c's).
#[derive(Clone, Copy, Debug)]
pub struct ReplayModel {
    pub status: i32,
    pub time_mult: f32,
    pub slow_mo_level: f32,
    pub current_frame: i32,
    pub recorded_frames: i32,
    pub cut_in: i32,
    pub cut_out: i32,
    pub has_loaded: bool,
}

impl Default for ReplayModel {
    fn default() -> ReplayModel {
        ReplayModel { status: 2, time_mult: 0.0, slow_mo_level: 2.0, current_frame: 0, recorded_frames: 0, cut_in: 0, cut_out: 0, has_loaded: false }
    }
}

/// A transport call on the manager: its members after it, and the line that says it was made.
pub fn replay_apply(m: &mut ReplayModel, what: &str, value: f32) -> String {
    let clamp01 = |p: f32| if p > 1.0 { 1.0 } else if p >= 0.0 { p } else { 0.0 };
    match what {
        "play" => {
            m.time_mult = 1.0;
            m.status = 0;
        }
        "pause" => {
            m.status = 1;
            m.time_mult = 0.0;
        }
        "stop" => {
            m.time_mult = 0.0;
            m.status = 2;
            m.current_frame = m.cut_in;
        }
        "nextFrame" => {
            m.status = 12;
            m.time_mult = 0.49;
        }
        "rewind" => {
            if m.time_mult > -1.0 {
                m.time_mult = -1.0;
            }
            m.time_mult = (m.time_mult - 1.0).clamp(-20.0, 0.0);
            m.status = 3;
        }
        "fastForward" => {
            if m.time_mult < 1.0 {
                m.time_mult = 1.0;
            }
            m.time_mult = (m.time_mult + 1.0).clamp(0.0, 20.0);
            m.status = 4;
        }
        "slowMotion" => {
            m.slow_mo_level = value;
            m.status = 5;
            m.time_mult = 1.0 / value;
        }
        "setNormalizedPosition" => m.current_frame = (m.recorded_frames as f32 * clamp01(value)) as i32,
        // ReplayManager::setCutInPerc 0x14015af30, setCutOutPerc 0x14015aff0
        "cutIn" => {
            m.cut_in = ((m.recorded_frames - 1) as f32 * clamp01(value)) as i32;
            if m.current_frame < m.cut_in {
                m.current_frame = m.cut_in;
            }
        }
        "cutOut" => {
            m.cut_out = ((m.recorded_frames - 1) as f32 * clamp01(value)) as i32;
            if m.cut_out <= m.current_frame {
                m.current_frame = m.cut_out;
            }
        }
        _ => {}
    }
    format!("replay {what} {:08x}", value.to_bits())
}

/// A member of the manager set by the script.
pub fn replay_set(m: &mut ReplayModel, field: &str, number: f32) {
    match field {
        "status" => m.status = number as i32,
        "mult" => m.time_mult = number,
        "level" => m.slow_mo_level = number,
        "frame" => m.current_frame = number as i32,
        "frames" => m.recorded_frames = number as i32,
        "cutIn" => m.cut_in = number as i32,
        "cutOut" => m.cut_out = number as i32,
        "loaded" => m.has_loaded = number != 0.0,
        _ => {}
    }
}

/// The bar's own state in the dump.
pub fn replay_extra(force_camera: bool, target_y: f32, stay_up: f32, last: i32, names: usize) -> String {
    format!(" replay={},{:08x},{:08x},{last},{names}", force_camera as u8, target_y.to_bits(), stay_up.to_bits())
}

/// The manager's members in the dump, and the camera that drives with the car.
pub fn replay_line(m: &ReplayModel, drivable: i32) -> String {
    format!("replay status={} mult={:08x} level={:08x} frame={}/{} cut={},{} loaded={} drivable={drivable}{}", m.status, m.time_mult.to_bits(), m.slow_mo_level.to_bits(), m.current_frame, m.recorded_frames, m.cut_in, m.cut_out, m.has_loaded as u8, char::from(10u8))
}

/// The replay's bar: it comes, lays itself out, finds the cameras; every button; the time
/// line (a click, the two cut markers dragged, the laps' marks); the spinners; the box that
/// asks for a name; the bar slides away and back; "Exit" in a replay of the session and in
/// a replay opened from a file. A screen of 1280 x 720 is taken for the mouse.
pub fn replay_bar(cars: usize) -> Vec<Op> {
    let mut o = Vec::new();
    field(&mut o, cars);
    let set = |o: &mut Vec<Op>, what: &'static str, value: f32| o.push(hud("menu.replaySet", value, what));
    let click = |o: &mut Vec<Op>, x: i32, y: i32| o.extend([Op::MouseMove(x, y), Op::MouseDown(x, y), Op::MouseUp(x, y), Op::Frame(DT)]);
    o.extend([hud("menu.replay", 0.0, ""), Op::Frame(DT)]);
    set(&mut o, "frames", 3000.0);
    set(&mut o, "cutOut", 2999.0);
    o.extend([hud("menu.replayLaps", 0.0, "0.21,0.5,0.93"), hud("menu.replayShow", 1.0, ""), Op::Frame(DT), Op::Frame(DT)]);
    // the camera manager is found: the cameras' names, the spinner goes to the camera in use
    o.extend([hud("menu.cameraSets", 0.0, "Camera set 1|TV 2"), hud("menu.cameraAt", 3.0, ""), hud("menu.replayCamera", 1.0, ""), Op::Frame(DT), Op::Frame(DT)]);
    let (row, bar) = (673, 172);
    let (exit, save, rewind, stop, play, next, forward, slow, photo) = (bar + 73, bar + 142, bar + 211, bar + 280, bar + 349, bar + 418, bar + 487, bar + 579, bar + 871);
    o.extend([Op::MouseMove(640, 700), Op::Frame(DT)]);
    for x in [exit, save, rewind, stop, play, next, forward, slow, photo] {
        o.extend([Op::MouseMove(x, row), Op::Frame(DT)]);
    }
    // the transport
    for (i, x) in [play, play, forward, forward, forward, forward, forward, forward, rewind, rewind, rewind, rewind, rewind, rewind, rewind, slow, slow, slow, next, stop, play, photo].into_iter().enumerate() {
        set(&mut o, "frame", 100.0 + 137.0 * i as f32);
        click(&mut o, x, row);
    }
    // the spinners: the slow motion's level, the camera, the car (both ends of each)
    for spinner in [539, 639, 739] {
        let (left, right, y) = (bar + spinner + 8, bar + spinner + 72, 620 + 41 + 11);
        for x in [right, right, right, left, right, right, right, right, right, left, left] {
            click(&mut o, x, y);
        }
    }
    // another camera is picked elsewhere: the spinner follows without picking it again
    o.extend([hud("menu.cameraAt", 6.0, ""), hud("menu.cameraIndex", 6.0, ""), Op::Frame(DT), hud("menu.cameraIndex", 6.0, ""), Op::Frame(DT)]);
    // the time line: clicks, the cut markers dragged and let go
    for x in [500, 980, 294, 640] {
        click(&mut o, x, 638);
    }
    o.extend([Op::MouseMove(284, 634), Op::MouseDown(284, 634), Op::MouseMove(400, 634), Op::Frame(DT), Op::MouseUp(400, 634), Op::Frame(DT)]);
    o.extend([Op::MouseMove(994, 634), Op::MouseDown(994, 634), Op::MouseMove(800, 636), Op::Frame(DT), Op::MouseUp(800, 636), Op::Frame(DT)]);
    click(&mut o, 300, 638);
    click(&mut o, 600, 638);
    click(&mut o, stop, row);
    // a long replay: the times with hours
    set(&mut o, "frames", 400_000.0);
    set(&mut o, "frame", 250_123.0);
    o.push(Op::Frame(DT));
    set(&mut o, "frames", 3000.0);
    set(&mut o, "frame", 1500.0);
    // the box that asks for the name: opened, written in, given; opened and closed
    click(&mut o, save, row);
    click(&mut o, 400, 582);
    o.extend([Op::KeyChar(0x5f), Op::KeyChar(0x61), Op::KeyChar(0x42), Op::Frame(DT), Op::MouseMove(640, 500), Op::Frame(DT), Op::MouseMove(640, 590), Op::Frame(DT)]);
    click(&mut o, 172 + 934 - 25, 582);
    o.extend([hud("menu.replaySaved", 1.0, ""), Op::Frame(DT)]);
    click(&mut o, save, row);
    o.push(Op::KeyChar(0x0d));
    click(&mut o, save, row);
    click(&mut o, save, row);
    click(&mut o, 400, 582);
    o.extend([Op::KeyChar(0x2f), Op::KeyChar(0x0d), Op::Frame(DT), hud("menu.replaySaved", 0.0, ""), Op::Frame(DT)]);
    // the mouse leaves the bottom of the screen: the bar waits, slides away; and comes back
    o.extend([Op::MouseMove(640, 300), Op::Frame(1.0), Op::Frame(1.0), Op::Frame(1.0), Op::Frame(0.1), Op::Frame(0.1), Op::Frame(0.1), Op::Frame(0.2), Op::Frame(0.2), Op::MouseMove(640, 716), Op::Frame(0.1), Op::Frame(0.1), Op::Frame(0.2), Op::Frame(0.2), Op::Frame(DT)]);
    // hidden and shown again; "Exit"
    o.extend([hud("menu.cameraAt", 1.0, ""), hud("menu.replayShow", 0.0, ""), Op::Frame(DT), hud("menu.replayShow", 1.0, ""), Op::Frame(DT), Op::Frame(DT)]);
    click(&mut o, exit, row);
    set(&mut o, "loaded", 1.0);
    click(&mut o, exit, row);
    o.extend([hud("menu.replayShow", 0.0, ""), Op::Frame(DT)]);
    o
}

/// An online session's own overlays: the ping in the corner in its three colours, the
/// server's welcome message, the red screen of a driver who is counted out.
pub fn online_overlays() -> Vec<Op> {
    let mut o = vec![hud("create", 2.0, ""), hud("car.name", 0.0, "Tazio Nuvolari"), hud("online", 1.0, ""), hud("menu.qos", 0.0, ""), Op::Frame(DT)];
    for ping in [23.0, 70.0, 71.0, 120.0, 121.0, 999.0, 0.0, -1.0] {
        o.extend([hud("menu.ping", ping, ""), Op::Frame(DT)]);
    }
    o.extend([hud("menu.welcome", 0.0, "Welcome to the made-up server|Be fair. No wrecking.||Have fun"), Op::Frame(DT), Op::Frame(DT), hud("menu.welcome", 0.0, ""), Op::Frame(DT)]);
    for now in [61_000.0, 61_999.0, 62_000.0, 63_500.0] {
        o.extend([hud("menu.env", now, "now"), hud("menu.disconnect", 0.0, "You have been voted to be kicked from the race"), Op::Frame(DT)]);
    }
    o.extend([hud("menu.welcome", 0.0, "both at once"), Op::Frame(DT), hud("menu.disconnect", 0.0, ""), hud("menu.welcome", 0.0, ""), Op::Frame(DT)]);
    o
}
