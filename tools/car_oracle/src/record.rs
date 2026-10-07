// SPDX-License-Identifier: MIT OR Apache-2.0

//! The recording format, its reader and the CSV converter.
//!
//! File layout (little endian):
//! ```text
//! "ACCARORC"  u32 version  u32 header length
//! header      UTF-8 text: `key=value` lines, then one `field <kind> <name>` line per value
//!             of the per-step block (kinds: f = f32, i = i32, x = u32 shown in hex,
//!             d = f64, which takes two words: low, high)
//! steps       u32 word count (the same for every step), the block's words,
//!             u32 call count, then 16 words per force call (see `Call`)
//! trailer     u32 0xffffffff, u32 length, UTF-8 text: one `callsite <address> <system>
//!             <function+offset>` line per place in the game's code that handed a force to
//!             a body during this run
//! ```
//! Every number is stored as the bits the game produced; nothing is rounded.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::io::Write as _;
use std::path::Path;

pub const MAGIC: &[u8; 8] = b"ACCARORC";
pub const VERSION: u32 = 1;
const TRAILER_MARK: u32 = 0xffff_ffff;
pub const CALL_WORDS: usize = 16;

/// Kinds of force calls on the tape (`Call::kind`).
pub const KIND_NAMES: [&str; 13] = [
    "?",
    "addForceAtPos",           // a = force (world), b = position (world)
    "addForceAtLocalPos",      // a = force (world), b = position (body)
    "addLocalForce",           // a = force (body), applied at the centre of mass
    "addLocalForceAtPos",      // a = force (body), b = position (world)
    "addLocalForceAtLocalPos", // a = force (body), b = position (body)
    "addTorque",               // a = torque (world)
    "addLocalTorque",          // a = torque (body)
    "stop",                    // velocities and accumulators zeroed
    "setVelocity",             // a = velocity (world)
    "setAngularVelocity",      // a = angular velocity (world)
    "setPosition",             // a = position (world)
    "setRotation",             // a = first row of the matrix handed over
];

/// One call that handed a force or torque to a rigid body (or changed its state by hand).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Call {
    /// Index into the recording's `bodies` list.
    pub body: u32,
    /// Index into [`KIND_NAMES`].
    pub kind: u32,
    /// Where the call came from: address (as an offset into acs.exe) of the instruction after it.
    pub site: u32,
    /// If the call was made on behalf of a suspension force entry (a tyre pushing on its
    /// hub …): the address after that outer call, else 0.
    pub outer_site: u32,
    pub a: [f32; 3],
    pub b: [f32; 3],
    /// The body's force accumulator right after the call.
    pub facc: [f32; 3],
    /// The body's torque accumulator right after the call.
    pub tacc: [f32; 3],
}

impl Call {
    pub fn to_words(&self) -> [u32; CALL_WORDS] {
        let mut w = [0u32; CALL_WORDS];
        w[0] = self.body;
        w[1] = self.kind;
        w[2] = self.site;
        w[3] = self.outer_site;
        for i in 0..3 {
            w[4 + i] = self.a[i].to_bits();
            w[7 + i] = self.b[i].to_bits();
            w[10 + i] = self.facc[i].to_bits();
            w[13 + i] = self.tacc[i].to_bits();
        }
        w
    }

    pub fn from_words(w: &[u32]) -> Call {
        let v = |i: usize| [f32::from_bits(w[i]), f32::from_bits(w[i + 1]), f32::from_bits(w[i + 2])];
        Call { body: w[0], kind: w[1], site: w[2], outer_site: w[3], a: v(4), b: v(7), facc: v(10), tacc: v(13) }
    }
}

/// The per-step block while it is being filled: values, and (for the first step) their names.
#[derive(Default)]
pub struct Row {
    pub words: Vec<u32>,
    /// `(kind, name)` per value; collected only when `naming` is set.
    pub fields: Vec<(char, String)>,
    pub naming: bool,
}

impl Row {
    pub fn new(naming: bool) -> Row {
        Row { naming, ..Row::default() }
    }

    fn name(&mut self, kind: char, name: &str) {
        if self.naming {
            self.fields.push((kind, name.to_string()));
        }
    }

    pub fn f(&mut self, name: &str, value: f32) {
        self.name('f', name);
        self.words.push(value.to_bits());
    }

    pub fn i(&mut self, name: &str, value: i32) {
        self.name('i', name);
        self.words.push(value as u32);
    }

