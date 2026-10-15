// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! The ESC menu (the menu of the pits: the column of buttons at the left edge, with a page
//! beside it) and its page "Race control" as a single-player session has it.
//!
//! `ESCMenu` (0x260 bytes): `ESCMenu::ESCMenu` 0x1400fcb40, `addButton` 0x1400fddf0, `render`
//! 0x1400fe420, `onVisibleChanged` 0x1400fe2c0, `selectButton` 0x1400ff220,
//! `setDriveButtonEnabled` 0x1400ff2a0, `setRaceControlScreen` 0x1400ff2b0, its click handlers
//! (drive 0x1400fdc30, setup 0x1400fd7e0, telemetry 0x1400fd730, race control 0x1400fd910,
//! time 0x1400fd9c0, exit 0x1400fdc70); of `Sim`: `startGame` 0x14019eb00 and the job it
//! leaves for the end of the fade (0x140195cf0), the handler of the menu's coming and going
//! (0x140196850).
//!
//! `RaceControlScreen` (0x1b8 bytes): constructor 0x1400b2370, `render` 0x1400b29c0,
//! `getShownSessionID` 0x1400b29b0. `RaceControlSPSession` (0x1a8): constructor 0x1400b7e90,
//! `render` 0x1400b8870, its three handlers (0x1400b87e0, 0x1400b87c0, 0x1400b87a0).
//! `RaceControlSessionInfo` (0x2b8): constructor 0x1400b3660, `render` 0x1400b3f80.
//! `timeToWString` 0x1400b34c0.
//!
//! Not ported: the pages "Setup" (`SetupScreen`), "Telemetry" (`ComparativeTelemetry`) and
//! "Time table" (`EscMenuTimeTable`): whoever makes the menu hands it a control for each (the
//! game's own stand where they are), or none, and then the page's button is drawn as a button
//! that cannot be used and hears no click. An online session's "Race control" has the
//! server's panel, the votes and the chat of `race_control_online` (Task 34).

use std::rc::Rc;

use rustyac_physics::data::ini::IniReader;

use crate::control::{Clicked, Id, Kind};
use crate::font::{Font, FontType};
use crate::gui::Gui;
use crate::hud::translate;
use crate::menus::{FadeJob, Menu, SimAction, SESSION_RACE};
use crate::painter::{Painter, Rect, Texture, ALIGN_CENTER, ALIGN_LEFT, ALIGN_RIGHT, GL_LINES, GRAPHICS_GL};

const COL_TITLE_BAR: [f32; 4] = [0.74, 0.0, 0.0, 1.0];
const COL_DARK_BAR: [f32; 4] = [0.21, 0.21, 0.21, 1.0];
const COL_DARK_TRANSP: [f32; 4] = [0.1, 0.1, 0.1, 0.7];

/// The pages the menu's maker hands it (see the module's text).
#[derive(Clone, Copy, Default)]
pub struct EscPages {
    pub setup_screen: Option<PageMaker>,
    pub comparative_telemetry: Option<PageMaker>,
    pub time_table: Option<PageMaker>,
}

/// Makes a page's control, at the moment the game's constructor makes the page.
pub type PageMaker = fn(&mut Gui, &mut dyn Painter) -> Id;

/// `ESCMenu`'s own members.
pub struct EscMenu {
    pub setup_screen: Option<Id>,
    pub drive_menu_pressed: bool,
    pub fade_time: f32,
    pub width: f32,
    pub ac_logo: Texture,
    pub but_exit: Id,
    pub but_drive: Id,
    pub but_drive_fake: Id,
    pub but_setup: Id,
    pub but_time: Id,
    pub but_comparative_telemetry: Id,
    pub last_selected: Option<Id>,
    pub but_race_control: Id,
    pub comparative_telemetry: Option<Id>,
    pub race_control: Id,
    pub time_table: Option<Id>,
}

/// `RaceControlScreen`'s own members.
pub struct RaceControlScreen {
    pub title_font: usize,
    pub leaderboard: Id,
    pub session_info: Id,
    /// online: `RaceControlVoting` (the server's panel and the chat are children only)
    pub voting: Option<Id>,
}

/// `RaceControlSPSession`'s own members.
pub struct SpSession {
    pub title_font: usize,
}

/// `RaceControlSessionInfo`'s own members.
pub struct SessionInfo {
    pub title_font: usize,
    pub data_font: usize,
    pub tx_remaining_time: Texture,
    pub tx_track_conditions: Texture,
    pub tx_time_of_the_day: Texture,
    pub tx_air_temperature: Texture,
    pub tx_asphalt_temperature: Texture,
    pub tx_next_session: Texture,
    pub tx_wind_indicator: Texture,
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

/// `toupper` on every letter (the C locale's).
fn upper(text: &str) -> String {
    text.chars().map(|c| c.to_ascii_uppercase()).collect()
}

/// `timeToWString` 0x1400b34c0.
pub fn time_to_wstring(ms: i32, apostrophe: bool, with_seconds: bool) -> String {
    let total = ms / 1000;
    let h = total / 3600;
    let m = total / 60 - h * 60;
    let s = total - (h * 60 + m) * 60;
    if ms <= 0 {
        if apostrophe { "0'00\"" } else { "0:00" }.to_string()
    } else if h > 0 {
        if !with_seconds {
            format!("{h}h{m:02}m")
        } else if apostrophe {
            format!("{h}'{m:02}'{s:02}")
        } else {
            format!("{h}:{m:02}:{s:02}")
        }
    } else if apostrophe {
        format!("{}'{s:02}\"", total / 60)
    } else {
        format!("{}:{s:02}", total / 60)
    }
}

impl Gui {
    pub fn esc_menu(&self, id: Id) -> &EscMenu {
        match &self.controls[id].kind {
            Kind::Menu(menu) => match &**menu {
                Menu::Esc(m) => m,
                _ => panic!("control {id} is not the ESC menu"),
            },
            _ => panic!("control {id} is not the ESC menu"),
        }
    }

