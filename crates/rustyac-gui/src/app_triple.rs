// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! `TripleScreenManager` (0x3c8), "Triple screen": the angle, width, distance and bezels of the
//! three screens, turned into their points for `CameraTripleScreen` at once and written to
//! `cfg\triple_screen.ini` when the game closes (only after a change). Made only in triple-screen
//! mode (`Sim::Sim`), and drawn only then.
//!
//! Ctor 0x1402ff920, `initGraphicalComponents` 0x1403016e0, `onSpinnerValueChanged` 0x140301b90
//! (all four spinners' handler), `updateGraphicalInterfaceData` 0x140302000,
//! `rotateLateralScreen` 0x140301f20, `render` 0x140301ee0, `drawVirtualDisplay` 0x140301290,
//! `drawDriverHead` 0x140300ec0, the destructor 0x140300710; `VirtualDisplay::updateData`
//! 0x14033e150, `rotateVertexBidimensionalDegreeRespectToPivot` 0x14033d890.

use std::any::Any;
use std::cell::RefCell;
use std::rc::Rc;

use rustyac_math::{cosf, sinf, sqrtf};
use rustyac_physics::data::ini::IniReader;
use rustyac_physics::vecmath::{Mat44f, Vec3f};

use crate::apps::{App, AppAction};
use crate::control::Id;
use crate::gui::Gui;
use crate::hud::translate;
use crate::painter::{Painter, Texture, ALIGN_CENTER, GRAPHICS_GL};
use crate::widgets::WidgetEvent;

/// `TripleScreenData::screens`, left to right: left point x, y, right point x, y, height.
pub type Screens = [[f32; 5]; 3];

/// `VirtualDisplay` / `LateralVirtualDisplay` (0x80): one screen of the drawing, seen from above.
#[derive(Clone, Copy, Default)]
struct VirtualDisplay {
    visible: [[f32; 2]; 4],
    display: [[f32; 2]; 4],
    back: [[f32; 2]; 4],
    lateral: bool,
}

impl VirtualDisplay {
    /// `VirtualDisplay::updateData(center, margin, width)`; the depth is the ctor's 12.
    fn update(&mut self, c: [f32; 2], m: f32, w: f32) {
        let (hd, hw) = (12.0f32 * 0.5, w * 0.5);
        let (l, r, t, b) = (c[0] - hw, hw + c[0], c[1] - hd, hd + c[1]);
        self.visible = [[l, t], [r, t], [r, b], [l, b]];
        self.display = [[l - m, t], [r + m, t], [r + m, b], [l - m, b]];
        self.back = [[(l + 5.0) - m, (c[1] - 5.0) - hd], [(r - 5.0) + m, (c[1] - 5.0) - hd], [r + m, b], [l - m, b]];
    }

    /// `rotateLateralScreen`: about one corner, which itself turns on its own turn.
    fn rotate(&mut self, pivot: usize, deg: f32) {
        for i in 0..4 {
            self.visible[i] = turn(self.visible[i], self.display[pivot], deg);
            self.display[i] = turn(self.display[i], self.display[pivot], deg);
            self.back[i] = turn(self.back[i], self.back[pivot], deg);
        }
    }
}

/// `rotateVertexBidimensionalDegreeRespectToPivot`.
fn turn(v: [f32; 2], p: [f32; 2], deg: f32) -> [f32; 2] {
    let (dx, dy) = (v[0] - p[0], v[1] - p[1]);
    let a = (deg * f32::from_bits(0x4049_0fdb)) * f32::from_bits(0x3bb6_0b61);
    let (s, c) = (sinf(a), cosf(a));
    [(c * dx - s * dy) + p[0], (c * dy + s * dx) + p[1]]
}

struct State {
    /// rotation, screen width, distance, margins
    sp: [Id; 4],
    screens: Screens,
    is_modified: bool,
    main_center: [f32; 2],
    displays: [VirtualDisplay; 3],
}

pub struct TripleScreenApp {
    state: Rc<RefCell<State>>,
    head: Texture,
}

fn value(gui: &Gui, id: Id) -> f32 {
    gui.spinner(id).value as f32
}