    pub fn d(&mut self, name: &str, value: f64) {
        self.name('d', name);
        let bits = value.to_bits();
        self.words.push(bits as u32);
        self.words.push((bits >> 32) as u32);
    }

    pub fn v(&mut self, name: &str, value: &[f32]) {
        const AXES: [&str; 4] = ["x", "y", "z", "w"];
        for (i, &component) in value.iter().enumerate() {
            if value.len() <= 4 {
                self.f(&format!("{name}.{}", AXES[i]), component);
            } else {
                self.f(&format!("{name}.{i}"), component);
            }
        }
    }
}

/// `SPageFilePhysics`, the shared-memory page `Local\\acpmf_physics` (0x250 bytes): name, kind
/// (`f` f32 / `i` i32) and element count of every member, in struct order (from acs.pdb).
pub const PAGE_FIELDS: [(&str, char, usize); 63] = [
    ("packetId", 'i', 1),
    ("gas", 'f', 1),
    ("brake", 'f', 1),
    ("fuel", 'f', 1),
    ("gear", 'i', 1),
    ("rpms", 'i', 1),
    ("steerAngle", 'f', 1),
    ("speedKmh", 'f', 1),
    ("velocity", 'f', 3),
    ("accG", 'f', 3),
    ("wheelSlip", 'f', 4),
    ("wheelLoad", 'f', 4),
    ("wheelsPressure", 'f', 4),
    ("wheelAngularSpeed", 'f', 4),
    ("tyreWear", 'f', 4),
    ("tyreDirtyLevel", 'f', 4),
    ("tyreCoreTemperature", 'f', 4),
    ("camberRAD", 'f', 4),
    ("suspensionTravel", 'f', 4),
    ("drs", 'f', 1),
    ("tc", 'f', 1),
    ("heading", 'f', 1),
    ("pitch", 'f', 1),
    ("roll", 'f', 1),
    ("cgHeight", 'f', 1),
    ("carDamage", 'f', 5),
    ("numberOfTyresOut", 'i', 1),
    ("pitLimiterOn", 'i', 1),
    ("abs", 'f', 1),
    ("kersCharge", 'f', 1),
    ("kersInput", 'f', 1),
    ("autoShifterOn", 'i', 1),
    ("rideHeight", 'f', 2),
    ("turboBoost", 'f', 1),
    ("ballast", 'f', 1),
    ("airDensity", 'f', 1),
    ("airTemp", 'f', 1),
    ("roadTemp", 'f', 1),
    ("localAngularVel", 'f', 3),
    ("finalFF", 'f', 1),
    ("performanceMeter", 'f', 1),
    ("engineBrake", 'i', 1),
    ("ersRecoveryLevel", 'i', 1),
    ("ersPowerLevel", 'i', 1),
    ("ersHeatCharging", 'i', 1),
    ("ersIsCharging", 'i', 1),
    ("kersCurrentKJ", 'f', 1),
    ("drsAvailable", 'i', 1),
    ("drsEnabled", 'i', 1),
    ("brakeTemp", 'f', 4),
    ("clutch", 'f', 1),
    ("tyreTempI", 'f', 4),
    ("tyreTempM", 'f', 4),
    ("tyreTempO", 'f', 4),
    ("isAIControlled", 'i', 1),
    ("tyreContactPoint", 'f', 12),
    ("tyreContactNormal", 'f', 12),
    ("tyreContactHeading", 'f', 12),
    ("brakeBias", 'f', 1),
    ("localVelocity", 'f', 3),
    ("P2PActivations", 'i', 1),
    ("P2PStatus", 'i', 1),
    ("currentMaxRpm", 'i', 1),
];
pub const PAGE_SIZE: usize = 0x250;

/// Column names of one page member as `ac_telemetry.py --to-csv` writes them.
pub fn page_columns(name: &str, count: usize) -> Vec<String> {
    const WHEELS: [&str; 4] = ["fl", "fr", "rl", "rr"];
    const AXES: [&str; 3] = ["x", "y", "z"];
    match count {
        1 => vec![name.to_string()],
        3 => AXES.iter().map(|a| format!("{name}_{a}")).collect(),
        4 => WHEELS.iter().map(|w| format!("{name}_{w}")).collect(),
        12 => WHEELS.iter().flat_map(|w| AXES.iter().map(move |a| format!("{name}_{w}_{a}"))).collect(),
        n => (0..n).map(|i| format!("{name}_{i}")).collect(),
    }
}

