// SPDX-License-Identifier: GPL-3.0-or-later

//! The DSP micro-oracle: the two plug-ins acs.exe registers with FMOD ("FMOD Distance Filter",
//! "FMOD Gain") against their Rust ports, callback by callback. Each side gets its description
//! (`FMODGetDSPDescription_*` of the game, `rustyac_audio::dsp::*::description()` of the port)
//! and is driven through the description's own callbacks, as FMOD would: create, parameters,
//! many blocks of random samples, reset, release. Every output is compared bit for bit.

use std::ffi::{c_char, c_void};

use rustyac_audio::dsp::{distance_filter, gain, DspState, DspSystemCallbacks};

use crate::acs::Acs;

const VA_GET_DISTANCE_FILTER: usize = 0x1_401f_c650; // FMODGetDSPDescription_DistanceFilter()
const VA_GET_GAIN: usize = 0x1_401f_d240; // FMODGetDSPDescription_Gain()
const VA_DISTANCE_FILTER_DC: usize = 0x1_4151_cfd4; // the distance filter's `static float dc`

/// FMOD's allocator for a plug-in: zeroed memory (never given back here: a test's worth).
unsafe extern "C" fn alloc(size: u32, _kind: u32, _source: *const c_char) -> *mut c_void {
    let block = vec![0u64; (size as usize).div_ceil(8).max(1)].into_boxed_slice();
    Box::leak(block).as_mut_ptr().cast()
}

unsafe extern "C" fn free(_pointer: *mut c_void, _kind: u32, _source: *const c_char) {}

unsafe extern "C" fn sample_rate(_state: *mut DspState, rate: *mut i32) -> i32 {
    *rate = 48000;
    0
}

/// One instance of a plug-in, called through its description.
struct Plugin {
    description: *const u8,
    state: Box<DspState>,
    _callbacks: Box<DspSystemCallbacks>,
    faulty: bool,
}

impl Plugin {
    unsafe fn new(description: *const c_void) -> Plugin {
        let callbacks = Box::new(DspSystemCallbacks { alloc, realloc: std::ptr::null(), free, getsamplerate: sample_rate });
        let state = Box::new(DspState {
            instance: std::ptr::null_mut(),
            plugindata: std::ptr::null_mut(),
            channelmask: 0,
            source_speakermode: 0,
            sidechaindata: std::ptr::null_mut(),
            sidechainchannels: 0,
            callbacks: &*callbacks,
        });
        Plugin { description: description.cast(), state, _callbacks: callbacks, faulty: false }
    }

    unsafe fn callback(&self, offset: usize) -> usize {
        self.description.add(offset).cast::<usize>().read_unaligned()
    }

    unsafe fn simple(&mut self, offset: usize) -> i32 {
        let f: unsafe extern "C" fn(*mut DspState) -> i32 = std::mem::transmute(self.callback(offset));
        f(&mut *self.state)
    }

    unsafe fn create(&mut self) -> i32 {
        self.simple(0x30)
    }
    unsafe fn release(&mut self) -> i32 {
        self.simple(0x38)
    }
    unsafe fn reset(&mut self) -> i32 {
        self.simple(0x40)
    }

    unsafe fn read(&mut self, input: &[f32], output: &mut [f32], length: u32, channels: i32) -> i32 {
        let f: unsafe extern "C" fn(*mut DspState, *mut f32, *mut f32, u32, i32, *mut i32) -> i32 = std::mem::transmute(self.callback(0x48));
        let mut out_channels = channels;
        f(&mut *self.state, input.as_ptr() as *mut f32, output.as_mut_ptr(), length, channels, &mut out_channels)
    }

    unsafe fn set_float(&mut self, index: i32, value: f32) -> i32 {
        let f: unsafe extern "C" fn(*mut DspState, i32, f32) -> i32 = std::mem::transmute(self.callback(0x70));
        // AUDIO_DSP_FAULT=1: the port's side gets the value with its last bit flipped (does the
        // comparison notice?)
        let value = if self.faulty { f32::from_bits(value.to_bits() ^ 1) } else { value };
        f(&mut *self.state, index, value)
    }

