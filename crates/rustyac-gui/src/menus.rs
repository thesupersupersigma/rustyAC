// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! The game's menus on top of the widgets, and the few functions of `Sim` they call.
//!
//! `PauseMenu` (0x200 bytes): `PauseMenu::PauseMenu` 0x14011d9b0, `render` 0x14011f990,
//! `onVisibleChanged` 0x14011f870, `blankButton` 0x14011f400, `getButtonByName` 0x14011f670,
//! its six click handlers (Resume 0x14011f140, Back to pits 0x14011edb0, Skip session
//! 0x14011eaf0, Restart session 0x14011f200, Replay 0x14011eec0, Exit 0x14011f1b0) and the two
//! jobs they leave for the end of the camera's fade (0x14011ec90, 0x14011f2a0).
//!
//! Of `Sim`: `activateGameGui` 0x1401977e0, `activateEscMenu` 0x140197780, `activatePauseMenu`
//! 0x140197840 (which of the ESC menu, the pause menu, the game screen and the app bar show),
//! and of `ACCameraManager` `registerToFadeInEvent` 0x140033ce0 and `fadeIn` 0x140033690 as far
//! as the menus use them.
//!
//! The game's menus reach into `Sim` (the referee, the cars, the replay, the sound). Here they
//! read a [`SimLink`] that whoever runs the GUI keeps up to date, and what they ask of the
//! session is left there as a list of [`SimAction`]s, in the order the game makes the calls.

use std::rc::Rc;

use rustyac_physics::data::ini::IniReader;

use crate::control::{Clicked, Id, Kind};
use crate::font::{Font, FontType};
use crate::gui::Gui;
use crate::hud::translate;
use crate::painter::{Painter, Texture, GRAPHICS_GL};

/// `SessionType::Race`.
pub const SESSION_RACE: i32 = 3;

/// `PauseMenuButton` (the argument of `PauseMenu::blankButton`).
pub const PAUSE_MENU_RESUME: i32 = 0;
pub const PAUSE_MENU_PITS: i32 = 1;
pub const PAUSE_MENU_RESTART: i32 = 2;
pub const PAUSE_MENU_REPLAY: i32 = 3;
pub const PAUSE_MENU_EXIT: i32 = 4;
pub const PAUSE_MENU_SKIP: i32 = 5;

/// A call a menu makes on the session, for whoever runs the GUI to carry out.
#[derive(Clone, Debug, PartialEq)]
pub enum SimAction {
    /// `Sim::setPauseMode` 0x14019e6d0
    SetPauseMode(bool),
    /// `SystemMessage::clearMessage` 0x1401b6400
    ClearMessage,
    /// `AudioEngine::stop` / `AudioEngine::start`
    AudioStop,
    AudioStart,
    /// `RaceManager::skipCurrentSession` 0x140141350
    SkipCurrentSession,
    /// `RaceManager::restartCurrentSession` 0x140140d00
    RestartCurrentSession,
    /// `ACCameraManager::fadeIn` 0x140033690 with this `fadeTotalTime`; when the fade is over
    /// the runner calls [`Gui::sim_fade_in_finished`]
    FadeIn(f32),
    /// `CarAvatar::lockControlsUntilTime(ms, now, true)` 0x1400d8e20 on the player's car
    LockControlsUntilTime(f64),
    /// `CarAvatar::goToSpawnPosition` on the player's car
    GoToSpawnPosition(String),
    /// `ReplayManager::startReplayMode(0)`
    StartReplayMode,
    /// `Game::shutdown` 0x140242fb0
    Shutdown,
}

/// What a handler of `ACCameraManager::evOnFadeInFinished` does when the fade is over.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FadeJob {
    /// the pause menu's "Back to pits" (0x14011ec90)
    BackToPits,
    /// the pause menu's "Replay" (0x14011f2a0)
    StartReplay,
}

/// What the menus read of `Sim`, and what they ask of it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SimLink {
    /// `Sim::gameScreen`, `Sim::pauseMenu`, `Sim::escMenu`
    pub game_screen: Option<Id>,
    pub pause_menu: Option<Id>,
    pub esc_menu: Option<Id>,
    /// `RaceManager::getCurrentSessionType`
    pub session_type: i32,
    /// `RaceManager::getSessionCount`
    pub session_count: u32,
    /// `Sim::client` is there
    pub online: bool,
    /// `RaceManager::getTimeToSessionStart`
    pub time_to_session_start: f64,
    /// the game object "REPLAY_MANAGER" is there
    pub has_replay_manager: bool,
    /// `ACCameraManager::evOnFadeInFinished`: who hung a job there, and the job
    pub fade_listeners: Vec<(Id, FadeJob)>,
    /// `ACCameraManager::fadeMode.sender`
    pub fade_sender: Option<Id>,
    pub actions: Vec<SimAction>,
}

