// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! The AI line of a track (`ai/fast_lane.ai`) and a car's position along it.
//!
//! AC's `kml.lib` splines as far as a car on a track uses them: `Spline`,
//! `InterpolatingSpline` (Catmull-Rom, the lookup grid), `AISpline` with its per-point
//! payload, and `SplineLocator`, which every physics step turns the car body's position into
//! the "normalised spline position" (0..1 along the lap) that the telemetry, the sector
//! table and the DRS zones read.
//!
//! The position is not a projection onto the line: it is a ten-step probe search within the
//! segment before or after the car's current spline point (`worldToSpline`), which only ever
//! moves forward one point per step. All of that is the game's and is kept.
//!
//! Not ported (the car does not read what they compute): `AISpline::calculateRadius`,
//! `initStraights`, `calculateNormals` (three rays per point at load, for the AI), the pit
//! lane's attach point, `ai_hints.ini`, version 6 files and `buildGrid` for a file that
//! stores no grid (the search then goes through all points, which finds the same nearest
//! point except where the game's grid would not hold it).

// the comparisons, their operand order and the tests for a number outside a range are the
// game's machine code (a NaN takes the same branch as there); clippy's rewrites would change that
#![allow(clippy::neg_cmp_op_on_partial_ord, clippy::double_comparisons, clippy::manual_range_contains, clippy::assign_op_pattern)]

use std::path::Path;

use super::Track;
use crate::data::ini::IniReader;
use crate::math::sqrtf;
use crate::vecmath::Vec3f;

type P3 = [f32; 3];

/// `cvttss2si` with a 64-bit destination: the "integer indefinite" for what does not fit.
fn truncate_i64(x: f32) -> i64 {
    if x.is_nan() || x >= 9.223_372e18 || x < -9.223_372e18 {
        i64::MIN
    } else {
        x as i64
    }
}

/// `cvttss2si` with a 32-bit destination.
fn truncate_i32(x: f32) -> i32 {
    if x.is_nan() || x >= 2_147_483_648.0 || x < -2_147_483_648.0 {
        i32::MIN
    } else {
        x as i32
    }
}

/// The squared length the game computes nearly everywhere: `(y*y + x*x) + z*z`.
#[inline]
fn len_sq(d: &P3) -> f32 {
    (d[1] * d[1] + d[0] * d[0]) + d[2] * d[2]
}

#[inline]
fn sub(a: &P3, b: &P3) -> P3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

/// `ucomiss x, 0 ; je`: zero, or not a number.
#[inline]
fn is_zero(x: f32) -> bool {
    !(x < 0.0 || x > 0.0)
}

/// A distance that tests for zero before the square root.
#[inline]
fn distance(a: &P3, b: &P3) -> f32 {
    let q = len_sq(&sub(a, b));
    if is_zero(q) {
        0.0
    } else {
        sqrtf(q)
    }
}

/// Divides a vector by its length unless that is zero (the square root is taken first).
fn normalized(mut d: P3) -> P3 {
    let l = sqrtf(len_sq(&d));
    if !is_zero(l) {
        let inv = 1.0 / l;
        d = [d[0] * inv, d[1] * inv, d[2] * inv];
    }
    d
}

/// `SplinePoint` (20 bytes).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SplinePoint {
    pub point: P3,
    /// `pointLength`: metres along the line from point 0.
    pub point_length: f32,
    /// `tag`: the index of the point's payload.
    pub tag: i32,
}

/// `GridData`: the lookup grid's place and cell size.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GridData {
    pub max_extreme: P3,
    pub min_extreme: P3,
    /// `samplingDensity`: the cells' edge, m.
    pub sampling_density: f32,
    pub neighbors_considered_number: u32,
}

/// AC's `InterpolatingSpline` in Catmull-Rom mode, on top of `Spline`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct InterpolatingSpline {
    pub points: Vec<SplinePoint>,
    /// `m_length`
    pub m_length: f32,
    /// `Spline::isClosed` (the game works it out again on every call; the points never change).
    closed: bool,
    pub grid_data: Option<GridData>,
    /// `grid`: for every X cell the run of its Z cells in `grid_cells`.
    grid_columns: Vec<(u32, u32)>,
    /// For every cell the run of its candidates in `grid_indices`.
    grid_cells: Vec<(u32, u32)>,
    grid_indices: Vec<u32>,
}

/// `AISplinePayload` (84 bytes): what the line knows at one of its points.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AiSplinePayload {
    pub speed_ms: f32,
    pub radius: f32,
    /// `sides`: the distance from the line to the track's two edges, m.
    pub sides: [f32; 2],
    pub camber: f32,
    pub direction: f32,
    pub normal: P3,
    /// `forwardVector`: from the point before to this one, unit length.
    pub forward_vector: P3,
    pub length: f32,
    pub gas: f32,
    pub brake: f32,
    pub grade: f32,
    pub grip: f32,
    pub dist_from_corner: f32,
    pub dist_from_next_corner: f32,
    pub is_pitlane: bool,
    pub compression: f32,
}

impl Default for AiSplinePayload {
    /// `AISplinePayload::AISplinePayload`: zeros, but direction and grip 1.
    fn default() -> AiSplinePayload {
        AiSplinePayload {
            speed_ms: 0.0,
            radius: 0.0,
            sides: [0.0; 2],
            camber: 0.0,
            direction: 1.0,
            normal: [0.0; 3],
            forward_vector: [0.0; 3],
            length: 0.0,
            gas: 0.0,
            brake: 0.0,
            grade: 0.0,
            grip: 1.0,
            dist_from_corner: 0.0,
            dist_from_next_corner: 0.0,
            is_pitlane: false,
            compression: 0.0,
        }
    }
}

/// AC's `AISpline` (0xe0 bytes).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AiSpline {
    /// `lapTime`
    pub lap_time: u32,
    /// `version`
    pub version: i32,
    pub spline: InterpolatingSpline,
    pub payloads: Vec<AiSplinePayload>,
    /// The file stored no grid and none could be built (fewer points than a cell holds): the
    /// nearest point is searched among all points.
    pub grid_missing: bool,
    /// The file stored no grid: it was built as the game builds it at load.
    pub grid_built: bool,
}

/// The Catmull-Rom weights of a parameter, as the game evaluates them.
#[inline]
fn catmull_coefficients(t: f32) -> [f32; 4] {
    let t2 = t * t;
    let t3 = t * (t * t);
    [((t2 * 2.0) - t3) - t, ((t3 * 3.0) - (t2 * 5.0)) + 2.0, ((t2 * 4.0) - (t3 * 3.0)) + t, t3 - t2]
}

/// `H3*c3 + ((H0*c0 + H1*c1) + H2*c2)` per component, with `Hk = Pk * 0.5`.
#[inline]
fn catmull_point(h: &[P3; 4], c: &[f32; 4]) -> P3 {
    let mut out = [0.0f32; 3];
    for k in 0..3 {
        out[k] = h[3][k] * c[3] + ((h[0][k] * c[0] + h[1][k] * c[1]) + h[2][k] * c[2]);
    }
    out
}

#[inline]
fn half(p: &P3) -> P3 {
    [p[0] * 0.5, p[1] * 0.5, p[2] * 0.5]
}

