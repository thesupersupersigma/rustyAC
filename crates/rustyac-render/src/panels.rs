// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! `DigitalPanels` (0xd8 bytes): digits made of one textured quad per value (`DisplayNode`,
//! 0x168 bytes) that show the car's race position and its push-to-pass count, and the lights
//! that glow while push-to-pass is on. `DigitalPanels::DigitalPanels` 0x1400820f0,
//! `initPanels` 0x140082740, `update` 0x140083f70; `DisplayNode::DisplayNode` 0x1400f6d40,
//! `render` 0x1400f74a0, `drawBase` 0x1400f6fb0, `drawBaseInverted` 0x1400f70e0, `drawTop`
//! 0x1400f7210, `drawTopInverted` 0x1400f7360; the lights are `DigitalLed` 0x1400f5460 of
//! type 0x14, `DigitalLed::update` 0x1400f6200.
//!
//! The file is `data/digital_panels.ini`: `FULLPOSITION_SERIES_n`, `PUSH2PASS_SERIES_n` and
//! `PUSH2PASS_LED_n`, read in that order.

use std::cell::{Cell, RefCell};
use std::path::Path;
use std::rc::Rc;

use rustyac_physics::data::ini::IniReader;

use crate::gl::GL_QUADS;
use crate::graphics::Graphics;
use crate::material::{MaterialId, PASS_TRANSPARENT};
use crate::scene::{NodeId, OnNodeRenderEvent, RenderableObject, Scene};
use crate::state::CarPhysicsState;
use crate::texture::Texture;

/// `DigitalItemType` values a `DisplayNode` tells apart.
pub const TYPE_GEAR_TX: i32 = 0x19;
pub const TYPE_POSITION_CAR: i32 = 0x1e;
pub const TYPE_DELTA_GRAPH: i32 = 0x21;

/// `DisplayNode`: a quad with a base texture and, for the graphs, a top texture over a part
/// of its width.
pub struct DisplayNode {
    pub node: NodeId,
    pub tx_base: Texture,
    pub tx_top: Texture,
    pub size: [f32; 2],
    pub color: [f32; 4],
    pub blend_x: f32,
    pub kind: i32,
    pub trigger: f32,
    pub digit: i32,
    /// `valueInt`: the number the node compares its trigger with (a member of its owner)
    pub value_int: Option<Rc<Cell<i32>>>,
}

impl DisplayNode {
    /// `DisplayNode::DisplayNode` 0x1400f6d40.
    pub fn new(node: NodeId) -> DisplayNode {
        DisplayNode { node, tx_base: Texture::default(), tx_top: Texture::default(), size: [0.0; 2], color: [1.0; 4], blend_x: 0.5, kind: 0, trigger: 0.0, digit: 0, value_int: None }
    }

    /// The four corners of one of the four draw functions, through the manager's `GLRenderer`.
    fn quad(&self, graphics: &mut Graphics, texture: &Texture, corners: [([f32; 2], f32); 4]) {
        if texture.kid.is_none() {
            return;
        }
        graphics.set_texture(0, texture);
        let Some(mut gl) = graphics.gl.take() else { return };
        gl.color4f(self.color[0], self.color[1], self.color[2], self.color[3]);
        gl.begin(GL_QUADS, None);
        let sy = self.size[1];
        for (i, (uv, x)) in corners.iter().enumerate() {
            gl.tex_coord2f(uv[0], uv[1]);
            gl.vertex3f(*x, if i < 2 { 0.0 } else { sy }, 0.0);
        }
        gl.end(graphics);
        graphics.gl = Some(gl);
    }

    /// `DisplayNode::drawBase` 0x1400f6fb0.
    fn draw_base(&self, graphics: &mut Graphics) {
        let sx = self.size[0];
        self.quad(graphics, &self.tx_base, [([0.0, 1.0], 0.0), ([1.0, 1.0], -sx), ([1.0, 0.0], -sx), ([0.0, 0.0], 0.0)]);
    }

    /// `DisplayNode::drawBaseInverted` 0x1400f70e0.
    fn draw_base_inverted(&self, graphics: &mut Graphics) {
        let sx = self.size[0];
        self.quad(graphics, &self.tx_base, [([1.0, 1.0], sx), ([0.0, 1.0], 0.0), ([0.0, 0.0], 0.0), ([1.0, 0.0], sx)]);
    }

    /// `DisplayNode::drawTop` 0x1400f7210.
    fn draw_top(&self, graphics: &mut Graphics) {
        let b = self.blend_x;
        let x = -(b * self.size[0]);
        self.quad(graphics, &self.tx_top, [([0.0, 1.0], 0.0), ([b, 1.0], x), ([b, 0.0], x), ([0.0, 0.0], 0.0)]);
    }

