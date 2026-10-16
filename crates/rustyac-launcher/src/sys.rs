// SPDX-License-Identifier: GPL-3.0-or-later

//! What the launcher asks Windows (and Steam) for: the screen, its modes, the time, a GUID and
//! the player's name, as the bridge's [`Host`].

use windows::core::PCWSTR;
use windows::Win32::Foundation::{FILETIME, SYSTEMTIME};
use windows::Win32::Globalization::{GetLocaleInfoEx, LOCALE_SABBREVMONTHNAME1};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_R8G8B8A8_UNORM, DXGI_MODE_DESC};
use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, IDXGIFactory1, DXGI_ENUM_MODES};
use windows::Win32::Graphics::Gdi::{EnumDisplaySettingsW, DEVMODEW, ENUM_DISPLAY_SETTINGS_MODE};
use windows::Win32::System::Com::CoCreateGuid;
use windows::Win32::System::SystemInformation::GetLocalTime;
use windows::Win32::System::Time::{FileTimeToSystemTime, SystemTimeToTzSpecificLocalTime};
use windows::Win32::UI::HiDpi::GetDpiForSystem;
use windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN};

use crate::bridge::Host;

pub struct Windows {
    /// `GetPersonaName`.
    pub player_name: String,
}

fn fields(t: &SYSTEMTIME) -> [i64; 7] {
    [t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond, t.wMilliseconds].map(i64::from)
}

fn now() -> SYSTEMTIME {
    unsafe { GetLocalTime() }
}

/// `DateTime.Now.ToString("yyyyMMdd-HHmmss")`.
pub fn now_stamp() -> String {
    let t = now();
    format!("{:04}{:02}{:02}-{:02}{:02}{:02}", t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond)
}

/// A file's last write time in local time.
fn file_local_time(file: &str) -> Option<SYSTEMTIME> {
    let modified = std::fs::metadata(file).ok()?.modified().ok()?;
    // 100 ns ticks since 1601
    let since = modified.duration_since(std::time::UNIX_EPOCH).ok()?;
    let ticks = since.as_nanos() as u64 / 100 + 116_444_736_000_000_000;
    let ft = FILETIME { dwLowDateTime: ticks as u32, dwHighDateTime: (ticks >> 32) as u32 };
    let mut utc = SYSTEMTIME::default();
    let mut local = SYSTEMTIME::default();
    unsafe {
        FileTimeToSystemTime(&ft, &mut utc).ok()?;
        SystemTimeToTzSpecificLocalTime(None, &utc, &mut local).ok()?;
    }
    Some(local)
}

impl Host for Windows {
    fn primary_resolution(&self) -> (f64, f64) {
        let scale = unsafe { GetDpiForSystem() } as f64 / 96.0;
        let (w, h) = unsafe { (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN)) };
        (w as f64 / scale, h as f64 / scale)
    }

    fn display_modes(&self) -> Vec<(u32, u32)> {
        let mut out = Vec::new();
        let mut mode = DEVMODEW { dmSize: std::mem::size_of::<DEVMODEW>() as u16, ..Default::default() };
        let mut i = 0;
        while unsafe { EnumDisplaySettingsW(PCWSTR::null(), ENUM_DISPLAY_SETTINGS_MODE(i), &mut mode) }.as_bool() {
            out.push((mode.dmPelsWidth, mode.dmPelsHeight));
            i += 1;
        }
        out
    }

    fn video_modes(&self) -> Option<Vec<(i32, i32, f32)>> {
        // acVideoModes.dll (32-bit, so a 64-bit launcher cannot load it): the first output of
        // the first adapter
        unsafe {
            let factory: IDXGIFactory1 = CreateDXGIFactory1().ok()?;
            let output = factory.EnumAdapters1(0).ok()?.EnumOutputs(0).ok()?;
            let mut n = 0u32;
            output.GetDisplayModeList(DXGI_FORMAT_R8G8B8A8_UNORM, DXGI_ENUM_MODES(0), &mut n, None).ok()?;
            let mut modes = vec![DXGI_MODE_DESC::default(); n as usize];
            output.GetDisplayModeList(DXGI_FORMAT_R8G8B8A8_UNORM, DXGI_ENUM_MODES(0), &mut n, Some(modes.as_mut_ptr())).ok()?;
            modes.truncate(n as usize);
            Some(
                modes
                    .iter()
                    .map(|m| {
                        let r = &m.RefreshRate;
                        (m.Width as i32, m.Height as i32, if r.Denominator == 0 { 0.0 } else { r.Numerator as f32 / r.Denominator as f32 })
                    })
                    .collect(),
            )
        }
    }

    fn local_time(&self, file: Option<&str>) -> [i64; 7] {
        fields(&file.and_then(file_local_time).unwrap_or_else(now))
    }

    fn month_abbrev(&self, month: u32) -> String {
        let mut buf = [0u16; 80];
        let n = unsafe { GetLocaleInfoEx(PCWSTR::null(), LOCALE_SABBREVMONTHNAME1 + month - 1, Some(&mut buf)) };
        String::from_utf16_lossy(&buf[..(n.max(1) - 1) as usize])
    }

    fn player_name(&self) -> String {
        self.player_name.clone()
    }

    fn new_guid(&self) -> String {
        let g = unsafe { CoCreateGuid() }.unwrap_or_default();
        let d4 = g.data4;
        format!(
            "{:08x}-{:04x}-{:04x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
            g.data1, g.data2, g.data3, d4[0], d4[1], d4[2], d4[3], d4[4], d4[5], d4[6], d4[7]
        )
    }
}
