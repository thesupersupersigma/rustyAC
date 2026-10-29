// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! `FormDriverModel` (0x220), the developers' "Driver Model Utilities": a G force pushed on the
//! 3D driver's head (its direction per axis), the head movement's settings of car 0's driver
//! (`[HEAD_MOVEMENT]` of driver3d.ini) and their save.
//!
//! Ctor 0x14031f0e0, render 0x140321320, updateForces 0x1403218f0, the save's lambda 0x140320a70.
//! What it reads and changes of the driver is [`DriverModelLink`]'s: the game's
//! `CarAvatar::driverModel_HR` fields and its `updateHeadMovement` calls.

use std::any::Any;
use std::cell::RefCell;
use std::rc::Rc;

use crate::app_volumes::to_wstring_f;
use crate::apps::{App, AppAction};
use crate::control::{Clicked, Id};
use crate::font::{Font, FontType};
use crate::gui::Gui;
use crate::hud::translate;
use crate::painter::Painter;
use crate::widgets::WidgetEvent;

/// Car 0's driver as the window sees it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DriverModelLink {
    /// `CarAvatar::unixName` and `configName` (the save's folder: `data` or `data_<config>`)
    pub unix_name: String,
    pub config_name: String,
    /// the loose save file exists (else the save button is hidden)
    pub save_exists: bool,
    /// `data/driver3d.ini [HEAD_MOVEMENT]` FILTER, MAX_G, ROLL_MAX_DEG, PITCH_MAX_DEG,
    /// YAW_MAX_DEG (None: no such section)
    pub ini: Option<[f32; 5]>,
    /// what the window wrote into `DriverModel::headMovement`: maxG, rollMaxDeg, pitchMaxDeg,
    /// yawMaxDeg, filter (+0x64..+0x74)
    pub head: [Option<f32>; 5],
    /// `DriverModel::lockAnimation`, set by the window
    pub lock_animation: bool,
    /// `FormDriverModel::accG`
    pub acc_g: [f32; 3],
    /// this frame's `updateHeadMovement(dt, dt, accG)` calls (the render's dt, accG then)
    pub head_updates: Vec<(f32, [f32; 3])>,
}

#[derive(Clone, Copy, Default)]
struct State {
    g_force: Id,
    g_direction: [Id; 3],
    max_g: Id,
    roll: Id,
    pitch: Id,
    yaw: Id,
    filter: Id,
    save: Id,
    loaded: bool,
    start_me: bool,
}

pub struct DriverModelForm {
    state: Rc<RefCell<State>>,
}

/// `updateForces` 0x1403218f0.
fn update_forces(gui: &mut Gui, s: &State) {
    let value = |gui: &Gui, id: Id| gui.spinner(id).value;
    let g = value(gui, s.g_force) as f32 * 0.1f32;
    let d = s.g_direction.map(|id| (value(gui, id) - 1) as f32);
    if let Some(l) = gui.apps.driver_model.as_mut() {
        l.lock_animation = true;
        l.acc_g = [g * d[0], g * d[1], g * d[2]];
    }
}

