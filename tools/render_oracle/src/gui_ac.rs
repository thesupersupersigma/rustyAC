// SPDX-License-Identifier: GPL-3.0-or-later

//! The game's side of the GUI oracle: acs.exe's own `ksgui::GUI`, `GameScreen`, forms and
//! widgets and its `Font` (FW1FontWrapper and DirectWrite inside it), built by address on the
//! WARP device and driven by a script of `gui_seq`.
//!
//! In place of what the game has around its GUI: a zeroed `KeyboardManager` (the GUI only
//! adds itself to its list), the mouse and key events handed to the GUI's own handlers as the
//! window would, and one more handler hung on every control's `evClicked` that writes the
//! click down. The layout file goes to the scratch root's stand-in Documents folder.

use std::collections::HashSet;
use std::sync::Mutex;

use crate::ac::{rd, wr, wstring, Game};
use crate::gui_seq::{Op, Outcome, Sequence, Val};
use crate::Args;

const VA_GRAPHICS_BEGIN_SCENE: usize = 0x1_4020_2580; // GraphicsManager::beginScene()
const VA_GRAPHICS_END_SCENE: usize = 0x1_4020_2850; // GraphicsManager::endScene()
const VA_GRAPHICS_SET_SCREEN_SPACE_MODE: usize = 0x1_4020_48e0; // GraphicsManager::setScreenSpaceMode()
const VA_FONT_WRAPPERS_INIT: usize = 0x1_4001_0f60; // dynamic initializer for Font::fontWrappers
const VA_FONT_CTOR: usize = 0x1_4020_0a60; // Font::Font(const std::wstring&, eFontType, float, bool, bool)
const VA_FONT_BLIT_STRING: usize = 0x1_4020_0e20; // Font::blitString(float, float, const std::wstring&, float, eFontAlign)
const VA_FONT_SET_COLOR: usize = 0x1_4020_15b0; // Font::setColor(float, float, float, float)
const VA_GUI_CTOR: usize = 0x1_4025_1ac0; // ksgui::GUI::GUI(GraphicsManager*, const std::wstring&, KeyboardManager*)
const VA_GUI_ADD_CONTROL: usize = 0x1_4025_2e50; // ksgui::GUI::addControl(Control*)
const VA_GUI_RENDER: usize = 0x1_4025_4c10; // ksgui::GUI::render(float)
const VA_GUI_ON_MOUSE_DOWN: usize = 0x1_4025_4290;
const VA_GUI_ON_MOUSE_MOVE: usize = 0x1_4025_4320;
const VA_GUI_ON_MOUSE_UP: usize = 0x1_4025_45c0;
const VA_GUI_ON_MOUSE_WHEEL: usize = 0x1_4025_4720;
const VA_GUI_ON_KEY_DOWN: usize = 0x1_4025_4230;
const VA_GUI_ON_KEY_CHAR: usize = 0x1_4025_41e0;
const VA_GUI_SERIALIZE_FORMS: usize = 0x1_4025_4d60; // ksgui::GUI::serializeForms(std::wstring)
const VA_CONTROL_SET_POSITION: usize = 0x1_4024_50a0;
const VA_CONTROL_SET_VISIBLE: usize = 0x1_4024_51a0;
const VA_FORM_SET_AUTO_HIDE: usize = 0x1_4024_6a30;
const VA_FORM_SET_BLOCKED: usize = 0x1_4024_6ae0;
const VA_TASKBAR_CYCLE_DESKTOP: usize = 0x1_4025_9df0;
const VA_GAME_SCREEN_SHOW_DESKTOP: usize = 0x1_4025_0e70;
const VA_BUTTON_SET_MULTIPLE_TEXTURE: usize = 0x1_4024_b700;
const VA_BUTTON_SET_SELECTED: usize = 0x1_4024_ba80;
const VA_CHECK_BOX_SET_CHECK: usize = 0x1_4024_ad40;
const VA_SLIDER_MARKER_LEFT: usize = 0x1_4025_0940;
const VA_SLIDER_MARKER_RIGHT: usize = 0x1_4025_09e0;
const VA_SLIDER_UPDATE_MARKERS: usize = 0x1_4025_0a90;
const VA_SLIDER_RESET_MARKERS: usize = 0x1_4025_0810;

