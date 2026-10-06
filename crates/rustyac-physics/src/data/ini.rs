//! 1:1 port of AC's `INIReader` for plain (extracted) ini files: the parser with all its
//! quirks, `hasSection` / `hasKey` / `getString`, the number readers and `getCurve`.
//!
//! Not ported: reading straight out of an encrypted `data.acd` (`INIReader::loadEncrypt`),
//! the static file cache, and the error pop-ups. A file that sits next to a `<folder>.acd`
//! is refused instead of being read differently from the game.
//!
//! Quirks that are AC's, not this port's:
//! * a line that contains both `[` and `]` **anywhere** is a section header, even inside a
//!   comment (`KEY=1 ; [deg]` starts section `deg` and `KEY` is lost);
//! * a repeated section header empties the section;
//! * a repeated key keeps its first non-empty value;
//! * keys are not trimmed, values only at the end (space, tab, CR);
//! * numbers are parsed like C `wcstod` / `wcstol`: leading number, the rest is ignored.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::curve::Curve;
use crate::math::{wcstod, wcstol};

/// AC's `INIReader` (0x68 bytes): `filename`, `ready` and the `sections` map.
#[derive(Clone, Debug, Default)]
pub struct IniReader {
    /// `filename`
    pub filename: PathBuf,
    /// `ready`: the file could be opened.
    pub ready: bool,
    /// `sections`: section name -> (key -> value), ordered like the game's `std::map`s.
    sections: BTreeMap<String, BTreeMap<String, String>>,
}

/// `Path::getPath(file) + L".acd"`: the archive the game would read instead of the file.
pub(crate) fn sibling_acd(file: &Path) -> Option<PathBuf> {
    let dir = file.parent()?;
    let name = dir.file_name()?;
    let mut acd = name.to_os_string();
    acd.push(".acd");
    Some(dir.with_file_name(acd))
}

/// What a text-mode `fopen` gives the stream: CR LF becomes LF and Ctrl-Z ends the file.
pub(crate) fn text_mode(bytes: &[u8]) -> Vec<u8> {
    let end = bytes.iter().position(|&b| b == 0x1a).unwrap_or(bytes.len());
    let bytes = &bytes[..end];
    let mut out = Vec::with_capacity(bytes.len());
    for (i, &b) in bytes.iter().enumerate() {
        if b == b'\r' && bytes.get(i + 1) == Some(&b'\n') {
            continue;
        }
        out.push(b);
    }
    out
}

/// `std::codecvt_utf8<wchar_t, 0x10ffff, 0>::do_in` as shipped with Visual C++ 2013, which
/// `INIReader::load` imbues its file stream with: lenient about overlong forms, 16-bit
/// output, and the stream simply ends at the first byte that is not valid UTF-8.
fn decode_utf8_like_the_game(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let by = bytes[i] as u32;
        let (mut ch, extra) = match by {
            0x00..=0x7f => (by, 0),
            0x80..=0xbf => break,
            0xc0..=0xdf => (by & 0x1f, 1),
            0xe0..=0xef => (by & 0x0f, 2),
            0xf0..=0xf7 => (by & 0x07, 3),
            _ => (by & 0x03, if by < 0xfc { 4 } else { 5 }),
        };
        if bytes.len() - i < extra + 1 {
            break;
        }
        let mut bad = false;
        for &next in &bytes[i + 1..i + 1 + extra] {
            if !(0x80..0xc0).contains(&next) {
                bad = true;
                break;
            }
            ch = ch << 6 | (next as u32 & 0x3f);
        }
        if bad || ch > 0x10ffff {
            break;
        }
        i += extra + 1;
        // stored into a 16-bit wchar_t; halves of surrogate pairs cannot be a Rust char
        out.push(char::from_u32(ch & 0xffff).unwrap_or(char::REPLACEMENT_CHARACTER));
    }
    out
}