    pub fn esc_menu_mut(&mut self, id: Id) -> &mut EscMenu {
        match &mut self.controls[id].kind {
            Kind::Menu(menu) => match &mut **menu {
                Menu::Esc(m) => m,
                _ => panic!("control {id} is not the ESC menu"),
            },
            _ => panic!("control {id} is not the ESC menu"),
        }
    }

    fn race_control(&self, id: Id) -> &RaceControlScreen {
        match &self.controls[id].kind {
            Kind::Menu(menu) => match &**menu {
                Menu::RaceControl(m) => m,
                _ => panic!("control {id} is not the race control screen"),
            },
            _ => panic!("control {id} is not the race control screen"),
        }
    }

    /// `ESCMenu::ESCMenu` 0x1400fcb40. `fades` is `system/cfg/fades.ini`. Whether the game is
    /// online, the setup is fixed, and the cars are read from [`Gui::sim`]. Who makes the menu
    /// adds it to the GUI and then names it in [`crate::menus::SimLink::esc_menu`] (from then
    /// on its coming and going hides and shows the game screen and the app bar).
    pub fn new_esc_menu(&mut self, painter: &mut dyn Painter, fades: Option<&IniReader>, pages: EscPages) -> Id {
        let id = self.new_control(painter, "ESCMenu");
        let (width, height) = painter.screen();
        self.fonts.push(Font::new(painter, "", FontType::Monospaced, 20.0, false, false));
        self.controls[id].font = Some(self.fonts.len() - 1);
        self.control_set_size(id, 100.0, height as f32);
        self.controls[id].draw_border = false;
        self.controls[id].draw_background = false;
        // (setVisible(false) with no child and nobody listening yet)
        self.controls[id].visible = false;
        let ac_logo = painter.get_texture("content/gui/logo_esc_menu.png");
        let menu = id;
        let setup_screen = pages.setup_screen.map(|make| make(self, painter));
        if let Some(setup) = setup_screen {
            self.control_add_control(menu, setup);
        }
        let but_drive = self.esc_menu_add_button(painter, menu, "drive", 125.0);
        let but_drive_fake = self.esc_menu_add_button(painter, menu, "drive_VALIDATE", 125.0);
        self.active_button_mut(but_drive_fake).text_coord = [0.0, 10.0];
        if let Some(font) = self.controls[but_drive_fake].font {
            self.fonts[font].scale = 12.0;
        }
        // (no test of the file being there)
        let fade_time = fades.map_or(0.0, |ini| ini.get_float("ESC_MENU_FADE", "MS").unwrap_or(0.0)) * f32::from_bits(0x3a83_126f);
        // drive (0x1400fdc30)
        self.controls[but_drive].ev_clicked.push(Rc::new(move |gui: &mut Gui, _ev: &Clicked| {
            if gui.active_button(gui.esc_menu(menu).but_drive).is_active {
                gui.sim_start_game(menu);
            }
        }));
        let but_setup = self.esc_menu_add_button(painter, menu, "setup", 225.0);
        if setup_screen.is_none() {
            self.active_button_mut(but_setup).is_active = false;
        } else if self.sim.fixed_setup {
            let b = self.active_button_mut(but_setup);
            b.using_multiple_textures = false;
            b.roll_on_color = b.unselected_color;
            self.controls[but_setup].back_texture = painter.get_texture("content/gui/inGameMenu/setup_OFF.png");
        }
        // setup (0x1400fd7e0)
        self.controls[but_setup].ev_clicked.push(Rc::new(move |gui: &mut Gui, ev: &Clicked| {
            let Some(setup) = gui.esc_menu(menu).setup_screen else { return };
            if !gui.sim.fixed_setup {
                // SetupScreen::switchVisibility: the flag alone
                gui.controls[setup].visible = !gui.controls[setup].visible;
                gui.esc_menu_select_button(menu, ev.control);
            } else {
                gui.sim.actions.push(SimAction::AddMessage { title: "Blocked".to_string(), text: "Cannot modify setup for this session".to_string() });
            }
        }));
        let but_time = self.esc_menu_add_button(painter, menu, "time", 325.0);
        let but_comparative_telemetry = self.esc_menu_add_button(painter, menu, "telemetry", 425.0);
        let comparative_telemetry = pages.comparative_telemetry.map(|make| make(self, painter));
        if let Some(form) = comparative_telemetry {
            self.form_set_icon_visible(form, false);
            self.set_visible(form, false);
            let right = self.controls[menu].rect.right;
            self.set_size(form, (width as f32 - right) - 20.0, height as f32);
            self.set_position(form, right + 10.0, 0.0);
            self.control_add_control(menu, form);
        } else {
            self.active_button_mut(but_comparative_telemetry).is_active = false;
        }
        // telemetry (0x1400fd730)
        self.controls[but_comparative_telemetry].ev_clicked.push(Rc::new(move |gui: &mut Gui, ev: &Clicked| {
            let Some(form) = gui.esc_menu(menu).comparative_telemetry else { return };
            let shown = gui.controls[form].visible;
            gui.set_visible(form, !shown);
            gui.esc_menu_select_button(menu, ev.control);
        }));
        let but_race_control = self.esc_menu_add_button(painter, menu, "racecontrol", 525.0);
        let race_control = self.new_race_control_screen(painter);
        self.control_add_control(menu, race_control);
        self.set_visible(race_control, false);
        self.set_position(race_control, 100.0, 0.0);
        self.set_size(race_control, (width - 100) as f32, height as f32);
        self.controls[race_control].draw_border = false;
        self.controls[race_control].draw_background = false;
        // race control (0x1400fd910)
        self.controls[but_race_control].ev_clicked.push(Rc::new(move |gui: &mut Gui, ev: &Clicked| {
            let page = gui.esc_menu(menu).race_control;
            let shown = gui.controls[page].visible;
            gui.set_visible(page, !shown);
            gui.esc_menu_select_button(menu, ev.control);
        }));
        self.controls[id].kind = Kind::Menu(Box::new(Menu::Esc(Box::new(EscMenu {
            setup_screen,
            drive_menu_pressed: false,
            fade_time,
            width: 100.0,
            ac_logo,
            but_exit: but_drive,
            but_drive,
            but_drive_fake,
            but_setup,
            but_time,
            but_comparative_telemetry,
            last_selected: None,
            but_race_control,
            comparative_telemetry,
            race_control,
            time_table: None,
        }))));
        // (a click the menu makes itself: the game hands on no place of the mouse)
        if self.sim.online {
            self.fire_clicked(but_race_control, Clicked { control: but_race_control, local_x: i32::MIN, local_y: i32::MIN });
        }
        let time_table = pages.time_table.map(|make| make(self, painter));
        self.esc_menu_mut(menu).time_table = time_table;
        if let Some(table) = time_table {
            self.control_add_control(menu, table);
            self.set_visible(table, false);
            self.set_position(table, 100.0, 0.0);
            self.set_size(table, 600.0, height as f32);
            self.controls[table].draw_border = false;
            self.controls[table].draw_background = false;
        } else {
            self.active_button_mut(but_time).is_active = false;
        }
        // time (0x1400fd9c0)
        self.controls[but_time].ev_clicked.push(Rc::new(move |gui: &mut Gui, ev: &Clicked| {
            let Some(table) = gui.esc_menu(menu).time_table else { return };
            // the page shows the session the race control's table shows
            if matches!(&gui.controls[table].kind, Kind::Menu(m) if matches!(**m, Menu::TimeTable(_))) {
                let session = gui.race_control_shown_session_id(gui.esc_menu(menu).race_control);
                gui.time_table_mut(table).session_id = session;
            }
            let shown = gui.controls[table].visible;
            gui.set_visible(table, !shown);
            gui.esc_menu_select_button(menu, ev.control);
        }));
        let but_exit = self.esc_menu_add_button(painter, menu, "exit", 625.0);
        self.esc_menu_mut(menu).but_exit = but_exit;
        // exit (0x1400fdc70)
        self.controls[but_exit].ev_clicked.push(Rc::new(move |gui: &mut Gui, ev: &Clicked| {
            gui.esc_menu_select_button(menu, ev.control);
            gui.sim.actions.push(SimAction::Shutdown);
        }));
        id
    }

