// SPDX-License-Identifier: MIT OR Apache-2.0

//! The C runtime functions the Vanilla physics calls, with AC's exact results.
//!
//! `acs.exe` imports `sinf`, `cosf`, `tanf`, `asinf`, `acosf`, `atanf`, `powf`, `sqrtf`, the
//! double-precision `sin` (the engine's camshaft-overlap term) and the number parsers
//! `wcstod` / `wcstol` from `MSVCR120.dll` (the Visual C++ 2013 runtime).
//! Rust's own `f32::sin` etc. end up in a different runtime (the UCRT). Measured on this
//! machine the two agree bit for bit (20 million random inputs per function, see the ignored
//! `std_vs_msvcr120` test), but nothing guarantees that for every input, CPU or future
//! toolchain. So on Windows the DLL the game itself uses is loaded at run time and its
//! functions are called directly: "same bits as AC" then holds by construction instead of
//! by observation.
//!
//! A second implementation, [`pure`], is the DLL's own algorithms ported to plain Rust
//! (bit-identical on all 2^32 inputs of every one-argument function, see `docs/port/web.md`).
//! It is what runs where the DLL cannot be had: **if `MSVCR120.dll` is not installed (or on
//! another OS) [`pure`] is used** and [`backend`] reports [`Backend::Pure`]; a wasm build
//! always uses it, because a browser cannot load a DLL. `RUSTYAC_MATH=pure` selects it by
//! hand. The desktop default stays the DLL itself.
//!
//! `RUSTYAC_MATH=no-dll` behaves as if the DLL were not installed (to try that fallback on a
//! machine that has it).
//!
//! The third, Rust's std functions ([`Backend::Std`]), is only used when asked for with
//! `RUSTYAC_MATH=std`, to compare. (It was the fallback up to v0.20.1.)
//!
//! `sqrtf` is not routed through the DLL: an IEEE square root has exactly one correct
//! result and both sides return it (checked by a test below).

use std::sync::OnceLock;

pub mod pure;
mod pure_tables;

/// Which implementation the runtime functions resolve to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Backend {
    /// `MSVCR120.dll`, the runtime `acs.exe` itself imports. Bit-exact with the game.
    Msvcr120,
    /// Rust std (UCRT on Windows). It is not the code the game runs and differs from it on a
    /// few inputs (`docs/port/web.md`, section 2.3). Only with `RUSTYAC_MATH=std`.
    Std,
    /// [`pure`]: MSVCR120's algorithms in plain Rust. What a wasm build runs, and what the
    /// desktop runs when the DLL is missing.
    Pure,
}

/// How the implementation in use was arrived at.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Choice {
    /// `MSVCR120.dll` was found and is used (the default).
    Dll,
    /// `MSVCR120.dll` was not found (or this is not Windows): [`pure`] is used instead.
    DllMissing,
    /// A wasm build: always [`pure`].
    Wasm,
    /// `RUSTYAC_MATH=pure` or `RUSTYAC_MATH=std` asked for it.
    Forced,
}

type F1 = unsafe extern "C" fn(f32) -> f32;
type F2 = unsafe extern "C" fn(f32, f32) -> f32;
type D1 = unsafe extern "C" fn(f64) -> f64;
type Wcstod = unsafe extern "C" fn(*const u16, *mut *mut u16) -> f64;
type Wcstol = unsafe extern "C" fn(*const u16, *mut *mut u16, i32) -> i32;
type Errno = unsafe extern "C" fn() -> *mut i32;

/// `ERANGE`, the errno value `wcstod` / `wcstol` report overflow with.
const ERANGE: i32 = 34;

