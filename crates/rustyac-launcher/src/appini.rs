// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa's launcher (AssettoCorsa.exe, Kunos Simulazioni).

//! `AC.Launcher.Utility.AppIniFile`: the player's copy of a cfg file (`Get`), the tidy-up the
//! launcher gives a file before the game reads it (`Format`), a file as JSON for the theme
//! (`GetAsJson`) and the merge of the game's defaults into the player's files (`Merge`).

use crate::folders::AppFolder;
use crate::ini::IniFile;
use crate::jss::{self, Value};
use crate::net;

/// `AppIniFile.Get(name)`: `cfg\<name>.ini` of the player, copied from the game's when missing.
pub fn get(folders: &AppFolder, name: &str) -> String {
    let name = format!("{name}.ini");
    let user = format!("{}{name}", folders.user_config_dir);
    let default = format!("{}{name}", folders.default_config_dir);
    if !net::file_exists(&user) && net::file_exists(&default) {
        let _ = net::copy(&default, &user);
    }
    user
}

/// `AppIniFile.Format(file)`: keeps the lines with a `=`, a `[` or a `;` in front, puts a blank
/// line before each section after the first line and ends every line with CRLF.
pub fn format(file: &str) {
    let Ok(text) = net::read_all_text(file) else { return };
    let mut out = String::new();
    for (i, line) in text.split('\n').enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        if !(line.contains('=') || line.starts_with('[') || line.starts_with(';')) {
            continue;
        }
        if line.starts_with('[') && i > 0 {
            out.push_str("\r\n");
        }
        out.push_str(&line.replace('\r', ""));
        out.push_str("\r\n");
    }
    let _ = net::write_all_text(file, &out);
}

/// `AppIniFile.Merge(source, target)`: a newer `HEADER/VERSION` replaces the player's file,
/// else every key the player's file lacks (or has empty) is written in.
pub fn merge(source: &str, target: &str) -> Result<(), crate::bridge::Thrown> {
    let src = IniFile::new(source);
    let tgt = IniFile::new(target);
    let version = src.read(Some("HEADER"), Some("VERSION"));
    if !version.is_empty() && version != tgt.read(Some("HEADER"), Some("VERSION")) {
        net::copy(source, target).map_err(|e| crate::bridge::Thrown::Other(e.to_string()))?;
        return Ok(());
    }
    for cat in src.categories()? {
        for key in src.keys(&cat)? {
            if tgt.read(Some(&cat), Some(&key)).is_empty() {
                tgt.write(Some(&cat), Some(&key), Some(&src.read(Some(&cat), Some(&key))))?;
            }
        }
    }
    Ok(())
}

/// `AppIniFile.GetAsJson(ininame, asarray, ignoresections, uppercase)`.
pub fn get_as_json(ininame: &str, asarray: bool, ignoresections: &[String], uppercase: bool) -> Result<String, crate::bridge::Thrown> {
    let ini = IniFile::new(ininame);
    let categories = ini.categories()?;
    if !asarray {
        let mut dictionary = Value::obj();
        for section in &categories {
            if ignoresections.contains(section) {
                continue;
            }
            // an ArgumentException (a key twice, a section twice) leaves the section out
            let Ok(keys) = ini.keys(section) else { continue };
            let mut value = Value::obj();
            let mut twice = false;
            for key in &keys {
                let name = if uppercase { net::to_upper(key) } else { net::to_lower(key) };
                let mut v = ini.read(Some(section), Some(key)).replace('\t', "");
                if matches!(key.as_str(), "MODEL" | "TRACK" | "SKIN" | "TYPE") {
                    v = net::to_lower(&v);
                }
                if !value.add(&name, Value::Str(v)) {
                    twice = true;
                    break;
                }
            }
            if twice {
                continue;
            }
            let name = if uppercase { net::to_upper(section) } else { net::to_lower(section) };
            dictionary.add(&name, value);
        }
        return Ok(jss::serialize(&dictionary));
    }
    let mut list = Vec::new();
    for section in &categories {
        if ignoresections.contains(section) {
            continue;
        }
        let keys = ini.keys(section)?;
        list.push(Value::Array(keys.iter().map(|key| Value::Str(ini.read(Some(section), Some(key)).replace('\t', ""))).collect()));
    }
    Ok(format!("{{\"aaData\":{}}}", jss::serialize(&Value::Array(list))))
}
