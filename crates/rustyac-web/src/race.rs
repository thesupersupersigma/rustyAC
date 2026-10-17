// SPDX-License-Identifier: GPL-3.0-or-later

//! A drive from AC's launcher in the page: the session of the launcher's `race.ini` (and
//! `assists.ini`), as the desktop's `rustyac_game::conditions::apply` makes it from the same
//! files, in the same order and with the same draws of the game's `rand()`. The desktop's
//! function reads the disk and the command line; this one reads the two files it is given and
//! the game's folder through the page's files (`system/cfg/assetto_corsa.ini`,
//! `system/cfg/pitstop.ini`).
//!
//! Only the player's car: the other cars of race.ini are not made in the browser yet (the
//! page refuses such a session before it starts, see `opponents`).

use std::path::Path;

use rustyac_game::input_file::{PitHeader, RefereeHeader, SimSetup};
use rustyac_physics::data::ini::IniReader;
use rustyac_physics::race::{QuickMenuSettings, SessionsIni};
use rustyac_physics::session::{generate_wind, Assists, MsvcRand, RaceIni};

/// The other cars race.ini asks for (`[RACE] CARS` - 1), as the desktop counts them.
pub fn opponents(race: &IniReader) -> i32 {
    (race.get_int("RACE", "CARS").unwrap_or(0) - 1).max(0)
}

/// `[RACE] MODEL`, `TRACK`, `CONFIG_TRACK`: the drive's car and track.
pub fn car_and_track(race: &IniReader) -> (String, String, String) {
    (race.get_string("RACE", "MODEL"), race.get_string("RACE", "TRACK"), race.get_string("RACE", "CONFIG_TRACK"))
}

/// `system/cfg/assetto_corsa.ini [RACE] OVER_TIME_MULT` (conditions::over_time_mult).
fn over_time_mult(root: &Path) -> f32 {
    match IniReader::load(&root.join("system/cfg/assetto_corsa.ini")) {
        Ok(ini) if ini.ready && ini.has_section("RACE") => ini.get_float("RACE", "OVER_TIME_MULT").unwrap_or(0.0),
        _ => 1.5,
    }
}

/// The pit stop of the session (conditions::pit_header, without a saved setup's presets).
fn pit_header(root: &Path, window: (u16, u16)) -> PitHeader {
    let settings = match IniReader::load(&root.join("system/cfg/pitstop.ini")) {
        Ok(ini) if ini.ready => QuickMenuSettings::from_ini(&ini),
        _ => QuickMenuSettings::default(),
    };
    PitHeader { window, settings, values: None, presets_ini: None }
}

