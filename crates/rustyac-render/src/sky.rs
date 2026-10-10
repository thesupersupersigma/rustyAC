// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! The sky dome: `SkyBox::SkyBox` 0x14021c790, `SkyBox::render` 0x14021d0d0,
//! `ShapeBuilder::buildHemiSphere` 0x1402267d0, `ShapeBuilder::getSpherePos` 0x140226e90.
//!
//! The dome is not a file: 91 quads of a hemisphere of 10 m, generated at start-up. It is drawn
//! first in the main pass with the shader `ksSky`, whose vertex shader moves it to the camera.
//!
//! The clouds: `SkyBox::updateCloudsGeneration` 0x14021db00, `updateCloudsAnimation`
//! 0x14021dad0, `renderClouds` 0x14021d4b0. Quads on a sphere around the camera, turned to
//! face it, drawn after the dome in every pass that has a sky. `SunAnimator` (0x88 bytes;
//! constructor 0x1401ad890, `update` 0x1401adc10) lets them drift and moves the sun.

use rustyac_math::{asinf, atan2f, cosf, sinf, sqrtf};
use rustyac_physics::data::ini::IniReader;
use rustyac_physics::vecmath::{Mat44f, Vec3f};

use crate::camera::DEG_TO_RAD;
use crate::gl::GL_QUADS;
use crate::graphics::{Graphics, BLEND_ALPHA, CULL_FRONT, DEPTH_OFF};
use crate::kgl::{KglIndexBuffer, KglVertexBuffer};
use crate::material::{Material, MaterialId};
use crate::scene::{RenderContext, Sphere};
use crate::shader::ShaderId;
use crate::texture::Texture;

/// The sky dome's material is not one of a model: it gets a name of its own for the filter.
const SKY_MATERIAL: MaterialId = MaterialId(u32::MAX);

/// The game's inline `floorf`: toward minus infinity, a whole number is kept as it is.
fn floor_inline(t: f32) -> f32 {
    let i = t as i32; // cvttss2si (the cases that overflow keep `t`)
    if !(-2147483648.0..2147483648.0).contains(&t) || t.is_nan() {
        return t;
    }
    if i as f32 != t {
        let i = i - if t.is_sign_negative() { 1 } else { 0 };
        return i as f32;
    }
    t
}

/// `ShapeBuilder::getSpherePos` 0x140226e90: a point of the unit sphere from two angles in
/// degrees, rounded down to thousandths.
fn get_sphere_pos(a_deg: f32, b_deg: f32) -> Vec3f {
    let m = Mat44f::create_from_axis_angle(&Vec3f::new(0.0, 1.0, 0.0), a_deg * DEG_TO_RAD);
    let mut v = [0.0f32; 3];
    for (c, slot) in v.iter_mut().enumerate() {
        *slot = (((m.m[1][c] * 0.0) + (m.m[0][c] * 0.0)) + (m.m[2][c] * -1.0)) + m.m[3][c];
    }
    let axis = Vec3f::new((v[1] * 0.0) - (v[2] * -1.0), (v[2] * 0.0) - (v[0] * 0.0), (v[0] * -1.0) - (v[1] * 0.0));
    let n = Mat44f::create_from_axis_angle(&axis, -(b_deg * DEG_TO_RAD));
    let mut w = [0.0f32; 3];
    for (c, slot) in w.iter_mut().enumerate() {
        *slot = (((v[0] * n.m[0][c]) + (v[1] * n.m[1][c])) + (v[2] * n.m[2][c])) + n.m[3][c];
    }
    let len = sqrtf(((w[1] * w[1]) + (w[0] * w[0])) + (w[2] * w[2]));
    if len != 0.0 && !len.is_nan() {
        let inv = 1.0 / len;
        w[1] *= inv;
        w[0] = inv * w[0];
        w[2] *= inv;
    }
    for c in &mut w {
        let t = floor_inline(*c * 1000.0);
        *c = t * f32::from_bits(0x3a83_126f);
    }
    Vec3f::new(w[0], w[1], w[2])
}

