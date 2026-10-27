// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! The `ac` module: the 116 functions `PythonInterface::PythonInterface` registers, in its
//! order, with the game's handlers' argument formats, returns and quirks (a failed parse
//! returns -1 and leaves Python's error set; handles are 1-based; a bad handle is printed).

use std::ffi::{c_char, c_int, c_long};
use std::rc::Rc;

use rustyac_gui::control::{Id, Kind};
use rustyac_gui::font::{Font, FontType};
use rustyac_gui::painter::GRAPHICS_GL;
use rustyac_gui::widgets::{Widget, WidgetEvent};
use rustyac_gui::widgets2::Widget2;
use rustyac_gui::widgets3::Widget3;

use crate::ffi::{api, MethodDef, ModuleDef, Obj};
use crate::{gui, painter, state, unicode, with_gui, Action};

type R = Obj;
const NUL: Obj = std::ptr::null_mut();

macro_rules! parse {
    ($args:expr, $fmt:literal $(, $p:expr)*) => {
        (api().arg_parse_tuple)($args, $fmt.as_ptr() $(, $p)*) != 0
    };
}

pub(crate) unsafe fn long(v: i32) -> Obj {
    (api().long_from_long)(v as c_long)
}

unsafe fn float(v: f64) -> Obj {
    (api().float_from_double)(v)
}

unsafe fn floats(v: &[f32]) -> Obj {
    let t = (api().tuple_new)(v.len() as isize);
    for (i, x) in v.iter().enumerate() {
        (api().tuple_set_item)(t, i as isize, float(*x as f64));
    }
    t
}

/// A list of ints (the game fills the list's items directly: `PyList_SetItem` is not imported).
unsafe fn ints(v: &[i32]) -> Obj {
    let l = (api().list_new)(v.len() as isize);
    let items = *(l as *const u8).add(0x18).cast::<*mut Obj>();
    for (i, x) in v.iter().enumerate() {
        *items.add(i) = long(*x);
    }
    l
}

/// `PyUnicode_Check`: `Py_TPFLAGS_UNICODE_SUBCLASS` in the type's `tp_flags` (+0xa8).
pub(crate) unsafe fn is_str(o: Obj) -> bool {
    let ty = *(o as *const *const u8).add(1);
    *ty.add(0xa8).cast::<u32>() & (1 << 28) != 0
}

/// `pyString` 0x140124590: a str as text (anything else: empty, Python's error left set).
pub(crate) unsafe fn text(o: Obj) -> String {
    let p = (api().unicode_as_wide_char_string)(o, std::ptr::null_mut());
    if p.is_null() {
        return String::new();
    }
    let mut n = 0;
    while *p.add(n) != 0 {
        n += 1;
    }
    let s = String::from_utf16_lossy(std::slice::from_raw_parts(p, n));
    (api().mem_free)(p.cast());
    s
}

pub(crate) use text as text_keep_error;

/// `PyUnicode_AsUnicodeAndSize` as `log` and `console` read their text.
unsafe fn unicode_text(o: Obj) -> String {
    let mut n = 0isize;
    let p = (api().unicode_as_unicode_and_size)(o, &mut n);
    if p.is_null() {
        return String::new();
    }
    String::from_utf16_lossy(std::slice::from_raw_parts(p, n as usize))
}

/// The control of a 1-based handle (none: printed, as the game does at every attempt).
fn control(index: i32) -> Option<Id> {
    let c = state(|s| (index >= 1).then(|| s.controls.get(index as usize - 1).copied()).flatten());
    if c.is_none() {
        println!("ERROR: [PYI ERROR]: Control index {index} out of range");
    }
    c
}

fn is_form(id: Id) -> bool {
    matches!(gui().controls[id].kind, Kind::Form(_))
}

fn widget(id: Id) -> Option<&'static Widget> {
    match &gui().controls[id].kind {
        Kind::Widget(w) => Some(w),
        _ => None,
    }
}

fn widget2(id: Id) -> Option<&'static Widget2> {
    match widget(id)? {
        Widget::More(w) => Some(w),
        _ => None,
    }
}

fn widget3(id: Id) -> Option<&'static Widget3> {
    match widget2(id)? {
        Widget2::More(w) => Some(w),
        _ => None,
    }
}

fn is_progress_bar(id: Id) -> bool {
    matches!(widget(id), Some(Widget::ProgressBar(_)))
}
fn is_check_box(id: Id) -> bool {
    matches!(widget(id), Some(Widget::CheckBox(_)))
}
fn is_text_box(id: Id) -> bool {
    matches!(widget2(id), Some(Widget2::TextBox(_)))
}
fn is_text_input(id: Id) -> bool {
    matches!(widget2(id), Some(Widget2::TextInput(_)))
}
fn is_graph(id: Id) -> bool {
    matches!(widget3(id), Some(Widget3::Graph(_)))
}
fn is_spinner(id: Id) -> bool {
    matches!(widget3(id), Some(Widget3::Spinner(_)))
}
fn in01(v: f32) -> bool {
    (0.0..=1.0).contains(&v)
}

/// The car of an index (`Sim::getCar`: unsigned, below the count).
fn car(index: i32) -> Option<usize> {
    let n = gui().apps.cars.len();
    ((index as u32 as usize) < n).then_some(index as usize)
}

fn info(index: usize) -> crate::CarInfo {
    state(|s| s.world.cars.get(index).cloned().unwrap_or_default())
}

// ---- the app window ----

unsafe extern "C" fn log(_: Obj, args: Obj) -> R {
    let mut o = NUL;
    if !parse!(args, c"O", &mut o) {
        return long(-1);
    }
    crate::write_to_log(&unicode_text(o));
    float(1.0)
}

unsafe extern "C" fn console(_: Obj, args: Obj) -> R {
    let mut o = NUL;
    if !parse!(args, c"O", &mut o) {
        return long(-1);
    }
    let t = unicode_text(o);
    state(|s| s.console.push(format!("[PYTHON]:{t}")));
    float(1.0)
}

unsafe extern "C" fn new_app(_: Obj, args: Obj) -> R {
    let (mut o, mut category) = (NUL, 0 as c_long);
    if !parse!(args, c"O|l", &mut o, &mut category) {
        return long(-1);
    }
    let name = text(o);
    let Some(painter) = painter() else { return long(-1) };
    let g = gui();
    let form = g.new_form(painter, &name, false);
    g.set_app(form, Box::new(crate::PythonForm));
    g.form_set_title(form, &name);
    // a setup app (category 1) is kept and `evOnSetupAppCreated` fires, but nothing in
    // acs.exe 1.16 listens to it (only the constructor, destructor and newApp touch it): the
    // form is on no screen and has no icon
    if category != 1 {
        let screen = state(|s| s.screen);
        g.game_screen_add_control(painter, screen, form, true);
    }
    state(|s| {
        s.forms.push((form, Vec::new()));
        s.controls.push(form);
        long(s.controls.len() as i32)
    })
}

unsafe extern "C" fn set_title(_: Obj, args: Obj) -> R {
    let (mut i, mut o) = (0 as c_long, NUL);
    if !parse!(args, c"lO", &mut i, &mut o) {
        return long(-1);
    }
    let t = text(o);
    match control(i) {
        Some(id) if is_form(id) => {
            gui().form_set_title(id, &t);
            long(1)
        }
        _ => long(-1),
    }
}