/// The session of `race` (and `assists`) on top of `setup` (the page's car and track, as
/// `content::setup` makes them). `seed`: the game's `rand()` seed (the desktop takes the
/// clock's milliseconds). `root`: the game's folder in the page's files. Returns notes.
pub fn apply(setup: &mut SimSetup, race_ini: &IniReader, assists_ini: Option<&IniReader>, seed: u32, root: &Path) -> Vec<String> {
    let mut notes = Vec::new();
    let ini = race_ini;
    let race = RaceIni::from_ini(ini);
    race.apply_to_env(&mut setup.env);
    if let Some(spawn) = match race.spawn_set.as_str() {
        "PIT" => Some("pit"),
        "START" => Some("start"),
        "HOTLAP_START" => Some("hotlap"),
        _ => None,
    } {
        setup.spawn = spawn.to_string();
    }
    let session = &mut setup.session;
    session.session_type = race.session_type;
    session.arm_first_lap = race.session_type.map(|_| race.arm_first_lap);
    session.penalties = race.penalties;
    session.ballast_kg = race.ballast_kg;
    session.restrictor = race.restrictor;
    let mut rand = MsvcRand(seed);
    let mut wind_base: Option<(f32, f32)> = None;
    if let Some(ini_track) = race.dynamic_track {
        // Track::initDynamicTrack, then the new session's draw (session 0)
        let mut track = ini_track.build(rand.next());
        track.on_new_session(0, rand.next());
        track.step(0);
        setup.env.dynamic_grip_level = track.dynamic_grip_level;
        session.dynamic_track = Some(track);
    }
    if let Some(ini_wind) = race.wind {
        let settings = ini_wind.settings(&mut || rand.next());
        wind_base = Some((settings.base_speed, settings.base_direction));
        if let Some((speed, direction)) = generate_wind(&settings, &mut || rand.next()) {
            session.wind_speed = speed;
            session.wind_direction_deg = direction;
        }
    }
    // RaceManager::initOffline: the time the gearbox stays locked before a race start
    rand.next();
    // the player's skin as the desktop's conditions::player_skin finds it: the first car of
    // RaceManager::initOffline's grid (the launcher writes `[CAR_0] MODEL=-`, the player's
    // car), on a copy of rand() so no later draw moves; the skins folder through the page's files
    if opponents(ini) == 0 {
        let cars = root.join("content").join("cars");
        let skins = |model: &str| -> Vec<String> {
            rustyac_content::vfs::read_dir(&cars.join(model).join("skins")).map(|l| l.into_iter().filter(|(_, dir)| *dir).map(|(n, _)| n).collect()).unwrap_or_default()
        };
        let exists = |model: &str, skin: &str| rustyac_content::vfs::is_dir(&cars.join(model).join("skins").join(skin));
        let mut copy = MsvcRand(rand.0);
        if let Ok(grid) = rustyac_physics::session::race_grid(ini, 1, &skins, &exists, &mut || copy.next()) {
            if let Some(first) = grid.cars.first().filter(|c| !c.skin.is_empty() && c.model == setup.car) {
                setup.player_skin = Some(first.skin.clone());
            }
        }
    }
    let alone_on_a_grid = ini.get_int("SESSION_0", "TYPE").unwrap_or(0) == 3 && ini.get_int("SESSION_0", "STARTING_POSITION").unwrap_or(0) > 1;
    if alone_on_a_grid {
        notes.push("race.ini's grid place of the player is not used in the browser yet: the car starts first".to_string());
    }
    if ini.has_section("SESSION_0") {
        let sessions = SessionsIni::from_ini(ini, setup.env.base_pit_penalty_laps);
        if let Some(mode) = sessions.jump_start_penalty_mode {
            setup.env.jump_start_penalty_mode = mode;
        }
        let driver_names = vec![ini.get_string("CAR_0", "DRIVER_NAME")];
        let window = (ini.get_int("RACE", "RACE_PIT_WINDOW_START").unwrap_or(0).clamp(0, 65535) as u16, ini.get_int("RACE", "RACE_PIT_WINDOW_END").unwrap_or(0).clamp(0, 65535) as u16);
        let pit = Some(pit_header(root, window));
        setup.referee = Some(RefereeHeader { sessions, over_time_mult: over_time_mult(root), first_start_delay_ms: 20000.0, next_start_delay_ms: 10000.0, seed, wind_base, driver_names, pit });
    }
    notes.push(format!(
        "race.ini: {} at {}{}, air {} C, road {} C, grip {:.1} %, wind {:.1} km/h",
        race.model,
        race.track,
        if race.track_config.is_empty() { String::new() } else { format!(" ({})", race.track_config) },
        setup.env.ambient_temperature,
        setup.env.road_temperature,
        setup.env.dynamic_grip_level * 100.0,
        setup.session.wind_speed * 3.6
    ));
    if let Some(assists_ini) = assists_ini.filter(|a| a.ready) {
        let assists = Assists::from_ini(assists_ini);
        assists.apply_to_env(&mut setup.env);
        setup.session.assists = Some((assists.abs, assists.traction_control, assists.stability_control));
        // DrivingAssistManager: the page drives with a pad or the keyboard, whose classes
        // force the automatic clutch on
        setup.auto_clutch = true;
        setup.auto_blip = Some(assists.auto_blip);
        setup.auto_shifter = assists.auto_shifter;
        notes.push(format!("assists.ini: ABS {}, traction control {}, stability {} %, automatic gearbox {}", assists.abs, assists.traction_control, assists.stability_control, assists.auto_shifter));
    }
    notes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_practice_of_the_launcher_becomes_the_session() {
        let race = IniReader::from_text(
            Path::new("race.ini"),
            "[RACE]\nMODEL=bmw_z4_gt3\nTRACK=ks_laguna_seca\nCONFIG_TRACK=\nCARS=1\n[CAR_0]\nMODEL=bmw_z4_gt3\nSKIN=01_white\nDRIVER_NAME=You\n\
             [SESSION_0]\nNAME=Practice\nTYPE=1\nDURATION_MINUTES=10\nSPAWN_SET=PIT\n[TEMPERATURE]\nAMBIENT=18\nROAD=23\n",
        );
        let assists = IniReader::from_text(Path::new("assists.ini"), "[ASSISTS]\nABS=1\nTRACTION_CONTROL=1\nSTABILITY_CONTROL=0\nAUTO_SHIFTER=1\nAUTO_BLIP=1\nAUTO_CLUTCH=0\n");
        let (car, track, layout) = car_and_track(&race);
        let mut setup = crate::content::setup(&car, &track, &layout, false, "hotlap");
        apply(&mut setup, &race, Some(&assists), 1, Path::new("no such folder"));
        assert_eq!((setup.car.as_str(), setup.track.as_str(), setup.spawn.as_str()), ("bmw_z4_gt3", "ks_laguna_seca", "pit"));
        assert_eq!(setup.player_skin.as_deref(), Some("01_white"));
        assert_eq!(setup.session.session_type, Some(1));
        assert_eq!((setup.env.ambient_temperature, setup.env.road_temperature), (18.0, 23.0));
        assert!(setup.auto_shifter && setup.auto_clutch);
        let referee = setup.referee.expect("the session list");
        assert_eq!(referee.over_time_mult, 1.5);
        assert_eq!(referee.sessions.sessions.len(), 1);
        assert_eq!(opponents(&race), 0);
    }
}
