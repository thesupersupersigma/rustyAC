// SPDX-License-Identifier: GPL-3.0-or-later

//! An online session inside the game: the client of a server ([`rustyac_net::client`]),
//! the other players' cars ([`rustyac_net::net_car`]) as bodies of the physics engine, and
//! what the displays show of both.
//!
//! Everything here runs on the physics thread, where Assetto Corsa runs it: the client's
//! work after every step (`ACClient::onPhysicsStep`) before the local car's `Car::postStep`,
//! the remote cars' steps (`NetCarStateProvider::step`) after it. See
//! [`rustyac_physics::engine::StepHooks`].
//!
//! No platform code: the transport is a trait object (TCP and UDP on the desktop, or a
//! WebSocket to a gateway; the page's WebSocket in the browser build), and the files the
//! server wants checksums of are read through `rustyac_content::vfs`.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};

use rustyac_net::client::{Client, Event};
use rustyac_net::handshake::{session_type, Accepted, CarEntry};
use rustyac_net::messages::Message;
use rustyac_net::net_car::{NetCarGeometry, NetCarStateProvider};
use rustyac_net::session::NetSession;
use rustyac_net::transport::Transport;
use rustyac_content::vfs;
use rustyac_physics::car::{PhysicsCore, RollingChassis};
use rustyac_physics::engine::{CarSlot, StepHooks};
use rustyac_physics::vecmath::Mat44f;

/// The transport of a session: any, behind a box. (On the desktop the session lives on
/// the physics thread; a browser has one thread, and its WebSocket cannot leave it.)
#[cfg(not(target_arch = "wasm32"))]
pub type BoxedTransport = Box<dyn Transport + Send>;
#[cfg(target_arch = "wasm32")]
pub type BoxedTransport = Box<dyn Transport>;

/// A name a server sent that becomes part of a path: one folder's name, nothing that leaves
/// the folder it is joined to.
pub fn is_plain_name(name: &str) -> bool {
    // (no separators, no drive, no wildcard or pipe characters, no control bytes, no dot or
    // space at the end, and none of the names Windows keeps for devices: `CON` read as a
    // file waits for the keyboard)
    let stem = name.split('.').next().unwrap_or("").trim_end().to_ascii_uppercase();
    let device = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL") || (stem.len() == 4 && (stem.starts_with("COM") || stem.starts_with("LPT")) && stem.as_bytes()[3].is_ascii_digit());
    !name.is_empty() && name != "." && name != ".." && !name.contains(['/', '\\', ':', '\0', '<', '>', '"', '|', '?', '*']) && !name.bytes().any(|b| b < 0x20) && !name.ends_with(['.', ' ']) && !device
}

fn read_game_file(root: &Path, path: &str) -> Option<Vec<u8>> {
    // the server's paths have forward slashes and never leave the game's folder: every
    // part is a plain name (no drive, no root, no "..")
    if path.is_empty() || !path.split(['/', '\\']).all(is_plain_name) {
        return None;
    }
    vfs::read(&root.join(path)).ok()
}

/// `ACClient::getCarMD5` @ 0x140046070: the MD5 of `content/cars/<model>/data.acd`, or,
/// for a car with an unpacked `data` folder, of all its files one after the other in the
/// order of their lower-cased names (`md5Folder` @ 0x14023a450).
pub fn car_md5(root: &Path, model: &str) -> [u8; 16] {
    let car = root.join("content").join("cars").join(model);
    if let Ok(bytes) = vfs::read(&car.join("data.acd")) {
        return rustyac_net::md5::digest(&bytes);
    }
    let folder = car.join("data");
    let mut names: Vec<(Vec<u16>, PathBuf)> = Vec::new();
    if let Ok(entries) = vfs::read_dir(&folder) {
        for (name, is_folder) in entries {
            if !is_folder {
                names.push((name.to_lowercase().encode_utf16().collect(), folder.join(&name)));
            }
        }
    }
    names.sort();
    let mut md5 = rustyac_net::md5::Md5::new();
    for (_, path) in &names {
        if let Ok(bytes) = vfs::read(path) {
            md5.update(&bytes);
        }
    }
    md5.finish()
}

/// What `RaceManager::initOnline` @ 0x14013d160 does with a finished join
/// ([`rustyac_net::client::Join`]), on the desktop and in the browser alike: the names the
/// server sent that become folders are checked, the files it wants checksums of are read
/// from the game's folder `root` (the desktop's disk, the browser's own files) and
/// digested, and the session is made. From the handshake on the server holds a slot for
/// this player: a failure says goodbye (`0x43`).
pub fn finish_join(mut client: Client<BoxedTransport>, cars: Vec<CarEntry>, root: Option<&Path>) -> Result<OnlineSession, String> {
    let accepted = client.accepted.clone();
    let rest = (|| -> Result<(), String> {
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
        // (a skin is a folder below the car's; a slot may have none)
        for skin in std::iter::once(&accepted.skin).chain(cars.iter().map(|car| &car.skin)) {
            if !skin.is_empty() && !is_plain_name(skin) {
                return Err(format!("the server names a skin {skin:?}: that is no folder's name"));
            }
        }
        // the checksums: every file the server names must be there ("Problems during checksum")
        let root = root.ok_or("the server asks for checksums of the game's files: Assetto Corsa's folder was not found (set AC_ROOT)")?;
        let mut digests = Vec::new();
        for file in &accepted.checksum_files {
            let bytes = read_game_file(root, file).ok_or(format!("PROBLEMS DURING CHECKSUM: {file} is not in {}", root.display()))?;
            digests.push(rustyac_net::md5::digest(&bytes));
        }
        client.set_checksums(digests, car_md5(root, &accepted.model));
        Ok(())
    })();
    if let Err(message) = rest {
        client.shutdown();
        return Err(message);
    }
    Ok(OnlineSession::new(client, &cars))
}

