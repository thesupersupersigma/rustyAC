// SPDX-License-Identifier: GPL-3.0-or-later

//! Replays a golden file of an online session (`tools/car_oracle net ... --csv-out`: the
//! local car's inputs, the tape of a live session and, per step, the hash of what the game's
//! own `ACClient` and `NetCarStateProvider` made of it) and says whether every step is the
//! game's. Task 26's proofs run it three ways:
//!
//! ```text
//! net_golden <file.netgold> [--cardata <folder>] [--root <Assetto Corsa folder>]
//!     (default)      the client on a tape player, the join waited for: the desktop's way
//!     --browser      the client on the WebSocket transport over a pipe that plays the tape,
//!                    the join polled: the browser's way. Built for wasm32-wasip1 and run
//!                    under wasmtime, this is the wasm client.
//!     --gateway      (desktop only) the client on the WebSocket transport through a real
//!                    rustyac-gateway on 127.0.0.1, which talks TCP and UDP to a stand-in
//!                    server that plays the tape; what that server received is held against
//!                    what the client sent.
//! ```
//!
//! Prints one line of numbers that must be the same in all three ways and on both machines
//! (`steps`, `digest` over every step's hash, `sent` messages and their digest), and
//! `IDENTICAL` or the first step that is not the game's. Exit code 1 on a difference.

use std::path::PathBuf;

use rustyac_net::client::{Client, Join, JoinStage};
use rustyac_net::handshake::CarEntry;
use rustyac_net::replay::{replay_request, NetGolden, ReplayReport};
use rustyac_net::transport::{Tape, TapePlayer};
use rustyac_net::ws::{TapePipe, WsTransport};
use rustyac_physics::engine::record::ReplayPaths;

/// The join as a page steps it: polled, never waited for.
fn polled<P: rustyac_net::ws::Pipe>(pipe: P) -> Result<(Client<WsTransport<P>>, Vec<CarEntry>), String> {
    let mut join = Join::start(WsTransport::new(pipe), &replay_request()).map_err(|e| e.to_string())?;
    for _ in 0..100_000 {
        if join.poll().map_err(|e| e.to_string())? == JoinStage::Done {
            return join.finish().ok_or("the join did not end".to_string());
        }
        #[cfg(not(target_arch = "wasm32"))]
        std::thread::sleep(std::time::Duration::from_micros(200));
    }
    Err("the polled join did not end".to_string())
}

#[cfg(not(target_arch = "wasm32"))]
mod through_gateway {
    //! A stand-in server that plays a tape over real sockets, in step with the client: at
    //! every physics step the messages the tape has for that step are sent (TCP and UDP),
    //! and the step goes on when they have all come out of the WebSocket.

    use std::collections::VecDeque;
    use std::io::{Read, Write};
    use std::net::{SocketAddr, TcpListener, TcpStream, UdpSocket};
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    use rustyac_gateway::gateway::{Config, Gateway};
    use rustyac_net::native::{connect_ws, WsPipe, WsUrl};
    use rustyac_net::transport::{Lane, Tape, TapeEntry};
    use rustyac_net::ws::{Pipe, WsTransport, LANE_DATAGRAM, LANE_RELIABLE};

    /// What the stand-in server received, in order.
    #[derive(Default)]
    pub struct Received {
        pub reliable: Vec<Vec<u8>>,
        pub datagrams: Vec<Vec<u8>>,
        /// Where the gateway's datagrams for this client come from.
        peer: Option<SocketAddr>,
    }

    pub struct GatewayPipe {
        inner: WsPipe<TcpStream>,
        _gateway: Gateway,
        server_tcp: TcpStream,
        server_udp: UdpSocket,
        pub received: Arc<Mutex<Received>>,
        reliable: VecDeque<TapeEntry>,
        datagrams: VecDeque<TapeEntry>,
        /// Sent by the stand-in server and not out of the WebSocket yet.
        owed: usize,
        arrived: VecDeque<Vec<u8>>,
        pub sent: Tape,
        tick: u64,
    }

