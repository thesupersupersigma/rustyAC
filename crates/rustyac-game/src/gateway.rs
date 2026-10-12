// SPDX-License-Identifier: GPL-3.0-or-later

//! `--join ws://...` / `--join wss://...`: the desktop joins through a WebSocket gateway
//! (`crates/rustyac-gateway`) instead of talking TCP and UDP to the server itself. For a
//! server that is only reachable over HTTPS: a Cloudflare Tunnel carries WebSockets, not
//! UDP, so with a gateway nobody needs a VPN or an open port.
//!
//! `ws://` is a plain TCP stream (local tests, a LAN). `wss://` is TLS: rustls with the ring
//! provider and Mozilla's root certificates compiled in (`webpki-roots`), plus the
//! certificate of `--tls-root` when one is given.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, ServerName};
use rustyac_net::native::{connect_tcp, connect_ws, WsPipe, WsSocket, WsUrl};
use rustyac_net::ws::WsTransport;

use crate::online::BoxedTransport;

/// TLS over a TCP stream, as a stream a WebSocket client can run on.
struct TlsStream(rustls::StreamOwned<rustls::ClientConnection, TcpStream>);

impl Read for TlsStream {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        match self.0.read(buffer) {
            // (a peer that hangs up without TLS's goodbye: the end of the stream all the same)
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => Ok(0),
            other => other,
        }
    }
}

impl Write for TlsStream {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        self.0.write(buffer)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.0.flush()
    }
}

impl WsSocket for TlsStream {
    fn set_nonblocking(&mut self, nonblocking: bool) -> std::io::Result<()> {
        self.0.sock.set_nonblocking(nonblocking)
    }
    fn set_read_timeout(&mut self, timeout: Option<Duration>) -> std::io::Result<()> {
        self.0.sock.set_read_timeout(timeout)
    }
}

/// A Cloudflare Access service token of the environment, as the two header fields
/// Cloudflare reads (so that the token is in no command line and no script). It is a
/// secret for one site: it only goes out over TLS, and only to the host that
/// `CF_ACCESS_HOST` names.
fn access_token(fields: &mut Vec<(String, String)>, url: &WsUrl) {
    let host = std::env::var("CF_ACCESS_HOST").unwrap_or_default();
    if !url.tls || host.trim().is_empty() || !host.trim().eq_ignore_ascii_case(&url.host) {
        return;
    }
    for (variable, name) in [("CF_ACCESS_CLIENT_ID", "CF-Access-Client-Id"), ("CF_ACCESS_CLIENT_SECRET", "CF-Access-Client-Secret")] {
        if let Ok(value) = std::env::var(variable) {
            if !value.trim().is_empty() && !fields.iter().any(|(known, _)| known.eq_ignore_ascii_case(name)) {
                fields.push((name.to_string(), value.trim().to_string()));
            }
        }
    }
}

/// The transport through the gateway at `address`.
pub fn connect(address: &str, headers: &[(String, String)], tls_root: Option<&Path>, timeout_ms: u32) -> Result<BoxedTransport, String> {
    let url = WsUrl::parse(address)?;
    let mut fields = headers.to_vec();
    access_token(&mut fields, &url);
    if !url.tls {
        return Ok(Box::new(connect_ws(&url, &fields, timeout_ms)?));
    }
    let mut roots = rustls::RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    if let Some(file) = tls_root {
        let mut added = 0;
        for certificate in CertificateDer::pem_file_iter(file).map_err(|e| format!("{}: {e}", file.display()))? {
            roots.add(certificate.map_err(|e| format!("{}: {e}", file.display()))?).map_err(|e| format!("{}: {e}", file.display()))?;
            added += 1;
        }
        if added == 0 {
            return Err(format!("{}: no certificate in it (a PEM file is expected)", file.display()));
        }
    }
    let config = rustls::ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
        .with_safe_default_protocol_versions()
        .map_err(|e| e.to_string())?
        .with_root_certificates(roots)
        .with_no_client_auth();
    let name = ServerName::try_from(url.host.clone()).map_err(|_| format!("{}: not a name a certificate can be for", url.host))?;
    let connection = rustls::ClientConnection::new(Arc::new(config), name).map_err(|e| format!("TLS: {e}"))?;
    let tcp = connect_tcp(&url.host, url.port, timeout_ms)?;
    tcp.set_read_timeout(Some(Duration::from_millis(timeout_ms as u64))).map_err(|e| e.to_string())?;
    let mut tls = TlsStream(rustls::StreamOwned::new(connection, tcp));
    // the TLS handshake, here, so that a certificate that is not trusted is said plainly
    while tls.0.conn.is_handshaking() {
        tls.0.conn.complete_io(&mut tls.0.sock).map_err(|e| format!("TLS with {}: {e}", url.host))?;
    }
    Ok(Box::new(WsTransport::new(WsPipe::open(tls, &url, &fields, timeout_ms)?)))
}
