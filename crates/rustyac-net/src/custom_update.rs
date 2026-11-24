// SPDX-License-Identifier: GPL-3.0-or-later

//! Custom Shaders Patch 0.3.2-preview2 (build 4305): the custom car update (`AB 03`), read with
//! the layout the server sends as text in its settings (`[EXTRA_DATA] CUSTOM_UPDATE_FORMAT`),
//! the way CSP parses it (`dwrite.dll` @ 0x180d77360, fields @ 0x180d75820 / 0x180d764d0 /
//! 0x180d754c0 / 0x180d75620) and reads it (@ 0x180d77280, 0x180d788a0, records @ 0x180d76dd0).
//! `docs/csp/assettoserver.md` §NH-5.3 has the rules.
//!
//! * The text is lines of `name: spec`. `packet:` starts a layout (with more than one, the
//!   datagram's first byte picks it), `group:` (a count byte, then that many records) or `single:`
//!   (one record) starts an entry, `skip: n` puts n bytes before the next entry, `padding: n`
//!   leaves n bytes in the record; `#` starts a comment.
//! * A number field is `type, modifiers…`: `*k` and `/k` change the scale and the offset, `+k` and
//!   `-k` the offset, `tyreAngularSpeed` AC's wheel-speed curve; its value is `raw × scale +
//!   offset`. A field the text doesn't name keeps the slot's last value.

use rustyac_math::powf;

use crate::datagram::Incoming;
use crate::net_car::NetCarState;

/// How a field's bytes become a number (the readers @ 0x180d76280..0x180d764c0, 0x180d76c90..).
#[derive(Clone, Copy, Debug, PartialEq)]
enum Raw {
    F32,
    Half,
    I32,
    U32,
    I16,
    U16,
    U8,
    I8,
    /// `pbyteNM`: N bits from bit N × M of a byte.
    Bits { shift: u8, mask: u8 },
}

impl Raw {
    fn size(self) -> i32 {
        match self {
            Raw::F32 | Raw::I32 | Raw::U32 => 4,
            Raw::Half | Raw::I16 | Raw::U16 => 2,
            Raw::U8 | Raw::I8 | Raw::Bits { .. } => 1,
        }
    }

    fn read(self, b: &[u8]) -> f32 {
        let u16_at = || u16::from_le_bytes([b[0], b[1]]);
        let u32_at = || u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
        match self {
            Raw::F32 => f32::from_bits(u32_at()),
            Raw::Half => half(u16_at()),
            Raw::I32 => u32_at() as i32 as f32,
            Raw::U32 => u32_at() as f32,
            Raw::I16 => u16_at() as i16 as f32,
            Raw::U16 => u16_at() as f32,
            Raw::U8 => b[0] as f32,
            Raw::I8 => b[0] as i8 as f32,
            Raw::Bits { shift, mask } => ((b[0] >> shift) & mask) as f32,
        }
    }
}

/// IEEE half to float, as CSP's tables give it (equal for all 65,536 halves:
/// `re/scratch/taskNH5/half_check.py`).
fn half(h: u16) -> f32 {
    let sign = ((h & 0x8000) as u32) << 16;
    let (exponent, mantissa) = ((h >> 10) & 0x1f, (h & 0x3ff) as u32);
    f32::from_bits(
        sign | match exponent {
            0 => (mantissa as f32 * f32::from_bits(0x3380_0000)).to_bits(),
            31 => 0x7f80_0000 | mantissa << 13,
            e => (e as u32 + 112) << 23 | mantissa << 13,
        },
    )
}

/// `cvttss2si`: out of range and NaN give `i32::MIN`.
fn truncate(x: f32) -> i32 {
    if (-2_147_483_648.0..2_147_483_648.0).contains(&x) { x as i32 } else { i32::MIN }
}

/// A number field: where it is in the record (-1: not in the format) and how it reads.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Number {
    at: i32,
    raw: Raw,
    scale: f32,
    offset: f32,
    tyre: bool,
}

