//! Scripted drivers. A script sees a few numbers of the car as they were at the end of the
//! previous step (speed, revs, gear, yaw rate) and hands the pedals, the wheel and the shift
//! paddles to the game through the fake controls provider. Nothing here depends on wall-clock
//! time; the only randomness is a seeded generator.

/// Physics step, s.
pub const DT: f32 = 0.003;
/// Every scenario but `settle` first lets the car sit for this long with no input.
pub const SETTLE_STEPS: usize = 400;

/// What the fake controls device reports for one step.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Controls {
    /// -1 … +1 of full lock; positive turns the car to its right.
    pub steer: f32,
    pub gas: f32,
    pub brake: f32,
    /// 1 = pedal up (clutch closed), 0 = pedal fully pressed (the game's convention).
    pub clutch: f32,
    pub gear_up: bool,
    pub gear_dn: bool,
}

/// What a script may look at: the car as the previous step left it.
#[derive(Clone, Copy, Debug, Default)]
pub struct CarView {
    /// Steps since the scenario started (0 on the first step).
    pub step: usize,
    /// Body speed, m/s.
    pub speed: f32,
    /// The revs the engine would turn at this road speed in the current gear with no wheel
    /// spin (0 in neutral and in the middle of a shift).
    pub road_rpm: f32,
    /// The game's index: 0 reverse, 1 neutral, 2 first gear …
    pub gear: i32,
    /// Yaw rate, rad/s, positive when the car turns right.
    pub yaw_rate: f32,
}

impl CarView {
    pub fn kmh(&self) -> f32 {
        self.speed * 3.6
    }
}

/// The road under a wheel: height of the surface at world (x, z).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Ground {
    Flat,
    /// A raised strip: `height` where `x_min <= x <= x_max` and `z_from <= z <= z_to`.
    Step { x_min: f32, x_max: f32, z_from: f32, z_to: f32, height: f32 },
}

impl Ground {
    /// The form stored in a recording's header.
    pub fn describe(&self) -> String {
        match *self {
            Ground::Flat => "flat".to_string(),
            Ground::Step { x_min, x_max, z_from, z_to, height } => {
                format!("step {x_min:?} {x_max:?} {z_from:?} {z_to:?} {height:?}")
            }
        }
    }

    pub fn parse(text: &str) -> Option<Ground> {
        let mut words = text.split(' ');
        match words.next()? {
            "flat" => Some(Ground::Flat),
            "step" => {
                let mut number = || words.next()?.parse::<f32>().ok();
                Some(Ground::Step {
                    x_min: number()?,
                    x_max: number()?,
                    z_from: number()?,
                    z_to: number()?,
                    height: number()?,
                })
            }
            _ => None,
        }
    }

    pub fn height(&self, x: f32, z: f32) -> f32 {
        match *self {
            Ground::Flat => 0.0,
            Ground::Step { x_min, x_max, z_from, z_to, height } => {
                if x >= x_min && x <= x_max && z >= z_from && z <= z_to {
                    height
                } else {
                    0.0
                }
            }
        }
    }
}

pub struct Scenario {
    pub name: &'static str,
    pub about: &'static str,
    pub steps: usize,
    /// The "automatic clutch" driving aid of the game.
    pub auto_clutch: bool,
    pub ground: Ground,
    /// Also give the road a real collision mesh (one flat quad in ODE's static space), so that
    /// the body's collision boxes can rest on it as they do in the game. Without it only the
    /// tyres know about the road.
    pub floor: bool,
    pub seed: u32,
    kind: Kind,
}

#[derive(Clone, Copy)]
enum Kind {
    Settle,
    Launch,
    Brake,
    SteadyCorner { kmh: f32 },
    Slalom,
    LiftOff,
    Kerb,
    Random,
}

const fn seconds(s: f32) -> usize {
    (s / DT) as usize
}