impl InterpolatingSpline {
    /// `Spline::pointsCount` @ 0x1401ee880
    pub fn points_count(&self) -> usize {
        self.points.len()
    }

    /// `Spline::length` @ 0x1401ee3a0
    pub fn length(&self) -> f32 {
        if self.points.is_empty() {
            0.0
        } else {
            self.m_length
        }
    }

    /// `Spline::pointAt` @ 0x1401ee820: zeros for an index past the end.
    pub fn point_at(&self, i: u32) -> P3 {
        self.points.get(i as usize).map(|p| p.point).unwrap_or([0.0; 3])
    }

    /// `Spline::isClosed` @ 0x1401ee310: the ends are no more than 75 m apart.
    pub fn is_closed(&self) -> bool {
        self.closed
    }

    fn compute_is_closed(&self) -> bool {
        let count = self.points.len();
        if count < 2 {
            return false;
        }
        let q = len_sq(&sub(&self.points[count - 1].point, &self.points[0].point));
        if is_zero(q) {
            return true;
        }
        !(sqrtf(q) > 75.0)
    }

    /// `Spline::boundInsideSpline` @ 0x1401ed860 (open splines)
    fn bound_inside_spline(&self, v: f32) -> f32 {
        if !(v >= 0.0) {
            return 0.0;
        }
        // (count - 1) as an unsigned 64-bit number: 2^64 - 1 for an empty spline
        let m = (self.points.len() as u64).wrapping_sub(1) as f32;
        if v > m {
            m
        } else {
            v
        }
    }

    /// `Spline::wrapIndex` @ 0x1401eead0
    pub fn wrap_index(&self, i: i32) -> i32 {
        if !self.closed {
            return truncate_i32(self.bound_inside_spline(i as f32));
        }
        let count = self.points.len() as i32;
        if i < 0 {
            count.wrapping_add(i)
        } else if i < count {
            i
        } else {
            (i as u32).wrapping_sub(count as u32) as i32
        }
    }

    /// `Spline::closestPointIndex` @ 0x1401ed900: through every point; the first of equal
    /// distances wins.
    fn closest_point_index_brute(&self, pos: &P3) -> u32 {
        if self.points.is_empty() || !pos.iter().all(|c| c.is_finite()) {
            return 0;
        }
        let mut best = 9_999_999.0f32;
        let mut best_index = 0u32;
        for (i, p) in self.points.iter().enumerate() {
            let q = len_sq(&sub(pos, &p.point));
            if !(q >= best) {
                best = q;
                best_index = i as u32;
            }
        }
        best_index
    }

    /// `Spline::closestPointIndexWithBounds` @ 0x1401eda10: only among the points of some
    /// index ranges.
    fn closest_point_index_with_bounds(&self, pos: &P3, bounds: &[(u32, u32)]) -> u32 {
        if self.points.is_empty() || !pos.iter().all(|c| c.is_finite()) {
            return 0;
        }
        let mut best = 9_999_999.0f32;
        let mut best_index = 0u32;
        for (i, p) in self.points.iter().enumerate() {
            let i = i as u32;
            if bounds.iter().any(|&(min, max)| !(i < min) && !(i > max)) {
                let d = sub(pos, &p.point);
                // the x term first here
                let q = (d[0] * d[0] + d[1] * d[1]) + d[2] * d[2];
                if !(q >= best) {
                    best = q;
                    best_index = i;
                }
            }
        }
        best_index
    }

    /// `InterpolatingSpline::closestPointIndex` @ 0x1401f0d80 (`closestPointIndexGrid` @
    /// 0x1401f0da0): the nearest of the candidates of the grid cell the position is in.
    pub fn closest_point_index(&self, pos: &P3) -> u32 {
        if self.points.is_empty() {
            return 0;
        }
        let Some(grid) = &self.grid_data else { return self.closest_point_index_brute(pos) };
        let inv = 1.0 / grid.sampling_density;
        let ix = truncate_i64((pos[0] - grid.min_extreme[0]) * inv) as u32;
        let iz = truncate_i64((pos[2] - grid.min_extreme[2]) * inv) as u32;
        let Some(&(first, rows)) = self.grid_columns.get(ix as usize) else { return self.closest_point_index_brute(pos) };
        if iz >= rows {
            return self.closest_point_index_brute(pos);
        }
        let (start, len) = self.grid_cells[(first + iz) as usize];
        let last = (self.points.len() - 1) as u32;
        let mut best = 9_999_999.0f32;
        let mut best_index = 0u32;
        for &candidate in &self.grid_indices[start as usize..(start + len) as usize] {
            let j = candidate.min(last);
            let q = len_sq(&sub(pos, &self.points[j as usize].point));
            if !(q >= best) {
                best = q;
                best_index = j;
            }
        }
        best_index
    }

    /// `InterpolatingSpline::getNormalizedPosition` @ 0x1401f2d80
    pub fn get_normalized_position(&self, i: u32) -> f32 {
        if self.points.is_empty() {
            return 0.0;
        }
        let length = self.length();
        if is_zero(length) {
            return 0.0;
        }
        self.points.get(i as usize).map(|p| p.point_length).unwrap_or(0.0) / length
    }

    /// `InterpolatingSpline::getLastIndexFromNorm` @ 0x1401f2b90: the point at or before a
    /// position, and how far into its segment the position is (not written at the end of an
    /// open spline).
    pub fn get_last_index_from_norm(&self, n: f32, frac: &mut f32) -> i32 {
        let count = self.points.len();
        if count == 0 {
            return 0;
        }
        let target = self.length() * n;
        let mut idx = truncate_i64(((count as u64 - 1) as f32) * n) as u32;
        // the game's loop has no end for two neighbouring points of the same length at the
        // target; a line like that is cut short here instead of hanging the physics
        let mut turns = 0usize;
        loop {
            turns += 1;
            if idx as u64 >= count as u64 - 1 || turns > 2 * count + 8 {
                break;
            }
            let a = self.points[idx as usize].point_length;
            if a > target {
                idx = idx.wrapping_sub(1);
                continue;
            }
            let next = self.points[idx as usize + 1].point_length;
            if !(target >= next) {
                *frac = (target - a) / (next - a);
                return idx as i32;
            }
            if a >= target {
                idx = idx.wrapping_sub(1);
                continue;
            }
            idx += 1;
        }
        if !self.closed {
            return idx as i32;
        }
        let d = sub(&self.points[0].point, &self.points[count - 1].point);
        // the straight gap between the ends, x term first
        let q = (d[0] * d[0] + d[1] * d[1]) + d[2] * d[2];
        let gap = if is_zero(q) { 0.0 } else { sqrtf(q) };
        *frac = (target - self.points[count - 1].point_length) / gap;
        idx as i32
    }

