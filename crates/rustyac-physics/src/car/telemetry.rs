// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! The telemetry link: the `acpmf_physics` shared-memory page (`SPageFilePhysics`, 592 bytes)
//! as AC's `SharedMemoryWriter::updatePhysics` @ 0x140186ef0 fills it after every physics
//! step, produced from a Rust car. The layout and the bits are the game's, so the tools that
//! read the game's page (`ac_telemetry.py`, `check_telemetry.py`, `car_oracle csv --table
//! telemetry`) read this one too.
//!
//! A few values reach the game's page through the state snapshot of the car
//! (`Car::getPhysicsState` @ 0x140270d70: the ride heights, the limiter's revs, the
//! push-to-pass state); the [`PhysicsPageWriter`] keeps what that needs between two steps.
//! In the running game the writer reads the main thread's copy of that snapshot, which is
//! refreshed once per drawn frame, so there those values can be up to a frame old; here (as in
//! `tools/car_oracle`) they are the snapshot of the same step.
//!
//! Only the physics page exists. The graphics page (`SharedMemoryWriter::update`, per frame)
//! and the static page (`writeStatic`, once) belong to the game's main thread.
//!
//! Fields of systems the port does not have are written as the game writes them for a car
//! without those systems: `kersCharge`, `kersInput`, `kersCurrentKJ` (0 unless the car has
//! `ers.ini` or `kers.ini`), `ersRecoveryLevel`, `ersPowerLevel`, `ersHeatCharging`,
//! `ersIsCharging` (0 unless `ers.ini`), `isAIControlled` (the device is not an AI driver),
//! `performanceMeter` (the lap-time meter; 0 off a timed lap).

use std::path::Path;

use super::aero::plane4f;
use super::chassis::RollingChassis;
use crate::data::ini::IniReader;
use crate::math::{asinf, atan2f};
use crate::vecmath::Vec3f;

/// The members of `SPageFilePhysics` in file order: name, kind (`i` = 32-bit integer, `f` =
/// float), number of elements. 148 four-byte values in all.
pub const PAGE_FIELDS: [(&str, char, usize); 63] = [
    ("packetId", 'i', 1),
    ("gas", 'f', 1),
    ("brake", 'f', 1),
    ("fuel", 'f', 1),
    ("gear", 'i', 1),
    ("rpms", 'i', 1),
    ("steerAngle", 'f', 1),
    ("speedKmh", 'f', 1),
    ("velocity", 'f', 3),
    ("accG", 'f', 3),
    ("wheelSlip", 'f', 4),
    ("wheelLoad", 'f', 4),
    ("wheelsPressure", 'f', 4),
    ("wheelAngularSpeed", 'f', 4),
    ("tyreWear", 'f', 4),
    ("tyreDirtyLevel", 'f', 4),
    ("tyreCoreTemperature", 'f', 4),
    ("camberRAD", 'f', 4),
    ("suspensionTravel", 'f', 4),
    ("drs", 'f', 1),
    ("tc", 'f', 1),
    ("heading", 'f', 1),
    ("pitch", 'f', 1),
    ("roll", 'f', 1),
    ("cgHeight", 'f', 1),
    ("carDamage", 'f', 5),
    ("numberOfTyresOut", 'i', 1),
    ("pitLimiterOn", 'i', 1),
    ("abs", 'f', 1),
    ("kersCharge", 'f', 1),
    ("kersInput", 'f', 1),
    ("autoShifterOn", 'i', 1),
    ("rideHeight", 'f', 2),
    ("turboBoost", 'f', 1),
    ("ballast", 'f', 1),
    ("airDensity", 'f', 1),
    ("airTemp", 'f', 1),
    ("roadTemp", 'f', 1),
    ("localAngularVel", 'f', 3),
    ("finalFF", 'f', 1),
    ("performanceMeter", 'f', 1),
    ("engineBrake", 'i', 1),
    ("ersRecoveryLevel", 'i', 1),
    ("ersPowerLevel", 'i', 1),
    ("ersHeatCharging", 'i', 1),
    ("ersIsCharging", 'i', 1),
    ("kersCurrentKJ", 'f', 1),
    ("drsAvailable", 'i', 1),
    ("drsEnabled", 'i', 1),
    ("brakeTemp", 'f', 4),
    ("clutch", 'f', 1),
    ("tyreTempI", 'f', 4),
    ("tyreTempM", 'f', 4),
    ("tyreTempO", 'f', 4),
    ("isAIControlled", 'i', 1),
    ("tyreContactPoint", 'f', 12),
    ("tyreContactNormal", 'f', 12),
    ("tyreContactHeading", 'f', 12),
    ("brakeBias", 'f', 1),
    ("localVelocity", 'f', 3),
    ("P2PActivations", 'i', 1),
    ("P2PStatus", 'i', 1),
    ("currentMaxRpm", 'i', 1),
];