    /// `ESCMenu::addButton` 0x1400fddf0.
    fn esc_menu_add_button(&mut self, painter: &mut dyn Painter, menu: Id, name: &str, y: f32) -> Id {
        let button = self.new_active_button(painter, name);
        self.control_add_control(menu, button);
        self.set_text(button, "");
        self.set_position(button, 1.0, y);
        self.set_size(button, 97.0, 97.0);
        self.controls[button].draw_border = false;
        self.controls[button].draw_background = false;
        self.set_multiple_texture(painter, button, &format!("content/gui/inGameMenu/{name}"));
        button
    }

    /// The page a button of the menu shows (its `tag`).
    fn esc_menu_page(&self, menu: Id, button: Id) -> Option<Id> {
        let m = self.esc_menu(menu);
        if button == m.but_setup {
            m.setup_screen
        } else if button == m.but_comparative_telemetry {
            m.comparative_telemetry
        } else if button == m.but_race_control {
            Some(m.race_control)
        } else if button == m.but_time {
            m.time_table
        } else {
            None
        }
    }

    /// `ESCMenu::selectButton` 0x1400ff220.
    pub fn esc_menu_select_button(&mut self, menu: Id, button: Id) {
        if let Some(last) = self.esc_menu(menu).last_selected {
            if let Some(page) = self.esc_menu_page(menu, last) {
                if self.controls[page].visible {
                    self.set_visible(page, false);
                }
            }
            self.active_button_mut(last).selected = false;
            if last == button {
                self.esc_menu_mut(menu).last_selected = None;
                return;
            }
        }
        self.active_button_mut(button).selected = true;
        self.esc_menu_mut(menu).last_selected = Some(button);
    }