    /// `DisplayNode::drawTopInverted` 0x1400f7360.
    fn draw_top_inverted(&self, graphics: &mut Graphics) {
        let b = self.blend_x;
        let x = b * self.size[0];
        self.quad(graphics, &self.tx_top, [([b, 1.0], x), ([0.0, 1.0], 0.0), ([0.0, 0.0], 0.0), ([b, 0.0], x)]);
    }
}

impl RenderableObject for DisplayNode {
    /// `DisplayNode::render` 0x1400f74a0. No state of its own: the blend, cull and depth
    /// modes are what the draw before it left, and the material cache is not reset.
    fn render(&mut self, scene: &mut Scene, graphics: &mut Graphics, event: &OnNodeRenderEvent) -> bool {
        if event.pass_id == PASS_TRANSPARENT {
            graphics.set_world_matrix(&scene.nodes[self.node].matrix_ws);
            match self.kind {
                TYPE_GEAR_TX => {
                    if let Some(v) = &self.value_int {
                        if v.get() as f32 == self.trigger {
                            self.draw_base(graphics);
                        }
                    }
                }
                TYPE_POSITION_CAR => {
                    if let Some(v) = &self.value_int {
                        // the game divides by `digit` without a test (0 stops it)
                        if self.digit != 0 && (v.get().wrapping_div(self.digit) % 10) as f32 == self.trigger {
                            self.draw_base(graphics);
                        }
                    }
                }
                TYPE_DELTA_GRAPH => {
                    if self.trigger == 0.0 {
                        self.draw_base(graphics);
                        self.draw_top(graphics);
                    } else {
                        self.draw_base_inverted(graphics);
                        self.draw_top_inverted(graphics);
                    }
                }
                _ => {
                    self.draw_base(graphics);
                    self.draw_top(graphics);
                }
            }
        }
        true
    }
}

/// `INIReader::getFloat4`: four numbers with commas between them; what is missing is 0.
pub(crate) fn float4(ini: &IniReader, section: &str, key: &str) -> [f32; 4] {
    let text = ini.get_string(section, key);
    let mut out = [0.0f32; 4];
    for (slot, part) in out.iter_mut().zip(text.split(',')) {
        *slot = part.trim().parse::<f64>().unwrap_or(0.0) as f32;
    }
    out
}

/// A `DigitalLed` of type 0x14 (`P2P_ENABLED_EXT`).
struct P2pLed {
    emissive: [f32; 3],
    blink_frequency: f32,
    show_max: f32,
    inverted: bool,
    var_emissive: Option<(MaterialId, usize)>,
}

/// What the panels read beyond the car's physics state.
#[derive(Clone, Copy, Debug, Default)]
pub struct PanelFrame {
    /// `Game::gameTime.now`, milliseconds
    pub game_time_ms: f64,
    /// the pause menu shows
    pub pause_menu: bool,
    /// `RaceManager::getCurrentSessionType`
    pub session_type: i32,
    /// `RaceManager::getCarLeaderboardPosition` (1 the first, -1 none)
    pub leaderboard_position: i32,
    /// `RaceManager::getCarRealTimePosition` (0 the first)
    pub real_time_position: i32,
}

pub struct DigitalPanels {
    last_position: Rc<Cell<i32>>,
    boost_count: Rc<Cell<i32>>,
    p2p_blink_frequency: f32,
    items: Vec<Rc<RefCell<DisplayNode>>>,
    leds: Vec<P2pLed>,
}

