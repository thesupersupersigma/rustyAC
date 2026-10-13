// SPDX-License-Identifier: GPL-3.0-or-later

//! The port side of the GUI oracle's pit stop: Task 28b's `QuickMenu` with the labels of
//! `rustyac_gui::pit_menu`, and the panel of a stop that is being worked on.

use rustyac_gui::control::Id;
use rustyac_gui::gui::Gui;
use rustyac_gui::painter::Painter;
use rustyac_gui::pit_menu::{PitMenu, PitMenuCar, PitMenuLooks, PitMenuScene, PitStopHud};
use rustyac_physics::data::ini::IniReader;
use rustyac_physics::race::pitstop::{QuickMenu, QuickMenuCar, QuickMenuSettings};

use crate::gui_port_hud::HudPort;
use crate::gui_seq_menu::script;

#[derive(Default)]
pub struct Pit {
    pub menu: Option<(QuickMenu, PitMenu)>,
    pub stop: Option<PitStopHud>,
    pub state: i32,
    pub screen: Id,
}

fn quick_car() -> QuickMenuCar {
    let s = script().pit;
    QuickMenuCar { max_fuel: s.max_fuel, compounds: s.compounds.len(), static_pressure: s.pressure, wing_angle: s.wing.to_vec() }
}

fn pit_car(hud: &HudPort) -> PitMenuCar {
    let s = script().pit;
    PitMenuCar {
        has_race_engineer: true,
        compounds: s.compounds.clone(),
        wing_angle: s.wing.to_vec(),
        fuel_per_lap: s.fuel_per_lap,
        lap_count: hud.input.cars.first().map_or(0, |car| car.lap_count as u32),
        normalized_spline_position: s.spline,
        session_type: hud.input.session_type,
        is_timed_race: s.timed,
        session_total_time: s.total_time,
    }
}

fn scene(hud: &HudPort, gui: &Gui) -> PitMenuScene {
    let i = &hud.input;
    PitMenuScene {
        esc_menu_visible: i.esc_menu_visible,
        pause_menu_visible: i.pause_menu_visible,
        in_replay: i.in_replay,
        end_session_active: i.end_session_active,
        camera_mode: i.camera_mode,
        app_interaction_enabled: gui.game_screen_data(hud.more.pit.screen).app_interaction_enabled,
        focused_car_index: i.focused_car_index,
        in_pitlane: script().pit.in_pitlane,
    }
}

pub fn call(hud: &mut HudPort, gui: &mut Gui, painter: &mut dyn Painter, root: &std::path::Path, screen: Id, what: &str, number: f32, text: &str) -> Result<(), String> {
    hud.more.pit.screen = screen;
    match what {
        "menu.pit" => {
            let ini = IniReader::load(&root.join("system/cfg/pitstop.ini")).ok();
            let settings = ini.as_ref().map_or_else(QuickMenuSettings::default, QuickMenuSettings::from_ini);
            let looks = PitMenuLooks::from_ini(ini.as_ref());
            let setup = IniReader::from_text(std::path::Path::new("task30_setup.ini"), if text == "small" { crate::gui_seq_menu::PIT_SETUP_INI_SMALL } else { crate::gui_seq_menu::PIT_SETUP_INI });
            let menu = QuickMenu::build(&quick_car(), script().pit.compounds, &setup, &settings);
            let labels = PitMenu::build(gui, painter, looks, &menu, &setup, &pit_car(hud));
            hud.more.pit.menu = Some((menu, labels));
        }
        "menu.pitKey" => {
            let car = pit_car(hud);
            let Some((menu, labels)) = &mut hud.more.pit.menu else { return Err("no pit menu".into()) };
            menu.send_input(number as u32, &quick_car());
            labels.update_ui(gui, menu, &car);
        }
        "menu.pitStop" => {
            hud.more.pit.stop = Some(PitStopHud::new(painter));
            hud.more.pit.state = script().pit.state;
        }
        "menu.pitState" => hud.more.pit.state = script().pit.state,
        other => return Err(format!("the port side knows no pit call {other:?}")),
    }
    Ok(())
}

pub fn render_menu(hud: &mut HudPort, gui: &mut Gui, painter: &mut dyn Painter, dt: f32) {
    if hud.more.pit.menu.is_none() {
        return;
    }
    let (car, scene) = (pit_car(hud), scene(hud, gui));
    if let Some((menu, labels)) = &mut hud.more.pit.menu {
        labels.render_hud(gui, painter, dt, menu, &car, &scene);
    }
}

pub fn render_stop(hud: &mut HudPort, gui: &mut Gui, painter: &mut dyn Painter) {
    if hud.more.pit.stop.is_none() {
        return;
    }
    let scene = scene(hud, gui);
    let (race_over, lock) = (hud.input.race_over, hud.input.cars.first().map_or(0.0, |car| car.lock_controls_time));
    let pit = &mut hud.more.pit;
    if let Some(stop) = &pit.stop {
        stop.render_hud(painter, &scene, race_over, lock, &mut pit.state, &script().pit.times);
    }
}

pub fn dump(hud: &HudPort, gui: &Gui, out: &mut String) {
    let pit = &hud.more.pit;
    if let Some((menu, labels)) = &pit.menu {
        out.push_str(&crate::gui_seq_menu::pit_line(menu.is_visible, menu.timer, menu.current_index, menu.current_preset, menu.fuel_index, &menu.presets));
        crate::gui_port::dump_control(gui, labels.root, 1, out);
    }
    if pit.stop.is_some() {
        out.push_str(&format!("pitstop state={}\n", pit.state));
    }
}