    /// `ESCMenu::setDriveButtonEnabled` 0x1400ff2a0.
    pub fn esc_menu_set_drive_button_enabled(&mut self, menu: Id, enabled: bool) {
        let drive = self.esc_menu(menu).but_drive;
        self.active_button_mut(drive).is_active = enabled;
    }

    /// `ESCMenu::setRaceControlScreen` 0x1400ff2b0.
    pub fn esc_menu_set_race_control_screen(&mut self, menu: Id) {
        let (button, page) = (self.esc_menu(menu).but_race_control, self.esc_menu(menu).race_control);
        if self.esc_menu(menu).last_selected != Some(button) {
            self.esc_menu_select_button(menu, button);
            self.set_visible(page, true);
        }
    }

    /// `Sim::startGame` 0x14019eb00: the camera fades, and at the fade's end the car is let go.
    pub fn sim_start_game(&mut self, menu: Id) {
        if self.sim.setup_state != 1 {
            return;
        }
        self.sim_register_to_fade_in_event(menu, FadeJob::Drive);
        let seconds = self.esc_menu(menu).fade_time;
        self.sim_fade_in(seconds, menu);
    }

    /// The job `Sim::startGame` leaves for the end of the fade (0x140195cf0).
    pub fn sim_drive(&mut self) {
        let Some(menu) = self.sim.esc_menu else { return };
        self.sim.actions.push(SimAction::StartRace(10000.0));
        self.set_visible(menu, false);
        self.sim.actions.push(SimAction::SetIntroMode(false));
        self.sim.actions.push(SimAction::SetControlsLock(false));
        if !self.sim.race.cars.is_empty() {
            self.sim.actions.push(SimAction::FocusPlayerCar);
        }
        self.sim.actions.push(SimAction::DriveMenuPressed);
        if !self.esc_menu(menu).drive_menu_pressed {
            self.esc_menu_mut(menu).drive_menu_pressed = true;
            self.sim.actions.push(SimAction::LoadPreviousSessionUserCamera);
        }
    }

    /// `ESCMenu::onVisibleChanged` 0x1400fe2c0.
    pub(crate) fn esc_menu_on_visible_changed(&mut self, id: Id, visible: bool) {
        for child in self.controls[id].controls.clone() {
            self.on_visible_changed(child, visible);
        }
        if self.controls[id].visible {
            let drive = self.esc_menu(id).but_drive;
            self.active_button_mut(drive).status = 0;
        }
        // Sim's handler of the menu's coming and going (0x140196850)
        if self.sim.esc_menu == Some(id) {
            if visible {
                self.set_visible(self.taskbar, false);
                if let Some(screen) = self.sim.game_screen {
                    self.set_visible(screen, false);
                }
            } else {
                if let Some(screen) = self.sim.game_screen {
                    self.set_visible(screen, true);
                }
                self.set_visible(self.taskbar, true);
            }
        }
        if !self.sim.online && self.controls[id].visible && self.sim.camera_mode != 5 && self.esc_menu(id).last_selected.is_none() {
            let button = self.esc_menu(id).but_race_control;
            self.fire_clicked(button, Clicked { control: button, local_x: i32::MIN, local_y: i32::MIN });
        }
    }

    /// `ESCMenu::render` 0x1400fe420.
    pub(crate) fn esc_menu_render(&mut self, id: Id, dt: f32, painter: &mut dyn Painter) {
        let gl = GRAPHICS_GL;
        let (width, height) = painter.screen();
        let h = height as f32;
        self.sim.actions.push(SimAction::SetControlsLock(true));
        if let Some(screen) = self.sim.game_screen {
            if let Kind::GameScreen(data) = &mut self.controls[screen].kind {
                data.app_interaction_enabled = true;
            }
        }
        if let Some(form) = self.esc_menu(id).comparative_telemetry {
            let r = self.controls[id].rect;
            self.set_size(form, (width as f32 - r.right) - 20.0, h);
            self.set_position(form, (r.right - r.left) + 10.0, 0.0);
        }
        let (drive, fake) = (self.esc_menu(id).but_drive, self.esc_menu(id).but_drive_fake);
        match self.sim.setup_state {
            0 => {
                self.set_visible(fake, true);
                self.set_text(fake, &translate("SETUP\nVALIDATING\n\nPLEASE WAIT"));
                self.set_visible(drive, false);
            }
            1 => {
                self.set_visible(fake, false);
                self.set_visible(drive, true);
            }
            2 => {
                self.set_visible(fake, true);
                self.set_text(fake, &translate("SETUP\nINVALID\n\nCLICK TO\nCHECK AGAIN"));
                self.set_visible(drive, false);
            }
            _ => {}
        }
        if self.sim.triple_screen {
            self.set_position(id, width as f32 * f32::from_bits(0x3eaa_aaab), 0.0);
        }
        let r = self.controls[id].rect;
        painter.gl_color4f(gl, 0.0, 0.0, 0.0, 0.7);
        painter.gl_quad(gl, r.left, r.top, self.esc_menu(id).width, h, false);
        painter.gl_color4f(gl, 0.25, 0.25, 0.25, 0.5);
        painter.gl_begin(gl, GL_LINES);
        painter.gl_vertex3f(gl, r.right, 0.0, 0.0);
        painter.gl_vertex3f(gl, r.right, h, 0.0);
        painter.gl_end(gl);
        if !self.sim.online {
            painter.gl_color4f(gl, 1.0, 1.0, 1.0, 1.0);
            let logo = self.esc_menu(id).ac_logo.clone();
            painter.set_texture(0, &logo);
            painter.gl_quad(gl, r.left, 0.0, 98.0, 98.0, true);
        } else {
            self.esc_menu_online_header(id, painter);
        }
        let exit = self.esc_menu(id).but_exit;
        let e = self.controls[exit].rect;
        self.set_position(exit, e.left, h - (e.right - e.left));
        self.control_render(id, dt, painter);
    }

