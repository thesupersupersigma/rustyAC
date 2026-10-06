//! The Rust rolling chassis against the whole-car recordings of `tools/car_oracle`.
//!
//! chassis_compare run [<scenario> ...] [--verbose] [--stop-after <steps>]
//!     Free run from step 0 of every `oracle/car/<scenario>.carrec` (default: all but
//!     `settle_floor`, whose floor contacts are stage 2 of the rigid-body port): the Rust
//!     chassis (body, fuel tank, hubs, double-wishbone suspensions, heave springs, anti-roll
//!     bars, steering, force feedback) with the Rust tyres on the Rust ODE, built from
//!     `cardata/<car>` and fed only what the systems that are not ported yet hand to it
//!     (controls, engine values for the fuel burn, brake torques, driven-wheel speeds, wing
//!     forces). After every step every body state, joint value, suspension value, tyre value,
//!     the steering signal, the force-feedback number and the whole force tape are compared
//!     with the game, bit for bit. The full run writes `oracle/chassis/results.md`.
//! chassis_compare excerpt
//!     Writes the golden excerpts of `crates/rustyac-physics/tests/chassis_golden.rs`.
//! chassis_compare faults [<scenario>]
//!     A check of the check: the same free run (default scenario `slalom`) with one small
//!     deliberate fault in the Rust chassis at a time (a damper rate, a bar rate, a spring rate
//!     … changed in its last bit, the setup rounding left out, the joint softening left out),
//!     and where the comparison first notices. Writes `oracle/chassis/faults.md`.
//!
//! Needs no acs.exe.

#[path = "../../car_oracle/src/record.rs"]
#[allow(dead_code, clippy::wrong_self_convention)]
mod record;
#[path = "../../car_oracle/src/sites.rs"]
#[allow(dead_code)]
mod sites;

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use record::Recording;
use rustyac_physics::car::replay::{
    self, body_words, Field, Golden, GoldenStep, Ground, RecordedCall, RecordedFeed, RecordedStep, RecordedWheel, RunSetup,
    DT, WHEELS,
};
use rustyac_physics::car::{CarControls, ChassisEnvironment, EngineFeed, ForceSource, RollingChassis};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().and_then(Path::parent).expect("the repository folder").to_path_buf()
}

/// Systems whose force calls the chassis makes itself.
const OWN_SYSTEMS: [&str; 10] =
    ["tyre", "surface", "spring", "damper", "bumpstop", "heave_spring", "heave_damper", "heave_bumpstop", "arb", "sleep"];
/// Systems whose force calls are fed from the tape.
const FED_SYSTEMS: [&str; 2] = ["aero_drag", "aero_lift"];

/// How a recording was made, as far as the chassis needs to know.
fn run_setup(recording: &Recording) -> Result<RunSetup, String> {
    let get = |key: &str| recording.get(key).ok_or(format!("the recording's header has no {key}"));
    let first = |name: &str| recording.f(0, name);
    let env = ChassisEnvironment {
        ambient_temperature: first("physics.ambientTemperature"),
        road_temperature: first("physics.roadTemperature"),
        dynamic_grip_level: first("track.dynamicGripLevel"),
        tyre_consumption_rate: first("tyre.lf.in_tyre_consumption_rate"),
        mechanical_damage_rate: first("tyre.lf.in_mechanical_damage_rate"),
        allow_tyre_blankets: recording.i(0, "tyre.lf.in_allow_tyre_blankets") != 0,
        ..ChassisEnvironment::default()
    };
    Ok(RunSetup {
        scenario: get("scenario")?.to_string(),
        ground: Ground::parse(get("ground")?).ok_or("the recording's ground")?,
        seed: get("seed")?.parse().map_err(|e| format!("seed: {e}"))?,
        clock_start_ms: get("clock_start_ms")?.parse().map_err(|e| format!("clock_start_ms: {e}"))?,
        env,
    })
}