    impl GatewayPipe {
        pub fn new(tape: &Tape) -> Result<GatewayPipe, String> {
            let e = |e: std::io::Error| e.to_string();
            // TCP and UDP on one port number, as an acServer has them
            let (listener, server_udp) = (0..50)
                .find_map(|_| {
                    let tcp = TcpListener::bind("127.0.0.1:0").ok()?;
                    let udp = UdpSocket::bind(tcp.local_addr().ok()?).ok()?;
                    Some((tcp, udp))
                })
                .ok_or("no port free for both TCP and UDP on 127.0.0.1")?;
            let gateway = Gateway::start(Config { name: "proof".to_string(), listen: "127.0.0.1:0".to_string(), server: listener.local_addr().map_err(e)?.to_string(), quiet: true, ..Config::default() })?;
            let url = WsUrl::parse(&format!("ws://{}/net/proof", gateway.address))?;
            let transport: WsTransport<WsPipe<TcpStream>> = connect_ws(&url, &[], 5000)?;
            let (server_tcp, _) = listener.accept().map_err(e)?;
            server_tcp.set_nodelay(true).map_err(e)?;
            let received = Arc::new(Mutex::new(Received::default()));
            // the stand-in server's two readers
            let (mut tcp_in, udp_in) = (server_tcp.try_clone().map_err(e)?, server_udp.try_clone().map_err(e)?);
            let log = Arc::clone(&received);
            std::thread::spawn(move || {
                let mut pending: Vec<u8> = Vec::new();
                let mut buffer = [0u8; 4096];
                while let Ok(count) = tcp_in.read(&mut buffer) {
                    if count == 0 {
                        break;
                    }
                    pending.extend_from_slice(&buffer[..count]);
                    while pending.len() >= 2 && pending.len() >= 2 + u16::from_le_bytes([pending[0], pending[1]]) as usize {
                        let length = u16::from_le_bytes([pending[0], pending[1]]) as usize;
                        log.lock().unwrap().reliable.push(pending[2..2 + length].to_vec());
                        pending.drain(..2 + length);
                    }
                }
            });
            let log = Arc::clone(&received);
            std::thread::spawn(move || {
                let mut buffer = vec![0u8; 0x10000];
                while let Ok((count, from)) = udp_in.recv_from(&mut buffer) {
                    let mut log = log.lock().unwrap();
                    log.peer = Some(from);
                    log.datagrams.push(buffer[..count].to_vec());
                }
            });
            Ok(GatewayPipe {
                inner: transport.pipe,
                _gateway: gateway,
                server_tcp,
                server_udp,
                received,
                reliable: tape.of(Lane::ReliableIn).cloned().collect(),
                datagrams: tape.of(Lane::DatagramIn).cloned().collect(),
                owed: 0,
                arrived: VecDeque::new(),
                sent: Tape::default(),
                tick: 0,
            })
        }

        fn serve_reliable(&mut self, message: &[u8]) -> Result<(), String> {
            let mut framed = (message.len() as u16).to_le_bytes().to_vec();
            framed.extend_from_slice(message);
            self.server_tcp.write_all(&framed).map_err(|e| e.to_string())?;
            self.owed += 1;
            Ok(())
        }

        fn serve_datagram(&mut self, message: &[u8]) -> Result<(), String> {
            // the gateway's UDP address for this client is known from its first datagram
            // (the client's association, which a server answers before it sends anything)
            let deadline = Instant::now() + Duration::from_secs(5);
            let peer = loop {
                if let Some(peer) = self.received.lock().unwrap().peer {
                    break peer;
                }
                if Instant::now() > deadline {
                    return Err("the tape has a datagram for a client that never sent one".to_string());
                }
                std::thread::sleep(Duration::from_micros(100));
            };
            self.server_udp.send_to(message, peer).map_err(|e| e.to_string())?;
            self.owed += 1;
            Ok(())
        }

        /// Waits until everything the stand-in server sent has come out of the WebSocket.
        fn collect(&mut self) -> Result<(), String> {
            while self.owed > 0 {
                match self.inner.wait(5000)? {
                    Some(message) => {
                        self.arrived.push_back(message);
                        self.owed -= 1;
                    }
                    None => return Err(format!("{} messages the stand-in server sent did not come out of the gateway within 5 s (step {})", self.owed, self.tick)),
                }
            }
            Ok(())
        }
    }

    impl Pipe for GatewayPipe {
        fn send(&mut self, message: &[u8]) -> Result<(), String> {
            let lane = match message.first() {
                Some(&LANE_RELIABLE) => Lane::ReliableOut,
                Some(&LANE_DATAGRAM) => Lane::DatagramOut,
                _ => return Err("a message without a lane byte".to_string()),
            };
            self.sent.entries.push(TapeEntry { tick: self.tick, lane, data: message[1..].to_vec() });
            self.inner.send(message)
        }

        fn poll(&mut self) -> Result<Option<Vec<u8>>, String> {
            Ok(self.arrived.pop_front())
        }

        /// (the join: the next answer of the tape, whatever its step, as on a tape player)
        fn wait(&mut self, _timeout_ms: u32) -> Result<Option<Vec<u8>>, String> {
            if self.arrived.is_empty() {
                if let Some(entry) = self.reliable.pop_front() {
                    self.serve_reliable(&entry.data)?;
                    self.collect()?;
                }
            }
            Ok(self.arrived.pop_front())
        }

        /// A new step: what the tape has up to it is sent now and awaited.
        fn set_tick(&mut self, tick: u64) {
            self.tick = tick;
            let mut failed = None;
            while self.reliable.front().is_some_and(|entry| entry.tick <= tick) {
                let entry = self.reliable.pop_front().unwrap();
                failed = failed.or(self.serve_reliable(&entry.data).err());
            }
            while self.datagrams.front().is_some_and(|entry| entry.tick <= tick) {
                let entry = self.datagrams.pop_front().unwrap();
                failed = failed.or(self.serve_datagram(&entry.data).err());
            }
            failed = failed.or(self.collect().err());
            if let Some(why) = failed {
                eprintln!("the gateway proof: {why}");
                std::process::exit(1);
            }
        }

