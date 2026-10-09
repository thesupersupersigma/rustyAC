// SPDX-License-Identifier: MIT OR Apache-2.0

//! Holds `rustyac_math::pure` (MSVCR120's maths in plain Rust) against the real
//! `MSVCR120.dll` of this PC.
//!
//! ```text
//! math_proof all [--threads N] [--pairs N] [--out <file.md>]   every check, a results table
//! math_proof one <function> [...]                             a single function
//! math_proof fma3-off                                         the same, with the DLL's FMA3
//!                                                             paths switched off (what a CPU
//!                                                             older than 2013 computes)
//! math_proof parse <folder>...                                 the number parsers (`wcstod`,
//!                                                             `wcstol`) against the Rust
//!                                                             fallback, on every number in
//!                                                             every text file below
//! math_proof digest [--stride N]                              pure only: a hash of the results
//!                                                             over a fixed input set (also
//!                                                             builds for wasm32-wasip1, to
//!                                                             compare a wasm build with native)
//! ```
//!
//! One-argument functions get every one of the 2^32 bit patterns. `atan2f` and `powf` get
//! `--pairs` random pairs (default 1.6e9) drawn four ways, plus the cross product of a list
//! of special values. The double `sin` gets every float widened to a double, random doubles
//! and the range the physics uses. Bits are compared, so a NaN with another payload counts.

use std::fmt::Write as _;

use rustyac_math::pure;

type F1 = unsafe extern "C" fn(f32) -> f32;
type F2 = unsafe extern "C" fn(f32, f32) -> f32;
type D1 = unsafe extern "C" fn(f64) -> f64;

#[cfg(windows)]
mod dll {
    use super::{D1, F1, F2};
    use std::ffi::c_void;

    #[link(name = "kernel32")]
    extern "system" {
        fn LoadLibraryA(name: *const u8) -> *mut c_void;
        fn GetProcAddress(module: *mut c_void, name: *const u8) -> *mut c_void;
    }

    #[derive(Clone, Copy)]
    pub struct Dll {
        pub one: [(&'static str, F1); 7],
        pub atan2f: F2,
        pub powf: F2,
        pub sin: D1,
        pub set_fma3: unsafe extern "C" fn(i32) -> i32,
    }

    pub fn load() -> Option<Dll> {
        // SAFETY: plain Win32 calls with NUL-terminated names; the exports are the documented
        // C functions.
        unsafe {
            let module = LoadLibraryA(c"msvcr120.dll".as_ptr().cast());
            if module.is_null() {
                return None;
            }
            let get = |name: &std::ffi::CStr| {
                let p = GetProcAddress(module, name.as_ptr().cast());
                assert!(!p.is_null(), "msvcr120.dll has no {name:?}");
                p
            };
            let f1 = |name| std::mem::transmute::<*mut c_void, F1>(get(name));
            Some(Dll {
                one: [
                    ("sinf", f1(c"sinf")),
                    ("cosf", f1(c"cosf")),
                    ("tanf", f1(c"tanf")),
                    ("expf", f1(c"expf")),
                    ("asinf", f1(c"asinf")),
                    ("acosf", f1(c"acosf")),
                    ("atanf", f1(c"atanf")),
                ],
                atan2f: std::mem::transmute::<*mut c_void, F2>(get(c"atan2f")),
                powf: std::mem::transmute::<*mut c_void, F2>(get(c"powf")),
                sin: std::mem::transmute::<*mut c_void, D1>(get(c"sin")),
                set_fma3: std::mem::transmute::<*mut c_void, unsafe extern "C" fn(i32) -> i32>(
                    get(c"_set_FMA3_enable"),
                ),
            })
        }
    }
}

type Pure1 = fn(f32) -> f32;

const PURE_ONE: [(&str, Pure1); 7] = [
    ("sinf", pure::sinf),
    ("cosf", pure::cosf),
    ("tanf", pure::tanf),
    ("expf", pure::expf),
    ("asinf", pure::asinf),
    ("acosf", pure::acosf),
    ("atanf", pure::atanf),
];

/// What one comparison run found.
#[derive(Default, Clone)]
struct Stats {
    total: u64,
    different: u64,
    /// Of `different`: both results are NaNs (only sign or payload differ).
    nan_only: u64,
    examples: Vec<String>,
}

impl Stats {
    fn add(&mut self, other: Stats) {
        self.total += other.total;
        self.different += other.different;
        self.nan_only += other.nan_only;
        for e in other.examples {
            if self.examples.len() < 8 {
                self.examples.push(e);
            }
        }
    }

