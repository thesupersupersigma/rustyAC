// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! `SetupTab` (0x1c0 bytes): a page of the setup editor, a box of spinners made from the
//! car's `data/setup.ini`. The special pages (tyres, fuel, gears, electronics, save / load, pit
//! strategy) are `SetupTab`s with more on them (see their modules).
//!
//! `SetupTab::SetupTab` 0x140183130, `addItem` 0x140183850, the spinners' handler 0x140183500
//! and its job for the physics thread 0x140183620, `render` 0x140184370, `drawFrame`
//! 0x140183dc0, `onVisibleChanged` 0x140184280, `getStringToSave` 0x140183e90, `loadFromINI`
//! 0x140184120, `setValueToSpinner` 0x1401844c0, `resetToDefaultValues` 0x140184420.

use std::rc::Rc;

use rustyac_physics::car::setup::SetupJob;
use rustyac_physics::data::ini::IniReader;

use crate::control::{Id, Kind};
use crate::font::{Font, FontType};
use crate::gui::Gui;
use crate::menus::Menu;
use crate::painter::{Painter, ALIGN_CENTER, GL_LINE_STRIP, GRAPHICS_GL};
use crate::widgets::{msvc_floor, WidgetEvent};

/// `SetupItemDef` (0x70 bytes): what `SetupScreen::loadINI` reads of an item's section.
#[derive(Clone, Debug, PartialEq)]
pub struct SetupItemDef {
    /// the spinner's label
    pub name: String,
    /// `connection`: the item's place in the car's `SetupManager::items`
    pub connection: usize,
    pub min_value: f64,
    pub max_value: f64,
    pub step: f64,
    pub px: f32,
    pub py: f32,
    /// `eShowClicksType`: 1 clicks, 2 clicks from the minimum, anything else the plain value
    pub show_clicks_mode: i32,
    pub default_value: i32,
    pub help_key: String,
}

impl Default for SetupItemDef {
    fn default() -> SetupItemDef {
        SetupItemDef { name: String::new(), connection: 0, min_value: 0.0, max_value: 1.0, step: 1.0, px: 0.0, py: 0.0, show_clicks_mode: 0, default_value: 0, help_key: String::new() }
    }
}

/// What a page is besides a `SetupTab`.
pub enum TabKind {
    Generic,
    Io(Box<crate::setup_io::SetupIo>),
    Gears(Box<crate::setup_gears::SetupGears>),
    Tyres(Box<crate::setup_tyres::SetupTyres>),
    Fuel(Box<crate::setup_fuel::SetupFuel>),
    Electronics(Box<crate::setup_electronics::SetupElectronics>),
    Presets(Box<crate::setup_presets::SetupPresets>),
}

/// `SetupTab`'s own members.
pub struct SetupTab {
    pub modified_values: bool,
    pub title_label: Id,
    pub items: Vec<SetupItemDef>,
    pub spinners: Vec<Id>,
    /// each spinner's `tag`: its place in `items` (none: a spinner without an item)
    pub tags: Vec<Option<usize>>,
    pub kind: TabKind,
}

/// `cvttsd2si`.
pub(crate) fn cut_d(x: f64) -> i32 {
    if x > -2147483649.0 && x < 2147483648.0 {
        x as i32
    } else {
        i32::MIN
    }
}

/// `cvttss2si`.
pub(crate) fn cut(x: f32) -> i32 {
    if x > -2147483904.0 && x < 2147483648.0 {
        x as i32
    } else {
        i32::MIN
    }
}

impl Gui {
    pub fn setup_tab(&self, id: Id) -> &SetupTab {
        match &self.controls[id].kind {
            Kind::Menu(menu) => match &**menu {
                Menu::SetupTab(tab) => tab,
                _ => panic!("control {id} is not a setup tab"),
            },
            _ => panic!("control {id} is not a setup tab"),
        }
    }

    pub fn setup_tab_mut(&mut self, id: Id) -> &mut SetupTab {
        match &mut self.controls[id].kind {
            Kind::Menu(menu) => match &mut **menu {
                Menu::SetupTab(tab) => tab,
                _ => panic!("control {id} is not a setup tab"),
            },
            _ => panic!("control {id} is not a setup tab"),
        }
    }

