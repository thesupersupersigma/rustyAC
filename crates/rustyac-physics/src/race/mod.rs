// SPDX-License-Identifier: GPL-3.0-or-later

//! Task 28: the referee. AC's race rules as its main thread keeps them: sessions, the start,
//! lap lists and leaderboard, when a race is over, results, flags, and the files a session
//! leaves behind. No platform code: the desktop game and the browser use the same one.
//! (What the physics thread does of the rules, the cut rule, the penalties and the jump
//! start, is in the car's step: `car::chassis` and `track::timing`.)

pub mod flags;
pub mod hud;
pub mod manager;
pub mod output;
pub mod referee;
pub mod results;
pub mod timing;

pub use manager::{Action, CarView, FadeJob, OnSessionEndEvent, RaceManager, Session, SessionResult, SessionsIni};
pub use referee::{Referee, RefereeSetup, World};
pub use timing::{Lap, LapDb, LeaderboardEntry, RaceTimingServices};
