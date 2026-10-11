// SPDX-License-Identifier: GPL-3.0-or-later

//! Prints a network tape (`rustyac.exe --net-tape <file>`): every message with the step
//! it crossed in, its channel and direction, and what it says.
//!
//! ```text
//! cargo run --release -p rustyac-net --example net_tape -- <file.nettape> [--cars n] [--all] [--hex]
//! ```
//!
//! Without `--all` the car updates and pings (the bulk of a tape) are only counted.

use rustyac_net::datagram::{self, Incoming};
use rustyac_net::handshake::{decode_car_list, HandshakeReply};
use rustyac_net::messages;
use rustyac_net::transport::{Lane, Tape};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(path) = args.first().filter(|a| !a.starts_with("--")) else {
        eprintln!("net_tape <file.nettape> [--cars n] [--all] [--hex]");
        std::process::exit(2);
    };
    let all = args.iter().any(|a| a == "--all");
    let hex = args.iter().any(|a| a == "--hex");
    let mut cars: usize = args.iter().position(|a| a == "--cars").and_then(|at| args.get(at + 1)).and_then(|n| n.parse().ok()).unwrap_or(0);
    let bytes = std::fs::read(path).unwrap_or_else(|e| {
        eprintln!("{path}: {e}");
        std::process::exit(1);
    });
    let tape = Tape::from_bytes(&bytes).unwrap_or_else(|e| {
        eprintln!("{path}: {e}");
        std::process::exit(1);
    });
    let mut counts = std::collections::BTreeMap::<(u8, u8), (u64, u64)>::new();
    let mut listed = Vec::new();
    for entry in &tape.entries {
        let id = entry.data.first().copied().unwrap_or(0);
        let slot = counts.entry((entry.lane as u8, id)).or_default();
        slot.0 += 1;
        slot.1 += entry.data.len() as u64;
        let lane = match entry.lane {
            Lane::ReliableOut => "TCP out",
            Lane::ReliableIn => "TCP in ",
            Lane::DatagramOut => "UDP out",
            Lane::DatagramIn => "UDP in ",
        };
        let text = match entry.lane {
            Lane::ReliableIn if id == 0x3e => match HandshakeReply::decode(&entry.data) {
                HandshakeReply::Accepted(a) => format!("accepted: slot {}, {} at {}, {} Hz, session {:?}", a.session_id, a.model, a.track, a.send_hz, a.current_session.name),
                HandshakeReply::Refused(r) => format!("refused: {}", r.message()),
            },
            Lane::ReliableIn if id == 0x40 => match decode_car_list(&entry.data, entry.data.get(1).copied().unwrap_or(0)) {
                Ok(list) => {
                    listed.extend(list.iter().map(|car| (car.session_id, car.is_spectator)));
                    format!("car list from {}: {} cars", entry.data.get(1).copied().unwrap_or(0), list.len())
                }
                Err(e) => e,
            },
            Lane::ReliableIn => {
                if cars == 0 {
                    // what the client makes of the list: every slot that is no spectator's
                    cars = listed.iter().filter(|(_, spectator)| !spectator).count().max(1);
                }
                format!("{:?}", messages::decode(&entry.data, cars))
            }
            Lane::DatagramIn => {
                let decoded = datagram::decode(&entry.data);
                if !all && decoded.iter().all(|d| matches!(d, Incoming::CarState { .. } | Incoming::Ping { .. })) {
                    continue;
                }
                format!("{decoded:?}")
            }
            Lane::DatagramOut if matches!(id, 0x46 | 0xf8) && !all => continue,
            _ => format!("id {id:#04x}, {} bytes", entry.data.len()),
        };
        let text: String = text.chars().take(if all { 4000 } else { 300 }).collect();
        println!("{:>8} {lane} {text}", entry.tick);
        if hex {
            println!("         {:02x?}", &entry.data[..entry.data.len().min(96)]);
        }
    }
    println!("--- {} messages", tape.entries.len());
    for ((lane, id), (count, bytes)) in counts {
        let lane = ["TCP out", "TCP in ", "UDP out", "UDP in "][lane as usize];
        println!("  {lane} id {id:#04x}: {count} messages, {bytes} bytes");
    }
}