    /// `InterpolatingSpline::calculateCatmullRom` @ 0x1401efde0
    fn calculate_catmull_rom(&self, n: f32) -> P3 {
        let count = self.points.len();
        if count < 4 {
            return [0.0; 3];
        }
        let mut unused = 0.0;
        let i = self.get_last_index_from_norm(n, &mut unused);
        let i0 = self.wrap_index(i.wrapping_sub(1));
        let i2 = self.wrap_index(i.wrapping_add(1));
        let i3 = self.wrap_index(i.wrapping_add(2));
        let na = self.get_normalized_position(i as u32);
        let nb = self.get_normalized_position(i2 as u32);
        let mut seg = nb - na;
        if seg >= 0.0 {
            if seg <= 0.0 {
                return self.points[count - 1].point;
            }
        } else {
            // the segment from the last point back to the first
            seg = (nb + 1.0) - na;
            if !(seg > 0.0) {
                return self.points[count - 1].point;
            }
        }
        let t = (n - na) / seg;
        let c = catmull_coefficients(t);
        let h = [half(&self.point_at(i0 as u32)), half(&self.point_at(i as u32)), half(&self.point_at(i2 as u32)), half(&self.point_at(i3 as u32))];
        catmull_point(&h, &c)
    }

    /// `InterpolatingSpline::splineToWorld` @ 0x1401f3a20: the point of the curve at a
    /// normalised position.
    pub fn spline_to_world(&self, n: f32) -> P3 {
        let mut n = n;
        if n > 1.0 {
            n = 1.0;
        } else if !(n >= 0.0) {
            n = 0.0;
            if let Some(first) = self.points.first() {
                return first.point;
            }
        } else if n == 0.0 {
            if let Some(first) = self.points.first() {
                return first.point;
            }
        }
        self.calculate_catmull_rom(n)
    }

    /// `InterpolatingSpline::wrapPosition` @ 0x1401f41e0
    pub fn wrap_position(&self, n: f32) -> f32 {
        if !self.closed {
            return self.bound_inside_spline(n);
        }
        let mut n = n;
        if !(n >= 0.0) {
            n += 1.0;
        }
        if n > 1.0 {
            n += -1.0;
        }
        n
    }

    /// `InterpolatingSpline::worldToSpline` @ 0x1401f3bf0: the normalised position of the
    /// curve's point nearest to `pos`, looked for in the segment before or after the point
    /// `index` (-1: the nearest point is found first).
    pub fn world_to_spline(&self, pos: &P3, index: i32) -> f32 {
        let mut index = index;
        if index == -1 {
            index = if self.points.is_empty() { 0 } else { self.closest_point_index(pos) as i32 };
        }
        let n_prev = self.get_normalized_position(self.wrap_index(index.wrapping_sub(1)) as u32);
        let n_cur = self.get_normalized_position(self.wrap_index(index) as u32);
        let n_next = self.get_normalized_position(self.wrap_index(index.wrapping_add(1)) as u32);
        let d = |n: f32| distance(&self.spline_to_world(n), pos);
        let d_prev = d(n_prev);
        let d_next = d(n_next);
        let mut half = 0.5f32;
        let mut quarter = 0.25f32;
        let range;
        if d_prev >= d_next {
            // forward
            let mut r = n_next - n_cur;
            if !(r >= 0.0) {
                r += 1.0;
            }
            range = r;
            for _ in 0..10 {
                let lo = half - quarter;
                let da = d(self.wrap_position((lo * range) + n_cur));
                half = quarter + half;
                let db = d(self.wrap_position((half * range) + n_cur));
                if !(da >= db) {
                    half = lo;
                }
                quarter *= 0.5;
            }
        } else {
            // backward
            let mut r = n_prev - n_cur;
            if r > 0.0 {
                r = -(1.0 - r);
            }
            range = r;
            for _ in 0..10 {
                let hi = quarter + half;
                let da = d(self.wrap_position((hi * range) + n_cur));
                half -= quarter;
                let db = d(self.wrap_position((half * range) + n_cur));
                if !(da >= db) {
                    half = hi;
                }
                quarter *= 0.5;
            }
        }
        self.wrap_position((range * half) + n_cur)
    }

    /// `InterpolatingSpline::getSignedDistanceFromSpline` @ 0x1401f2df0: how far `pos` is
    /// beside the curve at `n`; positive towards the edge `sides[0]`.
    pub fn get_signed_distance_from_spline(&self, pos: &P3, n: f32) -> f32 {
        let a = self.spline_to_world(n);
        let n2 = self.wrap_position((0.5 / self.length()) + n);
        let b = self.spline_to_world(n2);
        let dir = normalized(sub(&b, &a));
        let rel = sub(pos, &a);
        let u = normalized(rel);
        let cx = (u[2] * dir[1]) - (u[1] * dir[2]);
        let cy = (u[0] * dir[2]) - (u[2] * dir[0]);
        let cz = (u[1] * dir[0]) - (u[0] * dir[1]);
        let dot = ((cx * 0.0) + cy) + (cz * 0.0);
        let q = len_sq(&rel);
        let m = if is_zero(q) { 0.0 } else { sqrtf(q) };
        m * dot
    }

    /// `InterpolatingSpline::computeSplineLength` @ 0x1401f1db0: every point's distance from
    /// the start and the whole length, by walking the curve in steps of a thousandth of a
    /// segment.
    fn compute_spline_length(&mut self) {
        self.closed = self.compute_is_closed();
        let count = self.points.len();
        if count == 0 {
            return;
        }
        const STEP: f32 = 0.001;
        let mut total = 0.0f32;
        for i in 1..count {
            let i = i as i32;
            let h = [
                half(&self.point_at(self.wrap_index(i - 2) as u32)),
                half(&self.point_at((i - 1) as u32)),
                half(&self.point_at(self.wrap_index(i) as u32)),
                half(&self.point_at(self.wrap_index(i + 1) as u32)),
            ];
            let mut prev = self.points[i as usize - 1].point;
            let mut seg = 0.0f32;
            let mut t = 0.0f32;
            loop {
                let p = catmull_point(&h, &catmull_coefficients(t));
                let dist = distance(&p, &prev);
                t += STEP;
                seg += dist;
                prev = p;
                if !(t <= 1.0) {
                    break;
                }
            }
            total = seg + total;
            let i2 = self.wrap_index(i) as usize;
            self.points[i2].point_length = total;
        }
        if self.closed {
            let last = count as i32 - 1;
            let h = [
                half(&self.point_at(self.wrap_index(last - 1) as u32)),
                half(&self.point_at(last as u32)),
                half(&self.point_at(self.wrap_index(last + 1) as u32)),
                half(&self.point_at(self.wrap_index(last + 2) as u32)),
            ];
            let mut prev = self.points[last as usize].point;
            let mut t = 0.0f32;
            loop {
                let p = catmull_point(&h, &catmull_coefficients(t));
                let dist = distance(&p, &prev);
                t += STEP;
                // sample by sample straight into the total
                total += dist;
                prev = p;
                if !(t <= 1.0) {
                    break;
                }
            }
        }
        self.m_length = total;
    }
}

/// `Spline::ComparablePoint` (8 bytes): what `closestPointIndicesFlat` sorts.
#[derive(Clone, Copy, Debug, PartialEq)]
struct ComparablePoint {
    index: u32,
    distance: f32,
}

