// SPDX-License-Identifier: GPL-3.0-or-later

//! The port side of the GUI oracle's HUD: `rustyac_gui::hud`'s objects fed the script's
//! session state.

use rustyac_gui::control::Id;
use rustyac_gui::gui::Gui;
use rustyac_gui::hud::{HudInput, SystemMessage, SystemNotification};
use rustyac_gui::painter::Painter;
use rustyac_physics::data::ini::IniReader;

#[derive(Default)]
pub struct HudPort {
    pub input: HudInput,
    pub message: Option<SystemMessage>,
    pub notification: Option<SystemNotification>,
    pub more: crate::gui_port_hud2::More,
    pub apps: crate::gui_port_apps::AppsPort,
}

pub fn call(hud: &mut HudPort, gui: &mut Gui, painter: &mut dyn Painter, game_folder: &std::path::Path, screen: Id, what: &str, number: f32, text: &str) -> Result<(), String> {
    if what.starts_with("ta.") {
        return crate::gui_port_modes::call(gui, painter, what, number);
    }
    if what.starts_with("dr.") {
        return crate::gui_port_modes::drift_call(painter, what, number);
    }
    if what.starts_with("dg.") {
        return crate::gui_port_modes::drag_call(gui, painter, what, number);
    }
    if what.starts_with("console.") {
        let unix = hud.input.cars.first().map(|c| c.unix_name.clone()).unwrap_or_default();
        return crate::gui_seq_console::port_call(gui, painter, game_folder, &unix, what, number, text);
    }
    if what.starts_with("py.") {
        return crate::gui_py::port_call(gui, painter, game_folder, screen, what);
    }
    if what.starts_with("tele.") {
        crate::gui_seq_tele::input(what, number, text)?;
        let player = hud.input.cars.first().map(|c| c.driver_name.clone()).unwrap_or_default();
        crate::gui_seq_tele::port_sync(gui, &player);
        return Ok(());
    }
    if what.starts_with("app.") {
        if crate::gui_seq_apps::app_input(what, number, text)? {
            crate::gui_port_apps::sync(gui);
            for (car, time) in crate::gui_seq_apps::take_laps() {
                gui.apps_on_lap_completed(car, time);
            }
            return Ok(());
        }
        return crate::gui_port_apps::call(&mut hud.apps, gui, painter, game_folder, screen, what, number, text);
    }
    if what.starts_with("menu.") || what.starts_with("pit.") || what.starts_with("race.") {
        if crate::gui_seq_menu::menu_input(what, number, text) {
            return Ok(());
        }
        if what.starts_with("menu.pit") {
            crate::gui_port_menu::sync(hud, gui);
    crate::gui_port_modes::sync(gui);
    crate::gui_port_modes::drift_sync(gui);
    crate::gui_port_modes::drag_sync(gui);
            return crate::gui_port_pit::call(hud, gui, painter, game_folder, screen, what, number, text);
        }
        return crate::gui_port_menu::call(hud, gui, painter, game_folder, screen, what, number, text);
    }
    if crate::gui_seq::hud_input(&mut hud.input, what, number, text) {
        return Ok(());
    }
    if what == "create" {
        hud.input.cars.resize(number as usize, crate::gui_seq::default_car());
        let read = |name: &str| IniReader::load(&game_folder.join("system/cfg").join(name)).ok().filter(|ini| ini.ready);
        let (messages, session_info) = (read("messages.ini"), read("session_info.ini"));
        let mut message = SystemMessage::new(gui, painter, messages.as_ref(), session_info.as_ref());
        hud.notification = Some(SystemNotification::new(gui, painter, screen, None));
        message.load_drivers_info(painter, &hud.input.cars, None);
        hud.message = Some(message);
        return crate::gui_port_hud2::create(hud, gui, painter, game_folder, screen);
    }
    let (Some(message), Some(notification)) = (&mut hud.message, &mut hud.notification) else { return Err("the HUD was not made".into()) };
    match what {
        "message" => {
            let (title, description) = text.split_once('|').unwrap_or((text, ""));
            message.add_message(gui, title, description, number as i32);
        }
        "carMessage" => message.add_car_message(number as u32, text.parse().unwrap_or(0)),
        "clear" => message.clear_message(),
        "notify" => notification.show_notification(gui, number as i32),
        other => return crate::gui_port_hud2::call(hud, gui, painter, other, number, text),
    }
    Ok(())
}

pub fn update(hud: &mut HudPort, gui: &mut Gui, dt: f32) {
    crate::gui_port_menu::sync(hud, gui);
    let player = hud.input.cars.first().map(|c| c.driver_name.clone()).unwrap_or_default();
    crate::gui_seq_tele::port_sync(gui, &player);
    // Task 33: MicroSectors::update, once a frame before the GUI; Sim::update's statistics
    if gui.micro_sectors.enabled {
        gui.micro_sectors_update();
    }
    gui.stats_sim_update(crate::gui_seq_apps::DT);
    crate::gui_port_apps::sync(gui);
    let (Some(message), Some(notification)) = (&mut hud.message, &mut hud.notification) else { return };
    message.update(gui, dt, &hud.input);
    notification.update(gui, dt, &hud.input);
    crate::gui_port_hud2::update(hud, gui, dt);
    crate::gui_port_menu::update(hud, gui);
}

pub fn render_hud(hud: &mut HudPort, gui: &mut Gui, painter: &mut dyn Painter, dt: f32) {
    let Some(message) = &mut hud.message else { return };
    message.render_hud(gui, painter, dt, &hud.input);
    crate::gui_port_modes::render_hud(hud, gui, painter, dt);
    crate::gui_port_modes::drift_render_hud(hud, gui, painter, dt);
    crate::gui_port_modes::drag_render_hud(gui, painter);
    crate::gui_port_pit::render_menu(hud, gui, painter, dt);
    crate::gui_port_hud2::render_hud(hud, gui, painter, dt);
    // Task 33: the console, the last of the HUD objects
    crate::gui_seq_console::port_render(gui, painter);
}

pub fn dump(hud: &HudPort, gui: &Gui, out: &mut String) {
    let (Some(m), Some(n)) = (&hud.message, &hud.notification) else { return };
    out.push_str(&crate::gui_seq::message_line(m.current_status, m.timer, &m.session.text_to_render, m.session.last_current_lap_valid, m.session.invalid_lap_message_time, m.driver.current_car_index));
    crate::gui_port::dump_control(gui, m.default_system_message, 1, out);
    crate::gui_port::dump_control(gui, m.driver.control, 1, out);
    out.push_str(&crate::gui_seq::notification_line(n.current_status, n.timer, n.blink_time, n.show));
    crate::gui_port_hud2::dump(hud, gui, out);
    crate::gui_port_modes::dump(out);
    crate::gui_port_modes::drift_dump(out);
    crate::gui_port_modes::drag_dump(out);
    crate::gui_port_pit::dump(hud, gui, out);
    crate::gui_port_menu::dump(hud, gui, out);
    crate::gui_seq_menu::dump_actions(out);
    crate::gui_port_setup::items(out);
}
