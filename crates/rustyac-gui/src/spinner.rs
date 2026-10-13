// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! `ksgui::Spinner` (0x270 bytes), the value with a minus and a plus button the setup screen
//! is made of, and `ksgui::CustomSpinner` (0x1d8), four bare buttons round a picture.
//!
//! `Spinner::Spinner` 0x14024bac0, its handlers 0x14024c810 (the strip under it), 0x14024c9a0
//! (plus), 0x14024ca10 (the label) and 0x14024ca40 (minus), `addItem` 0x14024cb80,
//! `getCurrentItem` 0x14024cc20, `onMouseDown` 0x14024ccd0, `onMouseMove` 0x14024cd10,
//! `render` 0x14024cdd0, `resize` 0x14024d6b0, `scaleByMult` 0x14024d800, `setDrawArrows`
//! 0x14024d830, `setLabelText` 0x14024dac0, `setPosition` 0x14024db30, `setRepeatInterval`
//! 0x14024dbc0, `setSize` 0x14024dbe0, `setValue` 0x14024dd30 / 0x14024de60;
//! `CustomSpinner::CustomSpinner` 0x14024ead0, `render` 0x14024f0c0, `resize` 0x14024f3c0,
//! `scaleByMult` 0x14024f6c0, `setSize` 0x14024f6f0.

use std::rc::Rc;

use crate::control::{Clicked, Id, MouseEvent, NO_CONTROL};
use crate::font::{Font, FontType};
use crate::gui::Gui;
use crate::painter::{Painter, Texture, ALIGN_CENTER, GL_TRIANGLES};
use crate::widgets::{msvc_floor, WidgetEvent, WidgetHandler};
use crate::widgets3::Widget3;

pub struct Spinner {
    pub homogeneous_pressing_delay: bool,
    pub display_value_mult: f32,
    pub min_value: i32,
    pub max_value: i32,
    pub step: i32,
    pub handlers: Vec<WidgetHandler>,
    pub pop_over: Id,
    pub display_popover: bool,
    pub draw_underline: bool,
    pub display_central_value: bool,
    pub display_label: bool,
    pub units: String,
    pub text_size: f32,
    pub is_read_only: bool,
    pub counter: f32,
    pub pression_counter: f32,
    pub but_plus: Id,
    pub but_minus: Id,
    pub selection_slider: Id,
    pub label: Id,
    pub value_back: Texture,
    pub dressed: bool,
    pub last_on_control_clicked: Clicked,
    pub value: i32,
    pub items: Vec<String>,
    pub mirrored: bool,
    pub triangle_width: f32,
}

/// `CustomSpinnerMode`: 1 the round one (four arms), 2 the bar that goes up and down.
pub struct CustomSpinner {
    pub press_to_increment: bool,
    pub left_button: Id,
    pub right_button: Id,
    pub up_button: Id,
    pub down_button: Id,
    pub title: Id,
    pub bar: Texture,
    pub bar_position_normalized: f32,
    pub mode: i32,
}

impl Gui {
    pub fn spinner(&self, id: Id) -> &Spinner {
        match self.widget3(id) {
            Widget3::Spinner(s) => s,
            _ => panic!("control {id} is not a spinner"),
        }
    }

    pub fn spinner_mut(&mut self, id: Id) -> &mut Spinner {
        match self.widget3_mut(id) {
            Widget3::Spinner(s) => s,
            _ => panic!("control {id} is not a spinner"),
        }
    }

