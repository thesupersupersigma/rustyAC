// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! `ReplayInterface` (0x330 bytes): the bar along the bottom of the screen while a replay is
//! watched — the time line with its two cut markers and the laps' marks, the transport
//! buttons, the spinners of the slow motion's level, the camera and the car, the box that
//! asks for a name to save the replay under.
//!
//! `ReplayInterface::ReplayInterface` 0x140148300, `render` 0x14014d970,
//! `updateUIElementPosition` 0x14014f870, `onMouseMove` 0x14014d3d0, `onVisibleChanged`
//! 0x14014d6e0, `onSliderSelection` 0x14014d630, `onCutExtremesChanged` 0x14014d350,
//! `didFinishedSavingReplay` 0x14014d110, `resetCuts` 0x14014f860, and its handlers: the
//! buttons (rewind 0x14014c120, fast forward 0x14014c3e0, play 0x14014c2e0, slow motion
//! 0x14014c180, stop 0x14014c450, next frame 0x14014c270, exit 0x14014c0e0, save
//! 0x14014b990, the box's "Ok" 0x14014c360 and its validation 0x14014c200), the spinners
//! (camera 0x14014b370, car 0x14014c520, slow motion 0x14014c380) and the camera manager's
//! "another camera was picked" (0x14014c4b0).
//!
//! The interface reads the replay manager and the camera manager through
//! [`crate::menus::SimLink::replay`], and what it asks of them is left in the list of
//! [`SimAction`]s like the other menus' calls.
//!
//! Not ported: the photo mode. Its controls are made (they are children of the bar, hidden
//! every frame outside the photo mode) and its button is drawn and says what the game's says
//! with post-processing off; with it on a click does nothing, where the game goes into the
//! photo mode, and so
//! `updateUIElementPositionPhotoMode` 0x14014ffe0, the photo spinners' handlers, the view
//! finder and the mouse wheel's zoom are not here.

use std::rc::Rc;

use crate::control::{Clicked, Id, Kind, MouseEvent};
use crate::esc_menu::time_to_wstring;
use crate::font::{Font, FontType};
use crate::gui::Gui;
use crate::hud::translate;
use crate::menus::{Menu, SimAction};
use crate::painter::{Painter, Texture, ALIGN_CENTER, ALIGN_LEFT, ALIGN_RIGHT, GRAPHICS_GL};
use crate::widgets::WidgetEvent;

/// `eReplayStatus`.
pub const REPLAY_PLAY: i32 = 0;
pub const REPLAY_REWIND: i32 = 3;
pub const REPLAY_FAST_FORWARD: i32 = 4;
pub const REPLAY_SLOW_MOTION: i32 = 5;
pub const REPLAY_PHOTO_MODE: i32 = 11;

/// What the interface asks of `ReplayManager`.
#[derive(Clone, Debug, PartialEq)]
pub enum ReplayAction {
    /// `rewind(1, false)` 0x140159b40, `fastForward(1, false)` 0x140156780
    Rewind,
    FastForward,
    /// `play` 0x1401591f0, `pause` 0x140159070, `stop` 0x14015b6f0, `nextFrame` 0x140158900
    Play,
    Pause,
    Stop,
    NextFrame,
    /// `slowMotion(level)` 0x14015b3b0
    SlowMotion(f32),
    /// `stopReplayMode` 0x14015b800
    StopReplayMode,
    /// `setNormalizedPosition` 0x14015b0b0
    SetNormalizedPosition(f32),
    /// `Sim::evOnReplayStatusChanged` with the status 10 (a click on the time line), the
    /// manager's speed and slow-motion level
    SliderInteraction,
    /// `setCutInPerc` 0x14015af30, `setCutOutPerc` 0x14015aff0
    SetCutInPerc(f32),
    SetCutOutPerc(f32),
    /// `save(name, MAIN_DIR)` 0x140159d10; when it is done the runner calls
    /// [`Gui::replay_did_finished_saving`]
    Save(String),
}

/// What the interface reads of `ReplayManager` and `ACCameraManager`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ReplayLink {
    /// `status`, `timeMult`, `slowMoLevel`
    pub status: i32,
    pub time_mult: f32,
    pub slow_mo_level: f32,
    /// `currentFrame`, `recordedFrames`, `cutIn`, `cutOut`
    pub current_frame: i32,
    pub recorded_frames: i32,
    pub cut_in: i32,
    pub cut_out: i32,
    pub has_loaded_replay: bool,
    /// `getNormalizedLapChange` of the focused car
    pub lap_marks: Vec<f32>,
    /// `ReplayRecorder::recordIntervalMS`
    pub record_interval_ms: f64,
    /// `getAutoFileName`
    pub auto_file_name: String,
    /// the game object `ACCameraManager` is there; the names of the track's camera sets
    /// (`CameraTrack::getCameraSetName`); `currentGlobalCameraIndex`
    pub camera_manager: bool,
    pub track_camera_sets: Vec<String>,
    pub camera_index: i32,
    /// `cfg/video.ini [POST_PROCESS] ENABLED` as the photo button reads it (none: no file,
    /// or no such section)
    pub post_processing: Option<bool>,
}

