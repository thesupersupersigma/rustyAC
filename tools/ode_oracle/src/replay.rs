//! Replay of the car_oracle recordings (`oracle/car/*.carrec`) through the Rust port.
//!
//! The recordings hold, for every step of the game's own car: the state of the six bodies
//! when `dWorldStep` started and when it ended, the force and torque accumulators at its
//! start, every call that built them (the force tape), the joints' parameters and the
//! constraint forces. Two tests are made from that:
//!
//! * **per step**: load the recorded start state of step N, the recorded accumulators and
//!   joint parameters, step once in Rust, compare with the recorded end state;
//! * **free run**: load the start state of step 0 only, then run the whole scenario in
//!   Rust, feeding the force tape call by call through the Rust `dBodyAdd…` functions (plus
//!   what else the car changes from outside: joint parameters when the steering moves a rod
//!   or the softness rule switches, body mass when fuel burns, the "stop" of the sleeping
//!   rule). The state is never reset.

use std::path::{Path, PathBuf};

use crate::record::{Call, Recording};
use rustyac_ode::{BodyId, JointId, JointKind, Mass, World};

const H: f32 = 0.003;

/// The recorded car as a Rust world, built in the game's creation order (which decides the
/// island and row order): car body, fuel tank, the fixed joint (tank, body), four hubs,
/// five rods per wheel (body, hub).
pub struct CarWorld {
    pub world: World,
    pub bodies: Vec<BodyId>,
    pub joints: Vec<JointId>,
    pub body_names: Vec<String>,
    pub joint_names: Vec<String>,
    /// ODE type number of each joint (7 fixed, 15 DBall).
    pub joint_types: Vec<u32>,
    pub joint_bodies: Vec<(usize, usize)>,
}

fn body_columns(recording: &Recording, body: &str, part: &str) -> [usize; 16] {
    // pos 3, q 4, R 9: the first words of each group (they are stored contiguously)
    let mut c = [0usize; 16];
    for (k, axis) in ["x", "y", "z"].iter().enumerate() {
        c[k] = recording.col(&format!("{body}.{part}.pos.{axis}"));
    }
    for (k, axis) in ["w", "x", "y", "z"].iter().enumerate() {
        c[3 + k] = recording.col(&format!("{body}.{part}.q.{axis}"));
    }
    for k in 0..9 {
        c[7 + k] = recording.col(&format!("{body}.{part}.R.{k}"));
    }
    c
}

fn vec3_columns(recording: &Recording, name: &str) -> [usize; 3] {
    ["x", "y", "z"].map(|axis| recording.col(&format!("{name}.{axis}")))
}

/// Columns of everything the replay reads, looked up once.
pub struct Columns {
    pre: Vec<[usize; 16]>,
    post: Vec<[usize; 16]>,
    pre_lvel: Vec<[usize; 3]>,
    pre_avel: Vec<[usize; 3]>,
    post_lvel: Vec<[usize; 3]>,
    post_avel: Vec<[usize; 3]>,
    facc: Vec<[usize; 3]>,
    tacc: Vec<[usize; 3]>,
    mass: Vec<usize>,
    inertia: Vec<[usize; 3]>,
    body_tag: Vec<usize>,
    /// first column of each joint's parameter block and how many values it has
    joint_params: Vec<Vec<usize>>,
    joint_tag: Vec<usize>,
    joint_force: Vec<[usize; 12]>,
}

