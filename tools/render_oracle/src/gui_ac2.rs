// SPDX-License-Identifier: GPL-3.0-or-later

//! The game side of the GUI oracle, part two: the classes and HUD objects added after the
//! base (their constructors, methods, state and events).

use crate::ac::{rd, Game};

pub unsafe fn static_initialisers(_game: &Game) {}

pub unsafe fn render_hud(_game: &Game, _gui: *mut u8, _dt: f32) {}

/// What a handler the oracle hangs on an event of a widget is called with: the `std::function`
/// object and the event's argument.
pub type Callback = unsafe extern "C" fn(*mut u8, *const u8);

static EVENTS: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

fn record(text: String) {
    EVENTS.lock().unwrap().push(text);
}

unsafe fn name_of(control: *const u8) -> String {
    crate::gui_ac::read_wstring(control.add(8))
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

/// The events of a class besides `evClicked`: where the event is in the object, and the
/// handler that writes it down.
pub fn class_events(class: &str) -> Vec<(usize, Callback)> {
    match class {
        "CheckBox" => vec![(0x178, on_check_box as Callback)],
        "Slider" => vec![(0x178, on_slider as Callback), (0x190, on_cut as Callback)],
        _ => Vec::new(),
    }
}

pub fn events() -> Vec<String> {
    EVENTS.lock().unwrap().drain(..).collect()
}

pub unsafe fn extra(_game: &Game, _control: *const u8, _class: &str) -> String {
    String::new()
}

pub unsafe fn call(_game: &Game, _gui: *mut u8, _object: *mut u8, what: &str, _number: f32, _text: &str) -> Result<(), String> {
    Err(format!("the game side knows no call {what:?}"))
}
