// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! The session's conditions, read from the game's own files the way `acs.exe` reads them:
//! `Documents\Assetto Corsa\cfg\race.ini` (the last session the launcher set up) and
//! `cfg\assists.ini`.
//!
//! * [`RaceIni`]: the keys of race.ini that change a single car's physics
//!   (`RaceManager::initOffline` @ 0x14013a6c0, `Track::initDynamicTrack` @ 0x140278300).
//! * [`wind_settings`] / [`generate_wind`]: the wind of a session. The file gives a range;
//!   the game draws a speed in it, then (on the physics thread) once more 80..120 % of that
//!   and a direction within 20 degrees of the file's. Both draws are the C runtime's `rand()`,
//!   seeded from the clock in the game: here the caller hands the numbers in.
//! * [`Assists`]: `cfg\assists.ini` (`DrivingAssistManager::DrivingAssistManager` @ 0x1400fbd90).
//!
//! Nothing else of the two files reaches the car: `[WEATHER]` and `[LIGHTING]` are graphics,
//! `[GROOVE]` is the dark line's opacity, `[CAR_0] SETUP` is not read by `acs.exe` at all.

use crate::car::{ChassisEnvironment, RollingChassis};
use crate::data::ini::IniReader;
use crate::track::DynamicTrack;

/// The C runtime's `rand()` (MSVCR120). The value inside is its state: `srand(seed)` sets it
/// to `seed`, so a generator that has drawn some numbers is carried on by handing its state
/// over as a seed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MsvcRand(pub u32);

impl MsvcRand {
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> i32 {
        self.0 = self.0.wrapping_mul(214013).wrapping_add(2531011);
        ((self.0 >> 16) & 0x7fff) as i32
    }
}

/// `1 / 32767.5`-ish: the factor the game turns `rand()` into 0..1 with (0x38000100).
const RAND_TO_UNIT: f32 = f32::from_bits(0x3800_0100);
/// km/h to m/s as `Speed::fromKMH` @ 0x140239970 has it (0x3e8e38e4).
const KMH_TO_MS: f32 = f32::from_bits(0x3e8e_38e4);
/// m/s to km/h as `Speed::kmh` @ 0x140058f50 has it (0x40666666).
const MS_TO_KMH: f32 = f32::from_bits(0x4066_6666);

/// `ksRand(min, max)` @ 0x140033770 for one `rand()` result.
pub fn ks_rand(rand: i32, min: f32, max: f32) -> f32 {
    (rand as f32 * RAND_TO_UNIT) * (max - min) + min
}

/// `clamp<float>` @ 0x14002b6c0 as `initOffline` calls it for the wind (a NaN passes).
fn clamp(x: f32, low: f32, high: f32) -> f32 {
    if x > high {
        high
    } else if low > x {
        low
    } else {
        x
    }
}

/// `RaceManager::windSettings`: the session's mean wind.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WindSettings {
    /// `baseSpeed`, km/h
    pub base_speed: f32,
    /// `baseDirection`, degrees
    pub base_direction: f32,
}

/// The `[WIND]` part of `RaceManager::initOffline` (0x14013bd75..): a speed between
/// `SPEED_KMH_MIN` and `SPEED_KMH_MAX` (each kept within 0..40), and `DIRECTION_DEG`, or any
/// direction when that is negative. `rand` is called once, and once more for the direction.
pub fn wind_settings(speed_kmh_min: f32, speed_kmh_max: f32, direction_deg: f32, rand: &mut dyn FnMut() -> i32) -> WindSettings {
    let min = clamp(speed_kmh_min, 0.0, 40.0);
    let max = clamp(speed_kmh_max, 0.0, 40.0);
    let kmh = ks_rand(rand(), min, max);
    let speed = kmh * KMH_TO_MS;
    let mut direction = direction_deg;
    if direction < 0.0 {
        direction = ks_rand(rand(), 0.0, 360.0);
    }
    WindSettings { base_direction: direction, base_speed: speed * MS_TO_KMH }
}