impl Default for Number {
    fn default() -> Number {
        Number { at: -1, raw: Raw::U8, scale: 1.0, offset: 0.0, tyre: false }
    }
}

impl Number {
    /// @ 0x180d75780: `mulss`, `addss` (so -0.0 comes out +0.0), then AC's wheel-speed curve.
    fn value(&self, record: &[u8]) -> f32 {
        let x = self.raw.read(&record[self.at as usize..]) * self.scale + self.offset;
        if !self.tyre {
            return x;
        }
        let sign = if x < 0.0 { -1.0f32 } else { 1.0 };
        (powf(10.0, f32::from_bits(x.to_bits() & 0x7fff_ffff) * f32::from_bits(0x3d4c_cccd)) - 1.0) * sign
    }
}

/// A 3D vector field (`float3` and the rest).
#[derive(Clone, Copy, Debug, PartialEq)]
struct Vector {
    at: i32,
    raw: Raw,
    scale: f32,
    offset: f32,
}

impl Default for Vector {
    fn default() -> Vector {
        Vector { at: -1, raw: Raw::F32, scale: 1.0, offset: 0.0 }
    }
}

/// A `group:` or `single:` entry: its record layout.
#[derive(Clone, Debug, Default, PartialEq)]
struct Entry {
    /// Bytes before the entry (`skip`).
    skip: i32,
    size: i32,
    /// `group`: a count byte, then that many records.
    multiple: bool,
    session_id: i32,
    pak_sequence_id: i32,
    timestamp: i32,
    status_bytes: i32,
    ping: Number,
    gas: Number,
    clutch: Number,
    brake: Number,
    performance_delta: Number,
    tyre_angular_speed: [Number; 4],
    steer_angle: Number,
    wheel_angle: Number,
    engine_rpm: Number,
    gear: Number,
    pos: Vector,
    rotation: Vector,
    velocity: Vector,
}

/// A parsed `CUSTOM_UPDATE_FORMAT`: the packets, each a list of entries.
#[derive(Clone, Debug, PartialEq)]
pub struct Format {
    packets: Vec<Vec<Entry>>,
}

/// The whitespace CSP trims: space, tab, CR.
fn trim(s: &str) -> &str {
    s.trim_matches([' ', '\t', '\r'])
}

/// CSP's own number reader (@ 0x1815d5b90): not correctly rounded, so ported as it is.
fn number(s: &[u8], default: f64) -> f64 {
    let at = |i: usize| s.get(i).copied().unwrap_or(0);
    if s.is_empty() {
        return default;
    }
    let mut i = 0;
    let sign = if at(0) == b'-' { -1.0 } else { 1.0 };
    if matches!(at(0), b'-' | b'+') {
        i = 1;
    }
    let start = i;
    let (mut whole, mut fraction) = (0.0f64, 0.0f64);
    while at(i).is_ascii_digit() {
        whole = whole * 10.0 + (at(i) - b'0') as f64;
        i += 1;
    }
    if at(i) == b'.' {
        let mut step = 0.1f64;
        loop {
            i += 1;
            if !at(i).is_ascii_digit() {
                break;
            }
            let digit = (at(i) - b'0') as f64 * step;
            step *= 0.1;
            fraction += digit;
        }
        if !matches!(at(i), b'E' | b'e') {
            return sign * (fraction + whole);
        }
    } else if !matches!(at(i), b'E' | b'e') {
        return if i == start { default } else { sign * whole };
    }
    i += 1;
    let exponent_sign = if at(i) == b'-' { -1 } else { 1 };
    if matches!(at(i), b'-' | b'+') {
        i += 1;
    }
    let mut exponent = 0i32;
    while at(i).is_ascii_digit() {
        exponent = (at(i) as i32).wrapping_add(exponent.wrapping_mul(10)).wrapping_sub(48);
        i += 1;
    }
    // @ 0x1815d58a0 (CSP loops forever on i32::MIN; this stops)
    let exponent = exponent.wrapping_mul(exponent_sign);
    let (mut power, mut base) = (1.0f64, if exponent < 0 { 0.1 } else { 10.0 });
    let mut left = exponent.unsigned_abs();
    while left != 0 {
        if left & 1 != 0 {
            power *= base;
        }
        left >>= 1;
        base *= base;
    }
    power * ((fraction + whole) * sign)
}

