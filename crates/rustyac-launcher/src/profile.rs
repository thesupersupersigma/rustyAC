// SPDX-License-Identifier: GPL-3.0-or-later

//! A pure-Rust copy of Windows' `GetPrivateProfileStringW` and `WritePrivateProfileStringW` on
//! an ANSI file (code page 1252), for where there is no Win32: the browser's launcher. It works
//! on the file's bytes, so the caller keeps the file wherever it likes. Every rule here was taken
//! from what Windows does (re/scratch/task37/ini_probe*.ps1) and `tests/profile.rs` checks it
//! against Windows byte for byte on generated files and edits.
//!
//! shortcut: every file is read as ANSI bytes, where Windows reads a file as UTF-16 when it
//! starts with a byte order mark or when it guesses so (some tiny files such as "[" and a line feed); no
//! launcher or game file is one. Upgrade if one shows up.

/// Code page 1252's bytes 0x80..0x9f; the rest are the same number in Unicode.
const CP1252_HIGH: [u16; 32] = [
    0x20ac, 0x0081, 0x201a, 0x0192, 0x201e, 0x2026, 0x2020, 0x2021, 0x02c6, 0x2030, 0x0160, 0x2039, 0x0152, 0x008d, 0x017d, 0x008f,
    0x0090, 0x2018, 0x2019, 0x201c, 0x201d, 0x2022, 0x2013, 0x2014, 0x02dc, 0x2122, 0x0161, 0x203a, 0x0153, 0x009d, 0x017e, 0x0178,
];

/// `MultiByteToWideChar(1252)`.
pub fn ansi_to_wide(bytes: &[u8]) -> Vec<u16> {
    bytes.iter().map(|&b| if (0x80..0xa0).contains(&b) { CP1252_HIGH[b as usize - 0x80] } else { b as u16 }).collect()
}

/// `WideCharToMultiByte(1252)` for the characters the code page has; any other becomes `?`.
pub fn wide_to_ansi(units: &[u16]) -> Vec<u8> {
    units
        .iter()
        .map(|&u| match u {
            0..=0x7f | 0xa0..=0xff => u as u8,
            _ => CP1252_HIGH.iter().position(|&h| h == u).map_or(b'?', |i| 0x80 + i as u8),
        })
        .collect()
}

const CR: u16 = b'\r' as u16;
const LF: u16 = b'\n' as u16;
const OPEN: u16 = b'[' as u16;
const SEMICOLON: u16 = b';' as u16;

fn is_space(u: u16) -> bool {
    matches!(u, 0x20 | 0x09 | 0x0b | 0x0c)
}

fn trim(s: &[u16]) -> &[u16] {
    let start = s.iter().position(|&u| !is_space(u)).unwrap_or(s.len());
    let end = s.iter().rposition(|&u| !is_space(u)).map_or(start, |e| e + 1);
    &s[start..end]
}

/// Windows matches names ignoring the case of ASCII letters only (é is not É).
fn same_name(a: &[u16], b: &[u16]) -> bool {
    let up = |u: u16| if (b'a' as u16..=b'z' as u16).contains(&u) { u - 32 } else { u };
    a.len() == b.len() && a.iter().zip(b).all(|(&x, &y)| up(x) == up(y))
}

/// One line of the file: where it starts, where its text ends and where the next one starts.
#[derive(Clone, Copy)]
struct Line {
    start: usize,
    end: usize,
    next: usize,
}

enum Kind {
    Section { name: (usize, usize) },
    Key { name: (usize, usize), equals: usize },
    Other,
}

fn lines(text: &[u16]) -> Vec<Line> {
    let mut out = Vec::new();
    let mut start = 0;
    while start < text.len() {
        let end = text[start..].iter().position(|&u| u == CR || u == LF).map_or(text.len(), |i| start + i);
        let next = match text.get(end) {
            Some(&CR) if text.get(end + 1) == Some(&LF) => end + 2,
            Some(_) => end + 1,
            None => end,
        };
        out.push(Line { start, end, next });
        start = next;
    }
    out
}

/// `(start, end)` of `s[from..to]` without the spaces around it.
fn trimmed(text: &[u16], from: usize, to: usize) -> (usize, usize) {
    let t = trim(&text[from..to]);
    let start = from + text[from..to].iter().position(|&u| !is_space(u)).unwrap_or(to - from);
    (start, start + t.len())
}

