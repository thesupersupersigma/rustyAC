// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! The four mechanics who change the player's tyres in a pit stop, and their wheel guns.
//!
//! `PitCrew` (0xa8 bytes): constructor 0x1400a0b80, `pitCrewUpdate` 0x1400a3f20,
//! `onPitStopBegin` 0x1400a33e0, `updateCurrentPosition` 0x1400a4500, `onPitStopFinished`
//! 0x1400a3b50, `reset` 0x1400a4420 and the handlers 0x1400a2330 (replay), 0x1400a2410
//! (`PitStop::evPitStopCommit`), 0x1400a27c0 (`PitStop::evPitStopOver`). Each mechanic is a
//! copy of `pitcrewtyre.kn5` played through `pitcrewtyre.ksanim` by his own clock; the car's
//! wheel nodes follow the "old tyre" node in his hands and a copy of the wheel
//! (`SceneGraphCloner`) is the new tyre.
//!
//! The camera manager and the sound are behind [`CrewHost`]: this crate has neither.

use rustyac_math::sqrtf;
use rustyac_physics::data::ini::IniReader;
use rustyac_physics::vecmath::{xm_matrix_inverse, xm_matrix_multiply, Mat44f};

use crate::animator::{Animation, AnimationPlayer};
use crate::graphics::Graphics;
use crate::model::Kn5Io;
use crate::pit_crew::{load_crew_skin, CrewCar};
use crate::scene::{NodeId, Scene};

/// What the tyre changers read of the pit stop (`PitStop`) every frame.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CrewPitStop {
    /// `PitStop::state`: 0 away, 1 waiting for the driver, 2 choosing, 3 at work, 4 done, 5 reset
    pub state: i32,
    /// `pitTimer`, `maxPitTimer`, `tyreTime`
    pub pit_timer: f32,
    pub max_pit_timer: f32,
    pub tyre_time: f32,
}

/// `PitStopStart`, what `PitStop::evPitStopCommit` carries.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CrewStart {
    pub tyre_change_time: f32,
    pub total_time: f32,
    pub force_change_camera: bool,
}

/// What `PitCrew::pitCrewUpdate` reads of the frame.
#[derive(Clone, Copy, Debug)]
pub struct CrewInputs {
    /// `ReplayManager::isInReplaymode`
    pub replay: bool,
    /// the pause menu shows
    pub pause_menu: bool,
    /// 1 practice, 2 qualifying, 3 race: the crew works in these only
    pub session_type: i32,
    pub pit: CrewPitStop,
    /// `CarPhysicsState::tyreMatrix`, `tyreContactPoint` of the crew's car
    pub tyre_matrix: [Mat44f; 4],
    pub tyre_contact_point: [[f32; 3]; 4],
    /// `Sim::getFocusedCarIndex() == car->getGuid()`: the wheel guns are heard
    pub focused: bool,
}

/// The camera manager and the sound, as the crew uses them.
pub trait CrewHost {
    /// `ACCameraManager::mode`
    fn camera_mode(&mut self) -> i32;
    /// `ACCameraManager::setMode` 0x1400340d0
    fn set_camera_mode(&mut self, mode: i32, a: bool, b: bool);
    /// `CameraOnBoardFree::forceThetaPhi` 0x140115b90
    fn force_theta_phi(&mut self, theta: f32, phi: f32, radius: f32);
    /// `CameraOnBoardFree::lockCamera`
    fn lock_camera(&mut self, locked: bool);
    /// `Sim::isVrConnected`
    fn is_vr_connected(&mut self) -> bool {
        false
    }
    /// `ACCameraManager::registerToFadeInEvent` 0x140033ce0 and `fadeIn` 0x140033690 with
    /// this total time: at the black frame the host calls [`PitCrew::on_fade_in_finished`].
    fn fade_in(&mut self, fade_total_time: f32);
    /// `AudioEvent::isPlaying` 0x1401fa170 of a wheel's gun: `screw` (`event:/common/screw`)
    /// or not (`event:/common/unscrew`)
    fn event_is_playing(&mut self, wheel: usize, screw: bool) -> bool;
    /// `AudioEvent::set3DAttributes` 0x1401fb370 (at the matrix, standing still), then
    /// `AudioEvent::start` 0x1401fbf40
    fn event_start(&mut self, wheel: usize, screw: bool, matrix: &Mat44f);
    /// `AudioEvent::stop` 0x1401fc040
    fn event_stop(&mut self, wheel: usize, screw: bool);
}

