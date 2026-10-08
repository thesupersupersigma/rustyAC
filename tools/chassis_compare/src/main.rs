// SPDX-License-Identifier: GPL-3.0-or-later

//! The Rust rolling chassis against the whole-car recordings of `tools/car_oracle`.
//!
//! chassis_compare run [<scenario> ...] [--dir <folder>] [--verbose] [--stop-after <steps>]
//!     Free run from step 0 of every `<folder>/<scenario>.carrec` (default folder `oracle/car`;
//!     default: all but `settle_floor`, whose floor contacts are stage 2 of the rigid-body
//!     port): the Rust
//!     chassis (body, fuel tank, hubs, double-wishbone suspensions, heave springs, anti-roll
//!     bars, steering, force feedback) with the Rust tyres on the Rust ODE, built from
//!     `cardata/<car>` and fed only what the systems that are not ported yet hand to it
//!     (controls, engine values for the fuel burn, brake torques, driven-wheel speeds, wing
//!     forces). After every step every body state, joint value, suspension value, tyre value,
//!     the steering signal, the force-feedback number and the whole force tape are compared
//!     with the game, bit for bit. The full run writes `oracle/chassis/results.md`
//!     (`results_<folder name>.md` for another folder).
//! chassis_compare test-car
//!     Writes two copies of the F2004 with a few changed values, for branches no recording of
//!     the real car reaches: `cardata/f2004_tight_stops` (packers and bump stops of wheels and
//!     heave springs moved to where ordinary driving reaches them) and `cardata/f2004_fallbacks`
//!     (keys set to zero or removed so that the loaders' defaults apply, a linear steer-assist
//!     curve, a rear toe the setup screen rounds). Recordings of them are made by the game's
//!     own code: `car_oracle all --car <name> --out oracle/car_<suffix>`.
//! chassis_compare excerpt
//!     Writes the golden excerpts of `crates/rustyac-physics/tests/chassis_golden.rs`.
//! chassis_compare faults [<scenario>] [--dir <folder>]
//!     A check of the check: the same free run (default scenario `slalom`) with one small
//!     deliberate fault in the Rust chassis at a time (a damper rate, a bar rate, a spring rate
//!     … changed in its last bit, the setup rounding left out, the joint softening left out),
//!     and where the comparison first notices. Writes `oracle/chassis/faults.md`
//!     (`faults_<folder name>.md` for another folder).
//!
//! Needs no acs.exe.

#[path = "../../car_oracle/src/record.rs"]
#[allow(dead_code, clippy::wrong_self_convention)]
mod record;
// the game's input file and state dump (Task 11): `game-replay` writes the one and reads the other
#[path = "../../../crates/rustyac-game/src/dump.rs"]
#[allow(dead_code)]
mod dump;
#[path = "../../../crates/rustyac-game/src/input_file.rs"]
#[allow(dead_code)]
mod input_file;
#[path = "../../car_oracle/src/sites.rs"]
#[allow(dead_code)]
mod sites;

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use record::Recording;
use rustyac_physics::car::replay::{
    self, body_words, Field, Golden, GoldenStep, Ground, RecordedCall, RecordedStep, RecordedWheel, RunSetup, WHEELS,
};
use rustyac_physics::car::{CarControls, ChassisEnvironment, EngineFeed, ForceSource, RollingChassis, TapeCall};
use rustyac_physics::vecmath::Vec3f;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().and_then(Path::parent).expect("the repository folder").to_path_buf()
}

/// Systems whose force calls the chassis makes itself.
const OWN_SYSTEMS: [&str; 12] = [
    // the torque reaction of a rigid rear axle (a chassis that is fed its drivetrain has none)
    "drivetrain",
    "tyre",
    "surface",
    "spring",
    "damper",
    "bumpstop",
    "heave_spring",
    "heave_damper",
    "heave_bumpstop",
    "arb",
    "sleep",
    // the stability aid's yaw torque: made by the aids model (a chassis without one does not
    // make it, and the tape comparison says so)
    "stability",
];
/// Systems whose force calls are fed from the tape.
const FED_SYSTEMS: [&str; 2] = ["aero_drag", "aero_lift"];

/// Which systems the Rust car computes itself. What it does not, the recording feeds.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Systems {
    brakes: bool,
    drivetrain: bool,
    aero: bool,
    aids: bool,
}

impl Systems {
    /// Everything that is ported.
    const ALL: Systems = Systems { brakes: true, drivetrain: true, aero: true, aids: true };
    /// The rolling chassis alone, as in Task 08.
    const CHASSIS: Systems = Systems { brakes: false, drivetrain: false, aero: false, aids: false };

    fn describe(&self) -> String {
        if *self == Systems::ALL {
            return "the whole car in Rust: the only inputs are the driver's controls (device and cockpit brake-bias \
                    clicks) and the session's and track's settings"
                .to_string();
        }
        let fed = self.fed();
        format!("fed from the recording: {}; everything else in Rust", fed.join(", "))
    }

    /// The names of the systems taken from the recording.
    fn fed(&self) -> Vec<&'static str> {
        let mut fed = Vec::new();
        for (rust, name) in [(self.brakes, "brakes"), (self.drivetrain, "drivetrain"), (self.aero, "aero"), (self.aids, "aids")] {
            if !rust {
                fed.push(name);
            }
        }
        fed
    }

    /// The tail of the result files' names. The modes of the earlier tasks keep theirs.
    fn suffix(&self) -> String {
        match (self.brakes, self.drivetrain, self.aero, self.aids) {
            (true, true, true, true) => "_whole".to_string(),
            (false, false, false, false) => String::new(),
            (true, true, false, false) => "_powertrain".to_string(),
            (true, false, false, false) => "_brakes".to_string(),
            (false, true, false, false) => "_drivetrain".to_string(),
            _ => format!("_feed_{}", self.fed().join("_")),
        }
    }
}

/// How a recording was made, as far as the chassis needs to know.
fn run_setup(recording: &Recording, systems: Systems) -> Result<RunSetup, String> {
    let get = |key: &str| recording.get(key).ok_or(format!("the recording's header has no {key}"));
    let first = |name: &str| recording.f(0, name);
    let number = |key: &str| -> Result<f32, String> {
        match recording.get(key) {
            Some(text) => text.parse::<f32>().map_err(|e| format!("{key}: {e}")),
            None => Ok(0.0),
        }
    };
    let env = ChassisEnvironment {
        ambient_temperature: first("physics.ambientTemperature"),
        road_temperature: first("physics.roadTemperature"),
        dynamic_grip_level: first("track.dynamicGripLevel"),
        tyre_consumption_rate: first("tyre.lf.in_tyre_consumption_rate"),
        mechanical_damage_rate: first("tyre.lf.in_mechanical_damage_rate"),
        // the session's penalty rule (the engine's default is 3, "nothing")
        penalty_mode: match recording.get("penalty_mode") {
            Some(text) => text.parse().map_err(|e| format!("penalty_mode: {e}"))?,
            None => 3,
        },
        allow_tyre_blankets: recording.i(0, "tyre.lf.in_allow_tyre_blankets") != 0,
        ..ChassisEnvironment::default()
    };
    let conditions = conditions(recording)?;
    let mut env = env;
    if conditions.dynamic_track.is_some() {
        // the port's own dynamic track has to find the grip: nothing of the game's is handed in
        env.dynamic_grip_level = 1.0;
    }
    Ok(RunSetup {
        scenario: get("scenario")?.to_string(),
        ground: Ground::parse(get("ground")?).ok_or("the recording's ground")?,
        seed: get("seed")?.parse().map_err(|e| format!("seed: {e}"))?,
        clock_start_ms: get("clock_start_ms")?.parse().map_err(|e| format!("clock_start_ms: {e}"))?,
        env,
        rust_brakes: systems.brakes,
        rust_drivetrain: systems.drivetrain,
        rust_aero: systems.aero,
        rust_aids: systems.aids,
        // the page is the whole car's: it needs every system
        telemetry: systems == Systems::ALL,
        // the whole-car scenarios' session settings; older recordings have none of them
        pitlane: recording.get("pitlane").is_some_and(|v| v != "0"),
        stability_gain: number("stability_gain")?,
        wind_speed: number("wind_speed")?,
        wind_direction_deg: number("wind_direction_deg")?,
        damage: match recording.get("damage") {
            Some(text) => replay::parse_damage(text)?,
            None => [0.0; 5],
        },
        auto_clutch: get("auto_clutch")? != "0",
        // the key came with the powertrain scenarios; older recordings ran without the aid
        auto_shifter: recording.get("auto_shifter").is_some_and(|v| v != "0"),
        track: track_run(recording)?,
        collide: collide_run(recording)?,
        conditions,
    })
}

/// The session's conditions of a Task 15 recording (`car_oracle`'s header keys); the port
/// turns them into the car's values itself.
fn conditions(recording: &Recording) -> Result<replay::Conditions, String> {
    let numbers = |key: &str| recording.get(key).map(replay::Conditions::parse_hex).transpose().map_err(|e| format!("{key}: {e}"));
    let mut conditions = replay::Conditions::default();
    if let Some(w) = numbers("wind_ini")? {
        let [min, max, direction] = w[..] else { return Err("wind_ini: three numbers expected".to_string()) };
        conditions.wind = Some(rustyac_physics::session::WindIni { speed_kmh_min: min, speed_kmh_max: max, direction_deg: direction });
    }
    if let Some(d) = numbers("dynamic_track")? {
        let [start, randomness, gain, transfer] = d[..] else { return Err("dynamic_track: four numbers expected".to_string()) };
        conditions.dynamic_track = Some(rustyac_physics::session::DynamicTrackIni { session_start: start, randomness, lap_gain: gain, session_transfer: transfer });
    }
    if let Some(b) = numbers("ballast_kg")? {
        conditions.ballast_kg = b[0];
    }
    if let Some(r) = numbers("restrictor")? {
        conditions.restrictor = r[0];
    }
    conditions.setup_file = recording.get("setup_file").map(std::path::PathBuf::from);
    Ok(conditions)
}

/// What the car's body touches in a recording made with `car_oracle run --collide`. The
/// collider mesh is read by the port from the game's folder, as the oracle read it.
fn collide_run(recording: &Recording) -> Result<replay::CollideRun, String> {
    if !recording.get("collide").is_some_and(|v| v != "0") {
        return Ok(replay::CollideRun::default());
    }
    let get = |key: &str| recording.get(key).ok_or(format!("the recording's header has no {key}"));
    let mesh = match get("collider_mesh")? {
        "real" => {
            let car = get("car")?;
            let data = repo_root().join("cardata").join(car);
            let game = rustyac_physics::track::loader::game_root(Path::new(get("collider_kn5")?)).ok_or("the collider mesh is not inside a game folder")?;
            let colliders = rustyac_physics::car::colliders::load(&data, Some(&game), car)?;
            Some(colliders.mesh.ok_or("the port finds no collider.kn5 for this car")?)
        }
        _ => None,
    };
    Ok(replay::CollideRun {
        on: true,
        mesh,
        mesh_bounce_vel: f32::from_bits(u32::from_str_radix(get("mesh_bounce_vel")?, 16).map_err(|e| format!("mesh_bounce_vel: {e}"))?),
        no_collision_steps: get("no_collision_steps")?.parse().map_err(|e| format!("no_collision_steps: {e}"))?,
        floor: recording.get("floor_mesh").is_some_and(|v| v != "0"),
    })
}

/// Three floats written as hexadecimal bit patterns.
fn hex3(text: &str) -> Result<Vec3f, String> {
    let words: Vec<f32> = text.split(',').filter_map(|w| u32::from_str_radix(w, 16).ok()).map(f32::from_bits).collect();
    match words.as_slice() {
        [x, y, z] => Ok(Vec3f::new(*x, *y, *z)),
        _ => Err(format!("three hexadecimal words expected, got {text:?}")),
    }
}

