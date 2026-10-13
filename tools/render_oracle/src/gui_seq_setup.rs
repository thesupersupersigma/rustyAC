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
    // the game's English strings and setup help
    for file in ["system/locales/en.ini", "system/locales/setup/en.ini", "system/locales/setup/en.tag"] {
        let target = args.root.join(file);
        std::fs::create_dir_all(target.parent().unwrap_or(&args.root)).map_err(|e| e.to_string())?;
        std::fs::copy(args.game.join(file), &target).map_err(|e| format!("{file}: {e}"))?;
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