/// `PitCrew::CrewMember` (0xc8 bytes): one mechanic and his wheel.
pub struct CrewMember {
    player: AnimationPlayer,
    pub model: NodeId,
    old_tyre_null: NodeId,
    new_tyre_null: NodeId,
    pub new_tyre_model: NodeId,
    default_position: NodeId,
    pub car_wheels: Vec<NodeId>,
    tyre_old_transforms: Vec<Mat44f>,
    /// 0 idle, 1 going to his place, 2 unscrewing, 3 changing the tyre, 4 screwing, 5 finishing
    pub state: i32,
    pub time_to_position: f32,
    pub time_to_change_tyre: f32,
    pub time_to_finish: f32,
    pub time_screw: f32,
    pub timer: f32,
    loop_speed_mult: f32,
    percentage_begin_un_screw: f32,
    percentage_un_screw: f32,
    percentage_end_un_screw: f32,
    percentage_begin_screw: f32,
    percentage_screw: f32,
    percentage_end_screw: f32,
    time_to_position_default: f32,
    time_to_change_tyre_default: f32,
    time_to_finish_default: f32,
}

/// The timeline's numbers of `content/objects3D/pitcrewtyre.ini [ANIMATION]`, as the
/// constructor reads them for each mechanic.
#[derive(Clone, Copy, Debug)]
pub struct CrewTimeline {
    pub time_to_position_default: f32,
    pub time_to_change_tyre_default: f32,
    pub time_to_finish_default: f32,
    pub loop_speed_mult: f32,
    pub percentage_begin_un_screw: f32,
    pub percentage_un_screw: f32,
    pub percentage_end_un_screw: f32,
    pub percentage_begin_screw: f32,
    pub percentage_screw: f32,
    pub percentage_end_screw: f32,
}

impl CrewTimeline {
    /// `tyre_change_time_sec`: `car.ini [PIT_STOP] TYRE_CHANGE_TIME_SEC` (10 when missing).
    pub fn read(ini: &IniReader, tyre_change_time_sec: f32) -> CrewTimeline {
        let float = |key: &str| ini.get_float("ANIMATION", key).unwrap_or(0.0);
        let scale = tyre_change_time_sec * f32::from_bits(0x3dcc_cccd);
        let total = |key: &str| float(key) / float("TOTAL_FRAMES");
        CrewTimeline {
            time_to_finish_default: float("TIME_TO_FINISH") * scale,
            time_to_change_tyre_default: float("TIME_TO_CHANGE_TYRE") * scale,
            time_to_position_default: float("TIME_TO_POSITION") * scale,
            percentage_begin_un_screw: total("BEGIN_UNSCREW_FRAME"),
            percentage_un_screw: total("UNSCREW_FRAME"),
            percentage_end_un_screw: total("END_UNSCREW_FRAME"),
            percentage_begin_screw: total("BEGIN_SCREW_FRAME"),
            percentage_screw: total("SCREW_FRAME"),
            percentage_end_screw: total("END_SCREW_FRAME"),
            loop_speed_mult: float("LOOP_SPEED_MULT"),
        }
    }
}

/// The nodes of one mechanic.
pub struct CrewNodes {
    pub player: AnimationPlayer,
    pub model: NodeId,
    pub old_tyre_null: NodeId,
    pub new_tyre_null: NodeId,
    pub new_tyre_model: NodeId,
    pub default_position: NodeId,
    pub car_wheels: Vec<NodeId>,
}

