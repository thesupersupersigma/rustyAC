// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! The widget set of ksGUI, part one: `ProgressBar` (0x188 bytes), `ActiveButton` (0x240),
//! `CheckBox` (0x1a8) and `Slider` (0x1c8).
//!
//! `ProgressBar::ProgressBar` 0x14024def0, `render` 0x14024dff0, `renderHorizontal`
//! 0x14024e030, `renderVertical` 0x14024e1e0; `ActiveButton::ActiveButton` 0x14024b160,
//! `onMouseMove` 0x14024b470, `onVisibleChanged` 0x14024b500, `render` 0x14024b510,
//! `setMultipleTexture` 0x14024b700, `setSelected` 0x14024ba80, `setText` 0x14024baa0;
//! `CheckBox::CheckBox` 0x14024a870, its box's handler 0x14024aaa0, `render` 0x14024abf0,
//! `setCheck` 0x14024ad40; `Slider::Slider` 0x14024f9f0, `onMouseDown` 0x1402501c0,
//! `onMouseMove` 0x140250310, `onMouseUp` 0x1402504a0, `render` 0x140250580, `resetMarkers`
//! 0x140250810, `setMarkerLeftPosition` 0x140250940, `setMarkerRightPosition` 0x1402509e0,
//! `setSize` 0x140250a70, `updateMarkers` 0x140250a90.

use std::rc::Rc;

use crate::control::{Clicked, Id, Kind, MouseEvent};
use crate::font::{Font, FontType};
use crate::gui::Gui;
use crate::painter::{Painter, Texture, ALIGN_LEFT, GL_QUADS};

/// What the widgets tell their listeners (the game's `Event<OnCheckBoxChanged>` and the like).
#[derive(Clone, Debug, PartialEq)]
pub enum WidgetEvent {
    /// `OnCheckBoxChanged`
    CheckBoxChanged { check_box: Id, value: bool },
    /// `OnSliderInteraction`
    SliderInteraction { slider: Id, value: f32 },
    /// `OnCutExtremesChanged`
    CutExtremesChanged { slider: Id, cut_in: f32, cut_out: f32 },
    /// `OnSpinnerValueChanged`
    SpinnerValueChanged { spinner: Id, value: i32 },
    /// `OnListBoxSelection` and the like: a row of a list
    ListBoxRow { list_box: Id, row: i32, kind: i32 },
    /// `OnScrollBarValueChanged`
    ScrollBarValueChanged { scroll_bar: Id, value: u32 },
    /// `OnTextInputValidated` and the like
    Text { control: Id, text: String, kind: i32 },
    /// `OnCellSelected` of a tab bar
    TabSelected { tab_bar: Id, old_index: u32, new_index: u32 },
}

pub type WidgetHandler = Rc<dyn Fn(&mut Gui, &WidgetEvent)>;

/// `ksgui::ProgressBar`.
pub struct ProgressBar {
    pub max_value: f32,
    pub min_value: f32,
    pub value: f32,
    pub is_vertical: bool,
    pub is_inverted: bool,
}

/// `ksgui::ActiveButton`: a button that knows the mouse is over it and whether it is selected.
pub struct ActiveButton {
    pub text_coord: [f32; 2],
    pub unselected_color: [f32; 4],
    pub selected_color: [f32; 4],
    pub roll_on_color: [f32; 4],
    pub inactive_color: [f32; 4],
    /// 0 idle, 2 the mouse is over it
    pub status: i32,
    pub blanked: bool,
    pub highlight_on_cursor_over: bool,
    pub is_active: bool,
    pub active_button_text: String,
    pub selected: bool,
    pub using_multiple_textures: bool,
    pub texture_on: Texture,
    pub texture_off: Texture,
}

/// `ksgui::CheckBox`.
pub struct CheckBox {
    pub handlers: Vec<WidgetHandler>,
    pub box_value: bool,
    pub box_control: Id,
    pub label: Id,
}

