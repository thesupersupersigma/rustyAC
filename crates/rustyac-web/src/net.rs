// SPDX-License-Identifier: GPL-3.0-or-later

//! Multiplayer in the browser: the page's `WebSocket` to a gateway next to the server
//! (`crates/rustyac-gateway`) as the transport of `rustyac-net`'s client.
//!
//! The page owns the `WebSocket` (`web/app.js`): what arrives is handed in
//! ([`NetLink::push`]), what the client sends goes out through a function the page gives
//! ([`NetLink::new`]). In between sits [`rustyac_net::ws::WsTransport`], the same framing
//! the desktop uses through a gateway, and above it the same client, the same join (stepped
//! here with `Join::poll`, because a page cannot wait) and the same remote cars as on the
//! desktop.
//!
//! ```text
//! page:  link = new NetLink(send)        ws.onmessage -> link.push(bytes)
//!        join = NetJoin.start(link, id, name, car, password)
//!        join.status()   ... until "done" (or an error)
//!        the files of the server's car, track and other cars -> files_wanted / FileWriter
//!        game = await Game.create_online(canvas, join, ...)
//! ```

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::path::Path;
use std::rc::Rc;

use rustyac_game::online::{finish_join, BoxedTransport, OnlineSession};
use rustyac_net::client::{Join, JoinStage};
use rustyac_net::handshake::JoinRequest;
use rustyac_net::ws::{Pipe, WsTransport, DATAGRAM_BACKLOG, LANE_DATAGRAM};
use wasm_bindgen::prelude::*;

use crate::web::{json_text, now_ms};

/// When a car state arrived at the page and when the session took it (for measuring the
/// WebSocket's delay against UDP's): milliseconds since 1970, and the datagram's start.
struct Arrival {
    arrived: f64,
    taken: f64,
    head: Vec<u8>,
}

/// The WebSocket's end inside the wasm side.
pub struct Link {
    send: js_sys::Function,
    inbox: RefCell<VecDeque<(Vec<u8>, Option<usize>)>>,
    lost: RefCell<Option<String>>,
    /// Datagrams thrown away because the page did not step (a tab in the background).
    dropped: Cell<u64>,
    recording: Cell<bool>,
    arrivals: RefCell<Vec<Arrival>>,
}

impl Link {
    pub fn dropped(&self) -> u64 {
        self.dropped.get()
    }
}

/// Milliseconds since 1970 with the page's fine clock.
fn wall_ms() -> f64 {
    web_sys::window().and_then(|w| w.performance()).map_or_else(js_sys::Date::now, |p| p.time_origin() + p.now())
}

/// The page's handle of the link.
#[wasm_bindgen]
pub struct NetLink {
    link: Rc<Link>,
}

#[wasm_bindgen]
impl NetLink {
    /// `send`: called with a `Uint8Array` for every message that has to go out on the
    /// WebSocket (a binary message). It throws when the socket is not open.
    #[wasm_bindgen(constructor)]
    pub fn new(send: js_sys::Function) -> NetLink {
        NetLink { link: Rc::new(Link { send, inbox: RefCell::new(VecDeque::new()), lost: RefCell::new(None), dropped: Cell::new(0), recording: Cell::new(false), arrivals: RefCell::new(Vec::new()) }) }
    }

    /// A binary message of the WebSocket.
    pub fn push(&self, message: &[u8]) {
        let link = &self.link;
        let mut inbox = link.inbox.borrow_mut();
        let datagram = message.first() == Some(&LANE_DATAGRAM);
        // a page that does not step (a hidden tab) must not pile datagrams up for ever: the
        // oldest go, as a UDP socket's buffer would lose them
        if datagram && inbox.len() >= 4 * DATAGRAM_BACKLOG {
            if let Some(at) = inbox.iter().position(|(known, _)| known.first() == Some(&LANE_DATAGRAM)) {
                inbox.remove(at);
                link.dropped.set(link.dropped.get() + 1);
            }
        }
        let mut record = None;
        if link.recording.get() && datagram && matches!(message.get(1), Some(0x46 | 0x48)) {
            let mut arrivals = link.arrivals.borrow_mut();
            if arrivals.len() < 200_000 {
                record = Some(arrivals.len());
                arrivals.push(Arrival { arrived: wall_ms(), taken: 0.0, head: message[1..].to_vec() });
            }
        }
        inbox.push_back((message.to_vec(), record));
    }

