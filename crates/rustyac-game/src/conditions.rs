// SPDX-License-Identifier: GPL-3.0-or-later

//! The session of a live drive: Assetto Corsa's own last session
//! (`Documents\Assetto Corsa\cfg\race.ini` and `assists.ini`, read by
//! `rustyac_physics::session` the way `acs.exe` reads them), a saved setup, and what the
//! command line says on top (`--air`, `--road`, `--grip`, `--wind`, `--setup`).
//!
//! The files are only read. What they give is written into the drive's [`SimSetup`], so a
//! recorded drive replays in the same conditions wherever it is played.

use std::path::{Path, PathBuf};

use rustyac_physics::data::ini::IniReader;
use rustyac_physics::session::{generate_wind, wind_from_kmh, Assists, MsvcRand, RaceIni};

use crate::cli::Options;
use crate::input_file::SimSetup;

/// `Documents\Assetto Corsa` of the current user.
pub fn documents_folder() -> Option<PathBuf> {
    Some(PathBuf::from(std::env::var_os("USERPROFILE")?).join("Documents").join("Assetto Corsa"))
}

/// The game's last session file.
pub fn race_ini_path() -> Option<PathBuf> {
    Some(documents_folder()?.join("cfg").join("race.ini"))
}

/// Finds a saved setup: a file, or a name under `Documents\Assetto Corsa\setups\<car>\`, in
/// the track's folder first, then in `generic` (the two folders the game's setup screen
/// lists). `.ini` may be left off.
pub fn find_setup(name: &str, car: &str, track: &str) -> Result<PathBuf, String> {
    let with_ini = |path: PathBuf| -> Vec<PathBuf> {
        let mut named = path.clone().into_os_string();
        named.push(".ini");
        vec![path, PathBuf::from(named)]
    };
    let mut candidates = with_ini(PathBuf::from(name));
    if let Some(documents) = documents_folder() {
        let setups = documents.join("setups").join(car);
        if !track.is_empty() {
            candidates.extend(with_ini(setups.join(track).join(name)));
        }
        candidates.extend(with_ini(setups.join("generic").join(name)));
    }
    match candidates.iter().find(|path| path.is_file()) {
        Some(path) => Ok(std::path::absolute(path).unwrap_or_else(|_| path.clone())),
        None => Err(format!(
            "the setup {name:?} was not found; looked for {}",
            candidates.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join(", ")
        )),
    }
}

/// The wind of the game's own last session: the last line "Setting wind <km/h> kmh direction:
/// <deg> deg" (`PhysicsEngine::setWind` prints it) of `Documents\Assetto Corsa\logs\log.txt`,
/// as (km/h, degrees). The log has six decimals, so the wind is the game's to about a
/// millionth of a km/h. A session without wind writes no such line: then there is none.
pub fn wind_of_last_session() -> Result<(f32, f32), String> {
    let path = documents_folder().ok_or("--wind-from-log: the Documents folder is not known")?.join("logs").join("log.txt");
    let bytes = std::fs::read(&path).map_err(|e| format!("--wind-from-log: {}: {e}", path.display()))?;
    Ok(wind_in_log(&String::from_utf8_lossy(&bytes)).unwrap_or((0.0, 0.0)))
}

/// The last "Setting wind" line of a log's text.
pub fn wind_in_log(log: &str) -> Option<(f32, f32)> {
    log.lines().rev().find_map(|line| {
        let rest = line.trim().strip_prefix("Setting wind ")?;
        let (kmh, rest) = rest.split_once(" kmh direction: ")?;
        let direction = rest.trim().strip_suffix(" deg")?;
        Some((kmh.trim().parse().ok()?, direction.trim().parse().ok()?))
    })
}

