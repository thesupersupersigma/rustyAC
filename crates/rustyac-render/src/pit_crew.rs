// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! The pit lane's people and the pit box marker.
//!
//! `TrackAvatar::initPitCrew` 0x1401ca630: one lollipop man per pit box that has a car
//! (`LollipopCrew` 0xd8 bytes: constructor 0x140112bf0, `loadCrewSkin` 0x1401136d0,
//! `setMeshProperties` 0x140113a10, `update` 0x140113ab0, `updateCurrentPosition`
//! 0x140113b40), and the track's own crew meshes `AC_CREW_<n>…`, switched on for the boxes
//! that have a car and dressed in the car's colours.
//!
//! The marker: `pit_indicator.kn5`, loaded by `PitStop::PitStop` 0x1400a55d0 and shown or
//! hidden by `PitStop::update` 0x1400ab190 and `PitStop::renderHUD` 0x1400aa300 (the parts of
//! them that need no pit stop: those come with the stops themselves).
//!
//! The lollipop men are on layer 5: they only show with `WORLD_DETAIL` 5.

use rustyac_content::vfs::PathExt;
use std::path::{Path, PathBuf};

use rustyac_math::sqrtf;
use rustyac_physics::data::ini::IniReader;
use rustyac_physics::vecmath::{xm_matrix_multiply, Mat44f, Vec3f};

use crate::animator::{Animation, AnimationPlayer};
use crate::graphics::Graphics;
use crate::material::MaterialId;
use crate::model::Kn5Io;
use crate::scene::{NodeId, NodeKind, Scene};

/// `Track::rayCast` straight down from a point, for so many metres: where it hits.
pub type RayDown<'a> = &'a dyn Fn(&[f32; 3], f32) -> Option<[f32; 3]>;

/// What the pit crew reads of a car.
pub struct CrewCar {
    /// the car's folder (`content/cars/<car>`), as text and on disk
    pub folder_text: String,
    pub folder: PathBuf,
    /// `CarAvatar::getCurrentSkin`
    pub skin: String,
    /// `CarAvatar::getGuid`: 0 is the player
    pub guid: i32,
}

/// `LollipopCrew`.
pub struct LollipopCrew {
    pub node: NodeId,
    pub target_lolli_position: f32,
    pub animation_speed: f32,
    player: AnimationPlayer,
    player_idle_up: AnimationPlayer,
    player_idle_dw: AnimationPlayer,
    pub guid: i32,
}

/// `LollipopCrew::loadCrewSkin` 0x1401136d0: the folders of a part's colour and of its normal
/// maps, where they exist.
pub(crate) fn load_crew_skin(io: &mut Kn5Io, game: &str, ini: &IniReader, key: &str) {
    if !ini.has_key("CREW", key) {
        return;
    }
    let value = ini.get_string("CREW", key);
    let path = format!("{game}/content/texture/crew_{key}{value}");
    if Path::new(&path).vfs_is_dir() {
        io.skin_override_path.push(path.clone());
    }
    if let Some(at) = path.rfind('\\') {
        let nm = format!("{}_nm", &path[..at + 1]);
        if Path::new(&nm).vfs_is_dir() {
            io.skin_override_path.push(nm);
        }
    }
}

/// `setMeshProperties` 0x140113a10: everything drawable below the node is on layer 5 and seen
/// up to 100 m.
fn set_mesh_properties(scene: &mut Scene, node: NodeId) {
    if let Some(r) = &mut scene.nodes[node].renderable {
        r.layer = 5;
        r.lod_in = 0.0;
        r.lod_out = 100.0;
    }
    let children = scene.nodes[node].children.clone();
    for child in children {
        set_mesh_properties(scene, child);
    }
}

