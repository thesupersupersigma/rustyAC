// SPDX-License-Identifier: MIT OR Apache-2.0

//! Where Assetto Corsa is installed. Everything here only looks; nothing is written.
//!
//! [`ac_root`] is the one place that decides:
//! 1. the environment variable `AC_ROOT`, when it is set (and nothing else then: a wrong
//!    path there is not silently replaced by another install);
//! 2. Steam's usual place, `C:\Program Files (x86)\Steam\steamapps\common\assettocorsa`;
//! 3. every Steam library: Steam's own folder (from the registry, else the usual places) and
//!    the folders its `steamapps\libraryfolders.vdf` lists (a game installed on another
//!    drive is in one of those), each with `steamapps\common\assettocorsa`.

use std::path::{Path, PathBuf};

/// The game's folder name inside a Steam library.
const GAME_FOLDER: &str = r"steamapps\common\assettocorsa";
const USUAL_PLACE: &str = r"C:\Program Files (x86)\Steam\steamapps\common\assettocorsa";

/// Is this Assetto Corsa's folder? (It has `content` and `system`.)
pub fn is_ac_root(folder: &Path) -> bool {
    folder.join("content").is_dir() && folder.join("system").is_dir()
}

/// Assetto Corsa's own folder, or `None` when it is not found. Read only.
pub fn ac_root() -> Option<PathBuf> {
    if let Some(root) = std::env::var_os("AC_ROOT") {
        let root = PathBuf::from(root);
        return is_ac_root(&root).then_some(root);
    }
    let usual = PathBuf::from(USUAL_PLACE);
    if is_ac_root(&usual) {
        return Some(usual);
    }
    for steam in steam_folders() {
        for library in std::iter::once(steam.clone()).chain(libraries_of(&steam)) {
            let root = library.join(GAME_FOLDER);
            if is_ac_root(&root) {
                return Some(root);
            }
        }
    }
    None
}

/// What to tell the user when [`ac_root`] found nothing.
pub fn not_found_hint() -> String {
    match std::env::var_os("AC_ROOT") {
        Some(root) => format!("AC_ROOT is set to {}, which is not Assetto Corsa's folder (no content and system folders in it)", PathBuf::from(root).display()),
        None => "Assetto Corsa was not found in Steam's usual place or in any Steam library; set the environment variable AC_ROOT to its folder (the one with content and system in it)".to_string(),
    }
}

/// Steam's own folder: what the registry says, then the usual places.
fn steam_folders() -> Vec<PathBuf> {
    let mut folders: Vec<PathBuf> = Vec::new();
    let mut add = |folder: PathBuf| {
        if folder.join("steamapps").is_dir() && !folders.contains(&folder) {
            folders.push(folder);
        }
    };
    for folder in registry::steam_paths() {
        add(folder);
    }
    for variable in ["ProgramFiles(x86)", "ProgramFiles"] {
        if let Some(base) = std::env::var_os(variable) {
            add(PathBuf::from(base).join("Steam"));
        }
    }
    folders
}

/// The library folders a Steam install lists in `steamapps\libraryfolders.vdf` (or the older
/// `config\libraryfolders.vdf`).
fn libraries_of(steam: &Path) -> Vec<PathBuf> {
    [steam.join("steamapps").join("libraryfolders.vdf"), steam.join("config").join("libraryfolders.vdf")]
        .iter()
        .filter_map(|file| std::fs::read(file).ok())
        .flat_map(|bytes| library_paths(&String::from_utf8_lossy(&bytes)))
        .collect()
}

/// The folders named in the text of a `libraryfolders.vdf`. Two forms exist:
/// `"path"  "D:\\Games\\Steam"` inside a numbered block (today's), and
/// `"1"  "D:\\Games\\Steam"` directly (before 2021).
pub fn library_paths(vdf: &str) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    for line in vdf.lines() {
        let strings = quoted(line);
        let [key, value] = strings.as_slice() else { continue };
        let numbered = !key.is_empty() && key.bytes().all(|b| b.is_ascii_digit());
        if (key.eq_ignore_ascii_case("path") || numbered) && value.contains(['\\', '/']) {
            paths.push(PathBuf::from(value));
        }
    }
    paths
}

