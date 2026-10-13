// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! `GLRenderer` (0x110 bytes): the immediate-mode helper the car's ground shadows, the
//! flames, the mirror quad and the game's 2D drawing use. `GLRenderer::GLRenderer`
//! 0x1401fdfe0, `begin` 0x1401fe800, `color3f` 0x1401fe820, `color4f` 0x1401fe850,
//! `texCoord2f` 0x1401ffbf0, `vertex3f` 0x1401ffc20, `end` 0x1401fe870;
//! `GraphicsManager::createGLRenderer` 0x140202710.
//!
//! Vertices are collected as `MeshVertex` records (44 bytes: the position, the colour in the
//! normal's place, the texture coordinate, the alpha in the tangent's first cell) and drawn
//! from the smallest of its dynamic vertex buffers that holds them.

use crate::graphics::Graphics;
use crate::kgl::KglVertexBuffer;
use crate::shader::ShaderId;

/// `eGLPrimitiveType`.
pub const GL_LINES: i32 = 0;
pub const GL_LINE_STRIP: i32 = 1;
pub const GL_TRIANGLES: i32 = 2;
pub const GL_QUADS: i32 = 3;

const STRIDE: usize = 44;

pub struct GlRenderer {
    /// (how many vertices it holds, the buffer), smallest first
    buffers: Vec<(i32, KglVertexBuffer)>,
    current_index: i32,
    primitive: i32,
    color: [f32; 4],
    use_texture: bool,
    tex_coord: [f32; 2],
    shader: Option<ShaderId>,
    gl_shader: Option<ShaderId>,
    gl_shader_tex: Option<ShaderId>,
    temp_counter: u32,
    temp_vertices: [[u8; STRIDE]; 3],
    temp_buffer: Vec<u8>,
    max_vertices: u32,
    /// `fullQuadVB` / `fullQuadIB`: four corners at -1..1 with texture coordinates, six indices
    full_quad: Option<(KglVertexBuffer, crate::kgl::KglIndexBuffer)>,
}

impl GlRenderer {
    /// `GLRenderer::GLRenderer` 0x1401fdfe0 without the full-screen quad: dynamic buffers of
    /// 6, 12, 24 … vertices up to `max_vertices`, and one of exactly `max_vertices` when the
    /// doubling does not end on it.
    pub fn new(graphics: &Graphics, max_vertices: u32) -> GlRenderer {
        let mut buffers = Vec::new();
        let mut size = 6u32;
        loop {
            if size > max_vertices && !buffers.is_empty() {
                break;
            }
            let count = size.min(max_vertices);
            buffers.push((count as i32, graphics.kgl.create_vertex_buffer(&[], count as usize * STRIDE, STRIDE as u32, true)));
            size = count + count;
        }
        if (buffers.last().map(|b| b.0).unwrap_or(0) as u32) < max_vertices {
            buffers.push((max_vertices as i32, graphics.kgl.create_vertex_buffer(&[], max_vertices as usize * STRIDE, STRIDE as u32, true)));
        }
        GlRenderer {
            buffers,
            current_index: 0,
            primitive: GL_TRIANGLES,
            color: [0.0; 4],
            use_texture: false,
            tex_coord: [0.0; 2],
            shader: None,
            gl_shader: None,
            gl_shader_tex: None,
            temp_counter: 0,
            temp_vertices: [[0u8; STRIDE]; 3],
            temp_buffer: vec![0u8; max_vertices as usize * STRIDE],
            max_vertices,
            full_quad: None,
        }
    }

    /// `GLRenderer::initFullScreenQuad` 0x1401feb30 (the manager's own drawer has it).
    pub fn init_full_screen_quad(&mut self, graphics: &Graphics) {
        // position, normal (0, 1, 0), texture coordinate, tangent (0, 0, 0)
        let corners: [([f32; 2], [f32; 2]); 4] = [([-1.0, -1.0], [0.0, 1.0]), ([1.0, -1.0], [1.0, 1.0]), ([1.0, 1.0], [1.0, 0.0]), ([-1.0, 1.0], [0.0, 0.0])];
        let mut bytes = Vec::with_capacity(4 * STRIDE);
        for (p, uv) in corners {
            for v in [p[0], p[1], 0.0, 0.0, 1.0, 0.0, uv[0], uv[1], 0.0, 0.0, 0.0] {
                bytes.extend_from_slice(&v.to_le_bytes());
            }
        }
        let vb = graphics.kgl.create_vertex_buffer(&bytes, bytes.len(), STRIDE as u32, false);
        let ib = graphics.kgl.create_index_buffer(&[0, 1, 2, 0, 2, 3]);
        self.full_quad = Some((vb, ib));
    }

