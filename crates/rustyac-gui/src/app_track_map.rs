// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! `TrackMap` (0x368 bytes), the app "Track map": a window of the track's `map.png` round the
//! first car, turned with it, the other cars as icons, an arrow for the player; with
//! `TrackMapHandler` (0x88 bytes, its member `tmc`: the map's place in the world) and the
//! functions of `TrackMapSupport`.
//!
//! `TrackMap::TrackMap` 0x1402ee8e0, `initParameters` 0x1402ef150, `loadMap` 0x1402ef590,
//! `createMap` 0x1402eeee0, `render` 0x1402efd50, `showMapOnScreenZoom` 0x1402f00d0;
//! `TrackMapHandler::TrackMapHandler` 0x14033c420, `loadParameters` 0x14033cf30, `worldToMap`
//! 0x14033d790, `calculateAllMapParameters` 0x14033c690 (`createListOfPointsToDrawFromSpline`
//! 0x14033c740, `makePositiveCoordAndCalculateWidthAndHeightRT` 0x14033d330,
//! `calculateMapSize` 0x14033c6c0), `drawMapOnRenderTarget` 0x14033c810, `saveParameters`
//! 0x14033d440; `CombinedCoordAllowNegative` 0x14033d7e0, `rotateVertexBidimensional`
//! 0x14033d810.

use std::any::Any;

use rustyac_physics::data::ini::IniReader;

use crate::apps::{App, AppAction};
use crate::control::Id;
use crate::gui::Gui;
use crate::hud::translate;
use crate::painter::{Painter, Texture, GL_TRIANGLES, GRAPHICS_GL};

/// `TrackMapHandler`.
#[derive(Clone, Debug, Default)]
pub struct TrackMapHandler {
    pub points_to_draw: Vec<[f32; 3]>,
    pub width: f32,
    pub height: f32,
    pub margin: f32,
    pub map_square_size: f32,
    pub scale_factor: f32,
    pub drawing_size: f32,
    pub min_x: f32,
    pub max_x: f32,
    pub min_z: f32,
    pub max_z: f32,
    pub coord_offset_x: f32,
    pub coord_offset_z: f32,
    pub finishing_spline_link_threshold_meters: f32,
    pub textured: bool,
    pub road_texture: Texture,
}

impl TrackMapHandler {
    /// `TrackMapHandler::TrackMapHandler(Sim*)` 0x14033c420.
    pub fn new(painter: &mut dyn Painter) -> TrackMapHandler {
        let mut h = TrackMapHandler { margin: 20.0, scale_factor: 1.0, drawing_size: 10.0, finishing_spline_link_threshold_meters: 20.0, ..TrackMapHandler::default() };
        if painter.file_exists("content/texture/mapRoad.png") {
            h.road_texture = painter.get_texture("content/texture/mapRoad.png");
            h.textured = true;
        }
        h
    }

    /// `TrackMapHandler::loadParameters` 0x14033cf30 (`ini`: the track's `data\map.ini`).
    pub fn load_parameters(&mut self, ini: Option<&IniReader>, w: f32, h: f32) {
        let Some(ini) = ini.filter(|i| i.ready) else { return };
        let f = |key: &str| ini.get_float("PARAMETERS", key).unwrap_or(0.0);
        self.scale_factor = f("SCALE_FACTOR");
        self.width = w;
        self.height = h;
        self.margin = f("MARGIN");
        self.coord_offset_x = f("X_OFFSET");
        self.coord_offset_z = f("Z_OFFSET");
        self.drawing_size = f("DRAWING_SIZE");
    }

    /// `TrackMapHandler::worldToMap` 0x14033d790: (x, z).
    pub fn world_to_map(&self, p: [f32; 3]) -> [f32; 2] {
        let k = 1.0f32 / self.scale_factor;
        [(self.coord_offset_x + p[0]) * k, (self.coord_offset_z + p[2]) * k]
    }

