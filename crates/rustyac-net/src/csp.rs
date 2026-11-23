// SPDX-License-Identifier: GPL-3.0-or-later

//! Custom Shaders Patch 0.3.2-preview2 (build 4305) online: what CSP adds to Kunos'
//! protocol, as `docs/csp/assettoserver.md` §NH-2.1 recorded it from real AC + CSP joining
//! AssettoServer. Only `--csp` uses it; without it the protocol stays Kunos' own.
//!
//! * the handshake's tail ([`FEATURES`]), the server's `csp/<build>/../<track>` name
//!   ([`split_track`]) and the `CSPFACE` checksum rule ([`surfaces_fix`]);
//! * the welcome text's `$CSP0:` settings ([`split_welcome`], [`parse_ini`]);
//! * the `0xAB` envelope ([`decode_tcp`]) and the handshake reply ([`handshake_out`]).

use crate::packet::{Reader, Writer};

/// The CSP build rustyAC's `--csp` stands for.
pub const BUILD: u32 = 4305;

/// CSP 4305's `Features` field of the join request (0x3d), as it sent it: a fixed list
/// (not the server's), the build last. rustyAC claims the same; what it does not handle yet
/// arrives as `0xAB` messages that are logged and skipped.
pub const FEATURES: &str = "STEAM_TICKET,CLIENT_MESSAGES,SPECTATING_AWARE,WEATHERFX_V1,WEATHERFX_V2,WEATHERFX_V3,BATCH_FIX_V2,CUSTOM_UPDATE,CUSTOM_UPDATE_V2,CLIENT_UDP_MESSAGES,BATCH_MESSAGES,EXPLICIT_ADMIN_STATE,4305";

/// The id of CSP's messages, on both channels.
pub const EXTENDED: u8 = 0xab;

/// A track name with CSP's options: `csp/<build>/../[<flags>/../]<track>`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CspTrack<'a> {
    /// The lowest CSP build the server lets in.
    pub build: u32,
    /// The letter's offset from 'A': 1 custom car physics, 2 custom track physics, 4 no pit crew.
    pub flags: u8,
    pub track: &'a str,
}

/// The parts of a server's track name, if it has CSP's prefix.
pub fn split_track(name: &str) -> Option<CspTrack<'_>> {
    let (digits, rest) = name.strip_prefix("csp/")?.split_once("/../")?;
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let build = digits.parse().ok()?;
    let (flags, track) = match rest.split_once("/../") {
        Some((flags, track)) if !flags.is_empty() && flags.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') => (flags.as_bytes()[0].wrapping_sub(b'A'), track),
        _ => (0, rest),
    };
    (!track.is_empty()).then_some(CspTrack { build, flags, track })
}

/// A track's `data/surfaces.ini` as a CSP client hashes it: the first `SURFACE_0` reads
/// `CSPFACE_0` (so Kunos' AC finds no surfaces). Not at the file's very first byte
/// (AssettoServer's rule, which CSP has to meet).
pub fn surfaces_fix(mut bytes: Vec<u8>) -> Vec<u8> {
    if let Some(at) = bytes.windows(9).position(|w| w == b"SURFACE_0") {
        if at > 0 {
            bytes[at..at + 3].copy_from_slice(b"CSP");
        }
    }
    bytes
}

/// Whether a checksum path (after [`split_track`]'s prefix is taken out) gets
/// [`surfaces_fix`]: a track's `data/surfaces.ini`, never `system/data/surfaces.ini`.
pub fn is_track_surfaces(path: &str) -> bool {
    path.starts_with("content/tracks/") && path.ends_with("/data/surfaces.ini")
}

