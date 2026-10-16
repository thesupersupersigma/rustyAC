// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa's launcher (AssettoCorsa.exe, Kunos Simulazioni).

//! `Startup.SetupUserDirectory`: what the launcher does with the player's folder when it starts.
//! The game's cfg files are merged into the player's, the launcher's browser data is backed up
//! and the templates are copied.

use crate::appini;
use crate::folders::{create_if_non_existant, mirror_folder, AppFolder};
use crate::ini::IniFile;
use crate::net;

/// `Assembly.GetEntryAssembly().GetName().Version`: AssettoCorsa.exe's, the launcher this is a
/// port of (1.16.4).
pub const LAUNCHER_VERSION: &str = "0.18.115.5097";

/// What the player answers to a question the launcher asks with a message box.
pub trait Ask {
    /// "Version Mismatch": true for OK (reset the folder), false for Cancel (exit).
    fn version_mismatch(&self, text: &str) -> bool;
    /// "Error setting up user folders".
    fn setup_error(&self, text: &str);
}

/// Why the launcher stops instead of showing the theme (`KillProcesses(self: true)`).
#[derive(Debug)]
pub struct Exit;

const MISMATCH: &str = "READ THIS BEFORE PROCEEDING:\n\nWARNING! The Assetto Corsa folder in your documents folder belongs to a different version of Assetto Corsa.\n\nIncompatibilities with the version you are running might exist.\n\nPress \"OK\" to delete the contents of the folder (includes car setups, replays, local databases with PB times, all user settings) or \"Cancel\" if you would like to exit and delete the folder manually or make a backup before proceeding.";

/// `Directory.GetFiles(folder, "*.ini", AllDirectories)`: a folder's files, then its folders'.
fn ini_files(folder: &str, out: &mut Vec<String>) {
    out.extend(net::files_with(folder, ".ini").unwrap_or_default());
    for dir in net::list(folder, true).unwrap_or_default() {
        ini_files(&dir, out);
    }
}

/// `DeleteUserData`. The launcher deletes the whole player's folder; rustyAC's is the folder of
/// rustyac.exe, so only what the launcher keeps there goes: its cfg files, browser data and
/// templates.
fn delete_user_data(f: &AppFolder) {
    for dir in [&f.user_config_dir, &f.user_launcher_data_dir, &f.user_template_dir] {
        let _ = std::fs::remove_dir_all(dir.trim_end_matches('\\'));
    }
}

pub fn setup_user_directory(f: &AppFolder, ask: &dyn Ask) -> Result<(), Exit> {
    let mut flag = false;
    if net::file_exists(&format!("{}launcher.ini", f.user_config_dir)) {
        let default = IniFile::new(format!("{}launcher.ini", f.default_config_dir));
        let user = IniFile::new(format!("{}launcher.ini", f.user_config_dir));
        let text = default.read(Some("WINDOW"), Some("VERSION"));
        let text2 = user.read(Some("WINDOW"), Some("VERSION"));
        if user.read(Some("WINDOW"), Some("LASTVER")) != LAUNCHER_VERSION {
            let _ = user.write(Some("WINDOW"), Some("THEME"), Some("default"));
            let _ = user.write(Some("WINDOW"), Some("LASTVER"), Some(LAUNCHER_VERSION));
        }
        if text != text2 {
            if !ask.version_mismatch(MISMATCH) {
                return Err(Exit);
            }
            delete_user_data(f);
        }
    }
    let dirs = [
        &f.user_data_path,
        &f.user_themes_dir,
        &f.user_plugins_dir,
        &f.user_replay_dir,
        &f.user_license_dir,
        &f.user_launcher_data_dir,
        &f.user_launcher_data_file_store_dir,
        &f.user_launcher_data_backup_dir,
        &f.user_template_dir,
        &f.user_logs_dir,
        &f.user_out_dir,
    ];
    if let Err(e) = dirs.iter().try_for_each(|d| create_if_non_existant(d)) {
        ask.setup_error(&format!("{}\n{e}", f.user_data_path));
        flag = true;
    }
    // AppPath + "\\cfg": the launcher's paths have the backslash twice here
    let _ = create_if_non_existant(&format!("{}\\cfg", f.app_path));
    let _ = mirror_folder(&format!("{}\\cfg", f.app_path), &format!("{}\\cfg", f.user_data_path), false, false);
    let mut files = Vec::new();
    ini_files(&f.default_config_dir, &mut files);
    for text5 in files {
        let text6 = text5.replace(&f.default_config_dir, &f.user_config_dir);
        let _ = appini::merge(&text5, &text6);
        appini::format(&text6);
    }
    backup(f);
    if flag {
        return Err(Exit);
    }
    let _ = mirror_folder(&format!("{}cfg\\templates", f.app_path), &f.user_template_dir, false, true);
    let tracks = format!("{}cfg\\templates\\tracks.ini", f.app_path);
    let user_tracks = format!("{}tracks.ini", f.user_template_dir);
    if net::file_exists(&tracks) && net::file_exists(&user_tracks) {
        let _ = std::fs::copy(&tracks, &user_tracks);
    }
    let _ = create_if_non_existant(&format!("{}ppfilters", f.user_config_dir));
    let index = format!("{}index", f.user_launcher_data_dir);
    if net::file_exists(&index) {
        let _ = std::fs::remove_file(&index);
    }
    Ok(())
}

/// The newest ten backups of the launcher's browser data are kept, and a new one is made.
fn backup(f: &AppFolder) {
    let Ok(dirs) = net::list(&f.user_launcher_data_backup_dir, true) else { return };
    let mut dirs: Vec<(String, std::time::SystemTime)> =
        dirs.into_iter().map(|d| (d.clone(), std::fs::metadata(&d).and_then(|m| m.created()).unwrap_or(std::time::UNIX_EPOCH))).collect();
    dirs.sort_by_key(|d| std::cmp::Reverse(d.1));
    for (dir, _) in dirs.iter().skip(10) {
        let _ = std::fs::remove_dir_all(dir);
    }
    let stamp = crate::sys::now_stamp();
    for name in ["databases", "IndexedDB", "Local Storage"] {
        let folder = format!("{}{stamp}", f.user_launcher_data_backup_dir);
        let _ = create_if_non_existant(&folder);
        let _ = copy_directory(&format!("{}{name}", f.user_launcher_data_dir), &format!("{folder}\\{name}"));
    }
}

/// `FileSystem.CopyDirectory(source, dest)`: everything, files already there kept.
fn copy_directory(source: &str, dest: &str) -> std::io::Result<()> {
    if !net::dir_exists(source) {
        return Err(std::io::ErrorKind::NotFound.into());
    }
    create_if_non_existant(dest)?;
    for file in net::list(source, false)? {
        let target = format!("{dest}\\{}", net::file_name(&file));
        if !net::file_exists(&target) {
            std::fs::copy(&file, &target)?;
        }
    }
    for dir in net::list(source, true)? {
        copy_directory(&dir, &format!("{dest}\\{}", net::file_name(&dir)))?;
    }
    Ok(())
}
