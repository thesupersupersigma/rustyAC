// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! The other cars of race.ini, as `RaceManager::initOffline` makes them: one function for
//! the desktop's `conditions::apply` and the browser's `rustyac_web::race`, so both make the
//! same grid from the same file. The game's folder is read through `rustyac_content::vfs`
//! (the disk on the desktop, the page's files in the browser).

use std::path::Path;

use rustyac_physics::data::ini::IniReader;

use crate::input_file::SimSetup;

/// What the desktop's command line may change of the AI drivers (`--ai-level`,
/// `--ai-aggression`, `--stand-in-drivers`); the browser leaves all of them as race.ini has them.
#[derive(Clone, Copy, Debug, Default)]
pub struct DriverOptions {
    pub ai_level: Option<f32>,
    pub ai_aggression: Option<f32>,
    pub stand_in_drivers: bool,
}

/// The last folder name of a path or name (`spa`, `ks_ferrari_f2004`).
pub fn last_name(text: &str) -> String {
    Path::new(text).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

/// `content/cars/<model>/skins`'s folders in the order the file system lists them
/// (`session::model_skins`, through the page's files in the browser).
pub fn model_skins(cars_folder: &Path, model: &str) -> Vec<String> {
    rustyac_content::vfs::read_dir(&cars_folder.join(model).join("skins")).map(|list| list.into_iter().filter(|(_, dir)| *dir).map(|(name, _)| name).collect()).unwrap_or_default()
}

/// `RaceManager::isSkinAvailable` (`session::skin_available`): the text glued together, as
/// the game does.
pub fn skin_available(cars_folder: &Path, model: &str, skin: &str) -> bool {
    let mut text = cars_folder.as_os_str().to_os_string();
    text.push(format!("/{model}/skins/{skin}"));
    rustyac_content::vfs::is_dir(Path::new(&text))
}

/// The cars of race.ini as `RaceManager::initOffline` makes them
/// (`rustyac_physics::session::race_grid`): every car but the first becomes another car of the
/// session, with the model, the skin, the ballast, the restrictor, the pit box and the grid
/// slot the game gives it. The first is the player's: its car stays the drive's, its place is
/// the file's. Returns the lines for the console.
pub fn race_cars(ini: &IniReader, setup: &mut SimSetup, options: &DriverOptions, rand: &mut dyn FnMut() -> i32) -> Result<Vec<String>, String> {
    use crate::input_file::{GridPlace, Opponent, OpponentDrive};
    use rustyac_physics::session;
    let ac = rustyac_content::install::ac_root().ok_or("Assetto Corsa's folder was not found")?;
    let cars_folder = ac.join("content").join("cars");
    // TrackAvatar::getMaxSlotsAvailable("PIT"): the pit boxes of the track, counted once by
    // the track itself and once more for every session that starts in the pits
    let track_folder = crate::sim::find_track(&setup.track)?;
    let (track, _) = rustyac_physics::track::load_track(&track_folder, &setup.layout)?;
    let pit_slots = session::pit_slots_listed(ini, track.spawn_slots("PIT"));
    let grid = session::race_grid(ini, pit_slots, &|model| model_skins(&cars_folder, model), &|model, skin| skin_available(&cars_folder, model, skin), rand)?;
    let mut lines = Vec::new();
    let place = |car: &session::RaceCar| GridPlace { pit: car.index, start: car.start_index, ballast_kg: car.ballast_kg, restrictor: car.restrictor };
    let mut others = Vec::new();
    for car in &grid.cars {
        if car.index == 0 {
            setup.player_place = Some(place(car));
            // (the skin of the file, or the game's pick: only when the car is the file's car)
            if !car.skin.is_empty() && last_name(&setup.car) == car.model {
                setup.player_skin = Some(car.skin.clone());
            }
            if !car.model.is_empty() && last_name(&setup.car) != car.model {
                lines.push(format!("  (race.ini's first car is {}; yours is the one of --car, {})", car.model, last_name(&setup.car)));
            }
            continue;
        }
        if car.model.is_empty() {
            lines.push(format!("  car {}: race.ini names no car for it (the game would stop there): left out", car.index));
            continue;
        }
        // a car rustyAC cannot load or draw would end the session or the picture: left out
        match crate::sim::find_car_data(&car.model) {
            Ok(data) if crate::sim::find_car_model(&car.model, &data).is_some() => {}
            Ok(_) => {
                lines.push(format!("  car {}: {} has no 3D model in Assetto Corsa's folder: left out", car.index, car.model));
                continue;
            }
            Err(message) => {
                lines.push(format!("  car {}: {} cannot be loaded ({message}): left out", car.index, car.model));
                continue;
            }
        }
        lines.push(format!(
            "  car {}: {}, skin {}{}, pit box {}, grid slot {}{}{}{}",
            car.index,
            car.model,
            if car.skin.is_empty() { "-" } else { &car.skin },
            if car.skin_was_picked { " (picked as the game picks one: the file's is not a skin of the car)" } else { "" },
            car.index,
            car.start_index,
            if car.ballast_kg > 0.0 { format!(", ballast {} kg", car.ballast_kg) } else { String::new() },
            if car.restrictor > 0.0 { format!(", restrictor {}", car.restrictor) } else { String::new() },
            if car.driver_name.is_empty() { String::new() } else { format!(", {}", car.driver_name) }
        ));
        // RaceManager::initOffline's closure @ 0x140133e00: the driver's level from the
        // file's [RACE] AI_LEVEL and the car's own AI_LEVEL, its aggression from AI_AGGRESSION
        let drive = if options.stand_in_drivers {
            OpponentDrive::Auto
        } else {
            let race_level = options.ai_level.unwrap_or(grid.ai_level);
            let level = if car.ai_level != 0.0 { (car.ai_level * 0.01) * race_level } else { race_level } * 0.01;
            let push = if level > 1.0 {
                1.0
            } else if level >= 0.0 {
                level
            } else {
                0.0
            };
            let aggression = match options.ai_aggression {
                Some(percent) => (percent * 0.01).clamp(0.0, 1.0) * 0.95,
                None => car.ai_aggression,
            };
            OpponentDrive::Ai { push, aggression }
        };
        others.push(Opponent { car: car.model.clone(), skin: car.skin.clone(), drive, place: Some(place(car)) });
    }
    if others.is_empty() {
        // alone after all: the first slot as before, but a race's grid place stays
        if !(grid.first_session_type == 3 && setup.player_place.is_some_and(|place| place.start > 0)) {
            setup.player_place = None;
        }
        return Ok(lines);
    }
    lines.insert(
        0,
        format!(
            "{} other car(s) from race.ini ([RACE] CARS={}, {} pit boxes), each with {}. --no-race-cars leaves them out; --opponent names cars of your own",
            others.len(),
            ini.get_int("RACE", "CARS").unwrap_or(0),
            pit_slots,
            if options.stand_in_drivers {
                "rustyAC's stand-in line follower (--stand-in-drivers), which knows nothing of other cars".to_string()
            } else {
                format!("Assetto Corsa's AI driver ([RACE] AI_LEVEL={}; --ai-level and --ai-aggression change them, --stand-in-drivers gives the old line follower)", options.ai_level.unwrap_or(grid.ai_level))
            }
        ),
    );
    // the session's length as the AI drivers are told it
    let laps = ini.get_int("SESSION_0", "LAPS").unwrap_or(0);
    setup.ai.laps = if laps > 0 { laps } else { ini.get_int("RACE", "RACE_LAPS").unwrap_or(0) };
    setup.ai.time_secs = (ini.get_float("SESSION_0", "DURATION_MINUTES").unwrap_or(0.0) * 60.0) as f64;
    setup.opponents = others;
    Ok(lines)
}
