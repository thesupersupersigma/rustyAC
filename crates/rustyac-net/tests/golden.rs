// SPDX-License-Identifier: GPL-3.0-or-later

//! Task 25: an online session against the game's own code.
//!
//! `golden_laguna_join`: an excerpt of `tools/car_oracle net --scenario net_block` on the tape
//! of a live session (Kunos' `acServer`, Laguna Seca, four clients): the local F2004 stands on
//! the racing line, the three other cars of the tape arrive (two F2004s and a Z4 GT3, which
//! runs into it). The excerpt holds the local car's inputs, the tape's messages and, per
//! step, the hash of what the game's own `ACClient` and `NetCarStateProvider` made of them:
//! the local car, every remote car's body and state, every outgoing message. The port
//! replays it and must give the same hash at every step. Needs Assetto Corsa's folder (the
//! track, the cars' collider meshes): NOT TESTED without it.

use std::path::PathBuf;

use rustyac_net::replay::NetGolden;
use rustyac_net::transport::Lane;
use rustyac_physics::engine::record::ReplayPaths;

fn cardata() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../cardata")
}

fn game_root() -> Option<PathBuf> {
    let root = rustyac_content::install::ac_root().unwrap_or_else(|| PathBuf::from(r"C:\Program Files (x86)\Steam\steamapps\common\assettocorsa"));
    root.join("content").join("tracks").join("ks_laguna_seca").is_dir().then_some(root)
}

#[test]
fn golden_laguna_join() {
    let golden = NetGolden::from_bytes(include_bytes!("golden/laguna_join_3000.netgold")).expect("the golden file");
    for car in ["ks_ferrari_f2004", "bmw_z4_gt3"] {
        if !cardata().join(car).join("suspensions.ini").is_file() {
            eprintln!("NOT TESTED: cardata/{car} is missing (the car's extracted data files)");
            return;
        }
    }
    let Some(root) = game_root() else {
        eprintln!("NOT TESTED: Assetto Corsa's folder is missing (set AC_ROOT): the online golden replay needs the track ks_laguna_seca and the cars' collider.kn5");
        return;
    };
    let paths = ReplayPaths { cardata: cardata(), game_root: Some(root) };
    assert!(golden.hashes.len() >= 3000, "the excerpt is too short to reach the hit");
    match golden.replay(&paths) {
        Ok(result) => assert_eq!(result, None, "the port leaves the game's values at this step"),
        Err(message) if message.contains("track") => {
            eprintln!("NOT TESTED: the track could not be loaded ({message})");
            return;
        }
        Err(message) => panic!("the replay: {message}"),
    }
    // the comparison can fail: one bit of one car state that arrived
    let mut spoiled = golden.clone();
    let entry = spoiled.tape.entries.iter_mut().find(|entry| entry.lane == Lane::DatagramIn && entry.tick >= 1500 && entry.data.first() == Some(&0x46)).expect("a car state after step 1500");
    entry.data[12] ^= 1;
    let result = spoiled.replay(&paths).expect("the replay");
    assert!(result.is_some_and(|step| step >= 1500), "a changed bit of a car state went unnoticed");
}