    /// `SetupTab::SetupTab` 0x140183130: a box of 450 by 625 with its name on top.
    pub fn new_setup_tab(&mut self, painter: &mut dyn Painter, name: &str) -> Id {
        let id = self.new_control(painter, name);
        let localized = self.locale.get_string(name);
        if !localized.is_empty() {
            self.controls[id].name = localized;
        }
        let name = self.controls[id].name.clone();
        let title_label = self.new_label(painter, &name);
        self.set_position(title_label, 0.0, 4.0);
        self.controls[title_label].font_align = ALIGN_CENTER;
        self.controls[id].kind = Kind::Menu(Box::new(Menu::SetupTab(Box::new(SetupTab { modified_values: false, title_label, items: Vec::new(), spinners: Vec::new(), tags: Vec::new(), kind: TabKind::Generic }))));
        self.add_control(id, title_label);
        self.controls[id].draw_border = false;
        self.controls[id].draw_background = false;
        self.controls[id].text.clear();
        self.set_position(id, 25.0, 65.0);
        self.control_set_size(id, 450.0, 625.0);
        let r = self.controls[id].rect;
        self.set_size(title_label, r.right - r.left, 30.0);
        self.set_visible(id, true);
        self.controls[id].draw_border = false;
        self.controls[id].draw_background = true;
        self.controls[id].back_color = [0.0, 0.0, 0.0, 0.5];
        id
    }

    /// The handler every spinner of a setup item has (0x140183500), and its job for the
    /// physics thread (0x140183620): the item's `newValue` from the spinner's whole number.
    fn setup_tab_item_changed(&mut self, tab: Id, place: usize, value: i32) {
        self.setup_tab_mut(tab).modified_values = true;
        let def = &self.setup_tab(tab).items[place];
        let v = value as f32;
        let new_value = match def.show_clicks_mode {
            1 => def.step as f32 * v,
            2 => def.step as f32 * v + def.min_value as f32,
            _ => v,
        };
        let item = def.connection;
        if let Some(car) = &mut self.setup_car {
            car.job(SetupJob::SetNewValue { item, value: new_value });
        }
    }

    /// `SetupTab::addItem` 0x140183850: the item's spinner, a whole number.
    pub fn setup_tab_add_item(&mut self, painter: &mut dyn Painter, tab: Id, mut def: SetupItemDef) {
        let Some(car) = &self.setup_car else { return };
        let item = car.item(def.connection);
        let new_value = car.item_new_value(def.connection);
        let sp = self.new_spinner(painter, &item.name, true, false);
        self.fonts.push(Font::new(painter, "", FontType::Proportional, 12.0, false, false));
        self.controls[sp].font = Some(self.fonts.len() - 1);
        self.set_size(sp, 184.0, 21.0);
        let y = msvc_floor(def.py * 60.0 + 70.0);
        let x = msvc_floor(def.px * 220.0 + 23.0);
        self.spinner_set_position(sp, x, y);
        {
            let s = self.spinner_mut(sp);
            s.draw_underline = true;
            s.units = item.units.clone();
            s.display_value_mult = item.label_multiplier;
        }
        self.spinner_set_label_text(sp, &def.name);
        let help = self.locale.setup_help_text(&def.help_key);
        if help.is_empty() {
            self.spinner_mut(sp).display_popover = false;
        } else {
            let pop_over = self.spinner(sp).pop_over;
            self.pop_over_set_text(pop_over, &help);
            self.pop_over_set_label_title(pop_over, &def.name);
            self.spinner_mut(sp).display_popover = true;
        }
        let start = {
            let s = self.spinner_mut(sp);
            match def.show_clicks_mode {
                1 => {
                    s.min_value = cut_d(def.min_value / def.step);
                    s.max_value = cut_d(def.max_value / def.step);
                    s.step = 1;
                    s.units.clear();
                    cut_d(new_value as f64 / def.step + 0.5)
                }
                2 => {
                    s.min_value = 0;
                    s.max_value = cut_d((def.max_value - def.min_value) / def.step);
                    s.step = 1;
                    s.units.clear();
                    cut((new_value - def.min_value as f32) / def.step as f32 + 0.5)
                }
                _ => {
                    s.min_value = cut_d(def.min_value);
                    s.max_value = cut_d(def.max_value);
                    s.step = if def.step >= 1.0 { cut_d(def.step) } else { 1 };
                    cut(new_value)
                }
            }
        };
        // (nobody listens yet)
        self.spinner_set_value(sp, start);
        def.default_value = self.spinner(sp).value;
        let place = self.setup_tab(tab).items.len();
        self.spinner_mut(sp).handlers.push(Rc::new(move |gui: &mut Gui, ev: &WidgetEvent| {
            if let WidgetEvent::SpinnerValueChanged { value, .. } = ev {
                gui.setup_tab_item_changed(tab, place, *value);
            }
        }));
        let t = self.setup_tab_mut(tab);
        t.items.push(def);
        t.spinners.push(sp);
        t.tags.push(Some(place));
        self.add_control(tab, sp);
    }