/// `onSpinnerValueChanged`: the screens' points (metres) from the spinners, into the camera.
fn on_spinner_value_changed(gui: &mut Gui, state: &Rc<RefCell<State>>) {
    let sp = state.borrow().sp;
    let a = gui.screen.1 as f32 / (gui.screen.0 as f32 * f32::from_bits(0x3eaa_aaab));
    let milli = f32::from_bits(0x3a83_126f);
    let sw = value(gui, sp[1]) * milli;
    let dist = value(gui, sp[2]) * milli;
    let rot = value(gui, sp[0]) * f32::from_bits(0x3c8e_f998);
    let mg = value(gui, sp[3]) * milli;
    let height = sw * a;
    let hm = mg * 0.5;
    let (nhalf, phalf) = (sw * -0.5, sw * 0.5);
    let up = Vec3f::new(0.0, 1.0, 0.0);
    // (the screen's direction: the turned (-1, 0, 0) and (1, 0, 0), as the machine sums them)
    let m = Mat44f::create_from_axis_angle(&up, -rot).m;
    let vx = ((m[1][0] * 0.0 + m[0][0] * -1.0) + m[2][0] * 0.0) + m[3][0];
    let vz = ((m[1][2] * 0.0 + m[0][2] * -1.0) + m[2][2] * 0.0) + m[3][2];
    let right0 = [vx * hm + (nhalf - hm), vz * hm + dist];
    let left0 = [vx * sw + right0[0], vz * sw + right0[1]];
    let m = Mat44f::create_from_axis_angle(&up, rot).m;
    let vx = ((m[1][0] * 0.0 + m[0][0]) + m[2][0] * 0.0) + m[3][0];
    let vz = ((m[1][2] * 0.0 + m[0][2]) + m[2][2] * 0.0) + m[3][2];
    let left2 = [vx * hm + (hm + phalf), vz * hm + dist];
    let right2 = [vx * sw + left2[0], vz * sw + left2[1]];
    let screens = [
        [left0[0], left0[1], right0[0], right0[1], height],
        [nhalf, dist, phalf, dist, height],
        [left2[0], left2[1], right2[0], right2[1], height],
    ];
    {
        let mut s = state.borrow_mut();
        s.screens = screens;
        s.is_modified = true;
    }
    gui.apps.actions.push(AppAction::SetTripleScreens(screens));
    update_graphical_interface_data(gui, state);
}

/// `updateGraphicalInterfaceData`: the drawing, 200 pixels for 3 m.
fn update_graphical_interface_data(gui: &Gui, state: &Rc<RefCell<State>>) {
    let mut s = state.borrow_mut();
    let third = f32::from_bits(0x39ae_c33e);
    let m = (value(gui, s.sp[3]) * third) * 200.0;
    let w = (value(gui, s.sp[1]) * third) * 200.0;
    let rot = value(gui, s.sp[0]);
    let c = s.main_center;
    s.displays[0].update(c, m, w);
    s.displays[1].update([(c[0] - w) - m * 2.0, 150.0], m, w);
    s.displays[2].update([(m * 2.0 + c[0]) + w, 150.0], m, w);
    s.displays[1].rotate(2, -rot);
    s.displays[2].rotate(3, rot);
}

/// The file the destructor writes (`std::ofstream`, text mode, floats as `%g`).
pub fn ini_text(screens: &Screens, sp: [i32; 4]) -> String {
    let g = crate::acos::format_g6;
    let mut out = String::from("[TRIPLE_SCREEN]\r\nACTIVE=1\r\n\r\n");
    for (section, s) in ["LEFT_SCREEN", "CENTER_SCREEN", "RIGHT_SCREEN"].iter().zip(screens) {
        out += &format!("[{section}]\r\nLEFT_POINT={},{}\r\nRIGHT_POINT={},{}\r\n\r\n", g(s[0]), g(s[1]), g(s[2]), g(s[3]));
    }
    out + &format!("[SPINNERS]\r\nSP_MARGINS={}\r\nSP_ROTATION={}\r\nSP_DISTANCE={}\r\nSP_WIDTH={}\r\n", sp[3], sp[0], sp[2], sp[1])
}