thread_local! {
    /// The tracks loaded so far, by folder (loading Spa takes half a second).
    static TRACKS: std::cell::RefCell<Vec<(String, std::sync::Arc<rustyac_physics::track::Track>)>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// The track of a recording made with `car_oracle run --track`, loaded by the Rust port from
/// the game's folder, and the spawn the recording's header gives. A hot-lap spawn is worked
/// out again by the port and has to be the header's, bit for bit.
fn track_run(recording: &Recording) -> Result<Option<replay::TrackRun>, String> {
    let Some(folder) = recording.get("track_folder") else { return Ok(None) };
    let get = |key: &str| recording.get(key).ok_or(format!("the recording's header has no {key}"));
    let position = hex3(get("spawn_position")?)?;
    let tail = hex3(get("spawn_tail")?)?;
    let track = TRACKS.with(|tracks| -> Result<_, String> {
        if let Some((_, track)) = tracks.borrow().iter().find(|(f, _)| f == folder) {
            return Ok(std::sync::Arc::clone(track));
        }
        let (mut track, _) = rustyac_physics::track::load_track(Path::new(folder), "")?;
        rustyac_physics::track::init_respawn_position_set(&mut track, "HOTLAP_START");
        let track = std::sync::Arc::new(track);
        tracks.borrow_mut().push((folder.to_string(), std::sync::Arc::clone(&track)));
        Ok(track)
    })?;
    if get("spawn")? == "AC_HOTLAP_START_0" {
        let ours = track.spawn_pose("HOTLAP_START", 0).ok_or("the port finds no hot-lap spawn on this track")?;
        let bits = |v: &Vec3f| [v.x.to_bits(), v.y.to_bits(), v.z.to_bits()];
        if (bits(&ours.0), bits(&ours.1)) != (bits(&position), bits(&tail)) {
            return Err(format!("the port's hot-lap spawn {ours:?} is not the recording's {:?}", (position, tail)));
        }
    }
    if let Some(length) = recording.get("track_ai_length") {
        let ours = format!("{:08x}", track.length().to_bits());
        if ours != length {
            return Err(format!("the AI line is {ours} long in the port and {length} in the game"));
        }
    }
    Ok(Some(replay::TrackRun {
        track,
        position,
        tail,
        armed: get("armed")? != "0",
        allowed_tyres_out: get("allowed_tyres_out")?.parse().map_err(|e| format!("allowed_tyres_out: {e}"))?,
    }))
}

/// What the chassis is fed in one step: only values that systems outside the chassis own.
fn recorded_step(recording: &Recording, step: usize) -> Result<RecordedStep, String> {
    let f = |name: &str| recording.f(step, name);
    // the powertrain scenarios record what the script did with handbrake, H-shifter and brake
    // bias; in the older recordings the device left them alone
    let script_i = |name: &str, or: &str| recording.i(step, if recording.has(name) { name } else { or });
    let mut out = RecordedStep {
        controls: CarControls {
            gas: f("script.gas"),
            brake: f("script.brake"),
            steer: f("script.steer"),
            clutch: f("script.clutch"),
            gear_up: recording.i(step, "script.gearUp") != 0,
            gear_dn: recording.i(step, "script.gearDn") != 0,
            drs: recording.has("script.drs") && recording.i(step, "script.drs") != 0,
            kers: recording.has("script.kers") && recording.i(step, "script.kers") != 0,
            requested_gear_index: script_i("script.requestedGear", "controls.requestedGearIndex"),
            hand_brake: if recording.has("script.handBrake") { f("script.handBrake") } else { f("controls.handBrake") },
        },
        bias_clicks: if recording.has("script.biasClicks") { recording.i(step, "script.biasClicks") } else { 0 },
        hybrid: if recording.has("script.ersPower") {
            rustyac_physics::car::replay::HybridJobs {
                ers_power: recording.i(step, "script.ersPower"),
                ers_recovery: recording.i(step, "script.ersRecovery"),
                ers_heat: recording.i(step, "script.ersHeat"),
                engine_brake: recording.i(step, "script.engineBrake"),
            }
        } else {
            Default::default()
        },
        headlights: recording.has("script.headlights") && recording.i(step, "script.headlights") != 0,
        // only the automatic clutch rewrites the clutch pedal, before the sleeping rule reads it
        clutch: f("controls.clutch"),
        // what traction control and the pit limiter left for the next step
        engine_electronic_override: f("engine.electronicOverride"),
        brake_electronic_override: f("brakes.electronicOverride"),
        ..RecordedStep::default()
    };
    // what the engine and the gearbox left at the end of the previous step; before the first
    // step the car is in neutral and the engine has not run (its fuel use is zero)
    if step == 0 {
        out.gear = 1;
        out.engine = EngineFeed::default();
    } else {
        out.gear = recording.i(step - 1, "drivetrain.currentGear");
        out.engine = EngineFeed {
            rpm: recording.f(step - 1, "drivetrain.engineRPM"),
            gas_usage: recording.f(step - 1, "engine.gasUsage"),
            turbo_boost: recording.f(step - 1, "engine.status.turboBoost"),
        };
    }
    for (index, wheel) in WHEELS.iter().enumerate() {
        let t = |name: &str| recording.f(step, &format!("tyre.{wheel}.{name}"));
        let mut w = RecordedWheel {
            brake_torque: t("in_brake_torque"),
            hand_brake_torque: t("in_hand_brake_torque"),
            electric_torque: t("in_electric_torque"),
            abs_override: t("in_abs_override"),
            ai_mult: t("in_ai_mult"),
            driven: recording.i(step, &format!("tyre.{wheel}.in_driven")) != 0,
            ..RecordedWheel::default()
        };
        if w.driven {
            w.angular_velocity = t("in_set_av_value");
            for k in 0..16 {
                w.local_wheel_rotation[k] = t(&format!("in_localWheelRotation.M{}{}", k / 4 + 1, k % 4 + 1));
            }
        }
        out.wheels[index] = w;
    }
    out.edl_active = recording.has("edl.outLevel") && f("edl.outLevel") > 0.0;
    for call in &recording.steps[step].calls {
        let system = recording.system_of(call);
        if FED_SYSTEMS.contains(&system) {
            if call.body != 0 {
                return Err(format!("step {step}: a {system} call on body {}", call.body));
            }
            let source = ForceSource::from_name(system).unwrap();
            out.aero.push(RecordedCall { kind: call.kind, source, a: call.a, b: call.b });
        } else if system == "stability" {
            // only a chassis that is fed its aids takes these from the recording
            out.stability.push(RecordedCall { kind: call.kind, source: ForceSource::Stability, a: call.a, b: call.b });
        } else if !OWN_SYSTEMS.contains(&system) {
            return Err(format!("step {step}: a force call of the system '{system}', which is neither ported nor fed"));
        }
    }
    Ok(out)
}

/// The recording's columns of the compared fields.
struct Columns {
    fields: Vec<Field>,
    kinds: Vec<char>,
    columns: Vec<usize>,
}

/// The bodies and joints of the recorded car, from the names of the recording's fields (a
/// strut car has two more bodies than a double-wishbone car, an axle car one fewer).
fn recorded_layout(recording: &Recording) -> replay::CarLayout {
    let mut layout = replay::CarLayout { bodies: Vec::new(), joints: Vec::new() };
    for (_, name, _) in &recording.fields {
        if let Some(body) = name.strip_suffix(".pre.pos.x") {
            layout.bodies.push(body.to_string());
        }
        if let Some(joint) = name.strip_prefix("joint.").and_then(|rest| rest.strip_suffix(".tag")) {
            let has = |part: &str| recording.has(&format!("joint.{joint}.{part}"));
            let shape = if has("distance") {
                replay::JointShape::Rod
            } else if has("axis1.x") {
                replay::JointShape::Slider
            } else if has("qrel.w") {
                replay::JointShape::Fixed
            } else {
                replay::JointShape::Ball
            };
            layout.joints.push((joint.to_string(), shape));
        }
    }
    layout
}

/// The recorded name of a body of the force tape.
fn body_name(recording: &Recording, body: u32) -> String {
    recorded_layout(recording).bodies.get(body as usize).cloned().unwrap_or_else(|| format!("body {body}"))
}

impl Columns {
    fn new(recording: &Recording) -> Result<Columns, String> {
        let fields = replay::fields_of(&recorded_layout(recording));
        let mut columns = Vec::with_capacity(fields.len());
        for field in &fields {
            if !recording.has(&field.name) {
                return Err(format!("the recording has no field {}", field.name));
            }
            columns.push(recording.col(&field.name));
        }
        let kinds = fields.iter().map(|f| f.kind).collect();
        Ok(Columns { fields, kinds, columns })
    }

    /// The game's values of one step, in snapshot order.
    fn game(&self, recording: &Recording, step: usize) -> Vec<u64> {
        let words = &recording.steps[step].words;
        self.columns
            .iter()
            .zip(&self.kinds)
            .map(|(&c, &kind)| if kind == 'd' { words[c] as u64 | (words[c + 1] as u64) << 32 } else { words[c] as u64 })
            .collect()
    }
}

/// What the recording's script asked of the car besides the device's controls, applied the
/// way the oracle applied it: the car's own functions, called before the step (the clock
/// still shows the last step). Only the whole-car recordings have such jobs.
fn apply_jobs(chassis: &mut replay::Runner, recording: &Recording, step: usize) {
    if recording.has("script.teleport") {
        // a teleport of a track scenario, as the oracle made it through the game's own
        // Car::forceRotation and Car::forcePosition before the step
        let kind = recording.i(step, "script.teleport");
        let vector = |name: &str| Vec3f::new(recording.f(step, &format!("{name}.x")), recording.f(step, &format!("{name}.y")), recording.f(step, &format!("{name}.z")));
        if kind == 3 {
            // put down in any attitude (a rolled-over car)
            let row = |k: usize| {
                let v = vector(&format!("script.teleportRow{k}"));
                [v.x, v.y, v.z]
            };
            chassis.force_attitude(&[row(0), row(1), row(2)], &vector("script.teleportPosition"));
        } else if kind != 0 {
            chassis.force_rotation(&vector("script.teleportTail"));
            chassis.force_position_with(&vector("script.teleportPosition"), kind == 2);
        }
    }
    if !recording.has("script.lockMs") {
        return;
    }
    let lock_ms = recording.f(step, "script.lockMs");
    if lock_ms != 0.0 {
        let now = chassis.physics_time;
        chassis.lock_controls_until(lock_ms as f64, now);
    }
    match recording.i(step, "script.setLocked") {
        0 => {}
        value => chassis.lock_controls(value > 0),
    }
    match recording.i(step, "script.gentleStop") {
        0 => {}
        value => chassis.is_gentle_stopping = value > 0,
    }
    let penalty = recording.f(step, "script.addPenalty");
    if penalty != 0.0 {
        chassis.add_penalty(penalty as f64);
    }
}

fn same_float(a: f32, b: f32) -> bool {
    a.to_bits() == b.to_bits() || (a.is_nan() && b.is_nan())
}

/// Compares the Rust force tape of a step with the game's. `Err` describes the first difference.
fn compare_tape(recording: &Recording, step: usize, chassis: &RollingChassis) -> Result<(), String> {
    compare_tape_calls(recording, step, chassis.core.tape.as_deref().unwrap_or(&[]))
}

/// The same for a tape that is already off the chassis (`game-replay` reads it from the game's dump).
fn compare_tape_calls(recording: &Recording, step: usize, rust: &[TapeCall]) -> Result<(), String> {
    let game = &recording.steps[step].calls;
    for (seq, (g, r)) in game.iter().zip(rust).enumerate() {
        let system = recording.system_of(g);
        if g.body != r.body || g.kind != r.kind {
            return Err(format!(
                "force call {seq} ({} on {}, {system}): Rust made {} on {}",
                replay::kind_name(g.kind),
                body_name(recording, g.body),
                replay::kind_name(r.kind),
                body_name(recording, r.body)
            ));
        }
        let head = || format!("force call {seq} ({} on {}, {system})", replay::kind_name(g.kind), body_name(recording, g.body));
        if system != r.source.name() {
            return Err(format!("{}: Rust books it under {}", head(), r.source.name()));
        }
        // a `stop` has no vectors (the recording keeps the wrapper's unused argument there)
        let vectors: &[(&str, [f32; 3], [f32; 3])] = &[
            ("a", g.a, r.a),
            ("b", g.b, r.b),
            ("force accumulator after it", g.facc, r.facc),
            ("torque accumulator after it", g.tacc, r.tacc),
        ];
        for (name, gv, rv) in &vectors[if g.kind == 8 { 2 } else { 0 }..] {
            for k in 0..3 {
                if !same_float(gv[k], rv[k]) {
                    return Err(format!(
                        "{}: {name}[{k}]: game {:?} ({:#010x}) / Rust {:?} ({:#010x})",
                        head(),
                        gv[k],
                        gv[k].to_bits(),
                        rv[k],
                        rv[k].to_bits()
                    ));
                }
            }
        }
    }
    if game.len() != rust.len() {
        return Err(format!("the game made {} force calls, Rust {}", game.len(), rust.len()));
    }
    Ok(())
}

/// One deliberate fault put into the Rust chassis after it is built.
type Fault = fn(&mut RollingChassis);

/// The next f32 up (in magnitude).
fn nudge(x: f32) -> f32 {
    f32::from_bits(x.to_bits() + 1)
}

/// Takes a setup item off its value: an attached item would write the setup's value back at
/// the end of the next step (as it does in the game) and undo the fault.
fn detach(chassis: &mut RollingChassis, name: &str) {
    let item = chassis.setup_manager.items.iter_mut().find(|item| item.name == name).expect("a setup item");
    item.attached = false;
}

const FAULTS: [(&str, &str, Fault); 18] = [
    ("damper", "left front slow bump damping one bit up", |c| {
        detach(c, "DAMP_BUMP_LF");
        let d = c.suspensions[0].damper_mut();
        d.bump_slow = nudge(d.bump_slow);
    }),
    ("spring", "right rear spring rate one bit up", |c| {
        detach(c, "SPRING_RATE_RR");
        let b = c.suspensions[3].base_mut();
        b.k = nudge(b.k);
    }),
    ("rod", "left rear rod length one bit up (in magnitude)", |c| {
        detach(c, "ROD_LENGTH_LR");
        let b = c.suspensions[2].base_mut();
        b.rod_length = nudge(b.rod_length);
    }),
    ("bump_stop_rate", "right front packer / bump stop rate one bit up", |c| {
        detach(c, "BUMP_STOP_RATE_RF");
        let b = c.suspensions[1].base_mut();
        b.bump_stop_rate = nudge(b.bump_stop_rate);
    }),
    ("packer", "left front packer range one bit up", |c| {
        detach(c, "PACKER_RANGE_LF");
        let b = c.suspensions[0].base_mut();
        b.packer_range = nudge(b.packer_range);
    }),
    ("bump_stop_up", "left rear upper bump stop one bit further", |c| {
        let b = c.suspensions[2].base_mut();
        b.bump_stop_up = nudge(b.bump_stop_up);
    }),
    ("bump_stop_dn", "right rear lower bump stop one bit further", |c| {
        let b = c.suspensions[3].base_mut();
        b.bump_stop_dn = nudge(b.bump_stop_dn);
    }),
    ("heave_packer", "front heave spring packer range one bit up", |c| {
        c.heave_springs[0].packer_range = nudge(c.heave_springs[0].packer_range)
    }),
    ("heave_bump_stop", "rear heave spring lower bump stop one bit further", |c| {
        c.heave_springs[1].bump_stop_dn = nudge(c.heave_springs[1].bump_stop_dn)
    }),
    ("arb", "front anti-roll bar rate one bit up", |c| {
        detach(c, "ARB_FRONT");
        c.antiroll_bars[0].k = nudge(c.antiroll_bars[0].k)
    }),
    ("heave", "rear heave spring rate one bit up", |c| {
        detach(c, "SPRING_RATE_HR");
        c.heave_springs[1].k = nudge(c.heave_springs[1].k)
    }),
    ("heave_damper", "front heave damper slow rebound one bit up", |c| {
        detach(c, "DAMP_REBOUND_HF");
        c.heave_springs[0].damper.rebound_slow = nudge(c.heave_springs[0].damper.rebound_slow)
    }),
    ("steer_ratio", "steering rod ratio one bit up", |c| c.steering_system.linear_ratio = nudge(c.steering_system.linear_ratio)),
    ("ffmult", "force-feedback gain one bit up", |c| c.ff_mult = nudge(c.ff_mult)),
    ("fuel_kg", "fuel density one bit up", |c| c.fuel_kg = nudge(c.fuel_kg)),
    ("consumption", "fuel consumption factor one (single-precision) bit up", |c| {
        c.fuel_consumption_k = nudge(c.fuel_consumption_k as f32) as f64
    }),
    ("no_setup_rounding", "setup-screen rounding left out (camber, toe, packers as in the car's files)", |c| {
        for item in &mut c.setup_manager.items {
            item.attached = false;
        }
    }),
    ("erp", "the low-speed softening of the joints left out", |c| c.env.is_first_car = false),
];

/// The game's value of one traced powertrain value in a step, if the recording holds it.
fn game_trace_word(recording: &Recording, step: usize, value: &replay::TraceValue) -> Option<u64> {
    if !recording.has(&value.name) {
        return None;
    }
    Some(match value.kind {
        'd' => recording.d(step, &value.name).to_bits(),
        'i' => recording.i(step, &value.name) as u32 as u64,
        _ => recording.f(step, &value.name).to_bits() as u64,
    })
}

/// The game's values of the powertrain trace, in trace order; `None` for a value the recording
/// does not hold. A missing value that every recording should hold is an error.
fn game_trace(recording: &Recording, step: usize, trace: &[replay::TraceValue]) -> Result<Vec<Option<u64>>, String> {
    trace
        .iter()
        .map(|value| match game_trace_word(recording, step, value) {
            None if !value.extra => Err(format!("the recording has no field {}", value.name)),
            word => Ok(word),
        })
        .collect()
}

/// Deliberate faults in the ported brakes, engine and drivetrain ("one bit up" is the next
/// representable number). A value a setup item is attached to is detached first.
const POWERTRAIN_FAULTS: [(&str, &str, Fault); 12] = [
    ("brake_power", "brake torque at full pedal one bit up", |c| {
        let brakes = c.brake_system.as_mut().unwrap();
        // the public face of the brake system has no setter for its power: the multiplier
        detach_item(&mut c.setup_manager, "BRAKE_POWER_MULT");
        let base = brakes.base_mut();
        base.brake_power_multiplier = nudge(base.brake_power_multiplier);
    }),
    ("front_bias", "front brake bias one bit up", |c| {
        detach_item(&mut c.setup_manager, "FRONT_BIAS");
        let base = c.brake_system.as_mut().unwrap().base_mut();
        base.front_bias = nudge(base.front_bias);
    }),
    ("engine_inertia", "engine inertia one bit up", |c| {
        let base = c.drivetrain.as_mut().unwrap().engine_mut().base_mut();
        base.inertia = nudge(base.inertia);
    }),
    ("limiter", "rev limiter one part in 8 million lower", |c| {
        let base = c.drivetrain.as_mut().unwrap().engine_mut().base_mut();
        base.limiter_multiplier = f32::from_bits(base.limiter_multiplier.to_bits() - 1);
    }),
    ("clutch_torque", "clutch capacity one (double-precision) bit up", |c| {
        let base = c.drivetrain.as_mut().unwrap().base_mut();
        base.clutch_max_torque = f64::from_bits(base.clutch_max_torque.to_bits() + 1);
    }),
    ("clutch_inertia", "gearbox inertia one bit up", |c| {
        let base = c.drivetrain.as_mut().unwrap().base_mut();
        base.clutch_inertia = nudge(base.clutch_inertia);
    }),
    ("first_gear", "first gear's ratio one (double-precision) bit up", |c| {
        let base = c.drivetrain.as_mut().unwrap().base_mut();
        base.gears[2].ratio = f64::from_bits(base.gears[2].ratio.to_bits() + 1);
    }),
    ("final_ratio", "final drive ratio one bit up", |c| {
        detach_item(&mut c.setup_manager, "FINAL_RATIO");
        let base = c.drivetrain.as_mut().unwrap().base_mut();
        base.final_ratio = nudge(base.final_ratio);
    }),
    // (one bit would not show: a shift ends at the first whole step after its time)
    ("shift_time", "up-shift time one physics step (3 ms) longer", |c| {
        let base = c.drivetrain.as_mut().unwrap().base_mut();
        base.gear_up_time += 0.003;
    }),
    ("diff_power", "differential power ramp one bit up", |c| {
        detach_item(&mut c.setup_manager, "DIFF_POWER");
        let base = c.drivetrain.as_mut().unwrap().base_mut();
        base.diff_power_ramp = nudge(base.diff_power_ramp);
    }),
    ("diff_preload", "differential preload one bit up", |c| {
        detach_item(&mut c.setup_manager, "DIFF_PRELOAD");
        let base = c.drivetrain.as_mut().unwrap().base_mut();
        base.diff_pre_load = nudge(base.diff_pre_load);
    }),
    ("wheel_inertia", "left driven wheel's inertia one (double-precision) bit up", |c| {
        let base = c.drivetrain.as_mut().unwrap().base_mut();
        base.out_shaft_l.inertia = f64::from_bits(base.out_shaft_l.inertia.to_bits() + 1);
    }),
];

/// Deliberate faults in the systems of Task 10 (wings, aids, the car-level glue, the telemetry
/// page), for a whole car.
const WHOLE_FAULTS: [(&str, &str, Fault); 12] = [
    ("wing_area", "the rear wing's area one bit up", |c| {
        let wing = &mut c.aero.as_mut().unwrap().base_mut().wings[2];
        wing.data.area = nudge(wing.data.area);
    }),
    ("wing_cl_gain", "the front wing's lift gain one bit up", |c| {
        let wing = &mut c.aero.as_mut().unwrap().base_mut().wings[1];
        wing.data.cl_gain = nudge(wing.data.cl_gain);
    }),
    ("wing_position", "the body's aero point one bit further forward", |c| {
        let wing = &mut c.aero.as_mut().unwrap().base_mut().wings[0];
        wing.data.position.z = nudge(wing.data.position.z);
    }),
    ("wing_controller", "the smoothing rate of the first wing controller the car has one bit up", |c| {
        for wing in &mut c.aero.as_mut().unwrap().base_mut().wings {
            if let Some(controller) = wing.dynamic_controllers.first_mut() {
                controller.filter = nudge(controller.filter);
                return;
            }
        }
        // a car without active aero: the last wing's angle instead
        let wing = c.aero.as_mut().unwrap().base_mut().wings.last_mut().unwrap();
        wing.status.angle = nudge(wing.status.angle);
    }),
    ("wing_damage", "a dented nose (front damage level 60)", |c| c.damage_zone_level[0] = 60.0),
    ("wind", "a 3 m/s wind along the road instead of none", |c| c.env.set_wind(3.0, 0.0)),
    ("tc_rate", "traction control checks one step more often", |c| {
        let tc = &mut c.aids.as_mut().unwrap().base_mut().traction_control;
        tc.frequency -= 0.003;
    }),
    ("abs_on", "ABS forced on (a car without it) or switched off (a car with it)", |c| {
        let abs = &mut c.aids.as_mut().unwrap().base_mut().abs;
        let has = abs.is_present && abs.is_active;
        abs.is_present = !has;
        abs.is_active = !has;
    }),
    ("stability", "the stability aid at 50 % instead of off", |c| {
        c.aids.as_mut().unwrap().base_mut().stability_control.gain = 0.5;
    }),
    ("ff_gain", "the user's force-feedback gain one bit up (only the telemetry page shows it)", |c| {
        c.user_ff_gain = nudge(c.user_ff_gain);
    }),
    ("ride_pickup", "the front ride-height pickup point one bit higher (only the telemetry page shows it)", |c| {
        let writer = c.telemetry.as_mut().unwrap();
        writer.ride_pickup_point[0].y = nudge(writer.ride_pickup_point[0].y);
    }),
    ("black_flag", "the car is black-flagged at the start (controls dead, put into a pit box at the origin)", |c| {
        c.black_flagged = true;
    }),
];

fn detach_item(manager: &mut rustyac_physics::car::SetupManager, name: &str) {
    let item = manager.items.iter_mut().find(|item| item.name == name).expect("a setup item");
    item.attached = false;
}

/// How often the branches of brakes, engine and drivetrain were taken (counted on the Rust
/// car, which is the game's as long as the run is bit-exact).
#[derive(Clone, Copy, Default)]
struct PowertrainCoverage {
    /// Steps with the brake pedal down / with handbrake torque on the rear wheels.
    braking: usize,
    handbrake: usize,
    /// Steps with a cockpit brake bias in force, with an electronic brake balance, with
    /// steer-brake torque, with brake fade (disc temperatures on).
    cockpit_bias: usize,
    ebb: usize,
    brake_temps: usize,
    /// Steps the rev limiter cut, the engine ran below idle, the engine had no fuel pressure,
    /// traction control (fed) cut the throttle.
    limiter: usize,
    below_idle: usize,
    no_fuel: usize,
    tc_cut: usize,
    /// Steps with turbo boost.
    boost: usize,
    /// Steps by clutch state and gear.
    locked_in_gear: usize,
    locked_neutral: usize,
    slipping_in_gear: usize,
    slipping_neutral: usize,
    /// Paddle shifts started (up, down), steps of the throttle cut after an up-shift, paddle
    /// presses the gearbox refused (top gear, a shift in progress, down-shift protection).
    shifts_up: usize,
    shifts_down: usize,
    cut_off: usize,
    refused: usize,
    /// Steps in reverse, steps with the H-shifter grinding.
    reverse: usize,
    grinding: usize,
    /// Steps the differential held both wheels to the carrier / let them differ.
    diff_holding: usize,
    diff_slipping: usize,
    /// Steps the drivetrain held the driven wheels at zero (both flagged locked).
    wheels_held: usize,
    /// Steps a clutch profile of the automatic clutch was playing, steps the automatic blip
    /// raised the throttle, paddle presses of the automatic gearbox.
    clutch_sequence: usize,
    blip: usize,
    auto_shifts: usize,
}

impl PowertrainCoverage {
    fn count(&mut self, chassis: &RollingChassis, feed: &RecordedStep, before_request: i32, paddles_before: (bool, bool)) {
        if let Some(brakes) = &chassis.brake_system {
            self.braking += (feed.controls.brake > 0.0) as usize;
            self.handbrake += (chassis.tyres[2].inputs.hand_brake_torque > 0.0) as usize;
            let mut trace = Vec::new();
            brakes.trace(&mut trace);
            let value = |name: &str| trace.iter().find(|v| v.name == name).map(|v| f32::from_bits(v.word as u32)).unwrap_or(0.0);
            self.cockpit_bias += (value("brakes.biasOverride") != -1.0) as usize;
            self.ebb += brakes.is_using_ebb() as usize;
            // the rear discs: a car with temperatures for the rear discs only has no front brakes
            self.brake_temps += (value("brakes.disc.lr.t") != chassis.env.ambient_temperature && value("brakes.disc.lr.t") != 0.0) as usize;
        }
        if let Some(drivetrain) = &chassis.drivetrain {
            let b = drivetrain.base();
            let e = drivetrain.engine().base();
            self.limiter += e.status.is_limiter_on as usize;
            self.below_idle += (e.last_input.rpm < drivetrain.engine().minimum() as f32) as usize;
            self.no_fuel += (e.fuel_pressure < 1.0) as usize;
            // what is left for the next step's engine (the car's own value)
            self.tc_cut += (e.electronic_override != 1.0) as usize;
            self.boost += (e.status.turbo_boost > 0.0) as usize;
            let in_gear = b.ratio != 0.0;
            match (b.clutch_open_state, in_gear) {
                (false, true) => self.locked_in_gear += 1,
                (false, false) => self.locked_neutral += 1,
                (true, true) => self.slipping_in_gear += 1,
                (true, false) => self.slipping_neutral += 1,
            }
            let request = b.gear_request.request as i32;
            if before_request == 0 && request == 1 {
                self.shifts_up += 1;
            }
            if before_request == 0 && request == 2 {
                self.shifts_down += 1;
            }
            self.cut_off += (b.cut_off > 0.0) as usize;
            if chassis.controls.requested_gear_index == -1 {
                let changer = &chassis.gear_changer;
                self.refused += (chassis.controls.gear_up && !paddles_before.0 && !changer.was_gear_up_triggered) as usize;
                self.refused += (chassis.controls.gear_dn && !paddles_before.1 && !changer.was_gear_dn_triggered) as usize;
            }
            self.reverse += (b.current_gear == 0) as usize;
            self.grinding += b.is_gear_grinding as usize;
            let held = b.out_shaft_l.velocity == b.drive.velocity && b.out_shaft_r.velocity == b.drive.velocity;
            if held {
                self.diff_holding += 1;
            } else {
                self.diff_slipping += 1;
            }
            let (l, r) = (&chassis.tyres[b.tyre_left], &chassis.tyres[b.tyre_right]);
            self.wheels_held += (l.status.is_locked && r.status.is_locked && b.clutch_open_state) as usize;
            self.clutch_sequence += (!chassis.autoclutch.clutch_sequence.is_done) as usize;
            self.blip += (chassis.controls.gas > feed.controls.gas) as usize;
            self.auto_shifts += ((chassis.controls.gear_up && !feed.controls.gear_up) || (chassis.controls.gear_dn && !feed.controls.gear_dn)) as usize;
        }
    }

    fn add(&mut self, other: &PowertrainCoverage) {
        let pairs: [(&mut usize, usize); 26] = [
            (&mut self.braking, other.braking),
            (&mut self.handbrake, other.handbrake),
            (&mut self.cockpit_bias, other.cockpit_bias),
            (&mut self.ebb, other.ebb),
            (&mut self.brake_temps, other.brake_temps),
            (&mut self.limiter, other.limiter),
            (&mut self.below_idle, other.below_idle),
            (&mut self.no_fuel, other.no_fuel),
            (&mut self.tc_cut, other.tc_cut),
            (&mut self.boost, other.boost),
            (&mut self.locked_in_gear, other.locked_in_gear),
            (&mut self.locked_neutral, other.locked_neutral),
            (&mut self.slipping_in_gear, other.slipping_in_gear),
            (&mut self.slipping_neutral, other.slipping_neutral),
            (&mut self.shifts_up, other.shifts_up),
            (&mut self.shifts_down, other.shifts_down),
            (&mut self.cut_off, other.cut_off),
            (&mut self.refused, other.refused),
            (&mut self.reverse, other.reverse),
            (&mut self.grinding, other.grinding),
            (&mut self.diff_holding, other.diff_holding),
            (&mut self.diff_slipping, other.diff_slipping),
            (&mut self.wheels_held, other.wheels_held),
            (&mut self.clutch_sequence, other.clutch_sequence),
            (&mut self.blip, other.blip),
            (&mut self.auto_shifts, other.auto_shifts),
        ];
        for (mine, theirs) in pairs {
            *mine += theirs;
        }
    }

    const HEAD: &'static str = "| Scenario | Brake pedal / handbrake / cockpit bias / EBB / disc temps | Limiter / below idle / no fuel / throttle cut by an aid / boost | Clutch locked in gear / locked neutral / slipping in gear / slipping neutral | Paddle shifts up / down / cut-off steps / refused presses | Reverse / grinding | Diff holds / slips / wheels held | Clutch profile / blip / auto-shifter presses |\n|---|---|---|---|---|---|---|---|";

    fn row(&self, name: &str) -> String {
        format!(
            "| {name} | {} / {} / {} / {} / {} | {} / {} / {} / {} / {} | {} / {} / {} / {} | {} / {} / {} / {} | {} / {} | {} / {} / {} | {} / {} / {} |",
            self.braking,
            self.handbrake,
            self.cockpit_bias,
            self.ebb,
            self.brake_temps,
            self.limiter,
            self.below_idle,
            self.no_fuel,
            self.tc_cut,
            self.boost,
            self.locked_in_gear,
            self.locked_neutral,
            self.slipping_in_gear,
            self.slipping_neutral,
            self.shifts_up,
            self.shifts_down,
            self.cut_off,
            self.refused,
            self.reverse,
            self.grinding,
            self.diff_holding,
            self.diff_slipping,
            self.wheels_held,
            self.clutch_sequence,
            self.blip,
            self.auto_shifts,
        )
    }
}

/// How often the branches of the wings, the aids, the car-level glue and the telemetry page
/// were taken (Task 10).
#[derive(Clone, Copy, Default)]
struct WholeCoverage {
    /// Force calls of the wings on the game's tape.
    wing_calls: usize,
    /// Wing-steps in which a wing's controllers held its angle away from the set-up angle.
    active_aero: usize,
    /// Steps with the DRS open; presses of its button.
    drs_open: usize,
    drs_presses: usize,
    last_drs: bool,
    /// Steps with a wing under an angle override or an effect factor of the DRS.
    drs_wing_steps: usize,
    /// Steps traction control held the engine cut; wheel-steps with the brake released by
    /// the ABS; steps the differential lock braked a wheel; torque calls of the stability aid.
    tc_in_action: usize,
    abs_released: usize,
    edl: usize,
    stability_calls: usize,
    /// Steps the pit limiter cut the engine / asked for the brakes; steps the body mesh was
    /// a ghost for other cars.
    limiter: usize,
    limiter_brake: usize,
    pit_ghost: usize,
    /// Steps with wind.
    wind: usize,
    /// Steps the page's ride heights were the standing-car estimate.
    ride_estimate: usize,
    /// Steps a wing produced no force because the air came from behind or stood still.
    no_lift: usize,
}

impl WholeCoverage {
    const HEAD: &'static str = "| Scenario | Wing force calls / wing-steps moved by a controller | DRS open steps / button presses / wing-steps under DRS | TC cutting / wheel-steps released by ABS / EDL braking / stability torque calls | Pit limiter cutting / braking / ghost for cars | Wind steps | Ride height estimated | Wing-steps without lift |\n|---|---|---|---|---|---|---|---|";

    fn count(&mut self, chassis: &RollingChassis, recording: &Recording, step: usize, feed: &RecordedStep) {
        for call in &recording.steps[step].calls {
            match recording.system_of(call) {
                "aero_drag" | "aero_lift" => self.wing_calls += 1,
                "stability" => self.stability_calls += 1,
                _ => {}
            }
        }
        if let Some(aero) = &chassis.aero {
            let base = aero.base();
            for wing in &base.wings {
                self.active_aero += (!wing.dynamic_controllers.is_empty() && wing.status.angle != wing.status.input_angle) as usize;
                self.drs_wing_steps += (wing.override_status.is_active || wing.status.angle_mult != 1.0) as usize;
                self.no_lift += (wing.status.cl == 0.0 && wing.data.cl_gain != 0.0) as usize;
            }
            self.drs_open += base.drs.is_active as usize;
        }
        self.drs_presses += (feed.controls.drs && !self.last_drs) as usize;
        self.last_drs = feed.controls.drs;
        if let Some(aids) = &chassis.aids {
            let base = aids.base();
            self.tc_in_action += base.traction_control.is_in_action as usize;
            self.abs_released += chassis.tyres.iter().filter(|tyre| tyre.abs_override == 0.0).count();
            self.edl += (base.edl.is_present && base.edl.is_active && base.edl.out_level > 0.0) as usize;
            self.limiter += base.speed_limiter.is_limiting as usize;
        }
        if let Some(brakes) = &chassis.brake_system {
            self.limiter_brake += (brakes.base().electronic_override > 0.0) as usize;
        }
        self.pit_ghost += chassis.is_collision_off_for_pits as usize;
        self.wind += (chassis.env.wind_speed >= 0.01) as usize;
        if let Some(writer) = &chassis.telemetry {
            self.ride_estimate += (writer.minimum_height > 0.0 && step > 0 && recording.f(step - 1, "car.speed") < 2.0) as usize;
        }
    }

    fn add(&mut self, o: &WholeCoverage) {
        self.wing_calls += o.wing_calls;
        self.active_aero += o.active_aero;
        self.drs_open += o.drs_open;
        self.drs_presses += o.drs_presses;
        self.drs_wing_steps += o.drs_wing_steps;
        self.tc_in_action += o.tc_in_action;
        self.abs_released += o.abs_released;
        self.edl += o.edl;
        self.stability_calls += o.stability_calls;
        self.limiter += o.limiter;
        self.limiter_brake += o.limiter_brake;
        self.pit_ghost += o.pit_ghost;
        self.wind += o.wind;
        self.ride_estimate += o.ride_estimate;
        self.no_lift += o.no_lift;
    }

    fn row(&self, name: &str) -> String {
        format!(
            "| {name} | {} / {} | {} / {} / {} | {} / {} / {} / {} | {} / {} / {} | {} | {} | {} |",
            self.wing_calls,
            self.active_aero,
            self.drs_open,
            self.drs_presses,
            self.drs_wing_steps,
            self.tc_in_action,
            self.abs_released,
            self.edl,
            self.stability_calls,
            self.limiter,
            self.limiter_brake,
            self.pit_ghost,
            self.wind,
            self.ride_estimate,
            self.no_lift,
        )
    }
}

/// How often the branches that ordinary driving of the F2004 never reaches were taken.
#[derive(Clone, Copy, Default)]
struct Coverage {
    /// Wheel-steps with the packer engaged (travel above the packer range).
    packer: usize,
    /// Force calls of the wheels' bump stops on the game's tape.
    bumpstop_calls: usize,
    /// Axle-steps with a heave spring's packer engaged.
    heave_packer: usize,
    /// Force calls of the heave springs' bump stops on the game's tape.
    heave_bumpstop_calls: usize,
    /// Steps in which the sleeping rule froze the body.
    frozen: usize,
    /// Wheel-steps in which the spring did not push (travel not positive).
    spring_idle: usize,
}

struct Outcome {
    coverage: Coverage,
    powertrain: PowertrainCoverage,
    whole: WholeCoverage,
    track: TrackCoverage,
    collision: CollisionCoverage,
    track_name: String,
    /// Values of brakes, engine and drivetrain compared per step (those the recording holds).
    powertrain_values: usize,
    scenario: String,
    steps: usize,
    exact: usize,
    first: Option<(usize, String)>,
    /// How many of the compared values differed in the first diverging step.
    first_count: usize,
    values: usize,
    calls: usize,
    seconds: f64,
}

/// What a run on a real track went through, counted on the Rust car.
#[derive(Clone, Debug, Default)]
struct TrackCoverage {
    on_track: bool,
    /// Tyre rays cast, and those that found nothing.
    rays: usize,
    ray_misses: usize,
    /// Different meshes the rays hit.
    meshes: std::collections::BTreeSet<i32>,
    /// Tyre-steps per surface key.
    surfaces: std::collections::BTreeMap<String, usize>,
    /// Metres driven and the fastest moment, km/h.
    distance: f64,
    max_kmh: f32,
    /// The place along the AI line at the start and at the end (0..1).
    npos: (f32, f32),
    /// The ground's height under the car's left front wheel: lowest and highest.
    height: (f32, f32),
    /// The heaviest and the lightest tyre load of a step with all four tyres' loads summed, N.
    load: (f32, f32),
    /// Tyre-steps without load (off the ground).
    airborne: usize,
    /// Steps with more than two tyres off the track; cuts counted.
    steps_out: usize,
    cuts: usize,
    /// Timing lines crossed, laps counted, the first lap's time, ms.
    crossings: usize,
    laps: u32,
    last_lap: u32,
    last_pos: Option<[f32; 3]>,
    last_cuts: i32,
}

/// What a run with collisions on did with them, counted on the Rust car.
#[derive(Clone, Copy, Default)]
struct CollisionCoverage {
    /// The recording was made with collisions (`car_oracle run --collide`).
    on: bool,
    /// Steps with at least one contact joint of a floor box / of the collider mesh.
    steps_box: usize,
    steps_mesh: usize,
    /// The first step with a contact joint of the collider mesh.
    first_mesh_step: Option<usize>,
    /// Most contact joints alive in one step; the sum over all steps.
    max_joints: usize,
    joints: usize,
    /// Calls of `Car::onCollisionCallBack` (new contact joints); the highest closing speed, km/h.
    callbacks: usize,
    max_rel_speed: f32,
    /// Steps in which the stepper had bounded rows; rows and LCP pivots summed.
    lcp_steps: usize,
    lcp_rows: usize,
    lcp_pivots: usize,
    /// At the end: damage zones, suspension damage, engine life.
    damage: [f32; 5],
    suspension_damage: [f32; 4],
    engine_life: f64,
    steps: usize,
}

impl CollisionCoverage {
    fn count(&mut self, chassis: &RollingChassis) {
        use rustyac_physics::ode::{GeomRef, JointKind};
        if !chassis.collisions_enabled {
            return;
        }
        self.on = true;
        let core = &chassis.core;
        let (mut boxes, mut meshes, mut joints) = (0, 0, 0);
        for id in core.contact_joints() {
            let JointKind::Contact { contact, .. } = &core.world.joint(id).kind else { continue };
            joints += 1;
            for g in [contact.geom.g1, contact.geom.g2] {
                if let GeomRef::Dyn(geom) = g {
                    if core.box_colliders(chassis.body).contains(&geom) {
                        boxes += 1;
                    } else if core.mesh_colliders(chassis.body).contains(&geom) {
                        meshes += 1;
                    }
                }
            }
        }
        if boxes > 0 {
            self.steps_box += 1;
        }
        if meshes > 0 {
            self.steps_mesh += 1;
            self.first_mesh_step.get_or_insert(self.steps);
        }
        self.max_joints = self.max_joints.max(joints);
        self.joints += joints;
        self.callbacks += chassis.contact_callbacks as usize;
        for event in &chassis.collision_events {
            self.max_rel_speed = self.max_rel_speed.max(event.rel_speed);
        }
        if chassis.step_stats.bounded_rows > 0 {
            self.lcp_steps += 1;
            self.lcp_rows += chassis.step_stats.bounded_rows as usize;
            self.lcp_pivots += chassis.step_stats.lcp_pivots as usize;
        }
        self.damage = chassis.damage_zone_level;
        for (slot, suspension) in self.suspension_damage.iter_mut().zip(&chassis.suspensions) {
            *slot = suspension.get_damage();
        }
        if let Some(drivetrain) = &chassis.drivetrain {
            self.engine_life = drivetrain.engine().base().life_left;
        }
        self.steps += 1;
    }
}

impl TrackCoverage {
    fn count(&mut self, chassis: &RollingChassis, step: usize) {
        let Some(track) = &chassis.track else { return };
        self.on_track = true;
        let Some(trace) = &chassis.trace else { return };
        let mut load = 0.0;
        for (w, tyre) in trace.tyres.iter().enumerate() {
            self.rays += 1;
            if tyre.ray.hit {
                self.meshes.insert(tyre.ray.mesh);
            } else {
                self.ray_misses += 1;
            }
            if let Some(surface) = &chassis.tyres[w].surface_def {
                *self.surfaces.entry(track.surface_key(surface).to_string()).or_insert(0) += 1;
            }
            let tyre_load = chassis.tyres[w].status.load;
            load += tyre_load;
            self.airborne += (step > 400 && tyre_load <= 0.0) as usize;
        }
        if step > 400 {
            self.load = if self.load == (0.0, 0.0) { (load, load) } else { (self.load.0.max(load), self.load.1.min(load)) };
        }
        let p = chassis.core.get_position(chassis.body);
        let p = [p.x, p.y, p.z];
        if let Some(last) = self.last_pos {
            self.distance += ((p[0] - last[0]).powi(2) + (p[1] - last[1]).powi(2) + (p[2] - last[2]).powi(2)).sqrt() as f64;
        }
        self.last_pos = Some(p);
        self.max_kmh = self.max_kmh.max(chassis.speed * 3.6);
        let n = chassis.spline_locator.normalized_pos;
        if step == 0 {
            self.npos.0 = n;
        }
        self.npos.1 = n;
        if let Some(ray) = trace.tyres.first().filter(|t| t.ray.hit) {
            let y = ray.ray.pos[1];
            self.height = if self.height == (0.0, 0.0) { (y, y) } else { (self.height.0.min(y), self.height.1.max(y)) };
        }
        self.steps_out += (chassis.lap_invalidator.current_tyres_out > 2) as usize;
        let cuts = chassis.transponder.cuts;
        self.cuts += (cuts > self.last_cuts) as usize;
        self.last_cuts = cuts;
        let crossed = chassis.transponder.status.iter().zip(&track.time_lines).filter(|(s, line)| s.last_response == 2 && line.check(&chassis.tyres[0].world_position) == 2).count();
        let _ = crossed;
        self.crossings += chassis.transponder.finish_line_passed as usize + chassis.transponder.split_events.len();
        self.laps = chassis.transponder.lap_count;
        self.last_lap = chassis.transponder.last_lap;
    }

    fn row(&self, name: &str, track: &str) -> String {
        let surfaces: Vec<String> = self.surfaces.iter().map(|(key, n)| format!("{key} {n}")).collect();
        format!(
            "| `{name}` | {track} | {:.0} m, up to {:.0} km/h | {:.4} to {:.4} | {:.1} to {:.1} m | {} ({} without a hit), {} meshes | {} | {:.0} to {:.0} N; {} | {} / {} | {} / {} / {} |",
            self.distance,
            self.max_kmh,
            self.npos.0,
            self.npos.1,
            self.height.0,
            self.height.1,
            self.rays,
            self.ray_misses,
            self.meshes.len(),
            surfaces.join(", "),
            self.load.1,
            self.load.0,
            self.airborne,
            self.steps_out,
            self.cuts,
            self.crossings,
            self.laps,
            if self.last_lap == 0 { "-".to_string() } else { format!("{} ms", self.last_lap) }
        )
    }
}

fn compare(
    recording: &Recording,
    data: &Path,
    systems: Systems,
    verbose: bool,
    stop_after: Option<usize>,
    fault: Option<Fault>,
) -> Result<Outcome, String> {
    let mut systems = systems;
    if !systems.brakes && systems.aids && (0..recording.steps.len()).any(|step| recording.f(step, "edl.outLevel") > 0.0) {
        // the recorded brake torques already contain the differential lock's part, so the
        // lock cannot be computed on top of them
        println!("  (the aids are fed too: this car's differential lock acts, and the fed brake torques include it)");
        systems.aids = false;
    }
    let setup = run_setup(recording, systems)?;
    let columns = Columns::new(recording)?;
    let mut chassis = setup.build_runner(data)?;
    if let Some(fault) = fault {
        fault(&mut chassis);
    }
    let started = std::time::Instant::now();
    let mut outcome = Outcome {
        coverage: Coverage::default(),
        powertrain: PowertrainCoverage::default(),
        whole: WholeCoverage::default(),
        track: TrackCoverage::default(),
        collision: CollisionCoverage::default(),
        track_name: recording.get("track").unwrap_or("").to_string(),
        powertrain_values: 0,
        scenario: setup.scenario.clone(),
        steps: 0,
        exact: 0,
        first: None,
        first_count: 0,
        values: columns.fields.len(),
        calls: 0,
        seconds: 0.0,
    };
    let steps = recording.steps.len().min(stop_after.unwrap_or(usize::MAX));
    for step in 0..steps {
        let feed = recorded_step(recording, step)?;
        let feed = if systems == Systems::ALL { feed.driver_only() } else { feed };
        apply_jobs(&mut chassis, recording, step);
        let request_before = chassis.drivetrain.as_ref().map(|d| d.base().gear_request.request as i32).unwrap_or(0);
        let paddles_before = (chassis.gear_changer.last_gear_up, chassis.gear_changer.last_gear_dn);
        chassis.step_recorded(setup.time_of_step(step), &feed);
        let rust = replay::snapshot(&chassis);
        let game = columns.game(recording, step);
        let mut differing = Vec::new();
        for (k, field) in columns.fields.iter().enumerate() {
            if !replay::same_value(field.kind, game[k], rust[k]) {
                differing.push(k);
            }
        }
        // brakes, engine, drivetrain, shift helpers
        let mut trace = replay::powertrain_trace(&chassis);
        // on a real track: the rays, the lap timer, the place along the AI line
        trace.extend(replay::track_trace(&chassis));
        let game_values = game_trace(recording, step, &trace)?;
        outcome.powertrain_values = game_values.iter().flatten().count();
        let mut trace_differences = Vec::new();
        for (value, game) in trace.iter().zip(&game_values) {
            if let Some(game) = game {
                if !replay::same_value(value.kind, *game, value.word) {
                    trace_differences.push(format!(
                        "{}: game {} / Rust {}",
                        value.name,
                        replay::describe(value.kind, *game),
                        replay::describe(value.kind, value.word)
                    ));
                }
            }
        }
        outcome.powertrain.count(&chassis, &feed, request_before, paddles_before);
        outcome.whole.count(&chassis, recording, step, &feed);
        outcome.track.count(&chassis, step);
        outcome.collision.count(&chassis);
        // the lap and split events were counted: the list does not grow over a long run
        chassis.transponder.take_events();
        let tape = compare_tape(recording, step, &chassis);
        let coverage = &mut outcome.coverage;
        for suspension in &chassis.suspensions {
            let (travel, base) = (suspension.get_status().travel, suspension.base());
            coverage.packer += (base.packer_range != 0.0 && travel > base.packer_range && base.k != 0.0) as usize;
            coverage.spring_idle += (travel <= 0.0) as usize;
        }
        for heave in &chassis.heave_springs {
            coverage.heave_packer += (heave.k != 0.0 && heave.packer_range != 0.0 && heave.travel > heave.packer_range) as usize;
        }
        for call in &recording.steps[step].calls {
            match recording.system_of(call) {
                "bumpstop" => coverage.bumpstop_calls += 1,
                "heave_bumpstop" => coverage.heave_bumpstop_calls += 1,
                "sleep" if call.body == 0 => coverage.frozen += 1,
                _ => {}
            }
        }
        outcome.steps += 1;
        outcome.calls += recording.steps[step].calls.len();
        if differing.is_empty() && trace_differences.is_empty() && tape.is_ok() {
            outcome.exact += 1;
            continue;
        }
        if outcome.first.is_none() {
            let describe = |k: usize| {
                let field = &columns.fields[k];
                format!(
                    "{}: game {} / Rust {}",
                    field.name,
                    replay::describe(field.kind, game[k]),
                    replay::describe(field.kind, rust[k])
                )
            };
            // a value of the powertrain names its system; the tape is in execution order, so
            // its first difference is the earliest cause among the chassis values
            let text = match (trace_differences.first(), &tape, differing.first()) {
                (Some(text), _, _) => text.clone(),
                (None, Err(tape), _) => tape.clone(),
                (None, Ok(()), Some(&k)) => describe(k),
                (None, Ok(()), None) => unreachable!(),
            };
            outcome.first = Some((step, text));
            outcome.first_count = differing.len() + trace_differences.len();
            if verbose {
                println!(
                    "  first difference at step {step}; {} of {} values differ:",
                    differing.len() + trace_differences.len(),
                    columns.fields.len() + outcome.powertrain_values
                );
                for text in trace_differences.iter().take(60) {
                    println!("    {text}");
                }
                for &k in differing.iter().take(40) {
                    println!("    {}", describe(k));
                }
                if let Err(tape) = &tape {
                    println!("    tape: {tape}");
                }
            }
        }
    }
    outcome.seconds = started.elapsed().as_secs_f64();
    Ok(outcome)
}

fn percent(part: usize, whole: usize) -> String {
    if whole == 0 {
        return "n/a".to_string();
    }
    if part == whole {
        return format!("100 % ({part}/{whole})");
    }
    format!("{:.3} % ({part}/{whole})", part as f64 * 100.0 / whole as f64)
}

fn car_data(recording: &Recording) -> Result<PathBuf, String> {
    let car = recording.get("car").ok_or("the recording's header has no car")?;
    let data = repo_root().join("cardata").join(car);
    if !data.join("car.ini").is_file() {
        return Err(format!("{}: the car's data folder is missing", data.display()));
    }
    Ok(data)
}

fn run_command(names: &[String], dir: Option<&Path>, systems: Systems, verbose: bool, stop_after: Option<usize>) -> Result<(), String> {
    let repo = repo_root();
    let folder = match dir {
        Some(dir) if dir.is_absolute() => dir.to_path_buf(),
        Some(dir) => repo.join(dir),
        None => repo.join("oracle/car"),
    };
    let full = names.is_empty() && stop_after.is_none();
    let mut scenarios: Vec<String> = names.to_vec();
    if scenarios.is_empty() {
        for entry in std::fs::read_dir(&folder).map_err(|e| format!("{}: {e}", folder.display()))? {
            let path = entry.map_err(|e| e.to_string())?.path();
            if path.extension().is_some_and(|e| e == "carrec") {
                let name = path.file_stem().unwrap().to_string_lossy().to_string();
                scenarios.push(name);
            }
        }
        scenarios.sort();
    }
    let mut outcomes = Vec::new();
    for name in &scenarios {
        let path = folder.join(format!("{name}.carrec"));
        let recording = Recording::read(&path)?;
        if names.is_empty() && name == "settle_floor" && recording.get("collide").is_none() {
            // a recording from before Task 13: the game's car stood on its floor boxes, but
            // the file does not say what the floor was (record it again with --collide)
            println!("{name}: skipped (recorded before the collision port; no collision set-up in its header)");
            continue;
        }
        let data = car_data(&recording)?;
        println!("{name}: {} steps", recording.steps.len());
        let outcome = compare(&recording, &data, systems, verbose, stop_after, None)?;
        match &outcome.first {
            None => println!("  bit-exact: {} ({:.1} s)", percent(outcome.exact, outcome.steps), outcome.seconds),
            Some((step, text)) => {
                println!("  bit-exact: {}; first difference at step {step}: {text}", percent(outcome.exact, outcome.steps))
            }
        }
        outcomes.push(outcome);
    }
    let mut table = String::new();
    writeln!(table, "Free run, {}.\n", systems.describe()).unwrap();
    writeln!(
        table,
        "| Scenario | Steps | Bit-exact steps | First divergence | Chassis values compared per step | Values of the other \
         systems compared per step (brakes, engine, drivetrain; wings, aids, car glue, telemetry page) | Force calls compared |"
    )
    .unwrap();
    writeln!(table, "|---|---|---|---|---|---|---|").unwrap();
    let (mut steps, mut exact, mut calls) = (0, 0, 0);
    for o in &outcomes {
        let first = match &o.first {
            None => "none".to_string(),
            Some((step, text)) => format!("step {step}: {text} ({} values differ in that step)", o.first_count),
        };
        writeln!(
            table,
            "| `{}` | {} | {} | {} | {} | {} | {} |",
            o.scenario,
            o.steps,
            percent(o.exact, o.steps),
            first,
            o.values,
            o.powertrain_values,
            o.calls
        )
        .unwrap();
        steps += o.steps;
        exact += o.exact;
        calls += o.calls;
    }
    writeln!(table, "| **all** | **{steps}** | **{}** | | | | **{calls}** |", percent(exact, steps)).unwrap();
    writeln!(table).unwrap();
    if systems != Systems::CHASSIS {
        writeln!(table, "{}", PowertrainCoverage::HEAD).unwrap();
        let mut total = PowertrainCoverage::default();
        for o in &outcomes {
            writeln!(table, "{}", o.powertrain.row(&format!("`{}`", o.scenario))).unwrap();
            total.add(&o.powertrain);
        }
        writeln!(table, "{}", total.row("**all**")).unwrap();
        writeln!(table).unwrap();
    }
    if systems == Systems::ALL {
        writeln!(table, "{}", WholeCoverage::HEAD).unwrap();
        let mut total = WholeCoverage::default();
        for o in &outcomes {
            writeln!(table, "{}", o.whole.row(&format!("`{}`", o.scenario))).unwrap();
            total.add(&o.whole);
        }
        writeln!(table, "{}", total.row("**all**")).unwrap();
        writeln!(table).unwrap();
    }
    writeln!(
        table,
        "| Scenario | Wheel-steps on a packer | Bump-stop force calls | Axle-steps on a heave packer | Heave bump-stop force \
         calls | Steps frozen by the sleeping rule | Wheel-steps with an idle spring |"
    )
    .unwrap();
    writeln!(table, "|---|---|---|---|---|---|---|").unwrap();
    let mut total = Coverage::default();
    for o in &outcomes {
        let c = o.coverage;
        writeln!(
            table,
            "| `{}` | {} | {} | {} | {} | {} | {} |",
            o.scenario, c.packer, c.bumpstop_calls, c.heave_packer, c.heave_bumpstop_calls, c.frozen, c.spring_idle
        )
        .unwrap();
        total.packer += c.packer;
        total.bumpstop_calls += c.bumpstop_calls;
        total.heave_packer += c.heave_packer;
        total.heave_bumpstop_calls += c.heave_bumpstop_calls;
        total.frozen += c.frozen;
        total.spring_idle += c.spring_idle;
    }
    writeln!(
        table,
        "| **all** | **{}** | **{}** | **{}** | **{}** | **{}** | **{}** |",
        total.packer, total.bumpstop_calls, total.heave_packer, total.heave_bumpstop_calls, total.frozen, total.spring_idle
    )
    .unwrap();
    if outcomes.iter().any(|o| o.track.on_track) {
        writeln!(
            table,
            "\nOn a real track ({}). Counted on the Rust car, which the table above shows to be the game's car.\n",
            if outcomes.iter().any(|o| o.collision.on) {
                "with collisions: the car's floor boxes and collider mesh meet the track's meshes, as in a session of the game"
            } else {
                "the car's body is a ghost in the game and in the port; only the tyres' rays meet the track's meshes"
            }
        )
        .unwrap();
        writeln!(
            table,
            "| Scenario | Track | Driven | Along the AI line (0..1), start to end | Ground height under the left front wheel | Tyre rays, meshes hit | Tyre-steps per surface | Sum of the four tyre loads, lowest to highest; tyre-steps without load | Steps with more than two tyres off the track / cuts | Timing lines crossed / laps counted / last lap |"
        )
        .unwrap();
        writeln!(table, "|---|---|---|---|---|---|---|---|---|---|").unwrap();
        for o in outcomes.iter().filter(|o| o.track.on_track) {
            writeln!(table, "{}", o.track.row(&o.scenario, &o.track_name)).unwrap();
        }
    }
    if outcomes.iter().any(|o| o.collision.on) {
        writeln!(
            table,
            "\nCollisions (contacts between the car's six floor boxes / its collider mesh and the track's meshes, the contact joints the \
             solver gets, and what the game's collision callback does with them). Counted on the Rust car; contact joints, damage zones, \
             suspension damage, engine life and the two collision clocks are among the values compared every step.\n"
        )
        .unwrap();
        writeln!(
            table,
            "| Scenario | Steps with floor-box contact joints | Steps with collider-mesh contact joints (first at step) | Most contact joints in a step / in all steps | Collision callbacks / highest closing speed | Steps solved by the LCP solver / bounded rows / pivots | Damage zones at the end (front, rear, left, right, centre) | Suspension damage at the end (LF, RF, LR, RR) | Engine life at the end |"
        )
        .unwrap();
        writeln!(table, "|---|---|---|---|---|---|---|---|---|").unwrap();
        for o in outcomes.iter().filter(|o| o.collision.on) {
            let c = &o.collision;
            let list = |values: &[f32]| values.iter().map(|v| format!("{v:.1}")).collect::<Vec<_>>().join(", ");
            writeln!(
                table,
                "| `{}` | {} | {}{} | {} / {} | {} / {:.0} km/h | {} / {} / {} | {} | {} | {:.0} |",
                o.scenario,
                c.steps_box,
                c.steps_mesh,
                c.first_mesh_step.map(|s| format!(" ({s})")).unwrap_or_default(),
                c.max_joints,
                c.joints,
                c.callbacks,
                c.max_rel_speed,
                c.lcp_steps,
                c.lcp_rows,
                c.lcp_pivots,
                list(&c.damage),
                c.suspension_damage.iter().map(|v| format!("{v:.2}")).collect::<Vec<_>>().join(", "),
                c.engine_life
            )
            .unwrap();
        }
    }
    println!("\n{table}");
    let out = repo.join("oracle/chassis");
    std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
    let mut suffix = match dir {
        Some(dir) => format!("_{}", dir.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()),
        None => String::new(),
    };
    suffix.push_str(&systems.suffix());
    let file = out.join(if full { format!("results{suffix}.md") } else { format!("partial{suffix}.md") });
    std::fs::write(&file, &table).map_err(|e| format!("{}: {e}", file.display()))?;
    println!("{}", file.display());
    if outcomes.iter().any(|o| o.first.is_some()) {
        return Err("the Rust chassis and the game differ".to_string());
    }
    Ok(())
}

/// Does the recording's script ask anything of the car besides the device's controls (locks,
/// a gentle stop, penalties)? An input file of the game has no such commands.
fn has_jobs(recording: &Recording) -> bool {
    if recording.has("script.teleport") && (0..recording.steps.len()).any(|step| recording.i(step, "script.teleport") != 0) {
        return true;
    }
    recording.has("script.lockMs")
        && (0..recording.steps.len()).any(|step| {
            recording.f(step, "script.lockMs") != 0.0
                || recording.i(step, "script.setLocked") != 0
                || recording.i(step, "script.gentleStop") != 0
                || recording.f(step, "script.addPenalty") != 0.0
        })
}

/// `game-replay`: the game itself against the recordings (Task 11, check 1). The recorded
/// controls of a scenario become an input file of `rustyac.exe`; the game replays it
/// (`--replay --headless --dump-states -`) and its car is compared with the recording after
/// every step exactly as `run` compares: the 2,009 chassis values, the values of the other
/// systems with the telemetry page, and the force tape.
fn game_replay_command(names: &[String], dir: Option<&Path>, exe: Option<&Path>, verbose: bool) -> Result<(), String> {
    use std::process::{Command, Stdio};
    let repo = repo_root();
    let folder = match dir {
        Some(dir) if dir.is_absolute() => dir.to_path_buf(),
        Some(dir) => repo.join(dir),
        None => repo.join("oracle/car"),
    };
    let folder_name = folder.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let exe = exe.map(Path::to_path_buf).unwrap_or_else(|| repo.join("target/release/rustyac.exe"));
    if !exe.is_file() {
        return Err(format!("{}: build the game first (cargo build --release -p rustyac-game)", exe.display()));
    }
    let full = names.is_empty();
    let mut scenarios: Vec<String> = names.to_vec();
    if scenarios.is_empty() {
        for entry in std::fs::read_dir(&folder).map_err(|e| format!("{}: {e}", folder.display()))? {
            let path = entry.map_err(|e| e.to_string())?.path();
            if path.extension().is_some_and(|e| e == "carrec") {
                scenarios.push(path.file_stem().unwrap().to_string_lossy().to_string());
            }
        }
        scenarios.sort();
    }
    let inputs = repo.join("oracle/game");
    std::fs::create_dir_all(&inputs).map_err(|e| e.to_string())?;
    let mut table = String::new();
    writeln!(
        table,
        "The game against the recordings: `rustyac.exe --replay <input file> --headless --dump-states -`, the input \
         file holding nothing but the recording's driver controls (and its session values in the header).\n"
    )
    .unwrap();
    writeln!(
        table,
        "| Scenario | Steps | Bit-exact steps | First divergence | Chassis values compared per step | Values of the other \
         systems compared per step | Force calls compared |"
    )
    .unwrap();
    writeln!(table, "|---|---|---|---|---|---|---|").unwrap();
    let (mut all_steps, mut all_exact, mut all_calls) = (0, 0, 0);
    let mut failed = false;
    for name in &scenarios {
        let path = folder.join(format!("{name}.carrec"));
        let recording = Recording::read(&path)?;
        let count = recording.steps.len();
        if full && name == "settle_floor" && recording.get("collide").is_none() {
            println!("{name}: skipped (recorded before the collision port; no collision set-up in its header)");
            continue;
        }
        println!("{name}: {count} steps");
        if has_jobs(&recording) {
            println!("  skipped: its script locks the controls / adds penalties, which an input file cannot ask for");
            writeln!(table, "| `{name}` | {count} | skipped (the script calls the car's lock / penalty functions) | | | | |").unwrap();
            continue;
        }
        let run = run_setup(&recording, Systems::ALL)?;
        let setup = input_file::SimSetup {
            car: recording.get("car").ok_or("the recording's header has no car")?.to_string(),
            seed: run.seed,
            clock_start_ms: run.clock_start_ms,
            env: run.env,
            auto_clutch: run.auto_clutch,
            auto_shifter: run.auto_shifter,
            auto_blip: None,
            session_starts_at_spawn: false,
            drs_zones: true,
            ff_gain: 1.0,
            track: String::new(),
            spawn: "hotlap".to_string(),
            session: Default::default(),
            oracle: Some(input_file::OracleSetup {
                scenario: run.scenario.clone(),
                ground: run.ground,
                pitlane: run.pitlane,
                stability_gain: run.stability_gain,
                wind_speed: run.wind_speed,
                wind_direction_deg: run.wind_direction_deg,
                damage: run.damage,
                conditions: run.conditions.clone(),
                track: run.track.as_ref().map(|track| input_file::OracleTrack {
                    folder: recording.get("track_folder").unwrap_or("").to_string(),
                    position: [track.position.x, track.position.y, track.position.z],
                    tail: [track.tail.x, track.tail.y, track.tail.z],
                    armed: track.armed,
                    allowed_tyres_out: track.allowed_tyres_out,
                }),
                collide: run.collide.on.then(|| input_file::OracleCollide {
                    collider_mesh: run.collide.mesh.is_some(),
                    mesh_bounce_vel: run.collide.mesh_bounce_vel.to_bits(),
                    no_collision_steps: run.collide.no_collision_steps,
                    floor: run.collide.floor,
                }),
            }),
        };
        let mut steps = Vec::with_capacity(count);
        for step in 0..count {
            let feed = recorded_step(&recording, step)?.driver_only();
            steps.push(input_file::StepInput {
                controls: feed.controls,
                headlights: feed.headlights,
                events: 0,
                bias_clicks: feed.bias_clicks,
                device: 0,
            });
        }
        let input = inputs.join(format!("{folder_name}_{name}.ryin"));
        input_file::InputFile { setup, steps }.write(&input)?;
        let started = std::time::Instant::now();
        let mut child = Command::new(&exe)
            .args(["--replay", &input.to_string_lossy(), "--headless", "--no-shm", "--dump-states", "-"])
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|e| format!("{}: {e}", exe.display()))?;
        let stdout = child.stdout.take().expect("the game's standard output");
        let mut reader = dump::DumpReader::new(std::io::BufReader::with_capacity(1 << 20, stdout))?;
        let columns = Columns::new(&recording)?;
        let (mut exact, mut calls, mut other_values) = (0, 0, 0);
        let mut first: Option<(usize, String, usize)> = None;
        for step in 0..count {
            let Some(rust) = reader.next_step()? else {
                return Err(format!("{name}: the game stopped after {step} of {count} steps"));
            };
            let game = columns.game(&recording, step);
            if rust.snapshot.len() != game.len() {
                return Err(format!("{name}: the game's dump has {} chassis values, the recording {}", rust.snapshot.len(), game.len()));
            }
            let differing: Vec<usize> =
                (0..game.len()).filter(|&k| !replay::same_value(columns.fields[k].kind, game[k], rust.snapshot[k])).collect();
            let game_values = game_trace(&recording, step, &rust.trace)?;
            other_values = game_values.iter().flatten().count();
            let mut trace_differences = Vec::new();
            for (value, game) in rust.trace.iter().zip(&game_values) {
                if let Some(game) = game {
                    if !replay::same_value(value.kind, *game, value.word) {
                        trace_differences.push(format!(
                            "{}: game {} / rustyac.exe {}",
                            value.name,
                            replay::describe(value.kind, *game),
                            replay::describe(value.kind, value.word)
                        ));
                    }
                }
            }
            let tape = compare_tape_calls(&recording, step, &rust.tape);
            calls += recording.steps[step].calls.len();
            if differing.is_empty() && trace_differences.is_empty() && tape.is_ok() {
                exact += 1;
            } else if first.is_none() {
                let text = match (trace_differences.first(), &tape, differing.first()) {
                    (Some(text), _, _) => text.clone(),
                    (None, Err(tape), _) => tape.clone(),
                    (None, Ok(()), Some(&k)) => format!(
                        "{}: game {} / rustyac.exe {}",
                        columns.fields[k].name,
                        replay::describe(columns.fields[k].kind, game[k]),
                        replay::describe(columns.fields[k].kind, rust.snapshot[k])
                    ),
                    (None, Ok(()), None) => unreachable!(),
                };
                first = Some((step, text, differing.len() + trace_differences.len()));
                if verbose {
                    // the whole list of that step: the chassis values by name, then the others
                    for &k in differing.iter().take(40) {
                        println!(
                            "    {}: game {} / rustyac.exe {}",
                            columns.fields[k].name,
                            replay::describe(columns.fields[k].kind, game[k]),
                            replay::describe(columns.fields[k].kind, rust.snapshot[k])
                        );
                    }
                    for text in trace_differences.iter().take(60) {
                        println!("    {text}");
                    }
                    if let Err(tape) = &tape {
                        println!("    tape: {tape}");
                    }
                }
            }
        }
        if reader.next_step()?.is_some() {
            return Err(format!("{name}: the game ran more than {count} steps"));
        }
        let status = child.wait().map_err(|e| e.to_string())?;
        if !status.success() {
            return Err(format!("{name}: rustyac.exe ended with {status}"));
        }
        match &first {
            None => println!("  bit-exact: {} ({:.1} s)", percent(exact, count), started.elapsed().as_secs_f64()),
            Some((step, text, _)) => println!("  bit-exact: {}; first difference at step {step}: {text}", percent(exact, count)),
        }
        failed |= first.is_some();
        writeln!(
            table,
            "| `{name}` | {count} | {} | {} | {} | {other_values} | {calls} |",
            percent(exact, count),
            match &first {
                None => "none".to_string(),
                Some((step, text, differing)) => format!("step {step}: {text} ({differing} values differ in that step)"),
            },
            columns.fields.len()
        )
        .unwrap();
        all_steps += count;
        all_exact += exact;
        all_calls += calls;
    }
    writeln!(table, "| **all** | **{all_steps}** | **{}** | | | | **{all_calls}** |", percent(all_exact, all_steps)).unwrap();
    println!("\n{table}");
    let out = repo.join("oracle/chassis");
    std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
    let suffix = match dir {
        Some(_) => format!("_{folder_name}"),
        None => String::new(),
    };
    let file = out.join(if full { format!("results{suffix}_game_replay.md") } else { format!("partial{suffix}_game_replay.md") });
    std::fs::write(&file, &table).map_err(|e| format!("{}: {e}", file.display()))?;
    println!("{}", file.display());
    if failed {
        return Err("rustyac.exe and the game differ".to_string());
    }
    Ok(())
}

