// SPDX-License-Identifier: GPL-3.0-or-later

//! `DragMode`'s HUD (`DragMode::renderHUD` 0x140088730): the time panel with the strip and
//! the two cars on it, the reaction time and the finish speed, the false start and
//! disqualified pictures. The rules are `rustyac_physics::race::drag`'s; this draws a copy
//! of its state.

use crate::font::{Font, FontType};
use crate::gui::Gui;
use crate::menus::PAUSE_MENU_PITS;
use crate::painter::{Painter, Texture, ALIGN_CENTER, ALIGN_LEFT, BLEND_ALPHA, GL_LINE_STRIP, GRAPHICS_GL};

/// What the HUD and the end screen read of a drag session's mode (the player's car first).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct DragLink {
    /// `DragMode::status`: 0 game over, 1 race on, 2 suspended, 3 still
    pub status: i32,
    pub wins: [u32; 2],
    pub false_start: [bool; 2],
    pub disqualified: [bool; 2],
    /// the player's `playerTime`, `reactionTime` (s) and `finalSpeed`
    pub player_time: f32,
    pub reaction_time: f32,
    pub final_speed: i32,
    /// where the two cars are along the strip, 0..1 (`worldToSpline`, as `renderDragGUI` does)
    pub perc: [f32; 2],
}

pub struct DragHud {
    font: Font,
    tree: Texture,
    time: Texture,
    player_car: Texture,
    ai_car: Texture,
    track: Texture,
    finish_speed: Texture,
    false_start: Texture,
    disqualified: Texture,
}

impl DragHud {
    /// The HUD's part of `DragMode::DragMode` 0x140084a20 (the font, the textures, the pause
    /// menu's Pits blanked).
    pub fn new(gui: &mut Gui, painter: &mut dyn Painter) -> DragHud {
        let font = Font::new(painter, "", FontType::Monospaced, 45.0, false, false);
        let mut texture = |name: &str| painter.get_texture(&format!("content/gui/drag/{name}.png"));
        let (tree, time, player_car, ai_car) = (texture("xmas_tree"), texture("time_icon"), texture("player_car"), texture("ai_car"));
        let (track, finish_speed, false_start, disqualified) = (texture("race_straightline"), texture("speed_icon"), texture("false_start"), texture("disqualified"));
        if let Some(pause) = gui.sim.pause_menu {
            gui.pause_menu_blank_button(pause, PAUSE_MENU_PITS, true);
        }
        DragHud { font, tree, time, player_car, ai_car, track, finish_speed, false_start, disqualified }
    }

    fn picture(painter: &mut dyn Painter, texture: &Texture, x: f32, y: f32) {
        let (w, h) = painter.texture_size(texture);
        painter.set_texture(0, texture);
        painter.gl_quad(GRAPHICS_GL, x, y, w as f32, h as f32, true);
    }

    fn line(painter: &mut dyn Painter, points: &[(f32, f32)]) {
        painter.gl_color4f(GRAPHICS_GL, 0.6, 0.6, 0.6, 0.5);
        painter.gl_begin(GRAPHICS_GL, GL_LINE_STRIP);
        for &(x, y) in points {
            painter.gl_vertex3f(GRAPHICS_GL, x, y, 0.0);
        }
        painter.gl_end(GRAPHICS_GL);
    }

    fn panel(painter: &mut dyn Painter, x: f32, w: f32) {
        painter.gl_color4f(GRAPHICS_GL, 0.0, 0.0, 0.0, 0.5);
        painter.gl_quad(GRAPHICS_GL, x, 80.0, w, 85.0, false);
    }

    /// `renderHUD`: nothing while the pause menu or a replay is up, or while the cars stand.
    pub fn render_hud(&mut self, gui: &mut Gui, painter: &mut dyn Painter, link: &DragLink) {
        if gui.sim.in_replay || gui.sim.pause_menu.is_some_and(|pause| gui.controls[pause].visible) {
            return;
        }
        match link.status {
            0 | 1 => self.render_gui(painter, link),
            2 => {
                self.render_gui(painter, link);
                self.render_penalty(painter, link);
            }
            _ => {}
        }
    }

