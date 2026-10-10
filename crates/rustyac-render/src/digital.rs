// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! The dashboard's digital displays: `DigitalInstruments` (0xc8 bytes; constructor
//! 0x1400eb030, `initInstruments` 0x1400eb510, `update` 0x1400f0520), `DigitalItem`
//! (constructor 0x1400f0690, `update` 0x1400f3190), `DigitalLed` (constructors 0x1400f5400 /
//! 0x1400f5460 / 0x1400f6000, `update` 0x1400f6200), `TextNode` (0x14021ad10, `render`
//! 0x14021ae50) and `StringBlitter3D` (0x14020ff70, `blitString` 0x140210110, `blitStringV2`
//! 0x140210980, `getStringWidth` 0x140210c10, `initCoords` 0x140210d60).
//!
//! Text is a bitmap font (`content/fonts/<font>.png` with the glyphs' left edges in
//! `<font>.txt`), one quad per glyph, drawn in the transparent pass with the shader `ksFont`.
//! Shift lights are either meshes whose `ksEmissive` is switched (`LED_n`) or meshes that are
//! shown and hidden (`RPM_SERIE_n` and the other series).
//!
//! Not ported: `DigitalPanels` (two cars), the items drawn by a `DisplayNode` (`RPM_GRAPH`,
//! `DELTA_GRAPH`, `GEAR_TX`), and the item types and LED types listed where they are skipped.

use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;

use rustyac_physics::data::ini::IniReader;

use crate::gl::{GlRenderer, GL_QUADS};
use crate::graphics::{Graphics, BLEND_ALPHA};
use crate::material::{MaterialId, PASS_TRANSPARENT};
use crate::scene::{NodeId, NodeKind, OnNodeRenderEvent, RenderableObject, Scene};
use crate::shader::ShaderId;
use crate::state::CarPhysicsState;
use crate::texture::Texture;

/// `StringBlitter3D` (0x78 bytes).
pub struct StringBlitter3D {
    texture: Texture,
    /// the png's size, pixels
    tex_width: f32,
    size_y: f32,
    /// the left edge of each glyph from the space on, as a share of the png's width
    offsets: Vec<f32>,
    sh_font: Option<ShaderId>,
    scale_const: f32,
    gl: Rc<RefCell<GlRenderer>>,
}

impl StringBlitter3D {
    /// `StringBlitter3D::StringBlitter3D` 0x14020ff70.
    fn new(graphics: &mut Graphics, font: &str, gl: Rc<RefCell<GlRenderer>>) -> StringBlitter3D {
        let game = crate::model::path_text(&graphics.game_folder);
        let texture = graphics.resources.get_texture(&graphics.kgl, &format!("{game}/content/fonts/{font}.png"));
        let (tex_width, size_y) = graphics.resources.size(&texture).map(|(w, h)| (w as f32, h as f32)).unwrap_or((0.0, 0.0));
        // initCoords: 96 lines are asked for; the files have 95, the last read gives 0
        let mut offsets = Vec::with_capacity(96);
        let text = std::fs::read_to_string(graphics.game_folder.join(format!("content/fonts/{font}.txt"))).unwrap_or_default();
        let mut lines = text.lines();
        for _ in 0..96 {
            offsets.push(lines.next().map(|l| l.trim().parse::<f64>().unwrap_or(0.0) as f32).unwrap_or(0.0));
        }
        StringBlitter3D { texture, tex_width, size_y, offsets, sh_font: graphics.shaders.get_shader(&graphics.kgl, "ksFont").ok(), scale_const: f32::from_bits(0x3f4c_cccd), gl }
    }

    fn uvw(&self, c: char) -> Option<(f32, f32)> {
        let idx = (c as u32).wrapping_sub(0x20) as usize;
        Some((*self.offsets.get(idx)?, *self.offsets.get(idx + 1)?))
    }

    /// `StringBlitter3D::getStringWidth` 0x140210c10.
    fn get_string_width(&self, text: &str, scale: f32) -> f32 {
        let mut sum = 0.0f64;
        for c in text.chars() {
            let Some((u0, u1)) = self.uvw(c) else { continue };
            let mut d = self.tex_width as f64;
            d *= (u1 - u0) as f64;
            d *= scale as f64;
            d *= self.scale_const as f64;
            sum += d;
        }
        sum as f32
    }

    /// `blitString` 0x140210110 (version 1) and `blitStringV2` 0x140210980.
    fn blit(&mut self, graphics: &mut Graphics, text: &str, h: f32, align: i32, color: &[f32; 4], version: u16) {
        let n = text.chars().count();
        if n == 0 || n > 160 {
            return;
        }
        if version == 2 {
            self.scale_const = 1.0;
        }
        let k = self.scale_const;
        let w = self.get_string_width(text, h);
        graphics.set_texture(0, &self.texture);
        graphics.set_blend_mode(BLEND_ALPHA);
        let mut gl = self.gl.borrow_mut();
        gl.begin(GL_QUADS, self.sh_font);
        let chars: Vec<char> = text.chars().collect();
        if version == 1 {
            let mut x = match align {
                1 => w,
                2 => w * 0.5,
                _ => 0.0,
            };
            for &c in &chars {
                let Some((u0, u1)) = self.uvw(c) else { continue };
                gl.color4f(color[0], color[1], color[2], color[3]);
                let gw = (u1 - u0) * self.tex_width;
                let gh = h * self.size_y;
                gl.tex_coord2f(u1, 1.0);
                gl.vertex3f(x, 0.0, 0.0);
                gl.tex_coord2f(u1, 0.0);
                gl.vertex3f(x, gh, 0.0);
                let gw2 = gw * h;
                let x2 = gw2 + x;
                gl.tex_coord2f(u0, 0.0);
                gl.vertex3f(x2, gh, 0.0);
                gl.tex_coord2f(u0, 1.0);
                gl.vertex3f(x2, 0.0, 0.0);
                x -= gw2 * k;
            }
        } else {
            let mut x = match align {
                0 => -w,
                2 => -(w * 0.5),
                _ => 0.0,
            };
            for &c in chars.iter().rev() {
                let Some((u0, u1)) = self.uvw(c) else { continue };
                gl.color4f(color[0], color[1], color[2], color[3]);
                let gw = (u1 - u0) * self.tex_width;
                let gh = (h * self.size_y) * k;
                gl.tex_coord2f(u1, 1.0);
                gl.vertex3f(x, 0.0, 0.0);
                gl.tex_coord2f(u1, 0.0);
                gl.vertex3f(x, gh, 0.0);
                let gw2 = gw * h;
                let x2 = (gw2 * k) + x;
                gl.tex_coord2f(u0, 0.0);
                gl.vertex3f(x2, gh, 0.0);
                gl.tex_coord2f(u0, 1.0);
                gl.vertex3f(x2, 0.0, 0.0);
                x += gw2 * k;
            }
        }
        gl.end(graphics);
    }
}

/// `TextNode` (0x128 bytes).
pub struct TextNode {
    node: NodeId,
    pub text: String,
    pub scale: f32,
    pub color: [f32; 4],
    pub align: i32,
    pub version: u16,
    blitter: StringBlitter3D,
}

