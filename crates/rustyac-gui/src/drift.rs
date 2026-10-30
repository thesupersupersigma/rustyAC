// SPDX-License-Identifier: GPL-3.0-or-later

//! `DriftMode`'s HUD (`DriftMode::renderHUD` 0x14008b7c0): the start picture, the time and
//! points panel, the level bar, the rising bonus texts, the drift-in-progress and combo
//! panels. The rules and timers are `rustyac_physics::race::drift`'s; this draws what one
//! of its `hud` calls returned.

use crate::font::{Font, FontType};
use crate::gui::Gui;
use crate::menus::{PAUSE_MENU_PITS, PAUSE_MENU_RESUME};
use crate::painter::{Painter, Texture, ALIGN_CENTER, ALIGN_LEFT, ALIGN_RIGHT, GL_LINE_STRIP, GRAPHICS_GL};
use rustyac_physics::race::drift::DriftDraw;

/// What the HUD and the end screen read of a drift session's mode.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct DriftLink {
    /// `DriftMode::sessionPoints`
    pub session_points: f32,
    /// what the mode's last `renderHUD` drew
    pub drawn: Option<DriftDraw>,
}

pub struct DriftHud {
    font: Font,
    white_clock: Texture,
    white_cup: Texture,
    instant_drift: Texture,
    instant_drift_extreme: Texture,
    level_up: Texture,
    combo: Texture,
    start: Texture,
}

impl DriftHud {
    /// The HUD's part of `DriftMode::DriftMode` 0x140089610 (the textures and the font).
    pub fn new(painter: &mut dyn Painter) -> DriftHud {
        let mut texture = |name: &str| painter.get_texture(&format!("content/gui/drift/{name}.png"));
        // the constructor also loads star.png, which nothing draws
        let _star = texture("star");
        let (white_clock, white_cup, instant_drift, instant_drift_extreme) = (texture("time_icon"), texture("points_icon"), texture("instantDrift"), texture("instantDriftExtreme"));
        let (level_up, combo, start) = (texture("levelup"), texture("combo"), texture("drift_start"));
        let mut font = Font::new(painter, "", FontType::Monospaced, 35.0, false, false);
        font.shadowed = true;
        DriftHud { font, white_clock, white_cup, instant_drift, instant_drift_extreme, level_up, combo, start }
    }

    fn picture(painter: &mut dyn Painter, texture: &Texture, x: f32, y: f32, size: (u32, u32)) {
        painter.set_texture(0, texture);
        painter.gl_color4f(GRAPHICS_GL, 1.0, 1.0, 1.0, 1.0);
        painter.gl_quad(GRAPHICS_GL, x, y, size.0 as f32, size.1 as f32, true);
    }

    fn outline(painter: &mut dyn Painter, points: [(f32, f32); 5]) {
        painter.gl_color4f(GRAPHICS_GL, 0.6, 0.6, 0.6, 0.5);
        painter.gl_begin(GRAPHICS_GL, GL_LINE_STRIP);
        for (x, y) in points {
            painter.gl_vertex3f(GRAPHICS_GL, x, y, 0.0);
        }
        painter.gl_end(GRAPHICS_GL);
    }

    fn panel(painter: &mut dyn Painter, x: f32, w: f32) {
        painter.gl_color4f(GRAPHICS_GL, 0.0, 0.0, 0.0, 0.5);
        painter.gl_quad(GRAPHICS_GL, x, 80.0, w, 85.0, false);
    }