/// What the server dictates of the physics (the handshake's reply), as the game applies it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ServerRules {
    /// -1: no limit.
    pub allowed_tyres_out: i32,
    pub tyre_blankets: bool,
    /// 0 off, 1 the car's own, 2 for everybody.
    pub tc: i32,
    pub abs: i32,
    pub stability: bool,
    pub auto_clutch: bool,
    pub jump_start_mode: i32,
    pub damage_rate: f32,
    pub fuel_rate: f32,
    pub tyre_wear_rate: f32,
    /// The session: its kind (1 practice, 2 qualifying, 3 race) and the track's grip.
    pub session_type: i32,
    pub grip: f32,
    pub gas_penalty_disabled: bool,
}

impl ServerRules {
    pub fn of(accepted: &Accepted) -> ServerRules {
        ServerRules {
            allowed_tyres_out: accepted.allowed_tyres_out as i32,
            tyre_blankets: accepted.tyre_blankets,
            tc: accepted.tc as i32,
            abs: accepted.abs as i32,
            stability: accepted.stability,
            auto_clutch: accepted.auto_clutch,
            jump_start_mode: accepted.jump_start_mode as i32,
            damage_rate: accepted.damage_rate,
            fuel_rate: accepted.fuel_rate,
            tyre_wear_rate: accepted.tyre_wear_rate,
            session_type: accepted.current_session.kind as i32,
            grip: accepted.current_session.grip,
            gas_penalty_disabled: accepted.gas_penalty_disabled,
        }
    }

    /// `engine.penaltyMode` of a session (the job `ACClient::onNewSession` queues): losing
    /// the lap in practice and qualifying; in a race the cut-the-throttle penalty, or only
    /// the detection when the server switched that off.
    pub fn penalty_mode(&self) -> i32 {
        if self.session_type == session_type::RACE as i32 {
            if self.gas_penalty_disabled {
                4
            } else {
                0
            }
        } else {
            1
        }
    }
}

/// One slot of the server, for the player list.
/// What an online session draws over everything (`ACClient::renderHUD`, `QOSOverlay`): the
/// ping to the server, the server's welcome message, the message of the countdown of a
/// driver who was put out.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct OnlineGui {
    pub ping: i32,
    pub welcome: String,
    pub disconnect: Option<String>,
    /// Task 34: what the ESC menu's page "Race control" reads of `ACClient` and of the rules
    /// the server wrote into the physics
    pub server_name: String,
    pub rules: Option<ServerRules>,
    pub inverted_grid_positions: i32,
    /// `ACClient::sessionID`; `netCars`: each car's session id, guid (its index among the
    /// cars) and driver
    pub session_id: u8,
    pub net_cars: Vec<(u8, i32, String)>,
}

/// Task 34: what the client heard that the GUI acts on at once (in arrival order).
#[derive(Clone, Debug, PartialEq)]
pub enum OnlineGuiEvent {
    /// `ACClient::evOnChatMessage` {session id, message}
    Chat { session: u8, text: String },
    /// `ACClient::evOnVoteReceived` (a kick vote against the player is not passed on)
    /// (`target`: the car of the target's session id at that moment, its driver and guid)
    Vote { kind: i32, target: Option<(String, i32)>, time_left_ms: i32, is_me: bool },
    /// `ACClient::evOnVoteNotPassed`
    VoteNotPassed,
    /// `SystemMessage::addMessage(title, description, type)` of the client's messages
    Message { title: String, text: String, kind: i32 },
    /// `ACClient::onSetupReceived` 0x140050090: the server's setup (its first byte unread)
    Setup { values: Vec<(String, f32)> },
    /// Task 42b: the race is over and the car went to its pit box (lambda @ 0x14003dde0):
    /// the ESC menu shows, its "Drive" off until the next session
    SentToPits,
    /// Task 42b: a new session (the Sim's handler @ 0x140196830): "Drive" on again
    NewSession,
}

/// Task 34: what the GUI asks of the client.
#[derive(Clone, Debug, PartialEq)]
pub enum OnlineRequest {
    /// `ACClient::sendChat` (`now`: the game's clock, ms)
    Chat { text: String, now: f64 },
    /// `ACClient::sendVote` (`guid`: the car's, -1 for none)
    Vote { kind: i32, yes: bool, guid: i32 },
}

#[derive(Clone, Debug, PartialEq)]
pub struct RosterEntry {
    pub slot: u8,
    pub model: String,
    pub driver: String,
    /// The player's own slot.
    pub me: bool,
    pub connected: bool,
    pub ping: i32,
    pub laps: u32,
    pub last_lap_ms: u32,
    pub best_lap_ms: u32,
}

/// Another player's car: what the game keeps beside its `NetCarStateProvider`
/// (`net.cars`, same index).
pub struct RemoteCar {
    pub entry: CarEntry,
    pub data_path: PathBuf,
    /// `Car::damageZoneLevel` as the server tells it.
    pub damage: [f32; 5],
}

