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
#[path = "../../car_oracle/src/sites.rs"]
#[allow(dead_code)]
mod sites;

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use record::Recording;
use rustyac_physics::car::replay::{
    self, body_words, Field, Golden, GoldenStep, Ground, RecordedCall, RecordedStep, RecordedWheel, RunSetup, WHEELS,
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

    fn describe(&self) -> &'static str {
        match (self.brakes, self.drivetrain) {
            (true, true) => "brakes, engine and drivetrain in Rust",
            (true, false) => "brakes in Rust; engine and drivetrain fed",
            (false, true) => "engine and drivetrain in Rust; brakes fed",
            (false, false) => "brakes, engine and drivetrain fed",
        }
    }
}

/// How a recording was made, as far as the chassis needs to know.
fn run_setup(recording: &Recording, systems: Systems) -> Result<RunSetup, String> {
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
        rust_brakes: systems.brakes,
        rust_drivetrain: systems.drivetrain,
        rust_aero: systems.aero,
        rust_aids: systems.aids,
        auto_clutch: get("auto_clutch")? != "0",
        // the key came with the powertrain scenarios; older recordings ran without the aid
        auto_shifter: recording.get("auto_shifter").is_some_and(|v| v != "0"),
    })
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
            kers: false,
            requested_gear_index: script_i("script.requestedGear", "controls.requestedGearIndex"),
            hand_brake: if recording.has("script.handBrake") { f("script.handBrake") } else { f("controls.handBrake") },
        },
        bias_clicks: if recording.has("script.biasClicks") { recording.i(step, "script.biasClicks") } else { 0 },
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
            self.tc_cut += (feed.engine_electronic_override != 1.0) as usize;
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

    const HEAD: &'static str = "| Scenario | Brake pedal / handbrake / cockpit bias / EBB / disc temps | Limiter / below idle / no fuel / TC cut (fed) / boost | Clutch locked in gear / locked neutral / slipping in gear / slipping neutral | Paddle shifts up / down / cut-off steps / refused presses | Reverse / grinding | Diff holds / slips / wheels held | Clutch profile / blip / auto-shifter presses |\n|---|---|---|---|---|---|---|---|";

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

fn compare(
    recording: &Recording,
    data: &Path,
    systems: Systems,
    verbose: bool,
    stop_after: Option<usize>,
    fault: Option<Fault>,
) -> Result<Outcome, String> {
    let setup = run_setup(recording, systems)?;
    let columns = Columns::new(recording)?;
    let mut chassis = setup.build(data)?;
    if let Some(fault) = fault {
        fault(&mut chassis);
    }
    let started = std::time::Instant::now();
    let mut outcome = Outcome {
        coverage: Coverage::default(),
        powertrain: PowertrainCoverage::default(),
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
        let request_before = chassis.drivetrain.as_ref().map(|d| d.base().gear_request.request as i32).unwrap_or(0);
        let paddles_before = (chassis.gear_changer.last_gear_up, chassis.gear_changer.last_gear_dn);
        replay::step_recorded(&mut chassis, setup.time_of_step(step), &feed);
        let rust = replay::snapshot(&chassis);
        let game = columns.game(recording, step);
        let mut differing = Vec::new();
        for (k, field) in columns.fields.iter().enumerate() {
            if !replay::same_value(field.kind, game[k], rust[k]) {
                differing.push(k);
            }
        }
        // brakes, engine, drivetrain, shift helpers
        let trace = replay::powertrain_trace(&chassis);
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
        "| Scenario | Steps | Bit-exact steps | First divergence | Chassis values compared per step | Brake / engine / \
         drivetrain values compared per step | Force calls compared |"
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
    println!("\n{table}");
    let out = repo.join("oracle/chassis");
    std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
    let mut suffix = match dir {
        Some(dir) => format!("_{}", dir.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()),
        None => String::new(),
    };
    // the chassis-only results of Task 08 keep their file names
    match (systems.brakes, systems.drivetrain) {
        (true, true) => suffix.push_str("_powertrain"),
        (true, false) => suffix.push_str("_brakes"),
        (false, true) => suffix.push_str("_drivetrain"),
        (false, false) => {}
    }
    let file = out.join(if full { format!("results{suffix}.md") } else { format!("partial{suffix}.md") });
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
    let mut chassis = setup.build(data)?;
    for step in 0..first {
        let feed = recorded_step(recording, step)?;
        replay::step_recorded(&mut chassis, setup.time_of_step(step), &feed);
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
    let trace_names: Vec<replay::TraceValue> = replay::powertrain_trace(&chassis).into_iter().filter(|v| !v.extra).collect();
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
        let mut kinds = columns.kinds.clone();
        let mut words = columns.game(recording, step);
        for value in &trace_names {
            kinds.push(value.kind);
            words.push(game_trace_word(recording, step, value).ok_or(format!("the recording has no field {}", value.name))?);
        }
        let hash = replay::step_hash(&kinds, &words, &tape);
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
        replay::step_recorded(&mut chassis, golden.setup.time_of_step(step), &feed);
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
        let path = out.join(format!("powertrain_{scenario}_{first}_{count}.chgold"));
        let bytes = write_excerpt(&recording, &data, Systems::ALL, first, count, &path)?;
        println!("{} ({bytes} bytes, steps {first}..{})", path.display(), first + count);
    }
    Ok(())
}

/// Runs one scenario once per deliberate fault and reports where the comparison notices.
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
    let powertrain_faults = POWERTRAIN_FAULTS.iter().filter(|_| systems != Systems::CHASSIS);
    for &(name, about, fault) in chassis_faults.chain(powertrain_faults) {
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
    let tail = if systems == Systems::CHASSIS { "" } else { "_powertrain" };
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

fn test_car_command() -> Result<(), String> {
    write_test_car("f2004_tight_stops", &TIGHT_STOPS, &[])?;
    write_test_car("f2004_fallbacks", &FALLBACKS, &[])?;
    write_test_car("f2004_pt_street", &PT_STREET, &[])?;
    write_test_car("f2004_pt_ctrl", &PT_CTRL, &PT_CTRL_FILES)?;
    write_test_car("f2004_pt_fwd", &PT_FWD, &[])
}

fn write_test_car(name: &str, patches: &[(&str, &str, &str, &str)], files: &[(&str, &str)]) -> Result<(), String> {
    let repo = repo_root();
    let from = repo.join("cardata/ks_ferrari_f2004");
    let to = repo.join("cardata").join(name);
    std::fs::create_dir_all(&to).map_err(|e| e.to_string())?;
    for entry in std::fs::read_dir(&from).map_err(|e| format!("{}: {e}", from.display()))? {
        let entry = entry.map_err(|e| e.to_string())?;
        if entry.file_type().map_err(|e| e.to_string())?.is_file() {
            std::fs::copy(entry.path(), to.join(entry.file_name())).map_err(|e| e.to_string())?;
        }
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
     chassis_compare excerpt\n       chassis_compare faults [<scenario>] [--dir <folder>] [--feed brakes,drivetrain]\n       \
     chassis_compare test-car\n\
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
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--verbose" => verbose = true,
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
    let result = match command.as_str() {
        "run" => run_command(&names, dir.as_deref(), systems, verbose, stop_after),
        "test-car" => test_car_command(),
        "excerpt" => excerpt_command(),
        "faults" => faults_command(&names, dir.as_deref(), systems),
        _ => Err(usage()),
    };
    if let Err(message) = result {
        eprintln!("{message}");
        std::process::exit(1);
    }
}
