// SPDX-License-Identifier: GPL-3.0-or-later

//! The page's Controls panel: the values AC's `JoypadCarControl` and `KeyboardCarControl` read
//! from `controls.ini`, by AC's own section and key names, so the browser can drive with the
//! player's own numbers instead of the built-in layout's.
//!
//! The settings are a `controls.ini` like the desktop's ([`ControlsIni`], the same parser,
//! [`Bindings::from_ini`], the same classes). The page keeps its text in `localStorage`, shows
//! it ([`to_json`]), changes one value at a time ([`set`]) or takes a whole file
//! ([`from_file`]: the player's `Documents\Assetto Corsa\cfg\controls.ini`).
//!
//! The names and what the classes do with each value are from the disassembly
//! (`JoypadCarControl::JoypadCarControl` @ 0x1401d9280, `KeyboardCarControl::readFromIni`
//! @ 0x14010ff60). The ranges are the panel's: where the class limits a value itself the
//! range is that limit, elsewhere it is what the formula can take.

use std::fmt::Write as _;

use rustyac_game::input::bindings::{default_ini, effective, key_name, Bindings};
use rustyac_game::input::ini::ControlsIni;
use rustyac_game::input::keyboard::Keys;
use rustyac_game::input::pad::{button_from_name, button_label};

/// How a value is edited.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    /// A number between two ends, in steps.
    Number { min: f32, max: f32, step: f32 },
    /// One of a few texts.
    Choice(&'static [&'static str]),
    /// 0 or 1.
    Toggle,
}

/// One value of the panel.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Setting {
    pub section: &'static str,
    pub key: &'static str,
    pub label: &'static str,
    pub kind: Kind,
    pub what: &'static str,
}

const fn number(section: &'static str, key: &'static str, label: &'static str, min: f32, max: f32, step: f32, what: &'static str) -> Setting {
    Setting { section, key, label, kind: Kind::Number { min, max, step }, what }
}

/// Every value the two classes read from the file, in the panel's order: the pad, then the
/// keyboard.
pub const SETTINGS: [Setting; 14] = [
    number(
        "X360",
        "SPEED_SENSITIVITY",
        "Pad: speed sensitivity",
        0.0,
        1.0,
        0.01,
        "How much less the stick steers as the car gets faster. 0 = off: the full lock at any speed. At 1 the full stick gives 20 degrees of steering wheel from 200 km/h on.",
    ),
    number("X360", "STEER_GAMMA", "Pad: steering gamma", 1.0, 4.0, 0.01, "The stick's curve: 1 is straight, more makes the middle of the stick gentler. The class reads anything below 1 as 1."),
    number("X360", "STEER_FILTER", "Pad: steering filter", 0.0, 1.0, 0.01, "Smooths the steering: each physics step makes (1 - 0.99 x filter) of its move. 0 = off, 1 = very slow."),
    number("X360", "STEER_DEADZONE", "Pad: steering dead zone", 0.0, 0.9, 0.01, "The part of the stick around its middle that does nothing."),
    number("X360", "STEER_SPEED", "Pad: steering speed", 0.1, 1.0, 0.01, "The most the steering moves per physics step (a tenth of this number of the lock). The class keeps it between 0.1 and 1."),
    number("X360", "RUMBLE_INTENSITY", "Pad: rumble intensity", 0.0, 1.0, 0.01, "The strength of the pad's motors on kerbs and with sliding tyres. 0 = off."),
    Setting { section: "X360", key: "STEER_THUMB", label: "Pad: steering stick", kind: Kind::Choice(&["LEFT", "RIGHT"]), what: "The stick that steers. The class takes anything but LEFT as the right stick." },
    Setting {
        section: "ADVANCED",
        key: "COMBINE_WITH_KEYBOARD_CONTROL",
        label: "Pad: keyboard keys too",
        kind: Kind::Toggle,
        what: "While the pad drives, the KEY of each action (gears, DRS, KERS ...) works beside its pad button.",
    },
    Setting {
        section: "RUSTYAC",
        key: "USE_LEGACY_GAMEPAD_CODE",
        label: "Pad: legacy speed rule",
        kind: Kind::Toggle,
        what: "AC's older rule for the speed sensitivity (system/cfg/assetto_corsa.ini [GAMEPAD] USE_LEGACY_CODE), with engine rumble.",
    },
    number("KEYBOARD", "STEERING_SPEED", "Keys: steering speed", 0.1, 5.0, 0.05, "How fast the steering moves while a key is held: the part of the lock per second at a standstill. 0 would mean no steering."),
    number("KEYBOARD", "STEERING_OPPOSITE_DIRECTION_SPEED", "Keys: opposite-direction speed", 0.0, 10.0, 0.05, "Times the steering speed while steering against where the wheels point."),
    number("KEYBOARD", "STEER_RESET_SPEED", "Keys: return speed", 0.0, 10.0, 0.05, "Times the steering speed while no key is held: how fast the wheels come back to straight."),
    number("KEYBOARD", "STEER_GAIN", "Keys: look-ahead gain", 0.0, 1.0, 0.01, "Read by the class for its look-ahead on the AI line. That part is not in rustyAC: no effect."),
    number("KEYBOARD", "LOOKAHEAD_POINTS", "Keys: look-ahead distance", 0.0, 20.0, 1.0, "Read by the class for its look-ahead on the AI line. That part is not in rustyAC: no effect."),
];