/// `ReplayInterface`'s own members.
pub struct ReplayInterface {
    pub force_camera: bool,
    pub target_y: f32,
    pub stay_up_counter: f32,
    pub total_width: f32,
    pub replay_height: f32,
    /// `cm` was found
    pub camera_manager: bool,
    pub but_confirm_save: Id,
    pub but_rewind: Id,
    pub but_fast_forward: Id,
    pub but_play: Id,
    pub but_slow_motion: Id,
    pub but_save_edit: Id,
    pub but_exit: Id,
    pub but_stop: Id,
    pub last_selected: Option<Id>,
    pub but_photo_mode: Id,
    pub but_next_frame: Id,
    pub lap_indicator_texture: Texture,
    pub camera_names: Vec<String>,
    pub pos_slider: Id,
    pub slow_mo_spinner: Id,
    pub camera_spinner: Id,
    pub car_spinner: Id,
    pub text_input: Id,
    pub prev_camera_mode: i32,
    pub prev_status: i32,
    pub view_finder: Texture,
    /// the photo mode's: exposure, field of view, the depth of field's factor and focus,
    /// the sun's azimuth and zenith, the camera's roll, "take picture"
    pub photo_controls: [Id; 8],
}

/// `cvttss2si`.
fn cut(x: f32) -> i32 {
    if x > -2147483904.0 && x < 2147483648.0 {
        x as i32
    } else {
        i32::MIN
    }
}

/// The compiler's `floorf` of the layout code: the whole number at or under `x` (a number
/// too big for 32 bits is kept).
fn floor(x: f32) -> f32 {
    let mut t = cut(x);
    if t == i32::MIN {
        return x;
    }
    if t as f32 != x {
        t -= x.is_sign_negative() as i32;
    }
    t as f32
}

fn clamp01(p: f32) -> f32 {
    if p > 1.0 {
        1.0
    } else if p >= 0.0 {
        p
    } else {
        0.0
    }
}

impl Gui {
    pub fn replay_interface(&self, id: Id) -> &ReplayInterface {
        match &self.controls[id].kind {
            Kind::Menu(menu) => match &**menu {
                Menu::Replay(r) => r,
                _ => panic!("control {id} is not the replay interface"),
            },
            _ => panic!("control {id} is not the replay interface"),
        }
    }

    pub fn replay_interface_mut(&mut self, id: Id) -> &mut ReplayInterface {
        match &mut self.controls[id].kind {
            Kind::Menu(menu) => match &mut **menu {
                Menu::Replay(r) => r,
                _ => panic!("control {id} is not the replay interface"),
            },
            _ => panic!("control {id} is not the replay interface"),
        }
    }

    /// A button of the bar: 48 high, its picture, its words under it.
    #[allow(clippy::too_many_arguments)]
    fn replay_button(&mut self, painter: &mut dyn Painter, bar: Id, name: &str, text: Option<&str>, width: f32, texture: &str, texture_first: bool) -> Id {
        let button = self.new_active_button(painter, name);
        if let Some(text) = text {
            self.set_text(button, text);
        }
        if texture_first {
            self.controls[button].back_texture = painter.get_texture(texture);
        }
        self.active_button_mut(button).text_coord = [0.0, 48.0];
        self.set_size(button, width, 48.0);
        if !texture_first {
            self.controls[button].back_texture = painter.get_texture(texture);
        }
        self.add_control(bar, button);
        button
    }

    fn replay_click(&mut self, button: Id, handler: impl Fn(&mut Gui) + 'static) {
        self.controls[button].ev_clicked.push(Rc::new(move |gui: &mut Gui, _ev: &Clicked| handler(gui)));
    }