/// What the chassis is fed in one step: only values that systems outside the chassis own.
fn recorded_step(recording: &Recording, step: usize) -> Result<RecordedStep, String> {
    let f = |name: &str| recording.f(step, name);
    let mut out = RecordedStep {
        controls: CarControls { gas: f("script.gas"), brake: f("script.brake"), steer: f("script.steer"), clutch: f("script.clutch") },
        // only the automatic clutch rewrites the clutch pedal, before the sleeping rule reads it
        clutch: f("controls.clutch"),
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
    for call in &recording.steps[step].calls {
        let system = recording.system_of(call);
        if FED_SYSTEMS.contains(&system) {
            if call.body != 0 {
                return Err(format!("step {step}: a {system} call on body {}", call.body));
            }
            let source = ForceSource::from_name(system).unwrap();
            out.aero.push(RecordedCall { kind: call.kind, source, a: call.a, b: call.b });
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

impl Columns {
    fn new(recording: &Recording) -> Result<Columns, String> {
        let fields = replay::fields();
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

fn same_float(a: f32, b: f32) -> bool {
    a.to_bits() == b.to_bits() || (a.is_nan() && b.is_nan())
}

/// Compares the Rust force tape of a step with the game's. `Err` describes the first difference.
fn compare_tape(recording: &Recording, step: usize, chassis: &RollingChassis) -> Result<(), String> {
    let game = &recording.steps[step].calls;
    let rust = chassis.core.tape.as_deref().unwrap_or(&[]);
    for (seq, (g, r)) in game.iter().zip(rust).enumerate() {
        let system = recording.system_of(g);
        let head = format!("force call {seq} ({} on {}, {system})", replay::kind_name(g.kind), replay::BODIES[g.body as usize]);
        if g.body != r.body || g.kind != r.kind {
            return Err(format!("{head}: Rust made {} on {}", replay::kind_name(r.kind), replay::BODIES[r.body as usize]));
        }
        if system != r.source.name() {
            return Err(format!("{head}: Rust books it under {}", r.source.name()));
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
                        "{head}: {name}[{k}]: game {:?} ({:#010x}) / Rust {:?} ({:#010x})",
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

const FAULTS: [(&str, &str, Fault); 13] = [
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

struct Outcome {
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

fn compare(
    recording: &Recording,
    data: &Path,
    verbose: bool,
    stop_after: Option<usize>,
    fault: Option<Fault>,
) -> Result<Outcome, String> {
    let setup = run_setup(recording)?;
    let columns = Columns::new(recording)?;
    let mut chassis = setup.build(data)?;
    if let Some(fault) = fault {
        fault(&mut chassis);
    }
    let started = std::time::Instant::now();
    let mut outcome = Outcome {
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
        chassis.step(DT, setup.time_of_step(step), &mut RecordedFeed { step: &feed });
        let rust = replay::snapshot(&chassis);
        let game = columns.game(recording, step);
        let mut differing = Vec::new();
        for (k, field) in columns.fields.iter().enumerate() {
            if !replay::same_value(field.kind, game[k], rust[k]) {
                differing.push(k);
            }
        }
        let tape = compare_tape(recording, step, &chassis);
        outcome.steps += 1;
        outcome.calls += recording.steps[step].calls.len();
        if differing.is_empty() && tape.is_ok() {
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
            // the tape is in execution order, so its first difference is the earliest cause
            let text = match (&tape, differing.first()) {
                (Err(tape), _) => tape.clone(),
                (Ok(()), Some(&k)) => describe(k),
                (Ok(()), None) => unreachable!(),
            };
            outcome.first = Some((step, text));
            outcome.first_count = differing.len();
            if verbose {
                println!("  first difference at step {step}; {} of {} values differ:", differing.len(), columns.fields.len());
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

fn run_command(names: &[String], verbose: bool, stop_after: Option<usize>) -> Result<(), String> {
    let repo = repo_root();
    let folder = repo.join("oracle/car");
    let full = names.is_empty() && stop_after.is_none();
    let mut scenarios: Vec<String> = names.to_vec();
    if scenarios.is_empty() {
        for entry in std::fs::read_dir(&folder).map_err(|e| format!("{}: {e}", folder.display()))? {
            let path = entry.map_err(|e| e.to_string())?.path();
            if path.extension().is_some_and(|e| e == "carrec") {
                let name = path.file_stem().unwrap().to_string_lossy().to_string();
                // its floor contacts are stage 2 of the rigid-body port
                if name != "settle_floor" {
                    scenarios.push(name);
                }
            }
        }
        scenarios.sort();
    }
    let mut outcomes = Vec::new();
    for name in &scenarios {
        let path = folder.join(format!("{name}.carrec"));
        let recording = Recording::read(&path)?;
        let data = car_data(&recording)?;
        println!("{name}: {} steps", recording.steps.len());
        let outcome = compare(&recording, &data, verbose, stop_after, None)?;
        match &outcome.first {
            None => println!("  bit-exact: {} ({:.1} s)", percent(outcome.exact, outcome.steps), outcome.seconds),
            Some((step, text)) => {
                println!("  bit-exact: {}; first difference at step {step}: {text}", percent(outcome.exact, outcome.steps))
            }
        }
        outcomes.push(outcome);
    }
    let mut table = String::new();
    writeln!(table, "| Scenario | Steps | Bit-exact steps | First divergence | Values compared per step | Force calls compared |").unwrap();
    writeln!(table, "|---|---|---|---|---|---|").unwrap();
    let (mut steps, mut exact, mut calls) = (0, 0, 0);
    for o in &outcomes {
        let first = match &o.first {
            None => "none".to_string(),
            Some((step, text)) => format!("step {step}: {text} ({} values differ in that step)", o.first_count),
        };
        writeln!(table, "| `{}` | {} | {} | {} | {} | {} |", o.scenario, o.steps, percent(o.exact, o.steps), first, o.values, o.calls)
            .unwrap();
        steps += o.steps;
        exact += o.exact;
        calls += o.calls;
    }
    writeln!(table, "| **all** | **{steps}** | **{}** | | | **{calls}** |", percent(exact, steps)).unwrap();
    println!("\n{table}");
    let out = repo.join("oracle/chassis");
    std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
    let file = out.join(if full { "results.md" } else { "partial.md" });
    std::fs::write(&file, &table).map_err(|e| format!("{}: {e}", file.display()))?;
    println!("{}", file.display());
    if outcomes.iter().any(|o| o.first.is_some()) {
        return Err("the Rust chassis and the game differ".to_string());
    }
    Ok(())
}

/// Writes one golden excerpt: `count` steps of a recording from `first` on. The Rust chassis
/// runs freely up to `first` (and must agree with the game all the way), its state there is
/// the excerpt's start state; the expected values are the game's.
fn write_excerpt(recording: &Recording, data: &Path, first: usize, count: usize, path: &Path) -> Result<usize, String> {
    let setup = run_setup(recording)?;
    let columns = Columns::new(recording)?;
    let mut chassis = setup.build(data)?;
    for step in 0..first {
        let feed = recorded_step(recording, step)?;
        chassis.step(DT, setup.time_of_step(step), &mut RecordedFeed { step: &feed });
        let rust = replay::snapshot(&chassis);
        let game = columns.game(recording, step);
        if let Some(k) = (0..rust.len()).find(|&k| !replay::same_value(columns.kinds[k], game[k], rust[k])) {
            return Err(format!("step {step}: {} differs from the game: no excerpt written", columns.fields[k].name));
        }
        compare_tape(recording, step, &chassis)?;
    }
    let state = if first == 0 { Vec::new() } else { chassis.save_state() };
    let mut steps = Vec::with_capacity(count);
    for step in first..first + count {
        let feed = recorded_step(recording, step)?;
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
        let hash = replay::step_hash(&columns.kinds, &columns.game(recording, step), &tape);
        // the game's bodies after the step
        let mut bodies = Vec::new();
        for body in replay::BODIES {
            let col = |name: &str| recording.steps[step].words[recording.col(&format!("{body}.post.{name}"))];
            for name in ["pos.x", "pos.y", "pos.z", "q.w", "q.x", "q.y", "q.z", "lvel.x", "lvel.y", "lvel.z", "avel.x", "avel.y", "avel.z"] {
                bodies.push(col(name));
            }
        }
        steps.push(GoldenStep { feed, hash, bodies });
    }
    let golden = Golden { setup, first, state, steps };
    let bytes = golden.to_bytes();
    // the file must replay: parse it back and run it the way `cargo test` will
    let parsed = Golden::parse(&bytes)?;
    if parsed != golden {
        return Err("the golden file does not read back as written".to_string());
    }
    parsed.check(data)?;
    // and the free run itself must still agree at the end of the excerpt
    for step in first..first + count {
        let feed = recorded_step(recording, step)?;
        chassis.step(DT, golden.setup.time_of_step(step), &mut RecordedFeed { step: &feed });
        if body_words(&chassis) != golden.steps[step - first].bodies {
            return Err(format!("step {step}: the free run left the game"));
        }
    }
    std::fs::write(path, &bytes).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(bytes.len())
}

fn excerpt_command() -> Result<(), String> {
    let repo = repo_root();
    let out = repo.join("crates/rustyac-physics/tests/golden");
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
        let bytes = write_excerpt(&recording, &data, first, count, &path)?;
        println!("{} ({bytes} bytes, steps {first}..{})", path.display(), first + count);
    }
    Ok(())
}

/// Runs one scenario once per deliberate fault and reports where the comparison notices.
fn faults_command(names: &[String]) -> Result<(), String> {
    let repo = repo_root();
    let scenario = names.first().map(String::as_str).unwrap_or("slalom");
    let recording = Recording::read(&repo.join(format!("oracle/car/{scenario}.carrec")))?;
    let data = car_data(&recording)?;
    let mut table = String::new();
    writeln!(table, "Scenario `{scenario}`, {} steps. Without a fault: no difference.\n", recording.steps.len()).unwrap();
    writeln!(table, "| Fault | What is changed | Bit-exact steps | Noticed at | First value that differs |").unwrap();
    writeln!(table, "|---|---|---|---|---|").unwrap();
    let clean = compare(&recording, &data, false, None, None)?;
    if clean.first.is_some() {
        return Err("the run without a fault already differs".to_string());
    }
    let mut missed = Vec::new();
    for (name, about, fault) in FAULTS {
        let outcome = compare(&recording, &data, false, None, Some(fault))?;
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
    let file = out.join("faults.md");
    std::fs::write(&file, &table).map_err(|e| format!("{}: {e}", file.display()))?;
    println!("\n{table}\n{}", file.display());
    if !missed.is_empty() {
        return Err(format!("faults the comparison did not notice: {}", missed.join(", ")));
    }
    Ok(())
}

fn usage() -> String {
    "usage: chassis_compare run [<scenario> ...] [--verbose] [--stop-after <steps>]\n       chassis_compare excerpt\n       \
     chassis_compare faults [<scenario>]"
        .to_string()
}

fn main() {
    let mut args = std::env::args().skip(1);
    let command = args.next().unwrap_or_default();
    let mut names = Vec::new();
    let mut verbose = false;
    let mut stop_after = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--verbose" => verbose = true,
            "--stop-after" => stop_after = args.next().and_then(|v| v.parse().ok()),
            other if other.starts_with("--") => {
                eprintln!("unknown option {other}\n{}", usage());
                std::process::exit(2);
            }
            name => names.push(name.to_string()),
        }
    }
    let result = match command.as_str() {
        "run" => run_command(&names, verbose, stop_after),
        "excerpt" => excerpt_command(),
        "faults" => faults_command(&names),
        _ => Err(usage()),
    };
    if let Err(message) = result {
        eprintln!("{message}");
        std::process::exit(1);
    }
}