/// `sizeof(SPageFilePhysics)`.
pub const PAGE_SIZE: usize = 0x250;

/// One `acpmf_physics` page: the 148 values of [`PAGE_FIELDS`] as their 32-bit patterns.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PhysicsPage {
    pub words: Vec<u32>,
}

impl PhysicsPage {
    /// The page as the bytes of the shared memory (little endian).
    pub fn to_bytes(&self) -> Vec<u8> {
        self.words.iter().flat_map(|word| word.to_le_bytes()).collect()
    }

    /// Every value with its name: `name` for a single value, `name.<index>` for an array (the
    /// names `tools/car_oracle` records the game's page under, without the `page.` prefix).
    pub fn named(&self) -> Vec<(String, char, u32)> {
        let mut out = Vec::with_capacity(self.words.len());
        let mut at = 0;
        for (name, kind, count) in PAGE_FIELDS {
            for index in 0..count {
                let field = if count == 1 { name.to_string() } else { format!("{name}.{index}") };
                out.push((field, kind, self.words[at]));
                at += 1;
            }
        }
        out
    }

    /// One value by its name in [`PhysicsPage::named`].
    pub fn get(&self, name: &str) -> Option<u32> {
        self.named().into_iter().find(|(field, _, _)| field == name).map(|(_, _, word)| word)
    }
}

/// `cvttss2si`: toward zero; a NaN and anything outside the range of a 32-bit integer give
/// `i32::MIN`.
fn cvttss2si(x: f32) -> i32 {
    if x.is_nan() || x >= 2_147_483_648.0 || x < -2_147_483_648.0 {
        i32::MIN
    } else {
        x as i32
    }
}

/// AC's `SharedMemoryWriter` for the physics page, with the two things the game's writer
/// takes from the car's state snapshot.
#[derive(Clone, Debug, PartialEq)]
pub struct PhysicsPageWriter {
    /// `sharedMemories[2].packetId`: the number the next page gets.
    pub packet_id: i32,
    /// `sharedMemories[2].nullCounts`: the game writes no page during its first 300 calls
    /// with a car (0.9 s); it starts at 0. Set it to 300 for a writer that is warmed up (as
    /// `tools/car_oracle` does).
    pub null_counts: i32,
    /// `Car::ridePickupPoint`: front and rear, body axes (`car.ini [RIDE]`, z from the
    /// suspensions' base positions).
    pub ride_pickup_point: [Vec3f; 2],
    /// `physicsInfo.bumpStopsDn`: the lower bump stop of each wheel, taken when the car is
    /// attached. (`CarAvatar::initPhysics` stores it for double-wishbone wheels only and 0 for
    /// every other suspension class, whose page value is then the raw travel; the port has
    /// only double wishbones.)
    pub bump_stops_dn: [f32; 4],
    /// The cockpit's settings on the page (`engineBrake`, `ersRecoveryLevel`, `ersPowerLevel`,
    /// `ersHeatCharging`) are numbers of the car's avatar. `tools/car_oracle` hands the game's
    /// writer a zeroed avatar, so its recordings show 0 for all four: true here does the same.
    pub zero_cockpit: bool,
    /// `SetupManager::minimumHeight_m` (`car.ini [RULES] MIN_HEIGHT`; -1 without the section).
    pub minimum_height: f32,
    /// `CarPhysicsState::speed` of the previous snapshot, m/s.
    pub snapshot_speed: f32,
    /// `CarPhysicsState::rideHeight`
    pub ride_height: [f32; 2],
    /// `LapInvalidator::currentTyresOut`: -1 until the car's first step.
    pub current_tyres_out: i32,
}