/// Visual Studio 2013's `std::sort` (`std::_Sort` @ 0x1401ecaa0 with `_Unguarded_partition` @
/// 0x1401ecc70, `_Median` @ 0x1401ec810, `_Insertion_sort1` @ 0x1401ec6a0) over points by
/// distance. The sort is not stable, points at the same distance are common, and the order
/// it leaves them in decides which ten points a grid cell holds and which of two equally
/// near ones a search finds first: so it is this algorithm, move for move.
mod msvc_sort {
    use super::ComparablePoint;

    #[inline]
    fn lt(a: &ComparablePoint, b: &ComparablePoint) -> bool {
        a.distance < b.distance
    }

    fn med3(a: &mut [ComparablePoint], i: usize, j: usize, k: usize) {
        if lt(&a[j], &a[i]) {
            a.swap(j, i);
        }
        if lt(&a[k], &a[j]) {
            a.swap(k, j);
            if lt(&a[j], &a[i]) {
                a.swap(j, i);
            }
        }
    }

    /// `last` is the last element itself here.
    fn median(a: &mut [ComparablePoint], first: usize, mid: usize, last: usize) {
        if 40 < last - first {
            let step = (last - first + 1) / 8;
            med3(a, first, first + step, first + 2 * step);
            med3(a, mid - step, mid, mid + step);
            med3(a, last - 2 * step, last - step, last);
            med3(a, first + step, mid, last - step);
        } else {
            med3(a, first, mid, last);
        }
    }

    fn partition(a: &mut [ComparablePoint], first: usize, last: usize) -> (usize, usize) {
        let mid = first + (last - first) / 2;
        median(a, first, mid, last - 1);
        let mut pfirst = mid;
        let mut plast = pfirst + 1;
        while first < pfirst && !lt(&a[pfirst - 1], &a[pfirst]) && !lt(&a[pfirst], &a[pfirst - 1]) {
            pfirst -= 1;
        }
        while plast < last && !lt(&a[plast], &a[pfirst]) && !lt(&a[pfirst], &a[plast]) {
            plast += 1;
        }
        let mut gfirst = plast;
        let mut glast = pfirst;
        loop {
            while gfirst < last {
                if lt(&a[pfirst], &a[gfirst]) {
                } else if lt(&a[gfirst], &a[pfirst]) {
                    break;
                } else {
                    if plast != gfirst {
                        a.swap(plast, gfirst);
                    }
                    plast += 1;
                }
                gfirst += 1;
            }
            while first < glast {
                if lt(&a[glast - 1], &a[pfirst]) {
                } else if lt(&a[pfirst], &a[glast - 1]) {
                    break;
                } else {
                    pfirst -= 1;
                    if pfirst != glast - 1 {
                        a.swap(pfirst, glast - 1);
                    }
                }
                glast -= 1;
            }
            if glast == first && gfirst == last {
                return (pfirst, plast);
            }
            if glast == first {
                if plast != gfirst {
                    a.swap(pfirst, plast);
                }
                plast += 1;
                a.swap(pfirst, gfirst);
                pfirst += 1;
                gfirst += 1;
            } else if gfirst == last {
                glast -= 1;
                pfirst -= 1;
                if glast != pfirst {
                    a.swap(glast, pfirst);
                }
                plast -= 1;
                a.swap(pfirst, plast);
            } else {
                glast -= 1;
                a.swap(gfirst, glast);
                gfirst += 1;
            }
        }
    }

    fn insertion(a: &mut [ComparablePoint], first: usize, last: usize) {
        for next in first + 1..last {
            let val = a[next];
            if lt(&val, &a[first]) {
                a.copy_within(first..next, first + 1);
                a[first] = val;
            } else {
                let mut j = next;
                while lt(&val, &a[j - 1]) {
                    a[j] = a[j - 1];
                    j -= 1;
                }
                a[j] = val;
            }
        }
    }

    // The heap sort the algorithm falls back on when its quick sort degenerates. Not read in
    // the binary (no installed line reaches it): the same compiler's library header.
    fn push_heap(a: &mut [ComparablePoint], first: usize, mut hole: usize, top: usize, val: ComparablePoint) {
        while top < hole {
            let idx = (hole - 1) / 2;
            if !lt(&a[first + idx], &val) {
                break;
            }
            a[first + hole] = a[first + idx];
            hole = idx;
        }
        a[first + hole] = val;
    }

    fn adjust_heap(a: &mut [ComparablePoint], first: usize, mut hole: usize, bottom: usize, val: ComparablePoint) {
        let top = hole;
        let mut idx = 2 * hole + 2;
        while idx < bottom {
            if lt(&a[first + idx], &a[first + idx - 1]) {
                idx -= 1;
            }
            a[first + hole] = a[first + idx];
            hole = idx;
            idx = 2 * idx + 2;
        }
        if idx == bottom {
            a[first + hole] = a[first + bottom - 1];
            hole = bottom - 1;
        }
        push_heap(a, first, hole, top, val);
    }

    fn heap_sort(a: &mut [ComparablePoint], first: usize, mut last: usize) {
        let count = last - first;
        let mut hole = count / 2;
        while 0 < hole {
            hole -= 1;
            let val = a[first + hole];
            adjust_heap(a, first, hole, count, val);
        }
        while 1 < last - first {
            let val = a[last - 1];
            a[last - 1] = a[first];
            adjust_heap(a, first, 0, last - 1 - first, val);
            last -= 1;
        }
    }

    fn sort_range(a: &mut [ComparablePoint], mut first: usize, mut last: usize, mut ideal: isize) {
        let mut count;
        loop {
            count = last - first;
            if !(32 < count && 0 < ideal) {
                break;
            }
            let (pf, pl) = partition(a, first, last);
            ideal /= 2;
            ideal += ideal / 2;
            if pf - first < last - pl {
                sort_range(a, first, pf, ideal);
                first = pl;
            } else {
                sort_range(a, pl, last, ideal);
                last = pf;
            }
        }
        if 32 < count {
            heap_sort(a, first, last);
        } else if 1 < count {
            insertion(a, first, last);
        }
    }

    pub fn sort(a: &mut [ComparablePoint]) {
        let n = a.len();
        sort_range(a, 0, n, n as isize);
    }
}

impl InterpolatingSpline {
    /// `Spline::closestPointIndicesFlat` @ 0x1401edb00: the `n` points nearest to `pos` seen
    /// from above (x and z only), nearest first in the order the game's sort leaves them.
    /// (With fewer than `n` points the game reads past its array; here the list is shorter.)
    fn closest_point_indices_flat(&self, pos: &P3, n: u32, scratch: &mut Vec<ComparablePoint>) -> Vec<u32> {
        scratch.clear();
        for (i, p) in self.points.iter().enumerate() {
            let dz = pos[2] - p.point[2];
            let dx = pos[0] - p.point[0];
            let q = dx * dx + dz * dz;
            let d = if is_zero(q) { 0.0 } else { sqrtf(q) };
            scratch.push(ComparablePoint { index: i as u32, distance: d });
        }
        msvc_sort::sort(scratch);
        scratch.iter().take(n as usize).map(|c| c.index).collect()
    }

