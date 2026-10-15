// SPDX-License-Identifier: GPL-3.0-or-later

//! The scripts of the setup editor's proofs (Task 31) and what both sides share of them.

use crate::gui_seq::{Op, Sequence};
use crate::gui_seq_menu::{field, hud, leak};
use crate::Args;

const DT: f32 = 1.0 / 60.0;
/// The track folders of the scratch root ("ai" is one the game leaves out).
const TRACKS: [&str; 7] = ["ai", "imola", "ks_nordschleife", "ks_nurburgring", "magione", "monza", "spa"];
/// Where the setup screen's pages are on a screen of 1280 by 720.
const TAB_X: i32 = 200;
const TAB_Y: i32 = 65;
const SCROLL_RIGHT: (i32, i32) = (1226, 16);
const SCROLL_LEFT: (i32, i32) = (214, 16);

/// One setup item in the dump: name, `newValue`, attached, the car's own value.
pub fn item_line(name: &str, new_value: f32, attached: bool, value: f32) -> String {
    format!("item {name} {:08x} {} {:08x}\n", new_value.to_bits(), attached as u8, value.to_bits())
}

static STATE: std::sync::Mutex<f32> = std::sync::Mutex::new(0.0);

/// `Hud("menu.setupState", seed, "")`: the car's state from here on (0: all nothing).
pub fn set_car_state(seed: f32) {
    *STATE.lock().unwrap() = seed;
}

/// The car's state the script last asked for: numbers made from its seed.
pub fn car_state() -> rustyac_gui::setup_car::SetupCarState {
    use rustyac_gui::setup_car::{SetupCarState, TyreInfo};
    let seed = *STATE.lock().unwrap();
    let mut state = SetupCarState::default();
    if seed == 0.0 {
        return state;
    }
    for w in 0..4 {
        let k = seed + w as f32;
        state.camber_rad[w] = -0.031 - 0.0043 * k;
        // the suspension: turned about the upright axis by a small angle (the toe)
        let angle = 0.0021 * (k - 2.3) * if w % 2 == 0 { 1.0 } else { -1.0 };
        let (s, c) = (angle.sin(), angle.cos());
        state.suspension_matrix[w].m = [[c, 0.0, -s, 0.0], [0.0, 1.0, 0.0, 0.0], [s, 0.0, c, 0.0], [0.7 * k, 0.3, 1.2, 1.0]];
        state.tyres[w] = TyreInfo { last_grain: 3.7 * k, last_blister: 0.9 * k, dynamic_pressure: 27.4 + k, static_pressure: 21.9 + k * 0.5, last_set_imo: if w == 3 && seed > 5.0 { [-60.0, 0.0, 0.0] } else { [81.3 + k, 79.9 + k, 74.2 - k] } };
    }
    state.caster = 4.57 + seed * 0.11;
    state.ride_height = [0.0417 + seed * 0.001, 0.0652 - seed * 0.002];
    state.min_height_m = [0.045, 0.0, -1.0, 0.0437][seed as usize % 4];
    state
}

static TIME: std::sync::Mutex<f32> = std::sync::Mutex::new(0.0);

/// `Hud("menu.timeLaps", seed, "")`: the laps the page "Time" shows (0: the page is the
/// stand-in of the older scripts).
pub fn set_time_seed(seed: f32) {
    *TIME.lock().unwrap() = seed;
}

pub fn time_seed() -> f32 {
    *TIME.lock().unwrap()
}