unsafe extern "C" fn set_position(_: Obj, args: Obj) -> R {
    let (mut i, mut x, mut y) = (0 as c_long, 0f32, 0f32);
    if !parse!(args, c"lff", &mut i, &mut x, &mut y) {
        return long(-1);
    }
    match control(i) {
        Some(id) => {
            gui().set_position(id, x, y);
            long(1)
        }
        None => long(-1),
    }
}

unsafe fn form_part(args: Obj, part: impl FnOnce(&rustyac_gui::form::FormData) -> Option<Id>) -> R {
    let (mut i, mut x, mut y) = (0 as c_long, 0f32, 0f32);
    if !parse!(args, c"lff", &mut i, &mut x, &mut y) {
        return long(-1);
    }
    match control(i) {
        Some(id) if is_form(id) => {
            if let Some(p) = part(gui().form_data(id)) {
                gui().set_position(p, x, y);
            }
            long(1)
        }
        _ => long(-1),
    }
}

unsafe extern "C" fn set_icon_position(_: Obj, args: Obj) -> R {
    form_part(args, |f| f.icon)
}

unsafe extern "C" fn set_title_position(_: Obj, args: Obj) -> R {
    form_part(args, |f| f.form_title)
}

unsafe extern "C" fn set_text(_: Obj, args: Obj) -> R {
    let (mut i, mut o) = (0 as c_long, NUL);
    if !parse!(args, c"lO", &mut i, &mut o) {
        return long(-1);
    }
    let t = text(o);
    if control(i).is_some_and(is_text_box) {
        gui().set_formatted_text(control(i).unwrap(), &t);
        return long(1);
    }
    match control(i) {
        Some(id) => {
            gui().set_text(id, &t);
            long(1)
        }
        None => long(-1),
    }
}

unsafe extern "C" fn get_text(_: Obj, args: Obj) -> R {
    let mut i = 0 as c_long;
    if !parse!(args, c"l", &mut i) {
        return long(-1);
    }
    match control(i) {
        Some(id) => unicode(&gui().controls[id].text),
        None => long(-1),
    }
}

unsafe extern "C" fn get_position(_: Obj, args: Obj) -> R {
    let mut i = 0 as c_long;
    if !parse!(args, c"l", &mut i) {
        return long(-1);
    }
    match control(i) {
        Some(id) => {
            let r = gui().controls[id].rect;
            floats(&[r.left, r.top])
        }
        // shortcut: the game returns a tuple of two empty items here (Python crashes on it)
        None => long(-1),
    }
}

unsafe extern "C" fn set_size(_: Obj, args: Obj) -> R {
    let (mut i, mut w, mut h) = (0 as c_long, 0f32, 0f32);
    if !parse!(args, c"lff", &mut i, &mut w, &mut h) {
        return long(-1);
    }
    match control(i) {
        Some(id) => {
            gui().set_size(id, w, h);
            long(1)
        }
        None => long(-1),
    }
}

unsafe extern "C" fn get_size(_: Obj, args: Obj) -> R {
    let mut i = 0 as c_long;
    if !parse!(args, c"l", &mut i) {
        return long(-1);
    }
    match control(i) {
        Some(id) => {
            let r = gui().controls[id].rect;
            floats(&[r.right - r.left, r.bottom - r.top])
        }
        None => long(-1),
    }
}

unsafe extern "C" fn set_visible(_: Obj, args: Obj) -> R {
    let (mut i, mut v) = (0 as c_long, 0 as c_long);
    if !parse!(args, c"ll", &mut i, &mut v) {
        return long(-1);
    }
    match control(i) {
        Some(id) => {
            gui().set_visible(id, v > 0);
            long(1)
        }
        None => long(-1),
    }
}

unsafe extern "C" fn set_background_color(_: Obj, args: Obj) -> R {
    let (mut i, mut r, mut g, mut b) = (0 as c_long, 0f32, 0f32, 0f32);
    if !parse!(args, c"lfff", &mut i, &mut r, &mut g, &mut b) {
        return long(-1);
    }
    match control(i) {
        Some(id) if in01(r) && in01(g) && in01(b) => {
            let c = &mut gui().controls[id].back_color;
            c[0] = r;
            c[1] = g;
            c[2] = b;
            long(1)
        }
        _ => long(-1),
    }
}

unsafe extern "C" fn set_background_opacity(_: Obj, args: Obj) -> R {
    let (mut i, mut a) = (0 as c_long, 0f32);
    if !parse!(args, c"lf", &mut i, &mut a) {
        return long(-1);
    }
    match control(i) {
        Some(id) if in01(a) => {
            gui().controls[id].back_color[3] = a;
            long(1)
        }
        _ => long(-1),
    }
}

unsafe extern "C" fn draw_background(_: Obj, args: Obj) -> R {
    let (mut i, mut o) = (0 as c_long, NUL);
    if !parse!(args, c"lO", &mut i, &mut o) {
        return long(-1);
    }
    match control(i) {
        Some(id) => {
            // (the game reads the object's address as the flag: never 0)
            gui().controls[id].draw_background = true;
            long(1)
        }
        None => long(-1),
    }
}

unsafe extern "C" fn draw_border(_: Obj, args: Obj) -> R {
    let (mut i, mut v) = (0 as c_long, 0 as c_long);
    if !parse!(args, c"ll", &mut i, &mut v) {
        return long(-1);
    }
    match control(i) {
        Some(id) => {
            gui().controls[id].draw_border = v >= 1;
            long(1)
        }
        None => long(-1),
    }
}

unsafe extern "C" fn set_background_texture(_: Obj, args: Obj) -> R {
    let (mut i, mut o) = (0 as c_long, NUL);
    if !parse!(args, c"lO", &mut i, &mut o) {
        return long(-1);
    }
    let path = text(o);
    let Some(id) = control(i) else { return long(-1) };
    let t = crate::load_texture(&path);
    if t == 0 {
        return long(-1);
    }
    gui().controls[id].back_texture = crate::texture(t).unwrap_or_default();
    long(t)
}

unsafe extern "C" fn set_font_alignment(_: Obj, args: Obj) -> R {
    let (mut i, mut o) = (0 as c_long, NUL);
    if !parse!(args, c"lO", &mut i, &mut o) {
        return long(-1);
    }
    let t = text(o);
    let Some(id) = control(i) else { return long(-1) };
    let align = match t.as_str() {
        "left" => 0,
        "right" => 1,
        "center" => 2,
        _ => return long(-1),
    };
    gui().controls[id].font_align = align;
    long(1)
}

unsafe extern "C" fn add_render_callback(_: Obj, args: Obj) -> R {
    let (mut i, mut o) = (0 as c_long, NUL);
    if !parse!(args, c"lO", &mut i, &mut o) {
        return long(-1);
    }
    let Some(id) = control(i) else { return long(-1) };
    state(|s| match s.forms.iter_mut().find(|f| f.0 == id) {
        Some(f) => {
            f.1.push(o);
            long(1)
        }
        None => long(-1),
    })
}

// ---- listeners (the callbacks are borrowed; their results and errors are left as they are) ----

unsafe fn listener(args: Obj, add: impl FnOnce(Id, Obj) -> bool) -> R {
    let (mut i, mut o) = (0 as c_long, NUL);
    if !parse!(args, c"lO", &mut i, &mut o) {
        return long(-1);
    }
    let ok = match control(i) {
        Some(id) => add(id, o),
        None => false,
    };
    long(ok as i32)
}

unsafe extern "C" fn add_on_clicked_listener(_: Obj, args: Obj) -> R {
    listener(args, |id, cb| {
        gui().controls[id].ev_clicked.push(Rc::new(move |g, e| {
            with_gui(g, || unsafe { (api().object_call_function)(cb, c"ii".as_ptr(), e.local_x as c_int, e.local_y as c_int) });
        }));
        true
    })
}

