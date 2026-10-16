// SPDX-License-Identifier: GPL-3.0-or-later

//! The bridge's answers to a list of requests, written the way tools/launcher_oracle writes
//! AssettoCorsa.exe's: one `== <request>` line, then `<mime> <ok> <length>`, the body and a
//! newline. Request lines are `GET <url>`, `POST <url><TAB><body>`, `FORMAT <name>` and
//! `SETUP x`.

use std::io::Write;

use crate::bridge::Bridge;
use crate::folders::AppFolder;
use crate::{appini, setup, sys};

struct NoQuestions;

impl setup::Ask for NoQuestions {
    fn version_mismatch(&self, _: &str) -> bool {
        false
    }
    fn setup_error(&self, _: &str) {}
}

fn record(out: &mut Vec<u8>, request: &str, mime: &str, ok: bool, body: &[u8]) {
    out.extend_from_slice(format!("== {request}\n{mime} {ok} {}\n", body.len()).as_bytes());
    out.extend_from_slice(body);
    out.push(b'\n');
}

/// Runs `requests` against the player's folder `docs` and the game in `ac_root`.
pub fn run(ac_root: &str, docs: &str, requests: &str, out_file: &str, lobby: Option<String>) -> std::io::Result<()> {
    let folders = AppFolder::new(ac_root, docs);
    // no Steam: GetPersonaName's own answer
    let mut bridge = Bridge::new(folders.clone(), Box::new(sys::Windows { player_name: "You, the player".into() }));
    bridge.lobby = std::sync::Arc::new(std::sync::Mutex::new(crate::online::Lobby::new(lobby)));
    let text = std::fs::read_to_string(requests)?;
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.trim_end_matches('\r');
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (verb, rest) = line.split_once(' ').unwrap_or((line, ""));
        match verb {
            "FORMAT" => {
                appini::format(&appini::get(&folders, rest));
                record(&mut out, line, "-", false, b"done");
            }
            "SETUP" => {
                let result = match setup::setup_user_directory(&folders, &NoQuestions) {
                    Ok(()) => "done",
                    Err(_) => "exit",
                };
                record(&mut out, line, "-", false, result.as_bytes());
            }
            _ => {
                let (url, body) = match verb {
                    "POST" => rest.split_once('\t').map_or((rest, None), |(u, b)| (u, Some(b))),
                    _ => (rest, None),
                };
                let (answer, _) = bridge.request(url, body);
                record(&mut out, line, &answer.mime, answer.ok, &answer.body);
            }
        }
    }
    std::fs::File::create(out_file)?.write_all(&out)
}
