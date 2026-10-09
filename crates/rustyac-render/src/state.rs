// SPDX-License-Identifier: GPL-3.0-or-later

//! The game's `CarPhysicsState` (0xb70 bytes): what the physics hands the car's picture
//! every frame. The members have the game's names; [`CarPhysicsState::to_game_bytes`] lays
//! them out as `acs.exe` has them, so that one file can feed the game's own code and the port.

use rustyac_physics::vecmath::Mat44f;

/// The size of the game's `CarPhysicsState`.
pub const GAME_SIZE: usize = 0xb70;

/// `SurfaceDef` (0xc8 bytes) without its `userPointer`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SurfaceDef {
    pub wav_string: [u16; 64],
    pub wav_pitch_speed: f32,
    pub grip_mod: f32,
    pub sector_id: i32,
    pub dirt_additive_k: f32,
    pub collision_category: u32,
    pub is_valid_track: bool,
    pub black_flag_time: f32,
    pub sin_height: f32,
    pub sin_length: f32,
    pub is_pitlane: bool,
    pub damping: f32,
    pub granularity: f32,
    pub vibration_gain: f32,
    pub vibration_length: f32,
}

impl Default for SurfaceDef {
    fn default() -> SurfaceDef {
        SurfaceDef {
            wav_string: [0; 64],
            wav_pitch_speed: 0.0,
            grip_mod: 0.0,
            sector_id: 0,
            dirt_additive_k: 0.0,
            collision_category: 0,
            is_valid_track: false,
            black_flag_time: 0.0,
            sin_height: 0.0,
            sin_length: 0.0,
            is_pitlane: false,
            damping: 0.0,
            granularity: 0.0,
            vibration_gain: 0.0,
            vibration_length: 0.0,
        }
    }
}

/// `TyreThermalState` (0xc0 bytes).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TyreThermalState {
    pub temps: [[f32; 12]; 3],
    pub core_temp: f32,
    pub thermal_input: f32,
    pub dynamic_pressure: f32,
    pub static_pressure: f32,
    pub last_set_imo: [f32; 3],
    pub cp_temperature: f32,
    pub last_grain: f32,
    pub last_blister: f32,
    pub mult: f32,
    pub is_hot: bool,
}

