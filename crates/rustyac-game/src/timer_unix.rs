// SPDX-License-Identifier: GPL-3.0-or-later

//! `timer.rs` off Windows: the kernel's sleep already wakes within a fraction of a millisecond,
//! so a plain sleep for most of the wait and a short spin for the rest.

use std::time::{Duration, Instant};

/// How long before the deadline the sleep ends; the rest is spun away.
const SPIN: Duration = Duration::from_micros(250);

#[derive(Default)]
pub struct PreciseSleeper;

impl PreciseSleeper {
    pub fn new() -> PreciseSleeper {
        PreciseSleeper
    }

    /// Always: the system's sleep is the precise one here.
    pub fn high_resolution(&self) -> bool {
        true
    }

    /// Returns at `deadline` (at once if it has passed).
    pub fn sleep_until(&self, deadline: Instant) {
        if let Some(wait) = deadline.checked_duration_since(Instant::now()).and_then(|left| left.checked_sub(SPIN)) {
            std::thread::sleep(wait);
        }
        while Instant::now() < deadline {
            std::hint::spin_loop();
        }
    }
}

/// shortcut: an ordinary user may not raise a thread's priority on Linux (it needs a nice
/// limit or CAP_SYS_NICE), so the physics thread keeps its own; revisit with L5's timing.
pub fn raise_thread_priority() -> bool {
    false
}
