// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! The widget set of ksGUI, part two: `ScrollBar` (0x1c0 bytes) with its `MovingBar` (0x200),
//! `TextBox` (0x190), `TextInput` (0x1d0), `PopOver` (0x188), `ConnectedLabel` (0x1d8),
//! `TabBar` (0x200) and `DefaultSystemMessage` (0x1b0).
//!
//! `ScrollBar::ScrollBar` 0x14025ae60, `drawArrow` 0x14025b5d0, `onMouseMove` 0x14025ba30,
//! `onMouseWheelMovedEvent` 0x14025bae0, `render` 0x14025bd60, `setItemsNumber` 0x14025c0a0,
//! `setItemsPerPage` 0x14025c0b0, `setRepeatInterval` 0x14025c0d0, `setSize` 0x14025c0f0,
//! `setValue` 0x14025c150; `MovingBar::MovingBar` 0x14025ab60, `onMouseDown` 0x14025b800,
//! `onMouseMove` 0x14025b8b0, `onMouseUp` 0x14025bac0, `render` 0x14025bbd0;
//! `TextBox::TextBox` 0x14024e330, `onMouseDown` 0x14024e590, `render` 0x14024e5a0,
//! `setFormattedText` 0x14024e6e0; `TextInput::TextInput` 0x140245310, `onKeyChar`
//! 0x1402455d0, `onMouseDown` 0x1402456e0, `render` 0x140245700, `validateText` 0x140245820;
//! `PopOver::PopOver` 0x140255700, `render` 0x140255a20, `renderPopOver` 0x140255a70,
//! `setLabelTitle` 0x140255cb0, `setText` 0x140255d20; `ConnectedLabel::ConnectedLabel`
//! 0x14044e800 / 0x14044e8a0, `init` 0x14044ea00, `render` 0x14044ebb0; `TabBar::TabBar`
//! 0x140257990, its arrows' handlers 0x140257de0 / 0x140257ec0, `addTab` 0x140258120,
//! `onMouseMove` 0x140258340, `onTabClicked` 0x140258360, `orderAlphabetically` 0x140258500,
//! `render` 0x140258530, `updateRollOnIndex` 0x140258750, `updateTabBarPosition` 0x140258810,
//! `updateTabs` 0x1402588b0; `DefaultSystemMessage::DefaultSystemMessage` 0x14025a4a0,
//! `render` 0x14025a8d0, `setMessage` 0x14025aab0.

use std::rc::Rc;

use crate::control::{Clicked, Id, Kind, MouseEvent, NO_CONTROL};
use crate::font::{Font, FontType};
use crate::gui::Gui;
use crate::painter::{Painter, Texture, ALIGN_CENTER, ALIGN_LEFT, GL_LINE_STRIP, GL_TRIANGLES};
use crate::widgets::{Widget, WidgetEvent, WidgetHandler};

/// `ksgui::ScrollBar`: two buttons and a thumb; "minValue" is the items of a page.
pub struct ScrollBar {
    pub handlers: Vec<WidgetHandler>,
    pub but_plus: Id,
    pub but_minus: Id,
    pub dressed: bool,
    pub min_value: u32,
    pub max_value: u32,
    pub step: u32,
    pub value: u32,
    pub bar: Id,
}

/// `ksgui::MovingBar`: the thumb of a scroll bar.
pub struct MovingBar {
    pub scroll_bar: Id,
    pub upper_texture: Texture,
    pub lower_texture: Texture,
    pub middle_texture: Texture,
    pub pressing_point: [f32; 2],
}

/// `ksgui::TextBox`: lines of text, one under the other.
pub struct TextBox {
    pub text_lines: Vec<String>,
}

/// `ksgui::TextInput`: a line the keyboard writes into while it has the focus.
pub struct TextInput {
    pub back_color_focus: [f32; 4],
    pub back_color_unfocus: [f32; 4],
    /// `evValidate` (kind 0) and `evKeyDown` (kind 1)
    pub handlers: Vec<WidgetHandler>,
    pub has_focus: bool,
    pub max_number_of_char: u32,
}

/// `ksgui::PopOver`: the bubble that explains a spinner.
pub struct PopOver {
    pub text_box: Id,
    pub title: Id,
}

/// What a `ConnectedLabel` shows: the game reads it through a pointer every frame; here its
/// owner writes it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Connected {
    Float(f32),
    Int(i32),
}

/// `ksgui::ConnectedLabel`: "label value suffix", rebuilt every frame.
pub struct ConnectedLabel {
    pub value: Connected,
    pub label: String,
    pub suffix: String,
    pub precision: i32,
}

/// `TabBar`: a strip of tabs that slides the chosen one to its place.
pub struct TabBar {
    pub handlers: Vec<WidgetHandler>,
    pub tab_bar_x: f32,
    /// the chosen tab
    pub top_index: u32,
    /// the tab under the mouse
    pub roll_on_index: u32,
    pub left_scroller: Id,
    pub right_scroller: Id,
    pub selected_color: [f32; 4],
    pub unselected_color: [f32; 4],
    pub rollover_color: [f32; 4],
    pub elements: Vec<Id>,
    pub mouse: [i32; 2],
}

/// `DefaultSystemMessage`: the banner of two lines the HUD's messages show in.
pub struct DefaultSystemMessage {
    pub title: Id,
    pub description: Id,
    pub bar: Texture,
}

pub enum Widget2 {
    ScrollBar(ScrollBar),
    MovingBar(MovingBar),
    TextBox(TextBox),
    TextInput(TextInput),
    PopOver(PopOver),
    ConnectedLabel(ConnectedLabel),
    TabBar(TabBar),
    DefaultSystemMessage(DefaultSystemMessage),
    More(crate::widgets3::Widget3),
}

