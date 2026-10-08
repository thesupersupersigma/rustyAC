// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! "FMOD Distance Filter" (fmod_distance_filter.obj): two one-pole low-passes and a high-pass
//! whose band narrows towards `Frequency` as the sound gets `Max Dist` away from the listener.

use std::ffi::{c_char, c_void};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::OnceLock;

use super::*;

/// `FMODDistanceFilterState` (0x40 bytes, from FMOD's allocator)
#[repr(C)]
struct State {
    max_distance: f32,
    bandpass_frequency: f32,
    distance_to_listener: f32,
    target_highpass_time_const: f32,
    current_highpass_time_const: f32,
    target_lowpass_time_const: f32,
    current_lowpass_time_const: f32,
    ramp_samples_left: i32,
    previous_lp1_out: *mut f32,
    previous_lp2_out: *mut f32,
    previous_hp_out: *mut f32,
    sample_rate: i32,
    max_channels: i32,
}

/// The plug-in's `static float dc` (1e-20, sign flipped every frame): one for all instances.
static DC: AtomicU32 = AtomicU32::new(0x1e3c_e508);

/// Sets the shared `dc` (for tests that run this plug-in next to the game's).
pub fn set_dc_bits(bits: u32) {
    DC.store(bits, Ordering::Relaxed);
}

pub fn dc_bits() -> u32 {
    DC.load(Ordering::Relaxed)
}

static MAX_DIST_VALUES: [f32; 7] = [0.0, 1.0, 5.0, 20.0, 100.0, 500.0, 10000.0];
static MAX_DIST_POSITIONS: [f32; 7] = [0.0, 1.0, 2.0, 3.0, 4.0, 4.5, 5.0];

struct Statics {
    parameters: [ParameterDesc; 3],
    table: [*const ParameterDesc; 3],
    description: Option<DspDescription>,
}
// SAFETY: built once behind the OnceLock and only read afterwards.
unsafe impl Sync for Statics {}
unsafe impl Send for Statics {}

/// `FMODGetDSPDescription_DistanceFilter` @ 0x1401fc650
pub fn description() -> *const c_void {
    static CELL: OnceLock<Box<Statics>> = OnceLock::new();
    let statics = CELL.get_or_init(|| {
        let mut s = Box::new(Statics {
            parameters: [
                ParameterDesc::new(0, "Max Dist", "", c"Distance at which bandpass stops narrowing. 0 to 1000000000. Default = 100")
                    .float(MAX_DIST_VALUES[0], MAX_DIST_VALUES[6], 20.0, 2)
                    .points(&MAX_DIST_VALUES, &MAX_DIST_POSITIONS),
                ParameterDesc::new(0, "Frequency", "Hz", c"Bandpass target frequency. 100 to 10,000Hz. Default = 2000Hz").float(10.0, 22000.0, 1500.0, 1),
                // FMOD_DSP_PARAMETER_DATA_TYPE_3DATTRIBUTES
                ParameterDesc::new(3, "3D Attributes", "", c"").word(0x30, -2),
            ],
            table: [std::ptr::null(); 3],
            description: None,
        });
        s.table = [&s.parameters[0], &s.parameters[1], &s.parameters[2]];
        s.description = Some(DspDescription {
            pluginsdkversion: 0x6c,
            name: name32("FMOD Distance Filter"),
            version: 0x0001_0000,
            numinputbuffers: 1,
            numoutputbuffers: 1,
            create: create as *const () as usize,
            release: release as *const () as usize,
            reset: reset as *const () as usize,
            read: read as *const () as usize,
            process: 0,
            setposition: 0,
            numparameters: 3,
            paramdesc: s.table.as_ptr(),
            setparameterfloat: set_parameter_float as *const () as usize,
            setparameterint: 0,
            setparameterbool: 0,
            setparameterdata: set_parameter_data as *const () as usize,
            getparameterfloat: get_parameter_float as *const () as usize,
            getparameterint: 0,
            getparameterbool: 0,
            getparameterdata: get_parameter_data as *const () as usize,
            shouldiprocess: should_i_process as *const () as usize,
            userdata: std::ptr::null_mut(),
            sys_register: 0,
            sys_deregister: 0,
            sys_mix: 0,
        });
        s
    });
    statics.description.as_ref().unwrap() as *const DspDescription as *const c_void
}