    /// `GLRenderer::fullScreenQuad` 0x1401feaa0.
    pub fn full_screen_quad(&mut self, graphics: &mut Graphics, shader: Option<ShaderId>) {
        graphics.kgl.set_primitive_type(0);
        let shader = match shader {
            Some(shader) => Some(shader),
            None if self.use_texture => {
                if self.gl_shader_tex.is_none() {
                    self.gl_shader_tex = graphics.shaders.get_shader(&graphics.kgl, "GLTextured").ok();
                }
                self.gl_shader_tex
            }
            None => {
                if self.gl_shader.is_none() {
                    self.gl_shader = graphics.shaders.get_shader(&graphics.kgl, "GL").ok();
                }
                self.gl_shader
            }
        };
        if let Some(shader) = shader {
            graphics.set_shader(shader);
        }
        if let Some((vb, ib)) = &self.full_quad {
            graphics.set_vb(vb);
            graphics.set_ib(ib);
        }
        graphics.commit_shader_changes();
        graphics.kgl.draw_indexed(6, 0, 0);
    }

    /// `GLRenderer::begin` 0x1401fe800.
    pub fn begin(&mut self, primitive: i32, shader: Option<ShaderId>) {
        self.primitive = primitive;
        self.shader = shader;
        self.current_index = 0;
        self.temp_counter = 0;
        self.use_texture = false;
    }

    /// `GLRenderer::color3f` 0x1401fe820.
    pub fn color3f(&mut self, r: f32, g: f32, b: f32) {
        self.color = [r, g, b, 1.0];
    }

    /// `GLRenderer::color4f` 0x1401fe850.
    pub fn color4f(&mut self, r: f32, g: f32, b: f32, a: f32) {
        self.color = [r, g, b, a];
    }

    /// `GLRenderer::texCoord2f` 0x1401ffbf0.
    pub fn tex_coord2f(&mut self, u: f32, v: f32) {
        self.tex_coord = [u, v];
        self.use_texture = true;
    }

    /// The cells of a vertex `vertex3f` writes: the last two of the tangent are left alone.
    fn put(&self, target: &mut [u8], x: f32, y: f32, z: f32) {
        let mut at = 0;
        for v in [x, y, z, self.color[0], self.color[1], self.color[2], self.tex_coord[0], self.tex_coord[1], self.color[3]] {
            target[at..at + 4].copy_from_slice(&v.to_le_bytes());
            at += 4;
        }
    }

    /// `GLRenderer::vertex3f` 0x1401ffc20. A quad's fourth vertex puts six vertices into the
    /// buffer: 0 1 2, 0 2 3.
    pub fn vertex3f(&mut self, x: f32, y: f32, z: f32) {
        if self.current_index as u32 >= self.max_vertices {
            println!("ERROR! GLRenderer::vertex3f currentIndex> {}", self.max_vertices);
            return;
        }
        let slot = |index: i32| index as usize * STRIDE..index as usize * STRIDE + STRIDE;
        match self.primitive {
            0..=2 => {
                let mut v = [0u8; STRIDE];
                v.copy_from_slice(&self.temp_buffer[slot(self.current_index)]);
                self.put(&mut v, x, y, z);
                let at = slot(self.current_index);
                self.temp_buffer[at].copy_from_slice(&v);
                self.current_index += 1;
            }
            3 => {
                if self.temp_counter == 3 {
                    for k in [0usize, 1, 2, 0, 2] {
                        let at = slot(self.current_index);
                        self.temp_buffer[at].copy_from_slice(&self.temp_vertices[k]);
                        self.current_index += 1;
                    }
                    let mut v = [0u8; STRIDE];
                    v.copy_from_slice(&self.temp_buffer[slot(self.current_index)]);
                    self.put(&mut v, x, y, z);
                    let at = slot(self.current_index);
                    self.temp_buffer[at].copy_from_slice(&v);
                    self.current_index += 1;
                    self.temp_counter = 0;
                } else {
                    let mut v = self.temp_vertices[self.temp_counter as usize];
                    self.put(&mut v, x, y, z);
                    self.temp_vertices[self.temp_counter as usize] = v;
                    self.temp_counter += 1;
                }
            }
            _ => {}
        }
    }

    /// `GLRenderer::quad` 0x1401ff420: a rectangle of the screen, corners in the order top left,
    /// bottom left, bottom right, top right; it ends the drawing itself.
    pub fn quad(&mut self, graphics: &mut Graphics, x: f32, y: f32, w: f32, h: f32, use_texture: bool, shader: Option<ShaderId>) {
        self.primitive = GL_QUADS;
        self.current_index = 0;
        self.temp_counter = 0;
        self.use_texture = false;
        self.shader = shader;
        for (px, py, u, v) in [(x, y, 0.0, 0.0), (x, y + h, 0.0, 1.0), (x + w, y + h, 1.0, 1.0), (x + w, y, 1.0, 0.0)] {
            if use_texture {
                self.tex_coord2f(u, v);
            }
            self.vertex3f(px, py, 0.0);
        }
        self.end(graphics);
    }