impl CarWorld {
    pub fn build(recording: &Recording) -> Result<(CarWorld, Columns), String> {
        let body_names = recording.list("bodies");
        let mut world = World::assetto_corsa();
        let mut bodies = Vec::new();
        for _ in &body_names {
            // RigidBodyODE::RigidBodyODE: finite rotation on, axis (0,0,0), damping 0
            let b = world.body_create();
            world.body_set_finite_rotation_mode(b, true);
            world.body_set_finite_rotation_axis(b, 0.0, 0.0, 0.0);
            world.body_set_linear_damping(b, 0.0);
            world.body_set_angular_damping(b, 0.0);
            bodies.push(b);
        }
        let mut joints = Vec::new();
        let mut joint_names = Vec::new();
        let mut joint_types = Vec::new();
        let mut joint_bodies = Vec::new();
        for text in recording.list("joints") {
            let parts: Vec<&str> = text.split(':').collect();
            if parts.len() != 4 {
                return Err(format!("joint entry {text:?} in the recording's header"));
            }
            let index_of = |name: &str| {
                body_names.iter().position(|b| b == name).ok_or_else(|| format!("joint {text}: no body {name}"))
            };
            let (b1, b2) = (index_of(parts[2])?, index_of(parts[3])?);
            let kind: u32 = parts[1].parse().map_err(|_| format!("joint type in {text:?}"))?;
            let j = match kind {
                7 => world.joint_create_fixed(),
                15 => world.joint_create_dball(),
                other => return Err(format!("joint {text}: ODE type {other} is not in the recordings' car")),
            };
            world.joint_attach(j, Some(bodies[b1]), Some(bodies[b2]));
            world.joint_set_feedback(j, true);
            joints.push(j);
            joint_names.push(parts[0].to_string());
            joint_types.push(kind);
            joint_bodies.push((b1, b2));
        }
        let mut columns = Columns {
            pre: Vec::new(),
            post: Vec::new(),
            pre_lvel: Vec::new(),
            pre_avel: Vec::new(),
            post_lvel: Vec::new(),
            post_avel: Vec::new(),
            facc: Vec::new(),
            tacc: Vec::new(),
            mass: Vec::new(),
            inertia: Vec::new(),
            body_tag: Vec::new(),
            joint_params: Vec::new(),
            joint_tag: Vec::new(),
            joint_force: Vec::new(),
        };
        for b in &body_names {
            columns.pre.push(body_columns(recording, b, "pre"));
            columns.post.push(body_columns(recording, b, "post"));
            columns.pre_lvel.push(vec3_columns(recording, &format!("{b}.pre.lvel")));
            columns.pre_avel.push(vec3_columns(recording, &format!("{b}.pre.avel")));
            columns.post_lvel.push(vec3_columns(recording, &format!("{b}.post.lvel")));
            columns.post_avel.push(vec3_columns(recording, &format!("{b}.post.avel")));
            columns.facc.push(vec3_columns(recording, &format!("{b}.facc")));
            columns.tacc.push(vec3_columns(recording, &format!("{b}.tacc")));
            columns.mass.push(recording.col(&format!("{b}.mass")));
            columns.inertia.push(vec3_columns(recording, &format!("{b}.inertia")));
            columns.body_tag.push(recording.col(&format!("{b}.tag")));
        }
        for (name, kind) in joint_names.iter().zip(&joint_types) {
            let n = format!("joint.{name}");
            let fields: Vec<String> = if *kind == 15 {
                ["anchor1.x", "anchor1.y", "anchor1.z", "anchor2.x", "anchor2.y", "anchor2.z", "erp", "cfm", "distance"]
                    .iter()
                    .map(|f| format!("{n}.{f}"))
                    .collect()
            } else {
                ["qrel.w", "qrel.x", "qrel.y", "qrel.z", "offset.x", "offset.y", "offset.z", "erp", "cfm"]
                    .iter()
                    .map(|f| format!("{n}.{f}"))
                    .collect()
            };
            columns.joint_params.push(fields.iter().map(|f| recording.col(f)).collect());
            columns.joint_tag.push(recording.col(&format!("{n}.tag")));
            let mut force = [0usize; 12];
            for (g, group) in ["f1", "t1", "f2", "t2"].iter().enumerate() {
                for (k, axis) in ["x", "y", "z"].iter().enumerate() {
                    force[3 * g + k] = recording.col(&format!("{n}.{group}.{axis}"));
                }
            }
            columns.joint_force.push(force);
        }
        Ok((CarWorld { world, bodies, joints, body_names, joint_names, joint_types, joint_bodies }, columns))
    }

