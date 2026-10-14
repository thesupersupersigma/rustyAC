// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! `ksgui::GUI` (0x190 bytes): the root of the widgets. It owns the top-level controls and the
//! app bar, draws them once a frame, hands the window's mouse and key events to them, drags
//! forms, and keeps where every form is on every virtual desktop.
//!
//! `GUI::GUI` 0x140251ac0, `getSkinColor` 0x140253e60, `addControl` 0x140252e50, `render`
//! 0x140254c10, `doRender2D` 0x1402539d0, `onMouseDown` 0x140254290, `onMouseMove`
//! 0x140254320, `onMouseUp` 0x1402545c0, `onMouseWheelMoved` 0x140254720, `onKeyDown`
//! 0x140254230, `onKeyChar` 0x1402541e0, `setCapturedForm` 0x1402556c0, `isFormInside`
//! 0x140254070, `isPointInside` 0x140254190; the layout file's functions are in `acos`.
//!
//! Not ported: the VR path (`init3DMode` 0x140253f10, `preRender3D` 0x1402547c0, `render3D`
//! 0x140254900, `renderVPointer` 0x140254c30). [`Gui::render_target_3d`] is its hook: while it
//! is set `render` draws nothing, as in the game.

use std::collections::BTreeMap;

use rustyac_physics::data::ini::IniReader;

use crate::control::{Clicked, Control, Id, Kind, MouseEvent};
use crate::font::{Font, FontType};
use crate::painter::{Painter, BLEND_ALPHA, DEPTH_OFF};

/// `ksgui::FormData`: where a form is on one virtual desktop.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FormPlace {
    pub x: f32,
    pub y: f32,
    pub visible: bool,
    pub blocked: bool,
    pub scale: f32,
}

impl Default for FormPlace {
    fn default() -> FormPlace {
        FormPlace { x: 0.0, y: 80.0, visible: false, blocked: false, scale: 1.0 }
    }
}

/// The files the GUI reads when it is made.
pub struct GuiFiles {
    /// `content/gui/skin.ini`
    pub skin: IniReader,
    /// `cfg/acos.ini` (the form layout): rustyAC's own, else the game's as a start, else none
    pub acos: Option<IniReader>,
    /// `cfg/gameplay.ini`: `[GUI] ALLOW_OVERLAPPING_FORMS`
    pub gameplay: Option<IniReader>,
    /// `system/cfg/assetto_corsa.ini`: `[AC_APPS] SCALE_STEP`
    pub assetto_corsa: Option<IniReader>,
}

/// What a class of the game overrides is looked up by this.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Class {
    Control,
    Label,
    Form,
    GameScreen,
    Taskbar,
    TaskBarIcon,
    Widget,
    Menu,
}

pub struct Gui {
    /// every control ever made, by [`Id`]
    pub controls: Vec<Control>,
    /// every `Font` object the widgets share or own
    pub fonts: Vec<Font>,
    pub is_active: bool,
    /// `GUI::font`: Segoe UI, 16
    pub font: usize,
    pub skin: IniReader,
    pub version: i32,
    pub taskbar: Id,
    /// `GUI::controls`: the top-level controls, drawn in this order
    pub roots: Vec<Id>,
    pub is_pointer_visible: bool,
    pub allow_form_serialization: bool,
    pub captured_form: Option<Id>,
    pub captured_pos: [f32; 2],
    pub old_good_pos: [f32; 2],
    pub move_app: bool,
    pub render_drawing_area: bool,
    pub allow_overlapping_forms: bool,
    /// the VR path's render target exists (never, here)
    pub render_target_3d: bool,
    pub mouse_pos: [f32; 2],
    pub mouse_speed: f32,
    pub old_mouse_pos: [f32; 2],
    pub virtual_desktop: i32,
    /// `formDataMap`: "DESK_<n>_FORM_<name>" to the form's place there
    pub form_data_map: BTreeMap<String, FormPlace>,
    pub acos: Option<IniReader>,
    pub assetto_corsa: Option<IniReader>,
    /// `GraphicsManager::videoSettings.width` / `height`, as of the last call with a painter
    pub screen: (i32, i32),
    /// every `evClicked` that fired, in order (for whoever listens from outside; the oracle
    /// compares it with the game's)
    pub fired: Vec<Clicked>,
    /// every event of a widget that fired, in order
    pub widget_events: Vec<crate::widgets::WidgetEvent>,
    /// `ksgui::Graph::localGL`: the drawer all graphs share, made by the first that draws
    pub graph_gl: Option<crate::painter::GlId>,
    /// `ksGetTime()`: milliseconds since the game started, as whoever runs the GUI last set
    /// it (a text input's caret blinks by it)
    pub time_ms: f64,
    /// what the menus read of `Sim` and ask of it
    pub sim: crate::menus::SimLink,
    /// the strings of the setup editor
    pub locale: crate::locale::Locale,
    /// the car of the player as the setup editor sees it (none: there is no editor)
    pub setup_car: Option<Box<dyn crate::setup_car::SetupCar>>,
    pub setup: crate::setup_car::SetupLink,
    /// `SetupTab::fixedVector`: the spinners a fixed setup has set
    pub setup_fixed: Vec<Id>,
    /// what the apps read of `Sim` and ask of it
    pub apps: crate::apps::AppLink,
}

