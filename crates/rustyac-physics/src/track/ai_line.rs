// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! What an AI driver needs of a track's lines: AC's `AISplineRecorder` as far as it loads
//! (`ai/fast_lane.ai`, `ai/pit_lane.ai`, `data/ai_hints.ini`) and the rest of `AISpline`.
//!
//! A line's file is only the start. `AISpline::load` @ 0x1402a8390 works every point's
//! radius, direction, grade, length and distances to the corners out again
//! (`calculateRadius`, `initStraights`), and `AISplineRecorder::load` @ 0x1402952c0 then
//! takes every point's normal, camber and grip from the track itself with three ray casts
//! (`calculateNormals`). A line older than version 7 is rebuilt on top of that as
//! `AISplineRecorder::save` does before it writes it back as version 7: track limits from
//! ray casts, the closing gap cleaned and smoothed, a new lookup grid. The game writes that
//! file; here the rebuilt line only ever exists in memory.

// (the comparisons and the order of the operations are the machine code's)
#![allow(clippy::neg_cmp_op_on_partial_ord, clippy::assign_op_pattern, clippy::manual_range_contains, clippy::needless_range_loop, clippy::field_reassign_with_default, clippy::if_same_then_else, clippy::too_many_arguments)]

use std::path::Path;

use rustyac_content::vfs::{self, PathExt};

use super::spline::{distance, is_zero, len_sq, locate_on_spline, normalized, sub, AiSpline, AiSplinePayload, InterpolatingSpline, P3};
use super::Track;
use crate::data::ini::IniReader;
use crate::math::{acosf, atanf, sinf, sqrtf};
use crate::vecmath::Vec3f;

/// `AISplineSlimPayload`
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AiSlimPayload {
    pub camber: f32,
    pub grip: f32,
    pub grade: f32,
    pub is_pitlane: bool,
}

/// `AIStraightData`
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct AiStraightData {
    pub start: f32,
    pub end: f32,
    pub length: f32,
}

/// `AISplineHint`
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AiSplineHint {
    pub start_pos: f32,
    pub end_pos: f32,
    pub value: f32,
}

/// `AISplineDanger`
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AiSplineDanger {
    pub start_pos: f32,
    pub end_pos: f32,
    pub left: f32,
    pub right: f32,
}

/// The game's `3.14159f` (0x40490fd0): not pi.
const PI_ISH: f32 = f32::from_bits(0x4049_0fd0);

/// A ray straight down from 2 m (or 20 m) above a point, 100 m long: what the line code
/// asks the track.
fn ray_down(track: &Track, x: f32, y: f32, z: f32) -> Option<super::TrackRayHit> {
    track.ray_cast_hit(&Vec3f::new(x, y, z), &Vec3f::new(0.0, -1.0, 0.0), 100.0)
}

/// `(fwd.y*0 - fwd.z, fwd.z*0 - fwd.x*0, fwd.x - fwd.y*0)`, unit length: the line's right.
fn right_of(fwd: &P3) -> P3 {
    let ryy = fwd[1] * 0.0;
    let rx = ryy - fwd[2];
    let ry = fwd[2] * 0.0 - fwd[0] * 0.0;
    let rz = fwd[0] - ryy;
    let n = sqrtf((ry * ry + rx * rx) + rz * rz);
    if !is_zero(n) {
        let inv = 1.0 / n;
        [rx * inv, ry * inv, rz * inv]
    } else {
        [rx, ry, rz]
    }
}

impl AiSpline {
    fn tag_at(&self, i: i32) -> i32 {
        self.spline.points.get(i as u32 as usize).map_or(0, |p| p.tag)
    }

    /// `AISpline::payloadAtSplineIndex` @ 0x1402a95b0
    pub fn payload_at_spline_index(&self, i: i32) -> AiSplinePayload {
        let tag = self.tag_at(self.spline.wrap_index(i));
        if tag >= 0 && (tag as usize) < self.payloads.len() {
            self.payloads[tag as usize]
        } else {
            AiSplinePayload::default()
        }
    }

    /// `AISpline::setPayloadAtSplineIndex` @ 0x1402a9d70
    fn set_payload_at_spline_index(&mut self, i: i32, payload: AiSplinePayload) {
        let tag = self.tag_at(self.spline.wrap_index(i));
        if let Some(slot) = usize::try_from(tag).ok().and_then(|tag| self.payloads.get_mut(tag)) {
            *slot = payload;
        }
    }