pub fn all() -> Vec<Scenario> {
    let flat = |name, about, secs: f32, auto_clutch, kind| Scenario {
        name,
        about,
        steps: seconds(secs) + 1,
        auto_clutch,
        ground: Ground::Flat,
        floor: false,
        seed: 1,
        kind,
    };
    vec![
        flat("settle", "dropped on the plane, no input, 2 s", 2.0, true, Kind::Settle),
        Scenario {
            floor: true,
            ..flat(
                "settle_floor",
                "as settle, with a collision mesh under the car so that its floor can touch the road",
                2.0,
                true,
                Kind::Settle,
            )
        },
        flat(
            "launch_autoclutch_off",
            "standing start by hand clutch, flat out from 1st to 6th",
            14.0,
            false,
            Kind::Launch,
        ),
        flat(
            "launch_autoclutch_on",
            "standing start with the automatic clutch aid, flat out from 1st to 6th",
            14.0,
            true,
            Kind::Launch,
        ),
        flat("brake", "flat out to 250 km/h, then full brakes to a stop (the car has no ABS)", 16.0, true, Kind::Brake),
        flat("steady_corner_60", "100 m radius circle at 60 km/h", 20.0, true, Kind::SteadyCorner { kmh: 60.0 }),
        flat("steady_corner_120", "100 m radius circle at 120 km/h", 20.0, true, Kind::SteadyCorner { kmh: 120.0 }),
        flat("steady_corner_180", "100 m radius circle at 180 km/h", 20.0, true, Kind::SteadyCorner { kmh: 180.0 }),
        flat("slalom", "sine steering at 0.5 Hz, 100 km/h", 16.0, true, Kind::Slalom),
        flat(
            "liftoff_oversteer",
            "100 m radius at 180 km/h, then the throttle is dropped with the wheel held",
            16.0,
            true,
            Kind::LiftOff,
        ),
        Scenario {
            name: "kerb",
            about: "straight at 100 km/h over a 2 cm raised strip under the left wheels",
            steps: seconds(12.0) + 1,
            auto_clutch: true,
            // the car starts at the origin facing +z; its left side is +x
            ground: Ground::Step { x_min: 0.0, x_max: 5.0, z_from: 150.0, z_to: 170.0, height: 0.02 },
            floor: false,
            seed: 1,
            kind: Kind::Kerb,
        },
        Scenario {
            name: "random",
            about: "smoothly varying random wheel and pedals for 60 s",
            steps: seconds(60.0) + 1,
            auto_clutch: true,
            ground: Ground::Flat,
            floor: false,
            seed: 20040314,
            kind: Kind::Random,
        },
    ]
}

/// The limiter is at 19,000 rpm; shift a little before it.
const SHIFT_UP_RPM: f32 = 18_000.0;
const SHIFT_DN_RPM: f32 = 9_000.0;
const FIRST: i32 = 2;

/// State a script keeps between steps.
pub struct Driver {
    kind: Kind,
    auto_clutch: bool,
    /// Steps left of the current paddle press (positive) or of the pause after it (negative).
    paddle: i32,
    paddle_up: bool,
    /// The standing start is over (after that, neutral only means "in the middle of a shift").
    under_way: bool,
    speed_integral: f32,
    steer: f32,
    steer_integral: f32,
    phase_start: Option<usize>,
    hold_steer: Option<f32>,
    rng: u32,
    noise: [f32; 6],
    target: [f32; 3],
}

impl Scenario {
    pub fn driver(&self) -> Driver {
        Driver {
            kind: self.kind,
            auto_clutch: self.auto_clutch,
            paddle: 0,
            paddle_up: false,
            under_way: false,
            speed_integral: 0.0,
            steer: 0.0,
            steer_integral: 0.0,
            phase_start: None,
            hold_steer: None,
            rng: self.seed,
            noise: [0.0; 6],
            target: [0.0; 3],
        }
    }
}

impl Driver {
    /// xorshift32, uniform in [0, 1)
    fn random(&mut self) -> f32 {
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.rng = x;
        (x >> 8) as f32 / 16_777_216.0
    }