/// The laps of every car in the running session and in the sessions before it, made from
/// the script's seed: some invalid, some cars with none, some laps without sector times.
pub fn time_link(cars: usize, current_session: i32, focused: usize) -> rustyac_gui::time_table::TimeLink {
    use rustyac_gui::time_table::{TimeHistory, TimeLap, TimeLink};
    let seed = time_seed() as usize;
    let laps_of = |car: usize, session: usize| -> Vec<TimeLap> {
        let n = (seed * 3 + car * 5 + session * 7) % 23;
        (0..n)
            .map(|k| {
                let time = 80_000 + ((k * 7919 + car * 104_729 + (seed + session) * 31) % 9000) as u32;
                let (a, b) = (time / 3 - 500 + (k as u32 * 37) % 300, time / 3 + 200 - (k as u32 * 53) % 250);
                let valid = (k + car + session) % 6 != 5;
                TimeLap { time: if k % 11 == 10 { 0 } else { time }, splits: if (k + car) % 9 == 8 { Vec::new() } else { vec![a, b, time - a - b] }, cuts: !valid as u32, is_valid: valid, compound: ["S", "M", "H", ""][(car + k / 7) % 4].to_string() }
            })
            .collect()
    };
    let session_laps = |session: usize| -> Vec<Vec<TimeLap>> { (0..cars).map(|car| laps_of(car, session)).collect() };
    let history = (0..current_session.max(0) as usize)
        .map(|session| {
            let laps = session_laps(session + 1);
            let best_laps = laps.iter().map(|laps| laps.iter().filter(|lap| lap.is_valid && lap.time != 0).min_by_key(|lap| lap.time).cloned().unwrap_or_default()).collect();
            TimeHistory { laps, best_laps }
        })
        .collect();
    TimeLink { laps: session_laps(0), history, focused_car: focused, sector_count: 3, track_config: String::new(), out_dir: std::path::PathBuf::new(), time_stamp: String::new() }
}

/// The car a script's setup editor is for.
pub fn car_of(ops: &[Op]) -> Option<&'static str> {
    ops.iter().find_map(|op| match op {
        Op::Hud("menu.setupCar", _, car) => Some(*car),
        _ => None,
    })
}

/// The car's data folder in the scratch root (from the repo's `cardata`).
pub fn prepare_root(args: &Args, sequence: &Sequence) -> Result<(), String> {
    let Some(car) = car_of(&sequence.ops) else { return Ok(()) };
    let from = std::path::Path::new("cardata").join(car);
    let to = args.root.join("content/cars").join(car).join("data");
    std::fs::create_dir_all(&to).map_err(|e| e.to_string())?;
    for entry in std::fs::read_dir(&from).map_err(|e| format!("{}: {e}", from.display()))?.filter_map(|e| e.ok()) {
        if entry.path().is_file() {
            std::fs::copy(entry.path(), to.join(entry.file_name())).map_err(|e| e.to_string())?;
        }
    }
    // the assists as the game ships them: what the car has
    std::fs::create_dir_all(args.root.join("cfg")).map_err(|e| e.to_string())?;
    std::fs::write(args.root.join("cfg/assists.ini"), "[ASSISTS]\nTRACTION_CONTROL=1\nABS=1\n").map_err(|e| e.to_string())?;
    // the game's English strings and setup help
    for file in ["system/locales/en.ini", "system/locales/setup/en.ini", "system/locales/setup/en.tag"] {
        let target = args.root.join(file);
        std::fs::create_dir_all(target.parent().unwrap_or(&args.root)).map_err(|e| e.to_string())?;
        std::fs::copy(args.game.join(file), &target).map_err(|e| format!("{file}: {e}"))?;
    }
    Ok(())
}

/// The track folders of the scratch root, as the game's side finds them.
pub fn tracks(root: &std::path::Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(root.join("content/tracks")).map(|d| d.filter_map(|e| e.ok()).filter(|e| e.path().is_dir()).map(|e| e.file_name().to_string_lossy().into_owned()).collect()).unwrap_or_default();
    names.sort_by_key(|name| name.to_uppercase());
    names
}

