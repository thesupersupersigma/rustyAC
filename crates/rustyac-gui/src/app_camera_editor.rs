// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! `FormCamera` (0x2d0), the developers' "Camera editor": the driving views (F1) and the car's
//! own cameras (F6) of the first car, moved, turned and given an exposure and a field of view
//! with buttons, saved into the car's `data/car.ini`, `dash_cam.ini` and `cameras.ini`.
//!
//! `FormCamera::FormCamera(Sim*, CameraForwardYebis*)` 0x1402f1bc0, `render` 0x1402f6ff0, `move`
//! 0x1402f6b50, `elev` 0x1402f6670, `strafe` 0x1402f8ba0, `rotate` 0x1402f7550, `selectCamera`
//! 0x1402f8760, `setCamera` 0x1402f89b0, `setExp` 0x1402f8ae0, `increaseExp` 0x1402f69f0,
//! `decreaseExp` 0x1402f6510, `increaseSens` 0x1402f6ad0, `decreaseSens` 0x1402f65f0,
//! `savecameraF1` 0x1402f7990, `savecameraF6` 0x1402f8540.

use std::any::Any;
use std::cell::RefCell;
use std::rc::Rc;

use rustyac_physics::vecmath::{xm_matrix_inverse, xm_matrix_multiply, Mat44f, Vec3f};

use crate::acos::format_g;
use crate::app_volumes::to_wstring_f;
use crate::apps::{App, AppAction};
use crate::control::{Clicked, Id};
use crate::gui::Gui;
use crate::hud::translate;
use crate::painter::Painter;

const RED: [f32; 4] = [1.0, 0.0, 0.0, 1.0];
const CLEAR: [f32; 4] = [0.0; 4];
const WHITE: [f32; 4] = [1.0; 4];

/// One of `CarAvatar::cameras` (`CameraCarDefinition`, 0x4c): its matrix in the frame of the
/// car's body node.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct EditorCamera {
    pub matrix: Mat44f,
    pub fov: f32,
    pub exposure: f32,
    pub external_sound: bool,
}

/// What the window reads and changes of the game: the first car's, the focused car's
/// drivable views, the scene camera, YEBIS's exposure.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CameraEditorLink {
    /// `CarAvatar::unixName` of car 0 (its files: `content/cars/<unix>/data/...`)
    pub unix_name: String,
    pub on_board_exposure: f32,
    pub out_board_exposure: f32,
    pub dash_board_exposure: f32,
    pub cameras: Vec<EditorCamera>,
    /// `CarAvatar::driverEyesPosition`
    pub eyes: [f32; 3],
    /// `CameraOnBoard::customCameraSettings[0].pitch`, radians
    pub onboard_pitch: f32,
    /// the focused car's `DrivableCameraDef`
    pub bumper_pos: [f32; 3],
    pub bumper_pitch: f32,
    pub bonnet_pos: [f32; 3],
    pub bonnet_pitch: f32,
    /// `Sim::getSceneCamera()`'s matrix and car 0's body node's
    pub scene_camera: Mat44f,
    pub body: Mat44f,
    /// `CameraForwardYebis::currentAutoExposure` (read) and `useAutoExposureLimits` (cleared)
    pub auto_exposure: f32,
    pub use_auto_exposure_limits: bool,
    /// the filter's `[AUTO_EXPOSURE]` MIN, MAX and TARGET (`pp` +0xcbc..; the HDR Settings')
    pub auto_exposure_min: f32,
    pub auto_exposure_max: f32,
    pub auto_exposure_target: f32,
    /// the car has a `data.acd` (the HDR Settings' save button hidden)
    pub data_acd: bool,
}

#[derive(Default)]
struct State {
    is_by_button: bool,
    target_exp: f32,
    timing: f32,
    last_auto_exp: f32,
    current_level: i16,
    last_level: i16,
    high_exp: i16,
    low_exp: i16,
    high_diff: f32,
    low_diff: f32,
    /// 0 eCockpit (F1), 1 eCar (F6)
    current_mode: i32,
    current_camera: i32,
    current_sensibility: f32,
    explabel: Id,
    fovlabel: Id,
    ext_sound: Id,
    int_sound: Id,
    sens_label: [Id; 3],
    camera_f1: [Id; 6],
    camera_f6: [Id; 6],
}

