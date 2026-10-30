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
    rustyac_content::install::documents()
}

/// The game's last session file.
pub fn race_ini_path() -> Option<PathBuf> {
    Some(documents_folder()?.join("cfg").join("race.ini"))
}

/// `[RACE] TRACK` and `CONFIG_TRACK` of the session file a live drive reads (`--race-ini
/// <file>`, else the game's own unless `--no-race-ini`): the game's last track and layout.
pub fn race_track(options: &Options) -> Option<(String, String)> {
    let path = match (&options.race_ini_file, options.race_ini) {
        (Some(path), _) => path.clone(),
        (None, Some(false)) => return None,
        (None, _) => race_ini_path()?,
    };
    let ini = IniReader::load_race(&path).ok().filter(|ini| ini.ready)?;
    let race = RaceIni::from_ini(&ini);
    Some((race.track, race.track_config))
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

/// The player's skin when the other cars of race.ini are not made: the first car of
/// `RaceManager::initOffline`, the file's skin or the game's pick, on a copy of `rand()` so no
/// later draw moves. Only when race.ini's first car is the one of `--car`.
fn player_skin(ini: &IniReader, car: &str, mut rand: MsvcRand) -> Option<String> {
    use rustyac_physics::session;
    let cars_folder = rustyac_content::install::ac_root()?.join("content").join("cars");
    // (the first car is the same for any number of pit boxes)
    let grid = session::race_grid(ini, 1, &|model| session::model_skins(&cars_folder, model), &|model, skin| session::skin_available(&cars_folder, model, skin), &mut || rand.next()).ok()?;
    let first = grid.cars.first()?;
    (!first.skin.is_empty() && first.model == last_name(car)).then(|| first.skin.clone())
}

/// The cars of race.ini (`crate::race_cars::race_cars`, shared with the browser), with the
/// command line's `--ai-level`, `--ai-aggression` and `--stand-in-drivers`.
fn race_cars(ini: &IniReader, setup: &mut SimSetup, options: &Options, rand: &mut dyn FnMut() -> i32) -> Result<Vec<String>, String> {
    let drivers = crate::race_cars::DriverOptions { ai_level: options.ai_level, ai_aggression: options.ai_aggression, stand_in_drivers: options.stand_in_drivers };
    crate::race_cars::race_cars(ini, setup, &drivers, rand)
}

/// Fills the session of a live drive. Returns what was done, for the console.
///
/// `input_method`: `[HEADER] INPUT_METHOD` of the bindings in use (`X360`, `KEYBOARD`, `WHEEL`).
pub fn apply(options: &Options, setup: &mut SimSetup, input_method: &str) -> Result<Vec<String>, String> {
    let mut notes = Vec::new();
    // Task 27: CarAvatar::initControls @ 0x1400d6f30: INPUT_METHOD=AI gives the player's car
    // an AI driver
    if options.ai_drive || input_method == "AI" {
        setup.ai.player = true;
        notes.push(format!("an AI driver drives your car ({})", if options.ai_drive { "--ai-drive" } else { "controls.ini INPUT_METHOD=AI" }));
    }
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
    let mut wind_base: Option<(f32, f32)> = None;
    // assists.ini AUTO_CLUTCH, AUTO_BLIP, AUTO_SHIFTER
    let mut shift_aids: Option<(bool, bool, bool)> = None;
    let mut setup_name = options.setup.clone();
    let mut setup_from_race_ini = false;
    match &race_path {
        Some(path) => {
            let documents = IniReader::load(path)?;
            let official = documents.official_special_event();
            let mut game_mode = rustyac_physics::race::game_mode::GameMode::new(official.as_ref().unwrap_or(&documents), &documents, official.is_some());
            // TimeAttack's constructor: INIReaderDocuments(cfg/gameplay.ini)
            if let Some(gameplay) = documents_folder().and_then(|p| IniReader::load(&p.join("cfg/gameplay.ini")).ok()).filter(|ini| ini.ready && ini.has_section("TIME_ATTACK")) {
                game_mode.time_attack_mult = gameplay.get_float("TIME_ATTACK", "BASE_TIME_MULT")?;
            }
            let ini = official.unwrap_or(documents);
            if !ini.ready {
                return Err(format!("{}: cannot be read", path.display()));
            }
            let race = RaceIni::from_ini(&ini);
            race.apply_to_env(&mut setup.env);
            // where the session starts, unless the command line says where
            if !options.spawn_given {
                if let Some(spawn) = match race.spawn_set.as_str() {
                    "PIT" => Some("pit"),
                    "START" => Some("start"),
                    "HOTLAP_START" => Some("hotlap"),
                    _ => None,
                } {
                    setup.spawn = spawn.to_string();
                }
            }
            let session = &mut setup.session;
            session.session_type = race.session_type;
            session.arm_first_lap = race.session_type.map(|_| race.arm_first_lap);
            session.penalties = race.penalties;
            session.ballast_kg = race.ballast_kg;
            session.restrictor = race.restrictor;
            // the game seeds its `rand()` from the clock: every session draws anew
            // (--race-seed fixes it)
            let seed = options.race_seed.unwrap_or_else(|| std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u32).unwrap_or(1));
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
                wind_base = Some((settings.base_speed, settings.base_direction));
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
            // RaceManager::initOffline: the time the gearbox stays locked before a race start
            // (drawn, not used yet), then the cars of the file
            rand.next();
            // (also for a race of one car whose file gives it a grid place of its own)
            let alone_on_a_grid = ini.get_int("SESSION_0", "TYPE").unwrap_or(0) == 3 && ini.get_int("SESSION_0", "STARTING_POSITION").unwrap_or(0) > 1;
            let wanted = !options.no_race_cars && options.cars.is_none() && options.opponents.is_empty() && !setup.track.is_empty() && (ini.get_int("RACE", "CARS").unwrap_or(0) > 1 || alone_on_a_grid);
            let mut car_notes = Vec::new();
            if wanted {
                match race_cars(&ini, setup, options, &mut || rand.next()) {
                    Ok(lines) => car_notes = lines,
                    Err(message) => car_notes.push(format!("WARNING: the other cars of race.ini are left out: {message}")),
                }
            } else if !setup.track.is_empty() {
                setup.player_skin = player_skin(&ini, &setup.car, rand);
            }
            if !race.penalties {
                line.push_str("; penalties off (any number of tyres may leave the track)");
            }
            // Task 28: the session list and the rules, for the referee
            if !options.no_referee && options.sessions.is_none() && !setup.track.is_empty() && ini.has_section("SESSION_0") {
                let sessions = rustyac_physics::race::SessionsIni::from_ini(&ini, setup.env.base_pit_penalty_laps);
                if let Some(mode) = sessions.jump_start_penalty_mode {
                    setup.env.jump_start_penalty_mode = mode;
                }
                let cars = 1 + setup.opponents.len();
                let driver_names = (0..cars).map(|n| ini.get_string(&format!("CAR_{n}"), "DRIVER_NAME")).collect();
                car_notes.push(format!(
                    "referee: {} (race.ini's sessions; --no-referee: none)",
                    sessions.sessions.iter().map(|s| if s.session_type == 3 && !s.is_timed_race { format!("{} {} laps", s.name, s.laps) } else { format!("{} {} min", s.name, s.duration_minutes) }).collect::<Vec<_>>().join(", ")
                ));
                // Task 28b: the mandatory pit window. The game has it online only (the server's
                // RACE_PIT_WINDOW_START / RACE_PIT_WINDOW_END); the same two keys in race.ini's
                // [RACE] turn it on here. Without them: no window, nothing is checked.
                let window = (ini.get_int("RACE", "RACE_PIT_WINDOW_START").unwrap_or(0).clamp(0, 65535) as u16, ini.get_int("RACE", "RACE_PIT_WINDOW_END").unwrap_or(0).clamp(0, 65535) as u16);
                if window.1 != 0 {
                    car_notes.push(format!("mandatory pit window: opens at {}, closes at {} (the leader's laps; minutes in a timed race)", window.0, window.1));
                }
                let pit = (!options.no_pit_stop).then(|| pit_header(window, setup.session.setup_file.as_deref(), keyboard_driver(options, input_method)));
                setup.referee = Some(crate::input_file::RefereeHeader { sessions, over_time_mult: over_time_mult(), first_start_delay_ms: 20000.0, next_start_delay_ms: 10000.0, seed, wind_base, driver_names, pit, game_mode });
            }
            notes.push(line);
            notes.append(&mut car_notes);
            let last = format!("{} at {}{}", race.model, race.track, if race.track_config.is_empty() { String::new() } else { format!(" ({})", race.track_config) });
            notes.push(format!("  (that session was {last}, weather {:?}: the weather changes the looks only; car and track are still chosen with --car and --track)", race.weather));
            // the driving aids of the same folder
            let assists_path = path.with_file_name("assists.ini");
            if assists_path.is_file() {
                let assists_ini = IniReader::load(&assists_path)?;
                let assists = Assists::from_ini(&assists_ini);
                assists.apply_to_env(&mut setup.env);
                setup.session.assists = Some((assists.abs, assists.traction_control, assists.stability_control));
                shift_aids = Some((assists.auto_clutch, assists.auto_blip, assists.auto_shifter));
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
    // the shift aids as `DrivingAssistManager` sets them (0x1400fbfb8 .. 0x1400fc6aa): the
    // gearbox and the blip from the file; the clutch on when the file says so or the device is
    // a pad or the keyboard (their classes force it). The command line on top.
    let device_forces_clutch = matches!(input_method, "X360" | "KEYBOARD");
    if let Some((clutch, blip, shifter)) = shift_aids {
        setup.auto_clutch = clutch || device_forces_clutch;
        setup.auto_blip = Some(blip);
        setup.auto_shifter = shifter;
    }
    if options.auto_clutch {
        setup.auto_clutch = true;
    }
    if options.no_auto_clutch {
        setup.auto_clutch = false;
    }
    if options.auto_shifter || options.autodrive || options.drift_drive {
        // (the line follower does not shift)
        setup.auto_shifter = true;
    }
    if options.no_auto_shifter && !options.autodrive && !options.drift_drive {
        setup.auto_shifter = false;
    }
    if options.auto_blip.is_some() {
        setup.auto_blip = options.auto_blip;
    }
    notes.push(format!(
        "shift aids: automatic clutch {}, automatic gearbox {}, automatic blip {}{}",
        if setup.auto_clutch { "on" } else { "off" },
        if setup.auto_shifter { "on" } else { "off" },
        match setup.auto_blip {
            Some(true) | None => "on",
            Some(false) => "off (a car with an electronic blip blips anyway)",
        },
        if shift_aids.is_some() { " (assists.ini; a pad or the keyboard forces the clutch on, as in the game)" } else { "" }
    ));
    if let Some(session_type) = setup.session.session_type {
        let name = ["?", "practice", "qualifying", "race", "hot-lap", "time attack", "drift", "drag"].get(session_type as usize).copied().unwrap_or("?");
        notes.push(format!(
            "session: {name} ([SESSION_0] TYPE={session_type}), from {}{}",
            setup.spawn,
            if setup.session.arm_first_lap.unwrap_or(session_type == 4) {
                "; the clock starts again when the car first crosses the line (lap 1 is line to line)"
            } else {
                "; the clock runs from the spawn point, so lap 1 includes the run-up to the line (as in the game)"
            }
        ));
    }
    if let Some(percent) = options.damage {
        // DrivingAssistManager::DrivingAssistManager 0x1400fbd90: (float)DAMAGE * 0.01
        setup.env.mechanical_damage_rate = percent as f32 * 0.01;
        overrides.push(format!("mechanical damage {percent} % (--damage)"));
    } else if setup.env.mechanical_damage_rate <= 0.0 {
        notes.push("  mechanical damage is OFF (assists.ini DAMAGE=0), as in your last Assetto Corsa session: a crash bends no suspension and no tyre can burst. --damage 100 turns it on".to_string());
    }
    if let Some(density) = options.air_density {
        setup.env.air_density_override = Some(density);
        overrides.push(format!(
            "air density {density} kg/m3 (NOT the game's: acs.exe alone gives {:.4} at {} C)",
            rustyac_physics::car::engine::get_air_density(setup.env.ambient_temperature),
            setup.env.ambient_temperature
        ));
    }
    if let Some((at_26, per_degree)) = options.pressure_law {
        setup.env.experiment_tyre_pressure_law = Some((at_26, per_degree));
        overrides.push(format!("tyre pressure {at_26} psi + {per_degree} per deg C of core temperature above 26 (NOT the game's: acs.exe alone gives the static pressure + 0.16 per deg C)"));
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

/// `system/cfg/assetto_corsa.ini [RACE] OVER_TIME_MULT` of the installed game (only read):
/// 1.5 without the file or its section, as `RaceManager::RaceManager` has it.
pub fn over_time_mult() -> f32 {
    let Some(root) = rustyac_content::install::ac_root() else {
        return 1.5;
    };
    let path = root.join("system").join("cfg").join("assetto_corsa.ini");
    match IniReader::load(&path) {
        Ok(ini) if path.is_file() && ini.has_section("RACE") => ini.get_float("RACE", "OVER_TIME_MULT").unwrap_or(0.0),
        _ => 1.5,
    }
}

/// Task 28b: the player's pit stop of a session: `system/cfg/pitstop.ini` of the installed
/// game (only read; the constructor's values without it) and the pit stop presets beside
/// A driver of the keyboard, for the pit menu (`Sim::Sim` 0x140192070: `IsKeyboardControl`).
// shortcut: rustyAC's own drivers (--autodrive, --ai-drive) keep the quick menu unless --pit-click scripts the clicks, as they click no CONFIRM
pub fn keyboard_driver(options: &crate::cli::Options, input_method: &str) -> bool {
    input_method == "KEYBOARD" && (!options.pit_clicks.is_empty() || !(options.autodrive || options.ai_drive))
}

/// the saved setup (`<setup>.sp`, as `SetupScreen::loadSetupAbsolutePath` looks for them).
/// `keyboard`: the controls are the keyboard's (`Sim::Sim` 0x140192070 gives those drivers
/// the old mouse menu, as pitstop.ini's [SETTINGS] USE_MOUSE_PITSTOP does everyone).
pub fn pit_header(window: (u16, u16), setup_file: Option<&std::path::Path>, keyboard: bool) -> crate::input_file::PitHeader {
    let ini = rustyac_content::install::ac_root().map(|root| root.join("system").join("cfg").join("pitstop.ini")).filter(|path| path.is_file()).and_then(|path| IniReader::load(&path).ok());
    let settings = ini.as_ref().map(rustyac_physics::race::QuickMenuSettings::from_ini).unwrap_or_default();
    let mouse = keyboard || ini.is_some_and(|ini| ini.get_int("SETTINGS", "USE_MOUSE_PITSTOP").unwrap_or(0) != 0);
    let presets_ini = setup_file.map(|file| file.with_extension("sp")).filter(|file| file.is_file()).and_then(|file| std::fs::read_to_string(file).ok());
    crate::input_file::PitHeader { window, settings, values: None, presets_ini, mouse }
}

/// `--sessions p10,q5,r3,rt20+`: a session list as race.ini would give it.
pub fn sessions_from_list(list: &str, cars: usize, penalties: bool, jump_start: Option<i32>, base_pit_penalty_laps: i16) -> Result<rustyac_physics::race::SessionsIni, String> {
    rustyac_physics::race::SessionsIni::from_list(list, cars, penalties, jump_start, base_pit_penalty_laps).map_err(|e| format!("--sessions: {e}"))
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
