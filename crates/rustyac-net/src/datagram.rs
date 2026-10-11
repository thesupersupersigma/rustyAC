// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! The datagrams a server sends (`ACClient::onMessage` @ 0x14004b9d0, and the ping that
//! `UDPSocket::receive` @ 0x14025d400 answers itself) and the ones a client sends that
//! are not the car update.
//!
//! | id | way | what |
//! |---|---|---|
//! | 0x46 | in | one car's state (60 bytes after the id; several may follow each other) |
//! | 0x48 | in | "mega packet": `u32` server time, `u16` ping, `u8` count, then that many states of 57 bytes (no performance delta, no gas) |
//! | 0x4e | both | association: the client sends `4e <slot>` until the server echoes a bare `4e` |
//! | 0x4f | out | session check: `4f <session type>` |
//! | 0x57 | in | the race's start time: `i32` start, `u32` the server's idea of this client's clock, `u16` ping |
//! | 0xf9 | in | ping: bytes 1..4 to echo, `u16` the client's ping as the server measured it |
//! | 0xf8 | out | pong: the ping with id 0xf8 and the client's clock (`u32` ms) at offset 5, two bytes longer than the ping |

use rustyac_math::powf;

use crate::net_car::NetCarState;
use crate::packet::Reader;

pub mod id {
    pub const CAR_UPDATE: u8 = 0x46;
    pub const MEGA_PACKET: u8 = 0x48;
    pub const ASSOCIATION: u8 = 0x4e;
    pub const SESSION_CHECK: u8 = 0x4f;
    pub const RACE_START: u8 = 0x57;
    pub const PONG: u8 = 0xf8;
    pub const PING: u8 = 0xf9;
}

/// The size of one car's state in a mega packet, and in message 0x46 (after the id).
const MEGA_RECORD: usize = 57;
const SINGLE_RECORD: usize = 60;

/// What a datagram from the server says.
#[derive(Clone, Debug, PartialEq)]
pub enum Incoming {
    /// The state of the car in a slot (`rcv_time` is filled in by whoever takes it).
    CarState { slot: u8, state: NetCarState },
    /// The server knows this client's UDP address now.
    Associated,
    /// `ACP_RACE_START_UPDATE`
    RaceStart { start: i32, real: u32, ping: u16 },
    /// The server's ping, with the datagram to answer.
    Ping { ping: u16, datagram: Vec<u8> },
    /// A datagram the game would stop at ("Unknown packet id").
    Unknown(u8),
    /// A datagram that ends inside a record (the game reads on into whatever follows,
    /// or stops with an error; here it is dropped).
    Truncated(u8),
}

/// One wheel's speed from its byte: `sign * (10^(|b - 100| / 20) - 1)` rad/s.
pub fn decode_wheel_speed(byte: u8) -> f32 {
    let x = byte as i32 as f32 - 100.0;
    let magnitude = f32::from_bits(x.to_bits() & 0x7fff_ffff);
    let sign = if x > 0.0 {
        1.0f32
    } else if x < 0.0 {
        -1.0
    } else {
        0.0
    };
    (powf(10.0, magnitude * f32::from_bits(0x3d4c_cccd)) - 1.0) * sign
}

/// One car's state, as both 0x46 and 0x48 carry it; `single`: with the two fields only
/// 0x46 has.
fn decode_state(r: &mut Reader<'_>, single: bool) -> (u8, NetCarState) {
    let slot = r.u8();
    let mut state = NetCarState { pak_sequence_id: r.u8(), time_stamp: r.u32() as u64 as i64 as f64, ping: r.u16() as i32, pos: r.vec3(), rotation: r.vec3(), velocity: r.vec3(), ..NetCarState::default() };
    for wheel in 0..4 {
        state.tyre_angular_speed[wheel] = decode_wheel_speed(r.u8());
    }
    state.steer_angle = r.u8() as i32 as f32 - 127.0;
    state.wheel_angle = (r.u8() as i32 as f32 - 127.0) * 0.5;
    state.engine_rpm = r.u16();
    state.gear_index = r.u8() as i32;
    state.status_bytes = r.u32();
    if single {
        state.performance_delta = (r.i16() as i32 as f32) * f32::from_bits(0x3a83_126f);
        let gas = (r.u8() as i32 as f32) * f32::from_bits(0x3b80_8081);
        state.gas = if gas > 1.0 {
            1.0
        } else if gas < 0.0 {
            0.0
        } else {
            gas
        };
    }
    (slot, state)
}