    /// Writes a recorded state (the 16 pose columns plus velocities) into a body.
    fn load_state(&mut self, words: &[u32], b: usize, pose: &[usize; 16], lvel: &[usize; 3], avel: &[usize; 3]) {
        let f = |c: usize| f32::from_bits(words[c]);
        let body = self.world.body_mut(self.bodies[b]);
        for k in 0..3 {
            body.pos[k] = f(pose[k]);
            body.lvel[k] = f(lvel[k]);
            body.avel[k] = f(avel[k]);
        }
        for k in 0..4 {
            body.q[k] = f(pose[3 + k]);
        }
        for row in 0..3 {
            for col in 0..3 {
                body.r[4 * row + col] = f(pose[7 + 3 * row + col]);
            }
        }
    }

    /// What the car changes from outside between two steps besides forces: body masses
    /// (fuel) and joint parameters (steering rod anchors, the softness rule).
    fn load_parameters(&mut self, words: &[u32], columns: &Columns) {
        let f = |c: usize| f32::from_bits(words[c]);
        for b in 0..self.bodies.len() {
            let mass = f(columns.mass[b]);
            let inertia = columns.inertia[b].map(f);
            let body = self.world.body(self.bodies[b]);
            let current = [body.mass.i[0], body.mass.i[5], body.mass.i[10]];
            if body.mass.mass.to_bits() != mass.to_bits()
                || current.map(f32::to_bits) != inertia.map(f32::to_bits)
            {
                // RigidBodyODE::setMassBox: a box inertia (diagonal), then dBodySetMass
                let mut m = Mass::zero();
                m.mass = mass;
                m.i[0] = inertia[0];
                m.i[5] = inertia[1];
                m.i[10] = inertia[2];
                self.world.body_set_mass(self.bodies[b], &m);
            }
        }
        for j in 0..self.joints.len() {
            let p: Vec<f32> = columns.joint_params[j].iter().map(|&c| f(c)).collect();
            match &mut self.world.joint_mut(self.joints[j]).kind {
                JointKind::DBall { anchor1, anchor2, erp, cfm, target_distance } => {
                    anchor1[..3].copy_from_slice(&p[0..3]);
                    anchor2[..3].copy_from_slice(&p[3..6]);
                    *erp = p[6];
                    *cfm = p[7];
                    *target_distance = p[8];
                }
                JointKind::Fixed { qrel, offset, erp, cfm } => {
                    qrel.copy_from_slice(&p[0..4]);
                    offset[..3].copy_from_slice(&p[4..7]);
                    *erp = p[7];
                    *cfm = p[8];
                }
                _ => {}
            }
        }
    }

    /// One call of the force tape through the Rust API (kinds as in car_oracle's record.rs).
    fn apply_call(&mut self, call: &Call) -> Result<(), String> {
        let b = self.bodies[call.body as usize];
        match call.kind {
            1 => self.world.body_add_force_at_pos(b, call.a, call.b),
            2 => self.world.body_add_force_at_rel_pos(b, call.a, call.b),
            // RigidBodyODE::addLocalForce is dBodyAddRelForceAtRelPos(force, 0, 0, 0)
            3 => self.world.body_add_rel_force_at_rel_pos(b, call.a, [0.0; 3]),
            4 => self.world.body_add_rel_force_at_pos(b, call.a, call.b),
            5 => self.world.body_add_rel_force_at_rel_pos(b, call.a, call.b),
            6 => self.world.body_add_torque(b, call.a),
            7 => self.world.body_add_rel_torque(b, call.a),
            8 => {
                // RigidBodyODE::stop: velocities and accumulators to zero
                self.world.body_set_linear_vel(b, 0.0, 0.0, 0.0);
                self.world.body_set_angular_vel(b, 0.0, 0.0, 0.0);
                self.world.body_set_force(b, 0.0, 0.0, 0.0);
                self.world.body_set_torque(b, 0.0, 0.0, 0.0);
            }
            9 => self.world.body_set_linear_vel(b, call.a[0], call.a[1], call.a[2]),
            10 => self.world.body_set_angular_vel(b, call.a[0], call.a[1], call.a[2]),
            11 => self.world.body_set_position(b, call.a[0], call.a[1], call.a[2]),
            other => return Err(format!("tape call of kind {other} cannot be replayed")),
        }
        Ok(())
    }

