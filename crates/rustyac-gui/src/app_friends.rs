// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! `FriendsLeaderboardDisplayer` (0x270 bytes), the app "Friends Leaderboard": the Steam
//! leaderboard of the player's friends for a hot lap, time attack or drift session with
//! penalties on, with a button to reset the player's score and one to load the board again.
//!
//! `FriendsLeaderboardDisplayer::FriendsLeaderboardDisplayer` 0x140324fc0, `render`
//! 0x140326540. The rows come only from Steam (`SteamInterface::evOnLeaderboardUpdated`); rustyAC
//! has no Steam, so the board stays empty, as in a game whose Steam requests never answer: the
//! two buttons start nothing and the app shows the gates' texts ("NO LEADERBOARDS FOUND" in a
//! session that has a board).

use std::any::Any;
use std::rc::Rc;

use crate::apps::App;
use crate::control::{Clicked, Id};
use crate::font::{Font, FontType};
use crate::gui::Gui;
use crate::hud::translate;
use crate::painter::{Painter, Texture, ALIGN_CENTER, ALIGN_LEFT, ALIGN_RIGHT};

pub struct Friends {
    pub reset_button: Id,
    pub refresh_button: Id,
    pub tex_up: Texture,
    pub tex_mark: Texture,
    pub tex_down: Texture,
    /// the form's own font (Segoe UI 17)
    pub font: usize,
}

impl Gui {
    /// `FriendsLeaderboardDisplayer::FriendsLeaderboardDisplayer(Sim*)` 0x140324fc0.
    pub fn new_friends_app(&mut self, painter: &mut dyn Painter) -> Id {
        let form = self.new_form(painter, "FRIENDS_LEADERBOARD", false);
        self.form_data_mut(form).dev_app = false;
        self.form_set_title(form, &translate("Friends Leaderboard"));
        // (SteamInterface::evOnLeaderboardUpdated's handler: never called without Steam)
        self.fonts.push(Font::new(painter, "", FontType::Proportional, 17.0, false, false));
        let font = self.fonts.len() - 1;
        self.controls[form].font = Some(font);
        self.fonts[font].set_color(1.0, 0.0, 0.0, 1.0);
        self.set_size(form, 450.0, 250.0);
        let f = self.controls[form].rect;
        let reset = self.new_active_button(painter, "RESET_BUTTON");
        self.set_multiple_texture(painter, reset, "content/gui/friendsLeaderboardDisplayer/reset");
        self.set_size(reset, 50.0, 50.0);
        self.set_text(reset, &translate("Reset my score"));
        let r = self.controls[reset].rect;
        self.set_position(reset, (f.right - f.left) - (r.right - r.left), (f.bottom - f.top) - (r.bottom - r.top));
        self.active_button_mut(reset).text_coord = [-25.0, 10.0];
        if let Some(bf) = self.controls[reset].font {
            self.fonts[bf].scale = 20.0;
        }
        self.controls[reset].font_align = ALIGN_RIGHT;
        // SteamInterface::resetPlayerLeaderboard: a Steam request (none here)
        self.controls[reset].ev_clicked.push(Rc::new(|_: &mut Gui, _: &Clicked| {}));
        self.add_control(form, reset);
        let refresh = self.new_active_button(painter, "REFRESH_BUTTON");
        self.set_multiple_texture(painter, refresh, "content/gui/friendsLeaderboardDisplayer/refresh");
        self.set_size(refresh, 50.0, 50.0);
        let r = self.controls[reset].rect;
        self.set_position(refresh, 0.0, (f.bottom - f.top) - (r.bottom - r.top));
        self.set_text(refresh, &translate("Refresh"));
        self.active_button_mut(refresh).text_coord = [25.0, 10.0];
        if let Some(bf) = self.controls[refresh].font {
            self.fonts[bf].scale = 20.0;
        }
        self.controls[refresh].font_align = ALIGN_LEFT;
        // SteamInterface::refreshPlayerLeaderboard: a Steam request (none here)
        self.controls[refresh].ev_clicked.push(Rc::new(|_: &mut Gui, _: &Clicked| {}));
        self.add_control(form, refresh);
        let tex_up = painter.get_texture("content/gui/friendsLeaderboardDisplayer/up.png");
        let tex_down = painter.get_texture("content/gui/friendsLeaderboardDisplayer/down.png");
        let tex_mark = painter.get_texture("content/gui/friendsLeaderboardDisplayer/marker.png");
        self.set_app(form, Box::new(Friends { reset_button: reset, refresh_button: refresh, tex_up, tex_mark, tex_down, font }));
        form
    }
}

impl App for Friends {
    fn as_any(&mut self) -> &mut dyn Any {
        self
    }

    /// `FriendsLeaderboardDisplayer::render` 0x140326540 with the board empty.
    fn render(&mut self, gui: &mut Gui, id: Id, dt: f32, painter: &mut dyn Painter) {
        gui.control_render(id, dt, painter);
        let r = gui.controls[id].rect;
        let ty = gui.apps.session_type;
        let cy = (r.bottom - r.top) * f32::from_bits(0x3eaa_aaab) + r.top;
        let cx = (r.right - r.left) * 0.5 + r.left;
        let text = if (ty.wrapping_sub(4) as u32) > 2 {
            "LEADERBOARDS N.A. FOR CURRENT MODE"
        } else if !gui.apps.penalties_enabled {
            "ACTIVATE PENALTIES TO COMPETE"
        } else {
            // (leaderboardElements: filled by Steam only)
            "NO LEADERBOARDS FOUND"
        };
        gui.fonts[self.font].blit_string(painter, cx, cy, &translate(text), 1.0, ALIGN_CENTER);
        let _ = (&self.tex_up, &self.tex_mark, &self.tex_down, self.reset_button, self.refresh_button);
    }
}
