// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa's launcher (AssettoCorsa.exe, Kunos Simulazioni).

//! The online handlers of `ACRequestHandler`: the server list and a server's details from
//! Kunos' lobby (`getserverlist`, `getserver`, `getservercached`), `pingserver`, and the LAN
//! scan (`getlanservers`). They wait on the network, so each one is a [`Job`] the window runs
//! on a thread of its own (CEF ran them off its UI thread too). The lobby's host is
//! `ACRequestHandler._lobbyserver`, with its fallback when the lobby does not answer.

use crate::bridge::{Answer, Thrown};
use crate::ini::IniFile;
use crate::net;
use crate::query::Params;
use std::collections::HashSet;
use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpStream, ToSocketAddrs, UdpSocket};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// A request that waits on the network, answered on another thread.
pub type Job = Box<dyn FnOnce() -> Answer + Send>;

const MAIN: &str = "lobby.assettocorsa.net";
const FALLBACK: &str = "aclobby2.grecian.net:10080";

/// The launcher's static lobby fields: `_lobbyserver`, `_usefallback`, `_serverlist`.
pub struct Lobby {
    server: String,
    use_fallback: bool,
    list: Option<String>,
    /// `--lobby`: both hosts moved to this one (the proofs' stand-in).
    moved: Option<String>,
}

impl Lobby {
    pub fn new(moved: Option<String>) -> Lobby {
        Lobby { server: moved.clone().unwrap_or(MAIN.into()), use_fallback: false, list: None, moved }
    }

    fn host(&self, fallback: bool) -> String {
        self.moved.clone().unwrap_or(if fallback { FALLBACK } else { MAIN }.into())
    }
}

/// The job of an online request, or `None` when `host` is not one.
pub fn job(host: &str, p: &Params, lobby: &Arc<Mutex<Lobby>>, guid: impl FnOnce() -> String, launcher_ini: &str) -> Option<Job> {
    let lobby = lobby.clone();
    Some(match host {
        "getserver" | "getserverlist" => {
            let guid = guid();
            let authkey = IniFile::new(launcher_ini).read(Some("ONLINE"), Some("AUTHKEY"));
            let what = if host == "getserver" {
                format!("single?ip={}&port={}&guid={guid}", p.get("ip").unwrap_or_default(), p.get("port").unwrap_or_default())
            } else {
                format!("list?guid={guid}")
            };
            let list = host == "getserverlist";
            Box::new(move || lobby_get(&lobby, &authkey, &what, &guid, list))
        }
        "getservercached" => Box::new(move || {
            let text = lobby.lock().unwrap().list.clone();
            server_list_cached(text.as_deref())
        }),
        "pingserver" => {
            let (ip, port) = (p.get("ip"), try_parse(p.get("port").as_deref()).unwrap_or(0));
            Box::new(move || ping_server(ip.as_deref(), port))
        }
        "getlanservers" => {
            let (mut from, mut to) = (9000, 10000);
            if p.len() > 1 {
                from = try_parse(p.get("from").as_deref()).unwrap_or(9000);
                to = try_parse(p.get("to").as_deref()).unwrap_or(10000);
            }
            Box::new(move || lan_servers(from, to))
        }
        _ => return None,
    })
}

/// `int.TryParse`.
fn try_parse(s: Option<&str>) -> Option<i32> {
    s.map(str::trim).and_then(|s| s.parse().ok())
}

/// `GetServer` / `GetServerList`: the lobby's answer as it is, else the fallback host's, else
/// "networkerror" (and the main host again next time).
fn lobby_get(lobby: &Mutex<Lobby>, authkey: &str, what: &str, guid: &str, list: bool) -> Answer {
    loop {
        let server = lobby.lock().unwrap().server.clone();
        let key = if authkey.is_empty() { String::new() } else { format!("/{authkey}") };
        let url = format!("http://{server}{key}/lobby.ashx/{what}");
        let got = if list {
            let reversed: String = guid.chars().rev().collect();
            let hash = ac_hash(&ac_hash(&format!("{reversed}{guid}")));
            http_get(&url, &[(".", &hash)], true, 90_000)
        } else {
            http_get(&url, &[], false, 60_000)
        };
        let mut l = lobby.lock().unwrap();
        match got {
            Ok(text) => {
                if list {
                    l.list = Some(text.clone());
                }
                return Answer::json(&text);
            }
            Err(_) if !l.use_fallback => {
                l.use_fallback = true;
                l.server = l.host(true);
            }
            Err(_) => {
                l.use_fallback = false;
                l.server = l.host(false);
                return Answer::message("networkerror");
            }
        }
    }
}