/// `ksgui::Slider`: the replay's time line, with its two cut markers.
pub struct Slider {
    pub handlers: Vec<WidgetHandler>,
    pub value: f32,
    pub m_pos: Id,
    pub marker_left: Id,
    pub marker_right: Id,
}

pub enum Widget {
    ProgressBar(ProgressBar),
    ActiveButton(ActiveButton),
    CheckBox(CheckBox),
    Slider(Slider),
    More(crate::widgets2::Widget2),
}

/// The compiler's inline `floor` of these functions: cut towards zero, one less for a negative
/// number that was not whole; a number past the integers stays as it is.
pub fn msvc_floor(x: f32) -> f32 {
    // cvttss2si gives 0x80000000 for what does not fit
    let t = if x.is_nan() || x >= 2_147_483_648.0 || x < -2_147_483_648.0 { i32::MIN } else { x as i32 };
    if t == i32::MIN {
        return x;
    }
    if t as f32 != x {
        (t - (x.to_bits() >> 31) as i32) as f32
    } else {
        t as f32
    }
}

impl Gui {
    pub fn widget(&self, id: Id) -> &Widget {
        match &self.controls[id].kind {
            Kind::Widget(w) => w,
            _ => panic!("control {id} is not a widget"),
        }
    }

    pub fn widget_mut(&mut self, id: Id) -> &mut Widget {
        match &mut self.controls[id].kind {
            Kind::Widget(w) => w,
            _ => panic!("control {id} is not a widget"),
        }
    }

    /// The widget's event: remembered in [`Gui::widget_events`] and handed to its handlers.
    pub fn fire_widget(&mut self, handlers: Vec<WidgetHandler>, event: WidgetEvent) {
        self.widget_events.push(event.clone());
        for handler in handlers {
            handler(self, &event);
        }
    }

    // ----- ProgressBar -----

    /// `ProgressBar::ProgressBar` 0x14024def0.
    pub fn new_progress_bar(&mut self, painter: &mut dyn Painter, name: &str) -> Id {
        let id = self.new_control(painter, name);
        let c = &mut self.controls[id];
        c.fore_color = [0.5, 0.5, 0.5, 1.0];
        c.border_color = [1.0; 4];
        c.back_color = [0.5, 0.5, 0.5, 1.0];
        c.draw_background = false;
        c.kind = Kind::Widget(Box::new(Widget::ProgressBar(ProgressBar { max_value: 1.0, min_value: 0.0, value: 0.5, is_vertical: true, is_inverted: false })));
        id
    }

    pub fn progress_bar_mut(&mut self, id: Id) -> &mut ProgressBar {
        match self.widget_mut(id) {
            Widget::ProgressBar(p) => p,
            _ => panic!("control {id} is not a progress bar"),
        }
    }

    /// `ProgressBar::render` 0x14024dff0 with `renderHorizontal` and `renderVertical`.
    fn progress_bar_render(&mut self, id: Id, dt: f32, painter: &mut dyn Painter) {
        let mut r = self.controls[id].rect;
        self.get_world_rect(id, &mut r);
        let (l, rr, t, b) = (r.left, r.right, r.top, r.bottom);
        let c = &self.controls[id];
        let (gl, fore) = (c.gl, c.fore_color);
        let Widget::ProgressBar(p) = self.widget(id) else { return };
        let raw = (p.value - p.min_value) / (p.max_value - p.min_value);
        let frac = if raw > 1.0 {
            1.0
        } else if raw >= 0.0 {
            raw
        } else {
            0.0
        };
        painter.gl_begin(gl, GL_QUADS);
        painter.gl_color4f(gl, fore[0], fore[1], fore[2], fore[3]);
        if p.is_vertical {
            let h = t - b;
            let y = (h * frac) + b;
            painter.gl_vertex3f(gl, l, y, 0.0);
            painter.gl_vertex3f(gl, l, b, 0.0);
            painter.gl_vertex3f(gl, rr, b, 0.0);
            painter.gl_vertex3f(gl, rr, y, 0.0);
        } else {
            let w = rr - l;
            if !p.is_inverted {
                painter.gl_vertex3f(gl, l, t, 0.0);
                painter.gl_vertex3f(gl, l, b, 0.0);
                let x = (w * frac) + l;
                painter.gl_vertex3f(gl, x, b, 0.0);
                painter.gl_vertex3f(gl, x, t, 0.0);
            } else {
                painter.gl_vertex3f(gl, rr, t, 0.0);
                let x = rr - (w * frac);
                painter.gl_vertex3f(gl, x, t, 0.0);
                painter.gl_vertex3f(gl, x, b, 0.0);
                painter.gl_vertex3f(gl, rr, b, 0.0);
            }
        }
        painter.gl_end(gl);
        self.control_render(id, dt, painter);
    }

