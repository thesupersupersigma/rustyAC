// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! `Camera` and `CameraShadowMapped`: the projection and view matrices, the three shadow
//! cascades and the main pass (sky, opaque meshes, transparent meshes).
//! `Camera::getPerspectiveMatrix` 0x14020ef00, `Camera::getViewMatrix` 0x14020eff0,
//! `mat44f::createLookAt` 0x140024090, `Camera::renderCamera` 0x14020f5d0,
//! `CameraShadowMapped::shadowMapPass` 0x14020d6a0, `beginShadowMapPass` 0x14020c4f0,
//! `createShadowMapMatrix` 0x14020c790, `renderPass` 0x14020cf20.

use rustyac_math::{sqrtf, tanf};
use rustyac_physics::vecmath::{xm_matrix_inverse, xm_matrix_multiply, Mat44f, Vec3f};

use crate::graphics::{Graphics, RenderTarget, CULL_BACK, CULL_FRONT, DEPTH_NORMAL, DEPTH_NO_WRITE, DEPTH_OFF};
use crate::material::{MaterialFilter, PASS_OPAQUE, PASS_SHADOW, PASS_TRANSPARENT};
use crate::scene::{BoundingFrustum, CullCamera, NodeId, RenderContext, Scene};
use crate::sky::SkyBox;

/// The game's degrees-to-radians factor: not pi / 180 rounded, this literal.
pub const DEG_TO_RAD: f32 = f32::from_bits(0x3c8e_f998);

/// `mat44f::createLookAt` 0x140024090: right-handed, components divided by the length.
#[allow(clippy::neg_cmp_op_on_partial_ord)]
pub fn create_look_at(eye: &Vec3f, target: &Vec3f, up: &Vec3f) -> Mat44f {
    let mut zx = eye.x - target.x;
    let mut zz = eye.z - target.z;
    let mut zy = eye.y - target.y;
    let len = sqrtf((zy * zy + zx * zx) + zz * zz);
    if len != 0.0 {
        zx /= len;
        zy /= len;
        zz /= len;
    }
    let mut xx = up.y * zz - up.z * zy;
    let mut xy = up.z * zx - up.x * zz;
    let mut xz = up.x * zy - up.y * zx;
    let len = sqrtf((xy * xy + xx * xx) + xz * xz);
    if len != 0.0 {
        xx /= len;
        xy /= len;
        xz /= len;
    }
    let mut yx = xz * zy - zz * xy;
    let mut yy = zz * xx - xz * zx;
    let mut yz = xy * zx - zy * xx;
    let len = sqrtf((yy * yy + yx * yx) + yz * yz);
    if len != 0.0 {
        yx /= len;
        yy /= len;
        yz /= len;
    }
    Mat44f {
        m: [
            [xx, yx, zx, 0.0],
            [xy, yy, zy, 0.0],
            [xz, yz, zz, 0.0],
            [-((eye.y * xy + eye.x * xx) + eye.z * xz), -((eye.y * yy + eye.x * yx) + eye.z * yz), -((eye.y * zy + eye.x * zx) + eye.z * zz), 1.0],
        ],
    }
}

/// The projection of `Camera::getPerspectiveMatrix`: `D3DXMatrixPerspectiveFovRH`, the field
/// of view in degrees.
fn perspective(fov: f32, aspect: f32, zn: f32, zf: f32) -> Mat44f {
    let t = tanf((fov * DEG_TO_RAD) * 0.5);
    let ys = 1.0 / t;
    let xs = ys / aspect;
    let inv = 1.0 / (zn - zf);
    let mut out = Mat44f::default();
    out.m[0][0] = xs;
    out.m[1][1] = ys;
    out.m[2][2] = inv * zf;
    out.m[2][3] = -1.0;
    out.m[3][2] = (zn * zf) * inv;
    out
}

