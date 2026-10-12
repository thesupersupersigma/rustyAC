// SPDX-License-Identifier: GPL-3.0-or-later

//! Joining a server from the desktop: what `RaceManager::initOnline` @ 0x14013d160 does
//! before the track is loaded. The player's identity, the connection (TCP and UDP, or a
//! WebSocket to a gateway next to the server), the handshake, the car list, the checksums
//! the server asks for.
//!
//! Where the server is comes from the command line (`--join host:port`) or, for a start
//! without a car and a track on the command line, from race.ini's `[REMOTE]` section, which
//! is how Assetto Corsa itself and Content Manager's "join" start an online session.

use std::path::Path;

use rustyac_net::client::{Join, JOIN_TIMEOUT_MS};
use rustyac_net::handshake::{CarEntry, JoinRequest};
use rustyac_net::native::NativeTransport;
use rustyac_net::transport::{Taped, Transport};
use rustyac_physics::data::ini::IniReader;

use crate::cli::Options;
use crate::identity::{self, GuidSource};
use crate::online::{BoxedTransport, OnlineSession};

/// Where and as whom to join.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct JoinTarget {
    pub host: String,
    pub port: u16,
    pub name: String,
    pub team: String,
    pub nation_code: String,
    pub car: String,
    pub password: String,
    /// The server's name as a launcher wrote it (for the console only).
    pub server_name: String,
    /// `--join ws://...` / `wss://...`: the address of a gateway next to the server
    /// (`crates/rustyac-gateway`) instead of the server's own `host:port`.
    pub gateway: Option<String>,
}

/// `host:port` of `--join`.
pub fn parse_address(text: &str) -> Result<(String, u16), String> {
    let (host, port) = text.rsplit_once(':').ok_or(format!("--join {text}: <host>:<port> is expected (the server's TCP port, e.g. 127.0.0.1:9600), or a gateway's ws:// or wss:// address"))?;
    let port = port.parse().map_err(|_| format!("--join {text}: {port:?} is no port number"))?;
    if host.is_empty() {
        return Err(format!("--join {text}: the host is missing"));
    }
    Ok((host.to_string(), port))
}

/// `[REMOTE]` of a race.ini with `ACTIVE=1` (the keys `ACClient::handshakeTCP` reads:
/// `SERVER_IP, SERVER_PORT, NAME, TEAM, REQUESTED_CAR, PASSWORD`, and `[CAR_0]
/// NATION_CODE`). `GUID`, which launchers also write there, is not read: the game takes
/// the player's id from Steam, and so does rustyAC ([`crate::identity`]).
pub fn remote_of_race_ini(path: &Path) -> Result<Option<JoinTarget>, String> {
    if !path.is_file() {
        return Ok(None);
    }
    let ini = IniReader::load(path)?;
    if ini.get_int("REMOTE", "ACTIVE").unwrap_or(0) == 0 {
        return Ok(None);
    }
    let host = ini.get_string("REMOTE", "SERVER_IP");
    let port = ini.get_int("REMOTE", "SERVER_PORT").unwrap_or(0);
    if host.is_empty() || !(1..=65535).contains(&port) {
        return Err(format!("{}: [REMOTE] ACTIVE=1 without a usable SERVER_IP and SERVER_PORT", path.display()));
    }
    Ok(Some(JoinTarget {
        host,
        port: port as u16,
        name: ini.get_string("REMOTE", "NAME"),
        team: ini.get_string("REMOTE", "TEAM"),
        nation_code: ini.get_string("CAR_0", "NATION_CODE"),
        car: ini.get_string("REMOTE", "REQUESTED_CAR"),
        password: ini.get_string("REMOTE", "PASSWORD"),
        server_name: ini.get_string("REMOTE", "SERVER_NAME"),
        gateway: None,
    }))
}

