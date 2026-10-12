// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! The parts of AC's `Car` only the AI drivers (and the pit stops they make) use.

// (the comparisons and the order of the operations are the machine code's)
#![allow(clippy::double_comparisons, clippy::neg_cmp_op_on_partial_ord, clippy::assign_op_pattern, clippy::manual_range_contains, clippy::needless_range_loop, clippy::field_reassign_with_default, clippy::if_same_then_else, clippy::too_many_arguments)]

use super::SessionInfo;
use crate::car::aero::Drs;
use crate::car::RollingChassis;
use crate::data::ini::IniReader;
use crate::session::MsvcRand;

/// `PitStopTimings` (`car.ini [PIT_STOP]`)
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PitStopTimings {
    pub tyre_change_time_sec: f32,
    pub fuel_change_time_sec: f32,
    pub body_repair_time_sec: f32,
    pub engine_repair_time_sec: f32,
    pub susp_repair_time_sec: f32,
}

impl Default for PitStopTimings {
    /// `Car::Car`'s values, for a car without the section.
    fn default() -> PitStopTimings {
        PitStopTimings { tyre_change_time_sec: 10.0, fuel_change_time_sec: 0.1, body_repair_time_sec: 2.0, engine_repair_time_sec: 2.0, susp_repair_time_sec: 2.0 }
    }
}

/// `PitStopTime`
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PitStopTime {
    pub total: f32,
    pub tyres: f32,
    pub repair: f32,
    pub fuel: f32,
}

impl RollingChassis {
    /// `Car::initPitstopTimings` @ 0x140274020
    pub fn init_pitstop_timings(&mut self) -> Result<(), String> {
        let ini = IniReader::load(&self.data_path.join("car.ini"))?;
        if ini.ready && ini.has_section("PIT_STOP") {
            let t = &mut self.pit_timings;
            t.tyre_change_time_sec = ini.get_float("PIT_STOP", "TYRE_CHANGE_TIME_SEC")?;
            t.fuel_change_time_sec = ini.get_float("PIT_STOP", "FUEL_LITER_TIME_SEC")?;
            t.body_repair_time_sec = ini.get_float("PIT_STOP", "BODY_REPAIR_TIME_SEC")?;
            t.engine_repair_time_sec = ini.get_float("PIT_STOP", "ENGINE_REPAIR_TIME_SEC")?;
            t.susp_repair_time_sec = ini.get_float("PIT_STOP", "SUSP_REPAIR_TIME_SEC")?;
        }
        Ok(())
    }

    /// `Car::expectedFuelPerLap` as `Car::Car` @ 0x14026bf00 works it out, for a car on its
    /// track: `fuel_cons.ini [FUEL_EVAL] KM_PER_LITER` over the fast lane's length, else the
    /// race engineer's estimate ([`RollingChassis::evaluate_fuel_per_lap_from_track_spline`]).
    pub fn init_expected_fuel_per_lap(&mut self) -> Result<(), String> {
        self.expected_fuel_per_lap = 0.0;
        let path = self.data_path.join("fuel_cons.ini");
        if crate::data::exists(&path) {
            let ini = IniReader::load(&path)?;
            if ini.has_section("FUEL_EVAL") {
                let km_per_liter = ini.get_float("FUEL_EVAL", "KM_PER_LITER")?;
                self.expected_fuel_per_lap = self.fast_lane_length() * (1.0 / (km_per_liter * 1000.0));
            }
        }
        if self.expected_fuel_per_lap == 0.0 {
            self.expected_fuel_per_lap = self.evaluate_fuel_per_lap_from_track_spline()?;
        }
        Ok(())
    }

    /// `RaceEngineer::getMaxSpeedFromGear` @ 0x14027c240: the speed at the limiter in a gear
    /// (`gear` + 2 is its place in the drivetrain's list), m/s.
    fn get_max_speed_from_gear(&self, gear: i32) -> f32 {
        let Some(drivetrain) = &self.drivetrain else { return 0.0 };
        let base = drivetrain.base();
        let radius = if base.traction_type == crate::car::TractionType::Fwd { self.tyres[0].data.radius } else { self.tyres[2].data.radius };
        let diameter = radius * 2.0;
        let ratio = usize::try_from(gear + 2).ok().and_then(|index| base.gears.get(index)).map_or(0.0, |gear| gear.ratio);
        let limiter = drivetrain.engine().get_limiter_rpm() as f32;
        let turns = (base.final_ratio as f64 * ratio) * 336.0;
        let mph = (((limiter * diameter) * 39.0) as f64 / turns) as f32;
        mph * f32::from_bits(0x3ee4_e26d)
    }

