// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! Task 43c: the layouts of the game's shared memory pages (`SPageFileGraphic`,
//! `SPageFileStatic`, `docs/map/telemetry.md` 5.7, 5.8) and the main thread's writer of the
//! graphics page, `SharedMemoryWriter::update` @ 0x140186230. No platform code: the desktop
//! game maps the pages, the car oracle compares this writer with the game's.

use super::flags::Flag;
use crate::car::RollingChassis;

/// `sizeof(SPageFileGraphic)`, `sizeof(SPageFileStatic)`.
pub const GRAPHICS_SIZE: usize = 0x130;
pub const STATIC_SIZE: usize = 0x2ac;

/// `AC_STATUS`
pub const AC_OFF: i32 = 0;
pub const AC_LIVE: i32 = 2;
pub const AC_PAUSE: i32 = 3;

/// A member of a page: 32-bit integer, float, floats, or a `wchar_t` string of so many
/// characters (with its terminator).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    I,
    F,
    Fs(usize),
    W(usize),
}

/// `SPageFileGraphic` (4-byte packing).
pub const GRAPHICS_FIELDS: &[(&str, Kind)] = &[
    ("packetId", Kind::I),
    ("status", Kind::I),
    ("session", Kind::I),
    ("currentTime", Kind::W(15)),
    ("lastTime", Kind::W(15)),
    ("bestTime", Kind::W(15)),
    ("split", Kind::W(15)),
    ("completedLaps", Kind::I),
    ("position", Kind::I),
    ("iCurrentTime", Kind::I),
    ("iLastTime", Kind::I),
    ("iBestTime", Kind::I),
    ("sessionTimeLeft", Kind::F),
    ("distanceTraveled", Kind::F),
    ("isInPit", Kind::I),
    ("currentSectorIndex", Kind::I),
    ("lastSectorTime", Kind::I),
    ("numberOfLaps", Kind::I),
    ("tyreCompound", Kind::W(33)),
    ("replayTimeMultiplier", Kind::F),
    ("normalizedCarPosition", Kind::F),
    ("carCoordinates", Kind::Fs(3)),
    ("penaltyTime", Kind::F),
    ("flag", Kind::I),
    ("idealLineOn", Kind::I),
    ("isInPitLane", Kind::I),
    ("surfaceGrip", Kind::F),
    ("mandatoryPitDone", Kind::I),
    ("windSpeed", Kind::F),
    ("windDirection", Kind::F),
    ("timeLimitSessionLeft", Kind::I),
    ("isEscMenuVisible", Kind::I),
];

/// `SPageFileStatic` (4-byte packing).
pub const STATIC_FIELDS: &[(&str, Kind)] = &[
    ("smVersion", Kind::W(15)),
    ("acVersion", Kind::W(15)),
    ("numberOfSessions", Kind::I),
    ("numCars", Kind::I),
    ("carModel", Kind::W(33)),
    ("track", Kind::W(33)),
    ("playerName", Kind::W(33)),
    ("playerSurname", Kind::W(33)),
    ("playerNick", Kind::W(33)),
    ("sectorCount", Kind::I),
    ("maxTorque", Kind::F),
    ("maxPower", Kind::F),
    ("maxRpm", Kind::I),
    ("maxFuel", Kind::F),
    ("suspensionMaxTravel", Kind::Fs(4)),
    ("tyreRadius", Kind::Fs(4)),
    ("maxTurboBoost", Kind::F),
    ("deprecated_1", Kind::F),
    ("deprecated_2", Kind::F),
    ("penaltiesEnabled", Kind::I),
    ("aidFuelRate", Kind::F),
    ("aidTireRate", Kind::F),
    ("aidMechanicalDamage", Kind::F),
    ("aidAllowTyreBlankets", Kind::I),
    ("aidStability", Kind::F),
    ("aidAutoClutch", Kind::I),
    ("aidAutoBlip", Kind::I),
    ("hasDRS", Kind::I),
    ("hasERS", Kind::I),
    ("hasKERS", Kind::I),
    ("kersMaxJ", Kind::F),
    ("engineBrakeSettingsCount", Kind::I),
    ("ersPowerControllerCount", Kind::I),
    ("trackSPlineLength", Kind::F),
    ("trackConfiguration", Kind::W(33)),
    ("ersMaxJ", Kind::F),
    ("isTimedRace", Kind::I),
    ("hasExtraLap", Kind::I),
    ("carSkin", Kind::W(33)),
    ("reversedGridPositions", Kind::I),
    ("PitWindowStart", Kind::I),
    ("PitWindowEnd", Kind::I),
];

