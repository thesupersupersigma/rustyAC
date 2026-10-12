// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! AC's `NetCarStateProvider`: a car driven on another machine.
//!
//! It is no `Car`. It keeps the last three states the network delivered
//! ([`NetCarStateProvider::on_remote_state_received`] @ 0x14011b1e0), and after every
//! physics step ([`NetCarStateProvider::step`] @ 0x14011bd00) works out where the car is
//! now: the newest state carried forward by its velocity and by the acceleration between
//! the last two states (`projectNetStatePos` @ 0x14011b900), the jump a new state causes
//! faded out over `remoteFactor` milliseconds, the heading eased towards the state's
//! (`getSmoothBodyMatrix` @ 0x140119310), the wheels put on the ground with ray casts.
//!
//! In the physics the car is one rigid body with the car's collider mesh (1000 kg, a box's
//! inertia; category 8, mask 4: only the meshes of this machine's cars meet it). `step`
//! puts that body where the car is after every `dWorldStep` and sets its velocity to the
//! network's, so the local car can hit it and what the solver did to it during a contact
//! is thrown away again at the end of the step. It is a ghost (category 0) while
//! disconnected, while its newest state is older than five seconds, and while the local
//! car is in the pit lane.
//!
//! Every float expression below is in the order of the game's machine code.

// Comparisons are spelled the way the original branches so NaN takes the same path.
#![allow(clippy::neg_cmp_op_on_partial_ord, clippy::double_comparisons, clippy::manual_clamp, clippy::needless_range_loop)]

use rustyac_math::{acosf, sqrtf};
use rustyac_physics::car::aero::plane4f;
use rustyac_physics::car::body::category;
use rustyac_physics::car::colliders::ColliderMesh;
use rustyac_physics::car::{PhysicsCore, RigidBody, SlipStream};
use rustyac_physics::data::ini::IniReader;
use rustyac_physics::tyre::{RayTrackCollisionProvider, SurfaceDef};
use rustyac_physics::vecmath::{create_from_euler, xm_matrix_inverse, xm_matrix_multiply, Mat44f, Vec3f};

use crate::dxmath::mat44f_lerp;

/// AC's `NetCarState` (0x88 bytes): one state of a remote car as the network delivered it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct NetCarState {
    pub pos: [f32; 3],
    /// Euler angles, radians ([`create_from_euler`]).
    pub rotation: [f32; 3],
    pub velocity: [f32; 3],
    /// The sender's physics clock, ms (smoothed against the send interval on arrival).
    pub time_stamp: f64,
    /// This machine's physics clock when the state arrived, ms.
    pub rcv_time: f64,
    pub pak_sequence_id: u8,
    /// How far above the ground the state's position is (measured once per state).
    pub target_height: f32,
    pub tyre_angular_speed: [f32; 4],
    pub engine_rpm: u16,
    /// The steering wheel, degrees.
    pub steer_angle: f32,
    /// The front wheels, degrees.
    pub wheel_angle: f32,
    pub gear_index: i32,
    pub status_bytes: u32,
    pub aoa: f32,
    pub aoa_axis: [f32; 3],
    pub ping: i32,
    /// Seconds.
    pub performance_delta: f32,
    pub gas: f32,
}

/// The status bits of a car update (`NetCarState::statusBytes`).
pub mod status {
    /// Bits 0..3: tyre LF, RF, LR, RR is skidding.
    pub const SKID_LF: u32 = 0x1;
    pub const BRAKING: u32 = 0x10;
    pub const LIGHTS: u32 = 0x20;
    pub const HORN: u32 = 0x40;
    pub const PENALTY: u32 = 0x80;
    pub const FLASH: u32 = 0x100;
    pub const KERS_CHARGING: u32 = 0x200;
    pub const DRS: u32 = 0x400;
}

/// What `NetCarStateProvider::initPhysicsValues` @ 0x140119fe0 reads of the car's data:
/// where the wheels are, how large they are, and where the picture sits on the body.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NetCarGeometry {
    /// `wheelBasePosLS`: each wheel's place in the body's frame (LF, RF, LR, RR).
    pub wheel_base_pos_ls: [Mat44f; 4],
    pub tyre_radius: [f32; 4],
    pub graphics_offset: [f32; 3],
    /// Radians.
    pub graphics_pitch_rotation: f32,
    /// `engine.ini [ENGINE_DATA] LIMITER`
    pub limiter_rpm: i32,
}