/// A number as a stream with `std::fixed` and that precision writes it: the exact decimal
/// digits of the value, rounded half away from zero. Infinities and NaNs are MSVC's "1.#INF",
/// "-1.#IND", "1.#QNAN" with the characters after the point rounded like digits ("-1.#IO" at 3).
pub fn format_fixed(value: f64, precision: usize) -> String {
    if !value.is_finite() {
        let text = crate::acos::format_g6(value as f32);
        let (whole, mantissa) = text.split_once('.').unwrap_or((&text, ""));
        let mut digits: Vec<u8> = mantissa.bytes().chain(std::iter::repeat(b'0')).take(precision + 1).collect();
        let next = digits.pop().unwrap_or(b'0');
        let mut out = whole.to_string();
        if next >= b'5' {
            match digits.last_mut() {
                Some(d) => *d += 1,
                None => out = out.replace('1', "2"),
            }
        }
        if precision > 0 {
            out.push('.');
            out.push_str(std::str::from_utf8(&digits).unwrap_or(""));
        }
        return out;
    }
    let exact = format!("{:.330}", value.abs());
    let (whole, fraction) = exact.split_once('.').unwrap_or((&exact, ""));
    let mut digits: Vec<u8> = whole.bytes().chain(fraction.bytes().take(precision)).map(|b| b - b'0').collect();
    if fraction.as_bytes().get(precision).is_some_and(|d| *d >= b'5') {
        let mut i = digits.len();
        loop {
            if i == 0 {
                digits.insert(0, 1);
                break;
            }
            i -= 1;
            if digits[i] == 9 {
                digits[i] = 0;
            } else {
                digits[i] += 1;
                break;
            }
        }
    }
    let split = digits.len() - precision;
    let mut out = String::new();
    if value.is_sign_negative() {
        out.push('-');
    }
    out.extend(digits[..split].iter().map(|d| (b'0' + d) as char));
    if precision > 0 {
        out.push('.');
        out.extend(digits[split..].iter().map(|d| (b'0' + d) as char));
    }
    out
}

impl Gui {
    pub fn widget2(&self, id: Id) -> &Widget2 {
        match self.widget(id) {
            Widget::More(w) => w,
            _ => panic!("control {id} is not a widget of the second part"),
        }
    }

    pub fn widget2_mut(&mut self, id: Id) -> &mut Widget2 {
        match self.widget_mut(id) {
            Widget::More(w) => w,
            _ => panic!("control {id} is not a widget of the second part"),
        }
    }

    fn set_widget2(&mut self, id: Id, widget: Widget2) {
        self.controls[id].kind = Kind::Widget(Box::new(Widget::More(widget)));
    }

    // ----- ScrollBar and MovingBar -----

    pub fn scroll_bar(&self, id: Id) -> &ScrollBar {
        match self.widget2(id) {
            Widget2::ScrollBar(s) => s,
            _ => panic!("control {id} is not a scroll bar"),
        }
    }

    pub fn scroll_bar_mut(&mut self, id: Id) -> &mut ScrollBar {
        match self.widget2_mut(id) {
            Widget2::ScrollBar(s) => s,
            _ => panic!("control {id} is not a scroll bar"),
        }
    }

    fn moving_bar_mut(&mut self, id: Id) -> &mut MovingBar {
        match self.widget2_mut(id) {
            Widget2::MovingBar(m) => m,
            _ => panic!("control {id} is not a moving bar"),
        }
    }

    /// Where the thumb goes when the bar is made or sized: under the first button.
    fn scroll_bar_place_bar(&mut self, id: Id) {
        let s = self.scroll_bar(id);
        let (bar, offset) = (s.bar, s.value.wrapping_sub(s.min_value).wrapping_mul(s.step));
        let r = self.controls[id].rect;
        self.set_position(bar, 2.0, (r.right - r.left) + offset as f32);
    }

    /// `ScrollBar::ScrollBar(name, gui, dressed)` 0x14025ae60.
    pub fn new_scroll_bar(&mut self, painter: &mut dyn Painter, name: &str, dressed: bool) -> Id {
        let id = self.new_control(painter, name);
        let but_plus = self.new_control(painter, "PLUS");
        self.set_text(but_plus, "+");
        let but_minus = self.new_control(painter, "MINUS");
        self.set_text(but_minus, "-");
        // MovingBar::MovingBar 0x14025ab60
        let bar = self.new_control(painter, "MOVING BAR");
        let upper_texture = painter.get_texture("content/gui/controls/scrollbar/rectangleUp.png");
        let middle_texture = painter.get_texture("content/gui/controls/scrollbar/rectangleMiddle.png");
        let lower_texture = painter.get_texture("content/gui/controls/scrollbar/rectangleDown.png");
        self.set_widget2(bar, Widget2::MovingBar(MovingBar { scroll_bar: id, upper_texture, lower_texture, middle_texture, pressing_point: [0.0; 2] }));
        self.controls[bar].draw_border = false;
        self.controls[bar].is_mouse_down = false;
        self.control_set_size(bar, 50.0, 50.0);
        self.controls[bar].back_color = [1.0, 0.0, 0.0, 0.6];
        self.set_widget2(id, Widget2::ScrollBar(ScrollBar { handlers: Vec::new(), but_plus, but_minus, dressed, min_value: 0, max_value: 10, step: 1, value: 5, bar }));
        self.control_add_control(id, but_minus);
        self.control_add_control(id, but_plus);
        self.control_add_control(id, bar);
        self.controls[but_minus].ev_clicked.push(Rc::new(move |gui: &mut Gui, _: &Clicked| {
            let s = gui.scroll_bar(id);
            gui.scroll_bar_set_value(id, s.value.wrapping_sub(s.step));
        }));
        self.controls[but_plus].ev_clicked.push(Rc::new(move |gui: &mut Gui, _: &Clicked| {
            let s = gui.scroll_bar(id);
            gui.scroll_bar_set_value(id, s.value.wrapping_add(s.step));
        }));
        if dressed {
            self.controls[id].draw_border = false;
            self.controls[id].draw_background = false;
            for (button, file) in [(but_minus, "content/gui/controls/scrollbar/scrollbarArrowUp.png"), (but_plus, "content/gui/controls/scrollbar/scrollbarArrowDown.png")] {
                self.controls[button].back_texture = painter.get_texture(file);
                self.controls[button].draw_background = false;
                self.controls[button].draw_border = false;
                self.set_text(button, "");
            }
        }
        self.control_set_size(id, 300.0, 25.0);
        self.scroll_bar_place_bar(id);
        id
    }

    /// `ScrollBar::setValue` 0x14025c150: kept between the page's size and the number of items;
    /// the listeners hear of it every time.
    pub fn scroll_bar_set_value(&mut self, id: Id, value: u32) {
        let s = self.scroll_bar_mut(id);
        let v = if value > s.max_value {
            s.max_value
        } else if value < s.min_value {
            s.min_value
        } else {
            value
        };
        s.value = v;
        let handlers = s.handlers.clone();
        self.fire_widget(handlers, WidgetEvent::ScrollBarValueChanged { scroll_bar: id, value: v });
    }

