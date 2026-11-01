// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! F5's camera that orbits the car (`CameraOnBoardFree`), the cinematic free camera it is
//! built on (`CinematicFreeCamera`, F7's when `video_editing.ini [CINEMATIC_CAMERAS] ACTIVE`)
//! and the helicopter (`CameraHelicopter`, Python's `ac.setCameraMode(4)`), bit for bit with
//! the game's float order.
// `a = b + a` and `!(a >= b)` keep the game's operand order and NaN cases
#![allow(clippy::assign_op_pattern, clippy::neg_cmp_op_on_partial_ord, clippy::needless_range_loop)]

use rustyac_physics::vecmath::{xm_matrix_inverse, xm_matrix_multiply, Mat44f, Vec3f};

use std::path::{Path, PathBuf};

use super::scene::{create_target_exact, mul, rotate_pitch, three, CameraFrame};
use crate::input::ini::ControlsIni;
use crate::view::{Mat, IDENTITY};

/// The keys these cameras read, the cursor and the desktop (`GetAsyncKeyState`,
/// `GetCursorPos`, `GetWindowRect(GetDesktopWindow())`'s right edge, `SM_CYVIRTUALSCREEN`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OrbitInput {
    pub keys: [bool; 256],
    pub cursor: (i32, i32),
    pub screen: (i32, i32),
}

impl Default for OrbitInput {
    fn default() -> OrbitInput {
        OrbitInput { keys: [false; 256], cursor: (0, 0), screen: (1920, 1080) }
    }
}

impl OrbitInput {
    pub fn held(&self, vk: i32) -> bool {
        self.keys[vk as usize & 0xff]
    }
}

/// The scene camera's fields these cameras write (`Camera` +0x08 fov, +0x70 near, +0x74 far,
/// +0x19c exposure, +0x1a0/+0x1a4 the HDR levels, +0x1b0.. the depth of field).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShotCamera {
    pub matrix: Mat,
    pub fov: f32,
    pub near: f32,
    pub far: f32,
    pub exposure: f32,
    pub exposure_range: [f32; 2],
    pub dof: [f32; 3],
}

impl ShotCamera {
    /// The scene camera as the last mode left it.
    pub fn new(last: &CameraFrame) -> ShotCamera {
        ShotCamera { matrix: last.matrix, fov: last.fov, near: last.near, far: 0.0, exposure: 0.0, exposure_range: [0.0; 2], dof: [0.0; 3] }
    }

    pub fn frame(&self) -> CameraFrame {
        CameraFrame { matrix: self.matrix, fov: self.fov, near: self.near }
    }
}

/// `(a - x) clamped to 0..1`, NaN to 0 (`comiss`/`ja`, then `jae`).
fn clamp01(x: f32) -> f32 {
    if x > 1.0 {
        1.0
    } else if x >= 0.0 {
        x
    } else {
        0.0
    }
}

/// The sign `getMouseLook` takes: 1, 0, or -1 (a NaN too).
fn sign(v: f32) -> f32 {
    if v > 0.0 {
        1.0
    } else if v >= 0.0 {
        0.0
    } else {
        -1.0
    }
}

/// `m = turn(axis, angle) * m`.
fn turn(m: &mut Mat, axis: [f32; 3], angle: f32) {
    let r = Mat44f::create_from_axis_angle(&Vec3f::new(axis[0], axis[1], axis[2]), angle);
    *m = xm_matrix_multiply(&r, &Mat44f { m: *m }).m;
}

/// `CinematicFreeCamera` (0x130 bytes): ctor 0x14006c010.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CinematicCamera {
    pub fov: f32,
    pub exposure: f32,
    pub move_speed: f32,
    /// right, left, forward, back (+0x74..+0x80)
    delays: [f32; 4],
    mouse_delay_x: f32,
    mouse_delay_y: f32,
    delay_pitch: f32,
    delay_yaw: f32,
    pub movement_delay: f32,
    pub use_cinematic_features: bool,
    last_down: bool,
    last_mouse: (i32, i32),
    old_delta: (f32, f32),
    pub mouse_speed: f32,
    pub mouse_delay: f32,
    pub min_mult: f32,
    pub max_mult: f32,
    pub dof_factor: f32,
    pub dof_focus: f32,
    pub distance_scale: f32,
    /// Backspace's `Trigger` (+0xa0) as it last saw the key
    backspace: bool,
}

