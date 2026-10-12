// SPDX-License-Identifier: GPL-3.0-or-later

//! The transport through a gateway: both channels in one WebSocket.
//!
//! A browser can open neither a TCP connection nor a UDP socket, and an HTTP-only tunnel
//! carries neither. `rustyac-gateway` (`crates/rustyac-gateway`) stands next to the server
//! and carries one client per WebSocket. Every binary message of the WebSocket is one lane
//! byte and one whole message of the game's protocol:
//!
//! ```text
//! [0][message]    the reliable channel (the gateway adds / removes TCP's 16-bit length)
//! [1][datagram]   the datagram channel (the gateway's own UDP socket for this client)
//! ```
//!
//! [`WsTransport`] is that framing over a [`Pipe`]: anything that carries whole binary
//! messages. The desktop's pipe is a WebSocket client on a TCP or TLS stream
//! ([`crate::native::WsPipe`]); the browser's is the page's own `WebSocket`
//! (`rustyac-web`). The session logic above cannot tell them from TCP + UDP.

use std::collections::VecDeque;

use crate::transport::{Lane, Tape, TapeEntry, Transport, MAX_TCP_MESSAGE};

/// The lane byte of the reliable channel.
pub const LANE_RELIABLE: u8 = 0;
/// The lane byte of the datagram channel.
pub const LANE_DATAGRAM: u8 = 1;

/// How many arrived datagrams are kept when nobody takes them (a tab in the background
/// does not step): the oldest go first, as a UDP socket's buffer would lose them.
pub const DATAGRAM_BACKLOG: usize = 512;

/// Whole binary messages both ways (one WebSocket).
pub trait Pipe {
    fn send(&mut self, message: &[u8]) -> Result<(), String>;
    /// The next message, if one has arrived. `Err`: the connection is gone.
    fn poll(&mut self) -> Result<Option<Vec<u8>>, String>;
    /// Waits up to `timeout_ms` for a message. A pipe that cannot wait (the browser)
    /// returns what [`Pipe::poll`] returns.
    fn wait(&mut self, timeout_ms: u32) -> Result<Option<Vec<u8>>, String> {
        let _ = timeout_ms;
        self.poll()
    }
    /// The caller's clock (the number of the physics step), for a pipe that plays a tape.
    fn set_tick(&mut self, tick: u64) {
        let _ = tick;
    }
    /// What was sent so far, when the pipe keeps it.
    fn tape(&self) -> Option<&Tape> {
        None
    }
}

impl<P: Pipe + ?Sized> Pipe for Box<P> {
    fn send(&mut self, message: &[u8]) -> Result<(), String> {
        (**self).send(message)
    }
    fn poll(&mut self) -> Result<Option<Vec<u8>>, String> {
        (**self).poll()
    }
    fn wait(&mut self, timeout_ms: u32) -> Result<Option<Vec<u8>>, String> {
        (**self).wait(timeout_ms)
    }
    fn set_tick(&mut self, tick: u64) {
        (**self).set_tick(tick)
    }
    fn tape(&self) -> Option<&Tape> {
        (**self).tape()
    }
}

/// A message with its lane byte in front.
pub fn with_lane(lane: u8, message: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(message.len() + 1);
    out.push(lane);
    out.extend_from_slice(message);
    out
}

/// The two channels in one [`Pipe`].
pub struct WsTransport<P: Pipe> {
    pub pipe: P,
    reliable: VecDeque<Vec<u8>>,
    datagrams: VecDeque<Vec<u8>>,
    /// Why the pipe is gone, once it is (told when the reliable channel has been emptied).
    lost: Option<String>,
    /// Datagrams dropped because nobody took them in time.
    pub datagrams_dropped: u64,
}

impl<P: Pipe> WsTransport<P> {
    pub fn new(pipe: P) -> WsTransport<P> {
        WsTransport { pipe, reliable: VecDeque::new(), datagrams: VecDeque::new(), lost: None, datagrams_dropped: 0 }
    }

