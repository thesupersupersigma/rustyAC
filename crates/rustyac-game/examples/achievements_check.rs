// SPDX-License-Identifier: GPL-3.0-or-later

//! Task 47d: `achievements_check <achievements.tape> <scratch root> <AC root>`: the port's
//! `SteamInterface` (`rustyac_game::achievements`) fed the inputs of the game's run
//! (`car_oracle achievements-micro`) on the same stand-in Steam, every call, argument, message
//! and printf compared line by line.

use std::path::PathBuf;

use rustyac_game::achievements::{Achievements, Flat, Frame, Host, Lap, Row, Session};
use rustyac_game::steam::Interface;
use rustyac_physics::data::ini::IniReader;
use rustyac_physics::race::game_mode::{GameMode, Mode};
use rustyac_physics::track::timing::OnLapCompletedEvent;

#[allow(dead_code, clippy::all)]
#[path = "../../../tools/car_oracle/src/game/steam_standin.rs"]
mod standin;

use standin::{log, Answers};

struct Logger;

impl Host for Logger {
    fn printf(&mut self, line: &str) {
        if line.to_lowercase().contains("steam") || line.starts_with("ERROR: NO STAT FOR") {
            log(format!("printf {line}"));
        }
    }
    fn message(&mut self, text: &str, description: &str) {
        log(format!("Message({text} | {description} | 0)"));
    }
    fn create_texture(&mut self, width: u32, height: u32, rgba: &[u8]) -> u64 {
        log(format!("Texture {width}x{height} format 1 {:08x}", standin::image_hash(rgba)));
        0x51
    }
    fn destroy_texture(&mut self, id: u64) {
        log(format!("DestroyTexture {id:#x}"));
    }
    fn rows(&mut self, rows: &[Row]) {
        log(format!("rows {}", rows.len()));
        for r in rows {
            log(format!("row {} | {} | {} | {}", r.name, r.score, r.rank, r.is_player as u8));
        }
    }
}

unsafe extern "C" fn user_stats() -> Interface {
    standin::steam_user_stats() as Interface
}
unsafe extern "C" fn user() -> Interface {
    standin::steam_user() as Interface
}
unsafe extern "C" fn friends() -> Interface {
    standin::steam_friends() as Interface
}
unsafe extern "C" fn utils() -> Interface {
    standin::steam_utils() as Interface
}

fn flat() -> Flat {
    unsafe extern "C" fn run() {
        standin::run_callbacks()
    }
    unsafe extern "C" fn reg(cb: *mut u8, id: i32) {
        standin::register_callback(cb, id)
    }
    unsafe extern "C" fn unreg(cb: *mut u8) {
        standin::unregister_callback(cb)
    }
    unsafe extern "C" fn reg_result(cb: *mut u8, h: u64) {
        standin::register_call_result(cb, h)
    }
    unsafe extern "C" fn unreg_result(cb: *mut u8, h: u64) {
        standin::unregister_call_result(cb, h)
    }
    Flat { run_callbacks: run, register_callback: reg, unregister_callback: unreg, register_call_result: reg_result, unregister_call_result: unreg_result, user_stats, user, friends, utils }
}

fn unescape(s: &str) -> String {
    let mut out = String::new();
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') => out.push('\n'),
                Some(other) => out.push(other),
                None => {}
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// `key value key value …` after the line's first word
fn fields(line: &str) -> Vec<&str> {
    line.split(' ').skip(2).collect()
}

fn value<'a>(words: &[&'a str], key: &str) -> &'a str {
    words.iter().position(|w| *w == key).map_or("", |i| words.get(i + 1).copied().unwrap_or(""))
}

fn frame(line: &str) -> Frame {
    let w: Vec<&str> = line.split(' ').collect();
    let at = |key: &str| w.iter().position(|x| *x == key).unwrap() + 1;
    let f = |i: usize| w[i].parse::<f32>().unwrap();
    let b = |i: usize| w[i] == "1";
    let (p, d, l, r) = (at("pos"), at("damage"), at("lap"), at("replay"));
    Frame {
        speed: f(at("speed")),
        pos: [f(p), f(p + 1), f(p + 2)],
        laps: w[at("laps")].parse().unwrap(),
        combo: w[at("combo")].parse().unwrap(),
        damage: [f(d), f(d + 1), f(d + 2), f(d + 3), f(d + 4)],
        abs: b(at("abs")),
        tc: b(at("tc")),
        auto_shifter: b(at("shifter")),
        auto_blip: b(at("blip")),
        drift_level: w[at("level")].parse().ok(),
        replay: b(r),
        replay_loaded: b(r + 1),
        lap: Lap { time: w[l].parse().unwrap(), valid: b(l + 1), cuts: w[l + 2].parse().unwrap(), splits: w[l + 3].parse().unwrap() },
    }
}

struct Case {
    session: Session,
    answers: Answers,
    game_mode: Option<GameMode>,
    port: Option<Achievements>,
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 4 {
        eprintln!("usage: achievements_check <achievements.tape> <scratch root> <AC root>");
        std::process::exit(2);
    }
    let tape = std::fs::read_to_string(&args[1]).expect("the tape");
    let root = PathBuf::from(&args[2]);
    rustyac_content::install::set_ac_root(Some(PathBuf::from(&args[3])));
    std::fs::create_dir_all(root.join("cfg")).unwrap();