    /// `ScrollBar::setItemsNumber` 0x14025c0a0.
    pub fn scroll_bar_set_items_number(&mut self, id: Id, n: u32) {
        self.scroll_bar_mut(id).max_value = n;
    }

    /// `ScrollBar::setItemsPerPage` 0x14025c0b0.
    pub fn scroll_bar_set_items_per_page(&mut self, id: Id, n: u32) {
        let s = self.scroll_bar_mut(id);
        s.min_value = n;
        if n <= s.max_value {
            s.max_value = n.wrapping_add(1);
        }
    }

    /// `ScrollBar::setRepeatInterval` 0x14025c0d0: the two buttons repeat while held.
    pub fn scroll_bar_set_repeat_interval(&mut self, id: Id, v: f32) {
        let (plus, minus) = (self.scroll_bar(id).but_plus, self.scroll_bar(id).but_minus);
        self.controls[plus].repeat_interval = v;
        self.controls[minus].repeat_interval = v;
    }

    /// `ScrollBar::drawArrow` 0x14025b5d0: a white triangle over a button (0 left, 1 right, 2 up,
    /// 3 down).
    fn scroll_bar_draw_arrow(&mut self, id: Id, button: Id, direction: i32, painter: &mut dyn Painter) {
        self.set_text(button, "");
        let gl = self.controls[id].gl;
        painter.gl_begin(gl, GL_TRIANGLES);
        painter.gl_color4f(gl, 1.0, 1.0, 1.0, 1.0);
        let c = self.controls[button].rect;
        let mut r = c;
        self.get_world_rect(button, &mut r);
        let (l, rr, t, b) = (r.left, r.right, r.top, r.bottom);
        let mid_y = ((c.bottom - c.top) * 0.5) + t;
        let mid_x = ((c.right - c.left) * 0.5) + l;
        let points = match direction {
            0 => Some([(rr, t), (l, mid_y), (rr, b)]),
            1 => Some([(l, t), (l, b), (rr, mid_y)]),
            2 => Some([(l, b), (mid_x, t), (rr, b)]),
            3 => Some([(l, t), (mid_x, b), (rr, t)]),
            _ => None,
        };
        for (x, y) in points.into_iter().flatten() {
            painter.gl_vertex3f(gl, x, y, 0.0);
        }
        painter.gl_end(gl);
    }

    /// `ScrollBar::render` 0x14025bd60.
    fn scroll_bar_render(&mut self, id: Id, dt: f32, painter: &mut dyn Painter) {
        let (bar, plus, minus, dressed, min_value, max_value, value) = {
            let s = self.scroll_bar(id);
            (s.bar, s.but_plus, s.but_minus, s.dressed, s.min_value, s.max_value, s.value)
        };
        let r = self.controls[id].rect;
        let (w, h) = (r.right - r.left, r.bottom - r.top);
        let track = h - (w * 2.0);
        self.set_size(bar, w - 5.0, (min_value as f32 / max_value as f32) * track);
        let unit = track / max_value as f32;
        self.set_position(bar, 2.0, (value.wrapping_sub(min_value) as f32 * unit) + w);
        self.set_position(minus, 0.0, 0.0);
        self.set_size(minus, w - 1.0, w);
        self.set_position(plus, 0.0, h - w);
        self.set_size(plus, w - 1.0, w);
        let mut world = self.controls[id].rect;
        self.get_world_rect(id, &mut world);
        let gl = self.controls[id].gl;
        painter.gl_color4f(gl, 0.25, 0.25, 0.25, 1.0);
        painter.gl_quad(gl, world.left - 1.0, world.top + 10.0, 1.0, (world.bottom - world.top) - 20.0, false);
        if !dressed {
            self.scroll_bar_draw_arrow(id, minus, 2, painter);
            self.scroll_bar_draw_arrow(id, plus, 3, painter);
        }
        self.control_render(id, dt, painter);
    }

    /// `MovingBar::render` 0x14025bbd0: three pictures, top, middle, bottom.
    fn moving_bar_render(&mut self, id: Id, painter: &mut dyn Painter) {
        let c = self.controls[id].rect;
        let mut r = c;
        self.get_world_rect(id, &mut r);
        let (w, h) = (c.right - c.left, c.bottom - c.top);
        let gl = self.controls[id].gl;
        let Widget2::MovingBar(bar) = self.widget2(id) else { return };
        painter.gl_color4f(gl, 1.0, 1.0, 1.0, 1.0);
        painter.set_texture(0, &bar.upper_texture);
        painter.gl_quad(gl, r.left, r.top, w, 4.0, true);
        painter.gl_color4f(gl, 1.0, 1.0, 1.0, 1.0);
        painter.set_texture(0, &bar.middle_texture);
        painter.gl_quad(gl, r.left, r.top + 4.0, w, h - 8.0, true);
        painter.set_texture(0, &bar.lower_texture);
        painter.gl_quad(gl, r.left, r.bottom - 5.0, w, 4.0, true);
    }

    /// `MovingBar::onMouseDown` 0x14025b800.
    fn moving_bar_on_mouse_down(&mut self, id: Id, e: &MouseEvent) -> bool {
        let mut r = self.controls[id].rect;
        self.get_world_rect(id, &mut r);
        let (mx, my) = (e.x as f32, e.y as f32);
        if mx > r.left && mx < r.right && my > r.top && my < r.bottom {
            self.controls[id].is_mouse_down = true;
            self.moving_bar_mut(id).pressing_point = [mx, my];
            return self.control_on_mouse_down(id, e);
        }
        false
    }

    /// `MovingBar::onMouseMove` 0x14025b8b0: dragged a row's height, the list moves a row.
    fn moving_bar_on_mouse_move(&mut self, id: Id, e: &MouseEvent) {
        if !self.controls[id].is_mouse_down {
            return;
        }
        let (scroll_bar, pressing) = match self.widget2(id) {
            Widget2::MovingBar(m) => (m.scroll_bar, m.pressing_point),
            _ => return,
        };
        let my = e.y as f32;
        let d = my - pressing[1];
        let r = self.controls[scroll_bar].rect;
        let s = self.scroll_bar(scroll_bar);
        let (plus, minus) = (s.but_plus, s.but_minus);
        let n = s.max_value.wrapping_sub(s.min_value) as i32;
        let unit = ((r.bottom - r.top) - ((r.right - r.left) * 2.0)) * (1.0 / n as f32);
        if d.abs() > unit {
            self.moving_bar_mut(id).pressing_point = [e.x as f32, my];
            let event = Clicked { control: NO_CONTROL, local_x: 0, local_y: 0 };
            self.fire_clicked(if d > 0.0 { plus } else { minus }, event);
        }
    }