impl CinematicCamera {
    /// The ctor's numbers; `fov` is the scene camera's when no `video_editing.ini` gives one
    /// (the ctor leaves it).
    pub fn new(fov: f32) -> CinematicCamera {
        CinematicCamera {
            fov,
            exposure: 30.0,
            move_speed: 7.0,
            delays: [0.0; 4],
            mouse_delay_x: 0.5,
            mouse_delay_y: 0.5,
            delay_pitch: 0.0,
            delay_yaw: 0.0,
            movement_delay: 0.2,
            use_cinematic_features: true,
            last_down: false,
            last_mouse: (0, 0),
            old_delta: (0.0, 0.0),
            mouse_speed: 1.0,
            mouse_delay: 0.07,
            min_mult: 0.1,
            max_mult: 3.0,
            dof_factor: 0.0,
            dof_focus: 100.0,
            distance_scale: 1.0,
            backspace: false,
        }
    }

    /// `loadSettingsFromINI` 0x14006cf60: the player's `cfg/video_editing.ini [CAMERA_SETTINGS]`
    /// and the game's `system/cfg/camera_track.ini [CAMERA_SETTINGS] DISTANCE_SCALE`.
    pub fn read_settings(&mut self, video_editing: Option<&ControlsIni>, camera_track: Option<&ControlsIni>) {
        if let Some(ini) = video_editing {
            self.move_speed = ini.get_float("CAMERA_SETTINGS", "MOVE_SPEED");
            self.min_mult = ini.get_float("CAMERA_SETTINGS", "MIN_MULT");
            self.max_mult = ini.get_float("CAMERA_SETTINGS", "MAX_MULT");
            self.fov = ini.get_float("CAMERA_SETTINGS", "FOV");
            self.movement_delay = ini.get_float("CAMERA_SETTINGS", "MOVEMENT_DELAY");
            self.mouse_speed = ini.get_float("CAMERA_SETTINGS", "MOUSE_LOOK_SPEED");
        }
        if let Some(ini) = camera_track {
            self.distance_scale = 1.0 / ini.get_float("CAMERA_SETTINGS", "DISTANCE_SCALE");
        }
    }

    /// `getKeyboardMovement` 0x14006c580: W A S D and the arrows with their ramps, C and Z up
    /// and down, F and G roll, B and V pitch.
    fn keyboard(&mut self, m: &mut Mat, input: &OrbitInput, dt: f32, v: f32) {
        if input.held(0x26) {
            m[3][1] -= dt * v;
        }
        if input.held(0x28) {
            m[3][1] = dt * v + m[3][1];
        }
        let md = self.movement_delay;
        // (key, key, delay, along row, k from the ramp)
        type Ramp = fn(f32, f32, f32) -> f32;
        let ways: [(i32, i32, usize, usize, Ramp); 4] = [
            (0x57, 0x26, 2, 2, |d, v, dt| (d * v) * dt),
            (0x53, 0x28, 3, 2, |d, v, dt| -((d * v) * dt)),
            (0x41, 0x25, 1, 0, |d, v, dt| -((dt * v) * d)),
            (0x44, 0x27, 0, 0, |d, v, dt| (dt * v) * d),
        ];
        for (a, b, slot, axis, k) in ways {
            if input.held(a) || input.held(b) {
                self.delays[slot] = clamp01(dt / md + self.delays[slot]);
                let k = k(self.delays[slot], v, dt);
                // forward and back go along minus row 2
                let sign = if axis == 2 { -1.0f32 } else { 1.0 };
                for i in 0..3 {
                    m[3][i] = (sign * m[axis][i]) * k + m[3][i];
                }
            } else {
                self.delays[slot] = clamp01(self.delays[slot] - dt / md);
            }
        }
        if input.held(0x43) {
            m[3][1] = dt * v + m[3][1];
        }
        if input.held(0x5a) {
            m[3][1] -= dt * v;
        }
        if input.held(0x46) {
            turn(m, [0.0, 0.0, 1.0], (dt * v) * 0.2);
        }
        if input.held(0x47) {
            turn(m, [0.0, 0.0, 1.0], (dt * v) * -0.2);
        }
        if input.held(0x42) {
            *m = rotate_pitch(m, dt * v);
        }
        if input.held(0x56) {
            *m = rotate_pitch(m, -(dt * v));
        }
    }

