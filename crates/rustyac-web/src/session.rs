// SPDX-License-Identifier: GPL-3.0-or-later

//! One drive in the browser: the simulation, its clock and its camera.
//!
//! **The loop.** The physics runs at AC's 333 Hz (3 ms steps) from a fixed-step accumulator
//! that the page's `requestAnimationFrame` feeds with the time since its last frame. There
//! is no thread and no timer: every step of a frame runs inside that frame's callback, back
//! to back, and the picture is drawn between the last two steps (blended by what is left in
//! the accumulator), as the desktop's window draws between the physics thread's last two.
//!
//! **A slow frame.** A frame that took longer than [`MAX_FRAME_SECONDS`] (a hidden tab that
//! comes back, a stall while the browser compiles shaders) is counted as that long and no
//! longer: at most 34 steps are caught up and the rest of the time is dropped, so the car
//! falls behind the wall clock instead of the browser freezing while it catches up. It is the
//! desktop physics thread's own rule (more than 100 ms late: start the schedule again).
//! A machine that cannot do 333 steps a second at all therefore runs in slow motion; every
//! step is still a whole 3 ms step, so the physics itself is never different.

use std::sync::{Arc, Mutex};

use rustyac_game::input_file::{event, GridPlace, SimSetup};
use rustyac_game::online::OnlineSession;
use rustyac_game::render::scene::{CameraFrame, CarShape, DrivingCamera};
use rustyac_game::sim::{GameSim, SpawnSequence, DT};
use rustyac_game::view::CarView;

use crate::driver::{Input, SharedInput, WebSource};

/// The longest stretch of time one frame may hand to the physics, seconds.
pub const MAX_FRAME_SECONDS: f64 = 0.1;

/// Steps of lying still on its roof or side before the car is put back on the road: 3 s.
const FLIPPED_STEPS: u32 = 1000;

/// What a frame did, for the page's counters.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FrameStats {
    /// Physics steps run in this frame.
    pub steps: u32,
    /// Seconds of this frame that were dropped (the frame was longer than the cap).
    pub dropped_seconds: f64,
}

pub struct Session {
    pub sim: GameSim,
    pub input: SharedInput,
    pub shape: CarShape,
    pub camera: DrivingCamera,
    /// The keys and pad buttons in use, as text (the built-in layout).
    pub keys: String,
    /// The virtual-key codes the built-in layout uses.
    pub keys_used: Vec<i32>,
    accumulator: f64,
    prev: CarView,
    curr: CarView,
    /// The step count when the spawn sequence ended.
    drive_start: u64,
    flipped: u32,
    floor: f32,
    ceiling: f32,
    /// Steps and dropped time over the whole drive.
    pub total_steps: u64,
    pub total_dropped_seconds: f64,
    /// Online: the other players' cars, in the session's order: their shapes, and where each
    /// was after the last two steps of the last frame.
    pub remote_shapes: Vec<CarShape>,
    remote_prev: Vec<CarView>,
    remote_curr: Vec<CarView>,
    /// Online: what the session printed (joins, laps, chat ...), until the page takes it.
    pub console: Vec<String>,
    /// A drive from AC's launcher: what its race.ini made of the session (for the page's
    /// notes), and the drive as `key=value` lines (car, skin, track, layout, spawn, session).
    pub race_notes: Vec<String>,
    pub drive: String,
}

impl Session {
    /// Builds the car on the track and puts it down (the spawn sequence runs to its end here:
    /// 470 steps of settling and selecting first gear, unpaced as on the desktop).
    pub fn new(car: &str, track: &str, layout: &str, auto_shifter: bool, spawn: &str) -> Result<Session, String> {
        Session::with_sessions(car, track, layout, auto_shifter, spawn, None)
    }

