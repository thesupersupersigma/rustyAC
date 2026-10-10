// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! `FuelLapEvaluator` (0x48 bytes, `Car::fuelLapEvaluator`): how much fuel a lap takes and how
//! far the car has gone on how much fuel. `step` 0x14028d360 (the last thing of
//! `Car::stepComponents`), `getFuelLaps` 0x14028d260, `getFuelPerLap` 0x14028d290, the
//! handlers `init` 0x14028d2b0 registers (a lap completed 0x14028d180, a new session
//! 0x14028d140), `setIgnoreLap` 0x14028d350; `CarAvatar::getKmPerLiter` 0x1400d3510 reads it
//! for the dashboards' `FUEL_CONS`.

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FuelLapEvaluator {
    pub lap_count: i32,
    pub lap_start_fuel: f64,
    pub fuel_per_lap: f64,
    /// metres driven
    pub total_m: f64,
    /// litres used since the car first went faster than 5 m/s
    pub total_liters: f64,
    pub start_fuel: f64,
    pub old_fuel_per_lap: f64,
    pub ignore_lap: bool,
}

impl Default for FuelLapEvaluator {
    fn default() -> FuelLapEvaluator {
        FuelLapEvaluator::new()
    }
}

impl FuelLapEvaluator {
    /// As `Car::Car` 0x14026bf00 leaves it: no start fuel yet (-100).
    pub fn new() -> FuelLapEvaluator {
        FuelLapEvaluator { lap_count: 0, lap_start_fuel: 0.0, fuel_per_lap: 0.0, total_m: 0.0, total_liters: 0.0, start_fuel: -100.0, old_fuel_per_lap: 0.0, ignore_lap: false }
    }

    /// `FuelLapEvaluator::step` 0x14028d360. `speed`: `Car::getSpeed`, m/s; `fuel`: the tank,
    /// litres.
    pub fn step(&mut self, dt: f32, speed: f32, fuel: f64, in_pit_lane: bool) {
        self.total_m += (speed * dt) as f64;
        if -50.0 > self.start_fuel && speed > 5.0 {
            self.start_fuel = fuel;
        }
        self.total_liters = self.start_fuel - fuel;
        if in_pit_lane {
            self.ignore_lap = true;
        }
    }

    /// The handler of `Car::evOnLapCompleted` 0x14028d180.
    pub fn on_lap_completed(&mut self, fuel: f64, in_pit_lane: bool) {
        if !self.ignore_lap && !in_pit_lane && 0.0 < self.lap_start_fuel {
            self.lap_count += 1;
            self.fuel_per_lap = self.lap_start_fuel - fuel;
        }
        if !in_pit_lane {
            self.ignore_lap = false;
        }
        self.lap_start_fuel = fuel;
    }

    /// The handler of the physics' new-session event 0x14028d140.
    pub fn on_new_session(&mut self) {
        if self.fuel_per_lap != 0.0 {
            self.old_fuel_per_lap = self.fuel_per_lap;
        }
        self.lap_count = 0;
        self.lap_start_fuel = 0.0;
        self.fuel_per_lap = 0.0;
        self.ignore_lap = false;
    }

    /// `FuelLapEvaluator::getFuelLaps` 0x14028d260: the laps the tank lasts; -1 before a lap
    /// was measured.
    pub fn fuel_laps(&self, fuel: f64) -> f32 {
        if self.fuel_per_lap != 0.0 {
            (fuel / self.fuel_per_lap) as f32
        } else {
            -1.0
        }
    }

    /// `FuelLapEvaluator::getFuelPerLap` 0x14028d290.
    pub fn fuel_per_lap(&self) -> f32 {
        if self.lap_count > 1 {
            self.fuel_per_lap as f32
        } else {
            self.old_fuel_per_lap as f32
        }
    }

    /// `CarAvatar::getKmPerLiter` 0x1400d3510 (not a number before any fuel was used).
    pub fn km_per_liter(&self) -> f32 {
        (self.total_m * 0.001 / self.total_liters) as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fuel_use_counts_from_the_first_fast_step() {
        let mut e = FuelLapEvaluator::new();
        e.step(0.003, 1.0, 50.0, true);
        assert_eq!(e.start_fuel, -100.0);
        assert!(e.ignore_lap);
        // (before the start fuel is taken the litres are a nonsense -150: the display's range test hides it)
        assert_eq!(e.total_liters, -150.0);
        e.step(0.003, 40.0, 50.0, false);
        assert_eq!((e.start_fuel, e.total_liters), (50.0, 0.0));
        for _ in 0..1000 {
            e.step(0.003, 40.0, 49.5, false);
        }
        assert_eq!(e.total_liters, 0.5);
        let km = e.total_m * 0.001;
        assert_eq!(e.km_per_liter(), (km / 0.5) as f32);
        // a lap out of the pits does not count, the next one does
        e.on_lap_completed(49.5, false);
        assert_eq!((e.lap_count, e.fuel_laps(49.5)), (0, -1.0));
        e.on_lap_completed(47.0, false);
        assert_eq!((e.lap_count, e.fuel_per_lap), (1, 2.5));
        assert_eq!(e.fuel_laps(47.0), (47.0f64 / 2.5) as f32);
        e.on_new_session();
        assert_eq!((e.old_fuel_per_lap, e.fuel_per_lap, e.fuel_per_lap()), (2.5, 0.0, 2.5));
    }
}
