// SPDX-License-Identifier: GPL-3.0-or-later

//! `TimeAttack`'s HUD (`TimeAttack::renderHUD` 0x1401bda40): the start picture, the status
//! panel with the time left and the points, the checkpoint panel and the penalties panel. The
//! mode's rules and the timers the HUD moves are `rustyac_physics::race::time_attack`'s; this
//! only draws what they left.

use crate::font::{Font, FontType};
use crate::gui::Gui;
use crate::menus::{PAUSE_MENU_PITS, PAUSE_MENU_RESUME};
use crate::painter::{Painter, Texture, ALIGN_CENTER, ALIGN_RIGHT, GL_LINE_STRIP, GRAPHICS_GL};

/// What the HUD reads of the mode.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TimeAttackLink {
    /// the track has sectors and the mode a budget (else "not available")
    pub available: bool,
    /// `TimeAttackGameStatus::mode`: 0 over, 1 on, 2 still
    pub mode: i32,
    pub time_left: f32,
    pub points: f32,
    /// the checkpoint panel shows (`scoreLabelTimer` under 5 s), with what the last
    /// checkpoint gave
    pub checkpoint_shown: bool,
    pub last_gained_points: f32,
    pub last_gained_seconds: f32,
    pub penalty_points_accumulated: f32,
}

pub struct TimeAttackHud {
    font: Font,
    start: Texture,
    checkpoint: Texture,
    white_clock: Texture,
    white_cup: Texture,
    black_clock: Texture,
    black_cup: Texture,
    penalty: Texture,
}

impl TimeAttackHud {
    /// The HUD's part of `TimeAttack::TimeAttack` 0x1401b7e20: the textures, the font, the
    /// pause menu's Pits button blanked for the whole mode.
    pub fn new(gui: &mut Gui, painter: &mut dyn Painter) -> TimeAttackHud {
        if let Some(pause) = gui.sim.pause_menu {
            gui.pause_menu_blank_button(pause, PAUSE_MENU_PITS, true);
        }
        let mut texture = |name: &str| painter.get_texture(&format!("content/gui/timeAttack/{name}.png"));
        let (start, checkpoint, white_clock, white_cup) = (texture("timeattack_start"), texture("checkpoint"), texture("time_icon"), texture("points_icon"));
        let (black_clock, black_cup, penalty) = (texture("small_time_icon"), texture("small_points_icon"), texture("penalty_icon"));
        let mut font = Font::new(painter, "", FontType::Monospaced, 45.0, false, false);
        font.shadowed = true;
        TimeAttackHud { font, start, checkpoint, white_clock, white_cup, black_clock, black_cup, penalty }
    }

    fn picture(painter: &mut dyn Painter, texture: &Texture, x: f32, y: f32) {
        let (w, h) = painter.texture_size(texture);
        painter.set_texture(0, texture);
        painter.gl_color4f(GRAPHICS_GL, 1.0, 1.0, 1.0, 1.0);
        painter.gl_quad(GRAPHICS_GL, x, y, w as f32, h as f32, true);
    }

    fn frame(painter: &mut dyn Painter, x: f32, w: f32) {
        painter.gl_color4f(GRAPHICS_GL, 0.0, 0.0, 0.0, 0.5);
        painter.gl_quad(GRAPHICS_GL, x, 80.0, w, 85.0, false);
        painter.gl_color4f(GRAPHICS_GL, 0.6, 0.6, 0.6, 0.5);
        painter.gl_begin(GRAPHICS_GL, GL_LINE_STRIP);
        for (px, py) in [(x, 80.0), (x + w, 80.0), (x + w, 165.0), (x, 165.0), (x, 80.0)] {
            painter.gl_vertex3f(GRAPHICS_GL, px, py, 0.0);
        }
        painter.gl_end(GRAPHICS_GL);
    }

