// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni). Not covered by rustyAC's licenses — see LICENSING.md.

//! `VanillaTyreThermalModel`: 1:1 port of AC's `TyreThermalModel` (acs.exe build
//! 0x5a55e7a8): 3 stripes x 12 elements of surface patches around the tyre plus one core
//! temperature. Operation order follows the disassembly; comparisons are written the way
//! the original branches.

#![allow(
    clippy::neg_cmp_op_on_partial_ord,
    clippy::manual_clamp,
    clippy::manual_range_contains,
    clippy::assign_op_pattern
)]

use super::{TyreCar, TyrePatchData};
use crate::curve::Curve;

/// `1 / (2 pi)` as the double literal the game multiplies the wheel phase by.
const INV_TWO_PI: f64 = 0.15915494309644432;
/// `2^63`, the pivot of the compiler's f64 -> u64 conversion.
const TWO_POW_63: f64 = 9_223_372_036_854_775_808.0;

/// AC's `TyreThermalPatch` (0x28 bytes).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TyreThermalPatch {
    /// `connections`: neighbours, as indices into the model's `patches`, in the order
    /// `buildTyre` adds them (the order matters: heat is exchanged one neighbour at a time).
    pub connections: Vec<usize>,
    /// `T`: temperature, deg C.
    pub t: f32,
    /// `inputT`: heat waiting to be applied by the next `step`.
    pub input_t: f32,
    /// `elementIndex`: position around the tyre.
    pub element_index: i32,
    /// `stripeIndex`: 0, 1, 2 across the tread.
    pub stripe_index: i32,
}

/// AC's `TyreThermalModel` (0xe0 bytes). The `Car*` member is not stored: the functions
/// that read it take the car as an argument.
#[derive(Clone, Debug)]
pub struct VanillaTyreThermalModel {
    pub elements: i32,
    pub stripes: i32,
    pub patches: Vec<TyreThermalPatch>,
    /// Wheel rotation angle, rad, wrapped at 100000.
    pub phase: f64,
    /// `patchData`
    pub patch_data: TyrePatchData,
    /// `coreTemp`
    pub core_temp: f32,
    /// `performanceCurve`: grip multiplier over temperature.
    pub performance_curve: Curve,
    /// `isActive`
    pub is_active: bool,
    /// `thermalMultD`: the current grip multiplier from temperature.
    pub thermal_mult_d: f32,
    /// `practicalTemp`: the temperature `thermalMultD` was looked up at.
    pub practical_temp: f32,
    /// `camberSpreadK`: how strongly camber moves heat towards one edge.
    pub camber_spread_k: f32,
    /// `coreTInput`: core heat waiting to be applied by the next `step`.
    pub core_t_input: f32,
}

impl Default for VanillaTyreThermalModel {
    /// `TyreThermalModel::TyreThermalModel` @ 0x14026df80
    fn default() -> VanillaTyreThermalModel {
        VanillaTyreThermalModel {
            elements: 0,
            stripes: 0,
            patches: Vec::new(),
            phase: 0.0,
            patch_data: TyrePatchData::default(),
            core_temp: 0.0,
            performance_curve: Curve::new(),
            is_active: true,
            thermal_mult_d: 1.0,
            practical_temp: 0.0,
            camber_spread_k: 1.4,
            core_t_input: 0.0,
        }
    }
}

/// `cvttsd2si`: truncation, with the "integer indefinite" value for NaN and out of range.
fn cvttsd2si(value: f64) -> i64 {
    if value.is_nan() || value >= TWO_POW_63 || value < -TWO_POW_63 {
        i64::MIN
    } else {
        value as i64
    }
}

/// The compiler's `(unsigned __int64)double`.
fn f64_to_u64(mut value: f64) -> u64 {
    let mut offset = 0i64;
    if value >= TWO_POW_63 {
        value -= TWO_POW_63;
        if value < TWO_POW_63 {
            offset = i64::MIN;
        }
    }
    cvttsd2si(value).wrapping_add(offset) as u64
}

