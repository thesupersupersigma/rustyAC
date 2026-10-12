// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! What the referee says on the screen: the session banner (`SessionStatusSystemMessage`
//! with `RaceManager::getRaceText` / `getPracticeQualifyText`), `RaceManager::renderHUD`,
//! and the three penalty messages (`LapInvalidatorAvatar`, `LockControlsTimeAvatar`,
//! `DriveThroughPenaltyAvatar`). The strings are the game's English ones (it passes them
//! through its translation table; none is in `messages.ini`).

use super::manager::{CarView, RaceManager, SESSION_HOTLAP, SESSION_PRACTICE, SESSION_QUALIFY, SESSION_RACE};

/// `timeToWString(ms, false, false)` @ 0x1400b34c0.
pub fn time_to_wstring(ms: i32) -> String {
    if ms < 1 {
        return "0:00".to_string();
    }
    let s = ms / 1000;
    let h = s / 3600;
    let m = s / 60 - h * 60;
    let sec = s - (h * 60 + m) * 60;
    if h < 1 {
        format!("{m}:{sec:02}")
    } else {
        format!("{h}h{m:02}m")
    }
}

/// `cvttsd2si` to 32 bits.
fn cvttsd2si(value: f64) -> i32 {
    if !(-2_147_483_648.0..2_147_483_648.0).contains(&value) {
        i32::MIN
    } else {
        value as i32
    }
}

/// `RaceManager::getPracticeQualifyText` @ 0x140138d80 (offline).
pub fn practice_qualify_text(manager: &RaceManager) -> String {
    let left = cvttsd2si(manager.session_time_left());
    if left > -1 {
        return time_to_wstring(left);
    }
    let session = &manager.current_session;
    if session.session_type == SESSION_PRACTICE {
        #[allow(clippy::float_cmp)]
        if session.duration_minutes == 0.0 {
            return String::new();
        }
        return "PRACTICE OVER".to_string();
    }
    if manager.car_count() > 0 && manager.has_completed_flag(0) && session.session_type == SESSION_QUALIFY {
        return "QUALIFY OVER".to_string();
    }
    "OVERTIME".to_string()
}

/// `RaceManager::getRaceText` @ 0x140139150 (offline: no pit window).
pub fn race_text(manager: &RaceManager) -> String {
    let session = &manager.current_session;
    let leader_laps = session.leader_completed_laps;
    let to_start = manager.time_to_session_start() as f32;
    if !session.is_timed_race {
        let mut text = if to_start < 10000.0 { format!("{} / {} ", leader_laps.wrapping_add(1), session.laps) } else { time_to_wstring(cvttsd2si(manager.time_to_session_start())) };
        if leader_laps == (session.laps - 1) as u32 {
            text = "LEADER IS ON FINAL LAP".to_string();
        } else if session.laps as u32 <= leader_laps {
            text = if manager.car_count() > 0 && manager.lap_count(0) < session.laps as u32 { "FINAL LAP".to_string() } else { "RACE OVER".to_string() };
        }
        return text;
    }
    if to_start < 10000.0 {
        let left = cvttsd2si(manager.session_time_left());
        if left < 1 {
            return String::new();
        }
        if session.start_time < manager.now {
            return time_to_wstring(left);
        }
        return String::new();
    }
    time_to_wstring(cvttsd2si(manager.time_to_session_start()))
}

/// What the messages read of the player's car.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PlayerState {
    /// `CarAvatar::hasPenalty()`, s
    pub penalty_time: f64,
    /// `Car::disableMinSpeedPenaltyClear`
    pub min_speed_clear_disabled: bool,
    /// `physicsState.lockControlsTime` less the clock, ms (0 or less: not locked)
    pub lock_controls_time: f64,
    /// `PenaltyManager::pitPenaltyLaps`
    pub pit_penalty_laps: i16,
    /// `CarAvatar::isCurrentLapValid()`
    pub current_lap_valid: bool,
    pub is_black_flagged: bool,
}

/// The session banner at the top of the screen (`SessionStatusSystemMessage::update`
/// @ 0x140169bc0; `system/cfg/session_info.ini` shows all three kinds as shipped).
pub fn session_banner(manager: &RaceManager, player: &PlayerState, results_displayed: bool) -> String {
    #[allow(clippy::float_cmp)]
    if !(player.penalty_time == 0.0 && player.lock_controls_time <= 0.0 && !results_displayed) {
        return String::new();
    }
    match manager.current_session.session_type {
        SESSION_PRACTICE | SESSION_QUALIFY => practice_qualify_text(manager),
        SESSION_RACE => race_text(manager),
        _ => String::new(),
    }
}

