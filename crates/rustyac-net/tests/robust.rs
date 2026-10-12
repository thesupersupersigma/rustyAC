// SPDX-License-Identifier: GPL-3.0-or-later

//! Task 25: whatever a server sends, the decoders give a value, never a panic: every message
//! id with every short length, with bytes that make every count as large as it can be, and
//! with a simple pattern.

use rustyac_net::handshake::{decode_car_list, HandshakeReply};
use rustyac_net::transport::Tape;
use rustyac_net::{datagram, messages};

#[test]
fn cut_and_wild_messages_never_panic() {
    let mut decoded = 0usize;
    for id in 0..=255u8 {
        for length in 0..96usize {
            for fill in [0x00u8, 0x01, 0x0a, 0x7f, 0xff] {
                let mut message = vec![fill; length + 1];
                message[0] = id;
                for (k, byte) in message.iter_mut().enumerate().skip(1) {
                    if fill == 0x7f {
                        *byte = (k as u8).wrapping_mul(37) ^ id;
                    }
                }
                for cars in [0usize, 1, 12, 255] {
                    decoded += messages::decode(&message, cars).len();
                }
                let _ = HandshakeReply::decode(&message);
                let _ = decode_car_list(&message, fill);
                decoded += datagram::decode(&message).len();
                let _ = Tape::from_bytes(&message);
                let _ = rustyac_net::handshake::without_secrets(&message);
            }
        }
    }
    assert!(decoded > 0);
    // empty input
    assert!(messages::decode(&[], 3).is_empty());
    assert!(datagram::decode(&[]).is_empty());
    let _ = HandshakeReply::decode(&[]);
    let _ = decode_car_list(&[], 0);
    // a tape whose entry claims four gigabytes
    let mut tape = b"RYNTAPE1".to_vec();
    tape.extend_from_slice(&1u32.to_le_bytes());
    tape.extend_from_slice(&0u64.to_le_bytes());
    tape.push(1);
    tape.extend_from_slice(&0xffff_fff0u32.to_le_bytes());
    assert!(Tape::from_bytes(&tape).is_err());
}
