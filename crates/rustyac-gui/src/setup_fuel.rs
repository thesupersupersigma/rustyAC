// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! `SetupFuelManager` (0x228 bytes), the setup editor's page "FUEL": the litres in the tank,
//! how many laps and how long they last, the race's length and its pit window.
//!
//! Constructor 0x14016d880, the spinner's handler 0x14016e510 and its job for the physics
//! thread 0x14016e450 (`Car::setRequestedFuel` 0x140275cb0), `onVisibleChanged` 0x14016ea70,
//! `render` 0x14016ead0, `getSetupFuel` 0x14016e6f0, `getStringToSave` 0x14016e710,
//! `loadFromINI` 0x14016e8f0.

use std::rc::Rc;

use rustyac_physics::car::setup::SetupJob;
use rustyac_physics::data::ini::IniReader;

use crate::acos::format_g6;
use crate::control::Id;
use crate::esc_menu::time_to_wstring;
use crate::font::{Font, FontType};
use crate::gui::Gui;
use crate::hud::translate;
use crate::menus::SESSION_RACE;
use crate::painter::{Painter, Rect, ALIGN_CENTER};
use crate::setup_tab::{cut, cut_d, TabKind};
use crate::widgets::WidgetEvent;

const COL_TITLE_BAR: [f32; 4] = [0.74, 0.0, 0.0, 1.0];

/// `SetupFuelManager`'s own members.
pub struct SetupFuel {
    pub fuel_spinner: Id,
    pub default_value: i32,
    pub race_duration_minutes: i32,
    pub race_is_timed: bool,
    pub race_text: String,
    pub pit_title: Id,
    pub pit_start_title: Id,
    pub pit_start_value: Id,
    pub pit_end_title: Id,
    pub pit_end_value: Id,
}

/// The compiler's inline `floor` that leaves what does not fit as it is, on a value and its
/// whole number.
fn floor_idiom(x: f32) -> f32 {
    crate::widgets::msvc_floor(x)
}

impl Gui {
    fn setup_fuel(&self, id: Id) -> &SetupFuel {
        match &self.setup_tab(id).kind {
            TabKind::Fuel(fuel) => fuel,
            _ => panic!("control {id} is not the fuel page"),
        }
    }

