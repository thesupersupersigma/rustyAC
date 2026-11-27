// SPDX-License-Identifier: GPL-3.0-or-later

//! Task C6b part 4: CSP's `ffi.C.lj_*` natives of a car physics state ported, each against CSP's
//! function in csp_oracle (proof case `csp lua env`). CSP's bytecode declares their C types, so
//! each takes and returns what CSP's does: strings out as an MSVC `std::string` (CSP returns one
//! per thread and native, valid until that native runs again on the thread).

use std::cell::RefCell;
use std::ffi::{c_char, c_int, c_void, CStr};

use rustyac_lua::sys::*;

use super::inipp::Sections;

/// The layout CSP's cdef reads for a returned `std::string` (MSVC x64): the text, or a pointer to
/// it when the capacity is over 15, then size and capacity.
#[repr(C)]
pub struct MsvcString {
    ptr: *const u8,
    _inline_rest: [u8; 8],
    size: usize,
    cap: usize,
}

/// One returned string per native and thread, as CSP's thread_local statics.
struct Returned {
    text: Vec<u8>,
    msvc: MsvcString,
}

impl Returned {
    const fn new() -> Self {
        Returned { text: Vec::new(), msvc: MsvcString { ptr: std::ptr::null(), _inline_rest: [0; 8], size: 0, cap: 0 } }
    }

    /// Always in the pointer form (capacity at least 16), which CSP's reader takes for any size.
    fn set(&mut self, text: Vec<u8>) -> *const MsvcString {
        self.text = text;
        self.text.push(0);
        let size = self.text.len() - 1;
        self.msvc = MsvcString { ptr: self.text.as_ptr(), _inline_rest: [0; 8], size, cap: size.max(16) };
        &self.msvc
    }
}

thread_local! {
    static ENCODED: RefCell<Returned> = const { RefCell::new(Returned::new()) };
    static DECODED: RefCell<Returned> = const { RefCell::new(Returned::new()) };
}

const BASE64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// CSP's encoder (sub_181433D70): `=` padding unless `no_pad`.
fn encode_base64(data: &[u8], no_pad: bool) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let idx = [b[0] >> 2, ((b[0] & 3) << 4) + (b[1] >> 4), ((b[1] & 15) << 2) + (b[2] >> 6), b[2] & 63];
        out.extend(idx[..chunk.len() + 1].iter().map(|&i| BASE64[i as usize]));
        if !no_pad {
            out.extend(std::iter::repeat_n(b'=', 3 - chunk.len()));
        }
    }
    out
}

/// CSP's decoder (sub_181435190): stops at `=` or at the first byte that is not an ASCII letter,
/// digit, `+` or `/` (CRT isalnum in the C locale); a trailing group of n > 1 symbols gives n - 1 bytes.
fn decode_base64(text: &[u8]) -> Vec<u8> {
    let symbols: Vec<u8> = text
        .iter()
        .take_while(|&&c| c.is_ascii_alphanumeric() || c == b'+' || c == b'/')
        .map(|&c| BASE64.iter().position(|&s| s == c).unwrap() as u8)
        .collect();
    let mut out = Vec::with_capacity(symbols.len() * 3 / 4);
    for group in symbols.chunks(4) {
        let s = [group[0], *group.get(1).unwrap_or(&0), *group.get(2).unwrap_or(&0), *group.get(3).unwrap_or(&0)];
        let bytes = [(s[0] << 2) + ((s[1] >> 4) & 3), (s[1] << 4) + ((s[2] >> 2) & 15), (s[2] << 6) + s[3]];
        out.extend_from_slice(&bytes[..group.len() - 1]);
    }
    out
}

/// `lj_encodeBase64(span, no_pad)` (sub_1813D2C00): the span is CSP's {begin, end} of the data.
unsafe extern "C" fn lj_encode_base64(span: *const [*const u8; 2], no_pad: bool) -> *const MsvcString {
    if span.is_null() {
        return std::ptr::null();
    }
    let [begin, end] = *span;
    let data = std::slice::from_raw_parts(begin, end.offset_from(begin) as usize);
    ENCODED.with(|r| r.borrow_mut().set(encode_base64(data, no_pad)))
}

/// `lj_decodeBase64(text)` (sub_1813D2E30): a C string.
unsafe extern "C" fn lj_decode_base64(text: *const c_char) -> *const MsvcString {
    if text.is_null() {
        return std::ptr::null();
    }
    DECODED.with(|r| r.borrow_mut().set(decode_base64(CStr::from_ptr(text).to_bytes())))
}

/// `lj_structBytes(blob)` (sub_1813D3210): a copy of the bytes (`ac.StructItem`'s packing).
unsafe extern "C" fn lj_struct_bytes(blob: *const [*const u8; 2]) -> *const MsvcString {
    if blob.is_null() {
        return std::ptr::null();
    }
    let [begin, end] = *blob;
    let data = std::slice::from_raw_parts(begin, end.offset_from(begin) as usize);
    STRUCT_BYTES.with(|r| r.borrow_mut().set(data.to_vec()))
}

thread_local! {
    static STRUCT_BYTES: RefCell<Returned> = const { RefCell::new(Returned::new()) };
}

/// `lj_getPatchVersionCode` (sub_1802FD360): `(11078 - dword_1827FAB10) >> 1`, the global set by
/// CSP at start; 4305 for the CSP the proofs run (shortcut: CSP 4305 only, read it from the
/// installed dwrite.dll if rustyAC ever runs other CSP versions).
extern "C" fn lj_get_patch_version_code() -> i32 {
    4305
}