    /// `TimeAttack::renderHUD`: nothing while the ESC menu, the pause menu or a replay is up.
    pub fn render_hud(&mut self, gui: &mut Gui, painter: &mut dyn Painter, ta: &TimeAttackLink) {
        let half = painter.screen().0 as f32 * 0.5;
        if !ta.available {
            self.font.set_color(1.0, 0.0, 0.0, 1.0);
            self.font.blit_string(painter, half, 100.0, "TIME ATTACK is not available for this track ", 0.8, ALIGN_CENTER);
            return;
        }
        let visible = |id: Option<crate::control::Id>| id.is_some_and(|id| gui.controls[id].visible);
        if visible(gui.sim.esc_menu) || gui.sim.in_replay || visible(gui.sim.pause_menu) {
            return;
        }
        match ta.mode {
            // renderGameOver 0x1401bc470
            0 => {
                if let Some(pause) = gui.sim.pause_menu {
                    gui.pause_menu_blank_button(pause, PAUSE_MENU_PITS, true);
                    gui.pause_menu_blank_button(pause, PAUSE_MENU_RESUME, true);
                }
            }
            1 => self.render_status(painter, half, ta),
            // renderGuiStart 0x1401bcff0
            _ => {
                let w = painter.texture_size(&self.start).0 as f32;
                Self::picture(painter, &self.start, half - w * 0.5, 80.0);
            }
        }
    }

    /// `renderGuiStatus` 0x1401bd120 with `renderGuiCheckpointBreak` 0x1401bc4c0 and
    /// `renderGuiPenalties` 0x1401bcb90.
    fn render_status(&mut self, painter: &mut dyn Painter, half: f32, ta: &TimeAttackLink) {
        if ta.checkpoint_shown {
            let (w, h) = painter.texture_size(&self.checkpoint);
            Self::picture(painter, &self.checkpoint, half - w as f32 * 0.5, 165.0);
            painter.gl_color4f(GRAPHICS_GL, 1.0, 1.0, 1.0, 0.5);
            painter.gl_quad(GRAPHICS_GL, half - 270.0, h as f32 + 165.0 + 2.0, 540.0, 60.0, false);
            let y = h as f32 + 165.0 + 10.0;
            Self::picture(painter, &self.black_clock, half - 150.0, y);
            Self::picture(painter, &self.black_cup, half + 60.0, y);
            let seconds = format!("{}\"", crate::widgets2::format_fixed(ta.last_gained_seconds as f64, 1));
            if ta.last_gained_seconds <= 0.0 {
                self.font.set_color(1.0, 0.0, 0.0, 1.0);
                self.font.blit_string(painter, half - 20.0, y, &seconds, 0.6, ALIGN_RIGHT);
            } else {
                self.font.set_color(0.0, 1.0, 0.0, 1.0);
                self.font.blit_string(painter, half - 20.0, y, &format!("+{seconds}"), 0.6, ALIGN_RIGHT);
            }
            self.font.set_color(0.0, 1.0, 0.0, 1.0);
            self.font.blit_string(painter, half + 200.0, y, &format!("+{}pt", ta.last_gained_points as i32), 0.6, ALIGN_RIGHT);
        }
        if ta.penalty_points_accumulated < 0.0 {
            Self::frame(painter, half + 275.0, 250.0);
            Self::picture(painter, &self.penalty, half + 280.0, 95.0);
            self.font.set_color(1.0, 0.0, 0.0, 1.0);
            self.font.blit_string(painter, half + 500.0, 95.0, &(ta.penalty_points_accumulated as i32).to_string(), 1.0, ALIGN_RIGHT);
        }
        Self::frame(painter, half - 270.0, 540.0);
        painter.gl_begin(GRAPHICS_GL, GL_LINE_STRIP);
        painter.gl_vertex3f(GRAPHICS_GL, half, 95.0, 0.0);
        painter.gl_vertex3f(GRAPHICS_GL, half, 150.0, 0.0);
        painter.gl_end(GRAPHICS_GL);
        Self::picture(painter, &self.white_clock, half - 240.0, 90.0);
        Self::picture(painter, &self.white_cup, half + 30.0, 90.0);
        // shortcut: the game leaves the text unwritten when no time is left (never drawn so)
        self.font.set_color(1.0, 1.0, 1.0, 1.0);
        self.font.blit_string(painter, half - 10.0, 100.0, &format!("{}\"", crate::widgets2::format_fixed(ta.time_left as f64, 3)), 0.8, ALIGN_RIGHT);
        let red = if ta.points >= 0.0 { 1.0 } else { 0.0 };
        self.font.set_color(1.0, red, red, 1.0);
        self.font.blit_string(painter, half + 260.0, 100.0, &format!("{} pt", ta.points as i32), 0.8, ALIGN_RIGHT);
    }
}
