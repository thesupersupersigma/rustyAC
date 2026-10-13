// SPDX-License-Identifier: GPL-3.0-or-later

//! The port's physics track as a replay's playback asks it ([`PlaybackTrack`]): the best-lap
//! AI line for a car's place along the lap, a ray down for the ground under a tyre.

use super::recorder::PlaybackTrack;
use crate::car_physics_state::SurfaceDef;
use crate::track::Track;
use crate::vecmath::Vec3f;

/// A surface of the track as a `CarPhysicsState` carries it. The surface's sector and its
/// collision category are not kept by the port's track: they are 0 here.
pub fn surface_of(track: &Track, mesh: usize) -> SurfaceDef {
    let surface = &track.surfaces[mesh];
    let d = &surface.surface_def;
    let mut wav_string = [0u16; 64];
    for (slot, unit) in wav_string.iter_mut().zip(surface.wav.encode_utf16().take(63)) {
        *slot = unit;
    }
    SurfaceDef {
        wav_string,
        wav_pitch_speed: d.wav_pitch_speed,
        grip_mod: d.grip_mod,
        sector_id: 0,
        dirt_additive_k: d.dirt_additive_k,
        collision_category: 0,
        is_valid_track: d.is_valid_track,
        black_flag_time: d.black_flag_time,
        sin_height: d.sin_height,
        sin_length: d.sin_length,
        is_pitlane: d.is_pitlane,
        damping: d.damping,
        granularity: d.granularity,
        vibration_gain: d.vibration_gain,
        vibration_length: d.vibration_length,
    }
}

impl PlaybackTrack for Track {
    fn world_to_spline(&self, pos: &[f32; 3]) -> f32 {
        match &self.ai_spline {
            Some(line) => line.spline.world_to_spline(&[pos[0], pos[1], pos[2]], -1),
            None => 0.0,
        }
    }

    fn ray_cast(&self, origin: &[f32; 3], direction: &[f32; 3], length: f32) -> Option<(SurfaceDef, [f32; 3], [f32; 3])> {
        let hit = self.ray_cast_hit(&Vec3f::new(origin[0], origin[1], origin[2]), &Vec3f::new(direction[0], direction[1], direction[2]), length)?;
        Some((surface_of(self, hit.contact.mesh), hit.contact.pos, hit.contact.normal))
    }
}