pub fn fnv1a(hash: &mut u64, bytes: &[u8]) {
    for &byte in bytes {
        *hash = (*hash ^ byte as u64).wrapping_mul(0x0000_0100_0000_01b3);
    }
}

/// Writes a recording and keeps a hash of every byte (two runs are compared by it).
pub struct Writer {
    file: Option<std::io::BufWriter<std::fs::File>>,
    pub hash: u64,
    pub bytes: u64,
    meta: Vec<(String, String)>,
    block_words: usize,
    pub steps: usize,
}

impl Writer {
    /// `path = None` only hashes.
    pub fn new(path: Option<&Path>, meta: Vec<(String, String)>) -> std::io::Result<Writer> {
        let file = match path {
            Some(path) => {
                if let Some(dir) = path.parent() {
                    std::fs::create_dir_all(dir)?;
                }
                Some(std::io::BufWriter::with_capacity(1 << 20, std::fs::File::create(path)?))
            }
            None => None,
        };
        Ok(Writer { file, hash: 0xcbf2_9ce4_8422_2325, bytes: 0, meta, block_words: 0, steps: 0 })
    }

    fn put(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        fnv1a(&mut self.hash, bytes);
        self.bytes += bytes.len() as u64;
        if let Some(file) = &mut self.file {
            file.write_all(bytes)?;
        }
        Ok(())
    }

    fn put_words(&mut self, words: &[u32]) -> std::io::Result<()> {
        let mut bytes = Vec::with_capacity(words.len() * 4);
        for word in words {
            bytes.extend_from_slice(&word.to_le_bytes());
        }
        self.put(&bytes)
    }

    pub fn step(&mut self, row: &Row, calls: &[Call]) -> std::io::Result<()> {
        if self.steps == 0 {
            assert!(row.naming, "the first row must carry the field names");
            let mut header = String::new();
            for (key, value) in &self.meta {
                writeln!(header, "{key}={value}").unwrap();
            }
            for (kind, name) in &row.fields {
                writeln!(header, "field {kind} {name}").unwrap();
            }
            self.put(MAGIC)?;
            self.put_words(&[VERSION, header.len() as u32])?;
            self.put(header.as_bytes())?;
            self.block_words = row.words.len();
        }
        assert_eq!(row.words.len(), self.block_words, "the per-step block changed size");
        self.put_words(&[row.words.len() as u32])?;
        self.put_words(&row.words)?;
        self.put_words(&[calls.len() as u32])?;
        for call in calls {
            self.put_words(&call.to_words())?;
        }
        self.steps += 1;
        Ok(())
    }

    pub fn finish(mut self, trailer: &str) -> std::io::Result<(u64, u64)> {
        self.put_words(&[TRAILER_MARK, trailer.len() as u32])?;
        self.put(trailer.as_bytes())?;
        if let Some(mut file) = self.file.take() {
            file.flush()?;
        }
        Ok((self.hash, self.bytes))
    }
}

pub struct Step {
    pub words: Vec<u32>,
    pub calls: Vec<Call>,
}

pub struct Site {
    pub system: String,
    pub function: String,
}

/// A recording read back.
pub struct Recording {
    pub meta: Vec<(String, String)>,
    /// `(kind, name, index of its first word)`
    pub fields: Vec<(char, String, usize)>,
    index: BTreeMap<String, usize>,
    pub steps: Vec<Step>,
    pub sites: BTreeMap<u32, Site>,
}