    fn note32(&mut self, input: impl Fn() -> String, want: f32, got: f32) {
        self.total += 1;
        if want.to_bits() != got.to_bits() {
            self.different += 1;
            self.nan_only += (want.is_nan() && got.is_nan()) as u64;
            if self.examples.len() < 8 {
                self.examples.push(format!(
                    "{}: MSVCR120 {:08x}, pure {:08x}",
                    input(),
                    want.to_bits(),
                    got.to_bits()
                ));
            }
        }
    }

    fn note64(&mut self, input: f64, want: f64, got: f64) {
        self.total += 1;
        if want.to_bits() != got.to_bits() {
            self.different += 1;
            self.nan_only += (want.is_nan() && got.is_nan()) as u64;
            if self.examples.len() < 8 {
                self.examples.push(format!(
                    "{:016x}: MSVCR120 {:016x}, pure {:016x}",
                    input.to_bits(),
                    want.to_bits(),
                    got.to_bits()
                ));
            }
        }
    }
}

/// splitmix64.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    /// Uniform in `[0, 1)`.
    fn unit(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }

    fn range(&mut self, low: f64, high: f64) -> f64 {
        low + self.unit() * (high - low)
    }

    fn bits32(&mut self) -> f32 {
        f32::from_bits(self.next() as u32)
    }

