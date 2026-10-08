// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! What the debug view draws and from where: the car's shape as boxes, AC's chase and
//! cockpit cameras, and the little matrix arithmetic the picture needs. No Direct3D in here.

use std::path::Path;

use rustyac_physics::vecmath::{xm_matrix_multiply, Mat44f, Vec3f};

use crate::input::ini::ControlsIni;
use crate::view::{CarView, Mat, IDENTITY};

pub fn mul(a: &Mat, b: &Mat) -> Mat {
    xm_matrix_multiply(&Mat44f { m: *a }, &Mat44f { m: *b }).m
}

pub fn scale_then(sx: f32, sy: f32, sz: f32, m: &Mat) -> Mat {
    let mut s = IDENTITY;
    s[0][0] = sx;
    s[1][1] = sy;
    s[2][2] = sz;
    mul(&s, m)
}

pub fn translation(x: f32, y: f32, z: f32) -> Mat {
    let mut m = IDENTITY;
    m[3] = [x, y, z, 1.0];
    m
}

fn row(m: &Mat, k: usize) -> [f32; 3] {
    [m[k][0], m[k][1], m[k][2]]
}

fn add(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn times(a: [f32; 3], k: f32) -> [f32; 3] {
    [a[0] * k, a[1] * k, a[2] * k]
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn normalized(v: [f32; 3]) -> [f32; 3] {
    let length = dot(v, v).sqrt();
    if length != 0.0 {
        times(v, 1.0 / length)
    } else {
        v
    }
}

/// A point given in a part's own axes, in the world.
pub fn point(m: &Mat, p: [f32; 3]) -> [f32; 3] {
    add(add(add(times(row(m, 0), p[0]), times(row(m, 1), p[1])), times(row(m, 2), p[2])), row(m, 3))
}

/// `mat44f::setFromHeadingUp` @ 0x1400602f0: row 1 = heading x up, row 2 = up, row 3 = minus
/// the heading (a camera looks along its own -z).
pub fn set_from_heading_up(heading: [f32; 3], up: [f32; 3], position: [f32; 3]) -> Mat {
    let s = normalized(cross(heading, up));
    [
        [s[0], s[1], s[2], 0.0],
        [up[0], up[1], up[2], 0.0],
        [-heading[0], -heading[1], -heading[2], 0.0],
        [position[0], position[1], position[2], 1.0],
    ]
}

/// `mat44f::createTarget` @ 0x14005ff50: a camera at `position` aimed at `target` that never
/// rolls (its up is the world's, made square to the view).
pub fn create_target(position: [f32; 3], target: [f32; 3]) -> Mat {
    let f = normalized(sub(target, position));
    let a = normalized([-f[2], 0.0, f[0]]);
    let mut u = normalized(cross(f, a));
    if u[1] < 0.0 {
        u = times(u, -1.0);
    }
    set_from_heading_up(f, u, position)
}

/// `Camera::rotatePitch` @ 0x14020fa80: a turn about the camera's own x axis (positive = up).
pub fn rotate_pitch(camera: &Mat, angle: f32) -> Mat {
    let turn = Mat44f::create_from_axis_angle(&Vec3f::new(1.0, 0.0, 0.0), angle);
    xm_matrix_multiply(&turn, &Mat44f { m: *camera }).m
}

/// The view matrix of a camera matrix: its rigid inverse.
pub fn view_matrix(camera: &Mat) -> Mat {
    let (x, y, z, p) = (row(camera, 0), row(camera, 1), row(camera, 2), row(camera, 3));
    [
        [x[0], y[0], z[0], 0.0],
        [x[1], y[1], z[1], 0.0],
        [x[2], y[2], z[2], 0.0],
        [-dot(p, x), -dot(p, y), -dot(p, z), 1.0],
    ]
}

/// `Camera::getPerspectiveMatrix` @ 0x14020ef00: the right-handed projection, vertical field
/// of view in degrees, depth 0..1.
pub fn perspective(fov_degrees: f32, aspect: f32, near: f32, far: f32) -> Mat {
    let f = 1.0 / ((fov_degrees * 0.017_453) * 0.5).tan();
    let q = 1.0 / (near - far);
    [[f / aspect, 0.0, 0.0, 0.0], [0.0, f, 0.0, 0.0], [0.0, 0.0, q * far, -1.0], [0.0, 0.0, (near * far) * q, 0.0]]
}

/// [`perspective`] with the depth turned round: 1 at `near`, 0 at `far`. Not the game's (its
/// depth runs 0..1): a float depth buffer is far finer this way round, which is what keeps a
/// sign painted a few millimetres in front of a wall from flickering at a distance.
pub fn perspective_reversed(fov_degrees: f32, aspect: f32, near: f32, far: f32) -> Mat {
    let f = 1.0 / ((fov_degrees * 0.017_453) * 0.5).tan();
    let q = 1.0 / (far - near);
    [[f / aspect, 0.0, 0.0, 0.0], [0.0, f, 0.0, 0.0], [0.0, 0.0, q * near, -1.0], [0.0, 0.0, (near * far) * q, 0.0]]
}

/// `a` x `b` worked out in double precision: a model's world matrix times the camera's view
/// matrix. On a track both hold coordinates of a kilometre and their product a few metres;
/// in single precision the picture's depth would wobble by millimetres.
pub fn mul_precise(a: &Mat, b: &Mat) -> Mat {
    let mut out = [[0.0f32; 4]; 4];
    for (i, row) in out.iter_mut().enumerate() {
        for (j, cell) in row.iter_mut().enumerate() {
            *cell = (0..4).map(|k| a[i][k] as f64 * b[k][j] as f64).sum::<f64>() as f32;
        }
    }
    out
}

/// A box of the car in body axes: centre and full edge lengths.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BoxShape {
    pub centre: [f32; 3],
    pub size: [f32; 3],
    pub color: [f32; 3],
}

/// What is drawn of a car besides its wheels, and where its driver's eyes are.
#[derive(Clone, Debug, PartialEq)]
pub struct CarShape {
    pub boxes: Vec<BoxShape>,
    /// The driver's eye point in body axes (from the centre of mass), and the pitch of the
    /// cockpit view, radians.
    pub eye: [f32; 3],
    pub eye_pitch: f32,
    /// The cockpit view's vertical field of view, degrees.
    pub onboard_fov: f32,
    /// `car.ini [BASIC] GRAPHICS_PITCH_ROTATION`, radians: the 3D model's pitch against the
    /// physics body. AC's cameras ride on the model.
    pub graphics_pitch: f32,
    /// `car.ini [BASIC] GRAPHICS_OFFSET`: where the 3D model's origin is in body axes.
    pub graphics_offset: [f32; 3],
    /// `car.ini [GRAPHICS] BONNET_CAMERA_POS` / `BUMPER_CAMERA_POS` (body axes from the centre
    /// of mass) and `BONNET_CAMERA_PITCH` / `BUMPER_CAMERA_PITCH` (radians; 0 without the key).
    pub bonnet_pos: [f32; 3],
    pub bonnet_pitch: f32,
    pub bumper_pos: [f32; 3],
    pub bumper_pitch: f32,
    /// `dash_cam.ini [DASH_CAM] POS`, in the 3D model's frame (its origin without the file).
    pub dash_pos: [f32; 3],
    /// The cameras of `cameras.ini` (F6).
    pub car_cameras: Vec<CarCamera>,
}

fn three(text: &str) -> Option<[f32; 3]> {
    let mut parts = text.split(',').map(|part| part.trim().parse::<f32>());
    let out = [parts.next()?.ok()?, parts.next()?.ok()?, parts.next()?.ok()?];
    Some(out)
}

impl CarShape {
    /// A car nobody has data for: a plain body.
    pub fn plain() -> CarShape {
        CarShape {
            boxes: vec![BoxShape { centre: [0.0, 0.15, 0.1], size: [1.4, 0.5, 4.2], color: [0.75, 0.08, 0.06] }],
            eye: [0.0, 0.6, 0.0],
            eye_pitch: 0.0,
            onboard_fov: 54.0,
            graphics_pitch: 0.0,
            graphics_offset: [0.0; 3],
            bonnet_pos: [0.0, 0.9, 0.0],
            bonnet_pitch: 0.0,
            bumper_pos: [0.0, 0.5, 2.0],
            bumper_pitch: 0.0,
            dash_pos: [0.0, 0.6, 0.3],
            car_cameras: Vec::new(),
        }
    }

    /// The shape of a car the simulation has built, with the cockpit view's field of view
    /// from AC's own `camera_onboard.ini` (read only) where that file exists.
    pub fn of(info: &crate::sim::CarInfo) -> CarShape {
        let mut shape = CarShape::load(&info.data_path, &info.wheel_positions, &info.tyre_width, &info.tyre_radius);
        if let Some(home) = std::env::var_os("USERPROFILE") {
            let path = Path::new(&home).join("Documents").join("Assetto Corsa").join("cfg").join("camera_onboard.ini");
            if let Ok(ini) = ControlsIni::load(&path) {
                // `CameraOnBoard::CameraOnBoard`: a field of view of 0 means the default
                let fov = ini.get_float("MODE", "FOV");
                if fov > 0.0 && fov < 170.0 {
                    shape.onboard_fov = fov;
                }
            }
        }
        shape
    }

    /// The shape from a car's data folder: the collision boxes of `colliders.ini` (what the
    /// physics car is to walls), a body box between the wheels, and the eye point of
    /// `car.ini`.
    pub fn load(data_path: &Path, wheel_positions: &[[f32; 3]; 4], tyre_width: &[f32; 4], tyre_radius: &[f32; 4]) -> CarShape {
        let mut shape = CarShape::plain();
        shape.boxes.clear();
        let red = [0.78, 0.07, 0.05];
        let dark = [0.16, 0.16, 0.18];
        // the tub between the wheels: from behind the rear axle to ahead of the front axle
        let front_z = wheel_positions[0][2];
        let rear_z = wheel_positions[2][2];
        let inner = |k: usize| wheel_positions[k][0].abs() - tyre_width[k] * 0.5;
        let half_width = (inner(0).min(inner(2)) - 0.08).clamp(0.25, 1.0);
        let floor = wheel_positions[0][1].min(wheel_positions[2][1]) - tyre_radius[0] * 0.55;
        let top = floor + 0.42;
        shape.boxes.push(BoxShape {
            centre: [0.0, (floor + top) * 0.5, (front_z + rear_z) * 0.5 + 0.1],
            size: [half_width * 2.0, top - floor, (front_z - rear_z) + tyre_radius[0] * 1.2],
            color: red,
        });
        if let Ok(ini) = ControlsIni::load_car_data(&data_path.join("colliders.ini")) {
            for index in 0..32 {
                let section = format!("COLLIDER_{index}");
                if !ini.has_section(&section) {
                    break;
                }
                if let (Some(centre), Some(size)) = (three(ini.get_string(&section, "CENTRE")), three(ini.get_string(&section, "SIZE"))) {
                    // the thin floor plates are dark, the rest (airbox, wings) body colour
                    let color = if size[1] < 0.1 { dark } else { red };
                    shape.boxes.push(BoxShape { centre, size, color });
                }
            }
        }
        if let Ok(ini) = ControlsIni::load_car_data(&data_path.join("car.ini")) {
            // `CarAvatar::makeBodyMatrix`: a point of the 3D model is at q . Rx(pitch) + offset
            let offset = three(ini.get_string("BASIC", "GRAPHICS_OFFSET")).unwrap_or([0.0; 3]);
            let pitch = ini.get_float("BASIC", "GRAPHICS_PITCH_ROTATION") * 0.017_453;
            shape.graphics_pitch = pitch;
            shape.graphics_offset = offset;
            if let Some(q) = three(ini.get_string("GRAPHICS", "DRIVEREYES")) {
                let (s, c) = pitch.sin_cos();
                shape.eye = [q[0] + offset[0], q[1] * c - q[2] * s + offset[1], q[1] * s + q[2] * c + offset[2]];
            }
            shape.eye_pitch = ini.get_float("GRAPHICS", "ON_BOARD_PITCH_ANGLE") * 0.017_453;
            // `CameraDrivableManager::CameraDrivableManager` @ 0x1400c4a80: a missing position
            // is the centre of mass, a missing pitch 0
            shape.bonnet_pos = three(ini.get_string("GRAPHICS", "BONNET_CAMERA_POS")).unwrap_or([0.0; 3]);
            shape.bumper_pos = three(ini.get_string("GRAPHICS", "BUMPER_CAMERA_POS")).unwrap_or([0.0; 3]);
            shape.bonnet_pitch = ini.get_float("GRAPHICS", "BONNET_CAMERA_PITCH") * 0.017_453;
            shape.bumper_pitch = ini.get_float("GRAPHICS", "BUMPER_CAMERA_PITCH") * 0.017_453;
            shape.dash_pos = [0.0; 3];
        }
        if let Ok(ini) = ControlsIni::load_car_data(&data_path.join("dash_cam.ini")) {
            shape.dash_pos = three(ini.get_string("DASH_CAM", "POS")).unwrap_or([0.0; 3]);
        }
        if let Ok(ini) = ControlsIni::load_car_data(&data_path.join("cameras.ini")) {
            shape.car_cameras = load_car_cameras(&ini);
        }
        // the driver's helmet, so the cockpit is somewhere to be seen from outside
        shape.boxes.push(BoxShape { centre: [shape.eye[0], shape.eye[1] - 0.02, shape.eye[2] - 0.18], size: [0.24, 0.26, 0.28], color: [0.9, 0.8, 0.1] });
        shape
    }
}

/// AC's camera modes, as far as a car is driven with them (`ACCameraManager::mode`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CameraMode {
    /// `eCockpit`: the driver's eyes.
    Cockpit,
    /// `eDrivable`: one of the five views of [`Drivable`].
    Drivable,
    /// `eCar`: one of the cameras of the car's own `cameras.ini` (F6).
    Car,
}