    /// `InterpolatingSpline::buildGrid` @ 0x1401ef980: what the game does at load for a line
    /// whose file stores no lookup grid (`loadGrid` @ 0x1401f31a0 with a flag of 0). Cells of
    /// 10 m over the line's extent plus 350 m on every side; each holds the ten points
    /// nearest to its middle, seen from above.
    pub fn build_grid(&mut self) {
        self.grid_columns.clear();
        self.grid_cells.clear();
        self.grid_indices.clear();
        // the largest starts from the smallest positive number, not from the most negative
        let mut g = GridData {
            max_extreme: [f32::MIN_POSITIVE, 0.0, f32::MIN_POSITIVE],
            min_extreme: [f32::MAX, 0.0, f32::MAX],
            sampling_density: 10.0,
            neighbors_considered_number: 10,
        };
        #[allow(clippy::neg_cmp_op_on_partial_ord)] // a NaN coordinate is stored as the smallest
        for p in &self.points {
            let (x, z) = (p.point[0], p.point[2]);
            if !(x >= g.min_extreme[0]) {
                g.min_extreme[0] = x;
            }
            if !(z >= g.min_extreme[2]) {
                g.min_extreme[2] = z;
            }
            if x > g.max_extreme[0] {
                g.max_extreme[0] = x;
            }
            if z > g.max_extreme[2] {
                g.max_extreme[2] = z;
            }
        }
        g.min_extreme[0] -= 350.0;
        g.min_extreme[2] -= 350.0;
        g.max_extreme[0] += 350.0;
        g.max_extreme[2] += 350.0;
        let inv = 1.0f32 / g.sampling_density;
        let nx = truncate_i64((g.max_extreme[0] - g.min_extreme[0]) * inv) as u32;
        let nz = truncate_i64((g.max_extreme[2] - g.min_extreme[2]) * inv) as u32;
        self.grid_data = Some(g);
        let mut scratch = Vec::with_capacity(self.points.len());
        for ix in 0..nx {
            self.grid_columns.push((self.grid_cells.len() as u32, nz));
            let cx = ix as f64 + 0.5;
            for iz in 0..nz {
                let d = g.sampling_density;
                let z = ((iz as f64 + 0.5) * d as f64 + g.min_extreme[2] as f64) as f32;
                let x = (d as f64 * cx + g.min_extreme[0] as f64) as f32;
                let indices = self.closest_point_indices_flat(&[x, 0.0, z], g.neighbors_considered_number, &mut scratch);
                self.grid_cells.push((self.grid_indices.len() as u32, indices.len() as u32));
                self.grid_indices.extend(indices);
            }
        }
    }

    /// The lookup grid as the file's layout has it: per column, per cell, the candidates.
    pub fn grid_cells_in_order(&self) -> impl Iterator<Item = &[u32]> + '_ {
        self.grid_cells.iter().map(|&(start, len)| &self.grid_indices[start as usize..(start + len) as usize])
    }

    /// Columns and, per column, cells of the lookup grid.
    pub fn grid_size(&self) -> (usize, usize) {
        (self.grid_columns.len(), self.grid_columns.first().map_or(0, |c| c.1 as usize))
    }
}

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    /// `istream::read` of four bytes: a read past the end leaves the value as it was.
    fn u32(&mut self, preset: u32) -> u32 {
        match self.bytes.get(self.at..self.at + 4) {
            Some(b) => {
                self.at += 4;
                u32::from_le_bytes([b[0], b[1], b[2], b[3]])
            }
            None => {
                self.at = self.bytes.len();
                preset
            }
        }
    }

    fn i32(&mut self, preset: i32) -> i32 {
        self.u32(preset as u32) as i32
    }

    fn f32(&mut self, preset: f32) -> f32 {
        f32::from_bits(self.u32(preset.to_bits()))
    }

    fn p3(&mut self, preset: P3) -> P3 {
        [self.f32(preset[0]), self.f32(preset[1]), self.f32(preset[2])]
    }
}

impl AiSpline {
    /// `AISpline::loadFast` @ 0x1402a83c0 for the bytes of a `.ai` file of version 7 or newer
    /// (`loadVersion7` @ 0x1402a8b80, `InterpolatingSpline::loadGrid` @ 0x1401f31a0, then
    /// `computeSplineLength`).
    pub fn parse(bytes: &[u8]) -> Result<AiSpline, String> {
        let mut r = Reader { bytes, at: 0 };
        let mut ai = AiSpline { version: r.i32(0), ..AiSpline::default() };
        if ai.version < 7 {
            // AISpline::loadVersion6 @ 0x1402a85b0 (versions 0 to 6): the count, the lap time,
            // then the points. A line with points is then rebuilt by the game in the same
            // load (AISplineRecorder::save(true) @ 0x140296ae0: new track limits from rays,
            // cleanSpline, closeSmooth, a grid) and written back as version 7, which is why
            // no installed track has an old line with points. That rebuild is not ported.
            let points_count = r.u32(0);
            ai.lap_time = r.u32(0);
            if points_count != 0 {
                return Err(format!(
                    "an AI line of version {} with {points_count} points: the game rebuilds such a line when it loads it and saves it as version 7 (drive the track once in Assetto Corsa); the rebuild is not ported",
                    ai.version
                ));
            }
            // no points (the Drift track's file): the empty line the game has without a file
            ai.spline.compute_spline_length();
            return Ok(ai);
        }
        let points_count = r.u32(0);
        ai.lap_time = r.u32(0);
        let _sample_count = r.i32(0);
        if points_count as u64 * 20 > bytes.len() as u64 {
            return Err(format!("the AI line says it has {points_count} points, more than the file holds"));
        }
        for _ in 0..points_count {
            let point = r.p3([0.0; 3]);
            let point_length = r.f32(0.0);
            let tag = r.i32(0);
            ai.spline.points.push(SplinePoint { point, point_length, tag });
        }
        let payload_count = r.u32(0);
        if payload_count as u64 * 72 > bytes.len() as u64 {
            return Err(format!("the AI line says it has {payload_count} payloads, more than the file holds"));
        }
        let mut prev = [0.0f32; 3];
        for i in 0..payload_count {
            let mut p = AiSplinePayload { speed_ms: r.f32(0.0), ..AiSplinePayload::default() };
            p.gas = r.f32(0.0);
            p.brake = r.f32(0.0);
            let _obsolete_lat_g = r.f32(0.0);
            p.radius = r.f32(0.0);
            p.sides[0] = r.f32(0.0);
            p.sides[1] = r.f32(0.0);
            p.camber = r.f32(0.0);
            p.direction = r.f32(1.0);
            p.normal = r.p3([0.0; 3]);
            p.length = r.f32(0.0);
            let _stored_forward = r.p3([0.0; 3]);
            let _unused = r.i32(0);
            p.grade = r.f32(0.0);
            // the direction from the point before
            let cur = ai.spline.point_at(i);
            p.forward_vector = normalized(sub(&cur, &prev));
            ai.payloads.push(p);
            prev = cur;
        }

        // InterpolatingSpline::loadGrid
        let has_grid = r.i32(0);
        if has_grid > 0 {
            let max_extreme = r.p3([f32::MIN_POSITIVE, 0.0, f32::MIN_POSITIVE]);
            let min_extreme = r.p3([f32::MAX, 0.0, f32::MAX]);
            let neighbors_considered_number = r.u32(10);
            let sampling_density = r.f32(10.0);
            ai.spline.grid_data = Some(GridData { max_extreme, min_extreme, sampling_density, neighbors_considered_number });
            let nx = r.i32(0).max(0);
            let left = |r: &Reader| bytes.len().saturating_sub(r.at);
            if nx as usize > left(&r) / 4 {
                return Err("the AI line's grid has more columns than the file holds".to_string());
            }
            for _ in 0..nx {
                let nz = r.i32(0).max(0) as u32;
                if nz as usize > left(&r) / 4 {
                    return Err("the AI line's grid has more rows than the file holds".to_string());
                }
                ai.spline.grid_columns.push((ai.spline.grid_cells.len() as u32, nz));
                for _ in 0..nz {
                    let n = r.i32(0).max(0) as u32;
                    if n as usize * 4 > bytes.len() - r.at.min(bytes.len()) {
                        return Err("the AI line's grid runs past the end of the file".to_string());
                    }
                    ai.spline.grid_cells.push((ai.spline.grid_indices.len() as u32, n));
                    for _ in 0..n {
                        let index = r.u32(0);
                        ai.spline.grid_indices.push(index);
                    }
                }
            }
        } else if ai.spline.points.len() >= 10 {
            // InterpolatingSpline::loadGrid: no stored grid, the game builds one
            ai.spline.build_grid();
            ai.grid_built = true;
        } else {
            // (the game would read past its array of points for a cell's ten candidates)
            ai.grid_missing = true;
        }

        if ai.payloads.len() >= 2 {
            let last = ai.spline.point_at((ai.spline.points.len() as u32).wrapping_sub(1));
            let first = ai.spline.point_at(0);
            ai.payloads[0].forward_vector = normalized(sub(&first, &last));
        }
        ai.spline.compute_spline_length();
        Ok(ai)
    }