/// The online session of a [`crate::sim::GameSim`].
pub struct OnlineSession {
    /// The client and the other players' cars: the engine's step-completed handlers.
    pub net: NetSession<BoxedTransport>,
    /// The other slots' cars in the order of the server's list (spectators left out).
    pub remote: Vec<RemoteCar>,
    pub roster: Vec<RosterEntry>,
    /// The last lines of the chat and of the server's notices, newest last.
    pub chat: VecDeque<String>,
    /// Lines for the console since they were last taken.
    pub console: Vec<String>,
    /// What the game has to act on, in arrival order.
    pub pending: Vec<Event>,
    /// `ACClient::welcomeMessage`, `ACClient::dcCountdown.message` (once it is signaled)
    pub welcome: String,
    pub disconnect: Option<String>,
    /// The cars that become remote cars when the world is built (then empty).
    pub remote_entries: Vec<CarEntry>,
    /// `damageReport`: the damage the server was last told of, and when (ms).
    damage_sent: [f32; 5],
    damage_sent_at: f64,
    /// The local car's jump starts the server was told of.
    jump_starts_sent: u32,
    /// The local car's tyre compound the server was told of.
    compound_sent: Option<String>,
    /// The last session-start line said (the server repeats the message).
    last_start_line: String,
    /// Task 28: the server's session as the game's manager mirrors it
    /// (`RaceManager::convertRemoteSession`), the server's standings (with every lap
    /// message), and its results once the race is over (message 0x4b).
    pub session: rustyac_physics::race::Session,
    pub standings: Vec<rustyac_net::messages::LeaderboardRow>,
    pub results: Option<Vec<rustyac_net::messages::LeaderboardRow>>,
    /// the clock of the last step, ms
    pub now: f64,
    /// Task 28b: the player's pit stop (made once the car is there; `pit_failed`: the car's
    /// menu could not be made)
    pub pit: Option<OnlinePit>,
    pub pit_failed: bool,
    /// Task 34: what the GUI hears of (taken every step)
    pub gui_events: Vec<OnlineGuiEvent>,
    /// Task 42b: `ACClient::isResultScreenOn` (the race's results came) and
    /// `endSession.sendToPitsTimer` (5 s at the join, 0 after a new session), seconds
    result_screen_on: bool,
    send_to_pits_timer: f32,
}

/// `system/cfg/pitstop.ini` as the game ships it (a server session reads no file here).
pub const PIT_SETTINGS: rustyac_physics::race::QuickMenuSettings = rustyac_physics::race::QuickMenuSettings { stay_in_car: false, auto_app_on_pitlane: true, presets_count: 3, visibility_max_time: 5.0, cursor_start: 0 };

/// Task 28b: the player's pit stop on a server. `PitStop`, the pit menu and the applying job
/// are the client's own there too (no message asks the server for a stop); what a server
/// adds is the mandatory pit window (two numbers of its join reply) and message 0x0e: the
/// client tells that its stop was counted (`RaceManager::checkMandatoryPit`), the server
/// tells every client which cars have done theirs.
pub struct OnlinePit {
    pub pit: rustyac_physics::race::PitStop,
    pub menu: rustyac_physics::race::QuickMenu,
    pub avatar: rustyac_physics::race::PitAvatar,
    /// `ACClient::pitWindowStart` / `pitWindowEnd`
    pub window: (u16, u16),
    /// the manager's two window times, and the session they are measured in
    manager: rustyac_physics::race::RaceManager,
    rand: rustyac_physics::session::MsvcRand,
    step_in_frame: u32,
    /// a new session began since the last frame
    new_session: bool,
    /// the mouse menu's stop turned mouse steering on or off
    pub mouse_steering: Option<bool>,
}

impl OnlinePit {
    pub fn new(car: &RollingChassis, settings: &rustyac_physics::race::QuickMenuSettings, window: (u16, u16)) -> Result<OnlinePit, String> {
        let menu = rustyac_physics::race::QuickMenu::for_car(car, settings)?;
        let m = &car.pit_position.m;
        let mut pit = rustyac_physics::race::PitStop::new([m[3][0], m[3][1], m[3][2]]);
        pit.state = rustyac_physics::race::pitstop::STATE_UNDEFINED;
        let sessions = rustyac_physics::race::SessionsIni::from_list("p1", 1, false, None, 3)?;
        Ok(OnlinePit { pit, menu, avatar: Default::default(), window, manager: rustyac_physics::race::RaceManager::new(&sessions, 1, 0, 1.5), rand: rustyac_physics::session::MsvcRand(28), step_in_frame: 0, new_session: false, mouse_steering: None })
    }

    /// The pit lines of the HUD (the menu, the countdown, the window).
    pub fn lines(&self) -> Vec<String> {
        crate::referee::pit_lines_of(&self.pit, &self.menu, self.window, &self.avatar, &self.manager).all()
    }
}

/// `RaceManager::convertRemoteSession` for a session as the server's messages carry it
/// (`start`: its start on this machine's clock, ms).
fn mirror_session(info: &rustyac_net::handshake::SessionInfo, start: f64) -> rustyac_physics::race::Session {
    rustyac_physics::race::convert_remote_session(&rustyac_physics::race::RemoteSession {
        name: info.name.clone(),
        session_type: info.kind as i32,
        index: info.index as i32,
        time: info.time as i32,
        laps: info.laps as i32,
        start_time: start,
        is_spectator: false,
        is_timed_race: info.kind == session_type::RACE && info.laps == 0,
        has_extra_lap: false,
    })
}

/// How many chat lines are kept.
const CHAT_LINES: usize = 6;

impl OnlineSession {
    /// A session after the handshake and the car list (`cars`: every entry of the list).
    pub fn new(client: Client<BoxedTransport>, cars: &[CarEntry]) -> OnlineSession {
        let me = client.session_id;
        let roster = cars
            .iter()
            .map(|car| RosterEntry {
                slot: car.session_id,
                model: car.model.clone(),
                driver: car.driver_name.clone(),
                me: car.session_id == me,
                connected: car.session_id == me || !car.driver_name.is_empty(),
                ping: 0,
                laps: 0,
                last_lap_ms: 0,
                best_lap_ms: 0,
            })
            .collect();
        let remote_entries = OnlineSession::remote_entries(cars, me);
        let session = mirror_session(&client.current_session, client.session_start_time);
        OnlineSession { net: NetSession::new(client), remote: Vec::new(), roster, chat: VecDeque::new(), console: Vec::new(), pending: Vec::new(), welcome: String::new(), disconnect: None, remote_entries, damage_sent: [0.0; 5], damage_sent_at: 0.0, jump_starts_sent: 0, compound_sent: None, last_start_line: String::new(), session, standings: Vec::new(), results: None, now: 0.0, pit: None, pit_failed: false, gui_events: Vec::new(), result_screen_on: false, send_to_pits_timer: 5.0 }
    }

