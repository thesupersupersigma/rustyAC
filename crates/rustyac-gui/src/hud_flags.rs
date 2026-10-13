// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! The HUD's flags and warnings: what `FlagManager` (0x108 bytes) shows (five pictures and a
//! caption that are plain controls of the game screen), the three banners
//! `LapInvalidatorAvatar` (0xb8), `DriveThroughPenaltyAvatar` (0x78) and
//! `LockControlsTimeAvatar` (0x90), `WrongWayIndicator` (0xb8) and `FuelLight` (0xd8).
//!
//! `FlagManager::FlagManager` 0x14009ab60, `addFlagControl` 0x14009bfb0, `update`
//! 0x14009d140, `setFlag` 0x14009cf00, `clearFlag` 0x14009cb60, `setBlueFlag` 0x14009ce30;
//! `LapInvalidatorAvatar` ctor 0x140111990, `renderHUD` 0x140111c00;
//! `DriveThroughPenaltyAvatar` ctor 0x1400907b0, `renderHUD` 0x140090b50;
//! `LockControlsTimeAvatar` ctor 0x14009d8c0, `renderHUD` 0x14009dc50; `WrongWayIndicator`
//! ctor 0x1401d83d0, `update` 0x1401d8940, `renderHUD` 0x1401d87a0; `FuelLight` ctor
//! 0x1400bb510, `renderHUD` 0x1400bc030, its flag handler 0x1400bbad0.
//!
//! Which flag flies is the referee's business (`rustyac_physics::race::flags`, proven in Task
//! 28): [`FlagDisplay::update`] takes the three checks of the game's `update` as a closure, and
//! [`FlagDisplay::show`] takes the referee's answer as it is.

use rustyac_physics::data::ini::IniReader;

use crate::control::Id;
use crate::font::{Font, FontType};
use crate::gui::Gui;
use crate::hud::{translate, HudInput, SystemMessage};
use crate::painter::{Painter, Texture, ALIGN_CENTER, ALIGN_LEFT, BLEND_ALPHA, GL_LINES, GRAPHICS_GL};

/// `FlagType`.
pub const FLAG_NONE: i32 = 0;
pub const FLAG_BLUE: i32 = 1;
pub const FLAG_BLACK: i32 = 2;
pub const FLAG_CHEQUERED: i32 = 3;
pub const FLAG_YELLOW: i32 = 4;
pub const FLAG_PENALTY: i32 = 6;

/// `FlagTriggered` as `evOnFlagChange` carries it (the car is always the player's).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FlagTriggered {
    pub flag: i32,
    pub is_active: bool,
    pub flag_position: [f32; 2],
    pub flag_width: f32,
}

/// The part of `FlagManager` that is on the screen.
pub struct FlagDisplay {
    pub timer: f32,
    pub current_flag: i32,
    pub control_blue: Id,
    pub control_black: Id,
    pub control_chequered: Id,
    pub control_yellow: Id,
    pub control_penalty: Id,
    pub black_flag_caption: Id,
    /// `evOnFlagChange` as it was fired since the owner last emptied this
    pub events: Vec<FlagTriggered>,
}

