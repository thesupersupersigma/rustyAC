// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa's launcher (AssettoCorsa.exe, Kunos Simulazioni).

//! `AC.Launcher.Utility.IniFile`: the same Win32 calls (`GetPrivateProfileStringW`,
//! `WritePrivateProfileStringW`) with the same detour through the ANSI code page, so the files
//! come out byte for byte as the launcher writes them. The launcher turns a name or value into
//! UTF-8 bytes and reads those bytes as ANSI before it writes; Windows then writes the ANSI
//! file back as those bytes, which is UTF-8 again. Reading undoes it.
//!
//! Where there is no Win32 (the browser) the same calls are [`profile`]'s pure copy of them, on
//! the mounted [`files`](crate::files); on Windows [`use_pure`] switches to it too, for checks.

use std::sync::atomic::{AtomicBool, Ordering};

use crate::bridge::Thrown;
use crate::{net, profile};

pub struct IniFile {
    pub path: String,
}

static PURE: AtomicBool = AtomicBool::new(!cfg!(windows));

/// rustyAC's own: the pure copy instead of Win32 (always so where there is no Win32).
pub fn use_pure(on: bool) {
    PURE.store(on || !cfg!(windows), Ordering::Relaxed);
}

fn pure() -> bool {
    PURE.load(Ordering::Relaxed)
}

#[cfg(windows)]
mod win {
    use windows::core::{HSTRING, PCWSTR};
    use windows::Win32::Foundation::GetLastError;
    use windows::Win32::Globalization::{MultiByteToWideChar, WideCharToMultiByte, CP_ACP, MULTI_BYTE_TO_WIDE_CHAR_FLAGS};
    use windows::Win32::System::WindowsProgramming::{GetPrivateProfileStringW, WritePrivateProfileStringW};

    pub fn utf8_as_ansi(bytes: &[u8]) -> Vec<u16> {
        if bytes.is_empty() {
            return Vec::new();
        }
        unsafe {
            let n = MultiByteToWideChar(CP_ACP, MULTI_BYTE_TO_WIDE_CHAR_FLAGS(0), bytes, None);
            let mut out = vec![0u16; n as usize];
            MultiByteToWideChar(CP_ACP, MULTI_BYTE_TO_WIDE_CHAR_FLAGS(0), bytes, Some(&mut out));
            out
        }
    }

    pub fn ansi_bytes(units: &[u16]) -> Vec<u8> {
        if units.is_empty() {
            return Vec::new();
        }
        unsafe {
            let n = WideCharToMultiByte(CP_ACP, 0, units, None, None, None);
            let mut out = vec![0u8; n as usize];
            WideCharToMultiByte(CP_ACP, 0, units, Some(&mut out), None, None);
            out
        }
    }

    fn wide(units: Option<&[u16]>) -> Option<Vec<u16>> {
        units.map(|u| u.iter().copied().chain([0]).collect())
    }

    fn ptr(w: &Option<Vec<u16>>) -> PCWSTR {
        w.as_ref().map_or(PCWSTR::null(), |w| PCWSTR(w.as_ptr()))
    }

    pub fn get(path: &str, section: Option<&[u16]>, key: Option<&[u16]>, default: Option<&[u16]>, buf: &mut [u16]) {
        let (s, k, d) = (wide(section), wide(key), wide(default));
        unsafe {
            GetPrivateProfileStringW(ptr(&s), ptr(&k), ptr(&d), Some(buf), &HSTRING::from(path));
        }
    }

    pub fn write(path: &str, section: Option<&[u16]>, key: Option<&[u16]>, value: Option<&[u16]>) -> Result<(), u32> {
        let (s, k, v) = (wide(section), wide(key), wide(value));
        let ok = unsafe { WritePrivateProfileStringW(ptr(&s), ptr(&k), ptr(&v), &HSTRING::from(path)) };
        ok.map_err(|_| unsafe { GetLastError() }.0)
    }
}

/// `Encoding.Default.GetString(Encoding.UTF8.GetBytes(s))`.
fn utf8_as_ansi(s: &str) -> Vec<u16> {
    #[cfg(windows)]
    if !pure() {
        return win::utf8_as_ansi(s.as_bytes());
    }
    profile::ansi_to_wide(s.as_bytes())
}

/// `Encoding.Default.GetBytes(s)`.
fn ansi_bytes(units: &[u16]) -> Vec<u8> {
    #[cfg(windows)]
    if !pure() {
        return win::ansi_bytes(units);
    }
    profile::wide_to_ansi(units)
}

