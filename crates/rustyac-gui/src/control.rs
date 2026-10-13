// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! `ksgui::Control` (0x178 bytes), the base of every widget, and `ksgui::Label`.
//!
//! `Control::Control` 0x140244150, `render` 0x140244b80, `hitTest` 0x140244730,
//! `getWorldRect` 0x1402446c0, `localToWorld` 0x140244770, `worldToLocal` 0x1402452b0,
//! `setPosition` 0x1402450a0, `setSize` 0x140245120, `resetBaseRect` 0x140244ed0,
//! `scaleByMult` 0x140244f50, `setVisible` 0x1402451a0, `stepRepeatInterval` 0x1402451c0,
//! the mouse and key handlers 0x140244890 … 0x140244b10, `addControl` 0x140244690;
//! `Label::Label` 0x14024ad70, `Label::render` 0x14024afc0.
//!
//! The game's objects point at each other; here every control lives in the [`Gui`]'s list and
//! is named by its index ([`Id`]). What a subclass overrides (the game's virtual functions) is
//! chosen by [`Kind`] in the `Gui`'s methods of the same names.

use std::rc::Rc;

use crate::gui::Gui;
use crate::painter::{GlId, Painter, Rect, Texture, ALIGN_CENTER, ALIGN_LEFT, GL_LINE_STRIP, GL_QUADS};

/// A control of a [`Gui`].
pub type Id = usize;

/// The "no control" some handlers are called with (a null pointer in the game).
pub const NO_CONTROL: Id = usize::MAX;

/// `OnControlClicked`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Clicked {
    pub control: Id,
    pub local_x: i32,
    pub local_y: i32,
}

/// `MouseEvent` (and `OnMouseWheelMovedEvent` with its `delta`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct MouseEvent {
    pub x: i32,
    pub y: i32,
    pub button: i32,
    pub delta: f32,
}

/// A handler of `Event<OnControlClicked>`.
pub type ClickHandler = Rc<dyn Fn(&mut Gui, &Clicked)>;

/// What a control is besides a `ksgui::Control`: the members its class adds.
pub enum Kind {
    Control,
    /// `ksgui::Label`: `maxNumberOfCharDisplayed`
    Label { max_chars: u32 },
    Form(Box<crate::form::FormData>),
    GameScreen(Box<crate::form::GameScreenData>),
    Taskbar(Box<crate::taskbar::TaskbarData>),
    TaskBarIcon(Box<crate::taskbar::IconData>),
    Widget(Box<crate::widgets::Widget>),
}

pub struct Control {
    pub name: String,
    pub rect: Rect,
    pub back_color: [f32; 4],
    pub border_color: [f32; 4],
    pub fore_color: [f32; 4],
    pub highlight_text_color: [f32; 4],
    pub highlight: bool,
    pub back_texture_color: [f32; 4],
    /// a font of its own (an index into [`Gui::fonts`]); none: the GUI's
    pub font: Option<usize>,
    pub controls: Vec<Id>,
    pub parent: Option<Id>,
    pub font_align: i32,
    pub ev_clicked: Vec<ClickHandler>,
    pub back_texture: Texture,
    pub draw_border: bool,
    pub draw_background: bool,
    pub repeat_interval: f32,
    pub font_scale: f32,
    pub text: String,
    pub is_highlight: bool,
    pub visible: bool,
    pub is_mouse_down: bool,
    pub interval_counter: f32,
    pub is_repeating: bool,
    pub scale_mult: f32,
    /// `controlGLR`: its own drawer of 12 vertices
    pub gl: GlId,
    pub rect_base: Rect,
    pub kind: Kind,
}