impl FlagDisplay {
    /// The controls of `FlagManager::FlagManager` 0x14009ab60, added to the game screen.
    pub fn new(gui: &mut Gui, painter: &mut dyn Painter, screen: Id, triple_screen: bool) -> FlagDisplay {
        let mut font = Font::new(painter, "", FontType::Proportional, 25.0, false, true);
        font.shadowed = true;
        gui.fonts.push(font);
        let font = gui.fonts.len() - 1;
        let control_blue = gui.new_control(painter, "FLAG_BLUE");
        let control_black = gui.new_control(painter, "FLAG_BLACK");
        let control_chequered = gui.new_control(painter, "FLAG_CHEQUERED");
        let control_yellow = gui.new_control(painter, "FLAG_YELLOW");
        let control_penalty = gui.new_control(painter, "FLAG_PENALTY");
        let (w, h) = gui.screen;
        for (control, name) in [(control_blue, "blueFlag"), (control_black, "blackFlag"), (control_chequered, "finish"), (control_yellow, "yellowFlag"), (control_penalty, "penalty")] {
            // addFlagControl 0x14009bfb0
            let texture = painter.get_texture(&format!("content/gui/flags/{name}.png"));
            let (tw, th) = painter.texture_size(&texture);
            gui.controls[control].back_texture = texture;
            gui.set_size(control, tw as f32, th as f32);
            gui.controls[control].draw_border = false;
            gui.controls[control].draw_background = false;
            gui.set_visible(control, false);
            let r = gui.controls[control].rect;
            let (mut x, mut y) = (15.0f32, 15.0f32);
            if !triple_screen {
                if control == control_black {
                    x = (w / 2) as f32 - (r.right - r.left) * 0.5;
                    y = (h / 2) as f32 - (r.bottom - r.top) * 0.5;
                }
            } else if control == control_black {
                x = ((w / 2) as f32 - (r.right - r.left) * 0.5) + (w / 3) as f32;
                y = (h / 2) as f32 - (r.bottom - r.top) * 0.5;
            } else {
                x = (w / 3) as f32 + 15.0;
            }
            gui.set_position(control, x, y);
            gui.game_screen_add_control(painter, screen, control, false);
        }
        let caption = gui.new_label(painter, "BLACK_FLAG_CAPTION");
        gui.set_size(caption, 400.0, 21.0);
        let r = gui.controls[caption].rect;
        let mut x = (w / 2) as f32 - (r.right - r.left) * 0.5;
        let y = (h / 2) as f32 - (r.bottom - r.top) * 0.5;
        if triple_screen {
            x = x + (w / 3) as f32;
        }
        gui.set_position(caption, x, y);
        gui.controls[caption].font_align = ALIGN_CENTER;
        gui.controls[caption].fore_color = [1.0, 0.0, 0.0, 1.0];
        gui.controls[caption].font = Some(font);
        gui.set_text(caption, "");
        gui.game_screen_add_control(painter, screen, caption, false);
        FlagDisplay { timer: 0.0, current_flag: FLAG_NONE, control_blue, control_black, control_chequered, control_yellow, control_penalty, black_flag_caption: caption, events: Vec::new() }
    }

    fn fire(&mut self, flag: i32, is_active: bool) {
        self.events.push(FlagTriggered { flag, is_active, flag_position: [15.0, 15.0], flag_width: 0.0 });
    }

    /// `FlagManager::setFlag` 0x14009cf00.
    pub fn set_flag(&mut self, flag: i32) {
        match flag {
            FLAG_NONE => {
                self.current_flag = FLAG_NONE;
                self.fire(FLAG_NONE, false);
            }
            FLAG_BLACK => {
                if self.timer <= 0.0 || self.current_flag != FLAG_BLACK {
                    self.current_flag = FLAG_BLACK;
                    self.fire(FLAG_BLACK, true);
                }
            }
            FLAG_CHEQUERED => {
                self.current_flag = FLAG_CHEQUERED;
                self.timer = -1.0;
                self.fire(FLAG_CHEQUERED, true);
            }
            FLAG_YELLOW => {
                if (self.current_flag.wrapping_sub(2) as u32) >= 2 {
                    self.current_flag = FLAG_YELLOW;
                    self.timer = 5.0;
                    self.fire(FLAG_YELLOW, true);
                }
            }
            FLAG_PENALTY => {
                self.current_flag = FLAG_PENALTY;
                self.fire(FLAG_PENALTY, true);
            }
            _ => {}
        }
    }

    /// `FlagManager::clearFlag` 0x14009cb60.
    pub fn clear_flag(&mut self) {
        self.fire(FLAG_NONE, false);
        self.current_flag = FLAG_NONE;
        self.timer = -1.0;
    }

    /// `FlagManager::setBlueFlag` 0x14009ce30.
    pub fn set_blue_flag(&mut self) {
        if (self.current_flag.wrapping_sub(1) as u32) > 3 || self.current_flag == FLAG_CHEQUERED {
            self.current_flag = FLAG_BLUE;
            self.fire(FLAG_BLUE, true);
        }
    }

    /// The black flag's lambda 0x14009b830: the flag, and the caption under it (already
    /// translated; empty when the flag is lifted).
    pub fn on_black_flag(&mut self, gui: &mut Gui, flag: bool, caption: &str) {
        if flag {
            self.set_flag(FLAG_BLACK);
            gui.set_text(self.black_flag_caption, caption);
        } else {
            self.clear_flag();
            gui.set_text(self.black_flag_caption, "");
        }
    }

