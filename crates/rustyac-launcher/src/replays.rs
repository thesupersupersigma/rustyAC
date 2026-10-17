// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa's launcher (AssettoCorsa.exe, Kunos Simulazioni).

//! The replays page's handlers (`ListReplays`, `AddReplay`, `DelReplay`, `UpdReplay`,
//! `StoreReplay`): files in the player's `replay\` folder (and its `temp/`), nowhere else.

use crate::bridge::{io_thrown, Answer, Host, Thrown};
use crate::jss::{self, Value};
use crate::net;
use crate::query::Params;

/// `ListReplays`: `[name, "dd/MM/yyyy HH:mm", MB "0.00", ms since 1970 (local), bytes]` per file
/// but `cr` (the game's last replay), in the folder's order or newest first (`bytime`).
pub fn list(dir: &str, host: &dyn Host, p: &Params) -> Result<Answer, Thrown> {
    let folder = if p.get("temp").as_deref() == Some("true") { format!("{dir}temp/") } else { dir.to_string() };
    let files = net::list(&folder, false).map_err(|e| io_thrown(&folder, &e))?;
    let mut rows: Vec<(i64, Value)> = Vec::new();
    for f in files.iter().filter(|f| net::file_name(f) != "cr") {
        let len = net::len(f);
        let [y, mo, d, h, mi, s, ms] = host.local_time(Some(f));
        let since = days_from_civil(y, mo, d) * 86_400_000 + ((h * 60 + mi) * 60 + s) * 1000 + ms;
        let row = format!("{},{d:02}/{mo:02}/{y:04} {h:02}:{mi:02},{},{since},{len}", net::file_name(f), mb(len));
        // the launcher joins the fields with commas and splits them again: a comma in a name splits it too
        rows.push((since, Value::Array(row.split(',').map(|v| Value::Str(v.to_string())).collect())));
    }
    if p.get("bytime").as_deref() == Some("true") {
        // OrderByDescending(LastWriteTime), a stable sort
        rows.sort_by_key(|r| std::cmp::Reverse(r.0));
    }
    Ok(Answer::json(&jss::serialize(&Value::Array(rows.into_iter().map(|r| r.1).collect()))))
}

/// `AddReplay`, `DelReplay`, `UpdReplay`, `StoreReplay`.
pub fn change(host: &str, dir: &str, stamp: &str, p: &Params) -> Result<Answer, Thrown> {
    let name = |key: &str| p.get(key).unwrap_or_default();
    let text = match host {
        "addreplay" => {
            let from = format!("{dir}cr");
            let to = format!("{}@{}_{stamp}", name("car"), name("track"));
            if !net::file_exists(&from) {
                "ERROR,No replay file".to_string()
            } else if move_file(&from, &format!("{dir}{to}")) {
                "OK".to_string()
            } else {
                format!("ERROR,Unable to save {to}")
            }
        }
        "delreplay" => {
            let path = format!("{dir}{}", name("name"));
            if !net::file_exists(&path) {
                "ERROR,No such file".to_string()
            } else if net::remove_file(&path).is_ok() {
                "OK".to_string()
            } else {
                format!("ERROR,Unable to delete {}", name("name"))
            }
        }
        "updreplay" => {
            let (old, new) = (name("oldname"), name("newname"));
            if !net::file_exists(&format!("{dir}{old}")) {
                "ERROR,No such file".to_string()
            } else if move_file(&format!("{dir}{old}"), &format!("{dir}{new}")) {
                "OK".to_string()
            } else {
                format!("ERROR,Unable to rename {old} to {new}")
            }
        }
        _ => {
            let files = p.get("files").ok_or(Thrown::NullReference)?;
            let mut text = "OK".to_string();
            for f in files.split(',') {
                if !net::file_exists(&format!("{dir}temp/{f}")) {
                    text = "ERROR,No such file".to_string();
                    break;
                }
                if !move_file(&format!("{dir}temp/{f}"), &format!("{dir}{f}")) {
                    text = format!("ERROR,Unable to store {f}");
                    break;
                }
            }
            text
        }
    };
    Ok(Answer::message(&text))
}

/// `File.Move`: never over a file that is there (Rust's rename would replace it).
fn move_file(from: &str, to: &str) -> bool {
    net::rename(from, to).is_ok()
}

/// `((float)len / 1024f / 1024f).ToString("0.00")`: .NET Framework formats a float from its
/// first 7 significant digits, then rounds those half away from zero.
fn mb(len: u64) -> String {
    let x = len as f32 / 1024.0 / 1024.0;
    let sci = format!("{:.6e}", x as f64);
    let (mant, exp) = sci.split_once('e').unwrap();
    let digits: Vec<u8> = mant.bytes().filter(u8::is_ascii_digit).map(|b| b - b'0').collect();
    let exp: i32 = exp.parse().unwrap();
    // the value is 0.d1d2..d7 * 10^(exp+1): hundredths as an integer, half away from zero
    let mut hundredths: u64 = 0;
    let keep = exp + 3;
    for (i, &d) in digits.iter().enumerate() {
        if (i as i32) < keep {
            hundredths = hundredths * 10 + d as u64;
        } else if i as i32 == keep && d >= 5 {
            hundredths += 1;
        }
    }
    for _ in digits.len() as i32..keep.max(0) {
        hundredths *= 10;
    }
    if keep < 0 {
        hundredths = 0;
    }
    format!("{}.{:02}", hundredths / 100, hundredths % 100)
}

/// Days from 1970-01-01 to a date (proleptic Gregorian).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_and_days() {
        assert_eq!(mb(0), "0.00");
        assert_eq!(mb(1_048_576), "1.00");
        assert_eq!(mb(5_242), "0.00");
        assert_eq!(mb(5_243), "0.01");
        assert_eq!(mb(123_456_789), "117.74");
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(days_from_civil(2026, 10, 16), 20_742);
    }
}