    /// `ReplayInterface::ReplayInterface` 0x140148300. The slow motion's level and the number
    /// of cars are read from [`Gui::sim`].
    pub fn new_replay_interface(&mut self, painter: &mut dyn Painter) -> Id {
        let id = self.new_control(painter, "REPLAY_INTERFACE");
        let bar = id;
        let debug_label = self.new_label(painter, "MYLABEL");
        self.set_position(debug_label, 0.0, -50.0);
        self.set_visible(debug_label, false);
        self.add_control(bar, debug_label);
        let lap_indicator_texture = painter.get_texture("content\\gui\\replay\\lapIndicator.png");
        self.fonts.push(Font::new(painter, "", FontType::Proportional, 12.0, false, false));
        self.controls[id].font = Some(self.fonts.len() - 1);
        let (width, height) = painter.screen();
        let target_y = height as f32 - 105.0;
        let m = width as f32 * f32::from_bits(0x3c23_d70a) * 13.5;
        self.set_position(id, m, target_y);
        let m2 = m * 2.0;
        self.control_set_size(id, width as f32 - m2, 105.0);
        self.controls[id].back_color = [0.0, 0.0, 0.0, 0.5];
        self.controls[id].border_color = [0.25, 0.25, 0.25, 0.5];
        let replay_height = width as f32 - m2;

        // the box that asks for the replay's name, with its "Ok"
        let text_input = self.new_text_input(painter, "REPLAY_SAVE_NAME_TEXT_INPUT");
        self.set_visible(text_input, false);
        self.controls[text_input].back_color = [0.0, 0.0, 0.0, 0.5];
        self.controls[text_input].border_color = [0.25, 0.25, 0.25, 0.5];
        self.text_input_mut(text_input).back_color_unfocus = [0.25, 0.25, 0.25, 0.5];
        self.text_input_mut(text_input).back_color_focus = [0.0, 0.0, 0.0, 0.5];
        self.add_control(bar, text_input);
        let but_confirm_save = self.new_control(painter, "Confirm Save");
        self.set_size(but_confirm_save, 50.0, 25.0);
        self.set_text(but_confirm_save, "Ok");
        self.add_control(text_input, but_confirm_save);
        // (0x14014c360)
        self.replay_click(but_confirm_save, move |gui| gui.text_input_validate_text(text_input));

        let pos_slider = self.new_slider(painter, "POSITION_SLIDER", true);
        self.add_control(bar, pos_slider);
        self.slider_mut(pos_slider).handlers.push(Rc::new(move |gui: &mut Gui, ev: &WidgetEvent| match ev {
            WidgetEvent::SliderInteraction { value, .. } => gui.replay_on_slider_selection(*value),
            WidgetEvent::CutExtremesChanged { cut_in, cut_out, .. } => gui.replay_on_cut_extremes_changed(bar, *cut_in, *cut_out),
            _ => {}
        }));

        let but_save_edit = self.replay_button(painter, bar, "BUT_SAVE_EDIT", Some(&translate("SAVE REPLAY")), 64.0, "content/gui/replay/save.png", false);
        let but_rewind = self.replay_button(painter, bar, "BUT_REWIND", Some(&translate("REWIND")), 64.0, "content/gui/replay/rew.png", false);
        let but_stop = self.replay_button(painter, bar, "BUT_STOP", Some(&translate("STOP")), 64.0, "content/gui/replay/stop.png", false);
        let but_play = self.replay_button(painter, bar, "BUT_PLAY", Some(&translate("PLAY")), 64.0, "content/gui/replay/playPause.png", false);
        let but_slow_motion = self.replay_button(painter, bar, "BUT_SLOW", Some(&translate("SLOW")), 27.0, "content/gui/replay/slomo.png", false);
        let but_fast_forward = self.replay_button(painter, bar, "BUT_FAST_FORWARD", Some(&translate("FAST FORWARD")), 64.0, "content/gui/replay/ffd.png", false);
        let but_exit = self.replay_button(painter, bar, "BUT_EXIT", Some(&translate("EXIT")), 64.0, "content/gui/replay/exit.png", false);

        let slow_mo_spinner = self.new_spinner(painter, "SlowMotion Level", true, false);
        self.set_size(slow_mo_spinner, 80.0, 23.0);
        self.spinner_mut(slow_mo_spinner).min_value = 2;
        self.spinner_mut(slow_mo_spinner).max_value = 12;
        self.spinner_set_value(slow_mo_spinner, cut(self.sim.replay.slow_mo_level));
        let s = self.spinner_mut(slow_mo_spinner);
        s.display_central_value = false;
        s.draw_underline = false;
        s.display_label = false;
        self.add_control(bar, slow_mo_spinner);

        let camera_spinner = self.new_spinner(painter, "Camera", true, false);
        self.set_size(camera_spinner, 80.0, 23.0);
        self.spinner_mut(camera_spinner).min_value = 1;
        self.spinner_mut(camera_spinner).max_value = 4;
        self.spinner_set_value(camera_spinner, 1);
        self.spinner_mut(camera_spinner).draw_underline = false;
        self.spinner_mut(camera_spinner).display_label = false;
        self.spinner_set_draw_arrows(painter, camera_spinner, true);
        self.add_control(bar, camera_spinner);

        let car_spinner = self.new_spinner(painter, "Car", true, false);
        self.set_size(car_spinner, 80.0, 23.0);
        self.spinner_mut(car_spinner).min_value = 1;
        self.spinner_mut(car_spinner).max_value = self.sim.race.cars.len() as i32;
        self.spinner_set_value(car_spinner, 1);
        self.spinner_mut(car_spinner).draw_underline = false;
        self.spinner_mut(car_spinner).display_label = false;
        self.add_control(bar, car_spinner);

        let but_photo_mode = self.replay_button(painter, bar, "PhotoMode", Some(&translate("PHOTO MODE")), 64.0, "content/gui/photoMode/icn_photomode.png", false);
        let but_next_frame = self.replay_button(painter, bar, "NextFrame", None, 64.0, "content/gui/replay/1_ffd.png", true);

        // the handlers, in the game's order
        let handler = |f: fn(&mut Gui, Id, i32)| -> crate::widgets::WidgetHandler {
            Rc::new(move |gui: &mut Gui, ev: &WidgetEvent| {
                if let WidgetEvent::SpinnerValueChanged { value, .. } = ev {
                    f(gui, bar, *value);
                }
            })
        };
        self.spinner_mut(camera_spinner).handlers.push(handler(Gui::replay_on_camera_spinner));
        // the car (0x14014c520)
        self.spinner_mut(car_spinner).handlers.push(handler(|gui, _bar, value| {
            let car = (value - 1) as u32;
            if (car as usize) < gui.sim.race.cars.len() {
                gui.sim.actions.push(SimAction::SetFocusedCarIndex(car));
            }
        }));
        // the slow motion's level (0x14014c380): always into slow motion
        self.spinner_mut(slow_mo_spinner).handlers.push(handler(|gui, _bar, value| gui.replay_ask(ReplayAction::SlowMotion(value as f32))));
        // the box's name is given (0x14014c200)
        self.text_input_mut(text_input).handlers.push(Rc::new(move |gui: &mut Gui, ev: &WidgetEvent| {
            if let WidgetEvent::Text { text, kind: 0, .. } = ev {
                gui.replay_ask(ReplayAction::Save(text.clone()));
                gui.set_visible(text_input, false);
            }
        }));
        // "Save replay" (0x14014b990): the box opens over the bar, or closes
        self.replay_click(but_save_edit, move |gui| {
            let r = gui.replay_interface(bar);
            let (input, save, confirm) = (r.text_input, r.but_save_edit, r.but_confirm_save);
            if !gui.controls[input].visible {
                gui.controls[save].draw_background = true;
                let name = gui.sim.replay.auto_file_name.clone();
                gui.set_text(input, &name);
                gui.set_position(input, 0.0, -50.0);
                let b = gui.controls[bar].rect;
                gui.set_size(input, b.right - b.left, 25.0);
                let (i, c) = (gui.controls[input].rect, gui.controls[confirm].rect);
                gui.set_position(confirm, (i.right - i.left) - (c.right - c.left), 0.0);
                gui.set_visible(input, true);
            } else {
                gui.controls[save].draw_background = false;
                gui.set_visible(input, false);
            }
        });
        // (every transport handler begins by bringing the apps back out of the photo mode)
        self.replay_click(but_slow_motion, move |gui| {
            if gui.sim.replay.status != REPLAY_SLOW_MOTION {
                let level = gui.spinner(gui.replay_interface(bar).slow_mo_spinner).value;
                gui.replay_ask(ReplayAction::SlowMotion(level as f32));
            } else {
                gui.replay_ask(ReplayAction::Play);
            }
        });
        self.replay_click(but_rewind, |gui| {
            // (not `timeMult <= -6`: a speed that is no number rewinds too)
            if !(gui.sim.replay.time_mult <= -6.0) {
                gui.replay_ask(ReplayAction::Rewind);
            }
        });
        self.replay_click(but_fast_forward, |gui| {
            if gui.sim.replay.time_mult < 6.0 {
                gui.replay_ask(ReplayAction::FastForward);
            }
        });
        self.replay_click(but_play, |gui| {
            let status = gui.sim.replay.status;
            gui.replay_ask(if status != REPLAY_PLAY && status != REPLAY_SLOW_MOTION { ReplayAction::Play } else { ReplayAction::Pause });
        });
        self.replay_click(but_exit, |gui| {
            if gui.sim.replay.has_loaded_replay {
                gui.sim.actions.push(SimAction::Shutdown);
            } else {
                gui.replay_ask(ReplayAction::StopReplayMode);
            }
        });
        self.replay_click(but_stop, |gui| gui.replay_ask(ReplayAction::Stop));
        self.replay_click(but_next_frame, |gui| gui.replay_ask(ReplayAction::NextFrame));
        // "Photo mode" (0x14014aeb0). The game reads its video.ini anew at every press: none,
        // or none with a [POST_PROCESS], and nothing happens; post-processing off, and it
        // says so; on, and it would go into the photo mode, which is not ported
        self.replay_click(but_photo_mode, |gui| {
            if gui.sim.replay.post_processing == Some(false) {
                gui.sim.actions.push(SimAction::AddMessage { title: translate("Error cannot enter Photo Mode"), text: translate("Please activate Post Processing Effects to enter Photo Mode") });
            }
        });

        let w = |gui: &Gui, c: Id| gui.controls[c].rect.right - gui.controls[c].rect.left;
        let a = ((((((w(self, but_exit) + w(self, but_next_frame)) + w(self, but_save_edit)) + w(self, but_rewind)) + w(self, but_stop)) + w(self, but_play)) + w(self, but_fast_forward)) + w(self, but_photo_mode);
        let a = a + 40.0;
        let total_width = (((w(self, slow_mo_spinner) + a) + w(self, camera_spinner)) + w(self, car_spinner)) + 60.0;

        let view_finder = painter.get_texture("content//gui//photoMode//viewFinder.png");
        // the photo mode's controls
        let mut photo_spinner = |gui: &mut Gui, name: &str, min: i32, max: i32, step: i32, underline: bool, width: f32| {
            let s = gui.new_spinner(painter, name, true, false);
            let v = gui.spinner_mut(s);
            v.min_value = min;
            v.max_value = max;
            v.step = step;
            v.display_label = false;
            if !underline {
                v.draw_underline = false;
            }
            gui.set_size(s, width, 23.0);
            gui.add_control(bar, s);
            s
        };
        let spi_expo = photo_spinner(self, "EXPO", 5, 1500, 5, true, 100.0);
        let spi_fov = photo_spinner(self, "FOV", 1, 180, 1, false, 80.0);
        let spi_dof_factor = photo_spinner(self, "DOFFACTOR", 5, 1000, 5, true, 100.0);
        let spi_dof_focus = photo_spinner(self, "DOFFOCUS", 1, 10000, 1, true, 100.0);
        let mut custom = |gui: &mut Gui, name: &str, left: &str, right: &str| {
            let s = gui.new_custom_spinner(painter, name, 0);
            let (l, r) = (gui.custom_spinner(s).left_button, gui.custom_spinner(s).right_button);
            gui.controls[l].back_texture = painter.get_texture(&format!("content/gui/controls/spinner/{left}.png"));
            gui.controls[r].back_texture = painter.get_texture(&format!("content/gui/controls/spinner/{right}.png"));
            gui.set_size(s, 60.0, 23.0);
            gui.add_control(bar, s);
            s
        };
        let spi_camera_roll = custom(self, "ROLL", "leftContainerArrow", "rightContainerArrow");
        let spi_sun_azimuth = custom(self, "AZIMUTH", "leftContainerArrow", "rightContainerArrow");
        let spi_sun_zenith = custom(self, "ZENITH", "leftMinus", "rightPlus");
        let take = format!("{}(F8)", translate("TAKE PICTURE "));
        let but_take_pic = self.replay_button(painter, bar, "TakePic", Some(&take), 64.0, "content/gui/photoMode/icn_screenshot.png", false);

        self.controls[id].kind = Kind::Menu(Box::new(Menu::Replay(Box::new(ReplayInterface {
            force_camera: false,
            target_y,
            stay_up_counter: 3.0,
            total_width,
            replay_height,
            camera_manager: false,
            but_confirm_save,
            but_rewind,
            but_fast_forward,
            but_play,
            but_slow_motion,
            but_save_edit,
            but_exit,
            but_stop,
            last_selected: None,
            but_photo_mode,
            but_next_frame,
            lap_indicator_texture,
            camera_names: Vec::new(),
            pos_slider,
            slow_mo_spinner,
            camera_spinner,
            car_spinner,
            text_input,
            prev_camera_mode: 0,
            prev_status: 2,
            view_finder,
            photo_controls: [but_take_pic, spi_expo, spi_fov, spi_dof_factor, spi_dof_focus, spi_sun_azimuth, spi_sun_zenith, spi_camera_roll],
        }))));
        id
    }