/// `skip` / `padding`: `0x` hex or decimal (no digits: -1), then must not be negative.
fn count(s: &str, what: &str) -> Result<i32, String> {
    let b = s.as_bytes();
    let value = if b.len() > 2 && b[0] == b'0' && matches!(b[1], b'x' | b'X') {
        b[2..].iter().map_while(|c| (*c as char).to_digit(16)).fold(0i64, |v, d| v.wrapping_mul(16) | d as i64)
    } else {
        let digits = b.strip_prefix(b"-").unwrap_or(b);
        let n = digits.iter().take_while(|c| c.is_ascii_digit()).fold(0i64, |v, c| v.wrapping_mul(10).wrapping_add((c - b'0') as i64));
        match digits.iter().take_while(|c| c.is_ascii_digit()).count() {
            0 => -1,
            _ if b.first() == Some(&b'-') => n.wrapping_neg(),
            _ => n,
        }
    };
    if (value as i32) < 0 {
        return Err(format!("Incorrect {what} value: {}", value as i32));
    }
    Ok(value as i32)
}

/// The comma list of a field's spec, trimmed, empty items dropped.
fn items(spec: &str) -> Vec<&str> {
    spec.split(',').map(trim).filter(|s| !s.is_empty()).collect()
}

/// A number field (@ 0x180d75820). `text` is the whole format: `pbyte` reads its digits from there.
fn number_field(field: &mut Number, name: &str, cursor: &mut i32, parts: &[&str], text: &str) -> Result<(), String> {
    if field.at != -1 {
        return Err(format!("Field {name} is already set"));
    }
    field.raw = Raw::U8;
    let (mut size, mut back) = (1, 0);
    if let [_, spec] = parts {
        let items = items(spec);
        let kind = items[0];
        if kind != "-" {
            field.raw = match kind {
                "float" => Raw::F32,
                "half" => Raw::Half,
                "int" => Raw::I32,
                "uint" => Raw::U32,
                "short" => Raw::I16,
                "ushort" => Raw::U16,
                "byte" => Raw::U8,
                "char" => Raw::I8,
                // CSP checks for seven list items here where it means seven letters: a quirk, kept
                _ if kind.starts_with("pbyte") && items.len() == 7 => {
                    let at = kind.as_ptr() as usize - text.as_ptr() as usize;
                    let digit = |i| text.as_bytes().get(at + i).copied().unwrap_or(0) as i32 - 48;
                    let (bits, index) = (digit(5), digit(6));
                    if !matches!(bits, 1 | 2 | 4) {
                        return Err(format!("Incorrect pbyte size: {bits}"));
                    }
                    if index < 0 || index >= 8 / bits {
                        return Err(format!("Incorrect pbyte offset: {index}"));
                    }
                    (size, back) = (i32::from(index == 0), i32::from(index != 0));
                    Raw::Bits { shift: (bits * index) as u8, mask: ((1 << bits) - 1) as u8 }
                }
                _ => return Err(format!("Unknown type for a number {name}: {kind}")),
            };
            if !matches!(field.raw, Raw::Bits { .. }) {
                size = field.raw.size();
            }
        }
        for item in &items[1..] {
            let k = |default| number(&item.as_bytes()[1..], default) as f32;
            match item.as_bytes()[0] {
                b'*' => (field.scale, field.offset) = (k(1.0) * field.scale, k(1.0) * field.offset),
                b'/' => (field.scale, field.offset) = (field.scale / k(1.0), field.offset / k(1.0)),
                b'+' => field.offset += k(0.0),
                b'-' => field.offset -= k(0.0),
                _ if *item == "tyreAngularSpeed" => field.tyre = true,
                _ => return Err(format!("Unknown modifier for a number {name}: {item}")),
            }
        }
    }
    field.at = *cursor - back;
    *cursor += size;
    Ok(())
}