impl Gui {
    /// `TripleScreenManager::TripleScreenManager(Sim*)`: the camera's screens as they are now,
    /// `ini` the player's cfg/triple_screen.ini (none: no file).
    pub fn new_triple_screen_app(&mut self, painter: &mut dyn Painter, screens: Screens, ini: Option<&IniReader>) -> Id {
        let form = self.new_form(painter, "TRIPLE_SCREEN", false);
        self.form_set_title(form, &translate("Triple screen"));
        self.form_data_mut(form).dev_app = false;
        self.form_set_size(form, 600.0, 700.0);
        self.controls[form].font = Some(self.font);
        let spinner = |gui: &mut Gui, painter: &mut dyn Painter, name: &str, text: &str, (min, max): (i32, i32)| {
            let sp = gui.new_spinner(painter, name, false, false);
            gui.set_text(sp, &translate(text));
            gui.spinner_mut(sp).min_value = min;
            gui.spinner_mut(sp).max_value = max;
            sp
        };
        let help = |gui: &mut Gui, painter: &mut dyn Painter, name: &str, text: &str, x: f32, y: f32| {
            let label = gui.new_label(painter, name);
            gui.set_text(label, &translate(text));
            gui.controls[label].font_align = ALIGN_CENTER;
            gui.set_position(label, x, y);
            gui.add_control(form, label);
        };
        let rotation = spinner(self, painter, "ROTATION_SPINNER", "ANGLE", (0, 90));
        self.spinner_set_position(rotation, 30.0, 420.0);
        self.set_size(rotation, 200.0, 30.0);
        self.add_control(form, rotation);
        help(self, painter, "HELP_ROTATION", "Angle between central \n and lateral displays (deg)", 30.0, 460.0);
        let width = spinner(self, painter, "SPINNER_SCREEN_WIDTH", "SCREEN WIDTH", (0, 3000));
        self.spinner_set_position(width, 30.0, 570.0);
        self.set_size(width, 200.0, 30.0);
        self.add_control(form, width);
        help(self, painter, "HELP_WIDTH", "Width of each screen \n (mm)", 30.0, 610.0);
        // (the right column: the form's width less the spinner's, less 30)
        let right_x = |gui: &Gui, sp: Id| {
            let (f, s) = (gui.controls[form].rect, gui.controls[sp].rect);
            ((f.right - f.left) - (s.right - s.left)) - 30.0
        };
        let distance = spinner(self, painter, "DISTANCE_SPINNER", "DISTANCE", (0, 3000));
        self.set_size(distance, 200.0, 30.0);
        let x = right_x(self, distance);
        self.spinner_set_position(distance, x, 420.0);
        self.add_control(form, distance);
        help(self, painter, "HELP_DISTANCE", "Distance between central \n display and the player (mm)", right_x(self, distance), 460.0);
        let margins = spinner(self, painter, "MARGIN_SPINNER", "MARGINS", (-1000, 1000));
        self.set_size(margins, 200.0, 30.0);
        let x = right_x(self, margins);
        self.spinner_set_position(margins, x, 570.0);
        self.add_control(form, margins);
        help(self, painter, "HELP_MARGINS", "Margin of each \n display/Bezel (mm)", right_x(self, distance), 610.0);

        // the camera's centre screen (before the handlers: no event)
        let c = screens[1];
        self.spinner_set_value(distance, (c[1] * 1000.0) as i32);
        let (dx, dy) = (c[0] - c[2], c[1] - c[3]);
        self.spinner_set_value(width, (sqrtf(dy * dy + dx * dx) * 1000.0) as i32);
        self.spinner_set_value(rotation, 0);
        self.spinner_set_value(margins, 0);
        let lateral = VirtualDisplay { lateral: true, ..VirtualDisplay::default() };
        let state = Rc::new(RefCell::new(State { sp: [rotation, width, distance, margins], screens, is_modified: false, main_center: [0.0; 2], displays: [VirtualDisplay::default(), lateral, lateral] }));
        for (sp, interval) in [(rotation, 0x3dcc_cccd), (width, 0x3c23_d70a), (distance, 0x3c23_d70a), (margins, 0x3d4c_cccd)] {
            let state = state.clone();
            self.spinner_mut(sp).handlers.push(Rc::new(move |gui: &mut Gui, ev: &WidgetEvent| {
                if matches!(ev, WidgetEvent::SpinnerValueChanged { .. }) {
                    on_spinner_value_changed(gui, &state);
                }
            }));
            self.set_repeat_interval(sp, f32::from_bits(interval));
        }

        // initGraphicalComponents: the file's spinners (each a change of its own)
        if let Some(ini) = ini.filter(|ini| ini.ready && ini.has_section("SPINNERS")) {
            let get = |key: &str| ini.get_int("SPINNERS", key).unwrap_or(0);
            let v = get("SP_DISTANCE");
            if v > 0 {
                self.spinner_set_value(distance, v);
            }
            let v = get("SP_WIDTH");
            if v > 0 {
                self.spinner_set_value(width, v);
            }
            self.spinner_set_value(margins, get("SP_MARGINS"));
            self.spinner_set_value(rotation, get("SP_ROTATION"));
        }
        let head = painter.get_texture("content/gui/triplescreenManager/headHelmet.png");
        {
            let mut s = state.borrow_mut();
            s.main_center = [290.0, 150.0];
            s.is_modified = false;
        }
        update_graphical_interface_data(self, &state);
        self.set_app(form, Box::new(TripleScreenApp { state, head }));
        form
    }
}

