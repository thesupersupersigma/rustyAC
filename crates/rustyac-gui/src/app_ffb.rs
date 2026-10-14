// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! `FFBForm` (0x268 bytes), the app "Force Feedback Controller" of the player's car
//! (`Sim::cars[0]`, never the focused one): the user's gain (`CarAvatar::userFFGain`) with two
//! arrows, and, for a car whose `car.ini` is a loose file, the automatic adjustment of
//! `[CONTROLS] FFMULT` over a lap and "Quick Adjust"; with a wheel (`DICarControl`) the four
//! effects of `controls.ini [FF_ENHANCEMENT]` and their "Save".
//!
//! `FFBForm::FFBForm` 0x140314a10, `render` 0x1403172c0, `saveFFB` 0x140317750, `shutdown`
//! 0x140317a70; the arrows' handlers (`_Do_call` 0x140316f10 / 0x140316f50), "Auto FFB"
//! 0x1403164e0, "Quick Adjust" 0x140316f90, the lap's 0x1403167d0, the spinners' 0x140316ae0
//! / 0x1403166f0 / 0x140316bc0 / 0x140316400, "Save" 0x140316100.
//!
//! What the game queues for the physics thread (`PhysicsAvatar::executeOnPhysicsThread`) the
//! app asks for as [`AppAction`]s: the user's gain, the car's `ffMult`, a gain of the wheel's
//! `ffUpgrades`; the avatar's own copy of the gain changes at once, as in the game.

use std::any::Any;
use std::cell::Cell;
use std::rc::Rc;

use crate::apps::{App, AppAction};
use crate::control::{Clicked, Id};
use crate::font::{Font, FontType};
use crate::gui::Gui;
use crate::hud::translate;
use crate::painter::{Painter, Rect, GRAPHICS_GL};
use crate::widgets::WidgetEvent;
use crate::widgets2::Connected;

/// The app's members its handlers share.
#[derive(Default)]
pub struct FfbState {
    pub is_auto_adjusting_ff: Cell<bool>,
    pub ff_avg: Cell<f32>,
    pub ff_avt_percentage: Cell<f32>,
    pub suggested_ff_mult: Cell<f32>,
    pub is_save_needed: Cell<bool>,
}

pub struct Ffb {
    pub ffb: Id,
    pub ffb_value: Id,
    pub ffb_auto_adjust: Option<Id>,
    pub quick_adjust: Option<Id>,
    pub ffavg: Option<Id>,
    pub optff: Option<Id>,
    pub state: Rc<FfbState>,
    /// `filename`: the car's car.ini (the game folder's)
    pub filename: String,
}

/// `CarAvatar::getUserFFGain` 0x1400d3a90 of the player's car.
fn user_ff_gain(gui: &Gui) -> f32 {
    gui.apps.cars.first().map_or(0.0, |c| c.avatar_ff_gain)
}

/// `CarAvatar::getFFMult` 0x1400d3490 (0.0 without physics).
fn ff_mult(gui: &Gui) -> f32 {
    gui.apps.cars.first().map_or(0.0, |c| c.ff_mult)
}

/// `CarAvatar::setUserFFGain` 0x1400dad50: the avatar's copy at once, the car's on the physics
/// thread.
fn set_user_ff_gain(gui: &mut Gui, v: f32) {
    if let Some(car) = gui.apps.cars.first_mut() {
        if !car.has_physics {
            return;
        }
        car.avatar_ff_gain = v;
        gui.apps.actions.push(AppAction::SetUserFfGain(v));
    }
}

/// `CarAvatar::modifyUserFFGain` 0x1400d90f0.
fn modify_user_ff_gain(gui: &mut Gui, delta: f32) {
    if let Some(car) = gui.apps.cars.first_mut() {
        if !car.has_physics || delta == 0.0 {
            return;
        }
        let v = delta + car.avatar_ff_gain;
        let v = if v > 200.0 {
            200.0
        } else if v >= 0.0 {
            v
        } else {
            0.0
        };
        car.avatar_ff_gain = v;
        gui.apps.actions.push(AppAction::SetUserFfGain(v));
    }
}

/// `CarAvatar::setFFMult` 0x1400da160 (the physics thread's).
fn set_ff_mult(gui: &mut Gui, v: f32) {
    if let Some(car) = gui.apps.cars.first_mut() {
        if !car.has_physics {
            return;
        }
        car.ff_mult = v;
        gui.apps.actions.push(AppAction::SetFfMult(v));
    }
}