    /// The cars the game makes remote cars of: every slot but the player's and the
    /// spectators', in the list's order (`ACClient::getRemoteCarList`).
    pub fn remote_entries(cars: &[CarEntry], me: u8) -> Vec<CarEntry> {
        cars.iter().filter(|car| car.session_id != me && !car.is_spectator).cloned().collect()
    }

    /// `Sim::addNetCar` @ 0x140197c30, the physics side: the body of the car in `entry`'s
    /// slot in the engine's world. `guid`: its number among the session's cars (1 for the
    /// first remote car). `mesh`: the car's collider.
    pub fn add_remote_car(&mut self, core: &mut PhysicsCore, entry: CarEntry, data_path: PathBuf, guid: u32, mesh: Option<&rustyac_physics::car::ColliderMesh>, pit_box: Mat44f) {
        let geometry = NetCarGeometry::load(&data_path);
        let provider = NetCarStateProvider::new(core, entry.session_id, guid, &entry.model, geometry, mesh, pit_box);
        let damage = entry.damage_zone_level;
        self.net.cars.push(provider);
        self.remote.push(RemoteCar { entry, data_path, damage });
    }

    /// A system message of the client's (`SystemMessage::addMessage`), its words in the chat
    /// lines of the HUD too.
    fn message(&mut self, title: String, text: String, kind: i32) {
        self.say(if text.is_empty() { title.clone() } else { format!("{title} {text}") });
        self.gui_events.push(OnlineGuiEvent::Message { title, text, kind });
    }

    /// The driver of a session id as `ACClient::getCarAvatarFromSessionID` finds it (the
    /// player's own, a net car's; nobody's: `None`), with its guid.
    fn avatar_of(&self, slot: u8) -> Option<(String, i32)> {
        if slot == self.net.client.session_id {
            return Some((self.driver_of(slot), 0));
        }
        self.net.cars.iter().find(|car| car.session_id == slot).map(|car| (self.driver_of(slot), car.guid as i32))
    }

    /// `ACClient::sendVote` 0x140052ba0: the car's guid to its session id
    /// (`getSessionIdFromCarAvatarID`: nobody's 0xff).
    pub fn send_vote(&mut self, kind: i32, yes: bool, guid: i32) {
        let target = if guid == 0 { self.net.client.session_id } else { self.net.cars.iter().find(|car| car.guid as i32 == guid).map_or(0xff, |car| car.session_id) };
        if (0..=2).contains(&kind) {
            self.net.client.send_event(&rustyac_net::messages::ClientEvent::Vote { kind: kind as u8, yes, target: Some(target) });
        }
    }

    /// rustyAC's test hook `--chat "/vote next|restart|kick <driver> yes|no@<s>"` (headless
    /// runs have no console, no keys Y and N): the vote as the console's commands send it.
    pub fn scripted_vote(&mut self, line: &str) {
        let words: Vec<&str> = line.split(' ').collect();
        let yes = words.last() == Some(&"yes");
        match words.get(1).copied() {
            Some("next") => self.send_vote(0, yes, -1),
            Some("restart") => self.send_vote(1, yes, -1),
            Some("kick") if words.len() >= 4 => {
                let name = words[2..words.len() - 1].join(" ");
                let slot = self.roster.iter().rev().find(|entry| entry.driver == name).map(|entry| entry.slot);
                let guid = slot.and_then(|slot| self.avatar_of(slot)).map_or(-1, |(_, guid)| guid);
                self.console.push(format!("online: vote kick {name} (guid {guid}) {}", if yes { "yes" } else { "no" }));
                self.send_vote(2, yes, guid);
            }
            _ => self.console.push(format!("online: not a vote: {line}")),
        }
    }

    fn say(&mut self, line: String) {
        self.console.push(line.clone());
        self.chat.push_back(line);
        while self.chat.len() > CHAT_LINES {
            self.chat.pop_front();
        }
    }

    /// What the server is told of the local car right after a step: its sector times and
    /// laps (`ACClient::onSectorSplit`, `onLapCompleted`: laps that crossed every sector),
    /// a jump start, a change of tyres, and its damage when that changed
    /// (`ACClient::updateDamageReport`: at most every two seconds).
    pub fn report_local_car(&mut self, car: &RollingChassis, laps: &[rustyac_physics::track::timing::OnLapCompletedEvent], splits: &[rustyac_physics::track::timing::OnSectorSplitEvent], physics_time: f64) {
        use rustyac_net::messages::ClientEvent;
        for split in splits {
            self.net.client.send_event(&ClientEvent::SectorSplit { sector: split.sector_index as u8, time: split.sector_time, cuts: split.cuts.max(0) as u32 });
        }
        for lap in laps.iter().filter(|lap| lap.is_valid) {
            self.net.client.send_event(&ClientEvent::LapCompleted { physics_time, lap_time: lap.lap_time, splits: lap.splits.clone(), cuts: lap.cuts, lap_count: car.transponder.lap_count as u8 });
            self.console.push(format!("online: lap {} of {} told to the server{}", car.transponder.lap_count, lap_text(lap.lap_time), if lap.cuts > 0 { format!(" ({} cuts)", lap.cuts) } else { String::new() }));
        }
        if car.jump_start_events != self.jump_starts_sent {
            self.jump_starts_sent = car.jump_start_events;
            self.net.client.send_event(&ClientEvent::JumpStart);
        }
        let compound = car.tyres.first().and_then(|tyre| tyre.compound_defs.get(tyre.current_compound_index as usize)).map(|def| def.short_name.clone());
        if let Some(compound) = compound {
            if self.compound_sent.as_ref() != Some(&compound) {
                // (the first one is the compound the car starts on: nothing to tell)
                if self.compound_sent.is_some() {
                    self.net.client.send_event(&ClientEvent::TyreCompound(compound.clone()));
                }
                self.compound_sent = Some(compound);
            }
        }
        if physics_time - self.damage_sent_at > 2000.0 && car.damage_zone_level != self.damage_sent {
            self.damage_sent = car.damage_zone_level;
            self.damage_sent_at = physics_time;
            self.net.client.send_event(&ClientEvent::Damage(self.damage_sent));
        }
    }

