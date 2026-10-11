// SPDX-License-Identifier: GPL-3.0-or-later

//! How messages travel. The session logic ([`crate::client`]) only ever sees whole
//! messages on two channels, as AC's `ACClient` does behind `TCPSocket` and `UDPSocket`:
//!
//! * the **reliable** channel (AC: TCP, every message behind a 16-bit length);
//! * the **datagram** channel (AC: UDP, one message per datagram).
//!
//! [`Transport`] is that pair. On the desktop it is a TCP stream and a UDP socket
//! ([`crate::native::NativeTransport`]); in the browser (M2) it will be one WebSocket to a
//! gateway next to the server, which carries both channels. [`Framer`] is the reliable
//! channel's framing for anything that carries it as a byte stream.
//!
//! [`Tape`] records what crossed a transport, step by step, and [`TapePlayer`] is a
//! transport that plays the incoming side of a tape back: the proofs and the golden test
//! run the client against a recorded server with it.

/// The largest message `UDPPacket::send(TCPSocket&)` @ 0x14025db70 accepts (the game stops
/// with "Trying to send a TCP packet bigger than 2046" above it).
pub const MAX_TCP_MESSAGE: usize = 0x7fe;

/// The two channels to a server. Nothing here blocks.
pub trait Transport {
    /// Sends one message on the reliable channel (the transport adds the framing).
    fn send_reliable(&mut self, message: &[u8]) -> Result<(), String>;
    /// Sends one datagram.
    fn send_datagram(&mut self, message: &[u8]) -> Result<(), String>;
    /// The next whole message of the reliable channel, if one has arrived.
    /// `Err`: the connection is gone.
    fn poll_reliable(&mut self) -> Result<Option<Vec<u8>>, String>;
    /// The next datagram, if one has arrived.
    fn poll_datagram(&mut self) -> Result<Option<Vec<u8>>, String>;
    /// Waits up to `timeout_ms` for a message of the reliable channel (the handshake and
    /// the car list are question and answer). A transport that cannot wait (the browser)
    /// returns what [`Transport::poll_reliable`] returns.
    fn wait_reliable(&mut self, timeout_ms: u32) -> Result<Option<Vec<u8>>, String> {
        let _ = timeout_ms;
        self.poll_reliable()
    }
    /// The port the server takes datagrams on (its handshake reply names it). A
    /// transport that does not send datagrams itself (through a gateway) has no use for it.
    fn set_datagram_port(&mut self, port: u16) {
        let _ = port;
    }
    /// The caller's clock (the number of the physics step), for transports that keep a
    /// tape or play one.
    fn set_tick(&mut self, tick: u64) {
        let _ = tick;
    }
    /// What crossed so far, when the transport keeps a tape.
    fn tape(&self) -> Option<&Tape> {
        None
    }
}

impl<T: Transport + ?Sized> Transport for Box<T> {
    fn send_reliable(&mut self, message: &[u8]) -> Result<(), String> {
        (**self).send_reliable(message)
    }
    fn send_datagram(&mut self, message: &[u8]) -> Result<(), String> {
        (**self).send_datagram(message)
    }
    fn poll_reliable(&mut self) -> Result<Option<Vec<u8>>, String> {
        (**self).poll_reliable()
    }
    fn poll_datagram(&mut self) -> Result<Option<Vec<u8>>, String> {
        (**self).poll_datagram()
    }
    fn wait_reliable(&mut self, timeout_ms: u32) -> Result<Option<Vec<u8>>, String> {
        (**self).wait_reliable(timeout_ms)
    }
    fn set_datagram_port(&mut self, port: u16) {
        (**self).set_datagram_port(port)
    }
    fn set_tick(&mut self, tick: u64) {
        (**self).set_tick(tick)
    }
    fn tape(&self) -> Option<&Tape> {
        (**self).tape()
    }
}

/// The reliable channel's framing: `u16` length (of the message alone), then the message.
/// `TCPQueue::push` @ 0x14025e330 / `getPacket` @ 0x14025e240.
#[derive(Clone, Debug, Default)]
pub struct Framer {
    buffer: Vec<u8>,
}

impl Framer {
    pub fn new() -> Framer {
        Framer::default()
    }

    /// A message with its length in front, as it goes onto the stream.
    pub fn frame(message: &[u8]) -> Result<Vec<u8>, String> {
        if message.len() > MAX_TCP_MESSAGE {
            return Err(format!("a message of {} bytes is larger than the {MAX_TCP_MESSAGE} the protocol allows", message.len()));
        }
        let mut out = Vec::with_capacity(message.len() + 2);
        out.extend_from_slice(&(message.len() as u16).to_le_bytes());
        out.extend_from_slice(message);
        Ok(out)
    }

    /// Bytes that arrived on the stream.
    pub fn push(&mut self, bytes: &[u8]) {
        self.buffer.extend_from_slice(bytes);
    }

