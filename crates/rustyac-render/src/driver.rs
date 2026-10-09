// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! `DriverModel` (0x180 bytes): the driver. `DriverModel::DriverModel` 0x1400f76d0,
//! `loadDriverSkin` 0x1400fb260, `loadDriverBasePos` 0x1400fb0a0,
//! `addNodeHierarchyToHeadHide` 0x1400fa790, `update` 0x1400fb5d0, `animateHShifter`
//! 0x1400fa8b0, `animatePaddles` 0x1400fac40, `updateHeadMovement` 0x1400fb9a0, `setVisible`
//! 0x1400fb5a0, `setLockedPosition` 0x1400fb580; created by `CarAvatar::initDriver`
//! 0x1400d7380.
//!
//! A car has two: the detailed one (`content/driver/<NAME>.kn5`) and a plain one
//! (`<NAME>_B.kn5`) for far away. `driver_base_pos.knh` of the car seats them; `steer.ksanim`
//! turns their arms with the wheel; `shift.ksanim` (a lever) or `shift_up` / `shift_dw`
//! (paddles) play at a gear change, blended with the steering pose; the head leans with the
//! car's acceleration.

use std::path::Path;

use rustyac_physics::data::ini::IniReader;
use rustyac_physics::vecmath::{xm_matrix_multiply, Mat44f, Vec3f};

use crate::animator::{Animation, AnimationBlender, AnimationPlayer};
use crate::graphics::Graphics;
use crate::model::{path_text, Kn5Io};
use crate::scene::{NodeId, Scene};
use crate::state::CarPhysicsState;

const DEG: f32 = f32::from_bits(0x3c8e_f998);

fn clamp01(x: f32) -> f32 {
    if x > 1.0 {
        1.0
    } else if x >= 0.0 {
        x
    } else {
        0.0
    }
}

/// What a driver model reads of the game around it in a frame.
#[derive(Clone, Copy, Debug)]
pub struct DriverFrame {
    /// the frame's time, and the same times the replay's multiplier while a replay plays
    pub dt: f32,
    pub dt_scaled: f32,
    /// the scene camera's position
    pub camera_position: [f32; 3],
    /// `ACCameraManager::isCameraOnBoard(car)`
    pub on_board: bool,
    /// `Sim::useProView`
    pub use_pro_view: bool,
    /// `CarAvatar::getGraphicSteerDeg`
    pub graphic_steer_deg: f32,
    /// `CarPhysicsInfo::maxGear`
    pub max_gear: i32,
}

pub struct DriverModel {
    pub driver_root: NodeId,
    pub lock_animation: bool,
    pub max_g: f32,
    pub roll_max_deg: f32,
    pub pitch_max_deg: f32,
    pub yaw_max_deg: f32,
    pub filter: f32,
    pub driver_ini_version: u16,
    pub is_hr: bool,
    pub animation_enabled: bool,
    pub paddle_shifts: bool,
    pub shift_hands_inverted: bool,
    pub debug_hide_head_mode: bool,
    pub paddle_shift_status: i32,
    pub shift_animation_status: i32,
    pub steer_player: Option<AnimationPlayer>,
    pub shift_player: Option<AnimationPlayer>,
    pub shift_down_player: Option<AnimationPlayer>,
    shift_blender: AnimationBlender,
    pub force_hidden: bool,
    neck_node: Option<NodeId>,
    next_rest_matrix: Mat44f,
    local_acc: [f32; 3],
    head_hide_nodes: Vec<NodeId>,
    head_animation_time: f32,
    head_animation_mult: f32,
    animation_lock: f32,
    shift_blend_time: f32,
    /// blend, positive, static, negative: 1000 / milliseconds
    shift_timings: [f32; 4],
    pre_load_shift: bool,
    pre_load_rpm: i32,
    last_gear: i32,
    gear_change_timer: f64,
    gear_timer: f64,
}

