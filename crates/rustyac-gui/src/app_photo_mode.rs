// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! `PhotoMode` (0x250), the developers' "Photo mode" (not the replay's photo mode): "Start" puts
//! the scene camera into the free camera and pauses the physics (outside a replay), draws the
//! view finder over the screen, and seven -/+ spinners change YEBIS's exposure multiplier, the
//! scene camera's field of view, depth of field factor and focus and roll, and the sun's azimuth
//! and zenith. The mouse wheel changes the field of view (with the right button: the focus); the
//! middle button takes a screenshot.
//!
//! `PhotoMode::PhotoMode(Sim*)` 0x140321bc0, `render` 0x140323fc0, `onMouseWheelMovedEvent`
//! 0x140323ec0, the Start handler 0x1403233b0, the spinners' handlers 0x140323200 … 0x140323b70.
//!
//! The camera, the exposure and the sun are members of the game's scene camera, graphics
//! manager and sun animator; here they are [`PhotoLink`] in [`crate::apps::AppLink`], which
//! whoever runs the GUI fills and carries out.

use std::any::Any;
use std::cell::Cell;
use std::rc::Rc;

use rustyac_physics::vecmath::{xm_matrix_multiply, Mat44f, Vec3f};

use crate::app_eyes::format_g;
use crate::apps::{App, AppAction};
use crate::control::{Clicked, Id, MouseEvent};
use crate::font::{Font, FontType};
use crate::gui::Gui;
use crate::hud::translate;
use crate::painter::{Painter, Texture, GRAPHICS_GL};
use crate::widgets::WidgetEvent;

/// `ACCameraManager`'s `eFree`.
pub const CAMERA_FREE: i32 = 6;

/// What the app reads and changes.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PhotoLink {
    /// the scene camera (`CameraForward`): its matrix (+0xc), field of view (+8), depth of
    /// field factor (+0x1b0) and focus (+0x1b4)
    pub matrix: Mat44f,
    pub fov: f32,
    pub dof_factor: f32,
    pub dof_focus: f32,
    /// `GraphicsManager::exposureMultiplier` (+0xc)
    pub exposure_multiplier: f32,
    /// `SunAnimator::enableCustomSunDirection(true)` then `setSunDirection`: the sun's direction
    /// (`GraphicsManager::customSunDirection`); the sun animator stops
    pub custom_sun: Option<Vec3f>,
    /// `ACCameraManager::mode` (+0x120)
    pub camera_mode: i32,
    /// `ReplayManager::isInReplaymode`
    pub in_replay: bool,
    /// `GetAsyncKeyState`: the right button, the middle button
    pub right_button: bool,
    pub middle_button: bool,
}

/// The members the handlers share.
struct State {
    active: Cell<bool>,
    azimuth: Cell<f32>,
    zenith: Cell<f32>,
    prev_mode: Cell<i32>,
    multiplier: Cell<f32>,
}

pub struct PhotoMode {
    state: Rc<State>,
    view_finder: Texture,
    /// exposure, fov, dof factor, dof focus, roll, azimuth, zenith
    spinners: [Id; 7],
}

/// The "+" handlers' clamp to [0, 1000] (NaN: 1000).
fn clamp_up(f: f32) -> f32 {
    let mut u = 1000.0;
    if f <= 1000.0 {
        u = 0.0;
        if 0.0 <= f {
            return f;
        }
    }
    u
}

/// The "-" handlers' clamp to [0, 1000] (NaN stays).
fn clamp_down(f: f32) -> f32 {
    if 1000.0 < f {
        1000.0
    } else if f < 0.0 {
        0.0
    } else {
        f
    }
}

/// `SunAnimator::getSunDirection(zenith, azimuth, 0)` 0x1401adad0.
pub fn sun_direction(a: f32, b: f32) -> Vec3f {
    let deg = f32::from_bits(0x3c8e_f998);
    let t = ((90.0 - b) - 0.0) * deg;
    let s = -a * deg;
    let sn = rustyac_math::sinf(s);
    let ct = rustyac_math::cosf(t);
    let x = ct * sn;
    let cs = rustyac_math::cosf(s);
    let st = rustyac_math::sinf(t);
    let z0 = st * (sn * -1.0);
    let mut v = Vec3f::new(-x, -cs, -z0);
    v.normalize();
    v
}

