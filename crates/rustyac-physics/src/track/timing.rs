// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni). Not covered by rustyAC's licenses — see LICENSING.md.

//! Timing lines, a car's lap timer and the lap's validity; the spawn points. (Being ported.)

use super::loader::HelperNode;
use super::Track;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct TimeLine {}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct TimeTransponder {}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct LapInvalidator {}

pub fn init_respawn_position_set(_track: &mut Track, _helpers: &[HelperNode], _set: &str) {}

pub fn init_time_lines(_track: &mut Track, _helpers: &[HelperNode], _messages: &mut Vec<String>) {}
