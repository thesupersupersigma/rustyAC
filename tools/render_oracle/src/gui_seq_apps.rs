// SPDX-License-Identifier: GPL-3.0-or-later

//! The scripts of the apps' proofs (Task 32) and what both sides share of them.
//!
//! The cars' states come from tapes of the game's own physics (`car_oracle multi
//! --audio-tape`: every car's `CarPhysicsState` 60 times a second), so that the apps are fed
//! what a real session gives them. The game's side writes a frame's bytes into its
//! `CarAvatar::physicsState` blocks as they are; the port reads the same bytes with
//! `CarPhysicsState::from_game_bytes`.
//!
//! What the apps ask of `RaceManager` (laps with their sector times, places) is made here
//! from the tapes, the same for both sides: a lap is the tape's own (`lapCount`, `lastLap`,
//! `lapTime`), its sectors end where the car passes a third and two thirds of the spline,
//! the places are by laps and spline. The referee itself is Task 28's proof.

use std::sync::Mutex;

use rustyac_gui::apps::{AppCar, AppLink};
use rustyac_gui::leaderboard::RaceLap;
use rustyac_physics::car_physics_state::{CarPhysicsState, GAME_SIZE};

use crate::gui_seq::Op;
use crate::gui_seq_menu::{hud, leak};

pub const DT: f32 = 1.0 / 60.0;

/// What the stand-in referee knows of one car at one frame.
#[derive(Clone, Debug, Default)]
pub struct Timing {
    pub current_splits: Vec<u32>,
    pub best: RaceLap,
    pub last: RaceLap,
    pub split: i32,
}

#[derive(Default)]
pub struct AppScript {
    /// every car's frames: the game's `CarPhysicsState` bytes
    pub tapes: Vec<Vec<Vec<u8>>>,
    /// every car's frames: the byte after the state (traction control in action)
    pub tcs: Vec<Vec<u8>>,
    /// the cars' `physicsInfo.maxFuel`, `engineDamageRPM`; ABS and TC there and on
    pub max_fuel: f64,
    pub damage_rpm: i32,
    pub abs: bool,
    pub tc: bool,
    pub session_count: u32,
    pub time_to_wait: f64,
    /// the AI spline's length (m); air, road, grip, wind (m/s), the wind's direction
    pub spline_length: f32,
    pub env: [f32; 5],
    /// laps finished since the last frame the script went to: (car, time)
    pub pending_laps: Vec<(u32, u32)>,
    /// every car's timing at every frame of the tapes
    pub timing: Vec<Vec<Timing>>,
    /// the frame of the tapes the session is at
    pub frame: usize,
    pub focused: usize,
    pub session_type: i32,
    pub is_timed_race: bool,
    pub session_laps: i32,
    /// the session's start (ms of the tapes' clock) and length (minutes)
    pub start_time: f64,
    pub duration_minutes: f32,
    pub in_replay: bool,
    pub use_mph: bool,
    /// a car's state changed by the script after the tape's (car, name, value)
    pub sets: Vec<(usize, String, f32)>,
}

static SCRIPT: Mutex<Option<AppScript>> = Mutex::new(None);

pub fn script<T>(f: impl FnOnce(&mut AppScript) -> T) -> T {
    f(SCRIPT.lock().unwrap().get_or_insert_with(AppScript::default))
}

/// The car frames of a tape (see `frames::read_tape`): the raw bytes of every frame, and the
/// byte after each.
pub fn read_tape(path: &std::path::Path) -> Result<(Vec<Vec<u8>>, Vec<u8>), String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let head = bytes.windows(2).position(|w| w == [10u8, 10u8]).ok_or("the tape has no header")?;
    let mut at = head + 2;
    let (mut out, mut after) = (Vec::new(), Vec::new());
    while at < bytes.len() {
        match bytes[at] {
            1 => at += 1 + 0x48,
            2 => {
                out.push(bytes.get(at + 5..at + 5 + GAME_SIZE).ok_or("the tape ends inside a frame")?.to_vec());
                after.push(bytes.get(at + 5 + GAME_SIZE).copied().unwrap_or(0));
                at += 5 + GAME_SIZE + 1;
            }
            other => return Err(format!("{}: a record of kind {other} at byte {at}", path.display())),
        }
    }
    Ok((out, after))
}

