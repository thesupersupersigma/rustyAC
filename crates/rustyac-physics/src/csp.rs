// SPDX-License-Identifier: GPL-3.0-or-later

//! Custom Shaders Patch mode (`--csp`, `[CSP] ENABLED`): what CSP 0.3.2-preview2 (build 4305,
//! `dwrite.dll`) changes in the game's reading of content, as options of rustyAC's own modules
//! (docs/csp/plan.md decision 1: the hooking is not ported). Off, every reader is Vanilla's.
//!
//! CSP mode is CSP as it runs with a physics worker count of 0 (every dispatch in list order, one
//! thread) and `RANDOMIZE_WHEEL_ANGLES=0` (docs/csp/port_c3.md).

pub mod air;
pub mod core_body;
pub mod core_car;
pub mod core_track;
pub mod inipp;
#[cfg(feature = "lua")]
pub mod inipp_lua;

use crate::data::ini::IniReader;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

static ENABLED: AtomicBool = AtomicBool::new(false);

/// Turns CSP mode on or off for this process (the master switch, read at content load).
pub fn set_enabled(on: bool) {
    ENABLED.store(on, Ordering::Relaxed);
}

/// Is CSP mode on?
pub fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

static CORE_SWITCH: AtomicBool = AtomicBool::new(false);

/// Task R1: the developer switch (`--csp-core`, `[CSP] CORE=1`) for CSP's own core. Off, CSP mode
/// is v1.1.4's: CSP's data on Vanilla's core (CSP's core matches CSP only up to the tyre step, C5j).
pub fn set_core_enabled(on: bool) {
    CORE_SWITCH.store(on, Ordering::Relaxed);
}

/// Is the CSP core switch on (with CSP mode)?
pub fn core_enabled() -> bool {
    enabled() && CORE_SWITCH.load(Ordering::Relaxed)
}

static CORE_REFUSED: AtomicBool = AtomicBool::new(false);
static REFUSALS: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

/// Does this session's physics run in CSP's own core (CSP mode, the core switch, and no car or
/// track that [`choose_core`] turned away)?
pub fn core() -> bool {
    core_enabled() && !CORE_REFUSED.load(Ordering::Relaxed)
}

/// Task C5i-c: before anything is built, CSP's core for the session unless a car of `cars` (their
/// data folders) has a strut suspension, or the track has loose objects (counted `with_objects`) or
/// does not build in CSP's core: those are not ported (docs/csp/core.md C5i-c), and the session
/// then runs on Vanilla's core with CSP's data, as `--csp` did before C5i-a, said once on the
/// screen ([`take_refusals`]) and in the log.
pub fn choose_core(cars: &[&Path], track: Option<&crate::track::Track>, with_objects: bool) {
    let unported = |car: &Path| {
        let strut = IniReader::load(&car.join("suspensions.ini")).is_ok_and(|ini| ["FRONT", "REAR"].iter().any(|s| ini.get_string(s, "TYPE") == "STRUT"));
        // (CSP's core refuses a box with a zero side: `CspCore::add_box_collider`)
        let flat = crate::car::colliders::load_boxes(car).is_ok_and(|boxes| boxes.iter().any(|b| b.size.x == 0.0 || b.size.z == 0.0));
        (strut || flat).then(|| format!("{} has {}", car.display(), if strut { "a strut suspension" } else { "a collider box with a zero side" }))
    };
    let objects = track.filter(|_| with_objects).map_or(0, |t| t.objects.len());
    let not_ready = |what: &str, why: String| Some(format!("CSP physics for this {what} isn't ready yet: it runs with Vanilla's physics and CSP's data ({why})"));
    let refusal = if let Some(why) = cars.iter().find_map(|car| unported(car)) {
        not_ready("car", why)
    } else if objects > 0 {
        not_ready("track", format!("the track has {objects} loose objects"))
    } else if let Some(Err(e)) = track.filter(|_| core_enabled()).map(|t| t.csp_core()) {
        not_ready("track", e)
    } else {
        None
    };
    CORE_REFUSED.store(refusal.is_some(), Ordering::Relaxed);
    if let Some(message) = refusal.filter(|_| core_enabled()) {
        eprintln!("CSP: {message}");
        REFUSALS.lock().unwrap().push(message);
    }
}

/// The [`choose_core`] refusals not shown yet, for the game's banner.
pub fn take_refusals() -> Vec<String> {
    std::mem::take(&mut *REFUSALS.lock().unwrap())
}

