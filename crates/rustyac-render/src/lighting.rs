// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! Lighting from the ini files: `LightingSettings`, `GraphicsManager::loadLightingSettings`
//! 0x140203250, `GraphicsManager::updateLightingSetttings` 0x140205190,
//! `WeatherGenerator::loadPreset` 0x140227260, `WeatherManager::applyCustomWeather`
//! 0x1401d8110.
//!
//! "colorCurves.ini" holds no curves: eight colours (horizon, sky, sun, ambient, each low and
//! high), a gamma and a factor for when post-processing is off. Everything the shaders get is
//! one blend between the low and the high colour by how high the sun stands.

use std::path::Path;

use rustyac_math::{powf, sqrtf, wcstod};
use rustyac_physics::data::ini::IniReader;
use rustyac_physics::vecmath::{xm_matrix_multiply, Mat44f, Vec3f};

use crate::camera::DEG_TO_RAD;
use crate::graphics::Graphics;

/// `LightingSettings` (0xb4 bytes).
#[derive(Clone, Debug)]
pub struct LightingSettings {
    pub light_direction: Vec3f,
    pub light_color: Vec3f,
    pub horizon_low: Vec3f,
    pub horizon_high: Vec3f,
    pub sky_low: Vec3f,
    pub sky_high: Vec3f,
    pub sun_low: Vec3f,
    pub sun_high: Vec3f,
    /// race.ini `[LIGHTING] SUN_ANGLE`, degrees
    pub angle: f32,
    /// the track's `data/lighting.ini`, degrees
    pub heading_angle: f32,
    pub pitch_angle: f32,
    pub ambient_low: Vec3f,
    pub ambient_high: Vec3f,
    pub fog_color: Vec3f,
    pub fog_linear: f32,
    pub fog_blend: f32,
    pub cloud_cover: f32,
    pub cloud_cutoff: f32,
    pub cloud_color: f32,
    pub cloud_offset: f32,
    pub saturation: f32,
    pub game_time: f32,
    pub sun_angle_gamma: f32,
}

impl LightingSettings {
    /// `LightingSettings::LightingSettings` 0x140201da0.
    pub fn new() -> LightingSettings {
        let one = Vec3f::new(1.0, 1.0, 1.0);
        LightingSettings {
            light_direction: Vec3f::new(0.0, -1.0, 0.0),
            light_color: Vec3f::new(1.0, f32::from_bits(0x3f78_51ec), f32::from_bits(0x3f6b_851f)),
            horizon_low: one,
            horizon_high: one,
            sky_low: one,
            sky_high: one,
            sun_low: one,
            sun_high: one,
            angle: 0.0,
            heading_angle: 0.0,
            pitch_angle: 45.0,
            ambient_low: one,
            ambient_high: one,
            fog_color: Vec3f::new(f32::from_bits(0x3dcc_cccd), f32::from_bits(0x3f33_3333), f32::from_bits(0x3f66_6666)),
            fog_linear: 2000.0,
            fog_blend: 0.5,
            cloud_cover: f32::from_bits(0x3f19_999a),
            cloud_cutoff: f32::from_bits(0x3f33_3333),
            cloud_color: f32::from_bits(0x3e4c_cccd),
            cloud_offset: 0.0,
            saturation: 1.0,
            game_time: 0.0,
            sun_angle_gamma: 1.0,
        }
    }
}

impl Default for LightingSettings {
    fn default() -> LightingSettings {
        LightingSettings::new()
    }
}

fn get_float(ini: &IniReader, section: &str, key: &str) -> f32 {
    ini.get_float(section, key).unwrap_or(0.0)
}

/// `INIReader::getFloat4` 0x140235860 (`getVector4` 0x140236ad0): three pieces ended by a
/// comma and whatever follows up to the end of the line; a piece that is not there stays 0.
fn get_float4(ini: &IniReader, section: &str, key: &str) -> [f32; 4] {
    let text = ini.get_string(section, key);
    let mut out = [0.0f32; 4];
    let mut rest = text.as_str();
    for (index, slot) in out.iter_mut().enumerate() {
        let delimiters: &[char] = if index < 3 { &[','] } else { &['\n', '\r'] };
        let start = rest.trim_start_matches(delimiters);
        if start.is_empty() {
            break;
        }
        let (token, after) = match start.find(delimiters) {
            Some(end) => (&start[..end], &start[end + 1..]),
            None => (start, ""),
        };
        rest = after;
        *slot = wcstod(token).value as f32;
    }
    out
}