impl NetCarGeometry {
    /// From the car's data (`suspensions.ini`, `tyres.ini`, `car.ini`, `engine.ini`),
    /// each handed out by `ini` (a missing key reads as 0, as the game's reader gives it).
    pub fn from_inis(ini: &dyn Fn(&str) -> Option<IniReader>) -> NetCarGeometry {
        let float = |reader: &Option<IniReader>, section: &str, key: &str| reader.as_ref().and_then(|r| r.get_float(section, key).ok()).unwrap_or(0.0);
        let suspensions = ini("suspensions.ini");
        let wheelbase = float(&suspensions, "BASIC", "WHEELBASE");
        let cg = float(&suspensions, "BASIC", "CG_LOCATION");
        let front_y = float(&suspensions, "FRONT", "BASEY");
        let front_track = float(&suspensions, "FRONT", "TRACK");
        let rear_y = float(&suspensions, "REAR", "BASEY");
        let rear_track = float(&suspensions, "REAR", "TRACK");
        let mut wheels = [Mat44f::IDENTITY; 4];
        let front_z = (1.0 - cg) * wheelbase;
        let rear_z = -(cg * wheelbase);
        wheels[0].m[3] = [front_track * 0.5, front_y, front_z, 1.0];
        wheels[1].m[3] = [-(front_track * 0.5), front_y, front_z, 1.0];
        wheels[2].m[3] = [rear_track * 0.5, rear_y, rear_z, 1.0];
        wheels[3].m[3] = [-(rear_track * 0.5), rear_y, rear_z, 1.0];
        let tyres = ini("tyres.ini");
        let tyre_radius = [float(&tyres, "FRONT", "RADIUS"), float(&tyres, "FRONT", "RADIUS"), float(&tyres, "REAR", "RADIUS"), float(&tyres, "REAR", "RADIUS")];
        let car = ini("car.ini");
        let graphics_offset = car.as_ref().and_then(|r| r.get_float3("BASIC", "GRAPHICS_OFFSET").ok()).unwrap_or([0.0; 3]);
        let graphics_pitch_rotation = float(&car, "BASIC", "GRAPHICS_PITCH_ROTATION") * f32::from_bits(0x3c8e_f998);
        let engine = ini("engine.ini");
        let limiter_rpm = engine.as_ref().and_then(|r| r.get_int("ENGINE_DATA", "LIMITER").ok()).unwrap_or(0);
        NetCarGeometry { wheel_base_pos_ls: wheels, tyre_radius, graphics_offset, graphics_pitch_rotation, limiter_rpm }
    }

    /// From a car's `data` folder (packed or not).
    pub fn load(data_folder: &std::path::Path) -> NetCarGeometry {
        NetCarGeometry::from_inis(&|name| IniReader::load(&data_folder.join(name)).ok())
    }
}

/// The part of the `CarPhysicsState` a remote car makes up for its picture and its sound
/// (`NetCarStateProvider::state`, handed out by `getPhysicsState` @ 0x140119220).
#[derive(Clone, Debug, PartialEq)]
pub struct NetCarPhysicsState {
    pub world_matrix: Mat44f,
    pub suspension_matrix: [Mat44f; 4],
    pub tyre_matrix: [Mat44f; 4],
    pub engine_rpm: f32,
    pub wheel_angular_speed: [f32; 4],
    pub steer: f32,
    pub gas: f32,
    pub brake: f32,
    pub gear: i32,
    pub speed: f32,
    pub velocity: [f32; 3],
    pub slip_angle: [f32; 4],
    pub slip_ratio: [f32; 4],
    pub nd_slip: [f32; 4],
    pub load: [f32; 4],
    pub tyre_surface_def: [Option<SurfaceDef>; 4],
    pub acc_g: [f32; 3],
    pub lap_time: u32,
    pub tyre_contact_point: [[f32; 3]; 4],
    pub tyre_contact_normal: [[f32; 3]; 4],
    pub normalized_spline_position: f32,
    pub drivetrain_speed: f32,
    pub performance_meter: f32,
    pub limiter_rpm: i32,
    pub ground_plane: [f32; 4],
    pub time_stamp: f64,
    /// `DriverActionsState`: 0x40 the horn, 0x400 the headlight flash.
    pub actions_state: u32,
    pub kers_is_charging: bool,
    /// 1: the lights are on.
    pub status_bytes: u32,
    pub physics_guid: u8,
}

impl Default for NetCarPhysicsState {
    fn default() -> NetCarPhysicsState {
        NetCarPhysicsState {
            world_matrix: Mat44f::IDENTITY,
            suspension_matrix: [Mat44f::IDENTITY; 4],
            tyre_matrix: [Mat44f::IDENTITY; 4],
            engine_rpm: 0.0,
            wheel_angular_speed: [0.0; 4],
            steer: 0.0,
            gas: 0.0,
            brake: 0.0,
            gear: 0,
            speed: 0.0,
            velocity: [0.0; 3],
            slip_angle: [0.0; 4],
            slip_ratio: [0.0; 4],
            nd_slip: [0.0; 4],
            load: [0.0; 4],
            tyre_surface_def: [None, None, None, None],
            acc_g: [0.0; 3],
            lap_time: 0,
            tyre_contact_point: [[0.0; 3]; 4],
            tyre_contact_normal: [[0.0; 3]; 4],
            normalized_spline_position: 0.0,
            drivetrain_speed: 0.0,
            performance_meter: 0.0,
            limiter_rpm: 0,
            ground_plane: [0.0; 4],
            time_stamp: 0.0,
            actions_state: 0,
            kers_is_charging: false,
            status_bytes: 0,
            physics_guid: 0,
        }
    }
}

/// What a remote car's step reads of the world and of the local car.
pub struct StepWorld<'a> {
    /// The one world (`PhysicsEngine::core`).
    pub core: &'a mut PhysicsCore,
    /// `rayCastProvider`: the track.
    pub ground: &'a dyn RayTrackCollisionProvider,
    /// `ACClient::remoteFactor`, ms (250 unless the console changed it).
    pub remote_factor: f32,
    /// `Car::isInPitLane` of the local car (car 0).
    pub local_in_pit_lane: bool,
    /// The local car's body.
    pub local_body: Option<RigidBody>,
}