impl DigitalPanels {
    /// `DigitalPanels::DigitalPanels` 0x1400820f0 with `initPanels` 0x140082740.
    pub fn new(graphics: &mut Graphics, scene: &mut Scene, folder_text: &str, folder: &Path, body_transform: NodeId) -> Result<DigitalPanels, String> {
        let mut d = DigitalPanels { last_position: Rc::new(Cell::new(-1)), boost_count: Rc::new(Cell::new(-1)), p2p_blink_frequency: -1.0, items: Vec::new(), leds: Vec::new() };
        println!("DigitalPanels::initPanel()");
        let Ok(ini) = IniReader::load(&folder.join("data/digital_panels.ini")) else {
            return Ok(d);
        };
        let get = |s: &str, k: &str| ini.get_float(s, k).unwrap_or(0.0);
        let get_int = |s: &str, k: &str| ini.get_int(s, k).unwrap_or(0);
        let k255 = f32::from_bits(0x3b80_8081);
        for (family, push_to_pass) in [("FULLPOSITION_SERIES_", false), ("PUSH2PASS_SERIES_", true)] {
            let mut n = 0;
            loop {
                let section = format!("{family}{n}");
                if !ini.has_section(&section) {
                    break;
                }
                n += 1;
                let value = if push_to_pass { d.boost_count.clone() } else { d.last_position.clone() };
                value.set(0);
                let prefix = ini.get_string(&section, "PREFIX");
                let (start, end) = (get_int(&section, "START"), get_int(&section, "END"));
                if push_to_pass {
                    d.p2p_blink_frequency = get(&section, "BLINK_HZ");
                }
                let parent = ini.get_string(&section, "PARENT");
                if parent == "NULL" {
                    continue;
                }
                let mut parents = Vec::new();
                scene.find_children_by_name(body_transform, &parent, &mut parents);
                for p in parents {
                    let mut i = start;
                    while i <= end {
                        let file = format!("{folder_text}/texture/display_panel/{prefix}{i}.dds");
                        if Path::new(&file).is_file() {
                            let node = scene.object_node(&format!("{prefix}{i}"));
                            let mut display = DisplayNode::new(node);
                            display.tx_base = graphics.resources.get_texture(&graphics.kgl, &file);
                            display.trigger = i as f32;
                            display.digit = get_int(&section, "DIGIT");
                            display.value_int = Some(value.clone());
                            display.kind = TYPE_POSITION_CAR;
                            display.size = [get(&section, "WIDTH"), get(&section, "HEIGHT")];
                            let c = float4(&ini, &section, "COLOR");
                            let k = get(&section, "INTENSITY") * k255;
                            display.color = [k * c[0], k * c[1], k * c[2], c[3] * k255];
                            let position = ini.get_float3(&section, "POSITION").unwrap_or([0.0; 3]);
                            scene.nodes[node].matrix.m[3][0] = position[0];
                            scene.nodes[node].matrix.m[3][1] = position[1];
                            scene.nodes[node].matrix.m[3][2] = position[2];
                            let display = Rc::new(RefCell::new(display));
                            scene.set_renderable_object(node, display.clone());
                            scene.add_child(p, node);
                            d.items.push(display);
                        } else {
                            println!("[ERROR]: TEXTURE_BASE {file} NOT FOUND");
                        }
                        i += 1;
                    }
                }
            }
        }
        // PUSH2PASS_LED_n: one light per child of the body that holds the mesh
        let mut n = 0;
        loop {
            let section = format!("PUSH2PASS_LED_{n}");
            if !ini.has_section(&section) {
                break;
            }
            n += 1;
            for child in scene.nodes[body_transform].children.clone() {
                let name = ini.get_string(&section, "OBJECT_NAME");
                let Some(mesh) = crate::digital::mesh_node(scene, child, &name) else {
                    println!("ERROR: Digital Led target {name}");
                    continue;
                };
                let material = crate::digital::clone_material(graphics, scene, mesh)?;
                let mut led = P2pLed { emissive: ini.get_float3(&section, "EMISSIVE").unwrap_or([0.0; 3]), blink_frequency: 0.0, show_max: 0.0, inverted: false, var_emissive: None };
                led.inverted = get_int(&section, "INVERTED") != 0;
                led.blink_frequency = get(&section, "BLINK_HZ");
                led.show_max = 1.0;
                if let Some(id) = material {
                    let m = &mut scene.materials[id.0 as usize];
                    let diffuse = get(&section, "DIFFUSE");
                    m.set_float("ksDiffuse", diffuse);
                    m.set_float("ksAmbient", diffuse);
                    m.set_float("ksSpecular", 0.0);
                    led.var_emissive = m.get_var_reporting("ksEmissive").map(|v| (id, v));
                }
                d.leds.push(led);
            }
        }
        Ok(d)
    }

    /// `DigitalPanels::update` 0x140083f70.
    pub fn update(&mut self, scene: &mut Scene, s: &CarPhysicsState, f: &PanelFrame) {
        if self.items.is_empty() {
            return;
        }
        if self.last_position.get() >= 0 {
            let t = f.session_type;
            if t > 0 {
                if t <= 2 {
                    self.last_position.set(f.leaderboard_position);
                } else if t == 3 {
                    self.last_position.set(f.real_time_position + 1);
                }
            }
        }
        let odd = |hz: f32| ((f.game_time_ms / ((1000.0f32 / hz) as f64)) as i32) % 2 != 0;
        // (the game's second test is "not 0 >= frequency")
        #[allow(clippy::neg_cmp_op_on_partial_ord)]
        let blinks = !(0.0 >= self.p2p_blink_frequency);
        if self.boost_count.get() >= 0 || blinks {
            self.boost_count.set(s.p2p_activations as i32);
            if self.p2p_blink_frequency > 0.0 && s.p2p_status == 3 && odd(self.p2p_blink_frequency) {
                self.boost_count.set(-1);
            }
        }
        for led in &mut self.leds {
            // DigitalLed::update 0x1400f6200, type 0x14
            if f.pause_menu {
                continue;
            }
            if led.blink_frequency != 0.0 {
                led.show_max = if odd(led.blink_frequency) { 0.0 } else { 1.0 };
            }
            let on = if !led.inverted { s.p2p_status == 3 && led.show_max != 0.0 } else { s.p2p_status != 3 && s.p2p_status != 0 && led.show_max != 0.0 };
            if let Some((material, var)) = led.var_emissive {
                let c = if on { led.emissive } else { [0.0; 3] };
                scene.materials[material.0 as usize].update_var(var, |m| m.f_value3 = c);
            }
        }
    }
}