/// `trim2` @ 0x140238290: cut after the last character that is not space, tab or CR; a
/// string of only those becomes empty. The front is left alone.
fn trim2(text: &mut String) {
    match text.rfind(|c| !matches!(c, ' ' | '\t' | '\r')) {
        Some(last) => {
            let keep = last + text[last..].chars().next().map_or(0, char::len_utf8);
            text.truncate(keep);
        }
        None => text.clear(),
    }
}

impl IniReader {
    /// `INIReader::INIReader(const std::wstring&)` @ 0x1402340a0 + `INIReader::load`
    /// @ 0x140237140, for a file that is not inside an archive.
    pub fn load(path: &Path) -> Result<IniReader, String> {
        if let Some(acd) = sibling_acd(path).filter(|acd| acd.is_file()) {
            return Err(format!(
                "{} exists: the game would read {} from that archive, which is not ported; \
                 extract it with tools/acd_extract.py",
                acd.display(),
                path.display()
            ));
        }
        let mut reader = IniReader {
            filename: path.to_path_buf(),
            ..IniReader::default()
        };
        // a file that cannot be opened leaves `ready == false` and no sections
        if let Ok(bytes) = std::fs::read(path) {
            reader.ready = true;
            reader.parse(&decode_utf8_like_the_game(&text_mode(&bytes)));
        }
        Ok(reader)
    }

    /// An in-memory file, for tests; `filename` is where relative curve files are looked up.
    pub fn from_text(filename: &Path, code: &str) -> IniReader {
        let mut reader = IniReader {
            filename: filename.to_path_buf(),
            ready: true,
            sections: BTreeMap::new(),
        };
        reader.parse(code);
        reader
    }

    /// `INIReader::parse` @ 0x140237680
    fn parse(&mut self, code: &str) {
        let mut current_section = String::new();
        // std::getline(stream, line, L'\n')
        for line in code.split('\n') {
            let open = line.find('[');
            let close = line.find(']');
            if let (Some(open), Some(close)) = (open, close) {
                // substr(open + 1, close - open - 1); a negative count runs to the end
                current_section = if close > open {
                    line[open + 1..close].to_string()
                } else {
                    line[open + 1..].to_string()
                };
                self.sections
                    .insert(current_section.clone(), BTreeMap::new());
                continue;
            }
            let Some(equals) = line.find('=') else {
                continue;
            };
            let comment = line.find(';');
            if current_section.is_empty() || comment.is_some_and(|c| c <= equals) {
                continue;
            }
            let key = &line[..equals];
            let mut value = match comment {
                Some(c) => line[equals + 1..c].to_string(),
                None => line[equals + 1..].to_string(),
            };
            trim2(&mut value);
            let stored = self
                .sections
                .entry(current_section.clone())
                .or_default()
                .entry(key.to_string())
                .or_default();
            // a repeat is reported ("STILL USING OLD VALUE") and ignored
            if stored.is_empty() {
                *stored = value;
            }
        }
    }

    /// `INIReader::hasSection` @ 0x1402370b0
    pub fn has_section(&self, section: &str) -> bool {
        self.sections.contains_key(section)
    }

    /// `INIReader::hasKey` @ 0x140236f00
    pub fn has_key(&self, section: &str, key: &str) -> bool {
        self.sections
            .get(section)
            .is_some_and(|keys| keys.contains_key(key))
    }

    /// `INIReader::getString` @ 0x1402360f0: the value, or an empty string when the section
    /// or key is missing (the game reports `KEY_NOT_FOUND` and carries on).
    pub fn get_string(&self, section: &str, key: &str) -> String {
        self.sections
            .get(section)
            .and_then(|keys| keys.get(key))
            .cloned()
            .unwrap_or_default()
    }

