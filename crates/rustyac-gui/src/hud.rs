// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! The HUD's messages: `SystemMessage` (0x98 bytes) with its three faces, the banner of two
//! lines (`DefaultSystemMessage`, a widget), `DriverStatusSystemMessage` (0x228, "now
//! watching this car") and `SessionStatusSystemMessage` (0x58, the session's line at the top
//! of the screen), and `SystemNotification` (0xa8: the down-shift protection icon and the
//! "steering wheel is not centered" warning before a race start).
//!
//! `SystemMessage::SystemMessage` 0x1401b5610, `setMask` 0x1401b65d0, `addMessage`
//! 0x1401b61c0, `addCarMessage` 0x1401b60b0, `clearMessage` 0x1401b6400, `loadDriversInfo`
//! 0x1401b64f0, `update` 0x1401b66b0, `renderHUD` 0x1401b6520; `SessionStatusSystemMessage`
//! ctor 0x140169550, `update` 0x140169bc0, `render` 0x140169ac0; `DriverStatusSystemMessage`
//! ctor 0x14008e4a0, `loadDriversInfo` 0x14008f0b0, `render` 0x14008f800;
//! `SystemNotification::SystemNotification` 0x1401b68a0, `clearMessage` 0x1401b78b0,
//! `showNotification` 0x1401b7910, `update` 0x1401b79c0; `timeToString` 0x140053110.
//!
//! The game's objects read the session through `Sim`; here whoever runs the HUD hands in a
//! [`HudInput`] every frame. Texts go through [`translate`] where the game calls
//! `acTranslate` (English as it is, until the string tables are ported).

use std::collections::BTreeMap;

use rustyac_physics::data::ini::IniReader;
use rustyac_physics::vecmath::Mat44f;

use crate::control::Id;
use crate::font::{Font, FontType};
use crate::gui::Gui;
use crate::painter::{Painter, Texture, ALIGN_CENTER, ALIGN_LEFT, ALIGN_RIGHT, BLEND_ALPHA, GL_LINES, GRAPHICS_GL};
use crate::widgets::Widget;

/// `SystemMessageType`.
pub const MESSAGE_DEFAULT: i32 = 0;
pub const MESSAGE_NEW_GHOST: i32 = 1;
pub const MESSAGE_SERVER_PLAYER_DISCONNECTED: i32 = 2;
pub const MESSAGE_SERVER_PLAYER_JOINED: i32 = 3;
pub const MESSAGE_SERVER_KICKED: i32 = 4;
pub const MESSAGE_SERVER_SESSION_VOTING: i32 = 5;
pub const MESSAGE_SERVER_KICK_VOTING: i32 = 6;
pub const MESSAGE_SERVER_VOTE: i32 = 7;
pub const MESSAGE_ABS: i32 = 8;
pub const MESSAGE_TC: i32 = 9;
pub const MESSAGE_TURBO: i32 = 10;
pub const MESSAGE_BRAKE_BIAS: i32 = 11;
pub const MESSAGE_BRAKE_ENGINE: i32 = 12;
pub const MESSAGE_MGU: i32 = 13;

/// `acTranslate` 0x140240cf0: a string the tables do not have comes back as it is.
pub fn translate(text: &str) -> String {
    text.to_string()
}

/// `timeToString(ms, 3)` 0x140053110: minutes, seconds, thousandths.
pub fn time_to_string3(ms: i32) -> String {
    if ms > 0 {
        format!("{}:{:02}:{:03}", ms / 60000, (ms % 60000) / 1000, (ms % 60000) % 1000)
    } else {
        "-:--:---".to_string()
    }
}

