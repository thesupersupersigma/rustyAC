// SPDX-License-Identifier: MIT OR Apache-2.0

//! Checks on a finished recording: the tyres replayed through the Rust port, and a few
//! headline numbers for a plausibility look.

use std::path::Path;

use rustyac_physics::tyre::rig::{self, Rig, RigCar, RigHub, StepInput};
use rustyac_physics::tyre::{TyreCar, VanillaTyre};

use crate::record::{Call, Recording};
use crate::scenario::{Ground, SETTLE_STEPS};

/// The game switches collision detection off for the first 250 steps of a session
/// (`setNoCollisionSteps(250)` in the new-session job), so the body's boxes cannot touch the
/// road in that time in the game either.
const NO_COLLISION_STEPS: usize = 250;

pub struct Report {
    /// Steps times four wheels.
    pub wheel_steps: usize,
    /// Wheel-steps in which every force the tyre handed to its hub and the car body, and its
    /// Fx / Fy / Mz / load, are bit-identical in the game and in the Rust port.
    pub force_steps_matching: usize,
    /// Wheel-steps in which every recorded tyre value is.
    pub full_steps_matching: usize,
    /// Steps in which, for every body, the accumulators after the last call on the force tape
    /// are bit-identical to the accumulators read just before `dWorldStep` (nothing wrote to
    /// them after the last recorded call).
    pub tape_steps_closed: usize,
    /// Calls on the tape, and how many of them explain the change of the body's accumulators
    /// bit for bit: accumulators after the previous call on that body (zero at the start of
    /// the step) plus what ODE's add-force function does with this call's vectors and the
    /// body's pose must give the accumulators recorded after this call.
    pub tape_calls: usize,
    pub tape_calls_explained: usize,
    /// Wheel-steps in which the calls the tyre made on its hub and the tape's entries on that
    /// hub booked under "tyre" are the same list (kind, force bits, point bits, order).
    pub tape_tyre_links: usize,
    pub headline: Vec<String>,
    pub notes: Vec<String>,
}

pub fn percent(part: usize, whole: usize) -> String {
    if whole == 0 {
        return "n/a".to_string();
    }
    if part == whole {
        return format!("100 % ({part}/{whole})");
    }
    format!("{:.3} % ({part}/{whole})", part as f64 * 100.0 / whole as f64)
}

/// Is a tyre output one of "the forces"?
fn is_force(name: &str) -> bool {
    name.starts_with("call")
        || matches!(name, "status.Fx" | "status.Fy" | "status.Mz" | "status.load" | "status.feedbackTorque")
}

/// One recorded word of the single-wheel-rig layout, widened the way the rig stores it.
fn rig_word(recording: &Recording, step: usize, column: usize, kind: char) -> u64 {
    let words = &recording.steps[step].words;
    match kind {
        'd' => words[column] as u64 | (words[column + 1] as u64) << 32,
        _ => words[column] as u64,
    }
}

