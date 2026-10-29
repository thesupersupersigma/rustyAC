// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! `AIDriver` (AIDriver.obj of ksPhysicsAC.lib, 0x3e8 bytes): the circuit driver.

// (the comparisons and the order of the operations are the machine code's)
#![allow(clippy::neg_cmp_op_on_partial_ord, clippy::assign_op_pattern, clippy::manual_range_contains, clippy::needless_range_loop, clippy::field_reassign_with_default, clippy::if_same_then_else, clippy::too_many_arguments)]

use std::collections::BTreeMap;
use std::path::Path;

use super::engineer;
use super::{ks_g_from_radius, ks_speed_from_radius, sat, AiStepInput, PidController, SessionInfo, RAND_UNIT, SESSION_PRACTICE, SESSION_QUALIFY, SESSION_RACE, SESSION_UNDEFINED};
use crate::car::{CarControls, ControlsProvider, CarControlsInput, RollingChassis, TractionType};
use crate::curve::Curve;
use crate::data::ini::IniReader;
use crate::math::{asinf, fdtest_inf_or_nan, sinf, sqrtf};
use crate::session::MsvcRand;
use crate::track::ai_line::{AiSlimPayload, AiSplineRecorder};
use crate::track::spline::{locate_on_spline, AiSpline, AiSplinePayload};
use crate::track::Track;
use crate::vecmath::Vec3f;

/// `SplineSelection`
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SplineSelection {
    FastLane = 0,
    Pits = 1,
}

/// `AIBrakePointData`
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AiBrakePointData {
    pub target_speed: f32,
    pub target_normalized: f32,
    pub apex_normalized: f32,
    pub speed_diff: f32,
    pub lateral_offset: f32,
    pub was_obstacle: bool,
}

/// `HumanizeData`
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct HumanizeData {
    pub min_time_between_mistakes_ms: f64,
    pub time_between_mistakes_ms: f64,
    pub brake_mistake_time: f32,
    pub traction_mistake: f32,
    pub next_brake_mistake: f64,
    pub next_traction_mistake: f64,
    pub tractio_mistake_severity: f32,
}

/// `AISinOffsetData`
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct AiSinOffsetData {
    pub phase: f64,
    pub phase_speed: f64,
    pub steer_offset: f32,
    pub last_final_offset: f32,
}

/// `AIVariation`
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct AiVariation {
    pub blend: f32,
    pub start_push: f32,
    pub target_push: f32,
    pub accum: f32,
    pub current_push: f32,
    pub variation_time: f32,
}

/// `AIOpponentData` (0x50 bytes)
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct AiOpponentData {
    pub dist_front: f32,
    pub dist_back: f32,
    pub lateral_offset: f32,
    pub is_engaged: bool,
    pub speed_ms: f32,
    pub tti: f32,
    pub space_left: f32,
    pub space_right: f32,
    pub rel_pos: Vec3f,
    pub rel_angle: f32,
    pub is_blocker: bool,
    pub is_under_attack: f32,
    pub rel_x_speed: f32,
    pub dist_to_pass: f32,
    /// `car`: the opponent's place in `PhysicsEngine::cars`
    pub car: usize,
    pub is_outside_track: bool,
}

/// `AIBrakeObstacle`
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AiBrakeObstacle {
    pub distance: f32,
    pub speed_ms: f32,
    /// `odata`: the index into `opponentData`
    pub odata: usize,
}

/// `AIKersArea`
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct AiKersArea {
    pub start: f32,
    pub end: f32,
    pub length: f32,
    pub time: f32,
}

/// `AIWingSetup`
#[derive(Clone, Debug, PartialEq)]
pub struct AiWingSetup {
    pub index: i32,
    /// The car's setup item `WING_n`
    pub item: usize,
    pub min: f32,
    pub max: f32,
}

/// `AISetup`
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AiSetup {
    /// `finalRatio`: the car's setup item `FINAL_RATIO`
    pub final_ratio: Option<usize>,
    pub ratios: Vec<f32>,
    pub last_setup_change_time: f64,
    pub wings: Vec<AiWingSetup>,
}

/// Where the game's files for a driver are (all read-only).
#[derive(Clone, Debug, Default)]
pub struct AiFiles<'a> {
    /// `Documents/Assetto Corsa/setups/<car>/<track>/ai_default.ini`, where it exists, else
    /// `system/setups/<car>/<track>/ai_default.ini` of the game's folder, where that exists
    pub ai_setup: Option<&'a Path>,
}

/// AC's `AIDriver`.
#[derive(Clone, Debug)]
pub struct AiDriver {
    pub straight_radius: f32,
    pub aggression: f32,
    pub steer_min_lookahead: f32,
    pub gas_brake_lookahead: f32,
    pub base_push: f32,
    pub brake_point: AiBrakePointData,
    pub steer_target: Vec3f,
    pub pid_steer: PidController,
    pub steer_gain: f32,
    pub spline_selection: SplineSelection,
    pub brake_hint_base: f32,
    pub brake_hint_live: f32,
    pub change_up_rpm: f32,
    pub change_dn_perc: f32,
    pub request_pit_stop: bool,
    pub next_car_modifier: f32,
    pub humanize: HumanizeData,
    pub dist_to_next_corner: f32,
    pub aero_hint: f32,
    pub tyres_hint: f32,
    pub understeer_factor: f32,
    pub prj_dn_rpm: f32,
    pub time_to_start: f64,
    pub kerb_modifier: f32,
    pub outside_offset: f32,
    pub mongolino_max_gas: f32,
    pub genome: [f32; 20],
    pub use_magic_forces: bool,
    pub understeer_hint: f32,
    pub live_offset: f32,
    pub offset_dist_from_corner: f32,
    pub ultra_grip: f32,
    pub last_gear_up_change_time: f64,
    pub last_gear_dn_change_time: f64,
    pub is_changing_up: bool,
    pub is_retiring: bool,
    pub sin_offset_data: AiSinOffsetData,
    pub reaction_time: f64,
    pub old_steer: f32,
    pub target_speed: f32,
    pub start_time: u32,
    pub avg_speed: f64,
    pub requested_lane: f32,
    pub dynamic_push: f32,
    pub step_count: i32,
    pub freq_counter: i32,
    pub is_reacting_slow: bool,
    /// `turbo` (`AITurboStrategy`)
    pub turbo_active: bool,
    pub turbo_qualify_hot_laps: i32,
    pub auto_brake_adjust: f32,
    pub is_steering_to_pit: bool,
    pub danger_push: f32,
    pub wing_variation: f32,
    pub kers_areas: Vec<AiKersArea>,
    pub kers_accum: f32,
    pub tyre_stop_min_value: f32,
    pub tyre_stop_min_value_vkm: [f32; 4],
    /// `currentSpline`: the pit lane's line (else the fast lane)
    pub current_spline_is_pit: bool,
    pub gas_request: f32,
    pub brake_request: f32,
    pub steer_request: f32,
    pub pit_lane_start_flag: bool,
    pub current_offset: f32,
    pub desired_offset: f32,
    pub current_payload: AiSplinePayload,
    pub laps_to_complete: i32,
    pub time_to_start_revving_at_start: f64,
    pub race_start_flag: bool,
    pub run_lap_counter: i32,
    pub opponent_data: Vec<AiOpponentData>,
    pub brake_obstacles: Vec<AiBrakeObstacle>,
    pub gas_apex_delay: f32,
    pub preferred_compound: i32,
    pub has_choosen_tyres: bool,
    /// `accidentStopCounter`: the constructor never writes it and nothing ever sets it (the
    /// collision handler is empty): 0 here
    pub accident_stop_counter: f32,
    /// `tyreStrategies`: `maxKm` by compound short name
    pub tyre_strategies: BTreeMap<String, f32>,
    pub current_session_info: SessionInfo,
    pub loc_npos: f32,
    pub current_pit_line_index: i32,
    pub spline_target_speed: f32,
    pub oversteer_tc_mult: f32,
    pub min_forward_sides: [f32; 2],
    pub is_engaged: bool,
    pub ai_variation: AiVariation,
    pub engaged_push: f32,
    pub use_abs: bool,
    pub ai_setup: AiSetup,
    pub last_drs_activation_time: f64,
    pub smooth_loads: [f32; 4],
    pub aggression_hint: f32,
    pub target_steer_final: f32,
    /// The console lines the game prints, for whoever wants them
    pub log: Vec<String>,
}

fn ks(rand: &mut MsvcRand) -> f32 {
    rand.next() as f32 * RAND_UNIT
}