/// `FMODDistanceFilterState::updateTimeConstants` @ 0x1401fcf20
#[allow(clippy::neg_cmp_op_on_partial_ord)]
fn update_time_constants(s: &mut State) {
    let mut d = s.distance_to_listener;
    let m = s.max_distance;
    // comiss d,m ; jb: below or unordered divides
    d = if !(d >= m) { d / m } else { 1.0 };
    let f = s.bandpass_frequency;
    let rate = s.sample_rate as f32;
    let mut a = 1.0 - d;
    let b = 22000.0 - f;
    let dd = d * d;
    a *= a;
    a *= b;
    let dt = 1.0 / rate;
    let thr = rate * 0.318_309_87;
    let lp = a + f;
    let mut h = f - 10.0;
    h *= dd;
    let hp = h + 10.0;

    if lp >= 22000.0 {
        s.target_lowpass_time_const = 1.0;
    } else if lp > thr {
        let mut n = lp - thr;
        let mut q = 22000.0 - thr;
        q *= 3.0;
        n /= q;
        s.target_lowpass_time_const = n + 0.666_666_7;
    } else {
        let w = lp * 6.283_185_5;
        let mut rc = 1.0 / w;
        rc += dt;
        s.target_lowpass_time_const = dt / rc;
    }

    if hp >= 22000.0 {
        s.target_highpass_time_const = 0.0;
    } else if hp > thr {
        let n = 22000.0 - hp;
        let mut q = 22000.0 - thr;
        q *= 3.0;
        s.target_highpass_time_const = n / q;
    } else {
        let w = hp * 6.283_185_5;
        let rc = 1.0 / w;
        let t = rc + dt;
        s.target_highpass_time_const = rc / t;
    }
    s.ramp_samples_left = 0x100;
}

unsafe fn reset_state(s: &mut State) {
    s.current_lowpass_time_const = s.target_lowpass_time_const;
    s.current_highpass_time_const = s.target_highpass_time_const;
    s.ramp_samples_left = 0;
    let n = s.max_channels as isize as usize;
    std::ptr::write_bytes(s.previous_lp1_out, 0, n);
    std::ptr::write_bytes(s.previous_lp2_out, 0, n);
    std::ptr::write_bytes(s.previous_hp_out, 0, n);
}

/// `FMOD_DistanceFilter_dspcreate` @ 0x1401fc3f0 with `FMODDistanceFilterState::init` @ 0x1401fc820
unsafe extern "C" fn create(state: *mut DspState) -> i32 {
    let cb = || &*(*state).callbacks;
    let p = (cb().alloc)(0x40, 0, c"FMODDistanceFilterState".as_ptr()) as *mut State;
    if p.is_null() {
        // the game would have crashed in init
        (*state).plugindata = std::ptr::null_mut();
        return FMOD_ERR_MEMORY;
    }
    let s = &mut *p;
    (cb().getsamplerate)(state, &mut s.sample_rate);
    s.max_channels = 8;
    s.max_distance = 20.0;
    s.bandpass_frequency = 1500.0;
    s.distance_to_listener = 0.0;
    s.previous_lp1_out = (cb().alloc)(0x20, 0, c"Previous Lowpass1 outputs".as_ptr()) as *mut f32;
    s.previous_lp2_out = (cb().alloc)((s.max_channels << 2) as u32, 0, c"Previous Lowpass2 outputs".as_ptr()) as *mut f32;
    s.previous_hp_out = (cb().alloc)((s.max_channels << 2) as u32, 0, c"Previous Highpass outputs".as_ptr()) as *mut f32;
    update_time_constants(s);
    reset_state(s);
    (*state).plugindata = p.cast();
    0
}

/// `FMOD_DistanceFilter_dsprelease` @ 0x1401fc440
unsafe extern "C" fn release(state: *mut DspState) -> i32 {
    let s = (*state).plugindata as *mut State;
    let cb = || &*(*state).callbacks;
    (cb().free)((*s).previous_lp1_out.cast(), 0, c"Previous Lowpass1 outputs".as_ptr());
    (cb().free)((*s).previous_lp2_out.cast(), 0, c"Previous Lowpass2 outputs".as_ptr());
    (cb().free)((*s).previous_hp_out.cast(), 0, c"Previous Highpass outputs".as_ptr());
    (cb().free)(s.cast(), 0, c"FMODDistanceFilterState".as_ptr());
    0
}

/// `FMOD_DistanceFilter_dspreset` @ 0x1401fc4b0
unsafe extern "C" fn reset(state: *mut DspState) -> i32 {
    reset_state(&mut *((*state).plugindata as *mut State));
    0
}

