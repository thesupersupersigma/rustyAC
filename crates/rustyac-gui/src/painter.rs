// SPDX-License-Identifier: GPL-3.0-or-later

//! What the widgets draw with: the calls the game's GUI code makes on `GLRenderer`,
//! `GraphicsManager`, `ResourceStore` and kgl's fonts, as a trait. The Direct3D renderer
//! implements it with its ports of those classes (the same Direct3D calls as the game's); the
//! browser preview implements it on its own pipeline.

/// `ksgui::ksRect`: left, right, top, bottom (in that order in memory).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub left: f32,
    pub right: f32,
    pub top: f32,
    pub bottom: f32,
}

/// `Texture` (0x28 bytes): a texture of the resource store (none: `kid` is null) and its name.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Texture {
    pub kid: Option<u32>,
    pub file_name: String,
}

/// `eGLPrimitiveType`.
pub const GL_LINES: i32 = 0;
pub const GL_LINE_STRIP: i32 = 1;
pub const GL_TRIANGLES: i32 = 2;
pub const GL_QUADS: i32 = 3;

/// `eFontAlign`: `Font::blitString` turns it into kgl's 0 left, 1 centre, 2 right.
pub const ALIGN_LEFT: i32 = 0;
pub const ALIGN_RIGHT: i32 = 1;
pub const ALIGN_CENTER: i32 = 2;

/// `eBlendMode` / `eDepthMode` values the GUI uses.
pub const BLEND_OPAQUE: i32 = 0;
pub const BLEND_ALPHA: i32 = 1;
pub const DEPTH_OFF: i32 = 2;

/// A `GLRenderer` of the painter: 0 is `GraphicsManager::gl`, the others were made with
/// [`Painter::create_gl`] (every control has one of 12 vertices).
pub type GlId = usize;
pub const GRAPHICS_GL: GlId = 0;

pub trait Painter {
    /// `GraphicsManager::videoSettings.width` / `height`.
    fn screen(&self) -> (i32, i32);
    /// `GraphicsManager::resetRenderStates` 0x140204440.
    fn reset_render_states(&mut self);
    /// `GraphicsManager::setDepthMode` 0x140204580.
    fn set_depth_mode(&mut self, mode: i32);
    /// `GraphicsManager::setBlendMode` 0x1402044e0.
    fn set_blend_mode(&mut self, mode: i32);
    /// `GraphicsManager::setCullMode` 0x140204500.
    fn set_cull_mode(&mut self, mode: i32);
    /// `GraphicsManager::setTexture` 0x140204c70.
    fn set_texture(&mut self, slot: i32, texture: &Texture);
    /// `ResourceStore::getTexture` 0x1402000d0 (a path under the game's folder).
    fn get_texture(&mut self, path: &str) -> Texture;
    /// `kglTextureGetWidth` / `kglTextureGetHeight` (0 for none).
    fn texture_size(&self, texture: &Texture) -> (u32, u32);
    /// `Path::fileExists` for a path under the game's folder.
    fn file_exists(&self, path: &str) -> bool;

    /// `GraphicsManager::createGLRenderer` 0x140202710.
    fn create_gl(&mut self, max_vertices: u32) -> GlId;
    /// `GLRenderer::begin` 0x1401fe800 (no shader of its own).
    fn gl_begin(&mut self, gl: GlId, primitive: i32);
    /// `GLRenderer::color3f` 0x1401fe820.
    fn gl_color3f(&mut self, gl: GlId, r: f32, g: f32, b: f32);
    /// `GLRenderer::color4f` 0x1401fe850.
    fn gl_color4f(&mut self, gl: GlId, r: f32, g: f32, b: f32, a: f32);
    /// `GLRenderer::texCoord2f` 0x1401ffbf0.
    fn gl_tex_coord2f(&mut self, gl: GlId, u: f32, v: f32);
    /// `GLRenderer::vertex3f` 0x1401ffc20.
    fn gl_vertex3f(&mut self, gl: GlId, x: f32, y: f32, z: f32);
    /// `GLRenderer::end` 0x1401fe870.
    fn gl_end(&mut self, gl: GlId);
    /// `GLRenderer::quad` 0x1401ff420.
    fn gl_quad(&mut self, gl: GlId, x: f32, y: f32, w: f32, h: f32, textured: bool);
    /// `GLRenderer::quadCentred` 0x1401ff540.
    fn gl_quad_centred(&mut self, gl: GlId, x: f32, y: f32, w: f32, h: f32, textured: bool);
    /// `GLRenderer::quadCentredRotated` 0x1401ff6b0
    #[allow(clippy::too_many_arguments)]
    fn gl_quad_centred_rotated(&mut self, gl: GlId, x: f32, y: f32, w: f32, h: f32, angle: f32, textured: bool);

    /// `Font::getFontWrapper` 0x1402010d0: the wrapper of a family, italic or not, bold or not.
    fn font_wrapper(&mut self, family: &str, italic: bool, bold: bool) -> usize;
    /// `kglBlitStringWide` 0x140019aa0: `align` 0 left, 1 centre, 2 right; `color` 0xAABBGGRR.
    fn blit_string_wide(&mut self, wrapper: usize, x: f32, y: f32, text: &str, size: f32, align: u32, color: u32);
}