impl CrewMember {
    fn new(scene: &Scene, nodes: CrewNodes, t: &CrewTimeline) -> CrewMember {
        let tyre_old_transforms = nodes.car_wheels.iter().map(|n| scene.nodes[*n].matrix).collect();
        CrewMember {
            player: nodes.player,
            model: nodes.model,
            old_tyre_null: nodes.old_tyre_null,
            new_tyre_null: nodes.new_tyre_null,
            new_tyre_model: nodes.new_tyre_model,
            default_position: nodes.default_position,
            car_wheels: nodes.car_wheels,
            tyre_old_transforms,
            state: 0,
            time_to_position: 0.0,
            time_to_change_tyre: 0.0,
            time_to_finish: 0.0,
            time_screw: 0.0,
            timer: 0.0,
            loop_speed_mult: t.loop_speed_mult,
            percentage_begin_un_screw: t.percentage_begin_un_screw,
            percentage_un_screw: t.percentage_un_screw,
            percentage_end_un_screw: t.percentage_end_un_screw,
            percentage_begin_screw: t.percentage_begin_screw,
            percentage_screw: t.percentage_screw,
            percentage_end_screw: t.percentage_end_screw,
            time_to_position_default: t.time_to_position_default,
            time_to_change_tyre_default: t.time_to_change_tyre_default,
            time_to_finish_default: t.time_to_finish_default,
        }
    }

    /// `CrewMember::randomizeTimes` 0x1400a4390: three numbers of the picture's `rand()`.
    fn randomize_times(&mut self, rand: &mut dyn FnMut() -> i32) {
        let k = f32::from_bits(0x3800_0100);
        self.time_to_position = ((rand() as f32) * k) * 0.5 + self.time_to_position_default;
        self.time_to_change_tyre = (rand() as f32) * k + self.time_to_change_tyre_default;
        self.time_to_finish = ((rand() as f32) * k) * 0.5 + self.time_to_finish_default;
    }
}

/// The names of the car's wheel nodes, by wheel (`wheels` 0x140004590).
pub const CREW_WHEELS: [&str; 4] = ["WHEEL_LF", "WHEEL_RF", "WHEEL_LR", "WHEEL_RR"];

/// The unit of a vector (the squares summed as `(y*y + x*x) + z*z`, `sqrtss`, `ucomiss` with
/// zero): left alone when its length is zero (or not a number).
fn unit(x: &mut f32, y: &mut f32, z: &mut f32) {
    let l = sqrtf((*y * *y + *x * *x) + *z * *z);
    if !(l == 0.0 || l.is_nan()) {
        let inv = 1.0 / l;
        *x *= inv;
        *y *= inv;
        *z *= inv;
    }
}

/// The mirror for the right-hand wheels (the code at 0x1400a3838, 0x1400a497e, 0x1400a4bf8
/// and 0x1400a3c25): the X axis becomes the unit of minus (Y cross Z), the Z axis is turned
/// round; the Y axis, the position and the fourth column stay.
pub fn crew_flip(w: &Mat44f) -> Mat44f {
    let (r1, r2) = (w.m[1], w.m[2]);
    let mut x = r2[1] * r1[2] - r2[2] * r1[1];
    let mut y = r2[2] * r1[0] - r2[0] * r1[2];
    let mut z = r2[0] * r1[1] - r2[1] * r1[0];
    unit(&mut x, &mut y, &mut z);
    let mut out = *w;
    out.m[0][0] = x;
    out.m[0][1] = y;
    out.m[0][2] = z;
    out.m[2][0] = -r2[0];
    out.m[2][1] = -r2[1];
    out.m[2][2] = -r2[2];
    out
}

/// `PitCrew`.
pub struct PitCrew {
    /// the node `PIT_CREWS` (a child of the blurred node)
    pub pitcrews: NodeId,
    pub members: Vec<CrewMember>,
    pub pit_stop_on: bool,
    pub old_camera_mode: i32,
    /// the start a camera fade is running for (the functor of the lambda 0x1400a2410)
    waiting: Option<CrewStart>,
}

