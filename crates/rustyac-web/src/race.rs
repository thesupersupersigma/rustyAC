// SPDX-License-Identifier: GPL-3.0-or-later

//! A drive from AC's launcher in the page: the session of the launcher's `race.ini` (and
//! `assists.ini`), as the desktop's `rustyac_game::conditions::apply` makes it from the same
//! files, in the same order and with the same draws of the game's `rand()`. The desktop's
//! function reads the disk and the command line; this one reads the two files it is given and
//! the game's folder through the page's files (`system/cfg/assetto_corsa.ini`,
//! `system/cfg/pitstop.ini`).
//!
//! The other cars of race.ini get Assetto Corsa's AI drivers, as on the desktop. `seed`
//! stands for the desktop's `--race-seed S --ai-seed <S - 28000>` (wrapping; the desktop's
//! referee seed is the AI seed + 28000): the same seeds, the same draws.

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
    // the cars of the file as the desktop's conditions::apply makes them (the same function,
    // `rustyac_game::race_cars`, through the page's files), else the player's skin as its
    // conditions::player_skin finds it: the first car of RaceManager::initOffline's grid (the
    // launcher writes `[CAR_0] MODEL=-`, the player's car), on a copy of rand() so no later
    // draw moves
    let alone_on_a_grid = ini.get_int("SESSION_0", "TYPE").unwrap_or(0) == 3 && ini.get_int("SESSION_0", "STARTING_POSITION").unwrap_or(0) > 1;
    if !setup.track.is_empty() && (opponents(ini) > 0 || alone_on_a_grid) {
        match rustyac_game::race_cars::race_cars(ini, setup, &rustyac_game::race_cars::DriverOptions::default(), &mut || rand.next()) {
            Ok(lines) => notes.extend(lines),
            Err(message) => notes.push(format!("WARNING: the other cars of race.ini are left out: {message}")),
        }
    } else if !setup.track.is_empty() {
        let cars = root.join("content").join("cars");
        let mut copy = MsvcRand(rand.0);
        let grid = rustyac_physics::session::race_grid(ini, 1, &|model| rustyac_game::race_cars::model_skins(&cars, model), &|model, skin| rustyac_game::race_cars::skin_available(&cars, model, skin), &mut || copy.next());
        if let Some(first) = grid.ok().and_then(|grid| grid.cars.into_iter().next()).filter(|c| !c.skin.is_empty() && c.model == rustyac_game::race_cars::last_name(&setup.car)) {
            setup.player_skin = Some(first.skin);
        }
    }
    if ini.has_section("SESSION_0") {
        let sessions = SessionsIni::from_ini(ini, setup.env.base_pit_penalty_laps);
        if let Some(mode) = sessions.jump_start_penalty_mode {
            setup.env.jump_start_penalty_mode = mode;
        }
        let driver_names = (0..1 + setup.opponents.len()).map(|n| ini.get_string(&format!("CAR_{n}"), "DRIVER_NAME")).collect();
        let window = (ini.get_int("RACE", "RACE_PIT_WINDOW_START").unwrap_or(0).clamp(0, 65535) as u16, ini.get_int("RACE", "RACE_PIT_WINDOW_END").unwrap_or(0).clamp(0, 65535) as u16);
        let pit = Some(pit_header(root, window));
        setup.referee = Some(RefereeHeader { sessions, over_time_mult: over_time_mult(root), first_start_delay_ms: 20000.0, next_start_delay_ms: 10000.0, seed, wind_base, driver_names, pit });
    }
    // the AI drivers' random numbers. The desktop gives the referee `--ai-seed` + 28000, so a
    // desktop run of `--race-seed S --ai-seed <S - 28000>` draws as this drive
    if !setup.opponents.is_empty() {
        setup.ai.seed = seed.wrapping_sub(28_000);
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

/// The drive of the launcher's files: the page's car and track (`content::setup`) with the
/// session of `race_text` (and `assists_text`) on top. What `Session::from_race` drives.
pub fn setup_of(race_text: &str, assists_text: Option<&str>, seed: u32) -> Result<(SimSetup, Vec<String>), String> {
    // (as IniReader::load reads a file: CR LF as LF)
    let race = IniReader::from_text(Path::new("race.ini"), &race_text.replace("\r\n", "\n"));
    let assists = assists_text.map(|text| IniReader::from_text(Path::new("assists.ini"), &text.replace("\r\n", "\n")));
    let (car, track, layout) = car_and_track(&race);
    if car.is_empty() || track.is_empty() {
        return Err("race.ini names no car or no track ([RACE] MODEL, TRACK)".to_string());
    }
    let mut setup = crate::content::setup(&car, &track, &layout, false, "pit");
    let root = rustyac_content::install::ac_root().unwrap_or_else(|| std::path::PathBuf::from("ac"));
    let notes = apply(&mut setup, &race, assists.as_ref(), seed, &root);
    Ok((setup, notes))
}

/// What `replay_race` found.
pub struct RaceReplay {
    /// The setup the page made of the files, as a recorded drive's header has it.
    pub header: String,
    /// The recorded drive's own header.
    pub tape_header: String,
    /// After every step: every car's bodies (`GameSim::state_hash` from a fresh hash).
    pub step_hashes: Vec<u64>,
    /// The desktop's `--state-hash`: the same, folded into one hash step after step.
    pub chain: u64,
    /// The referee's results at the end (`race_out.json`), if there is a referee.
    pub race_out: Option<Vec<u8>>,
    pub cars: usize,
}

/// The proof that a drive from the launcher's files is the desktop's: the setup the page
/// makes of `race_text` / `assists_text` / `seed`, driven with the inputs of a drive the
/// desktop recorded (`rustyac.exe --race-ini … --race-seed S --ai-seed <S - 28000> --record`),
/// step for step as the desktop's `--replay … --headless --state-hash` does.
pub fn replay_race(race_text: &str, assists_text: Option<&str>, seed: u32, tape: &[u8]) -> Result<RaceReplay, String> {
    use rustyac_game::input_file::InputFile;
    use rustyac_game::sim::{GameSim, ReplaySource};
    let file = InputFile::parse(tape)?;
    let (setup, _) = setup_of(race_text, assists_text, seed)?;
    let header = setup.header();
    let mut sim = GameSim::new(setup, Box::new(ReplaySource::default()))?;
    let offset = 0xcbf2_9ce4_8422_2325u64;
    let (mut chain, mut step_hashes) = (offset, Vec::with_capacity(file.steps.len()));
    for step in &file.steps {
        sim.step_recorded(step)?;
        chain = sim.state_hash(chain);
        step_hashes.push(sim.state_hash(offset));
    }
    let race_out = sim.finish_referee().map(|(json, _, _)| json);
    Ok(RaceReplay { header, tape_header: file.setup.header(), step_hashes, chain, race_out, cars: 1 + sim.opponents.len() })
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