    pub fn load(path: &Path) -> Result<AiSpline, String> {
        let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        AiSpline::parse(&bytes).map_err(|e| format!("{}: {e}", path.display()))
    }

    /// The lap's length, m.
    pub fn length(&self) -> f32 {
        self.spline.length()
    }

    pub fn point_count(&self) -> usize {
        self.spline.points.len()
    }

    /// `AISpline::payloadAtPosition` @ 0x1402a90a0: the payload between the two points around
    /// a position.
    pub fn payload_at_position(&self, n: f32) -> AiSplinePayload {
        let mut out = AiSplinePayload::default();
        if self.spline.points.is_empty() || self.payloads.is_empty() {
            return out;
        }
        let w = self.spline.wrap_position(n);
        let mut frac = 0.0f32;
        let idx = self.spline.get_last_index_from_norm(w, &mut frac);
        let tag = self.spline.points.get(idx as u32 as usize).map(|p| p.tag).unwrap_or(0);
        let f = (((tag.wrapping_add(1)) as f32 - tag as f32) * frac) + tag as f32;
        // modf in double precision
        let double = f as f64;
        let whole = double.trunc();
        let t = (if double.is_infinite() { 0.0 } else { double - whole }) as f32;
        let a = if whole.is_nan() || whole >= 2_147_483_648.0 || whole < -2_147_483_648.0 { i32::MIN } else { whole as i32 };
        let count = self.payloads.len() as u64;
        let ia = (a as i64 as u64) % count;
        let ib = ((ia.wrapping_add(1)) as i32 as i64 as u64) % count;
        let (pa, pb) = (&self.payloads[ia as usize], &self.payloads[ib as usize]);
        let lerp = |x: f32, y: f32| ((y - x) * t) + x;
        let lerp3 = |x: &P3, y: &P3| normalized([lerp(x[0], y[0]), lerp(x[1], y[1]), lerp(x[2], y[2])]);
        out.camber = lerp(pa.camber, pb.camber);
        out.direction = lerp(pa.direction, pb.direction);
        out.gas = lerp(pa.gas, pb.gas);
        out.length = lerp(pa.length, pb.length);
        out.normal = lerp3(&pa.normal, &pb.normal);
        out.radius = lerp(pa.radius, pb.radius);
        out.sides = [lerp(pa.sides[0], pb.sides[0]), lerp(pa.sides[1], pb.sides[1])];
        out.speed_ms = lerp(pa.speed_ms, pb.speed_ms);
        out.brake = lerp(pa.brake, pb.brake);
        out.forward_vector = lerp3(&pa.forward_vector, &pb.forward_vector);
        out.grade = lerp(pa.grade, pb.grade);
        out.grip = lerp(pa.grip, pb.grip);
        out.dist_from_corner = lerp(pa.dist_from_corner, pb.dist_from_corner);
        out.dist_from_next_corner = lerp(pa.dist_from_next_corner, pb.dist_from_next_corner);
        out.is_pitlane = pa.is_pitlane || pb.is_pitlane;
        out
    }
}

/// `SplineLocator::locateOnSpline` @ 0x1402ab2f0: moves the car's current point on by one if
/// the next is nearer (or finds it afresh when the car is more than 50 m from it), then the
/// position within the segments around it.
pub fn locate_on_spline(spline: &AiSpline, pos: &P3, idx: &mut i32, bounds: Option<&[(u32, u32)]>) -> f32 {
    let sp = &spline.spline;
    if *idx == -1 {
        *idx = match bounds {
            // locateOnSplineWithBounds @ 0x1402ab400
            Some(bounds) => sp.closest_point_index_with_bounds(pos, bounds) as i32,
            None => sp.closest_point_index(pos) as i32,
        };
    } else {
        let d0 = len_sq(&sub(pos, &sp.point_at(*idx as u32)));
        let next = sp.wrap_index(idx.wrapping_add(1));
        let d1 = len_sq(&sub(pos, &sp.point_at(next as u32)));
        if !(d1 >= d0) {
            *idx = next;
        }
        if d0 > 2500.0 {
            *idx = sp.closest_point_index(pos) as i32;
        }
    }
    sp.world_to_spline(pos, *idx)
}

/// AC's `SplineLocator` (`Car::splineLocator`): where the car is along the AI line.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SplineLocator {
    /// `currentIndex`: the spline point the search starts from; -1 = not known.
    pub current_index: i32,
    /// `normalizedPos`: 0..1 along the line; -1 before the first step and after a teleport.
    pub normalized_pos: f32,
    /// `offset`: metres beside the line, positive towards the edge `sides[0]`.
    pub offset: f32,
    /// `isOutsideLimits`: the whole car is beyond the track's edge.
    pub is_outside_limits: bool,
}

impl Default for SplineLocator {
    /// `SplineLocator::init` @ 0x1402ab2a0
    fn default() -> SplineLocator {
        SplineLocator { current_index: -1, normalized_pos: -1.0, offset: 0.0, is_outside_limits: false }
    }
}

/// AC's `SplineLocatorData` (`Car::splineLocatorData`): what `Car::postStep` publishes.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SplineLocatorData {
    /// `npos`: the normalised position (the shared memory's `normalizedCarPosition`).
    pub npos: f32,
    pub current_index: u32,
    /// `lateralOffset`: minus the locator's offset.
    pub lateral_offset: f32,
    pub spline_length: f32,
    /// `sides`: the car's distance to the track's two edges.
    pub sides: [f32; 2],
    pub sides_from_il: [f32; 2],
    pub side_velocity: f32,
    pub is_outside_track_limits: bool,
}

