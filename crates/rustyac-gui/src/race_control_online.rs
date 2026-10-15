// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! The ESC menu's page "Race control" in an online session: the server's rules
//! (`RaceControlSessionServer`, 0x1a0 bytes: constructor 0x1400b5df0, `render` 0x1400b60c0,
//! `setBackground` 0x1400b7dc0), the votes (`RaceControlVoting`, 0x1c0 bytes: constructor
//! 0x1400b8a50, `render` 0x1400b97b0, `voteSpamCheck` 0x1400ba480) and the page's own chat
//! (`ACChat`, 0x200 bytes: constructor 0x140034aa0, `onChatMessage` 0x140035c00, `render`
//! 0x140036380). What they read of `ACClient` and of the physics the server dictates is
//! [`OnlineLink`]; what they ask of the client goes out as [`SimAction`]s.

use std::rc::Rc;

use crate::control::{Clicked, Id, Kind};
use crate::font::{Font, FontType};
use crate::gui::Gui;
use crate::hud::translate;
use crate::menus::{Menu, SimAction};
use crate::painter::{Painter, Rect, ALIGN_CENTER, ALIGN_LEFT, GRAPHICS_GL};
use crate::widgets::WidgetEvent;
use crate::widgets2::Widget2;

const COL_TITLE_BAR: [f32; 4] = [0.74, 0.0, 0.0, 1.0];
const COL_DARK_BAR: [f32; 4] = [0.21, 0.21, 0.21, 1.0];
const COL_DARK_TRANSP: [f32; 4] = [0.1, 0.1, 0.1, 0.7];

/// `ACClientVotingManager::currentVote`, as the panel reads it through `ACClient`.
#[derive(Clone, Debug, PartialEq)]
pub struct VoteLink {
    /// `getCurrentVoteType`: 0 next session, 1 restart, 2 kick, 3 none
    pub vote_type: i32,
    /// `hasVoted`
    pub has_voted: bool,
    /// `getVotingTimeLeft`, `getVotingMaxTime` (seconds)
    pub time_left: f32,
    pub max_time: f32,
    /// `getCurrentVotingTarget`: the car's `driverInfo.name` and `getGuid` (none: -1)
    pub target_name: String,
    pub target_guid: i32,
}

impl Default for VoteLink {
    fn default() -> VoteLink {
        VoteLink { vote_type: 3, has_voted: false, time_left: 0.0, max_time: 0.0, target_name: String::new(), target_guid: -1 }
    }
}

/// What the online pages read of `ACClient` and of the rules the server wrote into the physics.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct OnlineLink {
    /// `ACClient::serverName`
    pub server_name: String,
    /// `ACClient::serverDrivingAssists`: `tc`, `abs` (0 no, 1 factory, 2 yes), `stability`,
    /// `autoClutch`
    pub tc: i32,
    pub abs: i32,
    pub stability: bool,
    pub auto_clutch: bool,
    /// `PhysicsEngine`: `mechanicalDamageRate`, `fuelConsumptionRate`, `tyreConsumptionRate`,
    /// `allowTyreBlankets`, `penaltyRules.jumpStartPenaltyMode`
    pub damage_rate: f32,
    pub fuel_rate: f32,
    pub tyre_wear_rate: f32,
    pub tyre_blankets: bool,
    pub jump_start_mode: i32,
    pub vote: VoteLink,
    /// `ACClient::lastChatMessage` (game clock, ms)
    pub last_chat_message: f64,
    /// every `RaceControlVoting` (they hear `Sim::evOnNewSession`) and every `ACChat` (they
    /// hear `ACClient::evOnChatMessage`)
    pub votings: Vec<Id>,
    pub chats: Vec<Id>,
}

impl OnlineLink {
    /// `ACClient::isAllowedToSendChatMessage` 0x14004b1a0.
    pub fn is_allowed_to_send_chat_message(&self, now: f64) -> bool {
        now - self.last_chat_message > 1000.0
    }
}

/// `RaceControlSessionServer`'s own members.
pub struct SessionServer {
    pub title_font: usize,
    pub data_font: usize,
}

/// `RaceControlVoting`'s own members.
pub struct Voting {
    pub title_font: usize,
    pub but_vote_next_session: Id,
    pub but_vote_restart: Id,
    pub but_vote_yes: Id,
    pub but_vote_no: Id,
    /// game clock, ms
    pub time_to_next_request: f64,
    pub session_request_count: u32,
}