/// class, size, constructor, table (the class of an object the game made itself is told by its table)
pub const CLASSES: [(&str, usize, usize, usize); 10] = [
    ("Control", 0x178, 0x1_4024_4150, 0x1_404f_1210),
    ("Label", 0x180, 0x1_4024_ad70, 0x1_404f_20a0),
    ("Form", 0x1b8, 0x1_4024_5890, 0x1_404f_14d0),
    ("GameScreen", 0x1b0, 0x1_4025_0bd0, 0x1_404f_3070),
    ("Taskbar", 0x268, 0x1_4025_8a20, 0x1_404f_3e80),
    ("TaskBarIcon", 0x218, 0x1_4025_c1e0, 0x1_404f_48d8),
    ("ActiveButton", 0x240, 0x1_4024_b160, 0x1_404f_21f0),
    ("ProgressBar", 0x188, 0x1_4024_def0, 0x1_404f_28c0),
    ("CheckBox", 0x1a8, 0x1_4024_a870, 0x1_404f_1f38),
    ("Slider", 0x1c8, 0x1_4024_f9f0, 0x1_404f_2db0),
];

static EVENTS: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// An MSVC `std::wstring` of the game, read.
pub unsafe fn read_wstring(at: *const u8) -> String {
    let (size, capacity): (usize, usize) = (rd(at, 0x10), rd(at, 0x18));
    let data = if capacity >= 8 { rd::<*const u16>(at, 0) } else { at as *const u16 };
    if data.is_null() || size > 1 << 20 {
        return String::from("<bad string>");
    }
    String::from_utf16_lossy(std::slice::from_raw_parts(data, size))
}

/// Every control the oracle hooked.
static KNOWN: Mutex<Option<HashSet<usize>>> = Mutex::new(None);

/// The body of our handler on `evClicked` (slot 2 of a `std::function`'s implementation; the
/// object of a control's click recorder has the control at +8). A click whose event names no
/// control the oracle knows is one an app made itself with an event it did not fill in
/// (`DriverEyesPositioner::render` clicks its held arrows again every frame).
unsafe extern "C" fn click_recorder(this: *mut u8, event: *const u8) {
    let control: *const u8 = rd(event, 0);
    let own: *const u8 = rd(this, 8);
    if !own.is_null() && !control.is_null() && !KNOWN.lock().unwrap().as_ref().is_some_and(|k| k.contains(&(control as usize))) {
        EVENTS.lock().unwrap().push(format!("click \"{}\" by the app itself", read_wstring(own.add(8))));
        return;
    }
    let (x, y): (i32, i32) = (rd(event, 8), rd(event, 0xc));
    let name = if control.is_null() { String::from("<null>") } else { read_wstring(control.add(8)) };
    if crate::gui_ac_esc::synthetic() {
        EVENTS.lock().unwrap().push(format!("click \"{name}\" by the menu itself"));
        return;
    }
    EVENTS.lock().unwrap().push(format!("click \"{name}\" {x} {y}"));
}

unsafe extern "C" fn not_called() {
    eprintln!("oracle: the game copied or destroyed the oracle's event handler");
    std::process::abort();
}

pub unsafe fn class_of(game: &Game, object: *const u8) -> &'static str {
    let table: usize = rd(object, 0);
    CLASSES.iter().chain(crate::gui_ac2::CLASSES.iter()).chain(crate::gui_ac_menu::CLASSES.iter()).chain(crate::gui_ac_setup::CLASSES.iter()).chain(crate::gui_ac_apps::classes().iter()).find(|c| game.acs.va(c.3) == table).map(|c| c.0).unwrap_or("?")
}

pub unsafe fn children(object: *const u8) -> Vec<*mut u8> {
    let (begin, end): (*const *mut u8, *const *mut u8) = (rd(object, 0xa0), rd(object, 0xa8));
    if begin.is_null() {
        return Vec::new();
    }
    (0..end.offset_from(begin) as usize).map(|i| *begin.add(i)).collect()
}

