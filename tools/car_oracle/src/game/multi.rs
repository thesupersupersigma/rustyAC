// SPDX-License-Identifier: GPL-3.0-or-later

//! Task 24: several cars in the game's own `PhysicsEngine`, free-running from step 0.
//!
//! Every car is made with the game's `Car::Car` in one engine (so the second car has
//! `physicsGUID` 1 and so on), gets its collider mesh, a scripted device of its own, and is
//! put on the track. Each step the drivers set their devices, `PhysicsEngine::step` runs, and
//! one row goes into the recording ([`rustyac_physics::engine::record`]): what every device
//! reported, every body of every car, damage, tyres, engine, the collision switches, and the
//! contact joints of the world (with the `bounce_vel` the game's joints really got).

use rustyac_physics::car::colliders::CarColliders;
use rustyac_physics::engine::record::{contact_words, CarMeta, ContactNote, Recording};
use rustyac_physics::track::Track;

use super::*;
use crate::track_driver::Grip;

/// How one car of a scenario is driven.
#[derive(Clone, Debug)]
pub enum Drive {
    /// On the brakes.
    Parked,
    /// By the clock: from each time on, these pedals and this wheel (gas, brake, steer).
    Script(Vec<(f32, f32, f32, f32)>),
    /// Along the AI line. `lateral`: (from this time on, metres to the left of the line),
    /// eased; `pace` and `limit` (m/s) as the track scenarios have them; from `brake_at` on
    /// the car stops.
    Follow { lateral: Vec<(f32, f32)>, pace: f32, limit: f32, brake_at: f32 },
    /// Task 28: along the AI line, and off it sideways by `zones`' metres (to the left)
    /// while the car is between two places round the lap (from, to, metres), from `after`
    /// seconds on.
    FollowZones { zones: Vec<(f32, f32, f32)>, pace: f32, limit: f32, after: f32 },
    /// Along the pit lane's line from `from` on, then straight on with this throttle until
    /// `brake_at`.
    PitLane { from: f32, then_gas: f32, brake_at: f32 },
    /// Task 28b: out of the pit box and back into it (a few metres forward, then in
    /// reverse), a stop there on the brakes; from `leave_at` on along the pit lane and onto
    /// the track with the line follower. The car shifts by hand.
    PitBox { leave_at: f32 },
    /// Task 27: by the game's own `AIDriver`, with this `basePush` (the AI level, 0..1) and
    /// `aggression`.
    Ai { push: f32, aggression: f32 },
    /// Task 27: by the game's own `AIDriverDrag`, with this `push` (the AI level, 0..100);
    /// it is told to start `start_ms` after the clock's start.
    Drag { push: f32, start_ms: f32 },
}

const VA_AIDRIVERDRAG_CTOR: usize = 0x1_402a_b740; // AIDriverDrag::AIDriverDrag(Car*)
const VA_AIDRIVERDRAG_SET_START_TIME: usize = 0x1_402a_c3d0; // AIDriverDrag::setStartTime(double)
const DRAG_PUSH: usize = 0x18;

/// Task 27: the session an AI scenario runs in (`None`: the engine's "undefined" session, in
/// which a driver just drives): (type, milliseconds from the clock's start to the green
/// light, the session's seconds, laps).
pub fn ai_session(scenario: &str) -> Option<(i32, f64, f64, i32)> {
    match scenario {
        "ai_solo" | "ai_solo_level" => Some((3, 3000.0, 0.0, 3)),
        "ai_race" | "ai_race_levels" | "ai_field" => Some((3, 6000.0, 0.0, 3)),
        "ai_pass_front" | "ai_pass_mid" | "ai_pass_rev" | "ai_pass_free" => Some((3, 5000.0, 0.0, 3)),
        "ai_practice" => Some((1, 0.0, 1800.0, 0)),
        "ai_handover" => Some((4, 0.0, 0.0, 0)),
        "ai_qualify" => Some((2, 0.0, 600.0, 0)),
        _ => None,
    }
}

/// Task 28: the referee of a scenario (`rc_*`): the session list in short
/// (`SessionsIni::from_list`), `[RACE] PENALTIES`, `JUMP_START_PENALTY`, and `startRace`'s
/// delay for the first session and for a later race, ms.
pub struct RefereeSpec {
    pub list: &'static str,
    pub penalties: bool,
    pub jump_start: Option<i32>,
    pub first_delay_ms: f32,
    pub next_delay_ms: f32,
}

pub fn referee_of(scenario: &str) -> Option<RefereeSpec> {
    let spec = |list: &'static str, penalties: bool, jump_start: Option<i32>| Some(RefereeSpec { list, penalties, jump_start, first_delay_ms: 5000.0, next_delay_ms: 4000.0 });
    match scenario {
        "rc_race" => spec("r3", true, None),
        "rc_timed" => spec("rt2+", true, None),
        "rc_timed_plain" => spec("rt2", false, None),
        "rc_weekend" => spec("p1,q1,r2", true, None),
        "rc_weekend_q" => spec("p1,q4,r2", true, None),
        // (the start 12 s after the session's: the car is already rolling when the window
        // before the start opens, in which the gearbox is locked and nobody may move)
        "rc_pass_front" | "rc_pass_rev" => spec("r3", true, Some(1)),
        "pit_box" | "pit_box_repair" | "pit_box_none" | "pit_box_wing" | "pit_feed" | "pit_mouse" => spec("p20", true, None),
        "pit_ai_race" | "pit_window_served" | "pit_window_missed" | "pit_window_early" => spec("r4", true, None),
        "pit_window_timed" => spec("rt5", true, None),
        "rc_jump" => Some(RefereeSpec { list: "r3", penalties: true, jump_start: Some(1), first_delay_ms: 12000.0, next_delay_ms: 4000.0 }),
        "rc_jump_dt" => Some(RefereeSpec { list: "r5", penalties: true, jump_start: Some(2), first_delay_ms: 12000.0, next_delay_ms: 4000.0 }),
        "rc_cut" => spec("r3", true, None),
        "rc_recover" => spec("r4", true, None),
        "rc_cut_practice" => spec("p3", true, None),
        _ => None,
    }
}

const VA_AIDRIVER_CTOR: usize = 0x1_4029_8c60; // AIDriver::AIDriver(Car*)
const VA_SET_SESSION_INFO: usize = 0x1_4026_4560; // PhysicsEngine::setSessionInfo(const SessionInfo&)
const PE_EV_ON_NEW_SESSION_PHYSICS: usize = 0xa0;
const CAR_EXPECTED_FUEL_PER_LAP: usize = 0x3d38;
const CAR_PIT_POSITION: usize = 0x3a90;
const AI_BASE_PUSH: usize = 0x28;
// Task 43c: what the Telemetry app reads of the player's car
const CAR_CONTROLS_PROVIDER: usize = 0x3d80;
const CAR_ERS: usize = 0x3768;
const AI_LIVE_OFFSET: usize = 0x13c;
const AI_OFFSET_DIST_FROM_CORNER: usize = 0x140;
const VA_AIDRIVER_GET_LATERAL_PUSH: usize = 0x1_4029_dd10; // AIDriver::getLateralPush()
const CAR_SUSPENSIONS: usize = 0x2c88;
const VA_RACE_ENGINEER_GET_KPI_RAD: usize = 0x1_4027_c060; // KPI RaceEngineer::getKPI_RAD(int)
const VA_RACE_ENGINEER_GET_FRONT_CASTER_RAD: usize = 0x1_4027_bd40;
const VA_GET_OVERSTEER_FACTOR: usize = 0x1_402b_11d0; // DynamicController::getOversteerFactor(Car*)
const VA_GET_WHEEL_STEER_ANGLE_DEG: usize = 0x1_4027_2960; // Car::getWheelSterAngleDEG(int)

/// Task 27: the step at which a scenario's first car, driven by the line follower until
/// then, is handed to the game's `AIDriver` (the game's "AI drives my car" switch: a new
/// driver with the constructor's `basePush` 1 and no aggression).
pub fn ai_handover(scenario: &str) -> Option<usize> {
    match scenario {
        "ai_handover" => Some(2400),
        _ => None,
    }
}
const AI_AGGRESSION: usize = 0x1c;

/// `SHGetFolderPathW` for the game's `Path::getDocumentPath` (`AIDriver::initSetup` looks for
/// an AI setup there): an empty folder name, so nothing of the user's Documents is read.
pub extern "system" fn sh_get_folder_path_w(_window: usize, _folder: i32, _token: usize, _flags: u32, path: *mut u16) -> i32 {
    unsafe {
        for (i, c) in "oracle_no_documents".encode_utf16().chain(std::iter::once(0)).enumerate() {
            *path.add(i) = c;
        }
    }
    0
}

/// Task 28c: the game's own `Track` of a folder (with its AI line) and the port's copy, in an
/// engine of their own: what a replay's playback asks the track.
pub unsafe fn track_for_replay(acs: &Acs, track_folder: &Path, layout: &str) -> Result<(*mut u8, std::sync::Arc<Track>), String> {
    let engine = new_engine(acs, 1);
    let built = crate::track::GameTrack::build(acs, engine, track_folder, layout, false)?;
    let port = std::sync::Arc::new(built.rust.clone());
    let built: &'static crate::track::GameTrack = Box::leak(Box::new(built));
    Ok((built.track, port))
}

/// One car of a scenario.
#[derive(Clone, Debug)]
pub struct MultiCar {
    pub name: String,
    pub position: V3,
    pub tail: V3,
    pub drive: Drive,
}

pub struct MultiScenario {
    pub name: &'static str,
    pub about: &'static str,
    pub seconds: f32,
    /// No contacts are looked for during the first steps, as at a session's start.
    pub no_collision_steps: i32,
    pub cars: fn(&Track, &[String]) -> Result<Vec<MultiCar>, String>,
    /// The cars when `--cars` names none.
    pub default_cars: &'static [&'static str],
}

const F2004: &str = "ks_ferrari_f2004";

/// A pose on the AI line at `n`, moved `back` metres against the driving direction and
/// `left` metres to the left.
fn line_pose(track: &Track, n: f32, back: f32, left: f32) -> Result<(V3, V3), String> {
    let spline = track.ai_spline.as_ref().ok_or("the track has no AI line")?;
    let n = spline.spline.wrap_position(n - back / spline.length());
    let (p, tail) = track.pose_on_ai_line_at(n).ok_or("no pose on the AI line")?;
    // forward = -tail; left of forward, level
    let (fx, fz) = (-tail.x, -tail.z);
    let l = (fx * fx + fz * fz).sqrt().max(1e-6);
    let (lx, lz) = (fz / l, -fx / l);
    let moved = rustyac_physics::vecmath::Vec3f::new(p.x + lx * left, p.y + 3.0, p.z + lz * left);
    use rustyac_physics::tyre::RayTrackCollisionProvider;
    let ground = track.ray_cast(&moved, &rustyac_physics::vecmath::Vec3f::new(0.0, -1.0, 0.0), 20.0).map(|hit| hit.pos.y).unwrap_or(p.y);
    Ok(([moved.x, ground, moved.z], [tail.x, tail.y, tail.z]))
}

/// Task 28: where a car that follows the AI line can leave the track over a surface with a
/// penalty (`BLACK_FLAG_TIME` above 0 in the track's surfaces.ini): up to `count` stretches of
/// the lap, each with the metres to the left of the line at which the surface lies.
fn penalty_zones(track: &Track, count: usize) -> Vec<(f32, f32, f32)> {
    let mut zones: Vec<(f32, f32, f32)> = Vec::new();
    let Some(spline) = track.ai_spline.as_ref() else {
        return zones;
    };
    let steps = (spline.length() / 2.0) as usize;
    let mut open: Option<(f32, f32)> = None;
    for i in 0..steps {
        let n = 0.08 + 0.9 * i as f32 / steps as f32;
        let mut found = None;
        for side in [6.0f32, -6.0, 8.0, -8.0, 10.0, -10.0, 12.0, -12.0, 15.0, -15.0, 18.0, -18.0] {
            if open.is_some_and(|zone| zone.1 != side) {
                continue;
            }
            let Some((p, tail)) = track.pose_on_ai_line_at(n) else { continue };
            let (fx, fz) = (-tail.x, -tail.z);
            let l = (fx * fx + fz * fz).sqrt().max(1e-6);
            let point = rustyac_physics::vecmath::Vec3f::new(p.x + fz / l * side, p.y + 3.0, p.z - fx / l * side);
            if track.ray_cast_hit(&point, &rustyac_physics::vecmath::Vec3f::new(0.0, -1.0, 0.0), 20.0).is_some_and(|hit| hit.surface_def.black_flag_time > 0.0) {
                found = Some(side);
                break;
            }
        }
        match (open, found) {
            (None, Some(side)) => open = Some((n, side)),
            (Some((from, side)), None) => {
                // (a stretch of at least 40 m, entered a little before and left a little after)
                if (n - from) * spline.length() >= 12.0 {
                    zones.push((from - 15.0 / spline.length(), n - 10.0 / spline.length(), side));
                    if zones.len() == count {
                        break;
                    }
                }
                open = None;
            }
            _ => {}
        }
    }
    zones
}

