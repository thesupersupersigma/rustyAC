// SPDX-License-Identifier: GPL-3.0-or-later

//! The port side of the GUI oracle's game modes (Task 45): the mode's state is
//! `rustyac_physics::race::time_attack::TimeAttack` (whose `hud_tick` moves the HUD's timers),
//! drawn by `rustyac_gui::time_attack::TimeAttackHud`.

use std::cell::RefCell;

use rustyac_gui::gui::Gui;
use rustyac_gui::painter::Painter;
use rustyac_gui::time_attack::{TimeAttackHud, TimeAttackLink};
use rustyac_physics::race::time_attack::{TaFrame, TaMode, TimeAttack};

use crate::gui_port_hud::HudPort;

struct Mode {
    ta: TimeAttack,
    hud: Option<TimeAttackHud>,
    track_sectors: usize,
}

thread_local! {
    static MODE: RefCell<Option<Mode>> = const { RefCell::new(None) };
}

/// `ta.create` makes the HUD there, as the game's constructor makes its textures and font.
pub fn call(gui: &mut Gui, painter: &mut dyn Painter, what: &str, number: f32) -> Result<(), String> {
    MODE.with(|m| {
        let mut m = m.borrow_mut();
        if what == "ta.create" {
            let sectors = number as usize;
            let mut ta = TimeAttack::new(Default::default(), 1.0);
            ta.sectors = vec![30.0; sectors];
            *m = Some(Mode { ta, hud: Some(TimeAttackHud::new(gui, painter)), track_sectors: sectors });
            return Ok(());
        }
        let Some(Mode { ta, .. }) = m.as_mut() else { return Err("no time attack (ta.create first)".to_string()) };
        let name = what.trim_start_matches("ta.");
        crate::gui_ac_modes::member(name).ok_or_else(|| format!("the port side knows no time attack member {what:?}"))?;
        match name {
            "mode" => ta.mode = [TaMode::Over, TaMode::On, TaMode::Still][number as usize],
            "sectorIndex" => ta.sector_index = number as usize,
            "timeLeft" => ta.time_left = number,
            "points" => ta.points = number,
            "carDamage" => ta.car_damage = number,
            "scoreLabelTimer" => ta.score_label_timer = number,
            "penaltyPoints" => ta.penalty_points_accumulated = number,
            "penaltyTimer" => ta.penalty_timer = number,
            "gainedPoints" => ta.last_gained_points = number,
            "gainedSeconds" => ta.last_gained_seconds = number,
            _ => ta.last_car_damage = number,
        }
        Ok(())
    })
}

/// What the end screen reads of the mode.
pub fn sync(gui: &mut Gui) {
    MODE.with(|m| {
        if let Some(m) = m.borrow().as_ref() {
            let link = gui.sim.time_attack.unwrap_or_default();
            gui.sim.time_attack = Some(TimeAttackLink { points: m.ta.points, ..link });
        }
    });
}

pub fn render_hud(hud: &HudPort, gui: &mut Gui, painter: &mut dyn Painter, dt: f32) {
    MODE.with(|m| {
        let mut m = m.borrow_mut();
        let Some(Mode { ta, hud: drawn, track_sectors }) = m.as_mut() else { return };
        let input = &hud.input;
        let frame = TaFrame { track_sectors: *track_sectors, esc_open: input.esc_menu_visible, paused: input.pause_menu_visible, replay: input.in_replay, penalty_time: input.cars.first().map_or(0.0, |car| car.penalty_time) as f32, ..TaFrame::default() };
        ta.hud_tick(dt, &frame);
        let link = TimeAttackLink {
            available: *track_sectors != 0 && !ta.sectors.is_empty(),
            mode: ta.mode as i32,
            time_left: ta.time_left,
            points: ta.points,
            checkpoint_shown: ta.checkpoint_shown,
            last_gained_points: ta.last_gained_points,
            last_gained_seconds: ta.last_gained_seconds,
            penalty_points_accumulated: ta.penalty_points_accumulated,
        };
        gui.sim.time_attack = Some(link);
        if let Some(drawn) = drawn {
            drawn.render_hud(gui, painter, &link);
        }
    });
}

pub fn dump(out: &mut String) {
    MODE.with(|m| {
        if let Some(Mode { ta, .. }) = m.borrow().as_ref() {
            out.push_str(&crate::gui_ac_modes::line(ta.mode as i32, [ta.score_label_timer, ta.penalty_points_accumulated, ta.penalty_timer, ta.last_gained_points, ta.last_gained_seconds, ta.last_car_damage]));
        }
    });
}
