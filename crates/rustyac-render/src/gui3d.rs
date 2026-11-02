// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! The GUI in a headset (Task 47b): `ksgui::GUI`'s 3D mode. With a VR camera the game draws its
//! GUI and HUD each frame into a target the size of the window (`preRender3D`, then
//! `renderHUD`, then the mouse pointer), and a node of the scene (`3D_GUI_NODE`, in the
//! unblurred part) shows that target to each eye on a curved screen at the driver's eyes,
//! turning with the car (`render3D`).
//!
//! `GUI::init3DMode` 0x140253f10, `preRender3D` 0x1402547c0, `renderVPointer` 0x140254c30,
//! `render3D` 0x140254900; the node's handler `Sim::Sim`'s lambda (`_Do_call` 0x140196a30).

use rustyac_math::{cosf, powf, sinf};
use rustyac_physics::vecmath::Vec3f;

use crate::gl::{GlRenderer, GL_QUADS};
use crate::graphics::{Graphics, RenderTarget, BLEND_ALPHA, CULL_NONE, DEPTH_OFF};
use crate::kgl::FORMAT_RGBA8;
use crate::scene::{NodeEventHandler, NodeId, OnNodeRenderEvent, Scene};
use crate::texture::Texture;

/// What `ksgui::GUI` keeps for its 3D mode.
pub struct Gui3d {
    /// `renderTarget`: the window's size, colour only
    pub render_target: RenderTarget,
    /// `referenceFrameForOrientation`: the player's car body
    pub reference_frame: NodeId,
    /// `positionOffset3D`: the driver's eyes in the body (car.ini `[GRAPHICS] DRIVEREYES`)
    pub position_offset: [f32; 3],
    /// `txPointer`: content/gui/oculus_pointer.png
    pub tx_pointer: Texture,
    pub is_pointer_visible: bool,
    pub mouse_pos: [f32; 2],
    /// (-100, -100) until the first frame (`GUI::GUI`)
    pub old_mouse_pos: [f32; 2],
    pub mouse_speed: f32,
    /// `guiGL`, made by the first `render3D`
    gl: Option<GlRenderer>,
}

impl Gui3d {
    /// `GUI::init3DMode` 0x140253f10.
    pub fn new(graphics: &mut Graphics, width: i32, height: i32, reference_frame: NodeId, position_offset: [f32; 3]) -> Gui3d {
        let render_target = RenderTarget { kid_color: Some(graphics.kgl.create_render_target(FORMAT_RGBA8, width, height, 1, 1)), kid_depth: None, width, height };
        let game = crate::model::path_text(&graphics.game_folder);
        let tx_pointer = graphics.resources.get_texture(&graphics.kgl, &format!("{game}/content/gui/oculus_pointer.png"));
        Gui3d { render_target, reference_frame, position_offset, tx_pointer, is_pointer_visible: true, mouse_pos: [0.0; 2], old_mouse_pos: [-100.0; 2], mouse_speed: 0.0, gl: None }
    }

    /// `GUI::preRender3D` 0x1402547c0 up to `doRender2D`: the pointer's speed, the target
    /// bound (no depth, the viewport kept) and cleared. The caller then draws the GUI if it
    /// is active (`doRender2D`), the HUD and [`Gui3d::render_pointer`].
    pub fn pre_render(&mut self, graphics: &mut Graphics, dt: f32) {
        if -50.0 >= self.old_mouse_pos[0] {
            self.old_mouse_pos = self.mouse_pos;
        } else {
            let last = self.mouse_speed;
            let dy = self.mouse_pos[1] - self.old_mouse_pos[1];
            let dx = self.mouse_pos[0] - self.old_mouse_pos[0];
            self.old_mouse_pos = self.mouse_pos;
            let speed = (dy * dy + dx * dx).sqrt() / dt;
            // (it rises slowly and falls fast)
            let k = if speed > last { dt * 1.2 } else { dt * 5.2 };
            let k = if k > 1.0 {
                1.0
            } else if k >= 0.0 {
                k
            } else {
                0.0
            };
            let smoothed = (speed - last) * k + last;
            self.mouse_speed = if smoothed >= 4000.0 { 4000.0 } else { smoothed };
        }
        graphics.kgl.set_render_targets(self.render_target.kid_color.as_ref(), None);
        graphics.clear_render_target(&[0.0; 4]);
        graphics.set_depth_mode(DEPTH_OFF);
    }