impl Gui {
    /// `GUI::GUI` 0x140251ac0.
    pub fn new(painter: &mut dyn Painter, files: GuiFiles) -> Gui {
        let mut gui = Gui {
            controls: Vec::new(),
            fonts: Vec::new(),
            is_active: true,
            font: 0,
            skin: files.skin,
            version: 1,
            taskbar: 0,
            roots: Vec::new(),
            is_pointer_visible: true,
            allow_form_serialization: true,
            captured_form: None,
            captured_pos: [0.0; 2],
            old_good_pos: [0.0; 2],
            move_app: false,
            render_drawing_area: false,
            allow_overlapping_forms: false,
            render_target_3d: false,
            mouse_pos: [0.0; 2],
            mouse_speed: 0.0,
            old_mouse_pos: [0.0; 2],
            virtual_desktop: 0,
            form_data_map: BTreeMap::new(),
            acos: files.acos,
            assetto_corsa: files.assetto_corsa,
            screen: painter.screen(),
            fired: Vec::new(),
            widget_events: Vec::new(),
            graph_gl: None,
            time_ms: 0.0,
            sim: crate::menus::SimLink::default(),
            locale: crate::locale::Locale::default(),
            setup_car: None,
            setup: crate::setup_car::SetupLink::default(),
            setup_fixed: Vec::new(),
            apps: crate::apps::AppLink::default(),
        };
        gui.fonts.push(Font::new(painter, "", FontType::Proportional, 16.0, false, false));
        gui.taskbar = gui.new_taskbar(painter);
        gui.version = 1;
        gui.virtual_desktop = 0;
        if let Some(acos) = &gui.acos {
            if acos.has_section("HEADER") {
                if acos.has_key("HEADER", "VERSION") {
                    gui.version = acos.get_int("HEADER", "VERSION").unwrap_or(0);
                }
                if acos.has_key("HEADER", "DESKTOP_SELECTED") {
                    let n = acos.get_int("HEADER", "DESKTOP_SELECTED").unwrap_or(0);
                    let count = gui.taskbar_data(gui.taskbar).desktops.len() as i32;
                    let mut v = count;
                    if n <= count {
                        v = n.max(1);
                    }
                    gui.virtual_desktop = v - 1;
                }
            }
        }
        gui.select_virtual_desktop(gui.virtual_desktop);
        gui.form_data_map.clear();
        if let Some(gameplay) = &files.gameplay {
            if gameplay.ready {
                gui.allow_overlapping_forms = gameplay.get_int("GUI", "ALLOW_OVERLAPPING_FORMS").unwrap_or(0) != 0;
            }
        }
        gui.old_mouse_pos = [-100.0, -100.0];
        gui
    }

    /// `GUI::getSkinColor` 0x140253e60: four numbers of the skin file times 1/255 (the float
    /// 0x3b808081).
    pub fn get_skin_color(&self, section: &str, key: &str) -> [f32; 4] {
        let v = self.skin.get_float4(section, key).unwrap_or([0.0; 4]);
        let k = f32::from_bits(0x3b80_8081);
        [v[0] * k, v[1] * k, v[2] * k, v[3] * k]
    }