    /// The head of the column in an online session: its kind, and a clock.
    fn esc_menu_online_header(&mut self, id: Id, painter: &mut dyn Painter) {
        let font = self.controls[id].font.unwrap_or(self.font);
        let kind = self.sim.session_type;
        let mut name = translate(match kind {
            1 => "PRACTICE",
            2 => "QUALIFY",
            3 => "RACE",
            _ => "SESSION",
        });
        let wait = self.sim.time_to_wait;
        if wait != 0.0 {
            name = translate("WAIT TIME");
        }
        self.fonts[font].blit_string(painter, 50.0, 10.0, &name, 0.9, ALIGN_CENTER);
        let t = self.sim.time_to_session_start;
        if t < 5000.0 && t > 0.0 && !self.sim.player_in_pit {
            self.sim.actions.push(SimAction::GoToSpawnPosition("PIT".to_string()));
            self.sim.actions.push(SimAction::LockControlsUntilTime(20000.0));
        }
        let now = self.sim.game_time;
        // (the game asks the time twice and takes the smaller, never less than nothing)
        let clamp_time_left = |left: f64| if left >= 0.0 { left } else { 0.0 };
        let (label, clock) = if t > 0.0 {
            ("Countdown", time_to_wstring(cut_d(t), false, false))
        } else if wait > 0.0 {
            ("Countdown", time_to_wstring(cut_d(wait - now), false, false))
        } else {
            let session = self.sim.race.session_info(self.sim.race.current_session_index);
            let clock = if kind != SESSION_RACE || session.is_timed_race { time_to_wstring(cut_d(clamp_time_left(self.sim.session_time_left)), false, false) } else { time_to_wstring(cut_d(now - session.start_time), false, false) };
            ("Time", clock)
        };
        self.fonts[font].blit_string(painter, 50.0, 40.0, &translate(label), 0.7, ALIGN_CENTER);
        self.fonts[font].blit_string(painter, 50.0, 60.0, &clock, 1.2, ALIGN_CENTER);
    }

    // ----- RaceControlScreen -----

    /// `RaceControlScreen::RaceControlScreen` 0x1400b2370.
    pub fn new_race_control_screen(&mut self, painter: &mut dyn Painter) -> Id {
        let id = self.new_control(painter, "RACE_CONTROL_SCREEN");
        let session_info = self.new_session_info(painter);
        self.set_position(session_info, 20.0, 55.0);
        self.set_size(session_info, 290.0, 203.0);
        self.controls[session_info].draw_background = false;
        self.controls[session_info].draw_border = false;
        self.control_add_control(id, session_info);
        let leaderboard;
        let table_height;
        let mut voting = None;
        if !self.sim.online {
            let sp = self.new_sp_session(painter);
            let bottom = self.controls[session_info].rect.bottom;
            self.set_position(sp, 20.0, bottom + 15.0);
            self.set_size(sp, 290.0, 100.0);
            self.controls[sp].back_color = [0.12, 0.12, 0.12, 0.8];
            self.controls[sp].draw_border = false;
            self.control_add_control(id, sp);
            let height = painter.screen().1;
            let fit = cut((height - 200) as f32 * f32::from_bits(0x3d06_4b8a));
            let cars = self.sim.race.cars.len() as i32;
            let rows = if cars >= fit { fit } else { cars };
            leaderboard = self.new_leaderboard(painter, rows, false);
            table_height = rows as f32 * 30.5 + 20.0;
        } else {
            let server = self.new_session_server(painter);
            let bottom = self.controls[session_info].rect.bottom;
            self.set_position(server, 20.0, bottom + 15.0);
            self.set_size(server, 290.0, 325.0);
            self.controls[server].draw_background = false;
            self.controls[server].draw_border = false;
            self.control_add_control(id, server);
            let v = self.new_race_control_voting(painter);
            let bottom = self.controls[server].rect.bottom;
            self.set_position(v, 20.0, bottom + 15.0);
            self.set_size(v, 290.0, 175.0);
            self.controls[v].back_color = [0.12, 0.12, 0.12, 0.8];
            self.controls[v].draw_border = false;
            self.control_add_control(id, v);
            voting = Some(v);
            let chat = self.new_ac_chat(painter);
            self.set_position(chat, 325.0, 425.0);
            let h = self.controls[v].rect.bottom - self.controls[chat].rect.top;
            self.set_size(chat, 780.0, h);
            self.controls[chat].back_color = [0.12, 0.12, 0.12, 0.8];
            self.controls[chat].draw_border = false;
            self.control_add_control(id, chat);
            leaderboard = self.new_leaderboard(painter, 10, false);
            table_height = 325.0;
        }
        self.set_position(leaderboard, 325.0, 55.0);
        self.set_size(leaderboard, 780.0, table_height);
        self.leaderboard_mut(leaderboard).show_mode_buttons = true;
        self.control_add_control(id, leaderboard);
        self.fonts.push(Font::new(painter, "", FontType::Monospaced, 25.0, true, true));
        let title_font = self.fonts.len() - 1;
        self.controls[id].kind = Kind::Menu(Box::new(Menu::RaceControl(RaceControlScreen { title_font, leaderboard, session_info, voting })));
        id
    }