/// `RaceManager::generateWind`'s job on the physics thread (lambda @ 0x140133ac0): the wind
/// handed to `PhysicsEngine::setWind`, as (speed in m/s, direction in degrees). `None`
/// without a base speed above zero: no wind, and `rand` is not called.
#[allow(clippy::neg_cmp_op_on_partial_ord)] // the game's `comiss` + `jbe`: a NaN gives no wind
pub fn generate_wind(settings: &WindSettings, rand: &mut dyn FnMut() -> i32) -> Option<(f32, f32)> {
    let base = settings.base_speed;
    if !(base > 0.0) {
        return None;
    }
    let a = base * f32::from_bits(0x3e4c_cccd);
    let negative = -a;
    let r1 = rand() as f32 * RAND_TO_UNIT;
    let kmh = ((r1 * (a - negative)) + negative) + base;
    let speed = kmh * KMH_TO_MS;
    let r2 = rand() as f32 * RAND_TO_UNIT;
    let direction = settings.base_direction + ((r2 * 40.0) - 20.0);
    Some((speed, direction))
}

/// A wind asked for outright in km/h (not drawn): what `PhysicsEngine::setWind` gets.
pub fn wind_from_kmh(kmh: f32) -> f32 {
    kmh * KMH_TO_MS
}

/// `[DYNAMIC_TRACK]` as written in race.ini.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DynamicTrackIni {
    pub session_start: f32,
    pub randomness: f32,
    pub lap_gain: f32,
    pub session_transfer: f32,
}

impl DynamicTrackIni {
    /// `Track::initDynamicTrack` with one `rand()` result.
    pub fn build(&self, rand: i32) -> DynamicTrack {
        DynamicTrack::from_race_ini(self.session_start, self.randomness, self.lap_gain, self.session_transfer, rand)
    }
}

/// `[WIND]` as written in race.ini.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WindIni {
    pub speed_kmh_min: f32,
    pub speed_kmh_max: f32,
    pub direction_deg: f32,
}

impl WindIni {
    pub fn settings(&self, rand: &mut dyn FnMut() -> i32) -> WindSettings {
        wind_settings(self.speed_kmh_min, self.speed_kmh_max, self.direction_deg, rand)
    }
}

/// What race.ini says that reaches one car's physics. A missing key reads as 0, as the game's
/// `INIReader` gives it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RaceIni {
    /// `[TEMPERATURE] AMBIENT, ROAD`, deg C, when the section is there (else the engine keeps
    /// its constructor's 26 / 30).
    pub temperature: Option<(f32, f32)>,
    /// `[DYNAMIC_TRACK]`, when the section is there.
    pub dynamic_track: Option<DynamicTrackIni>,
    /// `[WIND]`, when the section is there.
    pub wind: Option<WindIni>,
    /// `[CAR_0] BALLAST`, kg: 0 unless `[HEADER] VERSION` is above 1 and the value above 0.
    pub ballast_kg: f32,
    /// `[CAR_0] RESTRICTOR`, under the same condition.
    pub restrictor: f32,
    /// `[RACE] PENALTIES` is 1 or more: two tyres may leave the track, else any number.
    pub penalties: bool,
    /// Not physics, for the display: `[RACE] TRACK`, `CONFIG_TRACK`, `MODEL`, `[WEATHER] NAME`.
    pub track: String,
    pub track_config: String,
    pub model: String,
    pub weather: String,
    /// `[CAR_0] SETUP`: not a key `acs.exe` reads (launchers write it); rustyAC loads the
    /// setup it names.
    pub setup: String,
    /// `[SESSION_0] TYPE`: 1 practice, 2 qualifying, 3 race, 4 hot-lap, 5 time attack, 6 drift,
    /// 7 drag; `None` without the section. Only a hot-lap session arms the first lap
    /// (`RaceManager::initOffline` @ 0x14013a6c0).
    pub session_type: Option<i32>,
    /// `[SESSION_0] SPAWN_SET`: `PIT`, `START` or `HOTLAP_START`; empty without the key.
    pub spawn_set: String,
    /// Any `[SESSION_n]` has `TYPE=4`: `RaceManager::initOffline` then arms the first lap of
    /// every car (`CarAvatar::armFirstLap`): the clock starts again when the car first crosses
    /// the line. The flag belongs to the file, not to one session.
    pub arm_first_lap: bool,
}