/// `lj_random_seed` (sub_1813D1E60): a random int in 0..=2147483646 (`ac.StructItem.combine`'s
/// type names). CSP's xoshiro256** is seeded per thread from `random_device` and the performance
/// counter, so no run repeats it; any random number in the range does the same.
extern "C" fn lj_random_seed() -> i32 {
    use std::hash::BuildHasher;
    (std::hash::RandomState::new().hash_one(std::time::Instant::now()) % 2_147_483_647) as i32
}

/// `ac.ControlButton`'s natives (`lj_binaryinput_*`, sub_180489F40 ..). `new` returns null unless the
/// script's car is the one the car controls provider drives (sub_1805DDC70); in csp_oracle's drive
/// it is not, and CSP passes that null to every later call, each of which then returns 0 / false.
// shortcut: no extended-control input (null buttons), upgrade when rustyAC drives a car with __EXT_CAR_ keys bound
extern "C" fn lj_binaryinput_none() -> usize {
    0
}

/// CSP's `lua_lut` (its cdef): a `std::vector<vec2>` of points (null pointers when empty),
/// bounds, the size (-1 until refreshed) and the two flags scripts set.
#[repr(C)]
pub struct LuaLut {
    begin: *mut [f32; 2],
    end: *mut [f32; 2],
    cap: *mut [f32; 2],
    b_min: [f32; 2],
    b_max: [f32; 2],
    size: i32,
    cubic: bool,
    extrapolate: bool,
}

impl LuaLut {
    fn boxed(points: Vec<[f32; 2]>) -> *mut LuaLut {
        let mut lut = Box::new(LuaLut { begin: std::ptr::null_mut(), end: std::ptr::null_mut(), cap: std::ptr::null_mut(), b_min: [0.0; 2], b_max: [0.0; 2], size: -1, cubic: false, extrapolate: false });
        lut.store(points);
        Box::into_raw(lut)
    }

    fn store(&mut self, points: Vec<[f32; 2]>) {
        let mut points = std::mem::ManuallyDrop::new(points);
        if points.capacity() == 0 {
            (self.begin, self.end, self.cap) = (std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut());
        } else {
            let p = points.as_mut_ptr();
            unsafe { (self.begin, self.end, self.cap) = (p, p.add(points.len()), p.add(points.capacity())) }
        }
    }

    /// The points, owned again (store them back or drop them).
    unsafe fn take(&mut self) -> Vec<[f32; 2]> {
        if self.begin.is_null() {
            return Vec::new();
        }
        Vec::from_raw_parts(self.begin, self.end.offset_from(self.begin) as usize, self.cap.offset_from(self.begin) as usize)
    }

    unsafe fn points(&self) -> &[[f32; 2]] {
        if self.begin.is_null() {
            return &[];
        }
        std::slice::from_raw_parts(self.begin, self.end.offset_from(self.begin) as usize)
    }
}

// CSP's SSE maxss / minss: the second operand when the first does not win (NaN included).
fn maxss(a: f32, b: f32) -> f32 {
    if a > b { a } else { b }
}

fn minss(a: f32, b: f32) -> f32 {
    if a < b { a } else { b }
}

/// sub_1814A3780 (two points or more): the segment around `x` as (i0, i1, index, width, t).
fn segment(v: &[[f32; 2]], x: f32) -> (usize, usize, i32, f32, f32) {
    let n = v.len() as u32;
    let (mut count, mut lo) = (n, 0u32);
    while count != 0 {
        let half = count >> 1;
        if x >= v[(lo + half) as usize][0] {
            lo += half + 1;
            count -= half + 1;
        } else {
            count = half;
        }
    }
    let i1 = if lo == n { lo - 1 } else { lo } as usize;
    let i0 = if lo == 0 { 0 } else { lo - 1 } as usize;
    let w = maxss(v[i1][0] - v[i0][0], f32::EPSILON);
    let t = maxss(x - v[i0][0], 0.0) / w;
    let t = if t >= 0.0 { minss(t, 1.0) } else { 0.0 };
    (i0, i1, lo as i32 - 1, w, t)
}

/// sub_1814A38A0: the slope around point `i`, neighbours clamped to the table.
fn tangent(v: &[[f32; 2]], i: i32) -> f32 {
    let clamp = |j: i32| if j < 0 { 0 } else if j >= v.len() as i32 { v.len() - 1 } else { j as usize };
    let (a, b) = (clamp(i - 1), clamp(i + 1));
    (v[b][1] - v[a][1]) / maxss(v[b][0] - v[a][0], f32::EPSILON)
}

/// sub_1814A2120 (linear) and sub_1814A21F0 (cubic Hermite): the value at `x`.
// spelled as CSP's branches so NaN takes the same path
#[allow(clippy::neg_cmp_op_on_partial_ord)]
fn lut_value(v: &[[f32; 2]], x: f32, cubic: bool, extrapolate: bool) -> f32 {
    match v.len() {
        0 => return 0.0,
        1 => return v[0][1],
        _ => {}
    }
    if extrapolate {
        let ([x1, y1], [xl, yl]) = (v[v.len() - 2], v[v.len() - 1]);
        if x > xl {
            return (yl - y1) / (xl - x1) * (x - xl) + yl;
        }
        let ([x0, y0], [x1, y1]) = (v[0], v[1]);
        if !(x >= x0) {
            return (y0 - y1) / (x0 - x1) * (x - x0) + y0;
        }
    }
    let (i0, i1, index, w, t) = segment(v, x);
    let (y0, y1) = (v[i0][1], v[i1][1]);
    if !cubic {
        return (y1 - y0) * t + y0;
    }
    if i0 == i1 {
        return y1;
    }
    let (t2, t3) = (t * t, t * t * t);
    let (c3, t3x2) = (t2 * 3.0, t3 + t3);
    let a = tangent(v, index + 1) * ((t3 - t2) * w);
    let b = tangent(v, index) * (((t3 - (t2 + t2)) + t) * w);
    (a + b) + (((t3x2 - c3) + 1.0) * y0 + (c3 - t3x2) * y1)
}

