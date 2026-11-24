// SPDX-License-Identifier: GPL-3.0-or-later

//! Custom Shaders Patch 0.3.2-preview2 (build 4305): CSP's hook on the game's
//! `NetCarStateProvider::onRemoteStateReceived` (`dwrite.dll` @ 0x18061ec60, installed as the
//! add's detour, and called straight by the `AB 03` reader). Every remote state goes through
//! it, whatever message brought it (`docs/csp/assettoserver.md` §NH-5.1). Only `--csp` uses
//! it; without it the add is the game's own.
//!
//! * A state with a position, rotation or velocity value at or above `FLT_MAX` is dropped,
//!   and so is one with a NaN there, except a NaN heading (the tests are `comiss` both ways).
//! * A **teleport** fills the car's three history states with this one, 30, 60 and 90 ms older,
//!   and puts the body there, so the car snaps instead of sliding across the jump. A
//!   teleport is a car that was disconnected (AC's `isDisconnected`, also set after five
//!   seconds without news), or, with the server's `RESET` motion setting, a state with
//!   status bit `0x20000000` or 50 m or more from where the car is drawn.
//! * It measures each car's send interval (time-stamp step over sequence step, smoothed by
//!   0.1). With the server's `EXTRAPOLATE_STATE` set to raw, the add uses this state's own
//!   interval in place of the client's.
//!
//! Every float expression is in the order of CSP's machine code.

// Comparisons are spelled the way the original branches so NaN takes the same path.
#![allow(clippy::neg_cmp_op_on_partial_ord)]

use crate::csp::IniSection;
use crate::net_car::{NetCarState, NetCarStateProvider};
use rustyac_physics::vecmath::Mat44f;

/// The build from which a server without `[EXTRA_TWEAKS] CUSTOM_MOTION` gets `RESET` (0xCC2).
pub const RESET_BY_DEFAULT_FROM: u32 = 3266;

/// The status bit a sender sets on a teleport (a reset to the pits, a respawn).
pub const TELEPORT_BIT: u32 = 0x2000_0000;

/// CSP's state-add hook and what it keeps between states.
#[derive(Clone, Debug)]
pub struct CspMotion {
    /// The server's `csp/<build>/../` number (`dword_1827FAD98`).
    pub build: u32,
    /// `RESET` (`byte_1827F9388`): a jump of 50 m or the teleport bit snaps the car.
    pub reset: bool,
    /// `EXTRAPOLATE_STATE`'s third item is `RAW` (`word_182803260` high byte): the add
    /// uses the state's own measured interval.
    pub raw_interval: bool,
    /// The measured send interval of each car (`dword_182813040[guid]`), ms. Read by CSP's
    /// body-matrix smoothing (`SMOOTH`), which is not ported.
    pub measured: [f32; 256],
}

/// The items of a list value as CSP's INI reader gives them (comma separated, trimmed).
fn items(value: &str) -> impl Iterator<Item = &str> {
    value.split(',').map(str::trim).filter(|item| !item.is_empty())
}

impl CspMotion {
    /// The hook before the server's settings arrive: `RESET` off (CSP's byte starts at 0),
    /// `build` the server's `csp/<build>/../` name (0 without one).
    pub fn new(build: u32) -> CspMotion {
        CspMotion { build, reset: false, raw_interval: false, measured: [0.0; 256] }
    }

    /// The settings as CSP reads them (`dwrite.dll` @ 0x1805596e0) from the welcome's
    /// `$CSP0:` text: `[EXTRA_TWEAKS] CUSTOM_MOTION` (a list; `RESET` and `SMOOTH` are
    /// its items, compared exactly) and `[CUSTOM_PHYSICS] EXTRAPOLATE_STATE`. Without
    /// `CUSTOM_MOTION`, `RESET` is on from build 3266 on.
    pub fn read_settings(&mut self, settings: &[IniSection]) {
        let value = |section: &str, key: &str| settings.iter().find(|(name, _)| name == section).and_then(|(_, keys)| keys.iter().find(|(name, _)| name == key)).map(|(_, value)| value.as_str());
        self.reset = match value("EXTRA_TWEAKS", "CUSTOM_MOTION") {
            Some(list) => items(list).any(|item| item == "RESET"),
            None => self.build >= RESET_BY_DEFAULT_FROM,
        };
        self.raw_interval = value("CUSTOM_PHYSICS", "EXTRAPOLATE_STATE").is_some_and(|list| items(list).nth(2) == Some("RAW"));
    }