/// AC's `NetCarStateProvider`.
#[derive(Clone, Debug)]
pub struct NetCarStateProvider {
    /// The server's slot of this car.
    pub session_id: u8,
    /// `guid`: its place among the session's cars (the local car is 0).
    pub guid: u32,
    pub unix_name: String,
    pub is_disconnected: bool,
    pub net_states: [NetCarState; 3],
    pub error_vector: [f32; 3],
    pub ping: i32,
    /// `hasCollisionInThisStep`: 1 when the local car hit this one, then down by 0.003 a
    /// step; while it is not negative the body keeps part of the velocity the solver gave it.
    pub has_collision_in_this_step: f32,
    pub qos_good_packets: u32,
    pub qos_bad_packets: u32,
    pub body_matrix: Mat44f,
    pub wheel_matrix: [Mat44f; 4],
    pub susp_matrix: [Mat44f; 4],
    pub last_screen_pos: [f32; 3],
    pub last_velocity: [f32; 3],
    pub instant_velocity: [f32; 3],
    pub smooth_velocity: [f32; 3],
    pub state: NetCarPhysicsState,
    pub geometry: NetCarGeometry,
    pub tyre_local_rotation: [Mat44f; 4],
    /// The car's pit box, lifted onto its wheels: where a car without news is shown.
    pub pit_position: Mat44f,
    pub slip_stream: SlipStream,
    pub last_step_speed_kmh: f32,
    pub last_step_gas: f32,
    pub last_integration_time: f64,
    pub last_slice_time_stamp: f64,
    pub last_update_time: f64,
    pub last_time: f64,
    pub current_aoa: f32,
    pub body: RigidBody,
}

fn v3(a: [f32; 3]) -> Vec3f {
    Vec3f::new(a[0], a[1], a[2])
}

/// The acceleration between the last two states: `(v0 - v1) / dt`, 0 for `dt == 0`.
fn acceleration(states: &[NetCarState; 3], dts: f32) -> [f32; 3] {
    // ucomiss + je: equal or unordered leaves zero
    if dts < 0.0 || dts > 0.0 {
        let inv = 1.0f32 / dts;
        let (v0, v1) = (&states[0].velocity, &states[1].velocity);
        [inv * (v0[0] - v1[0]), inv * (v0[1] - v1[1]), inv * (v0[2] - v1[2])]
    } else {
        [0.0; 3]
    }
}

impl NetCarStateProvider {
    /// `NetCarStateProvider::NetCarStateProvider` @ 0x140116c70, the physics side: the body
    /// (`createRigidBody`, `setMassBox(1000, 1.4, 1.0, 4.0)`) with the car's collider mesh
    /// (category 8; mask 4, or 0 for a car named `spectator`; sub-space `guid + 1`), a
    /// ghost until the first state arrives. `core` is the engine's one world; `pit_box`
    /// the world matrix of the car's pit box (`TrackAvatar::getSpawnPosition("PIT", slot)`).
    pub fn new(core: &mut PhysicsCore, session_id: u8, guid: u32, unix_name: &str, geometry: NetCarGeometry, mesh: Option<&ColliderMesh>, pit_box: Mat44f) -> NetCarStateProvider {
        let body = NetCarStateProvider::create_body(core, guid, unix_name, mesh);
        // the pit box, lifted by the larger of the two axles' "wheel centre above its
        // lowest point" (what puts the body on its wheels there)
        let front = -(geometry.wheel_base_pos_ls[0].m[3][1] - geometry.tyre_radius[0]);
        let rear = -(geometry.wheel_base_pos_ls[2].m[3][1] - geometry.tyre_radius[2]);
        let lift = if rear <= front { front } else { rear };
        // (along the pit box's own up axis)
        let mut pit_position = pit_box;
        pit_position.m[3][0] += pit_box.m[1][0] * lift;
        pit_position.m[3][1] += pit_box.m[1][1] * lift;
        pit_position.m[3][2] = pit_box.m[1][2] * lift + pit_box.m[3][2];
        let state = NetCarPhysicsState { limiter_rpm: geometry.limiter_rpm, physics_guid: guid as u8, ..NetCarPhysicsState::default() };
        NetCarStateProvider {
            session_id,
            guid,
            unix_name: unix_name.to_string(),
            is_disconnected: true,
            net_states: [NetCarState::default(); 3],
            error_vector: [0.0; 3],
            ping: 0,
            has_collision_in_this_step: 0.0,
            qos_good_packets: 0,
            qos_bad_packets: 0,
            body_matrix: Mat44f::IDENTITY,
            wheel_matrix: [Mat44f::IDENTITY; 4],
            susp_matrix: [Mat44f::IDENTITY; 4],
            last_screen_pos: [0.0; 3],
            last_velocity: [0.0; 3],
            instant_velocity: [0.0; 3],
            smooth_velocity: [0.0; 3],
            state,
            geometry,
            tyre_local_rotation: [Mat44f::IDENTITY; 4],
            pit_position,
            // (the constructor's 0.25 is replaced by SlipStream::init's 0.5 before any step)
            slip_stream: SlipStream::default(),
            last_step_speed_kmh: 0.0,
            last_step_gas: 0.0,
            last_integration_time: 0.0,
            last_slice_time_stamp: 0.0,
            last_update_time: 0.0,
            last_time: 0.0,
            current_aoa: 0.0,
            body,
        }
    }