    let mut out: Vec<String> = Vec::new();
    let mut case: Option<Case> = None;
    for line in tape.lines().filter(|l| l.starts_with("> ")) {
        out.push(line.to_string());
        let word = line.split(' ').nth(1).unwrap_or("");
        match word {
            "case" => {
                let w = fields(line);
                let config = value(&w, "config");
                case = Some(Case {
                    session: Session {
                        session_type: value(&w, "type").parse().unwrap(),
                        penalties: value(&w, "penalties") == "1",
                        official: value(&w, "official") == "1",
                        car: value(&w, "car").into(),
                        track: value(&w, "track").into(),
                        track_config: if config == "-" { String::new() } else { config.into() },
                        sectors: value(&w, "sectors").parse().unwrap(),
                        game_mode: None,
                        documents: root.clone(),
                    },
                    answers: Answers::default(),
                    game_mode: None,
                    port: None,
                });
            }
            "answers" => {
                let w = fields(line);
                let n = |k: &str| value(&w, k).parse::<u64>().unwrap();
                let c = case.as_mut().unwrap();
                c.answers = Answers {
                    seed: n("seed") as u32,
                    steam_id: n("id"),
                    stats_ok: n("stats") == 1,
                    set_stat_ok: n("set") == 1,
                    store_ok: n("store") == 1,
                    found: n("found") == 1,
                    upload_ok: n("upload") == 1,
                    changed: n("changed") == 1,
                    entries: n("entries") as i32,
                    avatar: n("avatar") as i32,
                    image_ok: n("image") == 1,
                };
            }
            "file" => {
                let mut parts = line.splitn(4, ' ');
                let (_, _, name, text) = (parts.next(), parts.next(), parts.next().unwrap(), parts.next().unwrap_or(""));
                std::fs::write(root.join(name), unescape(text)).unwrap();
            }
            "nofile" => {
                let _ = std::fs::remove_file(root.join(line.split(' ').nth(2).unwrap()));
            }
            "new" => {
                let c = case.as_mut().unwrap();
                standin::reset(c.answers);
                if matches!(c.session.session_type, 4..=6) {
                    let documents = IniReader::load(&root.join("cfg/race.ini")).unwrap();
                    let official = documents.official_special_event();
                    let gm = GameMode::new(official.as_ref().unwrap_or(&documents), &documents, official.is_some());
                    c.session.game_mode = Some(gm.guid.clone());
                    c.game_mode = Some(gm);
                }
                // SAFETY: the stand-in's functions have Steamworks' signatures
                c.port = Some(unsafe { Achievements::new(flat(), Box::new(Logger), c.session.clone()) });
            }
            "frame" => case.as_mut().unwrap().port.as_mut().unwrap().update(frame(line)),
            "hotlap" => {
                let c = case.as_mut().unwrap();
                let w: Vec<&str> = line.split(' ').collect();
                let (time, valid, cuts) = (w[2].parse::<u32>().unwrap(), w[3] == "1", w[4].parse::<i32>().unwrap());
                let gm = c.game_mode.as_mut().unwrap();
                let before: Vec<bool> = gm.conditions.iter().map(|x| x.achieved).collect();
                let mut mode = Mode::of(4, gm).unwrap();
                let mut messages = Vec::new();
                mode.on_lap_completed(&OnLapCompletedEvent { lap_time: time, is_valid: valid, cuts, ..Default::default() }, &mut |a, b| messages.push((a.to_string(), b.to_string())));
                if let Mode::Hotlap(after) = mode {
                    *gm = after;
                }
                let port = c.port.as_mut().unwrap();
                let tiers = (0..before.len()).filter(|&i| !before[i] && gm.conditions[i].achieved);
                for (i, (a, b)) in tiers.zip(messages) {
                    log(format!("Message({a} | {b} | 0)"));
                    port.tier(i as i32);
                }
                if time > 0 && valid && cuts == 0 {
                    port.score(time as i64 as f32);
                }
            }
            "score" => {
                let score = line.split(' ').nth(2).unwrap().parse::<f32>().unwrap();
                case.as_mut().unwrap().port.as_mut().unwrap().score(score);
            }
            "refresh" => case.as_mut().unwrap().port.as_mut().unwrap().refresh(),
            "reset" => case.as_mut().unwrap().port.as_mut().unwrap().reset(),
            "resetach" => case.as_mut().unwrap().port.as_mut().unwrap().reset_achievements(),
            "end" => {
                drop(case.as_mut().unwrap().port.take());
            }
            other => panic!("unknown input {other}"),
        }
        if let Some(c) = case.as_mut() {
            if let (Some(port), Some(gm)) = (c.port.as_mut(), c.game_mode.as_mut()) {
                for (k, achieved) in port.take_forced() {
                    if k < gm.conditions.len() {
                        gm.conditions[k].achieved = achieved;
                    }
                }
            }
        }
        out.extend(standin::take_calls());
    }

    let game: Vec<&str> = tape.lines().collect();
    let n = game.len().min(out.len());
    match (0..n).find(|&i| game[i] != out[i]) {
        Some(i) => {
            println!("DIFFERENT at line {}:\n  game: {}\n  port: {}", i + 1, game[i], out[i]);
            std::process::exit(1);
        }
        None if game.len() != out.len() => {
            println!("DIFFERENT: the game has {} lines, the port {}", game.len(), out.len());
            std::process::exit(1);
        }
        None => println!("IDENTICAL ({} lines, {} cases)", game.len(), tape.lines().filter(|l| l.starts_with("> case ")).count()),
    }
}