impl Recording {
    pub fn read(path: &Path) -> Result<Recording, String> {
        let data = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        if data.len() < 16 || &data[..8] != MAGIC {
            return Err(format!("{}: not a car_oracle recording", path.display()));
        }
        let word = |at: usize| u32::from_le_bytes(data[at..at + 4].try_into().unwrap());
        if word(8) != VERSION {
            return Err(format!("recording version {} (this build reads {VERSION})", word(8)));
        }
        let header_len = word(12) as usize;
        let header = std::str::from_utf8(&data[16..16 + header_len]).map_err(|e| e.to_string())?;
        let mut meta = Vec::new();
        let mut fields = Vec::new();
        let mut index = BTreeMap::new();
        let mut next = 0;
        for line in header.lines() {
            if let Some(rest) = line.strip_prefix("field ") {
                let (kind, name) = rest.split_once(' ').ok_or("bad field line")?;
                let kind = kind.chars().next().unwrap();
                index.insert(name.to_string(), fields.len());
                fields.push((kind, name.to_string(), next));
                next += if kind == 'd' { 2 } else { 1 };
            } else if let Some((key, value)) = line.split_once('=') {
                meta.push((key.to_string(), value.to_string()));
            }
        }
        let mut at = 16 + header_len;
        let mut steps = Vec::new();
        let mut sites = BTreeMap::new();
        while at + 4 <= data.len() {
            let count = word(at);
            at += 4;
            if count == TRAILER_MARK {
                let len = word(at) as usize;
                let text = std::str::from_utf8(&data[at + 4..at + 4 + len]).map_err(|e| e.to_string())?;
                for line in text.lines() {
                    let mut cells = line.splitn(4, ' ');
                    if cells.next() != Some("callsite") {
                        continue;
                    }
                    let (Some(address), Some(system), Some(function)) = (cells.next(), cells.next(), cells.next())
                    else {
                        continue;
                    };
                    let address = u32::from_str_radix(address, 16).map_err(|e| e.to_string())?;
                    sites.insert(address, Site { system: system.to_string(), function: function.to_string() });
                }
                break;
            }
            let count = count as usize;
            if count != next {
                return Err(format!("step {}: block of {count} words, header says {next}", steps.len()));
            }
            let words: Vec<u32> = (0..count).map(|i| word(at + i * 4)).collect();
            at += count * 4;
            let n_calls = word(at) as usize;
            at += 4;
            let mut calls = Vec::with_capacity(n_calls);
            for _ in 0..n_calls {
                let w: Vec<u32> = (0..CALL_WORDS).map(|i| word(at + i * 4)).collect();
                calls.push(Call::from_words(&w));
                at += CALL_WORDS * 4;
            }
            steps.push(Step { words, calls });
        }
        Ok(Recording { meta, fields, index, steps, sites })
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.meta.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
    }

    pub fn list(&self, key: &str) -> Vec<String> {
        self.get(key).map(|v| v.split(',').map(str::to_string).collect()).unwrap_or_default()
    }

    /// Word index of a field.
    pub fn col(&self, name: &str) -> usize {
        match self.index.get(name) {
            Some(&i) => self.fields[i].2,
            None => panic!("the recording has no field {name}"),
        }
    }

    pub fn has(&self, name: &str) -> bool {
        self.index.contains_key(name)
    }

    pub fn f(&self, step: usize, name: &str) -> f32 {
        f32::from_bits(self.steps[step].words[self.col(name)])
    }

    pub fn i(&self, step: usize, name: &str) -> i32 {
        self.steps[step].words[self.col(name)] as i32
    }

    pub fn d(&self, step: usize, name: &str) -> f64 {
        let at = self.col(name);
        let w = &self.steps[step].words;
        f64::from_bits(w[at] as u64 | (w[at + 1] as u64) << 32)
    }

    /// The system a call is booked under: by where it came from, or, for a call made inside
    /// one of the suspension's force entries, by who called that entry.
    pub fn system_of(&self, call: &Call) -> &str {
        let site = if call.outer_site != 0 { call.outer_site } else { call.site };
        self.sites.get(&site).map(|s| s.system.as_str()).unwrap_or("unknown")
    }

    /// Force and torque handed to each body by each system in one step: the sum of what each
    /// call added to the body's accumulators (`[body][system] -> [fx, fy, fz, tx, ty, tz]`).
    pub fn sums(&self, step: usize, systems: &[String], bodies: usize) -> Vec<Vec<[f64; 6]>> {
        let mut out = vec![vec![[0.0f64; 6]; systems.len()]; bodies];
        let mut acc = vec![[0.0f32; 6]; bodies];
        for call in &self.steps[step].calls {
            let b = call.body as usize;
            let after = [call.facc[0], call.facc[1], call.facc[2], call.tacc[0], call.tacc[1], call.tacc[2]];
            if call.kind <= 7 {
                let system = self.system_of(call);
                if let Some(s) = systems.iter().position(|name| name == system) {
                    for k in 0..6 {
                        out[b][s][k] += after[k] as f64 - acc[b][k] as f64;
                    }
                }
            }
            acc[b] = after;
        }
        out
    }
}

fn format_value(kind: char, words: &[u32], at: usize) -> String {
    match kind {
        'f' => format!("{:?}", f32::from_bits(words[at])),
        'd' => format!("{:?}", f64::from_bits(words[at] as u64 | (words[at + 1] as u64) << 32)),
        'x' => format!("{:#x}", words[at]),
        _ => format!("{}", words[at] as i32),
    }
}

