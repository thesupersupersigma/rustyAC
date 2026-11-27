// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa's launcher (AssettoCorsa.exe, Kunos Simulazioni).

//! The window's pieces that do not depend on its browser, for WebView2 ([`shell`](crate::shell))
//! and WebKitGTK (`shell_gtk.rs`, the module `shell` on Linux) alike: the theme, its events, the script steps
//! of the checks, and the game's command line when Drive is clicked.

use std::path::Path;
use std::process::Command;

use crate::folders::AppFolder;
use crate::ini::IniFile;
use crate::{appini, net};

/// rustyAC's controls page (`showcontrols`), not the launcher's.
pub const CONTROLS_PAGE: &str = include_str!("controls.html");

/// The launcher's theme (`Theme`): the default one, or launcher.ini's `[WINDOW] THEME`.
pub struct Theme {
    pub id: String,
    /// `index.html`.
    pub path: String,
    pub load_default_libs: bool,
    pub scripts: Vec<String>,
}

impl Theme {
    pub fn url(&self) -> String {
        net::uri_path(&self.path)
    }

    pub fn folder(&self) -> &str {
        &self.path[..self.path.rfind('\\').unwrap_or(0)]
    }
}

/// `GetAvailableThemes`, then the one launcher.ini names. A theme folder without a theme.ini
/// gets one written by the launcher; here such a theme is skipped, as the game's folder is
/// never written.
pub fn current_theme(f: &AppFolder, launcher_ini: &IniFile) -> Theme {
    let mut themes = Vec::new();
    let default_ini = format!("{}default\\theme.ini", f.default_themes_dir);
    let read_theme = |id: &str, folder: &str, ini_path: &str| {
        let ini = IniFile::new(ini_path);
        Theme {
            id: id.into(),
            path: format!("{folder}\\index.html"),
            load_default_libs: ini.read(Some("theme"), Some("loaddefault")) == "true",
            scripts: ini.read(Some("THEME"), Some("scripts")).split(',').map(str::to_string).collect(),
        }
    };
    if net::file_exists(&default_ini) {
        themes.push(read_theme("default", &format!("{}default", f.default_themes_dir), &default_ini));
    }
    for dir in net::list(&f.default_themes_dir, true).unwrap_or_default() {
        let ini = format!("{dir}\\theme.ini");
        if dir != format!("{}default", f.default_themes_dir) && net::file_exists(&ini) && net::file_exists(&format!("{dir}\\index.html")) {
            themes.push(read_theme(&dir.replace(&f.default_themes_dir, ""), &dir, &ini));
        }
    }
    let selected = launcher_ini.read(Some("WINDOW"), Some("Theme"));
    let i = themes.iter().position(|t| t.id == selected).unwrap_or(0);
    themes.swap_remove(i)
}

/// `GetJSEvent`.
pub fn js_event(name: &str, args: &[&str]) -> String {
    let mut text = args.join(",");
    if !args.is_empty() {
        text = format!("\"{}\"", text.replace(',', "\",\""));
    }
    format!("if(typeof $ != 'undefined') $(document).trigger(\"{name}\", [{text}]);")
}

/// One step of a script.
#[derive(Debug)]
pub enum Step {
    /// `wait <ms>`.
    Wait(u64),
    /// `js <code>`: run it in the page.
    Js(String),
    /// `click <selector>`: jQuery's click on what the selector finds.
    Click(String),
    /// `until <ms> <expression>`: wait until the expression is true (the check fails after ms).
    Until(u64, String),
    /// `shot <file.png>`: the page as it is now.
    Shot(String),
    /// `game-exit <ms>`: wait until the game that "drive" started has ended.
    GameExit(u64),
    /// `log <text>`.
    Log(String),
}

pub fn parse_step(line: &str) -> Option<Step> {
    let (verb, rest) = line.split_once(' ').unwrap_or((line, ""));
    let ms = |s: &str| s.parse::<u64>().ok();
    Some(match verb {
        "wait" => Step::Wait(ms(rest)?),
        "js" => Step::Js(rest.into()),
        "click" => Step::Click(rest.into()),
        "until" => {
            let (t, expr) = rest.split_once(' ')?;
            Step::Until(ms(t)?, expr.into())
        }
        "shot" => Step::Shot(rest.into()),
        "game-exit" => Step::GameExit(ms(rest)?),
        "log" => Step::Log(rest.into()),
        _ => return None,
    })
}

/// The original asks the language the first time on its own WPF screen (English, Italiano) and
/// writes launcher.ini [WINDOW] LANGUAGE. The theme's own first-run wizard asks the same two and
/// writes the same key, but the default theme never loads it: loaded with the theme's modules, it
/// shows itself only while that key is empty.
// jQuery's trigger("loadModules") also calls document.onloadModules, which needs no jQuery yet
pub const WIZARD_JS: &str = "document.onloadModules=function(){__AC.loadModule('FirstRunWizard','body > .modules','en',null,true)};";

/// A launcher path (its folders are spelled with `\`) as the system spells it.
pub fn os_path(path: &str) -> String {
    if cfg!(windows) {
        path.to_string()
    } else {
        path.replace('\\', "/")
    }
}

/// rustyac's command line for the race the theme set up (`acs.exe` reads it all from race.ini).
pub fn game_command(f: &AppFolder, game: &Path, game_args: &[String]) -> Command {
    let mut cmd = Command::new(game);
    cmd.arg("--documents").arg(os_path(f.user_data_path.trim_end_matches('\\'))).arg("--out-dir").arg(os_path(f.user_out_dir.trim_end_matches('\\')));
    // acs.exe takes car and track from race.ini; rustyac.exe from its command line
    let race = IniFile::new(appini::get(f, "race"));
    // an online join: rustyac.exe takes it from [REMOTE] when it is given no car or track
    let remote = race.read(Some("REMOTE"), Some("ACTIVE")) == "1";
    // a replay of the replays page: the file names its own track and cars
    let replay = race.read(Some("REPLAY"), Some("ACTIVE")) == "1";
    if replay {
        cmd.arg("--replay").arg(os_path(&format!("{}{}", f.user_replay_dir, race.read(Some("REPLAY"), Some("FILENAME")))));
    }
    for (flag, key) in [("--car", "MODEL"), ("--track", "TRACK"), ("--layout", "CONFIG_TRACK")].into_iter().filter(|_| !remote && !replay) {
        let value = race.read(Some("RACE"), Some(key));
        if !value.is_empty() {
            cmd.arg(flag).arg(value);
        }
    }
    cmd.args(game_args);
    // the extra cars and tracks laid over the install (Task W5): the game mounts the same
    if let Some(file) = crate::files::game_files() {
        cmd.env("RUSTYAC_GAME_FILES", file);
    }
    if let Some(dir) = game.parent() {
        cmd.current_dir(dir);
    }
    cmd
}