/// `ChatElement`.
#[derive(Clone, Debug, PartialEq)]
pub struct ChatElement {
    pub player_name: String,
    pub text: String,
    pub color: [f32; 4],
}

/// `ACChat`'s own members.
pub struct AcChat {
    pub title_font: usize,
    pub player_name_font: usize,
    pub player_text_font: usize,
    pub inp_test: Id,
    pub player_name: String,
    pub player_color: [f32; 4],
    pub texts: Vec<ChatElement>,
}

/// `cvttss2si`.
fn cut(x: f32) -> i32 {
    if x > -2147483904.0 && x < 2147483648.0 {
        x as i32
    } else {
        i32::MIN
    }
}

/// `cvttsd2si`.
fn cut_d(x: f64) -> i32 {
    if x > -2147483649.0 && x < 2147483648.0 {
        x as i32
    } else {
        i32::MIN
    }
}

/// `std::wstring::substr` (UTF-16 units).
fn substr(text: &str, pos: usize, len: usize) -> String {
    let units: Vec<u16> = text.encode_utf16().collect();
    let end = pos.saturating_add(len).min(units.len());
    String::from_utf16_lossy(&units[pos.min(units.len())..end])
}

impl Gui {
    fn world_left_top(&self, id: Id) -> (f32, f32) {
        let mut r = Rect { left: 0.0, right: 1.0, top: 0.0, bottom: 1.0 };
        self.get_world_rect(id, &mut r);
        let own = self.controls[id].rect;
        (r.left + own.left, r.top + own.top)
    }

    fn title_bar(&mut self, painter: &mut dyn Painter, font: usize, x: f32, y: f32, w: f32, text: &str) {
        let gl = GRAPHICS_GL;
        painter.gl_color4f(gl, COL_TITLE_BAR[0], COL_TITLE_BAR[1], COL_TITLE_BAR[2], COL_TITLE_BAR[3]);
        painter.gl_quad(gl, x, y, w, 20.0, false);
        self.fonts[font].set_color(1.0, 1.0, 1.0, 1.0);
        self.fonts[font].blit_string(painter, x + 10.0, y + 1.0, text, 1.0, ALIGN_LEFT);
    }

    // ----- RaceControlSessionServer -----

    /// `RaceControlSessionServer::RaceControlSessionServer` 0x1400b5df0.
    pub fn new_session_server(&mut self, painter: &mut dyn Painter) -> Id {
        let id = self.new_control(painter, "RACE_CONTROL_SESSION_SERVER");
        self.fonts.push(Font::new(painter, "", FontType::Monospaced, 14.0, true, true));
        let title_font = self.fonts.len() - 1;
        self.fonts.push(Font::new(painter, "", FontType::Proportional, 14.0, false, true));
        let data_font = self.fonts.len() - 1;
        self.controls[id].draw_border = false;
        self.controls[id].draw_background = false;
        self.controls[id].kind = Kind::Menu(Box::new(Menu::SessionServer(SessionServer { title_font, data_font })));
        id
    }