/// A 3D vector field (@ 0x180d764d0).
fn vector_field(field: &mut Vector, name: &str, cursor: &mut i32, parts: &[&str]) -> Result<(), String> {
    if field.at != -1 {
        return Err(format!("Field {name} is already set"));
    }
    let mut size = 12;
    if let [_, spec] = parts {
        let items = items(spec);
        (field.raw, size) = match items[0] {
            "-" | "float3" => (Raw::F32, 12),
            "half3" => (Raw::Half, 6),
            "char3" => (Raw::I8, 3),
            "byte3" => (Raw::U8, 3),
            "ushort3" => (Raw::U16, 6),
            "short3" => (Raw::I16, 6),
            kind => return Err(format!("Unknown type for a 3D vector {name}: {kind}")),
        };
        for item in &items[1..] {
            let k = |default| number(&item.as_bytes()[1..], default) as f32;
            match item.as_bytes()[0] {
                b'*' => (field.scale, field.offset) = (k(1.0) * field.scale, k(1.0) * field.offset),
                b'/' => (field.scale, field.offset) = (field.scale / k(1.0), field.offset / k(1.0)),
                b'+' => field.offset += k(0.0),
                b'-' => field.offset -= k(0.0),
                _ => return Err(format!("Unknown modifier for a 3D vector {name}: {item}")),
            }
        }
    }
    field.at = *cursor;
    *cursor += size;
    Ok(())
}

/// `sessionID` / `pakSequenceID` (a byte, @ 0x180d754c0) and `timestamp` / `statusBytes` (four
/// bytes, @ 0x180d75620): only `-` or that one type may be named.
fn fixed_field(at: &mut i32, name: &str, cursor: &mut i32, parts: &[&str], kind: &str, size: i32) -> Result<(), String> {
    if *at != -1 {
        return Err(format!("Field {name} is already set"));
    }
    if matches!(parts, [_, spec] if *spec != kind && *spec != "-") {
        return Err(format!("Only “{kind}” is allowed for {name}"));
    }
    *at = *cursor;
    *cursor += size;
    Ok(())
}