unsafe extern "C" fn add_on_validate_listener(_: Obj, args: Obj) -> R {
    listener(args, |id, cb| {
        if !is_text_input(id) {
            return false;
        }
        gui().text_input_mut(id).handlers.push(Rc::new(move |g, e| {
            if let WidgetEvent::Text { text, kind: 0, .. } = e {
                with_gui(g, || unsafe {
                    let w = crate::wide(text);
                    (api().object_call_function)(cb, c"S".as_ptr(), (api().unicode_from_wide_char)(w.as_ptr(), w.len() as isize));
                });
                g.set_text(id, "");
            }
        }));
        true
    })
}

unsafe extern "C" fn add_on_value_change_listener(_: Obj, args: Obj) -> R {
    listener(args, |id, cb| {
        if !is_spinner(id) {
            return false;
        }
        gui().spinner_mut(id).handlers.push(Rc::new(move |g, e| {
            if let WidgetEvent::SpinnerValueChanged { value, .. } = e {
                with_gui(g, || unsafe { (api().object_call_function)(cb, c"i".as_ptr(), *value as c_int) });
            }
        }));
        true
    })
}

unsafe extern "C" fn add_on_check_box_changed(_: Obj, args: Obj) -> R {
    listener(args, |id, cb| {
        if !is_check_box(id) {
            return false;
        }
        gui().check_box_mut(id).handlers.push(Rc::new(move |g, e| {
            if let WidgetEvent::CheckBoxChanged { value, .. } = e {
                let name = crate::wide(&g.controls[id].name);
                with_gui(g, || unsafe { (api().object_call_function)(cb, c"Si".as_ptr(), (api().unicode_from_unicode)(name.as_ptr(), name.len() as isize), *value as c_int) });
            }
        }));
        true
    })
}

/// The list box listeners: a list box cannot be made (`addListBox` is a stub), and their
/// callbacks never call Python.
unsafe extern "C" fn add_on_list_box_listener(_: Obj, args: Obj) -> R {
    listener(args, |_, _| false)
}

/// `PythonInterface::addPythonOnAppActivatedListener` 0x14012f120 / `...DismissedListener`
/// 0x14012f240: a form's handler on the taskbar's `evOnAppOpened` / `evOnAppDismissed`
/// (lambda 0x140126c40: the clicked icon's tag is the form, then `cb(index)`).
unsafe fn add_on_app_listener(args: Obj, dismissed: bool) -> R {
    let (mut i, mut o) = (0 as c_long, NUL);
    if !parse!(args, c"lO", &mut i, &mut o) {
        return long(-1);
    }
    let Some(form) = control(i).filter(|id| is_form(*id)) else { return long(0) };
    let g = gui();
    let taskbar = g.taskbar;
    let handler: rustyac_gui::control::ClickHandler = Rc::new(move |g, e| {
        let tag = (e.control < g.controls.len() && matches!(g.controls[e.control].kind, Kind::TaskBarIcon(_))).then(|| g.icon_data(e.control).form).flatten();
        if tag == Some(form) {
            with_gui(g, || unsafe { (api().object_call_function)(o, c"i".as_ptr(), i as c_int) });
        }
    });
    let data = g.taskbar_data_mut(taskbar);
    if dismissed { &mut data.ev_app_dismiss } else { &mut data.ev_app_opened }.push(handler);
    long(1)
}

unsafe extern "C" fn add_on_app_activated_listener(_: Obj, args: Obj) -> R {
    add_on_app_listener(args, false)
}

unsafe extern "C" fn add_on_app_dismissed_listener(_: Obj, args: Obj) -> R {
    add_on_app_listener(args, true)
}

unsafe extern "C" fn send_chat_message(_: Obj, args: Obj) -> R {
    let mut o = NUL;
    if !parse!(args, c"O", &mut o) {
        return long(-1);
    }
    let t = text(o);
    state(|s| {
        if s.world.server.is_some() {
            s.actions.push(Action::SendChat(t));
        }
    });
    long(1)
}

unsafe extern "C" fn add_on_chat_message_listener(_: Obj, args: Obj) -> R {
    let (mut i, mut o) = (0 as c_long, NUL);
    if !parse!(args, c"lO", &mut i, &mut o) {
        return long(-1);
    }
    // PythonInterface::addPythonOnChatMessageListener: a handler on the client's
    // `evOnChatMessage` (offline there is no client: none)
    state(|s| {
        if s.world.server.is_some() {
            s.chat.push(o);
        }
    });
    long(1)
}

// ---- fonts ----

unsafe extern "C" fn init_font(_: Obj, args: Obj) -> R {
    let (mut i, mut o, mut italic, mut bold) = (0 as c_long, NUL, 0 as c_long, 0 as c_long);
    if !parse!(args, c"lOll", &mut i, &mut o, &mut italic, &mut bold) {
        return long(-1);
    }
    let name = text(o);
    if let Some(p) = painter() {
        Font::new(p, &name, FontType::Custom, 12.0, italic != 0, bold != 0);
    }
    long(1)
}

unsafe extern "C" fn set_font(_: Obj, _args: Obj) -> R {
    long(1)
}

unsafe extern "C" fn set_font_color(_: Obj, args: Obj) -> R {
    let (mut i, mut r, mut g, mut b, mut a) = (0 as c_long, 0f32, 0f32, 0f32, 0f32);
    if !parse!(args, c"lffff", &mut i, &mut r, &mut g, &mut b, &mut a) {
        return long(-1);
    }
    if !(in01(r) && in01(g) && in01(b) && in01(a)) {
        return long(-1);
    }
    match control(i) {
        Some(id) => {
            gui().controls[id].fore_color = [r, g, b, a];
            long(1)
        }
        None => long(-1),
    }
}

/// The control of a font handler (the game looks twice: a list box first).
fn font_control(i: i32) -> Option<Id> {
    control(i)?;
    control(i)
}

unsafe extern "C" fn set_font_size(_: Obj, args: Obj) -> R {
    let (mut i, mut size) = (0 as c_long, 0f32);
    if !parse!(args, c"lf", &mut i, &mut size) {
        return long(-1);
    }
    if control(i).is_none() {
        control(i);
        return long(-1);
    }
    let id = control(i).unwrap();
    let g = gui();
    match g.controls[id].font {
        Some(f) => g.fonts[f].scale = size,
        None => {
            let Some(p) = painter() else { return long(-1) };
            g.fonts.push(Font::new(p, "", FontType::Proportional, size, false, false));
            g.controls[id].font = Some(g.fonts.len() - 1);
        }
    }
    long(1)
}

unsafe extern "C" fn set_custom_font(_: Obj, args: Obj) -> R {
    let (mut i, mut o, mut italic, mut bold) = (0 as c_long, NUL, 0 as c_long, 0 as c_long);
    if !parse!(args, c"lOll", &mut i, &mut o, &mut italic, &mut bold) {
        return long(-1);
    }
    let Some(id) = font_control(i).or_else(|| control(i)) else { return long(-1) };
    let name = text(o);
    let g = gui();
    let size = g.controls[id].font.map_or(12.0, |f| g.fonts[f].scale);
    let Some(p) = painter() else { return long(-1) };
    g.fonts.push(Font::new(p, &name, FontType::Custom, size, italic != 0, bold != 0));
    g.controls[id].font = Some(g.fonts.len() - 1);
    long(1)
}