    fn replay_ask(&mut self, action: ReplayAction) {
        self.sim.actions.push(SimAction::Replay(action));
    }

    /// `TextInput::validateText` 0x140245820.
    pub fn text_input_validate_text(&mut self, id: Id) {
        let input = self.text_input_mut(id);
        input.has_focus = false;
        let handlers = input.handlers.clone();
        let text = self.controls[id].text.clone();
        self.fire_widget(handlers, WidgetEvent::Text { control: id, text, kind: 0 });
    }

    /// The camera spinner was turned (0x14014b370): cockpit, one of the five cameras that
    /// drive with the car, or one of the track's sets.
    fn replay_on_camera_spinner(&mut self, bar: Id, value: i32) {
        if !self.replay_interface(bar).camera_manager || self.sim.replay.status == REPLAY_PHOTO_MODE {
            return;
        }
        if self.replay_interface(bar).force_camera {
            self.replay_interface_mut(bar).force_camera = false;
            return;
        }
        let mode = if value < 2 {
            0
        } else if value < 7 {
            self.sim.actions.push(SimAction::SetDrivableCamera(value - 2));
            2
        } else {
            self.sim.actions.push(SimAction::SetTrackCameraSet(value - 7));
            3
        };
        self.sim.actions.push(SimAction::SetCameraModeKept(mode));
        self.replay_interface_mut(bar).prev_camera_mode = mode;
    }

