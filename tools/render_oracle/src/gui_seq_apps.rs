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
/// Where the Audio app's form is on the screen (its top left).
const VOLUMES_AT: (i32, i32) = (0, 80);
/// Where the force feedback app's form is.
const FFB_AT: (i32, i32) = (0, 80);

/// What the stand-in referee knows of one car at one frame.
#[derive(Clone, Debug, Default)]
pub struct Timing {
    pub current_splits: Vec<u32>,
    pub best: RaceLap,
    pub last: RaceLap,
    pub split: i32,
}

/// Task 33b: FormTelemetry's stand-in car (an AWD2 with both dynamic anti-roll bars), the same
/// for both sides.
pub const TELEMETRY_CAR: rustyac_gui::app_telemetry::TelemetryCar = rustyac_gui::app_telemetry::TelemetryCar { limiter_rpm: 7800, clutch_max_torque: 450.0, awd2: true, arb_ready: [true, true] };

/// TyreWearingDebug's wear curve (km, grip), every tyre's: the game's own `Curve` on its side.
pub const TYRE_WEAR_CURVE: [(f32, f32); 5] = [(0.0, 0.98), (10.0, 1.0), (25.0, 0.985), (60.0, 0.93), (120.0, 0.81)];

/// TyreWearingDebug's tyres of a frame: the distance runs past the curve's end, the core
/// temperatures through the ranges where %g changes its shape, the thermal model on and off.
/// The Tyre tester's car: slow (15 m/s is used below 30), then fast; the loads apart per wheel;
/// the compounds changed after frame 400.
pub fn tyre_tester_car(frame: usize) -> rustyac_gui::app_tyre_tester::TyreTesterCar {
    let f = frame as f32;
    rustyac_gui::app_tyre_tester::TyreTesterCar {
        speed: if frame < 200 { 20.0 } else { 30.0 + f * 0.05 },
        load: [2500.0 + f * 3.0, 2700.0 + f * 2.0, 3300.0 - f, 3600.0 + f * 0.5],
        compound: if frame < 400 { [0; 4] } else { [1, 1, 2, 2] },
    }
}

pub fn tyre_debug_rows(frame: usize) -> [rustyac_gui::app_tyre_debug::TyreDebugRow; 4] {
    let mut curve = rustyac_physics::curve::Curve::new();
    for (x, y) in TYRE_WEAR_CURVE {
        curve.add_value(x, y);
    }
    std::array::from_fn(|k| {
        let virtual_km = frame as f64 * 0.37 + k as f64 * 5.3;
        rustyac_gui::app_tyre_debug::TyreDebugRow {
            virtual_km,
            wear_value: curve.get_value(virtual_km as f32),
            core_temp: ((frame * 7 + k * 13) % 90) as f32 * 1.37 - 4.0,
            active: (frame / 40 + k) % 2 == 0,
        }
    })
}

/// SetupDebug's members of the `Car`: pressures and rod lengths through the floor-to-four-decimals
/// rounding (negative ones too), three wings, R, N and six gears.
pub fn setup_debug_car(frame: usize) -> rustyac_gui::app_setup_debug::SetupDebugCar {
    let f = frame as f32;
    rustyac_gui::app_setup_debug::SetupDebugCar {
        pressure_static: [26.5 + f * 0.0137, 26.623456 + f * 0.0011, 27.0, -(f * 0.0731)],
        wing_angles: vec![f * 0.05 - 5.0, 12.25, -3.333333],
        rod_lengths: [0.01234, 0.013441, -0.004321 * (frame % 5) as f32, 0.1 + f * 0.00007],
        gear_ratios: vec![-3.2, 0.0, 3.1, 2.4, 1.9, 1.55, 1.3, 1.1],
        final_ratio: 3.875 + f * 0.001,
        max_gear: 6,
        tyre_compound: "Semislicks (SM)".to_string(),
    }
}

/// CarPhysicsForm's raw inputs of a frame: suspensions 0 and 2's base z, the body's mass and
/// inertia y, the fuel evaluator's metres and litres (none for the first 100 frames: 0 / 0, then
/// 50 more of m / 0).
pub fn car_physics_raw(frame: usize) -> ([f32; 2], f32, f32, f64, f64) {
    let f = frame as f32;
    let (m, l) = if frame < 100 { (0.0, 0.0) } else if frame < 150 { (1234.5, 0.0) } else { (frame as f64 * 41.7, frame as f64 * 0.0123) };
    ([1.42 + f * 0.0001, -(1.18 + f * 0.0002)], 1240.0 + f * 0.5, 1610.0 - f * 0.25, m, l)
}

/// CarPhysicsForm's numbers beside the physics state as the port gets them.
pub fn car_physics_extra(frame: usize) -> rustyac_gui::app_car_physics::CarPhysicsExtra {
    let (base_z, mass, iyy, m, l) = car_physics_raw(frame);
    rustyac_gui::app_car_physics::CarPhysicsExtra { base_z, mass, iyy, km_per_liter: (m * 0.001 / l) as f32, filtered_speed: (frame as f32 * 0.37).sin() * 40.0 + 120.0 }
}

/// ACDCarEngineer's inputs of a frame: the front heave spring standing in for a zero front spring
/// for the first 60 frames, a zero rear spring (infinite ratios) for the next 60, then springs and
/// dampers moving through the precision-2 `%g`'s shapes.
pub fn car_engineer(frame: usize) -> rustyac_gui::app_car_engineer::CarEngineer {
    let f = frame as f32;
    let k = [if frame < 60 { 0.0 } else { 80000.0 + f * 37.0 }, if (60..120).contains(&frame) { 0.0 } else { 95000.0 - f * 21.0 }];
    rustyac_gui::app_car_engineer::CarEngineer {
        total_mass: 1234.5 + f * 0.01,
        body_mass: 1120.0,
        unsprung: [27.25, 27.5, 30.125, 1234567.0],
        k,
        z: [1.31 + f * 0.0001, -(1.42 - f * 0.0002)],
        mass: 1100.0 + f * 0.75,
        dampers: [[4500.0 + f * 3.0, 2800.0 + f], [5200.0 - f * 2.0, 99.0 + f * 9.0]],
        heave: (frame < 60).then_some([120000.0, 3000.0, 1500.0 + f * 11.0]),
    }
}

/// FormWings' wings of a frame: three level wings and a fin, the angles, heights and forces
/// moving through the table's shapes (no lift for the first 30 frames: the front share and the
/// efficiency divide by zero).
/// ACSuspension's facts (an E30-like car) and wheel nodes a little off the tyres, more so
/// as the frames pass.
pub fn suspension_link(state: &CarPhysicsState, frame: usize) -> rustyac_gui::app_suspension::SuspensionLink {
    let k = frame as f32 * 0.0005;
    rustyac_gui::app_suspension::SuspensionLink {
        facts: rustyac_gui::app_suspension::SuspensionFacts {
            caster: 6.25,
            kpi_rad: 0.2232,
            scrub: 0.0127,
            sus_base_pos: [[-0.72, -0.2, 1.2], [0.72, -0.2, 1.2], [-0.71, -0.21, -1.36], [0.71, -0.21, -1.36]],
            ride_pickup_point: [[0.0, -0.1, 1.2], [0.0, -0.1, -1.36]],
            tyre_width: [0.205; 4],
            tyre_radius: [0.31; 4],
            fuel_tank_position: [0.0, -0.1, -1.0],
        },
        wheel_nodes: std::array::from_fn(|i| {
            let t = state.tyre_matrix[i].m[3];
            [t[0] + k * i as f32, t[1] - k, t[2] + 0.001 * i as f32]
        }),
    }
}

/// Task 43c: `GPUProfiler::getResults` rows of a frame: none, one, eight at one time (nothing
/// drawn), then eight passes moving (the first row's time is its timestamp, as in the game).
pub const GPU_NAMES: [&str; 8] = ["BEGIN_FRAME", "SHADOW_MAP", "CUBEMAP", "MAIN_PASS", "TRANS_PASS", "GUI", "END_FRAME", "PRESENT"];
pub fn gpu_rows(frame: usize) -> Vec<rustyac_gui::app_gpu_profiler::GpuQueryRow> {
    let count = match frame {
        0..30 => 0,
        30..60 => 1,
        _ => 8,
    };
    let f = frame as f64;
    let mut ts = 1000.0 + f * 16.7;
    let mut previous = 0.0;
    (0..count)
        .map(|i| {
            if (90..).contains(&frame) {
                ts += [0.0, 0.85, 0.41, 3.2 + (f * 0.05).sin(), 1.1, 0.6 + f * 0.001, 0.05, 0.3][i];
            }
            let row = rustyac_gui::app_gpu_profiler::GpuQueryRow { name: GPU_NAMES[i].to_string(), last_result: ts - previous, last_ts: ts };
            previous = ts;
            row
        })
        .collect()
}

pub fn wing_rows(frame: usize) -> Vec<rustyac_gui::app_wings::WingRow> {
    use rustyac_physics::car::aero::WingState;
    let f = frame as f32;
    let lift = if frame < 30 { 0.0 } else { 1.0 };
    let wing = |name: &str, chord: f32, span: f32, z: f32, vertical: bool, k: f32| rustyac_gui::app_wings::WingRow {
        name: name.to_string(),
        chord,
        span,
        position: [0.0, 0.45 + k * 0.1, z],
        is_vertical: vertical,
        state: WingState {
            aoa: -2.5 + f * 0.013 * k,
            cl: 0.8 + f * 0.001,
            cd: 0.12 + f * 0.0003 * k,
            angle: 4.0 + k - f * 0.007,
            ground_height: 0.0789 - f * 0.00011 * k,
            front_share: [1.0, 0.85, 0.15, 0.0][k as usize],
            drag_kg: 11.25 + f * 0.05 * k,
            lift_kg: lift * (120.5 + f * 0.3 * k),
            yaw_angle: (f * 0.0137 * k).sin() * 3.0,
            is_vertical: vertical,
            lift_vector: rustyac_physics::vecmath::Vec3f { x: 0.0, y: lift * (1200.0 + f * k), z: 40.0 * k },
            ..Default::default()
        },
    };
    vec![wing("BODY", 1.2, 1.6, 0.1, false, 0.0), wing("FRONT", 0.32, 1.7, 1.9, false, 1.0), wing("REAR", 0.41, 1.25, -2.1, false, 2.0), wing("FIN", 0.6, 0.35, -1.8, true, 3.0)]
}

