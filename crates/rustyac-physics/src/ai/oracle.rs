// SPDX-License-Identifier: GPL-3.0-or-later

//! The members of a driver that `tools/car_oracle` reads off the game's `AIDriver` after
//! every step, and the same ones of the port: one table for both sides.

use super::driver::AiDriver;

/// How a member is stored.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    F32,
    /// Two words: low, high.
    F64,
    I32,
    U8,
}

/// (name, offset in the game's `AIDriver`, kind)
pub const COLUMNS: &[(&str, usize, Kind)] = &[
    ("steerRequest", 0x230, Kind::F32),
    ("gasRequest", 0x228, Kind::F32),
    ("brakeRequest", 0x22c, Kind::F32),
    ("currentOffset", 0x238, Kind::F32),
    ("desiredOffset", 0x23c, Kind::F32),
    ("liveOffset", 0x13c, Kind::F32),
    ("targetSpeed", 0x184, Kind::F32),
    ("splineTargetSpeed", 0x350, Kind::F32),
    ("locNpos", 0x348, Kind::F32),
    ("brakePoint.targetSpeed", 0x2c, Kind::F32),
    ("brakePoint.targetNormalized", 0x30, Kind::F32),
    ("brakePoint.lateralOffset", 0x3c, Kind::F32),
    ("brakePoint.speedDiff", 0x40, Kind::F32),
    ("distToNextCorner", 0xb8, Kind::F32),
    ("understeerFactor", 0xc4, Kind::F32),
    ("outsideOffset", 0xdc, Kind::F32),
    ("nextCarModifier", 0x80, Kind::F32),
    ("dynamicPush", 0x19c, Kind::F32),
    ("engagedPush", 0x37c, Kind::F32),
    ("dangerPush", 0x1e0, Kind::F32),
    ("oldSteer", 0x180, Kind::F32),
    ("targetSteerFinal", 0x3e4, Kind::F32),
    ("prjDNRPM", 0xc8, Kind::F32),
    ("brakeHintLive", 0x70, Kind::F32),
    ("mongolinoMaxGas", 0xe0, Kind::F32),
    ("offsetDistFromCorner", 0x140, Kind::F32),
    ("smoothLoads.0", 0x3d0, Kind::F32),
    ("smoothLoads.1", 0x3d4, Kind::F32),
    ("smoothLoads.2", 0x3d8, Kind::F32),
    ("smoothLoads.3", 0x3dc, Kind::F32),
    ("minForwardSides.0", 0x358, Kind::F32),
    ("minForwardSides.1", 0x35c, Kind::F32),
    ("steerTarget.x", 0x44, Kind::F32),
    ("steerTarget.y", 0x48, Kind::F32),
    ("steerTarget.z", 0x4c, Kind::F32),
    ("pidSteer.currentError", 0x5c, Kind::F32),
    ("pidSteer.integral", 0x60, Kind::F32),
    ("aiVariation.startPush", 0x368, Kind::F32),
    ("aiVariation.targetPush", 0x36c, Kind::F32),
    ("aiVariation.accum", 0x370, Kind::F32),
    ("aiVariation.currentPush", 0x374, Kind::F32),
    ("aiVariation.variationTime", 0x378, Kind::F32),
    ("humanize.brakeMistakeTime", 0x98, Kind::F32),
    ("humanize.tractionMistake", 0x9c, Kind::F32),
    ("humanize.tractioMistakeSeverity", 0xb0, Kind::F32),
    ("sinOffset.steerOffset", 0x170, Kind::F32),
    ("sinOffset.lastFinalOffset", 0x174, Kind::F32),
    ("payload.radius", 0x244, Kind::F32),
    ("payload.sides.0", 0x248, Kind::F32),
    ("payload.sides.1", 0x24c, Kind::F32),
    ("payload.camber", 0x250, Kind::F32),
    ("payload.grade", 0x27c, Kind::F32),
    ("payload.grip", 0x280, Kind::F32),
    ("payload.distFromCorner", 0x284, Kind::F32),
    ("timeToStart", 0xd0, Kind::F64),
    ("reactionTime", 0x178, Kind::F64),
    ("avgSpeed", 0x190, Kind::F64),
    ("sinOffset.phase", 0x160, Kind::F64),
    ("humanize.nextBrakeMistake", 0xa0, Kind::F64),
    ("humanize.nextTractionMistake", 0xa8, Kind::F64),
    ("lastGearUPChangeTime", 0x148, Kind::F64),
    ("lastGearDNChangeTime", 0x150, Kind::F64),
    ("freqCounter", 0x1a4, Kind::I32),
    ("lapsToComplete", 0x294, Kind::I32),
    ("runLapCounter", 0x2a4, Kind::I32),
    ("splineSelection", 0x68, Kind::I32),
    ("currentPitLineIndex", 0x34c, Kind::I32),
    ("isEngaged", 0x360, Kind::U8),
    ("requestPitStop", 0x7c, Kind::U8),
    ("pitLaneStartFlag", 0x234, Kind::U8),
    ("raceStartFlag", 0x2a0, Kind::U8),
    ("isChangingUp", 0x158, Kind::U8),
    ("isRetiring", 0x159, Kind::U8),
    ("isReactingSlow", 0x1b0, Kind::U8),
    ("isSteeringToPit", 0x1dc, Kind::U8),
    ("brakePoint.wasObstacle", 0x38, Kind::U8),
    ("hasChoosenTyres", 0x2e0, Kind::U8),
];

