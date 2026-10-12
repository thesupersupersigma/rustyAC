// SPDX-License-Identifier: GPL-3.0-or-later

//! Joining a server from the desktop: what `RaceManager::initOnline` @ 0x14013d160 does
//! before the track is loaded. The player's identity, the TCP connection, the handshake, the
//! car list, the checksums the server asks for.
//!
//! Where the server is comes from the command line (`--join host:port`) or, for a start
//! without a car and a track on the command line, from race.ini's `[REMOTE]` section, which
//! is how Assetto Corsa itself and Content Manager's "join" start an online session.

use std::path::{Path, PathBuf};

use rustyac_net::client::Client;
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
}

/// `host:port` of `--join`.
pub fn parse_address(text: &str) -> Result<(String, u16), String> {
    let (host, port) = text.rsplit_once(':').ok_or(format!("--join {text}: <host>:<port> is expected (the server's TCP port, e.g. 127.0.0.1:9600)"))?;
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

/// A name a server sent that becomes part of a path: one folder's name, nothing that leaves
/// the folder it is joined to.
pub fn is_plain_name(name: &str) -> bool {
    !name.is_empty() && name != "." && name != ".." && !name.contains(['/', '\\', ':', '\0']) && !name.bytes().any(|b| b < 0x20)
}

fn read_game_file(root: &Path, path: &str) -> Option<Vec<u8>> {
    // the server's paths have forward slashes and never leave the game's folder: every
    // part is a plain name (no drive, no root, no "..")
    if path.is_empty() || !path.split(['/', '\\']).all(is_plain_name) {
        return None;
    }
    std::fs::read(root.join(path)).ok()
}

/// `ACClient::getCarMD5` @ 0x140046070: the MD5 of `content/cars/<model>/data.acd`, or,
/// for a car with an unpacked `data` folder, of all its files one after the other in the
/// order of their lower-cased names (`md5Folder` @ 0x14023a450).
pub fn car_md5(root: &Path, model: &str) -> [u8; 16] {
    let car = root.join("content").join("cars").join(model);
    if let Ok(bytes) = std::fs::read(car.join("data.acd")) {
        return rustyac_net::md5::digest(&bytes);
    }
    let mut names: Vec<(Vec<u16>, PathBuf)> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(car.join("data")) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() {
                let lower: Vec<u16> = entry.file_name().to_string_lossy().to_lowercase().encode_utf16().collect();
                names.push((lower, path));
            }
        }
    }
    names.sort();
    let mut md5 = rustyac_net::md5::Md5::new();
    for (_, path) in &names {
        if let Ok(bytes) = std::fs::read(path) {
            md5.update(&bytes);
        }
    }
    md5.finish()
}

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
    notes.push(format!("online: joining {}:{} as {:?}{}", target.host, target.port, request.name, if target.server_name.is_empty() { String::new() } else { format!(" ({})", target.server_name) }));
    let transport = NativeTransport::connect(&target.host, target.port, 8000)?;
    let transport: BoxedTransport = if options.net_tape.is_some() { Box::new(Taped::new(transport)) } else { Box::new(transport) };
    let mut client = Client::join(transport, &request).map_err(|e| e.to_string())?;
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
    // from here on the server holds a slot for this player: a failure says goodbye (0x43)
    let rest = (|| -> Result<Vec<CarEntry>, String> {
        let cars = client.car_list()?;
        // what becomes a folder's name is a plain name
        let named = [("track", &accepted.track), ("car", &accepted.model)].into_iter().chain(cars.iter().map(|car| ("car", &car.model)));
        for (what, name) in named {
            if !is_plain_name(name) {
                return Err(format!("the server names a {what} {name:?}: that is no folder's name"));
            }
        }
        if !accepted.track_config.is_empty() && !is_plain_name(&accepted.track_config) {
            return Err(format!("the server names a track layout {:?}: that is no folder's name", accepted.track_config));
        }
        // the checksums: every file the server names must be there ("Problems during checksum")
        let root = root.ok_or("the server asks for checksums of the game's files: Assetto Corsa's folder was not found (set AC_ROOT)")?;
        let mut digests = Vec::new();
        for file in &accepted.checksum_files {
            let bytes = read_game_file(&root, file).ok_or(format!("PROBLEMS DURING CHECKSUM: {file} is not in {}", root.display()))?;
            digests.push(rustyac_net::md5::digest(&bytes));
        }
        client.set_checksums(digests, car_md5(&root, &accepted.model));
        Ok(cars)
    })();
    let cars = match rest {
        Ok(cars) => cars,
        Err(message) => {
            client.shutdown();
            return Err(message);
        }
    };
    let session = OnlineSession::new(client, &cars);
    let others = session.remote_entries.iter().filter(|car| !car.driver_name.is_empty()).count();
    notes.push(format!("online: {} cars on the server, {} other driver(s) there now", cars.len(), others));
    Ok(Joined { session, cars, notes })
}

/// Writes the tape of a session (`--net-tape`).
pub fn save_tape(transport: &dyn Transport, path: &Path) -> Result<usize, String> {
    let tape = transport.tape().ok_or("this session kept no tape")?;
    std::fs::write(path, tape.to_bytes()).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(tape.entries.len())
}