fn name_of(cars: &[String], index: usize, default: &[&str]) -> String {
    cars.get(index).cloned().unwrap_or_else(|| default[index % default.len()].to_string())
}

/// The straight after Raidillon on Spa, as a share of the lap; any track's line works.
const STRAIGHT: f32 = 0.20;

pub fn scenarios() -> Vec<MultiScenario> {
    vec![
        // Task 25: the local car of an online session played from a tape (`car_oracle net`)
        MultiScenario {
            name: "net_watch",
            about: "online: the local car stands beside the racing line a little after the start line; the tape's cars pass",
            seconds: 60.0,
            no_collision_steps: 250,
            default_cars: &[F2004],
            cars: |track, cars| {
                let (position, tail) = line_pose(track, 0.03, 0.0, 4.5)?;
                Ok(vec![MultiCar { name: name_of(cars, 0, &[F2004]), position, tail, drive: Drive::Parked }])
            },
        },
        MultiScenario {
            name: "net_block",
            about: "online: the local car stands on the racing line a little after the start line; the tape's cars run into it",
            seconds: 30.0,
            no_collision_steps: 250,
            default_cars: &[F2004],
            cars: |track, cars| {
                let (position, tail) = line_pose(track, 0.03, 0.0, 0.0)?;
                Ok(vec![MultiCar { name: name_of(cars, 0, &[F2004]), position, tail, drive: Drive::Parked }])
            },
        },
        MultiScenario {
            name: "net_drive",
            about: "online: the local car drives the racing line from a little after the start line, among the tape's cars",
            seconds: 60.0,
            no_collision_steps: 250,
            default_cars: &[F2004],
            cars: |track, cars| {
                let (position, tail) = line_pose(track, 0.03, 0.0, 0.0)?;
                Ok(vec![MultiCar { name: name_of(cars, 0, &[F2004]), position, tail, drive: Drive::Follow { lateral: vec![(0.0, 0.0)], pace: 10.0, limit: 200.0, brake_at: 50.0 } }])
            },
        },
        MultiScenario {
            name: "net_pit",
            about: "online: the local car stands in the pit lane: every remote car is a ghost",
            seconds: 20.0,
            no_collision_steps: 250,
            default_cars: &[F2004],
            cars: |track, cars| {
                let (position, tail) = track.spawn_pose("PIT", 5).ok_or("the track has no pit box 5")?;
                Ok(vec![MultiCar { name: name_of(cars, 0, &[F2004]), position: [position.x, position.y, position.z], tail: [tail.x, tail.y, tail.z], drive: Drive::Parked }])
            },
        },
        MultiScenario {
            name: "mc_side_by_side",
            about: "two cars side by side down a straight, flat out, then on the brakes",
            seconds: 12.0,
            no_collision_steps: 0,
            default_cars: &[F2004, F2004],
            cars: |track, cars| {
                let mut out = Vec::new();
                for (i, left) in [1.6f32, -1.6].into_iter().enumerate() {
                    let (position, tail) = line_pose(track, STRAIGHT, 0.0, left)?;
                    out.push(MultiCar { name: name_of(cars, i, &[F2004]), position, tail, drive: Drive::Follow { lateral: vec![(0.0, left)], pace: 10.0, limit: 200.0, brake_at: 9.0 } });
                }
                Ok(out)
            },
        },
        MultiScenario {
            name: "mc_rear_end",
            about: "a car flat out into the back of a standing one",
            seconds: 9.0,
            no_collision_steps: 0,
            default_cars: &[F2004, F2004],
            cars: |track, cars| {
                let (p0, t0) = line_pose(track, STRAIGHT, 0.0, 0.0)?;
                let (p1, t1) = line_pose(track, STRAIGHT, 45.0, 0.0)?;
                Ok(vec![
                    MultiCar { name: name_of(cars, 0, &[F2004]), position: p0, tail: t0, drive: Drive::Parked },
                    MultiCar { name: name_of(cars, 1, &[F2004]), position: p1, tail: t1, drive: Drive::Follow { lateral: vec![(0.0, 0.0)], pace: 10.0, limit: 200.0, brake_at: 6.0 } },
                ])
            },
        },
        MultiScenario {
            name: "mc_side_swipe",
            about: "two cars side by side; after three seconds one turns into the other",
            seconds: 10.0,
            no_collision_steps: 0,
            default_cars: &[F2004, F2004],
            cars: |track, cars| {
                let (p0, t0) = line_pose(track, STRAIGHT, 0.0, 1.6)?;
                // (the car on the right is slower off the line: it starts eight metres ahead)
                let (p1, t1) = line_pose(track, STRAIGHT, -8.0, -1.6)?;
                Ok(vec![
                    MultiCar { name: name_of(cars, 0, &[F2004]), position: p0, tail: t0, drive: Drive::Follow { lateral: vec![(0.0, 1.6)], pace: 10.0, limit: 200.0, brake_at: 8.0 } },
                    MultiCar { name: name_of(cars, 1, &[F2004]), position: p1, tail: t1, drive: Drive::Follow { lateral: vec![(0.0, -1.6), (2.8, 2.4)], pace: 10.0, limit: 200.0, brake_at: 8.0 } },
                ])
            },
        },
        MultiScenario {
            name: "mc_pile_up",
            about: "four cars in a row leave the road to the left into the barrier, one after the other",
            seconds: 14.0,
            no_collision_steps: 0,
            default_cars: &[F2004, F2004, F2004, F2004],
            cars: |track, cars| {
                let mut out = Vec::new();
                for i in 0..4 {
                    let (position, tail) = line_pose(track, STRAIGHT, 16.0 * i as f32, 0.0)?;
                    out.push(MultiCar {
                        name: name_of(cars, i, &[F2004]),
                        position,
                        tail,
                        drive: Drive::Follow { lateral: vec![(0.0, 0.0), (2.5, 60.0)], pace: 10.0, limit: 200.0, brake_at: 9.0 },
                    });
                }
                Ok(out)
            },
        },
        MultiScenario {
            name: "mc_pit_lane",
            about: "a car parked in the pit lane; two more drive through it and out of the lane, nose to tail (the pit-lane rule)",
            seconds: 34.0,
            no_collision_steps: 0,
            default_cars: &[F2004, F2004, F2004],
            cars: |track, cars| {
                let pose = |n: f32| -> Result<(V3, V3), String> {
                    let (p, tail) = crate::track_driver::pit_pose(track, n).ok_or("the track has no pit lane line")?;
                    Ok(([p.x, p.y, p.z], [tail.x, tail.y, tail.z]))
                };
                let length = track.pit_lane_spline.as_ref().ok_or("the track has no pit lane line")?.length();
                let (p0, t0) = pose(0.30)?;
                let (p1, t1) = pose(0.30 - 14.0 / length)?;
                let (p2, t2) = pose(0.30 - 19.0 / length)?;
                Ok(vec![
                    MultiCar { name: name_of(cars, 0, &[F2004]), position: p0, tail: t0, drive: Drive::Parked },
                    MultiCar { name: name_of(cars, 1, &[F2004]), position: p1, tail: t1, drive: Drive::PitLane { from: 0.5, then_gas: 1.0, brake_at: 30.0 } },
                    MultiCar { name: name_of(cars, 2, &[F2004]), position: p2, tail: t2, drive: Drive::PitLane { from: 0.5, then_gas: 0.35, brake_at: 30.0 } },
                ])
            },
        },
        MultiScenario {
            name: "mc_field",
            about: "six cars from the grid on a lap with the line follower",
            seconds: 150.0,
            no_collision_steps: 250,
            default_cars: &[F2004, F2004, F2004, F2004, F2004, F2004],
            cars: |track, cars| grid(track, cars, &[F2004], 6, 0.78, 0.035),
        },
        MultiScenario {
            name: "mc_mixed",
            about: "four different cars from the grid: F2004, E30, a 4WD car, a live-axle car",
            seconds: 75.0,
            no_collision_steps: 250,
            default_cars: &[F2004, "bmw_m3_e30", "ks_lamborghini_sesto_elemento", "ks_ferrari_250_gto"],
            cars: |track, cars| grid(track, cars, &[F2004, "bmw_m3_e30", "ks_lamborghini_sesto_elemento", "ks_ferrari_250_gto"], 4, 0.72, 0.0),
        },
        MultiScenario {
            name: "mc_pass",
            about: "Task 24b: one car stands on the straight, a second comes from 300 m behind and passes it 3.5 m to its side at speed",
            seconds: 14.0,
            no_collision_steps: 0,
            default_cars: &[F2004, F2004],
            cars: |track, cars| {
                let (p0, t0) = line_pose(track, STRAIGHT, 0.0, 0.0)?;
                let (p1, t1) = line_pose(track, STRAIGHT, 300.0, 3.5)?;
                Ok(vec![
                    MultiCar { name: name_of(cars, 0, &[F2004]), position: p0, tail: t0, drive: Drive::Parked },
                    MultiCar { name: name_of(cars, 1, &[F2004]), position: p1, tail: t1, drive: Drive::Follow { lateral: vec![(0.0, 3.5)], pace: 0.9, limit: 95.0, brake_at: 11.0 } },
                ])
            },
        },
        MultiScenario {
            name: "mc_train",
            about: "Task 24b: as many cars as `--cars` names (26 by default) in two columns along the line after the start, 9 m apart, each following the line; more cars than any grid has slots",
            seconds: 12.0,
            no_collision_steps: 250,
            default_cars: &[F2004],
            cars: |track, cars| {
                let count = if cars.is_empty() { 26 } else { cars.len() };
                let mut out = Vec::new();
                for i in 0..count {
                    let side = if i % 2 == 0 { 2.2 } else { -2.2 };
                    let (position, tail) = line_pose(track, 0.06, 9.0 * i as f32, side)?;
                    out.push(MultiCar { name: name_of(cars, i, &[F2004]), position, tail, drive: Drive::Follow { lateral: vec![(0.0, side)], pace: 0.70 - 0.012 * i as f32, limit: 95.0, brake_at: f32::MAX } });
                }
                Ok(out)
            },
        },
        MultiScenario {
            name: "ai_solo",
            about: "Task 27: one car on the first grid slot, driven by the game's AIDriver at level 100 in a race of its own (green light after 3 s); give --steps for a whole lap",
            seconds: 60.0,
            no_collision_steps: 250,
            default_cars: &[F2004],
            cars: |track, cars| ai_grid(track, cars, &[F2004], 1, |_| 1.0, |_| 0.0),
        },
        MultiScenario {
            name: "ai_solo_level",
            about: "Task 27: as ai_solo with AI_LEVEL 82 and AI_AGGRESSION 60 (basePush 0.82, aggression 0.57)",
            seconds: 60.0,
            no_collision_steps: 250,
            default_cars: &[F2004],
            cars: |track, cars| ai_grid(track, cars, &[F2004], 1, |_| 0.82, |_| 0.6 * 0.95),
        },
        MultiScenario {
            name: "ai_field",
            about: "Task 27: AI drivers on the first grid slots (as many as --cars names, 12 by default), a race with the green light after 6 s",
            seconds: 60.0,
            no_collision_steps: 250,
            default_cars: &[F2004],
            cars: |track, cars| ai_grid(track, cars, &[F2004], 12, |i| 1.0 - 0.02 * (i % 4) as f32, |i| 0.2 * (i % 3) as f32),
        },
        MultiScenario {
            name: "ai_race",
            about: "Task 27: a race from the grid: AI drivers on the first slots (as many as --cars names, 6 by default), the lights go green after 6 s",
            seconds: 90.0,
            no_collision_steps: 250,
            default_cars: &[F2004],
            cars: |track, cars| ai_grid(track, cars, &[F2004], 6, |i| 1.0 - 0.015 * (i % 5) as f32, |i| 0.19 * (i % 4) as f32),
        },
        MultiScenario {
            name: "ai_pass_front",
            about: "Task 28b: a race start in which the first car stays on the grid on its brakes; 3 AI drivers behind it have to pass a stopped car",
            seconds: 30.0,
            no_collision_steps: 250,
            default_cars: &[F2004],
            cars: |track, cars| {
                let mut out = ai_grid(track, cars, &[F2004], 4, |_| 0.95, |_| 0.1)?;
                out[0].drive = Drive::Parked;
                Ok(out)
            },
        },
        MultiScenario {
            name: "ai_pass_mid",
            about: "Task 28b: as ai_pass_front with the second car stopped (an AI driver ahead of it, more behind)",
            seconds: 30.0,
            no_collision_steps: 250,
            default_cars: &[F2004],
            cars: |track, cars| {
                let mut out = ai_grid(track, cars, &[F2004], 5, |i| 1.0 - 0.02 * i as f32, |i| 0.15 * (i % 3) as f32)?;
                out[1].drive = Drive::Parked;
                Ok(out)
            },
        },
        MultiScenario {
            name: "ai_pass_rev",
            about: "Task 28b: as ai_pass_front, the stopped car in neutral with the throttle open (the line follower without its gearbox, as in the early jump-start variant)",
            seconds: 30.0,
            no_collision_steps: 250,
            default_cars: &[F2004],
            cars: |track, cars| {
                let mut out = ai_grid(track, cars, &[F2004], 4, |_| 0.95, |_| 0.1)?;
                out[0].drive = Drive::Follow { lateral: vec![(0.0, 0.0)], pace: 0.75, limit: 55.0, brake_at: 1.0e9 };
                Ok(out)
            },
        },
        MultiScenario {
            name: "ai_pass_free",
            about: "Task 28b: as ai_pass_front, the stopped car with no pedal at all (it may roll)",
            seconds: 30.0,
            no_collision_steps: 250,
            default_cars: &[F2004],
            cars: |track, cars| {
                let mut out = ai_grid(track, cars, &[F2004], 4, |i| 0.9 + 0.03 * i as f32, |i| 0.2 * i as f32)?;
                out[0].drive = Drive::Script(vec![(0.0, 0.0, 0.0, 0.0)]);
                Ok(out)
            },
        },
        MultiScenario {
            name: "rc_pass_front",
            about: "Task 28b: ai_pass_front under the game's referee (a race of 3 laps, JUMP_START_PENALTY=1, the green 5 s after the session's start)",
            seconds: 30.0,
            no_collision_steps: 250,
            default_cars: &[F2004],
            cars: |track, cars| {
                let mut out = ai_grid(track, cars, &[F2004], 4, |_| 0.95, |_| 0.1)?;
                out[0].drive = Drive::Parked;
                Ok(out)
            },
        },
        MultiScenario {
            name: "rc_pass_rev",
            about: "Task 28b: the early jump-start variant itself: the line follower without its gearbox on the first slot under the referee, 3 AI drivers",
            seconds: 30.0,
            no_collision_steps: 250,
            default_cars: &[F2004],
            cars: |track, cars| {
                let mut out = ai_grid(track, cars, &[F2004], 4, |_| 0.95, |_| 0.1)?;
                out[0].drive = Drive::Follow { lateral: vec![(0.0, 0.0)], pace: 0.75, limit: 55.0, brake_at: 1.0e9 };
                Ok(out)
            },
        },
        MultiScenario {
            name: "pit_box",
            about: "Task 28b: the player's pit stop. A practice; the first car drives out of its pit box and backs into it again, stops (fuel, another compound, other pressures), and drives out onto the track; AI drivers in the other boxes",
            seconds: 90.0,
            no_collision_steps: 250,
            default_cars: &[F2004],
            cars: |track, cars| pit_boxes(track, cars, 3, 34.0),
        },
        MultiScenario {
            name: "pit_box_wing",
            about: "Task 28b: as pit_box with a stop that adds 3 litres and three clicks of front wing (a car whose setup.ini has a wing with PITSTOP=)",
            seconds: 90.0,
            no_collision_steps: 250,
            default_cars: &[F2004],
            cars: |track, cars| pit_boxes(track, cars, 2, 30.0),
        },
        MultiScenario {
            name: "pit_feed",
            about: "Task 28b: the pit stop's state machine on made-up states: as pit_box, but what the game's PitStop reads of the car (place, speed, fuel, pit lane, black flag), the frame's length, the menu's values and the car's damage are drawn from the seed (--seed), frame by frame; whatever the stops then do to the car is real",
            seconds: 120.0,
            no_collision_steps: 250,
            default_cars: &[F2004],
            cars: |track, cars| pit_boxes(track, cars, 2, 20.0),
        },
        MultiScenario {
            name: "pit_mouse",
            about: "Task 34: as pit_feed with the old mouse pit menu (Sim::useMousePitstop): the game's own button handlers, fuel gauge and confirm are clicked at made-up moments from the seed, and its state \"choosing\" waits for them",
            seconds: 120.0,
            no_collision_steps: 250,
            default_cars: &[F2004],
            cars: |track, cars| pit_boxes(track, cars, 2, 20.0),
        },
        MultiScenario {
            name: "pit_box_repair",
            about: "Task 28b: as pit_box with a car that starts damaged (four body zones, the engine, a bent suspension): a repair stop (body, suspension, engine), no fuel, the tyres stay",
            seconds: 125.0,
            no_collision_steps: 250,
            default_cars: &[F2004],
            cars: |track, cars| pit_boxes(track, cars, 2, 100.0),
        },
        MultiScenario {
            name: "pit_box_none",
            about: "Task 28b: as pit_box with a menu that asks for nothing: the stop is over at once and nothing changes",
            seconds: 40.0,
            no_collision_steps: 250,
            default_cars: &[F2004],
            cars: |track, cars| pit_boxes(track, cars, 2, 22.0),
        },
        MultiScenario {
            name: "pit_ai_race",
            about: "Task 28b: a race of 4 laps, every car with the game's AI driver; the first two are called in on the first lap and stop at the same time; the first car's stop is also the player's (fuel, another compound)",
            seconds: 400.0,
            no_collision_steps: 250,
            default_cars: &[F2004],
            cars: |track, cars| ai_grid(track, cars, &[F2004], 4, |i| 1.0 - 0.02 * i as f32, |i| 0.1 * i as f32),
        },
        MultiScenario {
            name: "pit_window_served",
            about: "Task 28b: a race of 4 laps with a mandatory pit window (open from the leader's lap 1, closed at lap 3); the first car is called in on lap 2: the stop counts",
            seconds: 400.0,
            no_collision_steps: 250,
            default_cars: &[F2004],
            cars: |track, cars| ai_grid(track, cars, &[F2004], 3, |i| 1.0 - 0.02 * i as f32, |i| 0.1 * i as f32),
        },
        MultiScenario {
            name: "pit_window_missed",
            about: "Task 28b: as pit_window_served, nobody is called in: the first car is black-flagged when the window closes",
            seconds: 400.0,
            no_collision_steps: 250,
            default_cars: &[F2004],
            cars: |track, cars| ai_grid(track, cars, &[F2004], 3, |i| 1.0 - 0.02 * i as f32, |i| 0.1 * i as f32),
        },
        MultiScenario {
            name: "pit_window_early",
            about: "Task 28b: as pit_window_served with the window open from lap 2 and the first car called in on lap 1: a stop before the window does not count, and the black flag falls when it closes",
            seconds: 400.0,
            no_collision_steps: 250,
            default_cars: &[F2004],
            cars: |track, cars| ai_grid(track, cars, &[F2004], 3, |i| 1.0 - 0.02 * i as f32, |i| 0.1 * i as f32),
        },
        MultiScenario {
            name: "pit_window_timed",
            about: "Task 28b: a timed race of 5 minutes with a mandatory pit window in minutes (open after 1, closed after 3); the first car is called in during the second minute: the stop counts",
            seconds: 400.0,
            no_collision_steps: 250,
            default_cars: &[F2004],
            cars: |track, cars| ai_grid(track, cars, &[F2004], 3, |i| 1.0 - 0.02 * i as f32, |i| 0.1 * i as f32),
        },
        MultiScenario {
            name: "rc_race",
            about: "Task 28: a race of 3 laps with the game's referee: AI drivers on the grid (6 by default), the start 5 s after the session's, race over, results, race_out.json",
            seconds: 330.0,
            no_collision_steps: 250,
            default_cars: &[F2004],
            cars: |track, cars| ai_grid(track, cars, &[F2004], 6, |i| 1.0 - 0.015 * (i % 5) as f32, |i| 0.19 * (i % 4) as f32),
        },
        MultiScenario {
            name: "rc_timed",
            about: "Task 28: a race of 2 minutes and the lap after (ADDITITIONAL_LAP), 4 AI drivers",
            seconds: 330.0,
            no_collision_steps: 250,
            default_cars: &[F2004],
            cars: |track, cars| ai_grid(track, cars, &[F2004], 4, |i| 1.0 - 0.02 * i as f32, |i| 0.2 * (i % 3) as f32),
        },
        MultiScenario {
            name: "rc_timed_plain",
            about: "Task 28: a race of 2 minutes without the extra lap, 3 AI drivers",
            seconds: 300.0,
            no_collision_steps: 250,
            default_cars: &[F2004],
            cars: |track, cars| ai_grid(track, cars, &[F2004], 3, |i| 1.0 - 0.02 * i as f32, |_| 0.1),
        },
        MultiScenario {
            name: "rc_weekend_q",
            about: "Task 28: practice (1 min), qualifying (4 min: every car sets a time) and a race of 2 laps; the quicker drivers are in the later pit boxes, so the race's grid is not the entry list's order",
            seconds: 650.0,
            no_collision_steps: 250,
            default_cars: &[F2004],
            cars: |track, cars| {
                let count = if cars.is_empty() { 4 } else { cars.len() };
                let mut out = Vec::new();
                for i in 0..count {
                    let (p, tail) = track.spawn_pose("PIT", i).ok_or_else(|| format!("the track has no pit box {i}"))?;
                    out.push(MultiCar { name: name_of(cars, i, &[F2004]), position: [p.x, p.y, p.z], tail: [tail.x, tail.y, tail.z], drive: Drive::Ai { push: 0.88 + 0.04 * i as f32, aggression: 0.1 * i as f32 } });
                }
                Ok(out)
            },
        },
        MultiScenario {
            name: "rc_weekend",
            about: "Task 28: practice (1 min), qualifying (1 min) and a race of 2 laps with the grid from qualifying; AI drivers from the pit boxes (4 by default)",
            seconds: 480.0,
            no_collision_steps: 250,
            default_cars: &[F2004],
            cars: |track, cars| {
                let count = if cars.is_empty() { 4 } else { cars.len() };
                let mut out = Vec::new();
                for i in 0..count {
                    let (p, tail) = track.spawn_pose("PIT", i).ok_or_else(|| format!("the track has no pit box {i}"))?;
                    out.push(MultiCar { name: name_of(cars, i, &[F2004]), position: [p.x, p.y, p.z], tail: [tail.x, tail.y, tail.z], drive: Drive::Ai { push: 1.0 - 0.03 * i as f32, aggression: 0.1 * i as f32 } });
                }
                Ok(out)
            },
        },
        MultiScenario {
            name: "rc_jump",
            about: "Task 28: a race of 3 laps; the first car (the line follower) drives off before the green light: back to its pit box, controls locked for 20 s (JUMP_START_PENALTY=1); 3 AI drivers",
            seconds: 120.0,
            no_collision_steps: 250,
            default_cars: &[F2004],
            cars: |track, cars| {
                let mut out = ai_grid(track, cars, &[F2004], 4, |_| 0.95, |_| 0.1)?;
                out[0].drive = Drive::Follow { lateral: vec![(0.0, 0.0)], pace: 0.75, limit: 55.0, brake_at: 1.0e9 };
                Ok(out)
            },
        },
        MultiScenario {
            name: "rc_jump_dt",
            about: "Task 28: a race of 5 laps; the first car jumps the start and gets a drive-through (JUMP_START_PENALTY=2) it never serves: black flag at the line with one lap left to serve; 2 AI drivers",
            seconds: 420.0,
            no_collision_steps: 250,
            default_cars: &[F2004],
            cars: |track, cars| {
                let mut out = ai_grid(track, cars, &[F2004], 3, |_| 0.9, |_| 0.1)?;
                out[0].drive = Drive::Follow { lateral: vec![(0.0, 0.0)], pace: 0.75, limit: 55.0, brake_at: 1.0e9 };
                Ok(out)
            },
        },
        MultiScenario {
            name: "rc_cut",
            about: "Task 28: a race of 3 laps with penalties; the first car (the line follower) leaves the track with all four wheels now and then: time penalties, a black flag when one is still unserved at the line; 2 AI drivers",
            seconds: 300.0,
            no_collision_steps: 250,
            default_cars: &[F2004],
            cars: |track, cars| {
                let mut out = ai_grid(track, cars, &[F2004], 3, |_| 0.9, |_| 0.1)?;
                out.swap(0, 2);
                let (first, last) = (out[2].clone(), out[0].clone());
                out[0].position = first.position;
                out[0].tail = first.tail;
                out[2].position = last.position;
                out[2].tail = last.tail;
                out[2].drive = out[1].drive.clone();
                let zones = penalty_zones(track, 2);
                if zones.is_empty() {
                    return Err("the track has no surface with a penalty beside its AI line".to_string());
                }
                out[0].drive = Drive::FollowZones { zones, pace: 0.6, limit: 40.0, after: 0.0 };
                Ok(out)
            },
        },
        MultiScenario {
            name: "rc_recover",
            about: "Task 43c: rc_cut's race of 4 laps whose line follower laps cleanly first and cuts from 150 s on; with CAR_ORACLE_PENALTY_MODE=2 CAR_ORACLE_PERF_METER=1 the cuts give \"recover time\" penalties (5 s against the PerformanceMeter's best lap)",
            seconds: 420.0,
            no_collision_steps: 250,
            default_cars: &[F2004],
            cars: |track, cars| {
                let mut out = ai_grid(track, cars, &[F2004], 3, |_| 0.9, |_| 0.1)?;
                out.swap(0, 2);
                let (first, last) = (out[2].clone(), out[0].clone());
                out[0].position = first.position;
                out[0].tail = first.tail;
                out[2].position = last.position;
                out[2].tail = last.tail;
                out[2].drive = out[1].drive.clone();
                let zones = penalty_zones(track, 2);
                if zones.is_empty() {
                    return Err("the track has no surface with a penalty beside its AI line".to_string());
                }
                out[0].drive = Drive::FollowZones { zones, pace: 0.6, limit: 40.0, after: 150.0 };
                Ok(out)
            },
        },
        MultiScenario {
            name: "rc_cut_practice",
            about: "Task 28: a practice of 3 minutes with penalties; the first car (the line follower, from the grid) cuts now and then: laps with cuts that do not count; 1 AI driver from its pit box",
            seconds: 200.0,
            no_collision_steps: 250,
            default_cars: &[F2004],
            cars: |track, cars| {
                let mut out = ai_grid(track, cars, &[F2004], 2, |_| 0.9, |_| 0.1)?;
                let zones = penalty_zones(track, 2);
                if zones.is_empty() {
                    return Err("the track has no surface with a penalty beside its AI line".to_string());
                }
                out[0].drive = Drive::FollowZones { zones, pace: 0.6, limit: 40.0, after: 0.0 };
                Ok(out)
            },
        },
        MultiScenario {
            name: "ai_practice",
            about: "Task 27: a practice session: AI drivers in the first pit boxes (as many as --cars names, 2 by default); each waits, drives out along the pit lane's line, does its laps and comes in again",
            seconds: 240.0,
            no_collision_steps: 250,
            default_cars: &[F2004],
            cars: |track, cars| {
                let count = if cars.is_empty() { 2 } else { cars.len() };
                let mut out = Vec::new();
                for i in 0..count {
                    let (p, tail) = track.spawn_pose("PIT", i).ok_or_else(|| format!("the track has no pit box {i}"))?;
                    out.push(MultiCar { name: name_of(cars, i, &[F2004]), position: [p.x, p.y, p.z], tail: [tail.x, tail.y, tail.z], drive: Drive::Ai { push: 1.0 - 0.03 * i as f32, aggression: 0.1 * i as f32 } });
                }
                Ok(out)
            },
        },
        MultiScenario {
            name: "ai_handover",
            about: "Task 27: one car from the hot-lap start, driven by the oracle's line follower; after 7.2 s it is handed to the game's AIDriver (the game's \"AI drives my car\" switch)",
            seconds: 60.0,
            no_collision_steps: 250,
            default_cars: &[F2004],
            cars: |track, cars| {
                let (position, tail) = line_pose(track, 0.02, 0.0, 0.0)?;
                Ok(vec![MultiCar { name: name_of(cars, 0, &[F2004]), position, tail, drive: Drive::Follow { lateral: vec![(0.0, 0.0)], pace: 0.8, limit: 70.0, brake_at: f32::MAX } }])
            },
        },
        MultiScenario {
            name: "ai_drag",
            about: "Task 27: a drag race: the game's AIDriverDrag in every car on the first grid slots (2 by default), levels 100 and 90, the start after 4 s",
            seconds: 20.0,
            no_collision_steps: 250,
            default_cars: &[F2004],
            cars: |track, cars| {
                let count = if cars.is_empty() { 2 } else { cars.len() };
                let mut out = Vec::new();
                for i in 0..count {
                    let (p, tail) = track.spawn_pose("START", i).ok_or_else(|| format!("the track has no grid slot {i}"))?;
                    out.push(MultiCar { name: name_of(cars, i, &[F2004]), position: [p.x, p.y, p.z], tail: [tail.x, tail.y, tail.z], drive: Drive::Drag { push: 100.0 - 10.0 * i as f32, start_ms: 4000.0 } });
                }
                Ok(out)
            },
        },
        MultiScenario {
            name: "ai_race_levels",
            about: "Task 27: as ai_race with slow drivers in front (level 80 to 100 from the pole back) and aggression up to 95 %: overtakes",
            seconds: 90.0,
            no_collision_steps: 250,
            default_cars: &[F2004],
            cars: |track, cars| ai_grid(track, cars, &[F2004], 6, |i| (0.80 + 0.04 * i as f32).min(1.0), |i| (0.19 * i as f32).min(0.95)),
        },
    ]
}