pub unsafe fn dump_control(game: &Game, control: *const u8, depth: usize, out: &mut String) {
    let class = class_of(game, control);
    let bits = |offset: usize| rd::<u32>(control, offset);
    let float = |offset: usize| rd::<f32>(control, offset);
    let four = |offset: usize| rd::<[f32; 4]>(control, offset);
    let flag = |offset: usize| rd::<u8>(control, offset);
    let extra = match class {
        "Form" => format!(" form={}{}{} save={:08x} step={:08x}", flag(0x180), flag(0x181), flag(0x182), bits(0x184), bits(0x1b4)),
        "ActiveButton" => format!(" button={} sel={} multi={} active={} label=\"{}\"", rd::<i32>(control, 0x1c0), flag(0x1e8), flag(0x1e9), flag(0x1c6), read_wstring(control.add(0x1c8))),
        "ProgressBar" => format!(" progress={:08x},{:08x},{:08x},{}{}", bits(0x178), bits(0x17c), bits(0x180), flag(0x184), flag(0x185)),
        "CheckBox" => format!(" check={}", flag(0x190)),
        "Slider" => format!(" slider={:08x}", bits(0x1a8)),
        "TaskBarIcon" => format!(" icon={} target={:08x}", rd::<i32>(control, 0x178), bits(0x210)),
        "Taskbar" => format!(" bar={},{},{},{}", rd::<i32>(control, 0x178), flag(0x17c), rd::<i32>(control, 0x230), rd::<i16>(control, 0x260)),
        "GameScreen" => {
            let (begin, end): (usize, usize) = (rd(control, 0x198), rd(control, 0x1a0));
            format!(" screen={} hidden={}", flag(0x178), (end - begin) / 4)
        }
        other => match crate::gui_ac_menu::extra(control, other) {
            Some(extra) => extra,
            None => crate::gui_ac2::extra(game, control, other),
        },
    };
    out.push_str(&crate::gui_seq::control_line(
        depth,
        &crate::gui_seq::ControlState {
            class,
            name: read_wstring(control.add(8)),
            rect: four(0x28),
            rect_base: four(0x168),
            visible: flag(0x150) != 0,
            is_mouse_down: flag(0x151) != 0,
            highlight: flag(0x78) != 0,
            is_highlight: flag(0x140) != 0,
            draw_border: flag(0x110) != 0,
            draw_background: flag(0x111) != 0,
            text: read_wstring(control.add(0x120)),
            font_scale: float(0x118),
            scale_mult: float(0x15c),
            font_align: rd::<i32>(control, 0xc0),
            colors: [four(0x38), four(0x48), four(0x58), four(0x68), four(0x7c)],
            has_texture: rd::<usize>(control, 0xe8) != 0,
            repeat: (float(0x114), float(0x154), flag(0x158) != 0),
            extra,
        },
    ));
    if class == "TaskBarIcon" {
        // the title box is not a child: the icon draws it itself
        dump_control(game, rd::<*const u8>(control, 0x200), depth + 1, out);
    }
    for child in children(control) {
        dump_control(game, child, depth + 1, out);
    }
}

struct Side<'a> {
    game: &'a Game,
    gui: *mut u8,
    screen: *mut u8,
    objects: Vec<*mut u8>,
    hooked: HashSet<usize>,
    /// our `std::function` implementations, by the function each calls
    recorders: std::collections::HashMap<usize, *mut u8>,
    fonts: Vec<*mut u8>,
    blits: Vec<(usize, f32, f32, &'static str, f32, i32, [f32; 4], bool)>,
}