/// `DriverModel::loadDriverSkin` 0x1400fb260: the folders of a part's colour and of its
/// normal maps, where they exist.
fn load_driver_skin(kn5: &mut Kn5Io, game: &str, ini: &IniReader, model: &str, part: &str) {
    if !ini.has_key(model, part) {
        return;
    }
    let value = ini.get_string(model, part);
    let path = format!("{game}/content/texture/driver_{part}{value}");
    if Path::new(&path).is_dir() {
        kn5.skin_override_path.push(path.clone());
    }
    if let Some(at) = path.rfind('\\') {
        let nm = format!("{}_nm", &path[..at + 1]);
        if Path::new(&nm).is_dir() {
            kn5.skin_override_path.push(nm);
        }
    }
}

/// `loadDriverBasePos` 0x1400fb0a0: one record of `driver_base_pos.knh` and its children.
fn load_driver_base_pos(bytes: &[u8], at: &mut usize, scene: &mut Scene, root: NodeId, is_hr: bool) -> Option<()> {
    let u32_at = |at: usize| bytes.get(at..at + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]));
    let name_len = u32_at(*at)? as usize;
    *at += 4;
    let mut name = String::from_utf8_lossy(bytes.get(*at..*at + name_len)?).into_owned();
    *at += name_len;
    if !is_hr && name.starts_with("MODEL:") && name.len() >= 4 {
        let split = name.len() - 4;
        if name.is_char_boundary(split) {
            name.insert_str(split, "_B");
        }
    }
    let node = scene.find_child_by_name(root, &name, true);
    let block = bytes.get(*at..*at + 64)?;
    *at += 64;
    if let Some(node) = node {
        let f = |k: usize| f32::from_le_bytes([block[k * 4], block[k * 4 + 1], block[k * 4 + 2], block[k * 4 + 3]]);
        scene.nodes[node].matrix = Mat44f { m: std::array::from_fn(|r| std::array::from_fn(|c| f(r * 4 + c))) };
    }
    let children = u32_at(*at)?;
    *at += 4;
    for _ in 0..children {
        load_driver_base_pos(bytes, at, scene, root, is_hr)?;
    }
    Some(())
}

/// The node the constructor puts between a hand and its fingers.
fn insert_fingers(scene: &mut Scene, hand: NodeId, name: &str) -> NodeId {
    let fingers = scene.node(name);
    let children = std::mem::take(&mut scene.nodes[hand].children);
    for &c in &children {
        scene.nodes[c].parent = Some(fingers);
    }
    scene.nodes[fingers].children = children;
    scene.add_child(hand, fingers);
    fingers
}

