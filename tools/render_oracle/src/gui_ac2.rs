// SPDX-License-Identifier: GPL-3.0-or-later

//! The game side of the GUI oracle, part two: the classes and HUD objects added after the
//! base (their constructors, methods, state and events).

use std::sync::Mutex;

use crate::ac::{rd, wr, write_wstring, wstring, Game};
use crate::gui_ac::read_wstring;

const VA_KS_GET_TIME: usize = 0x1_4023_8a30; // double ksGetTime()
const VA_SCROLL_BAR_SET_VALUE: usize = 0x1_4025_c150;
const VA_SCROLL_BAR_SET_ITEMS_NUMBER: usize = 0x1_4025_c0a0;
const VA_SCROLL_BAR_SET_ITEMS_PER_PAGE: usize = 0x1_4025_c0b0;
const VA_TEXT_BOX_SET_FORMATTED_TEXT: usize = 0x1_4024_e6e0;
const VA_POP_OVER_RENDER_POP_OVER: usize = 0x1_4025_5a70;
const VA_POP_OVER_SET_LABEL_TITLE: usize = 0x1_4025_5cb0;
const VA_POP_OVER_SET_TEXT: usize = 0x1_4025_5d20;
const VA_TAB_BAR_ADD_TAB: usize = 0x1_4025_8120;
const VA_TAB_BAR_ORDER: usize = 0x1_4025_8500;
const VA_SYSTEM_MESSAGE_SET_MESSAGE: usize = 0x1_4025_aab0;

/// class, size, constructor, table
pub const CLASSES: [(&str, usize, usize, usize); 9] = [
    ("ScrollBar", 0x1c0, 0x1_4025_ae60, 0x1_404f_44b0),
    ("MovingBar", 0x200, 0x1_4025_ab60, 0x1_404f_4400),
    ("TextBox", 0x190, 0x1_4024_e330, 0x1_404f_2a08),
    ("TextInput", 0x1d0, 0x1_4024_5310, 0x1_404f_1380),
    ("PopOver", 0x188, 0x1_4025_5700, 0x1_404f_3840),
    ("ConnectedLabel", 0x1d8, 0x1_4044_e8a0, 0x1_4141_69a0),
    ("ConnectedInt", 0x1d8, 0x1_4044_e800, 0x1_4141_69a0),
    ("TabBar", 0x200, 0x1_4025_7990, 0x1_404f_3b20),
    ("DefaultSystemMessage", 0x1b0, 0x1_4025_a4a0, 0x1_404f_41a8),
];

/// The clock the game's `ksGetTime` answers with, in milliseconds.
static TIME_MS: Mutex<f64> = Mutex::new(0.0);
static TIME_CELL: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// The pop-overs drawn after the GUI in every frame (their owners' part in the game).
static POP_OVERS: Mutex<Vec<usize>> = Mutex::new(Vec::new());

pub unsafe fn static_initialisers(game: &Game) {
    // ksGetTime: mov rax, <cell>; movsd xmm0, [rax]; ret (a text input's caret blinks by it)
    let cell = game.acs.alloc(8);
    TIME_CELL.store(cell as usize, std::sync::atomic::Ordering::SeqCst);
    let mut code = vec![0x48, 0xb8];
    code.extend_from_slice(&(cell as usize).to_le_bytes());
    code.extend_from_slice(&[0xf2, 0x0f, 0x10, 0x00, 0xc3]);
    game.acs.patch(game.acs.va(VA_KS_GET_TIME), &code);
}

pub unsafe fn set_time(ms: f64) {
    *TIME_MS.lock().unwrap() = ms;
    let cell = TIME_CELL.load(std::sync::atomic::Ordering::SeqCst) as *mut u8;
    if !cell.is_null() {
        wr(cell, 0, ms);
    }
}

