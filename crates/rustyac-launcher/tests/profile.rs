// SPDX-License-Identifier: GPL-3.0-or-later

//! `rustyac_launcher::profile` against Windows' own `GetPrivateProfileStringW` and
//! `WritePrivateProfileStringW`: generated ANSI files, every read the launcher makes (and short
//! buffers), then random edits; the buffers, answers and file bytes must be the same.
//! `PROFILE_CASES=<n>` sets how many files (default 3,000).
#![cfg(windows)]

use std::path::Path;

use rustyac_launcher::profile;
use windows::core::{HSTRING, PCWSTR};
use windows::Win32::System::WindowsProgramming::{GetPrivateProfileStringW, WritePrivateProfileStringW};

/// A read: section, key, default, buffer size.
type Query = (Option<&'static str>, Option<&'static str>, Option<&'static str>, usize);

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
    fn pick<'a, T>(&mut self, of: &'a [T]) -> &'a T {
        &of[self.below(of.len())]
    }
}

const NAMES: &[&str] = &["a", "A", "key", "KEY", "Key", "x y", "\u{e9}t\u{e9}", "\u{c9}T\u{c9}", "\u{ff}", "\u{178}", "\u{b5}", "\u{df}", "\u{20ac}", "", "s1", "S1", "pad"];
const SPACES: &[&str] = &["", "", "", " ", "  ", "\t", " \t ", "\u{b}", "\u{c}"];
const VALUES: &[&str] = &["1", "v", "two words", "\"q\"", "'q'", "\"", "\"a'", "a=b", "1;c", " ; ", "", "\u{e9}\u{20ac}\u{81}", "x\"y\""];
const ENDS: &[&str] = &["\r\n", "\r\n", "\r\n", "\n", "\r", "\n\r", ""];

fn sp(r: &mut Rng) -> String {
    r.pick(SPACES).to_string()
}

fn gen_file(r: &mut Rng) -> String {
    let mut s = String::new();
    for _ in 0..r.below(14) {
        let line = match r.below(12) {
            0 | 1 => format!("{}[{}{}{}]{}", sp(r), sp(r), r.pick(NAMES), sp(r), if r.below(5) == 0 { " junk" } else { "" }),
            2 => format!("[{}", r.pick(NAMES)),
            3 => format!("{};{}={}", sp(r), r.pick(NAMES), r.pick(VALUES)),
            4 => sp(r),
            5 => format!("{}{}", sp(r), r.pick(NAMES)),
            _ => format!("{}{}{}={}{}{}", sp(r), r.pick(NAMES), sp(r), sp(r), r.pick(VALUES), sp(r)),
        };
        s.push_str(&line);
        s.push_str(r.pick(ENDS));
    }
    s
}

fn wide(s: &str) -> Vec<u16> {
    // as the launcher passes names: the characters of code page 1252
    s.encode_utf16().chain([0]).collect()
}

fn ptr(w: &Option<Vec<u16>>) -> PCWSTR {
    w.as_ref().map_or(PCWSTR::null(), |w| PCWSTR(w.as_ptr()))
}

fn win_get(path: &Path, section: Option<&str>, key: Option<&str>, default: Option<&str>, size: usize) -> (Vec<u16>, u32) {
    let mut buf = vec![0x7eu16; size + 2];
    let (s, k, d) = (section.map(wide), key.map(wide), default.map(wide));
    let n = unsafe { GetPrivateProfileStringW(ptr(&s), ptr(&k), ptr(&d), Some(&mut buf[..size]), &HSTRING::from(path)) };
    (buf, n)
}

fn pure_get(file: Option<&[u8]>, section: Option<&str>, key: Option<&str>, default: Option<&str>, size: usize) -> (Vec<u16>, u32) {
    let units = |s: &str| s.encode_utf16().collect::<Vec<u16>>();
    let (s, k, d) = (section.map(units), key.map(units), default.map(units));
    let (written, n) = profile::get_string(file, s.as_deref(), k.as_deref(), d.as_deref(), size);
    let mut buf = vec![0x7eu16; size + 2];
    buf[..written.len()].copy_from_slice(&written);
    (buf, n)
}

fn win_write(path: &Path, section: &str, key: Option<&str>, value: Option<&str>) {
    let (s, k, v) = (Some(wide(section)), key.map(wide), value.map(wide));
    let _ = unsafe { WritePrivateProfileStringW(ptr(&s), ptr(&k), ptr(&v), &HSTRING::from(path)) };
}