impl PitCrew {
    /// `PitCrew::PitCrew` 0x1400a0b80, and what `PitStop::PitStop` 0x1400a55d0 does with it:
    /// `PIT_CREWS` and the four new tyres become children of `blurred`. `car_node`: the
    /// car's node; `tyre_change_time_sec`: `car.ini [PIT_STOP] TYRE_CHANGE_TIME_SEC`.
    pub fn new(graphics: &mut Graphics, scene: &mut Scene, blurred: NodeId, car: &CrewCar, car_node: NodeId, tyre_change_time_sec: f32) -> Result<PitCrew, String> {
        let game = crate::model::path_text(&graphics.game_folder);
        let mut io = Kn5Io::new();
        let skin_folder = car.folder.join("skins").join(&car.skin);
        let skin_ini = skin_folder.join("skin.ini");
        let ini = if skin_ini.is_file() { IniReader::load(&skin_ini).ok().filter(|ini| ini.has_section("CREW")) } else { None };
        match &ini {
            Some(ini) => {
                for key in ["SUIT", "HELMET", "BRAND"] {
                    load_crew_skin(&mut io, &game, ini, key);
                }
            }
            None => io.skin_override_path.push(format!("{}/skins/{}", car.folder_text, car.skin)),
        }
        let pitcrews = scene.node("PIT_CREWS");
        let objects = graphics.game_folder.join("content/objects3D");
        let ini_file = objects.join("pitcrewtyre.ini");
        if !ini_file.is_file() {
            return Err("ERROR! Problems reading ini pitcrewtyre file".to_string());
        }
        let ini = IniReader::load(&ini_file)?;
        let mut members = Vec::new();
        for wheel in CREW_WHEELS {
            let timeline = CrewTimeline::read(&ini, tyre_change_time_sec);
            let filename = format!("{game}/content/objects3D/pitcrewtyre.kn5");
            let model = io.load(graphics, scene, &filename, &objects.join("pitcrewtyre.kn5"))?;
            scene.nodes[model].is_active = false;
            let animation = Animation::load(&objects.join("pitcrewtyre.ksanim"));
            let player = AnimationPlayer::new(&animation, scene, model);
            let find = |scene: &Scene, name: &str| scene.find_child_by_name(model, name, true).ok_or(format!("pitcrewtyre.kn5 has no node {name}"));
            let old_tyre_null = find(scene, "CREW:TIRE_OLD_Center")?;
            let new_tyre_null = find(scene, "CREW:TIRE_NEW_Center")?;
            let default_position = find(scene, "Position")?;
            let mut car_wheels = Vec::new();
            scene.find_children_by_name(car_node, wheel, &mut car_wheels);
            let nodes_so_far = CrewNodes { player, model, old_tyre_null, new_tyre_null, new_tyre_model: model, default_position, car_wheels };
            let mut member = CrewMember::new(scene, nodes_so_far, &timeline);
            scene.compile(graphics, model);
            scene.add_child(pitcrews, model);
            // the new tyre: a copy of the car's wheel, without its blurred rim
            let source = scene.find_child_by_name(car_node, wheel, true).ok_or(format!("the car has no node {wheel}"))?;
            let new_tyre_model = scene.clone_graph(source);
            scene.nodes[new_tyre_model].matrix = scene.get_world_matrix(new_tyre_null);
            scene.visit(
                new_tyre_model,
                &mut |scene, n| {
                    let name: Vec<u16> = scene.nodes[n].name.encode_utf16().take(8).collect();
                    scene.nodes[n].is_active = String::from_utf16_lossy(&name) != "RIM_BLUR";
                    true
                },
                &mut |_, _| {},
            );
            scene.nodes[new_tyre_model].is_active = false;
            scene.compile(graphics, new_tyre_model);
            scene.add_child(blurred, new_tyre_model);
            member.new_tyre_model = new_tyre_model;
            members.push(member);
        }
        scene.add_child(blurred, pitcrews);
        Ok(PitCrew { pitcrews, members, pit_stop_on: false, old_camera_mode: 0, waiting: None })
    }