/// FormTelemetry's raw channel values of a frame: five physics steps 3 ms apart (16 ms a frame:
/// the 12 ms rule takes about one in four), then the frame's own.
pub fn telemetry_feed(frame: usize) -> rustyac_gui::app_telemetry::TelemetryFeed {
    let n = rustyac_gui::app_telemetry::channels(&TELEMETRY_CAR).len();
    let raw = |k: usize| -> Vec<f32> { (0..n).map(|c| (((k.wrapping_mul(31) + c * 17) % 101) as f32 - 50.0) * 0.05 * ((c % 7) + 1) as f32).collect() };
    rustyac_gui::app_telemetry::TelemetryFeed { graphics: raw(frame * 8 + 7), steps: (0..5).map(|j| (frame as f64 * 16.0 + j as f64 * 3.0, raw(frame * 8 + j))).collect() }
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
    /// the compound's short name and ideal pressure (psi); EDL there and its output; DRS there;
    /// the player's force feedback gain (controls) and the user's
    pub compound: String,
    pub ideal_pressure: i32,
    pub edl: Option<f32>,
    pub drs: bool,
    pub ff_gain: f32,
    pub user_ff_gain: f32,
    /// the cars' folder; KERS / ERS: there, the lap's energy (J)
    pub unix: String,
    pub hybrid: [f32; 4],
    /// the track's folder and layout
    pub track: (String, String),
    /// the AI line's points (the track map of a track without map.png draws it)
    pub ai_points: Vec<[f32; 3]>,
    /// the track's sections (data/sections.ini), the sun's angle, the game's start (ms)
    pub sections: Vec<(f32, f32, String)>,
    pub sun_angle: f32,
    pub sun_set: bool,
    pub start_ms: f64,
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
    /// the sound cards' names; every car's bonnet and bumper views' "external sound" at the start
    pub drivers: Vec<String>,
    pub camera: Vec<[bool; 2]>,
    /// the player's car's `ffMult` and `steerAssist` (car.ini), its wheel's effects (none: no wheel)
    pub ff_mult: f32,
    pub steer_assist: f32,
    pub ff_upgrades: Option<[f32; 4]>,
    /// the player's car's ERS power controllers' names (none: the car has no physics block for
    /// the apps), its cockpit's ERS power index, recovery, engine brake and heat charging
    pub ers_names: Vec<String>,
    pub ers_state: [i32; 4],
    /// the player's car's eye point, the on-board camera's field of view and pitch (radians)
    /// at the start
    pub eyes: Option<([f32; 3], f32, f32)>,
    /// `RaceManager::penaltiesEnabled`
    pub penalties: bool,
    /// Task 33: `MicroSectors` on (`gameplay.ini [GUI] SHOW_SECTORS=1`)
    pub sectors: bool,
    /// Task 33: the numbers of the statistics apps
    pub stats: rustyac_gui::app_stats::AppStats,
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
        "app.compound" => script(|s| {
            s.compound = text.to_string();
            s.ideal_pressure = number as i32;
        }),
        "app.edl" => script(|s| s.edl = if text == "off" { None } else { Some(number) }),
        "app.drs" => script(|s| s.drs = on),
        "app.ff" => script(|s| s.ff_gain = number),
        "app.unix" => script(|s| s.unix = text.to_string()),
        "app.aiLine" => {
            let spline = rustyac_physics::track::spline::AiSpline::load(std::path::Path::new(text))?;
            let points = spline.spline.points.iter().map(|p| p.point).collect();
            script(|s| s.ai_points = points);
        }
        "app.track" => {
            let (name, config) = text.split_once('|').unwrap_or((text, ""));
            script(|s| s.track = (name.to_string(), config.to_string()));
        }
        "app.sections" => {
            let ini = rustyac_physics::data::ini::IniReader::load(std::path::Path::new(text)).ok();
            let sections = rustyac_gui::app_small::read_sections(ini.as_ref());
            script(|s| s.sections = sections);
        }
        "app.sun" => script(|s| {
            s.sun_angle = number;
            s.sun_set = true;
        }),
        "app.gameStart" => script(|s| s.start_ms = number as f64),
        "app.hybrid" => {
            let k = ["hasKers", "hasErs", "kersMaxJ", "ersMaxJ"].iter().position(|n| *n == text).ok_or("app.hybrid: hasKers, hasErs, kersMaxJ or ersMaxJ")?;
            script(|s| s.hybrid[k] = number);
        }
        "app.userFf" => script(|s| s.user_ff_gain = number),
        "app.env" => {
            let k = ["air", "road", "grip", "wind", "windDirection"].iter().position(|n| *n == text).ok_or("app.env: air, road, grip, wind or windDirection")?;
            script(|s| s.env[k] = number);
        }
        "app.focus" => script(|s| s.focused = number as usize),
        "app.drivers" => script(|s| s.drivers = text.split('|').map(str::to_string).collect()),
        "app.ffMult" => script(|s| s.ff_mult = number),
        "app.eyes" => {
            // "x,y,z|fov|pitch in degrees" (car.ini's ON_BOARD_PITCH_ANGLE * 0.017453f)
            let parts: Vec<&str> = text.split('|').collect();
            let e: Vec<f32> = parts.first().map(|p| p.split(',').filter_map(|v| v.trim().parse().ok()).collect()).unwrap_or_default();
            let fov: f32 = parts.get(1).and_then(|v| v.parse().ok()).ok_or("app.eyes: x,y,z|fov|pitch")?;
            let pitch: f32 = parts.get(2).and_then(|v| v.parse().ok()).ok_or("app.eyes: x,y,z|fov|pitch")?;
            if e.len() != 3 {
                return Err("app.eyes: x,y,z|fov|pitch".to_string());
            }
            script(|s| s.eyes = Some(([e[0], e[1], e[2]], fov, pitch * f32::from_bits(0x3c8e_f998))));
        }
        "app.penalties" => script(|s| s.penalties = on),
        "app.sectors" => script(|s| s.sectors = on),
        "app.stats" => {
            // "fps|dip|tri|scene|cpu|update|hud|render|audio|occupancy|late|fmod" (fmod "-": none)
            let v: Vec<&str> = text.split('|').collect();
            if v.len() != 12 {
                return Err("app.stats wants 12 values".to_string());
            }
            let f = |i: usize| v[i].parse::<f64>().map_err(|e| format!("app.stats {}: {e}", v[i]));
            let stats = rustyac_gui::app_stats::AppStats {
                fps: f(0)? as f32,
                delta_t: DT,
                dip_calls: f(1)? as i32,
                triangles: f(2)? as i32,
                scene_triangles: f(3)? as i32,
                cpu_time: f(4)?,
                update_time: f(5)?,
                render_hud_time: f(6)?,
                render_time: f(7)?,
                render_audio_time: f(8)?,
                occupancy: f(9)? as i32,
                physics_late_loops: f(10)? as u32,
                fmod_update_usage: if v[11] == "-" { None } else { Some(f(11)? as f32) },
            };
            script(|s| s.stats = stats);
        }
        "app.ersNames" => script(|s| s.ers_names = text.split('|').map(str::to_string).collect()),
        "app.ersState" => {
            let v: Vec<i32> = text.split(',').filter_map(|v| v.trim().parse().ok()).collect();
            if v.len() != 4 {
                return Err("app.ersState wants power,recovery,brake,heat".to_string());
            }
            script(|s| s.ers_state = [v[0], v[1], v[2], v[3]]);
        }
        "app.steerAssist" => script(|s| s.steer_assist = number),
        "app.wheel" => {
            let gains: Vec<f32> = text.split(',').filter_map(|v| v.trim().parse().ok()).collect();
            script(|s| s.ff_upgrades = (gains.len() == 4).then(|| [gains[0], gains[1], gains[2], gains[3]]));
        }
        "app.camera" => script(|s| {
            let car = number as usize;
            if s.camera.len() <= car {
                s.camera.resize(car + 1, [false; 2]);
            }
            s.camera[car] = [text.as_bytes().first() == Some(&b'1'), text.as_bytes().get(1) == Some(&b'1')];
        }),
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
const STATE_FIELDS: [(&str, usize, char); 3] = [("statusBytes", 0xb58, 'i'), ("gas", 0x260, 'f'), ("lastFF", 0x668, 'f')];

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

/// The sun's angle a script set (none: the graphics keep theirs).
pub fn sun() -> Option<f32> {
    script(|s| s.sun_set.then_some(s.sun_angle))
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
                    body_matrix: state.world_matrix,
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
                    unix_name: s.unix.clone(),
                    has_kers: s.hybrid[0] != 0.0,
                    has_ers: s.hybrid[1] != 0.0,
                    kers_max_j: s.hybrid[2],
                    ers_max_j: s.hybrid[3],
                    compound_short_name: s.compound.clone(),
                    // (only the player's car has the physics the getter asks)
                    ideal_pressure: if i == 0 { [s.ideal_pressure; 4] } else { [0; 4] },
                    edl_enabled: s.edl.is_some(),
                    edl_out_level: s.edl.unwrap_or(0.0),
                    has_drs: s.drs,
                    ff_global_gain: (i == 0).then_some(s.ff_gain),
                    user_ff_gain: (i == 0).then_some(s.user_ff_gain),
                    has_physics: i == 0 && !s.ers_names.is_empty(),
                    ers_power_name: if i == 0 { s.ers_names.get(s.ers_state[0] as usize).cloned().unwrap_or_default() } else { String::new() },
                    ers_recovery: if i == 0 { s.ers_state[1] } else { 0 },
                    engine_brake_setting: if i == 0 { s.ers_state[2] } else { 0 },
                    ers_heat_charging: i == 0 && s.ers_state[3] != 0,
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
            game_start_time: s.start_ms,
            sun_angle: s.sun_angle,
            time_mult: None,
            use_mph: s.use_mph,
            game_dt: DT,
            ff_upgrades: s.ff_upgrades,
            driver_eyes: s.eyes.map_or([0.0; 3], |e| e.0),
            onboard_fov: s.eyes.map_or(0.0, |e| e.1),
            onboard_pitch: s.eyes.map_or(0.0, |e| e.2),
            auto_eyes: None,
            ctrl_down: false,
            penalties_enabled: s.penalties,
            stats: rustyac_gui::app_stats::AppStats { delta_t: DT, ..s.stats.clone() },
            telemetry: telemetry_feed(s.frame),
            tyre_debug: Some(tyre_debug_rows(s.frame)),
            tyre_tester: Some(tyre_tester_car(s.frame)),
            suspension: states.first().map(|state| suspension_link(state, s.frame)),
            setup_debug: Some(setup_debug_car(s.frame)),
            car_physics: car_physics_extra(s.frame),
            car_engineer: car_engineer(s.frame),
            wings: wing_rows(s.frame),
            gpu_profile: gpu_rows(s.frame),
            gpu_profiler_active: false,
            track_name: s.track.0.clone(),
            track_config: s.track.1.clone(),
            camera_sounds: s.camera.clone(),
            audio_drivers: s.drivers.clone(),
            audio_ini: None,
            actions: Vec::new(),
        }
    })
}