    /// `INIReader::getFloat` @ 0x1402358c0. An empty value (or a missing key) is 0; text
    /// that does not start with a number makes the game throw, which is the `Err` here.
    pub fn get_float(&self, section: &str, key: &str) -> Result<f32, String> {
        let text = self.get_string(section, key);
        let mut chars = text.chars();
        if let (Some(only), None) = (chars.next(), chars.next()) {
            // one character that isspace(): 0 without parsing
            if matches!(only, ' ' | '\t' | '\n' | '\u{b}' | '\u{c}' | '\r') {
                return Ok(0.0);
            }
        }
        if text.is_empty() {
            return Ok(0.0);
        }
        let parsed = wcstod(&text);
        if parsed.consumed == 0 {
            return Err(self.error(section, key, &text, "invalid stof argument"));
        }
        if parsed.out_of_range {
            return Err(self.error(section, key, &text, "stof argument out of range"));
        }
        Ok(parsed.value as f32)
    }

    /// `INIReader::getFloat3` @ 0x1402357a0 (through `INIReader::getVector3` @ 0x140236540):
    /// three numbers separated by commas (`wcstok_s`, so empty pieces are skipped), each
    /// parsed like [`IniReader::get_float`]. An empty value (or a missing key) is (0, 0, 0),
    /// and so is a value with fewer than three numbers (the game reports `NOT_3_ELEMENTS`).
    pub fn get_float3(&self, section: &str, key: &str) -> Result<[f32; 3], String> {
        let text = self.get_string(section, key);
        let mut out = [0.0f32; 3];
        // longer than 1256 characters: "STRING_LONGER_THAN_1256", nothing is read
        if text.is_empty() || text.encode_utf16().count() > 0x4e7 {
            return Ok(out);
        }
        let mut tokens = text.split(',').filter(|piece| !piece.is_empty());
        for slot in &mut out {
            let Some(token) = tokens.next() else {
                return Ok([0.0; 3]);
            };
            let parsed = wcstod(token);
            if parsed.consumed == 0 {
                return Err(self.error(section, key, &text, "invalid stof argument"));
            }
            if parsed.out_of_range {
                return Err(self.error(section, key, &text, "stof argument out of range"));
            }
            *slot = parsed.value as f32;
        }
        Ok(out)
    }

    /// `INIReader::getInt` @ 0x140235c70: `wcstol(text, .., 10)`, 0 for an empty value.
    pub fn get_int(&self, section: &str, key: &str) -> Result<i32, String> {
        let text = self.get_string(section, key);
        if text.is_empty() {
            return Ok(0);
        }
        let parsed = wcstol(&text);
        if parsed.consumed == 0 {
            return Err(self.error(section, key, &text, "invalid stoi argument"));
        }
        if parsed.out_of_range {
            return Err(self.error(section, key, &text, "stoi argument out of range"));
        }
        Ok(parsed.value)
    }

    /// `INIReader::getCurve` @ 0x140235040. The value is either an inline table
    /// `(x=y|x=y|...)` or the name of a `.lut` file next to the ini. A file that does not
    /// exist gives an empty curve, silently, like the game.
    pub fn get_curve(&self, section: &str, key: &str) -> Result<Curve, String> {
        let text = self.get_string(section, key);
        let (Some(open), Some(close)) = (text.find('('), text.find(')')) else {
            // Path::getPath(filename) + L"/" + value
            let path = self.filename.parent().unwrap_or(Path::new("")).join(&text);
            let mut curve = Curve::new();
            if path.is_file() {
                curve.load(&path)?;
            }
            return Ok(curve);
        };
        // substr(open + 1, close - open - 1)
        let inner = if close > open {
            &text[open + 1..close]
        } else {
            &text[open + 1..]
        };
        let mut curve = Curve::new();
        // ksSplitString is wcstok: empty pieces are dropped
        for pair in inner.split('|').filter(|p| !p.is_empty()) {
            let parts: Vec<&str> = pair.split('=').filter(|p| !p.is_empty()).collect();
            let [reference, value] = parts[..] else {
                return Err(self.error(
                    section,
                    key,
                    &text,
                    &format!("is not a valid curve, at value:{pair}"),
                ));
            };
            // the value is parsed first, like the original
            let value = wcstod(value);
            let reference = wcstod(reference);
            if value.consumed == 0 || reference.consumed == 0 {
                return Err(self.error(section, key, &text, "invalid stof argument"));
            }
            if value.out_of_range || reference.out_of_range {
                return Err(self.error(section, key, &text, "stof argument out of range"));
            }
            curve.add_value(reference.value as f32, value.value as f32);
        }
        Ok(curve)
    }