/// Whether Windows takes this file for UTF-16 (it guesses so for some short ANSI files, such as
/// "[" and a line feed): it then writes UTF-16 into it. The pure copy reads every file as ANSI (its shortcut).
fn windows_sees_utf16(dir: &Path, bytes: &[u8]) -> bool {
    let probe = dir.join("probe.ini");
    std::fs::write(&probe, bytes).unwrap();
    win_write(&probe, "probe", Some("k"), Some("v"));
    let after = std::fs::read(&probe).unwrap_or_default();
    !bytes.contains(&0) && after.contains(&0)
}

#[test]
fn the_pure_copy_reads_and_writes_as_windows_does() {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("profile");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("t.ini");
    let cases: usize = std::env::var("PROFILE_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(3000);
    let mut r = Rng(0x9e37_79b9_7f4a_7c15);
    let (mut reads, mut writes, mut utf16) = (0usize, 0usize, 0usize);
    for case in 0..cases {
        let mut file: Option<Vec<u8>> = match r.below(10) {
            0 => None,
            1 => Some(Vec::new()),
            _ => Some(profile::wide_to_ansi(&gen_file(&mut r).encode_utf16().collect::<Vec<u16>>())),
        };
        if file.as_deref().is_some_and(|f| windows_sees_utf16(&dir, f)) {
            utf16 += 1;
            continue;
        }
        let _ = std::fs::remove_file(&path);
        if let Some(bytes) = &file {
            std::fs::write(&path, bytes).unwrap();
        }
        let mut queries: Vec<Query> = vec![(None, None, None, 65535)];
        for _ in 0..8 {
            let (s, k) = (*r.pick(NAMES), *r.pick(NAMES));
            queries.push((Some(s), None, None, 32768));
            queries.push((Some(s), Some(k), Some(""), 65535));
        }
        for _ in 0..3 {
            let size = r.below(9);
            let (s, k) = (*r.pick(NAMES), *r.pick(NAMES));
            queries.push((Some(s), None, None, size));
            queries.push((None, None, None, size));
            queries.push((Some(s), Some(k), Some(*r.pick(&["", "def", "de ", "default"])), size));
        }
        for (s, k, d, size) in queries {
            let w = win_get(&path, s, k, d, size);
            let p = pure_get(file.as_deref(), s, k, d, size);
            // the first units are enough to see a difference
            let head = |b: &(Vec<u16>, u32)| (String::from_utf16_lossy(&b.0[..b.0.len().min(48)]), b.1);
            assert_eq!(head(&p), head(&w), "case {case}: read {s:?} {k:?} {d:?} size {size} of {:?}", file.as_ref().map(|f| String::from_utf8_lossy(f).to_string()));
            assert_eq!(p, w, "case {case}: read {s:?} {k:?} {d:?} size {size}: the buffers' ends differ");
            reads += 1;
        }
        for _ in 0..r.below(6) {
            let section = format!("{}{}{}", sp(&mut r), r.pick(NAMES), sp(&mut r));
            let key = match r.below(8) {
                0 => None,
                _ => Some(format!("{}{}{}", sp(&mut r), r.pick(NAMES), sp(&mut r))),
            };
            let value = match r.below(4) {
                0 => None,
                _ => Some(format!("{}{}", if r.below(4) == 0 { "  " } else { "" }, r.pick(VALUES))),
            };
            if file.as_deref().is_some_and(|f| windows_sees_utf16(&dir, f)) {
                utf16 += 1;
                break;
            }
            let before = file.clone();
            win_write(&path, &section, key.as_deref(), value.as_deref());
            let units = |s: &str| s.encode_utf16().collect::<Vec<u16>>();
            let pure = profile::write_string(file.as_deref(), &units(&section), key.as_deref().map(units).as_deref(), value.as_deref().map(units).as_deref());
            if let Some(bytes) = pure {
                file = Some(bytes);
            }
            let on_disk = std::fs::read(&path).ok();
            assert_eq!(
                file.as_ref().map(|f| String::from_utf8_lossy(f).to_string()),
                on_disk.as_ref().map(|f| String::from_utf8_lossy(f).to_string()),
                "case {case}: write {section:?} {key:?} {value:?} to {:?}",
                before.as_ref().map(|f| String::from_utf8_lossy(f).to_string())
            );
            assert_eq!(file, on_disk, "case {case}: bytes");
            writes += 1;
        }
    }
    println!("profile: {cases} files, {reads} reads and {writes} writes the same as Windows ({utf16} times a file Windows takes for UTF-16 was left out)");
}
