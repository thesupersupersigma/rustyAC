// SPDX-License-Identifier: GPL-3.0-or-later

//! Task 28: the referee. AC's race rules as its main thread keeps them: sessions, the start,
//! lap lists and leaderboard, when a race is over, results, flags, and the files a session
//! leaves behind. No platform code: the desktop game and the browser use the same one.
//! (What the physics thread does of the rules, the cut rule, the penalties and the jump
//! start, is in the car's step: `car::chassis` and `track::timing`.)

pub mod flags;
pub mod drift;
pub mod game_mode;
pub mod hud;
pub mod manager;
pub mod output;
pub mod pitstop;
pub mod referee;
pub mod results;
pub mod shm;
pub mod time_attack;
pub mod timing;
pub mod toy;

pub use manager::{convert_remote_session, Action, CarView, FadeJob, OnSessionEndEvent, RaceManager, RemoteSession, Session, SessionResult, SessionsIni};
pub use pitstop::{PitAction, PitApply, PitStop, QuickMenu, QuickMenuCar, QuickMenuSettings};
pub use referee::{PitAvatar, Referee, RefereeSetup, World};
pub use timing::{Lap, LapDb, LeaderboardEntry, RaceTimingServices};