impl Side<'_> {
    unsafe fn vcall<F: Copy>(&self, object: *mut u8, offset: usize) -> F {
        let table: *const usize = rd(object, 0);
        std::mem::transmute_copy(&*table.add(offset / 8))
    }

    fn va<F: Copy>(&self, va: usize) -> F {
        unsafe { std::mem::transmute_copy(&self.game.acs.va(va)) }
    }

    unsafe fn class_of(&self, object: *const u8) -> &'static str {
        class_of(self.game, object)
    }

    unsafe fn children(&self, object: *const u8) -> Vec<*mut u8> {
        children(object)
    }

    unsafe fn new_object(&mut self, class: &str, name: &str, flag: bool) -> Result<*mut u8, String> {
        let acs = &self.game.acs;
        let Some((_, size, ctor, _)) = CLASSES.iter().find(|c| c.0 == class) else {
            return crate::gui_ac2::new_object(self.game, self.gui, class, name, flag);
        };
        let object = acs.alloc(*size);
        match class {
            "Form" | "Slider" => {
                let ctor: extern "C" fn(*mut u8, *mut u8, *mut u8, bool) -> *mut u8 = self.va(*ctor);
                ctor(object, wstring(acs, name), self.gui, flag);
            }
            _ => {
                let ctor: extern "C" fn(*mut u8, *mut u8, *mut u8) -> *mut u8 = self.va(*ctor);
                ctor(object, wstring(acs, name), self.gui);
            }
        }
        Ok(object)
    }

    /// Our handler at the end of every control's `evClicked` (a vector of 0x28-byte entries: a
    /// key and a `std::function`, whose implementation pointer is the entry's last word).
    unsafe fn hook(&mut self, control: *mut u8) {
        if !self.hooked.insert(control as usize) {
            return;
        }
        KNOWN.lock().unwrap().get_or_insert_with(HashSet::new).insert(control as usize);
        self.hook_event(control, 0xc8, click_recorder);
        for (offset, callback) in crate::gui_ac2::class_events(self.class_of(control)) {
            self.hook_event(control, offset, callback);
        }
    }

    /// An object whose table has `callback` where a `std::function` calls its target (slot 2).
    unsafe fn recorder(&mut self, callback: crate::gui_ac2::Callback) -> *mut u8 {
        let acs = &self.game.acs;
        *self.recorders.entry(callback as usize).or_insert_with(|| {
            let table = acs.alloc(6 * 8);
            for slot in 0..6 {
                wr(table, slot * 8, not_called as *const () as usize);
            }
            wr(table, 2 * 8, callback as *const () as usize);
            let recorder = acs.alloc(8);
            wr(recorder, 0, table);
            recorder
        })
    }

    unsafe fn hook_event(&mut self, control: *mut u8, at: usize, callback: crate::gui_ac2::Callback) {
        let mut recorder = self.recorder(callback);
        let acs = &self.game.acs;
        if at == 0xc8 && callback as *const () as usize == click_recorder as *const () as usize {
            // the click recorder's own object, with its control
            let own = acs.alloc(0x10);
            wr(own, 0, rd::<usize>(recorder, 0));
            wr(own, 8, control);
            recorder = own;
        }
        let (begin, end): (*mut u8, *mut u8) = (rd(control, at), rd(control, at + 8));
        let count = if begin.is_null() { 0 } else { end.offset_from(begin) as usize / 0x28 };
        let fresh = acs.alloc((count + 1) * 0x28);
        for i in 0..count {
            let (from, to) = (begin.add(i * 0x28), fresh.add(i * 0x28));
            std::ptr::copy_nonoverlapping(from, to, 0x28);
            // a small function object lives inside its entry: its pointer moves with it
            let implementation: usize = rd(from, 0x20);
            if implementation >= from as usize && implementation < from as usize + 0x28 {
                wr(to, 0x20, implementation - from as usize + to as usize);
            }
        }
        let entry = fresh.add(count * 0x28);
        wr(entry, 0, recorder);
        wr(entry, 0x20, recorder);
        wr(control, at, fresh);
        wr(control, at + 8, fresh.add((count + 1) * 0x28));
        wr(control, at + 16, fresh.add((count + 1) * 0x28));
    }

    unsafe fn hook_tree(&mut self, control: *mut u8) {
        self.hook(control);
        for child in self.children(control) {
            self.hook_tree(child);
        }
    }

    unsafe fn hook_all(&mut self) {
        let taskbar: *mut u8 = rd(self.gui, 0x98);
        self.hook_tree(taskbar);
        let (begin, end): (*const *mut u8, *const *mut u8) = (rd(self.gui, 0xa0), rd(self.gui, 0xa8));
        for i in 0..end.offset_from(begin) as usize {
            self.hook_tree(*begin.add(i));
        }
        // (controls of the game objects that draw and hear the mouse themselves)
        for root in crate::gui_ac_menu::extra_roots() {
            self.hook_tree(root);
        }
    }

    unsafe fn dump(&self, out: &mut String) {
        let g = self.gui;
        let captured: *const u8 = rd(g, 0xc8);
        let name = if captured.is_null() { String::from("-") } else { read_wstring(captured.add(8)) };
        out.push_str(&format!(
            "gui captured={name} at={:08x},{:08x} good={:08x},{:08x} move={} area={} mouse={:08x},{:08x} desktop={}\n",
            rd::<u32>(g, 0xd0),
            rd::<u32>(g, 0xd4),
            rd::<u32>(g, 0xd8),
            rd::<u32>(g, 0xdc),
            rd::<u8>(g, 0xe0),
            rd::<u8>(g, 0xe1),
            rd::<u32>(g, 0x158),
            rd::<u32>(g, 0x15c),
            rd::<i32>(g, 0x178),
        ));
        let (begin, end): (*const *mut u8, *const *mut u8) = (rd(g, 0xa0), rd(g, 0xa8));
        for i in 0..end.offset_from(begin) as usize {
            dump_control(self.game, *begin.add(i), 0, out);
        }
        dump_control(self.game, rd::<*const u8>(g, 0x98), 0, out);
        crate::gui_ac_hud::dump(self.game, out);
    }

    unsafe fn mouse(&mut self, va: usize, x: i32, y: i32, button: i32, delta: f32) {
        self.hook_all();
        let event = self.game.acs.alloc(16);
        wr(event, 0, x);
        wr(event, 4, y);
        wr(event, 8, button);
        wr(event, 12, delta);
        let handler: extern "C" fn(*mut u8, *mut u8) = self.va(va);
        handler(self.gui, event);
        // (the game objects that hang on the window's mouse events hear of it after the GUI)
        crate::gui_ac_menu::window_mouse(va, event);
    }

    unsafe fn op(&mut self, op: &Op) -> Result<(), String> {
        // (our handlers are on every event before anything can fire one)
        self.hook_all();
        let acs = &self.game.acs;
        match op {
            Op::New(class, name, flag) => {
                let object = self.new_object(class, name, *flag)?;
                self.objects.push(object);
            }
            Op::Child(parent, index) => {
                let child = *self.children(self.objects[*parent]).get(*index).ok_or("no such child")?;
                self.objects.push(child);
            }
            Op::AddScreen(control, taskbar) => {
                let add: extern "C" fn(*mut u8, *mut u8, bool) = self.vcall(self.screen, 0xa8);
                add(self.screen, self.objects[*control], *taskbar);
            }
            Op::Add(parent, child) => {
                let add: extern "C" fn(*mut u8, *mut u8) = self.vcall(self.objects[*parent], 0x10);
                add(self.objects[*parent], self.objects[*child]);
            }
            Op::Pos(id, x, y) => {
                let set: extern "C" fn(*mut u8, f32, f32) = self.va(VA_CONTROL_SET_POSITION);
                set(self.objects[*id], *x, *y);
            }
            Op::Size(id, w, h) => {
                let set: extern "C" fn(*mut u8, f32, f32) = self.vcall(self.objects[*id], 0x38);
                set(self.objects[*id], *w, *h);
            }
            Op::Text(id, text) => {
                let set: extern "C" fn(*mut u8, *mut u8) = self.vcall(self.objects[*id], 0x88);
                set(self.objects[*id], wstring(acs, text));
            }
            Op::Visible(id, visible) => {
                let set: extern "C" fn(*mut u8, bool) = self.va(VA_CONTROL_SET_VISIBLE);
                set(self.objects[*id], *visible);
            }
            Op::Set(id, name, value) => {
                let (offset, _) = crate::gui_seq::field(name)?;
                let object = self.objects[*id];
                match value {
                    Val::F(v) => wr(object, offset, *v),
                    Val::B(v) => wr(object, offset, *v as u8),
                    Val::I(v) => wr(object, offset, *v),
                    Val::C(v) => wr(object, offset, *v),
                }
            }
            Op::Call(id, what, number, text) => {
                let object = self.objects[*id];
                let with_bool = |va: usize| {
                    let f: extern "C" fn(*mut u8, bool) = self.va(va);
                    f(object, *number != 0.0)
                };
                let with_float = |va: usize| {
                    let f: extern "C" fn(*mut u8, f32) = self.va(va);
                    f(object, *number)
                };
                let plain = |va: usize, object: *mut u8| {
                    let f: extern "C" fn(*mut u8) = self.va(va);
                    f(object)
                };
                match *what {
                    "scaleByMult" => {
                        let scale: extern "C" fn(*mut u8, f32) = self.vcall(object, 0x98);
                        scale(object, *number);
                    }
                    "title" => {
                        let title: *mut u8 = rd(object, 0x178);
                        let set: extern "C" fn(*mut u8, *mut u8) = self.vcall(title, 0x88);
                        set(title, wstring(acs, text));
                    }
                    "autohide" => with_bool(VA_FORM_SET_AUTO_HIDE),
                    "blocked" => with_bool(VA_FORM_SET_BLOCKED),
                    "cycleDesktop" => plain(VA_TASKBAR_CYCLE_DESKTOP, rd(self.gui, 0x98)),
                    "showDesktop" => plain(VA_GAME_SCREEN_SHOW_DESKTOP, self.screen),
                    "multipleTexture" => {
                        let set: extern "C" fn(*mut u8, *mut u8) = self.va(VA_BUTTON_SET_MULTIPLE_TEXTURE);
                        set(object, wstring(acs, text));
                    }
                    "setSelected" => with_bool(VA_BUTTON_SET_SELECTED),
                    "setCheck" => with_bool(VA_CHECK_BOX_SET_CHECK),
                    "markerLeft" => with_float(VA_SLIDER_MARKER_LEFT),
                    "markerRight" => with_float(VA_SLIDER_MARKER_RIGHT),
                    "updateMarkers" => plain(VA_SLIDER_UPDATE_MARKERS, object),
                    "resetMarkers" => plain(VA_SLIDER_RESET_MARKERS, object),
                    other => return crate::gui_ac2::call(self.game, self.gui, object, other, *number, text),
                }
            }
            Op::MouseMove(x, y) => self.mouse(VA_GUI_ON_MOUSE_MOVE, *x, *y, -1, 0.0),
            Op::MouseDown(x, y) => self.mouse(VA_GUI_ON_MOUSE_DOWN, *x, *y, 0, 0.0),
            Op::MouseUp(x, y) => self.mouse(VA_GUI_ON_MOUSE_UP, *x, *y, 0, 0.0),
            Op::Wheel(x, y, delta) => self.mouse(VA_GUI_ON_MOUSE_WHEEL, *x, *y, -1, *delta),
            Op::KeyDown(code) => {
                self.hook_all();
                let event = acs.alloc(8);
                wr(event, 0, *code);
                let handler: extern "C" fn(*mut u8, *mut u8) = self.va(VA_GUI_ON_KEY_DOWN);
                handler(self.gui, event);
            }
            Op::KeyChar(code) => {
                self.hook_all();
                let handler: extern "C" fn(*mut u8, u32) = self.va(VA_GUI_ON_KEY_CHAR);
                handler(self.gui, *code);
            }
            Op::Font(kind, scale, italic, bold) => {
                let font = acs.alloc(0x20);
                let ctor: extern "C" fn(*mut u8, *const u8, i32, f32, bool, bool) -> *mut u8 = self.va(VA_FONT_CTOR);
                ctor(font, wstring(acs, ""), *kind, *scale, *italic, *bold);
                self.fonts.push(font);
            }
            Op::Blit(font, x, y, text, size, align, color, shadowed) => self.blits.push((*font, *x, *y, text, *size, *align, *color, *shadowed)),
            Op::ClearText => self.blits.clear(),
            Op::Hud(what, number, text) => crate::gui_ac_hud::call(self.game, self.gui, self.screen, what, *number, text)?,
            Op::Frame(_) | Op::Save => {}
        }
        Ok(())
    }
}

