// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! What an online session draws over everything else: the server's welcome message (until a
//! click), the red screen that counts a kicked driver out (`ACClient::renderHUD` 0x140051650),
//! and the ping in the top right corner (`QOSOverlay`, 0x68 bytes: constructor 0x1400ac470,
//! `renderHUD` 0x1400ac5d0).

use crate::font::{Font, FontType};
use crate::gui::Gui;
use crate::painter::{Painter, ALIGN_CENTER, ALIGN_LEFT, GRAPHICS_GL};

/// `ACClient`, as far as its own `renderHUD` goes.
pub struct ClientHud {
    /// `ACClient::font` (Segoe UI, 32)
    font: usize,
}

/// `QOSOverlay`.
pub struct QosOverlay {
    /// Segoe UI, 16
    font: usize,
}

/// `cvttsd2si`.
fn cut(x: f64) -> i32 {
    if x > -2147483649.0 && x < 2147483648.0 {
        x as i32
    } else {
        i32::MIN
    }
}

impl ClientHud {
    pub fn new(gui: &mut Gui, painter: &mut dyn Painter) -> ClientHud {
        gui.fonts.push(Font::new(painter, "", FontType::Proportional, 32.0, false, false));
        ClientHud { font: gui.fonts.len() - 1 }
    }

    /// `ACClient::renderHUD` 0x140051650. `welcome`: `welcomeMessage` (the server's text; any
    /// mouse button pressed on the window empties it); `disconnect`: `dcCountdown.message`
    /// while the countdown runs; `now`: the game's clock, ms.
    pub fn render_hud(&self, gui: &mut Gui, painter: &mut dyn Painter, welcome: &str, disconnect: Option<&str>, now: f64) {
        let gl = GRAPHICS_GL;
        let (width, height) = painter.screen();
        if !welcome.is_empty() {
            painter.gl_color4f(gl, 0.0, 0.0, 0.0, 0.85);
            painter.gl_quad(gl, 100.0, 100.0, (width - 200) as f32, (height - 150) as f32, false);
            gui.fonts[self.font].set_color(1.0, 1.0, 1.0, 1.0);
            gui.fonts[self.font].blit_string(painter, 150.0, 150.0, welcome, 0.8, ALIGN_LEFT);
            gui.fonts[self.font].blit_string(painter, width as f32 * 0.5, (height - 100) as f32, "click to close", 0.5, ALIGN_CENTER);
        }
        if let Some(message) = disconnect {
            painter.gl_color4f(gl, 0.5, 0.0, 0.0, 0.8);
            painter.gl_quad(gl, 0.0, 0.0, width as f32, height as f32, false);
            // white in the even seconds, yellow in the odd ones
            let t = cut(now * 0.001);
            let b = if t % 2 == 0 { 1.0 } else { 0.0 };
            gui.fonts[self.font].set_color(1.0, 1.0, b, 1.0);
            gui.fonts[self.font].blit_string(painter, width as f32 * 0.5, height as f32 * 0.5, message, 1.0, ALIGN_CENTER);
        }
    }
}

impl QosOverlay {
    pub fn new(gui: &mut Gui, painter: &mut dyn Painter) -> QosOverlay {
        gui.fonts.push(Font::new(painter, "", FontType::Proportional, 16.0, false, false));
        QosOverlay { font: gui.fonts.len() - 1 }
    }

    /// `QOSOverlay::renderHUD` 0x1400ac5d0: "P:" and the ping to the server (`ACClient::
    /// getPing(null)`), green up to 70 ms, yellow up to 120, red above.
    pub fn render_hud(&self, gui: &mut Gui, painter: &mut dyn Painter, ping: i32) {
        let font = &mut gui.fonts[self.font];
        font.set_color(1.0, 1.0, 0.0, 1.0);
        if ping <= 70 {
            font.set_color(0.0, 1.0, 0.0, 1.0);
        } else if ping <= 120 {
            font.set_color(1.0, 1.0, 0.0, 1.0);
        } else {
            font.set_color(1.0, 0.0, 0.0, 1.0);
        }
        let x = (painter.screen().0 - 50) as f32;
        font.blit_string(painter, x, 0.0, &format!("P:{ping}"), 1.0, ALIGN_LEFT);
    }
}
