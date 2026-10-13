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
}

static SCRIPT: Mutex<MenuScript> = Mutex::new(MenuScript { session_count: 1, replay_manager: false });
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