impl Gui {
    /// `FormDriverModel::FormDriverModel(CarAvatar*)`: 415 x 350, then scaled;
    /// `gui.apps.driver_model` must be there.
    pub fn new_driver_model_app(&mut self, painter: &mut dyn Painter) -> Id {
        let l = self.apps.driver_model.clone().unwrap_or_default();
        let form = self.new_form(painter, "FORM_DRIVER_MODEL", true);
        self.form_set_title(form, &translate("Driver Model Utilities"));
        self.form_data_mut(form).dev_app = true;
        self.set_auto_hide_mode(form, true);
        self.fonts.push(Font::new(painter, "", FontType::Monospaced, 14.0, false, false));
        let font = self.fonts.len() - 1;
        let state = Rc::new(RefCell::new(State::default()));
        let dress = |gui: &mut Gui, c: Id| {
            gui.controls[c].font_scale = gui.controls[form].font_scale;
            gui.controls[c].font = Some(font);
            gui.controls[c].font_align = 2;
        };
        let title = self.new_label(painter, "Head_movement_title");
        dress(self, title);
        self.set_text(title, "Head Movement");
        self.controls[title].fore_color = [1.0; 4];
        self.set_size(title, 200.0, 20.0);
        self.set_position(title, 5.0, 30.0);
        self.control_add_control(form, title);
        let floats = |n: i32, step: u32| (0..n).map(|i| to_wstring_f(i as f32 * f32::from_bits(step))).collect::<Vec<_>>();
        let ints = || (0..=100).map(|i| (i - 50).to_string()).collect::<Vec<_>>();
        let directions = || ["Negative", "No Force", "Positive"].map(String::from).to_vec();
        // (name, position, items, setValue(1)), in the game's order
        let rows: [(&str, f32, f32, Vec<String>, bool); 9] = [
            ("G Force", 5.0, 80.0, floats(31, 0x3dcc_cccd), false),
            ("G Direction X", 5.0, 130.0, directions(), true),
            ("G Direction Y", 5.0, 180.0, directions(), true),
            ("G Direction Z", 5.0, 230.0, directions(), true),
            ("Max G Force", 210.0, 80.0, floats(31, 0x3dcc_cccd), false),
            ("Roll Max Deg", 210.0, 130.0, ints(), false),
            ("Yaw Max Deg", 210.0, 180.0, ints(), false),
            ("Pitch Max Deg", 210.0, 230.0, ints(), false),
            ("Filter", 210.0, 280.0, floats(300, 0x3c23_d70a), false),
        ];
        let mut ids = Vec::new();
        for (k, (name, x, y, items, one)) in rows.into_iter().enumerate() {
            let sp = self.new_spinner(painter, name, true, false);
            dress(self, sp);
            self.controls[sp].fore_color = [1.0; 4];
            self.spinner_set_size(sp, 200.0, 20.0);
            self.spinner_set_position(sp, x, y);
            for item in &items {
                self.spinner_add_item(sp, item);
            }
            if one {
                self.spinner_set_value(sp, 1);
            }
            // the handlers: the G spinners only updateForces; the others, once loaded, the
            // driver's head movement (maxG, roll, yaw, pitch, filter) first
            let s = Rc::clone(&state);
            self.spinner_mut(sp).handlers.push(Rc::new(move |gui: &mut Gui, ev: &WidgetEvent| {
                let WidgetEvent::SpinnerValueChanged { value, .. } = ev else { return };
                let st = s.borrow();
                let v = *value as f32;
                let field = match k {
                    4 => Some((0, v * 0.1f32)),
                    5 => Some((1, v - 50.0)),
                    6 => Some((3, v - 50.0)),
                    7 => Some((2, v - 50.0)),
                    8 => Some((4, v * f32::from_bits(0x392e_c33e))),
                    _ => None,
                };
                if let (true, Some((i, v)), Some(l)) = (st.loaded, field, gui.apps.driver_model.as_mut()) {
                    l.head[i] = Some(v);
                }
                update_forces(gui, &st);
            }));
            self.control_add_control(form, sp);
            ids.push(sp);
        }
        let save = self.new_control(painter, "SaveIni");
        self.controls[save].font_scale = f32::from_bits(0x3f33_3333);
        self.set_text(save, "Start");
        self.set_size(save, 200.0, 20.0);
        self.set_repeat_interval(save, f32::from_bits(0x3d4c_cccd));
        self.set_position(save, 5.0, 280.0);
        self.controls[save].back_color = [1.0, 0.0, 0.0, 1.0];
        self.control_add_control(form, save);
        {
            let mut s = state.borrow_mut();
            s.g_force = ids[0];
            s.g_direction = [ids[1], ids[2], ids[3]];
            (s.max_g, s.roll, s.yaw, s.pitch, s.filter) = (ids[4], ids[5], ids[6], ids[7], ids[8]);
            s.save = save;
        }
        let folder = if l.config_name.is_empty() { "data".to_string() } else { format!("data_{}", l.config_name) };
        let file = format!("content/cars/{}/{folder}/driver3d.ini", l.unix_name);
        if !l.save_exists {
            self.set_visible(save, false);
        } else {
            let s = Rc::clone(&state);
            self.controls[save].ev_clicked.push(Rc::new(move |gui: &mut Gui, _: &Clicked| {
                let mut st = s.borrow_mut();
                if !st.start_me {
                    st.start_me = true;
                    return;
                }
                let v = |id: Id| gui.spinner(id).value;
                let keys = [
                    ("FILTER", to_wstring_f(v(st.filter) as f32 * f32::from_bits(0x3c23_d70a))),
                    ("MAX_G", to_wstring_f(v(st.max_g) as f32 * 0.1f32)),
                    ("ROLL_MAX_DEG", (v(st.roll) - 50).to_string()),
                    ("PITCH_MAX_DEG", (v(st.pitch) - 50).to_string()),
                    ("YAW_MAX_DEG", (v(st.yaw) - 50).to_string()),
                ];
                for (key, value) in keys {
                    gui.apps.actions.push(AppAction::WriteProfileString { file: file.clone(), section: "HEAD_MOVEMENT".into(), key: key.into(), value });
                }
            }));
        }
        self.form_set_size(form, 415.0, 350.0);
        let scale = self.controls[form].scale_mult;
        self.form_scale_by_mult(form, scale);
        self.set_app(form, Box::new(DriverModelForm { state }));
        form
    }
}

impl App for DriverModelForm {
    fn as_any(&mut self) -> &mut dyn Any {
        self
    }

    /// `FormDriverModel::render` 0x140321320: after the first Start click, once, the ini's
    /// head movement into the spinners (their handlers fire, not yet loaded); every frame
    /// `updateHeadMovement(dt, dt, accG)`.
    fn render(&mut self, gui: &mut Gui, id: Id, dt: f32, painter: &mut dyn Painter) {
        let (loaded, start_me) = {
            let s = self.state.borrow();
            (s.loaded, s.start_me)
        };
        if !loaded && start_me {
            let s = *self.state.borrow();
            gui.set_text(s.save, "Save Ini");
            match gui.apps.driver_model.as_ref().and_then(|l| l.ini) {
                Some([filter, max_g, roll, pitch, yaw]) => {
                    gui.spinner_set_value(s.filter, (filter * 100.0f32) as i32);
                    gui.spinner_set_value(s.max_g, (max_g * 10.0f32) as i32);
                    gui.spinner_set_value(s.roll, (roll + 50.0f32) as i32);
                    gui.spinner_set_value(s.pitch, (pitch + 50.0f32) as i32);
                    gui.spinner_set_value(s.yaw, (yaw + 50.0f32) as i32);
                }
                None => gui.spinner_set_value(s.filter, 15),
            }
            self.state.borrow_mut().loaded = true;
        }
        if let Some(l) = gui.apps.driver_model.as_mut() {
            l.head_updates.push((dt, l.acc_g));
        }
        gui.control_render(id, dt, painter);
    }
}