impl Graphics {
    /// `GraphicsManager::loadLightingSettings` 0x140203250.
    pub fn load_lighting_settings(&mut self, path: &Path) {
        let Ok(ini) = IniReader::load(path) else {
            return;
        };
        let version = ini.get_int("HEADER", "VERSION").unwrap_or(0);
        let mut m = 1.0f32;
        if !self.video.pp_hdr_enabled {
            m = get_float(&ini, "HEADER", "HDR_OFF_MULT");
        }
        let k = f32::from_bits(0x3b80_8081); // a multiply by this, not a division by 255
        if version >= 2 {
            let f = |section: &str, key: &str| {
                let v = get_float4(&ini, section, key);
                let w = v[3];
                let mut z = w * v[2];
                let mut y = w * v[1];
                let mut x = w * v[0];
                z *= k;
                y *= k;
                x *= k;
                z *= m;
                y *= m;
                x *= m;
                Vec3f::new(x, y, z)
            };
            self.lighting.horizon_high = f("HORIZON", "HIGH");
            self.lighting.horizon_low = f("HORIZON", "LOW");
            self.lighting.sky_low = f("SKY", "LOW");
            self.lighting.sky_high = f("SKY", "HIGH");
            self.lighting.sun_low = f("SUN", "LOW");
            self.lighting.sun_high = f("SUN", "HIGH");
            self.lighting.ambient_low = f("AMBIENT", "LOW");
            self.lighting.ambient_high = f("AMBIENT", "HIGH");
        } else {
            let f = |section: &str, key: &str| {
                let v = ini.get_float3(section, key).unwrap_or([0.0; 3]);
                Vec3f::new(v[0] * k, v[1] * k, v[2] * k)
            };
            self.lighting.horizon_high = f("HORIZON", "HIGH");
            self.lighting.horizon_low = f("HORIZON", "LOW");
            self.lighting.sky_low = f("SKY", "LOW");
            self.lighting.sky_high = f("SKY", "HIGH");
            self.lighting.sun_low = f("SUN", "LOW");
            self.lighting.sun_high = f("SUN", "HIGH");
            self.lighting.ambient_low = f("AMBIENT", "LOW");
            let v = ini.get_float3("AMBIENT", "HIGH").unwrap_or([0.0; 3]);
            let inv = 1.0f32 / 255.0;
            self.lighting.ambient_high = Vec3f::new(inv * v[0], inv * v[1], inv * v[2]);
        }
        self.lighting.sun_angle_gamma = get_float(&ini, "HEADER", "ANGLE_GAMMA");
        self.update_lighting_settings();
    }

    /// `GraphicsManager::updateLightingSetttings` 0x140205190: the sun direction from the three
    /// angles, then every field of the lighting buffer.
    pub fn update_lighting_settings(&mut self) {
        if !self.use_custom_sun_direction {
            let ls = &self.lighting;
            let p = Mat44f::create_from_axis_angle(&Vec3f::new(1.0, 0.0, 0.0), ls.pitch_angle * DEG_TO_RAD);
            let a = Mat44f::create_from_axis_angle(&Vec3f::new(0.0, 0.0, 1.0), ls.angle * DEG_TO_RAD);
            let r1 = xm_matrix_multiply(&p, &a);
            let h = Mat44f::create_from_axis_angle(&Vec3f::new(0.0, 1.0, 0.0), ls.heading_angle * DEG_TO_RAD);
            let r = xm_matrix_multiply(&r1, &h);
            // (0, -1, 0) as a row vector through R: the products by 0 and by -1 are real
            let mut v = [0.0f32; 3];
            for (c, slot) in v.iter_mut().enumerate() {
                *slot = (((r.m[1][c] * -1.0) + (r.m[0][c] * 0.0)) + (r.m[2][c] * 0.0)) + r.m[3][c];
            }
            let len = sqrtf(((v[1] * v[1]) + (v[0] * v[0])) + (v[2] * v[2]));
            if len != 0.0 && !len.is_nan() {
                let inv = 1.0 / len;
                v[0] *= inv;
                v[1] *= inv;
                v[2] *= inv;
            }
            self.lighting.light_direction = Vec3f::new(v[0], v[1], v[2]);
        } else {
            self.lighting.light_direction = self.custom_sun_direction;
        }
        let ls = self.lighting.clone();
        let cb = &mut self.cb_lighting;
        cb.set_f32s(&[ls.light_direction.x, ls.light_direction.y, ls.light_direction.z, 0.0], 0x00);
        let mut t = -ls.light_direction.y;
        t = if t > 1.0 {
            1.0
        } else if t >= 0.0 {
            t
        } else {
            0.0
        };
        let k = powf(1.0 - t, ls.sun_angle_gamma);
        let lerp = |low: Vec3f, high: Vec3f| [((low.x - high.x) * k) + high.x, ((low.y - high.y) * k) + high.y, ((low.z - high.z) * k) + high.z];
        cb.set_f32s(&lerp(ls.ambient_low, ls.ambient_high), 0x10);
        let sun = lerp(ls.sun_low, ls.sun_high);
        cb.set_f32s(&[sun[0], sun[1], sun[2], 0.0], 0x20);
        cb.set_f32s(&lerp(ls.horizon_low, ls.horizon_high), 0x30);
        cb.set_f32s(&lerp(ls.sky_low, ls.sky_high), 0x40);
        cb.set_f32(ls.fog_linear, 0x5c);
        cb.set_f32(ls.fog_blend, 0x60);
        cb.set_f32s(&[ls.fog_color.x, ls.fog_color.y, ls.fog_color.z], 0x64);
        cb.set_f32(ls.cloud_cover, 0x70);
        cb.set_f32(ls.cloud_cutoff, 0x74);
        cb.set_f32(ls.cloud_color, 0x78);
        cb.set_f32(ls.cloud_offset, 0x7c);
        cb.set_f32(ls.saturation, 0x90);
        cb.set_f32(ls.game_time, 0x94);
    }