/// A datagram from the server, taken apart in the order the game walks it.
pub fn decode(datagram: &[u8]) -> Vec<Incoming> {
    let mut out = Vec::new();
    let Some(&first) = datagram.first() else {
        return out;
    };
    let mut r = Reader::new(datagram);
    r.u8();
    match first {
        id::PING => {
            let ping = if datagram.len() >= 7 { u16::from_le_bytes([datagram[5], datagram[6]]) } else { 0 };
            out.push(Incoming::Ping { ping, datagram: datagram.to_vec() });
        }
        id::ASSOCIATION => out.push(Incoming::Associated),
        id::CAR_UPDATE => {
            while !r.is_eof() {
                if r.remaining() < SINGLE_RECORD {
                    out.push(Incoming::Truncated(first));
                    break;
                }
                let (slot, state) = decode_state(&mut r, true);
                out.push(Incoming::CarState { slot, state });
            }
        }
        id::MEGA_PACKET => {
            while !r.is_eof() {
                if r.remaining() < 7 {
                    out.push(Incoming::Truncated(first));
                    break;
                }
                // the server's time and the ping of the header are not used by the game
                let _server_time = r.u32();
                let _ping = r.u16();
                let count = r.u8() as usize;
                if r.remaining() < count * MEGA_RECORD {
                    out.push(Incoming::Truncated(first));
                    break;
                }
                for _ in 0..count {
                    let (slot, state) = decode_state(&mut r, false);
                    out.push(Incoming::CarState { slot, state });
                }
            }
        }
        id::RACE_START => {
            while !r.is_eof() {
                if r.remaining() < 10 {
                    out.push(Incoming::Truncated(first));
                    break;
                }
                out.push(Incoming::RaceStart { start: r.i32(), real: r.u32(), ping: r.u16() });
            }
        }
        other => out.push(Incoming::Unknown(other)),
    }
    out
}

/// The answer to a ping (`UDPSocket::receive`): the ping itself with id 0xf8 and the
/// client's clock, whole milliseconds, at offset 5; two bytes longer than the ping.
pub fn pong(ping: &[u8], clock_ms: f64) -> Vec<u8> {
    let mut out = ping.to_vec();
    out.resize(ping.len() + 2, 0);
    out[0] = id::PONG;
    if out.len() >= 9 {
        out[5..9].copy_from_slice(&((clock_ms as i64) as u32).to_le_bytes());
    }
    out
}

/// `ACClient::associate` @ 0x140044640.
pub fn association(slot: u8) -> Vec<u8> {
    vec![id::ASSOCIATION, slot]
}

/// The session check of `ACClient::update` @ 0x140053b30.
pub fn session_check(session_type: u8) -> Vec<u8> {
    vec![id::SESSION_CHECK, session_type]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wheel_speed_bytes() {
        assert_eq!(decode_wheel_speed(100), 0.0);
        assert!((decode_wheel_speed(120) - 9.0).abs() < 1e-5);
        assert!((decode_wheel_speed(80) + 9.0).abs() < 1e-5);
    }

    #[test]
    fn pong_is_two_bytes_longer() {
        let ping = [0xf9, 1, 2, 3, 4, 0x34, 0x12];
        let decoded = decode(&ping);
        assert_eq!(decoded, vec![Incoming::Ping { ping: 0x1234, datagram: ping.to_vec() }]);
        let answer = pong(&ping, 65_537.9);
        assert_eq!(answer, vec![0xf8, 1, 2, 3, 4, 0x01, 0x00, 0x01, 0x00]);
    }

    #[test]
    fn mega_packet_records() {
        let mut datagram = vec![0x48, 0, 0, 0, 0, 9, 0, 2];
        for slot in [3u8, 7] {
            let mut record = vec![slot, 5];
            record.extend_from_slice(&1000u32.to_le_bytes());
            record.extend_from_slice(&40u16.to_le_bytes());
            for value in [1.0f32, 2.0, 3.0, 0.1, 0.2, 0.3, 10.0, 0.0, -1.0] {
                record.extend_from_slice(&value.to_le_bytes());
            }
            record.extend_from_slice(&[100, 100, 120, 120, 127, 129]);
            record.extend_from_slice(&7000u16.to_le_bytes());
            record.push(4);
            record.extend_from_slice(&0x20u32.to_le_bytes());
            assert_eq!(record.len(), MEGA_RECORD);
            datagram.extend(record);
        }
        let decoded = decode(&datagram);
        assert_eq!(decoded.len(), 2);
        let Incoming::CarState { slot, state } = &decoded[1] else { panic!() };
        assert_eq!(*slot, 7);
        assert_eq!((state.pak_sequence_id, state.time_stamp, state.ping), (5, 1000.0, 40));
        assert_eq!((state.steer_angle, state.wheel_angle, state.engine_rpm, state.gear_index, state.status_bytes), (0.0, 1.0, 7000, 4, 0x20));
        assert_eq!(decode(&datagram[..40]), vec![Incoming::Truncated(0x48)]);
    }
}