    /// Task 28: the same with the referee and a session list (`p10,q5,r3`, as
    /// `rustyac.exe --sessions`): the first session decides where the car starts.
    pub fn with_sessions(car: &str, track: &str, layout: &str, auto_shifter: bool, spawn: &str, sessions: Option<&str>) -> Result<Session, String> {
        let mut setup = crate::content::setup(car, track, layout, auto_shifter, spawn);
        if let Some(list) = sessions.filter(|list| !list.is_empty()) {
            let sessions = rustyac_physics::race::SessionsIni::from_list(list, 1, true, None, setup.env.base_pit_penalty_laps).map_err(|e| format!("sessions: {e}"))?;
            setup.session.session_type = Some(sessions.sessions[0].session_type);
            setup.spawn = if sessions.sessions[0].spawn_set == "START" { "start" } else { "pit" }.to_string();
            setup.env.allowed_tyres_out = sessions.allowed_tyres_out();
            // (OVER_TIME_MULT as the game ships it)
            setup.referee = Some(rustyac_game::input_file::RefereeHeader { sessions, over_time_mult: 1.2, first_start_delay_ms: 20000.0, next_start_delay_ms: 10000.0, seed: 28_000, wind_base: None, driver_names: Vec::new(), pit: Some(rustyac_game::input_file::PitHeader { window: (0, 0), settings: rustyac_physics::race::QuickMenuSettings { stay_in_car: false, auto_app_on_pitlane: true, presets_count: 3, visibility_max_time: 5.0 , cursor_start: 0}, values: None, presets_ini: None }) });
        }
        Session::build(setup, None)
    }

    /// Task 37b: the drive AC's launcher asked for: the session of its `race.ini` (and
    /// `assists.ini`), as `rustyac.exe` makes it from the same files ([`crate::race`]).
    /// `seed`: the game's `rand()` seed (the desktop takes the clock's milliseconds).
    pub fn from_race(race_text: &str, assists_text: Option<&str>, seed: u32) -> Result<Session, String> {
        let (setup, race_notes) = crate::race::setup_of(race_text, assists_text, seed)?;
        let drive = format!(
            "car={}
skin={}
track={}
layout={}
spawn={}
session_type={}
sessions={}
",
            setup.car,
            setup.player_skin.as_deref().unwrap_or(""),
            setup.track,
            setup.layout,
            setup.spawn,
            setup.session.session_type.unwrap_or(0),
            setup.referee.as_ref().map(|r| r.sessions.sessions.iter().map(|s| s.name.clone()).collect::<Vec<_>>().join(",")).unwrap_or_default()
        );
        let mut session = Session::build(setup, None)?;
        session.race_notes = race_notes;
        session.drive = drive;
        Ok(session)
    }

    /// The same on a server: the server's car and track, in the pit box of the player's
    /// slot, with the other players' cars. `online`: a finished join
    /// (`rustyac_game::online::finish_join`). What the desktop's `join_server` and
    /// `online_setup` do.
    pub fn new_online(online: OnlineSession, auto_shifter: bool, spawn: &str) -> Result<Session, String> {
        let accepted = online.net.client.accepted.clone();
        let mut setup = crate::content::setup(&accepted.model, &accepted.track, &accepted.track_config, auto_shifter, spawn);
        setup.player_skin = Some(accepted.skin.clone());
        // CarAvatar::setSpawnPositionIndex("PIT", sessionID) / ("START", handshakeCarPosition)
        setup.player_place = Some(GridPlace { pit: accepted.session_id as usize, start: accepted.current_session.handshake_position.map_or(-1, |p| p as i32), ballast_kg: 0.0, restrictor: 0.0 });
        // the server's weather comes with its first weather message; its grip with the session
        setup.session.dynamic_track = None;
        setup.session.ballast_kg = 0.0;
        setup.session.restrictor = 0.0;
        Session::build(setup, Some(online))
    }