    unsafe fn set_bool(&mut self, index: i32, value: i32) -> i32 {
        let f: unsafe extern "C" fn(*mut DspState, i32, i32) -> i32 = std::mem::transmute(self.callback(0x80));
        f(&mut *self.state, index, value)
    }

    unsafe fn set_data(&mut self, index: i32, data: &[f32]) -> i32 {
        let f: unsafe extern "C" fn(*mut DspState, i32, *mut c_void, u32) -> i32 = std::mem::transmute(self.callback(0x88));
        f(&mut *self.state, index, data.as_ptr() as *mut c_void, (data.len() * 4) as u32)
    }

    unsafe fn get_float(&mut self, index: i32) -> (i32, u32, String) {
        let f: unsafe extern "C" fn(*mut DspState, i32, *mut f32, *mut c_char) -> i32 = std::mem::transmute(self.callback(0x90));
        let mut value = 0.0f32;
        let mut text = [0u8; 64];
        let r = f(&mut *self.state, index, &mut value, text.as_mut_ptr().cast());
        (r, value.to_bits(), text_of(&text))
    }

    unsafe fn get_bool(&mut self, index: i32) -> (i32, u32, String) {
        let f: unsafe extern "C" fn(*mut DspState, i32, *mut i32, *mut c_char) -> i32 = std::mem::transmute(self.callback(0xa0));
        let mut value = 0i32;
        let mut text = [0u8; 64];
        let r = f(&mut *self.state, index, &mut value, text.as_mut_ptr().cast());
        (r, value as u32, text_of(&text))
    }

    unsafe fn should_process(&mut self, idle: i32) -> i32 {
        let f: unsafe extern "C" fn(*mut DspState, i32) -> i32 = std::mem::transmute(self.callback(0xb0));
        f(&mut *self.state, idle)
    }
}

fn text_of(bytes: &[u8]) -> String {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..end]).into_owned()
}

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
    /// 0..1
    fn unit(&mut self) -> f32 {
        (self.next() >> 40) as f32 / (1u64 << 24) as f32
    }
    fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.unit()
    }
    /// A sample: mostly -1..1, now and then something a mixer can also hand over.
    fn sample(&mut self) -> f32 {
        match self.below(400) {
            0 => 0.0,
            1 => -0.0,
            2 => f32::from_bits(1),
            3 => 1.0e-30,
            4 => 8.0,
            _ => self.range(-1.0, 1.0),
        }
    }
}

/// Same bits, or both a NaN (the payload of a NaN depends on operand order the compiler picks).
fn same(a: u32, b: u32) -> bool {
    a == b || (f32::from_bits(a).is_nan() && f32::from_bits(b).is_nan())
}

#[derive(Default)]
struct Tally {
    cases: u64,
    blocks: u64,
    samples: u64,
    parameters: u64,
    mismatches: u64,
    first: Option<String>,
}

impl Tally {
    fn check(&mut self, what: &str, ok: bool) {
        if !ok {
            self.mismatches += 1;
            if self.first.is_none() {
                self.first = Some(what.to_string());
            }
        }
    }
}