/// The views of AC's `CameraDrivableManager` (`currentMode`), in its order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Drivable {
    /// 3.0 m behind and 1.4 m above the rear axle.
    Chase,
    /// 3.9 m behind, 1.9 m above.
    Chase2,
    /// `car.ini [GRAPHICS] BONNET_CAMERA_POS`
    Bonnet,
    /// `BUMPER_CAMERA_POS`
    Bumper,
    /// `dash_cam.ini [DASH_CAM] POS`
    Dash,
}

impl Drivable {
    const ALL: [Drivable; 5] = [Drivable::Chase, Drivable::Chase2, Drivable::Bonnet, Drivable::Bumper, Drivable::Dash];

    fn name(self) -> &'static str {
        match self {
            Drivable::Chase => "chase",
            Drivable::Chase2 => "chase 2",
            Drivable::Bonnet => "bonnet",
            Drivable::Bumper => "bumper",
            Drivable::Dash => "dash",
        }
    }
}

/// A camera for one frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CameraFrame {
    /// The camera's world matrix (row 3 = minus the view direction, row 4 = position).
    pub matrix: Mat,
    pub fov: f32,
    pub near: f32,
}

/// AC's cameras for the driven car, as far as a debug view wants them (no glance, no head
/// shake, no fades): the cockpit (`CameraOnBoard::update` @ 0x1400c9ea0), the five drivable
/// views (`CameraDrivableManager::update` @ 0x1400c6a30) and the car's own cameras
/// (`CameraCarManager::update` @ 0x1400c47d0), with the game's two keys
/// (`ACCameraManager::setMode` @ 0x1400340d0, `Sim::onKeyDown` @ 0x14019a940).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DrivingCamera {
    pub mode: CameraMode,
    /// `CameraDrivableManager::currentMode`: kept while another mode is shown.
    pub drivable: Drivable,
    /// `ACCameraManager::lastDrivingMode`: what F1 comes back to from a car camera.
    last_driving: CameraMode,
    /// `CameraCarManager::currentCameraIndex`: kept too.
    pub car_index: usize,
    /// The chase camera's place relative to the rear axle, lagging behind the g forces.
    current_offset: [f32; 3],
    started: bool,
}

