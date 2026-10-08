// SPDX-License-Identifier: GPL-3.0-or-later

//! `grip_probe`: how fast one stretch of a track can be taken, measured with nobody at the
//! controls and without a clock (Task 16, the grip question).
//!
//! A line follower (the same pure pursuit as `--autodrive`) drives one lap at a safe pace
//! and then takes a stretch of the track (by default Spa's Bus Stop) at a higher "pace":
//! the share of the grip it plans its speed and its braking with. A sweep raises the pace
//! until the car no longer makes it (spin, all four tyres off the track, or it never
//! arrives); the last pace that worked gives the minimum speed and the peak lateral g the
//! car can do there. Run with one item of the session changed (`--set`), the difference
//! says what that item is worth in grip.
//!
//! ```text
//! cargo run --release -p rustyac-game --example grip_probe -- --sweep 0.9 2.2 0.05
//! cargo run --release -p rustyac-game --example grip_probe -- --sweep 0.9 2.2 0.05 --set air_density=1.165
//! cargo run --release -p rustyac-game --example grip_probe -- --pace 1.2 --csv re/scratch/task16/bus_stop.csv
//! ```
//!
//! `--set key=value` (several allowed): `air`, `road` (deg C), `grip` (1 = 100 %),
//! `air_density` (kg/m3; not Assetto Corsa, Custom Shaders Patch's thin air), `blankets`
//! (0/1), `wind` (km/h, from 0 deg) and `wind_dir`, `tc`, `abs` (0 off, 1 as the car has it,
//! 2 on), `stability` (percent), `auto_clutch` (0/1), `wear_rate`, `fuel_rate`, `ballast`
//! (kg), `fuel` (litres at the start), `pressure` (psi added to every tyre's static
//! pressure), `pressure_static` (psi) and `pressure_gain` (psi per deg C of core temperature
//! above 26; the game's is 0.16), `setup` (a saved setup file), `steer_rate` (full locks per second the driver
//! may turn the wheel: a pad's limit; 0 = none), `base_pace` (the pace outside the stretch),
//! `brake_pace` (the share of the braking planned with inside it, default 0.85).

use std::collections::VecDeque;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;

use rustyac_game::input_file::SimSetup;
use rustyac_game::sim::{CarProbe, DriverSource, GameSim, SpawnSequence};
use rustyac_game::view::CarView;
use rustyac_physics::car::telemetry::PAGE_FIELDS;
use rustyac_physics::car::{CarControls, CarControlsInput};
use rustyac_physics::session::wind_from_kmh;
use rustyac_physics::track::{AiSpline, Track};

/// Seconds of one physics step.
const DT: f64 = 0.003;
/// It never plans for more than this, m/s.
const TOP_SPEED: f32 = 95.0;

/// What the driver is asked to do.
#[derive(Clone, Copy, Debug)]
struct Plan {
    /// The pace outside the stretch (the `--autodrive` driver's is 0.80).
    base_pace: f32,
    /// The pace inside it.
    zone_pace: f32,
    /// The stretch: from / to along the lap, 0..1.
    zone: (f32, f32),
    /// Full locks per second the wheel may be turned; 0: no limit.
    steer_rate: f32,
    /// Stand on the brake at the spawn point (`--standing`).
    standing: bool,
    /// The share of the braking the driver plans with inside the stretch (the pace is for
    /// the bends alone there, so that a sweep finds the cornering limit, not the braking one).
    brake_pace: f32,
}

struct ProbeDriver {
    plan: Plan,
    track: Option<Arc<Track>>,
    probe: CarProbe,
    steer: f32,
    /// The car has crossed the line once: the stretch is taken at its pace from now on (the
    /// hot-lap start is inside the stretch, and the first pass is from a standstill).
    flying: bool,
    last_npos: f32,
}

impl ProbeDriver {
    fn new(plan: Plan) -> ProbeDriver {
        ProbeDriver { plan, track: None, probe: CarProbe::default(), steer: 0.0, flying: false, last_npos: -1.0 }
    }

    fn in_zone(&self, npos: f32) -> bool {
        self.flying && npos >= self.plan.zone.0 && npos <= self.plan.zone.1
    }

    fn pace_at(&self, npos: f32) -> f32 {
        if self.in_zone(npos) {
            self.plan.zone_pace
        } else {
            self.plan.base_pace
        }
    }