    /// The body of a remote car in `core`: 1000 kg with a box's inertia (1.4 x 1.0 x 4.0 m),
    /// the collider mesh in sub-space `guid + 1` (category 8; mask 4, or 0 for a car named
    /// `spectator`), and a ghost to begin with (a new remote car is disconnected).
    pub fn create_body(core: &mut PhysicsCore, guid: u32, unix_name: &str, mesh: Option<&ColliderMesh>) -> RigidBody {
        let body = core.create_rigid_body();
        core.set_mass_box(body, 1000.0, f32::from_bits(0x3fb3_3333), 1.0, 4.0);
        if let Some(mesh) = mesh {
            let mask = if unix_name == "spectator" { 0 } else { 4 };
            core.add_mesh_collider(body, mesh.vertices.clone(), mesh.indices.clone(), &mesh.matrix, category::REMOTE_CAR, mask, guid + 1);
        }
        core.set_mesh_collide_category(body, 0, 0);
        body
    }

    /// `NetCarStateProvider::onRemoteStateReceived` @ 0x14011b1e0. `physics_time` is the
    /// engine's clock (`ACClient::getPhysicsTime`), `send_interval` the client's, ms.
    /// Returns whether the state was taken.
    pub fn on_remote_state_received(&mut self, mut new: NetCarState, physics_time: f64, send_interval: f64) -> bool {
        self.is_disconnected = false;
        let (seq, last) = (new.pak_sequence_id, self.net_states[0].pak_sequence_id);
        // newer, or wrapped round, or more than a second away in time (strictly)
        let newer = seq > last || (seq < 10 && last > 0xf0) || (new.time_stamp - self.net_states[0].time_stamp).abs() > 1000.0;
        if !newer {
            return false;
        }
        self.ping = new.ping;
        new.rcv_time = physics_time;
        self.net_states[2] = self.net_states[1];
        self.net_states[1] = self.net_states[0];
        self.net_states[0] = new;
        // the time stamp is pulled towards where the send interval says it should be
        let gap = self.net_states[0].pak_sequence_id.wrapping_sub(self.net_states[1].pak_sequence_id);
        let expected = (gap as i32 as f64) * send_interval + self.net_states[1].time_stamp;
        if gap < 5 {
            let ts0 = self.net_states[0].time_stamp;
            // comisd + jae: skipped when the difference is at least a second (not for NaN)
            if !((expected - ts0).abs() >= 1000.0) {
                self.net_states[0].time_stamp = (ts0 - expected) * 0.3 + expected;
            }
        }
        if gap == 1 {
            self.qos_good_packets = self.qos_good_packets.wrapping_add(1);
        } else {
            self.qos_bad_packets = self.qos_bad_packets.wrapping_add(1);
        }
        let r0 = create_from_euler(self.net_states[0].rotation, [0.0; 3]);
        // (the game also builds the matrix of the state before; nothing reads it)
        let _ = create_from_euler(self.net_states[1].rotation, [0.0; 3]);
        // the angle between where the car points and where it goes
        let m1 = -1.0f32;
        let fx = (-r0.m[2][0]) * m1;
        let fy = (-r0.m[2][1]) * m1;
        let fz = (-r0.m[2][2]) * m1;
        let [mut vx, mut vy, mut vz] = self.net_states[0].velocity;
        let length = sqrtf((vx * vx + vy * vy) + vz * vz);
        if length < 0.0 || length > 0.0 {
            let inv = 1.0f32 / length;
            vx *= inv;
            vy *= inv;
            vz *= inv;
        }
        let dot = (vy * fy + vx * fx) + vz * fz;
        self.net_states[0].aoa = if dot > m1 && dot < 1.0 { acosf(dot) } else { 0.0 };
        let mut cx = vz * fy - vy * fz;
        let mut cy = vx * fz - vz * fx;
        let mut cz = vy * fx - vx * fy;
        let length = sqrtf((cy * cy + cx * cx) + cz * cz);
        if length < 0.0 || length > 0.0 {
            let inv = 1.0f32 / length;
            cy *= inv;
            cx *= inv;
            cz *= inv;
        }
        self.net_states[0].aoa_axis = [cx, cy, cz];
        // how far the picture is from where the new state says the car is now
        let bm = &self.body_matrix.m[3];
        self.last_screen_pos = [bm[0], bm[1], bm[2]];
        let (ts0, ts1) = (self.net_states[0].time_stamp, self.net_states[1].time_stamp);
        let since = (self.last_integration_time - ts0) * 0.001;
        let between = (ts0 - ts1) * 0.001;
        let mut a = [0.0f32; 3];
        if between < 0.0 || between > 0.0 {
            let (v0, v1) = (&self.net_states[0].velocity, &self.net_states[1].velocity);
            let (dx, dz, dy) = (v0[0] - v1[0], v0[2] - v1[2], v0[1] - v1[1]);
            let inv = 1.0f32 / (between as f32);
            a = [inv * dx, inv * dy, inv * dz];
        }
        let dtl = since as f32;
        let (p0, v0) = (self.net_states[0].pos, self.net_states[0].velocity);
        let mut error = [0.0f32; 3];
        for k in 0..3 {
            let half = a[k] * 0.5;
            let projected = (dtl * v0[k] + p0[k]) + (half * dtl) * dtl;
            error[k] = self.last_screen_pos[k] - projected;
        }
        self.error_vector = error;
        self.last_velocity = self.instant_velocity;
        let iv = &self.instant_velocity;
        // comiss + jae: kept only when the picture moves at 2 (m/s)^2 or more
        if !((iv[0] * iv[0] + iv[1] * iv[1]) + iv[2] * iv[2] >= 2.0) {
            self.error_vector = [0.0; 3];
        }
        true
    }