struct Crt {
    backend: Backend,
    sinf: F1,
    cosf: F1,
    tanf: F1,
    expf: F1,
    asinf: F1,
    acosf: F1,
    atanf: F1,
    atan2f: F2,
    powf: F2,
    sin: D1,
    /// `wcstod`, `wcstol`, `_errno`: only with the real runtime; std has its own parser.
    parse: Option<(Wcstod, Wcstol, Errno)>,
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
unsafe extern "C" fn std_expf(x: f32) -> f32 {
    x.exp()
}
unsafe extern "C" fn std_asinf(x: f32) -> f32 {
    x.asin()
}
unsafe extern "C" fn std_acosf(x: f32) -> f32 {
    x.acos()
}
unsafe extern "C" fn std_atanf(x: f32) -> f32 {
    x.atan()
}
unsafe extern "C" fn std_atan2f(y: f32, x: f32) -> f32 {
    y.atan2(x)
}
unsafe extern "C" fn std_powf(x: f32, y: f32) -> f32 {
    x.powf(y)
}
unsafe extern "C" fn std_sin(x: f64) -> f64 {
    x.sin()
}

unsafe extern "C" fn pure_sinf(x: f32) -> f32 {
    pure::sinf(x)
}
unsafe extern "C" fn pure_cosf(x: f32) -> f32 {
    pure::cosf(x)
}
unsafe extern "C" fn pure_tanf(x: f32) -> f32 {
    pure::tanf(x)
}
unsafe extern "C" fn pure_expf(x: f32) -> f32 {
    pure::expf(x)
}
unsafe extern "C" fn pure_asinf(x: f32) -> f32 {
    pure::asinf(x)
}
unsafe extern "C" fn pure_acosf(x: f32) -> f32 {
    pure::acosf(x)
}
unsafe extern "C" fn pure_atanf(x: f32) -> f32 {
    pure::atanf(x)
}
unsafe extern "C" fn pure_atan2f(y: f32, x: f32) -> f32 {
    pure::atan2f(y, x)
}
unsafe extern "C" fn pure_powf(x: f32, y: f32) -> f32 {
    pure::powf(x, y)
}
unsafe extern "C" fn pure_sin(x: f64) -> f64 {
    pure::sin(x)
}

/// The number parsers stay Rust's with this backend (see [`wcstod`]).
const PURE: Crt = Crt {
    backend: Backend::Pure,
    sinf: pure_sinf,
    cosf: pure_cosf,
    tanf: pure_tanf,
    expf: pure_expf,
    asinf: pure_asinf,
    acosf: pure_acosf,
    atanf: pure_atanf,
    atan2f: pure_atan2f,
    powf: pure_powf,
    sin: pure_sin,
    parse: None,
};

const STD: Crt = Crt {
    backend: Backend::Std,
    sinf: std_sinf,
    cosf: std_cosf,
    tanf: std_tanf,
    expf: std_expf,
    asinf: std_asinf,
    acosf: std_acosf,
    atanf: std_atanf,
    atan2f: std_atan2f,
    powf: std_powf,
    sin: std_sin,
    parse: None,
};

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
        // SAFETY: plain Win32 calls with NUL-terminated names; the exports are the documented
        // C functions with the signatures of the `F1` / `F2` / `Wcstod` / ... aliases.
        unsafe {
            let module = LoadLibraryA(c"msvcr120.dll".as_ptr().cast());
            if module.is_null() {
                return None;
            }
            let get = |name: &std::ffi::CStr| {
                let p = GetProcAddress(module, name.as_ptr().cast());
                (!p.is_null()).then_some(p)
            };
            Some(Crt {
                backend: Backend::Msvcr120,
                sinf: std::mem::transmute::<*mut c_void, super::F1>(get(c"sinf")?),
                cosf: std::mem::transmute::<*mut c_void, super::F1>(get(c"cosf")?),
                tanf: std::mem::transmute::<*mut c_void, super::F1>(get(c"tanf")?),
                expf: std::mem::transmute::<*mut c_void, super::F1>(get(c"expf")?),
                asinf: std::mem::transmute::<*mut c_void, super::F1>(get(c"asinf")?),
                acosf: std::mem::transmute::<*mut c_void, super::F1>(get(c"acosf")?),
                atanf: std::mem::transmute::<*mut c_void, super::F1>(get(c"atanf")?),
                atan2f: std::mem::transmute::<*mut c_void, super::F2>(get(c"atan2f")?),
                powf: std::mem::transmute::<*mut c_void, super::F2>(get(c"powf")?),
                sin: std::mem::transmute::<*mut c_void, super::D1>(get(c"sin")?),
                parse: Some((
                    std::mem::transmute::<*mut c_void, super::Wcstod>(get(c"wcstod")?),
                    std::mem::transmute::<*mut c_void, super::Wcstol>(get(c"wcstol")?),
                    std::mem::transmute::<*mut c_void, super::Errno>(get(c"_errno")?),
                )),
            })
        }
    }
}

