// SPDX-License-Identifier: GPL-3.0-or-later

//! The physics thread: steps the car at AC's fixed 333 Hz (3 ms), on its own clock, whatever
//! the display does. The display reads the last two steps ([`Frames`]) and blends them.
//!
//! Every step has a due time on an absolute schedule (`start + n x 3 ms`), so being a little
//! late once never shifts the steps after it: simulated time cannot drift from wall time.
//! Only when the thread falls hopelessly behind (more than 100 ms: a debugger, a sleeping
//! laptop) is the schedule started again from "now".

use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, AtomicU64, Ordering};
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
    /// How fast the steps follow each other (a replay's speed; 1 while driving)
    pub speed: f32,
}

impl Frames {
    /// The car as it should be drawn at `now`: one step behind the physics, blended between
    /// the last two steps by how far `now` is past the newer one's due time.
    pub fn at(&self, now: Instant) -> CarView {
        let since = now.saturating_duration_since(self.curr_due).as_secs_f32();
        CarView::between(&self.prev, &self.curr, since * self.speed / DT)
    }
}

/// The last two steps of the session's other cars (car 1, 2, …), for the display.
#[derive(Clone, Debug, Default)]
pub struct OtherFrames {
    pub prev: Vec<CarView>,
    pub curr: Vec<CarView>,
}

/// The other cars after the last step.
fn capture_others(sim: &GameSim) -> Vec<CarView> {
    (0..sim.opponents.len()).map(|index| CarView::capture_opponent(sim, index, 0.0)).chain((0..sim.remote_count()).map(|index| CarView::capture_remote(sim, index))).collect()
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
    /// Task 33: the last step's own time (`PhysicsEngine::step` alone), ms, as
    /// `PhysicsDriveThread::run` 0x140123150 measures it for `occupancy`
    pub last_step_ms: f64,
    /// Task 33: `physicsLateLoops`: the times the steps fell more than one step behind and had
    /// to catch up, after the first 30 s of the physics clock (one a run of late steps)
    pub late_loops: u32,
    /// the previous step started late by more than a step
    pub was_late: bool,
}

impl Timing {
    pub fn step_avg_us(&self) -> f64 {
        self.step_sum_us / (self.paced_steps + self.burst_steps).max(1) as f64
    }

    pub fn late_avg_us(&self) -> f64 {
        self.late_sum_us / self.paced_steps.max(1) as f64
    }