    /// Compares the Rust state after a step with the recorded end state of that step.
    /// Returns the name of the first field that differs, with both values.
    fn compare(&self, words: &[u32], columns: &Columns, with_tags: bool) -> Option<String> {
        let f = |c: usize| f32::from_bits(words[c]);
        let differ = |a: f32, b: f32| a.to_bits() != b.to_bits() && !(a.is_nan() && b.is_nan());
        let report = |name: String, game: f32, rust: f32| {
            Some(format!("{name}: game {game:?} ({:#010x}) / Rust {rust:?} ({:#010x})", game.to_bits(), rust.to_bits()))
        };
        for b in 0..self.bodies.len() {
            let body = self.world.body(self.bodies[b]);
            let name = &self.body_names[b];
            let post = &columns.post[b];
            for k in 0..3 {
                if differ(f(post[k]), body.pos[k]) {
                    return report(format!("{name}.pos[{k}]"), f(post[k]), body.pos[k]);
                }
            }
            for k in 0..4 {
                if differ(f(post[3 + k]), body.q[k]) {
                    return report(format!("{name}.q[{k}]"), f(post[3 + k]), body.q[k]);
                }
            }
            for row in 0..3 {
                for col in 0..3 {
                    let (game, rust) = (f(post[7 + 3 * row + col]), body.r[4 * row + col]);
                    if differ(game, rust) {
                        return report(format!("{name}.R[{row}][{col}]"), game, rust);
                    }
                }
            }
            for k in 0..3 {
                if differ(f(columns.post_lvel[b][k]), body.lvel[k]) {
                    return report(format!("{name}.lvel[{k}]"), f(columns.post_lvel[b][k]), body.lvel[k]);
                }
                if differ(f(columns.post_avel[b][k]), body.avel[k]) {
                    return report(format!("{name}.avel[{k}]"), f(columns.post_avel[b][k]), body.avel[k]);
                }
            }
            if with_tags && words[columns.body_tag[b]] as i32 != body.tag {
                return Some(format!("{name}.tag: game {} / Rust {}", words[columns.body_tag[b]] as i32, body.tag));
            }
        }
        for j in 0..self.joints.len() {
            let joint = self.world.joint(self.joints[j]);
            let fb = joint.feedback.unwrap_or_default();
            let rust = [
                fb.f1[0], fb.f1[1], fb.f1[2], fb.t1[0], fb.t1[1], fb.t1[2], fb.f2[0], fb.f2[1], fb.f2[2], fb.t2[0],
                fb.t2[1], fb.t2[2],
            ];
            for k in 0..12 {
                let game = f(columns.joint_force[j][k]);
                if differ(game, rust[k]) {
                    let part = ["f1", "t1", "f2", "t2"][k / 3];
                    return report(format!("joint {} {part}[{}]", self.joint_names[j], k % 3), game, rust[k]);
                }
            }
            if with_tags && words[columns.joint_tag[j]] as i32 != joint.tag {
                return Some(format!(
                    "joint {} tag: game {} / Rust {}",
                    self.joint_names[j], words[columns.joint_tag[j]] as i32, joint.tag
                ));
            }
        }
        None
    }

    /// Largest position difference (metres) between the Rust bodies and the recorded end state.
    fn position_error(&self, words: &[u32], columns: &Columns) -> f32 {
        let mut worst = 0.0f32;
        for b in 0..self.bodies.len() {
            let body = self.world.body(self.bodies[b]);
            for k in 0..3 {
                worst = worst.max((f32::from_bits(words[columns.post[b][k]]) - body.pos[k]).abs());
            }
        }
        worst
    }
}

