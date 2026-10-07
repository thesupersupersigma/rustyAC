// SPDX-License-Identifier: GPL-3.0-or-later

//! Which button does what. The file format is AC's `controls.ini`, read with AC's rules
//! ([`super::ini`]); a few sections that start with `RUSTYAC` hold what AC has no name for.
//!
//! Where the bindings come from, in this order: `--controls <file>`; `rustyac_controls.ini`
//! next to the program (yours to edit); AC's own `Documents\Assetto Corsa\cfg\controls.ini`
//! (only ever read); the built-in layout. Whatever is in use is written to
//! `rustyac_controls.ini` if that file does not exist yet.

use std::path::{Path, PathBuf};

use super::ini::ControlsIni;
use super::keyboard::Keys;
use super::pad::{button, button_from_name, button_label, PadButton, PAD_ACTIONS};
use crate::cli::Options;

/// The sections of AC's file that rustyAC reads (whole sections are carried over).
const AC_SECTIONS: [&str; 14] = [
    "HEADER",
    "X360",
    "KEYBOARD",
    "ADVANCED",
    "STEER",
    "THROTTLE",
    "BRAKES",
    "CLUTCH",
    "SHIFTER",
    "FF_TWEAKS",
    "FF_ENHANCEMENT",
    "FF_ENHANCEMENT_2",
    "FF_SKIP_STEPS",
    "__EXT_KEYBOARD_CLUTCH",
];
/// AC's command keys (with Ctrl) that rustyAC knows.
const COMMAND_SECTIONS: [&str; 3] = ["ABS", "TRACTION_CONTROL", "AUTO_SHIFTER"];

#[derive(Clone, Debug, PartialEq)]
pub struct Bindings {
    /// The bindings in use, as `rustyac_controls.ini` holds them.
    pub ini: ControlsIni,
    /// Where they came from (for the console).
    pub origin: String,
    /// `[HEADER] INPUT_METHOD`: `X360`, `KEYBOARD` or `WHEEL` (the device that drives at the
    /// start; any device takes over when touched).
    pub input_method: String,
    /// AC's `system/cfg/assetto_corsa.ini [GAMEPAD] USE_LEGACY_CODE`.
    pub use_legacy_gamepad_code: bool,
    /// rustyAC's own pad buttons.
    pub pad_reset: PadButton,
    pub pad_pause: PadButton,
    /// These bindings may be written to `rustyac_controls.ini` (they are AC's or the built-in
    /// ones found the normal way, not a file or layout asked for on the command line).
    pub keep: bool,
    /// Keys of AC's commands, pressed with Ctrl (Shift for "down"): ABS, traction control,
    /// automatic gearbox.
    pub key_abs: i32,
    pub key_traction_control: i32,
    pub key_auto_shifter: i32,
}