/// One random life of an instance, the same on both sides.
unsafe fn case(game: *const c_void, port: *const c_void, is_gain: bool, rng: &mut Rng, tally: &mut Tally) {
    let mut a = Plugin::new(game);
    let mut b = Plugin::new(port);
    b.faulty = std::env::var_os("AUDIO_DSP_FAULT").is_some();
    tally.cases += 1;
    tally.check("create", a.create() == b.create());
    if is_gain {
        // the gain plug-in never initialises its state: FMOD sets the defaults and resets
        for p in [&mut a, &mut b] {
            p.set_float(0, 0.0);
            p.set_bool(1, 0);
            p.reset();
        }
    }
    let steps = 6 + rng.below(30);
    for step in 0..steps {
        match rng.below(10) {
            0 | 1 if !is_gain => {
                let (index, value) = match rng.below(8) {
                    0 => (0, 0.0),
                    1 => (1, 22000.0),
                    2 => (1, 30000.0),
                    3 => (2, 1.0),
                    4 => (0, rng.range(0.0, 10000.0)),
                    5 => (1, rng.range(10.0, 22000.0)),
                    6 => (0, f32::NAN),
                    _ => (0, rng.range(0.0, 200.0)),
                };
                tally.parameters += 1;
                tally.check(&format!("distance filter setparameterfloat({index}, {value})"), a.set_float(index, value) == b.set_float(index, value));
            }
            2 if !is_gain => {
                let scale = [0.0, 1.0, 30.0, 400.0, 20000.0][rng.below(5) as usize];
                let data = [rng.range(-1.0, 1.0) * scale, rng.range(-1.0, 1.0) * scale, rng.range(-1.0, 1.0) * scale, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0];
                let index = if rng.below(12) == 0 { 0 } else { 2 };
                tally.parameters += 1;
                tally.check("distance filter setparameterdata", a.set_data(index, &data) == b.set_data(index, &data));
            }
            0 | 1 if is_gain => {
                let (index, value) = match rng.below(8) {
                    0 => (0, -80.0),
                    1 => (0, -100.0),
                    2 => (0, 10.0),
                    3 => (1, 0.0),
                    4 => (0, f32::NAN),
                    5 => (0, 0.0),
                    _ => (0, rng.range(-80.0, 10.0)),
                };
                tally.parameters += 1;
                tally.check(&format!("gain setparameterfloat({index}, {value})"), a.set_float(index, value) == b.set_float(index, value));
            }
            2 if is_gain => {
                let (index, value) = (if rng.below(10) == 0 { 0 } else { 1 }, rng.below(3) as i32);
                tally.parameters += 1;
                tally.check("gain setparameterbool", a.set_bool(index, value) == b.set_bool(index, value));
            }
            3 => {
                for index in 0..3 {
                    let (x, y) = (a.get_float(index), b.get_float(index));
                    tally.parameters += 1;
                    tally.check(&format!("getparameterfloat({index}): {x:?} against {y:?}"), x.0 == y.0 && same(x.1, y.1) && (x.2 == y.2 || f32::from_bits(x.1).is_nan()));
                }
                if is_gain {
                    let (x, y) = (a.get_bool(1), b.get_bool(1));
                    tally.parameters += 1;
                    tally.check(&format!("getparameterbool: {x:?} against {y:?}"), x == y);
                }
            }
            4 if step > 2 && rng.below(4) == 0 => {
                tally.check("reset", a.reset() == b.reset());
            }
            5 if rng.below(3) == 0 => {
                let idle = rng.below(2) as i32;
                tally.check("shouldiprocess", a.should_process(idle) == b.should_process(idle));
            }
            _ => {
                // a block of samples; the distance filter refuses more than 8 channels
                let channels = if is_gain { 1 + rng.below(12) as i32 } else { 1 + rng.below(9) as i32 };
                let length = [1u32, 2, 3, 17, 64, 255, 256, 257, 512, 800, 1024][rng.below(11) as usize];
                let n = length as usize * channels as usize;
                let input: Vec<f32> = (0..n).map(|_| rng.sample()).collect();
                let mut out_a = vec![0.25f32; n];
                let mut out_b = vec![0.25f32; n];
                let (ra, rb) = (a.read(&input, &mut out_a, length, channels), b.read(&input, &mut out_b, length, channels));
                tally.blocks += 1;
                tally.samples += n as u64;
                tally.check("read: the result", ra == rb);
                if let Some(at) = (0..n).find(|&i| !same(out_a[i].to_bits(), out_b[i].to_bits())) {
                    tally.check(&format!("read: sample {at} of {n} ({channels} channels): {:08x} against {:08x}", out_a[at].to_bits(), out_b[at].to_bits()), false);
                }
            }
        }
    }
    tally.check("release", a.release() == b.release());
}