impl AiDriver {
    /// `AIDriver::AIDriver` @ 0x140298c60 for `car`, whose track and session are set. Draws
    /// from `rand` as the game's constructor draws from `rand()`.
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    pub fn new(car: &mut RollingChassis, session: &SessionInfo, rand: &mut MsvcRand, files: &AiFiles<'_>) -> Result<AiDriver, String> {
        let track = car.track.clone().ok_or("an AI driver needs a track")?;
        track.init_ai()?;
        let recorder = track.ai().ok_or("the track has no AI line")?;
        let mut sin_offset_data = AiSinOffsetData::default();
        // AISinOffsetData::AISinOffsetData @ 0x14029abd0 (a member: before the body)
        sin_offset_data.phase = ((ks(rand)) * 10.0) as f64;
        sin_offset_data.phase_speed = ((ks(rand)) * 0.4 + 0.8) as f64;

        let mut ai = AiDriver {
            straight_radius: 250.0,
            aggression: 0.0,
            steer_min_lookahead: 12.0,
            gas_brake_lookahead: 10.0,
            base_push: 1.0,
            brake_point: AiBrakePointData { target_speed: -1.0, target_normalized: -1.0, apex_normalized: -1.0, speed_diff: 0.0, lateral_offset: 0.0, was_obstacle: false },
            steer_target: Vec3f::new(0.0, 0.0, 0.0),
            pid_steer: PidController::default(),
            steer_gain: 0.05,
            spline_selection: SplineSelection::FastLane,
            brake_hint_base: 1.0,
            brake_hint_live: 1.0,
            change_up_rpm: 6000.0,
            change_dn_perc: 0.8,
            request_pit_stop: false,
            next_car_modifier: 0.0,
            humanize: HumanizeData { min_time_between_mistakes_ms: 20000.0, time_between_mistakes_ms: 100000.0, ..HumanizeData::default() },
            dist_to_next_corner: 0.0,
            aero_hint: 1.0,
            tyres_hint: 1.0,
            understeer_factor: 0.0,
            prj_dn_rpm: 0.0,
            time_to_start: -1.0,
            kerb_modifier: 1.0,
            outside_offset: 0.0,
            mongolino_max_gas: 1.0,
            genome: [0.0; 20],
            use_magic_forces: true,
            understeer_hint: 1.0,
            live_offset: 0.0,
            offset_dist_from_corner: 0.0,
            ultra_grip: 1.0,
            last_gear_up_change_time: 0.0,
            last_gear_dn_change_time: 0.0,
            is_changing_up: false,
            is_retiring: false,
            sin_offset_data,
            reaction_time: 0.0,
            old_steer: 0.0,
            target_speed: 0.0,
            start_time: 0,
            avg_speed: 1000.0,
            requested_lane: -1.0,
            dynamic_push: 1.0,
            step_count: 100,
            freq_counter: 0,
            is_reacting_slow: false,
            turbo_active: false,
            turbo_qualify_hot_laps: -1,
            auto_brake_adjust: 1.0,
            is_steering_to_pit: false,
            danger_push: 1.0,
            wing_variation: 0.15,
            kers_areas: Vec::new(),
            kers_accum: 0.0,
            tyre_stop_min_value: 0.96,
            tyre_stop_min_value_vkm: [0.0; 4],
            current_spline_is_pit: false,
            gas_request: 0.0,
            brake_request: 0.0,
            steer_request: 0.0,
            pit_lane_start_flag: false,
            current_offset: 0.0,
            desired_offset: 0.0,
            current_payload: AiSplinePayload::default(),
            laps_to_complete: 10000,
            time_to_start_revving_at_start: 5000.0,
            race_start_flag: false,
            run_lap_counter: 0,
            opponent_data: Vec::new(),
            brake_obstacles: Vec::new(),
            gas_apex_delay: 0.0,
            preferred_compound: -1,
            has_choosen_tyres: false,
            accident_stop_counter: 0.0,
            tyre_strategies: BTreeMap::new(),
            current_session_info: SessionInfo { session_type: SESSION_UNDEFINED, start_time_ms: 0.0, time_secs: 0.0, laps: 0, index: 0 },
            loc_npos: 0.0,
            current_pit_line_index: 0,
            spline_target_speed: 0.0,
            oversteer_tc_mult: 1.0,
            min_forward_sides: [0.0; 2],
            is_engaged: false,
            ai_variation: AiVariation { blend: 0.0, start_push: 1.0, target_push: 1.0, accum: 0.0, current_push: 1.0, variation_time: 50.0 },
            engaged_push: 1.0,
            use_abs: false,
            ai_setup: AiSetup::default(),
            last_drs_activation_time: 0.0,
            smooth_loads: [0.0; 4],
            aggression_hint: 1.0,
            target_steer_final: 0.0,
            log: Vec::new(),
        };
        for gene in &mut ai.genome {
            *gene = ((ks(rand)) * 0.04 - 0.02) + 1.0;
        }
        // AIDriver::initSetup @ 0x14029e250: an AI setup file, where there is one
        if let Some(path) = files.ai_setup {
            let saved = IniReader::load(path)?;
            car.load_setup(&saved, "ai_default")?;
        }
        ai.tyre_stop_min_value = ((ks(rand)) * 0.02 - 0.01) + 0.96;
        ai.ai_variation.accum = (ks(rand)) * 10.0;
        ai.freq_counter = (car.physics_guid % 5) as i32;
        ai.time_to_start_revving_at_start = ((ks(rand)) * 5500.0 + 1500.0) as f64;

        let data = car.data_path.clone();
        let ini_path = data.join("ai.ini");
        let ini = if crate::data::exists(&ini_path) { IniReader::load(&ini_path)? } else { IniReader::default() };
        if ini.ready {
            let mut version = 1;
            if ini.has_section("HEADER") {
                version = ini.get_int("HEADER", "VERSION").unwrap_or(0);
            }
            ai.change_up_rpm = ini.get_int("GEARS", "UP").unwrap_or(0) as f32;
            if version > 3 {
                ai.change_dn_perc = ini.get_int("GEARS", "DOWN").unwrap_or(0) as f32 * 0.01;
            }
            let draw = ks(rand);
            if car.physics_guid != 0 {
                ai.change_dn_perc = ((draw * 0.15 - 0.075) + 1.0) * ai.change_dn_perc;
            }
            if ini.has_section("STEER") {
                ai.steer_gain = ini.get_float("STEER", "STEER_GAIN").unwrap_or(0.0);
            }
            if ini.has_section("LOOKAHEAD") {
                ai.steer_min_lookahead = ini.get_float("LOOKAHEAD", "BASE").unwrap_or(0.0);
                if version < 2 {
                    ai.gas_brake_lookahead = ini.get_float("LOOKAHEAD", "GAS_BRAKE_LOOKAHEAD").unwrap_or(0.0);
                }
            }
            if ini.has_section("UNDERSTEER") {
                ai.understeer_hint = ini.get_float("UNDERSTEER", "HINT").unwrap_or(0.0);
            }
            if ini.has_section("TYRES") {
                ai.tyres_hint = ini.get_float("TYRES", "HINT").unwrap_or(0.0);
            }
            ai.brake_hint_base = ini.get_float("PEDALS", "BRAKE_HINT").unwrap_or(0.0);
            if ai.brake_hint_base == 0.0 {
                ai.brake_hint_base = 1.0;
            }
            ai.brake_hint_live = ai.brake_hint_base;
            if version > 2 {
                ai.aero_hint = ini.get_float("PHYSICS_HINTS", "AERO_HINT").unwrap_or(0.0);
            }
            {
                let engine = car.drivetrain.as_ref().ok_or("an AI driver needs the car's drivetrain")?.engine();
                if engine.turbo_adjustable_from_cockpit() && !engine.has_turbo_controllers() {
                    ai.turbo_active = true;
                    ai.turbo_qualify_hot_laps = 1;
                }
            }
            if ini.has_section("BRAKE_BIAS") {
                if let Some(item) = car.setup_manager.get_setup_item("FRONT_BIAS") {
                    let value = ini.get_float("BRAKE_BIAS", "VALUE").unwrap_or(0.0);
                    let item = &mut car.setup_manager.items[item];
                    item.attached = true;
                    item.new_value = value;
                    ai.log.push(format!("AI car {} overriding Brake bias to {}", car.physics_guid, value));
                }
            }
            if ini.has_section("WING_VARIATION") {
                ai.wing_variation = ini.get_float("WING_VARIATION", "VALUE").unwrap_or(0.0);
            }
        }
        if ai.steer_min_lookahead == 0.0 {
            ai.steer_min_lookahead = 12.0;
        }
        ai.gas_brake_lookahead = 0.0;
        ai.pid_steer.p = 0.3;
        ai.pid_steer.i = 0.02;
        ai.pid_steer.d = 0.02;

        // AIDriver::initTyreStrategies @ 0x14029e600
        let tyres_path = data.join("ai_tyres.ini");
        if crate::data::exists(&tyres_path) {
            let strategies = IniReader::load(&tyres_path)?;
            if strategies.ready {
                for def in &car.tyres[0].compound_defs {
                    if strategies.has_section(&def.short_name) {
                        let km = strategies.get_float(&def.short_name, "MAX_KM").unwrap_or(0.0) * 1000.0;
                        ai.tyre_strategies.insert(def.short_name.clone(), km);
                    }
                }
            }
        }

        {
            let aids = car.aids.as_mut().ok_or("an AI driver needs the car's aids")?.base_mut();
            ai.use_abs = aids.abs.is_active && aids.abs.is_present;
            aids.abs.is_active = false;
            aids.traction_control.is_active = false;
        }
        car.auto_shifter.is_active = false;
        car.autoclutch.use_auto_on_start = true;
        car.autoclutch.use_auto_on_change = true;
        // Drivetrain::forceAutoCutoffTime(0)
        car.drivetrain.as_mut().unwrap().base_mut().auto_cut_off_time = 0.0;
        car.auto_blip.is_active = true;
        if let Some(brakes) = &mut car.brake_system {
            brakes.base_mut().brake_power_multiplier = 1.0;
        }
        ai.select_time_start_time(car, session, &track, rand);
        ai.ultra_grip = 1.2;
        let tyres_ini = IniReader::load(&data.join("tyres.ini"))?;
        if tyres_ini.has_section("COMPOUND_DEFAULT") {
            ai.preferred_compound = tyres_ini.get_int("COMPOUND_DEFAULT", "INDEX").unwrap_or(0);
        }

        // how much grip the car has, and how peaky its tyres are: the wheelspin limit
        let grip_curve = Curve::from_pairs(&[(0.7, 4.0), (1.0, 3.0), (1.3, 2.0), (1.55, 1.0), (2.0, 0.6)]);
        let mass = car.get_total_mass(true);
        let dy = car.tyres[2].get_dy((mass * 0.25) * 9.806);
        let by_grip = grip_curve.get_value(dy);
        let asy_curve = Curve::from_pairs(&[(0.0, 1.0), (0.2, 0.2), (1.0, 0.0)]);
        let asy = car.tyres[2].model_data.asy;
        let by_asy = asy_curve.get_value((1.0 - asy) * dy);
        let mult = by_asy * by_grip + 1.0;
        ai.oversteer_tc_mult = mult;
        let traction = car.drivetrain.as_ref().unwrap().base().traction_type;
        if traction != TractionType::Rwd {
            ai.oversteer_tc_mult = mult * 1.3;
        }

        // what of the setup the driver may change
        let setup_path = data.join("setup.ini");
        let setup = if crate::data::exists(&setup_path) { IniReader::load(&setup_path)? } else { IniReader::default() };
        if setup.ready {
            if setup.has_section("FINAL_GEAR_RATIO") {
                ai.ai_setup.final_ratio = car.setup_manager.get_setup_item("FINAL_RATIO");
                if let Some(item) = ai.ai_setup.final_ratio {
                    if car.setup_manager.items[item].attached {
                        let file = setup.get_string("FINAL_GEAR_RATIO", "RATIOS");
                        ai.ai_setup.ratios = crate::car::setup::load_gear_ratios(&data.join(&file)).map(|ratios| ratios.into_iter().map(|r| r.1).collect()).unwrap_or_default();
                    }
                }
            }
            for index in 0..10 {
                let section = format!("WING_{index}");
                if setup.has_section(&section) {
                    if let Some(item) = car.setup_manager.get_setup_item(&section) {
                        let max = setup.get_float(&section, "MAX").unwrap_or(0.0);
                        let min = setup.get_float(&section, "MIN").unwrap_or(0.0);
                        ai.ai_setup.wings.push(AiWingSetup { index, item, min, max });
                    }
                }
            }
        }

        // a slow gearbox gets a downshift profile of the driver's own
        if car.autoclutch.get_downshift_sequence_duration() > 0.5 {
            let dn = car.drivetrain.as_ref().unwrap().base().gear_dn_time;
            let mut profile = Curve::new();
            profile.add_value(0.0, 1.0);
            profile.add_value(0.01, 0.0);
            profile.add_value(dn as f32, 0.0);
            profile.add_value((dn * 2.5) as f32, 1.0);
            car.autoclutch.set_downshift_profile(&profile);
        }

        // the straights the KERS is spent on: the first ones that make half of all straights
        if let Some(kers) = &car.kers {
            let discharge = kers.get_discharge_time_s();
            let straights = &recorder.best_lap().straights;
            let mut total = 0.0f32;
            for straight in straights {
                total += straight.length;
            }
            let mut share = 0.0f32;
            let mut index = 0;
            loop {
                if straights.len() <= index {
                    break;
                }
                let straight = &straights[index];
                share += straight.length / total;
                ai.kers_areas.push(AiKersArea { start: straight.start, end: straight.end, length: straight.length, time: 0.0 });
                index += 1;
                if !(share < 0.5) {
                    break;
                }
            }
            let mut sum = 0.0f32;
            for area in &ai.kers_areas {
                sum += area.length;
            }
            for area in &mut ai.kers_areas {
                area.time = ((1.0 / sum) * area.length) * discharge;
            }
        }
        Ok(ai)
    }