/// The clamp to [-1, 1] both camber terms use (a NaN ends up as -1).
fn clamp_camber(x: f32) -> f32 {
    if x > 1.0 {
        1.0
    } else if x >= -1.0 {
        x
    } else {
        -1.0
    }
}

impl VanillaTyreThermalModel {
    /// `TyreThermalModel::init` @ 0x1402ae170. Parameter names are the PDB's.
    pub fn init(&mut self, a_elements: i32, a_stripes: i32, car: Option<&dyn TyreCar>) {
        self.elements = a_elements;
        self.stripes = a_stripes;
        self.phase = 0.0;
        self.core_temp = match car {
            Some(car) => car.ambient_temperature(),
            None => 26.0,
        };
        self.is_active = true;
        self.build_tyre(car);
    }

    /// `TyreThermalModel::buildTyre` @ 0x1402ad300: creates the patches and links each one
    /// to the stripe before it, the element before it and, for the last element, the first
    /// one (closing the ring). Every link is stored on both ends.
    pub fn build_tyre(&mut self, car: Option<&dyn TyreCar>) {
        self.patches.resize(
            (self.elements * self.stripes) as usize,
            TyreThermalPatch::default(),
        );
        for stripe in 0..self.stripes {
            for element in 0..self.elements {
                let this = self.patch_index(stripe, element);
                self.patches[this].element_index = element;
                self.patches[this].stripe_index = stripe;
                self.patches[this].t = match car {
                    Some(car) => car.ambient_temperature(),
                    None => 26.0,
                };
                if stripe > 0 {
                    let other = self.patch_index(stripe - 1, element);
                    self.patches[this].connections.push(other);
                    self.patches[other].connections.push(this);
                }
                if element > 0 {
                    let other = self.patch_index(stripe, element - 1);
                    self.patches[this].connections.push(other);
                    self.patches[other].connections.push(this);
                }
                if element == self.elements - 1 {
                    let other = self.patch_index(stripe, 0);
                    self.patches[this].connections.push(other);
                    self.patches[other].connections.push(this);
                }
            }
        }
    }

    /// `TyreThermalModel::getPatchAt` @ 0x1402adfb0, as an index. `x` is the stripe, `y`
    /// the element (PDB names). Out of range is a "CRITICAL ERROR" crash in the game.
    pub fn patch_index(&self, x: i32, y: i32) -> usize {
        assert!(
            x >= 0 && x < self.stripes && y >= 0 && y < self.elements,
            "ERROR, CANNOT FIND PATCH AT:{x} {y}"
        );
        (self.elements * x + y) as usize
    }

    /// The element currently on the ground: `(u64)(phase / 2pi * elements) % elements`.
    fn contact_element(&self) -> i32 {
        let turns = self.phase * INV_TWO_PI * self.elements as f64;
        (f64_to_u64(turns) % (self.elements as i64 as u64)) as i32
    }

    /// `TyreThermalModel::reset` @ 0x1402ae1c0
    pub fn reset(&mut self, car: Option<&dyn TyreCar>) {
        self.core_temp = match car {
            Some(car) => car.ambient_temperature(),
            None => 26.0,
        };
        for patch in &mut self.patches {
            patch.t = self.core_temp;
        }
        self.phase = 0.0;
    }

    /// `TyreThermalModel::setTemperature` @ 0x1402ae310
    pub fn set_temperature(&mut self, temp: f32) {
        self.core_temp = temp;
        for patch in &mut self.patches {
            patch.t = temp;
        }
    }

    /// `TyreThermalModel::getCorrectedD` @ 0x1402adbc0 (`camberRAD` is not used).
    pub fn get_corrected_d(&self, d: f32, _camber_rad: f32) -> f32 {
        if self.is_active {
            d * self.thermal_mult_d
        } else {
            d
        }
    }