    /// `RaceControlScreen::getShownSessionID` 0x1400b29b0.
    pub fn race_control_shown_session_id(&self, id: Id) -> i32 {
        self.leaderboard(self.race_control(id).leaderboard).session_id
    }

    /// `RaceControlScreen::render` 0x1400b29c0: the page, then a bar for every session.
    pub(crate) fn race_control_render(&mut self, id: Id, dt: f32, painter: &mut dyn Painter) {
        self.control_render(id, dt, painter);
        let gl = GRAPHICS_GL;
        let mut r = Rect { left: 0.0, right: 1.0, top: 0.0, bottom: 1.0 };
        self.get_world_rect(id, &mut r);
        let own = self.controls[id].rect;
        let mut x0 = r.left + (own.left + 20.0);
        let y0 = r.top + (own.top + 8.0);
        let title_font = self.race_control(id).title_font;
        let mut i = 0;
        while (i as u32) < self.sim.session_count {
            let s = self.sim.race.session_info(i);
            let col = if i == self.sim.race.current_session_index { COL_TITLE_BAR } else { COL_DARK_BAR };
            painter.gl_color4f(gl, col[0], col[1], col[2], col[3]);
            painter.gl_quad(gl, x0, y0, 290.0, 32.0, false);
            let race = s.session_type == SESSION_RACE;
            let mut out = translate(&upper(&s.name));
            if race && self.sim.race.inverted_grid_positions != 0 {
                out.push_str("x2");
            }
            out.push(' ');
            if race && !s.is_timed_race {
                out.push_str(&format!("{}L", s.laps));
            } else {
                out.push_str(&time_to_wstring(cut(s.duration_minutes * 60000.0), true, false));
                if self.sim.online && race && self.sim.race.has_extra_lap {
                    out.push('+');
                }
            }
            if race && self.sim.race.pit_window_end != 0 {
                painter.gl_color4f(gl, col[0], col[1], col[2], col[3]);
                painter.gl_quad(gl, x0 + 290.0, y0, 152.0, 32.0, false);
                let (start, end) = (self.sim.race.pit_window_start, self.sim.race.pit_window_end);
                let (open, close) = if !s.is_timed_race {
                    (format!("PIT OPEN: lap {start}"), format!("PIT CLOSE: lap {end}"))
                } else {
                    (format!("PIT OPEN: {} minutes left", cut(s.duration_minutes - start as f32)), format!("PIT CLOSE: {} minutes left", cut(s.duration_minutes - end as f32)))
                };
                self.fonts[title_font].blit_string(painter, x0 + 440.0, y0, &open, 0.5, ALIGN_RIGHT);
                self.fonts[title_font].blit_string(painter, x0 + 440.0, y0 + 16.0, &close, 0.5, ALIGN_RIGHT);
            }
            self.fonts[title_font].blit_string(painter, x0 + 145.0, y0, &out, 1.0, ALIGN_CENTER);
            x0 = x0 + 308.0;
            i += 1;
        }
        // online: no votes in the last 30 seconds of a practice or a qualifying
        if let Some(voting) = self.race_control(id).voting {
            let show = if self.sim.session_type == SESSION_RACE { true } else { self.sim.session_time_left > 30000.0 };
            self.set_visible(voting, show);
        }
    }

    // ----- RaceControlSPSession -----

