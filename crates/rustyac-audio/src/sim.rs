// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! What the game does around `CarAudioFMOD`, in other classes, and the sound depends on:
//!
//! * `BackfireParams` and `CarAvatar::update`'s backfire test, which fires the event the
//!   backfire sound listens to;
//! * the gear-change trigger of `CarAvatar` (`EventTriggerOnChange<int>`);
//! * `Sim::stepPhysicsEvent`: collisions from the physics thread become `onCarHit`;
//! * the part of `Sim::update` that ranks the cars by their distance to the listener;
//! * the order of all of it within one picture frame ([`AudioWorld::frame`]).

use rustyac_physics::data::ini::{append_path, IniReader};

use crate::car::{CarAudio, CarFrame, CarInfo, SimView};
use crate::engine::{AudioEngine, Mat, Vec3};
use crate::track::TrackAudio;

/// `BackfireParams` (0x28 bytes), from `sounds.ini [BACKFIRE]`.
#[derive(Clone, Copy, Debug)]
pub struct BackfireParams {
    pub max_gas: f32,
    new_max_gas: f32,
    pub min_rpm: f32,
    pub max_rpm: f32,
    pub trigger_gas: f32,
    new_trigger_gas: f32,
    last_gas: f32,
    trigger_ready: bool,
}

impl BackfireParams {
    /// `BackfireParams::BackfireParams` @ 0x1400cccb0
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    pub fn new(sounds_ini: &IniReader) -> Result<BackfireParams, String> {
        let mut p = BackfireParams {
            max_gas: 0.25,
            new_max_gas: 0.25,
            min_rpm: 9750.0,
            max_rpm: 24000.0,
            trigger_gas: 0.8,
            new_trigger_gas: 0.8,
            last_gas: 0.6,
            trigger_ready: false,
        };
        if sounds_ini.ready {
            p.max_gas = sounds_ini.get_float("BACKFIRE", "MAXGAS")?;
            if !(0.3 >= p.max_gas) {
                p.max_gas = 0.3;
            }
            p.min_rpm = sounds_ini.get_float("BACKFIRE", "MINRPM")?;
            p.max_rpm = sounds_ini.get_float("BACKFIRE", "MAXRPM")?;
            p.trigger_gas = sounds_ini.get_float("BACKFIRE", "TRIGGERGAS")?;
        }
        Ok(p)
    }

    /// `BackfireParams::checkBackfire` @ 0x1400d26b0: a lift after hard throttle, at revs, with
    /// "fuel in the exhaust" built up for more than a second.
    #[allow(clippy::neg_cmp_op_on_partial_ord, clippy::float_cmp)]
    pub fn check(&mut self, frame: &CarFrame, fuel_in_exhaust: &mut f32, ai_active: bool, dt: f32) -> bool {
        if 0.0 >= frame.fuel {
            return false;
        }
        if 0.0 >= frame.engine_life_left {
            return false;
        }
        let gas = frame.gas;
        if gas > self.last_gas && (gas < 0.0 || gas > 0.0) {
            self.last_gas = gas;
            self.new_trigger_gas = gas * self.trigger_gas;
            self.new_max_gas = self.max_gas * gas;
        }
        // comiss newTriggerGas,gas ; jb: below or unordered arms the trigger
        if !(self.new_trigger_gas >= frame.gas) {
            self.trigger_ready = true;
        }
        if !self.trigger_ready {
            return false;
        }
        let gas = frame.gas;
        let rpm = frame.engine_rpm;
        let all = !(gas >= self.new_max_gas) && !(gas.is_nan()) && (gas < 0.0 || gas > 0.0) && rpm > self.min_rpm && !(rpm > self.max_rpm) && !(1.0 >= *fuel_in_exhaust);
        if all {
            self.trigger_ready = false;
            if ai_active && 0.1 >= frame.brake {
                return false;
            }
            true
        } else {
            let x = dt + *fuel_in_exhaust;
            *fuel_in_exhaust = if !(x > 10.0) { x } else { 10.0 };
            false
        }
    }
}

/// A physics event as the engine queues it (`ACPhysicsEvent`, 0x48 bytes).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PhysicsEvent {
    pub kind: i32,
    pub param1: f32,
    pub param2: f32,
    pub param3: f32,
    pub param4: f32,
    pub v_param1: Vec3,
    pub v_param2: Vec3,
    pub ul_param0: u64,
}

impl PhysicsEvent {
    pub fn from_bytes(b: &[u8]) -> PhysicsEvent {
        let f = |at: usize| f32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]]);
        PhysicsEvent {
            kind: i32::from_le_bytes([b[0], b[1], b[2], b[3]]),
            param1: f(0x04),
            param2: f(0x08),
            param3: f(0x0c),
            param4: f(0x10),
            v_param1: [f(0x14), f(0x18), f(0x1c)],
            v_param2: [f(0x20), f(0x24), f(0x28)],
            ul_param0: u64::from_le_bytes([b[0x40], b[0x41], b[0x42], b[0x43], b[0x44], b[0x45], b[0x46], b[0x47]]),
        }
    }
}