/// `ShapeBuilder::buildHemiSphere` 0x1402267d0: the vertex records (44 bytes each) and the
/// indices.
pub fn build_hemi_sphere(radius: f32, n: i32) -> (Vec<u8>, Vec<u16>) {
    const INV_PI: f32 = f32::from_bits(0x3ea2_f982);
    // position, normal, texture coordinate; the tangent stays zero
    let mut vertices: Vec<([f32; 3], [f32; 3], [f32; 2])> = Vec::new();
    let mut indices: Vec<u16> = Vec::new();
    let inv = 1.0 / n as f32;
    let step_b = inv * 180.0;
    let step_a = inv * 360.0;
    let mut quad = 0u32;
    for i in -1..n {
        let a1 = (i + 1) as f32 * step_a;
        let a0 = i as f32 * step_a;
        for j in -1..n / 2 {
            let b0 = j as f32 * step_b;
            let b1 = (j + 1) as f32 * step_b;
            let points = [get_sphere_pos(a0, b0), get_sphere_pos(a1, b0), get_sphere_pos(a1, b1), get_sphere_pos(a0, b1)];
            for p in points {
                vertices.push(([p.x * radius, p.y * radius, p.z * radius], [p.x, p.y, p.z], [0.0, 0.0]));
            }
            let count = vertices.len();
            let (v0, v1, v2, v3) = (count - 4, count - 3, count - 2, count - 1);
            for v in [v3, v2, v1, v0] {
                let normal = vertices[v].1;
                vertices[v].2[0] = (asinf(normal[1]) * INV_PI) + 0.5;
                vertices[v].2[1] = ((atan2f(normal[2], normal[0]) * INV_PI) + 1.0) * 0.5;
            }
            // the seam
            if (vertices[v1].2[1] - vertices[v0].2[1]).abs() > 0.1 || (vertices[v3].2[1] - vertices[v2].2[1]).abs() > 0.1 {
                vertices[v2].2[1] -= 1.0;
                vertices[v1].2[1] -= 1.0;
            }
            // the pole (`comiss; jae` skips: a NaN passes)
            if !((vertices[v3].2[0] - 1.0).abs() >= f32::from_bits(0x3a83_126f)) && !((vertices[v2].2[0] - 1.0).abs() >= f32::from_bits(0x3a83_126f)) {
                vertices[v3].2[1] = (vertices[v0].2[1] + vertices[v1].2[1]) * 0.5;
                vertices[v2].2[1] = (vertices[v0].2[1] + vertices[v1].2[1]) * 0.5;
            }
            // ShapeBuilder::addQuadIndicesToMesh 0x1402264d0
            let base = (4 * quad) as u16;
            indices.extend([base, base + 2, base + 1, base, base + 3, base + 2]);
            quad += 1;
        }
    }
    let mut bytes = Vec::with_capacity(vertices.len() * 44);
    for (position, normal, uv) in &vertices {
        for v in position.iter().chain(normal).chain(uv).chain(&[0.0f32; 3]) {
            bytes.extend(v.to_le_bytes());
        }
    }
    (bytes, indices)
}

/// `CloudSettings`.
#[derive(Clone, Copy, Debug)]
pub struct CloudSettings {
    pub cloud_width: f32,
    pub cloud_height: f32,
    pub cloud_radius: f32,
    pub base_speed_mult: f32,
    pub number_of_clouds: i32,
}

/// `CloudBillboard` (0x38 bytes): `theta` is the angle from straight up, `phi` the azimuth.
#[derive(Clone, Debug)]
pub struct CloudBillboard {
    pub theta_rad: f32,
    pub phi_rad: f32,
    pub radius_mt: f32,
    pub speed_mult: f32,
    pub texture: Texture,
}

/// `SkyBox` (0x80 bytes).
pub struct SkyBox {
    pub settings: CloudSettings,
    pub cloud_textures: Vec<Texture>,
    pub clouds: Vec<CloudBillboard>,
    /// `commonBoundingSphere`: its radius
    sphere_radius: f32,
    vb: KglVertexBuffer,
    ib: KglIndexBuffer,
    index_count: i32,
    pub material: Material,
    pub clouds_shader: ShaderId,
    pub cubemap_sky_shader: ShaderId,
    pub skybox_gain: f32,
}

