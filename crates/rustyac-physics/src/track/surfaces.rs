// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! AC's `SurfacesManager`: the surface types of a track (`surfaces.ini`) and which one a
//! mesh gets from its name.
//!
//! A mesh of a track model is physical when its name starts with a number other than 0
//! (`01ROAD`, `12GRASS_07`, `3WALL_pit`). The number is the group it belongs to; the surface
//! is the one entry whose `KEY` occurs somewhere in the upper-cased name. No entry, or more
//! than one, gives a default surface that the wheels still stand on.

use std::collections::BTreeMap;
use std::path::Path;

use crate::data::ini::IniReader;
use crate::math::wcstol;
use crate::tyre::SurfaceDef;

/// A surface type: AC's `SurfaceDef` and the one member that is not a number.
#[derive(Clone, Debug, PartialEq)]
pub struct SurfaceType {
    pub def: SurfaceDef,
    /// `wavString`: the sound of tyres on it.
    pub wav: String,
}

/// What a name matched ([`SurfacesManager::get_surface`]).
#[derive(Clone, Debug, PartialEq)]
pub struct Match {
    pub surface: SurfaceType,
    /// The matching `KEY`; `None` for the default surface.
    pub key: Option<String>,
    /// The game's console message when nothing or too much matched.
    pub error: Option<String>,
}

/// AC's `SurfacesManager` (0x20 bytes).
#[derive(Clone, Debug, PartialEq)]
pub struct SurfacesManager {
    /// `surfaces`: `KEY` -> surface, in the order of the game's `std::map`.
    pub surfaces: BTreeMap<String, SurfaceType>,
    /// Files that were asked for and not found (the game prints a warning for each).
    pub missing: Vec<String>,
}

/// What every surface is before its keys are read: zeros.
fn blank() -> SurfaceDef {
    SurfaceDef {
        grip_mod: 0.0,
        dirt_additive_k: 0.0,
        sin_height: 0.0,
        sin_length: 0.0,
        damping: 0.0,
        granularity: 0.0,
        is_valid_track: false,
        is_pitlane: false,
        vibration_gain: 0.0,
        vibration_length: 0.0,
        wav_pitch_speed: 0.0,
        black_flag_time: 0.0,
        collision_category: 0,
        sector_id: 0,
        user_pointer: u32::MAX,
    }
}

/// The surface of a mesh no key (or more than one) matched: full grip, on track, and nothing
/// the car's body collides with (category 0).
pub fn no_match_surface() -> SurfaceType {
    SurfaceType { def: SurfaceDef { grip_mod: 1.0, is_valid_track: true, vibration_length: 1.5, ..blank() }, wav: String::new() }
}

/// `NKUtils::getSectorID` @ 0x14018d2b0: the number a name starts with (`std::stoi`, whose
/// exceptions the function swallows): 0 when there is none or it does not fit.
pub fn get_sector_id(name: &str) -> i32 {
    let parsed = wcstol(name);
    if parsed.consumed == 0 || parsed.out_of_range {
        0
    } else {
        parsed.value
    }
}

/// The upper-casing of `TrackAvatar::getSurfaceDescFromMeshName`: the C runtime's narrow
/// `toupper` on every UTF-16 unit, which in the "C" locale changes `a`..`z` only.
pub fn upper_case(name: &str) -> String {
    name.chars().map(|c| if c.is_ascii_lowercase() { c.to_ascii_uppercase() } else { c }).collect()
}

impl SurfacesManager {
    /// `SurfacesManager::SurfacesManager` @ 0x1401ae9b0: the built-in wall, the game's own
    /// `system/data/surfaces.ini`, then the track's `data/surfaces.ini` (same keys replace).
    pub fn new(system_surfaces: &Path, track_surfaces: &Path) -> Result<SurfacesManager, String> {
        let mut manager = SurfacesManager { surfaces: BTreeMap::new(), missing: Vec::new() };
        manager.surfaces.insert(
            "WALL".to_string(),
            SurfaceType { def: SurfaceDef { grip_mod: 1.0, collision_category: 2, is_valid_track: true, vibration_length: 1.5, ..blank() }, wav: String::new() },
        );
        manager.load_surface_definitions(system_surfaces)?;
        manager.load_surface_definitions(track_surfaces)?;
        Ok(manager)
    }