// ---- controls ----

/// The add* handlers' common steps: the parent must be a form; a new control, its settings,
/// `addControl`, `push_back`; the new handle.
unsafe fn add(args: Obj, fmt: &std::ffi::CStr, make: impl FnOnce(&mut rustyac_gui::gui::Gui, &mut dyn rustyac_gui::painter::Painter, &str, Id, i32) -> Id) -> R {
    let (mut i, mut o, mut extra) = (0 as c_long, NUL, 0 as c_long);
    if (api().arg_parse_tuple)(args, fmt.as_ptr(), &mut i, &mut o, &mut extra) == 0 {
        return long(-1);
    }
    let name = text(o);
    let Some(parent) = control(i) else { return long(-1) };
    if !is_form(parent) {
        return long(-1);
    }
    let Some(p) = painter() else { return long(-1) };
    let g = gui();
    let id = make(g, p, &name, parent, extra);
    g.add_control(parent, id);
    state(|s| {
        s.controls.push(id);
        long(s.controls.len() as i32)
    })
}

unsafe extern "C" fn add_label(_: Obj, args: Obj) -> R {
    add(args, c"lO", |g, p, name, _, _| g.new_label(p, name))
}

unsafe extern "C" fn add_button(_: Obj, args: Obj) -> R {
    add(args, c"lO", |g, p, name, form, _| {
        let id = g.new_control(p, name);
        g.set_size(id, 200.0, 200.0);
        g.controls[id].font = g.controls[form].font;
        g.set_text(id, name);
        id
    })
}

unsafe extern "C" fn add_check_box(_: Obj, args: Obj) -> R {
    add(args, c"lO", |g, p, name, form, _| {
        let id = g.new_check_box(p, name);
        g.set_size(id, 300.0, 25.0);
        g.controls[id].font = g.controls[form].font;
        id
    })
}

unsafe extern "C" fn add_graph(_: Obj, args: Obj) -> R {
    add(args, c"lO", |g, p, name, _, _| {
        let id = g.new_graph(p, name);
        g.graph_mut(id).min_y = 0.0;
        g.graph_mut(id).max_y = 100.0;
        g.set_size(id, 200.0, 200.0);
        g.graph_mut(id).max_values_count = 300;
        id
    })
}

unsafe extern "C" fn add_spinner(_: Obj, args: Obj) -> R {
    add(args, c"lO", |g, p, name, _, _| {
        let id = g.new_spinner(p, name, false, false);
        g.spinner_mut(id).min_value = 0;
        g.spinner_mut(id).max_value = 100;
        g.set_size(id, 200.0, 30.0);
        id
    })
}

unsafe extern "C" fn add_progress_bar(_: Obj, args: Obj) -> R {
    add(args, c"lO|l", |g, p, name, _, vertical| {
        let id = g.new_progress_bar(p, name);
        g.set_position(id, 50.0, 50.0);
        g.progress_bar_mut(id).is_vertical = vertical > 0;
        id
    })
}

unsafe extern "C" fn add_text_input(_: Obj, args: Obj) -> R {
    add(args, c"lO", |g, p, name, _, _| {
        let id = g.new_text_input(p, name);
        g.set_position(id, 0.0, 0.0);
        g.set_size(id, 500.0, 50.0);
        id
    })
}

unsafe extern "C" fn add_text_box(_: Obj, args: Obj) -> R {
    add(args, c"lO", |g, p, name, _, _| g.new_text_box(p, name, FontType::Proportional))
}

unsafe extern "C" fn add_serie_to_graph(_: Obj, args: Obj) -> R {
    let (mut i, mut r, mut g, mut b) = (0 as c_long, 0f32, 0f32, 0f32);
    if !parse!(args, c"lfff", &mut i, &mut r, &mut g, &mut b) {
        return long(-1);
    }
    if !(in01(r) && in01(g) && in01(b)) {
        return long(-1);
    }
    match control(i) {
        Some(id) if is_graph(id) => {
            gui().graph_add_serie(id, [r, g, b]);
            long(i)
        }
        _ => long(-1),
    }
}

unsafe extern "C" fn add_value_to_graph(_: Obj, args: Obj) -> R {
    let (mut i, mut serie, mut v) = (0 as c_long, 0 as c_int, 0f32);
    if !parse!(args, c"lif", &mut i, &mut serie, &mut v) {
        return long(-1);
    }
    match control(i) {
        Some(id) if is_graph(id) && serie < gui().graph_mut(id).series.len() as i32 => {
            if serie >= 0 {
                gui().graph_add_value(id, serie as u32, v);
            }
            long(1)
        }
        _ => long(-1),
    }
}

/// `addListBox`: a stub in this build (no list box is made).
unsafe extern "C" fn add_list_box(_: Obj, args: Obj) -> R {
    let (mut i, mut o) = (0 as c_long, NUL);
    if parse!(args, c"lO", &mut i, &mut o) {
        text(o);
    }
    long(-1)
}

unsafe extern "C" fn add_item(_: Obj, args: Obj) -> R {
    let (mut i, mut o) = (0 as c_long, NUL);
    if parse!(args, c"lO", &mut i, &mut o) {
        control(i);
    }
    long(-1)
}

/// `removeItem`, `setItemNumberPerPage`, `highlightListBoxItem`, `setAllowMultiSelection`:
/// one stub.
unsafe extern "C" fn list_box_ll(_: Obj, args: Obj) -> R {
    let (mut i, mut v) = (0 as c_long, 0 as c_long);
    let _ = parse!(args, c"ll", &mut i, &mut v);
    long(-1)
}

/// `getItemCount`, `getSelectedItems`.
unsafe extern "C" fn list_box_l(_: Obj, args: Obj) -> R {
    let mut i = 0 as c_long;
    let _ = parse!(args, c"l", &mut i);
    long(-1)
}

unsafe extern "C" fn set_allow_deselection(_: Obj, args: Obj) -> R {
    let (mut i, mut v) = (0 as c_long, 0 as c_long);
    if !parse!(args, c"ll", &mut i, &mut v) {
        return long(-1);
    }
    // (no list box can exist)
    control(i);
    long(-1)
}

unsafe extern "C" fn set_step(_: Obj, args: Obj) -> R {
    let (mut i, mut step) = (0 as c_long, 0 as c_int);
    if !parse!(args, c"li", &mut i, &mut step) {
        return long(-1);
    }
    match control(i) {
        Some(id) if is_spinner(id) && step < gui().spinner_mut(id).max_value => {
            gui().spinner_mut(id).step = step;
            long(1)
        }
        _ => long(-1),
    }
}

unsafe extern "C" fn set_range(_: Obj, args: Obj) -> R {
    let (mut i, mut lo, mut hi, mut n) = (0 as c_long, 0f32, 0f32, 300 as c_int);
    if !parse!(args, c"lff|i", &mut i, &mut lo, &mut hi, &mut n) {
        return long(-1);
    }
    let g = gui();
    if let Some(id) = control(i).filter(|id| is_spinner(*id)) {
        g.spinner_mut(id).min_value = lo as i32;
        g.spinner_mut(id).max_value = hi as i32;
        return long(1);
    }
    if let Some(id) = control(i).filter(|id| is_progress_bar(*id)) {
        g.progress_bar_mut(id).min_value = lo;
        g.progress_bar_mut(id).max_value = hi;
        return long(1);
    }
    if let Some(id) = control(i).filter(|id| is_graph(*id)) {
        let graph = g.graph_mut(id);
        graph.min_y = lo;
        graph.max_y = hi;
        graph.max_values_count = n as _;
        return long(1);
    }
    long(-1)
}