impl TripleScreenApp {
    /// What the destructor would write now (none: nothing changed).
    pub fn file(&self, gui: &Gui) -> Option<Vec<u8>> {
        let s = self.state.borrow();
        s.is_modified.then(|| ini_text(&s.screens, s.sp.map(|id| gui.spinner(id).value)).into_bytes())
    }
}

impl App for TripleScreenApp {
    fn as_any(&mut self) -> &mut dyn Any {
        self
    }

    /// `render`: only in triple-screen mode; the form, then the drawing of the screens.
    fn render(&mut self, gui: &mut Gui, id: Id, dt: f32, painter: &mut dyn Painter) {
        if !gui.sim.triple_screen {
            return;
        }
        gui.control_render(id, dt, painter);
        let r = gui.controls[id].rect;
        let (fx, fy) = (r.left, r.top);
        let s = self.state.borrow();
        let gl = GRAPHICS_GL;
        // drawVirtualDisplay: the frame, its back, the picture (red at the sides, green ahead)
        for d in &s.displays {
            painter.gl_color3f(gl, 0.0, 0.0, 0.0);
            for points in [&d.display, &d.back] {
                painter.gl_begin(gl, 3);
                for p in points {
                    painter.gl_vertex3f(gl, p[0] + fx, p[1] + fy, 0.0);
                }
                painter.gl_end(gl);
            }
            if d.lateral {
                painter.gl_color3f(gl, 1.0, 0.0, 0.0);
            } else {
                painter.gl_color3f(gl, 0.0, 1.0, 0.0);
            }
            painter.gl_begin(gl, 3);
            for p in &d.visible {
                painter.gl_vertex3f(gl, p[0] + fx, p[1] + fy, 0.0);
            }
            painter.gl_end(gl);
        }
        // drawDriverHead: the driver at the distance, and what each side screen shows of the view
        let p = ((value(gui, s.sp[2]) * f32::from_bits(0x39ae_c33e)) * 200.0) * 2.0;
        painter.set_texture(0, &self.head);
        let (x, y) = (fx + 290.0, (12.0f32 * 0.5 + (fy + 150.0)) + p);
        painter.gl_quad(gl, x - 25.0, y, 50.0, 50.0, true);
        painter.gl_color4f(gl, 1.0, 1.0, 0.0, f32::from_bits(0x3f4c_cccd));
        let [central, left, right] = &s.displays;
        let at = |v: [f32; 2]| (v[0] + fx, v[1] + fy);
        let tri = [(x, y), at(left.visible[3]), at(right.visible[2])];
        let quad = [at(left.visible[3]), at(left.display[2]), at(central.display[2]), at(right.visible[2])];
        for (mode, points) in [(2, &tri[..]), (3, &quad[..])] {
            painter.gl_begin(gl, mode);
            for &(vx, vy) in points {
                painter.gl_vertex3f(gl, vx, vy, 0.0);
            }
            painter.gl_end(gl);
        }
    }

    /// The destructor: the file, only after a change.
    fn shutdown(&mut self, gui: &mut Gui, _id: Id) {
        if let Some(bytes) = self.file(gui) {
            gui.apps.actions.push(AppAction::WriteFile { path: "cfg/triple_screen.ini".into(), bytes });
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn ini_text_layout() {
        let text = super::ini_text(&[[-1.0, 0.5, -0.3, 0.7, 0.3]; 3], [10, 600, 700, 5]);
        assert!(text.starts_with("[TRIPLE_SCREEN]\r\nACTIVE=1\r\n\r\n[LEFT_SCREEN]\r\nLEFT_POINT=-1,0.5\r\nRIGHT_POINT=-0.3,0.7\r\n\r\n"));
        assert!(text.ends_with("[SPINNERS]\r\nSP_MARGINS=5\r\nSP_ROTATION=10\r\nSP_DISTANCE=700\r\nSP_WIDTH=600\r\n"));
    }
}
