// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! `CarFakeShadow` (0x158 bytes): the flat shadows under the body and under each tyre, drawn
//! when the `CAR_SHADOWS` node is reached in the opaque pass. `CarFakeShadow::CarFakeShadow`
//! 0x1400e0d80, `loadShadows` 0x1400e1eb0, `onNodeRenderEvent` 0x1400e22b0;
//! `mat44f::setFromHeadingUp` 0x1400602f0.
//!
//! `generateFakeShadow` 0x1400e1430 makes the five pictures of a car that has no
//! `body_shadow.png`: the body and each wheel seen from 10 m below, white on black. The game
//! saves them into the car's folder and shows no shadow until its next start; here they are
//! kept in memory (`generated`) and never written into the game's folder.

use std::path::Path;

use rustyac_math::sqrtf;
use rustyac_physics::data::ini::IniReader;
use rustyac_physics::vecmath::Mat44f;

use crate::gl::{GlRenderer, GL_QUADS};
use crate::graphics::{Graphics, BLEND_ALPHA, CULL_FRONT, DEPTH_NORMAL, DEPTH_NO_WRITE};
use crate::material::{MaterialFilter, PASS_OPAQUE};
use crate::scene::{NodeEventHandler, NodeId, OnNodeRenderEvent, RenderContext, Scene};
use crate::shader::ShaderId;
use crate::state::CarPhysicsState;
use crate::texture::Texture;

pub struct CarFakeShadow {
    pub width: f32,
    pub length: f32,
    pub tx_body: Texture,
    pub tx_wheels: [Texture; 4],
    pub tyre_side_size: f32,
    pub body_ref_height: f32,
    pub height_gain: f32,
    pub ambient_gain: f32,
    sh_fake_car_shadows: Option<ShaderId>,
    local_gl: GlRenderer,
    /// `car->carNode` and `car->bodyTransform`
    car_node: NodeId,
    body_transform: NodeId,
    /// `car->physicsState`, as of the frame's update
    pub state: CarPhysicsState,
    /// what `generateFakeShadow` drew: the four wheels (64 x 64) and the body (512 x 512), as
    /// RGBA bytes with the rows from the top, in the order the game saves them
    pub generated: Option<Vec<Vec<u8>>>,
}

/// `mat44f::setFromHeadingUp` 0x1400602f0: the three rows of the rotation; the rest of the
/// matrix stays.
pub fn set_from_heading_up(m: &mut Mat44f, h: &[f32; 3], u: &[f32; 3]) {
    let mut sx = u[2] * h[1] - u[1] * h[2];
    let mut sy = u[0] * h[2] - h[0] * u[2];
    let mut sz = h[0] * u[1] - u[0] * h[1];
    let l = sqrtf((sy * sy + sx * sx) + sz * sz);
    if l != 0.0 && !l.is_nan() {
        let inv = 1.0 / l;
        sx *= inv;
        sy *= inv;
        sz *= inv;
    }
    m.m[2][0] = -h[0];
    m.m[2][1] = -h[1];
    m.m[2][2] = -h[2];
    m.m[1][0] = u[0];
    m.m[1][1] = u[1];
    m.m[1][2] = u[2];
    m.m[0][0] = sx;
    m.m[0][1] = sy;
    m.m[0][2] = sz;
}

impl CarFakeShadow {
    /// `CarFakeShadow::CarFakeShadow` 0x1400e0d80. `folder_text` is the car's folder as the
    /// textures are named by; `folder` the same on disk.
    /// `wheels`: `SuspensionAvatar::getWheelTransform(i)` (for the generator).
    pub fn new(graphics: &mut Graphics, scene: &mut Scene, folder_text: &str, folder: &Path, car_node: NodeId, body_transform: NodeId, wheels: [Option<NodeId>; 4]) -> CarFakeShadow {
        let local_gl = GlRenderer::new(graphics, 6);
        let sh_fake_car_shadows = graphics.shaders.get_shader(&graphics.kgl, "ksFakeCarShadows").ok();
        let ini = IniReader::load(&folder.join("data/ambient_shadows.ini")).ok();
        let (width, length) = match &ini {
            Some(ini) => (ini.get_float("SETTINGS", "WIDTH").unwrap_or(0.0), ini.get_float("SETTINGS", "LENGTH").unwrap_or(0.0)),
            None => (2.0, 4.0),
        };
        let mut shadow = CarFakeShadow {
            width,
            length,
            tx_body: Texture::default(),
            tx_wheels: Default::default(),
            tyre_side_size: f32::from_bits(0x3e99_999a), // 0.3
            body_ref_height: 0.0,
            height_gain: 0.0,
            ambient_gain: 0.5,
            sh_fake_car_shadows,
            local_gl,
            car_node,
            body_transform,
            state: CarPhysicsState::at_origin(),
            generated: None,
        };
        if folder.join("body_shadow.png").is_file() {
            // loadShadows 0x1400e1eb0
            shadow.tx_body = graphics.resources.get_texture(&graphics.kgl, &format!("{folder_text}/body_shadow.png"));
            for i in 0..4 {
                shadow.tx_wheels[i] = graphics.resources.get_texture(&graphics.kgl, &format!("{folder_text}/tyre_{i}_shadow.png"));
            }
        } else {
            match shadow.generate_fake_shadow(graphics, scene, wheels) {
                Ok(pictures) => shadow.generated = Some(pictures),
                Err(message) => println!("WARNING: {folder_text} has no body_shadow.png and its ground shadows could not be generated: {message}"),
            }
        }
        shadow
    }