/// Before both sides start: a few track folders, and each side's own setups folder (the
/// game's in its stand-in Documents, the port's beside it) emptied and filled alike with
/// what `<root>/seed_setups/<car>` holds.
pub fn seed(args: &Args, sequence: &Sequence) -> Result<(), String> {
    let Some(car) = car_of(&sequence.ops) else { return Ok(()) };
    let io = |e: std::io::Error| e.to_string();
    for track in TRACKS {
        std::fs::create_dir_all(args.root.join("content/tracks").join(track)).map_err(io)?;
    }
    for documents in ["documents", "port_documents"] {
        let setups = args.root.join(documents).join("Assetto Corsa/setups");
        let _ = std::fs::remove_dir_all(&setups);
        std::fs::create_dir_all(&setups).map_err(io)?;
        copy_tree(&args.root.join("seed_setups").join(car), &setups.join(car))?;
    }
    Ok(())
}

fn copy_tree(from: &std::path::Path, to: &std::path::Path) -> Result<(), String> {
    let Ok(entries) = std::fs::read_dir(from) else { return Ok(()) };
    std::fs::create_dir_all(to).map_err(|e| e.to_string())?;
    for entry in entries.filter_map(|e| e.ok()) {
        let target = to.join(entry.file_name());
        if entry.path().is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), &target).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

/// The server's setup of a script: `NAME=value|...`.
pub fn server_values(text: &str) -> Vec<(String, f32)> {
    text.split('|').filter(|pair| !pair.is_empty()).map(|pair| {
        let (name, value) = pair.split_once('=').unwrap_or((pair, "0"));
        (name.to_string(), value.parse().unwrap_or(0.0))
    }).collect()
}

/// The TCP message 0x52 past its id: a byte the game does not use, the count, each name (a
/// byte of length and its bytes) and value.
pub fn server_packet(first: u8, text: &str) -> Vec<u8> {
    let values = server_values(text);
    let mut out = vec![first, values.len() as u8];
    for (name, value) in values {
        out.push(name.len() as u8);
        out.extend_from_slice(name.as_bytes());
        out.extend_from_slice(&value.to_le_bytes());
    }
    out
}

fn click(o: &mut Vec<Op>, x: i32, y: i32) {
    o.extend([Op::MouseMove(x, y), Op::MouseDown(x, y), Op::MouseUp(x, y), Op::Frame(DT)]);
}

/// The ESC menu with the setup editor of `car` open.
fn open(car: &'static str) -> Vec<Op> {
    let mut o = Vec::new();
    field(&mut o, 1);
    let gears = std::fs::read_to_string(std::path::Path::new("cardata").join(car).join("drivetrain.ini")).ok().and_then(|text| rustyac_physics::data::ini::IniReader::from_text(std::path::Path::new("drivetrain.ini"), &text.replace('\r', "")).get_int("GEARS", "COUNT").ok()).unwrap_or(0);
    o.extend([hud("car.unixName", 0.0, car), hud("menu.setupCar", gears as f32, car), hud("race.sessions", 0.0, "1:0:Practice:0:30:0"), hud("race.current", 0.0, ""), hud("race.board", 0.0, "")]);
    o.extend([hud("menu.escMenu", 0.0, ""), Op::Frame(DT), hud("menu.escShow", 0.0, ""), Op::Frame(DT)]);
    click(&mut o, 50, 225 + 48);
    o.push(Op::Frame(DT));
    o
}

/// The next page (the tab bar's right arrow), and the frames the bar slides for.
fn next_tab(o: &mut Vec<Op>) {
    click(o, SCROLL_RIGHT.0, SCROLL_RIGHT.1);
    for _ in 0..6 {
        o.push(Op::Frame(DT));
    }
}

fn previous_tab(o: &mut Vec<Op>) {
    click(o, SCROLL_LEFT.0, SCROLL_LEFT.1);
    for _ in 0..6 {
        o.push(Op::Frame(DT));
    }
}

