// SPDX-License-Identifier: GPL-3.0-or-later

//! rustyAC's own binding to FMOD Studio 1.08.12, the version Assetto Corsa was built against.
//! The DLLs are the player's (`fmod64.dll`, `fmodstudio64.dll` in the AC folder), loaded at run
//! time; rustyAC ships no FMOD file and uses no FMOD header.

pub mod dsp_abi;
pub mod log;
pub mod raw;
pub mod types;