    /// `RaceControlSPSession::RaceControlSPSession` 0x1400b7e90.
    pub fn new_sp_session(&mut self, painter: &mut dyn Painter) -> Id {
        let id = self.new_control(painter, "RACE_CONTROL_SESSION");
        self.control_set_size(id, 620.0, 200.0);
        self.fonts.push(Font::new(painter, "", FontType::Monospaced, 14.0, true, true));
        let title_font = self.fonts.len() - 1;
        self.fonts.push(Font::new(painter, "", FontType::Proportional, 20.0, false, false));
        let font = self.fonts.len() - 1;
        self.fonts[font].set_color(1.0, 1.0, 1.0, 1.0);
        self.controls[id].font = Some(font);
        self.controls[id].kind = Kind::Menu(Box::new(Menu::SpSession(SpSession { title_font })));
        // (the names are the game's: they were copied from the votes' panel)
        for (name, texture, text, x, action) in [
            ("BUT_VOTE_NEXT_SESSION", "restart_icon", "Restart Session", 45.0, SimAction::RestartCurrentSession),
            ("BUT_VOTE_RESTART_SESSION", "skip_icon", "Skip Session", 130.0, SimAction::SkipCurrentSession),
            ("BUT_VOTE_YES", "vote_no", "Exit", 215.0, SimAction::Shutdown),
        ] {
            let button = self.new_active_button(painter, name);
            self.set_multiple_texture(painter, button, &format!("content/gui/raceControl/{texture}"));
            self.set_text(button, &translate(text));
            self.set_position(button, x, 30.0);
            self.set_size(button, 40.0, 40.0);
            let r = self.controls[button].rect;
            self.active_button_mut(button).text_coord = [0.0, r.bottom - r.top];
            self.controls[button].ev_clicked.push(Rc::new(move |gui: &mut Gui, _ev: &Clicked| gui.sim.actions.push(action.clone())));
            self.control_add_control(id, button);
        }
        id
    }

    /// `RaceControlSPSession::render` 0x1400b8870.
    pub(crate) fn sp_session_render(&mut self, id: Id, dt: f32, painter: &mut dyn Painter) {
        self.control_render(id, dt, painter);
        let gl = GRAPHICS_GL;
        let mut r = Rect { left: 0.0, right: 1.0, top: 0.0, bottom: 1.0 };
        self.get_world_rect(id, &mut r);
        let own = self.controls[id].rect;
        let (x, y) = (r.left + own.left, r.top + own.top);
        painter.gl_color4f(gl, COL_TITLE_BAR[0], COL_TITLE_BAR[1], COL_TITLE_BAR[2], COL_TITLE_BAR[3]);
        painter.gl_quad(gl, x, y, own.right - own.left, 20.0, false);
        let title_font = match &self.controls[id].kind {
            Kind::Menu(menu) => match &**menu {
                Menu::SpSession(s) => s.title_font,
                _ => return,
            },
            _ => return,
        };
        self.fonts[title_font].set_color(1.0, 1.0, 1.0, 1.0);
        self.fonts[title_font].blit_string(painter, x + 10.0, y + 1.0, &translate("SESSION CONTROL"), 1.0, ALIGN_LEFT);
    }

    // ----- RaceControlSessionInfo -----

    /// `RaceControlSessionInfo::RaceControlSessionInfo` 0x1400b3660.
    pub fn new_session_info(&mut self, painter: &mut dyn Painter) -> Id {
        let id = self.new_control(painter, "RACE_CONTROL_SESSION_INFO");
        self.fonts.push(Font::new(painter, "", FontType::Monospaced, 14.0, true, true));
        let title_font = self.fonts.len() - 1;
        self.fonts.push(Font::new(painter, "", FontType::Proportional, 14.0, false, true));
        let data_font = self.fonts.len() - 1;
        let mut texture = |name: &str| painter.get_texture(name);
        let info = SessionInfo {
            title_font,
            data_font,
            tx_remaining_time: texture("content/gui/raceControl/remaining.png"),
            tx_track_conditions: texture("content/gui/raceControl/conditions.png"),
            tx_time_of_the_day: texture("content/gui/raceControl/time_of_day.png"),
            tx_air_temperature: texture("content/gui/raceControl/air_temp.png"),
            tx_asphalt_temperature: texture("content/gui/raceControl/asphalt_temp.png"),
            tx_next_session: texture("content/gui/raceControl/next_session.png"),
            tx_wind_indicator: texture("content/gui/wind_indicator.png"),
        };
        self.controls[id].kind = Kind::Menu(Box::new(Menu::SessionInfo(Box::new(info))));
        self.controls[id].draw_border = false;
        self.controls[id].draw_background = false;
        id
    }

