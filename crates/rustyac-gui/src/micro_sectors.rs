// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! `MicroSectors` (0x90 bytes, the game object "MICRO_SECTORS" of `Sim`): every car's lap in 24
//! sectors, each sector of the running lap slower than the car's best (grey), a new best of
//! the car (green) or the best of all (purple); the strip under "Performance Delta" and under
//! the overlay leaderboard's cells.
//!
//! Constructor 0x14009e9d0, `initialize` 0x14009fb40, `update` 0x14009fe20,
//! `getSectorsPerformance` 0x14009fab0, `drawSectors` 0x14009f290; `CarAvatar::isInPit`
//! 0x1400d8bd0.

use std::collections::BTreeMap;

use crate::painter::{GlId, Painter};

/// `SectorData`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SectorData {
    pub best_personal: f32,
    pub total_time_here: f32,
    /// 0 not done, 1 slower than the car's best, 3 the car's new best
    pub performance: i32,
}

impl Default for SectorData {
    fn default() -> SectorData {
        SectorData { best_personal: f32::MAX, total_time_here: 0.0, performance: 0 }
    }
}

/// `CarPerformance`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CarPerformance {
    pub started: bool,
    pub sector_started: i32,
    pub sectors: Vec<SectorData>,
}

/// `BestSector`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BestSector {
    pub best: f32,
    pub id_car: i32,
}

/// What `update` reads of a car.
#[derive(Clone, Copy, Debug, Default)]
pub struct MsCar {
    /// `CarAvatar::guid`
    pub guid: i32,
    /// `CarAvatar::isInPit`
    pub in_pit: bool,
    /// `physicsState.normalizedSplinePosition`, `lapTime` (ms)
    pub npos: f32,
    pub lap_time: u32,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct MicroSectors {
    pub cars: BTreeMap<i32, CarPerformance>,
    pub best_sectors: Vec<BestSector>,
    pub enabled: bool,
}

/// `cvttss2si` into 32 bits: NaN and what does not fit is `i32::MIN`.
#[allow(clippy::manual_range_contains)]
fn cvtt(x: f32) -> i32 {
    if x.is_nan() || x >= 2_147_483_648.0 || x < -2_147_483_648.0 {
        i32::MIN
    } else {
        x as i32
    }
}

/// `CarAvatar::isInPit` 0x1400d8bd0: within 1.5 m of the pit box (`pos`: the car's, `pit`: the
/// box's place; NaN counts as in the pit).
pub fn is_in_pit(pos: [f32; 3], pit: [f32; 3]) -> bool {
    let dx = pos[0] - pit[0];
    let dy = pos[1] - pit[1];
    let dz = pos[2] - pit[2];
    let s = (dy * dy + dx * dx) + dz * dz;
    let d = if s != 0.0 { s.sqrt() } else { 0.0 };
    !(d >= 1.5)
}

impl MicroSectors {
    /// `MicroSectors::MicroSectors` 0x14009e9d0: `show_sectors` is Documents
    /// `cfg/gameplay.ini [GUI] SHOW_SECTORS` (none: the key is not there).
    pub fn new(show_sectors: Option<i32>) -> MicroSectors {
        let enabled = show_sectors.is_some_and(|v| v != 0);
        MicroSectors { cars: BTreeMap::new(), best_sectors: if enabled { vec![BestSector { best: f32::MAX, id_car: -1 }; 24] } else { Vec::new() }, enabled }
    }

    /// `MicroSectors::initialize` 0x14009fb40 (`Sim::evOnNewSession`).
    pub fn initialize(&mut self) {
        self.cars.clear();
        for b in &mut self.best_sectors {
            *b = BestSector { best: f32::MAX, id_car: -1 };
        }
    }