    /// `Spinner::Spinner(name, gui, dressed, mirrored)` 0x14024bac0.
    pub fn new_spinner(&mut self, painter: &mut dyn Painter, name: &str, dressed: bool, mirrored: bool) -> Id {
        let id = self.new_control(painter, name);
        let but_plus = self.new_control(painter, "PLUS");
        let but_minus = self.new_control(painter, "MINUS");
        self.set_text(but_plus, "+");
        self.set_text(but_minus, "-");
        // one font for the strip and the label: Consolas, 13
        self.fonts.push(Font::new(painter, "", FontType::Monospaced, 13.0, false, false));
        let font = self.fonts.len() - 1;
        let selection_slider = self.new_control(painter, "SELECTION_SLIDER");
        self.controls[selection_slider].back_color = [0.5; 4];
        self.controls[selection_slider].border_color = [1.0, 1.0, 1.0, 0.0];
        self.controls[selection_slider].font = Some(font);
        self.add_control(id, selection_slider);
        let label = self.new_label(painter, name);
        self.controls[label].font = Some(font);
        self.controls[label].font_align = ALIGN_CENTER;
        self.set_text(label, name);
        self.controls[label].draw_background = false;
        self.controls[label].draw_border = false;
        self.add_control(id, but_minus);
        self.add_control(id, but_plus);
        self.add_control(id, label);
        let mut value_back = Texture::default();
        if dressed {
            self.controls[selection_slider].draw_background = false;
            self.controls[selection_slider].draw_border = false;
            self.controls[id].draw_border = false;
            self.controls[id].draw_background = false;
            value_back = painter.get_texture("content/gui/controls/spinner/backContainer.png");
            self.controls[but_minus].back_texture = painter.get_texture(if mirrored { "content/gui/controls/spinner/leftPlus.png" } else { "content/gui/controls/spinner/leftMinus.png" });
            self.controls[but_minus].draw_background = false;
            self.controls[but_minus].back_color = [1.0; 4];
            self.controls[but_minus].draw_border = false;
            self.controls[but_plus].draw_background = false;
            self.controls[but_plus].draw_border = false;
            self.controls[but_plus].back_texture = painter.get_texture(if mirrored { "content/gui/controls/spinner/rightMinus.png" } else { "content/gui/controls/spinner/rightPlus.png" });
            self.controls[but_plus].back_color = [1.0; 4];
            self.controls[label].back_texture = painter.get_texture("content/gui/controls/spinner/labelContainer.png");
            self.controls[label].draw_background = false;
            self.controls[label].draw_border = false;
            self.set_text(but_plus, "");
            self.set_text(but_minus, "");
        }
        let pop_over = self.new_pop_over(painter, &format!("{name}_POPOVER"));
        self.set_visible(pop_over, false);
        self.add_control(id, pop_over);
        self.set_widget3(
            id,
            Widget3::Spinner(Spinner {
                homogeneous_pressing_delay: false,
                display_value_mult: 1.0,
                min_value: 0,
                max_value: 10,
                step: 1,
                handlers: Vec::new(),
                pop_over,
                display_popover: false,
                draw_underline: true,
                display_central_value: true,
                display_label: true,
                units: String::new(),
                text_size: 15.0,
                is_read_only: false,
                counter: 0.0,
                pression_counter: 0.0,
                but_plus,
                but_minus,
                selection_slider,
                label,
                value_back,
                dressed,
                last_on_control_clicked: Clicked { control: NO_CONTROL, local_x: 0, local_y: 0 },
                value: 0,
                items: Vec::new(),
                mirrored,
                triangle_width: 10.0,
            }),
        );
        // a click on the strip: the value at that share of its width, in whole steps
        self.controls[selection_slider].ev_clicked.push(Rc::new(move |gui: &mut Gui, ev: &Clicked| {
            let s = gui.spinner(id);
            if s.is_read_only {
                return;
            }
            let r = gui.controls[s.selection_slider].rect;
            let mut lx = ev.local_x as f32;
            let mut w = r.right - r.left;
            if s.dressed {
                let t = s.triangle_width * gui.controls[id].scale_mult;
                lx -= t;
                let w2 = w - t;
                if lx > w2 {
                    lx = w2;
                } else if lx < 0.0 {
                    lx = 0.0;
                }
                w = (r.right - r.left) - t;
            }
            let v = ((s.max_value.wrapping_sub(s.min_value)) as f32 * (lx / w) + s.min_value as f32 + 0.5) as i32;
            let step = s.step;
            if step != 0 {
                gui.spinner_set_value(id, v.wrapping_div(step).wrapping_mul(step));
            }
        }));
        self.controls[label].ev_clicked.push(Rc::new(move |gui: &mut Gui, _: &Clicked| {
            let s = gui.spinner(id);
            if s.display_popover {
                let pop_over = s.pop_over;
                let visible = gui.controls[pop_over].visible;
                gui.set_visible(pop_over, !visible);
            }
        }));
        self.controls[but_minus].ev_clicked.push(Rc::new(move |gui: &mut Gui, ev: &Clicked| {
            let s = gui.spinner_mut(id);
            s.last_on_control_clicked = *ev;
            let v = s.value.wrapping_sub(s.step);
            gui.spinner_set_value(id, v);
        }));
        self.controls[but_plus].ev_clicked.push(Rc::new(move |gui: &mut Gui, ev: &Clicked| {
            let s = gui.spinner_mut(id);
            s.last_on_control_clicked = *ev;
            let v = s.value.wrapping_add(s.step);
            gui.spinner_set_value(id, v);
        }));
        id
    }