    /// `RaceControlSessionInfo::render` 0x1400b3f80: a title and six rows.
    pub(crate) fn session_info_render(&mut self, id: Id, dt: f32, painter: &mut dyn Painter) {
        self.control_render(id, dt, painter);
        let gl = GRAPHICS_GL;
        let mut r = Rect { left: 0.0, right: 1.0, top: 0.0, bottom: 1.0 };
        self.get_world_rect(id, &mut r);
        let own = self.controls[id].rect;
        let x = r.left + own.left;
        let mut y = r.top + own.top;
        let w = own.right - own.left;
        let (title_font, data_font, textures) = match &self.controls[id].kind {
            Kind::Menu(menu) => match &**menu {
                Menu::SessionInfo(s) => (s.title_font, s.data_font, [s.tx_remaining_time.clone(), s.tx_track_conditions.clone(), s.tx_time_of_the_day.clone(), s.tx_air_temperature.clone(), s.tx_asphalt_temperature.clone(), s.tx_next_session.clone(), s.tx_wind_indicator.clone()]),
                _ => return,
            },
            _ => return,
        };
        const ROW: f32 = 30.5;
        let row = |painter: &mut dyn Painter, col: [f32; 4], y: f32| {
            painter.gl_color4f(gl, col[0], col[1], col[2], col[3]);
            painter.gl_quad(gl, x, y, w, ROW, false);
        };
        let size = |painter: &mut dyn Painter, texture: &Texture| if texture.kid.is_some() { painter.texture_size(texture) } else { (0, 0) };
        let icon = |painter: &mut dyn Painter, texture: &Texture, y: f32| {
            painter.gl_color4f(gl, 1.0, 1.0, 1.0, 1.0);
            painter.set_texture(0, texture);
            let (tw, th) = size(painter, texture);
            painter.gl_quad(gl, x + 5.0, y + 8.0, tw as f32, th as f32, true);
        };
        painter.gl_color4f(gl, COL_TITLE_BAR[0], COL_TITLE_BAR[1], COL_TITLE_BAR[2], COL_TITLE_BAR[3]);
        painter.gl_quad(gl, x, y, w, 20.0, false);
        self.fonts[title_font].set_color(1.0, 1.0, 1.0, 1.0);
        self.fonts[title_font].blit_string(painter, x + 10.0, y + 1.0, &translate("SESSION INFORMATION"), 1.0, ALIGN_LEFT);
        y = y + 20.0;

        // what is left of the session
        row(painter, COL_DARK_BAR, y);
        let race = &self.sim.race;
        let current = race.session_info(race.current_session_index);
        let name = upper(&current.name);
        let text = if self.sim.time_to_session_start > 0.0 {
            format!("{name} {}", translate("not started"))
        } else {
            let mut text = format!("{}: {name} - ", translate("remaining"));
            if self.sim.session_type == SESSION_RACE && !current.is_timed_race {
                // (the game reads the first place whether there is one or not)
                let leader = race.leaderboard.first().and_then(|e| race.cars.get(e.car)).map_or(0, |car| car.lap_count);
                let mut left = current.laps.wrapping_sub(leader as i32);
                if left > 9999 {
                    left = 9999;
                } else if left < 0 {
                    left = 0;
                }
                text.push_str(&format!("{left} L"));
            } else if self.sim.session_time_left > 0.0 {
                text.push_str(&time_to_wstring(cut_d(self.sim.session_time_left), false, false));
            }
            text
        };
        let line = |gui: &mut Gui, painter: &mut dyn Painter, texture: &Texture, text: &str, y: f32| {
            icon(painter, texture, y);
            gui.fonts[data_font].set_color(1.0, 1.0, 1.0, 1.0);
            gui.fonts[data_font].blit_string(painter, x + 30.0, y + 6.1, text, 1.0, ALIGN_LEFT);
        };
        line(self, painter, &textures[0], &text, y);
        y = y + ROW;

        // the track's grip
        row(painter, COL_DARK_TRANSP, y);
        let text = format!("{}: {} %", translate("track grip"), cut(self.sim.track_grip * 100.0));
        line(self, painter, &textures[1], &text, y);
        y = y + ROW;

        // the time of the day, from the sun's angle
        row(painter, COL_DARK_BAR, y);
        let t = ((self.sim.sun_angle + 80.0) * 0.0625) + 8.0;
        let mut whole = cut(t);
        if whole != i32::MIN && whole as f32 != t {
            whole -= t.is_sign_negative() as i32;
        }
        let hf = whole as f32;
        let text = format!("{}: {}:{:02}", translate("time"), cut(hf), cut((t - hf) * 60.0));
        line(self, painter, &textures[2], &text, y);
        y = y + ROW;

        // the air and the wind
        row(painter, COL_DARK_TRANSP, y);
        let text = format!("{}: {}\u{b0}C ", translate("air"), cut(self.sim.ambient_temperature));
        line(self, painter, &textures[3], &text, y);
        let text = format!("{}: {} km/h", translate("wind"), cut(self.sim.wind_speed * 3.6));
        self.fonts[data_font].blit_string(painter, x + 170.0, y + 6.1, &text, 1.0, ALIGN_LEFT);
        painter.set_texture(0, &textures[6]);
        let (tw, th) = size(painter, &textures[6]);
        painter.gl_quad_centred_rotated(gl, x + 155.0, y + 15.0, tw as f32, th as f32, self.sim.wind_direction_deg * 0.017453, true);
        y = y + ROW;

        // the road
        row(painter, COL_DARK_BAR, y);
        let text = format!("{}: {}\u{b0}C", translate("asphalt"), cut(self.sim.road_temperature));
        line(self, painter, &textures[4], &text, y);
        y = y + ROW;

        // the session that comes next
        row(painter, COL_DARK_TRANSP, y);
        let race = &self.sim.race;
        let (index, count) = (race.current_session_index, self.sim.session_count);
        let text = if (index + 1) as u32 >= count {
            translate(if count == 1 { "single session" } else { "last session" })
        } else {
            let next = race.session_info(index + 1);
            let name = upper(&next.name);
            if next.session_type == SESSION_RACE && !next.is_timed_race {
                format!("{}: {name} - {} L", translate("next session"), next.laps)
            } else {
                format!("{}: {name} - {}", translate("next session"), time_to_wstring(cut(next.duration_minutes * 60000.0), true, false))
            }
        };
        line(self, painter, &textures[5], &text, y);
    }
}