    /// `TyreThermalModel::addThermalCoreInput` @ 0x1402ad070
    pub fn add_thermal_core_input(&mut self, temp: f32) {
        self.core_t_input += temp;
    }

    /// `TyreThermalModel::addThermalInput` @ 0x1402ad090: queues `temp` (plus the road
    /// temperature) on the three patches touching the ground, split by camber (`xpos`) and
    /// pressure (`pressureRel`). Parameter names are the PDB's.
    pub fn add_thermal_input(
        &mut self,
        xpos: f32,
        pressure_rel: f32,
        temp: f32,
        car: Option<&dyn TyreCar>,
    ) {
        let temp = temp
            + match car {
                Some(car) => car.road_temperature(),
                None => 26.0,
            };
        let spread = clamp_camber(xpos * self.camber_spread_k);
        let inner = spread + 1.0;
        let pressure_gain = pressure_rel * -0.5 + 1.0;
        let pressure_edge = pressure_rel * 0.1;
        let half_edge = pressure_edge * 0.5;

        let patch = self.patch_index(0, self.contact_element());
        self.patches[patch].input_t =
            (inner - half_edge) * pressure_gain * temp + self.patches[patch].input_t;

        let patch = self.patch_index(1, self.contact_element());
        self.patches[patch].input_t =
            (pressure_edge + 1.0) * pressure_gain * temp + self.patches[patch].input_t;

        let patch = self.patch_index(2, self.contact_element());
        self.patches[patch].input_t =
            (1.0 - spread - half_edge) * pressure_gain * temp + self.patches[patch].input_t;
    }

    /// `TyreThermalModel::getCurrentCPTemp` @ 0x1402adbe0: camber-weighted mean of the
    /// three patches on the ground.
    pub fn get_current_cp_temp(&self, camber: f32) -> f32 {
        let spread = clamp_camber(camber * self.camber_spread_k);
        let t0 = self.patches[self.patch_index(0, self.contact_element())].t;
        let t1 = self.patches[self.patch_index(1, self.contact_element())].t;
        let t2 = self.patches[self.patch_index(2, self.contact_element())].t;
        ((spread + 1.0) * t0 + t1 + (1.0 - spread) * t2) * 0.333_333_34
    }

    /// `TyreThermalModel::getPracticalTemp` @ 0x1402ae140
    pub fn get_practical_temp(&self, camber: f32) -> f32 {
        (self.get_current_cp_temp(camber) - self.core_temp) * 0.25 + self.core_temp
    }

    /// `TyreThermalModel::getIMO` @ 0x1402addb0: mean temperature of each stripe (the game
    /// hard-codes 3 stripes of 12).
    pub fn get_imo(&self) -> [f32; 3] {
        let mut out = [0.0f32; 3];
        for (stripe, mean) in out.iter_mut().enumerate() {
            for element in 0..12 {
                *mean += self.patches[self.patch_index(stripe as i32, element)].t;
            }
            *mean *= 0.083_333_336;
        }
        out
    }

    /// `TyreThermalModel::getAvgSurfaceTemp` @ 0x1402adb90
    pub fn get_avg_surface_temp(&self) -> f32 {
        let mut sum = 0.0;
        for patch in &self.patches {
            sum += patch.t;
        }
        sum * 0.027_777_778
    }

