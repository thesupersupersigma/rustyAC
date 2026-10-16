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
//! * [`shell`]: `MainWindow`, the window with the browser in it, and starting rustyac.exe.
//! * [`transcript`]: the answers to a list of requests, as the launcher oracle writes them.
//!
//! The player's folder (`AppFolder.UserDataPath`, `Documents\Assetto Corsa` in the game) is
//! the folder of rustyac.exe, so the files the theme writes are rustyAC's own cfg files.
//! See docs/port/launcher.md.

#![cfg(windows)]

pub mod appini;
pub mod bridge;
pub mod folders;
pub mod ini;
pub mod jss;
pub mod net;
pub mod online;
pub mod pbs;
pub mod query;
pub mod replays;
pub mod setup;
pub mod shell;
pub mod sort;
pub mod sys;
pub mod transcript;
