// SPDX-License-Identifier: MIT OR Apache-2.0

//! The gateway itself: a listener, and per WebSocket three threads (WebSocket to server,
//! the server's TCP to WebSocket, the server's UDP to WebSocket). Everything blocks on its
//! own socket, so nothing waits for anything else and nothing is polled.

use std::io::{ErrorKind, Read, Write};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream, ToSocketAddrs, UdpSocket};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::ws::{self, Decoder, Frame, Head};

/// The largest message of the server's TCP stream (Assetto Corsa's own limit).
pub const MAX_TCP_MESSAGE: usize = 0x7fe;
/// The protocol's first message of a client (the join request) and its goodbye.
const MESSAGE_JOIN: u8 = 0x3d;
const MESSAGE_GOODBYE: u8 = 0x43;
/// How long a read waits before it looks whether the connection is over.
const TICK: Duration = Duration::from_millis(250);

/// One gateway: one listening port for one game server.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Config {
    /// A short name for the log lines ("spa").
    pub name: String,
    /// Where WebSockets are accepted, `address:port`.
    pub listen: String,
    /// The game server's TCP port, `host:port`.
    pub server: String,
    /// The game server's UDP port (`None`: the same number as the TCP port, as every
    /// `acServer` of the kit has it).
    pub udp_port: Option<u16>,
    /// The `Origin` values a browser may come from: exact (`https://rustyac.example.com`),
    /// any port of a host (`http://127.0.0.1:*`) or `*` for all. A request without an
    /// `Origin` field is no browser's (a desktop client) and is let in.
    pub origins: Vec<String>,
    /// How many clients at once.
    pub max_clients: usize,
    /// The largest WebSocket message taken from a client, bytes.
    pub max_message: usize,
    /// A client that sends nothing (not even the answer to a ping) for this long is dropped.
    pub idle_timeout_s: u64,
    /// How often the gateway pings a client (keeps tunnels from closing a quiet connection).
    pub ping_interval_s: u64,
    /// No log lines.
    pub quiet: bool,
}

impl Default for Config {
    fn default() -> Config {
        Config {
            name: "gateway".to_string(),
            listen: "127.0.0.1:9601".to_string(),
            server: "127.0.0.1:9600".to_string(),
            udp_port: None,
            origins: vec!["http://127.0.0.1:*".to_string(), "http://localhost:*".to_string()],
            max_clients: 24,
            max_message: 4096,
            idle_timeout_s: 300,
            ping_interval_s: 20,
            quiet: false,
        }
    }
}

/// Whether a browser from `origin` is let in.
pub fn origin_allowed(allowed: &[String], origin: &str) -> bool {
    allowed.iter().any(|pattern| {
        if pattern == "*" {
            return true;
        }
        match pattern.strip_suffix(":*") {
            // any port of that host, or none
            Some(host) => origin.eq_ignore_ascii_case(host) || origin.len() > host.len() + 1 && origin[..host.len()].eq_ignore_ascii_case(host) && origin.as_bytes()[host.len()] == b':' && origin[host.len() + 1..].bytes().all(|b| b.is_ascii_digit()),
            None => origin.eq_ignore_ascii_case(pattern.trim_end_matches('/')),
        }
    })
}

struct Shared {
    config: Config,
    clients: AtomicUsize,
    served: AtomicU64,
    stop: AtomicBool,
}

impl Shared {
    fn log(&self, text: &str) {
        if !self.config.quiet {
            let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
            println!("{now} [{}] {text}", self.config.name);
        }
    }
}