/// `BoundingFrustum::getCorners` 0x140228ac0: the eight corners, near then far.
pub fn frustum_corners(view_proj: &Mat44f) -> Vec<Vec3f> {
    let inverse = xm_matrix_inverse(view_proj);
    let i = &inverse.m;
    let mut out = Vec::with_capacity(8);
    for far in [false, true] {
        for (sx, sy) in [(-1.0f32, -1.0f32), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
            let mut v = [0.0f32; 4];
            for (c, slot) in v.iter_mut().enumerate() {
                let x = if sx == 1.0 { i[0][c] } else { sx * i[0][c] };
                let y = if sy == 1.0 { i[1][c] } else { sy * i[1][c] };
                let z = if far { i[2][c] } else { i[2][c] * 0.0 };
                *slot = ((x + y) + z) + i[3][c];
            }
            let inv = 1.0 / v[3];
            out.push(Vec3f::new(inv * v[0], inv * v[1], inv * v[2]));
        }
    }
    out
}

/// `Camera` (0x210 bytes).
pub struct Camera {
    /// degrees, vertical
    pub fov: f32,
    /// the camera's world matrix; row 3 is where it is, row 2 points backwards
    pub matrix: Mat44f,
    pub near_plane: f32,
    pub far_plane: f32,
    pub clear_mode: i32,
    pub clear_color: [f32; 4],
    pub clear_depth: f32,
    pub frustum: BoundingFrustum,
    pub exposure: f32,
    /// -1: the screen's
    pub aspect_ratio: f32,
    pub max_layer: f32,
    pub is_mirror: bool,
    pub lod_multiplier: f32,
    pub is_cube_map_camera: bool,
}

impl Camera {
    /// `Camera::Camera` 0x14020ea40.
    pub fn new() -> Camera {
        let mut frustum = BoundingFrustum::default();
        frustum.set_matrix(&Mat44f::IDENTITY);
        Camera {
            fov: 60.0,
            matrix: Mat44f::IDENTITY,
            near_plane: 0.1,
            far_plane: 5000.0,
            clear_mode: 2,
            clear_color: [0.0, 0.0, 0.0, 1.0],
            clear_depth: 1.0,
            frustum,
            exposure: 40.0,
            aspect_ratio: -1.0,
            max_layer: 5.0,
            is_mirror: false,
            lod_multiplier: 1.0,
            is_cube_map_camera: false,
        }
    }

    /// `Camera::getPerspectiveMatrix` 0x14020ef00.
    pub fn get_perspective_matrix(&self, graphics: &Graphics) -> Mat44f {
        let mut a = self.aspect_ratio;
        if a == -1.0 {
            a = graphics.video.width as f32 / graphics.video.height as f32;
        }
        perspective(self.fov, a, self.near_plane, self.far_plane)
    }

    /// `Camera::getViewMatrix` 0x14020eff0.
    pub fn get_view_matrix(&self) -> Mat44f {
        let m = &self.matrix.m;
        let eye = Vec3f::new(m[3][0], m[3][1], m[3][2]);
        let target = Vec3f::new(m[3][0] + (-m[2][0]), m[3][1] + (-m[2][1]), m[3][2] + (-m[2][2]));
        let up = Vec3f::new(m[1][0], m[1][1], m[1][2]);
        create_look_at(&eye, &target, &up)
    }

    /// `Camera::renderCamera` 0x14020f5d0.
    pub fn render_camera(&mut self, graphics: &mut Graphics) {
        let mut proj = self.get_perspective_matrix(graphics);
        if self.is_mirror {
            let mut flip = Mat44f::IDENTITY;
            flip.m[0][0] = -1.0;
            proj = xm_matrix_multiply(&flip, &proj);
        }
        let view = self.get_view_matrix();
        graphics.set_projection_matrix(&proj);
        graphics.set_view_matrix(&view, Some(self));
        self.frustum.set_matrix(&xm_matrix_multiply(&view, &proj));
    }

    /// `Camera::clearBuffers` 0x14020ee60.
    pub fn clear_buffers(&self, graphics: &Graphics) {
        match self.clear_mode {
            2 => {
                graphics.clear_render_target(&self.clear_color);
                graphics.clear_render_target_depth(self.clear_depth);
            }
            1 => graphics.clear_render_target_depth(self.clear_depth),
            0 => graphics.clear_render_target(&self.clear_color),
            _ => {}
        }
    }