    // ----- ActiveButton -----

    /// `ActiveButton::ActiveButton` 0x14024b160: its own font (Segoe UI, 10), no frame.
    pub fn new_active_button(&mut self, painter: &mut dyn Painter, name: &str) -> Id {
        let id = self.new_control(painter, name);
        self.fonts.push(Font::new(painter, "", FontType::Proportional, 10.0, false, false));
        let c = &mut self.controls[id];
        c.font = Some(self.fonts.len() - 1);
        c.draw_border = false;
        c.draw_background = false;
        c.kind = Kind::Widget(Box::new(Widget::ActiveButton(ActiveButton {
            text_coord: [0.0; 2],
            unselected_color: [0.6, 0.6, 0.6, 1.0],
            selected_color: [f32::from_bits(0x3f3d_b8bb), 0.0, 0.0, 1.0],
            roll_on_color: [1.0; 4],
            inactive_color: [0.7, 0.7, 0.7, 1.0],
            status: 0,
            blanked: false,
            highlight_on_cursor_over: true,
            is_active: true,
            active_button_text: String::new(),
            selected: false,
            using_multiple_textures: false,
            texture_on: Texture::default(),
            texture_off: Texture::default(),
        })));
        id
    }

    pub fn active_button(&self, id: Id) -> &ActiveButton {
        match self.widget(id) {
            Widget::ActiveButton(b) => b,
            _ => panic!("control {id} is not an active button"),
        }
    }

    pub fn active_button_mut(&mut self, id: Id) -> &mut ActiveButton {
        match self.widget_mut(id) {
            Widget::ActiveButton(b) => b,
            _ => panic!("control {id} is not an active button"),
        }
    }

    /// `ActiveButton::setSelected` 0x14024ba80.
    pub fn set_selected(&mut self, id: Id, selected: bool) {
        let b = self.active_button_mut(id);
        b.selected = selected;
        b.status = 0;
    }

    /// `ActiveButton::setMultipleTexture` 0x14024b700: `<base>_ON.png` and `<base>_OFF.png`.
    pub fn set_multiple_texture(&mut self, painter: &mut dyn Painter, id: Id, base: &str) {
        let (on, off) = (format!("{base}_ON.png"), format!("{base}_OFF.png"));
        let (on_ok, off_ok) = (painter.file_exists(&on), painter.file_exists(&off));
        if on_ok && off_ok {
            let texture_off = painter.get_texture(&off);
            let texture_on = painter.get_texture(&on);
            let b = self.active_button_mut(id);
            b.using_multiple_textures = true;
            b.texture_off = texture_off;
            b.texture_on = texture_on;
        } else {
            if !on_ok {
                println!("ActiveButton :: COULD NOT FIND :: {on}");
            }
            if !off_ok {
                println!("ActiveButton :: COULD NOT FIND :: {off}");
            }
            let texture_off = painter.get_texture("content/gui/icons/default_icon_OFF.png");
            let texture_on = painter.get_texture("content/gui/icons/default_icon_ON.png");
            let b = self.active_button_mut(id);
            b.using_multiple_textures = false;
            b.texture_off = texture_off;
            b.texture_on = texture_on;
        }
    }