/// What the keyboard class also reads and rustyAC does not use (mouse steering is not ported).
pub const NOT_USED: [(&str, &str); 3] = [("KEYBOARD", "MOUSE_STEER"), ("KEYBOARD", "MOUSE_SPEED"), ("KEYBOARD", "MOUSE_ACCELERATOR_BRAKE")];

/// The settings without a file: the built-in layout, as the page has always driven.
pub fn defaults() -> ControlsIni {
    effective(&default_ini())
}

/// A `controls.ini` as the desktop takes it: the same parser, the same test for "can this be
/// used", the same sections carried over.
pub fn from_file(bytes: &[u8]) -> Result<ControlsIni, String> {
    let ini = ControlsIni::from_bytes(bytes);
    if !(ini.has_section("X360") || ini.has_section("KEYBOARD") || ini.has_section("STEER")) {
        return Err("no [X360], [KEYBOARD] or [STEER] section in the file: it is not a controls.ini".to_string());
    }
    Ok(effective(&ini))
}

fn setting(section: &str, key: &str) -> Option<&'static Setting> {
    SETTINGS.iter().find(|s| s.section == section && s.key == key)
}

/// Changes one value of the panel. A number is kept inside the panel's range; a text that is
/// no number, or a value the panel does not have, is refused.
pub fn set(ini: &mut ControlsIni, section: &str, key: &str, value: &str) -> Result<(), String> {
    let setting = setting(section, key).ok_or_else(|| format!("[{section}] {key} is not a value of the Controls panel"))?;
    let value = value.trim();
    let text = match setting.kind {
        Kind::Number { min, max, .. } => {
            let x: f32 = value.parse().map_err(|_| format!("{}: \"{value}\" is not a number", setting.label))?;
            if !x.is_finite() {
                return Err(format!("{}: \"{value}\" is not a number", setting.label));
            }
            x.clamp(min, max).to_string()
        }
        Kind::Choice(choices) => choices.iter().find(|c| c.eq_ignore_ascii_case(value)).ok_or_else(|| format!("{}: {value} is not one of {}", setting.label, choices.join(", ")))?.to_string(),
        Kind::Toggle => if value == "1" || value.eq_ignore_ascii_case("true") { "1" } else { "0" }.to_string(),
    };
    ini.set(section, key, text);
    Ok(())
}

/// The value of a setting as the classes read it (a missing number is 0, as in the game).
fn value_json(ini: &ControlsIni, setting: &Setting) -> String {
    match setting.kind {
        Kind::Number { .. } => {
            let x = ini.get_float(setting.section, setting.key);
            if x.is_finite() {
                x.to_string()
            } else {
                "0".to_string()
            }
        }
        Kind::Choice(_) => json_text(if ini.get_string(setting.section, setting.key) == "LEFT" { "LEFT" } else { "RIGHT" }),
        Kind::Toggle => (ini.get_int(setting.section, setting.key) > 0).to_string(),
    }
}