    /// What the mesh filter and the bounding-sphere nodes read of the camera right now.
    pub fn cull_camera(&self) -> CullCamera {
        CullCamera { fov: self.fov, lod_multiplier: self.lod_multiplier, position: [self.matrix.m[3][0], self.matrix.m[3][1], self.matrix.m[3][2]], frustum: self.frustum, matrix: self.matrix, is_cube_map_camera: self.is_cube_map_camera, is_mirror: self.is_mirror }
    }
}

impl Default for Camera {
    fn default() -> Camera {
        Camera::new()
    }
}

/// `ShadowMapSettings`: one cascade's range in front of the near plane.
#[derive(Clone, Copy, Debug, Default)]
pub struct ShadowMapSettings {
    pub near_split: f32,
    pub far_split: f32,
    pub height: f32,
}

/// `CameraShadowMapped` (0x2a0 bytes).
pub struct CameraShadowMapped {
    pub camera: Camera,
    pub shadow_map_settings: [ShadowMapSettings; 4],
    pub shadow_bias: f32,
    pub shadow_rt: Vec<RenderTarget>,
    pub shadow_matrices: [Mat44f; 3],
    pub sky_box: Option<SkyBox>,
}

impl CameraShadowMapped {
    /// `CameraShadowMapped::CameraShadowMapped` 0x14020bdd0.
    pub fn new(graphics: &mut Graphics) -> Result<CameraShadowMapped, String> {
        let shadow_bias = f32::from_bits(0x3b44_9ba6); // 0.003
        let size = graphics.video.shadow_map_size;
        let mut shadow_rt = Vec::new();
        for _ in 0..3 {
            // RenderTarget(graphics, format 3, size, size, no colour, depth, 1 mip)
            let depth = graphics.kgl.create_render_target_depth(size, size, 1)?;
            shadow_rt.push(RenderTarget { kid_color: None, kid_depth: Some(depth), width: size, height: size });
        }
        let mut settings = [ShadowMapSettings::default(); 4];
        settings[0] = ShadowMapSettings { near_split: 0.0, far_split: f32::from_bits(0x3fe6_6666), height: 0.0 };
        settings[1] = ShadowMapSettings { near_split: f32::from_bits(0x3fe6_6666), far_split: 20.0, height: 0.0 };
        settings[2] = ShadowMapSettings { near_split: 20.0, far_split: 180.0, height: 0.0 };
        settings[3] = ShadowMapSettings { near_split: 180.0, far_split: 500.0, height: 0.0 };
        let k = f32::from_bits(0x3a83_126f); // 0.001
        graphics.set_shadow_map_bias((shadow_bias * f32::from_bits(0x3fe6_6666)) * k, (shadow_bias * f32::from_bits(0x4191_999a)) * k, (shadow_bias * 160.0) * k);
        if let Ok(ini) = rustyac_physics::data::ini::IniReader::load(&graphics.game_folder.join("system/cfg/lighting.ini")) {
            if ini.has_section("LIGHT") {
                for s in &mut settings {
                    s.height = ini.get_float("LIGHT", "LIGHT_HEIGHT").unwrap_or(0.0);
                }
            }
        }
        Ok(CameraShadowMapped { camera: Camera::new(), shadow_map_settings: settings, shadow_bias, shadow_rt, shadow_matrices: [Mat44f::default(); 3], sky_box: None })
    }

    /// `CameraShadowMapped::setShadowMapsSplits` 0x14020d5f0: what the game camera of the
    /// moment (chase, cockpit, track side …) calls every frame.
    pub fn set_shadow_maps_splits(&mut self, graphics: &mut Graphics, a: f32, b: f32, c: f32, d: f32) {
        let s = &mut self.shadow_map_settings;
        s[0].near_split = 0.0;
        s[0].far_split = a;
        s[1].near_split = a;
        s[1].far_split = b;
        s[2].near_split = b;
        s[2].far_split = c;
        s[3].near_split = c;
        s[3].far_split = d;
        let k = f32::from_bits(0x3a83_126f);
        graphics.set_shadow_map_bias((self.shadow_bias * a) * k, ((b - a) * self.shadow_bias) * k, ((c - b) * self.shadow_bias) * k);
    }