    fn driver_of(&self, slot: u8) -> String {
        match self.roster.iter().find(|entry| entry.slot == slot) {
            Some(entry) if !entry.driver.is_empty() => entry.driver.clone(),
            _ => format!("car {slot}"),
        }
    }

    /// What arrived, for the displays; what the game must act on is left in `pending`.
    fn digest(&mut self) {
        for event in std::mem::take(&mut self.net.events) {
            match &event {
                Event::Message(Message::Chat { slot, text }) => {
                    let who = if *slot == 0xff { "SERVER".to_string() } else { self.driver_of(*slot) };
                    self.say(format!("{who}: {text}"));
                    self.gui_events.push(OnlineGuiEvent::Chat { session: *slot, text: text.clone() });
                }
                Event::Message(Message::NewSession(info)) => {
                    // the handler of the client's new-session event (lambda @ 0x1401337d0):
                    // the manager's session is a copy of the server's
                    self.session = mirror_session(info, info.start as f64);
                    self.standings.clear();
                    self.results = None;
                    // ACClient::onNewSession: the result screen is over
                    self.result_screen_on = false;
                    self.send_to_pits_timer = 0.0;
                    self.gui_events.push(OnlineGuiEvent::NewSession);
                    if let Some(pit) = &mut self.pit {
                        pit.new_session = true;
                    }
                    // (D.9: not at the join, which the handshake carries)
                    self.message("New Session :".to_string(), info.name.clone(), 0);
                    for entry in &mut self.roster {
                        entry.laps = 0;
                        entry.last_lap_ms = 0;
                        entry.best_lap_ms = 0;
                    }
                }
                Event::Message(Message::LapCompleted { slot, lap_time, cuts, standings, .. }) => {
                    // the standings: laps and (in practice and qualifying) the best lap
                    self.standings = standings.clone();
                    let race = self.net.client.current_session.kind == session_type::RACE;
                    for row in standings {
                        if let Some(entry) = self.roster.iter_mut().find(|entry| entry.slot == row.slot) {
                            entry.laps = row.laps as u32;
                            if !race {
                                entry.best_lap_ms = if row.time == rustyac_net::messages::NO_TIME { 0 } else { row.time };
                            }
                        }
                    }
                    if *slot != 0xff {
                        if let Some(entry) = self.roster.iter_mut().find(|entry| entry.slot == *slot) {
                            entry.last_lap_ms = *lap_time;
                            if race && *cuts == 0 && (entry.best_lap_ms == 0 || *lap_time < entry.best_lap_ms) {
                                entry.best_lap_ms = *lap_time;
                            }
                        }
                        let who = self.driver_of(*slot);
                        self.console.push(format!("online: the server counts a lap of {who}: {}{}", lap_text(*lap_time), if *cuts > 0 { format!(" ({cuts} cuts)") } else { String::new() }));
                    }
                }
                Event::Message(Message::ClientDisconnected { slot }) => {
                    let who = self.driver_of(*slot);
                    if let Some(entry) = self.roster.iter_mut().find(|entry| entry.slot == *slot) {
                        entry.connected = false;
                        entry.driver.clear();
                    }
                    // (only for a net car of that session id)
                    if self.net.cars.iter().any(|car| car.session_id == *slot) {
                        self.message("Server Communication".to_string(), format!("Player {who} disconnected from the server"), 2);
                    } else {
                        self.say(format!("Player {who} disconnected from the server"));
                    }
                }
                Event::Message(Message::DriverConnected { name, slot, nation_code }) => {
                    if let Some(entry) = self.roster.iter_mut().find(|entry| entry.slot == *slot) {
                        entry.driver = name.clone();
                        entry.connected = true;
                    }
                    if let Some(remote) = self.remote.iter_mut().find(|remote| remote.entry.session_id == *slot) {
                        remote.damage = [0.0; 5];
                    }
                    self.message(format!("Car {slot} is now driven by {name} ({nation_code})"), String::new(), 3);
                }
                Event::Message(Message::DriverNames(names)) => {
                    for (slot, name) in names {
                        if let Some(entry) = self.roster.iter_mut().find(|entry| entry.slot == *slot) {
                            entry.driver = name.clone();
                            if entry.me || !name.is_empty() {
                                entry.connected = true;
                            }
                        }
                    }
                }
                Event::Message(Message::MandatoryPit { slot, done }) => {
                    // ACP_CE_MANDATORY_PIT: a car has done its mandatory stop (the server
                    // says so of the player's own car too)
                    if *done {
                        let who = self.driver_of(*slot);
                        if *slot == self.net.client.session_id {
                            if let Some(pit) = &mut self.pit {
                                pit.avatar.mandatory_pitstop_done = true;
                            }
                        }
                        self.say(format!("{who} has done the mandatory pit stop"));
                    }
                }
                Event::Message(Message::Damage { slot, levels }) => {
                    if let Some(remote) = self.remote.iter_mut().find(|remote| remote.entry.session_id == *slot) {
                        remote.damage = *levels;
                    }
                }
                Event::Message(Message::WelcomeMessage(text)) => {
                    self.welcome = text.clone();
                    for line in text.lines().filter(|line| !line.trim().is_empty()) {
                        self.console.push(format!("server: {line}"));
                    }
                }
                // the player is put out himself: no notice, the countdown's red screen
                // (ACClient's handler of the message 0x68)
                Event::Message(Message::Kicked { slot, reason }) if *slot == self.net.client.session_id => {
                    if self.disconnect.is_none() {
                        self.disconnect = Some(
                            match reason {
                                0 => "You have been voted to be kicked from the race",
                                1 => "You have been voted to be banned from the server",
                                2 => "You have been voted to be blacklisted from the server",
                                3 => "Checksum failed. You have been kicked out of the race",
                                _ => "You have been kicked from the server",
                            }
                            .to_string(),
                        );
                    }
                }
                Event::Message(Message::Kicked { slot, reason }) => {
                    let who = self.driver_of(*slot);
                    let why = match reason {
                        0 => "Kicked from the race",
                        1 => "Kicked until the server restarts",
                        2 => "Kicked and put on blacklist",
                        3 => "Checksum failed",
                        _ => "Kicked from the server",
                    };
                    self.message(format!("Kicked Driver {who}"), why.to_string(), 4);
                }
                Event::Message(Message::RaceOver { standings, .. }) => {
                    // the server's results of the race (RemoteSessionResume: what the game's
                    // end-of-session screen is made from online)
                    self.results = Some(standings.clone());
                    self.result_screen_on = true;
                    self.say("The race is over".to_string());
                    for (place, row) in standings.iter().enumerate() {
                        let who = self.driver_of(row.slot);
                        if self.roster.iter().any(|entry| entry.slot == row.slot && entry.connected) {
                            self.console.push(format!("online:   {}. {who}  {}  {} laps", place + 1, lap_text(row.time), row.laps));
                        }
                    }
                }
                Event::Message(Message::Weather { ambient, road, graphics, wind_speed_kmh, wind_direction_deg }) => {
                    self.console.push(format!("online: the server's weather: air {ambient} C, road {road} C, {graphics}, wind {wind_speed_kmh} km/h from {wind_direction_deg} deg"));
                }
                Event::Message(Message::JumpStart { slot }) => {
                    let who = self.driver_of(*slot);
                    self.say(format!("[SERVER] Jumpstart detected for {who}"));
                }
                // (the panel, the prompt and the "not passed" message are the GUI's voting
                // manager's; a kick vote against the player never reaches it)
                Event::Message(Message::Vote { kind, target, time_left_ms, last_voter, .. }) => {
                    if !(*kind == 2 && *target == self.net.client.session_id) {
                        let target = if *kind == 2 { self.avatar_of(*target) } else { None };
                        self.gui_events.push(OnlineGuiEvent::Vote { kind: *kind as i32, target, time_left_ms: *time_left_ms as i32, is_me: *last_voter == self.net.client.session_id });
                    }
                }
                Event::Message(Message::Setup { values, .. }) => {
                    let named: Vec<String> = values.iter().map(|(name, value)| format!("{name}={value}")).collect();
                    self.console.push(format!("online: the server's fixed setup: {}", named.join(" ")));
                    self.gui_events.push(OnlineGuiEvent::Setup { values: values.clone() });
                }
                Event::Message(Message::VoteNotPassed) => {
                    self.say("Voting ended. Not passed".to_string());
                    self.gui_events.push(OnlineGuiEvent::VoteNotPassed);
                }
                Event::RaceStart(start) => {
                    // (the server repeats it to make sure it arrives)
                    let line = format!("online: the session starts at {:.1} s of the server's clock", start / 1000.0);
                    if self.last_start_line != line {
                        self.last_start_line = line.clone();
                        self.console.push(line);
                    }
                }
                Event::Associated => self.console.push("online: the server answers on UDP, car updates flow".to_string()),
                Event::ServerSilent => self.message("Connection Issue".to_string(), "The server seems to be unavailable.".to_string(), 0),
                Event::ConnectionLost(message) => self.say(format!("Connection lost: {message}")),
                _ => {}
            }
            self.pending.push(event);
        }
    }