pub fn check(recording: &Recording, data: &Path) -> Result<Report, String> {
    let mut report = Report {
        wheel_steps: 0,
        force_steps_matching: 0,
        full_steps_matching: 0,
        tape_steps_closed: 0,
        tape_calls: 0,
        tape_calls_explained: 0,
        tape_tyre_links: 0,
        headline: Vec::new(),
        notes: Vec::new(),
    };
    let wheels = recording.list("wheels");
    let compound: i32 = recording.get("compound").and_then(|v| v.parse().ok()).ok_or("no compound in the header")?;
    let input_names = rig::input_fields();
    let output_names = rig::output_fields();
    let output_kinds: Vec<char> = output_names.iter().map(|n| rig::field_kind(n)).collect();

    for (index, wheel) in wheels.iter().enumerate() {
        let input_columns: Vec<(usize, char)> = input_names
            .iter()
            .map(|n| (recording.col(&format!("tyre.{wheel}.{n}")), rig::field_kind(n)))
            .collect();
        let output_columns: Vec<usize> =
            output_names.iter().map(|n| recording.col(&format!("tyre.{wheel}.{n}"))).collect();
        let rotation_columns: Vec<usize> = (0..16)
            .map(|k| recording.col(&format!("tyre.{wheel}.in_localWheelRotation.M{}{}", k / 4 + 1, k % 4 + 1)))
            .collect();
        let input_at = |step: usize| {
            let words: Vec<u64> = input_columns.iter().map(|&(c, kind)| rig_word(recording, step, c, kind)).collect();
            StepInput::from_words(&words)
        };
        if recording.steps.is_empty() {
            continue;
        }
        // built the way the game builds the car's tyres up to the first step: Tyre::init with
        // the car present (compound 0), the Tyre::reset of Car::forcePosition, then the setup
        // screen's Tyre::setCompound(default compound)
        let first = input_at(0);
        let mut tyre = VanillaTyre::new();
        {
            let mut calls = Vec::new();
            let mut hub = RigHub { input: &first, calls: &mut calls };
            let car = RigCar { input: &first, calls: Vec::new() };
            let car: Option<&dyn TyreCar> = Some(&car);
            tyre.init(&mut hub, data, index as i32, car)?;
            tyre.reset(&mut hub, car);
            if !tyre.set_compound(compound, &mut hub, car) {
                return Err(format!("compound index {compound} does not exist"));
            }
        }
        let mut rig = Rig { tyre };
        let mut first_divergence: Option<String> = None;
        let mut field_mismatches = vec![0usize; output_names.len()];
        let mut ambiguous = 0;
        for step in 0..recording.steps.len() {
            let mut input = input_at(step);
            // The blanket flag is the tyre's own business: never fed in. Wheel speed and the
            // wheel's spin matrix are rewritten between two tyre steps by the drivetrain, for
            // the driven wheels only: those two are fed in for driven wheels (the spin matrix
            // is a whole-car input the single-wheel rig has no word for). A non-driven wheel
            // runs free on the Rust side for the whole recording.
            input.set_blankets = None;
            if input.driven {
                for (k, &column) in rotation_columns.iter().enumerate() {
                    rig.tyre.local_wheel_rotation.m[k / 4][k % 4] =
                        f32::from_bits(recording.steps[step].words[column]);
                }
            } else {
                input.set_angular_velocity = None;
            }
            let got = rig.step(&input);
            let mut forces_same = true;
            let mut all_same = true;
            for (k, name) in output_names.iter().enumerate() {
                let expected = rig_word(recording, step, output_columns[k], output_kinds[k]);
                if !rig::same_value(output_kinds[k], expected, got[k]) {
                    all_same = false;
                    forces_same &= !is_force(name);
                    field_mismatches[k] += 1;
                    if first_divergence.is_none() {
                        first_divergence = Some(format!(
                            "wheel {wheel}: first difference at step {step}, {name}: game {} / Rust {}",
                            rig::describe_word(name, expected),
                            rig::describe_word(name, got[k])
                        ));
                    }
                }
            }
            report.wheel_steps += 1;
            report.force_steps_matching += forces_same as usize;
            report.full_steps_matching += all_same as usize;
            ambiguous += recording.i(step, &format!("tyre.{wheel}.ambiguous")) as usize;
        }
        if let Some(text) = first_divergence {
            report.notes.push(text);
            let mut worst: Vec<(usize, &String)> =
                field_mismatches.iter().copied().zip(&output_names).filter(|(n, _)| *n > 0).collect();
            worst.sort_by(|a, b| b.0.cmp(&a.0));
            let list: Vec<String> = worst.iter().take(8).map(|(n, name)| format!("{name} x{n}")).collect();
            report.notes.push(format!("wheel {wheel}: fields that differed: {}", list.join(", ")));
        }
        if ambiguous > 0 {
            report.notes.push(format!(
                "wheel {wheel}: in {ambiguous} steps the hub gave the tyre two different answers to one question"
            ));
        }
    }
    tape_checks(recording, &wheels, &mut report);
    headline(recording, &mut report);
    floor_clearance(recording, data, &mut report)?;
    Ok(report)
}

