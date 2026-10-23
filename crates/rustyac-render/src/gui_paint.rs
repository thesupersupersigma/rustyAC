// SPDX-License-Identifier: GPL-3.0-or-later

//! The GUI's painter on Direct3D: `rustyac_gui::painter::Painter` carried out by the ports of
//! the game's own classes (`GraphicsManager`, `GLRenderer`, `ResourceStore`, kgl's fonts), so
//! that the widgets ask Direct3D for exactly what the game's ask.

use rustyac_gui::painter::{GlId, Painter, Texture};

use crate::fw1::Fonts;
use crate::gl::GlRenderer;
use crate::graphics::Graphics;
use crate::texture::TextureId;

/// What the GUI keeps between frames on the renderer's side: the fonts and every control's
/// `GLRenderer`.
pub struct GuiRenderer {
    pub fonts: Fonts,
    /// index 0 is unused: it stands for `GraphicsManager::gl`
    gls: Vec<Option<GlRenderer>>,
    /// the render target `TrackMapHandler::drawMapOnRenderTarget` draws into
    map_target: Option<crate::kgl::KglRenderTarget>,
}

/// A PNG of RGBA pixels with stored (not compressed) deflate blocks: the same pixels as any
/// other PNG writer gives, so that loading it again makes the same texture.
pub fn encode_png(width: u32, height: u32, rgba: &[u8]) -> Vec<u8> {
    fn crc32(bytes: &[u8]) -> u32 {
        let mut c = 0xffff_ffffu32;
        for b in bytes {
            c ^= *b as u32;
            for _ in 0..8 {
                c = if c & 1 != 0 { 0xedb8_8320 ^ (c >> 1) } else { c >> 1 };
            }
        }
        !c
    }
    fn chunk(out: &mut Vec<u8>, kind: &[u8], data: &[u8]) {
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        let mut body = kind.to_vec();
        body.extend_from_slice(data);
        out.extend_from_slice(&body);
        out.extend_from_slice(&crc32(&body).to_be_bytes());
    }
    let mut raw = Vec::with_capacity((width as usize * 4 + 1) * height as usize);
    for row in rgba.chunks(width as usize * 4).take(height as usize) {
        raw.push(0);
        raw.extend_from_slice(row);
    }
    let mut z = vec![0x78, 0x01];
    let blocks: Vec<&[u8]> = raw.chunks(65535).collect();
    for (i, block) in blocks.iter().enumerate() {
        z.push((i + 1 == blocks.len()) as u8);
        z.extend_from_slice(&(block.len() as u16).to_le_bytes());
        z.extend_from_slice(&(!(block.len() as u16)).to_le_bytes());
        z.extend_from_slice(block);
    }
    if blocks.is_empty() {
        z.extend_from_slice(&[1, 0, 0, 0xff, 0xff]);
    }
    let (mut a, mut b) = (1u32, 0u32);
    for x in &raw {
        a = (a + *x as u32) % 65521;
        b = (b + a) % 65521;
    }
    z.extend_from_slice(&((b << 16) | a).to_be_bytes());
    let mut out = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
    let mut header = Vec::new();
    header.extend_from_slice(&width.to_be_bytes());
    header.extend_from_slice(&height.to_be_bytes());
    header.extend_from_slice(&[8, 6, 0, 0, 0]);
    chunk(&mut out, b"IHDR", &header);
    chunk(&mut out, b"IDAT", &z);
    chunk(&mut out, b"IEND", &[]);
    out
}

impl GuiRenderer {
    /// kgl's fonts are started (`kglInitFonts`).
    pub fn new(graphics: &Graphics) -> GuiRenderer {
        GuiRenderer { fonts: Fonts::new(&graphics.game_folder), gls: vec![None], map_target: None }
    }

    pub fn painter<'a>(&'a mut self, graphics: &'a mut Graphics) -> GuiPainter<'a> {
        GuiPainter { graphics, state: self }
    }
}

pub struct GuiPainter<'a> {
    pub graphics: &'a mut Graphics,
    state: &'a mut GuiRenderer,
}

fn render_texture(texture: &Texture) -> crate::texture::Texture {
    crate::texture::Texture { kid: texture.kid.map(TextureId), file_name: texture.file_name.clone() }
}