    /// `AISpline::getPayloadIndices` @ 0x1402a7430: the two payloads around a position and
    /// how far between them it is.
    pub fn get_payload_indices(&self, pos: f32) -> (usize, usize, f32) {
        if self.spline.points.is_empty() || self.payloads.is_empty() {
            return (0, 0, 0.0);
        }
        let w = self.spline.wrap_position(pos);
        let mut frac = 0.0f32;
        let idx = self.spline.get_last_index_from_norm(w, &mut frac);
        let tag = self.tag_at(idx);
        let f = (((tag.wrapping_add(1)) as f32 - tag as f32) * frac) + tag as f32;
        // modf in double precision
        let double = f as f64;
        let whole = double.trunc();
        let t = (if double.is_infinite() { 0.0 } else { double - whole }) as f32;
        let a = if whole.is_nan() || whole >= 2_147_483_648.0 || whole < -2_147_483_648.0 { i32::MIN } else { whole as i32 };
        let count = self.payloads.len() as u64;
        let ia = (a as i64 as u64) % count;
        let ib = ((ia.wrapping_add(1)) as i32 as i64 as u64) % count;
        (ia as usize, ib as usize, t)
    }

    /// `AISpline::getSidesAtPos(pos, out, true)` @ 0x1402a7a20: the room to the track's two
    /// edges at a position.
    pub fn get_sides_at_pos(&self, pos: f32) -> [f32; 2] {
        if self.payloads.is_empty() {
            return [0.0; 2];
        }
        let (ia, ib, t) = self.get_payload_indices(pos);
        let (a, b) = (&self.payloads[ia].sides, &self.payloads[ib].sides);
        [(b[0] - a[0]) * t + a[0], (b[1] - a[1]) * t + a[1]]
    }

    /// `AISpline::getSlimPayloadAt` @ 0x1402a7af0
    pub fn get_slim_payload_at(&self, pos: f32, out: &mut AiSlimPayload) {
        if self.spline.points.is_empty() || self.payloads.is_empty() {
            return;
        }
        let (ia, ib, t) = self.get_payload_indices(pos);
        let (a, b) = (&self.payloads[ia], &self.payloads[ib]);
        out.camber = (b.camber - a.camber) * t + a.camber;
        out.grade = (b.grade - a.grade) * t + a.grade;
        out.grip = (b.grip - a.grip) * t + a.grip;
        out.is_pitlane = a.is_pitlane || b.is_pitlane;
    }

    /// `AISpline::getPointWithOffset(pos, offset, margin)` @ 0x1402a7560: the point `offset`
    /// metres to the right of the line, kept `margin` inside the track's edges.
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    pub fn get_point_with_offset_at(&self, pos: f32, mut offset: f32, margin: f32) -> P3 {
        if self.spline.points.is_empty() || self.payloads.is_empty() {
            return [0.0; 3];
        }
        let (ia, ib, t) = self.get_payload_indices(pos);
        let (a, b) = (&self.payloads[ia], &self.payloads[ib]);
        let s0 = (b.sides[0] - a.sides[0]) * t + a.sides[0];
        let s1 = (b.sides[1] - a.sides[1]) * t + a.sides[1];
        let (fa, fb) = (&a.forward_vector, &b.forward_vector);
        let mut fwd = [fa[0] + (fb[0] - fa[0]) * t, fa[1] + (fb[1] - fa[1]) * t, fa[2] + (fb[2] - fa[2]) * t];
        let n = sqrtf(len_sq(&fwd));
        if !is_zero(n) {
            let inv = 1.0 / n;
            fwd = [fwd[0] * inv, fwd[1] * inv, fwd[2] * inv];
        }
        let w = self.spline.spline_to_world(pos);
        if s1 + s0 > 4.0 && margin > 0.0 {
            let hi = s1 - margin;
            let lo = margin - s0;
            if offset > hi {
                offset = hi;
            } else if !(offset >= lo) {
                offset = lo;
            }
        }
        let r = right_of(&fwd);
        [r[0] * offset + w[0], r[1] * offset + w[1], r[2] * offset + w[2]]
    }

