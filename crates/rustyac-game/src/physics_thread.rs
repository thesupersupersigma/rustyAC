//! The physics thread: steps the car at AC's fixed 333 Hz (3 ms), on its own clock, whatever
//! the display does. The display reads the last two steps ([`Frames`]) and blends them.
//!
//! Every step has a due time on an absolute schedule (`start + n x 3 ms`), so being a little
//! late once never shifts the steps after it: simulated time cannot drift from wall time.
//! Only when the thread falls hopelessly behind (more than 100 ms: a debugger, a sleeping
//! laptop) is the schedule started again from "now".

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::input_file::{event, StepInput};
use crate::sim::{GameSim, DT};
use crate::timer::{raise_thread_priority, PreciseSleeper};
use crate::view::CarView;

/// One physics step of wall time.
pub const STEP: Duration = Duration::from_micros(3000);
/// Further behind than this, the schedule starts again.
const HOPELESS: Duration = Duration::from_millis(100);

/// The last two steps, for the display.
#[derive(Clone, Copy, Debug)]
pub struct Frames {
    pub prev: CarView,
    pub curr: CarView,
    /// When `curr`'s step was due.
    pub curr_due: Instant,
}

impl Frames {
    /// The car as it should be drawn at `now`: one step behind the physics, blended between
    /// the last two steps by how far `now` is past the newer one's due time.
    pub fn at(&self, now: Instant) -> CarView {
        let since = now.saturating_duration_since(self.curr_due).as_secs_f32();
        CarView::between(&self.prev, &self.curr, since / DT)
    }
}

/// How well the thread kept its schedule.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Timing {
    /// Steps run against the clock.
    pub paced_steps: u64,
    /// Steps of spawn sequences, run without waiting.
    pub burst_steps: u64,
    /// Time one step took to compute (with the shared memory and the recorder), microseconds.
    pub step_min_us: f64,
    pub step_max_us: f64,
    pub step_sum_us: f64,
    /// How long after its due time a step started, microseconds.
    pub late_max_us: f64,
    pub late_sum_us: f64,
    pub late_over_1ms: u64,
    pub late_over_3ms: u64,
    /// Times the schedule was started again because the thread was more than 100 ms behind.
    pub restarts: u64,
    /// Wall time the paced steps were given (pauses and spawn sequences left out), s.
    pub wall_seconds: f64,
}

impl Timing {
    pub fn step_avg_us(&self) -> f64 {
        self.step_sum_us / (self.paced_steps + self.burst_steps).max(1) as f64
    }

    pub fn late_avg_us(&self) -> f64 {
        self.late_sum_us / self.paced_steps.max(1) as f64
    }

    /// Simulated minus wall time over the paced steps, ms. The wall time is taken at the end
    /// of the last step, which was due one step before its simulated time is over: with the
    /// schedule held this is a little under +3 ms, and it does not grow.
    pub fn drift_ms(&self) -> f64 {
        (self.paced_steps as f64 * DT as f64 - self.wall_seconds) * 1000.0
    }

    /// Steps per second of wall time.
    pub fn rate_hz(&self) -> f64 {
        self.paced_steps as f64 / self.wall_seconds.max(1e-9)
    }

    pub fn report(&self) -> String {
        format!(
            "physics: {} steps in {:.3} s of wall time = {:.2} Hz (AC: 333.33), simulated - wall = {:+.3} ms\n\
             \x20        step time min / avg / max = {:.3} / {:.3} / {:.3} ms\n\
             \x20        start after due time avg / max = {:.3} / {:.3} ms; more than 1 ms late: {} steps, more than a whole step (3 ms): {}\n\
             \x20        schedule restarts (over 100 ms behind): {}; spawn-sequence steps run unpaced: {}",
            self.paced_steps,
            self.wall_seconds,
            self.rate_hz(),
            self.drift_ms(),
            self.step_min_us / 1000.0,
            self.step_avg_us() / 1000.0,
            self.step_max_us / 1000.0,
            self.late_avg_us() / 1000.0,
            self.late_max_us / 1000.0,
            self.late_over_1ms,
            self.late_over_3ms,
            self.restarts,
            self.burst_steps
        )
    }
}

