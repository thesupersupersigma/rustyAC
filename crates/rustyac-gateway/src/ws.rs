// SPDX-License-Identifier: MIT OR Apache-2.0

//! The WebSocket protocol (RFC 6455) as far as the gateway and its clients need it: the
//! opening handshake of both sides, frames both ways (binary messages, ping, pong, close;
//! fragmented messages are put together), and a small client over any byte stream.
//!
//! What travels inside is the gateway's own framing: every binary message is one lane byte
//! ([`LANE_RELIABLE`] or [`LANE_DATAGRAM`]) and one whole Assetto Corsa message.

use std::io::{ErrorKind, Read, Write};
use std::time::{Duration, Instant};

use crate::sha1::{base64, sha1};

/// A message of the server's TCP stream (without its 16-bit length).
pub const LANE_RELIABLE: u8 = 0;
/// A UDP datagram.
pub const LANE_DATAGRAM: u8 = 1;

pub const OP_CONTINUATION: u8 = 0x0;
pub const OP_TEXT: u8 = 0x1;
pub const OP_BINARY: u8 = 0x2;
pub const OP_CLOSE: u8 = 0x8;
pub const OP_PING: u8 = 0x9;
pub const OP_PONG: u8 = 0xa;

/// Close codes the gateway uses.
pub const CLOSE_NORMAL: u16 = 1000;
pub const CLOSE_GOING_AWAY: u16 = 1001;
pub const CLOSE_PROTOCOL: u16 = 1002;
pub const CLOSE_TOO_BIG: u16 = 1009;
pub const CLOSE_SERVER_ERROR: u16 = 1011;

/// `Sec-WebSocket-Accept` for a `Sec-WebSocket-Key`.
pub fn accept_key(key: &str) -> String {
    base64(&sha1(format!("{}258EAFA5-E914-47DA-95CA-C5AB0DC85B11", key.trim()).as_bytes()))
}

/// One frame as it goes onto the stream. A client masks (`mask`), a server does not.
pub fn encode_frame(opcode: u8, payload: &[u8], mask: Option<[u8; 4]>) -> Vec<u8> {
    let mut out = Vec::with_capacity(payload.len() + 14);
    out.push(0x80 | opcode);
    let masked = if mask.is_some() { 0x80 } else { 0 };
    match payload.len() {
        length @ 0..=125 => out.push(masked | length as u8),
        length @ 126..=0xffff => {
            out.push(masked | 126);
            out.extend_from_slice(&(length as u16).to_be_bytes());
        }
        length => {
            out.push(masked | 127);
            out.extend_from_slice(&(length as u64).to_be_bytes());
        }
    }
    match mask {
        Some(key) => {
            out.extend_from_slice(&key);
            out.extend(payload.iter().enumerate().map(|(at, byte)| byte ^ key[at & 3]));
        }
        None => out.extend_from_slice(payload),
    }
    out
}

/// The payload of a close frame.
pub fn close_payload(code: u16, reason: &str) -> Vec<u8> {
    let mut out = code.to_be_bytes().to_vec();
    // (a control frame carries at most 125 bytes)
    let mut end = reason.len().min(123);
    while !reason.is_char_boundary(end) {
        end -= 1;
    }
    out.extend_from_slice(&reason.as_bytes()[..end]);
    out
}

/// What a peer sent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Frame {
    Binary(Vec<u8>),
    Text(Vec<u8>),
    Ping(Vec<u8>),
    Pong(Vec<u8>),
    Close(u16, String),
}

/// Takes a byte stream apart into frames; fragments are joined into whole messages.
#[derive(Clone, Debug)]
pub struct Decoder {
    buffer: Vec<u8>,
    /// A fragmented message so far: its opcode and bytes.
    partial: Option<(u8, Vec<u8>)>,
    /// The largest message taken, bytes.
    max_message: usize,
    /// A server only takes masked frames, a client only unmasked ones.
    expect_masked: bool,
}

impl Decoder {
    /// `server`: this side is the server (the peer's frames must be masked).
    pub fn new(server: bool, max_message: usize) -> Decoder {
        Decoder { buffer: Vec::new(), partial: None, max_message, expect_masked: server }
    }