    /// `ACCameraManager::evOnCameraSelectedIndex` as the interface hears it (0x14014c4b0):
    /// the spinner follows a camera picked some other way.
    pub fn replay_on_camera_selected_index(&mut self, bar: Id, index: i32) {
        let r = self.replay_interface(bar);
        if r.camera_manager && self.spinner(r.camera_spinner).value != index + 1 {
            let spinner = r.camera_spinner;
            self.replay_interface_mut(bar).force_camera = true;
            self.spinner_set_value(spinner, self.sim.replay.camera_index + 1);
        }
    }

    /// `ReplayInterface::onSliderSelection` 0x14014d630: a click on the time line.
    fn replay_on_slider_selection(&mut self, value: f32) {
        self.replay_ask(ReplayAction::SetNormalizedPosition(value));
        self.replay_ask(ReplayAction::SliderInteraction);
    }

    /// `ReplayInterface::onCutExtremesChanged` 0x14014d350: a cut marker was let go; the
    /// markers go to the frames the manager cut at (`setCutInPerc` 0x14015af30,
    /// `setCutOutPerc` 0x14015aff0: their answers are worked out here).
    fn replay_on_cut_extremes_changed(&mut self, bar: Id, cut_in: f32, cut_out: f32) {
        let recorded = self.sim.replay.recorded_frames;
        let answer = |p: f32| {
            let p = if p > 1.0 {
                1.0
            } else if p >= 0.0 {
                p
            } else {
                0.0
            };
            let frame = cut((recorded - 1) as f32 * p);
            clamp01(frame as f32 / (recorded - 2) as f32)
        };
        self.replay_ask(ReplayAction::SetCutInPerc(cut_in));
        self.replay_ask(ReplayAction::SetCutOutPerc(cut_out));
        let slider = self.replay_interface(bar).pos_slider;
        self.slider_set_marker_left_position(slider, answer(cut_in));
        self.slider_set_marker_right_position(slider, answer(cut_out));
    }