/// The last folder name of a path or name (`spa`, `ks_ferrari_f2004`).
fn last_name(text: &str) -> String {
    Path::new(text).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

/// Fills the session of a live drive. Returns what was done, for the console.
pub fn apply(options: &Options, setup: &mut SimSetup) -> Result<Vec<String>, String> {
    let mut notes = Vec::new();
    let race_path = match (&options.race_ini_file, options.race_ini) {
        (Some(path), _) => Some(path.clone()),
        (None, Some(false)) => None,
        (None, wanted) => match race_ini_path().filter(|path| path.is_file()) {
            Some(path) => Some(path),
            None if wanted == Some(true) => return Err("--race-ini: Documents\\Assetto Corsa\\cfg\\race.ini is not there".to_string()),
            None => None,
        },
    };
    let mut wind_base_direction = 0.0;
    let mut setup_name = options.setup.clone();
    let mut setup_from_race_ini = false;
    match &race_path {
        Some(path) => {
            let ini = IniReader::load(path)?;
            if !ini.ready {
                return Err(format!("{}: cannot be read", path.display()));
            }
            let race = RaceIni::from_ini(&ini);
            race.apply_to_env(&mut setup.env);
            let session = &mut setup.session;
            session.penalties = race.penalties;
            session.ballast_kg = race.ballast_kg;
            session.restrictor = race.restrictor;
            // the game seeds its `rand()` from the clock: every session draws anew
            let seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u32).unwrap_or(1);
            let mut rand = MsvcRand(seed);
            let mut line = format!("conditions from {}: air {} C, road {} C", path.display(), setup.env.ambient_temperature, setup.env.road_temperature);
            if let Some(ini_track) = race.dynamic_track {
                // Track::initDynamicTrack, then the new session's draw (session 0)
                let mut track = ini_track.build(rand.next());
                track.on_new_session(0, rand.next());
                track.step(0);
                line.push_str(&format!(
                    "; track grip {:.1} % at the start, {:+.2} % per lap (SESSION_START {}, RANDOMNESS {}, LAP_GAIN {})",
                    track.dynamic_grip_level * 100.0,
                    track.grip_per_lap * 100.0,
                    ini_track.session_start,
                    ini_track.randomness,
                    ini_track.lap_gain
                ));
                setup.env.dynamic_grip_level = track.dynamic_grip_level;
                session.dynamic_track = Some(track);
            } else {
                line.push_str("; track grip 100 % (no [DYNAMIC_TRACK])");
            }
            if let Some(ini_wind) = race.wind {
                let settings = ini_wind.settings(&mut || rand.next());
                wind_base_direction = settings.base_direction;
                match generate_wind(&settings, &mut || rand.next()) {
                    Some((speed, direction)) => {
                        session.wind_speed = speed;
                        session.wind_direction_deg = direction;
                        line.push_str(&format!(
                            "; wind {:.1} km/h from {:.1} deg (drawn as the game draws it: 80 to 120 % of {:.1} km/h, within 20 deg of {:.0}; --wind <km/h> --wind-dir <deg> fixes it)",
                            speed * 3.6,
                            direction,
                            settings.base_speed,
                            settings.base_direction
                        ));
                    }
                    None => line.push_str("; no wind"),
                }
            } else {
                line.push_str("; no wind (no [WIND])");
            }
            if race.ballast_kg > 0.0 || race.restrictor > 0.0 {
                line.push_str(&format!("; ballast {} kg, restrictor {}", race.ballast_kg, race.restrictor));
            }
            if !race.penalties {
                line.push_str("; penalties off (any number of tyres may leave the track)");
            }
            notes.push(line);
            let last = format!("{} at {}{}", race.model, race.track, if race.track_config.is_empty() { String::new() } else { format!(" ({})", race.track_config) });
            notes.push(format!("  (that session was {last}, weather {:?}: the weather changes the looks only; car and track are still chosen with --car and --track)", race.weather));
            // the driving aids of the same folder
            let assists_path = path.with_file_name("assists.ini");
            if assists_path.is_file() {
                let assists_ini = IniReader::load(&assists_path)?;
                let assists = Assists::from_ini(&assists_ini);
                assists.apply_to_env(&mut setup.env);
                session.assists = Some((assists.abs, assists.traction_control, assists.stability_control));
                let level = |v: i32| ["off", "as the car has it", "on"].get(v as usize).copied().unwrap_or("?");
                notes.push(format!(
                    "aids from {}: ABS {}, traction control {}, stability {} %, mechanical damage {:.0} %, fuel x{}, tyre wear x{}, tyre blankets {}",
                    assists_path.display(),
                    level(assists.abs),
                    level(assists.traction_control),
                    assists.stability_control,
                    assists.mechanical_damage_rate * 100.0,
                    assists.fuel_consumption_rate,
                    assists.tyre_consumption_rate,
                    if assists.allow_tyre_blankets { "on" } else { "off" }
                ));
            } else {
                notes.push(format!("  ({} is not there: the aids are as the car has them, damage, fuel use and tyre wear at 100 %)", assists_path.display()));
            }
            // not a key of acs.exe: launchers write the setup's name here
            if setup_name.is_none() && !race.setup.is_empty() {
                setup_name = Some(race.setup.clone());
                setup_from_race_ini = true;
            }
        }
        None => notes.push("conditions: the built-in ones (26 C air, 30 C road, grip 100 %, no wind); no race.ini is read".to_string()),
    }
    // the command line on top
    let mut overrides = Vec::new();
    if let Some(air) = options.air {
        setup.env.ambient_temperature = air;
        overrides.push(format!("air {air} C"));
    }
    if let Some(road) = options.road {
        setup.env.road_temperature = road;
        overrides.push(format!("road {road} C"));
    }
    if let Some(grip) = options.grip {
        // a fixed level, as a server sets it (Track::setGripLevelExternal): no gain per lap
        let level = if grip > 1.5 { grip / 100.0 } else { grip };
        setup.session.dynamic_track = None;
        setup.env.dynamic_grip_level = level;
        overrides.push(format!("grip {:.1} % (fixed)", level * 100.0));
    }
    if options.wind_from_log {
        let (kmh, direction) = wind_of_last_session()?;
        setup.session.wind_speed = if kmh > 0.0 { wind_from_kmh(kmh) } else { 0.0 };
        setup.session.wind_direction_deg = direction;
        overrides.push(format!("wind {kmh} km/h from {direction} deg (what Assetto Corsa drew in its last session, from its log)"));
    } else if let Some(kmh) = options.wind {
        if kmh > 0.0 {
            setup.session.wind_speed = wind_from_kmh(kmh);
            setup.session.wind_direction_deg = options.wind_dir.unwrap_or(wind_base_direction);
            overrides.push(format!("wind {kmh} km/h from {} deg", setup.session.wind_direction_deg));
        } else {
            setup.session.wind_speed = 0.0;
            setup.session.wind_direction_deg = 0.0;
            overrides.push("no wind".to_string());
        }
    } else if let Some(direction) = options.wind_dir {
        setup.session.wind_direction_deg = direction;
        overrides.push(format!("wind from {direction} deg"));
    }
    if let Some(density) = options.air_density {
        setup.env.air_density_override = Some(density);
        overrides.push(format!(
            "air density {density} kg/m3 (NOT the game's: acs.exe alone gives {:.4} at {} C)",
            rustyac_physics::car::engine::get_air_density(setup.env.ambient_temperature),
            setup.env.ambient_temperature
        ));
    }
    if !overrides.is_empty() {
        notes.push(format!("from the command line: {}", overrides.join(", ")));
    }
    if let Some(name) = setup_name {
        let car = crate::sim::find_car_data(&setup.car).map(|data| crate::sim::car_name(&data)).unwrap_or_else(|_| last_name(&setup.car));
        match find_setup(&name, &car, &last_name(&setup.track)) {
            Ok(file) => {
                notes.push(format!("setup: {}", file.display()));
                setup.session.setup_file = Some(file);
            }
            // nobody asked for it on the command line (it may belong to another car or track)
            Err(message) if setup_from_race_ini => notes.push(format!("race.ini names a setup that is not used: {message}; the default setup it is")),
            Err(message) => return Err(message),
        }
    }
    Ok(notes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_wind_line_of_the_game_s_log() {
        let log = "Loading engine\nGenerating wind, base 10.000000 0.000000 -> 9.1 3.0\nSetting wind 9.100000 kmh direction: 3.000000 deg\nwind vector: (0 , 0 , 1)\n\
                   later\nGenerating wind, base 10.000000 0.000000 -> 8.005005 2.543413\r\nSetting wind 8.005005 kmh direction: 2.543413 deg\r\nwind vector: (-0.1 , 0.0 , -2.2)\n";
        assert_eq!(wind_in_log(log), Some((8.005005, 2.543413)));
        assert_eq!(wind_in_log("no wind today\n"), None);
        assert_eq!(wind_in_log("Setting wind fast kmh direction: north deg\n"), None);
    }

    #[test]
    fn a_setup_is_found_by_file_or_refused_with_the_places_looked_in() {
        let message = find_setup("no such setup of task 15", "no_such_car", "spa").unwrap_err();
        assert!(message.contains("no such setup of task 15") && message.contains("generic"), "{message}");
        // a file is taken as it is
        let file = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
        assert_eq!(find_setup(file.to_str().unwrap(), "any", "").unwrap().file_name(), file.file_name());
    }
}
