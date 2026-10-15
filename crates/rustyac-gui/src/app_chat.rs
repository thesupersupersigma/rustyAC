// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! `FormChat` (0x3e0 bytes), the app "Chat": the online chat's lines, wrapped at a number of
//! characters, coloured by driver, fading out 10 + 5 s after the mouse was last over it, six
//! buttons to scroll, a line to type in; off line the typed lines are only echoed.
//!
//! `FormChat::FormChat` 0x140308190, `addMessage` 0x14030b780, `onChatMessage` 0x14030bd90,
//! `onMouseMove` 0x14030c000, `render` 0x14030c180, `~FormChat` 0x14030a2c0 (the export). The
//! settings are the game's `system/cfg/chat_app.ini`. The export the game writes into
//! `Documents\Assetto Corsa\out\chat[_<time>].txt` when the form is destroyed goes into
//! rustyAC's own `out` folder (the action `out/chat….txt`), at the game's end.

use std::any::Any;
use std::cell::RefCell;
use std::rc::Rc;

use crate::apps::{AppAction, App};
use crate::control::{Clicked, Id, MouseEvent};
use crate::font::{Font, FontType};
use crate::gui::Gui;
use crate::hud::translate;
use crate::menus::SimAction;
use crate::painter::{Painter, Texture, ALIGN_CENTER, ALIGN_LEFT};
use crate::widgets::WidgetEvent;
use crate::widgets2::Widget2;
use rustyac_physics::data::ini::IniReader;

/// The local time as `_localtime64(_time64(0))` gives it: year, month, day, hour, minute,
/// second.
pub type LocalClock = fn() -> [i32; 6];

/// `FormChat::ChatMessage` (0x80): `visibility` is never other than 1.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ChatMessage {
    pub msgid: i32,
    pub msg: Vec<u16>,
    pub userid: i32,
    pub username: Vec<u16>,
    pub space: bool,
    pub time: Vec<u16>,
}

/// `FormChat::ChatIntensities`
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ChatIntensities {
    pub prefade: f32,
    pub fade: f32,
    pub fore: f32,
    pub back: f32,
}

/// The members the form's handlers change too (the buttons' and the text input's lambdas).
pub struct ChatState {
    pub messages: Vec<ChatMessage>,
    pub new_messages: Vec<ChatMessage>,
    pub shown_messages: Vec<ChatIntensities>,
    pub times: Vec<Id>,
    pub rows: Vec<Id>,
    pub fore_colors: Vec<[f32; 4]>,
    pub back_colors: Vec<[f32; 4]>,
    pub server_fore_color: [f32; 4],
    pub server_back_color: [f32; 4],
    pub player_fore_color: [f32; 4],
    pub player_back_color: [f32; 4],
    pub must_update: bool,
    pub reset_visibility: bool,
    pub is_last_row: bool,
    pub message_alert: bool,
    pub top_row: i32,
    pub max_width: i32,
    pub last_msg_id: i32,
    pub added_lines: i32,
    pub debug: bool,
    pub show_timestamp: bool,
    pub font_is_bold: bool,
    pub font_is_italic: bool,
    pub fade_mult: f32,
    pub prefade_mult: f32,
    pub export_on_close: bool,
    pub overwrite_export: bool,
    pub row_count: u16,
    pub bg_intensity_main: f32,
    pub font_size: f32,
    pub pix_per_char: f32,
    pub time_row_w: f32,
    pub single_row_h: f32,
    pub single_row_w: f32,
    pub fwidth: f32,
    pub fheight: f32,
    pub current_time_to_lose: f32,
    pub time_to_lose_focus: f32,
    pub scroll_color: [f32; 4],
    pub scroll_top: Id,
    pub scroll_up: Id,
    pub scroll_down: Id,
    pub scroll_bottom: Id,
    pub scroll_half_up: Id,
    pub scroll_half_down: Id,
    pub text_input: Id,
    /// `sim->client` was there when the form was made: the chat's lines are heard
    pub listening: bool,
    pub clock: LocalClock,
}