    pub(super) fn spline<'a>(&self, recorder: &'a AiSplineRecorder) -> &'a AiSpline {
        if self.current_spline_is_pit {
            recorder.pit_lane()
        } else {
            recorder.best_lap()
        }
    }

    /// `AIDriver::selectTimeStartTime` @ 0x14029f7a0: also the handler of
    /// `Car::evOnForcedPositionCompleted`.
    pub fn select_time_start_time(&mut self, car: &mut RollingChassis, session: &SessionInfo, track: &Track, rand: &mut MsvcRand) {
        let kind = session.session_type;
        if kind >= 0 {
            if kind < 3 {
                self.generate_pitlane_wait(car, rand);
                self.spline_selection = SplineSelection::Pits;
                self.pit_lane_start_flag = true;
                self.current_spline_is_pit = true;
            } else if kind == SESSION_RACE && !self.request_pit_stop {
                self.spline_selection = SplineSelection::FastLane;
                self.time_to_start = -1.0;
                self.pit_lane_start_flag = false;
                self.current_spline_is_pit = false;
            }
        }
        self.update_current_spline_pos(car, track);
    }

    /// `AIDriver::generatePitlaneWait` @ 0x14029cd20
    fn generate_pitlane_wait(&mut self, car: &mut RollingChassis, rand: &mut MsvcRand) {
        if self.current_session_info.session_type == SESSION_RACE {
            let before = car.fuel;
            let mut laps = (self.current_session_info.laps.wrapping_sub(car.transponder.lap_count as i32)).wrapping_add(1);
            let filled = car.set_fuel_for_laps(laps, 1.2);
            if filled != laps {
                laps = filled;
            }
            let after = car.fuel;
            self.run_lap_counter = 0;
            let time = car.get_pitstop_time((after - before) as f32, true, true, false, true, true, rand);
            self.time_to_start = time.total as f64;
            self.spline_selection = SplineSelection::Pits;
            self.laps_to_complete = laps;
            self.current_spline_is_pit = true;
            return;
        }
        self.run_lap_counter = 0;
        self.time_to_start = ((ks(rand)) * 120.0) as f64;
        let laps = if !self.turbo_active || self.turbo_qualify_hot_laps < 1 { crate::car::drivetrain::cvttss2si((ks(rand)) * 5.0 + 3.0) } else { self.turbo_qualify_hot_laps + 2 };
        self.laps_to_complete = laps;
        if car.physics_guid == 0 {
            self.laps_to_complete = 10;
            self.time_to_start = 5.0;
        }
        car.set_fuel_for_laps(self.laps_to_complete + 3, 1.1);
    }

    /// `AIDriver::onNewSession` @ 0x14029f140: the handler of
    /// `PhysicsEngine::evOnNewSessionPhysics`.
    pub fn on_new_session(&mut self, car: &mut RollingChassis, info: &SessionInfo, rand: &mut MsvcRand) {
        self.humanize = HumanizeData { min_time_between_mistakes_ms: 20000.0, time_between_mistakes_ms: 100000.0, ..HumanizeData::default() };
        self.laps_to_complete = 10000;
        self.request_pit_stop = false;
        self.has_choosen_tyres = false;
        if info.session_type == SESSION_RACE {
            self.generate_pit_stop_strategy(car, info.laps, rand);
        }
        self.current_session_info = *info;
        if car.physics_guid != 0 {
            for wing in &self.ai_setup.wings {
                let draw = ks(rand);
                let current = car.setup_manager.items[wing.item].target.get(car);
                let item = &mut car.setup_manager.items[wing.item];
                let (max, min) = (wing.max, wing.min);
                let value = ((max - min) * (draw * 2.0 - 1.0)) * self.wing_variation + current;
                let value = if value <= max {
                    if value < min {
                        min
                    } else {
                        value
                    }
                } else {
                    max
                };
                item.new_value = crate::car::drivetrain::cvttss2si(value) as f32;
                item.attached = true;
            }
        }
    }

    /// `AIDriver::generatePitStopStrategy` @ 0x14029cb70
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    fn generate_pit_stop_strategy(&mut self, car: &mut RollingChassis, laps: i32, rand: &mut MsvcRand) {
        let per_lap = car.get_fuel_per_lap();
        let max_laps = car.max_fuel / per_lap as f64;
        if max_laps < laps as f64 {
            if (((laps as f64) / max_laps) as i32) + 1 < 3 {
                let draw = ks(rand);
                let stint = crate::car::drivetrain::cvttss2si(draw * (laps as f32 * 0.33333334 + 0.5) + (laps / 3) as f32);
                car.set_fuel_for_laps(stint + 1, 1.2);
                self.laps_to_complete = stint;
            } else {
                let max = car.max_fuel as f32;
                car.fuel = max as f64;
                car.requested_fuel = max;
                self.laps_to_complete = 10000;
            }
            return;
        }
        car.set_fuel_for_laps(laps + 1, 1.2);
        self.laps_to_complete = 10000;
    }

    /// `AIDriver::acquireControls` @ 0x14029baa0: the car's controls of this step.
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    pub fn acquire(&mut self, car: &mut RollingChassis, dt: f32) {
        let Some(track) = car.track.clone() else { return };
        let Some(recorder) = track.ai() else { return };
        let mut input = std::mem::take(&mut car.ai_input);
        let time = car.physics_time;

        {
            let stability = &mut car.aids.as_mut().unwrap().base_mut().stability_control;
            stability.use_beta = true;
            stability.gain = if time - car.last_collision_time <= 1000.0 { 0.0 } else { 2.0 };
        }
        self.step_push_variations(dt, &mut input.rand);
        self.step_setup(car, time);

        // the stint: fuel for two laps is the limit
        let mut request = false;
        if self.current_session_info.session_type == SESSION_RACE {
            if !(self.request_pit_stop || car.spline_locator_data.npos <= 0.5 || (self.current_session_info.laps.wrapping_sub(car.transponder.lap_count as i32)).wrapping_sub(1) < 2) {
                if self.laps_to_complete < 2 {
                    request = true;
                } else {
                    let laps = car.fuel_lap_evaluator.fuel_laps(car.fuel);
                    if !(laps < 0.0 || 2.0 <= laps) {
                        request = true;
                    }
                }
            }
        } else {
            let laps = car.fuel_lap_evaluator.fuel_laps(car.fuel);
            if !(self.request_pit_stop || (1 < self.laps_to_complete && (laps < 0.0 || 2.0 <= laps)) || car.spline_locator_data.npos <= 0.5) {
                request = true;
            }
        }
        if request {
            self.request_pit_stop = true;
        }
        if car.fuel < 1.0 {
            car.fuel = 1.0;
            self.request_pit_stop = true;
        }
        self.is_reacting_slow = time - car.last_collision_with_car_time < 1000.0;
        self.brake_hint_live = recorder.get_brake_hint_at_spline_pos(self.loc_npos) * self.brake_hint_base;
        self.step_humanizer(car, dt, time, &mut input.rand);
        self.step_spline_selection(car, &track, recorder, &input.session, &mut input.rand);
        self.step_pit_lane_mode(car, &input, time);
        self.step_sin_offset(car, dt);

        let session = input.session;
        if self.freq_counter < 4 {
            self.freq_counter += 1;
        } else {
            let dt5 = dt * 5.0;
            self.danger_push = 1.0;
            if !(!self.is_engaged && self.current_offset.abs() <= 0.1) {
                if let Some((left, right)) = recorder.get_danger_at_spline_pos(self.loc_npos) {
                    let worst = if right <= left { left } else { right };
                    self.danger_push = 1.0 - worst * 0.2;
                }
            }
            self.dist_to_next_corner = self.get_distance_to_next_corner(car, recorder, 25.0);
            self.update_current_spline_pos(car, &track);
            self.step_cars_awareness2(car, recorder, &input, dt5);
            self.step_steer(car, recorder, &session, time, dt5);
            self.step_gears(car, &session, time);
            self.step_gas_brake(car, recorder, &mut input, time, dt5);
            self.step_turbo(car, &session);
            if car.drs().is_some_and(|drs| drs.is_present) {
                self.step_drs(car, time);
            }
            if car.kers.is_none() {
                car.controls.kers = false;
            } else {
                self.step_kers(car);
            }
            self.avg_speed = car.speed as f64 * 0.001 + self.avg_speed * 0.999;
            if self.avg_speed < 8.0 && !car.is_retired && !car.is_in_pits() && (session.session_type != SESSION_RACE || car.transponder.lap_count < session.laps as u32) {
                car.is_retired = true;
            }
            if car.is_retired {
                let m = car.pit_position.m;
                car.force_rotation(&Vec3f::new(-m[2][0], -m[2][1], -m[2][2]));
                car.force_position_with(&Vec3f::new(m[3][0], m[3][1], m[3][2]), true);
                self.forced_positions(car, &session, &track, &mut input.rand);
                self.brake_request = 1.0;
                self.gas_request = 0.0;
                self.steer_request = 0.0;
                self.current_spline_is_pit = true;
            }
            if session.session_type == SESSION_RACE && car.drivetrain.as_ref().unwrap().engine().push_to_pass().is_some_and(|p2p| p2p.enabled) {
                self.step_p2p(car, recorder, &session);
            }
            self.freq_counter = 0;
        }
        if 0.0 < self.time_to_start {
            self.time_to_start -= dt as f64;
        }
        if self.start_time == 0 {
            self.start_time = time as i64 as u32;
        }
        if self.spline(recorder).point_count() == 0 {
            self.brake_request = 0.0;
            self.steer_request = 0.0;
            self.gas_request = 0.0;
            car.controls.gear_up = false;
            car.controls.gear_dn = false;
        }
        car.controls.steer = sat(self.steer_request, -1.0, 1.0);
        car.controls.gas = sat(self.gas_request, 0.0, 1.0);
        car.controls.brake = sat(self.brake_request, 0.0, 1.0);
        car.controls.clutch = 1.0;
        self.step_superhuman(car, recorder, dt);
        car.is_requesting_pit_stop = self.request_pit_stop;
        car.ai_laps_to_complete = self.laps_to_complete;
        car.ai_input = input;
    }

    /// `Car::evOnForcedPositionCompleted`, for every `Car::forcePosition` since the driver
    /// was last told: its handler is `selectTimeStartTime`.
    fn forced_positions(&mut self, car: &mut RollingChassis, session: &SessionInfo, track: &Track, rand: &mut MsvcRand) {
        while car.forced_position_events > 0 {
            car.forced_position_events -= 1;
            self.select_time_start_time(car, session, track, rand);
        }
    }

    /// `AIDriver::stepPushVariations` @ 0x1402a1f20
    fn step_push_variations(&mut self, dt: f32, rand: &mut MsvcRand) {
        let v = &mut self.ai_variation;
        if v.variation_time < v.accum {
            v.start_push = v.current_push;
            v.target_push = ((ks(rand)) * 0.08 - 0.04) + self.base_push;
            let draw = ks(rand);
            v.accum = 0.0;
            v.variation_time = draw * 570.0 + 30.0;
        }
        let start = v.start_push;
        let accum = v.accum;
        let push = (v.target_push - start) * (accum / v.variation_time) + start;
        v.current_push = sat(push, 0.0, 1.0);
        v.accum = accum + dt;
    }

    /// `AIDriver::stepSetup` @ 0x1402a2020: a longer final ratio when the engine sits on its
    /// limiter in top gear.
    fn step_setup(&mut self, car: &mut RollingChassis, time: f64) {
        if 10000.0 <= time - self.ai_setup.last_setup_change_time {
            if let Some(item) = self.ai_setup.final_ratio {
                let drivetrain = car.drivetrain.as_ref().unwrap();
                let base = drivetrain.base();
                if base.current_gear as i64 == base.gears.len() as i64 - 1 && drivetrain.engine().is_limiter_on() {
                    for &ratio in &self.ai_setup.ratios {
                        if ratio < base.final_ratio {
                            if 0.0 < ratio {
                                car.setup_manager.items[item].new_value = ratio;
                                self.ai_setup.last_setup_change_time = time;
                            }
                            break;
                        }
                    }
                }
            }
            if let Some(brakes) = &mut car.brake_system {
                brakes.base_mut().brake_power_multiplier = 1.0;
            }
        }
    }

    /// `AIDriver::stepHumanizer` @ 0x1402a19e0
    fn step_humanizer(&mut self, car: &mut RollingChassis, dt: f32, time: f64, rand: &mut MsvcRand) {
        if car.physics_guid == 0 {
            return;
        }
        let h = &mut self.humanize;
        if h.next_brake_mistake < time {
            let draw = ks(rand);
            h.next_brake_mistake = 0.0;
            h.brake_mistake_time = draw * 2.0;
        }
        if h.next_traction_mistake < time {
            h.traction_mistake = 0.0;
            h.next_traction_mistake = 0.0;
        }
        if h.next_brake_mistake == 0.0 {
            h.next_brake_mistake = (ks(rand)) as f64 * h.time_between_mistakes_ms + time + h.min_time_between_mistakes_ms;
        }
        if h.next_traction_mistake == 0.0 {
            h.next_traction_mistake = (ks(rand)) as f64 * h.time_between_mistakes_ms + time + h.min_time_between_mistakes_ms;
            h.tractio_mistake_severity = (ks(rand)) * 0.5 + 1.0;
        }
        if 0.0 <= h.traction_mistake {
            h.traction_mistake -= dt;
            car.aids.as_mut().unwrap().base_mut().stability_control.gain = 0.0;
        }
        if 0.0 <= h.brake_mistake_time {
            h.brake_mistake_time -= dt;
        }
    }

    /// `AIDriver::stepSplineSelection` @ 0x1402a2240: onto the pit lane's line for a stop,
    /// the stop itself, and back onto the fast lane.
    fn step_spline_selection(&mut self, car: &mut RollingChassis, track: &Track, recorder: &AiSplineRecorder, session: &SessionInfo, rand: &mut MsvcRand) {
        if self.spline_selection == SplineSelection::FastLane {
            self.current_spline_is_pit = false;
            if self.request_pit_stop {
                let pit = recorder.pit_lane();
                let p = car.core.get_position(car.body);
                let first = pit.spline.point_at(0);
                let dy = first[1] - p.y;
                let dx = first[0] - p.x;
                let dz = first[2] - p.z;
                if (dy * dy + dx * dx) + dz * dz < 25.0 {
                    self.spline_selection = SplineSelection::Pits;
                    self.current_spline_is_pit = true;
                    return;
                }
            }
            self.current_spline_is_pit = false;
        } else {
            self.current_offset = 0.0;
            self.current_spline_is_pit = true;
            if self.request_pit_stop {
                let pit = &recorder.pit_lane().spline;
                let m = car.pit_position.m;
                let box_pos = [m[3][0], m[3][1], m[3][2]];
                let index = pit.closest_point_index(&box_pos);
                let box_n = pit.world_to_spline(&box_pos, index as i32);
                let p = car.core.get_position(car.body);
                let p = [p.x, p.y, p.z];
                let index = pit.closest_point_index(&p);
                let car_n = pit.world_to_spline(&p, index as i32);
                let length = recorder.pit_lane().length();
                if length * (box_n - car_n) < 20.0 {
                    self.is_steering_to_pit = true;
                }
                if box_n < car_n {
                    // the stop: the car is put into its box, repaired and refuelled
                    car.force_rotation(&Vec3f::new(-m[2][0], -m[2][1], -m[2][2]));
                    car.force_position_with(&Vec3f::new(m[3][0], m[3][1], m[3][2]), false);
                    self.forced_positions(car, session, track, rand);
                    car.set_damage_level(0.0);
                    car.reset_suspension_damage_level();
                    self.generate_pitlane_wait(car, rand);
                    self.request_pit_stop = false;
                    self.is_steering_to_pit = false;
                    for wheel in 0..4 {
                        let compound = car.tyres[wheel].current_compound_index;
                        car.set_tyre_compound(wheel, compound);
                    }
                    return;
                }
            }
            if self.loc_npos <= 0.0 || self.loc_npos < recorder.pit_lane_spline_attach_point {
                self.current_spline_is_pit = true;
            } else {
                self.brake_point.target_speed = -1.0;
                self.spline_selection = SplineSelection::FastLane;
                self.current_spline_is_pit = false;
                car.reset_spline_locator();
            }
        }
    }

    /// `AIDriver::stepPitLaneMode` @ 0x1402a1d00: waiting in the box while a car comes down
    /// the pit lane, and not going out when the session is nearly over.
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    fn step_pit_lane_mode(&mut self, car: &RollingChassis, input: &AiStepInput, time: f64) {
        if !(self.spline_selection == SplineSelection::Pits && 0.0 < self.time_to_start && self.time_to_start < 1.0) {
            return;
        }
        let session = &input.session;
        if self.current_session_info.session_type != SESSION_RACE && (session.time_secs * 1000.0 + session.start_time_ms) - ((car.transponder.best_lap as f32 * 1.2) as f64) < time {
            self.time_to_start = 100.0;
        }
        let me = &input.cars[input.me];
        for (index, other) in input.cars.iter().enumerate() {
            if index == input.me {
                continue;
            }
            if !(other.speed * 3.6 <= 2.0) && other.is_in_pit_lane {
                // Car::getBackDistanceFromCar @ 0x140270640
                let mut back = me.npos - other.npos;
                if back < 0.0 {
                    back += 1.0;
                }
                let distance = car.fast_lane_length() * back;
                if !(10.0 <= distance / other.speed) {
                    self.time_to_start += 1.0;
                }
            }
        }
    }

    /// `AIDriver::stepSinOffset` @ 0x1402a2170: the slow weave on the straights.
    fn step_sin_offset(&mut self, car: &RollingChassis, dt: f32) {
        let s = &mut self.sin_offset_data;
        s.phase = (dt * 0.2) as f64 + s.phase;
        let sides = self.current_payload.sides;
        let wave = sinf(s.phase_speed as f32 * s.phase as f32);
        let by_speed = sat(car.speed * 0.2 - 1.0, 0.0, 1.0);
        s.steer_offset = ((sides[1] + sides[0]) * 0.1) * (by_speed * wave);
    }

    /// `AIDriver::getDistanceToNextCorner` @ 0x14029d250
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    fn get_distance_to_next_corner(&self, car: &RollingChassis, recorder: &AiSplineRecorder, step: f32) -> f32 {
        let spline = self.spline(recorder);
        let mut distance = 0.0f32;
        let mut n = self.loc_npos;
        let length = spline.length();
        loop {
            let half_width = car.car_half_width;
            let hint = recorder.get_hint_at_spline_pos(n);
            let radius = spline.get_corner_radius_at(n, 15.0, 0.0, half_width);
            if radius * hint <= self.straight_radius {
                return distance;
            }
            distance += step;
            n = spline.spline.wrap_position(step / length + n);
            if !(distance < 500.0) {
                return 500.0;
            }
        }
    }

    /// `AIDriver::updateCurrentSplinePos` @ 0x1402a3200
    fn update_current_spline_pos(&mut self, car: &RollingChassis, track: &Track) {
        let Some(recorder) = track.ai() else { return };
        if self.spline_selection == SplineSelection::FastLane {
            self.loc_npos = car.spline_locator_data.npos;
        } else {
            let p = car.core.get_position(car.body);
            self.loc_npos = locate_on_spline(self.spline(recorder), &[p.x, p.y, p.z], &mut self.current_pit_line_index, None);
        }
        self.current_payload = self.spline(recorder).payload_at_position(self.loc_npos);
    }

    /// `AIDriver::getCornerRadiusHint` @ 0x14029d0b0
    fn get_corner_radius_hint(&self, recorder: &AiSplineRecorder, n: f32, distance: f32, offset: f32, half_width: f32) -> f32 {
        let hint = recorder.get_hint_at_spline_pos(n);
        let radius = self.spline(recorder).get_corner_radius_at(n, distance, offset, half_width);
        radius * hint
    }

    /// `AIDriver::getLateralPush` @ 0x14029dd10: how much of the grip the driver uses.
    pub fn get_lateral_push(&self, car: &RollingChassis) -> f32 {
        let pits = if self.spline_selection == SplineSelection::Pits { 0.92f32 } else { 1.0 };
        let gene = crate::car::drivetrain::cvttss2si(self.loc_npos * 20.0).clamp(0, 0x14);
        if car.physics_guid == 0 {
            return ((self.dynamic_push * self.base_push) * pits) * self.engaged_push;
        }
        // (the genome has 20 genes; gene 20 is the member after it: `useMagicForces` and
        // three bytes of padding, a number near 0 the driver would stop for. The game only
        // gets there at npos = 1.0 exactly.)
        let gene = if gene < 20 { self.genome[gene as usize] } else { f32::from_bits(self.use_magic_forces as u32) };
        let push = ((((self.ai_variation.current_push * self.dynamic_push) * pits) * self.engaged_push) * self.danger_push) * gene;
        sat(push, 0.0, 1.0)
    }

    /// `AIDriver::getOutsideOffset` @ 0x14029ddd0
    pub fn get_outside_offset(&self, car: &RollingChassis, recorder: &AiSplineRecorder) -> f32 {
        if self.spline_selection == SplineSelection::FastLane {
            let payload = self.spline(recorder).payload_at_position(self.loc_npos);
            let side = if payload.direction <= 0.0 { -1.0f32 } else { 1.0 };
            (car.spline_locator_data.lateral_offset - (self.current_offset + self.sin_offset_data.last_final_offset)) * side
        } else {
            0.0
        }
    }

    /// `AIDriver::stepSteer` @ 0x1402a2630
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    fn step_steer(&mut self, car: &mut RollingChassis, recorder: &AiSplineRecorder, session: &SessionInfo, time: f64, dt: f32) {
        if car.speed * 3.6 < 5.0 {
            self.pid_steer.reset();
        }
        let lookahead = if self.spline_selection == SplineSelection::FastLane || !self.pit_lane_start_flag {
            sat(car.speed * 0.018, 0.0, 1.0) * self.steer_min_lookahead + 3.0
        } else {
            4.0
        };
        let mut ahead = AiSplinePayload::default();
        let mut target = self.get_steer_at_distance(car, recorder, lookahead, &mut ahead, dt);
        let p = self.steer_gain * 0.27;
        self.target_steer_final = target;
        self.pid_steer.p = p;
        let mut skip = false;
        if self.spline_selection == SplineSelection::Pits {
            if !self.pit_lane_start_flag {
                skip = true;
            } else if target.abs() < 1.0 {
                let m = car.core.get_world_matrix(car.body).m;
                if 0.85 < (ahead.forward_vector[1] * m[2][1] + ahead.forward_vector[0] * m[2][0]) + ahead.forward_vector[2] * m[2][2] {
                    self.pit_lane_start_flag = false;
                }
            }
        }
        if !skip && self.pit_lane_start_flag && self.spline_selection != SplineSelection::Pits {
            self.pit_lane_start_flag = false;
        }
        if time <= session.start_time_ms + self.reaction_time || 0.0 < self.time_to_start {
            target = 0.0;
            self.steer_request = 0.0;
        } else {
            target = self.pid_steer.eval(0.0, target, dt);
            if fdtest_inf_or_nan(target) {
                target = 0.0;
            }
            let (mut hi, mut lo) = (0.9f32, -0.9f32);
            target = sat(target, -0.9, 0.9);
            let mut slip_angle = car.tyres[0].status.slip_angle_rad;
            if car.tyres[0].status.load < car.tyres[1].status.load {
                slip_angle = car.tyres[1].status.slip_angle_rad;
            }
            if 30.0 < car.speed * 3.6 {
                let inv = 1.0 / car.steer_lock;
                lo = ((slip_angle * 57.29578 - 10.0) * 12.0) * inv + self.old_steer;
                hi = ((slip_angle * 57.29578 + 10.0) * 12.0) * inv + self.old_steer;
            }
            let request = self.steer_request;
            if 0.4 < (target - request).abs() {
                if target <= request {
                    target = request - 0.4;
                } else {
                    target = request + 0.4;
                }
            }
            if self.is_reacting_slow {
                target = (target - self.old_steer) * 0.1 + self.old_steer;
            }
            self.steer_request = if hi < target {
                hi
            } else if lo <= target {
                target
            } else {
                lo
            };
            target = self.steer_request;
        }
        self.old_steer = target;
    }

    /// `AIDriver::getSteerAtDistance` @ 0x14029de70
    fn get_steer_at_distance(&mut self, car: &RollingChassis, recorder: &AiSplineRecorder, distance: f32, ahead: &mut AiSplinePayload, dt: f32) -> f32 {
        let spline = self.spline(recorder);
        let length = spline.length();
        let here = self.loc_npos;
        let n = spline.spline.wrap_position(here + distance / length);
        let mut payload = spline.payload_at_position(n);
        *ahead = payload;
        let weave = sat(ahead.dist_from_corner * 0.0033333334, 0.0, 1.0) * self.sin_offset_data.steer_offset;
        self.offset_dist_from_corner = ahead.dist_from_corner;
        let margin = car.car_half_width * 1.5;
        let hi = payload.sides[1] - margin;
        let lo = margin - payload.sides[0];
        let mut offset = sat(weave, lo, hi);
        if self.spline_selection == SplineSelection::Pits || self.is_engaged {
            offset = 0.0;
        }
        let last = self.sin_offset_data.last_final_offset;
        let k = sat(dt * 7.0, 0.0, 1.0);
        let last = (offset - last) * k + last;
        self.sin_offset_data.last_final_offset = last;
        self.live_offset = last + self.current_offset;
        if self.is_engaged {
            if let Some((left, right)) = recorder.get_danger_at_spline_pos(here) {
                payload.sides[0] -= left;
                payload.sides[1] -= right;
            }
        }
        let mut point = spline.get_point_with_offset(n, self.live_offset, car.car_half_width, &payload);
        if self.is_steering_to_pit {
            let m = car.pit_position.m;
            point = [m[3][0], m[3][1], m[3][2]];
        }
        let local = car.core.world_to_local(car.body, &Vec3f::new(point[0], point[1], point[2]));
        let flat = sqrtf(local.z * local.z + local.x * local.x);
        let mut x = local.x;
        if flat != 0.0 {
            x = local.x / flat;
        }
        self.steer_target = Vec3f::new(point[0], point[1], point[2]);
        asinf(x) * 7.0
    }

    /// `AIDriver::stepGears` @ 0x1402a1710
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    fn step_gears(&mut self, car: &mut RollingChassis, session: &SessionInfo, time: f64) {
        if time <= session.start_time_ms + self.reaction_time {
            car.controls.gear_up = false;
            car.controls.gear_dn = false;
            return;
        }
        let drivetrain = car.drivetrain.as_ref().unwrap();
        let base = drivetrain.base();
        let velocity = base.engine.velocity;
        let slip = engineer::get_driving_tyres_slip(car);
        let (a, b) = if base.traction_type == TractionType::Fwd { (0, 1) } else { (3, 2) };
        let mut slip_ratio = car.tyres[a].status.slip_ratio;
        if slip_ratio < car.tyres[b].status.slip_ratio {
            slip_ratio = car.tyres[b].status.slip_ratio;
        }
        let rpm = ((velocity * 0.15915507152579872) * 60.0) as i32 as f32;
        let mut up = false;
        if self.change_up_rpm < rpm
            && !car.controls.gear_up
            && base.cut_off <= 0.0
            && !drivetrain.is_changing_gear()
            && (car.speed < 1.0 || slip_ratio < 1.0)
            && (0.99 < car.controls.clutch || base.current_gear == 1)
            && 1000.0 < time - self.last_gear_dn_change_time
            && 1000.0 < time - self.last_gear_up_change_time
        {
            up = true;
        }
        if up {
            car.controls.gear_up = true;
            car.controls.gear_dn = false;
            self.is_changing_up = true;
            self.last_gear_up_change_time = time;
        } else {
            car.controls.gear_up = false;
            let projected = drivetrain.project_rpm_at_downshift();
            self.prj_dn_rpm = projected;
            let wanted = !(projected <= 0.0 || self.change_dn_perc * self.change_up_rpm <= projected);
            if drivetrain.is_changing_gear() || car.controls.clutch <= 0.98 || !wanted || base.current_gear < 3 || 1.7 <= slip || time - self.last_gear_up_change_time <= 1000.0 {
                car.controls.gear_dn = false;
            } else {
                car.controls.gear_dn = true;
                self.is_changing_up = false;
                self.last_gear_dn_change_time = time;
            }
        }
    }

    /// `AIDriver::stepGasBrakeStart` @ 0x1402a1580: before the green light.
    fn step_gas_brake_start(&mut self, car: &RollingChassis, recorder: &AiSplineRecorder, session: &SessionInfo, time: f64, rand: &mut MsvcRand) {
        let to_start = session.start_time_ms - time;
        self.gas_request = 0.0;
        if session.session_type == SESSION_RACE && -1000.0 < to_start {
            if to_start < self.time_to_start_revving_at_start {
                let rpm = car.drivetrain.as_ref().unwrap().get_engine_rpm();
                let gas = (self.change_up_rpm - rpm) * 0.001 + self.gas_request;
                self.gas_request = sat(gas, 0.0, 1.0);
                self.race_start_flag = true;
            }
            if to_start + self.reaction_time < 80.0 {
                self.gas_request = 1.0;
            }
            if to_start < 1000.0 && 500.0 < to_start {
                self.reaction_time = ((ks(rand)) * 350.0 + 50.0) as f64;
            }
        }
        self.brake_request = 0.85;
        let point = self.spline(recorder).spline.spline_to_world(self.loc_npos);
        let local = car.core.world_to_local(car.body, &Vec3f::new(point[0], point[1], point[2]));
        self.current_offset = local.x;
        if self.spline_selection == SplineSelection::Pits {
            self.current_offset = 0.0;
        }
    }

    /// `AIDriver::updateTyres` @ 0x1402a32f0: the compound, once per session.
    fn update_tyres(&mut self, car: &mut RollingChassis, recorder: &AiSplineRecorder, session: &SessionInfo, rand: &mut MsvcRand) {
        if self.has_choosen_tyres {
            return;
        }
        let compound = if session.session_type == SESSION_PRACTICE {
            let found = engineer::find_tyre_compound(car, engineer::CompoundChoice::Random, rand);
            if car.physics_guid == 0 {
                self.preferred_compound
            } else {
                found
            }
        } else if session.session_type == SESSION_QUALIFY {
            self.find_tyre_for_qualify(car, recorder)
        } else if self.preferred_compound == -1 {
            engineer::find_tyre_compound(car, engineer::CompoundChoice::Fastest, rand)
        } else {
            self.preferred_compound
        };
        for wheel in 0..4 {
            if car.tyres[wheel].current_compound_index != compound {
                car.set_tyre_compound(wheel, compound);
                if let Some(def) = usize::try_from(compound).ok().and_then(|c| car.tyres[wheel].compound_defs.get(c)) {
                    self.tyre_stop_min_value_vkm[wheel] = get_vkm_for_mult(&def.model_data.wear_curve, self.tyre_stop_min_value);
                }
            }
        }
        self.has_choosen_tyres = true;
    }

    /// `AIDriver::findTyreForQualify` @ 0x14029c9f0
    fn find_tyre_for_qualify(&self, car: &RollingChassis, recorder: &AiSplineRecorder) -> i32 {
        let length = recorder.best_lap().length();
        let mut winner = -1i32;
        let mut best = 0.0f32;
        for def in &car.tyres[0].compound_defs {
            // (the game's map makes an entry for a compound without a strategy: a
            // `TyreCompoundStrategy` starts with `maxKm` 100000, node maker @ 0x1402974e0)
            let max_km = self.tyre_strategies.get(&def.short_name).copied().unwrap_or(100_000.0);
            let dy = engineer::get_compound_dy(def, 3000.0);
            if length * 2.5 < max_km && best < dy {
                winner = def.index as i32;
                best = dy;
            }
        }
        winner
    }

    /// `AIDriver::stepGasBrake` @ 0x1402a08e0: the pedals.
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    fn step_gas_brake(&mut self, car: &mut RollingChassis, recorder: &AiSplineRecorder, input: &mut AiStepInput, time: f64, dt: f32) {
        let _ = dt;
        let session = input.session;
        let profile = engineer::get_acceleration_profile(car);
        let spline = self.spline(recorder);
        let length = spline.length();
        let ahead_n = spline.spline.wrap_position(self.gas_brake_lookahead / length + self.loc_npos);
        let ahead = spline.payload_at_position(ahead_n);
        let speed = car.speed;
        let radius = self.get_corner_radius_hint(recorder, ahead_n, 15.0, self.current_offset, car.car_half_width);
        for wheel in 0..4 {
            self.smooth_loads[wheel] = (car.tyres[wheel].status.load - self.smooth_loads[wheel]) * 0.1 + self.smooth_loads[wheel];
        }
        let lift = car.aero.as_ref().map_or(0.0, |aero| aero.base().get_current_lift_kg());
        let mass = car.get_total_mass(true);
        let grip = engineer::eval_lateral_grip(car, speed, mass, self.aero_hint, Some(&self.smooth_loads), lift);
        let mut target = ks_speed_from_radius(radius, (grip * ahead.grip) * self.tyres_hint, ahead.camber);
        self.target_speed = target;
        let push = self.get_lateral_push(car);
        target -= (1.0 - push) * 22.222223;
        let hint = recorder.get_max_speed_hint_at_spline_pos(self.loc_npos);
        if 0.0 < hint {
            let limit = hint * 0.2777778;
            if !(target < limit) {
                target = limit;
            }
        }
        let outside = self.get_outside_offset(car, recorder);
        self.outside_offset = outside;
        if 0.0 < outside && self.spline_selection == SplineSelection::FastLane {
            let k = sat((self.target_steer_final * 0.6666667).abs(), 0.0, 1.0);
            target = (speed / (outside * 0.055 + 1.0) - target) * k + target;
        }
        self.spline_target_speed = target;
        let t = &car.tyres;
        let mut rear = if t[2].status.nd_slip < t[3].status.nd_slip { t[3].status.nd_slip } else { t[2].status.nd_slip };
        let mut front = if t[0].status.nd_slip < t[1].status.nd_slip { t[1].status.nd_slip } else { t[0].status.nd_slip };
        if t[0].status.load <= 0.0 && t[1].status.load <= 0.0 {
            front = 1.0;
        }
        if t[2].status.load <= 0.0 && t[3].status.load <= 0.0 {
            rear = 1.0;
        }
        let worst = if rear <= front { front } else { rear };
        if ahead.is_pitlane && !(target <= 22.222223) {
            target = 22.222223;
        }
        if self.pit_lane_start_flag {
            target = ((1.0 - self.steer_request.abs()) * 40.0 + 20.0) * 0.2777778;
        }
        self.target_speed = target;
        if time <= session.start_time_ms + self.reaction_time || 0.0 < self.time_to_start {
            car.is_retired = false;
            self.avg_speed = 1000.0;
            self.step_gas_brake_start(car, recorder, &session, time, &mut input.rand);
            self.update_tyres(car, recorder, &session, &mut input.rand);
            return;
        }
        let mut brake_max = engineer::get_optimal_brake(car) * 0.99;
        if 0.0 < self.humanize.brake_mistake_time && !self.use_abs {
            brake_max *= 2.0;
        }
        let mut brake = sat(self.get_la_braking(car, recorder), 0.0, 1.0) * brake_max;
        self.brake_request = brake;
        let la_brake = brake;
        let torque_to_grip = engineer::get_torque_to_grip_ratio(car);
        if 0.0 < self.brake_point.target_speed && self.brake_point.target_speed < target {
            target = self.brake_point.target_speed;
        }
        self.understeer_factor = front - rear;
        if !(car.speed * 3.6 <= 30.0 || worst * self.understeer_hint <= 1.0 || self.understeer_factor <= 0.0 || target <= speed) {
            let k = sat(self.understeer_factor, 0.0, 1.0);
            let k = sat(1.0 - (k * 0.2) * self.understeer_hint, 0.0, 1.0);
            target = k * car.speed;
        } else {
            self.understeer_factor = 0.0;
        }
        let cap = speed + 15.0;
        if !(target <= cap) {
            target = cap;
        }
        self.target_speed = target;
        let mut gas = sat(((1.0 / torque_to_grip) * 0.6) * (target - speed) + profile, 0.0, 1.0);
        if 0.0 < self.brake_point.target_speed {
            gas = 0.0;
        }
        if 0.0 < brake {
            gas = sat(gas - brake * 4.0, 0.0, 1.0);
        }
        let follow = self.next_car_modifier;
        if follow != 0.0 {
            if gas <= 0.0 || 0.01 <= brake {
                gas = 0.0;
                let by_car = sat(follow, 0.0, 1.0) * brake_max;
                brake = if by_car <= la_brake { la_brake } else { by_car };
            } else {
                gas = sat(gas - follow, 0.0, 1.0);
            }
        }
        if self.spline_selection == SplineSelection::FastLane {
            let lateral = car.spline_locator_data.lateral_offset;
            let width = car.car_half_width * 2.0;
            if lateral + width < -ahead.sides[0] || ahead.sides[1] < lateral - width {
                // off the track: slowly back, and wait for a gap
                let m = car.core.get_world_matrix(car.body).m;
                let along = (-m[2][1] * -ahead.forward_vector[1] + -m[2][0] * -ahead.forward_vector[0]) + -m[2][2] * -ahead.forward_vector[2];
                let along = if along <= 0.0 { 0.0 } else { along };
                let limit = along * 10.0 + 20.0;
                if !(target <= limit) {
                    target = limit;
                }
                let lateral = car.spline_locator_data.lateral_offset;
                let out = if 0.0 <= lateral { (lateral - car.car_half_width * 2.0) - ahead.sides[1] } else { car.car_half_width * 2.0 - (lateral + ahead.sides[0]) };
                if out < 12.0 && self.get_closest_following_car_tti(input) < 5.0 {
                    target = 0.0;
                    self.avg_speed = 20.0;
                }
                self.target_speed = target;
                if speed <= target {
                    brake = 0.0;
                    gas = 0.2;
                } else {
                    brake = 1.0;
                    gas = 0.0;
                }
            }
        }
        {
            let drivetrain = car.drivetrain.as_ref().unwrap();
            let rpm = drivetrain.get_engine_rpm();
            let over = !(rpm < drivetrain.engine().rpm_damage_threshold() * 0.99);
            if over {
                gas = 0.5;
            }
            let mult = if (over || 0.0 < gas) && brake == 0.0 && !self.is_engaged { f32::from_bits(0x3f80_20c5) } else { self.ultra_grip };
            for tyre in &mut car.tyres {
                tyre.ai_mult = mult;
            }
        }
        {
            let drivetrain = car.drivetrain.as_ref().unwrap();
            if !self.race_start_flag && drivetrain.base().auto_cut_off_time <= 0.0 && drivetrain.is_changing_gear() && self.is_changing_up {
                gas = 0.0;
            }
        }
        if !(car.speed * 3.6 <= 10.0 || car.autoclutch.clutch_sequence.is_done || 0.9 <= car.controls.clutch) {
            let clutch = car.controls.clutch;
            if clutch <= gas {
                gas = clutch;
            }
        }
        self.is_retiring = false;
        if car.drivetrain.as_ref().unwrap().engine().base().life_left as f32 <= 0.0 {
            // a blown engine: off the line and stop
            gas = 0.0;
            brake = 0.3;
            if car.speed * 3.6 < 50.0 {
                if ahead.sides[1] <= ahead.sides[0] {
                    self.current_offset = ahead.sides[1] + 4.0;
                } else {
                    self.current_offset = -ahead.sides[0] - 4.0;
                }
                brake = if 0.1 <= self.steer_request.abs() { 0.0 } else { 0.2 };
                self.is_retiring = true;
            }
        }
        for suspension in &car.suspensions {
            if 0.3 < suspension.get_damage() {
                self.is_retiring = true;
            }
        }
        self.brake_request = brake;
        self.mongolino_max_gas = if 0.9 <= self.base_push { 1.0 } else { 1.0 - (0.9 - self.base_push) * 2.0 };
        if !fdtest_inf_or_nan(gas) {
            self.gas_request = if self.mongolino_max_gas <= gas { self.mongolino_max_gas } else { gas };
        }
        if 0.0 < self.humanize.traction_mistake {
            let more = self.humanize.tractio_mistake_severity * self.gas_request;
            self.gas_request = if 1.0 < more {
                1.0
            } else if 0.0 <= more {
                more
            } else {
                0.0
            };
        }
    }

    /// `AIDriver::getClosestFollowingCarTTI` @ 0x14029cf70
    fn get_closest_following_car_tti(&self, input: &AiStepInput) -> f32 {
        let me = &input.cars[input.me];
        let mut closest = 100000.0f32;
        for (index, other) in input.cars.iter().enumerate() {
            if index == input.me {
                continue;
            }
            let mut back = me.npos - other.npos;
            if back < 0.0 {
                back += 1.0;
            }
            let tti = (back * me.spline_length) / other.speed;
            if !(closest <= tti) {
                closest = tti;
            }
        }
        closest
    }

    /// `AIDriver::getLABraking` @ 0x14029d650: looks down the line in 4 m steps, slowing an
    /// imagined car as hard as it could brake, for the first place it would be too fast
    /// (a corner, or a car in the way); the answer is how hard to brake now.
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    fn get_la_braking(&mut self, car: &RollingChassis, recorder: &AiSplineRecorder) -> f32 {
        let spline = self.spline(recorder);
        let mass = car.get_total_mass(true);
        let speed = car.speed;
        let mut slim = AiSlimPayload { camber: self.current_payload.camber, grade: self.current_payload.grade, is_pitlane: false, grip: self.current_payload.grip };
        let mut n = self.loc_npos;
        let mut distance = 0.0f32;
        let push_all = self.get_lateral_push(car);
        let step_n = 4.0 / spline.length();
        let inv_mass = 1.0 / mass;
        let max_brake = engineer::get_max_braking_force(car) * (1.0 / mass);
        let mut radius = self.get_corner_radius_hint(recorder, n, 15.0, self.current_offset, 0.0);
        if self.brake_point.was_obstacle || self.brake_point.lateral_offset != self.current_offset || self.brake_point.target_speed <= 0.0 {
            self.brake_point.target_speed = -1.0;
        }
        let push = self.get_lateral_push(car);
        let mut v = speed;
        let mut none = true;
        if speed > 0.0 {
            loop {
                if 300.0 <= distance {
                    break;
                }
                let grip = (engineer::eval_lateral_grip(car, v, mass, self.aero_hint, None, -1.0) * slim.grip) * self.tyres_hint;
                let corner = ks_speed_from_radius(radius, grip, slim.camber);
                let limit = if !slim.is_pitlane {
                    let (half, pushed) = (corner * 0.5, corner - (1.0 - push) * 22.222223);
                    if pushed <= half {
                        half
                    } else {
                        pushed
                    }
                } else {
                    let pushed = corner - (1.0 - self.get_lateral_push(car)) * 22.222223;
                    if pushed <= 22.222223 {
                        pushed
                    } else {
                        22.222223
                    }
                };
                if limit < v && (self.brake_point.target_speed == -1.0 || limit <= self.brake_point.target_speed) {
                    let now = engineer::eval_lateral_grip(car, speed, mass, self.aero_hint, None, -1.0);
                    let now = ks_speed_from_radius(radius, (now * slim.grip) * self.tyres_hint, slim.camber);
                    let (half, pushed) = (now * 0.5, now - (1.0 - push) * 22.222223);
                    let mut now = if pushed <= half { half } else { pushed };
                    if slim.is_pitlane && !(now <= 22.222223) {
                        now = 22.222223;
                    }
                    if now < speed {
                        if limit <= now {
                            self.brake_point.target_speed = now;
                            self.brake_point.speed_diff = speed - now;
                        } else {
                            self.brake_point.target_speed = limit;
                            self.brake_point.speed_diff = v - limit;
                        }
                        self.brake_point.target_normalized = n;
                        self.brake_point.lateral_offset = self.current_offset;
                        self.brake_point.was_obstacle = false;
                    }
                }
                for obstacle in &self.brake_obstacles {
                    if (obstacle.distance < distance || obstacle.distance < 4.0) && obstacle.speed_ms < v && (self.brake_point.target_speed == -1.0 || obstacle.speed_ms < self.brake_point.target_speed) {
                        self.brake_point.target_speed = obstacle.speed_ms;
                        self.brake_point.target_normalized = n;
                        self.brake_point.was_obstacle = true;
                        self.brake_point.speed_diff = v - obstacle.speed_ms;
                        if let Some(data) = self.opponent_data.get_mut(obstacle.odata) {
                            data.is_blocker = true;
                        }
                        self.brake_point.lateral_offset = self.current_offset;
                    }
                }
                n = spline.spline.wrap_position(step_n + n);
                let g = ks_g_from_radius(radius, v, slim.camber);
                let available = engineer::eval_available_brake(car, v, g, car.get_total_mass(true)) * slim.grip;
                let decel = if grip <= g {
                    engineer::project_wings_drag(car, v) * inv_mass
                } else {
                    let tyres = (push * if available <= grip { available } else { grip }) * 9.806;
                    let drag = engineer::project_wings_drag(car, v);
                    (drag * inv_mass + if tyres <= max_brake { tyres } else { max_brake }) * self.brake_hint_live
                };
                distance += 4.0;
                let total = push_all * decel + slim.grade * 9.806;
                let total = if total <= 0.0 { 0.0 } else { total };
                v -= (4.0 / v) * total;
                spline.get_slim_payload_at(n, &mut slim);
                let offset = self.current_offset;
                let hint = recorder.get_hint_at_spline_pos(n);
                radius = spline.get_corner_radius_at(n, 15.0, offset, 0.0) * hint;
                if !(0.0 < v) {
                    break;
                }
            }
            none = distance < 4.0;
        }
        if none {
            self.brake_point.target_speed = -1.0;
        }
        let mut reached = true;
        if self.brake_point.target_speed != -1.0 && self.brake_point.target_speed <= car.speed && self.loc_npos <= self.brake_point.target_normalized {
            reached = false;
        }
        if reached {
            self.brake_point.target_speed = -1.0;
            0.0
        } else {
            (car.speed - self.brake_point.target_speed) * 0.5
        }
    }

    /// `AIDriver::stepTurbo` @ 0x1402a30c0: the boost knob by session.
    fn step_turbo(&mut self, car: &mut RollingChassis, session: &SessionInfo) {
        if !self.turbo_active {
            return;
        }
        let trunc = |x: f32| crate::car::drivetrain::cvttss2si(x * 10.0) as f32 * 0.1;
        let engine = car.drivetrain.as_mut().unwrap().engine_mut();
        let safe = engine.get_safe_turbo_level();
        let mut level = 1.0f32;
        match session.session_type {
            SESSION_PRACTICE => level = trunc(safe * 0.8),
            SESSION_QUALIFY => {
                let laps = self.run_lap_counter;
                if laps == 0 {
                    level = crate::car::drivetrain::cvttss2si((safe * 0.5) * 10.0) as f32 * 0.1;
                }
                if (0 < laps && laps <= self.turbo_qualify_hot_laps) || (laps == 0 && 0.8 < self.loc_npos) {
                    level = 1.0;
                }
                if laps > self.turbo_qualify_hot_laps {
                    level = trunc(safe * 0.5);
                }
            }
            SESSION_RACE => level = trunc(safe),
            _ => {}
        }
        if (engine.base().life_left as f32) < 10.0 {
            level = trunc(safe);
        }
        engine.set_turbo_boost_level(level);
    }

    /// `AIDriver::stepDRS` @ 0x1402a0850
    fn step_drs(&mut self, car: &mut RollingChassis, time: f64) {
        if car.controls.drs {
            car.controls.drs = false;
            return;
        }
        let (active, available) = car.drs().map_or((false, false), |drs| (drs.is_active, drs.is_available));
        if active {
            self.last_drs_activation_time = time;
        }
        if self.dist_to_next_corner <= 200.0 || active || !available {
            car.controls.drs = false;
        } else if 5000.0 < time - self.last_drs_activation_time {
            car.controls.drs = true;
        }
    }

    /// `AIDriver::stepKers` @ 0x1402a1bb0
    fn step_kers(&mut self, car: &mut RollingChassis) {
        if 100.0 < self.dist_to_next_corner {
            let base = car.drivetrain.as_ref().unwrap().base();
            car.controls.kers = (base.current_gear as i64 as u64) < (base.gears.len() as u64).wrapping_sub(2);
            return;
        }
        car.controls.kers = false;
    }

    /// `AIDriver::stepP2P` @ 0x1402a1c20
    fn step_p2p(&mut self, car: &mut RollingChassis, recorder: &AiSplineRecorder, session: &SessionInfo) {
        if 50.0 <= car.speed * 3.6 {
            let activations = car.drivetrain.as_ref().unwrap().engine().push_to_pass().map_or(0, |p2p| p2p.activations);
            let laps_left = session.laps.wrapping_sub(car.transponder.lap_count as i32);
            let straight = recorder.best_lap().is_in_straight(self.loc_npos);
            if straight != -1 && (straight as f32) < activations as f32 / laps_left as f32 {
                car.controls.kers = true;
            }
        }
    }

    /// `AIDriver::stepSuperhuman` @ 0x1402a2a60: every step: no locked or spinning wheels,
    /// and for an AI car a hand that pushes it towards its line and slows it before a
    /// braking point.
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    fn step_superhuman(&mut self, car: &mut RollingChassis, recorder: &AiSplineRecorder, dt: f32) {
        let loads = (car.tyres[1].status.load + car.tyres[0].status.load + car.tyres[2].status.load) + car.tyres[3].status.load;
        let t = &car.tyres;
        let front = if t[0].status.nd_slip < t[1].status.nd_slip { t[1].status.nd_slip } else { t[0].status.nd_slip };
        let mut rear = if t[2].status.nd_slip < t[3].status.nd_slip { t[3].status.nd_slip } else { t[2].status.nd_slip };
        for tyre in &mut car.tyres {
            tyre.abs_override = 1.0;
            if (self.use_abs || self.humanize.brake_mistake_time <= 0.0) && 1.0 < tyre.status.nd_slip && (tyre.driven || 0.05 < tyre.status.load / loads) {
                tyre.abs_override = 0.0;
            }
        }
        let right = car.tyres[engineer::right_driven_tyre(car)].status.nd_slip;
        let left = car.tyres[engineer::left_driven_tyre(car)].status.nd_slip;
        let driven = if left < right { right } else { left };
        let mut limit = 1.0f32;
        match car.drivetrain.as_ref().unwrap().base().traction_type {
            TractionType::Rwd => {
                if front < rear {
                    limit = self.oversteer_tc_mult;
                }
            }
            TractionType::Fwd => limit = self.oversteer_tc_mult,
            _ => {
                rear *= 1.2;
                if front < rear {
                    limit = self.oversteer_tc_mult;
                }
            }
        }
        let mut cut = 0.2f32;
        let kmh = car.speed * 3.6;
        if self.race_start_flag {
            cut = 0.5;
            limit = if 20.0 <= kmh { 4.0 } else { (20.0 - kmh) * 10.0 + 4.0 };
            if 50.0 < car.speed * 3.6 {
                self.race_start_flag = false;
            }
        }
        if limit < driven && (5.0 < kmh || self.spline_selection == SplineSelection::Pits) && self.humanize.traction_mistake <= 0.0 {
            car.controls.gas = sat(cut * self.gas_request, 0.0, 1.0);
        }
        if !(car.physics_guid == 0 && !self.use_magic_forces) {
            let (gain, reach) = if self.is_engaged { (2.0f32, 6.0f32) } else { (1.0, 4.0) };
            let off = car.spline_locator_data.lateral_offset - (self.current_offset + self.sin_offset_data.last_final_offset);
            if self.spline_selection == SplineSelection::FastLane && !(car.physics_time - car.last_collision_time <= 1000.0) && !(car.speed * 3.6 <= 20.0 || reach <= off.abs()) {
                let push = if off <= 2.0 {
                    if -2.0 <= off {
                        off
                    } else {
                        -2.0
                    }
                } else {
                    2.0
                };
                let mass = car.core.get_mass(car.body);
                car.core.add_local_force(car.body, &Vec3f::new((mass * push) * gain, 0.0, 0.0));
            }
            if self.spline_selection == SplineSelection::FastLane && 0.0 < self.brake_point.target_speed {
                let length = recorder.best_lap().length();
                if length * (self.brake_point.target_normalized - car.spline_locator_data.npos) < 50.0 {
                    let k = sat(self.brake_point.speed_diff * 0.3, 0.0, 1.0);
                    let mass = car.core.get_mass(car.body);
                    car.core.add_local_force(car.body, &Vec3f::new(0.0, 0.0, (mass * k) * -5.0));
                }
            }
        }
        if 0.0 < self.accident_stop_counter {
            car.controls.brake = 1.0;
            car.controls.gas = 0.0;
            if car.speed < 1.0 {
                self.accident_stop_counter -= dt;
            }
        }
    }
}

