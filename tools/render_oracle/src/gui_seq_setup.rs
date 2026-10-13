// SPDX-License-Identifier: GPL-3.0-or-later

//! The scripts of the setup editor's proofs (Task 31) and what both sides share of them.

use crate::gui_seq::{Op, Sequence};
use crate::gui_seq_menu::{field, hud, leak};
use crate::Args;

const DT: f32 = 1.0 / 60.0;

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
    for track in ["ai", "imola", "ks_nordschleife", "magione", "spa"] {
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

pub fn sequence(name: &str) -> Option<Vec<Op>> {
    let rest = name.strip_prefix("setup_")?;
    let (kind, car) = rest.split_once('@').unwrap_or((rest, "ks_ferrari_f2004"));
    let car = leak(car.to_string());
    Some(match kind {
        "probe" => open(car),
        _ => return None,
    })
}
