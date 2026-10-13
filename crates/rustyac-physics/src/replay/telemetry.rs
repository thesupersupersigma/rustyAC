// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! `ComparativeTelemetryRecorder` (0x170 bytes): the player's gear, speed and pedals along the
//! lap in 3999 slots by the place on the track, for the lap that runs, the last lap and the
//! best one, and the best one's file (`.tc`).
//!
//! Constructor 0x1400731b0; the sample of every physics step (the handler 0x140073820 of
//! `Car::evOnStepComplete`, into a queue); `update` 0x140076850 (once a picture: the queue's
//! samples into their slots); the lap's end (the handler 0x1400736e0 of
//! `Car::evOnLapCompleted`); `saveBest` 0x140075d50 and `loadBest` 0x140074d30.

use std::collections::VecDeque;

use super::manager::{decode_utf8, encode_utf8};
use crate::curve::Curve;

/// The slots of a lap (`resize(3999)` in the constructor).
pub const SLOTS: usize = 3999;

/// `CTData` (20 bytes).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CtData {
    /// `drivetrain.currentGear` as it is (0 reverse, 1 neutral)
    pub gear: u32,
    /// `splineLocatorData.npos`, one physics step old
    pub normalized_position: f32,
    /// km/h
    pub speed: f32,
    pub gas: f32,
    pub brake: f32,
}

impl CtData {
    pub fn to_bytes(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&self.gear.to_le_bytes());
        for v in [self.normalized_position, self.speed, self.gas, self.brake] {
            out.extend_from_slice(&v.to_le_bytes());
        }
    }

    pub fn from_bytes(b: &[u8]) -> CtData {
        let f = |at: usize| f32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]]);
        CtData { gear: u32::from_le_bytes([b[0], b[1], b[2], b[3]]), normalized_position: f(4), speed: f(8), gas: f(12), brake: f(16) }
    }
}

/// The four names of a file.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TelemetryNames {
    pub driver: String,
    pub track: String,
    pub car: String,
    pub config: String,
}

/// `cvttss2si` into 64 bits with the low half taken: what does not fit is 0.
#[allow(clippy::manual_range_contains)]
fn low_half(x: f32) -> u32 {
    if x.is_nan() || x >= 9_223_372_036_854_775_808.0 || x < -9_223_372_036_854_775_808.0 {
        0
    } else {
        x as i64 as u32
    }
}

/// `ComparativeTelemetryRecorder`.
#[derive(Clone, Debug)]
pub struct ComparativeTelemetryRecorder {
    /// `psQueue`: the physics thread's samples the picture's thread has not taken yet
    pub queue: VecDeque<CtData>,
    pub current: Vec<CtData>,
    pub last: Vec<CtData>,
    pub best: Vec<CtData>,
    pub names: TelemetryNames,
    pub best_time: u32,
    pub last_time: u32,
    pub need_to_save: bool,
    pub last_recording_index: u32,
    pub recording_interval: f32,
}

impl ComparativeTelemetryRecorder {
    /// `ComparativeTelemetryRecorder::ComparativeTelemetryRecorder` 0x1400731b0, without the
    /// load of the best lap's file ([`ComparativeTelemetryRecorder::load_bytes`]).
    pub fn new(names: TelemetryNames) -> ComparativeTelemetryRecorder {
        ComparativeTelemetryRecorder {
            queue: VecDeque::new(),
            current: vec![CtData::default(); SLOTS],
            last: vec![CtData::default(); SLOTS],
            best: vec![CtData::default(); SLOTS],
            names,
            best_time: u32::MAX,
            last_time: u32::MAX,
            need_to_save: false,
            last_recording_index: 0,
            recording_interval: 0.00025,
        }
    }

    /// The handler 0x140073820 of `Car::evOnStepComplete`: a sample of every physics step.
    /// `speed_ms`: `Car::getSpeed`, m/s.
    pub fn on_step_complete(&mut self, gear: i32, normalized_position: f32, speed_ms: f32, gas: f32, brake: f32) {
        self.queue.push_back(CtData { gear: gear as u32, normalized_position, speed: speed_ms * 3.6, gas, brake });
    }

    /// `ComparativeTelemetryRecorder::update` 0x140076850: the waiting samples go into their
    /// slots. `lap_count`: `RaceManager::getLapCount` of the player's car now (while it is 0,
    /// the lap out of the pits, the samples are dropped).
    pub fn update(&mut self, lap_count: u32) {
        while let Some(s) = self.queue.pop_front() {
            if lap_count == 0 {
                continue;
            }
            let n = self.current.len();
            let x = n as f32 * (self.recording_interval * 0.5 + s.normalized_position);
            let idx = low_half(x);
            let mut last = self.last_recording_index;
            if u64::from(idx.wrapping_sub(last)) <= (n as u64).wrapping_sub(3) {
                while last < idx {
                    // (the game writes past its vector for a place beyond the lap's end)
                    if let Some(slot) = self.current.get_mut(self.last_recording_index as usize) {
                        *slot = s;
                    }
                    self.last_recording_index += 1;
                    last = self.last_recording_index;
                }
            }
        }
    }

