// SPDX-License-Identifier: GPL-3.0-or-later

//! The worker threads of the physics engine: AC's `ThreadPool` (ctor @ 0x1402d04a0) as far as
//! `PhysicsEngine::step` uses it. The game queues one task per car and waits until all of
//! them are done; so does [`ThreadPool::run`]. Which worker takes which car is up to the
//! scheduler, in the game and here, and must not matter: a task touches its own car only.

use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;

/// The job of one [`ThreadPool::run`]: a borrowed closure whose lifetime is erased. `run`
/// does not return before every worker has let go of it.
#[derive(Clone, Copy)]
struct Job(*const (dyn Fn(usize) + Sync));

// The pointer is only followed between the start and the end of `run`, whose caller holds
// the closure; the closure itself is `Sync`.
unsafe impl Send for Job {}

struct State {
    job: Option<Job>,
    /// The next item nobody has taken.
    next: usize,
    /// How many items the job has.
    count: usize,
    /// Items taken or waiting that are not finished.
    pending: usize,
    /// A task panicked.
    failed: bool,
    quit: bool,
}

struct Shared {
    state: Mutex<State>,
    work: Condvar,
    done: Condvar,
}

/// Worker threads that wait for [`ThreadPool::run`].
pub struct ThreadPool {
    shared: Arc<Shared>,
    workers: Vec<JoinHandle<()>>,
}

impl ThreadPool {
    /// A pool of `threads` workers (the calling thread works too while it waits, so `run`
    /// has `threads + 1` hands).
    pub fn new(threads: usize) -> ThreadPool {
        let shared = Arc::new(Shared {
            state: Mutex::new(State { job: None, next: 0, count: 0, pending: 0, failed: false, quit: false }),
            work: Condvar::new(),
            done: Condvar::new(),
        });
        let workers = (0..threads)
            .map(|index| {
                let shared = shared.clone();
                std::thread::Builder::new()
                    .name(format!("physics worker {index}"))
                    .spawn(move || worker(&shared))
                    .expect("a physics worker thread")
            })
            .collect();
        ThreadPool { shared, workers }
    }

    /// The number of worker threads.
    pub fn threads(&self) -> usize {
        self.workers.len()
    }

    /// Calls `f(0)`, `f(1)`, ... `f(count - 1)`, each exactly once, on the workers and on
    /// this thread, and returns when all calls have returned.
    pub fn run(&self, count: usize, f: &(dyn Fn(usize) + Sync)) {
        self.run_with(count, f, || {});
    }

    /// [`ThreadPool::run`], and this thread first does `before` (work that must stay on it)
    /// while the workers have already started.
    pub fn run_with(&self, count: usize, f: &(dyn Fn(usize) + Sync), before: impl FnOnce()) {
        if count == 0 {
            before();
            return;
        }
        // SAFETY: the lifetime of `f` is erased for the workers. They only reach it through
        // `state.job`, which is taken away again below before this function returns, after
        // every call has ended (`pending == 0`).
        let job = Job(unsafe { std::mem::transmute::<*const (dyn Fn(usize) + Sync + '_), *const (dyn Fn(usize) + Sync + 'static)>(f) });
        {
            let mut state = self.shared.state.lock().unwrap_or_else(|e| e.into_inner());
            state.job = Some(job);
            state.next = 0;
            state.count = count;
            state.pending = count;
            state.failed = false;
        }
        self.shared.work.notify_all();
        let before_ok = std::panic::catch_unwind(std::panic::AssertUnwindSafe(before)).is_ok();
        // this thread takes items like a worker
        let mut state = self.shared.state.lock().unwrap_or_else(|e| e.into_inner());
        loop {
            if state.next < state.count {
                let item = state.next;
                state.next += 1;
                drop(state);
                let ok = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(item))).is_ok();
                state = self.shared.state.lock().unwrap_or_else(|e| e.into_inner());
                state.pending -= 1;
                state.failed |= !ok;
            } else if state.pending > 0 {
                state = self.shared.done.wait(state).unwrap_or_else(|e| e.into_inner());
            } else {
                break;
            }
        }
        state.job = None;
        let failed = state.failed;
        drop(state);
        assert!(!failed && before_ok, "a physics task panicked");
    }
}

fn worker(shared: &Shared) {
    let mut state = shared.state.lock().unwrap_or_else(|e| e.into_inner());
    loop {
        if state.quit {
            return;
        }
        match state.job {
            Some(job) if state.next < state.count => {
                let item = state.next;
                state.next += 1;
                drop(state);
                // SAFETY: see `ThreadPool::run`: the closure lives until `pending` is 0, and
                // this item is counted in `pending`.
                let ok = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe { (*job.0)(item) })).is_ok();
                state = shared.state.lock().unwrap_or_else(|e| e.into_inner());
                state.pending -= 1;
                state.failed |= !ok;
                if state.pending == 0 {
                    shared.done.notify_all();
                }
            }
            _ => state = shared.work.wait(state).unwrap_or_else(|e| e.into_inner()),
        }
    }
}

impl Drop for ThreadPool {
    fn drop(&mut self) {
        self.shared.state.lock().unwrap_or_else(|e| e.into_inner()).quit = true;
        self.shared.work.notify_all();
        for worker in self.workers.drain(..) {
            let _ = worker.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    #[test]
    fn every_item_once() {
        let pool = ThreadPool::new(3);
        for round in 0..200usize {
            let count = round % 9;
            let hits: Vec<AtomicU32> = (0..count).map(|_| AtomicU32::new(0)).collect();
            pool.run(count, &|i| {
                hits[i].fetch_add(1, Ordering::Relaxed);
            });
            assert!(hits.iter().all(|h| h.load(Ordering::Relaxed) == 1), "round {round}");
        }
    }

    #[test]
    fn no_workers_is_a_plain_loop() {
        let pool = ThreadPool::new(0);
        let sum = AtomicU32::new(0);
        pool.run(5, &|i| {
            sum.fetch_add(i as u32, Ordering::Relaxed);
        });
        assert_eq!(sum.load(Ordering::Relaxed), 10);
    }
}