/// Task 26: the join as a state machine. The browser cannot wait for an answer: it polls,
/// and each answer arrives in a frame of its own. The desktop waits. Both ways must send the
/// same messages in the same order and end with the same client and car list. The tape of
/// the golden excerpt answers (no Assetto Corsa file is needed here).
#[test]
fn join_polled_is_join_waited() {
    use rustyac_net::client::{Client, Join, JoinStage};
    use rustyac_net::handshake::JoinRequest;
    use rustyac_net::transport::{TapePlayer, Transport};

    /// A transport on which an answer only arrives after it was asked for twice in vain
    /// (as in a browser, where the answer comes some frames later).
    struct Late {
        inner: TapePlayer,
        idle: u32,
    }
    impl Transport for Late {
        fn send_reliable(&mut self, message: &[u8]) -> Result<(), String> {
            self.idle = 0;
            self.inner.send_reliable(message)
        }
        fn send_datagram(&mut self, message: &[u8]) -> Result<(), String> {
            self.inner.send_datagram(message)
        }
        fn poll_reliable(&mut self) -> Result<Option<Vec<u8>>, String> {
            self.idle += 1;
            if self.idle <= 2 {
                return Ok(None);
            }
            // (one answer per question, as the server gives them)
            self.idle = u32::MIN;
            self.inner.wait_reliable(0)
        }
        fn poll_datagram(&mut self) -> Result<Option<Vec<u8>>, String> {
            self.inner.poll_datagram()
        }
    }

    let golden = NetGolden::from_bytes(include_bytes!("golden/laguna_join_3000.netgold")).expect("the golden file");
    let request = JoinRequest { guid: "0".to_string(), name: "replay".to_string(), ..JoinRequest::default() };
    let (waited, waited_cars) = Client::join(TapePlayer::new(&golden.tape), &request).expect("the waited join");

    let mut join = Join::start(Late { inner: TapePlayer::new(&golden.tape), idle: 0 }, &request).expect("the request");
    let mut stages = vec![join.stage()];
    let mut polls = 0;
    while join.stage() != JoinStage::Done {
        polls += 1;
        assert!(polls < 1000, "the polled join does not end");
        let stage = join.poll().expect("a poll");
        if stages.last() != Some(&stage) {
            stages.push(stage);
        }
    }
    assert_eq!(stages, [JoinStage::Handshake, JoinStage::CarList, JoinStage::Done]);
    assert!(polls > 4, "the answers were there at once: nothing was polled");
    let (polled, polled_cars) = join.finish().expect("a finished join");
    assert_eq!(polled_cars, waited_cars);
    assert!(polled_cars.len() >= 4, "the tape's server has more cars than that");
    assert_eq!(polled.accepted, waited.accepted);
    assert_eq!(polled.session_id, waited.session_id);
    assert_eq!(polled.transport.inner.sent, waited.transport.sent, "the two joins sent different messages");
    assert!(waited.transport.sent.entries.len() >= 2, "a join request and at least one car list request");
}

/// Task 26: the same excerpt the way the browser plays it: the client on the WebSocket
/// transport (one lane byte per message) over a pipe that plays the tape, the join polled.
/// Every step must still be the game's, and the numbers must be the ones of the desktop's
/// way (a tape player, a waited join). The wasm build of `examples/net_golden.rs` gives the
/// same line under wasmtime (docs/port/multiplayer_web.md). NOT TESTED without the AC folder.
#[test]
fn golden_laguna_join_the_browsers_way() {
    use rustyac_net::client::{Client, Join, JoinStage};
    use rustyac_net::replay::replay_request;
    use rustyac_net::transport::TapePlayer;
    use rustyac_net::ws::{TapePipe, WsTransport};

    let golden = NetGolden::from_bytes(include_bytes!("golden/laguna_join_3000.netgold")).expect("the golden file");
    if ["ks_ferrari_f2004", "bmw_z4_gt3"].iter().any(|car| !cardata().join(car).join("suspensions.ini").is_file()) {
        eprintln!("NOT TESTED: cardata/ is missing (the cars' extracted data files)");
        return;
    }
    let Some(root) = game_root() else {
        eprintln!("NOT TESTED: Assetto Corsa's folder is missing (set AC_ROOT): the online golden replay needs the track ks_laguna_seca and the cars' collider.kn5");
        return;
    };
    let paths = ReplayPaths { cardata: cardata(), game_root: Some(root) };
    let desktop = match golden.replay_on(&paths, |tape| Client::join(TapePlayer::new(tape), &replay_request()).map_err(|e| e.to_string())) {
        Ok(report) => report,
        Err(message) if message.contains("track") => {
            eprintln!("NOT TESTED: the track could not be loaded ({message})");
            return;
        }
        Err(message) => panic!("the replay: {message}"),
    };
    let browser = golden
        .replay_on(&paths, |tape| {
            let mut join = Join::start(WsTransport::new(TapePipe::new(tape)), &replay_request()).map_err(|e| e.to_string())?;
            for _ in 0..10_000 {
                if join.poll().map_err(|e| e.to_string())? == JoinStage::Done {
                    return join.finish().ok_or("the join did not end".to_string());
                }
            }
            Err("the polled join did not end".to_string())
        })
        .expect("the replay on the WebSocket transport");
    assert_eq!(browser.first_difference, None, "on the WebSocket transport the port leaves the game's values at this step");
    assert_eq!(desktop.first_difference, None);
    assert!(browser.steps >= 3000 && browser.sent_compared > 100, "{browser:?}");
    assert_eq!((browser.steps, browser.digest, browser.sent, browser.sent_digest, browser.join_messages), (desktop.steps, desktop.digest, desktop.sent, desktop.sent_digest, desktop.join_messages));
    // what the two transports kept of the outgoing side is the same tape
    assert_eq!(browser.transport_tape.as_ref().map(|tape| &tape.entries), desktop.transport_tape.as_ref().map(|tape| &tape.entries));
}
