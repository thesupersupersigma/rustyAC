// SPDX-License-Identifier: GPL-3.0-or-later

//! The port side of the GUI oracle's HUD, part two: the objects besides the messages.

use rustyac_gui::control::Id;
use rustyac_gui::gui::Gui;
use rustyac_gui::hud_flags::{DriveThroughPenalty, FlagDisplay, FuelLight, LapInvalidator, LockControlsTime, WrongWayIndicator};
use rustyac_gui::painter::Painter;
use rustyac_physics::data::ini::IniReader;

use crate::gui_port_hud::HudPort;

pub struct Flags {
    pub wrong: WrongWayIndicator,
    pub lap: LapInvalidator,
    pub drive: DriveThroughPenalty,
    pub lock: LockControlsTime,
    pub flags: FlagDisplay,
    pub fuel: FuelLight,
    pub black_flagged: bool,
}

#[derive(Default)]
pub struct More {
    pub flags: Option<Flags>,
    pub cars: crate::gui_port_hud3::Cars,
}

pub fn create(hud: &mut HudPort, gui: &mut Gui, painter: &mut dyn Painter, game_folder: &std::path::Path, screen: Id) -> Result<(), String> {
    let messages = IniReader::load(&game_folder.join("system/cfg/messages.ini")).ok().filter(|ini| ini.ready);
    let wrong = WrongWayIndicator::new(painter, false);
    let lap = LapInvalidator::new(painter);
    let drive = DriveThroughPenalty::new(painter);
    let lock = LockControlsTime::new(painter, messages.as_ref());
    let flags = FlagDisplay::new(gui, painter, screen, hud.input.triple_screen);
    let fuel = FuelLight::new(painter, 0.0);
    hud.more.flags = Some(Flags { wrong, lap, drive, lock, flags, fuel, black_flagged: false });
    crate::gui_port_hud3::create(hud, gui, painter, game_folder, screen)
}

pub fn call(hud: &mut HudPort, gui: &mut Gui, painter: &mut dyn Painter, what: &str, number: f32, text: &str) -> Result<(), String> {
    let Some(f) = &mut hud.more.flags else { return Err("the HUD was not made".into()) };
    match what {
        "setFlag" => f.flags.set_flag(number as i32),
        "clearFlag" => f.flags.clear_flag(),
        "setBlueFlag" => f.flags.set_blue_flag(),
        "flagTimer" => f.flags.timer = number,
        "blackFlag" => {
            f.flags.on_black_flag(gui, number != 0.0, text);
            f.black_flagged = number != 0.0;
        }
        "wrongWay" => f.wrong.is_wrong_way = number != 0.0,
        "fuelLimit" => f.fuel.fuel_limit = number,
        "replayActive" => {
            f.wrong.is_active = number != 0.0;
            f.fuel.is_active = number != 0.0;
        }
        other => return crate::gui_port_hud3::call(hud, gui, painter, other, number, text),
    }
    for e in std::mem::take(&mut f.flags.events) {
        f.fuel.on_flag(&e);
    }
    Ok(())
}

pub fn update(hud: &mut HudPort, gui: &mut Gui, dt: f32) {
    crate::gui_port_hud3::update(hud, gui, dt);
    let Some(f) = &mut hud.more.flags else { return };
    f.flags.update(gui, dt, &hud.input, &mut |_| {});
    for e in std::mem::take(&mut f.flags.events) {
        f.fuel.on_flag(&e);
    }
}

pub fn render_hud(hud: &mut HudPort, gui: &mut Gui, painter: &mut dyn Painter, dt: f32) {
    {
        let (Some(f), Some(message)) = (&mut hud.more.flags, &mut hud.message) else { return };
        if f.wrong.is_active {
            f.wrong.render_hud(gui, painter, &hud.input);
        }
        f.lap.render_hud(gui, painter, dt, &hud.input, message);
        f.drive.render_hud(gui, painter, &hud.input, f.black_flagged);
        f.lock.render_hud(gui, painter, &hud.input, message);
    }
    crate::gui_port_hud3::render_hud(hud, gui, painter, dt);
    let Some(f) = &mut hud.more.flags else { return };
    if f.fuel.is_active {
        f.fuel.render_hud(gui, painter, &hud.input);
    }
}

pub fn dump(hud: &HudPort, gui: &Gui, out: &mut String) {
    let Some(f) = &hud.more.flags else { return };
    out.push_str(&crate::gui_seq::flags_line(f.flags.current_flag, f.flags.timer, f.lap.invalid_lap_message_time, f.lap.last_current_lap_valid, f.fuel.x_pos, f.drive.text_pos));
    crate::gui_port_hud3::dump(hud, gui, out);
}