    /// `getMouseLook` 0x14006ca60: the left button turns `m` with a lag; the yaw is about the
    /// scene camera's up (`scene`, before this frame's look). The cursor wraps at the desktop's
    /// edges: the place it is moved to, if it is.
    fn mouse_look(&mut self, m: &mut Mat, scene: &Mat, input: &OrbitInput, dt: f32) -> Option<(i32, i32)> {
        let (mut x, mut y) = input.cursor;
        let down = input.held(0x01);
        let mut moved_to = None;
        if down && self.last_down {
            let (dx, dy) = if input.held(0x58) { self.old_delta } else { (x.wrapping_sub(self.last_mouse.0) as f32, y.wrapping_sub(self.last_mouse.1) as f32) };
            self.mouse_delay_x = dx.abs() * self.mouse_delay;
            self.mouse_delay_y = dy.abs() * self.mouse_delay;
            if sign(dx) != sign(self.delay_yaw) {
                self.delay_yaw = 0.0;
            }
            self.delay_yaw = dx + self.delay_yaw;
            if self.delay_yaw.abs() > 3.0 {
                self.delay_yaw = sign(self.delay_yaw) * 3.0;
            }
            let step = (((self.delay_yaw * self.mouse_delay_x) * self.mouse_speed) * dt) * 0.5;
            self.delay_yaw -= step;
            let inverse = xm_matrix_inverse(&Mat44f { m: *scene }).m;
            turn(m, [inverse[1][0], inverse[1][1], inverse[1][2]], -step);
            if sign(dy) != sign(self.delay_pitch) {
                self.delay_pitch = 0.0;
            }
            self.delay_pitch = dy + self.delay_pitch;
            if self.delay_pitch.abs() > 3.0 {
                self.delay_pitch = sign(self.delay_pitch) * 3.0;
            }
            let step = (((self.delay_pitch * self.mouse_delay_y) * self.mouse_speed) * dt) * 0.5;
            self.delay_pitch -= step;
            *m = rotate_pitch(m, -step);
            let (right, height) = input.screen;
            let mut moved = false;
            if x >= right - 10 {
                x = 15;
                moved = true;
            } else if x <= 10 {
                x = right - 15;
                moved = true;
            }
            if y == height - 1 {
                y = 0;
                moved = true;
            } else if y == 0 {
                y = height - 1;
                moved = true;
            }
            if moved {
                moved_to = Some((x, y));
            }
            self.old_delta = (dx, dy);
        }
        self.last_mouse = (x, y);
        self.last_down = down;
        moved_to
    }

    /// `standardMouseLook` 0x14006d820: the left button turns the scene camera itself as F7's
    /// look does, `speed` per pixel and degree of fov, sixty to the turn.
    fn standard_look(&mut self, scene: &mut Mat, scene_fov: f32, input: &OrbitInput, speed: f32) {
        let (x, y) = input.cursor;
        let down = input.held(0x01);
        if down && self.last_down {
            let k = f32::from_bits(0x3c88_8889);
            *scene = rotate_pitch(scene, -(((y.wrapping_sub(self.last_mouse.1) as f32 * speed) * scene_fov) * k));
            let a = ((self.last_mouse.0.wrapping_sub(x) as f32 * speed) * scene_fov) * k;
            let inverse = xm_matrix_inverse(&Mat44f { m: *scene }).m;
            turn(scene, [inverse[1][0], inverse[1][1], inverse[1][2]], a);
        }
        self.last_down = down;
        self.last_mouse = (x, y);
    }

    /// `setMatrixMovementFromUserInput` 0x14006d750 on `m`; `scene` is the scene camera when
    /// `m` is not it (F5), `None` when it is (the cinematic camera). The cursor's new place.
    pub fn movement(&mut self, m: &mut Mat, scene: Option<&mut Mat>, scene_fov: f32, input: &OrbitInput, dt: f32) -> Option<(i32, i32)> {
        let (mut look, mut v) = (self.mouse_speed, self.move_speed);
        if input.held(0x10) {
            v *= self.min_mult;
            look *= self.min_mult;
        }
        if input.held(0x09) {
            v *= self.max_mult;
            look *= self.max_mult;
        }
        self.keyboard(m, input, dt, v);
        if self.use_cinematic_features {
            let axis_from = scene.map_or(*m, |s| *s);
            self.mouse_look(m, &axis_from, input, dt)
        } else {
            match scene {
                Some(s) => self.standard_look(s, scene_fov, input, look),
                None => self.standard_look(m, scene_fov, input, look),
            }
            None
        }
    }