/// A page being filled: the members' offsets worked out from the layout.
#[derive(Clone, Debug)]
pub struct Page {
    offsets: Vec<(&'static str, Kind, usize)>,
    pub bytes: Vec<u8>,
}

impl Page {
    pub fn new(fields: &[(&'static str, Kind)]) -> Page {
        let mut offsets = Vec::new();
        let mut at = 0usize;
        for (name, kind) in fields {
            let (align, size) = match kind {
                Kind::I | Kind::F => (4, 4),
                Kind::Fs(count) => (4, 4 * count),
                Kind::W(count) => (2, 2 * count),
            };
            at = at.div_ceil(align) * align;
            offsets.push((*name, *kind, at));
            at += size;
        }
        Page { offsets, bytes: vec![0; at.div_ceil(4) * 4] }
    }

    pub fn graphics() -> Page {
        Page::new(GRAPHICS_FIELDS)
    }

    pub fn statics() -> Page {
        Page::new(STATIC_FIELDS)
    }

    pub fn offset(&self, name: &str) -> usize {
        self.offsets.iter().find(|(n, _, _)| *n == name).unwrap_or_else(|| panic!("the page has no member {name}")).2
    }

    fn member(&self, name: &str) -> (Kind, usize) {
        let (_, kind, offset) = self.offsets.iter().find(|(n, _, _)| *n == name).unwrap_or_else(|| panic!("the page has no member {name}"));
        (*kind, *offset)
    }

    pub fn set_i(&mut self, name: &str, value: i32) {
        let (kind, at) = self.member(name);
        assert_eq!(kind, Kind::I, "{name}");
        self.bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
    }

    pub fn set_f(&mut self, name: &str, value: f32) {
        let (kind, at) = self.member(name);
        assert_eq!(kind, Kind::F, "{name}");
        self.bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
    }

    pub fn set_fs(&mut self, name: &str, values: &[f32]) {
        let (kind, at) = self.member(name);
        assert_eq!(kind, Kind::Fs(values.len()), "{name}");
        for (k, value) in values.iter().enumerate() {
            self.bytes[at + 4 * k..at + 4 * k + 4].copy_from_slice(&value.to_le_bytes());
        }
    }

    /// A string: UTF-16, cut to fit, always terminated.
    pub fn set_w(&mut self, name: &str, text: &str) {
        let (kind, at) = self.member(name);
        let Kind::W(count) = kind else { panic!("{name} is not a string") };
        self.bytes[at..at + 2 * count].fill(0);
        for (k, unit) in text.encode_utf16().take(count - 1).enumerate() {
            self.bytes[at + 2 * k..at + 2 * k + 2].copy_from_slice(&unit.to_le_bytes());
        }
    }

    pub fn get_i(&self, name: &str) -> i32 {
        let at = self.offset(name);
        i32::from_le_bytes(self.bytes[at..at + 4].try_into().unwrap())
    }

    pub fn get_f(&self, name: &str) -> f32 {
        let at = self.offset(name);
        f32::from_le_bytes(self.bytes[at..at + 4].try_into().unwrap())
    }
}

/// `AC_STATUS` while a replay plays.
pub const AC_REPLAY: i32 = 1;

/// What the main thread's objects say in a frame, besides the player's car itself.
#[derive(Clone, Copy, Debug)]
pub struct GraphicsFrame<'a> {
    /// `ReplayManager::isInReplaymode`, the pause menu's and the ESC menu's `visible`
    pub replay: bool,
    pub paused: bool,
    pub esc_menu: bool,
    /// `RaceManager::getCurrentSessionType` (race.ini's TYPE) and `getCurrentSession().laps`
    pub session_type: i32,
    pub session_laps: i32,
    /// the player's laps: `RaceManager::getCurrentLap`'s splits, `getLastLap`'s time and last
    /// split, `getBestLap`'s time
    pub current_splits: &'a [u32],
    pub last_lap_time: u32,
    pub last_lap_last_split: u32,
    pub best_lap_time: u32,
    /// `getLapCount`, `getCarLeaderboardPosition`, `getSessionTimeLeft`
    pub lap_count: i32,
    pub position: i32,
    pub session_time_left: f64,
    /// `FlagManager::getCurrentFlag`
    pub flag: Flag,
    /// `DrivingAssistManager::idealLine`, `ReplayManager::timeMult`
    pub ideal_line: bool,
    pub replay_time_multiplier: f32,
    /// `CarAvatar::inPitlane`, `mandatoryPitstopDone`
    pub in_pitlane: bool,
    pub mandatory_pit_done: bool,
    /// `timeLimitedTest->lastRemainingTime`; None without a time-limited test
    pub time_limit_left: Option<i32>,
}

impl GraphicsFrame<'_> {
    /// While a replay plays (`ReplayManager::isInReplaymode`): its time multiplier; the race
    /// manager answers the position from the replay (`ReplayManager::getCarLeaderboardPosition`
    /// is -1), the time left -1 and, offline, the last lap from the car's
    /// `physicsState.lastLap` with no splits.
    pub fn in_replay(&mut self, time_mult: f32, last_lap: u32, online: bool) {
        self.replay = true;
        self.replay_time_multiplier = time_mult;
        self.position = -1;
        self.session_time_left = -1.0;
        if !online {
            self.last_lap_time = last_lap;
            self.last_lap_last_split = 0;
        }
    }
}

/// `SharedMemoryWriter`'s graphics page state.
#[derive(Clone, Debug, Default)]
pub struct GraphicsWriter {
    /// `sharedMemories[1].packetId`, `.nullCounts`
    pub packet_id: i32,
    pub null_counts: i32,
    /// `flagManager` looked up (it is not, before the first page: that page shows no flag)
    flag_manager: bool,
    /// `lastPosition`, `currentDistance`
    last_position: [f32; 3],
    pub current_distance: f32,
}

impl GraphicsWriter {
    /// `SharedMemoryWriter::update` @ 0x140186230 (after `writeStatic`): fills the page and
    /// says so, except in the first 30 frames, which only remember where the car is.
    pub fn update(&mut self, page: &mut Page, car: &RollingChassis, f: &GraphicsFrame) -> bool {
        use crate::track::timing::time_to_string;
        let p = car.core.get_position(car.body);
        let position = [p.x, p.y, p.z];
        if self.null_counts < 30 {
            self.last_position = position;
            self.null_counts += 1;
            return false;
        }
        page.bytes.fill(0);
        self.packet_id += 1;
        page.set_i("status", if f.replay { AC_REPLAY } else if f.paused { AC_PAUSE } else { AC_LIVE });
        // timeToString(t, 3), "0" for a text that does not fit
        let time = |ms: u32| {
            let text = time_to_string(ms as i32);
            if text.encode_utf16().count() < 15 { text } else { "0".to_string() }
        };
        let tp = &car.transponder;
        page.set_w("currentTime", &time(tp.t));
        page.set_w("lastTime", &time(f.last_lap_time));
        page.set_w("bestTime", &time(f.best_lap_time));
        match f.current_splits.last() {
            // timeToSectorString(split, 1)
            Some(&split) => page.set_w("split", &format!("{}.{}", split as i32 / 1000, split as i32 % 1000 / 100)),
            None => page.set_w("split", " "),
        }
        page.set_i("completedLaps", f.lap_count);
        page.set_i("numberOfLaps", f.session_laps);
        // AC_PRACTICE 0 ... AC_DRAG 6: one less than race.ini's TYPE, else AC_UNKNOWN
        page.set_i("session", if (1..=7).contains(&f.session_type) { f.session_type - 1 } else { -1 });
        if self.flag_manager {
            page.set_i("flag", f.flag.shared_memory());
        }
        self.flag_manager = true;
        page.set_f("penaltyTime", car.get_penalty_time() as f32);
        page.set_f("sessionTimeLeft", f.session_time_left as f32);
        page.set_i("position", f.position);
        page.set_i("iCurrentTime", tp.t as i32);
        page.set_i("iLastTime", tp.last_lap as i32);
        page.set_i("iBestTime", tp.best_lap as i32);
        page.set_i("packetId", self.packet_id);
        self.packet_id += 1;
        // CarAvatar::getTyreCompound(0, true): physicsInfo.tyreCompounds, the compounds' names
        // (shortcut: the game's list is empty unless all four tyres have as many compounds; never so in Kunos cars)
        if let Some(tyre) = car.tyres.first() {
            if let Some(compound) = tyre.compound_defs.get(tyre.current_compound_index as usize) {
                page.set_w("tyreCompound", &compound.name);
            }
        }
        page.set_f("normalizedCarPosition", car.spline_locator_data.npos);
        page.set_fs("carCoordinates", &position);
        page.set_f("replayTimeMultiplier", f.replay_time_multiplier);
        let d = [self.last_position[0] - position[0], self.last_position[1] - position[1], self.last_position[2] - position[2]];
        let d2 = d[0] * d[0] + d[1] * d[1] + d[2] * d[2];
        // `ucomiss` + `je`: zero (and NaN) adds nothing
        self.current_distance += if d2 == 0.0 || d2.is_nan() { 0.0 } else { d2.sqrt() };
        self.last_position = position;
        page.set_f("distanceTraveled", self.current_distance);
        // CarAvatar::isInPit @ 0x1400d8bd0: within 1.5 m of the pit box
        let m = &car.pit_position.m[3];
        let (dx, dy, dz) = (position[0] - m[0], position[1] - m[1], position[2] - m[2]);
        let box2 = dy * dy + dx * dx + dz * dz;
        page.set_i("isInPit", ((if box2 != 0.0 { box2.sqrt() } else { 0.0 }) < 1.5) as i32);
        page.set_i("currentSectorIndex", f.current_splits.len() as i32);
        let last_sector = match f.current_splits.last() {
            Some(&split) => split,
            None if f.last_lap_time == 0 => 0,
            None => f.last_lap_last_split,
        };
        page.set_i("lastSectorTime", last_sector as i32);
        page.set_i("isInPitLane", f.in_pitlane as i32);
        page.set_i("idealLineOn", f.ideal_line as i32);
        page.set_f("surfaceGrip", car.env.dynamic_grip_level);
        page.set_i("mandatoryPitDone", f.mandatory_pit_done as i32);
        page.set_f("windSpeed", car.env.wind_speed * 3.6);
        page.set_f("windDirection", car.env.wind_direction_deg);
        page.set_i("timeLimitSessionLeft", f.time_limit_left.unwrap_or(-1));
        page.set_i("isEscMenuVisible", f.esc_menu as i32);
        true
    }
}

/// The game's `changelog.txt` in the working folder (the game's own, which the game runs
/// in): its first line, the game's version.
pub fn ac_version() -> String {
    std::fs::read("changelog.txt").map(|bytes| String::from_utf8_lossy(&bytes).lines().next().unwrap_or_default().to_string()).unwrap_or_default()
}

/// The online numbers of the static page (the server's): `RaceManager::isTimedRace` of the
/// race, `hasExtraLap`, `getInvertedGridPositions`, `getPitWindowStart` / `End`.
#[derive(Clone, Copy, Debug, Default)]
pub struct OnlineStatic {
    pub is_timed_race: bool,
    pub has_extra_lap: bool,
    pub reversed_grid_positions: i32,
    pub pit_window: (i32, i32),
}

/// What `SharedMemoryWriter::writeStatic` reads besides the car.
pub struct StaticFacts<'a> {
    /// [`ac_version`]
    pub ac_version: &'a str,
    /// `RaceManager::getSessionCount`
    pub sessions: i32,
    /// the sim's cars
    pub cars: i32,
    /// `CarAvatar::unixName`
    pub car_model: &'a str,
    /// `Sim::getTrackData`: the track's name and layout
    pub track: &'a str,
    pub track_config: &'a str,
    /// `Spline::length` of the AI line (0 without)
    pub spline_length: f32,
    /// the track's lap and sector lines
    pub sector_count: i32,
    /// `RaceManager::penaltiesEnabled`
    pub penalties: bool,
    /// `driverInfo.name`
    pub player_name: &'a str,
    /// `CarAvatar::getCurrentSkin`
    pub skin: &'a str,
    /// `physicsInfo.maxPowerW`, `maxTurboBoost`: taken when the car is built
    pub max_power_w: f32,
    pub max_turbo_boost: f32,
    /// `None`: offline (the game's `sim->client` is null)
    pub online: Option<OnlineStatic>,
}