unsafe extern "C" fn set_value(_: Obj, args: Obj) -> R {
    let (mut i, mut v) = (0 as c_long, 0f32);
    if !parse!(args, c"lf", &mut i, &mut v) {
        return long(-1);
    }
    let g = gui();
    if let Some(id) = control(i).filter(|id| is_progress_bar(*id)) {
        let bar = g.progress_bar_mut(id);
        if bar.min_value <= v && v <= bar.max_value {
            bar.value = v;
            return long(1);
        }
        return long(-1);
    }
    if let Some(id) = control(i).filter(|id| is_spinner(*id)) {
        let s = g.spinner_mut(id);
        if s.min_value as f32 <= v && v <= s.max_value as f32 {
            g.spinner_set_value(id, v as i32);
            return long(1);
        }
        return long(-1);
    }
    if let Some(id) = control(i).filter(|id| is_check_box(*id)) {
        g.set_check(id, v >= 1.0);
        return long(1);
    }
    long(-1)
}

unsafe extern "C" fn get_value(_: Obj, args: Obj) -> R {
    let mut i = 0 as c_long;
    if !parse!(args, c"l", &mut i) {
        return long(-1);
    }
    let g = gui();
    if let Some(id) = control(i).filter(|id| is_progress_bar(*id)) {
        return float(g.progress_bar_mut(id).value as f64);
    }
    if let Some(id) = control(i).filter(|id| is_spinner(*id)) {
        return float(g.spinner_mut(id).value as f64);
    }
    long(-1)
}

unsafe extern "C" fn set_focus(_: Obj, args: Obj) -> R {
    let (mut i, mut v) = (0 as c_long, 0 as c_long);
    if !parse!(args, c"ll", &mut i, &mut v) {
        return long(-1);
    }
    match control(i) {
        Some(id) if is_text_input(id) => {
            gui().text_input_mut(id).has_focus = v >= 1;
            long(1)
        }
        _ => long(-1),
    }
}

// ---- drawing (in a render callback: form-relative) ----

fn offset() -> (f32, f32) {
    state(|s| s.offset)
}

unsafe extern "C" fn gl_begin(_: Obj, args: Obj) -> R {
    let mut v = 0 as c_long;
    if !parse!(args, c"l", &mut v) {
        return long(-1);
    }
    if let Some(p) = painter() {
        p.gl_begin(GRAPHICS_GL, v);
    }
    long(1)
}

unsafe extern "C" fn gl_end(_: Obj, _args: Obj) -> R {
    if let Some(p) = painter() {
        p.gl_end(GRAPHICS_GL);
    }
    float(1.0)
}

unsafe extern "C" fn gl_vertex2f(_: Obj, args: Obj) -> R {
    let (mut x, mut y) = (0f32, 0f32);
    if !parse!(args, c"ff", &mut x, &mut y) {
        return long(-1);
    }
    let o = offset();
    if let Some(p) = painter() {
        p.gl_vertex3f(GRAPHICS_GL, o.0 + x, o.1 + y, 0.0);
    }
    long(1)
}

unsafe extern "C" fn gl_color3f(_: Obj, args: Obj) -> R {
    let (mut r, mut g, mut b) = (0f32, 0f32, 0f32);
    if !parse!(args, c"fff", &mut r, &mut g, &mut b) {
        return long(-1);
    }
    if let Some(p) = painter() {
        p.gl_color3f(GRAPHICS_GL, r, g, b);
    }
    long(1)
}

unsafe extern "C" fn gl_color4f(_: Obj, args: Obj) -> R {
    let (mut r, mut g, mut b, mut a) = (0f32, 0f32, 0f32, 0f32);
    if !parse!(args, c"ffff", &mut r, &mut g, &mut b, &mut a) {
        return long(-1);
    }
    if let Some(p) = painter() {
        p.gl_color4f(GRAPHICS_GL, r, g, b, a);
    }
    long(1)
}

unsafe extern "C" fn gl_quad(_: Obj, args: Obj) -> R {
    let (mut x, mut y, mut w, mut h) = (0f32, 0f32, 0f32, 0f32);
    if !parse!(args, c"ffff", &mut x, &mut y, &mut w, &mut h) {
        return long(-1);
    }
    let o = offset();
    if let Some(p) = painter() {
        p.gl_quad(GRAPHICS_GL, o.0 + x, o.1 + y, w, h, false);
    }
    long(1)
}

unsafe extern "C" fn gl_quad_textured(_: Obj, args: Obj) -> R {
    let (mut x, mut y, mut w, mut h, mut t) = (0f32, 0f32, 0f32, 0f32, 0 as c_long);
    if !parse!(args, c"ffffl", &mut x, &mut y, &mut w, &mut h, &mut t) {
        return long(-1);
    }
    let Some(tex) = crate::texture(t) else { return long(-1) };
    let o = offset();
    if let Some(p) = painter() {
        p.set_texture(0, &tex);
        p.gl_quad(GRAPHICS_GL, o.0 + x, o.1 + y, w, h, true);
    }
    long(1)
}

unsafe extern "C" fn new_texture(_: Obj, args: Obj) -> R {
    let mut o = NUL;
    if !parse!(args, c"O", &mut o) {
        return long(-1);
    }
    long(crate::load_texture(&text(o)))
}

// ---- the cars ----