/// What the HUD reads of one car (`CarAvatar`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct HudCar {
    /// `driverInfo.name`
    pub driver_name: String,
    /// `guiName`
    pub gui_name: String,
    /// `unixName`: the car's folder
    pub unix_name: String,
    /// `hasPenalty()`: the seconds to serve, 0 for none
    pub penalty_time: f64,
    /// `isMinSpeedPenaltyClearDisabled()`
    pub min_speed_penalty_clear_disabled: bool,
    /// `Car::penaltyManager.pitPenaltyLaps`
    pub pit_penalty_laps: i16,
    /// `physicsState.fuel`
    pub fuel: f32,
    /// `driverInfo.nationCode`
    pub nation_code: String,
    /// `isConnected()`: the car's node is shown
    pub connected: bool,
    /// the translation of `physicsState.worldMatrix`
    pub world_position: [f32; 3],
    /// `bodyTransform->getWorldMatrix()`
    pub body_matrix: Mat44f,
    /// `RaceManager::getLapCount`
    pub lap_count: i32,
    /// `RaceManager::getCarRealTimePosition`
    pub real_time_position: i32,
    /// `ACClient::getPing` (online)
    pub ping: i32,
    /// `physicsState.damageZoneLevel`: front, rear, left, right
    pub damage_zone_level: [f32; 4],
    /// `physicsState.engineLifeLeft`
    pub engine_life_left: f32,
    /// the float at `physicsState` + 0x7f4 the damage picture reads as the gearbox's health
    pub gearbox: f32,
    /// `physicsState.tyreInflation`
    pub tyre_inflation: [f32; 4],
    /// `physicsState.susDamage`
    pub sus_damage: [f32; 4],
    /// `physicsState.tyreFlatSpot`
    pub tyre_flat_spot: [f32; 4],
    /// `physicsState.lockControlsTime`
    pub lock_controls_time: f32,
    /// `isCurrentLapValid()`
    pub current_lap_valid: bool,
    /// `RaceManager::getCarLeaderboardPosition`
    pub leaderboard_position: i32,
    /// `RaceManager::getLastLap(car).time`
    pub last_lap_ms: u32,
    /// `physicsState.steer`, degrees
    pub steer: f32,
    /// `physicsInfo.steerLock`
    pub steer_lock: f32,
}

/// What the HUD reads of the session every frame (the game's `Sim` and what hangs on it).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct HudInput {
    pub cars: Vec<HudCar>,
    /// `Sim::escMenu`: whether it shows, and its width
    pub esc_menu_visible: bool,
    pub esc_menu_width: f32,
    /// `Sim::pauseMenu` shows
    pub pause_menu_visible: bool,
    /// `ReplayManager::isInReplaymode`
    pub in_replay: bool,
    /// `Sim::endSessionDisplayer` is active
    pub end_session_active: bool,
    /// `RaceManager::getCurrentSession().sessionType`: 1 practice, 2 qualify, 3 race
    pub session_type: i32,
    /// `RaceManager::getPracticeQualifyText()`
    pub practice_qualify_text: String,
    /// `RaceManager::getRaceText()`
    pub race_text: String,
    /// `RaceManager::getTimeToSessionStart()`, ms
    pub time_to_session_start: f64,
    /// `ACCameraManager::currentGlobalCameraIndex` and `randomMode` (none: no camera manager)
    pub camera: Option<(i32, bool)>,
    /// `ACCameraManager::mode`: 0 cockpit, 1 car, 2 drivable, 3 track, 4 helicopter, 5 on board free, 6 free, 7 start
    pub camera_mode: i32,
    /// `Sim::getFocusedCarIndex` (the player's car is 0)
    pub focused_car_index: i32,
    /// `RaceManager::isRaceOver(0)`
    pub race_over: bool,
    /// `RaceManager::getSessionTimeLeft()`, ms
    pub session_time_left: f64,
    /// `Sim::isInTripleScreenMode`
    pub triple_screen: bool,
    /// `Speed::useMPH`
    pub use_mph: bool,
    /// `Sim::sceneCamera->matrix`: where the camera is
    pub camera_matrix: Mat44f,
    /// `Sim::client` is there
    pub online: bool,
    /// `Game::gameTime.now`
    pub game_time_now: f64,
}

/// `SessionStatusSystemMessage`.
pub struct SessionStatus {
    pub font: Font,
    pub text_to_render: String,
    pub last_current_lap_valid: bool,
    pub invalid_lap_message_time: f32,
    pub position: [f32; 2],
    pub background_color: [f32; 4],
    pub pratice_visible: bool,
    pub qualify_visible: bool,
    pub race_visible: bool,
}