    /// A crew on nodes someone else made (the sound's proofs, which have no picture): the
    /// same timeline on four mechanics.
    pub fn on_nodes(scene: &mut Scene, nodes: Vec<CrewNodes>, timeline: &CrewTimeline) -> PitCrew {
        let pitcrews = scene.node("PIT_CREWS");
        let members = nodes.into_iter().map(|n| CrewMember::new(scene, n, timeline)).collect();
        PitCrew { pitcrews, members, pit_stop_on: false, old_camera_mode: 0, waiting: None }
    }

    /// The handler of `Sim::evOnReplayStatusChanged` (lambda 0x1400a2330): entering a replay
    /// (status 6) hides the crew and puts the car's wheels back.
    pub fn on_replay_status_changed(&mut self, scene: &mut Scene, status: i32) {
        if status != 6 {
            return;
        }
        for m in &self.members {
            scene.nodes[m.model].is_active = false;
            scene.nodes[m.new_tyre_model].is_active = false;
            for (wheel, old) in m.car_wheels.iter().zip(&m.tyre_old_transforms) {
                scene.nodes[*wheel].matrix = *old;
            }
        }
    }

    /// The handler of `PitStop::evPitStopCommit` (lambda 0x1400a2410). `changing_tyres`:
    /// `PitStop::isChangingTyres` 0x1400a9870. The crew comes under a camera fade of 0.25 s.
    pub fn on_pit_stop_commit(&mut self, host: &mut dyn CrewHost, start: &CrewStart, changing_tyres: bool) {
        if changing_tyres {
            self.waiting = Some(*start);
            host.fade_in(0.25);
        }
    }

    /// The fade the commit asked for has reached its black frame (`_Do_call` 0x1400a27a0).
    pub fn on_fade_in_finished(&mut self, scene: &mut Scene, host: &mut dyn CrewHost, rand: &mut dyn FnMut() -> i32, i: &CrewInputs) {
        if let Some(start) = self.waiting.take() {
            self.on_pit_stop_begin(scene, host, rand, i, &start);
        }
    }

    /// The handler of `PitStop::evPitStopOver` (lambda 0x1400a27c0).
    pub fn on_pit_stop_over(&mut self, host: &mut dyn CrewHost) {
        if self.pit_stop_on && !host.is_vr_connected() {
            host.set_camera_mode(self.old_camera_mode, true, false);
        }
    }

    /// `PitCrew::pitCrewUpdate` 0x1400a3f20, the handler of `Game::evOnPostUpdate`.
    pub fn pit_crew_update(&mut self, scene: &mut Scene, host: &mut dyn CrewHost, dt: f32, i: &CrewInputs) {
        if i.replay {
            return;
        }
        if i.pause_menu || !(1..=3).contains(&i.session_type) {
            return;
        }
        match i.pit.state {
            // (0: `onPitStopIdle` 0x1400a3e80 does nothing)
            0 | 5 => {
                if self.pit_stop_on {
                    self.reset(scene, host);
                    host.set_camera_mode(self.old_camera_mode, true, false);
                }
            }
            3 => {
                if self.pit_stop_on {
                    self.update_current_position(scene, host, dt, i);
                }
            }
            4 => {
                if self.pit_stop_on {
                    self.on_pit_stop_finished(scene, host);
                }
            }
            _ => {}
        }
    }