impl RaceIni {
    pub fn from_ini(ini: &IniReader) -> RaceIni {
        let float = |section: &str, key: &str| ini.get_float(section, key).unwrap_or(0.0);
        let int = |section: &str, key: &str| ini.get_int(section, key).unwrap_or(0);
        let mut race = RaceIni {
            penalties: int("RACE", "PENALTIES") >= 1,
            track: ini.get_string("RACE", "TRACK"),
            track_config: ini.get_string("RACE", "CONFIG_TRACK"),
            model: ini.get_string("RACE", "MODEL"),
            weather: ini.get_string("WEATHER", "NAME"),
            setup: ini.get_string("CAR_0", "SETUP"),
            session_type: ini.has_section("SESSION_0").then(|| int("SESSION_0", "TYPE")),
            arm_first_lap: (0..).map(|n| format!("SESSION_{n}")).take_while(|section| ini.has_section(section)).any(|section| int(&section, "TYPE") == 4),
            spawn_set: ini.get_string("SESSION_0", "SPAWN_SET"),
            ..RaceIni::default()
        };
        if ini.has_section("TEMPERATURE") {
            race.temperature = Some((float("TEMPERATURE", "AMBIENT"), float("TEMPERATURE", "ROAD")));
        }
        if ini.ready && ini.has_section("DYNAMIC_TRACK") {
            race.dynamic_track = Some(DynamicTrackIni {
                session_start: float("DYNAMIC_TRACK", "SESSION_START"),
                randomness: float("DYNAMIC_TRACK", "RANDOMNESS"),
                lap_gain: float("DYNAMIC_TRACK", "LAP_GAIN"),
                session_transfer: float("DYNAMIC_TRACK", "SESSION_TRANSFER"),
            });
        }
        if ini.has_section("WIND") {
            race.wind = Some(WindIni {
                speed_kmh_min: float("WIND", "SPEED_KMH_MIN"),
                speed_kmh_max: float("WIND", "SPEED_KMH_MAX"),
                direction_deg: float("WIND", "DIRECTION_DEG"),
            });
        }
        // ballast and restrictor came with version 2 of the file
        let version = if ini.has_section("HEADER") { int("HEADER", "VERSION") } else { 1 };
        if version > 1 {
            let ballast = float("CAR_0", "BALLAST");
            if ballast > 0.0 {
                race.ballast_kg = ballast;
            }
            let restrictor = float("CAR_0", "RESTRICTOR");
            if restrictor > 0.0 {
                race.restrictor = restrictor;
            }
        }
        race
    }

    /// The part the engine holds before the car is built: the temperatures (tyres, brakes and
    /// water start from the air's) and how many tyres may leave the track.
    pub fn apply_to_env(&self, env: &mut ChassisEnvironment) {
        if let Some((ambient, road)) = self.temperature {
            env.ambient_temperature = ambient;
            env.road_temperature = road;
        }
        env.allowed_tyres_out = if self.penalties { 2 } else { -1 };
    }

    /// The part set on the car once it exists (`CarAvatar::setBallastKG`, `setRestrictor`).
    pub fn apply_to_car(&self, car: &mut RollingChassis) {
        if self.ballast_kg > 0.0 {
            car.ballast_kg = self.ballast_kg;
        }
        if self.restrictor > 0.0 {
            car.set_restrictor(self.restrictor);
        }
    }
}