    /// Presses a paddle for 10 steps, then keeps both released for 60 (longer than a shift).
    fn paddles(&mut self, want_up: bool, want_dn: bool, c: &mut Controls) {
        if self.paddle > 0 {
            self.paddle -= 1;
            c.gear_up = self.paddle_up;
            c.gear_dn = !self.paddle_up;
            if self.paddle == 0 {
                self.paddle = -60;
            }
        } else if self.paddle < 0 {
            self.paddle += 1;
        } else if want_up || want_dn {
            self.paddle = 10;
            self.paddle_up = want_up;
        }
    }

    /// Changes gear by the road speed (so that wheel spin does not trigger a shift): up when the
    /// engine would be near its limiter (not beyond `top`), down when it would be far below it.
    fn shift_by_rpm(&mut self, car: &CarView, top: i32, c: &mut Controls) {
        let up = car.gear >= FIRST && car.gear < top && car.road_rpm > SHIFT_UP_RPM;
        let dn = car.gear > FIRST && car.road_rpm > 0.0 && car.road_rpm < SHIFT_DN_RPM;
        self.paddles(up, dn, c);
    }

    /// Gets the car rolling: neutral -> first, revs, clutch. Returns true once it is under way.
    fn pull_away(&mut self, car: &CarView, t: f32, c: &mut Controls) -> bool {
        if self.under_way {
            c.gas = 1.0;
            return true;
        }
        if car.gear < FIRST {
            // in neutral: clutch pressed, ask for first
            c.clutch = 0.0;
            self.paddles(true, false, c);
            return false;
        }
        let since = *self.phase_start.get_or_insert(car.step);
        let dt = (car.step - since) as f32 * DT;
        if self.auto_clutch {
            // the aid works the clutch itself
            c.clutch = 1.0;
            c.gas = 1.0;
            self.under_way = dt > 0.5;
            return self.under_way;
        }
        // by hand: hold the revs for 0.4 s, then let the clutch up over 0.6 s
        let _ = t;
        if dt < 0.4 {
            c.clutch = 0.0;
            c.gas = 0.5;
            false
        } else if dt < 1.0 {
            c.clutch = (dt - 0.4) / 0.6;
            c.gas = 1.0;
            false
        } else {
            c.clutch = 1.0;
            c.gas = 1.0;
            self.under_way = true;
            true
        }
    }

    /// Throttle (and a little brake) to hold `target` m/s.
    fn hold_speed(&mut self, car: &CarView, target: f32, c: &mut Controls) {
        let error = target - car.speed;
        self.speed_integral = (self.speed_integral + error * DT * 0.3).clamp(-0.2, 0.8);
        let demand = error * 0.5 + self.speed_integral;
        c.gas = demand.clamp(0.0, 1.0);
        c.brake = (-demand * 0.5).clamp(0.0, 0.5);
    }

