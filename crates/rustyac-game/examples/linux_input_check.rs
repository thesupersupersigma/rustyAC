// SPDX-License-Identifier: GPL-3.0-or-later

//! Task P3b (L4): the Linux devices against Windows' input path. Driven on its standard input
//! by `re/scratch/taskP3/fakedev.py`, which makes a kernel Xbox 360 pad and a force-feedback
//! wheel (`uinput`), sets them to raw values and sends the same values here:
//!
//! - `pad <buttons> <lt> <rt> <lx> <ly> <rx> <ry>`: XInput's raw state; the pad read through
//!   SDL must be that state, and AC's pad class must make the same controls from both;
//! - `wheel <x> <z> <rz> <throttle> <pov> <buttons>`: the wheel's raw values (ranges 0..65535,
//!   0..255, 0..255, 0..1023); DirectInput's state of them (its range mapped as `MulDiv`
//!   does) must be what the wheel read through SDL gives, and AC's wheel class must make the
//!   same controls from both;
//! - `ff <f32>`: a force sent as DirectInput's path sends it; prints the magnitude DirectInput
//!   would get, the script compares it with what reached the kernel;
//! - `rumble <left> <right>`: prints XInput's two motor words, as `ff`.
//!
//! `cargo run -p rustyac-game --features wgpu --example linux_input_check` (Linux only)

