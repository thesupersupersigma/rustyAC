// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! Five small apps: `ACHelp` (0x1c0 bytes, "Help"), `ACHideAll` (0x1d0, "Hide all Apps"),
//! `FormRunningTime` (0x1d0, "Running Time"), `ACTimeFlow` (0x290, "Time of the day") and
//! `TrackDescription` (0x1c8, "Track Description").
//!
//! `ACHelp::ACHelp` 0x140307900; `ACHideAll::ACHideAll` 0x14030f3e0 with its handlers of
//! `Taskbar::evAppOpened` (0x14030f710), `evAppDismiss` (0x14030f6d0) and
//! `GameScreen::evDesktopShown` (0x14030f6a0), its `render` an empty function;
//! `FormRunningTime::FormRunningTime` 0x1402dc760, `render` 0x1402dca90;
//! `ACTimeFlow::ACTimeFlow` 0x14030cb10, `render` 0x14030d270, its handler of
//! `Sim::evOnReplayStatusChanged` (0x14030d1f0); `TrackDescription::TrackDescription`
//! 0x1403270e0, `render` 0x1403273c0, `TrackAvatar::getSectorDescription` 0x1401c80c0.

use std::any::Any;
use std::rc::Rc;

use crate::apps::App;
use crate::control::{Clicked, Id, Kind};
use crate::font::{Font, FontType};
use crate::gui::Gui;
use crate::hud::{time_to_string3, translate};
use crate::painter::{Painter, Texture, ALIGN_CENTER, ALIGN_LEFT, GRAPHICS_GL};

/// `ACHelp`: a text box with `system/locales/help/<language>.tag`.
pub struct Help;

/// `ACHideAll`: nothing drawn; its icon on the app bar hides every app and brings them back.
pub struct HideAll {
    pub screen: Id,
}

/// `FormRunningTime`: the time a "time limited test" has left.
pub struct RunningTime {
    pub time_left: Id,
    /// `TimeLimitedTest::getMaxRunningTime` (minutes)
    pub max_running_time: i32,
}

/// `ACTimeFlow`: the time of the day of the sun, the session's running time.
pub struct TimeFlow {
    pub tex_day_time: Texture,
    pub tex_session_time: Texture,
    pub tex_multiplier: Texture,
    pub tod: String,
    pub st: String,
    pub active: bool,
}

/// `TrackDescription`: the name of the section of the track the focused car is in.
pub struct TrackDescription {
    pub section_description: Id,
    /// the track's `data/sections.ini`: in, out, text
    pub sections: Vec<(f32, f32, String)>,
}

impl Gui {
    /// `ACHelp::ACHelp(GUI*)` 0x140307900; `help` is `system/locales/help/<language>.tag`'s text.
    pub fn new_help_app(&mut self, painter: &mut dyn Painter, help: &str) -> Id {
        let form = self.new_form(painter, "HELP", false);
        self.form_set_title(form, &translate("Help"));
        self.form_data_mut(form).dev_app = false;
        self.set_size(form, 450.0, 550.0);
        let tb = self.new_text_box(painter, "HELP_TEXTBOX", FontType::Proportional);
        let r = self.controls[form].rect;
        self.set_size(tb, r.right - r.left, r.bottom - r.top);
        self.set_position(tb, 20.0, 30.0);
        let mut font = Font::new(painter, "", FontType::Proportional, 18.0, false, false);
        font.scale = 15.0;
        self.fonts.push(font);
        self.controls[tb].font = Some(self.fonts.len() - 1);
        // (read in text mode: the line ends are plain)
        let document = crate::locale::parse_tag_document(&help.replace("\r\n", "\n"));
        let text = document.get("HELP").map(|t| t.text.clone()).unwrap_or_default();
        self.set_formatted_text(tb, &text);
        self.controls[tb].draw_border = false;
        self.controls[tb].draw_background = false;
        self.add_control(form, tb);
        self.set_app(form, Box::new(Help));
        form
    }