impl Gui {
    /// `ksgui::Control::Control` 0x140244150.
    pub fn new_control(&mut self, painter: &mut dyn Painter, name: &str) -> Id {
        let gl = painter.create_gl(12);
        let control = Control {
            name: name.to_string(),
            rect: Rect::default(),
            back_color: self.get_skin_color("CONTROL", "BACK"),
            border_color: self.get_skin_color("CONTROL", "BORDER"),
            fore_color: self.get_skin_color("CONTROL", "FORE"),
            highlight_text_color: [0.0; 4],
            highlight: false,
            back_texture_color: [1.0; 4],
            font: None,
            controls: Vec::new(),
            parent: None,
            font_align: ALIGN_CENTER,
            ev_clicked: Vec::new(),
            back_texture: Texture::default(),
            draw_border: true,
            draw_background: true,
            repeat_interval: 0.0,
            font_scale: 1.0,
            text: String::new(),
            is_highlight: false,
            visible: true,
            is_mouse_down: false,
            interval_counter: 0.0,
            is_repeating: false,
            scale_mult: 1.0,
            gl,
            rect_base: Rect::default(),
            kind: Kind::Control,
        };
        self.controls.push(control);
        self.controls.len() - 1
    }

    /// `ksgui::Label::Label` 0x14024ad70: the name is the text.
    pub fn new_label(&mut self, painter: &mut dyn Painter, name: &str) -> Id {
        let id = self.new_control(painter, name);
        let (back, border, fore) = (self.get_skin_color("LABEL", "BACK"), self.get_skin_color("LABEL", "BORDER"), self.get_skin_color("LABEL", "FORE"));
        let c = &mut self.controls[id];
        c.back_color = back;
        c.border_color = border;
        c.fore_color = fore;
        c.font_align = ALIGN_LEFT;
        c.text = name.to_string();
        c.kind = Kind::Label { max_chars: 0 };
        id
    }

    /// `Control::addControl` 0x140244690 (the base's; see [`Gui::add_control`] for the classes
    /// that have their own).
    pub fn control_add_control(&mut self, parent: Id, child: Id) {
        self.controls[parent].controls.push(child);
        self.controls[child].parent = Some(parent);
    }

    /// `Control::getWorldRect` 0x1402446c0: `r` (the control's own rectangle to begin with) moved
    /// by every ancestor's left and top.
    pub fn get_world_rect(&self, id: Id, r: &mut Rect) {
        let mut at = id;
        while let Some(parent) = self.controls[at].parent {
            let p = &self.controls[parent].rect;
            r.bottom = p.top + r.bottom;
            r.top = p.top + r.top;
            r.left = p.left + r.left;
            r.right = p.left + r.right;
            at = parent;
        }
    }

    /// `Control::localToWorld` 0x140244770.
    pub fn local_to_world(&self, id: Id, p: [f32; 2]) -> [f32; 2] {
        let c = &self.controls[id];
        let v = [c.rect.left + p[0], c.rect.top + p[1]];
        match c.parent {
            Some(parent) => self.local_to_world(parent, v),
            None => v,
        }
    }

    /// `Control::worldToLocal` 0x1402452b0.
    pub fn world_to_local(&self, id: Id, p: [f32; 2]) -> [f32; 2] {
        let c = &self.controls[id];
        let v = [p[0] - c.rect.left, p[1] - c.rect.top];
        match c.parent {
            Some(parent) => self.world_to_local(parent, v),
            None => v,
        }
    }

    /// `Control::hitTest` 0x140244730: a point of the control's own space.
    pub fn control_hit_test(&self, id: Id, x: i32, y: i32) -> bool {
        let r = &self.controls[id].rect;
        x >= 0 && x as f32 <= r.right - r.left && y >= 0 && y as f32 <= r.bottom - r.top
    }

    /// `Control::setPosition` 0x1402450a0.
    pub fn set_position(&mut self, id: Id, x: f32, y: f32) {
        let c = &mut self.controls[id];
        let right = (c.rect.right - c.rect.left) + x;
        let bottom = (c.rect.bottom - c.rect.top) + y;
        c.rect = Rect { left: x, right, top: y, bottom };
        if c.rect_base.left == 0.0 && c.rect_base.top == 0.0 {
            c.rect_base = c.rect;
        }
    }

    /// `Control::setSize` 0x140245120.
    pub fn control_set_size(&mut self, id: Id, w: f32, h: f32) {
        let c = &mut self.controls[id];
        let right = c.rect.left + w;
        let bottom = c.rect.top + h;
        c.rect.right = right;
        c.rect.bottom = bottom;
        let b = &c.rect_base;
        if (b.right == 0.0 && b.bottom == 0.0) || (b.right == b.left && b.top == b.bottom) {
            c.rect_base = c.rect;
        }
    }

