// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! `ksgui::Taskbar` (0x268 bytes), the app bar on the right edge of the screen with its four
//! virtual desktops, and `ksgui::TaskBarIcon` (0x218 bytes), one icon of it.
//!
//! `Taskbar::Taskbar` 0x140258a20, `addDesktop` 0x1402599f0, `addForm` 0x140259c10,
//! `cycleDekstop` 0x140259df0, `mouseWheelMoved` 0x140259eb0, `onMouseMove` 0x140259f70,
//! `render` 0x14025a050, `selectVirtualDesktop` 0x14025a2b0, `updateAppIconsPosition`
//! 0x14025a360 and its four handlers; `TaskBarIcon::TaskBarIcon` 0x14025c1e0, `hide`
//! 0x14025c720, `onMouseMove` 0x14025c7c0, `render` 0x14025c8d0, `setDescriptionText`
//! 0x14025ca40, `setIcon` 0x14025cb60.
//!
//! The bar is 64 pixels wide and slides in at 250 pixels a second while the mouse is in the
//! rightmost 64 pixels of the screen. An icon shows its form's title in a box that slides out
//! to its left at 500 pixels a second.

use std::rc::Rc;

use crate::control::{Clicked, Id, Kind, MouseEvent};
use crate::font::{Font, FontType};
use crate::gui::Gui;
use crate::painter::{Painter, Rect, Texture, ALIGN_CENTER};

pub struct TaskbarData {
    pub target_x: i32,
    pub task_bar_full_showed: bool,
    /// `evAppOpened` (true) / `evAppDismiss` (false) as they fired
    pub app_events: Vec<(bool, Clicked)>,
    pub forms: Vec<Id>,
    pub top_scroller: Id,
    pub bottom_scroller: Id,
    pub desktops: Vec<Id>,
    pub target_out_of_sight: f32,
    pub target_full_show: f32,
    pub top_app_index: i32,
    /// `appFont`: Consolas, 18
    pub app_font: usize,
    pub icons: Vec<Id>,
    pub virtual_desktop: i16,
}

pub struct IconData {
    /// 1: its form is hidden; 0: it shows
    pub status: i32,
    pub is_over: bool,
    pub texture: Texture,
    pub texture_on: Texture,
    pub description_rect: Id,
    pub description_label: Id,
    pub target_x: f32,
    /// `tag`: the form
    pub form: Option<Id>,
}

impl Gui {
    pub fn taskbar_data(&self, id: Id) -> &TaskbarData {
        match &self.controls[id].kind {
            Kind::Taskbar(data) => data,
            _ => panic!("control {id} is not the taskbar"),
        }
    }

    pub fn taskbar_data_mut(&mut self, id: Id) -> &mut TaskbarData {
        match &mut self.controls[id].kind {
            Kind::Taskbar(data) => data,
            _ => panic!("control {id} is not the taskbar"),
        }
    }

    pub fn icon_data(&self, id: Id) -> &IconData {
        match &self.controls[id].kind {
            Kind::TaskBarIcon(data) => data,
            _ => panic!("control {id} is not a taskbar icon"),
        }
    }

    pub fn icon_data_mut(&mut self, id: Id) -> &mut IconData {
        match &mut self.controls[id].kind {
            Kind::TaskBarIcon(data) => data,
            _ => panic!("control {id} is not a taskbar icon"),
        }
    }