fn chosen() -> &'static (Crt, Choice) {
    static CRT: OnceLock<(Crt, Choice)> = OnceLock::new();
    CRT.get_or_init(|| {
        // no DLL and no environment in a browser
        if cfg!(target_family = "wasm") {
            return (PURE, Choice::Wasm);
        }
        let choice = std::env::var("RUSTYAC_MATH").unwrap_or_default();
        if choice.eq_ignore_ascii_case("std") {
            return (STD, Choice::Forced);
        }
        if choice.eq_ignore_ascii_case("pure") {
            return (PURE, Choice::Forced);
        }
        // (`RUSTYAC_MATH=no-dll`: as if the DLL were not installed, to try the fallback)
        #[cfg(windows)]
        if !choice.eq_ignore_ascii_case("no-dll") {
            if let Some(crt) = msvcr120::load() {
                return (crt, Choice::Dll);
            }
        }
        // the DLL's own algorithms, not another library's
        (PURE, Choice::DllMissing)
    })
}

fn crt() -> &'static Crt {
    &chosen().0
}

/// The implementation in use for this process.
pub fn backend() -> Backend {
    crt().backend
}

/// How it was chosen.
pub fn choice() -> Choice {
    chosen().1
}

/// One line for a console: which maths is used and why.
pub fn describe() -> String {
    match (backend(), choice()) {
        (Backend::Msvcr120, _) => "maths: MSVCR120.dll, the runtime Assetto Corsa itself uses".to_string(),
        (Backend::Pure, Choice::DllMissing) => {
            "maths: MSVCR120.dll (the Visual C++ 2013 runtime) was not found; its functions rewritten in Rust are used instead (rustyac_math::pure: the results the DLL gives on any processor since about 2013, bit for bit)".to_string()
        }
        (Backend::Pure, Choice::Wasm) => "maths: MSVCR120.dll's functions rewritten in Rust (rustyac_math::pure)".to_string(),
        (Backend::Pure, _) => "maths: MSVCR120.dll's functions rewritten in Rust (rustyac_math::pure), because RUSTYAC_MATH=pure".to_string(),
        (Backend::Std, _) => "maths: Rust's std, because RUSTYAC_MATH=std (not the game's maths: results can differ in the last digits)".to_string(),
    }
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

/// `expf` (MSVCR120).
#[inline]
pub fn expf(x: f32) -> f32 {
    // SAFETY: as `sinf`.
    unsafe { (crt().expf)(x) }
}

/// `asinf` (MSVCR120).
#[inline]
pub fn asinf(x: f32) -> f32 {
    // SAFETY: as `sinf`.
    unsafe { (crt().asinf)(x) }
}

/// `acosf` (MSVCR120).
#[inline]
pub fn acosf(x: f32) -> f32 {
    // SAFETY: as `sinf`.
    unsafe { (crt().acosf)(x) }
}

/// `atanf` (MSVCR120).
#[inline]
pub fn atanf(x: f32) -> f32 {
    // SAFETY: as `sinf`.
    unsafe { (crt().atanf)(x) }
}

/// `atan2f` (MSVCR120).
#[inline]
pub fn atan2f(y: f32, x: f32) -> f32 {
    // SAFETY: as `sinf`.
    unsafe { (crt().atan2f)(y, x) }
}

/// `powf` (MSVCR120).
#[inline]
pub fn powf(x: f32, y: f32) -> f32 {
    // SAFETY: as `sinf`.
    unsafe { (crt().powf)(x, y) }
}

/// `sin` (MSVCR120), double precision.
#[inline]
pub fn sin(x: f64) -> f64 {
    // SAFETY: as `sinf`.
    unsafe { (crt().sin)(x) }
}

/// `sqrtf`: correctly rounded on both sides, so std is used.
#[inline]
pub fn sqrtf(x: f32) -> f32 {
    x.sqrt()
}

/// `_fdtest(&x) > 0` as the game uses it: true for an infinity or a NaN.
#[inline]
pub fn fdtest_inf_or_nan(x: f32) -> bool {
    !x.is_finite()
}

/// `_dtest(&x) > 0`: the f64 version of [`fdtest_inf_or_nan`].
#[inline]
pub fn dtest_inf_or_nan(x: f64) -> bool {
    !x.is_finite()
}

/// What a C number parser returned.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Parsed<T> {
    pub value: T,
    /// How many UTF-16 units of the input were used, leading white space included.
    /// `0` means "no number here" (`std::stof` / `std::stoi` throw `invalid_argument`).
    pub consumed: usize,
    /// `errno == ERANGE` (`std::stof` / `std::stoi` throw `out_of_range`).
    pub out_of_range: bool,
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(Some(0)).collect()
}