/// What one call on the tape does to its body's accumulators: ODE 0.13.1 in single precision,
/// with the operation order of the compiled `dBodyAdd…` functions (r3 brief, section 4.5).
/// `r` is the body's rotation matrix by rows (world = R·local), `pos` its position.
fn apply_call(call: &Call, pos: [f32; 3], r: &[f32], facc: &mut [f32; 3], tacc: &mut [f32; 3]) {
    let rot = |v: [f32; 3]| {
        [
            (v[0] * r[0] + v[1] * r[1]) + v[2] * r[2],
            (v[0] * r[3] + v[1] * r[4]) + v[2] * r[5],
            (v[0] * r[6] + v[1] * r[7]) + v[2] * r[8],
        ]
    };
    fn add(acc: &mut [f32; 3], v: [f32; 3]) {
        for i in 0..3 {
            acc[i] = v[i] + acc[i];
        }
    }
    fn add_cross(tacc: &mut [f32; 3], a: [f32; 3], b: [f32; 3]) {
        tacc[0] = (a[1] * b[2] - a[2] * b[1]) + tacc[0];
        tacc[1] = (a[2] * b[0] - a[0] * b[2]) + tacc[1];
        tacc[2] = (a[0] * b[1] - a[1] * b[0]) + tacc[2];
    }
    let from_centre = |p: [f32; 3]| [p[0] - pos[0], p[1] - pos[1], p[2] - pos[2]];
    let (a, b) = (call.a, call.b);
    match call.kind {
        // addForceAtPos: world force at a world point
        1 => {
            add(facc, a);
            add_cross(tacc, from_centre(b), a);
        }
        // addForceAtLocalPos: world force at a body point
        2 => {
            add(facc, a);
            add_cross(tacc, rot(b), a);
        }
        // addLocalForce: body force at the centre (through the "at body point" function)
        3 => {
            let force = rot(a);
            add(facc, force);
            add_cross(tacc, rot([0.0; 3]), force);
        }
        // addLocalForceAtPos: body force at a world point
        4 => {
            let force = rot(a);
            add(facc, force);
            add_cross(tacc, from_centre(b), force);
        }
        // addLocalForceAtLocalPos: body force at a body point
        5 => {
            let force = rot(a);
            add(facc, force);
            add_cross(tacc, rot(b), force);
        }
        6 => add(tacc, a),
        7 => add(tacc, rot(a)),
        // stop: both accumulators (and the velocities) zeroed
        8 => {
            *facc = [0.0; 3];
            *tacc = [0.0; 3];
        }
        // setVelocity, setAngularVelocity, setPosition, setRotation: no accumulator
        _ => {}
    }
}

/// Checks that tie the force tape to the rest of the recording.
fn tape_checks(recording: &Recording, wheels: &[String], report: &mut Report) {
    let bodies = recording.list("bodies");
    let columns = |b: &str, names: &[&str]| -> Vec<usize> {
        names.iter().map(|n| recording.col(&format!("{b}.{n}"))).collect()
    };
    let acc_columns: Vec<Vec<usize>> =
        bodies.iter().map(|b| columns(b, &["facc.x", "facc.y", "facc.z", "tacc.x", "tacc.y", "tacc.z"])).collect();
    let pos_columns: Vec<Vec<usize>> =
        bodies.iter().map(|b| columns(b, &["pre.pos.x", "pre.pos.y", "pre.pos.z"])).collect();
    let rot_columns: Vec<Vec<usize>> = bodies
        .iter()
        .map(|b| (0..9).map(|k| recording.col(&format!("{b}.pre.R.{k}"))).collect())
        .collect();
    let call_columns: Vec<Vec<[usize; 8]>> = wheels
        .iter()
        .map(|wheel| {
            (0..rig::MAX_CALLS)
                .map(|c| {
                    ["kind", "ax", "ay", "az", "bx", "by", "bz", "flags"]
                        .map(|n| recording.col(&format!("tyre.{wheel}.call{c}.{n}")))
                })
                .collect()
        })
        .collect();
    let count_columns: Vec<usize> = wheels.iter().map(|w| recording.col(&format!("tyre.{w}.calls"))).collect();
    let hubs: Vec<usize> = wheels
        .iter()
        .map(|w| bodies.iter().position(|b| b == &format!("hub_{w}")).unwrap_or(usize::MAX))
        .collect();
    let bits = |v: [f32; 3]| v.map(f32::to_bits);
    let mut first_open = None;
    let mut first_unexplained = None;
    for (step, record) in recording.steps.iter().enumerate() {
        let f = |column: usize| f32::from_bits(record.words[column]);
        // 1. nothing wrote to the accumulators after the last recorded call (ODE leaves them
        //    at zero after a step), and 2. every call explains the change it made
        let mut last = vec![[0u32; 6]; bodies.len()];
        let mut running = vec![([0f32; 3], [0f32; 3]); bodies.len()];
        for (seq, call) in record.calls.iter().enumerate() {
            let b = call.body as usize;
            for k in 0..3 {
                last[b][k] = call.facc[k].to_bits();
                last[b][3 + k] = call.tacc[k].to_bits();
            }
            let pos = [f(pos_columns[b][0]), f(pos_columns[b][1]), f(pos_columns[b][2])];
            let r: Vec<f32> = rot_columns[b].iter().map(|&c| f(c)).collect();
            let (facc, tacc) = &mut running[b];
            apply_call(call, pos, &r, facc, tacc);
            let explained = bits(*facc) == bits(call.facc) && bits(*tacc) == bits(call.tacc);
            report.tape_calls += 1;
            report.tape_calls_explained += explained as usize;
            if !explained && first_unexplained.is_none() {
                first_unexplained = Some((step, seq));
            }
            // go on from what the game really had, so that one miss is counted once
            *facc = call.facc;
            *tacc = call.tacc;
        }
        let closed = (0..bodies.len()).all(|b| (0..6).all(|k| last[b][k] == record.words[acc_columns[b][k]]));
        report.tape_steps_closed += closed as usize;
        if !closed && first_open.is_none() {
            first_open = Some(step);
        }
        // 3. the tyre's own list of hub calls against the tape's "tyre" entries on that hub
        for (i, _) in wheels.iter().enumerate() {
            let count = (record.words[count_columns[i]] as usize).min(rig::MAX_CALLS);
            // capture: (tape kind, force, point); 1 = force at a point, 2 = torque
            let mut captured = Vec::new();
            let mut comparable = true;
            for c in 0..count {
                let w = call_columns[i][c].map(|column| record.words[column]);
                match w[0] {
                    1 => captured.push((1u32, [w[1], w[2], w[3]], [w[4], w[5], w[6]])),
                    2 => captured.push((6u32, [w[1], w[2], w[3]], [0f32.to_bits(); 3])),
                    // 4 is the tyre's push on the car body (surface drag), not a hub call
                    4 => {}
                    // 3 (force and torque in hub axes) becomes several hub calls
                    _ => comparable = false,
                }
            }
            let on_tape: Vec<(u32, [u32; 3], [u32; 3])> = record
                .calls
                .iter()
                .filter(|call| {
                    call.body as usize == hubs[i] && call.outer_site != 0 && recording.system_of(call) == "tyre"
                })
                .map(|call| (call.kind, bits(call.a), bits(call.b)))
                .collect();
            report.tape_tyre_links += (comparable && captured == on_tape) as usize;
        }
    }
    if let Some(step) = first_open {
        report.notes.push(format!("force tape: the accumulators do not close at step {step}"));
    }
    if let Some((step, seq)) = first_unexplained {
        report.notes.push(format!("force tape: call {seq} of step {step} does not explain the accumulators after it"));
    }
}