/// `DriverStatusSystemMessage`: a control with five labels, drawn by hand.
pub struct DriverStatus {
    pub control: Id,
    pub current_car_index: u32,
    pub has_camera_manager: bool,
    pub lab_driver_name: Id,
    pub lab_car_name: Id,
    pub lab_camera_description: Id,
    pub lab_driver_position: Id,
    pub lab_last_time: Id,
    pub car_logos: Vec<Texture>,
    pub driver_nationality: Vec<Texture>,
    pub camera_names: Vec<String>,
    pub bar: Texture,
}

pub struct SystemMessage {
    /// 0 the banner, 1 the driver's bar, anything else the session's line
    pub current_status: i32,
    pub timer: f32,
    /// `messageMask`: a kind of message that is switched off in `messages.ini`
    pub message_mask: BTreeMap<i32, bool>,
    pub default_system_message: Id,
    pub driver: DriverStatus,
    pub session: SessionStatus,
}

impl SystemMessage {
    /// `SystemMessage::SystemMessage` 0x1401b5610. `messages` is `system/cfg/messages.ini`,
    /// `session_info` `system/cfg/session_info.ini` (none: the file is not there).
    pub fn new(gui: &mut Gui, painter: &mut dyn Painter, messages: Option<&IniReader>, session_info: Option<&IniReader>) -> SystemMessage {
        let default_system_message = gui.new_default_system_message(painter);

        // DriverStatusSystemMessage 0x14008e4a0
        let control = gui.new_control(painter, "DriverStatusSystemMessage");
        gui.controls[control].back_color = [0.0, 0.0, 0.0, 0.5];
        gui.controls[control].border_color = [0.5, 0.5, 0.5, 0.95];
        let bar = painter.get_texture("content/gui/systemMessage/bar.png");
        let mut labels = [0; 5];
        let specs: [(&str, Option<(f32, f32)>, i32, f32, bool); 5] = [
            ("DRIVER_NAME", Some((300.0, 25.0)), ALIGN_RIGHT, 21.0, false),
            ("CAR_NAME", Some((300.0, 25.0)), ALIGN_LEFT, 21.0, true),
            ("CAMERA_DESCRIPTION", Some((220.0, 25.0)), ALIGN_RIGHT, 18.0, false),
            ("DRIVER_POSITION", None, ALIGN_CENTER, 18.0, false),
            ("CAR_NAME", Some((220.0, 25.0)), ALIGN_LEFT, 18.0, false),
        ];
        for (slot, (name, size, align, font_size, italic)) in labels.iter_mut().zip(specs) {
            let label = gui.new_label(painter, name);
            if let Some((w, h)) = size {
                gui.set_size(label, w, h);
            }
            gui.controls[control].fore_color = [1.0; 4];
            gui.controls[label].fore_color = [1.0; 4];
            gui.controls[label].draw_background = false;
            gui.controls[label].font_align = align;
            gui.fonts.push(Font::new(painter, "", FontType::Proportional, font_size, italic, false));
            gui.controls[label].font = Some(gui.fonts.len() - 1);
            gui.control_add_control(control, label);
            *slot = label;
        }
        let driver = DriverStatus {
            control,
            current_car_index: 0,
            has_camera_manager: false,
            lab_driver_name: labels[0],
            lab_car_name: labels[1],
            lab_camera_description: labels[2],
            lab_driver_position: labels[3],
            lab_last_time: labels[4],
            car_logos: Vec::new(),
            driver_nationality: Vec::new(),
            camera_names: Vec::new(),
            bar,
        };

        // SessionStatusSystemMessage 0x140169550
        let mut session = SessionStatus {
            font: Font::new(painter, "", FontType::Proportional, 16.0, false, true),
            text_to_render: String::new(),
            last_current_lap_valid: false,
            invalid_lap_message_time: 0.0,
            position: [0.0; 2],
            background_color: [0.0; 4],
            pratice_visible: true,
            qualify_visible: true,
            race_visible: true,
        };
        match session_info.filter(|ini| ini.ready) {
            Some(ini) => {
                session.background_color = ini.get_float4("OPTIONS", "BACKGROUND_COLOR").unwrap_or([0.0; 4]);
                session.pratice_visible = ini.get_int("OPTIONS", "PRATICE_VISIBLE").unwrap_or(0) != 0;
                session.qualify_visible = ini.get_int("OPTIONS", "QUALIFY_VISIBLE").unwrap_or(0) != 0;
                session.race_visible = ini.get_int("OPTIONS", "RACE_VISIBLE").unwrap_or(0) != 0;
                session.font.set_color4(&ini.get_float4("OPTIONS", "FONT_COLOR").unwrap_or([0.0; 4]));
            }
            None => {
                session.background_color = [0.0, 0.0, 0.0, 0.3];
                session.font.set_color(1.0, 1.0, 1.0, 1.0);
            }
        }

        let mut message_mask = BTreeMap::new();
        if let Some(ini) = messages.filter(|ini| ini.ready) {
            let keys = [
                ("NEW_GHOST", MESSAGE_NEW_GHOST),
                ("SERVER_PLAYER_JOINED", MESSAGE_SERVER_PLAYER_JOINED),
                ("SERVER_PLAYER_DISCONNECTED", MESSAGE_SERVER_PLAYER_DISCONNECTED),
                ("SERVER_KICKED_MSG", MESSAGE_SERVER_KICKED),
                ("SERVER_SESSION_VOTING", MESSAGE_SERVER_SESSION_VOTING),
                ("SERVER_KICK_VOTING", MESSAGE_SERVER_KICK_VOTING),
                ("SERVER_VOTE", MESSAGE_SERVER_VOTE),
                ("ABS", MESSAGE_ABS),
                ("TC", MESSAGE_TC),
                ("TURBO", MESSAGE_TURBO),
                ("BRAKE_BIAS", MESSAGE_BRAKE_BIAS),
                ("BRAKE_ENGINE", MESSAGE_BRAKE_ENGINE),
                ("MGU", MESSAGE_MGU),
            ];
            for (key, kind) in keys {
                // setMask 0x1401b65d0: on, unless the key is there with another value than 1
                let on = if ini.has_key("SYSTEM", key) { ini.get_int("SYSTEM", key).unwrap_or(0) == 1 } else { true };
                message_mask.insert(kind, on);
            }
        }
        SystemMessage { current_status: 0, timer: 0.0, message_mask, default_system_message, driver, session }
    }

