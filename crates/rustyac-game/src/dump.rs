// SPDX-License-Identifier: MIT OR Apache-2.0

//! The state dump of `--dump-states`: what the car looked like after every step of a replay,
//! bit for bit, so that a test or `tools/chassis_compare` can hold it against a car stepped
//! some other way (directly, or by the game itself in an oracle recording).
//!
//! Layout: `RYSTATE1`, then chunks. `NAME` gives the names of the traced values and is
//! written again whenever they change (the telemetry page joins once the writer has warmed
//! up); `STEP` holds one step.
//!
//! Like `input_file.rs` this needs nothing but `rustyac-physics` and is included by path in
//! `tools/chassis_compare`.

use std::io::{Read, Write};
use std::path::Path;

use rustyac_physics::car::replay::{self, TraceValue};
use rustyac_physics::car::{ForceSource, RollingChassis, TapeCall};

pub const MAGIC: &[u8; 8] = b"RYSTATE1";

/// One step's values.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct StepDump {
    /// `RollingChassis::save_state`: bodies, joints, tyres, the car's counters.
    pub state: Vec<u32>,
    /// `replay::powertrain_trace`: controls, brakes, engine, drivetrain, wings, aids, the
    /// car's own values and the telemetry page, under the names of the oracle's recordings.
    pub trace: Vec<TraceValue>,
    /// `replay::snapshot`: the 2,009 values `tools/chassis_compare` compares. Oracle set-ups
    /// only (it needs the step's trace instruments).
    pub snapshot: Vec<u64>,
    /// The force tape of the step. Oracle set-ups only.
    pub tape: Vec<TapeCall>,
}

impl StepDump {
    /// The car after a step.
    pub fn capture(chassis: &RollingChassis) -> StepDump {
        StepDump {
            state: chassis.save_state(),
            trace: replay::powertrain_trace(chassis),
            snapshot: if chassis.trace.is_some() { replay::snapshot(chassis) } else { Vec::new() },
            tape: chassis.core.tape.clone().unwrap_or_default(),
        }
    }

    /// Everything but the names, as words: two dumps of the same steps are equal exactly when
    /// these are.
    pub fn words(&self) -> Vec<u64> {
        let mut out: Vec<u64> = self.state.iter().map(|w| *w as u64).collect();
        out.extend(self.trace.iter().map(|v| v.word));
        out.extend(&self.snapshot);
        for call in &self.tape {
            out.push(call.body as u64 | (call.kind as u64) << 32);
            for v in [call.a, call.b, call.facc, call.tacc] {
                out.extend(v.map(|x| x.to_bits() as u64));
            }
        }
        out
    }
}

fn io(e: std::io::Error) -> String {
    format!("writing the state dump: {e}")
}

/// Writes a dump step by step, to a file or (path `-`) to the standard output.
pub struct DumpWriter {
    file: Box<dyn Write>,
    names: Vec<(String, char, bool)>,
    pub steps: u64,
}

impl DumpWriter {
    pub fn create(path: &Path) -> Result<DumpWriter, String> {
        let file: Box<dyn Write> = if path.as_os_str() == "-" {
            Box::new(std::io::BufWriter::with_capacity(1 << 20, std::io::stdout()))
        } else {
            let file = std::fs::File::create(path).map_err(|e| format!("{}: {e}", path.display()))?;
            Box::new(std::io::BufWriter::with_capacity(1 << 20, file))
        };
        let mut writer = DumpWriter { file, names: Vec::new(), steps: 0 };
        writer.file.write_all(MAGIC).map_err(io)?;
        Ok(writer)
    }

    pub fn push(&mut self, step: &StepDump) -> Result<(), String> {
        let same = self.names.len() == step.trace.len()
            && self.names.iter().zip(&step.trace).all(|(n, v)| n.0 == v.name && n.1 == v.kind && n.2 == v.extra);
        let mut out: Vec<u8> = Vec::with_capacity(40_000);
        if !same {
            self.names = step.trace.iter().map(|v| (v.name.clone(), v.kind, v.extra)).collect();
            out.extend(b"NAME");
            out.extend((self.names.len() as u32).to_le_bytes());
            for (name, kind, extra) in &self.names {
                out.extend([*kind as u8, *extra as u8]);
                out.extend((name.len() as u16).to_le_bytes());
                out.extend(name.as_bytes());
            }
        }
        out.extend(b"STEP");
        out.extend((step.state.len() as u32).to_le_bytes());
        for word in &step.state {
            out.extend(word.to_le_bytes());
        }
        for value in &step.trace {
            out.extend(value.word.to_le_bytes());
        }
        out.extend((step.snapshot.len() as u32).to_le_bytes());
        for word in &step.snapshot {
            out.extend(word.to_le_bytes());
        }
        out.extend((step.tape.len() as u32).to_le_bytes());
        for call in &step.tape {
            let name = call.source.name();
            out.push(name.len() as u8);
            out.extend(name.as_bytes());
            out.extend(call.body.to_le_bytes());
            out.extend(call.kind.to_le_bytes());
            for v in [call.a, call.b, call.facc, call.tacc] {
                for x in v {
                    out.extend(x.to_bits().to_le_bytes());
                }
            }
        }
        self.file.write_all(&out).map_err(io)?;
        self.steps += 1;
        Ok(())
    }

