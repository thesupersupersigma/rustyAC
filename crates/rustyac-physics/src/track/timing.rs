// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni). Not covered by rustyAC's licenses — see LICENSING.md.

//! Timing lines, a car's lap timer and the lap's validity; the spawn points. (Being ported.)

use super::loader::HelperNode;
use super::Track;
use crate::tyre::RayTrackCollisionProvider;
use crate::vecmath::Vec3f;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct TimeLine {}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct TimeTransponder {}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct LapInvalidator {}

/// `TrackAvatar::initRespawnPositionSet` @ 0x1401cb8c0: the nodes `AC_<set>_0`, `AC_<set>_1`
/// ... up to the first missing number, each dropped onto the road with a ray from 10 m above.
pub fn init_respawn_position_set(track: &mut Track, helpers: &[HelperNode], set: &str) {
    let mut slots = Vec::new();
    for n in 0.. {
        let name = format!("AC_{set}_{n}");
        let Some(helper) = helpers.iter().find(|h| h.name == name) else { break };
        let mut matrix = helper.world;
        let p = matrix.m[3];
        if let Some(hit) = track.ray_cast(&Vec3f::new(p[0], p[1] + 10.0, p[2]), &Vec3f::new(0.0, -1.0, 0.0), 100.0) {
            matrix.m[3][1] = hit.pos.y;
        }
        slots.push(matrix);
    }
    if !slots.is_empty() {
        track.spawn_positions.insert(set.to_string(), slots);
    }
}

pub fn init_time_lines(_track: &mut Track, _helpers: &[HelperNode], _messages: &mut Vec<String>) {}

impl Track {
    /// Where a car is put for a spawn slot: the point on the road and the direction of its
    /// tail (what `Car::forceRotation` takes).
    pub fn spawn_pose(&self, set: &str, index: usize) -> Option<(Vec3f, Vec3f)> {
        let m = self.spawn_positions.get(set)?.get(index)?.m;
        Some((Vec3f::new(m[3][0], m[3][1], m[3][2]), Vec3f::new(-m[2][0], -m[2][1], -m[2][2])))
    }

    /// The nearest point of the AI line to `position`, and the tail direction of a car that
    /// drives along the line there.
    pub fn pose_on_ai_line(&self, _position: &Vec3f) -> Option<(Vec3f, Vec3f)> {
        None
    }
}
