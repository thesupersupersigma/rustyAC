// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).
// FW1FontWrapper (c) Erik Rufelt, MIT licence: see LICENSING.md.

//! FW1FontWrapper on kgl's wgpu backend: what `fw1.rs` does through DirectWrite and Direct3D 11,
//! with the glyphs read and drawn from the same font files by `ttf.rs` (DirectWrite is Windows
//! only), packed into the same glyph sheets and drawn by the same shaders (FW1's HLSL, compiled
//! once into `fw1/*.dxbc`) through kgl. Its kgl calls are FW1's own, as Direct3D 11's are: they
//! stay out of the kgl call log, which has the same `fw1 …` lines from both backends instead.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub use crate::fw1_atlas::{FW1_ALIASED, FW1_CENTER, FW1_NOWORDWRAP, FW1_RESTORESTATE, FW1_RIGHT};
use crate::fw1_atlas::{GlyphAtlas, GlyphMetrics, GlyphVertex, SHEET_HEIGHT, SHEET_WIDTH, VERTEX_BUFFER_SIZE};
use crate::kgl::{Format, InputLayout, Kgl, KglCBuffer, KglIndexBuffer, KglVertexBuffer, PixelShader, Sampler, VertexShader, View};
use crate::kgl_log;
use crate::ttf::{Collection, Face};

/// FW1's `VS` and `PS` (`fw1.rs`'s VS_SIMPLE and PS_SIMPLE) as d3dcompiler_43 makes them for
/// vs_5_0 / ps_5_0: the bytes Direct3D 11 runs.
const VS: &[u8] = include_bytes!("../fw1/simple_vs.dxbc");
const PS: &[u8] = include_bytes!("../fw1/simple_ps.dxbc");

/// FW1's states as kgl's state indices: its own blend, no depth test, no culling.
const DEPTH_OFF: i32 = 99;
const CULL_NONE: i32 = 2;

struct FontWrapper {
    key: String,
    face: usize,
    atlas: GlyphAtlas<View>,
    /// `CFW1GlyphProvider`'s glyph maps (one face here): atlas id of each glyph, by size
    maps: HashMap<u32, Vec<u32>>,
    layout: Option<InputLayout>,
    vs: Option<VertexShader>,
    ps: Option<PixelShader>,
    constants: KglCBuffer,
    sampler: Option<Sampler>,
    vertex_buffer: KglVertexBuffer,
    index_buffer: KglIndexBuffer,
}

fn sheet_texture(kgl: &Kgl) -> Option<View> {
    kgl_log::quiet(|| kgl.create_texture(Format(61), SHEET_WIDTH, SHEET_HEIGHT, &[(vec![0; (SHEET_WIDTH * SHEET_HEIGHT) as usize], SHEET_WIDTH)]).ok())
}

impl FontWrapper {
    /// `FW1CreateFontWrapper`.
    fn new(kgl: &Kgl, key: String, face: usize) -> Option<FontWrapper> {
        let mut atlas = GlyphAtlas::new();
        // CFW1GlyphAtlas::initGlyphAtlas: glyph 0, a white block of 16 x 16
        if atlas.insert(&GlyphMetrics { offset_x: 0.0, offset_y: 0.0, width: 16, height: 16 }, &[0xff; 256], 16, 1, || sheet_texture(kgl)) == u32::MAX {
            return None;
        }
        kgl_log::quiet(|| FontWrapper::made(kgl, key, face, atlas))
    }

    fn made(kgl: &Kgl, key: String, face: usize, atlas: GlyphAtlas<View>) -> Option<FontWrapper> {
        let vs = kgl.create_vertex_shader(VS);
        let ps = kgl.create_pixel_shader(PS);
        let elements = [
            crate::shader::InputElement { semantic: "POSITION", index: 0, format: Format(2), offset: 0 },
            crate::shader::InputElement { semantic: "GLYPHCOLOR", index: 0, format: Format(28), offset: 16 },
        ];
        let layout = kgl.create_input_layout(&elements, VS);
        if vs.is_none() || ps.is_none() || layout.is_none() {
            return None;
        }
        Some(FontWrapper {
            key,
            face,
            atlas,
            maps: HashMap::new(),
            layout,
            vs,
            ps,
            constants: kgl.create_cbuffer(80),
            sampler: kgl.create_sampler(1, true, 1, 0.0),
            vertex_buffer: kgl.create_vertex_buffer(&[], VERTEX_BUFFER_SIZE as usize, crate::fw1_atlas::QUAD_VERTEX_SIZE, true),
            index_buffer: kgl.create_index_buffer(&crate::fw1_atlas::quad_indices()),
        })
    }