    /// `setCameraParameterFromUserInput` 0x14006d690: numpad + and - widen and narrow `fov`.
    pub fn fov_keys(&self, fov: &mut f32, input: &OrbitInput, dt: f32) {
        let mut s = self.move_speed;
        if input.held(0x09) {
            s *= self.max_mult;
        }
        if input.held(0x10) {
            s *= self.min_mult;
        }
        // `comiss; jae`: a NaN fov widens
        if input.held(0x6b) && !(*fov >= 125.0) {
            *fov = s * dt + *fov;
        }
        if input.held(0x6d) && *fov > 1.0 {
            *fov -= s * dt;
        }
    }

    /// `CinematicFreeCamera::update` 0x14006d980 as F7's camera: Backspace switches the
    /// cinematic look on and off (the system message it shows: `Some`), then the moves and
    /// the fov. Insert's reload of the settings and Delete's blur are the caller's.
    pub fn update(&mut self, camera: &mut ShotCamera, input: &OrbitInput, dt: f32) -> (Option<(i32, i32)>, Option<&'static str>) {
        let backspace = input.held(0x08);
        let mut message = None;
        if backspace && !self.backspace {
            self.use_cinematic_features = !self.use_cinematic_features;
            message = Some(if self.use_cinematic_features { "Cinematic Camera LOOK ON" } else { "Cinematic Camera LOOK OFF" });
        }
        self.backspace = backspace;
        let fov = camera.fov;
        let cursor = self.movement(&mut camera.matrix, None, fov, input, dt);
        let mut own = self.fov;
        self.fov_keys(&mut own, input, dt);
        self.fov = own;
        camera.fov = own;
        (cursor, message)
    }
}

/// A `CameraValue`: the value and its limits.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CameraValue {
    pub value: f32,
    pub min: f32,
    pub max: f32,
}

/// F5, `CameraOnBoardFree` (0x1b0 bytes): ctor 0x140115100, update 0x140115db0.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OnboardFreeCamera {
    pub cinematic: CinematicCamera,
    pub theta: CameraValue,
    pub phi: CameraValue,
    pub radius: f32,
    pub min_radius: f32,
    pub max_radius: f32,
    pub spherical: bool,
    /// the keys-and-orbit way's place, on the car's axes (`setPosition` writes its row 3)
    pub matrix: Mat,
    /// the ESC menu's slow turn (`setIntroMode`)
    pub esc_mode: bool,
    pub esc_dof_factor: f32,
    pub rotation_speed: f32,
    /// `lockCamera` (+0x130): the pit crew's stop holds the mouse's drag
    pub locked: bool,
}

impl OnboardFreeCamera {
    /// The ctor (after `CinematicFreeCamera`'s) with `system/cfg/camera_onboard_free.ini`
    /// when there is one, and `Sim::Sim`'s `setPosition`: the car's `data/escmode.ini
    /// [SETTINGS] POSITION`, else (0, 1.4, 3.2).
    pub fn new(cinematic: CinematicCamera, ini: Option<&ControlsIni>, position: Option<[f32; 3]>) -> OnboardFreeCamera {
        let mut c = OnboardFreeCamera {
            cinematic,
            theta: CameraValue { value: 75.0, min: 5.0, max: 85.0 },
            phi: CameraValue { value: -45.0, min: 0.0, max: 360.0 },
            radius: 5.0,
            min_radius: 2.5,
            max_radius: 10.0,
            spherical: true,
            matrix: IDENTITY,
            esc_mode: true,
            esc_dof_factor: 1.0,
            rotation_speed: 0.1,
            locked: false,
        };
        c.cinematic.dof_focus = 0.1;
        c.cinematic.dof_factor = 0.0;
        c.cinematic.fov = 60.0;
        if let Some(ini) = ini {
            c.rotation_speed = ini.get_float("CAMERA_SETTINGS", "ROTATION_SPEED");
            c.esc_dof_factor = ini.get_float("CAMERA_SETTINGS", "ESC_DOF_FACTOR");
            c.cinematic.min_mult = ini.get_float("CAMERA_SETTINGS", "MIN_MULT");
            c.cinematic.max_mult = ini.get_float("CAMERA_SETTINGS", "MAX_MULT");
            c.cinematic.fov = ini.get_float("CAMERA_SETTINGS", "FOV");
            c.cinematic.exposure = ini.get_float("CAMERA_SETTINGS", "EXPOSURE");
            c.spherical = ini.get_int("CAMERA_SETTINGS", "SPHERICAL_COORDS") > 0;
        }
        c.cinematic.last_down = false;
        c.matrix[3][..3].copy_from_slice(&position.unwrap_or([0.0, 1.4, 3.2]));
        c
    }