    /// `NetCarStateProvider::projectNetStatePos` @ 0x14011b900: state `index` carried
    /// forward to `t` (the engine's clock, ms).
    pub fn project_net_state_pos(&self, index: usize, t: f64) -> [f32; 3] {
        let dts = ((self.net_states[0].time_stamp - self.net_states[1].time_stamp) * 0.001) as f32;
        let tt = (t - self.net_states[index].time_stamp) * 0.001;
        let a = acceleration(&self.net_states, dts);
        let tt2 = (tt * tt) as f32;
        let ttf = tt as f32;
        let (p, v) = (&self.net_states[index].pos, &self.net_states[index].velocity);
        let mut out = [0.0f32; 3];
        for k in 0..3 {
            out[k] = (a[k] * 0.5) * tt2 + (v[k] * ttf + p[k]);
        }
        out
    }

    /// `NetCarStateProvider::getSmoothBodyMatrix` @ 0x140119310.
    fn get_smooth_body_matrix(&mut self, t: f64, world: &StepWorld<'_>) -> Mat44f {
        let mut m = create_from_euler(self.net_states[0].rotation, [0.0; 3]);
        let _ = create_from_euler(self.net_states[1].rotation, [0.0; 3]);
        let elapsed = t - self.net_states[0].rcv_time;
        // how much of the jump is still shown: 1 when the state is new, 0 after remoteFactor ms
        let mut fade = 1.0f32 - ((elapsed / world.remote_factor as f64) as f32);
        if fade > 1.0 {
            fade = 1.0;
        } else if !(fade >= 0.0) {
            fade = 0.0;
        }
        let v0 = self.net_states[0].velocity;
        let (ts0, ts1) = (self.net_states[0].time_stamp, self.net_states[1].time_stamp);
        let dts = ((ts0 - ts1) * 0.001) as f32;
        let mut a = [0.0f32; 3];
        if dts < 0.0 || dts > 0.0 {
            let inv = 1.0f32 / dts;
            let v1 = &self.net_states[1].velocity;
            a = [(v0[0] - v1[0]) * inv, (v0[1] - v1[1]) * inv, (v0[2] - v1[2]) * inv];
        }
        if ts0 > ts1 {
            let (vx, vy, vz) = (v0[0], v0[1], v0[2]);
            if (vy * vy + vx * vx) + vz * vz > 1.0 {
                // the heading follows the turn of the velocity since the state was taken
                let el = (elapsed * 0.001) as f32;
                let (ax, ay, az) = (a[0] * el, a[1] * el, a[2] * el);
                let (pz, px, py) = (vz + az, vx + ax, vy + ay);
                let (mut nx, mut ny, mut nz) = (vx, vy, vz);
                let length = sqrtf((nx * nx + ny * ny) + nz * nz);
                if length < 0.0 || length > 0.0 {
                    let inv = 1.0f32 / length;
                    nx *= inv;
                    ny *= inv;
                    nz *= inv;
                }
                let length = sqrtf((py * py + px * px) + pz * pz);
                let (mut qx, mut qy, mut qz) = (px, py, pz);
                if length < 0.0 || length > 0.0 {
                    let inv = 1.0f32 / length;
                    qx = px * inv;
                    qy = py * inv;
                    qz = pz * inv;
                }
                let dot = (qx * nx + qy * ny) + qz * nz;
                let angle = if dot > -1.0 && dot < 1.0 { acosf(dot) } else { 0.0 };
                let mut cx = py * vz - pz * vy;
                let mut cy = pz * vx - px * vz;
                let mut cz = px * vy - py * vx;
                let length = sqrtf((cy * cy + cx * cx) + cz * cz);
                if length < 0.0 || length > 0.0 {
                    let inv = 1.0f32 / length;
                    cx *= inv;
                    cy *= inv;
                    cz *= inv;
                }
                let turn = Mat44f::create_from_axis_angle(&Vec3f::new(cx, cy, cz), -angle);
                m = xm_matrix_multiply(&m, &turn);
            } else {
                self.current_aoa = 0.0;
            }
            m = mat44f_lerp(&self.body_matrix, &m, f32::from_bits(0x3dcc_cccd));
        }
        let p = self.project_net_state_pos(0, t);
        let e = self.error_vector;
        let (px, mut py, pz);
        // comiss + jae: a jump of 3 m or more is not faded, the picture snaps
        if !((e[0] * e[0] + e[1] * e[1]) + e[2] * e[2] >= 9.0) {
            px = p[0] + fade * e[0];
            py = p[1] + fade * e[1];
            pz = p[2] + fade * e[2];
        } else {
            px = p[0];
            py = p[1];
            pz = p[2];
        }
        m.m[3] = [px, py, pz, 1.0];
        // onto the ground: the state's own height above it
        if let Some(hit) = world.ground.ray_cast(&Vec3f::new(px, py + 1.0, pz), &Vec3f::new(0.0, -1.0, 0.0), 10.0) {
            py = hit.pos.y + self.net_states[0].target_height;
            m.m[3][1] = py;
        }
        let dti = (t - self.last_integration_time) * 0.001;
        if dti < 0.0 || dti > 0.0 {
            let b = &self.body_matrix.m[3];
            let (dx, dz, dy) = (m.m[3][0] - b[0], m.m[3][2] - b[2], m.m[3][1] - b[1]);
            let inv = 1.0f32 / (dti as f32);
            self.instant_velocity = [inv * dx, inv * dy, inv * dz];
        }
        let iv = &self.instant_velocity;
        let squared = (iv[0] * iv[0] + iv[1] * iv[1]) + iv[2] * iv[2];
        self.state.speed = if squared < 0.0 || squared > 0.0 { sqrtf(squared) } else { 0.0 };
        self.last_integration_time = t;
        m
    }