    // ----- TextBox -----

    /// `TextBox::TextBox(name, gui, fontType)` 0x14024e330.
    pub fn new_text_box(&mut self, painter: &mut dyn Painter, name: &str, font: FontType) -> Id {
        let id = self.new_control(painter, name);
        self.controls[id].back_color = [0.0, 0.0, 0.0, 0.5];
        self.controls[id].border_color = [0.25, 0.25, 0.25, 0.5];
        self.fonts.push(Font::new(painter, "", font, 18.0, false, false));
        self.controls[id].font = Some(self.fonts.len() - 1);
        self.set_widget2(id, Widget2::TextBox(TextBox { text_lines: Vec::new() }));
        id
    }

    /// `TextBox::setFormattedText` 0x14024e6e0: one line a line of the text; a line of 250
    /// characters or more ends it (the stream's buffer is that long).
    pub fn set_formatted_text(&mut self, id: Id, text: &str) {
        let mut lines = Vec::new();
        let mut parts: Vec<&str> = text.split('\n').collect();
        if parts.last() == Some(&"") {
            parts.pop();
        }
        for part in parts {
            if part.encode_utf16().count() >= 250 {
                break;
            }
            lines.push(part.to_string());
        }
        if let Widget2::TextBox(t) = self.widget2_mut(id) {
            t.text_lines = lines;
        }
    }

    /// `TextBox::render` 0x14024e5a0: the lines under each other; the box grows to hold them.
    fn text_box_render(&mut self, id: Id, dt: f32, painter: &mut dyn Painter) {
        self.control_render(id, dt, painter);
        let o = self.local_to_world(id, [0.0, 0.0]);
        let font = self.controls[id].font.unwrap_or(self.font);
        let lines = match self.widget2(id) {
            Widget2::TextBox(t) => t.text_lines.clone(),
            _ => return,
        };
        let mut acc = 0.0f32;
        for line in &lines {
            self.fonts[font].blit_string(painter, o[0], o[1] + acc, line, 1.0, ALIGN_LEFT);
            acc += self.fonts[font].scale * 1.25;
        }
        let r = self.controls[id].rect;
        if acc > r.bottom - r.top {
            self.set_size(id, r.right - r.left, acc);
        }
    }

    // ----- TextInput -----

    /// `TextInput::TextInput` 0x140245310.
    pub fn new_text_input(&mut self, painter: &mut dyn Painter, name: &str) -> Id {
        let id = self.new_control(painter, name);
        let input = TextInput {
            back_color_focus: self.get_skin_color("TEXT_INPUT", "FOCUS_BACK"),
            back_color_unfocus: self.get_skin_color("TEXT_INPUT", "UNFOCUS_BACK"),
            handlers: Vec::new(),
            has_focus: false,
            max_number_of_char: 0x40,
        };
        self.controls[id].font_align = ALIGN_LEFT;
        self.controls[id].fore_color = self.get_skin_color("TEXT_INPUT", "FORE");
        self.set_widget2(id, Widget2::TextInput(input));
        id
    }

    pub fn text_input_mut(&mut self, id: Id) -> &mut TextInput {
        match self.widget2_mut(id) {
            Widget2::TextInput(t) => t,
            _ => panic!("control {id} is not a text input"),
        }
    }

    /// `TextInput::onKeyChar` 0x1402455d0: Enter validates, Backspace takes the last character
    /// away, anything else is appended (there is no caret to move).
    fn text_input_on_key_char(&mut self, id: Id, key: u32) {
        let (has_focus, max) = match self.widget2(id) {
            Widget2::TextInput(t) => (t.has_focus, t.max_number_of_char),
            _ => return,
        };
        if !has_focus || !self.controls[id].visible || !self.is_active {
            return;
        }
        if key == 0x0d {
            // TextInput::validateText 0x140245820
            let input = self.text_input_mut(id);
            input.has_focus = false;
            let handlers = input.handlers.clone();
            let text = self.controls[id].text.clone();
            self.fire_widget(handlers, WidgetEvent::Text { control: id, text, kind: 0 });
            return;
        }
        let mut units: Vec<u16> = self.controls[id].text.encode_utf16().collect();
        if key == 0x08 {
            if !units.is_empty() {
                units.pop();
                self.controls[id].text = String::from_utf16_lossy(&units);
            }
            return;
        }
        if (units.len() as u32) < max {
            units.push(key as u16);
            self.controls[id].text = String::from_utf16_lossy(&units);
        }
        let handlers = self.text_input_mut(id).handlers.clone();
        let text = self.controls[id].text.clone();
        self.fire_widget(handlers, WidgetEvent::Text { control: id, text, kind: 1 });
    }

    /// `TextInput::render` 0x140245700: with the focus, a "|" after the text for half of every
    /// second (by the game's clock, [`Gui::time_ms`]).
    fn text_input_render(&mut self, id: Id, dt: f32, painter: &mut dyn Painter) {
        let (has_focus, focus, unfocus) = match self.widget2(id) {
            Widget2::TextInput(t) => (t.has_focus, t.back_color_focus, t.back_color_unfocus),
            _ => return,
        };
        self.controls[id].back_color = if has_focus { focus } else { unfocus };
        let saved = self.controls[id].text.clone();
        if has_focus && (self.time_ms * 0.002) as i32 % 2 != 0 {
            self.controls[id].text.push('|');
        }
        self.control_render(id, dt, painter);
        self.controls[id].text = saved;
    }

    // ----- PopOver -----