    /// `MicroSectors::update` 0x14009fe20, once a frame. A car whose spline position is out
    /// of 0..1 (whose sector the game would look up out of its vectors) is left out.
    pub fn update(&mut self, time_to_session_start: f64, cars: &[MsCar]) {
        if !self.enabled {
            return;
        }
        if time_to_session_start > 0.0 {
            return;
        }
        for c in cars {
            if c.in_pit {
                continue;
            }
            let mut x = c.npos * 24.0;
            let t = cvtt(x);
            if t != i32::MIN && t as f32 != x {
                x = (t - x.is_sign_negative() as i32) as f32;
            }
            let s = cvtt(x);
            let k = if s == 0 { 23 } else { s.wrapping_sub(1) };
            if !(0..24).contains(&k) {
                continue;
            }
            let ku = k as usize;
            let Some(p) = self.cars.get_mut(&c.guid) else {
                let mut sector_started = k + 1;
                if sector_started > 24 {
                    sector_started = 0;
                }
                self.cars.insert(c.guid, CarPerformance { started: false, sector_started, sectors: vec![SectorData::default(); 24] });
                continue;
            };
            let mut skip_reset = false;
            if !p.started && (k > p.sector_started || ((k as f32) < 12.0 && (p.sector_started as f32) > 12.0)) {
                p.started = true;
                skip_reset = true;
            }
            if !skip_reset && k == 0 && p.sectors[12].performance != 0 {
                for j in 0..24 {
                    p.sectors[j].performance = 0;
                    if self.best_sectors[j].id_car == c.guid {
                        self.best_sectors[j].id_car = -1;
                    }
                }
            }
            if p.sectors[ku].performance == 0 && p.started {
                let mut tcur = c.lap_time as f32;
                if k > 0 {
                    tcur -= p.sectors[ku - 1].total_time_here;
                }
                if tcur >= p.sectors[ku].best_personal {
                    p.sectors[ku].performance = 1;
                } else {
                    p.sectors[ku].best_personal = tcur;
                    p.sectors[ku].performance = 3;
                    if tcur < self.best_sectors[ku].best {
                        self.best_sectors[ku] = BestSector { best: tcur, id_car: c.guid };
                    }
                }
                p.sectors[ku].total_time_here = c.lap_time as f32;
            }
        }
    }

    /// `MicroSectors::getSectorsPerformance` 0x14009fab0.
    pub fn get_sectors_performance(&self, guid: i32) -> Vec<SectorData> {
        self.cars.get(&guid).map(|p| p.sectors.clone()).unwrap_or_default()
    }

    /// `MicroSectors::drawSectors` 0x14009f290.
    #[allow(clippy::too_many_arguments)]
    pub fn draw_sectors(&self, painter: &mut dyn Painter, gl: GlId, guid: i32, x: f32, y: f32, w: f32, h: f32, color: [f32; 4]) {
        if !self.enabled {
            return;
        }
        let mut xoff = 1.0f32;
        let cell = (w - 1.0) * f32::from_bits(0x3d1d_89d9);
        let mut row = 0i32;
        painter.gl_color4f(gl, color[0], color[1], color[2], color[3]);
        painter.gl_quad(gl, x, y + 1.0, w, h - 1.0, false);
        let h = h + -5.0;
        let sectors = self.get_sectors_performance(guid);
        let cm1 = cell - 1.0;
        let y3 = y + 3.0;
        for (i, s) in sectors.iter().enumerate() {
            let mut st = s.performance;
            if self.best_sectors.get(i).is_some_and(|b| b.id_car == guid) {
                st = 2;
            }
            match st {
                0 => painter.gl_color4f(gl, f32::from_bits(0x3dcc_cccd), f32::from_bits(0x3dcc_cccd), f32::from_bits(0x3dcc_cccd), 0.0),
                1 => painter.gl_color4f(gl, f32::from_bits(0x3f19_999a), f32::from_bits(0x3f19_999a), f32::from_bits(0x3f19_999a), f32::from_bits(0x3f33_3333)),
                2 => painter.gl_color4f(gl, f32::from_bits(0x3f4c_cccd), 0.0, f32::from_bits(0x3f4c_cccd), f32::from_bits(0x3f33_3333)),
                3 => painter.gl_color4f(gl, 0.0, f32::from_bits(0x3f4c_cccd), 0.0, f32::from_bits(0x3f33_3333)),
                _ => {}
            }
            if row as f32 >= 8.0 {
                xoff += cell;
                row = 0;
            }
            row += 1;
            let qx = (i as f32 * cell) + (xoff + x);
            painter.gl_quad(gl, qx, y3, cm1, h, false);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_lap_in_sectors() {
        let mut m = MicroSectors::new(Some(1));
        let car = |npos: f32, t: u32| MsCar { guid: 0, in_pit: false, npos, lap_time: t };
        // first seen in sector 2: its partial sector is not counted
        m.update(0.0, &[car(0.09, 9000)]);
        for k in 4..24 {
            m.update(0.0, &[car(k as f32 / 24.0 + 0.01, k * 1000)]);
        }
        let s = m.get_sectors_performance(0);
        // (the first counted sector's time runs from the lap's start: no sector before it)
        assert_eq!(s[3].best_personal, 4000.0);
        assert_eq!(s[4].performance, 3);
        assert_eq!(s[4].best_personal, 1000.0);
        assert_eq!(m.best_sectors[4], BestSector { best: 1000.0, id_car: 0 });
        assert!(is_in_pit([1.0, 0.0, 0.0], [0.0, 0.0, 0.0]));
        assert!(!is_in_pit([2.0, 0.0, 0.0], [0.0, 0.0, 0.0]));
    }
}