/// sub_1814A23C0: {min, max} of the points; one point at x 0 spans x 0..1.
#[allow(clippy::neg_cmp_op_on_partial_ord)]
fn bounds(v: &[[f32; 2]]) -> ([f32; 2], [f32; 2]) {
    let Some(&first) = v.first() else { return ([0.0; 2], [0.0; 2]) };
    let (mut min, mut max) = (first, first);
    if v.len() == 1 && first[0] == 0.0 {
        max[0] = 1.0;
    }
    for p in &v[1..] {
        for k in 0..2 {
            min[k] = minss(p[k], min[k]);
            max[k] = maxss(p[k], max[k]);
        }
    }
    (min, max)
}

/// CSP's float parser over the number at the start of `text` (0 when there is none).
fn float(text: &[u8]) -> f32 {
    let n = text.iter().take_while(|c| c.is_ascii_digit() || b"+-.eE".contains(c)).count();
    super::parse_float(std::str::from_utf8(&text[..n]).unwrap(), 0.0) as f32
}

fn trim<'a>(text: &'a [u8], cut: &[u8]) -> &'a [u8] {
    let start = text.iter().position(|c| !cut.contains(c)).unwrap_or(text.len());
    let end = text.iter().rposition(|c| !cut.contains(c)).map_or(start, |e| e + 1);
    &text[start..end]
}

/// The output of a `x|y,...` or `x=y,...` pair: its first `,` piece, trimmed of ' ', '\t', '\r'.
fn first_piece(text: &[u8]) -> f32 {
    float(trim(text.split(|&c| c == b',').next().unwrap(), b" \t\r"))
}

/// sub_1814A3160: inline `(x=y|x=y)` when there is a `=`, else a `.lut` file's lines when there is
/// a digit, sign or comment, else nothing; points kept in their order.
fn parse_lut(text: &[u8]) -> Vec<[f32; 2]> {
    let t = trim(text, b"\n\r\t ");
    let (mut i, mut file) = (0, false);
    while i < t.len() {
        match t[i] {
            b'=' => return parse_inline(t),
            b';' | b'#' => file = true,
            b'/' if t.get(i + 1) == Some(&b'/') => file = true,
            c if c.is_ascii_digit() || c == b'+' || c == b'-' => {
                file = true;
                i += 1;
                continue;
            }
            _ => {
                i += 1;
                continue;
            }
        }
        // a comment: up to the line's end
        while i < t.len() && t[i] != b'\n' {
            i += 1;
        }
        i += 1;
    }
    if file { parse_file(text) } else { Vec::new() }
}

fn parse_inline(s: &[u8]) -> Vec<[f32; 2]> {
    let (mut out, mut i, mut start, mut key) = (Vec::new(), 0, 0, f32::MAX);
    while i < s.len() {
        match s[i] {
            b'(' => start = i + 1,
            b'=' => {
                if i != start {
                    key = float(&s[start..]);
                }
                start = i + 1;
            }
            b'|' => {
                if i != start && key != f32::MAX {
                    out.push([key, first_piece(&s[start..i])]);
                    key = f32::MAX;
                }
                start = i + 1;
            }
            b')' => break,
            b'\t' | b'\r' | b' ' if start == i => start += 1,
            _ => {}
        }
        i += 1;
    }
    if i != start && key != f32::MAX {
        out.push([key, first_piece(&s[start..i])]);
    }
    out
}

/// sub_1814A1B30: a `.lut` file: lines trimmed, empty and comment lines skipped, `x|y` points.
fn parse_file(text: &[u8]) -> Vec<[f32; 2]> {
    let lines = text.split(|&c| c == b'\n' || c == 0).map(|l| trim(l, b" \t\r"));
    let lines = lines.filter(|l| !l.is_empty() && !l.starts_with(b";") && !l.starts_with(b"#") && !l.starts_with(b"//"));
    lines
        .filter_map(|l| {
            let p = l.iter().position(|&c| c == b'|')?;
            (p != 0 && p + 1 < l.len()).then(|| [float(l), first_piece(&l[p + 1..])])
        })
        .collect()
}

/// The car data folder of each car index whose physics state is set up (csp::lua_env).
static CAR_DATA: std::sync::Mutex<std::collections::BTreeMap<i32, std::path::PathBuf>> = std::sync::Mutex::new(std::collections::BTreeMap::new());

pub(super) fn set_car_data(car_index: i32, dir: &str, dwrite: &std::path::Path) {
    *DWRITE.lock().unwrap() = Some(dwrite.into());
    CAR_DATA.lock().unwrap().insert(car_index, dir.into());
}

unsafe extern "C" fn lj_lut_empty() -> *mut LuaLut {
    LuaLut::boxed(Vec::new())
}

unsafe extern "C" fn lj_lut_parse(text: *const c_char) -> *mut LuaLut {
    if text.is_null() {
        return std::ptr::null_mut();
    }
    LuaLut::boxed(parse_lut(CStr::from_ptr(text).to_bytes()))
}