    pub fn class(&self, id: Id) -> Class {
        match &self.controls[id].kind {
            Kind::Control => Class::Control,
            Kind::Label { .. } => Class::Label,
            Kind::Form(_) => Class::Form,
            Kind::GameScreen(_) => Class::GameScreen,
            Kind::Taskbar(_) => Class::Taskbar,
            Kind::TaskBarIcon(_) => Class::TaskBarIcon,
            Kind::Widget(_) => Class::Widget,
            Kind::Menu(_) => Class::Menu,
        }
    }

    /// `GUI::addControl` 0x140252e50: a top-level control; a form also gets its icon on the
    /// app bar.
    pub fn add_root(&mut self, painter: &mut dyn Painter, control: Id) {
        self.roots.push(control);
        if self.class(control) == Class::Form {
            self.taskbar_add_form(painter, control);
        }
    }

    // ----- the virtual functions of ksgui::Control, by class -----

    /// vtable +0x08.
    pub fn hit_test(&self, id: Id, x: i32, y: i32) -> bool {
        match self.class(id) {
            Class::Form => self.form_hit_test(id, x, y),
            _ => self.control_hit_test(id, x, y),
        }
    }

    /// vtable +0x10.
    pub fn add_control(&mut self, parent: Id, child: Id) {
        self.control_add_control(parent, child);
    }

    /// vtable +0x18.
    pub fn render(&mut self, id: Id, dt: f32, painter: &mut dyn Painter) {
        match self.class(id) {
            Class::Label => {
                let max_chars = match self.controls[id].kind {
                    Kind::Label { max_chars } => max_chars,
                    _ => 0,
                };
                self.label_render(id, dt, painter, max_chars)
            }
            Class::GameScreen => self.game_screen_render(id, dt, painter),
            Class::Taskbar => self.taskbar_render(id, dt, painter),
            Class::TaskBarIcon => self.icon_render(id, dt, painter),
            Class::Widget => self.widget_render(id, dt, painter),
            Class::Menu => self.menu_render(id, dt, painter),
            Class::Form => {
                if self.with_app(id, |app, gui| app.render(gui, id, dt, painter)).is_none() {
                    self.control_render(id, dt, painter)
                }
            }
            Class::Control => self.control_render(id, dt, painter),
        }
    }

    /// vtable +0x38.
    pub fn set_size(&mut self, id: Id, w: f32, h: f32) {
        match self.class(id) {
            Class::Form => self.form_set_size(id, w, h),
            Class::Widget => self.widget_set_size(id, w, h),
            _ => self.control_set_size(id, w, h),
        }
    }

    /// vtable +0x40.
    pub fn on_visible_changed(&mut self, id: Id, visible: bool) {
        match self.class(id) {
            Class::Form => {
                if self.with_app(id, |app, gui| app.on_visible_changed(gui, id, visible)).is_none() {
                    self.form_on_visible_changed(id)
                }
            }
            Class::Widget => self.widget_on_visible_changed(id, visible),
            Class::Menu => self.menu_on_visible_changed(id, visible),
            _ => self.control_on_visible_changed(id, visible),
        }
    }

    /// vtable +0x50.
    pub fn on_mouse_down(&mut self, id: Id, e: &MouseEvent) -> bool {
        match self.class(id) {
            Class::GameScreen if !self.game_screen_data(id).app_interaction_enabled => false,
            Class::Form if self.is_app(id) => self.with_app(id, |app, gui| app.on_mouse_down(gui, id, e)).unwrap_or(false),
            Class::Widget => self.widget_on_mouse_down(id, e),
            Class::Menu => self.menu_on_mouse_down(id, e),
            _ => self.control_on_mouse_down(id, e),
        }
    }

    /// vtable +0x58.
    pub fn on_mouse_up(&mut self, id: Id, e: &MouseEvent) {
        match self.class(id) {
            Class::GameScreen if !self.game_screen_data(id).app_interaction_enabled => {}
            Class::Form if self.is_app(id) => {
                self.with_app(id, |app, gui| app.on_mouse_up(gui, id, e));
            }
            Class::Widget => self.widget_on_mouse_up(id, e),
            _ => self.control_on_mouse_up(id, e),
        }
    }