pub struct ReplayResult {
    pub scenario: String,
    pub steps: usize,
    /// Steps in which the game's world held contact joints (the body's collision boxes on a
    /// road mesh): stage 2 of the port, outside what stage 1 can reproduce.
    pub contact_steps: usize,
    /// Bit-exact steps among the contact-free ones.
    pub per_step_exact: usize,
    pub per_step_first: Option<String>,
    pub free_run_first: Option<String>,
    pub free_run_accumulators_exact: usize,
    pub free_run_end_error: f32,
}

pub fn replay(recording: &Recording) -> Result<ReplayResult, String> {
    let scenario = recording.get("scenario").unwrap_or("?").to_string();
    let steps = recording.steps.len();

    // per-step test
    let (mut car, columns) = CarWorld::build(recording)?;
    let mut per_step_exact = 0;
    let mut per_step_first = None;
    let world_joints = recording.col("world.joints");
    let has_contacts = |words: &[u32]| words[world_joints] as usize != car_joint_count(recording);
    let contact_steps = recording.steps.iter().filter(|s| has_contacts(&s.words)).count();
    for (n, step) in recording.steps.iter().enumerate() {
        let words = &step.words;
        if has_contacts(words) {
            continue;
        }
        for b in 0..car.bodies.len() {
            car.load_state(words, b, &columns.pre[b], &columns.pre_lvel[b], &columns.pre_avel[b]);
            let body = car.world.body_mut(car.bodies[b]);
            for k in 0..3 {
                body.facc[k] = f32::from_bits(words[columns.facc[b][k]]);
                body.tacc[k] = f32::from_bits(words[columns.tacc[b][k]]);
            }
        }
        car.load_parameters(words, &columns);
        car.world.step(H);
        match car.compare(words, &columns, true) {
            None => per_step_exact += 1,
            Some(text) => {
                if per_step_first.is_none() {
                    per_step_first = Some(format!("step {n}: {text}"));
                }
            }
        }
    }

    // free run: the start state of step 0, then only what the car does from outside
    let (mut car, columns) = CarWorld::build(recording)?;
    let mut free_run_first = None;
    let mut accumulators_exact = 0;
    let mut end_error = 0.0f32;
    if let Some(first) = recording.steps.first() {
        for b in 0..car.bodies.len() {
            car.load_state(&first.words, b, &columns.pre[b], &columns.pre_lvel[b], &columns.pre_avel[b]);
        }
    }
    for (n, step) in recording.steps.iter().enumerate() {
        let words = &step.words;
        car.load_parameters(words, &columns);
        for call in &step.calls {
            car.apply_call(call)?;
        }
        // the accumulators the Rust add-force functions built must be the recorded ones
        let mut same = true;
        for b in 0..car.bodies.len() {
            let body = car.world.body(car.bodies[b]);
            for k in 0..3 {
                same &= body.facc[k].to_bits() == words[columns.facc[b][k]];
                same &= body.tacc[k].to_bits() == words[columns.tacc[b][k]];
            }
        }
        accumulators_exact += same as usize;
        if !same && free_run_first.is_none() {
            free_run_first = Some(format!("step {n}: the accumulators built from the force tape"));
        }
        car.world.step(H);
        if free_run_first.is_none() {
            if let Some(text) = car.compare(words, &columns, true) {
                free_run_first = Some(if has_contacts(words) {
                    format!("step {n}, the first step with floor contact joints (stage 2): {text}")
                } else {
                    format!("step {n}: {text}")
                });
            }
        }
        if n + 1 == steps {
            end_error = car.position_error(words, &columns);
        }
    }
    Ok(ReplayResult {
        scenario,
        steps,
        contact_steps,
        per_step_exact,
        per_step_first,
        free_run_first,
        free_run_accumulators_exact: accumulators_exact,
        free_run_end_error: end_error,
    })
}