    /// Simulated minus wall time over the paced steps, ms: 0 while the schedule is held (n
    /// steps in n periods), negative by as much as the thread is behind at the end.
    pub fn drift_ms(&self) -> f64 {
        // a step is 3 ms exactly (the f32 0.003 the car is stepped with is a hair more)
        (self.paced_steps as f64 * 0.003 - self.wall_seconds) * 1000.0
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

/// "Keyboard debug"'s share: the parameters as of the last step, the edits not yet applied.
pub type KeyboardTuningShare = (Option<rustyac_gui::app_keyboard_debug::KeyboardTuning>, Vec<(usize, f32)>);

/// "X360 Controller debug"'s share, the same for the pad's class.
pub type PadTuningShare = (Option<rustyac_gui::app_pad_debug::PadTuning>, Vec<(usize, f32)>);

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
    /// Clicks of the cockpit's brake-bias control asked for from outside the thread.
    pub bias_requests: AtomicI32,
    /// The replay or the duration has run out.
    pub finished: AtomicBool,
    pub frames: Mutex<Frames>,
    /// The other cars' last two steps: written and read while `frames` is held, so that every
    /// car of a picture is of the same step.
    pub others: Mutex<OtherFrames>,
    pub timing: Mutex<Timing>,
    /// Steps done (for anyone who only wants to know that it lives).
    pub steps: AtomicU64,
    /// One line about who is driving, for the HUD.
    pub driver_text: Mutex<String>,
    /// Presses of a device's camera button (the display counts them).
    pub camera_toggles: AtomicU32,
    /// Task 34: Ctrl+M presses not yet given to the keyboard's mouse steering
    pub mouse_steering_toggles: AtomicU32,
    /// Task 34: the keyboard steers with the mouse (for the window, which leaves the cursor alone then)
    pub mouse_steering: std::sync::atomic::AtomicBool,
    /// Task 34: the player's car is driven by the keyboard's class (Ctrl+M acts only then)
    pub keyboard_drives: std::sync::atomic::AtomicBool,
    /// "Keyboard debug": the keyboard's steering parameters as of the last step (None when
    /// the game would not have picked the keyboard's class), and the app's edits not yet applied
    pub keyboard_tuning: Mutex<KeyboardTuningShare>,
    pub pad_tuning: Mutex<PadTuningShare>,
    /// The car's unchanging facts, once it is built.
    pub car_info: Mutex<Option<crate::sim::CarInfo>>,
    /// The same of the session's other cars (car 1, 2, …), set before `car_info`.
    pub other_infos: Mutex<Vec<crate::sim::CarInfo>>,
    /// A replay's speed (the bits of an `f32`; 0: it stands): the steps are paced by it.
    pub replay_speed: AtomicU32,
    /// A replay goes back to its first step (the transport's Stop).
    pub replay_restart: AtomicBool,
    /// Task 25: an online session's lines for the HUD (the player list, the chat).
    pub online_hud: Mutex<Vec<String>>,
    /// Task 30: what AC's own online overlays show
    pub online_gui: Mutex<Option<crate::online::OnlineGui>>,
    /// Task 34: what the client heard for the GUI, what the GUI asks of the client
    pub online_events: Mutex<Vec<crate::online::OnlineGuiEvent>>,
    pub online_requests: Mutex<Vec<crate::online::OnlineRequest>>,
    /// Task 28: what the referee says, for the HUD.
    pub referee_hud: Mutex<Option<crate::referee::RefereeHud>>,
    /// Task 45b: the window's drawn frames not yet run (`GameTime::deltaT` of each), for Time
    /// Attack and Drift ([`GameSim::drawn_frame`])
    pub drawn_frames: Mutex<Vec<f32>>,
    /// Task 28c: the session's track, for the replay's playback on the display thread
    pub track: Mutex<Option<Arc<rustyac_physics::track::Track>>>,
    /// Task 31: the setup editor's jobs (the game's `PhysicsAvatar::commandQueue`), and the
    /// player's car as the editor sees it, with a count of how often it was taken
    pub setup_jobs: Mutex<Vec<rustyac_physics::car::setup::SetupJob>>,
    pub setup_snapshot: Mutex<(u64, Option<rustyac_gui::setup_car::SetupSnapshot>)>,
    /// Task 34: the pit menu as the page "PITSTOP STRATEGY" last changed it
    pub pit_page: Mutex<Option<rustyac_physics::race::QuickMenu>>,
    /// the old mouse menu's clicks
    pub pit_clicks: Mutex<Vec<rustyac_physics::race::pitstop::MousePitClick>>,
    /// Task 33b: the Telemetry app is shown; the steps' channel values (physics time in ms)
    /// since the display took them
    pub telemetry_on: AtomicBool,
    pub telemetry: Mutex<Vec<(f64, Vec<f32>)>>,
    /// the Setup debug app's car, after each step while the app is shown
    pub setup_debug_on: AtomicBool,
    /// Task 44c: FormAI is shown (its view of the focused car, `setup_debug_focus`'s)
    pub ai_app_on: AtomicBool,
    pub ai_app: Mutex<Option<rustyac_gui::app_ai::AiAppView>>,
    pub setup_debug: Mutex<Option<rustyac_gui::app_setup_debug::SetupDebugCar>>,
    /// the display's focused car, which the Setup debug app reads (0: the player's)
    pub setup_debug_focus: std::sync::atomic::AtomicUsize,
}

impl Shared {
    pub fn new() -> Arc<Shared> {
        let view = CarView::default();
        Arc::new(Shared {
            quit: AtomicBool::new(false),
            paused: AtomicBool::new(false),
            focused: AtomicBool::new(true),
            requests: AtomicU32::new(0),
            bias_requests: AtomicI32::new(0),
            finished: AtomicBool::new(false),
            frames: Mutex::new(Frames { prev: view, curr: view, curr_due: Instant::now(), speed: 1.0 }),
            others: Mutex::new(OtherFrames::default()),
            other_infos: Mutex::new(Vec::new()),
            replay_speed: AtomicU32::new(1.0f32.to_bits()),
            replay_restart: AtomicBool::new(false),
            online_hud: Mutex::new(Vec::new()),
            online_gui: Mutex::new(None),
            online_events: Mutex::new(Vec::new()),
            online_requests: Mutex::new(Vec::new()),
            referee_hud: Mutex::new(None),
            drawn_frames: Mutex::new(Vec::new()),
            track: Mutex::new(None),
            setup_jobs: Mutex::new(Vec::new()),
            setup_snapshot: Mutex::new((0, None)),
            pit_page: Mutex::new(None),
            pit_clicks: Mutex::new(Vec::new()),
            telemetry_on: AtomicBool::new(false),
            telemetry: Mutex::new(Vec::new()),
            setup_debug_on: AtomicBool::new(false),
            ai_app_on: AtomicBool::new(false),
            ai_app: Mutex::new(None),
            setup_debug: Mutex::new(None),
            setup_debug_focus: std::sync::atomic::AtomicUsize::new(0),
            timing: Mutex::new(Timing::default()),
            steps: AtomicU64::new(0),
            driver_text: Mutex::new(String::new()),
            camera_toggles: AtomicU32::new(0),
            mouse_steering_toggles: AtomicU32::new(0),
            mouse_steering: std::sync::atomic::AtomicBool::new(false),
            keyboard_drives: std::sync::atomic::AtomicBool::new(false),
            keyboard_tuning: Mutex::new((None, Vec::new())),
            pad_tuning: Mutex::new((None, Vec::new())),
            car_info: Mutex::new(None),
        })
    }

