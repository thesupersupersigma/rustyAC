// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! `SetupElectronics` (0x1e8 bytes), the setup editor's page "ELECTRONICS": traction control,
//! ABS, turbo boost, the hybrid's three knobs and the engine brake, each a spinner that steps
//! the car's own setting (the car is cycled as often as the spinner moved).
//!
//! Constructor 0x14016a020, `checkDrivingAssist` 0x14016cb40, `addTC` 0x14016c610, `addABS`
//! 0x14016b900, `addTurbo` 0x14016c890, `addErsPower` 0x14016be50, `addErsRecovery`
//! 0x14016c100, `addHeatCharging` 0x14016c380, `addEngineBrake` 0x14016bbb0, `setSpinner`
//! 0x14016d3f0, the spinners' handler 0x14016a8f0, `onVisibleChanged` 0x14016d180, `render`
//! 0x14016d2c0, `getStringToSave` 0x14016cd60, `loadFromINI` 0x14016d050,
//! `resetToDefaultValues` 0x14016d350.

use std::rc::Rc;

use rustyac_physics::car::setup::SetupJob;
use rustyac_physics::data::ini::IniReader;

use crate::control::Id;
use crate::font::{Font, FontType};
use crate::gui::Gui;
use crate::hud::translate;
use crate::painter::Painter;
use crate::setup_tab::{cut, TabKind};
use crate::widgets::WidgetEvent;

/// Which of the car's settings a spinner of the page is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Knob {
    TractionControl,
    Abs,
    Turbo,
    ErsPower,
    ErsRecovery,
    HeatCharging,
    EngineBrake,
}

/// `SpinnerData`.
#[derive(Clone, Debug, PartialEq)]
pub struct SpinnerData {
    pub name: String,
    pub spinner: Id,
    pub default_value: f32,
    pub min_value: f32,
    pub max_value: f32,
    /// `functionGetValue`
    pub knob: Knob,
}

/// `SetupElectronics`'s own members.
#[derive(Default)]
pub struct SetupElectronics {
    pub abs_assist: i32,
    pub tc_assist: i32,
    pub spinner_datas: Vec<SpinnerData>,
}

impl Gui {
    fn setup_electronics(&self, id: Id) -> &SetupElectronics {
        match &self.setup_tab(id).kind {
            TabKind::Electronics(electronics) => electronics,
            _ => panic!("control {id} is not the electronics page"),
        }
    }

    /// A spinner's getter: the car's setting as a number.
    fn setup_electronics_get(&self, knob: Knob) -> f32 {
        let Some(car) = &self.setup_car else { return 0.0 };
        match knob {
            Knob::TractionControl => car.tc_mode().0 as f32,
            Knob::Abs => car.abs_mode().0 as f32,
            Knob::Turbo => car.turbo_boost_level() * 10.0,
            Knob::ErsPower => car.ers_power_index() as f32,
            Knob::ErsRecovery => car.ers_recovery() as f32,
            Knob::HeatCharging => car.ers_heat_charging() as u8 as f32,
            Knob::EngineBrake => car.engine_brake_setting() as f32,
        }
    }

    /// A spinner's setter: the car's own cycler, one step (`CarAvatar::cycleTcMode` and the
    /// others; a system the car has not got says so).
    fn setup_electronics_set(&mut self, knob: Knob, value: f32) {
        let Some(car) = &mut self.setup_car else { return };
        let dir = cut(value);
        match knob {
            Knob::TractionControl if !car.tc_available() => self.sim.actions.push(crate::menus::SimAction::AddMessage { title: "TC".to_string(), text: translate("Not available") }),
            Knob::Abs if !car.abs_available() => self.sim.actions.push(crate::menus::SimAction::AddMessage { title: "ABS".to_string(), text: translate("Not available") }),
            Knob::TractionControl => car.job(SetupJob::CycleTc { dir }),
            Knob::Abs => car.job(SetupJob::CycleAbs { dir }),
            Knob::Turbo => car.job(SetupJob::SetTurboBoostLevel { level: value * f32::from_bits(0x3dcc_cccd) }),
            Knob::ErsPower => car.job(SetupJob::CycleErsPower { dir }),
            Knob::ErsRecovery => car.job(SetupJob::CycleErsRecovery { dir }),
            Knob::HeatCharging => car.job(SetupJob::CycleErsHeatCharging),
            Knob::EngineBrake => car.job(SetupJob::CycleEngineBrake { dir }),
        }
    }