/// sub_180FEE320: a file of the car's data (data.acd or the folder), read as a `.lut` file; NULL
/// for a car not loaded, no points for a file not there.
unsafe extern "C" fn lj_lut_loadcardata(car_index: i32, name: *const c_char) -> *mut LuaLut {
    let Some(dir) = CAR_DATA.lock().unwrap().get(&car_index).cloned() else { return std::ptr::null_mut() };
    if name.is_null() {
        return std::ptr::null_mut();
    }
    let file = dir.join(CStr::from_ptr(name).to_string_lossy().as_ref());
    // shortcut: CSP's text read (sub_18142A660) and its per-file cache are not ported, bytes as they are
    LuaLut::boxed(crate::data::read(&file).ok().flatten().map_or(Vec::new(), |b| parse_file(&b)))
}

unsafe extern "C" fn lj_lut_gc(lut: *mut LuaLut) {
    if !lut.is_null() {
        let mut lut = Box::from_raw(lut);
        drop(lut.take());
    }
}

/// sub_1814A2670: sorted insert (before the first larger x) unless past the last point.
unsafe extern "C" fn lj_lut_add(lut: *mut LuaLut, x: f32, y: f32) {
    let lut = &mut *lut;
    let mut v = lut.take();
    match v.last() {
        Some(last) if x < last[0] => {
            let at = v.iter().position(|p| x < p[0]).unwrap_or(v.len());
            v.insert(at, [x, y]);
        }
        _ => v.push([x, y]),
    }
    lut.store(v);
    lut.size = -1;
}

unsafe extern "C" fn lj_lut_refresh(lut: *mut LuaLut) {
    let lut = &mut *lut;
    (lut.b_min, lut.b_max) = bounds(lut.points());
    lut.size = lut.points().len() as i32;
}

unsafe extern "C" fn lj_lut_get(lut: *const LuaLut, x: f32) -> f32 {
    let lut = &*lut;
    lut_value(lut.points(), x, lut.cubic, lut.extrapolate)
}

unsafe extern "C" fn lj_lut_atkey(lut: *const LuaLut, i: i32) -> f32 {
    (*lut).points().get(i as usize).map_or(f32::NAN, |p| p[0])
}

unsafe extern "C" fn lj_lut_atvalue(lut: *const LuaLut, i: i32) -> f32 {
    (*lut).points().get(i as usize).map_or(f32::NAN, |p| p[1])
}

/// CSP's `lua_lut2d` (its cdef): the table (null while it has no value), a float `get` returns
/// without one, the bounds (x, y, value) as of `refresh`, the size (-1 until refreshed), the flag.
#[repr(C)]
pub struct LuaLut2d {
    inner: *mut Lut2d,
    lut1: u64,
    b_min: [f32; 3],
    b_max: [f32; 3],
    size: i32,
    bicubic: bool,
}
const _: () = assert!(std::mem::size_of::<LuaLut2d>() == 48);

/// sub_1814A52A0's table: (x, y) keys and values in their order, the grid built on demand.
pub struct Lut2d {
    keys: Vec<[f32; 2]>,
    values: Vec<f32>,
    grid: Option<Grid>,
}

/// sub_1814A55E0's bilinear grid: the y and x axes, a cell's corner values over its area, bounds.
struct Grid {
    y: Vec<f32>,
    x: Vec<f32>,
    coefs: Vec<[f32; 4]>,
    min: [f32; 3],
    max: [f32; 3],
}

impl LuaLut2d {
    fn boxed(table: Option<(Vec<[f32; 2]>, Vec<f32>)>) -> *mut LuaLut2d {
        let inner = match table {
            Some((keys, values)) if !values.is_empty() => Box::into_raw(Box::new(Lut2d { keys, values, grid: None })),
            _ => std::ptr::null_mut(),
        };
        Box::into_raw(Box::new(LuaLut2d { inner, lut1: 0, b_min: [0.0; 3], b_max: [0.0; 3], size: -1, bicubic: false }))
    }
}

/// CRT strtof (sub_181DAE96C): the number at the start of `text` after white space, 0 when none.
// shortcut: no inf / nan / hex forms, upgrade when a table spells one
fn strtof(text: &[u8]) -> f32 {
    let t = &text[text.iter().position(|c| !b" \t\n\x0b\x0c\r".contains(c)).unwrap_or(text.len())..];
    let digits = |from: usize| t[from.min(t.len())..].iter().take_while(|c| c.is_ascii_digit()).count();
    let mut n = usize::from(matches!(t.first(), Some(b'+' | b'-')));
    let int = digits(n);
    n += int;
    let mut frac = 0;
    if t.get(n) == Some(&b'.') {
        frac = digits(n + 1);
        n += 1 + frac;
    }
    if int + frac == 0 {
        return 0.0;
    }
    if matches!(t.get(n), Some(b'e' | b'E')) {
        let sign = usize::from(matches!(t.get(n + 1), Some(b'+' | b'-')));
        let e = digits(n + 1 + sign);
        if e > 0 {
            n += 1 + sign + e;
        }
    }
    std::str::from_utf8(&t[..n]).unwrap().parse().unwrap_or(0.0)
}

/// sub_1815FE960 over a key's `,` pieces (trimmed of ' ', '\t'): x, then y (x again for one piece).
fn key2d(text: &[u8]) -> [f32; 2] {
    let pieces: Vec<&[u8]> = text.split(|&c| c == b',').map(|p| trim(p, b" \t")).collect();
    let x = float(pieces[0]);
    [x, if pieces.len() == 1 { x } else { float(pieces[1]) }]
}

