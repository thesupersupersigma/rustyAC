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

// DriftMode: `rustyac_physics::race::drift::DriftMode::hud` per frame, drawn by
// `rustyac_gui::drift::DriftHud`.

struct Drift {
    mode: rustyac_physics::race::drift::DriftMode,
    hud: rustyac_gui::drift::DriftHud,
    /// speed, points, instant, combo, bonus (the game side's car inputs)
    car: [f32; 5],
}

thread_local! {
    static DRIFT: RefCell<Option<Drift>> = const { RefCell::new(None) };
}

pub fn drift_call(painter: &mut dyn Painter, what: &str, number: f32) -> Result<(), String> {
    use rustyac_physics::race::drift::{DriftMode, DriftStatus};
    DRIFT.with(|d| {
        let mut d = d.borrow_mut();
        let name = what.trim_start_matches("dr.");
        if name == "create" {
            *d = Some(Drift { mode: DriftMode::new(Default::default()), hud: rustyac_gui::drift::DriftHud::new(painter), car: [0.0; 5] });
            return Ok(());
        }
        let Some(Drift { mode, car, .. }) = d.as_mut() else { return Err("no drift mode (dr.create first)".to_string()) };
        if name == "goal" {
            let name = ["BRONZE", "SILVER", "GOLD", "GOLDEN GLORY"].get(mode.base.conditions.len()).unwrap_or(&"MEDAL").to_string();
            mode.base.conditions.push(rustyac_physics::race::game_mode::Condition { kind: "POINTS".into(), name, objective: number as i32, achieved: false });
            return Ok(());
        }
        if let Some(i) = crate::gui_ac_modes::drift_input(name) {
            car[i] = number;
            return Ok(());
        }
        crate::gui_ac_modes::drift_member(name).ok_or_else(|| format!("the port side knows no drift member {what:?}"))?;
        match name {
            "mode" => mode.mode = [DriftStatus::Over, DriftStatus::Drifting, DriftStatus::Still][number as usize],
            "seconds" => mode.seconds_left = number,
            "level" => mode.current_level = number as i32,
            _ => mode.session_points = number,
        }
        Ok(())
    })
}

pub fn drift_sync(gui: &mut Gui) {
    DRIFT.with(|d| {
        if let Some(d) = d.borrow().as_ref() {
            gui.sim.drift = Some(rustyac_gui::drift::DriftLink { session_points: d.mode.session_points, drawn: d.mode.drawn });
        }
    });
}

pub fn drift_render_hud(hud: &mut HudPort, gui: &mut Gui, painter: &mut dyn Painter, dt: f32) {
    DRIFT.with(|d| {
        let mut d = d.borrow_mut();
        let Some(Drift { mode, hud: drawn, car }) = d.as_mut() else { return };
        let input = &hud.input;
        let [speed, drift_points, instant_drift, combo, bonus] = *car;
        let frame = rustyac_physics::race::drift::DriftFrame { drift_points, instant_drift, combo: combo as i32, bonus_on: bonus != 0.0, speed, esc_open: input.esc_menu_visible, paused: input.pause_menu_visible, replay: input.in_replay };
        let mut out = Default::default();
        let mut said = Vec::new();
        mode.drawn = mode.hud(dt, &frame, &mut |title, text| said.push((title.to_string(), text.to_string())), &mut out);
        if let Some(message) = &mut hud.message {
            for (title, text) in said {
                message.add_message(gui, &title, &text, rustyac_gui::hud::MESSAGE_DEFAULT);
            }
        }
        let link = rustyac_gui::drift::DriftLink { session_points: mode.session_points, drawn: mode.drawn };
        gui.sim.drift = Some(link);
        drawn.render_hud(gui, painter, &link);
    });
}

pub fn drift_dump(out: &mut String) {
    DRIFT.with(|d| {
        let d = d.borrow();
        let Some(Drift { mode: m, .. }) = d.as_ref() else { return };
        let text = |b: &rustyac_physics::race::drift::Bonus, unit: &str| if b.value == 0 { String::new() } else { format!("+{}{unit}", b.value) };
        out.push_str(&crate::gui_ac_modes::drift_line(
            m.mode as i32,
            [m.max_session_combo_count, m.level_up_time, m.old_points, m.seconds_left, m.session_points, m.current_points_per_level],
            [m.last_time_left, m.current_level, m.max_points, m.last_points_achieved, m.last_seconds_achieved],
            [(m.bonus_time.y, text(&m.bonus_time, "\"")), (m.bonus_points.y, text(&m.bonus_points, " pt"))],
        ));
    });
}

