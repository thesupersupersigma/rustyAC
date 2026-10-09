// SPDX-License-Identifier: GPL-3.0-or-later

//! `expf` of the game's C runtime (MSVCR120.dll), which `rustyac-math` does not carry: the
//! keyboard steering uses it. Taken from the DLL when the machine has it (it does wherever
//! AC is installed); otherwise Rust's `exp`, which can differ in the last bit.

#[cfg(windows)]
use std::ffi::c_void;
#[cfg(windows)]
use std::sync::OnceLock;

type F1 = unsafe extern "C" fn(f32) -> f32;

#[cfg(windows)]
#[link(name = "kernel32")]
extern "system" {
    fn LoadLibraryA(name: *const u8) -> *mut c_void;
    fn GetProcAddress(module: *mut c_void, name: *const u8) -> *mut c_void;
}

/// Not Windows: no DLL. `expf` is then `rustyac-math`'s, which in a wasm build is the DLL's
/// algorithm in plain Rust (bit-identical, see `docs/port/web.md`).
#[cfg(not(windows))]
fn runtime() -> Option<F1> {
    None
}

#[cfg(windows)]
fn runtime() -> Option<F1> {
    static EXPF: OnceLock<Option<F1>> = OnceLock::new();
    *EXPF.get_or_init(|| {
        if std::env::var("RUSTYAC_MATH").is_ok_and(|v| v.eq_ignore_ascii_case("std")) {
            return None;
        }
        // SAFETY: plain Win32 calls with terminated names; `expf` is the documented C function.
        unsafe {
            let module = LoadLibraryA(c"msvcr120.dll".as_ptr().cast());
            if module.is_null() {
                return None;
            }
            let p = GetProcAddress(module, c"expf".as_ptr().cast());
            (!p.is_null()).then(|| std::mem::transmute::<*mut c_void, F1>(p))
        }
    })
}

/// Is `expf` the game's own (the DLL's, or its algorithm in plain Rust)?
pub fn is_msvcr120() -> bool {
    runtime().is_some() || rustyac_math::backend() == rustyac_math::Backend::Pure && cfg!(not(windows))
}

/// `expf` (MSVCR120 when available).
pub fn expf(x: f32) -> f32 {
    match runtime() {
        // SAFETY: a C function of one float.
        Some(f) => unsafe { f(x) },
        #[cfg(windows)]
        None => x.exp(),
        #[cfg(not(windows))]
        None => rustyac_math::expf(x),
    }
}