    pub fn push(&mut self, bytes: &[u8]) {
        self.buffer.extend_from_slice(bytes);
    }

    /// The next whole message or control frame, when all of it is there. `Err`: the peer
    /// broke the protocol or sent more than is taken (the text says which).
    pub fn pop(&mut self) -> Result<Option<Frame>, String> {
        loop {
            let Some((fin, opcode, payload)) = self.pop_raw()? else {
                return Ok(None);
            };
            match opcode {
                OP_PING | OP_PONG | OP_CLOSE => {
                    if !fin || payload.len() > 125 {
                        return Err("a control frame that is fragmented or too long".to_string());
                    }
                    return Ok(Some(match opcode {
                        OP_PING => Frame::Ping(payload),
                        OP_PONG => Frame::Pong(payload),
                        _ => {
                            let code = if payload.len() >= 2 { u16::from_be_bytes([payload[0], payload[1]]) } else { 1005 };
                            Frame::Close(code, String::from_utf8_lossy(payload.get(2..).unwrap_or(&[])).into_owned())
                        }
                    }));
                }
                OP_TEXT | OP_BINARY => {
                    if self.partial.is_some() {
                        return Err("a new message inside a fragmented one".to_string());
                    }
                    if fin {
                        return Ok(Some(if opcode == OP_TEXT { Frame::Text(payload) } else { Frame::Binary(payload) }));
                    }
                    self.partial = Some((opcode, payload));
                }
                OP_CONTINUATION => {
                    let Some((first, mut whole)) = self.partial.take() else {
                        return Err("a continuation without a message".to_string());
                    };
                    if whole.len() + payload.len() > self.max_message {
                        return Err(format!("a message larger than {} bytes", self.max_message));
                    }
                    whole.extend_from_slice(&payload);
                    if fin {
                        return Ok(Some(if first == OP_TEXT { Frame::Text(whole) } else { Frame::Binary(whole) }));
                    }
                    self.partial = Some((first, whole));
                }
                other => return Err(format!("an unknown frame type {other:#x}")),
            }
        }
    }

    fn pop_raw(&mut self) -> Result<Option<(bool, u8, Vec<u8>)>, String> {
        let buffer = &self.buffer;
        if buffer.len() < 2 {
            return Ok(None);
        }
        if buffer[0] & 0x70 != 0 {
            return Err("a frame with reserved bits set (no extension was agreed)".to_string());
        }
        let fin = buffer[0] & 0x80 != 0;
        let opcode = buffer[0] & 0x0f;
        let masked = buffer[1] & 0x80 != 0;
        if masked != self.expect_masked {
            return Err(if self.expect_masked { "an unmasked frame from a client" } else { "a masked frame from a server" }.to_string());
        }
        let (length, mut at) = match buffer[1] & 0x7f {
            126 => {
                if buffer.len() < 4 {
                    return Ok(None);
                }
                (u16::from_be_bytes([buffer[2], buffer[3]]) as u64, 4)
            }
            127 => {
                if buffer.len() < 10 {
                    return Ok(None);
                }
                (u64::from_be_bytes(buffer[2..10].try_into().unwrap()), 10)
            }
            short => (short as u64, 2),
        };
        if length > self.max_message as u64 {
            return Err(format!("a message larger than {} bytes", self.max_message));
        }
        let length = length as usize;
        let key = if masked {
            if buffer.len() < at + 4 {
                return Ok(None);
            }
            at += 4;
            Some([buffer[at - 4], buffer[at - 3], buffer[at - 2], buffer[at - 1]])
        } else {
            None
        };
        if buffer.len() < at + length {
            return Ok(None);
        }
        let mut payload = buffer[at..at + length].to_vec();
        if let Some(key) = key {
            for (index, byte) in payload.iter_mut().enumerate() {
                *byte ^= key[index & 3];
            }
        }
        self.buffer.drain(..at + length);
        Ok(Some((fin, opcode, payload)))
    }
}

/// The head of an HTTP request or response: its first line and its header fields.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Head {
    pub first_line: String,
    /// Names in lower case.
    pub fields: Vec<(String, String)>,
}