    /// `calculateAllMapParameters` 0x14033c690 over the AI line's points.
    pub fn calculate_all_map_parameters(&mut self, points: &[[f32; 3]]) {
        // createListOfPointsToDrawFromSpline
        for p in points {
            self.points_to_draw.push(*p);
            if p[0] > self.max_x {
                self.max_x = p[0];
            }
            if p[0] < self.min_x {
                self.min_x = p[0];
            }
            if p[2] > self.max_z {
                self.max_z = p[2];
            }
            if p[2] < self.min_z {
                self.min_z = p[2];
            }
        }
        // makePositiveCoordAndCalculateWidthAndHeightRT
        self.coord_offset_x = self.min_x.abs() + self.margin;
        self.coord_offset_z = self.min_z.abs() + self.margin;
        let (mut mx, mut mz) = (0.0f32, 0.0f32);
        for p in self.points_to_draw.iter_mut() {
            p[0] = p[0] + self.coord_offset_x;
            if p[0] > mx {
                mx = p[0];
            }
            p[2] = self.coord_offset_z + p[2];
            if p[2] > mz {
                mz = p[2];
            }
        }
        self.height = self.margin + mz;
        self.width = self.margin + mx;
        // calculateMapSize
        let size = if self.width < self.height { self.height } else { self.width };
        self.map_square_size = size;
        let mut s = self.scale_factor;
        let mut q = size / s;
        if q >= 1600.0 {
            loop {
                s = s + f32::from_bits(0x3dcc_cccd);
                q = size / s;
                if !(q >= 1600.0) {
                    break;
                }
            }
            self.scale_factor = s;
        }
        self.map_square_size = q;
        let inv = 1.0f32 / self.scale_factor;
        self.height = inv * self.height;
        self.width = inv * self.width;
    }

    /// `TrackMapHandler::saveParameters` 0x14033d440: the text of `data\map.ini` (`std::ofstream`
    /// in text mode, floats with six significant digits).
    pub fn parameters_text(&self) -> String {
        let g = |v: f32| format_g6(v as f64);
        format!(
            "[PARAMETERS]\r\nWIDTH={}\r\nHEIGHT={}\r\nMARGIN={}\r\nSCALE_FACTOR={}\r\nX_OFFSET={}\r\nZ_OFFSET={}\r\nDRAWING_SIZE={}\r\n",
            g(self.width),
            g(self.height),
            g(self.margin),
            g(self.scale_factor),
            g(self.coord_offset_x),
            g(self.coord_offset_z),
            g(self.drawing_size)
        )
    }
}

/// `operator<<(ostream&, float)` with the default precision: `%g` of six significant digits.
pub fn format_g6(v: f64) -> String {
    if v == 0.0 {
        return "0".to_string();
    }
    let exp = v.abs().log10().floor() as i32;
    if !(-5..6).contains(&exp) {
        let s = format!("{:.5e}", v);
        let (mantissa, e) = s.split_once('e').unwrap_or((&s, "0"));
        let mantissa = mantissa.trim_end_matches('0').trim_end_matches('.');
        let e: i32 = e.parse().unwrap_or(0);
        return format!("{mantissa}e{}{:02}", if e < 0 { '-' } else { '+' }, e.abs());
    }
    let decimals = (5 - exp).max(0) as usize;
    let s = format!("{:.*}", decimals, v);
    if s.contains('.') {
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    } else {
        s
    }
}

/// `CombinedCoordAllowNegative` 0x14033d7e0: (sup, inf).
fn combined(c: f32, r: f32) -> (f32, f32) {
    (c + r, c - r)
}

/// `rotateVertexBidimensional` 0x14033d810.
fn rotate(v: [f32; 2], a: f32) -> [f32; 2] {
    let (s, c) = (rustyac_math::sinf(a), rustyac_math::cosf(a));
    [c * v[0] - s * v[1], c * v[1] + s * v[0]]
}

/// The inline `floorf` of the game: cut, one less for a negative with a fraction.
fn floor_cut(v: f32) -> f32 {
    let mut t = crate::app_essentials::cut_f(v);
    if t != i32::MIN && t as f32 != v {
        t = t.wrapping_sub(v.is_sign_negative() as i32);
    }
    if t == i32::MIN {
        return v;
    }
    t as f32
}