    pub fn frames(&self) -> Frames {
        *self.frames.lock().unwrap()
    }

    /// Every car of the session as it should be drawn at `now`: the player's first.
    pub fn world_at(&self, now: Instant) -> Vec<CarView> {
        let frames = self.frames.lock().unwrap();
        let others = self.others.lock().unwrap();
        let since = now.saturating_duration_since(frames.curr_due).as_secs_f32();
        let t = since * frames.speed / DT;
        let mut world = Vec::with_capacity(1 + others.curr.len());
        world.push(CarView::between(&frames.prev, &frames.curr, t));
        for (prev, curr) in others.prev.iter().zip(&others.curr) {
            world.push(CarView::between(prev, curr, t));
        }
        world
    }

    /// The player's car as the setup editor sees it, as of now.
    pub fn publish_setup(&self, sim: &GameSim) {
        let mut snapshot = rustyac_gui::setup_car::SetupSnapshot::of(&sim.car.car, &sim.setup_car_name());
        snapshot.applied_jobs = sim.setup_log.len() as u64;
        let mut slot = self.setup_snapshot.lock().unwrap();
        *slot = (slot.0 + 1, Some(snapshot));
    }

    /// Puts a fresh start of the session before the display: nothing to blend through.
    fn publish_start(&self, sim: &GameSim) {
        let view = CarView::capture(sim, 0.0);
        let others = capture_others(sim);
        let mut frames = self.frames.lock().unwrap();
        *frames = Frames { prev: view, curr: view, curr_due: Instant::now(), speed: 1.0 };
        *self.others.lock().unwrap() = OtherFrames { prev: others.clone(), curr: others };
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
    /// Task 25, an online session: where its tape goes at the end (`--net-tape`), and the
    /// chat lines to send at given times (`--chat`).
    pub net_tape: Option<std::path::PathBuf>,
    pub chat: Vec<(String, f64)>,
    /// Task 28: where the referee's `race_out.json` and `laps.ini` go at the end, and its
    /// buttons pressed at given seconds of the run (`--next-session-at`).
    pub referee_out: Option<std::path::PathBuf>,
    pub session_script: Vec<(f64, u32)>,
    /// Task 43: the mouse menu's clicks at given seconds (`--pit-click`; not over a replay)
    pub pit_clicks: Vec<(f64, rustyac_physics::race::pitstop::MousePitClick)>,
    /// Task 45b, a proof: drawn frames of uneven length from this seed (`--drawn-frames`)
    pub drawn_frames: Option<u32>,
}

/// Steps of lying still on its roof (or side) before the car is put back: 3 s.
const FLIPPED_STEPS: u32 = 1000;

/// The rolled-over watch, one step on: how many steps in a row the car has now lain still on
/// its roof or side. `up_y` is the height of the body's up axis (1 upright, -1 on the roof),
/// `speed` in m/s, `spin_squared` the squared angular velocity. A car that still slides or
/// tumbles starts the count again.
fn flipped_steps(previous: u32, up_y: f32, speed: f32, spin_squared: f32) -> u32 {
    let at_rest = speed < 0.5 && spin_squared < 0.25;
    if up_y < 0.2 && at_rest {
        previous + 1
    } else {
        0
    }
}

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
    // the heights between which the car is in the world: around the flat road, or around the
    // track's meshes (Spa's road goes well below its start line)
    let (floor, ceiling) = match &sim.track {
        Some(track) if !track.world.meshes.is_empty() => {
            let lowest = track.world.meshes.iter().map(|mesh| mesh.aabb[2]).fold(f32::MAX, f32::min);
            let highest = track.world.meshes.iter().map(|mesh| mesh.aabb[3]).fold(f32::MIN, f32::max);
            (lowest - 20.0, highest + 500.0)
        }
        _ => (-3.0, 500.0),
    };
    // a replay can go back to its start: the drive is built again from the same set-up
    let initial_setup = config.replay.as_ref().map(|_| sim.setup.clone());
    let mut last_speed = 1.0f32;
    let mut chat_at = 0;
    let mut telemetry_names: Option<Vec<String>> = None;
    let mut result = Ok(());
    // --drawn-frames: the frames' clock (sim seconds) and their lengths' draws
    let (mut frame_at, mut frame_rand) = (0.0f64, config.drawn_frames.unwrap_or(0));
    if let (Some(_), Some(referee)) = (config.drawn_frames, sim.referee.as_mut()) {
        referee.record_mode_frames();
    }
    {
        shared.publish_start(&sim);
        *shared.track.lock().unwrap() = sim.track.clone();
        *shared.other_infos.lock().unwrap() = sim.opponent_infos();
        shared.publish_setup(&sim);
        let mut info = sim.car_info();
        info.setup = shared.setup_snapshot.lock().unwrap().1.clone().map(Box::new);
        *shared.car_info.lock().unwrap() = Some(info);
    }
    // closes the current stretch of paced steps: n steps own n periods of wall time from the
    // first step's due time on, or more if the last step ended later than that
    let close = |timing: &mut Timing, n: u32, origin: Instant| {
        if n > 0 {
            timing.wall_seconds += origin.elapsed().max(STEP * n).as_secs_f64();
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
        if let (Some(setup), true) = (&initial_setup, shared.replay_restart.swap(false, Ordering::Relaxed)) {
            match GameSim::new(setup.clone(), Box::new(crate::sim::ReplaySource::default())) {
                Ok(again) => sim = again,
                Err(message) => {
                    result = Err(message);
                    break;
                }
            }
            replay_at = 0;
            drive_start = sim.steps;
            close(&mut timing, n, origin);
            n = 0;
            fresh = true;
            shared.finished.store(false, Ordering::Relaxed);
            shared.publish_start(&sim);
        }
        let speed = if config.replay.is_some() { f32::from_bits(shared.replay_speed.load(Ordering::Relaxed)) } else { 1.0 };
        let replay_done = config.replay.as_ref().is_some_and(|steps| replay_at >= steps.len());
        // (an online session never pauses: the server and the others go on)
        let online = sim.online.is_some();
        let paused = (!online
            && (shared.paused.load(Ordering::Relaxed) || speed <= 0.0 || speed.is_nan() || (config.pause_unfocused && !shared.focused.load(Ordering::Relaxed)) || crate::window::MODAL_LOOP.load(Ordering::Relaxed)))
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
            // (paused, Time Attack and Drift skip their frames)
            shared.drawn_frames.lock().unwrap().clear();
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
            sim.setup_jobs.append(&mut shared.setup_jobs.lock().unwrap());
            if let Some(page) = shared.pit_page.lock().unwrap().take() {
                sim.apply_pit_page(&page);
            }
            for click in std::mem::take(&mut *shared.pit_clicks.lock().unwrap()) {
                sim.pit_click(click);
            }
            let clicks = shared.bias_requests.swap(0, Ordering::Relaxed);
            if clicks != 0 {
                sim.car.device.source.request_bias(clicks);
            }
        }
        // (a car held for the ESC menu's "Drive" does not move its sequence on: paced, not a burst;
        // nor does a car an AI driver drives, which never asks the sequence)
        let burst = sim.car.device.source.in_spawn_sequence() && !sim.car.car.is_controls_locked && sim.car.device.ai.is_none() && sim.car.device.drag.is_none();
        let mut due = Instant::now();
        if burst {
            if !fresh {
                close(&mut timing, n, origin);
                n = 0;
                fresh = true;
            }
        } else {
            if speed != last_speed {
                // a new speed: a new stretch of the schedule
                close(&mut timing, n, origin);
                last_speed = speed;
                fresh = true;
            }
            if fresh {
                origin = Instant::now();
                n = 0;
                fresh = false;
            }
            due = if speed == 1.0 { origin + STEP * n } else { origin + Duration::from_secs_f64(STEP.as_secs_f64() * n as f64 / speed as f64) };
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
            if late > 3000.0 && !timing.was_late && sim.steps * 3 > 30_000 {
                timing.late_loops += 1;
            }
            timing.was_late = late > 3000.0;
        }
        for (at, bits) in &config.session_script {
            if (at / 0.003).round() as u64 == sim.steps {
                sim.car.device.source.request(*bits);
            }
        }
        if config.replay.is_none() {
            for (at, click) in &config.pit_clicks {
                if (at / 0.003).round() as u64 == sim.steps {
                    sim.pit_click(*click);
                }
            }
        }
        for dt in std::mem::take(&mut *shared.drawn_frames.lock().unwrap()) {
            sim.drawn_frame(dt);
        }
        while config.drawn_frames.is_some() && sim.sim_seconds() >= frame_at {
            frame_rand = frame_rand.wrapping_mul(214_013).wrapping_add(2_531_011);
            let dt = (5 + (frame_rand >> 16 & 0x7fff) % 36) as f32 * 0.001;
            frame_at += dt as f64;
            sim.drawn_frame(dt);
        }
        let begin = Instant::now();
        let jobs_before = sim.setup_log.len();
        let input = match sim.step() {
            Ok(input) => input,
            Err(message) => {
                result = Err(message);
                break;
            }
        };
        timing.last_step_ms = begin.elapsed().as_secs_f64() * 1000.0;
        if let (Some(_), Some(referee)) = (config.drawn_frames, sim.referee.as_mut()) {
            referee.drift_step_line(crate::sim::DT, &sim.car.car.drift_input, &sim.car.car.drift);
        }
        // the car as the setup editor sees it: after its jobs, and a few times a second
        if sim.setup_log.len() != jobs_before || sim.steps.is_multiple_of(64) {
            shared.publish_setup(&sim);
        }
        if shared.ai_app_on.load(Ordering::Relaxed) {
            *shared.ai_app.lock().unwrap() = Some(crate::view::ai_app(&sim, shared.setup_debug_focus.load(Ordering::Relaxed)));
        }
        if shared.setup_debug_on.load(Ordering::Relaxed) {
            *shared.setup_debug.lock().unwrap() = Some(crate::view::setup_debug(&sim, shared.setup_debug_focus.load(Ordering::Relaxed)));
        }
        if shared.telemetry_on.load(Ordering::Relaxed) {
            let names = telemetry_names.get_or_insert_with(|| rustyac_gui::app_telemetry::channels(&crate::view::telemetry_car(&sim)).into_iter().map(|c| c.name).collect());
            let mut steps = shared.telemetry.lock().unwrap();
            // (a display that stopped taking them: keep the newest second)
            if steps.len() >= 333 {
                steps.remove(0);
            }
            steps.push((sim.sim_seconds() * 1000.0, crate::view::telemetry_values(&sim, names)));
        }
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
            // a car that has come to rest on its roof or side for three seconds, has left the
            // world or has broken numbers is put back (on a track: on the road where it lies,
            // as with Shift+R). While it still slides or tumbles it is left alone.
            let body = sim.car.car.core.get_world_matrix(sim.car.car.body).m;
            let broken = body.iter().flatten().any(|x| !x.is_finite());
            let spin = sim.car.car.core.get_angular_velocity(sim.car.car.body);
            flipped = flipped_steps(flipped, body[1][1], sim.car.car.speed, spin.x * spin.x + spin.y * spin.y + spin.z * spin.z);
            if broken {
                sim.car.device.source.request(event::REBUILD);
            } else if body[3][1] < floor || body[3][1] > ceiling || flipped > FLIPPED_STEPS {
                sim.car.device.source.request(if sim.track.is_some() { event::TO_TRACK } else { event::RESET });
                flipped = 0;
            }
        }
        let view = CarView::capture(&sim, (sim.steps - drive_start) as f64 * DT as f64);
        let other_views = capture_others(&sim);
        {
            let mut frames = shared.frames.lock().unwrap();
            frames.prev = frames.curr;
            frames.curr = view;
            frames.curr_due = due;
            frames.speed = speed;
            if burst {
                // nothing to blend through while the car is put down
                frames.prev = view;
            }
            if !other_views.is_empty() {
                let mut others = shared.others.lock().unwrap();
                let others = &mut *others;
                std::mem::swap(&mut others.prev, &mut others.curr);
                others.curr = other_views;
                if burst || others.prev.len() != others.curr.len() {
                    others.prev.clone_from(&others.curr);
                }
            }
        }
        let (steps, seconds) = (sim.steps, sim.sim_seconds());
        if let Some(session) = &mut sim.online {
            for line in session.console.drain(..) {
                println!("{line}");
            }
            while chat_at < config.chat.len() && seconds >= config.chat[chat_at].1 {
                // (`/client_list` stays here, as in the game: the list is printed, nothing is sent)
                if config.chat[chat_at].0 == "/client_list" {
                    for line in session.hud_lines() {
                        println!("online: {line}");
                    }
                } else if config.chat[chat_at].0.starts_with("/vote ") {
                    let line = config.chat[chat_at].0.clone();
                    session.scripted_vote(&line);
                } else {
                    session.net.client.send_chat(&config.chat[chat_at].0, seconds * 1000.0);
                }
                chat_at += 1;
            }
            // the GUI's chat lines and votes (ACChat, RaceControlVoting, the keys Y and N)
            for request in std::mem::take(&mut *shared.online_requests.lock().unwrap()) {
                match request {
                    crate::online::OnlineRequest::Chat { text, now } => {
                        if text == "/client_list" {
                            for line in session.hud_lines() {
                                println!("online: {line}");
                            }
                        } else {
                            session.net.client.send_chat(&text, now);
                        }
                    }
                    crate::online::OnlineRequest::Vote { kind, yes, guid } => session.send_vote(kind, yes, guid),
                }
            }
            if !session.gui_events.is_empty() {
                shared.online_events.lock().unwrap().append(&mut session.gui_events);
            }
            // (every step while the ESC menu's 5 s before the start can come: its header asks then)
            let start_near = (0.0..6000.0).contains(&(session.session.start_time - session.now));
            if steps.is_multiple_of(64) || start_near || shared.online_gui.lock().unwrap().is_none() {
                *shared.online_hud.lock().unwrap() = session.hud_lines();
                let at = sim.car.car.core.get_world_matrix(sim.car.car.body).m[3];
                let pit = sim.car.car.pit_position.m[3];
                let in_pit = rustyac_gui::micro_sectors::is_in_pit([at[0], at[1], at[2]], [pit[0], pit[1], pit[2]]);
                *shared.online_gui.lock().unwrap() = Some(session.gui_state(in_pit));
            }
            if session.net.client.is_lost() {
                result = Err("the connection to the server is gone".to_string());
                break;
            }
        }
        if sim.referee.is_some() && sim.steps.is_multiple_of(9) {
            *shared.referee_hud.lock().unwrap() = sim.referee_hud();
        }
        shared.steps.store(sim.steps, Ordering::Relaxed);
        if sim.steps.is_multiple_of(32) {
            let mut live = timing;
            close(&mut live, n, origin);
            *shared.timing.lock().unwrap() = live;
        }
    }
    close(&mut timing, n, origin);
    if let Some(session) = &mut sim.online {
        // ACClient::shutdown: tell the server
        session.net.client.shutdown();
        println!(
            "online: left the server after {} steps ({} datagrams in, {} out, {} messages in)",
            sim.steps, session.net.client.datagrams_in, session.net.client.datagrams_out, session.net.client.messages_in
        );
        for line in session.summary() {
            println!("{line}");
        }
        if let Some(path) = &config.net_tape {
            match crate::join::save_tape(&session.net.client.transport, path) {
                Ok(entries) => println!("online: the session's tape ({entries} messages) is in {}", path.display()),
                Err(message) => eprintln!("WARNING: {message}"),
            }
        }
    }
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
    // Task 28: the end of the program for the referee: the results, the two files (Task 43:
    // online also after the server went away, as RaceManager::shutdown runs whatever ended it)
    if result.is_ok() || sim.online.is_some() {
        if let Some(lines) = crate::referee::finish(&mut sim, config.referee_out.as_deref()) {
            for line in lines {
                println!("{line}");
            }
        }
    }
    shared.finished.store(true, Ordering::Relaxed);
    result.map(|()| (sim, timing))
}

