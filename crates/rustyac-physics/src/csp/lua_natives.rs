// SPDX-License-Identifier: GPL-3.0-or-later

//! Task C6b part 4: CSP's `ffi.C.lj_*` natives of a car physics state ported, each against CSP's
//! function in csp_oracle (proof case `csp lua env`). CSP's bytecode declares their C types, so
//! each takes and returns what CSP's does: strings out as an MSVC `std::string` (CSP returns one
//! per thread and native, valid until that native runs again on the thread).

use std::cell::RefCell;
use std::ffi::{c_char, c_void, CStr};

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

pub(super) fn set_car_data(car_index: i32, dir: &str) {
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
}