    fn build(setup: SimSetup, online: Option<OnlineSession>) -> Result<Session, String> {
        let input: SharedInput = Arc::new(Mutex::new(Input::default()));
        let source = WebSource::new(input.clone());
        let keys = source.bindings.describe();
        let keys_used = source.keys_used();
        let source = SpawnSequence::new(source, true);
        let mut sim = match online {
            Some(online) => GameSim::new_online(setup, Box::new(source), online)?,
            None => GameSim::new(setup, Box::new(source))?,
        };
        while sim.car.device.source.in_spawn_sequence() {
            sim.step()?;
        }
        // the other cars: race.ini's (AI drivers) first, then the server's, as `opponent_infos`
        let remote_shapes: Vec<CarShape> = sim.opponent_infos().iter().map(CarShape::of).collect();
        let remote: Vec<CarView> = (0..remote_shapes.len()).map(|index| other_view(&sim, index, 0.0)).collect();
        let shape = CarShape::of(&sim.car_info());
        let (floor, ceiling) = match &sim.track {
            Some(track) if !track.world.meshes.is_empty() => {
                let lowest = track.world.meshes.iter().map(|mesh| mesh.aabb[2]).fold(f32::MAX, f32::min);
                let highest = track.world.meshes.iter().map(|mesh| mesh.aabb[3]).fold(f32::MIN, f32::max);
                (lowest - 20.0, highest + 500.0)
            }
            _ => (-3.0, 500.0),
        };
        let view = CarView::capture(&sim, 0.0);
        Ok(Session {
            drive_start: sim.steps,
            sim,
            input,
            shape,
            camera: DrivingCamera::chase(),
            keys,
            keys_used,
            accumulator: 0.0,
            prev: view,
            curr: view,
            flipped: 0,
            floor,
            ceiling,
            total_steps: 0,
            total_dropped_seconds: 0.0,
            remote_shapes,
            remote_prev: remote.clone(),
            remote_curr: remote,
            console: Vec::new(),
            race_notes: Vec::new(),
            drive: String::new(),
        })
    }

    fn step(&mut self) -> Result<(), String> {
        self.sim.step()?;
        let burst = self.sim.car.device.source.in_spawn_sequence();
        if burst {
            self.drive_start = self.sim.steps;
        }
        // as the desktop's physics thread: a car at rest on its roof or side for three
        // seconds, out of the world or with broken numbers is put back
        let body = self.sim.car.car.core.get_world_matrix(self.sim.car.car.body).m;
        let broken = body.iter().flatten().any(|x| !x.is_finite());
        let spin = self.sim.car.car.core.get_angular_velocity(self.sim.car.car.body);
        let at_rest = self.sim.car.car.speed < 0.5 && spin.x * spin.x + spin.y * spin.y + spin.z * spin.z < 0.25;
        self.flipped = if body[1][1] < 0.2 && at_rest { self.flipped + 1 } else { 0 };
        if broken {
            self.sim.car.device.source.request(event::REBUILD);
        } else if body[3][1] < self.floor || body[3][1] > self.ceiling || self.flipped > FLIPPED_STEPS {
            self.sim.car.device.source.request(if self.sim.track.is_some() { event::TO_TRACK } else { event::RESET });
            self.flipped = 0;
        }
        let view = CarView::capture(&self.sim, (self.sim.steps - self.drive_start) as f64 * DT as f64);
        self.prev = if burst { view } else { self.curr };
        self.curr = view;
        self.total_steps += 1;
        if let Some(session) = &mut self.sim.online {
            self.console.append(&mut session.console);
            if self.console.len() > 200 {
                self.console.drain(..100);
            }
            if session.net.client.is_lost() {
                return Err("the connection to the server is gone".to_string());
            }
        }
        // the other cars: only the last two steps of a frame are ever drawn between
        if !self.remote_curr.is_empty() && (self.accumulator < 2.0 * DT as f64 || burst) {
            let seconds = (self.sim.steps - self.drive_start) as f64 * DT as f64;
            for index in 0..self.remote_curr.len() {
                let view = other_view(&self.sim, index, seconds);
                self.remote_prev[index] = if burst { view } else { self.remote_curr[index] };
                self.remote_curr[index] = view;
            }
        }
        Ok(())
    }

    /// One frame of the page: `seconds` since its last frame.
    pub fn advance(&mut self, seconds: f64) -> Result<FrameStats, String> {
        let mut stats = FrameStats::default();
        let seconds = if seconds.is_finite() { seconds.max(0.0) } else { 0.0 };
        let counted = seconds.min(MAX_FRAME_SECONDS);
        stats.dropped_seconds = seconds - counted;
        self.total_dropped_seconds += stats.dropped_seconds;
        self.accumulator += counted;
        while self.accumulator >= DT as f64 {
            self.accumulator -= DT as f64;
            self.step()?;
            stats.steps += 1;
            // a car that was just put back (R, N) settles unpaced, as on the desktop and as at
            // the start: the 470 steps of the spawn sequence run inside this frame
            let mut burst = 0;
            while self.sim.car.device.source.in_spawn_sequence() && burst < 2000 {
                self.step()?;
                stats.steps += 1;
                burst += 1;
            }
        }
        Ok(stats)
    }