unsafe extern "C" fn get_car_state(_: Obj, args: Obj) -> R {
    let (mut c, mut what, mut w) = (0 as c_long, 0 as c_long, 0 as c_long);
    if !parse!(args, c"ll|l", &mut c, &mut what, &mut w) {
        return long(-1);
    }
    let w = if w.abs() > 4 { 0 } else { w };
    let Some(ci) = car(c) else { return long(-1) };
    let link = &gui().apps;
    let car = &link.cars[ci];
    let s = &car.state;
    let x = info(ci);
    let ix = w as usize;
    let f = |v: f32| float(v as f64);
    match what {
        0 => f(s.speed),
        1 => f(s.speed * 2.236_936_3),
        2 => f(s.speed * 3.6),
        3 => f(s.gas),
        4 => f(s.brake),
        5 => f(s.clutch),
        6 => long(s.gear),
        7 if (0..=2).contains(&w) => f(s.aero[ix]),
        7 | 8 => long(car.best_lap.time as i32),
        9 => floats(&s.camber_rad),
        10 => floats(&s.acc_g),
        11 => f(s.cg_height),
        12 | 13 => float(0.0),
        14 => f(s.drift_points),
        15 => f(s.drivetrain_speed),
        16 => floats(&s.dy),
        17 => f(s.engine_rpm),
        18 => floats(&s.load),
        19 => f(s.instant_drift),
        20 => long((s.is_drift_valid as u8 == 0) as i32),
        21 => long(s.is_engine_limiter_on as i32),
        22 => long(car.lap_count as i32),
        23 => long(0),
        24 => long(car.current_lap.time as i32),
        25 => {
            let fc = link.focused();
            let gain = fc.user_ff_gain.unwrap_or(1.0);
            f(fc.ff_global_gain.map_or(0.0, |g| g * (fc.state.last_ff_pure.abs() * gain)))
        }
        26 => long(car.last_lap.time as i32),
        27 => floats(&s.local_angular_velocity),
        28 => floats(&s.local_velocity),
        29 => floats(&s.mz),
        30 => floats(&s.nd_slip),
        31 => f(s.normalized_spline_position),
        32 => f(s.performance_meter),
        33 => floats(&s.slip_angle),
        34 => floats(&[0.0; 4]),
        35 => floats(&s.slip_ratio),
        36 => floats(&[s.speed * 3.6, s.speed * 2.236_936_3, s.speed]),
        37 => f(s.steer),
        38 => floats(&s.suspension_travel),
        39 => f(s.turbo_boost),
        40 => floats(&s.tyre_dirty_level),
        41 => floats(&s.tyre_contact_normal[ix.min(3)]),
        42 => floats(&s.tyre_contact_point[ix.min(3)]),
        44 => floats(&s.tyre_loaded_radius),
        45 => floats(&s.tyre_radius),
        47 => floats(&s.tyre_slip),
        50 => floats(&s.velocity),
        51 => floats(&s.wheel_angular_speed),
        52 => floats(&[s.world_matrix.m[3][0], s.world_matrix.m[3][1], s.world_matrix.m[3][2]]),
        53 => float(x.caster),
        54 => floats(&s.tyre_thermal_states.map(|t| t.core_temp)),
        55 => floats(&s.tyre_thermal_states[ix.min(3)].last_set_imo),
        56 => floats(&s.tyre_thermal_states.map(|t| t.dynamic_pressure)),
        57 => floats(&s.ride_height),
        58 => f(toe_deg(car, ix.min(3))),
        59 => floats(&s.camber_rad.map(|c| c * 57.295_78)),
        60 => f(s.kers_charge),
        61 => f(s.kers_input),
        62 => long(((s.status_bytes >> 1) & 1) as i32),
        63 => long(((s.status_bytes >> 2) & 1) as i32),
        64 => long(car.engine_brake_setting),
        65 => long(if car.has_ers { car.ers_recovery } else { 0 }),
        66 => long(if car.has_ers { x.ers_power } else { 0 }),
        67 => long(if car.has_ers { car.ers_heat_charging as i32 } else { 0 }),
        68 => f(s.kers_current_kj),
        69 => f(if car.has_ers {
            car.ers_max_j
        } else if car.has_kers {
            car.kers_max_j
        } else {
            0.0
        }),
        70 => long(x.completed as i32),
        71 => long(s.p2p_status as i32),
        72 => long(s.p2p_activations as i32),
        _ => long(-1),
    }
}

/// `SetupInformation::getTOE`'s sum for one wheel (`ToeInDeg`): the y of the suspension's
/// direction in the body is dropped before it is normalised.
fn toe_deg(car: &rustyac_gui::apps::AppCar, wheel: usize) -> f32 {
    let m = &car.state.suspension_matrix[wheel].m;
    let v = [-m[2][0], -m[2][1], -m[2][2]];
    let i = rustyac_physics::vecmath::xm_matrix_inverse(&car.body_matrix).m;
    let x = (v[1] * i[1][0] + v[0] * i[0][0]) + v[2] * i[2][0];
    let z = (v[1] * i[1][2] + v[0] * i[0][2]) + v[2] * i[2][2];
    let l = (z * z + x * x).sqrt();
    let x = if l != 0.0 { x / l } else { x };
    rustyac_physics::math::asinf(x) * 57.295_78
}

/// A handler of one car index that answers from the car (-1: no such car).
unsafe fn by_car(args: Obj, f: impl FnOnce(usize) -> Obj) -> R {
    let mut c = 0 as c_long;
    if !parse!(args, c"l", &mut c) {
        return long(-1);
    }
    match car(c) {
        Some(i) => f(i),
        None => long(-1),
    }
}

fn app_car(i: usize) -> &'static rustyac_gui::apps::AppCar {
    &gui().apps.cars[i]
}

unsafe extern "C" fn get_car_min_height(_: Obj, args: Obj) -> R {
    // (the game has no check of the car here: a bad one crashes it)
    by_car(args, |i| info(i).min_height.map_or(long(-1), |h| float(h as f64)))
}
unsafe extern "C" fn is_car_in_pitlane(_: Obj, args: Obj) -> R {
    by_car(args, |i| long(app_car(i).in_pitlane as i32))
}
unsafe extern "C" fn is_car_in_pit(_: Obj, args: Obj) -> R {
    by_car(args, |i| long(app_car(i).in_pit as i32))
}
unsafe extern "C" fn is_connected(_: Obj, args: Obj) -> R {
    by_car(args, |i| long(app_car(i).connected as i32))
}
unsafe extern "C" fn get_car_ballast(_: Obj, args: Obj) -> R {
    by_car(args, |i| long(info(i).ballast_kg as i32))
}
unsafe extern "C" fn get_car_restrictor(_: Obj, args: Obj) -> R {
    by_car(args, |i| long(info(i).restrictor as i32))
}
unsafe extern "C" fn get_car_tyre_compound(_: Obj, args: Obj) -> R {
    let mut c = 0 as c_long;
    if !parse!(args, c"l", &mut c) {
        return unicode("");
    }
    unicode(&car(c).map(|i| app_car(i).compound_short_name.clone()).unwrap_or_default())
}
unsafe extern "C" fn is_ai_controlled(_: Obj, args: Obj) -> R {
    by_car(args, |i| long(info(i).ai_controlled as i32))
}
unsafe extern "C" fn get_car_engine_brake_count(_: Obj, args: Obj) -> R {
    by_car(args, |i| long(if app_car(i).has_ers { info(i).engine_brake_count } else { 0 }))
}
unsafe extern "C" fn get_car_power_controller_count(_: Obj, args: Obj) -> R {
    by_car(args, |i| long(if app_car(i).has_ers { info(i).power_controller_count } else { 0 }))
}

unsafe extern "C" fn get_car_ffb(_: Obj, _args: Obj) -> R {
    let gain = gui().apps.cars.first().and_then(|c| c.user_ff_gain).unwrap_or(0.0);
    long((gain * 100.0) as i32)
}

unsafe extern "C" fn set_car_ffb(_: Obj, args: Obj) -> R {
    let mut v = 0f32;
    if !parse!(args, c"f", &mut v) {
        return long(-1);
    }
    let delta = v * 0.01;
    let Some(car) = gui().apps.cars.first_mut() else { return long(0) };
    let mut gain = car.user_ff_gain.unwrap_or(0.0);
    if car.has_physics && delta != 0.0 {
        gain = (delta + gain).clamp(0.0, 200.0);
        car.user_ff_gain = Some(gain);
        state(|s| s.actions.push(Action::SetUserFfGain(gain)));
    }
    long(gain as i32)
}

unsafe extern "C" fn get_cars_count(_: Obj, _args: Obj) -> R {
    long(gui().apps.cars.len() as i32)
}

unsafe extern "C" fn get_car_leaderboard_position(_: Obj, args: Obj) -> R {
    by_car(args, |i| long(app_car(i).leaderboard_position))
}

unsafe extern "C" fn get_car_real_time_leaderboard_position(_: Obj, args: Obj) -> R {
    by_car(args, |i| long(app_car(i).realtime_position))
}

// ---- the camera ----

unsafe extern "C" fn set_camera_mode(_: Obj, args: Obj) -> R {
    let mut v = 0 as c_long;
    if !parse!(args, c"l", &mut v) {
        return long(-1);
    }
    state(|s| {
        s.world.camera_mode = if v == 7 { 3 } else { v };
        s.actions.push(Action::SetCameraMode(v));
    });
    long(1)
}

unsafe extern "C" fn get_camera_mode(_: Obj, _args: Obj) -> R {
    long(state(|s| s.world.camera_mode))
}

