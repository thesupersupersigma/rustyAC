// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni). Not covered by rustyAC's licenses — see LICENSING.md.

//! The track: what the car's wheels stand on and what times its laps.
//!
//! * [`surfaces`]: AC's `SurfacesManager` (surface types, name matching).
//! * [`Track`]: AC's physics `Track` (0x148 bytes): the collision meshes with their surfaces
//!   in ODE's static space, the ground ray, the timing lines, the AI line, the grip level.
//! * [`loader`]: the part of AC's `TrackAvatar` that builds a `Track` from a track folder:
//!   which meshes of the kn5 models are physical, the spawn points, the timing gates.
//! * [`spline`]: the AI line (`fast_lane.ai`) and a car's position along it.
//! * [`timing`]: timing lines, the lap timer of a car, the lap's validity.
//!
//! A [`Track`] never changes once it is loaded, so the game shares one between its physics
//! thread and its display (`Arc<Track>`); what does change during a session (the grip level,
//! each car's timer) lives in [`DynamicTrack`] and in the car.

pub mod loader;
pub mod spline;
pub mod surfaces;
pub mod timing;

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use rustyac_ode::collision::{RayContact, StaticWorld};

use crate::tyre::{RayCastResult, RayTrackCollisionProvider, SurfaceDef};
use crate::vecmath::{Mat44f, Vec3f};

pub use loader::{load_track, TrackLoadReport};
pub use spline::{AiSpline, SplineLocator};
pub use surfaces::{SurfaceType, SurfacesManager};
pub use timing::{LapInvalidator, TimeLine, TimeTransponder};

/// The collide bits every track mesh gets (`Track::addSurface` @ 0x140277e50).
pub const TRACK_MESH_COLLIDE_BITS: u32 = 0x14;

/// One collision mesh of the track with the copy of its surface (`CollisionMeshODE` and the
/// `SurfaceDef` behind its user pointer).
#[derive(Clone, Debug, PartialEq)]
pub struct TrackSurface {
    /// The mesh's name in the kn5 (the game does not keep it; the display does).
    pub name: String,
    /// The surface type's `KEY`, or empty for the default surface.
    pub key: String,
    /// `wavString`
    pub wav: String,
    pub surface_def: SurfaceDef,
}

/// A ray's answer with what the game's `RayCastHit` adds to the tyre's view of it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TrackRayHit {
    pub contact: RayContact,
    pub surface_def: SurfaceDef,
}

/// AC's physics `Track`.
#[derive(Clone, Debug, Default)]
pub struct Track {
    /// `name`, `config`: the track's folder name and its layout ("" for none).
    pub name: String,
    pub config: String,
    /// `dataFolder`: `content/tracks/<name>[/<config>]`
    pub data_folder: PathBuf,
    /// `PhysicsCore::spaceStatic` with the track's meshes.
    pub world: StaticWorld,
    /// `surfaces`: one per mesh of [`Track::world`], in the same order.
    pub surfaces: Vec<TrackSurface>,
    /// `timeLines`: 0 is start / finish, the others end a sector each.
    pub time_lines: Vec<TimeLine>,
    /// `sectorsNormalizedPositions`: where each timing line is along the AI line.
    pub sectors_normalized_positions: Vec<f32>,
    /// `isOpen`: a point-to-point track (A to B lines).
    pub is_open: bool,
    /// `aiSplineRecorder->bestLapSpline`: the AI line, where the track has one.
    pub ai_spline: Option<AiSpline>,
    /// `aiSplineRecorder->pitLaneSpline`
    pub pit_lane_spline: Option<AiSpline>,
    /// `startingBounds` (`data/starting_bounds.ini`)
    pub starting_bounds: Vec<(f32, f32)>,
    /// `TrackAvatar::spawnPositions`: set name (`PIT`, `START`, `HOTLAP_START` ...) -> the
    /// slots' matrices, already put on the ground.
    pub spawn_positions: BTreeMap<String, Vec<Mat44f>>,
}

impl Track {
    /// `Track::addSurface` @ 0x140277e50: a collision mesh with its own copy of the surface,
    /// in the sub-space `sub_space_id`. Returns the mesh's index.
    pub fn add_surface(&mut self, name: &str, key: &str, wav: &str, vertices: Vec<[f32; 3]>, indices: Vec<u16>, surface_def: &SurfaceDef, sub_space_id: u32) -> usize {
        let index = self.world.create_tri_mesh(vertices, indices, surface_def.collision_category, TRACK_MESH_COLLIDE_BITS, sub_space_id);
        let mut surface_def = *surface_def;
        // the port's stand-in for the game's mesh pointer
        surface_def.user_pointer = index as u32;
        self.surfaces.push(TrackSurface { name: name.to_string(), key: key.to_string(), wav: wav.to_string(), surface_def });
        index
    }

