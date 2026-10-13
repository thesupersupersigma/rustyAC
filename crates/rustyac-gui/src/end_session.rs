// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! The end of a session on the screen: `EndSessionDisplayer` (0x130 bytes, a game object that
//! draws in the HUD pass) with its `EndSessionCommonGui` (0x1c0: the dark overlay, the logo,
//! the bar "SELECT" and the buttons Exit, Restart, Replay) and the results in an
//! `ACStandardLeaderboard`.
//!
//! `EndSessionDisplayer::EndSessionDisplayer` 0x14010a650, `update` 0x14010bf90,
//! `displayResults` 0x14010b260, `hide` 0x14010b380, `renderHUD` 0x14010b520, its four
//! handlers of the window's mouse events (0x14010ae00 … 0x14010ae60);
//! `EndSessionCommonGui::EndSessionCommonGui` 0x140091ed0, `render` 0x1400929c0, its three
//! click handlers (Exit 0x1400928a0, Restart 0x1400928c0, Replay 0x1400928f0).
//!
//! Not ported: what `renderHUD` draws instead of the table at the end of a time attack, a
//! drift session and a drag race (rustyAC has none of the three).

use std::rc::Rc;

use rustyac_physics::data::ini::IniReader;

use crate::control::{Clicked, Id, Kind, MouseEvent};
use crate::font::{Font, FontType};
use crate::gui::Gui;
use crate::hud::translate;
use crate::menus::{Menu, SimAction};
use crate::painter::{Painter, Texture, ALIGN_LEFT, BLEND_ALPHA, DEPTH_OFF, GRAPHICS_GL};

/// `colTitleBar`, `colDarkTransp` of the file.
const COL_TITLE_BAR: [f32; 4] = [0.74, 0.0, 0.0, 1.0];
const COL_DARK_TRANSP: [f32; 4] = [0.1, 0.1, 0.1, 0.7];

/// `EndSessionCommonGui`'s own members.
pub struct EndCommon {
    pub bt_exit: Id,
    pub bt_restart: Id,
    pub bt_replay: Id,
    tex_logo: Texture,
}

/// `EndSessionDisplayer`.
pub struct EndSession {
    /// `GameObject::isActive`: the results show (`Sim::isDisplayingResults`)
    pub is_active: bool,
    /// Consolas bold 30 with a shadow (the points of a drift or a time attack, not ported)
    pub font: usize,
    pub parent_control: Id,
    pub leaderboard: Id,
    pub common_gui: Id,
    /// the three banners of the modes that are not ported (loaded as the game loads them)
    pub tex_endpoints_time_attack: Texture,
    pub tex_endpoints_drift: Texture,
    pub tex_end_drag: Texture,
    /// `oldMode`: the camera's mode before the results came
    pub old_mode: i32,
    pub is_loaded: bool,
}

impl Gui {
    fn end_common(&self, id: Id) -> &EndCommon {
        match &self.controls[id].kind {
            Kind::Menu(menu) => match &**menu {
                Menu::EndCommon(c) => c,
                _ => panic!("control {id} is not the end of session's panel"),
            },
            _ => panic!("control {id} is not the end of session's panel"),
        }
    }

    /// `EndSessionCommonGui::EndSessionCommonGui` 0x140091ed0. `ini` is
    /// `system/cfg/endSessionDisplayer.ini` (the game does not ship one).
    pub fn new_end_common(&mut self, painter: &mut dyn Painter, ini: Option<&IniReader>) -> Id {
        let id = self.new_control(painter, "END_SESSION_COMMON_GUI");
        self.controls[id].draw_border = false;
        self.controls[id].draw_background = false;
        if let Some(ini) = ini.filter(|ini| ini.ready) {
            self.controls[id].back_color = ini.get_float4("BACKGROUND", "COLOR").unwrap_or([0.0; 4]);
        }
        let tex_logo = painter.get_texture("content/gui/endRace/logo.png");
        let common = id;
        let mut button = |gui: &mut Gui, name: &str, texture: &str, label: &str, handler: fn(&mut Gui, Id)| {
            let b = gui.new_active_button(painter, name);
            gui.controls[b].ev_clicked.push(Rc::new(move |gui: &mut Gui, _: &Clicked| handler(gui, common)));
            gui.controls[b].back_color = [0.0, 0.0, 0.0, 0.5];
            gui.set_multiple_texture(painter, b, texture);
            gui.set_size(b, 60.0, 60.0);
            gui.set_text(b, &translate(label));
            gui.active_button_mut(b).text_coord = [0.0, 54.0];
            if let Some(font) = gui.controls[b].font {
                gui.fonts[font].scale = gui.controls[common].font_scale;
            }
            gui.control_add_control(common, b);
            b
        };
        // Exit (0x1400928a0), Restart (0x1400928c0), Replay (0x1400928f0: whether the panel shows or not)
        let bt_exit = button(self, "BUTTON_DISMISS", "content/gui/endRace/exit_icon", "Exit", |gui, common| {
            if gui.controls[common].visible {
                gui.sim.actions.push(SimAction::Shutdown);
            }
        });
        let bt_restart = button(self, "BUTTON_RESTART", "content/gui/endRace/restart_icon", "Restart", |gui, common| {
            if gui.controls[common].visible {
                gui.sim.actions.push(SimAction::RestartCurrentSession);
            }
        });
        let bt_replay = button(self, "BUTTON_REPLAY", "content/gui/endRace/replay", "Replay", |gui, _| gui.sim.actions.push(SimAction::StartReplayMode));
        self.fonts.push(Font::new(painter, "", FontType::Monospaced, 14.0, true, true));
        self.controls[id].font = Some(self.fonts.len() - 1);
        self.controls[id].kind = Kind::Menu(Box::new(Menu::EndCommon(EndCommon { bt_exit, bt_restart, bt_replay, tex_logo })));
        id
    }