fn json_text(text: &str) -> String {
    let mut out = String::from("\"");
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            c if (c as u32) < 0x20 => out.push(' '),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// What drives the car with these settings, one row per action: its keys and its pad button.
pub fn layout_rows(ini: &ControlsIni) -> Vec<(&'static str, String, String)> {
    let bindings = Bindings::from_ini(ini, String::new());
    let (keys, keys2) = (Keys::from_ini(&bindings.ini), Keys::second_from_ini(&bindings.ini));
    let key = |name: &str| -> String {
        let find = |keys: &Keys| keys.named().into_iter().find(|(n, _)| *n == name).map_or(0, |(_, code)| code);
        let names: Vec<String> = [find(&keys), find(&keys2)].into_iter().filter(|code| *code > 0).map(key_name).collect();
        if names.is_empty() {
            "-".to_string()
        } else {
            names.join(" / ")
        }
    };
    let pad = |section: &str| button_label(button_from_name(bindings.ini.get_string(section, "XBOXBUTTON"))).to_string();
    let stick = if bindings.ini.get_string("X360", "STEER_THUMB") == "LEFT" { "left stick" } else { "right stick" };
    vec![
        ("gas, brake", format!("{}, {}", key("GAS"), key("BRAKE")), "RT, LT".to_string()),
        ("steer", format!("{}, {}", key("LEFT"), key("RIGHT")), stick.to_string()),
        ("gear up, down", format!("{}, {}", key("GEARUP"), key("GEARDN")), format!("{}, {}", pad("GEARUP"), pad("GEARDN"))),
        ("clutch, handbrake", format!("{}, {}", key("CLUTCH"), key("HANDBRAKE")), format!("{}, {}", pad("__EXT_KEYBOARD_CLUTCH"), pad("HANDBRAKE"))),
        ("DRS, KERS / ERS", format!("{}, {}", key("DRS"), key("KERS")), format!("{}, {}", pad("DRS"), pad("KERS"))),
        ("traction control up, down", "T, Shift+T".to_string(), format!("{}, {}", pad("TCUP"), pad("TCDN"))),
        ("brake bias forward, rearward", "], [".to_string(), format!("{}, {}", pad("BALANCEUP"), pad("BALANCEDN"))),
        ("headlights", key("ACTION_HEADLIGHTS"), pad("ACTION_HEADLIGHTS")),
    ]
}

/// The panel as JSON: `{"origin", "custom", "settings": [{"section", "key", "label", "what",
/// "kind", "min", "max", "step", "choices", "value", "default"}], "not_used": [...],
/// "layout": [{"what", "keys", "pad"}]}`. `custom`: the settings are not the built-in ones.
pub fn to_json(ini: &ControlsIni, origin: &str) -> String {
    let base = defaults();
    let mut out = format!("{{\"origin\": {}, \"custom\": {}, \"settings\": [", json_text(origin), *ini != base);
    for (n, setting) in SETTINGS.iter().enumerate() {
        if n > 0 {
            out.push_str(", ");
        }
        let _ = write!(out, "{{\"section\": {}, \"key\": {}, \"label\": {}, \"what\": {}, ", json_text(setting.section), json_text(setting.key), json_text(setting.label), json_text(setting.what));
        match setting.kind {
            Kind::Number { min, max, step } => {
                let _ = write!(out, "\"kind\": \"number\", \"min\": {min}, \"max\": {max}, \"step\": {step}, ");
            }
            Kind::Choice(choices) => {
                let _ = write!(out, "\"kind\": \"choice\", \"choices\": [{}], ", choices.iter().map(|c| json_text(c)).collect::<Vec<_>>().join(", "));
            }
            Kind::Toggle => out.push_str("\"kind\": \"toggle\", "),
        }
        let _ = write!(out, "\"value\": {}, \"default\": {}}}", value_json(ini, setting), value_json(&base, setting));
    }
    out.push_str("], \"not_used\": [");
    out.push_str(&NOT_USED.iter().map(|(section, key)| json_text(&format!("[{section}] {key}"))).collect::<Vec<_>>().join(", "));
    out.push_str("], \"layout\": [");
    for (n, (what, keys, pad)) in layout_rows(ini).iter().enumerate() {
        if n > 0 {
            out.push_str(", ");
        }
        let _ = write!(out, "{{\"what\": {}, \"keys\": {}, \"pad\": {}}}", json_text(what), json_text(keys), json_text(pad));
    }
    out.push_str("]}");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustyac_game::input::keyboard::KeyboardCarControl;
    use rustyac_game::input::pad::JoypadCarControl;

    #[test]
    fn the_defaults_are_the_built_in_layout() {
        let ini = defaults();
        assert_eq!(Bindings::from_ini(&ini, "x".to_string()), Bindings::from_ini(&default_ini(), "x".to_string()));
        let json = to_json(&ini, "the built-in layout");
        assert!(json.contains("\"custom\": false"), "{json}");
        assert!(json.contains("\"key\": \"SPEED_SENSITIVITY\"") && json.contains("\"value\": 0.5, \"default\": 0.5"), "{json}");
        assert!(json.contains("{\"what\": \"gear up, down\", \"keys\": \"Space / E, Left Ctrl / Q\", \"pad\": \"Y, X\"}"), "{json}");
        // every value of the panel is one the built-in layout has, inside the panel's range
        for setting in SETTINGS {
            assert!(ini.has_key(setting.section, setting.key), "{setting:?}");
            if let Kind::Number { min, max, .. } = setting.kind {
                let x = ini.get_float(setting.section, setting.key);
                assert!((min..=max).contains(&x), "{setting:?} {x}");
            }
        }
    }

    #[test]
    fn a_change_reaches_the_classes_and_stays_in_range() {
        let mut ini = defaults();
        set(&mut ini, "X360", "SPEED_SENSITIVITY", "0").unwrap();
        set(&mut ini, "X360", "STEER_GAMMA", "0.2").unwrap();
        set(&mut ini, "X360", "STEER_THUMB", "right").unwrap();
        set(&mut ini, "KEYBOARD", "STEERING_SPEED", "99").unwrap();
        set(&mut ini, "ADVANCED", "COMBINE_WITH_KEYBOARD_CONTROL", "1").unwrap();
        let pad = JoypadCarControl::from_ini(&ini, false);
        assert_eq!((pad.speed_sensitivity, pad.steer_gamma, pad.is_steer_with_right), (0.0, 1.0, true));
        assert!(pad.buttons[0].allow_keyboard_input);
        assert_eq!(KeyboardCarControl::from_ini(&ini).steer_speed, 5.0);
        assert!(to_json(&ini, "changed").contains("\"custom\": true"));
        assert!(set(&mut ini, "X360", "STEER_GAMMA", "soft").is_err());
        assert!(set(&mut ini, "X360", "STEER_GAMMA", "NaN").is_err());
        assert!(set(&mut ini, "GEARUP", "KEY", "0x41").is_err(), "only the panel's values");
    }

    #[test]
    fn a_file_is_read_as_the_desktop_reads_it() {
        let text = "[HEADER]\nINPUT_METHOD=X360\n[X360]\nSTEER_THUMB=LEFT\nSTEER_GAMMA=1.4 ; mine\nSPEED_SENSITIVITY=0\nSTEER_SPEED=0.95\n[GEARUP]\nKEY=0x58\nXBOXBUTTON=B\n";
        let ini = from_file(text.as_bytes()).unwrap();
        assert_eq!(Bindings::from_ini(&ini, "x".to_string()), Bindings::from_ini(&ControlsIni::parse(text), "x".to_string()));
        // UTF-16 with a byte order mark, as some editors save it
        let mut wide = vec![0xff, 0xfe];
        wide.extend(text.encode_utf16().flat_map(u16::to_le_bytes));
        assert_eq!(from_file(&wide).unwrap(), ini);
        // written to the browser's storage and read back
        assert_eq!(from_file(ini.to_text().as_bytes()).unwrap(), ini);
        assert!(to_json(&ini, "mine").contains("\"keys\": \"X / E, Q\""), "{}", to_json(&ini, "mine"));
        assert!(from_file(b"[VIDEO]\nWIDTH=1920\n").is_err());
    }
}