fn kind(text: &[u16], line: Line) -> Kind {
    let first = (line.start..line.end).find(|&i| !is_space(text[i]));
    match first.map(|i| (i, text[i])) {
        Some((i, OPEN)) => {
            let close = (i + 1..line.end).find(|&j| text[j] == b']' as u16).unwrap_or(line.end);
            Kind::Section { name: trimmed(text, i + 1, close) }
        }
        Some((_, SEMICOLON)) | None => Kind::Other,
        Some(_) => match (line.start..line.end).find(|&j| text[j] == b'=' as u16) {
            Some(equals) => Kind::Key { name: trimmed(text, line.start, equals), equals },
            None => Kind::Other,
        },
    }
}

struct Parsed {
    text: Vec<u16>,
    lines: Vec<Line>,
}

impl Parsed {
    fn new(file: &[u8]) -> Parsed {
        let text = ansi_to_wide(file);
        let lines = lines(&text);
        Parsed { text, lines }
    }

    /// Where line `i`'s text starts: Windows deletes a line from there, not from the line's start.
    fn text_start(&self, i: usize) -> usize {
        let line = self.lines[i];
        (line.start..line.end).find(|&j| !is_space(self.text[j])).unwrap_or(line.start)
    }

    fn name(&self, (a, b): (usize, usize)) -> &[u16] {
        &self.text[a..b]
    }

    /// The first section called `name`: the index of its line and of the next section's line.
    fn section(&self, name: &[u16]) -> Option<(usize, usize)> {
        let name = trim_blanks(name);
        let at = (0..self.lines.len()).find(|&i| matches!(kind(&self.text, self.lines[i]), Kind::Section { name: n } if same_name(self.name(n), name)))?;
        let next = (at + 1..self.lines.len()).find(|&i| matches!(kind(&self.text, self.lines[i]), Kind::Section { .. })).unwrap_or(self.lines.len());
        Some((at, next))
    }

    fn keys(&self, (at, next): (usize, usize)) -> impl Iterator<Item = (usize, (usize, usize), usize)> + '_ {
        (at + 1..next).filter_map(move |i| match kind(&self.text, self.lines[i]) {
            Kind::Key { name, equals } => Some((i, name, equals)),
            _ => None,
        })
    }

    fn key(&self, section: (usize, usize), key: &[u16]) -> Option<(usize, usize)> {
        let key = trim_blanks(key);
        self.keys(section).find(|&(_, name, _)| same_name(self.name(name), key)).map(|(i, _, equals)| (i, equals))
    }

    /// Whether the file's last line holds nothing but spaces.
    fn blank_end(&self) -> bool {
        self.lines.last().is_some_and(|l| self.text[l.start..l.end].iter().all(|&u| is_space(u)))
    }

    /// Where a section's text ends, for deleting it: after its last line that is neither blank
    /// nor a comment (or its name's line); in the file's last section the very end when the
    /// file's last line is blank.
    fn section_end(&self, (at, next): (usize, usize)) -> usize {
        if next == self.lines.len() && self.blank_end() {
            return self.text.len();
        }
        let text_of = |i: usize| -> Option<u16> {
            let l = self.lines[i];
            self.text[l.start..l.end].iter().copied().find(|&u| !is_space(u))
        };
        let last = (at + 1..next).rev().find(|&i| text_of(i).is_some_and(|u| u != SEMICOLON)).unwrap_or(at);
        self.lines[last].next
    }
}

/// Copies `s` as a value: at most `size - 1` units and a NUL. Returns what goes into the buffer
/// and the function's answer.
fn copy_value(s: &[u16], size: usize) -> (Vec<u16>, u32) {
    if size == 0 {
        return (Vec::new(), 0);
    }
    let n = s.len().min(size - 1);
    let mut out = s[..n].to_vec();
    out.push(0);
    (out, n as u32)
}

/// Copies a list of NUL-ended names and the closing NUL; cut short, it ends in two NULs.
fn copy_list(list: &[u16], size: usize) -> (Vec<u16>, u32) {
    match size {
        0 => (Vec::new(), 0),
        _ if list.len() + 1 < size => {
            let mut out = list.to_vec();
            out.push(0);
            (out, list.len() as u32)
        }
        1 => (vec![0], 0),
        _ => {
            let mut out = list[..size - 2].to_vec();
            out.extend([0, 0]);
            (out, size as u32 - 2)
        }
    }
}

