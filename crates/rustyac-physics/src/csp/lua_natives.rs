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

/// The ported natives by their `ffi.C` name.
pub fn native(name: &[u8]) -> Option<*mut c_void> {
    Some(match name {
        b"lj_encodeBase64" => lj_encode_base64 as *mut c_void,
        b"lj_decodeBase64" => lj_decode_base64 as *mut c_void,
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
}
