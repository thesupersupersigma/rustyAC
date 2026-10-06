//! Checks on a finished recording: the tyres replayed through the Rust port, and a few
//! headline numbers for a plausibility look.

use std::path::Path;

use rustyac_physics::tyre::rig::{self, Rig, RigCar, RigHub, StepInput};
use rustyac_physics::tyre::{TyreCar, VanillaTyre};

use crate::record::Recording;
use crate::scenario::SETTLE_STEPS;

pub struct Report {
    /// Steps times four wheels.
    pub wheel_steps: usize,
    /// Wheel-steps in which every force the tyre handed to its hub and the car body, and its
    /// Fx / Fy / Mz / load, are bit-identical in the game and in the Rust port.
    pub force_steps_matching: usize,
    /// Wheel-steps in which every recorded tyre value is.
    pub full_steps_matching: usize,
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
        let input_at = |step: usize| {
            let words: Vec<u64> = input_columns.iter().map(|&(c, kind)| rig_word(recording, step, c, kind)).collect();
            StepInput::from_words(&words)
        };
        if recording.steps.is_empty() {
            continue;
        }
        // built the way the car builds its tyres: Tyre::init with the car present, the default
        // compound, and the Tyre::reset of Car::forcePosition
        let first = input_at(0);
        let mut tyre = VanillaTyre::new();
        {
            let mut calls = Vec::new();
            let mut hub = RigHub { input: &first, calls: &mut calls };
            let car = RigCar { input: &first, calls: Vec::new() };
            let car: Option<&dyn TyreCar> = Some(&car);
            tyre.init(&mut hub, data, index as i32, car)?;
            if compound != 0 && !tyre.set_compound(compound, &mut hub, car) {
                return Err(format!("compound index {compound} does not exist"));
            }
            tyre.reset(&mut hub, car);
        }
        let mut rig = Rig { tyre };
        let mut first_divergence: Option<String> = None;
        let mut field_mismatches = vec![0usize; output_names.len()];
        let mut ambiguous = 0;
        for step in 0..recording.steps.len() {
            let input = input_at(step);
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
    headline(recording, &mut report);
    Ok(report)
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
        if step >= from {
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
    if contact_points != 0 || extra_joints != 0 {
        report.notes.push(format!(
            "ODE contacts appeared: {contact_points} contact points, {extra_joints} steps with extra joints"
        ));
    }
}