    /// `ActiveButton::onMouseMove` 0x14024b470.
    fn active_button_on_mouse_move(&mut self, id: Id, e: &MouseEvent) {
        let mut r = self.controls[id].rect;
        self.get_world_rect(id, &mut r);
        let (mx, my) = (e.x as f32, e.y as f32);
        let b = self.active_button_mut(id);
        b.status = if b.highlight_on_cursor_over && mx > r.left && my < r.bottom && mx < r.right && my > r.top { 2 } else { 0 };
    }

    /// `ActiveButton::render` 0x14024b510.
    fn active_button_render(&mut self, id: Id, dt: f32, painter: &mut dyn Painter) {
        self.control_render(id, dt, painter);
        let mut r = self.controls[id].rect;
        self.get_world_rect(id, &mut r);
        let (l, rr, t, b) = (r.left, r.right, r.top, r.bottom);
        let c = &self.controls[id];
        let (gl, font, align, scale) = (c.gl, c.font.unwrap_or(self.font), c.font_align, c.scale_mult);
        let button = self.active_button(id);
        let textured;
        if !button.using_multiple_textures {
            if button.is_active {
                let color = if button.selected {
                    Some(button.selected_color)
                } else if button.status == 0 {
                    Some(button.unselected_color)
                } else if button.status == 2 {
                    Some(button.roll_on_color)
                } else {
                    None
                };
                if let Some(color) = color {
                    painter.gl_color4f(gl, color[0], color[1], color[2], color[3]);
                }
            } else {
                let color = button.inactive_color;
                painter.gl_color4f(gl, color[0], color[1], color[2], color[3]);
            }
            textured = c.back_texture.kid.is_some();
            if textured {
                painter.set_texture(0, &c.back_texture);
            }
        } else {
            let texture = if button.is_active {
                painter.gl_color4f(gl, 1.0, 1.0, 1.0, 1.0);
                if button.selected || button.status == 2 {
                    &button.texture_on
                } else {
                    &button.texture_off
                }
            } else {
                let color = button.inactive_color;
                painter.gl_color4f(gl, color[0], color[1], color[2], color[3]);
                &button.texture_off
            };
            painter.set_texture(0, texture);
            textured = true;
        }
        painter.gl_quad(gl, l, t, rr - l, b - t, textured);
        let color = if button.blanked { [0.5; 4] } else { [1.0; 4] };
        let x = ((rr - l) * 0.5) + l + button.text_coord[0];
        let y = t + button.text_coord[1];
        let text = button.active_button_text.clone();
        self.fonts[font].set_color4(&color);
        self.fonts[font].blit_string(painter, x, y, &text, scale, align);
    }

    // ----- CheckBox -----

    /// `CheckBox::CheckBox` 0x14024a870: a box and a label; a click on the box toggles it.
    pub fn new_check_box(&mut self, painter: &mut dyn Painter, name: &str) -> Id {
        let id = self.new_control(painter, name);
        self.control_set_size(id, 300.0, 25.0);
        self.controls[id].draw_border = false;
        self.controls[id].draw_background = false;
        let box_control = self.new_control(painter, "BOX");
        self.controls[box_control].back_color = [1.0; 4];
        self.control_add_control(id, box_control);
        let label = self.new_control(painter, name);
        self.set_text(label, name);
        self.controls[label].font_align = ALIGN_LEFT;
        self.controls[label].draw_background = false;
        self.controls[label].draw_border = false;
        self.control_add_control(id, label);
        self.controls[id].kind = Kind::Widget(Box::new(Widget::CheckBox(CheckBox { handlers: Vec::new(), box_value: false, box_control, label })));
        self.controls[box_control].ev_clicked.push(Rc::new(move |gui: &mut Gui, _: &Clicked| {
            let Widget::CheckBox(check) = gui.widget_mut(id) else { return };
            check.box_value = !check.box_value;
            let (value, handlers) = (check.box_value, check.handlers.clone());
            gui.fire_widget(handlers, WidgetEvent::CheckBoxChanged { check_box: id, value });
        }));
        id
    }

