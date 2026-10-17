// SPDX-License-Identifier: GPL-3.0-or-later

//! The bridge's answers to a list of requests, written the way tools/launcher_oracle writes
//! AssettoCorsa.exe's: one `== <request>` line, then `<mime> <ok> <length>`, the body and a
//! newline. Request lines are `GET <url>`, `POST <url><TAB><body>`, `FORMAT <name>` and
//! `SETUP x`.

use std::io::Write;

use crate::bridge::{Bridge, Host};
use crate::folders::AppFolder;
use crate::{appini, net, setup};

/// The setup's questions answered as a check wants them: no version prompt, errors ignored.
pub struct NoQuestions;

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

/// rustyAC's own host for checks: the same answers on every machine and in wasm (a made-up
/// screen and modes, UTC times, English month names, no Steam), so a transcript of the
/// desktop's bridge and one of its wasm build can be compared byte for byte.
pub struct Fixed;

/// Calendar fields of a time in UTC.
fn utc_fields(t: std::time::SystemTime) -> [i64; 7] {
    let d = t.duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
    let secs = d.as_secs() as i64;
    let (days, rest) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // days since 1970-01-01 to a civil date (Howard Hinnant's algorithm)
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    [year, month, day, rest / 3600, rest / 60 % 60, rest % 60, i64::from(d.subsec_millis())]
}

impl Host for Fixed {
    fn primary_resolution(&self) -> (f64, f64) {
        (1920.0, 1080.0)
    }
    fn display_modes(&self) -> Vec<(u32, u32)> {
        vec![(1280, 720), (1920, 1080)]
    }
    fn video_modes(&self) -> Option<Vec<(i32, i32, f32)>> {
        Some(vec![(1280, 720, 60.0), (1920, 1080, 60.0)])
    }
    fn local_time(&self, file: Option<&str>) -> [i64; 7] {
        utc_fields(file.map_or_else(std::time::SystemTime::now, net::modified))
    }
    fn month_abbrev(&self, month: u32) -> String {
        ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"][(month as usize).clamp(1, 12) - 1].to_string()
    }
    fn player_name(&self) -> String {
        "You, the player".into()
    }
    fn player_id(&self) -> String {
        "00000000-0000-0000-0000-000000000037".into()
    }
}

/// Runs `requests` against the player's folder `docs` and the game in `ac_root`.
pub fn run(ac_root: &str, docs: &str, requests: &str, out_file: &str, lobby: Option<String>, host: Box<dyn Host>) -> std::io::Result<()> {
    let text = std::fs::read_to_string(requests)?;
    let out = run_text(ac_root, docs, &text, lobby, host);
    std::fs::File::create(out_file)?.write_all(&out)
}

/// [`run`] on the request list's text, the transcript returned (the browser's check).
pub fn run_text(ac_root: &str, docs: &str, text: &str, lobby: Option<String>, host: Box<dyn Host>) -> Vec<u8> {
    let folders = AppFolder::new(ac_root, docs);
    let mut bridge = Bridge::new(folders.clone(), host);
    bridge.lobby = std::sync::Arc::new(std::sync::Mutex::new(crate::online::Lobby::new(lobby)));
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
    out
}