    /// `ACHideAll::ACHideAll(Sim*)` 0x14030f3e0 (`Sim::Sim` puts it on the screen first of all).
    pub fn new_hide_all_app(&mut self, painter: &mut dyn Painter, screen: Id) -> Id {
        let form = self.new_form(painter, "HIDE_APPS", false);
        self.form_set_title(form, &translate("Hide all Apps"));
        self.form_data_mut(form).dev_app = false;
        self.set_visible(form, false);
        self.controls[form].draw_border = false;
        self.controls[form].draw_background = false;
        if let Some(title) = self.form_data(form).form_title {
            self.set_visible(title, false);
        }
        self.form_set_icon_visible(form, false);
        self.set_size(form, 0.0, 0.0);
        self.set_position(form, 0.0, 0.0);
        let taskbar = self.taskbar;
        let count = move |gui: &Gui| gui.game_screen_data(screen).old_apps_opened.len();
        let tag = |gui: &Gui, ev: &Clicked| if ev.control < gui.controls.len() && matches!(gui.controls[ev.control].kind, Kind::TaskBarIcon(_)) { gui.icon_data(ev.control).form } else { None };
        self.taskbar_data_mut(taskbar).ev_app_opened.push(Rc::new(move |gui: &mut Gui, ev: &Clicked| {
            let Some(f) = tag(gui, ev) else { return };
            if f == form {
                gui.show_desktop(screen);
                gui.controls[form].visible = count(gui) > 0;
            } else {
                gui.controls[form].visible = false;
            }
        }));
        self.taskbar_data_mut(taskbar).ev_app_dismiss.push(Rc::new(move |gui: &mut Gui, ev: &Clicked| {
            if tag(gui, ev) == Some(form) {
                gui.show_desktop(screen);
                gui.controls[form].visible = count(gui) > 0;
            }
        }));
        if let Kind::GameScreen(data) = &mut self.controls[screen].kind {
            data.ev_desktop_shown.push(Rc::new(move |gui: &mut Gui, _shown: bool| {
                gui.controls[form].visible = count(gui) > 0;
            }));
        }
        self.controls[form].visible = false;
        self.set_app(form, Box::new(HideAll { screen }));
        form
    }

    /// `FormRunningTime::FormRunningTime(Sim*)` 0x1402dc760 (made only with an active
    /// `cfg/time_limited_test.ini`).
    pub fn new_running_time_app(&mut self, painter: &mut dyn Painter, max_running_time: i32) -> Id {
        let form = self.new_form(painter, "RUNNING_TIME", true);
        self.form_set_title(form, "Running Time");
        self.form_data_mut(form).dev_app = false;
        let mut font = Font::new(painter, "", FontType::Monospaced, 64.0, false, false);
        font.shadowed = true;
        self.fonts.push(font);
        let font = self.fonts.len() - 1;
        self.controls[form].font = Some(font);
        let l = self.new_label(painter, "");
        self.set_position(l, 5.0, 25.0);
        self.set_size(l, 400.0, 32.0);
        self.controls[l].font = Some(font);
        self.controls[l].fore_color = [1.0; 4];
        self.add_control(form, l);
        self.set_size(form, 500.0, 50.0);
        self.set_auto_hide_mode(form, true);
        let m = self.controls[form].scale_mult;
        self.scale_by_mult(form, m);
        self.set_app(form, Box::new(RunningTime { time_left: l, max_running_time }));
        form
    }

    /// `ACTimeFlow::ACTimeFlow(Sim*)` 0x14030cb10.
    pub fn new_time_flow_app(&mut self, painter: &mut dyn Painter) -> Id {
        let form = self.new_form(painter, "TIME_FLOW", true);
        self.form_set_title(form, "Time of the day");
        self.form_data_mut(form).dev_app = false;
        self.set_size(form, 300.0, 150.0);
        self.form_set_title(form, &translate("Time of the day"));
        let mut font = Font::new(painter, "", FontType::Proportional, 20.0, false, false);
        font.set_color(1.0, 0.0, 0.0, 1.0);
        self.fonts.push(font);
        self.controls[form].font = Some(self.fonts.len() - 1);
        let tex_day_time = painter.get_texture("content/gui/actimeflow/inapp_time.png");
        let tex_session_time = painter.get_texture("content/gui/actimeflow/inapp_sessiontime.png");
        let tex_multiplier = painter.get_texture("content/gui/actimeflow/inapp_multi.png");
        let m = self.controls[form].scale_mult;
        self.scale_by_mult(form, m);
        self.set_app(form, Box::new(TimeFlow { tex_day_time, tex_session_time, tex_multiplier, tod: String::new(), st: String::new(), active: true }));
        form
    }