pub struct TrackMap {
    pub mode: u32,
    pub adversary_icon: Texture,
    pub map: Texture,
    pub track_name: String,
    pub track_config_name: String,
    pub tmc: TrackMapHandler,
    pub zoom_mode_width: f32,
    pub zoom_mode_height: f32,
    pub size_icon: f32,
    pub max_dimension_form: f32,
    pub zoom_section: f32,
    pub original_width: f32,
    pub original_height: f32,
    pub scaled_x: f32,
    pub scaled_z: f32,
    pub zw: f32,
    pub zh: f32,
    pub zh_top: f32,
    pub zh_bottom: f32,
    pub x_off: f32,
    pub z_off: f32,
    pub starting_alpha: f32,
    pub finishing_alpha: f32,
}

/// What `TrackMap`'s constructor reads besides the GUI's files.
pub struct TrackMapFiles<'a> {
    /// `system/cfg/map.ini`
    pub map_ini: Option<&'a IniReader>,
    /// the track's `data\map.ini` (none: there is none)
    pub track_map_ini: Option<&'a IniReader>,
    /// the track's folder name and layout
    pub track: &'a str,
    pub config: &'a str,
    /// the AI line's points (x, y, z) for a track without `map.png`
    pub ai_points: &'a [[f32; 3]],
}

impl Gui {
    /// `TrackMap::TrackMap(GUI*, Sim*, bool)` 0x1402ee8e0.
    pub fn new_track_map_app(&mut self, painter: &mut dyn Painter, files: &TrackMapFiles) -> Id {
        let form = self.new_form(painter, "KSMAP", false);
        let tmc = TrackMapHandler::new(painter);
        self.form_set_title(form, &translate("Track map"));
        self.form_data_mut(form).dev_app = false;
        let mut app = TrackMap {
            mode: 1,
            adversary_icon: Texture::default(),
            map: Texture::default(),
            track_name: files.track.to_string(),
            track_config_name: files.config.to_string(),
            tmc,
            zoom_mode_width: 0.0,
            zoom_mode_height: 0.0,
            size_icon: 10.0,
            max_dimension_form: 300.0,
            zoom_section: f32::from_bits(0x3fac_cccd),
            original_width: 0.0,
            original_height: 0.0,
            scaled_x: 0.0,
            scaled_z: 0.0,
            zw: 0.0,
            zh: 0.0,
            zh_top: 0.0,
            zh_bottom: 0.0,
            x_off: 0.0,
            z_off: 0.0,
            starting_alpha: f32::from_bits(0x3f4c_cccd),
            finishing_alpha: 0.0,
        };
        // initParameters
        if let Some(ini) = files.map_ini.filter(|i| i.ready) {
            let f = |section: &str, key: &str| ini.get_float(section, key).unwrap_or(0.0);
            app.size_icon = f("DRIVER_ICON", "SIZE");
            app.zoom_section = f("MAP_SECTION", "ZOOM_ON_SECTION");
            app.max_dimension_form = f("MAP_FORM", "MAX_DIMENSION_FORM");
            app.starting_alpha = f("MAP_SECTION", "STARTING_ALPHA");
            app.finishing_alpha = f("MAP_SECTION", "FINISHING_ALPHA");
        }
        // loadMap
        let base = if app.track_config_name.is_empty() { format!("content\\tracks\\{}", app.track_name) } else { format!("content\\tracks\\{}\\{}", app.track_name, app.track_config_name) };
        let mut made_ini = None;
        if !painter.file_exists(&format!("{base}\\map.png")) {
            made_ini = Some(app.create_map(self, painter, &base, files.ai_points));
        }
        app.map = painter.get_texture(&format!("{base}\\map.png"));
        let (w, h) = if app.map.kid.is_some() { painter.texture_size(&app.map) } else { (0, 0) };
        let made = made_ini.as_ref().map(|text| IniReader::from_text(std::path::Path::new("map.ini"), text));
        app.tmc.load_parameters(made.as_ref().or(files.track_map_ini), w as f32, h as f32);
        app.original_width = w as f32;
        app.original_height = h as f32;
        let m = if (h as f32) <= (w as f32) { w as f32 } else { h as f32 };
        let mut scale = 1.0f32;
        let mut z = m;
        while app.max_dimension_form < z {
            scale = scale + f32::from_bits(0x3dcc_cccd);
            z = m / scale;
        }
        app.zoom_mode_width = z;
        app.zoom_mode_height = z;
        let inv = 1.0f32 / app.zoom_section;
        app.scaled_x = (z / app.original_width) * inv;
        app.scaled_z = (z / app.original_height) * inv;
        app.zw = z * 0.5;
        app.zh = z * 0.5;
        app.zh_top = (z * 4.0) * f32::from_bits(0x3e4c_cccd);
        app.zh_bottom = z * f32::from_bits(0x3e4c_cccd);
        let r = self.controls[form].rect;
        app.x_off = app.zw + r.left;
        app.z_off = app.zh_top + r.top;
        let (big, small) = if app.zoom_mode_width < app.zoom_mode_height { (app.zoom_mode_height, app.zoom_mode_width) } else { (app.zoom_mode_width, app.zoom_mode_height) };
        self.set_size(form, big, small);
        self.set_auto_hide_mode(form, true);
        app.adversary_icon = painter.get_texture("content/gui/ksmap/adversaryMapTexture.png");
        self.set_app(form, Box::new(app));
        form
    }
}