/// `system/cfg/chase_cam.ini` as shipped: distance, height, pitch (radians) of the two chase cameras.
const CHASE: [(f32, f32, f32); 2] = [(3.0, 1.4, 0.034_906), (3.9, 1.9, -0.017_453)];

impl DrivingCamera {
    /// The first chase view.
    pub fn chase() -> DrivingCamera {
        DrivingCamera { mode: CameraMode::Drivable, drivable: Drivable::Chase, last_driving: CameraMode::Drivable, car_index: 0, current_offset: [0.0; 3], started: false }
    }

    /// The camera `--camera <name>` asks for: `cockpit`, `chase`, `chase2`, `bonnet`, `bumper`,
    /// `dash`, or `car0`, `car1` ... (a camera of the car's `cameras.ini`).
    pub fn from_name(name: &str, shape: &CarShape) -> Result<DrivingCamera, String> {
        let mut camera = DrivingCamera::chase();
        match name {
            "cockpit" => {
                camera.mode = CameraMode::Cockpit;
                camera.last_driving = CameraMode::Cockpit;
            }
            "chase" => {}
            "chase2" => camera.drivable = Drivable::Chase2,
            "bonnet" => camera.drivable = Drivable::Bonnet,
            "bumper" => camera.drivable = Drivable::Bumper,
            "dash" => camera.drivable = Drivable::Dash,
            _ => {
                let index: usize = name.strip_prefix("car").and_then(|n| n.parse().ok()).ok_or_else(|| camera_name_error(name))?;
                let count = shape.car_cameras.len();
                if index >= count {
                    return Err(if count == 0 {
                        format!("--camera {name}: this car has no cameras.ini (no car cameras)")
                    } else {
                        format!("--camera {name}: this car has {count} car cameras: car0 to car{}", count - 1)
                    });
                }
                camera.mode = CameraMode::Car;
                camera.car_index = index;
            }
        }
        Ok(camera)
    }