/// `wcstod` (MSVCR120): parses the longest number at the start of `text`. This is what
/// `INIReader::getFloat` and `Curve::load` use (inlined `std::stof`), so "1.5 ; note" is
/// 1.5. The Visual C++ 2013 parser is not guaranteed to round the last bit like Rust's,
/// which is why the game's own function is called when the runtime is available.
pub fn wcstod(text: &str) -> Parsed<f64> {
    let Some((wcstod, _, errno)) = crt().parse else {
        return std_wcstod(text);
    };
    let buffer = wide(text);
    let mut end = std::ptr::null_mut();
    // SAFETY: `buffer` is NUL-terminated and outlives the call; `end` points into it.
    unsafe {
        *errno() = 0;
        let value = wcstod(buffer.as_ptr(), &mut end);
        Parsed {
            value,
            consumed: end.cast_const().offset_from(buffer.as_ptr()) as usize,
            out_of_range: *errno() == ERANGE,
        }
    }
}

/// `wcstol(text, &end, 10)` (MSVCR120), see [`wcstod`]. `long` is 32 bits on Windows.
pub fn wcstol(text: &str) -> Parsed<i32> {
    let Some((_, wcstol, errno)) = crt().parse else {
        return std_wcstol(text);
    };
    let buffer = wide(text);
    let mut end = std::ptr::null_mut();
    // SAFETY: as `wcstod`.
    unsafe {
        *errno() = 0;
        let value = wcstol(buffer.as_ptr(), &mut end, 10);
        Parsed {
            value,
            consumed: end.cast_const().offset_from(buffer.as_ptr()) as usize,
            out_of_range: *errno() == ERANGE,
        }
    }
}

/// C `iswspace` for the characters that can appear in a data file.
fn is_c_space(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\u{b}' | '\u{c}' | '\r')
}

/// Fallback for [`wcstod`] without MSVCR120: `[space] [sign] digits [. digits] [e|d exp]`.
/// Public so `tools/math_proof parse` can hold it against the runtime's parser.
pub fn std_wcstod(text: &str) -> Parsed<f64> {
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() && is_c_space(chars[i]) {
        i += 1;
    }
    let mut number = String::new();
    if i < chars.len() && (chars[i] == '+' || chars[i] == '-') {
        number.push(chars[i]);
        i += 1;
    }
    let mut digits = 0;
    let mut nonzero = false;
    let mut seen_point = false;
    while i < chars.len() && (chars[i].is_ascii_digit() || (chars[i] == '.' && !seen_point)) {
        seen_point |= chars[i] == '.';
        digits += chars[i].is_ascii_digit() as usize;
        nonzero |= matches!(chars[i], '1'..='9');
        number.push(chars[i]);
        i += 1;
    }
    if digits == 0 {
        return Parsed {
            value: 0.0,
            consumed: 0,
            out_of_range: false,
        };
    }
    if i < chars.len() && matches!(chars[i], 'e' | 'E' | 'd' | 'D') {
        let mut j = i + 1;
        let mut exponent = String::from("e");
        if j < chars.len() && (chars[j] == '+' || chars[j] == '-') {
            exponent.push(chars[j]);
            j += 1;
        }
        let first_digit = j;
        while j < chars.len() && chars[j].is_ascii_digit() {
            exponent.push(chars[j]);
            j += 1;
        }
        if j > first_digit {
            number.push_str(&exponent);
            i = j;
        }
    }
    let value: f64 = number.parse().unwrap_or(0.0);
    Parsed {
        value,
        consumed: chars[..i].iter().map(|c| c.len_utf16()).sum(),
        out_of_range: value.is_infinite() || (value == 0.0 && nonzero),
    }
}

/// Fallback for [`wcstol`] without MSVCR120.
pub fn std_wcstol(text: &str) -> Parsed<i32> {
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() && is_c_space(chars[i]) {
        i += 1;
    }
    let negative = i < chars.len() && chars[i] == '-';
    if i < chars.len() && (chars[i] == '+' || chars[i] == '-') {
        i += 1;
    }
    let first_digit = i;
    let mut value: i64 = 0;
    let mut out_of_range = false;
    while i < chars.len() && chars[i].is_ascii_digit() {
        value = value * 10 + chars[i] as i64 - '0' as i64;
        if value > i32::MAX as i64 + 1 {
            out_of_range = true;
            value = i32::MAX as i64 + 1;
        }
        i += 1;
    }
    if i == first_digit {
        return Parsed {
            value: 0,
            consumed: 0,
            out_of_range: false,
        };
    }
    let value = if negative { -value } else { value };
    out_of_range |= value > i32::MAX as i64;
    Parsed {
        value: value.clamp(i32::MIN as i64, i32::MAX as i64) as i32,
        consumed: chars[..i].iter().map(|c| c.len_utf16()).sum(),
        out_of_range,
    }
}

