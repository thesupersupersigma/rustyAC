// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! `FormDRSZones` (0x210), the developers' "DRS Zones": one button marks, where the player's
//! car is along the lap, a zone's detection point, then its start, then its end; "Save" writes
//! the zones as the track's `data/drs_zones.ini.candidate`.
//!
//! `FormDRSZones::FormDRSZones(CarAvatar*)` 0x140336210, `render` 0x140336fd0, the button's
//! lambda 0x140336c80, the save lambda 0x1403367f0.

use std::any::Any;
use std::cell::RefCell;
use std::rc::Rc;

use crate::acos::format_g6;
use crate::apps::{App, AppAction};
use crate::control::{Clicked, Id};
use crate::gui::Gui;
use crate::hud::translate;
use crate::painter::Painter;
use crate::widgets2::{format_fixed, Connected};

/// `DRSZone`: lap fractions.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct DrsZone {
    pub detection: f32,
    pub start: f32,
    pub end: f32,
}

/// The form's zones (shared with its buttons: an app is out of its form while a click runs).
#[derive(Default)]
pub struct DrsZonesState {
    pub current_zone: DrsZone,
    pub zones: Vec<DrsZone>,
    /// `DRSRecordingStatus`: 0 detection, 1 start, 2 end. The constructor leaves 2 under a
    /// "Set Detection" button: the first press ends a zone that begins at 0.
    pub recording_status: i32,
}

pub struct DrsZonesApp {
    pub lbl_nsp: Id,
    pub lbl_zones: Id,
    pub state: Rc<RefCell<DrsZonesState>>,
}

impl Gui {
    /// `FormDRSZones::FormDRSZones` 0x140336210: 400 x 400.
    pub fn new_drs_zones_app(&mut self, painter: &mut dyn Painter) -> Id {
        let form = self.new_form(painter, "DRS_ZONES", false);
        self.form_set_title(form, &translate("DRS Zones"));
        let nsp = self.new_connected_label(painter, "NSP", Connected::Float(0.0));
        self.set_position(nsp, 5.0, 40.0);
        self.set_size(nsp, 100.0, 30.0);
        self.controls[nsp].font_scale = 2.0;
        self.control_add_control(form, nsp);
        let button = self.new_control(painter, "START_STOP");
        self.set_position(button, 5.0, 100.0);
        self.set_size(button, 180.0, 30.0);
        self.set_text(button, "Set Detection");
        self.control_add_control(form, button);
        let state = Rc::new(RefCell::new(DrsZonesState { recording_status: 2, ..DrsZonesState::default() }));
        let shared = Rc::clone(&state);
        self.controls[button].ev_clicked.push(Rc::new(move |gui: &mut Gui, _: &Clicked| {
            let npos = gui.apps.cars.first().map_or(0.0, |c| c.state.normalized_spline_position);
            let a = &mut *shared.borrow_mut();
            let (text, next) = match a.recording_status {
                0 => {
                    a.current_zone.detection = npos;
                    ("Set Start", 1)
                }
                1 => {
                    a.current_zone.start = npos;
                    ("Set End", 2)
                }
                2 => {
                    a.current_zone.end = npos;
                    a.zones.push(a.current_zone);
                    ("Set Detection", 0)
                }
                _ => return,
            };
            a.recording_status = next;
            gui.set_text(button, text);
        }));
        let zones = self.new_label(painter, "ZONES");
        self.set_position(zones, 200.0, 140.0);
        self.set_size(zones, 100.0, 300.0);
        self.controls[zones].font_scale = 1.5;
        self.control_add_control(form, zones);
        let save = self.new_control(painter, "SAVE");
        self.set_position(save, 5.0, 350.0);
        self.set_size(save, 100.0, 30.0);
        self.set_text(save, "Save");
        self.control_add_control(form, save);
        let shared = Rc::clone(&state);
        self.controls[save].ev_clicked.push(Rc::new(move |gui: &mut Gui, _: &Clicked| {
            let zones = shared.borrow().zones.clone();
            let action = if zones.is_empty() {
                AppAction::AddMessage { title: "Nothing to save".into(), text: "DRS Zones".into() }
            } else {
                // TrackAvatar::getDataFolder: content/tracks/<track>[/<layout>]
                let mut folder = format!("content/tracks/{}", gui.apps.track_name);
                if !gui.apps.track_config.is_empty() {
                    folder = format!("{folder}/{}", gui.apps.track_config);
                }
                let path = format!("{folder}/data/drs_zones.ini.candidate");
                gui.apps.actions.push(AppAction::WriteFile { path: path.clone(), bytes: save_text(&zones).into_bytes() });
                AppAction::AddMessage { title: "DRS Zones saved".into(), text: path }
            };
            gui.apps.actions.push(action);
        }));
        self.form_set_size(form, 400.0, 400.0);
        self.set_app(form, Box::new(DrsZonesApp { lbl_nsp: nsp, lbl_zones: zones, state }));
        form
    }
}

/// The save lambda's `std::wofstream` (text mode: CR LF; floats `%g` of six digits).
pub fn save_text(zones: &[DrsZone]) -> String {
    zones.iter().enumerate().map(|(i, z)| format!("[ZONE_{i}]\r\nDETECTION={}\r\nSTART={}\r\nEND={}\r\n\r\n", format_g6(z.detection), format_g6(z.start), format_g6(z.end))).collect()
}

impl App for DrsZonesApp {
    fn as_any(&mut self) -> &mut dyn Any {
        self
    }

    /// `FormDRSZones::render` 0x140336fd0: the zones' list (fixed, three decimals), then the form.
    fn render(&mut self, gui: &mut Gui, id: Id, dt: f32, painter: &mut dyn Painter) {
        let f = |v: f32| format_fixed(v as f64, 3);
        let text: String = self.state.borrow().zones.iter().enumerate().map(|(i, z)| format!("{i}: ({} , {} , {})\n", f(z.detection), f(z.start), f(z.end))).collect();
        gui.set_text(self.lbl_zones, &text);
        let npos = gui.apps.cars.first().map_or(0.0, |c| c.state.normalized_spline_position);
        gui.connected_label_mut(self.lbl_nsp).value = Connected::Float(npos);
        gui.control_render(id, dt, painter);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_text_is_the_games_ini() {
        let zones = [DrsZone { detection: 0.0, start: 0.0, end: 0.25 }, DrsZone { detection: 0.912_345_7, start: 0.95, end: 0.0001 }];
        assert_eq!(save_text(&zones), "[ZONE_0]\r\nDETECTION=0\r\nSTART=0\r\nEND=0.25\r\n\r\n[ZONE_1]\r\nDETECTION=0.912346\r\nSTART=0.95\r\nEND=0.0001\r\n\r\n");
    }
}