    /// `RaceControlSessionServer::render` 0x1400b60c0: a title and a row for every rule.
    pub(crate) fn session_server_render(&mut self, id: Id, dt: f32, painter: &mut dyn Painter) {
        if !self.sim.online {
            return;
        }
        self.control_render(id, dt, painter);
        let gl = GRAPHICS_GL;
        let (title_font, data_font) = match &self.controls[id].kind {
            Kind::Menu(menu) => match &**menu {
                Menu::SessionServer(s) => (s.title_font, s.data_font),
                _ => return,
            },
            _ => return,
        };
        let (l, mut t) = self.world_left_top(id);
        let own = self.controls[id].rect;
        let w = own.right - own.left;
        self.title_bar(painter, title_font, l, t, w, &translate("SERVER"));
        t = t + 20.0;
        const ROW: f32 = 30.5;
        let quad = |painter: &mut dyn Painter, col: [f32; 4], t: f32| {
            painter.gl_color4f(gl, col[0], col[1], col[2], col[3]);
            painter.gl_quad(gl, l, t, w, ROW, false);
        };
        // the server's name, on two lines when it is long
        quad(painter, COL_DARK_BAR, t);
        let mut flag = false;
        let link = self.sim.online_link.clone();
        self.fonts[data_font].set_color(1.0, 1.0, 1.0, 1.0);
        if link.server_name.encode_utf16().count() <= 30 {
            self.fonts[data_font].blit_string(painter, l + 30.0, t + 6.1, &link.server_name, 1.0, ALIGN_LEFT);
        } else {
            let a = substr(&link.server_name, 0, 30);
            let b = substr(&link.server_name, 30, usize::MAX);
            self.fonts[data_font].blit_string(painter, l + 30.0, (t + 6.1) - 5.0, &a, 0.8, ALIGN_LEFT);
            self.fonts[data_font].blit_string(painter, l + 30.0, (t + 6.1) + 7.0, &b, 0.8, ALIGN_LEFT);
        }
        t = t + ROW;
        let inverted = self.sim.race.inverted_grid_positions;
        let col = if inverted != 0 {
            quad(painter, COL_DARK_TRANSP, t);
            flag = true;
            let text = format!("{} ( TOP {inverted} )", translate("Race x2: Reversed Grid"));
            self.fonts[data_font].set_color(1.0, 1.0, 1.0, 1.0);
            self.fonts[data_font].blit_string(painter, l + 30.0, t + 6.1, &text, 1.0, ALIGN_LEFT);
            t = t + ROW;
            COL_DARK_BAR
        } else {
            COL_DARK_TRANSP
        };
        quad(painter, col, t);
        flag = !flag;
        let assist = |v: i32| match v {
            0 => translate("No"),
            1 => translate("Factory"),
            2 => translate("Yes"),
            _ => String::new(),
        };
        let mut s = translate("TC: ");
        s.push_str(&assist(link.tc));
        s.push_str(&translate(" / ABS: "));
        s.push_str(&assist(link.abs));
        self.fonts[data_font].set_color(1.0, 1.0, 1.0, 1.0);
        self.fonts[data_font].blit_string(painter, l + 30.0, t + 6.1, &s, 1.0, ALIGN_LEFT);
        t = t + ROW;
        let rows = [
            translate(if link.stability { "Stability Control: Allowed" } else { "Stability Control: Not Allowed" }),
            translate(if link.auto_clutch { "Auto Clutch: Allowed" } else { "Auto Clutch: Not Allowed" }),
            format!("{}{}%", translate("Damage: "), cut(link.damage_rate * 100.0)),
            format!("{}{}%", translate("Fuel Rate: "), cut(link.fuel_rate * 100.0)),
            format!("{}{}%", translate("Tyre Wear Rate: "), cut(link.tyre_wear_rate * 100.0)),
            translate(if link.tyre_blankets { "Tyre Blankets: Yes" } else { "Tyre Blankets: No" }),
            match link.jump_start_mode {
                0 => translate("False Start: No"),
                1 => translate("False Start: Teleport To Pits"),
                2 => translate("False Start: Drive-through"),
                _ => String::new(),
            },
        ];
        for text in rows {
            // RaceControlSessionServer::setBackground 0x1400b7dc0
            quad(painter, if flag { COL_DARK_BAR } else { COL_DARK_TRANSP }, t);
            flag = !flag;
            self.fonts[data_font].set_color(1.0, 1.0, 1.0, 1.0);
            self.fonts[data_font].blit_string(painter, l + 30.0, t + 6.1, &text, 1.0, ALIGN_LEFT);
            t = t + ROW;
        }
    }

    // ----- RaceControlVoting -----

    pub fn voting(&self, id: Id) -> &Voting {
        match &self.controls[id].kind {
            Kind::Menu(menu) => match &**menu {
                Menu::Voting(v) => v,
                _ => panic!("control {id} is not a voting panel"),
            },
            _ => panic!("control {id} is not a voting panel"),
        }
    }

    pub fn voting_mut(&mut self, id: Id) -> &mut Voting {
        match &mut self.controls[id].kind {
            Kind::Menu(menu) => match &mut **menu {
                Menu::Voting(v) => v,
                _ => panic!("control {id} is not a voting panel"),
            },
            _ => panic!("control {id} is not a voting panel"),
        }
    }

