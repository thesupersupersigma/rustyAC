// SPDX-License-Identifier: GPL-3.0-or-later

//! The port side of the apps' proofs: `rustyac_gui`'s app forms, fed the same tapes.

use rustyac_gui::control::Id;
use rustyac_gui::gui::Gui;
use rustyac_gui::painter::Painter;

#[derive(Default)]
pub struct AppsPort {
    pub made: Vec<(String, Id)>,
}

fn app(apps: &AppsPort, name: &str) -> Result<Id, String> {
    apps.made.iter().find(|a| a.0 == name).map(|a| a.1).ok_or(format!("no app {name:?} was made"))
}

/// The game's folder (`content/...` and `system/cfg/...` are read from there by the apps).
pub fn make(gui: &mut Gui, painter: &mut dyn Painter, game_folder: &std::path::Path, name: &str) -> Result<Id, String> {
    let read = |path: std::path::PathBuf| rustyac_physics::data::ini::IniReader::load(&path).ok().filter(|ini| ini.ready);
    Ok(match name {
        "tyres" => gui.new_tyres_app(painter, read(game_folder.join("system/cfg/tyres_app.ini")).as_ref()),
        "pedals" => gui.new_pedals_app(painter, read(game_folder.join("documents/Assetto Corsa/cfg/controls.ini")).as_ref()),
        "electronics" => gui.new_electronics_app(painter),
        "laps" => gui.new_laps_app(painter),
        "position" => gui.new_race_position_app(painter),
        "essentials" => gui.new_essentials_app(painter),
        "delta" => gui.new_performance_delta_app(painter),
        "realtime" => gui.new_realtime_app(painter),
        _ => return Err(format!("no app {name:?} in the port")),
    })
}

pub fn call(apps: &mut AppsPort, gui: &mut Gui, painter: &mut dyn Painter, game_folder: &std::path::Path, screen: Id, what: &str, number: f32, text: &str) -> Result<(), String> {
    sync(gui);
    match what {
        "app.create" => {}
        "app.new" => {
            let form = make(gui, painter, game_folder, text)?;
            gui.game_screen_add_control(painter, screen, form, number == 0.0);
            apps.made.push((text.to_string(), form));
        }
        "app.visible" => {
            let form = app(apps, text)?;
            gui.set_visible(form, number != 0.0);
        }
        other => return Err(format!("the port's side knows no {other:?}")),
    }
    Ok(())
}

/// What the apps read, as the script has it now.
pub fn sync(gui: &mut Gui) {
    let actions = std::mem::take(&mut gui.apps.actions);
    gui.apps = crate::gui_seq_apps::link();
    gui.apps.actions = actions;
}