/// `RaceManager::renderHUD` @ 0x14013f960: the lines at the middle of the screen.
pub fn manager_lines(manager: &RaceManager, cars: &[CarView], player: &PlayerState, results_displayed: bool) -> Vec<String> {
    let mut lines = Vec::new();
    if cars.is_empty() {
        return lines;
    }
    let session_type = manager.current_session.session_type;
    if session_type == SESSION_QUALIFY && cars[0].is_in_pit && manager.has_completed_flag(0) {
        let waiting = manager.cars_race_status.iter().filter(|status| !manager.has_completed_flag(status.car)).count();
        lines.push(format!("Qualify finished waiting for {waiting} cars"));
    }
    if session_type != SESSION_RACE || !manager.is_race_over(0, cars) || results_displayed || player.is_black_flagged {
        return lines;
    }
    let place = manager.car_leaderboard_position(0, cars);
    let suffix = match place {
        1 => "st",
        2 => "nd",
        3 => "rd",
        _ => "th",
    };
    lines.push(format!("Race : {place}{suffix} place"));
    lines.push("Session Over".to_string());
    lines
}

/// The penalty messages and their own timers.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PenaltyMessages {
    /// `LapInvalidatorAvatar::lastCurrentLapValid`
    pub last_current_lap_valid: bool,
    /// `LapInvalidatorAvatar::invalidLapMessageTime`, s
    pub invalid_lap_message_time: f32,
    /// `messages.ini [PENALTY] LOCK_CONTROL` is not 0
    pub show_lock_message: bool,
}

impl Default for PenaltyMessages {
    fn default() -> PenaltyMessages {
        PenaltyMessages { last_current_lap_valid: true, invalid_lap_message_time: 0.0, show_lock_message: true }
    }
}

impl PenaltyMessages {
    /// One frame of `LapInvalidatorAvatar::renderHUD` @ 0x140111c00,
    /// `LockControlsTimeAvatar::renderHUD` @ 0x14009dc50 and
    /// `DriveThroughPenaltyAvatar::renderHUD` @ 0x140090b50: the messages, each two lines.
    pub fn frame(&mut self, dt: f32, session_type: i32, player: &PlayerState, mph: bool) -> Vec<[String; 2]> {
        let mut out = Vec::new();
        if player.penalty_time > 0.0 {
            let seconds = player.penalty_time as i32;
            let first = if player.min_speed_clear_disabled { format!("PENALTY, SLOW DOWN FOR {seconds} SECONDS") } else { format!("PENALTY, SLOW DOWN FOR {seconds} SECONDS OR SLOW DOWN TO {}", if mph { "21 MPH" } else { "35 KMH" }) };
            out.push([first, "BEFORE THE END OF CURRENT LAP".to_string()]);
        } else if matches!(session_type, SESSION_PRACTICE | SESSION_QUALIFY | SESSION_HOTLAP) {
            if self.last_current_lap_valid && !player.current_lap_valid {
                self.invalid_lap_message_time = 5.0;
            }
            if self.invalid_lap_message_time > 0.0 {
                out.push(["CUT DETECTED".to_string(), "LAP WILL NOT COUNT".to_string()]);
                self.invalid_lap_message_time -= dt;
            }
            self.last_current_lap_valid = player.current_lap_valid;
        }
        if self.show_lock_message && player.lock_controls_time > 0.0 {
            out.push(["PENALTY".to_string(), format!("CONTROLS ARE LOCKED FOR {} SECONDS", (player.lock_controls_time * 0.001) as i32)]);
        }
        if player.pit_penalty_laps > 0 && !player.is_black_flagged {
            out.push(["DRIVE THROUGH PENALTY".to_string(), format!("{} LAPS REMAINING", player.pit_penalty_laps as i32 - 1)]);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn times_read_as_the_game_writes_them() {
        assert_eq!(time_to_wstring(0), "0:00");
        assert_eq!(time_to_wstring(9_999), "0:09");
        assert_eq!(time_to_wstring(425_000), "7:05");
        assert_eq!(time_to_wstring(3_900_000), "1h05m");
    }
}