/// A running gateway.
pub struct Gateway {
    /// Where it listens (the port the system gave when the configuration said 0).
    pub address: SocketAddr,
    shared: Arc<Shared>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Gateway {
    /// Binds the port and accepts in a thread of its own.
    pub fn start(config: Config) -> Result<Gateway, String> {
        let listener = TcpListener::bind(&config.listen).map_err(|e| format!("listening on {}: {e}", config.listen))?;
        let address = listener.local_addr().map_err(|e| e.to_string())?;
        let shared = Arc::new(Shared { config, clients: AtomicUsize::new(0), served: AtomicU64::new(0), stop: AtomicBool::new(false) });
        shared.log(&format!(
            "listening on {address} for {} (UDP port {}), at most {} clients, origins {:?}",
            shared.config.server,
            shared.config.udp_port.map_or("the same".to_string(), |port| port.to_string()),
            shared.config.max_clients,
            shared.config.origins
        ));
        let accepting = Arc::clone(&shared);
        let thread = std::thread::spawn(move || {
            for stream in listener.incoming() {
                if accepting.stop.load(Ordering::SeqCst) {
                    break;
                }
                let Ok(stream) = stream else {
                    continue;
                };
                let shared = Arc::clone(&accepting);
                // (a thread that cannot be started: the connection is dropped)
                let _ = std::thread::Builder::new().name("gateway-client".to_string()).spawn(move || serve(stream, &shared));
            }
        });
        Ok(Gateway { address, shared, thread: Some(thread) })
    }

    /// Clients connected now.
    pub fn clients(&self) -> usize {
        self.shared.clients.load(Ordering::SeqCst)
    }

    /// Waits for the listener (for ever, unless [`Gateway::stop`] is called elsewhere).
    pub fn wait(mut self) {
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }

    /// Stops accepting. Connections that are open go on until they end.
    pub fn stop(&mut self) {
        self.shared.stop.store(true, Ordering::SeqCst);
        // (wakes the listener)
        let mut wake = self.address;
        if wake.ip().is_unspecified() {
            wake.set_ip(std::net::Ipv4Addr::LOCALHOST.into());
        }
        let _ = TcpStream::connect_timeout(&wake, Duration::from_millis(500));
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for Gateway {
    fn drop(&mut self) {
        if self.thread.is_some() {
            self.stop();
        }
    }
}

fn respond(stream: &mut TcpStream, status: &str, extra: &str, body: &str) {
    let text = format!("HTTP/1.1 {status}\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n{extra}\r\n{body}", body.len());
    let _ = stream.write_all(text.as_bytes());
    let _ = stream.flush();
}

/// One connection: an HTTP request that is either the health check or a WebSocket.
fn serve(mut stream: TcpStream, shared: &Arc<Shared>) {
    let config = &shared.config;
    let _ = stream.set_nodelay(true);
    let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(10)));
    let Ok((head, rest)) = ws::read_head(&mut stream, 0x4000) else {
        return;
    };
    let head = Head::parse(&head);
    let mut request = head.first_line.split(' ');
    let (method, target) = (request.next().unwrap_or(""), request.next().unwrap_or(""));
    let path = target.split('?').next().unwrap_or("");
    if method != "GET" {
        return respond(&mut stream, "405 Method Not Allowed", "Allow: GET\r\n", "GET only\n");
    }
    if path == "/healthz" || path.ends_with("/healthz") {
        return respond(&mut stream, "200 OK", "", "ok\n");
    }
    if !head.has_token("connection", "upgrade") || !head.has_token("upgrade", "websocket") {
        return respond(&mut stream, "426 Upgrade Required", "Upgrade: websocket\r\nConnection: Upgrade\r\n", "this is rustyAC's WebSocket gateway: connect with a WebSocket\n");
    }
    let Some(key) = head.field("sec-websocket-key").filter(|_| head.field("sec-websocket-version") == Some("13")) else {
        return respond(&mut stream, "426 Upgrade Required", "Sec-WebSocket-Version: 13\r\n", "WebSocket version 13 is expected\n");
    };
    if let Some(origin) = head.field("origin") {
        if !origin_allowed(&config.origins, origin) {
            shared.log(&format!("refused: a browser from the origin {:?}, which is not allowed", origin.chars().take(100).collect::<String>()));
            return respond(&mut stream, "403 Forbidden", "", "this origin is not allowed on this gateway\n");
        }
    }
    // the client's place: taken now, given back however this ends
    struct Place<'a>(&'a AtomicUsize);
    impl Drop for Place<'_> {
        fn drop(&mut self) {
            self.0.fetch_sub(1, Ordering::SeqCst);
        }
    }
    let count = shared.clients.fetch_add(1, Ordering::SeqCst) + 1;
    let _place = Place(&shared.clients);
    if count > config.max_clients {
        shared.log("refused: the gateway is full");
        return respond(&mut stream, "503 Service Unavailable", "", "the gateway is full\n");
    }
    let number = shared.served.fetch_add(1, Ordering::SeqCst) + 1;
    let peer = stream.peer_addr().map_or("?".to_string(), |address| address.ip().to_string());
    // the game server: one TCP connection and one UDP socket for this client alone
    let connect = || -> Result<(TcpStream, UdpSocket), String> {
        let address = config.server.to_socket_addrs().map_err(|e| e.to_string())?.find(SocketAddr::is_ipv4).ok_or("no IPv4 address")?;
        let tcp = TcpStream::connect_timeout(&address, Duration::from_secs(5)).map_err(|e| e.to_string())?;
        tcp.set_nodelay(true).map_err(|e| e.to_string())?;
        let local = tcp.local_addr().map_err(|e| e.to_string())?;
        let udp = UdpSocket::bind(SocketAddr::new(local.ip(), 0)).map_err(|e| e.to_string())?;
        udp.connect(SocketAddr::new(address.ip(), config.udp_port.unwrap_or(address.port()))).map_err(|e| e.to_string())?;
        Ok((tcp, udp))
    };
    let (mut tcp, udp) = match connect() {
        Ok(sockets) => sockets,
        Err(why) => {
            shared.log(&format!("#{number} refused: the game server {} is not reachable: {why}", config.server));
            return respond(&mut stream, "502 Bad Gateway", "", "the game server is not reachable from the gateway\n");
        }
    };
    let accept = format!("HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {}\r\n\r\n", ws::accept_key(key));
    if stream.write_all(accept.as_bytes()).and_then(|()| stream.flush()).is_err() {
        return;
    }
    shared.log(&format!("#{number} connected from {peer}{} ({count} of {} places taken)", if head.field("origin").is_some() { " (a browser)" } else { "" }, config.max_clients));

    let started = Instant::now();
    let done = Arc::new(AtomicBool::new(false));
    let counts = Arc::new([AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0)]);
    let (Ok(ws_out), Ok(tcp_in), Ok(udp_in), Ok(ws_end), Ok(tcp_end)) = (stream.try_clone(), tcp.try_clone(), udp.try_clone(), stream.try_clone(), tcp.try_clone()) else {
        return;
    };
    let writer = Arc::new(Mutex::new(ws_out));
    let send = |writer: &Mutex<TcpStream>, opcode: u8, parts: &[&[u8]]| -> bool {
        let payload = parts.concat();
        let frame = ws::encode_frame(opcode, &payload, None);
        let mut out = writer.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        out.write_all(&frame).and_then(|()| out.flush()).is_ok()
    };
    // whatever ends first ends all three threads: the sockets are shut, which wakes the
    // reads where the system does that (Linux); elsewhere (Windows) every read has a short
    // timeout and looks at `done`
    let end = {
        let (done, sockets) = (Arc::clone(&done), Arc::new((ws_end, tcp_end)));
        move || {
            done.store(true, Ordering::SeqCst);
            let _ = sockets.0.shutdown(Shutdown::Both);
            let _ = sockets.1.shutdown(Shutdown::Both);
        }
    };

    // the server's TCP stream to the WebSocket: one message per frame
    let from_tcp = {
        let (writer, done, counts, end) = (Arc::clone(&writer), Arc::clone(&done), Arc::clone(&counts), end.clone());
        let mut tcp_in = tcp_in;
        std::thread::spawn(move || {
            let _ = tcp_in.set_read_timeout(Some(TICK));
            let mut buffer = [0u8; 0x1000];
            let mut pending: Vec<u8> = Vec::new();
            'read: loop {
                match tcp_in.read(&mut buffer) {
                    Ok(0) => break,
                    Ok(count) => pending.extend_from_slice(&buffer[..count]),
                    Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut | ErrorKind::Interrupted) => {
                        if done.load(Ordering::SeqCst) {
                            break;
                        }
                        continue;
                    }
                    Err(_) => break,
                }
                while pending.len() >= 2 {
                    let length = u16::from_le_bytes([pending[0], pending[1]]) as usize;
                    if pending.len() < 2 + length {
                        break;
                    }
                    if !send(&writer, ws::OP_BINARY, &[&[ws::LANE_RELIABLE], &pending[2..2 + length]]) {
                        break 'read;
                    }
                    counts[2].fetch_add(1, Ordering::Relaxed);
                    pending.drain(..2 + length);
                }
            }
            if !done.load(Ordering::SeqCst) {
                let _ = send(&writer, ws::OP_CLOSE, &[&ws::close_payload(ws::CLOSE_GOING_AWAY, "the game server closed the connection")]);
            }
            end();
        })
    };

    // the server's datagrams to the WebSocket, and the pings that keep a quiet tunnel open
    let from_udp = {
        let (writer, done, counts, end) = (Arc::clone(&writer), Arc::clone(&done), Arc::clone(&counts), end.clone());
        let ping_every = Duration::from_secs(config.ping_interval_s.max(1));
        std::thread::spawn(move || {
            let _ = udp_in.set_read_timeout(Some(TICK));
            let mut buffer = vec![0u8; 0x10000];
            let mut last_ping = Instant::now();
            while !done.load(Ordering::SeqCst) {
                match udp_in.recv(&mut buffer) {
                    Ok(count) => {
                        if !send(&writer, ws::OP_BINARY, &[&[ws::LANE_DATAGRAM], &buffer[..count]]) {
                            break;
                        }
                        counts[3].fetch_add(1, Ordering::Relaxed);
                    }
                    // nothing for a moment; Windows reports an ICMP "port unreachable" of an
                    // earlier send as a reset
                    Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut | ErrorKind::ConnectionReset | ErrorKind::Interrupted) => {}
                    Err(_) => break,
                }
                if last_ping.elapsed() >= ping_every {
                    last_ping = Instant::now();
                    if !send(&writer, ws::OP_PING, &[]) {
                        break;
                    }
                }
            }
            end();
        })
    };

    // the WebSocket to the server
    let _ = stream.set_read_timeout(Some(TICK));
    let idle_timeout = Duration::from_secs(config.idle_timeout_s.max(1));
    let mut last_heard = Instant::now();
    let mut decoder = Decoder::new(true, config.max_message);
    decoder.push(&rest);
    let (mut joined, mut said_goodbye) = (false, false);
    let mut buffer = [0u8; 0x1000];
    let why = 'session: loop {
        loop {
            let frame = match decoder.pop() {
                Ok(Some(frame)) => frame,
                Ok(None) => break,
                Err(why) => {
                    let code = if why.contains("larger") { ws::CLOSE_TOO_BIG } else { ws::CLOSE_PROTOCOL };
                    let _ = send(&writer, ws::OP_CLOSE, &[&ws::close_payload(code, &why)]);
                    break 'session why;
                }
            };
            match frame {
                Frame::Binary(message) => match message.split_first() {
                    Some((&ws::LANE_RELIABLE, body)) if body.len() <= MAX_TCP_MESSAGE => {
                        joined |= body.first() == Some(&MESSAGE_JOIN);
                        said_goodbye |= body.first() == Some(&MESSAGE_GOODBYE);
                        let mut framed = (body.len() as u16).to_le_bytes().to_vec();
                        framed.extend_from_slice(body);
                        if tcp.write_all(&framed).is_err() {
                            break 'session "the game server closed the connection".to_string();
                        }
                        counts[0].fetch_add(1, Ordering::Relaxed);
                    }
                    Some((&ws::LANE_DATAGRAM, body)) => {
                        // (a datagram that cannot be sent is lost, as UDP may lose it anyway)
                        let _ = udp.send(body);
                        counts[1].fetch_add(1, Ordering::Relaxed);
                    }
                    _ => {
                        let why = "a message without a known lane byte, or longer than the game's protocol allows".to_string();
                        let _ = send(&writer, ws::OP_CLOSE, &[&ws::close_payload(ws::CLOSE_PROTOCOL, &why)]);
                        break 'session why;
                    }
                },
                Frame::Ping(payload) => {
                    let _ = send(&writer, ws::OP_PONG, &[&payload]);
                }
                Frame::Pong(_) => {}
                Frame::Text(_) => {
                    let why = "a text message (only binary messages are carried)".to_string();
                    let _ = send(&writer, ws::OP_CLOSE, &[&ws::close_payload(1003, &why)]);
                    break 'session why;
                }
                Frame::Close(..) => {
                    let _ = send(&writer, ws::OP_CLOSE, &[&ws::close_payload(ws::CLOSE_NORMAL, "")]);
                    break 'session "the client closed the WebSocket".to_string();
                }
            }
        }
        match stream.read(&mut buffer) {
            Ok(0) => break "the client's connection ended".to_string(),
            Ok(count) => {
                last_heard = Instant::now();
                decoder.push(&buffer[..count]);
            }
            Err(_) if done.load(Ordering::SeqCst) => break "the game server closed the connection".to_string(),
            Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut | ErrorKind::Interrupted) => {
                if last_heard.elapsed() >= idle_timeout {
                    let why = format!("nothing from the client for {} s", config.idle_timeout_s);
                    let _ = send(&writer, ws::OP_CLOSE, &[&ws::close_payload(ws::CLOSE_GOING_AWAY, &why)]);
                    break why;
                }
            }
            Err(e) => break format!("the client's connection broke: {e}"),
        }
    };
    // a client that joined and went without a word (a closed tab): the gateway says the
    // protocol's goodbye for it, so that the server frees the slot and tells the others
    let mut spoke_for_it = false;
    if joined && !said_goodbye && !done.load(Ordering::SeqCst) {
        spoke_for_it = tcp.write_all(&[1, 0, MESSAGE_GOODBYE]).and_then(|()| tcp.flush()).is_ok();
        // (the server reads it before the connection closes)
        let _ = tcp.shutdown(Shutdown::Write);
        std::thread::sleep(Duration::from_millis(100));
    }
    end();
    let _ = from_tcp.join();
    let _ = from_udp.join();
    shared.log(&format!(
        "#{number} left after {} s: {why}{}; client to server {} messages + {} datagrams, server to client {} + {}",
        started.elapsed().as_secs(),
        if spoke_for_it { " (the gateway said goodbye to the server for it)" } else { "" },
        counts[0].load(Ordering::Relaxed),
        counts[1].load(Ordering::Relaxed),
        counts[2].load(Ordering::Relaxed),
        counts[3].load(Ordering::Relaxed)
    ));
}