/// Whom to join, if anybody: `--join`, else race.ini's `[REMOTE]` when the command line
/// names neither a car nor a track nor a recording (a launcher's start) and does not say
/// `--offline` or `--no-race-ini`.
pub fn target(options: &Options) -> Result<Option<JoinTarget>, String> {
    if options.offline {
        return Ok(None);
    }
    let mut target = match &options.join {
        Some(address) if rustyac_net::native::is_gateway_url(address) => {
            let url = rustyac_net::native::WsUrl::parse(address)?;
            Some(JoinTarget { host: url.host.clone(), port: url.port, gateway: Some(address.clone()), ..JoinTarget::default() })
        }
        Some(address) => {
            let (host, port) = parse_address(address)?;
            Some(JoinTarget { host, port, ..JoinTarget::default() })
        }
        None => {
            let launcher_start = !options.car_given && options.track.is_none() && options.replay.is_none() && options.race_ini != Some(false);
            let path = options.race_ini_file.clone().or_else(crate::conditions::race_ini_path);
            match (launcher_start, path) {
                (true, Some(path)) => remote_of_race_ini(&path)?,
                _ => None,
            }
        }
    };
    if let Some(target) = &mut target {
        // the command line on top
        if let Some(name) = &options.driver_name {
            target.name = name.clone();
        }
        if target.name.is_empty() {
            target.name = identity::default_name();
        }
        if let Some(password) = &options.password {
            target.password = password.clone();
        }
        if options.car_given {
            target.car = options.car.clone();
        }
        if let Some(team) = &options.team {
            target.team = team.clone();
        }
    }
    Ok(target)
}

pub use crate::online::{car_md5, is_plain_name};

/// What a join gives: the session (the client with the car list) and what was learnt.
pub struct Joined {
    pub session: OnlineSession,
    pub cars: Vec<CarEntry>,
    pub notes: Vec<String>,
}

/// Connects, shakes hands, asks for the cars and prepares the checksums.
pub fn join(options: &Options, target: &JoinTarget) -> Result<Joined, String> {
    let root = rustyac_content::install::ac_root();
    let mut notes = Vec::new();
    let identity = identity::resolve(options.guid.as_deref(), !options.no_steam, root.as_deref(), options.net_identity.as_deref())?;
    notes.push(match &identity.source {
        GuidSource::Steam => "online: your player id is your Steam id (asked of Steam through the game's own steam_api64.dll)".to_string(),
        GuidSource::Own(file) => format!("online: Steam is not running (or was not asked): your player id is rustyAC's own, kept in {}", file.display()),
        GuidSource::Given => "online: the player id of the command line".to_string(),
    });
    let request = JoinRequest { guid: identity.guid, name: target.name.clone(), team: target.team.clone(), nation_code: target.nation_code.clone(), car: target.car.clone(), password: target.password.clone() };
    notes.push(format!("online: joining {} as {:?}{}", target.gateway.clone().unwrap_or(format!("{}:{}", target.host, target.port)), request.name, if target.server_name.is_empty() { String::new() } else { format!(" ({})", target.server_name) }));
    let transport: BoxedTransport = match &target.gateway {
        // through a gateway: both channels in one WebSocket
        Some(url) => crate::gateway::connect(url, &options.ws_headers, options.tls_root.as_deref(), 8000)?,
        None => Box::new(NativeTransport::connect(&target.host, target.port, 8000)?),
    };
    let transport: BoxedTransport = if options.net_tape.is_some() { Box::new(Taped::new(transport)) } else { transport };
    let transport: BoxedTransport = match &options.net_arrivals {
        Some(file) => Box::new(Arrivals { inner: transport, file: file.clone(), rows: Vec::new() }),
        None => transport,
    };
    // the handshake and the car list: the same state machine the browser steps, here
    // waiting for each answer
    let mut join = Join::start(transport, &request).map_err(|e| e.to_string())?;
    join.wait(JOIN_TIMEOUT_MS).map_err(|e| e.to_string())?;
    let (client, cars) = join.finish().ok_or("the join did not end")?;
    let accepted = client.accepted.clone();
    notes.push(format!(
        "online: {:?} lets you in: slot {}, {} ({}) at {}{}, session {:?}, {} updates a second",
        accepted.server_name,
        accepted.session_id,
        accepted.model,
        accepted.skin,
        accepted.track,
        if accepted.track_config.is_empty() { String::new() } else { format!(" ({})", accepted.track_config) },
        accepted.current_session.name,
        accepted.send_hz
    ));
    let level = |v: u8| ["no", "as the car has it", "yes"].get(v as usize).copied().unwrap_or("?");
    notes.push(format!(
        "  the server's rules: ABS {}, traction control {}, stability control {}, automatic clutch {}, damage {:.0} %, fuel {:.0} %, tyre wear {:.0} %, tyre blankets {}, tyres out {}, grip {:.1} %",
        level(accepted.abs),
        level(accepted.tc),
        if accepted.stability { "allowed" } else { "not allowed" },
        if accepted.auto_clutch { "allowed" } else { "not allowed" },
        accepted.damage_rate * 100.0,
        accepted.fuel_rate * 100.0,
        accepted.tyre_wear_rate * 100.0,
        if accepted.tyre_blankets { "yes" } else { "no" },
        accepted.allowed_tyres_out,
        accepted.current_session.grip * 100.0
    ));
    let session = crate::online::finish_join(client, cars.clone(), root.as_deref())?;
    let others = session.remote_entries.iter().filter(|car| !car.driver_name.is_empty()).count();
    notes.push(format!("online: {} cars on the server, {} other driver(s) there now", cars.len(), others));
    Ok(Joined { session, cars, notes })
}