#[cfg(all(target_os = "linux", feature = "wgpu"))]
fn main() {
    use std::io::BufRead;

    use rustyac_game::input::bindings::{default_ini, Bindings};
    use rustyac_game::input::dinput::{axis_value, magnitude, DiState, FF_CAP};
    use rustyac_game::input::pad::{motor_word, JoypadCarControl, PadState, XInput};
    use rustyac_game::input::wheel::{DiCarControl, WheelDevice};
    use rustyac_game::input::Extra;
    use rustyac_physics::car::feed::CarControls;
    use rustyac_physics::car::CarControlsInput;

    // the bindings: AC's defaults, the wheel on JOY 1 behind the pad on JOY 0 as Windows
    // listed them ([CONTROLLERS]); SDL finds the wheel first
    let mut ini = default_ini();
    for (section, key, value) in [
        ("HEADER", "INPUT_METHOD", "WHEEL"),
        ("CONTROLLERS", "CON0", "Controller (XBOX 360 For Windows)"),
        ("CONTROLLERS", "PGUID0", "028E045E-0000-0000-0000-504944564944"),
        ("CONTROLLERS", "CON1", "Logitech G29 Driving Force Racing Wheel USB"),
        ("CONTROLLERS", "PGUID1", "C24F046D-0000-0000-0000-504944564944"),
        ("STEER", "JOY", "1"),
        ("STEER", "AXLE", "0"),
        ("STEER", "LOCK", "900"),
        ("STEER", "FF_GAIN", "1"),
        ("THROTTLE", "JOY", "1"),
        ("THROTTLE", "AXLE", "2"),
        ("THROTTLE", "MIN", "1"),
        ("THROTTLE", "MAX", "-1"),
        ("BRAKES", "JOY", "1"),
        ("BRAKES", "AXLE", "5"),
        ("BRAKES", "MIN", "1"),
        ("BRAKES", "MAX", "-1"),
        ("CLUTCH", "JOY", "1"),
        ("CLUTCH", "AXLE", "6"),
        ("CLUTCH", "MIN", "1"),
        ("CLUTCH", "MAX", "-1"),
        ("GEARUP", "JOY", "1"),
        ("GEARUP", "BUTTON", "4"),
        ("GEARDN", "JOY", "1"),
        ("GEARDN", "BUTTON", "5"),
    ] {
        ini.set(section, key, value);
    }
    let bindings = Bindings::from_ini(&ini, "linux_input_check".to_string());
    let mut opened = WheelDevice::open(&bindings, 0, true);
    print!("{}", rustyac_game::input::dinput::describe_devices(&opened));
    let wheel = opened.as_mut().expect("SDL's joysticks");
    let mut xinput = XInput::new();
    println!("  pad: {:?}", xinput.index);
    let joy = wheel.steer_joy().expect("the wheel is JOY 1");
    println!("READY steer_joy={joy} ffb={}", wheel.ffb);

    let mut pads = [JoypadCarControl::from_ini(&bindings.ini, false), JoypadCarControl::from_ini(&bindings.ini, false)];
    let mut windows_wheel = DiCarControl::from_ini(&bindings.ini);
    let mut controls = [CarControls::default(); 4];
    let (mut same, mut different) = (0, 0);
    // DirectInput's -10000..10000 of a raw value in min..max (`MulDiv`: half rounds up)
    let muldiv = |v: i64, max: i64| axis_value((((v * 20000) + max / 2) / max - 10000) as i32);
    for (n, line) in std::io::stdin().lock().lines().enumerate() {
        let line = line.expect("stdin");
        let f: Vec<&str> = line.split_whitespace().collect();
        let num = |k: usize| f[k].parse::<i64>().expect("number");
        let input = CarControlsInput { steer_lock: 540.0, speed: (n % 90) as f32 };
        let verdict = match f.first().copied() {
            Some("pad") => {
                let raw = PadState {
                    buttons: num(1) as u16,
                    left_trigger: num(2) as u8,
                    right_trigger: num(3) as u8,
                    thumb_lx: num(4) as i16,
                    thumb_ly: num(5) as i16,
                    thumb_rx: num(6) as i16,
                    thumb_ry: num(7) as i16,
                };
                let linux = xinput.poll().unwrap_or_default();
                let mut extras = [Extra::default(); 2];
                for (k, state) in [linux, raw].iter().enumerate() {
                    pads[k].acquire_controls(state, &mut controls[k], &mut extras[k], &input, &|_| false);
                }
                (linux == raw && controls[0] == controls[1] && extras[0] == extras[1], format!("sdl {linux:?} raw {raw:?} steer {} {}", controls[0].steer, controls[1].steer))
            }
            Some("wheel") => {
                let mut windows = DiState::default();
                windows.axes[0] = muldiv(num(1), 65535);
                windows.axes[2] = muldiv(num(2), 255);
                windows.axes[5] = muldiv(num(3), 255);
                windows.axes[6] = muldiv(num(4), 1023);
                windows.povs = [num(5) as u32, u32::MAX, u32::MAX, u32::MAX];
                for b in 0..16 {
                    windows.buttons[b] = if num(6) >> b & 1 != 0 { 0x80 } else { 0 };
                }
                wheel.poll();
                let linux = *wheel.di.state(joy).expect("the wheel");
                let mut extras = [Extra::default(); 2];
                wheel.acquire_controls(&mut controls[2], &mut extras[0], 1.0 / 333.0, &input, &|_| false);
                windows_wheel.acquire_controls(&|j| (j == joy).then_some(windows), &mut controls[3], &mut extras[1], 1.0 / 333.0, &input, &|_| false);
                let axes = |s: &DiState| s.axes.map(f32::to_bits);
                (
                    axes(&linux) == axes(&windows) && linux.povs == windows.povs && linux.buttons == windows.buttons && controls[2] == controls[3] && extras[0] == extras[1],
                    format!("sdl {:?} {:?} directinput {:?} {:?} gas {} {}", linux.axes, linux.povs[0], windows.axes, windows.povs[0], controls[2].gas, controls[3].gas),
                )
            }
            Some("ff") => {
                let ff: f32 = f[1].parse().expect("force");
                wheel.di.send_ff(joy, ff);
                println!("ff {} magnitude {}", f[1], magnitude(ff.clamp(-1.0, 1.0) * FF_CAP));
                continue;
            }
            Some("rumble") => {
                let (left, right): (f32, f32) = (f[1].parse().expect("left"), f[2].parse().expect("right"));
                xinput.rumble(left, right);
                println!("rumble {} {} words {} {}", f[1], f[2], motor_word(left), motor_word(right));
                continue;
            }
            Some("quit") => break,
            _ => panic!("line {n}: {line}"),
        };
        if verdict.0 {
            same += 1;
            println!("{n} IDENTICAL");
        } else {
            different += 1;
            println!("{n} DIFFERENT {}", verdict.1);
        }
    }
    println!("checked {} lines: {same} IDENTICAL, {different} DIFFERENT", same + different);
}

#[cfg(not(all(target_os = "linux", feature = "wgpu")))]
fn main() {
    eprintln!("linux_input_check: Linux with --features wgpu only");
}