/// Asks a gateway's health check (`GET /healthz`). `Ok`: it answered `200`.
pub fn check(address: &str) -> Result<(), String> {
    let address = address.replace("0.0.0.0", "127.0.0.1");
    let target = address.to_socket_addrs().map_err(|e| format!("{address}: {e}"))?.next().ok_or(format!("{address}: no address"))?;
    let mut stream = TcpStream::connect_timeout(&target, Duration::from_secs(3)).map_err(|e| format!("{address}: {e}"))?;
    stream.set_read_timeout(Some(Duration::from_secs(3))).map_err(|e| e.to_string())?;
    stream.write_all(format!("GET /healthz HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\n\r\n").as_bytes()).map_err(|e| e.to_string())?;
    let (head, _) = ws::read_head(&mut stream, 0x1000)?;
    if head.starts_with("HTTP/1.1 200") {
        Ok(())
    } else {
        Err(format!("{address} answered {:?}", head.lines().next().unwrap_or("")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn origins() {
        let allowed = vec!["https://rustyac.example.com".to_string(), "http://127.0.0.1:*".to_string()];
        assert!(origin_allowed(&allowed, "https://rustyac.example.com"));
        assert!(origin_allowed(&allowed, "HTTPS://RUSTYAC.example.com"));
        assert!(origin_allowed(&allowed, "http://127.0.0.1:8080"));
        assert!(origin_allowed(&allowed, "http://127.0.0.1"));
        assert!(!origin_allowed(&allowed, "http://127.0.0.1.evil.example:8080"));
        assert!(!origin_allowed(&allowed, "http://127.0.0.1:80a"));
        assert!(!origin_allowed(&allowed, "https://rustyac.example.com.evil.example"));
        assert!(!origin_allowed(&allowed, "null"));
        assert!(origin_allowed(&["*".to_string()], "https://anything.example"));
        assert!(!origin_allowed(&[], "https://anything.example"));
    }
}