    /// The local car hit this one (the handler `ACClient` hangs on the local car's
    /// collision event, lambda @ 0x14003fb60): for a second the body keeps part of the
    /// velocity the solver gives it.
    pub fn on_local_collision(&mut self) {
        self.has_collision_in_this_step = 1.0;
    }

    /// `NetCarStateProvider::step` @ 0x14011bd00, after every `dWorldStep`. `t`: the
    /// engine's clock, ms.
    pub fn step(&mut self, t: f64, world: &mut StepWorld<'_>) {
        if self.is_disconnected {
            world.core.set_mesh_collide_category(self.body, 0, 0);
            return;
        }
        let down = Vec3f::new(0.0, -1.0, 0.0);
        // A. the newest state's height above the ground, once per state
        if self.last_slice_time_stamp < self.net_states[0].time_stamp || self.last_slice_time_stamp > self.net_states[0].time_stamp {
            let p = self.net_states[0].pos;
            if let Some(hit) = world.ground.ray_cast(&Vec3f::new(p[0], p[1] + 1.0, p[2]), &down, 10.0) {
                self.net_states[0].target_height = p[1] - hit.pos.y;
            }
            self.last_slice_time_stamp = self.net_states[0].time_stamp;
        }
        // B. a ghost while there is no news or the local car is in the pit lane
        let late = (t - self.net_states[0].rcv_time) > 5000.0;
        if late || world.local_in_pit_lane {
            world.core.set_mesh_collide_category(self.body, 0, 0);
        } else if world.core.get_mesh_collide_category(self.body, 0).unwrap_or(0) == 0 {
            if let Some(local) = world.local_body {
                let a = world.core.get_world_matrix(local).m[3];
                let b = world.core.get_world_matrix(self.body).m[3];
                let (dy, dx, dz) = (a[1] - b[1], a[0] - b[0], a[2] - b[2]);
                let squared = (dy * dy + dx * dx) + dz * dz;
                // solid again only when more than 6 m from the local car
                if (squared < 0.0 || squared > 0.0) && sqrtf(squared) > 6.0 {
                    world.core.set_mesh_collide_category(self.body, 0, category::REMOTE_CAR);
                }
            }
        }
        if late {
            self.step_lagging(t, world);
            return;
        }
        // C. what the state says of the car
        let status = self.net_states[0].status_bytes;
        self.state.status_bytes = (status & status::LIGHTS != 0) as u32;
        if status & status::HORN != 0 {
            self.state.actions_state |= 0x40;
        } else {
            self.state.actions_state &= !0x40;
        }
        if status & status::FLASH != 0 {
            self.state.actions_state |= 0x400;
        } else {
            self.state.actions_state &= !0x400;
        }
        self.state.kers_is_charging = status & status::KERS_CHARGING != 0;
        self.last_update_time = self.state.time_stamp;
        self.state.performance_meter = self.net_states[0].performance_delta;
        self.state.lap_time = (self.state.time_stamp - self.last_time) as i64 as u32;
        self.body_matrix = self.get_smooth_body_matrix(t, world);

        // the wheels: on the ground under their places on the body
        let z_axis = Vec3f::new(0.0, 0.0, 1.0);
        let y_axis = Vec3f::new(0.0, 1.0, 0.0);
        let x_axis = Vec3f::new(1.0, 0.0, 0.0);
        for i in 0..4 {
            let base = self.geometry.wheel_base_pos_ls[i];
            self.wheel_matrix[i] = xm_matrix_multiply(&base, &self.body_matrix);
            self.susp_matrix[i] = xm_matrix_multiply(&base, &self.body_matrix);
            // two degrees of camber, mirrored left to right
            let camber = if i & 1 != 0 { f32::from_bits(0xbd0e_f998) } else { f32::from_bits(0x3d0e_f998) };
            self.susp_matrix[i] = xm_matrix_multiply(&Mat44f::create_from_axis_angle(&z_axis, camber), &self.susp_matrix[i]);
            self.wheel_matrix[i] = xm_matrix_multiply(&Mat44f::create_from_axis_angle(&z_axis, camber), &self.wheel_matrix[i]);
            if i < 2 {
                let steer = (-self.net_states[0].wheel_angle) * f32::from_bits(0x3c8e_f998);
                self.susp_matrix[i] = xm_matrix_multiply(&Mat44f::create_from_axis_angle(&y_axis, steer), &self.susp_matrix[i]);
                self.wheel_matrix[i] = xm_matrix_multiply(&Mat44f::create_from_axis_angle(&y_axis, steer), &self.wheel_matrix[i]);
            }
            let (sx, sz) = (self.susp_matrix[i].m[3][0], self.susp_matrix[i].m[3][2]);
            let origin = Vec3f::new(sx, self.susp_matrix[i].m[3][1] + 1.0, sz);
            if let Some(hit) = world.ground.ray_cast(&origin, &down, 5.0) {
                // the contact point in the body's frame, kept within 10 cm of where the
                // wheel's lowest point is by design, and back: the wheel centre's height
                let mut local = world.core.world_to_local(self.body, &hit.pos);
                let base_y = base.m[3][1];
                let radius = self.geometry.tyre_radius[i];
                let tenth = f32::from_bits(0x3dcc_cccd);
                let high = (base_y + tenth) - radius;
                let low = (base_y - tenth) - radius;
                local.y = if local.y > high {
                    high
                } else if local.y >= low {
                    local.y
                } else {
                    low
                };
                let on_body = world.core.local_to_world(self.body, &local);
                self.susp_matrix[i].m[3][0] = sx;
                self.susp_matrix[i].m[3][2] = sz;
                self.susp_matrix[i].m[3][1] = on_body.y + radius;
                self.wheel_matrix[i] = self.susp_matrix[i];
                self.state.tyre_surface_def[i] = Some(hit.surface_def);
                self.state.tyre_contact_normal[i] = [hit.normal.x, hit.normal.y, hit.normal.z];
                self.state.tyre_contact_point[i] = [hit.pos.x, hit.pos.y, hit.pos.z];
            }
            // the spin
            let mut speed = self.net_states[0].tyre_angular_speed[i];
            if !(speed.abs() >= f32::from_bits(0x3e4c_cccd)) {
                speed = 0.0;
                self.net_states[0].tyre_angular_speed[i] = 0.0;
            }
            self.state.wheel_angular_speed[i] = speed;
            let speed = self.net_states[0].tyre_angular_speed[i];
            if speed < 0.0 || speed > 0.0 {
                let spin = Mat44f::create_from_axis_angle(&x_axis, speed * f32::from_bits(0x3b44_9ba6));
                self.tyre_local_rotation[i] = xm_matrix_multiply(&self.tyre_local_rotation[i], &spin);
            }
            // a skidding tyre: numbers that make smoke and noise
            if status & (status::SKID_LF << i) != 0 {
                self.state.slip_ratio[i] = f32::from_bits(0x3fcc_cccd);
                self.state.load[i] = 20.0;
                self.state.nd_slip[i] = f32::from_bits(0x4066_6666);
            } else {
                self.state.nd_slip[i] = 0.0;
                self.state.slip_ratio[i] = 0.0;
            }
            self.state.slip_angle[i] = 0.0;
            self.state.suspension_matrix[i] = self.susp_matrix[i];
            self.state.tyre_matrix[i] = xm_matrix_multiply(&self.tyre_local_rotation[i], &self.wheel_matrix[i]);
        }

        // the body's height: the mean of the four contact points plus the state's height
        let cp = &self.state.tyre_contact_point;
        let height = (((cp[1][1] + cp[0][1]) + cp[2][1]) + cp[3][1]) * 0.25 + self.net_states[0].target_height;
        self.body_matrix.m[3][1] = height;

        // the drivetrain's speed for the sound: the rear wheels', eased
        let was = &self.state.wheel_angular_speed;
        let rear = (was[2].abs() + was[3].abs()) * 0.5;
        let two_hundredths = f32::from_bits(0x3ca3_d70a);
        let mut k = rear * two_hundredths;
        if k > 1.0 {
            k = 1.0;
        } else if !(k >= 0.0) {
            k = 0.0;
        }
        let before = self.state.drivetrain_speed;
        let ratio = ((t - self.net_states[0].rcv_time) / world.remote_factor as f64) as f32;
        self.state.time_stamp = t;
        let gain = (((1.0f32 - k) as f64 * 0.7) as f32) + two_hundredths;
        self.state.drivetrain_speed = (ratio * gain) * (rear - before) + before;
        let point = |p: &[f32; 3]| v3(*p);
        self.state.ground_plane = plane4f(&point(&cp[0]), &point(&cp[1]), &point(&cp[2]));
        self.last_step_speed_kmh = self.state.speed * f32::from_bits(0x4066_6666);
        self.state.gear = self.net_states[0].gear_index;
        self.state.engine_rpm = self.net_states[0].engine_rpm as i32 as f32;
        let gas = (self.net_states[0].gas - self.last_step_gas) * f32::from_bits(0x3d4c_cccd) + self.last_step_gas;
        self.state.gas = gas;
        self.last_step_gas = gas;
        self.state.world_matrix = self.body_matrix;
        let ease = f32::from_bits(0x3b44_9ba6);
        let v0 = self.net_states[0].velocity;
        for k in 0..3 {
            self.smooth_velocity[k] = (v0[k] - self.smooth_velocity[k]) * ease + self.smooth_velocity[k];
        }
        // the acceleration in the body's frame, in g
        let per_second = f32::from_bits(0x43a6_aaab);
        let per_g = f32::from_bits(0x3dd0_da0b);
        let ax = ((self.smooth_velocity[0] - self.state.velocity[0]) * per_second) * per_g;
        let ay = ((self.smooth_velocity[1] - self.state.velocity[1]) * per_second) * per_g;
        let az = ((self.smooth_velocity[2] - self.state.velocity[2]) * per_second) * per_g;
        let inverse = xm_matrix_inverse(&self.body_matrix);
        let r = &inverse.m;
        self.state.acc_g = [
            (r[1][0] * ay + r[0][0] * ax) + r[2][0] * az,
            (r[1][1] * ay + r[0][1] * ax) + r[2][1] * az,
            (r[1][2] * ay + r[0][2] * ax) + r[2][2] * az,
        ];
        self.state.velocity = self.smooth_velocity;
        self.state.brake = if status & status::BRAKING != 0 { 1.0 } else { 0.0 };
        self.state.steer = self.net_states[0].steer_angle;

        // D. the body
        world.core.set_rotation(self.body, &self.body_matrix);
        let place = Vec3f::new(self.body_matrix.m[3][0], self.body_matrix.m[3][1], self.body_matrix.m[3][2]);
        world.core.set_position(self.body, &place);
        let k = self.has_collision_in_this_step;
        if !(k >= 0.0) {
            world.core.set_velocity(self.body, &v3(self.net_states[0].velocity));
            world.core.set_angular_velocity(self.body, &Vec3f::new(0.0, 0.0, 0.0));
        } else {
            // after a hit: part of what the solver made of the body's velocity stays
            let solved = world.core.get_velocity(self.body);
            let target = self.net_states[0].velocity;
            let v = Vec3f::new(target[0] + k * (solved.x - target[0]), target[1] + k * (solved.y - target[1]), target[2] + k * (solved.z - target[2]));
            world.core.set_velocity(self.body, &v);
        }

        // E. the wake, for the local car's slipstream
        let velocity = world.core.get_velocity(self.body);
        let position = world.core.get_position(self.body);
        self.slip_stream.set_position(&position, &velocity);
        self.has_collision_in_this_step -= f32::from_bits(0x3b44_9ba6);
    }