    /// `SurfacesManager::loadSurfaceDefinitions` @ 0x1401afad0: `[SURFACE_0]`, `[SURFACE_1]` ...
    /// up to the first number that is missing.
    pub fn load_surface_definitions(&mut self, path: &Path) -> Result<(), String> {
        let ini = IniReader::load(path)?;
        if !ini.ready {
            self.missing.push(path.display().to_string());
            return Ok(());
        }
        self.read_surface_definitions(&ini)
    }

    /// The loop of `loadSurfaceDefinitions` over an ini that was read.
    pub fn read_surface_definitions(&mut self, ini: &IniReader) -> Result<(), String> {
        for n in 0.. {
            let section = format!("SURFACE_{n}");
            if !ini.has_section(&section) {
                break;
            }
            let mut d = blank();
            d.dirt_additive_k = ini.get_float(&section, "DIRT_ADDITIVE")?;
            d.grip_mod = ini.get_float(&section, "FRICTION")?;
            d.collision_category = 1;
            let wav = ini.get_string(&section, "WAV");
            d.wav_pitch_speed = ini.get_float(&section, "WAV_PITCH")?;
            d.black_flag_time = ini.get_float(&section, "BLACK_FLAG_TIME")?;
            d.is_valid_track = ini.get_int(&section, "IS_VALID_TRACK")? != 0;
            d.sin_height = ini.get_float(&section, "SIN_HEIGHT")?;
            d.sin_length = ini.get_float(&section, "SIN_LENGTH")?;
            d.is_pitlane = ini.get_int(&section, "IS_PITLANE")? != 0;
            d.damping = ini.get_float(&section, "DAMPING")?;
            d.vibration_gain = ini.get_float(&section, "VIBRATION_GAIN")?;
            d.vibration_length = ini.get_float(&section, "VIBRATION_LENGTH")?;
            self.surfaces.insert(ini.get_string(&section, "KEY"), SurfaceType { def: d, wav });
        }
        Ok(())
    }

    /// `SurfacesManager::getSurface` @ 0x1401af340: the one surface whose key occurs in
    /// `name` (already upper-cased); with none or several, the default surface.
    pub fn get_surface(&self, name: &str) -> Match {
        // an empty key occurs in every name, as `std::wstring::find` has it
        let hits: Vec<(&String, &SurfaceType)> = self.surfaces.iter().filter(|(key, _)| name.contains(key.as_str())).collect();
        match hits.as_slice() {
            [(key, surface)] => Match { surface: (*surface).clone(), key: Some((*key).clone()), error: None },
            [] => Match { surface: no_match_surface(), key: None, error: Some(format!("ERROR: SURFACE NOT FOUND FOR OBJECT:{name}")) },
            many => {
                let keys: Vec<&str> = many.iter().map(|(key, _)| key.as_str()).collect();
                Match { surface: no_match_surface(), key: None, error: Some(format!("ERROR: Mesh {name} CAN BE: {}", keys.join(" "))) }
            }
        }
    }

