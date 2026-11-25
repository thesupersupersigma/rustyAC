// SPDX-License-Identifier: GPL-3.0-or-later

//! Task C6a-2a: rustyAC's INIpp flatten of each ini in a list (one path a line), in the form
//! `csp_oracle --inipp` prints CSP's: `== <path>` then the text. C6a-2b: with feature `lua` and a
//! CSP dwrite.dll as the second argument (its `ini_std`), Lua on. C6a-2c: `inipp_flat --configs <ac>
//! <dwrite.dll>`: every installed car (each skin with its own ext_config.ini, and the first) and
//! track (each layout) through CSP's loader order, in the form tools/proofs/csp_configs.py compares.

fn main() {
    #[cfg(feature = "lua")]
    if std::env::args().nth(1).as_deref() == Some("--configs") {
        return configs();
    }
    let list = std::env::args().nth(1).expect("usage: inipp_flat <list>");
    let text = std::fs::read_to_string(&list).unwrap_or_else(|e| panic!("{list}: {e}"));
    #[cfg(feature = "lua")]
    let std = std::env::args().nth(2).map(|dll| {
        rustyac_physics::csp::inipp_lua::ini_std(std::path::Path::new(&dll)).unwrap_or_else(|| panic!("{dll}: no ini_std"))
    });
    for path in text.lines().map(str::trim).filter(|l| !l.is_empty()) {
        println!("== {path}");
        let path = std::path::Path::new(path);
        #[cfg(feature = "lua")]
        let flat = match (&std, std::env::var("CSP_ORACLE_INIPP_DIRS")) {
            // csp_oracle's include folders (C6a-2c probes)
            (Some(std), Ok(dirs)) => {
                let dirs = dirs.split(';').filter(|d| !d.is_empty()).map(std::path::PathBuf::from).collect();
                let o = rustyac_physics::csp::inipp::Load { std, dirs, after: Vec::new(), base: 0, read: Box::new(|_: &[u8]| None) };
                rustyac_physics::csp::inipp::load(path, o).map(|s| rustyac_physics::csp::inipp::text(&s))
            }
            (Some(std), _) => rustyac_physics::csp::inipp::flatten_lua(path, std),
            (None, _) => rustyac_physics::csp::inipp::flatten(path),
        };
        #[cfg(not(feature = "lua"))]
        let flat = rustyac_physics::csp::inipp::flatten(path);
        println!("{}", flat.as_deref().unwrap_or("(not loaded)"));
    }
}

/// `== <kind> <id> <skin or layout> <cached 0/1> <CSP's cache entry name> <file>`, then a line per key:
/// section, key and the values joined by 0x1e, 0x1f between (newlines as `\n`).
#[cfg(feature = "lua")]
fn configs() {
    use rustyac_physics::csp::config;
    let arg = |n: usize| std::path::PathBuf::from(std::env::args().nth(n).expect("usage: inipp_flat --configs <ac> <dwrite.dll>"));
    let (ac, dll) = (arg(2), arg(3));
    let std = rustyac_physics::csp::inipp_lua::ini_std(&dll).unwrap_or_else(|| panic!("{}: no ini_std", dll.display()));
    let f = config::Folders { ac: ac.clone(), user: rustyac_content::install::user_documents().map(|d| d.join("cfg").join("extension")) };
    let dirs = |p: std::path::PathBuf| -> Vec<String> {
        let mut v: Vec<String> = std::fs::read_dir(p).into_iter().flatten().flatten().filter(|e| e.path().is_dir()).map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        v.sort();
        v
    };
    let print = |kind: &str, id: &str, which: &str, cs: Vec<config::Config>| {
        for c in cs {
            println!("== {kind} {id} {} {} {} {}", which.replace(' ', "%20"), c.cached as u8, config::cache_name(&c.file), c.file.display());
            for (s, keys) in &c.sections {
                for (k, v) in keys {
                    let e = |b: &[u8]| String::from_utf8_lossy(b).replace('\n', "\\n");
                    println!("{}\x1f{}\x1f{}", e(s), e(k), v.iter().map(|x| e(x)).collect::<Vec<_>>().join("\x1e"));
                }
            }
        }
    };
    let cars = ac.join("content").join("cars");
    for id in dirs(cars.clone()) {
        let skins = dirs(cars.join(&id).join("skins"));
        let own: Vec<&String> = skins.iter().filter(|s| cars.join(&id).join("skins").join(s).join("ext_config.ini").is_file()).collect();
        for skin in skins.first().into_iter().chain(own).collect::<std::collections::BTreeSet<_>>() {
            // the player's car and a later one (CSP's rain tyres for every car once the player's is loaded)
            print("car", &id, skin, config::car(&f, &std, &id, skin, Some(0)));
            print("car", &id, skin, config::car(&f, &std, &id, skin, Some(1)));
        }
    }
    let tracks = ac.join("content").join("tracks");
    for id in dirs(tracks.clone()) {
        let ui = tracks.join(&id).join("ui");
        let mut layouts: Vec<String> = dirs(ui.clone()).into_iter().filter(|l| ui.join(l).join("ui_track.json").is_file()).collect();
        if ui.join("ui_track.json").is_file() || layouts.is_empty() {
            layouts.insert(0, String::new());
        }
        for l in layouts {
            print("track", &id, if l.is_empty() { "-" } else { &l }, config::track(&f, &std, &id, &l));
        }
    }
}