// DragMode: the HUD draws a `DragLink`, the strip's places from
// `rustyac_physics::race::drag::DragMode`'s spline (made by its constructor).

struct Drag {
    mode: rustyac_physics::race::drag::DragMode,
    hud: rustyac_gui::drag::DragHud,
    link: rustyac_gui::drag::DragLink,
    x: [f32; 2],
}

thread_local! {
    static DRAG: RefCell<Option<Drag>> = const { RefCell::new(None) };
}

struct NoHost;

impl rustyac_physics::race::drag::DragHost for NoHost {
    fn rand(&mut self) -> i32 {
        0
    }
    fn is_ai(&self, _car: usize) -> bool {
        false
    }
    fn ai_start_time(&mut self, _car: usize, _time: f64) {}
    fn gentle_stop(&mut self, _car: usize, _on: bool) {}
    fn go_to_spawn(&mut self, _car: usize) {}
    fn show_esc_menu(&mut self) {}
    fn fade_in(&mut self, _seconds: f32, _results: bool) -> bool {
        false
    }
}

pub fn drag_call(gui: &mut Gui, painter: &mut dyn Painter, what: &str, number: f32) -> Result<(), String> {
    DRAG.with(|d| {
        let mut d = d.borrow_mut();
        let name = what.trim_start_matches("dg.");
        if name == "create" {
            let l = crate::gui_ac_modes::DRAG_LENGTH;
            let mode = rustyac_physics::race::drag::DragMode::new(Default::default(), 0, [[0.0, 0.0, -2.0], [0.0, 0.0, 2.0]], [[l, 0.0, -3.0], [l, 0.0, 3.0]], &[[0.0; 3]; 2], 0.0, &mut NoHost);
            let link = rustyac_gui::drag::DragLink { status: 3, ..Default::default() };
            *d = Some(Drag { mode, hud: rustyac_gui::drag::DragHud::new(gui, painter), link, x: [0.0; 2] });
            return Ok(());
        }
        let Some(Drag { link, x, .. }) = d.as_mut() else { return Err("no drag mode (dg.create first)".to_string()) };
        if let Some(i) = ["x0", "x1"].iter().position(|n| *n == name) {
            x[i] = number;
            return Ok(());
        }
        let (car, _, _) = crate::gui_ac_modes::drag_member(name).ok_or_else(|| format!("the port side knows no drag member {what:?}"))?;
        let i = car.unwrap_or(0);
        match name.trim_end_matches(['0', '1']) {
            "status" => link.status = number as i32,
            "false" => link.false_start[i] = number != 0.0,
            "disq" => link.disqualified[i] = number != 0.0,
            "wins" => link.wins[i] = number as u32,
            "reaction" => link.reaction_time = number,
            "time" => link.player_time = number,
            _ => link.final_speed = number as i32,
        }
        Ok(())
    })
}

pub fn drag_sync(gui: &mut Gui) {
    DRAG.with(|d| {
        if let Some(d) = d.borrow().as_ref() {
            gui.sim.drag = Some(d.link);
        }
    });
}

pub fn drag_render_hud(gui: &mut Gui, painter: &mut dyn Painter) {
    DRAG.with(|d| {
        let mut d = d.borrow_mut();
        let Some(Drag { mode, hud, link, x }) = d.as_mut() else { return };
        // the game's playerPerc/aiPerc move only when renderDragGUI runs
        let hidden = gui.sim.in_replay || gui.sim.pause_menu.is_some_and(|p| gui.controls[p].visible);
        if link.status < 3 && !hidden {
            link.perc = x.map(|x| mode.spline.world_to_spline(&[x, 0.0, 0.0], -1));
        }
        gui.sim.drag = Some(*link);
        hud.render_hud(gui, painter, link);
    });
}

pub fn drag_dump(out: &mut String) {
    DRAG.with(|d| {
        let d = d.borrow();
        let Some(Drag { link, .. }) = d.as_ref() else { return };
        out.push_str(&crate::gui_ac_modes::drag_line(link.status, link.perc));
    });
}