pub use crate::sites::SYSTEMS;

pub struct CsvOptions {
    pub table: String,
    pub from: usize,
    pub to: usize,
    pub every: usize,
    /// Keep only columns whose name starts with one of these (empty = all).
    pub only: Vec<String>,
}

/// Writes one table of a recording as CSV with full-precision values.
pub fn to_csv(recording: &Recording, options: &CsvOptions, out: &Path) -> Result<usize, String> {
    let file = std::fs::File::create(out).map_err(|e| format!("{}: {e}", out.display()))?;
    let mut w = std::io::BufWriter::with_capacity(1 << 20, file);
    let bodies = recording.list("bodies");
    let systems: Vec<String> = SYSTEMS.iter().map(|s| s.to_string()).collect();
    let keep = |name: &str| options.only.is_empty() || options.only.iter().any(|p| name.starts_with(p.as_str()));
    let to = options.to.min(recording.steps.len());
    let range = (options.from..to).step_by(options.every.max(1));
    let io = |e: std::io::Error| e.to_string();
    let mut rows = 0;
    match options.table.as_str() {
        "steps" => {
            let mut names: Vec<String> = Vec::new();
            let mut picked = Vec::new();
            for (kind, name, at) in &recording.fields {
                if keep(name) {
                    names.push(name.clone());
                    picked.push((*kind, *at));
                }
            }
            // derived: what each system handed to each body (see `Recording::sums`), and what
            // dWorldStep itself adds before solving (gravity, gyroscopic torque)
            let mut sum_cols = Vec::new();
            let mut ode_cols = Vec::new();
            for body in &bodies {
                for (name, a, b) in [("gravity", "solver.facc", "facc"), ("gyroscopic", "solver.tacc", "tacc")] {
                    for axis in ["x", "y", "z"] {
                        let column = format!("sum.{body}.{name}.{}{axis}", if name == "gravity" { "f" } else { "t" });
                        if keep(&column) && recording.has(&format!("{body}.{a}.{axis}")) {
                            names.push(column);
                            ode_cols.push((
                                recording.col(&format!("{body}.{a}.{axis}")),
                                recording.col(&format!("{body}.{b}.{axis}")),
                            ));
                        }
                    }
                }
            }
            for (b, body) in bodies.iter().enumerate() {
                for (s, system) in systems.iter().enumerate() {
                    for (k, axis) in ["fx", "fy", "fz", "tx", "ty", "tz"].iter().enumerate() {
                        let name = format!("sum.{body}.{system}.{axis}");
                        if keep(&name) {
                            names.push(name);
                            sum_cols.push((b, s, k));
                        }
                    }
                }
            }
            writeln!(w, "{}", names.join(",")).map_err(io)?;
            for step in range {
                let words = &recording.steps[step].words;
                let mut cells: Vec<String> = picked.iter().map(|&(kind, at)| format_value(kind, words, at)).collect();
                // (accumulator the solver used) - (accumulator the game handed over)
                cells.extend(
                    ode_cols.iter().map(|&(a, b)| format!("{:?}", f32::from_bits(words[a]) - f32::from_bits(words[b]))),
                );
                if !sum_cols.is_empty() {
                    let sums = recording.sums(step, &systems, bodies.len());
                    cells.extend(sum_cols.iter().map(|&(b, s, k)| format!("{:?}", sums[b][s][k] as f32)));
                }
                writeln!(w, "{}", cells.join(",")).map_err(io)?;
                rows += 1;
            }
        }
        "tape" => {
            writeln!(
                w,
                "step,seq,body,kind,system,site,function,outer_site,outer_function,ax,ay,az,bx,by,bz,\
                 facc_x,facc_y,facc_z,tacc_x,tacc_y,tacc_z"
            )
            .map_err(io)?;
            for step in range {
                for (seq, call) in recording.steps[step].calls.iter().enumerate() {
                    let site = recording.sites.get(&call.site);
                    let outer = recording.sites.get(&call.outer_site);
                    let mut line = format!(
                        "{step},{seq},{},{},{},{:#x},{},{:#x},{}",
                        bodies.get(call.body as usize).map(String::as_str).unwrap_or("?"),
                        KIND_NAMES.get(call.kind as usize).copied().unwrap_or("?"),
                        recording.system_of(call),
                        0x1_4000_0000u64 + call.site as u64,
                        site.map(|s| s.function.as_str()).unwrap_or(""),
                        if call.outer_site == 0 { 0 } else { 0x1_4000_0000u64 + call.outer_site as u64 },
                        outer.map(|s| s.function.as_str()).unwrap_or(""),
                    );
                    for v in [call.a, call.b, call.facc, call.tacc] {
                        for x in v {
                            write!(line, ",{x:?}").unwrap();
                        }
                    }
                    writeln!(w, "{line}").map_err(io)?;
                    rows += 1;
                }
            }
        }
        "telemetry" => {
            // the shared-memory physics page of every step, in the columns of the logger's
            // raw-recording converter (`ac_telemetry.py --to-csv`), so the two can be compared
            let mut names = vec!["t".to_string(), "posX".to_string(), "posY".to_string(), "posZ".to_string()];
            let mut picked = Vec::new();
            for (name, kind, count) in PAGE_FIELDS {
                for (i, column) in page_columns(name, count).into_iter().enumerate() {
                    names.push(column);
                    let field = if count == 1 { format!("page.{name}") } else { format!("page.{name}.{i}") };
                    picked.push((kind, recording.col(&field)));
                }
            }
            let position: Vec<usize> =
                ["x", "y", "z"].iter().map(|a| recording.col(&format!("body.post.pos.{a}"))).collect();
            writeln!(w, "{}", names.join(",")).map_err(io)?;
            for step in range {
                let words = &recording.steps[step].words;
                // seconds since the start of the scenario
                let mut cells = vec![format!("{:?}", (recording.i(step, "step") + 1) as f64 * 0.003)];
                cells.extend(position.iter().map(|&at| format_value('f', words, at)));
                cells.extend(picked.iter().map(|&(kind, at)| format_value(kind, words, at)));
                writeln!(w, "{}", cells.join(",")).map_err(io)?;
                rows += 1;
            }
        }
        other => return Err(format!("unknown table {other} (steps, tape, telemetry)")),
    }
    w.flush().map_err(io)?;
    Ok(rows)
}

