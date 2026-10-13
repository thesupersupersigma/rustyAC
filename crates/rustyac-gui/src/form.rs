// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! `ksgui::Form` (0x1b8 bytes): a window that can be moved, pinned and scaled (every "app" is
//! one), and `ksgui::GameScreen` (0x1b0 bytes), the full-screen container that holds them.
//!
//! `Form::Form` 0x140245890, `hitTest` 0x1402466a0, `onMouseMove` 0x140246720,
//! `onTitleClicked` 0x140246830, `onVisibleChanged` 0x140246860, `scaleByMult` 0x1402468f0,
//! `setAutoHideMode` 0x140246a30, `setBlocked` 0x140246ae0, `setIcon` 0x140246b10,
//! `setIconVisible` 0x140246cc0, `setSize` 0x140246d60, `updateScaleByMult` 0x140246df0 and
//! the five handlers 0x140246490 … 0x140246540; `GameScreen::GameScreen` 0x140250bd0,
//! `addControl` 0x140250d10, `render` 0x140250de0, `showDesktop` 0x140250e70, `shutdown`
//! 0x1402510c0.
//!
//! A form draws nothing of its own (`Form::render` is `Control::render`): its "title bar" is
//! three children, the title text, the logo and the pin button, plus the two zoom buttons of
//! a form that can be scaled. Pin and zoom show only while the mouse is over the form.

use std::rc::Rc;

use crate::control::{Clicked, Id, Kind, MouseEvent};
use crate::gui::{Class, Gui};
use crate::painter::{Painter, ALIGN_CENTER};

pub struct FormData {
    pub form_title: Option<Id>,
    /// a developer's app: no icon on the app bar unless `ENABLE_DEV_APPS` is set
    pub dev_app: bool,
    pub autohide: bool,
    /// pinned: it cannot be dragged
    pub blocked: bool,
    pub scale_to_be_saved: f32,
    pub can_be_scaled: bool,
    pub icon: Option<Id>,
    pub pin_icon: Option<Id>,
    pub zoom_in_icon: Option<Id>,
    pub zoom_out_icon: Option<Id>,
    pub header_height: f32,
    pub scale_step: f32,
}

pub struct GameScreenData {
    pub app_interaction_enabled: bool,
    /// `evDesktopShown`
    pub ev_desktop_shown: Vec<Rc<dyn Fn(&mut Gui, bool)>>,
    /// places in the screen's list of the forms "show desktop" has hidden
    pub old_apps_opened: Vec<usize>,
}

impl Gui {
    pub fn form_data(&self, id: Id) -> &FormData {
        match &self.controls[id].kind {
            Kind::Form(data) => data,
            _ => panic!("control {id} is not a form"),
        }
    }

    pub fn form_data_mut(&mut self, id: Id) -> &mut FormData {
        match &mut self.controls[id].kind {
            Kind::Form(data) => data,
            _ => panic!("control {id} is not a form"),
        }
    }

    pub fn game_screen_data(&self, id: Id) -> &GameScreenData {
        match &self.controls[id].kind {
            Kind::GameScreen(data) => data,
            _ => panic!("control {id} is not a game screen"),
        }
    }