/// `SharedMemoryWriter::writeStatic` @ 0x1401877c0: the static page of the player's car.
pub fn static_page(car: &RollingChassis, f: &StaticFacts) -> Page {
    let mut page = Page::statics();
    page.set_w("smVersion", "1.7");
    // (a string of 15 characters or more is "0", as the car's and track's of 33)
    let cut = |text: &str, limit: usize| if text.encode_utf16().count() < limit { text.to_string() } else { "0".to_string() };
    page.set_w("acVersion", &cut(f.ac_version, 15));
    page.set_i("numberOfSessions", f.sessions);
    page.set_i("numCars", f.cars);
    page.set_w("carModel", &cut(f.car_model, 33));
    page.set_w("track", &cut(f.track, 33));
    // the driver's name, cut to 30 characters, in all three
    let name: String = String::from_utf16_lossy(&f.player_name.encode_utf16().take(30).collect::<Vec<u16>>());
    for field in ["playerName", "playerSurname", "playerNick"] {
        page.set_w(field, &name);
    }
    page.set_i("sectorCount", f.sector_count);
    if let Some(drivetrain) = &car.drivetrain {
        let engine = drivetrain.engine();
        page.set_f("maxTorque", engine.get_max_torque_nm());
        page.set_i("maxRpm", engine.get_limiter_rpm());
        page.set_i("engineBrakeSettingsCount", engine.coast_settings_count());
    }
    page.set_f("maxPower", f.max_power_w);
    page.set_f("maxTurboBoost", f.max_turbo_boost);
    page.set_f("maxFuel", car.max_fuel as f32);
    let mut travel = [0.0; 4];
    let mut radius = [0.0; 4];
    for index in 0..4.min(car.tyres.len()) {
        let base = car.suspensions[index].base();
        travel[index] = base.bump_stop_up - base.bump_stop_dn;
        radius[index] = car.tyres[index].data.radius;
    }
    page.set_fs("suspensionMaxTravel", &travel);
    page.set_fs("tyreRadius", &radius);
    page.set_i("penaltiesEnabled", f.penalties as i32);
    page.set_f("aidFuelRate", car.env.fuel_consumption_rate);
    page.set_f("aidTireRate", car.env.tyre_consumption_rate);
    page.set_f("aidMechanicalDamage", car.env.mechanical_damage_rate);
    page.set_i("aidAllowTyreBlankets", car.env.allow_tyre_blankets as i32);
    if let Some(aids) = &car.aids {
        page.set_f("aidStability", aids.base().stability_control.gain);
    }
    page.set_i("aidAutoClutch", car.autoclutch.use_auto_on_change as i32);
    page.set_i("aidAutoBlip", car.auto_blip.is_active as i32);
    if let Some(aero) = &car.aero {
        page.set_i("hasDRS", aero.base().drs.is_present as i32);
    }
    // the hybrid system (`physicsInfo.hasKERS`, `hasERS`, the two allowances in joules)
    page.set_i("hasKERS", car.kers.is_some() as i32);
    page.set_i("hasERS", car.ers.is_some() as i32);
    if let Some(kers) = &car.kers {
        page.set_f("kersMaxJ", kers.max_j);
    }
    if let Some(ers) = &car.ers {
        page.set_f("ersMaxJ", ers.max_j);
        page.set_i("ersPowerControllerCount", ers.power_controllers.len() as i32);
    }
    page.set_w("trackConfiguration", &cut(f.track_config, 33));
    page.set_f("trackSPlineLength", f.spline_length);
    if let Some(online) = f.online {
        page.set_i("isTimedRace", online.is_timed_race as i32);
        page.set_i("hasExtraLap", online.has_extra_lap as i32);
        page.set_i("reversedGridPositions", online.reversed_grid_positions);
        page.set_i("PitWindowStart", online.pit_window.0);
        page.set_i("PitWindowEnd", online.pit_window.1);
    }
    page.set_w("carSkin", &cut(f.skin, 33));
    page
}