/// `cfg\assists.ini [ASSISTS]`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Assists {
    /// `ABS`, `TRACTION_CONTROL`: 0 off, 1 as the car has it ("factory"), 2 on
    pub abs: i32,
    pub traction_control: i32,
    /// `STABILITY_CONTROL`, percent
    pub stability_control: f32,
    pub auto_clutch: bool,
    pub auto_blip: bool,
    pub auto_shifter: bool,
    /// `DAMAGE` / 100: `PhysicsEngine::mechanicalDamageRate`
    pub mechanical_damage_rate: f32,
    /// `FUEL_RATE`, `TYRE_WEAR`
    pub fuel_consumption_rate: f32,
    pub tyre_consumption_rate: f32,
    /// `TYRE_BLANKETS`
    pub allow_tyre_blankets: bool,
}

impl Assists {
    pub fn from_ini(ini: &IniReader) -> Assists {
        let float = |key: &str| ini.get_float("ASSISTS", key).unwrap_or(0.0);
        let int = |key: &str| ini.get_int("ASSISTS", key).unwrap_or(0);
        Assists {
            abs: int("ABS"),
            traction_control: int("TRACTION_CONTROL"),
            stability_control: float("STABILITY_CONTROL"),
            auto_clutch: int("AUTO_CLUTCH") != 0,
            auto_blip: int("AUTO_BLIP") != 0,
            auto_shifter: int("AUTO_SHIFTER") != 0,
            mechanical_damage_rate: int("DAMAGE") as f32 * 0.01,
            fuel_consumption_rate: float("FUEL_RATE"),
            tyre_consumption_rate: float("TYRE_WEAR"),
            allow_tyre_blankets: int("TYRE_BLANKETS") != 0,
        }
    }

    /// The rates and the tyre blankets, which the engine holds.
    pub fn apply_to_env(&self, env: &mut ChassisEnvironment) {
        env.mechanical_damage_rate = self.mechanical_damage_rate;
        env.fuel_consumption_rate = self.fuel_consumption_rate;
        env.tyre_consumption_rate = self.tyre_consumption_rate;
        env.allow_tyre_blankets = self.allow_tyre_blankets;
    }

    /// ABS, traction control and the stability aid of a car that has its aids.
    pub fn apply_to_car(&self, car: &mut RollingChassis) {
        if let Some(aids) = &mut car.aids {
            aids.base_mut().apply_driving_assists(self.abs, self.traction_control, self.stability_control);
        }
    }
}

/// One car of race.ini as `RaceManager::initOffline` @ 0x14013a6c0 makes it.
#[derive(Clone, Debug, PartialEq)]
pub struct RaceCar {
    /// The number of its section (`[CAR_<n>]`), and whether the file has that section. A car
    /// without one is made all the same: with the model, the skin and the driver level of the
    /// car before it (the game skips the reads, not the car).
    pub section: usize,
    pub has_section: bool,
    /// `MODEL` and `MODEL_CONFIG` (a `-` keeps those of the car before), and the skin the car gets.
    pub model: String,
    pub config: String,
    pub skin: String,
    /// The skin is not the one of the file: that one is empty or not a folder of the car.
    pub skin_was_picked: bool,
    /// `BALLAST` (kg) and `RESTRICTOR`: 0 unless `[HEADER] VERSION` is 2 or more and the value
    /// is above 0.
    pub ballast_kg: f32,
    pub restrictor: f32,
    /// `DRIVER_NAME`, and `NATION_CODE` when the key is there.
    pub driver_name: String,
    pub nation_code: Option<String>,
    /// What the driver of the car is made from (every car but the first gets the AI of the
    /// game): `[CAR_n] AI_LEVEL` as read (0 without the key), and `AI_AGGRESSION` as the game
    /// keeps it (`saturate(value * 0.01) * 0.95`; 0 without the key).
    pub ai_level: f32,
    pub ai_aggression: f32,
    /// The number of the car among the cars that were made (`Sim::cars`): its guid, and its
    /// pit box (`AC_PIT_<n>`).
    pub index: usize,
    /// Its slot of the `START` set (`AC_START_<n>`). -1 or a number the track has no slot for
    /// puts the car at the origin of the world in the game.
    pub start_index: i32,
}