    /// F1 (and the pad's camera button, and C): `setMode(eCockpit, false, false)`. From the
    /// cockpit to the drivable view last shown; from a drivable view to the next one, and
    /// after the last (dash) to the cockpit; from a car camera back to the view driven with
    /// before, without moving on.
    pub fn f1(&mut self) {
        self.mode = match self.mode {
            CameraMode::Cockpit => CameraMode::Drivable,
            CameraMode::Drivable => {
                // CameraDrivableManager::nextMode @ 0x1400c6790
                let next = Drivable::ALL.iter().position(|d| *d == self.drivable).unwrap_or(0) + 1;
                self.drivable = Drivable::ALL[next % Drivable::ALL.len()];
                if next == Drivable::ALL.len() {
                    CameraMode::Cockpit
                } else {
                    CameraMode::Drivable
                }
            }
            CameraMode::Car => self.last_driving,
        };
        self.last_driving = self.mode;
        self.started = false;
    }

    /// F6: to the car's cameras, at the one last shown; pressed again, to the next one
    /// (`CameraCarManager::nextCamera` @ 0x1400c4710). `count`: how many the car has.
    pub fn f6(&mut self, count: usize) {
        if count == 0 {
            return;
        }
        if self.mode == CameraMode::Car {
            self.car_index = (self.car_index + 1) % count;
        } else {
            self.mode = CameraMode::Car;
            self.car_index = self.car_index.min(count - 1);
        }
        self.started = false;
    }

    /// The view's name, for the display.
    pub fn name(&self) -> String {
        match self.mode {
            CameraMode::Cockpit => "cockpit".to_string(),
            CameraMode::Drivable => self.drivable.name().to_string(),
            CameraMode::Car => format!("car {}", self.car_index),
        }
    }

    /// The camera for a car as it is drawn now; `dt` is the time since the last frame, `acc_g`
    /// the car's acceleration in g (body axes).
    pub fn update(&mut self, view: &CarView, shape: &CarShape, acc_g: [f32; 3], dt: f32) -> CameraFrame {
        // `CarAvatar::makeBodyMatrix`: the axes of the car's 3D model, which the cameras use
        let body = &rotate_pitch(&view.body, shape.graphics_pitch);
        // the whole matrix of the 3D model (`CarAvatar::bodyMatrix`): its origin is the
        // centre of mass moved by `GRAPHICS_OFFSET`
        let model = {
            let mut moved = view.body;
            let origin = point(&view.body, shape.graphics_offset);
            moved[3] = [origin[0], origin[1], origin[2], 1.0];
            rotate_pitch(&moved, shape.graphics_pitch)
        };
        if self.mode == CameraMode::Car && shape.car_cameras.is_empty() {
            self.mode = self.last_driving;
        }
        match (self.mode, self.drivable) {
            (CameraMode::Cockpit, _) => {
                // rolls and pitches with the car (`IS_WORLD_ALIGNED=0`)
                let heading = normalized(row(body, 2));
                let up = normalized(row(body, 1));
                let eye = point(&view.body, shape.eye);
                let matrix = rotate_pitch(&set_from_heading_up(heading, up, eye), shape.eye_pitch);
                self.started = false;
                return CameraFrame { matrix, fov: shape.onboard_fov, near: 0.05 };
            }
            (CameraMode::Car, _) => {
                // CameraCarManager::update: the camera's own matrix in the frame of the model,
                // with the camera's own field of view; bolted on, nothing moves it
                self.car_index = self.car_index.min(shape.car_cameras.len() - 1);
                let camera = &shape.car_cameras[self.car_index];
                let matrix = xm_matrix_multiply(&Mat44f { m: camera.matrix }, &Mat44f { m: model }).m;
                self.started = false;
                return CameraFrame { matrix, fov: camera.fov, near: 0.05 };
            }
            (CameraMode::Drivable, Drivable::Bonnet | Drivable::Bumper | Drivable::Dash) => {
                // updateBonnet @ 0x1400c6ba0, updateBumper @ 0x1400c6df0, updateDash @ 0x1400c7b50.
                // As in the game: the bonnet view is all on the physics body; the bumper view
                // takes its axes from the model and its place from the body; the dash view
                // the other way round, and it has the cockpit's field of view and no pitch.
                let (axes, pitch, position, fov) = match self.drivable {
                    Drivable::Bonnet => ((row(&view.body, 2), row(&view.body, 1)), shape.bonnet_pitch, point(&view.body, shape.bonnet_pos), 60.0),
                    Drivable::Bumper => ((normalized(row(&model, 2)), normalized(row(&model, 1))), shape.bumper_pitch, point(&view.body, shape.bumper_pos), 60.0),
                    _ => ((row(&view.body, 2), row(&view.body, 1)), 0.0, point(&model, shape.dash_pos), shape.onboard_fov),
                };
                let mut matrix = rotate_pitch(&set_from_heading_up(axes.0, axes.1, [0.0; 3]), pitch);
                matrix[3] = [position[0], position[1], position[2], 1.0];
                self.started = false;
                return CameraFrame { matrix, fov, near: 0.05 };
            }
            _ => {}
        }
        let (distance, height, pitch) = CHASE[(self.drivable == Drivable::Chase2) as usize];
        // an upside-down car is looked at from below
        let h = if 0.0 >= body[1][1] { -height } else { height };
        let kmh = view.speed_kmh;
        let k = (kmh * 0.1).clamp(0.0, 1.0);
        let gx = (acc_g[0] * 0.4).clamp(-0.5, 0.5);
        let gz = (acc_g[2] * -0.2).clamp(-0.5, 0.5);
        let target = [gx * k, h, -distance + gz * k];
        let e = sub(self.current_offset, target);
        if !self.started || dot(e, e) >= 2.0 || kmh < 2.0 {
            self.current_offset = target;
            self.started = true;
        } else {
            let f = (dt * 3.0).clamp(0.0, 1.0);
            for (offset, target) in self.current_offset.iter_mut().zip(target) {
                *offset += (target - *offset) * f;
            }
        }
        // the middle of the rear axle
        let p = times(add(row(&view.hubs[2], 3), row(&view.hubs[3], 3)), 0.5);
        let o = self.current_offset;
        let eye = add(add(add(times(row(body, 0), o[0]), times(row(body, 1), o[1])), times(row(body, 2), o[2])), p);
        // a point 4 m ahead of the rear axle
        let look = add(p, times(row(body, 2), 4.0));
        let matrix = rotate_pitch(&create_target(eye, look), pitch);
        CameraFrame { matrix, fov: 60.0, near: 1.0 }
    }
}