/// Writes one golden excerpt: `count` steps of a recording from `first` on. The Rust chassis
/// runs freely up to `first` (and must agree with the game all the way), its state there is
/// the excerpt's start state; the expected values are the game's.
fn write_excerpt(
    recording: &Recording,
    data: &Path,
    systems: Systems,
    first: usize,
    count: usize,
    path: &Path,
) -> Result<usize, String> {
    let setup = run_setup(recording, systems)?;
    let columns = Columns::new(recording)?;
    let mut chassis = setup.build_runner(data)?;
    // a golden step holds the driver's controls only: the script's own commands (a car put
    // down somewhere, locked controls) can come before the excerpt, not inside it
    let job_in = |step: usize| {
        (recording.has("script.teleport") && recording.i(step, "script.teleport") != 0)
            || (recording.has("script.lockMs")
                && (recording.f(step, "script.lockMs") != 0.0
                    || recording.i(step, "script.setLocked") != 0
                    || recording.i(step, "script.gentleStop") != 0
                    || recording.f(step, "script.addPenalty") != 0.0))
    };
    if let Some(step) = (first..first + count).find(|&step| job_in(step)) {
        return Err(format!("step {step}: the script commands the car there; an excerpt cannot hold that"));
    }
    for step in 0..first {
        let feed = recorded_step(recording, step)?;
        let feed = if systems == Systems::ALL { feed.driver_only() } else { feed };
        apply_jobs(&mut chassis, recording, step);
        chassis.step_recorded(setup.time_of_step(step), &feed);
        let rust = replay::snapshot(&chassis);
        let game = columns.game(recording, step);
        if let Some(k) = (0..rust.len()).find(|&k| !replay::same_value(columns.kinds[k], game[k], rust[k])) {
            return Err(format!("step {step}: {} differs from the game: no excerpt written", columns.fields[k].name));
        }
        let trace = replay::powertrain_trace(&chassis);
        for (value, game) in trace.iter().zip(game_trace(recording, step, &trace)?) {
            if game.is_some_and(|game| !replay::same_value(value.kind, game, value.word)) {
                return Err(format!("step {step}: {} differs from the game: no excerpt written", value.name));
            }
        }
        compare_tape(recording, step, &chassis)?;
    }
    // the values of the ported systems that every recording holds join the hash, in the
    // order of the trace (which the chassis fixes, not the step)
    let mut trace_names: Vec<replay::TraceValue> = replay::powertrain_trace(&chassis).into_iter().filter(|v| !v.extra).collect();
    // on a track: the rays, the lap timer, the place along the AI line
    trace_names.extend(replay::track_trace(&chassis));
    // with collisions: the contact joints, the damage, the collision clocks
    if chassis.collisions_enabled {
        if first != 0 && !chassis.core.contact_joints().is_empty() {
            return Err(format!("step {first}: contact joints are alive; an excerpt has to start at a moment without any"));
        }
        trace_names.extend(replay::collision_trace(&chassis));
    }
    let state = if first == 0 { Vec::new() } else { chassis.save_state() };
    let mut steps = Vec::with_capacity(count);
    for step in first..first + count {
        let feed = recorded_step(recording, step)?;
        let feed = if systems == Systems::ALL { feed.driver_only() } else { feed };
        // the game's tape, in the form the hash takes it
        let tape: Vec<rustyac_physics::car::TapeCall> = recording.steps[step]
            .calls
            .iter()
            .map(|call| rustyac_physics::car::TapeCall {
                body: call.body,
                kind: call.kind,
                source: ForceSource::from_name(recording.system_of(call)).unwrap_or(ForceSource::Other),
                a: if call.kind == 8 { [0.0; 3] } else { call.a },
                b: if call.kind == 8 { [0.0; 3] } else { call.b },
                facc: call.facc,
                tacc: call.tacc,
            })
            .collect();
        let mut kinds = columns.kinds.clone();
        let mut words = columns.game(recording, step);
        for value in &trace_names {
            kinds.push(value.kind);
            words.push(game_trace_word(recording, step, value).ok_or(format!("the recording has no field {}", value.name))?);
        }
        let hash = replay::step_hash(&kinds, &words, &tape);
        // the game's bodies after the step
        let mut bodies = Vec::new();
        for body in &recorded_layout(recording).bodies {
            let col = |name: &str| recording.steps[step].words[recording.col(&format!("{body}.post.{name}"))];
            for name in ["pos.x", "pos.y", "pos.z", "q.w", "q.x", "q.y", "q.z", "lvel.x", "lvel.y", "lvel.z", "avel.x", "avel.y", "avel.z"] {
                bodies.push(col(name));
            }
        }
        steps.push(GoldenStep { feed, hash, bodies });
    }
    // a track excerpt names its track; the spawn is not kept (the start state is)
    let track = setup.track.as_ref().map(|run| replay::GoldenTrack {
        name: recording.get("track").unwrap_or("?").to_string(),
        armed: run.armed,
        allowed_tyres_out: run.allowed_tyres_out,
    });
    let attached = setup.track.as_ref().map(|run| std::sync::Arc::clone(&run.track));
    // nor is the car's collider mesh (it is the game's file)
    let mesh = setup.collide.mesh.clone();
    let mut golden =
        Golden { car: recording.get("car").unwrap_or("?").to_string(), setup, first, state, steps, track, collider_mesh: mesh.is_some() };
    golden.setup.track = None;
    golden.setup.collide.mesh = None;
    if let Some(track) = &attached {
        golden.attach_track(std::sync::Arc::clone(track));
    }
    if let Some(mesh) = &mesh {
        golden.attach_collider_mesh(mesh.clone());
    }
    let bytes = golden.to_bytes();
    // the file must replay: parse it back and run it the way `cargo test` will
    let mut parsed = Golden::parse(&bytes)?;
    if let Some(track) = &attached {
        parsed.attach_track(std::sync::Arc::clone(track));
    }
    if let Some(mesh) = &mesh {
        parsed.attach_collider_mesh(mesh.clone());
    }
    if parsed != golden {
        return Err("the golden file does not read back as written".to_string());
    }
    parsed.check(data)?;
    // and the free run itself must still agree at the end of the excerpt
    for step in first..first + count {
        let feed = recorded_step(recording, step)?;
        let feed = if systems == Systems::ALL { feed.driver_only() } else { feed };
        chassis.step_recorded(golden.setup.time_of_step(step), &feed);
        if body_words(&chassis) != golden.steps[step - first].bodies {
            return Err(format!("step {step}: the free run left the game"));
        }
    }
    std::fs::write(path, &bytes).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(bytes.len())
}