impl Head {
    pub fn parse(text: &str) -> Head {
        let mut lines = text.split("\r\n");
        let first_line = lines.next().unwrap_or("").to_string();
        let fields = lines.filter_map(|line| line.split_once(':')).map(|(name, value)| (name.trim().to_ascii_lowercase(), value.trim().to_string())).collect();
        Head { first_line, fields }
    }

    pub fn field(&self, name: &str) -> Option<&str> {
        self.fields.iter().find(|(known, _)| known == name).map(|(_, value)| value.as_str())
    }

    /// Whether a comma-separated field names `token` (in any case).
    pub fn has_token(&self, name: &str, token: &str) -> bool {
        self.fields.iter().filter(|(known, _)| known == name).any(|(_, value)| value.split(',').any(|part| part.trim().eq_ignore_ascii_case(token)))
    }
}

/// Reads up to the empty line that ends an HTTP head (at most `max` bytes). Returns the
/// head's text and whatever was read past it.
pub fn read_head(stream: &mut impl Read, max: usize) -> Result<(String, Vec<u8>), String> {
    let mut bytes = Vec::new();
    let mut buffer = [0u8; 1024];
    loop {
        if let Some(end) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
            let rest = bytes.split_off(end + 4);
            bytes.truncate(end);
            return Ok((String::from_utf8_lossy(&bytes).into_owned(), rest));
        }
        if bytes.len() > max {
            return Err("the HTTP head is too long".to_string());
        }
        match stream.read(&mut buffer) {
            Ok(0) => return Err("the connection closed before the HTTP head ended".to_string()),
            Ok(count) => bytes.extend_from_slice(&buffer[..count]),
            Err(e) if e.kind() == ErrorKind::Interrupted => {}
            Err(e) => return Err(format!("reading the HTTP head: {e}")),
        }
    }
}

/// A byte stream a [`Client`] can run on: a TCP stream, or TLS over one.
pub trait Socket: Read + Write {
    fn set_nonblocking(&mut self, nonblocking: bool) -> std::io::Result<()>;
    fn set_read_timeout(&mut self, timeout: Option<Duration>) -> std::io::Result<()>;
}

impl Socket for std::net::TcpStream {
    fn set_nonblocking(&mut self, nonblocking: bool) -> std::io::Result<()> {
        std::net::TcpStream::set_nonblocking(self, nonblocking)
    }
    fn set_read_timeout(&mut self, timeout: Option<Duration>) -> std::io::Result<()> {
        std::net::TcpStream::set_read_timeout(self, timeout)
    }
}

/// Numbers for masks and the handshake key. A mask only has to differ from frame to frame
/// (it keeps proxies from mistaking a frame for HTTP); it hides nothing.
struct Xorshift(u64);

impl Xorshift {
    fn new() -> Xorshift {
        let time = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos() as u64);
        let place = &time as *const u64 as u64;
        Xorshift((time ^ place.rotate_left(32) ^ (std::process::id() as u64) << 20) | 1)
    }

    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
}

/// The client side of one WebSocket: whole binary messages both ways. Sends wait until
/// the bytes are out; [`Client::poll`] never waits.
pub struct Client<S: Socket> {
    stream: S,
    decoder: Decoder,
    random: Xorshift,
    nonblocking: bool,
    /// Why the connection is over, once it is.
    closed: Option<String>,
}

/// The largest message a [`Client`] takes from the gateway (a datagram and its lane byte).
pub const CLIENT_MAX_MESSAGE: usize = 0x10001;