    /// The mouse wheel (lambda 0x140115a70): nearer or farther, within the limits.
    pub fn wheel(&mut self, delta: i32) {
        let r = self.radius - delta as f32 * 0.002;
        self.radius = if self.max_radius < r {
            self.max_radius
        } else if r < self.min_radius {
            self.min_radius
        } else {
            r
        };
    }

    /// `PitCrew::onPitStopBegin`'s `forceThetaPhi` 0x140115b90.
    pub fn force_theta_phi(&mut self, theta: f32, phi: f32, radius: f32) {
        self.theta.value = theta;
        self.phi.value = phi;
        self.radius = radius;
    }

    /// `updateSphericalCoord` 0x1401167e0: the left button drags theta and phi.
    fn spherical_place(&mut self, input: &OrbitInput, dt: f32) -> [f32; 3] {
        let s = dt * 20.0;
        if self.locked {
            return spherical(self.radius, self.theta.value, self.phi.value);
        }
        let (x, y) = input.cursor;
        let down = input.held(0x01);
        let c = &mut self.cinematic;
        if down && c.last_down {
            let v = self.theta.value - ((y.wrapping_sub(c.last_mouse.1) as f32 * s) * 0.3);
            self.theta.value = if v > self.theta.max {
                self.theta.max
            } else if v >= self.theta.min {
                v
            } else {
                self.theta.min
            };
            self.phi.value -= (x.wrapping_sub(c.last_mouse.0) as f32 * s) * 0.3;
        }
        c.last_down = down;
        c.last_mouse = (x, y);
        spherical(self.radius, self.theta.value, self.phi.value)
    }

    /// `update` 0x140115db0 on the scene camera; `body` the focused car's 3D matrix,
    /// `replay_loaded` / `replay_playing` the replay manager's `hasLoadedReplay` / `isActive`,
    /// `hdr` the sim's HDR levels. The cursor's new place, if the look moves it.
    #[allow(clippy::too_many_arguments)]
    pub fn update(&mut self, camera: &mut ShotCamera, body: &Mat, replay_loaded: bool, replay_playing: bool, hdr: [f32; 2], input: &OrbitInput, dt: f32) -> Option<(i32, i32)> {
        if replay_loaded {
            self.esc_mode = false;
        }
        camera.fov = self.cinematic.fov;
        camera.exposure = self.cinematic.exposure;
        camera.exposure_range = hdr;
        camera.near = 0.1;
        let mut speed = self.rotation_speed;
        if self.esc_mode && !replay_playing {
            camera.dof[0] = self.esc_dof_factor;
            camera.dof[2] = 250.0;
            let p = camera.matrix[3];
            let w = body[3];
            let d2 = ((p[1] - w[1]) * (p[1] - w[1]) + (p[0] - w[0]) * (p[0] - w[0])) + (p[2] - w[2]) * (p[2] - w[2]);
            let d = if d2 == 0.0 { 0.0 } else { d2.sqrt() };
            camera.dof = [1.0, d, 3.0];
            orbit_static(&mut self.matrix, dt * speed);
            camera.matrix = mul(&self.matrix, body);
            return None;
        }
        if input.held(0x10) {
            speed *= self.cinematic.min_mult;
        }
        if input.held(0x09) {
            speed *= self.cinematic.max_mult;
        }
        if self.spherical {
            camera.dof[0] = 0.0;
            let p = self.spherical_place(input, dt);
            camera.matrix = mul(&create_target_exact(p, [0.0, 0.5, 0.0]), body);
            if input.held(0x02) {
                self.phi.value = dt * 10.0 + self.phi.value;
            }
            return None;
        }
        let fov = camera.fov;
        let cursor = self.cinematic.movement(&mut self.matrix, Some(&mut camera.matrix), fov, input, dt);
        let mut own = self.cinematic.fov;
        self.cinematic.fov_keys(&mut own, input, dt);
        self.cinematic.fov = own;
        if input.held(0x02) {
            orbit(&mut self.matrix, input, dt * speed);
        }
        camera.matrix = mul(&self.matrix, body);
        cursor
    }
}