    /// `Control::resetBaseRect` 0x140244ed0.
    pub fn reset_base_rect(&mut self, id: Id) {
        self.controls[id].rect_base = Rect::default();
        for child in self.controls[id].controls.clone() {
            self.reset_base_rect(child);
        }
    }

    /// `Control::setVisible` 0x1402451a0.
    pub fn set_visible(&mut self, id: Id, visible: bool) {
        if self.controls[id].visible != visible {
            self.controls[id].visible = visible;
            self.on_visible_changed(id, visible);
        }
    }

    /// `Control::onVisibleChanged` 0x140035ff0: told to every child (their own flags stay).
    pub fn control_on_visible_changed(&mut self, id: Id, visible: bool) {
        for child in self.controls[id].controls.clone() {
            self.on_visible_changed(child, visible);
        }
    }

    /// `Control::scaleByMult(float)` 0x140244f50.
    pub fn control_scale_by_mult(&mut self, id: Id, m: f32) {
        let b = self.controls[id].rect_base;
        let w = (b.right - b.left) * m;
        let h = (b.bottom - b.top) * m;
        self.controls[id].scale_mult = m;
        self.set_size(id, w, h);
        self.controls[id].font_scale = m;
        for child in self.controls[id].controls.clone() {
            self.scale_by_mult(child, m);
            let c = &mut self.controls[child];
            let left = c.rect_base.left * m;
            let right = (c.rect.right - c.rect.left) + left;
            let top = c.rect_base.top * m;
            let bottom = (c.rect.bottom - c.rect.top) + top;
            c.rect = Rect { left, right, top, bottom };
            if c.rect_base.left == 0.0 && c.rect_base.top == 0.0 {
                c.rect_base = c.rect;
            }
        }
    }

    /// Every handler of a control's `evClicked`, in the order they were added.
    pub fn fire_clicked(&mut self, id: Id, event: Clicked) {
        self.fired.push(event);
        for handler in self.controls[id].ev_clicked.clone() {
            handler(self, &event);
        }
    }

    /// `Control::stepRepeatInterval` 0x1402451c0: a control held down for more than a second
    /// clicks again every `repeatInterval` seconds.
    pub fn step_repeat_interval(&mut self, id: Id, dt: f32) {
        let c = &mut self.controls[id];
        if c.is_mouse_down && c.repeat_interval != 0.0 {
            c.interval_counter = dt + c.interval_counter;
            if !c.is_repeating {
                c.is_repeating = c.interval_counter > 1.0;
            }
            if c.interval_counter >= c.repeat_interval && c.is_repeating {
                self.fire_clicked(id, Clicked { control: id, local_x: 0, local_y: 0 });
                self.controls[id].interval_counter = 0.0;
            }
        } else {
            c.interval_counter = 0.0;
            c.is_repeating = false;
        }
    }

    /// `Control::render` 0x140244b80.
    pub fn control_render(&mut self, id: Id, dt: f32, painter: &mut dyn Painter) {
        self.step_repeat_interval(id, dt);
        let font = self.controls[id].font.unwrap_or(self.font);
        let mut r = self.controls[id].rect;
        self.get_world_rect(id, &mut r);
        let (l, rr, t, b) = (r.left, r.right, r.top, r.bottom);
        let c = &self.controls[id];
        let gl = c.gl;
        if c.draw_background {
            painter.gl_begin(gl, GL_QUADS);
            painter.gl_color4f(gl, c.back_color[0], c.back_color[1], c.back_color[2], c.back_color[3]);
            painter.gl_vertex3f(gl, l, t, 0.0);
            painter.gl_vertex3f(gl, l, b, 0.0);
            painter.gl_vertex3f(gl, rr, b, 0.0);
            painter.gl_vertex3f(gl, rr, t, 0.0);
            painter.gl_end(gl);
        }
        if c.back_texture.kid.is_some() {
            painter.set_texture(0, &c.back_texture);
            painter.gl_color4f(gl, c.back_texture_color[0], c.back_texture_color[1], c.back_texture_color[2], c.back_texture_color[3]);
            painter.gl_quad(gl, l, t, rr - l, b - t, true);
        }
        if c.draw_border {
            painter.gl_begin(gl, GL_LINE_STRIP);
            painter.gl_color4f(gl, c.border_color[0], c.border_color[1], c.border_color[2], c.border_color[3]);
            painter.gl_vertex3f(gl, l, t, 0.0);
            painter.gl_vertex3f(gl, l, b, 0.0);
            painter.gl_vertex3f(gl, rr, b, 0.0);
            painter.gl_vertex3f(gl, rr, t, 0.0);
            painter.gl_vertex3f(gl, l, t, 0.0);
            painter.gl_end(gl);
        }
        let color = if c.highlight && c.is_highlight { c.highlight_text_color } else { c.fore_color };
        let (align, scale) = (c.font_align, c.font_scale);
        let x = match align {
            0 => Some(l),
            1 => Some(rr),
            2 => Some((rr - l) * 0.5 + l),
            _ => None,
        };
        self.fonts[font].set_color(color[0], color[1], color[2], color[3]);
        if let Some(x) = x {
            let text = self.controls[id].text.clone();
            self.fonts[font].blit_string(painter, x, t, &text, scale, align);
        }
        for child in self.controls[id].controls.clone() {
            if self.controls[child].visible {
                self.render(child, dt, painter);
            }
        }
    }