    /// `TyreThermalModel::step` @ 0x1402ae340. Parameter names are the PDB's.
    pub fn step(
        &mut self,
        dt: f32,
        angular_speed: f32,
        camber_rad: f32,
        car: Option<&dyn TyreCar>,
    ) {
        self.phase += (dt * angular_speed) as f64;
        if self.phase > 100000.0 {
            self.phase -= 100000.0;
        }
        if !(self.phase >= 0.0) {
            self.phase += 100000.0;
        }

        // the core never takes less than the ambient temperature as its input
        let floor = match car {
            Some(car) => car.ambient_temperature(),
            None => 26.0,
        };
        let core_input = match car {
            Some(_) if floor > self.core_t_input => floor,
            None if !(floor < self.core_t_input) => floor,
            _ => self.core_t_input,
        };
        self.core_t_input = 0.0;
        self.core_temp = (core_input - self.core_temp)
            * (dt * self.patch_data.internal_core_transfer)
            + self.core_temp;

        let data = self.patch_data;
        match car {
            Some(car) => {
                let speed = car.get_speed();
                let patch_core = dt * data.patch_core_transfer;
                let cooling =
                    (speed * speed * data.cool_factor_gain + 1.0) * data.surface_transfer * dt;
                for i in 0..self.patches.len() {
                    let ambient = car.ambient_temperature();
                    let patch = &mut self.patches[i];
                    if patch.input_t > ambient {
                        patch.t =
                            (patch.input_t - patch.t) * (dt * data.surface_transfer) + patch.t;
                    } else {
                        patch.t = (ambient - patch.t) * cooling + patch.t;
                    }
                    patch.input_t = 0.0;
                    self.exchange_with_neighbours(i, dt);
                    let patch = &mut self.patches[i];
                    patch.t = (self.core_temp - patch.t) * patch_core + patch.t;
                    self.core_temp = (patch.t - self.core_temp) * patch_core + self.core_temp;
                }
            }
            None => {
                for i in 0..self.patches.len() {
                    let patch = &mut self.patches[i];
                    let (rate, difference) = if patch.input_t > 26.0 {
                        (dt * data.surface_transfer, patch.input_t - patch.t)
                    } else {
                        (
                            (data.cool_factor_gain * 300.0 + 1.0) * data.surface_transfer * dt,
                            26.0 - patch.t,
                        )
                    };
                    patch.input_t = 0.0;
                    patch.t = rate * difference + patch.t;
                    self.exchange_with_neighbours(i, dt);
                    let patch = &mut self.patches[i];
                    patch.t =
                        (self.core_temp - patch.t) * (dt * data.patch_core_transfer) + patch.t;
                    self.core_temp = (patch.t - self.core_temp) * (dt * data.patch_core_transfer)
                        + self.core_temp;
                }
            }
        }

        if self.is_active && self.performance_curve.get_count() > 0 {
            let practical =
                (self.get_current_cp_temp(camber_rad) - self.core_temp) * 0.25 + self.core_temp;
            self.practical_temp = practical;
            self.thermal_mult_d = self.performance_curve.get_value(practical);
        }
    }

    /// The neighbour loop of `step`: one connection at a time, each seeing the result of
    /// the one before.
    fn exchange_with_neighbours(&mut self, i: usize, dt: f32) {
        let mut t = self.patches[i].t;
        for c in 0..self.patches[i].connections.len() {
            let other = self.patches[self.patches[i].connections[c]].t;
            t += (other - t) * (dt * self.patch_data.patch_transfer);
            self.patches[i].t = t;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_tyre_links_a_closed_grid() {
        let mut model = VanillaTyreThermalModel::default();
        model.init(12, 3, None);
        assert_eq!(model.patches.len(), 36);
        // every patch: two neighbours around the ring, plus one or two across the tread
        for patch in &model.patches {
            let expected = if patch.stripe_index == 1 { 4 } else { 3 };
            assert_eq!(patch.connections.len(), expected, "{patch:?}");
            assert_eq!(patch.t, 26.0);
        }
        // first patch: linked by element 1, then by the last element closing the ring,
        // then by the stripe next to it
        assert_eq!(model.patches[0].connections, [1, 11, 12]);
    }

    #[test]
    fn unsigned_conversion_matches_the_compiler() {
        assert_eq!(f64_to_u64(5.9), 5);
        assert_eq!(f64_to_u64(1e19), 10_000_000_000_000_000_000);
        assert_eq!(f64_to_u64(-1.0), u64::MAX);
        assert_eq!(f64_to_u64(f64::NAN), 1 << 63);
    }
}