/// sub_1814A63E0: inline `(x,y=v|x,y=v)`, kept in order; a value whose key x is NaN (no key)
/// is dropped (`ucomiss xmm6, xmm6` at 0x1814a6740 / 0x1814a697f).
fn parse_inline2d(s: &[u8]) -> (Vec<[f32; 2]>, Vec<f32>) {
    let (mut keys, mut values, mut key) = (Vec::new(), Vec::new(), [f32::NAN; 2]);
    let (mut i, mut start) = (0, 0);
    while i < s.len() {
        match s[i] {
            b'(' => start = i + 1,
            b'=' => {
                if i > start {
                    key = key2d(&s[start..i]);
                }
                start = i + 1;
            }
            b'|' => {
                if i > start && !key[0].is_nan() {
                    values.push(strtof(&s[start..i]));
                    keys.push(key);
                    key = [f32::NAN; 2];
                }
                start = i + 1;
            }
            b')' => break,
            _ => {}
        }
        i += 1;
    }
    if i > start && !key[0].is_nan() {
        values.push(strtof(&s[start..i]));
        keys.push(key);
    }
    (keys, values)
}

/// sub_1814A6E70: a `.2Dlut` file: lines trimmed, comment lines skipped; the first holds the xs,
/// each other a y then a value per x. None for no header or a row of another length.
// shortcut: the binary / encrypted forms (a magic prefix) are read as text, upgrade when a car ships one
fn parse_file2d(text: &[u8]) -> Option<(Vec<[f32; 2]>, Vec<f32>)> {
    let lines = text.split(|&c| c == b'\n' || c == 0).map(|l| trim(l, b"\t\r "));
    let mut lines = lines.filter(|l| !l.is_empty() && !l.starts_with(b";") && !l.starts_with(b"#") && !l.starts_with(b"//"));
    // sub_18142B3D0: pieces between spaces / tabs, a number when it looks like one, else NaN
    let pieces = |l: &[u8]| -> Vec<f32> {
        let number = |p: &[u8]| p[0].is_ascii_digit() || (matches!(p[0], b'-' | b'.') && p.len() > 1);
        l.split(|&c| c == b' ' || c == b'\t').filter(|p| !p.is_empty()).map(|p| if number(p) { strtof(p) } else { f32::NAN }).collect()
    };
    let head = pieces(lines.next()?);
    if head.is_empty() {
        return None;
    }
    let (mut keys, mut values) = (Vec::new(), Vec::new());
    for row in lines.map(pieces) {
        if row.len() != head.len() + 1 {
            return None;
        }
        for (k, &x) in head.iter().enumerate() {
            keys.push([x, row[0]]);
            values.push(row[k + 1]);
        }
    }
    Some((keys, values))
}

/// sub_1814A55E0 (bilinear) and sub_1814A9BC0: bounds over the values that are not NaN, the row
/// length as the leading keys sharing the first y, each cell's corners over its area.
fn build2d(keys: &[[f32; 2]], values: &[f32]) -> Grid {
    let n = values.len();
    let (mut min, mut max) = ([f32::MAX; 3], [-f32::MAX; 3]);
    let (mut ys, mut xs, mut vs) = (vec![0.0f32; n], vec![0.0f32; n], vec![0.0f32; n]);
    for i in (0..n).rev() {
        let ([x, y], v) = (keys[i], values[i]);
        if v.is_nan() {
            continue;
        }
        // fminf / fmaxf: the number when the other is NaN
        // shortcut: a -0 / +0 tie may keep the other zero than CSP's, upgrade when a table has both
        min = [min[0].min(x), y.min(min[1]), v.min(min[2])];
        max = [max[0].max(x), y.max(max[1]), v.max(max[2])];
        (ys[i], xs[i], vs[i]) = (y, x, v);
    }
    // CSP divides by zero on a table of one value (n1 = 0), so does this (a panic, an abort)
    let n1 = (0..n - 1).take_while(|&k| { let d = (ys[k] - ys[0]).abs(); (d as f64) < 1e-40 || d.is_nan() }).count();
    let ny = n / n1;
    let (y, x): (Vec<f32>, Vec<f32>) = ((0..ny).map(|i| ys[n1 * i]).collect(), xs[..n1].to_vec());
    let (ny1, nx1) = (ny - 1, n1 - 1);
    let mut coefs = vec![[0.0f32; 4]; ny1 * nx1];
    for iy in 0..ny1 {
        for jx in 0..nx1 {
            let p = jx + n1 * iy;
            let inv = 1.0 / ((y[iy + 1] - y[iy]) * (x[jx + 1] - x[jx]));
            coefs[iy + ny1 * jx] = [inv * vs[p], inv * vs[p + n1], inv * vs[p + 1], inv * vs[p + n1 + 1]];
        }
    }
    Grid { y, x, coefs, min, max }
}

/// std::lower_bound as MSVC writes it: the first element not below `v` (`v <= e` goes left).
#[allow(clippy::neg_cmp_op_on_partial_ord)]
fn lower_bound(axis: &[f32], v: f32) -> usize {
    let (mut lo, mut count) = (0, axis.len());
    while count > 0 {
        let half = count / 2;
        if !(v <= axis[lo + half]) {
            lo += half + 1;
            count -= half + 1;
        } else {
            count = half;
        }
    }
    lo
}

/// sub_1814A98C0: 0 off the grid, else the cell's bilinear value in CSP's operation order.
fn eval2d(g: &Grid, y: f32, x: f32) -> f32 {
    if y < g.y[0] || y > g.y[g.y.len() - 1] || x < g.x[0] || x > g.x[g.x.len() - 1] {
        return 0.0;
    }
    let (j, k) = (lower_bound(&g.y, y).saturating_sub(1), lower_bound(&g.x, x).saturating_sub(1));
    let (a, b, cx, d) = (g.y[j + 1] - y, y - g.y[j], g.x[k + 1] - x, x - g.x[k]);
    let c = g.coefs[j + (g.y.len() - 1) * k];
    ((b * c[3]) + (a * c[2])) * d + ((b * c[1]) + (a * c[0])) * cx
}

