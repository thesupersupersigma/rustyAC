//! The C runtime maths functions the Vanilla physics calls, with AC's exact results.
//!
//! `acs.exe` imports `sinf`, `cosf`, `tanf`, `powf` and `sqrtf` from `MSVCR120.dll` (the
//! Visual C++ 2013 runtime). Rust's own `f32::sin` etc. end up in a different runtime (the
//! UCRT), whose results differ from MSVCR120's in the last bit for some inputs. So on Windows
//! the same DLL the game uses is loaded at run time and its functions are called directly.
//!
//! If `MSVCR120.dll` is not installed (or on another OS) the std functions are used instead
//! and [`backend`] reports [`Backend::Std`]: the physics still runs, but is no longer
//! guaranteed bit-identical to AC. Setting the environment variable `RUSTYAC_MATH=std`
//! forces that fallback, to measure the difference.
//!
//! `sqrtf` is not routed through the DLL: an IEEE square root has exactly one correct
//! result and both sides return it (checked by a test below).

use std::sync::OnceLock;

/// Which implementation the transcendental functions resolve to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Backend {
    /// `MSVCR120.dll`, the runtime `acs.exe` itself imports. Bit-exact with the game.
    Msvcr120,
    /// Rust std (UCRT on Windows). Last-bit differences from the game are possible.
    Std,
}

type F1 = unsafe extern "C" fn(f32) -> f32;
type F2 = unsafe extern "C" fn(f32, f32) -> f32;

struct Crt {
    backend: Backend,
    sinf: F1,
    cosf: F1,
    tanf: F1,
    powf: F2,
}

unsafe extern "C" fn std_sinf(x: f32) -> f32 {
    x.sin()
}
unsafe extern "C" fn std_cosf(x: f32) -> f32 {
    x.cos()
}
unsafe extern "C" fn std_tanf(x: f32) -> f32 {
    x.tan()
}
unsafe extern "C" fn std_powf(x: f32, y: f32) -> f32 {
    x.powf(y)
}

const STD: Crt =
    Crt { backend: Backend::Std, sinf: std_sinf, cosf: std_cosf, tanf: std_tanf, powf: std_powf };

#[cfg(windows)]
mod msvcr120 {
    use super::{Backend, Crt};
    use std::ffi::c_void;

    #[link(name = "kernel32")]
    extern "system" {
        fn LoadLibraryA(name: *const u8) -> *mut c_void;
        fn GetProcAddress(module: *mut c_void, name: *const u8) -> *mut c_void;
    }

    pub fn load() -> Option<Crt> {
        // SAFETY: plain Win32 calls with NUL-terminated names; the four exports are the
        // documented C functions `float f(float)` / `float f(float, float)`.
        unsafe {
            let module = LoadLibraryA(b"msvcr120.dll\0".as_ptr());
            if module.is_null() {
                return None;
            }
            let get = |name: &[u8]| {
                let p = GetProcAddress(module, name.as_ptr());
                (!p.is_null()).then_some(p)
            };
            Some(Crt {
                backend: Backend::Msvcr120,
                sinf: std::mem::transmute::<*mut c_void, super::F1>(get(b"sinf\0")?),
                cosf: std::mem::transmute::<*mut c_void, super::F1>(get(b"cosf\0")?),
                tanf: std::mem::transmute::<*mut c_void, super::F1>(get(b"tanf\0")?),
                powf: std::mem::transmute::<*mut c_void, super::F2>(get(b"powf\0")?),
            })
        }
    }
}

fn crt() -> &'static Crt {
    static CRT: OnceLock<Crt> = OnceLock::new();
    CRT.get_or_init(|| {
        if std::env::var("RUSTYAC_MATH").is_ok_and(|v| v.eq_ignore_ascii_case("std")) {
            return STD;
        }
        #[cfg(windows)]
        if let Some(crt) = msvcr120::load() {
            return crt;
        }
        STD
    })
}

/// The implementation in use for this process.
pub fn backend() -> Backend {
    crt().backend
}

/// `sinf` (MSVCR120).
#[inline]
pub fn sinf(x: f32) -> f32 {
    // SAFETY: a pure C maths function taking and returning a float by value.
    unsafe { (crt().sinf)(x) }
}

/// `cosf` (MSVCR120).
#[inline]
pub fn cosf(x: f32) -> f32 {
    // SAFETY: as `sinf`.
    unsafe { (crt().cosf)(x) }
}

/// `tanf` (MSVCR120).
#[inline]
pub fn tanf(x: f32) -> f32 {
    // SAFETY: as `sinf`.
    unsafe { (crt().tanf)(x) }
}

/// `powf` (MSVCR120).
#[inline]
pub fn powf(x: f32, y: f32) -> f32 {
    // SAFETY: as `sinf`.
    unsafe { (crt().powf)(x, y) }
}

/// `sqrtf`: correctly rounded on both sides, so std is used.
#[inline]
pub fn sqrtf(x: f32) -> f32 {
    x.sqrt()
}

#[cfg(all(test, windows))]
mod tests {
    use std::ffi::c_void;

    #[link(name = "kernel32")]
    extern "system" {
        fn LoadLibraryA(name: *const u8) -> *mut c_void;
        fn GetProcAddress(module: *mut c_void, name: *const u8) -> *mut c_void;
    }

    /// MSVCR120's `sqrtf` and Rust's `f32::sqrt` agree bit for bit.
    #[test]
    fn sqrtf_matches_msvcr120() {
        let sqrtf: unsafe extern "C" fn(f32) -> f32 = unsafe {
            let module = LoadLibraryA(b"msvcr120.dll\0".as_ptr());
            if module.is_null() {
                eprintln!("msvcr120.dll not installed, skipping");
                return;
            }
            std::mem::transmute(GetProcAddress(module, b"sqrtf\0".as_ptr()))
        };
        let mut state = 0x9e37_79b9_7f4a_7c15u64;
        for _ in 0..2_000_000 {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            // every non-negative finite bit pattern class, including subnormals
            let x = f32::from_bits((state >> 33) as u32 & 0x7fff_ffff);
            if x.is_finite() {
                assert_eq!(unsafe { sqrtf(x) }.to_bits(), x.sqrt().to_bits(), "sqrtf({x:e})");
            }
        }
    }
}