    /// What the session saw of every driver, for the console at the end.
    pub fn summary(&self) -> Vec<String> {
        let mut lines = Vec::new();
        for entry in self.roster.iter().filter(|entry| entry.connected) {
            let remote = self.net.cars.iter().find(|car| car.session_id == entry.slot);
            let packets = match remote {
                Some(car) => format!(
                    ", states taken {} in order + {} out of order, ping {} ms, {}",
                    car.qos_good_packets,
                    car.qos_bad_packets,
                    car.ping,
                    if car.is_disconnected { "not there at the end" } else { "there at the end" }
                ),
                None => String::new(),
            };
            lines.push(format!(
                "online:   slot {:2} {:<20} {:<18} laps {} (best {}){}{}",
                entry.slot,
                if entry.driver.is_empty() { "?" } else { &entry.driver },
                entry.model,
                entry.laps,
                if entry.best_lap_ms > 0 { lap_text(entry.best_lap_ms) } else { "-".to_string() },
                if entry.me { "  <- you" } else { "" },
                packets
            ));
        }
        lines
    }

    /// What AC's own online overlays show.
    pub fn gui_state(&self) -> OnlineGui {
        let accepted = &self.net.client.accepted;
        let net_cars = self.net.cars.iter().map(|car| (car.session_id, car.guid as i32, self.driver_of(car.session_id))).collect();
        OnlineGui {
            ping: self.net.client.ping as i32,
            welcome: self.welcome.clone(),
            disconnect: self.disconnect.clone(),
            server_name: accepted.server_name.clone(),
            rules: Some(ServerRules::of(accepted)),
            inverted_grid_positions: accepted.inverted_grid_positions as i32,
            session_id: self.net.client.session_id,
            net_cars,
        }
    }