/// A class of this part made: the constructors take different things after the GUI.
pub unsafe fn new_object(game: &Game, gui: *mut u8, class: &str, name: &str, flag: bool) -> Result<*mut u8, String> {
    let acs = &game.acs;
    let class = if class == "ConnectedLabel" && !flag { "ConnectedInt" } else { class };
    let (_, size, ctor, _) = CLASSES.iter().find(|c| c.0 == class).ok_or(format!("the oracle knows no class {class:?}"))?;
    let object = acs.alloc(*size);
    let va = acs.va(*ctor);
    match class {
        "ScrollBar" => {
            let ctor: extern "C" fn(*mut u8, *mut u8, *mut u8, bool) -> *mut u8 = std::mem::transmute(va);
            ctor(object, wstring(acs, name), gui, flag);
        }
        "TextBox" => {
            let ctor: extern "C" fn(*mut u8, *mut u8, *mut u8, i32) -> *mut u8 = std::mem::transmute(va);
            ctor(object, wstring(acs, name), gui, flag as i32);
        }
        "ConnectedLabel" | "ConnectedInt" => {
            // the value lives in a cell of its own (the game's forms point at their members)
            let cell = acs.alloc(8);
            let ctor: extern "C" fn(*mut u8, *mut u8, *mut u8, *mut u8) -> *mut u8 = std::mem::transmute(va);
            ctor(object, wstring(acs, name), gui, cell);
        }
        "DefaultSystemMessage" => {
            let ctor: extern "C" fn(*mut u8, *mut u8) -> *mut u8 = std::mem::transmute(va);
            ctor(object, gui);
        }
        _ => {
            let ctor: extern "C" fn(*mut u8, *mut u8, *mut u8) -> *mut u8 = std::mem::transmute(va);
            ctor(object, wstring(acs, name), gui);
        }
    }
    Ok(object)
}

pub unsafe fn render_hud(game: &Game, _gui: *mut u8, dt: f32) {
    for pop_over in POP_OVERS.lock().unwrap().iter() {
        let render: extern "C" fn(*mut u8, f32) = std::mem::transmute(game.acs.va(VA_POP_OVER_RENDER_POP_OVER));
        render(*pop_over as *mut u8, dt);
    }
}

/// What a handler the oracle hangs on an event of a widget is called with: the `std::function`
/// object and the event's argument.
pub type Callback = unsafe extern "C" fn(*mut u8, *const u8);

static EVENTS: Mutex<Vec<String>> = Mutex::new(Vec::new());

fn record(text: String) {
    EVENTS.lock().unwrap().push(text);
}

unsafe fn name_of(control: *const u8) -> String {
    if control.is_null() {
        return String::from("<null>");
    }
    read_wstring(control.add(8))
}

/// `OnCheckBoxChanged { CheckBox*, bool }`.
unsafe extern "C" fn on_check_box(_this: *mut u8, e: *const u8) {
    record(format!("check \"{}\" {}", name_of(rd(e, 0)), rd::<u8>(e, 8)));
}

/// `OnSliderInteraction { Slider*, float }`.
unsafe extern "C" fn on_slider(_this: *mut u8, e: *const u8) {
    record(format!("slider \"{}\" {:08x}", name_of(rd(e, 0)), rd::<u32>(e, 8)));
}

/// `OnCutExtremesChanged { Slider*, float, float }`.
unsafe extern "C" fn on_cut(_this: *mut u8, e: *const u8) {
    record(format!("cut \"{}\" {:08x} {:08x}", name_of(rd(e, 0)), rd::<u32>(e, 8), rd::<u32>(e, 0xc)));
}

/// `OnScrollBarValueChanged { ScrollBar*, int }`.
unsafe extern "C" fn on_scroll(_this: *mut u8, e: *const u8) {
    record(format!("scroll \"{}\" {}", name_of(rd(e, 0)), rd::<u32>(e, 8)));
}

/// A text input's events are called with its text, a member of the control.
unsafe extern "C" fn on_validate(_this: *mut u8, text: *const u8) {
    record(format!("validate \"{}\" \"{}\"", name_of(text.sub(0x120)), read_wstring(text)));
}

unsafe extern "C" fn on_key(_this: *mut u8, text: *const u8) {
    record(format!("key \"{}\" \"{}\"", name_of(text.sub(0x120)), read_wstring(text)));
}

/// `OnCellSelected { TabBar*, uint old, uint new }`.
unsafe extern "C" fn on_tab(_this: *mut u8, e: *const u8) {
    record(format!("tab \"{}\" {} {}", name_of(rd(e, 0)), rd::<u32>(e, 8), rd::<u32>(e, 0xc)));
}

/// The events of a class besides `evClicked`: where the event is in the object, and the
/// handler that writes it down.
pub fn class_events(class: &str) -> Vec<(usize, Callback)> {
    match class {
        "CheckBox" => vec![(0x178, on_check_box as Callback)],
        "Slider" => vec![(0x178, on_slider as Callback), (0x190, on_cut as Callback)],
        "ScrollBar" => vec![(0x178, on_scroll as Callback)],
        "TextInput" => vec![(0x198, on_validate as Callback), (0x1b0, on_key as Callback)],
        "TabBar" => vec![(0x178, on_tab as Callback)],
        _ => Vec::new(),
    }
}