/// What `ACCameraManager`'s ctor 0x140031270 builds of these, read as the game reads them:
/// F5's camera (its cinematic part's `video_editing.ini` from `own_cfg` else Documents' `cfg`,
/// the game's `system/cfg/camera_track.ini` and `camera_onboard_free.ini` from `own_cfg`'s
/// `system/cfg` else the install's), placed by `Sim::Sim` from the player's car's
/// `escmode.ini`; and F7's cinematic camera when `video_editing.ini [CINEMATIC_CAMERAS]
/// ACTIVE` asks for it instead of the mouse one.
pub fn load(own_cfg: Option<&Path>, car_data: Option<&Path>) -> (OnboardFreeCamera, Option<CinematicCamera>) {
    let [video, track, onboard] = settings(own_cfg);
    // (the ctor leaves the fov to the ini; 60 stands in for the memory it was not given)
    let cinematic = || {
        let mut c = CinematicCamera::new(60.0);
        c.read_settings(video.as_ref(), track.as_ref());
        c
    };
    let position = car_data.and_then(|d| ControlsIni::load_car_data(&d.join("escmode.ini")).ok()).map(|ini| three(ini.get_string("SETTINGS", "POSITION")).unwrap_or([0.0; 3]));
    let f5 = OnboardFreeCamera::new(cinematic(), onboard.as_ref(), position);
    let f7 = video.as_ref().filter(|ini| ini.has_section("CINEMATIC_CAMERAS") && ini.get_int("CINEMATIC_CAMERAS", "ACTIVE") != 0).map(|_| cinematic());
    (f5, f7)
}

/// `video_editing.ini`, `camera_track.ini` and `camera_onboard_free.ini` as `load` finds them
/// (the first two: `CinematicFreeCamera::loadSettingsFromINI`'s, again on Insert).
pub fn settings(own_cfg: Option<&Path>) -> [Option<ControlsIni>; 3] {
    let documents = rustyac_content::install::documents().map(|d| d.join("cfg"));
    let root = rustyac_content::install::ac_root();
    let first = |paths: [Option<PathBuf>; 2]| paths.into_iter().flatten().find_map(|p| ControlsIni::load_car_data(&p).ok());
    let system = |name: &str| first([own_cfg.map(|c| c.join("system/cfg").join(name)), root.as_ref().map(|r| r.join("system/cfg").join(name))]);
    [first([own_cfg.map(|c| c.join("video_editing.ini")), documents.map(|d| d.join("video_editing.ini"))]), system("camera_track.ini"), system("camera_onboard_free.ini")]
}

/// `getSphericalCoord` 0x140115bb0: radius, theta and phi in degrees to a place.
pub fn spherical(r: f32, theta: f32, phi: f32) -> [f32; 3] {
    let a = (theta * std::f32::consts::PI) * 0.005_555_555_7;
    let s = rustyac_math::sinf(a);
    let b = (phi * std::f32::consts::PI) * 0.005_555_555_7;
    let s8 = s * r;
    [rustyac_math::cosf(b) * s8, rustyac_math::cosf(a) * r, -(rustyac_math::sinf(b) * s8)]
}

/// `signedAngleBetweenVersors` 0x140115c90.
fn signed_angle(a: [f32; 3], b: [f32; 3]) -> f32 {
    let mut a = Vec3f::new(a[0], a[1], a[2]);
    let mut b = Vec3f::new(b[0], b[1], b[2]);
    a.normalize();
    b.normalize();
    let c0 = b.z * a.y - b.y * a.z;
    let c1 = b.x * a.z - a.x * b.z;
    let c2 = a.x * b.y - b.x * a.y;
    let l2 = (c1 * c1 + c0 * c0) + c2 * c2;
    let len = if l2 == 0.0 { 0.0 } else { l2.sqrt() };
    let angle = rustyac_math::atan2f(len, (a.x * b.x + b.y * a.y) + a.z * b.z);
    // `comiss; jae`: a NaN turns the other way too
    if (c0 * 0.0 + c1) + c2 * 0.0 >= 0.0 {
        angle
    } else {
        -angle
    }
}

