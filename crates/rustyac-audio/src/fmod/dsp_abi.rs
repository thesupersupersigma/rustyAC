// SPDX-License-Identifier: GPL-3.0-or-later

//! The FMOD 1.08 DSP plug-in ABI: the description a plug-in hands to
//! `Studio::System::registerPlugin`, and the state FMOD passes to its callbacks.

use std::ffi::{c_char, c_void, CStr};
use std::fmt::Write as _;

/// Appends the plain members of an `FMOD_DSP_DESCRIPTION` to a log line.
///
/// # Safety
/// `description` must point to a description of the 1.08 layout.
pub(crate) unsafe fn describe(description: *const c_void, text: &mut String) {
    let bytes = description.cast::<u8>();
    let name = CStr::from_ptr(bytes.add(4).cast::<c_char>()).to_string_lossy();
    let word = |offset: usize| bytes.add(offset).cast::<u32>().read_unaligned();
    let _ = write!(text, " {:#x} \"{name}\" {:#x} {} {} {}", word(0), word(36), word(40), word(44), word(96));
}