    /// `Spinner::setValue(int)` 0x14024de60: kept in the range; the listeners hear of it every
    /// time.
    pub fn spinner_set_value(&mut self, id: Id, value: i32) {
        let s = self.spinner_mut(id);
        let v = if value > s.max_value {
            s.max_value
        } else if value < s.min_value {
            s.min_value
        } else {
            value
        };
        s.value = v;
        let handlers = s.handlers.clone();
        self.fire_widget(handlers, WidgetEvent::SpinnerValueChanged { spinner: id, value: v });
    }

    /// `Spinner::setValue(const std::wstring&)` 0x14024dd30: the (last) item of that text.
    pub fn spinner_set_value_by_item(&mut self, id: Id, item: &str) {
        if let Some(index) = self.spinner(id).items.iter().rposition(|i| i == item) {
            self.spinner_set_value(id, index as i32);
        }
    }

    /// `Spinner::addItem` 0x14024cb80: with items the value is an index into them.
    pub fn spinner_add_item(&mut self, id: Id, item: &str) {
        let s = self.spinner_mut(id);
        s.items.push(item.to_string());
        s.min_value = 0;
        s.max_value = s.items.len() as i32 - 1;
    }

    /// `Spinner::getCurrentItem` 0x14024cc20.
    pub fn spinner_current_item(&self, id: Id) -> String {
        let s = self.spinner(id);
        if s.value >= 0 { s.items.get(s.value as usize).cloned().unwrap_or_default() } else { String::new() }
    }

    /// `Spinner::setLabelText` 0x14024dac0.
    pub fn spinner_set_label_text(&mut self, id: Id, text: &str) {
        let label = self.spinner(id).label;
        self.set_text(label, text);
    }

    /// `Spinner::setDrawArrows` 0x14024d830: arrows in place of minus and plus.
    pub fn spinner_set_draw_arrows(&mut self, painter: &mut dyn Painter, id: Id, arrows: bool) {
        let (plus, minus) = (self.spinner(id).but_plus, self.spinner(id).but_minus);
        let (right, left) = if arrows { ("content/gui/controls/spinner/rightContainerArrow.png", "content/gui/controls/spinner/leftContainerArrow.png") } else { ("content/gui/controls/spinner/rightPlus.png", "content/gui/controls/spinner/leftMinus.png") };
        self.controls[plus].back_texture = painter.get_texture(right);
        self.controls[minus].back_texture = painter.get_texture(left);
    }

    /// `Spinner::setRepeatInterval` 0x14024dbc0.
    pub fn spinner_set_repeat_interval(&mut self, id: Id, v: f32) {
        let (plus, minus) = (self.spinner(id).but_plus, self.spinner(id).but_minus);
        self.controls[plus].repeat_interval = v;
        self.controls[minus].repeat_interval = v;
    }

    /// The four children's places (`setSize`, `setPosition` and `resize` share them): minus at
    /// the left, plus at the right, the strip under the value, the label above.
    fn spinner_place_children(&mut self, id: Id, label_x_extra: f32) {
        let s = self.spinner(id);
        let (plus, minus, slider, label) = (s.but_plus, s.but_minus, s.selection_slider, s.label);
        let r = self.controls[id].rect;
        let (h, w) = (r.bottom - r.top, r.right - r.left);
        self.set_position(minus, 0.0, 0.0);
        self.set_position(plus, w - h, 0.0);
        self.set_position(slider, h, h);
        self.set_position(label, h + label_x_extra, h * -0.9);
    }