    /// `SetupTab::render` 0x140184370: the box and its spinners, a grey frame, then the help
    /// bubbles that are open.
    pub(crate) fn setup_tab_render_base(&mut self, id: Id, dt: f32, painter: &mut dyn Painter) {
        self.control_render(id, dt, painter);
        let mut r = self.controls[id].rect;
        self.get_world_rect(id, &mut r);
        // drawFrame 0x140183dc0
        painter.gl_color4f(GRAPHICS_GL, 0.25, 0.25, 0.25, 0.5);
        painter.gl_begin(GRAPHICS_GL, GL_LINE_STRIP);
        painter.gl_vertex3f(GRAPHICS_GL, r.right, r.top, 0.0);
        painter.gl_vertex3f(GRAPHICS_GL, r.right, r.bottom, 0.0);
        painter.gl_vertex3f(GRAPHICS_GL, r.left, r.bottom, 0.0);
        painter.gl_vertex3f(GRAPHICS_GL, r.left, r.top, 0.0);
        painter.gl_vertex3f(GRAPHICS_GL, r.right, r.top, 0.0);
        painter.gl_end(GRAPHICS_GL);
        for sp in self.setup_tab(id).spinners.clone() {
            let pop_over = self.spinner(sp).pop_over;
            if self.controls[pop_over].visible {
                self.render_pop_over(pop_over, dt, painter);
            }
        }
    }

    /// `SetupTab::onVisibleChanged` 0x140184280: a page that comes into view sends every
    /// spinner's value again.
    pub(crate) fn setup_tab_on_visible_changed_base(&mut self, id: Id, visible: bool) {
        if !visible || self.setup_car.is_none() {
            return;
        }
        for child in self.controls[id].controls.clone() {
            if self.is_spinner(child) {
                let value = self.spinner(child).value;
                self.spinner_set_value(child, value);
            }
        }
    }

    /// `SetupTab::getStringToSave` 0x140183e90.
    pub(crate) fn setup_tab_string_to_save_base(&self, id: Id) -> String {
        let mut out = String::new();
        for sp in &self.setup_tab(id).spinners {
            out.push_str(&format!("[{}]\nVALUE={}\n\n", self.controls[*sp].name, self.spinner(*sp).value));
        }
        out
    }

    /// `SetupTab::setValueToSpinner` 0x1401844c0: a value of a fixed setup stays for good
    /// (`SetupTab::fixedVector`).
    pub(crate) fn setup_tab_set_value_to_spinner(&mut self, sp: Id, fixed: bool, value: i32) {
        if fixed {
            self.spinner_mut(sp).is_read_only = true;
            self.setup_fixed.push(sp);
            self.spinner_set_value(sp, value);
        } else if self.setup_fixed.is_empty() {
            self.spinner_mut(sp).is_read_only = false;
            self.spinner_set_value(sp, value);
        } else {
            self.spinner_mut(sp).is_read_only = true;
            if self.setup_fixed.contains(&sp) {
                return;
            }
            self.spinner_set_value(sp, value);
            self.spinner_mut(sp).is_read_only = false;
        }
    }

    /// `SetupTab::loadFromINI` 0x140184120: a section missing leaves the spinner as it is
    /// (and ends the page's loop at a spinner that cannot be changed).
    pub(crate) fn setup_tab_load_from_ini_base(&mut self, id: Id, ini: &IniReader, fixed: bool) {
        for sp in self.setup_tab(id).spinners.clone() {
            let name = self.controls[sp].name.clone();
            if ini.has_section(&name) {
                let value = ini.get_int(&name, "VALUE").unwrap_or(0);
                self.setup_tab_set_value_to_spinner(sp, fixed, value);
            } else {
                if self.spinner(sp).is_read_only {
                    return;
                }
                self.spinner_mut(sp).is_read_only = false;
            }
        }
    }