/// Row 3 of `m` turned by `r` (`R.M21*y + R.M11*x + R.M31*z + R.M41` ...).
fn turned_place(r: &Mat, m: &Mat) -> [f32; 3] {
    let [x, y, z] = [m[3][0], m[3][1], m[3][2]];
    [0, 1, 2].map(|k| ((r[1][k] * y + r[0][k] * x) + r[2][k] * z) + r[3][k])
}

/// `updateOrbitStatic` 0x1401166b0: round the car's up, looking at its origin.
fn orbit_static(m: &mut Mat, angle: f32) {
    let r = Mat44f::create_from_axis_angle(&Vec3f::new(0.0, 1.0, 0.0), angle).m;
    *m = create_target_exact(turned_place(&r, m), [0.0; 3]);
}

/// `mat44f::setFromHeadingUp` 0x1400602f0 in place: rows 0..2 from the heading and up.
fn set_from_heading_up(m: &mut Mat, h: [f32; 3], u: [f32; 3]) {
    let mut s = [u[2] * h[1] - u[1] * h[2], u[0] * h[2] - h[0] * u[2], h[0] * u[1] - u[0] * h[1]];
    let len = ((s[1] * s[1] + s[0] * s[0]) + s[2] * s[2]).sqrt();
    // `ucomiss; je`: a NaN length divides too
    if len != 0.0 {
        let k = 1.0 / len;
        s = s.map(|v| v * k);
    }
    m[0][..3].copy_from_slice(&s);
    m[1][..3].copy_from_slice(&u);
    m[2][..3].copy_from_slice(&[-h[0], -h[1], -h[2]]);
}

/// `updateOrbit` 0x140116310: the right button's turn round the car (Caps Lock the other
/// way); the left button looks away from the car, the middle one keeps the matrix's tilt.
fn orbit(m: &mut Mat, input: &OrbitInput, angle: f32) {
    // `comiss; ja`: a NaN angle goes on
    if signed_angle([m[1][0], m[1][1], m[1][2]], [0.0, 1.0, 0.0]).abs() > 1.0 {
        return;
    }
    let sign = if input.held(0x14) { -1.0f32 } else { 1.0 };
    let r = Mat44f::create_from_axis_angle(&Vec3f::new(0.0, 1.0, 0.0), sign * angle).m;
    let p = turned_place(&r, m);
    let old = [-m[2][0], 0.0, -m[2][2]];
    let t = create_target_exact(p, [0.0; 3]);
    let h = if input.held(0x01) { [t[2][0], -0.0, t[2][2]] } else { [-t[2][0], 0.0, -t[2][2]] };
    turn(m, [0.0, 1.0, 0.0], signed_angle(old, h));
    m[3][..3].copy_from_slice(&p);
    if !input.held(0x04) {
        let h = [-m[2][0], -m[2][1], -m[2][2]];
        set_from_heading_up(m, h, [0.0, 1.0, 0.0]);
    }
}

/// `CameraHelicopter` (ctor 0x1400c8010, update 0x1400c80e0): high above the car, looking
/// down at it. `ACCameraManager`'s ctor sets the offset (30, 180, 20).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HelicopterCamera {
    pub offset: [f32; 3],
}

impl Default for HelicopterCamera {
    fn default() -> HelicopterCamera {
        HelicopterCamera { offset: [30.0, 180.0, 20.0] }
    }
}

impl HelicopterCamera {
    pub fn update(&self, camera: &mut ShotCamera, body: &Mat) {
        let target = [body[3][0], body[3][1], body[3][2]];
        let eye = [target[0] + self.offset[0], target[1] + self.offset[1], target[2] + self.offset[2]];
        camera.matrix = create_target_exact(eye, target);
        camera.fov = 20.0;
        camera.near = 1.0;
        camera.far = 35000.0;
        camera.dof[0] = 0.0;
    }
}