impl SplineLocator {
    /// `SplineLocator::reset` @ 0x1402ab520 (every teleport).
    pub fn reset(&mut self) {
        self.normalized_pos = -1.0;
        self.current_index = -1;
    }

    /// `SplineLocator::step` @ 0x1402ab5c0. `body_pos` is the car body's position (its centre
    /// of mass) before the step is integrated, `car_half_width` `Car::carHalfWidth`.
    pub fn step(&mut self, spline: &AiSpline, starting_bounds: &[(u32, u32)], body_pos: &Vec3f, car_half_width: f32) {
        let pos = [body_pos.x, body_pos.y, body_pos.z];
        // the first step after a teleport may only look inside the track's starting bounds
        let bounds = if 0.0 <= self.normalized_pos || self.normalized_pos.is_nan() || starting_bounds.is_empty() { None } else { Some(starting_bounds) };
        self.normalized_pos = locate_on_spline(spline, &pos, &mut self.current_index, bounds);
        self.offset = spline.spline.get_signed_distance_from_spline(&pos, self.normalized_pos);
        let payload = spline.payload_at_position(self.normalized_pos);
        let w = car_half_width * 2.0;
        self.is_outside_limits = if !((w - self.offset) >= (-payload.sides[0])) { true } else { ((-self.offset) - w) > payload.sides[1] };
    }

    /// `SplineLocator::getSides` @ 0x1402ab130: the car's distance to the two edges.
    pub fn get_sides(spline: &AiSpline, body_pos: &Vec3f, n: f32) -> [f32; 2] {
        let a = spline.spline.spline_to_world(n);
        let p = spline.payload_at_position(n);
        let d = sub(&a, &[body_pos.x, body_pos.y, body_pos.z]);
        let cross = (p.forward_vector[2] * d[0]) - (p.forward_vector[0] * d[2]);
        let q = len_sq(&d);
        let dist = if is_zero(q) { 0.0 } else { sqrtf(q) };
        let s = if cross > 0.0 {
            1.0
        } else if cross >= 0.0 {
            0.0
        } else {
            -1.0
        };
        [((s * dist) + p.sides[0]).abs(), (p.sides[1] - (s * dist)).abs()]
    }

    /// The spline part of `Car::postStep` @ 0x140275430: `body_pos` is the body's position
    /// after the step.
    pub fn post_step(&self, data: &mut SplineLocatorData, spline: &AiSpline, body_pos: &Vec3f, dt: f32) {
        data.is_outside_track_limits = self.is_outside_limits;
        data.lateral_offset = -self.offset;
        data.npos = self.normalized_pos;
        data.current_index = self.current_index as u32;
        data.spline_length = spline.spline.length();
        let sides = SplineLocator::get_sides(spline, body_pos, data.npos);
        data.side_velocity = (sides[0] - data.sides[0]) / dt;
        data.sides = sides;
        data.sides_from_il = spline.payload_at_position(self.normalized_pos).sides;
    }
}

/// `Track::initAISpline` @ 0x1402782a0 as far as a car needs it: the AI line, the pit lane's
/// line, the starting bounds.
pub fn init_ai_spline(track: &mut Track, ai: &Path, data: &Path, messages: &mut Vec<String>) -> Result<(), String> {
    let fast_lane = ai.join("fast_lane.ai");
    if fast_lane.is_file() {
        match AiSpline::load(&fast_lane) {
            Ok(spline) => {
                if spline.grid_missing {
                    messages.push(format!(
                        "{}: no lookup grid is stored and the line has fewer than ten points; every point is searched instead",
                        fast_lane.display()
                    ));
                }
                if spline.point_count() == 0 {
                    // a file without points is the game's empty line: no position along the lap
                    messages.push(format!("{}: the AI line has no points (version {}): no position along the lap", fast_lane.display(), spline.version));
                } else {
                    track.ai_spline = Some(spline);
                }
            }
            // the track still drives: without its line there is no position along the lap
            Err(e) => messages.push(format!("the AI line was not read ({e}): no position along the lap")),
        }
    } else {
        messages.push(format!("the track has no AI line ({}): no position along the lap", fast_lane.display()));
    }
    let pit_lane = ai.join("pit_lane.ai");
    if pit_lane.is_file() {
        // only shown, never searched: a missing grid does not matter
        track.pit_lane_spline = AiSpline::load(&pit_lane).ok();
    }
    // Track::initStartingBounds @ 0x140278790
    track.starting_bounds.clear();
    let bounds = IniReader::load(&data.join("starting_bounds.ini"))?;
    if bounds.ready {
        let count = track.ai_spline.as_ref().map(|s| s.point_count()).unwrap_or(0) as i32 as f32;
        for n in 0.. {
            let section = format!("BOUND_{n}");
            if !bounds.has_section(&section) {
                break;
            }
            let index = |key: &str| -> Result<u32, String> { Ok(truncate_i64(bounds.get_float(&section, key)? * count) as u32) };
            track.starting_bounds.push((index("MIN")?, index("MAX")?));
        }
    }
    Ok(())
}

impl Track {
    /// `Track::getSector` @ 0x140278220: which sector a normalised position is in.
    pub fn get_sector(&self, n: f32) -> usize {
        let s = &self.sectors_normalized_positions;
        for i in 1..s.len() {
            if n > s[i - 1] && !(n >= s[i]) {
                return i - 1;
            }
        }
        s.len().wrapping_sub(1)
    }

    /// The nearest point of the AI line to `position`, put on the road, and the tail
    /// direction of a car that drives along the line there. Built from the game's pieces
    /// (`worldToSpline`, `splineToWorld`, the payload's forward vector); the game itself has
    /// no such command.
    pub fn pose_on_ai_line(&self, position: &Vec3f) -> Option<(Vec3f, Vec3f)> {
        let spline = self.ai_spline.as_ref()?;
        if spline.spline.points.len() < 4 {
            return None;
        }
        self.pose_on_ai_line_at(spline.spline.world_to_spline(&[position.x, position.y, position.z], -1))
    }

