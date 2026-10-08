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
    // pluginsdkversion, name, version, numinputbuffers, numoutputbuffers, numparameters
    let count = word(0x60);
    let _ = write!(text, " {:#x} \"{name}\" {:#x} {} {} {count}", word(0), word(0x24), word(0x28), word(0x2c));
    let table = bytes.add(0x68).cast::<*const *const u8>().read_unaligned();
    for i in 0..count.min(16) as usize {
        // type, name, label, and the start of the union (a float's min, max, default and its
        // mapping type and point count; a bool's default; a data parameter's type)
        let p = table.add(i).read_unaligned();
        let kind = p.cast::<u32>().read_unaligned();
        let name = CStr::from_ptr(p.add(4).cast::<c_char>()).to_string_lossy();
        let label = CStr::from_ptr(p.add(0x14).cast::<c_char>()).to_string_lossy();
        let w = |offset: usize| p.add(offset).cast::<u32>().read_unaligned();
        let _ = write!(text, " [{kind} \"{name}\" \"{label}\" {:08x} {:08x} {:08x} {} {}]", w(0x30), w(0x34), w(0x38), w(0x40), w(0x48));
    }
}