impl LollipopCrew {
    /// `LollipopCrew::LollipopCrew` 0x140112bf0. `matrix`: the pit box's node matrix (after it
    /// was put on the road); `ray(from, down for 10 m)`: where the track is under a point.
    pub fn new(graphics: &mut Graphics, scene: &mut Scene, car: &CrewCar, matrix: &Mat44f, side: f32, ray: RayDown) -> Result<LollipopCrew, String> {
        let game = crate::model::path_text(&graphics.game_folder);
        let mut io = Kn5Io::new();
        let skin_folder = car.folder.join("skins").join(&car.skin);
        let skin_ini = skin_folder.join("skin.ini");
        let ini = if skin_ini.vfs_is_file() { IniReader::load(&skin_ini).ok().filter(|ini| ini.has_section("CREW")) } else { None };
        match &ini {
            Some(ini) => {
                for key in ["SUIT", "HELMET", "BRAND"] {
                    load_crew_skin(&mut io, &game, ini, key);
                }
            }
            None => io.skin_override_path.push(format!("{}/skins/{}", car.folder_text, car.skin)),
        }
        let stem = if side >= 0.0 { "pitcrew_Left" } else { "pitcrew" };
        let objects = graphics.game_folder.join("content/objects3D");
        let filename = format!("{game}/content/objects3D/{stem}.kn5");
        let node = io.load(graphics, scene, &filename, &objects.join(format!("{stem}.kn5")))?;
        let animation = Animation::load(&objects.join(format!("{stem}.ksanim")));
        let animation_idle_up = Animation::load(&objects.join(format!("{stem}_idle_up.ksanim")));
        let animation_idle_dw = Animation::load(&objects.join(format!("{stem}_idle_dw.ksanim")));
        scene.compile(graphics, node);
        // where he stands: 4 m along the box and 2 m to its side
        let m = matrix.m;
        scene.nodes[node].matrix = *matrix;
        let mut pos = [0.0f32; 3];
        for k in 0..3 {
            let a = (m[0][k] * side) * 2.0;
            let b = (-m[2][k]) * -4.0;
            pos[k] = (m[3][k] + b) + a;
        }
        if let Some(hit) = ray(&[pos[0], pos[1] + 1.0, pos[2]], 10.0) {
            pos[1] = hit[1];
        }
        scene.nodes[node].matrix.m[3][..3].copy_from_slice(&pos);
        // turned round, about his own up axis (the literal is not quite pi)
        let n = scene.nodes[node].matrix;
        let r = Mat44f::create_from_axis_angle(&Vec3f::new(n.m[1][0], n.m[1][1], n.m[1][2]), f32::from_bits(0x4049_0efe));
        scene.nodes[node].matrix = xm_matrix_multiply(&r, &n);
        let player = AnimationPlayer::new(&animation, scene, node);
        let player_idle_up = AnimationPlayer::new(&animation_idle_up, scene, node);
        let player_idle_dw = AnimationPlayer::new(&animation_idle_dw, scene, node);
        set_mesh_properties(scene, node);
        Ok(LollipopCrew { node, target_lolli_position: 1.0, animation_speed: 1.0, player, player_idle_up, player_idle_dw, guid: car.guid })
    }

    /// `LollipopCrew::update` 0x140113ab0. `pit_stop_state`: `PitStop::state` (`None`: there
    /// is no pit stop object); `in_pit`: `CarAvatar::isInPit` of the man's car.
    pub fn update(&mut self, scene: &mut Scene, dt: f32, pit_stop_state: Option<i32>, in_pit: bool) {
        if self.guid == 0 {
            match pit_stop_state {
                Some(1) => self.target_lolli_position = 1.0,
                Some(4) => self.target_lolli_position = 0.0,
                _ => {}
            }
        } else {
            self.target_lolli_position = if in_pit { 1.0 } else { 0.0 };
        }
        // updateCurrentPosition 0x140113b40
        let mut cur = self.player.get_current_pos();
        let target = self.target_lolli_position;
        if cur != target {
            let step = dt * self.animation_speed;
            cur = if cur > target { cur - step } else { cur + step };
            if cur > 1.0 {
                cur = 1.0;
            } else if !(cur >= 0.0) {
                cur = 0.0;
            }
            self.player.set_current_pos(scene, cur, false);
        } else {
            let idle = if cur == 0.0 { &mut self.player_idle_up } else { &mut self.player_idle_dw };
            let mut p = idle.get_current_pos();
            p += dt * f32::from_bits(0x3daa_aaab);
            if p > 1.0 {
                p += -1.0;
            }
            idle.set_current_pos(scene, p, false);
        }
    }
}