/// The quoted strings of one line, with `\\` and `\"` unescaped.
fn quoted(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        if c != '"' {
            continue;
        }
        let mut text = String::new();
        while let Some(c) = chars.next() {
            match c {
                '\\' => text.extend(chars.next()),
                '"' => break,
                c => text.push(c),
            }
        }
        out.push(text);
    }
    out
}

#[cfg(windows)]
mod registry {
    use std::ffi::c_void;
    use std::path::PathBuf;

    const HKEY_CURRENT_USER: isize = 0x8000_0001u32 as i32 as isize;
    const HKEY_LOCAL_MACHINE: isize = 0x8000_0002u32 as i32 as isize;
    /// A string, and nothing else, is accepted.
    const RRF_RT_REG_SZ: u32 = 0x0000_0002;

    #[link(name = "advapi32")]
    extern "system" {
        fn RegGetValueW(key: isize, sub_key: *const u16, value: *const u16, flags: u32, kind: *mut u32, data: *mut c_void, size: *mut u32) -> i32;
    }

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain([0]).collect()
    }

    fn string_value(root: isize, key: &str, value: &str) -> Option<String> {
        let (key, value) = (wide(key), wide(value));
        let mut buffer = [0u16; 1024];
        let mut size = std::mem::size_of_val(&buffer) as u32;
        // SAFETY: a read of one registry string into a buffer whose size in bytes is passed;
        // the two names are NUL-terminated and live through the call.
        let status = unsafe { RegGetValueW(root, key.as_ptr(), value.as_ptr(), RRF_RT_REG_SZ, std::ptr::null_mut(), buffer.as_mut_ptr().cast(), &mut size) };
        if status != 0 {
            return None;
        }
        let len = buffer.iter().position(|&c| c == 0).unwrap_or(buffer.len());
        Some(String::from_utf16_lossy(&buffer[..len]))
    }

    /// Where Steam says it is installed (read only).
    pub fn steam_paths() -> Vec<PathBuf> {
        [
            (HKEY_CURRENT_USER, r"Software\Valve\Steam", "SteamPath"),
            (HKEY_LOCAL_MACHINE, r"SOFTWARE\WOW6432Node\Valve\Steam", "InstallPath"),
            (HKEY_LOCAL_MACHINE, r"SOFTWARE\Valve\Steam", "InstallPath"),
        ]
        .into_iter()
        .filter_map(|(root, key, value)| string_value(root, key, value))
        .map(|path| PathBuf::from(path.replace('/', "\\")))
        .collect()
    }
}

#[cfg(not(windows))]
mod registry {
    pub fn steam_paths() -> Vec<std::path::PathBuf> {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn library_folders_of_both_file_forms() {
        let new = "\"libraryfolders\"\n{\n\t\"0\"\n\t{\n\t\t\"path\"\t\t\"C:\\\\Program Files (x86)\\\\Steam\"\n\t\t\"label\"\t\t\"\"\n\t\t\"apps\"\n\t\t{\n\t\t\t\"244210\"\t\t\"30415652446\"\n\t\t}\n\t}\n\t\"1\"\n\t{\n\t\t\"path\"\t\t\"D:\\\\Steam Library\"\n\t}\n}\n";
        assert_eq!(library_paths(new), [PathBuf::from(r"C:\Program Files (x86)\Steam"), PathBuf::from(r"D:\Steam Library")]);
        let old = "\"LibraryFolders\"\n{\n\t\"TimeNextStatsReport\"\t\t\"1600000000\"\n\t\"ContentStatsID\"\t\t\"-123\"\n\t\"1\"\t\t\"E:\\\\Games\"\n}\n";
        assert_eq!(library_paths(old), [PathBuf::from(r"E:\Games")]);
    }

    #[test]
    fn the_steam_registry_can_be_asked() {
        // whatever it says (nothing on a machine without Steam), asking must not fail
        let _ = registry::steam_paths();
        let _ = not_found_hint();
    }
}