    pub fn finish(mut self) -> Result<u64, String> {
        self.file.flush().map_err(io)?;
        Ok(self.steps)
    }
}

/// Reads a dump step by step (a file, or the standard output of a running `rustyac`).
pub struct DumpReader<R: Read> {
    reader: R,
    names: Vec<(String, char, bool)>,
}

impl<R: Read> DumpReader<R> {
    pub fn new(mut reader: R) -> Result<DumpReader<R>, String> {
        let mut magic = [0; 8];
        reader.read_exact(&mut magic).map_err(|e| format!("the state dump has no head: {e}"))?;
        if &magic != MAGIC {
            return Err("not a rustyAC state dump".to_string());
        }
        Ok(DumpReader { reader, names: Vec::new() })
    }

    fn take(&mut self, count: usize) -> Result<Vec<u8>, String> {
        let mut out = vec![0; count];
        self.reader.read_exact(&mut out).map_err(|e| format!("the state dump is cut short: {e}"))?;
        Ok(out)
    }

    fn u32(&mut self) -> Result<u32, String> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }

    fn words32(&mut self, count: usize) -> Result<Vec<u32>, String> {
        Ok(self.take(count * 4)?.chunks_exact(4).map(|c| u32::from_le_bytes(c.try_into().unwrap())).collect())
    }

    fn words64(&mut self, count: usize) -> Result<Vec<u64>, String> {
        Ok(self.take(count * 8)?.chunks_exact(8).map(|c| u64::from_le_bytes(c.try_into().unwrap())).collect())
    }

    /// The next step, or `None` at the end of the dump.
    pub fn next_step(&mut self) -> Result<Option<StepDump>, String> {
        loop {
            let mut tag = [0; 4];
            // the end of the dump is only legal between two chunks
            let mut got = 0;
            while got < 4 {
                match self.reader.read(&mut tag[got..]) {
                    Ok(0) if got == 0 => return Ok(None),
                    Ok(0) => return Err("the state dump is cut short".to_string()),
                    Ok(n) => got += n,
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                    Err(e) => return Err(format!("reading the state dump: {e}")),
                }
            }
            match &tag {
                b"NAME" => {
                    let count = self.u32()? as usize;
                    self.names.clear();
                    for _ in 0..count {
                        let head = self.take(4)?;
                        let length = u16::from_le_bytes([head[2], head[3]]) as usize;
                        let name = String::from_utf8(self.take(length)?).map_err(|e| e.to_string())?;
                        self.names.push((name, head[0] as char, head[1] != 0));
                    }
                }
                b"STEP" => {
                    let mut step = StepDump::default();
                    let count = self.u32()? as usize;
                    step.state = self.words32(count)?;
                    let words = self.words64(self.names.len())?;
                    step.trace = self
                        .names
                        .iter()
                        .zip(words)
                        .map(|((name, kind, extra), word)| TraceValue { name: name.clone(), kind: *kind, word, extra: *extra })
                        .collect();
                    let count = self.u32()? as usize;
                    step.snapshot = self.words64(count)?;
                    for _ in 0..self.u32()? {
                        let length = self.take(1)?[0] as usize;
                        let name = String::from_utf8(self.take(length)?).map_err(|e| e.to_string())?;
                        let source = ForceSource::from_name(&name).ok_or(format!("unknown force source {name:?}"))?;
                        let w = self.words32(14)?;
                        let v = |k: usize| [f32::from_bits(w[k]), f32::from_bits(w[k + 1]), f32::from_bits(w[k + 2])];
                        step.tape.push(TapeCall { body: w[0], kind: w[1], source, a: v(2), b: v(5), facc: v(8), tacc: v(11) });
                    }
                    return Ok(Some(step));
                }
                other => return Err(format!("unknown chunk {other:?} in the state dump")),
            }
        }
    }
}

/// Reads a whole dump file.
pub fn read(path: &Path) -> Result<Vec<StepDump>, String> {
    let file = std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut reader =
        DumpReader::new(std::io::BufReader::with_capacity(1 << 20, file)).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut steps = Vec::new();
    while let Some(step) = reader.next_step().map_err(|e| format!("{}: {e}", path.display()))? {
        steps.push(step);
    }
    Ok(steps)
}