impl RenderableObject for TextNode {
    /// `TextNode::render` 0x14021ae50.
    fn render(&mut self, scene: &mut Scene, graphics: &mut Graphics, event: &OnNodeRenderEvent) -> bool {
        if event.pass_id == PASS_TRANSPARENT {
            graphics.set_world_matrix(&scene.nodes[self.node].matrix_ws);
            if self.version == 1 || self.version == 2 {
                let (text, color) = (self.text.clone(), self.color);
                self.blitter.blit(graphics, &text, self.scale, self.align, &color, self.version);
            }
        }
        true
    }

    fn resets_material_cache(&self) -> bool {
        true
    }
}

/// The item types that are ported (`DigitalItem::type`).
#[derive(Clone, Copy, Debug, PartialEq)]
enum ItemType {
    Unknown,
    Gear,
    Speed,
    Rpm,
    MaxRpm,
    MaxSpeed,
    Fuel,
    Clock,
    LastLap,
    Perf,
    LapTime,
    TcLevel,
    TurboBoost,
    Pressure,
    AbsLevel,
    WaterTemp,
    AmbientTemp,
    CurrentLap,
    KersCharge,
    PlaceHolder,
    GearColor,
    BestLap,
    TurboLevel,
    TotalLaps,
    EstLaps,
    FuelCons,
    GForces,
    KersLoad,
    PositionCar,
    PositionCount,
    P2pDash,
    FuelPerc,
}

struct DigitalItem {
    kind: ItemType,
    color: [f32; 4],
    color2: [f32; 4],
    color3: [f32; 4],
    /// `rpmGraphMin` / `rpmGraphMax`: also the limits of a `GFORCES` item
    graph_min: f32,
    graph_max: f32,
    add_sign: i32,
    fuel_cons_units: i32,
    text_node: Rc<RefCell<TextNode>>,
    decimals: i32,
    max_rpm: i32,
    max_speed: i32,
    pre_fix: String,
    post_fix: String,
    update_refresh: f32,
    current_refresh: f32,
    speedometer_units: i32,
    tyre_number: i32,
    last_gear: i32,
    time_to_ignore: f32,
    time_to_ignore_base: f32,
}

/// `DigitalLedType`, the ported ones by number.
struct DigitalLed {
    kind: i32,
    fswitch: f32,
    emissive: [f32; 3],
    emissive2: [f32; 3],
    emissive3: [f32; 3],
    blink_frequency: f32,
    blink_switch: f32,
    selected_wing: i32,
    inverted: bool,
    has_show_limit: bool,
    show_min: f32,
    show_max: f32,
    show_excluded: bool,
    g_force: i32,
    g_force_sign: i32,
    var_emissive: Option<(MaterialId, usize)>,
    target_mesh: Option<NodeId>,
}

impl DigitalLed {
    fn plain(kind: i32) -> DigitalLed {
        DigitalLed {
            kind,
            fswitch: -1.0,
            emissive: [0.0; 3],
            emissive2: [0.0; 3],
            emissive3: [0.0; 3],
            blink_frequency: 0.0,
            blink_switch: -1.0,
            selected_wing: 0,
            inverted: false,
            has_show_limit: false,
            show_min: 0.0,
            show_max: 0.0,
            show_excluded: false,
            g_force: 0,
            g_force_sign: 1,
            var_emissive: None,
            target_mesh: None,
        }
    }
}

/// What the displays read beyond the car's physics state.
#[derive(Clone, Copy, Debug, Default)]
pub struct DigitalFrame {
    pub dt: f32,
    /// `Game::gameTime.now`, milliseconds
    pub game_time_ms: f64,
    /// the car is the one the cameras follow
    pub focused: bool,
    /// the pause menu shows
    pub pause_menu: bool,
    /// `CarAvatar::getTCMode().first` and `getABSMode().first`
    pub tc_level: u32,
    pub abs_level: u32,
    /// `PhysicsEngine::ambientTemperature`
    pub ambient_temperature: f32,
    /// `RaceManager::getLapCount(car)`
    pub lap_count: u32,
    /// `LightingSettings::angle` (race.ini `SUN_ANGLE`)
    pub sun_angle: f32,
    /// `Speed::useMPH`
    pub use_mph: bool,
    /// `RaceManager::getCurrentSessionType`, the laps of the session (`getSessionInfo`),
    /// `getCarRealTimePosition` and `getCarLeaderboardPosition` of this car, the cars of the `Sim`
    pub session_type: i32,
    pub session_laps: u32,
    pub real_time_position: i32,
    pub leaderboard_position: i32,
    pub cars_count: i32,
    /// `CarAvatar::getKmPerLiter` 0x1400d3510
    pub km_per_liter: f32,
    /// `CarPhysicsInfo`: `maxFuel`, `kersMaxJ`, `ersMaxJ`, `hasKERS`
    pub max_fuel: f64,
    pub kers_max_j: f32,
    pub ers_max_j: f32,
    pub has_kers: bool,
    /// `CarAvatar::wingsStatus`: the angle of each wing
    pub wing_angles: [f32; 8],
    pub wings_count: usize,
    /// `Car::drivetrain.totalTorque` and `ratio`; `None` without a physics car
    pub drivetrain: Option<(f32, f64)>,
    /// `CarAvatar::currentERSNormalizedRecharge`
    pub ers_recharge: f32,
}

#[derive(Default)]
pub struct DigitalInstruments {
    items: Vec<DigitalItem>,
    leds: Vec<DigitalLed>,
}

const GEARS: [&str; 11] = ["R", "N", "1", "2", "3", "4", "5", "6", "7", "8", "9"];

/// `INIReader::getFloat4`: four numbers with commas between them; what is missing is 0.
fn float4(ini: &IniReader, section: &str, key: &str) -> [f32; 4] {
    let text = ini.get_string(section, key);
    let mut out = [0.0f32; 4];
    for (slot, part) in out.iter_mut().zip(text.split(',')) {
        *slot = part.trim().parse::<f64>().unwrap_or(0.0) as f32;
    }
    out
}

/// `Node::getNodeChild<Mesh>` 0x1400e5d50: the first node of that name that is a mesh (a
/// plain node of the same name is searched through like any other).
pub(crate) fn mesh_node(scene: &Scene, node: NodeId, name: &str) -> Option<NodeId> {
    for &child in &scene.nodes[node].children {
        if scene.nodes[child].name == name && matches!(scene.nodes[child].kind, NodeKind::Mesh(_) | NodeKind::SkinnedMesh(_)) {
            return Some(child);
        }
        if let Some(found) = mesh_node(scene, child, name) {
            return Some(found);
        }
    }
    None
}

fn mesh_material(scene: &Scene, n: NodeId) -> Option<MaterialId> {
    match &scene.nodes[n].kind {
        NodeKind::Mesh(mesh) => mesh.material,
        NodeKind::SkinnedMesh(mesh) => mesh.material,
        _ => None,
    }
}