/// `count` cars on the first grid slots, each following the line at its own pace (the car in
/// front the fastest).
/// Task 28b: what a pit stop scenario sets up beside its referee.
pub struct PitSpec {
    /// the mandatory pit window (0, 0: none)
    pub window: (u16, u16),
    /// what the menu asks for: the arrow keys pressed before the first step
    pub keys: &'static [u32],
    /// the first car starts damaged
    pub damage: bool,
    /// (car, seconds): its AI driver is called in (`requestPitStop`) from that time on
    pub calls: &'static [(usize, f32)],
}

/// The damage a `pit_box_repair` car starts with: the five zone levels, the engine's life
/// left, and the speed of the hit that bent the second suspension (`Suspension::setDamage`).
pub const PIT_DAMAGE: ([f32; 5], f64, f32) = ([40.0, 25.0, 0.0, 8.0, 40.0], 900.0, 75.0);
/// The step before which it is damaged (a session's start repairs every car).
pub const PIT_DAMAGE_STEP: usize = 400;

const UP: u32 = 0x26;
const DOWN: u32 = 0x28;
const LEFT: u32 = 0x25;
const RIGHT: u32 = 0x27;

pub fn pit_of(scenario: &str) -> Option<PitSpec> {
    // the first key only shows the menu; then down to the fuel line: 12 litres, the next
    // compound, two pressures up and one down
    const FUEL_TYRES: &[u32] = &[DOWN, DOWN, RIGHT, RIGHT, RIGHT, RIGHT, RIGHT, RIGHT, RIGHT, RIGHT, RIGHT, RIGHT, RIGHT, RIGHT, DOWN, RIGHT, RIGHT, DOWN, RIGHT, RIGHT, DOWN, RIGHT, DOWN, LEFT, LEFT, LEFT, LEFT, LEFT, LEFT];
    // the second preset: 5 litres; then the three repairs, from the menu's last lines
    const REPAIRS: &[u32] = &[DOWN, RIGHT, DOWN, RIGHT, RIGHT, RIGHT, RIGHT, RIGHT, DOWN, DOWN, DOWN, DOWN, DOWN, DOWN, DOWN, DOWN, DOWN, DOWN, DOWN, DOWN, RIGHT, UP, RIGHT, UP, RIGHT];
    const NOTHING: &[u32] = &[DOWN, DOWN];
    // 3 litres, and three clicks more of the first wing a stop may change (the menu's
    // seventh line on a car with four pressures)
    const WING: &[u32] = &[DOWN, DOWN, RIGHT, RIGHT, RIGHT, DOWN, DOWN, DOWN, DOWN, DOWN, DOWN, RIGHT, RIGHT, RIGHT];
    match scenario {
        "pit_box" | "pit_feed" | "pit_mouse" => Some(PitSpec { window: (0, 0), keys: FUEL_TYRES, damage: false, calls: &[] }),
        "pit_box_repair" => Some(PitSpec { window: (0, 0), keys: REPAIRS, damage: true, calls: &[] }),
        "pit_box_none" => Some(PitSpec { window: (0, 0), keys: NOTHING, damage: false, calls: &[] }),
        "pit_box_wing" => Some(PitSpec { window: (0, 0), keys: WING, damage: false, calls: &[] }),
        "pit_ai_race" => Some(PitSpec { window: (0, 0), keys: FUEL_TYRES, damage: false, calls: &[(0, 20.0), (1, 20.0)] }),
        "pit_window_served" => Some(PitSpec { window: (1, 3), keys: FUEL_TYRES, damage: false, calls: &[(0, 110.0)] }),
        "pit_window_missed" => Some(PitSpec { window: (1, 3), keys: FUEL_TYRES, damage: false, calls: &[] }),
        "pit_window_early" => Some(PitSpec { window: (2, 3), keys: FUEL_TYRES, damage: false, calls: &[(0, 20.0)] }),
        "pit_window_timed" => Some(PitSpec { window: (1, 3), keys: FUEL_TYRES, damage: false, calls: &[(0, 70.0)] }),
        _ => None,
    }
}