/// `PauseMenu`'s own members.
pub struct PauseMenu {
    pub ac_logo: Texture,
    pub buttons: Vec<Id>,
    pub currently_selected_button: i32,
    pub fade_seconds: f32,
}

/// A menu of the game: a `ksgui::Control` with a `render` and an `onVisibleChanged` of its own.
pub enum Menu {
    Pause(PauseMenu),
}

impl Gui {
    // ----- Sim -----

    fn sim_show(&mut self, esc: bool, pause: bool, screen: bool) {
        if let Some(id) = self.sim.esc_menu {
            self.set_visible(id, esc);
        }
        if let Some(id) = self.sim.pause_menu {
            self.set_visible(id, pause);
        }
        if let Some(id) = self.sim.game_screen {
            self.set_visible(id, screen);
        }
        self.set_visible(self.taskbar, screen);
    }

    /// `Sim::activateGameGui` 0x1401977e0.
    pub fn sim_activate_game_gui(&mut self) {
        self.sim_show(false, false, true);
    }

    /// `Sim::activateEscMenu` 0x140197780.
    pub fn sim_activate_esc_menu(&mut self) {
        self.sim_show(true, false, false);
    }

    /// `Sim::activatePauseMenu` 0x140197840.
    pub fn sim_activate_pause_menu(&mut self) {
        self.sim_show(false, true, false);
    }

    /// `Sim::setPauseMode` 0x14019e6d0 (what it does is the runner's).
    pub fn sim_set_pause_mode(&mut self, paused: bool) {
        self.sim.actions.push(SimAction::SetPauseMode(paused));
    }

    /// `ACCameraManager::registerToFadeInEvent` 0x140033ce0: a fade that still runs ends here
    /// and now (its jobs are done, and the jobs of who started it are taken off), then the new
    /// job is hung up.
    pub fn sim_register_to_fade_in_event(&mut self, owner: Id, job: FadeJob) {
        if !self.sim.fade_listeners.is_empty() {
            for (_, job) in self.sim.fade_listeners.clone() {
                self.sim_run_fade_job(job);
            }
            let sender = self.sim.fade_sender;
            self.sim.fade_listeners.retain(|(owner, _)| Some(*owner) != sender);
        }
        self.sim.fade_listeners.push((owner, job));
    }

    /// `ACCameraManager::fadeIn` 0x140033690.
    pub fn sim_fade_in(&mut self, total_time: f32, sender: Id) {
        self.sim.fade_sender = Some(sender);
        self.sim.actions.push(SimAction::FadeIn(total_time));
    }

    /// The camera's fade is over: every job hung on it is done, and the list is emptied.
    pub fn sim_fade_in_finished(&mut self) {
        for (_, job) in std::mem::take(&mut self.sim.fade_listeners) {
            self.sim_run_fade_job(job);
        }
    }

    fn sim_run_fade_job(&mut self, job: FadeJob) {
        match job {
            // lambda 0x14011ec90
            FadeJob::BackToPits => {
                if self.sim.session_type == SESSION_RACE {
                    self.sim.actions.push(SimAction::LockControlsUntilTime(20000.0));
                } else {
                    self.sim.actions.push(SimAction::GoToSpawnPosition("PIT".to_string()));
                }
                self.sim_activate_esc_menu();
                self.sim_set_pause_mode(false);
            }
            // lambda 0x14011f2a0
            FadeJob::StartReplay => {
                self.sim_activate_game_gui();
                self.sim_set_pause_mode(false);
                if self.sim.has_replay_manager {
                    self.sim.actions.push(SimAction::StartReplayMode);
                }
            }
        }
    }

    // ----- PauseMenu -----

    pub fn pause_menu(&self, id: Id) -> &PauseMenu {
        match &self.controls[id].kind {
            Kind::Menu(menu) => match &**menu {
                Menu::Pause(p) => p,
            },
            _ => panic!("control {id} is not the pause menu"),
        }
    }

    pub fn pause_menu_mut(&mut self, id: Id) -> &mut PauseMenu {
        match &mut self.controls[id].kind {
            Kind::Menu(menu) => match &mut **menu {
                Menu::Pause(p) => p,
            },
            _ => panic!("control {id} is not the pause menu"),
        }
    }