    /// `NetCarStateProvider::stepLagging` @ 0x14011cf60: no news for five seconds. The
    /// picture goes to the pit box; the body stays where it was, a ghost, with the last
    /// velocity of the picture.
    fn step_lagging(&mut self, t: f64, world: &mut StepWorld<'_>) {
        let down = Vec3f::new(0.0, -1.0, 0.0);
        for i in 0..4 {
            let base = self.geometry.wheel_base_pos_ls[i];
            self.wheel_matrix[i] = xm_matrix_multiply(&base, &self.pit_position);
            self.susp_matrix[i] = xm_matrix_multiply(&base, &self.pit_position);
            let (sx, sz) = (self.susp_matrix[i].m[3][0], self.susp_matrix[i].m[3][2]);
            let origin = Vec3f::new(sx, self.susp_matrix[i].m[3][1] + 1.0, sz);
            let hit = world.ground.ray_cast(&origin, &down, 10.0);
            // (a ray that hits nothing leaves the game's result all zero)
            let hit_y = hit.as_ref().map_or(0.0, |h| h.pos.y);
            if hit.is_some() {
                self.susp_matrix[i].m[3][0] = sx;
                self.susp_matrix[i].m[3][2] = sz;
                self.susp_matrix[i].m[3][1] = hit_y + self.geometry.tyre_radius[i];
                self.wheel_matrix[i] = self.susp_matrix[i];
            }
            self.state.tyre_contact_point[i] = [sx, hit_y, sz];
            self.state.tyre_contact_normal[i] = hit.as_ref().map_or([0.0; 3], |h| [h.normal.x, h.normal.y, h.normal.z]);
            self.state.wheel_angular_speed[i] = 0.0;
            self.state.slip_ratio[i] = 0.0;
            self.state.suspension_matrix[i] = self.susp_matrix[i];
            self.state.tyre_matrix[i] = xm_matrix_multiply(&self.tyre_local_rotation[i], &self.wheel_matrix[i]);
        }
        self.state.time_stamp = t;
        let cp = &self.state.tyre_contact_point;
        self.state.ground_plane = plane4f(&v3(cp[0]), &v3(cp[1]), &v3(cp[2]));
        self.state.engine_rpm = 0.0;
        self.state.gas = 0.0;
        self.state.brake = 0.0;
        self.state.steer = 0.0;
        self.state.gear = 1;
        self.state.world_matrix = self.pit_position;
        self.state.velocity = [0.0; 3];
        world.core.set_rotation(self.body, &self.body_matrix);
        let place = Vec3f::new(self.body_matrix.m[3][0], self.body_matrix.m[3][1], self.body_matrix.m[3][2]);
        world.core.set_position(self.body, &place);
        world.core.set_velocity(self.body, &v3(self.instant_velocity));
        world.core.set_angular_velocity(self.body, &Vec3f::new(0.0, 0.0, 0.0));
    }

    /// The server said the driver left (`ACP_CLIENT_DISCONNECTED`).
    pub fn mark_disconnected(&mut self) {
        self.is_disconnected = true;
    }

    /// `NetCarStateProvider::update` @ 0x14011d950, once a frame with the frame's clock
    /// (`now`, ms, on the scale of the physics clock): a car whose newest state is older
    /// than five seconds is disconnected until a state arrives (and is not drawn).
    pub fn update(&mut self, now: f64) {
        self.is_disconnected = (now - self.net_states[0].rcv_time > 5000.0) || self.is_disconnected;
    }
}