/// `excerpt-track`: the golden files of the car on a real track, from the recordings in
/// `oracle/track` (`car_oracle run --track spa --scenario ...`).
/// `excerpt16`: the golden excerpts of Task 16, one four-wheel-drive car and one ERS car, from
/// the recordings `oracle/awd_sesto/wc_spirited.carrec` (the Sesto Elemento in the slalom: all
/// three differentials at work) and `oracle/hy_ks_ferrari_sf15t/hy_modes.carrec` (the SF15-T
/// from throttle onto the brakes, with an engine-brake setting and the MGU-H mode changed on
/// the way).
fn excerpt16_command() -> Result<(), String> {
    let repo = repo_root();
    let out = repo.join("crates/rustyac-physics/tests/golden");
    for (folder, scenario, name, first, count) in
        [("oracle/awd_sesto", "wc_spirited", "awd_sesto_wc_spirited", 2400usize, 300usize), ("oracle/hy_ks_ferrari_sf15t", "hy_modes", "ers_sf15t_hy_modes", 7700, 400)]
    {
        let recording = Recording::read(&repo.join(format!("{folder}/{scenario}.carrec")))?;
        let data = car_data(&recording)?;
        let path = out.join(format!("{name}_{first}_{count}.chgold"));
        let bytes = write_excerpt(&recording, &data, Systems::ALL, first, count, &path)?;
        println!("{} ({bytes} bytes, steps {first}..{})", path.display(), first + count);
    }
    Ok(())
}