    /// `PhysicsCore::rayCast` @ 0x1402cd070 with the surface of the mesh that was hit
    /// (`Track::rayCast` @ 0x140278bb0 for the shared ray, `RayCaster::rayCast` @
    /// 0x1402cee90 for a tyre's own).
    pub fn ray_cast_hit(&self, org: &Vec3f, dir: &Vec3f, length: f32) -> Option<TrackRayHit> {
        let contact = self.world.ray_cast(&[org.x, org.y, org.z], &[dir.x, dir.y, dir.z], length)?;
        Some(TrackRayHit { contact, surface_def: self.surfaces[contact.mesh].surface_def })
    }

    /// The name of the surface type a `SurfaceDef` of this track came from, for the display.
    pub fn surface_key(&self, surface_def: &SurfaceDef) -> &str {
        match self.surfaces.get(surface_def.user_pointer as usize) {
            Some(surface) if surface.key.is_empty() => "?",
            Some(surface) => &surface.key,
            None => "",
        }
    }

    /// The lap's length: the AI line's, 0 without one.
    pub fn length(&self) -> f32 {
        self.ai_spline.as_ref().map(|s| s.length()).unwrap_or(0.0)
    }
}

impl RayTrackCollisionProvider for Track {
    fn ray_cast(&self, org: &Vec3f, dir: &Vec3f, length: f32) -> Option<RayCastResult> {
        let hit = self.ray_cast_hit(org, dir, length)?;
        let (p, n) = (hit.contact.pos, hit.contact.normal);
        Some(RayCastResult { surface_def: hit.surface_def, pos: Vec3f::new(p[0], p[1], p[2]), normal: Vec3f::new(n[0], n[1], n[2]) })
    }

    /// `Track::createRayCaster` @ 0x1402781f0: a real track gives every tyre its own ray.
    fn has_ray_caster(&self) -> bool {
        true
    }
}

/// The road a car stands on when it is a [`Track`] shared with others.
#[derive(Clone, Debug)]
pub struct TrackGround(pub Arc<Track>);

impl RayTrackCollisionProvider for TrackGround {
    fn ray_cast(&self, org: &Vec3f, dir: &Vec3f, length: f32) -> Option<RayCastResult> {
        self.0.ray_cast(org, dir, length)
    }

    fn has_ray_caster(&self) -> bool {
        true
    }
}

/// `DynamicTrackData` and `Track::dynamicGripLevel`: the one grip number of the whole track.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DynamicTrack {
    /// `isExternal`: a server owns the level.
    pub is_external: bool,
    /// `enabled`: `cfg/race.ini` has a `[DYNAMIC_TRACK]` section.
    pub enabled: bool,
    /// `sessionStartGrip` (`SESSION_START` / 100, at least 0.85)
    pub session_start_grip: f32,
    /// `baseGrip`: the session's level before any lap
    pub base_grip: f32,
    /// `randomGrip` (`RANDOMNESS` / 100)
    pub random_grip: f32,
    /// `gripPerLap` (0.01 / `LAP_GAIN`)
    pub grip_per_lap: f32,
    /// `sessionTransfer` (`SESSION_TRANSFER` / 100)
    pub session_transfer: f32,
    /// `Track::dynamicGripLevel`: what the tyres multiply their grip by.
    pub dynamic_grip_level: f32,
}

impl Default for DynamicTrack {
    /// `Track::Track` @ 0x140277100 without a `[DYNAMIC_TRACK]` section: off, full grip.
    fn default() -> DynamicTrack {
        DynamicTrack {
            is_external: false,
            enabled: false,
            session_start_grip: 1.0,
            base_grip: 1.0,
            random_grip: 0.01,
            grip_per_lap: 0.1,
            session_transfer: 0.0,
            dynamic_grip_level: 1.0,
        }
    }
}

/// `(float)rand() * 3.0518509e-05 * 2 - 1`: the game's random number in -1..1.
fn random_unit(rand: i32) -> f32 {
    rand as f32 * 3.051_850_9e-5 * 2.0 - 1.0
}