    /// `CameraShadowMapped::shadowMapPass` 0x14020d6a0.
    pub fn shadow_map_pass(&mut self, graphics: &mut Graphics, scene: &mut Scene, node: NodeId) -> Result<(), String> {
        graphics.set_depth_mode(DEPTH_NORMAL);
        graphics.set_blend_mode(0);
        let mut filter = MaterialFilter::shadow_map(graphics)?;
        let max_layer = self.camera.max_layer as i32;
        let cam_matrix = self.camera.matrix;
        for i in 0..3 {
            self.begin_shadow_map_pass(graphics, i, &cam_matrix);
            let mut rc = RenderContext { material_filter: &mut filter, pass_id: PASS_SHADOW, max_layer, camera: self.camera.cull_camera() };
            scene.render(graphics, node, &mut rc);
            graphics.kgl.set_render_targets(None, None);
            graphics.set_shadow_map_texture(i as i32, Some(&self.shadow_rt[i]));
            graphics.set_shadow_map_matrix(i as i32, &self.shadow_matrices[i]);
        }
        Ok(())
    }

    /// `CameraShadowMapped::beginShadowMapPass` 0x14020c4f0.
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    fn begin_shadow_map_pass(&mut self, graphics: &mut Graphics, i: usize, cam_matrix: &Mat44f) {
        graphics.set_cull_mode(CULL_BACK);
        graphics.set_depth_mode(DEPTH_NORMAL);
        let mut l = graphics.lighting.light_direction;
        let len = sqrtf((l.x * l.x + l.y * l.y) + l.z * l.z);
        if len != 0.0 {
            let inv = 1.0 / len;
            l.x *= inv;
            l.y *= inv;
            l.z *= inv;
        }
        let zn = self.camera.near_plane + self.shadow_map_settings[i].near_split;
        let zf = self.camera.near_plane + self.shadow_map_settings[i].far_split;
        let (view, proj) = self.create_shadow_map_matrix(graphics, cam_matrix, zn, zf, &l);
        graphics.set_projection_matrix(&proj);
        graphics.set_view_matrix(&view, None);
        self.shadow_matrices[i] = xm_matrix_multiply(&view, &proj);
        self.camera.frustum.set_matrix(&self.shadow_matrices[i]);
        graphics.set_shadow_map_texture(i as i32, None);
        graphics.kgl.set_render_targets(None, self.shadow_rt[i].kid_depth.as_ref());
        graphics.set_viewport(0, 0, self.shadow_rt[i].width, self.shadow_rt[i].height);
        if let Some(depth) = &self.shadow_rt[i].kid_depth {
            graphics.kgl.render_target_clear(depth, &[0.0; 4], 1.0);
        }
    }