/// `ACUtility.Hash`: MD5 of the ASCII bytes, upper-case hex.
fn ac_hash(s: &str) -> String {
    let ascii: Vec<u8> = s.chars().map(|c| if c.is_ascii() { c as u8 } else { b'?' }).collect();
    net::to_upper(&rustyac_net::md5::hex(&rustyac_net::md5::digest(&ascii)))
}

/// `GetServerListCached`: how many fields the first server of the last list has.
fn server_list_cached(text: Option<&str>) -> Answer {
    let thrown = match text {
        None => Thrown::Other("Value cannot be null.\r\nParameter name: input".into()),
        Some(t) if t.encode_utf16().count() > 2_097_152 => Thrown::Other(
            "Error during serialization or deserialization using the JSON JavaScriptSerializer. The length of the string exceeds the value set on the maxJsonLength property.\r\nParameter name: input".into(),
        ),
        Some(t) => match crate::jss::deserialize(t) {
            Ok(crate::jss::Value::Array(servers)) => match servers.first() {
                Some(crate::jss::Value::Object(fields)) => return Answer::message(&fields.len().to_string()),
                Some(_) => Thrown::Other("Cannot convert object of type 'System.Object' to type 'System.Collections.Generic.Dictionary`2[System.String,System.Object]'".into()),
                None => Thrown::ArgumentOutOfRange,
            },
            // shortcut: one message for every other way the list does not parse, the lobby always sends a list
            _ => Thrown::Other("Invalid JSON primitive: .".into()),
        },
    };
    thrown.answer()
}

/// The `host:port`s that answered as HTTP/1.1 servers: .NET's `ServicePoint` sends them no
/// `Connection: Keep-Alive`. shortcut: a ServicePoint is never let go (.NET drops it after
/// 100 s idle and sends Keep-Alive again), upgrade if a proof catches it.
static HTTP11: Mutex<Option<HashSet<String>>> = Mutex::new(None);