    /// The WebSocket closed or failed: `why` is what the player is told.
    pub fn lost(&self, why: &str) {
        let mut lost = self.link.lost.borrow_mut();
        if lost.is_none() {
            *lost = Some(if why.is_empty() { "the connection to the gateway closed".to_string() } else { why.to_string() });
        }
    }

    /// Keep the arrival time of every car state (a check measures the delay with it).
    pub fn record(&self, on: bool) {
        self.link.recording.set(on);
    }

    /// The recorded arrivals as CSV: `arrived_ms,taken_ms,slot,sequence,x,y,z` (one line per
    /// car state; the times are milliseconds since 1970; `taken` is when the session read it).
    pub fn arrivals(&self) -> String {
        let mut out = String::from("arrived_ms,taken_ms,slot,sequence,x,y,z\n");
        for arrival in self.link.arrivals.borrow().iter() {
            for (slot, sequence, position) in rustyac_net::datagram::car_state_keys(&arrival.head) {
                out.push_str(&format!("{:.3},{:.3},{slot},{sequence},{:08x},{:08x},{:08x}\n", arrival.arrived, arrival.taken, position[0], position[1], position[2]));
            }
        }
        out
    }
}

/// The link as a pipe of whole messages for [`WsTransport`].
struct JsPipe(Rc<Link>);

impl Pipe for JsPipe {
    fn send(&mut self, message: &[u8]) -> Result<(), String> {
        if let Some(why) = self.0.lost.borrow().as_ref() {
            return Err(why.clone());
        }
        let bytes = js_sys::Uint8Array::from(message);
        self.0.send.call1(&JsValue::NULL, &bytes).map(|_| ()).map_err(|error| {
            let why = format!("the WebSocket took no message: {}", error.as_string().unwrap_or_else(|| format!("{error:?}")));
            *self.0.lost.borrow_mut() = Some(why.clone());
            why
        })
    }

    fn poll(&mut self) -> Result<Option<Vec<u8>>, String> {
        match self.0.inbox.borrow_mut().pop_front() {
            Some((message, record)) => {
                if let Some(index) = record {
                    if let Some(arrival) = self.0.arrivals.borrow_mut().get_mut(index) {
                        arrival.taken = wall_ms();
                    }
                }
                Ok(Some(message))
            }
            // (what arrived before the socket closed is handed out first)
            None => match self.0.lost.borrow().as_ref() {
                Some(why) => Err(why.clone()),
                None => Ok(None),
            },
        }
    }
}

/// A join in progress: the handshake and the car list, stepped by the page.
#[wasm_bindgen]
pub struct NetJoin {
    join: Option<Join<BoxedTransport>>,
    link: Rc<Link>,
    error: Option<String>,
    started: f64,
}

#[wasm_bindgen]
impl NetJoin {
    /// Sends the join request on an open link. `guid`: the player's id (the page keeps a
    /// random one in its local storage); `car`: the model asked for (empty: any free slot).
    pub fn start(link: &NetLink, guid: &str, name: &str, car: &str, password: &str) -> Result<NetJoin, JsValue> {
        if guid.is_empty() || guid.len() > 64 || !guid.bytes().all(|b| b.is_ascii_alphanumeric()) {
            return Err(JsValue::from_str("the player id is not letters and digits"));
        }
        let name: String = name.trim().chars().filter(|c| !c.is_control()).take(32).collect();
        let request = JoinRequest { guid: guid.to_string(), name: if name.is_empty() { "rustyAC driver".to_string() } else { name }, team: String::new(), nation_code: String::new(), car: car.to_string(), password: password.to_string() };
        let transport: BoxedTransport = Box::new(WsTransport::new(JsPipe(link.link.clone())));
        let join = Join::start(transport, &request).map_err(|e| JsValue::from_str(&e.to_string()))?;
        Ok(NetJoin { join: Some(join), link: link.link.clone(), error: None, started: now_ms() })
    }

