// SPDX-License-Identifier: GPL-3.0-or-later

//! One drive in the browser: the simulation, its clock and its camera.
//!
//! **The loop.** The physics runs at AC's 333 Hz (3 ms steps) from a fixed-step accumulator
//! that the page's `requestAnimationFrame` feeds with the time since its last frame. There
//! is no thread and no timer: every step of a frame runs inside that frame's callback, back
//! to back, and the picture is drawn between the last two steps (blended by what is left in
//! the accumulator), as the desktop's window draws between the physics thread's last two.
//!
//! **A slow frame.** A frame that took longer than [`MAX_FRAME_SECONDS`] (a hidden tab that
//! comes back, a stall while the browser compiles shaders) is counted as that long and no
//! longer: at most 34 steps are caught up and the rest of the time is dropped, so the car
//! falls behind the wall clock instead of the browser freezing while it catches up. It is the
//! desktop physics thread's own rule (more than 100 ms late: start the schedule again).
//! A machine that cannot do 333 steps a second at all therefore runs in slow motion; every
//! step is still a whole 3 ms step, so the physics itself is never different.

use std::sync::{Arc, Mutex};

use rustyac_game::input_file::event;
use rustyac_game::render::scene::{CameraFrame, CarShape, DrivingCamera};
use rustyac_game::sim::{GameSim, SpawnSequence, DT};
use rustyac_game::view::CarView;

use crate::driver::{Input, SharedInput, WebSource};

/// The longest stretch of time one frame may hand to the physics, seconds.
pub const MAX_FRAME_SECONDS: f64 = 0.1;

/// Steps of lying still on its roof or side before the car is put back on the road: 3 s.
const FLIPPED_STEPS: u32 = 1000;

/// What a frame did, for the page's counters.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FrameStats {
    /// Physics steps run in this frame.
    pub steps: u32,
    /// Seconds of this frame that were dropped (the frame was longer than the cap).
    pub dropped_seconds: f64,
}

pub struct Session {
    pub sim: GameSim,
    pub input: SharedInput,
    pub shape: CarShape,
    pub camera: DrivingCamera,
    /// The keys and pad buttons in use, as text (the built-in layout).
    pub keys: String,
    accumulator: f64,
    prev: CarView,
    curr: CarView,
    /// The step count when the spawn sequence ended.
    drive_start: u64,
    flipped: u32,
    floor: f32,
    ceiling: f32,
    /// Steps and dropped time over the whole drive.
    pub total_steps: u64,
    pub total_dropped_seconds: f64,
}

impl Session {
    /// Builds the car on the track and puts it down (the spawn sequence runs to its end here:
    /// 470 steps of settling and selecting first gear, unpaced as on the desktop).
    pub fn new(car: &str, track: &str, layout: &str, auto_shifter: bool, spawn: &str) -> Result<Session, String> {
        let input: SharedInput = Arc::new(Mutex::new(Input::default()));
        let source = WebSource::new(input.clone());
        let keys = source.bindings.describe();
        let source = SpawnSequence::new(source, true);
        let mut sim = GameSim::new(crate::content::setup(car, track, layout, auto_shifter, spawn), Box::new(source))?;
        while sim.car.device.source.in_spawn_sequence() {
            sim.step()?;
        }
        let shape = CarShape::of(&sim.car_info());
        let (floor, ceiling) = match &sim.track {
            Some(track) if !track.world.meshes.is_empty() => {
                let lowest = track.world.meshes.iter().map(|mesh| mesh.aabb[2]).fold(f32::MAX, f32::min);
                let highest = track.world.meshes.iter().map(|mesh| mesh.aabb[3]).fold(f32::MIN, f32::max);
                (lowest - 20.0, highest + 500.0)
            }
            _ => (-3.0, 500.0),
        };
        let view = CarView::capture(&sim, 0.0);
        Ok(Session {
            drive_start: sim.steps,
            sim,
            input,
            shape,
            camera: DrivingCamera::chase(),
            keys,
            accumulator: 0.0,
            prev: view,
            curr: view,
            flipped: 0,
            floor,
            ceiling,
            total_steps: 0,
            total_dropped_seconds: 0.0,
        })
    }

    fn step(&mut self) -> Result<(), String> {
        self.sim.step()?;
        let burst = self.sim.car.device.source.in_spawn_sequence();
        if burst {
            self.drive_start = self.sim.steps;
        }
        // as the desktop's physics thread: a car at rest on its roof or side for three
        // seconds, out of the world or with broken numbers is put back
        let body = self.sim.car.car.core.get_world_matrix(self.sim.car.car.body).m;
        let broken = body.iter().flatten().any(|x| !x.is_finite());
        let spin = self.sim.car.car.core.get_angular_velocity(self.sim.car.car.body);
        let at_rest = self.sim.car.car.speed < 0.5 && spin.x * spin.x + spin.y * spin.y + spin.z * spin.z < 0.25;
        self.flipped = if body[1][1] < 0.2 && at_rest { self.flipped + 1 } else { 0 };
        if broken {
            self.sim.car.device.source.request(event::REBUILD);
        } else if body[3][1] < self.floor || body[3][1] > self.ceiling || self.flipped > FLIPPED_STEPS {
            self.sim.car.device.source.request(if self.sim.track.is_some() { event::TO_TRACK } else { event::RESET });
            self.flipped = 0;
        }
        let view = CarView::capture(&self.sim, (self.sim.steps - self.drive_start) as f64 * DT as f64);
        self.prev = if burst { view } else { self.curr };
        self.curr = view;
        self.total_steps += 1;
        Ok(())
    }

    /// One frame of the page: `seconds` since its last frame.
    pub fn advance(&mut self, seconds: f64) -> Result<FrameStats, String> {
        let mut stats = FrameStats::default();
        let seconds = if seconds.is_finite() { seconds.max(0.0) } else { 0.0 };
        let counted = seconds.min(MAX_FRAME_SECONDS);
        stats.dropped_seconds = seconds - counted;
        self.total_dropped_seconds += stats.dropped_seconds;
        self.accumulator += counted;
        while self.accumulator >= DT as f64 {
            self.accumulator -= DT as f64;
            self.step()?;
            stats.steps += 1;
        }
        Ok(stats)
    }

    /// The car between the last two steps, where the page's clock is now.
    pub fn view(&self) -> CarView {
        CarView::between(&self.prev, &self.curr, (self.accumulator / DT as f64) as f32)
    }

    /// The camera for `view`, `seconds` after the last frame's.
    pub fn camera_frame(&mut self, view: &CarView, seconds: f64) -> CameraFrame {
        let toggles = std::mem::take(&mut self.input.lock().unwrap().camera_toggles);
        for _ in 0..toggles {
            self.camera.f1();
        }
        self.camera.update(view, &self.shape, view.acc_g, seconds.clamp(0.0, MAX_FRAME_SECONDS) as f32)
    }

    /// R, Shift+R, N and the other one-off commands (`rustyac_game::input_file::event` bits).
    pub fn request(&mut self, events: u32) {
        self.input.lock().unwrap().requests |= events;
    }
}