unsafe extern "C" fn is_camera_on_board(_: Obj, args: Obj) -> R {
    let mut c = 0 as c_long;
    if !parse!(args, c"l", &mut c) {
        return long(-1);
    }
    let focused = gui().apps.focused_car as i32;
    let on = state(|s| c == focused && (s.world.camera_mode == 0 || (s.world.camera_mode == 2 && s.world.drivable_dash)));
    (api().bool_from_long)(on as c_long)
}

unsafe extern "C" fn get_camera_car_count(_: Obj, args: Obj) -> R {
    by_car(args, |i| long(info(i).camera_count))
}

unsafe extern "C" fn set_camera_car(_: Obj, args: Obj) -> R {
    let (mut cam, mut c) = (0 as c_long, 0 as c_long);
    if !parse!(args, c"ll", &mut cam, &mut c) {
        return long(-1);
    }
    if let Some(i) = car(c) {
        // (online through the car's NetCarStateProvider: the player's own has none)
        if (cam as u32 as i64) < info(i).camera_count as i64 && (i != 0 || state(|s| s.world.server.is_none())) {
            state(|s| {
                s.world.camera_mode = 1;
                s.actions.push(Action::SetCameraCar(cam, c));
            });
        }
    }
    long(1)
}

unsafe extern "C" fn focus_car(_: Obj, args: Obj) -> R {
    let mut c = 0 as c_long;
    if !parse!(args, c"l", &mut c) {
        return long(-1);
    }
    match car(c) {
        Some(i) => {
            gui().apps.focused_car = i;
            state(|s| s.actions.push(Action::FocusCar(c)));
            long(1)
        }
        None => long(-1),
    }
}

unsafe extern "C" fn get_focused_car(_: Obj, _args: Obj) -> R {
    long(gui().apps.focused_car as i32)
}

unsafe extern "C" fn free_camera_set_clear_color(_: Obj, args: Obj) -> R {
    let (mut r, mut g, mut b, mut a) = (0f32, 0f32, 0f32, 0f32);
    if !parse!(args, c"ffff", &mut r, &mut g, &mut b, &mut a) {
        return long(-1);
    }
    state(|s| s.actions.push(Action::FreeCameraClearColor([r, g, b, a])));
    long(1)
}

unsafe fn free_camera(args: Obj, action: fn(f32) -> Action) -> R {
    let mut v = 0f32;
    if !parse!(args, c"f", &mut v) {
        return long(-1);
    }
    state(|s| s.actions.push(action(v)));
    long(1)
}

unsafe extern "C" fn free_camera_move_forward(_: Obj, args: Obj) -> R {
    free_camera(args, Action::FreeCameraMoveForward)
}
unsafe extern "C" fn free_camera_move_right(_: Obj, args: Obj) -> R {
    free_camera(args, Action::FreeCameraMoveRight)
}
unsafe extern "C" fn free_camera_move_up_world(_: Obj, args: Obj) -> R {
    free_camera(args, Action::FreeCameraMoveUpWorld)
}
unsafe extern "C" fn free_camera_rotate_pitch(_: Obj, args: Obj) -> R {
    free_camera(args, Action::FreeCameraRotatePitch)
}
unsafe extern "C" fn free_camera_rotate_heading(_: Obj, args: Obj) -> R {
    free_camera(args, Action::FreeCameraRotateHeading)
}
unsafe extern "C" fn free_camera_rotate_roll(_: Obj, args: Obj) -> R {
    free_camera(args, Action::FreeCameraRotateRoll)
}

// ---- names, the track, the session ----

unsafe extern "C" fn get_driver_name(_: Obj, args: Obj) -> R {
    by_car(args, |i| unicode(&app_car(i).driver_name))
}
unsafe extern "C" fn get_driver_nation_code(_: Obj, args: Obj) -> R {
    by_car(args, |i| unicode(&info(i).nation_code))
}
unsafe extern "C" fn get_track_name(_: Obj, args: Obj) -> R {
    by_car(args, |_| unicode(&gui().apps.track_name))
}
unsafe extern "C" fn get_track_configuration(_: Obj, args: Obj) -> R {
    by_car(args, |_| unicode(&gui().apps.track_config))
}
unsafe extern "C" fn get_track_length(_: Obj, _args: Obj) -> R {
    let l = gui().apps.ai_spline_length;
    float(if l > 0.0 { l as f64 } else { -1.0 })
}
unsafe extern "C" fn get_car_name(_: Obj, args: Obj) -> R {
    by_car(args, |i| unicode(&app_car(i).unix_name))
}
unsafe extern "C" fn get_car_skin(_: Obj, args: Obj) -> R {
    by_car(args, |i| unicode(&info(i).skin))
}

/// A lap's splits, padded with 0 to the track's sectors.
unsafe fn splits(args: Obj, lap: fn(&rustyac_gui::apps::AppCar) -> &rustyac_gui::leaderboard::RaceLap) -> R {
    by_car(args, |i| {
        let mut v: Vec<i32> = lap(app_car(i)).splits.iter().map(|s| *s as i32).collect();
        let n = state(|s| s.world.sector_count);
        while v.len() < n {
            v.push(0);
        }
        ints(&v)
    })
}

unsafe extern "C" fn get_last_splits(_: Obj, args: Obj) -> R {
    splits(args, |c| &c.last_lap)
}
unsafe extern "C" fn get_current_splits(_: Obj, args: Obj) -> R {
    splits(args, |c| &c.current_lap)
}

unsafe extern "C" fn is_ac_live(_: Obj, _args: Obj) -> R {
    let replay = gui().apps.in_replay;
    long((!replay && !state(|s| s.world.paused)) as i32)
}

unsafe extern "C" fn get_wind_speed(_: Obj, _args: Obj) -> R {
    long((gui().apps.wind_speed * 3.6) as i32)
}
unsafe extern "C" fn get_wind_direction(_: Obj, _args: Obj) -> R {
    long(gui().apps.wind_direction_deg as i32)
}

unsafe extern "C" fn shutdown(_: Obj, _args: Obj) -> R {
    println!("Shutdown through Python interface");
    state(|s| s.actions.push(Action::Shutdown));
    long(0)
}

#[link(name = "kernel32")]
extern "system" {
    fn WritePrivateProfileStringW(app: *const u16, key: *const u16, value: *const u16, file: *const u16) -> i32;
}

unsafe extern "C" fn restart(_: Obj, _args: Obj) -> R {
    println!("Restart through Python interface");
    let file = state(|s| s.documents.join("cfg").join("race.ini"));
    let z = |t: &str| t.encode_utf16().chain(Some(0)).collect::<Vec<u16>>();
    let path: Vec<u16> = std::os::windows::ffi::OsStrExt::encode_wide(file.as_os_str()).chain(Some(0)).collect();
    WritePrivateProfileStringW(z("RESTART").as_ptr(), z("ACTIVE").as_ptr(), z("1").as_ptr(), path.as_ptr());
    state(|s| s.actions.push(Action::Restart));
    long(0)
}

unsafe extern "C" fn get_server_name(_: Obj, _args: Obj) -> R {
    unicode(&state(|s| s.world.server.as_ref().map(|v| v.0.clone()).unwrap_or_default()))
}
unsafe extern "C" fn get_server_ip(_: Obj, _args: Obj) -> R {
    unicode(&state(|s| s.world.server.as_ref().map(|v| v.1.clone()).unwrap_or_default()))
}
unsafe extern "C" fn get_server_http_port(_: Obj, _args: Obj) -> R {
    long(state(|s| s.world.server.as_ref().map_or(0, |v| v.2)))
}
unsafe extern "C" fn get_server_slots_count(_: Obj, _args: Obj) -> R {
    long(state(|s| s.world.server.as_ref().map_or(0, |v| v.3)))
}

