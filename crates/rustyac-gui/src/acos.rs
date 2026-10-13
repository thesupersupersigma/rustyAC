// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! The form layout file `cfg/acos.ini`: where every form is, whether it shows, is pinned and
//! how large it is, on each of the virtual desktops.
//!
//! `GUI::deserializeForm` 0x140252f90, `deserializeOldForm` 0x140253890, `fromIni`
//! 0x140253b20, `deserializeFormDesktop` 0x1402533a0, `serializeMemoryForm` 0x1402553e0,
//! `serializeForms` 0x140254d60 with its section writer 0x140252540, and the handler of the
//! app bar's desktop change 0x140252490.
//!
//! The game reads the file again for every form and desktop and writes it once, when the
//! session ends. This crate touches no file: the reader is handed in when the GUI is made and
//! [`Gui::serialize_forms`] gives the bytes to write (rustyAC keeps its own copy and never
//! writes into the game's Documents folder).

use crate::control::Id;
use crate::gui::{FormPlace, Gui};

/// A float as `std::wostream << float` writes it: `%g` with six significant digits.
pub fn format_g6(value: f32) -> String {
    let v = value as f64;
    if v.is_nan() {
        return if v.is_sign_negative() { "-1.#IND".into() } else { "1.#QNAN".into() };
    }
    if v.is_infinite() {
        return if v < 0.0 { "-1.#INF".into() } else { "1.#INF".into() };
    }
    if v == 0.0 {
        return if v.is_sign_negative() { "-0".into() } else { "0".into() };
    }
    // the exact decimal digits, rounded half away from zero at the sixth
    let exact = format!("{:.80e}", v.abs());
    let (mantissa, exponent) = exact.split_once('e').unwrap_or((&exact, "0"));
    let mut exponent: i32 = exponent.parse().unwrap_or(0);
    let mut digits: Vec<u8> = mantissa.bytes().filter(|b| b.is_ascii_digit()).map(|b| b - b'0').collect();
    let round_up = digits.get(6).is_some_and(|d| *d >= 5);
    digits.truncate(6);
    if round_up {
        let mut i = 5;
        loop {
            if digits[i] == 9 {
                digits[i] = 0;
                if i == 0 {
                    digits.insert(0, 1);
                    digits.truncate(6);
                    exponent += 1;
                    break;
                }
                i -= 1;
            } else {
                digits[i] += 1;
                break;
            }
        }
    }
    let mut out = String::new();
    if v < 0.0 {
        out.push('-');
    }
    let text: String = digits.iter().map(|d| (b'0' + d) as char).collect();
    if !(-4..6).contains(&exponent) {
        // d.ddddde+XXX, zeros at the end dropped
        let rest = text[1..].trim_end_matches('0');
        out.push_str(&text[..1]);
        if !rest.is_empty() {
            out.push('.');
            out.push_str(rest);
        }
        out.push_str(&format!("e{}{:03}", if exponent < 0 { '-' } else { '+' }, exponent.abs()));
    } else if exponent < 0 {
        out.push_str("0.");
        for _ in 0..(-exponent - 1) {
            out.push('0');
        }
        out.push_str(text.trim_end_matches('0'));
    } else {
        let whole = exponent as usize + 1;
        out.push_str(&text[..whole]);
        let rest = text[whole..].trim_end_matches('0');
        if !rest.is_empty() {
            out.push('.');
            out.push_str(rest);
        }
    }
    out
}

impl Gui {
    fn desk_key(desktop: i32, name: &str) -> String {
        format!("DESK_{desktop}_FORM_{name}")
    }

    /// `GUI::fromIni` 0x140253b20: a section of the layout file; the place is kept on the
    /// screen (not past 80 pixels from its right edge, not above 80 from the top, not below
    /// the bottom).
    pub fn from_ini(&self, section: &str) -> FormPlace {
        let Some(ini) = self.acos.as_ref().filter(|ini| ini.has_section(section)) else { return FormPlace::default() };
        let px = ini.get_float(section, "POSX").unwrap_or(0.0);
        let py = ini.get_float(section, "POSY").unwrap_or(0.0);
        let (w, h) = (self.screen.0 as f32, self.screen.1 as f32);
        let x = if px > w - 80.0 { w - 80.0 } else { px };
        let y = if py > h {
            h
        } else if py >= 80.0 {
            py
        } else {
            80.0
        };
        FormPlace {
            x,
            y,
            visible: ini.get_int(section, "VISIBLE").unwrap_or(0) != 0,
            blocked: ini.get_int(section, "BLOCKED").unwrap_or(0) != 0,
            scale: if ini.has_key(section, "SCALE") { ini.get_float(section, "SCALE").unwrap_or(0.0) } else { 1.0 },
        }
    }