fn excerpt_track_command() -> Result<(), String> {
    let repo = repo_root();
    let out = repo.join("crates/rustyac-physics/tests/golden");
    for scenario in ["spa_kerbs", "spa_launch"] {
        let recording = Recording::read(&repo.join(format!("oracle/track/{scenario}.carrec")))?;
        let data = car_data(&recording)?;
        let steps = recording.steps.len();
        let (first, count) = match scenario {
            // the Bus Stop chicane: over its kerbs and off the track until the lap is cut
            "spa_kerbs" => {
                let cut = (0..steps).find(|&step| recording.i(step, "transponder.cuts") > 0).ok_or("the kerbs recording never cuts the track")?;
                (cut - 180, 300)
            }
            // the start line at speed: the armed first crossing starts the lap timer anew
            _ => {
                let crossing = (0..steps)
                    .find(|&step| recording.i(step, "transponder.status.0.isValid") != 0)
                    .ok_or("the launch recording never crosses the start line")?;
                (crossing - 120, 200)
            }
        };
        let path = out.join(format!("track_{scenario}_{first}_{count}.chgold"));
        let bytes = write_excerpt(&recording, &data, Systems::ALL, first, count, &path)?;
        println!("{} ({bytes} bytes, steps {first}..{})", path.display(), first + count);
    }
    Ok(())
}

/// `excerpt-collide`: the golden files of the car touching things, from the recordings in
/// `oracle/collide` (`car_oracle run --track spa --collide --scenario ...`).
fn excerpt_collide_command() -> Result<(), String> {
    let repo = repo_root();
    let out = repo.join("crates/rustyac-physics/tests/golden");
    // (scenario, steps before the collider mesh first touches something, steps)
    for (scenario, before, count) in [("spa_wall_low", 30usize, 200usize), ("spa_wall_high", 30, 200), ("spa_rollover", 20, 340)] {
        let recording = Recording::read(&repo.join(format!("oracle/collide/{scenario}.carrec")))?;
        let data = car_data(&recording)?;
        let steps = recording.steps.len();
        // the car's collider mesh is geom 2000 among the contact joints a recording lists
        let mesh_contact =
            |step: usize| (0..6).any(|k| recording.i(step, &format!("collide.c{k}.g1")) >= 2000 || recording.i(step, &format!("collide.c{k}.g2")) >= 2000);
        let hit = (0..steps).find(|&step| mesh_contact(step)).ok_or(format!("{scenario}: the collider mesh never touches anything"))?;
        // a little before that, at a moment no contact joint lives into
        let mut first = hit.saturating_sub(before).max(1);
        while first > 1 && recording.i(first - 1, "collide.contactJoints") != 0 {
            first -= 1;
        }
        let path = out.join(format!("collide_{scenario}_{first}_{count}.chgold"));
        let bytes = write_excerpt(&recording, &data, Systems::ALL, first, count, &path)?;
        let with_joints = (first..first + count).filter(|&step| recording.i(step, "collide.contactJoints") != 0).count();
        println!("{} ({bytes} bytes, steps {first}..{}, first mesh contact at {hit}, {with_joints} steps with contact joints)", path.display(), first + count);
    }
    Ok(())
}

fn excerpt_command() -> Result<(), String> {
    let repo = repo_root();
    let out = repo.join("crates/rustyac-physics/tests/golden");
    // the rolling chassis alone (Task 08): brakes, engine and drivetrain are fed
    for (scenario, count) in [("slalom", 300usize), ("kerb", 300)] {
        let recording = Recording::read(&repo.join(format!("oracle/car/{scenario}.carrec")))?;
        let data = car_data(&recording)?;
        let first = match scenario {
            // full steering swing at 100 km/h
            "slalom" => 3000,
            // from just before the left wheels climb the kerb strip
            _ => {
                let on_kerb = (0..recording.steps.len())
                    .find(|&step| recording.f(step, "tyre.lf.in_ground_y") != 0.0)
                    .ok_or("the kerb recording never reaches the kerb")?;
                on_kerb - 40
            }
        };
        let path = out.join(format!("chassis_{scenario}_{first}_{count}.chgold"));
        let bytes = write_excerpt(&recording, &data, Systems::CHASSIS, first, count, &path)?;
        println!("{} ({bytes} bytes, steps {first}..{})", path.display(), first + count);
    }
    // brakes, engine and drivetrain computed in Rust (Task 09)
    for (scenario, count) in [("launch_autoclutch_off", 460usize), ("brake", 400)] {
        let recording = Recording::read(&repo.join(format!("oracle/car/{scenario}.carrec")))?;
        let data = car_data(&recording)?;
        let first = match scenario {
            // from a little before the clutch comes up: the standing start, first gear's
            // wheelspin on the limiter and the traction control cutting in above 40 km/h
            "launch_autoclutch_off" => {
                let moving = (0..recording.steps.len())
                    .find(|&step| recording.f(step, "script.clutch") > 0.0 && recording.i(step, "drivetrain.currentGear") == 2)
                    .ok_or("the launch recording never lets the clutch up")?;
                moving - 20
            }
            // from a little before the brake pedal goes down at 250 km/h: full braking and
            // the first down-shifts with their clutch profile and throttle blips
            _ => {
                let braking = (0..recording.steps.len())
                    .find(|&step| recording.f(step, "script.brake") > 0.0)
                    .ok_or("the brake recording never brakes")?;
                braking - 40
            }
        };
        let path = out.join(format!("whole_{scenario}_{first}_{count}.chgold"));
        let bytes = write_excerpt(&recording, &data, Systems::ALL, first, count, &path)?;
        println!("{} ({bytes} bytes, steps {first}..{})", path.display(), first + count);
    }
    // a second car, the 488 GT3: full braking with the ABS at work (Task 10); the whole car,
    // driver's controls only
    {
        let recording = Recording::read(&repo.join("oracle/car_wc_488/wc_stops.carrec"))?;
        let data = car_data(&recording)?;
        let releasing = (0..recording.steps.len())
            .find(|&step| recording.f(step, "abs.override.lf") == 0.0)
            .ok_or("the 488's recording never has the ABS release a brake")?;
        let (first, count) = (releasing - 60, 400);
        let path = out.join(format!("whole_488_gt3_wc_stops_{first}_{count}.chgold"));
        let bytes = write_excerpt(&recording, &data, Systems::ALL, first, count, &path)?;
        println!("{} ({bytes} bytes, steps {first}..{})", path.display(), first + count);
    }
    Ok(())
}

/// Runs one scenario once per deliberate fault and reports where the comparison notices.
/// Deliberate faults in the systems of Task 16, each with the kind of car it is for: `awd`
/// (three differentials), `awd2` (the coupling), `kers`, `ers`, `front` (an ERS with front
/// motors). The battery's rates are changed by a small part, not by one bit: one bit of a
/// rate is lost when the step's share is added to a charge near 1.
const TASK16_FAULTS: [(&str, &str, &str, Fault); 16] = [
    ("awd", "awd_front_share", "the front axle's torque share one bit up", |c| {
        let base = c.drivetrain.as_mut().unwrap().base_mut();
        base.awd_front_share = nudge(base.awd_front_share);
    }),
    ("awd", "awd_rear_coast", "the rear differential's coast lock one bit up", |c| {
        let base = c.drivetrain.as_mut().unwrap().base_mut();
        base.awd_rear_diff.coast = nudge(base.awd_rear_diff.coast);
    }),
    ("awd", "awd_front_power", "the front differential's power lock one bit up", |c| {
        let base = c.drivetrain.as_mut().unwrap().base_mut();
        base.awd_front_diff.power = nudge(base.awd_front_diff.power);
    }),
    ("awd", "awd_centre_preload", "the centre differential's preload one bit up", |c| {
        let base = c.drivetrain.as_mut().unwrap().base_mut();
        base.awd_center_diff.preload = nudge(base.awd_center_diff.preload);
    }),
    ("awd", "awd_front_inertia", "the left front shaft's inertia one (double-precision) bit up", |c| {
        let base = c.drivetrain.as_mut().unwrap().base_mut();
        base.out_shaft_lf.inertia = f64::from_bits(base.out_shaft_lf.inertia.to_bits() + 1);
    }),
    ("awd2", "awd2_ramp", "the coupling's ramp one (double-precision) bit up", |c| {
        let base = c.drivetrain.as_mut().unwrap().base_mut();
        base.awd2.ramp = f64::from_bits(base.awd2.ramp.to_bits() + 1);
    }),
    ("awd2", "awd2_front_inertia", "the right front shaft's inertia one (double-precision) bit up", |c| {
        let base = c.drivetrain.as_mut().unwrap().base_mut();
        base.out_shaft_rf.inertia = f64::from_bits(base.out_shaft_rf.inertia.to_bits() + 1);
    }),
    ("kers", "kers_discharge", "the battery's discharge rate 0.01 % up", |c| {
        let kers = c.kers.as_mut().unwrap();
        kers.discharge_k *= 1.0001;
    }),
    ("kers", "kers_charge", "the battery's filling rate 0.01 % up", |c| {
        let kers = c.kers.as_mut().unwrap();
        kers.charge_k *= 1.0001;
    }),
    ("kers", "kers_brake_level", "the brake torque for the full filling rate 0.01 % up", |c| {
        let kers = c.kers.as_mut().unwrap();
        kers.brake_for_max_charge *= 1.0001;
    }),
    ("ers", "ers_discharge", "the battery's discharge rate one part in a million up", |c| {
        let ers = c.ers.as_mut().unwrap();
        ers.discharge_k *= 1.000001;
    }),
    ("ers", "ers_charge", "the kinetic recovery's rate one part in a million up", |c| {
        let ers = c.ers.as_mut().unwrap();
        ers.charge_k *= 1.000001;
    }),
    ("ers", "ers_heat_torque", "the MGU-H's share of the torque one bit up", |c| {
        let ers = c.ers.as_mut().unwrap();
        ers.heat_torque = nudge(ers.heat_torque);
    }),
    ("ers", "ers_rear_correction", "the rear brake correction one bit up", |c| {
        let ers = c.ers.as_mut().unwrap();
        ers.rear_correction_torque = nudge(ers.rear_correction_torque);
    }),
    ("ers", "ers_start_recovery", "the recovery level at the start one bit up", |c| {
        let ers = c.ers.as_mut().unwrap();
        ers.kinetic_recovery = nudge(ers.kinetic_recovery);
    }),
    ("front", "ers_front_discharge", "the front motors' discharge rate one part in a million up", |c| {
        let ers = c.ers.as_mut().unwrap();
        ers.discharge_k_front *= 1.000001;
    }),
];