/// Task 28b: the first car in its pit box with the driver that shuffles out and back in,
/// AI drivers in the next boxes.
fn pit_boxes(track: &Track, cars: &[String], count: usize, leave_at: f32) -> Result<Vec<MultiCar>, String> {
    let count = if cars.is_empty() { count } else { cars.len() };
    let mut out = Vec::new();
    for i in 0..count {
        let (p, tail) = track.spawn_pose("PIT", i).ok_or_else(|| format!("the track has no pit box {i}"))?;
        let drive = if i == 0 { Drive::PitBox { leave_at } } else { Drive::Ai { push: 0.95 - 0.03 * i as f32, aggression: 0.1 * i as f32 } };
        out.push(MultiCar { name: name_of(cars, i, &[F2004]), position: [p.x, p.y, p.z], tail: [tail.x, tail.y, tail.z], drive });
    }
    Ok(out)
}

fn ai_grid(track: &Track, cars: &[String], default: &[&str], count: usize, push: fn(usize) -> f32, aggression: fn(usize) -> f32) -> Result<Vec<MultiCar>, String> {
    let mut out = grid(track, cars, default, count, 0.9, 0.0)?;
    for (index, car) in out.iter_mut().enumerate() {
        car.drive = Drive::Ai { push: push(index), aggression: aggression(index) };
    }
    Ok(out)
}

fn grid(track: &Track, cars: &[String], default: &[&str], count: usize, pace: f32, pace_step: f32) -> Result<Vec<MultiCar>, String> {
    let count = if cars.is_empty() { count } else { cars.len() };
    let spline = track.ai_spline.as_ref().ok_or("the track has no AI line")?;
    let mut out = Vec::new();
    for i in 0..count {
        let (p, tail) = track.spawn_pose("START", i).ok_or_else(|| format!("the track has no grid slot {i}"))?;
        // where the slot is beside the line: the car eases onto the line over the first seconds
        let n = spline.spline.world_to_spline(&[p.x, p.y, p.z], -1);
        let q = spline.spline.spline_to_world(n);
        let f = spline.payload_at_position(n).forward_vector;
        let l = (f[0] * f[0] + f[2] * f[2]).sqrt().max(1e-6);
        let side = (p.x - q[0]) * (f[2] / l) + (p.z - q[2]) * (-f[0] / l);
        out.push(MultiCar {
            name: name_of(cars, i, default),
            position: [p.x, p.y, p.z],
            tail: [tail.x, tail.y, tail.z],
            drive: Drive::Follow { lateral: vec![(0.0, side), (6.0 + i as f32, 0.0)], pace: pace - pace_step * i as f32, limit: 95.0, brake_at: f32::MAX },
        });
    }
    Ok(out)
}

// --- the scripted device of one car -----------------------------------------------------------
// +0x00 vtable, +0x10 gas, brake, steer, clutch, hand brake (f32), +0x24 buttons, +0x28
// requested gear, +0x2c how often it was asked
const DEV_GAS: usize = 0x10;
const DEV_BUTTONS: usize = 0x24;
const DEV_REQUESTED_GEAR: usize = 0x28;
const DEV_POLLED: usize = 0x2c;

extern "C" fn multi_acquire_controls(this: *mut u8, controls: *mut u8, _dt: f32, _input: *const f32) {
    unsafe {
        let pedals: [f32; 5] = rd(this, DEV_GAS);
        let buttons: u32 = rd(this, DEV_BUTTONS);
        wr(controls, CC_GEAR_UP, (buttons & 1 != 0) as u8);
        wr(controls, CC_GEAR_DN, (buttons & 2 != 0) as u8);
        wr(controls, CC_DRS, (buttons & 4 != 0) as u8);
        wr(controls, CC_KERS, (buttons & 8 != 0) as u8);
        wr(controls, CC_HAND_BRAKE, pedals[4]);
        wr(controls, CC_REQUESTED_GEAR_INDEX, rd::<i32>(this, DEV_REQUESTED_GEAR));
        wr(controls, CC_GAS, pedals[0]);
        wr(controls, CC_BRAKE, pedals[1]);
        wr(controls, CC_STEER, pedals[2]);
        wr(controls, CC_CLUTCH, pedals[3]);
        wr(this, DEV_POLLED, rd::<u32>(this, DEV_POLLED) + 1);
    }
}

extern "C" fn multi_get_action(_this: *mut u8, _action: i32) -> u8 {
    0
}

/// One car of the game's engine.
struct GameCar {
    car: *mut u8,
    device: *mut u8,
    /// ODE's `dxBody` of every body, in creation order.
    bodies: Vec<*mut u8>,
    suspensions: [*mut u8; 4],
    mesh_geom: *mut u8,
    drive: Drive,
    lateral: f32,
    pit_done: bool,
    /// Task 28b: where the pit-box driver is (see `Drive::PitBox`), and its box
    pit_phase: u8,
    pit_box: ([f32; 3], [f32; 3]),
    grip: Grip,
    /// Task 27: the game's `AIDriver` of the car (null: none)
    ai: *mut u8,
}