impl<S: Socket> Client<S> {
    /// The opening handshake on a connected stream. `host`: what goes into the `Host`
    /// field; `path`: the request's path; `fields`: more header fields (name, value).
    pub fn connect(mut stream: S, host: &str, path: &str, fields: &[(String, String)], timeout_ms: u32) -> Result<Client<S>, String> {
        let mut random = Xorshift::new();
        let mut key = [0u8; 16];
        for chunk in key.chunks_mut(8) {
            chunk.copy_from_slice(&random.next().to_le_bytes());
        }
        let key = base64(&key);
        let mut request = format!("GET {path} HTTP/1.1\r\nHost: {host}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\n");
        for (name, value) in fields {
            if name.contains(['\r', '\n', ':']) || value.contains(['\r', '\n']) {
                return Err(format!("the header field {name:?} cannot be sent"));
            }
            request.push_str(&format!("{name}: {value}\r\n"));
        }
        request.push_str("\r\n");
        stream.set_read_timeout(Some(Duration::from_millis(timeout_ms as u64))).map_err(|e| e.to_string())?;
        stream.write_all(request.as_bytes()).and_then(|()| stream.flush()).map_err(|e| format!("sending the WebSocket request: {e}"))?;
        let (head, rest) = read_head(&mut stream, 0x4000)?;
        let head = Head::parse(&head);
        let status = head.first_line.split(' ').nth(1).unwrap_or("");
        if status != "101" {
            // what the gateway (or a proxy in front of it) says instead
            let mut body = String::from_utf8_lossy(&rest).trim().to_string();
            if body.is_empty() || body.starts_with('<') {
                body = match status {
                    "302" | "401" | "403" => "a login is asked for (Cloudflare Access?): see docs/port/multiplayer_web.md for a service token".to_string(),
                    "502" | "503" | "530" => "the gateway is not reachable behind this address".to_string(),
                    _ => String::new(),
                };
            }
            let line: String = body.lines().next().unwrap_or("").chars().take(200).collect();
            return Err(format!("the gateway answered {:?}{}{line}", head.first_line, if line.is_empty() { "" } else { ": " }));
        }
        if head.field("sec-websocket-accept") != Some(accept_key(&key).as_str()) {
            return Err("the answer is not a WebSocket handshake (wrong accept key)".to_string());
        }
        let mut decoder = Decoder::new(false, CLIENT_MAX_MESSAGE);
        decoder.push(&rest);
        stream.set_read_timeout(None).map_err(|e| e.to_string())?;
        Ok(Client { stream, decoder, random, nonblocking: false, closed: None })
    }

    fn mode(&mut self, nonblocking: bool) -> Result<(), String> {
        if self.nonblocking != nonblocking {
            self.stream.set_nonblocking(nonblocking).map_err(|e| e.to_string())?;
            self.nonblocking = nonblocking;
        }
        Ok(())
    }

    fn write_frame(&mut self, opcode: u8, payload: &[u8]) -> Result<(), String> {
        if let Some(why) = &self.closed {
            return Err(why.clone());
        }
        // a send must not be cut short by the non-blocking mode of the reads
        self.mode(false)?;
        let mask = (self.random.next() as u32).to_le_bytes();
        let frame = encode_frame(opcode, payload, Some(mask));
        self.stream.write_all(&frame).and_then(|()| self.stream.flush()).map_err(|e| {
            let why = format!("the connection to the gateway broke: {e}");
            self.closed = Some(why.clone());
            why
        })
    }

    /// Sends one binary message.
    pub fn send(&mut self, message: &[u8]) -> Result<(), String> {
        self.write_frame(OP_BINARY, message)
    }

    /// A message that is already whole in the buffer; answers pings on the way.
    fn pop(&mut self) -> Result<Option<Vec<u8>>, String> {
        loop {
            match self.decoder.pop() {
                Ok(Some(Frame::Binary(message))) => return Ok(Some(message)),
                Ok(Some(Frame::Ping(payload))) => self.write_frame(OP_PONG, &payload)?,
                Ok(Some(Frame::Pong(_) | Frame::Text(_))) => {}
                Ok(Some(Frame::Close(code, reason))) => {
                    let _ = self.write_frame(OP_CLOSE, &close_payload(CLOSE_NORMAL, ""));
                    let why = if reason.is_empty() { format!("the gateway closed the connection (code {code})") } else { format!("the gateway closed the connection: {reason}") };
                    self.closed = Some(why.clone());
                    return Err(why);
                }
                Ok(None) => return Ok(None),
                Err(why) => {
                    let why = format!("the gateway broke the WebSocket protocol: {why}");
                    self.closed = Some(why.clone());
                    return Err(why);
                }
            }
        }
    }