    fn hide(&self, gui: &mut Gui) {
        for control in [self.control_blue, self.control_black, self.control_yellow, self.control_penalty, self.control_chequered, self.black_flag_caption] {
            gui.set_visible(control, false);
        }
    }

    /// `FlagManager::update` 0x14009d140. `checks` stands where the game calls `checkForBlue`,
    /// `checkForYellow` and `checkforPenalty`.
    pub fn update(&mut self, gui: &mut Gui, dt: f32, input: &HudInput, checks: &mut dyn FnMut(&mut FlagDisplay)) {
        self.hide(gui);
        if input.in_replay || input.pause_menu_visible || input.esc_menu_visible {
            return;
        }
        // (the player's car is car 0 and its guid is 0)
        if input.focused_car_index != 0 {
            return;
        }
        checks(self);
        let ended = input.end_session_active;
        match self.current_flag {
            FLAG_BLUE => {
                if !input.race_over && !ended {
                    gui.set_visible(self.control_blue, true);
                }
            }
            FLAG_BLACK => {
                if !input.race_over && !ended && input.focused_car_index == 0 {
                    gui.set_visible(self.control_black, true);
                    gui.set_visible(self.black_flag_caption, true);
                }
            }
            FLAG_YELLOW => {
                if !input.race_over && !ended && 0.0 < self.timer {
                    gui.set_visible(self.control_yellow, true);
                }
            }
            FLAG_PENALTY => {
                if !input.race_over && !ended {
                    gui.set_visible(self.control_penalty, true);
                }
            }
            _ => {}
        }
        let chequered = if input.session_type == 2 { !(input.session_time_left >= 0.0) } else { input.session_type == 3 && input.race_over && !ended };
        if chequered {
            self.current_flag = FLAG_CHEQUERED;
            self.timer = -1.0;
            self.fire(FLAG_CHEQUERED, true);
            gui.set_visible(self.control_chequered, true);
        }
        self.timer = self.timer - dt;
    }

    /// The pictures as the referee's own `FlagManager::update` decided them (it has the same
    /// gates and the same switch): blue, black, yellow, penalty, chequered.
    pub fn show(&mut self, gui: &mut Gui, blue: bool, black: bool, yellow: bool, penalty: bool, chequered: bool) {
        self.hide(gui);
        for (control, on) in [(self.control_blue, blue), (self.control_black, black), (self.black_flag_caption, black), (self.control_yellow, yellow), (self.control_penalty, penalty), (self.control_chequered, chequered)] {
            if on {
                gui.set_visible(control, true);
            }
        }
    }
}

/// The width the banners span: the screen, or up to the app bar.
fn banner_width(gui: &Gui) -> f32 {
    let a = gui.controls[gui.taskbar].rect.left - 1.0;
    let b = gui.screen.0 as f32;
    if b <= a {
        b
    } else {
        a
    }
}

const BANNER_BACK: [f32; 4] = [0.0, 0.0, 0.0, 0.5];
const BANNER_BORDER: [f32; 4] = [0.5, 0.5, 0.5, 0.95];

/// The dark box 79 pixels high with a grey line under it.
fn backdrop(painter: &mut dyn Painter, w: f32) {
    let (c, b) = (BANNER_BACK, BANNER_BORDER);
    painter.gl_color4f(GRAPHICS_GL, c[0], c[1], c[2], c[3]);
    painter.gl_quad(GRAPHICS_GL, 0.0, 0.0, w, 79.0, false);
    painter.gl_color4f(GRAPHICS_GL, b[0], b[1], b[2], b[3]);
    painter.gl_begin(GRAPHICS_GL, GL_LINES);
    painter.gl_vertex3f(GRAPHICS_GL, 0.0, 80.0, 0.0);
    painter.gl_vertex3f(GRAPHICS_GL, w, 80.0, 0.0);
    painter.gl_end(GRAPHICS_GL);
    painter.gl_color4f(GRAPHICS_GL, 1.0, 1.0, 1.0, 1.0);
}

/// `LapInvalidatorAvatar`: "CUT DETECTED" for five seconds, or the penalty to serve.
pub struct LapInvalidator {
    pub font: Font,
    pub invalid_lap_message_time: f32,
    pub last_current_lap_valid: bool,
}

