// SPDX-License-Identifier: GPL-3.0-or-later

//! NOT Assetto Corsa: a log of the kgl calls themselves, one line a call, the same for both kgl
//! backends (Direct3D 11 in `kgl.rs`, wgpu in `kgl_wgpu.rs`). Objects are named by the order they
//! are made in (`rt3`, `cb12`, ...), textures by their file, data by its FNV-1a hash, so the log of
//! one frame drawn by the two backends is the same text when the renderer asked both the same.
//! (`gpulog` logs the Direct3D 11 context below kgl; this one is above it.)

use std::collections::HashMap;
use std::fmt::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

struct Log {
    names: HashMap<usize, String>,
    counters: HashMap<&'static str, u32>,
    text: Option<String>,
}

static ON: AtomicBool = AtomicBool::new(false);
static LOG: Mutex<Option<Log>> = Mutex::new(None);

fn with<R>(f: impl FnOnce(&mut Log) -> R) -> R {
    let mut guard = LOG.lock().unwrap_or_else(|e| e.into_inner());
    f(guard.get_or_insert_with(|| Log { names: HashMap::new(), counters: HashMap::new(), text: None }))
}

/// Starts keeping lines (names are given out whether or not a capture runs).
pub fn begin(title: &str) {
    with(|l| l.text = Some(format!("# kgl log: {title}\n")));
    ON.store(true, Ordering::Relaxed);
}

/// The lines since [`begin`]; the capture stops.
pub fn end() -> String {
    ON.store(false, Ordering::Relaxed);
    with(|l| l.text.take().unwrap_or_default())
}

pub fn on() -> bool {
    ON.load(Ordering::Relaxed)
}

/// One line, when a capture runs.
pub fn line(args: std::fmt::Arguments) {
    if on() {
        with(|l| {
            if let Some(text) = &mut l.text {
                let _ = text.write_fmt(args);
                text.push('\n');
            }
        });
    }
}

/// Gives the object at `key` the next name of its kind (`prefix` and a number).
pub fn new_name(key: usize, prefix: &'static str) -> String {
    with(|l| {
        let n = l.counters.entry(prefix).or_insert(0);
        *n += 1;
        let name = format!("{prefix}{n}");
        l.names.insert(key, name.clone());
        name
    })
}

/// Gives the object at `key` a name of its own (a texture: its file).
pub fn set_name(key: usize, name: &str) {
    with(|l| l.names.insert(key, name.to_string()));
}

/// The name of the object at `key`: `-` for none, `?` for one no name was given to.
pub fn name(key: Option<usize>) -> String {
    match key {
        None | Some(0) => "-".into(),
        Some(k) => with(|l| l.names.get(&k).cloned().unwrap_or_else(|| "?".into())),
    }
}

/// FNV-1a of bytes, for the data a call carries.
pub fn hash(bytes: &[u8]) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100_0000_01b3);
    }
    h
}

#[cfg(test)]
mod tests {
    #[test]
    fn names_and_lines() {
        let a = super::new_name(0x1000, "zz");
        let b = super::new_name(0x2000, "zz");
        assert_ne!(a, b);
        assert_eq!(super::name(Some(0x2000)), b);
        assert_eq!(super::name(None), "-");
        super::begin("t");
        super::line(format_args!("draw {}", 3));
        let text = super::end();
        assert!(text.ends_with("draw 3\n"), "{text}");
        super::line(format_args!("lost"));
        assert_eq!(super::end(), "");
    }
}