/// The game's world with several cars.
pub struct MultiWorld<'a> {
    acs: &'a Acs,
    pub engine: *mut u8,
    cars: Vec<GameCar>,
    track: std::sync::Arc<Track>,
    objects: Vec<*mut u8>,
    pub step_index: usize,
    pub recording: Recording,
    /// Task 24b: the engine's physics events of the steps since they were last taken (only
    /// kept while `taping`), for the tapes of the picture and the sound
    pub taping: bool,
    pub tape_events: Vec<[u8; 0x48]>,
    state_buffer: *mut u8,
    /// each car's folder name
    pub names: Vec<String>,
    /// Task 27: the step at which car 0 gets the game's `AIDriver`
    handover: Option<usize>,
    /// Task 28: the game's referee of the scenario
    pub referee: Option<super::race::WholeReferee<'a>>,
    /// Task 28: the first car jumps the start (the `rc_jump*` scenarios)
    jump_start: bool,
    shift_wait: u32,
    /// Task 28b: (car, step) at which its AI driver is called in
    pit_calls: Vec<(usize, usize)>,
    pit_damage: bool,
    /// Task 28c: the game's `Track` and its loose objects (`PhysicsObject`s), for the replays
    pub game_track: *mut u8,
    pub physics_objects: Vec<*mut u8>,
}

unsafe fn car_bodies(acs: &Acs, car: *mut u8) -> (Vec<*mut u8>, [*mut u8; 4]) {
    let ode = |wrapper: *mut u8| -> *mut u8 { rd(wrapper, RB_ODE_BODY) };
    let mut bodies = vec![ode(rd(car, CAR_BODY)), ode(rd(car, CAR_FUEL_TANK_BODY))];
    let axle: *mut u8 = rd(car, CAR_RIGID_AXLE);
    if !axle.is_null() {
        bodies.push(ode(axle));
    }
    let begin: *const *mut u8 = rd(car, CAR_SUSPENSIONS);
    let end: *const *mut u8 = rd(car, CAR_SUSPENSIONS + 8);
    assert_eq!(end.offset_from(begin), 4, "the car does not have four suspensions");
    let mut suspensions = [std::ptr::null_mut(); 4];
    for (i, slot) in suspensions.iter_mut().enumerate() {
        let suspension = *begin.add(i);
        let vtable = rd::<usize>(suspension, 0);
        let class = SUS_CLASSES.iter().find(|class| acs.va(class.vtable) == vtable).expect("one of the four suspension classes");
        if class.name == "ML" {
            // members the game never writes (see `find_bodies_and_joints`)
            for offset in [0x10, SUS_BUMP_STOP_UP, SUS_BUMP_STOP_DN, 0x2c] {
                wr(suspension, offset, 0.0f32);
            }
        }
        if let Some(hub) = class.hub {
            bodies.push(ode(rd(suspension, hub)));
        }
        if let Some(strut) = class.strut_body {
            bodies.push(ode(rd(suspension, strut)));
        }
        *slot = suspension;
    }
    (bodies, suspensions)
}

