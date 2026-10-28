// SPDX-License-Identifier: GPL-3.0-or-later

//! Nothing rustyAC writes lands in the game's folder: path options are absolute from the start,
//! so the Python apps' change of working directory (into the game's folder) does not move them,
//! and every write goes through `rustyac_content::guard`, which refuses the game's folder and
//! `Documents\Assetto Corsa`. The game's folder here is a stand-in under the target folder.

use std::path::{Path, PathBuf};

use rustyac_content::guard;
use rustyac_game::cli::Options;

const PATH_OPTIONS: [&str; 8] = ["--out-dir", "--cfg-dir", "--documents", "--replay-dir", "--setups-dir", "--screenshot", "--telemetry-dir", "--save-replay"];

fn paths(o: &Options) -> Vec<PathBuf> {
    [&o.out_dir, &o.cfg_dir, &o.documents, &o.replay_dir, &o.setups_dir, &o.screenshot, &o.telemetry_dir, &o.save_replay].into_iter().map(|p| p.clone().unwrap()).collect()
}

#[test]
fn nothing_is_written_into_the_game_folder() {
    let scratch = Path::new(env!("CARGO_TARGET_TMPDIR")).join("write_guard");
    let fake_ac = scratch.join("assettocorsa");
    let own = scratch.join("own");
    for folder in [fake_ac.join("content"), fake_ac.join("system"), fake_ac.join("apps/python/x/__pycache__"), own.clone()] {
        std::fs::create_dir_all(folder).unwrap();
    }
    std::fs::write(fake_ac.join("apps/python/x/__pycache__/x.pyc"), b"kept").unwrap();
    rustyac_content::install::set_ac_root(Some(fake_ac.clone()));

    // relative options, given while the working directory is rustyAC's own
    std::env::set_current_dir(&own).unwrap();
    let relative = Options::parse(PATH_OPTIONS.iter().flat_map(|o| [o.to_string(), format!("rel{o}")])).unwrap();
    for (option, path) in PATH_OPTIONS.iter().zip(paths(&relative)) {
        assert!(path.is_absolute(), "{option} {}", path.display());
        assert!(path.starts_with(own.canonicalize().unwrap()) || path.starts_with(&own), "{option} {}", path.display());
    }
    // "-" stays stdout
    assert_eq!(Options::parse(["--replay", "x.ryin", "--headless", "--dump-states", "-"].map(String::from).into_iter()).unwrap().dump_states.unwrap().as_os_str(), "-");
    // absolute ones stay as given
    let absolute = Options::parse(PATH_OPTIONS.iter().flat_map(|o| [o.to_string(), own.join(&o[2..]).display().to_string()])).unwrap();
    for (option, path) in PATH_OPTIONS.iter().zip(paths(&absolute)) {
        assert_eq!(path, own.join(&option[2..]));
    }

    // the Python apps' start: the working directory becomes the game's folder
    std::env::set_current_dir(&fake_ac).unwrap();
    for refused in ["out/race_out.json", "cfg/race.ini", "logs/py_log.txt", "replay/x.acreplay", "setups/x.ini", "screenshots/x.png", "./system/../x.txt"] {
        assert!(guard::write(refused, b"no").is_err(), "relative {refused}");
        assert!(guard::write(fake_ac.join(refused), b"no").is_err(), "absolute {refused}");
    }
    let upper = PathBuf::from(fake_ac.display().to_string().to_uppercase()).join("x.txt");
    assert!(guard::create(&upper).is_err(), "{}", upper.display());
    assert_eq!(guard::write("x.txt", b"no").unwrap_err().kind(), std::io::ErrorKind::PermissionDenied);
    // the game's cache clearing is refused too: the file stays
    assert!(guard::remove_file("apps/python/x/__pycache__/x.pyc").is_err());
    assert!(fake_ac.join("apps/python/x/__pycache__/x.pyc").is_file());
    // the user's real Documents\Assetto Corsa (only checked: nothing is written)
    if let Some(profile) = std::env::var_os("USERPROFILE") {
        assert!(guard::check(&Path::new(&profile).join("Documents/Assetto Corsa/cfg/race.ini")).is_err());
    }

    // the options parsed before still point to rustyAC's own folder, which is written
    for path in paths(&relative) {
        guard::create_dir_all(path.parent().unwrap()).unwrap();
        guard::write(&path, b"ok").unwrap();
        assert!(path.starts_with(&own) || path.starts_with(own.canonicalize().unwrap()));
    }
    for entry in walk(&fake_ac) {
        assert!(entry.ends_with("x.pyc"), "written into the game's folder: {}", entry.display());
    }
    std::env::set_current_dir(env!("CARGO_MANIFEST_DIR")).unwrap();
    rustyac_content::install::set_ac_root(None);
}

fn walk(folder: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for entry in std::fs::read_dir(folder).unwrap().flatten() {
        let path = entry.path();
        if path.is_dir() { files.extend(walk(&path)) } else { files.push(path) }
    }
    files
}