    /// Reads what the stream has (without waiting, or up to the stream's read timeout).
    /// Returns whether anything arrived.
    fn fill(&mut self) -> Result<bool, String> {
        let mut buffer = [0u8; 0x2000];
        let mut any = false;
        loop {
            match self.stream.read(&mut buffer) {
                Ok(0) => {
                    let why = "the gateway closed the connection".to_string();
                    self.closed = Some(why.clone());
                    return if any { Ok(true) } else { Err(why) };
                }
                Ok(count) => {
                    self.decoder.push(&buffer[..count]);
                    any = true;
                    if !self.nonblocking {
                        return Ok(true);
                    }
                }
                Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => return Ok(any),
                Err(e) if e.kind() == ErrorKind::Interrupted => {}
                Err(e) => {
                    let why = format!("the connection to the gateway broke: {e}");
                    self.closed = Some(why.clone());
                    return if any { Ok(true) } else { Err(why) };
                }
            }
        }
    }

    /// The next binary message, if one has arrived. Never waits.
    pub fn poll(&mut self) -> Result<Option<Vec<u8>>, String> {
        if let Some(message) = self.pop()? {
            return Ok(Some(message));
        }
        if let Some(why) = &self.closed {
            return Err(why.clone());
        }
        self.mode(true)?;
        self.fill()?;
        self.pop()
    }

    /// Waits up to `timeout_ms` for the next binary message.
    pub fn wait(&mut self, timeout_ms: u32) -> Result<Option<Vec<u8>>, String> {
        let deadline = Instant::now() + Duration::from_millis(timeout_ms as u64);
        loop {
            if let Some(message) = self.pop()? {
                return Ok(Some(message));
            }
            if let Some(why) = &self.closed {
                return Err(why.clone());
            }
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Ok(None);
            }
            self.mode(false)?;
            self.stream.set_read_timeout(Some(left)).map_err(|e| e.to_string())?;
            self.fill()?;
        }
    }

    /// Says goodbye (a close frame). The stream closes when the client is dropped.
    pub fn close(&mut self) {
        if self.closed.is_none() {
            let _ = self.write_frame(OP_CLOSE, &close_payload(CLOSE_NORMAL, ""));
            self.closed = Some("closed".to_string());
        }
    }

    pub fn stream(&self) -> &S {
        &self.stream
    }
}

/// `ws://host[:port]/path` or `wss://...` taken apart.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Url {
    pub tls: bool,
    pub host: String,
    pub port: u16,
    /// With its leading slash (and a query, if there is one).
    pub path: String,
}

impl Url {
    pub fn parse(text: &str) -> Result<Url, String> {
        let (tls, rest) = if let Some(rest) = text.strip_prefix("wss://") {
            (true, rest)
        } else if let Some(rest) = text.strip_prefix("ws://") {
            (false, rest)
        } else {
            return Err(format!("{text}: ws://<host>[:port]/<path> or wss://... is expected"));
        };
        let (authority, path) = match rest.find('/') {
            Some(at) => (&rest[..at], &rest[at..]),
            None => (rest, "/"),
        };
        let (host, port) = match authority.rsplit_once(':') {
            Some((host, port)) => (host, port.parse().map_err(|_| format!("{text}: {port:?} is no port number"))?),
            None => (authority, if tls { 443 } else { 80 }),
        };
        if host.is_empty() || host.contains(['@', ' ', '[']) || path.contains([' ', '\r', '\n']) {
            return Err(format!("{text}: not an address rustyAC can use"));
        }
        Ok(Url { tls, host: host.to_string(), port, path: path.to_string() })
    }

    /// What goes into the `Host` field.
    pub fn host_field(&self) -> String {
        if self.port == if self.tls { 443 } else { 80 } {
            self.host.clone()
        } else {
            format!("{}:{}", self.host, self.port)
        }
    }
}

