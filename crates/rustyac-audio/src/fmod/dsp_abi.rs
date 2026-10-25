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
    // by field, not offset: the same bytes on x64 (the 1.08 layout) and a layout of 4-byte
    // pointers in the browser
    let d = &*description.cast::<crate::dsp::DspDescription>();
    let name = CStr::from_ptr(d.name.as_ptr().cast::<c_char>()).to_string_lossy();
    // pluginsdkversion, name, version, numinputbuffers, numoutputbuffers, numparameters
    let count = d.numparameters as u32;
    let _ = write!(text, " {:#x} \"{name}\" {:#x} {} {} {count}", d.pluginsdkversion, d.version, d.numinputbuffers as u32, d.numoutputbuffers as u32);
    let table = d.paramdesc.cast::<*const u8>();
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