/// The cars of a race.ini.
#[derive(Clone, Debug, PartialEq)]
pub struct RaceGrid {
    /// `RaceManager::carsToBeLoaded`: `[RACE] CARS` (at least 1), at most the pit boxes.
    pub cars_to_be_loaded: i32,
    pub cars: Vec<RaceCar>,
    /// `[RACE] AI_LEVEL`
    pub ai_level: f32,
    /// `[SESSION_0] TYPE` (1 without any session) and what a race takes as the place of the
    /// player on the grid: `STARTING_POSITION` of the session, or `[RACE] CARS` as written.
    pub first_session_type: i32,
    pub forced_position: i32,
}

/// `ksRand()` @ 0x1400337c0: `rand()` as a number from 0 to just under 1.
pub fn ks_rand_unit(rand: &mut dyn FnMut() -> i32) -> f32 {
    rand() as f32 * RAND_TO_UNIT
}

/// The `[CAR_n]` loop of `RaceManager::initOffline` @ 0x14013a6c0 (0x14013c1a0 to 0x14013cc8f)
/// with what it takes from the lines before it.
///
/// * `pit_slots`: `TrackAvatar::getMaxSlotsAvailable("PIT")`.
/// * `skins`: `RaceManager::getModelSkins` @ 0x140138c20, the folders of
///   `content/cars/<model>/skins` in the order the file system lists them (on NTFS: by name).
/// * `skin_exists`: `RaceManager::isSkinAvailable` @ 0x14013ebc0.
/// * `rand`: the `rand()` of the C runtime where the loop finds it (the game has drawn the wind
///   and the time of the gearbox lock before; a car without a valid skin draws one number, the
///   first time its model has none).
///
/// A count of 0 cars is the "CANNOT HAVE 0 CARS IN RACE.INI" of the game.
pub fn race_grid(ini: &IniReader, pit_slots: usize, skins: &dyn Fn(&str) -> Vec<String>, skin_exists: &dyn Fn(&str, &str) -> bool, rand: &mut dyn FnMut() -> i32) -> Result<RaceGrid, String> {
    let float = |section: &str, key: &str| ini.get_float(section, key).unwrap_or(0.0);
    let int = |section: &str, key: &str| ini.get_int(section, key).unwrap_or(0);
    let race_ai = float("RACE", "AI_LEVEL");
    let version = if ini.has_section("HEADER") { int("HEADER", "VERSION") } else { 1 };
    // the first session: its type, and the grid place of a race
    let raw_cars = int("RACE", "CARS");
    let (mut first_session_type, mut forced_position) = (1, -1);
    if ini.has_section("SESSION_0") {
        first_session_type = int("SESSION_0", "TYPE");
        if first_session_type == 3 {
            forced_position = if ini.has_key("SESSION_0", "STARTING_POSITION") { int("SESSION_0", "STARTING_POSITION") } else { raw_cars };
        }
    }
    let cars_to_be_loaded = raw_cars.max(1).min(pit_slots.min(i32::MAX as usize) as i32);
    if cars_to_be_loaded == 0 {
        return Err("CANNOT HAVE 0 CARS IN RACE.INI".to_string());
    }
    let mut model = ini.get_string("RACE", "MODEL");
    let mut config = ini.get_string("RACE", "MODEL_CONFIG");
    // where the next car without a skin of its own goes on in the list of its model
    let mut next_skin: Vec<(String, usize)> = Vec::new();
    let mut car_ai = 1.0f32;
    let mut ai_aggression = 0.0f32;
    let mut skin = String::new();
    let mut cars: Vec<RaceCar> = Vec::new();
    for i in 0..cars_to_be_loaded as usize {
        let section = format!("CAR_{i}");
        // (a section that is not there: the five reads are jumped over at 0x14013c208, and the
        // car is made from what the car before left in these variables)
        let has_section = ini.has_section(&section);
        if has_section {
            let m = ini.get_string(&section, "MODEL");
            if m != "-" {
                model = m;
            }
            let c = ini.get_string(&section, "MODEL_CONFIG");
            if c != "-" {
                config = c;
            }
            skin = ini.get_string(&section, "SKIN");
            if i > 0 {
                car_ai = float(&section, "AI_LEVEL");
            }
            ai_aggression = if ini.has_key(&section, "AI_AGGRESSION") {
                let x = float(&section, "AI_AGGRESSION") * 0.01;
                // saturate @ 0x1400244b0 (`ja` then `jbe`: a value that is not a number stays)
                #[allow(clippy::manual_clamp)] // the game's two compares, in its order
                let x = if x > 1.0 {
                    1.0
                } else if 0.0 > x {
                    0.0
                } else {
                    x
                };
                x * 0.95
            } else {
                0.0
            };
        }
        let mut skin_was_picked = false;
        if skin.is_empty() || !skin_exists(&model, &skin) {
            let list = skins(&model);
            if !list.is_empty() {
                let at = match next_skin.iter().position(|(name, _)| *name == model) {
                    Some(at) => at,
                    None => {
                        next_skin.push((model.clone(), (list.len() as f32 * ks_rand_unit(rand)) as i32 as usize));
                        next_skin.len() - 1
                    }
                };
                let k = &mut next_skin[at].1;
                // (the game reads the list without looking at its length; the number is below it)
                skin = list.get(*k).cloned().unwrap_or_default();
                skin_was_picked = true;
                *k += 1;
                if *k >= list.len() {
                    *k = 0;
                }
            }
        }
        let (mut ballast_kg, mut restrictor) = (0.0, 0.0);
        if version >= 2 {
            let b = float(&section, "BALLAST");
            if 0.0 < b {
                ballast_kg = b;
            }
            let r = float(&section, "RESTRICTOR");
            if 0.0 < r {
                restrictor = r;
            }
        }
        // Sim::addCar: the pit box and the grid slot with the number of the car among the cars
        let index = cars.len();
        let mut start_index = index as i32;
        match first_session_type {
            3 => {
                let f = if i == 0 { forced_position } else { i as i32 };
                start_index = f - 1;
                if i != 0 && forced_position <= f {
                    start_index = f;
                }
            }
            7 => start_index = i as i32,
            _ => {}
        }
        cars.push(RaceCar {
            section: i,
            has_section,
            model: model.clone(),
            config: config.clone(),
            skin: skin.clone(),
            skin_was_picked,
            ballast_kg,
            restrictor,
            driver_name: ini.get_string(&section, "DRIVER_NAME"),
            nation_code: ini.has_key(&section, "NATION_CODE").then(|| ini.get_string(&section, "NATION_CODE")),
            ai_level: car_ai,
            ai_aggression,
            index,
            start_index,
        });
    }
    Ok(RaceGrid { cars_to_be_loaded, cars, ai_level: race_ai, first_session_type, forced_position })
}