/// `faults16 <scenario> --dir <folder>`: the faults of [`TASK16_FAULTS`] that fit the
/// recording's car, each of which the comparison must notice.
fn faults16_command(names: &[String], dir: Option<&Path>) -> Result<(), String> {
    let repo = repo_root();
    let scenario = names.first().map(String::as_str).ok_or("faults16 <scenario> --dir <folder>")?;
    let dir = dir.ok_or("faults16 needs --dir <folder of recordings>")?;
    let folder = if dir.is_absolute() { dir.to_path_buf() } else { repo.join(dir) };
    let recording = Recording::read(&folder.join(format!("{scenario}.carrec")))?;
    let data = car_data(&recording)?;
    let traction = if recording.has("drivetrain.tractionType") { recording.i(0, "drivetrain.tractionType") } else { 0 };
    let fits = |kind: &str| match kind {
        "awd" => traction == 2,
        "awd2" => traction == 3,
        "kers" => recording.has("kers.charge"),
        "ers" => recording.has("ers.charge"),
        // the front motors' map has stages
        "front" => recording.has("ers.controllerFront.stages") && recording.i(0, "ers.controllerFront.stages") > 0,
        _ => false,
    };
    let clean = compare(&recording, &data, Systems::ALL, false, None, None)?;
    if clean.first.is_some() {
        return Err("the run without a fault already differs".to_string());
    }
    let mut table = String::new();
    writeln!(table, "Scenario `{scenario}` of the car `{}`, {} steps. Without a fault: no difference.\n", recording.get("car").unwrap_or("?"), recording.steps.len()).unwrap();
    writeln!(table, "| Fault | What is changed | Bit-exact steps | Noticed at | First value that differs |").unwrap();
    writeln!(table, "|---|---|---|---|---|").unwrap();
    let mut missed = Vec::new();
    for &(kind, name, about, fault) in TASK16_FAULTS.iter().filter(|(kind, ..)| fits(kind)) {
        let _ = kind;
        let outcome = compare(&recording, &data, Systems::ALL, false, None, Some(fault))?;
        let (at, text) = match &outcome.first {
            Some((step, text)) => (format!("step {step}"), text.clone()),
            None => {
                missed.push(name);
                ("never".to_string(), "-".to_string())
            }
        };
        println!("{name}: {at}: {text}");
        writeln!(table, "| `{name}` | {about} | {} | {at} | {text} |", percent(outcome.exact, outcome.steps)).unwrap();
    }
    let out = repo.join("oracle/chassis");
    std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
    let file = out.join(format!("faults16_{}_{scenario}.md", dir.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()));
    std::fs::write(&file, &table).map_err(|e| format!("{}: {e}", file.display()))?;
    println!("\n{table}\n{}", file.display());
    if !missed.is_empty() {
        return Err(format!("faults the comparison did not notice: {}", missed.join(", ")));
    }
    Ok(())
}

fn faults_command(names: &[String], dir: Option<&Path>, systems: Systems) -> Result<(), String> {
    let repo = repo_root();
    let scenario = names.first().map(String::as_str).unwrap_or("slalom");
    let folder = match dir {
        Some(dir) if dir.is_absolute() => dir.to_path_buf(),
        Some(dir) => repo.join(dir),
        None => repo.join("oracle/car"),
    };
    let recording = Recording::read(&folder.join(format!("{scenario}.carrec")))?;
    let data = car_data(&recording)?;
    let mut table = String::new();
    writeln!(
        table,
        "Scenario `{scenario}` of the car `{}`, {} steps. Without a fault: no difference.\n",
        recording.get("car").unwrap_or("?"),
        recording.steps.len()
    )
    .unwrap();
    writeln!(table, "| Fault | What is changed | Bit-exact steps | Noticed at | First value that differs |").unwrap();
    writeln!(table, "|---|---|---|---|---|").unwrap();
    let clean = compare(&recording, &data, systems, false, None, None)?;
    if clean.first.is_some() {
        return Err("the run without a fault already differs".to_string());
    }
    let mut missed = Vec::new();
    let chassis_faults = FAULTS.iter().filter(|_| systems == Systems::CHASSIS);
    let powertrain_faults = POWERTRAIN_FAULTS.iter().filter(|_| systems.brakes && systems.drivetrain);
    let whole_faults = WHOLE_FAULTS.iter().filter(|_| systems == Systems::ALL);
    for &(name, about, fault) in chassis_faults.chain(powertrain_faults).chain(whole_faults) {
        let outcome = compare(&recording, &data, systems, false, None, Some(fault))?;
        let (at, text) = match &outcome.first {
            Some((step, text)) => (format!("step {step}"), text.clone()),
            None => {
                missed.push(name);
                ("never".to_string(), "-".to_string())
            }
        };
        println!("{name}: {at}: {text}");
        writeln!(table, "| `{name}` | {about} | {} | {at} | {text} |", percent(outcome.exact, outcome.steps)).unwrap();
    }
    let out = repo.join("oracle/chassis");
    std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
    let tail = systems.suffix();
    let file = out.join(match dir {
        Some(dir) => format!("faults_{}{tail}.md", dir.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()),
        None => format!("faults{tail}.md"),
    });
    std::fs::write(&file, &table).map_err(|e| format!("{}: {e}", file.display()))?;
    println!("\n{table}\n{}", file.display());
    if !missed.is_empty() {
        return Err(format!("faults the comparison did not notice: {}", missed.join(", ")));
    }
    Ok(())
}

/// The test car of `test-car`: every change to the F2004's files, as (file, section, key, value).
const TIGHT_STOPS: [(&str, &str, &str, &str); 18] = [
    // wheels: bump stops compare the hub height (travel minus rod length), packers the travel;
    // a negative BUMPSTOP_DN puts the lower stop above the design position
    ("suspensions.ini", "FRONT", "BUMPSTOP_UP", "0.028"),
    ("suspensions.ini", "FRONT", "BUMPSTOP_DN", "-0.016"),
    ("suspensions.ini", "FRONT", "PACKER_RANGE", "0.020"),
    ("suspensions.ini", "FRONT", "BUMP_STOP_PROGRESSIVE", "2000000"),
    ("suspensions.ini", "REAR", "BUMPSTOP_UP", "0.068"),
    ("suspensions.ini", "REAR", "BUMPSTOP_DN", "-0.052"),
    ("suspensions.ini", "REAR", "PACKER_RANGE", "0.030"),
    ("suspensions.ini", "REAR", "BUMP_STOP_PROGRESSIVE", "1000000"),
    // heave springs
    ("suspensions.ini", "HEAVE_FRONT", "BUMPSTOP_UP", "0.027"),
    ("suspensions.ini", "HEAVE_FRONT", "BUMPSTOP_DN", "-0.017"),
    ("suspensions.ini", "HEAVE_FRONT", "PACKER_RANGE", "0.012"),
    ("suspensions.ini", "HEAVE_REAR", "BUMPSTOP_UP", "0.067"),
    ("suspensions.ini", "HEAVE_REAR", "BUMPSTOP_DN", "-0.051"),
    ("suspensions.ini", "HEAVE_REAR", "PACKER_RANGE", "0.025"),
    // the setup screen would push the packers back into its own range
    ("setup.ini", "PACKER_RANGE_LF", "MIN", "5"),
    ("setup.ini", "PACKER_RANGE_RF", "MIN", "5"),
    ("setup.ini", "PACKER_RANGE_LR", "MIN", "5"),
    ("setup.ini", "PACKER_RANGE_RR", "MIN", "5"),
];

/// Sets `key=value` inside `[section]` of an ini text: an existing line keeps its comment, a
/// missing key is added right after the section header.
fn patch_ini(text: &str, section: &str, key: &str, value: &str) -> Result<String, String> {
    let header = format!("[{section}]");
    let mut out = Vec::new();
    let mut inside = false;
    let mut found_section = false;
    let mut done = false;
    let mut insert_at = 0;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            inside = trimmed == header;
            if inside {
                found_section = true;
                insert_at = out.len() + 1;
            }
        } else if inside && !done && trimmed.split('=').next().map(str::trim) == Some(key) {
            let comment = line.find(';').map(|at| format!("\t\t\t\t{}", &line[at..])).unwrap_or_default();
            out.push(format!("{key}={value}{comment}"));
            done = true;
            continue;
        }
        out.push(line.to_string());
    }
    if !found_section {
        // a section the car does not have: appended
        out.push(String::new());
        out.push(header);
        out.push(format!("{key}={value}"));
        done = true;
    }
    if !done {
        out.insert(insert_at, format!("{key}={value}"));
    }
    Ok(out.join("\r\n") + "\r\n")
}

/// The second test car: values that make the loaders take their fall-backs, and two changes
/// that reach branches of the steering and force-feedback code.
const FALLBACKS: [(&str, &str, &str, &str); 16] = [
    // no rim offset is applied below version 2
    ("suspensions.ini", "HEADER", "VERSION", "1"),
    // a hub mass of 0 becomes 20 kg
    ("suspensions.ini", "FRONT", "HUB_MASS", "0"),
    // a fast damper rate of 0 becomes the slow rate, a threshold of 0 becomes 0.2 m/s
    ("suspensions.ini", "FRONT", "DAMP_FAST_REBOUND", "0"),
    ("suspensions.ini", "FRONT", "DAMP_FAST_BUMPTHRESHOLD", "0"),
    ("suspensions.ini", "REAR", "DAMP_FAST_BUMP", "0"),
    ("suspensions.ini", "REAR", "DAMP_FAST_REBOUNDTHRESHOLD", "0"),
    // a bump-stop rate of 0 becomes 500,000 N/m (and the setup screen then clamps it)
    ("suspensions.ini", "REAR", "BUMP_STOP_RATE", "0"),
    // a rear toe off the setup screen's grid: the setup change reseats the rear steering rods
    ("suspensions.ini", "REAR", "TOE_OUT", "-0.00006"),
    // the heave springs' fall-backs
    ("suspensions.ini", "HEAVE_FRONT", "DAMP_FAST_BUMP", "0"),
    ("suspensions.ini", "HEAVE_FRONT", "DAMP_FAST_BUMPTHRESHOLD", "0"),
    ("suspensions.ini", "HEAVE_REAR", "DAMP_FAST_REBOUND", "0"),
    ("suspensions.ini", "HEAVE_REAR", "BUMP_STOP_RATE", "0"),
    // steer assist 1 takes the linear force-feedback path, a rod ratio of 0 becomes 0.003
    ("car.ini", "CONTROLS", "STEER_ASSIST", "1"),
    ("car.ini", "CONTROLS", "LINEAR_STEER_ROD_RATIO", "0"),
    // a start fuel of 0 becomes 30 litres; a fuel density from [FUEL_EXT]
    ("car.ini", "FUEL", "FUEL", "0"),
    ("car.ini", "FUEL_EXT", "KG_PER_LITER", "0.76"),
];

/// Third test car (Task 09), `f2004_pt_street`: the F2004 with what road cars have and it
/// has not. Two turbos (one with a wastegate and a cockpit boost level below 1), a second
/// throttle map, an engine-brake setting, a non-linear coast curve, camshaft overlap, a
/// blow-off threshold, engine damage that accrues; a handbrake, disc temperatures with fade,
/// the load-based electronic brake balance; an H-shifter gearbox (so no down-shift
/// protection), gearbox wear, a spool differential, a clutch profile for up-shifts, and an
/// automatic blip that is the driver's aid instead of the car's.
const PT_STREET: [(&str, &str, &str, &str); 50] = [
    ("engine.ini", "ENGINE_DATA", "LIMITER_HZ", "30"),
    ("engine.ini", "ENGINE_DATA", "MINIMUM", "3500"),
    ("engine.ini", "ENGINE_DATA", "DEFAULT_TURBO_ADJUSTMENT", "0.7"),
    ("engine.ini", "COAST_REF", "NON_LINEARITY", "0.25"),
    ("engine.ini", "TURBO_0", "LAG_DN", "0.985"),
    ("engine.ini", "TURBO_0", "LAG_UP", "0.992"),
    ("engine.ini", "TURBO_0", "MAX_BOOST", "0.5"),
    ("engine.ini", "TURBO_0", "WASTEGATE", "0.42"),
    ("engine.ini", "TURBO_0", "REFERENCE_RPM", "9000"),
    ("engine.ini", "TURBO_0", "GAMMA", "2"),
    ("engine.ini", "TURBO_0", "COCKPIT_ADJUSTABLE", "1"),
    ("engine.ini", "TURBO_1", "LAG_DN", "0.97"),
    ("engine.ini", "TURBO_1", "LAG_UP", "0.98"),
    ("engine.ini", "TURBO_1", "MAX_BOOST", "0.3"),
    ("engine.ini", "TURBO_1", "WASTEGATE", "0"),
    ("engine.ini", "TURBO_1", "REFERENCE_RPM", "12000"),
    ("engine.ini", "TURBO_1", "GAMMA", "0.8"),
    ("engine.ini", "TURBO_1", "COCKPIT_ADJUSTABLE", "0"),
    ("engine.ini", "DAMAGE", "TURBO_BOOST_THRESHOLD", "0.3"),
    ("engine.ini", "DAMAGE", "TURBO_DAMAGE_K", "5"),
    ("engine.ini", "DAMAGE", "RPM_THRESHOLD", "17500"),
    ("engine.ini", "DAMAGE", "RPM_DAMAGE_K", "0.02"),
    ("engine.ini", "BOV", "PRESSURE_THRESHOLD", "0.1"),
    ("engine.ini", "OVERLAP", "FREQUENCY", "0.02"),
    ("engine.ini", "OVERLAP", "GAIN", "0.004"),
    ("engine.ini", "OVERLAP", "IDEAL_RPM", "9000"),
    ("engine.ini", "THROTTLE_RESPONSE", "RPM_REFERENCE", "12000"),
    ("engine.ini", "THROTTLE_RESPONSE", "LUT", "(|0=0|50=70|100=100|)"),
    ("engine.ini", "COAST_SETTINGS", "LUT", "(|0=0|1=0.05|2=0.12|)"),
    ("engine.ini", "COAST_SETTINGS", "DEFAULT", "2"),
    ("engine.ini", "COAST_SETTINGS", "ACTIVATION_RPM", "3000"),
    ("brakes.ini", "DATA", "HANDBRAKE_TORQUE", "1800"),
    ("brakes.ini", "TEMPS_FRONT", "TORQUE_K", "0.3"),
    ("brakes.ini", "TEMPS_FRONT", "PERF_CURVE", "(|0=0.7|300=0.95|500=1.0|800=0.8|)"),
    ("brakes.ini", "TEMPS_FRONT", "COOL_TRANSFER", "0.02"),
    ("brakes.ini", "TEMPS_FRONT", "COOL_SPEED_FACTOR", "0.004"),
    ("brakes.ini", "TEMPS_REAR", "TORQUE_K", "0.2"),
    ("brakes.ini", "TEMPS_REAR", "PERF_CURVE", "(|0=0.75|250=1.0|700=0.85|)"),
    ("brakes.ini", "TEMPS_REAR", "COOL_TRANSFER", "0.015"),
    ("brakes.ini", "TEMPS_REAR", "COOL_SPEED_FACTOR", "0.003"),
    ("brakes.ini", "EBB", "FRONT_SHARE_MULTIPLIER", "1.25"),
    ("drivetrain.ini", "GEARBOX", "SUPPORTS_SHIFTER", "1"),
    ("drivetrain.ini", "DAMAGE", "RPM_WINDOW_K", "100"),
    ("drivetrain.ini", "DIFFERENTIAL", "POWER", "1.0"),
    ("drivetrain.ini", "DIFFERENTIAL", "COAST", "1.0"),
    ("drivetrain.ini", "AUTOCLUTCH", "UPSHIFT_PROFILE", "UPSHIFT_PROFILE"),
    ("drivetrain.ini", "UPSHIFT_PROFILE", "POINT_0", "10"),
    ("drivetrain.ini", "UPSHIFT_PROFILE", "POINT_1", "40"),
    ("drivetrain.ini", "UPSHIFT_PROFILE", "POINT_2", "80"),
    ("drivetrain.ini", "AUTOBLIP", "ELECTRONIC", "0"),
];

/// Fourth test car, `f2004_pt_ctrl`: every `DynamicController` the ported systems can have
/// (brake balance, steer-brake, differential lock, turbo boost, wastegate, both anti-roll
/// bars), with stages that
/// between them use every input, both combinators, filters, limits, an unsorted table, an
/// unknown input and an unknown combinator. Its engine wears fast and dies on the way.
const PT_CTRL: [(&str, &str, &str, &str); 11] = [
    ("engine.ini", "TURBO_0", "LAG_DN", "0.98"),
    ("engine.ini", "TURBO_0", "LAG_UP", "0.99"),
    ("engine.ini", "TURBO_0", "MAX_BOOST", "0.4"),
    ("engine.ini", "TURBO_0", "WASTEGATE", "0.3"),
    ("engine.ini", "TURBO_0", "REFERENCE_RPM", "8000"),
    ("engine.ini", "TURBO_0", "GAMMA", "1"),
    ("engine.ini", "TURBO_0", "COCKPIT_ADJUSTABLE", "0"),
    ("engine.ini", "DAMAGE", "RPM_THRESHOLD", "17000"),
    ("engine.ini", "DAMAGE", "RPM_DAMAGE_K", "0.25"),
    ("drivetrain.ini", "DAMAGE", "RPM_WINDOW_K", "100"),
    ("brakes.ini", "DATA", "HANDBRAKE_TORQUE", "900"),
];