    /// The controls for this step.
    pub fn controls(&mut self, car: &CarView) -> Controls {
        let mut c = Controls { clutch: 1.0, ..Controls::default() };
        if matches!(self.kind, Kind::Settle) || car.step < SETTLE_STEPS {
            return c;
        }
        let t = (car.step - SETTLE_STEPS) as f32 * DT;
        match self.kind {
            Kind::Settle => {}
            Kind::Launch => {
                if self.pull_away(car, t, &mut c) {
                    self.shift_by_rpm(car, FIRST + 5, &mut c);
                }
            }
            Kind::Brake => {
                if self.hold_steer.is_none() {
                    // phase 1: flat out
                    if self.pull_away(car, t, &mut c) {
                        self.shift_by_rpm(car, FIRST + 6, &mut c);
                    }
                    if car.kmh() >= 250.0 {
                        self.hold_steer = Some(0.0);
                    }
                } else {
                    // phase 2: full brakes, down through the gears
                    c.gas = 0.0;
                    c.brake = 1.0;
                    let dn = car.gear > FIRST && car.road_rpm > 0.0 && car.road_rpm < 12_000.0;
                    self.paddles(false, dn, &mut c);
                }
            }
            Kind::SteadyCorner { kmh } => self.corner(car, t, kmh / 3.6, None, &mut c),
            Kind::LiftOff => self.corner(car, t, 180.0 / 3.6, Some(11.0), &mut c),
            Kind::Slalom => {
                if self.pull_away(car, t, &mut c) {
                    self.shift_by_rpm(car, FIRST + 6, &mut c);
                    self.hold_speed(car, 100.0 / 3.6, &mut c);
                }
                if t > 6.0 {
                    let phase = (t - 6.0) * 0.5 * std::f32::consts::TAU;
                    c.steer = 0.12 * phase.sin();
                }
            }
            Kind::Kerb => {
                if self.pull_away(car, t, &mut c) {
                    self.shift_by_rpm(car, FIRST + 6, &mut c);
                    self.hold_speed(car, 100.0 / 3.6, &mut c);
                }
            }
            Kind::Random => {
                if self.pull_away(car, t, &mut c) {
                    self.shift_by_rpm(car, FIRST + 6, &mut c);
                    // a new target every 0.6 s, followed through two low-pass filters
                    if (car.step - SETTLE_STEPS) % 200 == 0 {
                        let r = [self.random(), self.random(), self.random(), self.random()];
                        self.target[0] = (r[0] * 2.0 - 1.0) * 0.25; // steer
                        // mostly throttle (so that it gets through the gears), sometimes brakes
                        if r[3] < 0.15 {
                            self.target[1] = 0.0;
                            self.target[2] = r[2] * 0.8;
                        } else {
                            self.target[1] = 0.25 + 0.75 * r[1];
                            self.target[2] = 0.0;
                        }
                    }
                    for i in 0..3 {
                        self.noise[i] += (self.target[i] - self.noise[i]) * 0.02;
                        self.noise[i + 3] += (self.noise[i] - self.noise[i + 3]) * 0.02;
                    }
                    // less wheel at speed, so the car spends its time driving rather than spinning
                    c.steer = self.noise[3] / (1.0 + car.speed * 0.03);
                    c.gas = self.noise[4].clamp(0.0, 1.0);
                    c.brake = self.noise[5].clamp(0.0, 1.0);
                    if car.speed < 8.0 {
                        // do not let it stop: brakes off, a little throttle
                        c.brake = 0.0;
                        c.gas = c.gas.max(0.4);
                    }
                }
            }
        }
        c
    }

    /// Straight up to `speed`, then a right-hand circle of 100 m radius; with `lift_at` the
    /// throttle is shut at that time and the wheel stays where it was.
    fn corner(&mut self, car: &CarView, t: f32, speed: f32, lift_at: Option<f32>, c: &mut Controls) {
        const RADIUS: f32 = 100.0;
        const TURN_IN: f32 = 7.0;
        if !self.pull_away(car, t, c) {
            return;
        }
        self.shift_by_rpm(car, FIRST + 6, c);
        if let (Some(at), Some(held)) = (lift_at, self.hold_steer) {
            if t >= at {
                c.gas = 0.0;
                c.brake = 0.0;
                c.steer = held;
                return;
            }
        }
        self.hold_speed(car, speed, c);
        if t >= TURN_IN {
            // wheel angle from the yaw rate the circle needs, eased in over a second
            let ease = ((t - TURN_IN) / 1.0).min(1.0);
            let want = car.speed.max(5.0) / RADIUS * ease;
            let error = want - car.yaw_rate;
            self.steer_integral = (self.steer_integral + error * DT * 2.0).clamp(-0.5, 0.5);
            self.steer = (error * 0.15 + self.steer_integral).clamp(-0.6, 0.6);
            c.steer = self.steer;
            if let Some(at) = lift_at {
                if t >= at - DT {
                    self.hold_steer = Some(self.steer);
                }
            }
        }
    }
}