impl Gui {
    /// `PhotoMode::PhotoMode(Sim*)` 0x140321bc0: 280 x 450, a developers' app.
    pub fn new_photo_mode_app(&mut self, painter: &mut dyn Painter) -> Id {
        let form = self.new_form(painter, "CAMERA_PICTURE_MODE", false);
        self.form_set_title(form, &translate("Photo mode"));
        self.form_data_mut(form).dev_app = true;
        self.form_set_size(form, 280.0, 450.0);
        let view_finder = painter.get_texture("content//gui//photoMode//viewFinder.png");
        let state = Rc::new(State { active: Cell::new(false), azimuth: Cell::new(0.0), zenith: Cell::new(0.0), prev_mode: Cell::new(0), multiplier: Cell::new(1.0) });
        // "Start": the free camera and the physics paused, or the camera back
        let start = self.new_control(painter, "ACTIVATE_CONTROL");
        self.add_control(form, start);
        self.set_size(start, 100.0, 25.0);
        let x = ((self.controls[form].rect.right - self.controls[form].rect.left) - (self.controls[start].rect.right - self.controls[start].rect.left)) * 0.5;
        self.set_position(start, x, 50.0);
        let st = state.clone();
        self.controls[start].ev_clicked.push(Rc::new(move |gui: &mut Gui, _: &Clicked| {
            st.active.set(!st.active.get());
            let mode = if st.active.get() {
                st.prev_mode.set(gui.apps.photo.camera_mode);
                CAMERA_FREE
            } else {
                st.prev_mode.get()
            };
            gui.apps.photo.camera_mode = mode;
            gui.apps.actions.push(AppAction::SetCameraMode(mode));
            if !gui.apps.photo.in_replay {
                gui.apps.actions.push(AppAction::PausePhysics(st.active.get()));
            }
        }));
        self.set_text(start, &translate("Start"));
        self.fonts.push(Font::new(painter, "", FontType::Monospaced, 20.0, false, false));
        let font = self.fonts.len() - 1;
        self.fonts[font].set_color(1.0, 0.0, 0.0, 1.0);
        self.controls[form].font = Some(font);
        // the precision: 10^(item - 3)
        let precision = self.new_spinner(painter, "PRECISION EXP", true, false);
        self.set_size(precision, 200.0, 30.0);
        let x = ((self.controls[form].rect.right - self.controls[form].rect.left) - 200.0) * 0.5;
        self.spinner_set_position(precision, x, 110.0);
        for i in 0..=6 {
            self.spinner_add_item(precision, &(i - 3).to_string());
        }
        let st = state.clone();
        self.spinner_mut(precision).handlers.push(Rc::new(move |_: &mut Gui, ev: &WidgetEvent| {
            let WidgetEvent::SpinnerValueChanged { value, .. } = ev else { return };
            st.multiplier.set(10f64.powf((*value - 3) as f64) as f32);
        }));
        self.spinner_set_value(precision, 3);
        self.add_control(form, precision);
        // the seven -/+ spinners: (name, y, "+", "-")
        type Step = fn(&mut PhotoLink, &State);
        fn sun(p: &mut PhotoLink, s: &State) {
            p.custom_sun = Some(sun_direction(s.zenith.get(), s.azimuth.get()));
        }
        fn roll(p: &mut PhotoLink, a: f32) {
            let turn = Mat44f::create_from_axis_angle(&Vec3f::new(0.0, 0.0, 1.0), a);
            p.matrix = xm_matrix_multiply(&turn, &p.matrix);
        }
        let rows: [(&str, f32, Step, Step); 7] = [
            ("EXPO", 150.0, |p, _| p.exposure_multiplier += f32::from_bits(0x3ba3_d70a), |p, _| p.exposure_multiplier -= f32::from_bits(0x3ba3_d70a)),
            ("EXPO", 190.0, |p, s| p.fov = clamp_up(s.multiplier.get() * 0.5 + p.fov), |p, s| p.fov = clamp_down(p.fov - s.multiplier.get() * 0.5)),
            ("EXPO", 230.0, |p, s| p.dof_factor = clamp_up(s.multiplier.get() * 0.05 + p.dof_factor), |p, s| p.dof_factor = clamp_down(p.dof_factor - s.multiplier.get() * 0.05)),
            ("EXPO", 270.0, |p, s| p.dof_focus = clamp_up(s.multiplier.get() * 0.005 + p.dof_focus), |p, s| p.dof_focus = clamp_down(p.dof_focus - s.multiplier.get() * 0.005)),
            ("EXPO", 310.0, |p, s| roll(p, s.multiplier.get() * 0.005), |p, s| roll(p, s.multiplier.get() * -0.005)),
            (
                "AZIMUTH",
                350.0,
                |p, s| {
                    s.azimuth.set(s.multiplier.get() * 0.35 + s.azimuth.get());
                    sun(p, s)
                },
                |p, s| {
                    s.azimuth.set(s.azimuth.get() - s.multiplier.get() * 0.35);
                    sun(p, s)
                },
            ),
            (
                "EXPO",
                390.0,
                |p, s| {
                    s.zenith.set(s.multiplier.get() * 0.35 + s.zenith.get());
                    sun(p, s)
                },
                |p, s| {
                    s.zenith.set(s.zenith.get() - s.multiplier.get() * 0.35);
                    sun(p, s)
                },
            ),
        ];
        let text_left = painter.get_texture("content/gui/controls/spinner/leftMinus.png");
        let text_right = painter.get_texture("content/gui/controls/spinner/rightPlus.png");
        let mut spinners = [0; 7];
        for (k, (name, y, plus, minus)) in rows.into_iter().enumerate() {
            let sp = self.new_custom_spinner(painter, name, 0);
            let (l, r) = (self.custom_spinner(sp).left_button, self.custom_spinner(sp).right_button);
            self.controls[l].back_texture = text_left.clone();
            self.controls[r].back_texture = text_right.clone();
            self.set_size(sp, 49.0, 23.0);
            self.set_position(sp, 10.0, y);
            self.add_control(form, sp);
            for (button, step) in [(r, plus), (l, minus)] {
                let st = state.clone();
                self.controls[button].ev_clicked.push(Rc::new(move |gui: &mut Gui, _: &Clicked| step(&mut gui.apps.photo, &st)));
            }
            spinners[k] = sp;
        }
        self.set_app(form, Box::new(PhotoMode { state, view_finder, spinners }));
        form
    }
}