    /// `CarFakeShadow::loadShadows` 0x1400e1eb0 from a folder of one's choice (the pictures
    /// `generateFakeShadow` made, saved somewhere that is not the game's folder).
    pub fn load_shadows(&mut self, graphics: &mut Graphics, folder_text: &str) {
        self.tx_body = graphics.resources.get_texture(&graphics.kgl, &format!("{folder_text}/body_shadow.png"));
        for i in 0..4 {
            self.tx_wheels[i] = graphics.resources.get_texture(&graphics.kgl, &format!("{folder_text}/tyre_{i}_shadow.png"));
        }
    }

    /// `CarFakeShadow::generateFakeShadow` 0x1400e1430, without the saving: the pictures come
    /// back instead. Nothing is put back afterwards, as in the game (the next frame's start
    /// does that).
    fn generate_fake_shadow(&mut self, graphics: &mut Graphics, scene: &mut Scene, wheels: [Option<NodeId>; 4]) -> Result<Vec<Vec<u8>>, String> {
        use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_R8G8B8A8_UNORM;
        let k = f32::from_bits(0x3f7a_e148); // 0.98
        // an orthographic projection of +-w by +-l, near 0.1, far 500
        let ortho = |w: f32, l: f32| -> Mat44f {
            let mut p = Mat44f { m: [[0.0; 4]; 4] };
            p.m[0][0] = 2.0 / (w - (-w));
            p.m[1][1] = 2.0 / (l - (-l));
            p.m[2][2] = f32::from_bits(0xbb03_1925);
            p.m[3][0] = ((-w) + w) / ((-w) - w);
            p.m[3][1] = ((-l) + l) / ((-l) - l);
            p.m[3][2] = f32::from_bits(0xb951_c1d5);
            p.m[3][3] = 1.0;
            p
        };
        let look_up_at = |p: [f32; 3]| crate::camera::create_look_at(&rustyac_physics::vecmath::Vec3f::new(p[0], p[1] - 10.0, p[2]), &rustyac_physics::vecmath::Vec3f::new(p[0], p[1], p[2]), &rustyac_physics::vecmath::Vec3f::new(0.0, 0.0, 1.0));
        let no_camera = {
            let mut c = crate::camera::Camera::new().cull_camera();
            c.none = true;
            c
        };
        // the body
        let body = graphics.kgl.create_render_target(DXGI_FORMAT_R8G8B8A8_UNORM, 512, 512, 1, 1);
        graphics.kgl.set_render_target(Some(&body));
        graphics.kgl.set_viewport(0.0, 0.0, 512.0, 512.0);
        graphics.kgl.clear_color(&[0.0, 0.0, 0.0, 1.0]);
        graphics.kgl.clear_depth(1.0);
        graphics.set_blend_mode(0);
        graphics.set_depth_mode(0);
        graphics.set_projection_matrix(&ortho(self.width * k, self.length * k));
        let m = &scene.nodes[self.body_transform].matrix.m;
        graphics.set_view_matrix(&look_up_at([m[3][0], m[3][1], m[3][2]]), None);
        let mut filter = MaterialFilter::Fake;
        let shader = graphics.shaders.get_shader(&graphics.kgl, "ksFakeCarShadowsGen")?;
        graphics.set_shader(shader);
        scene.traverse(self.body_transform);
        {
            let s = graphics.shaders.get_mut(shader);
            for (name, value) in [("refHeight", self.body_ref_height), ("heightGain", self.height_gain), ("gain", self.ambient_gain)] {
                if let Some(v) = s.get_var(name) {
                    s.set_var(v, &value.to_le_bytes());
                }
            }
            if let Some(b) = s.get_var("gain").and_then(|v| s.vars[v].buffer) {
                s.cbuffers[b].commit(&graphics.kgl);
            }
        }
        graphics.commit_shader_changes();
        {
            let mut rc = RenderContext { material_filter: &mut filter, pass_id: PASS_OPAQUE, max_layer: 5, camera: no_camera };
            scene.render(graphics, self.body_transform, &mut rc);
        }
        // the wheels
        let side = self.tyre_side_size * k;
        graphics.set_projection_matrix(&ortho(side, side));
        let mut pictures = Vec::new();
        for wheel in wheels {
            let target = graphics.kgl.create_render_target(DXGI_FORMAT_R8G8B8A8_UNORM, 64, 64, 1, 1);
            let Some(wheel) = wheel else { return Err("a wheel has no node".into()) };
            scene.traverse(wheel);
            graphics.kgl.set_render_target(Some(&target));
            graphics.kgl.set_viewport(0.0, 0.0, 64.0, 64.0);
            graphics.clear_render_target(&[0.0, 0.0, 0.0, 1.0]);
            graphics.clear_render_target_depth(1.0);
            let w = scene.get_world_matrix(wheel);
            graphics.set_view_matrix(&look_up_at([w.m[3][0], w.m[3][1], w.m[3][2]]), None);
            let mut rc = RenderContext { material_filter: &mut filter, pass_id: PASS_OPAQUE, max_layer: 5, camera: no_camera };
            scene.render(graphics, wheel, &mut rc);
            pictures.push(graphics.kgl.read_render_target(&target)?);
        }
        pictures.push(graphics.kgl.read_render_target(&body)?);
        Ok(pictures)
    }
}