/// Whether `text` is an address of a gateway (and not `host:port` of a server).
pub fn is_url(text: &str) -> bool {
    text.starts_with("ws://") || text.starts_with("wss://")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accept_key_of_the_rfc() {
        assert_eq!(accept_key("dGhlIHNhbXBsZSBub25jZQ=="), "s3pPLMBiTxaQ9kYGzzhZRbK+xOo=");
    }

    #[test]
    fn frames_round_trip() {
        for length in [0usize, 1, 125, 126, 127, 300, 65535, 65536, 70000] {
            let payload: Vec<u8> = (0..length).map(|i| (i * 7) as u8).collect();
            for mask in [None, Some([1, 2, 3, 4])] {
                let frame = encode_frame(OP_BINARY, &payload, mask);
                let mut decoder = Decoder::new(mask.is_some(), 1 << 20);
                // fed in pieces: nothing comes out before the last byte
                for chunk in frame.chunks(977) {
                    assert_eq!(decoder.pop().unwrap(), None);
                    decoder.push(chunk);
                }
                if length == 0 {
                    decoder.pop().unwrap();
                    continue;
                }
                assert_eq!(decoder.pop().unwrap(), Some(Frame::Binary(payload.clone())));
                assert_eq!(decoder.pop().unwrap(), None);
            }
        }
    }

    #[test]
    fn fragments_controls_and_faults() {
        let mut decoder = Decoder::new(false, 16);
        let mut first = encode_frame(OP_BINARY, &[1, 2], None);
        first[0] &= 0x7f;
        decoder.push(&first);
        decoder.push(&encode_frame(OP_PING, b"hi", None));
        let mut middle = encode_frame(OP_CONTINUATION, &[3], None);
        middle[0] &= 0x7f;
        decoder.push(&middle);
        decoder.push(&encode_frame(OP_CONTINUATION, &[4], None));
        decoder.push(&encode_frame(OP_CLOSE, &close_payload(1001, "bye"), None));
        assert_eq!(decoder.pop().unwrap(), Some(Frame::Ping(b"hi".to_vec())));
        assert_eq!(decoder.pop().unwrap(), Some(Frame::Binary(vec![1, 2, 3, 4])));
        assert_eq!(decoder.pop().unwrap(), Some(Frame::Close(1001, "bye".to_string())));
        // too long, the wrong masking, reserved bits, a stray continuation
        let mut decoder = Decoder::new(false, 16);
        decoder.push(&encode_frame(OP_BINARY, &[0; 17], None));
        assert!(decoder.pop().is_err());
        let mut decoder = Decoder::new(true, 16);
        decoder.push(&encode_frame(OP_BINARY, &[0; 3], None));
        assert!(decoder.pop().is_err());
        let mut decoder = Decoder::new(false, 16);
        decoder.push(&[0xc2, 0]);
        assert!(decoder.pop().is_err());
        let mut decoder = Decoder::new(false, 16);
        decoder.push(&encode_frame(OP_CONTINUATION, &[1], None));
        assert!(decoder.pop().is_err());
        // a length field that claims more than is taken is refused before the bytes arrive
        let mut decoder = Decoder::new(false, 16);
        decoder.push(&[0x82, 127, 0, 0, 0, 1, 0, 0, 0, 0]);
        assert!(decoder.pop().is_err());
    }

    #[test]
    fn urls() {
        assert_eq!(Url::parse("ws://127.0.0.1:9601/net/spa").unwrap(), Url { tls: false, host: "127.0.0.1".into(), port: 9601, path: "/net/spa".into() });
        let url = Url::parse("wss://rustyac.example.com/net/spa").unwrap();
        assert_eq!((url.tls, url.port, url.host_field()), (true, 443, "rustyac.example.com".to_string()));
        assert_eq!(Url::parse("ws://host").unwrap().path, "/");
        assert!(Url::parse("http://host/").is_err());
        assert!(Url::parse("ws://host:x/").is_err());
        assert!(is_url("wss://a") && !is_url("127.0.0.1:9600"));
    }

    #[test]
    fn heads() {
        let head = Head::parse("GET /net/spa HTTP/1.1\r\nHost: a\r\nConnection: keep-alive, Upgrade\r\nUPGRADE: WebSocket");
        assert_eq!(head.first_line, "GET /net/spa HTTP/1.1");
        assert_eq!(head.field("host"), Some("a"));
        assert!(head.has_token("connection", "upgrade") && head.has_token("upgrade", "websocket"));
        assert!(!head.has_token("connection", "close"));
    }
}