    /// `Taskbar::Taskbar` 0x140258a20.
    pub fn new_taskbar(&mut self, painter: &mut dyn Painter) -> Id {
        let id = self.new_control(painter, "TASKBAR");
        let width = self.screen.0;
        let out = (width + 10) as f32;
        let full = (width - 64) as f32;
        self.controls[id].kind = Kind::Taskbar(Box::new(TaskbarData {
            target_x: out as i32,
            task_bar_full_showed: false,
            app_events: Vec::new(),
            forms: Vec::new(),
            top_scroller: 0,
            bottom_scroller: 0,
            desktops: Vec::new(),
            target_out_of_sight: out,
            target_full_show: full,
            top_app_index: 0,
            app_font: 0,
            icons: Vec::new(),
            virtual_desktop: 0,
        }));
        self.set_position(id, out as i32 as f32, 0.0);
        self.set_size(id, 68.0, 68.0);
        let mut font = Font::new(painter, "", FontType::Monospaced, 18.0, false, false);
        font.set_color(1.0, 0.0, 0.0, 1.0);
        self.fonts.push(font);
        self.taskbar_data_mut(id).app_font = self.fonts.len() - 1;
        self.controls[id].border_color = [1.0, 0.0, 0.0, 0.8];
        self.controls[id].back_color = [0.0, 0.0, 0.0, 0.5];

        let top = self.new_control(painter, "TOP");
        self.controls[top].draw_background = false;
        self.controls[top].draw_border = false;
        self.controls[top].back_texture = painter.get_texture("content/gui/controls/taskbar/scroll_up.png");
        self.add_control(id, top);
        self.set_position(top, 2.0, 2.0);
        self.set_size(top, 61.0, 24.0);
        self.taskbar_data_mut(id).top_scroller = top;

        for (index, x) in [(0, 2), (1, 18), (2, 33), (3, 48)] {
            let button = self.taskbar_add_desktop(painter, id, index, x, 0);
            self.taskbar_data_mut(id).desktops.push(button);
        }
        // (the GUI is not there yet: its own list of desktops is this one)
        self.taskbar_select_virtual_desktop(id, 0);

        let bottom = self.new_control(painter, "TOP");
        self.controls[bottom].draw_background = false;
        self.controls[bottom].draw_border = false;
        self.controls[bottom].back_texture = painter.get_texture("content/gui/controls/taskbar/scroll_down.png");
        self.add_control(id, bottom);
        self.set_position(bottom, 2.0, 2.0);
        self.set_size(bottom, 61.0, 24.0);
        self.taskbar_data_mut(id).bottom_scroller = bottom;

        self.controls[top].ev_clicked.push(Rc::new(move |gui: &mut Gui, _: &Clicked| gui.taskbar_scroll(id, -1, true)));
        self.controls[bottom].ev_clicked.push(Rc::new(move |gui: &mut Gui, _: &Clicked| gui.taskbar_scroll(id, 1, true)));
        id
    }

    /// The scroll buttons' handlers (0x140259410 up, 0x140259370 down) and the wheel: the first
    /// icon shown moves by one, and stays between 0 and three before the last.
    fn taskbar_scroll(&mut self, id: Id, by: i32, hide: bool) {
        let data = self.taskbar_data_mut(id);
        let v = data.top_app_index + by;
        let n = data.icons.len() as i32 - 3;
        data.top_app_index = if v > n { n } else { v.max(0) };
        self.update_app_icons_position(id);
        if hide {
            for icon in self.taskbar_data(id).icons.clone() {
                self.icon_hide(icon, true);
            }
        }
    }

    /// `Taskbar::addDesktop` 0x1402599f0: one of the four small buttons.
    fn taskbar_add_desktop(&mut self, painter: &mut dyn Painter, id: Id, index: i32, x: i32, y: i32) -> Id {
        let button = self.new_control(painter, &index.to_string());
        self.controls[button].draw_border = true;
        self.controls[button].draw_background = false;
        self.set_size(button, 15.0, 15.0);
        self.set_position(button, x as f32, y as f32);
        self.set_text(button, &(index + 1).to_string());
        self.controls[button].font_align = ALIGN_CENTER;
        self.fonts.push(Font::new(painter, "", FontType::Proportional, 9.0, false, false));
        self.controls[button].font = Some(self.fonts.len() - 1);
        self.add_control(id, button);
        self.controls[button].ev_clicked.push(Rc::new(move |gui: &mut Gui, _: &Clicked| {
            gui.on_desktop_change(index);
            gui.taskbar_select_virtual_desktop(id, index);
        }));
        button
    }

    /// `Taskbar::addForm` 0x140259c10: an icon for a form.
    pub fn taskbar_add_form(&mut self, painter: &mut dyn Painter, form: Id) {
        let id = self.taskbar;
        self.taskbar_data_mut(id).forms.push(form);
        let name = self.controls[form].name.clone();
        let font = self.taskbar_data(id).app_font;
        let icon = self.new_taskbar_icon(painter, &name, font);
        let title = match self.form_data(form).form_title {
            Some(title) => self.controls[title].text.clone(),
            None => String::new(),
        };
        self.icon_set_description_text(icon, &title);
        self.icon_data_mut(icon).form = Some(form);
        self.controls[icon].ev_clicked.push(Rc::new(move |gui: &mut Gui, ev: &Clicked| {
            let Some(form) = gui.icon_data(ev.control).form else { return };
            let showing = gui.controls[form].visible;
            gui.taskbar_data_mut(id).app_events.push((!showing, *ev));
            gui.set_visible(form, !showing);
        }));
        self.taskbar_data_mut(id).icons.push(icon);
        self.add_control(id, icon);
    }