impl NodeEventHandler for CarFakeShadow {
    /// `CarFakeShadow::onNodeRenderEvent` 0x1400e22b0.
    fn on_node_render(&mut self, scene: &mut Scene, graphics: &mut Graphics, event: &OnNodeRenderEvent) {
        if event.pass_id != PASS_OPAQUE {
            return;
        }
        // CarAvatar::isConnected 0x1400d8b40
        if !scene.nodes[self.car_node].is_active || event.camera.is_cube_map_camera {
            return;
        }
        let gl = &mut self.local_gl;
        gl.begin(GL_QUADS, self.sh_fake_car_shadows);
        gl.color3f(3.0, 0.0, 3.0);
        graphics.set_world_matrix(&Mat44f::IDENTITY);
        graphics.set_texture(0, &self.tx_body);
        graphics.set_depth_mode(DEPTH_NO_WRITE);
        graphics.set_blend_mode(BLEND_ALPHA);
        graphics.set_cull_mode(CULL_FRONT);
        let b = &scene.nodes[self.body_transform].matrix.m;
        let (w, l) = (self.width, self.length);
        let corner = |x: f32, z: f32| -> [f32; 3] {
            [
                ((x * b[0][0] + b[1][0] * 0.0) + z * b[2][0]) + b[3][0],
                ((x * b[0][1] + b[1][1] * 0.0) + z * b[2][1]) + b[3][1],
                ((x * b[0][2] + b[1][2] * 0.0) + z * b[2][2]) + b[3][2],
            ]
        };
        let mut corners = [corner(-w, -l), corner(-w, l), corner(w, l), corner(w, -l)];
        // each corner straight down (or up) onto the plane through the tyres' contact points
        let [nx, ny, nz, d] = self.state.ground_plane;
        for c in &mut corners {
            let den = (nx * 0.0 + ny * -1.0) + nz * 0.0;
            let y = if den != 0.0 && !den.is_nan() { ((((nx * c[0] + ny * c[1]) + nz * c[2]) + d) * (-1.0 / den)) * -1.0 + c[1] } else { 0.0 };
            c[1] = y + f32::from_bits(0x3c23_d70a);
        }
        for (uv, c) in [(0.0, 1.0), (0.0, 0.0), (1.0, 0.0), (1.0, 1.0)].iter().zip(&corners) {
            gl.tex_coord2f(uv.0, uv.1);
            gl.vertex3fv(c);
        }
        gl.end(graphics);
        for i in 0..4 {
            gl.begin(GL_QUADS, self.sh_fake_car_shadows);
            graphics.set_texture(0, &self.tx_wheels[i]);
            let mut m = self.state.suspension_matrix[i];
            let n = self.state.tyre_contact_normal[i];
            // the hub's forward direction laid into the ground
            let (fx, fy, fz) = (-m.m[2][0], -m.m[2][1], -m.m[2][2]);
            let dot = (fx * n[0] + fy * n[1]) + fz * n[2];
            let mut h = [fx - dot * n[0], fy - dot * n[1], fz - dot * n[2]];
            let len = sqrtf((h[1] * h[1] + h[0] * h[0]) + h[2] * h[2]);
            if len != 0.0 && !len.is_nan() {
                let inv = 1.0 / len;
                h = [inv * h[0], h[1] * inv, h[2] * inv];
            }
            let (px, pz) = (m.m[3][0], m.m[3][2]);
            set_from_heading_up(&mut m, &h, &n);
            m.m[3][0] = px;
            m.m[3][2] = pz;
            m.m[3][1] = self.state.tyre_contact_point[i][1];
            graphics.set_world_matrix(&m);
            let s = self.tyre_side_size;
            let y = f32::from_bits(0x3c23_d70a);
            gl.tex_coord2f(0.0, 1.0);
            gl.vertex3f(-s, y, -s);
            gl.tex_coord2f(0.0, 0.0);
            gl.vertex3f(-s, y, s);
            gl.tex_coord2f(1.0, 0.0);
            gl.vertex3f(s, y, s);
            gl.tex_coord2f(1.0, 1.0);
            gl.vertex3f(s, y, -s);
            gl.end(graphics);
        }
        graphics.set_depth_mode(DEPTH_NORMAL);
        graphics.set_cull_mode(CULL_FRONT);
    }
}
