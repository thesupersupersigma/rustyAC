// SPDX-License-Identifier: GPL-3.0-or-later

//! A TLS front for a local check of `rustyac.exe --join wss://...`: accepts TLS on one port
//! with a certificate of your own and passes the bytes on to a plain port (the gateway's),
//! the way an HTTPS tunnel stands in front of a gateway.
//!
//! ```text
//! # a root of your own and a certificate for "localhost" signed by it (a certificate that
//! # signs itself is a root, and a root is not taken as a server's certificate)
//! openssl req -x509 -newkey rsa:2048 -nodes -keyout ca_key.pem -out ca.pem -days 2 -subj "/CN=test root"
//! openssl req -newkey rsa:2048 -nodes -keyout key.pem -out leaf.csr -subj "/CN=localhost"
//! printf 'basicConstraints=CA:FALSE\nsubjectAltName=DNS:localhost\n' > leaf.ext
//! openssl x509 -req -in leaf.csr -CA ca.pem -CAkey ca_key.pem -CAcreateserial -out cert.pem -days 2 -extfile leaf.ext
//! cargo run --release -p rustyac-game --example tls_front -- 127.0.0.1:9643 127.0.0.1:9601 cert.pem key.pem
//! rustyac.exe --join wss://localhost:9643/net/spa --tls-root ca.pem ...
//! ```
//!
//! Only for tests on one machine: one thread per connection that looks at both sides in
//! turn.

#[cfg(windows)]
fn main() {
    use std::io::{ErrorKind, Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::sync::Arc;

    use rustls::pki_types::pem::PemObject;
    use rustls::pki_types::{CertificateDer, PrivateKeyDer};

    let args: Vec<String> = std::env::args().skip(1).collect();
    let [listen, target, cert, key] = args.as_slice() else {
        eprintln!("tls_front <listen addr:port> <target addr:port> <cert.pem> <key.pem>");
        std::process::exit(2);
    };
    let certs: Vec<CertificateDer> = CertificateDer::pem_file_iter(cert).expect("the certificate file").map(|c| c.expect("a certificate")).collect();
    let key = PrivateKeyDer::from_pem_file(key).expect("the key file");
    let config = rustls::ServerConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
        .with_safe_default_protocol_versions()
        .expect("protocol versions")
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .expect("the certificate and its key");
    let config = Arc::new(config);
    let listener = TcpListener::bind(listen).expect("the listening port");
    println!("TLS on {listen}, passing on to {target}");
    for stream in listener.incoming().flatten() {
        let (config, target) = (Arc::clone(&config), target.clone());
        std::thread::spawn(move || {
            let Ok(mut back) = TcpStream::connect(&target) else {
                return;
            };
            let Ok(connection) = rustls::ServerConnection::new(config) else {
                return;
            };
            let mut front = rustls::StreamOwned::new(connection, stream);
            // the handshake while the socket still waits, then both sides without waiting
            while front.conn.is_handshaking() {
                if front.conn.complete_io(&mut front.sock).is_err() {
                    return;
                }
            }
            let _ = front.sock.set_nonblocking(true);
            let _ = back.set_nonblocking(true);
            let _ = back.set_nodelay(true);
            let _ = front.sock.set_nodelay(true);
            let mut buffer = [0u8; 0x4000];
            // a write that would block is tried again: nothing is dropped
            fn write_all(to: &mut impl Write, mut bytes: &[u8]) -> bool {
                while !bytes.is_empty() {
                    match to.write(bytes) {
                        Ok(0) => return false,
                        Ok(count) => bytes = &bytes[count..],
                        Err(e) if e.kind() == ErrorKind::WouldBlock => std::thread::sleep(std::time::Duration::from_micros(200)),
                        Err(_) => return false,
                    }
                }
                loop {
                    match to.flush() {
                        Ok(()) => return true,
                        Err(e) if e.kind() == ErrorKind::WouldBlock => std::thread::sleep(std::time::Duration::from_micros(200)),
                        Err(_) => return false,
                    }
                }
            }
            loop {
                let mut idle = true;
                match front.read(&mut buffer) {
                    Ok(0) => return,
                    Ok(count) => {
                        idle = false;
                        if !write_all(&mut back, &buffer[..count]) {
                            return;
                        }
                    }
                    Err(e) if e.kind() == ErrorKind::WouldBlock => {}
                    Err(_) => return,
                }
                match back.read(&mut buffer) {
                    Ok(0) => return,
                    Ok(count) => {
                        idle = false;
                        if !write_all(&mut front, &buffer[..count]) {
                            return;
                        }
                    }
                    Err(e) if e.kind() == ErrorKind::WouldBlock => {}
                    Err(_) => return,
                }
                if idle {
                    std::thread::sleep(std::time::Duration::from_micros(500));
                }
            }
        });
    }
}

#[cfg(not(windows))]
fn main() {
    eprintln!("tls_front is a Windows-side test helper (rustyac-game's TLS is only built there)");
}