/// The welcome text (0x51) split into what is shown and the server's CSP settings: tabs,
/// `$CSP0:`, base64 without padding of the zlib-compressed INI text. `None`: no settings
/// (or settings that do not decode: the whole text is shown).
pub fn split_welcome(text: &str) -> Option<(String, String)> {
    let marker = text.find("$CSP0:")?;
    let shown = text[..marker].trim_end_matches('\t');
    if shown.len() == marker {
        return None;
    }
    let encoded = text[marker + 6..].split(char::is_whitespace).next().unwrap_or("");
    let packed = base64(encoded)?;
    let ini = miniz_oxide::inflate::decompress_to_vec_zlib(&packed).ok()?;
    Some((shown.to_string(), String::from_utf8_lossy(&ini).into_owned()))
}

fn base64(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len() * 3 / 4);
    let (mut acc, mut bits) = (0u32, 0);
    for c in text.bytes().take_while(|&c| c != b'=') {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        };
        acc = (acc << 6 | v as u32) & 0xffff;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    Some(out)
}

/// One section of the server's settings and its keys, in the text's order.
pub type IniSection = (String, Vec<(String, String)>);

/// The server's settings: a repeated section adds its keys to the first one (a repeated
/// key: the later value), `'...'` values may run over several lines.
/// shortcut: no INIpp (no includes, variables, expressions or comments after a value), enough
/// for what AssettoServer writes; the full reader comes with CSP's own INI work.
pub fn parse_ini(text: &str) -> Vec<IniSection> {
    let mut out: Vec<IniSection> = Vec::new();
    let mut current = None;
    let mut lines = text.lines();
    while let Some(line) = lines.next() {
        let line = line.trim();
        if line.starts_with(';') || line.starts_with('#') {
            continue;
        }
        if let (true, Some(end)) = (line.starts_with('['), line.find(']')) {
            let name = &line[1..end];
            current = Some(out.iter().position(|(known, _)| known == name).unwrap_or_else(|| {
                out.push((name.to_string(), Vec::new()));
                out.len() - 1
            }));
            continue;
        }
        let (Some(section), Some((key, value))) = (current, line.split_once('=')) else {
            continue;
        };
        let mut value = value.trim().to_string();
        if value.starts_with('\'') {
            while value.len() < 2 || !value.ends_with('\'') {
                let Some(more) = lines.next() else {
                    break;
                };
                value.push('\n');
                value.push_str(more);
            }
            value = value.trim_start_matches('\'').trim_end_matches('\'').to_string();
        }
        let keys = &mut out[section].1;
        let key = key.trim().to_string();
        match keys.iter_mut().find(|(known, _)| *known == key) {
            Some(known) => known.1 = value,
            None => keys.push((key, value)),
        }
    }
    out
}

/// Client-message types (the `u16` after `AB 03`).
pub mod message_type {
    pub const HANDSHAKE_IN: u16 = 0;
    pub const HANDSHAKE_OUT: u16 = 1;
}

/// A CSP message of the reliable channel, taken apart as far as rustyAC uses it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CspMessage {
    /// `AB 03 FF 0000`: the server's welcome to CSP; [`handshake_out`] answers it.
    HandshakeIn { min_version: u32, weatherfx: bool },
    /// `AB 02`: show or hide a car (AssettoServer's traffic before it spawns).
    Visibility { slot: u8, hidden: bool },
    /// `AB 05`: the reason of a kick.
    KickReason(String),
    /// Any other client message: whose (255: the server's), its type and the size of its data.
    ClientMessage { from: u8, kind: u16, size: usize },
    /// Any other sub-id and its size.
    Other { sub: u8, size: usize },
}

/// A message of the reliable channel that starts with [`EXTENDED`].
pub fn decode_tcp(message: &[u8]) -> CspMessage {
    let mut r = Reader::new(message);
    r.u8();
    let sub = r.u8();
    let other = CspMessage::Other { sub, size: message.len() };
    let decoded = match sub {
        2 => CspMessage::Visibility { slot: r.u8(), hidden: r.u8() != 0 },
        3 => {
            let (from, kind) = (r.u8(), r.u16());
            match kind {
                message_type::HANDSHAKE_IN => CspMessage::HandshakeIn { min_version: r.u32(), weatherfx: r.u8() != 0 },
                _ => CspMessage::ClientMessage { from, kind, size: r.remaining() },
            }
        }
        5 => {
            let count = r.u32() as usize;
            CspMessage::KickReason(String::from_utf8_lossy(&r.bytes(count.min(r.remaining()))).into_owned())
        }
        _ => other.clone(),
    };
    if r.overrun {
        other
    } else {
        decoded
    }
}