    /// `TrackAvatar::getSurfaceDescFromMeshName` @ 0x1401c8300.
    pub fn get_surface_desc_from_mesh_name(&self, name: &str) -> Match {
        let upper = upper_case(name);
        let mut found = self.get_surface(&upper);
        found.surface.def.sector_id = get_sector_id(&upper);
        found
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manager(track_ini: &str) -> SurfacesManager {
        let mut manager = SurfacesManager { surfaces: BTreeMap::new(), missing: Vec::new() };
        manager.surfaces.insert(
            "WALL".to_string(),
            SurfaceType { def: SurfaceDef { grip_mod: 1.0, collision_category: 2, is_valid_track: true, vibration_length: 1.5, ..blank() }, wav: String::new() },
        );
        manager.read_surface_definitions(&IniReader::from_text(Path::new("surfaces.ini"), track_ini)).unwrap();
        manager
    }

    const INI: &str = "[SURFACE_0]\nKEY=ROAD\nFRICTION=0.98\nIS_VALID_TRACK=1\nVIBRATION_LENGTH=0\n\
        [SURFACE_1]\nKEY=KERB\nFRICTION=0.94\nIS_VALID_TRACK=1\nVIBRATION_GAIN=0.5\nVIBRATION_LENGTH=1.5\nWAV=kerb.wav\nWAV_PITCH=1.3\n\
        [SURFACE_2]\nKEY=PITS\nFRICTION=0.97\nIS_PITLANE=1\nIS_VALID_TRACK=1\n\
        [SURFACE_4]\nKEY=NEVER\nFRICTION=0.5\n";

    #[test]
    fn the_leading_number_is_the_group_and_zero_means_not_physical() {
        assert_eq!(get_sector_id("01ROAD"), 1);
        assert_eq!(get_sector_id("12GRASS_07"), 12);
        assert_eq!(get_sector_id("0ROAD"), 0);
        assert_eq!(get_sector_id("ROAD1"), 0);
        assert_eq!(get_sector_id(" 4x"), 4);
        assert_eq!(get_sector_id("-3x"), -3);
        assert_eq!(get_sector_id("99999999999ROAD"), 0);
        assert_eq!(get_sector_id(""), 0);
    }

    #[test]
    fn one_key_in_the_name_gives_its_surface() {
        let m = manager(INI);
        // the numbering stopped at the gap: SURFACE_4 was never read
        assert_eq!(m.surfaces.len(), 4);
        let road = m.get_surface_desc_from_mesh_name("03road_main");
        assert_eq!(road.key.as_deref(), Some("ROAD"));
        assert_eq!((road.surface.def.grip_mod, road.surface.def.sector_id, road.surface.def.collision_category), (0.98, 3, 1));
        let kerb = m.get_surface_desc_from_mesh_name("1KERB_L3");
        assert_eq!((kerb.surface.def.vibration_gain, kerb.surface.def.vibration_length, kerb.surface.wav.as_str()), (0.5, 1.5, "kerb.wav"));
        assert_eq!(kerb.surface.def.wav_pitch_speed, 1.3);
        assert!(m.get_surface_desc_from_mesh_name("2PITS").surface.def.is_pitlane);
        let wall = m.get_surface_desc_from_mesh_name("5wall_inner");
        assert_eq!((wall.surface.def.collision_category, wall.surface.def.vibration_length), (2, 1.5));
    }

    #[test]
    fn no_key_or_two_keys_give_the_default_surface() {
        let m = manager(INI);
        let none = m.get_surface_desc_from_mesh_name("7SOMETHING");
        assert_eq!(none.key, None);
        assert_eq!((none.surface.def.grip_mod, none.surface.def.collision_category, none.surface.def.is_valid_track), (1.0, 0, true));
        assert_eq!(none.surface.def.sector_id, 7);
        assert!(none.error.unwrap().contains("SURFACE NOT FOUND"));
        let two = m.get_surface_desc_from_mesh_name("1ROADKERB");
        assert_eq!(two.key, None);
        assert!(two.error.unwrap().contains("CAN BE: KERB ROAD"));
        // a key with a lower-case letter can never match an upper-cased name
        let m = manager("[SURFACE_0]\nKEY=3dMISC\nFRICTION=0.9\n");
        assert_eq!(m.get_surface_desc_from_mesh_name("13dMISC").key, None);
    }
}