/// The game's `CarPhysicsState`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CarPhysicsState {
    pub physics_guid: u8,
    /// the body's world matrix
    pub world_matrix: Mat44f,
    /// each hub's world matrix (`ISuspension::getHubWorldMatrix`), LF, RF, LR, RR
    pub suspension_matrix: [Mat44f; 4],
    /// each wheel's rotation with its spin (`Tyre::getFinalTyreRotation`) at the hub
    pub tyre_matrix: [Mat44f; 4],
    pub engine_rpm: f32,
    pub is_engine_limiter_on: bool,
    /// rad/s; 0 while a tyre is locked or the car sleeps
    pub wheel_angular_speed: [f32; 4],
    /// the steering wheel's angle, degrees (`controls.steer * steerLock`)
    pub steer: f32,
    pub gas: f32,
    pub brake: f32,
    pub clutch: f32,
    /// 0 reverse, 1 neutral, 2 first
    pub gear: i32,
    /// m/s
    pub speed: f32,
    pub velocity: [f32; 3],
    pub local_velocity: [f32; 3],
    pub local_angular_velocity: [f32; 3],
    pub angular_velocity: [f32; 3],
    pub slip_angle: [f32; 4],
    pub slip_ratio: [f32; 4],
    pub tyre_slip: [f32; 4],
    pub nd_slip: [f32; 4],
    pub load: [f32; 4],
    pub dy: [f32; 4],
    pub mz: [f32; 4],
    pub tyre_dirty_level: [f32; 4],
    pub tyre_surface_def: [SurfaceDef; 4],
    pub cg_height: f32,
    pub acc_g: [f32; 3],
    pub lap_time: u32,
    pub last_lap: u32,
    pub best_lap: u32,
    pub lap_count: u32,
    pub last_ff_pure: f32,
    pub last_ff_final: f32,
    /// CD, CL front, CL rear
    pub aero: [f32; 3],
    pub tyre_contact_point: [[f32; 3]; 4],
    pub tyre_contact_normal: [[f32; 3]; 4],
    pub camber_rad: [f32; 4],
    pub tyre_radius: [f32; 4],
    pub tyre_loaded_radius: [f32; 4],
    pub suspension_travel: [f32; 4],
    pub normalized_spline_position: f32,
    pub drift_points: f32,
    pub instant_drift: f32,
    pub is_drift_valid: bool,
    pub drift_combo_counter: i32,
    pub drift_bonus_on: bool,
    pub drivetrain_speed: f32,
    pub turbo_boost: f32,
    pub performance_meter: f32,
    pub performance_meter_speed_diff_ms: f32,
    pub is_gear_grinding: bool,
    pub body_work_volume: f32,
    pub tyre_virtual_km: [f32; 4],
    pub damage_zone_level: [f32; 5],
    pub limiter_rpm: i32,
    /// normal, d
    pub ground_plane: [f32; 4],
    pub time_stamp: f64,
    pub air_density: f32,
    pub fuel: f32,
    pub fuel_laps: f32,
    pub ride_height: [f32; 2],
    pub is_retired: bool,
    pub engine_life_left: f32,
    pub turbo_bov: f32,
    pub turbo_boost_level: f32,
    pub tyre_grain: [f32; 4],
    pub tyre_blister: [f32; 4],
    pub actions_state: i32,
    pub setup_state: i32,
    pub tyre_inflation: [f32; 4],
    pub kers_charge: f32,
    pub kers_input: f32,
    pub gear_rpm_window: f32,
    pub sus_damage: [f32; 4],
    pub tyre_flat_spot: [f32; 4],
    pub water: f32,
    pub tyre_thermal_states: [TyreThermalState; 4],
    pub disc_temps: [f32; 4],
    pub wear: [f32; 4],
    pub wear_mult: [f32; 4],
    pub lock_controls_time: f32,
    pub kers_current_kj: f32,
    pub kers_is_charging: bool,
    pub status_bytes: u32,
    pub p2p_status: u8,
    pub p2p_activations: u8,
    pub anti_squat: f32,
    pub caster: [f32; 2],
}

impl Default for CarPhysicsState {
    fn default() -> CarPhysicsState {
        CarPhysicsState::at_origin()
    }
}

struct Writer(Vec<u8>);

impl Writer {
    fn f(&mut self, at: usize, v: f32) {
        self.0[at..at + 4].copy_from_slice(&v.to_le_bytes());
    }
    fn fs(&mut self, at: usize, v: &[f32]) {
        for (i, x) in v.iter().enumerate() {
            self.f(at + 4 * i, *x);
        }
    }
    fn i(&mut self, at: usize, v: i32) {
        self.0[at..at + 4].copy_from_slice(&v.to_le_bytes());
    }
    fn u(&mut self, at: usize, v: u32) {
        self.0[at..at + 4].copy_from_slice(&v.to_le_bytes());
    }
    fn b(&mut self, at: usize, v: bool) {
        self.0[at] = v as u8;
    }
    fn m(&mut self, at: usize, m: &Mat44f) {
        for r in 0..4 {
            self.fs(at + 16 * r, &m.m[r]);
        }
    }
}

struct Reader<'a>(&'a [u8]);

impl Reader<'_> {
    fn f(&self, at: usize) -> f32 {
        f32::from_le_bytes([self.0[at], self.0[at + 1], self.0[at + 2], self.0[at + 3]])
    }
    fn fs<const N: usize>(&self, at: usize) -> [f32; N] {
        std::array::from_fn(|i| self.f(at + 4 * i))
    }
    fn i(&self, at: usize) -> i32 {
        self.f(at).to_bits() as i32
    }
    fn u(&self, at: usize) -> u32 {
        self.f(at).to_bits()
    }
    fn b(&self, at: usize) -> bool {
        self.0[at] != 0
    }
    fn m(&self, at: usize) -> Mat44f {
        Mat44f { m: std::array::from_fn(|r| self.fs(at + 16 * r)) }
    }
}