    /// `GLRenderer::quadCentred` 0x1401ff540: a rectangle around a point, corners in the order
    /// top left, bottom left, bottom right, top right; it ends the drawing itself.
    pub fn quad_centred(&mut self, graphics: &mut Graphics, x: f32, y: f32, w: f32, h: f32, use_texture: bool, shader: Option<ShaderId>) {
        self.primitive = GL_QUADS;
        self.current_index = 0;
        self.temp_counter = 0;
        self.use_texture = false;
        self.shader = shader;
        let (half_w, half_h) = (w * 0.5, h * 0.5);
        let (left, top) = (x - half_w, y - half_h);
        let (right, bottom) = (x + half_w, y + half_h);
        for (px, py, u, v) in [(left, top, 0.0, 0.0), (left, bottom, 0.0, 1.0), (right, bottom, 1.0, 1.0), (right, top, 1.0, 0.0)] {
            if use_texture {
                self.tex_coord2f(u, v);
            }
            self.vertex3f(px, py, 0.0);
        }
        self.end(graphics);
    }

    /// `GLRenderer::vertex3fv` 0x1401ffc00.
    pub fn vertex3fv(&mut self, v: &[f32; 3]) {
        self.vertex3f(v[0], v[1], v[2]);
    }

    /// `GLRenderer::end` 0x1401fe870.
    pub fn end(&mut self, graphics: &mut Graphics) {
        if self.current_index as u32 > self.max_vertices {
            self.max_vertices = self.current_index as u32;
        }
        let Some((_, vb)) = self.buffers.iter().find(|(size, _)| self.current_index <= *size) else {
            println!("ERROR: Could not find suitable buffer for {} vertices in {} buffers", self.current_index, self.buffers.len());
            return;
        };
        graphics.kgl.vertex_buffer_map(vb, &self.temp_buffer[..self.current_index as usize * STRIDE]);
        graphics.kgl.set_vertex_buffer(vb);
        let shader = match self.shader {
            Some(shader) => Some(shader),
            None if self.use_texture => {
                if self.gl_shader_tex.is_none() {
                    self.gl_shader_tex = graphics.shaders.get_shader(&graphics.kgl, "GLTextured").ok();
                }
                self.gl_shader_tex
            }
            None => {
                if self.gl_shader.is_none() {
                    self.gl_shader = graphics.shaders.get_shader(&graphics.kgl, "GL").ok();
                }
                self.gl_shader
            }
        };
        if let Some(shader) = shader {
            graphics.set_shader(shader);
        }
        graphics.commit_shader_changes();
        // lines: a list; a line strip: a strip; triangles and quads: a triangle list
        let topology = match self.primitive {
            0 if self.current_index >= 1 => 1,
            1 if self.current_index >= 1 => 2,
            2 if self.current_index >= 3 => 0,
            3 if self.current_index >= 4 => 0,
            0..=3 => return,
            _ => -1,
        };
        if topology >= 0 {
            graphics.kgl.set_primitive_type(topology);
        }
        graphics.kgl.draw(self.current_index, 0);
        graphics.kgl.set_primitive_type(0);
    }
}

/// `graphics->gl->fullScreenQuad(shader)`: the manager's own drawer, taken out while it draws.
pub fn full_screen_quad(graphics: &mut Graphics, shader: Option<ShaderId>) {
    let Some(mut gl) = graphics.gl.take() else { return };
    gl.full_screen_quad(graphics, shader);
    graphics.gl = Some(gl);
}

/// The overlay of `ACCameraManager`'s fade: its handler of `Game::evOnPostGui` (the lambda
/// 0x140032770), the last thing drawn on the screen, over the GUI and the HUD. `time` is
/// `fadeMode.fadeTime` and `total` `fadeTotalTime`: the screen goes to the fade colour (black)
/// over `total` seconds and comes back over the same time. Nothing is drawn once the fade is
/// over (`time >= total * 2`). The handler sets no state of its own: the quad is drawn with
/// what the 2D pass left. The caller adds the frame's time to `time` afterwards, as the handler
/// does.
pub fn fade_overlay(graphics: &mut Graphics, time: f32, total: f32) {
    if time >= total * 2.0 {
        return;
    }
    let alpha = if time < total { time / total } else { 2.0 - time / total };
    let Some(mut gl) = graphics.gl.take() else { return };
    gl.color4f(0.0, 0.0, 0.0, alpha);
    let (width, height) = (graphics.video.width as f32, graphics.video.height as f32);
    gl.quad(graphics, 0.0, 0.0, width, height, false, None);
    graphics.gl = Some(gl);
}
