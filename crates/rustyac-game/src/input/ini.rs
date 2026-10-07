//! `controls.ini` as the game's input classes read it: sections of `KEY=value` lines, and
//! getters with the game's answers for a missing key (`INIReader::getString` "" / `getFloat`
//! 0 / `getInt` 0 / `getHex` -1).

use std::collections::BTreeMap;
use std::path::Path;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ControlsIni {
    sections: BTreeMap<String, BTreeMap<String, String>>,
}

/// The number a text starts with, the way C's `wcstod` reads it: leading white space, a
/// sign, digits with an optional fraction and exponent; whatever follows is ignored. No
/// number at all gives `None`.
fn leading_number(text: &str) -> Option<f64> {
    let text = text.trim_start();
    let bytes = text.as_bytes();
    let mut end = 0;
    if end < bytes.len() && (bytes[end] == b'+' || bytes[end] == b'-') {
        end += 1;
    }
    let digits_from = end;
    while end < bytes.len() && bytes[end].is_ascii_digit() {
        end += 1;
    }
    if end < bytes.len() && bytes[end] == b'.' {
        end += 1;
        while end < bytes.len() && bytes[end].is_ascii_digit() {
            end += 1;
        }
    }
    if !bytes[digits_from..end].iter().any(u8::is_ascii_digit) {
        return None;
    }
    if end < bytes.len() && (bytes[end] == b'e' || bytes[end] == b'E') {
        let mut exponent = end + 1;
        if exponent < bytes.len() && (bytes[exponent] == b'+' || bytes[exponent] == b'-') {
            exponent += 1;
        }
        if exponent < bytes.len() && bytes[exponent].is_ascii_digit() {
            while exponent < bytes.len() && bytes[exponent].is_ascii_digit() {
                exponent += 1;
            }
            end = exponent;
        }
    }
    text[..end].parse().ok()
}

impl ControlsIni {
    pub fn load(path: &Path) -> Result<ControlsIni, String> {
        let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        // Content Manager writes UTF-8; a UTF-16 file (byte order mark) is read as such
        let text = if bytes.starts_with(&[0xff, 0xfe]) {
            let units: Vec<u16> = bytes[2..].chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
            String::from_utf16_lossy(&units)
        } else {
            String::from_utf8_lossy(bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(&bytes)).to_string()
        };
        Ok(ControlsIni::parse(&text))
    }

    pub fn parse(text: &str) -> ControlsIni {
        let mut ini = ControlsIni::default();
        let mut section: Option<String> = None;
        for line in text.lines() {
            // a comment runs from ';' (or "//") to the end of the line
            let line = line.split(';').next().unwrap_or("");
            let line = line.split("//").next().unwrap_or("").trim();
            if let Some(name) = line.strip_prefix('[').and_then(|rest| rest.split(']').next()) {
                section = Some(name.trim().to_string());
                ini.sections.entry(name.trim().to_string()).or_default();
                continue;
            }
            let (Some(section), Some((key, value))) = (&section, line.split_once('=')) else { continue };
            // the first of two equal keys counts
            ini.sections.get_mut(section).unwrap().entry(key.trim().to_string()).or_insert_with(|| value.trim().to_string());
        }
        ini
    }

    pub fn is_empty(&self) -> bool {
        self.sections.is_empty()
    }

    pub fn has_section(&self, section: &str) -> bool {
        self.sections.contains_key(section)
    }

    /// The keys of a section (none for a missing section).
    pub fn keys(&self, section: &str) -> Vec<String> {
        self.sections.get(section).map(|keys| keys.keys().cloned().collect()).unwrap_or_default()
    }

    pub fn has_key(&self, section: &str, key: &str) -> bool {
        self.sections.get(section).is_some_and(|keys| keys.contains_key(key))
    }

    /// `INIReader::getString`: "" for a missing section or key.
    pub fn get_string(&self, section: &str, key: &str) -> &str {
        self.sections.get(section).and_then(|keys| keys.get(key)).map(String::as_str).unwrap_or("")
    }