    fn hidden(&self, kind: i32) -> bool {
        self.message_mask.get(&kind) == Some(&false)
    }

    /// `SystemMessage::addMessage` 0x1401b61c0: the banner shows the two lines for five
    /// seconds; a new message takes the place of the one that shows.
    pub fn add_message(&mut self, gui: &mut Gui, title: &str, description: &str, kind: i32) {
        if self.hidden(kind) {
            println!("System hidden message\n - {title}\n - {description}");
            return;
        }
        self.current_status = 0;
        gui.default_system_message_set(self.default_system_message, title, description);
        self.timer = 5.0;
    }

    /// `SystemMessage::addCarMessage` 0x1401b60b0: the bar of a car, for five seconds.
    pub fn add_car_message(&mut self, car_index: u32, kind: i32) {
        if self.hidden(kind) {
            return;
        }
        self.current_status = 1;
        self.driver.current_car_index = car_index;
        self.timer = 5.0;
    }

    /// `SystemMessage::clearMessage` 0x1401b6400.
    pub fn clear_message(&mut self) {
        self.timer = -1.0;
    }

    /// `SystemMessage::loadDriversInfo` 0x1401b64f0 (once the cars are there): the cars' logos,
    /// the flags (always the default one) and the cameras' names. `track_cameras` are the
    /// names of the track's camera sets when there is a camera manager.
    pub fn load_drivers_info(&mut self, painter: &mut dyn Painter, cars: &[HudCar], track_cameras: Option<&[String]>) {
        let d = &mut self.driver;
        d.driver_nationality.resize(cars.len(), Texture::default());
        d.car_logos.resize(cars.len(), Texture::default());
        for (i, car) in cars.iter().enumerate() {
            d.driver_nationality[i] = painter.get_texture("content/gui/Nationality/default.png");
            d.car_logos[i] = painter.get_texture(&format!("content/cars/{}/logo.png", car.unix_name));
        }
        d.has_camera_manager = track_cameras.is_some();
        if let Some(track_cameras) = track_cameras {
            for name in ["Cockpit", "Chase 1", "Chase 2", "Bonnet", "Bumper", "Dash"] {
                d.camera_names.push(translate(name));
            }
            d.camera_names.extend(track_cameras.iter().cloned());
        }
        self.timer = -1.0;
    }