    /// `EndSessionCommonGui::render` 0x1400929c0.
    pub(crate) fn end_common_render(&mut self, id: Id, dt: f32, painter: &mut dyn Painter) {
        let gl = GRAPHICS_GL;
        let (w, h) = painter.screen();
        let half = w / 2;
        let top = (h - 150) as f32;
        painter.set_depth_mode(DEPTH_OFF);
        let back = self.controls[id].back_color;
        painter.gl_color4f(gl, back[0], back[1], back[2], back[3]);
        painter.gl_quad(gl, 0.0, 0.0, w as f32, h as f32, false);
        painter.gl_color4f(gl, 1.0, 1.0, 1.0, 1.0);
        let logo = self.end_common(id).tex_logo.clone();
        painter.set_texture(0, &logo);
        let (logo_w, logo_h) = if logo.kid.is_some() { painter.texture_size(&logo) } else { (0, 0) };
        let x = half.wrapping_sub((logo_w >> 1) as i32) as u32 as i64 as f32;
        painter.gl_quad(gl, x, 50.0, logo_w as f32, logo_h as f32, true);
        painter.gl_color4f(gl, COL_TITLE_BAR[0], COL_TITLE_BAR[1], COL_TITLE_BAR[2], COL_TITLE_BAR[3]);
        painter.gl_quad(gl, half as f32 - 250.0, top, 500.0, 20.0, false);
        if let Some(font) = self.controls[id].font {
            self.fonts[font].set_color4(&[1.0; 4]);
            self.fonts[font].blit_string(painter, (half + 10) as f32 - 250.0, top, "SELECT", 1.0, ALIGN_LEFT);
        }
        painter.gl_color4f(gl, COL_DARK_TRANSP[0], COL_DARK_TRANSP[1], COL_DARK_TRANSP[2], COL_DARK_TRANSP[3]);
        painter.gl_quad(gl, half as f32 - 250.0, top + 20.0, 500.0, 80.0, false);
        let y = (top + 5.0) + 20.0;
        let (exit, restart, replay) = {
            let c = self.end_common(id);
            (c.bt_exit, c.bt_restart, c.bt_replay)
        };
        self.set_position(restart, (half as f32 - 250.0) + 50.0, y);
        let r = self.controls[replay].rect;
        self.set_position(replay, half as f32 - ((r.right - r.left) * 0.5), y);
        self.set_position(exit, ((half as f32 + 250.0) - 50.0) - 60.0, y);
        self.control_render(id, dt, painter);
    }
}