    /// `CameraShadowMapped::createShadowMapMatrix` 0x14020c790: the light looks from where the
    /// camera is along the sun's direction; the box is the bounding rectangle of the camera's
    /// sub-frustum, with a fixed depth range.
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    fn create_shadow_map_matrix(&self, graphics: &Graphics, cam_matrix: &Mat44f, zn: f32, zf: f32, l: &Vec3f) -> (Mat44f, Mat44f) {
        let c = &cam_matrix.m;
        let pos = Vec3f::new(c[3][0], c[3][1], c[3][2]);
        let aspect = graphics.video.width as f32 / graphics.video.height as f32;
        let p = perspective(self.camera.fov, aspect, zn, zf);
        let up0 = Vec3f::new(c[1][0], c[1][1], c[1][2]);
        let target = Vec3f::new((-c[2][0]) + pos.x, (-c[2][1]) + pos.y, (-c[2][2]) + pos.z);
        let v0 = create_look_at(&pos, &target, &up0);
        let corners = frustum_corners(&xm_matrix_multiply(&v0, &p));
        let look = Vec3f::new(c[2][0], c[2][1], c[2][2]);
        let d = (look.x * l.x + look.y * l.y) + look.z * l.z;
        let mut u = Vec3f::new(look.x - d * l.x, look.y - d * l.y, look.z - d * l.z);
        let n2 = (u.y * u.y + u.x * u.x) + u.z * u.z;
        if n2 == 0.0 || n2.is_nan() {
            u = Vec3f::new(1.0, 0.0, 0.0);
        }
        let len = sqrtf((u.y * u.y + u.x * u.x) + u.z * u.z);
        if len != 0.0 {
            let inv = 1.0 / len;
            u.x *= inv;
            u.y *= inv;
            u.z *= inv;
        }
        let tgt = Vec3f::new(pos.x + l.x, l.y + pos.y, l.z + pos.z);
        let view = create_look_at(&pos, &tgt, &u);
        let v = &view.m;
        let (mut min_x, mut min_y) = (f32::from_bits(0x4b18_967f), f32::from_bits(0x4974_23f0));
        let (mut max_x, mut max_y) = (f32::from_bits(0xcb18_967f), f32::from_bits(0xc974_23f0));
        for corner in &corners {
            let x = ((corner.x * v[0][0] + corner.y * v[1][0]) + corner.z * v[2][0]) + v[3][0];
            let y = ((corner.x * v[0][1] + corner.y * v[1][1]) + corner.z * v[2][1]) + v[3][1];
            // `comiss a, b; jae skip` / `jbe skip`
            if !(x >= min_x) {
                min_x = x;
            }
            if !(y >= min_y) {
                min_y = y;
            }
            if x > max_x {
                max_x = x;
            }
            if y > max_y {
                max_y = y;
            }
        }
        let mut proj = Mat44f::default();
        proj.m[0][0] = 2.0 / (max_x - min_x);
        proj.m[1][1] = 2.0 / (max_y - min_y);
        proj.m[2][2] = f32::from_bits(0xba83_126f);
        proj.m[3][0] = (max_x + min_x) / (min_x - max_x);
        proj.m[3][1] = (max_y + min_y) / (min_y - max_y);
        proj.m[3][2] = 0.5;
        proj.m[3][3] = 1.0;
        (view, proj)
    }

    /// `CameraShadowMapped::renderPass` 0x14020cf20: sky, opaque meshes, transparent meshes.
    pub fn render_pass(&mut self, graphics: &mut Graphics, scene: &mut Scene, node: NodeId) {
        graphics.set_depth_mode(DEPTH_NORMAL);
        let max_layer = self.camera.max_layer as i32;
        let mut filter = MaterialFilter::plain();
        graphics.set_cull_mode(CULL_FRONT);
        self.camera.render_camera(graphics);
        self.camera.clear_buffers(graphics);
        if let Some(sky) = &mut self.sky_box {
            graphics.set_world_matrix(&Mat44f::IDENTITY);
            graphics.set_depth_mode(DEPTH_OFF);
            let mut rc = RenderContext { material_filter: &mut filter, pass_id: PASS_OPAQUE, max_layer, camera: self.camera.cull_camera() };
            sky.render(graphics, &mut rc, self.camera.is_cube_map_camera);
            graphics.set_depth_mode(DEPTH_NORMAL);
        }
        {
            let mut rc = RenderContext { material_filter: &mut filter, pass_id: PASS_OPAQUE, max_layer, camera: self.camera.cull_camera() };
            scene.render(graphics, node, &mut rc);
        }
        graphics.set_depth_mode(DEPTH_NO_WRITE);
        {
            let mut rc = RenderContext { material_filter: &mut filter, pass_id: PASS_TRANSPARENT, max_layer, camera: self.camera.cull_camera() };
            scene.render(graphics, node, &mut rc);
        }
        graphics.set_depth_mode(DEPTH_NORMAL);
    }
}