type Shared = Rc<RefCell<State>>;

pub struct CameraEditor {
    state: Shared,
}

impl Gui {
    /// `FormCamera::FormCamera` 0x1402f1bc0. `auto_exp`: `system/cfg/assetto_corsa.ini
    /// [AC_APPS] AUTOEXP` (0 without it).
    pub fn new_camera_editor_app(&mut self, painter: &mut dyn Painter, auto_exp: f32) -> Id {
        let form = self.new_form(painter, "FORM_CAMERA", false);
        let state: Shared = Rc::new(RefCell::new(State {
            target_exp: auto_exp,
            current_sensibility: 3.0,
            ..State::default()
        }));
        self.form_set_title(form, &translate("Camera editor"));
        // the F1 row: the names are the game's pointer arithmetic on L"COCK" (L"COCK" + i)
        let cock = ["COCK", "OCK", "CK", "K", "", ""];
        for (i, name) in cock.iter().enumerate() {
            let c = self.new_control(painter, name);
            self.control_add_control(form, c);
            self.set_text(c, &format!("F1-{}", i + 1));
            self.set_size(c, 50.0, 30.0);
            self.controls[c].back_color = CLEAR;
            self.set_position(c, i as f32 * 55.0 + 5.0, 35.0);
            if i == 1 || i == 2 {
                self.set_visible(c, false);
            }
            let s = Rc::clone(&state);
            self.controls[c]
                .ev_clicked
                .push(Rc::new(move |gui: &mut Gui, _: &Clicked| {
                    select_camera(gui, &mut s.borrow_mut(), 0, i as i32)
                }));
            state.borrow_mut().camera_f1[i] = c;
        }
        // the F6 row: L"CAM" + i, and past its end the narrow "F6-" read as wide characters
        let cam = ["CAM", "AM", "M", "", "\u{3646}-", "-"];
        for (i, name) in cam.iter().enumerate() {
            let c = self.new_control(painter, name);
            self.control_add_control(form, c);
            self.set_text(c, &format!("F6-{}", i + 1));
            self.set_size(c, 50.0, 30.0);
            self.controls[c].back_color = CLEAR;
            self.set_position(c, i as f32 * 55.0 + 5.0, 70.0);
            let s = Rc::clone(&state);
            self.controls[c]
                .ev_clicked
                .push(Rc::new(move |gui: &mut Gui, _: &Clicked| {
                    select_camera(gui, &mut s.borrow_mut(), 1, i as i32)
                }));
            state.borrow_mut().camera_f6[i] = c;
            // car 0 gets at least six cameras, copies of its first
            if let Some(l) = self.apps.camera_editor.as_mut() {
                if i >= l.cameras.len() && !l.cameras.is_empty() {
                    let first = l.cameras[0];
                    l.cameras.push(first);
                }
            }
        }
        let label = |gui: &mut Gui,
                     painter: &mut dyn Painter,
                     name: &str,
                     text: &str,
                     x: f32,
                     y: f32|
         -> Id {
            let l = gui.new_label(painter, name);
            gui.controls[l].font_scale = 0.5;
            gui.set_text(l, text);
            gui.controls[l].back_color = RED;
            gui.set_size(l, 50.0, 10.0);
            gui.set_position(l, x, y);
            gui.control_add_control(form, l);
            l
        };
        type Action = fn(&mut Gui, &mut State);
        let button = |gui: &mut Gui,
                      painter: &mut dyn Painter,
                      name: &str,
                      text: &str,
                      scale: f32,
                      x: f32,
                      y: f32,
                      action: Action|
         -> Id {
            let b = gui.new_control(painter, name);
            gui.controls[b].font_scale = scale;
            gui.set_text(b, text);
            gui.set_size(b, 50.0, 20.0);
            gui.set_repeat_interval(b, 0.05);
            gui.set_position(b, x, y);
            let s = Rc::clone(&state);
            gui.controls[b]
                .ev_clicked
                .push(Rc::new(move |gui: &mut Gui, _: &Clicked| {
                    action(gui, &mut s.borrow_mut())
                }));
            gui.control_add_control(form, b);
            b
        };
        for (i, (name, text)) in [
            ("PitchUP", "PITCH"),
            ("PitchDOWN", "PITCH"),
            ("RollLeft", "ROLL"),
            ("RollRight", "ROLL"),
            ("YawLeft", "YAW"),
            ("YawRight", "YAW"),
        ]
        .into_iter()
        .enumerate()
        {
            label(self, painter, name, text, i as f32 * 55.0 + 5.0, 105.0);
        }
        let turns: [(&str, &str, Action); 6] = [
            ("PitchUP_V", "Up", |g, s| rotate(g, s, [1.0, 0.0, 0.0], 1)),
            ("PitchDOWN_V", "Down", |g, s| {
                rotate(g, s, [1.0, 0.0, 0.0], -1)
            }),
            ("RollLeft_V", "Left", |g, s| {
                rotate(g, s, [0.0, 0.0, 1.0], 1)
            }),
            ("RollRight_V", "Right", |g, s| {
                rotate(g, s, [0.0, 0.0, 1.0], -1)
            }),
            ("YawLeft_V", "Left", |g, s| rotate(g, s, [0.0, 1.0, 0.0], 1)),
            ("YawRight_V", "Right", |g, s| {
                rotate(g, s, [0.0, 1.0, 0.0], -1)
            }),
        ];
        for (i, (name, text, action)) in turns.into_iter().enumerate() {
            button(
                self,
                painter,
                name,
                text,
                0.7,
                i as f32 * 55.0 + 5.0,
                115.0,
                action,
            );
        }
        for (i, (name, text)) in [
            ("ElevUP", "ELEV"),
            ("ElevDOWN", "ELEV"),
            ("StrafeLeft", "STRAFE"),
            ("StrafeRight", "STRAFE"),
            ("MoveFor", "MOVE"),
            ("MoveBack", "MOVE"),
        ]
        .into_iter()
        .enumerate()
        {
            label(self, painter, name, text, i as f32 * 55.0 + 5.0, 140.0);
        }
        let moves: [(&str, &str, Action); 6] = [
            ("ElevUP_V", "Up", |g, s| {
                translate_camera(g, s, Way::Elev, 1)
            }),
            ("ElevDOWN_V", "Down", |g, s| {
                translate_camera(g, s, Way::Elev, -1)
            }),
            ("StrafeLeft_V", "Left", |g, s| {
                translate_camera(g, s, Way::Strafe, -1)
            }),
            ("StrafeRight_V", "Right", |g, s| {
                translate_camera(g, s, Way::Strafe, 1)
            }),
            ("MoveFor_V", "Forw", |g, s| {
                translate_camera(g, s, Way::Move, 1)
            }),
            ("MoveBack_V", "Back", |g, s| {
                translate_camera(g, s, Way::Move, -1)
            }),
        ];
        for (i, (name, text, action)) in moves.into_iter().enumerate() {
            button(
                self,
                painter,
                name,
                text,
                0.7,
                i as f32 * 55.0 + 5.0,
                150.0,
                action,
            );
        }
        label(self, painter, "ExpDec", "EXP", 5.0, 175.0);
        label(self, painter, "Val", "CURRENT", 60.0, 175.0);
        label(self, painter, "ExpInc", "EXP", 115.0, 175.0);
        let bar = self.new_label(painter, "SensB");
        self.set_text(bar, "");
        self.controls[bar].back_color = RED;
        self.set_size(bar, 160.0, 10.0);
        self.set_position(bar, 170.0, 175.0);
        self.control_add_control(form, bar);
        label(self, painter, "SensLabel", "SENSITIVITY", 170.0, 175.0);
        button(self, painter, "ExpDec_V", "-", 0.8, 5.0, 185.0, |g, s| {
            add_exp(g, s, -1.0)
        });
        let e = self.new_label(painter, "e");
        self.controls[e].font_scale = 0.7;
        self.set_size(e, 20.0, 20.0);
        self.set_position(e, 75.0, 185.0);
        self.control_add_control(form, e);
        button(self, painter, "ExpInc_V", "+", 0.7, 115.0, 185.0, |g, s| {
            add_exp(g, s, 1.0)
        });
        let sens_button =
            |gui: &mut Gui, painter: &mut dyn Painter, name: &str, x: f32, up: bool| {
                let b = gui.new_control(painter, name);
                gui.controls[b].font_scale = 0.7;
                gui.set_text(b, if up { "+" } else { "-" });
                gui.set_size(b, 20.0, 20.0);
                gui.set_position(b, x, 185.0);
                let s = Rc::clone(&state);
                gui.controls[b]
                    .ev_clicked
                    .push(Rc::new(move |gui: &mut Gui, _: &Clicked| {
                        change_sens(gui, &mut s.borrow_mut(), up)
                    }));
                gui.control_add_control(form, b);
            };
        sens_button(self, painter, "SenDec", 170.0, false);
        let mut sens = [0; 3];
        for (i, name) in ["Sens1", "Sens2", "Sens3"].into_iter().enumerate() {
            let l = self.new_label(painter, name);
            self.controls[l].font_scale = 0.7;
            self.set_text(l, " ");
            self.controls[l].back_color = RED;
            self.set_size(l, 40.0, 20.0);
            self.set_position(l, 190.0 + i as f32 * 40.0, 185.0);
            self.control_add_control(form, l);
            sens[i] = l;
        }
        sens_button(self, painter, "SenInc", 310.0, true);
        label(self, painter, "FovDec", "FOV", 5.0, 210.0);
        label(self, painter, "Valf", "CURRENT", 60.0, 210.0);
        label(self, painter, "FovInc", "FOV", 115.0, 210.0);
        // the one 160 wide: sized before it is placed (the base rectangle keeps the first place)
        let sound = self.new_label(painter, "Sound");
        self.controls[sound].font_scale = 0.5;
        self.set_text(sound, "Sound");
        self.controls[sound].back_color = RED;
        self.set_size(sound, 160.0, 10.0);
        self.set_position(sound, 170.0, 210.0);
        self.control_add_control(form, sound);
        button(self, painter, "FovDec_V", "-", 0.8, 5.0, 220.0, |g, s| {
            add_fov(g, s, -1.0)
        });
        let f = self.new_label(painter, "f");
        self.controls[f].font_scale = 0.7;
        self.set_size(f, 20.0, 20.0);
        self.set_position(f, 75.0, 220.0);
        self.control_add_control(form, f);
        button(self, painter, "FovInc_V", "+", 0.7, 115.0, 220.0, |g, s| {
            add_fov(g, s, 1.0)
        });
        let ext = button(
            self,
            painter,
            "ExtSound",
            "External",
            0.8,
            170.0,
            220.0,
            |g, s| set_sound(g, s, true),
        );
        let int = button(
            self,
            painter,
            "IntSound",
            "Internal",
            0.8,
            253.0,
            220.0,
            |g, s| set_sound(g, s, false),
        );
        for b in [ext, int] {
            self.set_size(b, 77.0, 20.0);
            // the game sizes these before placing them: the base rectangle is 77 wide too
            self.controls[b].rect_base = self.controls[b].rect;
            self.controls[b].back_color = WHITE;
        }
        let bottom: [(&str, &str, f32, f32, Action); 3] = [
            ("SETCAMERA", "SET", 50.0, 5.0, |g, s| {
                if s.current_mode == 1 {
                    sync_car_cam(g, s);
                }
            }),
            ("SAVE CAMERACAR", "SAVE CAMERAS", 160.0, 60.0, |g, s| {
                save_f1(g, s);
                save_f6(g);
            }),
            ("AUTOEXP", "AUTO EXP", 120.0, 225.0, |g, s| {
                select_camera(g, s, 0, 0);
                s.is_by_button = true;
            }),
        ];
        for (name, text, w, x, action) in bottom {
            let b = self.new_control(painter, name);
            self.control_add_control(form, b);
            self.set_text(b, &translate(text));
            self.set_size(b, w, 30.0);
            self.set_position(b, x, 245.0);
            let s = Rc::clone(&state);
            self.controls[b]
                .ev_clicked
                .push(Rc::new(move |gui: &mut Gui, _: &Clicked| {
                    action(gui, &mut s.borrow_mut())
                }));
        }
        {
            let mut s = state.borrow_mut();
            (
                s.explabel,
                s.fovlabel,
                s.ext_sound,
                s.int_sound,
                s.sens_label,
            ) = (e, f, ext, int, sens);
        }
        self.form_set_size(form, 350.0, 280.0);
        self.set_app(form, Box::new(CameraEditor { state }));
        form
    }
}

