// SPDX-License-Identifier: GPL-3.0-or-later

//! The port side of the GUI oracle's HUD, part three: the objects that follow the cars.

use rustyac_gui::control::Id;
use rustyac_gui::gui::Gui;
use rustyac_gui::hud_cars::{DamageDisplayer, DriverNames, ProximityIndicator};
use rustyac_gui::painter::Painter;
use rustyac_physics::data::ini::IniReader;
use rustyac_physics::vecmath::Mat44f;

use crate::gui_port_hud::HudPort;

#[derive(Default)]
pub struct Cars {
    pub proximity: Option<ProximityIndicator>,
    pub names: Option<DriverNames>,
    pub damage: Option<DamageDisplayer>,
}

pub fn create(hud: &mut HudPort, gui: &mut Gui, painter: &mut dyn Painter, root: &std::path::Path, _screen: Id) -> Result<(), String> {
    let read = |name: &str| IniReader::load(&root.join("system/cfg").join(name)).ok().filter(|ini| ini.ready);
    if hud.input.camera_matrix.m[3][3] == 0.0 {
        hud.input.camera_matrix = Mat44f::IDENTITY;
    }
    let c = &mut hud.more.cars;
    c.proximity = Some(ProximityIndicator::new(read("proximity_indicator.ini").as_ref()));
    c.names = Some(DriverNames::new(gui, painter, read("name_displayer.ini").as_ref(), None, hud.input.cars.len()));
    c.damage = Some(DamageDisplayer::new(painter, read("damage_displayer.ini").as_ref(), None, hud.input.focused_car_index));
    Ok(())
}

pub fn call(hud: &mut HudPort, _gui: &mut Gui, _painter: &mut dyn Painter, what: &str, number: f32, _text: &str) -> Result<(), String> {
    let c = &mut hud.more.cars;
    let (Some(proximity), Some(names), Some(damage)) = (&mut c.proximity, &mut c.names, &mut c.damage) else { return Err("the HUD was not made".into()) };
    match what {
        "namesActive" => names.is_active = number != 0.0,
        "proximityActive" => proximity.is_active = number != 0.0,
        "damageEnabled" => damage.enabled = number != 0.0,
        "damageShow" => damage.show(),
        other => return Err(format!("the port side knows no HUD call {other:?}")),
    }
    Ok(())
}

pub fn update(hud: &mut HudPort, _gui: &mut Gui, _dt: f32) {
    let Some(names) = &mut hud.more.cars.names else { return };
    let (view, perspective) = crate::gui_seq::hud_view(&hud.input.camera_matrix);
    names.update(&hud.input.camera_matrix, &view, &perspective);
}

pub fn render_hud(hud: &mut HudPort, gui: &mut Gui, painter: &mut dyn Painter, dt: f32) {
    let c = &mut hud.more.cars;
    let (Some(proximity), Some(names), Some(damage)) = (&mut c.proximity, &mut c.names, &mut c.damage) else { return };
    if proximity.is_active {
        proximity.render_hud(gui, painter, &hud.input);
    }
    // (the name tags' own switch is tested inside, as the game's `isActive` is outside)
    names.render_hud(gui, painter, dt, &hud.input);
    damage.render_hud(gui, painter, dt, &hud.input);
}

pub fn dump(hud: &HudPort, gui: &Gui, out: &mut String) {
    let c = &hud.more.cars;
    let (Some(names), Some(damage)) = (&c.names, &c.damage) else { return };
    for label in &names.car_displayers {
        crate::gui_port::dump_control(gui, label.control, 1, out);
    }
    let items: Vec<(f32, f32)> = damage.items.iter().map(|i| (i.health, i.blink_time)).collect();
    out.push_str(&crate::gui_seq::damage_line(damage.render_timer, damage.initialized, damage.enabled, &items));
}