    /// vtable +0x60.
    pub fn on_mouse_move(&mut self, id: Id, e: &MouseEvent) {
        match self.class(id) {
            Class::GameScreen if !self.game_screen_data(id).app_interaction_enabled => {}
            Class::Form => {
                if self.with_app(id, |app, gui| app.on_mouse_move(gui, id, e)).is_none() {
                    self.form_on_mouse_move(id, e)
                }
            }
            Class::Taskbar => self.taskbar_on_mouse_move(id, e),
            Class::TaskBarIcon => self.icon_on_mouse_move(id, e),
            Class::Widget => self.widget_on_mouse_move(id, e),
            Class::Menu => self.menu_on_mouse_move(id, e),
            _ => self.control_on_mouse_move(id, e),
        }
    }

    /// vtable +0x68.
    pub fn on_mouse_wheel(&mut self, id: Id, e: &MouseEvent) {
        match self.class(id) {
            Class::GameScreen if !self.game_screen_data(id).app_interaction_enabled => {}
            Class::Form if self.is_app(id) => {
                self.with_app(id, |app, gui| app.on_mouse_wheel(gui, id, e));
            }
            Class::Widget => self.widget_on_mouse_wheel(id, e),
            _ => self.control_on_mouse_wheel(id, e),
        }
    }

    /// vtable +0x70.
    pub fn on_key_char(&mut self, id: Id, key: u32) {
        match self.class(id) {
            Class::Widget => self.widget_on_key_char(id, key),
            _ => self.control_on_key_char(id, key),
        }
    }

    /// vtable +0x78.
    pub fn on_key_down(&mut self, id: Id, key: u32) {
        match self.class(id) {
            Class::Widget => self.widget_on_key_down(id, key),
            _ => self.control_on_key_down(id, key),
        }
    }

    /// vtable +0x80.
    pub fn set_repeat_interval(&mut self, id: Id, v: f32) {
        match self.class(id) {
            Class::Widget => self.widget_set_repeat_interval(id, v),
            _ => self.controls[id].repeat_interval = v,
        }
    }

    /// vtable +0x88 (`ACControl::setText` 0x1401eb190 for most classes).
    pub fn set_text(&mut self, id: Id, text: &str) {
        match self.class(id) {
            Class::Widget => self.widget_set_text(id, text),
            _ => self.controls[id].text = text.to_string(),
        }
    }

    /// vtable +0x98.
    pub fn scale_by_mult(&mut self, id: Id, m: f32) {
        match self.class(id) {
            Class::Form => {
                if self.with_app(id, |app, gui| app.scale_by_mult(gui, id, m)).is_none() {
                    self.form_scale_by_mult(id, m)
                }
            }
            Class::Widget => self.widget_scale_by_mult(id, m),
            _ => self.control_scale_by_mult(id, m),
        }
    }

    // ----- drawing -----

    /// `GUI::render` 0x140254c10 and `doRender2D` 0x1402539d0: the states of the 2D pass, the
    /// top-level controls in order, the app bar on top.
    pub fn render_all(&mut self, dt: f32, painter: &mut dyn Painter) {
        self.screen = painter.screen();
        if !self.is_active || self.render_target_3d {
            return;
        }
        painter.reset_render_states();
        painter.set_depth_mode(DEPTH_OFF);
        painter.set_blend_mode(BLEND_ALPHA);
        for id in self.roots.clone() {
            if self.controls[id].visible {
                self.render(id, dt, painter);
            }
        }
        if self.controls[self.taskbar].visible {
            self.render(self.taskbar, dt, painter);
        }
    }

    // ----- input -----

    /// `GUI::setCapturedForm` 0x1402556c0: the form the mouse holds, and where it holds it.
    pub fn set_captured_form(&mut self, form: Id, local_x: i32, local_y: i32) {
        self.captured_form = Some(form);
        let r = self.controls[form].rect;
        self.old_good_pos = [r.left, r.top];
        self.captured_pos = [local_x as f32, local_y as f32];
    }