#[derive(Clone, Copy)]
enum Way {
    Move,
    Elev,
    Strafe,
}

/// `powf(10.0f, currentSensibility)`
fn power(s: &State) -> f32 {
    rustyac_math::powf(10.0, s.current_sensibility)
}

/// The scene camera as car 0's camera `currentCamera`, in its body's frame.
fn sync_car_cam(gui: &mut Gui, s: &State) {
    let Some(l) = gui.apps.camera_editor.as_mut() else {
        return;
    };
    let m = xm_matrix_multiply(&l.scene_camera, &xm_matrix_inverse(&l.body));
    if let Some(c) = l.cameras.get_mut(s.current_camera as usize) {
        c.matrix = m;
    }
}

/// `move` / `elev` / `strafe`: the scene camera along its own axis (F6), or the eye point or the
/// bonnet or bumper camera's place (F1).
fn translate_camera(gui: &mut Gui, s: &mut State, way: Way, dir: i32) {
    let step = power(s) * 1e-5;
    let fd = dir as f32;
    let Some(l) = gui.apps.camera_editor.as_mut() else {
        return;
    };
    match s.current_mode {
        1 => {
            let m = &mut l.scene_camera.m;
            for k in 0..3 {
                let axis = match way {
                    Way::Move => -m[2][k],
                    Way::Elev => m[1][k],
                    Way::Strafe => m[0][k],
                };
                m[3][k] += (axis * fd) * step;
            }
            sync_car_cam(gui, s);
        }
        0 => {
            let p = match s.current_camera {
                0 => &mut l.eyes,
                3 => &mut l.bonnet_pos,
                4 => &mut l.bumper_pos,
                _ => return,
            };
            match way {
                Way::Move => p[2] += fd * step,
                Way::Elev => p[1] += fd * step,
                Way::Strafe => p[0] -= fd * step,
            }
            let action = drivable_action(l, s.current_camera);
            gui.apps.actions.extend(action);
        }
        _ => {}
    }
}