    /// `AISpline::getPointWithOffset(pos, offset, margin, payload)` @ 0x1402a7900: the same
    /// with a payload the caller has already.
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    pub fn get_point_with_offset(&self, pos: f32, mut offset: f32, margin: f32, payload: &AiSplinePayload) -> P3 {
        if self.spline.points.is_empty() {
            return [0.0; 3];
        }
        let w = self.spline.spline_to_world(pos);
        let (p0, p1) = (payload.sides[0], payload.sides[1]);
        if p0 + p1 > 4.0 {
            let hi = p1 - margin;
            let lo = margin - p0;
            if offset > hi {
                offset = hi;
            } else if !(offset >= lo) {
                offset = lo;
            }
        }
        let r = right_of(&payload.forward_vector);
        [r[0] * offset + w[0], r[1] * offset + w[1], r[2] * offset + w[2]]
    }

    /// The radius of the circle through three points of the (offset) line, `d` apart along
    /// it: the body of `getCornerRadiusAt` and of the corner searches.
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    fn radius_around(&self, pos: f32, d: f32, offset: f32, margin: f32) -> f32 {
        let p0 = self.get_point_with_offset_at(self.spline.wrap_position(pos - d), offset, margin);
        let p1 = self.get_point_with_offset_at(pos, offset, margin);
        let p2 = self.get_point_with_offset_at(self.spline.wrap_position(d + pos), offset, margin);
        let (cy, cx, cz) = (p2[1] - p0[1], p2[0] - p0[0], p2[2] - p0[2]);
        let sum = (cy * cy + cx * cx) + cz * cz;
        let chord = if is_zero(sum) { 0.0 } else { sqrtf(sum) };
        let flat = |x: f32, z: f32| -> (f32, f32, f32) {
            let n = sqrtf(z * z + x * x);
            if !is_zero(n) {
                let inv = 1.0 / n;
                (x * inv, z * inv, inv * 0.0)
            } else {
                (x, z, 0.0)
            }
        };
        let (ax, az, ay) = flat(p1[0] - p0[0], p1[2] - p0[2]);
        let (bx, bz, by) = flat(p2[0] - p1[0], p2[2] - p1[2]);
        let mut dot = (by * ay + bx * ax) + bz * az;
        if dot > 1.0 {
            return 10000.0;
        }
        if dot >= 0.0 {
            if dot >= 1.0 {
                return 10000.0;
            }
        } else {
            dot = 0.0;
        }
        chord / (sinf(PI_ISH - acosf(dot)) * 2.0)
    }

    /// `AISpline::getCornerRadiusAt` @ 0x1402a6bb0
    pub fn get_corner_radius_at(&self, pos: f32, dist: f32, offset: f32, margin: f32) -> f32 {
        if self.spline.points.is_empty() {
            return 10000.0;
        }
        self.radius_around(pos, dist / self.length(), offset, margin)
    }

    /// `AISpline::getDistToCornerFwd` @ 0x1402a6df0 / `getDistToCornerRev` @ 0x1402a70f0:
    /// metres along the line to the first place tighter than `threshold` (500 at most).
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    fn get_dist_to_corner(&self, threshold: f32, mut pos: f32, step: f32, forward: bool) -> f32 {
        let mut dist = 0.0f32;
        let length = self.length();
        let sn = step / length;
        let d = 20.0 / length;
        loop {
            let r = self.radius_around(pos, d, 0.0, 0.6);
            if !(r > threshold) {
                return dist;
            }
            dist += step;
            pos = if forward { self.spline.wrap_position(sn + pos) } else { self.spline.wrap_position(pos - sn) };
            if dist >= 500.0 {
                return 500.0;
            }
        }
    }

