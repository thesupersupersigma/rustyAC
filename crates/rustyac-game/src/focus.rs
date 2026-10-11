// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! The focused car (`Sim::focusedCarIndex`): the car the camera is on, whose mirror, dashboard
//! and sounds are the full ones. The commands that move it (`Sim::onKeyDown` 0x14019a940: with
//! Ctrl, the keys of `controls.ini [NEXT_CAR]`, `[PREVIOUS_CAR]`, `[PLAYER_CAR]`, by default
//! numpad 3, 1 and 2; F4 goes back to the player's car), which car "next" is, and the fade to
//! black the change goes through (`ACCameraManager::fadeIn`, the camera manager's handler of
//! `Game::evOnPostGui` 0x140032770).

use crate::view::CarView;

/// `Sim::nextCar` 0x14019a740 / `Sim::previousCar` 0x14019e180: the car just ahead of (behind)
/// the focused one along the lap, by `physicsState.normalizedSplinePosition` of every car. Not
/// the next number: with nobody ahead the answer is car 0.
#[allow(clippy::neg_cmp_op_on_partial_ord)] // a distance that is not a number takes the wrapped branch, as in the game
pub fn neighbour_car(world: &[CarView], current: usize, ahead: bool) -> usize {
    if world.len() <= 1 {
        return current;
    }
    let reference = world[current].lap.position;
    let (mut best, mut index) = (1.0f32, 0usize);
    for (i, view) in world.iter().enumerate() {
        if i == current {
            continue;
        }
        let position = view.lap.position;
        let mut d = if ahead { position - reference } else { reference - position };
        if !(d >= 0.0) {
            d = if ahead { (1.0 - reference) + position } else { (1.0 - position) + reference };
        }
        if d < best {
            best = d;
            index = i;
        }
    }
    index
}

/// The three commands of `Sim::onKeyDown` 0x14019a940 that move the focus (Ctrl + numpad 3, 1
/// and 2 unless `controls.ini` says otherwise).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FocusCommand {
    Next,
    Previous,
    Player,
}

/// The focused car (`Sim::focusedCarIndex`) and the fade a change of it goes through
/// (`ACCameraManager::fadeIn` with `Sim::cameraFadeTime`): the picture goes black over the fade
/// time, the focus changes at the black frame, and the picture comes back over the same time.
pub struct Focus {
    pub index: usize,
    /// `system/cfg/fades.ini [CHANGE_CAMERA_FADE] MS`, seconds (0 when the file is not there)
    pub fade_time: f32,
    /// seconds since the fade began
    pub fade: Option<f32>,
    pub pending: Option<FocusCommand>,
}

impl Focus {
    pub fn new(index: usize) -> Focus {
        let mut fade_time = 0.0f32;
        if let Some(ac) = rustyac_content::install::ac_root() {
            let file = ac.join("system/cfg/fades.ini");
            if file.is_file() {
                if let Ok(ini) = rustyac_physics::data::ini::IniReader::load(&file) {
                    fade_time = ini.get_float("CHANGE_CAMERA_FADE", "MS").unwrap_or(0.0) * f32::from_bits(0x3a83_126f);
                }
            }
        }
        Focus { index, fade_time, fade: None, pending: None }
    }

    fn apply(&mut self, command: FocusCommand, world: &[CarView], names: &[String]) {
        self.index = match command {
            FocusCommand::Next => neighbour_car(world, self.index.min(world.len() - 1), true),
            FocusCommand::Previous => neighbour_car(world, self.index.min(world.len() - 1), false),
            FocusCommand::Player => 0,
        };
        // (SystemMessage::addCarMessage: the game names the car on the screen)
        println!("FOCUSED CAR: {} ({})", self.index, names.get(self.index).map_or("", String::as_str));
    }