impl DriverModel {
    /// `DriverModel::DriverModel` 0x1400f76d0. `folder_text` / `folder`: the car's folder;
    /// `guid`: 0 for the player's car. `None`: the car has no `driver3d.ini` it can read.
    pub fn new(graphics: &mut Graphics, scene: &mut Scene, folder_text: &str, folder: &Path, skin: &str, guid: i32, is_hr: bool) -> Result<Option<DriverModel>, String> {
        let game = path_text(&graphics.game_folder);
        let mut gear_change_timer = f64::from_bits(0x3fd3_3333_3333_3333);
        if let Ok(ini) = IniReader::load(&folder.join("data/drivetrain.ini")) {
            gear_change_timer = (ini.get_float("GEARBOX", "CHANGE_DN_TIME").unwrap_or(0.0) * f32::from_bits(0x3a83_126f)) as f64;
        }
        let Ok(ini) = IniReader::load(&folder.join("data/driver3d.ini")) else {
            return Ok(None);
        };
        let mut driver_ini_version = 1u16;
        if ini.has_key("HEADER", "VERSION") {
            driver_ini_version = ini.get_int("HEADER", "VERSION").unwrap_or(0) as u16;
        }
        let shift_hands_inverted = ini.get_int("SHIFT_ANIMATION", "INVERT_SHIFTING_HANDS").unwrap_or(0) > 0;
        let mut head_animation_mult = 5.0f32;
        if ini.has_section("HEAD_ANIMATION") {
            head_animation_mult = ini.get_float("HEAD_ANIMATION", "SPEED").unwrap_or(0.0);
        }
        // the model
        let knh = folder.join("driver_base_pos.knh");
        if !knh.is_file() {
            return Err(format!("{} is missing: the game cannot seat a driver without it", knh.display()));
        }
        println!("USING DEFAULT DRIVER SYSTEM");
        let model_name = ini.get_string("MODEL", "NAME");
        let mut is_hr = is_hr;
        let mut file = format!("{game}/content/driver/{model_name}{}.kn5", if is_hr { "" } else { "_B" });
        if !Path::new(&file).is_file() {
            if !is_hr {
                is_hr = true;
                file = format!("{game}/content/driver/{model_name}.kn5");
            } else {
                println!("Error: driver file '{file}' not found");
            }
        }
        let mut kn5 = Kn5Io::new();
        let mut skins_handled = false;
        let mut need_skin_folder = true;
        if guid == 0 {
            if let Ok(skin_ini) = IniReader::load(&graphics.documents_folder.join("Assetto Corsa/cfg/driverskin.ini")) {
                if skin_ini.has_section(&model_name) {
                    for part in ["SUIT", "GLOVES", "HELMET"] {
                        load_driver_skin(&mut kn5, &game, &skin_ini, &model_name, part);
                    }
                    skins_handled = true;
                    need_skin_folder = false;
                }
            }
        }
        let skin_folder = format!("{folder_text}/skins/{skin}");
        if !skins_handled && folder.join("skins").join(skin).join("skin.ini").is_file() {
            if let Ok(skin_ini) = IniReader::load(&folder.join("skins").join(skin).join("skin.ini")) {
                if skin_ini.has_section(&model_name) {
                    for part in ["SUIT", "GLOVES", "HELMET"] {
                        load_driver_skin(&mut kn5, &game, &skin_ini, &model_name, part);
                    }
                    need_skin_folder = false;
                }
            }
        }
        if need_skin_folder {
            kn5.skin_override_path.push(skin_folder);
        }
        let driver_root = kn5.load(graphics, scene, &file, Path::new(&file))?;
        scene.compile(graphics, driver_root);
        if let Ok(bytes) = std::fs::read(&knh) {
            let mut at = 0usize;
            let _ = load_driver_base_pos(&bytes, &mut at, scene, driver_root, is_hr);
        }

        let mut driver = DriverModel {
            driver_root,
            lock_animation: false,
            max_g: 1.0,
            roll_max_deg: -12.0,
            pitch_max_deg: -10.0,
            yaw_max_deg: 20.0,
            filter: f32::from_bits(0x3e19_999a),
            driver_ini_version,
            is_hr,
            animation_enabled: true,
            paddle_shifts: false,
            shift_hands_inverted,
            debug_hide_head_mode: false,
            paddle_shift_status: 0,
            shift_animation_status: 0,
            steer_player: None,
            shift_player: None,
            shift_down_player: None,
            shift_blender: AnimationBlender::default(),
            force_hidden: false,
            neck_node: None,
            next_rest_matrix: Mat44f { m: [[0.0; 4]; 4] },
            local_acc: [0.0; 3],
            head_hide_nodes: Vec::new(),
            head_animation_time: 0.0,
            head_animation_mult,
            animation_lock: 0.0,
            shift_blend_time: 0.0,
            shift_timings: [0.5, 1.0, 0.5, 1.0],
            pre_load_shift: false,
            pre_load_rpm: 0,
            last_gear: 0,
            gear_change_timer,
            gear_timer: 0.0,
        };

        // the animations, of the car's folder
        let animations = folder.join("animations");
        let steer_path = animations.join("steer.ksanim");
        if steer_path.is_file() {
            let a = Animation::load(&steer_path);
            driver.steer_player = Some(AnimationPlayer::with_mode(&a, scene, driver_root, 0));
            driver.animation_lock = ini.get_float("STEER_ANIMATION", "LOCK").unwrap_or(0.0);
        }
        let mut shift_path = animations.join("shift.ksanim");
        if !shift_path.is_file() {
            shift_path = animations.join("shift_up.ksanim");
            let a = Animation::load(&animations.join("shift_dw.ksanim"));
            let mut player = AnimationPlayer::with_mode(&a, scene, driver_root, 0);
            let (hand, name) = if shift_hands_inverted { ("DRIVER:RIG_HAND_R", "DRIVER:FINGERS_R") } else { ("DRIVER:RIG_HAND_L", "DRIVER:FINGERS_L") };
            let hand = scene.find_child_by_name(driver_root, hand, true).ok_or_else(|| format!("the driver model {file} has no {hand}"))?;
            let fingers = insert_fingers(scene, hand, name);
            driver.shift_blender.add_target_node(scene, fingers, true);
            player.activate_nodes(scene, fingers);
            driver.shift_down_player = Some(player);
            driver.paddle_shifts = true;
        }
        if shift_path.is_file() {
            let a = Animation::load(&shift_path);
            let mut player = AnimationPlayer::with_mode(&a, scene, driver_root, 0);
            if driver.paddle_shifts {
                let (hand, name) = if shift_hands_inverted { ("DRIVER:RIG_HAND_L", "DRIVER:FINGERS_L") } else { ("DRIVER:RIG_HAND_R", "DRIVER:FINGERS_R") };
                let hand = scene.find_child_by_name(driver_root, hand, true).ok_or_else(|| format!("the driver model {file} has no {hand}"))?;
                let fingers = insert_fingers(scene, hand, name);
                driver.shift_blender.add_target_node(scene, fingers, true);
                player.activate_nodes(scene, fingers);
            } else {
                let name = if shift_hands_inverted { "DRIVER:RIG_Clave_L" } else { "DRIVER:RIG_Clave_R" };
                let clave = scene.find_child_by_name(driver_root, name, true).ok_or_else(|| format!("ERROR: COULDN'T FIND {name}"))?;
                driver.shift_blender.add_target_node(scene, clave, true);
                player.activate_nodes(scene, clave);
            }
            driver.shift_player = Some(player);
            driver.pre_load_rpm = ini.get_int("SHIFT_ANIMATION", "PRELOAD_RPM").unwrap_or(0);
            let mult = |key: &str| 1000.0f32 / ini.get_float("SHIFT_ANIMATION", key).unwrap_or(0.0);
            driver.shift_timings = [mult("BLEND_TIME"), mult("POSITIVE_TIME"), mult("STATIC_TIME"), mult("NEGATIVE_TIME")];
            if !driver.shift_timings[0].is_finite() {
                driver.shift_timings[0] = 2.0;
                println!("ERROR: SHIFT ANIMATION BLEND_TIME CANNOT BE 0");
            }
            if !driver.shift_timings[1].is_finite() {
                driver.shift_timings[1] = 1.0;
                println!("ERROR: SHIFT ANIMATION POSITIVE_TIME CANNOT BE 0");
            }
            if !driver.shift_timings[3].is_finite() {
                driver.shift_timings[3] = 1.0;
                println!("ERROR: SHIFT ANIMATION STATIC_TIME CANNOT BE 0");
            }
            if !driver.shift_timings[2].is_finite() {
                driver.shift_timings[2] = 1.0;
                println!("ERROR: SHIFT ANIMATION NEGATIVE_TIME CANNOT BE 0");
            }
        }
        // the neck, as the seating left it
        driver.neck_node = scene.find_child_by_name(driver_root, "DRIVER:RIG_Nek", true);
        if let Some(neck) = driver.neck_node {
            driver.next_rest_matrix = scene.nodes[neck].matrix;
        }
        // what is hidden from the driver's own eyes
        let mut i = 0;
        loop {
            let section = format!("HIDE_OBJECT_{i}");
            if !ini.has_section(&section) {
                break;
            }
            let name = ini.get_string(&section, "NAME");
            if let Some(n) = scene.find_child_by_name(driver_root, &name, true) {
                driver.add_node_hierarchy_to_head_hide(scene, n);
            }
            i += 1;
        }
        let step = f32::from_bits(0x3c88_8889);
        if ini.has_section("HEAD_MOVEMENT") {
            driver.filter = ini.get_float("HEAD_MOVEMENT", "FILTER").unwrap_or(0.0) / step;
            driver.pitch_max_deg = ini.get_float("HEAD_MOVEMENT", "PITCH_MAX_DEG").unwrap_or(0.0);
            driver.yaw_max_deg = ini.get_float("HEAD_MOVEMENT", "YAW_MAX_DEG").unwrap_or(0.0);
            driver.roll_max_deg = ini.get_float("HEAD_MOVEMENT", "ROLL_MAX_DEG").unwrap_or(0.0);
            driver.max_g = ini.get_float("HEAD_MOVEMENT", "MAX_G").unwrap_or(0.0);
        } else {
            driver.filter /= step;
        }
        if driver.driver_ini_version < 2 {
            let k = f32::from_bits(0x3f2a_aaab);
            driver.pitch_max_deg *= k;
            driver.yaw_max_deg *= k;
            driver.roll_max_deg *= k;
        }
        // system/cfg/assetto_corsa.ini [DRIVER] HIDE
        if let Ok(ac) = IniReader::load(&graphics.game_folder.join("system/cfg/assetto_corsa.ini")) {
            if ac.has_section("DRIVER") {
                let hide = ac.get_int("DRIVER", "HIDE").unwrap_or(0) != 0;
                scene.nodes[driver_root].is_active = !hide;
                driver.force_hidden = !scene.nodes[driver_root].is_active;
            }
        }
        Ok(Some(driver))
    }