    /// The hook (@ 0x18061ec60): the drop and teleport test (@ 0x18061e750), the interval
    /// measure, then the game's add. Returns whether the state was taken.
    pub fn add(&mut self, car: &mut NetCarStateProvider, new: NetCarState, physics_time: f64, send_interval: f64) -> bool {
        if drops(&new) {
            return false;
        }
        let p = new.pos;
        let jump = new.status_bytes & TELEPORT_BIT != 0 || {
            let b = &car.body_matrix.m[3];
            let (dx, dy, dz) = (p[0] - b[0], p[1] - b[1], p[2] - b[2]);
            (dx * dx + dy * dy) + dz * dz >= 2500.0
        };
        if (jump && self.reset) || car.is_disconnected {
            let m = euler_matrix(new.rotation);
            for (row, out) in car.body_matrix.m.iter_mut().enumerate() {
                *out = [m.m[0][row], m.m[1][row], m.m[2][row], m.m[3][row]];
            }
            car.body_matrix.m[3][..3].copy_from_slice(&p);
            for (k, state) in car.net_states.iter_mut().enumerate() {
                let back = 30.0 * (k + 1) as f64;
                *state = new;
                state.pak_sequence_id = new.pak_sequence_id.wrapping_sub(k as u8 + 1);
                state.rcv_time = physics_time - back;
                state.time_stamp = new.time_stamp - back;
            }
        }
        let steps = {
            let d = new.pak_sequence_id as i32 - car.net_states[0].pak_sequence_id as i32;
            if d >= 0 { d } else { d + 256 }
        };
        let dt = new.time_stamp - car.net_states[0].time_stamp;
        let interval = dt / steps.max(1) as f64;
        let far = dt.abs() > 1000.0;
        if far || steps > 0 {
            if let Some(m) = self.measured.get_mut(car.guid as usize) {
                *m = if far || steps > 10 {
                    0.0
                } else {
                    let now = interval as f32;
                    // ucomiss + jz: equal or unordered keeps the new value
                    if *m == 0.0 || m.is_nan() { now } else { (now - *m) * f32::from_bits(0x3dcc_cccd) + *m }
                };
            }
        }
        // (the swap also needs RaceManager::ini to have run, byte_1827F9495: always so in a race)
        car.on_remote_state_received(new, physics_time, if self.raw_interval { interval } else { send_interval })
    }
}

/// The hook's drop test (@ 0x18061e750): a position, rotation or velocity value at or above
/// `FLT_MAX`, or NaN (`comiss MAX, x; jbe`); the heading is tested the other way round
/// (`comiss x, MAX; jae`), so a NaN heading passes. (rustyAC's client drops every state with
/// a value that is no finite number before this: see `Client::on_datagram`.)
pub fn drops(new: &NetCarState) -> bool {
    const MAX: f32 = f32::MAX;
    let [p, r, v] = [new.pos, new.rotation, new.velocity];
    !(MAX > p[0]) || !(MAX > p[1]) || !(MAX > p[2]) || r[0] >= MAX || !(MAX > r[1]) || !(MAX > r[2]) || !(MAX > v[0]) || !(MAX > v[1]) || !(MAX > v[2])
}

