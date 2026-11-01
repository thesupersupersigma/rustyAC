// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! F3, the track's TV cameras (`CameraTrack` 0x1400cab20): the camera sets of the track's
//! `data/cameras.ini`, `cameras_1.ini`, ... (`CameraTrack::loadSet` 0x1400cb670,
//! `CameraManager::load` 0x1401f48e0), the camera that films the focused car's part of the lap
//! (`CameraManager::getActiveCamera` 0x1401f4820) and how it aims, zooms and wobbles
//! (`CameraTrack::update` 0x1400cbe30). The arithmetic is in the machine's order.

// the machine's comparisons, numbers that are not numbers included
#![allow(clippy::neg_cmp_op_on_partial_ord)]

use std::path::Path;

use rustyac_physics::data::ini::IniReader;
use rustyac_physics::track::ai_line::spline_from_csv;
use rustyac_physics::track::spline::InterpolatingSpline;
use rustyac_physics::vecmath::{xm_matrix_multiply, Mat44f, Vec3f};

use super::scene::{create_target_exact, point};
use crate::view::Mat;

/// `ACCamera` (ctor 0x1401f4240): one camera of a set.
#[derive(Clone, Debug, PartialEq)]
pub struct TrackCamera {
    pub name: String,
    /// rows: side, up, minus the view direction, position
    pub matrix: Mat,
    /// +0x68 `MAX_FOV`, +0x6c `MIN_FOV`: the field of view with the car near and far
    pub max_fov: f32,
    pub min_fov: f32,
    /// `IN_POINT`, `OUT_POINT`: the part of the lap (0..1) the camera films
    pub in_point: f32,
    pub out_point: f32,
    pub shadow_splits: [f32; 3],
    pub near: f32,
    pub far: f32,
    pub min_exposure: f32,
    pub max_exposure: f32,
    pub dof_factor: f32,
    pub dof_focus: f32,
    pub dof_range: f32,
    pub dof_manual: bool,
    pub spline_file: String,
    pub spline_rotation: f32,
    /// the camera's path (`SPLINE=`, a csv file beside cameras.ini), its points from the
    /// camera's place
    pub spline: Option<InterpolatingSpline>,
    pub fov_gamma: f32,
    pub spline_animation_length: f32,
    pub is_fixed: bool,
    /// +0xac, +0xb0: the farthest and nearest AI line point between `IN_POINT` and
    /// `OUT_POINT` (`CameraManager::initCamerasDistanceRanges` 0x1401f48b0)
    pub far_distance: f32,
    pub near_distance: f32,
}

impl Default for TrackCamera {
    fn default() -> TrackCamera {
        TrackCamera {
            name: "UNNAMED".to_string(),
            matrix: crate::view::IDENTITY,
            max_fov: 30.0,
            min_fov: 30.0,
            in_point: -1.0,
            out_point: -1.0,
            shadow_splits: [1.8, 100.0, 180.0],
            near: 0.1,
            far: 35000.0,
            min_exposure: 0.0,
            max_exposure: 10000.0,
            dof_factor: 10.0,
            dof_focus: 0.0,
            dof_range: 10000.0,
            dof_manual: false,
            spline_file: String::new(),
            spline_rotation: 0.0,
            spline: None,
            fov_gamma: 0.0,
            spline_animation_length: 15.0,
            is_fixed: false,
            far_distance: 0.0,
            near_distance: 0.0,
        }
    }
}

/// `CameraManager`: one camera set (one `cameras*.ini`).
#[derive(Clone, Debug, PartialEq)]
pub struct CameraSet {
    pub name: String,
    pub cameras: Vec<TrackCamera>,
    /// `currentCamera`
    pub current: Option<usize>,
}

/// `unit()` as the loader writes it: scaled by 1 / length unless the length is 0.
fn unit(v: [f32; 3]) -> [f32; 3] {
    let length = ((v[0] * v[0] + v[1] * v[1]) + v[2] * v[2]).sqrt();
    if length == 0.0 {
        return v;
    }
    let k = 1.0 / length;
    [v[0] * k, v[1] * k, v[2] * k]
}