/// Every page of the car's editor in turn: the mouse over every place a spinner's label can
/// be (the help bubbles), a click on plus, two on minus and one on the bar of every place a
/// spinner can be.
fn tabs(car: &'static str, pages: usize) -> Vec<Op> {
    let mut o = open(car);
    for page in 0..pages {
        for row in 0..9 {
            for column in 0..3 {
                let (x, y) = (TAB_X + 23 + column * 110, TAB_Y + 70 + row * 60);
                // the label above the spinner, plus, minus twice, the bar under it
                o.extend([Op::MouseMove(x + 60, y - 10), Op::Frame(DT)]);
                click(&mut o, x + 163 + 10, y + 10);
                o.extend([Op::MouseDown(x + 10, y + 10), Op::MouseUp(x + 10, y + 10), Op::MouseDown(x + 10, y + 10), Op::MouseUp(x + 10, y + 10), Op::Frame(DT)]);
                if (row + column + page as i32) % 4 == 0 {
                    click(&mut o, x + 21 + 100, y + 23);
                }
            }
        }
        // the pages whose spinners are elsewhere: the compound's and the fuel's arrows, the gears
        for (x, y) in [(50 + 329 + 10, 100 + 10), (50 + 10, 100 + 10), (133 + 163 + 10, 100 + 10), (133 + 10, 100 + 10), (133 + 163 + 10, 80 + 10), (133 + 10, 80 + 10)] {
            click(&mut o, TAB_X + x, TAB_Y + y);
        }
        for gear in 0..8 {
            let y = TAB_Y + 55 + 72 * gear + 10;
            o.extend([Op::MouseMove(TAB_X + 135 + 60, y - 20), Op::Frame(DT)]);
            click(&mut o, TAB_X + 135 + 163 + 10, y);
            click(&mut o, TAB_X + 135 + 10, y);
            click(&mut o, TAB_X + 135 + 10, y);
        }
        o.extend([Op::MouseMove(640, 700), Op::Frame(DT)]);
        if page % 3 == 2 {
            // the menu goes and comes: every page sends its values again
            o.extend([hud("menu.resume", 0.0, ""), Op::Frame(DT), hud("menu.escShow", 0.0, ""), Op::Frame(DT)]);
        }
        next_tab(&mut o);
    }
    previous_tab(&mut o);
    previous_tab(&mut o);
    o
}

fn type_text(o: &mut Vec<Op>, text: &str) {
    click(o, TAB_X + 15 + 100, TAB_Y + 565 + 12);
    // (what the field holds is rubbed out first)
    for _ in 0..24 {
        o.push(Op::KeyChar(8));
    }
    o.extend(text.chars().map(|c| Op::KeyChar(c as u32)));
    o.push(Op::Frame(DT));
}

/// A row of the tracks' (0) or the setups' (1) list.
fn list_row(o: &mut Vec<Op>, list: i32, row: i32) {
    click(o, TAB_X + [15, 232][list as usize] + 60, TAB_Y + 110 + (row as f32 * 19.34) as i32 + 9);
}

const LOAD: i32 = 15;
const RESET: i32 = 130;
const DELETE: i32 = 270;
const SAVE: i32 = 385;

fn button(o: &mut Vec<Op>, x: i32) {
    click(o, TAB_X + x + 25, TAB_Y + 410 + 25);
}

