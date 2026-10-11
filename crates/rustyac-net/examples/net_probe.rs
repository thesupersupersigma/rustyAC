// SPDX-License-Identifier: GPL-3.0-or-later

//! Joins a server as far as the handshake and the car list, prints what it says, and
//! leaves again:
//!
//! ```text
//! cargo run --release -p rustyac-net --example net_probe -- 127.0.0.1:9600 --guid <id> [--name n] [--car c] [--password p]
//! ```

use rustyac_net::handshake::{car_list_request, decode_car_list, HandshakeReply, JoinRequest};
use rustyac_net::native::NativeTransport;
use rustyac_net::transport::Transport;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let value = |name: &str| args.iter().position(|a| a == name).and_then(|at| args.get(at + 1)).cloned();
    let Some(address) = args.first().filter(|a| !a.starts_with("--")) else {
        eprintln!("net_probe <host:port> --guid <id> [--name n] [--car c] [--password p]");
        std::process::exit(2);
    };
    let (host, port) = address.rsplit_once(':').expect("host:port");
    let request = JoinRequest {
        guid: value("--guid").expect("--guid <id>: this tool invents no player id"),
        name: value("--name").unwrap_or_else(|| "rustyAC probe".to_string()),
        team: String::new(),
        nation_code: String::new(),
        car: value("--car").unwrap_or_default(),
        password: value("--password").unwrap_or_default(),
    };
    let mut transport = NativeTransport::connect(host, port.parse().expect("port"), 5000).unwrap_or_else(|e| {
        eprintln!("{e}");
        std::process::exit(1);
    });
    transport.send_reliable(&request.encode()).unwrap();
    let reply = transport.wait_reliable(5000).unwrap().unwrap_or_default();
    println!("reply: {} bytes, id {:#04x}", reply.len(), reply.first().copied().unwrap_or(0));
    let accepted = match HandshakeReply::decode(&reply) {
        HandshakeReply::Accepted(accepted) => accepted,
        HandshakeReply::Refused(refusal) => {
            println!("refused: {}", refusal.message());
            std::process::exit(1);
        }
    };
    println!("{accepted:#?}");
    let mut index = 0u8;
    loop {
        transport.send_reliable(&car_list_request(index)).unwrap();
        let page = transport.wait_reliable(5000).unwrap().unwrap_or_default();
        let cars = decode_car_list(&page, index).unwrap();
        for car in &cars {
            println!("slot {:2}: {} / {} driver {:?} spectator {} damage {:?}", car.session_id, car.model, car.skin, car.driver_name, car.is_spectator, car.damage_zone_level);
        }
        if cars.len() != 10 {
            break;
        }
        index = index.wrapping_add(10);
    }
    // anything the server sent on its own meanwhile
    while let Ok(Some(message)) = transport.wait_reliable(500) {
        println!("also: {:02x?}", &message[..message.len().min(48)]);
    }
    transport.send_reliable(&[0x43]).unwrap();
    println!("left");
}