    /// `Label::render` 0x14024afc0: a text longer than `maxNumberOfCharDisplayed` is drawn cut,
    /// with three dots.
    pub fn label_render(&mut self, id: Id, dt: f32, painter: &mut dyn Painter, max_chars: u32) {
        let saved = self.controls[id].text.clone();
        let units: Vec<u16> = saved.encode_utf16().collect();
        if max_chars != 0 && units.len() > max_chars as usize {
            self.controls[id].text = String::from_utf16_lossy(&units[..max_chars as usize]) + "...";
        }
        self.control_render(id, dt, painter);
        self.controls[id].text = saved;
    }

    /// `Control::onMouseDown` 0x140244890: the children first, then the control itself; a hit
    /// fires `evClicked`.
    pub fn control_on_mouse_down(&mut self, id: Id, e: &MouseEvent) -> bool {
        if !self.controls[id].visible {
            return false;
        }
        for child in self.controls[id].controls.clone() {
            if self.controls[child].visible {
                self.on_mouse_down(child, e);
            }
        }
        let q = self.world_to_local(id, [e.x as f32, e.y as f32]);
        let (lx, ly) = (q[0] as i32, q[1] as i32);
        if self.hit_test(id, lx, ly) {
            self.fire_clicked(id, Clicked { control: id, local_x: lx, local_y: ly });
            self.controls[id].is_mouse_down = true;
            return true;
        }
        false
    }

    /// `Control::onMouseMove` 0x1402449b0.
    pub fn control_on_mouse_move(&mut self, id: Id, e: &MouseEvent) {
        if !self.controls[id].visible {
            return;
        }
        if self.controls[id].highlight {
            let q = self.world_to_local(id, [e.x as f32, e.y as f32]);
            self.controls[id].is_highlight = self.hit_test(id, q[0] as i32, q[1] as i32);
        }
        for child in self.controls[id].controls.clone() {
            if self.controls[child].visible {
                self.on_mouse_move(child, e);
            }
        }
    }

    /// `Control::onMouseUp` 0x140244a90.
    pub fn control_on_mouse_up(&mut self, id: Id, e: &MouseEvent) {
        self.controls[id].is_mouse_down = false;
        for child in self.controls[id].controls.clone() {
            self.on_mouse_up(child, e);
        }
    }

    /// `Control::onMouseWheelMovedEvent` 0x140244b10.
    pub fn control_on_mouse_wheel(&mut self, id: Id, e: &MouseEvent) {
        for child in self.controls[id].controls.clone() {
            self.on_mouse_wheel(child, e);
        }
    }

    /// `Control::onKeyChar` 0x1402447d0.
    pub fn control_on_key_char(&mut self, id: Id, key: u32) {
        if self.controls[id].visible {
            for child in self.controls[id].controls.clone() {
                self.on_key_char(child, key);
            }
        }
    }

    /// `Control::onKeyDown` 0x140244830.
    pub fn control_on_key_down(&mut self, id: Id, key: u32) {
        if self.controls[id].visible {
            for child in self.controls[id].controls.clone() {
                self.on_key_down(child, key);
            }
        }
    }
}