    /// `PopOver::PopOver` 0x140255700: a white box with a red frame, a red title and black text.
    pub fn new_pop_over(&mut self, painter: &mut dyn Painter, name: &str) -> Id {
        let id = self.new_control(painter, name);
        self.control_set_size(id, 360.0, 200.0);
        self.controls[id].back_color = [1.0; 4];
        self.controls[id].border_color = [1.0, 0.0, 0.0, 1.0];
        let title = self.new_label(painter, "TITLE");
        let r = self.controls[id].rect;
        self.set_size(title, r.right - r.left, 30.0);
        self.set_position(title, 0.0, 0.0);
        self.controls[title].fore_color = [1.0, 0.0, 0.0, 1.0];
        self.fonts.push(Font::new(painter, "", FontType::Proportional, 22.0, false, false));
        self.controls[title].font = Some(self.fonts.len() - 1);
        self.controls[title].font_align = ALIGN_CENTER;
        self.add_control(id, title);
        let text_box = self.new_text_box(painter, &format!("{name}_TEXTBOX"), FontType::Proportional);
        self.set_size(text_box, 330.0, 159.0);
        self.set_position(text_box, 15.0, 40.0);
        self.controls[text_box].border_color = [1.0; 4];
        if let Some(font) = self.controls[text_box].font {
            self.fonts[font].scale = 15.0;
        }
        self.controls[text_box].back_color = [1.0; 4];
        self.controls[text_box].fore_color = [0.0, 0.0, 0.0, 1.0];
        self.add_control(id, text_box);
        self.set_widget2(id, Widget2::PopOver(PopOver { text_box, title }));
        id
    }

    fn pop_over(&self, id: Id) -> &PopOver {
        match self.widget2(id) {
            Widget2::PopOver(p) => p,
            _ => panic!("control {id} is not a pop-over"),
        }
    }

    /// `PopOver::setLabelTitle` 0x140255cb0.
    pub fn pop_over_set_label_title(&mut self, id: Id, title: &str) {
        let control = self.pop_over(id).title;
        self.set_text(control, title);
    }

    /// `PopOver::setText` 0x140255d20.
    pub fn pop_over_set_text(&mut self, id: Id, text: &str) {
        let text_box = self.pop_over(id).text_box;
        self.set_formatted_text(text_box, text);
    }

    /// `PopOver::render` 0x140255a20: as a child it only grows to hold its text; whoever owns
    /// it draws it with [`Gui::render_pop_over`].
    fn pop_over_render(&mut self, id: Id) {
        let text_box = self.pop_over(id).text_box;
        let r = self.controls[id].rect;
        let t = self.controls[text_box].rect;
        let need = (t.bottom - t.top) + t.top;
        if need > r.bottom - r.top {
            self.set_size(id, r.right - r.left, need + 5.0);
        }
    }

    /// `PopOver::renderPopOver` 0x140255a70: to the right of its parent, kept on the screen,
    /// with an arrow pointing at the parent.
    pub fn render_pop_over(&mut self, id: Id, dt: f32, painter: &mut dyn Painter) {
        let Some(parent) = self.controls[id].parent else { return };
        let p = self.controls[parent].rect;
        let (pw, ph) = (p.right - p.left, p.bottom - p.top);
        let qy = ph * -0.5;
        let r = self.controls[id].rect;
        let mut a = [pw, qy - ((r.bottom - r.top) * 0.5)];
        let w = self.local_to_world(parent, a);
        if w[1] < 0.0 {
            a[1] += w[1].abs();
        }
        let bottom = (r.bottom - r.top) + w[1];
        let screen_h = self.screen.1 as f32;
        if bottom > screen_h {
            a[1] += screen_h - bottom;
        }
        self.set_position(id, a[0], a[1]);
        let anchor = self.local_to_world(parent, [pw, qy]);
        self.control_render(id, dt, painter);
        let gl = self.controls[id].gl;
        let (x, y) = (anchor[0], anchor[1]);
        painter.gl_color4f(gl, 1.0, 1.0, 1.0, 1.0);
        painter.gl_begin(gl, GL_TRIANGLES);
        painter.gl_vertex3f(gl, x, (y - 22.0) - 1.0, 0.0);
        painter.gl_vertex3f(gl, x - 23.0, y, 0.0);
        painter.gl_vertex3f(gl, x, (y + 22.0) + 1.0, 0.0);
        painter.gl_end(gl);
        painter.gl_color4f(gl, 1.0, 0.0, 0.0, 1.0);
        painter.gl_begin(gl, GL_LINE_STRIP);
        painter.gl_vertex3f(gl, x, y - 22.0, 0.0);
        painter.gl_vertex3f(gl, x - 22.0, y, 0.0);
        painter.gl_vertex3f(gl, x, (y + 22.0) + 1.0, 0.0);
        painter.gl_end(gl);
    }

    // ----- ConnectedLabel -----

    /// `ConnectedLabel::ConnectedLabel` 0x14044e800 (an int) / 0x14044e8a0 (a float), with
    /// `init` 0x14044ea00.
    pub fn new_connected_label(&mut self, painter: &mut dyn Painter, name: &str, value: Connected) -> Id {
        let id = self.new_control(painter, name);
        self.controls[id].back_color = self.get_skin_color("LABEL", "BACK");
        self.controls[id].border_color = self.get_skin_color("LABEL", "BORDER");
        self.controls[id].fore_color = self.get_skin_color("LABEL", "FORE");
        self.controls[id].font_align = ALIGN_LEFT;
        self.set_widget2(id, Widget2::ConnectedLabel(ConnectedLabel { value, label: name.to_string(), suffix: String::new(), precision: 3 }));
        id
    }

    pub fn connected_label_mut(&mut self, id: Id) -> &mut ConnectedLabel {
        match self.widget2_mut(id) {
            Widget2::ConnectedLabel(c) => c,
            _ => panic!("control {id} is not a connected label"),
        }
    }

    /// `ConnectedLabel::render` 0x14044ebb0.
    fn connected_label_render(&mut self, id: Id, dt: f32, painter: &mut dyn Painter) {
        let text = match self.widget2(id) {
            Widget2::ConnectedLabel(c) => {
                let number = match c.value {
                    Connected::Float(v) => format_fixed(v as f64, c.precision.max(0) as usize),
                    Connected::Int(v) => v.to_string(),
                };
                format!("{} {}{}", c.label, number, c.suffix)
            }
            _ => return,
        };
        self.controls[id].text = text;
        self.control_render(id, dt, painter);
    }

    // ----- TabBar -----