    /// A spinner's text for the number `i`.
    fn setup_electronics_text(&self, knob: Knob, i: i32) -> String {
        let Some(car) = &self.setup_car else { return i.to_string() };
        match knob {
            Knob::TractionControl | Knob::Abs => {
                if i == 0 {
                    "OFF".to_string()
                } else {
                    i.to_string()
                }
            }
            Knob::Turbo | Knob::ErsRecovery => format!("{}%", i * 10),
            Knob::EngineBrake => format!("{i}/{}", car.engine_brake_settings_count()),
            Knob::ErsPower => usize::try_from(i).ok().and_then(|i| car.ers_power_controller_names().get(i).cloned()).unwrap_or_default(),
            Knob::HeatCharging => if i == 1 { "Battery" } else { "Motor" }.to_string(),
        }
    }

    /// The spinners' handler (0x14016a8f0): the car is stepped from where it is to the
    /// spinner's number (the turbo is set outright).
    fn setup_electronics_changed(&mut self, knob: Knob, value: i32) {
        if knob == Knob::Turbo {
            self.setup_electronics_set(knob, value as f32);
            return;
        }
        let current = self.setup_electronics_get(knob);
        let diff = cut(value as f32 - current);
        let dir = if diff < 0 { -1.0 } else { 1.0 };
        for _ in 0..diff.unsigned_abs() {
            self.setup_electronics_set(knob, dir);
        }
    }

    /// One of the `add…` functions with `SetupElectronics::setSpinner` 0x14016d3f0.
    #[allow(clippy::too_many_arguments)]
    fn setup_electronics_add(&mut self, painter: &mut dyn Painter, label: &str, name: &str, default_value: f32, min_value: f32, max_value: f32, knob: Knob) -> SpinnerData {
        let sp = self.new_spinner(painter, &translate(label), true, false);
        {
            let s = self.spinner_mut(sp);
            s.min_value = cut(min_value);
            s.step = 1;
        }
        let help = self.locale.setup_help_text(&format!("HELP_{name}"));
        if help.is_empty() {
            self.spinner_mut(sp).display_popover = false;
        } else {
            let pop_over = self.spinner(sp).pop_over;
            let title = self.controls[sp].name.clone();
            self.pop_over_set_text(pop_over, &help);
            self.pop_over_set_label_title(pop_over, &title);
            self.spinner_mut(sp).display_popover = true;
        }
        let mut i = cut(min_value);
        while (i as f32) < max_value + 1.0 {
            let text = self.setup_electronics_text(knob, i);
            self.spinner_add_item(sp, &text);
            i += 1;
        }
        self.spinner_set_value(sp, cut(default_value));
        self.spinner_mut(sp).handlers.push(Rc::new(move |gui: &mut Gui, ev: &WidgetEvent| {
            if let WidgetEvent::SpinnerValueChanged { value, .. } = ev {
                gui.setup_electronics_changed(knob, *value);
            }
        }));
        self.spinner_set_draw_arrows(painter, sp, true);
        self.set_size(sp, 184.0, 21.0);
        self.fonts.push(Font::new(painter, "", FontType::Proportional, 12.0, false, false));
        self.controls[sp].font = Some(self.fonts.len() - 1);
        SpinnerData { name: name.to_string(), spinner: sp, default_value, min_value, max_value, knob }
    }