    /// `renderHUD`'s drawing (`drawn`: `None` while the ESC menu, the pause menu or a replay
    /// is up, or in the frame the clock ran out).
    pub fn render_hud(&mut self, gui: &mut Gui, painter: &mut dyn Painter, link: &DriftLink) {
        let Some(drawn) = link.drawn else { return };
        let screen_w = painter.screen().0;
        let cx = screen_w as f32 * 0.5;
        let blank = |gui: &mut Gui, pits: bool, resume: bool| {
            if let Some(pause) = gui.sim.pause_menu {
                gui.pause_menu_blank_button(pause, PAUSE_MENU_PITS, pits);
                gui.pause_menu_blank_button(pause, PAUSE_MENU_RESUME, resume);
            }
        };
        match &drawn {
            // modeGameOver 0x14008b750
            DriftDraw::Over => blank(gui, true, true),
            DriftDraw::Start => {
                blank(gui, true, false);
                let size = painter.texture_size(&self.start);
                Self::picture(painter, &self.start, cx - size.0 as f32 * 0.5, 80.0, size);
            }
            DriftDraw::Drifting { bonuses, level, current_points_per_level, points_per_level, level_up, seconds, session_points, instant_drift, combo, bonus_on } => {
                // 1: animateBonuses 0x14008a0e0
                self.font.set_color(0.0, 1.0, 0.0, 1.0);
                for (i, bonus) in bonuses.iter().enumerate() {
                    let Some(bonus) = bonus else { continue };
                    let text = if i == 0 { format!("+{}\"", bonus.value) } else { format!("+{} pt", bonus.value) };
                    self.font.blit_string(painter, bonus.x + cx, bonus.y, &text, 1.0, ALIGN_RIGHT);
                }
                // 2-3: the main panel, the level bar's back
                Self::panel(painter, cx - 270.0, 540.0);
                Self::outline(painter, [(cx - 270.0, 80.0), (cx + 270.0, 80.0), (cx + 270.0, 165.0), (cx - 270.0, 165.0), (cx - 270.0, 80.0)]);
                painter.gl_color4f(GRAPHICS_GL, 0.0, 0.0, 0.0, 0.5);
                painter.gl_quad(GRAPHICS_GL, cx - 270.0, 167.0, 539.0, 20.0, false);
                // 5-6: the points earned in the level, then the drift in progress
                let w1 = current_points_per_level / points_per_level * 538.0;
                painter.gl_color4f(GRAPHICS_GL, 0.2, 0.7, 0.2, 0.6);
                painter.gl_quad(GRAPHICS_GL, cx - 270.0 + 1.0, 168.0, w1, 18.0, false);
                let rem = 538.0 - w1;
                let mut w2 = (instant_drift / points_per_level).clamp(0.0, 1.0) * 537.0;
                if w2 > rem {
                    w2 = rem;
                    painter.gl_color4f(GRAPHICS_GL, 1.0, 1.0, 0.2, 0.6);
                } else {
                    painter.gl_color4f(GRAPHICS_GL, 0.7, 0.7, 0.2, 0.6);
                }
                painter.gl_quad(GRAPHICS_GL, cx - 270.0 + 1.0 + w1, 168.0, w2, 18.0, false);
                // 7-8: the level, updateLevelUp 0x14008ba10
                self.font.set_color(1.0, 1.0, 1.0, 0.7);
                self.font.blit_string(painter, (screen_w / 2) as f32, 167.0, &format!("Level {}", level + 1), 0.5, ALIGN_CENTER);
                if *level_up {
                    let size = painter.texture_size(&self.level_up);
                    Self::picture(painter, &self.level_up, cx - (size.0 >> 1) as f32, 167.0, size);
                }
                // 9-15: the divider, the icons, the time and the points
                painter.gl_color4f(GRAPHICS_GL, 0.6, 0.6, 0.6, 0.5);
                painter.gl_begin(GRAPHICS_GL, GL_LINE_STRIP);
                painter.gl_vertex3f(GRAPHICS_GL, cx, 95.0, 0.0);
                painter.gl_vertex3f(GRAPHICS_GL, cx, 150.0, 0.0);
                painter.gl_end(GRAPHICS_GL);
                let (clock, cup) = (painter.texture_size(&self.white_clock), painter.texture_size(&self.white_cup));
                Self::picture(painter, &self.white_clock, cx - 240.0, 90.0, clock);
                Self::picture(painter, &self.white_cup, cx + 30.0, 90.0, cup);
                let c = if *seconds < 6 { 0.0 } else { 1.0 };
                self.font.set_color(1.0, c, c, 1.0);
                self.font.blit_string(painter, cx - 10.0, 100.0, &format!("{seconds}\""), 1.0, ALIGN_RIGHT);
                self.font.set_color(1.0, 1.0, 1.0, 1.0);
                self.font.blit_string(painter, cx + 260.0, 100.0, &format!("{session_points} pt"), 1.0, ALIGN_RIGHT);
                let icon = painter.texture_size(&self.instant_drift);
                // 16: the drift in progress
                if *instant_drift as i32 != 0 {
                    Self::panel(painter, cx + 275.0, 250.0);
                    Self::outline(painter, [(cx + 275.0, 80.0), (cx + 525.0, 80.0), (cx + 525.0, 165.0), (cx + 275.0, 165.0), (cx + 275.0, 80.0)]);
                    if *bonus_on {
                        Self::picture(painter, &self.instant_drift_extreme, cx + 420.0, 90.0, icon);
                    }
                    self.font.set_color(1.0, 1.0, 0.0, 1.0);
                    self.font.blit_string(painter, cx + 280.0, 100.0, &format!("+ {}", *instant_drift as i32), 1.0, ALIGN_LEFT);
                }
                // 17: the combo
                if *combo > 1 {
                    Self::panel(painter, cx - 275.0 - 250.0, 250.0);
                    Self::outline(painter, [(cx - 275.0, 80.0), (cx - 525.0, 80.0), (cx - 525.0, 165.0), (cx - 275.0, 165.0), (cx - 275.0, 80.0)]);
                    Self::picture(painter, &self.combo, cx - 515.0, 90.0, icon);
                    self.font.set_color(1.0, 1.0, 1.0, 1.0);
                    self.font.blit_string(painter, cx - 430.0, 95.0, &format!(" x {combo}"), 1.15, ALIGN_LEFT);
                }
                self.font.set_color(1.0, 1.0, 1.0, 1.0);
            }
        }
    }
}