impl GuiPainter<'_> {
    /// The drawer of that number, taken out of its place while it draws.
    fn with_gl(&mut self, gl: GlId, f: impl FnOnce(&mut GlRenderer, &mut Graphics)) {
        let taken = if gl == 0 { self.graphics.gl.take() } else { self.state.gls.get_mut(gl).and_then(|g| g.take()) };
        let Some(mut renderer) = taken else { return };
        f(&mut renderer, self.graphics);
        if gl == 0 {
            self.graphics.gl = Some(renderer);
        } else {
            self.state.gls[gl] = Some(renderer);
        }
    }
}

impl Painter for GuiPainter<'_> {
    fn screen(&self) -> (i32, i32) {
        (self.graphics.video.width, self.graphics.video.height)
    }

    fn reset_render_states(&mut self) {
        self.graphics.reset_render_states();
    }

    fn set_depth_mode(&mut self, mode: i32) {
        self.graphics.set_depth_mode(mode);
    }

    fn set_blend_mode(&mut self, mode: i32) {
        self.graphics.set_blend_mode(mode);
    }

    fn set_cull_mode(&mut self, mode: i32) {
        self.graphics.set_cull_mode(mode);
    }

    fn set_texture(&mut self, slot: i32, texture: &Texture) {
        self.graphics.set_texture(slot, &render_texture(texture));
    }

    fn get_texture(&mut self, path: &str) -> Texture {
        let full = self.graphics.game_folder.join(path);
        let texture = self.graphics.resources.get_texture(&self.graphics.kgl, &full.to_string_lossy());
        Texture { kid: texture.kid.map(|k| k.0), file_name: if texture.kid.is_some() { path.to_string() } else { String::new() } }
    }

    fn texture_size(&self, texture: &Texture) -> (u32, u32) {
        self.graphics.resources.size(&render_texture(texture)).unwrap_or((0, 0))
    }

    fn file_exists(&self, path: &str) -> bool {
        self.graphics.game_folder.join(path).is_file()
    }

    fn create_gl(&mut self, max_vertices: u32) -> GlId {
        self.state.gls.push(Some(GlRenderer::new(self.graphics, max_vertices)));
        self.state.gls.len() - 1
    }

    fn gl_begin(&mut self, gl: GlId, primitive: i32) {
        self.with_gl(gl, |g, _| g.begin(primitive, None));
    }

    fn gl_color3f(&mut self, gl: GlId, r: f32, g: f32, b: f32) {
        self.with_gl(gl, |renderer, _| renderer.color3f(r, g, b));
    }

    fn gl_color4f(&mut self, gl: GlId, r: f32, g: f32, b: f32, a: f32) {
        self.with_gl(gl, |renderer, _| renderer.color4f(r, g, b, a));
    }

    fn gl_tex_coord2f(&mut self, gl: GlId, u: f32, v: f32) {
        self.with_gl(gl, |g, _| g.tex_coord2f(u, v));
    }

    fn gl_vertex3f(&mut self, gl: GlId, x: f32, y: f32, z: f32) {
        self.with_gl(gl, |g, _| g.vertex3f(x, y, z));
    }

    fn gl_end(&mut self, gl: GlId) {
        self.with_gl(gl, |g, graphics| g.end(graphics));
    }

    fn gl_quad(&mut self, gl: GlId, x: f32, y: f32, w: f32, h: f32, textured: bool) {
        self.with_gl(gl, |g, graphics| g.quad(graphics, x, y, w, h, textured, None));
    }

    fn gl_quad_centred(&mut self, gl: GlId, x: f32, y: f32, w: f32, h: f32, textured: bool) {
        self.with_gl(gl, |g, graphics| g.quad_centred(graphics, x, y, w, h, textured, None));
    }

    fn gl_quad_centred_rotated(&mut self, gl: GlId, x: f32, y: f32, w: f32, h: f32, angle: f32, textured: bool) {
        self.with_gl(gl, |g, graphics| g.quad_centred_rotated(graphics, x, y, w, h, angle, textured, None));
    }

    fn begin_map_target(&mut self, w: i32, h: i32, wf: f32, hf: f32) -> bool {
        let g = &mut *self.graphics;
        // RenderTarget::RenderTarget(gm, eR8G8B8A8, w, h, colour, no depth, 1) 0x14021c030
        let target = g.kgl.create_render_target(crate::kgl::FORMAT_RGBA8, w, h, 1, 1);
        g.kgl.set_render_targets(Some(&target), None);
        g.set_viewport(0, 0, w, h);
        g.clear_render_target(&[0.0; 4]);
        g.set_depth_mode(2);
        g.set_blend_mode(0);
        let mut p = rustyac_physics::vecmath::Mat44f::default();
        p.m[0][0] = 2.0 / wf;
        p.m[1][1] = 2.0 / -hf;
        p.m[2][2] = -0.5;
        p.m[3][0] = wf / -wf;
        p.m[3][1] = hf / hf;
        p.m[3][2] = 0.5;
        p.m[3][3] = 1.0;
        g.set_projection_matrix(&p);
        g.set_view_matrix(&rustyac_physics::vecmath::Mat44f::IDENTITY, None);
        g.set_world_matrix(&rustyac_physics::vecmath::Mat44f::IDENTITY);
        g.set_cull_mode(2);
        self.state.map_target = Some(target);
        true
    }

    fn end_map_target(&mut self, path: &str) -> Option<Vec<u8>> {
        let target = self.state.map_target.take()?;
        self.graphics.kgl.flush();
        let pixels = self.graphics.kgl.read_render_target(&target).unwrap_or_default();
        let png = encode_png(target.width as u32, target.height as u32, &pixels);
        // the game saves the picture and its ResourceStore reads it again by that name
        let full = self.graphics.game_folder.join(path);
        let _ = self.graphics.resources.add_file_texture(&self.graphics.kgl, &full.to_string_lossy(), &png);
        Some(png)
    }

    fn override_sampler_state(&mut self, state: i32) {
        // GraphicsManager::overrideSamplerState 0x140204410: slot 5
        match state {
            0 => self.graphics.kgl.set_sampler_ps(&self.graphics.samplers.linear_simple, 5),
            1 => self.graphics.kgl.set_sampler_ps(&self.graphics.samplers.linear_clamp, 5),
            _ => {}
        }
    }

    fn get_shader(&mut self, name: &str) -> Option<usize> {
        self.graphics.shaders.get_shader(&self.graphics.kgl, name).ok().map(|s| s.0)
    }

    fn shader_var(&self, shader: usize, name: &str) -> Option<usize> {
        self.graphics.shaders.get(crate::shader::ShaderId(shader)).get_var(name)
    }

    fn set_shader_var(&mut self, shader: usize, var: usize, bytes: &[u8]) {
        self.graphics.shaders.get_mut(crate::shader::ShaderId(shader)).set_var(var, bytes);
    }

    fn commit_shader_var(&mut self, shader: usize, var: usize) {
        let kgl = &self.graphics.kgl;
        let s = self.graphics.shaders.get_mut(crate::shader::ShaderId(shader));
        if let Some(buffer) = s.vars.get(var).and_then(|v| v.buffer) {
            s.cbuffers[buffer].commit(kgl);
        }
    }

    fn gl_quad_shader(&mut self, gl: GlId, x: f32, y: f32, w: f32, h: f32, textured: bool, shader: usize) {
        self.with_gl(gl, |g, graphics| g.quad(graphics, x, y, w, h, textured, Some(crate::shader::ShaderId(shader))));
    }

    fn font_wrapper(&mut self, family: &str, italic: bool, bold: bool) -> usize {
        // (none: an index no wrapper has, so that nothing is drawn with it)
        self.state.fonts.get_font_wrapper(&self.graphics.kgl, family, italic, bold).unwrap_or(usize::MAX)
    }

    fn blit_string_wide(&mut self, wrapper: usize, x: f32, y: f32, text: &str, size: f32, align: u32, color: u32) {
        let wide: Vec<u16> = text.encode_utf16().collect();
        self.state.fonts.blit_string(&self.graphics.kgl, wrapper, x, y, &wide, size, align, color);
    }
}