    /// `SetupElectronics::SetupElectronics` 0x14016a020. The assists (`cfg/assists.ini`, or
    /// the server's) are [`crate::setup_car::SetupLink::tc_assist`] and `abs_assist`.
    pub fn new_setup_electronics(&mut self, painter: &mut dyn Painter) -> Id {
        let id = self.new_setup_tab(painter, &translate("ELECTRONICS"));
        let mut datas = Vec::new();
        // checkDrivingAssist 0x14016cb40
        let (tc_assist, abs_assist) = (self.setup.tc_assist, self.setup.abs_assist);
        if let Some(car) = &self.setup_car {
            let (tc_available, abs_available) = (car.tc_available(), car.abs_available());
            let (tc_mode, abs_mode) = (car.tc_mode(), car.abs_mode());
            let (turbo, boost) = (car.has_adjustable_turbo(), car.turbo_boost_level());
            let (has_ers, controllers) = (car.has_ers(), car.ers_power_controller_names().len() as i32);
            let (delivery, recovery, heat) = (car.has_cockpit_ers_delivery_profile(), car.has_cockpit_ers_recovery(), car.has_cockpit_ers_mgu_h_mode());
            let (power_index, recovery_now, heat_now) = (car.ers_power_index(), car.ers_recovery(), car.ers_heat_charging());
            let (brake_count, brake_now) = (car.engine_brake_settings_count(), car.engine_brake_setting());
            if (tc_available || tc_assist == 2) && tc_assist != 0 {
                let (first, second) = if tc_available { tc_mode } else { (1, 1) };
                datas.push(self.setup_electronics_add(painter, "Traction Control", "TRACTION_CONTROL", first as f32, 0.0, second as f32, Knob::TractionControl));
            }
            if (abs_available || abs_assist == 2) && abs_assist != 0 {
                let (first, second) = if abs_available { abs_mode } else { (1, 1) };
                datas.push(self.setup_electronics_add(painter, "ABS", "ABS", first as f32, 0.0, second as f32, Knob::Abs));
            }
            if turbo {
                datas.push(self.setup_electronics_add(painter, "TURBO", "TURBO", boost * 10.0, 0.0, 10.0, Knob::Turbo));
            }
            if has_ers {
                if controllers > 0 {
                    if delivery {
                        datas.push(self.setup_electronics_add(painter, "MGUK-Delivery", "MGUK_DELIVERY", power_index as f32, 0.0, (controllers - 1) as f32, Knob::ErsPower));
                    }
                    if recovery {
                        datas.push(self.setup_electronics_add(painter, "MGUK-Recovery", "MGUK_RECOVERY", recovery_now as f32, 0.0, 10.0, Knob::ErsRecovery));
                    }
                }
                if heat {
                    datas.push(self.setup_electronics_add(painter, "MGUH-Mode", "MGUH_MODE", heat_now as u8 as f32, 0.0, 1.0, Knob::HeatCharging));
                }
            }
            if brake_count != 0 {
                datas.push(self.setup_electronics_add(painter, "Brake Engine", "BRAKE_ENGINE", brake_now as f32, 1.0, brake_count as f32, Knob::EngineBrake));
            }
        }
        let mut y = 0x50;
        for data in &datas {
            let (tab, own) = (self.controls[id].rect, self.controls[data.spinner].rect);
            let x = (tab.right - tab.left) * 0.5 - (own.right - own.left) * 0.5;
            self.spinner_set_position(data.spinner, x, y as f32);
            self.add_control(id, data.spinner);
            y += 0x50;
        }
        self.control_set_size(id, 450.0, y as f32);
        self.fonts.push(Font::new(painter, "", FontType::Proportional, 18.0, false, false));
        self.controls[id].font = Some(self.fonts.len() - 1);
        self.setup_tab_mut(id).kind = TabKind::Electronics(Box::new(SetupElectronics { abs_assist, tc_assist, spinner_datas: datas }));
        id
    }

    pub(crate) fn setup_electronics_has_spinners(&self, id: Id) -> bool {
        !self.setup_electronics(id).spinner_datas.is_empty()
    }

    /// `SetupElectronics::onVisibleChanged` 0x14016d180: the spinners take the car's settings.
    pub(crate) fn setup_electronics_on_visible_changed(&mut self, id: Id, visible: bool) {
        if !visible || self.setup_car.is_none() {
            return;
        }
        for data in self.setup_electronics(id).spinner_datas.clone() {
            let value = cut(self.setup_electronics_get(data.knob));
            self.spinner_set_value(data.spinner, value);
        }
    }

    /// `SetupElectronics::render` 0x14016d2c0.
    pub(crate) fn setup_electronics_render(&mut self, id: Id, dt: f32, painter: &mut dyn Painter) {
        self.setup_tab_render_base(id, dt, painter);
        for data in self.setup_electronics(id).spinner_datas.clone() {
            let pop_over = self.spinner(data.spinner).pop_over;
            if self.controls[pop_over].visible {
                self.render_pop_over(pop_over, dt, painter);
            }
        }
    }

    /// `SetupElectronics::getStringToSave` 0x14016cd60.
    pub(crate) fn setup_electronics_string_to_save(&self, id: Id) -> String {
        let mut out = String::new();
        for data in &self.setup_electronics(id).spinner_datas {
            out.push_str(&format!("[{}]\nVALUE={}\n", data.name, self.spinner(data.spinner).value));
        }
        out + &self.setup_tab_string_to_save_base(id)
    }

    /// `SetupElectronics::loadFromINI` 0x14016d050: never fixed.
    pub(crate) fn setup_electronics_load_from_ini(&mut self, id: Id, ini: &IniReader, fixed: bool) {
        for data in self.setup_electronics(id).spinner_datas.clone() {
            if ini.has_section(&data.name) {
                self.setup_tab_set_value_to_spinner(data.spinner, false, ini.get_int(&data.name, "VALUE").unwrap_or(0));
            } else {
                self.spinner_mut(data.spinner).is_read_only = false;
            }
        }
        self.setup_tab_load_from_ini_base(id, ini, fixed);
    }

    /// `SetupElectronics::resetToDefaultValues` 0x14016d350.
    pub(crate) fn setup_electronics_reset_to_default_values(&mut self, id: Id) {
        for data in self.setup_electronics(id).spinner_datas.clone() {
            if !self.spinner(data.spinner).is_read_only {
                self.spinner_set_value(data.spinner, cut(data.default_value));
            }
        }
        self.setup_tab_reset_to_default_values_base(id);
    }
}