impl CarPhysicsState {
    /// The car at rest at the origin, everything else zero.
    pub fn at_origin() -> CarPhysicsState {
        CarPhysicsState {
            physics_guid: 0,
            world_matrix: Mat44f::IDENTITY,
            suspension_matrix: [Mat44f::IDENTITY; 4],
            tyre_matrix: [Mat44f::IDENTITY; 4],
            engine_rpm: 0.0,
            is_engine_limiter_on: false,
            wheel_angular_speed: [0.0; 4],
            steer: 0.0,
            gas: 0.0,
            brake: 0.0,
            clutch: 0.0,
            gear: 0,
            speed: 0.0,
            velocity: [0.0; 3],
            local_velocity: [0.0; 3],
            local_angular_velocity: [0.0; 3],
            angular_velocity: [0.0; 3],
            slip_angle: [0.0; 4],
            slip_ratio: [0.0; 4],
            tyre_slip: [0.0; 4],
            nd_slip: [0.0; 4],
            load: [0.0; 4],
            dy: [0.0; 4],
            mz: [0.0; 4],
            tyre_dirty_level: [0.0; 4],
            tyre_surface_def: [SurfaceDef::default(); 4],
            cg_height: 0.0,
            acc_g: [0.0; 3],
            lap_time: 0,
            last_lap: 0,
            best_lap: 0,
            lap_count: 0,
            last_ff_pure: 0.0,
            last_ff_final: 0.0,
            aero: [0.0; 3],
            tyre_contact_point: [[0.0; 3]; 4],
            tyre_contact_normal: [[0.0; 3]; 4],
            camber_rad: [0.0; 4],
            tyre_radius: [0.0; 4],
            tyre_loaded_radius: [0.0; 4],
            suspension_travel: [0.0; 4],
            normalized_spline_position: 0.0,
            drift_points: 0.0,
            instant_drift: 0.0,
            is_drift_valid: false,
            drift_combo_counter: 0,
            drift_bonus_on: false,
            drivetrain_speed: 0.0,
            turbo_boost: 0.0,
            performance_meter: 0.0,
            performance_meter_speed_diff_ms: 0.0,
            is_gear_grinding: false,
            body_work_volume: 0.0,
            tyre_virtual_km: [0.0; 4],
            damage_zone_level: [0.0; 5],
            limiter_rpm: 0,
            ground_plane: [0.0; 4],
            time_stamp: 0.0,
            air_density: 0.0,
            fuel: 0.0,
            fuel_laps: 0.0,
            ride_height: [0.0; 2],
            is_retired: false,
            engine_life_left: 0.0,
            turbo_bov: 0.0,
            turbo_boost_level: 0.0,
            tyre_grain: [0.0; 4],
            tyre_blister: [0.0; 4],
            actions_state: 0,
            setup_state: 0,
            tyre_inflation: [0.0; 4],
            kers_charge: 0.0,
            kers_input: 0.0,
            gear_rpm_window: 0.0,
            sus_damage: [0.0; 4],
            tyre_flat_spot: [0.0; 4],
            water: 0.0,
            tyre_thermal_states: [TyreThermalState::default(); 4],
            disc_temps: [0.0; 4],
            wear: [0.0; 4],
            wear_mult: [0.0; 4],
            lock_controls_time: 0.0,
            kers_current_kj: 0.0,
            kers_is_charging: false,
            status_bytes: 0,
            p2p_status: 0,
            p2p_activations: 0,
            anti_squat: 0.0,
            caster: [0.0; 2],
        }
    }