    /// `INIReader::getFloat`: 0 for a missing or empty value.
    pub fn get_float(&self, section: &str, key: &str) -> f32 {
        leading_number(self.get_string(section, key)).unwrap_or(0.0) as f32
    }

    /// `INIReader::getInt`: 0 for a missing or empty value.
    pub fn get_int(&self, section: &str, key: &str) -> i32 {
        leading_number(self.get_string(section, key)).map(|x| x as i32).unwrap_or(0)
    }

    /// `INIReader::getHex`: -1 for a missing value; else the text read as a hexadecimal
    /// number (`0x20` is 32; `-1` is -1).
    pub fn get_hex(&self, section: &str, key: &str) -> i32 {
        let text = self.get_string(section, key).trim();
        if text.is_empty() {
            return -1;
        }
        let (negative, digits) = match text.strip_prefix('-') {
            Some(rest) => (true, rest),
            None => (false, text),
        };
        let digits = digits.strip_prefix("0x").or_else(|| digits.strip_prefix("0X")).unwrap_or(digits);
        let end = digits.find(|c: char| !c.is_ascii_hexdigit()).unwrap_or(digits.len());
        match i64::from_str_radix(&digits[..end], 16) {
            Ok(value) => (if negative { -value } else { value }) as i32,
            // a stream that fails to read a number leaves the int at 0
            Err(_) => 0,
        }
    }

    pub fn set(&mut self, section: &str, key: &str, value: impl ToString) {
        self.sections.entry(section.to_string()).or_default().insert(key.to_string(), value.to_string());
    }

    /// The file's text, sections and keys in alphabetical order (as Content Manager writes it).
    pub fn to_text(&self) -> String {
        let mut out = String::new();
        for (section, keys) in &self.sections {
            out.push_str(&format!("[{section}]\n"));
            for (key, value) in keys {
                out.push_str(&format!("{key}={value}\n"));
            }
            out.push('\n');
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn getters_answer_like_the_game_s() {
        let ini = ControlsIni::parse(
            "[X360]\nSTEER_SPEED=0.95\nSTEER_GAMMA=1.4 ; comment\nEMPTY=\n\n[GEARUP]\nKEY=0x20\nXBOXBUTTON=Y\n[GEARDN]\nKEY=0xA2\n[DRS]\nKEY=-1\n",
        );
        assert_eq!(ini.get_float("X360", "STEER_SPEED"), 0.95);
        assert_eq!(ini.get_float("X360", "STEER_GAMMA"), 1.4);
        // missing key, missing section, empty value: 0, not a default
        assert_eq!(ini.get_float("X360", "RUMBLE_INTENSITY"), 0.0);
        assert_eq!(ini.get_float("NOPE", "X"), 0.0);
        assert_eq!(ini.get_float("X360", "EMPTY"), 0.0);
        assert_eq!(ini.get_int("X360", "EMPTY"), 0);
        assert_eq!(ini.get_string("GEARUP", "XBOXBUTTON"), "Y");
        assert_eq!(ini.get_string("GEARDN", "XBOXBUTTON"), "");
        // spec test 7
        assert_eq!(ini.get_hex("GEARUP", "KEY"), 32);
        assert_eq!(ini.get_hex("GEARDN", "KEY"), 162);
        assert_eq!(ini.get_hex("DRS", "KEY"), -1);
        assert_eq!(ini.get_hex("KERS", "KEY"), -1);
        assert!(ini.has_section("DRS") && !ini.has_section("ADVANCED"));
        // written and read back
        assert_eq!(ControlsIni::parse(&ini.to_text()), ini);
    }

    #[test]
    fn numbers_are_read_like_wcstod() {
        assert_eq!(leading_number("1.75"), Some(1.75));
        assert_eq!(leading_number("  -0.5abc"), Some(-0.5));
        assert_eq!(leading_number("1e2"), Some(100.0));
        assert_eq!(leading_number("3e"), Some(3.0));
        assert_eq!(leading_number(".5"), Some(0.5));
        assert_eq!(leading_number("LEFT"), None);
        assert_eq!(leading_number(""), None);
    }
}