/// CSP's integer parser (`dwrite+0x15d58e0`): `0x` / `0X` hex (no digits = 0), else an optional `-`
/// and decimal digits up to the first other character, wrapping; no digits = `default`.
pub fn parse_int(text: &str, default: i64) -> i64 {
    let bytes = text.as_bytes();
    if bytes.len() >= 2 && bytes[0] == b'0' && (bytes[1] | 0x20) == b'x' {
        let digits = bytes[2..].iter().map_while(|&b| (b as char).to_digit(16));
        return digits.fold(0i64, |v, d| v << 4 | d as i64);
    }
    let negative = bytes.first() == Some(&b'-');
    let digits: Vec<i64> = bytes[negative as usize..].iter().map_while(|b| b.is_ascii_digit().then(|| (b - b'0') as i64)).collect();
    if digits.is_empty() {
        return default;
    }
    let value = digits.iter().fold(0i64, |v, &d| v.wrapping_mul(10).wrapping_add(d));
    if negative { value.wrapping_neg() } else { value }
}

/// CSP's float parser (`dwrite+0x15d5b90`): sign, digits, `.` digits, `e` / `E` signed exponent,
/// in doubles, as it computes them; no digits and no `.` = `default`.
pub fn parse_float(text: &str, default: f64) -> f64 {
    let bytes = text.as_bytes();
    if bytes.is_empty() {
        return default;
    }
    let mut at = 0;
    let mut sign = 1.0;
    match bytes[0] {
        b'-' => (sign, at) = (-1.0, 1),
        b'+' => at = 1,
        _ => {}
    }
    let digit = |at: usize| bytes.get(at).filter(|b| b.is_ascii_digit()).map(|b| (b - b'0') as f64);
    let (mut whole, mut fraction, mut any) = (0.0f64, 0.0f64, false);
    while let Some(d) = digit(at) {
        whole = whole * 10.0 + d;
        (at, any) = (at + 1, true);
    }
    if bytes.get(at) == Some(&b'.') {
        at += 1;
        let mut scale = 0.1;
        while let Some(d) = digit(at) {
            fraction += d * scale;
            scale *= 0.1;
            at += 1;
        }
        any = true;
    }
    if !matches!(bytes.get(at), Some(b'e' | b'E')) {
        return if any { sign * (fraction + whole) } else { default };
    }
    at += 1;
    let mut exponent_sign = 1;
    match bytes.get(at) {
        Some(b'-') => (exponent_sign, at) = (-1, at + 1),
        Some(b'+') => at += 1,
        _ => {}
    }
    let mut exponent = 0i32;
    while let Some(d) = digit(at) {
        exponent = exponent.wrapping_mul(10).wrapping_add(d as i32);
        at += 1;
    }
    pow10(exponent_sign * exponent) * ((fraction + whole) * sign)
}

/// `dwrite+0x15d58a0`: 10^n by squaring (0.1 for a negative n).
fn pow10(n: i32) -> f64 {
    let (mut result, mut base, mut n) = (1.0f64, if n < 0 { 0.1 } else { 10.0 }, n.unsigned_abs());
    while n != 0 {
        if n & 1 != 0 {
            result *= base;
        }
        base *= base;
        n >>= 1;
    }
    result
}

/// `WAV_PITCH` of a surfaces.ini `[SURFACE_n]` under CSP (`INIReader::getFloat` detour
/// `dwrite+0x5e4180`): `extended-N` is N; `None` when the value is not of that form.
pub fn wav_pitch(value: &str) -> Option<f32> {
    value.strip_prefix("extended-").map(|rest| parse_float(rest, 0.0) as f32)
}

/// car.ini `[HEADER] VERSION` as `CarAvatar::initCommon` @ 0x1400d56e0 reads it: Vanilla's
/// `getInt` (`std::stoi`, which refuses `extended-N`); under CSP its `getInt` detour
/// (`dwrite+0x5e3590`): `extended-N` / `extendedV2-N` are N, a value starting with a digit is
/// Vanilla's read, anything else (empty, `Extended-2`, words) is 2. The flag: the `extended` form.
pub fn car_version(ini: &IniReader, csp: bool) -> Result<(i32, Option<bool>), String> {
    if !ini.ready || !ini.has_key("HEADER", "VERSION") {
        return Ok((1, None));
    }
    let text = ini.get_string("HEADER", "VERSION");
    if csp {
        if let Some(rest) = text.strip_prefix("extendedV2-") {
            return Ok((parse_int(rest, 0) as i32, Some(true)));
        }
        if let Some(rest) = text.strip_prefix("extended-") {
            return Ok((parse_int(rest, 0) as i32, Some(false)));
        }
        if !text.as_bytes().first().is_some_and(u8::is_ascii_digit) {
            return Ok((2, None));
        }
    }
    let version = ini.get_int("HEADER", "VERSION")?;
    Ok((version, None))
}

