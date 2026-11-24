// SPDX-License-Identifier: GPL-3.0-or-later

//! `shm.rs` off Windows: no shared memory pages yet (step L5 publishes them in `/dev/shm`), so
//! [`SharedMemory::create`] refuses and the drive goes on without them. The flags the game's
//! loop sets for the pages are kept, unread.

use std::convert::Infallible;
use std::sync::atomic::{AtomicBool, AtomicU32};
use std::sync::Mutex;

use crate::input_file::StepInput;
use crate::physics_thread::StepSink;
use crate::sim::GameSim;

pub static IDEAL_LINE_ON: AtomicBool = AtomicBool::new(false);
pub static IN_REPLAY: AtomicBool = AtomicBool::new(false);
pub static REPLAY_TIME_MULT: AtomicU32 = AtomicU32::new(0x3f80_0000);
pub static REPLAY_FRAMES: Mutex<Vec<[f32; 3]>> = Mutex::new(Vec::new());

pub struct SharedMemory(Infallible);

impl SharedMemory {
    pub fn create() -> Result<SharedMemory, String> {
        Err("not off Windows yet (step L5)".to_string())
    }

    pub fn reused(&self) -> bool {
        match self.0 {}
    }
}

pub struct ShmSink(SharedMemory);

impl ShmSink {
    pub fn new(memory: SharedMemory) -> ShmSink {
        ShmSink(memory)
    }
}

impl StepSink for ShmSink {
    fn after_step(&mut self, _sim: &GameSim, _input: &StepInput) -> Result<(), String> {
        match self.0 .0 {}
    }
}