impl LapInvalidator {
    /// `LapInvalidatorAvatar::LapInvalidatorAvatar` 0x140111990 (its `danger.png` is loaded and
    /// never drawn).
    pub fn new(painter: &mut dyn Painter) -> LapInvalidator {
        let _danger_icon = painter.get_texture("content/texture/danger.png");
        LapInvalidator { font: Font::new(painter, "", FontType::Proportional, 15.0, false, false), invalid_lap_message_time: 0.0, last_current_lap_valid: true }
    }

    /// `LapInvalidatorAvatar::renderHUD` 0x140111c00.
    pub fn render_hud(&mut self, gui: &Gui, painter: &mut dyn Painter, dt: f32, input: &HudInput, message: &mut SystemMessage) {
        let Some(car) = input.cars.first() else { return };
        let pen = car.penalty_time;
        if pen > 0.0 && !input.in_replay && !input.pause_menu_visible {
            let w = banner_width(gui);
            message.clear_message();
            backdrop(painter, w);
            let mut line = format!("{} {} {}", translate("PENALTY, SLOW DOWN FOR"), pen as i32, translate("SECONDS"));
            if !car.min_speed_penalty_clear_disabled {
                line = format!("{line} {} {}", translate("OR SLOW DOWN TO"), if input.use_mph { "21 MPH" } else { "35 KMH" });
            }
            self.font.set_color(1.0, 0.0, 0.0, 1.0);
            self.font.shadowed = true;
            self.font.blit_string(painter, w * 0.5, 5.0, &line, 2.0, ALIGN_CENTER);
            self.font.blit_string(painter, w * 0.5, 40.0, &translate("BEFORE THE END OF CURRENT LAP"), 2.0, ALIGN_CENTER);
            return;
        }
        let t = input.session_type;
        if (t.wrapping_sub(5) as u32) <= 2 || t == 3 {
            return;
        }
        // (`CarAvatar::isCurrentLapValid` 0x1400d8b50 says yes to everything in a replay)
        let valid = input.in_replay || car.current_lap_valid;
        if !valid && self.last_current_lap_valid {
            self.invalid_lap_message_time = 5.0;
        }
        self.last_current_lap_valid = valid;
        if !(self.invalid_lap_message_time > 0.0) {
            return;
        }
        let w = banner_width(gui);
        message.clear_message();
        backdrop(painter, w);
        self.font.set_color(1.0, 0.0, 0.0, 1.0);
        self.font.shadowed = true;
        self.font.blit_string(painter, w * 0.5, 5.0, &translate("CUT DETECTED"), 2.0, ALIGN_CENTER);
        self.font.blit_string(painter, w * 0.5, 40.0, &translate("LAP WILL NOT COUNT"), 2.0, ALIGN_CENTER);
        self.invalid_lap_message_time = self.invalid_lap_message_time - dt;
    }
}

/// `DriveThroughPenaltyAvatar`: two red lines under the penalty flag.
pub struct DriveThroughPenalty {
    pub font: Font,
    pub text_pos: f32,
}

impl DriveThroughPenalty {
    /// `DriveThroughPenaltyAvatar::DriveThroughPenaltyAvatar` 0x1400907b0. (The game adds the
    /// flag's height and 20 to a member it never set: here that member is 0. With a headset
    /// it also makes a control of its own: see the report's "Not done".)
    pub fn new(painter: &mut dyn Painter) -> DriveThroughPenalty {
        let mut font = Font::new(painter, "", FontType::Proportional, 15.0, false, false);
        font.shadowed = true;
        font.shadow_pixel_distance = 1.0;
        let texture = painter.get_texture("content/gui/flags/penalty.png");
        let height = painter.texture_size(&texture).1;
        font.set_color(1.0, 0.0, 0.0, 1.0);
        DriveThroughPenalty { font, text_pos: (height + 20) as f32 + 0.0 }
    }

    /// `DriveThroughPenaltyAvatar::renderHUD` 0x140090b50.
    pub fn render_hud(&mut self, gui: &Gui, painter: &mut dyn Painter, input: &HudInput, black_flagged: bool) {
        let Some(car) = input.cars.first() else { return };
        if car.pit_penalty_laps <= 0 || input.in_replay || input.pause_menu_visible || input.esc_menu_visible || black_flagged {
            return;
        }
        let x = if input.triple_screen { (gui.screen.0 / 3 + 0xf) as f32 } else { 15.0 };
        self.font.blit_string(painter, x, self.text_pos, &translate("DRIVE THROUGH PENALTY"), 1.0, ALIGN_LEFT);
        let line = format!("{} {}", car.pit_penalty_laps as i32 - 1, translate("LAPS REMAINING"));
        self.font.blit_string(painter, x, self.text_pos + 20.0, &line, 1.0, ALIGN_LEFT);
    }
}