    /// The lines of the player list and the chat, for the HUD.
    pub fn hud_lines(&self) -> Vec<String> {
        let mut lines = Vec::new();
        let session = &self.net.client.current_session;
        lines.push(format!("ONLINE  {}  ({})  ping {} ms", self.net.client.accepted.server_name, session.name, self.net.client.ping));
        lines.extend(self.referee_lines());
        if let Some(pit) = &self.pit {
            lines.extend(pit.lines());
        }
        for entry in self.roster.iter().filter(|entry| entry.connected) {
            let best = if entry.best_lap_ms > 0 { crate::online::lap_text(entry.best_lap_ms) } else { "-:--.---".to_string() };
            let name = if entry.driver.is_empty() { "?" } else { &entry.driver };
            lines.push(format!("{} {:<20} {:<18} laps {:<3} best {}{}", if entry.me { ">" } else { " " }, name, entry.model, entry.laps, best, if entry.me { String::new() } else { format!("  {} ms", entry.ping) }));
        }
        lines.extend(self.chat.iter().cloned());
        lines
    }

    /// Task 28b: a frame of the player's pit stop (every third physics step, as offline):
    /// `CarAvatar::updateInPitlaneState`, the pit window's part of `RaceManager::update`
    /// @ 0x140141550 and `PitStop::update`. A stop that counts is told to the server.
    pub fn pit_frame(&mut self, car: &mut RollingChassis, now: f64) {
        use rustyac_physics::race::pitstop::{self, PitAction, PitCarView, PitFrame};
        let Some(pit) = &mut self.pit else { return };
        pit.step_in_frame += 1;
        if pit.step_in_frame < rustyac_physics::race::referee::FRAME_STEPS {
            return;
        }
        pit.step_in_frame = 0;
        let dt = rustyac_physics::race::referee::FRAME_DT;
        let race = self.session.session_type == session_type::RACE as i32;
        // the manager's session is the server's; the leader's laps come with the standings
        pit.manager.current_session = self.session.clone();
        pit.manager.current_session.leader_completed_laps = self.standings.iter().map(|row| row.laps as u32).max().unwrap_or(0);
        let mut actions = Vec::new();
        if std::mem::take(&mut pit.new_session) {
            // RaceManager::setCurrentSession (resetMandatoryPit), ACClient::onNewSession and
            // PitStop's own handler
            pit.manager.pit_window_start_time = 0.0;
            pit.manager.pit_window_end_time = 0.0;
            pit.avatar.last_pitstop_time = 0.0;
            pit.avatar.mandatory_pitstop_done = false;
            pit.avatar.pit_lane_entry_time = 0.0;
            pit.avatar.pit_lane_exit_time = 0.0;
            pit.pit.on_new_session(&mut actions);
        }
        // CarAvatar::updateInPitlaneState
        let in_pitlane = car.is_in_pit_lane();
        let mut counted = false;
        let a = &mut pit.avatar;
        a.in_pitlane = in_pitlane;
        if in_pitlane {
            if !a.was_in_pitlane {
                a.was_in_pitlane = true;
                a.pit_lane_entry_time = now as f32;
            }
        } else if a.was_in_pitlane {
            a.was_in_pitlane = false;
            a.pit_lane_exit_time = now as f32;
            if pit.manager.check_mandatory_pit(a.mandatory_pitstop_done, a.last_pitstop_time, a.pit_lane_entry_time, a.pit_lane_exit_time) {
                a.mandatory_pitstop_done = true;
                counted = true;
            }
        }
        // the window, and the black flag for a car that missed it
        let (opened, closed) = (pit.manager.pit_window_start_time, pit.manager.pit_window_end_time);
        let mut said = Vec::new();
        if pit.window.1 != 0 && pit.manager.update_pit_window(pit.window.0, pit.window.1, now, pit.avatar.mandatory_pitstop_done, pit.avatar.in_pitlane) {
            if !car.black_flagged && !car.is_retired {
                said.push("online: black flag: You have been disqualified for not doing the Mandatory Pit".to_string());
            }
            car.queue(|car| car.set_black_flag_for(true, 3));
            car.is_retired = true;
        }
        if opened == 0.0 && pit.manager.pit_window_start_time != 0.0 {
            said.push(format!("online: the pit window is open ({:.1} s into the race)", pit.manager.pit_window_start_time / 1000.0));
        }
        if closed == 0.0 && pit.manager.pit_window_end_time != 0.0 {
            said.push(format!("online: the pit window is closed ({:.1} s into the race)", pit.manager.pit_window_end_time / 1000.0));
        }
        // PitStop::update
        let view = PitCarView::of(car);
        let race_over = race && self.results.is_some();
        let frame = PitFrame { dt, session_type: self.session.session_type, race_over, in_replay: false, pause_menu: false, esc_menu: false };
        let mut rand = pit.rand;
        pit.pit.update(&frame, &view, &mut pit.menu, car, &mut rand, &mut actions);
        pit.rand = rand;
        if pit.pit.timer_ran {
            pit.avatar.last_pitstop_time = now as f32;
        }
        let allowed = (1..=3).contains(&frame.session_type) && pit.pit.app_interaction_enabled;
        pit.menu.frame(dt, view.is_in_pitlane, allowed);
        for action in actions {
            match action {
                PitAction::ControlsLock(locked) => car.queue(move |car| car.lock_controls(locked)),
                PitAction::Commit(start) => said.push(format!("Pit stop: {:.1} s", start.total_time)),
                PitAction::Over => said.push("Pit stop over".to_string()),
                PitAction::Apply(choices) => car.queue(move |car| pitstop::apply(car, &choices)),
                PitAction::MouseApply(choices) => car.queue(move |car| pitstop::apply_mouse(car, &choices)),
                PitAction::MouseSteering(on) => pit.mouse_steering = Some(on),
            }
        }
        if counted {
            // the handler of ACClient::evOnMandatoryPitDone (lambda @ 0x14003e2c0)
            self.net.client.send_event(&rustyac_net::messages::ClientEvent::MandatoryPit(true));
            said.push("online: the mandatory pit stop is done, told to the server".to_string());
        }
        for line in said {
            self.console.push(line);
        }
    }

