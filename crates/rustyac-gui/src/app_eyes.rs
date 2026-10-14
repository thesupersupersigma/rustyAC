// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! `DriverEyesPositioner` (0x290 bytes), the app "Onboard settings" of the player's car
//! (`Sim::cars[0]`): the driver's eye point (`CarAvatar::driverEyesPosition`, in the 3D model's
//! frame) moved by four arrows and the seat spinner, the cockpit view's field of view
//! (`CameraOnBoard::onBoardFov`) and pitch (`customCameraSettings[0].pitch`), and their saving
//! into Documents `cfg/cars/<car>/view.ini` and `cfg/camera_onboard.ini` (or, for a car whose
//! car.ini is a loose file, into its car.ini / dash_cam.ini).
//!
//! `DriverEyesPositioner::DriverEyesPositioner` 0x14030fa60, `render` 0x140312a70,
//! `saveSettings` 0x140313920, `saveToINI` 0x1403144e0, `saveToDashINI` 0x140314120; the
//! handlers of section 2.4 of the brief (0x140312000 … 0x140311930).
//!
//! The eye point, the field of view and the pitch are plain members of the game's car and
//! camera, which the app writes; here the app writes them in [`crate::apps::AppLink`] and asks
//! for them with [`AppAction`]s, which whoever runs the GUI carries out on its camera.

use std::any::Any;
use std::cell::Cell;
use std::rc::Rc;

use crate::apps::{App, AppAction};
use crate::control::{Clicked, Id};
use crate::font::{Font, FontType};
use crate::gui::Gui;
use crate::hud::translate;
use crate::painter::{Painter, Rect, ALIGN_CENTER, ALIGN_LEFT, GRAPHICS_GL};

/// What the constructor reads: car.ini `[GRAPHICS] DRIVEREYES` and `ON_BOARD_PITCH_ANGLE`
/// (degrees; none: car.ini is not there), game-folder `cfg/camera_onboard.ini [MODE] FOV`.
#[derive(Clone, Copy, Debug, Default)]
pub struct EyesDefaults {
    pub eyes: Option<[f32; 3]>,
    pub pitch_degrees: Option<f32>,
    pub fov: Option<f32>,
}

/// The members the handlers share.
pub struct EyesState {
    pub increment: Cell<f32>,
    pub speed: f32,
    pub default_fov: f32,
    pub default_pitch: f32,
    pub default_eyes: [f32; 3],
    pub current_fov: Cell<f32>,
    pub need_to_save: Cell<bool>,
}

pub struct Eyes {
    pub regs: [Id; 3],
    pub arrows: [Id; 4],
    pub state: Rc<EyesState>,
    /// the form's font (`Control::font`): the constructor's Consolas 20 until the first render
    pub font: usize,
}

/// `ostream << float`: MSVC 12's `%g` (six significant digits, three exponent digits).
pub fn format_g(v: f32) -> String {
    let s = crate::app_track_map::format_g6(v as f64);
    match s.split_once('e') {
        Some((m, e)) => {
            let (sign, digits) = e.split_at(1);
            format!("{m}e{sign}{:0>3}", digits)
        }
        None => s,
    }
}

fn eyes(gui: &Gui) -> [f32; 3] {
    gui.apps.driver_eyes
}

fn set_eyes(gui: &mut Gui, e: [f32; 3]) {
    gui.apps.driver_eyes = e;
    gui.apps.actions.push(AppAction::SetDriverEyes(e));
}

fn set_fov(gui: &mut Gui, f: f32) {
    gui.apps.onboard_fov = f;
    gui.apps.actions.push(AppAction::SetOnboardFov(f));
}

fn set_pitch(gui: &mut Gui, p: f32) {
    gui.apps.onboard_pitch = p;
    gui.apps.actions.push(AppAction::SetOnboardPitch(p));
}

/// `SystemMessage::addMessage(title "Driver Eyes Position", text, eDefault)`.
fn message(gui: &mut Gui, text: &str) {
    let text = translate(text);
    gui.apps.actions.push(AppAction::AddMessage { title: translate("Driver Eyes Position"), text });
}

/// The player's car's folder name.
fn unix_name(gui: &Gui) -> String {
    gui.apps.cars.first().map(|c| c.unix_name.clone()).unwrap_or_default()
}