    /// `CFW1GlyphProvider::insertNewGlyph`: the glyph drawn and put in the atlas.
    fn insert_glyph(&mut self, kgl: &Kgl, face: &Face, size: f32, glyph: u16) -> u32 {
        let (left, top, width, height, pixels) = face.raster(glyph, size);
        let metrics = if width == 0 || height == 0 {
            GlyphMetrics { offset_x: 0.0, offset_y: 0.0, width: 0, height: 0 }
        } else {
            GlyphMetrics { offset_x: left as f32, offset_y: top as f32, width, height }
        };
        crate::fw1_atlas::dump_glyph(&self.key, size, glyph, &metrics, &pixels, width, 1);
        self.atlas.insert(&metrics, &pixels, width, 1, || sheet_texture(kgl))
    }

    /// `CFW1GlyphProvider::GetAtlasIdFromGlyphIndex`, its map made (with glyph 0) the first time.
    fn atlas_id(&mut self, kgl: &Kgl, face: &Face, size: f32, glyph: u16) -> u32 {
        let key = size.to_bits();
        if let std::collections::hash_map::Entry::Vacant(e) = self.maps.entry(key) {
            e.insert(vec![u32::MAX; face.glyph_count() as usize]);
            let first = self.insert_glyph(kgl, face, size, 0);
            if let Some(slot) = self.maps.get_mut(&key).and_then(|m| m.first_mut()) {
                *slot = first;
            }
        }
        let map = &self.maps[&key];
        let fallback = map.first().copied().filter(|i| *i != u32::MAX).unwrap_or(0);
        match map.get(glyph as usize).copied() {
            None => fallback,
            Some(id) if id != u32::MAX => id,
            Some(_) => match self.insert_glyph(kgl, face, size, glyph) {
                u32::MAX => fallback,
                id => {
                    if let Some(slot) = self.maps.get_mut(&key).and_then(|m| m.get_mut(glyph as usize)) {
                        *slot = id;
                    }
                    id
                }
            },
        }
    }

    /// `CFW1FontWrapper::DrawString`.
    #[allow(clippy::too_many_arguments)]
    fn draw_string(&mut self, kgl: &Kgl, face: &Face, text: &[u16], size: f32, x: f32, y: f32, color: u32, flags: u32) {
        // DirectWrite makes no layout of an empty string: FW1 draws nothing, not even its states
        if text.is_empty() {
            return;
        }
        let flags = flags | FW1_NOWORDWRAP;
        let glyphs = crate::ttf::place(face, text, size, x, y, flags);
        let mut vertices = Vec::with_capacity(glyphs.len());
        let mut placed = Vec::with_capacity(glyphs.len());
        let mut max_sheet_index = 0;
        for &(gx, gy, glyph) in &glyphs {
            let atlas_id = self.atlas_id(kgl, face, size, glyph);
            vertices.push(GlyphVertex { x: gx, y: gy, glyph_index: atlas_id, color });
            placed.push((gx, gy, glyph, color));
            max_sheet_index = max_sheet_index.max(atlas_id >> 16);
        }
        crate::fw1_atlas::dump_layout(&self.key, size, x, y, flags, text, &placed);
        // CFW1GlyphAtlas::Flush
        self.atlas.flush(|view, rect, bytes| kgl.update_texture(view, rect, bytes, SHEET_WIDTH));
        // CFW1FontWrapper::DrawGeometry, its states saved and put back
        let viewport = kgl.viewport();
        let constants = crate::fw1_atlas::constants(Some((viewport[2], viewport[3])));
        let batches = crate::fw1_atlas::batches(&vertices, max_sheet_index, &self.atlas);
        crate::fw1_atlas::log_geometry(&placed, &batches, &constants);
        if batches.is_empty() {
            return;
        }
        let saved = kgl.save_bound();
        kgl_log::quiet(|| {
            kgl.set_primitive_type(0);
            kgl.set_shader(&self.layout, &self.vs, &self.ps);
            kgl.cbuffer_map(&self.constants, &constants);
            kgl.cbuffer_bind(&self.constants, 0);
            kgl.set_blend_state(crate::kgl::FW1_BLEND);
            kgl.set_depth_state(DEPTH_OFF);
            kgl.set_cull_state(CULL_NONE);
            kgl.set_sampler_ps(&self.sampler, 0);
            kgl.set_index_buffer(&self.index_buffer);
            kgl.set_vertex_buffer(&self.vertex_buffer);
            let mut active_sheet = usize::MAX;
            for batch in &batches {
                kgl.vertex_buffer_map(&self.vertex_buffer, &batch.bytes);
                for &(sheet, index_count, base) in &batch.draws {
                    if sheet != active_sheet {
                        kgl.set_texture(0, &Some(self.atlas.sheets[sheet].1.clone()));
                        active_sheet = sheet;
                    }
                    kgl.draw_indexed(index_count as i32, 0, base);
                }
            }
        });
        kgl.restore_bound(saved);
    }
}