    /// The next whole message, when all of it is there.
    pub fn pop(&mut self) -> Option<Vec<u8>> {
        if self.buffer.len() < 2 {
            return None;
        }
        let length = u16::from_le_bytes([self.buffer[0], self.buffer[1]]) as usize;
        if self.buffer.len() < 2 + length {
            return None;
        }
        let message = self.buffer[2..2 + length].to_vec();
        self.buffer.drain(..2 + length);
        Some(message)
    }

    pub fn buffered(&self) -> usize {
        self.buffer.len()
    }
}

/// Which way and on which channel a taped message went.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Lane {
    ReliableOut = 0,
    ReliableIn = 1,
    DatagramOut = 2,
    DatagramIn = 3,
}

impl Lane {
    pub fn from_u8(value: u8) -> Option<Lane> {
        Some(match value {
            0 => Lane::ReliableOut,
            1 => Lane::ReliableIn,
            2 => Lane::DatagramOut,
            3 => Lane::DatagramIn,
            _ => return None,
        })
    }

    pub fn is_out(self) -> bool {
        matches!(self, Lane::ReliableOut | Lane::DatagramOut)
    }
}

/// One message of a tape. `tick` is the caller's clock: the client stamps the number of
/// the physics step in which the message crossed (0 while loading).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TapeEntry {
    pub tick: u64,
    pub lane: Lane,
    pub data: Vec<u8>,
}

/// What crossed a transport, in order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Tape {
    pub entries: Vec<TapeEntry>,
}

const TAPE_MAGIC: &[u8; 8] = b"RYNTAPE1";

impl Tape {
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = TAPE_MAGIC.to_vec();
        out.extend_from_slice(&(self.entries.len() as u32).to_le_bytes());
        for entry in &self.entries {
            out.extend_from_slice(&entry.tick.to_le_bytes());
            out.push(entry.lane as u8);
            out.extend_from_slice(&(entry.data.len() as u32).to_le_bytes());
            out.extend_from_slice(&entry.data);
        }
        out
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Tape, String> {
        if bytes.len() < 12 || &bytes[..8] != TAPE_MAGIC {
            return Err("not a rustyAC network tape".to_string());
        }
        let count = u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]) as usize;
        let mut pos = 12;
        let mut entries = Vec::with_capacity(count.min(1 << 20));
        for _ in 0..count {
            if pos + 13 > bytes.len() {
                return Err("the network tape is cut short".to_string());
            }
            let tick = u64::from_le_bytes(bytes[pos..pos + 8].try_into().unwrap());
            let lane = Lane::from_u8(bytes[pos + 8]).ok_or("the network tape has an unknown lane")?;
            let length = u32::from_le_bytes(bytes[pos + 9..pos + 13].try_into().unwrap()) as usize;
            pos += 13;
            if pos + length > bytes.len() {
                return Err("the network tape is cut short".to_string());
            }
            entries.push(TapeEntry { tick, lane, data: bytes[pos..pos + length].to_vec() });
            pos += length;
        }
        Ok(Tape { entries })
    }

    pub fn of(&self, lane: Lane) -> impl Iterator<Item = &TapeEntry> {
        self.entries.iter().filter(move |entry| entry.lane == lane)
    }
}

/// A transport that writes everything crossing `inner` onto a tape.
pub struct Taped<T: Transport> {
    pub inner: T,
    pub tape: Tape,
    /// The clock the next entries are stamped with.
    pub tick: u64,
}

impl<T: Transport> Taped<T> {
    pub fn new(inner: T) -> Taped<T> {
        Taped { inner, tape: Tape::default(), tick: 0 }
    }

    fn note(&mut self, lane: Lane, data: &[u8]) {
        self.tape.entries.push(TapeEntry { tick: self.tick, lane, data: data.to_vec() });
    }
}

impl<T: Transport> Transport for Taped<T> {
    fn send_reliable(&mut self, message: &[u8]) -> Result<(), String> {
        self.note(Lane::ReliableOut, message);
        self.inner.send_reliable(message)
    }

    fn send_datagram(&mut self, message: &[u8]) -> Result<(), String> {
        self.note(Lane::DatagramOut, message);
        self.inner.send_datagram(message)
    }

    fn poll_reliable(&mut self) -> Result<Option<Vec<u8>>, String> {
        let message = self.inner.poll_reliable()?;
        if let Some(message) = &message {
            self.note(Lane::ReliableIn, message);
        }
        Ok(message)
    }

    fn poll_datagram(&mut self) -> Result<Option<Vec<u8>>, String> {
        let message = self.inner.poll_datagram()?;
        if let Some(message) = &message {
            self.note(Lane::DatagramIn, message);
        }
        Ok(message)
    }