    /// `RaceControlVoting::RaceControlVoting` 0x1400b8a50.
    pub fn new_race_control_voting(&mut self, painter: &mut dyn Painter) -> Id {
        let id = self.new_control(painter, "RACE_CONTROL_VOTING");
        self.control_set_size(id, 620.0, 200.0);
        self.fonts.push(Font::new(painter, "", FontType::Proportional, 14.0, true, true));
        let title_font = self.fonts.len() - 1;
        self.fonts.push(Font::new(painter, "", FontType::Monospaced, 20.0, false, false));
        let font = self.fonts.len() - 1;
        self.fonts[font].set_color4(&[1.0, 1.0, 1.0, 1.0]);
        self.controls[id].font = Some(font);
        self.controls[id].kind = Kind::Menu(Box::new(Menu::Voting(Voting {
            title_font,
            but_vote_next_session: 0,
            but_vote_restart: 0,
            but_vote_yes: 0,
            but_vote_no: 0,
            time_to_next_request: 0.0,
            session_request_count: 0,
        })));
        let mut buttons = [0; 4];
        for (i, (name, texture, text, x, y)) in [
            ("BUT_VOTE_NEXT_SESSION", "skip_icon", "Next Session", 45.0, 20.0),
            ("BUT_VOTE_RESTART_SESSION", "restart_icon", "Restart Session", 185.0, 20.0),
            ("BUT_VOTE_YES", "vote_yes", "Yes", 45.0, 100.0),
            ("BUT_VOTE_NO", "vote_no", "No", 185.0, 100.0),
        ]
        .into_iter()
        .enumerate()
        {
            let button = self.new_active_button(painter, name);
            buttons[i] = button;
            self.set_multiple_texture(painter, button, &format!("content/gui/raceControl/{texture}"));
            self.set_text(button, text);
            // (the last one is sized before it is placed)
            if i == 3 {
                self.set_size(button, 40.0, 40.0);
                self.set_position(button, x, y);
            } else {
                self.set_position(button, x, y);
                self.set_size(button, 40.0, 40.0);
            }
            let r = self.controls[button].rect;
            self.active_button_mut(button).text_coord = [0.0, r.bottom - r.top];
            self.controls[button].ev_clicked.push(Rc::new(move |gui: &mut Gui, _ev: &Clicked| gui.voting_clicked(id, i)));
            self.control_add_control(id, button);
            if i >= 2 {
                self.set_visible(button, false);
            }
        }
        let v = self.voting_mut(id);
        [v.but_vote_next_session, v.but_vote_restart, v.but_vote_yes, v.but_vote_no] = buttons;
        // (the handler of Sim::evOnNewSession, 0x1400b9520: see `sim_on_new_session`)
        self.sim.online_link.votings.push(id);
        id
    }

    /// The four buttons' lambdas (0x1400b9540, 0x1400b9590, 0x1400b95e0, 0x1400b9660).
    fn voting_clicked(&mut self, id: Id, button: usize) {
        match button {
            0 | 1 => {
                if self.vote_spam_check(id) {
                    self.sim.actions.push(SimAction::SendVote { kind: button as i32, value: true, guid: -1 });
                }
            }
            _ => {
                let vote = &self.sim.online_link.vote;
                let action = SimAction::SendVote { kind: vote.vote_type, value: button == 2, guid: vote.target_guid };
                self.sim.actions.push(action);
            }
        }
    }

    /// `RaceControlVoting::voteSpamCheck` 0x1400ba480: the n-th request locks the two request
    /// buttons for n(n+1)/2 minutes.
    fn vote_spam_check(&mut self, id: Id) -> bool {
        let now = self.sim.game_time;
        let v = self.voting_mut(id);
        if !(now >= v.time_to_next_request) {
            return false;
        }
        v.session_request_count = v.session_request_count.wrapping_add(1);
        v.time_to_next_request = 0.0;
        if v.session_request_count != 0 {
            let mut sum = 0.0f64;
            let mut k = 1u32;
            let mut step = 60000u32;
            loop {
                let value = step;
                step = step.wrapping_add(60000);
                k = k.wrapping_add(1);
                sum += value as f64;
                if k > v.session_request_count {
                    break;
                }
            }
            v.time_to_next_request = sum;
        }
        v.time_to_next_request = now + v.time_to_next_request;
        true
    }

    /// The panels' handler of `Sim::evOnNewSession` (0x1400b9520).
    pub fn votings_on_new_session(&mut self) {
        for id in self.sim.online_link.votings.clone() {
            let v = self.voting_mut(id);
            v.time_to_next_request = 0.0;
            v.session_request_count = 0;
        }
    }