/// `LockControlsTimeAvatar`: "CONTROLS ARE LOCKED FOR n SECONDS".
pub struct LockControlsTime {
    pub font: Font,
    pub show_message: bool,
}

impl LockControlsTime {
    /// `LockControlsTimeAvatar::LockControlsTimeAvatar` 0x14009d8c0; `messages` is
    /// `system/cfg/messages.ini`.
    pub fn new(painter: &mut dyn Painter, messages: Option<&IniReader>) -> LockControlsTime {
        let font = Font::new(painter, "", FontType::Proportional, 15.0, false, false);
        let mut show_message = true;
        if let Some(v) = messages.filter(|ini| ini.ready).and_then(|ini| ini.get_int("PENALTY", "LOCK_CONTROL").ok()) {
            show_message = v == 1;
        }
        LockControlsTime { font, show_message }
    }

    /// `LockControlsTimeAvatar::renderHUD` 0x14009dc50.
    pub fn render_hud(&mut self, gui: &Gui, painter: &mut dyn Painter, input: &HudInput, message: &mut SystemMessage) {
        if !self.show_message {
            return;
        }
        let Some(car) = input.cars.first() else { return };
        if !(0.0 < car.lock_controls_time) {
            return;
        }
        if input.in_replay || input.pause_menu_visible || input.esc_menu_visible {
            return;
        }
        let w = banner_width(gui);
        message.clear_message();
        backdrop(painter, w);
        self.font.set_color(1.0, 0.0, 0.0, 1.0);
        self.font.shadowed = true;
        self.font.blit_string(painter, w * 0.5, 5.0, &translate("PENALTY"), 2.0, ALIGN_CENTER);
        let line = format!("{} {} {}", translate("CONTROLS ARE LOCKED FOR"), (car.lock_controls_time * 0.001) as i32, translate("SECONDS"));
        self.font.blit_string(painter, w * 0.5, 40.0, &line, 2.0, ALIGN_CENTER);
    }
}

/// What `WrongWayIndicator::update` reads of the player's car.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct WrongWayCar {
    /// `CarAvatar::getRoadDirection`
    pub road_direction: [f32; 3],
    /// the third row of `physicsState.worldMatrix`
    pub look: [f32; 3],
    pub normalized_spline_position: f32,
}

/// What an `update` of the wrong-way indicator asks of its owner.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct WrongWayResult {
    /// the car goes back to the pits and "Driving wrong way" is shown (offline, penalties on)
    pub penalty: bool,
    /// `evOnDrivingWrongWay` fired with this many metres (what an online session listens to)
    pub event: Option<f32>,
}

/// `WrongWayIndicator`.
pub struct WrongWayIndicator {
    /// `GameObject::isActive`: off while a replay shows
    pub is_active: bool,
    pub is_wrong_way: bool,
    pub texture: Texture,
    pub meter_counter: f32,
    pub last_position_on_spline: f32,
    /// `race.ini [RACE] PENALTIES`
    pub penalties: bool,
}

impl WrongWayIndicator {
    /// `WrongWayIndicator::WrongWayIndicator` 0x1401d83d0.
    pub fn new(painter: &mut dyn Painter, penalties: bool) -> WrongWayIndicator {
        WrongWayIndicator { is_active: true, is_wrong_way: false, texture: painter.get_texture("content/texture/wrongway.png"), meter_counter: 0.0, last_position_on_spline: 0.0, penalties }
    }