pub struct ChatApp {
    pub state: Rc<RefCell<ChatState>>,
    pub textures: [Texture; 4],
}

/// `rgbToVec4f` 0x14030cac0: each byte times the f32 1/255.
fn rgb_to_vec4f(v: [f32; 4]) -> [f32; 4] {
    let k = f32::from_bits(0x3b80_8081);
    [v[0] * k, v[1] * k, v[2] * k, v[3] * k]
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

/// `ksTrim` 0x1402396f0: " \t\n\v\f\r" off both ends.
fn ks_trim(s: &[u16]) -> Vec<u16> {
    let blank = |c: &u16| matches!(*c, 0x20 | 0x09 | 0x0a | 0x0b | 0x0c | 0x0d);
    match (s.iter().position(|c| !blank(c)), s.iter().rposition(|c| !blank(c))) {
        (Some(a), Some(b)) => s[a..=b].to_vec(),
        _ => Vec::new(),
    }
}

impl ChatState {
    fn n(&self) -> usize {
        self.messages.len()
    }

    /// The preamble of the four scroll lambdas that go up (and of "a page down" / "a line
    /// down" in their own order).
    fn scroll_up_by(&mut self, step: Option<i32>) {
        if self.n() == 0 {
            return;
        }
        self.must_update = true;
        self.reset_visibility = true;
        let rc = self.row_count as usize;
        self.is_last_row = self.n() <= rc;
        if rc < self.n() {
            self.top_row = match step {
                Some(k) => self.top_row - k,
                None => self.row_count as i32 - 1,
            };
            self.top_row = self.top_row.max(self.row_count as i32 - 1);
        }
    }

    fn scroll_down_by(&mut self, step: i32) {
        if self.n() == 0 {
            return;
        }
        self.top_row += step;
        self.must_update = true;
        self.reset_visibility = true;
        self.is_last_row = self.n() <= self.row_count as usize;
        if self.n() as u64 <= self.top_row as i64 as u64 {
            self.top_row = self.n() as i32 - 1;
            self.is_last_row = true;
        }
    }

    /// `FormChat::addMessage` 0x14030b780.
    fn add_message(&mut self, m: ChatMessage) {
        if self.debug {
            println!("CM ADD {} : {}", String::from_utf16_lossy(&m.username), String::from_utf16_lossy(&m.msg));
        }
        self.message_alert = true;
        let mut c = m.clone();
        let mut text = Vec::new();
        if !c.username.is_empty() {
            text.extend_from_slice(&c.username);
            text.extend(wide(": "));
            let t = (self.clock)();
            c.time = wide(&format!("{:02}:{:02}:{:02}", t[3], t[4], t[5]));
        }
        text.extend_from_slice(&c.msg);
        c.space = text.first() == Some(&0x20);
        let text = ks_trim(&text);
        let max = self.max_width as i64 as u64;
        if text.len() as u64 > max {
            let width = self.max_width as usize;
            let head = &text[..width.min(text.len())];
            let pos = head.iter().rposition(|&ch| ch == 0x20).unwrap_or(width);
            c.msg = text[..pos].to_vec();
            self.messages.push(c);
            self.added_lines += 1;
            // (a cut at 0 would come back with the same text: the game's would never end)
            if pos < text.len() && pos != 0 {
                let cont = ChatMessage { msgid: m.msgid, msg: text[pos..].to_vec(), userid: m.userid, username: Vec::new(), space: false, time: Vec::new() };
                self.add_message(cont);
            }
        } else {
            c.msg = text;
            self.messages.push(c);
            self.added_lines += 1;
        }
        self.must_update = true;
    }

    /// `FormChat::onChatMessage` 0x14030bd90. `net`: `getNetCarFromSessionID`'s driver and guid.
    pub fn on_chat_message(&mut self, session: u32, message: &str, net: Option<(&str, i32)>) {
        let mut m = ChatMessage::default();
        if let Some((name, guid)) = net {
            m.userid = guid + 2;
            m.username = wide(name);
        } else if session == 0xff {
            m.userid = 1;
            m.username = wide("SERVER");
        } else {
            m.userid = 0;
            m.username = wide("Me");
        }
        m.msg = wide(message);
        self.last_msg_id += 1;
        m.msgid = self.last_msg_id;
        self.new_messages.push(m);
    }

    /// `~FormChat`'s export: one line a message, "HH:MM:SS name: text", as `std::wofstream`
    /// writes it in text mode (CR LF; a character the "C" locale cannot narrow, above 0xff,
    /// stops the stream for good). None: `EXPORT_ON_CLOSE` is off.
    pub fn export(&self) -> Option<(String, Vec<u8>)> {
        if !self.export_on_close {
            return None;
        }
        let mut suffix = String::new();
        if self.overwrite_export {
            let t = (self.clock)();
            suffix = format!("_{:04}{:02}{:02}{:02}{:02}{:02}", t[0], t[1], t[2], t[3], t[4], t[5]);
        }
        let mut lines: Vec<Vec<u16>> = Vec::new();
        let (mut prev, mut cur) = (-1, Vec::new());
        for m in &self.messages {
            if m.msgid == prev {
                if m.space {
                    cur.push(0x20);
                }
                cur.extend_from_slice(&m.msg);
            } else {
                if !cur.is_empty() {
                    lines.push(std::mem::take(&mut cur));
                }
                cur = m.time.clone();
                cur.push(0x20);
                cur.extend_from_slice(&m.msg);
                prev = m.msgid;
            }
        }
        if !cur.is_empty() {
            lines.push(cur);
        }
        let mut bytes = Vec::new();
        'out: for line in lines {
            for ch in line.into_iter().chain([0x0a]) {
                match ch {
                    0x0a => bytes.extend_from_slice(b"\r\n"),
                    0..=0xff => bytes.push(ch as u8),
                    _ => break 'out,
                }
            }
        }
        Some((format!("out/chat{suffix}.txt"), bytes))
    }
}