impl PhysicsPageWriter {
    /// A writer attached to `car` (`SharedMemoryWriter::SharedMemoryWriter`, with the part of
    /// `Car::initCarData` that reads the ride-height pickup points).
    pub fn new(car: &RollingChassis, data_path: &Path) -> Result<PhysicsPageWriter, String> {
        let ini = IniReader::load(&data_path.join("car.ini"))?;
        let front = ini.get_float("RIDE", "PICKUP_FRONT_HEIGHT")?;
        let rear = ini.get_float("RIDE", "PICKUP_REAR_HEIGHT")?;
        let mut bump_stops_dn = [0.0f32; 4];
        for (wheel, value) in bump_stops_dn.iter_mut().enumerate() {
            *value = car.suspensions[wheel].base().bump_stop_dn;
        }
        Ok(PhysicsPageWriter {
            packet_id: 0,
            null_counts: 0,
            ride_pickup_point: [
                Vec3f::new(0.0, front, car.suspensions[0].get_base_position().z),
                Vec3f::new(0.0, rear, car.suspensions[2].get_base_position().z),
            ],
            bump_stops_dn,
            zero_cockpit: false,
            // `SetupManager::init`: kept only when it is above 0
            minimum_height: {
                let height = if ini.has_section("RULES") { ini.get_float("RULES", "MIN_HEIGHT")? } else { -1.0 };
                if height > 0.0 {
                    height
                } else {
                    -1.0
                }
            },
            snapshot_speed: 0.0,
            ride_height: [0.0; 2],
            current_tyres_out: -1,
        })
    }

    /// `Car::computeRideHeight` @ 0x14026fc90: the height of a pickup point above the plane
    /// `ground` (unit normal and offset), measured straight down.
    pub fn compute_ride_height(&self, car: &RollingChassis, index: usize, ground: &[f32; 4]) -> f32 {
        let m = car.core.get_world_matrix(car.body).m;
        let p = self.ride_pickup_point[index];
        let wx = ((p.y * m[1][0] + p.x * m[0][0]) + p.z * m[2][0]) + m[3][0];
        let wy = ((p.x * m[0][1] + p.y * m[1][1]) + p.z * m[2][1]) + m[3][1];
        let wz = ((p.x * m[0][2] + p.y * m[1][2]) + p.z * m[2][2]) + m[3][2];
        let denominator = (ground[0] * 0.0 + ground[1] * -1.0) + ground[2] * 0.0;
        let hit = if denominator < 0.0 || denominator > 0.0 {
            -((((ground[0] * wx + ground[1] * wy) + ground[2] * wz) + ground[3]) / denominator) * -1.0 + wy
        } else {
            0.0
        };
        wy - hit
    }

    /// The part of `Car::getPhysicsState` @ 0x140270d70 the page depends on: the ride heights
    /// over the plane through the first three tyres' contact points. Call it after every step,
    /// before [`PhysicsPageWriter::update_physics`] (the game's physics thread does both after
    /// each step).
    pub fn snapshot(&mut self, car: &RollingChassis) {
        let t = &car.tyres;
        let ground =
            plane4f(&t[0].unmodified_contact_point, &t[1].unmodified_contact_point, &t[2].unmodified_contact_point);
        // a standing car with a minimum-height rule shows an estimate from spring rates and
        // weight instead of the measured height (the speed is the previous snapshot's)
        if !(self.snapshot_speed >= 2.0) && self.minimum_height > 0.0 {
            self.ride_height = [self.eval_ride_height(car, 0), self.eval_ride_height(car, 1)];
        } else {
            for index in 0..2 {
                self.ride_height[index] = self.compute_ride_height(car, index, &ground);
            }
        }
        self.snapshot_speed = car.speed;
    }