    /// `SystemMessage::update` 0x1401b66b0: the two bars go from the left edge (or the ESC
    /// menu's right) to the app bar; the session's line is made.
    pub fn update(&mut self, gui: &mut Gui, dt: f32, input: &HudInput) {
        let screen_w = gui.screen.0 as f32;
        for control in [self.default_system_message, self.driver.control] {
            if !input.esc_menu_visible {
                gui.set_position(control, 0.0, 0.0);
            } else {
                gui.set_position(control, input.esc_menu_width, 0.0);
            }
        }
        for control in [self.default_system_message, self.driver.control] {
            if !input.esc_menu_visible {
                let a = gui.controls[gui.taskbar].rect.left - 1.0;
                let w = if screen_w <= a { screen_w } else { a };
                gui.set_size(control, w, 80.0);
            } else {
                gui.set_size(control, screen_w - input.esc_menu_width, 80.0);
            }
        }

        // SessionStatusSystemMessage::update 0x140169bc0
        let s = &mut self.session;
        s.text_to_render = String::new();
        let Some(car) = input.cars.first() else { return };
        if car.penalty_time > 0.0 || 0.0 < car.lock_controls_time || input.esc_menu_visible || input.pause_menu_visible || input.in_replay || input.end_session_active {
            return;
        }
        let valid = car.current_lap_valid;
        if !valid && s.last_current_lap_valid {
            s.invalid_lap_message_time = 5.0;
        }
        s.last_current_lap_valid = valid;
        if s.invalid_lap_message_time > 0.0 {
            s.invalid_lap_message_time -= dt;
            return;
        }
        match input.session_type {
            1 if s.pratice_visible => s.text_to_render = input.practice_qualify_text.clone(),
            2 if s.qualify_visible => s.text_to_render = input.practice_qualify_text.clone(),
            3 if s.race_visible => s.text_to_render = input.race_text.clone(),
            _ => {}
        }
    }

    /// `SystemMessage::renderHUD` 0x1401b6520.
    pub fn render_hud(&mut self, gui: &mut Gui, painter: &mut dyn Painter, dt: f32, input: &HudInput) {
        if input.esc_menu_visible {
            self.timer = -1.0;
        }
        if !(0.0 < self.timer) {
            self.session_render(gui, painter);
            return;
        }
        painter.set_blend_mode(BLEND_ALPHA);
        match self.current_status {
            0 => gui.render(self.default_system_message, dt, painter),
            1 => self.driver_render(gui, painter, dt, input),
            _ => self.session_render(gui, painter),
        }
        self.timer -= dt;
    }

    /// `SessionStatusSystemMessage::render` 0x140169ac0: ten pixels a character of dark box at
    /// the top of the screen, the text centred in it.
    fn session_render(&mut self, gui: &Gui, painter: &mut dyn Painter) {
        let s = &mut self.session;
        if s.text_to_render.is_empty() {
            return;
        }
        let w = gui.screen.0 as f32;
        let n = s.text_to_render.encode_utf16().count() as f32;
        let c = s.background_color;
        painter.gl_color4f(GRAPHICS_GL, c[0], c[1], c[2], c[3]);
        painter.gl_quad(GRAPHICS_GL, w * 0.5 - n * 5.0, s.position[1], n * 10.0, 24.0, false);
        let text = s.text_to_render.clone();
        s.font.blit_string(painter, w * 0.5, 0.0, &text, 1.0, ALIGN_CENTER);
    }