impl DynamicTrack {
    /// `Track::initDynamicTrack` @ 0x140278300 for a `[DYNAMIC_TRACK]` section with these
    /// values (as written in the file); `rand` is the C runtime's `rand()` at that moment.
    pub fn from_race_ini(session_start: f32, randomness: f32, lap_gain: f32, session_transfer: f32, rand: i32) -> DynamicTrack {
        let mut track = DynamicTrack { enabled: true, ..DynamicTrack::default() };
        track.grip_per_lap = if lap_gain > 0.0 { 0.01 / lap_gain } else { 0.0 };
        track.session_start_grip = session_start * 0.01;
        if track.session_start_grip < 0.85 {
            track.session_start_grip = 0.85;
        }
        track.random_grip = randomness * 0.01;
        track.session_transfer = session_transfer * 0.01;
        track.base_grip = random_unit(rand) * track.random_grip + track.session_start_grip;
        track
    }

    /// The handler `Track` hangs on a new session (lambda @ 0x140277740).
    pub fn on_new_session(&mut self, session_index: i32, rand: i32) {
        if !self.enabled || self.is_external {
            return;
        }
        if session_index == 0 {
            self.base_grip = random_unit(rand) * self.random_grip + self.session_start_grip;
        } else {
            let carried = (self.dynamic_grip_level - self.session_start_grip) * self.session_transfer + self.session_start_grip;
            self.base_grip = random_unit(rand) * self.random_grip + carried;
        }
    }

    /// `Track::step` @ 0x140278d20, the grip part: `laps` is the sum of every car's lap count.
    pub fn step(&mut self, laps: i32) {
        if self.enabled {
            let v = laps as f32 * self.grip_per_lap + self.base_grip;
            self.dynamic_grip_level = if v > 1.0 {
                1.0
            } else if !(v >= 0.85) {
                0.85
            } else {
                v
            };
        } else if !self.is_external {
            self.dynamic_grip_level = 1.0;
        }
    }

    /// `Track::setGripLevelExternal` @ 0x140278d00
    pub fn set_grip_level_external(&mut self, grip: f32) {
        self.dynamic_grip_level = grip;
        self.is_external = true;
        self.enabled = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_grip_level_is_kept_between_85_and_100_percent() {
        let mut track = DynamicTrack::from_race_ini(96.0, 0.0, 10.0, 50.0, 0);
        let start = 96.0f32 * 0.01;
        assert_eq!((track.session_start_grip, track.grip_per_lap, track.base_grip), (start, 0.01f32 / 10.0, -1.0f32 * 0.0 + start));
        track.step(0);
        assert_eq!(track.dynamic_grip_level, track.base_grip);
        track.step(10);
        assert_eq!(track.dynamic_grip_level, 10.0f32 * track.grip_per_lap + track.base_grip);
        track.step(1000);
        assert_eq!(track.dynamic_grip_level, 1.0);
        track.base_grip = 0.2;
        track.step(0);
        assert_eq!(track.dynamic_grip_level, 0.85);
        track.base_grip = f32::NAN;
        track.step(0);
        assert_eq!(track.dynamic_grip_level, 0.85);
        // a low start is raised, a missing gain means no gain
        let low = DynamicTrack::from_race_ini(50.0, 0.0, 0.0, 0.0, 0);
        assert_eq!((low.session_start_grip, low.grip_per_lap), (0.85, 0.0));
    }

    #[test]
    fn without_the_section_the_grip_is_full_and_a_server_can_own_it() {
        let mut track = DynamicTrack::default();
        track.dynamic_grip_level = 0.9;
        track.step(5);
        assert_eq!(track.dynamic_grip_level, 1.0);
        track.set_grip_level_external(0.93);
        track.step(5);
        assert_eq!(track.dynamic_grip_level, 0.93);
    }

    #[test]
    fn a_new_session_carries_part_of_the_gain_over() {
        let mut track = DynamicTrack::from_race_ini(90.0, 0.0, 1.0, 50.0, 0);
        track.step(4);
        let start = track.session_start_grip;
        assert_eq!(track.dynamic_grip_level, 4.0f32 * track.grip_per_lap + track.base_grip);
        let level = track.dynamic_grip_level;
        assert!(level > start);
        track.on_new_session(1, 0);
        assert_eq!(track.base_grip, -1.0f32 * 0.0 + ((level - start) * 0.5 + start));
        track.on_new_session(0, 0);
        assert_eq!(track.base_grip, -1.0f32 * 0.0 + start);
    }
}