/// `DriverEyesPositioner::saveSettings` 0x140313920: view.ini rewritten whole, camera_onboard.ini
/// `[MODE] FOV`.
fn save_settings(gui: &mut Gui) {
    let unix = unix_name(gui);
    let e = eyes(gui);
    let text = format!("[DRIVER_EYES_POSITION]\r\nDRIVEREYES={},{},{}\r\n\r\n[CAMERA]\r\nON_BOARD_PITCH_ANGLE={}\r\n", format_g(e[0]), format_g(e[1]), format_g(e[2]), format_g(gui.apps.onboard_pitch));
    gui.apps.actions.push(AppAction::WriteFile { path: format!("cfg/cars/{unix}/view.ini"), bytes: text.into_bytes() });
    let fov = format_g(gui.apps.onboard_fov);
    gui.apps.actions.push(AppAction::WriteProfileString { file: "cfg/camera_onboard.ini".to_string(), section: "MODE".to_string(), key: "FOV".to_string(), value: fov });
}

impl Gui {
    /// `DriverEyesPositioner::DriverEyesPositioner(CarAvatar*)` 0x14030fa60 for the player's car;
    /// `car_ini_loose`: its car.ini is a file of its own (`Path::fileExists(.., true)`).
    pub fn new_eyes_app(&mut self, painter: &mut dyn Painter, defaults: EyesDefaults, car_ini_loose: bool) -> Id {
        let form = self.new_form(painter, "DRIVER_EYES_POSITIONER", true);
        // (the render-finished handler of the 3D pass: see the module's notes in the report)
        self.form_set_title(form, &translate("Onboard settings"));
        self.fonts.push(Font::new(painter, "", FontType::Monospaced, 20.0, false, false));
        let font = self.fonts.len() - 1;
        self.controls[form].font = Some(font);
        self.form_data_mut(form).dev_app = false;
        self.set_size(form, 400.0, 315.0);
        let state = Rc::new(EyesState {
            increment: Cell::new(0.0),
            speed: f32::from_bits(0x3d4c_cccd),
            // (a missing camera_onboard.ini or car.ini leaves the game's members unset: 54 and 0 here)
            default_fov: defaults.fov.unwrap_or(54.0),
            default_pitch: defaults.pitch_degrees.map_or(0.0, |p| p * f32::from_bits(0x3c8e_f998)),
            default_eyes: defaults.eyes.unwrap_or([0.0; 3]),
            current_fov: Cell::new(0.0),
            need_to_save: Cell::new(false),
        });
        let text_left = painter.get_texture("content/gui/controls/spinner/leftMinus.png");
        let text_right = painter.get_texture("content/gui/controls/spinner/rightPlus.png");
        let spinner = |gui: &mut Gui, painter: &mut dyn Painter, name: &str, y: f32, size_first: bool| {
            let sp = gui.new_custom_spinner(painter, name, 0);
            gui.add_control(form, sp);
            gui.set_position(sp, 200.0, y);
            if size_first {
                gui.set_size(sp, 150.0, 21.0);
                gui.controls[sp].fore_color = [0.0; 4];
            } else {
                gui.controls[sp].fore_color = [0.0; 4];
                gui.set_size(sp, 150.0, 21.0);
            }
            sp
        };
        let textures = |gui: &mut Gui, sp: Id| {
            let (l, r) = (gui.custom_spinner(sp).left_button, gui.custom_spinner(sp).right_button);
            gui.controls[l].back_texture = text_left.clone();
            gui.controls[r].back_texture = text_right.clone();
            (l, r)
        };
        let on_click = |gui: &mut Gui, c: Id, f: Rc<dyn Fn(&mut Gui)>| {
            gui.controls[c].ev_clicked.push(Rc::new(move |gui: &mut Gui, _: &Clicked| f(gui)));
        };
        // the seat: along the model's z
        let distance = spinner(self, painter, "DISTANCE_SPINNER", 135.0, true);
        self.set_text(distance, &translate("SEAT POS."));
        let (l, r) = textures(self, distance);
        let st = state.clone();
        on_click(self, l, Rc::new(move |gui| {
            let mut e = eyes(gui);
            e[2] = e[2] - st.increment.get();
            set_eyes(gui, e);
            st.need_to_save.set(true);
        }));
        let st = state.clone();
        on_click(self, r, Rc::new(move |gui| {
            let mut e = eyes(gui);
            e[2] = st.increment.get() + e[2];
            set_eyes(gui, e);
            st.need_to_save.set(true);
        }));
        // the field of view: 25 degrees a second, 1 to 125
        let fov = spinner(self, painter, "FOV_SPINNER", 65.0, false);
        let (l, r) = textures(self, fov);
        let st = state.clone();
        on_click(self, l, Rc::new(move |gui| {
            let f = gui.apps.onboard_fov - (st.increment.get() * 25.0f32);
            let f = if f > 125.0 {
                125.0
            } else if f >= 1.0 {
                f
            } else {
                1.0
            };
            set_fov(gui, f);
            st.need_to_save.set(true);
        }));
        let st = state.clone();
        on_click(self, r, Rc::new(move |gui| {
            let f = (st.increment.get() * 25.0f32) + gui.apps.onboard_fov;
            let f = if f > 125.0 {
                125.0
            } else if f < 1.0 {
                1.0
            } else {
                f
            };
            set_fov(gui, f);
            st.need_to_save.set(true);
        }));
        // the pitch: 15 degrees each way
        let pitch = spinner(self, painter, "PITCH_SPINNER", 170.0, true);
        let (l, r) = textures(self, pitch);
        let limit = f32::from_bits(0x3e86_09fe);
        let clamp = move |f: f32| {
            if f > limit {
                limit
            } else if f < -limit {
                -limit
            } else {
                f
            }
        };
        let st = state.clone();
        on_click(self, l, Rc::new(move |gui| {
            let f = st.increment.get() + gui.apps.onboard_pitch;
            set_pitch(gui, clamp(f));
            st.need_to_save.set(true);
        }));
        let st = state.clone();
        on_click(self, r, Rc::new(move |gui| {
            let f = gui.apps.onboard_pitch - st.increment.get();
            set_pitch(gui, clamp(f));
            st.need_to_save.set(true);
        }));
        // the four arrows: the model's y and x
        let mut arrows = [0usize; 4];
        let rows: [(&str, f32, f32, &str, usize, f32); 4] = [("ButtonUp", 75.0, 65.0, "arrowUp", 1, 1.0), ("ButtonDown", 75.0, 165.0, "arrowDown", 1, -1.0), ("ButtonLeft", 25.0, 115.0, "arrowLeft", 0, 1.0), ("ButtonRight", 125.0, 115.0, "arrowRight", 0, -1.0)];
        for (slot, (name, x, y, picture, axis, sign)) in arrows.iter_mut().zip(rows) {
            let b = self.new_control(painter, name);
            self.add_control(form, b);
            self.set_position(b, x, y);
            self.set_size(b, 30.0, 30.0);
            self.controls[b].back_texture = painter.get_texture(&format!("content/gui/driverEyesPositioner/{picture}.png"));
            self.controls[b].draw_background = false;
            self.controls[b].draw_border = false;
            let st = state.clone();
            on_click(self, b, Rc::new(move |gui| {
                let mut e = eyes(gui);
                // up and left add (inc + e), down and right subtract (e - inc)
                e[axis] = if sign > 0.0 { st.increment.get() + e[axis] } else { e[axis] - st.increment.get() };
                set_eyes(gui, e);
                st.need_to_save.set(true);
            }));
            *slot = b;
        }
        let pitch_right = self.custom_spinner(pitch).right_button;
        let w = self.controls[pitch_right].rect.right - self.controls[pitch_right].rect.left;
        let reset = self.new_control(painter, "ButtonReset");
        self.add_control(form, reset);
        self.set_position(reset, w + 200.0, 250.0);
        self.set_size(reset, 108.0, 21.0);
        let st = state.clone();
        on_click(self, reset, Rc::new(move |gui| {
            set_eyes(gui, st.default_eyes);
            set_pitch(gui, st.default_pitch);
            set_fov(gui, st.default_fov);
            st.need_to_save.set(false);
            message(gui, "Reset");
        }));
        self.set_text(reset, &translate("Reset"));
        let save = self.new_control(painter, "ButtonSave");
        self.add_control(form, save);
        self.set_position(save, w + 50.0, 250.0);
        self.set_size(save, 108.0, 21.0);
        let st = state.clone();
        on_click(self, save, Rc::new(move |gui| {
            save_settings(gui);
            st.need_to_save.set(false);
            message(gui, "Configuration Saved");
        }));
        self.set_text(save, &translate("Save"));
        let auto = self.new_control(painter, "Auto pos");
        self.add_control(form, auto);
        let h = self.controls[save].rect.bottom - self.controls[save].rect.top;
        self.set_position(auto, w + 200.0, (h + 250.0) + 10.0);
        self.set_size(auto, 108.0, 21.0);
        on_click(self, auto, Rc::new(|gui| {
            // DriverModel::getEyesPosition in the body node's frame (Node::worldToLocal)
            if let Some(e) = gui.apps.auto_eyes {
                set_eyes(gui, e);
            }
            set_fov(gui, 54.0);
        }));
        self.set_text(auto, &translate("Auto Pos"));
        if car_ini_loose {
            let unix = unix_name(self);
            let save_ini = self.new_control(painter, "ButtonSaveINI");
            self.add_control(form, save_ini);
            self.set_position(save_ini, (w + 100.0) - 70.0, ((h + 250.0) + 10.0) - 70.0);
            self.set_size(save_ini, 108.0, 21.0);
            let (st, u) = (state.clone(), unix.clone());
            on_click(self, save_ini, Rc::new(move |gui| {
                // DriverEyesPositioner::saveToINI 0x1403144e0
                let e = eyes(gui);
                let file = format!("content/cars/{u}/data/car.ini");
                gui.apps.actions.push(AppAction::WriteProfileString { file: file.clone(), section: "GRAPHICS".to_string(), key: "DRIVEREYES".to_string(), value: format!("{},{},{}", format_g(e[0]), format_g(e[1]), format_g(e[2])) });
                let degrees = gui.apps.onboard_pitch * f32::from_bits(0x4265_2ee1);
                gui.apps.actions.push(AppAction::WriteProfileString { file, section: "GRAPHICS".to_string(), key: "ON_BOARD_PITCH_ANGLE".to_string(), value: crate::app_volumes::to_wstring_f(degrees) });
                gui.apps.actions.push(AppAction::DeleteFile { path: format!("cfg/cars/{u}/view.ini") });
                st.need_to_save.set(false);
                message(gui, "Configuration Saved to car.ini");
            }));
            self.set_text(save_ini, &translate("Save car.ini"));
            let save_dash = self.new_control(painter, "ButtonSaveINI");
            self.add_control(form, save_dash);
            self.set_position(save_dash, (w + 300.0) - 70.0, ((h + 250.0) + 10.0) - 70.0);
            self.set_size(save_dash, 108.0, 21.0);
            let st = state.clone();
            on_click(self, save_dash, Rc::new(move |gui| {
                // DriverEyesPositioner::saveToDashINI 0x140314120
                let e = eyes(gui);
                gui.apps.actions.push(AppAction::WriteProfileString { file: format!("content/cars/{unix}/data/dash_cam.ini"), section: "DASH_CAM".to_string(), key: "POS".to_string(), value: format!("{},{},{}", format_g(e[0]), format_g(e[1]), format_g(e[2])) });
                gui.apps.actions.push(AppAction::DeleteFile { path: format!("cfg/cars/{unix}/view.ini") });
                st.need_to_save.set(false);
                message(gui, "Configuration Saved to dash_cam.ini");
            }));
            self.set_text(save_dash, &translate("Save Dash"));
        }
        let m = self.controls[form].scale_mult;
        self.scale_by_mult(form, m);
        self.set_app(form, Box::new(Eyes { regs: [distance, fov, pitch], arrows, state, font }));
        form
    }
}