    /// `Spinner::setSize` 0x14024dbe0.
    pub fn spinner_set_size(&mut self, id: Id, w: f32, h: f32) {
        self.control_set_size(id, w, h);
        let s = self.spinner(id);
        let (plus, minus, slider, label) = (s.but_plus, s.but_minus, s.selection_slider, s.label);
        let r = self.controls[id].rect;
        let (hh, ww) = (r.bottom - r.top, r.right - r.left);
        self.set_size(minus, hh, hh);
        self.set_size(plus, hh, hh);
        self.set_size(label, (ww - (hh * 2.0)) - 2.0, hh * 0.9);
        self.set_size(slider, ww - (hh * 2.0), 5.0);
        self.set_position(id, r.left, r.top);
        self.spinner_place_children(id, 1.0);
    }

    /// `Spinner::setPosition` 0x14024db30 (the class' own, not a virtual function).
    pub fn spinner_set_position(&mut self, id: Id, x: f32, y: f32) {
        self.set_position(id, x, y);
        self.spinner_place_children(id, 1.0);
    }

    /// `Spinner::scaleByMult` 0x14024d800 with `resize` 0x14024d6b0.
    pub fn spinner_scale_by_mult(&mut self, id: Id, m: f32) {
        self.control_scale_by_mult(id, m);
        let s = self.spinner(id);
        let (plus, minus, slider, label) = (s.but_plus, s.but_minus, s.selection_slider, s.label);
        let r = self.controls[id].rect;
        let (hh, ww) = (r.bottom - r.top, r.right - r.left);
        self.set_size(minus, hh, hh);
        self.set_size(plus, hh, hh);
        self.set_size(label, (ww - (hh * 2.0)) - (m * 2.0), hh * 0.9);
        self.set_size(slider, ww - (hh * 2.0), m * 5.0);
        self.spinner_place_children(id, m);
    }

    /// `Spinner::onMouseDown` 0x14024ccd0.
    pub fn spinner_on_mouse_down(&mut self, id: Id, e: &MouseEvent) -> bool {
        let hit = self.control_on_mouse_down(id, e);
        if self.controls[id].is_mouse_down {
            let s = self.spinner_mut(id);
            s.pression_counter = if s.homogeneous_pressing_delay { 0.35 } else { 0.0 };
        }
        hit
    }

    /// `Spinner::onMouseMove` 0x14024cd10: the help bubble shows while the mouse is on the label.
    pub fn spinner_on_mouse_move(&mut self, id: Id, e: &MouseEvent) {
        let s = self.spinner(id);
        let (label, pop_over, display) = (s.label, s.pop_over, s.display_popover);
        let l = self.controls[label].rect;
        let p = self.local_to_world(id, [l.left, l.top]);
        let (x, y) = (e.x as f32, e.y as f32);
        let show = display && x > p[0] && x < (l.right - l.left) + p[0] && y > p[1] && y < (l.bottom - l.top) + p[1];
        self.set_visible(pop_over, show);
    }