    /// `AISpline::calculateRadius` @ 0x1402a6420: radius, direction, grade, compression,
    /// length and the distances to the corners of every point but the last, from the points
    /// `p` before and after it.
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    pub fn calculate_radius(&mut self, p: i32) {
        let count = self.spline.points.len();
        if count < 2 {
            return;
        }
        let mut prev_grade = 0.0f32;
        for i in 0..count as i32 - 1 {
            let a = self.spline.point_at(self.spline.wrap_index(i - p) as u32);
            let b = self.spline.point_at(self.spline.wrap_index(i) as u32);
            let c = self.spline.point_at(self.spline.wrap_index(i + p) as u32);
            let (ax, ay, az) = (a[0] as f64, a[1] as f64, a[2] as f64);
            let (bx, by, bz) = (b[0] as f64, b[1] as f64, b[2] as f64);
            let (cx, cz) = (c[0] as f64, c[2] as f64);
            let (dx0, dz0) = (bx - ax, bz - az);
            let (dx1, dz1) = (cx - bx, cz - bz);
            let s1 = dz1 * dz1 + dx1 * dx1;
            let l = (if s1 == 0.0 { 0.0 } else { s1.sqrt() }) as f32;
            let unit = |dx: f64, dz: f64, s: f64| -> (f64, f64, f64) {
                let q = s.sqrt();
                if q != 0.0 && !q.is_nan() {
                    let inv = 1.0 / q;
                    (dx * inv, dz * inv, inv * 0.0)
                } else {
                    (dx, dz, 0.0)
                }
            };
            let (x1, z1, y1) = unit(dx1, dz1, s1);
            let s0 = dz0 * dz0 + dx0 * dx0;
            let (x0, z0, y0) = unit(dx0, dz0, s0);
            let dot = (x1 * x0 + y1 * y0) + z1 * z0;
            let mut angle = crate::math::asin(dot);
            let mut c_angle = if dot > -1.0 && dot < 1.0 { crate::math::acos(dot) } else { 0.0 };
            if dot.abs() >= 1.0 {
                angle = 1.0;
                c_angle = 0.0;
            }
            let mut payload = self.payload_at_spline_index(i);
            payload.direction = if x1 * z0 - z1 * x0 > 0.0 { 1.0 } else { -1.0 };
            let t = crate::math::tan(angle);
            let mut radius = 99999.0f32;
            if c_angle != 0.0 && !c_angle.is_nan() && !(dot >= 1.0) {
                let u = t * l as f64;
                radius = sqrtf((u * u) as f32 + l * l);
            }
            payload.radius = radius;
            let dy = (by - ay) as f32;
            let hl = (if s0 == 0.0 { 0.0 } else { s0.sqrt() }) as f32;
            if !is_zero(hl) {
                let inv = 1.0 / hl;
                let g = inv * dy;
                payload.grade = g;
                payload.compression = (g - prev_grade) * inv;
            }
            prev_grade = payload.grade;
            payload.length = (if s0 == 0.0 { 0.0 } else { s0.sqrt() }) as f32;
            let d = if s0 == 0.0 { 0.0 } else { s0.sqrt() };
            if d == 0.0 || !(d >= 0.5) {
                payload.radius = 9000.0;
            }
            let f = self.spline.get_normalized_position(i as u32);
            let fwd = self.get_dist_to_corner(250.0, f, 1.0, true);
            let rev = self.get_dist_to_corner(250.0, f, 1.0, false);
            payload.dist_from_next_corner = fwd;
            payload.dist_from_corner = if fwd > rev { rev } else { fwd };
            self.set_payload_at_spline_index(i, payload);
        }
    }

    /// `AISpline::calculateNormals` @ 0x1402a5e10: every point's normal, camber and grip from
    /// the track under it. An error where the game would crash (no track 0.6 m beside the
    /// line).
    pub fn calculate_normals(&mut self, track: &Track) -> Result<(), String> {
        let count = self.spline.points.len();
        if count < 2 {
            return Ok(());
        }
        let mut prev = self.spline.point_at(count as u32 - 1);
        for i in 0..count as i32 {
            let cur = self.spline.point_at(i as u32);
            let d = normalized(sub(&cur, &prev));
            let ryy = d[1] * 0.0;
            let mut rz = d[0] - ryy;
            let mut rx = ryy - d[2];
            let nn = sqrtf(rz * rz + rx * rx);
            let mut ry = 0.0f32;
            if !is_zero(nn) {
                let inv = 1.0 / nn;
                rx *= inv;
                ry = inv * 0.0;
                rz *= inv;
            }
            let next = self.spline.point_at(self.spline.wrap_index(i + 1) as u32);
            let Some(ray1) = ray_down(track, cur[0], cur[1] + 2.0, cur[2]) else { continue };
            let mut payload = self.payload_at_spline_index(i);
            let cr = (cur[0] - prev[0]) * (next[2] - cur[2]) - (next[0] - cur[0]) * (cur[2] - prev[2]);
            let sg = if cr > 0.0 {
                1.0f32
            } else if cr >= 0.0 {
                0.0
            } else {
                -1.0
            };
            let (ox, oy, oz) = ((rx * sg) * 0.6, (sg * ry) * 0.6, (rz * sg) * 0.6);
            let missing = || format!("the AI line's point {i} has no track 0.6 m beside it (the game crashes on such a line)");
            let ray2 = ray_down(track, cur[0] + ox, (cur[1] + 2.0) + oy, cur[2] + oz).ok_or_else(missing)?;
            let ray3 = ray_down(track, cur[0] - ox, (cur[1] + 2.0) - oy, cur[2] - oz).ok_or_else(missing)?;
            let third = f32::from_bits(0x3fd5_5555);
            let a = atanf((ray1.contact.pos[1] - ray2.contact.pos[1]) * third);
            let b = atanf((ray3.contact.pos[1] - ray1.contact.pos[1]) * third);
            payload.camber = b * 0.7 + a * 0.3;
            let (g2, g3) = (ray2.surface_def.grip_mod, ray3.surface_def.grip_mod);
            payload.grip = (g3 - g2) * 0.9 + g2;
            payload.normal = ray1.contact.normal;
            self.set_payload_at_spline_index(i, payload);
            prev = cur;
        }
        Ok(())
    }