    /// The point of the AI line at the normalised position `n`, put on the road, and the tail
    /// direction of a car that drives along the line there.
    pub fn pose_on_ai_line_at(&self, n: f32) -> Option<(Vec3f, Vec3f)> {
        use crate::tyre::RayTrackCollisionProvider;
        let spline = self.ai_spline.as_ref()?;
        if spline.spline.points.len() < 4 {
            return None;
        }
        let p = spline.spline.spline_to_world(n);
        let forward = spline.payload_at_position(n).forward_vector;
        // the line is recorded at the height of a car's body: down to the road
        let ground = self.ray_cast(&Vec3f::new(p[0], p[1] + 3.0, p[2]), &Vec3f::new(0.0, -1.0, 0.0), 20.0).map(|hit| hit.pos.y).unwrap_or(p[1]);
        Some((Vec3f::new(p[0], ground, p[2]), Vec3f::new(-forward[0], -forward[1], -forward[2])))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The grids stored in the game's own AI lines were written by the game's `buildGrid`
    /// from the very points of the file: building them again has to give the same header,
    /// the same cells and the same order inside every cell (which only the game's own sort
    /// gives: a stable sort gets about one cell in 300 wrong).
    #[test]
    fn the_lookup_grid_is_built_as_the_game_builds_it() {
        let Some(root) = rustyac_content::install::ac_root() else {
            eprintln!("NOT TESTED: Assetto Corsa's folder was not found (set AC_ROOT); the grid test needs the game's own AI lines");
            return;
        };
        let mut tested = 0;
        for track in ["magione", "monza", "ks_laguna_seca"] {
            let file = root.join("content").join("tracks").join(track).join("ai").join("fast_lane.ai");
            if !file.is_file() {
                eprintln!("NOT TESTED: {} is missing", file.display());
                continue;
            }
            let stored = AiSpline::load(&file).expect("the game's AI line");
            assert!(!stored.grid_built && !stored.grid_missing, "{track}: the file stores its grid");
            let mut built = stored.spline.clone();
            built.build_grid();
            assert_eq!(built.grid_data, stored.spline.grid_data, "{track}: the grid's header");
            assert_eq!(built.grid_size(), stored.spline.grid_size(), "{track}: the grid's size");
            let differing = built.grid_cells_in_order().zip(stored.spline.grid_cells_in_order()).filter(|(a, b)| a != b).count();
            assert_eq!(differing, 0, "{track}: cells that differ from the game's");
            assert_eq!(built.grid_indices.len(), stored.spline.grid_indices.len());
            tested += 1;
        }
        eprintln!("the lookup grid of {tested} of the game's AI lines was rebuilt cell for cell");
    }

    #[test]
    fn the_sort_orders_by_distance_and_handles_every_size() {
        // sizes around the insertion-sort limit (32) and the median-of-nine limit (40)
        for n in [0usize, 1, 2, 31, 32, 33, 40, 41, 42, 100, 1000] {
            let mut state = 12345u32;
            let mut points: Vec<ComparablePoint> = (0..n)
                .map(|i| {
                    state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                    // few different values: many ties
                    ComparablePoint { index: i as u32, distance: ((state >> 24) % 17) as f32 }
                })
                .collect();
            msvc_sort::sort(&mut points);
            assert!(points.windows(2).all(|w| w[0].distance <= w[1].distance), "{n} points are not in order");
            let mut seen: Vec<u32> = points.iter().map(|p| p.index).collect();
            seen.sort_unstable();
            assert!(seen.iter().enumerate().all(|(i, &index)| i as u32 == index), "{n} points: one was lost");
        }
    }

    #[test]
    fn an_old_line_without_points_is_an_empty_line_and_one_with_points_is_refused() {
        // the Drift track's file: version 3, no points, no lap time
        let empty = AiSpline::parse(&[3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]).unwrap();
        assert_eq!((empty.version, empty.point_count(), empty.length()), (3, 0, 0.0));
        let mut old = vec![6u8, 0, 0, 0, 2, 0, 0, 0, 0, 0, 0, 0];
        old.extend([0u8; 128]);
        assert!(AiSpline::parse(&old).unwrap_err().contains("version 6 with 2 points"));
    }

    /// A circle of radius 100 m around the origin in the x-z plane, a point every 2 degrees.
    fn circle() -> AiSpline {
        let mut bytes = Vec::new();
        let count = 180u32;
        for word in [7u32, count, 0, 0] {
            bytes.extend(word.to_le_bytes());
        }
        for i in 0..count {
            let a = (i as f32 * 2.0).to_radians();
            for value in [100.0 * a.cos(), 0.0, 100.0 * a.sin(), 0.0] {
                bytes.extend(value.to_le_bytes());
            }
            bytes.extend((i as i32).to_le_bytes());
        }
        bytes.extend(count.to_le_bytes());
        for _ in 0..count {
            let mut payload = [0.0f32; 18];
            payload[5] = 6.0; // sides[0]
            payload[6] = 4.0; // sides[1]
            for value in payload {
                bytes.extend(value.to_le_bytes());
            }
        }
        bytes.extend(0i32.to_le_bytes());
        AiSpline::parse(&bytes).unwrap()
    }

    #[test]
    fn a_closed_line_has_the_length_of_its_curve() {
        let ai = circle();
        assert!(ai.spline.is_closed());
        let length = ai.length();
        assert!((length - 628.3).abs() < 0.5, "{length}");
        // the points' lengths grow and the last is one segment short of the whole
        let lengths: Vec<f32> = ai.spline.points.iter().map(|p| p.point_length).collect();
        assert!(lengths.windows(2).all(|w| w[1] > w[0]));
        assert!((length - lengths[179] - 3.4907).abs() < 0.03, "{length} {}", lengths[179]);
        // the forward vector of the first point comes from the last
        let f = ai.payloads[0].forward_vector;
        assert!(f[2] > 0.99 && f[0].abs() < 0.05, "{f:?}");
    }

    #[test]
    fn a_position_is_found_along_and_beside_the_line() {
        let ai = circle();
        // a quarter of the way round, 2 m outside the circle
        let pos = [0.0, 0.0, 102.0];
        let n = ai.spline.world_to_spline(&pos, -1);
        assert!((n - 0.25).abs() < 0.001, "{n}");
        let offset = ai.spline.get_signed_distance_from_spline(&pos, n);
        assert!((offset.abs() - 2.0).abs() < 0.01, "{offset}");
        let on = ai.spline.spline_to_world(n);
        assert!((on[2] - 100.0).abs() < 0.01 && on[0].abs() < 0.2, "{on:?}");
        // the position wraps at the start
        assert_eq!(ai.spline.spline_to_world(0.0), ai.spline.points[0].point);
        assert_eq!(ai.spline.wrap_position(1.25), 0.25);
        assert_eq!(ai.spline.wrap_position(-0.25), 0.75);
    }

    #[test]
    fn the_locator_follows_a_car_one_point_at_a_time() {
        let ai = circle();
        let mut locator = SplineLocator::default();
        let mut last = -1.0f32;
        for step in 0..400 {
            let a = (step as f32 * 0.25).to_radians();
            let pos = Vec3f::new(99.0 * a.cos(), 0.5, 99.0 * a.sin());
            locator.step(&ai, &[], &pos, 0.9);
            assert!(locator.normalized_pos >= last - 1e-6, "step {step}: {} after {last}", locator.normalized_pos);
            last = locator.normalized_pos;
            assert!(!locator.is_outside_limits);
        }
        assert!((last - 100.0 / 360.0).abs() < 0.002, "{last}");
        // far outside the edge
        locator.step(&ai, &[], &Vec3f::new(0.0, 0.0, 120.0), 0.9);
        assert!(locator.is_outside_limits);
        let mut data = SplineLocatorData::default();
        locator.post_step(&mut data, &ai, &Vec3f::new(0.0, 0.0, 120.0), 0.003);
        assert_eq!(data.npos, locator.normalized_pos);
        assert_eq!(data.lateral_offset, -locator.offset);
        assert_eq!(data.sides_from_il, [6.0, 4.0]);
        // 20 m beyond one edge: the two distances differ by the track's width
        assert!(((data.sides[0] - data.sides[1]).abs() - 10.0).abs() < 0.05, "{:?}", data.sides);
    }
}