    pub fn check_box_mut(&mut self, id: Id) -> &mut CheckBox {
        match self.widget_mut(id) {
            Widget::CheckBox(c) => c,
            _ => panic!("control {id} is not a check box"),
        }
    }

    /// `CheckBox::setCheck` 0x14024ad40 (no event).
    pub fn set_check(&mut self, id: Id, value: bool) {
        let check = self.check_box_mut(id);
        check.box_value = value;
        let box_control = check.box_control;
        self.controls[box_control].back_color = if value { [1.0, 0.0, 0.0, 1.0] } else { [1.0; 4] };
    }

    /// `CheckBox::render` 0x14024abf0: the box is as high as the control, red when checked.
    fn check_box_render(&mut self, id: Id, dt: f32, painter: &mut dyn Painter) {
        let (value, box_control, label) = {
            let Widget::CheckBox(check) = self.widget(id) else { return };
            (check.box_value, check.box_control, check.label)
        };
        let r = self.controls[id].rect;
        let h = r.bottom - r.top;
        let bh = self.controls[box_control].rect.bottom - self.controls[box_control].rect.top;
        let y = (h * 0.5) - (bh * 0.5);
        self.set_position(box_control, 0.0, y);
        self.set_size(box_control, h, h);
        self.controls[box_control].back_color = if value { [1.0, 0.0, 0.0, 1.0] } else { [1.0; 4] };
        if !self.controls[id].text.is_empty() {
            let text = std::mem::take(&mut self.controls[id].text);
            self.set_text(label, &text);
        }
        let r = self.controls[id].rect;
        self.set_size(label, r.right - r.left, 25.0);
        self.set_position(label, (r.bottom - r.top) + 15.0, -5.0);
        self.controls[label].fore_color = self.controls[id].fore_color;
        self.control_render(id, dt, painter);
    }

    // ----- Slider -----

    /// `Slider::Slider(name, gui, withTextures)` 0x14024f9f0.
    pub fn new_slider(&mut self, painter: &mut dyn Painter, name: &str, with_textures: bool) -> Id {
        let id = self.new_control(painter, name);
        self.controls[id].draw_border = false;
        self.controls[id].draw_background = false;
        let marker_left = self.new_control(painter, "MARKER_LEFT");
        self.controls[marker_left].draw_background = false;
        self.controls[marker_left].draw_border = false;
        self.set_visible(marker_left, true);
        self.set_size(marker_left, 16.0, 15.0);
        self.controls[marker_left].border_color = [1.0; 4];
        self.control_add_control(id, marker_left);
        let marker_right = self.new_control(painter, "MARKER_RIGHT");
        self.controls[marker_right].draw_background = false;
        self.controls[marker_right].draw_border = false;
        self.set_visible(marker_right, true);
        self.controls[marker_right].border_color = [1.0; 4];
        self.set_size(marker_right, 16.0, 15.0);
        self.control_add_control(id, marker_right);
        let m_pos = self.new_control(painter, "POSITION_MARKER");
        self.controls[m_pos].draw_background = false;
        self.controls[m_pos].draw_border = false;
        self.set_size(m_pos, 14.0, 14.0);
        self.set_visible(m_pos, true);
        self.control_add_control(id, m_pos);
        self.controls[id].kind = Kind::Widget(Box::new(Widget::Slider(Slider { handlers: Vec::new(), value: 0.0, m_pos, marker_left, marker_right })));
        self.slider_reset_markers(id);
        // a marker that is clicked gets its frame: it is the one being dragged
        for marker in [marker_left, marker_right] {
            self.controls[marker].ev_clicked.push(Rc::new(|gui: &mut Gui, ev: &Clicked| gui.controls[ev.control].draw_border = true));
        }
        if with_textures {
            self.controls[m_pos].back_texture = painter.get_texture("content/gui/marker.png");
            self.controls[marker_left].back_texture = painter.get_texture("content/gui/replay/markerLeft.png");
            self.controls[marker_right].back_texture = painter.get_texture("content/gui/replay/markerRight.png");
        }
        id
    }