    /// The 0xb70 bytes of the game's structure (padding bytes are zero).
    pub fn to_game_bytes(&self) -> Vec<u8> {
        let mut w = Writer(vec![0u8; GAME_SIZE]);
        w.0[0] = self.physics_guid;
        w.m(0x4, &self.world_matrix);
        for i in 0..4 {
            w.m(0x44 + 0x40 * i, &self.suspension_matrix[i]);
            w.m(0x144 + 0x40 * i, &self.tyre_matrix[i]);
        }
        w.f(0x244, self.engine_rpm);
        w.b(0x248, self.is_engine_limiter_on);
        w.fs(0x24c, &self.wheel_angular_speed);
        w.f(0x25c, self.steer);
        w.f(0x260, self.gas);
        w.f(0x264, self.brake);
        w.f(0x268, self.clutch);
        w.i(0x26c, self.gear);
        w.f(0x270, self.speed);
        w.fs(0x274, &self.velocity);
        w.fs(0x280, &self.local_velocity);
        w.fs(0x28c, &self.local_angular_velocity);
        w.fs(0x298, &self.angular_velocity);
        w.fs(0x2a4, &self.slip_angle);
        w.fs(0x2b4, &self.slip_ratio);
        w.fs(0x2c4, &self.tyre_slip);
        w.fs(0x2d4, &self.nd_slip);
        w.fs(0x2e4, &self.load);
        w.fs(0x2f4, &self.dy);
        w.fs(0x304, &self.mz);
        w.fs(0x314, &self.tyre_dirty_level);
        for i in 0..4 {
            let at = 0x328 + 0xc8 * i;
            let s = &self.tyre_surface_def[i];
            for (k, c) in s.wav_string.iter().enumerate() {
                w.0[at + 2 * k..at + 2 * k + 2].copy_from_slice(&c.to_le_bytes());
            }
            w.f(at + 0x80, s.wav_pitch_speed);
            w.f(at + 0x90, s.grip_mod);
            w.i(at + 0x94, s.sector_id);
            w.f(at + 0x98, s.dirt_additive_k);
            w.u(at + 0x9c, s.collision_category);
            w.b(at + 0xa0, s.is_valid_track);
            w.f(at + 0xa4, s.black_flag_time);
            w.f(at + 0xa8, s.sin_height);
            w.f(at + 0xac, s.sin_length);
            w.b(at + 0xb0, s.is_pitlane);
            w.f(at + 0xb4, s.damping);
            w.f(at + 0xb8, s.granularity);
            w.f(at + 0xbc, s.vibration_gain);
            w.f(at + 0xc0, s.vibration_length);
        }
        w.f(0x648, self.cg_height);
        w.fs(0x64c, &self.acc_g);
        w.u(0x658, self.lap_time);
        w.u(0x65c, self.last_lap);
        w.u(0x660, self.best_lap);
        w.u(0x664, self.lap_count);
        w.f(0x668, self.last_ff_pure);
        w.f(0x66c, self.last_ff_final);
        w.fs(0x670, &self.aero);
        for i in 0..4 {
            w.fs(0x67c + 12 * i, &self.tyre_contact_point[i]);
            w.fs(0x6ac + 12 * i, &self.tyre_contact_normal[i]);
        }
        w.fs(0x6dc, &self.camber_rad);
        w.fs(0x6ec, &self.tyre_radius);
        w.fs(0x6fc, &self.tyre_loaded_radius);
        w.fs(0x70c, &self.suspension_travel);
        w.f(0x71c, self.normalized_spline_position);
        w.f(0x720, self.drift_points);
        w.f(0x724, self.instant_drift);
        w.b(0x728, self.is_drift_valid);
        w.i(0x72c, self.drift_combo_counter);
        w.b(0x730, self.drift_bonus_on);
        w.f(0x734, self.drivetrain_speed);
        w.f(0x738, self.turbo_boost);
        w.f(0x73c, self.performance_meter);
        w.f(0x740, self.performance_meter_speed_diff_ms);
        w.b(0x744, self.is_gear_grinding);
        w.f(0x748, self.body_work_volume);
        w.fs(0x74c, &self.tyre_virtual_km);
        w.fs(0x75c, &self.damage_zone_level);
        w.i(0x770, self.limiter_rpm);
        w.fs(0x774, &self.ground_plane);
        w.0[0x788..0x790].copy_from_slice(&self.time_stamp.to_le_bytes());
        w.f(0x790, self.air_density);
        w.f(0x794, self.fuel);
        w.f(0x798, self.fuel_laps);
        w.fs(0x79c, &self.ride_height);
        w.b(0x7a4, self.is_retired);
        w.f(0x7a8, self.engine_life_left);
        w.f(0x7ac, self.turbo_bov);
        w.f(0x7b0, self.turbo_boost_level);
        w.fs(0x7b4, &self.tyre_grain);
        w.fs(0x7c4, &self.tyre_blister);
        w.i(0x7d4, self.actions_state);
        w.i(0x7d8, self.setup_state);
        w.fs(0x7dc, &self.tyre_inflation);
        w.f(0x7ec, self.kers_charge);
        w.f(0x7f0, self.kers_input);
        w.f(0x7f4, self.gear_rpm_window);
        w.fs(0x7f8, &self.sus_damage);
        w.fs(0x808, &self.tyre_flat_spot);
        w.f(0x818, self.water);
        for i in 0..4 {
            let at = 0x81c + 0xc0 * i;
            let t = &self.tyre_thermal_states[i];
            for r in 0..3 {
                w.fs(at + 48 * r, &t.temps[r]);
            }
            w.f(at + 0x90, t.core_temp);
            w.f(at + 0x94, t.thermal_input);
            w.f(at + 0x98, t.dynamic_pressure);
            w.f(at + 0x9c, t.static_pressure);
            w.fs(at + 0xa0, &t.last_set_imo);
            w.f(at + 0xac, t.cp_temperature);
            w.f(at + 0xb0, t.last_grain);
            w.f(at + 0xb4, t.last_blister);
            w.f(at + 0xb8, t.mult);
            w.b(at + 0xbc, t.is_hot);
        }
        w.fs(0xb1c, &self.disc_temps);
        w.fs(0xb2c, &self.wear);
        w.fs(0xb3c, &self.wear_mult);
        w.f(0xb4c, self.lock_controls_time);
        w.f(0xb50, self.kers_current_kj);
        w.b(0xb54, self.kers_is_charging);
        w.u(0xb58, self.status_bytes);
        w.0[0xb5c] = self.p2p_status;
        w.0[0xb5d] = self.p2p_activations;
        w.f(0xb60, self.anti_squat);
        w.fs(0xb64, &self.caster);
        w.0
    }