/// `GetPrivateProfileStringW(section, key, default, buffer, size, file)` on a file's bytes
/// (`None`: no such file). Returns the units it writes at the start of the buffer and its answer.
pub fn get_string(file: Option<&[u8]>, section: Option<&[u16]>, key: Option<&[u16]>, default: Option<&[u16]>, size: usize) -> (Vec<u16>, u32) {
    let parsed = Parsed::new(file.unwrap_or_default());
    let default = || copy_value(trim_end(default.unwrap_or_default()), size);
    let Some(section) = section else {
        let mut list = Vec::new();
        for line in &parsed.lines {
            if let Kind::Section { name } = kind(&parsed.text, *line) {
                list.extend_from_slice(parsed.name(name));
                list.push(0);
            }
        }
        return copy_list(&list, size);
    };
    let Some(found) = parsed.section(section) else { return default() };
    let Some(key) = key else {
        let mut list = Vec::new();
        for (_, name, _) in parsed.keys(found) {
            list.extend_from_slice(parsed.name(name));
            list.push(0);
        }
        return copy_list(&list, size);
    };
    let Some((line, equals)) = parsed.key(found, key) else { return default() };
    let value = trim(&parsed.text[equals + 1..parsed.lines[line].end]);
    let value = match value {
        [q, .., e] if q == e && (*q == b'"' as u16 || *q == b'\'' as u16) => &value[1..value.len() - 1],
        _ => value,
    };
    copy_value(value, size)
}

fn trim_end(s: &[u16]) -> &[u16] {
    &s[..s.iter().rposition(|&u| !is_space(u)).map_or(0, |e| e + 1)]
}

/// What Windows takes off a name it writes: only plain spaces.
fn trim_blanks(s: &[u16]) -> &[u16] {
    let start = s.iter().position(|&u| u != 0x20).unwrap_or(s.len());
    let end = s.iter().rposition(|&u| u != 0x20).map_or(start, |e| e + 1);
    &s[start..end]
}

fn line_of(name: &[u16], value: &[u16]) -> Vec<u16> {
    let mut out = trim_blanks(name).to_vec();
    out.push(b'=' as u16);
    out.extend_from_slice(value);
    out.extend([CR, LF]);
    out
}

/// `WritePrivateProfileStringW(section, key, value, file)` on a file's bytes (`None`: no such
/// file; `key` or `value` `None` deletes the section or the key). Returns the file's new bytes,
/// or `None` when Windows leaves the file as it is (a missing file it always makes).
pub fn write_string(file: Option<&[u8]>, section: &[u16], key: Option<&[u16]>, value: Option<&[u16]>) -> Option<Vec<u8>> {
    let mut parsed = Parsed::new(file.unwrap_or_default());
    // the text after the last LF: with text in it, it loses its spaces at the end and gets a line
    // ending (whatever the edit is); only spaces, it goes when something is added at the end
    let tail = parsed.text.iter().rposition(|&u| u == LF).map_or(0, |i| i + 1);
    let spaces_tail = parsed.text[tail..].iter().all(|&u| is_space(u));
    if !spaces_tail {
        let kept = parsed.text.iter().rposition(|&u| !is_space(u)).map_or(0, |i| i + 1);
        parsed.text.truncate(kept);
        parsed.text.extend([CR, LF]);
    }
    parsed.lines = lines(&parsed.text);
    let edit = match parsed.section(section) {
        Some(found) => match key {
            None => Some((parsed.text_start(found.0), parsed.section_end(found), Vec::new())),
            Some(key) => match (parsed.key(found, key), value) {
                (Some((line, _)), None) => Some((parsed.text_start(line), parsed.lines[line].next, Vec::new())),
                (Some((line, equals)), Some(value)) => {
                    let end = parsed.lines[line].end;
                    let stop = (equals + 1..end).rev().find(|&i| !is_space(parsed.text[i])).map_or(equals + 1, |i| i + 1);
                    Some((equals + 1, stop, value.to_vec()))
                }
                (None, None) => None,
                (None, Some(value)) => {
                    // after the section's last key (or its name); in the file's last section at the
                    // very end when the file's last line is blank
                    let at = if found.1 == parsed.lines.len() && parsed.blank_end() {
                        parsed.text.len()
                    } else {
                        parsed.lines[parsed.keys(found).last().map_or(found.0, |(i, _, _)| i)].next
                    };
                    Some((at, at, line_of(key, value)))
                }
            },
        },
        None => match (key, value) {
            (Some(key), Some(value)) => {
                let mut add: Vec<u16> = vec![b'[' as u16];
                add.extend_from_slice(trim_blanks(section));
                add.extend([b']' as u16, CR, LF]);
                add.extend(line_of(key, value));
                Some((parsed.text.len(), parsed.text.len(), add))
            }
            _ => None,
        },
    };
    let mut text = parsed.text;
    if let Some((from, to, with)) = edit {
        let from = if spaces_tail && from == text.len() { tail } else { from };
        text.splice(from..to, with);
    }
    let bytes = wide_to_ansi(&text);
    (file != Some(&bytes[..])).then_some(bytes)
}