    /// `AISpline::getStraightDataFromIndex` @ 0x1402a7cb0
    fn get_straight_data_from_index(&self, index: i32, out: &mut AiStraightData, threshold: f32) -> bool {
        let mut acc = 0.0f32;
        for k in 0..self.spline.points.len() as i32 {
            let j = self.spline.wrap_index(index + k);
            let tag = self.tag_at(self.spline.wrap_index(j));
            let (radius, length) = match usize::try_from(tag).ok().and_then(|tag| self.payloads.get(tag)) {
                Some(payload) => (payload.radius, payload.length),
                None => (0.0, 0.0),
            };
            if radius <= threshold {
                out.length = acc;
                out.end = self.spline.get_normalized_position(j as u32);
                return true;
            }
            acc += length;
        }
        false
    }

    /// `AISpline::initStraights` @ 0x1402a8130: the straights, the longest first.
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    pub fn init_straights(&mut self, threshold: f32) {
        let count = self.spline.points.len() as i32;
        if count == 0 {
            return;
        }
        // (`comiss threshold, radius ; setb`: true for a NaN)
        let mut prev_above = !(threshold >= self.payload_at_spline_index(count - 1).radius);
        let mut local = AiStraightData { start: self.spline.get_normalized_position(count as u32 - 1), end: 0.0, length: 0.0 };
        let mut i = count - 1;
        while i >= 1 {
            let r = self.payload_at_spline_index(i).radius;
            let above = r > threshold;
            if prev_above {
                if !above {
                    if self.get_straight_data_from_index(self.spline.wrap_index(i + 1), &mut local, threshold) {
                        self.straights.push(local);
                    }
                } else {
                    local.start = self.spline.get_normalized_position(i as u32);
                }
            }
            prev_above = above;
            i -= 1;
        }
        crate::msvc_sort::sort(&mut self.straights, &|a: &AiStraightData, b: &AiStraightData| a.length > b.length);
    }

    /// `AISpline::isInStraight` @ 0x1402a82f0: which straight (by length, 0 the longest) a
    /// position is on, or -1.
    pub fn is_in_straight(&self, pos: f32) -> i32 {
        for (i, s) in self.straights.iter().enumerate() {
            let inside = if s.end <= s.start { (s.start <= pos && pos <= 1.0) || pos <= s.end } else { s.start <= pos && pos <= s.end };
            if inside {
                return i as i32;
            }
        }
        -1
    }

    /// `AISpline::initPitlane` @ 0x1402a7f70: the pit lane line's grip and pit-lane flag
    /// from the surface under each point.
    pub fn init_pitlane(&mut self, track: &Track) {
        for i in 0..self.spline.points.len() as i32 {
            let p = self.spline.point_at(i as u32);
            if let Some(hit) = ray_down(track, p[0], p[1] + 2.0, p[2]) {
                let mut payload = self.payload_at_spline_index(i);
                payload.grip = hit.surface_def.grip_mod;
                payload.is_pitlane = hit.surface_def.is_pitlane;
                self.set_payload_at_spline_index(i, payload);
            }
        }
    }