    /// The car between the last two steps, where the page's clock is now.
    pub fn view(&self) -> CarView {
        CarView::between(&self.prev, &self.curr, (self.accumulator / DT as f64) as f32)
    }

    /// The other cars between the last two steps: race.ini's, then the other players', in the
    /// session's order.
    pub fn remote_views(&self) -> Vec<CarView> {
        let blend = (self.accumulator / DT as f64) as f32;
        self.remote_prev.iter().zip(&self.remote_curr).map(|(prev, curr)| CarView::between(prev, curr, blend)).collect()
    }

    /// The camera for `view`, `seconds` after the last frame's.
    pub fn camera_frame(&mut self, view: &CarView, seconds: f64) -> CameraFrame {
        let toggles = std::mem::take(&mut self.input.lock().unwrap().camera_toggles);
        for _ in 0..toggles {
            self.camera.f1();
        }
        let seconds = if seconds.is_finite() { seconds.clamp(0.0, MAX_FRAME_SECONDS) } else { 0.0 };
        self.camera.update(view, &self.shape, view.acc_g, seconds as f32)
    }

    /// New settings from the page's Controls panel (a `controls.ini`, see [`crate::controls`]):
    /// AC's keyboard and pad classes are built again from them before the next physics step.
    /// Only what the player's own devices make of a key or a stick changes.
    pub fn set_controls(&mut self, ini: &rustyac_game::input::ini::ControlsIni, origin: &str) {
        let source = WebSource::with_controls(Arc::default(), ini, origin);
        self.keys = source.bindings.describe();
        self.keys_used = source.keys_used();
        self.input.lock().unwrap().controls = Some(ini.clone());
    }

    /// R, Shift+R, N and the other one-off commands (`rustyac_game::input_file::event` bits).
    pub fn request(&mut self, events: u32) {
        self.input.lock().unwrap().requests |= events;
    }

    /// One of the page's command keys by its `KeyboardEvent.code` name, with `Shift+` in front
    /// for the shifted one: what `web/app.js` does on that key, for a check that has no page
    /// (`examples/selftest.rs --keys`). False for a key that is no command.
    pub fn press(&mut self, name: &str) -> bool {
        let (shift, code) = match name.strip_prefix("Shift+") {
            Some(code) => (true, code),
            None => (false, name),
        };
        match code {
            "KeyT" => self.request(if shift { event::TC_DN } else { event::TC_UP }),
            "KeyY" => self.request(if shift { event::ABS_DN } else { event::ABS_UP }),
            "BracketRight" => self.request_bias(1),
            "BracketLeft" => self.request_bias(-1),
            "KeyR" => self.request(if shift { event::TO_TRACK } else { event::RESET }),
            "KeyN" => self.request(event::REBUILD),
            // Task 28: the referee's two buttons
            "F7" => self.request(event::NEXT_SESSION),
            "F8" => self.request(event::RESTART_SESSION),
            // Task 28b: the pit menu
            "Numpad4" => self.request(event::PIT_LEFT),
            "Numpad8" => self.request(event::PIT_UP),
            "Numpad6" => self.request(event::PIT_RIGHT),
            "Numpad2" => self.request(event::PIT_DOWN),
            "KeyG" => self.request(event::AUTO_SHIFTER),
            _ => return false,
        }
        true
    }

    /// Clicks of the cockpit's brake-bias control (] forward, [ rearward), before the next step.
    pub fn request_bias(&mut self, clicks: i32) {
        self.input.lock().unwrap().bias_clicks += clicks;
    }
}

/// The other car `index` of the session: race.ini's cars (AI drivers) first, then the server's.
fn other_view(sim: &GameSim, index: usize, drive_seconds: f64) -> CarView {
    match index.checked_sub(sim.opponents.len()) {
        None => CarView::capture_opponent(sim, index, drive_seconds),
        Some(remote) => CarView::capture_remote(sim, remote),
    }
}