/// A few numbers a person can judge.
fn headline(recording: &Recording, report: &mut Report) {
    let n = recording.steps.len();
    if n == 0 {
        return;
    }
    let from = SETTLE_STEPS.min(n - 1);
    let mut top_speed = 0f32;
    let mut max_rpm = 0f32;
    let mut top_gear = 0;
    let (mut acc_min, mut acc_max) = ([0f32; 3], [0f32; 3]);
    let mut max_calls = 0;
    let mut contact_points = 0;
    let mut extra_joints = 0;
    for step in 0..n {
        top_speed = top_speed.max(recording.f(step, "car.speed") * 3.6);
        max_rpm = max_rpm.max(recording.f(step, "drivetrain.engineRPM"));
        top_gear = top_gear.max(recording.i(step, "drivetrain.currentGear") - 1);
        // accG is a difference of velocities: a step on which the sleeping rule zeroes the
        // body's velocity (a `stop` entry on the tape) shows a jump that is not a force
        let frozen = recording.steps[step].calls.iter().any(|call| call.kind == 8);
        if step >= from && !frozen {
            for (k, axis) in ["x", "y", "z"].iter().enumerate() {
                let g = recording.f(step, &format!("car.accG.{axis}"));
                acc_min[k] = acc_min[k].min(g);
                acc_max[k] = acc_max[k].max(g);
            }
        }
        max_calls = max_calls.max(recording.steps[step].calls.len());
        contact_points += recording.i(step, "world.contactPoints") as i64;
        extra_joints += (recording.i(step, "world.joints") != 21) as usize;
    }
    let last = n - 1;
    let loads: Vec<f32> =
        ["lf", "rf", "lr", "rr"].iter().map(|w| recording.f(last, &format!("tyre.{w}.status.load"))).collect();
    report.headline.push(format!("top speed {top_speed:.1} km/h"));
    report.headline.push(format!("max {max_rpm:.0} rpm, top gear {top_gear}"));
    report.headline.push(format!(
        "accG long {:+.2} … {:+.2}, lat {:+.2} … {:+.2}",
        acc_min[2], acc_max[2], acc_min[0], acc_max[0]
    ));
    report.headline.push(format!(
        "end: {:.1} km/h, body height {:.4} m, wheel loads {:.0}/{:.0}/{:.0}/{:.0} N",
        recording.f(last, "car.speed") * 3.6,
        recording.f(last, "body.post.pos.y"),
        loads[0],
        loads[1],
        loads[2],
        loads[3]
    ));
    report.headline.push(format!("up to {max_calls} force calls per step"));
    let flat_spot = (0..n)
        .flat_map(|step| ["lf", "rf", "lr", "rr"].map(|w| recording.d(step, &format!("tyre.{w}.status.flatSpot"))))
        .fold(0.0f64, f64::max);
    report.headline.push(format!(
        "engine life {:.0} -> {:.0}, largest tyre flat spot {:.3}, fuel {:.2} -> {:.2} l",
        recording.d(0, "engine.lifeLeft"),
        recording.d(last, "engine.lifeLeft"),
        flat_spot,
        recording.d(0, "car.fuel"),
        recording.d(last, "car.fuel")
    ));
    if contact_points != 0 || extra_joints != 0 {
        report.notes.push(format!(
            "ODE contacts: {contact_points} contact points in all, {extra_joints} steps with contact joints"
        ));
    }
}