    /// `TrackDescription::TrackDescription(Sim*)` 0x1403270e0; `sections` from the track's
    /// `data/sections.ini` (`TrackAvatar::TrackAvatar`).
    pub fn new_track_description_app(&mut self, painter: &mut dyn Painter, sections: Vec<(f32, f32, String)>) -> Id {
        let form = self.new_form(painter, "TRACK_DESCRIPTION", true);
        self.form_data_mut(form).dev_app = false;
        self.form_data_mut(form).autohide = true;
        self.form_set_title(form, &translate("Track Description"));
        self.set_size(form, 450.0, 80.0);
        let l = self.new_label(painter, "TRACK_SECTION_DESCRIPTION");
        let mut font = Font::new(painter, "", FontType::Proportional, 30.0, false, false);
        font.shadowed = true;
        self.fonts.push(font);
        self.controls[l].font = Some(self.fonts.len() - 1);
        self.set_position(l, 0.0, 35.0);
        let r = self.controls[form].rect;
        self.set_size(l, r.right - r.left, 50.0);
        self.controls[l].font_align = ALIGN_CENTER;
        self.add_control(form, l);
        let m = self.controls[form].scale_mult;
        self.scale_by_mult(form, m);
        self.set_app(form, Box::new(TrackDescription { section_description: l, sections }));
        form
    }
}

/// The track's `data/sections.ini` as `TrackAvatar::TrackAvatar` 0x1401c5250 reads it:
/// `SECTION_0`, `SECTION_1` … until one is missing.
pub fn read_sections(ini: Option<&rustyac_physics::data::ini::IniReader>) -> Vec<(f32, f32, String)> {
    let mut out = Vec::new();
    let Some(ini) = ini.filter(|i| i.ready) else { return out };
    let mut n = 0;
    while ini.has_section(&format!("SECTION_{n}")) {
        let section = format!("SECTION_{n}");
        out.push((ini.get_float(&section, "IN").unwrap_or(0.0), ini.get_float(&section, "OUT").unwrap_or(0.0), ini.get_string(&section, "TEXT")));
        n += 1;
    }
    out
}

impl App for Help {
    fn as_any(&mut self) -> &mut dyn Any {
        self
    }
}

impl App for HideAll {
    fn as_any(&mut self) -> &mut dyn Any {
        self
    }

    /// The table's `render` is an empty function (0x140017870): nothing is drawn.
    fn render(&mut self, _gui: &mut Gui, _id: Id, _dt: f32, _painter: &mut dyn Painter) {
        let _ = self.screen;
    }
}

impl App for RunningTime {
    fn as_any(&mut self) -> &mut dyn Any {
        self
    }

    /// `FormRunningTime::render` 0x1402dca90.
    fn render(&mut self, gui: &mut Gui, id: Id, dt: f32, painter: &mut dyn Painter) {
        let elapsed = crate::app_laps::cut(gui.apps.game_time - gui.apps.game_start_time);
        let rem = self.max_running_time.wrapping_mul(60000).wrapping_sub(elapsed);
        if rem <= 0 {
            gui.controls[self.time_left].fore_color = [1.0, 0.0, 0.0, 1.0];
            gui.set_text(self.time_left, "TIME:TEST ENDED");
        } else {
            let mut c = [1.0; 4];
            // (half seconds: the division is by 500)
            if rem < 30000 && (rem / 500) % 2 != 0 {
                c = [1.0, 0.0, 0.0, 1.0];
            }
            gui.controls[self.time_left].fore_color = c;
            gui.set_text(self.time_left, &time_to_string3(rem));
        }
        gui.control_render(id, dt, painter);
    }
}