    /// `WrongWayIndicator::update` 0x1401d8940. `spline_to_world` is the AI line's
    /// `InterpolatingSpline::splineToWorld`.
    pub fn update(&mut self, car: &WrongWayCar, offline: bool, spline_to_world: &mut dyn FnMut(f32) -> [f32; 3]) -> WrongWayResult {
        let mut result = WrongWayResult::default();
        let (rd, m) = (car.road_direction, car.look);
        let mut dot = rd[0] * -m[0];
        dot += rd[1] * -m[1];
        dot += rd[2] * -m[2];
        self.is_wrong_way = dot > 0.4;
        if self.is_wrong_way {
            let pos = car.normalized_spline_position;
            if pos < self.last_position_on_spline {
                let a = spline_to_world(pos);
                let b = spline_to_world(self.last_position_on_spline);
                let (dz, dy, dx) = (b[2] - a[2], b[1] - a[1], b[0] - a[0]);
                let mut s = dy * dy;
                s += dx * dx;
                s += dz * dz;
                let d = if s != 0.0 { s.sqrt() } else { 0.0 };
                self.meter_counter = d + self.meter_counter;
                if offline && self.meter_counter > 500.0 && self.penalties {
                    result.penalty = true;
                    self.meter_counter = 0.0;
                }
                result.event = Some(self.meter_counter);
            }
        } else {
            self.meter_counter = 0.0;
        }
        self.last_position_on_spline = car.normalized_spline_position;
        result
    }

    /// `WrongWayIndicator::renderHUD` 0x1401d87a0.
    pub fn render_hud(&mut self, gui: &Gui, painter: &mut dyn Painter, input: &HudInput) {
        if input.camera.map(|c| c.1).unwrap_or(false) || input.camera_mode == 3 {
            return;
        }
        let (tw, th) = painter.texture_size(&self.texture);
        let (w, h) = gui.screen;
        let x = ((w / 2) as i64 - (tw >> 1) as i64) as f32;
        let y = ((h / 4) as i64 - (th >> 1) as i64) as f32;
        if self.is_wrong_way {
            painter.set_texture(0, &self.texture);
            painter.set_blend_mode(BLEND_ALPHA);
            painter.gl_color4f(GRAPHICS_GL, 1.0, 1.0, 1.0, 1.0);
            painter.gl_quad(GRAPHICS_GL, x, y, tw as f32, th as f32, true);
        }
    }
}

/// `FuelLight`: the pump that goes from yellow to red under the car's reserve.
pub struct FuelLight {
    /// `GameObject::isActive`: off while a replay shows
    pub is_active: bool,
    pub fuel_texture: Texture,
    pub out_of_fuel_texture: Texture,
    /// `fuelLimits[0]`: `car.ini [GRAPHICS] FUEL_LIGHT_MIN_LITERS` of the player's car
    pub fuel_limit: f32,
    pub x_pos: f32,
}

impl FuelLight {
    /// `FuelLight::FuelLight` 0x1400bb510; `min_liters` is the key as `getFloat` gives it (0
    /// when it is not there).
    pub fn new(painter: &mut dyn Painter, min_liters: f32) -> FuelLight {
        let fuel_texture = painter.get_texture("content//gui//fuelIndicator//fuelIndicator.png");
        let out_of_fuel_texture = painter.get_texture("content//gui//fuelIndicator//outOfFuel.png");
        let fuel_limit = if min_liters == 0.0 { 3.0 } else { min_liters };
        FuelLight { is_active: true, fuel_texture, out_of_fuel_texture, fuel_limit, x_pos: 25.0 }
    }

    /// The handler on `FlagManager::evOnFlagChange` 0x1400bbad0.
    pub fn on_flag(&mut self, e: &FlagTriggered) {
        if e.is_active && e.flag != FLAG_BLACK {
            self.x_pos = (e.flag_width + e.flag_position[0]) + 15.0;
        } else {
            self.x_pos = 25.0;
        }
    }

    /// `FuelLight::renderHUD` 0x1400bc030.
    pub fn render_hud(&mut self, gui: &Gui, painter: &mut dyn Painter, input: &HudInput) {
        if input.focused_car_index != 0 {
            return;
        }
        let Some(car) = input.cars.first() else { return };
        let fuel = car.fuel;
        if self.fuel_limit < fuel {
            return;
        }
        if input.esc_menu_visible || input.race_over || input.end_session_active {
            return;
        }
        let g = 1.0 - (1.0 - fuel / self.fuel_limit);
        painter.gl_color4f(GRAPHICS_GL, 1.0, g, 0.0, 1.0);
        painter.set_blend_mode(BLEND_ALPHA);
        painter.set_texture(0, if 0.0 < fuel { &self.fuel_texture } else { &self.out_of_fuel_texture });
        let x = if input.triple_screen { (gui.screen.0 / 3) as f32 + self.x_pos } else { self.x_pos };
        painter.gl_quad(GRAPHICS_GL, x, 25.0, 50.0, 50.0, true);
    }
}