pub(crate) fn clone_material(graphics: &mut Graphics, scene: &mut Scene, n: NodeId) -> Result<Option<MaterialId>, String> {
    let Some(old) = mesh_material(scene, n) else {
        return Ok(None);
    };
    let clone = scene.materials[old.0 as usize].clone_material(graphics)?;
    let id = MaterialId(scene.materials.len() as u32);
    scene.materials.push(clone);
    match &mut scene.nodes[n].kind {
        NodeKind::Mesh(mesh) => mesh.material = Some(id),
        NodeKind::SkinnedMesh(mesh) => mesh.material = Some(id),
        _ => {}
    }
    Ok(Some(id))
}

/// `GraphicsManager::getLDRColor` 0x140202a70.
fn get_ldr_color(graphics: &Graphics, c: [f32; 3]) -> [f32; 3] {
    if graphics.video.pp_hdr_enabled {
        return c;
    }
    let t = if c[1] > c[2] { c[1] } else { c[2] };
    let m = if c[0] > t { c[0] } else { t };
    if m > 1.0 {
        let s = 1.0 / m;
        [s * c[0], s * c[1], s * c[2]]
    } else {
        c
    }
}

/// `timeToString` 0x140053110.
fn time_to_string(ms: i32, d: i32) -> String {
    let m = ms / 60000;
    let s = (ms - m * 60000) / 1000;
    let r = ms - m * 60000 - s * 1000;
    if ms > 0 {
        match d {
            3 => format!("{m}:{s:02}:{r:03}"),
            2 => format!("{m}:{s:02}:{:02}", r / 10),
            1 => format!("{m}:{s:02}:{:01}", r / 100),
            _ => String::new(),
        }
    } else {
        match d {
            1 => "-:--:-".into(),
            2 => "-:--:--".into(),
            3 => "-:--:---".into(),
            _ => String::new(),
        }
    }
}

/// `timeToSecsString` 0x140053020.
fn time_to_secs_string(ms: i32) -> String {
    let m = ms / 60000;
    let s = (ms - m * 60000) / 1000;
    if ms > 0 {
        format!("{m}:{s:02}")
    } else {
        "-:--".into()
    }
}

/// `timeToDiffString` 0x140099b70.
fn time_to_diff_string(ms: i32, d: i32) -> String {
    let s = ms / 1000;
    let r = ms - s * 1000;
    if ms == 0 {
        return match d {
            1 => "-".into(),
            2 => "--".into(),
            3 => "---".into(),
            _ => String::new(),
        };
    }
    let sign = if ms < 0 { "-" } else { "+" };
    match d {
        1 => format!("{sign}{}.{:01}", s.abs(), (r / 100).abs()),
        2 => format!("{sign}{}.{:02}", s.abs(), (r / 10).abs()),
        3 => format!("{sign}{}.{:03}", s.abs(), r.abs()),
        _ => String::new(),
    }
}