/// What rustyac.exe is told after a change to the eye point or a drivable view.
fn drivable_action(l: &CameraEditorLink, camera: i32) -> Option<AppAction> {
    match camera {
        0 => Some(AppAction::SetDriverEyes(l.eyes)),
        _ => None,
    }
}

/// `rotate(axis, dir)`: the scene camera about its own axis (F6), the pitch of the cockpit,
/// bonnet or bumper view (F1, the pitch buttons only), at most 15 degrees either way.
fn rotate(gui: &mut Gui, s: &mut State, axis: [f32; 3], dir: i32) {
    let p = power(s);
    let angle =
        ((1.0f64.atan() * 4.0) * 0.005_555_555_555_555_556 * dir as f64 * (p * 0.001) as f64)
            as f32;
    let Some(l) = gui.apps.camera_editor.as_mut() else {
        return;
    };
    match s.current_mode {
        1 => {
            let r = Mat44f::create_from_axis_angle(&Vec3f::new(axis[0], axis[1], axis[2]), angle);
            l.scene_camera = xm_matrix_multiply(&r, &l.scene_camera);
            sync_car_cam(gui, s);
        }
        0 if axis[0] == 1.0 => {
            let step = p * 1e-5;
            const LIMIT: f32 = 0.261_794_98;
            let clamp = |x: f32| {
                if x > LIMIT {
                    LIMIT
                } else if x >= -LIMIT {
                    x
                } else {
                    -LIMIT
                }
            };
            let pitch = match s.current_camera {
                0 => &mut l.onboard_pitch,
                3 => &mut l.bonnet_pitch,
                4 => &mut l.bumper_pitch,
                _ => return,
            };
            *pitch = clamp(dir as f32 * step + *pitch);
            if s.current_camera == 0 {
                let p = l.onboard_pitch;
                gui.apps.actions.push(AppAction::SetOnboardPitch(p));
            }
        }
        _ => {}
    }
}

