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