/// What the physics thread and the rest of the program share.
pub struct Shared {
    /// Stop the thread.
    pub quit: AtomicBool,
    /// The user paused.
    pub paused: AtomicBool,
    /// The window has the keyboard (an unfocused window pauses, unless told otherwise).
    pub focused: AtomicBool,
    /// [`event`] bits asked for from outside the thread (the window's keys).
    pub requests: AtomicU32,
    /// The replay or the duration has run out.
    pub finished: AtomicBool,
    pub frames: Mutex<Frames>,
    pub timing: Mutex<Timing>,
    /// Steps done (for anyone who only wants to know that it lives).
    pub steps: AtomicU64,
    /// One line about who is driving, for the HUD.
    pub driver_text: Mutex<String>,
}

impl Shared {
    pub fn new() -> Arc<Shared> {
        let view = CarView::default();
        Arc::new(Shared {
            quit: AtomicBool::new(false),
            paused: AtomicBool::new(false),
            focused: AtomicBool::new(true),
            requests: AtomicU32::new(0),
            finished: AtomicBool::new(false),
            frames: Mutex::new(Frames { prev: view, curr: view, curr_due: Instant::now() }),
            timing: Mutex::new(Timing::default()),
            steps: AtomicU64::new(0),
            driver_text: Mutex::new(String::new()),
        })
    }

    pub fn frames(&self) -> Frames {
        *self.frames.lock().unwrap()
    }

    pub fn timing(&self) -> Timing {
        *self.timing.lock().unwrap()
    }
}

/// Something that wants every step: the recorder, the shared memory.
pub trait StepSink: Send {
    fn after_step(&mut self, sim: &GameSim, input: &StepInput) -> Result<(), String>;

    /// The simulation is paused / running again.
    fn set_paused(&mut self, _paused: bool) {}

    fn finish(&mut self) -> Result<(), String> {
        Ok(())
    }
}

pub struct LoopConfig {
    /// A recorded drive to play in real time; the thread reports `finished` at its end.
    pub replay: Option<Vec<StepInput>>,
    /// Stop after this much wall time.
    pub duration: Option<Duration>,
    /// Stop after this many steps.
    pub max_steps: Option<u64>,
    /// Pause while the window does not have the keyboard.
    pub pause_unfocused: bool,
    /// Put a car that fell over or off the world back on the spawn point.
    pub auto_reset: bool,
}

/// Steps of lying on its roof (or side) before the car is put back: 3 s.
const FLIPPED_STEPS: u32 = 1000;

