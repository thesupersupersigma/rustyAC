// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni). Not covered by rustyAC's licenses — see LICENSING.md.

//! The AI line of a track and a car's position along it. (Being ported.)

use std::path::Path;

use super::Track;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct AiSpline {}

impl AiSpline {
    pub fn length(&self) -> f32 {
        0.0
    }

    pub fn point_count(&self) -> usize {
        0
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SplineLocator {}

pub fn init_ai_spline(_track: &mut Track, _ai: &Path, _data: &Path, _messages: &mut Vec<String>) -> Result<(), String> {
    Ok(())
}
