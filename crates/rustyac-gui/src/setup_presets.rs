// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! `SetupPitstopPresets` (0x228 bytes, vtable 0x1404d4810), the setup editor's page "PITSTOP
//! STRATEGY": the pit menu's presets, one at a time, as six spinners (fuel to add, compound,
//! the four tyre pressures) under a row of numbered buttons.
//!
//! Constructor 0x1401787d0, the buttons' handler 0x14017a3c0, the spinners' 0x14017a3e0,
//! `render` 0x14017a7e0, `onVisibleChanged` 0x14017a7d0 (= `syncSpinners` 0x14017ac00),
//! `syncCarPitstop` 0x14017aa70, `loadPresets` 0x14017a570, `savePresets` 0x14017a8b0,
//! `loadFixed` 0x14017a560 (`QuickMenuPitstop::loadFixedSetup` 0x1400b0850).
//!
//! The page builds its controls by hand (no `SetupTab::addItem`): it saves an empty line in a
//! setup, loads nothing from one and never marks the setup changed. The values live in the pit
//! menu ([`crate::setup_car::SetupPit`]); the spinners only show them.

use std::path::Path;
use std::rc::Rc;

use rustyac_physics::data::ini::IniReader;
use rustyac_physics::race::pitstop::{ITEM_FUEL, ITEM_TYRE_COMPOUND, ITEM_TYRE_PRESSURE};

use crate::control::{Clicked, Id};
use crate::font::{Font, FontType};
use crate::gui::Gui;
use crate::hud::translate;
use crate::painter::{Painter, ALIGN_CENTER};
use crate::setup_tab::{cut, cut_d, TabKind};
use crate::widgets::WidgetEvent;

const COL_TITLE_BAR: [f32; 4] = [0.74, 0.0, 0.0, 1.0];

/// `SetupPitstopPresets`'s own members.
pub struct SetupPresets {
    /// the GUI's pit menu (`pit`), made by the setup screen
    pub quick_menu: Option<Id>,
    /// the spinners' handlers write into the preset only once a `syncSpinners` has ended
    pub force: bool,
    /// the preset shown and edited (the page's own; not the one the pit stop uses)
    pub current_preset: f32,
    pub presets: Vec<Id>,
    pub fuel_spinner: Id,
    pub compound_spinner: Id,
    pub pressure_spinner: [Id; 4],
    pub pressure_alert: Id,
}

impl Gui {
    fn setup_presets(&self, id: Id) -> &SetupPresets {
        match &self.setup_tab(id).kind {
            TabKind::Presets(presets) => presets,
            _ => panic!("control {id} is not the pit strategy page"),
        }
    }

    fn setup_presets_mut(&mut self, id: Id) -> &mut SetupPresets {
        match &mut self.setup_tab_mut(id).kind {
            TabKind::Presets(presets) => presets,
            _ => panic!("control {id} is not the pit strategy page"),
        }
    }

    /// The x that centres a control on the page.
    fn setup_presets_centre(&self, id: Id, control: Id) -> f32 {
        let (tab, own) = (self.controls[id].rect, self.controls[control].rect);
        (tab.right - tab.left) * 0.5 - (own.right - own.left) * 0.5
    }

    fn setup_presets_font(&mut self, painter: &mut dyn Painter, control: Id) {
        self.fonts.push(Font::new(painter, "", FontType::Proportional, 12.0, false, false));
        self.controls[control].font = Some(self.fonts.len() - 1);
    }

    /// The handler of every spinner of the page (the linker folded the six into 0x14017a3e0).
    fn setup_presets_spinner_handler(&mut self, id: Id, spinner: Id) {
        self.spinner_mut(spinner).handlers.push(Rc::new(move |gui: &mut Gui, ev: &WidgetEvent| {
            if let WidgetEvent::SpinnerValueChanged { .. } = ev {
                gui.setup_presets_sync_car_pitstop(id);
            }
        }));
    }

