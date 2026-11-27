// SPDX-License-Identifier: GPL-3.0-or-later

//! Assetto Corsa's launcher (`AssettoCorsa.exe`, a .NET WPF program with CEF 3 inside), ported:
//! its C# side in Rust, its HTML theme (the player's own, from the game's folder) shown in
//! WebView2 and answered through the same `ac://` bridge.
//!
//! * [`bridge`]: `ACSchemeHandler` / `ACRequestHandler`, every request the theme makes.
//! * [`setup`]: `Startup.SetupUserDirectory`, the player's folder before the theme starts.
//! * [`ini`], [`appini`]: `IniFile` (the Win32 profile functions) and `AppIniFile`.
//! * [`jss`], [`query`], [`net`], [`sort`]: the .NET pieces the answers depend on, byte for
//!   byte (`JavaScriptSerializer`, `HttpUtility`, string and file helpers, `Array.Sort`).
//! * [`online`]: the server list, a server's details, ping and the LAN scan.
//! * [`pbs`]: `--import-pbs`, the personal bests of an Assetto Corsa install.
//! * [`replays`]: the replays page's files.
//! * [`shell`]: `MainWindow`, the window with the browser in it, and starting rustyac.exe
//!   (WebView2 on Windows; WebKitGTK through wry on Linux, with the game-files chooser).
//! * [`page`]: the window's pieces that do not depend on its browser.
//! * [`transcript`]: the answers to a list of requests, as the launcher oracle writes them.
//!
//! The player's folder (`AppFolder.UserDataPath`, `Documents\Assetto Corsa` in the game) is
//! the folder of rustyac.exe, so the files the theme writes are rustyAC's own cfg files.
//! See docs/port/launcher.md.

pub mod appini;
pub mod bridge;
pub mod files;
pub mod folders;
pub mod ini;
pub mod jss;
pub mod net;
pub mod online;
pub mod page;
pub mod pbs;
pub mod profile;
pub mod query;
pub mod replays;
pub mod setup;
#[cfg(windows)]
pub mod shell;
/// Off Windows: the same window on WebKitGTK (wry), with the game-files chooser.
#[cfg(target_os = "linux")]
#[path = "shell_gtk.rs"]
pub mod shell;
pub mod sort;
#[cfg(windows)]
pub mod sys;
#[cfg(target_os = "linux")]
#[path = "sys_unix.rs"]
pub mod sys;
pub mod transcript;
