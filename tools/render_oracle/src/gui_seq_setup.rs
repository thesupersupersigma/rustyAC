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

pub fn sequence(name: &str) -> Option<Vec<Op>> {
    let rest = name.strip_prefix("setup_")?;
    let (kind, car) = rest.split_once('@').unwrap_or((rest, "ks_ferrari_f2004"));
    let car = leak(car.to_string());
    Some(match kind {
        "probe" => open(car),
        "tabs" => tabs(car, 14),
        "io" => io(car),
        _ => return None,
    })
}
