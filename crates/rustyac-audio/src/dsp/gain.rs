// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! "FMOD Gain" (fmod_gain.obj): a gain in dB with a 256-frame ramp and an invert switch.

use std::ffi::{c_char, c_void};
use std::sync::OnceLock;

use super::*;

/// `FMODGainState` (0x10 bytes, from FMOD's allocator; the plug-in never initialises it)
#[repr(C)]
struct State {
    target_gain: f32,
    current_gain: f32,
    ramp_samples_left: i32,
    invert: u8,
}

static GAIN_VALUES: [f32; 5] = [-80.0, -50.0, -30.0, -10.0, 10.0];
static GAIN_POSITIONS: [f32; 5] = [0.0, 2.0, 4.0, 7.0, 11.0];

struct Statics {
    parameters: [ParameterDesc; 2],
    table: [*const ParameterDesc; 2],
    description: Option<DspDescription>,
}
// SAFETY: built once behind the OnceLock and only read afterwards.
unsafe impl Sync for Statics {}
unsafe impl Send for Statics {}

/// `FMODGetDSPDescription_Gain` @ 0x1401fd240
pub fn description() -> *const c_void {
    static CELL: OnceLock<Box<Statics>> = OnceLock::new();
    let statics = CELL.get_or_init(|| {
        let mut s = Box::new(Statics {
            parameters: [
                ParameterDesc::new(0, "Gain", "dB", c"Gain in dB. -80 to 10. Default = 0").float(GAIN_VALUES[0], GAIN_VALUES[4], 0.0, 2).points(&GAIN_VALUES, &GAIN_POSITIONS),
                ParameterDesc::new(2, "Invert", "", c"Invert signal. Default = off"),
            ],
            table: [std::ptr::null(); 2],
            description: None,
        });
        s.table = [&s.parameters[0], &s.parameters[1]];
        s.description = Some(DspDescription {
            pluginsdkversion: 0x6c,
            name: name32("FMOD Gain"),
            version: 0x0001_0000,
            numinputbuffers: 1,
            numoutputbuffers: 1,
            create: create as *const () as usize,
            release: release as *const () as usize,
            reset: reset as *const () as usize,
            read: read as *const () as usize,
            process: 0,
            setposition: 0,
            numparameters: 2,
            paramdesc: s.table.as_ptr(),
            setparameterfloat: set_parameter_float as *const () as usize,
            setparameterint: 0,
            setparameterbool: set_parameter_bool as *const () as usize,
            setparameterdata: 0,
            getparameterfloat: get_parameter_float as *const () as usize,
            getparameterint: 0,
            getparameterbool: get_parameter_bool as *const () as usize,
            getparameterdata: 0,
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

/// `FMOD_Gain_dspcreate` @ 0x1401fd060
unsafe extern "C" fn create(state: *mut DspState) -> i32 {
    let p = ((*(*state).callbacks).alloc)(0x10, 0, c"FMODGainState".as_ptr());
    (*state).plugindata = p;
    if p.is_null() {
        FMOD_ERR_MEMORY
    } else {
        0
    }
}

/// `FMOD_Gain_dsprelease` @ 0x1401fd0a0
unsafe extern "C" fn release(state: *mut DspState) -> i32 {
    ((*(*state).callbacks).free)((*state).plugindata, 0, c"FMODGainState".as_ptr());
    0
}

/// `FMOD_Gain_dspreset` @ 0x1401fd0c0
unsafe extern "C" fn reset(state: *mut DspState) -> i32 {
    let s = &mut *((*state).plugindata as *mut State);
    s.ramp_samples_left = 0;
    s.current_gain = s.target_gain;
    0
}

/// `FMOD_Gain_dspread` @ 0x1401fd0e0 -> `FMODGainState::process` @ 0x1401fd3f0
unsafe extern "C" fn read(state: *mut DspState, input: *mut f32, output: *mut f32, length: u32, channels: i32, _out_channels: *mut i32) -> i32 {
    let s = &mut *((*state).plugindata as *mut State);
    let mut g = s.current_gain;
    let mut input = input;
    let mut output = output;
    let mut length = length;
    if s.ramp_samples_left != 0 {
        let tgt = s.target_gain;
        let delta = (tgt - g) / s.ramp_samples_left as f32;
        while length != 0 {
            s.ramp_samples_left -= 1;
            if s.ramp_samples_left == 0 {
                g = tgt;
                break;
            }
            g += delta;
            if channels > 0 {
                for _ in 0..channels {
                    *output = g * *input;
                    output = output.add(1);
                    input = input.add(1);
                }
            }
            length -= 1;
        }
    }
    let mut count = length.wrapping_mul(channels as u32);
    while count != 0 {
        *output = g * *input;
        output = output.add(1);
        input = input.add(1);
        count -= 1;
    }
    s.current_gain = g;
    0
}

/// `FMODGainState::setGain` @ 0x1401fd590: dB to a linear gain, 256 frames to get there.
fn set_gain(s: &mut State, db: f32) {
    let above = db > -80.0;
    if s.invert != 0 {
        if above {
            let t = rustyac_math::powf(10.0, db * 0.05);
            s.ramp_samples_left = 0x100;
            s.target_gain = -t;
        } else {
            s.ramp_samples_left = 0x100;
            s.target_gain = -0.0;
        }
    } else if above {
        s.target_gain = rustyac_math::powf(10.0, db * 0.05);
        s.ramp_samples_left = 0x100;
    } else {
        s.ramp_samples_left = 0x100;
        s.target_gain = 0.0;
    }
}

/// `FMODGainState::gain` @ 0x1401fd380, with the plug-in's own slip: an inverted gain that is
/// not zero reads as -80 dB.
#[allow(clippy::neg_cmp_op_on_partial_ord, clippy::float_cmp)]
fn gain(s: &State) -> f32 {
    let t = s.target_gain;
    let inverted = s.invert != 0;
    let c = if inverted {
        -t
    } else if !(t > 0.0) {
        1.0
    } else {
        0.0
    };
    // ucomiss c,0 ; je: equal or unordered goes on to the logarithm
    if c < 0.0 || c > 0.0 {
        return -80.0;
    }
    let x = if inverted { -t } else { t };
    log10f(x) * 20.0
}

/// `FMOD_Gain_dspsetparamfloat` @ 0x1401fd100
unsafe extern "C" fn set_parameter_float(state: *mut DspState, index: i32, value: f32) -> i32 {
    if index != 0 {
        return FMOD_ERR_INVALID_PARAM;
    }
    set_gain(&mut *((*state).plugindata as *mut State), value);
    0
}

/// `FMOD_Gain_dspsetparambool` @ 0x1401fd130
unsafe extern "C" fn set_parameter_bool(state: *mut DspState, index: i32, value: i32) -> i32 {
    if index != 1 {
        return FMOD_ERR_INVALID_PARAM;
    }
    let s = &mut *((*state).plugindata as *mut State);
    let b = (value != 0) as u8;
    if b != s.invert {
        s.ramp_samples_left = 0x100;
        s.target_gain = -s.target_gain;
    }
    s.invert = b;
    0
}

/// `FMOD_Gain_dspgetparamfloat` @ 0x1401fd170
unsafe extern "C" fn get_parameter_float(state: *mut DspState, index: i32, value: *mut f32, text: *mut c_char) -> i32 {
    if index != 0 {
        return FMOD_ERR_INVALID_PARAM;
    }
    let s = &*((*state).plugindata as *mut State);
    *value = gain(s);
    if !text.is_null() {
        sprintf_f(text, c"%.1f dB", gain(s) as f64);
    }
    0
}

/// `FMOD_Gain_dspgetparambool` @ 0x1401fd1f0
unsafe extern "C" fn get_parameter_bool(state: *mut DspState, index: i32, value: *mut i32, text: *mut c_char) -> i32 {
    if index != 1 {
        return FMOD_ERR_INVALID_PARAM;
    }
    let s = &*((*state).plugindata as *mut State);
    *value = s.invert as i32;
    if !text.is_null() {
        copy_text(text, if s.invert != 0 { "Inverted" } else { "Off" });
    }
    0
}

#[allow(dead_code)]
fn _unused(_: *mut c_void) {}