    /// `SetupPitstopPresets::SetupPitstopPresets` 0x1401787d0.
    pub fn new_setup_presets(&mut self, painter: &mut dyn Painter, quick_menu: Option<Id>) -> Id {
        let id = self.new_setup_tab(painter, &translate("PITSTOP STRATEGY"));
        self.control_set_size(id, 450.0, 400.0);
        let fuel_spinner = self.new_spinner(painter, &translate("Add Liters"), true, false);
        {
            let max_fuel = self.setup_car.as_ref().map_or(0.0, |car| car.max_fuel());
            let s = self.spinner_mut(fuel_spinner);
            s.min_value = 0;
            s.max_value = cut_d(max_fuel);
            s.step = 1;
        }
        self.spinner_set_draw_arrows(painter, fuel_spinner, true);
        self.set_size(fuel_spinner, 184.0, 21.0);
        let x = self.setup_presets_centre(id, fuel_spinner);
        self.spinner_set_position(fuel_spinner, x, 120.0);
        self.setup_presets_font(painter, fuel_spinner);
        self.setup_presets_spinner_handler(id, fuel_spinner);
        self.add_control(id, fuel_spinner);
        // the numbered buttons, one per preset
        let count = self.setup.pit.as_ref().map_or(0, |pit| pit.menu.presets_count);
        let width = self.controls[id].rect.right - self.controls[id].rect.left;
        let cw = (width - 40.0) / count as f32;
        let mut presets = Vec::new();
        let mut i = 0;
        while (i as f32) < count as f32 {
            let b = self.new_control(painter, &format!("PITSTOP_PRESETS_{i}"));
            self.set_text(b, &(i + 1).to_string());
            self.set_size(b, cw, 21.0);
            self.controls[b].font_align = ALIGN_CENTER;
            self.set_position(b, i as f32 * cw + 20.0, 60.0);
            self.controls[b].back_color = [1.0; 4];
            self.controls[b].fore_color = [0.0, 0.0, 0.0, 1.0];
            self.setup_presets_font(painter, b);
            presets.push(b);
            self.controls[b].ev_clicked.push(Rc::new(move |gui: &mut Gui, _ev: &Clicked| {
                gui.setup_presets_mut(id).current_preset = i as f32;
                gui.setup_presets_sync_spinners(id);
            }));
            self.add_control(id, b);
            i += 1;
        }
        let compound_spinner = self.new_spinner(painter, &translate("Compound"), true, false);
        self.spinner_mut(compound_spinner).step = 1;
        self.spinner_set_draw_arrows(painter, compound_spinner, true);
        self.set_size(compound_spinner, 350.0, 21.0);
        let x = self.setup_presets_centre(id, compound_spinner);
        self.spinner_set_position(compound_spinner, x, 180.0);
        self.setup_presets_font(painter, compound_spinner);
        self.spinner_add_item(compound_spinner, "NO CHANGE");
        for name in self.setup_car.as_ref().map(|car| car.tyre_compound_names()).unwrap_or_default() {
            self.spinner_add_item(compound_spinner, &name);
        }
        self.setup_presets_spinner_handler(id, compound_spinner);
        self.add_control(id, compound_spinner);
        let mut pressure_spinner = [0; 4];
        for (wheel, name) in ["Pressure LF", "Pressure RF", "Pressure LR", "Pressure RR"].iter().enumerate() {
            let sp = self.new_spinner(painter, &translate(name), true, false);
            pressure_spinner[wheel] = sp;
            // QuickMenuPitstop::getItemValues 0x1400b02f0: the first pressure item of the wheel
            // (none: the default item's 0 and 1)
            let (min, max) = self
                .setup
                .pit
                .as_ref()
                .and_then(|pit| pit.menu.items.iter().find(|item| item.kind == ITEM_TYRE_PRESSURE && item.component_index == wheel as i32))
                .map_or((0.0, 1.0), |item| (item.min_value, item.max_value));
            {
                let s = self.spinner_mut(sp);
                s.min_value = cut(min);
                s.max_value = cut(max);
                s.step = 1;
            }
            self.spinner_set_draw_arrows(painter, sp, true);
            self.set_size(sp, 184.0, 21.0);
            let (tab, own) = (self.controls[id].rect, self.controls[sp].rect);
            let right = (tab.right - tab.left - 20.0) - (own.right - own.left);
            let (x, y) = match wheel {
                0 => (20.0, 240.0),
                1 => (right, 240.0),
                2 => (20.0, 300.0),
                _ => (right, 300.0),
            };
            self.spinner_set_position(sp, x, y);
            self.setup_presets_font(painter, sp);
            self.setup_presets_spinner_handler(id, sp);
            self.add_control(id, sp);
        }
        let pressure_alert = self.new_label(painter, "PRESSURE_ALERT");
        self.set_text(pressure_alert, &translate("Effective pressure adjustment will be limited to a range of 8psi"));
        self.controls[pressure_alert].font_align = ALIGN_CENTER;
        let width = self.controls[id].rect.right - self.controls[id].rect.left;
        self.set_position(pressure_alert, width * 0.5, 340.0);
        self.add_control(id, pressure_alert);
        self.setup_tab_mut(id).kind = TabKind::Presets(Box::new(SetupPresets {
            quick_menu,
            force: false,
            current_preset: 0.0,
            presets,
            fuel_spinner,
            compound_spinner,
            pressure_spinner,
            pressure_alert,
        }));
        self.setup_presets_sync_spinners(id);
        id
    }

