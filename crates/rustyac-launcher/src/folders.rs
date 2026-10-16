// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa's launcher (AssettoCorsa.exe, Kunos Simulazioni).

//! `AC.Launcher.Utility.AppFolder`: the game's folders (`AppPath`, read only here) and the
//! player's (`UserDataPath`, which the launcher has as `Documents\Assetto Corsa` and rustyAC's
//! launcher has as the folder of rustyac.exe, so a drive reads what the launcher wrote).
//! Every path ends with `\`, as the launcher builds them.

#[derive(Clone, Debug)]
pub struct AppFolder {
    pub app_path: String,
    pub launcher_support_dir: String,
    pub user_data_path: String,
    pub default_config_dir: String,
    pub user_config_dir: String,
    pub default_content_dir: String,
    pub user_content_dir: String,
    pub user_plugins_dir: String,
    pub user_launcher_data_dir: String,
    pub user_launcher_data_file_store_dir: String,
    pub user_launcher_data_backup_dir: String,
    pub user_out_dir: String,
    pub user_setup_dir: String,
    pub default_themes_dir: String,
    pub user_themes_dir: String,
    pub default_apps_dir: String,
    pub user_replay_dir: String,
    pub user_license_dir: String,
    pub user_logs_dir: String,
    pub user_template_dir: String,
}

fn slash(p: &str) -> String {
    let p = p.replace('/', "\\");
    if p.ends_with('\\') {
        p
    } else {
        format!("{p}\\")
    }
}

impl AppFolder {
    /// `app_path`: the game's folder; `user_data_path`: the player's.
    pub fn new(app_path: &str, user_data_path: &str) -> AppFolder {
        let app = slash(app_path);
        let user = slash(user_data_path);
        let launcher_data = format!("{user}launcherdata\\");
        AppFolder {
            launcher_support_dir: format!("{app}launcher\\support\\"),
            default_config_dir: format!("{app}cfg\\"),
            user_config_dir: format!("{user}cfg\\"),
            default_content_dir: format!("{app}content\\"),
            user_content_dir: format!("{user}content\\"),
            user_plugins_dir: format!("{user}plugins\\"),
            user_launcher_data_file_store_dir: format!("{launcher_data}filestore\\"),
            user_launcher_data_backup_dir: format!("{launcher_data}backups\\"),
            user_launcher_data_dir: launcher_data,
            user_out_dir: format!("{user}out\\"),
            user_setup_dir: format!("{user}setups\\"),
            default_themes_dir: format!("{app}launcher\\themes\\"),
            user_themes_dir: format!("{user}themes\\"),
            default_apps_dir: format!("{app}apps\\python\\"),
            user_replay_dir: format!("{user}replay\\"),
            user_license_dir: format!("{user}lic\\"),
            user_logs_dir: format!("{user}logs\\"),
            user_template_dir: format!("{user}templates\\"),
            app_path: app,
            user_data_path: user,
        }
    }
}

/// `AppFolder.CreateIfNonExistant`.
pub fn create_if_non_existant(path: &str) -> std::io::Result<()> {
    if !crate::net::dir_exists(path) {
        std::fs::create_dir_all(path)?;
    }
    Ok(())
}

/// `Path.Combine(folder, name)`.
fn combine(folder: &str, name: &str) -> String {
    if folder.ends_with('\\') || folder.ends_with('/') {
        format!("{folder}{name}")
    } else {
        format!("{folder}\\{name}")
    }
}

/// `AppFolder.MirrorFolder(source, dest, delete, overwrite)`. Its folders below the first get
/// `overwrite` true again, as in the launcher.
pub fn mirror_folder(source: &str, dest: &str, delete: bool, overwrite: bool) -> std::io::Result<()> {
    create_if_non_existant(dest)?;
    for file in crate::net::list(source, false)? {
        let target = combine(dest, crate::net::file_name(&file));
        if !crate::net::file_exists(&target) || overwrite {
            std::fs::copy(&file, &target)?;
        }
    }
    if delete {
        for file in crate::net::list(dest, false)? {
            if !crate::net::file_exists(&combine(source, crate::net::file_name(&file))) {
                std::fs::remove_file(&file)?;
            }
        }
    }
    for dir in crate::net::list(source, true)? {
        mirror_folder(&dir, &combine(dest, crate::net::file_name(&dir)), delete, true)?;
    }
    Ok(())
}
