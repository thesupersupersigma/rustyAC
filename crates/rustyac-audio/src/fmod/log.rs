// SPDX-License-Identifier: GPL-3.0-or-later

//! The FMOD call log: one text line per API call, with every argument (floats as raw bits) and
//! the objects FMOD hands out replaced by numbers in the order they first appear. The same layer
//! sits under the port and under acs.exe's own sound code in `tools/audio_oracle`, so two logs
//! are equal exactly when the two programs made the same calls with the same argument bits.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::io::Write as _;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

pub struct CallLog {
    out: Option<std::io::BufWriter<std::fs::File>>,
    ids: HashMap<usize, u32>,
    next_id: u32,
    lines: u64,
    hash: u64,
    /// running hashes at the marks, for golden tests: (label, lines so far, hash so far)
    marks: Vec<(String, u64, u64)>,
    keep_marks: bool,
}

pub(crate) static LOG: Mutex<Option<CallLog>> = Mutex::new(None);
/// Is a log running? (So that a call without a log costs one load.)
pub(crate) static LOGGING: AtomicBool = AtomicBool::new(false);

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// What a finished log was: how many lines, and the FNV-1a hash of all its bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LogSummary {
    pub lines: u64,
    pub hash: u64,
    pub marks: Vec<(String, u64, u64)>,
}

/// Starts logging every FMOD call, to `file` when one is given (the hash is kept either way).
pub fn start(file: Option<&Path>) -> Result<(), String> {
    let out = match file {
        Some(path) => Some(std::io::BufWriter::with_capacity(
            1 << 20,
            std::fs::File::create(path).map_err(|e| format!("{}: {e}", path.display()))?,
        )),
        None => None,
    };
    *LOG.lock().unwrap() = Some(CallLog {
        out,
        ids: HashMap::new(),
        next_id: 0,
        lines: 0,
        hash: FNV_OFFSET,
        marks: Vec::new(),
        keep_marks: true,
    });
    LOGGING.store(true, Ordering::Relaxed);
    Ok(())
}

/// Is a log running?
pub fn active() -> bool {
    LOG.lock().unwrap().is_some()
}

/// A line of the caller's own in the log (frame numbers and the like). Both sides of a
/// comparison must write the same marks.
pub fn mark(text: &str) {
    if let Some(log) = LOG.lock().unwrap().as_mut() {
        let line = format!("# {text}");
        log.push(&line);
        if log.keep_marks {
            log.marks.push((text.to_string(), log.lines, log.hash));
        }
    }
}

/// Ends the log and says what it was.
pub fn finish() -> Option<LogSummary> {
    LOGGING.store(false, Ordering::Relaxed);
    let mut log = LOG.lock().unwrap().take()?;
    if let Some(out) = log.out.as_mut() {
        let _ = out.flush();
    }
    Some(LogSummary { lines: log.lines, hash: log.hash, marks: log.marks })
}

impl CallLog {
    pub(crate) fn push(&mut self, line: &str) {
        for &b in line.as_bytes().iter().chain(b"\n") {
            self.hash = (self.hash ^ b as u64).wrapping_mul(FNV_PRIME);
        }
        self.lines += 1;
        if let Some(out) = self.out.as_mut() {
            let _ = out.write_all(line.as_bytes());
            let _ = out.write_all(b"\n");
        }
    }

    /// The number of an FMOD object: given at its first appearance.
    pub(crate) fn id(&mut self, pointer: usize) -> String {
        if pointer == 0 {
            return "h-".to_string();
        }
        let next = &mut self.next_id;
        let id = *self.ids.entry(pointer).or_insert_with(|| {
            *next += 1;
            *next
        });
        format!("h{id}")
    }

    /// A new number for `pointer` whatever it was before (objects whose end is not seen).
    pub(crate) fn fresh_id(&mut self, pointer: usize) -> String {
        self.ids.remove(&pointer);
        self.id(pointer)
    }

    /// The object is gone: the same address may come back as another object.
    pub(crate) fn forget(&mut self, pointer: usize) {
        self.ids.remove(&pointer);
    }
}

/// Raw bytes as hex, in 4-byte groups (a float reads as its bit pattern, little-endian word).
pub(crate) fn hex_words(text: &mut String, bytes: &[u8]) {
    for (i, chunk) in bytes.chunks(4).enumerate() {
        if i > 0 {
            text.push('_');
        }
        if chunk.len() == 4 {
            let _ = write!(text, "{:08x}", u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]));
        } else {
            for b in chunk {
                let _ = write!(text, "{b:02x}");
            }
        }
    }
}
