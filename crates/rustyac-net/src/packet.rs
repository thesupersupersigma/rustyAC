// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! AC's `UDPPacket` (ksNetwork.lib): the byte buffer every message is written into and
//! read from, on both transports. All numbers are little-endian and unaligned.
//!
//! Three kinds of text travel:
//!
//! | kind | on the wire | AC |
//! |---|---|---|
//! | narrow | `u8` length, then that many bytes | `writeString(std::string)` @ 0x14025dca0, `readString` @ 0x14025d9d0 |
//! | wide | `u8` length in characters, then 4 bytes per character (the UTF-16 unit, zero-extended) | `writeString(std::wstring)` @ 0x14025dd70, `readStringW` @ 0x14025da90 |
//! | big wide | `u16` length in characters, then 4 bytes per character | `readBigStringW` @ 0x14025d8f0 |
//!
//! A wide character is one UTF-16 unit of the game's `wchar_t` in a 32-bit cell: the reader
//! keeps the low 16 bits of each cell. Characters outside the basic plane therefore travel
//! as their two surrogates, one per cell.

/// A message being written.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Writer {
    pub data: Vec<u8>,
}

impl Writer {
    pub fn new() -> Writer {
        Writer { data: Vec::with_capacity(64) }
    }

    /// A message that starts with its id byte.
    pub fn with_id(id: u8) -> Writer {
        let mut writer = Writer::new();
        writer.u8(id);
        writer
    }

    pub fn u8(&mut self, value: u8) -> &mut Writer {
        self.data.push(value);
        self
    }

    pub fn u16(&mut self, value: u16) -> &mut Writer {
        self.data.extend_from_slice(&value.to_le_bytes());
        self
    }

    pub fn i16(&mut self, value: i16) -> &mut Writer {
        self.data.extend_from_slice(&value.to_le_bytes());
        self
    }

    pub fn u32(&mut self, value: u32) -> &mut Writer {
        self.data.extend_from_slice(&value.to_le_bytes());
        self
    }

    pub fn i32(&mut self, value: i32) -> &mut Writer {
        self.data.extend_from_slice(&value.to_le_bytes());
        self
    }

    pub fn u64(&mut self, value: u64) -> &mut Writer {
        self.data.extend_from_slice(&value.to_le_bytes());
        self
    }

    pub fn f32(&mut self, value: f32) -> &mut Writer {
        self.data.extend_from_slice(&value.to_le_bytes());
        self
    }

    pub fn f64(&mut self, value: f64) -> &mut Writer {
        self.data.extend_from_slice(&value.to_le_bytes());
        self
    }

    pub fn vec3(&mut self, value: [f32; 3]) -> &mut Writer {
        self.f32(value[0]).f32(value[1]).f32(value[2])
    }

    pub fn bytes(&mut self, bytes: &[u8]) -> &mut Writer {
        self.data.extend_from_slice(bytes);
        self
    }

    /// `UDPPacket::writeString(std::string)`: at most 254 bytes (the game throws on more;
    /// here the text is cut).
    pub fn string(&mut self, text: &[u8]) -> &mut Writer {
        let text = &text[..text.len().min(254)];
        self.u8(text.len() as u8);
        self.bytes(text)
    }

    /// `UDPPacket::writeString(std::wstring)`: at most 254 UTF-16 units (the game writes
    /// nothing at all for more and logs an error; here the text is cut).
    pub fn wide(&mut self, text: &str) -> &mut Writer {
        let units: Vec<u16> = text.encode_utf16().take(254).collect();
        self.u8(units.len() as u8);
        for unit in units {
            self.u32(unit as u32);
        }
        self
    }

    /// `UDPPacket::writeStringANSI`: a wide string sent narrow (`wstring2string`: the low
    /// byte of each unit).
    pub fn ansi(&mut self, text: &str) -> &mut Writer {
        let bytes: Vec<u8> = text.encode_utf16().map(|unit| unit as u8).collect();
        self.string(&bytes)
    }

    pub fn finish(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.data)
    }
}

/// A message being read. Reading past the end gives zeros and sets [`Reader::overrun`]
/// (the game reads whatever its 64 KB buffer holds there).
#[derive(Clone, Debug)]
pub struct Reader<'a> {
    pub data: &'a [u8],
    pub pos: usize,
    pub overrun: bool,
}

