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

const VA_LIST_BOX_ADD_ITEM: usize = 0x1_4024_95f0;
const VA_LIST_BOX_CLEAR: usize = 0x1_4024_96f0;
const VA_LIST_BOX_SELECT_ITEM: usize = 0x1_4024_a290;
const VA_LIST_BOX_SELECT_BY_NAME: usize = 0x1_4024_a3c0;
const VA_LIST_BOX_SCROLL_TO_TOP: usize = 0x1_4024_a250;
const VA_LIST_BOX_DRAW_CELLS: usize = 0x1_4024_97b0;
const VA_LIST_BOX_FONT_SIZE: usize = 0x1_4024_a5a0;
const VA_GRAPH_ADD_SERIE: usize = 0x1_4024_7620;
const VA_GRAPH_ADD_VALUE: usize = 0x1_4024_7700;
const VA_GRAPH_ADD_AXIS: usize = 0x1_4024_7610;
const VA_GRAPH_CLEAR_SERIE: usize = 0x1_4024_7760;
const VA_SPINNER_SET_VALUE: usize = 0x1_4024_de60;
const VA_SPINNER_SET_VALUE_ITEM: usize = 0x1_4024_dd30;
const VA_SPINNER_ADD_ITEM: usize = 0x1_4024_cb80;
const VA_SPINNER_SET_LABEL_TEXT: usize = 0x1_4024_dac0;
const VA_SPINNER_SET_DRAW_ARROWS: usize = 0x1_4024_d830;
const VA_SPINNER_SET_POSITION: usize = 0x1_4024_db30;