    /// The speed the bends ahead allow, m/s (the `--autodrive` driver's rule, with the pace
    /// of the place each bend is at, and the braking of the place the car is at).
    fn allowed_speed(&self, spline: &AiSpline, index: usize, npos: f32) -> f32 {
        let pace_here = self.pace_at(npos);
        let in_zone = self.in_zone(npos);
        let points = &spline.spline.points;
        let n = points.len();
        let here = points[index % n].point_length;
        let length = spline.length();
        let mut allowed = TOP_SPEED;
        let mut i = index;
        while i < index + n {
            let mut ahead = points[i % n].point_length - here;
            if ahead < 0.0 {
                ahead += length;
            }
            if ahead > 320.0 {
                break;
            }
            let pace = self.pace_at(points[i % n].point_length / length);
            let k = curvature(spline, i, 5).abs();
            let (a0, a1) = (14.0 * pace, 0.0036 * pace);
            let corner = if k <= a1 * 1.08 { TOP_SPEED } else { (a0 / (k - a1)).sqrt() };
            let reachable = if in_zone {
                // braking that grows with the square of the speed (the wings): from
                // v dv/ds = -(b0 + b1 v^2)
                let (b0, b1) = (17.0 * self.plan.brake_pace, 0.004 * self.plan.brake_pace);
                (((b0 + b1 * corner * corner) * (2.0 * b1 * ahead).exp() - b0) / b1).sqrt()
            } else {
                (corner * corner + 2.0 * 21.0 * pace_here * ahead).sqrt()
            };
            allowed = allowed.min(reachable);
            i += 3;
        }
        allowed
    }
}

/// Curvature (1/m, positive to the left) of the line around point `i`, seen from above.
fn curvature(spline: &AiSpline, i: usize, span: usize) -> f32 {
    let points = &spline.spline.points;
    let n = points.len();
    let a = points[(i + n - span) % n].point;
    let b = points[i % n].point;
    let c = points[(i + span) % n].point;
    let (abx, abz, bcx, bcz) = (b[0] - a[0], b[2] - a[2], c[0] - b[0], c[2] - b[2]);
    let cross = abz * bcx - abx * bcz;
    let (ab, bc, ac) = ((abx * abx + abz * abz).sqrt(), (bcx * bcx + bcz * bcz).sqrt(), ((c[0] - a[0]).powi(2) + (c[2] - a[2]).powi(2)).sqrt());
    if ab * bc * ac < 1e-6 {
        0.0
    } else {
        2.0 * cross / (ab * bc * ac)
    }
}

impl DriverSource for ProbeDriver {
    fn set_track(&mut self, track: &Arc<Track>) {
        self.track = Some(Arc::clone(track));
    }

    fn wants_probe(&self) -> bool {
        true
    }

    fn set_probe(&mut self, probe: &CarProbe) {
        self.probe = *probe;
    }

    fn acquire(&mut self, controls: &mut CarControls, dt: f32, input: &CarControlsInput) {
        *controls = CarControls { clutch: 1.0, ..CarControls::default() };
        if self.plan.standing {
            controls.brake = 1.0;
            return;
        }
        let Some(track) = self.track.clone() else { return };
        let Some(spline) = track.ai_spline.as_ref() else { return };
        if spline.point_count() < 16 {
            return;
        }
        let car = self.probe;
        let length = spline.length();
        let npos = if car.npos >= 0.0 { car.npos } else { spline.spline.world_to_spline(&car.position, -1) };
        let index = spline.spline.closest_point_index(&car.position) as usize;
        if self.last_npos > 0.9 && npos < 0.1 {
            self.flying = true;
        }
        self.last_npos = npos;

        // the wheel: at a point of the line ahead
        let look = (5.0 + 0.22 * input.speed).clamp(6.0, 26.0);
        let p = spline.spline.spline_to_world(spline.spline.wrap_position(npos + look / length));
        let d = [p[0] - car.position[0], p[1] - car.position[1], p[2] - car.position[2]];
        let to_left = d[0] * car.left[0] + d[1] * car.left[1] + d[2] * car.left[2];
        let to_front = d[0] * car.forward[0] + d[1] * car.forward[1] + d[2] * car.forward[2];
        let alpha = to_left.atan2(to_front.max(0.5));
        let distance = (to_left * to_left + to_front * to_front).sqrt().max(1.0);
        let wheel_angle = (2.0 * car.wheelbase * alpha.sin() / distance).atan();
        // positive turns right
        let wanted = (-wheel_angle / car.max_wheel_angle.max(0.05)).clamp(-1.0, 1.0);
        if self.plan.steer_rate > 0.0 {
            let most = self.plan.steer_rate * 2.0 * dt;
            self.steer += (wanted - self.steer).clamp(-most, most);
        } else {
            self.steer = wanted;
        }
        controls.steer = self.steer;

        // the pedals
        let error = self.allowed_speed(spline, index, npos) - input.speed;
        if error >= 0.0 {
            controls.gas = (0.25 + error * 0.4).clamp(0.0, 1.0);
        } else {
            controls.brake = (-error * 0.25).clamp(0.0, 1.0);
        }
    }
}