/// The exposure `setExp` / `increaseExp` / `decreaseExp` change.
fn exposure<'a>(l: &'a mut CameraEditorLink, s: &State) -> Option<&'a mut f32> {
    match s.current_mode {
        0 => Some(match s.current_camera {
            0 => &mut l.on_board_exposure,
            5 => &mut l.dash_board_exposure,
            _ => &mut l.out_board_exposure,
        }),
        1 => l
            .cameras
            .get_mut(s.current_camera as usize)
            .map(|c| &mut c.exposure),
        _ => None,
    }
}

fn add_exp(gui: &mut Gui, s: &mut State, delta: f32) {
    if let Some(e) = gui.apps.camera_editor.as_mut().and_then(|l| exposure(l, s)) {
        *e += delta;
    }
}

fn add_fov(gui: &mut Gui, s: &mut State, delta: f32) {
    if s.current_mode != 1 {
        return;
    }
    if let Some(c) = gui
        .apps
        .camera_editor
        .as_mut()
        .and_then(|l| l.cameras.get_mut(s.current_camera as usize))
    {
        c.fov += delta;
    }
}

fn set_sound(gui: &mut Gui, s: &mut State, external: bool) {
    if s.current_mode == 1 {
        if let Some(c) = gui
            .apps
            .camera_editor
            .as_mut()
            .and_then(|l| l.cameras.get_mut(s.current_camera as usize))
        {
            c.external_sound = external;
        }
    }
    gui.controls[s.int_sound].back_color = if external { CLEAR } else { RED };
    gui.controls[s.ext_sound].back_color = if external { RED } else { CLEAR };
}