impl EndSession {
    /// `EndSessionDisplayer::EndSessionDisplayer` 0x14010a650. `ini`: see
    /// [`Gui::new_end_common`]. The panel is as large as the screen of the painter.
    pub fn new(gui: &mut Gui, painter: &mut dyn Painter, ini: Option<&IniReader>) -> EndSession {
        let mut font = Font::new(painter, "", FontType::Monospaced, 30.0, false, true);
        font.shadowed = true;
        gui.fonts.push(font);
        let font = gui.fonts.len() - 1;
        let tex_endpoints_time_attack = painter.get_texture("content/gui/timeAttack/endpoints.png");
        let tex_endpoints_drift = painter.get_texture("content/gui/drift/endpoints.png");
        let tex_end_drag = painter.get_texture("content/gui/drag/drag_end.png");
        let (w, h) = painter.screen();
        let parent = gui.new_control(painter, "PARENT_CONTROL");
        gui.set_position(parent, 0.0, 0.0);
        gui.set_size(parent, w as f32, h as f32);
        gui.controls[parent].back_color = [0.0; 4];
        gui.controls[parent].draw_border = false;
        let common = gui.new_end_common(painter, ini);
        gui.add_control(parent, common);
        gui.set_position(common, 0.0, 0.0);
        gui.set_size(common, w as f32, h as f32);
        let leaderboard = gui.new_leaderboard(painter, 10, false);
        let (p, l) = (gui.controls[parent].rect, gui.controls[leaderboard].rect);
        let x = ((p.right - p.left) * 0.5) - ((l.right - l.left) * 0.5);
        let y = ((p.bottom - p.top) * 0.5) - ((l.bottom - l.top) * 0.5);
        gui.add_control(parent, leaderboard);
        gui.set_position(leaderboard, x, y);
        gui.leaderboard_mut(leaderboard).show_mode_buttons = false;
        EndSession { is_active: false, font, parent_control: parent, leaderboard, common_gui: common, tex_endpoints_time_attack, tex_endpoints_drift, tex_end_drag, old_mode: 0, is_loaded: false }
    }

    /// `EndSessionDisplayer::displayResults` 0x14010b260.
    pub fn display_results(&mut self, gui: &mut Gui) {
        if self.is_active {
            return;
        }
        self.is_active = true;
        self.old_mode = gui.sim.camera_mode;
        if !gui.sim.vr_connected {
            gui.sim.actions.push(SimAction::SetCameraMode(5));
            gui.sim.actions.push(SimAction::SetIntroMode(true));
        }
        gui.is_active = false;
        gui.set_visible(self.parent_control, true);
        gui.sim.actions.push(SimAction::SetControlsLock(true));
    }

    /// `EndSessionDisplayer::hide` 0x14010b380.
    pub fn hide(&mut self, gui: &mut Gui) {
        if !self.is_active {
            return;
        }
        self.is_active = false;
        gui.sim.actions.push(SimAction::SetCameraMode(self.old_mode));
        gui.is_active = true;
        gui.set_visible(self.parent_control, false);
        gui.sim.actions.push(SimAction::SetControlsLock(false));
    }

    /// `EndSessionDisplayer::update` 0x14010bf90 (while the results show): no message banner;
    /// during a replay the panel is away and the GUI is back.
    pub fn update(&mut self, gui: &mut Gui) {
        if !self.is_active {
            return;
        }
        gui.sim.actions.push(SimAction::ClearMessage);
        let in_replay = gui.sim.in_replay;
        gui.set_visible(self.parent_control, !in_replay);
        gui.is_active = in_replay;
    }

    /// `EndSessionDisplayer::renderHUD` 0x14010b520.
    pub fn render_hud(&mut self, gui: &mut Gui, painter: &mut dyn Painter, dt: f32) {
        if !self.is_active || gui.sim.pause_menu.is_some_and(|pause| gui.controls[pause].visible) || gui.sim.in_replay {
            return;
        }
        painter.set_blend_mode(BLEND_ALPHA);
        gui.render(self.parent_control, dt, painter);
        painter.gl_color4f(GRAPHICS_GL, 1.0, 1.0, 1.0, 1.0);
        let kind = gui.sim.session_type;
        if kind <= 0 {
            gui.set_visible(self.leaderboard, false);
            return;
        }
        if kind <= 3 {
            if !self.is_loaded {
                gui.set_visible(self.leaderboard, true);
                gui.leaderboard_set_last_session(self.leaderboard);
                self.is_loaded = true;
            }
            gui.render(self.leaderboard, dt, painter);
            return;
        }
        // (a hot lap has no table; nor have the time attack, the drift and the drag, whose
        // banners are not ported)
        gui.set_visible(self.leaderboard, false);
    }

    /// The handlers the displayer hangs on the window's mouse events (0x14010ae00 …): while
    /// the results show, the panel hears of the mouse itself (it is no control of the GUI).
    pub fn on_mouse_down(&self, gui: &mut Gui, e: &MouseEvent) {
        if self.is_active {
            gui.on_mouse_down(self.parent_control, e);
        }
    }

    pub fn on_mouse_up(&self, gui: &mut Gui, e: &MouseEvent) {
        if self.is_active {
            gui.on_mouse_up(self.parent_control, e);
        }
    }

    pub fn on_mouse_move(&self, gui: &mut Gui, e: &MouseEvent) {
        if self.is_active {
            gui.on_mouse_move(self.parent_control, e);
        }
    }

    pub fn on_mouse_wheel(&self, gui: &mut Gui, e: &MouseEvent) {
        if self.is_active {
            gui.on_mouse_wheel(self.parent_control, e);
        }
    }
}
