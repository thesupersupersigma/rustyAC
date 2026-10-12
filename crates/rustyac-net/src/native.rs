// SPDX-License-Identifier: GPL-3.0-or-later

//! The desktop transport: a TCP stream and a UDP socket, as AC's `TCPSocket` and
//! `UDPSocket` (ksNetwork.lib) use them. Plain `std::net`: nothing here is Windows code.
//!
//! * `TCPSocket::connect` @ 0x14025e1a0: a blocking connect; the game then sets
//!   `SO_DEBUG` where it means `TCP_NODELAY` (its own log line says so). Here the stream
//!   really has `TCP_NODELAY`: the bytes on the wire are the same, they only leave sooner.
//! * `UDPSocket::receive` @ 0x14025d400 answers the server's ping (`0xf9`) inside the
//!   socket; that answer is session logic here ([`crate::client`]), so that every
//!   transport gets it.

use std::io::{ErrorKind, Read, Write};
use std::net::{SocketAddr, TcpStream, ToSocketAddrs, UdpSocket};
use std::time::Duration;

use crate::transport::{Framer, Transport};

pub struct NativeTransport {
    tcp: TcpStream,
    udp: UdpSocket,
    /// Where datagrams go: the server's address with the UDP port its handshake reply
    /// named ([`NativeTransport::set_datagram_port`]).
    udp_target: SocketAddr,
    framer: Framer,
    closed: bool,
}

impl NativeTransport {
    /// Connects the reliable channel to `host:port` (the server's TCP port). The UDP socket
    /// is bound on the same local address the TCP connection uses (so a server on
    /// 127.0.0.1 is talked to from 127.0.0.1 only).
    pub fn connect(host: &str, port: u16, timeout_ms: u32) -> Result<NativeTransport, String> {
        let address = (host, port)
            .to_socket_addrs()
            .map_err(|e| format!("{host}:{port}: {e}"))?
            .find(SocketAddr::is_ipv4)
            .ok_or(format!("{host}: no IPv4 address (Assetto Corsa's protocol is IPv4)"))?;
        let tcp = TcpStream::connect_timeout(&address, Duration::from_millis(timeout_ms as u64)).map_err(|e| format!("connecting to {address}: {e}"))?;
        tcp.set_nodelay(true).map_err(|e| e.to_string())?;
        let local = tcp.local_addr().map_err(|e| e.to_string())?;
        let udp = UdpSocket::bind(SocketAddr::new(local.ip(), 0)).map_err(|e| format!("UDP socket: {e}"))?;
        udp.set_nonblocking(true).map_err(|e| e.to_string())?;
        Ok(NativeTransport { tcp, udp, udp_target: address, framer: Framer::new(), closed: false })
    }

    pub fn server(&self) -> SocketAddr {
        self.udp_target
    }

    fn read_available(&mut self) -> Result<(), String> {
        let mut buffer = [0u8; 0x800];
        loop {
            match self.tcp.read(&mut buffer) {
                Ok(0) => {
                    self.closed = true;
                    return Ok(());
                }
                Ok(count) => self.framer.push(&buffer[..count]),
                Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => return Ok(()),
                Err(e) if e.kind() == ErrorKind::Interrupted => {}
                Err(e) => {
                    self.closed = true;
                    return Err(format!("the connection to the server broke: {e}"));
                }
            }
        }
    }
}

impl Transport for NativeTransport {
    fn send_reliable(&mut self, message: &[u8]) -> Result<(), String> {
        let frame = Framer::frame(message)?;
        // a send must not be cut short by the non-blocking mode of the reads
        self.tcp.set_nonblocking(false).map_err(|e| e.to_string())?;
        self.tcp.write_all(&frame).map_err(|e| format!("sending to the server: {e}"))
    }

    fn send_datagram(&mut self, message: &[u8]) -> Result<(), String> {
        match self.udp.send_to(message, self.udp_target) {
            Ok(_) => Ok(()),
            // (a full send buffer drops the datagram, as UDP may anyway)
            Err(e) if e.kind() == ErrorKind::WouldBlock => Ok(()),
            Err(e) => Err(format!("sending a datagram to the server: {e}")),
        }
    }

    fn poll_reliable(&mut self) -> Result<Option<Vec<u8>>, String> {
        if let Some(message) = self.framer.pop() {
            return Ok(Some(message));
        }
        if self.closed {
            return Err("the server closed the connection".to_string());
        }
        self.tcp.set_nonblocking(true).map_err(|e| e.to_string())?;
        self.read_available()?;
        match self.framer.pop() {
            Some(message) => Ok(Some(message)),
            None if self.closed => Err("the server closed the connection".to_string()),
            None => Ok(None),
        }
    }

    fn poll_datagram(&mut self) -> Result<Option<Vec<u8>>, String> {
        let mut buffer = vec![0u8; 0x10000];
        loop {
            match self.udp.recv_from(&mut buffer) {
                // (only the server's datagrams count)
                Ok((count, from)) if from.ip() == self.udp_target.ip() => return Ok(Some(buffer[..count].to_vec())),
                Ok(_) => {}
                Err(e) if e.kind() == ErrorKind::WouldBlock => return Ok(None),
                // Windows reports an ICMP "port unreachable" of an earlier send here
                Err(e) if e.kind() == ErrorKind::ConnectionReset => return Ok(None),
                Err(e) if e.kind() == ErrorKind::Interrupted => {}
                Err(e) => return Err(format!("receiving a datagram: {e}")),
            }
        }
    }

    fn set_datagram_port(&mut self, port: u16) {
        self.udp_target.set_port(port);
    }

    fn wait_reliable(&mut self, timeout_ms: u32) -> Result<Option<Vec<u8>>, String> {
        if let Some(message) = self.framer.pop() {
            return Ok(Some(message));
        }
        if self.closed {
            return Err("the server closed the connection".to_string());
        }
        self.tcp.set_nonblocking(false).map_err(|e| e.to_string())?;
        let deadline = std::time::Instant::now() + Duration::from_millis(timeout_ms as u64);
        let mut buffer = [0u8; 0x800];
        loop {
            let left = deadline.saturating_duration_since(std::time::Instant::now());
            if left.is_zero() {
                return Ok(None);
            }
            self.tcp.set_read_timeout(Some(left)).map_err(|e| e.to_string())?;
            match self.tcp.read(&mut buffer) {
                Ok(0) => {
                    self.closed = true;
                    return Err("the server closed the connection".to_string());
                }
                Ok(count) => {
                    self.framer.push(&buffer[..count]);
                    if let Some(message) = self.framer.pop() {
                        return Ok(Some(message));
                    }
                }
                Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => return Ok(None),
                Err(e) if e.kind() == ErrorKind::Interrupted => {}
                Err(e) => {
                    self.closed = true;
                    return Err(format!("the connection to the server broke: {e}"));
                }
            }
        }
    }
}
