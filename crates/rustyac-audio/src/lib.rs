// SPDX-License-Identifier: GPL-3.0-or-later

//! rustyAC's sound: Assetto Corsa's own FMOD Studio banks, played through the player's own
//! FMOD DLLs, driven by a 1:1 port of the game's sound code.
//!
//! * [`fmod`]: the binding to the player's `fmod64.dll` / `fmodstudio64.dll` and the call log.
//! * [`engine`]: AC's `AudioEngine`, `AudioEvent`, `AudioReverb`, `AudioOccluder`.
//! * [`dsp`]: the two DSP plug-ins the game registers.
//! * [`car`]: AC's `CarAudioFMOD`, the sounds of one car.
//! * [`track`]: AC's `TrackAudio` and the surface-sound pool.
//! * [`sim`]: what `CarAvatar` and `Sim` do for the sound, and the frame.
//! * [`tape`]: the recorded input of the oracle runs.

// The ported code keeps the game's comparisons as the machine code has them: "less or greater"
// is false for a NaN where "not equal" is true, and a negated comparison is not its opposite.
// Loops over the four wheels index several arrays, as the game's do.
// Products keep the operand order of the instruction (`x = k * x`).
#![allow(clippy::double_comparisons, clippy::neg_cmp_op_on_partial_ord, clippy::needless_range_loop, clippy::assign_op_pattern)]

pub mod car;
pub mod dsp;
pub mod engine;
pub mod fmod;
pub mod sim;
pub mod tape;
pub mod track;