/// The scratch root as the sequence wants it: the game's GUI pictures and fonts, the layout
/// file (or none) and the gameplay settings where `INIReaderDocuments` looks for them.
pub fn prepare_root(args: &Args, sequence: &Sequence) -> Result<(), String> {
    crate::root::prepare(args)?;
    crate::gui_seq_setup::prepare_root(args, sequence)?;
    let to = args.root.join("content/fonts");
    std::fs::create_dir_all(&to).map_err(|e| e.to_string())?;
    let from = args.game.join("content/fonts");
    for entry in std::fs::read_dir(&from).map_err(|e| format!("{}: {e}", from.display()))?.filter_map(|e| e.ok()) {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("ttf")) && !to.join(entry.file_name()).is_file() {
            std::fs::copy(&path, to.join(entry.file_name())).map_err(|e| e.to_string())?;
        }
    }
    copy_tree(&args.game.join("content/gui"), &args.root.join("content/gui"))?;
    copy_tree(&args.game.join("system/locales"), &args.root.join("system/locales"))?;
    // the HUD's pictures besides the GUI's: a car's logo, the damage pictures, the wrong-way sign
    for file in ["content/cars/ks_ferrari_f2004/logo.png", "content/texture/wrongway.png", "content/texture/mini_finish.png", "content/cars/ks_ferrari_f2004/skins/00_official/livery.png", "content/cars/ks_ferrari_f2004/skins/01_ferrari_1/livery.png", "content/cars/ks_ferrari_f2004/skins/02_ferrari_2/livery.png"] {
        let (from, to) = (args.game.join(file), args.root.join(file));
        if from.is_file() && !to.is_file() {
            std::fs::create_dir_all(to.parent().unwrap_or(&args.root)).map_err(|e| e.to_string())?;
            std::fs::copy(&from, &to).map_err(|e| e.to_string())?;
        }
    }
    if args.game.join("content/texture/damage").is_dir() {
        copy_tree(&args.game.join("content/texture/damage"), &args.root.join("content/texture/damage"))?;
    }
    // the HUD's own settings: as the game ships them, or as the sequence wants them
    for name in ["name_displayer.ini", "damage_displayer.ini", "fades.ini", "pitstop.ini"] {
        std::fs::copy(args.game.join("system/cfg").join(name), args.root.join("system/cfg").join(name)).map_err(|e| format!("{name}: {e}"))?;
    }
    for (name, text) in sequence.cfg {
        std::fs::write(args.root.join("system/cfg").join(name), text).map_err(|e| format!("{name}: {e}"))?;
    }
    let gameplay = format!("[VIRTUAL_MIRROR]\nACTIVE=0\n\n[GUI]\nALLOW_OVERLAPPING_FORMS={}\n", sequence.allow_overlapping as i32);
    for cfg in [args.root.join("cfg"), args.root.join("documents/Assetto Corsa/cfg")] {
        std::fs::create_dir_all(&cfg).map_err(|e| e.to_string())?;
        std::fs::write(cfg.join("gameplay.ini"), &gameplay).map_err(|e| e.to_string())?;
        let acos = cfg.join("acos.ini");
        match sequence.acos {
            Some(text) => std::fs::write(&acos, text).map_err(|e| e.to_string())?,
            None => {
                if acos.is_file() {
                    std::fs::remove_file(&acos).map_err(|e| e.to_string())?;
                }
            }
        }
    }
    Ok(())
}