    /// `Form::Form(name, gui, canBeScaled)` 0x140245890.
    pub fn new_form(&mut self, painter: &mut dyn Painter, name: &str, can_be_scaled: bool) -> Id {
        let form = self.new_control(painter, name);
        self.controls[form].kind = Kind::Form(Box::new(FormData {
            form_title: None,
            dev_app: true,
            autohide: false,
            blocked: false,
            scale_to_be_saved: 1.0,
            can_be_scaled,
            icon: None,
            pin_icon: None,
            zoom_in_icon: None,
            zoom_out_icon: None,
            header_height: 32.0,
            scale_step: 0.25,
        }));
        self.controls[form].back_color = self.get_skin_color("FORM", "BACK");
        self.controls[form].border_color = self.get_skin_color("FORM", "BORDER");
        self.controls[form].fore_color = self.get_skin_color("FORM", "FORE");
        // a click on the form: it is held by the mouse
        self.controls[form].ev_clicked.push(Rc::new(move |gui: &mut Gui, ev: &Clicked| gui.form_on_title_clicked(form, ev)));

        let pin = self.new_active_button(painter, "PIN_ICON");
        self.set_text(pin, "");
        self.active_button_mut(pin).text_coord = [0.0, 32.0];
        self.set_multiple_texture(painter, pin, "content/gui/controls/form/pin_icn");
        self.set_size(pin, 26.0, 26.0);
        self.control_add_control(form, pin);
        self.form_data_mut(form).pin_icon = Some(pin);
        self.controls[pin].ev_clicked.push(Rc::new(move |gui: &mut Gui, _: &Clicked| {
            let blocked = !gui.form_data(form).blocked;
            gui.form_data_mut(form).blocked = blocked;
            gui.set_selected(pin, blocked);
        }));

        if can_be_scaled {
            if let Some(ini) = &self.assetto_corsa {
                if ini.ready && ini.has_key("AC_APPS", "SCALE_STEP") {
                    let step = ini.get_float("AC_APPS", "SCALE_STEP").unwrap_or(0.0);
                    self.form_data_mut(form).scale_step = step;
                }
            }
            let zoom_in = self.new_control(painter, &format!("ZOOM_IN_ICON_{name}"));
            self.controls[zoom_in].back_color = [0.0; 4];
            self.controls[zoom_in].draw_border = false;
            self.controls[zoom_in].back_texture = painter.get_texture("content/gui/controls/form/zoomIn.png");
            self.control_add_control(form, zoom_in);
            let zoom_out = self.new_control(painter, &format!("ZOOM_OUT_ICON_{name}"));
            self.controls[zoom_out].back_color = [0.0; 4];
            self.controls[zoom_out].draw_border = false;
            self.controls[zoom_out].back_texture = painter.get_texture("content/gui/controls/form/zoomOut.png");
            self.control_add_control(form, zoom_out);
            self.form_data_mut(form).zoom_in_icon = Some(zoom_in);
            self.form_data_mut(form).zoom_out_icon = Some(zoom_out);
            self.controls[zoom_in].ev_clicked.push(Rc::new(move |gui: &mut Gui, _: &Clicked| {
                if gui.controls[form].scale_mult < 2.0 {
                    let m = gui.controls[form].scale_mult + gui.form_data(form).scale_step;
                    gui.controls[form].scale_mult = m;
                    gui.form_data_mut(form).scale_to_be_saved = m;
                    gui.form_scale_by_mult(form, m);
                }
            }));
            self.controls[zoom_out].ev_clicked.push(Rc::new(move |gui: &mut Gui, _: &Clicked| {
                if 0.5 < gui.controls[form].scale_mult {
                    let m = gui.controls[form].scale_mult - gui.form_data(form).scale_step;
                    gui.controls[form].scale_mult = m;
                    gui.form_data_mut(form).scale_to_be_saved = m;
                    gui.form_scale_by_mult(form, m);
                }
            }));
        }

        let title = self.new_control(painter, name);
        self.set_text(title, "");
        self.controls[title].draw_background = false;
        self.controls[title].draw_border = false;
        self.controls[title].font_align = ALIGN_CENTER;
        self.set_position(title, 0.0, 4.0);
        self.control_add_control(form, title);
        self.form_data_mut(form).form_title = Some(title);

        let icon = self.new_control(painter, "TITLE_ICON");
        self.set_position(icon, 1.0, 1.0);
        self.set_size(icon, 40.0, 31.0);
        self.controls[icon].back_color = [0.0; 4];
        self.controls[icon].draw_border = false;
        self.controls[icon].back_texture = painter.get_texture("content/gui/inapp_aclogo.png");
        self.control_add_control(form, icon);
        self.form_data_mut(form).icon = Some(icon);
        // a click on the logo closes the form
        self.controls[icon].ev_clicked.push(Rc::new(move |gui: &mut Gui, _: &Clicked| {
            gui.controls[form].visible = false;
            gui.on_visible_changed(form, false);
        }));

        self.set_position(form, 0.0, 80.0);
        self.deserialize_form(form);
        form
    }

    /// `Form::hitTest` 0x1402466a0: the form and the strip of the pin button's height above it.
    pub fn form_hit_test(&self, id: Id, x: i32, y: i32) -> bool {
        let r = &self.controls[id].rect;
        if x < 0 || x as f32 > r.right - r.left {
            return false;
        }
        let lower = match self.form_data(id).pin_icon {
            Some(pin) => -(self.controls[pin].rect.bottom - self.controls[pin].rect.top),
            None => 0.0,
        };
        if (y as f32) < lower {
            return false;
        }
        y as f32 <= r.bottom - r.top
    }

    /// `Form::onMouseMove` 0x140246720.
    pub fn form_on_mouse_move(&mut self, id: Id, e: &MouseEvent) {
        let r = self.controls[id].rect;
        let (lx, ly) = (e.x - r.left as i32, e.y - r.top as i32);
        let hit = self.hit_test(id, lx, ly);
        let (autohide, blocked, can_be_scaled, title, icon, pin, zoom_in, zoom_out) = {
            let d = self.form_data(id);
            (d.autohide, d.blocked, d.can_be_scaled, d.form_title, d.icon, d.pin_icon, d.zoom_in_icon, d.zoom_out_icon)
        };
        if autohide && blocked {
            self.controls[id].draw_border = hit;
            self.controls[id].draw_background = hit;
            if let Some(title) = title {
                self.set_visible(title, hit);
            }
            if let Some(icon) = icon {
                self.set_visible(icon, hit);
            }
        }
        if can_be_scaled {
            if let (Some(zoom_in), Some(zoom_out)) = (zoom_in, zoom_out) {
                self.set_visible(zoom_in, hit);
                self.set_visible(zoom_out, hit);
                if blocked {
                    self.set_visible(zoom_in, false);
                    self.set_visible(zoom_out, false);
                }
            }
        }
        if let Some(pin) = pin {
            self.set_visible(pin, hit);
        }
        for child in self.controls[id].controls.clone() {
            self.on_mouse_move(child, e);
        }
    }