/// The frame of a referee's player (car 0), with no menus, replay or ideal line.
pub fn referee_frame<'a>(referee: &'a super::Referee) -> GraphicsFrame<'a> {
    let manager = &referee.manager;
    let laps = &manager.timing.lap_dbs[0];
    let last = laps.laps.last().cloned().unwrap_or_default();
    GraphicsFrame {
        replay: false,
        paused: false,
        esc_menu: false,
        session_type: manager.current_session.session_type,
        session_laps: manager.current_session.laps,
        current_splits: &laps.current_splits,
        last_lap_time: last.time,
        last_lap_last_split: last.splits.last().copied().unwrap_or(0),
        best_lap_time: laps.best_lap.time,
        lap_count: manager.lap_count(0) as i32,
        position: manager.car_leaderboard_position(0, &referee.views),
        session_time_left: manager.session_time_left(),
        flag: referee.flags.current_flag,
        ideal_line: false,
        replay_time_multiplier: 1.0,
        in_pitlane: referee.pit_avatar.in_pitlane,
        mandatory_pit_done: referee.pit_avatar.mandatory_pitstop_done,
        time_limit_left: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pages_have_the_game_s_layout() {
        // sizes and offsets from acs.pdb (docs/map/telemetry.md 5.7, 5.8)
        let g = Page::graphics();
        assert_eq!(g.bytes.len(), GRAPHICS_SIZE);
        for (name, offset) in [
            ("packetId", 0x000),
            ("session", 0x008),
            ("currentTime", 0x00c),
            ("lastTime", 0x02a),
            ("bestTime", 0x048),
            ("split", 0x066),
            ("position", 0x088),
            ("iBestTime", 0x094),
            ("sessionTimeLeft", 0x098),
            ("distanceTraveled", 0x09c),
            ("isInPit", 0x0a0),
            ("numberOfLaps", 0x0ac),
            ("tyreCompound", 0x0b0),
            ("replayTimeMultiplier", 0x0f4),
            ("carCoordinates", 0x0fc),
            ("penaltyTime", 0x108),
            ("flag", 0x10c),
            ("surfaceGrip", 0x118),
            ("windSpeed", 0x120),
            ("timeLimitSessionLeft", 0x128),
            ("isEscMenuVisible", 0x12c),
        ] {
            assert_eq!(g.offset(name), offset, "{name}");
        }
        let s = Page::statics();
        assert_eq!(s.bytes.len(), STATIC_SIZE);
        for (name, offset) in [
            ("smVersion", 0x000),
            ("acVersion", 0x01e),
            ("numberOfSessions", 0x03c),
            ("numCars", 0x040),
            ("carModel", 0x044),
            ("track", 0x086),
            ("playerName", 0x0c8),
            ("playerSurname", 0x10a),
            ("playerNick", 0x14c),
            ("sectorCount", 0x190),
            ("maxTorque", 0x194),
            ("maxPower", 0x198),
            ("maxRpm", 0x19c),
            ("maxFuel", 0x1a0),
            ("suspensionMaxTravel", 0x1a4),
            ("tyreRadius", 0x1b4),
            ("maxTurboBoost", 0x1c4),
            ("penaltiesEnabled", 0x1d0),
            ("aidFuelRate", 0x1d4),
            ("aidAllowTyreBlankets", 0x1e0),
            ("aidStability", 0x1e4),
            ("aidAutoClutch", 0x1e8),
            ("aidAutoBlip", 0x1ec),
            ("hasDRS", 0x1f0),
            ("kersMaxJ", 0x1fc),
            ("engineBrakeSettingsCount", 0x200),
            ("trackSPlineLength", 0x208),
            ("trackConfiguration", 0x20c),
            ("ersMaxJ", 0x250),
            ("isTimedRace", 0x254),
            ("carSkin", 0x25c),
            ("reversedGridPositions", 0x2a0),
            ("PitWindowEnd", 0x2a8),
        ] {
            assert_eq!(s.offset(name), offset, "{name}");
        }
    }
}
