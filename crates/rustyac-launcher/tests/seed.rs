// SPDX-License-Identifier: GPL-3.0-or-later

//! The launcher's cfg folder is first filled from the player's real one: what is missing is
//! copied, what the launcher has is kept, and the player's folder is only read.
#![cfg(windows)]

use rustyac_launcher::folders::AppFolder;
use rustyac_launcher::setup::seed_from_player;

#[test]
fn missing_cfg_files_come_from_the_player_and_nothing_else_changes() {
    let base = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("seed");
    let _ = std::fs::remove_dir_all(&base);
    let (player, ours) = (base.join("documents_cfg"), base.join("launcher"));
    std::fs::create_dir_all(player.join("ppfilters")).unwrap();
    std::fs::create_dir_all(ours.join("cfg")).unwrap();
    std::fs::write(player.join("controls.ini"), "[HEADER]\nINPUT_METHOD=KEYBOARD\n").unwrap();
    std::fs::write(player.join("video.ini"), "[VIDEO]\nWIDTH=1920\n").unwrap();
    std::fs::write(player.join("notes.txt"), "not an ini").unwrap();
    std::fs::write(player.join("ppfilters").join("a.ini"), "x").unwrap();
    std::fs::write(ours.join("cfg").join("video.ini"), "[VIDEO]\nWIDTH=800\n").unwrap();
    let before: Vec<_> = ["controls.ini", "video.ini", "notes.txt"].iter().map(|n| std::fs::read(player.join(n)).unwrap()).collect();

    let f = AppFolder::new("C:\\no_game", &ours.to_string_lossy());
    assert_eq!(seed_from_player(&f, &player), ["controls.ini"]);
    assert_eq!(std::fs::read_to_string(ours.join("cfg").join("controls.ini")).unwrap(), "[HEADER]\nINPUT_METHOD=KEYBOARD\n");
    // the launcher's own file is kept, folders and other files are not copied
    assert_eq!(std::fs::read_to_string(ours.join("cfg").join("video.ini")).unwrap(), "[VIDEO]\nWIDTH=800\n");
    assert!(!ours.join("cfg").join("notes.txt").exists() && !ours.join("cfg").join("ppfilters").exists());
    // a second start copies nothing, and the player's folder is as it was
    assert!(seed_from_player(&f, &player).is_empty());
    let after: Vec<_> = ["controls.ini", "video.ini", "notes.txt"].iter().map(|n| std::fs::read(player.join(n)).unwrap()).collect();
    assert_eq!(before, after);
    // no player folder: nothing
    assert!(seed_from_player(&f, &base.join("missing")).is_empty());
}