    /// `Form::onTitleClicked` 0x140246830.
    pub fn form_on_title_clicked(&mut self, id: Id, ev: &Clicked) {
        if ev.control == id && self.controls[id].visible {
            self.set_captured_form(id, ev.local_x, ev.local_y);
        }
    }

    /// `Form::onVisibleChanged` 0x140246860: back inside the screen.
    pub fn form_on_visible_changed(&mut self, id: Id) {
        let (w, h) = (self.screen.0 as f32, self.screen.1 as f32);
        let r = self.controls[id].rect;
        if r.right > w {
            self.set_position(id, w - (r.right - r.left), r.top);
        }
        let r = self.controls[id].rect;
        if r.bottom > h {
            self.set_position(id, r.left, h - (r.bottom - r.top));
        }
    }

    /// `Form::scaleByMult(float)` 0x1402468f0.
    pub fn form_scale_by_mult(&mut self, id: Id, m: f32) {
        self.control_scale_by_mult(id, m);
        let (can_be_scaled, title, pin, zoom_in, zoom_out) = {
            let d = self.form_data(id);
            (d.can_be_scaled, d.form_title, d.pin_icon, d.zoom_in_icon, d.zoom_out_icon)
        };
        if !can_be_scaled {
            return;
        }
        let (Some(title), Some(pin), Some(zoom_in), Some(zoom_out)) = (title, pin, zoom_in, zoom_out) else { return };
        self.set_size(zoom_in, 26.0, 26.0);
        self.set_size(zoom_out, 26.0, 26.0);
        self.set_size(pin, 26.0, 26.0);
        let r = self.controls[id].rect;
        self.set_size(title, r.right - r.left, 26.0);
        let p = self.controls[pin].rect;
        let y = -1.0 - (p.bottom - p.top);
        let r = self.controls[id].rect;
        let mut x = ((r.right - r.left) - 1.0) - (p.right - p.left);
        self.set_position(pin, x, y);
        let z = self.controls[zoom_out].rect;
        x -= z.right - z.left;
        self.set_position(zoom_out, x, y);
        let z = self.controls[zoom_in].rect;
        x -= z.right - z.left;
        self.set_position(zoom_in, x, y);
    }

    /// `Form::setSize` 0x140246d60.
    pub fn form_set_size(&mut self, id: Id, w: f32, h: f32) {
        self.control_set_size(id, w, h);
        let (can_be_scaled, header, title, pin) = {
            let d = self.form_data(id);
            (d.can_be_scaled, d.header_height, d.form_title, d.pin_icon)
        };
        if !can_be_scaled {
            if let Some(pin) = pin {
                let r = self.controls[id].rect;
                self.set_position(pin, (r.right - r.left) - (header + 1.0), 1.0);
            }
        }
        if let Some(title) = title {
            let r = self.controls[id].rect;
            let scale = self.controls[id].scale_mult;
            self.set_size(title, r.right - r.left, (header - 1.0) * scale);
        }
    }

    /// `Form::updateScaleByMult` 0x140246df0.
    pub fn update_scale_by_mult(&mut self, id: Id) {
        if self.form_data(id).can_be_scaled {
            let m = self.controls[id].scale_mult;
            self.form_data_mut(id).scale_to_be_saved = m;
            self.scale_by_mult(id, m);
        }
    }

    /// `Form::setBlocked` 0x140246ae0.
    pub fn set_blocked(&mut self, id: Id, blocked: bool) {
        if let Some(pin) = self.form_data(id).pin_icon {
            self.set_selected(pin, blocked);
        }
        self.form_data_mut(id).blocked = blocked;
    }