fn copy_tree(from: &std::path::Path, to: &std::path::Path) -> Result<(), String> {
    std::fs::create_dir_all(to).map_err(|e| e.to_string())?;
    for entry in std::fs::read_dir(from).map_err(|e| format!("{}: {e}", from.display()))?.filter_map(|e| e.ok()) {
        let target = to.join(entry.file_name());
        if entry.path().is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else if !target.is_file() {
            std::fs::copy(entry.path(), &target).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

pub fn run(args: &Args, sequence: &Sequence, name: &str) -> Result<Outcome, String> {
    prepare_root(args, sequence)?;
    std::env::set_current_dir(&args.root).map_err(|e| format!("{}: {e}", args.root.display()))?;
    let game = Game::start(args)?;
    let mut outcome = Outcome::default();
    unsafe {
        let acs = &game.acs;
        let call0 = |va: usize| {
            let f: extern "C" fn(*mut u8) = std::mem::transmute(acs.va(va));
            f(game.graphics)
        };
        let init: extern "C" fn() = std::mem::transmute(acs.va(VA_FONT_WRAPPERS_INIT));
        init();
        crate::gui_ac2::static_initialisers(&game);
        // Task 33: initLocalization's acLocaleSetLocale, before anything is made
        if let Some(language) = crate::gui_seq::language() {
            for va in [0x1_4001_1060usize, 0x1_4001_1070, 0x1_4001_1090, 0x1_4001_10c0, 0x1_4001_10f0] {
                let init: extern "C" fn() = std::mem::transmute(acs.va(va));
                init();
            }
            let set_locale: extern "C" fn(*mut u8) -> bool = std::mem::transmute(acs.va(0x1_4024_0980));
            if !set_locale(wstring(acs, &language)) {
                return Err(format!("acLocaleSetLocale({language}) failed"));
            }
        }

        // Game::Game: the GUI, with the keyboard manager it adds itself to
        let keyboard = acs.alloc(0x30);
        let gui = acs.alloc(0x190);
        let ctor: extern "C" fn(*mut u8, *mut u8, *mut u8, *mut u8) -> *mut u8 = std::mem::transmute(acs.va(VA_GUI_CTOR));
        ctor(gui, game.graphics, wstring(acs, "Assetto Corsa"), keyboard);

        let mut side = Side { game: &game, gui, screen: std::ptr::null_mut(), objects: Vec::new(), hooked: HashSet::new(), recorders: std::collections::HashMap::new(), fonts: Vec::new(), blits: Vec::new() };
        // Sim::Sim: the game screen
        side.screen = side.new_object("GameScreen", "SIM_GAME_SCREEN", false)?;
        let add: extern "C" fn(*mut u8, *mut u8) = std::mem::transmute(acs.va(VA_GUI_ADD_CONTROL));
        add(gui, side.screen);
        side.objects.push(side.screen);

        let blit: extern "C" fn(*mut u8, f32, f32, *const u8, f32, i32) = std::mem::transmute(acs.va(VA_FONT_BLIT_STRING));
        let set_color: extern "C" fn(*mut u8, f32, f32, f32, f32) = std::mem::transmute(acs.va(VA_FONT_SET_COLOR));
        let render: extern "C" fn(*mut u8, f32) = std::mem::transmute(acs.va(VA_GUI_RENDER));
        let mut frame = 0usize;
        for op in &sequence.ops {
            match op {
                Op::Frame(dt) => {
                    side.hook_all();
                    crate::gui_ac2::set_time(crate::gui_seq::time_ms(frame));
                    rustyac_render::gpulog::begin_capture(name);
                    // Game::onIdle 0x140242730: the scene (none here), the screen-space mode, the GUI
                    call0(VA_GRAPHICS_BEGIN_SCENE);
                    crate::gui_ac_hud::update(&game, *dt);
                    crate::gui_ac_apps::update();
                    call0(VA_GRAPHICS_SET_SCREEN_SPACE_MODE);
                    crate::gui_ac_menu::client_present(true);
                    crate::gui_ac_esc::sun(&game, true);
                    crate::gui_ac_apps::sun(&game, true);
                    if let Some(h) = crate::gui_ac_hud::hud() {
                        crate::gui_ac_replay::around_render(h, true);
                    }
                    render(gui, *dt);
                    if let Some(h) = crate::gui_ac_hud::hud() {
                        crate::gui_ac_replay::around_render(h, false);
                    }
                    crate::gui_ac_apps::sun(&game, false);
                    crate::gui_ac_esc::sun(&game, false);
                    crate::gui_ac_menu::client_present(false);
                    crate::gui_ac_hud::render_hud(&game, *dt);
                    crate::gui_ac2::render_hud(&game, gui, *dt);
                    for (font, x, y, text, size, align, color, shadowed) in &side.blits {
                        let font = side.fonts[*font];
                        set_color(font, color[0], color[1], color[2], color[3]);
                        wr(font, 0x4, *shadowed as u8);
                        blit(font, *x, *y, wstring(acs, text), *size, *align);
                    }
                    let capture = rustyac_render::gpulog::end_capture();
                    let (width, height, pixels) = game.read_back()?;
                    call0(VA_GRAPHICS_END_SCENE);
                    crate::gui_compare::frame_done(args, name, "ac", frame, &capture.text, capture.draws, width, height, &pixels, &mut outcome)?;
                    outcome.state.push_str(&format!("frame {frame}\n"));
                    for event in EVENTS.lock().unwrap().drain(..) {
                        outcome.state.push_str(&format!("event {event}\n"));
                    }
                    for event in crate::gui_ac2::events() {
                        outcome.state.push_str(&format!("event {event}\n"));
                    }
                    side.dump(&mut outcome.state);
                    frame += 1;
                }
                Op::Save => {
                    // the game's own writer, into the stand-in Documents folder of the scratch root
                    let file = crate::root::documents().join("Assetto Corsa/cfg/acos.ini");
                    let _ = std::fs::remove_file(&file);
                    let serialize: extern "C" fn(*mut u8, *mut u8) = std::mem::transmute(acs.va(VA_GUI_SERIALIZE_FORMS));
                    serialize(gui, wstring(acs, "Assetto Corsa"));
                    outcome.saved.push(std::fs::read(&file).map_err(|e| format!("{}: {e}", file.display()))?);
                }
                other => side.op(other)?,
            }
        }
    }
    Ok(outcome)
}