/// What `--camera` accepts.
pub fn camera_name_error(name: &str) -> String {
    format!("--camera: {name:?} is not one of cockpit, chase, chase2, bonnet, bumper, dash, car0, car1 ... (the cameras of the car's cameras.ini)")
}

/// A camera of a car's `data/cameras.ini` (`CameraCarDefinition`): its matrix in the frame of
/// the car's 3D model, and its own vertical field of view in degrees.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CarCamera {
    pub matrix: Mat,
    pub fov: f32,
}

/// `CarAvatar::initCameraCar` @ 0x1400d47c0: the sections `[CAMERA_0]`, `[CAMERA_1]` ... up to
/// the first one missing, at most six. `POSITION`, `FORWARD` and `UP` are in the model's axes
/// (+x the car's left, +y up, +z forward) and nothing is mirrored; `UP` is made square to
/// `FORWARD`; a field of view that is not above zero is 60.
pub fn load_car_cameras(ini: &ControlsIni) -> Vec<CarCamera> {
    let mut count = 0;
    while ini.has_section(&format!("CAMERA_{count}")) {
        count += 1;
    }
    let unit = |v: [f32; 3]| {
        let length = ((v[0] * v[0] + v[1] * v[1]) + v[2] * v[2]).sqrt();
        if length != 0.0 {
            let inverse = 1.0 / length;
            [v[0] * inverse, v[1] * inverse, v[2] * inverse]
        } else {
            v
        }
    };
    // the game's cross product at this place, term by term
    let side = |u: [f32; 3], f: [f32; 3]| [u[2] * f[1] - u[1] * f[2], u[0] * f[2] - u[2] * f[0], u[1] * f[0] - u[0] * f[1]];
    let upright = |s: [f32; 3], f: [f32; 3]| [s[1] * f[2] - s[2] * f[1], s[2] * f[0] - s[0] * f[2], s[0] * f[1] - s[1] * f[0]];
    (0..count.min(6))
        .map(|index| {
            let section = format!("CAMERA_{index}");
            let vector = |key: &str| three(ini.get_string(&section, key)).unwrap_or([0.0; 3]);
            let position = vector("POSITION");
            let forward = unit(vector("FORWARD"));
            let up = unit(vector("UP"));
            let s = unit(side(up, forward));
            let up = unit(upright(s, forward));
            let s = unit(side(up, forward));
            let fov = ini.get_float(&section, "FOV");
            CarCamera {
                matrix: [
                    [s[0], s[1], s[2], 0.0],
                    [up[0], up[1], up[2], 0.0],
                    [-forward[0], -forward[1], -forward[2], 0.0],
                    [position[0], position[1], position[2], 1.0],
                ],
                fov: if fov > 0.0 { fov } else { 60.0 },
            }
        })
        .collect()
}

/// A vertex of the solid shapes.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
#[repr(C)]
pub struct Vertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
}

