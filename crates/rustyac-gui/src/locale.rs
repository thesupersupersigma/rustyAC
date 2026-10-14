// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! The game's string tables of the setup editor (`LocalizationManager`): the flat map
//! `strings` that `loadSetupINI` 0x1402414e0 fills from `system/locales/setup/<lang>.ini`
//! (`acLocaleGetString` 0x1402404a0), and the help texts of `system/locales/setup/<lang>.tag`
//! (`TagDocumentReader::load` 0x14044e030, `acLocaleGetSetupHelpText` 0x140240410).
//!
//! `acTranslate` 0x140240cf0 reads `translatedText`, the `[STRINGS]` of
//! `system/locales/<lang>.ini` (`loadMainINI` 0x140240ee0), kept here for the whole process as
//! in the game (a global); [`crate::hud::translate`] is it. A string the table does not have
//! comes back as it is.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::RwLock;

use rustyac_physics::data::ini::{read_text_utf8, IniReader};

/// `TagDocumentText`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TagDocumentText {
    pub text: String,
    pub lines_count: u32,
}

/// `translatedText` (none: no table was loaded, every string is itself).
static TRANSLATED_TEXT: RwLock<Option<BTreeMap<String, String>>> = RwLock::new(None);

/// `acTranslate` 0x140240cf0.
pub fn ac_translate(text: &str) -> String {
    match TRANSLATED_TEXT.read() {
        Ok(table) => table.as_ref().and_then(|t| t.get(text)).cloned().unwrap_or_else(|| text.to_string()),
        Err(_) => text.to_string(),
    }
}

/// `loadMainINI` 0x140240ee0 on `system/locales/<language>.ini` of the game's folder `root`:
/// every key of `[STRINGS]` and its value, a written `\n` in either made a line break, into
/// `translatedText` (none: the file is not there, "CANNOT OPEN LOCALIZATION FILE").
pub fn load_main_ini(root: &Path, language: &str) -> Option<BTreeMap<String, String>> {
    let ini = IniReader::load(&root.join(format!("system/locales/{language}.ini"))).ok().filter(|ini| ini.ready)?;
    let mut table = BTreeMap::new();
    for key in ini.keys("STRINGS") {
        let value = ini.get_string("STRINGS", key);
        table.insert(key.replace("\\n", "\n"), value.replace("\\n", "\n"));
    }
    Some(table)
}

/// The language `initLocalization` 0x1400c15c0 sets: `Documents\Assetto Corsa\cfg\launcher.ini
/// [WINDOW] LANGUAGE` when that file can be read (an empty value when the key is missing),
/// else "en". `forced`: `--language`. `documents_cfg`: the Documents `cfg` folder.
pub fn language(documents_cfg: Option<&Path>, forced: Option<&str>) -> String {
    if let Some(code) = forced {
        return code.to_string();
    }
    match documents_cfg.and_then(|folder| IniReader::load(&folder.join("launcher.ini")).ok()).filter(|ini| ini.ready) {
        Some(ini) => ini.get_string("WINDOW", "LANGUAGE"),
        None => "en".to_string(),
    }
}

/// The font locale `acLocaleSetLocale` gives `kglSetFontLocale` for a language (none: the
/// default one).
pub fn font_locale(language: &str) -> Option<&'static str> {
    match language {
        "chs" => Some("zh-CN"),
        "cht" => Some("zh-HK"),
        _ => None,
    }
}

#[derive(Clone, Debug, Default)]
pub struct Locale {
    /// `strings`
    pub strings: BTreeMap<String, String>,
    /// `setupHelp.document`
    pub setup_help: BTreeMap<String, TagDocumentText>,
    /// `currentLocale`
    pub language: String,
}

/// `TagDocumentReader::load` 0x14044e030 on the file's text (as the text-mode stream gives it):
/// a line state machine. The line after a header is always taken; the first line of a
/// section is not counted; a file that ends with a line break gives its last section one
/// more empty line.
pub fn parse_tag_document(text: &str) -> BTreeMap<String, TagDocumentText> {
    let mut document = BTreeMap::new();
    // std::getline: line i of the split; the stream's eof bit is set by the last of them
    let lines: Vec<&str> = text.split('\n').collect();
    let mut next = 0usize;
    let mut eof = false;
    let mut getline = |eof: &mut bool| -> String {
        let line = lines.get(next).copied().unwrap_or("");
        next += 1;
        *eof = next >= lines.len();
        line.to_string()
    };
    let mut header = getline(&mut eof);
    while !eof {
        let mut body = getline(&mut eof);
        let mut text = format!("{body}\n");
        let mut count = 0u32;
        while !eof {
            if body.starts_with('[') {
                break;
            }
            body = getline(&mut eof);
            if !body.starts_with('[') {
                text.push_str(&body);
                text.push('\n');
                count += 1;
            }
        }
        // substr(1, size - 2)
        let units: Vec<u16> = header.encode_utf16().collect();
        let key = if units.len() >= 2 { String::from_utf16_lossy(&units[1..units.len() - 1]) } else { String::new() };
        document.insert(key, TagDocumentText { text, lines_count: count });
        if body.starts_with('[') {
            header = body;
        }
    }
    document
}

impl Locale {
    /// `acLocaleSetLocale` 0x140240980. `root` is the game's folder. `translatedText` (the
    /// process's, [`ac_translate`]) is filled first; the setup editor's tables only when it
    /// could be ("Set locale failed" otherwise: every string stays itself).
    pub fn load(root: &Path, language: &str) -> Locale {
        let mut locale = Locale { language: language.to_string(), ..Locale::default() };
        let main = load_main_ini(root, language);
        let failed = main.is_none();
        if let Ok(mut table) = TRANSLATED_TEXT.write() {
            // (operator[] on the game's map: a second locale adds to the first)
            let into = table.get_or_insert_with(BTreeMap::new);
            into.extend(main.unwrap_or_default());
        }
        if failed {
            return locale;
        }
        // loadSetupINI: every key of every section, whatever the section is called
        if let Ok(ini) = IniReader::load(&root.join(format!("system/locales/setup/{language}.ini"))) {
            let sections: Vec<String> = ini.sections().map(str::to_string).collect();
            for section in sections {
                for key in ini.keys(&section) {
                    locale.strings.insert(key.to_string(), ini.get_string(&section, key));
                }
            }
        }
        if let Some(text) = read_text_utf8(&root.join(format!("system/locales/setup/{language}.tag"))) {
            locale.setup_help = parse_tag_document(&text);
        }
        locale
    }

    /// `acLocaleGetString` 0x1402404a0: empty for a string the table does not have.
    pub fn get_string(&self, key: &str) -> String {
        self.strings.get(key).cloned().unwrap_or_default()
    }

    /// `acLocaleGetSetupHelpText` 0x140240410.
    pub fn setup_help_text(&self, key: &str) -> String {
        self.setup_help.get(key).map(|t| t.text.clone()).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tag_document() {
        let d = parse_tag_document("[A]\none\ntwo\n\n[B]\nthree\n");
        assert_eq!(d["A"], TagDocumentText { text: "one\ntwo\n\n".to_string(), lines_count: 2 });
        // the file ends with a line break: one more empty line
        assert_eq!(d["B"], TagDocumentText { text: "three\n\n".to_string(), lines_count: 1 });
        let d = parse_tag_document("[A]\none");
        assert_eq!(d["A"], TagDocumentText { text: "one\n".to_string(), lines_count: 0 });
    }
}