    /// `Taskbar::selectVirtualDesktop` 0x14025a2b0: the chosen button is red. (The forms are
    /// moved by the GUI's handler of the bar's event, [`Gui::on_desktop_change`].)
    pub fn taskbar_select_virtual_desktop(&mut self, id: Id, index: i32) {
        for (i, desktop) in self.taskbar_data(id).desktops.clone().into_iter().enumerate() {
            let color = if i as i32 == index { [1.0, 0.0, 0.0, 1.0] } else { [1.0; 4] };
            self.controls[desktop].fore_color = color;
            self.controls[desktop].border_color = color;
        }
        self.taskbar_data_mut(id).virtual_desktop = index as i16;
    }

    pub fn select_virtual_desktop(&mut self, index: i32) {
        self.taskbar_select_virtual_desktop(self.taskbar, index);
    }

    /// `Taskbar::cycleDekstop` 0x140259df0: the next desktop (the key).
    pub fn cycle_desktop(&mut self) -> i16 {
        let id = self.taskbar;
        let data = self.taskbar_data_mut(id);
        data.virtual_desktop = data.virtual_desktop.wrapping_add(1);
        if data.virtual_desktop as i64 as u64 >= data.desktops.len() as u64 {
            data.virtual_desktop = 0;
        }
        let now = data.virtual_desktop;
        self.on_desktop_change(now as i32);
        self.taskbar_select_virtual_desktop(id, now as i32);
        now
    }

    /// `Taskbar::onMouseMove` 0x140259f70: the mouse in the rightmost 64 pixels calls the bar.
    pub fn taskbar_on_mouse_move(&mut self, id: Id, e: &MouseEvent) {
        self.control_on_mouse_move(id, e);
        let data = self.taskbar_data_mut(id);
        let full = data.target_full_show;
        data.target_x = if e.x as f32 > full { full as i32 } else { data.target_out_of_sight as i32 };
    }

    /// `Taskbar::mouseWheelMoved` 0x140259eb0.
    pub fn taskbar_mouse_wheel_moved(&mut self, e: &MouseEvent) {
        let id = self.taskbar;
        if !self.controls[id].visible {
            return;
        }
        let left = self.controls[id].rect.left;
        if !(left < self.screen.0 as f32) || !(e.x as f32 > left) {
            return;
        }
        self.taskbar_scroll(id, if 0.0 >= e.delta { 1 } else { -1 }, false);
        self.on_mouse_move(id, &MouseEvent { x: e.x, y: e.y, button: -1, delta: 0.0 });
    }

    /// `Taskbar::render` 0x14025a050.
    pub fn taskbar_render(&mut self, id: Id, dt: f32, painter: &mut dyn Painter) {
        let (width, height) = self.screen;
        let full = (width - 64) as f32;
        let out = (width + 10) as f32;
        let data = self.taskbar_data_mut(id);
        data.target_full_show = full;
        data.target_out_of_sight = out;
        let tx = data.target_x as f32;
        if tx == out && out == self.controls[id].rect.left {
            return;
        }
        self.control_render(id, dt, painter);
        let step = dt * 250.0;
        let cur = self.controls[id].rect.left;
        let mut x = tx;
        if tx == full {
            let t = cur - step;
            if t >= full {
                x = t;
            } else {
                self.controls[id].rect.left = full;
            }
        }
        if tx == out {
            let t = step + cur;
            if t > out {
                self.controls[id].rect.left = out;
            } else {
                x = t;
            }
        }
        if full - cur > 100.0 {
            x = out;
        }
        self.set_position(id, x, 0.0);
        let left = self.controls[id].rect.left;
        if left == full {
            self.taskbar_data_mut(id).task_bar_full_showed = true;
        }
        if left == out {
            self.taskbar_data_mut(id).task_bar_full_showed = false;
        }
        let w = (width - left as i32 + 2) as f32;
        let h = height as f32 + 1.0;
        self.set_size(id, w, h);
        let bottom = self.controls[id].rect.bottom;
        let (bottom_scroller, desktops, icons) = {
            let d = self.taskbar_data(id);
            (d.bottom_scroller, d.desktops.clone(), d.icons.clone())
        };
        self.set_position(bottom_scroller, 2.0, bottom - 26.0);
        for desktop in desktops {
            let x = self.controls[desktop].rect.left;
            self.set_position(desktop, x, bottom - 41.0);
        }
        for icon in icons {
            if let Some(form) = self.icon_data(icon).form {
                let status = if self.controls[form].visible { 0 } else { 1 };
                self.icon_data_mut(icon).status = status;
            }
        }
        self.update_app_icons_position(id);
    }