/// `increaseSens` / `decreaseSens`: 1 to 3, as many bars.
fn change_sens(gui: &mut Gui, s: &mut State, up: bool) {
    if up {
        s.current_sensibility += 1.0;
        if s.current_sensibility > 3.0 {
            s.current_sensibility = 3.0;
        }
    } else {
        s.current_sensibility -= 1.0;
        if !(s.current_sensibility >= 1.0) {
            s.current_sensibility = 1.0;
        }
    }
    for i in 0..3 {
        gui.set_visible(s.sens_label[i], (i as f32) < s.current_sensibility);
    }
}

/// `selectCamera(mode, index)`.
fn select_camera(gui: &mut Gui, s: &mut State, mode: i32, index: i32) {
    s.current_mode = mode;
    for id in s.camera_f1.into_iter().chain(s.camera_f6) {
        gui.controls[id].back_color = CLEAR;
    }
    let save = s.target_exp != 0.0 && s.current_level != s.last_level && !s.is_by_button;
    if mode == 0 {
        gui.apps
            .actions
            .push(AppAction::SelectCamera { mode, index });
        if save {
            save_f1(gui, s);
        }
        s.last_level = s.current_level;
        s.current_camera = index;
        gui.controls[s.camera_f1[index as usize]].back_color = RED;
        gui.controls[s.ext_sound].back_color = WHITE;
        gui.controls[s.int_sound].back_color = WHITE;
    } else if mode == 1 {
        gui.apps
            .actions
            .push(AppAction::SelectCamera { mode, index });
        if save {
            save_f6(gui);
        }
        s.last_level = s.current_level;
        s.current_camera = index;
        gui.controls[s.camera_f6[index as usize]].back_color = RED;
        let external = gui
            .apps
            .camera_editor
            .as_ref()
            .and_then(|l| l.cameras.get(index as usize))
            .is_some_and(|c| c.external_sound);
        gui.controls[s.int_sound].back_color = if external { CLEAR } else { RED };
        gui.controls[s.ext_sound].back_color = if external { RED } else { CLEAR };
    }
}

