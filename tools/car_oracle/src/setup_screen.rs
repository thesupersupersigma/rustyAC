// SPDX-License-Identifier: GPL-3.0-or-later

//! The setup screen's part of a session start (Task C5b: out of game.rs, so that csp_oracle
//! starts its drives the same way).

use crate::game::{rd, wr};

pub const CAR_SETUP_MANAGER: usize = 0x35d0;
// SetupItem (0x88 bytes): one adjustable value of the car
pub const SETUP_ITEM_SIZE: usize = 0x88;
pub const SI_NAME: usize = 0x8;
pub const SI_CONNECTED_FLOAT: usize = 0x28;
pub const SI_MULTIPLIER: usize = 0x50;
pub const SI_NEW_VALUE: usize = 0x54;
pub const SI_ATTACHED: usize = 0x58;
pub const SI_LABEL_MULTIPLIER: usize = 0x80;

/// What the game's setup screen does to the car when a session starts with the default setup.
///
/// Every `SetupManager` item that has a section in the car's `setup.ini` is shown as a
/// spinner holding an integer; on entry the screen sets every spinner to its default and
/// writes the spinner's value back to the item. A value that is not on the spinner's grid
/// (e.g. a camber of 3.0 degrees stored as radians and shown in tenths of a degree) comes
/// back changed. Mirrors `SetupScreen::loadINI` 0x14017d950, `SetupTab::addItem` 0x140183850
/// and the spinner job 0x140183620; the game's own `SetupManager::step` then applies the
/// values in the first step. Returns `name:old->new` for every value that will change.
pub unsafe fn apply_setup_screen_defaults(car: *mut u8, setup: &rustyac_physics::data::ini::IniReader) -> Vec<String> {
    let manager = car.add(CAR_SETUP_MANAGER);
    let mut item: *mut u8 = rd(manager, 0);
    let end: *mut u8 = rd(manager, 8);
    let clicks = setup.has_section("DISPLAY_METHOD");
    let mut changes = Vec::new();
    while item < end {
        let name = read_wstring(item.add(SI_NAME));
        if setup.has_section(&name) {
            // a missing key reads as 0, as in the game's INIReader
            let float = |key: &str| setup.get_float(&name, key).unwrap_or(0.0);
            let label = rd::<f32>(item, SI_LABEL_MULTIPLIER).abs();
            let min = (float("MIN") / label) as f64;
            let max = (float("MAX") / label) as f64;
            let step = setup.get_int(&name, "STEP").unwrap_or(0) as f64;
            let mode = if clicks { setup.get_int(&name, "SHOW_CLICKS").unwrap_or(0) } else { 0 };
            let value: f32 = rd(item, SI_NEW_VALUE);
            // the spinner's range and position (C casts: toward zero)
            let (low, high, position) = match mode {
                1 => ((min / step) as i32, (max / step) as i32, (value as f64 / step + 0.5) as i32),
                2 => (0, ((max - min) / step) as i32, ((value - min as f32) / step as f32 + 0.5f32) as i32),
                _ => (min as i32, max as i32, value as i32),
            };
            let position = if position > high { high } else { position.max(low) };
            let back: f32 = match mode {
                1 => step as f32 * position as f32,
                2 => step as f32 * position as f32 + min as f32,
                _ => position as f32,
            };
            wr(item, SI_NEW_VALUE, back);
            wr(item, SI_ATTACHED, 1u8);
            let connected: *const f32 = rd(item, SI_CONNECTED_FLOAT);
            let applied = rd::<f32>(item, SI_MULTIPLIER) * back;
            if !connected.is_null() && applied.to_bits() != (*connected).to_bits() && applied != *connected {
                changes.push(format!("{name}:{:?}->{:?}", *connected, applied));
            }
        }
        item = item.add(SETUP_ITEM_SIZE);
    }
    changes
}

pub unsafe fn read_wstring(string: *const u8) -> String {
    let len: usize = rd(string, 0x10);
    let capacity: usize = rd(string, 0x18);
    let data: *const u16 = if capacity < 8 { string.cast() } else { rd(string, 0) };
    String::from_utf16_lossy(std::slice::from_raw_parts(data, len))
}