/// The stand-in referee over one car's frames.
fn timing_of(frames: &[Vec<u8>]) -> Vec<Timing> {
    let mut out = Vec::with_capacity(frames.len());
    let mut t = Timing { split: -1, ..Timing::default() };
    let mut last_count = None;
    let mut last_spline = 0.0f32;
    for bytes in frames {
        let s = CarPhysicsState::from_game_bytes(bytes).unwrap_or_default();
        if last_count.is_some_and(|c| s.lap_count > c) {
            // a lap is over: the tape's own time, the sectors seen on the way
            let time = s.last_lap;
            let mut splits = std::mem::take(&mut t.current_splits);
            if splits.len() == 2 && splits[0] + splits[1] < time {
                splits.push(time - splits[0] - splits[1]);
            } else {
                splits.clear();
            }
            let lap = RaceLap { time, splits, is_valid: true };
            t.split = time as i32 - t.best.time as i32;
            if t.best.time == 0 || time < t.best.time {
                t.best = lap.clone();
            }
            t.last = lap;
        } else {
            for k in 1..=2u32 {
                let at = k as f32 / 3.0;
                if t.current_splits.len() + 1 == k as usize && last_spline < at && s.normalized_spline_position >= at && s.normalized_spline_position < at + 0.2 {
                    let done: u32 = t.current_splits.iter().sum();
                    t.current_splits.push(s.lap_time.saturating_sub(done));
                }
            }
        }
        last_count = Some(s.lap_count);
        last_spline = s.normalized_spline_position;
        out.push(t.clone());
    }
    out
}

/// A script's change of what the apps read (true: it was one).
pub fn app_input(what: &str, number: f32, text: &str) -> Result<bool, String> {
    let on = number != 0.0;
    match what {
        "app.tape" => {
            // "<prefix>": <prefix>.car<n>.audiotape for n = 0, 1, … (as many as there are)
            let (mut tapes, mut tcs) = (Vec::new(), Vec::new());
            loop {
                let path = std::path::PathBuf::from(format!("{text}.car{}.audiotape", tapes.len()));
                if !path.is_file() {
                    break;
                }
                let (t, c) = read_tape(&path)?;
                tapes.push(t);
                tcs.push(c);
            }
            if tapes.is_empty() {
                // a single car's tape of `car_oracle run --audio-tape`
                let (t, c) = read_tape(std::path::Path::new(&format!("{text}.audiotape")))?;
                tapes.push(t);
                tcs.push(c);
            }
            let timing = tapes.iter().map(|t| timing_of(t)).collect();
            script(|s| {
                s.tapes = tapes;
                s.tcs = tcs;
                s.timing = timing;
            });
        }
        "app.at" => script(|s| {
            let to = number as usize;
            // the laps finished on the way (RaceManager::evOnLapCompleted)
            let mut laps = Vec::new();
            for (car, tape) in s.tapes.iter().enumerate() {
                let at = |f: usize| CarPhysicsState::from_game_bytes(&tape[f.min(tape.len().saturating_sub(1))]).unwrap_or_default();
                if tape.is_empty() {
                    continue;
                }
                let (old, new) = (at(s.frame), at(to));
                if new.lap_count > old.lap_count && to > s.frame {
                    laps.push((car as u32, new.last_lap));
                }
            }
            s.pending_laps.extend(laps);
            s.frame = to;
        }),
        "app.spline" => script(|s| s.spline_length = number),
        "app.env" => {
            let k = ["air", "road", "grip", "wind", "windDirection"].iter().position(|n| *n == text).ok_or("app.env: air, road, grip, wind or windDirection")?;
            script(|s| s.env[k] = number);
        }
        "app.focus" => script(|s| s.focused = number as usize),
        "app.session" => script(|s| s.session_type = number as i32),
        "app.timed" => script(|s| s.is_timed_race = on),
        "app.laps" => script(|s| s.session_laps = number as i32),
        "app.start" => script(|s| s.start_time = number as f64),
        "app.duration" => script(|s| s.duration_minutes = number),
        "app.replay" => script(|s| s.in_replay = on),
        "app.mph" => script(|s| s.use_mph = on),
        "app.maxFuel" => script(|s| s.max_fuel = number as f64),
        "app.damageRpm" => script(|s| s.damage_rpm = number as i32),
        "app.abs" => script(|s| s.abs = on),
        "app.tc" => script(|s| s.tc = on),
        "app.sessions" => script(|s| s.session_count = number as u32),
        "app.wait" => script(|s| s.time_to_wait = number as f64),
        "app.set" => {
            // "car:name"
            let (car, name) = text.split_once(':').ok_or("app.set wants \"car:name\"")?;
            let car: usize = car.parse().map_err(|_| "app.set: not a car")?;
            script(|s| {
                s.sets.retain(|(c, n, _)| !(*c == car && n == name));
                s.sets.push((car, name.to_string(), number));
            });
        }
        _ => return Ok(false),
    }
    Ok(true)
}