/// `CarAvatar::isInPit` 0x1400d8bd0: within 1.5 m of the car's pit box.
pub fn is_in_pit(car_position: &[f32; 3], pit_position: &[f32; 3]) -> bool {
    let (dx, dy, dz) = (car_position[0] - pit_position[0], car_position[1] - pit_position[1], car_position[2] - pit_position[2]);
    let d2 = ((dy * dy) + (dx * dx)) + (dz * dz);
    let d = if d2 == 0.0 { 0.0 } else { sqrtf(d2) };
    d < 1.5
}

/// `TrackAvatar::initPitCrew` 0x1401ca630. `model`: the track's model; `data_folder`: the
/// folder that holds the track's (or its layout's) `data`; `pit_matrices`: the matrices of the
/// `AC_PIT_<n>` nodes as `initRespawnPositionSet` left them (on the road); `cars`: the cars, by
/// pit box. The answer: the lollipop men, by pit box.
pub fn init_pit_crew(graphics: &mut Graphics, scene: &mut Scene, model: NodeId, data_folder: &Path, pit_matrices: &[Mat44f], cars: &[CrewCar], ray: RayDown) -> Result<Vec<LollipopCrew>, String> {
    let game = crate::model::path_text(&graphics.game_folder);
    let mut side = -1.0f32;
    let crew_ini = data_folder.join("data/crew.ini");
    if rustyac_physics::data::exists(&crew_ini) {
        if let Ok(ini) = IniReader::load(&crew_ini) {
            side = ini.get_float("HEADER", "SIDE").unwrap_or(0.0);
        }
    }
    let mut crews = Vec::new();
    for (n, matrix) in pit_matrices.iter().enumerate() {
        let Some(car) = cars.get(n) else { continue };
        let crew = LollipopCrew::new(graphics, scene, car, matrix, side, ray)?;
        scene.add_child(model, crew.node);
        crews.push(crew);
    }
    // the track's own crew meshes: on for the boxes that have a car, in the car's colours
    let mut nodes = Vec::new();
    scene.find_children_by_prefix(model, "AC_CREW_", &mut nodes);
    for x in nodes {
        let digits: String = scene.nodes[x].name[8..].chars().take_while(|c| c.is_ascii_digit()).collect();
        // (a name without a number: the game throws; here the node is left as it is)
        let Ok(n) = digits.parse::<usize>() else { continue };
        let Some(car) = cars.get(n) else {
            scene.nodes[x].is_active = false;
            continue;
        };
        scene.nodes[x].is_active = true;
        let material = match &scene.nodes[x].kind {
            NodeKind::Mesh(mesh) => mesh.material,
            NodeKind::SkinnedMesh(mesh) => mesh.material,
            _ => continue,
        };
        let Some(material) = material else { continue };
        let Some(name) = scene.materials[material.0 as usize].resources.first().map(|r| r.texture.file_name.clone()) else { continue };
        // (the game's folder is relative there: "C:/..." here must not be taken for "folder::file")
        let Some(p) = name.find("::") else { continue };
        let mut tex = name.get(p + 2..).unwrap_or("").to_string();
        if tex == "DRIVER_Suit.dds" {
            // (the label of the material the track's meshes share, not what it shows)
            scene.materials[material.0 as usize].resources[0].texture.file_name = format!("{}::DRIVER_Suit2.dds", &name[..p]);
            tex = "DRIVER_Suit2.dds".to_string();
        }
        let skin_folder = car.folder.join("skins").join(&car.skin);
        let mut file = format!("{}/skins/{}/{tex}", car.folder_text, car.skin);
        let skin_ini = skin_folder.join("skin.ini");
        if skin_ini.vfs_is_file() {
            if let Ok(ini) = IniReader::load(&skin_ini) {
                if ini.has_section("CREW") && ini.has_key("CREW", "SUIT") {
                    let folder = format!("{game}/content/texture/crew_SUIT{}", ini.get_string("CREW", "SUIT"));
                    let candidate = format!("{folder}/{tex}");
                    if Path::new(&folder).vfs_is_dir() && Path::new(&candidate).vfs_is_file() {
                        file = candidate;
                    }
                }
            }
        }
        if Path::new(&file).vfs_is_file() {
            let mut clone = scene.materials[material.0 as usize].clone_material(graphics)?;
            let texture = graphics.resources.get_texture(&graphics.kgl, &file);
            clone.resources[0].texture = texture;
            let id = MaterialId(scene.materials.len() as u32);
            scene.materials.push(clone);
            match &mut scene.nodes[x].kind {
                NodeKind::Mesh(mesh) => mesh.material = Some(id),
                NodeKind::SkinnedMesh(mesh) => mesh.material = Some(id),
                _ => {}
            }
        }
    }
    Ok(crews)
}