/// The built-in layout: the Xbox pad as asked for in Task 11 (left stick, RT gas, LT brake,
/// Y up-shift, X down-shift, A clutch, LB DRS, B KERS), the keyboard on the arrows.
pub fn default_ini() -> ControlsIni {
    let mut ini = ControlsIni::default();
    ini.set("HEADER", "INPUT_METHOD", "X360");
    // the stick's shaping: the numbers of AC's stock gamepad preset as far as remembered
    // (not checked against an untouched install)
    for (key, value) in [
        ("STEER_THUMB", "LEFT"),
        ("RUMBLE_INTENSITY", "0.8"),
        ("STEER_SPEED", "0.2"),
        ("STEER_GAMMA", "2"),
        ("STEER_FILTER", "0.7"),
        ("STEER_DEADZONE", "0.05"),
        ("SPEED_SENSITIVITY", "0.5"),
    ] {
        ini.set("X360", key, value);
    }
    for (key, value) in [
        ("STEERING_SPEED", "1.75"),
        ("STEERING_OPPOSITE_DIRECTION_SPEED", "2.5"),
        ("STEER_GAIN", "0.18"),
        ("STEER_RESET_SPEED", "1.8"),
        ("LOOKAHEAD_POINTS", "6"),
        ("GAS", "0x26"),
        ("BRAKE", "0x28"),
        ("LEFT", "0x25"),
        ("RIGHT", "0x27"),
    ] {
        ini.set("KEYBOARD", key, value);
    }
    ini.set("ADVANCED", "COMBINE_WITH_KEYBOARD_CONTROL", "0");
    for action in PAD_ACTIONS {
        ini.set(action, "XBOXBUTTON", "-1");
        ini.set(action, "KEY", "-1");
        // no DirectInput button (a missing key would mean button 0 of device 0)
        ini.set(action, "JOY", "-1");
        ini.set(action, "BUTTON", "-1");
    }
    // no DirectInput axis either
    for section in ["STEER", "THROTTLE", "BRAKES", "CLUTCH", "HANDBRAKE"] {
        ini.set(section, "JOY", "-1");
        ini.set(section, "AXLE", "-1");
    }
    for (key, value) in [("LOCK", "900"), ("SCALE", "1"), ("STEER_GAMMA", "1"), ("FF_GAIN", "1"), ("DEBOUNCING_MS", "50")] {
        ini.set("STEER", key, value);
    }
    for (action, pad, key) in [
        ("GEARUP", "Y", "0x20"),
        ("GEARDN", "X", "0xA2"),
        ("DRS", "LSHOULDER", "-1"),
        ("KERS", "B", "-1"),
        ("HANDBRAKE", "RSHOULDER", "-1"),
        ("ACTION_HEADLIGHTS", "LTHUMB_PRESS", "0x4C"),
        ("ACTION_CHANGE_CAMERA", "RTHUMB_PRESS", "-1"),
        ("BALANCEUP", "DPAD_RIGHT", "-1"),
        ("BALANCEDN", "DPAD_LEFT", "-1"),
        ("TCUP", "DPAD_UP", "-1"),
        ("TCDN", "DPAD_DOWN", "-1"),
    ] {
        ini.set(action, "XBOXBUTTON", pad);
        ini.set(action, "KEY", key);
    }
    ini.set("__EXT_KEYBOARD_CLUTCH", "XBOXBUTTON", "A");
    ini.set("__EXT_KEYBOARD_CLUTCH", "KEY", "-1");
    ini.set("ABS", "KEY", "0x41");
    ini.set("TRACTION_CONTROL", "KEY", "0x54");
    ini.set("AUTO_SHIFTER", "KEY", "0x47");
    ini
}

/// rustyAC's own sections, where the file does not have them yet.
fn add_own_sections(ini: &mut ControlsIni) {
    let mut default = |section: &str, key: &str, value: &str| {
        if !ini.has_key(section, key) {
            ini.set(section, key, value);
        }
    };
    default("RUSTYAC", "USE_LEGACY_GAMEPAD_CODE", "0");
    default("RUSTYAC_RESET", "XBOXBUTTON", "BACK");
    default("RUSTYAC_PAUSE", "XBOXBUTTON", "START");
    // a second key for each action: WASD beside the arrows, E / Q for the gears
    for (key, value) in [
        ("GAS", "0x57"),
        ("BRAKE", "0x53"),
        ("LEFT", "0x41"),
        ("RIGHT", "0x44"),
        ("GEARUP", "0x45"),
        ("GEARDN", "0x51"),
        ("HANDBRAKE", "0x42"),
        ("DRS", "0x46"),
        ("KERS", "0x4B"),
        ("CLUTCH", "0xA0"),
        ("ACTION_HEADLIGHTS", "-1"),
        ("BALANCEUP", "-1"),
        ("BALANCEDN", "-1"),
        ("ABSUP", "-1"),
        ("ABSDN", "-1"),
        ("TCUP", "-1"),
        ("TCDN", "-1"),
    ] {
        default("RUSTYAC_KEYS_2", key, value);
    }
}

/// The part of a `controls.ini` that rustyAC uses, with its own sections added.
pub fn effective(source: &ControlsIni) -> ControlsIni {
    let mut ini = ControlsIni::default();
    for section in AC_SECTIONS.iter().chain(&PAD_ACTIONS).chain(&COMMAND_SECTIONS).chain(&["RUSTYAC", "RUSTYAC_RESET", "RUSTYAC_PAUSE", "RUSTYAC_KEYS_2"]) {
        for key in source.keys(section) {
            ini.set(section, &key, source.get_string(section, &key));
        }
    }
    if !ini.has_section("KEYBOARD") {
        // AC's keyboard class without a file: its constructor's numbers, no driving keys.
        // The arrows are given here so that the keyboard works.
        let defaults = default_ini();
        for key in defaults.keys("KEYBOARD") {
            ini.set("KEYBOARD", &key, defaults.get_string("KEYBOARD", &key));
        }
    }
    add_own_sections(&mut ini);
    ini
}

/// Can a `controls.ini` be used: does it say how to drive with a pad or a keyboard?
fn usable(ini: &ControlsIni) -> bool {
    ini.has_section("X360") || ini.has_section("KEYBOARD") || ini.has_section("STEER")
}