impl Format {
    /// Parse the server's text; an error is CSP's message ("Failed to initialize: …" in its log,
    /// and no `AB 03` is read).
    pub fn parse(text: &str) -> Result<Format, String> {
        let mut packets: Vec<Vec<Entry>> = Vec::new();
        let mut entry: Option<usize> = None;
        let mut skip = 0i32;
        for line in text.split('\n').map(trim).filter(|line| !line.is_empty()) {
            let line = line.split('#').next().unwrap_or("");
            let parts: Vec<&str> = line.split(':').map(trim).collect();
            match parts.as_slice() {
                [""] => continue,
                [word, ""] => match *word {
                    "packet" => {
                        packets.push(Vec::new());
                        entry = None;
                    }
                    "group" | "single" => {
                        let packet = packets.last_mut().ok_or("First, use “packet:” to start a new packet description")?;
                        packet.push(Entry {
                            skip: std::mem::take(&mut skip),
                            multiple: *word == "group",
                            session_id: -1,
                            pak_sequence_id: -1,
                            timestamp: -1,
                            status_bytes: -1,
                            ..Entry::default()
                        });
                        entry = Some(packet.len() - 1);
                    }
                    _ => return Err(format!("Unexpected line: {line}")),
                },
                ["skip", value] => {
                    // ends the entry: the bytes go before the next one
                    entry = None;
                    skip = skip.wrapping_add(count(value, "skip")?);
                }
                _ => {
                    let Some(index) = entry else {
                        return Err(format!("Unexpected line: {line} (to define fields, first start a group or a single entry)"));
                    };
                    let e = &mut packets.last_mut().expect("an entry is in a packet")[index];
                    let name = parts[0];
                    let cursor = &mut e.size;
                    match name {
                        "sessionID" => fixed_field(&mut e.session_id, name, cursor, &parts, "byte", 1)?,
                        "pakSequenceID" => fixed_field(&mut e.pak_sequence_id, name, cursor, &parts, "byte", 1)?,
                        "timestamp" => fixed_field(&mut e.timestamp, name, cursor, &parts, "uint", 4)?,
                        "statusBytes" => fixed_field(&mut e.status_bytes, name, cursor, &parts, "uint", 4)?,
                        "pos" => vector_field(&mut e.pos, name, cursor, &parts)?,
                        "rotation" => vector_field(&mut e.rotation, name, cursor, &parts)?,
                        "velocity" => vector_field(&mut e.velocity, name, cursor, &parts)?,
                        "wheelAngle" => number_field(&mut e.wheel_angle, name, cursor, &parts, text)?,
                        "engineRPM" => number_field(&mut e.engine_rpm, name, cursor, &parts, text)?,
                        "gas" => number_field(&mut e.gas, name, cursor, &parts, text)?,
                        "clutch" => number_field(&mut e.clutch, name, cursor, &parts, text)?,
                        "brake" => number_field(&mut e.brake, name, cursor, &parts, text)?,
                        "ping" => number_field(&mut e.ping, name, cursor, &parts, text)?,
                        "tyreAngularSpeedFL" => number_field(&mut e.tyre_angular_speed[0], name, cursor, &parts, text)?,
                        "tyreAngularSpeedFR" => number_field(&mut e.tyre_angular_speed[1], name, cursor, &parts, text)?,
                        "tyreAngularSpeedRL" => number_field(&mut e.tyre_angular_speed[2], name, cursor, &parts, text)?,
                        "tyreAngularSpeedRR" => number_field(&mut e.tyre_angular_speed[3], name, cursor, &parts, text)?,
                        "steerAngle" => number_field(&mut e.steer_angle, name, cursor, &parts, text)?,
                        "gear" => number_field(&mut e.gear, name, cursor, &parts, text)?,
                        "performanceDelta" => number_field(&mut e.performance_delta, name, cursor, &parts, text)?,
                        "padding" => match parts.as_slice() {
                            [_, value] => *cursor += count(value, "padding")?,
                            _ => return Err("Padding: parameter is required".into()),
                        },
                        // CSP passes over a name it doesn't know, taking no bytes
                        _ => {}
                    }
                }
            }
        }
        if packets.is_empty() {
            return Err("No valid definitions found".into());
        }
        for entry in packets.iter().flatten() {
            for (at, name) in [(entry.session_id, "sessionID"), (entry.pak_sequence_id, "pakSequenceID"), (entry.timestamp, "timestamp")] {
                if at == -1 {
                    return Err(format!("Field “{name}” is not set"));
                }
            }
        }
        Ok(Format { packets })
    }
}

/// The reader: the format and each slot's last state, which a record starts from.
#[derive(Clone, Debug)]
pub struct Reader {
    format: Format,
    last: Vec<NetCarState>,
}

impl Reader {
    pub fn new(format: Format) -> Reader {
        Reader { format, last: vec![NetCarState::default(); 256] }
    }

    /// The server's settings' format: `None` without the key (CSP reads no `AB 03` then),
    /// `Some(Err)` when CSP would fail to parse it.
    pub fn from_settings(settings: &[crate::csp::IniSection]) -> Option<Result<Reader, String>> {
        let (_, keys) = settings.iter().find(|(section, _)| section == "EXTRA_DATA")?;
        let (_, text) = keys.iter().find(|(key, _)| key == "CUSTOM_UPDATE_FORMAT")?;
        Some(Format::parse(text).map(Reader::new))
    }