pub fn events() -> Vec<String> {
    EVENTS.lock().unwrap().drain(..).collect()
}

pub unsafe fn extra(_game: &Game, control: *const u8, class: &str) -> String {
    let bits = |offset: usize| rd::<u32>(control, offset);
    match class {
        "ScrollBar" => format!(" scroll={},{},{},{} dressed={}", bits(0x1a4), bits(0x1a8), bits(0x1ac), bits(0x1b0), rd::<u8>(control, 0x1a0)),
        "MovingBar" => format!(" pressed={:08x},{:08x}", bits(0x1f8), bits(0x1fc)),
        "TextBox" => {
            let (begin, end): (*const u8, *const u8) = (rd(control, 0x178), rd(control, 0x180));
            let count = if begin.is_null() { 0 } else { end.offset_from(begin) as usize / 0x20 };
            let lines: Vec<String> = (0..count).map(|i| read_wstring(begin.add(i * 0x20))).collect();
            format!(" lines={} \"{}\"", count, lines.join("|"))
        }
        "TextInput" => format!(" focus={} max={}", rd::<u8>(control, 0x1c8), bits(0x1cc)),
        "ConnectedLabel" | "ConnectedInt" => format!(" precision={}", rd::<i32>(control, 0x1c0)),
        "TabBar" => format!(" tabs={:08x},{},{} mouse={},{}", bits(0x190), bits(0x194), bits(0x198), rd::<i32>(control, 0x1f8), rd::<i32>(control, 0x1fc)),
        _ => String::new(),
    }
}

pub unsafe fn call(game: &Game, _gui: *mut u8, object: *mut u8, what: &str, number: f32, text: &str) -> Result<(), String> {
    let acs = &game.acs;
    let with_u32 = |va: usize| {
        let f: extern "C" fn(*mut u8, u32) = std::mem::transmute(acs.va(va));
        f(object, number as u32)
    };
    let with_text = |va: usize| {
        let f: extern "C" fn(*mut u8, *mut u8) = std::mem::transmute(acs.va(va));
        f(object, wstring(acs, text))
    };
    match what {
        "sbValue" => with_u32(VA_SCROLL_BAR_SET_VALUE),
        "sbItems" => with_u32(VA_SCROLL_BAR_SET_ITEMS_NUMBER),
        "sbPerPage" => with_u32(VA_SCROLL_BAR_SET_ITEMS_PER_PAGE),
        "repeat" => {
            let table: *const usize = rd(object, 0);
            let set: extern "C" fn(*mut u8, f32) = std::mem::transmute(*table.add(0x80 / 8));
            set(object, number);
        }
        "formatted" => with_text(VA_TEXT_BOX_SET_FORMATTED_TEXT),
        "popTitle" => with_text(VA_POP_OVER_SET_LABEL_TITLE),
        "popText" => with_text(VA_POP_OVER_SET_TEXT),
        "drawPopOver" => {
            let mut list = POP_OVERS.lock().unwrap();
            list.retain(|p| *p != object as usize);
            if number != 0.0 {
                list.push(object as usize);
            }
        }
        "connect" => {
            // variableConnection: 0 a float, 1 an int
            if rd::<i32>(object, 0x178) == 0 {
                wr(rd::<*mut u8>(object, 0x1c8), 0, number);
            } else {
                wr(rd::<*mut u8>(object, 0x1d0), 0, number as i32);
            }
        }
        "suffix" => write_wstring(acs, object.add(0x1a0), text),
        "labelText" => write_wstring(acs, object.add(0x180), text),
        "precision" => wr(object, 0x1c0, number as i32),
        "addTab" => with_text(VA_TAB_BAR_ADD_TAB),
        "orderTabs" => {
            let count = (rd::<usize>(object, 0x1e8) - rd::<usize>(object, 0x1e0)) / 8;
            let order: extern "C" fn(*mut u8, u32, u32, bool) = std::mem::transmute(acs.va(VA_TAB_BAR_ORDER));
            order(object, number as u32, count as u32, false);
        }
        "message" => {
            let (title, description) = text.split_once('|').unwrap_or((text, ""));
            let set: extern "C" fn(*mut u8, *mut u8, *mut u8) = std::mem::transmute(acs.va(VA_SYSTEM_MESSAGE_SET_MESSAGE));
            set(object, wstring(acs, title), wstring(acs, description));
        }
        other => return Err(format!("the game side knows no call {other:?}")),
    }
    Ok(())
}