    /// `PitCrew::onPitStopBegin` 0x1400a33e0.
    #[allow(clippy::neg_cmp_op_on_partial_ord)] // `comiss; jae`: not a number counts as below zero
    pub fn on_pit_stop_begin(&mut self, scene: &mut Scene, host: &mut dyn CrewHost, rand: &mut dyn FnMut() -> i32, i: &CrewInputs, s: &CrewStart) {
        println!("Starting Pit animation");
        self.pit_stop_on = true;
        self.old_camera_mode = host.camera_mode();
        if !host.is_vr_connected() && s.force_change_camera {
            host.set_camera_mode(5, false, false);
            host.force_theta_phi(f32::from_bits(0x3dcc_cccd), f32::from_bits(0x3dcc_cccd), 5.0);
            host.lock_camera(true);
        }
        for m in &mut self.members {
            m.timer = 0.0;
            m.randomize_times(rand);
            let mut x = ((s.tyre_change_time - m.time_to_position) - m.time_to_change_tyre) - m.time_to_finish;
            if x > 9999.0 {
                x = 9999.0;
            } else if !(x >= 0.0) {
                x = 0.0;
            }
            m.state = 1;
            m.time_screw = x * 0.5;
        }
        for (k, m) in self.members.iter_mut().enumerate() {
            for (wheel, old) in m.car_wheels.iter().zip(m.tyre_old_transforms.iter_mut()) {
                *old = scene.nodes[*wheel].matrix;
            }
            scene.nodes[m.model].matrix = Mat44f::IDENTITY;
            let p = scene.get_world_matrix(m.default_position).m[3];
            let q = scene.get_world_matrix(m.model).m[3];
            let (dx, dy, dz) = (q[0] - p[0], q[1] - p[1], q[2] - p[2]);
            let sum = (dy * dy + dx * dx) + dz * dz;
            let d = if sum == 0.0 || sum.is_nan() { 0.0 } else { sqrtf(sum) };
            let a = i.tyre_matrix[k].m[0];
            let cp = i.tyre_contact_point[k];
            // the horizontal unit of the axle (used for the right-hand wheels only)
            let (mut bx, mut bz) = (a[0], a[2]);
            let mut by = 0.0f32;
            let len = sqrtf(bz * bz + bx * bx);
            if !(len == 0.0 || len.is_nan()) {
                let inv = 1.0 / len;
                bx *= inv;
                bz *= inv;
                by = inv * 0.0;
            }
            // (the mechanic looks at the wheel from `d` metres out along the axle)
            let (dir, mut x, mut y, mut z);
            if k & 1 == 1 {
                let (cx, cy, cz) = (-bx, -by, -bz);
                y = cz * 0.0 - cx * 0.0;
                z = cx - cy * 0.0;
                x = cy * 0.0 - cz;
                dir = [cx, cy, cz];
            } else {
                let (ax, ay, az) = (a[0], a[1], a[2]);
                z = ax - ay * 0.0;
                x = ay * 0.0 - az;
                y = az * 0.0 - ax * 0.0;
                dir = [ax, ay, az];
            }
            unit(&mut x, &mut y, &mut z);
            let model = &mut scene.nodes[m.model].matrix.m;
            model[0][0] = x;
            model[0][1] = y;
            model[0][2] = z;
            model[1][0] = 0.0;
            model[1][1] = 1.0;
            model[1][2] = 0.0;
            for c in 0..3 {
                model[2][c] = -dir[c];
                model[3][c] = dir[c] * d + cp[c];
            }
            let tyre = scene.get_world_matrix(m.new_tyre_null);
            scene.nodes[m.new_tyre_model].matrix = if k & 1 == 1 { crew_flip(&tyre) } else { tyre };
            m.player.set_current_pos(scene, 0.0, true);
            scene.nodes[m.model].is_active = true;
            scene.nodes[m.new_tyre_model].is_active = true;
        }
    }