/// kgl's fonts (`kglCreateFont` and `kglBlitStringWide`'s wrappers), from the game's
/// `content/fonts`.
#[derive(Default)]
pub struct Fonts {
    folder: PathBuf,
    collection: Option<Collection>,
    wrappers: Vec<FontWrapper>,
    by_key: HashMap<String, Option<usize>>,
}

impl Fonts {
    pub fn new(game_folder: &Path) -> Fonts {
        Fonts { folder: game_folder.join("content/fonts"), ..Default::default() }
    }

    /// The locale only turns "Segoe UI" into "Segoe" for DirectWrite, which then finds the
    /// system's Segoe UI: the same faces as the game's here.
    pub fn set_locale(&mut self, _locale: &str) {}

    /// The game's collection (`CFW1Factory`'s custom collection: every .ttf of the folder).
    fn collection(&mut self) -> &Collection {
        let folder = &self.folder;
        self.collection.get_or_insert_with(|| {
            let mut collection = Collection::default();
            let mut names: Vec<String> = rustyac_content::vfs::read_dir(folder).unwrap_or_default().into_iter().filter(|(name, dir)| !dir && name.to_ascii_lowercase().ends_with(".ttf")).map(|(name, _)| name).collect();
            names.sort();
            for name in names {
                if let Some(face) = rustyac_content::vfs::read(&folder.join(&name)).ok().and_then(Face::parse) {
                    collection.faces.push(face);
                }
            }
            collection
        })
    }

    pub fn get_font_wrapper(&mut self, kgl: &Kgl, family: &str, italic: bool, bold: bool) -> Option<usize> {
        let key = crate::fw1_atlas::font_key(family, italic, bold);
        if let Some(known) = self.by_key.get(&key) {
            return *known;
        }
        // DirectWrite falls back for a family the collection does not have: Segoe UI here
        let collection = self.collection();
        let weight = if bold { 700 } else { 400 };
        let face = collection.find(family, weight, italic).or_else(|| collection.find("Segoe UI", weight, italic)).or((!collection.faces.is_empty()).then_some(0));
        let made = face.and_then(|face| FontWrapper::new(kgl, key.clone(), face)).map(|wrapper| {
            self.wrappers.push(wrapper);
            self.wrappers.len() - 1
        });
        if made.is_none() {
            println!("ERROR: pFW1Factory->CreateFontWrapper({family}) failed");
        }
        crate::fw1_atlas::log_create_font(&key, made, made.map(|_| (VS, PS)));
        self.by_key.insert(key, made);
        made
    }

    #[allow(clippy::too_many_arguments)]
    pub fn blit_string(&mut self, kgl: &Kgl, wrapper: usize, x: f32, y: f32, text: &[u16], size: f32, align: u32, color: u32) {
        let flags = crate::fw1_atlas::blit_flags(align);
        let Some(collection) = &self.collection else { return };
        if let Some(w) = self.wrappers.get_mut(wrapper) {
            crate::fw1_atlas::log_draw_string(wrapper, x, y, text, size, flags, color);
            let face = &collection.faces[w.face];
            w.draw_string(kgl, face, text, size, x, y, color, flags);
        }
    }
}