    /// Section names, in the game's map order.
    pub fn sections(&self) -> impl Iterator<Item = &str> {
        self.sections.keys().map(String::as_str)
    }

    fn error(&self, section: &str, key: &str, value: &str, what: &str) -> String {
        format!(
            "{}: [{section}] {key}={value}: {what}",
            self.filename.display()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ini(code: &str) -> IniReader {
        IniReader::from_text(Path::new("mem/test.ini"), code)
    }

    #[test]
    fn plain_keys_and_comments() {
        let r = ini("[A]\nX=1.5 ; note\nY = 2\t\r\n; Z=3\nW=;4\n");
        assert_eq!(r.get_string("A", "X"), "1.5");
        // keys are not trimmed, values only at the end
        assert!(!r.has_key("A", "Y"));
        assert_eq!(r.get_string("A", "Y "), " 2");
        assert_eq!(r.get_float("A", "Y ").unwrap(), 2.0);
        assert!(!r.has_key("A", "Z"));
        assert!(r.has_key("A", "W"));
        assert_eq!(r.get_float("A", "W").unwrap(), 0.0);
        assert_eq!(r.get_float("A", "MISSING").unwrap(), 0.0);
        assert_eq!(r.get_int("A", "X").unwrap(), 1);
        assert!(!r.has_section("B"));
    }

    #[test]
    fn brackets_in_a_comment_start_a_section() {
        let r = ini("[A]\nX=1\nANGLE=8 ; [deg]\nY=2\n");
        assert!(!r.has_key("A", "ANGLE"));
        assert!(!r.has_key("A", "Y"));
        assert_eq!(r.get_string("deg", "Y"), "2");
    }

    #[test]
    fn repeats_keep_the_first_value_and_reset_sections() {
        let r = ini("[A]\nX=1\nX=2\nE=\nE=5\n[B]\nK=1\n[A]\nQ=7\n");
        assert_eq!(r.get_string("A", "X"), "");
        assert_eq!(r.get_string("A", "Q"), "7");
        let r = ini("[A]\nX=1\nX=2\nE=\nE=5\n");
        assert_eq!(r.get_string("A", "X"), "1");
        assert_eq!(r.get_string("A", "E"), "5");
        // no section yet: ignored
        assert!(ini("X=1\n[A]\n").sections().eq(["A"]));
    }

    #[test]
    fn numbers_that_are_not_numbers() {
        let r = ini("[A]\nX=abc\nY=1e400\n");
        assert!(r.get_float("A", "X").is_err());
        assert!(r.get_float("A", "Y").is_err());
        assert!(r.get_int("A", "X").is_err());
    }

    #[test]
    fn inline_curves() {
        let r = ini("[A]\nC=(0=1|10=3||20=2)\nBAD=(0=1|5)\nNOFILE=missing.lut\n");
        let c = r.get_curve("A", "C").unwrap();
        assert_eq!(c.get_count(), 3);
        assert_eq!(c.get_value(5.0), 2.0);
        assert!(r.get_curve("A", "BAD").is_err());
        assert_eq!(r.get_curve("A", "NOFILE").unwrap().get_count(), 0);
    }

    #[test]
    fn text_mode_and_utf8() {
        assert_eq!(text_mode(b"a\r\nb\rc\x1ad"), b"a\nb\rc");
        assert_eq!(decode_utf8_like_the_game("a\u{e9}b".as_bytes()), "a\u{e9}b");
        // a Latin-1 degree sign is not UTF-8: the stream ends there
        assert_eq!(decode_utf8_like_the_game(b"ab\xb0cd"), "ab");
    }
}