impl DigitalInstruments {
    /// `DigitalInstruments::DigitalInstruments` 0x1400eb030 with `initInstruments` 0x1400eb510.
    pub fn new(graphics: &mut Graphics, scene: &mut Scene, folder: &Path, body_transform: NodeId) -> Result<DigitalInstruments, String> {
        let mut d = DigitalInstruments::default();
        let Ok(ini) = IniReader::load(&folder.join("data/digital_instruments.ini")) else {
            return Ok(d);
        };
        println!("DigitalInstruments::initInstruments()");
        let gl = Rc::new(RefCell::new(GlRenderer::new(graphics, 0x80)));
        let get = |s: &str, k: &str| ini.get_float(s, k).unwrap_or(0.0);
        let get3 = |s: &str, k: &str| ini.get_float3(s, k).unwrap_or([0.0; 3]);
        let sections = |prefix: &str| -> Vec<String> {
            let mut out = Vec::new();
            let mut n = 0;
            loop {
                let name = format!("{prefix}{n}");
                if !ini.has_section(&name) {
                    break;
                }
                out.push(name);
                n += 1;
            }
            out
        };
        // 1: ITEM_n
        for section in sections("ITEM_") {
            let mut update_refresh = 0.0f32;
            let mut current_refresh = 0.0f32;
            if ini.has_key(&section, "REFRESH") {
                update_refresh = get(&section, "REFRESH");
                current_refresh = (graphics.crt_rand.next() as f32 * f32::from_bits(0x3800_0100)) * update_refresh;
            }
            let type_name = ini.get_string(&section, "TYPE");
            let mut item_decimals = 3;
            let (mut pre_fix, mut post_fix) = (String::new(), String::new());
            let mut tyre_number = 0;
            let mut time_to_ignore_base = -1.0f32;
            let mut color2 = [0.0f32; 4];
            let mut color3 = [0.0f32; 4];
            let (mut graph_min, mut graph_max, mut add_sign, mut fuel_cons_units) = (0.0f32, 0.0f32, 0, 0);
            let mut max_rpm = 0;
            let mut units = 0;
            // KERS_LOAD keeps its INVERTED flag in the decimals' place
            let optional_inverted = |decimals: &mut i32| {
                if ini.has_key(&section, "INVERTED") {
                    *decimals = ini.get_int(&section, "INVERTED").unwrap_or(0);
                }
            };
            let optional_decimals = |decimals: &mut i32| {
                if ini.has_key(&section, "DECIMALS") {
                    *decimals = ini.get_int(&section, "DECIMALS").unwrap_or(0);
                }
            };
            let kind = match type_name.as_str() {
                "PRESSURE" => {
                    tyre_number = get(&section, "TYRE_NUMBER") as i32;
                    ItemType::Pressure
                }
                "GEAR" => {
                    if ini.has_key(&section, "N_TIME") {
                        time_to_ignore_base = get(&section, "N_TIME");
                    }
                    ItemType::Gear
                }
                "GEAR_COLOR" => {
                    if ini.has_key(&section, "N_TIME") {
                        time_to_ignore_base = get(&section, "N_TIME");
                    }
                    let c = float4(&ini, &section, "COLOR_2");
                    let k = get(&section, "INTENSITY_2") * f32::from_bits(0x3b80_8081);
                    color2 = [k * c[0], k * c[1], k * c[2], c[3] * f32::from_bits(0x3b80_8081)];
                    max_rpm = ini.get_int(&section, "RPM_TRIGGER").unwrap_or(0);
                    ItemType::GearColor
                }
                "SPEED" => {
                    units = match ini.get_string(&section, "UNITS").as_str() {
                        "KMH" => 1,
                        "MPH" => 2,
                        _ => 0,
                    };
                    ItemType::Speed
                }
                "WATER_TEMP" => ItemType::WaterTemp,
                "AMBIENT_TEMP" => ItemType::AmbientTemp,
                "CURRENT_LAP" => {
                    pre_fix = ini.get_string(&section, "PREFIX");
                    post_fix = ini.get_string(&section, "POSTFIX");
                    ItemType::CurrentLap
                }
                "KERS_CHARGE" => ItemType::KersCharge,
                "PLACE_HOLDER" => {
                    pre_fix = ini.get_string(&section, "TEXT");
                    ItemType::PlaceHolder
                }
                "RPM" => ItemType::Rpm,
                "MAX_RPM" => ItemType::MaxRpm,
                "MAX_SPEED" => ItemType::MaxSpeed,
                "LAST_LAP" => {
                    optional_decimals(&mut item_decimals);
                    ItemType::LastLap
                }
                "LAPTIME" => {
                    optional_decimals(&mut item_decimals);
                    ItemType::LapTime
                }
                "BEST_LAP" => {
                    optional_decimals(&mut item_decimals);
                    ItemType::BestLap
                }
                "PERF" => {
                    optional_decimals(&mut item_decimals);
                    ItemType::Perf
                }
                "TURBO_BOOST" => ItemType::TurboBoost,
                "TURBO_LEVEL" => ItemType::TurboLevel,
                "TOTAL_LAPS" => ItemType::TotalLaps,
                "EST_LAPS" => ItemType::EstLaps,
                "POSITION_CAR" => ItemType::PositionCar,
                "POSITION_COUNT" => ItemType::PositionCount,
                "FUEL_CONS" => {
                    if ini.has_key(&section, "UNITS") {
                        fuel_cons_units = match ini.get_string(&section, "UNITS").as_str() {
                            "MPG_UK" => 1,
                            "MPG_US" => 2,
                            "L100" => 3,
                            _ => 0,
                        };
                    }
                    ItemType::FuelCons
                }
                "GFORCES" => {
                    item_decimals = ini.get_int(&section, "DECIMALS").unwrap_or(0);
                    if ini.has_key(&section, "DIRECTION") {
                        tyre_number = match ini.get_string(&section, "DIRECTION").as_str() {
                            "Y" => 1,
                            "Z" => 2,
                            _ => 0,
                        };
                    }
                    add_sign = ini.get_int(&section, "ADDSIGN").unwrap_or(0);
                    graph_min = get(&section, "MIN");
                    graph_max = get(&section, "MAX");
                    ItemType::GForces
                }
                "KERS_LOAD" => {
                    optional_inverted(&mut item_decimals);
                    ItemType::KersLoad
                }
                "P2P_DASH" => {
                    let c = float4(&ini, &section, "COLOR_ACTIVE");
                    let k = get(&section, "INTENSITY_ACTIVE") * f32::from_bits(0x3b80_8081);
                    color2 = [k * c[0], k * c[1], k * c[2], c[3] * f32::from_bits(0x3b80_8081)];
                    let c = float4(&ini, &section, "COLOR_COOLING");
                    let k = get(&section, "INTENSITY_COOLING") * f32::from_bits(0x3b80_8081);
                    color3 = [k * c[0], k * c[1], k * c[2], c[3] * f32::from_bits(0x3b80_8081)];
                    ItemType::P2pDash
                }
                "FUEL_PERC" => {
                    pre_fix = ini.get_string(&section, "PREFIX");
                    post_fix = ini.get_string(&section, "POSTFIX");
                    item_decimals = ini.get_int(&section, "DECIMALS").unwrap_or(0);
                    ItemType::FuelPerc
                }
                "TC_LEVEL" => ItemType::TcLevel,
                "ABS_LEVEL" => ItemType::AbsLevel,
                "CLOCK" => ItemType::Clock,
                "FUEL" => {
                    pre_fix = ini.get_string(&section, "PREFIX");
                    post_fix = ini.get_string(&section, "POSTFIX");
                    item_decimals = 0;
                    optional_decimals(&mut item_decimals);
                    ItemType::Fuel
                }
                "RPM_GRAPH" | "DELTA_GRAPH" | "GEAR_TX" => {
                    println!("NOTE: digital item {section} TYPE={type_name} is drawn by a DisplayNode, which is not ported: it is left out");
                    continue;
                }
                other => {
                    println!("NOTE: digital item {section} TYPE={other} is not ported: it shows nothing");
                    ItemType::Unknown
                }
            };
            // PARENT
            let parent_name = ini.get_string(&section, "PARENT");
            let mut parent = body_transform;
            if parent_name != "NULL" {
                match scene.find_child_by_name(body_transform, &parent_name, true) {
                    Some(n) => parent = n,
                    None => println!("[ERROR] DIGITAL ITEM PARENT: {parent_name} NOT FOUND"),
                }
            }
            let c = float4(&ini, &section, "COLOR");
            let k = get(&section, "INTENSITY") * f32::from_bits(0x3b80_8081);
            let color = [k * c[0], k * c[1], k * c[2], c[3] * f32::from_bits(0x3b80_8081)];
            let blitter = StringBlitter3D::new(graphics, &ini.get_string(&section, "FONT"), gl.clone());
            let node = scene.object_node(&section);
            let scale = get(&section, "SIZE") / blitter.size_y;
            let position = get3(&section, "POSITION");
            scene.nodes[node].matrix.m[3][0] = position[0];
            scene.nodes[node].matrix.m[3][1] = position[1];
            scene.nodes[node].matrix.m[3][2] = position[2];
            let mut version = 1u16;
            if ini.has_key(&section, "VERSION") {
                version = ini.get_int(&section, "VERSION").unwrap_or(0) as u16;
            }
            let align = match ini.get_string(&section, "ALIGN").as_str() {
                "CENTER" => 2,
                "RIGHT" => 1,
                _ => 0,
            };
            let text_node = Rc::new(RefCell::new(TextNode { node, text: String::new(), scale, color, align, version, blitter }));
            scene.set_renderable_object(node, text_node.clone());
            scene.add_child(parent, node);
            d.items.push(DigitalItem {
                kind,
                color,
                color2,
                color3,
                graph_min,
                graph_max,
                add_sign,
                fuel_cons_units,
                text_node,
                decimals: item_decimals,
                max_rpm,
                max_speed: 0,
                pre_fix,
                post_fix,
                update_refresh,
                current_refresh,
                speedometer_units: units,
                tyre_number,
                last_gear: 1,
                time_to_ignore: -1.0,
                time_to_ignore_base,
            });
        }
        // 2: DISPLAY_n: the model's own material glows
        for section in sections("DISPLAY_") {
            let name = ini.get_string(&section, "NAME");
            let intensity = get(&section, "INTENSITY");
            let e = get3(&section, "EMISSIVE");
            let v = [intensity * e[0], intensity * e[1], intensity * e[2]];
            match mesh_node(scene, body_transform, &name).and_then(|n| mesh_material(scene, n)) {
                Some(id) => {
                    let m = &mut scene.materials[id.0 as usize];
                    if let Some(var) = m.get_var_reporting("ksEmissive") {
                        m.update_var(var, |x| x.f_value3 = v);
                    }
                    m.set_float("ksDiffuse", 0.0);
                    m.set_float("ksAmbient", 0.0);
                    m.set_float("ksSpecular", 0.0);
                }
                None => println!("[ERROR]: DISPLAY MESH {name}NOT FOUND"),
            }
        }
        // the LEDs that an ini section describes (a mesh with a material of its own)
        let ini_led = |graphics: &mut Graphics, scene: &mut Scene, section: &str, kind: i32| -> Result<DigitalLed, String> {
            let mut led = DigitalLed::plain(kind);
            let name = ini.get_string(section, "OBJECT_NAME");
            let Some(mesh) = mesh_node(scene, body_transform, &name) else {
                println!("ERROR: Digital Led target {name}");
                return Ok(led);
            };
            led.target_mesh = Some(mesh);
            let material = clone_material(graphics, scene, mesh)?;
            led.emissive = get3(section, "EMISSIVE");
            match kind {
                0 => {
                    led.fswitch = get(section, "RPM_SWITCH");
                    led.blink_switch = get(section, "BLINK_SWITCH");
                    led.blink_frequency = get(section, "BLINK_HZ");
                }
                3 => {
                    led.fswitch = get(section, "FUEL_SWITCH");
                    led.inverted = ini.get_int(section, "INVERTED").unwrap_or(0) != 0;
                }
                13 => led.show_max = get(section, "BLINK_TIME"),
                15 => led.fswitch = get(section, "TURBO_SWITCH"),
                19 => {
                    led.blink_switch = get(section, "SLIP_SWITCH");
                    led.selected_wing = ini.get_int(section, "SHOW_LOCK").unwrap_or(0);
                    led.fswitch = ini.get_int(section, "TYRE_INDEX").unwrap_or(0) as f32;
                    led.emissive2 = get3(section, "EMISSIVE_LOCK");
                    led.show_max = get(section, "WHEEL_SPEED_MULT");
                }
                _ => {}
            }
            if let Some(id) = material {
                let m = &mut scene.materials[id.0 as usize];
                let diffuse = get(section, "DIFFUSE");
                m.set_float("ksDiffuse", diffuse);
                m.set_float("ksAmbient", diffuse);
                m.set_float("ksSpecular", 0.0);
                led.var_emissive = m.get_var_reporting("ksEmissive").map(|v| (id, v));
            }
            Ok(led)
        };
        // 3: LED_n
        for section in sections("LED_") {
            d.leds.push(ini_led(graphics, scene, &section, 0)?);
        }
        // 4: FUEL_n
        for section in sections("FUEL_") {
            let mut led = DigitalLed::plain(2);
            led.target_mesh = mesh_node(scene, body_transform, &ini.get_string(&section, "OBJECT_NAME"));
            led.fswitch = ini.get_int(&section, "FUEL_SWITCH").unwrap_or(0) as f32;
            d.leds.push(led);
        }
        // 5: FUEL_WARNING_n
        for section in sections("FUEL_WARNING_") {
            if ini.has_key(&section, "EMISSIVE") {
                d.leds.push(ini_led(graphics, scene, &section, 3)?);
            } else {
                let mut led = DigitalLed::plain(3);
                led.target_mesh = mesh_node(scene, body_transform, &ini.get_string(&section, "OBJECT_NAME"));
                led.fswitch = ini.get_int(&section, "FUEL_SWITCH").unwrap_or(0) as f32;
                d.leds.push(led);
            }
        }
        // 6 .. 9, 11
        for (prefix, kind) in [("TYRE_LOCK_SLIP_", 19), ("DRS_AVAILABLE_", 12), ("DRS_ENABLED_", 13), ("KERS_ENABLED_", 14)] {
            for section in sections(prefix) {
                d.leds.push(ini_led(graphics, scene, &section, kind)?);
            }
        }
        // 10: PERF_LED_n
        let mut aborted = false;
        for section in sections("PERF_LED_") {
            let mut led = DigitalLed::plain(10);
            let name = ini.get_string(&section, "OBJECT_NAME");
            let Some(mesh) = mesh_node(scene, body_transform, &name) else {
                println!("ERROR: Digital Led target {name}");
                aborted = true;
                break;
            };
            if let Some(id) = clone_material(graphics, scene, mesh)? {
                led.var_emissive = scene.materials[id.0 as usize].get_var_reporting("ksEmissive").map(|v| (id, v));
            }
            led.target_mesh = Some(mesh);
            led.emissive = get3(&section, "EMISSIVE_NEG");
            led.emissive2 = get3(&section, "EMISSIVE_POS");
            led.emissive3 = get3(&section, "EMISSIVE_BASE");
            d.leds.push(led);
        }
        if aborted {
            return Ok(d);
        }
        for section in sections("TURBO_BOOST_LED_") {
            d.leds.push(ini_led(graphics, scene, &section, 15)?);
        }
        // the series: one mesh per step, shown from its threshold on
        let serie = |scene: &Scene, name: String, threshold: f32, blink_switch: f32, blink_frequency: f32| -> DigitalLed {
            let mut led = DigitalLed::plain(1);
            led.fswitch = threshold;
            led.blink_switch = blink_switch;
            led.blink_frequency = blink_frequency;
            led.target_mesh = mesh_node(scene, body_transform, &name);
            if led.target_mesh.is_none() {
                println!("ERROR: CANNOT FIND DigitalLed mesh:{name}");
            }
            led
        };
        // an index of a series is streamed as a float: 12.0 prints 12
        let float_text = |v: f32| if v == v.trunc() && v.abs() < 1.0e6 { format!("{}", v as i64) } else { format!("{v}") };
        let get_int = |s: &str, k: &str| ini.get_int(s, k).unwrap_or(0);
        // 12: RPM_SERIE_n
        for section in sections("RPM_SERIE_") {
            let prefix = ini.get_string(&section, "PREFIX");
            let (start, end) = (get_int(&section, "START_INDEX") as f32, get_int(&section, "END_INDEX") as f32);
            let (rpm_start, rpm_end) = (get_int(&section, "RPM_START") as f32, get_int(&section, "RPM_END") as f32);
            let blink_switch = if ini.has_key(&section, "BLINK_SWITCH") { get(&section, "BLINK_SWITCH") } else { -1.0 };
            let blink_hz = if ini.has_key(&section, "BLINK_HZ") { get(&section, "BLINK_HZ") } else { 0.0 };
            let step = (rpm_end - rpm_start) / (end - start);
            let mut t = rpm_start;
            let mut i = start;
            while i <= end {
                d.leds.push(serie(scene, format!("{prefix}{}", float_text(i)), t, blink_switch, blink_hz));
                t += step;
                i += 1.0;
            }
        }
        // 13: GFORCE_SERIE_n
        for section in sections("GFORCE_SERIE_") {
            let prefix = ini.get_string(&section, "PREFIX");
            let (start, end) = (get_int(&section, "START_INDEX") as f32, get_int(&section, "END_INDEX") as f32);
            let a = get(&section, "FORCE_START") * 1000.0;
            let b = get(&section, "FORCE_END") * 1000.0;
            let sign = get_int(&section, "SIGN");
            let direction = match ini.get_string(&section, "DIRECTION").as_str() {
                "Y" => 1,
                "Z" => 2,
                _ => 0,
            };
            let step = (b - a) / (end - start);
            let mut t = a;
            let mut i = start;
            while i <= end {
                let mut led = serie(scene, format!("{prefix}{}", float_text(i)), t, -1.0, 0.0);
                led.kind = 9;
                led.g_force = direction;
                led.g_force_sign = sign;
                d.leds.push(led);
                t += step;
                i += 1.0;
            }
        }
        // 14, 16: the KERS series
        for (name, kind) in [("KERS_CHARGE_SERIE_", 4), ("TURBO_BOOST_SERIE_", 7), ("KERS_INPUT_SERIE_", 5)] {
            for section in sections(name) {
                let prefix = ini.get_string(&section, "PREFIX");
                let (start, end) = if kind == 5 { (get(&section, "START_INDEX"), get(&section, "END_INDEX")) } else { (get_int(&section, "START_INDEX") as f32, get_int(&section, "END_INDEX") as f32) };
                let (mut t, step);
                if kind == 7 {
                    let a = get(&section, "BOOST_START") * 100.0;
                    step = (get(&section, "BOOST_END") * 100.0 - a) / (end - start);
                    t = a;
                } else if ini.has_key(&section, "PERC_START") && ini.has_key(&section, "PERC_END") {
                    let (p0, p1) = (get(&section, "PERC_START"), get(&section, "PERC_END"));
                    step = (p1 - p0) / (end - start);
                    t = p0;
                } else {
                    step = (100.0 - 0.0) / (end - start);
                    t = 1.0;
                }
                let mut i = start;
                while i <= end {
                    let mut led = serie(scene, format!("{prefix}{}", float_text(i)), t, -1.0, 0.0);
                    led.kind = kind;
                    if kind == 4 && ini.has_key(&section, "SHOW_MIN") && ini.has_key(&section, "SHOW_MAX") {
                        led.show_min = get(&section, "SHOW_MIN");
                        led.show_max = get(&section, "SHOW_MAX");
                        led.show_excluded = get_int(&section, "SHOW_EXCLUDED") != 0;
                        led.has_show_limit = led.show_min != 0.0 || led.show_max != 0.0;
                    }
                    if kind == 5 && ini.has_key(&section, "INVERTED") {
                        led.inverted = get_int(&section, "INVERTED") != 0;
                    }
                    d.leds.push(led);
                    t += step;
                    i += 1.0;
                }
            }
        }
        // 17: DRS_SERIE_n
        for section in sections("DRS_SERIE_") {
            let prefix = ini.get_string(&section, "PREFIX");
            let (a, b) = (get(&section, "START_ANGLE"), get(&section, "END_ANGLE"));
            let count = get_int(&section, "LED_COUNT") as f32;
            let inverted = get_int(&section, "INVERTED_DRS") != 0;
            let step = (b - a) / count;
            let mut t = step + a;
            let mut k = 0;
            while (k as f32) < count {
                let mut led = serie(scene, format!("{prefix}{k}"), t, -1.0, 0.0);
                led.kind = 8;
                led.selected_wing = get_int(&section, "WING_NUMBER");
                led.inverted = inverted;
                d.leds.push(led);
                t += step;
                k += 1;
            }
        }
        // 18: KERS_LOAD_SERIE_n
        for section in sections("KERS_LOAD_SERIE_") {
            let prefix = ini.get_string(&section, "PREFIX");
            let (start, end) = (get_int(&section, "START_INDEX") as f32, get_int(&section, "END_INDEX") as f32);
            let inverted = get_int(&section, "INVERTED") != 0;
            let (p0, p1) = (get(&section, "PERC_START"), get(&section, "PERC_END"));
            let step = (p1 - p0) / (end - start);
            let mut t = p0;
            let mut i = start;
            while i <= end {
                let mut led = serie(scene, format!("{prefix}{}", float_text(i)), t, -1.0, 0.0);
                led.kind = 11;
                led.inverted = inverted;
                d.leds.push(led);
                t += step;
                i += 1.0;
            }
        }
        // 19, 20: counted series
        for (name, kind, start_key, end_key) in [("WATER_TEMP_", 6, "START_TEMP", "END_TEMP"), ("SPEED_SERIE_", 16, "START_SPEED", "END_SPEED")] {
            for section in sections(name) {
                let prefix = ini.get_string(&section, "PREFIX");
                let (a, b) = (get_int(&section, start_key) as f32, get_int(&section, end_key) as f32);
                let count = get_int(&section, "LED_COUNT") as f32;
                let step = (b - a) / count;
                let mut t = step + a;
                let mut k = 0;
                while (k as f32) < count {
                    let mut led = serie(scene, format!("{prefix}{k}"), t, -1.0, 0.0);
                    led.kind = kind;
                    d.leds.push(led);
                    t += step;
                    k += 1;
                }
            }
        }
        // 21: POWER_918_n
        for section in sections("POWER_918_") {
            let prefix = ini.get_string(&section, "PREFIX");
            let (a, b) = (get(&section, "START_TORQUE"), get(&section, "END_TORQUE"));
            let count = get_int(&section, "LED_COUNT") as f32;
            let step = (b - a) / count;
            let mut t = step + a;
            let mut k = 0;
            while (k as f32) < count {
                let mut led = serie(scene, format!("{prefix}{k}"), t, -1.0, 0.0);
                led.kind = 17;
                led.show_min = get(&section, "FILTER");
                d.leds.push(led);
                t += step;
                k += 1;
            }
        }
        // 22: KERS_RECHARGE_SERIE_n
        for section in sections("KERS_RECHARGE_SERIE_") {
            let prefix = ini.get_string(&section, "PREFIX");
            let (start, end) = (get(&section, "START_INDEX"), get(&section, "END_INDEX"));
            let step = get(&section, "NORM_MAX") / (end - start);
            let inverted = ini.has_key(&section, "INVERTED") && get_int(&section, "INVERTED") != 0;
            let mut t = 0.0f32;
            let mut i = start;
            while i <= end {
                let mut led = serie(scene, format!("{prefix}{}", float_text(i)), t, -1.0, 0.0);
                led.kind = 18;
                led.inverted = inverted;
                d.leds.push(led);
                t += step;
                i += 1.0;
            }
        }
        // with the HDR post-processing off the colours are brought down to 1
        if !graphics.video.pp_hdr_enabled {
            for led in &mut d.leds {
                led.emissive = get_ldr_color(graphics, led.emissive);
                led.emissive2 = get_ldr_color(graphics, led.emissive2);
                led.emissive3 = get_ldr_color(graphics, led.emissive3);
            }
            for item in &mut d.items {
                let c = get_ldr_color(graphics, [item.color[0], item.color[1], item.color[2]]);
                item.color = [c[0], c[1], c[2], item.color[3]];
                let c = get_ldr_color(graphics, [item.color2[0], item.color2[1], item.color2[2]]);
                item.color2 = [c[0], c[1], c[2], item.color2[3]];
                let mut node = item.text_node.borrow_mut();
                let c = get_ldr_color(graphics, [node.color[0], node.color[1], node.color[2]]);
                node.color = [c[0], c[1], c[2], node.color[3]];
            }
        }
        Ok(d)
    }