impl Gui {
    /// `FormChat::FormChat(Sim*)` 0x140308190. `ini`: `system/cfg/chat_app.ini` when it loaded.
    pub fn new_chat_app(&mut self, painter: &mut dyn Painter, ini: Option<&IniReader>, clock: LocalClock) -> Id {
        let form = self.new_form(painter, "CHAT_APP", true);
        self.form_data_mut(form).dev_app = false;
        self.form_data_mut(form).autohide = true;
        self.form_set_title(form, &translate("Chat"));
        let mut tex = |name: &str| painter.get_texture(&format!("content/gui/chatapp/{name}.png"));
        let textures = [tex("scrollbarArrowTop"), tex("scrollbarArrowUp"), tex("scrollbarArrowDown"), tex("scrollbarArrowBottom")];
        let mut s = ChatState {
            messages: Vec::new(),
            new_messages: Vec::new(),
            shown_messages: Vec::new(),
            times: Vec::new(),
            rows: Vec::new(),
            fore_colors: Vec::new(),
            back_colors: Vec::new(),
            server_fore_color: [0.0; 4],
            server_back_color: [0.0; 4],
            player_fore_color: [0.0; 4],
            player_back_color: [0.0; 4],
            must_update: true,
            reset_visibility: false,
            is_last_row: true,
            message_alert: false,
            top_row: -1,
            max_width: -1,
            last_msg_id: -1,
            added_lines: 0,
            debug: false,
            show_timestamp: true,
            font_is_bold: false,
            font_is_italic: false,
            fade_mult: 10.0,
            prefade_mult: 10.0,
            export_on_close: false,
            overwrite_export: true,
            row_count: 10,
            bg_intensity_main: f32::from_bits(0x3e4c_cccd),
            font_size: 12.0,
            pix_per_char: 7.0,
            time_row_w: 70.0,
            single_row_h: 17.0,
            single_row_w: 300.0,
            fwidth: 0.0,
            fheight: 0.0,
            current_time_to_lose: 0.0,
            time_to_lose_focus: 5.0,
            scroll_color: [1.0; 4],
            scroll_top: 0,
            scroll_up: 0,
            scroll_down: 0,
            scroll_bottom: 0,
            scroll_half_up: 0,
            scroll_half_down: 0,
            text_input: 0,
            // (the handler of ACClient::evOnChatMessage: `chat_apps_on_chat_message`)
            listening: self.sim.online,
            clock,
        };
        if let Some(r) = ini {
            let int = |k: &str| r.get_int("OPTIONS", k).unwrap_or(0);
            let float = |k: &str| r.get_float("OPTIONS", k).unwrap_or(0.0);
            s.debug = int("DEBUG") != 0;
            s.show_timestamp = int("SHOW_TIME") != 0;
            s.export_on_close = int("EXPORT_ON_CLOSE") != 0;
            s.overwrite_export = int("OVERWRITE_EXPORT") != 0;
            s.prefade_mult = float("PREFADE_MULT");
            if s.prefade_mult == 0.0 {
                s.prefade_mult = 10.0;
            }
            s.fade_mult = float("FADE_MULT");
            if s.fade_mult == 0.0 {
                s.fade_mult = 10.0;
            }
            s.row_count = int("ROW_COUNT") as u16;
            s.bg_intensity_main = float("BG_INTENSITY_MAIN");
            s.font_size = float("FONT_SIZE");
            s.pix_per_char = float("PIX_PER_CHAR");
            s.time_row_w = float("TIME_ROW_W");
            s.single_row_h = float("SINGLE_ROW_H");
            s.single_row_w = float("SINGLE_ROW_W");
            if r.has_key("OPTIONS", "BOLD") {
                s.font_is_bold = int("BOLD") != 0;
            }
            if r.has_key("OPTIONS", "ITALIC") {
                s.font_is_italic = int("ITALIC") != 0;
            }
            if r.has_key("OPTIONS", "TIME_TO_LOSE_FOCUS") {
                s.time_to_lose_focus = int("TIME_TO_LOSE_FOCUS") as f32;
            }
            if r.has_section("COLORS") {
                let color = |k: &str| rgb_to_vec4f(r.get_float4("COLORS", k).unwrap_or([0.0; 4]));
                s.server_fore_color = color("SERVER_FORECOLOR");
                s.server_back_color = color("SERVER_BACKCOLOR");
                s.player_fore_color = color("PLAYER_FORECOLOR");
                s.player_back_color = color("PLAYER_BACKCOLOR");
                let mut i = 0;
                while r.has_key("COLORS", &format!("FORECOLOR_{i}")) {
                    s.fore_colors.push(color(&format!("FORECOLOR_{i}")));
                    s.back_colors.push(color(&format!("BACKCOLOR_{i}")));
                    i += 1;
                }
            }
        }
        if !s.show_timestamp {
            s.time_row_w = 0.0;
        }
        let h = s.single_row_h;
        s.fwidth = (s.single_row_w + s.time_row_w) + h;
        let header = self.form_data(form).header_height;
        s.fheight = (header + s.row_count as f32 * h) + h;
        self.fonts.push(Font::new(painter, "", FontType::Monospaced, s.font_size, s.font_is_italic, s.font_is_bold));
        let main_font = self.fonts.len() - 1;

        let input = self.new_text_input(painter, "CHAT_TEXT_INPUT");
        self.set_visible(input, true);
        self.controls[input].back_color = [0.0, 0.0, 0.0, 0.5];
        self.controls[input].border_color = [0.25, 0.25, 0.25, 0.5];
        self.text_input_mut(input).back_color_unfocus = [0.25, 0.25, 0.25, 0.5];
        self.text_input_mut(input).back_color_focus = [0.0, 0.0, 0.0, 0.5];
        self.set_position(input, 0.0, s.fheight - h);
        self.set_size(input, (s.single_row_w + s.time_row_w) + h, h);
        self.controls[input].font = Some(main_font);
        self.add_control(form, input);
        s.text_input = input;

        let mut y = header;
        let mut xoff = 0.0;
        for i in 0..s.row_count as i32 {
            let time = self.new_label(painter, &format!("chat_time_t{i}"));
            self.fonts.push(Font::new(painter, "", FontType::Monospaced, s.font_size, false, false));
            self.controls[time].font = Some(self.fonts.len() - 1);
            self.controls[time].font_align = ALIGN_CENTER;
            self.controls[time].fore_color = [1.0, 1.0, 1.0, f32::from_bits(0x3f33_3333)];
            self.set_size(time, s.time_row_w, h);
            self.set_position(time, 0.0, y);
            self.set_text(time, "");
            self.set_visible(time, false);
            s.times.push(time);
            self.add_control(form, time);
            if s.show_timestamp {
                xoff = s.time_row_w;
            }
            let row = self.new_label(painter, &format!("chat_lbl_t{i}"));
            self.controls[row].font = Some(main_font);
            self.controls[row].font_align = ALIGN_LEFT;
            self.set_size(row, s.single_row_w, h);
            self.set_position(row, xoff, y);
            self.set_text(row, "");
            self.set_visible(row, false);
            s.rows.push(row);
            self.add_control(form, row);
            y += h;
            s.shown_messages.push(ChatIntensities { prefade: 1.0, fade: 1.0, fore: 1.0, back: 1.0 });
        }

        let state = Rc::new(RefCell::new(s));
        let s = state.borrow();
        let fwidth = s.fwidth;
        let fheight = s.fheight;
        let scroll_color = s.scroll_color;
        drop(s);
        let scroll = |gui: &mut Gui, painter: &mut dyn Painter, name: &str, w: f32, hh: f32, texture: Option<&Texture>, x: f32, y: f32, f: fn(&mut ChatState)| {
            let c = gui.new_control(painter, name);
            gui.set_text(c, "");
            gui.set_size(c, w, hh);
            gui.set_repeat_interval(c, 0.25);
            gui.controls[c].back_color = scroll_color;
            if let Some(t) = texture {
                gui.controls[c].back_texture = t.clone();
                gui.controls[c].back_texture_color = [0.0, 0.0, 0.0, 1.0];
            }
            gui.controls[c].draw_border = false;
            gui.set_position(c, x, y);
            let st = state.clone();
            gui.controls[c].ev_clicked.push(Rc::new(move |_: &mut Gui, _: &Clicked| f(&mut st.borrow_mut())));
            gui.add_control(form, c);
            c
        };
        let hdr = header;
        let top = scroll(self, painter, "scrollTop", h, h, Some(&textures[0]), fwidth - h, hdr, |s| s.scroll_up_by(None));
        let up = scroll(self, painter, "scrollUp", h, h, Some(&textures[1]), fwidth - h, hdr + h, |s| s.scroll_up_by(Some(1)));
        let f = (((fheight - hdr) - h * 5.0) * 0.5) as i32 as f32;
        let half_up = scroll(self, painter, "scrollHalfUp", h, f, None, fwidth - h, hdr + h * 2.0, |s| {
            let k = s.row_count as i32;
            s.scroll_up_by(Some(k))
        });
        let half_down = scroll(self, painter, "scrollHalfDown", h, f, None, fwidth - h, (fheight - h * 3.0) - f, |s| {
            let k = s.row_count as i32;
            s.scroll_down_by(k)
        });
        let down = scroll(self, painter, "scrollDown", h, h, Some(&textures[2]), fwidth - h, fheight - h * 3.0, |s| s.scroll_down_by(1));
        let bottom = scroll(self, painter, "scrollBottom", h, h, Some(&textures[3]), fwidth - h, fheight - h * 2.0, |s| {
            if !s.messages.is_empty() {
                s.must_update = true;
                s.reset_visibility = true;
                s.top_row = s.n() as i32 - 1;
                s.is_last_row = true;
            }
        });
        {
            let mut s = state.borrow_mut();
            (s.scroll_top, s.scroll_up, s.scroll_half_up, s.scroll_half_down, s.scroll_down, s.scroll_bottom) = (top, up, half_up, half_down, down, bottom);
        }
        // the text input's evKeyDown (lambda_61896d) and evValidate (lambda_7bf6e8)
        let st = state.clone();
        if let Widget2::TextInput(t) = self.widget2_mut(input) {
            t.handlers.push(Rc::new(move |gui: &mut Gui, ev: &WidgetEvent| {
                let WidgetEvent::Text { text, kind, .. } = ev else { return };
                let mut s = st.borrow_mut();
                if *kind == 1 {
                    s.current_time_to_lose = s.time_to_lose_focus;
                    return;
                }
                let t = gui.controls[input].text.clone();
                if !t.is_empty() {
                    if !gui.sim.online {
                        let mut m = ChatMessage { userid: 0, username: wide("Me"), msg: wide(&t), ..ChatMessage::default() };
                        s.last_msg_id += 1;
                        m.msgid = s.last_msg_id;
                        s.new_messages.push(m);
                    } else if gui.sim.online_link.is_allowed_to_send_chat_message(gui.sim.game_time) {
                        gui.sim.actions.push(SimAction::SendChat(text.clone()));
                    }
                    gui.set_text(input, "");
                }
                s.current_time_to_lose = s.time_to_lose_focus;
                gui.text_input_mut(input).has_focus = true;
            }));
        }
        self.form_set_size(form, fwidth, fheight);
        {
            let mut s = state.borrow_mut();
            s.max_width = (s.single_row_w / s.pix_per_char) as i32;
            self.controls[form].back_color = [0.0, 0.0, 0.0, s.bg_intensity_main];
        }
        // lambda_b6b100: a click anywhere on the form gives the line the keys for 5 s
        let st = state.clone();
        self.controls[form].ev_clicked.push(Rc::new(move |gui: &mut Gui, _: &Clicked| {
            let mut s = st.borrow_mut();
            s.current_time_to_lose = s.time_to_lose_focus;
            gui.text_input_mut(input).has_focus = true;
        }));
        {
            let mut s = state.borrow_mut();
            s.must_update = true;
            s.reset_visibility = true;
        }
        let m = self.controls[form].scale_mult;
        self.scale_by_mult(form, m);
        self.set_app(form, Box::new(ChatApp { state, textures }));
        form
    }