    fn sort(&mut self, message: Vec<u8>) {
        match message.first() {
            Some(&LANE_RELIABLE) => self.reliable.push_back(message[1..].to_vec()),
            Some(&LANE_DATAGRAM) => {
                if self.datagrams.len() >= DATAGRAM_BACKLOG {
                    self.datagrams.pop_front();
                    self.datagrams_dropped += 1;
                }
                self.datagrams.push_back(message[1..].to_vec());
            }
            // (a lane this version does not know: a newer gateway's; not for the session)
            _ => {}
        }
    }

    /// Takes everything the pipe has into the two queues.
    fn pump(&mut self) {
        while self.lost.is_none() {
            match self.pipe.poll() {
                Ok(Some(message)) => self.sort(message),
                Ok(None) => break,
                Err(why) => self.lost = Some(why),
            }
        }
    }
}

impl<P: Pipe> Transport for WsTransport<P> {
    fn send_reliable(&mut self, message: &[u8]) -> Result<(), String> {
        if message.len() > MAX_TCP_MESSAGE {
            return Err(format!("a message of {} bytes is larger than the {MAX_TCP_MESSAGE} the protocol allows", message.len()));
        }
        self.pipe.send(&with_lane(LANE_RELIABLE, message))
    }

    fn send_datagram(&mut self, message: &[u8]) -> Result<(), String> {
        self.pipe.send(&with_lane(LANE_DATAGRAM, message))
    }

    fn poll_reliable(&mut self) -> Result<Option<Vec<u8>>, String> {
        if self.reliable.is_empty() {
            self.pump();
        }
        match (self.reliable.pop_front(), &self.lost) {
            (Some(message), _) => Ok(Some(message)),
            (None, Some(why)) => Err(why.clone()),
            (None, None) => Ok(None),
        }
    }

    fn poll_datagram(&mut self) -> Result<Option<Vec<u8>>, String> {
        if self.datagrams.is_empty() {
            self.pump();
        }
        // (a lost pipe is told on the reliable channel, as a lost TCP connection is)
        Ok(self.datagrams.pop_front())
    }

    fn wait_reliable(&mut self, timeout_ms: u32) -> Result<Option<Vec<u8>>, String> {
        if let Some(message) = self.poll_reliable()? {
            return Ok(Some(message));
        }
        // datagrams that come first are kept for their own channel. Each wait starts the
        // timeout again: only an unbroken silence of `timeout_ms` ends it.
        loop {
            match self.pipe.wait(timeout_ms) {
                Ok(Some(message)) => {
                    self.sort(message);
                    if let Some(message) = self.reliable.pop_front() {
                        return Ok(Some(message));
                    }
                }
                Ok(None) => return Ok(None),
                Err(why) => {
                    self.lost = Some(why.clone());
                    return Err(why);
                }
            }
        }
    }

    fn set_tick(&mut self, tick: u64) {
        self.pipe.set_tick(tick);
    }

    fn tape(&self) -> Option<&Tape> {
        self.pipe.tape()
    }
}

/// A pipe that plays a tape, for the proofs: the tape's incoming messages of both channels
/// come out as a gateway would send them (one lane byte in front), each once the clock has
/// reached the step it was recorded in, in the tape's order; what is sent is taken apart
/// again and kept as a tape. A session on a [`WsTransport`] over this pipe must behave
/// exactly as one on a [`crate::transport::TapePlayer`] of the same tape.
#[derive(Clone, Debug, Default)]
pub struct TapePipe {
    arriving: VecDeque<TapeEntry>,
    pub sent: Tape,
    pub tick: u64,
}

impl TapePipe {
    pub fn new(tape: &Tape) -> TapePipe {
        TapePipe { arriving: tape.entries.iter().filter(|entry| !entry.lane.is_out()).cloned().collect(), sent: Tape::default(), tick: 0 }
    }
}