    /// The five calls that put a form where a `FormData` has it.
    fn apply_place(&mut self, form: Id, place: FormPlace, update_scale: bool) {
        self.set_position(form, place.x, place.y);
        self.form_data_mut(form).scale_to_be_saved = place.scale;
        self.controls[form].scale_mult = place.scale;
        if update_scale {
            self.update_scale_by_mult(form);
        }
        self.set_visible(form, place.visible);
        self.set_blocked(form, place.blocked);
    }

    /// `GUI::deserializeForm` 0x140252f90 (from `Form::Form`): the form's place on every
    /// desktop is read; the current desktop's is applied.
    pub fn deserialize_form(&mut self, form: Id) {
        let name = self.controls[form].name.clone();
        if self.version < 2 {
            // GUI::deserializeOldForm 0x140253890: one section a form, no desktops
            let place = self.from_ini(&format!("FORM_{name}"));
            self.apply_place(form, place, false);
            return;
        }
        let mut i = 0;
        while (i as usize) < self.taskbar_data(self.taskbar).desktops.len() {
            let key = Gui::desk_key(i, &name);
            let place = self.from_ini(&key);
            if i == self.virtual_desktop {
                self.apply_place(form, place, true);
            }
            self.form_data_map.insert(key, place);
            i += 1;
        }
    }

    /// `GUI::deserializeFormDesktop` 0x1402533a0: the form as it was left on that desktop.
    pub fn deserialize_form_desktop(&mut self, form: Id, desktop: i32) {
        let key = Gui::desk_key(desktop, &self.controls[form].name);
        let place = *self.form_data_map.entry(key).or_default();
        self.apply_place(form, place, true);
    }

    /// `GUI::serializeMemoryForm` 0x1402553e0: the form as it is, kept for the current desktop.
    pub fn serialize_memory_form(&mut self, form: Id) {
        let key = Gui::desk_key(self.virtual_desktop, &self.controls[form].name);
        let c = &self.controls[form];
        let d = self.form_data(form);
        let place = FormPlace { x: c.rect.left, y: c.rect.top, visible: c.visible, blocked: d.blocked, scale: d.scale_to_be_saved };
        self.form_data_map.insert(key, place);
    }

    /// The GUI's handler of the app bar's `evOnDesktopChange` (0x140252490): every form of the
    /// bar is remembered for the old desktop and put as the new one has it.
    pub fn on_desktop_change(&mut self, desktop: i32) {
        if self.virtual_desktop != desktop {
            for form in self.taskbar_data(self.taskbar).forms.clone() {
                self.serialize_memory_form(form);
                self.deserialize_form_desktop(form, desktop);
            }
            self.virtual_desktop = desktop;
        }
    }

    /// `GUI::serializeForms` 0x140254d60: the bytes of `acos.ini` (`None`: serialisation is
    /// switched off). The stream is a wide one in text mode with the "C" locale: a line ends
    /// in CR LF, and a character above 0xff ends the file there, as it makes the game's stream
    /// fail.
    pub fn serialize_forms(&mut self) -> Option<Vec<u8>> {
        if !self.allow_form_serialization {
            return None;
        }
        let mut text = String::new();
        text.push_str("[HEADER]\n");
        text.push_str("VERSION=2\n");
        text.push_str(&format!("DESKTOP_SELECTED={}\n\n", self.virtual_desktop + 1));
        let mut i = 0;
        while (i as usize) < self.taskbar_data(self.taskbar).desktops.len() {
            for form in self.taskbar_data(self.taskbar).forms.clone() {
                if i == self.virtual_desktop {
                    self.serialize_memory_form(form);
                }
                let key = Gui::desk_key(i, &self.controls[form].name);
                let place = *self.form_data_map.entry(key.clone()).or_default();
                text.push_str(&format!("[{key}]\n"));
                text.push_str(&format!("POSX={}\n", format_g6(place.x)));
                text.push_str(&format!("POSY={}\n", format_g6(place.y)));
                text.push_str(&format!("VISIBLE={}\n", place.visible as i32));
                text.push_str(&format!("BLOCKED={}\n", place.blocked as i32));
                text.push_str(&format!("SCALE={}\n\n", format_g6(place.scale)));
            }
            i += 1;
        }
        let mut bytes = Vec::with_capacity(text.len() + text.len() / 8);
        for unit in text.encode_utf16() {
            if unit > 0xff {
                break;
            }
            if unit == b'\n' as u16 {
                bytes.push(b'\r');
            }
            bytes.push(unit as u8);
        }
        Some(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::format_g6;

    #[test]
    fn floats_as_the_stream_writes_them() {
        for (value, text) in [(0.0, "0"), (60.0, "60"), (697.5, "697.5"), (1.3, "1.3"), (1.25, "1.25"), (80.0, "80"), (-12.0, "-12"), (1234567.0, "1.23457e+006"), (0.00001, "1e-005"), (123456.5, "123457"), (0.1, "0.1"), (1919.0, "1919"), (0.75, "0.75"), (100000.0, "100000"), (999999.5, "1e+006")] {
            assert_eq!(format_g6(value), text, "{value}");
        }
    }
}