    /// The reverse of [`CarPhysicsState::to_game_bytes`].
    pub fn from_game_bytes(bytes: &[u8]) -> Result<CarPhysicsState, String> {
        if bytes.len() != GAME_SIZE {
            return Err(format!("a CarPhysicsState is {GAME_SIZE} bytes, not {}", bytes.len()));
        }
        let r = Reader(bytes);
        let mut s = CarPhysicsState::at_origin();
        s.physics_guid = bytes[0];
        s.world_matrix = r.m(0x4);
        for i in 0..4 {
            s.suspension_matrix[i] = r.m(0x44 + 0x40 * i);
            s.tyre_matrix[i] = r.m(0x144 + 0x40 * i);
        }
        s.engine_rpm = r.f(0x244);
        s.is_engine_limiter_on = r.b(0x248);
        s.wheel_angular_speed = r.fs(0x24c);
        s.steer = r.f(0x25c);
        s.gas = r.f(0x260);
        s.brake = r.f(0x264);
        s.clutch = r.f(0x268);
        s.gear = r.i(0x26c);
        s.speed = r.f(0x270);
        s.velocity = r.fs(0x274);
        s.local_velocity = r.fs(0x280);
        s.local_angular_velocity = r.fs(0x28c);
        s.angular_velocity = r.fs(0x298);
        s.slip_angle = r.fs(0x2a4);
        s.slip_ratio = r.fs(0x2b4);
        s.tyre_slip = r.fs(0x2c4);
        s.nd_slip = r.fs(0x2d4);
        s.load = r.fs(0x2e4);
        s.dy = r.fs(0x2f4);
        s.mz = r.fs(0x304);
        s.tyre_dirty_level = r.fs(0x314);
        for i in 0..4 {
            let at = 0x328 + 0xc8 * i;
            let d = &mut s.tyre_surface_def[i];
            for k in 0..64 {
                d.wav_string[k] = u16::from_le_bytes([bytes[at + 2 * k], bytes[at + 2 * k + 1]]);
            }
            d.wav_pitch_speed = r.f(at + 0x80);
            d.grip_mod = r.f(at + 0x90);
            d.sector_id = r.i(at + 0x94);
            d.dirt_additive_k = r.f(at + 0x98);
            d.collision_category = r.u(at + 0x9c);
            d.is_valid_track = r.b(at + 0xa0);
            d.black_flag_time = r.f(at + 0xa4);
            d.sin_height = r.f(at + 0xa8);
            d.sin_length = r.f(at + 0xac);
            d.is_pitlane = r.b(at + 0xb0);
            d.damping = r.f(at + 0xb4);
            d.granularity = r.f(at + 0xb8);
            d.vibration_gain = r.f(at + 0xbc);
            d.vibration_length = r.f(at + 0xc0);
        }
        s.cg_height = r.f(0x648);
        s.acc_g = r.fs(0x64c);
        s.lap_time = r.u(0x658);
        s.last_lap = r.u(0x65c);
        s.best_lap = r.u(0x660);
        s.lap_count = r.u(0x664);
        s.last_ff_pure = r.f(0x668);
        s.last_ff_final = r.f(0x66c);
        s.aero = r.fs(0x670);
        for i in 0..4 {
            s.tyre_contact_point[i] = r.fs(0x67c + 12 * i);
            s.tyre_contact_normal[i] = r.fs(0x6ac + 12 * i);
        }
        s.camber_rad = r.fs(0x6dc);
        s.tyre_radius = r.fs(0x6ec);
        s.tyre_loaded_radius = r.fs(0x6fc);
        s.suspension_travel = r.fs(0x70c);
        s.normalized_spline_position = r.f(0x71c);
        s.drift_points = r.f(0x720);
        s.instant_drift = r.f(0x724);
        s.is_drift_valid = r.b(0x728);
        s.drift_combo_counter = r.i(0x72c);
        s.drift_bonus_on = r.b(0x730);
        s.drivetrain_speed = r.f(0x734);
        s.turbo_boost = r.f(0x738);
        s.performance_meter = r.f(0x73c);
        s.performance_meter_speed_diff_ms = r.f(0x740);
        s.is_gear_grinding = r.b(0x744);
        s.body_work_volume = r.f(0x748);
        s.tyre_virtual_km = r.fs(0x74c);
        s.damage_zone_level = r.fs(0x75c);
        s.limiter_rpm = r.i(0x770);
        s.ground_plane = r.fs(0x774);
        s.time_stamp = f64::from_le_bytes(bytes[0x788..0x790].try_into().expect("8 bytes"));
        s.air_density = r.f(0x790);
        s.fuel = r.f(0x794);
        s.fuel_laps = r.f(0x798);
        s.ride_height = r.fs(0x79c);
        s.is_retired = r.b(0x7a4);
        s.engine_life_left = r.f(0x7a8);
        s.turbo_bov = r.f(0x7ac);
        s.turbo_boost_level = r.f(0x7b0);
        s.tyre_grain = r.fs(0x7b4);
        s.tyre_blister = r.fs(0x7c4);
        s.actions_state = r.i(0x7d4);
        s.setup_state = r.i(0x7d8);
        s.tyre_inflation = r.fs(0x7dc);
        s.kers_charge = r.f(0x7ec);
        s.kers_input = r.f(0x7f0);
        s.gear_rpm_window = r.f(0x7f4);
        s.sus_damage = r.fs(0x7f8);
        s.tyre_flat_spot = r.fs(0x808);
        s.water = r.f(0x818);
        for i in 0..4 {
            let at = 0x81c + 0xc0 * i;
            let t = &mut s.tyre_thermal_states[i];
            for row in 0..3 {
                t.temps[row] = r.fs(at + 48 * row);
            }
            t.core_temp = r.f(at + 0x90);
            t.thermal_input = r.f(at + 0x94);
            t.dynamic_pressure = r.f(at + 0x98);
            t.static_pressure = r.f(at + 0x9c);
            t.last_set_imo = r.fs(at + 0xa0);
            t.cp_temperature = r.f(at + 0xac);
            t.last_grain = r.f(at + 0xb0);
            t.last_blister = r.f(at + 0xb4);
            t.mult = r.f(at + 0xb8);
            t.is_hot = r.b(at + 0xbc);
        }
        s.disc_temps = r.fs(0xb1c);
        s.wear = r.fs(0xb2c);
        s.wear_mult = r.fs(0xb3c);
        s.lock_controls_time = r.f(0xb4c);
        s.kers_current_kj = r.f(0xb50);
        s.kers_is_charging = r.b(0xb54);
        s.status_bytes = r.u(0xb58);
        s.p2p_status = bytes[0xb5c];
        s.p2p_activations = bytes[0xb5d];
        s.anti_squat = r.f(0xb60);
        s.caster = r.fs(0xb64);
        Ok(s)
    }