    /// `DigitalInstruments::update` 0x1400f0520.
    pub fn update(&mut self, scene: &mut Scene, s: &CarPhysicsState, f: &DigitalFrame) {
        if !f.focused {
            return;
        }
        for item in &mut self.items {
            item.update(s, f);
        }
        if f.pause_menu {
            return;
        }
        let blink_odd = |hz: f32| ((f.game_time_ms / ((1000.0f32 / hz) as f64)) as i32) % 2 != 0;
        for led in &mut self.leds {
            let (var_emissive, target_mesh) = (led.var_emissive, led.target_mesh);
            let set = |scene: &mut Scene, c: [f32; 3]| {
                if let Some((material, var)) = var_emissive {
                    scene.materials[material.0 as usize].update_var(var, |m| m.f_value3 = c);
                }
            };
            let show = |scene: &mut Scene, on: bool| {
                if let Some(n) = target_mesh {
                    scene.nodes[n].is_active = on;
                }
            };
            let rpm = s.engine_rpm;
            let off = [0.0f32; 3];
            match led.kind {
                0 => {
                    set(scene, if led.fswitch > rpm { off } else { led.emissive });
                    if led.blink_frequency == 0.0 || led.blink_switch > rpm {
                        continue;
                    }
                    set(scene, if blink_odd(led.blink_frequency) { led.emissive } else { off });
                }
                1 => {
                    if led.target_mesh.is_none() {
                        continue;
                    }
                    if led.blink_frequency == 0.0 || rpm < led.blink_switch {
                        show(scene, led.fswitch <= rpm);
                    } else {
                        show(scene, if blink_odd(led.blink_frequency) { led.fswitch <= rpm } else { false });
                    }
                }
                2 => show(scene, led.fswitch <= s.fuel),
                3 => {
                    if led.target_mesh.is_none() {
                        continue;
                    }
                    if led.emissive != [0.0; 3] {
                        let low = led.fswitch > s.fuel;
                        set(scene, if low != led.inverted { led.emissive } else { off });
                    } else if !led.inverted {
                        show(scene, s.fuel < led.fswitch);
                    } else {
                        show(scene, led.fswitch <= s.fuel);
                    }
                }
                4 => {
                    let v = s.kers_charge * 100.0;
                    let within = if !led.has_show_limit {
                        true
                    } else if !led.show_excluded {
                        led.show_min <= v && v <= led.show_max
                    } else {
                        v <= led.show_min && v >= led.show_max
                    };
                    show(scene, v >= led.fswitch && within);
                }
                5 => {
                    let v = s.kers_input * 100.0;
                    show(scene, if led.inverted { v <= led.fswitch } else { v >= led.fswitch });
                }
                6 => show(scene, led.fswitch <= s.water),
                7 => show(scene, s.turbo_boost * 100.0 >= led.fswitch),
                8 => {
                    if led.selected_wing < 0 || led.selected_wing as usize >= f.wings_count {
                        continue;
                    }
                    let a = f.wing_angles[(led.selected_wing as usize).min(7)];
                    show(scene, if led.inverted { a <= led.fswitch } else { led.fswitch <= a });
                }
                11 => {
                    if led.target_mesh.is_none() {
                        continue;
                    }
                    let milli = f32::from_bits(0x3a83_126f);
                    if f.kers_max_j > 0.0 {
                        let v = (s.kers_current_kj * 100.0) / (f.kers_max_j * milli);
                        show(scene, if led.inverted { v <= led.fswitch } else { v >= led.fswitch });
                    }
                    if f.ers_max_j > 0.0 {
                        let v = (s.kers_current_kj * 100.0) / (f.ers_max_j * milli);
                        show(scene, if led.inverted { v <= led.fswitch } else { v >= led.fswitch });
                    }
                }
                17 => {
                    let (Some(_), Some((torque, ratio))) = (led.target_mesh, f.drivetrain) else { continue };
                    let r = if ratio != 0.0 { ratio as f32 } else { 1.0 };
                    if !torque.is_finite() || !r.is_finite() {
                        continue;
                    }
                    let mut t = if ratio != 0.0 { torque / r } else { torque };
                    t = if t > 1000.0 { 1000.0 } else if t >= 0.0 { t } else { 0.0 };
                    let k = f.dt * led.show_min;
                    let k = if k > 1.0 { 1.0 } else if k >= 0.0 { k } else { 0.0 };
                    led.show_max = (t - led.show_max) * k + led.show_max;
                    show(scene, led.show_max >= led.fswitch);
                }
                18 => show(scene, if led.inverted { f.ers_recharge <= led.fswitch } else { led.fswitch <= f.ers_recharge }),
                9 => {
                    let g = match led.g_force {
                        0 => s.acc_g[0],
                        1 => s.acc_g[1],
                        2 => s.acc_g[2],
                        _ => 0.0,
                    } * 1000.0;
                    if s.speed * f32::from_bits(0x4066_6666) < 2.0 {
                        show(scene, false);
                        continue;
                    }
                    if (g >= 0.0 && led.g_force_sign > 0) || (g < 0.0 && led.g_force_sign < 0) {
                        show(scene, g.abs() > led.fswitch);
                    }
                }
                10 => {
                    if led.target_mesh.is_none() {
                        continue;
                    }
                    let m = s.performance_meter;
                    set(scene, if m > 0.0 { led.emissive } else if m < 0.0 { led.emissive2 } else { led.emissive3 });
                }
                12 => set(scene, if s.status_bytes & 2 != 0 { led.emissive } else { off }),
                13 => {
                    if led.show_max == 0.0 {
                        set(scene, if s.status_bytes & 4 != 0 { led.emissive } else { off });
                    } else {
                        let button = (s.actions_state >> 19) & 1 != 0;
                        if s.status_bytes & 2 != 0 {
                            if button && !led.has_show_limit {
                                led.show_min = led.show_max;
                            }
                            led.has_show_limit = button;
                        }
                        if led.show_min > 0.0 {
                            let t = led.show_min - f.dt;
                            led.show_min = if t > 0.0 { t } else { 0.0 };
                            set(scene, led.emissive);
                        } else {
                            set(scene, off);
                        }
                    }
                }
                14 => set(scene, if (s.actions_state >> 20) & 1 != 0 { led.emissive } else { off }),
                15 => {
                    if led.target_mesh.is_none() {
                        continue;
                    }
                    set(scene, if led.fswitch <= s.turbo_boost { led.emissive } else { off });
                }
                16 => show(scene, s.speed * f32::from_bits(0x4066_6666) >= led.fswitch),
                19 => {
                    set(scene, off);
                    let i = (led.fswitch as i32).clamp(0, 3) as usize;
                    set(scene, if s.nd_slip[i] >= led.blink_switch { led.emissive } else { off });
                    if led.selected_wing == 1 {
                        let kmh = s.speed * f32::from_bits(0x4066_6666);
                        if s.wheel_angular_speed[i].abs() <= kmh * led.show_max && kmh > 10.0 {
                            set(scene, led.emissive2);
                        }
                    }
                }
                _ => {}
            }
        }
    }
}