unsafe extern "C" fn lj_lut2d_empty() -> *mut LuaLut2d {
    LuaLut2d::boxed(None)
}

unsafe extern "C" fn lj_lut2d_parse(text: *const c_char) -> *mut LuaLut2d {
    if text.is_null() {
        return std::ptr::null_mut();
    }
    LuaLut2d::boxed(Some(parse_inline2d(CStr::from_ptr(text).to_bytes())))
}

/// sub_180FEE8B0: `<car>/data/<file>` of a car whose physics state is set up as a `.2Dlut` file;
/// NULL for a car not loaded or a file not there.
unsafe extern "C" fn lj_lut2d_loadcardata(car_index: i32, name: *const c_char) -> *mut LuaLut2d {
    let Some(dir) = CAR_DATA.lock().unwrap().get(&car_index).cloned() else { return std::ptr::null_mut() };
    if name.is_null() {
        return std::ptr::null_mut();
    }
    let file = dir.join(CStr::from_ptr(name).to_string_lossy().as_ref());
    match crate::data::read(&file).ok().flatten() {
        Some(bytes) => LuaLut2d::boxed(parse_file2d(&bytes)),
        None => std::ptr::null_mut(),
    }
}

unsafe extern "C" fn lj_lut2d_gc(lut: *mut LuaLut2d) {
    if !lut.is_null() {
        let lut = Box::from_raw(lut);
        if !lut.inner.is_null() {
            drop(Box::from_raw(lut.inner));
        }
    }
}

/// sub_1814A7EE0: appended unsorted, the grid dropped.
unsafe extern "C" fn lj_lut2d_add(lut: *mut LuaLut2d, key: *const [f32; 2], value: f32) {
    let lut = &mut *lut;
    let Some(t) = lut.inner.as_mut() else {
        // CSP divides by zero adding to a table with no value
        eprintln!("CSP Lua: lj_lut2d_add on a table with no value crashes CSP (docs/csp/lua_host.md)");
        std::process::abort();
    };
    t.keys.push(*key);
    t.values.push(value);
    t.grid = None;
    lut.size = -1;
}

/// sub_180FEEBA0: the size and the grid's bounds (zeros with no value).
unsafe extern "C" fn lj_lut2d_refresh(lut: *mut LuaLut2d) {
    let lut = &mut *lut;
    let Some(t) = lut.inner.as_mut() else {
        (lut.size, lut.b_min, lut.b_max) = (0, [0.0; 3], [0.0; 3]);
        return;
    };
    let g = t.grid.get_or_insert_with(|| build2d(&t.keys, &t.values));
    (lut.size, lut.b_min, lut.b_max) = (t.keys.len() as i32, g.min, g.max);
}

/// sub_180FEEB10 over sub_1814A6100: the key clamped to the bounds (NaN to the low one), 0 for
/// an infinite or NaN result; the float at +8 with no value.
unsafe extern "C" fn lj_lut2d_get(lut: *mut LuaLut2d, key: *const [f32; 2]) -> f32 {
    let lut = &mut *lut;
    let Some(t) = lut.inner.as_mut() else { return f32::from_bits(lut.lut1 as u32) };
    if lut.bicubic {
        eprintln!("CSP Lua: lj_lut2d_get with useBicubicInterpolation (sub_1814A55E0 mode 1) is not ported yet (docs/csp/lua_host.md)");
        std::process::abort();
    }
    let g = t.grid.get_or_insert_with(|| build2d(&t.keys, &t.values));
    let clamp = |v: f32, lo: f32, hi: f32| if v >= lo { minss(hi, v) } else { lo };
    let [x, y] = *key;
    let r = eval2d(g, clamp(y, g.min[1], g.max[1]), clamp(x, g.min[0], g.max[0]));
    if r.is_finite() { r } else { 0.0 }
}

/// Task C6b part 5: `__util.nativf(hash, ...)`, CSP's natives called by hash (registered by
/// sub_18101A9E0), the ported ones; any other stops, naming its hash.
pub(super) unsafe extern "C-unwind" fn nativf(l: *mut lua_State) -> c_int {
    match lua_tonumber(l, 1) as i64 as u32 {
        0x14c2318e => ini_parse(l),
        0xe8bebf1b => ini_car_data(l),
        hash => {
            eprintln!("CSP Lua: __util.nativf({hash:#010x}) is not ported yet (docs/csp/lua_host.md)");
            std::process::abort();
        }
    }
}

unsafe fn arg_bytes<'a>(l: *mut lua_State, idx: c_int) -> &'a [u8] {
    let mut n = 0;
    let p = lua_tolstring(l, idx, &mut n);
    if p.is_null() {
        &[]
    } else {
        std::slice::from_raw_parts(p.cast(), n)
    }
}

/// sub_1813EED80: `{section = {key = {value, ...}}}` pushed.
unsafe fn push_sections(l: *mut lua_State, sections: &Sections) -> c_int {
    let push = |b: &[u8]| lua_pushlstring(l, b.as_ptr().cast(), b.len());
    lua_createtable(l, 0, sections.len() as c_int);
    for (name, keys) in sections {
        push(name);
        lua_createtable(l, 0, keys.len() as c_int);
        for (key, values) in keys {
            push(key);
            lua_createtable(l, values.len() as c_int, 0);
            for (i, v) in values.iter().enumerate() {
                lua_pushnumber(l, (i + 1) as f64);
                push(v);
                lua_settable(l, -3);
            }
            lua_settable(l, -3);
        }
        lua_settable(l, -3);
    }
    1
}