impl SkyBox {
    /// `SkyBox::SkyBox` 0x14021c790.
    ///
    /// `weather` is race.ini `[WEATHER] NAME` when the file has that section: the clouds are
    /// generated a first time here (and their textures made before the two shaders).
    pub fn new(graphics: &mut Graphics, weather: Option<&str>) -> Result<SkyBox, String> {
        let (vertices, indices) = build_hemi_sphere(10.0, 12);
        let mut material = Material::new("SKYBOX MATERIAL");
        material.set_shader(graphics, "ksSky")?;
        material.depth_mode = DEPTH_OFF;
        material.blend_mode = 0;
        // graphics->compile(mesh)
        let vb = graphics.kgl.create_vertex_buffer(&vertices, vertices.len(), 44, false);
        let ib = graphics.kgl.create_index_buffer(&indices);
        let mut sky = SkyBox {
            settings: CloudSettings { cloud_width: 4.0, cloud_height: 2.0, cloud_radius: 4.0, base_speed_mult: f32::from_bits(0x3c23_d70a), number_of_clouds: 100 },
            cloud_textures: Vec::new(),
            clouds: Vec::new(),
            sphere_radius: 0.0,
            vb,
            ib,
            index_count: indices.len() as i32,
            material,
            clouds_shader: ShaderId(0),
            cubemap_sky_shader: ShaderId(0),
            skybox_gain: 1.0,
        };
        if let Some(weather) = weather {
            sky.update_clouds_generation(graphics, weather);
        }
        sky.clouds_shader = graphics.shaders.get_shader(&graphics.kgl, "ksClouds")?;
        sky.cubemap_sky_shader = graphics.shaders.get_shader(&graphics.kgl, "ksSkyCubemap")?;
        if let Ok(ini) = IniReader::load(&graphics.game_folder.join("system/cfg/graphics.ini")) {
            sky.skybox_gain = ini.get_float("DX11", "SKYBOX_REFLECTION_GAIN").unwrap_or(0.0);
        }
        Ok(sky)
    }

    /// `SkyBox::updateCloudsGeneration` 0x14021db00: the clouds of `content/weather/<name>`,
    /// drawn from the main thread's `rand()`.
    pub fn update_clouds_generation(&mut self, graphics: &mut Graphics, weather: &str) {
        println!("Update Clouds Generation to {weather} ");
        self.clouds.clear();
        self.cloud_textures.clear();
        if let Ok(ini) = IniReader::load(&graphics.game_folder.join("content/weather").join(weather).join("weather.ini")) {
            if ini.has_section("CLOUDS") {
                let get = |key: &str| ini.get_float("CLOUDS", key).unwrap_or(0.0);
                self.settings.cloud_height = get("HEIGHT");
                self.settings.cloud_width = get("WIDTH");
                self.settings.cloud_radius = get("RADIUS");
                self.settings.number_of_clouds = (get("NUMBER") * (graphics.video.world_detail as f32 * f32::from_bits(0x3e4c_cccd))) as i32;
                self.settings.base_speed_mult = get("BASE_SPEED_MULT");
            }
        }
        // Path::getFiles: the files of the folder itself, in the order of their names
        let folder = graphics.game_folder.join("content/texture/clouds");
        let mut names: Vec<String> = std::fs::read_dir(&folder)
            .map(|d| d.filter_map(|e| e.ok()).filter(|e| e.path().is_file()).map(|e| e.file_name().to_string_lossy().into_owned()).filter(|n| n.to_ascii_lowercase().ends_with(".dds")).collect())
            .unwrap_or_default();
        names.sort_by_key(|n| n.to_ascii_lowercase());
        for name in names {
            let path = format!("{}/content/texture/clouds/{name}", crate::model::path_text(&graphics.game_folder));
            let texture = graphics.resources.get_texture(&graphics.kgl, &path);
            self.cloud_textures.push(texture);
        }
        let k = f32::from_bits(0x3800_0100);
        let n = self.settings.number_of_clouds;
        let mut i = 0u32;
        while n > 0 && i < n as u32 {
            let mut a = graphics.crt_rand.next() as f32 * k;
            a -= 0.5;
            a *= 10.0;
            a += 360.0 / n as f32;
            a *= DEG_TO_RAD;
            let phi = a * i as f32;
            let mut t6 = graphics.crt_rand.next() as f32 * k;
            i += 1;
            let mut x = graphics.crt_rand.next() as f32 * k;
            x -= 0.5;
            x *= 5.0;
            x += 15.0;
            x *= DEG_TO_RAD;
            x *= (i % 5) as f32;
            t6 -= 0.5;
            t6 *= 30.0;
            t6 += 20.0;
            t6 *= DEG_TO_RAD;
            let theta = x + t6;
            let radius = ((1.0 - cosf(theta)) * 4.0) + self.settings.cloud_radius;
            #[allow(clippy::neg_cmp_op_on_partial_ord)]
            let speed = if self.settings.base_speed_mult == 0.0 || self.settings.base_speed_mult.is_nan() {
                0.0
            } else {
                let v = (graphics.crt_rand.next() as f32 * k) * self.settings.base_speed_mult;
                if v > 1.0 {
                    1.0
                } else if v >= f32::from_bits(0x3a03_126f) {
                    v
                } else {
                    f32::from_bits(0x3a03_126f)
                }
            };
            let count = self.cloud_textures.len() as f32;
            let index = (count * (graphics.crt_rand.next() as f32 * k)) as i64;
            // (a draw of 32767 gives the index one past the last texture, which the game reads
            // from the vector's spare room; here it is the last one)
            let texture = self.cloud_textures.get((index.max(0) as usize).min(self.cloud_textures.len().saturating_sub(1))).cloned().unwrap_or_default();
            self.clouds.push(CloudBillboard { theta_rad: theta, phi_rad: phi, radius_mt: radius, speed_mult: speed, texture });
        }
        let (h, w) = (self.settings.cloud_height * 0.5, self.settings.cloud_width * 0.5);
        self.sphere_radius = if h > w { h } else { w };
    }