/// The page "Setup I/O": saving (a new name, the same again and its "Override", a name that
/// is no file name, no name), loading, the reset, deleting with its confirmation.
fn io(car: &'static str) -> Vec<Op> {
    let mut o = open(car);
    // something to save: the gears' and the tyres' pages, then back
    next_tab(&mut o);
    click(&mut o, TAB_X + 135 + 163 + 10, TAB_Y + 55 + 10);
    click(&mut o, TAB_X + 135 + 163 + 10, TAB_Y + 55 + 72 + 10);
    next_tab(&mut o);
    click(&mut o, TAB_X + 50 + 329 + 10, TAB_Y + 110);
    for column in 0..2 {
        click(&mut o, TAB_X + 23 + column * 220 + 163 + 10, TAB_Y + 70 + 2 * 60 + 10);
    }
    previous_tab(&mut o);
    previous_tab(&mut o);
    // no track chosen, no name
    button(&mut o, SAVE);
    type_text(&mut o, "first");
    button(&mut o, SAVE);
    list_row(&mut o, 0, 1);
    button(&mut o, SAVE);
    type_text(&mut o, "first");
    button(&mut o, SAVE);
    button(&mut o, SAVE);
    type_text(&mut o, "bad/name");
    button(&mut o, SAVE);
    // another setup: more changes, saved for the same track; then for "generic"
    for _ in 0..3 {
        next_tab(&mut o);
    }
    click(&mut o, TAB_X + 133 + 10, TAB_Y + 110);
    click(&mut o, TAB_X + 133 + 10, TAB_Y + 110);
    for _ in 0..3 {
        previous_tab(&mut o);
    }
    list_row(&mut o, 0, 1);
    type_text(&mut o, "Second one");
    button(&mut o, SAVE);
    list_row(&mut o, 0, 0);
    type_text(&mut o, "zz");
    // (Enter saves nothing)
    o.extend([Op::KeyChar(13), Op::Frame(DT)]);
    button(&mut o, SAVE);
    // loading: every setup of every track
    for track in 0..7 {
        list_row(&mut o, 0, track);
        for setup in 0..3 {
            list_row(&mut o, 1, setup);
            button(&mut o, LOAD);
        }
    }
    button(&mut o, RESET);
    o.push(Op::Frame(1.0));
    o.push(Op::Frame(4.0));
    // deleting: asked, given up (a click in the text), asked again and done
    list_row(&mut o, 0, 1);
    list_row(&mut o, 1, 0);
    button(&mut o, DELETE);
    click(&mut o, TAB_X + 15 + 100, TAB_Y + 565 + 12);
    button(&mut o, DELETE);
    click(&mut o, TAB_X + 390 + 25, TAB_Y + 570 + 8);
    list_row(&mut o, 1, 0);
    button(&mut o, LOAD);
    // the menu goes and comes with a page other than this one in front
    next_tab(&mut o);
    o.extend([hud("menu.resume", 0.0, ""), Op::Frame(DT), hud("menu.escShow", 0.0, ""), Op::Frame(DT)]);
    previous_tab(&mut o);
    o
}

/// Plus and minus on every place a spinner can be on the page in front.
fn poke_page(o: &mut Vec<Op>) {
    for row in 0..9 {
        for column in 0..3 {
            let (x, y) = (TAB_X + 23 + column * 110, TAB_Y + 70 + row * 60);
            click(o, x + 163 + 10, y + 10);
            click(o, x + 10, y + 10);
        }
    }
    for (x, y) in [(50 + 329 + 10, 100 + 10), (50 + 10, 100 + 10), (133 + 163 + 10, 100 + 10), (133 + 10, 100 + 10), (133 + 163 + 10, 80 + 10), (133 + 10, 80 + 10)] {
        click(o, TAB_X + x, TAB_Y + y);
    }
    for gear in 0..8 {
        click(o, TAB_X + 135 + 163 + 10, TAB_Y + 55 + 72 * gear + 10);
    }
    o.extend([Op::MouseMove(640, 700), Op::Frame(DT)]);
}

/// Online (Task 34): the server's setup arrives (TCP 0x52) and locks what it names, a name
/// twice, a name no item has, values that are no whole numbers; every page poked; a saved
/// setup loaded over it; a second message; the menu gone and back.
fn server(car: &'static str) -> Vec<Op> {
    let mut o = open(car);
    o.extend([hud("menu.setupServer", 0.0, "PRESSURE_LF=27.5|CAMBER_LF=-25|FUEL=42.9|ARB_FRONT=3|WING_1=1e-05|NOT_AN_ITEM=5|PRESSURE_LF=26|TYRES=1|FINAL_GEAR_RATIO=2|GEAR_3=4|DAMP_BUMP_LF=12345678|TOE_OUT_RR=-0.75"), Op::Frame(DT), Op::Frame(DT)]);
    for _ in 0..6 {
        poke_page(&mut o);
        next_tab(&mut o);
    }
    for _ in 0..6 {
        previous_tab(&mut o);
    }
    list_row(&mut o, 0, 1);
    list_row(&mut o, 1, 0);
    button(&mut o, LOAD);
    o.extend([hud("menu.setupServer", 1.0, "PRESSURE_RF=25|FUEL=10|ARB_REAR=2|DIFF_POWER=40"), Op::Frame(DT), Op::Frame(DT)]);
    for _ in 0..6 {
        poke_page(&mut o);
        next_tab(&mut o);
    }
    o.extend([hud("menu.resume", 0.0, ""), Op::Frame(DT), hud("menu.escShow", 0.0, ""), Op::Frame(DT)]);
    poke_page(&mut o);
    o
}