    fn setup_fuel_mut(&mut self, id: Id) -> &mut SetupFuel {
        match &mut self.setup_tab_mut(id).kind {
            TabKind::Fuel(fuel) => fuel,
            _ => panic!("control {id} is not the fuel page"),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn setup_fuel_label(&mut self, painter: &mut dyn Painter, tab: Id, name: &str, w: f32, x: Option<f32>, y: f32, fore: [f32; 4], back: [f32; 4], text: &str) -> Id {
        let label = self.new_label(painter, name);
        self.set_size(label, w, 21.0);
        let x = x.unwrap_or_else(|| {
            let (t, l) = (self.controls[tab].rect, self.controls[label].rect);
            (t.right - t.left) * 0.5 - (l.right - l.left) * 0.5
        });
        self.set_position(label, x, y);
        self.controls[label].fore_color = fore;
        self.controls[label].font_align = ALIGN_CENTER;
        self.controls[label].back_color = back;
        self.set_text(label, text);
        self.add_control(tab, label);
        label
    }

    /// `SetupFuelManager::SetupFuelManager` 0x14016d880.
    pub fn new_setup_fuel(&mut self, painter: &mut dyn Painter) -> Id {
        let id = self.new_setup_tab(painter, &translate("FUEL"));
        self.control_set_size(id, 450.0, 400.0);
        let sp = self.new_spinner(painter, &translate("Liters"), true, false);
        let mut default_value = 0;
        {
            let max_fuel = self.setup_car.as_ref().map_or(0.0, |car| car.max_fuel());
            let s = self.spinner_mut(sp);
            s.min_value = 0;
            s.max_value = cut_d(max_fuel);
            s.step = 1;
        }
        if let Some(car) = &self.setup_car {
            default_value = cut(car.requested_fuel());
            self.spinner_set_value(sp, default_value);
            // the spinner's handler (0x14016e510) and its job (0x14016e450)
            self.spinner_mut(sp).handlers.push(Rc::new(move |gui: &mut Gui, ev: &WidgetEvent| {
                if let WidgetEvent::SpinnerValueChanged { spinner, value } = ev {
                    let now = gui.spinner(*spinner).value;
                    if let Some(car) = &mut gui.setup_car {
                        car.job(SetupJob::SetRequestedFuel { value: *value as f32, spinner_value: now });
                    }
                }
            }));
        }
        self.spinner_set_draw_arrows(painter, sp, true);
        self.set_size(sp, 184.0, 21.0);
        self.fonts.push(Font::new(painter, "", FontType::Proportional, 12.0, false, false));
        self.controls[sp].font = Some(self.fonts.len() - 1);
        let (tab, own) = (self.controls[id].rect, self.controls[sp].rect);
        let x = (tab.right - tab.left) * 0.5 - (own.right - own.left) * 0.5;
        self.spinner_set_position(sp, x, 100.0);
        self.add_control(id, sp);
        let white = [1.0; 4];
        let black = [0.0, 0.0, 0.0, 1.0];
        let pit_title = self.setup_fuel_label(painter, id, "SETUP_FUEL_MANAGER_PIT_TITLE", 284.0, None, 258.0, white, COL_TITLE_BAR, &translate("PIT WINDOW"));
        let left = self.controls[pit_title].rect.left;
        let pit_start_title = self.setup_fuel_label(painter, id, "SETUP_FUEL_MANAGER_PITSTART_TITLE", 64.0, Some(left), 280.0, white, COL_TITLE_BAR, &translate("OPEN"));
        let r = self.controls[pit_start_title].rect;
        let pit_start_value = self.setup_fuel_label(painter, id, "SETUP_FUEL_MANAGER_PITSTART_VALUE", 220.0, Some((r.right - r.left) + r.left), 280.0, black, white, "Minute 35");
        let pit_end_title = self.setup_fuel_label(painter, id, "SETUP_FUEL_MANAGER_PITEND_TITLE", 64.0, Some(left), 302.0, white, COL_TITLE_BAR, &translate("CLOSE"));
        let r = self.controls[pit_end_title].rect;
        let pit_end_value = self.setup_fuel_label(painter, id, "SETUP_FUEL_MANAGER_PITEND_VALUE", 220.0, Some((r.right - r.left) + r.left), 302.0, black, white, "Minute 45");
        self.fonts.push(Font::new(painter, "", FontType::Proportional, 18.0, false, false));
        self.controls[id].font = Some(self.fonts.len() - 1);
        self.setup_tab_mut(id).kind = TabKind::Fuel(Box::new(SetupFuel {
            fuel_spinner: sp,
            default_value,
            race_duration_minutes: -1,
            race_is_timed: false,
            race_text: String::new(),
            pit_title,
            pit_start_title,
            pit_start_value,
            pit_end_title,
            pit_end_value,
        }));
        id
    }

    /// `SetupFuelManager::onVisibleChanged` 0x14016ea70: the spinner takes the car's fuel
    /// again, shown or hidden.
    pub(crate) fn setup_fuel_on_visible_changed(&mut self, id: Id, _visible: bool) {
        let Some(car) = &self.setup_car else { return };
        let value = cut(car.requested_fuel());
        let sp = self.setup_fuel(id).fuel_spinner;
        self.spinner_set_value(sp, value);
    }

    /// `SetupFuelManager::getSetupFuel` 0x14016e6f0.
    pub fn setup_fuel_get_setup_fuel(&self, id: Id) -> f32 {
        self.spinner(self.setup_fuel(id).fuel_spinner).value as f32
    }

    /// `SetupFuelManager::isFixedFuel` 0x14016e8e0.
    pub fn setup_fuel_is_fixed(&self, id: Id) -> bool {
        self.spinner(self.setup_fuel(id).fuel_spinner).is_read_only
    }

    /// `SetupFuelManager::getStringToSave` 0x14016e710.
    pub(crate) fn setup_fuel_string_to_save(&self, id: Id) -> String {
        format!("[FUEL]\nVALUE={}\n{}", self.spinner(self.setup_fuel(id).fuel_spinner).value, self.setup_tab_string_to_save_base(id))
    }

    /// `SetupFuelManager::loadFromINI` 0x14016e8f0.
    pub(crate) fn setup_fuel_load_from_ini(&mut self, id: Id, ini: &IniReader, fixed: bool) {
        let sp = self.setup_fuel(id).fuel_spinner;
        if ini.has_section("FUEL") {
            self.setup_tab_set_value_to_spinner(sp, fixed, ini.get_int("FUEL", "VALUE").unwrap_or(0));
        } else {
            self.spinner_mut(sp).is_read_only = false;
        }
        self.setup_tab_load_from_ini_base(id, ini, fixed);
    }

    /// `SetupFuelManager::render` 0x14016ead0.
    pub(crate) fn setup_fuel_render(&mut self, id: Id, dt: f32, painter: &mut dyn Painter) {
        self.setup_tab_render_base(id, dt, painter);
        let Some(car) = &self.setup_car else { return };
        let (fuel_per_lap, spline) = (car.fuel_per_lap(), car.normalized_spline_position());
        let f = self.setup_fuel(id);
        let (sp, pit_title, pit_start_title, pit_start_value, pit_end_title, pit_end_value) = (f.fuel_spinner, f.pit_title, f.pit_start_title, f.pit_start_value, f.pit_end_title, f.pit_end_value);
        let value = self.spinner(sp).value as f32;
        let mut estimated_ms = 0;
        let text = if fuel_per_lap > 0.0 {
            let laps = self.sim.race.cars.first().map_or(0, |car| car.lap_count);
            if laps != 0 {
                let total = self.setup.session_total_time as f32;
                let t = total / (laps as f32 + spline);
                let x = floor_idiom(((value * 10.0) / fuel_per_lap) * t);
                estimated_ms = cut(x * f32::from_bits(0x3dcc_cccd));
            }
            let laps_left = floor_idiom((value * 10.0) / fuel_per_lap) * f32::from_bits(0x3dcc_cccd);
            format!("{} : {}", translate("Estimated laps"), format_g6(laps_left))
        } else {
            translate("To estimate fuel quantity complete at least two laps")
        };
        self.set_visible(pit_title, false);
        let font = self.controls[id].font.unwrap_or(self.font);
        let x = |gui: &Gui| {
            let mut rect = Rect { left: 0.0, right: 1.0, top: 0.0, bottom: 1.0 };
            gui.get_world_rect(sp, &mut rect);
            let own = gui.controls[sp].rect;
            (own.right - own.left) * 0.5 + (rect.left + 140.0)
        };
        let at = x(self);
        self.fonts[font].blit_string(painter, at, 200.0, &text, 1.0, ALIGN_CENTER);
        if estimated_ms > 0 {
            let text = format!("{} : {}", translate("Estimated time"), time_to_wstring(estimated_ms, false, false));
            let at = x(self);
            self.fonts[font].blit_string(painter, at, 230.0, &text, 1.0, ALIGN_CENTER);
        }
        // the race's length, looked for once
        if self.setup_fuel(id).race_duration_minutes == -1 && self.sim.session_count != 0 {
            let mut i = 0;
            while (i as u32) < self.sim.session_count {
                let session = self.sim.race.session_info(i);
                let fuel = self.setup_fuel_mut(id);
                fuel.race_duration_minutes = 0;
                fuel.race_is_timed = false;
                if session.session_type == SESSION_RACE {
                    fuel.race_duration_minutes = cut(session.duration_minutes);
                    fuel.race_is_timed = session.is_timed_race;
                    fuel.race_text = if session.is_timed_race {
                        format!("{} {}", translate("Race Time"), time_to_wstring(cut((session.duration_minutes * 60.0) * 1000.0), false, false))
                    } else {
                        format!("{} {}", translate("Race Laps"), session.laps)
                    };
                    break;
                }
                i += 1;
            }
        }
        let race_text = self.setup_fuel(id).race_text.clone();
        let at = x(self);
        self.fonts[font].blit_string(painter, at, 260.0, &race_text, 1.0, ALIGN_CENTER);
        let (start, end) = (self.sim.race.pit_window_start, self.sim.race.pit_window_end);
        if end != 0 {
            self.set_visible(pit_title, true);
            let fuel = self.setup_fuel(id);
            let (open, close) = if fuel.race_is_timed {
                (format!("{} minutes left", fuel.race_duration_minutes - start as i32), format!("{} minutes left", fuel.race_duration_minutes - end as i32))
            } else {
                (format!("{} {start}", translate("Lap")), format!("{} {end}", translate("Lap")))
            };
            self.set_text(pit_start_value, &open);
            self.set_text(pit_end_value, &close);
        }
        let shown = self.controls[pit_title].visible;
        for label in [pit_start_title, pit_start_value, pit_end_title, pit_end_value] {
            self.set_visible(label, shown);
        }
    }
}
