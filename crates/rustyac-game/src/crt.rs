// SPDX-License-Identifier: GPL-3.0-or-later

//! `expf` of the game's C runtime (MSVCR120.dll) for the keyboard steering. Taken from the
//! DLL when `rustyac-math` uses the DLL (it does wherever AC is installed); otherwise it is
//! `rustyac-math`'s own: the DLL's algorithm in plain Rust when the DLL is missing or with
//! `RUSTYAC_MATH=pure`, Rust's `exp` only with `RUSTYAC_MATH=std`.

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
        // only where the maths crate itself uses the DLL (not with RUSTYAC_MATH=std or =pure)
        if rustyac_math::backend() != rustyac_math::Backend::Msvcr120 {
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
    runtime().is_some() || rustyac_math::backend() == rustyac_math::Backend::Pure
}

/// `expf` (the DLL's, or `rustyac-math`'s).
pub fn expf(x: f32) -> f32 {
    match runtime() {
        // SAFETY: a C function of one float.
        Some(f) => unsafe { f(x) },
        None => rustyac_math::expf(x),
    }
}