        fn tape(&self) -> Option<&Tape> {
            Some(&self.sent)
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let value = |name: &str| args.iter().position(|a| a == name).and_then(|at| args.get(at + 1)).cloned();
    let Some(path) = args.first().filter(|a| !a.starts_with("--")) else {
        eprintln!("net_golden <file.netgold> [--cardata <folder>] [--root <Assetto Corsa folder>] [--browser | --gateway]");
        std::process::exit(2);
    };
    let fail = |why: String| -> ! {
        eprintln!("{why}");
        std::process::exit(1);
    };
    let golden = std::fs::read(path).map_err(|e| format!("{path}: {e}")).and_then(|bytes| NetGolden::from_bytes(&bytes)).unwrap_or_else(|why| fail(why));
    let cardata = PathBuf::from(value("--cardata").unwrap_or_else(|| "cardata".to_string()));
    let game_root = value("--root").map(PathBuf::from).or_else(rustyac_content::install::ac_root);
    let paths = ReplayPaths { cardata, game_root };
    let (way, report): (&str, Result<ReplayReport, String>) = if args.iter().any(|a| a == "--browser") {
        ("the browser's way (WebSocket transport, polled join)", golden.replay_on(&paths, |tape: &Tape| polled(TapePipe::new(tape))))
    } else if args.iter().any(|a| a == "--gateway") {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let mut received = None;
            let report = golden.replay_on(&paths, |tape: &Tape| {
                let pipe = through_gateway::GatewayPipe::new(tape)?;
                received = Some(std::sync::Arc::clone(&pipe.received));
                // (through the gateway the join is the desktop's: waited for)
                Client::join(WsTransport::new(pipe), &replay_request()).map_err(|e| e.to_string())
            });
            // what the stand-in server got through the gateway is what the client sent
            let report = report.and_then(|report| {
                let sent = report.transport_tape.clone().ok_or("no record of what was sent")?;
                let received = received.ok_or("no stand-in server")?;
                let wanted = |lane| sent.of(lane).map(|entry| entry.data.clone()).collect::<Vec<_>>();
                let (reliable, datagrams) = (wanted(rustyac_net::transport::Lane::ReliableOut), wanted(rustyac_net::transport::Lane::DatagramOut));
                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
                loop {
                    let got = received.lock().unwrap();
                    if got.reliable.len() >= reliable.len() && got.datagrams.len() >= datagrams.len() || std::time::Instant::now() > deadline {
                        // (the replay's client vanishes at the end without the protocol's
                        // goodbye: the gateway says it for it, as for a closed browser tab)
                        let (same, extra) = got.reliable.split_at(reliable.len().min(got.reliable.len()));
                        if same != reliable.as_slice() || !(extra.is_empty() || extra == [vec![0x43u8]]) {
                            return Err(format!("the stand-in server got {} TCP messages, the client sent {}: not the same", got.reliable.len(), reliable.len()));
                        }
                        if !extra.is_empty() {
                            println!("through the gateway: after the client vanished the gateway said the protocol's goodbye (0x43) to the server for it");
                        }
                        if got.datagrams != datagrams {
                            return Err(format!("the stand-in server got {} datagrams, the client sent {}: not the same", got.datagrams.len(), datagrams.len()));
                        }
                        println!("through the gateway: the stand-in server received {} TCP messages and {} datagrams, byte for byte what the client sent, in order", reliable.len(), got.datagrams.len());
                        return Ok(report);
                    }
                    drop(got);
                    std::thread::sleep(std::time::Duration::from_millis(1));
                }
            });
            ("through a real gateway (WebSocket, TCP and UDP on 127.0.0.1)", report)
        }
        #[cfg(target_arch = "wasm32")]
        fail("--gateway needs sockets: not in a wasm build".to_string())
    } else {
        ("the desktop's way (tape player, waited join)", golden.replay_on(&paths, |tape: &Tape| Client::join(TapePlayer::new(tape), &replay_request()).map_err(|e| e.to_string())))
    };
    let report = report.unwrap_or_else(|why| fail(why));
    println!(
        "steps {} remote_cars {} values {} join_messages {} sent {} sent_compared {} digest {:016x} sent_digest {:016x}",
        report.steps, report.remote_cars, report.values, report.join_messages, report.sent, report.sent_compared, report.digest, report.sent_digest
    );
    match report.first_difference {
        None => println!("IDENTICAL to the game at every one of {} steps: {way}", report.steps),
        Some(step) => {
            println!("DIFFERENT from the game at step {step}: {way}");
            std::process::exit(1);
        }
    }
}
