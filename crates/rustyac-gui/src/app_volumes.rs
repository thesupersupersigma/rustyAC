// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! `FormVolumes` (0x258 bytes), the app "Audio": the master level and the car sounds' levels
//! (Documents `cfg/audio.ini [LEVELS]`), whether the bonnet's and the bumper's views hear the
//! car from outside (`cfg/camera_manager.ini [AUDIO]`), the sound card (`[SETTINGS]
//! DRIVER_NAME`).
//!
//! `FormVolumes::FormVolumes` 0x1402f8fd0, `render` 0x1402fc010, `setSound` 0x1402fcc80,
//! `saveVolume` 0x1402fcb30, `setSpinner` 0x1402fcf80, `refreshDrivers` 0x1402fbd80, the
//! spinners' handlers (0x1402face0 master, 0x1402fae60 engine, 0x1402fb680 wind, 0x1402fb1a0
//! tyres, 0x1402fb340 surfaces, 0x1402fab40 dirt, 0x1402fb4e0 opponents, 0x1402fa8c0
//! transmission), the buttons' (0x1402fba60, 0x1402fbac0, 0x1402fbb10, 0x1402fba20) and
//! "SET DRIVER"'s (0x1402fb000).
//!
//! What the app changes in the session it asks for as [`AppAction`]s: the master level
//! (`AudioEngine::setVolume`), a level of every car's `MixVolumes`, the focused car's view
//! sounds (`CameraDrivableManager::setDrivableCameraDefAt`), and the lines the game writes
//! with `WritePrivateProfileStringW`.

use std::any::Any;
use std::cell::Cell;
use std::rc::Rc;

use rustyac_physics::data::ini::IniReader;

use crate::apps::{App, AppAction, MixLevel};
use crate::control::{Clicked, Id};
use crate::font::{Font, FontType};
use crate::gui::Gui;
use crate::hud::translate;
use crate::painter::{Painter, ALIGN_CENTER, ALIGN_LEFT};
use crate::widgets::WidgetEvent;

const RED: [f32; 4] = [f32::from_bits(0x3f3d_70a4), 0.0, 0.0, 1.0];

pub struct Volumes {
    pub master: Id,
    pub engine: Id,
    pub wind: Id,
    pub tyres: Id,
    pub opponents: Id,
    pub dirt_bottom: Id,
    pub transmission: Id,
    pub surfaces: Id,
    pub bumper_int: Id,
    pub bumper_ext: Id,
    pub bonnet_int: Id,
    pub bonnet_ext: Id,
    pub lab_warning: Id,
    pub driver_list_box: Id,
    pub driver_button: Id,
    /// `driverButtonTimeAccum` (shared with SET DRIVER's handler)
    pub driver_button_time_accum: Rc<Cell<f32>>,
    /// `loaded` (shared with the master's handler)
    pub loaded: Rc<Cell<bool>>,
    pub last_driver_number: i32,
}

fn clamp01(x: f32) -> f32 {
    if x > 1.0 {
        1.0
    } else if x >= 0.0 {
        x
    } else {
        0.0
    }
}

/// `std::to_wstring(float)`: `%f`.
pub fn to_wstring_f(v: f32) -> String {
    format!("{:.6}", v as f64)
}

/// `wcstod` (`std::stoi` for the master) of a spinner's item; the items are "0" … "10".
fn number(text: &str) -> f32 {
    text.trim().parse::<f64>().unwrap_or(0.0) as f32
}

/// `FormVolumes::saveVolume` 0x1402fcb30.
fn save_volume(gui: &mut Gui, key: &str, v: f32) {
    gui.apps.actions.push(AppAction::WriteProfileString { file: "cfg/audio.ini".to_string(), section: "LEVELS".to_string(), key: key.to_string(), value: to_wstring_f(v) });
}

/// `FormVolumes::setSound` 0x1402fcc80: the buttons' colours, the focused car's view, and
/// (from a click) `cfg/camera_manager.ini`.
fn set_sound(gui: &mut Gui, buttons: [Id; 4], which: i16, external: bool, save: bool) {
    let (inner, outer, key) = if which == 0 { (buttons[0], buttons[1], "BONNET_EXTERNAL") } else { (buttons[2], buttons[3], "BUMPER_EXTERNAL") };
    if external {
        gui.controls[inner].back_color = [0.0; 4];
        gui.controls[outer].back_color = RED;
    } else {
        gui.controls[inner].back_color = RED;
        gui.controls[outer].back_color = [0.0; 4];
    }
    if save {
        gui.apps.actions.push(AppAction::WriteProfileString { file: "cfg/camera_manager.ini".to_string(), section: "AUDIO".to_string(), key: key.to_string(), value: (external as u32).to_string() });
    }
    let focused = gui.apps.focused_car;
    if let Some(flags) = gui.apps.camera_sounds.get_mut(focused) {
        if flags[which as usize] != external {
            flags[which as usize] = external;
            gui.apps.actions.push(AppAction::SetCameraSound { car: focused, which: which as usize, external });
        }
    }
}