/// `FFBForm::saveFFB` 0x140317750: the gain back to 1, the car's car.ini `[CONTROLS] FFMULT`
/// and `STEER_ASSIST` (`%.3f`).
fn save_ffb(gui: &mut Gui, filename: &str) {
    if user_ff_gain(gui) != 1.0 {
        set_user_ff_gain(gui, 1.0);
    }
    let mult = ff_mult(gui) * 1000.0f32;
    gui.apps.actions.push(AppAction::WriteProfileString { file: filename.to_string(), section: "CONTROLS".to_string(), key: "FFMULT".to_string(), value: format!("{:.3}", mult as f64) });
    let assist = gui.apps.cars.first().map_or(0.0, |c| c.steer_assist);
    gui.apps.actions.push(AppAction::WriteProfileString { file: filename.to_string(), section: "CONTROLS".to_string(), key: "STEER_ASSIST".to_string(), value: format!("{:.3}", assist as f64) });
}

impl Gui {
    /// `FFBForm::FFBForm(CarAvatar*)` 0x140314a10 for the player's car. `car_ini_loose`: its
    /// `car.ini` is a file of its own (`Path::fileExists(filename, true)`); `ff_upgrades`: the
    /// wheel's `DICarControl::ffUpgrades` (curbs, road, slips, ABS), none without a wheel.
    pub fn new_ffb_app(&mut self, painter: &mut dyn Painter, car_ini_loose: bool, ff_upgrades: Option<[f32; 4]>) -> Id {
        let form = self.new_form(painter, "FFB_FORM", true);
        let state = Rc::new(FfbState { suggested_ff_mult: Cell::new(1.0), ..FfbState::default() });
        self.form_data_mut(form).dev_app = false;
        let text_left = painter.get_texture("content/gui/controls/spinner/leftMinus.png");
        let text_right = painter.get_texture("content/gui/controls/spinner/rightPlus.png");
        self.form_set_title(form, "Force Feedback Controller");
        self.fonts.push(Font::new(painter, "", FontType::Monospaced, 16.0, false, false));
        let font = self.fonts.len() - 1;
        // the gain's arrows
        let ffb = self.new_custom_spinner(painter, "FFB_SPINNER", 0);
        self.set_position(ffb, 25.0, 75.0);
        self.controls[ffb].fore_color = [0.0; 4];
        self.set_text(ffb, &translate("FFB"));
        self.controls[ffb].font = Some(font);
        self.set_size(ffb, 300.0, 21.0);
        let (left, right) = (self.custom_spinner(ffb).left_button, self.custom_spinner(ffb).right_button);
        self.controls[left].back_texture = text_left;
        self.controls[right].back_texture = text_right;
        // Game::gameTime.deltaT times -0.1 / +0.1
        self.controls[left].ev_clicked.push(Rc::new(|gui: &mut Gui, _: &Clicked| {
            let d = gui.apps.game_dt;
            modify_user_ff_gain(gui, d * f32::from_bits(0xbdcc_cccd));
        }));
        self.controls[right].ev_clicked.push(Rc::new(|gui: &mut Gui, _: &Clicked| {
            let d = gui.apps.game_dt;
            modify_user_ff_gain(gui, d * f32::from_bits(0x3dcc_cccd));
        }));
        self.add_control(form, ffb);
        let short_name = self.apps.cars.first().map(|c| c.gui_short_name.clone()).unwrap_or_default();
        let ffb_value = self.new_label(painter, "FFB_VALUE");
        self.controls[ffb_value].font = Some(font);
        self.set_text(ffb_value, &short_name);
        self.set_size(ffb_value, 300.0, 21.0);
        self.set_position(ffb_value, 250.0, 40.0);
        self.add_control(form, ffb_value);
        let name = self.new_label(painter, "NAME");
        self.controls[name].font = Some(font);
        self.set_text(name, &short_name);
        self.set_size(name, 300.0, 21.0);
        self.set_position(name, 25.0, 40.0);
        self.add_control(form, name);
        // the car's car.ini, relative to the game's folder
        let car = self.apps.cars.first();
        let unix = car.map(|c| c.unix_name.clone()).unwrap_or_default();
        let config = car.map(|c| c.config_name.clone()).unwrap_or_default();
        let filename = if config.is_empty() { format!("content/cars/{unix}/data/car.ini") } else { format!("content/cars/{unix}/data_{config}/car.ini") };
        let (mut ffb_auto_adjust, mut quick_adjust, mut ffavg, mut optff) = (None, None, None, None);
        let y;
        if car_ini_loose {
            let auto = self.new_control(painter, "FFB_RESET");
            self.controls[auto].font_scale = f32::from_bits(0x3f33_3333);
            self.set_text(auto, "Auto FFB OFF");
            self.set_size(auto, 170.0, 21.0);
            self.set_repeat_interval(auto, 0.0);
            self.set_position(auto, 155.0, 115.0);
            {
                let (state, filename) = (state.clone(), filename.clone());
                self.controls[auto].ev_clicked.push(Rc::new(move |gui: &mut Gui, _: &Clicked| {
                    state.is_auto_adjusting_ff.set(!state.is_auto_adjusting_ff.get());
                    if !state.is_auto_adjusting_ff.get() {
                        gui.controls[auto].back_color = gui.get_skin_color("CONTROL", "BACK");
                        gui.set_text(auto, "Auto FFB OFF");
                        save_ffb(gui, &filename);
                    } else {
                        state.is_save_needed.set(true);
                        if user_ff_gain(gui) != 1.0 {
                            set_user_ff_gain(gui, 1.0);
                        }
                        gui.controls[auto].back_color = [230.0, 0.0, 0.0, 255.0];
                        state.ff_avg.set(0.0);
                        state.ff_avt_percentage.set(0.0);
                        state.suggested_ff_mult.set(1.0);
                        gui.set_text(auto, "Auto FFB ON");
                    }
                }));
            }
            self.add_control(form, auto);
            let quick = self.new_control(painter, "FFB_QUICK");
            self.controls[quick].font_scale = f32::from_bits(0x3f33_3333);
            self.set_text(quick, "Quick Adjust");
            self.set_size(quick, 170.0, 21.0);
            self.set_repeat_interval(quick, 0.0);
            self.set_position(quick, 155.0, 145.0);
            {
                let state = state.clone();
                self.controls[quick].ev_clicked.push(Rc::new(move |gui: &mut Gui, _: &Clicked| {
                    let fm = ff_mult(gui);
                    let g = user_ff_gain(gui);
                    set_ff_mult(gui, g * fm);
                    set_user_ff_gain(gui, 1.0);
                    state.is_save_needed.set(true);
                }));
            }
            self.add_control(form, quick);
            let avg = self.new_connected_label(painter, "FFAVG", Connected::Float(0.0));
            self.set_position(avg, 25.0, 115.0);
            self.set_size(avg, 50.0, 25.0);
            self.add_control(form, avg);
            let opt = self.new_connected_label(painter, "OPT_FF", Connected::Float(1.0));
            self.set_position(opt, 25.0, 145.0);
            self.set_size(opt, 50.0, 25.0);
            self.add_control(form, opt);
            self.set_size(form, 350.0, 430.0);
            (ffb_auto_adjust, quick_adjust, ffavg, optff) = (Some(auto), Some(quick), Some(avg), Some(opt));
            y = 200.0f32;
        } else {
            self.set_size(form, 350.0, 350.0);
            y = 120.0f32;
        }
        if let Some(upgrades) = ff_upgrades {
            // (name, label, x, y, height, the gain's index in ffUpgrades, its value)
            let rows = [("SP_SLIPS", "Slips", 25.0, y, 20.0, 2usize, upgrades[2]), ("SP_ROAD", "Road", 20.0, y + 60.0, 25.0, 1, upgrades[1]), ("SP_CURB", "Curbs", 20.0, y + 120.0, 25.0, 0, upgrades[0]), ("SP_ABS", "ABS", 20.0, y + 180.0, 25.0, 3, upgrades[3])];
            for (name, label, x, sy, h, which, value) in rows {
                let sp = self.new_spinner(painter, name, true, false);
                self.spinner_set_label_text(sp, &translate(label));
                let s = self.spinner_mut(sp);
                s.min_value = 0;
                s.max_value = 100;
                s.step = 1;
                self.spinner_set_position(sp, x, sy);
                self.set_size(sp, 300.0, h);
                // Spinner::setValue before the handler is hooked: nobody hears of it
                let s = self.spinner_mut(sp);
                s.value = crate::app_essentials::cut_f(value * 100.0f32).clamp(s.min_value, s.max_value);
                self.spinner_mut(sp).handlers.push(Rc::new(move |gui: &mut Gui, ev: &WidgetEvent| {
                    let WidgetEvent::SpinnerValueChanged { value, .. } = ev else { return };
                    let v = *value as f32 * f32::from_bits(0x3c23_d70a);
                    gui.apps.actions.push(AppAction::SetFfUpgrade { which, value: v });
                }));
                self.add_control(form, sp);
            }
            let save = self.new_control(painter, "SAVE_FF");
            self.set_position(save, 20.0, y + 240.0);
            self.set_size(save, 300.0, 25.0);
            self.set_text(save, &translate("Save"));
            // the four gains as the wheel has them now (the spinners' jobs are done by then)
            self.controls[save].ev_clicked.push(Rc::new(|gui: &mut Gui, _: &Clicked| {
                let gains = gui.apps.ff_upgrades.unwrap_or_default();
                for (key, i) in [("SLIPS", 2), ("CURBS", 0), ("ROAD", 1), ("ABS", 3)] {
                    gui.apps.actions.push(AppAction::WriteProfileString { file: "cfg/controls.ini".to_string(), section: "FF_ENHANCEMENT".to_string(), key: key.to_string(), value: crate::app_volumes::to_wstring_f(gains[i]) });
                }
                gui.apps.actions.push(AppAction::AddMessage { title: translate("FF settings saved to controls.ini"), text: String::new() });
            }));
            self.add_control(form, save);
        }
        let m = self.controls[form].scale_mult;
        self.scale_by_mult(form, m);
        self.set_app(form, Box::new(Ffb { ffb, ffb_value, ffb_auto_adjust, quick_adjust, ffavg, optff, state, filename }));
        form
    }
}

