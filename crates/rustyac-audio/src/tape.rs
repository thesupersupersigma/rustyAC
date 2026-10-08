// SPDX-License-Identifier: GPL-3.0-or-later

//! The audio tape `car_oracle run --audio-tape` writes: the game's own `CarPhysicsState` at
//! 60 frames a second and every physics event, the input both the game's sound code and the
//! port are run on (`tools/audio_oracle`).
//!
//! A header of `key=value` lines ends with an empty line; then records: `1` + 0x48 bytes = an
//! `ACPhysicsEvent`; `2` + u32 step count + 0xb70 bytes + 1 byte = a frame (the state after
//! that many physics steps, and whether the traction control is acting).

use std::path::Path;

pub const STATE_SIZE: usize = 0xb70;
pub const EVENT_SIZE: usize = 0x48;

pub struct TapeFrame {
    /// Physics steps done when the frame was taken.
    pub steps: u32,
    /// The raw `CarPhysicsState`.
    pub state: Vec<u8>,
    pub tc_in_action: bool,
    /// The raw physics events since the frame before.
    pub events: Vec<[u8; EVENT_SIZE]>,
}

pub struct Tape {
    pub header: Vec<(String, String)>,
    pub frames: Vec<TapeFrame>,
}

impl Tape {
    pub fn read(path: &Path) -> Result<Tape, String> {
        let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let end = bytes.windows(2).position(|w| w == b"\n\n").ok_or("not an audio tape: no header")?;
        let header_text = String::from_utf8_lossy(&bytes[..end]);
        let header: Vec<(String, String)> = header_text.lines().filter_map(|l| l.split_once('=')).map(|(k, v)| (k.to_string(), v.to_string())).collect();
        if header.first().map(|h| h.0.as_str()) != Some("audiotape") {
            return Err(format!("{}: not an audio tape", path.display()));
        }
        let mut at = end + 2;
        let mut frames = Vec::new();
        let mut events = Vec::new();
        while at < bytes.len() {
            match bytes[at] {
                1 => {
                    let raw: [u8; EVENT_SIZE] = bytes.get(at + 1..at + 1 + EVENT_SIZE).ok_or("the tape ends inside an event")?.try_into().unwrap();
                    events.push(raw);
                    at += 1 + EVENT_SIZE;
                }
                2 => {
                    let body = bytes.get(at + 1..at + 6 + STATE_SIZE).ok_or("the tape ends inside a frame")?;
                    frames.push(TapeFrame {
                        steps: u32::from_le_bytes([body[0], body[1], body[2], body[3]]),
                        state: body[4..4 + STATE_SIZE].to_vec(),
                        tc_in_action: body[4 + STATE_SIZE] != 0,
                        events: std::mem::take(&mut events),
                    });
                    at += 6 + STATE_SIZE;
                }
                other => return Err(format!("{}: unknown record {other} at {at}", path.display())),
            }
        }
        Ok(Tape { header, frames })
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.header.iter().find(|h| h.0 == key).map(|h| h.1.as_str())
    }
}

/// The three listeners the oracle drives are run with. A camera looks along minus its row 2;
/// the car's row 2 points forwards.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum OracleCamera {
    /// At the driver's head, the cockpit camera's mode.
    Cockpit,
    /// Behind and above the car, the chase camera's mode.
    Chase,
    /// A fixed point beside where the drive starts, the track camera's mode.
    Trackside,
}

impl OracleCamera {
    pub fn parse(text: &str) -> Result<OracleCamera, String> {
        match text {
            "cockpit" => Ok(OracleCamera::Cockpit),
            "chase" => Ok(OracleCamera::Chase),
            "trackside" => Ok(OracleCamera::Trackside),
            _ => Err(format!("unknown camera {text:?} (cockpit, chase, trackside)")),
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            OracleCamera::Cockpit => "cockpit",
            OracleCamera::Chase => "chase",
            OracleCamera::Trackside => "trackside",
        }
    }

    /// What the camera manager says for this camera.
    pub fn view(&self) -> crate::car::CameraView {
        match self {
            OracleCamera::Cockpit => crate::car::CameraView::COCKPIT,
            OracleCamera::Chase => crate::car::CameraView::CHASE,
            OracleCamera::Trackside => crate::car::CameraView::TRACK,
        }
    }

    /// The listener's matrix and velocity for a frame (`first` is the drive's first frame).
    pub fn listener(&self, frame: &crate::car::CarFrame, first: &crate::car::CarFrame) -> (crate::engine::Mat, crate::engine::Vec3) {
        let car = &frame.world_matrix;
        let mut m = *car;
        for c in 0..3 {
            m[c] = -car[c];
            m[8 + c] = -car[8 + c];
        }
        match self {
            OracleCamera::Cockpit => {
                for c in 0..3 {
                    m[12 + c] = car[12 + c] + car[4 + c] * 0.9;
                }
                (m, frame.velocity)
            }
            OracleCamera::Chase => {
                for c in 0..3 {
                    m[12 + c] = car[12 + c] + car[4 + c] * 1.8 - car[8 + c] * 6.0;
                }
                (m, frame.velocity)
            }
            OracleCamera::Trackside => {
                let f = &first.world_matrix;
                ([1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, f[12] + 8.0, f[13] + 2.0, f[14] + 8.0, 1.0], [0.0; 3])
            }
        }
    }
}