    /// `TabBar::TabBar` 0x140257990.
    pub fn new_tab_bar(&mut self, painter: &mut dyn Painter, name: &str) -> Id {
        let id = self.new_control(painter, name);
        self.controls[id].border_color = [0.25, 0.25, 0.25, 0.5];
        self.controls[id].back_color = [0.0, 0.0, 0.0, 0.5];
        let mut scrollers = [0; 2];
        for (slot, (name, file, y)) in scrollers.iter_mut().zip([("LEFT", "content/gui/controls/tabbar/scroll_left_small.png", 0.0), ("RIGHT", "content/gui/controls/tabbar/scroll_right_small.png", 2.0)]) {
            let scroller = self.new_control(painter, name);
            self.controls[scroller].draw_background = false;
            self.controls[scroller].draw_border = false;
            self.controls[scroller].back_texture = painter.get_texture(file);
            self.set_position(scroller, 2.0, y);
            self.set_size(scroller, 24.0, 33.0);
            self.add_control(id, scroller);
            *slot = scroller;
        }
        let [left_scroller, right_scroller] = scrollers;
        self.set_widget2(
            id,
            Widget2::TabBar(TabBar {
                handlers: Vec::new(),
                tab_bar_x: 0.0,
                top_index: 0,
                roll_on_index: 0,
                left_scroller,
                right_scroller,
                selected_color: [1.0, 0.0, 0.0, 0.8],
                unselected_color: [0.0; 4],
                rollover_color: [0.25, 0.25, 0.25, 0.5],
                elements: Vec::new(),
                mouse: [0; 2],
            }),
        );
        // the tab before; from the first it goes to the last
        self.controls[left_scroller].ev_clicked.push(Rc::new(move |gui: &mut Gui, _: &Clicked| {
            let bar = gui.tab_bar_mut(id);
            let old = bar.top_index;
            let n1 = (bar.elements.len() as u64).wrapping_sub(1) as f32;
            let v = bar.top_index.wrapping_sub(1) as f32;
            bar.top_index = (if v > n1 {
                n1
            } else if v < 0.0 {
                0.0
            } else {
                v
            }) as i64 as u32;
            gui.tab_bar_selected(id, old);
        }));
        // the tab after; from the last it goes to the first
        self.controls[right_scroller].ev_clicked.push(Rc::new(move |gui: &mut Gui, _: &Clicked| {
            let bar = gui.tab_bar_mut(id);
            let old = bar.top_index;
            if bar.elements.is_empty() {
                return;
            }
            bar.top_index = ((bar.top_index as u64 + 1) % bar.elements.len() as u64) as u32;
            gui.tab_bar_selected(id, old);
        }));
        id
    }

    pub fn tab_bar(&self, id: Id) -> &TabBar {
        match self.widget2(id) {
            Widget2::TabBar(t) => t,
            _ => panic!("control {id} is not a tab bar"),
        }
    }

    pub fn tab_bar_mut(&mut self, id: Id) -> &mut TabBar {
        match self.widget2_mut(id) {
            Widget2::TabBar(t) => t,
            _ => panic!("control {id} is not a tab bar"),
        }
    }

    /// `evOnCellSelected`.
    fn tab_bar_selected(&mut self, id: Id, old: u32) {
        let bar = self.tab_bar(id);
        let (handlers, new) = (bar.handlers.clone(), bar.top_index);
        self.fire_widget(handlers, WidgetEvent::TabSelected { tab_bar: id, old_index: old, new_index: new });
    }

    /// `TabBar::addTab` 0x140258120: a label of 200 by 33; the first tab is chosen again.
    pub fn tab_bar_add_tab(&mut self, painter: &mut dyn Painter, id: Id, name: &str) -> Id {
        let label = self.new_label(painter, name);
        self.set_text(label, name);
        self.set_size(label, 200.0, 33.0);
        self.controls[label].back_color = self.tab_bar(id).unselected_color;
        self.controls[label].draw_border = false;
        self.controls[label].draw_background = true;
        self.controls[label].font_align = ALIGN_CENTER;
        self.controls[label].ev_clicked.push(Rc::new(move |gui: &mut Gui, ev: &Clicked| gui.tab_bar_on_tab_clicked(id, ev)));
        let x = self.tab_bar(id).elements.len() as f32 * 200.0 + 226.0;
        self.set_position(label, x, 0.0);
        let bar = self.tab_bar_mut(id);
        bar.elements.push(label);
        bar.top_index = 0;
        self.add_control(id, label);
        label
    }

    /// `TabBar::onTabClicked` 0x140258360: the tab is found by its name (the last of that name).
    fn tab_bar_on_tab_clicked(&mut self, id: Id, ev: &Clicked) {
        let bar = self.tab_bar(id);
        let (elements, old, unselected, selected) = (bar.elements.clone(), bar.top_index, bar.unselected_color, bar.selected_color);
        if let Some(tab) = elements.get(old as usize) {
            self.controls[*tab].back_color = unselected;
        }
        let name = if ev.control == NO_CONTROL { String::new() } else { self.controls[ev.control].name.clone() };
        for (i, tab) in elements.iter().enumerate() {
            if self.controls[*tab].name == name {
                self.tab_bar_mut(id).top_index = i as u32;
            }
        }
        self.tab_bar_selected(id, old);
        let top = self.tab_bar(id).top_index;
        if let Some(tab) = elements.get(top as usize) {
            self.controls[*tab].back_color = selected;
        }
    }

    /// `TabBar::orderAlphabetically` 0x140258500: the tabs from `first` to before `last` by
    /// their text (the compiler's `std::sort`).
    pub fn tab_bar_order_alphabetically(&mut self, id: Id, first: usize, last: usize) {
        let mut elements = self.tab_bar(id).elements.clone();
        let (first, last) = (first.min(elements.len()), last.min(elements.len()));
        if first < last {
            let key = |tab: &Id| self.controls[*tab].text.encode_utf16().collect::<Vec<u16>>();
            rustyac_physics::msvc_sort::sort(&mut elements[first..last], &|a: &Id, b: &Id| key(a) < key(b));
        }
        self.tab_bar_mut(id).elements = elements;
    }