/// `ac.INIConfig.parse(text, format, includeFolders)` (sub_180FECE10 -> sub_180FEAD20): the
/// extended formats (10, 11 without include folders).
unsafe fn ini_parse(l: *mut lua_State) -> c_int {
    let format = lua_tonumber(l, 3) as i32;
    if format < 10 || (format == 11 && lua_type(l, 4) == LUA_TTABLE) {
        // shortcut: CSP's own parser of the AC formats (sub_180FE8970) and includes (sub_1815889E0) not ported; no car script parses
        eprintln!("CSP Lua: ac.INIConfig.parse in format {format} is not ported yet (docs/csp/lua_host.md)");
        std::process::abort();
    }
    push_sections(l, &super::inipp::parse(arg_bytes(l, 2)))
}

/// dwrite.dll of the car physics states set up (csp::lua_env): its INIpp Lua library.
static DWRITE: std::sync::Mutex<Option<std::path::PathBuf>> = std::sync::Mutex::new(None);

/// `ac.INIConfig.carData(carIndex, fileName)` (sub_180FECC40 -> sub_180FEA170): a file of the
/// car's data (data.acd or the folder, name lowercased after its last slash) read by INIpp with
/// Lua on; no sections for a file not there or a negative index.
unsafe fn ini_car_data(l: *mut lua_State) -> c_int {
    let index = lua_tonumber(l, 2) as i32;
    let file = arg_bytes(l, 3);
    if index < 0 {
        return push_sections(l, &Vec::new());
    }
    let Some(dir) = CAR_DATA.lock().unwrap().get(&index).cloned() else {
        // shortcut: only cars with a physics state set up are known here; upgrade with the game's car list (C6c)
        eprintln!("CSP Lua: ac.INIConfig.carData of car {index} (no physics state) is not ported yet (docs/csp/lua_host.md)");
        std::process::abort();
    };
    let name = data_name(file);
    let text = crate::data::read(&dir.join(String::from_utf8_lossy(&name).as_ref())).ok().flatten().unwrap_or_default();
    let lib = DWRITE.lock().unwrap().as_deref().and_then(super::inipp_lua::ini_std);
    let Some(mut sections) = lib.and_then(|lib| super::inipp::parse_lua(&text, &lib)) else {
        eprintln!("CSP Lua: ac.INIConfig.carData: no INIpp Lua library in dwrite.dll");
        std::process::abort();
    };
    if name == b"tyres.ini" && !tyres_as_read(&mut sections) {
        eprintln!("CSP Lua: ac.INIConfig.carData of tyres.ini CSP changes (sub_180914710) is not ported yet (docs/csp/lua_host.md)");
        std::process::abort();
    }
    push_sections(l, &sections)
}

/// sub_180FE9E40 and the lowercasing of sub_180FEA170: the name after its last slash, lowercase.
fn data_name(file: &[u8]) -> Vec<u8> {
    let at = file.iter().rposition(|&c| c == b'/' || c == b'\\').map_or(0, |i| i + 1);
    file[at..].to_ascii_lowercase()
}

/// tyres.ini as carData gives it, `false` if CSP changes it on loading: sections with
/// `_VIRTUAL_ORIGIN` and the extra tyres it makes (sub_18090C6C0, sub_180914710; off only by
/// `[_EXTENSION] GENERATE_EXTRA_TYRES` false); its `_EFX_` keys are left out (sub_180FEA170).
fn tyres_as_read(sections: &mut Sections) -> bool {
    let virt = sections.iter().any(|(_, keys)| keys.iter().any(|(k, _)| k == b"_VIRTUAL_ORIGIN"));
    let extra = sections.iter().find(|(n, _)| n == b"_EXTENSION").and_then(|(_, keys)| keys.iter().find(|(k, _)| k == b"GENERATE_EXTRA_TYRES"));
    // shortcut: CSP makes them only for extended physics or once the player's car is loaded (sub_18090D9F0), taken as always
    if virt || !extra.is_some_and(|(_, v)| !super::inipp::truthy(v.first())) {
        return false;
    }
    for (_, keys) in sections.iter_mut() {
        keys.retain(|(k, _)| !k.starts_with(b"_EFX_"));
    }
    true
}