    /// `AISpline::buildSides` @ 0x1402a5630: the room to the track's edges of every point
    /// but the first, by walking outwards in 5 cm steps until the surface is no track.
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    fn build_sides(&mut self, track: &Track) {
        let count = self.spline.points.len();
        if count < 2 {
            return;
        }
        for i in 1..count {
            let prev = self.spline.points[i - 1].point;
            let cur = self.spline.points[i].point;
            let mut d = sub(&cur, &prev);
            let n = sqrtf((d[0] * d[0] + d[1] * d[1]) + d[2] * d[2]);
            if !is_zero(n) {
                let inv = 1.0 / n;
                d = [d[0] * inv, d[1] * inv, d[2] * inv];
            }
            let mut rz = d[0] - d[1] * 0.0;
            let mut rx = d[1] * 0.0 - d[2];
            let nn = sqrtf(rx * rx + rz * rz);
            let mut ry = 0.0f32;
            if !is_zero(nn) {
                let inv = 1.0 / nn;
                rx *= inv;
                ry = inv * 0.0;
                rz *= inv;
            }
            if i >= self.payloads.len() {
                continue;
            }
            for side in 0..2 {
                let sign = if side == 0 { -1.0f32 } else { 1.0 };
                let floor = if side == 0 { 1.0f32 } else { 0.2 };
                let cast = |s: f32| ray_down(track, cur[0] + sign * (rx * s), (cur[1] + sign * (ry * s)) + 20.0, cur[2] + sign * (rz * s));
                let mut s = 0.05f32;
                let mut hit = cast(s);
                if hit.is_none() {
                    self.payloads[i].sides[side] = 4.0;
                    continue;
                }
                loop {
                    let surface = hit.as_ref().unwrap().surface_def;
                    if !surface.is_valid_track {
                        self.payloads[i].sides[side] = if floor <= s { s } else { floor };
                        break;
                    }
                    if s > 100.0 {
                        break;
                    }
                    s += 0.05;
                    hit = cast(s);
                    if hit.is_none() {
                        self.payloads[i].sides[side] = if s <= 0.1 { 4.0 } else { s };
                        break;
                    }
                }
            }
        }
    }

    /// `AISpline::cleanSpline` @ 0x1402a6a20: points are taken off the end while the gap to
    /// the first point is under 2 m.
    fn clean_spline(&mut self) {
        loop {
            let count = self.spline.points.len();
            if count == 0 {
                break;
            }
            let last = self.spline.points[count - 1].point;
            let first = self.spline.points[0].point;
            let d2 = len_sq(&sub(&last, &first));
            if !(is_zero(d2) || sqrtf(d2) < 2.0) {
                break;
            }
            if count < 5 {
                break;
            }
            self.spline.points.pop();
            self.payloads.pop();
        }
        self.spline.compute_spline_length();
    }

    /// `AISpline::closeSmooth` @ 0x1402a6b80: the 25 points either side of the closing gap
    /// are laid on one Bezier curve, and the payloads around it are blended.
    fn close_smooth(&mut self) {
        self.spline.bezier_endpoint();
        self.adjust_payload_endpoint(10);
    }

    /// `AISpline::adjustPayloadEndpoint` @ 0x1402a5330
    fn adjust_payload_endpoint(&mut self, p: usize) {
        let count = self.payloads.len();
        if count < 2 * p + 1 {
            return;
        }
        for j in 0..2 * p {
            let t = j as f32 * (1.0 / (p as f32 * 2.0));
            let a = self.payloads[p];
            let b = self.payloads[count - p];
            let target = &mut self.payloads[(count - p + j) % count];
            let mix = |x: f32, y: f32| (x - y) * t + y;
            target.speed_ms = mix(a.speed_ms, b.speed_ms);
            target.radius = mix(a.radius, b.radius);
            target.sides = [mix(a.sides[0], b.sides[0]), mix(a.sides[1], b.sides[1])];
            target.camber = mix(a.camber, b.camber);
            target.direction = mix(a.direction, b.direction);
            target.normal = [mix(a.normal[0], b.normal[0]), mix(a.normal[1], b.normal[1]), mix(a.normal[2], b.normal[2])];
            target.length = mix(a.length, b.length);
            target.gas = mix(a.gas, b.gas);
            target.brake = 0.0;
        }
    }

    /// `AISpline::load` @ 0x1402a8390 after its `loadFast`: `calculateRadius(1)` and
    /// `initStraights(250)`.
    pub fn finish_load(&mut self) {
        self.calculate_radius(1);
        self.init_straights(250.0);
    }
}