const PT_CTRL_FILES: [(&str, &str); 7] = [
    (
        "ctrl_arb_front.ini",
        "[CONTROLLER_0]
INPUT=SPEED_KMH
COMBINATOR=ADD
LUT=(|0=30000|100=40000|300=60000|)
FILTER=0.9
UP_LIMIT=0
DOWN_LIMIT=0
",
    ),
    (
        "ctrl_arb_rear.ini",
        "[CONTROLLER_0]
INPUT=LATG
COMBINATOR=ADD
LUT=(|-3=25000|0=15000|3=25000|)
FILTER=0.5
UP_LIMIT=24000
DOWN_LIMIT=1000
",
    ),
    (
        "ctrl_turbo0.ini",
        "[CONTROLLER_0]\r\nINPUT=RPMS\r\nCOMBINATOR=ADD\r\nLUT=(|0=0.1|8000=0.3|16000=0.45|)\r\nFILTER=0.9\r\nUP_LIMIT=0.5\r\nDOWN_LIMIT=0.05\r\n\r\n\
         [CONTROLLER_1]\r\nINPUT=GEAR\r\nCOMBINATOR=MULT\r\nLUT=(|0=1.0|1=0.6|3=1.0|7=1.1|)\r\nFILTER=0\r\nUP_LIMIT=0\r\nDOWN_LIMIT=0\r\n",
    ),
    (
        "ctrl_wastegate0.ini",
        "[CONTROLLER_0]\r\nINPUT=CONST\r\nCONST_VALUE=0.25\r\nCOMBINATOR=ADD\r\nFILTER=0\r\nUP_LIMIT=0\r\nDOWN_LIMIT=0\r\n\r\n\
         [CONTROLLER_1]\r\nINPUT=GAS\r\nCOMBINATOR=ADD\r\nLUT=(|0=0|1=0.1|)\r\nFILTER=0.5\r\nUP_LIMIT=0.4\r\nDOWN_LIMIT=0.1\r\n\r\n\
         [CONTROLLER_2]\r\nINPUT=SPEED_KMH\r\nCOMBINATOR=MULT\r\nLUT=(|0=1|300=1.2|)\r\nFILTER=0.99\r\nUP_LIMIT=0\r\nDOWN_LIMIT=0\r\n",
    ),
    (
        "ctrl_ebb.ini",
        "[CONTROLLER_0]\r\nINPUT=LOAD_SPREAD_LF\r\nCOMBINATOR=ADD\r\nLUT=(|0=0.62|0.3=0.58|0.1=0.56|0.5=0.55|0.1=0.56|0.3=0.58|1=0.62|)\r\nFILTER=0.95\r\nUP_LIMIT=1\r\nDOWN_LIMIT=0.0\r\n\r\n\
         [CONTROLLER_1]\r\nINPUT=OVERSTEER_FACTOR\r\nCOMBINATOR=MULT\r\nLUT=(|0=1|0.2=1|0.4=1.1|)\r\nFILTER=0.95\r\nUP_LIMIT=0.75\r\nDOWN_LIMIT=0.0\r\n\r\n\
         [CONTROLLER_2]\r\nINPUT=BRAKE\r\nCOMBINATOR=ADD\r\nLUT=(|0=0|1=0.03|)\r\nFILTER=0.3\r\nUP_LIMIT=0\r\nDOWN_LIMIT=0\r\n\r\n\
         [CONTROLLER_3]\r\nINPUT=LONG\r\nCOMBINATOR=ADD\r\nLUT=(|-4=0.02|0=0|)\r\nFILTER=0.8\r\nUP_LIMIT=0\r\nDOWN_LIMIT=0\r\n\r\n\
         [CONTROLLER_4]\r\nINPUT=LOAD_SPREAD_RF\r\nCOMBINATOR=MULT\r\nLUT=(|0=0.98|1=1.02|)\r\nFILTER=0.9\r\nUP_LIMIT=0\r\nDOWN_LIMIT=0\r\n",
    ),
    (
        "steer_brake_controller.ini",
        "[CONTROLLER_0]\r\nINPUT=STEER\r\nCOMBINATOR=ADD\r\nLUT=(|-1=-600|0=0|1=600|)\r\nFILTER=0.8\r\nUP_LIMIT=0\r\nDOWN_LIMIT=0\r\n\r\n\
         [CONTROLLER_1]\r\nINPUT=SPEED_KMH\r\nCOMBINATOR=MULT\r\nLUT=(|0=0|40=1|300=1|)\r\nFILTER=0\r\nUP_LIMIT=0\r\nDOWN_LIMIT=0\r\n\r\n\
         [CONTROLLER_2]\r\nINPUT=LATG\r\nCOMBINATOR=ADD\r\nLUT=(|-3=-50|3=50|)\r\nFILTER=0.9\r\nUP_LIMIT=0\r\nDOWN_LIMIT=0\r\n\r\n\
         [CONTROLLER_3]\r\nINPUT=STEER_DEG\r\nCOMBINATOR=ADD\r\nLUT=(|-20=-20|20=20|)\r\nFILTER=0\r\nUP_LIMIT=0\r\nDOWN_LIMIT=0\r\n\r\n\
         [CONTROLLER_4]\r\nINPUT=WHEEL_STEER_DEG\r\nCOMBINATOR=ADD\r\nLUT=(|-2=-10|2=10|)\r\nFILTER=0.5\r\nUP_LIMIT=400\r\nDOWN_LIMIT=-400\r\n",
    ),
    (
        "ctrl_single_lock.ini",
        "[CONTROLLER_0]\r\nINPUT=CONST\r\nCONST_VALUE=7\r\nCOMBINATOR=NOPE\r\nFILTER=0\r\nUP_LIMIT=0\r\nDOWN_LIMIT=0\r\n\r\n\
         [CONTROLLER_1]\r\nINPUT=GAS\r\nCOMBINATOR=ADD\r\nLUT=(|0=20|1=120|)\r\nFILTER=0.9\r\nUP_LIMIT=0\r\nDOWN_LIMIT=0\r\n\r\n\
         [CONTROLLER_2]\r\nINPUT=SLIPRATIO_MAX\r\nCOMBINATOR=ADD\r\nLUT=(|0=0|0.3=60|)\r\nFILTER=0.7\r\nUP_LIMIT=0\r\nDOWN_LIMIT=0\r\n\r\n\
         [CONTROLLER_3]\r\nINPUT=SLIPRATIO_AVG\r\nCOMBINATOR=ADD\r\nLUT=(|-0.3=20|0=0|0.3=10|)\r\nFILTER=0.7\r\nUP_LIMIT=0\r\nDOWN_LIMIT=0\r\n\r\n\
         [CONTROLLER_4]\r\nINPUT=REAR_SPEED_RATIO\r\nCOMBINATOR=MULT\r\nLUT=(|0=1|1=1|1.2=1.5|)\r\nFILTER=0.5\r\nUP_LIMIT=0\r\nDOWN_LIMIT=0\r\n\r\n\
         [CONTROLLER_5]\r\nINPUT=SLIPANGLE_FRONT_AVG\r\nCOMBINATOR=ADD\r\nLUT=(|-10=5|0=0|10=5|)\r\nFILTER=0.6\r\nUP_LIMIT=0\r\nDOWN_LIMIT=0\r\n\r\n\
         [CONTROLLER_6]\r\nINPUT=SLIPANGLE_REAR_AVG\r\nCOMBINATOR=ADD\r\nLUT=(|-10=4|0=0|10=4|)\r\nFILTER=0.6\r\nUP_LIMIT=0\r\nDOWN_LIMIT=0\r\n\r\n\
         [CONTROLLER_7]\r\nINPUT=SLIPANGLE_FRONT_MAX\r\nCOMBINATOR=ADD\r\nLUT=(|0=0|10=3|)\r\nFILTER=0.6\r\nUP_LIMIT=0\r\nDOWN_LIMIT=0\r\n\r\n\
         [CONTROLLER_8]\r\nINPUT=SLIPANGLE_REAR_MAX\r\nCOMBINATOR=ADD\r\nLUT=(|0=0|10=6|)\r\nFILTER=0.6\r\nUP_LIMIT=0\r\nDOWN_LIMIT=0\r\n\r\n\
         [CONTROLLER_9]\r\nINPUT=AVG_TRAVEL_REAR\r\nCOMBINATOR=ADD\r\nLUT=(|0=0|60=2|)\r\nFILTER=0.4\r\nUP_LIMIT=0\r\nDOWN_LIMIT=0\r\n\r\n\
         [CONTROLLER_10]\r\nINPUT=SUS_TRAVEL_LR\r\nCOMBINATOR=ADD\r\nLUT=(|0=0|60=1|)\r\nFILTER=0.4\r\nUP_LIMIT=0\r\nDOWN_LIMIT=0\r\n\r\n\
         [CONTROLLER_11]\r\nINPUT=SUS_TRAVEL_RR\r\nCOMBINATOR=ADD\r\nLUT=(|0=0|60=1.5|)\r\nFILTER=0.4\r\nUP_LIMIT=0\r\nDOWN_LIMIT=0\r\n\r\n\
         [CONTROLLER_12]\r\nINPUT=NO_SUCH_INPUT\r\nCOMBINATOR=ADD\r\nLUT=(|0=3|1=40|)\r\nFILTER=0\r\nUP_LIMIT=300\r\nDOWN_LIMIT=5\r\n",
    ),
];

/// Fifth test car, `f2004_pt_fwd`: front-wheel drive with an open differential, and values
/// that make the loaders of drivetrain, engine and brakes take their fall-backs (shift times,
/// clutch torque, shift window, idle speed, limiter rate, no coast reference,
/// automatic-clutch speeds, automatic-gearbox shift points), a forced automatic clutch, no
/// down-shift protection, disc temperatures for the rear discs only (which leaves the front
/// brakes without torque), no cockpit brake bias, and a tank that runs dry.
const PT_FWD: [(&str, &str, &str, &str); 29] = [
    ("drivetrain.ini", "TRACTION", "TYPE", "FWD"),
    ("drivetrain.ini", "DIFFERENTIAL", "POWER", "0"),
    ("drivetrain.ini", "DIFFERENTIAL", "COAST", "0"),
    ("drivetrain.ini", "DIFFERENTIAL", "PRELOAD", "0"),
    ("drivetrain.ini", "GEARBOX", "CHANGE_UP_TIME", "0"),
    ("drivetrain.ini", "GEARBOX", "CHANGE_DN_TIME", "0"),
    ("drivetrain.ini", "GEARBOX", "AUTO_CUTOFF_TIME", "0"),
    ("drivetrain.ini", "GEARBOX", "VALID_SHIFT_RPM_WINDOW", "0"),
    ("drivetrain.ini", "CLUTCH", "MAX_TORQUE", "0"),
    ("drivetrain.ini", "AUTOCLUTCH", "FORCED_ON", "1"),
    ("drivetrain.ini", "AUTOCLUTCH", "MIN_RPM", "0"),
    ("drivetrain.ini", "DOWNSHIFT_PROTECTION", "ACTIVE", "0"),
    ("drivetrain.ini", "AUTO_SHIFTER", "UP", "0"),
    ("drivetrain.ini", "AUTO_SHIFTER", "DOWN", "0"),
    // the setup screen would push an open differential back into its own range
    ("setup.ini", "DIFF_POWER", "MIN", "0"),
    ("setup.ini", "DIFF_COAST", "MIN", "0"),
    ("setup.ini", "DIFF_PRELOAD", "MIN", "0"),
    ("engine.ini", "ENGINE_DATA", "MINIMUM", "0"),
    ("engine.ini", "ENGINE_DATA", "LIMITER_HZ", "0"),
    ("engine.ini", "HEADER", "COAST_CURVE", "NONE"),
    ("engine.ini", "DAMAGE", "RPM_THRESHOLD", "0"),
    ("brakes.ini", "DATA", "COCKPIT_ADJUSTABLE", "0"),
    ("brakes.ini", "DATA", "FRONT_SHARE", "0.7"),
    ("brakes.ini", "DATA", "HANDBRAKE_TORQUE", "600"),
    ("brakes.ini", "TEMPS_REAR", "TORQUE_K", "0.25"),
    ("brakes.ini", "TEMPS_REAR", "PERF_CURVE", "(|0=0.8|400=1.0|)"),
    ("brakes.ini", "TEMPS_REAR", "COOL_TRANSFER", "0.01"),
    ("brakes.ini", "TEMPS_REAR", "COOL_SPEED_FACTOR", "0.002"),
    // enough for about five seconds of driving
    ("car.ini", "FUEL", "FUEL", "0.13"),
];

/// The test car of the whole-car port (Task 10): the F2004 with the aids and aero options no
/// drive of a shipped car reaches. Two-channel ABS (`[ABS_V2]`), an electronic differential
/// lock, traction control that checks every step, a DRS that opens the front wing by a factor
/// and the rear wing by a fixed angle and closes above 0.4 g sideways, and wing controllers on
/// the inputs the F2004's own do not use (brake, lateral and longitudinal g, rear suspension
/// travel), one naming a wing that does not exist and one with an unknown input and combinator.
const WC_AIDS: [(&str, &str, &str, &str); 62] = [
    ("electronics.ini", "ABS_V2", "SLIP_RATIO_LIMIT", "0.12"),
    ("electronics.ini", "ABS_V2", "PRESENT", "1"),
    ("electronics.ini", "ABS_V2", "ACTIVE", "1"),
    ("electronics.ini", "ABS_V2", "RATE_HZ", "200"),
    ("electronics.ini", "ABS_V2", "CHANNELS", "2"),
    ("electronics.ini", "EDL", "PRESENT", "1"),
    ("electronics.ini", "EDL", "ACTIVE", "1"),
    ("electronics.ini", "EDL", "BRAKE_TORQUE_POWER", "1500"),
    ("electronics.ini", "EDL", "BRAKE_TORQUE_COAST", "700"),
    ("electronics.ini", "EDL", "DEAD_ZONE_POWER", "0.02"),
    ("electronics.ini", "EDL", "DEAD_ZONE_COAST", "0.04"),
    ("electronics.ini", "EDL", "MAX_SPIN_POWER", "0.25"),
    ("electronics.ini", "EDL", "MAX_SPIN_COAST", "0.5"),
    ("electronics.ini", "TRACTION_CONTROL", "RATE_HZ", "500"),
    ("electronics.ini", "TRACTION_CONTROL", "MIN_SPEED_KMH", "20"),
    // an open differential lets one wheel spin, so that the lock has something to do
    ("drivetrain.ini", "DIFFERENTIAL", "POWER", "0.02"),
    ("drivetrain.ini", "DIFFERENTIAL", "COAST", "0.02"),
    ("drivetrain.ini", "DIFFERENTIAL", "PRELOAD", "2"),
    ("aero.ini", "DYNAMIC_CONTROLLER_6", "WING", "1"),
    ("aero.ini", "DYNAMIC_CONTROLLER_6", "COMBINATOR", "ADD"),
    ("aero.ini", "DYNAMIC_CONTROLLER_6", "INPUT", "BRAKE"),
    ("aero.ini", "DYNAMIC_CONTROLLER_6", "LUT", "wc_brake.lut"),
    ("aero.ini", "DYNAMIC_CONTROLLER_6", "FILTER", "0.5"),
    ("aero.ini", "DYNAMIC_CONTROLLER_6", "UP_LIMIT", "30"),
    ("aero.ini", "DYNAMIC_CONTROLLER_6", "DOWN_LIMIT", "0"),
    ("aero.ini", "DYNAMIC_CONTROLLER_7", "WING", "1"),
    ("aero.ini", "DYNAMIC_CONTROLLER_7", "COMBINATOR", "MULT"),
    ("aero.ini", "DYNAMIC_CONTROLLER_7", "INPUT", "LATG"),
    ("aero.ini", "DYNAMIC_CONTROLLER_7", "LUT", "wc_latg.lut"),
    ("aero.ini", "DYNAMIC_CONTROLLER_7", "FILTER", "0.9"),
    ("aero.ini", "DYNAMIC_CONTROLLER_7", "UP_LIMIT", "30"),
    ("aero.ini", "DYNAMIC_CONTROLLER_7", "DOWN_LIMIT", "2"),
    ("aero.ini", "DYNAMIC_CONTROLLER_8", "WING", "0"),
    ("aero.ini", "DYNAMIC_CONTROLLER_8", "COMBINATOR", "ADD"),
    ("aero.ini", "DYNAMIC_CONTROLLER_8", "INPUT", "LONG"),
    ("aero.ini", "DYNAMIC_CONTROLLER_8", "LUT", "wc_long.lut"),
    ("aero.ini", "DYNAMIC_CONTROLLER_8", "FILTER", "0"),
    ("aero.ini", "DYNAMIC_CONTROLLER_8", "UP_LIMIT", "3"),
    ("aero.ini", "DYNAMIC_CONTROLLER_8", "DOWN_LIMIT", "-3"),
    ("aero.ini", "DYNAMIC_CONTROLLER_9", "WING", "6"),
    ("aero.ini", "DYNAMIC_CONTROLLER_9", "COMBINATOR", "ADD"),
    ("aero.ini", "DYNAMIC_CONTROLLER_9", "INPUT", "SUS_TRAVEL_LR"),
    ("aero.ini", "DYNAMIC_CONTROLLER_9", "LUT", "wc_travel.lut"),
    ("aero.ini", "DYNAMIC_CONTROLLER_9", "FILTER", "0.3"),
    ("aero.ini", "DYNAMIC_CONTROLLER_9", "UP_LIMIT", "8"),
    ("aero.ini", "DYNAMIC_CONTROLLER_9", "DOWN_LIMIT", "-8"),
    ("aero.ini", "DYNAMIC_CONTROLLER_10", "WING", "6"),
    ("aero.ini", "DYNAMIC_CONTROLLER_10", "COMBINATOR", "MULT"),
    ("aero.ini", "DYNAMIC_CONTROLLER_10", "INPUT", "SUS_TRAVEL_RR"),
    ("aero.ini", "DYNAMIC_CONTROLLER_10", "LUT", "wc_travel_mult.lut"),
    ("aero.ini", "DYNAMIC_CONTROLLER_10", "FILTER", "0.3"),
    ("aero.ini", "DYNAMIC_CONTROLLER_10", "UP_LIMIT", "8"),
    ("aero.ini", "DYNAMIC_CONTROLLER_10", "DOWN_LIMIT", "-8"),
    // a wing that does not exist
    ("aero.ini", "DYNAMIC_CONTROLLER_11", "WING", "12"),
    ("aero.ini", "DYNAMIC_CONTROLLER_11", "COMBINATOR", "ADD"),
    ("aero.ini", "DYNAMIC_CONTROLLER_11", "INPUT", "GAS"),
    ("aero.ini", "DYNAMIC_CONTROLLER_11", "LUT", "wc_brake.lut"),
    // an input and a combinator the game does not know: the stage's limits still act
    ("aero.ini", "DYNAMIC_CONTROLLER_12", "WING", "3"),
    ("aero.ini", "DYNAMIC_CONTROLLER_12", "COMBINATOR", "AVERAGE"),
    ("aero.ini", "DYNAMIC_CONTROLLER_12", "INPUT", "SCRIPT_12"),
    ("aero.ini", "DYNAMIC_CONTROLLER_12", "LUT", "wc_brake.lut"),
    ("aero.ini", "DYNAMIC_CONTROLLER_12", "UP_LIMIT", "1.2"),
];
const WC_AIDS_FILES: [(&str, &str); 6] = [
    (
        "drs.ini",
        "[HEADER]\r\nVERSION=2\r\n\r\n[DRS_ZONES]\r\nIGNORE_ZONES=1\r\n\r\n[DEACTIVATION]\r\nLIMIT_G=0.4\r\n\r\n\
         [WING_1]\r\nEFFECT=0.6\r\nMODE=EFFECT\r\n\r\n[WING_2]\r\nEFFECT=0\r\nMODE=ANGLE\r\nANGLE=6\r\n",
    ),
    ("wc_brake.lut", "0|0\r\n1|4\r\n"),
    ("wc_latg.lut", "-3|0.8\r\n0|1\r\n3|0.8\r\n"),
    ("wc_long.lut", "-4|-2\r\n0|0\r\n2|2\r\n"),
    ("wc_travel.lut", "0|-1\r\n20|0\r\n60|3\r\n"),
    ("wc_travel_mult.lut", "0|0.5\r\n30|1\r\n80|1.5\r\n"),
];

/// The same ABS with one channel: all four wheels are released together.
const WC_ABS1: [(&str, &str, &str, &str); 5] = [
    ("electronics.ini", "ABS_V2", "SLIP_RATIO_LIMIT", "0.12"),
    ("electronics.ini", "ABS_V2", "PRESENT", "1"),
    ("electronics.ini", "ABS_V2", "ACTIVE", "1"),
    ("electronics.ini", "ABS_V2", "RATE_HZ", "120"),
    ("electronics.ini", "ABS_V2", "CHANNELS", "1"),
];