/// What the marker's rules read every frame.
#[derive(Clone, Copy, Debug)]
pub struct PitInputs {
    /// the car's body matrix, its position (`CarAvatar::bodyMatrix` row 3)
    pub car_position: [f32; 3],
    /// m/s
    pub speed: f32,
    /// `CarAvatar::isInPitlane`: a tyre is on a pit-lane surface
    pub in_pitlane: bool,
    /// 1 practice, 2 qualifying, 3 race; anything else hides the marker
    pub session_type: i32,
    pub pause_menu: bool,
    pub replay: bool,
}

/// The pit box marker (`PitStop::pitIndicator`) and as much of `PitStop` as moves it.
pub struct PitIndicator {
    pub node: NodeId,
    pub pit_position: [f32; 3],
    /// `cfg/gameplay.ini [PIT_INDICATOR] ONLY_IN_PITLINE` (on when the key is missing)
    pub only_in_pitline: bool,
    /// `PitStop::state`: 0 away, 1 in the pit lane, 2 the mouse menu, 3 a stop runs, 4 done
    /// (and at first), 5 reset
    pub state: i32,
    displaying_pit_menu: bool,
}

impl PitIndicator {
    /// The marker's part of `PitStop::PitStop` 0x1400a55d0: the model under the track's node,
    /// at the player's pit box.
    pub fn new(graphics: &mut Graphics, scene: &mut Scene, track_node: NodeId, pit_matrix: &Mat44f, only_in_pitline: bool) -> Result<PitIndicator, String> {
        let game = crate::model::path_text(&graphics.game_folder);
        let node = scene.node("PIT_INDICATOR");
        let mut io = Kn5Io::new();
        let filename = format!("{game}/content/objects3D/pit_indicator.kn5");
        // Model::load 0x140217d30
        io.skin_override_path.push(format!("{}/texture", crate::model::get_path(&filename)));
        let top = io.load(graphics, scene, &filename, &graphics.game_folder.join("content/objects3D/pit_indicator.kn5"))?;
        scene.add_child(node, top);
        scene.compile(graphics, node);
        scene.add_child(track_node, node);
        scene.nodes[node].matrix = *pit_matrix;
        Ok(PitIndicator { node, pit_position: [pit_matrix.m[3][0], pit_matrix.m[3][1], pit_matrix.m[3][2]], only_in_pitline, state: 4, displaying_pit_menu: false })
    }