/// The page "PITSTOP STRATEGY" from "Setup I/O" and back: it is the last page, and the tab
/// bar's arrows go round.
fn to_pit_page(o: &mut Vec<Op>) {
    previous_tab(o);
}

fn to_io_page(o: &mut Vec<Op>) {
    next_tab(o);
}

/// The editor with the pit menu of the arrow keys, and its page in front.
fn open_pit(car: &'static str) -> Vec<Op> {
    let mut o = open(car);
    let at = o.iter().position(|op| matches!(op, Op::Hud("menu.setupCar", ..))).unwrap_or(0) + 1;
    let pit = [hud("pit.pressure0", 26.0, ""), hud("pit.pressure1", 27.0, ""), hud("pit.pressure2", 25.0, ""), hud("pit.pressure3", 24.0, ""), hud("pit.wing1", 3.0, ""), hud("menu.setupPit", 0.0, "")];
    o.splice(at..at, pit);
    to_pit_page(&mut o);
    o
}

/// On the page "PITSTOP STRATEGY": a preset's button, and plus or minus of its spinners.
fn preset_button(o: &mut Vec<Op>, preset: i32) {
    click(o, TAB_X + 20 + preset * 137 + 60, TAB_Y + 60 + 10);
}

fn pit_spinner(o: &mut Vec<Op>, spinner: usize, plus: bool, times: usize) {
    let (x, y, w) = [(133, 120, 184), (50, 180, 350), (20, 240, 184), (246, 240, 184), (20, 300, 184), (246, 300, 184)][spinner];
    for _ in 0..times {
        click(o, TAB_X + x + if plus { w - 21 + 10 } else { 10 }, TAB_Y + y + 10);
    }
}

fn poke_presets(o: &mut Vec<Op>, round: usize) {
    for preset in 0..3 {
        preset_button(o, preset);
        pit_spinner(o, 0, true, preset as usize + 2 + round);
        pit_spinner(o, 0, false, 1);
        pit_spinner(o, 1, true, preset as usize + 1);
        pit_spinner(o, 2, true, 1);
        pit_spinner(o, 3, false, 2);
        pit_spinner(o, 4, true, 3 + round);
        pit_spinner(o, 5, false, 1);
        o.extend([Op::MouseMove(640, 700), Op::Frame(DT)]);
    }
    for preset in [0, 2, 1] {
        preset_button(o, preset);
    }
}

/// Task 34: the page "PITSTOP STRATEGY": every preset changed, saved with a setup (its `.sp`),
/// changed again and loaded back; the server's fixed setup (its pressures locked, its fuel the
/// most to add, its wing fixed), the page shown again, saved once more.
fn pit_page(car: &'static str) -> Vec<Op> {
    let mut o = open_pit(car);
    poke_presets(&mut o, 0);
    o.extend([hud("menu.resume", 0.0, ""), Op::Frame(DT), hud("menu.escShow", 0.0, ""), Op::Frame(DT)]);
    to_io_page(&mut o);
    list_row(&mut o, 0, 1);
    type_text(&mut o, "strategy");
    button(&mut o, SAVE);
    to_pit_page(&mut o);
    poke_presets(&mut o, 1);
    to_io_page(&mut o);
    list_row(&mut o, 1, 0);
    button(&mut o, LOAD);
    to_pit_page(&mut o);
    o.extend([hud("menu.setupServer", 0.0, "PRESSURE_LF=27|PRESSURE_RR=23.5|FUEL=30|WING_1=2|ARB_FRONT=3"), Op::Frame(DT), Op::Frame(DT)]);
    poke_presets(&mut o, 2);
    previous_tab(&mut o);
    next_tab(&mut o);
    poke_presets(&mut o, 3);
    to_io_page(&mut o);
    list_row(&mut o, 0, 0);
    type_text(&mut o, "fixed");
    button(&mut o, SAVE);
    o
}