/// A member of the game's `CarPhysicsState` a script may change: name, offset, kind.
const STATE_FIELDS: [(&str, usize, char); 0] = [];

/// The bytes of one car's state as the session stands (the tape's frame, then the script's
/// changes); none: the script has no tapes.
pub fn raw_state(s: &AppScript, car: usize) -> Option<Vec<u8>> {
    let tape = s.tapes.get(car)?;
    let mut bytes = tape.get(s.frame.min(tape.len().saturating_sub(1)))?.clone();
    for (c, name, value) in &s.sets {
        if *c != car {
            continue;
        }
        if let Some((_, at, kind)) = STATE_FIELDS.iter().find(|f| f.0 == name) {
            match kind {
                'f' => bytes[*at..*at + 4].copy_from_slice(&value.to_le_bytes()),
                'i' => bytes[*at..*at + 4].copy_from_slice(&(*value as i32).to_le_bytes()),
                _ => bytes[*at] = (*value != 0.0) as u8,
            }
        }
    }
    Some(bytes)
}

/// The name of a car of the tapes.
pub fn driver_name(car: usize) -> String {
    ["Kimi Raikkonen", "Driver Number Two", "Lewis", "A Driver With A Very Long Name", "Mika", "Juan Pablo"].get(car).map(|n| n.to_string()).unwrap_or_else(|| format!("Driver {}", car + 1))
}

/// The laps the script's last move finished, for each side to announce.
pub fn take_laps() -> Vec<(u32, u32)> {
    script(|s| std::mem::take(&mut s.pending_laps))
}

/// The tapes' clock (ms) at a frame.
pub fn now(frame: usize) -> f64 {
    frame as f64 * (1000.0 / 60.0)
}