/// `HttpWebRequest` GET with `UserAgent = "Assetto Corsa Launcher"` and the given headers, the
/// body read with `StreamReader` (UTF-8 unless a BOM says otherwise). `Err` stands for the
/// `WebException`. shortcut: redirects are not followed (.NET follows them), nothing redirects here.
fn http_get(url: &str, headers: &[(&str, &str)], decompress: bool, timeout_ms: u64) -> Result<String, String> {
    let rest = url.strip_prefix("http://").ok_or("not http")?;
    let (authority, target) = rest.find('/').map_or((rest, "/"), |i| (&rest[..i], &rest[i..]));
    let (host, port) = match authority.rsplit_once(':') {
        Some((h, p)) => (h, p.parse::<u16>().map_err(|e| e.to_string())?),
        None => (authority, 80),
    };
    let host_header = if port == 80 { host.to_string() } else { format!("{host}:{port}") };
    let timeout = Duration::from_millis(timeout_ms);
    let addr = (host, port).to_socket_addrs().map_err(|e| e.to_string())?.find(SocketAddr::is_ipv4).ok_or("no address")?;
    let mut tcp = TcpStream::connect_timeout(&addr, timeout).map_err(|e| e.to_string())?;
    tcp.set_read_timeout(Some(timeout)).map_err(|e| e.to_string())?;
    let mut head = format!("GET {target} HTTP/1.1\r\nUser-Agent: Assetto Corsa Launcher\r\n");
    for (name, value) in headers {
        head += &format!("{name}: {value}\r\n");
    }
    head += &format!("Host: {host_header}\r\n");
    if decompress {
        head += "Accept-Encoding: gzip, deflate\r\n";
    }
    let known = HTTP11.lock().unwrap().get_or_insert_with(HashSet::new).contains(&host_header);
    if !known {
        head += "Connection: Keep-Alive\r\n";
    }
    head += "\r\n";
    tcp.write_all(head.as_bytes()).map_err(|e| e.to_string())?;

    let mut raw = Vec::new();
    let mut buf = [0u8; 65536];
    let split = loop {
        if let Some(i) = raw.windows(4).position(|w| w == b"\r\n\r\n") {
            break i;
        }
        let n = tcp.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            return Err("connection closed".into());
        }
        raw.extend_from_slice(&buf[..n]);
    };
    let head = String::from_utf8_lossy(&raw[..split]).into_owned();
    let mut body = raw[split + 4..].to_vec();
    let mut lines = head.split("\r\n");
    let status = lines.next().unwrap_or("");
    let code: u16 = status.split(' ').nth(1).and_then(|c| c.parse().ok()).unwrap_or(0);
    let field = |name: &str| {
        head.split("\r\n").skip(1).find_map(|l| l.split_once(':').filter(|(n, _)| n.trim().eq_ignore_ascii_case(name)).map(|(_, v)| v.trim().to_string()))
    };
    let chunked = field("Transfer-Encoding").is_some_and(|v| v.eq_ignore_ascii_case("chunked"));
    let length = field("Content-Length").and_then(|v| v.parse::<usize>().ok());
    loop {
        if !chunked && length.is_some_and(|l| body.len() >= l) {
            break;
        }
        if chunked && body.windows(5).any(|w| w == b"0\r\n\r\n") {
            break;
        }
        let n = tcp.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        body.extend_from_slice(&buf[..n]);
    }
    if status.starts_with("HTTP/1.1") {
        HTTP11.lock().unwrap().get_or_insert_with(HashSet::new).insert(host_header);
    }
    if !(200..300).contains(&code) {
        return Err(format!("The remote server returned an error: ({code})."));
    }
    if let Some(l) = length.filter(|_| !chunked) {
        body.truncate(l);
    }
    if chunked {
        body = dechunk(&body);
    }
    if decompress {
        let mut out = Vec::new();
        match field("Content-Encoding").map(|v| v.to_ascii_lowercase()).as_deref() {
            Some("gzip") => flate2::read::GzDecoder::new(&body[..]).read_to_end(&mut out).map_err(|e| e.to_string())?,
            Some("deflate") => flate2::read::DeflateDecoder::new(&body[..]).read_to_end(&mut out).map_err(|e| e.to_string())?,
            _ => {
                out = body;
                0
            }
        };
        body = out;
    }
    Ok(net::decode(&body))
}

fn dechunk(mut b: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    while let Some(i) = b.windows(2).position(|w| w == b"\r\n") {
        let size = usize::from_str_radix(String::from_utf8_lossy(&b[..i]).split(';').next().unwrap_or("").trim(), 16).unwrap_or(0);
        if size == 0 || b.len() < i + 2 + size {
            break;
        }
        out.extend_from_slice(&b[i + 2..i + 2 + size]);
        b = &b[(i + 4 + size).min(b.len())..];
    }
    out
}

/// `DateTime.Now` in ticks: the system clock, as coarse as .NET Framework reads it.
fn now_ticks() -> i64 {
    let t = unsafe { windows::Win32::System::SystemInformation::GetSystemTimeAsFileTime() };
    ((t.dwHighDateTime as i64) << 32) | t.dwLowDateTime as i64
}

/// `pingServer`: 200 as 4 bytes to the server's UDP port, 3 bytes back (200 and its HTTP port).
fn ping_once(ip: Ipv4Addr, port: i32) -> [i32; 2] {
    let mut b = [0u8; 3];
    let (mut t0, mut t1) = (now_ticks(), now_ticks());
    if let Ok(socket) = UdpSocket::bind("0.0.0.0:0") {
        let _ = socket.set_read_timeout(Some(Duration::from_millis(2000)));
        t0 = now_ticks();
        // .NET's Receive throws when the datagram is longer than 3 bytes, after filling them.
        if u16::try_from(port).is_ok_and(|p| socket.send_to(&200i32.to_le_bytes(), (ip, p)).is_ok()) && socket.recv(&mut b).is_ok() {
            t1 = now_ticks();
        }
    }
    if b[0] == 200 && b[1] as i32 + b[2] as i32 > 0 {
        [((t1 - t0) as f64 / 10_000.0).round_ties_even() as i32, i16::from_le_bytes([b[1], b[2]]) as i32]
    } else {
        [-1, -1]
    }
}