    /// `Spinner::render` 0x14024cdd0.
    pub fn spinner_render(&mut self, id: Id, dt: f32, painter: &mut dyn Painter) {
        let (plus, minus, read_only) = {
            let s = self.spinner(id);
            (s.but_plus, s.but_minus, s.is_read_only)
        };
        self.set_visible(plus, !read_only);
        self.set_visible(minus, !read_only);
        self.control_render(id, dt, painter);
        // a held button: again after half a second, then every 0.15 s
        let s = self.spinner_mut(id);
        s.pression_counter = dt + s.pression_counter;
        if s.counter < 0.15 {
            s.counter = dt + s.counter;
        }
        let pression = s.pression_counter;
        let last = s.last_on_control_clicked;
        let target = if self.controls[plus].is_mouse_down && pression > 0.5 {
            Some(plus)
        } else if self.controls[minus].is_mouse_down && pression > 0.5 {
            Some(minus)
        } else {
            None
        };
        if let Some(target) = target {
            self.fire_clicked(target, last);
            self.spinner_mut(id).pression_counter = 0.35;
        }
        let s = self.spinner(id);
        let text = if s.items.is_empty() { format!("{} {}", crate::acos::format_g6(s.value as f32 * s.display_value_mult), s.units) } else { s.items.get(s.value as usize).cloned().unwrap_or_default() };
        self.controls[id].text = text;
        let s = self.spinner(id);
        let (label, slider, dressed, sm) = (s.label, s.selection_slider, s.dressed, self.controls[id].scale_mult);
        if !s.display_label {
            self.set_visible(label, false);
        }
        let gl = self.controls[id].gl;
        if dressed {
            let font = self.controls[id].font.unwrap_or(self.font);
            self.fonts[font].set_color4(&[0.0, 0.0, 0.0, 1.0]);
            let s = self.spinner(id);
            painter.set_texture(0, &s.value_back);
            painter.gl_color4f(gl, 1.0, 1.0, 1.0, 1.0);
            let mut world = self.controls[id].rect;
            self.get_world_rect(id, &mut world);
            if s.display_central_value {
                let r = self.controls[id].rect;
                let (h, w) = (r.bottom - r.top, r.right - r.left);
                painter.gl_quad(gl, (h + world.left) + sm, world.top, (w - h * 2.0) - sm * 2.0, h, true);
                let x = (world.right - world.left) * 0.5 + world.left;
                if s.items.is_empty() {
                    let text = self.controls[id].text.clone();
                    self.fonts[font].blit_string(painter, x, world.top, &text, sm * 1.2, ALIGN_CENTER);
                } else {
                    let item = s.items.get(s.value as usize).cloned().unwrap_or_default();
                    let max = match self.controls[label].kind {
                        crate::control::Kind::Label { max_chars } => max_chars as usize,
                        _ => 0,
                    };
                    let units: Vec<u16> = item.encode_utf16().collect();
                    let shown = if max != 0 && max < units.len() { String::from_utf16_lossy(&units[..max]) + "..." } else { item };
                    self.fonts[font].scale = s.text_size;
                    self.fonts[font].blit_string(painter, x, world.top, &shown, sm, ALIGN_CENTER);
                }
            }
            self.controls[id].text = String::new();
        }
        let s = self.spinner(id);
        if s.draw_underline {
            let mut world = self.controls[id].rect;
            self.get_world_rect(id, &mut world);
            painter.gl_color3f(gl, 1.0, 0.0, 0.0);
            let frac = s.value.wrapping_sub(s.min_value) as f32 / s.max_value.wrapping_sub(s.min_value) as f32;
            let mut knob = (world.right - world.left) - (world.bottom - world.top) * 2.0;
            let k = sm * 2.0;
            if dressed {
                let tw = sm * s.triangle_width;
                knob = ((knob - tw) - k) * frac + tw;
            } else {
                knob = (knob - k) * frac;
            }
            let sr = self.controls[slider].rect;
            let h = sr.bottom - sr.top;
            let r = self.controls[id].rect;
            let base = (r.bottom - r.top) + world.left;
            painter.gl_quad(gl, msvc_floor(base) + sm, msvc_floor(world.bottom), msvc_floor(knob), h, false);
            if dressed && knob != 0.0 && !knob.is_nan() {
                painter.gl_begin(gl, GL_TRIANGLES);
                painter.gl_vertex3f(gl, msvc_floor(base - sm * 9.0) + knob, world.bottom, 0.0);
                painter.gl_vertex3f(gl, msvc_floor((base + sm) + knob), world.bottom, 0.0);
                painter.gl_vertex3f(gl, msvc_floor((base - sm * 4.0) + knob), world.bottom - sm * 5.0, 0.0);
                painter.gl_end(gl);
            }
        }
    }

    // ----- CustomSpinner -----

    pub fn custom_spinner(&self, id: Id) -> &CustomSpinner {
        match self.widget3(id) {
            Widget3::CustomSpinner(s) => s,
            _ => panic!("control {id} is not a custom spinner"),
        }
    }

    pub fn custom_spinner_mut(&mut self, id: Id) -> &mut CustomSpinner {
        match self.widget3_mut(id) {
            Widget3::CustomSpinner(s) => s,
            _ => panic!("control {id} is not a custom spinner"),
        }
    }