impl CameraSet {
    /// `CameraManager::load` 0x1401f48e0 (`append` false) of one file.
    pub fn load(path: &Path) -> Result<CameraSet, String> {
        let ini = IniReader::load(path)?;
        let mut set = CameraSet { name: String::new(), cameras: Vec::new(), current: None };
        if !ini.ready {
            return Ok(set);
        }
        let version = ini.get_int("HEADER", "VERSION")?;
        let count = ini.get_int("HEADER", "CAMERA_COUNT")?;
        set.name = ini.get_string("HEADER", "SET_NAME");
        for i in 0..count.max(0) {
            let s = format!("CAMERA_{i}");
            let mut c = TrackCamera { name: ini.get_string(&s, "NAME"), ..TrackCamera::default() };
            let position = ini.get_float3(&s, "POSITION")?;
            // heading, up, side = heading x up, up again = side x heading, side again
            let f = unit(ini.get_float3(&s, "FORWARD")?);
            let u = unit(ini.get_float3(&s, "UP")?);
            let side = unit([u[2] * f[1] - u[1] * f[2], u[0] * f[2] - u[2] * f[0], u[1] * f[0] - u[0] * f[1]]);
            let up = unit([side[1] * f[2] - side[2] * f[1], side[2] * f[0] - side[0] * f[2], side[0] * f[1] - side[1] * f[0]]);
            let side = unit([up[2] * f[1] - up[1] * f[2], up[0] * f[2] - up[2] * f[0], up[1] * f[0] - up[0] * f[1]]);
            c.matrix = [
                [side[0], side[1], side[2], 0.0],
                [up[0], up[1], up[2], 0.0],
                [-f[0], -f[1], -f[2], 0.0],
                [position[0], position[1], position[2], 1.0],
            ];
            c.min_fov = ini.get_float(&s, "MIN_FOV")?;
            c.max_fov = ini.get_float(&s, "MAX_FOV")?;
            c.in_point = ini.get_float(&s, "IN_POINT")?;
            c.out_point = ini.get_float(&s, "OUT_POINT")?;
            if version >= 2 {
                for (k, split) in c.shadow_splits.iter_mut().enumerate() {
                    *split = ini.get_float(&s, &format!("SHADOW_SPLIT{k}"))?;
                }
                c.near = ini.get_float(&s, "NEAR_PLANE")?;
                c.far = ini.get_float(&s, "FAR_PLANE")?;
                if c.near == 0.0 {
                    (c.near, c.far) = (0.1, 35000.0);
                }
            }
            if version >= 3 {
                c.min_exposure = ini.get_float(&s, "MIN_EXPOSURE")?;
                c.max_exposure = ini.get_float(&s, "MAX_EXPOSURE")?;
                c.dof_factor = ini.get_float(&s, "DOF_FACTOR")?;
                c.dof_focus = ini.get_float(&s, "DOF_FOCUS")?;
                c.dof_range = ini.get_float(&s, "DOF_RANGE")?;
                c.spline_file = ini.get_string(&s, "SPLINE");
                c.spline_rotation = ini.get_float(&s, "SPLINE_ROTATION")?;
                c.fov_gamma = ini.get_float(&s, "FOV_GAMMA")?;
                if !(c.fov_gamma > 0.0) {
                    c.fov_gamma = 1.0;
                }
                c.spline_animation_length = ini.get_float(&s, "SPLINE_ANIMATION_LENGTH")?;
                // `ucomiss; jne`: a length that is not a number is replaced too
                if c.spline_animation_length == 0.0 || c.spline_animation_length.is_nan() {
                    c.spline_animation_length = 10.0;
                }
                c.dof_manual = ini.get_int(&s, "DOF_MANUAL")? > 0;
            }
            c.is_fixed = ini.get_int(&s, "IS_FIXED")? != 0;
            set.cameras.push(c);
        }
        Ok(set)
    }

    /// `CameraManager::initCamerasDistanceRanges` 0x1401f48b0: for every camera, the farthest
    /// and the nearest point of the AI line within its part of the lap.
    pub fn distance_ranges(&mut self, line: &InterpolatingSpline) {
        let n = line.points.len() as u32;
        for c in &mut self.cameras {
            let lo = if c.in_point > c.out_point { c.out_point } else { c.in_point };
            let hi = if c.out_point > c.in_point { c.out_point } else { c.in_point };
            for i in 0..n {
                let t = i as f32 / n as f32;
                if !(t >= lo) || t > hi {
                    continue;
                }
                let p = line.points[i as usize].point;
                let (dx, dy, dz) = (c.matrix[3][0] - p[0], c.matrix[3][1] - p[1], c.matrix[3][2] - p[2]);
                let q = (dy * dy + dx * dx) + dz * dz;
                let d = if q == 0.0 { 0.0 } else { q.sqrt() };
                if c.far_distance == 0.0 || d > c.far_distance {
                    c.far_distance = d;
                }
                if c.near_distance == 0.0 || !(d >= c.near_distance) {
                    c.near_distance = d;
                }
            }
        }
    }