impl<'a> MultiWorld<'a> {
    /// The current directory must be the scratch root, prepared for every car and the track.
    #[allow(clippy::too_many_arguments)]
    pub fn build(
        acs: &'a Acs,
        scenario: &MultiScenario,
        car_names: &[String],
        track_folder: &Path,
        layout: &str,
        colliders: &dyn Fn(&str) -> Result<CarColliders, String>,
        seed: u32,
    ) -> Result<MultiWorld<'a>, String> {
        unsafe {
            let engine = new_engine(acs, seed);
            let _ = std::fs::remove_file("cfg/race.ini");
            // the game's own track with its real collision categories
            let built = crate::track::GameTrack::build(acs, engine, track_folder, layout, false)?;
            if !built.surface_mismatches.is_empty() {
                return Err(format!("the game's surfaces differ from the port's: {:?}", built.surface_mismatches));
            }
            let place = |name: &str| -> Option<V3> {
                let m = built.rust.helper_nodes.iter().find(|h| h.name == name)?.local.m;
                Some([m[3][0], m[3][1], m[3][2]])
            };
            let add_time_line: extern "C" fn(*mut u8, *const V3, *const V3, i32) = std::mem::transmute(acs.va(VA_TRACK_ADD_TIME_LINE));
            let mut lines = 0;
            while let (Some(left), Some(right)) = (place(&format!("AC_TIME_{lines}_L")), place(&format!("AC_TIME_{lines}_R"))) {
                add_time_line(built.track, &left, &right, 0);
                lines += 1;
            }
            // Task 27 (the drag strip): a point-to-point track's gates, as the one-car oracle adds them
            let ab = ["AC_AB_START_L", "AC_AB_START_R", "AC_AB_FINISH_L", "AC_AB_FINISH_R"].map(|name| place(name));
            if let [Some(sl), Some(sr), Some(fl), Some(fr)] = ab {
                add_time_line(built.track, &sl, &sr, 1);
                add_time_line(built.track, &fl, &fr, 2);
                lines += 2;
            }
            if let (Some(left), Some(right)) = (place("AC_OPEN_FINISH_L"), place("AC_OPEN_FINISH_R")) {
                add_time_line(built.track, &left, &right, 0);
                lines += 1;
            }
            assert_eq!(lines, built.rust.time_lines.len(), "the port found other timing lines");
            wr(engine, PE_PENALTY_MODE, 1i32);
            wr(engine, PE_ALLOWED_TYRES_OUT, 2i32);
            let game_objects = crate::track::create_objects(acs, engine, &built.rust);
            let objects: Vec<*mut u8> = game_objects.iter().map(|o| o.body()).collect();
            let physics_objects: Vec<*mut u8> = game_objects.iter().map(|o| o.object).collect();
            let game_track = built.track;
            let track = std::sync::Arc::new(built.rust.clone());
            let built: &'static crate::track::GameTrack = Box::leak(Box::new(built));
            let _ = built;

            let plan = (scenario.cars)(&track, car_names)?;
            let srand: extern "C" fn(u32) = std::mem::transmute(acs.crt_function(c"srand"));
            let force_rotation: extern "C" fn(*mut u8, *const V3) = std::mem::transmute(acs.va(VA_CAR_FORCE_ROTATION));
            let force_position: extern "C" fn(*mut u8, *const V3, u8) = std::mem::transmute(acs.va(VA_CAR_FORCE_POSITION));
            let set_damage_level: extern "C" fn(*mut u8, f32) = std::mem::transmute(acs.va(VA_CAR_SET_DAMAGE_LEVEL));
            let reset_suspension_damage: extern "C" fn(*mut u8) = std::mem::transmute(acs.va(VA_CAR_RESET_SUSPENSION_DAMAGE));
            let set_compound: extern "C" fn(*mut u8, i32) -> u8 = std::mem::transmute(acs.va(VA_TYRE_SET_COMPOUND));
            let init_mesh: extern "C" fn(*mut u8, *mut u8, *const [f32; 16]) = std::mem::transmute(acs.va(VA_CAR_INIT_COLLIDER_MESH));
            let ctor: extern "C" fn(*mut u8, *mut u8, *mut u8, *mut u8) -> *mut u8 = std::mem::transmute(acs.va(VA_CAR_CTOR));

            // one device class for all cars: the RTTI pointer of a real device in front so
            // that the game's dynamic_cast<AIDriver*> answers "not an AI"
            let keyboard = acs.va(VA_KEYBOARD_CONTROL_VTABLE) as *const usize;
            let table: Vec<usize> = vec![
                *keyboard.offset(-1),
                device_destructor as *const () as usize,
                multi_acquire_controls as *const () as usize,
                multi_get_action as *const () as usize,
                device_send_ff as *const () as usize,
                device_get_name as *const () as usize,
                device_get_ff_global_gain as *const () as usize,
                device_false as *const () as usize,
                device_on_auto_shifter_changed as *const () as usize,
                device_false as *const () as usize,
                device_set_vibrations as *const () as usize,
                device_set_engine_rpm as *const () as usize,
                device_false as *const () as usize,
            ];
            let table = leak(table).add(1);

            let mut cars = Vec::new();
            let mut metas = Vec::new();
            let mut geom_high = Vec::new();
            for (index, planned) in plan.iter().enumerate() {
                // each car's draws from rand() start from a known point
                let car_seed = seed.wrapping_add(1000 * (index as u32 + 1));
                srand(car_seed);
                let car = acs.alloc(CAR_SIZE);
                ctor(car, engine, wstring(acs, &planned.name), wstring(acs, ""));
                assert_eq!(rd::<u32>(car, CAR_PHYSICS_GUID), index as u32, "the car's physicsGUID is not its number");
                let (bodies, suspensions) = car_bodies(acs, car);

                // the collider mesh (CarAvatar::initPhysics: the first mesh of collider.kn5)
                let loaded = colliders(&planned.name)?;
                let real = loaded.mesh.as_ref().ok_or_else(|| format!("{}: no collider.kn5 in the game's folder", planned.name))?;
                let mesh = object(0x168);
                let mut vertices = vec![0f32; real.vertices.len() * 11];
                for (i, p) in real.vertices.iter().enumerate() {
                    vertices[i * 11..i * 11 + 3].copy_from_slice(p);
                }
                let indices = real.indices.clone();
                let mut matrix = [0f32; 16];
                for r in 0..4 {
                    for c in 0..4 {
                        matrix[4 * r + c] = real.matrix.m[r][c];
                    }
                }
                let vertex_count = vertices.len() / 11;
                let vp = leak(vertices).cast::<u8>();
                wr(mesh, 0x108, vp);
                wr(mesh, 0x110, vp.add(vertex_count * 0x2c));
                wr(mesh, 0x118, vp.add(vertex_count * 0x2c));
                let n = indices.len();
                let ip = leak(indices).cast::<u8>();
                wr(mesh, 0x120, ip);
                wr(mesh, 0x128, ip.add(n * 2));
                wr(mesh, 0x130, ip.add(n * 2));
                init_mesh(car, mesh, &matrix);
                let body: *const u8 = rd(car, CAR_BODY);
                let meshes: *const *const u8 = rd(body, 0x30);
                let mesh_geom: *mut u8 = rd(*meshes, 0x10);
                geom_high.push((mesh_geom as usize >> 32) as u32);

                // (KeyboardCarControl's whole size, 0x120: with its RTTI in front the game's
                // dynamic_cast<KeyboardCarControl*> succeeds, and PitStop::isInPitPosition
                // reads and writes mouseSteering at +0x71, which must be this block's zero)
                let device = object(0x120);
                wr(device, 0, table);
                wr(device, DEV_GAS, [0.0f32, 0.0, 0.0, 1.0, 0.0]);
                wr(device, DEV_REQUESTED_GEAR, -1i32);
                wr(car, CAR_CONTROLS_PROVIDER, device);
                // (Task 32: CAR_ORACLE_PERF_METER=1 leaves the game's PerformanceMeter on, so that
                // a tape of the session has the delta the app "Performance Delta" shows)
                if std::env::var("CAR_ORACLE_PERF_METER").map_or(true, |v| v != "1") {
                    wr(car, CAR_PERFORMANCE_METER_IS_ENABLED, 0u8);
                }
                wr(car, CAR_TELEMETRY_IS_ENABLED, 0u8);
                // the automatic clutch and gearbox (CarAvatar::setAutoClutchEnabled, the assist option)
                wr(car, CAR_AUTOCLUTCH + 0xc, 1u8);
                wr(car, CAR_AUTOCLUTCH + 0xd, 1u8);
                // (the car that jumps the start shifts by hand: the H-shifter's first gear)
                let auto_shifter = !((scenario.name.starts_with("rc_jump") || scenario.name.ends_with("_pass_rev") || matches!(planned.drive, Drive::PitBox { .. })) && index == 0);
                wr(car, CAR_AUTO_SHIFT, auto_shifter as u8);

                force_rotation(car, &planned.tail);
                force_position(car, &planned.position, 1);
                set_damage_level(car, 0.0);
                reset_suspension_damage(car);
                // the session start: the default compound, the setup screen's round trip
                use rustyac_physics::data::ini::IniReader;
                let data = Path::new("content/cars").join(&planned.name).join("data");
                let tyres = IniReader::load(&data.join("tyres.ini"))?;
                let compound = tyres.get_int("COMPOUND_DEFAULT", "INDEX").unwrap_or(0);
                for wheel in 0..4 {
                    set_compound(car.add(CAR_TYRES + wheel * TYRE_SIZE), compound);
                }
                if let Ok(setup) = IniReader::load(&data.join("setup.ini")) {
                    apply_setup_screen_defaults(car, &setup);
                }
                metas.push(CarMeta {
                    name: planned.name.clone(),
                    bodies: bodies.len(),
                    seed: car_seed,
                    position: planned.position,
                    tail: planned.tail,
                    auto_clutch: true,
                    auto_shifter,
                    collider_mesh: true,
                    ai: None,
                    pit: None,
                    handover: index == 0 && ai_handover(scenario.name).is_some(),
                    drag: None,
                });
                let lateral = match &planned.drive {
                    Drive::Follow { lateral, .. } => lateral.first().map_or(0.0, |l| l.1),
                    _ => 0.0,
                };
                cars.push(GameCar { car, device, bodies, suspensions, mesh_geom, drive: planned.drive.clone(), lateral, pit_done: false, pit_phase: 0, pit_box: (planned.position, [-planned.tail[0], -planned.tail[1], -planned.tail[2]]), grip: Grip::of(&planned.name), ai: std::ptr::null_mut() });
            }

            // Task 27: the AI drivers, after all cars, in creation order, from a known point of
            // rand(): `new AIDriver(car)` as the game's own code does it (its closure sets
            // basePush and aggression after the constructor and makes it the car's provider)
            let ai_seed = seed.wrapping_add(27_000);
            let mut recording_ai_seed = false;
            let with_ai = cars.iter().any(|car| matches!(car.drive, Drive::Ai { .. })) || ai_handover(scenario.name).is_some();
            if with_ai {
                // every car's pit box (CarAvatar gives the physics car its `AC_PIT_<n>`)
                for (index, car) in cars.iter().enumerate() {
                    if let Some(node) = track.spawn_positions.get("PIT").and_then(|slots| slots.get(index)) {
                        let m = track.helper_nodes[*node].local.m;
                        let mut flat = [0f32; 16];
                        for r in 0..4 {
                            flat[4 * r..4 * r + 4].copy_from_slice(&m[r]);
                        }
                        wr(car.car, CAR_PIT_POSITION, flat);
                        metas[index].pit = Some(flat);
                    }
                }
            }
            if with_ai {
                srand(ai_seed);
                let new: extern "C" fn(usize) -> *mut u8 = std::mem::transmute(acs.crt_function(c"??2@YAPEAX_K@Z"));
                let ai_ctor: extern "C" fn(*mut u8, *mut u8) -> *mut u8 = std::mem::transmute(acs.va(VA_AIDRIVER_CTOR));
                for (index, car) in cars.iter_mut().enumerate() {
                    let Drive::Ai { push, aggression } = car.drive else { continue };
                    let ai = new(1000);
                    std::ptr::write_bytes(ai, 0, 1000);
                    ai_ctor(ai, car.car);
                    wr(ai, AI_BASE_PUSH, push);
                    wr(ai, AI_AGGRESSION, aggression);
                    wr(car.car, CAR_CONTROLS_PROVIDER, ai);
                    car.ai = ai;
                    metas[index].ai = Some(rustyac_physics::engine::record::AiMeta { base_push: push, aggression, expected_fuel_per_lap: rd(car.car, CAR_EXPECTED_FUEL_PER_LAP) });
                }
            }
            // Task 27: the drag drivers (RaceManager::initOffline's drag branch): `new
            // AIDriverDrag(car)`, its push, the car's provider; then every car on its place
            // once more (the driver lays its line) and the start time (DragMode's call)
            if cars.iter().any(|car| matches!(car.drive, Drive::Drag { .. })) {
                let new: extern "C" fn(usize) -> *mut u8 = std::mem::transmute(acs.crt_function(c"??2@YAPEAX_K@Z"));
                let drag_ctor: extern "C" fn(*mut u8, *mut u8) -> *mut u8 = std::mem::transmute(acs.va(VA_AIDRIVERDRAG_CTOR));
                let set_start_time: extern "C" fn(*mut u8, f64) = std::mem::transmute(acs.va(VA_AIDRIVERDRAG_SET_START_TIME));
                let mut drivers = Vec::new();
                for (index, car) in cars.iter_mut().enumerate() {
                    let Drive::Drag { push, start_ms } = car.drive else {
                        drivers.push(std::ptr::null_mut());
                        continue;
                    };
                    let ai = new(0x118);
                    std::ptr::write_bytes(ai, 0, 0x118);
                    drag_ctor(ai, car.car);
                    wr(ai, DRAG_PUSH, push);
                    wr(car.car, CAR_CONTROLS_PROVIDER, ai);
                    car.ai = ai;
                    metas[index].drag = Some((push, start_ms));
                    drivers.push(ai);
                }
                for (index, planned) in plan.iter().enumerate() {
                    force_rotation(cars[index].car, &planned.tail);
                    force_position(cars[index].car, &planned.position, 1);
                }
                srand(ai_seed);
                for (index, car) in cars.iter().enumerate() {
                    if let Drive::Drag { start_ms, .. } = car.drive {
                        set_start_time(drivers[index], CLOCK_START_MS + start_ms as f64);
                    }
                }
                recording_ai_seed = true;
            }
            let session = ai_session(scenario.name);
            if let Some((kind, start_ms, time_secs, laps)) = session {
                // the game's new-session job: PhysicsEngine::setSessionInfo, then every handler
                // of evOnNewSessionPhysics in the order they were registered (the cars', the
                // track's, the drivers'); then every car is put on its place once more, which
                // tells its driver (Car::evOnForcedPositionCompleted)
                let mut info = [0u8; 32];
                info[0..4].copy_from_slice(&kind.to_le_bytes());
                info[8..16].copy_from_slice(&(CLOCK_START_MS + start_ms).to_le_bytes());
                info[16..24].copy_from_slice(&time_secs.to_le_bytes());
                info[24..28].copy_from_slice(&laps.to_le_bytes());
                let set_session_info: extern "C" fn(*mut u8, *const u8) = std::mem::transmute(acs.va(VA_SET_SESSION_INFO));
                set_session_info(engine, info.as_ptr());
                let begin: *const u8 = rd(engine, PE_EV_ON_NEW_SESSION_PHYSICS);
                let end: *const u8 = rd(engine, PE_EV_ON_NEW_SESSION_PHYSICS + 8);
                let mut handler = begin;
                while handler < end {
                    let function: *mut u8 = rd(handler, 0x20);
                    if !function.is_null() {
                        let vtable: *const usize = rd(function, 0);
                        let do_call: extern "C" fn(*mut u8, *const u8) = std::mem::transmute(*vtable.add(2));
                        do_call(function, info.as_ptr());
                    }
                    handler = handler.add(0x28);
                }
                for (index, planned) in plan.iter().enumerate() {
                    force_rotation(cars[index].car, &planned.tail);
                    force_position(cars[index].car, &planned.position, 1);
                }
            }
            let core: *mut u8 = rd(engine, PE_CORE);
            wr(core, 0x60, scenario.no_collision_steps);

            let mut recording = Recording::default();
            recording.set("scenario", scenario.name);
            // Task 33: with the game's PerformanceMeter on, its outputs are compared with the
            // port's at every step
            if std::env::var("CAR_ORACLE_PERF_METER").is_ok_and(|v| v == "1") {
                recording.set("meter", "1");
            }
            // Task 43c: a penalty mode of the tester's choice where the game picks one
            if let Some(mode) = super::race::forced_penalty_mode() {
                recording.set("penalty_force", mode.to_string());
            }
            // Task 43c: the game's shared memory graphics page at every step
            // Task 43c: the Telemetry app's channels that read the player's car through its functions
            if std::env::var("CAR_ORACLE_TELEMETRY").is_ok_and(|v| v == "1") {
                recording.set("telem", "1");
            }
            if std::env::var("CAR_ORACLE_SHM").is_ok_and(|v| v == "1") {
                recording.set("shm", "1");
            }
            recording.set("about", scenario.about);
            recording.set("track", track.name.clone());
            recording.set("track_folder", track_folder.display().to_string());
            recording.set("track_layout", layout);
            recording.set("track_objects", "1");
            recording.set("track_object_count", objects.len().to_string());
            recording.set("seed", seed.to_string());
            recording.set("clock_start_ms", format!("{CLOCK_START_MS}"));
            recording.set("ambient", format!("{AMBIENT_TEMPERATURE}"));
            recording.set("road", format!("{ROAD_TEMPERATURE}"));
            recording.set("penalty_mode", "1");
            recording.set("allowed_tyres_out", "2");
            recording.set("no_collision_steps", scenario.no_collision_steps.to_string());
            // what the game's mesh contact joints get as bounce_vel when nothing better is
            // known: the upper half of the address of a car's mesh geom (all the same)
            recording.set("mesh_geom_high", geom_high.iter().map(|h| format!("{h:08x}")).collect::<Vec<_>>().join(","));
            recording.set("mesh_bounce_vel", format!("{:08x}", geom_high[0]));
            recording.set_cars(&metas);
            if recording_ai_seed {
                recording.set("ai_seed", ai_seed.to_string());
            }
            if with_ai {
                recording.set("ai_seed", ai_seed.to_string());
                if let Some(step) = ai_handover(scenario.name) {
                    recording.set("ai_handover", step.to_string());
                    recording.set("ai_handover_expected_fuel", format!("{:08x}", rd::<u32>(cars[0].car, CAR_EXPECTED_FUEL_PER_LAP)));
                }
                if let Some((kind, start_ms, time_secs, laps)) = session {
                    recording.set("ai_session", format!("{kind},{start_ms},{time_secs},{laps}"));
                }
            }
            // Task 28: the referee, made once every car and driver is there
            let mut referee = None;
            if let Some(spec) = referee_of(scenario.name) {
                let count = cars.len();
                let sessions = rustyac_physics::race::SessionsIni::from_list(spec.list, count, spec.penalties, spec.jump_start, 3)?;
                let referee_seed = seed.wrapping_add(28_000);
                let setup = rustyac_physics::race::RefereeSetup {
                    over_time_mult: 1.2,
                    start_index: (0..count as i32).collect(),
                    pit_index: (0..count as i32).collect(),
                    players: metas.iter().enumerate().map(|(n, meta)| rustyac_physics::race::output::Player { name: format!("Driver {n}"), car: meta.name.clone(), skin: format!("skin_{n}") }).collect(),
                    seed: referee_seed,
                    first_start_delay_ms: spec.first_delay_ms,
                    next_start_delay_ms: spec.next_delay_ms,
                    yellow_flag_distance: None,
                    layout: layout.to_string(),
                    sessions,
                };
                // RaceManager::initOffline: how long before the start the gearbox is locked
                // (ksRand() x 1000 + 5000 in the game)
                wr(engine, 0xc0, 5500.0f64);
                recording.set("lock_gearbox_ms", "5500");
                recording.set("allowed_tyres_out", setup.sessions.allowed_tyres_out().to_string());
                if let Some(mode) = setup.sessions.jump_start_penalty_mode {
                    recording.set("jump_start_penalty_mode", mode.to_string());
                }
                recording.set(
                    "referee",
                    format!("{};{};{};{:08x};{:08x};{:08x};{referee_seed}", spec.list, spec.penalties as u8, spec.jump_start.map_or("-".to_string(), |mode| mode.to_string()), setup.over_time_mult.to_bits(), spec.first_delay_ms.to_bits(), spec.next_delay_ms.to_bits()),
                );
                let car_pointers: Vec<*mut u8> = cars.iter().map(|car| car.car).collect();
                // CarAvatar::getCurrentCompoundShortName: the short name of the compound the car is on
                let mut compounds = Vec::new();
                for meta in &metas {
                    use rustyac_physics::data::ini::IniReader;
                    let tyres = IniReader::load(&Path::new("content/cars").join(&meta.name).join("data").join("tyres.ini"))?;
                    let index = tyres.get_int("COMPOUND_DEFAULT", "INDEX").unwrap_or(0);
                    compounds.push(tyres.get_string(&if index == 0 { "FRONT".to_string() } else { format!("FRONT_{index}") }, "SHORT_NAME"));
                }
                let mut whole = super::race::WholeReferee::build(acs, engine, &car_pointers, &track, &setup, &compounds, CLOCK_START_MS);
                // Task 28b: the player's pit stop, the menu it reads and the mandatory window
                if let Some(pit) = pit_of(scenario.name) {
                    use rustyac_physics::race::pitstop::{QuickMenu, QuickMenuSettings};
                    let car = cars[0].car;
                    let (menu_car, names) = super::pit::menu_car(car);
                    let setup_ini = rustyac_physics::data::ini::IniReader::load(&Path::new("content/cars").join(&metas[0].name).join("data").join("setup.ini"))?;
                    let mut menu = QuickMenu::build(&menu_car, names, &setup_ini, &QuickMenuSettings::default());
                    for key in pit.keys {
                        menu.send_input(*key, &menu_car);
                    }
                    println!("the pit menu:");
                    for line in menu.lines() {
                        println!("    {line}");
                    }
                    recording.set("pit", format!("{};{};{}", pit.window.0, pit.window.1, menu.values_text()));
                    if pit.damage {
                        let (zones, life, hit) = PIT_DAMAGE;
                        recording.set("pit_damage", format!("{PIT_DAMAGE_STEP};{};{:016x};{:08x}", zones.iter().map(|z| format!("{:08x}", z.to_bits())).collect::<Vec<_>>().join(","), life.to_bits(), hit.to_bits()));
                    }
                    if !pit.calls.is_empty() {
                        recording.set("ai_pit_calls", pit.calls.iter().map(|(car, seconds)| format!("{car}:{}", (*seconds / DT) as usize)).collect::<Vec<_>>().join(","));
                    }
                    let place: [f32; 3] = rd(car, CAR_PIT_POSITION + 0x30);
                    whole.enable_pit(place, &menu, pit.window);
                    if scenario.name == "pit_feed" {
                        recording.set("pit_feed", seed.to_string());
                        if let Some(game_pit) = &mut whole.pit {
                            game_pit.feed = Some(std::cell::RefCell::new(rustyac_physics::race::pitstop::PitFeed::new(seed)));
                        }
                    }
                    if scenario.name == "pit_mouse" {
                        recording.set("pit_mouse", seed.to_string());
                        if let Some(game_pit) = &mut whole.pit {
                            let mut feed = rustyac_physics::race::pitstop::PitFeed::new(seed);
                            feed.mouse = true;
                            game_pit.feed = Some(std::cell::RefCell::new(feed));
                            game_pit.enable_mouse();
                        }
                    }
                }
                // Task 43c: the game's graphics page after every frame
                if recording.get("shm").is_some() {
                    let shm = super::race::GameShm::build(acs, &whole, &track.data_folder.join("ai").join("fast_lane.ai"));
                    recording.set("shm_flags", shm.flag_values().iter().map(|v| v.to_string()).collect::<Vec<_>>().join(" "));
                    recording.set("shm_static", shm.static_page().iter().map(|b| format!("{b:02x}")).collect::<String>());
                    whole.shm = Some(shm);
                }
                referee = Some(whole);
            }
            Ok(MultiWorld { game_track, physics_objects, acs, engine, cars, track, objects, step_index: 0, recording, taping: false, tape_events: Vec::new(), state_buffer: acs.alloc(0xb70), names: metas.iter().map(|m| m.name.clone()).collect(), handover: ai_handover(scenario.name), referee, jump_start: scenario.name.starts_with("rc_jump"), shift_wait: 0, pit_calls: pit_of(scenario.name).map(|pit| pit.calls.iter().map(|(car, seconds)| (*car, (*seconds / DT) as usize)).collect()).unwrap_or_default(), pit_damage: pit_of(scenario.name).is_some_and(|pit| pit.damage) })
        }
    }

    unsafe fn view(&self, car: &GameCar) -> CarView {
        let body = car.bodies[0];
        let r: [f32; 12] = rd(body, B_R);
        let lock: f32 = rd(car.car, CAR_STEER_LOCK);
        let steer_ratio: f32 = rd(car.car, CAR_STEER_RATIO);
        CarView {
            position: v3(body, B_POS),
            forward: [r[2], r[6], r[10]],
            left: [r[0], r[4], r[8]],
            npos: if self.step_index == 0 { -1.0 } else { rd(car.car, CAR_SPLINE_LOCATOR_DATA) },
            offset: rd(car.car, CAR_SPLINE_LOCATOR + 0x24),
            max_wheel_angle: if steer_ratio != 0.0 { (lock / steer_ratio).to_radians() } else { 0.3 },
            step: self.step_index,
            speed: rd(car.car, CAR_VALUE_CACHE_SPEED),
            road_rpm: 0.0,
            gear: rd(car.car, CAR_DRIVETRAIN + 0x584),
            yaw_rate: 0.0,
            laps: rd(car.car.add(CAR_TRANSPONDER), 0x0c),
            damaged: rd::<[f32; 5]>(car.car, CAR_DAMAGE_ZONE_LEVEL)[4] > 0.0,
        }
    }

    /// The controls of every car for the next step.
    unsafe fn drive(&mut self) {
        let t = self.step_index as f32 * DT;
        for index in 0..self.cars.len() {
            let view = self.view(&self.cars[index]);
            let mut c = Controls { clutch: 1.0, requested_gear: -1, ..Controls::default() };
            let track = std::sync::Arc::clone(&self.track);
            let car = &mut self.cars[index];
            match &car.drive {
                // (the game's driver is the car's provider: this device is never asked)
                Drive::Ai { .. } | Drive::Drag { .. } => continue,
                Drive::Parked => c.brake = 1.0,
                Drive::Script(segments) => {
                    if let Some(&(_, gas, brake, steer)) = segments.iter().rev().find(|segment| t >= segment.0) {
                        c.gas = gas;
                        c.brake = brake;
                        c.steer = steer;
                    }
                }
                Drive::Follow { lateral, pace, limit, brake_at } => {
                    let target = lateral.iter().rev().find(|l| t >= l.0).map_or(0.0, |l| l.1);
                    car.lateral += (target - car.lateral) * (DT / 0.35).min(1.0);
                    crate::track_driver::follow_line(&track, &view, car.lateral, *pace, *limit, &car.grip, t, &mut c);
                    if t >= *brake_at {
                        c.gas = 0.0;
                        c.brake = 1.0;
                    }
                }
                Drive::FollowZones { zones, pace, limit, after } => {
                    let target = if t < *after { 0.0 } else { zones.iter().find(|zone| view.npos >= zone.0 && view.npos < zone.1).map_or(0.0, |zone| zone.2) };
                    car.lateral += (target - car.lateral) * (DT / 0.35).min(1.0);
                    crate::track_driver::follow_line(&track, &view, car.lateral, *pace, *limit, &car.grip, t, &mut c);
                }
                Drive::PitBox { leave_at } => {
                    // metres ahead of the box, along the way the box points
                    let (place, ahead) = car.pit_box;
                    let along = (view.position[0] - place[0]) * ahead[0] + (view.position[1] - place[1]) * ahead[1] + (view.position[2] - place[2]) * ahead[2];
                    let mut want_gear = view.gear;
                    match car.pit_phase {
                        // out of the box: first gear, gently, until 6 m ahead of it
                        0 => {
                            c.brake = 1.0;
                            if t >= 1.0 {
                                car.pit_phase = 1;
                            }
                        }
                        1 => {
                            want_gear = 2;
                            if view.gear == 2 {
                                c.gas = if view.speed < 3.0 { 0.35 } else { 0.0 };
                            } else {
                                c.brake = 1.0;
                            }
                            if along > 6.0 {
                                car.pit_phase = 2;
                            }
                        }
                        2 => {
                            c.brake = 1.0;
                            if view.speed < 0.05 {
                                car.pit_phase = 3;
                            }
                        }
                        // back into it in reverse
                        3 => {
                            want_gear = 0;
                            if view.gear == 0 {
                                c.gas = if view.speed < 2.0 { 0.3 } else { 0.0 };
                            } else {
                                c.brake = 1.0;
                            }
                            if along < 1.6 {
                                car.pit_phase = 4;
                            }
                        }
                        // the stop
                        4 => {
                            c.brake = 1.0;
                            if t >= *leave_at {
                                car.pit_phase = 5;
                            }
                        }
                        // and away: along the pit lane's line, then the track's
                        _ => {
                            if car.pit_done || !crate::track_driver::follow_pit_lane(&track, &view, &mut c) {
                                car.pit_done = true;
                                crate::track_driver::follow_line(&track, &view, 0.0, 0.7, 45.0, &car.grip, t, &mut c);
                            } else {
                                c.gas = if view.speed < 12.0 { 0.5 } else { 0.0 };
                            }
                            const UP: [f32; 4] = [12.0, 20.0, 29.0, 38.0];
                            const DOWN: [f32; 4] = [8.0, 15.0, 23.0, 31.0];
                            let slot = (view.gear - 2).clamp(0, 3) as usize;
                            if view.gear <= 1 || (view.gear <= 5 && view.speed > UP[slot]) {
                                want_gear = view.gear + 1;
                            } else if view.gear > 2 && view.speed < DOWN[(view.gear - 3).clamp(0, 3) as usize] {
                                want_gear = view.gear - 1;
                            }
                        }
                    }
                    if self.shift_wait > 0 {
                        self.shift_wait -= 1;
                    } else if want_gear > view.gear {
                        c.gear_up = true;
                        self.shift_wait = 150;
                    } else if want_gear < view.gear {
                        c.gear_dn = true;
                        self.shift_wait = 150;
                    }
                }
                Drive::PitLane { from, then_gas, brake_at } => {
                    if t < *from {
                        c.brake = 1.0;
                    } else if t >= *brake_at {
                        c.brake = 1.0;
                    } else if car.pit_done || !crate::track_driver::follow_pit_lane(&track, &view, &mut c) {
                        car.pit_done = true;
                        c.gas = *then_gas;
                    }
                }
            }
            // Task 28, the jump start: the first car has first gear in (the H-shifter's slot)
            // from 1 s on and drives off; the window before the start finds it rolling
            if self.jump_start && index == 0 {
                // (it shifts by hand, by its speed: the automatic gearbox would wait for the green light)
                const UP: [f32; 4] = [12.0, 20.0, 29.0, 38.0];
                const DOWN: [f32; 4] = [8.0, 15.0, 23.0, 31.0];
                if (8.1..11.0).contains(&t) {
                    c.gas = 0.5;
                    c.brake = 0.0;
                }
                let gear = view.gear;
                if self.shift_wait > 0 {
                    self.shift_wait -= 1;
                } else if t >= 8.0 {
                    let slot = (gear - 2).clamp(0, 3) as usize;
                    if gear <= 1 || (gear <= 5 && view.speed > UP[slot]) {
                        c.gear_up = true;
                        self.shift_wait = 150;
                    } else if gear > 2 && view.speed < DOWN[(gear - 3).clamp(0, 3) as usize] {
                        c.gear_dn = true;
                        self.shift_wait = 150;
                    }
                }
            }
            let buttons = c.gear_up as u32 | (c.gear_dn as u32) << 1 | (c.drs as u32) << 2 | (c.kers as u32) << 3;
            wr(car.device, DEV_GAS, [c.gas, c.brake, c.steer, c.clutch, c.hand_brake]);
            wr(car.device, DEV_BUTTONS, buttons);
            wr(car.device, DEV_REQUESTED_GEAR, c.requested_gear);
        }
    }

    unsafe fn contacts(&self) -> Vec<ContactNote> {
        let core: *const u8 = rd(self.engine, PE_CORE);
        let world: *const u8 = rd(core, CORE_WORLD);
        let vtable = self.acs.va(0x1_4051_2d50); // dxJointContact
        let car_of = |body: *mut u8| -> i32 {
            if body.is_null() {
                -1
            } else {
                self.cars.iter().position(|car| car.bodies.contains(&body)).map_or(-2, |n| n as i32)
            }
        };
        let mut out = Vec::new();
        // the world's joint list: the newest joint first
        let mut joint: *const u8 = rd(world, 0x28);
        while !joint.is_null() {
            if rd::<usize>(joint, 0) == vtable {
                out.push(ContactNote {
                    pos: rd(joint, 0xd0),
                    normal: rd(joint, 0xe0),
                    depth: rd(joint, 0xf0),
                    car1: car_of(rd(joint, 0x40)),
                    car2: car_of(rd(joint, 0x58)),
                    reversed: rd::<u32>(joint, 0x30) & 2 != 0,
                    mode: rd(joint, 0x90),
                    mu: rd(joint, 0x94),
                    bounce: rd(joint, 0xa8),
                    bounce_vel: rd(joint, 0xac),
                    soft_cfm: rd(joint, 0xb4),
                });
            }
            joint = rd(joint, 0x10);
        }
        out
    }

    /// The game's `Car` object of car `index`.
    /// The physics time of the last step, ms.
    pub fn time_ms(&self) -> f64 {
        CLOCK_START_MS + self.step_index as f64 * 3.0
    }

    /// The port's copy of the track.
    pub fn port_track(&self) -> std::sync::Arc<Track> {
        std::sync::Arc::clone(&self.track)
    }

    pub fn car_ptr(&self, index: usize) -> *mut u8 {
        self.cars[index].car
    }

    /// One step of the game's engine; the row goes into the recording.
    pub fn step(&mut self) {
        unsafe {
            if self.handover == Some(self.step_index) {
                // the game's "AI drives my car" job (a closure in `Sim`, run from the physics
                // thread's queue before the step): `new AIDriver(car)`, made the car's provider
                let new: extern "C" fn(usize) -> *mut u8 = std::mem::transmute(self.acs.crt_function(c"??2@YAPEAX_K@Z"));
                let ai_ctor: extern "C" fn(*mut u8, *mut u8) -> *mut u8 = std::mem::transmute(self.acs.va(VA_AIDRIVER_CTOR));
                let ai = new(1000);
                std::ptr::write_bytes(ai, 0, 1000);
                ai_ctor(ai, self.cars[0].car);
                wr(self.cars[0].car, CAR_CONTROLS_PROVIDER, ai);
                self.cars[0].ai = ai;
                self.cars[0].drive = Drive::Ai { push: 1.0, aggression: 0.0 };
            }
            // Task 28b: the damage a repair stop starts from
            if self.pit_damage && self.step_index == PIT_DAMAGE_STEP {
                let (zones, life, hit) = PIT_DAMAGE;
                let car = self.cars[0].car;
                wr(car, CAR_DAMAGE_ZONE_LEVEL, zones);
                wr(car, 0x710, life);
                let suspension = self.cars[0].suspensions[1];
                let vtable: *const usize = rd(suspension, 0);
                let set_damage: extern "C" fn(*mut u8, f32) = std::mem::transmute(*vtable.add(0x78 / 8));
                set_damage(suspension, hit);
            }
            // Task 28b: a driver is called in (`AIDriver::requestPitStop`)
            for &(car, step) in &self.pit_calls {
                if step == self.step_index && !self.cars[car].ai.is_null() {
                    wr(self.cars[car].ai, 0x7c, 1u8);
                }
            }
            self.drive();
            for car in &self.cars {
                wr(car.device, DEV_POLLED, 0u32);
            }
            let time_ms = CLOCK_START_MS + (self.step_index + 1) as f64 * 3.0;
            // Task 28: what the referee queued for the physics thread runs first, with the
            // clock at this step's time
            if let Some(referee) = &mut self.referee {
                wr(self.engine, PE_PHYSICS_TIME, time_ms);
                referee.before_step();
            }
            let step: extern "C" fn(*mut u8, f32, f64, f64) = std::mem::transmute(self.acs.va(VA_PHYSICS_ENGINE_STEP));
            step(self.engine, DT, time_ms, time_ms);
            // (and the referee's frame, every third step)
            if let Some(referee) = &mut self.referee {
                referee.after_step(time_ms);
            }
            // the sounds' queue is not read by anybody here: emptied as the game's main thread does
            let pop: unsafe extern "system" fn(*mut u8, *mut u8) -> bool =
                std::mem::transmute(self.acs.msvcp_function(c"?_Internal_pop_if_present@_Concurrent_queue_base_v4@details@Concurrency@@IEAA_NPEAX@Z"));
            let mut raw = [0u8; 0x48];
            while pop(self.engine.add(0x30), raw.as_mut_ptr()) {
                if self.taping {
                    self.tape_events.push(raw);
                }
            }

            let core: *const u8 = rd(self.engine, PE_CORE);
            let mut row: Vec<u32> = Vec::with_capacity(self.recording.row_words());
            row.extend(contact_words(&self.contacts()));
            row.push(rd::<i32>(core, 0x60) as u32);
            row.push(rd::<u32>(core, 0xa0));
            for car in &self.cars {
                if car.ai.is_null() && self.handover.is_some() && std::ptr::eq(car, &self.cars[0]) {
                    // (before the hand-over: the device's inputs, and no driver yet)
                    let pedals: [u32; 5] = rd(car.device, DEV_GAS);
                    row.extend(pedals);
                    row.push(rd(car.device, DEV_BUTTONS));
                    row.push(rd(car.device, DEV_REQUESTED_GEAR));
                } else if car.ai.is_null() {
                    let pedals: [u32; 5] = rd(car.device, DEV_GAS);
                    row.extend(pedals);
                    row.push(rd(car.device, DEV_BUTTONS));
                    row.push(rd(car.device, DEV_REQUESTED_GEAR));
                    // (a car the referee has locked or black-flagged is not asked)
                    let polled = rd::<u32>(car.device, DEV_POLLED);
                    assert!(polled == 1 || (polled == 0 && self.referee.is_some()), "a car's device was not asked exactly once");
                } else {
                    // an AI driver's inputs: what it left in the car's controls
                    let c = car.car.add(CAR_CONTROLS);
                    row.extend([rd::<u32>(c, CC_GAS), rd(c, CC_BRAKE), rd(c, CC_STEER), rd(c, CC_CLUTCH), rd(c, CC_HAND_BRAKE)]);
                    row.push(rd::<u8>(c, CC_GEAR_UP) as u32 | (rd::<u8>(c, CC_GEAR_DN) as u32) << 1 | (rd::<u8>(c, CC_DRS) as u32) << 2 | (rd::<u8>(c, CC_KERS) as u32) << 3);
                    row.push(rd(c, CC_REQUESTED_GEAR_INDEX));
                }
                for &body in &car.bodies {
                    row.extend(rd::<[u32; 3]>(body, B_POS));
                    row.extend(rd::<[u32; 4]>(body, B_Q));
                    row.extend(rd::<[u32; 3]>(body, B_LVEL));
                    row.extend(rd::<[u32; 3]>(body, B_AVEL));
                }
                row.extend(rd::<[u32; 5]>(car.car, CAR_DAMAGE_ZONE_LEVEL));
                for &suspension in &car.suspensions {
                    // ISuspension::getDamage (slot +0x88)
                    let vtable: *const usize = rd(suspension, 0);
                    let get_damage: extern "C" fn(*mut u8) -> f32 = std::mem::transmute(*vtable.add(17));
                    row.push(get_damage(suspension).to_bits());
                }
                for wheel in 0..4 {
                    let status = car.car.add(CAR_TYRES + wheel * TYRE_SIZE + T_STATUS);
                    for offset in [S_ANGULAR_VELOCITY, 0x4, 0x2c, 0x10] {
                        row.push(rd(status, offset));
                    }
                }
                let d = car.car.add(CAR_DRIVETRAIN);
                row.extend(rd::<[u32; 2]>(d, 0x8));
                row.push(rd(d, 0x584));
                row.extend(rd::<[u32; 2]>(car.car, CAR_FUEL));
                row.push(rd(car.car, CAR_VALUE_CACHE_SPEED));
                row.push(rd(car.mesh_geom, 0x7c));
                row.push(rd::<u8>(car.car, 0x3e68) as u32);
                row.extend(rd::<[u32; 2]>(car.car, 0x3508));
                row.extend(rd::<[u32; 2]>(car.car, 0x3c30));
                row.push(rd(car.car.add(CAR_TRANSPONDER), 0x0c));
                row.push(rd(car.car, CAR_SPLINE_LOCATOR_DATA));
                if car.ai.is_null() && self.handover.is_some() && std::ptr::eq(car, &self.cars[0]) {
                    // the columns of the driver to come
                    row.extend(std::iter::repeat_n(0u32, rustyac_physics::ai::oracle::column_names().len()));
                }
                if !car.ai.is_null() && !matches!(car.drive, Drive::Drag { .. }) {
                    use rustyac_physics::ai::oracle::{Kind, COLUMNS};
                    for &(_, offset, kind) in COLUMNS {
                        match kind {
                            Kind::F32 | Kind::I32 => row.push(rd(car.ai, offset)),
                            Kind::F64 => row.extend(rd::<[u32; 2]>(car.ai, offset)),
                            Kind::U8 => row.push(rd::<u8>(car.ai, offset) as u32),
                        }
                    }
                }
            }
            if let Some(referee) = &self.referee {
                row.extend([referee.state_hash as u32, (referee.state_hash >> 32) as u32]);
            }
            if self.recording.get("meter").is_some() {
                // Task 33: PerformanceMeter::currentDiff (+0x60), currentSpeedDiffMS (+0x68)
                let meter = self.cars[0].car.add(CAR_PERFORMANCE_METER_IS_ENABLED);
                row.extend(rd::<[u32; 2]>(meter, 0x60));
                row.push(rd(meter, 0x68));
            }
            if self.recording.get("telem").is_some() {
                // Task 43c: FormTelemetry::initChannels' lambdas on the player's car
                let (car, ai) = (self.cars[0].car, self.cars[0].ai);
                let driving = !ai.is_null() && rd::<*mut u8>(car, CAR_CONTROLS_PROVIDER) == ai;
                let lateral_push: extern "C" fn(*mut u8) -> f32 = std::mem::transmute(self.acs.va(VA_AIDRIVER_GET_LATERAL_PUSH));
                let oversteer: extern "C" fn(*mut u8) -> f32 = std::mem::transmute(self.acs.va(VA_GET_OVERSTEER_FACTOR));
                let steer: extern "C" fn(*mut u8, i32) -> f32 = std::mem::transmute(self.acs.va(VA_GET_WHEEL_STEER_ANGLE_DEG));
                // (ERS::getOutputTorque through the ERS's ITorqueGenerator table, as the lambda calls it)
                let ers = car.add(CAR_ERS);
                let ers_torque: extern "C" fn(*mut u8) -> f32 = std::mem::transmute(*rd::<*const usize>(ers, 0).add(1));
                row.push(if driving { lateral_push(ai) * 100.0 } else { 0.0 }.to_bits());
                row.push(if driving { rd(ai, AI_LIVE_OFFSET) } else { 0 });
                row.push(if driving { rd(ai, AI_OFFSET_DIST_FROM_CORNER) } else { 0 });
                row.push(ers_torque(ers).to_bits());
                row.push(oversteer(car).to_bits());
                for wheel in 0..4 {
                    row.push(steer(car, wheel).to_bits());
                }
                let (state, _) = self.physics_state(0);
                let word = |at: usize| u32::from_le_bytes([state[at], state[at + 1], state[at + 2], state[at + 3]]);
                row.extend([word(0x66c), word(0x79c), word(0x7a0)]);
                // the Suspensions app: RaceEngineer::getKPI_RAD(0) and getFrontCasterRAD through an
                // engineer of this car, then every suspension's getDebugLines on the state's
                // worldMatrix (+4) and suspensionMatrix[w] (+0x44)
                // (a RaceEngineer is 0x18 bytes: its car at +8)
                let mut engineer = [0usize, car as usize, 0];
                let engineer = engineer.as_mut_ptr() as *mut u8;
                let kpi: extern "C" fn(*mut u8, *mut [f32; 2], i32) -> *mut [f32; 2] = std::mem::transmute(self.acs.va(VA_RACE_ENGINEER_GET_KPI_RAD));
                let caster: extern "C" fn(*mut u8) -> f32 = std::mem::transmute(self.acs.va(VA_RACE_ENGINEER_GET_FRONT_CASTER_RAD));
                let mut k = [0f32; 2];
                kpi(engineer, &mut k, 0);
                row.extend([k[0].to_bits(), k[1].to_bits(), caster(engineer).to_bits()]);
                let free: extern "C" fn(*mut u8) = std::mem::transmute(self.acs.crt_function(c"free"));
                let suspensions: *const *mut u8 = rd(car, CAR_SUSPENSIONS);
                for w in 0..4 {
                    let s = *suspensions.add(w);
                    let lines: extern "C" fn(*mut u8, *mut [usize; 3], *const u8, *const u8, i32) -> *mut [usize; 3] = std::mem::transmute(*rd::<*const usize>(s, 0).add(0x70 / 8));
                    let mut v = [0usize; 3];
                    lines(s, &mut v, state.as_ptr().add(4), state.as_ptr().add(0x44 + 0x40 * w), 0);
                    let count = (v[1] - v[0]) / 0x2c;
                    row.push(count as u32);
                    let mut words = [0u32; rustyac_physics::engine::record::DEBUG_LINES * 10];
                    for k in 0..count.min(rustyac_physics::engine::record::DEBUG_LINES) {
                        for j in 0..10 {
                            words[k * 10 + j] = rd((v[0] + k * 0x2c) as *mut u8, j * 4);
                        }
                    }
                    row.extend(words);
                    if v[0] != 0 {
                        free(v[0] as *mut u8);
                    }
                }
            }
            if let Some(shm) = self.referee.as_ref().and_then(|referee| referee.shm.as_ref()) {
                row.extend(shm.words());
            }
            assert_eq!(row.len(), self.recording.row_words(), "a row of the wrong length");
            self.recording.rows.push(row);
            self.step_index += 1;
        }
    }

    /// Task 24b: the `CarPhysicsState` the game's own `Car::getPhysicsState` fills for a car
    /// (what the main thread's `CarAvatar` reads), and its `tractionControl.isInAction`.
    pub fn physics_state(&self, index: usize) -> (Vec<u8>, u8) {
        unsafe {
            std::ptr::write_bytes(self.state_buffer, 0, 0xb70);
            let get_physics_state: extern "C" fn(*mut u8, *mut u8) = std::mem::transmute(self.acs.va(VA_CAR_GET_PHYSICS_STATE));
            get_physics_state(self.cars[index].car, self.state_buffer);
            (std::slice::from_raw_parts(self.state_buffer, 0xb70).to_vec(), rd::<u8>(self.cars[index].car, CAR_TRACTION_CONTROL + 8))
        }
    }


    /// A line about each car for the console.
    pub fn summary(&self) -> Vec<String> {
        unsafe {
            self.cars
                .iter()
                .enumerate()
                .map(|(n, car)| {
                    let p = v3(car.bodies[0], B_POS);
                    let damage: [f32; 5] = rd(car.car, CAR_DAMAGE_ZONE_LEVEL);
                    format!(
                        "car {n}: at ({:.1}, {:.1}, {:.1}), {:.0} km/h, laps {}, damage {:?}, pit ghost {}",
                        p[0],
                        p[1],
                        p[2],
                        rd::<f32>(car.car, CAR_VALUE_CACHE_SPEED) * 3.6,
                        rd::<u32>(car.car.add(CAR_TRANSPONDER), 0x0c),
                        damage,
                        rd::<u8>(car.car, 0x3e68)
                    )
                })
                .collect()
        }
    }

    pub fn object_count(&self) -> usize {
        self.objects.len()
    }
}