    /// Task 42b: `ACClient::update` 0x140053b30 while the race's results show: the car stops
    /// gently, and once `sendToPitsTimer` runs out it goes to its pit box (true: now).
    /// shortcut: options.ini's IGNORE_RESULT_TELEPORT is not read and there is no fade; add them if a player asks.
    pub fn result_frame(&mut self, car: &mut RollingChassis, dt: f32) -> bool {
        // (CarAvatar::setGentleStop: on every frame of the result screen, off at a new session)
        car.is_gentle_stopping = self.result_screen_on;
        if !self.result_screen_on || !pits_due(&mut self.send_to_pits_timer, dt) {
            return false;
        }
        self.gui_events.push(OnlineGuiEvent::SentToPits);
        true
    }

    /// Task 28: the server's session as the referee's banner words it, its standings, and
    /// its results when the race is over.
    pub fn referee_lines(&self) -> Vec<String> {
        let mut lines = Vec::new();
        let mirrored = &self.session;
        let race = mirrored.session_type == session_type::RACE as i32;
        let left = mirrored.duration_minutes as f64 * 60.0 * 1000.0 + mirrored.start_time - self.now;
        let leader_laps = self.standings.iter().map(|row| row.laps).max().unwrap_or(0);
        let state = if race && !mirrored.is_timed_race {
            if self.results.is_some() {
                "RACE OVER".to_string()
            } else if mirrored.start_time > self.now {
                format!("start in {}", rustyac_physics::race::hud::time_to_wstring((mirrored.start_time - self.now) as i32))
            } else {
                format!("{} / {}", (leader_laps as i32 + 1).min(mirrored.laps.max(1)), mirrored.laps)
            }
        } else if mirrored.duration_minutes > 0.0 {
            if left >= 0.0 {
                rustyac_physics::race::hud::time_to_wstring(left as i32)
            } else if race {
                "RACE OVER".to_string()
            } else {
                "OVERTIME".to_string()
            }
        } else {
            String::new()
        };
        lines.push(format!("{}  {state}", if mirrored.name.is_empty() { "SESSION" } else { &mirrored.name }));
        let table = self.results.as_ref().map(|rows| ("RESULTS", rows)).or((!self.standings.is_empty()).then_some(("STANDINGS", &self.standings)));
        if let Some((title, rows)) = table {
            lines.push(title.to_string());
            for (place, row) in rows.iter().enumerate() {
                let who = self.driver_of(row.slot);
                let time = if row.time == rustyac_net::messages::NO_TIME || row.time == 0 { "-:--.---".to_string() } else { lap_text(row.time) };
                lines.push(format!("{:>2}  {:<20} laps {:<3} {} {time}", place + 1, who, row.laps, if race { "total" } else { "best" }));
            }
        }
        lines
    }
}

/// A lap time as `m:ss.mmm`.
pub fn lap_text(ms: u32) -> String {
    if ms >= rustyac_net::messages::NO_TIME {
        return "no time".to_string();
    }
    format!("{}:{:02}.{:03}", ms / 60_000, ms / 1000 % 60, ms % 1000)
}

/// The session's work in the step-completed phase of a physics step: the shared
/// [`NetSession`]'s, and after it what the displays keep.
pub struct OnlineHooks<'a> {
    pub session: &'a mut OnlineSession,
}

impl StepHooks for OnlineHooks<'_> {
    fn before_cars(&mut self, core: &mut PhysicsCore, cars: &mut [CarSlot<'_>], physics_time: f64) {
        self.session.net.before_cars(core, cars, physics_time);
        self.session.digest();
    }

    fn after_cars(&mut self, core: &mut PhysicsCore, cars: &mut [CarSlot<'_>], physics_time: f64) {
        let session = &mut *self.session;
        session.net.after_cars(core, cars, physics_time);
        session.now = physics_time;
        // NetCarStateProvider::update (the game: once a frame)
        session.net.update_cars(physics_time);
        // the displays: who is there
        for entry in &mut session.roster {
            if let Some(car) = session.net.cars.iter().find(|car| car.session_id == entry.slot) {
                entry.ping = car.ping;
                if !car.is_disconnected {
                    entry.connected = true;
                }
            }
        }
    }
}

/// `endSession.sendToPitsTimer` of `ACClient::update`: true once, the frame it runs out
/// (`FLT_MAX` afterwards, until a new session sets it again).
fn pits_due(timer: &mut f32, dt: f32) -> bool {
    if *timer == f32::MAX {
        return false;
    }
    *timer -= dt;
    if *timer >= 0.0 {
        return false;
    }
    *timer = f32::MAX;
    true
}

#[cfg(test)]
mod tests {
    #[test]
    fn sent_to_pits_once_after_the_timer() {
        // the first race's results: 5 s (ACClient::ACClient), then never again in that session
        let mut timer = 5.0;
        let frames: Vec<bool> = (0..400).map(|_| super::pits_due(&mut timer, 1.0 / 60.0)).collect();
        assert_eq!(frames.iter().filter(|due| **due).count(), 1);
        assert_eq!(frames.iter().position(|due| *due), Some(299)); // the 300th frame: 5 s
        // a later session's: 0 s (ACClient::onNewSession), the first frame
        let mut timer = 0.0;
        assert!(super::pits_due(&mut timer, 0.003));
        assert!(!super::pits_due(&mut timer, 0.003));
    }
}