impl Pipe for TapePipe {
    fn send(&mut self, message: &[u8]) -> Result<(), String> {
        let lane = match message.first() {
            Some(&LANE_RELIABLE) => Lane::ReliableOut,
            Some(&LANE_DATAGRAM) => Lane::DatagramOut,
            _ => return Err("a message without a lane byte".to_string()),
        };
        self.sent.entries.push(TapeEntry { tick: self.tick, lane, data: message[1..].to_vec() });
        Ok(())
    }

    fn poll(&mut self) -> Result<Option<Vec<u8>>, String> {
        if !self.arriving.front().is_some_and(|entry| entry.tick <= self.tick) {
            return Ok(None);
        }
        Ok(self.arriving.pop_front().map(|entry| with_lane(if entry.lane == Lane::ReliableIn { LANE_RELIABLE } else { LANE_DATAGRAM }, &entry.data)))
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

    /// A pipe whose other end is two lists.
    #[derive(Default)]
    struct Loop {
        arriving: VecDeque<Vec<u8>>,
        sent: Vec<Vec<u8>>,
        broken: bool,
    }

    impl Pipe for Loop {
        fn send(&mut self, message: &[u8]) -> Result<(), String> {
            self.sent.push(message.to_vec());
            Ok(())
        }
        fn poll(&mut self) -> Result<Option<Vec<u8>>, String> {
            match self.arriving.pop_front() {
                Some(message) => Ok(Some(message)),
                None if self.broken => Err("gone".to_string()),
                None => Ok(None),
            }
        }
    }

    #[test]
    fn lanes_are_kept_apart_and_in_order() {
        let mut transport = WsTransport::new(Loop::default());
        transport.send_reliable(&[0x3d, 1]).unwrap();
        transport.send_datagram(&[0x46, 2, 3]).unwrap();
        assert_eq!(transport.pipe.sent, vec![vec![0, 0x3d, 1], vec![1, 0x46, 2, 3]]);
        assert!(transport.send_reliable(&[0; MAX_TCP_MESSAGE + 1]).is_err());
        for message in [vec![1, 10], vec![0, 20], vec![1, 11], vec![7, 99], vec![], vec![0, 21], vec![0]] {
            transport.pipe.arriving.push_back(message);
        }
        assert_eq!(transport.poll_datagram().unwrap(), Some(vec![10]));
        assert_eq!(transport.poll_reliable().unwrap(), Some(vec![20]));
        assert_eq!(transport.poll_reliable().unwrap(), Some(vec![21]));
        assert_eq!(transport.poll_reliable().unwrap(), Some(vec![]));
        assert_eq!(transport.poll_datagram().unwrap(), Some(vec![11]));
        assert_eq!(transport.poll_datagram().unwrap(), None);
        assert_eq!(transport.poll_reliable().unwrap(), None);
    }

    #[test]
    fn a_lost_pipe_is_told_after_what_arrived() {
        let mut transport = WsTransport::new(Loop::default());
        transport.pipe.arriving.push_back(vec![0, 5]);
        transport.pipe.broken = true;
        assert_eq!(transport.poll_datagram().unwrap(), None);
        assert_eq!(transport.poll_reliable().unwrap(), Some(vec![5]));
        assert_eq!(transport.poll_reliable(), Err("gone".to_string()));
        assert_eq!(transport.poll_datagram().unwrap(), None);
    }

    #[test]
    fn old_datagrams_go_first_when_nobody_takes_them() {
        let mut transport = WsTransport::new(Loop::default());
        for number in 0..DATAGRAM_BACKLOG as u32 + 10 {
            transport.pipe.arriving.push_back(with_lane(LANE_DATAGRAM, &number.to_le_bytes()));
        }
        assert_eq!(transport.poll_datagram().unwrap(), Some(10u32.to_le_bytes().to_vec()));
        assert_eq!(transport.datagrams_dropped, 10);
    }
}