    /// `PauseMenu::PauseMenu` 0x14011d9b0. `fades` is `system/cfg/fades.ini`. The session's
    /// type, the number of sessions and whether the game is online are read from [`Gui::sim`].
    pub fn new_pause_menu(&mut self, painter: &mut dyn Painter, fades: Option<&IniReader>) -> Id {
        let id = self.new_control(painter, "PAUSE_MENU");
        let mut fade_seconds = 0.0f32;
        if let Some(ini) = fades.filter(|ini| ini.ready) {
            fade_seconds = ini.get_float("FADE", "MS").unwrap_or(0.0) * f32::from_bits(0x3a83_126f);
        }
        self.fonts.push(Font::new(painter, "", FontType::Proportional, 22.0, false, false));
        let font = self.fonts.len() - 1;
        self.controls[id].font = Some(font);
        self.control_set_size(id, 400.0, 400.0);
        let ac_logo = painter.get_texture("content/gui/pauseMenuLogo.png");
        let height = if ac_logo.kid.is_some() { painter.texture_size(&ac_logo).1 } else { 0 };
        let mut y = height as f32 * 0.5 + 2.0;
        self.controls[id].back_color = [0.0, 0.0, 0.0, 0.5];
        self.controls[id].border_color = [0.25, 0.25, 0.25, 0.5];
        self.controls[id].kind = Kind::Menu(Box::new(Menu::Pause(PauseMenu { ac_logo, buttons: Vec::new(), currently_selected_button: 0, fade_seconds })));

        let menu = id;
        // Resume (0x14011f140)
        self.pause_menu_button(painter, menu, font, "RESUME", "Resume", y, |gui, _menu, ev| {
            if !gui.active_button(ev.control).blanked {
                gui.sim_activate_game_gui();
                gui.sim_set_pause_mode(false);
            }
        });
        y = y + 45.0;
        // Back to pits (0x14011edb0)
        self.pause_menu_button(painter, menu, font, "PITS", "Back to pits", y, |gui, menu, ev| {
            if !gui.active_button(ev.control).blanked {
                if gui.sim.session_type == SESSION_RACE && 0.0 < gui.sim.time_to_session_start {
                    return;
                }
                gui.set_visible(menu, false);
                gui.sim_register_to_fade_in_event(menu, FadeJob::BackToPits);
                let seconds = gui.pause_menu(menu).fade_seconds;
                gui.sim_fade_in(seconds, menu);
            }
        });
        y = y + 45.0;
        if self.sim.session_count > 1 && self.sim.session_type != SESSION_RACE && !self.sim.online {
            // Skip session (0x14011eaf0): `RaceManager::skipCurrentSession` always answers yes,
            // so the "Wait for AI cars to complete at least 3 laps" message is never made
            self.pause_menu_button(painter, menu, font, "SKIP", "Skip session", y, |gui, menu, ev| {
                gui.sim.actions.push(SimAction::SkipCurrentSession);
                gui.sim_activate_game_gui();
                gui.sim_set_pause_mode(false);
                if !gui.active_button(ev.control).blanked {
                    gui.set_visible(menu, false);
                }
            });
            y = y + 45.0;
        }
        if !self.sim.online {
            // Restart session (0x14011f200)
            self.pause_menu_button(painter, menu, font, "RESTART", "Restart Session", y, |gui, menu, ev| {
                if !gui.active_button(ev.control).blanked {
                    gui.set_visible(menu, false);
                    gui.sim.actions.push(SimAction::RestartCurrentSession);
                    gui.sim_activate_game_gui();
                    gui.sim_set_pause_mode(false);
                }
            });
            y = y + 45.0;
            // Replay (0x14011eec0)
            self.pause_menu_button(painter, menu, font, "REPLAY", "Replay", y, |gui, menu, ev| {
                if !gui.active_button(ev.control).blanked {
                    gui.set_visible(menu, false);
                    gui.sim_register_to_fade_in_event(menu, FadeJob::StartReplay);
                    let seconds = gui.pause_menu(menu).fade_seconds;
                    gui.sim_fade_in(seconds, menu);
                }
            });
            y = y + 45.0;
        }
        y = y + 45.0;
        // Exit (0x14011f1b0)
        self.pause_menu_button(painter, menu, font, "EXIT", "Exit", y, |gui, menu, ev| {
            if !gui.active_button(ev.control).blanked {
                gui.set_visible(menu, false);
                gui.sim.actions.push(SimAction::Shutdown);
            }
        });
        y = y + 45.0;
        self.control_set_size(id, 400.0, y + 50.0);
        id
    }