/// AC's own bindings file of the current user.
pub fn ac_controls_path() -> Option<PathBuf> {
    let home = std::env::var_os("USERPROFILE")?;
    Some(PathBuf::from(home).join("Documents").join("Assetto Corsa").join("cfg").join("controls.ini"))
}

/// `rustyac_controls.ini` next to the program.
pub fn own_controls_path() -> Option<PathBuf> {
    Some(std::env::current_exe().ok()?.parent()?.join("rustyac_controls.ini"))
}

impl Bindings {
    pub fn from_ini(source: &ControlsIni, origin: String) -> Bindings {
        let ini = effective(source);
        let button = |section: &str| PadButton {
            button: button_from_name(ini.get_string(section, "XBOXBUTTON")),
            key: -1,
            allow_keyboard_input: false,
        };
        let method = ini.get_string("HEADER", "INPUT_METHOD").to_string();
        Bindings {
            origin,
            input_method: if method.is_empty() { "X360".to_string() } else { method },
            use_legacy_gamepad_code: ini.get_int("RUSTYAC", "USE_LEGACY_GAMEPAD_CODE") != 0,
            pad_reset: button("RUSTYAC_RESET"),
            pad_pause: button("RUSTYAC_PAUSE"),
            keep: true,
            key_abs: ini.get_hex("ABS", "KEY"),
            key_traction_control: ini.get_hex("TRACTION_CONTROL", "KEY"),
            key_auto_shifter: ini.get_hex("AUTO_SHIFTER", "KEY"),
            ini,
        }
    }

    /// Finds the bindings as the module's head describes; `notes` gets what happened on the way.
    pub fn load(options: &Options, notes: &mut Vec<String>) -> Bindings {
        let try_file = |path: &Path, notes: &mut Vec<String>| -> Option<ControlsIni> {
            match ControlsIni::load(path) {
                Ok(ini) if usable(&ini) => Some(ini),
                Ok(_) => {
                    notes.push(format!("{}: no [X360], [KEYBOARD] or [STEER] section, not used", path.display()));
                    None
                }
                Err(message) => {
                    notes.push(format!("not read: {message}"));
                    None
                }
            }
        };
        if let Some(path) = &options.controls {
            if let Some(ini) = try_file(path, notes) {
                return Bindings { keep: false, ..Bindings::from_ini(&ini, format!("{} (--controls)", path.display())) };
            }
        }
        if options.default_controls {
            return Bindings { keep: false, ..Bindings::from_ini(&default_ini(), "the built-in layout (--default-controls)".to_string()) };
        }
        if let Some(path) = own_controls_path().filter(|p| p.is_file()) {
            if let Some(ini) = try_file(&path, notes) {
                return Bindings::from_ini(&ini, format!("{} (yours to edit; delete it to read AC's bindings again)", path.display()));
            }
        }
        if let Some(path) = ac_controls_path() {
            if path.is_file() {
                if let Some(ini) = try_file(&path, notes) {
                    return Bindings::from_ini(&ini, format!("{} (AC's own, read only)", path.display()));
                }
            } else {
                notes.push(format!("{}: not there", path.display()));
            }
        }
        Bindings::from_ini(&default_ini(), "the built-in layout".to_string())
    }

    /// The text of `rustyac_controls.ini`.
    pub fn file_text(&self) -> String {
        format!(
            "; rustyAC's bindings. The format is Assetto Corsa's controls.ini; the sections whose names start\n\
             ; with RUSTYAC are rustyAC's own. First written from: {}\n\
             ; XBOXBUTTON: A B X Y LSHOULDER RSHOULDER DPAD_LEFT DPAD_RIGHT DPAD_UP DPAD_DOWN LTHUMB_PRESS\n\
             ;             RTHUMB_PRESS START BACK, or -1 for none. KEY: a Windows virtual-key code (0x20 = Space), -1 = none.\n\
             ; [RUSTYAC_KEYS_2] gives every action a second key. Delete this file to start again from AC's bindings.\n\n{}",
            self.origin,
            self.ini.to_text()
        )
    }

    /// Writes `rustyac_controls.ini` next to the program unless it is there already (or the
    /// bindings were only asked for on the command line).
    pub fn write_if_missing(&self) -> Result<Option<PathBuf>, String> {
        let Some(path) = own_controls_path() else { return Ok(None) };
        if path.exists() || !self.keep {
            return Ok(None);
        }
        std::fs::write(&path, self.file_text()).map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(Some(path))
    }