    /// `RaceEngineer::evaluateFuelPerLapFromTrackSpline` @ 0x14027ac60: the litres a lap
    /// takes, from the speeds and the throttle the track's `data/ideal_line.ai` was recorded
    /// with; 30 for a track without that file.
    pub fn evaluate_fuel_per_lap_from_track_spline(&self) -> Result<f32, String> {
        use rustyac_content::vfs::{self, PathExt};
        let Some(track) = &self.track else { return Ok(30.0) };
        let path = track.data_folder.join("data").join("ideal_line.ai");
        if !path.vfs_is_file() {
            return Ok(30.0);
        }
        let bytes = vfs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let line = crate::track::AiSpline::parse_fast(&bytes).map_err(|e| format!("{}: {e}", path.display()))?;
        let Some(drivetrain) = &self.drivetrain else { return Ok(30.0) };
        let limiter = drivetrain.engine().get_limiter_rpm();
        let up = if limiter == 0 { self.auto_shifter.change_up_rpm } else { limiter };
        let down = self.auto_shifter.change_dn_rpm as f32;
        let k = self.fuel_consumption_k as f32;
        let gears = (drivetrain.base().gears.len() as i32).clamp(0, 8);
        let top = super::sat(self.get_max_speed_from_gear(gears - 4) * 3.6, 0.0, 225.0);
        let mut turbos = drivetrain.engine().turbos_copy();
        let (mut max_speed, mut max_gas) = (0.0f32, 0.0f32);
        let count = line.point_count() as i32;
        for i in 0..count {
            let p = line.payload_at_spline_index(i);
            if max_speed <= p.speed_ms && p.speed_ms < 300.0 {
                max_speed = p.speed_ms;
            }
            if max_gas <= p.gas {
                max_gas = p.gas;
            }
        }
        let mut total = 0.0f32;
        if count != 0 {
            let inv_gas = 1.0 / max_gas;
            let inv_speed = 1.0 / max_speed;
            let span = up as f32 - down;
            for i in 0..count {
                let p = line.payload_at_spline_index(i);
                let pace = super::sat(inv_speed * p.speed_ms, 0.0, 1.0);
                let gas = super::sat(inv_gas * p.gas, 0.0, 1.0);
                let rpm = span * gas + down;
                let prev = line.spline.point_at(line.spline.wrap_index(i - 1) as u32);
                let cur = line.spline.point_at(i as u32);
                let (dx, dy, dz) = (cur[0] - prev[0], cur[1] - prev[1], cur[2] - prev[2]);
                let squared = (dy * dy + dx * dx) + dz * dz;
                let distance = if squared < 0.0 || squared > 0.0 { crate::math::sqrtf(squared) } else { 0.0 };
                let seconds = distance / (((top - 1.0) * pace + 1.0) * f32::from_bits(0x3e8e_38e4));
                let mut boost = 0.0f32;
                if !turbos.is_empty() {
                    let turning = if rpm <= 0.0 { 0.0 } else { rpm };
                    for turbo in &mut turbos {
                        turbo.step(gas, turning, seconds);
                        boost += turbo.get_boost();
                    }
                }
                let boost = if boost <= 0.0 { 0.0 } else { boost };
                let rate = ((boost * 1.1 + 1.0) * (rpm.abs() * gas)) * k;
                total += ((rate as f64) * 0.001) as f32 * seconds;
            }
        }
        total *= 1.25;
        let max = self.max_fuel as f32;
        Ok(if total <= max {
            if total < 0.0 {
                0.0
            } else {
                total
            }
        } else {
            max
        })
    }

    /// `Car::getFuelPerLap` @ 0x140270bd0
    pub fn get_fuel_per_lap(&self) -> f32 {
        let per_lap = self.fuel_lap_evaluator.fuel_per_lap();
        if per_lap <= 0.0 {
            self.expected_fuel_per_lap
        } else {
            per_lap
        }
    }