    /// The handler 0x1400736e0 of `Car::evOnLapCompleted` (`lap_time`, `lap_count`: the
    /// event's). True: the lap is the new best.
    pub fn on_lap_completed(&mut self, lap_time: u32, lap_count: u32) -> bool {
        // the slots the lap did not reach get the next slot's old content
        let n = self.current.len();
        while (self.last_recording_index as usize) < n.saturating_sub(1) {
            let at = self.last_recording_index as usize;
            self.current[at + 1] = self.current[at];
            self.last_recording_index += 1;
        }
        let mut new_best = false;
        if lap_count != 0 {
            if lap_time < self.best_time {
                self.need_to_save = true;
                self.best = self.current.clone();
                self.best_time = lap_time;
                new_best = true;
            }
            self.last = self.current.clone();
            self.last_time = lap_time;
        }
        self.last_recording_index = 0;
        new_best
    }

    /// The file below the telemetry folder: `player/<driver>_<track>_<layout>_<car>.tc` (the
    /// layout is always there, empty or not).
    pub fn file_name(names: &TelemetryNames) -> String {
        format!("player/{}_{}_{}_{}.tc", names.driver, names.track, names.config, names.car)
    }

    /// `ComparativeTelemetryRecorder::saveBest` 0x140075d50: the file's bytes when there is a
    /// new best lap. The best lap is laid out anew first, at even steps of the place on the
    /// track (the lap in memory changes with it, as in the game).
    pub fn save_bytes(&mut self) -> Option<Vec<u8>> {
        if !self.need_to_save {
            return None;
        }
        let (mut brake, mut gas, mut speed, mut gear) = (Curve::new(), Curve::new(), Curve::new(), Curve::new());
        for d in &self.best {
            let x = d.normalized_position;
            brake.add_value(x, d.brake);
            gas.add_value(x, d.gas);
            speed.add_value(x, d.speed);
            gear.add_value(x, d.gear as i32 as f32);
        }
        let mut x = 0.0f32;
        for d in &mut self.best {
            d.brake = brake.get_value(x);
            d.gas = gas.get_value(x);
            d.speed = speed.get_value(x);
            let g = gear.get_value(x);
            d.gear = low_half(g);
            x += self.recording_interval;
        }
        let mut out = Vec::new();
        out.extend_from_slice(&1i32.to_le_bytes());
        for text in [&self.names.driver, &self.names.track, &self.names.car, &self.names.config] {
            let bytes = encode_utf8(text);
            out.extend_from_slice(&(bytes.len() as i32).to_le_bytes());
            out.extend_from_slice(&bytes);
        }
        out.extend_from_slice(&self.best_time.to_le_bytes());
        out.extend_from_slice(&(self.best.len() as i32).to_le_bytes());
        for d in &self.best {
            d.to_bytes(&mut out);
        }
        Some(out)
    }

    /// `ComparativeTelemetryRecorder::loadBest` 0x140074d30 on a file's bytes: the best lap of
    /// an earlier run, when the file is of this driver, track, car and layout.
    pub fn load_bytes(&mut self, bytes: &[u8]) -> bool {
        let mut at = 0usize;
        let int = |at: &mut usize| -> Option<i32> {
            let raw = bytes.get(*at..*at + 4)?;
            *at += 4;
            Some(i32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]))
        };
        let Some(_version) = int(&mut at) else { return false };
        let mut names = Vec::new();
        for _ in 0..4 {
            let Some(len) = int(&mut at).and_then(|len| usize::try_from(len).ok()) else { return false };
            let Some(text) = bytes.get(at..at.saturating_add(len)) else { return false };
            at += len;
            names.push(decode_utf8(text));
        }
        if names[0] != self.names.driver || names[1] != self.names.track || names[2] != self.names.car || names[3] != self.names.config {
            return false;
        }
        let Some(time) = int(&mut at) else { return false };
        self.best_time = time as u32;
        let Some(count) = int(&mut at) else { return false };
        // (the game reads a longer file past its vector; the port stops at the lap's slots)
        for i in 0..(count.max(0) as usize).min(self.best.len()) {
            let Some(raw) = bytes.get(at..at + 20) else { return false };
            at += 20;
            self.best[i] = CtData::from_bytes(raw);
        }
        true
    }
}