    /// `CameraManager::getActiveCamera` 0x1401f4820: the last camera whose part of the lap
    /// holds `position`, else the one before, else the first; and whether that is a change
    /// (`wasTriggered`).
    pub fn active(&mut self, position: f32) -> Option<(usize, bool)> {
        let mut pick = self.current;
        for (i, c) in self.cameras.iter().enumerate() {
            let lo = if c.in_point > c.out_point { c.out_point } else { c.in_point };
            if !(position >= lo) {
                continue;
            }
            let hi = if c.out_point > c.in_point { c.out_point } else { c.in_point };
            if !(position > hi) {
                pick = Some(i);
            }
        }
        if pick.is_none() && !self.cameras.is_empty() {
            pick = Some(0);
        }
        let pick = pick?;
        let triggered = self.current != Some(pick);
        self.current = Some(pick);
        Some((pick, triggered))
    }
}

/// `Spline::getPointSmooth` 0x1401ee050: the point at `t` (0..1) along a camera's path.
pub fn point_smooth(spline: &InterpolatingSpline, t: f32) -> [f32; 3] {
    let t = if t > 1.0 { 1.0 } else if t >= 0.0 { t } else { 0.0 };
    let t = t * f32::from_bits(0x3f7f_be77);
    let count = spline.points.len() as u64;
    // (count - 1) as a signed 64-bit number, as an unsigned one when below 0
    let last = count.wrapping_sub(1) as f32;
    let i = (last * t) as i32;
    let mut k = t - i as f32 / count as f32;
    k /= 1.0 / count as f32;
    let j = spline.wrap_index(i + 1);
    let (a, b) = (spline.point_at(i as u32), spline.point_at(j as u32));
    [(b[0] - a[0]) * k + a[0], (b[1] - a[1]) * k + a[1], (b[2] - a[2]) * k + a[2]]
}

/// What a track camera sets on the scene camera in a frame.
#[derive(Clone, Debug, PartialEq)]
pub struct TrackFrame {
    pub matrix: Mat,
    pub fov: f32,
    pub near: f32,
    pub far: f32,
    pub min_exposure: f32,
    pub max_exposure: f32,
    pub dof_factor: f32,
    pub dof_focus: f32,
    pub dof_range: f32,
    pub shadow_splits: [f32; 3],
    pub name: String,
}

/// `CameraTrack`: the sets, the settings of `system/cfg/camera_track.ini` and what the
/// camera keeps from frame to frame.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct TrackCameras {
    pub sets: Vec<CameraSet>,
    pub current_set: usize,
    /// `[CAMERA_SETTINGS] DISTANCE_SCALE` (as 1 / it), `UPWARD_OFFSET`,
    /// `CAMERA_WOBBLING_SPEED`, `CAMERA_WOBBLING_STRENGTH`
    pub distance_scale: f32,
    pub upward_offset: f32,
    pub wobbling_speed: f32,
    pub wobbling_strength: f32,
    /// `stillCamera`: the game holds the wobble while paused
    pub still: bool,
    pub animation_time: f32,
    pub time: f32,
    pub rot: [f32; 3],
    pub dd: f32,
    /// `oldAc`: the camera (set, camera) of the last frame
    pub old: Option<(usize, usize)>,
}