/// `--net-arrivals <file.csv>`: a transport that notes when every state of another car
/// arrived (the wall clock, milliseconds since 1970) and writes the list when the session
/// ends. With the same list of another client of the same session it tells how much later
/// one transport delivers than the other (`docs/port/multiplayer_web.md`).
struct Arrivals {
    inner: BoxedTransport,
    file: std::path::PathBuf,
    rows: Vec<(f64, u8, u8, [u32; 3])>,
}

impl Transport for Arrivals {
    fn send_reliable(&mut self, message: &[u8]) -> Result<(), String> {
        self.inner.send_reliable(message)
    }
    fn send_datagram(&mut self, message: &[u8]) -> Result<(), String> {
        self.inner.send_datagram(message)
    }
    fn poll_reliable(&mut self) -> Result<Option<Vec<u8>>, String> {
        self.inner.poll_reliable()
    }
    fn poll_datagram(&mut self) -> Result<Option<Vec<u8>>, String> {
        let datagram = self.inner.poll_datagram()?;
        if let Some(datagram) = datagram.as_ref().filter(|d| matches!(d.first(), Some(0x46 | 0x48)) && self.rows.len() < 2_000_000) {
            let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0.0, |d| d.as_secs_f64() * 1000.0);
            self.rows.extend(rustyac_net::datagram::car_state_keys(datagram).into_iter().map(|(slot, sequence, position)| (now, slot, sequence, position)));
        }
        Ok(datagram)
    }
    fn wait_reliable(&mut self, timeout_ms: u32) -> Result<Option<Vec<u8>>, String> {
        self.inner.wait_reliable(timeout_ms)
    }
    fn set_datagram_port(&mut self, port: u16) {
        self.inner.set_datagram_port(port)
    }
    fn set_tick(&mut self, tick: u64) {
        self.inner.set_tick(tick)
    }
    fn tape(&self) -> Option<&rustyac_net::transport::Tape> {
        self.inner.tape()
    }
}

impl Drop for Arrivals {
    fn drop(&mut self) {
        let mut text = String::from("arrived_ms,taken_ms,slot,sequence,x,y,z\n");
        for (at, slot, sequence, position) in &self.rows {
            text.push_str(&format!("{at:.3},{at:.3},{slot},{sequence},{:08x},{:08x},{:08x}\n", position[0], position[1], position[2]));
        }
        match std::fs::write(&self.file, text) {
            Ok(()) => println!("online: {} arrivals of car states are in {}", self.rows.len(), self.file.display()),
            Err(e) => eprintln!("WARNING: {}: {e}", self.file.display()),
        }
    }
}

/// Writes the tape of a session (`--net-tape`).
pub fn save_tape(transport: &dyn Transport, path: &Path) -> Result<usize, String> {
    let tape = transport.tape().ok_or("this session kept no tape")?;
    std::fs::write(path, tape.to_bytes()).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(tape.entries.len())
}