/// What the apps read, the same on both sides.
pub fn link() -> AppLink {
    script(|s| {
        let states: Vec<CarPhysicsState> = (0..s.tapes.len()).map(|car| raw_state(s, car).and_then(|b| CarPhysicsState::from_game_bytes(&b).ok()).unwrap_or_default()).collect();
        // the places: laps, then the spline
        let mut order: Vec<usize> = (0..states.len()).collect();
        order.sort_by(|a, b| {
            let (x, y) = (&states[*a], &states[*b]);
            (y.lap_count as f32 + y.normalized_spline_position).partial_cmp(&(x.lap_count as f32 + x.normalized_spline_position)).unwrap_or(std::cmp::Ordering::Equal).then(a.cmp(b))
        });
        let cars = states
            .iter()
            .enumerate()
            .map(|(i, state)| {
                let t = s.timing.get(i).and_then(|t| t.get(s.frame.min(t.len().saturating_sub(1)))).cloned().unwrap_or_default();
                let place = order.iter().position(|c| *c == i).unwrap_or(0) as i32;
                AppCar {
                    state: *state,
                    current_lap: RaceLap { time: state.lap_time, splits: t.current_splits.clone(), is_valid: false },
                    best_lap: t.best.clone(),
                    last_lap: t.last.clone(),
                    split: t.split,
                    current_lap_valid: true,
                    lap_count: state.lap_count,
                    leaderboard_position: place + 1,
                    realtime_position: place,
                    max_fuel: s.max_fuel,
                    engine_damage_rpm: s.damage_rpm,
                    abs_enabled: s.abs,
                    // (ABS works when a braked wheel slips: the game's absOverride is not on the tape)
                    abs_in_action: s.abs && state.brake > 0.1 && state.slip_ratio.iter().any(|r| *r < -0.1),
                    tc_enabled: s.tc,
                    tc_in_action: s.tc && s.tcs.get(i).and_then(|t| t.get(s.frame.min(t.len().saturating_sub(1)))).is_some_and(|b| *b != 0),
                    driver_name: driver_name(i),
                    connected: true,
                    ..AppCar::default()
                }
            })
            .collect();
        let now = now(s.frame);
        AppLink {
            cars,
            focused_car: s.focused,
            connected_cars_count: s.tapes.len() as i32,
            session_type: s.session_type,
            is_timed_race: s.is_timed_race,
            session_laps: s.session_laps,
            session_count: s.session_count,
            time_to_wait: s.time_to_wait,
            has_p2p: false,
            session_time_left: if s.in_replay { -1.0 } else { ((s.duration_minutes as f64 * 60.0 * 1000.0) + s.start_time) - now },
            time_to_session_start: s.start_time - now,
            in_replay: s.in_replay,
            ai_spline_length: s.spline_length,
            ambient_temperature: s.env[0],
            road_temperature: s.env[1],
            grip: s.env[2],
            wind_speed: s.env[3],
            wind_direction_deg: s.env[4],
            game_time: now,
            use_mph: s.use_mph,
            actions: Vec::new(),
        }
    })
}

/// The frames `from..to` of the tapes, one drawn every `every`.
pub fn play(o: &mut Vec<Op>, from: usize, to: usize, every: usize) {
    let mut f = from;
    while f < to {
        o.push(hud("app.at", f as f32, ""));
        o.push(Op::Frame(DT));
        f += every.max(1);
    }
}

/// A tape's path from the repository's folder (the sides run in the scratch root).
fn tape(path: &str) -> &'static str {
    let here = std::env::current_dir().unwrap_or_default();
    leak(here.join(path).to_string_lossy().into_owned())
}

/// The session of the race tapes (`oracle/t32/race`, Task 28's `rc_race`: 6 cars, 3 laps).
fn race(o: &mut Vec<Op>, cars: usize) {
    o.push(hud("app.create", cars as f32, ""));
    o.push(hud("app.tape", 0.0, tape("oracle/t32/race/rc_race")));
    o.extend([hud("app.session", 3.0, ""), hud("app.laps", 3.0, ""), hud("app.start", 5000.0, ""), hud("app.duration", 0.0, ""), hud("app.sessions", 1.0, "")]);
    // the F2004's tank and engine (car.ini [FUEL] MAX_FUEL, engine.ini [DAMAGE] RPM_THRESHOLD)
    o.extend([hud("app.maxFuel", 150.0, ""), hud("app.damageRpm", 20200.0, ""), hud("app.abs", 0.0, ""), hud("app.tc", 1.0, "")]);
    // Magione's AI spline, the car oracle's weather (26 C air, 30 C road, no wind)
    o.extend([hud("app.spline", 2507.6, ""), hud("app.env", 26.0, "air"), hud("app.env", 30.0, "road"), hud("app.env", 0.97, "grip"), hud("app.env", 0.0, "wind"), hud("app.env", 0.0, "windDirection")]);
}