impl TrackCameras {
    /// `CameraTrack::CameraTrack` 0x1400cab20: the sets of the track (`data_folder` is
    /// `content/tracks/<track>[/<layout>]`), `cameras.ini` then `cameras_1.ini` ... up to the
    /// first missing one, with their distances to `line` (the AI line) and their paths.
    pub fn load(data_folder: &Path, line: &InterpolatingSpline, camera_track_ini: Option<&IniReader>) -> Result<TrackCameras, String> {
        let mut cameras = TrackCameras::default();
        for n in 0.. {
            let name = if n == 0 { "cameras.ini".to_string() } else { format!("cameras_{n}.ini") };
            let path = data_folder.join("data").join(name);
            if !rustyac_physics::data::exists(&path) {
                break;
            }
            let mut set = CameraSet::load(&path)?;
            set.distance_ranges(line);
            for c in &mut set.cameras {
                if c.spline_file.is_empty() {
                    continue;
                }
                let file = data_folder.join("data").join(&c.spline_file);
                // (a missing file: "[ERROR] Spline CSV file ... not found", no path)
                let Ok(bytes) = rustyac_content::vfs::read(&file) else { continue };
                let mut spline = spline_from_csv(&String::from_utf8_lossy(&bytes));
                // `ucomiss; je`: a turn that is not a number is no turn either
                if c.spline_rotation != 0.0 && !c.spline_rotation.is_nan() {
                    let m = Mat44f::create_from_axis_angle(&Vec3f::new(0.0, 1.0, 0.0), c.spline_rotation * f32::from_bits(0x3c8e_f998)).m;
                    for p in &mut spline.points {
                        p.point = point(&m, p.point);
                    }
                }
                // (the game's paths stay open: `loadFromCSV` leaves `m_closed` false)
                c.spline = Some(spline);
            }
            cameras.sets.push(set);
        }
        let get = |key: &str| camera_track_ini.map_or(Ok(0.0), |ini| ini.get_float("CAMERA_SETTINGS", key));
        cameras.distance_scale = 1.0 / get("DISTANCE_SCALE")?;
        cameras.upward_offset = get("UPWARD_OFFSET")?;
        cameras.wobbling_speed = get("CAMERA_WOBBLING_SPEED")?;
        cameras.wobbling_strength = get("CAMERA_WOBBLING_STRENGTH")?;
        Ok(cameras)
    }

    /// `CameraTrack::nextSet` 0x1400cbc90 (F3 again): the next set, after the last the first;
    /// its name.
    pub fn next_set(&mut self) -> String {
        if self.sets.is_empty() {
            return String::new();
        }
        self.current_set += 1;
        if self.current_set >= self.sets.len() {
            self.current_set = 0;
        }
        self.sets[self.current_set].name.clone()
    }

    /// `CameraTrack::update` 0x1400cbe30 (`dt` already times the replay's speed). `previous`
    /// is the scene camera's matrix as the frame before left it; `body` the focused car's
    /// body node in the world, `steer` its steering wheel's place in the car
    /// (`orgSteerMatrix`), `spline_position` its place on the lap, `speed` m/s. `None`: the
    /// track has no cameras, the scene camera stays as it is.
    #[allow(clippy::too_many_arguments)]
    pub fn update(&mut self, previous: &Mat, body: &Mat, steer: [f32; 3], spline_position: f32, speed: f32, dt: f32) -> Option<TrackFrame> {
        let t = point(body, steer);
        let target = [t[0], self.upward_offset + t[1], t[2]];
        let set_index = self.current_set;
        let set = self.sets.get_mut(set_index)?;
        let (index, triggered) = set.active(spline_position)?;
        let c = &set.cameras[index];
        let changed = self.old != Some((set_index, index));
        if !self.still {
            self.animation_time += dt;
        }
        if triggered {
            self.animation_time = 0.0;
        }
        let position = [c.matrix[3][0], c.matrix[3][1], c.matrix[3][2]];
        let old_back = [-previous[2][0], -previous[2][1], -previous[2][2]];
        let mut m = if c.is_fixed { c.matrix } else { create_target_exact(position, target) };
        let fov = if c.far_distance > c.near_distance && c.spline.is_none() {
            let (dx, dy, dz) = (target[0] - position[0], target[1] - position[1], target[2] - position[2]);
            let q = (dy * dy + dx * dx) + dz * dz;
            let d = if q == 0.0 { 0.0 } else { q.sqrt() };
            let mut k = (d - c.near_distance) / (c.far_distance - c.near_distance);
            k = if k > 1.0 { 1.0 } else if k >= 0.0 { k } else { 0.0 };
            // `ucomiss; je`: a gamma that is not a number counts as 1
            if c.fov_gamma != 1.0 && !c.fov_gamma.is_nan() {
                k = rustyac_math::powf(k, c.fov_gamma);
            }
            (c.min_fov - c.max_fov) * k + c.max_fov
        } else {
            c.min_fov
        };
        if !c.is_fixed {
            let angle = if !self.still {
                self.time += dt * 1000.0;
                let s = self.time * self.wobbling_speed;
                self.rot = [rustyac_math::sinf(s), rustyac_math::cosf(s), rustyac_math::cosf(s + 100.0)];
                let dot = ((-m[2][1] * old_back[1]) + (-m[2][0] * old_back[0])) + (-m[2][2] * old_back[2]);
                self.dd = (1.0 - dot) + 0.05;
                if changed && (self.dd as f64) > 0.05 {
                    self.dd = 0.05;
                }
                let k = speed * 3.6 * 0.1;
                let k = if k > 1.0 { 1.0 } else if k >= 0.0 { k } else { 0.0 };
                ((self.rot[0] * self.wobbling_strength) * self.dd) * k
            } else {
                (self.rot[0] * 0.04) * self.dd
            };
            let r = Mat44f::create_from_axis_angle(&Vec3f::new(self.rot[0], self.rot[1], self.rot[2]), angle);
            m = xm_matrix_multiply(&Mat44f { m }, &r).m;
        }
        match &c.spline {
            Some(spline) => {
                let k = self.animation_time / c.spline_animation_length;
                let k = if k > 1.0 { 1.0 } else if k >= 0.0 { k } else { 0.0 };
                let p = point_smooth(spline, k);
                let at = [position[0] + p[0], position[1] + p[1], position[2] + p[2]];
                m = create_target_exact(at, target);
            }
            None => m[3][..3].copy_from_slice(&position),
        }
        let dof_focus = if c.dof_manual {
            c.dof_focus
        } else {
            let (dx, dy, dz) = (m[3][0] - target[0], m[3][1] - target[1], m[3][2] - target[2]);
            let q = (dy * dy + dx * dx) + dz * dz;
            if q == 0.0 {
                0.0
            } else {
                q.sqrt()
            }
        };
        self.old = Some((set_index, index));
        Some(TrackFrame {
            matrix: m,
            fov,
            near: c.near,
            far: c.far,
            min_exposure: c.min_exposure,
            max_exposure: c.max_exposure,
            dof_factor: c.dof_factor,
            dof_focus,
            dof_range: c.dof_range,
            shadow_splits: c.shadow_splits,
            name: c.name.clone(),
        })
    }
}