/// A cube of edge 1 around the origin, as triangles.
pub fn cube() -> Vec<Vertex> {
    let mut out = Vec::with_capacity(36);
    for axis in 0..3 {
        for sign in [-1.0f32, 1.0] {
            let (u, v) = ((axis + 1) % 3, (axis + 2) % 3);
            let mut normal = [0.0; 3];
            normal[axis] = sign;
            let corner = |a: f32, b: f32| {
                let mut p = [0.0; 3];
                p[axis] = sign * 0.5;
                p[u] = a * 0.5;
                p[v] = b * 0.5;
                Vertex { position: p, normal }
            };
            let quad = [corner(-1.0, -1.0), corner(1.0, -1.0), corner(1.0, 1.0), corner(-1.0, 1.0)];
            out.extend([quad[0], quad[1], quad[2], quad[0], quad[2], quad[3]]);
        }
    }
    out
}

/// A cylinder of radius 1 and width 1 around the origin, its axis along x (a wheel spins
/// about its own x axis), as triangles.
pub fn cylinder(segments: usize) -> Vec<Vertex> {
    let mut out = Vec::with_capacity(segments * 12);
    let ring = |k: usize| {
        let angle = k as f32 / segments as f32 * std::f32::consts::TAU;
        (angle.cos(), angle.sin())
    };
    for k in 0..segments {
        let (y0, z0) = ring(k);
        let (y1, z1) = ring(k + 1);
        let side = |x: f32, y: f32, z: f32| Vertex { position: [x, y, z], normal: [0.0, y, z] };
        out.extend([side(-0.5, y0, z0), side(0.5, y0, z0), side(0.5, y1, z1), side(-0.5, y0, z0), side(0.5, y1, z1), side(-0.5, y1, z1)]);
        for x in [-0.5f32, 0.5] {
            let cap = |y: f32, z: f32| Vertex { position: [x, y, z], normal: [x.signum(), 0.0, 0.0] };
            out.extend([cap(0.0, 0.0), cap(y0, z0), cap(y1, z1)]);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: [f32; 3], b: [f32; 3]) -> bool {
        (0..3).all(|k| (a[k] - b[k]).abs() < 1e-4)
    }

    /// A car standing at the origin, nose towards +z, as the spawn puts it.
    fn standing() -> CarView {
        let mut view = CarView { body: translation(0.0, 0.25, 0.0), ..CarView::default() };
        // the F2004's rear hubs
        view.hubs[2] = translation(0.7025, 0.33, -1.3664);
        view.hubs[3] = translation(-0.7025, 0.33, -1.3664);
        view
    }

    #[test]
    fn the_chase_camera_sits_behind_the_rear_axle() {
        let mut camera = DrivingCamera::chase();
        let frame = camera.update(&standing(), &CarShape::plain(), [0.0; 3], 0.016);
        // 3.0 m behind and 1.4 m above the middle of the rear axle
        assert!(close(row(&frame.matrix, 3), [0.0, 0.33 + 1.4, -1.3664 - 3.0]), "{:?}", frame.matrix[3]);
        // it looks forward and a little down: row 3 is minus the view direction
        let direction = times(row(&frame.matrix, 2), -1.0);
        assert!(direction[2] > 0.97 && direction[1] < -0.1 && direction[0].abs() < 1e-5, "{direction:?}");
        assert_eq!((frame.fov, frame.near), (60.0, 1.0));
        // the far chase camera
        let mut far = DrivingCamera { drivable: Drivable::Chase2, ..DrivingCamera::chase() };
        assert!(close(row(&far.update(&standing(), &CarShape::plain(), [0.0; 3], 0.016).matrix, 3), [0.0, 0.33 + 1.9, -1.3664 - 3.9]));
    }

    #[test]
    fn the_chase_camera_leans_with_the_g_forces() {
        let mut camera = DrivingCamera::chase();
        let mut view = standing();
        view.speed_kmh = 150.0;
        camera.update(&view, &CarShape::plain(), [0.0; 3], 0.016);
        // 2 g sideways: the target is 0.5 m to the side (the clamp), approached by dt * 3
        let frame = camera.update(&view, &CarShape::plain(), [2.0, 0.0, 0.0], 0.1);
        let x = frame.matrix[3][0];
        assert!((x - 0.15).abs() < 1e-4, "{x}");
        // standing still there is no lag at all
        view.speed_kmh = 0.0;
        assert_eq!(camera.update(&view, &CarShape::plain(), [2.0, 0.0, 0.0], 0.1).matrix[3][0], 0.0);
    }

    #[test]
    fn the_f2004_s_eye_point() {
        let Some(data) = [std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../cardata/ks_ferrari_f2004")].into_iter().find(|p| p.join("car.ini").is_file())
        else {
            eprintln!("NOT TESTED: cardata/ks_ferrari_f2004 is missing");
            return;
        };
        let wheels = [[0.735, 0.1, 1.6836], [-0.735, 0.1, 1.6836], [0.7025, 0.02, -1.3664], [-0.7025, 0.02, -1.3664]];
        let shape = CarShape::load(&data, &wheels, &[0.245, 0.245, 0.325, 0.325], &[0.33; 4]);
        // spec 4.6: (0.000573, 0.452217, 0.277742) from the centre of mass, pitch -5.264918 degrees
        assert!(close(shape.eye, [0.000573, 0.452217, 0.277742]), "{:?}", shape.eye);
        assert!((shape.eye_pitch + 0.0918886).abs() < 1e-5);
        // the six collision boxes, the tub and the helmet
        assert_eq!(shape.boxes.len(), 8);
        // the cockpit camera is at the eye point and looks ahead, slightly down
        let mut camera = DrivingCamera { mode: CameraMode::Cockpit, ..DrivingCamera::chase() };
        let frame = camera.update(&standing(), &shape, [0.0; 3], 0.016);
        assert!(close(row(&frame.matrix, 3), [0.000573, 0.25 + 0.452217, 0.277742]));
        let direction = times(row(&frame.matrix, 2), -1.0);
        assert!(direction[2] > 0.99 && direction[1] < -0.08 && direction[1] > -0.1, "{direction:?}");
    }

    /// The F2004's `cameras.ini`, `car.ini` and `dash_cam.ini` values the views are made of.
    fn f2004_shape() -> CarShape {
        let cameras = ControlsIni::parse(
            "[CAMERA_0]\nPOSITION=0.0035553,1.0791,-0.40759\nFORWARD=-0.0049209,-0.18102,0.98347\nUP=0.004713,0.98346,0.18104\nFOV=60\nEXPOSURE=26\n\
             [CAMERA_1]\nPOSITION=0.82675,0.61309,-0.14557\nFORWARD=0.062927,-0.076572,0.99508\nUP=0.017173,0.99699,0.075633\nFOV=60\n\
             [CAMERA_2]\nPOSITION=-0.32285,0.80252,-0.089554\nFORWARD=-0.030636,0.060454,0.9977\nUP=0.027757,0.99784,-0.05961\nFOV=45\n\
             [CAMERA_3]\nPOSITION=-0.35515,0.6834,0.57217\nFORWARD=0.39638,-0.030345,-0.91759\nUP=0.017972,0.99952,-0.02529\nFOV=60\n\
             [CAMERA_4]\nPOSITION=0.27107,0.84532,-0.10077\nFORWARD=0.10486,-0.16778,-0.98023\nUP=-0.018188,0.98518,-0.17057\nFOV=60\n\
             [CAMERA_5]\nPOSITION=0.0032349,1.0891,-0.59433\nFORWARD=0.017753,-0.085741,-0.99616\nUP=0.0018743,0.99632,-0.085721\nFOV=0\n",
        );
        CarShape {
            graphics_pitch: -0.5 * 0.017_453,
            graphics_offset: [0.0, -0.23, 0.35],
            bonnet_pos: [0.0, 0.8, -0.07],
            bonnet_pitch: -13.749608 * 0.017_453,
            bumper_pos: [0.0, 0.8, 3.0],
            bumper_pitch: 0.0,
            dash_pos: [0.000573258, 0.666138, 0.219858],
            onboard_fov: 54.0,
            car_cameras: load_car_cameras(&cameras),
            ..CarShape::plain()
        }
    }

    /// A camera's place and the direction it looks in.
    fn place_and_view(frame: &CameraFrame) -> ([f32; 3], [f32; 3]) {
        (row(&frame.matrix, 3), times(row(&frame.matrix, 2), -1.0))
    }

    fn near(a: [f32; 3], b: [f32; 3], tolerance: f32) -> bool {
        (0..3).all(|k| (a[k] - b[k]).abs() <= tolerance)
    }

    #[test]
    fn the_car_s_own_cameras() {
        let shape = f2004_shape();
        assert_eq!(shape.car_cameras.len(), 6);
        // a car standing at the origin: the body's axes are the world's
        let view = CarView::default();
        let mut camera = DrivingCamera::from_name("car0", &shape).unwrap();
        let frame = camera.update(&view, &shape, [0.0; 3], 0.016);
        let (place, looks) = place_and_view(&frame);
        // above and just behind the helmet, looking down the nose
        assert!(near(place, [0.00356, 0.84554, -0.06698], 2e-4), "{place:?}");
        assert!(near(looks, [-0.0049, -0.1724, 0.9850], 1e-3), "{looks:?}");
        // the camera's right is the car's right (-x): nothing is mirrored
        assert!((frame.matrix[0][0] + 1.0).abs() < 1e-3, "{:?}", frame.matrix[0]);
        assert_eq!((frame.fov, frame.near, camera.name().as_str()), (60.0, 0.05, "car 0"));
        // camera 2 has its own field of view; camera 5 looks back and its FOV=0 means 60
        let mut camera = DrivingCamera::from_name("car2", &shape).unwrap();
        assert_eq!(camera.update(&view, &shape, [0.0; 3], 0.016).fov, 45.0);
        let mut camera = DrivingCamera::from_name("car5", &shape).unwrap();
        let frame = camera.update(&view, &shape, [0.0; 3], 0.016);
        assert!((place_and_view(&frame).1[2] + 0.9954).abs() < 1e-3 && frame.fov == 60.0 && frame.matrix[0][0] > 0.999);
        // names that are not cameras
        assert!(DrivingCamera::from_name("car6", &shape).unwrap_err().contains("car0 to car5"));
        assert!(DrivingCamera::from_name("car0", &CarShape::plain()).unwrap_err().contains("no cameras.ini"));
        assert!(DrivingCamera::from_name("roof", &shape).is_err());
        // the sections end at the first gap; six at most
        let gap = ControlsIni::parse("[CAMERA_0]\nPOSITION=0,1,0\nFORWARD=0,0,1\nUP=0,1,0\n[CAMERA_1]\nPOSITION=0,1,0\nFORWARD=0,0,1\nUP=0,1,0\n[CAMERA_3]\nPOSITION=0,1,0\n");
        assert_eq!(load_car_cameras(&gap).len(), 2);
        let eight: String = (0..8).map(|n| format!("[CAMERA_{n}]\nPOSITION=0,1,0\nFORWARD=0,0,1\nUP=0,1,0\n")).collect();
        assert_eq!(load_car_cameras(&ControlsIni::parse(&eight)).len(), 6);
        // straight ahead, upright: the game's own matrix for it (row 1 = -x)
        let plain = load_car_cameras(&gap)[0];
        assert_eq!(plain.matrix, [[-1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, -1.0, 0.0], [0.0, 1.0, 0.0, 1.0]]);
    }

    #[test]
    fn bonnet_bumper_and_dash() {
        let shape = f2004_shape();
        let view = CarView::default();
        let shot = |name: &str| {
            let mut camera = DrivingCamera::from_name(name, &shape).unwrap();
            let frame = camera.update(&view, &shape, [0.0; 3], 0.016);
            (place_and_view(&frame), frame.fov, frame.near)
        };
        // the bonnet view: on the physics body, pitched down by the car's own angle
        let ((place, looks), fov, near_plane) = shot("bonnet");
        assert!(near(place, [0.0, 0.8, -0.07], 1e-5) && near(looks, [0.0, -0.2377, 0.9713], 1e-3), "{place:?} {looks:?}");
        assert_eq!((fov, near_plane), (60.0, 0.05));
        // the bumper view: its place from the body, its axes from the model, whose nose points
        // half a degree up for GRAPHICS_PITCH_ROTATION=-0.5 (the same turn that puts the
        // car cameras where they are)
        let ((place, looks), fov, _) = shot("bumper");
        assert!(near(place, [0.0, 0.8, 3.0], 1e-5) && near(looks, [0.0, 0.00873, 0.99996], 1e-4), "{place:?} {looks:?}");
        assert_eq!(fov, 60.0);
        // the dash view: its place in the model's frame, level with the body, the cockpit's field of view
        let ((place, looks), fov, _) = shot("dash");
        assert!(near(place, [0.00057, 0.43804, 0.56397], 2e-4) && near(looks, [0.0, 0.0, 1.0], 1e-6), "{place:?} {looks:?}");
        assert_eq!(fov, 54.0);
    }

    #[test]
    fn f1_and_f6_as_in_the_game() {
        let shape = f2004_shape();
        let names = |camera: &mut DrivingCamera, keys: &str| -> Vec<String> {
            keys.chars()
                .map(|key| {
                    if key == '1' {
                        camera.f1()
                    } else {
                        camera.f6(shape.car_cameras.len())
                    }
                    camera.name()
                })
                .collect()
        };
        // F1 from the cockpit: the drivable views in turn, then the cockpit again
        let mut camera = DrivingCamera::from_name("cockpit", &shape).unwrap();
        assert_eq!(names(&mut camera, "1111111"), ["chase", "chase 2", "bonnet", "bumper", "dash", "cockpit", "chase"]);
        // the drivable view is remembered while the cockpit is shown
        let mut camera = DrivingCamera::from_name("bonnet", &shape).unwrap();
        assert_eq!(names(&mut camera, "11111"), ["bumper", "dash", "cockpit", "chase", "chase 2"]);
        // F6 enters the car's cameras and steps through them; F1 goes back where it was
        // without moving on; F6 again shows the camera last used
        let mut camera = DrivingCamera::chase();
        assert_eq!(names(&mut camera, "66161"), ["car 0", "car 1", "chase", "car 1", "chase"]);
        let mut camera = DrivingCamera::from_name("car5", &shape).unwrap();
        assert_eq!(names(&mut camera, "6"), ["car 0"]);
        // a car without cameras.ini: F6 does nothing
        let mut camera = DrivingCamera::chase();
        camera.f6(0);
        assert_eq!(camera.name(), "chase");
    }

    #[test]
    fn view_and_projection() {
        // a camera at (1, 2, 3) looking along +z: a point 10 m ahead is on the axis, at depth 0..1
        let camera = create_target([1.0, 2.0, 3.0], [1.0, 2.0, 13.0]);
        let view = view_matrix(&camera);
        let p = point(&view, [1.0, 2.0, 13.0]);
        assert!(close(p, [0.0, 0.0, -10.0]), "{p:?}");
        let projection = perspective(60.0, 16.0 / 9.0, 1.0, 3000.0);
        let clip = |p: [f32; 3]| {
            let w = p[2] * projection[2][3];
            let z = p[2] * projection[2][2] + projection[3][2];
            (p[0] * projection[0][0] / w, p[1] * projection[1][1] / w, z / w)
        };
        let (x, y, z) = clip([0.0, 0.0, -10.0]);
        assert!(x == 0.0 && y == 0.0 && z > 0.0 && z < 1.0, "{z}");
        // +x of the world (the car's left) is on the left of the picture when looking along +z
        let left = point(&view, [2.0, 2.0, 13.0]);
        assert!(clip(left).0 < 0.0);
        assert!((clip([0.0, 0.0, -1.0]).2).abs() < 1e-6 && (clip([0.0, 0.0, -3000.0]).2 - 1.0).abs() < 1e-4);
        // the same picture with the depth turned round: 1 at the near plane, 0 at the far one
        let reversed = perspective_reversed(60.0, 16.0 / 9.0, 1.0, 3000.0);
        let depth = |z: f32| (z * reversed[2][2] + reversed[3][2]) / (z * reversed[2][3]);
        assert!((depth(-1.0) - 1.0).abs() < 1e-6 && depth(-3000.0).abs() < 1e-6 && depth(-10.0) > depth(-20.0));
        assert_eq!((reversed[0][0], reversed[1][1], reversed[2][3]), (projection[0][0], projection[1][1], projection[2][3]));
        // a product of two matrices with large numbers that nearly cancel
        let far_away = translation(1200.0, 30.0, -900.0);
        let back = translation(-1200.0, -30.0, 900.0);
        assert_eq!(mul_precise(&far_away, &back), crate::view::IDENTITY);
        // meshes
        assert_eq!(cube().len(), 36);
        assert_eq!(cylinder(24).len(), 24 * 12);
    }
}