    /// `DriverStatusSystemMessage::render` 0x14008f800.
    fn driver_render(&mut self, gui: &mut Gui, painter: &mut dyn Painter, dt: f32, input: &HudInput) {
        let d = &mut self.driver;
        let r = gui.controls[d.control].rect;
        let (w, h) = (r.right - r.left, r.bottom - r.top);
        let cx = w * 0.5;
        let y1 = h * 0.25 - 12.0;
        let y2 = y1 + 40.0;
        if d.current_car_index as usize > input.cars.len() {
            return;
        }
        let Some(car) = input.cars.get(d.current_car_index as usize) else { return };
        gui.set_text(d.lab_driver_name, &car.driver_name);
        let l = gui.controls[d.lab_driver_name].rect;
        gui.set_position(d.lab_driver_name, (cx - 30.0) - (l.right - l.left), y1);
        gui.set_text(d.lab_car_name, &car.gui_name);
        gui.set_position(d.lab_car_name, cx + 30.0, y1);
        // (the game compares the car's index with the number of camera names here)
        if (d.current_car_index as usize) < d.camera_names.len() {
            let name = match input.camera {
                Some((index, random)) if d.has_camera_manager && index >= 0 && (index as usize) < d.camera_names.len() && !random => d.camera_names[index as usize].clone(),
                _ => String::new(),
            };
            gui.set_text(d.lab_camera_description, &name);
            gui.set_position(d.lab_camera_description, cx - 330.0, y2);
        }
        let position = car.leaderboard_position;
        let text = match input.session_type {
            1 => translate("Practice"),
            2 => {
                let mut s = translate("Qualify");
                if position >= 0 {
                    s = format!("{s} : {position}");
                }
                s
            }
            3 => {
                let mut s = translate("Race");
                if position >= 0 {
                    s = format!("{} : {position}", translate("Position"));
                }
                s
            }
            _ => String::new(),
        };
        gui.set_text(d.lab_driver_position, &text);
        gui.set_size(d.lab_driver_position, w, 25.0);
        gui.set_position(d.lab_driver_position, 0.0, y2);
        gui.set_text(d.lab_last_time, &format!("{} {}", translate("Last"), time_to_string3(car.last_lap_ms as i32)));
        gui.set_position(d.lab_last_time, cx + 110.0, y2);
        gui.control_render(d.control, dt, painter);
        let gl = GRAPHICS_GL;
        if let Some(flag) = d.driver_nationality.get(d.current_car_index as usize) {
            painter.gl_color4f(gl, 1.0, 1.0, 1.0, 1.0);
            painter.set_texture(0, flag);
            let x = if input.esc_menu_visible { input.esc_menu_width + (cx - 15.0) } else { cx - 15.0 };
            painter.gl_quad(gl, x, y1, 30.0, 20.0, true);
        }
        let y3 = h * 0.5 + 6.0;
        let border = gui.controls[d.control].border_color;
        painter.gl_color4f(gl, border[0], border[1], border[2], border[3]);
        painter.set_texture(0, &d.bar);
        painter.gl_quad(gl, cx - 275.0, h * 0.5, 550.0, 1.0, true);
        for x in [cx - 85.0, cx + 85.0] {
            painter.gl_color4f(gl, border[0], border[1], border[2], border[3]);
            painter.gl_begin(gl, GL_LINES);
            painter.gl_vertex3f(gl, x, y3, 0.0);
            painter.gl_vertex3f(gl, x, y3 + 28.0, 0.0);
            painter.gl_end(gl);
        }
        if let Some(logo) = d.car_logos.get(d.current_car_index as usize) {
            painter.gl_color4f(gl, 1.0, 1.0, 1.0, 1.0);
            painter.set_texture(0, logo);
            painter.gl_quad(gl, (cx - 275.0) - 80.0, 5.0, 70.0, 70.0, true);
        }
    }
}

/// `SystemNotification`: four controls on the game screen, switched by `update`.
pub struct SystemNotification {
    pub control_down_shift: Id,
    pub control_rotated_wheel: Id,
    pub pb_right: Id,
    pub pb_left: Id,
    /// 0 nothing, 1 down-shift protection, 2 the wheel is not centred
    pub current_status: i32,
    pub timer: f32,
    pub blink_time: f32,
    pub show: bool,
    pub max_timer: [f32; 2],
    pub max_blink_time: f32,
    pub wheel_centered_active: bool,
}