    /// A command: `ACCameraManager::registerToFadeInEvent` (a change that still waits for its
    /// black frame happens at once), then the fade begins.
    pub fn command(&mut self, command: FocusCommand, world: &[CarView], names: &[String]) {
        if world.len() <= 1 {
            return;
        }
        if let Some(waiting) = self.pending.take() {
            self.apply(waiting, world, names);
        }
        self.pending = Some(command);
        self.fade = Some(0.0);
    }

    /// The fade as the frame's picture shows it: (`fadeTime`, `fadeTotalTime`).
    pub fn for_picture(&self) -> Option<(f32, f32)> {
        self.fade.map(|t| (t, self.fade_time))
    }

    /// The camera manager's handler of `Game::evOnPostGui` (the lambda 0x140032770), after the
    /// frame is drawn: from the black frame on (`fadeTime >= fadeTotalTime`) the waiting change
    /// happens, so the next frame is the new car's; then the frame's time is added.
    #[allow(clippy::neg_cmp_op_on_partial_ord)] // `comiss; jae`: a time that is not a number is past the black frame
    pub fn post_gui(&mut self, dt: f32, world: &[CarView], names: &[String]) {
        let Some(t) = self.fade else { return };
        if t >= self.fade_time * 2.0 {
            if let Some(command) = self.pending.take() {
                self.apply(command, world, names);
            }
            self.fade = None;
            return;
        }
        if !(t < self.fade_time) {
            if let Some(command) = self.pending.take() {
                self.apply(command, world, names);
            }
        }
        self.fade = Some(t + dt);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn world(positions: &[f32]) -> Vec<CarView> {
        positions
            .iter()
            .map(|p| {
                let mut view = CarView::default();
                view.lap.position = *p;
                view
            })
            .collect()
    }

    #[test]
    fn next_is_the_car_ahead_on_the_lap_not_the_next_number() {
        // car 0 at 50 % of the lap, car 1 just behind it, car 2 just ahead, car 3 far ahead
        let w = world(&[0.50, 0.48, 0.52, 0.90]);
        assert_eq!(neighbour_car(&w, 0, true), 2);
        assert_eq!(neighbour_car(&w, 0, false), 1);
        // from the last car on the lap, "ahead" goes over the line to the first
        assert_eq!(neighbour_car(&w, 3, true), 1);
        assert_eq!(neighbour_car(&w, 1, false), 3);
        // one car: it stays
        assert_eq!(neighbour_car(&world(&[0.3]), 0, true), 0);
        // two cars at the very same place: the distance is 0, which is below 1
        assert_eq!(neighbour_car(&world(&[0.3, 0.3]), 0, true), 1);
    }

    #[test]
    fn the_focus_changes_at_the_black_frame_of_the_fade() {
        let w = world(&[0.50, 0.48, 0.52]);
        let names = vec![String::new(); 3];
        let mut focus = Focus { index: 0, fade_time: 0.1, fade: None, pending: None };
        focus.command(FocusCommand::Next, &w, &names);
        let dt = 1.0 / 60.0;
        let mut changed_at = None;
        for frame in 0..20 {
            let shown = focus.for_picture();
            let before = focus.index;
            focus.post_gui(dt, &w, &names);
            if focus.index != before {
                // the frame that showed the old car was at or past the black point
                let (t, total) = shown.unwrap();
                assert!(t >= total && t < total * 2.0, "changed at t = {t}");
                changed_at = Some(frame);
            }
        }
        assert_eq!(focus.index, 2);
        assert_eq!(changed_at, Some(6));
        assert_eq!(focus.fade, None);
        // a second command while the first still waits: the first happens at once
        focus.command(FocusCommand::Previous, &w, &names);
        focus.command(FocusCommand::Player, &w, &names);
        assert_eq!(focus.index, 0, "from car 2 the car behind is car 0");
        // no fade time (fades.ini unreadable): the change happens in the frame of the key
        let mut focus = Focus { index: 0, fade_time: 0.0, fade: None, pending: None };
        focus.command(FocusCommand::Next, &w, &names);
        focus.post_gui(dt, &w, &names);
        assert_eq!((focus.index, focus.fade), (2, None));
    }
}