/// Number of joints of the car itself (the recording's header list).
fn car_joint_count(recording: &Recording) -> usize {
    recording.list("joints").len()
}

fn percent(part: usize, whole: usize) -> String {
    if part == whole {
        format!("100 % ({part}/{whole})")
    } else {
        format!("{:.4} % ({part}/{whole})", 100.0 * part as f64 / whole.max(1) as f64)
    }
}

pub fn replay_command(files: &[PathBuf], verbose: bool) -> Result<(), String> {
    let repo = crate::repo_root();
    let mut files: Vec<PathBuf> = files.to_vec();
    if files.is_empty() {
        let dir = repo.join("oracle/car");
        let mut found: Vec<PathBuf> = std::fs::read_dir(&dir)
            .map_err(|e| format!("{}: {e} (run `car_oracle all` first)", dir.display()))?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|x| x == "carrec"))
            .collect();
        found.sort();
        files = found;
    }
    let mut table = String::from(
        "| scenario | steps | steps with contact joints (stage 2) | per-step: bit-exact among the contact-free steps \
         | free run: first difference | free run: accumulators rebuilt from the force tape | free run: largest \
         position error at the end, m |\n|---|---|---|---|---|---|---|\n",
    );
    let mut failed = false;
    let (mut total_steps, mut total_exact, mut total_contact) = (0, 0, 0);
    for file in &files {
        let recording = Recording::read(file)?;
        let result = replay(&recording)?;
        let stage1_steps = result.steps - result.contact_steps;
        failed |= result.per_step_exact != stage1_steps;
        // a free run may only leave the recording where the game had contacts
        failed |= result.contact_steps == 0 && result.free_run_first.is_some();
        total_steps += result.steps;
        total_contact += result.contact_steps;
        total_exact += result.per_step_exact;
        let line = format!(
            "| `{}` | {} | {} | {} | {} | {} | {} |",
            result.scenario,
            result.steps,
            result.contact_steps,
            percent(result.per_step_exact, stage1_steps),
            result.free_run_first.clone().unwrap_or_else(|| "none".into()),
            percent(result.free_run_accumulators_exact, result.steps),
            if result.free_run_end_error == 0.0 { "0".to_string() } else { format!("{:.3}", result.free_run_end_error) },
        );
        println!("{line}");
        if verbose {
            if let Some(text) = &result.per_step_first {
                println!("    per-step, first difference: {text}");
            }
        }
        table.push_str(&line);
        table.push('\n');
    }
    table.push_str(&format!(
        "| **all** | **{total_steps}** | **{total_contact}** | **{}** | | | |\n",
        percent(total_exact, total_steps - total_contact)
    ));
    let out = repo.join("oracle/ode");
    std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
    let path = out.join("replay_results.md");
    std::fs::write(&path, &table).map_err(|e| format!("{}: {e}", path.display()))?;
    println!("wrote {}", path.display());
    if failed {
        return Err("at least one step of at least one recording differs".into());
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// Golden excerpts for `cargo test` (format documented in crates/rustyac-ode/tests/golden_car.rs)

fn push_f(out: &mut Vec<u8>, v: f32) {
    out.extend_from_slice(&v.to_bits().to_le_bytes());
}

fn push_u(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_le_bytes());
}