    /// `addNodeHierarchyToHeadHide` 0x1400fa790: the drawable nodes of a branch.
    fn add_node_hierarchy_to_head_hide(&mut self, scene: &Scene, n: NodeId) {
        if scene.nodes[n].renderable.is_some() {
            self.head_hide_nodes.push(n);
        }
        for &child in &scene.nodes[n].children {
            self.add_node_hierarchy_to_head_hide(scene, child);
        }
    }

    /// `DriverModel::setVisible` 0x1400fb5a0.
    pub fn set_visible(&self, scene: &mut Scene, visible: bool) {
        scene.nodes[self.driver_root].is_active = visible && !self.force_hidden;
    }

    /// `DriverModel::setLockedPosition` 0x1400fb580.
    pub fn set_locked_position(&mut self, scene: &mut Scene) {
        if let Some(player) = &mut self.steer_player {
            player.set_current_pos(scene, 0.5, false);
        }
    }

    /// `DriverModel::update` 0x1400fb5d0.
    pub fn update(&mut self, scene: &mut Scene, state: &CarPhysicsState, frame: &DriverFrame) {
        let dt_raw = frame.dt;
        let dt_scaled = if self.animation_enabled { frame.dt_scaled } else { 0.0 };
        let w = &state.world_matrix.m;
        let (dx, dy, dz) = (w[3][0] - frame.camera_position[0], w[3][1] - frame.camera_position[1], w[3][2] - frame.camera_position[2]);
        let d2 = (dy * dy + dx * dx) + dz * dz;
        if d2 > 40000.0 {
            return;
        }
        if let Some(player) = &mut self.steer_player {
            if !self.lock_animation {
                let k = -1.0f32 / self.animation_lock;
                let mut x = frame.graphic_steer_deg * k;
                if !(x >= -1.0 && x <= 1.0) {
                    // fmodf: exact, so the language's remainder is the C runtime's
                    x %= 1.0;
                }
                player.set_current_pos(scene, (x + 1.0) * 0.5, false);
            } else if player.get_current_pos() != 0.0 {
                player.set_current_pos(scene, 0.5, false);
            }
        }
        if self.shift_player.is_some() && !self.lock_animation {
            let g = state.gear;
            let lg = self.last_gear;
            let mut trigger = false;
            if self.paddle_shifts {
                if lg != g && lg == 2 && g == 1 {
                    if self.gear_change_timer < self.gear_timer {
                        trigger = true;
                        self.gear_timer = 0.0;
                    }
                    self.gear_timer += dt_raw as f64;
                } else if lg < 2 {
                    self.gear_timer = 0.0;
                    trigger = lg != g;
                } else {
                    self.gear_timer = 0.0;
                    trigger = !(lg == g || g == 1);
                }
            } else {
                trigger = lg != g;
            }
            if trigger {
                if self.shift_animation_status == 0 && g <= frame.max_gear + 1 && g >= 0 {
                    self.gear_timer = 0.0;
                    self.paddle_shift_status = if g <= lg { 2 } else { 1 };
                    self.shift_animation_status = 1;
                    self.shift_blend_time = 0.0;
                }
                self.last_gear = g;
                self.pre_load_shift = false;
            }
            if self.paddle_shifts {
                self.animate_paddles(scene, dt_scaled);
            } else {
                self.animate_h_shifter(scene, state, frame, dt_scaled);
            }
        }
        let head_visible = !frame.on_board && !self.debug_hide_head_mode;
        for &n in &self.head_hide_nodes {
            if let Some(r) = &mut scene.nodes[n].renderable {
                r.is_visible = head_visible;
            }
        }
        if frame.use_pro_view {
            let children = scene.nodes[self.driver_root].children.clone();
            for c in children {
                scene.nodes[c].is_active = head_visible;
            }
        }
        if self.neck_node.is_some() && !self.lock_animation {
            self.update_head_movement(scene, dt_raw, dt_scaled, &state.acc_g);
        }
    }