impl SystemNotification {
    /// `SystemNotification::SystemNotification` 0x1401b68a0. `gameplay` is `cfg/gameplay.ini`.
    pub fn new(gui: &mut Gui, painter: &mut dyn Painter, game_screen: Id, gameplay: Option<&IniReader>) -> SystemNotification {
        let mut n = SystemNotification { control_down_shift: 0, control_rotated_wheel: 0, pb_right: 0, pb_left: 0, current_status: 0, timer: 0.0, blink_time: 0.0, show: false, max_timer: [2.2, 3.0], max_blink_time: 0.3, wheel_centered_active: true };
        if let Some(ini) = gameplay.filter(|ini| ini.ready) {
            let section = "DOWNSHIFT_PROTECTION_NOTIFICATION";
            if ini.has_section(section) {
                if ini.has_key(section, "TIME_SHOW") {
                    n.max_timer[0] = ini.get_float(section, "TIME_SHOW").unwrap_or(0.0);
                }
                if ini.has_key(section, "TIME_BLINKING") {
                    n.max_blink_time = ini.get_float(section, "TIME_BLINKING").unwrap_or(0.0);
                }
            }
            let section = "STEERING_CENTERED_NOTIFICATION";
            if ini.has_section(section) && ini.has_key(section, "ACTIVE") {
                n.wheel_centered_active = ini.get_float(section, "ACTIVE").unwrap_or(0.0) != 0.0;
            }
        }
        let (width, height) = painter.screen();
        let (w, h) = (width as f32, height as f32);
        let half = (width / 2) as f32;

        let down_shift = gui.new_control(painter, "DownShiftProtection");
        let texture = painter.get_texture("content/gui/SystemNotification/down_shift_protection.png");
        let (tex_w, tex_h) = painter.texture_size(&texture);
        let y_d = h * f32::from_bits(0x3a72_b9d6) * 64.0;
        gui.set_position(down_shift, half - tex_w as f32 * 0.5 * 0.5, y_d);
        gui.controls[down_shift].back_texture = texture;
        gui.set_size(down_shift, tex_w as f32 * 0.5, tex_h as f32 * 0.5);
        gui.controls[down_shift].draw_background = false;
        gui.controls[down_shift].draw_border = false;
        gui.set_visible(down_shift, false);
        gui.controls[down_shift].back_texture_color = [1.0, 1.0, 0.0, 1.0];
        gui.control_add_control(game_screen, down_shift);

        let wheel = gui.new_control(painter, "DownShiftProtection");
        let y = y_d + 100.0;
        let text = translate("Warning, steering wheel is not centered");
        gui.set_text(wheel, &text);
        let tw = text.encode_utf16().count() as f32 * 12.0;
        gui.set_size(wheel, tw, 25.0);
        gui.set_position(wheel, half - tw * 0.5, y);
        gui.controls[wheel].font_align = ALIGN_CENTER;
        let mut font = Font::new(painter, "Orbitron", FontType::Custom, 18.0, false, false);
        font.shadowed = true;
        font.shadow_pixel_distance = 1.0;
        gui.fonts.push(font);
        gui.controls[wheel].font = Some(gui.fonts.len() - 1);
        gui.controls[wheel].fore_color = [0.9, 0.0, 0.0, 1.0];
        gui.controls[wheel].draw_background = true;
        gui.controls[wheel].back_color = [1.0, 1.0, 1.0, 0.2];
        gui.controls[wheel].draw_border = true;
        gui.set_visible(wheel, false);
        gui.control_add_control(game_screen, wheel);

        let y2 = y + 40.0;
        let left = gui.new_progress_bar(painter, "LEFT");
        gui.set_size(left, tw * 0.5, 5.0);
        let r = gui.controls[left].rect;
        gui.set_position(left, w * 0.5 - (r.right - r.left), y2);
        gui.controls[left].fore_color = [1.0, 0.0, 0.0, 1.0];
        gui.controls[left].border_color = [1.0, 1.0, 1.0, 0.0];
        {
            let p = gui.progress_bar_mut(left);
            p.is_vertical = false;
            p.value = 0.0;
            p.is_inverted = true;
        }
        gui.set_visible(left, false);
        gui.control_add_control(game_screen, left);

        let right = gui.new_progress_bar(painter, "RIGHT");
        gui.set_size(right, tw * 0.5, 5.0);
        gui.set_position(right, w * 0.5, y2);
        gui.controls[right].fore_color = [1.0, 0.0, 0.0, 1.0];
        gui.controls[right].border_color = [1.0, 1.0, 1.0, 0.0];
        {
            let p = gui.progress_bar_mut(right);
            p.is_vertical = false;
            p.is_inverted = false;
            p.value = 0.0;
        }
        gui.set_visible(right, false);
        gui.control_add_control(game_screen, right);

        n.control_down_shift = down_shift;
        n.control_rotated_wheel = wheel;
        n.pb_left = left;
        n.pb_right = right;
        n.clear_message(gui);
        n
    }