    /// `ReplayInterface::didFinishedSavingReplay` 0x14014d110 (`file`: the manager's
    /// `lastFilename`).
    pub fn replay_did_finished_saving(&mut self, bar: Id, ok: bool, file: &str) {
        let save = self.replay_interface(bar).but_save_edit;
        self.controls[save].draw_background = false;
        if ok {
            self.sim.actions.push(SimAction::AddMessage { title: format!("{} : ", translate("Replay saved")), text: file.to_string() });
        } else {
            self.sim.actions.push(SimAction::AddMessage { title: translate("ERROR"), text: translate("Replay not saved") });
        }
    }

    /// `ReplayInterface::resetCuts` 0x14014f860 (`ReplayManager::startReplayMode` calls it).
    pub fn replay_reset_cuts(&mut self, bar: Id) {
        let slider = self.replay_interface(bar).pos_slider;
        self.slider_reset_markers(slider);
    }

    /// `ReplayInterface::onVisibleChanged` 0x14014d6e0 (shown or hidden alike; the children
    /// are not told).
    pub(crate) fn replay_on_visible_changed(&mut self, id: Id) {
        if self.replay_interface(id).camera_manager {
            self.replay_interface_mut(id).force_camera = true;
            let spinner = self.replay_interface(id).camera_spinner;
            self.spinner_set_value(spinner, self.sim.replay.camera_index + 1);
        }
    }

    /// `ReplayInterface::onMouseMove` 0x14014d3d0 (shown or not): every child hears it; the
    /// bar comes up while the mouse is over the bottom of the screen and goes down otherwise.
    pub(crate) fn replay_on_mouse_move(&mut self, id: Id, e: &MouseEvent) {
        let mut i = 0;
        while i < self.controls[id].controls.len() {
            let child = self.controls[id].controls[i];
            self.on_mouse_move(child, e);
            i += 1;
        }
        let top = if self.controls[self.replay_interface(id).text_input].visible { 155.0 } else { 105.0 };
        let h = self.screen.1 as f32;
        self.replay_interface_mut(id).target_y = if e.y as f32 > h - top { (h - 105.0) + 5.0 } else { h + 10.0 };
    }