    pub fn slider(&self, id: Id) -> &Slider {
        match self.widget(id) {
            Widget::Slider(s) => s,
            _ => panic!("control {id} is not a slider"),
        }
    }

    pub fn slider_mut(&mut self, id: Id) -> &mut Slider {
        match self.widget_mut(id) {
            Widget::Slider(s) => s,
            _ => panic!("control {id} is not a slider"),
        }
    }

    /// `Slider::resetMarkers` 0x140250810: the markers at the two ends.
    pub fn slider_reset_markers(&mut self, id: Id) {
        let (left, right) = (self.slider(id).marker_left, self.slider(id).marker_right);
        let r = self.controls[id].rect;
        let h = r.bottom - r.top;
        let ml = self.controls[left].rect;
        let y = msvc_floor(h - (ml.bottom - ml.top));
        self.set_position(left, msvc_floor(-(ml.right - ml.left)), y);
        let mr = self.controls[right].rect;
        let y = msvc_floor(h - (mr.bottom - mr.top));
        self.set_position(right, msvc_floor(r.right - r.left), y);
    }

    /// `Slider::setMarkerLeftPosition` 0x140250940.
    pub fn slider_set_marker_left_position(&mut self, id: Id, f: f32) {
        let left = self.slider(id).marker_left;
        let r = self.controls[id].rect;
        let ml = self.controls[left].rect;
        let y = msvc_floor((r.bottom - r.top) - (ml.bottom - ml.top));
        self.set_position(left, msvc_floor(((r.right - r.left) * f) - (ml.right - ml.left)), y);
    }

    /// `Slider::setMarkerRightPosition` 0x1402509e0.
    pub fn slider_set_marker_right_position(&mut self, id: Id, f: f32) {
        let right = self.slider(id).marker_right;
        let r = self.controls[id].rect;
        let mr = self.controls[right].rect;
        let y = msvc_floor((r.bottom - r.top) - (mr.bottom - mr.top));
        self.set_position(right, msvc_floor((r.right - r.left) * f), y);
    }

    /// `Slider::updateMarkers` 0x140250a90.
    pub fn slider_update_markers(&mut self, id: Id) {
        let (left, right) = (self.slider(id).marker_left, self.slider(id).marker_right);
        let r = self.controls[id].rect;
        let (h, w) = (r.bottom - r.top, r.right - r.left);
        let mr = self.controls[right].rect;
        let y = msvc_floor(h - (mr.bottom - mr.top));
        self.set_position(right, msvc_floor((mr.left / w) * w), y);
        let ml = self.controls[left].rect;
        let y = msvc_floor(h - (ml.bottom - ml.top));
        self.set_position(left, msvc_floor(ml.left), y);
    }

    /// `Slider::onMouseDown` 0x1402501c0: a click between the markers moves the time.
    fn slider_on_mouse_down(&mut self, id: Id, e: &MouseEvent) -> bool {
        self.control_on_mouse_down(id, e);
        let (left, right) = (self.slider(id).marker_left, self.slider(id).marker_right);
        let mut s = self.controls[id].rect;
        self.get_world_rect(id, &mut s);
        let mut ml = self.controls[left].rect;
        self.get_world_rect(left, &mut ml);
        let mut mr = self.controls[right].rect;
        self.get_world_rect(right, &mut mr);
        let (mx, my) = (e.x as f32, e.y as f32);
        if mx >= ml.right && mx <= mr.left && my > s.top - 5.0 && my < s.bottom + 5.0 {
            let q = self.world_to_local(id, [mx, my]);
            let r = self.controls[id].rect;
            let value = q[0] / (r.right - r.left);
            let slider = self.slider_mut(id);
            slider.value = value;
            let handlers = slider.handlers.clone();
            self.fire_widget(handlers, WidgetEvent::SliderInteraction { slider: id, value });
        }
        false
    }