/// class, size, constructor, table
pub const CLASSES: [(&str, usize, usize, usize); 16] = [
    // DriverStatusSystemMessage: a plain control to the port, which draws it by hand
    ("Control", 0x228, 0x1_4008_e4a0, 0x1_404b_4168),
    ("ListBox", 0x218, 0x1_4024_8340, 0x1_404f_1cf0),
    ("ListBoxRow", 0x1a8, 0x1_4025_5dd0, 0x1_404f_39c8),
    ("Graph", 0x1b8, 0x1_4024_7170, 0x1_404f_1978),
    ("Spinner", 0x270, 0x1_4024_bac0, 0x1_404f_24d8),
    ("SpinnerMirrored", 0x270, 0x1_4024_bac0, 0x1_404f_24d8),
    ("CustomSpinner", 0x1d8, 0x1_4024_ead0, 0x1_404f_2b40),
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
        "ListBox" => {
            // six rows of three columns, or four of one
            let (rows, columns) = if flag { (4u32, 1u32) } else { (6, 3) };
            let ctor: extern "C" fn(*mut u8, *mut u8, u32, u32, *mut u8, bool) -> *mut u8 = std::mem::transmute(va);
            ctor(object, wstring(acs, name), rows, columns, gui, false);
        }
        "Spinner" | "SpinnerMirrored" => {
            let ctor: extern "C" fn(*mut u8, *mut u8, *mut u8, bool, bool) -> *mut u8 = std::mem::transmute(va);
            ctor(object, wstring(acs, name), gui, flag, class == "SpinnerMirrored");
        }
        "CustomSpinner" => {
            let ctor: extern "C" fn(*mut u8, *mut u8, *mut u8, i32) -> *mut u8 = std::mem::transmute(va);
            ctor(object, wstring(acs, name), gui, if flag { 2 } else { 1 });
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

/// `OnListBoxItemClickedEvent { ListBox*, ListBoxRowData*, uint index }`.
unsafe extern "C" fn on_list_clicked(_this: *mut u8, e: *const u8) {
    record(format!("list \"{}\" {} 0", name_of(rd(e, 0)), rd::<u32>(e, 0x10)));
}

unsafe extern "C" fn on_list_deselected(_this: *mut u8, e: *const u8) {
    record(format!("list \"{}\" {} 1", name_of(rd(e, 0)), rd::<u32>(e, 0x10)));
}

/// `OnSpinnerValueChanged { Spinner*, int }`.
unsafe extern "C" fn on_spinner(_this: *mut u8, e: *const u8) {
    record(format!("spin \"{}\" {}", name_of(rd(e, 0)), rd::<i32>(e, 8)));
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
        "ListBox" => vec![(0x180, on_list_clicked as Callback), (0x198, on_list_deselected as Callback)],
        "Spinner" | "SpinnerMirrored" => vec![(0x190, on_spinner as Callback)],
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
        "ListBox" => format!(" list={},{},{}", rd::<i32>(control, 0x208), rd::<i32>(control, 0x20c), (rd::<usize>(control, 0x1c8) - rd::<usize>(control, 0x1c0)) / 0x68),
        "ListBoxRow" => format!(" row={} cells={} {:08x},{:08x},{:08x},{:08x}", rd::<i32>(control, 0x188), rd::<u8>(control, 0x18c), bits(0x178), bits(0x17c), bits(0x180), bits(0x184)),
        "Graph" => {
            let (begin, end): (*const *const u8, *const *const u8) = (rd(control, 0x188), rd(control, 0x190));
            let count = if begin.is_null() { 0 } else { end.offset_from(begin) as usize };
            let sizes: Vec<String> = (0..count).map(|i| ((rd::<usize>(*begin.add(i), 0x18) - rd::<usize>(*begin.add(i), 0x10)) / 4).to_string()).collect();
            format!(" graph={:08x},{:08x},{},{} series={}", bits(0x178), bits(0x17c), bits(0x180), rd::<u8>(control, 0x184), sizes.join(","))
        }
        "Spinner" | "SpinnerMirrored" => format!(
            " spinner={},{},{},{} counters={:08x},{:08x} items={} flags={}{}{}{}{}",
            rd::<i32>(control, 0x248),
            rd::<i32>(control, 0x180),
            rd::<i32>(control, 0x184),
            rd::<i32>(control, 0x188),
            bits(0x1e0),
            bits(0x1e4),
            (rd::<usize>(control, 0x258) - rd::<usize>(control, 0x250)) / 0x20,
            rd::<u8>(control, 0x1b0),
            rd::<u8>(control, 0x1b1),
            rd::<u8>(control, 0x1b2),
            rd::<u8>(control, 0x1b3),
            rd::<u8>(control, 0x1dc)
        ),
        "CustomSpinner" => format!(" cspin={},{:08x},{}", rd::<i32>(control, 0x1d4), bits(0x1d0), rd::<u8>(control, 0x178)),
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
        "addItem" => {
            // a ListBoxRowData (0x68 bytes) laid out by hand; the list copies it
            let (name, columns, back, fore) = crate::gui_seq::list_item(text, number);
            let data = acs.alloc(0x68);
            write_wstring(acs, data, &name);
            wr(data, 0x20, number as u32);
            wr(data, 0x24, back);
            let colors = acs.alloc(fore.len() * 16 + 16);
            for (i, color) in fore.iter().enumerate() {
                wr(colors, i * 16, *color);
            }
            wr(data, 0x38, colors);
            wr(data, 0x40, colors.add(fore.len() * 16));
            wr(data, 0x48, colors.add(fore.len() * 16));
            let texts = acs.alloc(columns.len() * 0x20 + 0x20);
            for (i, column) in columns.iter().enumerate() {
                write_wstring(acs, texts.add(i * 0x20), column);
            }
            wr(data, 0x50, texts);
            wr(data, 0x58, texts.add(columns.len() * 0x20));
            wr(data, 0x60, texts.add(columns.len() * 0x20));
            let add: extern "C" fn(*mut u8, *mut u8) = std::mem::transmute(acs.va(VA_LIST_BOX_ADD_ITEM));
            add(object, data);
        }
        "lbClear" => {
            let f: extern "C" fn(*mut u8) = std::mem::transmute(acs.va(VA_LIST_BOX_CLEAR));
            f(object);
        }
        "lbSelect" => {
            let f: extern "C" fn(*mut u8, i32) = std::mem::transmute(acs.va(VA_LIST_BOX_SELECT_ITEM));
            f(object, number as i32);
        }
        "lbSelectName" => with_text(VA_LIST_BOX_SELECT_BY_NAME),
        "lbTop" => {
            let f: extern "C" fn(*mut u8) = std::mem::transmute(acs.va(VA_LIST_BOX_SCROLL_TO_TOP));
            f(object);
        }
        "lbCells" => {
            let f: extern "C" fn(*mut u8, bool) = std::mem::transmute(acs.va(VA_LIST_BOX_DRAW_CELLS));
            f(object, number != 0.0);
        }
        "lbFont" => {
            let f: extern "C" fn(*mut u8, f32) = std::mem::transmute(acs.va(VA_LIST_BOX_FONT_SIZE));
            f(object, number);
        }
        "addSerie" => {
            let color = acs.alloc(16);
            wr(color, 0, crate::gui_seq::code_color(number));
            let f: extern "C" fn(*mut u8, *mut u8) = std::mem::transmute(acs.va(VA_GRAPH_ADD_SERIE));
            f(object, color);
        }
        "fill" => {
            let (serie, values) = crate::gui_seq::fill_values(text, number);
            let f: extern "C" fn(*mut u8, u32, f32) = std::mem::transmute(acs.va(VA_GRAPH_ADD_VALUE));
            for value in values {
                f(object, serie, value);
            }
        }
        "axis" => {
            let axis = acs.alloc(0x14);
            wr(axis, 0, [0.0f32, 1.0, 0.0]);
            wr(axis, 0xc, number);
            wr(axis, 0x10, (text == "v") as u8);
            let f: extern "C" fn(*mut u8, *mut u8) = std::mem::transmute(acs.va(VA_GRAPH_ADD_AXIS));
            f(object, axis);
        }
        "clearSerie" => with_u32(VA_GRAPH_CLEAR_SERIE),
        "spValue" => {
            let f: extern "C" fn(*mut u8, i32) = std::mem::transmute(acs.va(VA_SPINNER_SET_VALUE));
            f(object, number as i32);
        }
        "spValueItem" => with_text(VA_SPINNER_SET_VALUE_ITEM),
        "spItem" => with_text(VA_SPINNER_ADD_ITEM),
        "spLabel" => with_text(VA_SPINNER_SET_LABEL_TEXT),
        "spUnits" => write_wstring(acs, object.add(0x1b8), text),
        "spArrows" => {
            let f: extern "C" fn(*mut u8, bool) = std::mem::transmute(acs.va(VA_SPINNER_SET_DRAW_ARROWS));
            f(object, number != 0.0);
        }
        "spPos" => {
            let f: extern "C" fn(*mut u8, f32, f32) = std::mem::transmute(acs.va(VA_SPINNER_SET_POSITION));
            f(object, number, text.parse().unwrap_or(0.0));
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