/// Mouse clicks and drags on an app: moved by its title, pinned, scaled up and down, hidden
/// from the app bar and shown again.
fn handle(o: &mut Vec<Op>, x: i32, y: i32, w: i32) {
    let click = |o: &mut Vec<Op>, x: i32, y: i32| o.extend([Op::MouseMove(x, y), Op::MouseDown(x, y), Op::MouseUp(x, y), Op::Frame(DT)]);
    // dragged by its title
    o.extend([Op::MouseMove(x + w / 2, y + 10), Op::MouseDown(x + w / 2, y + 10), Op::MouseMove(x + w / 2 + 60, y + 50), Op::Frame(DT), Op::MouseMove(x + w / 2 + 90, y + 80), Op::Frame(DT), Op::MouseUp(x + w / 2 + 90, y + 80), Op::Frame(DT)]);
    let (x, y) = (x + 90, y + 70);
    // the buttons above its right edge: zoom in, zoom out, pin
    click(o, x + w - 13, y - 14);
    click(o, x + w - 13 - 26, y - 14);
    click(o, x + w - 13 - 52, y - 14);
    click(o, x + w - 13 - 26, y - 14);
}

pub fn sequence(name: &str) -> Option<Vec<Op>> {
    let mut o = Vec::new();
    let (app, rest) = name.split_once('@').unwrap_or((name, ""));
    match app {
        "app_laps" | "app_position" | "app_essentials" | "app_delta" | "app_realtime" => {
            let which = leak(app[4..].to_string());
            race(&mut o, 6);
            o.push(hud("app.new", 0.0, which));
            o.push(Op::Frame(DT));
            o.push(hud("app.visible", 1.0, which));
            // the grid, the start, the first laps, a car's best lap, the end
            play(&mut o, 0, 600, 60);
            play(&mut o, 600, 1400, 4);
            if rest != "short" {
                play(&mut o, 1400, 21000, 97);
                o.push(hud("app.focus", 3.0, ""));
                play(&mut o, 9000, 9400, 7);
                o.push(hud("app.focus", 0.0, ""));
                // ABS on, miles an hour, a wait before a race's end
                o.extend([hud("app.abs", 1.0, ""), hud("app.mph", 1.0, "")]);
                play(&mut o, 15000, 15600, 6);
                o.extend([hud("app.abs", 0.0, ""), hud("app.mph", 0.0, ""), hud("app.timed", 1.0, ""), hud("app.start", 400000.0, ""), hud("app.wait", 65000.0, ""), Op::Frame(DT), hud("app.wait", 0.0, ""), Op::Frame(DT), hud("app.timed", 0.0, ""), hud("app.start", 5000.0, "")]);
                handle(&mut o, 0, 80, 300);
                o.extend([hud("app.replay", 1.0, ""), Op::Frame(DT), hud("app.replay", 0.0, ""), hud("app.timed", 1.0, ""), hud("app.duration", 2.0, ""), Op::Frame(DT)]);
                play(&mut o, 12000, 12600, 30);
                // some wind, a car in the pits
                o.extend([hud("app.env", 4.2, "wind"), hud("app.env", 137.0, "windDirection"), Op::Frame(DT), hud("app.env", 11.9, "wind"), hud("app.env", 290.5, "windDirection"), Op::Frame(DT)]);
                o.extend([hud("app.session", 1.0, ""), Op::Frame(DT), hud("app.sessions", 3.0, ""), Op::Frame(DT), hud("app.session", 2.0, ""), Op::Frame(DT), hud("app.duration", 0.0, ""), Op::Frame(DT), hud("app.session", 4.0, ""), Op::Frame(DT), hud("app.session", 0.0, ""), Op::Frame(DT)]);
                o.extend([hud("app.visible", 0.0, which), Op::Frame(DT), hud("app.visible", 1.0, which), Op::Frame(DT), Op::Save]);
            }
        }
        _ => return None,
    }
    Some(o)
}
