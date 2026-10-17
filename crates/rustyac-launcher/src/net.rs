// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa's launcher (AssettoCorsa.exe, Kunos Simulazioni).

//! The few .NET Framework string and file calls the launcher's code leans on, with their
//! .NET behaviour: `String.Trim`, `ToUpper` / `ToLowerInvariant`, `File.ReadAllText` (byte
//! order marks) and `File.WriteAllText` (with or without one).

use crate::files;

/// `String.Trim()`: .NET's white space is Rust's.
pub fn trim(s: &str) -> &str {
    s.trim()
}

fn map_case(s: &str, upper: bool) -> String {
    s.chars()
        .map(|c| {
            let mut m: Vec<char> = if upper { c.to_uppercase().collect() } else { c.to_lowercase().collect() };
            // .NET maps one UTF-16 unit to one: a character that would grow stays as it is
            if m.len() == 1 && (m[0] as u32 >= 0x10000) == (c as u32 >= 0x10000) {
                m.pop().unwrap()
            } else {
                c
            }
        })
        .collect()
}

/// `String.ToUpper()` / `ToUpperInvariant()`.
pub fn to_upper(s: &str) -> String {
    map_case(s, true)
}

/// `String.ToLower()` / `ToLowerInvariant()`.
pub fn to_lower(s: &str) -> String {
    map_case(s, false)
}

/// Text read the way `StreamReader` does with UTF-8: a UTF-8, UTF-16 or UTF-32 byte order mark
/// says the encoding and is dropped, bad bytes become U+FFFD.
pub fn decode(bytes: &[u8]) -> String {
    if let Some(rest) = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]) {
        return String::from_utf8_lossy(rest).into_owned();
    }
    if bytes.starts_with(&[0xff, 0xfe, 0, 0]) || bytes.starts_with(&[0, 0, 0xfe, 0xff]) {
        let le = bytes[0] == 0xff;
        return bytes[4..]
            .chunks(4)
            .map(|c| {
                let mut b = [0u8; 4];
                b[..c.len()].copy_from_slice(c);
                let v = if le { u32::from_le_bytes(b) } else { u32::from_be_bytes(b) };
                char::from_u32(v).unwrap_or('\u{fffd}')
            })
            .collect();
    }
    if bytes.starts_with(&[0xff, 0xfe]) || bytes.starts_with(&[0xfe, 0xff]) {
        let le = bytes[0] == 0xff;
        let units: Vec<u16> = bytes[2..]
            .chunks(2)
            .map(|c| if c.len() < 2 { 0xfffd } else if le { u16::from_le_bytes([c[0], c[1]]) } else { u16::from_be_bytes([c[0], c[1]]) })
            .collect();
        return String::from_utf16_lossy(&units);
    }
    String::from_utf8_lossy(bytes).into_owned()
}

/// `File.ReadAllText(path)`.
pub fn read_all_text(path: &str) -> std::io::Result<String> {
    Ok(decode(&files::with(|f| f.read(path))?))
}

/// `File.WriteAllText(path, text)`: UTF-8 without a byte order mark.
pub fn write_all_text(path: &str, text: &str) -> std::io::Result<()> {
    files::with(|f| f.write(path, text.as_bytes()))
}

/// `File.WriteAllText(path, text, Encoding.UTF8)`: UTF-8 behind a byte order mark, which
/// .NET writes even for no text.
pub fn write_all_text_bom(path: &str, text: &str) -> std::io::Result<()> {
    let mut bytes = vec![0xef, 0xbb, 0xbf];
    bytes.extend_from_slice(text.as_bytes());
    files::with(|f| f.write(path, &bytes))
}

/// `File.Exists`.
pub fn file_exists(path: &str) -> bool {
    files::with(|f| f.is_file(path))
}

/// `Directory.Exists`.
pub fn dir_exists(path: &str) -> bool {
    files::with(|f| f.is_dir(path))
}