    /// A float near something the code branches on: a chosen exponent with a mantissa that is
    /// zero, all ones, one bit, or random.
    fn edgy(&mut self) -> f32 {
        const EXPONENTS: [u32; 28] = [
            0, 1, 2, 22, 23, 24, 100, 101, 113, 114, 120, 121, 122, 123, 124, 125, 126, 127, 128,
            129, 130, 133, 134, 150, 151, 152, 254, 255,
        ];
        let r = self.next();
        let exponent = EXPONENTS[(r % 28) as usize];
        let mantissa = match (r >> 8) % 6 {
            0 => 0,
            1 => 0x7f_ffff,
            2 => 1 << ((r >> 16) % 23),
            3 => 0x7f_ffff ^ (1 << ((r >> 16) % 23)),
            _ => (r >> 32) as u32 & 0x7f_ffff,
        };
        f32::from_bits(((r >> 63) as u32) << 31 | exponent << 23 | mantissa)
    }
}

/// The values every two-argument function sees in all combinations.
fn specials() -> Vec<f32> {
    let mut list = Vec::new();
    let magnitudes: [u32; 52] = [
        0x0000_0000,
        0x0000_0001,
        0x0000_0002,
        0x0040_0000,
        0x007f_ffff,
        0x0080_0000,
        0x0080_0001,
        0x3300_0000,
        0x3980_0000,
        0x3e80_0000,
        0x3eff_ffff,
        0x3f00_0000,
        0x3f00_0001,
        0x3f7f_ffff,
        0x3f80_0000,
        0x3f80_0001,
        0x3f87_ffff,
        0x3f88_0000,
        0x3f88_0001,
        0x3fc0_0000,
        0x3fff_ffff,
        0x4000_0000,
        0x4000_0001,
        0x4040_0000,
        0x4080_0000,
        0x40a0_0000,
        0x4120_0000,
        0x42fe_0000,
        0x4300_0000,
        0x4380_0000,
        0x4a7f_fffc,
        0x4aff_fffe,
        0x4b00_0000,
        0x4b00_0001,
        0x4b7f_ffff,
        0x4b80_0000,
        0x4b80_0001,
        0x4c00_0000,
        0x4f00_0000,
        0x4f80_0000,
        0x5f00_0000,
        0x7e00_0000,
        0x7f7f_fffe,
        0x7f7f_ffff,
        0x7f80_0000,
        0x7f80_0001,
        0x7fa0_0000,
        0x7fbf_ffff,
        0x7fc0_0000,
        0x7fc0_0001,
        0x7fe0_1234,
        0x7fff_ffff,
    ];
    for m in magnitudes {
        list.push(f32::from_bits(m));
        list.push(f32::from_bits(m | 0x8000_0000));
    }
    for i in 1..=40 {
        for v in [
            i as f32,
            i as f32 + 0.5,
            1.0 / i as f32,
            1.0 + i as f32 / 1024.0,
        ] {
            list.push(v);
            list.push(-v);
        }
    }
    list
}

fn threads_default() -> usize {
    std::thread::available_parallelism().map_or(4, |n| n.get())
}

fn split<T: Send>(threads: usize, job: impl Fn(usize) -> T + Sync) -> Vec<T> {
    std::thread::scope(|scope| {
        let handles: Vec<_> = (0..threads)
            .map(|t| {
                let job = &job;
                scope.spawn(move || job(t))
            })
            .collect();
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    })
}

#[cfg(windows)]
fn exhaustive(name: &str, want: F1, got: fn(f32) -> f32, threads: usize) -> Stats {
    let chunk = (1u64 << 32) / threads as u64;
    let mut stats = Stats::default();
    for part in split(threads, |t| {
        let start = t as u64 * chunk;
        let end = if t + 1 == threads {
            1u64 << 32
        } else {
            start + chunk
        };
        let mut s = Stats::default();
        for bits in start..end {
            let x = f32::from_bits(bits as u32);
            // SAFETY: a C maths function taking and returning a float by value.
            let w = unsafe { want(x) };
            s.note32(|| format!("{name}({:08x})", bits as u32), w, got(x));
        }
        s
    }) {
        stats.add(part);
    }
    stats
}

/// How a random pair is drawn; `kind` cycles through four families.
fn pair(rng: &mut Rng, kind: u64, pow: bool) -> (f32, f32) {
    match kind % 4 {
        0 => (rng.bits32(), rng.bits32()),
        1 if pow => (rng.range(0.0, 20000.0) as f32, rng.range(-4.0, 8.0) as f32),
        1 => (
            rng.range(-1000.0, 1000.0) as f32,
            rng.range(-1000.0, 1000.0) as f32,
        ),
        2 => (rng.edgy(), rng.edgy()),
        _ if pow => {
            // a base of any size (often negative with a whole exponent, often next to 1) and
            // an exponent that puts the result near the overflow or underflow edge
            let r = rng.next();
            let mut base = match r % 4 {
                0 => rng.range(0.9, 1.1) as f32,
                1 => (2.0f64.powf(rng.range(-149.0, 128.0))) as f32,
                2 => rng.range(0.0, 3.0) as f32,
                _ => rng.bits32().abs(),
            };
            let ln = (base as f64).ln();
            let target = match (r >> 8) % 4 {
                0 => rng.range(88.0, 89.5),
                1 => rng.range(-104.5, -102.5),
                2 => rng.range(-103.3, 88.8),
                _ => rng.range(-1.0, 1.0),
            };
            let mut exponent = if ln == 0.0 { 1.0 } else { target / ln };
            if (r >> 16).is_multiple_of(3) {
                exponent = exponent.round();
                if (r >> 20) & 1 == 0 {
                    base = -base;
                }
            }
            (base, exponent as f32)
        }
        _ => {
            // any two magnitudes with a chosen exponent gap (the function branches on it)
            let a = rng.bits32();
            let gap = rng.range(-160.0, 160.0);
            let b = (a as f64 * 2.0f64.powf(gap) * rng.range(0.5, 2.0)) as f32;
            if rng.next() & 1 == 0 {
                (a, -b)
            } else {
                (a, b)
            }
        }
    }
}

#[cfg(windows)]
fn pairs(
    name: &str,
    want: F2,
    got: fn(f32, f32) -> f32,
    pow: bool,
    count: u64,
    threads: usize,
) -> Stats {
    let mut stats = Stats::default();
    let list = specials();
    for &a in &list {
        for &b in &list {
            // SAFETY: as `exhaustive`.
            let w = unsafe { want(a, b) };
            stats.note32(
                || format!("{name}({:08x}, {:08x})", a.to_bits(), b.to_bits()),
                w,
                got(a, b),
            );
        }
    }
    let each = count / threads as u64;
    for part in split(threads, |t| {
        let mut rng = Rng(0x20a_0000 + t as u64 * 7919 + pow as u64);
        let mut s = Stats::default();
        for i in 0..each {
            let (a, b) = pair(&mut rng, i, pow);
            // SAFETY: as `exhaustive`.
            let w = unsafe { want(a, b) };
            s.note32(
                || format!("{name}({:08x}, {:08x})", a.to_bits(), b.to_bits()),
                w,
                got(a, b),
            );
        }
        s
    }) {
        stats.add(part);
    }
    stats
}

/// The doubles `sin` is tried on: `kind` 0 = every float widened, 1 = random bit patterns,
/// 2 = the range the physics uses (seconds times 0.1), 3 = up to the large-argument switch
/// and past it.
#[cfg(windows)]
fn sin_double(want: D1, count: u64, threads: usize) -> [Stats; 4] {
    let mut all: [Stats; 4] = Default::default();
    let chunk = (1u64 << 32) / threads as u64;
    let each = count / threads as u64;
    for part in split(threads, |t| {
        let mut s: [Stats; 4] = Default::default();
        let start = t as u64 * chunk;
        let end = if t + 1 == threads {
            1u64 << 32
        } else {
            start + chunk
        };
        for bits in start..end {
            let x = f32::from_bits(bits as u32) as f64;
            // SAFETY: as `exhaustive`.
            s[0].note64(x, unsafe { want(x) }, pure::sin(x));
        }
        let mut rng = Rng(0x51_0000 + t as u64 * 104729);
        for _ in 0..each {
            let x = f64::from_bits(rng.next());
            // SAFETY: as `exhaustive`.
            s[1].note64(x, unsafe { want(x) }, pure::sin(x));
            let x = rng.range(-1.0e6, 1.0e6);
            // SAFETY: as `exhaustive`.
            s[2].note64(x, unsafe { want(x) }, pure::sin(x));
            let x =
                2.0f64.powf(rng.range(-30.0, 40.0)) * if rng.next() & 1 == 0 { 1.0 } else { -1.0 };
            // SAFETY: as `exhaustive`.
            s[3].note64(x, unsafe { want(x) }, pure::sin(x));
        }
        s
    }) {
        for (a, p) in all.iter_mut().zip(part) {
            a.add(p);
        }
    }
    all
}

fn row(out: &mut String, name: &str, what: &str, s: &Stats) {
    let verdict = if s.different == 0 {
        "identical".to_string()
    } else {
        format!(
            "**{} differ** ({} of them NaN against NaN)",
            s.different, s.nan_only
        )
    };
    let _ = writeln!(out, "| `{name}` | {what} | {} | {verdict} |", s.total);
    println!("{name:8} {what}: {} inputs, {}", s.total, verdict);
    for e in &s.examples {
        println!("    {e}");
    }
}

/// FNV-1a over result bits.
fn fnv(hash: &mut u64, bits: u64) {
    for byte in bits.to_le_bytes() {
        *hash ^= byte as u64;
        *hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
}

/// `digest`: results of the pure functions over a fixed input set, as one hash per function.
/// Needs no DLL, so the same command under wasm must print the same lines.
fn digest(stride: u64) {
    println!("pure maths digest, every {stride}th float and 2,000,000 pairs / doubles:");
    for (name, f) in PURE_ONE {
        let mut hash = 0xcbf2_9ce4_8422_2325u64;
        let mut bits = 0u64;
        while bits < 1 << 32 {
            fnv(&mut hash, f(f32::from_bits(bits as u32)).to_bits() as u64);
            bits += stride;
        }
        println!("{name:8} {hash:016x}");
    }
    for (name, f, pow) in [
        ("atan2f", pure::atan2f as fn(f32, f32) -> f32, false),
        ("powf", pure::powf as fn(f32, f32) -> f32, true),
    ] {
        let mut hash = 0xcbf2_9ce4_8422_2325u64;
        let list = specials();
        for &a in &list {
            for &b in &list {
                fnv(&mut hash, f(a, b).to_bits() as u64);
            }
        }
        let mut rng = Rng(77);
        for i in 0..2_000_000u64 {
            let (a, b) = pair(&mut rng, i, pow);
            fnv(&mut hash, f(a, b).to_bits() as u64);
        }
        println!("{name:8} {hash:016x}");
    }
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    let mut rng = Rng(78);
    for i in 0..2_000_000u64 {
        let x = match i % 3 {
            0 => f64::from_bits(rng.next()),
            1 => rng.range(-1.0e6, 1.0e6),
            _ => f32::from_bits(rng.next() as u32) as f64,
        };
        fnv(&mut hash, pure::sin(x).to_bits());
    }
    println!("{:8} {hash:016x}", "sin");
}

/// `parse`: every number-like token of every text file under the folders, through
/// MSVCR120's `wcstod` / `wcstol` and through the Rust fallback a wasm build uses.
fn parse(folders: &[String]) {
    if rustyac_math::backend() != rustyac_math::Backend::Msvcr120 {
        println!("NOT TESTED: msvcr120.dll is not in use");
        return;
    }
    fn walk(folder: &std::path::Path, files: &mut Vec<std::path::PathBuf>) {
        let Ok(entries) = std::fs::read_dir(folder) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, files);
            } else if path.extension().is_some_and(|e| {
                ["ini", "lut", "rto", "txt", "json"]
                    .iter()
                    .any(|x| e.eq_ignore_ascii_case(x))
            }) {
                files.push(path);
            }
        }
    }
    let mut files = Vec::new();
    for folder in folders {
        walk(std::path::Path::new(folder), &mut files);
    }
    let mut tokens = std::collections::HashSet::new();
    for file in &files {
        let Ok(bytes) = std::fs::read(file) else {
            continue;
        };
        let text = String::from_utf8_lossy(&bytes);
        for token in text.split(|c: char| {
            c.is_whitespace()
                || matches!(c, '=' | ',' | ';' | '|' | ':' | '"' | '(' | ')' | '[' | ']')
        }) {
            if token.chars().any(|c| c.is_ascii_digit()) {
                tokens.insert(token.to_string());
                // and what follows the first sign or digit, as in `KEY=-1.5abc`
                if let Some(at) = token.find(|c: char| c.is_ascii_digit() || c == '-' || c == '.') {
                    tokens.insert(token[at..].to_string());
                }
            }
        }
    }
    let mut different = 0;
    for token in &tokens {
        let (a, b) = (rustyac_math::wcstod(token), rustyac_math::std_wcstod(token));
        let (c, e) = (rustyac_math::wcstol(token), rustyac_math::std_wcstol(token));
        let same = a.value.to_bits() == b.value.to_bits()
            && a.consumed == b.consumed
            && a.out_of_range == b.out_of_range
            && c == e;
        if !same {
            different += 1;
            if different <= 20 {
                println!("    {token:?}: MSVCR120 {a:?} {c:?}, Rust {b:?} {e:?}");
            }
        }
    }
    println!(
        "number parsers: {} files, {} distinct tokens, {different} parsed differently",
        files.len(),
        tokens.len()
    );
}