/// One of CSP's own sections (name starting with `_`, as `[_EXTENSION]`,
/// `[_SCRIPTING_PHYSICS]`) in a content file: kept as read, its behaviour is later work.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CspSection {
    pub file: String,
    pub name: String,
    pub keys: Vec<(String, String)>,
}

/// What CSP data a car or a track carries, found when it loads.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CspFeatures {
    /// car.ini `VERSION=extended-N` (`false`) / `extendedV2-N` (`true`) and N.
    pub extended: Option<(bool, i32)>,
    /// The `[SURFACE_n]` sections whose `WAV_PITCH` is `extended-N`, with the value read.
    pub wav_pitch: Vec<(String, f32)>,
    /// CSP's own sections, file by file.
    pub sections: Vec<CspSection>,
}

impl CspFeatures {
    /// Is there any CSP data?
    pub fn any(&self) -> bool {
        self.extended.is_some() || !self.wav_pitch.is_empty() || !self.sections.is_empty()
    }

    /// The CSP sections of one ini file.
    pub fn add_sections(&mut self, file: &str, ini: &IniReader) {
        for name in ini.sections().filter(|name| name.starts_with('_')) {
            let keys = ini.keys(name).into_iter().map(|key| (key.to_string(), ini.get_string(name, key))).collect();
            self.sections.push(CspSection { file: file.to_string(), name: name.to_string(), keys });
        }
    }

    /// A car's data folder (or archive): its `VERSION` read under the current mode (an error
    /// is the game's refusal) and the CSP sections of every ini file.
    pub fn for_car(data_path: &Path) -> Result<CspFeatures, String> {
        let car_ini = IniReader::load(&data_path.join("car.ini"))?;
        let mut features = CspFeatures::default();
        let csp = enabled();
        let (version, extended) = car_version(&car_ini, csp).map_err(|error| {
            if car_ini.get_string("HEADER", "VERSION").starts_with("extended") {
                format!("{error} (this car is made for Custom Shaders Patch: run with --csp)")
            } else {
                error
            }
        })?;
        features.extended = extended.map(|v2| (v2, version));
        let mut names = rustyac_content::acd::names(data_path)?;
        names.sort();
        for name in names.iter().filter(|name| name.to_ascii_lowercase().ends_with(".ini")) {
            features.add_sections(name, &IniReader::load(&data_path.join(name))?);
        }
        Ok(features)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ints_as_csp_reads_them() {
        assert_eq!(parse_int("2", 0), 2);
        assert_eq!(parse_int("-3x", 0), -3);
        assert_eq!(parse_int("+2", 7), 7);
        assert_eq!(parse_int("", 7), 7);
        assert_eq!(parse_int("0x1F", 7), 31);
        assert_eq!(parse_int("0x", 7), 0);
        assert_eq!(parse_int("4294967298", 0) as i32, 2);
    }

    #[test]
    fn wav_pitch_values_match_csp() {
        // re/scratch/taskC3/wav/odd.ini through CSP's getFloat in csp_oracle --surfaces
        let cases = [
            ("extended-0", 0x0000_0000),
            ("extended-1.5", 0x3fc0_0000),
            ("extended-+2e1", 0x41a0_0000),
            ("extended-", 0x0000_0000),
            ("extended--0.25E-1", 0xbccc_cccd),
            ("extended-.", 0x0000_0000),
            ("extended-7x", 0x40e0_0000),
            ("extended-0.1e400", 0x7f80_0000),
            ("extended-123456789.123456789", 0x4ceb_79a3),
        ];
        for (text, bits) in cases {
            assert_eq!(wav_pitch(text).map(f32::to_bits), Some(bits), "{text}");
        }
        assert_eq!(wav_pitch("0.75"), None);
    }

    #[test]
    fn car_version_vanilla_and_csp() {
        let ini = |value: &str| IniReader::from_text(Path::new("car.ini"), &format!("[HEADER]\nVERSION={value}\n"));
        assert!(car_version(&ini("extended-2"), false).is_err());
        assert_eq!(car_version(&ini("extended-2"), true), Ok((2, Some(false))));
        assert_eq!(car_version(&ini("extendedV2-3"), true), Ok((3, Some(true))));
        assert_eq!(car_version(&ini("Extended-5"), true), Ok((2, None)));
        assert_eq!(car_version(&ini(""), true), Ok((2, None)));
        assert_eq!(car_version(&ini("2"), false), Ok((2, None)));
        assert!(car_version(&ini("2x"), true).is_ok());
        let none = IniReader::from_text(Path::new("car.ini"), "[HEADER]\n");
        assert_eq!(car_version(&none, false), Ok((1, None)));
    }
}
