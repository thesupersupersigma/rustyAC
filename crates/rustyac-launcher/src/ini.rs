// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa's launcher (AssettoCorsa.exe, Kunos Simulazioni).

//! `AC.Launcher.Utility.IniFile`: the same Win32 calls (`GetPrivateProfileStringW`,
//! `WritePrivateProfileStringW`) with the same detour through the ANSI code page, so the files
//! come out byte for byte as the launcher writes them. The launcher turns a name or value into
//! UTF-8 bytes and reads those bytes as ANSI before it writes; Windows then writes the ANSI
//! file back as those bytes, which is UTF-8 again. Reading undoes it.

use windows::core::{HSTRING, PCWSTR};
use windows::Win32::Foundation::GetLastError;
use windows::Win32::Globalization::{MultiByteToWideChar, WideCharToMultiByte, CP_ACP, MULTI_BYTE_TO_WIDE_CHAR_FLAGS};
use windows::Win32::System::WindowsProgramming::{GetPrivateProfileStringW, WritePrivateProfileStringW};

use crate::bridge::Thrown;
use crate::net;

pub struct IniFile {
    pub path: String,
}

/// `Encoding.Default.GetString(Encoding.UTF8.GetBytes(s))`.
fn utf8_as_ansi(s: &str) -> Vec<u16> {
    let bytes = s.as_bytes();
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

/// `Encoding.Default.GetBytes(s)`.
fn ansi_bytes(units: &[u16]) -> Vec<u8> {
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

fn wide(units: Option<Vec<u16>>) -> Option<Vec<u16>> {
    units.map(|mut u| {
        u.push(0);
        u
    })
}

fn ptr(w: &Option<Vec<u16>>) -> PCWSTR {
    match w {
        Some(w) => PCWSTR(w.as_ptr()),
        None => PCWSTR::null(),
    }
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

    /// `GetPrivateProfileStringW` into a .NET string of 65535 spaces, as the launcher passes it.
    fn get(&self, section: &Option<Vec<u16>>, key: &Option<Vec<u16>>, default: Option<&[u16]>, size: u32) -> Vec<u16> {
        let mut buf = vec![0x20u16; 65536];
        buf[65535] = 0;
        let default = default.map(|d| {
            let mut d = d.to_vec();
            d.push(0);
            d
        });
        let path = HSTRING::from(self.path.as_str());
        unsafe {
            GetPrivateProfileStringW(ptr(section), ptr(key), ptr(&default), Some(&mut buf[..size as usize]), &path);
        }
        buf.truncate(65535);
        buf
    }

    /// `IniWriteValue(Section, Key, Value)`; `None` deletes, as Win32 does.
    pub fn write(&self, section: Option<&str>, key: Option<&str>, value: Option<&str>) -> Result<(), Thrown> {
        let key = key.map(net::to_upper);
        let section = section.map(net::to_upper);
        let value = wide(value.map(utf8_as_ansi));
        let key = wide(key.as_deref().map(utf8_as_ansi));
        let section = wide(section.as_deref().map(utf8_as_ansi));
        let path = HSTRING::from(self.path.as_str());
        let ok = unsafe { WritePrivateProfileStringW(ptr(&section), ptr(&key), ptr(&value), &path) };
        if ok.is_err() {
            let code = unsafe { GetLastError() }.0;
            return Err(Thrown::Other(format!("Cannot write to:\n{}\n\n Error code: {code}", self.path)));
        }
        Ok(())
    }

    /// `IniReadValue(Section, Key)`.
    pub fn read(&self, section: Option<&str>, key: Option<&str>) -> String {
        let key = wide(key.map(utf8_as_ansi));
        let section = wide(section.map(utf8_as_ansi));
        let text = self.get(&section, &key, Some(&[]), 65535);
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
        let mut c: Vec<u16> = category.encode_utf16().collect();
        c.push(0);
        let text = self.get(&Some(c), &None, None, 32768);
        IniFile::split_names(&text)
    }

    /// `GetCategories()`.
    pub fn categories(&self) -> Result<Vec<String>, Thrown> {
        let text = self.get(&None, &None, None, 65535);
        IniFile::split_names(&text)
    }
}
