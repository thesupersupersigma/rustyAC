// SPDX-License-Identifier: GPL-3.0-or-later

//! What the launcher asks the system for off Windows (GDK for the screen, GLib for the local
//! time), as the bridge's [`Host`]. Call `gtk::init` first.

use gtk::gdk;
use gtk::glib;
use gtk::prelude::*;

use crate::bridge::Host;

pub struct Unix {
    /// `GetPersonaName`.
    pub player_name: String,
}

/// Each monitor: its size in pixels and its refresh rate.
fn monitors() -> Vec<(i32, i32, f32)> {
    let Some(display) = gdk::Display::default() else { return Vec::new() };
    (0..display.n_monitors())
        .filter_map(|i| display.monitor(i))
        .map(|m| {
            let (g, s) = (m.geometry(), m.scale_factor());
            (g.width() * s, g.height() * s, m.refresh_rate() as f32 / 1000.0)
        })
        .collect()
}

fn fields(t: &glib::DateTime) -> [i64; 7] {
    [t.year(), t.month(), t.day_of_month(), t.hour(), t.minute(), t.second(), t.microsecond() / 1000].map(i64::from)
}

impl Host for Unix {
    fn primary_resolution(&self) -> (f64, f64) {
        let display = gdk::Display::default();
        let monitor = display.as_ref().and_then(|d| d.primary_monitor().or_else(|| d.monitor(0)));
        monitor.map_or((1920.0, 1080.0), |m| (m.geometry().width() as f64, m.geometry().height() as f64))
    }

    fn display_modes(&self) -> Vec<(u32, u32)> {
        // the proofs' fixed list, as on Windows (tools/proofs/launcher.sh)
        if let Ok(list) = std::env::var("RUSTYAC_DISPLAY_MODES") {
            return list.split(',').filter_map(|m| m.split_once('x')).map(|(w, h)| (w.parse().unwrap_or(0), h.parse().unwrap_or(0))).collect();
        }
        // shortcut: GDK names each monitor's current size only, not every mode; when the game's own list is needed, ask SDL as the game does
        monitors().into_iter().map(|(w, h, _)| (w as u32, h as u32)).collect()
    }

    fn video_modes(&self) -> Option<Vec<(i32, i32, f32)>> {
        Some(monitors()).filter(|m| !m.is_empty())
    }

    fn local_time(&self, file: Option<&str>) -> [i64; 7] {
        let modified = file.and_then(|f| std::fs::metadata(f.replace('\\', "/")).and_then(|m| m.modified()).ok());
        let at = modified.and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).and_then(|d| glib::DateTime::from_unix_local(d.as_secs() as i64).ok()?.add(glib::TimeSpan::from_microseconds(i64::from(d.subsec_micros()))).ok());
        fields(&at.or_else(|| glib::DateTime::now_local().ok()).expect("the local time"))
    }

    fn month_abbrev(&self, month: u32) -> String {
        glib::DateTime::from_local(2000, month as i32, 1, 0, 0, 0.0).ok().and_then(|t| t.format("%b").ok()).map(|s| s.to_string()).unwrap_or_default()
    }

    fn player_name(&self) -> String {
        self.player_name.clone()
    }

    fn player_id(&self) -> String {
        // rustyac's own rule (it sits beside the launcher, so rustyac_net.ini is the same file)
        match rustyac_game::identity::resolve(None, true, rustyac_content::install::ac_root().as_deref(), None, &std::env::current_exe().ok().and_then(|exe| exe.parent().map(|f| f.to_path_buf())).unwrap_or_default()) {
            Ok(id) => id.guid,
            // what the launcher answers without Steam: a new GUID each time
            Err(_) => glib::uuid_string_random().to_string(),
        }
    }
}
