//! Waiting for a point in time to within a fraction of a millisecond: a high-resolution
//! waitable timer for most of the wait, a short spin for the rest. (Plain `Sleep` wakes up
//! to 15 ms late; a 3 ms physics step needs better.)

use std::time::{Duration, Instant};

use windows::core::PCWSTR;
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::Media::{timeBeginPeriod, timeEndPeriod};
use windows::Win32::System::Threading::{
    CreateWaitableTimerExW, GetCurrentThread, SetThreadPriority, SetWaitableTimer, WaitForSingleObject,
    CREATE_WAITABLE_TIMER_HIGH_RESOLUTION, INFINITE, THREAD_PRIORITY_HIGHEST, TIMER_ALL_ACCESS,
};

/// How long before the deadline the timer is asked to fire; the rest is spun away.
const SPIN: Duration = Duration::from_micros(250);

pub struct PreciseSleeper {
    /// A high-resolution waitable timer (Windows 10 1803 and later), if the system has them.
    timer: Option<HANDLE>,
    /// `timeBeginPeriod(1)` is in force (the fall-back, and a help for the timer on old builds).
    period: bool,
}

impl PreciseSleeper {
    pub fn new() -> PreciseSleeper {
        // SAFETY: plain Win32 calls with valid arguments; the handle is closed in `drop`.
        unsafe {
            let timer = CreateWaitableTimerExW(None, PCWSTR::null(), CREATE_WAITABLE_TIMER_HIGH_RESOLUTION, TIMER_ALL_ACCESS.0).ok();
            let period = timeBeginPeriod(1) == 0;
            PreciseSleeper { timer, period }
        }
    }

    /// Is the high-resolution timer in use (else: 1 ms sleeps)?
    pub fn high_resolution(&self) -> bool {
        self.timer.is_some()
    }

    /// Returns at `deadline` (at once if it has passed).
    pub fn sleep_until(&self, deadline: Instant) {
        let now = Instant::now();
        if deadline <= now {
            return;
        }
        let remaining = deadline - now;
        if remaining > SPIN {
            let wait = remaining - SPIN;
            match self.timer {
                Some(timer) => {
                    // relative due time in 100 ns units, negative
                    let due = -((wait.as_nanos() / 100).max(1) as i64);
                    // SAFETY: `timer` is a live timer handle; `due` outlives the call.
                    unsafe {
                        if SetWaitableTimer(timer, &due, 0, None, None, false).is_ok() {
                            WaitForSingleObject(timer, INFINITE);
                        }
                    }
                }
                None => {
                    if wait >= Duration::from_millis(2) {
                        std::thread::sleep(wait - Duration::from_millis(1));
                    }
                }
            }
        }
        while Instant::now() < deadline {
            std::hint::spin_loop();
        }
    }
}

impl Default for PreciseSleeper {
    fn default() -> PreciseSleeper {
        PreciseSleeper::new()
    }
}

impl Drop for PreciseSleeper {
    fn drop(&mut self) {
        // SAFETY: the handle was created in `new` and is closed once.
        unsafe {
            if let Some(timer) = self.timer.take() {
                let _ = CloseHandle(timer);
            }
            if self.period {
                timeEndPeriod(1);
            }
        }
    }
}

/// Raises the calling thread's priority, as a physics thread that must not be late wants.
pub fn raise_thread_priority() -> bool {
    // SAFETY: the pseudo handle of the current thread is always valid.
    unsafe { SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_HIGHEST).is_ok() }
}