/// What a run changes in the session.
#[derive(Clone, Debug, Default)]
struct Variant {
    settings: Vec<(String, String)>,
}

impl Variant {
    fn label(&self) -> String {
        if self.settings.is_empty() {
            "baseline".to_string()
        } else {
            self.settings.iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join(",")
        }
    }

    fn number(key: &str, value: &str) -> Result<f32, String> {
        value.parse::<f32>().map_err(|e| format!("--set {key}={value}: {e}"))
    }

    /// The part set before the car is built.
    fn apply_to_setup(&self, setup: &mut SimSetup, plan: &mut Plan) -> Result<(), String> {
        let (mut abs, mut tc, mut stability) = (1, 1, 0.0);
        let (mut wind, mut wind_dir) = (0.0, 0.0);
        for (key, value) in &self.settings {
            let n = || Variant::number(key, value);
            match key.as_str() {
                "air" => setup.env.ambient_temperature = n()?,
                "road" => setup.env.road_temperature = n()?,
                "grip" => setup.env.dynamic_grip_level = n()?,
                "air_density" => setup.env.air_density_override = Some(n()?),
                "blankets" => setup.env.allow_tyre_blankets = n()? != 0.0,
                "wear_rate" => setup.env.tyre_consumption_rate = n()?,
                "fuel_rate" => setup.env.fuel_consumption_rate = n()?,
                "wind" => wind = n()?,
                "wind_dir" => wind_dir = n()?,
                "abs" => abs = n()? as i32,
                "tc" => tc = n()? as i32,
                "stability" => stability = n()?,
                "auto_clutch" => setup.auto_clutch = n()? != 0.0,
                "ballast" => setup.session.ballast_kg = n()?,
                "setup" => setup.session.setup_file = Some(PathBuf::from(value)),
                "steer_rate" => plan.steer_rate = n()?,
                "base_pace" => plan.base_pace = n()?,
                "brake_pace" => plan.brake_pace = n()?,
                "fuel" | "pressure" | "pressure_static" | "pressure_gain" => {}
                other => return Err(format!("--set {other}: not a setting of grip_probe")),
            }
        }
        setup.session.assists = Some((abs, tc, stability));
        if wind > 0.0 {
            setup.session.wind_speed = wind_from_kmh(wind);
            setup.session.wind_direction_deg = wind_dir;
        }
        Ok(())
    }