/// The ported natives by their `ffi.C` name.
pub fn native(name: &[u8]) -> Option<*mut c_void> {
    Some(match name {
        b"lj_encodeBase64" => lj_encode_base64 as *mut c_void,
        b"lj_decodeBase64" => lj_decode_base64 as *mut c_void,
        b"lj_structBytes" => lj_struct_bytes as *mut c_void,
        b"lj_random_seed" => lj_random_seed as *mut c_void,
        b"lj_getPatchVersionCode" => lj_get_patch_version_code as *mut c_void,
        b"lj_lut_empty" => lj_lut_empty as *mut c_void,
        b"lj_lut_parse" => lj_lut_parse as *mut c_void,
        b"lj_lut_loadcardata" => lj_lut_loadcardata as *mut c_void,
        b"lj_lut_gc" => lj_lut_gc as *mut c_void,
        b"lj_lut_add" => lj_lut_add as *mut c_void,
        b"lj_lut_refresh" => lj_lut_refresh as *mut c_void,
        b"lj_lut_get" => lj_lut_get as *mut c_void,
        b"lj_lut_atkey" => lj_lut_atkey as *mut c_void,
        b"lj_lut_atvalue" => lj_lut_atvalue as *mut c_void,
        b"lj_lut2d_empty" => lj_lut2d_empty as *mut c_void,
        b"lj_lut2d_parse" => lj_lut2d_parse as *mut c_void,
        b"lj_lut2d_loadcardata" => lj_lut2d_loadcardata as *mut c_void,
        b"lj_lut2d_gc" => lj_lut2d_gc as *mut c_void,
        b"lj_lut2d_add" => lj_lut2d_add as *mut c_void,
        b"lj_lut2d_refresh" => lj_lut2d_refresh as *mut c_void,
        b"lj_lut2d_get" => lj_lut2d_get as *mut c_void,
        b"lj_binaryinput_new" => lj_binaryinput_none as *mut c_void,
        b"lj_binaryinput_pressedphysics" => lj_binaryinput_none as *mut c_void,
        b"lj_binaryinput_downphysics" => lj_binaryinput_none as *mut c_void,
        b"lj_binaryinput_set" => lj_binaryinput_none as *mut c_void,
        b"lj_binaryinput_setalwaysactive" => lj_binaryinput_none as *mut c_void,
        b"lj_binaryinput_setdisabled" => lj_binaryinput_none as *mut c_void,
        b"lj_binaryinput_onpressed" => lj_binaryinput_none as *mut c_void,
        b"lj_binaryinput_onreleased" => lj_binaryinput_none as *mut c_void,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64() {
        assert_eq!(encode_base64(b"hello", false), b"aGVsbG8=");
        assert_eq!(encode_base64(b"hello", true), b"aGVsbG8");
        assert_eq!(encode_base64(b"", false), b"");
        assert_eq!(decode_base64(b"aGVsbG8"), b"hello");
        assert_eq!(decode_base64(b"aGVsbG8=junk"), b"hello");
        assert_eq!(decode_base64(b"!!"), b"");
        assert_eq!(decode_base64(b"QQ"), b"A");
        assert_eq!(decode_base64(b"Q"), b"");
    }

    #[test]
    fn car_data_glue() {
        assert_eq!(data_name(b"a/B\\Tyres.INI"), b"tyres.ini");
        assert_eq!(data_name(b"car.ini"), b"car.ini");
        let ini = |t: &[u8]| super::super::inipp::parse(t);
        let mut s = ini(b"[_EXTENSION]\nGENERATE_EXTRA_TYRES=0\n[FRONT]\nNAME=x\n_EFX_A=1\n_EF=2\n");
        assert!(tyres_as_read(&mut s));
        assert_eq!(s[1].1.iter().map(|(k, _)| k.as_slice()).collect::<Vec<_>>(), [&b"NAME"[..], b"_EF"]);
        assert!(!tyres_as_read(&mut ini(b"[FRONT]\nNAME=x\n")));
        assert!(!tyres_as_read(&mut ini(b"[_EXTENSION]\nGENERATE_EXTRA_TYRES=1\n")));
        assert!(!tyres_as_read(&mut ini(b"[_EXTENSION]\nGENERATE_EXTRA_TYRES=0\n[FRONT_1]\n_VIRTUAL_ORIGIN=0\n")));
    }

    #[test]
    fn lut() {
        assert_eq!(parse_lut(b" (1=2|3 = 4, 9 |5=)"), [[1.0, 2.0], [3.0, 4.0]]);
        assert_eq!(parse_lut(b"; c\n1|2\n// x\n 3 | 4,5\n|7\n8|\n"), [[1.0, 2.0], [3.0, 4.0]]);
        assert_eq!(parse_lut(b"abc"), [[0.0f32; 2]; 0]);
        let v = [[0.0, 0.0], [1.0, 10.0], [2.0, 0.0]];
        assert_eq!(lut_value(&v, 0.5, false, false), 5.0);
        assert_eq!(lut_value(&v, 3.0, false, false), 0.0);
        assert_eq!(lut_value(&v, 3.0, false, true), -10.0);
        assert_eq!(lut_value(&v, 1.0, true, false), 10.0);
        assert_eq!(bounds(&v), ([0.0, 0.0], [2.0, 10.0]));
    }

    #[test]
    fn lut2d() {
        assert_eq!([strtof(b" 1.5e1x"), strtof(b"-.5"), strtof(b"+2"), strtof(b"3e"), strtof(b"1e+2"), strtof(b".e1"), strtof(b"")], [15.0, -0.5, 2.0, 3.0, 100.0, 0.0, 0.0]);
        let (k, v) = parse_inline2d(b"(0,0=1| 1 ,0=3|0,1=5|1,1=11|7|2=4)");
        assert_eq!(v, [1.0, 3.0, 5.0, 11.0, 4.0]);
        assert!(k[4] == [2.0, 2.0] && k[1] == [1.0, 0.0]);
        assert_eq!(parse_inline2d(b"(abc)").1.len(), 0);
        let g = build2d(&k[..4], &v[..4]);
        assert_eq!((g.y.as_slice(), g.x.as_slice(), g.min, g.max), (&[0.0, 1.0][..], &[0.0, 1.0][..], [0.0, 0.0, 1.0], [1.0, 1.0, 11.0]));
        assert_eq!([eval2d(&g, 0.0, 0.0), eval2d(&g, 0.0, 1.0), eval2d(&g, 1.0, 0.0), eval2d(&g, 0.5, 0.5), eval2d(&g, 2.0, 0.5)], [1.0, 3.0, 5.0, 5.0, 0.0]);
        let (k, v) = parse_file2d(b"; c\n\t0\t10 20\n0 1 2 3\n\n1 4 5 6\n").unwrap();
        assert_eq!((k[4], v), ([10.0, 1.0], vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0]));
        assert!(parse_file2d(b"0 1\n0 1\n").is_none() && parse_file2d(b"// x\n").is_none());
        assert!(parse_file2d(b"a -\n0 1 2\n").unwrap().0[0][0].is_nan());
    }
}