    /// `GUI::renderVPointer` 0x140254c30: the pointer, larger while the mouse moves fast.
    pub fn render_pointer(&mut self, graphics: &mut Graphics) {
        if !self.is_pointer_visible {
            return;
        }
        let scale = powf(self.mouse_speed * 0.00025, 2.0) * 5.0 + 1.0;
        let Some(mut gl) = graphics.gl.take() else { return };
        graphics.set_blend_mode(BLEND_ALPHA);
        graphics.set_texture(0, &self.tx_pointer);
        gl.color3f(5.0, 2.5, 0.0);
        let (w, h) = graphics.resources.size(&self.tx_pointer).unwrap_or((0, 0));
        gl.quad_centred(graphics, self.mouse_pos[0], self.mouse_pos[1], w as f32 * scale, h as f32 * scale, true, None);
        graphics.gl = Some(gl);
    }

    /// `GUI::render3D` 0x140254900: the target on 24 quads, 140 degrees of a cylinder of
    /// 0.4 m around the eyes, 0.45 m high, facing the car's front.
    pub fn render_3d(&mut self, scene: &Scene, graphics: &mut Graphics) {
        graphics.set_blend_mode(BLEND_ALPHA);
        graphics.set_cull_mode(CULL_NONE);
        graphics.set_depth_mode(DEPTH_OFF);
        let gl = self.gl.get_or_insert_with(|| GlRenderer::new(graphics, 0x90));
        graphics.kgl.set_texture(0, &self.render_target.kid_color.as_ref().and_then(|c| c.shader_resource_view.clone()));
        let eyes = scene.local_to_world(self.reference_frame, &Vec3f { x: self.position_offset[0], y: self.position_offset[1], z: self.position_offset[2] });
        let mut world = scene.nodes[self.reference_frame].matrix_ws;
        world.m[3][0] = eyes.x;
        world.m[3][1] = eyes.y;
        world.m[3][2] = eyes.z;
        graphics.set_world_matrix(&world);
        let c = if graphics.video.pp_hdr_enabled { 3.0 } else { 1.0 };
        gl.color3f(c, c, c);
        gl.begin(GL_QUADS, None);
        let (step, start, radius, half, u_step) = (f32::from_bits(0x3dd0_8153), f32::from_bits(0x3f9c_60fe), f32::from_bits(0x3ecc_cccd), f32::from_bits(0x3e66_6667), f32::from_bits(0x3d2a_aaab));
        let at = |i: i32| {
            let a = i as f32 * step - start;
            (sinf(a) * radius, cosf(a) * radius, 1.0 - i as f32 * u_step)
        };
        for i in 0..24 {
            let (x0, z0, u0) = at(i);
            let (x1, z1, u1) = at(i + 1);
            gl.tex_coord2f(u0, 0.0);
            gl.vertex3f(x0, half, z0);
            gl.tex_coord2f(u1, 0.0);
            gl.vertex3f(x1, half, z1);
            gl.tex_coord2f(u1, 1.0);
            gl.vertex3f(x1, -half, z1);
            gl.tex_coord2f(u0, 1.0);
            gl.vertex3f(x0, -half, z0);
        }
        gl.end(graphics);
    }
}

impl NodeEventHandler for Gui3d {
    /// The `3D_GUI_NODE` handler: the transparent pass only.
    fn on_node_render(&mut self, scene: &mut Scene, graphics: &mut Graphics, event: &OnNodeRenderEvent) {
        if event.pass_id == 1 {
            self.render_3d(scene, graphics);
        }
    }
}