    /// `RaceControlVoting::render` 0x1400b97b0.
    pub(crate) fn voting_render(&mut self, id: Id, dt: f32, painter: &mut dyn Painter) {
        self.control_render(id, dt, painter);
        let (l, mut t) = self.world_left_top(id);
        let own = self.controls[id].rect;
        let (title_font, next, restart, yes, no, time_to_next_request) = {
            let v = self.voting(id);
            (v.title_font, v.but_vote_next_session, v.but_vote_restart, v.but_vote_yes, v.but_vote_no, v.time_to_next_request)
        };
        let font = self.controls[id].font.unwrap_or(0);
        self.title_bar(painter, title_font, l, t, own.right - own.left, &translate("VOTING"));
        t = t + 20.0;
        let vote = self.sim.online_link.vote.clone();
        let tl = vote.time_left;
        let b = !(tl > 0.0);
        self.set_visible(restart, b);
        self.set_visible(next, b);
        let now = self.sim.game_time;
        if now < time_to_next_request && !(tl > 0.0) {
            self.set_visible(restart, false);
            self.set_visible(next, false);
            let w = own.right - own.left;
            self.fonts[font].blit_string(painter, w * 0.5 + l, t, &translate("Voting request is locked for"), 0.9, ALIGN_CENTER);
            let s = crate::esc_menu::time_to_wstring(cut_d(time_to_next_request - now), false, false);
            self.fonts[font].blit_string(painter, w * 0.5 + l, t + 20.0, &s, 1.0, ALIGN_CENTER);
        }
        let show = tl > 0.0 && !vote.has_voted;
        self.set_visible(no, show);
        self.set_visible(yes, show);
        if !(tl > 0.0) {
            return;
        }
        // (the bar and the prompt are placed by the control's own rectangle and the page's 100)
        let gl = GRAPHICS_GL;
        painter.gl_color4f(gl, COL_TITLE_BAR[0], COL_TITLE_BAR[1], COL_TITLE_BAR[2], COL_TITLE_BAR[3]);
        let wq = own.right - own.left;
        let bar = (vote.time_left * wq) / vote.max_time;
        painter.gl_quad(gl, ((wq * 0.5 + own.left) + 100.0) - wq * 0.5, own.top + 75.0, bar, 15.0, false);
        let x = (wq * 0.5 + own.left) + 100.0;
        self.fonts[font].blit_string(painter, x, own.top + 50.0, &translate("in progress"), 0.9, ALIGN_CENTER);
        let prompt = if !vote.has_voted {
            match vote.vote_type {
                0 => translate("Skip session?"),
                1 => translate("Restart session?"),
                2 => format!("{} {} ?", translate("Kick"), vote.target_name),
                _ => translate("Choose your vote"),
            }
        } else {
            match vote.vote_type {
                0 => translate("Voted on skipping session"),
                1 => translate("Voted on restarting session"),
                2 => format!("{} {}", translate("Voted on kicking"), vote.target_name),
                _ => translate("Vote Submitted"),
            }
        };
        self.fonts[font].blit_string(painter, x, own.top + 20.0, &prompt, 0.9, ALIGN_CENTER);
    }

    // ----- ACChat -----

    pub fn ac_chat(&self, id: Id) -> &AcChat {
        match &self.controls[id].kind {
            Kind::Menu(menu) => match &**menu {
                Menu::AcChat(c) => c,
                _ => panic!("control {id} is not a chat"),
            },
            _ => panic!("control {id} is not a chat"),
        }
    }

    pub fn ac_chat_mut(&mut self, id: Id) -> &mut AcChat {
        match &mut self.controls[id].kind {
            Kind::Menu(menu) => match &mut **menu {
                Menu::AcChat(c) => c,
                _ => panic!("control {id} is not a chat"),
            },
            _ => panic!("control {id} is not a chat"),
        }
    }