    /// Takes the answers that have arrived and says where the join is, as JSON:
    /// `{"stage": "handshake" | "cars" | "done" | "error", "error", "server", "track",
    /// "layout", "car", "slot", "session", "cars": [models], "drivers": [names],
    /// "checksum_files": [paths]}`. Call it until the stage is `done` or `error`.
    pub fn status(&mut self) -> String {
        if self.error.is_none() {
            if let Some(join) = &mut self.join {
                match join.poll() {
                    Ok(_) => {}
                    Err(why) => {
                        self.error = Some(why.to_string());
                        self.join = None;
                    }
                }
            }
            // (the game waits for ever; a page says so after a while)
            if self.error.is_none() && self.join.as_ref().is_some_and(|join| join.stage() != JoinStage::Done) && now_ms() - self.started > rustyac_net::client::JOIN_TIMEOUT_MS as f64 {
                self.error = Some(rustyac_net::handshake::Refusal::NoResponse.message());
                if let Some(join) = self.join.take() {
                    join.abandon();
                }
            }
        }
        if let Some(why) = &self.error {
            return format!("{{\"stage\": \"error\", \"error\": {}}}", json_text(why));
        }
        let Some(join) = &self.join else {
            return "{\"stage\": \"error\", \"error\": \"the join is over\"}".to_string();
        };
        let stage = match join.stage() {
            JoinStage::Handshake => "handshake",
            JoinStage::CarList => "cars",
            JoinStage::Done => "done",
        };
        let Some(accepted) = join.accepted() else {
            return format!("{{\"stage\": \"{stage}\"}}");
        };
        let list = |texts: Vec<String>| texts.iter().map(|text| json_text(text)).collect::<Vec<_>>().join(", ");
        let (cars, drivers) = match join.cars() {
            Some(cars) => {
                let others = OnlineSession::remote_entries(cars, accepted.session_id);
                (others.iter().map(|car| car.model.clone()).collect(), others.iter().filter(|car| !car.driver_name.is_empty()).map(|car| car.driver_name.clone()).collect())
            }
            None => (Vec::new(), Vec::new()),
        };
        format!(
            "{{\"stage\": \"{stage}\", \"server\": {}, \"track\": {}, \"layout\": {}, \"car\": {}, \"slot\": {}, \"session\": {}, \"cars\": [{}], \"drivers\": [{}], \"checksum_files\": [{}]}}",
            json_text(&accepted.server_name),
            json_text(&accepted.track),
            json_text(&accepted.track_config),
            json_text(&accepted.model),
            accepted.session_id,
            json_text(&accepted.current_session.name),
            list(cars),
            list(drivers),
            list(accepted.checksum_files.clone())
        )
    }

    /// Gives the join up (the content is not here, the player changed their mind): a
    /// player the server had let in says goodbye.
    pub fn abandon(&mut self) {
        if let Some(join) = self.join.take() {
            join.abandon();
        }
    }
}

impl NetJoin {
    /// The session of a join that is done: the names the server sent are checked, the
    /// checksums it asked for are worked out from the page's own files below `root`.
    pub(crate) fn finish(mut self, root: &Path) -> Result<(OnlineSession, Rc<Link>), String> {
        let join = self.join.take().ok_or("the join is over")?;
        if join.stage() != JoinStage::Done {
            join.abandon();
            return Err("the join is not done".to_string());
        }
        let (client, cars) = join.finish().ok_or("the join is not done")?;
        Ok((finish_join(client, cars, Some(root))?, self.link))
    }
}