    /// `CustomSpinner::CustomSpinner(name, gui, mode)` 0x14024ead0.
    pub fn new_custom_spinner(&mut self, painter: &mut dyn Painter, name: &str, mode: i32) -> Id {
        let id = self.new_control(painter, name);
        self.controls[id].draw_border = false;
        self.controls[id].draw_background = false;
        if mode == 1 {
            self.controls[id].back_texture = painter.get_texture("content\\gui\\controls\\customSpinner\\roundCentral.png");
        }
        let bar = if mode == 2 { painter.get_texture("content\\gui\\controls\\customSpinner\\bar.png") } else { Texture::default() };
        let mut buttons = [0; 4];
        for (slot, name) in buttons.iter_mut().zip(["LEFT_BUTTON", "RIGHT_BUTTON", "UP_BUTTON", "DOWN_BUTTON"]) {
            let button = self.new_control(painter, name);
            self.controls[button].draw_background = false;
            self.controls[button].draw_border = false;
            self.control_add_control(id, button);
            *slot = button;
        }
        let title = self.new_label(painter, "TITLE");
        self.set_text(title, "");
        self.set_position(title, 100.0, 0.0);
        self.controls[title].fore_color = [1.0; 4];
        self.control_add_control(id, title);
        self.set_widget3(id, Widget3::CustomSpinner(CustomSpinner { press_to_increment: true, left_button: buttons[0], right_button: buttons[1], up_button: buttons[2], down_button: buttons[3], title, bar, bar_position_normalized: 0.0, mode }));
        id
    }

    /// The buttons' places: `k` is the share of an arm of the round one.
    fn custom_spinner_layout(&mut self, id: Id, k: f32) {
        let s = self.custom_spinner(id);
        let (left, right, up, down, mode) = (s.left_button, s.right_button, s.up_button, s.down_button, s.mode);
        let r = self.controls[id].rect;
        let (w, h) = (r.right - r.left, r.bottom - r.top);
        let whole = |v: f32| v as i32 as f32;
        if mode == 1 {
            self.set_position(left, 0.0, 0.0);
            self.set_size(left, whole(w * k), whole(h));
            self.set_position(right, whole(msvc_floor(w - whole(w * k))), 0.0);
            self.set_size(right, whole(w * k), whole(h));
            self.set_position(up, 0.0, 0.0);
            self.set_size(up, whole(w), whole(h * k));
            self.set_position(down, 0.0, whole(h - (h * k)));
            self.set_size(down, whole(w), whole(h * k));
        } else if mode & !2 == 0 {
            self.set_position(left, 0.0, 0.0);
            self.set_size(left, h, h);
            self.set_position(right, msvc_floor(w - h), 0.0);
            self.set_size(right, h, h);
            self.set_position(up, 0.0, 0.0);
            self.set_size(up, w, w);
            self.set_position(down, 0.0, h - w);
            self.set_size(down, w, w);
        }
    }

    /// `CustomSpinner::setSize` 0x14024f6f0.
    pub fn custom_spinner_set_size(&mut self, id: Id, w: f32, h: f32) {
        self.control_set_size(id, w, h);
        self.custom_spinner_layout(id, f32::from_bits(0x3e8f_5c29));
    }

    /// `CustomSpinner::scaleByMult` 0x14024f6c0 with `resize` 0x14024f3c0.
    pub fn custom_spinner_scale_by_mult(&mut self, id: Id, m: f32) {
        self.control_scale_by_mult(id, m);
        let k = (self.controls[id].scale_mult as f64 * f64::from_bits(0x3fd1_eb85_1eb8_51ec)) as f32;
        self.custom_spinner_layout(id, k);
    }

    /// `CustomSpinner::render` 0x14024f0c0: a held button clicks every frame.
    pub fn custom_spinner_render(&mut self, id: Id, dt: f32, painter: &mut dyn Painter) {
        self.control_render(id, dt, painter);
        let s = self.custom_spinner(id);
        if s.press_to_increment {
            for button in [s.right_button, s.left_button, s.up_button, s.down_button] {
                if self.controls[button].is_mouse_down {
                    self.fire_clicked(button, Clicked { control: NO_CONTROL, local_x: 0, local_y: 0 });
                }
            }
        }
        let mut world = self.controls[id].rect;
        self.get_world_rect(id, &mut world);
        let s = self.custom_spinner(id);
        if s.mode == 2 {
            painter.set_texture(0, &s.bar);
            let texture_height = painter.texture_size(&s.bar).1 as f32;
            let r = self.controls[id].rect;
            let (w, h) = (r.right - r.left, r.bottom - r.top);
            let y = (((h - w * 2.0) - texture_height) * s.bar_position_normalized) + (w + world.top);
            painter.gl_quad(self.controls[id].gl, world.left, y, w, texture_height, true);
        }
    }
}