impl<'a> Reader<'a> {
    pub fn new(data: &'a [u8]) -> Reader<'a> {
        Reader { data, pos: 0, overrun: false }
    }

    fn take<const N: usize>(&mut self) -> [u8; N] {
        let mut out = [0u8; N];
        if self.pos + N <= self.data.len() {
            out.copy_from_slice(&self.data[self.pos..self.pos + N]);
        } else {
            self.overrun = true;
            let have = self.data.len().saturating_sub(self.pos);
            out[..have].copy_from_slice(&self.data[self.data.len() - have..]);
        }
        self.pos += N;
        out
    }

    /// `UDPPacket::isEOF`
    pub fn is_eof(&self) -> bool {
        self.pos >= self.data.len()
    }

    pub fn remaining(&self) -> usize {
        self.data.len().saturating_sub(self.pos)
    }

    pub fn u8(&mut self) -> u8 {
        self.take::<1>()[0]
    }

    pub fn bool(&mut self) -> bool {
        self.u8() != 0
    }

    pub fn u16(&mut self) -> u16 {
        u16::from_le_bytes(self.take())
    }

    pub fn i16(&mut self) -> i16 {
        i16::from_le_bytes(self.take())
    }

    pub fn u32(&mut self) -> u32 {
        u32::from_le_bytes(self.take())
    }

    pub fn i32(&mut self) -> i32 {
        i32::from_le_bytes(self.take())
    }

    pub fn u64(&mut self) -> u64 {
        u64::from_le_bytes(self.take())
    }

    pub fn i64(&mut self) -> i64 {
        i64::from_le_bytes(self.take())
    }

    pub fn f32(&mut self) -> f32 {
        f32::from_le_bytes(self.take())
    }

    pub fn f64(&mut self) -> f64 {
        f64::from_le_bytes(self.take())
    }

    pub fn vec3(&mut self) -> [f32; 3] {
        [self.f32(), self.f32(), self.f32()]
    }

    pub fn bytes(&mut self, count: usize) -> Vec<u8> {
        let mut out = vec![0u8; count];
        let have = count.min(self.remaining());
        if have > 0 {
            out[..have].copy_from_slice(&self.data[self.pos..self.pos + have]);
        }
        if have < count {
            self.overrun = true;
        }
        self.pos = self.pos.saturating_add(count);
        out
    }

    /// `UDPPacket::readString`, as raw bytes.
    pub fn string_bytes(&mut self) -> Vec<u8> {
        let length = self.u8() as usize;
        self.bytes(length)
    }

    /// `UDPPacket::readString`, each byte as one character (the game's narrow strings are
    /// in the system's code page; names of cars, tracks and skins are ASCII).
    pub fn string(&mut self) -> String {
        self.string_bytes().into_iter().map(|byte| byte as char).collect()
    }

    fn wide_units(&mut self, count: usize) -> String {
        let mut units = Vec::with_capacity(count);
        for _ in 0..count {
            units.push(self.u32() as u16);
        }
        String::from_utf16_lossy(&units)
    }

    /// `UDPPacket::readStringW`
    pub fn wide(&mut self) -> String {
        let count = self.u8() as usize;
        self.wide_units(count)
    }

    /// `UDPPacket::readBigStringW`
    pub fn big_wide(&mut self) -> String {
        let count = self.u16() as usize;
        self.wide_units(count)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn reads_past_the_end_give_zeros_and_never_panic() {
        let mut r = super::Reader::new(&[3]);
        assert_eq!(r.u8(), 3);
        assert_eq!(r.u32(), 0);
        assert_eq!(r.string(), "");
        assert_eq!(r.bytes(4), vec![0; 4]);
        assert_eq!(r.wide(), "");
        assert!(r.overrun);
    }

    use super::*;

    #[test]
    fn strings_round_trip() {
        let mut w = Writer::with_id(0x47);
        w.string(b"spa").wide("h\u{e9}llo \u{1f600}").u16(0xbeef).f32(1.5).ansi("SV");
        let data = w.finish();
        assert_eq!(&data[..5], &[0x47, 3, b's', b'p', b'a']);
        // 6 basic-plane characters + one surrogate pair = 8 cells
        assert_eq!(data[5], 8);
        let mut r = Reader::new(&data);
        assert_eq!(r.u8(), 0x47);
        assert_eq!(r.string(), "spa");
        assert_eq!(r.wide(), "h\u{e9}llo \u{1f600}");
        assert_eq!(r.u16(), 0xbeef);
        assert_eq!(r.f32(), 1.5);
        assert_eq!(r.string(), "SV");
        assert!(r.is_eof() && !r.overrun);
        assert_eq!(r.u32(), 0);
        assert!(r.overrun);
    }
}