pub fn run(args: &crate::Args) -> Result<(), String> {
    let acs = Acs::load(&args.acs)?;
    acs.silence_game_stdout();
    let mut report = String::new();
    let mut ok = true;
    unsafe {
        let get: extern "C" fn() -> *const c_void = std::mem::transmute(acs.va(VA_GET_DISTANCE_FILTER));
        let game_filter = get();
        let get: extern "C" fn() -> *const c_void = std::mem::transmute(acs.va(VA_GET_GAIN));
        let game_gain = get();
        for (name, game, port, is_gain) in [("FMOD Distance Filter", game_filter, distance_filter::description(), false), ("FMOD Gain", game_gain, gain::description(), true)] {
            // the 0xd8 bytes of the description apart from its addresses, and the parameter tables
            let plain = |d: *const c_void| -> Vec<u8> {
                let b = std::slice::from_raw_parts(d.cast::<u8>(), 0xd8);
                let mut v = b[..0x30].to_vec();
                v.extend_from_slice(&b[0x60..0x64]);
                let count = i32::from_le_bytes([b[0x60], b[0x61], b[0x62], b[0x63]]) as usize;
                let table = d.cast::<u8>().add(0x68).cast::<*const u8>().read_unaligned();
                for i in 0..count {
                    let p = std::slice::from_raw_parts(table.cast::<*const u8>().add(i).read_unaligned(), 0x60);
                    v.extend_from_slice(&p[..0x28]);
                    v.extend_from_slice(&p[0x30..0x50]);
                }
                // which callbacks exist
                for slot in (0x30..0x60).step_by(8).chain((0x70..0xd8).step_by(8)) {
                    v.push((b[slot..slot + 8] != [0u8; 8]) as u8);
                }
                v
            };
            let description_same = plain(game) == plain(port);
            let mut rng = Rng(0x9e37_79b9_7f4a_7c15 ^ args.seed.wrapping_mul(0x100_0000_01b3) ^ is_gain as u64);
            let mut tally = Tally::default();
            // both sides start from the game's `dc`
            distance_filter::set_dc_bits(acs.global::<u32>(VA_DISTANCE_FILTER_DC));
            for _ in 0..args.count {
                case(game, port, is_gain, &mut rng, &mut tally);
            }
            let dc_same = is_gain || distance_filter::dc_bits() == acs.global::<u32>(VA_DISTANCE_FILTER_DC);
            let good = description_same && dc_same && tally.mismatches == 0;
            ok &= good;
            report.push_str(&format!(
                "| {name} | {} | {} | {} | {} | {} | {} | {} |\n",
                if description_same { "identical" } else { "DIFFERENT" },
                tally.cases,
                tally.parameters,
                tally.blocks,
                tally.samples,
                tally.mismatches,
                if good { "ok" } else { "FAIL" }
            ));
            if let Some(first) = tally.first {
                report.push_str(&format!("first mismatch: {first}\n"));
            }
        }
    }
    let text = format!(
        "`audio_oracle dsp --count {} --seed {}`\n\n| Plug-in | Description and parameter tables | Instances | Parameter calls | Blocks | Samples compared | Mismatches | |\n|---|---|---|---|---|---|---|---|\n{report}",
        args.count, args.seed
    );
    println!("{text}");
    let path = crate::repo_root().join("oracle/audio/dsp_results.md");
    std::fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
    std::fs::write(&path, &text).map_err(|e| e.to_string())?;
    if ok {
        Ok(())
    } else {
        Err("the game's plug-ins and the port do not agree".to_string())
    }
}