/// `PingServer`: up to 12 tries, 150 ms apart.
fn ping_server(ip: Option<&str>, port: i32) -> Answer {
    // shortcut: dotted IPv4 only, IPAddress.Parse takes more forms, the theme sends the lobby's dotted ones
    let Some(ip) = ip.and_then(|s| s.parse::<Ipv4Addr>().ok()) else {
        return Thrown::Other(if ip.is_none() { "Value cannot be null.\r\nParameter name: ipString".into() } else { "An invalid IP address was specified.".into() }).answer();
    };
    let mut r = ping_once(ip, port);
    let mut tries = 0;
    while r[0] == -1 && tries < 11 {
        r = ping_once(ip, port);
        std::thread::sleep(Duration::from_millis(150));
        tries += 1;
    }
    Answer::json(&format!("[{},{}]", r[0], r[1]))
}

/// `GetLanServers`: on each IPv4 address of this PC (`Dns.GetHostEntry(Dns.GetHostName())`),
/// the subnet's broadcast address pinged on every port of `from..to`; each server that
/// answers is asked for its `/INFO`. The original adds them as its threads finish; the port
/// in port order.
fn lan_servers(from: i32, to: i32) -> Answer {
    let mut servers = Vec::new();
    for ip in host_addresses() {
        let Some(mask) = subnet_mask(ip) else {
            return Thrown::Other(format!("Can't find subnetmask for IP address '{ip}'")).answer();
        };
        let bcast = Ipv4Addr::from(u32::from(ip) | !u32::from(mask));
        let ports: Vec<i32> = (from..to).collect();
        let found = Mutex::new(Vec::new());
        let next = std::sync::atomic::AtomicUsize::new(0);
        std::thread::scope(|s| {
            for _ in 0..32 {
                s.spawn(|| {
                    while let Some(&port) = ports.get(next.fetch_add(1, std::sync::atomic::Ordering::Relaxed)) {
                        let text = bcast_ping(bcast, port);
                        if !text.ends_with("-1") {
                            let a: Vec<&str> = text.split(':').collect();
                            if let Ok(info) = server_info(a[0], a[2]) {
                                found.lock().unwrap().push((port, info));
                            }
                        }
                    }
                });
            }
        });
        let mut found = found.into_inner().unwrap();
        found.sort_by_key(|f| f.0);
        servers.extend(found.into_iter().map(|f| f.1));
    }
    Answer::json(&format!("[{}]", servers.join(",")))
}

/// `Dns.GetHostEntry(Dns.GetHostName()).AddressList`, the IPv4 ones.
fn host_addresses() -> Vec<Ipv4Addr> {
    use windows::Win32::System::SystemInformation::{ComputerNameDnsHostname, GetComputerNameExW};
    let mut buf = [0u16; 256];
    let mut len = buf.len() as u32;
    if unsafe { GetComputerNameExW(ComputerNameDnsHostname, Some(windows::core::PWSTR(buf.as_mut_ptr())), &mut len) }.is_err() {
        return Vec::new();
    }
    let name = String::from_utf16_lossy(&buf[..len as usize]);
    (name.as_str(), 0).to_socket_addrs().map_or(Vec::new(), |a| {
        a.filter_map(|a| match a {
            SocketAddr::V4(v) => Some(*v.ip()),
            _ => None,
        })
        .collect()
    })
}