    /// `ACChat::ACChat` 0x140034aa0.
    pub fn new_ac_chat(&mut self, painter: &mut dyn Painter) -> Id {
        let id = self.new_control(painter, "ACChat");
        self.fonts.push(Font::new(painter, "", FontType::Monospaced, 14.0, true, true));
        let title_font = self.fonts.len() - 1;
        self.fonts.push(Font::new(painter, "", FontType::Monospaced, 14.0, false, true));
        let player_name_font = self.fonts.len() - 1;
        self.fonts.push(Font::new(painter, "", FontType::Proportional, 14.0, true, false));
        let player_text_font = self.fonts.len() - 1;
        self.control_set_size(id, 620.0, 340.0);
        // (the handler of ACClient::evOnChatMessage: see `chats_on_chat_message`)
        self.sim.online_link.chats.push(id);
        let inp_test = self.new_text_input(painter, "TEXT_INPUT");
        self.control_add_control(id, inp_test);
        if let Widget2::TextInput(input) = self.widget2_mut(inp_test) {
            // lambda 0x1400350f0: Enter sends the line (no echo: the server's comes back)
            input.handlers.push(Rc::new(move |gui: &mut Gui, ev: &WidgetEvent| {
                let WidgetEvent::Text { text, kind: 0, .. } = ev else { return };
                if !gui.controls[inp_test].text.is_empty() && gui.sim.online_link.is_allowed_to_send_chat_message(gui.sim.game_time) {
                    gui.sim.actions.push(SimAction::SendChat(text.clone()));
                    gui.set_text(inp_test, "");
                }
                gui.text_input_mut(inp_test).has_focus = true;
            }));
        }
        let texts = vec![ChatElement { player_name: String::new(), text: String::new(), color: [1.0, 1.0, 0.0, 1.0] }; 19];
        self.controls[id].kind = Kind::Menu(Box::new(Menu::AcChat(Box::new(AcChat {
            title_font,
            player_name_font,
            player_text_font,
            inp_test,
            player_name: "Me".into(),
            player_color: [1.0, 0.0, 0.0, 1.0],
            texts,
        }))));
        id
    }

    /// `ACChat::onChatMessage` 0x140035c00, for every chat (`ACClient::evOnChatMessage`, in
    /// the order they were made). `driver`: `getNetCarFromSessionID(session)`'s name, when
    /// there is such a car.
    pub fn chats_on_chat_message(&mut self, session: u32, message: &str, driver: Option<&str>) {
        for id in self.sim.online_link.chats.clone() {
            let c = self.ac_chat_mut(id);
            let n = c.texts.len();
            for i in 1..n {
                c.texts[i - 1] = c.texts[i].clone();
            }
            let player_name = c.player_name.clone();
            let player_color = c.player_color;
            let last = &mut c.texts[n - 1];
            if let Some(name) = driver {
                last.player_name = name.to_string();
                last.text = message.to_string();
            } else if session == 0xff {
                last.player_name = "SERVER".into();
                last.text = message.to_string();
                self.sim.actions.push(SimAction::AddMessage { title: format!("SERVER:{message}"), text: String::new() });
            } else {
                last.player_name = player_name;
                last.text = message.to_string();
                last.color = player_color;
            }
        }
    }

    /// `ACChat::render` 0x140036380: a title, the last 19 lines and the line to write in.
    pub(crate) fn ac_chat_render(&mut self, id: Id, dt: f32, painter: &mut dyn Painter) {
        self.control_render(id, dt, painter);
        let (l, mut t) = self.world_left_top(id);
        let own = self.controls[id].rect;
        let (title_font, name_font, text_font, inp_test) = {
            let c = self.ac_chat(id);
            (c.title_font, c.player_name_font, c.player_text_font, c.inp_test)
        };
        self.title_bar(painter, title_font, l, t, own.right - own.left, "CHAT");
        t = t + 20.0;
        let h = own.bottom - own.top;
        let input = self.controls[inp_test].rect;
        self.set_position(inp_test, 10.0, (h - (input.bottom - input.top)) - 10.0);
        self.set_size(inp_test, (own.right - own.left) - 20.0, 30.0);
        let texts = self.ac_chat(id).texts.clone();
        let mut yoff = 0.0f32;
        for line in &texts {
            self.fonts[name_font].set_color4(&line.color);
            let y = yoff + t;
            let x = l + 15.0;
            if !line.player_name.is_empty() {
                self.fonts[name_font].blit_string(painter, x, y + 2.0, &format!("{}:", line.player_name), 1.0, ALIGN_LEFT);
            }
            self.fonts[text_font].set_color(1.0, 1.0, 1.0, 1.0);
            let units = line.player_name.encode_utf16().count() as u64 + 1;
            let xt = ((self.fonts[name_font].scale as f64 * 0.57) * units as f64) as f32 + x;
            self.fonts[text_font].blit_string(painter, xt, y, &line.text, 1.0, ALIGN_LEFT);
            yoff += 15.0;
        }
    }
}