    /// `ReplayInterface::updateUIElementPosition` 0x14014f870: the bar slides in and out, and
    /// the row of controls is laid out.
    fn replay_update_ui_element_position(&mut self, id: Id, dt: f32) {
        self.reset_base_rect(id);
        let mut wd = self.screen.0 as f32;
        if self.sim.triple_screen {
            wd = self.screen.0 as f32 * f32::from_bits(0x3eaa_aaab);
        }
        let mut m = wd * f32::from_bits(0x3c23_d70a) * 13.5;
        let mut bar_w = wd - (m * 2.0);
        if !(bar_w >= 700.0) {
            bar_w = 700.0;
        }
        let replay_height = self.replay_interface(id).replay_height;
        self.set_size(id, bar_w, replay_height);
        let h = self.screen.1 as f32;
        let y_shown = (h - 105.0) + 5.0;
        let mut y = y_shown;
        let counter = self.replay_interface(id).stay_up_counter;
        if counter == 0.0 {
            let mut r = self.controls[id].rect;
            self.get_world_rect(id, &mut r);
            let step = dt * 300.0;
            let cur = r.top;
            let tgt = self.replay_interface(id).target_y;
            if cur > tgt {
                y = cur - step;
            } else if cur < tgt {
                y = cur + step;
            }
            if (tgt - cur).abs() < step {
                y = tgt;
            }
            if y == y_shown {
                self.replay_interface_mut(id).stay_up_counter = 3.0;
            }
        } else {
            let mut c = counter - dt;
            if c < dt {
                c = 0.0;
            }
            self.replay_interface_mut(id).stay_up_counter = c;
        }
        if self.sim.triple_screen {
            m = m + wd;
        }
        self.set_position(id, floor(m), y);
        let own = self.controls[id].rect;
        let half = (own.right - own.left) * 0.5;
        let r = self.replay_interface(id);
        let (sl, total_width) = (r.pos_slider, r.total_width);
        let (exit, save, rewind, stop, play, next, forward, slow_spinner, slow, camera, car, photo) = (r.but_exit, r.but_save_edit, r.but_rewind, r.but_stop, r.but_play, r.but_next_frame, r.but_fast_forward, r.slow_mo_spinner, r.but_slow_motion, r.camera_spinner, r.car_spinner, r.but_photo_mode);
        let photo_controls = r.photo_controls;
        let s = self.controls[sl].rect;
        if s.right - s.left == 0.0 {
            let k = ((bar_w * 2.0) * f32::from_bits(0x3c6a_0ea1)) * 9.0;
            let w = floor((own.right - own.left) - k);
            self.set_size(sl, w, 6.0);
        }
        self.slider_update_markers(sl);
        let s = self.controls[sl].rect;
        let sx = floor(half - ((s.right - s.left) * 0.5));
        self.set_position(sl, sx, 15.0);
        let replay = &self.sim.replay;
        let p = replay.current_frame as f32 / (replay.recorded_frames - 2) as f32;
        self.slider_mut(sl).value = clamp01(p);
        let y2 = self.controls[sl].rect.bottom + 8.0;
        let x0 = floor(half - (total_width * 0.5));
        self.set_position(exit, x0, y2);
        let mut before = exit;
        for button in [save, rewind, stop, play, next, forward] {
            let x = floor(self.controls[before].rect.right + 5.0);
            self.set_position(button, x, y2);
            before = button;
        }
        let y3 = y2 + 12.0;
        let x7 = floor(self.controls[forward].rect.right + 20.0);
        self.spinner_set_position(slow_spinner, x7, y3);
        let ss = self.controls[slow_spinner].rect;
        // (the spinner's height, not its width)
        let x8 = floor(((ss.bottom - ss.top) + ss.left) + 4.0);
        self.set_position(slow, x8, y2);
        let x9 = floor(self.controls[slow_spinner].rect.right + 20.0);
        self.spinner_set_position(camera, x9, y3);
        let x10 = floor(self.controls[camera].rect.right + 20.0);
        self.spinner_set_position(car, x10, y3);
        let x11 = floor(self.controls[car].rect.right + 20.0);
        self.set_position(photo, x11, y2);
        for control in photo_controls {
            self.set_visible(control, false);
        }
    }

    fn replay_select(&mut self, id: Id, button: Id) {
        if let Some(last) = self.replay_interface(id).last_selected {
            self.active_button_mut(last).selected = false;
        }
        self.active_button_mut(button).selected = true;
        self.replay_interface_mut(id).last_selected = Some(button);
    }