/// `GetSubnetMask`: the mask of the network interface with that address.
fn subnet_mask(ip: Ipv4Addr) -> Option<Ipv4Addr> {
    use windows::Win32::NetworkManagement::IpHelper::{GetAdaptersAddresses, GAA_FLAG_INCLUDE_GATEWAYS, IP_ADAPTER_ADDRESSES_LH};
    use windows::Win32::Networking::WinSock::{AF_INET, AF_UNSPEC, SOCKADDR_IN};
    let mut size = 32_768u32;
    let mut buf: Vec<u64>;
    loop {
        buf = vec![0u64; size as usize / 8 + 1];
        let r = unsafe { GetAdaptersAddresses(AF_UNSPEC.0 as u32, GAA_FLAG_INCLUDE_GATEWAYS, None, Some(buf.as_mut_ptr().cast()), &mut size) };
        if r == windows::Win32::Foundation::ERROR_BUFFER_OVERFLOW.0 {
            continue;
        }
        if r != 0 {
            return None;
        }
        break;
    }
    let mut adapter = buf.as_ptr() as *const IP_ADAPTER_ADDRESSES_LH;
    while let Some(a) = unsafe { adapter.as_ref() } {
        let mut unicast = a.FirstUnicastAddress;
        while let Some(u) = unsafe { unicast.as_ref() } {
            let sa = u.Address.lpSockaddr;
            if !sa.is_null() && unsafe { (*sa).sa_family } == AF_INET {
                let sin = unsafe { &*(sa as *const SOCKADDR_IN) };
                if Ipv4Addr::from(u32::from_be(unsafe { sin.sin_addr.S_un.S_addr })) == ip {
                    let len = u.OnLinkPrefixLength as u32;
                    return Some(Ipv4Addr::from(if len == 0 { 0 } else { u32::MAX << (32 - len.min(32)) }));
                }
            }
            unicast = u.Next;
        }
        adapter = a.Next;
    }
    None
}

/// `BCastPing`: 200 sent to the broadcast address with `DontRoute` (no `SO_BROADCAST`, as the
/// original), 50 ms for an answer; "ip:port:httpport", or ending in -1 without one.
fn bcast_ping(bcast: Ipv4Addr, port: i32) -> String {
    use std::os::windows::io::AsRawSocket;
    use windows::Win32::Networking::WinSock::{sendto, setsockopt, IPPROTO_UDP, MSG_DONTROUTE, SOCKADDR, SOCKADDR_IN, SOCKET};
    let mut from = String::from("0.0.0.0:0");
    let mut b = [0u8; 256];
    if let (Ok(socket), Ok(port16)) = (UdpSocket::bind("0.0.0.0:0"), u16::try_from(port)) {
        let raw = SOCKET(socket.as_raw_socket() as usize);
        // SocketOptionName.Debug at the UDP level: option 1, UDP_NOCHECKSUM.
        unsafe { setsockopt(raw, IPPROTO_UDP.0, 1, Some(&1i32.to_le_bytes())) };
        let mut to = SOCKADDR_IN { sin_family: windows::Win32::Networking::WinSock::AF_INET, sin_port: port16.to_be(), ..Default::default() };
        to.sin_addr.S_un.S_addr = u32::from(bcast).to_be();
        let sent = unsafe { sendto(raw, &200i32.to_le_bytes(), MSG_DONTROUTE.0, &to as *const SOCKADDR_IN as *const SOCKADDR, size_of::<SOCKADDR_IN>() as i32) };
        if sent >= 0 {
            let _ = socket.set_read_timeout(Some(Duration::from_millis(50)));
            if let Ok((_, addr)) = socket.recv_from(&mut b) {
                from = addr.to_string();
            }
        }
    }
    let v = if b[0] == 200 && b[1] as i32 + b[2] as i32 > 0 { i16::from_le_bytes([b[1], b[2]]) as i32 } else { -1 };
    format!("{from}:{v}")
}

/// `GetServerInfo`: the server's `/INFO`, with its address put in and marked as a LAN one.
fn server_info(ip: &str, port: &str) -> Result<String, String> {
    let text = http_get(&format!("http://{ip}:{port}/INFO"), &[], false, 30_000)?;
    Ok(text.replace("\"ip\":\"\"", &format!("\"ip\":\"{ip}\"")).replace("\"l\":false", "\"l\":true"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_and_chunks() {
        // ACUtility.Hash("abc"): MD5, upper-case hex
        assert_eq!(ac_hash("abc"), "900150983CD24FB0D6963F7D28E17F72");
        assert_eq!(dechunk(b"4\r\nWiki\r\n5\r\npedia\r\n0\r\n\r\n"), b"Wikipedia");
        assert_eq!(server_list_cached(Some("[{\"a\":1,\"b\":2}]")).body, b"2");
        assert_eq!(server_list_cached(Some("[]")).body, Thrown::ArgumentOutOfRange.answer().body);
    }
}