/// The recorder of `--record`: the input file, and beside it the setup editor's jobs.
pub struct Recorder {
    pub writer: Option<crate::input_file::InputWriter>,
    pub path: std::path::PathBuf,
    pub setup_jobs: Vec<(u64, rustyac_physics::car::setup::SetupJob)>,
    pub pit_jobs: Vec<(u64, crate::input_file::PitJob)>,
}

impl Recorder {
    pub fn new(path: &std::path::Path, writer: crate::input_file::InputWriter) -> Recorder {
        Recorder { writer: Some(writer), path: path.to_path_buf(), setup_jobs: Vec::new(), pit_jobs: Vec::new() }
    }
}

impl StepSink for Recorder {
    fn after_step(&mut self, sim: &GameSim, input: &StepInput) -> Result<(), String> {
        if sim.setup_log.len() > self.setup_jobs.len() {
            self.setup_jobs.extend_from_slice(&sim.setup_log[self.setup_jobs.len()..]);
        }
        if sim.pit_log.len() > self.pit_jobs.len() {
            self.pit_jobs.extend_from_slice(&sim.pit_log[self.pit_jobs.len()..]);
        }
        match &mut self.writer {
            Some(writer) => writer.push(input),
            None => Ok(()),
        }
    }

    fn finish(&mut self) -> Result<(), String> {
        match self.writer.take() {
            Some(writer) => {
                writer.finish()?;
                crate::input_file::write_setup_jobs(&self.path, &self.setup_jobs)?;
                crate::input_file::write_pit_jobs(&self.path, &self.pit_jobs)
            }
            None => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_rolled_over_car_is_put_back_only_after_three_seconds_at_rest() {
        // on its roof and still sliding: the count does not start
        let mut flipped = 0;
        for _ in 0..5000 {
            flipped = flipped_steps(flipped, -0.98, 3.0, 0.0);
        }
        assert_eq!(flipped, 0);
        // at rest: put back after 1000 steps (3 s), not before
        for step in 1..=FLIPPED_STEPS + 1 {
            flipped = flipped_steps(flipped, -0.98, 0.1, 0.01);
            assert_eq!(flipped > FLIPPED_STEPS, step == FLIPPED_STEPS + 1, "step {step}");
        }
        // a nudge (it rocks) starts the three seconds again; so does landing on its wheels
        assert_eq!(flipped_steps(900, -0.98, 0.1, 0.3), 0);
        assert_eq!(flipped_steps(900, 0.9, 0.0, 0.0), 0);
        // on its side counts as rolled over
        assert_eq!(flipped_steps(10, 0.05, 0.0, 0.0), 11);
    }
}