    /// `SetupTab::resetToDefaultValues` 0x140184420.
    pub(crate) fn setup_tab_reset_to_default_values_base(&mut self, id: Id) {
        let (spinners, tags) = (self.setup_tab(id).spinners.clone(), self.setup_tab(id).tags.clone());
        for (sp, tag) in spinners.into_iter().zip(tags) {
            if let Some(place) = tag {
                if !self.spinner(sp).is_read_only {
                    let value = self.setup_tab(id).items[place].default_value;
                    self.spinner_set_value(sp, value);
                }
            }
        }
        self.setup_tab_mut(id).modified_values = false;
    }

    // ----- the three virtual functions of a page, and the two of a control it has its own of -----

    /// vtable +0xa8.
    pub fn setup_tab_string_to_save(&self, id: Id) -> String {
        match &self.setup_tab(id).kind {
            TabKind::Gears(_) => self.setup_gears_string_to_save(id),
            TabKind::Tyres(_) => self.setup_tyres_string_to_save(id),
            TabKind::Fuel(_) => self.setup_fuel_string_to_save(id),
            TabKind::Electronics(_) => self.setup_electronics_string_to_save(id),
            TabKind::Generic | TabKind::Io(_) | TabKind::Presets(_) => self.setup_tab_string_to_save_base(id),
        }
    }

    /// vtable +0xb0.
    pub fn setup_tab_load_from_ini(&mut self, id: Id, ini: &IniReader, fixed: bool) {
        match &self.setup_tab(id).kind {
            TabKind::Gears(_) => self.setup_gears_load_from_ini(id, ini, fixed),
            TabKind::Tyres(_) => self.setup_tyres_load_from_ini(id, ini, fixed),
            TabKind::Fuel(_) => self.setup_fuel_load_from_ini(id, ini, fixed),
            TabKind::Electronics(_) => self.setup_electronics_load_from_ini(id, ini, fixed),
            TabKind::Generic | TabKind::Io(_) | TabKind::Presets(_) => self.setup_tab_load_from_ini_base(id, ini, fixed),
        }
    }

    /// vtable +0xb8.
    pub fn setup_tab_reset_to_default_values(&mut self, id: Id) {
        match &self.setup_tab(id).kind {
            TabKind::Gears(_) => self.setup_gears_reset_to_default_values(id),
            TabKind::Tyres(_) | TabKind::Fuel(_) => self.setup_tyres_fuel_reset_to_default_values(id),
            TabKind::Electronics(_) => self.setup_electronics_reset_to_default_values(id),
            TabKind::Generic | TabKind::Io(_) | TabKind::Presets(_) => self.setup_tab_reset_to_default_values_base(id),
        }
    }

    pub(crate) fn setup_tab_render(&mut self, id: Id, dt: f32, painter: &mut dyn Painter) {
        match &self.setup_tab(id).kind {
            TabKind::Io(_) => self.setup_io_render(id, dt, painter),
            TabKind::Gears(_) => self.setup_gears_render(id, dt, painter),
            TabKind::Tyres(_) => self.setup_tyres_render(id, dt, painter),
            TabKind::Fuel(_) => self.setup_fuel_render(id, dt, painter),
            TabKind::Electronics(_) => self.setup_electronics_render(id, dt, painter),
            TabKind::Presets(_) => self.setup_presets_render(id, dt, painter),
            TabKind::Generic => self.setup_tab_render_base(id, dt, painter),
        }
    }

    pub(crate) fn setup_tab_on_visible_changed(&mut self, id: Id, visible: bool) {
        match &self.setup_tab(id).kind {
            TabKind::Io(_) => self.setup_io_on_visible_changed(id, visible),
            TabKind::Fuel(_) => self.setup_fuel_on_visible_changed(id, visible),
            TabKind::Electronics(_) => self.setup_electronics_on_visible_changed(id, visible),
            TabKind::Presets(_) => self.setup_presets_on_visible_changed(id, visible),
            TabKind::Generic | TabKind::Gears(_) | TabKind::Tyres(_) => self.setup_tab_on_visible_changed_base(id, visible),
        }
    }
}