    fn blend(&self, scene: &mut Scene, which: u8, w: f32) {
        let other = if which == 2 { &self.shift_down_player } else { &self.shift_player };
        if let (Some(steer), Some(other)) = (&self.steer_player, other) {
            self.shift_blender.blend_animations(scene, steer, other, w);
        }
    }

    /// `DriverModel::animateHShifter` 0x1400fa8b0.
    fn animate_h_shifter(&mut self, scene: &mut Scene, state: &CarPhysicsState, frame: &DriverFrame, dt: f32) {
        let mut pre = self.pre_load_rpm as f32;
        let mut thr = f32::from_bits(0x3e4c_cccd);
        let g = state.gear;
        if g == 1 && state.speed * f32::from_bits(0x4066_6666) < 5.0 {
            pre *= 0.5;
            thr = f32::from_bits(0x3c23_d70a);
        }
        let rpm = state.engine_rpm;
        if rpm > pre && g < frame.max_gear + 1 && self.shift_animation_status == 0 && thr < state.gas {
            self.paddle_shift_status = 1;
            self.shift_animation_status = 1;
            self.shift_blend_time = 0.0;
            self.pre_load_shift = true;
        } else if self.pre_load_shift && (thr > state.gas || !(rpm >= pre * f32::from_bits(0x3f33_3333))) {
            self.pre_load_shift = false;
        }
        let [blend_mult, positive_mult, static_mult, negative_mult] = self.shift_timings;
        let mut t = self.shift_blend_time;
        let pos = |me: &mut DriverModel, scene: &mut Scene, p: f32, force: bool| {
            if let Some(player) = &mut me.shift_player {
                player.set_current_pos(scene, p, force);
            }
        };
        match self.shift_animation_status {
            0 => {
                pos(self, scene, 0.0, false);
                self.blend(scene, 1, 0.0);
                t = 0.0;
            }
            1 => {
                t = (dt * blend_mult) + t;
                self.shift_blend_time = t;
                pos(self, scene, 0.0, false);
                self.blend(scene, 1, clamp01(t));
                if !(1.0 > t) {
                    self.shift_animation_status = 2;
                    t = 0.0;
                }
            }
            2 => {
                t = (dt * positive_mult) + t;
                self.shift_blend_time = t;
                pos(self, scene, clamp01(t), false);
                if !(1.0 > t) {
                    self.shift_animation_status = 3;
                    t = 0.0;
                }
            }
            3 => {
                t = (dt * static_mult) + t;
                self.shift_blend_time = t;
                pos(self, scene, 1.0, true);
                if !(1.0 > t) && !self.pre_load_shift {
                    self.shift_animation_status = 4;
                    t = 0.0;
                }
            }
            4 => {
                t = (dt * negative_mult) + t;
                self.shift_blend_time = t;
                pos(self, scene, clamp01(1.0 - t), false);
                if !(1.0 > t) {
                    self.shift_animation_status = 5;
                    t = 0.0;
                }
            }
            5 => {
                t = (dt * blend_mult) + t;
                self.shift_blend_time = t;
                pos(self, scene, 0.0, false);
                self.blend(scene, 1, clamp01(1.0 - t));
                if !(1.0 > t) {
                    self.shift_animation_status = 0;
                    t = 0.0;
                }
            }
            _ => return,
        }
        self.shift_blend_time = t;
    }