fn option<T: std::str::FromStr>(args: &[String], name: &str) -> Option<T> {
    let at = args.iter().position(|a| a == name)?;
    args.get(at + 1)?.parse().ok()
}

#[cfg(windows)]
fn compare(args: &[String], only: &[String], fma3: bool) {
    let Some(dll) = dll::load() else {
        println!("NOT TESTED: msvcr120.dll is not installed");
        return;
    };
    // SAFETY: `_set_FMA3_enable(0 | 1)` only writes the runtime's own flag.
    let state = unsafe { (dll.set_fma3)(fma3 as i32) };
    let threads = option(args, "--threads").unwrap_or_else(threads_default);
    let pair_count: u64 = option(args, "--pairs").unwrap_or(1_600_000_000);
    let wanted = |name: &str| only.is_empty() || only.iter().any(|o| o == name);
    println!(
        "MSVCR120.dll against rustyac_math::pure, {threads} threads; the DLL's FMA3 paths are {} \
         (_set_FMA3_enable returned {state})",
        if state != 0 { "on" } else { "off" }
    );
    let mut out = String::from("| Function | Inputs | Count | Result |\n|---|---|---|---|\n");
    let started = std::time::Instant::now();
    for ((name, want), (_, got)) in dll.one.into_iter().zip(PURE_ONE) {
        if wanted(name) {
            let s = exhaustive(name, want, got, threads);
            row(&mut out, name, "every 32-bit pattern", &s);
        }
    }
    let what = format!(
        "{} special pairs + random pairs (any bits / physics range / branch edges / targeted)",
        specials().len().pow(2)
    );
    if wanted("atan2f") {
        let s = pairs(
            "atan2f",
            dll.atan2f,
            pure::atan2f,
            false,
            pair_count,
            threads,
        );
        row(&mut out, "atan2f", &what, &s);
    }
    if wanted("powf") {
        let s = pairs("powf", dll.powf, pure::powf, true, pair_count, threads);
        row(&mut out, "powf", &what, &s);
    }
    if wanted("sin") {
        let [floats, random, physics, wide] = sin_double(dll.sin, pair_count / 4, threads);
        row(&mut out, "sin", "every float, widened to a double", &floats);
        row(&mut out, "sin", "random 64-bit patterns", &random);
        row(&mut out, "sin", "uniform in -1e6 .. 1e6", &physics);
        row(&mut out, "sin", "2^-30 .. 2^40, either sign", &wide);
    }
    println!("took {:.0} s", started.elapsed().as_secs_f64());
    if let Some(path) = option::<String>(args, "--out") {
        std::fs::write(&path, out).expect("writing the results file");
        println!("wrote {path}");
    }
}

#[cfg(not(windows))]
fn compare(_: &[String], _: &[String], _: bool) {
    println!("NOT TESTED: the comparison needs msvcr120.dll (Windows)");
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("all") => compare(&args, &[], true),
        Some("fma3-off") => compare(&args, &[], false),
        Some("one") => {
            let only: Vec<String> = args[1..]
                .iter()
                .take_while(|a| !a.starts_with("--"))
                .cloned()
                .collect();
            compare(&args, &only, !args.iter().any(|a| a == "--fma3-off"));
        }
        Some("parse") => parse(&args[1..]),
        Some("digest") => digest(option(&args, "--stride").unwrap_or(4099)),
        _ => {
            eprintln!(
                "usage: math_proof all | fma3-off | one <function>... | digest | parse <folder>...  \
                 [--threads N] [--pairs N] [--out <file.md>] [--stride N]"
            );
            std::process::exit(2);
        }
    }
}