    /// The active mapping, for the console.
    pub fn describe(&self) -> String {
        let ini = &self.ini;
        let keys = Keys::from_ini(ini);
        let keys2 = Keys::second_from_ini(ini);
        let key_pair = |name: &str| -> String {
            let find = |keys: &Keys| keys.named().into_iter().find(|(n, _)| *n == name).map(|(_, code)| code).unwrap_or(0);
            let names: Vec<String> = [find(&keys), find(&keys2)].into_iter().filter(|code| *code > 0).map(key_name).collect();
            if names.is_empty() {
                "-".to_string()
            } else {
                names.join(" or ")
            }
        };
        let pad = |section: &str| button_label(button_from_name(ini.get_string(section, "XBOXBUTTON")));
        let thumb = if ini.get_string("X360", "STEER_THUMB") == "LEFT" { "left stick" } else { "right stick" };
        let mut out = format!("bindings from {}\n", self.origin);
        out.push_str(&format!("  {:<24} {:<28} {}\n", "", "Xbox pad", "keyboard"));
        let mut row = |what: &str, pad: &str, keys: String| out.push_str(&format!("  {what:<24} {pad:<28} {keys}\n"));
        row("steer", thumb, format!("{} / {}", key_pair("LEFT"), key_pair("RIGHT")));
        row("gas", "RT", key_pair("GAS"));
        row("brake", "LT", key_pair("BRAKE"));
        row("gear up", pad("GEARUP"), key_pair("GEARUP"));
        row("gear down", pad("GEARDN"), key_pair("GEARDN"));
        row("clutch (to the floor)", pad("__EXT_KEYBOARD_CLUTCH"), key_pair("CLUTCH"));
        row("handbrake", pad("HANDBRAKE"), key_pair("HANDBRAKE"));
        row("DRS", pad("DRS"), key_pair("DRS"));
        row("KERS / ERS (no-op)", pad("KERS"), key_pair("KERS"));
        row("headlights", pad("ACTION_HEADLIGHTS"), key_pair("ACTION_HEADLIGHTS"));
        row("brake bias + / -", &format!("{} / {}", pad("BALANCEUP"), pad("BALANCEDN")), format!("{} / {}", key_pair("BALANCEUP"), key_pair("BALANCEDN")));
        // a Ctrl key that is bound as a driving key does not make a command
        let drives = |key: i32| [&keys, &keys2].iter().any(|k| k.named().iter().any(|(_, code)| *code == key));
        let ctrl = match (drives(0xa2) || drives(0x11), drives(0xa3) || drives(0x11)) {
            (false, false) => "Ctrl",
            (true, false) => "Right Ctrl",
            (false, true) => "Left Ctrl",
            (true, true) => "(no Ctrl key is free)",
        };
        row(
            "traction control + / -",
            &format!("{} / {}", pad("TCUP"), pad("TCDN")),
            format!("{ctrl}+{} (with Shift: down)", key_name(self.key_traction_control)),
        );
        row("ABS + / -", &format!("{} / {}", pad("ABSUP"), pad("ABSDN")), format!("{ctrl}+{} (with Shift: down)", key_name(self.key_abs)));
        row("automatic gearbox", "-", format!("{ctrl}+{}", key_name(self.key_auto_shifter)));
        row("camera", pad("ACTION_CHANGE_CAMERA"), "C".to_string());
        row("reset to spawn", button_label(self.pad_reset.button), "R (N: a new car)".to_string());
        row("back on track", &format!("{} held", button_label(self.pad_reset.button)), "Shift+R".to_string());
        row("pause", button_label(self.pad_pause.button), "P".to_string());
        row("quit", "-", "Esc".to_string());
        out.push_str(&format!(
            "  pad steering: dead zone {}, gamma {}, speed {}, filter {}, speed sensitivity {}, rumble {}{}\n",
            ini.get_float("X360", "STEER_DEADZONE"),
            ini.get_float("X360", "STEER_GAMMA"),
            ini.get_float("X360", "STEER_SPEED"),
            ini.get_float("X360", "STEER_FILTER"),
            ini.get_float("X360", "SPEED_SENSITIVITY"),
            ini.get_float("X360", "RUMBLE_INTENSITY"),
            if self.use_legacy_gamepad_code { ", legacy code" } else { "" }
        ));
        out.push_str(&format!(
            "  keyboard steering: speed {}, opposite {}, reset {}\n",
            ini.get_float("KEYBOARD", "STEERING_SPEED"),
            ini.get_float("KEYBOARD", "STEERING_OPPOSITE_DIRECTION_SPEED"),
            ini.get_float("KEYBOARD", "STEER_RESET_SPEED")
        ));
        if button_from_name(ini.get_string("__EXT_KEYBOARD_CLUTCH", "XBOXBUTTON")) == button::NONE {
            out.push_str("  (no clutch button on the pad: the automatic clutch does it all, as in AC)\n");
        }
        out
    }
}