    /// `DriverModel::animatePaddles` 0x1400fac40.
    fn animate_paddles(&mut self, scene: &mut Scene, dt: f32) {
        let which = self.paddle_shift_status;
        let [blend_mult, positive_mult, static_mult, _] = self.shift_timings;
        let mut t = self.shift_blend_time;
        let pos = |me: &mut DriverModel, scene: &mut Scene, which: i32, p: f32, force: bool| {
            let player = match which {
                1 => &mut me.shift_player,
                2 => &mut me.shift_down_player,
                _ => return,
            };
            if let Some(player) = player {
                player.set_current_pos(scene, p, force);
            }
        };
        let playing = which == 1 || which == 2;
        match self.shift_animation_status {
            0 => {
                pos(self, scene, 1, 0.0, false);
                self.blend(scene, 1, 0.0);
                pos(self, scene, 2, 0.0, false);
                self.blend(scene, 2, 0.0);
                t = 0.0;
            }
            1 => {
                t = (dt * blend_mult) + t;
                self.shift_blend_time = t;
                if playing {
                    pos(self, scene, which, 0.0, false);
                    self.blend(scene, which as u8, clamp01(t));
                }
                if !(1.0 >= t) {
                    t = 0.0;
                    self.shift_animation_status = 2;
                }
            }
            2 => {
                t = (dt * positive_mult) + t;
                self.shift_blend_time = t;
                if playing {
                    pos(self, scene, which, clamp01(t), false);
                }
                if !(1.0 >= t) {
                    t = 0.0;
                    self.shift_animation_status = 3;
                }
            }
            3 => {
                t = (dt * static_mult) + t;
                self.shift_blend_time = t;
                if playing {
                    pos(self, scene, which, 1.0, true);
                }
                if !(1.0 >= t) && !self.pre_load_shift {
                    t = 0.0;
                    self.shift_animation_status = 5;
                }
            }
            5 => {
                t = (dt * blend_mult) + t;
                self.shift_blend_time = t;
                if playing {
                    pos(self, scene, which, 0.0, false);
                    self.blend(scene, which as u8, clamp01(1.0 - t));
                }
                if !(1.0 >= t) {
                    t = 0.0;
                    self.shift_animation_status = 0;
                }
            }
            _ => return,
        }
        self.shift_blend_time = t;
    }