/// The ESC menu's page "Time": the laps of the focused car in the running session and in
/// the ones before it, page by page; another car focused; the session picked on the page
/// "Race control"; the menu gone and back.
fn time_page() -> Vec<Op> {
    let mut o = Vec::new();
    field(&mut o, 3);
    o.extend([hud("race.sessions", 0.0, "1:0:Practice:0:30:0|2:0:Qualifying:0:15:0|3:0:Quick Race:5:0:0"), hud("race.history", 0.0, crate::gui_seq_menu::board(3, false, 4)), hud("race.current", 1.0, ""), hud("race.board", 0.0, crate::gui_seq_menu::board(3, false, 1)), hud("session", 2.0, ""), hud("timeLeft", 1_234_567.0, ""), hud("menu.timeLaps", 5.0, "")]);
    o.extend([hud("menu.escMenu", 0.0, ""), Op::Frame(DT), hud("menu.escShow", 0.0, ""), Op::Frame(DT)]);
    let time = |o: &mut Vec<Op>| click(o, 50, 325 + 48);
    time(&mut o);
    o.push(Op::Frame(DT));
    // the pages of the laps: next until past the end, then back
    for _ in 0..3 {
        click(&mut o, 100 + 466 + 20, 658);
    }
    for _ in 0..4 {
        click(&mut o, 100 + 416 + 20, 658);
    }
    for car in [1.0, 2.0, 0.0] {
        o.extend([hud("focus", car, ""), Op::Frame(DT), Op::Frame(DT)]);
        click(&mut o, 100 + 466 + 20, 658);
    }
    // other laps come in while the page shows; the menu goes and comes
    o.extend([hud("menu.timeLaps", 6.0, ""), Op::Frame(DT), hud("menu.resume", 0.0, ""), Op::Frame(DT), hud("menu.escShow", 0.0, ""), Op::Frame(DT)]);
    // the sessions before: picked on the page "Race control" (its bars at the top), then "Time" again
    for session in 0..2 {
        click(&mut o, 50, 525 + 48);
        click(&mut o, 425 + 150 * session + 20, 55 + 20 + 12);
        time(&mut o);
        o.push(Op::Frame(DT));
        o.extend([hud("focus", 1.0, ""), Op::Frame(DT), hud("focus", 0.0, ""), Op::Frame(DT)]);
        click(&mut o, 100 + 466 + 20, 658);
        time(&mut o);
    }
    o
}

pub fn sequence(name: &str) -> Option<Vec<Op>> {
    let rest = name.strip_prefix("setup_")?;
    let (kind, car) = rest.split_once('@').unwrap_or((rest, "ks_ferrari_f2004"));
    let car = leak(car.to_string());
    Some(match kind {
        "probe" => open(car),
        "tabs" => tabs(car, 14),
        "time" => time_page(),
        "io" => io(car),
        "server" => server(car),
        "pit" => pit_page(car),
        "pitprobe" => open_pit(car),
        "server1" => {
            // (only the first message: its file is the one left)
            let mut o = server(car);
            let at = o.iter().position(|op| matches!(op, Op::Hud("menu.setupServer", ..))).unwrap_or(0);
            o.truncate(at + 3);
            poke_page(&mut o);
            o
        }
        _ => return None,
    })
}