    /// `SkyBox::updateCloudsAnimation` 0x14021dad0.
    pub fn update_clouds_animation(&mut self, dt: f32) {
        for b in &mut self.clouds {
            b.phi_rad = (dt * b.speed_mult) + b.phi_rad;
        }
    }

    /// `SkyBox::renderClouds` 0x14021d4b0.
    pub fn render_clouds(&mut self, graphics: &mut Graphics, rc: &mut RenderContext) {
        if self.clouds.is_empty() {
            return;
        }
        graphics.set_cull_mode(CULL_FRONT);
        graphics.set_depth_mode(DEPTH_OFF);
        graphics.set_blend_mode(BLEND_ALPHA);
        let Some(mut gl) = graphics.gl.take() else { return };
        let cam = rc.camera.matrix;
        let (px, py, pz) = (cam.m[3][0], cam.m[3][1], cam.m[3][2]);
        let normalize = |x: &mut f32, y: &mut f32, z: &mut f32| {
            let len = sqrtf((*y * *y + *x * *x) + *z * *z);
            if len != 0.0 && !len.is_nan() {
                let inv = 1.0 / len;
                *x *= inv;
                *y *= inv;
                *z *= inv;
            }
        };
        for b in &self.clouds {
            let st = sinf(b.theta_rad);
            let ro = b.radius_mt * st;
            let cp = cosf(b.phi_rad);
            let xo = cp * ro;
            let ct = cosf(b.theta_rad);
            let yo = ct * b.radius_mt;
            let sp = sinf(b.phi_rad);
            let zo = -(sp * ro);
            let (cx, cy, cz) = (px + xo, py + yo, pz + zo);
            let (mut fx, mut fy, mut fz) = (cx - px, cy - py, cz - pz);
            normalize(&mut fx, &mut fy, &mut fz);
            let mut vx = (fy * 0.0) - (fz * -1.0);
            let mut vy = (fz * 0.0) - (fx * 0.0);
            let mut vz = (fx * -1.0) - (fy * 0.0);
            normalize(&mut vx, &mut vy, &mut vz);
            if vx == 0.0 && vy == 0.0 && vz == 0.0 {
                vx = fy - (fz * 0.0);
                vy = (fz * 0.0) - fx;
                vz = (fx * 0.0) - (fy * 0.0);
            }
            let mut w3 = (vy * fx) - (vx * fy);
            let mut w2 = (vx * fz) - (vz * fx);
            let mut w1 = (vz * fy) - (vy * fz);
            normalize(&mut w1, &mut w2, &mut w3);
            let mut r1 = (w3 * fy) - (w2 * fz);
            let mut r2 = (w1 * fz) - (w3 * fx);
            let mut r3 = (w2 * fx) - (w1 * fy);
            normalize(&mut r1, &mut r2, &mut r3);
            let mut m = cam;
            m.m[0][0] = r1;
            m.m[0][1] = r2;
            m.m[0][2] = r3;
            m.m[1][0] = w1;
            m.m[1][1] = w2;
            m.m[1][2] = w3;
            m.m[2][0] = -fx;
            m.m[2][1] = -fy;
            m.m[2][2] = -fz;
            m.m[3][0] = cx;
            m.m[3][1] = cy;
            m.m[3][2] = cz;
            let sphere = Sphere { center: Vec3f::new(cx, cy, cz), radius: self.sphere_radius };
            if !rc.camera.frustum.intersect(&sphere) {
                continue;
            }
            graphics.set_world_matrix(&m);
            graphics.set_texture(0, &b.texture);
            gl.begin(GL_QUADS, Some(self.clouds_shader));
            gl.color4f(0.0, -1.0, 0.0, 0.0);
            let (w, h) = (self.settings.cloud_width, self.settings.cloud_height);
            gl.tex_coord2f(0.0, 1.0);
            gl.vertex3f(w * -0.5, h * -0.5, 0.0);
            gl.tex_coord2f(1.0, 1.0);
            gl.vertex3f(w * 0.5, h * -0.5, 0.0);
            gl.tex_coord2f(1.0, 0.0);
            gl.vertex3f(w * 0.5, h * 0.5, 0.0);
            gl.tex_coord2f(0.0, 0.0);
            gl.vertex3f(w * -0.5, h * 0.5, 0.0);
            gl.end(graphics);
        }
        graphics.gl = Some(gl);
    }