impl Gui {
    /// `FormVolumes::FormVolumes(Sim*)` 0x1402f8fd0.
    pub fn new_volumes_app(&mut self, painter: &mut dyn Painter) -> Id {
        let form = self.new_form(painter, "FORM_VOLUMES", false);
        let (height, space) = (23.0f32, 35.0f32);
        let loaded = Rc::new(Cell::new(false));
        let accum = Rc::new(Cell::new(5.0f32));
        self.form_data_mut(form).dev_app = false;
        // one Consolas 15 for the captions and the buttons, made before the title
        self.fonts.push(Font::new(painter, "", FontType::Monospaced, 15.0, false, false));
        let font = self.fonts.len() - 1;
        self.form_set_title(form, "Audio");
        let items = |gui: &mut Gui, sp: Id| {
            for i in 0..=10 {
                gui.spinner_add_item(sp, &i.to_string());
            }
        };
        // the car levels' handlers (no `loaded` guard): every car's level, then the file
        let level = |gui: &mut Gui, sp: Id, key: &'static str, kind: MixLevel| {
            gui.spinner_mut(sp).handlers.push(Rc::new(move |gui: &mut Gui, ev: &WidgetEvent| {
                let WidgetEvent::SpinnerValueChanged { spinner, .. } = ev else { return };
                let v = clamp01(number(&gui.spinner_current_item(*spinner)) * f32::from_bits(0x3dcc_cccd));
                gui.apps.actions.push(AppAction::SetMixLevel { level: kind, value: v });
                save_volume(gui, key, v);
            }));
        };
        let caption = |gui: &mut Gui, painter: &mut dyn Painter, name: &str, text: &str, x: f32, y: f32| {
            let l = gui.new_label(painter, name);
            gui.controls[l].font = Some(font);
            gui.set_text(l, text);
            gui.controls[l].font_align = ALIGN_CENTER;
            gui.controls[l].back_color = RED;
            gui.set_size(l, 150.0, height);
            gui.set_position(l, x, y);
            gui.add_control(form, l);
        };
        // the four sound buttons; their ids reach setSound once all four are made
        let ids = Rc::new(Cell::new([0usize; 4]));
        let button = |gui: &mut Gui, painter: &mut dyn Painter, name: &str, text: &str, (x, y): (f32, f32), which: i16, external: bool| {
            let c = gui.new_control(painter, name);
            gui.controls[c].font = Some(font);
            gui.set_text(c, text);
            gui.set_size(c, 100.0, height);
            gui.set_repeat_interval(c, f32::from_bits(0x3d4c_cccd));
            gui.set_position(c, x, y);
            let ids = ids.clone();
            gui.controls[c].ev_clicked.push(Rc::new(move |gui: &mut Gui, _: &Clicked| set_sound(gui, ids.get(), which, external, true)));
            gui.add_control(form, c);
            c
        };
        // the left column
        let mut y = 0.0f32;
        let mut left = |gui: &mut Gui, painter: &mut dyn Painter, name: &str| {
            let sp = gui.new_spinner(painter, name, true, false);
            gui.set_size(sp, 200.0, height);
            y = y + (height + space);
            gui.spinner_set_position(sp, 50.0, y);
            items(gui, sp);
            gui.add_control(form, sp);
            (sp, y)
        };
        let (master, _) = left(self, painter, "Master");
        {
            let loaded = loaded.clone();
            self.spinner_mut(master).handlers.push(Rc::new(move |gui: &mut Gui, ev: &WidgetEvent| {
                let WidgetEvent::SpinnerValueChanged { spinner, .. } = ev else { return };
                if !loaded.get() {
                    return;
                }
                let v = clamp01(number(&gui.spinner_current_item(*spinner)) * f32::from_bits(0x3dcc_cccd));
                gui.apps.actions.push(AppAction::SetMasterVolume(v));
                let v = clamp01(number(&gui.spinner_current_item(*spinner)) * f32::from_bits(0x3dcc_cccd));
                save_volume(gui, "MASTER", v);
            }));
        }
        let (engine, _) = left(self, painter, "Engine");
        level(self, engine, "ENGINE", MixLevel::Engine);
        let (wind, _) = left(self, painter, "Wind");
        level(self, wind, "WIND", MixLevel::Wind);
        let (tyres, y) = left(self, painter, "Tyres");
        level(self, tyres, "TYRES", MixLevel::Tyres);
        let y = y + space;
        caption(self, painter, "BONNET_LBL", "Bonnet", 75.0, y);
        let y = y + height;
        let bonnet_int = button(self, painter, "bonnetInt", "Internal", (50.0, y), 0, false);
        let bonnet_ext = button(self, painter, "bonnetExt", "External", (150.0, y), 0, true);
        // the right column
        let mut y = 0.0f32;
        let mut right = |gui: &mut Gui, painter: &mut dyn Painter, name: &str| {
            // FormVolumes::setSpinner 0x1402fcf80
            let sp = gui.new_spinner(painter, name, true, false);
            y = space + height + y;
            gui.set_size(sp, 200.0, height);
            gui.spinner_set_position(sp, 220.0 + 50.0, y);
            items(gui, sp);
            gui.add_control(form, sp);
            (sp, y)
        };
        let (opponents, _) = right(self, painter, "Opponents");
        level(self, opponents, "OPPONENTS", MixLevel::Opponents);
        let (dirt_bottom, _) = right(self, painter, "Dirt");
        level(self, dirt_bottom, "DIRT_BOTTOM", MixLevel::Dirt);
        let (surfaces, y) = right(self, painter, "Surfaces");
        level(self, surfaces, "SURFACES", MixLevel::Surfaces);
        let transmission = self.new_spinner(painter, "Transmission", true, false);
        let y = y + (height + space);
        self.set_size(transmission, 200.0, height);
        self.spinner_set_position(transmission, 270.0, y);
        self.spinner_add_item(transmission, "ENGINE");
        items(self, transmission);
        self.add_control(form, transmission);
        self.spinner_mut(transmission).handlers.push(Rc::new(move |gui: &mut Gui, ev: &WidgetEvent| {
            let WidgetEvent::SpinnerValueChanged { spinner, .. } = ev else { return };
            let item = gui.spinner_current_item(*spinner);
            let v = if item == "ENGINE" { -1.0 } else { clamp01(number(&item) * f32::from_bits(0x3dcc_cccd)) };
            gui.apps.actions.push(AppAction::SetMixLevel { level: MixLevel::Transmission, value: v });
            save_volume(gui, "TRANSMISSION", v);
        }));
        let y = y + space;
        caption(self, painter, "bumper_LBL", "Bumper", 295.0, y);
        let y = y + height;
        let bumper_int = button(self, painter, "bumperInt", "Internal", (270.0, y), 1, false);
        let bumper_ext = button(self, painter, "bumperExt", "External", (370.0, y), 1, true);
        ids.set([bonnet_int, bonnet_ext, bumper_int, bumper_ext]);
        let y = y + (space + height);
        let driver_list_box = self.new_list_box(painter, "DRIVER_LIST_BOX", 6, 1);
        self.reset_base_rect(driver_list_box);
        self.set_size(driver_list_box, 420.0, 100.0);
        self.set_position(driver_list_box, 50.0, y);
        self.controls[driver_list_box].font_scale = f32::from_bits(0x3f4c_cccd);
        self.list_box_mut(driver_list_box).allow_deselection = false;
        let (grey, edge, alpha) = (f32::from_bits(0x3e19_999a), f32::from_bits(0x3e4c_cccd), f32::from_bits(0x3f4c_cccd));
        self.controls[driver_list_box].back_color = [grey, grey, grey, alpha];
        self.controls[driver_list_box].border_color = [edge, edge, edge, alpha];
        self.list_box_draw_cells_backgrounds(driver_list_box, false);
        let title_row = self.list_box_mut(driver_list_box).title_row;
        self.set_visible(title_row, false);
        self.add_control(form, driver_list_box);
        let y = y + 100.0;
        let driver_button = self.new_control(painter, "DRIVER_BUTTON");
        self.controls[driver_button].font = Some(font);
        self.set_text(driver_button, "SET DRIVER");
        self.set_size(driver_button, 100.0, height);
        self.set_repeat_interval(driver_button, f32::from_bits(0x3d4c_cccd));
        self.set_position(driver_button, 370.0, y + 5.0);
        let warning = Rc::new(Cell::new(0usize));
        {
            let (accum, warning) = (accum.clone(), warning.clone());
            self.controls[driver_button].ev_clicked.push(Rc::new(move |gui: &mut Gui, _: &Clicked| {
                if gui.list_box_mut(driver_list_box).selected_item_index <= -1 {
                    return;
                }
                let name = gui.list_box_selected_item(driver_list_box).map(|d| d.name.clone()).unwrap_or_default();
                gui.apps.actions.push(AppAction::WriteProfileString { file: "cfg/audio.ini".to_string(), section: "SETTINGS".to_string(), key: "DRIVER_NAME".to_string(), value: name });
                gui.set_text(driver_button, &translate("Saved"));
                accum.set(0.0);
                gui.set_visible(warning.get(), true);
            }));
        }
        self.add_control(form, driver_button);
        let lab_warning = self.new_label(painter, "LAB_WARNING");
        warning.set(lab_warning);
        self.controls[lab_warning].font = Some(font);
        self.controls[lab_warning].font_align = ALIGN_LEFT;
        self.set_text(lab_warning, &translate("* driver changes will only be effective after restart"));
        self.set_size(lab_warning, 100.0, height);
        let y = y + 25.0;
        self.set_position(lab_warning, 10.0, y + 5.0);
        self.set_visible(lab_warning, false);
        self.add_control(form, lab_warning);
        self.set_size(form, 520.0, 500.0);
        self.set_app(
            form,
            Box::new(Volumes {
                master,
                engine,
                wind,
                tyres,
                opponents,
                dirt_bottom,
                transmission,
                surfaces,
                bumper_int,
                bumper_ext,
                bonnet_int,
                bonnet_ext,
                lab_warning,
                driver_list_box,
                driver_button,
                driver_button_time_accum: accum,
                loaded,
                last_driver_number: 0,
            }),
        );
        form
    }
}