/// `FMOD_DistanceFilter_dspread` @ 0x1401fc510 -> `FMODDistanceFilterState::process` @ 0x1401fc900
unsafe extern "C" fn read(state: *mut DspState, input: *mut f32, output: *mut f32, length: u32, channels: i32, _out_channels: *mut i32) -> i32 {
    let s = &mut *((*state).plugindata as *mut State);
    if channels > s.max_channels {
        return FMOD_ERR_INVALID_PARAM;
    }
    let mut lpc = s.current_lowpass_time_const;
    let mut hpc = s.current_highpass_time_const;
    let n = channels as isize;
    let mut input = input;
    let mut output = output;
    let mut length = length;
    let mut dc;

    // one frame: every channel through low-pass, low-pass, high-pass
    macro_rules! frame {
        () => {
            let mut c = 0isize;
            while c < n {
                let p1 = *s.previous_lp1_out.offset(c);
                let mut lp1 = dc + *input;
                lp1 -= p1;
                lp1 *= lpc;
                lp1 += p1;
                let p2 = *s.previous_lp2_out.offset(c);
                let mut lp2 = lp1 - p2;
                lp2 *= lpc;
                lp2 += p2;
                let mut o = lp2 + *s.previous_hp_out.offset(c);
                o -= p2;
                o *= hpc;
                *output = o;
                *s.previous_lp1_out.offset(c) = lp1;
                *s.previous_lp2_out.offset(c) = lp2;
                *s.previous_hp_out.offset(c) = *output;
                input = input.add(1);
                output = output.add(1);
                c += 1;
            }
        };
    }

    let mut plain = true;
    if s.ramp_samples_left != 0 {
        let r = s.ramp_samples_left as f32;
        let inv = 1.0 / r;
        let dlp = (s.target_lowpass_time_const - lpc) * inv;
        let dhp = (s.target_highpass_time_const - hpc) * inv;
        plain = false;
        if length != 0 {
            dc = f32::from_bits(DC.load(Ordering::Relaxed));
            loop {
                s.ramp_samples_left -= 1;
                if s.ramp_samples_left == 0 {
                    // the ramp is over: this frame and the rest go the plain way
                    lpc = s.target_lowpass_time_const;
                    hpc = s.target_highpass_time_const;
                    plain = true;
                    break;
                }
                lpc += dlp;
                hpc += dhp;
                frame!();
                dc = -dc;
                DC.store(dc.to_bits(), Ordering::Relaxed);
                length -= 1;
                if length == 0 {
                    break;
                }
            }
        }
    }
    if plain && length != 0 {
        dc = f32::from_bits(DC.load(Ordering::Relaxed));
        while length != 0 {
            length -= 1;
            frame!();
            dc = -dc;
        }
        DC.store(dc.to_bits(), Ordering::Relaxed);
    }
    s.current_highpass_time_const = hpc;
    s.current_lowpass_time_const = lpc;
    0
}

/// `FMOD_DistanceFilter_dspsetparamfloat` @ 0x1401fc520
unsafe extern "C" fn set_parameter_float(state: *mut DspState, index: i32, value: f32) -> i32 {
    let s = &mut *((*state).plugindata as *mut State);
    match index {
        0 => s.max_distance = value,
        1 => s.bandpass_frequency = value,
        _ => return FMOD_ERR_INVALID_PARAM,
    }
    update_time_constants(s);
    0
}

/// `FMOD_DistanceFilter_dspsetparamdata` @ 0x1401fc560: the position relative to the listener.
unsafe extern "C" fn set_parameter_data(state: *mut DspState, index: i32, data: *mut c_void, _length: u32) -> i32 {
    if index != 2 {
        return FMOD_ERR_INVALID_PARAM;
    }
    let s = &mut *((*state).plugindata as *mut State);
    let p = data as *const f32;
    let (x, y, z) = (*p, *p.add(1), *p.add(2));
    let yy = y * y;
    let xx = x * x;
    let zz = z * z;
    let mut t = xx + yy;
    t += zz;
    s.distance_to_listener = t.sqrt();
    update_time_constants(s);
    0
}

/// `FMOD_DistanceFilter_dspgetparamfloat` @ 0x1401fc5c0
unsafe extern "C" fn get_parameter_float(state: *mut DspState, index: i32, value: *mut f32, text: *mut c_char) -> i32 {
    let s = &*((*state).plugindata as *mut State);
    match index {
        0 => {
            *value = s.max_distance;
            if !text.is_null() {
                sprintf_f(text, c"%.1f", s.max_distance as f64);
            }
        }
        1 => {
            *value = s.bandpass_frequency;
            if !text.is_null() {
                sprintf_f(text, c"%.1f Hz", s.bandpass_frequency as f64);
            }
        }
        _ => return FMOD_ERR_INVALID_PARAM,
    }
    0
}

/// `FMOD_DistanceFilter_dspgetparamdata` @ 0x1401fc630
unsafe extern "C" fn get_parameter_data(_state: *mut DspState, _index: i32, _data: *mut *mut c_void, _length: *mut u32, _text: *mut c_char) -> i32 {
    FMOD_ERR_INVALID_PARAM
}