    /// `RaceEngineer::evalFrontRideHeight` @ 0x14027a0f0 (`axle` 0) and
    /// `RaceEngineer::evalRearRideHeight` @ 0x14027a850 (`axle` 1): the ride height the car
    /// would settle at, from its weight on the axle, the wheel rate (the heave spring's when
    /// the wheels have none) in series with the tyre's spring, and the rod length.
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    pub fn eval_ride_height(&self, car: &RollingChassis, axle: usize) -> f32 {
        let mass = car.get_total_mass(true);
        // RaceEngineer::getWeightDistribution @ 0x14027cb90: the front axle's share
        let front_z = car.suspensions[0].get_base_position().z;
        let rear_z = car.suspensions[2].get_base_position().z;
        let front_share = 1.0 - front_z.abs() / (front_z - rear_z).abs();
        let share = if axle == 0 { front_share } else { 1.0 - front_share };
        let load = (share * mass) * 9.806;
        let (left, right) = (2 * axle, 2 * axle + 1);
        let (a, b) = (car.suspensions[left].base(), car.suspensions[right].base());
        let mut rate = (a.k + b.k) * 0.5;
        let mut rod = (a.rod_length + b.rod_length) * 0.5;
        if !(rate < 0.0 || rate > 0.0) {
            rate = car.heave_springs[axle].k;
            rod = car.heave_springs[axle].rod_length;
        }
        // Tyre::getDynamicK @ 0x140280560
        let dynamic_k = |wheel: usize| -> f32 {
            let tyre = &car.tyres[wheel];
            let k = (tyre.status.pressure_dynamic - tyre.model_data.pressure_ref) * tyre.model_data.pressure_spring_gain
                + tyre.data.k;
            if k >= 0.0 {
                k
            } else {
                0.0
            }
        };
        // front: right then left; rear: left then right
        let tyre_rate = if axle == 0 { (dynamic_k(1) + dynamic_k(0)) * 0.5 } else { (dynamic_k(2) + dynamic_k(3)) * 0.5 };
        let base = car.suspensions[left].get_base_position();
        let height = car.tyres[left].data.radius - base.y;
        let load = load * 0.5;
        let series = (tyre_rate * rate) / (tyre_rate + rate);
        let sag = load / series - rod;
        self.ride_pickup_point[axle].y + (height - sag)
    }

    /// The part of `LapInvalidator::step` @ 0x1402c0580 the page shows: how many tyres stand
    /// on a surface that is not track; 0 for five seconds after a contact with another car.
    /// Only the player's car has a lap invalidator that runs (the count stays -1 otherwise).
    /// Runs inside the car's step in the game; call it after the step.
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    pub fn step_tyres_out(&mut self, car: &RollingChassis) {
        if !car.env.is_first_car {
            return;
        }
        if !(car.physics_time - car.last_collision_with_car_time >= 5000.0) {
            self.current_tyres_out = 0;
            return;
        }
        let mut out = 0;
        for tyre in &car.tyres {
            if let Some(surface) = &tyre.surface_def {
                if !surface.is_valid_track {
                    out += 1;
                }
            }
        }
        self.current_tyres_out = out;
    }