    /// `Taskbar::updateAppIconsPosition` 0x14025a360: 64 pixels an icon, 4 between them, the
    /// first at 28; an icon shows when it is whole between the two scroll buttons.
    pub fn update_app_icons_position(&mut self, id: Id) {
        let (top_app_index, bottom_scroller, icons) = {
            let d = self.taskbar_data(id);
            (d.top_app_index, d.bottom_scroller, d.icons.clone())
        };
        let mut y = 28.0 - top_app_index as f32 * 68.0;
        for icon in icons {
            self.controls[icon].rect.top = y;
            self.controls[icon].rect.bottom = y + 64.0;
            let r = self.controls[icon].rect;
            let limit = self.controls[bottom_scroller].rect.top - 17.0;
            self.set_visible(icon, 0.0 < r.top && limit > r.bottom);
            y += 68.0;
        }
    }

    // ----- ksgui::TaskBarIcon -----

    /// `TaskBarIcon::TaskBarIcon(name, font, gui)` 0x14025c1e0.
    fn new_taskbar_icon(&mut self, painter: &mut dyn Painter, name: &str, font: usize) -> Id {
        let id = self.new_control(painter, &format!("{name}_ICON"));
        let (texture, texture_on) = self.icon_textures(painter, name, id);
        self.controls[id].font = Some(font);
        self.control_set_size(id, 64.0, 64.0);
        self.set_position(id, 0.0, 0.0);
        self.controls[id].draw_border = true;
        self.controls[id].draw_background = true;
        self.controls[id].back_color = [0.5, 0.5, 0.5, 1.0];
        self.controls[id].border_color = [0.5, 0.5, 0.5, 1.0];

        let rect = self.new_control(painter, &format!("{name}_DESCRIPTION"));
        self.controls[rect].draw_background = true;
        self.controls[rect].back_color = [0.0, 0.0, 0.0, 0.5];
        self.controls[rect].draw_border = true;
        self.controls[rect].border_color = [0.25, 0.25, 0.25, 0.5];
        self.set_size(rect, 150.0, 64.0);
        self.set_visible(rect, false);

        let label = self.new_label(painter, "INDICATOR_LABEL");
        self.controls[label].draw_background = false;
        self.controls[label].draw_border = false;
        self.controls[label].font = Some(font);
        self.set_position(label, 0.0, 19.0);
        let r = self.controls[rect].rect;
        self.set_size(label, r.right - r.left, (r.bottom - r.top) - 15.0);
        self.controls[label].font_align = ALIGN_CENTER;
        self.set_text(label, name);
        self.add_control(rect, label);

        self.controls[id].kind = Kind::TaskBarIcon(Box::new(IconData { status: 1, is_over: false, texture, texture_on, description_rect: rect, description_label: label, target_x: 0.0, form: None }));
        self.icon_fit_description(id, name);
        let mut world = self.controls[id].rect;
        self.get_world_rect(id, &mut world);
        let target = world.left - 68.0;
        self.icon_data_mut(id).target_x = target;
        let top = self.controls[id].rect.top;
        self.set_position(rect, target, top);
        id
    }