impl TrackMap {
    /// `TrackMap::createMap` 0x1402eeee0: the map drawn from the AI line into a render target
    /// and saved as the track's `map.png`, with its `data\map.ini`. The game writes both into
    /// its own folder; here the painter keeps the picture (under the path the game would
    /// write) and the parameters' file goes to whoever runs the GUI. Returns the file's text.
    fn create_map(&mut self, gui: &mut Gui, painter: &mut dyn Painter, base: &str, points: &[[f32; 3]]) -> String {
        self.tmc.calculate_all_map_parameters(points);
        if let Some(png) = self.draw_map_on_render_target(painter, &format!("{base}\\map.png")) {
            gui.apps.actions.push(AppAction::WriteFile { path: format!("{base}\\map.png"), bytes: png });
        }
        let text = self.tmc.parameters_text();
        gui.apps.actions.push(AppAction::WriteFile { path: format!("{base}\\data\\map.ini"), bytes: text.clone().into_bytes() });
        text
    }

    /// `TrackMapHandler::drawMapOnRenderTarget` 0x14033c810.
    fn draw_map_on_render_target(&self, painter: &mut dyn Painter, path: &str) -> Option<Vec<u8>> {
        let h = &self.tmc;
        let (w, ht) = (h.width, h.height);
        if !painter.begin_map_target(crate::app_essentials::cut_f(w), crate::app_essentials::cut_f(ht), w, ht) {
            return None;
        }
        let gl = GRAPHICS_GL;
        painter.gl_color4f(gl, 1.0, 1.0, 1.0, 1.0);
        if h.textured {
            painter.set_texture(0, &h.road_texture);
        }
        let n = h.points_to_draw.len() as u32;
        let p = &h.points_to_draw;
        let (mut a_prev, mut b_prev) = ([0.0f32; 2], [0.0f32; 2]);
        let (mut first_a, mut first_b) = ([0.0f32; 2], [0.0f32; 2]);
        let mut i: u32 = 1;
        while i < n {
            let (prev, cur) = (p[i as usize - 1], p[i as usize]);
            let mut dx = cur[0] - prev[0];
            let mut dz = cur[2] - prev[2];
            let len = (dz * dz + dx * dx).sqrt();
            let mut z0 = 0.0f32;
            if len != 0.0 {
                let inv = 1.0 / len;
                dx = dx * inv;
                z0 = inv * 0.0;
                dz = dz * inv;
            }
            z0 = z0 * 0.0;
            let nx = dx - z0;
            let nz = z0 - dz;
            let s = 1.0 / h.scale_factor;
            let (px, pz, cx, cz) = (s * prev[0], s * prev[2], s * cur[0], s * cur[2]);
            let half = h.drawing_size * 0.5;
            let ap = [half * nz + px, nx * half + pz];
            let bp = [px - half * nz, pz - nx * half];
            let ac = [cx + half * nz, nx * half + cz];
            let bc = [cx - half * nz, cz - nx * half];
            if i == 1 {
                a_prev = ap;
                b_prev = bp;
                first_a = ap;
                first_b = bp;
            }
            let closing = !((i as f32) < (n as f32) - h.finishing_spline_link_threshold_meters);
            painter.gl_begin(gl, crate::painter::GL_QUADS);
            if h.textured {
                painter.gl_tex_coord2f(gl, 0.0, 0.0);
            }
            painter.gl_vertex3f(gl, b_prev[0], b_prev[1], 0.0);
            if h.textured {
                painter.gl_tex_coord2f(gl, 1.0, 0.0);
            }
            painter.gl_vertex3f(gl, a_prev[0], a_prev[1], 0.0);
            if h.textured {
                painter.gl_tex_coord2f(gl, 1.0, 1.0);
            }
            let (c3, c4) = if !closing { (ac, bc) } else { (first_a, first_b) };
            painter.gl_vertex3f(gl, c3[0], c3[1], 0.0);
            if h.textured {
                painter.gl_tex_coord2f(gl, 0.0, 1.0);
            }
            painter.gl_vertex3f(gl, c4[0], c4[1], 0.0);
            painter.gl_end(gl);
            a_prev = ac;
            b_prev = bc;
            i += 1;
        }
        painter.end_map_target(path)
    }