    fn wait_reliable(&mut self, timeout_ms: u32) -> Result<Option<Vec<u8>>, String> {
        let message = self.inner.wait_reliable(timeout_ms)?;
        if let Some(message) = &message {
            self.note(Lane::ReliableIn, message);
        }
        Ok(message)
    }

    fn set_datagram_port(&mut self, port: u16) {
        self.inner.set_datagram_port(port);
    }

    fn set_tick(&mut self, tick: u64) {
        self.tick = tick;
        self.inner.set_tick(tick);
    }

    fn tape(&self) -> Option<&Tape> {
        Some(&self.tape)
    }
}

/// A transport that plays a tape's incoming messages back: each is handed out once the
/// clock ([`TapePlayer::tick`]) has reached the tick it was recorded at, in the recorded
/// order per channel. What the client sends is kept in [`TapePlayer::sent`], stamped with
/// the clock, to be held against the tape's outgoing side.
#[derive(Clone, Debug, Default)]
pub struct TapePlayer {
    reliable: std::collections::VecDeque<TapeEntry>,
    datagrams: std::collections::VecDeque<TapeEntry>,
    pub sent: Tape,
    pub tick: u64,
}

impl TapePlayer {
    pub fn new(tape: &Tape) -> TapePlayer {
        TapePlayer {
            reliable: tape.of(Lane::ReliableIn).cloned().collect(),
            datagrams: tape.of(Lane::DatagramIn).cloned().collect(),
            sent: Tape::default(),
            tick: 0,
        }
    }

    /// Incoming messages not handed out yet.
    pub fn pending(&self) -> usize {
        self.reliable.len() + self.datagrams.len()
    }

    fn next(queue: &mut std::collections::VecDeque<TapeEntry>, tick: u64) -> Option<Vec<u8>> {
        if queue.front().is_some_and(|entry| entry.tick <= tick) {
            queue.pop_front().map(|entry| entry.data)
        } else {
            None
        }
    }
}

impl Transport for TapePlayer {
    fn send_reliable(&mut self, message: &[u8]) -> Result<(), String> {
        self.sent.entries.push(TapeEntry { tick: self.tick, lane: Lane::ReliableOut, data: message.to_vec() });
        Ok(())
    }

    fn send_datagram(&mut self, message: &[u8]) -> Result<(), String> {
        self.sent.entries.push(TapeEntry { tick: self.tick, lane: Lane::DatagramOut, data: message.to_vec() });
        Ok(())
    }

    fn poll_reliable(&mut self) -> Result<Option<Vec<u8>>, String> {
        Ok(TapePlayer::next(&mut self.reliable, self.tick))
    }

    fn poll_datagram(&mut self) -> Result<Option<Vec<u8>>, String> {
        Ok(TapePlayer::next(&mut self.datagrams, self.tick))
    }

    /// (the handshake and the car list wait: on a tape the answer is always there)
    fn wait_reliable(&mut self, _timeout_ms: u32) -> Result<Option<Vec<u8>>, String> {
        Ok(self.reliable.pop_front().map(|entry| entry.data))
    }

    fn set_tick(&mut self, tick: u64) {
        self.tick = tick;
    }

    fn tape(&self) -> Option<&Tape> {
        Some(&self.sent)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn framer_splits_a_stream() {
        let mut stream = Framer::frame(&[0x3d, 1, 2]).unwrap();
        stream.extend(Framer::frame(&[]).unwrap());
        stream.extend(Framer::frame(&[9; 300]).unwrap());
        let mut framer = Framer::new();
        let mut out = Vec::new();
        for chunk in stream.chunks(7) {
            framer.push(chunk);
            while let Some(message) = framer.pop() {
                out.push(message);
            }
        }
        assert_eq!(out, vec![vec![0x3d, 1, 2], vec![], vec![9; 300]]);
        assert!(Framer::frame(&[0; MAX_TCP_MESSAGE + 1]).is_err());
    }

    #[test]
    fn tape_round_trip_and_player() {
        let mut tape = Tape::default();
        tape.entries.push(TapeEntry { tick: 0, lane: Lane::ReliableOut, data: vec![1] });
        tape.entries.push(TapeEntry { tick: 0, lane: Lane::ReliableIn, data: vec![2, 3] });
        tape.entries.push(TapeEntry { tick: 5, lane: Lane::DatagramIn, data: vec![4] });
        assert_eq!(Tape::from_bytes(&tape.to_bytes()).unwrap(), tape);
        let mut player = TapePlayer::new(&tape);
        assert_eq!(player.poll_reliable().unwrap(), Some(vec![2, 3]));
        assert_eq!(player.poll_datagram().unwrap(), None);
        player.tick = 5;
        assert_eq!(player.poll_datagram().unwrap(), Some(vec![4]));
        assert_eq!(player.pending(), 0);
    }
}