/// The names of the columns in a row, in order: a float is `ai.<name>`, the halves of a
/// double `ai.<name>.lo` / `.hi`, a whole number or a flag `ai.i.<name>`.
pub fn column_names() -> Vec<String> {
    let mut out = Vec::new();
    for &(name, _, kind) in COLUMNS {
        match kind {
            Kind::F32 => out.push(format!("ai.{name}")),
            Kind::F64 => {
                out.push(format!("ai.{name}.lo"));
                out.push(format!("ai.{name}.hi"));
            }
            Kind::I32 | Kind::U8 => out.push(format!("ai.i.{name}")),
        }
    }
    out
}

impl AiDriver {
    /// The driver's members in the order of [`COLUMNS`], as the words of a row.
    pub fn oracle_words(&self) -> Vec<u32> {
        let f = |x: f32| vec![x.to_bits()];
        let d = |x: f64| vec![x.to_bits() as u32, (x.to_bits() >> 32) as u32];
        let i = |x: i32| vec![x as u32];
        let b = |x: bool| vec![x as u32];
        let bp = &self.brake_point;
        let v = &self.ai_variation;
        let h = &self.humanize;
        let s = &self.sin_offset_data;
        let p = &self.current_payload;
        let values: Vec<(&str, Vec<u32>)> = vec![
            ("steerRequest", f(self.steer_request)),
            ("gasRequest", f(self.gas_request)),
            ("brakeRequest", f(self.brake_request)),
            ("currentOffset", f(self.current_offset)),
            ("desiredOffset", f(self.desired_offset)),
            ("liveOffset", f(self.live_offset)),
            ("targetSpeed", f(self.target_speed)),
            ("splineTargetSpeed", f(self.spline_target_speed)),
            ("locNpos", f(self.loc_npos)),
            ("brakePoint.targetSpeed", f(bp.target_speed)),
            ("brakePoint.targetNormalized", f(bp.target_normalized)),
            ("brakePoint.lateralOffset", f(bp.lateral_offset)),
            ("brakePoint.speedDiff", f(bp.speed_diff)),
            ("distToNextCorner", f(self.dist_to_next_corner)),
            ("understeerFactor", f(self.understeer_factor)),
            ("outsideOffset", f(self.outside_offset)),
            ("nextCarModifier", f(self.next_car_modifier)),
            ("dynamicPush", f(self.dynamic_push)),
            ("engagedPush", f(self.engaged_push)),
            ("dangerPush", f(self.danger_push)),
            ("oldSteer", f(self.old_steer)),
            ("targetSteerFinal", f(self.target_steer_final)),
            ("prjDNRPM", f(self.prj_dn_rpm)),
            ("brakeHintLive", f(self.brake_hint_live)),
            ("mongolinoMaxGas", f(self.mongolino_max_gas)),
            ("offsetDistFromCorner", f(self.offset_dist_from_corner)),
            ("smoothLoads.0", f(self.smooth_loads[0])),
            ("smoothLoads.1", f(self.smooth_loads[1])),
            ("smoothLoads.2", f(self.smooth_loads[2])),
            ("smoothLoads.3", f(self.smooth_loads[3])),
            ("minForwardSides.0", f(self.min_forward_sides[0])),
            ("minForwardSides.1", f(self.min_forward_sides[1])),
            ("steerTarget.x", f(self.steer_target.x)),
            ("steerTarget.y", f(self.steer_target.y)),
            ("steerTarget.z", f(self.steer_target.z)),
            ("pidSteer.currentError", f(self.pid_steer.current_error)),
            ("pidSteer.integral", f(self.pid_steer.integral)),
            ("aiVariation.startPush", f(v.start_push)),
            ("aiVariation.targetPush", f(v.target_push)),
            ("aiVariation.accum", f(v.accum)),
            ("aiVariation.currentPush", f(v.current_push)),
            ("aiVariation.variationTime", f(v.variation_time)),
            ("humanize.brakeMistakeTime", f(h.brake_mistake_time)),
            ("humanize.tractionMistake", f(h.traction_mistake)),
            ("humanize.tractioMistakeSeverity", f(h.tractio_mistake_severity)),
            ("sinOffset.steerOffset", f(s.steer_offset)),
            ("sinOffset.lastFinalOffset", f(s.last_final_offset)),
            ("payload.radius", f(p.radius)),
            ("payload.sides.0", f(p.sides[0])),
            ("payload.sides.1", f(p.sides[1])),
            ("payload.camber", f(p.camber)),
            ("payload.grade", f(p.grade)),
            ("payload.grip", f(p.grip)),
            ("payload.distFromCorner", f(p.dist_from_corner)),
            ("timeToStart", d(self.time_to_start)),
            ("reactionTime", d(self.reaction_time)),
            ("avgSpeed", d(self.avg_speed)),
            ("sinOffset.phase", d(s.phase)),
            ("humanize.nextBrakeMistake", d(h.next_brake_mistake)),
            ("humanize.nextTractionMistake", d(h.next_traction_mistake)),
            ("lastGearUPChangeTime", d(self.last_gear_up_change_time)),
            ("lastGearDNChangeTime", d(self.last_gear_dn_change_time)),
            ("freqCounter", i(self.freq_counter)),
            ("lapsToComplete", i(self.laps_to_complete)),
            ("runLapCounter", i(self.run_lap_counter)),
            ("splineSelection", i(self.spline_selection as i32)),
            ("currentPitLineIndex", i(self.current_pit_line_index)),
            ("isEngaged", b(self.is_engaged)),
            ("requestPitStop", b(self.request_pit_stop)),
            ("pitLaneStartFlag", b(self.pit_lane_start_flag)),
            ("raceStartFlag", b(self.race_start_flag)),
            ("isChangingUp", b(self.is_changing_up)),
            ("isRetiring", b(self.is_retiring)),
            ("isReactingSlow", b(self.is_reacting_slow)),
            ("isSteeringToPit", b(self.is_steering_to_pit)),
            ("brakePoint.wasObstacle", b(bp.was_obstacle)),
            ("hasChoosenTyres", b(self.has_choosen_tyres)),
        ];
        assert_eq!(values.len(), COLUMNS.len(), "the table and the port's list differ in length");
        let mut out = Vec::new();
        for ((name, words), column) in values.into_iter().zip(COLUMNS) {
            assert_eq!(name, column.0, "the table and the port's list differ in order");
            out.extend(words);
        }
        out
    }
}