    /// The part set on the car once it stands at its spawn point.
    fn apply_to_car(&self, sim: &mut GameSim) -> Result<(), String> {
        for (key, value) in &self.settings {
            match key.as_str() {
                "fuel" => sim.car.car.fuel = Variant::number(key, value)? as f64,
                "pressure" => {
                    let delta = Variant::number(key, value)?;
                    for tyre in &mut sim.car.car.tyres {
                        tyre.status.pressure_static += delta;
                    }
                }
                "pressure_static" => {
                    let static_psi = Variant::number(key, value)?;
                    for tyre in &mut sim.car.car.tyres {
                        tyre.status.pressure_static = static_psi;
                    }
                }
                // psi per deg C of core temperature above 26 (the game's own: 0.16)
                "pressure_gain" => {
                    let gain = Variant::number(key, value)?;
                    for tyre in &mut sim.car.car.tyres {
                        tyre.pressure_temperature_gain = gain;
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }
}

/// The numbers of one pass of the stretch.
#[derive(Clone, Debug, Default)]
struct Pass {
    pace: f32,
    /// Empty: the car made it.
    failure: String,
    entry_kmh: f32,
    min_kmh: f32,
    exit_kmh: f32,
    /// Peak lateral g to the left and to the right, averaged over 10 m.
    peak_left_g: f32,
    peak_right_g: f32,
    /// Peak braking g, averaged over 10 m.
    peak_brake_g: f32,
    /// The largest angle between where the car points and where it goes, degrees.
    max_beta_deg: f32,
    seconds: f32,
    max_tyres_out: i32,
    /// The largest slip over the slip at peak grip, front and rear (mean of the axle).
    max_slip_front: f32,
    max_slip_rear: f32,
    /// At the start of the measured part: tyre core temperatures (LF, RF, LR, RR), fuel, grip.
    core_temp: [f32; 4],
    fuel: f32,
    air_density: f32,
    lap_ms: u32,
}

/// The page's values by name.
struct PageIndex {
    names: Vec<(String, usize)>,
}

impl PageIndex {
    fn new() -> PageIndex {
        let mut names = Vec::new();
        let mut at = 0;
        for (name, _, count) in PAGE_FIELDS {
            names.push((name.to_string(), at));
            at += count;
        }
        PageIndex { names }
    }

    fn at(&self, name: &str) -> usize {
        self.names.iter().find(|(n, _)| n == name).map(|(_, at)| *at).unwrap_or_else(|| panic!("no page field {name}"))
    }
}

struct RunOptions {
    car: String,
    track: String,
    plan: Plan,
    /// The measured part of the stretch.
    measure: (f32, f32),
    variant: Variant,
    csv: Option<PathBuf>,
}

fn run(options: &RunOptions) -> Result<Pass, String> {
    let mut plan = options.plan;
    let mut setup = SimSetup { car: options.car.clone(), track: options.track.clone(), auto_shifter: true, ..SimSetup::default() };
    // the conditions of the owner's race.ini, without what the game draws at random
    setup.env.ambient_temperature = 14.0;
    setup.env.road_temperature = 20.0;
    options.variant.apply_to_setup(&mut setup, &mut plan)?;
    let mut sim = GameSim::new(setup, Box::new(SpawnSequence::new(ProbeDriver::new(plan), true)))?;
    options.variant.apply_to_car(&mut sim)?;
    if options.csv.is_some() {
        // the start of the session, for the record
        let car = &sim.car.car;
        if let Some(aids) = &car.aids {
            let tc = &aids.base().traction_control;
            let (mode, modes) = tc.get_current_mode();
            println!("  traction control: present {}, active {}, level {mode} of {modes}, slip limit {}", tc.is_present, tc.is_active, tc.slip_ratio_limit);
        }
        println!(
            "  start: fuel {} l, tyres {} at {:.1} C core, {:.2} psi static, auto clutch {}/{}, auto blip {}, auto shifter {}",
            car.fuel,
            car.tyres[0].compound_defs[car.tyres[0].current_compound_index as usize].name,
            car.tyres[0].thermal_model.core_temp,
            car.tyres[0].status.pressure_static,
            car.autoclutch.use_auto_on_start,
            car.autoclutch.use_auto_on_change,
            car.auto_blip.is_active,
            car.auto_shifter.is_active
        );
    }

    let index = PageIndex::new();
    let f = |sim: &GameSim, at: usize| sim.car.physics_page().map(|page| f32::from_bits(page.words[at])).unwrap_or(0.0);
    let (i_load, i_slip, i_core, i_pressure, i_local, i_density, i_ride, i_acc) = (
        index.at("wheelLoad"),
        index.at("wheelSlip"),
        index.at("tyreCoreTemperature"),
        index.at("wheelsPressure"),
        index.at("localVelocity"),
        index.at("airDensity"),
        index.at("rideHeight"),
        index.at("accG"),
    );
    let mut csv = match &options.csv {
        Some(path) => {
            let mut file = std::io::BufWriter::new(std::fs::File::create(path).map_err(|e| format!("{}: {e}", path.display()))?);
            writeln!(
                file,
                "t,trackPos,speedKmh,gear,gas,brake,steerAngle,accG_x,accG_z,beta_deg,wheelLoad_fl,wheelLoad_fr,wheelLoad_rl,wheelLoad_rr,wheelSlip_fl,wheelSlip_fr,wheelSlip_rl,wheelSlip_rr,tyreCoreTemperature_fl,tyreCoreTemperature_fr,tyreCoreTemperature_rl,tyreCoreTemperature_rr,wheelsPressure_fl,wheelsPressure_fr,wheelsPressure_rl,wheelsPressure_rr,rideHeight_0,rideHeight_1,airDensity,fuel,numberOfTyresOut,surface_fl,surface_fr,surface_rl,surface_rr"
            )
            .map_err(|e| e.to_string())?;
            Some(file)
        }
        None => None,
    };

    let mut pass = Pass { pace: plan.zone_pace, min_kmh: f32::MAX, ..Pass::default() };
    let (mut last_position, mut flying, mut measuring, mut measured) = (-1.0f32, false, false, false);
    let mut entered_at = 0.0f64;
    // the last 10 m: (distance, lateral g, longitudinal g)
    let mut window: VecDeque<(f64, f32, f32)> = VecDeque::new();
    let (mut sum_lat, mut sum_long, mut travelled) = (0.0f64, 0.0f64, 0.0f64);
    let limit_steps = (420.0 / DT) as u64;
    while sim.steps < limit_steps {
        sim.step()?;
        let view = CarView::capture(&sim, 0.0);
        let position = view.lap.position;
        let seconds = sim.sim_seconds();
        if !flying && seconds > 40.0 && last_position >= 0.0 && last_position < plan.zone.0 && position >= plan.zone.0 {
            flying = true;
            entered_at = seconds;
        }
        let crossed_line = flying && last_position > 0.9 && position < 0.1;
        last_position = position;
        if !flying {
            continue;
        }
        let (lateral, forward) = (f(&sim, i_local), f(&sim, i_local + 2));
        let beta = if view.speed_kmh > 36.0 { lateral.atan2(forward.abs().max(0.1)).to_degrees() } else { 0.0 };
        let (lat_g, long_g) = (f(&sim, i_acc), f(&sim, i_acc + 2));
        if let Some(file) = &mut csv {
            let page = |at: usize| f(&sim, at);
            write!(
                file,
                "{:.3},{:.6},{:.3},{},{:.3},{:.3},{:.4},{:.4},{:.4},{:.3}",
                seconds,
                position,
                view.speed_kmh,
                view.gear - 1,
                view.gas,
                view.brake,
                view.steer,
                lat_g,
                long_g,
                beta
            )
            .map_err(|e| e.to_string())?;
            for at in [i_load, i_slip, i_core, i_pressure] {
                for wheel in 0..4 {
                    write!(file, ",{:.4}", page(at + wheel)).map_err(|e| e.to_string())?;
                }
            }
            write!(file, ",{:.5},{:.5},{:.5},{:.3},{}", page(i_ride), page(i_ride + 1), page(i_density), view.fuel, view.lap.tyres_out).map_err(|e| e.to_string())?;
            for surface in &view.surfaces {
                write!(file, ",{}", surface.as_str()).map_err(|e| e.to_string())?;
            }
            writeln!(file).map_err(|e| e.to_string())?;
        }
        // the 10 m average
        travelled += view.speed_kmh as f64 / 3.6 * DT;
        window.push_back((travelled, lat_g, long_g));
        sum_lat += lat_g as f64;
        sum_long += long_g as f64;
        while window.front().is_some_and(|front| travelled - front.0 > 10.0) {
            let (_, a, b) = window.pop_front().unwrap_or_default();
            sum_lat -= a as f64;
            sum_long -= b as f64;
        }
        let (mean_lat, mean_long) = ((sum_lat / window.len() as f64) as f32, (sum_long / window.len() as f64) as f32);

        if position >= options.measure.0 && position <= options.measure.1 {
            if !measuring {
                measuring = true;
                measured = true;
                pass.entry_kmh = view.speed_kmh;
                pass.seconds = seconds as f32;
                for wheel in 0..4 {
                    pass.core_temp[wheel] = f(&sim, i_core + wheel);
                }
                pass.fuel = view.fuel;
                pass.air_density = f(&sim, i_density);
            }
            pass.min_kmh = pass.min_kmh.min(view.speed_kmh);
            pass.exit_kmh = view.speed_kmh;
            // accG.x is positive when the car is pushed to its left: a left-hand corner
            pass.peak_left_g = pass.peak_left_g.max(mean_lat);
            pass.peak_right_g = pass.peak_right_g.max(-mean_lat);
            if view.wheel_load[0].min(view.wheel_load[1]) > 300.0 {
                pass.max_slip_front = pass.max_slip_front.max((view.wheel_slip[0] + view.wheel_slip[1]) * 0.5);
            }
            if view.wheel_load[2].min(view.wheel_load[3]) > 300.0 {
                pass.max_slip_rear = pass.max_slip_rear.max((view.wheel_slip[2] + view.wheel_slip[3]) * 0.5);
            }
        } else if measuring {
            measuring = false;
            pass.seconds = seconds as f32 - pass.seconds;
        }
        pass.peak_brake_g = pass.peak_brake_g.max(-mean_long);
        pass.max_beta_deg = pass.max_beta_deg.max(beta.abs());
        pass.max_tyres_out = pass.max_tyres_out.max(view.lap.tyres_out);
        if beta.abs() > 25.0 {
            pass.failure = format!("spin at {position:.4} ({beta:.0} deg)");
        } else if view.lap.tyres_out >= 4 {
            pass.failure = format!("off the track at {position:.4}");
        } else if seconds - entered_at > 6.0 && view.speed_kmh < 30.0 {
            pass.failure = format!("stopped at {position:.4}");
        } else if seconds - entered_at > 60.0 {
            pass.failure = format!("never arrived (at {position:.4})");
        }
        if !pass.failure.is_empty() {
            break;
        }
        if crossed_line {
            pass.lap_ms = view.lap.last_ms;
            break;
        }
    }
    if let Some(file) = &mut csv {
        file.flush().map_err(|e| e.to_string())?;
    }
    if !flying {
        pass.failure = "the car never came round to the stretch".to_string();
    } else if pass.failure.is_empty() && !measured {
        pass.failure = "the measured part was never reached".to_string();
    }
    if pass.min_kmh == f32::MAX {
        pass.min_kmh = 0.0;
    }
    Ok(pass)
}

fn print_pass(pass: &Pass) {
    println!(
        "  pace {:4.2}  {:<28} in {:6.1}  min {:6.1}  out {:6.1} km/h  lat g L {:4.2} R {:4.2}  brake g {:4.2}  beta {:4.1} deg  {:5.2} s  slip F {:4.2} R {:4.2}  out {}  cores {:.0}/{:.0}/{:.0}/{:.0} C",
        pass.pace,
        if pass.failure.is_empty() { "ok" } else { &pass.failure },
        pass.entry_kmh,
        pass.min_kmh,
        pass.exit_kmh,
        pass.peak_left_g,
        pass.peak_right_g,
        pass.peak_brake_g,
        pass.max_beta_deg,
        pass.seconds,
        pass.max_slip_front,
        pass.max_slip_rear,
        pass.max_tyres_out,
        pass.core_temp[0],
        pass.core_temp[1],
        pass.core_temp[2],
        pass.core_temp[3],
    );
}

fn main() {
    if let Err(message) = real_main() {
        eprintln!("grip_probe: {message}");
        std::process::exit(1);
    }
}

fn real_main() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let mut car = "ks_ferrari_f2004".to_string();
    let mut track = "spa".to_string();
    // Spa: Blanchimont's exit to the line; the Bus Stop itself is 0.947 to 0.985
    let mut plan = Plan { base_pace: 0.80, zone_pace: 0.80, zone: (0.915, 0.999), steer_rate: 0.0, standing: false, brake_pace: 0.85 };
    let mut measure = (0.947f32, 0.985f32);
    let mut sweep: Option<(f32, f32, f32)> = None;
    let mut variant = Variant::default();
    let mut csv = None;
    let mut jobs = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).saturating_sub(4).max(1);
    let mut verbose = false;
    let mut standing_steps: Option<u64> = None;
    while let Some(arg) = args.next() {
        let mut value = |name: &str| args.next().ok_or(format!("{name} needs a value"));
        let number = |name: &str, text: String| text.parse::<f32>().map_err(|e| format!("{name} {text}: {e}"));
        match arg.as_str() {
            "--car" => car = value("--car")?,
            "--track" => track = value("--track")?,
            "--pace" => plan.zone_pace = number("--pace", value("--pace")?)?,
            "--sweep" => sweep = Some((number("--sweep", value("--sweep")?)?, number("--sweep", value("--sweep")?)?, number("--sweep", value("--sweep")?)?)),
            "--zone" => plan.zone = (number("--zone", value("--zone")?)?, number("--zone", value("--zone")?)?),
            "--measure" => measure = (number("--measure", value("--measure")?)?, number("--measure", value("--measure")?)?),
            "--csv" => csv = Some(PathBuf::from(value("--csv")?)),
            "--jobs" => jobs = number("--jobs", value("--jobs")?)? as usize,
            "--verbose" => verbose = true,
            "--standing" => standing_steps = Some(number("--standing", value("--standing")?)? as u64),
            "--set" => {
                let text = value("--set")?;
                let (key, val) = text.split_once('=').ok_or(format!("--set {text}: key=value expected"))?;
                variant.settings.push((key.to_string(), val.to_string()));
            }
            other => return Err(format!("unknown option {other} (see the head of crates/rustyac-game/examples/grip_probe.rs)")),
        }
    }
    if let Some(steps) = standing_steps {
        // the car at rest at its spawn point after this many steps: what the tyres' pressure
        // does to the ride height
        plan.standing = true;
        let mut setup = SimSetup { car, track, auto_shifter: true, ..SimSetup::default() };
        setup.env.ambient_temperature = 14.0;
        setup.env.road_temperature = 20.0;
        variant.apply_to_setup(&mut setup, &mut plan)?;
        let mut sim = GameSim::new(setup, Box::new(SpawnSequence::new(ProbeDriver::new(plan), false)))?;
        variant.apply_to_car(&mut sim)?;
        for _ in 0..steps {
            sim.step()?;
        }
        println!("{} at rest after {steps} steps:", variant.label());
        if let Some(page) = sim.car.physics_page() {
            for (name, kind, word) in page.named() {
                let wanted = ["wheelLoad", "wheelsPressure", "tyreCoreTemperature", "suspensionTravel", "rideHeight", "cgHeight", "fuel", "airDensity", "camberRAD", "speedKmh"];
                if wanted.iter().any(|w| name.starts_with(w)) {
                    match kind {
                        'i' => println!("  {name} = {}", word as i32),
                        _ => println!("  {name} = {:.5}", f32::from_bits(word)),
                    }
                }
            }
        }
        return Ok(());
    }
    let Some((low, high, step)) = sweep else {
        let pass = run(&RunOptions { car, track, plan, measure, variant: variant.clone(), csv })?;
        println!("{}:", variant.label());
        print_pass(&pass);
        return Ok(());
    };
    if step <= 0.0 || high < low {
        return Err("--sweep <from> <to> <step>".to_string());
    }
    let count = ((high - low) / step).round() as usize + 1;
    let paces: Vec<f32> = (0..count).map(|k| low + k as f32 * step).collect();
    let next = std::sync::atomic::AtomicUsize::new(0);
    let results = std::sync::Mutex::new(Vec::new());
    std::thread::scope(|scope| {
        for _ in 0..jobs.max(1).min(count) {
            scope.spawn(|| loop {
                let k = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                let Some(&pace) = paces.get(k) else { break };
                let mut plan = plan;
                plan.zone_pace = pace;
                let options = RunOptions { car: car.clone(), track: track.clone(), plan, measure, variant: variant.clone(), csv: None };
                let outcome = run(&options);
                results.lock().unwrap_or_else(|e| e.into_inner()).push((k, outcome));
            });
        }
    });
    let mut results = results.into_inner().unwrap_or_else(|e| e.into_inner());
    results.sort_by_key(|(k, _)| *k);
    println!("{}:", variant.label());
    let mut best: Option<Pass> = None;
    let mut failed = false;
    for (_, outcome) in results {
        let pass = outcome?;
        if verbose {
            print_pass(&pass);
        }
        if pass.failure.is_empty() && !failed {
            best = Some(pass);
        } else if !pass.failure.is_empty() {
            if !failed && !verbose {
                print_pass(&pass);
            }
            failed = true;
        }
    }
    match best {
        Some(pass) => {
            print_pass(&pass);
            println!(
                "LIMIT {:<34} pace {:4.2}  min {:6.1} km/h  peak lat g {:4.2}  brake g {:4.2}  time {:5.2} s  fuel {:.1}  air {:.4}",
                variant.label(),
                pass.pace,
                pass.min_kmh,
                pass.peak_left_g.max(pass.peak_right_g),
                pass.peak_brake_g,
                pass.seconds,
                pass.fuel,
                pass.air_density
            );
        }
        None => println!("LIMIT {:<34} none of the paces worked", variant.label()),
    }
    Ok(())
}
