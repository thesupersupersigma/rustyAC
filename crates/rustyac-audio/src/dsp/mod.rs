// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! The two DSP plug-ins acs.exe carries inside itself and registers with FMOD Studio, because
//! the sound banks use them: "FMOD Distance Filter" (fmod_distance_filter.obj) and "FMOD Gain"
//! (fmod_gain.obj). Ported instruction for instruction and registered the same way, with the
//! same descriptions.
//!
//! This file has the plug-in ABI of FMOD 1.08 as those two use it.

pub mod distance_filter;
pub mod gain;

use std::ffi::{c_char, c_void};
use std::sync::OnceLock;

/// `FMOD_DSP_STATE`, as far as the plug-ins look.
#[repr(C)]
pub struct DspState {
    pub instance: *mut c_void,
    pub plugindata: *mut c_void,
    pub channelmask: u32,
    pub source_speakermode: i32,
    pub sidechaindata: *mut f32,
    pub sidechainchannels: i32,
    pub callbacks: *const DspSystemCallbacks,
}

/// `FMOD_DSP_STATE_SYSTEMCALLBACKS`: FMOD's allocator and the sample rate.
#[repr(C)]
pub struct DspSystemCallbacks {
    pub alloc: unsafe extern "C" fn(size: u32, kind: u32, source: *const c_char) -> *mut c_void,
    pub realloc: *const c_void,
    pub free: unsafe extern "C" fn(pointer: *mut c_void, kind: u32, source: *const c_char),
    pub getsamplerate: unsafe extern "C" fn(state: *mut DspState, rate: *mut i32) -> i32,
}

/// `FMOD_DSP_DESCRIPTION` (0xd8 bytes). The callbacks are kept as addresses: each plug-in fills
/// in the ones it has.
#[repr(C)]
pub struct DspDescription {
    pub pluginsdkversion: u32,
    pub name: [u8; 32],
    pub version: u32,
    pub numinputbuffers: i32,
    pub numoutputbuffers: i32,
    pub create: usize,
    pub release: usize,
    pub reset: usize,
    pub read: usize,
    pub process: usize,
    pub setposition: usize,
    pub numparameters: i32,
    pub paramdesc: *const *const ParameterDesc,
    pub setparameterfloat: usize,
    pub setparameterint: usize,
    pub setparameterbool: usize,
    pub setparameterdata: usize,
    pub getparameterfloat: usize,
    pub getparameterint: usize,
    pub getparameterbool: usize,
    pub getparameterdata: usize,
    pub shouldiprocess: usize,
    pub userdata: *mut c_void,
    pub sys_register: usize,
    pub sys_deregister: usize,
    pub sys_mix: usize,
}

/// `FMOD_DSP_PARAMETER_DESC` (0x60 bytes), written by offset: type @0, name @4, label @0x14,
/// description @0x28, and the union @0x30.
#[repr(C, align(8))]
pub struct ParameterDesc(pub [u8; 0x60]);

// SAFETY: built once, never changed afterwards; the pointers inside point to statics.
unsafe impl Sync for DspDescription {}
unsafe impl Send for DspDescription {}
unsafe impl Sync for ParameterDesc {}
unsafe impl Send for ParameterDesc {}

pub const FMOD_ERR_DSP_DONTPROCESS: i32 = 6;
pub const FMOD_ERR_INVALID_PARAM: i32 = 0x1f;
pub const FMOD_ERR_MEMORY: i32 = 0x26;

impl ParameterDesc {
    /// `memset` to zero, the type, and `strncpy(.., 15)` of the name and the label.
    pub(crate) fn new(kind: i32, name: &str, label: &str, description: &'static std::ffi::CStr) -> ParameterDesc {
        let mut p = ParameterDesc([0; 0x60]);
        p.0[0..4].copy_from_slice(&kind.to_le_bytes());
        p.0[4..4 + name.len().min(15)].copy_from_slice(&name.as_bytes()[..name.len().min(15)]);
        p.0[0x14..0x14 + label.len().min(15)].copy_from_slice(&label.as_bytes()[..label.len().min(15)]);
        p.0[0x28..0x30].copy_from_slice(&(description.as_ptr() as usize).to_le_bytes());
        p
    }

