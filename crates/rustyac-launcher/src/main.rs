// SPDX-License-Identifier: GPL-3.0-or-later

//! rustyac-launcher: Assetto Corsa's launcher with the player's own theme. See
//! docs/port/launcher.md.

#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

const USAGE: &str = "rustyac-launcher [options]: Assetto Corsa's launcher, with the game's own theme
  --docs <folder>        the player's folder (default: the folder of this exe, rustyac.exe's)
  --ac-root <folder>     Assetto Corsa's folder (default: AC_ROOT, else Steam's)
  --game <exe>           what Drive starts (default: rustyac.exe beside this exe)
  --game-arg <arg>       one more argument for it (again for more)
  --player-name <name>   the player's name (default: [PROFILE] NAME of <docs>\\cfg\\profile.ini,
                         else [CAR_0] DRIVER_NAME of Documents\\Assetto Corsa\\cfg\\race.ini)
  --script <file>        steps that drive the page (wait / js / click / until / shot / game-exit / log)
  --log <file>           every request, event and script error
  --offscreen <w>x<h>    the window off the screen, at that size (with --script)
  --transcript <requests> <out>   no window: the bridge's answers to a list of requests";

#[cfg(windows)]
fn main() {
    use rustyac_launcher::ini::IniFile;
    use std::path::PathBuf;

    let exe_dir = std::env::current_exe().ok().and_then(|e| e.parent().map(PathBuf::from)).unwrap_or_default();
    let mut docs = exe_dir.clone();
    let mut ac_root = None;
    let mut game = exe_dir.join("rustyac.exe");
    let mut game_args = Vec::new();
    let mut player_name = None;
    let mut script = None;
    let mut log = None;
    let mut offscreen = None;
    let mut transcript = None;
    let mut args = std::env::args().skip(1);
    let fail = |m: String| -> ! {
        eprintln!("{m}\n{USAGE}");
        std::process::exit(2)
    };
    while let Some(a) = args.next() {
        let mut value = || args.next().unwrap_or_else(|| fail(format!("{a} needs a value")));
        match a.as_str() {
            "--docs" => docs = PathBuf::from(value()),
            "--ac-root" => ac_root = Some(PathBuf::from(value())),
            "--game" => game = PathBuf::from(value()),
            "--game-arg" => game_args.push(value()),
            "--player-name" => player_name = Some(value()),
            "--script" => script = Some(PathBuf::from(value())),
            "--log" => log = Some(PathBuf::from(value())),
            "--offscreen" => {
                let v = value();
                let size = v.split_once('x').and_then(|(w, h)| Some((w.parse().ok()?, h.parse().ok()?)));
                offscreen = Some(size.unwrap_or_else(|| fail(format!("--offscreen {v}: <w>x<h>"))));
            }
            "--transcript" => {
                let requests = value();
                transcript = Some((requests, value()));
            }
            "--help" | "-h" => {
                println!("{USAGE}");
                return;
            }
            _ => fail(format!("unknown option {a}")),
        }
    }
    rustyac_content::install::set_ac_root(ac_root);
    let Some(root) = rustyac_content::install::ac_root() else { fail(rustyac_content::install::not_found_hint()) };
    let absolute = |p: PathBuf| std::path::absolute(&p).unwrap_or(p).to_string_lossy().into_owned();
    let (root, docs) = (absolute(root), absolute(docs));
    if let Some((requests, out)) = transcript {
        if let Err(e) = rustyac_launcher::transcript::run(&root, &docs, &requests, &out) {
            fail(format!("transcript: {e}"));
        }
        return;
    }
    // the launcher asks Steam (GetPersonaName); rustyAC has its own setting
    let player_name = player_name
        .or_else(|| Some(IniFile::new(format!("{docs}\\cfg\\profile.ini")).read(Some("PROFILE"), Some("NAME"))).filter(|n| !n.is_empty()))
        .or_else(|| {
            let race = rustyac_content::install::documents()?.join("cfg").join("race.ini");
            Some(IniFile::new(race.to_string_lossy()).read(Some("CAR_0"), Some("DRIVER_NAME"))).filter(|n| !n.is_empty())
        })
        .unwrap_or_else(|| "You, the player".into());
    let script = script.map(|s| std::fs::read_to_string(&s).unwrap_or_else(|e| fail(format!("{}: {e}", s.display()))).lines().map(str::to_string).collect());
    let options = rustyac_launcher::shell::Options { ac_root: root, docs, game, game_args, player_name, script, log, offscreen };
    match rustyac_launcher::shell::run(options) {
        Ok(code) => std::process::exit(code),
        Err(e) => fail(format!("rustyac-launcher: {e}")),
    }
}

#[cfg(not(windows))]
fn main() {
    eprintln!("{USAGE}\nrustyac-launcher runs on Windows only (the launcher's browser is WebView2).");
}