    /// `SkyBox::render` 0x14021d0d0 for the plain camera: the dome (`Mesh::render` of a mesh
    /// without a parent: no visibility test), then the clouds.
    pub fn render(&mut self, graphics: &mut Graphics, rc: &mut RenderContext, cube_map_camera: bool) {
        if cube_map_camera {
            crate::cubemap::render_sky(self, graphics, rc);
        } else {
            self.render_mesh(graphics, rc);
        }
        self.render_clouds(graphics, rc);
    }

    /// `Mesh::render` 0x1402261a0 of the dome.
    pub fn render_mesh(&mut self, graphics: &mut Graphics, rc: &mut RenderContext) {
        rc.material_filter.apply(SKY_MATERIAL, &mut self.material, graphics, rc.pass_id);
        graphics.set_vb(&self.vb);
        graphics.set_ib(&self.ib);
        graphics.commit_shader_changes();
        graphics.draw_primitive(self.index_count, 0, 0);
    }
}

/// `SunAnimator` (0x88 bytes): the sun's angle and the lighting's clock move with the time, and
/// the clouds drift. `RaceManager::initLighting` 0x14013a3d0 makes it when race.ini has
/// `[LIGHTING]`.
#[derive(Clone, Copy, Debug)]
pub struct SunAnimator {
    pub base_angle: f32,
    pub cloud_speed: f32,
    /// set by the replay's status: 0 paused, 1 / slow motion, the fast-forward factor
    pub time_mult: f32,
    pub time_passed: f32,
    pub velocity_multiplier: f32,
}

/// What the replay hands the `SunAnimator`: `ReplayManager::timeMult` and
/// `getCurrentSunAngle`.
#[derive(Clone, Copy, Debug)]
pub struct ReplaySun {
    pub time_mult: f32,
    pub sun_angle: f32,
}

impl SunAnimator {
    /// `SunAnimator::SunAnimator` 0x1401ad890 with the values `initLighting` reads:
    /// `SUN_ANGLE`, `TIME_MULT` (at least 1) and `CLOUD_SPEED`.
    pub fn new(base_angle: f32, time_mult: f32, cloud_speed: f32) -> SunAnimator {
        SunAnimator { base_angle, cloud_speed, time_mult: 1.0, time_passed: 0.0, velocity_multiplier: if time_mult >= 1.0 { time_mult } else { 1.0 } }
    }

    /// `SunAnimator::update` 0x1401adc10.
    pub fn update(&mut self, graphics: &mut Graphics, sky: Option<&mut SkyBox>, dt: f32, pause_menu: bool, replay: Option<ReplaySun>) {
        let tm = dt * self.time_mult;
        if pause_menu {
            return;
        }
        let k = f32::from_bits(0x3b90_2de0);
        match replay {
            None => {
                if let Some(sky) = sky {
                    sky.update_clouds_animation((dt * self.velocity_multiplier) * self.time_mult);
                }
                self.time_passed += dt;
                graphics.lighting.cloud_offset = ((self.time_passed * k) * self.velocity_multiplier) * self.cloud_speed;
                let a = ((self.time_passed * k) * self.velocity_multiplier) + self.base_angle;
                #[allow(clippy::manual_clamp)]
                let angle = if a > 80.0 {
                    80.0
                } else if a < -80.0 || a.is_nan() {
                    -80.0
                } else {
                    a
                };
                graphics.lighting.angle = angle;
                graphics.lighting.game_time = (tm * 1000.0) + graphics.lighting.game_time;
            }
            Some(r) => {
                if let Some(sky) = sky {
                    sky.update_clouds_animation(((dt * self.velocity_multiplier) * self.time_mult) * r.time_mult);
                }
                graphics.lighting.angle = r.sun_angle;
                graphics.lighting.game_time = (tm * 1000.0) + graphics.lighting.game_time;
                graphics.lighting.cloud_offset = ((self.time_passed * k) * self.velocity_multiplier) * self.cloud_speed;
            }
        }
        graphics.update_lighting_settings();
    }
}