    pub(crate) fn float(mut self, min: f32, max: f32, default: f32, mapping: i32) -> ParameterDesc {
        self.0[0x30..0x34].copy_from_slice(&min.to_le_bytes());
        self.0[0x34..0x38].copy_from_slice(&max.to_le_bytes());
        self.0[0x38..0x3c].copy_from_slice(&default.to_le_bytes());
        self.0[0x40..0x44].copy_from_slice(&mapping.to_le_bytes());
        self
    }

    pub(crate) fn points(mut self, values: &'static [f32], positions: &'static [f32]) -> ParameterDesc {
        self.0[0x48..0x4c].copy_from_slice(&(values.len() as i32).to_le_bytes());
        self.0[0x50..0x58].copy_from_slice(&(values.as_ptr() as usize).to_le_bytes());
        self.0[0x58..0x60].copy_from_slice(&(positions.as_ptr() as usize).to_le_bytes());
        self
    }

    pub(crate) fn word(mut self, offset: usize, value: i32) -> ParameterDesc {
        self.0[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        self
    }
}

pub(crate) fn name32(name: &str) -> [u8; 32] {
    let mut out = [0u8; 32];
    out[..name.len()].copy_from_slice(name.as_bytes());
    out
}

/// `shouldiprocess` of both plug-ins (one function in the game, 0x1401fc640): idle inputs are
/// not processed.
///
/// # Safety
/// Called by FMOD.
pub unsafe extern "C" fn should_i_process(_state: *mut DspState, inputs_idle: i32) -> i32 {
    if inputs_idle != 0 {
        FMOD_ERR_DSP_DONTPROCESS
    } else {
        0
    }
}

type Sprintf = unsafe extern "C" fn(*mut c_char, *const c_char, ...) -> i32;
type F1 = unsafe extern "C" fn(f32) -> f32;

/// `sprintf` and `log10f` of the game's C runtime (MSVCR120), when the machine has it.
struct Crt {
    sprintf: Option<Sprintf>,
    log10f: Option<F1>,
}

fn crt() -> &'static Crt {
    #[link(name = "kernel32")]
    extern "system" {
        fn LoadLibraryA(name: *const u8) -> *mut c_void;
        fn GetProcAddress(module: *mut c_void, name: *const u8) -> *mut c_void;
    }
    static CRT: OnceLock<Crt> = OnceLock::new();
    CRT.get_or_init(|| {
        if std::env::var("RUSTYAC_MATH").is_ok_and(|v| v.eq_ignore_ascii_case("std")) {
            return Crt { sprintf: None, log10f: None };
        }
        // SAFETY: plain Win32 calls with terminated names; documented C functions.
        unsafe {
            let module = LoadLibraryA(c"msvcr120.dll".as_ptr().cast());
            if module.is_null() {
                return Crt { sprintf: None, log10f: None };
            }
            let s = GetProcAddress(module, c"sprintf".as_ptr().cast());
            let l = GetProcAddress(module, c"log10f".as_ptr().cast());
            Crt {
                sprintf: (!s.is_null()).then(|| std::mem::transmute::<*mut c_void, Sprintf>(s)),
                log10f: (!l.is_null()).then(|| std::mem::transmute::<*mut c_void, F1>(l)),
            }
        }
    })
}

pub(crate) fn log10f(x: f32) -> f32 {
    match crt().log10f {
        // SAFETY: a C function of one float.
        Some(f) => unsafe { f(x) },
        None => x.log10(),
    }
}

/// `sprintf(out, format, (double)value)` with a format of one `%.1f`.
pub(crate) unsafe fn sprintf_f(out: *mut c_char, format: &std::ffi::CStr, value: f64) {
    match crt().sprintf {
        Some(f) => {
            f(out, format.as_ptr(), value);
        }
        None => {
            let text = format.to_string_lossy().replace("%.1f", &format!("{value:.1}"));
            copy_text(out, &text);
        }
    }
}

pub(crate) unsafe fn copy_text(out: *mut c_char, text: &str) {
    std::ptr::copy_nonoverlapping(text.as_ptr(), out.cast::<u8>(), text.len());
    *out.add(text.len()) = 0;
}
