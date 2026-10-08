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
    /// Another run's log, whose answers to the state queries are given to this run's caller
    /// (see [`start_scripted`]).
    script: Option<Vec<String>>,
    /// Or just those answers, in the order of the questions (see [`start_with_answers`]).
    answers: Option<std::collections::VecDeque<u32>>,
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
        script: None,
        answers: None,
    });
    LOGGING.store(true, Ordering::Relaxed);
    Ok(())
}

/// As [`start`], with the log of another run of the same drive as a script: wherever this run
/// asks FMOD whether an event is playing or paused with the same call at the same place, it is
/// told what the other run was told.
///
/// FMOD's mix is not the same from run to run (it picks samples and start offsets with random
/// numbers seeded from the clock), so a one-shot sound ends a frame earlier or later, and the
/// caller, who asks whether it still plays, goes another way. With the answers of one run
/// given to the other, two callers make the same calls exactly when they are the same logic.
pub fn start_scripted(file: Option<&Path>, script: &Path) -> Result<(), String> {
    let text = std::fs::read_to_string(script).map_err(|e| format!("{}: {e}", script.display()))?;
    start(file)?;
    if let Some(log) = LOG.lock().unwrap().as_mut() {
        log.script = Some(text.lines().map(str::to_string).collect());
    }
    Ok(())
}

/// As [`start_scripted`] with only the other run's answers, in the order it asked (a golden
/// file carries them like this).
pub fn start_with_answers(file: Option<&Path>, answers: &[u32]) -> Result<(), String> {
    start(file)?;
    if let Some(log) = LOG.lock().unwrap().as_mut() {
        log.answers = Some(answers.iter().copied().collect());
    }
    Ok(())
}

/// The answers a finished log holds to the questions [`start_scripted`] answers (a playback
/// state or a paused flag each), in order.
pub fn answers_of(log_text: &str) -> Vec<u32> {
    log_text
        .lines()
        .filter(|l| l.starts_with("event_get_playback_state ") || l.starts_with("event_get_paused "))
        .filter_map(|l| l.rsplit(' ').next().and_then(|word| u32::from_str_radix(word, 16).ok()))
        .collect()
}

/// The line count and hash a log file has (what [`finish`] reports for the run that wrote it).
pub fn summary_of(log_text: &[u8]) -> (u64, u64) {
    let mut hash = FNV_OFFSET;
    let mut lines = 0u64;
    for &b in log_text {
        hash = (hash ^ b as u64).wrapping_mul(FNV_PRIME);
        lines += (b == b'\n') as u64;
    }
    (lines, hash)
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

    /// What the script's run was answered at this place of the log, if it made the call that
    /// `before_arrow` is: the words after the result code.
    pub(crate) fn scripted_answer(&mut self, before_arrow: &str) -> Option<Vec<String>> {
        if let Some(answers) = self.answers.as_mut() {
            let answer = answers.pop_front()?;
            // a playback state is a word, a paused flag a byte
            return Some(vec![if before_arrow.starts_with("event_get_paused") { format!("{answer:02x}") } else { format!("{answer:08x}") }]);
        }
        let line = self.script.as_ref()?.get(self.lines as usize)?;
        let (call, answer) = line.split_once(" -> ")?;
        if call != before_arrow {
            return None;
        }
        Some(answer.split(' ').skip(1).map(str::to_string).collect())
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