    /// `SetupPitstopPresets::syncSpinners` 0x14017ac00: the shown preset into the spinners
    /// (their handlers write nothing back meanwhile). A fixed pressure turns read-only.
    fn setup_presets_sync_spinners(&mut self, id: Id) {
        self.setup_presets_mut(id).force = false;
        let Some(pit) = self.setup.pit.as_ref().filter(|pit| !pit.menu.items.is_empty()) else {
            self.setup_presets_mut(id).force = true;
            return;
        };
        let p = self.setup_presets(id);
        let preset = pit.menu.presets.get(p.current_preset as i64 as usize).cloned().unwrap_or_default();
        let items: Vec<_> = pit.menu.items.iter().map(|item| (item.kind, item.component_index, item.is_fixed)).collect();
        let (fuel, compound, pressure) = (p.fuel_spinner, p.compound_spinner, p.pressure_spinner);
        for (i, (kind, component, fixed)) in items.into_iter().enumerate() {
            let value = preset.get(i).copied().unwrap_or(0.0);
            match kind {
                ITEM_FUEL => self.spinner_set_value(fuel, cut(value)),
                ITEM_TYRE_COMPOUND => self.spinner_set_value(compound, cut(value).wrapping_add(1)),
                ITEM_TYRE_PRESSURE => {
                    let Some(&sp) = pressure.get(component.max(0) as usize) else { continue };
                    if fixed {
                        self.spinner_mut(sp).is_read_only = true;
                    }
                    self.spinner_set_value(sp, cut(value));
                }
                _ => {}
            }
        }
        self.setup_presets_mut(id).force = true;
    }

    /// `SetupPitstopPresets::syncCarPitstop` 0x14017aa70: every spinner into the shown preset.
    fn setup_presets_sync_car_pitstop(&mut self, id: Id) {
        let p = self.setup_presets(id);
        if !p.force {
            return;
        }
        let index = p.current_preset as i64 as usize;
        let (fuel, compound, pressure) = (self.spinner(p.fuel_spinner).value, self.spinner(p.compound_spinner).value, p.pressure_spinner.map(|sp| self.spinner(sp).value));
        let Some(pit) = &mut self.setup.pit else { return };
        let items: Vec<_> = pit.menu.items.iter().map(|item| (item.kind, item.component_index)).collect();
        let Some(preset) = pit.menu.presets.get_mut(index) else { return };
        for (i, (kind, component)) in items.into_iter().enumerate() {
            let value = match kind {
                ITEM_FUEL => fuel as f32,
                ITEM_TYRE_COMPOUND => compound.wrapping_sub(1) as f32,
                ITEM_TYRE_PRESSURE => match pressure.get(component.max(0) as usize) {
                    Some(&value) => value as f32,
                    None => continue,
                },
                _ => continue,
            };
            if let Some(slot) = preset.get_mut(i) {
                *slot = value;
            }
        }
        pit.edits += 1;
    }

    /// `SetupPitstopPresets::render` 0x14017a7e0: the shown preset's button in the title bar's
    /// red with white text, the others white with black.
    pub(crate) fn setup_presets_render(&mut self, id: Id, dt: f32, painter: &mut dyn Painter) {
        let p = self.setup_presets(id);
        let (presets, current) = (p.presets.clone(), p.current_preset);
        for (i, b) in presets.into_iter().enumerate() {
            let selected = i as f32 == current;
            self.controls[b].back_color = if selected { COL_TITLE_BAR } else { [1.0; 4] };
            self.controls[b].fore_color = if selected { [1.0; 4] } else { [0.0, 0.0, 0.0, 1.0] };
        }
        self.setup_tab_render_base(id, dt, painter);
    }

    /// `onVisibleChanged` 0x14017a7d0: `syncSpinners`, on showing and on hiding.
    pub(crate) fn setup_presets_on_visible_changed(&mut self, id: Id, _visible: bool) {
        self.setup_presets_sync_spinners(id);
    }

    /// `savePresets` 0x14017a8b0: the `.sp` file beside a saved setup.
    pub fn setup_presets_save_text(&self, _id: Id) -> String {
        self.setup.pit.as_ref().map_or_else(String::new, |pit| pit.menu.save_presets())
    }

    /// `loadFixed` 0x14017a560: the server's setup into the pit menu (the spinners follow at
    /// the page's next showing or hiding).
    pub fn setup_presets_load_fixed(&mut self, _id: Id, path: &Path) {
        if let Some(pit) = &mut self.setup.pit {
            let ini = IniReader::load(path).unwrap_or_default();
            pit.menu.load_fixed_setup(&ini, &pit.car);
            pit.edits += 1;
        }
    }

    /// `loadPresets` 0x14017a570: a setup's `.sp` file, then `syncSpinners`.
    pub fn setup_presets_load_presets(&mut self, id: Id, reader: &IniReader) {
        if let Some(pit) = &mut self.setup.pit {
            pit.menu.load_presets(reader);
            pit.edits += 1;
        }
        self.setup_presets_sync_spinners(id);
    }
}
