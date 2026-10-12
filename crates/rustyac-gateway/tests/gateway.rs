// SPDX-License-Identifier: MIT OR Apache-2.0

//! The gateway between a WebSocket client and a stand-in game server (a TCP listener and a
//! UDP socket on 127.0.0.1, on ports the system picks).

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream, UdpSocket};
use std::time::Duration;

use rustyac_gateway::gateway::{check, Config, Gateway};
use rustyac_gateway::ws::{self, Client, LANE_DATAGRAM, LANE_RELIABLE};

/// The stand-in server: TCP and UDP on the same port number, as an `acServer` has them.
fn server() -> (TcpListener, UdpSocket) {
    for _ in 0..50 {
        let tcp = TcpListener::bind("127.0.0.1:0").unwrap();
        if let Ok(udp) = UdpSocket::bind(tcp.local_addr().unwrap()) {
            udp.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
            return (tcp, udp);
        }
    }
    panic!("no port free for both TCP and UDP");
}

fn gateway(server: &TcpListener, change: impl FnOnce(&mut Config)) -> Gateway {
    let mut config = Config { listen: "127.0.0.1:0".to_string(), server: server.local_addr().unwrap().to_string(), quiet: true, ..Config::default() };
    change(&mut config);
    Gateway::start(config).unwrap()
}

fn connect(gateway: &Gateway, fields: &[(String, String)]) -> Result<Client<TcpStream>, String> {
    let stream = TcpStream::connect(gateway.address).unwrap();
    Client::connect(stream, &gateway.address.to_string(), "/net/test", fields, 5000)
}

fn read_framed(stream: &mut TcpStream) -> Vec<u8> {
    stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    let mut length = [0u8; 2];
    stream.read_exact(&mut length).unwrap();
    let mut message = vec![0u8; u16::from_le_bytes(length) as usize];
    stream.read_exact(&mut message).unwrap();
    message
}

fn lane(lane: u8, message: &[u8]) -> Vec<u8> {
    [&[lane], message].concat()
}