/// `savecameraF1`: `car.ini [GRAPHICS]` (`dash_cam.ini [DASH_CAM] EXP` for the dash view).
fn save_f1(gui: &mut Gui, s: &State) {
    let Some(l) = gui.apps.camera_editor.as_ref() else {
        return;
    };
    let folder = format!("content/cars/{}/data", l.unix_name);
    let mut keys = Vec::new();
    let file = if s.current_camera == 5 {
        keys.push((
            "DASH_CAM",
            "EXP",
            (l.dash_board_exposure as i32).to_string(),
        ));
        format!("{folder}/dash_cam.ini")
    } else {
        keys.push((
            "GRAPHICS",
            "ONBOARD_EXPOSURE",
            (l.on_board_exposure as i32).to_string(),
        ));
        keys.push((
            "GRAPHICS",
            "OUTBOARD_EXPOSURE",
            (l.out_board_exposure as i32).to_string(),
        ));
        let three = |p: [f32; 3]| {
            format!(
                "{},{},{}",
                to_wstring_f(p[0]),
                to_wstring_f(p[1]),
                to_wstring_f(p[2])
            )
        };
        match s.current_camera {
            0 => {
                keys.push((
                    "GRAPHICS",
                    "ON_BOARD_PITCH_ANGLE",
                    to_wstring_f(l.onboard_pitch * 57.29578),
                ));
                keys.push(("GRAPHICS", "DRIVEREYES", three(l.eyes)));
            }
            3 => {
                keys.push((
                    "GRAPHICS",
                    "BONNET_CAMERA_PITCH",
                    to_wstring_f(l.bonnet_pitch * 57.29578),
                ));
                keys.push(("GRAPHICS", "BONNET_CAMERA_POS", three(l.bonnet_pos)));
            }
            4 => {
                keys.push((
                    "GRAPHICS",
                    "BUMPER_CAMERA_PITCH",
                    to_wstring_f(l.bumper_pitch * 57.29578),
                ));
                keys.push(("GRAPHICS", "BUMPER_CAMERA_POS", three(l.bumper_pos)));
            }
            _ => {}
        }
        format!("{folder}/car.ini")
    };
    for (section, key, value) in keys {
        gui.apps.actions.push(AppAction::WriteProfileString {
            file: file.clone(),
            section: section.into(),
            key: key.into(),
            value,
        });
    }
}

/// `savecameraF6`: every camera of car 0 into `cameras.ini` (a `wofstream`, `%.5g`).
fn save_f6(gui: &mut Gui) {
    let Some(l) = gui.apps.camera_editor.as_ref() else {
        return;
    };
    gui.apps.actions.push(AppAction::WriteFile {
        path: format!("content/cars/{}/data/cameras.ini", l.unix_name),
        bytes: cameras_ini(&l.cameras).into_bytes(),
    });
}

fn cameras_ini(cameras: &[EditorCamera]) -> String {
    let g = |v: f32| format_g(v as f64, 5);
    let three = |v: [f32; 3]| format!("{},{},{}", g(v[0]), g(v[1]), g(v[2]));
    let mut out = String::new();
    for (n, c) in cameras.iter().enumerate() {
        let m = &c.matrix.m;
        out += &format!("[CAMERA_{n}]\r\n");
        out += &format!("POSITION={}\r\n", three([m[3][0], m[3][1], m[3][2]]));
        out += &format!("FORWARD={}\r\n", three([-m[2][0], -m[2][1], -m[2][2]]));
        out += &format!("UP={}\r\n", three([m[1][0], m[1][1], m[1][2]]));
        out += &format!("FOV={}\r\n", g(c.fov));
        out += &format!("EXPOSURE={}\r\n", g(c.exposure));
        out += &format!(
            "EXTERNAL_SOUND={}\r\n\r\n",
            if c.external_sound { "1" } else { "0" }
        );
    }
    out
}

impl App for CameraEditor {
    fn as_any(&mut self) -> &mut dyn Any {
        self
    }