    /// `Slider::onMouseMove` 0x140250310: a held marker follows the mouse.
    fn slider_on_mouse_move(&mut self, id: Id, e: &MouseEvent) {
        let (left, right) = (self.slider(id).marker_left, self.slider(id).marker_right);
        let q = self.world_to_local(id, [e.x as f32, e.y as f32]);
        if self.controls[left].draw_border {
            let ml = self.controls[left].rect;
            let mut x = q[0] - ((ml.right - ml.left) * 0.5);
            let hi = self.controls[right].rect.left - (ml.right - ml.left);
            let lo = -(ml.right - ml.left);
            if x > hi {
                x = hi;
            } else if !(x >= lo) {
                x = lo;
            }
            self.set_position(left, msvc_floor(x), ml.top);
        }
        if self.controls[right].draw_border {
            let mr = self.controls[right].rect;
            let mut x = q[0] - ((mr.right - mr.left) * 0.5);
            let r = self.controls[id].rect;
            let hi = r.right - r.left;
            let lo = self.controls[left].rect.right;
            if x > hi {
                x = hi;
            } else if !(x >= lo) {
                x = lo;
            }
            self.set_position(right, msvc_floor(x), mr.top);
        }
    }

    /// `Slider::onMouseUp` 0x1402504a0: the markers are let go; their places are told.
    fn slider_on_mouse_up(&mut self, id: Id) {
        let (left, right) = (self.slider(id).marker_left, self.slider(id).marker_right);
        if self.controls[left].draw_border || self.controls[right].draw_border {
            self.controls[left].draw_border = false;
            self.controls[right].draw_border = false;
            let r = self.controls[id].rect;
            let inv = 1.0 / (r.right - r.left);
            let cut_in = inv * self.controls[left].rect.right;
            let cut_out = inv * self.controls[right].rect.left;
            let handlers = self.slider(id).handlers.clone();
            self.fire_widget(handlers, WidgetEvent::CutExtremesChanged { slider: id, cut_in, cut_out });
        }
    }

    /// `Slider::render` 0x140250580.
    fn slider_render(&mut self, id: Id, dt: f32, painter: &mut dyn Painter) {
        let (value, m_pos, left, right) = {
            let s = self.slider(id);
            (s.value, s.m_pos, s.marker_left, s.marker_right)
        };
        let gl = self.controls[id].gl;
        if self.controls[id].back_texture.kid.is_some() {
            painter.set_texture(0, &self.controls[id].back_texture);
        }
        let mut w = self.controls[id].rect;
        self.get_world_rect(id, &mut w);
        let (l, rr, t, b) = (w.left, w.right, w.top, w.bottom);
        painter.gl_color4f(gl, 1.0, 1.0, 1.0, 1.0);
        painter.gl_quad(gl, l, t, rr - l, b - t, false);
        let r = self.controls[id].rect;
        let p = self.controls[m_pos].rect;
        let a = (r.right - r.left) * value;
        let d = ((p.bottom - p.top) * 0.5) - ((r.bottom - r.top) * 0.5);
        let x = msvc_floor(a - ((p.right - p.left) * 0.5));
        self.set_position(m_pos, x, -d);
        let p = self.controls[m_pos].rect;
        painter.gl_color4f(gl, 1.0, 0.0, 0.0, 1.0);
        painter.gl_quad(gl, l, t, ((p.right - p.left) * 0.5) + p.left, b - t, false);
        painter.gl_color4f(gl, 0.5, 0.5, 0.5, 0.75);
        painter.gl_quad(gl, l, t, self.controls[left].rect.right, b - t, false);
        painter.gl_color4f(gl, 0.5, 0.5, 0.5, 0.75);
        let mr_left = self.controls[right].rect.left;
        let r = self.controls[id].rect;
        painter.gl_quad(gl, mr_left + l, t, (r.right - r.left) - mr_left, b - t, false);
        self.control_render(id, dt, painter);
    }