#[test]
fn both_lanes_both_ways() {
    let (tcp, udp) = server();
    let gateway = gateway(&tcp, |_| {});
    let mut client = connect(&gateway, &[]).unwrap();
    let (mut stream, _) = tcp.accept().unwrap();
    assert_eq!(gateway.clients(), 1);

    // the reliable lane: the gateway adds the length going in and takes it off coming out
    client.send(&lane(LANE_RELIABLE, &[0x3d, 1, 2, 3])).unwrap();
    client.send(&lane(LANE_RELIABLE, &[])).unwrap();
    let long: Vec<u8> = (0..0x7fe).map(|i| i as u8).collect();
    client.send(&lane(LANE_RELIABLE, &long)).unwrap();
    assert_eq!(read_framed(&mut stream), [0x3d, 1, 2, 3]);
    assert_eq!(read_framed(&mut stream), [] as [u8; 0]);
    assert_eq!(read_framed(&mut stream), long);
    // two messages in one write and one cut in two writes: whole messages come out
    stream.write_all(&[2, 0, 0x3e, 9, 3, 0, 0x40]).unwrap();
    stream.flush().unwrap();
    std::thread::sleep(Duration::from_millis(50));
    stream.write_all(&[7, 8]).unwrap();
    assert_eq!(client.wait(5000).unwrap(), Some(lane(LANE_RELIABLE, &[0x3e, 9])));
    assert_eq!(client.wait(5000).unwrap(), Some(lane(LANE_RELIABLE, &[0x40, 7, 8])));

    // the datagram lane: from a UDP socket of this client's own
    client.send(&lane(LANE_DATAGRAM, &[0x4e, 5])).unwrap();
    let mut buffer = [0u8; 2048];
    let (count, from) = udp.recv_from(&mut buffer).unwrap();
    assert_eq!(&buffer[..count], &[0x4e, 5]);
    for number in 0..20u8 {
        udp.send_to(&[0x46, number], from).unwrap();
    }
    for number in 0..20u8 {
        assert_eq!(client.wait(5000).unwrap(), Some(lane(LANE_DATAGRAM, &[0x46, number])));
    }
    assert_eq!(client.poll().unwrap(), None);

    // the client says goodbye itself: the gateway adds nothing
    client.send(&lane(LANE_RELIABLE, &[0x43])).unwrap();
    client.close();
    drop(client);
    assert_eq!(read_framed(&mut stream), [0x43]);
    let mut rest = Vec::new();
    stream.read_to_end(&mut rest).unwrap();
    assert!(rest.is_empty(), "the gateway sent {rest:02x?} after the client's own goodbye");
    for _ in 0..100 {
        if gateway.clients() == 0 {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(gateway.clients(), 0);
}

#[test]
fn a_client_that_vanishes_is_said_goodbye_for() {
    let (tcp, _udp) = server();
    let gateway = gateway(&tcp, |_| {});
    // one that joined and vanished (a closed tab)
    let mut client = connect(&gateway, &[]).unwrap();
    let (mut stream, _) = tcp.accept().unwrap();
    client.send(&lane(LANE_RELIABLE, &[0x3d, 0xca, 0])).unwrap();
    assert_eq!(read_framed(&mut stream), [0x3d, 0xca, 0]);
    // (the server lets it in)
    stream.write_all(&[3, 0, 0x3e, 1, 2]).unwrap();
    assert_eq!(client.wait(5000).unwrap(), Some(lane(LANE_RELIABLE, &[0x3e, 1, 2])));
    drop(client.stream().try_clone().unwrap().shutdown(std::net::Shutdown::Both));
    assert_eq!(read_framed(&mut stream), [0x43], "the gateway did not say goodbye for a client that vanished");
    // one the server refused, and one that never asked: nothing to say
    let mut client = connect(&gateway, &[]).unwrap();
    let (mut stream, _) = tcp.accept().unwrap();
    client.send(&lane(LANE_RELIABLE, &[0x3d, 0xca, 0])).unwrap();
    assert_eq!(read_framed(&mut stream), [0x3d, 0xca, 0]);
    stream.write_all(&[1, 0, 0x45]).unwrap();
    assert_eq!(client.wait(5000).unwrap(), Some(lane(LANE_RELIABLE, &[0x45])));
    drop(client);
    let mut rest = Vec::new();
    stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    stream.read_to_end(&mut rest).unwrap();
    assert!(rest.is_empty(), "the gateway said {rest:02x?} for a client the server had refused");
    let client = connect(&gateway, &[]).unwrap();
    let (mut stream, _) = tcp.accept().unwrap();
    drop(client);
    let mut rest = Vec::new();
    stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    stream.read_to_end(&mut rest).unwrap();
    assert!(rest.is_empty());
}

#[test]
fn the_server_closing_closes_the_websocket() {
    let (tcp, _udp) = server();
    let gateway = gateway(&tcp, |_| {});
    let mut client = connect(&gateway, &[]).unwrap();
    let (stream, _) = tcp.accept().unwrap();
    drop(stream);
    let why = client.wait(5000).unwrap_err();
    assert!(why.contains("game server closed"), "{why}");
}

#[test]
fn refusals_and_the_health_check() {
    let (tcp, _udp) = server();
    let gateway = gateway(&tcp, |config| {
        config.max_clients = 1;
        config.max_message = 64;
        config.origins = vec!["https://rustyac.example.com".to_string()];
    });
    check(&gateway.address.to_string()).unwrap();
    // a browser from elsewhere
    let field = |origin: &str| vec![("Origin".to_string(), origin.to_string())];
    let why = connect(&gateway, &field("https://evil.example")).err().unwrap();
    assert!(why.contains("403") && why.contains("origin"), "{why}");
    // plain HTTP on the WebSocket's path
    let mut plain = TcpStream::connect(gateway.address).unwrap();
    plain.write_all(b"GET /net/test HTTP/1.1\r\nHost: x\r\n\r\n").unwrap();
    let (head, _) = ws::read_head(&mut plain, 4096).unwrap();
    assert!(head.starts_with("HTTP/1.1 426"), "{head}");
    // the allowed origin gets in; the place is the only one
    let mut first = connect(&gateway, &field("https://rustyac.example.com")).unwrap();
    let (mut stream, _) = tcp.accept().unwrap();
    let why = connect(&gateway, &[]).err().unwrap();
    assert!(why.contains("503") && why.contains("full"), "{why}");
    // a lane that does not exist, a message too long for the protocol, one too long for the gateway
    first.send(&[9, 1, 2]).unwrap();
    assert!(first.wait(5000).unwrap_err().contains("lane"));
    drop(first);
    let mut rest = Vec::new();
    stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    stream.read_to_end(&mut rest).unwrap();
    assert!(rest.is_empty());
    for _ in 0..100 {
        if gateway.clients() == 0 {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let mut second = connect(&gateway, &[]).unwrap();
    let _accepted = tcp.accept().unwrap();
    second.send(&[LANE_DATAGRAM; 65]).unwrap();
    assert!(second.wait(5000).unwrap_err().contains("larger than 64"));
}

#[test]
fn a_server_that_is_not_there() {
    let (tcp, _udp) = server();
    let gateway = gateway(&tcp, |_| {});
    drop(tcp);
    let why = connect(&gateway, &[]).err().unwrap();
    assert!(why.contains("502") && why.contains("not reachable"), "{why}");
    assert_eq!(gateway.clients(), 0);
}