impl DigitalItem {
    /// `DigitalItem::update` 0x1400f3190.
    fn update(&mut self, s: &CarPhysicsState, f: &DigitalFrame) {
        if self.update_refresh != 0.0 {
            self.current_refresh += f.dt;
            if self.current_refresh < self.update_refresh {
                return;
            }
            self.current_refresh = 0.0;
        }
        let kmh = f32::from_bits(0x4066_6666);
        let mph = f32::from_bits(0x400f_29f7);
        let mut node = self.text_node.borrow_mut();
        let text = match self.kind {
            ItemType::Unknown => return,
            ItemType::Gear | ItemType::GearColor => {
                let g = s.gear as u32;
                if g > 9 {
                    return;
                }
                if self.time_to_ignore_base <= 0.0 {
                    self.last_gear = g as i32;
                } else if self.last_gear != g as i32 && self.time_to_ignore <= 0.0 {
                    self.time_to_ignore = self.time_to_ignore_base;
                } else {
                    self.time_to_ignore = if g == 1 { self.time_to_ignore - f.dt } else { 0.0 };
                    if self.time_to_ignore <= 0.0 {
                        self.time_to_ignore = 0.0;
                        self.last_gear = s.gear;
                    }
                }
                if self.kind == ItemType::GearColor {
                    node.color = if (self.max_rpm as f32) > s.engine_rpm { self.color } else { self.color2 };
                }
                GEARS.get(self.last_gear as usize).copied().unwrap_or("").to_string()
            }
            ItemType::Speed => {
                let in_kmh = self.speedometer_units == 1 || (self.speedometer_units == 0 && !f.use_mph);
                format!("{}", (s.speed * if in_kmh { kmh } else { mph }) as i32)
            }
            ItemType::Rpm => format!("{}", s.engine_rpm as i32),
            ItemType::MaxRpm => {
                self.max_rpm = self.max_rpm.max(s.engine_rpm as i32);
                format!("{}", self.max_rpm)
            }
            ItemType::MaxSpeed => {
                self.max_speed = self.max_speed.max((s.speed * if f.use_mph { mph } else { kmh }) as i32);
                format!("{}", self.max_speed)
            }
            ItemType::Fuel => format!("{}{:.*}{}", self.pre_fix, self.decimals.max(0) as usize, s.fuel as f64, self.post_fix),
            ItemType::Clock => {
                // ksTimeFromAngle 0x1400b3eb0
                let x = (f.sun_angle + 80.0) * 0.0625 + 8.0;
                let h = x.floor();
                let m = ((x - h) * 60.0) as i32;
                format!("{:02}:{:02}", h as i32, m)
            }
            ItemType::LastLap => {
                if self.decimals == 0 {
                    time_to_secs_string(s.last_lap as i32)
                } else {
                    time_to_string(s.last_lap as i32, self.decimals)
                }
            }
            ItemType::BestLap => {
                if self.decimals == 0 {
                    time_to_secs_string(s.best_lap as i32)
                } else {
                    time_to_string(s.best_lap as i32, self.decimals)
                }
            }
            ItemType::LapTime => {
                let v = if s.lap_time < 10000 { s.last_lap } else { s.lap_time } as i32;
                if self.decimals == 0 {
                    time_to_secs_string(v)
                } else {
                    time_to_string(v, self.decimals)
                }
            }
            ItemType::Perf => time_to_diff_string((s.performance_meter * 1000.0) as i32, if self.decimals == 0 { 3 } else { self.decimals }),
            ItemType::TcLevel => format!("{}", f.tc_level),
            ItemType::AbsLevel => format!("{}", f.abs_level),
            ItemType::TurboBoost => format!("{:.1}", s.turbo_boost as f64),
            ItemType::Pressure => format!("{}", s.tyre_thermal_states.get(self.tyre_number as usize).map(|t| t.dynamic_pressure).unwrap_or(0.0) as i32),
            ItemType::WaterTemp => format!("{}", s.water as i32),
            ItemType::AmbientTemp => format!("{}", f.ambient_temperature as i32),
            ItemType::CurrentLap => format!("{}{}{}", self.pre_fix, f.lap_count.wrapping_add(1), self.post_fix),
            ItemType::KersCharge => format!("{}", (s.kers_charge * 100.0) as i32),
            ItemType::PlaceHolder => self.pre_fix.clone(),
            ItemType::TurboLevel => {
                let r = (s.turbo_boost_level * 10.0).round();
                format!("{}%", (r * 10.0) as i32)
            }
            ItemType::TotalLaps => {
                if f.session_type == 3 {
                    format!("{}", f.session_laps)
                } else {
                    "---".to_string()
                }
            }
            ItemType::EstLaps => {
                if s.fuel_laps >= 0.0 {
                    format!("{:.1}", s.fuel_laps as f64)
                } else {
                    "--.-".to_string()
                }
            }
            ItemType::FuelCons => {
                let x = f.km_per_liter as f64;
                if x > 0.0 && x <= 99.0 {
                    let v = match self.fuel_cons_units {
                        1 => x * 2.819999933242798,
                        2 => x * 2.3499999046325684,
                        3 => 100.0 / x,
                        _ => x,
                    };
                    format!("{v:.1}")
                } else {
                    "--.-".to_string()
                }
            }
            ItemType::GForces => {
                let g = match self.tyre_number {
                    0 => s.acc_g[0],
                    1 => s.acc_g[1],
                    2 => s.acc_g[2],
                    _ => 0.0,
                };
                let v = if g > self.graph_max {
                    self.graph_max
                } else if g < self.graph_min {
                    self.graph_min
                } else {
                    g
                };
                let sign = if self.add_sign == 0 {
                    ""
                } else if v > 0.0 {
                    "+"
                } else if v < 0.0 {
                    "-"
                } else {
                    ""
                };
                format!("{sign}{:.*}", self.decimals.max(0) as usize, v.abs() as f64)
            }
            ItemType::KersLoad => {
                if f.has_kers {
                    let mut v = (s.kers_current_kj / f.kers_max_j) * 100000.0;
                    if self.decimals == 0 {
                        v = 100.0 - v;
                    }
                    let v = if v > 100.0 {
                        100.0
                    } else if v < 0.0 {
                        0.0
                    } else {
                        v
                    };
                    format!("{:.0}{}", v as f64, self.post_fix)
                } else {
                    String::new()
                }
            }
            ItemType::PositionCar => {
                if f.session_type == 3 {
                    format!("{}{}{}", self.pre_fix, f.real_time_position + 1, self.post_fix)
                } else {
                    format!("{}{}{}", self.pre_fix, f.leaderboard_position, self.post_fix)
                }
            }
            ItemType::PositionCount => format!("{}{}{}", self.pre_fix, f.cars_count, self.post_fix),
            ItemType::P2pDash => {
                match s.p2p_status {
                    1 => node.color = self.color3,
                    2 => node.color = self.color,
                    3 => node.color = self.color2,
                    _ => {}
                }
                format!("{}", s.p2p_activations)
            }
            ItemType::FuelPerc => {
                let x = s.fuel as f64 / f.max_fuel;
                let x = if x > 1.0 {
                    1.0
                } else if x < 0.0 {
                    0.0
                } else {
                    x
                };
                let n = (x * 100.0) as i32;
                format!("{}{:.*}{}", self.pre_fix, self.decimals.max(0) as usize, n as f64, self.post_fix)
            }
        };
        node.text = text;
    }
}