    /// `TabBar::updateRollOnIndex` 0x140258750.
    fn tab_bar_update_roll_on_index(&mut self, id: Id) {
        let mut r = self.controls[id].rect;
        self.get_world_rect(id, &mut r);
        let bar = self.tab_bar_mut(id);
        let (mx, my) = (bar.mouse[0] as f32, bar.mouse[1] as f32);
        if my < 35.0 {
            let l = r.left + 24.0;
            if mx > l && mx < r.right - 24.0 {
                let index = ((mx - l) * 0.005) as i64;
                if index as u32 as u64 <= bar.elements.len() as u64 {
                    bar.roll_on_index = bar.top_index.wrapping_sub(1).wrapping_add(index as u32);
                }
                return;
            }
        }
        bar.roll_on_index = bar.top_index;
    }

    /// `TabBar::render` 0x140258530: the bar is 88 hundredths of its parent wide; the chosen
    /// tab slides to 226 pixels from its left at 2500 pixels a second.
    fn tab_bar_render(&mut self, id: Id, dt: f32, painter: &mut dyn Painter) {
        self.tab_bar_update_position(id);
        self.tab_bar_render_rest(id, dt, painter);
    }

    /// `TabBar::updateTabBarPosition` 0x140258810.
    pub fn tab_bar_update_position(&mut self, id: Id) {
        if let Some(parent) = self.controls[id].parent {
            let p = self.controls[parent].rect;
            let w = p.right - p.left;
            let x = (w * 0.01) * 8.5;
            self.tab_bar_mut(id).tab_bar_x = x;
            self.set_size(id, (w * 0.01) * 88.0, 35.0);
            let r = self.controls[id].rect;
            let right = self.tab_bar(id).right_scroller;
            self.set_position(right, ((r.right - r.left) - 24.0) - 1.0, 0.0);
            self.set_position(id, x, 0.0);
        }
    }

    fn tab_bar_render_rest(&mut self, id: Id, dt: f32, painter: &mut dyn Painter) {
        self.tab_bar_update_roll_on_index(id);
        // updateTabs 0x1402588b0
        let step = dt * 2500.0;
        let (elements, top) = (self.tab_bar(id).elements.clone(), self.tab_bar(id).top_index);
        if let Some(selected) = elements.get(top as usize) {
            let d = self.controls[*selected].rect.left - 226.0;
            if d.abs() > step {
                let direction = if d > 0.0 {
                    1.0f32
                } else if d < 0.0 {
                    -1.0
                } else {
                    0.0
                };
                let delta = -direction * step;
                for tab in &elements {
                    let r = &mut self.controls[*tab].rect;
                    r.left = delta + r.left;
                    r.right = delta + r.right;
                }
            } else {
                let mut x = 226.0 - top as f32 * 200.0;
                for tab in &elements {
                    let r = &mut self.controls[*tab].rect;
                    r.left = x;
                    x += 200.0;
                    r.right = x;
                }
            }
        }
        self.control_render(id, dt, painter);
        let gl = self.controls[id].gl;
        let (left, right) = (self.tab_bar(id).left_scroller, self.tab_bar(id).right_scroller);
        for (i, tab) in elements.iter().enumerate() {
            let visible = self.controls[left].rect.left < self.controls[*tab].rect.left && self.controls[right].rect.left > self.controls[*tab].rect.right;
            self.set_visible(*tab, visible);
            if self.controls[*tab].visible {
                let bar = self.tab_bar(id);
                self.controls[*tab].back_color = if i as u32 == bar.top_index {
                    bar.selected_color
                } else if bar.roll_on_index == i as u32 {
                    bar.rollover_color
                } else {
                    bar.unselected_color
                };
                if i + 1 < elements.len() {
                    painter.gl_color4f(gl, 1.0, 1.0, 1.0, 1.0);
                    let mut r = self.controls[*tab].rect;
                    self.get_world_rect(*tab, &mut r);
                    painter.gl_quad(gl, r.right - 1.0, 5.0, 2.0, 21.0, false);
                }
            }
        }
    }

    // ----- DefaultSystemMessage -----

    /// `DefaultSystemMessage::DefaultSystemMessage` 0x14025a4a0.
    pub fn new_default_system_message(&mut self, painter: &mut dyn Painter) -> Id {
        let id = self.new_control(painter, "DEFAULT_SYSTEM_MESSAGE");
        self.controls[id].back_color = [0.0, 0.0, 0.0, 0.5];
        self.controls[id].border_color = [0.5, 0.5, 0.5, 0.95];
        let bar = painter.get_texture("content/gui/systemMessage/bar.png");
        self.controls[id].fore_color = [1.0; 4];
        let mut labels = [0; 2];
        for (slot, (name, size)) in labels.iter_mut().zip([("SYSTEM_MESSAGE_TITLE", 21.0), ("SYSTEM_MESSAGE_DESCRIPTION", 17.0)]) {
            let label = self.new_label(painter, name);
            self.controls[label].fore_color = [1.0; 4];
            self.controls[label].draw_background = false;
            self.controls[label].font_align = ALIGN_CENTER;
            self.fonts.push(Font::new(painter, "", FontType::Proportional, size, false, false));
            self.controls[label].font = Some(self.fonts.len() - 1);
            self.add_control(id, label);
            *slot = label;
        }
        self.set_widget2(id, Widget2::DefaultSystemMessage(DefaultSystemMessage { title: labels[0], description: labels[1], bar }));
        id
    }

    /// `DefaultSystemMessage::setMessage` 0x14025aab0.
    pub fn default_system_message_set(&mut self, id: Id, title: &str, description: &str) {
        let Widget2::DefaultSystemMessage(m) = self.widget2(id) else { return };
        let (t, d) = (m.title, m.description);
        self.set_text(t, title);
        self.set_text(d, description);
    }

    /// `DefaultSystemMessage::render` 0x14025a8d0: the title over the description, a line of
    /// 550 pixels between them.
    fn default_system_message_render(&mut self, id: Id, dt: f32, painter: &mut dyn Painter) {
        let (title, description, bar) = match self.widget2(id) {
            Widget2::DefaultSystemMessage(m) => (m.title, m.description, m.bar.clone()),
            _ => return,
        };
        let r = self.controls[id].rect;
        let (w, h) = (r.right - r.left, r.bottom - r.top);
        let half_w = w * 0.5;
        if self.controls[description].visible {
            self.set_size(title, w, h * 0.5);
            self.set_size(description, w, h * 0.5);
            self.set_position(title, 0.0, 6.0);
            self.set_position(description, 0.0, (h * 0.5) + 8.0);
            self.control_render(id, dt, painter);
            let (gl, color) = (self.controls[id].gl, self.controls[id].border_color);
            painter.gl_color4f(gl, color[0], color[1], color[2], color[3]);
            painter.set_texture(0, &bar);
            let r = self.controls[id].rect;
            painter.gl_quad(gl, half_w - 275.0, (r.bottom - r.top) * 0.5, 550.0, 1.0, true);
        } else {
            self.set_size(title, w, h * 0.5);
            let t = self.controls[title].rect;
            self.set_position(title, 0.0, (h * 0.5) - ((t.bottom - t.top) * f32::from_bits(0x3eaa_aaab)));
            self.control_render(id, dt, painter);
        }
    }