    /// `ACClient::evOnChatMessage` as the Chat app hears it (it is hung up before `ACChat`'s).
    /// `net`: `getNetCarFromSessionID`'s driver and guid.
    pub fn chat_apps_on_chat_message(&mut self, session: u32, message: &str, net: Option<(&str, i32)>) {
        for id in 0..self.controls.len() {
            if let Some(app) = self.app_mut::<ChatApp>(id) {
                let mut s = app.state.borrow_mut();
                if s.listening {
                    s.on_chat_message(session, message, net);
                }
            }
        }
    }
}

impl App for ChatApp {
    fn as_any(&mut self) -> &mut dyn Any {
        self
    }

    /// `FormChat::onMouseMove` 0x14030c000: the mouse over the form brings the lines back.
    fn on_mouse_move(&mut self, gui: &mut Gui, id: Id, e: &MouseEvent) {
        let r = gui.controls[id].rect;
        let (x, y) = (e.x.wrapping_sub(r.left as i32), e.y.wrapping_sub(r.top as i32));
        self.state.borrow_mut().reset_visibility = gui.form_hit_test(id, x, y);
        gui.form_on_mouse_move(id, e);
    }

    /// `FormChat::render` 0x14030c180.
    fn render(&mut self, gui: &mut Gui, id: Id, dt: f32, painter: &mut dyn Painter) {
        let mut s = self.state.borrow_mut();
        let v = gui.controls[id].draw_background;
        for c in [s.scroll_bottom, s.scroll_down, s.scroll_half_down, s.scroll_half_up, s.scroll_up, s.scroll_top] {
            gui.set_visible(c, v);
        }
        let t = s.current_time_to_lose - dt;
        s.current_time_to_lose = if t > 0.0 { t } else { 0.0 };
        if s.current_time_to_lose == 0.0 {
            let input = s.text_input;
            gui.text_input_mut(input).has_focus = false;
        }
        s.added_lines = 0;
        for m in std::mem::take(&mut s.new_messages) {
            s.add_message(m);
        }
        if s.must_update && !s.messages.is_empty() {
            let n = s.messages.len();
            let mut i_msg = n as i32 - 1;
            if !s.is_last_row {
                i_msg = s.top_row;
            }
            for r in (0..s.row_count as usize).rev() {
                let (row, time) = (s.rows[r], s.times[r]);
                gui.set_text(row, "");
                gui.controls[row].back_color = [0.0; 4];
                gui.set_text(time, "");
                gui.controls[time].back_color = gui.controls[row].back_color;
                if (i_msg as i64 as u64) < n as u64 {
                    let msg = s.messages[i_msg as usize].clone();
                    gui.set_text(row, &String::from_utf16_lossy(&msg.msg));
                    gui.set_visible(row, true);
                    let (fore, back) = match msg.userid {
                        0 => (s.player_fore_color, s.player_back_color),
                        1 => (s.server_fore_color, s.server_back_color),
                        u => {
                            // (no colours: the game divides by zero here)
                            let pick = |c: &Vec<[f32; 4]>| if c.is_empty() { [0.0; 4] } else { c[(u as i64 as u64 % c.len() as u64) as usize] };
                            (pick(&s.fore_colors), pick(&s.back_colors))
                        }
                    };
                    gui.controls[row].fore_color = fore;
                    gui.controls[row].back_color = back;
                    gui.controls[time].back_color = gui.controls[row].back_color;
                    gui.controls[time].fore_color = gui.controls[row].fore_color;
                    if s.show_timestamp {
                        gui.set_visible(time, true);
                        if !msg.time.is_empty() {
                            gui.set_text(time, &String::from_utf16_lossy(&msg.time));
                        }
                    }
                    if s.added_lines != 0 && s.is_last_row {
                        s.added_lines -= 1;
                        if !s.shown_messages.is_empty() {
                            s.shown_messages.remove(0);
                        }
                        s.shown_messages.push(ChatIntensities { prefade: 1.0, fade: 1.0, fore: gui.controls[row].fore_color[3], back: gui.controls[row].back_color[3] });
                        s.top_row += 1;
                    }
                    i_msg -= 1;
                }
            }
            s.must_update = false;
        }
        let n = s.messages.len();
        if s.top_row as i64 == n as i64 - 1 || n as u64 <= s.row_count as u64 {
            s.message_alert = false;
            gui.controls[s.scroll_bottom].back_color = s.scroll_color;
        }
        if s.message_alert {
            gui.controls[s.scroll_bottom].back_color = [1.0, 0.0, 0.0, 1.0];
            let b = s.scroll_bottom;
            gui.set_visible(b, true);
        }
        let (reset, prefade_mult, fade_mult) = (s.reset_visibility, s.prefade_mult, s.fade_mult);
        for k in 0..s.shown_messages.len() {
            let m = &mut s.shown_messages[k];
            if reset {
                m.prefade = 1.0;
                m.fade = 1.0;
            } else if m.prefade > 0.0 {
                let v = m.prefade - dt / prefade_mult;
                m.prefade = if v > 0.0 { v } else { 0.0 };
            } else {
                let v = m.fade - dt / fade_mult;
                m.fade = if v > 0.0 { v } else { 0.0 };
            }
            let (fore, back) = (m.fore * m.fade, m.back * m.fade);
            let (row, time) = (s.rows[k], s.times[k]);
            gui.controls[row].fore_color[3] = fore;
            gui.controls[time].fore_color[3] = gui.controls[row].fore_color[3];
            gui.controls[row].back_color[3] = back;
            gui.controls[time].back_color[3] = gui.controls[row].back_color[3];
        }
        s.reset_visibility = false;
        drop(s);
        gui.control_render(id, dt, painter);
    }

    /// `~FormChat` 0x14030a2c0 at the game's end: the export, when `EXPORT_ON_CLOSE`.
    fn shutdown(&mut self, gui: &mut Gui, _id: Id) {
        if let Some((path, bytes)) = self.state.borrow().export() {
            gui.apps.actions.push(AppAction::WriteFile { path, bytes });
        }
    }
}