/// `Path.GetFileName`: what follows the last `\` or `/`.
pub fn file_name(path: &str) -> &str {
    match path.rfind(['\\', '/']) {
        Some(i) => &path[i + 1..],
        None => path,
    }
}

/// A path as .NET's exceptions name it (`Path.GetFullPath`): `/` as `\`, a run of `\` as one,
/// none at the end.
pub fn full_path(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    for c in path.chars().map(|c| if c == '/' { '\\' } else { c }) {
        if !(c == '\\' && out.ends_with('\\')) {
            out.push(c);
        }
    }
    out.trim_end_matches('\\').to_string()
}

/// `Path.GetFileNameWithoutExtension`.
pub fn file_stem(path: &str) -> &str {
    let name = file_name(path);
    match name.rfind('.') {
        Some(i) => &name[..i],
        None => name,
    }
}

/// The entries of a folder in the order Windows lists them (`FindFirstFile`, as
/// `Directory.GetDirectories` / `GetFiles` do), as full paths `folder` + `\` + name.
/// `dirs`: folders, else files.
pub fn list(folder: &str, dirs: bool) -> std::io::Result<Vec<String>> {
    let mut out = Vec::new();
    let base = if folder.ends_with('\\') || folder.ends_with('/') { folder.to_string() } else { format!("{folder}\\") };
    for (name, is_dir) in files::with(|f| f.list(folder))? {
        if is_dir == dirs {
            out.push(format!("{base}{name}"));
        }
    }
    Ok(out)
}

/// `Directory.GetFiles(folder, "*.ext")`: names ending with the extension, ignoring case (the
/// three-letter-extension quirk of the Win32 pattern does not matter for the launcher's names).
pub fn files_with(folder: &str, suffix: &str) -> std::io::Result<Vec<String>> {
    let suffix = to_lower(suffix);
    Ok(list(folder, false)?.into_iter().filter(|f| to_lower(file_name(f)).ends_with(&suffix)).collect())
}

/// `new Uri(path.Replace("\\", "/")).AbsoluteUri` for a full Windows path.
pub fn uri_path(path: &str) -> String {
    let p = path.replace('\\', "/");
    let mut out = String::from("file:///");
    for b in p.bytes() {
        let keep = b.is_ascii_alphanumeric() || b"-._~!$&'()*+,;=:@/".contains(&b);
        if keep {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// `File.Copy(from, to, true)`.
pub fn copy(from: &str, to: &str) -> std::io::Result<()> {
    files::with(|f| f.copy(from, to))
}

/// `File.Delete`.
pub fn remove_file(path: &str) -> std::io::Result<()> {
    files::with(|f| f.remove_file(path))
}

/// `Directory.Delete(path, true)`.
pub fn remove_dir_all(path: &str) -> std::io::Result<()> {
    files::with(|f| f.remove_dir_all(path))
}

/// `Directory.CreateDirectory`.
pub fn create_dir_all(path: &str) -> std::io::Result<()> {
    files::with(|f| f.create_dir_all(path))
}

/// `File.Move(from, to)`: never over a file that is there.
pub fn rename(from: &str, to: &str) -> std::io::Result<()> {
    files::with(|f| f.rename(from, to))
}

/// `File.AppendAllText` of bytes.
pub fn append(path: &str, bytes: &[u8]) -> std::io::Result<()> {
    files::with(|f| f.append(path, bytes))
}

/// `FileInfo.Length` (0 when there is no such file).
pub fn len(path: &str) -> u64 {
    files::with(|f| f.len(path)).unwrap_or(0)
}

/// `File.GetLastWriteTime` / `GetCreationTime` (the epoch when there is no such file).
pub fn modified(path: &str) -> std::time::SystemTime {
    files::with(|f| f.modified(path)).unwrap_or(std::time::UNIX_EPOCH)
}

pub fn created(path: &str) -> std::time::SystemTime {
    files::with(|f| f.created(path)).unwrap_or(std::time::UNIX_EPOCH)
}