/// One car's sound with the `CarAvatar` state around it.
pub struct CarSound {
    /// `None` for a car without a sound bank.
    pub audio: Option<CarAudio>,
    backfire: BackfireParams,
    /// `CarAvatar::fuelInExhaust`
    fuel_in_exhaust: f32,
    /// `evOnGearChanged.oldFloatValue`
    gear_old: i32,
    /// `aiState.isActive`
    pub ai_active: bool,
}

impl CarSound {
    /// What the `CarAvatar` constructors and `initCommonPostPhysics` do for the sound.
    pub fn new(engine: &mut AudioEngine, info: CarInfo) -> Result<CarSound, String> {
        let sounds = IniReader::load(&append_path(&info.data_folder, "sounds.ini"))?;
        let backfire = BackfireParams::new(&sounds)?;
        let audio = CarAudio::for_car(engine, info)?;
        Ok(CarSound { audio, backfire, fuel_in_exhaust: 0.0, gear_old: 0, ai_active: false })
    }

    /// The sound's share of `CarAvatar::update` @ 0x1400db830: the backfire test first, then
    /// the gear trigger (`EventTriggerOnChange<int>::update` @ 0x1400daef0).
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    pub fn avatar_update(&mut self, engine: &AudioEngine, frame: &CarFrame, sim: &SimView, dt: f32) {
        if !(0.0 >= frame.engine_life_left) && self.backfire.check(frame, &mut self.fuel_in_exhaust, self.ai_active, dt) {
            if let Some(audio) = self.audio.as_ref() {
                audio.on_backfire(engine, sim);
            }
        }
        let current = frame.gear;
        if current != self.gear_old {
            if let Some(audio) = self.audio.as_mut() {
                audio.on_gear_event(engine, sim, current as f32);
            }
            self.gear_old = current;
        }
    }
}

/// The sound of a session: the engine, the track's sounds and the cars', and one frame of it.
pub struct AudioWorld {
    pub engine: AudioEngine,
    pub track: Option<TrackAudio>,
    pub cars: Vec<CarSound>,
}

/// What one frame of the sound is given.
pub struct FrameInput<'a> {
    /// One per car, in the cars' order.
    pub cars: &'a [CarFrame],
    /// The physics events since the last frame, in the order the physics queued them.
    pub events: &'a [PhysicsEvent],
    pub sim: SimView,
    /// The camera's matrix and velocity (`AudioEngine::setListener`), when a camera sets one.
    pub listener: Option<(Mat, Vec3)>,
    /// The frame time in seconds.
    pub dt: f32,
}

impl AudioWorld {
    /// `Sim::stepPhysicsEvent` @ 0x14019ebd0: collision events go to the car they name.
    fn step_physics_events(&mut self, events: &[PhysicsEvent]) {
        for e in events {
            if e.kind != 0 {
                continue;
            }
            let index = e.param1 as i32;
            if index < 0 || index as usize >= self.cars.len() {
                continue;
            }
            if let Some(audio) = self.cars[index as usize].audio.as_mut() {
                audio.on_car_hit(&self.engine, &e.v_param1, e.param4, e.ul_param0);
            }
        }
    }

    /// The part of `Sim::update` @ 0x14019ef90 that ranks the connected cars by their distance
    /// to the listener: each gets its rank and the gap to the car before it.
    fn listener_priorities(&mut self, frames: &[CarFrame]) {
        let mut items: Vec<(usize, f32)> = Vec::new();
        for (i, frame) in frames.iter().enumerate() {
            if i < self.cars.len() && frame.node_active {
                let dist = self.engine.listener_distance(&frame.body_position);
                items.push((i, dist));
            }
        }
        // std::sort on `a.dist < b.dist` (an insertion sort for these sizes: stable)
        items.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
        let mut prio = 0;
        let mut prev = 0.0f32;
        for (i, dist) in items {
            if let Some(audio) = self.cars[i].audio.as_mut() {
                audio.listener_priority = prio;
                audio.listener_distance = dist - prev;
            }
            prev = dist;
            if !CarAudio::is_in_pit(&frames[i]) {
                prio += 1;
            }
        }
    }

    /// One picture frame, in the order of `Game::onIdle`: the update of the simulation
    /// (collision events, the cars' own updates, the ranking by distance), the camera's
    /// listener, the track's render, the cars' `renderAudio`, and the engine's update.
    pub fn frame(&mut self, input: &FrameInput) {
        self.step_physics_events(input.events);
        for (car, frame) in self.cars.iter_mut().zip(input.cars) {
            car.avatar_update(&self.engine, frame, &input.sim, input.dt);
        }
        self.listener_priorities(input.cars);
        if let Some((matrix, velocity)) = &input.listener {
            self.engine.set_listener(matrix, velocity);
        }
        if let Some(track) = self.track.as_mut() {
            track.render(&mut self.engine);
        }
        for (car, frame) in self.cars.iter_mut().zip(input.cars) {
            if let Some(audio) = car.audio.as_mut() {
                audio.render_audio(&mut self.engine, frame, &input.sim, input.dt);
            }
        }
        self.engine.update(input.dt);
    }
}