    /// `Car::setFuelForLaps` @ 0x140275b60: fills the tank for a number of laps; the laps
    /// the tank holds when that is fewer (and then nothing is filled).
    pub fn set_fuel_for_laps(&mut self, laps: i32, mult: f32) -> i32 {
        let per_lap = self.get_fuel_per_lap();
        let max = self.max_fuel;
        let wanted = (laps as f32 * per_lap) * mult;
        if max <= wanted as f64 {
            let fits = max / per_lap as f64;
            if fits.is_nan() || !(-2147483648.0..2147483648.0).contains(&fits) {
                i32::MIN
            } else {
                fits as i32
            }
        } else {
            let max = max as f32;
            let fuel = if wanted <= max {
                if wanted < 0.0 {
                    0.0
                } else {
                    wanted
                }
            } else {
                max
            };
            self.requested_fuel = fuel;
            self.fuel = fuel as f64;
            laps
        }
    }

    /// `Car::getPitstopTime` @ 0x140271e60: the seconds a stop takes. `randomize` draws
    /// one `rand()`.
    #[allow(clippy::too_many_arguments)]
    pub fn get_pitstop_time(&self, fuel: f32, tyres: bool, body: bool, engine: bool, suspension: bool, randomize: bool, rand: &mut MsvcRand) -> PitStopTime {
        let t = &self.pit_timings;
        let extra = if randomize { rand.next() as f32 * super::RAND_UNIT } else { 0.0 };
        let tyres_time = if tyres { extra + t.tyre_change_time_sec } else { 0.0 };
        let mut fuel_time = fuel * t.fuel_change_time_sec;
        if 0.0 < fuel_time {
            fuel_time += extra;
        }
        let mut out = PitStopTime { tyres: tyres_time, ..PitStopTime::default() };
        out.total = if tyres_time < fuel_time { fuel_time } else { tyres_time };
        let mut repair = 0.0f32;
        if body {
            for zone in 0..4 {
                let level = self.damage_zone_level[zone];
                if 0.0 < level {
                    let share = super::sat(level * 0.01, 0.0, 1.0);
                    let part = (t.body_repair_time_sec - 2.0) * share;
                    repair = if zone == 0 { (part + 2.0) + extra } else { ((repair + part) + 2.0) + extra };
                }
            }
        }
        if engine {
            if let Some(drivetrain) = &self.drivetrain {
                let life = drivetrain.engine().base().life_left as f32;
                if life < 1000.0 {
                    let mut lost = 1000.0 - life;
                    if lost < 0.0 {
                        lost = 0.0;
                    }
                    repair = (repair + (lost * t.engine_repair_time_sec) * 0.1) + extra;
                }
            }
        }
        if suspension {
            for s in &self.suspensions {
                let damage = s.get_damage();
                if 0.0 < damage {
                    let damage = super::sat(damage, 0.0, 1.0);
                    repair = (repair + damage * t.susp_repair_time_sec) + extra;
                }
            }
        }
        out.repair = repair;
        if out.total <= repair {
            out.total = repair;
        }
        out.fuel = fuel_time;
        out
    }

    /// `Car::resetSplineLocator` @ 0x140275960: `SplineLocator::resetToClosestPoint`
    /// @ 0x1402ab530.
    pub fn reset_spline_locator(&mut self) {
        let Some(track) = self.track.clone() else { return };
        let Some(spline) = &track.ai_spline else { return };
        let p = self.core.get_position(self.body);
        let p = [p.x, p.y, p.z];
        self.spline_locator.current_index = -1;
        self.spline_locator.normalized_pos = crate::track::spline::locate_on_spline(spline, &p, &mut self.spline_locator.current_index, None);
        self.spline_locator.offset = spline.spline.get_signed_distance_from_spline(&p, self.spline_locator.normalized_pos);
    }

    /// `Spline::length` of the locator's line (the fast lane).
    pub fn fast_lane_length(&self) -> f32 {
        self.track.as_ref().and_then(|track| track.ai_spline.as_ref()).map_or(0.0, |spline| spline.length())
    }

    /// `ksPhysics->sessionInfo` as the car was last handed it.
    pub fn ai_session(&self) -> SessionInfo {
        self.ai_input.session
    }

    /// `Car::drs`
    pub fn drs(&self) -> Option<&Drs> {
        self.aero.as_ref().map(|aero| &aero.base().drs)
    }

    /// `Car::getCGHeight` @ 0x140270b40: the body's height above the mean of the four
    /// contact points.
    pub fn get_cg_height(&self) -> f32 {
        let mut sum = 0.0f32;
        for tyre in &self.tyres {
            sum += tyre.contact_point.y;
        }
        self.core.get_position(self.body).y - sum * 0.25
    }

    /// `ksPhysics->track->dynamicGripLevel`
    pub fn track_grip_level(&self) -> f32 {
        self.env.dynamic_grip_level
    }
}