#[cfg(test)]
mod parse_tests {
    use super::*;

    #[test]
    fn number_prefix_parsing() {
        for parse in [wcstod, std_wcstod] {
            assert_eq!(parse("1.5 ; note").value, 1.5);
            assert_eq!(parse("1.5 ; note").consumed, 3);
            assert_eq!(parse("\t-0.25e1x").value, -2.5);
            assert_eq!(parse("abc").consumed, 0);
            assert_eq!(parse("").consumed, 0);
            assert_eq!(parse("7.").value, 7.0);
            assert_eq!(parse(".5").value, 0.5);
        }
        for parse in [wcstol, std_wcstol] {
            assert_eq!(parse("10").value, 10);
            assert_eq!(parse(" -3.9").value, -3);
            assert_eq!(parse("x").consumed, 0);
            assert!(parse("99999999999").out_of_range);
        }
    }
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
            let module = LoadLibraryA(c"msvcr120.dll".as_ptr().cast());
            if module.is_null() {
                eprintln!("msvcr120.dll not installed, skipping");
                return;
            }
            std::mem::transmute(GetProcAddress(module, c"sqrtf".as_ptr().cast()))
        };
        let mut state = 0x9e37_79b9_7f4a_7c15u64;
        for _ in 0..2_000_000 {
            state = state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            // every non-negative finite bit pattern class, including subnormals
            let x = f32::from_bits((state >> 33) as u32 & 0x7fff_ffff);
            if x.is_finite() {
                assert_eq!(
                    unsafe { sqrtf(x) }.to_bits(),
                    x.sqrt().to_bits(),
                    "sqrtf({x:e})"
                );
            }
        }
    }
}

#[cfg(all(test, windows))]
mod std_vs_msvcr120 {
    use super::*;

    /// Diagnostic, not a pass/fail check: how often Rust std (UCRT) differs from MSVCR120.
    /// Run with `cargo test --release -- --ignored --nocapture std_vs_msvcr120`.
    #[test]
    #[ignore]
    fn report() {
        let Some(crt) = msvcr120::load() else {
            eprintln!("msvcr120.dll not installed, skipping");
            return;
        };
        const N: usize = 20_000_000;
        let mut state = 0x1234_5678_9abc_def0u64;
        let mut unit = move || {
            state = state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (state >> 11) as f64 / (1u64 << 53) as f64
        };
        let mut diff = [0usize; 7];
        for _ in 0..N {
            // angles as the tyre model sees them, loads and exponents as getStaticDY does
            let angle = ((unit() - 0.5) * 3.2) as f32;
            let base = (unit() * 20000.0) as f32;
            let exponent = (0.3 + unit() * 2.7) as f32;
            let ratio = ((unit() - 0.5) * 2.0) as f32;
            unsafe {
                diff[0] += ((crt.sinf)(angle).to_bits() != angle.sin().to_bits()) as usize;
                diff[1] += ((crt.cosf)(angle).to_bits() != angle.cos().to_bits()) as usize;
                diff[2] += ((crt.tanf)(angle).to_bits() != angle.tan().to_bits()) as usize;
                diff[3] += ((crt.powf)(base, exponent).to_bits() != base.powf(exponent).to_bits())
                    as usize;
                diff[4] += ((crt.asinf)(ratio).to_bits() != ratio.asin().to_bits()) as usize;
                diff[5] += ((crt.acosf)(ratio).to_bits() != ratio.acos().to_bits()) as usize;
                diff[6] += ((crt.atanf)(angle).to_bits() != angle.atan().to_bits()) as usize;
            }
        }
        let names = ["sinf", "cosf", "tanf", "powf", "asinf", "acosf", "atanf"];
        for (name, count) in names.iter().zip(diff) {
            println!("{name}: {count} of {N} results differ between MSVCR120 and Rust std");
        }
    }
}