impl InterpolatingSpline {
    /// `InterpolatingSpline::bezierEndopoint` @ 0x1401ef720
    fn bezier_endpoint(&mut self) {
        let count = self.points.len();
        if count < 50 {
            return;
        }
        let order: Vec<usize> = (count - 25..count).chain(0..25).collect();
        let control: Vec<P3> = order.iter().map(|&i| self.points[i].point).collect();
        let mut t = 0.0f32;
        for &index in &order {
            self.points[index].point = nth_bezier(&control, t);
            t += 1.0 / control.len() as f32;
        }
        self.compute_spline_length();
    }
}

/// `EvaluateBinomialCoefficient` @ 0x1401ef010
fn binomial(n: f64, k: f64) -> f64 {
    if k == 1.0 {
        return n;
    }
    let fact = |m: f64| -> f64 {
        if m == 0.0 || m == 1.0 {
            1.0
        } else if m == 2.0 {
            2.0
        } else {
            let mut a = m;
            let mut r = m;
            loop {
                a -= 1.0;
                r *= a;
                if !(2.0 < a) {
                    break;
                }
            }
            r
        }
    };
    fact(n) / (fact(n - k) * fact(k))
}

/// `Spline::calculateNthBezier` @ 0x1401f0700
fn nth_bezier(control: &[P3], t: f32) -> P3 {
    let count = control.len();
    let n1 = (count - 1) as f64;
    let mut out = [0.0f32; 3];
    for (k, p) in control.iter().enumerate() {
        let bin = binomial(n1, k as f64);
        let w = (bin * crate::math::pow((1.0 - t) as f64, (count - 1 - k) as f64)) * crate::math::pow(t as f64, k as f64);
        for axis in 0..3 {
            out[axis] = (p[axis] as f64 * w) as f32 + out[axis];
        }
    }
    out
}

/// AC's `AISplineRecorder` as far as it loads: the fast lane, the pit lane's line and the
/// track's hints for the drivers.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AiSplineRecorder {
    /// `bestLapSpline`: the fast lane
    pub best_lap: AiSpline,
    pub pit_lane: AiSpline,
    /// `pitLaneSplineAttachPoint`: where along the pit lane's line it meets the fast lane
    /// again (-1: nowhere)
    pub pit_lane_spline_attach_point: f32,
    pub hints: Vec<AiSplineHint>,
    pub hints_brake: Vec<AiSplineHint>,
    pub hints_danger: Vec<AiSplineDanger>,
    pub hints_max_speed: Vec<AiSplineHint>,
    /// The fast lane's file was older than version 7 and was rebuilt as the game rebuilds it
    pub rebuilt_from_version: Option<i32>,
}

fn hint_at(hints: &[AiSplineHint], pos: f32) -> Option<f32> {
    hints.iter().find(|h| h.start_pos <= pos && pos <= h.end_pos).map(|h| h.value)
}

impl AiSplineRecorder {
    /// `getBestLapSpline`
    pub fn best_lap(&self) -> &AiSpline {
        &self.best_lap
    }

    /// `getPitLaneSpline`
    pub fn pit_lane(&self) -> &AiSpline {
        &self.pit_lane
    }

    /// `AISplineRecorder::getHintAtSplinePos` @ 0x140295230
    pub fn get_hint_at_spline_pos(&self, pos: f32) -> f32 {
        hint_at(&self.hints, pos).unwrap_or(1.0)
    }

    /// `AISplineRecorder::getBrakeHintAtSplinePos` @ 0x1402951b0
    pub fn get_brake_hint_at_spline_pos(&self, pos: f32) -> f32 {
        hint_at(&self.hints_brake, pos).unwrap_or(1.0)
    }

    /// `AISplineRecorder::getMaxSpeedHintAtSplinePos` @ 0x140295270
    pub fn get_max_speed_hint_at_spline_pos(&self, pos: f32) -> f32 {
        hint_at(&self.hints_max_speed, pos).unwrap_or(-1.0)
    }

    /// `AISplineRecorder::getDangerAtSplinePos` @ 0x1402951f0: (left, right)
    pub fn get_danger_at_spline_pos(&self, pos: f32) -> Option<(f32, f32)> {
        self.hints_danger.iter().find(|h| h.start_pos <= pos && pos <= h.end_pos).map(|h| (h.left, h.right))
    }