    /// The Task 20 pose file: 149 floats, little endian: the body, the four hubs, the four
    /// wheels, the wheel speeds, the steer angle.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(149 * 4);
        let mut put = |m: &Mat44f| {
            for row in &m.m {
                for v in row {
                    out.extend(v.to_le_bytes());
                }
            }
        };
        put(&self.world_matrix);
        self.suspension_matrix.iter().for_each(&mut put);
        self.tyre_matrix.iter().for_each(&mut put);
        for v in self.wheel_angular_speed.iter().chain([&self.steer]) {
            out.extend(v.to_le_bytes());
        }
        out
    }

    /// A pose file of Task 20 (149 floats) or a whole state (0xb70 bytes).
    pub fn from_bytes(bytes: &[u8]) -> Result<CarPhysicsState, String> {
        if bytes.len() == GAME_SIZE {
            return CarPhysicsState::from_game_bytes(bytes);
        }
        if bytes.len() != 149 * 4 {
            return Err(format!("a car state is {} or {GAME_SIZE} bytes, not {}", 149 * 4, bytes.len()));
        }
        let f: Vec<f32> = bytes.chunks_exact(4).map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])).collect();
        let matrix = |at: usize| {
            let mut m = Mat44f::default();
            for r in 0..4 {
                for c in 0..4 {
                    m.m[r][c] = f[at + r * 4 + c];
                }
            }
            m
        };
        Ok(CarPhysicsState {
            world_matrix: matrix(0),
            suspension_matrix: [matrix(16), matrix(32), matrix(48), matrix(64)],
            tyre_matrix: [matrix(80), matrix(96), matrix(112), matrix(128)],
            wheel_angular_speed: [f[144], f[145], f[146], f[147]],
            steer: f[148],
            ..CarPhysicsState::at_origin()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn game_bytes_round_trip() {
        let mut s = CarPhysicsState::at_origin();
        s.engine_rpm = 7200.0;
        s.gear = 3;
        s.damage_zone_level = [1.0, 2.0, 3.0, 4.0, 5.0];
        s.tyre_surface_def[2].dirt_additive_k = 0.5;
        s.tyre_thermal_states[3].core_temp = 80.0;
        s.caster = [0.1, 0.2];
        s.time_stamp = 12.5;
        let bytes = s.to_game_bytes();
        assert_eq!(bytes.len(), GAME_SIZE);
        assert_eq!(CarPhysicsState::from_game_bytes(&bytes).unwrap(), s);
        assert_eq!(f32::from_le_bytes(bytes[0x244..0x248].try_into().unwrap()), 7200.0);
    }
}