    /// `DriverModel::updateHeadMovement` 0x1400fb9a0.
    fn update_head_movement(&mut self, scene: &mut Scene, dt_raw: f32, dt_scaled: f32, acc_g: &[f32; 3]) {
        let Some(neck) = self.neck_node else {
            return;
        };
        let f = clamp01(self.filter * dt_raw);
        for k in 0..3 {
            self.local_acc[k] = ((acc_g[k] - self.local_acc[k]) * f) + self.local_acc[k];
        }
        let m = self.max_g;
        let limit = |v: f32| {
            if v > m {
                m
            } else if v >= -m {
                v
            } else {
                -m
            }
        };
        let inv = 1.0f32 / m;
        let u = inv * limit(self.local_acc[0]);
        let v = inv * limit(self.local_acc[2]);
        let roll = Mat44f::create_from_axis_angle(&Vec3f::new(0.0, 0.0, 1.0), (u * self.roll_max_deg) * DEG);
        let pitch = Mat44f::create_from_axis_angle(&Vec3f::new(1.0, 0.0, 0.0), (v * self.pitch_max_deg) * DEG);
        let yaw = Mat44f::create_from_axis_angle(&Vec3f::new(0.0, 1.0, 0.0), (u * self.yaw_max_deg) * DEG);
        let step = self.head_animation_mult * dt_scaled;
        let mut t = self.head_animation_time;
        if t.abs() > step {
            let sign = if t > 0.0 {
                1.0
            } else if t < 0.0 {
                -1.0
            } else {
                0.0
            };
            t -= sign * step;
        } else {
            t = 0.0;
        }
        self.head_animation_time = t;
        let head = Mat44f::create_from_axis_angle(&Vec3f::new(0.0, 1.0, 0.0), t);
        let r1 = xm_matrix_multiply(&roll, &pitch);
        let r2 = xm_matrix_multiply(&head, &yaw);
        let r3 = xm_matrix_multiply(&r1, &r2);
        scene.nodes[neck].matrix = xm_matrix_multiply(&r3, &self.next_rest_matrix);
    }
}