/// The inline `floorf` (cut, one less for a negative with a fraction).
fn floor_asm(x: f32) -> f32 {
    let t = crate::app_essentials::cut_f(x);
    if t == i32::MIN {
        return x;
    }
    let f = t as f32;
    if f != x && x.is_sign_negative() {
        f - 1.0
    } else {
        f
    }
}

/// `std::ostream << float` in the default state (six significant digits, no trailing zeros).
fn stream_float(v: f32) -> String {
    crate::app_track_map::format_g6(v as f64)
}

impl App for TimeFlow {
    fn as_any(&mut self) -> &mut dyn Any {
        self
    }

    /// `ACTimeFlow::render` 0x14030d270.
    fn render(&mut self, gui: &mut Gui, id: Id, dt: f32, painter: &mut dyn Painter) {
        gui.control_render(id, dt, painter);
        let link = gui.apps.clone();
        let s = gui.controls[id].scale_mult;
        let o = s * 10.0;
        let a = s * 65.0;
        let [x, y] = gui.local_to_world(id, [s * 20.0, s * 40.0]);
        let font = gui.controls[id].font.unwrap_or(gui.font);
        if !self.active {
            gui.fonts[font].blit_string(painter, (x + a) - s * 20.0, y - s * 25.0, "Time paused", 1.0, ALIGN_LEFT);
        } else {
            let angle = link.sun_angle;
            let t = (angle + 80.0) * 0.0625 + 8.0;
            let h = floor_asm(t);
            let m1 = (t - h) * 60.0;
            let mf = floor_asm(m1);
            let s1 = floor_asm((m1 - mf) * 60.0);
            let (hi, mi, si) = (crate::app_essentials::cut_f(h), crate::app_essentials::cut_f(m1), crate::app_essentials::cut_f(s1));
            let two = |v: i32| if v < 10 { format!("0{v}") } else { v.to_string() };
            let mut tod = if hi > 17 {
                "18:00:00".to_string()
            } else if hi < 8 {
                "8:00:00".to_string()
            } else {
                format!("{}:{}:{}", two(hi), two(mi), two(si))
            };
            let secs = ((link.game_time - link.game_start_time) * 0.001) as f32;
            let hh = crate::app_essentials::cut_f(floor_asm(secs * f32::from_bits(0x3991_a2b4)));
            let mm = crate::app_essentials::cut_f(secs * f32::from_bits(0x3c88_8889)) % 60;
            let ss = crate::app_essentials::cut_f(floor_asm(secs)) % 60;
            let st = format!("{}:{}:{}", two(hh), two(mm), two(ss));
            if let Some(mult) = link.time_mult {
                tod.push_str(&format!("  (x {})", stream_float(mult)));
            }
            self.tod = tod;
            self.st = st;
        }
        let gl = GRAPHICS_GL;
        painter.gl_color4f(gl, 1.0, 1.0, 1.0, 1.0);
        painter.set_texture(0, &self.tex_day_time);
        painter.gl_quad(gl, x, y, s * 48.0, s * 48.0, true);
        gui.fonts[font].blit_string(painter, x + a, y + o, &self.tod, s, ALIGN_LEFT);
        painter.set_texture(0, &self.tex_session_time);
        painter.gl_quad(gl, x, s * 45.0 + y, s * 48.0, s * 48.0, true);
        gui.fonts[font].blit_string(painter, x + a, (s * 45.0 + y) + o, &self.st, s, ALIGN_LEFT);
        let _ = &self.tex_multiplier;
    }

    /// The handler of `Sim::evOnReplayStatusChanged` 0x14030d1f0.
    fn on_replay_status(&mut self, _gui: &mut Gui, _id: Id, entered: bool) {
        self.active = !entered;
    }
}

impl App for TrackDescription {
    fn as_any(&mut self) -> &mut dyn Any {
        self
    }

    /// `TrackDescription::render` 0x1403273c0.
    fn render(&mut self, gui: &mut Gui, id: Id, dt: f32, painter: &mut dyn Painter) {
        gui.control_render(id, dt, painter);
        let p = gui.apps.focused().state.normalized_spline_position;
        let text = self.sections.iter().find(|(i, o, _)| *i < p && p < *o).map(|s| s.2.clone()).unwrap_or_default();
        gui.set_text(self.section_description, &text);
    }
}
