// SPDX-License-Identifier: GPL-3.0-or-later

//! The port side of the GUI oracle's pit stop: Task 28b's `QuickMenu` with the labels of
//! `rustyac_gui::pit_menu`, and the panel of a stop that is being worked on.

use rustyac_gui::control::{Id, MouseEvent};
use rustyac_gui::gui::Gui;
use rustyac_gui::painter::Painter;
use rustyac_gui::pit_menu::{MousePitValues, PitMenu, PitMenuCar, PitMenuLooks, PitMenuScene, PitStopHud};
use rustyac_physics::data::ini::IniReader;
use rustyac_physics::race::pitstop::{MousePitstop, QuickMenu, QuickMenuCar, QuickMenuSettings, STATE_CHOOSING};

use crate::gui_port_hud::HudPort;
use crate::gui_seq_menu::script;

#[derive(Default)]
pub struct Pit {
    pub menu: Option<(QuickMenu, PitMenu)>,
    pub stop: Option<PitStopHud>,
    pub mouse: Option<MousePitstop>,
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
            let s = script().pit;
            hud.more.pit.stop = Some(PitStopHud::new(gui, painter, &s.compounds));
            hud.more.pit.mouse = Some(MousePitstop::new(s.compounds, s.max_fuel));
            hud.more.pit.state = s.state;
        }
        // (PitStop::update arriving in the box: the controls reset, the menu shown)
        "menu.pitMouse" => {
            let Some(mouse) = &mut hud.more.pit.mouse else { return Err("no pit stop".into()) };
            mouse.fuel_percentage = number;
            mouse.requested_fuel_percentage = 0.0;
            for selected in &mut mouse.tyre_selected {
                *selected = false;
            }
            if let Some(first) = mouse.tyre_selected.first_mut() {
                *first = true;
            }
            mouse.desired_tyre_compound.clear();
            mouse.repair_body = false;
            mouse.repair_engine = false;
            mouse.repair_suspension = false;
            mouse.parent_visible = true;
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

pub fn render_stop(hud: &mut HudPort, gui: &mut Gui, painter: &mut dyn Painter, dt: f32) {
    if hud.more.pit.stop.is_none() {
        return;
    }
    let scene = scene(hud, gui);
    let (race_over, lock) = (hud.input.race_over, hud.input.cars.first().map_or(0.0, |car| car.lock_controls_time));
    let s = script().pit;
    let fuel = hud.input.cars.first().map_or(0.0, |car| car.fuel);
    let pit = &mut hud.more.pit;
    if let (Some(stop), Some(mouse)) = (&mut pit.stop, &pit.mouse) {
        stop.sync(gui, mouse);
        let values = MousePitValues { fuel, real_max_fuel: mouse.real_max_fuel(), estimated_time: s.estimate };
        stop.render_hud(gui, painter, dt, &scene, race_over, lock, &mut pit.state, &s.times, &values);
    }
}

/// `PitStop`'s three handlers of the window's mouse; the clicks change the controls as the
/// game's own handlers do.
pub fn window_mouse(hud: &mut HudPort, gui: &mut Gui, kind: u32, e: &MouseEvent) {
    let pit = &mut hud.more.pit;
    let (Some(stop), Some(mouse)) = (&pit.stop, &mut pit.mouse) else { return };
    if kind > 2 {
        return;
    }
    stop.sync(gui, mouse);
    for click in stop.mouse(gui, pit.state == STATE_CHOOSING, kind, e) {
        mouse.click(click);
    }
}

pub fn dump(hud: &HudPort, gui: &Gui, out: &mut String) {
    let pit = &hud.more.pit;
    if let Some((menu, labels)) = &pit.menu {
        out.push_str(&crate::gui_seq_menu::pit_line(menu.is_visible, menu.timer, menu.current_index, menu.current_preset, menu.fuel_index, &menu.presets));
        crate::gui_port::dump_control(gui, labels.root, 1, out);
    }
    if let (Some(stop), Some(mouse)) = (&pit.stop, &pit.mouse) {
        out.push_str(&format!("pitstop state={} tyre={:?}\n", pit.state, mouse.desired_tyre_compound));
        crate::gui_port::dump_control(gui, stop.parent, 1, out);
    }
}

/// What the pit strategy page's setup editor makes its pit menu of (Task 34): the cfg's looks
/// and the car's `setup.ini`, kept by `menu.setupPit` for the page maker.
static SETUP_PIT: std::sync::Mutex<Option<(PitMenuLooks, IniReader)>> = std::sync::Mutex::new(None);

/// `menu.setupPit`: the setup editor gets the pit menu of the arrow keys (no
/// `USE_MOUSE_PITSTOP`), and with it the page "PITSTOP STRATEGY".
pub fn setup_pit(gui: &mut Gui, root: &std::path::Path) -> Result<(), String> {
    let car = gui.setup_car.as_ref().ok_or("no setup car")?;
    let s = script().pit;
    let quick = QuickMenuCar { max_fuel: car.max_fuel() as f32, compounds: car.tyre_compound_names().len(), static_pressure: s.pressure, wing_angle: s.wing.to_vec() };
    let ini = IniReader::load(&root.join("system/cfg/pitstop.ini")).ok();
    let settings = ini.as_ref().map_or_else(QuickMenuSettings::default, QuickMenuSettings::from_ini);
    let setup = IniReader::load(&car.data_path().join("setup.ini")).unwrap_or_default();
    let menu = QuickMenu::build(&quick, car.tyre_compound_names(), &setup, &settings);
    *SETUP_PIT.lock().unwrap() = Some((PitMenuLooks::from_ini(ini.as_ref()), setup));
    gui.setup.use_mouse_pitstop = false;
    gui.setup.pit = Some(rustyac_gui::setup_car::SetupPit { menu, car: quick, edits: 0 });
    Ok(())
}

/// The setup editor's `new QuickMenuPitstop` and its `buildControls` (never shown here).
pub fn setup_pit_menu(gui: &mut Gui, painter: &mut dyn Painter) -> Id {
    let (looks, setup) = SETUP_PIT.lock().unwrap().clone().unwrap_or_else(|| (PitMenuLooks::from_ini(None), IniReader::default()));
    let menu = gui.setup.pit.as_ref().map(|pit| pit.menu.clone()).unwrap_or_else(|| QuickMenu::build(&quick_car(), Vec::new(), &setup, &QuickMenuSettings::default()));
    let s = script().pit;
    let car = PitMenuCar { has_race_engineer: true, compounds: menu.compounds.clone(), wing_angle: s.wing.to_vec(), ..PitMenuCar::default() };
    PitMenu::build(gui, painter, looks, &menu, &setup, &car).root
}