/// After a sequence: the files the two sides wrote (the AI-line map's `map.png`, compared by
/// its pixels, and `data\map.ini`, byte for byte).
pub fn compare_files(args: &crate::Args, name: &str) -> Result<bool, String> {
    if name.starts_with("app_chat") {
        // FormChat's export: Documents\Assetto Corsa\out\chat[_<time>].txt
        let (game, port) = (args.root.join("documents/Assetto Corsa/out"), args.root.join("port_files/out"));
        let list = |d: &std::path::Path| {
            let mut v: Vec<String> = std::fs::read_dir(d).map(|r| r.filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().into_owned()).collect()).unwrap_or_default();
            v.sort();
            v
        };
        let (a, b) = (list(&game), list(&port));
        if a != b {
            println!("file out/: DIFFERENT\n  game: {a:?}\n  port: {b:?}");
            return Ok(false);
        }
        if a.is_empty() {
            println!("file out/: IDENTICAL (no export on either side)");
        }
        let mut same = true;
        for f in a {
            match (std::fs::read(game.join(&f)), std::fs::read(port.join(&f))) {
                (Ok(x), Ok(y)) if x == y => println!("file out/{f}: IDENTICAL ({} bytes)", x.len()),
                (x, y) => {
                    same = false;
                    println!("file out/{f}: DIFFERENT\n  game: {:?}\n  port: {:?}", x.map(|x| String::from_utf8_lossy(&x).into_owned()), y.map(|y| String::from_utf8_lossy(&y).into_owned()));
                }
            }
        }
        return Ok(same);
    }
    if name.starts_with("app_eyes") {
        let mut same = true;
        let docs = args.root.join("documents/Assetto Corsa/cfg");
        let port = args.root.join("port_files/documents/Assetto Corsa/cfg");
        for (what, a, b) in [
            ("cfg/cars/ks_ferrari_f2004/view.ini", docs.join("cars/ks_ferrari_f2004/view.ini"), port.join("cars/ks_ferrari_f2004/view.ini")),
            ("cfg/camera_onboard.ini", docs.join("camera_onboard.ini"), port.join("camera_onboard.ini")),
            ("content/cars/ks_ferrari_f2004/data/car.ini", args.root.join("content/cars/ks_ferrari_f2004/data/car.ini"), args.root.join("port_files/content/cars/ks_ferrari_f2004/data/car.ini")),
            ("content/cars/ks_ferrari_f2004/data/dash_cam.ini", args.root.join("content/cars/ks_ferrari_f2004/data/dash_cam.ini"), args.root.join("port_files/content/cars/ks_ferrari_f2004/data/dash_cam.ini")),
            ("the on-board camera's settings (the eye point, the field of view, the pitch, the messages)", args.root.join("effects_game.txt"), args.root.join("effects_port.txt")),
        ] {
            match (std::fs::read(&a), std::fs::read(&b)) {
                (Ok(a), Ok(b)) if a == b => println!("file {what}: IDENTICAL ({} bytes)", a.len()),
                (a, b) => {
                    same = false;
                    println!("file {what}: DIFFERENT\n  game: {:?}\n  port: {:?}", a.map(|x| String::from_utf8_lossy(&x).into_owned()), b.map(|x| String::from_utf8_lossy(&x).into_owned()));
                }
            }
        }
        return Ok(same);
    }
    if name.starts_with("app_driverperf") {
        let rel = "savedData/driverPerformances/magione/ks_ferrari_f2004.ini";
        let (a, b) = (std::fs::read(args.root.join("documents/Assetto Corsa").join(rel)), std::fs::read(args.root.join("port_files").join(rel)));
        return Ok(match (a, b) {
            (Ok(a), Ok(b)) if a == b => {
                println!("file {rel}: IDENTICAL ({} bytes)", a.len());
                true
            }
            (a, b) => {
                println!("file {rel}: DIFFERENT\n  game: {:?}\n  port: {:?}", a.map(|x| String::from_utf8_lossy(&x).into_owned()), b.map(|x| String::from_utf8_lossy(&x).into_owned()));
                false
            }
        });
    }
    if name.starts_with("app_ballast") {
        let (a, b) = (std::fs::read(args.root.join("effects_game.txt")), std::fs::read(args.root.join("effects_port.txt")));
        return Ok(match (a, b) {
            (Ok(a), Ok(b)) if a == b => {
                println!("file the player's car's ballast: IDENTICAL ({} bytes)", a.len());
                true
            }
            (a, b) => {
                println!("file the player's car's ballast: DIFFERENT\n  game: {:?}\n  port: {:?}", a.map(|x| String::from_utf8_lossy(&x).into_owned()), b.map(|x| String::from_utf8_lossy(&x).into_owned()));
                false
            }
        });
    }
    if name.starts_with("app_ffb") {
        let mut same = true;
        let car_ini = "content/cars/ks_ferrari_f2004/data/car.ini";
        for (what, a, b) in [
            ("cfg/controls.ini", args.root.join("documents/Assetto Corsa/cfg/controls.ini"), args.root.join("port_files/documents/Assetto Corsa/cfg/controls.ini")),
            (car_ini, args.root.join(car_ini), args.root.join("port_files").join(car_ini)),
            ("the force feedback's settings (the gains, the FF mult, the wheel's effects, the messages)", args.root.join("effects_game.txt"), args.root.join("effects_port.txt")),
        ] {
            match (std::fs::read(&a), std::fs::read(&b)) {
                (Ok(a), Ok(b)) if a == b => println!("file {what}: IDENTICAL ({} bytes)", a.len()),
                (a, b) => {
                    same = false;
                    println!("file {what}: DIFFERENT\n  game: {:?}\n  port: {:?}", a.map(|x| String::from_utf8_lossy(&x).into_owned()), b.map(|x| String::from_utf8_lossy(&x).into_owned()));
                }
            }
        }
        return Ok(same);
    }
    if name.starts_with("app_volumes") {
        let mut same = true;
        let docs = args.root.join("documents/Assetto Corsa/cfg");
        let port = args.root.join("port_files/documents/Assetto Corsa/cfg");
        for (what, a, b) in [
            ("cfg/audio.ini", docs.join("audio.ini"), port.join("audio.ini")),
            ("cfg/camera_manager.ini", docs.join("camera_manager.ini"), port.join("camera_manager.ini")),
            ("the sound's settings (master level calls, every car's mix, the views' sounds)", args.root.join("effects_game.txt"), args.root.join("effects_port.txt")),
        ] {
            match (std::fs::read(&a), std::fs::read(&b)) {
                (Ok(a), Ok(b)) if a == b => println!("file {what}: IDENTICAL ({} bytes)", a.len()),
                (a, b) => {
                    same = false;
                    println!("file {what}: DIFFERENT\n  game: {:?}\n  port: {:?}", a.map(|x| String::from_utf8_lossy(&x).into_owned()), b.map(|x| String::from_utf8_lossy(&x).into_owned()));
                }
            }
        }
        return Ok(same);
    }
    if name.starts_with("app_drszones") {
        let rel = "content/tracks/magione/data/drs_zones.ini.candidate";
        let mut same = true;
        for (what, a, b) in [(rel, args.root.join(rel), args.root.join("port_files").join(rel)), ("the messages", args.root.join("effects_game.txt"), args.root.join("effects_port.txt"))] {
            match (std::fs::read(&a), std::fs::read(&b)) {
                (Ok(a), Ok(b)) if a == b => println!("file {what}: IDENTICAL ({} bytes)", a.len()),
                (a, b) => {
                    same = false;
                    println!("file {what}: DIFFERENT
  game: {:?}
  port: {:?}", a.map(|x| String::from_utf8_lossy(&x).into_owned()), b.map(|x| String::from_utf8_lossy(&x).into_owned()));
                }
            }
        }
        return Ok(same);
    }
    if !name.contains("ailine") {
        return Ok(true);
    }
    let game = args.root.join("content/tracks/t32_ai_line");
    let port = args.root.join("port_files/content/tracks/t32_ai_line");
    let mut same = true;
    let (a, b) = (std::fs::read(game.join("data/map.ini")), std::fs::read(port.join("data/map.ini")));
    match (a, b) {
        (Ok(a), Ok(b)) if a == b => println!("file data/map.ini: IDENTICAL ({} bytes)", a.len()),
        (a, b) => {
            same = false;
            println!("file data/map.ini: DIFFERENT (game {:?} bytes, port {:?})", a.map(|x| String::from_utf8_lossy(&x).into_owned()), b.map(|x| String::from_utf8_lossy(&x).into_owned()));
        }
    }
    let pixels = |path: std::path::PathBuf| -> Result<(u32, u32, Vec<u8>), String> {
        let decoder = png::Decoder::new(std::fs::File::open(&path).map_err(|e| format!("{}: {e}", path.display()))?);
        let mut reader = decoder.read_info().map_err(|e| e.to_string())?;
        let mut buf = vec![0; reader.output_buffer_size()];
        let info = reader.next_frame(&mut buf).map_err(|e| e.to_string())?;
        buf.truncate(info.buffer_size());
        let rgba = match info.color_type {
            png::ColorType::Rgba => buf,
            png::ColorType::Rgb => buf.chunks(3).flat_map(|c| [c[0], c[1], c[2], 255]).collect(),
            other => return Err(format!("{}: colour type {other:?}", path.display())),
        };
        Ok((info.width, info.height, rgba))
    };
    match (pixels(game.join("map.png")), pixels(port.join("map.png"))) {
        (Ok(a), Ok(b)) if a == b => println!("file map.png: pixels IDENTICAL ({} x {})", a.0, a.1),
        (Ok(a), Ok(b)) => {
            same = false;
            let differing = a.2.chunks(4).zip(b.2.chunks(4)).filter(|(x, y)| x != y).count();
            println!("file map.png: pixels DIFFERENT (game {} x {}, port {} x {}, {differing} pixels differ)", a.0, a.1, b.0, b.1);
        }
        (a, b) => {
            same = false;
            println!("file map.png: missing ({:?} / {:?})", a.err(), b.err());
        }
    }
    Ok(same)
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
pub(crate) fn race(o: &mut Vec<Op>, cars: usize) {
    o.push(hud("app.create", cars as f32, ""));
    o.push(hud("app.tape", 0.0, tape("oracle/t32/race/rc_race")));
    o.extend([hud("app.session", 3.0, ""), hud("app.laps", 3.0, ""), hud("app.start", 5000.0, ""), hud("app.duration", 0.0, ""), hud("app.sessions", 1.0, "")]);
    // the F2004's tank and engine (car.ini [FUEL] MAX_FUEL, engine.ini [DAMAGE] RPM_THRESHOLD)
    o.extend([hud("app.maxFuel", 150.0, ""), hud("app.damageRpm", 20200.0, ""), hud("app.abs", 0.0, ""), hud("app.tc", 1.0, ""), hud("app.unix", 0.0, "ks_ferrari_f2004"), hud("app.track", 0.0, "magione")]);
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

/// The files the apps read, the same for both sides: `system/cfg/tyres_app.ini` of the game,
/// and a `cfg/controls.ini` in the stand-in Documents.
pub fn prepare_root(args: &crate::Args, name: &str) -> Result<(), String> {
    if !name.starts_with("app_") {
        return Ok(());
    }
    let cfg = args.root.join("system/cfg");
    std::fs::create_dir_all(&cfg).map_err(|e| e.to_string())?;
    // FormChat's settings (Task 34): the shipped ones, or with the times shown, the export on,
    // other fonts and colours; no export left from before
    let shipped = std::fs::read_to_string(args.game.join("system/cfg/chat_app.ini")).map_err(|e| format!("chat_app.ini: {e}"))?;
    let chat_ini = match name.split('@').next().unwrap_or(name) {
        // (the same size as the shipped file: the root's copy is left alone)
        "app_chat_export" => shipped.replace("SHOW_TIME=0", "SHOW_TIME=1").replace("EXPORT_ON_CLOSE=0", "EXPORT_ON_CLOSE=1").replace("OVERWRITE_EXPORT=0", "OVERWRITE_EXPORT=1").replace("BOLD=0", "BOLD=1").replace("ROW_COUNT=10", "ROW_COUNT=6").replace("FADE_MULT=5", "FADE_MULT=0.5").replace("PREFADE_MULT=10", "PREFADE_MULT=1").replace("SINGLE_ROW_W=300", "SINGLE_ROW_W=250"),
        // (the same size as the shipped file: the root's copy is left alone)
        "app_chat_online" => shipped.replace("EXPORT_ON_CLOSE=0", "EXPORT_ON_CLOSE=1").replace("ITALIC=0", "ITALIC=1").replace("BG_INTENSITY_MAIN=0.2", "TIME_TO_LOSE_FOCUS=02").replace("FORECOLOR_4=", "FORECOLOR_Y="),
        _ => shipped,
    };
    std::fs::write(cfg.join("chat_app.ini"), chat_ini).map_err(|e| e.to_string())?;
    // (an empty out folder, as the game's start leaves it: its wofstream makes no folders)
    let _ = std::fs::remove_dir_all(args.root.join("documents/Assetto Corsa/out"));
    std::fs::create_dir_all(args.root.join("documents/Assetto Corsa/out")).map_err(|e| e.to_string())?;
    std::fs::copy(args.game.join("system/cfg/tyres_app.ini"), cfg.join("tyres_app.ini")).map_err(|e| format!("tyres_app.ini: {e}"))?;
    let docs = args.root.join("documents/Assetto Corsa/cfg");
    std::fs::create_dir_all(&docs).map_err(|e| e.to_string())?;
    std::fs::write(docs.join("controls.ini"), "[HEADER]
INPUT_METHOD=WHEEL

[STEER]
FF_GAIN=0.85
").map_err(|e| e.to_string())?;
    // FormTelemetry's presets; its user file only for "app_telemetry_user" (the "AI Debug" preset)
    std::fs::copy(args.game.join("system/cfg/telemetry_presets.ini"), cfg.join("telemetry_presets.ini")).map_err(|e| format!("telemetry_presets.ini: {e}"))?;
    let user = docs.join("telemetry_user.ini");
    if name.starts_with("app_telemetry_user") {
        std::fs::write(&user, "[USER]
PRESET=AI Debug
").map_err(|e| e.to_string())?;
    } else if user.exists() {
        std::fs::remove_file(&user).map_err(|e| e.to_string())?;
    }
    // the gauges' pictures of FormGear
    let tex = args.root.join("content/texture");
    std::fs::create_dir_all(&tex).map_err(|e| e.to_string())?;
    for name in ["rpm_off", "rpm_on", "turbo_off", "turbo_on", "kers_left_off", "kers_right_off", "kersmotor_on", "battery_on", "kersload_on", "kers_inner_right_off"] {
        let file = format!("content/texture/{name}.png");
        std::fs::copy(args.game.join(&file), args.root.join(&file)).map_err(|e| format!("{file}: {e}"))?;
    }
    // the track map's files: system/cfg/map.ini, the road's picture, every test track's map
    std::fs::copy(args.game.join("system/cfg/map.ini"), cfg.join("map.ini")).map_err(|e| format!("map.ini: {e}"))?;
    std::fs::copy(args.game.join("content/texture/mapRoad.png"), tex.join("mapRoad.png")).map_err(|e| format!("mapRoad.png: {e}"))?;
    for track in ["magione", "spa", "ks_laguna_seca"] {
        let to = args.root.join("content/tracks").join(track);
        std::fs::create_dir_all(to.join("data")).map_err(|e| e.to_string())?;
        for file in ["map.png", "data/map.ini"] {
            std::fs::copy(args.game.join("content/tracks").join(track).join(file), to.join(file)).map_err(|e| format!("{track} {file}: {e}"))?;
        }
    }
    // ACHelp's text; a time limited test of 3 minutes (FormRunningTime)
    let help = args.root.join("system/locales/help");
    std::fs::create_dir_all(&help).map_err(|e| e.to_string())?;
    std::fs::copy(args.game.join("system/locales/help/en.tag"), help.join("en.tag")).map_err(|e| format!("en.tag: {e}"))?;
    std::fs::write(docs.join("time_limited_test.ini"), "[SETTINGS]\nMAX_RUNNING_TIME=3\n").map_err(|e| e.to_string())?;
    // a track with no map.png: the map is drawn from the AI line, written by each side into
    // its own place (the game's into the scratch root, the port's into port_files)
    let ai_track = args.root.join("content/tracks/t32_ai_line");
    let _ = std::fs::remove_dir_all(&ai_track);
    let _ = std::fs::remove_dir_all(args.root.join("port_files"));
    std::fs::create_dir_all(ai_track.join("data")).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(args.root.join("port_files/content/tracks/t32_ai_line/data")).map_err(|e| e.to_string())?;
    // the cars' engine.ini (the repo's cardata): FormGear reads [TURBO_n] DISPLAY_MAX_BOOST
    for car in ["ks_ferrari_f2004", "ks_ferrari_sf15t"] {
        let to = args.root.join("content/cars").join(car).join("data");
        std::fs::create_dir_all(&to).map_err(|e| e.to_string())?;
        std::fs::copy(std::path::Path::new("cardata").join(car).join("engine.ini"), to.join("engine.ini")).map_err(|e| format!("{car} engine.ini: {e}"))?;
        // the Tyre tester's: tyres.ini and the curves it names, loose for both sides
        let from = std::path::Path::new("cardata").join(car);
        let tyres = std::fs::read_to_string(from.join("tyres.ini")).map_err(|e| format!("{car} tyres.ini: {e}"))?;
        let luts = tyres.lines().filter_map(|l| l.split_once('=')).map(|(_, v)| v.split(';').next().unwrap_or("").trim()).filter(|v| v.to_ascii_lowercase().ends_with(".lut"));
        for name in std::iter::once("tyres.ini").chain(luts) {
            std::fs::copy(from.join(name), to.join(name)).map_err(|e| format!("{car} {name}: {e}"))?;
        }
    }
    // the Audio app's files: an audio.ini as a hand would leave it (levels between the steps,
    // one past 1, no TRANSMISSION), no camera_manager.ini; the same for the port's side
    let port_docs = args.root.join("port_files/documents/Assetto Corsa/cfg");
    std::fs::create_dir_all(&port_docs).map_err(|e| e.to_string())?;
    for dir in [&docs, &port_docs] {
        std::fs::write(dir.join("audio.ini"), "[LEVELS]\r\nMASTER=0.85\r\nENGINE=0.85\r\nWIND=0.4\r\nTYRES=1.2\r\nSURFACES=0.6\r\nDIRT_BOTTOM=0.3\r\nOPPONENTS=0.9\r\n\r\n[SETTINGS]\r\nDRIVER_NAME=Speakers\r\n").map_err(|e| e.to_string())?;
        let _ = std::fs::remove_file(dir.join("camera_manager.ini"));
    }
    for file in ["effects_game.txt", "effects_port.txt"] {
        let _ = std::fs::remove_file(args.root.join(file));
    }
    let _ = std::fs::remove_dir_all(args.root.join("documents/Assetto Corsa/savedData"));
    // the force feedback app's: the same controls.ini for the port's writes; the F2004's
    // car.ini a loose file (FFBForm's "Auto FFB" and "Quick Adjust" are only for such a car)
    std::fs::copy(docs.join("controls.ini"), port_docs.join("controls.ini")).map_err(|e| e.to_string())?;
    let car_ini = "content/cars/ks_ferrari_f2004/data/car.ini";
    // the Onboard settings' files: the game folder's cfg/camera_onboard.ini, no view.ini, no
    // dash_cam.ini and no Documents camera_onboard.ini on either side
    std::fs::create_dir_all(args.root.join("cfg")).map_err(|e| e.to_string())?;
    std::fs::copy(args.game.join("cfg/camera_onboard.ini"), args.root.join("cfg/camera_onboard.ini")).map_err(|e| format!("camera_onboard.ini: {e}"))?;
    for dir in [&docs, &port_docs] {
        let _ = std::fs::remove_file(dir.join("cars/ks_ferrari_f2004/view.ini"));
        let _ = std::fs::remove_file(dir.join("camera_onboard.ini"));
    }
    for root in [args.root.clone(), args.root.join("port_files")] {
        let _ = std::fs::remove_file(root.join("content/cars/ks_ferrari_f2004/data/dash_cam.ini"));
    }
    if name.starts_with("app_ffb") || name.starts_with("app_eyes") {
        for to in [args.root.join(car_ini), args.root.join("port_files").join(car_ini)] {
            std::fs::create_dir_all(to.parent().unwrap_or(&args.root)).map_err(|e| e.to_string())?;
            std::fs::copy("cardata/ks_ferrari_f2004/car.ini", &to).map_err(|e| format!("car.ini: {e}"))?;
        }
    } else {
        let _ = std::fs::remove_file(args.root.join(car_ini));
    }
    Ok(())
}

/// FormChat's clock (`_localtime64`): the same moment for both sides.
pub fn chat_clock() -> [i32; 6] {
    [2024, 5, 17, 13, 4, 9]
}

/// The Chat app (Task 34): off line its typed lines echoed, scrolled, faded and brought back by
/// the mouse; online (made up) the drivers', the server's and the player's lines, the player's
/// sent; the export at the end.
fn chat(o: &mut Vec<Op>, app: &str) {
    let online = app == "app_chat_online";
    if online {
        o.push(hud("online", 1.0, ""));
        for (sid, car) in [(3.0, "1"), (4.0, "2"), (7.0, "5"), (9.0, "6")] {
            o.push(hud("menu.netCar", sid, car));
        }
        o.extend([hud("menu.env", 61_000.0, "now"), Op::Frame(DT)]);
    }
    o.push(hud("app.create", 1.0, ""));
    o.push(Op::Frame(DT));
    o.push(hud("app.new", 0.0, "chat"));
    o.push(Op::Frame(DT));
    o.push(hud("app.visible", 1.0, "chat"));
    o.push(Op::Frame(DT));
    let click = |o: &mut Vec<Op>, x: i32, y: i32| o.extend([Op::MouseMove(x, y), Op::MouseDown(x, y), Op::MouseUp(x, y), Op::Frame(DT)]);
    let typed = |o: &mut Vec<Op>, text: &str| {
        for c in text.chars() {
            o.push(Op::KeyChar(c as u32));
        }
        o.extend([Op::Frame(DT), Op::KeyChar(13), Op::Frame(DT)]);
    };
    // a click on the form gives the line the keys
    click(o, 60, 80 + 40);
    if online {
        for (sid, text) in [(3.0, "hello everyone"), (255.0, "Welcome to the server, races every hour"), (4.0, "gl hf"), (9.0, "a rather long line from the driver of car six that wraps twice over the rows"), (7.0, "colour"), (11.0, "nobody's session")] {
            o.extend([hud("menu.chat", sid, text), Op::Frame(DT)]);
        }
        typed(o, "hi all");
        // too soon after: kept (the game sends nothing)
        typed(o, "again");
        o.extend([hud("menu.env", 63_000.0, "now"), Op::KeyChar(13), Op::Frame(DT)]);
        for i in 0..14 {
            o.push(hud("menu.chat", [3.0, 4.0, 7.0, 9.0, 255.0][i % 5], leak(format!("line {i} of the chat"))));
        }
        o.push(Op::Frame(DT));
    } else {
        for text in ["hello", "  a line with blanks around  ", "a long line of more than forty two characters to be cut in two", "nospacesinthislineatallsoitiscutatthewidthofrowsandagain", "", "   ", "last"] {
            typed(o, text);
        }
        for i in 0..12 {
            typed(o, leak(format!("row {i}")));
        }
    }
    // the scroll buttons (top, up, a page up, a page down, down, bottom) on the right edge
    let w = if app == "app_chat_export" { 250 + 70 + 17 } else { 300 + 17 };
    let rows = if app == "app_chat_export" { 6 } else { 10 };
    let x = w - 8;
    let top = 80 + 22;
    let h = top + rows * 17 + 17;
    for y in [top + 8, top + 17 + 8, top + 34 + 8, h - 51 - 8, h - 51 + 8, h - 34 + 8, top + 8, top + 8, h - 34 + 8] {
        click(o, x, y);
    }
    // the input loses the keys; the lines fade; the mouse over the form brings them back
    for _ in 0..34 {
        o.push(Op::Frame(0.5));
    }
    o.extend([Op::MouseMove(100, top + 30), Op::Frame(DT), Op::Frame(DT), Op::MouseMove(700, 500), Op::Frame(DT)]);
    for _ in 0..30 {
        o.push(Op::Frame(0.5));
    }
    o.extend([Op::Save, hud("app.shutdown", 0.0, "chat")]);
}

pub fn sequence(name: &str) -> Option<Vec<Op>> {
    let mut o = Vec::new();
    let (app, rest) = name.split_once('@').unwrap_or((name, ""));
    match app {
        "app_chat" | "app_chat_export" | "app_chat_online" => {
            let _ = rest;
            chat(&mut o, app);
        }
        "app_laps" | "app_position" | "app_essentials" | "app_delta" | "app_realtime" | "app_delta_sectors" => {
            // (Task 33: the delta with its micro-sectors)
            if app == "app_delta_sectors" {
                o.push(hud("app.sectors", 1.0, ""));
            }
            let which = leak(app[4..].trim_end_matches("_sectors").to_string());
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
        "app_gears_ers" | "app_mgu_ers" => {
            // the SF15-T's lap of Spa with the ERS (Task 16's spa_hybrid_lap), alone
            let which = leak(app[4..app.len() - 4].to_string());
            o.push(hud("app.create", 1.0, ""));
            o.push(hud("app.tape", 0.0, tape("oracle/t32/sf15t/spa_hybrid_lap")));
            o.extend([hud("app.session", 1.0, ""), hud("app.sessions", 1.0, ""), hud("app.duration", 30.0, ""), hud("app.maxFuel", 120.0, ""), hud("app.damageRpm", 15500.0, ""), hud("app.unix", 0.0, "ks_ferrari_sf15t")]);
            o.extend([hud("app.hybrid", 0.0, "hasKers"), hud("app.hybrid", 1.0, "hasErs"), hud("app.hybrid", 0.0, "kersMaxJ"), hud("app.hybrid", 4_000_000.0, "ersMaxJ"), hud("app.spline", 7004.0, "")]);
            // the SF15-T's six ctrl_ers_N.ini; the cockpit at Balanced high, recovery 5, brake 3,
            // charging the battery
            o.extend([hud("app.ersNames", 0.0, "Charging|Balanced low|Balanced high|Overtake|Top Speed|Hotlap"), hud("app.ersState", 0.0, "2,5,2,1")]);
            o.push(hud("app.new", 0.0, which));
            o.push(Op::Frame(DT));
            o.push(hud("app.visible", 1.0, which));
            play(&mut o, 0, 400, 40);
            play(&mut o, 400, 1000, 3);
            if rest != "short" {
                play(&mut o, 1000, 3540, 13);
                o.extend([hud("app.mph", 1.0, ""), hud("app.hybrid", 1.0, "hasKers"), hud("app.hybrid", 400_000.0, "kersMaxJ")]);
                play(&mut o, 2000, 2300, 6);
                o.extend([hud("app.mph", 0.0, ""), hud("app.hybrid", 0.0, "ersMaxJ")]);
                play(&mut o, 2300, 2500, 6);
                // the driver's ERS buttons: Overtake, recovery 9, engine brake 1, the MGU-H to the motor
                o.extend([hud("app.ersState", 0.0, "3,9,0,0"), Op::Frame(DT), hud("app.ersState", 0.0, "5,0,10,1"), Op::Frame(DT), hud("app.ersState", 0.0, "0,10,4,0"), Op::Frame(DT)]);
                handle(&mut o, 0, 80, 196);
                o.extend([hud("app.visible", 0.0, which), Op::Frame(DT), hud("app.visible", 1.0, which), Op::Frame(DT), Op::Save]);
            }
        }
        "app_trackmap" => {
            match rest.split('.').next().unwrap_or("") {
                "spa" => {
                    o.push(hud("app.create", 1.0, ""));
                    o.push(hud("app.tape", 0.0, tape("oracle/t32/sf15t/spa_hybrid_lap")));
                    o.extend([hud("app.session", 1.0, ""), hud("app.track", 0.0, "spa")]);
                }
                "laguna" => {
                    o.push(hud("app.create", 6.0, ""));
                    o.push(hud("app.tape", 0.0, tape("oracle/t32/laguna/mc_field")));
                    o.extend([hud("app.session", 1.0, ""), hud("app.track", 0.0, "ks_laguna_seca")]);
                }
                "ailine" => {
                    race(&mut o, 6);
                    let line = std::path::Path::new(r"C:\Program Files (x86)\Steam\steamapps\common\assettocorsa\content\tracks\magione\ai\fast_lane.ai");
                    o.extend([hud("app.aiLine", 0.0, leak(line.to_string_lossy().into_owned())), hud("app.track", 0.0, "t32_ai_line")]);
                }
                _ => race(&mut o, 6),
            }
            o.push(hud("app.new", 0.0, "trackmap"));
            o.push(Op::Frame(DT));
            o.push(hud("app.visible", 1.0, "trackmap"));
            play(&mut o, 0, 600, 30);
            play(&mut o, 600, 1200, 3);
            if !rest.ends_with("short") {
                play(&mut o, 1200, 9000, 31);
                // dragged (the map follows), hidden and back
                o.extend([Op::MouseMove(150, 200), Op::MouseDown(150, 200), Op::MouseMove(400, 300), Op::Frame(DT), Op::MouseMove(700, 380), Op::Frame(DT), Op::MouseUp(700, 380), Op::Frame(DT)]);
                play(&mut o, 9000, 9600, 10);
                o.extend([hud("app.visible", 0.0, "trackmap"), Op::Frame(DT), hud("app.visible", 1.0, "trackmap"), Op::Frame(DT), Op::Save]);
            }
        }
        "app_small" => {
            // Help, Hide all, Running time, Time of the day, Track description on the race
            race(&mut o, 6);
            let sections = std::path::Path::new(r"C:\Program Files (x86)\Steam\steamapps\common\assettocorsa\content\tracks\magione\data\sections.ini");
            o.extend([hud("app.sections", 0.0, leak(sections.to_string_lossy().into_owned())), hud("app.sun", -40.0, ""), hud("app.gameStart", 1000.0, "")]);
            for app in ["hideall", "help", "timeflow", "trackdescription", "runningtime"] {
                o.push(hud("app.new", 0.0, app));
            }
            o.push(Op::Frame(DT));
            for app in ["help", "timeflow", "trackdescription", "runningtime"] {
                o.push(hud("app.visible", 1.0, app));
            }
            play(&mut o, 0, 600, 30);
            play(&mut o, 600, 1200, 6);
            if !rest.ends_with("short") {
                for sun in [-80.0f32, -12.5, 0.0, 33.3, 77.0, 95.0] {
                    o.push(hud("app.sun", sun, ""));
                    play(&mut o, 1200, 1260, 30);
                }
                play(&mut o, 1260, 11000, 61);
                o.extend([hud("app.replay", 1.0, ""), Op::Frame(DT), Op::Frame(DT), hud("app.replay", 0.0, ""), Op::Frame(DT)]);
                // past the test's three minutes, the last half minute blinking
                play(&mut o, 9300, 9500, 5);
                play(&mut o, 11000, 11400, 20);
                // the app bar: shown, Hide all's icon pressed (every app hides), pressed again
                // (the bar slides in for 24 frames, an icon's title for 22)
                o.push(Op::MouseMove(1270, 300));
                o.extend(std::iter::repeat_n(Op::Frame(DT), 24));
                o.push(Op::MouseMove(1250, 60));
                o.extend(std::iter::repeat_n(Op::Frame(DT), 22));
                let click = |o: &mut Vec<Op>, x: i32, y: i32| o.extend([Op::MouseMove(x, y), Op::MouseDown(x, y), Op::MouseUp(x, y), Op::Frame(DT), Op::Frame(DT)]);
                // Hide all on (every app hides); Help's icon (Hide all goes off); Hide all on and off
                click(&mut o, 1250, 60);
                click(&mut o, 1250, 130);
                click(&mut o, 1250, 60);
                click(&mut o, 1250, 60);
                o.extend([Op::MouseMove(600, 400), Op::Frame(DT), Op::Frame(DT), Op::Save]);
            }
        }
        "app_driverperf" => {
            // Driver performances and Race Fuel Monitor on the race from the grid: the wait, the
            // runs to 50 ... 160 km/h and 100 m ... 1 km, the records written at the end
            race(&mut o, 6);
            for app in ["driverperf", "fuelmonitor"] {
                o.push(hud("app.new", 0.0, app));
            }
            o.push(Op::Frame(DT));
            for app in ["driverperf", "fuelmonitor"] {
                o.push(hud("app.visible", 1.0, app));
            }
            play(&mut o, 0, 600, 2);
            if rest != "short" {
                play(&mut o, 600, 5000, 4);
                play(&mut o, 5000, 21000, 40);
            }
            o.extend([hud("app.shutdown", 0.0, ""), Op::Frame(DT), Op::Save]);
        }
        "app_stats" => {
            // Render Stats and the developers' Graphics stats on the race, the numbers moving
            race(&mut o, 6);
            o.push(hud("app.stats", 0.0, "60|1800|900000|700000|9.5|2.25|1.5|5.75|0.8|35|0|1.6"));
            for app in ["rstats", "gamestats"] {
                o.push(hud("app.new", 0.0, app));
            }
            o.push(Op::Frame(DT));
            for app in ["rstats", "gamestats"] {
                o.push(hud("app.visible", 1.0, app));
            }
            play(&mut o, 0, 300, 10);
            let rows = ["144.5|2400|1350000|1000000|5.25|1.5|1.25|2.5|0.6|12|1|2.4", "31|300|20000|0|31|6|8|17|4|250|7|-", "59.9|0|0|0|0|0|0|0|0|0|20000|0.0", "60|1800|900001|700000|16.7|4|3|9.7|1.1|98|3|11.5"];
            let long = rest != "short";
            for row in rows {
                o.push(hud("app.stats", 0.0, row));
                play(&mut o, 300, if long { 600 } else { 330 }, 5);
            }
            handle(&mut o, 0, 80, 160);
            o.extend([hud("app.visible", 0.0, "rstats"), Op::Frame(DT), hud("app.visible", 1.0, "rstats"), Op::Frame(DT), Op::Save]);
        }
        "app_ffb" => {
            // the force feedback app of the F2004 (car.ini FFMULT 3.752, STEER_ASSIST 0.86) with
            // a wheel, on the race
            race(&mut o, 6);
            o.extend([hud("app.ff", 0.85, ""), hud("app.userFf", 1.0, ""), hud("app.ffMult", 3.752 * 0.001, ""), hud("app.steerAssist", 0.86, ""), hud("app.wheel", 0.0, "0.1,0.2,0.3,0.4")]);
            o.push(hud("app.new", 0.0, "ffb"));
            o.push(Op::Frame(DT));
            o.push(hud("app.visible", 1.0, "ffb"));
            play(&mut o, 0, 60, 20);
            let (x, y) = FFB_AT;
            let hold = |o: &mut Vec<Op>, cx: i32, cy: i32, frames: usize| {
                o.extend([Op::MouseMove(x + cx, y + cy), Op::MouseDown(x + cx, y + cy)]);
                o.extend(std::iter::repeat_n(Op::Frame(DT), frames));
                o.extend([Op::MouseUp(x + cx, y + cy), Op::Frame(DT)]);
            };
            // the gain up for half a second, down for a fifth; Quick Adjust (the gain into the
            // car's FF mult, the gain back to 1)
            hold(&mut o, 314, 85, if rest == "short" { 6 } else { 30 });
            hold(&mut o, 35, 85, 12);
            hold(&mut o, 240, 155, 1);
            // the wheel's effects: slips up 2, road down 1, curbs up 1, ABS down 3, then "Save"
            for (cx, cy, n) in [(315, 210, 2), (32, 272, 1), (307, 332, 1), (32, 392, 3)] {
                for _ in 0..n {
                    hold(&mut o, cx, cy, 1);
                }
            }
            hold(&mut o, 170, 452, 1);
            // the gain up again; Auto FFB on (the gain back to 1), the average over the laps (at
            // each of the player's laps the car's FF mult adjusted, with its message)
            hold(&mut o, 314, 85, 10);
            hold(&mut o, 240, 125, 1);
            play(&mut o, 60, if rest == "short" { 300 } else { 4000 }, 4);
            if rest != "short" {
                play(&mut o, 4000, 24000, 97);
            }
            // the gain up, Auto FFB off (the gain back to 1, car.ini saved)
            hold(&mut o, 314, 85, 10);
            hold(&mut o, 240, 125, 1);
            o.extend([hud("app.visible", 0.0, "ffb"), Op::Frame(DT), hud("app.visible", 1.0, "ffb"), Op::Frame(DT), Op::Frame(DT), Op::Save]);
        }
        "app_volumes" => {
            // the Audio app on the race: two sound cards, the first car's bonnet view heard
            // from outside, the second's bumper view
            race(&mut o, 6);
            o.extend([hud("app.drivers", 0.0, "Speakers (High Definition Audio Device)|Headphones (USB Audio)"), hud("app.camera", 0.0, "10"), hud("app.camera", 1.0, "01")]);
            o.push(hud("app.new", 0.0, "volumes"));
            o.push(Op::Frame(DT));
            o.push(hud("app.visible", 1.0, "volumes"));
            play(&mut o, 0, 60, 20);
            let (x, y) = VOLUMES_AT;
            let click = |o: &mut Vec<Op>, cx: i32, cy: i32| o.extend([Op::MouseMove(x + cx, y + cy), Op::MouseDown(x + cx, y + cy), Op::Frame(DT), Op::MouseUp(x + cx, y + cy), Op::Frame(DT)]);
            // a spinner's minus and plus: 11 px into its ends; the rows 58, 116, 174, 232
            let (minus_l, plus_l, minus_r, plus_r) = (61, 238, 281, 458);
            let row = |r: i32| 58 + r * 58 + 11;
            for (cx, r, n) in [(plus_l, 0, 2), (minus_l, 0, 1), (minus_l, 1, 3), (plus_l, 2, 1), (minus_l, 3, 1), (minus_r, 0, 1), (plus_r, 1, 1), (minus_r, 2, 2), (plus_r, 3, 1), (minus_r, 3, 1), (plus_r, 3, 4)] {
                for _ in 0..n {
                    click(&mut o, cx, row(r));
                }
            }
            if rest != "short" {
                // the master to 0 and past it, and up to 10 and past it
                for _ in 0..10 {
                    click(&mut o, minus_l, row(0));
                }
                for _ in 0..12 {
                    click(&mut o, plus_l, row(0));
                }
            }
            // bonnet inside, bumper outside, bumper inside
            for cx in [100, 420, 320] {
                click(&mut o, cx, 301);
            }
            // SET DRIVER with nothing chosen, a sound card chosen, SET DRIVER (its "Saved" for
            // five seconds, the warning)
            click(&mut o, 420, 464);
            click(&mut o, 150, 375);
            click(&mut o, 420, 464);
            o.extend(std::iter::repeat_n(Op::Frame(DT), if rest == "short" { 20 } else { 310 }));
            // the focus on the second car: its views' buttons
            o.extend([hud("app.focus", 1.0, ""), Op::Frame(DT), Op::Frame(DT)]);
            click(&mut o, 200, 301);
            o.extend([hud("app.focus", 0.0, ""), Op::Frame(DT)]);
            o.extend([hud("app.visible", 0.0, "volumes"), Op::Frame(DT), hud("app.visible", 1.0, "volumes"), Op::Frame(DT), Op::Frame(DT), Op::Save]);
        }
        "app_friends" => {
            // the race (no board for a race), a hot lap without penalties, with them (no board
            // found: no Steam), the two buttons, a time attack, a drift
            race(&mut o, 6);
            o.push(hud("app.new", 0.0, "friends"));
            o.push(Op::Frame(DT));
            o.push(hud("app.visible", 1.0, "friends"));
            play(&mut o, 0, 60, 20);
            o.extend([hud("app.session", 4.0, ""), Op::Frame(DT), Op::Frame(DT), hud("app.penalties", 1.0, ""), Op::Frame(DT), Op::Frame(DT)]);
            let (x, y) = FFB_AT;
            for (cx, cy) in [(425, 225), (25, 225)] {
                o.extend([Op::MouseMove(x + cx, y + cy), Op::Frame(DT), Op::MouseDown(x + cx, y + cy), Op::Frame(DT), Op::MouseUp(x + cx, y + cy), Op::Frame(DT)]);
            }
            o.extend([hud("app.session", 5.0, ""), Op::Frame(DT), hud("app.session", 6.0, ""), Op::Frame(DT), hud("app.session", 7.0, ""), Op::Frame(DT), hud("app.penalties", 0.0, ""), hud("app.session", 4.0, ""), Op::Frame(DT)]);
            o.extend([Op::MouseMove(600, 400), Op::Frame(DT)]);
            o.extend([hud("app.visible", 0.0, "friends"), Op::Frame(DT), hud("app.visible", 1.0, "friends"), Op::Frame(DT), Op::Frame(DT), Op::Save]);
        }
        "app_eyes" => {
            // Onboard settings of the F2004 (car.ini DRIVEREYES, ON_BOARD_PITCH_ANGLE -5.264918,
            // the game's camera_onboard.ini FOV 56), its car.ini a loose file
            race(&mut o, 6);
            o.push(hud("app.eyes", 0.0, "0.000573,0.682822,-0.0663019|56|-5.264918"));
            o.push(hud("app.new", 0.0, "eyes"));
            o.push(Op::Frame(DT));
            o.push(hud("app.visible", 1.0, "eyes"));
            play(&mut o, 0, 60, 20);
            let (x, y) = FFB_AT;
            let hold = |o: &mut Vec<Op>, cx: i32, cy: i32, frames: usize| {
                o.extend([Op::MouseMove(x + cx, y + cy), Op::MouseDown(x + cx, y + cy)]);
                o.extend(std::iter::repeat_n(Op::Frame(DT), frames));
                o.extend([Op::MouseUp(x + cx, y + cy), Op::Frame(DT)]);
            };
            let long = rest != "short";
            // the eye up, left; the seat back; the field of view wider, narrower; the view down
            for (cx, cy, n) in [(90, 80, 20), (40, 130, 10), (339, 145, 15), (339, 75, 30), (210, 75, 10), (210, 180, 20), (140, 130, 6), (90, 180, 8)] {
                hold(&mut o, cx, cy, if long { n * 3 } else { n });
            }
            // Save car.ini, Save Dash (both delete view.ini), more, then Save (view.ini and
            // camera_onboard.ini), Reset
            hold(&mut o, 105, 221, 1);
            hold(&mut o, 305, 221, 1);
            hold(&mut o, 339, 145, 9);
            hold(&mut o, 125, 260, 1);
            play(&mut o, 60, 120, 10);
            hold(&mut o, 275, 260, 1);
            o.extend([Op::MouseMove(600, 400), Op::Frame(DT)]);
            o.extend([hud("app.visible", 0.0, "eyes"), Op::Frame(DT), hud("app.visible", 1.0, "eyes"), Op::Frame(DT), Op::Frame(DT), Op::Save]);
        }
        "app_ballast" => {
            // the ballast spinner of the player's car: up 3, its arrows held, down to 0 and below
            race(&mut o, 6);
            o.push(hud("app.ersNames", 0.0, "-"));
            o.push(hud("app.new", 0.0, "ballast"));
            o.push(Op::Frame(DT));
            o.push(hud("app.visible", 1.0, "ballast"));
            play(&mut o, 0, 60, 20);
            let (x, y) = FFB_AT;
            let hold = |o: &mut Vec<Op>, cx: i32, cy: i32, frames: usize| {
                o.extend([Op::MouseMove(x + cx, y + cy), Op::MouseDown(x + cx, y + cy)]);
                o.extend(std::iter::repeat_n(Op::Frame(DT), frames));
                o.extend([Op::MouseUp(x + cx, y + cy), Op::Frame(DT)]);
            };
            // the spinner at (58, 70), 184 x 21: minus 58..79, plus 221..242
            for _ in 0..3 {
                hold(&mut o, 231, 80, 1);
            }
            hold(&mut o, 231, 80, if rest == "short" { 10 } else { 90 });
            hold(&mut o, 68, 80, if rest == "short" { 20 } else { 120 });
            play(&mut o, 60, 400, 20);
            o.extend([Op::MouseMove(150, 75), Op::Frame(DT), Op::Frame(DT)]);
            o.extend([hud("app.visible", 0.0, "ballast"), Op::Frame(DT), hud("app.visible", 1.0, "ballast"), Op::Frame(DT), Op::Frame(DT), Op::Save]);
        }
        "app_telemetry" | "app_telemetry_user" => {
            // the form is 600 x 700, its controls from (0, 20): the presets spinner (2, 50) 400 x 30 has its plus
            // at 372..402, graph k's spinner ((600 - 200) / 2, 100 + 150 k) 200 x 30 its minus at
            // 200..230 and plus at 370..400
            let click = |o: &mut Vec<Op>, x: i32, y: i32| o.extend([Op::MouseMove(x, y), Op::MouseDown(x, y), Op::Frame(DT), Op::MouseUp(x, y), Op::Frame(DT)]);
            race(&mut o, 6);
            o.push(hud("app.new", 0.0, "telemetry"));
            o.push(Op::Frame(DT));
            o.push(hud("app.visible", 1.0, "telemetry"));
            play(&mut o, 0, 600, 3);
            if app == "app_telemetry_user" {
                // graph 0: AI_PUSH, then ARB_FRONT (an auto range), back and forth
                click(&mut o, 385, 20 + 115);
                play(&mut o, 600, 900, 3);
                click(&mut o, 215, 20 + 115);
                click(&mut o, 385, 20 + 115);
                play(&mut o, 900, 1200, 3);
            } else {
                // the next presets, one after another; graph 3's channel by hand
                for k in 0..4 {
                    click(&mut o, 387, 20 + 65);
                    play(&mut o, 600 + k * 100, 700 + k * 100, 4);
                }
                click(&mut o, 385, 20 + 565);
                play(&mut o, 1000, 1200, 4);
            }
            o.extend([hud("app.visible", 0.0, "telemetry"), Op::Frame(DT), Op::Frame(DT), hud("app.visible", 1.0, "telemetry")]);
            play(&mut o, 1200, 1300, 4);
            o.push(Op::Save);
        }
        "app_setupdebug" => {
            race(&mut o, 6);
            o.push(hud("app.userFf", 1.0, ""));
            o.push(hud("app.new", 0.0, "setupdebug"));
            o.push(Op::Frame(DT));
            o.push(hud("app.visible", 1.0, "setupdebug"));
            play(&mut o, 0, 900, 3);
            // the focus elsewhere: that car's state, the same Car
            o.push(hud("app.focus", 2.0, ""));
            play(&mut o, 900, 1200, 3);
            o.extend([hud("app.visible", 0.0, "setupdebug"), Op::Frame(DT), Op::Frame(DT), hud("app.visible", 1.0, "setupdebug")]);
            play(&mut o, 1200, 1300, 4);
            o.push(Op::Save);
        }
        "app_carphysics" => {
            race(&mut o, 6);
            o.push(hud("app.new", 0.0, "carphysics"));
            o.push(Op::Frame(DT));
            o.push(hud("app.visible", 1.0, "carphysics"));
            play(&mut o, 0, 1200, 3);
            o.extend([hud("app.visible", 0.0, "carphysics"), Op::Frame(DT), Op::Frame(DT), hud("app.visible", 1.0, "carphysics")]);
            play(&mut o, 1200, 1300, 4);
            o.push(Op::Save);
        }
        "app_carengineer" => {
            race(&mut o, 6);
            o.push(hud("app.new", 0.0, "carengineer"));
            o.push(Op::Frame(DT));
            o.push(hud("app.visible", 1.0, "carengineer"));
            play(&mut o, 0, 1200, 3);
            o.extend([hud("app.visible", 0.0, "carengineer"), Op::Frame(DT), Op::Frame(DT), hud("app.visible", 1.0, "carengineer")]);
            play(&mut o, 1200, 1300, 4);
            o.push(Op::Save);
        }
        "app_wings" => {
            race(&mut o, 6);
            o.push(hud("app.new", 0.0, "wings"));
            o.push(Op::Frame(DT));
            o.push(hud("app.visible", 1.0, "wings"));
            play(&mut o, 0, 600, 3);
            o.extend([hud("app.visible", 0.0, "wings"), Op::Frame(DT), Op::Frame(DT), hud("app.visible", 1.0, "wings")]);
            play(&mut o, 600, 700, 4);
            o.push(Op::Save);
        }
        "app_gpuprofiler" => {
            race(&mut o, 6);
            o.push(hud("app.new", 0.0, "gpuprofiler"));
            o.push(Op::Frame(DT));
            o.push(hud("app.visible", 1.0, "gpuprofiler"));
            play(&mut o, 0, 300, 2);
            o.extend([hud("app.visible", 0.0, "gpuprofiler"), Op::Frame(DT), Op::Frame(DT), hud("app.visible", 1.0, "gpuprofiler")]);
            play(&mut o, 300, 340, 4);
            o.push(Op::Save);
        }
        "app_suspension" => {
            race(&mut o, 6);
            o.push(hud("app.new", 0.0, "suspension"));
            o.push(Op::Frame(DT));
            o.push(hud("app.visible", 1.0, "suspension"));
            play(&mut o, 0, 900, 3);
            // (no click on its TempRun button: the game's handler queues a job on the physics
            // thread, which the oracle has not)
            play(&mut o, 900, 960, 6);
            o.extend([hud("app.visible", 0.0, "suspension"), Op::Frame(DT), Op::Frame(DT), hud("app.visible", 1.0, "suspension")]);
            play(&mut o, 960, 1100, 4);
            o.push(Op::Save);
        }
        "app_drszones" => {
            // the button (180 x 30 at (5, 100) of the form at VOLUMES_AT), Save (100 x 30 at (5, 350))
            let (x, y) = VOLUMES_AT;
            let click = |o: &mut Vec<Op>, cx: i32, cy: i32| o.extend([Op::MouseMove(x + cx, y + cy), Op::MouseDown(x + cx, y + cy), Op::Frame(DT), Op::MouseUp(x + cx, y + cy), Op::Frame(DT)]);
            race(&mut o, 6);
            o.push(hud("app.new", 0.0, "drszones"));
            o.push(Op::Frame(DT));
            o.push(hud("app.visible", 1.0, "drszones"));
            play(&mut o, 0, 200, 20);
            // nothing to save yet; then the first press (it ends a zone from 0), two whole zones
            click(&mut o, 55, 365);
            for k in 0..7 {
                play(&mut o, 200 + k * 400, 400 + k * 400, 50);
                click(&mut o, 95, 115);
            }
            click(&mut o, 55, 365);
            play(&mut o, 3000, 3300, 50);
            click(&mut o, 95, 115);
            o.extend([hud("app.visible", 0.0, "drszones"), Op::Frame(DT), Op::Frame(DT), hud("app.visible", 1.0, "drszones")]);
            play(&mut o, 3300, 3400, 25);
            o.push(Op::Save);
        }
        "app_tyretester" => {
            // the tyre spinner (100 wide at (250, 10) of the form at VOLUMES_AT): its plus and minus
            let (x, y) = VOLUMES_AT;
            let click = |o: &mut Vec<Op>, cx: i32| o.extend([Op::MouseMove(x + cx, y + 25), Op::MouseDown(x + cx, y + 25), Op::Frame(DT), Op::MouseUp(x + cx, y + 25), Op::Frame(DT)]);
            race(&mut o, 6);
            o.push(hud("app.new", 0.0, "tyretester"));
            o.push(Op::Frame(DT));
            o.push(hud("app.visible", 1.0, "tyretester"));
            play(&mut o, 0, 300, 30);
            for _ in 0..2 {
                click(&mut o, 339);
            }
            play(&mut o, 300, 500, 40);
            for _ in 0..3 {
                click(&mut o, 339);
            }
            click(&mut o, 261);
            o.extend([hud("app.visible", 0.0, "tyretester"), Op::Frame(DT), Op::Frame(DT), hud("app.visible", 1.0, "tyretester")]);
            play(&mut o, 500, 600, 50);
            o.push(Op::Save);
        }
        "app_tyredebug" => {
            race(&mut o, 6);
            o.push(hud("app.new", 0.0, "tyredebug"));
            o.push(Op::Frame(DT));
            o.push(hud("app.visible", 1.0, "tyredebug"));
            play(&mut o, 0, 1200, 3);
            o.extend([hud("app.visible", 0.0, "tyredebug"), Op::Frame(DT), Op::Frame(DT), hud("app.visible", 1.0, "tyredebug")]);
            play(&mut o, 1200, 1300, 4);
            o.push(Op::Save);
        }
        "app_tyres" | "app_pedals" | "app_electronics" | "app_gears" | "app_mgu" | "app_sustelemetry" => {
            let which = leak(app[4..].to_string());
            race(&mut o, 6);
            o.extend([hud("app.compound", 23.0, "S"), hud("app.ff", 0.85, ""), hud("app.userFf", 1.0, "")]);
            o.push(hud("app.new", 0.0, which));
            o.push(Op::Frame(DT));
            o.push(hud("app.visible", 1.0, which));
            play(&mut o, 0, 600, 60);
            play(&mut o, 600, 1400, 4);
            if rest != "short" {
                play(&mut o, 1400, 21000, 97);
                // the focus elsewhere (Pedals follows it, the others keep the first car), a
                // stronger force, the gas held (its light), EDL and DRS
                o.push(hud("app.focus", 2.0, ""));
                play(&mut o, 9000, 9300, 10);
                o.extend([hud("app.focus", 0.0, ""), hud("app.ff", 2.6, ""), hud("app.userFf", 1.4, "")]);
                play(&mut o, 9300, 9600, 10);
                o.extend([hud("app.set", 1.0, "0:gas"), Op::Frame(DT), hud("app.set", 0.4, "0:gas"), Op::Frame(DT)]);
                o.extend([hud("app.edl", 0.05, ""), hud("app.drs", 1.0, ""), Op::Frame(DT), hud("app.edl", 0.6, ""), hud("app.set", 2.0, "0:statusBytes"), Op::Frame(DT), hud("app.set", 6.0, "0:statusBytes"), Op::Frame(DT), hud("app.edl", 0.0, "off"), hud("app.drs", 0.0, ""), Op::Frame(DT)]);
                o.extend([hud("app.compound", 26.0, "M"), Op::Frame(DT), hud("app.abs", 1.0, ""), Op::Frame(DT)]);
                play(&mut o, 15000, 15300, 5);
                handle(&mut o, 0, 80, 240);
                o.extend([hud("app.replay", 1.0, ""), Op::Frame(DT), Op::Frame(DT), hud("app.replay", 0.0, ""), Op::Frame(DT)]);
                o.extend([hud("app.visible", 0.0, which), Op::Frame(DT), hud("app.visible", 1.0, which), Op::Frame(DT), Op::Save]);
            }
        }
        _ => return None,
    }
    Some(o)
}