    // ----- the virtual functions, by widget -----

    pub fn widget2_render(&mut self, id: Id, dt: f32, painter: &mut dyn Painter) {
        match self.widget2(id) {
            Widget2::ScrollBar(_) => self.scroll_bar_render(id, dt, painter),
            Widget2::MovingBar(_) => self.moving_bar_render(id, painter),
            Widget2::TextBox(_) => self.text_box_render(id, dt, painter),
            Widget2::TextInput(_) => self.text_input_render(id, dt, painter),
            Widget2::PopOver(_) => self.pop_over_render(id),
            Widget2::ConnectedLabel(_) => self.connected_label_render(id, dt, painter),
            Widget2::TabBar(_) => self.tab_bar_render(id, dt, painter),
            Widget2::DefaultSystemMessage(_) => self.default_system_message_render(id, dt, painter),
            Widget2::More(_) => self.widget3_render(id, dt, painter),
        }
    }

    pub fn widget2_set_size(&mut self, id: Id, w: f32, h: f32) {
        match self.widget2(id) {
            Widget2::ScrollBar(_) => {
                self.control_set_size(id, w, h);
                self.scroll_bar_place_bar(id);
            }
            Widget2::More(_) => self.widget3_set_size(id, w, h),
            _ => self.control_set_size(id, w, h),
        }
    }

    pub fn widget2_on_mouse_down(&mut self, id: Id, e: &MouseEvent) -> bool {
        match self.widget2(id) {
            Widget2::MovingBar(_) => self.moving_bar_on_mouse_down(id, e),
            Widget2::TextBox(_) => {
                self.control_on_mouse_down(id, e);
                true
            }
            Widget2::TextInput(_) => {
                let hit = self.control_on_mouse_down(id, e);
                self.text_input_mut(id).has_focus = hit;
                hit
            }
            Widget2::More(_) => self.widget3_on_mouse_down(id, e),
            _ => self.control_on_mouse_down(id, e),
        }
    }

    pub fn widget2_on_mouse_up(&mut self, id: Id, e: &MouseEvent) {
        match self.widget2(id) {
            Widget2::More(_) => self.widget3_on_mouse_up(id, e),
            // (MovingBar::onMouseUp clears the flag the base clears as well)
            _ => self.control_on_mouse_up(id, e),
        }
    }

    pub fn widget2_on_mouse_move(&mut self, id: Id, e: &MouseEvent) {
        match self.widget2(id) {
            Widget2::ScrollBar(_) => {
                if self.controls[id].visible {
                    for child in self.controls[id].controls.clone() {
                        if self.controls[child].visible {
                            self.on_mouse_move(child, e);
                        }
                    }
                }
            }
            Widget2::MovingBar(_) => self.moving_bar_on_mouse_move(id, e),
            Widget2::TabBar(_) => {
                self.tab_bar_mut(id).mouse = [e.x, e.y];
                self.tab_bar_update_roll_on_index(id);
            }
            Widget2::More(_) => self.widget3_on_mouse_move(id, e),
            _ => self.control_on_mouse_move(id, e),
        }
    }

    pub fn widget2_on_mouse_wheel(&mut self, id: Id, e: &MouseEvent) {
        match self.widget2(id) {
            Widget2::ScrollBar(s) => {
                // any turn of the wheel anywhere moves it a step, through the buttons' handlers
                let (plus, minus) = (s.but_plus, s.but_minus);
                self.control_on_mouse_wheel(id, e);
                let event = Clicked { control: id, local_x: 0, local_y: 0 };
                self.fire_clicked(if e.delta > 0.0 { minus } else { plus }, event);
            }
            Widget2::More(_) => self.widget3_on_mouse_wheel(id, e),
            _ => self.control_on_mouse_wheel(id, e),
        }
    }

    pub fn widget2_on_key_char(&mut self, id: Id, key: u32) {
        match self.widget2(id) {
            Widget2::TextInput(_) => self.text_input_on_key_char(id, key),
            Widget2::More(_) => self.widget3_on_key_char(id, key),
            _ => self.control_on_key_char(id, key),
        }
    }

    pub fn widget2_on_key_down(&mut self, id: Id, key: u32) {
        match self.widget2(id) {
            // TextInput's is empty: a key going down does nothing to it
            Widget2::TextInput(_) => {}
            Widget2::More(_) => self.widget3_on_key_down(id, key),
            _ => self.control_on_key_down(id, key),
        }
    }

    pub fn widget2_set_text(&mut self, id: Id, text: &str) {
        match self.widget2(id) {
            Widget2::More(_) => self.widget3_set_text(id, text),
            _ => self.controls[id].text = text.to_string(),
        }
    }

    pub fn widget2_scale_by_mult(&mut self, id: Id, m: f32) {
        match self.widget2(id) {
            Widget2::More(_) => self.widget3_scale_by_mult(id, m),
            _ => self.control_scale_by_mult(id, m),
        }
    }

    pub fn widget2_on_visible_changed(&mut self, id: Id, visible: bool) {
        self.control_on_visible_changed(id, visible);
    }

    pub fn widget2_set_repeat_interval(&mut self, id: Id, v: f32) {
        match self.widget2(id) {
            Widget2::ScrollBar(_) => self.scroll_bar_set_repeat_interval(id, v),
            Widget2::More(_) => self.widget3_set_repeat_interval(id, v),
            _ => self.controls[id].repeat_interval = v,
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn special_values_round_like_digits() {
        assert_eq!(super::format_fixed(-f64::NAN, 3), "-1.#IO");
        assert_eq!(super::format_fixed(f64::INFINITY, 2), "1.#J");
        assert_eq!(super::format_fixed(f64::INFINITY, 4), "1.#INF");
        assert_eq!(super::format_fixed(f64::INFINITY, 6), "1.#INF00");
    }
}