/// `getVKMForMult` @ 0x14029e190: the wear (virtual km) at which a compound's grip has
/// fallen to `mult`, past its peak.
fn get_vkm_for_mult(curve: &Curve, mult: f32) -> f32 {
    let mut past_peak = false;
    let mut vkm = 0.0f32;
    let mut last = -9999.0f32;
    loop {
        let value = curve.get_value(vkm);
        if past_peak {
            if value < mult {
                return vkm;
            }
        } else if value < last {
            past_peak = true;
        }
        vkm += 0.2;
        last = value;
        if !(vkm < 1000.0) {
            return 1000.0;
        }
    }
}

impl ControlsProvider for AiDriver {
    /// The driver needs its car: [`ControlsProvider::acquire_controls_car`] is its entry.
    fn acquire_controls(&mut self, _controls: &mut CarControls, _dt: f32, _input: &CarControlsInput) {}

    fn acquire_controls_car(&mut self, car: &mut RollingChassis, dt: f32) {
        self.acquire(car, dt);
    }

    fn is_ai(&self) -> bool {
        true
    }

    /// The driver's handler of `Car::evOnLapCompleted` @ 0x14029b1e0
    fn on_lap_completed(&mut self, valid: bool) {
        if valid {
            self.laps_to_complete -= 1;
            self.run_lap_counter += 1;
        }
    }

    fn on_forced_position(&mut self, car: &mut RollingChassis) {
        let Some(track) = car.track.clone() else { return };
        let mut input = std::mem::take(&mut car.ai_input);
        let session = input.session;
        self.forced_positions(car, &session, &track, &mut input.rand);
        car.ai_input = input;
    }

    fn on_new_session(&mut self, car: &mut RollingChassis, info: &SessionInfo) {
        let mut input = std::mem::take(&mut car.ai_input);
        AiDriver::on_new_session(self, car, info, &mut input.rand);
        car.ai_input = input;
    }
}