/// Runs the simulation until told to quit. Returns it with the timing.
pub fn run(mut sim: GameSim, shared: &Shared, mut sinks: Vec<Box<dyn StepSink>>, config: LoopConfig) -> Result<(GameSim, Timing), String> {
    raise_thread_priority();
    let sleeper = PreciseSleeper::new();
    let started = Instant::now();
    // the schedule: step `n` of the current stretch is due at `origin + n x 3 ms`
    let mut origin = Instant::now();
    let mut n: u32 = 0;
    let mut fresh = true;
    let mut timing = Timing { step_min_us: f64::MAX, ..Timing::default() };
    let mut was_paused = false;
    let mut replay_at = 0;
    let mut drive_start = sim.steps;
    let mut flipped = 0;
    let mut result = Ok(());
    {
        let view = CarView::capture(&sim, 0.0);
        *shared.frames.lock().unwrap() = Frames { prev: view, curr: view, curr_due: Instant::now() };
    }
    // closes the current stretch of paced steps: its wall time runs from its first step's due
    // time to now (the end of its last step)
    let close = |timing: &mut Timing, n: u32, origin: Instant| {
        if n > 0 {
            timing.wall_seconds += origin.elapsed().as_secs_f64();
        }
    };
    loop {
        if shared.quit.load(Ordering::Relaxed) {
            break;
        }
        if config.duration.is_some_and(|d| started.elapsed() >= d) || config.max_steps.is_some_and(|m| sim.steps >= m) {
            shared.finished.store(true, Ordering::Relaxed);
            break;
        }
        let replay_done = config.replay.as_ref().is_some_and(|steps| replay_at >= steps.len());
        let paused = shared.paused.load(Ordering::Relaxed)
            || (config.pause_unfocused && !shared.focused.load(Ordering::Relaxed))
            || replay_done;
        if paused {
            if !was_paused {
                close(&mut timing, n, origin);
                n = 0;
                fresh = true;
                was_paused = true;
                for sink in &mut sinks {
                    sink.set_paused(true);
                }
            }
            if replay_done {
                shared.finished.store(true, Ordering::Relaxed);
            }
            sim.car.device.source.idle();
            std::thread::sleep(Duration::from_millis(5));
            continue;
        }
        if was_paused {
            was_paused = false;
            for sink in &mut sinks {
                sink.set_paused(false);
            }
        }
        if let Some(steps) = &config.replay {
            sim.car.device.source.load(&steps[replay_at]);
            replay_at += 1;
        } else {
            let requests = shared.requests.swap(0, Ordering::Relaxed);
            if requests != 0 {
                sim.car.device.source.request(requests);
            }
        }
        let burst = sim.car.device.source.in_spawn_sequence();
        let mut due = Instant::now();
        if burst {
            if !fresh {
                close(&mut timing, n, origin);
                n = 0;
                fresh = true;
            }
        } else {
            if fresh {
                origin = Instant::now();
                n = 0;
                fresh = false;
            }
            due = origin + STEP * n;
            let now = Instant::now();
            if now < due {
                sleeper.sleep_until(due);
            } else if now - due > HOPELESS {
                close(&mut timing, n, origin);
                timing.restarts += 1;
                origin = now;
                n = 0;
                due = now;
            }
            let late = Instant::now().saturating_duration_since(due).as_secs_f64() * 1e6;
            timing.late_sum_us += late;
            timing.late_max_us = timing.late_max_us.max(late);
            timing.late_over_1ms += (late > 1000.0) as u64;
            timing.late_over_3ms += (late > 3000.0) as u64;
        }
        let begin = Instant::now();
        let input = match sim.step() {
            Ok(input) => input,
            Err(message) => {
                result = Err(message);
                break;
            }
        };
        if input.events & (event::RESET | event::REBUILD) != 0 {
            flipped = 0;
        }
        for sink in &mut sinks {
            if let Err(message) = sink.after_step(&sim, &input) {
                result = Err(message);
            }
        }
        if result.is_err() {
            break;
        }
        let took = begin.elapsed().as_secs_f64() * 1e6;
        timing.step_min_us = timing.step_min_us.min(took);
        timing.step_max_us = timing.step_max_us.max(took);
        timing.step_sum_us += took;
        if burst {
            timing.burst_steps += 1;
            drive_start = sim.steps;
        } else {
            timing.paced_steps += 1;
            n += 1;
        }
        if config.auto_reset && config.replay.is_none() {
            // the flat road has no walls and the car body nothing to lie on: a car that is on
            // its roof, has left the world or has broken numbers is put back
            let body = sim.car.car.core.get_world_matrix(sim.car.car.body).m;
            let broken = body.iter().flatten().any(|x| !x.is_finite());
            flipped = if body[1][1] < 0.2 { flipped + 1 } else { 0 };
            if broken {
                sim.car.device.source.request(event::REBUILD);
            } else if body[3][1] < -3.0 || body[3][1] > 500.0 || flipped > FLIPPED_STEPS {
                sim.car.device.source.request(event::RESET);
                flipped = 0;
            }
        }
        let view = CarView::capture(&sim, (sim.steps - drive_start) as f64 * DT as f64);
        {
            let mut frames = shared.frames.lock().unwrap();
            frames.prev = frames.curr;
            frames.curr = view;
            frames.curr_due = due;
            if burst {
                // nothing to blend through while the car is put down
                frames.prev = view;
            }
        }
        shared.steps.store(sim.steps, Ordering::Relaxed);
        if sim.steps % 32 == 0 {
            let mut live = timing;
            close(&mut live, n, origin);
            *shared.timing.lock().unwrap() = live;
        }
    }
    close(&mut timing, n, origin);
    if timing.step_min_us == f64::MAX {
        timing.step_min_us = 0.0;
    }
    *shared.timing.lock().unwrap() = timing;
    for sink in &mut sinks {
        let finished = sink.finish();
        if result.is_ok() {
            result = finished;
        }
    }
    shared.finished.store(true, Ordering::Relaxed);
    result.map(|()| (sim, timing))
}

/// The recorder of `--record`.
pub struct Recorder(pub Option<crate::input_file::InputWriter>);

impl StepSink for Recorder {
    fn after_step(&mut self, _sim: &GameSim, input: &StepInput) -> Result<(), String> {
        match &mut self.0 {
            Some(writer) => writer.push(input),
            None => Ok(()),
        }
    }

    fn finish(&mut self) -> Result<(), String> {
        match self.0.take() {
            Some(writer) => writer.finish().map(|_| ()),
            None => Ok(()),
        }
    }
}