    /// `FormCamera::render` 0x1402f6ff0: the exposure and field of view shown, the automatic
    /// exposure's runs (YEBIS's measured exposure, camera after camera), then the form.
    fn render(&mut self, gui: &mut Gui, id: Id, dt: f32, painter: &mut dyn Painter) {
        let s = &mut *self.state.borrow_mut();
        let (mut exp, mut fov) = (40, 60);
        if let Some(l) = gui.apps.camera_editor.as_mut() {
            if s.current_mode == 0 {
                exp = match s.current_camera {
                    0 => l.on_board_exposure as i32,
                    5 => {
                        if l.dash_board_exposure as i32 == 0 {
                            l.dash_board_exposure = (l.on_board_exposure as i32) as f32;
                        }
                        l.dash_board_exposure as i32
                    }
                    _ => l.out_board_exposure as i32,
                };
            } else if s.current_mode == 1 {
                if let Some(c) = l.cameras.get(s.current_camera as usize) {
                    (exp, fov) = (c.exposure as i32, c.fov as i32);
                }
            }
        }
        gui.set_text(s.explabel, &exp.to_string());
        gui.set_text(s.fovlabel, &fov.to_string());
        let auto = gui
            .apps
            .camera_editor
            .as_ref()
            .map_or(0.0, |l| l.auto_exposure);
        if s.target_exp == 0.0 {
            if s.is_by_button {
                s.timing += dt;
                if s.timing > 3.0 {
                    s.target_exp = auto;
                    s.current_level += 1;
                    s.timing = 0.0;
                }
            }
        } else {
            if let Some(l) = gui.apps.camera_editor.as_mut() {
                l.use_auto_exposure_limits = false;
            }
            s.timing += dt;
            if s.timing > 3.0 {
                s.last_auto_exp = auto;
                s.timing = 0.0;
                let (last, target) = (s.last_auto_exp, s.target_exp);
                if last <= target {
                    if (target - last) as f64 > 0.05 {
                        add_exp(gui, s, 1.0);
                    }
                    add_exp(gui, s, 1.0);
                    s.low_exp = exp as i16;
                    s.low_diff = s.target_exp - s.last_auto_exp;
                } else {
                    if (last - target) as f64 > 0.05 {
                        add_exp(gui, s, -1.0);
                    }
                    add_exp(gui, s, -1.0);
                    s.high_exp = exp as i16;
                    s.high_diff = s.last_auto_exp - s.target_exp;
                }
                if s.high_exp != 0 && s.low_exp != 0 {
                    let pick = if s.low_diff < s.high_diff {
                        s.low_exp
                    } else {
                        s.high_exp
                    };
                    if let Some(e) = gui.apps.camera_editor.as_mut().and_then(|l| exposure(l, s)) {
                        *e = pick as i32 as f32;
                    }
                    s.high_exp = 0;
                    s.low_exp = 0;
                    s.current_level += 1;
                }
            }
            match s.current_level {
                0 => select_camera(gui, s, 0, 0),
                1..=6 => select_camera(gui, s, 1, s.current_level as i32 - 1),
                7 => select_camera(gui, s, 0, 2),
                8 => select_camera(gui, s, 0, 5),
                9 => {
                    save_f1(gui, s);
                    save_f6(gui);
                    s.target_exp = 0.0;
                    // shortcut: the game quits (exit(2)) after a run started from assetto_corsa.ini's AUTOEXP; rustyAC goes on
                    s.is_by_button = false;
                }
                _ => {}
            }
        }
        gui.control_render(id, dt, painter);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cameras_ini_is_the_wofstream_s() {
        let mut c = EditorCamera {
            matrix: Mat44f::IDENTITY,
            fov: 56.0,
            exposure: 40.5,
            external_sound: true,
        };
        c.matrix.m[3] = [0.123_456, -0.0, 1e-5, 1.0];
        assert_eq!(cameras_ini(&[c]), "[CAMERA_0]\r\nPOSITION=0.12346,-0,1e-005\r\nFORWARD=-0,-0,-1\r\nUP=0,1,0\r\nFOV=56\r\nEXPOSURE=40.5\r\nEXTERNAL_SOUND=1\r\n\r\n");
    }
}