    fn pause_menu_button(&mut self, painter: &mut dyn Painter, menu: Id, font: usize, name: &str, text: &str, y: f32, handler: impl Fn(&mut Gui, Id, &Clicked) + 'static) {
        let button = self.new_active_button(painter, name);
        self.control_add_control(menu, button);
        let c = &mut self.controls[button];
        c.draw_border = false;
        c.draw_background = false;
        c.font = Some(font);
        let b = self.active_button_mut(button);
        b.unselected_color = [0.0; 4];
        b.roll_on_color = [0.25, 0.25, 0.25, 0.8];
        b.selected_color = [0.8, 0.8, 0.8, 0.7];
        self.set_size(button, 400.0, 45.0);
        self.active_button_mut(button).text_coord[1] = 7.0;
        self.set_position(button, 0.0, y);
        self.set_text(button, &translate(text));
        self.controls[button].ev_clicked.push(Rc::new(move |gui: &mut Gui, ev: &Clicked| handler(gui, menu, ev)));
        self.pause_menu_mut(menu).buttons.push(button);
    }

    /// `PauseMenu::getButtonByName` 0x14011f670.
    pub fn pause_menu_button_by_name(&self, id: Id, name: &str) -> Option<Id> {
        self.pause_menu(id).buttons.iter().copied().find(|b| self.controls[*b].name == name)
    }

    /// `PauseMenu::blankButton` 0x14011f400.
    pub fn pause_menu_blank_button(&mut self, id: Id, button: i32, blanked: bool) {
        let name = match button {
            PAUSE_MENU_RESUME => "RESUME",
            PAUSE_MENU_PITS => "PITS",
            PAUSE_MENU_RESTART => "RESTART",
            PAUSE_MENU_REPLAY => "REPLAY",
            PAUSE_MENU_EXIT => "EXIT",
            PAUSE_MENU_SKIP => "SKIP",
            _ => return,
        };
        if let Some(b) = self.pause_menu_button_by_name(id, name) {
            self.active_button_mut(b).blanked = blanked;
        }
    }

    /// `PauseMenu::onVisibleChanged` 0x14011f870 (the children are not told).
    fn pause_menu_on_visible_changed(&mut self, id: Id, visible: bool) {
        if self.sim.session_type == SESSION_RACE {
            if let Some(skip) = self.pause_menu_button_by_name(id, "SKIP") {
                self.active_button_mut(skip).blanked = true;
            }
        }
        self.sim.actions.push(SimAction::ClearMessage);
        self.sim.actions.push(if visible { SimAction::AudioStop } else { SimAction::AudioStart });
        let selected = self.pause_menu(id).currently_selected_button;
        if selected != -1 {
            let button = self.pause_menu(id).buttons[selected as usize];
            self.active_button_mut(button).selected = false;
            self.pause_menu_mut(id).currently_selected_button = -1;
        }
    }

    /// `PauseMenu::render` 0x14011f990: the menu in the middle of the screen, the logo over
    /// its top edge.
    fn pause_menu_render(&mut self, id: Id, dt: f32, painter: &mut dyn Painter) {
        let (width, height) = painter.screen();
        let r = self.controls[id].rect;
        let y = height as f32 * 0.5 - (r.bottom - r.top) * 0.5;
        let x = width as f32 * 0.5 - (r.right - r.left) * 0.5;
        self.set_position(id, x, y);
        self.control_render(id, dt, painter);
        let mut r = self.controls[id].rect;
        self.get_world_rect(id, &mut r);
        painter.gl_color4f(GRAPHICS_GL, 1.0, 1.0, 1.0, 1.0);
        let logo = self.pause_menu(id).ac_logo.clone();
        painter.set_texture(0, &logo);
        let (w, h) = if logo.kid.is_some() { painter.texture_size(&logo) } else { (0, 0) };
        let qx = ((r.right - r.left) * 0.5 + r.left) - w as f32 * 0.5;
        let qy = r.top - h as f32 * 0.5;
        painter.gl_quad(GRAPHICS_GL, qx, qy, w as f32, h as f32, true);
    }

    // ----- the virtual functions of the menus -----

    pub fn menu_render(&mut self, id: Id, dt: f32, painter: &mut dyn Painter) {
        match &self.controls[id].kind {
            Kind::Menu(menu) => match &**menu {
                Menu::Pause(_) => self.pause_menu_render(id, dt, painter),
            },
            _ => self.control_render(id, dt, painter),
        }
    }

    pub fn menu_on_visible_changed(&mut self, id: Id, visible: bool) {
        match &self.controls[id].kind {
            Kind::Menu(menu) => match &**menu {
                Menu::Pause(_) => self.pause_menu_on_visible_changed(id, visible),
            },
            _ => self.control_on_visible_changed(id, visible),
        }
    }
}