    /// `WeatherGenerator::loadPreset` 0x140227260: fog and cloud numbers of a `weather.ini`.
    pub fn load_weather_preset(&mut self, file: &Path, m: f32) -> bool {
        let Ok(ini) = IniReader::load(file) else {
            println!("ERROR: Could not open: {}", file.display());
            self.lighting.fog_linear = 1.0;
            return false;
        };
        let ls = &mut self.lighting;
        ls.cloud_color = get_float(&ini, "CLOUDS", "COLOR") * m;
        ls.cloud_cover = get_float(&ini, "CLOUDS", "COVER");
        ls.cloud_cutoff = get_float(&ini, "CLOUDS", "CUTOFF");
        let v = ini.get_float3("FOG", "COLOR").unwrap_or([0.0; 3]);
        ls.fog_color = Vec3f::new(m * v[0], m * v[1], m * v[2]);
        ls.fog_blend = get_float(&ini, "FOG", "BLEND");
        ls.fog_linear = get_float(&ini, "FOG", "DISTANCE");
        if ls.fog_linear == 0.0 || ls.fog_linear.is_nan() {
            ls.fog_linear = 1.0;
        }
        self.update_lighting_settings();
        true
    }

    /// `WeatherManager::applyCustomWeather` 0x1401d8110: `content/weather/<name>/weather.ini`
    /// and `colorCurves.ini`.
    pub fn apply_custom_weather(&mut self, name: &str) {
        println!("Loading custom weather: {name}");
        let folder = self.game_folder.join("content/weather").join(name);
        let weather_ini = folder.join("weather.ini");
        let curves_ini = folder.join("colorCurves.ini");
        let mut m = 1.0f32;
        if !self.video.pp_hdr_enabled {
            if let Ok(c) = IniReader::load(&curves_ini) {
                m = get_float(&c, "HEADER", "HDR_OFF_MULT");
            }
        }
        if weather_ini.is_file() {
            self.load_weather_preset(&weather_ini, m);
        } else {
            println!("ERROR: Could not find: {}", weather_ini.display());
        }
        if curves_ini.is_file() {
            self.load_lighting_settings(&curves_ini);
        } else {
            println!("ERROR: Could not find: {}", curves_ini.display());
        }
    }

    /// What `RaceManager::initLighting` 0x14013a3d0 does with race.ini `[LIGHTING] SUN_ANGLE`.
    pub fn set_sun_angle(&mut self, angle: f32) {
        self.lighting.angle = angle;
        self.update_lighting_settings();
    }

    /// What `TrackAvatar::TrackAvatar` 0x1401c5250 does with the track's `data/lighting.ini`.
    pub fn load_track_lighting(&mut self, lighting_ini: &Path) {
        if let Ok(ini) = IniReader::load(lighting_ini) {
            self.lighting.pitch_angle = get_float(&ini, "LIGHTING", "SUN_PITCH_ANGLE");
            self.lighting.heading_angle = get_float(&ini, "LIGHTING", "SUN_HEADING_ANGLE");
            self.update_lighting_settings();
        }
    }
}