    /// `Form::setAutoHideMode` 0x140246a30: frame, title and buttons only under the mouse.
    pub fn set_auto_hide_mode(&mut self, id: Id, on: bool) {
        self.form_data_mut(id).autohide = on;
        self.controls[id].draw_border = !on;
        self.controls[id].draw_background = !on;
        let (blocked, can_be_scaled, title, icon, pin, zoom_in, zoom_out) = {
            let d = self.form_data(id);
            (d.blocked, d.can_be_scaled, d.form_title, d.icon, d.pin_icon, d.zoom_in_icon, d.zoom_out_icon)
        };
        for control in [title, icon, pin].into_iter().flatten() {
            self.set_visible(control, !on);
        }
        if can_be_scaled {
            if let (Some(zoom_in), Some(zoom_out)) = (zoom_in, zoom_out) {
                self.set_visible(zoom_in, !on);
                self.set_visible(zoom_out, !on);
                if blocked {
                    self.set_visible(zoom_in, false);
                    self.set_visible(zoom_out, false);
                }
            }
        }
    }

    /// `Form::setIcon` 0x140246b10.
    pub fn form_set_icon(&mut self, painter: &mut dyn Painter, id: Id, path: &str) {
        let texture = if painter.file_exists(path) { painter.get_texture(path) } else { painter.get_texture("content/gui/inapp_aclogo.png") };
        if let Some(icon) = self.form_data(id).icon {
            self.controls[icon].back_texture = texture;
        }
    }

    /// `Form::setIconVisible` 0x140246cc0.
    pub fn form_set_icon_visible(&mut self, id: Id, visible: bool) {
        let (blocked, can_be_scaled, icon, pin, zoom_in, zoom_out) = {
            let d = self.form_data(id);
            (d.blocked, d.can_be_scaled, d.icon, d.pin_icon, d.zoom_in_icon, d.zoom_out_icon)
        };
        for control in [icon, pin].into_iter().flatten() {
            self.set_visible(control, visible);
        }
        if can_be_scaled {
            if let (Some(zoom_in), Some(zoom_out)) = (zoom_in, zoom_out) {
                self.set_visible(zoom_in, visible);
                self.set_visible(zoom_out, visible);
                if blocked {
                    self.set_visible(zoom_in, false);
                    self.set_visible(zoom_out, false);
                }
            }
        }
    }

    /// The text of the title bar (`formTitle->setText`).
    pub fn form_set_title(&mut self, id: Id, title: &str) {
        if let Some(control) = self.form_data(id).form_title {
            self.set_text(control, title);
        }
    }

    // ----- ksgui::GameScreen -----

    /// `GameScreen::GameScreen` 0x140250bd0.
    pub fn new_game_screen(&mut self, painter: &mut dyn Painter, name: &str) -> Id {
        let id = self.new_control(painter, name);
        self.controls[id].kind = Kind::GameScreen(Box::new(GameScreenData { app_interaction_enabled: true, ev_desktop_shown: Vec::new(), old_apps_opened: Vec::new() }));
        id
    }

    /// `GameScreen::addControl(control, showInTaskbar)` 0x140250d10.
    pub fn game_screen_add_control(&mut self, painter: &mut dyn Painter, screen: Id, control: Id, show_in_taskbar: bool) {
        self.control_add_control(screen, control);
        if self.class(control) == Class::Form && show_in_taskbar {
            self.taskbar_add_form(painter, control);
        }
    }

    /// `GameScreen::render` 0x140250de0: its children, nothing of its own.
    pub fn game_screen_render(&mut self, id: Id, dt: f32, painter: &mut dyn Painter) {
        if !self.controls[id].visible {
            return;
        }
        for child in self.controls[id].controls.clone() {
            if self.controls[child].visible {
                self.render(child, dt, painter);
            }
        }
    }

    /// `GameScreen::showDesktop` 0x140250e70: hides every form that shows (but "HIDE_APPS");
    /// when none shows, the hidden ones come back.
    pub fn show_desktop(&mut self, screen: Id) {
        let n0 = self.game_screen_data(screen).old_apps_opened.len();
        let children = self.controls[screen].controls.clone();
        let mut list = self.game_screen_data(screen).old_apps_opened.clone();
        for (i, child) in children.iter().enumerate() {
            if self.class(*child) == Class::Form && self.controls[*child].visible && self.controls[*child].name != "HIDE_APPS" {
                list.push(i);
            }
        }
        if list.len() == n0 {
            for i in &list {
                self.set_visible(children[*i], true);
            }
            list.clear();
        } else {
            for i in &list {
                self.set_visible(children[*i], false);
            }
        }
        let handlers = match &mut self.controls[screen].kind {
            Kind::GameScreen(data) => {
                data.old_apps_opened = list;
                data.ev_desktop_shown.clone()
            }
            _ => Vec::new(),
        };
        for handler in handlers {
            handler(self, true);
        }
    }

    /// `GameScreen::shutdown` 0x1402510c0: the hidden forms show again (so that the layout file
    /// gets them as they were).
    pub fn game_screen_shutdown(&mut self, screen: Id) {
        let children = self.controls[screen].controls.clone();
        for i in self.game_screen_data(screen).old_apps_opened.clone() {
            self.set_visible(children[i], true);
        }
    }
}
