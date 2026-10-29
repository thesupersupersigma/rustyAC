// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! `FormHDR` (0x218), the developers' "HDR Settings": YEBIS' auto exposure in a graph (the
//! current value, the filter's min and max), its limits on or off, car 0's onboard and outboard
//! exposure (saved into its car.ini) and the filter's auto exposure target (not saved).
//!
//! Ctor 0x140329e20, render 0x14032af90, handlers 0x14032ac40 (limits), 0x14032adb0 / ... (the
//! exposures), 0x14032ae10 (target), 0x14032aa60 (save). Only with YEBIS' camera: what it reads
//! and changes is [`crate::app_camera_editor::CameraEditorLink`]'s.

use std::any::Any;
use std::cell::Cell;
use std::rc::Rc;

use crate::apps::{App, AppAction};
use crate::control::{Clicked, Id};
use crate::gui::Gui;
use crate::hud::translate;
use crate::painter::Painter;
use crate::widgets::WidgetEvent;
use crate::widgets2::Connected;

pub struct HdrSettings {
    graph: Id,
    label: Id,
    save: Id,
    exp_mult: Id,
    /// `savedTimeStamp`: game time (ms) the "SAVED" text shows until
    saved: Rc<Cell<f64>>,
}

impl Gui {
    /// `FormHDR::FormHDR(GUI*, CameraForwardYebis&, Sim&)`: 400 x 600; `gui.apps.camera_editor`
    /// must be there.
    pub fn new_hdr_app(&mut self, painter: &mut dyn Painter) -> Id {
        let l = self.apps.camera_editor.clone().unwrap_or_default();
        let form = self.new_form(painter, "FORM_HDR", false);
        self.form_set_title(form, &translate("HDR Settings"));
        self.form_data_mut(form).dev_app = true;
        self.form_set_size(form, 400.0, 600.0);
        let graph = self.new_graph(painter, "AutoExposure");
        self.control_add_control(form, graph);
        self.set_size(graph, 350.0, 300.0);
        self.set_position(graph, 25.0, 30.0);
        for color in [[1.0, 1.0, 1.0], [1.0, 1.0, 0.0], [1.0, 0.0, 0.0]] {
            self.graph_add_serie(graph, color);
        }
        self.graph_mut(graph).max_y = 10.0;
        let label = self.new_connected_label(painter, "AutoExposure", Connected::Float(0.0));
        self.set_position(label, 25.0, 350.0);
        self.control_add_control(form, label);
        let limits = self.new_control(painter, "Disable Limits");
        self.set_text(limits, "Disable Limits");
        self.control_add_control(form, limits);
        self.set_position(limits, 250.0, 350.0);
        self.set_size(limits, 100.0, 20.0);
        self.controls[limits].ev_clicked.push(Rc::new(|gui: &mut Gui, ev: &Clicked| {
            let Some(l) = gui.apps.camera_editor.as_mut() else { return };
            l.use_auto_exposure_limits = !l.use_auto_exposure_limits;
            let text = if l.use_auto_exposure_limits { "Disable Limits" } else { "Enable Limits" };
            gui.set_text(ev.control, text);
        }));
        // the two exposures: (name, label, (x, y), car 0's value, its setter)
        type Set = fn(&mut crate::app_camera_editor::CameraEditorLink, f32);
        let exposures: [(&str, &str, f32, f32, Set); 2] = [
            ("spOnBoardExp", "OnBoard EXP", 25.0, l.on_board_exposure, |l, v| l.on_board_exposure = v),
            ("spOutBoardExp", "OutBoard EXP", 225.0, l.out_board_exposure, |l, v| l.out_board_exposure = v),
        ];
        for (name, text, x, value, set) in exposures {
            let sp = self.new_spinner(painter, name, false, false);
            self.control_add_control(form, sp);
            self.spinner_set_position(sp, x, 400.0);
            self.set_size(sp, 150.0, 30.0);
            self.spinner_set_label_text(sp, text);
            self.spinner_mut(sp).min_value = 0;
            self.spinner_mut(sp).max_value = 100;
            self.spinner_set_value(sp, value as i32);
            self.spinner_mut(sp).handlers.push(Rc::new(move |gui: &mut Gui, ev: &WidgetEvent| {
                let WidgetEvent::SpinnerValueChanged { value, .. } = ev else { return };
                if let Some(l) = gui.apps.camera_editor.as_mut() {
                    set(l, *value as f32);
                }
            }));
        }
        let exp_mult = self.new_control(painter, "labExpMult");
        self.control_add_control(form, exp_mult);
        self.set_position(exp_mult, 25.0, 480.0);
        self.set_size(exp_mult, 150.0, 30.0);
        let target = self.new_spinner(painter, "target", false, false);
        self.control_add_control(form, target);
        self.spinner_set_position(target, 225.0, 480.0);
        self.set_size(target, 150.0, 30.0);
        self.spinner_set_label_text(target, "PP Target (not saved)");
        self.spinner_mut(target).min_value = 0;
        self.spinner_mut(target).max_value = 100;
        self.spinner_mut(target).display_value_mult = f32::from_bits(0x3c23_d70a);
        self.spinner_mut(target).handlers.push(Rc::new(|gui: &mut Gui, ev: &WidgetEvent| {
            let WidgetEvent::SpinnerValueChanged { value, .. } = ev else { return };
            if let Some(l) = gui.apps.camera_editor.as_mut() {
                l.auto_exposure_target = *value as f32 * f32::from_bits(0x3c23_d70a);
            }
        }));
        self.spinner_set_value(target, (l.auto_exposure_target * 100.0f32) as i32);
        let save = self.new_control(painter, "Save");
        self.set_text(save, "Save in car.ini");
        self.control_add_control(form, save);
        self.set_size(save, 350.0, 20.0);
        let r = self.controls[save].rect;
        self.set_position(save, 25.0, 600.0 - (r.bottom - r.top));
        if l.data_acd {
            self.set_visible(save, false);
        }
        let saved = Rc::new(Cell::new(0.0f64));
        let s = Rc::clone(&saved);
        self.controls[save].ev_clicked.push(Rc::new(move |gui: &mut Gui, ev: &Clicked| {
            let Some(l) = gui.apps.camera_editor.as_ref() else { return };
            let file = format!("content/cars/{}/data/car.ini", l.unix_name);
            for (key, value) in [("ONBOARD_EXPOSURE", l.on_board_exposure), ("OUTBOARD_EXPOSURE", l.out_board_exposure)] {
                gui.apps.actions.push(AppAction::WriteProfileString { file: file.clone(), section: "GRAPHICS".into(), key: key.into(), value: (value as i32).to_string() });
            }
            gui.set_text(ev.control, "SAVED");
            s.set(gui.apps.game_time + 3000.0);
        }));
        self.form_data_mut(form).autohide = false;
        self.set_app(form, Box::new(HdrSettings { graph, label, save, exp_mult, saved }));
        form
    }
}

impl App for HdrSettings {
    fn as_any(&mut self) -> &mut dyn Any {
        self
    }

    /// `FormHDR::render` 0x14032af90: the current auto exposure, the filter's min and max (even
    /// with the limits off), the button's text back after 3 s, the exposure multiplier.
    fn render(&mut self, gui: &mut Gui, id: Id, dt: f32, painter: &mut dyn Painter) {
        let l = gui.apps.camera_editor.clone().unwrap_or_default();
        gui.connected_label_mut(self.label).value = Connected::Float(l.auto_exposure);
        gui.graph_add_value(self.graph, 0, l.auto_exposure);
        gui.graph_add_value(self.graph, 1, l.auto_exposure_min);
        gui.graph_add_value(self.graph, 2, l.auto_exposure_max);
        if !(self.saved.get() >= gui.apps.game_time) {
            gui.set_text(self.save, "Save in car.ini");
        }
        let m = gui.apps.photo.exposure_multiplier;
        gui.set_text(self.exp_mult, &format!("Exp multiplier: {m:.2}"));
        gui.control_render(id, dt, painter);
    }
}