    // ----- the virtual functions, by widget -----

    pub fn widget_render(&mut self, id: Id, dt: f32, painter: &mut dyn Painter) {
        match self.widget(id) {
            Widget::ProgressBar(_) => self.progress_bar_render(id, dt, painter),
            Widget::ActiveButton(_) => self.active_button_render(id, dt, painter),
            Widget::CheckBox(_) => self.check_box_render(id, dt, painter),
            Widget::Slider(_) => self.slider_render(id, dt, painter),
            Widget::More(_) => self.widget2_render(id, dt, painter),
        }
    }

    pub fn widget_set_size(&mut self, id: Id, w: f32, h: f32) {
        match self.widget(id) {
            Widget::Slider(_) => {
                self.control_set_size(id, w, h);
                self.slider_reset_markers(id);
            }
            Widget::More(_) => self.widget2_set_size(id, w, h),
            _ => self.control_set_size(id, w, h),
        }
    }

    pub fn widget_on_mouse_down(&mut self, id: Id, e: &MouseEvent) -> bool {
        match self.widget(id) {
            Widget::Slider(_) => self.slider_on_mouse_down(id, e),
            Widget::More(_) => self.widget2_on_mouse_down(id, e),
            _ => self.control_on_mouse_down(id, e),
        }
    }

    pub fn widget_on_mouse_up(&mut self, id: Id, e: &MouseEvent) {
        match self.widget(id) {
            Widget::Slider(_) => self.slider_on_mouse_up(id),
            Widget::More(_) => self.widget2_on_mouse_up(id, e),
            _ => self.control_on_mouse_up(id, e),
        }
    }

    pub fn widget_on_mouse_move(&mut self, id: Id, e: &MouseEvent) {
        match self.widget(id) {
            Widget::ActiveButton(_) => self.active_button_on_mouse_move(id, e),
            Widget::Slider(_) => self.slider_on_mouse_move(id, e),
            Widget::More(_) => self.widget2_on_mouse_move(id, e),
            _ => self.control_on_mouse_move(id, e),
        }
    }

    pub fn widget_on_mouse_wheel(&mut self, id: Id, e: &MouseEvent) {
        match self.widget(id) {
            Widget::More(_) => self.widget2_on_mouse_wheel(id, e),
            _ => self.control_on_mouse_wheel(id, e),
        }
    }

    pub fn widget_on_key_char(&mut self, id: Id, key: u32) {
        match self.widget(id) {
            Widget::More(_) => self.widget2_on_key_char(id, key),
            _ => self.control_on_key_char(id, key),
        }
    }

    pub fn widget_on_key_down(&mut self, id: Id, key: u32) {
        match self.widget(id) {
            Widget::More(_) => self.widget2_on_key_down(id, key),
            _ => self.control_on_key_down(id, key),
        }
    }

    pub fn widget_set_text(&mut self, id: Id, text: &str) {
        match self.widget_mut(id) {
            Widget::ActiveButton(b) => b.active_button_text = text.to_string(),
            Widget::More(_) => self.widget2_set_text(id, text),
            _ => self.controls[id].text = text.to_string(),
        }
    }

    pub fn widget_scale_by_mult(&mut self, id: Id, m: f32) {
        match self.widget(id) {
            Widget::More(_) => self.widget2_scale_by_mult(id, m),
            _ => self.control_scale_by_mult(id, m),
        }
    }

    pub fn widget_set_repeat_interval(&mut self, id: Id, v: f32) {
        match self.widget(id) {
            Widget::More(_) => self.widget2_set_repeat_interval(id, v),
            _ => self.controls[id].repeat_interval = v,
        }
    }

    pub fn widget_on_visible_changed(&mut self, id: Id, visible: bool) {
        match self.widget_mut(id) {
            Widget::ActiveButton(b) => b.status = 0,
            Widget::More(_) => self.widget2_on_visible_changed(id, visible),
            _ => self.control_on_visible_changed(id, visible),
        }
    }
}