/// A virtual-key code as a person reads it.
pub fn key_name(code: i32) -> String {
    match code {
        0x08 => "Backspace".to_string(),
        0x09 => "Tab".to_string(),
        0x0d => "Enter".to_string(),
        0x10 => "Shift".to_string(),
        0x11 => "Ctrl".to_string(),
        0x12 => "Alt".to_string(),
        0x20 => "Space".to_string(),
        0x21 => "PageUp".to_string(),
        0x22 => "PageDown".to_string(),
        0x25 => "Left".to_string(),
        0x26 => "Up".to_string(),
        0x27 => "Right".to_string(),
        0x28 => "Down".to_string(),
        0x30..=0x39 | 0x41..=0x5a => (code as u8 as char).to_string(),
        0x60..=0x69 => format!("Num{}", code - 0x60),
        0x70..=0x87 => format!("F{}", code - 0x6f),
        0xa0 => "Left Shift".to_string(),
        0xa1 => "Right Shift".to_string(),
        0xa2 => "Left Ctrl".to_string(),
        0xa3 => "Right Ctrl".to_string(),
        0xa4 => "Left Alt".to_string(),
        code if code <= 0 => "-".to_string(),
        other => format!("key {other:#04x}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::keyboard::KeyboardCarControl;
    use crate::input::pad::JoypadCarControl;

    #[test]
    fn the_built_in_layout_is_the_one_asked_for() {
        let b = Bindings::from_ini(&default_ini(), "test".to_string());
        let pad = JoypadCarControl::from_ini(&b.ini, false);
        let at = |action: &str| pad.buttons[PAD_ACTIONS.iter().position(|a| *a == action).unwrap()].button;
        assert!(!pad.is_steer_with_right, "left stick");
        assert_eq!((at("GEARUP"), at("GEARDN"), at("DRS"), at("KERS")), (button::Y, button::X, button::LEFT_SHOULDER, button::B));
        assert_eq!(pad.clutch.button, button::A);
        assert_eq!((b.pad_reset.button, b.pad_pause.button), (button::BACK, button::START));
        let keyboard = KeyboardCarControl::from_ini(&b.ini);
        assert_eq!((keyboard.keys.gas, keyboard.keys.brake, keyboard.keys.left, keyboard.keys.right), (0x26, 0x28, 0x25, 0x27));
        assert_eq!((keyboard.keys2.gas, keyboard.keys2.brake, keyboard.keys2.left, keyboard.keys2.right), (0x57, 0x53, 0x41, 0x44));
        assert_eq!(keyboard.steer_speed, 1.75);
        let text = b.describe();
        assert!(text.contains("gear up") && text.contains("Space or E"), "{text}");
        // the built-in layout binds no DirectInput device: no axis, no button
        let wheel = crate::input::wheel::DiCarControl::from_ini(&b.ini);
        assert_eq!((wheel.steer.joy, wheel.steer.index, wheel.gas.joy, wheel.brake.index), (-1, -1, -1, -1));
        assert_eq!((wheel.gear_up.joy, wheel.gear_up.index, wheel.drs.index, wheel.hand_brake.index), (-1, -1, -1, -1));
        assert!(b.keep);
    }

    #[test]
    fn the_file_reads_back_as_written() {
        let b = Bindings::from_ini(&default_ini(), "test".to_string());
        let again = Bindings::from_ini(&ControlsIni::parse(&b.file_text()), "test".to_string());
        assert_eq!(b, again);
        // an AC file without a [KEYBOARD] section still gives a keyboard that steers
        let pad_only = Bindings::from_ini(&ControlsIni::parse("[HEADER]\nINPUT_METHOD=X360\n[X360]\nSTEER_THUMB=LEFT\n"), "test".to_string());
        assert_eq!(KeyboardCarControl::from_ini(&pad_only.ini).keys.left, 0x25);
        assert!(!usable(&ControlsIni::parse("[VIDEO]\nX=1\n")));
    }
}
