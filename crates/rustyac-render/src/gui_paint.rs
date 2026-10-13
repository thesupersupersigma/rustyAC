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
}

impl GuiRenderer {
    /// kgl's fonts are started (`kglInitFonts`).
    pub fn new(graphics: &Graphics) -> GuiRenderer {
        GuiRenderer { fonts: Fonts::new(&graphics.game_folder), gls: vec![None] }
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

    fn font_wrapper(&mut self, family: &str, italic: bool, bold: bool) -> usize {
        // (none: an index no wrapper has, so that nothing is drawn with it)
        self.state.fonts.get_font_wrapper(&self.graphics.kgl, family, italic, bold).unwrap_or(usize::MAX)
    }

    fn blit_string_wide(&mut self, wrapper: usize, x: f32, y: f32, text: &str, size: f32, align: u32, color: u32) {
        let wide: Vec<u16> = text.encode_utf16().collect();
        self.state.fonts.blit_string(&self.graphics.kgl, wrapper, x, y, &wide, size, align, color);
    }
}