/// The track's cameras as `CameraTrack`'s ctor finds them for `folder` (`content/tracks/<track>`)
/// and its `layout`: the sets, their distances to the AI line, the install's
/// `system/cfg/camera_track.ini`.
pub fn of_track(folder: &Path, layout: &str) -> Result<TrackCameras, String> {
    let data = if layout.is_empty() { folder.to_path_buf() } else { folder.join(layout) };
    let line = rustyac_physics::track::spline::AiSpline::load(&data.join("ai/fast_lane.ai")).map(|ai| ai.spline).unwrap_or_default();
    let ini = rustyac_content::install::ac_root().map(|ac| ac.join("system/cfg/camera_track.ini")).filter(|f| rustyac_physics::data::exists(f)).map(|f| IniReader::load(&f)).transpose()?;
    TrackCameras::load(&data, &line, ini.as_ref())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn camera(in_point: f32, out_point: f32) -> TrackCamera {
        TrackCamera { in_point, out_point, ..TrackCamera::default() }
    }

    #[test]
    fn the_last_camera_holding_the_place_films_it_and_the_last_one_stays_between() {
        let mut set = CameraSet { name: String::new(), cameras: vec![camera(0.0, 0.3), camera(0.25, 0.5), camera(0.7, 0.6)], current: None };
        assert_eq!(set.active(0.1), Some((0, true)));
        assert_eq!(set.active(0.27), Some((1, true)));
        assert_eq!(set.active(0.4), Some((1, false)));
        // in no camera's part: the one before stays
        assert_eq!(set.active(0.55), Some((1, false)));
        // IN_POINT after OUT_POINT: the same part
        assert_eq!(set.active(0.65), Some((2, true)));
    }

    #[test]
    fn a_path_starts_at_its_first_point_and_runs_past_the_one_before_last_by_the_games_formula() {
        let spline = spline_from_csv("0,0,0
10,0,0
20,0,0");
        assert_eq!(point_smooth(&spline, 0.0), [0.0, 0.0, 0.0]);
        // i = trunc(2 * t), k = (t - i / 3) * 3: near t = 1 it walks two steps past point 1
        let end = point_smooth(&spline, 1.0);
        assert!(end[0] > 29.0 && end[0] < 30.0, "{end:?}");
    }
}