    /// `SystemNotification::clearMessage` 0x1401b78b0.
    pub fn clear_message(&mut self, gui: &mut Gui) {
        for control in [self.control_down_shift, self.control_rotated_wheel, self.pb_left, self.pb_right] {
            gui.set_visible(control, false);
        }
        self.timer = -1.0;
        self.show = false;
        self.current_status = 0;
    }

    fn show_wheel(&mut self, gui: &mut Gui) {
        gui.set_visible(self.control_down_shift, false);
        gui.set_visible(self.control_rotated_wheel, true);
        gui.set_visible(self.pb_left, true);
        gui.set_visible(self.pb_right, true);
        self.timer = self.max_timer[1];
    }

    /// `SystemNotification::showNotification` 0x1401b7910 (1: the car refused a down shift).
    pub fn show_notification(&mut self, gui: &mut Gui, mode: i32) {
        self.show = true;
        self.current_status = mode;
        if mode == 1 {
            gui.set_visible(self.control_down_shift, true);
            gui.set_visible(self.control_rotated_wheel, false);
            gui.set_visible(self.pb_left, false);
            gui.set_visible(self.pb_right, false);
            self.blink_time = self.max_blink_time;
            self.timer = self.max_timer[0];
        } else if mode == 2 {
            self.show_wheel(gui);
        }
    }

    /// `SystemNotification::update` 0x1401b79c0.
    pub fn update(&mut self, gui: &mut Gui, dt: f32, input: &HudInput) {
        let car = input.cars.first().cloned().unwrap_or_default();
        if self.wheel_centered_active && input.session_type == 3 && input.time_to_session_start < 30000.0 && input.time_to_session_start > 1000.0 {
            if car.steer.abs() > 90.0 {
                if self.current_status != 2 {
                    self.show = true;
                    self.current_status = 2;
                    self.show_wheel(gui);
                }
            } else if self.current_status == 2 {
                self.clear_message(gui);
            }
        }
        if input.esc_menu_visible || !(self.timer > 0.0) {
            self.clear_message(gui);
            return;
        }
        if !self.show {
            return;
        }
        if self.current_status == 2 {
            let mut a = car.steer.abs() / car.steer_lock;
            if a > 1.0 {
                a = 1.0;
            } else if a < 0.0 {
                a = 0.0;
            }
            let color = [a * 2.0, (1.0 - a) * 2.0, 0.0, 1.0];
            let (bar, other) = if car.steer >= 0.0 { (self.pb_right, self.pb_left) } else { (self.pb_left, self.pb_right) };
            gui.controls[bar].fore_color = color;
            if let crate::control::Kind::Widget(widget) = &mut gui.controls[bar].kind {
                if let Widget::ProgressBar(p) = &mut **widget {
                    p.value = a;
                }
            }
            gui.set_visible(bar, true);
            gui.set_visible(other, false);
            self.timer -= dt;
        } else if self.current_status == 1 {
            self.blink_time -= dt;
            if self.blink_time > 0.0 || self.max_blink_time == 0.0 {
                self.timer -= dt;
            } else {
                self.blink_time = self.max_blink_time;
                let visible = gui.controls[self.control_down_shift].visible;
                gui.set_visible(self.control_down_shift, !visible);
            }
        }
    }
}