/// The F2004 with the old one-body aero (`aero.ini [DATA]`, no wings), which no shipped car
/// uses: body drag with its sideways and vertical factors, the torque against the body's
/// rotation, lift split front / rear.
const WC_OLDAERO_FILES: [(&str, &str); 1] = [(
    "aero.ini",
    "[HEADER]\r\nVERSION=1\r\n\r\n[DATA]\r\nREFERENCE_AREA=1.4\r\nCD=0.95\r\nCL=1.8\r\nFRONT_SHARE=0.42\r\nCDX=0.3\r\n\
     CDY=2.0\r\n",
)];

/// Task 16, a four-wheel-drive car with the two `AWD` controller files no installed car that
/// loads has (`ctrl_awd_front_share.ini`, which no car ships at all, and
/// `ctrl_awd_center_lock.ini`), differentials away from their usual values, and three of the
/// four-wheel-drive setup items on the setup screen.
const AWD_CTRL: [(&str, &str, &str, &str); 34] = [
    ("drivetrain.ini", "AWD", "FRONT_SHARE", "35"),
    ("drivetrain.ini", "AWD", "FRONT_DIFF_POWER", "0.22"),
    ("drivetrain.ini", "AWD", "FRONT_DIFF_COAST", "0.11"),
    ("drivetrain.ini", "AWD", "FRONT_DIFF_PRELOAD", "15"),
    ("drivetrain.ini", "AWD", "CENTRE_DIFF_POWER", "0.3"),
    ("drivetrain.ini", "AWD", "CENTRE_DIFF_COAST", "0.2"),
    ("drivetrain.ini", "AWD", "CENTRE_DIFF_PRELOAD", "30"),
    ("setup.ini", "REAR_DIFF_PRELOAD", "SHOW_CLICKS", "0"),
    ("setup.ini", "REAR_DIFF_PRELOAD", "TAB", "DRIVETRAIN"),
    ("setup.ini", "REAR_DIFF_PRELOAD", "NAME", "Rear diff preload"),
    ("setup.ini", "REAR_DIFF_PRELOAD", "MIN", "5"),
    ("setup.ini", "REAR_DIFF_PRELOAD", "MAX", "400"),
    ("setup.ini", "REAR_DIFF_PRELOAD", "STEP", "7"),
    ("setup.ini", "REAR_DIFF_PRELOAD", "POS_X", "0.5"),
    ("setup.ini", "REAR_DIFF_PRELOAD", "POS_Y", "4"),
    ("setup.ini", "REAR_DIFF_PRELOAD", "HELP", "HELP_DIFF_PRELOAD"),
    ("setup.ini", "FRONT_DIFF_POWER", "SHOW_CLICKS", "0"),
    ("setup.ini", "FRONT_DIFF_POWER", "TAB", "DRIVETRAIN"),
    ("setup.ini", "FRONT_DIFF_POWER", "NAME", "Front diff power"),
    ("setup.ini", "FRONT_DIFF_POWER", "MIN", "0"),
    ("setup.ini", "FRONT_DIFF_POWER", "MAX", "100"),
    ("setup.ini", "FRONT_DIFF_POWER", "STEP", "3"),
    ("setup.ini", "FRONT_DIFF_POWER", "POS_X", "0.5"),
    ("setup.ini", "FRONT_DIFF_POWER", "POS_Y", "5"),
    ("setup.ini", "FRONT_DIFF_POWER", "HELP", "HELP_DIFF_PRELOAD"),
    ("setup.ini", "FRONT_DIFF_COAST", "SHOW_CLICKS", "0"),
    ("setup.ini", "FRONT_DIFF_COAST", "TAB", "DRIVETRAIN"),
    ("setup.ini", "FRONT_DIFF_COAST", "NAME", "Front diff coast"),
    ("setup.ini", "FRONT_DIFF_COAST", "MIN", "0"),
    ("setup.ini", "FRONT_DIFF_COAST", "MAX", "100"),
    ("setup.ini", "FRONT_DIFF_COAST", "STEP", "4"),
    ("setup.ini", "FRONT_DIFF_COAST", "POS_X", "0.5"),
    ("setup.ini", "FRONT_DIFF_COAST", "POS_Y", "6"),
    ("setup.ini", "FRONT_DIFF_COAST", "HELP", "HELP_DIFF_PRELOAD"),
];

const AWD_CTRL_FILES: [(&str, &str); 2] = [
    (
        "ctrl_awd_front_share.ini",
        "[CONTROLLER_0]\r\nINPUT=SPEED_KMH\r\nCOMBINATOR=ADD\r\nLUT=(|0=0.45|80=0.35|200=0.2|)\r\nFILTER=0.95\r\nUP_LIMIT=0.6\r\nDOWN_LIMIT=0.1\r\n\
         [CONTROLLER_1]\r\nINPUT=GAS\r\nCOMBINATOR=MULT\r\nLUT=(|0=1.2|1=0.8|)\r\nFILTER=0.5\r\nUP_LIMIT=0.7\r\nDOWN_LIMIT=0.05\r\n",
    ),
    (
        "ctrl_awd_center_lock.ini",
        "[CONTROLLER_0]\r\nINPUT=GEAR\r\nCOMBINATOR=ADD\r\nLUT=(|0=60|1=120|2=90|3=60|6=40|)\r\nFILTER=0.9\r\nUP_LIMIT=10000\r\nDOWN_LIMIT=0\r\n\
         [CONTROLLER_1]\r\nINPUT=REAR_SPEED_RATIO\r\nCOMBINATOR=ADD\r\nLUT=(|0=20|1.02=20|1.1=200|1.3=400|)\r\nFILTER=0.98\r\nUP_LIMIT=10000\r\nDOWN_LIMIT=0\r\n\
         [CONTROLLER_2]\r\nINPUT=BRAKE\r\nCOMBINATOR=MULT\r\nLUT=(|0=1|1=0.2|)\r\nFILTER=0.8\r\nUP_LIMIT=10000\r\nDOWN_LIMIT=0\r\n",
    ),
];

/// Task 16, an `AWD2` car without `ctrl_awd2.ini` (every installed one has it): the limit of
/// the coupling is the fixed `CENTRE_MAX_TORQUE`, low enough to be reached, and the car has
/// its `DIFF_*` setup items, two of them on the setup screen.
const AWD2_PLAIN: [(&str, &str, &str, &str); 20] = [
    ("drivetrain.ini", "AWD2", "CENTRE_RAMP_TORQUE", "60"),
    ("drivetrain.ini", "AWD2", "CENTRE_MAX_TORQUE", "350"),
    ("setup.ini", "DIFF_PRELOAD", "SHOW_CLICKS", "0"),
    ("setup.ini", "DIFF_PRELOAD", "TAB", "DRIVETRAIN"),
    ("setup.ini", "DIFF_PRELOAD", "NAME", "Rear diff preload"),
    ("setup.ini", "DIFF_PRELOAD", "MIN", "0"),
    ("setup.ini", "DIFF_PRELOAD", "MAX", "200"),
    ("setup.ini", "DIFF_PRELOAD", "STEP", "7"),
    ("setup.ini", "DIFF_PRELOAD", "POS_X", "0.5"),
    ("setup.ini", "DIFF_PRELOAD", "POS_Y", "4"),
    ("setup.ini", "DIFF_PRELOAD", "HELP", "HELP_DIFF_PRELOAD"),
    ("setup.ini", "DIFF_POWER", "SHOW_CLICKS", "0"),
    ("setup.ini", "DIFF_POWER", "TAB", "DRIVETRAIN"),
    ("setup.ini", "DIFF_POWER", "NAME", "Rear diff power"),
    ("setup.ini", "DIFF_POWER", "MIN", "0"),
    ("setup.ini", "DIFF_POWER", "MAX", "100"),
    ("setup.ini", "DIFF_POWER", "STEP", "3"),
    ("setup.ini", "DIFF_POWER", "POS_X", "0.5"),
    ("setup.ini", "DIFF_POWER", "POS_Y", "5"),
    ("setup.ini", "DIFF_POWER", "HELP", "HELP_DIFF_PRELOAD"),
];

/// Task 16, an `AWD2` car whose rear axle is a spool (`[DIFFERENTIAL]` 1 / 1: the test is
/// made on that section, though the values of the rear differential then come from `[AWD2]`).
const AWD2_SPOOL: [(&str, &str, &str, &str); 2] = [("drivetrain.ini", "DIFFERENTIAL", "POWER", "1"), ("drivetrain.ini", "DIFFERENTIAL", "COAST", "1")];

/// Task 16, a KERS on the driven wheels (no installed car has `ATTACH=WHEELS`): a torque table
/// that slopes over wheel speeds, a quick battery and a lap allowance low enough to be used up.
const KERS_WHEELS: [(&str, &str, &str, &str); 4] = [
    ("kers.ini", "KERS", "ATTACH", "WHEELS"),
    ("kers.ini", "KERS", "MAX_KJ_PER_LAP", "120"),
    ("kers.ini", "KERS", "CHARGE_K", "0.01"),
    ("kers.ini", "KERS", "DISCHARGE_TIME", "5000"),
];

const KERS_WHEELS_LUT: (&str, &str) = ("kers_torque.lut", "0|600\r\n1000|500\r\n2000|300\r\n3000|150\r\n3001|0\r\n");

/// A whole kers.ini for a car that has none: on the wheels, the button, a lap allowance.
const KERS_WHEELS_INI: (&str, &str) = (
    "kers.ini",
    "[HEADER]\r\nVERSION=3\r\n\r\n[KERS]\r\nBRAKE_LEVEL=0.5\r\nCHARGE_K=0.004\r\nTORQUE_CURVE=kers_torque.lut\r\nDISCHARGE_TIME=8000\r\n\
     NEGATIVE_INPUT_CHARGE_K=1\r\nCONTROLLER=\r\nATTACH=WHEELS\r\nHAS_BUTTON_OVERRIDE=1\r\nMAX_KJ_PER_LAP=200\r\n",
);

/// Task 16, a controller-driven KERS that also has the button and a lap allowance (the
/// LaFerrari's file is version 2, which reads neither).
const KERS_BUTTON: [(&str, &str, &str, &str); 3] =
    [("kers.ini", "HEADER", "VERSION", "3"), ("kers.ini", "KERS", "HAS_BUTTON_OVERRIDE", "1"), ("kers.ini", "KERS", "MAX_KJ_PER_LAP", "150")];

/// Task 16, an ERS whose lap allowance is used up quickly, whose default delivery profile does
/// not exist (the map of ers.ini itself, which has no stages, stays: no delivery without the
/// button) and whose turbo fills the battery faster.
const ERS_LIMITS: [(&str, &str, &str, &str); 6] = [
    ("ers.ini", "KINETIC", "MAX_KJ_PER_LAP", "150"),
    ("ers.ini", "KINETIC", "DEFAULT_CONTROLLER", "9"),
    ("ers.ini", "HEAT", "CHARGE_K", "0.02"),
    ("ers.ini", "COCKPIT_CONTROLS", "RECOVERY", "1"),
    ("ers.ini", "COCKPIT_CONTROLS", "DELIVERY_PROFILE", "1"),
    ("ers.ini", "COCKPIT_CONTROLS", "MGU_H_MODE", "0"),
];

/// Task 16, front motors with torque vectoring (the two cars that have front motors say 0),
/// the button, a reachable lap allowance and a rear brake correction; with
/// [`ERS_FRONT_MAPS`] the front motors also deliver without the button.
const ERS_FRONT: [(&str, &str, &str, &str); 4] = [
    ("ers.ini", "FRONT_MOTORS", "FRONT_TORQUE_VECTORING_BIAS", "0.6"),
    ("ers.ini", "KINETIC", "HAS_BUTTON_OVERRIDE", "1"),
    ("ers.ini", "KINETIC", "MAX_KJ_PER_LAP", "300"),
    ("ers.ini", "KINETIC", "BRAKE_REAR_CORRECTION", "25"),
];

/// The front motors' map of the Lithium's default profile ("High": throttle x revs x speed):
/// the mod's own tables are all zero, so its front motors never deliver by the map. These
/// do: with the throttle, less above 100 km/h.
const ERS_FRONT_MAPS: [(&str, &str); 3] = [
    ("High_FRONT_GAS.lut", "0|0
1|0.8
"),
    ("High_FRONT_RPMS.lut", "0|1
21000|1
"),
    ("High_FRONT_SPEED_KMH.lut", "0|1
100|1
320|0.2
"),
];

fn test_car_command() -> Result<(), String> {
    write_test_car_from("ks_lamborghini_sesto_elemento", "sesto_awd_ctrl", &AWD_CTRL, &AWD_CTRL_FILES, &[])?;
    write_test_car_from("ks_audi_r8_plus", "r8_awd2_plain", &AWD2_PLAIN, &[], &["ctrl_awd2.ini"])?;
    write_test_car_from("ks_audi_r8_plus", "r8_awd2_spool", &AWD2_SPOOL, &[], &[])?;
    write_test_car_from("ks_ferrari_f138", "f138_kers_wheels", &KERS_WHEELS, &[KERS_WHEELS_LUT], &[])?;
    write_test_car_from("ferrari_laferrari", "laferrari_kers_button", &KERS_BUTTON, &[], &[])?;
    write_test_car_from("ks_lamborghini_sesto_elemento", "sesto_awd_kers", &[], &[KERS_WHEELS_INI, KERS_WHEELS_LUT], &[])?;
    write_test_car_from("ks_audi_r8_plus", "r8_awd2_kers", &[], &[KERS_WHEELS_INI, KERS_WHEELS_LUT], &[])?;
    write_test_car_from("ks_ferrari_sf15t", "sf15t_ers_limits", &ERS_LIMITS, &[], &[])?;
    // (a mod car: only where it is installed and unpacked into cardata/)
    if repo_root().join("cardata/vrc_formula_lithium_2023/ers.ini").is_file() {
        write_test_car_from("vrc_formula_lithium_2023", "lithium_ers_front", &ERS_FRONT, &ERS_FRONT_MAPS, &[])?;
    }
    write_test_car("f2004_wc_oldaero", &[], &WC_OLDAERO_FILES)?;
    write_test_car("f2004_wc_aids", &WC_AIDS, &WC_AIDS_FILES)?;
    write_test_car("f2004_wc_abs1", &WC_ABS1, &[])?;
    write_test_car("f2004_tight_stops", &TIGHT_STOPS, &[])?;
    write_test_car("f2004_fallbacks", &FALLBACKS, &[])?;
    write_test_car("f2004_pt_street", &PT_STREET, &[])?;
    write_test_car("f2004_pt_ctrl", &PT_CTRL, &PT_CTRL_FILES)?;
    write_test_car("f2004_pt_fwd", &PT_FWD, &[])?;
    // a wheel KERS on a front-wheel-drive car: the front tyres get the torque
    write_test_car_from("f2004_pt_fwd", "f2004_fwd_kers", &[], &[KERS_WHEELS_INI, KERS_WHEELS_LUT], &[])
}

fn write_test_car(name: &str, patches: &[(&str, &str, &str, &str)], files: &[(&str, &str)]) -> Result<(), String> {
    write_test_car_from("ks_ferrari_f2004", name, patches, files, &[])
}

/// A test car: the data files of `base` with values changed, files added and files left out.
fn write_test_car_from(base: &str, name: &str, patches: &[(&str, &str, &str, &str)], files: &[(&str, &str)], without: &[&str]) -> Result<(), String> {
    let repo = repo_root();
    let from = repo.join("cardata").join(base);
    let to = repo.join("cardata").join(name);
    // a folder left from an earlier run may hold files this car must not have
    if to.is_dir() {
        std::fs::remove_dir_all(&to).map_err(|e| format!("{}: {e}", to.display()))?;
    }
    std::fs::create_dir_all(&to).map_err(|e| e.to_string())?;
    for entry in std::fs::read_dir(&from).map_err(|e| format!("{}: {e}", from.display()))? {
        let entry = entry.map_err(|e| e.to_string())?;
        if entry.file_type().map_err(|e| e.to_string())?.is_file() && !without.iter().any(|name| entry.file_name().to_string_lossy() == *name) {
            std::fs::copy(entry.path(), to.join(entry.file_name())).map_err(|e| e.to_string())?;
        }
    }
    for name in without {
        println!("{name} (left out)");
    }
    for &(file, section, key, value) in patches {
        let path = to.join(file);
        let bytes = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let text = String::from_utf8_lossy(&bytes);
        let patched = patch_ini(&text, section, key, value).map_err(|e| format!("{}: {e}", path.display()))?;
        std::fs::write(&path, patched).map_err(|e| format!("{}: {e}", path.display()))?;
        println!("{file} [{section}] {key}={value}");
    }
    for &(file, text) in files {
        std::fs::write(to.join(file), text).map_err(|e| format!("{}: {e}", to.join(file).display()))?;
        println!("{file} (new file)");
    }
    println!("{}", to.display());
    Ok(())
}

fn usage() -> String {
    "usage: chassis_compare run [<scenario> ...] [--dir <folder>] [--feed brakes,drivetrain] [--verbose] [--stop-after <steps>]\n       \
     chassis_compare excerpt\n       chassis_compare excerpt-track\n       chassis_compare excerpt-collide\n       chassis_compare faults [<scenario>] [--dir <folder>] [--feed brakes,drivetrain]\n       chassis_compare faults16 <scenario> --dir <folder>\n       \
     chassis_compare test-car\n       \
     chassis_compare game-replay [<scenario> ...] [--dir <folder>] [--exe <rustyac.exe>]\n\
     --feed names the ported systems to take from the recording instead of computing them in Rust (default: none)"
        .to_string()
}

fn main() {
    let mut args = std::env::args().skip(1);
    let command = args.next().unwrap_or_default();
    let mut names = Vec::new();
    let mut verbose = false;
    let mut stop_after = None;
    let mut dir: Option<PathBuf> = None;
    let mut systems = Systems::ALL;
    let mut exe: Option<PathBuf> = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--verbose" => verbose = true,
            "--exe" => exe = args.next().map(PathBuf::from),
            "--dir" => dir = args.next().map(PathBuf::from),
            "--feed" => {
                for name in args.next().unwrap_or_default().split(',') {
                    match name {
                        "brakes" => systems.brakes = false,
                        "drivetrain" | "engine" => systems.drivetrain = false,
                        "aero" => systems.aero = false,
                        "aids" => systems.aids = false,
                        other => {
                            eprintln!("--feed: unknown system {other:?}\n{}", usage());
                            std::process::exit(2);
                        }
                    }
                }
            }
            "--stop-after" => stop_after = args.next().and_then(|v| v.parse().ok()),
            other if other.starts_with("--") => {
                eprintln!("unknown option {other}\n{}", usage());
                std::process::exit(2);
            }
            name => names.push(name.to_string()),
        }
    }
    if !systems.drivetrain && systems.aids {
        // traction control and the pit limiter act on the engine: without an engine of its own
        // the car cannot have its own aids
        println!("(the aids are fed too: they act on the engine, which comes from the recording)");
        systems.aids = false;
    }
    let result = match command.as_str() {
        "run" => run_command(&names, dir.as_deref(), systems, verbose, stop_after),
        "test-car" => test_car_command(),
        "excerpt" => excerpt_command(),
        "excerpt-track" => excerpt_track_command(),
        "excerpt-collide" => excerpt_collide_command(),
        "faults" => faults_command(&names, dir.as_deref(), systems),
        "faults16" => faults16_command(&names, dir.as_deref()),
        "excerpt16" => excerpt16_command(),
        "game-replay" => game_replay_command(&names, dir.as_deref(), exe.as_deref(), verbose),
        _ => Err(usage()),
    };
    if let Err(message) = result {
        eprintln!("{message}");
        std::process::exit(1);
    }
}