/// `String.Trim` on UTF-16 units.
fn trim_units(u: &[u16]) -> &[u16] {
    let ws = |x: &u16| char::from_u32(*x as u32).is_some_and(char::is_whitespace);
    let start = u.iter().position(|x| !ws(x)).unwrap_or(u.len());
    let end = u.iter().rposition(|x| !ws(x)).map_or(start, |e| e + 1);
    &u[start..end.max(start)]
}

impl IniFile {
    pub fn new(path: impl Into<String>) -> IniFile {
        IniFile { path: path.into() }
    }

    fn bytes(&self) -> Option<Vec<u8>> {
        crate::files::with(|f| f.read(&self.path)).ok()
    }

    /// `GetPrivateProfileStringW` into a .NET string of 65535 spaces, as the launcher passes it.
    fn get(&self, section: Option<&[u16]>, key: Option<&[u16]>, default: Option<&[u16]>, size: u32) -> Vec<u16> {
        let mut buf = vec![0x20u16; 65536];
        buf[65535] = 0;
        let size = size as usize;
        if pure() {
            let (written, _) = profile::get_string(self.bytes().as_deref(), section, key, default, size);
            buf[..written.len()].copy_from_slice(&written);
        } else {
            #[cfg(windows)]
            win::get(&self.path, section, key, default, &mut buf[..size]);
        }
        buf.truncate(65535);
        buf
    }

    /// `IniWriteValue(Section, Key, Value)`; `None` deletes, as Win32 does.
    pub fn write(&self, section: Option<&str>, key: Option<&str>, value: Option<&str>) -> Result<(), Thrown> {
        let key = key.map(net::to_upper);
        let section = section.map(net::to_upper);
        let value = value.map(utf8_as_ansi);
        let key = key.as_deref().map(utf8_as_ansi);
        let section = section.as_deref().map(utf8_as_ansi);
        let result = if pure() {
            match section {
                // Win32 only flushes its cache
                None => Ok(()),
                Some(section) => match profile::write_string(self.bytes().as_deref(), &section, key.as_deref(), value.as_deref()) {
                    None => Ok(()),
                    // ERROR_PATH_NOT_FOUND, ERROR_ACCESS_DENIED as Windows answers
                    Some(bytes) => crate::files::with(|f| f.write(&self.path, &bytes)).map_err(|e| if e.kind() == std::io::ErrorKind::NotFound { 3 } else { 5 }),
                },
            }
        } else {
            #[cfg(windows)]
            {
                win::write(&self.path, section.as_deref(), key.as_deref(), value.as_deref())
            }
            #[cfg(not(windows))]
            unreachable!()
        };
        result.map_err(|code| Thrown::Other(format!("Cannot write to:\n{}\n\n Error code: {code}", self.path)))
    }

    /// `IniReadValue(Section, Key)`.
    pub fn read(&self, section: Option<&str>, key: Option<&str>) -> String {
        let key = key.map(utf8_as_ansi);
        let section = section.map(utf8_as_ansi);
        let text = self.get(section.as_deref(), key.as_deref(), Some(&[]), 65535);
        let bytes = ansi_bytes(trim_units(&text));
        let text = String::from_utf8_lossy(&bytes).replace('\0', "");
        let mut text = net::trim(&text).to_string();
        if let Some(i) = text.find(';') {
            text = net::trim(&text[..i]).to_string();
        }
        text
    }

    /// `IniReadValue(Section, Key, DefaultValue)`.
    pub fn read_or(&self, section: &str, key: &str, default: &str) -> String {
        let v = self.read(Some(section), Some(key));
        if v.is_empty() {
            default.to_string()
        } else {
            v
        }
    }

    fn split_names(text: &[u16]) -> Result<Vec<String>, Thrown> {
        let text = String::from_utf16_lossy(trim_units(text));
        let text = net::to_upper(&text);
        let mut list: Vec<String> = text.split('\0').map(str::to_string).collect();
        // List.RemoveRange(Count - 2, 2)
        if list.len() < 2 {
            return Err(Thrown::ArgumentOutOfRange);
        }
        list.truncate(list.len() - 2);
        Ok(list)
    }

    /// `GetKeys(category)`.
    pub fn keys(&self, category: &str) -> Result<Vec<String>, Thrown> {
        let c: Vec<u16> = category.encode_utf16().collect();
        let text = self.get(Some(&c), None, None, 32768);
        IniFile::split_names(&text)
    }

    /// `GetCategories()`.
    pub fn categories(&self) -> Result<Vec<String>, Thrown> {
        let text = self.get(None, None, None, 65535);
        IniFile::split_names(&text)
    }
}
