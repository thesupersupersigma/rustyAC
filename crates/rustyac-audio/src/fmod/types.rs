// SPDX-License-Identifier: GPL-3.0-or-later

//! The few FMOD 1.08 data types that cross the API, laid out as the 1.08 ABI has them (written
//! from the way acs.exe fills and reads them; no FMOD header is used or shipped).

use std::ffi::c_void;

/// The version acs.exe insists on (`System::getVersion`): FMOD 1.08.12.
pub const FMOD_VERSION: u32 = 0x0001_0812;

pub const FMOD_OK: i32 = 0;

/// `FMOD_OUTPUTTYPE`
pub const OUTPUTTYPE_NOSOUND: i32 = 2;
pub const OUTPUTTYPE_NOSOUND_NRT: i32 = 4;
pub const OUTPUTTYPE_WAVWRITER_NRT: i32 = 5;

/// `FMOD_SPEAKERMODE`
pub const SPEAKERMODE_STEREO: i32 = 3;

/// `FMOD_STUDIO_INITFLAGS`
pub const STUDIO_INIT_NORMAL: u32 = 0;
pub const STUDIO_INIT_LIVEUPDATE: u32 = 1;
pub const STUDIO_INIT_SYNCHRONOUS_UPDATE: u32 = 4;

/// `FMOD_STUDIO_PLAYBACK_STATE`
pub const PLAYBACK_PLAYING: i32 = 0;
pub const PLAYBACK_SUSTAINING: i32 = 1;
pub const PLAYBACK_STOPPED: i32 = 2;
pub const PLAYBACK_STARTING: i32 = 3;
pub const PLAYBACK_STOPPING: i32 = 4;

/// `FMOD_STUDIO_STOP_MODE`
pub const STOP_ALLOWFADEOUT: i32 = 0;
pub const STOP_IMMEDIATE: i32 = 1;

/// An opaque FMOD object as the API hands it out.
pub type RawHandle = *mut c_void;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vector {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

/// `FMOD_3D_ATTRIBUTES`
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Attributes3d {
    pub position: Vector,
    pub velocity: Vector,
    pub forward: Vector,
    pub up: Vector,
}

/// `FMOD_GUID`
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Guid {
    pub data1: u32,
    pub data2: u16,
    pub data3: u16,
    pub data4: [u8; 8],
}

/// `FMOD_REVERB_PROPERTIES` (1.08: twelve floats)
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ReverbProperties {
    pub decay_time: f32,
    pub early_delay: f32,
    pub late_delay: f32,
    pub hf_reference: f32,
    pub hf_decay_ratio: f32,
    pub diffusion: f32,
    pub density: f32,
    pub low_shelf_frequency: f32,
    pub low_shelf_gain: f32,
    pub high_cut: f32,
    pub early_late_mix: f32,
    pub wet_level: f32,
}

/// Plain data that the call log prints as raw bytes.
///
/// # Safety
/// Only for `repr(C)` types without padding and without pointers.
pub unsafe trait Pod: Copy {}
unsafe impl Pod for Vector {}
unsafe impl Pod for Attributes3d {}
unsafe impl Pod for Guid {}
unsafe impl Pod for ReverbProperties {}
unsafe impl Pod for f32 {}
unsafe impl Pod for i32 {}
unsafe impl Pod for u32 {}
unsafe impl Pod for bool {}