/// Writes `count` steps of a recording, starting at `first`, as a self-contained excerpt:
/// start state, then per step what the car changed from outside (stop, mass, joint
/// parameters, accumulators) and the state the game's ODE produced.
fn write_excerpt(recording: &Recording, first: usize, count: usize, path: &Path) -> Result<usize, String> {
    let (car, columns) = CarWorld::build(recording)?;
    let (nb, nj) = (car.bodies.len(), car.joints.len());
    let mut out = Vec::new();
    out.extend_from_slice(b"ODEGOLD1");
    push_u(&mut out, nb as u32);
    push_u(&mut out, nj as u32);
    push_u(&mut out, count as u32);
    push_u(&mut out, first as u32);
    for v in [0.0f32, -9.806, 0.0, 0.3, 1e-7, H] {
        push_f(&mut out, v);
    }
    for j in 0..nj {
        push_u(&mut out, car.joint_types[j]);
        push_u(&mut out, car.joint_bodies[j].0 as u32);
        push_u(&mut out, car.joint_bodies[j].1 as u32);
    }
    let start = &recording.steps[first].words;
    for b in 0..nb {
        for &c in columns.pre[b].iter().chain(&columns.pre_lvel[b]).chain(&columns.pre_avel[b]) {
            push_u(&mut out, start[c]);
        }
    }
    let mut last_mass: Vec<[u32; 4]> = vec![[0; 4]; nb];
    let mut last_params: Vec<Vec<u32>> = vec![Vec::new(); nj];
    for step in &recording.steps[first..first + count] {
        let words = &step.words;
        // bodies stopped by the car before this step
        let mut stop_mask = 0u32;
        for call in &step.calls {
            match call.kind {
                1..=7 => {}
                8 => stop_mask |= 1 << call.body,
                other => return Err(format!("the excerpt cannot hold a tape call of kind {other}")),
            }
        }
        push_u(&mut out, stop_mask);
        // masses that changed
        let mut changed = Vec::new();
        for b in 0..nb {
            let now = [
                words[columns.mass[b]],
                words[columns.inertia[b][0]],
                words[columns.inertia[b][1]],
                words[columns.inertia[b][2]],
            ];
            if now != last_mass[b] {
                changed.push((b, now));
                last_mass[b] = now;
            }
        }
        push_u(&mut out, changed.len() as u32);
        for (b, now) in changed {
            push_u(&mut out, b as u32);
            for w in now {
                push_u(&mut out, w);
            }
        }
        // joint parameters that changed (nine values each)
        let mut changed = Vec::new();
        for j in 0..nj {
            let now: Vec<u32> = columns.joint_params[j].iter().map(|&c| words[c]).collect();
            if now != last_params[j] {
                changed.push((j, now.clone()));
                last_params[j] = now;
            }
        }
        push_u(&mut out, changed.len() as u32);
        for (j, now) in changed {
            push_u(&mut out, j as u32);
            for w in now {
                push_u(&mut out, w);
            }
        }
        // accumulators at the start of dWorldStep
        for b in 0..nb {
            for &c in columns.facc[b].iter().chain(&columns.tacc[b]) {
                push_u(&mut out, words[c]);
            }
        }
        // what the game's ODE made of it: pos, q, lvel, avel of every body ...
        for b in 0..nb {
            for &c in columns.post[b][..7].iter().chain(&columns.post_lvel[b]).chain(&columns.post_avel[b]) {
                push_u(&mut out, words[c]);
            }
        }
        // ... and the joints' constraint forces as an FNV-1a 64 hash of their bits
        let mut hash = 0xcbf2_9ce4_8422_2325u64;
        for j in 0..nj {
            for &c in &columns.joint_force[j] {
                for byte in words[c].to_le_bytes() {
                    hash ^= byte as u64;
                    hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
                }
            }
        }
        out.extend_from_slice(&hash.to_le_bytes());
    }
    std::fs::write(path, &out).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(out.len())
}

pub fn excerpt_command() -> Result<(), String> {
    let repo = crate::repo_root();
    let data = repo.join("crates/rustyac-ode/tests/data");
    std::fs::create_dir_all(&data).map_err(|e| e.to_string())?;
    // settle: the spawn drop and the freeze by the sleeping rule; slalom: full steering swing
    for (scenario, first, count) in [("settle", 0usize, 300usize), ("slalom", 3000, 300)] {
        let recording = Recording::read(&repo.join(format!("oracle/car/{scenario}.carrec")))?;
        let path = data.join(format!("{scenario}_{first}_{count}.odegold"));
        let bytes = write_excerpt(&recording, first, count, &path)?;
        println!("{} ({bytes} bytes)", path.display());
    }
    Ok(())
}