/// CSP's rotation about a unit axis (@ 0x180307e60): `(I - aa) cos + aa + [a]x sin`, the
/// axis normalised first. CSP's own `sinf` / `cosf` (0x181dc7c90 / 0x181dc75f0) give the
/// game's MSVCR120 bits for all 2^32 inputs (`csp_oracle --maths`, NH-5), so `rustyac_math`'s.
fn axis_angle(axis: [f32; 3], angle: f32) -> [[f32; 3]; 3] {
    let n = 1.0f32 / ((axis[1] * axis[1] + axis[0] * axis[0]) + axis[2] * axis[2]).sqrt();
    let (x, y, z) = (n * axis[0], n * axis[1], n * axis[2]);
    let aa = [x * x, y * x, z * x, y * x, y * y, z * y, z * x, z * y, z * z];
    let s = rustyac_math::sinf(angle);
    let (zs, ys, xs) = (z * s, y * s, x * s);
    let c = rustyac_math::cosf(angle);
    let i = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0f32];
    let e = |k: usize| (i[k] - aa[k]) * c + aa[k];
    [[e(0), e(1) - zs, e(2) + ys], [e(3) + zs, e(4), e(5) - xs], [e(6) - ys, e(7) + xs, e(8)]]
}

/// CSP's matrix product `a * b` (@ 0x180307830): each value `(p0 + p2) + (p1 + p3)`.
fn multiply(a: &Mat44f, b: &Mat44f) -> Mat44f {
    let mut out = Mat44f { m: [[0.0; 4]; 4] };
    for (row, result) in a.m.iter().zip(out.m.iter_mut()) {
        for (col, value) in result.iter_mut().enumerate() {
            let p = [row[0] * b.m[0][col], row[1] * b.m[1][col], row[2] * b.m[2][col], row[3] * b.m[3][col]];
            *value = (p[0] + p[2]) + (p[1] + p[3]);
        }
    }
    out
}

fn mat(r: [[f32; 3]; 3]) -> Mat44f {
    Mat44f { m: [[r[0][0], r[0][1], r[0][2], 0.0], [r[1][0], r[1][1], r[1][2], 0.0], [r[2][0], r[2][1], r[2][2], 0.0], [0.0, 0.0, 0.0, 1.0]] }
}

/// CSP's matrix of a state's Euler angles (@ 0x180306bf0 with the angles negated, as the
/// teleport passes them): heading about Y, pitch about X, roll about Z. The body is its
/// transpose.
pub fn euler_matrix(r: [f32; 3]) -> Mat44f {
    let z = mat(axis_angle([0.0, 0.0, 1.0], -r[2]));
    let x = mat(axis_angle([1.0, 0.0, 0.0], -r[1]));
    let y = mat(axis_angle([0.0, 1.0, 0.0], -r[0]));
    multiply(&multiply(&y, &x), &z)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(build: u32, text: &str) -> CspMotion {
        let mut motion = CspMotion::new(build);
        motion.read_settings(&crate::csp::parse_ini(text));
        motion
    }

    #[test]
    fn settings_and_defaults() {
        assert!(!CspMotion::new(4305).reset);
        // AssettoServer pre49's defaults: csp/2651, no motion keys
        let none = read(2651, "[EXTRA_TWEAKS]
MIN_TIME_BETWEEN_COLLISIONS = 2
");
        assert!(!none.reset && !none.raw_interval);
        assert!(read(3266, "").reset);
        // the key wins over the build either way
        assert!(!read(4305, "[EXTRA_TWEAKS]
CUSTOM_MOTION = SMOOTH
").reset);
        assert!(read(0, "[EXTRA_TWEAKS]
CUSTOM_MOTION = SMOOTH, RESET
").reset);
        assert!(!read(0, "[EXTRA_TWEAKS]
CUSTOM_MOTION = reset
").reset);
        assert!(read(0, "[CUSTOM_PHYSICS]
EXTRAPOLATE_STATE = A, B, RAW
").raw_interval);
        assert!(!read(0, "[CUSTOM_PHYSICS]
EXTRAPOLATE_STATE = RAW
").raw_interval);
    }

    #[test]
    fn euler_matrix_is_a_rotation() {
        assert_eq!(euler_matrix([0.0; 3]).m, Mat44f::IDENTITY.m);
        // a quarter turn of heading: CSP's (not the game's) signs
        let m = euler_matrix([std::f32::consts::FRAC_PI_2, 0.0, 0.0]);
        assert!((m.m[0][2] + 1.0).abs() < 1e-6 && (m.m[2][0] - 1.0).abs() < 1e-6, "{:?}", m.m);
    }
}