    /// `GUI::onMouseDown` 0x140254290.
    pub fn mouse_down(&mut self, e: &MouseEvent) {
        self.captured_form = None;
        self.on_mouse_down(self.taskbar, e);
        for id in self.roots.clone() {
            self.on_mouse_down(id, e);
        }
    }

    /// `ksgui::isPointInside` 0x140254190.
    fn is_point_inside(&self, x: f32, y: f32, form: Id) -> bool {
        let r = &self.controls[form].rect;
        x > r.left && x < (r.right - r.left) + r.left && y > r.top && y < (r.bottom - r.top) + r.top
    }

    /// `ksgui::isFormInside` 0x140254070: a corner of `a` is inside `b`.
    fn is_form_inside(&self, a: Id, b: Id) -> bool {
        let r = &self.controls[a].rect;
        let (left, top) = (r.left, r.top);
        let right = (r.right - left) + left;
        let bottom = (r.bottom - top) + top;
        self.is_point_inside(left, top, b) || self.is_point_inside(right, top, b) || self.is_point_inside(right, bottom, b) || self.is_point_inside(left, bottom, b)
    }

    /// `GUI::onMouseMove` 0x140254320: a held form follows the mouse (it is red, and goes back
    /// when let go, where it would lie over another form), then everyone hears of the move.
    pub fn mouse_move(&mut self, e: &MouseEvent) {
        if let Some(captured) = self.captured_form {
            if self.form_data(captured).blocked {
                return;
            }
            self.render_drawing_area = true;
            self.move_app = true;
            self.controls[captured].back_color = [0.0, 0.0, 0.0, 0.85];
            let nx = e.x as f32 - self.captured_pos[0];
            let mut ny = e.y as f32 - self.captured_pos[1];
            let r = self.controls[captured].rect;
            let max_y = self.screen.1 as f32 - (r.bottom - r.top);
            if ny > max_y {
                ny = max_y;
            } else if ny < 80.0 {
                ny = 80.0;
            }
            for f in self.taskbar_data(self.taskbar).forms.clone() {
                if !self.controls[f].visible || f == captured {
                    continue;
                }
                let c = self.controls[captured].rect;
                let over = self.is_form_inside(captured, f) || self.is_form_inside(f, captured) || self.is_point_inside((c.right - c.left) * 0.5 + c.left, (c.bottom - c.top) * 0.5 + c.top, f);
                if over && !self.allow_overlapping_forms {
                    self.controls[captured].back_color = [1.0, 0.0, 0.0, 0.5];
                    self.move_app = false;
                }
            }
            if self.move_app {
                self.old_good_pos = [nx, ny];
            }
            self.set_position(captured, nx, ny);
        }
        self.on_mouse_move(self.taskbar, e);
        for id in self.roots.clone() {
            self.on_mouse_move(id, e);
        }
        self.mouse_pos = [e.x as f32, e.y as f32];
    }

    /// `GUI::onMouseUp` 0x1402545c0.
    pub fn mouse_up(&mut self, e: &MouseEvent) {
        if let Some(captured) = self.captured_form {
            if !self.move_app {
                self.set_position(captured, self.old_good_pos[0], self.old_good_pos[1]);
            }
            self.controls[captured].back_color = self.get_skin_color("FORM", "BACK");
            self.render_drawing_area = false;
        }
        self.captured_form = None;
        self.on_mouse_up(self.taskbar, e);
        for id in self.roots.clone() {
            self.on_mouse_up(id, e);
        }
    }

    /// `GUI::onMouseWheelMoved` 0x140254720: the controls, then the app bar's list of icons.
    pub fn mouse_wheel(&mut self, e: &MouseEvent) {
        for id in self.roots.clone() {
            self.on_mouse_wheel(id, e);
        }
        self.taskbar_mouse_wheel_moved(e);
    }

    /// `GUI::onKeyDown` 0x140254230.
    pub fn key_down(&mut self, key: u32) {
        for id in self.roots.clone() {
            self.on_key_down(id, key);
        }
    }

    /// `GUI::onKeyChar` 0x1402541e0.
    pub fn key_char(&mut self, key: u32) {
        for id in self.roots.clone() {
            self.on_key_char(id, key);
        }
    }
}