/// The numbers of `rand()` `RaceManager::initOffline` draws before its `[CAR_n]` loop: one
/// for the speed of the wind (only with a `[WIND]` section), one more for its direction when
/// the one of the file is below 0, and one for the time the gearbox stays locked before a
/// race start.
#[allow(clippy::neg_cmp_op_on_partial_ord)]
pub fn draws_before_the_cars(ini: &IniReader, rand: &mut dyn FnMut() -> i32) {
    if ini.has_section("WIND") {
        rand();
        if !(ini.get_float("WIND", "DIRECTION_DEG").unwrap_or(0.0) >= 0.0) {
            rand();
        }
    }
    rand();
}

/// The folders of `content/cars/<model>/skins` as `Path::getFolders` @ 0x140231260 lists
/// them: in the order of the file system (on NTFS: by name).
pub fn model_skins(cars_folder: &std::path::Path, model: &str) -> Vec<String> {
    // (no sorting: `read_dir` lists a folder with the same system call as the game's
    // `_wfindfirst`, so the order is the game's on any file system)
    match std::fs::read_dir(cars_folder.join(model).join("skins")) {
        Ok(entries) => entries.filter_map(|e| e.ok()).filter(|e| e.path().is_dir()).map(|e| e.file_name().to_string_lossy().into_owned()).collect(),
        Err(_) => Vec::new(),
    }
}