impl App for Volumes {
    fn as_any(&mut self) -> &mut dyn Any {
        self
    }

    /// `FormVolumes::render` 0x1402fc010.
    fn render(&mut self, gui: &mut Gui, id: Id, dt: f32, painter: &mut dyn Painter) {
        let a = self.driver_button_time_accum.get();
        if a >= 0.0 {
            if a <= 5.0 {
                self.driver_button_time_accum.set(a + dt);
            } else {
                gui.set_text(self.driver_button, &translate(&translate("SET DRIVER")));
                self.driver_button_time_accum.set(-1.0);
            }
        }
        // the four buttons in step with the focused car's views
        let buttons = [self.bonnet_int, self.bonnet_ext, self.bumper_int, self.bumper_ext];
        let flags = gui.apps.camera_sounds.get(gui.apps.focused_car).copied().unwrap_or_default();
        set_sound(gui, buttons, 0, flags[0], false);
        let flags = gui.apps.camera_sounds.get(gui.apps.focused_car).copied().unwrap_or_default();
        set_sound(gui, buttons, 1, flags[1], false);
        // FormVolumes::refreshDrivers 0x1402fbd80 when the sound cards' count changes
        if self.last_driver_number != gui.apps.audio_drivers.len() as i32 {
            self.last_driver_number = gui.apps.audio_drivers.len() as i32;
            gui.list_box_clear(self.driver_list_box);
            for name in gui.apps.audio_drivers.clone() {
                gui.list_box_add_item(self.driver_list_box, &crate::setup_io::row_data(&name));
            }
        }
        gui.control_render(id, dt, painter);
        if self.loaded.get() {
            return;
        }
        // Documents cfg/audio.ini, read every frame until it is there
        let Some(text) = gui.apps.audio_ini.clone() else { return };
        let ini = IniReader::from_text(std::path::Path::new("cfg/audio.ini"), &text);
        if !ini.ready {
            return;
        }
        let level = |gui: &mut Gui, sp: Id, key: &str| {
            let v = clamp01(ini.get_float("LEVELS", key).unwrap_or(0.0));
            gui.spinner_set_value_by_item(sp, &crate::app_essentials::cut_f(v * 10.0f32).to_string());
        };
        level(gui, self.master, "MASTER");
        level(gui, self.wind, "WIND");
        level(gui, self.tyres, "TYRES");
        level(gui, self.surfaces, "SURFACES");
        level(gui, self.dirt_bottom, "DIRT_BOTTOM");
        level(gui, self.opponents, "OPPONENTS");
        level(gui, self.engine, "ENGINE");
        if ini.has_key("LEVELS", "TRANSMISSION") {
            let t = ini.get_float("LEVELS", "TRANSMISSION").unwrap_or(0.0);
            let t = if t > 1.0 {
                1.0
            } else if t >= -1.0 {
                t
            } else {
                -1.0
            };
            if t != -1.0 {
                gui.spinner_set_value_by_item(self.transmission, &crate::app_essentials::cut_f(t * 10.0f32).to_string());
            } else {
                gui.spinner_set_value_by_item(self.transmission, "ENGINE");
            }
        } else {
            gui.spinner_set_value_by_item(self.transmission, "ENGINE");
        }
        self.loaded.set(true);
    }
}