    /// `TrackMap::showMapOnScreenZoom` 0x1402f00d0.
    fn show_map_on_screen_zoom(&mut self, gui: &mut Gui, id: Id, painter: &mut dyn Painter, pos: [f32; 3], dir: [f32; 3]) {
        let gl = GRAPHICS_GL;
        painter.set_texture(0, &self.map);
        let w = self.tmc.world_to_map(pos);
        let u = w[0] / self.original_width;
        let v = w[1] / self.original_height;
        let (mut dxn, mut dzn, mut t) = (dir[0], dir[2], 0.0f32);
        let len = (dir[2] * dir[2] + dir[0] * dir[0]).sqrt();
        if len != 0.0 {
            let inv = 1.0 / len;
            dxn = dir[0] * inv;
            t = inv * 0.0;
            dzn = dir[2] * inv;
        }
        let dot = ((t * 0.0) + (dxn * 0.0)) + dzn;
        let mut ang = if dot > -1.0 && dot < 1.0 { rustyac_math::acosf(dot) } else { 0.0 };
        if !(dxn > 0.0) {
            ang = -ang;
        }
        let (sup_x, inf_x) = combined(u, self.scaled_x);
        let (sup_z, inf_z) = combined(v, self.scaled_z);
        let r = gui.controls[id].rect;
        let mut others = Vec::new();
        let link = gui.apps.clone();
        for c in link.cars.iter().skip(1) {
            if c.connected {
                let m = c.body_matrix.m;
                let o = self.tmc.world_to_map([m[3][0], m[3][1], m[3][2]]);
                let x = ((((o[0] / self.original_width) - inf_x) / (sup_x - inf_x)) * 2.0) * self.zw - self.zw;
                let y = ((((o[1] / self.original_height) - inf_z) / (sup_z - inf_z)) * 2.0) * self.zh - self.zh;
                others.push([x, y]);
            }
        }
        self.x_off = self.zw + r.left;
        self.z_off = self.zh + r.top;
        let (zw, zh) = (self.zw, self.zh);
        painter.set_cull_mode(2);
        let a = rotate([-zw, -zh], ang);
        let b = rotate([zw, -zh], ang);
        let c = rotate([zw, zh], ang);
        let d = rotate([-zw, zh], ang);
        for p in others.iter_mut() {
            *p = rotate(*p, ang);
        }
        let uc = (sup_x + inf_x) * 0.5;
        let vc = (sup_z + inf_z) * 0.5;
        let (xo, zo) = (self.x_off, self.z_off);
        let (sa, fa) = (self.starting_alpha, self.finishing_alpha);
        painter.gl_begin(gl, GL_TRIANGLES);
        let fan = |painter: &mut dyn Painter, p: [f32; 2], q: [f32; 2], tp: [f32; 2], tq: [f32; 2]| {
            painter.gl_color4f(gl, 1.0, 1.0, 1.0, sa);
            painter.gl_tex_coord2f(gl, uc, vc);
            painter.gl_vertex3f(gl, xo, zo, 0.0);
            painter.gl_color4f(gl, 1.0, 1.0, 1.0, fa);
            painter.gl_tex_coord2f(gl, tp[0], tp[1]);
            painter.gl_vertex3f(gl, p[0] + xo, p[1] + zo, 0.0);
            painter.gl_color4f(gl, 1.0, 1.0, 1.0, fa);
            painter.gl_tex_coord2f(gl, tq[0], tq[1]);
            painter.gl_vertex3f(gl, q[0] + xo, q[1] + zo, 0.0);
        };
        fan(painter, a, b, [inf_x, inf_z], [sup_x, inf_z]);
        fan(painter, b, c, [sup_x, inf_z], [sup_x, sup_z]);
        fan(painter, c, d, [sup_x, sup_z], [inf_x, sup_z]);
        fan(painter, d, a, [inf_x, sup_z], [inf_x, inf_z]);
        painter.gl_end(gl);
        for p in &others {
            painter.gl_color4f(gl, 1.0, 1.0, 1.0, 1.0);
            let x = p[0] + xo;
            let z = p[1] + zo;
            if x > r.left && x < r.right && z > r.top && z < r.bottom {
                painter.set_texture(0, &self.adversary_icon);
                let zf = floor_cut(z - 7.5);
                let xf = floor_cut(x - 7.5);
                painter.gl_quad(gl, xf, zf, 15.0, 15.0, true);
            }
        }
        painter.gl_color4f(gl, 1.0, 0.0, 0.0, 1.0);
        let si = self.size_icon;
        painter.gl_begin(gl, GL_TRIANGLES);
        painter.gl_vertex3f(gl, xo, zo - si * 0.5, 0.0);
        painter.gl_vertex3f(gl, si + xo, si + zo, 0.0);
        let h = si * 0.5;
        painter.gl_vertex3f(gl, xo - h, h + zo, 0.0);
        painter.gl_end(gl);
        painter.gl_begin(gl, GL_TRIANGLES);
        painter.gl_vertex3f(gl, xo, zo - si * 0.5, 0.0);
        painter.gl_vertex3f(gl, xo - si, si + zo, 0.0);
        let h = si * 0.5;
        painter.gl_vertex3f(gl, h + xo, h + zo, 0.0);
        painter.gl_end(gl);
        painter.set_cull_mode(0);
    }
}

impl App for TrackMap {
    fn as_any(&mut self) -> &mut dyn Any {
        self
    }

    /// `TrackMap::render` 0x1402efd50.
    fn render(&mut self, gui: &mut Gui, id: Id, dt: f32, painter: &mut dyn Painter) {
        gui.control_render(id, dt, painter);
        let car0 = gui.apps.cars.first().cloned().unwrap_or_default();
        painter.override_sampler_state(1);
        if self.mode == 1 {
            let m = car0.body_matrix.m;
            let dir = [-m[2][0], -m[2][1], -m[2][2]];
            let pos = [m[3][0], m[3][1], m[3][2]];
            self.show_map_on_screen_zoom(gui, id, painter, pos, dir);
        }
        painter.override_sampler_state(0);
        let _ = self.zh_bottom;
    }
}