    /// `renderDragGUI` 0x140087460
    fn render_gui(&mut self, painter: &mut dyn Painter, link: &DragLink) {
        let cx = painter.screen().0 as f32 * 0.5;
        Self::panel(painter, cx - 270.0, 540.0);
        Self::line(painter, &[(cx - 270.0, 80.0), (cx + 270.0, 80.0), (cx + 270.0, 165.0), (cx - 270.0, 165.0), (cx - 270.0, 80.0)]);
        painter.set_blend_mode(BLEND_ALPHA);
        painter.gl_color4f(GRAPHICS_GL, 1.0, 1.0, 1.0, 1.0);
        let xt = cx - 260.0;
        Self::picture(painter, &self.time, xt, 90.0);
        let time_w = painter.texture_size(&self.time).0;
        self.font.set_color(1.0, 1.0, 1.0, 1.0);
        self.font.blit_string(painter, time_w as f32 + xt + 10.0, 100.0, &if link.player_time < 0.0 { String::new() } else { format!("{}\"", crate::widgets2::format_fixed(link.player_time as f64, 1)) }, 1.0, ALIGN_LEFT);
        let x2 = time_w.wrapping_mul(4) as f32 + xt + 10.0;
        Self::line(painter, &[(x2, 95.0), (x2, 150.0)]);
        painter.gl_color4f(GRAPHICS_GL, 1.0, 1.0, 1.0, 1.0);
        Self::picture(painter, &self.track, x2, 85.0);
        let track_w = painter.texture_size(&self.track).0 as i32;
        let along = |perc: f32| ((track_w - 0x3c) as f32 * perc + x2 + 40.0) as i32 as f32;
        Self::picture(painter, &self.player_car, along(link.perc[0]), 100.0);
        self.font.blit_string(painter, x2 + 10.0, 95.0, &link.wins[0].to_string(), 0.5, ALIGN_LEFT);
        Self::picture(painter, &self.ai_car, along(link.perc[1]), 125.0);
        self.font.blit_string(painter, x2 + 10.0, 115.0, &link.wins[1].to_string(), 0.5, ALIGN_LEFT);
        if link.reaction_time > 0.0 {
            let xr = cx - 275.0;
            Self::panel(painter, xr - 250.0, 250.0);
            Self::line(painter, &[(xr, 80.0), (cx - 525.0, 80.0), (cx - 525.0, 165.0), (xr, 165.0), (xr, 80.0)]);
            painter.gl_color4f(GRAPHICS_GL, 1.0, 1.0, 1.0, 1.0);
            Self::picture(painter, &self.tree, cx - 515.0, 90.0);
            painter.gl_color4f(GRAPHICS_GL, 1.0, 0.0, 0.0, 1.0);
            self.font.set_color(1.0, 0.0, 0.0, 1.0);
            self.font.blit_string(painter, cx - 400.0, 95.0, &crate::widgets2::format_fixed(link.reaction_time as f64, 3), 0.9, ALIGN_CENTER);
        }
        if link.final_speed != 0 {
            let xs = cx + 275.0;
            Self::panel(painter, xs, 250.0);
            Self::line(painter, &[(xs, 80.0), (cx + 525.0, 80.0), (cx + 525.0, 165.0), (xs, 165.0), (xs, 80.0)]);
            painter.gl_color4f(GRAPHICS_GL, 1.0, 1.0, 1.0, 1.0);
            Self::picture(painter, &self.finish_speed, cx + 290.0, 90.0);
            let speed_w = painter.texture_size(&self.finish_speed).0;
            self.font.set_color(1.0, 0.0, 0.0, 1.0);
            self.font.blit_string(painter, speed_w as f32 + cx + 290.0 + 20.0, 95.0, &link.final_speed.to_string(), 0.9, ALIGN_LEFT);
        }
    }

    /// `renderDragPenalty` 0x140088380
    fn render_penalty(&mut self, painter: &mut dyn Painter, link: &DragLink) {
        painter.gl_color4f(GRAPHICS_GL, 1.0, 1.0, 1.0, 1.0);
        let half = painter.screen().0 / 2;
        for (on, texture) in [(link.false_start, &self.false_start), (link.disqualified, &self.disqualified)] {
            if on[0] || on[1] {
                let w = painter.texture_size(texture).0;
                Self::picture(painter, texture, (half - (w >> 1) as i32) as f32, 165.0);
            }
        }
    }
}
