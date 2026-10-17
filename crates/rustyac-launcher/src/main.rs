// SPDX-License-Identifier: GPL-3.0-or-later

//! rustyac-launcher: Assetto Corsa's launcher with the player's own theme. See
//! docs/port/launcher.md.

#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

const USAGE: &str = "rustyac-launcher [options]: Assetto Corsa's launcher, with the game's own theme
  --docs <folder>        the player's folder (default: the folder of this exe, rustyac.exe's)
  --ac-root <folder>     Assetto Corsa's folder (default: AC_ROOT, else Steam's)
  --game <exe>           what Drive starts (default: rustyac.exe beside this exe)
  --game-arg <arg>       one more argument for it (again for more)
  --player-cfg <folder|none>  where the launcher's own cfg files that are missing are first
                         copied from, only read (default: Documents\\Assetto Corsa\\cfg)
  --player-name <name>   the player's name (default: [PROFILE] NAME of <docs>\\cfg\\profile.ini,
                         else [CAR_0] DRIVER_NAME of Documents\\Assetto Corsa\\cfg\\race.ini)
  --script <file>        steps that drive the page (wait / js / click / until / shot / game-exit / log)
  --log <file>           every request, event and script error
  --offscreen <w>x<h>    the window off the screen, at that size (with --script)
  --lobby <host:port>    the server list from there instead of Kunos' lobby (a recording)
  --import-pbs [<folder>] copy the personal bests of an Assetto Corsa Documents folder (default:
                         Documents\\Assetto Corsa) into <docs>\\personalbest.ini, where they beat
                         ours, then exit; that folder is only read
  --transcript <requests> <out>   no window: the bridge's answers to a list of requests
  --pure-ini             ini files through rustyAC's pure copy of the Win32 calls, as in the browser
  --fixed-host           with --transcript: the same screen, times and player everywhere, as the
                         transcript example does in wasm";

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
    let mut player_cfg = rustyac_content::install::documents().map(|d| d.join("cfg"));
    let mut script = None;
    let mut log = None;
    let mut offscreen = None;
    let mut transcript = None;
    let mut fixed_host = false;
    let mut lobby = None;
    let mut import_pbs = None;
    let mut args = std::env::args().skip(1).peekable();
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
            "--pure-ini" => rustyac_launcher::ini::use_pure(true),
            "--fixed-host" => fixed_host = true,
            "--player-cfg" => player_cfg = Some(value()).filter(|v| v != "none").map(PathBuf::from),
            "--script" => script = Some(PathBuf::from(value())),
            "--log" => log = Some(PathBuf::from(value())),
            "--offscreen" => {
                let v = value();
                let size = v.split_once('x').and_then(|(w, h)| Some((w.parse().ok()?, h.parse().ok()?)));
                offscreen = Some(size.unwrap_or_else(|| fail(format!("--offscreen {v}: <w>x<h>"))));
            }
            "--lobby" => lobby = Some(value()),
            "--import-pbs" => import_pbs = Some(args.next_if(|v| !v.starts_with("--")).map(PathBuf::from)),
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
    if let Some(from) = import_pbs {
        let Some(from) = from.or_else(rustyac_content::install::documents) else { fail("--import-pbs: no Documents\\Assetto Corsa folder found: name it".into()) };
        let (from, to) = (from.join("personalbest.ini"), docs.join("personalbest.ini"));
        if !from.is_file() {
            fail(format!("--import-pbs: no {}", from.display()));
        }
        match rustyac_launcher::pbs::import(&from.to_string_lossy(), &std::path::absolute(&to).unwrap_or(to.clone()).to_string_lossy()) {
            Ok((copied, kept)) => println!("personal bests from {}: {copied} copied into {}, {kept} of ours as good or better kept", from.display(), to.display()),
            Err(e) => fail(format!("--import-pbs: {e:?}")),
        }
        return;
    }
    rustyac_content::install::set_ac_root(ac_root);
    let Some(root) = rustyac_content::install::ac_root() else { fail(rustyac_content::install::not_found_hint()) };
    let absolute = |p: PathBuf| std::path::absolute(&p).unwrap_or(p).to_string_lossy().into_owned();
    let (root, docs) = (absolute(root), absolute(docs));
    if let Some((requests, out)) = transcript {
        // no Steam: GetPersonaName's own answer
        let host: Box<dyn rustyac_launcher::bridge::Host> =
            if fixed_host { Box::new(rustyac_launcher::transcript::Fixed) } else { Box::new(rustyac_launcher::sys::Windows { player_name: "You, the player".into() }) };
        if let Err(e) = rustyac_launcher::transcript::run(&root, &docs, &requests, &out, lobby, host) {
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
    let options = rustyac_launcher::shell::Options { ac_root: root, docs, game, game_args, player_name, player_cfg, script, log, offscreen, lobby };
    match rustyac_launcher::shell::run(options) {
        Ok(code) => std::process::exit(code),
        Err(e) => fail(format!("rustyac-launcher: {e}")),
    }
}

#[cfg(not(windows))]
fn main() {
    eprintln!("{USAGE}\nrustyac-launcher runs on Windows only (the launcher's browser is WebView2).");
}