impl App for Eyes {
    fn as_any(&mut self) -> &mut dyn Any {
        self
    }

    /// `DriverEyesPositioner::render` 0x140312a70.
    fn render(&mut self, gui: &mut Gui, id: Id, dt: f32, painter: &mut dyn Painter) {
        let r0 = gui.controls[id].rect;
        let mut w = Rect { left: r0.left, right: r0.right, top: r0.top, bottom: r0.bottom };
        gui.get_world_rect(id, &mut w);
        let st = self.state.clone();
        if st.current_fov.get() == 0.0 {
            st.current_fov.set(gui.apps.onboard_fov);
        }
        gui.control_render(id, dt, painter);
        let scale = gui.controls[id].scale_mult;
        let font = self.font;
        gui.fonts[font].set_color(1.0, 1.0, 1.0, 1.0);
        gui.fonts[font].blit_string(painter, scale * 200.0 + w.left, scale * 45.0 + w.top, &translate("global settings"), scale, ALIGN_LEFT);
        let text = unix_name(gui) + " " + &translate("settings");
        gui.fonts[font].blit_string(painter, scale * 200.0 + w.left, scale * 110.0 + w.top, &text, scale, ALIGN_LEFT);
        // from here on (and from the next frame on) the GUI's own font
        gui.controls[id].font = Some(gui.font);
        self.font = gui.font;
        let font = self.font;
        let fov_text = format!("{} ({})", translate("FOV"), crate::app_essentials::cut_f(gui.apps.onboard_fov));
        gui.set_text(self.regs[1], &fov_text);
        let pitch_text = format!("{} ({})", translate("PITCH"), crate::app_essentials::cut_f(gui.apps.onboard_pitch * f32::from_bits(0x4265_2ee1)));
        gui.set_text(self.regs[2], &pitch_text);
        for sp in self.regs {
            painter.gl_color4f(GRAPHICS_GL, 1.0, 1.0, 1.0, 1.0);
            let left = gui.custom_spinner(sp).left_button;
            let l0 = gui.controls[left].rect;
            let mut r = Rect { left: l0.left, right: l0.right, top: l0.top, bottom: l0.bottom };
            gui.get_world_rect(left, &mut r);
            painter.gl_quad(GRAPHICS_GL, scale + r.right, r.top, scale * 108.0 - scale * 2.0, scale * 21.0, false);
            gui.fonts[font].set_color(0.0, 0.0, 0.0, 1.0);
            let text = gui.controls[sp].text.clone();
            gui.fonts[font].blit_string(painter, ((scale * 108.0) * 0.5) + r.right, r.top, &text, scale * f32::from_bits(0x3f59_999a), ALIGN_CENTER);
        }
        st.increment.set(dt * st.speed);
        // the arrows held: clicked again every frame
        for b in self.arrows {
            if gui.controls[b].is_mouse_down {
                // (an event the game does not fill in: `local_x` i32::MIN + 1 says so)
                let click = Clicked { control: b, local_x: i32::MIN + 1, local_y: 0 };
                let handlers = gui.controls[b].ev_clicked.clone();
                for h in handlers {
                    h(gui, &click);
                }
                gui.fired.push(click);
            }
        }
        st.current_fov.set(gui.apps.onboard_fov);
        let x = ((w.right - w.left) * 0.5) + w.left;
        let y = ((w.bottom - w.top) + w.top) - scale * 50.0;
        if !st.need_to_save.get() {
            gui.fonts[font].set_color(0.0, 1.0, 0.0, 1.0);
            gui.fonts[font].blit_string(painter, x, y, "", scale, ALIGN_CENTER);
        } else {
            gui.fonts[font].set_color(1.0, 0.0, 0.0, 1.0);
            gui.fonts[font].blit_string(painter, x, y, &translate("Save Needed"), scale, ALIGN_CENTER);
        }
        // Ctrl held: a magenta line down the middle of the screen
        if gui.apps.ctrl_down {
            let (sw, sh) = painter.screen();
            painter.gl_begin(GRAPHICS_GL, 0);
            painter.gl_color3f(GRAPHICS_GL, 1.0, 0.0, 1.0);
            painter.gl_vertex3f(GRAPHICS_GL, sw as f32 * 0.5, 0.0, 0.0);
            painter.gl_vertex3f(GRAPHICS_GL, sw as f32 * 0.5, sh as f32, 0.0);
            painter.gl_end(GRAPHICS_GL);
        }
    }
}