/// Value-by-value comparison of two recordings: which fields ever differ, where first, and the
/// same for the force tape.
pub fn diff(a: &Recording, b: &Recording, only: &[String], ignore: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    if a.steps.len() != b.steps.len() {
        out.push(format!("step counts differ: {} / {}", a.steps.len(), b.steps.len()));
    }
    let steps = a.steps.len().min(b.steps.len());
    let mut compared = 0;
    let mut differing = Vec::new();
    for (kind, name, at) in &a.fields {
        if !only.is_empty() && !only.iter().any(|p| name.starts_with(p.as_str())) {
            continue;
        }
        if ignore.iter().any(|part| name.contains(part.as_str())) {
            continue;
        }
        if !b.has(name) {
            out.push(format!("{name}: only in the first recording"));
            continue;
        }
        let other = b.col(name);
        let width = if *kind == 'd' { 2 } else { 1 };
        compared += 1;
        let mut count = 0;
        let mut first = None;
        for step in 0..steps {
            let (wa, wb) = (&a.steps[step].words, &b.steps[step].words);
            if wa[*at..*at + width] != wb[other..other + width] {
                count += 1;
                first.get_or_insert_with(|| {
                    format!("step {step}: {} / {}", format_value(*kind, wa, *at), format_value(*kind, wb, other))
                });
            }
        }
        if count > 0 {
            differing.push(format!("{name}: differs in {count} steps, first at {}", first.unwrap()));
        }
    }
    for (_, name, _) in &b.fields {
        if !a.has(name) {
            out.push(format!("{name}: only in the second recording"));
        }
    }
    // bits, not values: -0.0 against +0.0 is a difference, NaN against the same NaN is not
    let words = |r: &Recording, s: usize| -> Vec<[u32; CALL_WORDS]> { r.steps[s].calls.iter().map(Call::to_words).collect() };
    let tape_steps = (0..steps).filter(|&s| words(a, s) != words(b, s)).count();
    let first_tape = (0..steps).find(|&s| words(a, s) != words(b, s));
    out.push(format!(
        "{compared} fields compared over {steps} steps: {} differ; force tape differs in {tape_steps} steps{}",
        differing.len(),
        first_tape.map(|s| format!(" (first at step {s})")).unwrap_or_default()
    ));
    let total = differing.len();
    out.extend(differing.into_iter().take(40));
    if total > 40 {
        out.push(format!("... and {} more fields", total - 40));
    }
    out
}