    /// `ReplayInterface::render` 0x14014d970.
    pub(crate) fn replay_render(&mut self, id: Id, dt: f32, painter: &mut dyn Painter) {
        let gl = GRAPHICS_GL;
        // the camera manager, once it is there
        if !self.replay_interface(id).camera_manager && self.sim.replay.camera_manager {
            let mut names: Vec<String> = ["Cockpit", "Chase 1", "Chase 2", "Bonnet", "Bumper", "Dash"].iter().map(|name| translate(name)).collect();
            names.extend(self.sim.replay.track_camera_sets.iter().cloned());
            let spinner = self.replay_interface(id).camera_spinner;
            self.spinner_mut(spinner).min_value = 1;
            self.spinner_mut(spinner).max_value = names.len() as i32;
            let r = self.replay_interface_mut(id);
            r.camera_manager = true;
            r.camera_names = names;
            self.spinner_set_value(spinner, self.sim.replay.camera_index + 1);
        }
        let r = self.replay_interface(id);
        let (car_spinner, sl, camera_spinner, text_input) = (r.car_spinner, r.pos_slider, r.camera_spinner, r.text_input);
        let (play, rewind, forward, slow, photo) = (r.but_play, r.but_rewind, r.but_fast_forward, r.but_slow_motion, r.but_photo_mode);
        let lap_texture = r.lap_indicator_texture.clone();
        self.spinner_mut(car_spinner).max_value = self.sim.race.cars.len() as i32;
        let lap_marks = self.sim.replay.lap_marks.clone();

        self.control_render(id, dt, painter);
        for v in lap_marks {
            let s = self.controls[sl].rect;
            let mut out = self.local_to_world(id, [s.left, s.top]);
            out[0] = out[0] + (s.right - s.left) * v;
            painter.gl_color4f(gl, 1.0, 1.0, 0.0, 0.4);
            painter.set_texture(0, &lap_texture);
            painter.gl_quad(gl, out[0] - 6.0, out[1] - 15.0, 12.0, 30.0, true);
        }

        // (in the photo mode the game lays the photo controls out here instead)
        self.replay_update_ui_element_position(id, dt);

        // the buttons' words and which one is lit
        let (status, time_mult, slow_mo_level) = (self.sim.replay.status, self.sim.replay.time_mult, self.sim.replay.slow_mo_level);
        let mut n = cut(time_mult.abs());
        if n >= 5 {
            n = 5;
        }
        match status {
            REPLAY_PLAY => {
                self.replay_select(id, play);
                self.set_text(play, &translate("PAUSE"));
            }
            REPLAY_REWIND => {
                self.replay_select(id, rewind);
                self.set_text(rewind, &format!("{n}{}", translate("X REWIND")));
            }
            REPLAY_FAST_FORWARD => {
                self.replay_select(id, forward);
                self.set_text(forward, &format!("{n}{}", translate("X FAST FORWARD")));
            }
            REPLAY_SLOW_MOTION => {
                self.set_text(play, &translate("PAUSE"));
                self.replay_select(id, slow);
            }
            REPLAY_PHOTO_MODE => self.replay_select(id, photo),
            _ => {
                for button in [play, forward, rewind, slow, photo] {
                    self.active_button_mut(button).selected = false;
                }
            }
        }
        if status != REPLAY_PLAY && status != REPLAY_SLOW_MOTION {
            self.set_text(play, "PLAY (1X)");
        }
        if status != REPLAY_REWIND {
            self.set_text(rewind, &format!("2{}", translate("X REWIND")));
        }
        if status != REPLAY_FAST_FORWARD {
            self.set_text(forward, &format!("2{}", translate("X FAST FORWARD")));
        }
        self.set_text(slow, &format!("{slow_mo_level}{}", translate("X SLOW")));

        let font = self.controls[id].font.unwrap_or(self.font);
        // the words over the box that asks for the name
        let mut t = self.controls[text_input].rect;
        self.get_world_rect(text_input, &mut t);
        if self.controls[text_input].visible {
            self.fonts[font].set_color(1.0, 1.0, 1.0, 1.0);
            let x = self.screen.0 as f32 * 0.5;
            self.fonts[font].blit_string(painter, x, t.top - 25.0, &format!("{} :", translate("Replay name")), 1.2, ALIGN_CENTER);
        }
        // the camera's name and "CAR#" under their spinners
        let mut c = self.controls[camera_spinner].rect;
        self.get_world_rect(camera_spinner, &mut c);
        let names = &self.replay_interface(id).camera_names;
        if !names.is_empty() {
            // (the game reads the name whether the spinner's number has one or not)
            let name = names.get((self.spinner(camera_spinner).value - 1) as usize).cloned().unwrap_or_default();
            let text = format!("{} {name}", translate("CAMERA"));
            self.fonts[font].blit_string(painter, ((c.right - c.left) * 0.5) + c.left, c.bottom + 13.0, &text, 1.0, ALIGN_CENTER);
        }
        let mut c = self.controls[car_spinner].rect;
        self.get_world_rect(car_spinner, &mut c);
        self.fonts[font].blit_string(painter, ((c.right - c.left) * 0.5) + c.left, c.bottom + 13.0, &format!("{}#", translate("CAR")), 1.0, ALIGN_CENTER);
        // the time now and the whole time, left and right of the time line
        let mut c = self.controls[sl].rect;
        self.get_world_rect(sl, &mut c);
        let replay = &self.sim.replay;
        let time = |frames: i32| {
            let ms = frames as f64 * replay.record_interval_ms;
            time_to_wstring(if ms > -2147483649.0 && ms < 2147483648.0 { ms as i32 } else { i32::MIN }, false, true)
        };
        let (now, whole) = (time(replay.current_frame), time(replay.recorded_frames));
        self.fonts[font].blit_string(painter, c.left - 35.0, c.top - 12.0, &now, 1.3, ALIGN_RIGHT);
        self.fonts[font].blit_string(painter, c.right + 35.0, c.top - 12.0, &whole, 1.3, ALIGN_LEFT);
    }
}