    /// `AISplineRecorder::AISplineRecorder` @ 0x140293bf0 for a loaded track: `load`
    /// @ 0x1402952c0 (the fast lane with everything worked out again, a line older than
    /// version 7 rebuilt in memory, the pit lane's line) and `loadHints` @ 0x140295670.
    /// `ai` is the layout's `ai` folder, `data` its `data` folder.
    pub fn load(track: &Track, ai: &Path, data: &Path) -> Result<AiSplineRecorder, String> {
        let mut recorder = AiSplineRecorder { pit_lane_spline_attach_point: -1.0, ..AiSplineRecorder::default() };
        let fast_lane = ai.join("fast_lane.ai");
        if fast_lane.vfs_is_file() {
            let bytes = vfs::read(&fast_lane).map_err(|e| format!("{}: {e}", fast_lane.display()))?;
            let mut line = AiSpline::parse_fast(&bytes).map_err(|e| format!("{}: {e}", fast_lane.display()))?;
            line.finish_load();
            if line.version < 7 {
                // AISplineRecorder::save(true) @ 0x140296ae0, without the writing
                recorder.rebuilt_from_version = Some(line.version);
                if line.point_count() != 0 {
                    line.calculate_radius(5);
                    line.lap_time = 0;
                    line.build_sides(track);
                    line.clean_spline();
                    if line.spline.is_closed() {
                        line.close_smooth();
                    }
                    line.spline.compute_spline_length();
                    line.calculate_normals(track)?;
                    line.spline.build_grid();
                    line.grid_missing = false;
                    line.grid_built = true;
                }
            }
            line.calculate_normals(track)?;
            recorder.best_lap = line;
        }
        // AISplineRecorder::loadPitLaneSpline @ 0x140296200
        let pit_lane = ai.join("pit_lane.ai");
        if pit_lane.vfs_is_file() {
            let bytes = vfs::read(&pit_lane).map_err(|e| format!("{}: {e}", pit_lane.display()))?;
            let mut line = AiSpline::parse_fast(&bytes).map_err(|e| format!("{}: {e}", pit_lane.display()))?;
            line.finish_load();
            recorder.pit_lane = line;
        }
        let count = recorder.pit_lane.point_count();
        let mut seen_far = false;
        if recorder.best_lap.point_count() != 0 {
            for i in 0..count {
                let pt = recorder.pit_lane.spline.points[i].point;
                let mut index = -1i32;
                let n = locate_on_spline(&recorder.best_lap, &pt, &mut index, None);
                let w = recorder.best_lap.spline.spline_to_world(n);
                let dist = distance(&pt, &w);
                if seen_far {
                    if dist <= 1.0 && (count >> 1) < i {
                        recorder.pit_lane_spline_attach_point = i as f32 / count as f32;
                        break;
                    }
                } else if dist > 1.0 {
                    seen_far = true;
                }
            }
        }
        recorder.pit_lane.init_pitlane(track);

        // AISplineRecorder::loadHints
        let hints_path = data.join("ai_hints.ini");
        if hints_path.vfs_is_file() {
            let ini = IniReader::load(&hints_path)?;
            if ini.ready {
                let family = |name: &str| -> Result<Vec<AiSplineHint>, String> {
                    let mut out = Vec::new();
                    for n in 0.. {
                        let section = format!("{name}_{n}");
                        if !ini.has_section(&section) {
                            break;
                        }
                        out.push(AiSplineHint { start_pos: ini.get_float(&section, "START")?, end_pos: ini.get_float(&section, "END")?, value: ini.get_float(&section, "VALUE")? });
                    }
                    Ok(out)
                };
                recorder.hints = family("HINT")?;
                recorder.hints_brake = family("BRAKEHINT")?;
                recorder.hints_max_speed = family("MAXSPEED")?;
                for n in 0.. {
                    let section = format!("DANGER_{n}");
                    if !ini.has_section(&section) {
                        break;
                    }
                    recorder.hints_danger.push(AiSplineDanger {
                        start_pos: ini.get_float(&section, "START")?,
                        end_pos: ini.get_float(&section, "END")?,
                        left: ini.get_float(&section, "LEFT")?,
                        right: ini.get_float(&section, "RIGHT")?,
                    });
                }
            }
        }
        Ok(recorder)
    }
}