/// `RaceManager::isSkinAvailable` @ 0x14013ebc0: is `content/cars/<model>/skins/<skin>` a
/// folder. The game glues the text together, so a skin written as a path of its own
/// (`C:\…`) is not found.
pub fn skin_available(cars_folder: &std::path::Path, model: &str, skin: &str) -> bool {
    let mut text = cars_folder.as_os_str().to_os_string();
    text.push(format!("/{model}/skins/{skin}"));
    std::path::Path::new(&text).is_dir()
}

/// `TrackAvatar::getMaxSlotsAvailable("PIT")` as `RaceManager::initOffline` finds it: the pit
/// boxes of the track are listed once by the track itself and once more for every session
/// whose `SPAWN_SET` is `PIT` (`initRespawnPositionSet` @ 0x1401cb8c0 adds to the list without
/// emptying it). A file without any `[SESSION_n]` gets one session, which starts in the pits.
pub fn pit_slots_listed(ini: &IniReader, pit_boxes: usize) -> usize {
    let sessions: Vec<String> = (0..).map(|n| format!("SESSION_{n}")).take_while(|section| ini.has_section(section)).collect();
    let pit_sessions = if sessions.is_empty() { 1 } else { sessions.iter().filter(|section| ini.get_string(section, "SPAWN_SET") == "PIT").count() };
    pit_boxes * (1 + pit_sessions)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn ini(text: &str) -> IniReader {
        IniReader::from_text(Path::new("race.ini"), text)
    }

    #[test]
    fn the_runtime_s_rand() {
        // srand(1): the first numbers every MSVC program prints
        let mut rand = MsvcRand(1);
        assert_eq!([rand.next(), rand.next(), rand.next()], [41, 18467, 6334]);
    }

    #[test]
    fn race_ini_of_a_cold_day() {
        let race = RaceIni::from_ini(&ini(
            "[RACE]\nTRACK=spa\nMODEL=ks_ferrari_f2004\nPENALTIES=0\n[TEMPERATURE]\nAMBIENT=14\nROAD=20\n[DYNAMIC_TRACK]\nSESSION_START=100\nSESSION_TRANSFER=100\nRANDOMNESS=0\nLAP_GAIN=1\n[WIND]\nSPEED_KMH_MIN=10\nSPEED_KMH_MAX=10\nDIRECTION_DEG=0\n[HEADER]\nVERSION=2\n[CAR_0]\nSETUP=\nBALLAST=0\nRESTRICTOR=0\n[WEATHER]\nNAME=3_clear\n",
        ));
        assert_eq!(race.temperature, Some((14.0, 20.0)));
        assert_eq!(race.dynamic_track, Some(DynamicTrackIni { session_start: 100.0, randomness: 0.0, lap_gain: 1.0, session_transfer: 100.0 }));
        assert_eq!(race.wind, Some(WindIni { speed_kmh_min: 10.0, speed_kmh_max: 10.0, direction_deg: 0.0 }));
        assert_eq!((race.ballast_kg, race.restrictor, race.penalties), (0.0, 0.0, false));
        assert_eq!((race.track.as_str(), race.model.as_str(), race.weather.as_str(), race.setup.as_str()), ("spa", "ks_ferrari_f2004", "3_clear", ""));
        let mut env = ChassisEnvironment::default();
        race.apply_to_env(&mut env);
        assert_eq!((env.ambient_temperature, env.road_temperature, env.allowed_tyres_out), (14.0, 20.0, -1));
        // full grip from the start, whatever is drawn
        let track = race.dynamic_track.unwrap().build(12345);
        assert_eq!((track.enabled, track.base_grip, track.grip_per_lap), (true, 1.0, 0.01));
    }

    #[test]
    fn sections_that_are_missing_change_nothing() {
        let race = RaceIni::from_ini(&ini("[RACE]\nPENALTIES=1\n[CAR_0]\nBALLAST=50\nRESTRICTOR=30\n"));
        assert_eq!((race.temperature, race.dynamic_track, race.wind), (None, None, None));
        // without [HEADER] the file is version 1: no ballast, no restrictor
        assert_eq!((race.ballast_kg, race.restrictor, race.penalties), (0.0, 0.0, true));
        let with_header = RaceIni::from_ini(&ini("[HEADER]\nVERSION=2\n[CAR_0]\nBALLAST=50\nRESTRICTOR=30\n"));
        assert_eq!((with_header.ballast_kg, with_header.restrictor), (50.0, 30.0));
        let mut env = ChassisEnvironment::default();
        race.apply_to_env(&mut env);
        assert_eq!((env.ambient_temperature, env.road_temperature, env.allowed_tyres_out), (26.0, 30.0, 2));
    }

    #[test]
    fn the_wind_is_drawn_twice() {
        // 10 km/h in the file: the base stays 10, the session's wind is 8 to 12 km/h from
        // within 20 degrees of the file's direction
        let mut none = || 0;
        let settings = wind_settings(10.0, 10.0, 0.0, &mut none);
        assert_eq!(settings, WindSettings { base_speed: 10.0, base_direction: 0.0 });
        let (low, left) = generate_wind(&settings, &mut || 0).unwrap();
        let (high, right) = generate_wind(&settings, &mut || 32767).unwrap();
        assert!((low * 3.6 - 8.0).abs() < 1e-4 && (high * 3.6 - 12.0).abs() < 1e-3, "{low} {high}");
        assert!((left + 20.0).abs() < 1e-4 && (right - 20.0).abs() < 1e-2, "{left} {right}");
        // (the restrictor's own conversion is `RollingChassis::set_restrictor`)
        // no wind in the file: none, and nothing is drawn
        let calm = wind_settings(0.0, 0.0, 0.0, &mut none);
        assert_eq!(generate_wind(&calm, &mut || panic!("no draw without wind")), None);
        // the range is kept within 0..40 km/h, a negative direction is drawn
        let wild = wind_settings(-5.0, 90.0, -1.0, &mut || 32767);
        assert!((wild.base_speed - 40.0).abs() < 0.01 && (wild.base_direction - 360.0).abs() < 0.2, "{wild:?}");
    }

    #[test]
    fn assists_ini() {
        let a = Assists::from_ini(&ini("[ASSISTS]\nABS=1\nTRACTION_CONTROL=1\nSTABILITY_CONTROL=0\nAUTO_CLUTCH=0\nDAMAGE=0\nFUEL_RATE=1\nTYRE_WEAR=1\nTYRE_BLANKETS=1\n"));
        assert_eq!((a.abs, a.traction_control, a.stability_control), (1, 1, 0.0));
        assert_eq!((a.mechanical_damage_rate, a.fuel_consumption_rate, a.tyre_consumption_rate, a.allow_tyre_blankets), (0.0, 1.0, 1.0, true));
        let half = Assists::from_ini(&ini("[ASSISTS]\nDAMAGE=50\n"));
        assert_eq!(half.mechanical_damage_rate, 0.5);
    }
}