    /// `AB 03`: the states in it, in order. A short datagram ends with `Truncated` (CSP throws
    /// "Unexpected end": the records before it stay read).
    pub fn decode(&mut self, datagram: &[u8]) -> Vec<Incoming> {
        let mut out = Vec::new();
        let truncated = Incoming::Truncated(crate::csp::EXTENDED);
        let mut body = datagram.get(2..).unwrap_or_default();
        let last = &mut self.last;
        let entries = match self.format.packets.as_slice() {
            [only] => only,
            packets => {
                let Some((&pick, rest)) = body.split_first() else {
                    return vec![truncated];
                };
                body = rest;
                // shortcut: CSP reads past its list on a selector it has no packet for (game fault), dropped here
                let Some(entries) = packets.get(pick as usize) else {
                    return out;
                };
                entries
            }
        };
        let mut pos = 0usize;
        for entry in entries {
            pos += entry.skip.max(0) as usize;
            let records = if entry.multiple {
                let Some(&n) = body.get(pos) else {
                    out.push(truncated);
                    return out;
                };
                pos += 1;
                n as usize
            } else {
                1
            };
            for _ in 0..records {
                let Some(record) = body.get(pos..pos + entry.size as usize) else {
                    out.push(truncated);
                    return out;
                };
                pos += entry.size as usize;
                out.push(Reader::record(last, entry, record));
            }
        }
        out
    }

    /// One record (@ 0x180d76dd0), on a copy of the slot's last state.
    fn record(last: &mut [NetCarState], e: &Entry, record: &[u8]) -> Incoming {
        let at = |at: i32| (at >= 0).then(|| &record[at as usize..]);
        let slot = at(e.session_id).map_or(0xff, |b| b[0]);
        let mut state = last[slot as usize];
        for (v, out) in [(&e.pos, &mut state.pos), (&e.rotation, &mut state.rotation), (&e.velocity, &mut state.velocity)] {
            if v.at >= 0 {
                let w = (v.raw.size()) as usize;
                *out = [0, 1, 2].map(|i| v.raw.read(&record[v.at as usize + i * w..]) * v.scale + v.offset);
            }
        }
        if let Some(b) = at(e.pak_sequence_id) {
            state.pak_sequence_id = b[0];
        }
        if let Some(b) = at(e.timestamp) {
            // signed, where AC's 0x48 reads it unsigned
            state.time_stamp = i32::from_le_bytes([b[0], b[1], b[2], b[3]]) as f64;
        }
        let number = |n: &Number| (n.at >= 0).then(|| n.value(record));
        if let Some(x) = number(&e.ping) {
            state.ping = truncate(x);
        }
        if let Some(x) = number(&e.gas) {
            state.gas = x;
        }
        // shortcut: brake / clutch go into the car object (+2380 / +2384, not mapped); no known server sends them, map them when one does
        if let Some(x) = number(&e.performance_delta) {
            // no × 0.001: CSP keeps the server's milliseconds where AC's 0x46 has seconds
            state.performance_delta = x;
        }
        for wheel in 0..4 {
            if let Some(x) = number(&e.tyre_angular_speed[wheel]) {
                state.tyre_angular_speed[wheel] = x;
            }
        }
        if let Some(x) = number(&e.steer_angle) {
            state.steer_angle = x;
        }
        if let Some(x) = number(&e.wheel_angle) {
            state.wheel_angle = x;
        }
        if let Some(x) = number(&e.engine_rpm) {
            let rpm = truncate(x);
            state.engine_rpm = if rpm < 0 { 0 } else if rpm < 0xffff { rpm as u16 } else { 0xffff };
        }
        if let Some(x) = number(&e.gear) {
            state.gear_index = truncate(x);
        }
        if let Some(b) = at(e.status_bytes) {
            state.status_bytes = u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
        }
        // the slot's copy is replaced only by a newer state
        if state.time_stamp > last[slot as usize].time_stamp {
            last[slot as usize] = state;
        }
        Incoming::CarState { slot, state }
    }
}