/// The car's collision boxes (`colliders.ini`) never meet anything in the oracle, because the
/// fake road only answers the tyres' rays. In the game they would rest on the road mesh, so a
/// recording is only what the game would do while every box stays above the road: this
/// reports the smallest gap (negative = a box corner went below the road).
fn floor_clearance(recording: &Recording, data: &Path, report: &mut Report) -> Result<(), String> {
    let path = data.join("colliders.ini");
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let vector = |line: &str| -> Option<[f32; 3]> {
        let value = line.split_once('=')?.1.split(';').next()?;
        let mut parts = value.split(',').map(|x| x.trim().parse::<f32>());
        Some([parts.next()?.ok()?, parts.next()?.ok()?, parts.next()?.ok()?])
    };
    let mut boxes: Vec<([f32; 3], [f32; 3])> = Vec::new();
    let mut centre = None;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with("CENTRE") {
            centre = vector(line);
        } else if line.starts_with("SIZE") {
            if let (Some(c), Some(size)) = (centre.take(), vector(line)) {
                boxes.push((c, size));
            }
        }
    }
    let ground = recording.get("ground").and_then(Ground::parse).ok_or("no ground in the header")?;
    let pos: Vec<usize> = ["x", "y", "z"].iter().map(|a| recording.col(&format!("body.post.pos.{a}"))).collect();
    let rot: Vec<usize> = (0..9).map(|k| recording.col(&format!("body.post.R.{k}"))).collect();
    let has_floor = recording.get("floor_mesh") == Some("1");
    // [whole run, after the game's no-collision window]: smallest gap, its step, steps with a
    // box below the road
    let mut lowest = [f32::INFINITY; 2];
    let mut at = [0usize; 2];
    let mut below = [0usize; 2];
    for (step, record) in recording.steps.iter().enumerate() {
        let f = |column: usize| f32::from_bits(record.words[column]);
        let p = [f(pos[0]), f(pos[1]), f(pos[2])];
        let r: Vec<f32> = rot.iter().map(|&c| f(c)).collect();
        let mut step_gap = f32::INFINITY;
        for (c, size) in &boxes {
            for corner in 0..8 {
                let local = [
                    c[0] + if corner & 1 == 0 { -0.5 } else { 0.5 } * size[0],
                    c[1] + if corner & 2 == 0 { -0.5 } else { 0.5 } * size[1],
                    c[2] + if corner & 4 == 0 { -0.5 } else { 0.5 } * size[2],
                ];
                let world = |row: usize| p[row] + r[row * 3] * local[0] + r[row * 3 + 1] * local[1] + r[row * 3 + 2] * local[2];
                step_gap = step_gap.min(world(1) - ground.height(world(0), world(2)));
            }
        }
        for phase in 0..2 {
            if phase == 1 && step < NO_COLLISION_STEPS {
                continue;
            }
            below[phase] += (step_gap < 0.0) as usize;
            if step_gap < lowest[phase] {
                lowest[phase] = step_gap;
                at[phase] = step;
            }
        }
    }
    report.headline.push(format!(
        "gap under the body's collision boxes: {:.1} mm at its smallest (step {}){}",
        lowest[0] * 1000.0,
        at[0],
        if lowest[1].is_finite() {
            format!(", {:.1} mm from step {NO_COLLISION_STEPS} on (step {})", lowest[1] * 1000.0, at[1])
        } else {
            String::new()
        }
    ));
    if below[1] > 0 && !has_floor {
        report.notes.push(format!(
            "in {} steps after the first {NO_COLLISION_STEPS} (deepest {:.1} mm, at step {}) a collision box of \
             the body is below the road: in the game the floor would touch the road there; this recording has no \
             collision mesh, so it does not",
            below[1],
            -lowest[1] * 1000.0,
            at[1]
        ));
    }
    let _ = below[0];
    Ok(())
}