    /// `SharedMemoryWriter::updatePhysics` @ 0x140186ef0: the page for the step that just
    /// ran. `None` during the warm-up.
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    pub fn update_physics(&mut self, car: &RollingChassis) -> Option<PhysicsPage> {
        if self.null_counts < 300 {
            self.null_counts += 1;
            return None;
        }
        let f = f32::to_bits;
        let b = |flag: bool| flag as u32;
        let mut w: Vec<u32> = Vec::with_capacity(PAGE_SIZE / 4);
        let drivetrain = car.drivetrain.as_deref();
        let engine = drivetrain.map(|d| d.engine());
        let aero = car.aero.as_deref().map(|a| a.base());
        let aids = car.aids.as_deref().map(|a| a.base());
        let c = &car.controls;

        w.push(self.packet_id as u32);
        self.packet_id = self.packet_id.wrapping_add(1);
        w.push(f(c.gas));
        w.push(f(c.brake));
        w.push(f(car.fuel as f32));
        w.push(drivetrain.map(|d| d.base().current_gear).unwrap_or(1) as u32);
        w.push(cvttss2si(drivetrain.map(|d| d.get_engine_rpm()).unwrap_or(0.0)) as u32);
        w.push(f(c.steer));
        w.push(f(car.speed * 3.6));
        let v = car.core.get_velocity(car.body);
        w.extend([v.x, v.y, v.z].map(f));
        w.extend([car.acc_g.x, car.acc_g.y, car.acc_g.z].map(f));
        w.extend(car.tyres.iter().map(|t| f(t.status.nd_slip)));
        w.extend(car.tyres.iter().map(|t| f(t.status.load)));
        w.extend(car.tyres.iter().map(|t| f(t.status.pressure_dynamic)));
        w.extend(car.tyres.iter().map(|t| f(t.status.angular_velocity)));
        w.extend(car.tyres.iter().map(|t| f(t.model_data.wear_curve.get_value(t.status.virtual_km as f32) * 100.0)));
        w.extend(car.tyres.iter().map(|t| f(t.status.dirty_level)));
        w.extend(car.tyres.iter().map(|t| f(t.thermal_model.core_temp)));
        w.extend(car.tyres.iter().map(|t| f(t.status.camber_rad)));
        for (wheel, suspension) in car.suspensions.iter().enumerate() {
            w.push(f(suspension.get_status().travel - self.bump_stops_dn[wheel]));
        }
        let drs = aero.map(|a| &a.drs);
        w.push(f(if drs.is_some_and(|d| d.is_present && d.is_active) { 1.0 } else { 0.0 }));
        let tc = aids.map(|a| &a.traction_control);
        w.push(f(match tc {
            Some(tc) if tc.is_active => tc.slip_ratio_limit,
            _ => 0.0,
        }));

        // heading, pitch and roll from the body's axes
        let m = car.core.get_world_matrix(car.body).m;
        let mut heading = atan2f(-m[2][0], m[2][2]);
        let sine = m[2][1];
        let pitch = asinf(if sine > 1.0 {
            1.0
        } else if sine >= -1.0 {
            sine
        } else {
            -1.0
        });
        let roll;
        // two `ucomiss` + `jne`: the degenerate case also for NaNs
        if !(m[0][1] < 0.0 || m[0][1] > 0.0) && !(0.0 < m[1][1] || 0.0 > m[1][1]) {
            heading = 0.0;
            roll = atan2f(m[1][0], m[0][0]);
        } else {
            roll = atan2f(-m[0][1], m[1][1]);
        }
        w.extend([heading, pitch, roll].map(f));

        // Car::getCGHeight @ 0x1402703b0
        let mut sum = 0.0f32;
        for tyre in &car.tyres {
            sum += tyre.contact_point.y;
        }
        w.push(f(car.core.get_position(car.body).y - sum * 0.25));
        w.extend(car.damage_zone_level.map(f));
        w.push(self.current_tyres_out as u32);
        w.push(b(aids.is_some_and(|a| a.speed_limiter.is_limiting)));
        let abs = aids.map(|a| &a.abs);
        w.push(f(match abs {
            Some(abs) if abs.is_present && abs.is_active => abs.slip_ratio_limit,
            _ => 0.0,
        }));
        // kersCharge, kersInput: zero for a car without KERS or ERS
        let (kers_charge, kers_input, kers_kj) = match (&car.ers, &car.kers) {
            // an ERS first (its charge is a double)
            (Some(ers), _) => (ers.charge as f32, ers.input, ers.current_j * 0.001),
            (None, Some(kers)) => (kers.charge, kers.input, kers.current_j * 0.001),
            (None, None) => (0.0, 0.0, 0.0),
        };
        w.extend([kers_charge, kers_input].map(f));
        w.push(b(car.auto_shifter.is_active));
        w.extend(self.ride_height.map(f));
        w.push(f(engine.map(|e| e.base().status.turbo_boost).unwrap_or(0.0)));
        w.push(f(car.ballast_kg));
        w.push(f(match aero {
            Some(aero) => aero.air_density,
            None => car.air_density,
        }));
        w.push(f(car.env.ambient_temperature));
        w.push(f(car.env.road_temperature));
        let spin = car.core.get_local_angular_velocity(car.body);
        w.extend([spin.x, spin.y, spin.z].map(f));
        // Car::getFinalFF @ 0x140270960: the device's global gain (1 for everything but a
        // wheel) times (lastFF * userFFGain)
        w.push(f(car.ff_global_gain * (car.last_ff * car.user_ff_gain)));
        // performanceMeter: the lap-time meter needs the track's racing line
        w.push(f(car.performance_split as f32));
        // engineBrake: the cockpit setting; ersRecoveryLevel, ersPowerLevel, ersHeatCharging,
        // ersIsCharging: only for a car with ERS. The first three are numbers of the game's
        // main thread (the car's avatar), `cockpit` here; the last is the car's own
        let cockpit = if self.zero_cockpit {
            super::chassis::CockpitState { engine_brake: 0, ers_power_index: 0, ers_recovery: 0, ers_heat_charging: false }
        } else {
            car.cockpit
        };
        w.push(cockpit.engine_brake as u32);
        match &car.ers {
            Some(ers) => w.extend([cockpit.ers_recovery as u32, cockpit.ers_power_index as u32, cockpit.ers_heat_charging as u32, ers.is_charging as u32]),
            None => w.extend([0u32; 4]),
        }
        // kersCurrentKJ
        w.push(f(kers_kj));
        w.push(b(drs.is_some_and(|d| d.is_present && d.is_available)));
        w.push(b(drs.is_some_and(|d| d.is_present && d.is_active)));
        let discs = car.brake_system.as_deref().map(|brakes| brakes.disc_temperatures()).unwrap_or([0.0; 4]);
        w.extend(discs.map(f));
        w.push(f(c.clutch));
        let imo: Vec<[f32; 3]> = car.tyres.iter().map(|t| t.thermal_model.get_imo()).collect();
        for stripe in 0..3 {
            w.extend(imo.iter().map(|t| f(t[stripe])));
        }
        // isAIControlled
        w.push(0);
        for pick in [0usize, 1, 2] {
            for tyre in &car.tyres {
                let v = match pick {
                    0 => tyre.contact_point,
                    1 => tyre.contact_normal,
                    _ => tyre.road_heading,
                };
                w.extend([v.x, v.y, v.z].map(f));
            }
        }
        w.push(f(car.brake_system.as_deref().map(|brakes| brakes.get_front_bias()).unwrap_or(0.0)));
        let local = car.core.get_local_velocity(car.body);
        w.extend([local.x, local.y, local.z].map(f));
        // P2PActivations, P2PStatus: only for a car with push-to-pass (the snapshot's bytes:
        // 3 while a push is on, 2 when one is available, 1 otherwise)
        match engine.and_then(|e| e.push_to_pass()).filter(|p2p| p2p.enabled) {
            Some(p2p) => {
                #[allow(clippy::neg_cmp_op_on_partial_ord)]
                let status = if p2p.active {
                    3
                } else if p2p.activations > 0 && !(p2p.cool_down_s >= p2p.time_accum) {
                    2
                } else {
                    1
                };
                w.extend([p2p.activations as u8 as u32, status]);
            }
            None => w.extend([0u32; 2]),
        }
        w.push(engine.map(|e| e.get_limiter_rpm()).unwrap_or(0) as u32);
        debug_assert_eq!(w.len() * 4, PAGE_SIZE);
        Some(PhysicsPage { words: w })
    }
}