    /// The rules alone, for a marker whose model someone else holds (`node`: any node of the
    /// scene the rules are run on).
    pub fn rules_only(node: NodeId, pit_position: [f32; 3], only_in_pitline: bool) -> PitIndicator {
        PitIndicator { node, pit_position, only_in_pitline, state: 4, displaying_pit_menu: false }
    }

    /// `PitStop::isInsidePitArea` 0x1400a9af0: within 3 m of the box.
    fn is_inside_pit_area(&self, i: &PitInputs) -> bool {
        let (dx, dy, dz) = (self.pit_position[0] - i.car_position[0], self.pit_position[1] - i.car_position[1], self.pit_position[2] - i.car_position[2]);
        let sum = (dy * dy + dx * dx) + dz * dz;
        let dist = if sum == 0.0 { 0.0 } else { sqrtf(sum) };
        dist < 3.0
    }

    /// `PitStop::isCarOutOfPitArea` 0x1400a9840: leaving the box shows the marker.
    fn is_car_out_of_pit_area(&mut self, scene: &mut Scene, i: &PitInputs) -> bool {
        let out = !self.is_inside_pit_area(i);
        if out {
            scene.nodes[self.node].is_active = true;
        }
        out
    }

    /// `PitStop::isInPitPosition` 0x1400a98c0 (the quick menu's way): standing in the box.
    fn is_in_pit_position(&mut self, i: &PitInputs) -> bool {
        let kmh = i.speed * f32::from_bits(0x4066_6666);
        let old = self.displaying_pit_menu;
        let now = self.is_inside_pit_area(i) && (kmh < 1.5 || old);
        self.displaying_pit_menu = now;
        if now && kmh >= 2.0 {
            self.displaying_pit_menu = false;
        }
        self.displaying_pit_menu
    }

    /// `PitStop::update` 0x1400ab190, before the scene is drawn. No stop is ever asked for
    /// (the quick menu is not there yet): standing in the box ends in state 4 at once, after
    /// the one number of `rand()` that `Car::getPitstopTime` draws.
    pub fn update(&mut self, scene: &mut Scene, rand: &mut dyn FnMut(), i: &PitInputs) {
        if i.replay || !(1..=3).contains(&i.session_type) || i.pause_menu {
            scene.nodes[self.node].is_active = false;
            return;
        }
        match self.state {
            0 => {
                if i.in_pitlane {
                    scene.nodes[self.node].is_active = true;
                    self.state = 1;
                }
            }
            1 | 2 => {
                if self.state == 1 && self.is_in_pit_position(i) {
                    rand();
                    self.state = 4;
                }
                if !self.is_in_pit_position(i) {
                    self.state = 1;
                }
            }
            4 | 5 => {
                scene.nodes[self.node].is_active = false;
                if self.is_car_out_of_pit_area(scene, i) {
                    self.state = 0;
                }
            }
            _ => {}
        }
        if !i.in_pitlane && self.state != 5 {
            self.state = 0;
        }
    }

    /// The marker with the pit stop itself at work (`rustyac_physics::race::PitStop`, which
    /// is `PitStop::update` whole): its state, for the lollipop man, and the visibility it
    /// gave the marker. `rand`: the picture's `rand()`, of which the game's stop draws one
    /// number when it is timed (`Car::getPitstopTime`, as the driver stops in the box): the
    /// stop here has the referee's stream, and the picture's stays in step with the game's.
    pub fn follow(&mut self, scene: &mut Scene, rand: &mut dyn FnMut(), state: i32, visible: bool) {
        if self.state == 1 && (state == 3 || state == 4) {
            rand();
        }
        self.state = state;
        scene.nodes[self.node].is_active = visible;
    }

    /// The marker's part of `PitStop::renderHUD` 0x1400aa300, after the scene is drawn.
    pub fn render_hud(&mut self, scene: &mut Scene, i: &PitInputs) {
        if i.pause_menu || i.replay {
            return;
        }
        if self.only_in_pitline {
            scene.nodes[self.node].is_active = i.in_pitlane;
        }
    }
}