impl App for Ffb {
    fn as_any(&mut self) -> &mut dyn Any {
        self
    }

    /// `FFBForm::render` 0x1403172c0.
    fn render(&mut self, gui: &mut Gui, id: Id, dt: f32, painter: &mut dyn Painter) {
        let n = crate::app_essentials::cut_f(user_ff_gain(gui) * 100.0f32);
        gui.set_text(self.ffb_value, &format!("{} ({n})", translate("FFB ")));
        gui.controls[id].font = Some(gui.font);
        painter.gl_color4f(GRAPHICS_GL, 1.0, 1.0, 1.0, 1.0);
        let left = gui.custom_spinner(self.ffb).left_button;
        let r0 = gui.controls[left].rect;
        let mut r = Rect { left: r0.left, right: r0.right, top: r0.top, bottom: r0.bottom };
        gui.get_world_rect(left, &mut r);
        let s = gui.controls[id].scale_mult;
        let w = (s * 300.0 - s * 42.0) - s * 2.0;
        painter.gl_quad(GRAPHICS_GL, s + r.right, r.top, w, s * 21.0, false);
        let st = &self.state;
        if st.is_auto_adjusting_ff.get() {
            let t = if dt > 1.0 {
                1.0
            } else if dt >= 0.0 {
                dt
            } else {
                0.0
            };
            let pure = gui.apps.cars.first().map_or(0.0, |c| c.state.last_ff_pure);
            st.ff_avg.set((pure - st.ff_avg.get()) * t + st.ff_avg.get());
            if st.ff_avg.get().abs() * 100.0f32 > st.ff_avt_percentage.get() {
                st.ff_avt_percentage.set((st.ff_avg.get() * 100.0f32).abs());
                let m = ff_mult(gui);
                st.suggested_ff_mult.set(((f32::from_bits(0x3f66_6666) / st.ff_avg.get().abs()) * m) * 1000.0f32);
            }
        }
        if let (Some(avg), Some(opt)) = (self.ffavg, self.optff) {
            gui.connected_label_mut(avg).value = Connected::Float(st.ff_avt_percentage.get());
            gui.connected_label_mut(opt).value = Connected::Float(st.suggested_ff_mult.get());
        }
        gui.control_render(id, dt, painter);
    }

    /// The lap handler 0x1403167d0 (hooked only with a loose car.ini).
    fn on_lap_completed(&mut self, gui: &mut Gui, id: Id, car: u32, _time: u32) {
        let st = &self.state;
        if self.ffb_auto_adjust.is_none() || car != 0 || !gui.controls[id].visible || !st.is_auto_adjusting_ff.get() {
            return;
        }
        let old = ff_mult(gui);
        let old_k = old * 1000.0f32;
        set_ff_mult(gui, st.suggested_ff_mult.get() * f32::from_bits(0x3a83_126f));
        let text = format!("ffMult auto adjusted to:{:.3} {:.1}%", st.suggested_ff_mult.get() as f64, ((st.suggested_ff_mult.get() / old_k) * 100.0f32) as f64);
        gui.apps.actions.push(AppAction::AddMessage { title: text, text: String::new() });
        st.ff_avt_percentage.set(0.0);
        st.ff_avg.set(0.0);
    }

    /// `FFBForm::shutdown` 0x140317a70.
    fn shutdown(&mut self, gui: &mut Gui, _id: Id) {
        if self.state.is_save_needed.get() {
            save_ffb(gui, &self.filename);
        }
    }
}