    /// The box is as wide as its text is long: 0.65 of the font's size a character, 150 at
    /// least.
    fn icon_fit_description(&mut self, id: Id, text: &str) {
        let (rect, label) = (self.icon_data(id).description_rect, self.icon_data(id).description_label);
        let font = self.controls[label].font.unwrap_or(self.font);
        let mut width = text.encode_utf16().count() as f32 * self.fonts[font].scale * 0.65;
        if width <= 150.0 {
            width = 150.0;
        }
        let r = self.controls[id].rect;
        self.set_size(rect, width, r.bottom - r.top);
        let r = self.controls[rect].rect;
        self.set_size(label, r.right - r.left, (r.bottom - r.top) - 15.0);
    }

    /// `TaskBarIcon::setIcon` 0x14025cb60: `content/gui/icons/<name>_ON.png` and `_OFF.png`,
    /// else the default icon.
    fn icon_textures(&mut self, painter: &mut dyn Painter, name: &str, id: Id) -> (Texture, Texture) {
        let base = format!("content/gui/icons/{name}");
        let (on, off) = (format!("{base}_ON.png"), format!("{base}_OFF.png"));
        if painter.file_exists(&on) && painter.file_exists(&off) {
            let texture = painter.get_texture(&off);
            let texture_on = painter.get_texture(&on);
            (texture, texture_on)
        } else {
            println!("WARNING: TaskBarIcon :: COULD NOT FIND TEXTURES FOR {}", self.controls[id].name);
            let texture = painter.get_texture("content/gui/icons/default_icon_OFF.png");
            let texture_on = painter.get_texture("content/gui/icons/default_icon_ON.png");
            (texture, texture_on)
        }
    }

    /// `TaskBarIcon::setDescriptionText` 0x14025ca40.
    pub fn icon_set_description_text(&mut self, id: Id, text: &str) {
        let label = self.icon_data(id).description_label;
        self.set_text(label, text);
        self.icon_fit_description(id, text);
    }

    /// `TaskBarIcon::hide` 0x14025c720.
    pub fn icon_hide(&mut self, id: Id, hide: bool) {
        let mut world = self.controls[id].rect;
        self.get_world_rect(id, &mut world);
        let rect = self.icon_data(id).description_rect;
        // (the icon's own frame and background: they show with the title box)
        self.controls[id].draw_background = !hide;
        self.controls[id].draw_border = !hide;
        self.set_visible(rect, !hide);
        if hide {
            let top = self.controls[id].rect.top;
            self.set_position(rect, world.left, top);
        } else {
            let r = self.controls[rect].rect;
            self.icon_data_mut(id).target_x = world.left - (r.right - r.left);
        }
    }

    /// `TaskBarIcon::onMouseMove` 0x14025c7c0: the box shows while the mouse is on the icon.
    pub fn icon_on_mouse_move(&mut self, id: Id, e: &MouseEvent) {
        let mut world: Rect = self.controls[id].rect;
        self.get_world_rect(id, &mut world);
        let (x, y) = (e.x as f32, e.y as f32);
        let over = x > world.left && x < world.right && y > world.top && y < world.bottom && self.controls[id].visible;
        self.icon_hide(id, !over);
    }

    /// `TaskBarIcon::render` 0x14025c8d0.
    pub fn icon_render(&mut self, id: Id, dt: f32, painter: &mut dyn Painter) {
        let mut world = self.controls[id].rect;
        self.get_world_rect(id, &mut world);
        let rect = self.icon_data(id).description_rect;
        if self.controls[rect].visible {
            let cur = self.controls[rect].rect.left;
            let target = self.icon_data(id).target_x;
            let x = if target < cur {
                let candidate = cur - dt * 500.0;
                if candidate < target {
                    target
                } else {
                    candidate
                }
            } else {
                target
            };
            let top = self.controls[id].rect.top;
            self.set_position(rect, x, top);
            self.render(rect, dt, painter);
            let gl = self.controls[id].gl;
            painter.gl_color4f(gl, 1.0, 0.0, 0.0, 1.0);
            let h = (world.bottom - world.top) + 1.0;
            let y = world.top - 1.0;
            painter.gl_quad(gl, world.left - 10.0, y, 10.0, h, false);
        }
        let data = self.icon_data(id);
        let texture = match data.status {
            0 => Some(data.texture_on.clone()),
            1 => Some(data.texture.clone()),
            _ => None,
        };
        if let Some(texture) = texture {
            self.controls[id].back_texture = texture;
        }
        self.control_render(id, dt, painter);
    }
}
