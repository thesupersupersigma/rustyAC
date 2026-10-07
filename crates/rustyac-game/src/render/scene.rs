// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni). Not covered by rustyAC's licenses — see LICENSING.md.

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
        if let Ok(ini) = ControlsIni::load(&data_path.join("colliders.ini")) {
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
        if let Ok(ini) = ControlsIni::load(&data_path.join("car.ini")) {
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
        }
        // the driver's helmet, so the cockpit is somewhere to be seen from outside
        shape.boxes.push(BoxShape { centre: [shape.eye[0], shape.eye[1] - 0.02, shape.eye[2] - 0.18], size: [0.24, 0.26, 0.28], color: [0.9, 0.8, 0.1] });
        shape
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CameraMode {
    /// AC's chase camera 0: 3.0 m behind and 1.4 m above the rear axle.
    Chase,
    /// AC's chase camera 1: 3.9 m behind, 1.9 m above.
    ChaseFar,
    /// The driver's eyes.
    Cockpit,
}

impl CameraMode {
    pub fn next(self) -> CameraMode {
        match self {
            CameraMode::Chase => CameraMode::ChaseFar,
            CameraMode::ChaseFar => CameraMode::Cockpit,
            CameraMode::Cockpit => CameraMode::Chase,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            CameraMode::Chase => "chase",
            CameraMode::ChaseFar => "chase far",
            CameraMode::Cockpit => "cockpit",
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

/// AC's driving cameras (`CameraDrivableManager::updateChase` @ 0x1400c7120,
/// `CameraOnBoard::update` @ 0x1400c9ea0), as far as a debug view wants them: no glance, no
/// head shake.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DrivingCamera {
    pub mode: CameraMode,
    /// The chase camera's place relative to the rear axle, lagging behind the g forces.
    current_offset: [f32; 3],
    started: bool,
}

/// `system/cfg/chase_cam.ini` as shipped: distance, height, pitch (radians) of the two chase cameras.
const CHASE: [(f32, f32, f32); 2] = [(3.0, 1.4, 0.034_906), (3.9, 1.9, -0.017_453)];

impl DrivingCamera {
    pub fn new(mode: CameraMode) -> DrivingCamera {
        DrivingCamera { mode, current_offset: [0.0; 3], started: false }
    }

    /// The camera for a car as it is drawn now; `dt` is the time since the last frame, `acc_g`
    /// the car's acceleration in g (body axes).
    pub fn update(&mut self, view: &CarView, shape: &CarShape, acc_g: [f32; 3], dt: f32) -> CameraFrame {
        // `CarAvatar::makeBodyMatrix`: the axes of the car's 3D model, which the cameras use
        let body = &rotate_pitch(&view.body, shape.graphics_pitch);
        if self.mode == CameraMode::Cockpit {
            // rolls and pitches with the car (`IS_WORLD_ALIGNED=0`)
            let heading = normalized(row(body, 2));
            let up = normalized(row(body, 1));
            let eye = point(&view.body, shape.eye);
            let matrix = rotate_pitch(&set_from_heading_up(heading, up, eye), shape.eye_pitch);
            self.started = false;
            return CameraFrame { matrix, fov: shape.onboard_fov, near: 0.05 };
        }
        let (distance, height, pitch) = CHASE[(self.mode == CameraMode::ChaseFar) as usize];
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
        let mut camera = DrivingCamera::new(CameraMode::Chase);
        let frame = camera.update(&standing(), &CarShape::plain(), [0.0; 3], 0.016);
        // 3.0 m behind and 1.4 m above the middle of the rear axle
        assert!(close(row(&frame.matrix, 3), [0.0, 0.33 + 1.4, -1.3664 - 3.0]), "{:?}", frame.matrix[3]);
        // it looks forward and a little down: row 3 is minus the view direction
        let direction = times(row(&frame.matrix, 2), -1.0);
        assert!(direction[2] > 0.97 && direction[1] < -0.1 && direction[0].abs() < 1e-5, "{direction:?}");
        assert_eq!((frame.fov, frame.near), (60.0, 1.0));
        // the far chase camera
        let mut far = DrivingCamera::new(CameraMode::ChaseFar);
        assert!(close(row(&far.update(&standing(), &CarShape::plain(), [0.0; 3], 0.016).matrix, 3), [0.0, 0.33 + 1.9, -1.3664 - 3.9]));
    }

    #[test]
    fn the_chase_camera_leans_with_the_g_forces() {
        let mut camera = DrivingCamera::new(CameraMode::Chase);
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
        let mut camera = DrivingCamera::new(CameraMode::Cockpit);
        let frame = camera.update(&standing(), &shape, [0.0; 3], 0.016);
        assert!(close(row(&frame.matrix, 3), [0.000573, 0.25 + 0.452217, 0.277742]));
        let direction = times(row(&frame.matrix, 2), -1.0);
        assert!(direction[2] > 0.99 && direction[1] < -0.08 && direction[1] > -0.1, "{direction:?}");
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
        // meshes
        assert_eq!(cube().len(), 36);
        assert_eq!(cylinder(24).len(), 24 * 12);
    }
}