    /// `PitCrew::updateCurrentPosition` 0x1400a4500: every frame of the stop's work.
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    fn update_current_position(&mut self, scene: &mut Scene, host: &mut dyn CrewHost, dt: f32, i: &CrewInputs) {
        for (k, m) in self.members.iter_mut().enumerate() {
            let mut f = (i.pit.pit_timer - (i.pit.max_pit_timer - i.pit.tyre_time)) / i.pit.tyre_time;
            if f > 1.0 {
                f = 1.0;
            } else if !(f >= 0.0) {
                f = 0.0;
            }
            let mut pos = 1.0 - f;
            m.timer += dt;
            scene.nodes[m.model].is_active = true;
            scene.nodes[m.new_tyre_model].is_active = true;
            let t = m.timer;
            let mut gun: Option<(bool, i32)> = None;
            match m.state {
                1 => {
                    pos = (t / m.time_to_position) * m.percentage_un_screw;
                    if t >= m.time_to_position {
                        m.state = 2;
                        m.timer = 0.0;
                    }
                }
                2 => {
                    pos = rustyac_math::sinf(t * m.loop_speed_mult) * (m.percentage_end_un_screw - m.percentage_begin_un_screw) + m.percentage_un_screw;
                    if t >= m.time_screw {
                        m.timer = 0.0;
                        m.state = 3;
                    }
                    gun = Some((false, 2));
                }
                3 => {
                    let a = m.percentage_un_screw;
                    pos = (m.percentage_screw - a) * (t / m.time_to_change_tyre) + a;
                    if t >= m.time_to_change_tyre {
                        m.state = 4;
                        m.timer = 0.0;
                    }
                }
                4 => {
                    pos = rustyac_math::sinf(t * m.loop_speed_mult) * (m.percentage_end_screw - m.percentage_begin_screw) + m.percentage_screw;
                    if t >= m.time_screw {
                        m.timer = 0.0;
                        m.state = 5;
                    }
                    gun = Some((true, 4));
                }
                5 => pos = (t / m.time_to_finish) * (1.0 - m.percentage_screw) + m.percentage_screw,
                _ => {}
            }
            // the wheel gun, heard for the focused car: started again whenever it has ended
            if let Some((screw, state)) = gun {
                if i.focused && !host.event_is_playing(k, screw) {
                    host.event_start(k, screw, &i.tyre_matrix[k]);
                }
                if !(i.focused && m.state == state) {
                    host.event_stop(k, screw);
                }
            }
            m.player.set_current_pos(scene, pos, false);
            // the car's wheel goes with the old tyre's place in the mechanic's hands
            if k & 1 == 1 {
                let old = crew_flip(&scene.get_world_matrix(m.old_tyre_null));
                for &wheel in &m.car_wheels {
                    let Some(parent) = scene.nodes[wheel].parent else { continue };
                    let inverse = xm_matrix_inverse(&scene.get_world_matrix(parent));
                    scene.nodes[wheel].matrix = xm_matrix_multiply(&old, &inverse);
                }
                scene.nodes[m.new_tyre_model].matrix = crew_flip(&scene.get_world_matrix(m.new_tyre_null));
            } else {
                for &wheel in &m.car_wheels {
                    let old = scene.get_world_matrix(m.old_tyre_null);
                    let Some(parent) = scene.nodes[wheel].parent else { continue };
                    let inverse = xm_matrix_inverse(&scene.get_world_matrix(parent));
                    scene.nodes[wheel].matrix = xm_matrix_multiply(&old, &inverse);
                }
                scene.nodes[m.new_tyre_model].matrix = scene.get_world_matrix(m.new_tyre_null);
            }
        }
    }

    /// `PitCrew::onPitStopFinished` 0x1400a3b50.
    fn on_pit_stop_finished(&mut self, scene: &mut Scene, host: &mut dyn CrewHost) {
        self.pit_stop_on = false;
        host.lock_camera(false);
        for (k, m) in self.members.iter_mut().enumerate() {
            let old = scene.get_world_matrix(m.old_tyre_null);
            scene.nodes[m.new_tyre_model].matrix = if k & 1 == 1 { crew_flip(&old) } else { old };
            for (wheel, old) in m.car_wheels.iter().zip(&m.tyre_old_transforms) {
                scene.nodes[*wheel].matrix = *old;
            }
            m.player.set_current_pos(scene, 1.0, true);
        }
    }

    /// `PitCrew::reset` 0x1400a4420 (also what a new session does, after the pit stop's
    /// state became 5).
    pub fn reset(&mut self, scene: &mut Scene, host: &mut dyn CrewHost) {
        self.on_pit_stop_finished(scene, host);
        for m in &self.members {
            scene.nodes[m.model].is_active = false;
            scene.nodes[m.new_tyre_model].is_active = false;
        }
    }
}