/// AssettoServer pre49's format (`CSPPositionUpdate`), for tools and tests; a client reads the
/// one its server sends.
pub const ASSETTO_SERVER: &str = "packet:
  group:
    sessionID
    pakSequenceID
    timestamp
    ping: ushort
    pos: float3
    rotation: float3
    velocity: float3
    tyreAngularSpeedFL: byte, -100, tyreAngularSpeed
    tyreAngularSpeedFR: byte, -100, tyreAngularSpeed
    tyreAngularSpeedRL: byte, -100, tyreAngularSpeed
    tyreAngularSpeedRR: byte, -100, tyreAngularSpeed
    steerAngle: byte, -127
    wheelAngle: byte, -127, /2
    engineRPM: ushort
    gear: byte
    statusBytes
    gas: byte, /255
    performanceDelta: short";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn assetto_server_layout() {
        let format = Format::parse(ASSETTO_SERVER).unwrap();
        let e = &format.packets[0][0];
        assert!(e.multiple && e.size == 60);
        assert_eq!((e.session_id, e.pak_sequence_id, e.timestamp, e.ping.at, e.pos.at, e.status_bytes, e.performance_delta.at), (0, 1, 2, 6, 8, 53, 58));
        // `byte, -127, /2`: the modifiers act on the offset too
        assert_eq!((e.wheel_angle.scale, e.wheel_angle.offset), (0.5, -63.5));
        assert_eq!(e.gas.scale.to_bits(), 0x3b80_8081);
    }

    #[test]
    fn custom_update_record() {
        // the first AB 03 of Super's capture (taskNH7 cap/udp.log, 121.535 s)
        let datagram: Vec<u8> = (0..126).step_by(2).map(|i| u8::from_str_radix(&"ab03010200d4240000000022d192c3b94ca64134242fc448586ec000e69b3c44781a3b27f734c1f256c83e935789c1888888887f7f130b0200000000000000"[i..i + 2], 16).unwrap()).collect();
        let mut reader = Reader::new(Format::parse(ASSETTO_SERVER).unwrap());
        let decoded = reader.decode(&datagram);
        assert_eq!(decoded.len(), 1);
        let Incoming::CarState { slot, state } = &decoded[0] else { panic!() };
        assert_eq!((*slot, state.pak_sequence_id, state.time_stamp, state.ping), (2, 0, 9428.0, 0));
        assert_eq!(state.pos, [f32::from_bits(0xc392d122), f32::from_bits(0x41a64cb9), f32::from_bits(0xc42f2434)]);
        assert_eq!((state.steer_angle, state.wheel_angle, state.engine_rpm, state.gear_index, state.status_bytes, state.gas), (0.0, 0.0, 2835, 2, 0, 0.0));
        assert_eq!(state.tyre_angular_speed, [crate::datagram::decode_wheel_speed(136); 4]);
        assert_eq!(reader.decode(&datagram[..40]), vec![Incoming::Truncated(0xab)]);
    }

    #[test]
    fn made_up_layout() {
        // two packets, a skip, a single entry, padding, comments and the other types
        let text = "packet: # the first
  group:
    sessionID
    pakSequenceID
    timestamp
packet:
  skip: 0x2
  single:
    timestamp: uint
    sessionID: -
    padding: 1
    pakSequenceID: byte
    pos: half3, *2, +1
    rotation: short3, /100
    gas: half
    gear: char, -1
    engineRPM: float, *1e3
    ping: int
    brake: byte, /255
    mystery: float
    statusBytes";
        let format = Format::parse(text).unwrap();
        assert_eq!(format.packets[0][0].size, 6);
        let e = &format.packets[1][0];
        assert_eq!((e.skip, e.multiple, e.size), (2, false, 35));
        assert_eq!((e.timestamp, e.session_id, e.pak_sequence_id, e.pos.at, e.rotation.at, e.gas.at, e.gear.at, e.engine_rpm.at, e.ping.at, e.status_bytes), (0, 4, 6, 7, 13, 19, 21, 22, 26, 31));
        let mut datagram = vec![0xab, 0x03, 1, 0xee, 0xee];
        datagram.extend(1234i32.to_le_bytes());
        datagram.extend([7, 0, 9]);
        for h in [0x3c00u16, 0xc000, 0x0001] {
            datagram.extend(h.to_le_bytes());
        }
        for s in [100i16, -250, 0] {
            datagram.extend(s.to_le_bytes());
        }
        datagram.extend(0x3800u16.to_le_bytes());
        datagram.push(0xfe);
        datagram.extend(7.5f32.to_le_bytes());
        datagram.extend((-3i32).to_le_bytes());
        datagram.push(255);
        datagram.extend(0x20u32.to_le_bytes());
        let mut reader = Reader::new(format);
        let decoded = reader.decode(&datagram);
        let [Incoming::CarState { slot, state }] = decoded.as_slice() else { panic!("{decoded:?}") };
        assert_eq!((*slot, state.pak_sequence_id, state.time_stamp, state.ping, state.status_bytes), (7, 9, 1234.0, -3, 0x20));
        assert_eq!(state.pos, [3.0, -3.0, half(1) * 2.0 + 1.0]);
        assert_eq!(state.rotation, [1.0, -2.5, 0.0]);
        assert_eq!((state.gas, state.gear_index, state.engine_rpm), (0.5, -3, 7500));
        // a field the format leaves out keeps the slot's last value
        assert_eq!(state.steer_angle, 0.0);
        assert_eq!(reader.decode(&datagram[..20]), vec![Incoming::Truncated(0xab)]);
        assert_eq!(reader.decode(&[0xab, 0x03, 5]), vec![]);
    }

    #[test]
    fn csp_errors() {
        for (text, error) in [
            ("group:", "First, use “packet:” to start a new packet description"),
            ("packet:\nsessionID", "Unexpected line: sessionID (to define fields, first start a group or a single entry)"),
            ("packet:\ngroup:\nsessionID\npakSequenceID", "Field “timestamp” is not set"),
            ("packet:\ngroup:\nsessionID: char", "Only “byte” is allowed for sessionID"),
            ("packet:\ngroup:\ngas: byte, %2", "Unknown modifier for a number gas: %2"),
            ("packet:\ngroup:\ngas: pbyte40", "Unknown type for a number gas: pbyte40"),
            ("packet:\ngroup:\ngas: pbyte30, a, b, c, d, e, f", "Incorrect pbyte size: 3"),
            ("packet:\ngroup:\ngas\ngas", "Field gas is already set"),
            ("packet:\ngroup:\npadding", "Padding: parameter is required"),
            ("packet:\nskip: -1", "Incorrect skip value: -1"),
            ("", "No valid definitions found"),
        ] {
            assert_eq!(Format::parse(text), Err(error.to_string()), "{text}");
        }
    }

    #[test]
    fn numbers_and_halves() {
        assert_eq!(number(b"255", 1.0), 255.0);
        assert_eq!(number(b"", 1.0), 1.0);
        assert_eq!(number(b"-", 7.0), 7.0);
        assert_eq!(number(b".5x", 1.0), 0.5);
        assert_eq!(number(b"-2e-1", 0.0), 0.1 * -2.0);
        // CSP's fraction is a sum of digit × 0.1^n, not the nearest double
        assert_eq!(number(b"0.3", 0.0), 0.0 + 3.0 * 0.1);
        assert_eq!([half(0x3c00), half(0x8000).to_bits() as f32, half(0x7c00), half(0x0001)], [1.0, 2_147_483_648.0, f32::INFINITY, 5.960_464_5e-8]);
        assert!(half(0x7e00).is_nan());
        assert_eq!((truncate(f32::NAN), truncate(3e9), truncate(-2.9)), (i32::MIN, i32::MIN, -2));
    }

    #[test]
    fn pbyte_bits() {
        let text = "packet:\nsingle:\nsessionID\npakSequenceID\ntimestamp\ngear: pbyte40, a, b, c, d, e, f";
        let e = &Format::parse(text);
        // the modifiers aren't known, but the type is checked first: seven items make it a pbyte
        assert_eq!(e, &Err("Unknown modifier for a number gear: a".to_string()));
        let mut field = Number::default();
        let (mut cursor, text) = (3, "x: pbyte41, *1, *1, *1, *1, +0, -0");
        let parts: Vec<&str> = text.split(':').map(trim).collect();
        number_field(&mut field, "ping", &mut cursor, &parts, text).unwrap();
        assert_eq!((field.at, cursor, field.raw), (2, 3, Raw::Bits { shift: 4, mask: 15 }));
        assert_eq!(field.value(&[0, 0, 0xa5]), 10.0);
    }
}