impl App for PhotoMode {
    fn as_any(&mut self) -> &mut dyn Any {
        self
    }

    /// `PhotoMode::render` 0x140323fc0: the view finder and the middle button while active, the
    /// spinners' titles, the form.
    fn render(&mut self, gui: &mut Gui, id: Id, dt: f32, painter: &mut dyn Painter) {
        if self.state.active.get() {
            painter.gl_color4f(GRAPHICS_GL, 1.0, 1.0, 1.0, 1.0);
            painter.set_texture(0, &self.view_finder);
            painter.gl_quad(GRAPHICS_GL, 0.0, 0.0, gui.screen.0 as f32, gui.screen.1 as f32, true);
            // shortcut: END (RayPicker's autofocus on the screen's centre) not ported, rustyAC has no ray picker; port it with the DOF pass
            if gui.apps.photo.middle_button {
                gui.apps.actions.push(AppAction::TakeScreenshot);
            }
        }
        let p = gui.apps.photo;
        let titles = [
            format!("{} : {}", translate("EXP. Mult"), format_g(p.exposure_multiplier)),
            format!("{} : {}", translate("FOV"), format_g(p.fov)),
            format!("{} : {}", translate("DOF factor"), format_g(p.dof_factor * 100.0)),
            format!("{} : {}", translate("DOF focus"), format_g(p.dof_focus * 10.0)),
            translate("Roll"),
            translate("Azimuth"),
            translate("Zenith"),
        ];
        // (the game's order: DOF factor, DOF focus, EXP, FOV, Roll, Azimuth, Zenith)
        for k in [2, 3, 0, 1, 4, 5, 6] {
            let title = gui.custom_spinner(self.spinners[k]).title;
            gui.set_text(title, &titles[k]);
        }
        gui.control_render(id, dt, painter);
    }

    /// `PhotoMode::onMouseWheelMovedEvent` 0x140323ec0: while the window is visible, the field
    /// of view (down: wider), with the right button down the focus.
    fn on_mouse_wheel(&mut self, gui: &mut Gui, id: Id, e: &MouseEvent) {
        if !gui.controls[id].visible {
            return;
        }
        let m = self.state.multiplier.get();
        let p = &mut gui.apps.photo;
        if !p.right_button {
            if 0.0 >= e.delta {
                if p.fov < 120.0 {
                    p.fov = m * 0.5 + p.fov;
                }
            } else if p.fov > 0.5 {
                p.fov = p.fov - m * 0.5;
            }
        } else {
            p.dof_focus = clamp_down(e.delta * f32::from_bits(0x3a03_126f) + p.dof_focus);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamps_and_sun() {
        assert_eq!(clamp_up(f32::NAN), 1000.0);
        assert!(clamp_down(f32::NAN).is_nan());
        assert_eq!((clamp_up(-1.0), clamp_up(2000.0), clamp_down(-1.0)), (0.0, 1000.0, 0.0));
        // zenith 0: the sun straight overhead, the light pointing down
        let v = sun_direction(0.0, 0.0);
        assert!((v.y + 1.0).abs() < 1e-6 && v.x.abs() < 1e-6, "{v:?}");
    }
}