// ---- the module ----

type H = unsafe extern "C" fn(Obj, Obj) -> Obj;

/// The names `PythonInterface::PythonInterface` registers, in its order, and their handlers.
pub const FUNCTIONS: [(&std::ffi::CStr, H); 116] = [
    (c"log", log),
    (c"console", console),
    (c"newApp", new_app),
    (c"setTitle", set_title),
    (c"setPosition", set_position),
    (c"setIconPosition", set_icon_position),
    (c"setTitlePosition", set_title_position),
    (c"setText", set_text),
    (c"getText", get_text),
    (c"getPosition", get_position),
    (c"setSize", set_size),
    (c"getSize", get_size),
    (c"setVisible", set_visible),
    (c"setBackgroundColor", set_background_color),
    (c"setBackgroundOpacity", set_background_opacity),
    (c"drawBackground", draw_background),
    (c"drawBorder", draw_border),
    (c"setBackgroundTexture", set_background_texture),
    (c"setFontAlignment", set_font_alignment),
    (c"addRenderCallback", add_render_callback),
    (c"addOnClickedListener", add_on_clicked_listener),
    (c"addOnValidateListener", add_on_validate_listener),
    (c"addOnValueChangeListener", add_on_value_change_listener),
    (c"addOnCheckBoxChanged", add_on_check_box_changed),
    (c"addOnListBoxSelectionListener", add_on_list_box_listener),
    (c"addOnListBoxDeselectionListener", add_on_list_box_listener),
    (c"addOnAppActivatedListener", add_on_app_activated_listener),
    (c"addOnAppDismissedListener", add_on_app_dismissed_listener),
    (c"sendChatMessage", send_chat_message),
    (c"addOnChatMessageListener", add_on_chat_message_listener),
    (c"initFont", init_font),
    (c"setFont", set_font),
    (c"setFontColor", set_font_color),
    (c"setFontSize", set_font_size),
    (c"setCustomFont", set_custom_font),
    (c"addLabel", add_label),
    (c"addButton", add_button),
    (c"addCheckBox", add_check_box),
    (c"addGraph", add_graph),
    (c"addSerieToGraph", add_serie_to_graph),
    (c"addValueToGraph", add_value_to_graph),
    (c"addListBox", add_list_box),
    (c"addItem", add_item),
    (c"removeItem", list_box_ll),
    (c"getItemCount", list_box_l),
    (c"setItemNumberPerPage", list_box_ll),
    (c"highlightListBoxItem", list_box_ll),
    (c"setAllowDeselection", set_allow_deselection),
    (c"setAllowMultiSelection", list_box_ll),
    (c"getSelectedItems", list_box_l),
    (c"addTextBox", add_text_box),
    (c"addSpinner", add_spinner),
    (c"setStep", set_step),
    (c"addProgressBar", add_progress_bar),
    (c"setRange", set_range),
    (c"setValue", set_value),
    (c"getValue", get_value),
    (c"addTextInput", add_text_input),
    (c"setFocus", set_focus),
    (c"glBegin", gl_begin),
    (c"glEnd", gl_end),
    (c"glVertex2f", gl_vertex2f),
    (c"glColor3f", gl_color3f),
    (c"glColor4f", gl_color4f),
    (c"glQuad", gl_quad),
    (c"glQuadTextured", gl_quad_textured),
    (c"newTexture", new_texture),
    (c"getCarState", get_car_state),
    (c"getCarMinHeight", get_car_min_height),
    (c"isCarInPitline", is_car_in_pitlane),
    (c"isCarInPitlane", is_car_in_pitlane),
    (c"isCarInPit", is_car_in_pit),
    (c"isConnected", is_connected),
    (c"getCarBallast", get_car_ballast),
    (c"getCarRestrictor", get_car_restrictor),
    (c"getCarTyreCompound", get_car_tyre_compound),
    (c"isAIControlled", is_ai_controlled),
    (c"getCarEngineBrakeCount", get_car_engine_brake_count),
    (c"getCarPowerControllerCount", get_car_power_controller_count),
    (c"getCarFFB", get_car_ffb),
    (c"setCarFFB", set_car_ffb),
    (c"getCarsCount", get_cars_count),
    (c"getCarLeaderboardPosition", get_car_leaderboard_position),
    (c"getCarRealTimeLeaderboardPosition", get_car_real_time_leaderboard_position),
    (c"setCameraMode", set_camera_mode),
    (c"getCameraMode", get_camera_mode),
    (c"isCameraOnBoard", is_camera_on_board),
    (c"getCameraCarCount", get_camera_car_count),
    (c"setCameraCar", set_camera_car),
    (c"focusCar", focus_car),
    (c"getFocusedCar", get_focused_car),
    (c"freeCameraSetClearColor", free_camera_set_clear_color),
    (c"freeCameraMoveForward", free_camera_move_forward),
    (c"freeCameraMoveRight", free_camera_move_right),
    (c"freeCameraMoveUpWorld", free_camera_move_up_world),
    (c"freeCameraRotatePitch", free_camera_rotate_pitch),
    (c"freeCameraRotateHeading", free_camera_rotate_heading),
    (c"freeCameraRotateRoll", free_camera_rotate_roll),
    (c"getDriverName", get_driver_name),
    (c"getDriverNationCode", get_driver_nation_code),
    (c"getTrackName", get_track_name),
    (c"getTrackConfiguration", get_track_configuration),
    (c"getTrackLength", get_track_length),
    (c"getCarName", get_car_name),
    (c"getCarSkin", get_car_skin),
    (c"getLastSplits", get_last_splits),
    (c"getCurrentSplits", get_current_splits),
    (c"isAcLive", is_ac_live),
    (c"getWindSpeed", get_wind_speed),
    (c"getWindDirection", get_wind_direction),
    (c"shutdown", shutdown),
    (c"restart", restart),
    (c"getServerName", get_server_name),
    (c"getServerIP", get_server_ip),
    (c"getServerHttpPort", get_server_http_port),
    (c"getServerSlotsCount", get_server_slots_count),
];

/// `PyInit_emb` 0x1402cf4a0: `PyModule_Create2(&EmbModule, 1013)` (the tables live as long as
/// the process: the interpreter keeps pointers into them).
pub(crate) unsafe extern "C" fn init() -> Obj {
    let mut methods: Vec<MethodDef> = FUNCTIONS.iter().map(|(name, h)| MethodDef { name: name.as_ptr() as *const c_char, meth: Some(*h), flags: 1, doc: std::ptr::null() }).collect();
    methods.push(MethodDef { name: std::ptr::null(), meth: None, flags: 0, doc: std::ptr::null() });
    let methods = Box::leak(methods.into_boxed_slice());
    let def = Box::leak(Box::new(ModuleDef {
        refcnt: 1,
        ob_type: std::ptr::null_mut(),
        m_init: std::ptr::null_mut(),
        m_index: 0,
        m_copy: std::ptr::null_mut(),
        name: c"ac".as_ptr(),
        doc: std::ptr::null(),
        size: -1,
        methods: methods.as_mut_ptr(),
        reload: std::ptr::null_mut(),
        traverse: std::ptr::null_mut(),
        clear: std::ptr::null_mut(),
        free: std::ptr::null_mut(),
    }));
    (api().module_create2)(def, 1013)
}