/// CSP's answer to [`CspMessage::HandshakeIn`]: `AB 03`, type 1 (a client's message has no
/// sender byte), the build, WeatherFX on, input 3, RainFX on, a zero byte, then the 8-byte
/// key AssettoServer logs as the player's "HWID".
pub fn handshake_out(key: u64) -> Vec<u8> {
    let mut w = Writer::with_id(EXTENDED);
    w.u8(3).u16(message_type::HANDSHAKE_OUT).u32(BUILD).u8(1).u8(3).u8(1).u8(0).u64(key);
    w.finish()
}

/// rustyAC's own 8-byte key for a player id: the same for the same player, nobody's hardware.
pub fn handshake_key(guid: &str) -> u64 {
    let digest = crate::md5::digest(format!("rustyAC CSP key {guid}").as_bytes());
    u64::from_le_bytes([digest[0], digest[1], digest[2], digest[3], digest[4], digest[5], digest[6], digest[7]])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn track_names() {
        assert_eq!(split_track("csp/2651/../spa"), Some(CspTrack { build: 2651, flags: 0, track: "spa" }));
        assert_eq!(split_track("csp/3000/../C/../ks_nordschleife"), Some(CspTrack { build: 3000, flags: 2, track: "ks_nordschleife" }));
        assert_eq!(split_track("spa"), None);
        assert_eq!(split_track("csp/x/../spa"), None);
        assert_eq!(split_track("csp/2651/../"), None);
    }

    #[test]
    fn surfaces() {
        assert_eq!(surfaces_fix(b"[SURFACE_0]\nKEY=SURFACE_0".to_vec()), b"[CSPFACE_0]\nKEY=SURFACE_0".to_vec());
        assert_eq!(surfaces_fix(b"SURFACE_0".to_vec()), b"SURFACE_0".to_vec());
        assert!(is_track_surfaces("content/tracks/spa/data/surfaces.ini"));
        assert!(!is_track_surfaces("system/data/surfaces.ini"));
    }

    #[test]
    fn welcome_and_settings() {
        // "[A]\r\nX = 1\r\n[B]\r\nF = 'a\r\n  b'\r\n[A]\r\nY = 2", zlib-packed, base64 with its padding cut
        let text = "Hello\t\t\t$CSP0:eNqLdozl5YpQsFUw5OWKdgKy3YBs9UReLgWFJHWgEEg6EihkBACkCAeZ";
        let (shown, ini) = split_welcome(text).unwrap();
        assert_eq!(shown, "Hello");
        let ini = parse_ini(&ini);
        assert_eq!(ini.len(), 2);
        assert_eq!(ini[0], ("A".to_string(), vec![("X".to_string(), "1".to_string()), ("Y".to_string(), "2".to_string())]));
        assert_eq!(ini[1].1[0].1, "a\n  b");
        assert_eq!(split_welcome("no settings"), None);
    }

    #[test]
    fn messages() {
        // the capture's HandshakeIn and the reply's layout
        assert_eq!(decode_tcp(&[0xab, 3, 0xff, 0, 0, 0x5b, 0x0a, 0, 0, 1]), CspMessage::HandshakeIn { min_version: 2651, weatherfx: true });
        assert_eq!(decode_tcp(&[0xab, 2, 3, 1]), CspMessage::Visibility { slot: 3, hidden: true });
        assert_eq!(decode_tcp(&[0xab, 2]), CspMessage::Other { sub: 2, size: 2 });
        let out = handshake_out(0x0807_0605_0403_0201);
        assert_eq!(out, [0xab, 3, 1, 0, 0xd1, 0x10, 0, 0, 1, 3, 1, 0, 1, 2, 3, 4, 5, 6, 7, 8]);
    }
}
