// SPDX-License-Identifier: GPL-3.0-or-later

//! Scripted drivers. A script sees a few numbers of the car as they were at the end of the
//! previous step (speed, revs, gear, yaw rate) and hands the pedals, the wheel and the shift
//! paddles to the game through the fake controls provider. Nothing here depends on wall-clock
//! time; the only randomness is a seeded generator.

pub use crate::track_driver::{Follower, TrackKind};

/// Physics step, s.
pub const DT: f32 = 0.003;
/// Every scenario but `settle` first lets the car sit for this long with no input.
pub const SETTLE_STEPS: usize = 400;

/// What the fake controls device reports for one step.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Controls {
    /// -1 … +1 of full lock; positive turns the car to its right.
    pub steer: f32,
    pub gas: f32,
    pub brake: f32,
    /// 1 = pedal up (clutch closed), 0 = pedal fully pressed (the game's convention).
    pub clutch: f32,
    pub gear_up: bool,
    pub gear_dn: bool,
    /// 0 … 1
    pub hand_brake: f32,
    /// The H-shifter's gear in the game's index (0 reverse, 1 neutral, 2 first …); -1 = the
    /// car is driven with the paddles (what the game's own devices report without a shifter).
    pub requested_gear: i32,
    /// Clicks of the cockpit brake-bias control asked for before this step (+1 / -1; 0 none).
    /// Not a member of the game's controls: the harness calls
    /// `BrakeSystem::setManualFrontBias` before the step, as the game's command queue does.
    pub bias_clicks: i32,
    /// The DRS button (`CarControls::drs`).
    pub drs: bool,
    /// The headlight switch (the device's `getAction(4)`).
    pub headlights: bool,
    /// Jobs the game's main thread would queue for the physics thread; the harness calls the
    /// game's own functions before the step. `lock_ms` != 0: `Car::lockControlsUntil(lock_ms,
    /// physicsTime)`; `set_locked` 1 / -1: `Car::lockControls(true / false)`; `gentle_stop`
    /// 1 / -1: `Car::isGentleStopping` on / off; `add_penalty` != 0: `Car::addPenalty(seconds)`.
    pub lock_ms: f32,
    pub set_locked: i32,
    pub gentle_stop: i32,
    pub add_penalty: f32,
    /// A teleport before this step, through the game's own `Car::forceRotation` and
    /// `Car::forcePosition`: 1 = without spoiling the lap, 2 = with (0 = none), to this point
    /// on the road and with the tail in this direction. Track scenarios only.
    ///
    /// 3 = put down in any attitude (Task 13, a rolled-over car): the rows of `teleport_rows`
    /// (the body's x, y and z axes in the world) are set on the car body and the fuel tank
    /// with the game's `RigidBodyODE::setRotation`, then `Car::forcePosition` without
    /// spoiling the lap.
    pub teleport: i32,
    pub teleport_position: [f32; 3],
    pub teleport_tail: [f32; 3],
    pub teleport_rows: [[f32; 3]; 3],
    /// Task 16. The KERS / ERS button (`CarControls::kers`).
    pub kers: bool,
    /// The cockpit settings of a hybrid car. In the game they are jobs the main thread queues
    /// for the physics thread (`CarAvatar::cycleERSPower`, `cycleERSRecovery`,
    /// `cycleERSHeatCharging`, `cycleEngineBrake`); the harness does them before the step.
    /// `ers_power` >= 0: `ERS::setPowerController` with this index (taken modulo the number
    /// the car has). `ers_recovery` 0..10: `kineticRecovery = level * 0.1f`. `ers_heat` 1 /
    /// -1: the MGU-H charges the battery / helps the engine. `engine_brake` >= 0:
    /// `Engine::setCoastSettings` (limited to the settings the car has).
    pub ers_power: i32,
    pub ers_recovery: i32,
    pub ers_heat: i32,
    pub engine_brake: i32,
}

impl Default for Controls {
    fn default() -> Controls {
        Controls {
            steer: 0.0,
            gas: 0.0,
            brake: 0.0,
            clutch: 0.0,
            gear_up: false,
            gear_dn: false,
            hand_brake: 0.0,
            requested_gear: -1,
            bias_clicks: 0,
            drs: false,
            headlights: false,
            lock_ms: 0.0,
            set_locked: 0,
            gentle_stop: 0,
            add_penalty: 0.0,
            teleport: 0,
            teleport_position: [0.0; 3],
            teleport_tail: [0.0; 3],
            teleport_rows: [[0.0; 3]; 3],
            kers: false,
            ers_power: -1,
            ers_recovery: -1,
            ers_heat: 0,
            engine_brake: -1,
        }
    }
}

/// What a whole-car scenario (Task 10: aero, aids, the `Car::step` shell) sets up besides the
/// driver's controls. All of it is session or track environment, stored in the recording's
/// header.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Whole {
    /// The scenario is one of [`whole`]: its recording also holds the DRS button and more
    /// internal values of the wings and the aids.
    pub on: bool,
    /// The road is marked as pit lane (`SurfaceDef::isPitlane`): the pit limiter works.
    pub pitlane: bool,
    /// `StabilityControl::gain`: the game's stability aid, 0 (off) to 1.
    pub stability_gain: f32,
    /// Wind speed, m/s (0: none), and the direction it is handed to the game with, degrees.
    pub wind_speed: f32,
    pub wind_direction_deg: f32,
    /// `Car::damageZoneLevel` (front, rear, left, right, centre) written after the spawn: the
    /// wings of a dented car.
    pub damage: [f32; 5],
    /// The session's penalty rule is "cut gas" (`PhysicsEngine::penaltyMode` 0) instead of the
    /// engine's default "nothing" (3).
    pub penalty_cut_gas: bool,
    /// Task 15: the session's conditions as `cfg/race.ini` has them.
    pub conditions: Conditions,
    /// Task 16: the KERS / ERS button is held whenever the throttle is above a half.
    pub kers_held: bool,
    /// Task 16: the DRS button is pressed for 30 ms every 1.2 s (the wing opens where the
    /// track's zones allow it).
    pub drs_pulsed: bool,
}

/// The session of a Task 15 scenario: what the game reads from `cfg/race.ini` (and a saved
/// setup). Everything `None` / 0 is the oracle's usual session: 26 deg C air, 30 deg C road,
/// no `[DYNAMIC_TRACK]` section (full grip), no wind, the default setup.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Conditions {
    /// `[TEMPERATURE] AMBIENT`, `ROAD`
    pub temperature: Option<(f32, f32)>,
    /// `[DYNAMIC_TRACK] SESSION_START`, `RANDOMNESS`, `LAP_GAIN`, `SESSION_TRANSFER`
    pub dynamic_track: Option<[f32; 4]>,
    /// `[WIND] SPEED_KMH_MIN`, `SPEED_KMH_MAX`, `DIRECTION_DEG`
    pub wind: Option<[f32; 3]>,
    /// `[CAR_0] BALLAST`, `RESTRICTOR`
    pub ballast_kg: f32,
    pub restrictor: f32,
    /// The scenario loads the saved setup given with `--setup <file>`.
    pub setup: bool,
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
    /// The body's position, and its forward and left axes in the world (track scenarios).
    pub position: [f32; 3],
    pub forward: [f32; 3],
    pub left: [f32; 3],
    /// The game's own place along the AI line after the last step (`splineLocatorData.npos`;
    /// -1 before the first) and the car's offset beside it.
    pub npos: f32,
    pub offset: f32,
    /// The front wheels' angle at full lock, rad.
    pub max_wheel_angle: f32,
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
    /// The "automatic gearbox" driving aid of the game (`AutoShifter::isActive`).
    pub auto_shifter: bool,
    /// A scenario written for the brakes / engine / drivetrain port (Task 09): its recording
    /// also holds the handbrake, H-shifter and brake-bias inputs and a few more internal
    /// values of those systems. The twelve scenarios of [`all`] are not; their recordings keep
    /// the layout (and hashes) of Task 06.
    pub powertrain: bool,
    /// Task 10's additions (all off for the older scenarios).
    pub whole: Whole,
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
    Handbrake,
    Shifter,
    AutoShift,
    Protect,
    PtRandom,
    WcDrs,
    WcStops,
    WcPit,
    WcSpirited,
    WcShell,
    /// Task 16, a hybrid car: the button held until the battery is empty, a stop that fills
    /// it again, the button until the lap's allowance is used.
    HyDeploy,
    /// Task 16, an ERS car: every cockpit setting once.
    HyModes,
    /// On a real track (`--track`), following its AI line.
    Track(TrackKind),
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
        auto_shifter: false,
        powertrain: false,
        whole: Whole::default(),
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
            auto_shifter: false,
            powertrain: false,
            whole: Whole::default(),
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
            auto_shifter: false,
            powertrain: false,
            whole: Whole::default(),
            kind: Kind::Random,
        },
    ]
}

/// The scenarios of the brakes / engine / drivetrain port. They are not part of
/// `car_oracle all` unless named with `--only`, and are meant for the altered test cars as
/// much as for the real one.
pub fn powertrain() -> Vec<Scenario> {
    let pt = |name, about, secs: f32, auto_clutch, auto_shifter, seed, kind| Scenario {
        name,
        about,
        steps: seconds(secs) + 1,
        auto_clutch,
        ground: Ground::Flat,
        floor: false,
        seed,
        auto_shifter,
        powertrain: true,
        whole: Whole::default(),
        kind,
    };
    vec![
        pt(
            "pt_handbrake",
            "to 90 km/h, clutch in and handbrake on to a stop (with cockpit brake-bias clicks), down to reverse, backwards, brake",
            11.0,
            false,
            false,
            1,
            Kind::Handbrake,
        ),
        pt(
            "pt_shifter",
            "H-shifter: revs to the limiter in neutral, start by hand, clutch-dip up-shifts, a shift without clutch (grinding), neutral coasting, re-engage, stop",
            12.0,
            false,
            false,
            1,
            Kind::Shifter,
        ),
        pt(
            "pt_autoshift",
            "automatic gearbox and clutch aids: flat out from neutral, brake to a stop, flat out again",
            12.0,
            true,
            true,
            1,
            Kind::AutoShift,
        ),
        pt(
            "pt_protect",
            "paddles: down-shift requests the gearbox must refuse (neutral at speed, over-rev), up-shift in top gear, then down through the box under braking",
            10.0,
            true,
            false,
            1,
            Kind::Protect,
        ),
        pt(
            "pt_random",
            "random wheel, pedals, clutch dips, handbrake pulls, paddle presses at any revs and brake-bias clicks",
            14.0,
            true,
            false,
            20040921,
            Kind::PtRandom,
        ),
    ]
}

/// The scenarios of the whole-car port (aero, aids, the `Car::step` shell). Like the powertrain
/// ones they only run when named, record the powertrain extras too, and are driven with the
/// game's automatic clutch and gearbox so that they work on any car.
pub fn whole() -> Vec<Scenario> {
    let wc = |name, about, secs: f32, whole: Whole, kind| Scenario {
        name,
        about,
        steps: seconds(secs) + 1,
        auto_clutch: true,
        ground: Ground::Flat,
        floor: false,
        seed: 1,
        auto_shifter: true,
        powertrain: true,
        whole: Whole { on: true, ..whole },
        kind,
    };
    let none = Whole::default();
    vec![
        wc(
            "wc_drs",
            "flat out; the DRS button opens the wing, a second press closes it, braking closes it, a press while braking does nothing, open again in a fast bend",
            12.0,
            none,
            Kind::WcDrs,
        ),
        wc(
            "wc_stops",
            "two full-throttle runs each ended by a full-pedal stop, the second while steering: traction control and ABS at work",
            13.0,
            none,
            Kind::WcStops,
        ),
        wc(
            "wc_pit",
            "the road is a pit lane: flat out into the pit limiter (80 km/h), lift, flat out again, brake to a stop",
            10.0,
            Whole { pitlane: true, ..none },
            Kind::WcPit,
        ),
        wc(
            "wc_spirited",
            "throttle on and off through a growing slalom (one wheel of the driven pair spins: differential lock on cars that have one)",
            12.0,
            none,
            Kind::WcSpirited,
        ),
        wc(
            "wc_stability",
            "as wc_spirited with the stability aid at 100 %",
            12.0,
            Whole { stability_gain: 1.0, ..none },
            Kind::WcSpirited,
        ),
        wc(
            "wc_shell",
            "the car-level rules: a penalty forgiven at low speed, controls locked for 0.6 s, headlight switch, a gentle stop, a penalty that waits on the throttle and runs down off it, a second penalty on top, controls locked outright",
            11.0,
            Whole { penalty_cut_gas: true, ..none },
            Kind::WcShell,
        ),
        wc(
            "wc_damage",
            "as wc_spirited with dented bodywork (damage levels 45 front, 20 rear, 70 left, 10 right)",
            12.0,
            Whole { damage: [45.0, 20.0, 70.0, 10.0, 70.0], ..none },
            Kind::WcSpirited,
        ),
        wc(
            "wc_wind",
            "as wc_spirited in a 12 m/s wind from the front left",
            12.0,
            Whole { wind_speed: 12.0, wind_direction_deg: 60.0, ..none },
            Kind::WcSpirited,
        ),
    ]
}

/// The scenarios on a real track (Task 12): the game's own track and collision meshes with the
/// game's car on them, its body a ghost (only the tyres' rays meet the track). They need
/// `--track <folder>` and are written for Spa; each ends early if the car leaves the track
/// for good (where its body would be in a wall).
pub fn track() -> Vec<Scenario> {
    let on_track = |name, about, secs: f32, kind| Scenario {
        name,
        about,
        steps: seconds(secs) + 1,
        auto_clutch: true,
        ground: Ground::Flat,
        floor: false,
        seed: 1,
        auto_shifter: true,
        powertrain: true,
        whole: Whole { on: true, ..Whole::default() },
        kind: Kind::Track(kind),
    };
    // Task 15: the launch again, in other weather, on another track state, with another setup
    let conditions_on = |name, about, secs: f32, kind, conditions: Conditions| Scenario {
        whole: Whole { on: true, conditions, ..Whole::default() },
        ..on_track(name, about, secs, kind)
    };
    let in_conditions = |name, about, conditions: Conditions| conditions_on(name, about, 24.0, TrackKind::Launch, conditions);
    let mut scenarios = vec![
        in_conditions(
            "spa_cold_green_wind",
            "spa_launch on a cold day (8 deg C air, 11 deg C road), a green track (88 % grip, +-3 % drawn, half a percent more per lap) and a wind drawn from 12 to 26 km/h around 130 degrees",
            Conditions { temperature: Some((8.0, 11.0)), dynamic_track: Some([88.0, 3.0, 2.0, 50.0]), wind: Some([12.0, 26.0, 130.0]), ..Conditions::default() },
        ),
        in_conditions(
            "spa_hot_optimum",
            "spa_launch on a hot day (36 deg C air, 48 deg C road), the track at its optimum (100 %), no wind; 25 kg of ballast and a restrictor of 60",
            Conditions { temperature: Some((36.0, 48.0)), dynamic_track: Some([100.0, 0.0, 1.0, 100.0]), ballast_kg: 25.0, restrictor: 60.0, ..Conditions::default() },
        ),
        in_conditions(
            "spa_user",
            "spa_launch in the conditions of the user's own race.ini: 14 deg C air, 20 deg C road, grip 100 %, wind 10 km/h from 0 degrees (drawn: 8 to 12 km/h, within 20 degrees)",
            Conditions { temperature: Some((14.0, 20.0)), dynamic_track: Some([100.0, 0.0, 1.0, 100.0]), wind: Some([10.0, 10.0, 0.0]), ..Conditions::default() },
        ),
        conditions_on(
            "spa_setup",
            "spa_eau_rouge with a saved setup (--setup <file>) loaded before the first step",
            16.0,
            TrackKind::EauRouge,
            Conditions { setup: true, ..Conditions::default() },
        ),
        in_conditions(
            "spa_setup_launch",
            "spa_launch with a saved setup (--setup <file>) loaded before the first step",
            Conditions { setup: true, ..Conditions::default() },
        ),
        in_conditions(
            "spa_wind_any",
            "spa_launch with a wind from any direction (DIRECTION_DEG=-1: the direction is drawn too), 5 to 35 km/h",
            Conditions { wind: Some([5.0, 35.0, -1.0]), ..Conditions::default() },
        ),
        conditions_on(
            "spa_green_laps",
            "spa_timing on a green track that gains grip with every lap counted: 86 % at the start, +-2 % drawn, 1.25 % more per lap",
            60.0,
            TrackKind::Timing,
            Conditions { dynamic_track: Some([86.0, 2.0, 0.8, 0.0]), ..Conditions::default() },
        ),
    ];
    let with_button = |name, about, secs: f32, kind| Scenario {
        whole: Whole { on: true, kers_held: true, ..Whole::default() },
        ..on_track(name, about, secs, kind)
    };
    scenarios.extend(vec![
        with_button(
            "spa_hybrid_timing",
            "spa_timing with the KERS / ERS button held on the throttle: the battery and the lap's energy count over line crossings (the count starts again with every lap) and over teleports (both are reset)",
            60.0,
            TrackKind::Timing,
        ),
        with_button("spa_hybrid_lap", "spa_lap with the KERS / ERS button held on the throttle", 60.0, TrackKind::Lap),
        Scenario {
            whole: Whole { on: true, kers_held: true, drs_pulsed: true, ..Whole::default() },
            ..on_track(
                "spa_drs_timing",
                "spa_hybrid_timing with the DRS button pressed every 1.2 s: the wing opens only in the track's DRS zones (data/drs_zones.ini; the first one runs over the start line) and shuts on leaving one",
                60.0,
                TrackKind::Timing,
            )
        },
    ]);
    scenarios.extend(vec![
        on_track("spa_launch", "from the hot-lap start flat out down the pit straight, then on the brakes for La Source and round it", 24.0, TrackKind::Launch),
        on_track("spa_eau_rouge", "from 300 m before the bottom of Eau Rouge flat out through it and up Raidillon: compression, then the crest", 16.0, TrackKind::EauRouge),
        on_track("spa_kerbs", "through the Bus Stop chicane with the inner wheels over its kerbs", 20.0, TrackKind::Kerbs),
        on_track("spa_lap", "a lap along the AI line from the hot-lap start, as far as a minute goes", 60.0, TrackKind::Lap),
        on_track("spa_grass", "up the Kemmel straight, the two right wheels on the grass for four seconds, then back", 16.0, TrackKind::Grass),
        on_track(
            "spa_timing",
            "lap timing without driving whole laps: over the start line, then teleported (the game's own Car::forcePosition) from 60 m before one timing line to 60 m before the next, twice round all three, and a last crossing of the start line after a teleport that spoils the lap",
            60.0,
            TrackKind::Timing,
        ),
        // Task 13: the body touches things (recorded with --collide)
        on_track("spa_wall_low", "towards La Source at 60 km/h and straight on where the road turns: into the barrier", 14.0, TrackKind::WallLow),
        on_track("spa_wall_high", "flat out towards Blanchimont, then a little wheel to the outside: off the road and into the barrier at more than 280 km/h", 20.0, TrackKind::WallHigh),
        on_track("spa_wall_gravel", "flat out towards Blanchimont and straight on: over the run-off and into the barrier at about 195 km/h, the right front corner first (suspension damage)", 20.0, TrackKind::WallGravel),
        on_track("spa_wall_slide", "from the hot-lap start at 100 km/h, steered further and further to the left: off the road and along the wall", 18.0, TrackKind::WallSlide),
        on_track("spa_bottoming", "from 300 m before the bottom of Eau Rouge flat out through the compression: the floor on the road", 16.0, TrackKind::Bottoming),
        on_track("spa_kerb_strike", "at the Bus Stop chicane much too fast and deep over its inner kerbs", 16.0, TrackKind::KerbStrike),
        on_track("spa_rollover", "put down on its roof 0.8 m above the road and left to settle, five seconds later put down on its side (a second of throttle each time: a sleeping car would hang in the air)", 10.0, TrackKind::Rollover),
    ]);
    scenarios
}

/// The scenarios of the hybrid systems (Task 16: KERS, ERS). Like the whole-car ones: only
/// when named, driven with the automatic clutch and gearbox.
pub fn hybrid() -> Vec<Scenario> {
    let hy = |name, about, secs: f32, kind| Scenario {
        name,
        about,
        steps: seconds(secs) + 1,
        auto_clutch: true,
        ground: Ground::Flat,
        floor: false,
        seed: 1,
        auto_shifter: true,
        powertrain: true,
        whole: Whole { on: true, ..Whole::default() },
        kind,
    };
    vec![
        hy(
            "hy_deploy",
            "flat out with the KERS / ERS button held until the battery is empty and a second more, a full stop (recovery), the button again until the lap's allowance is used, lift, the button on the brakes, half throttle with short presses",
            30.0,
            Kind::HyDeploy,
        ),
        hy(
            "hy_modes",
            "an ERS car's cockpit settings one after the other while it drives and brakes: recovery 100 %, 0 %, 70 %, every delivery profile, MGU-H to the motor and back to the battery, three engine-brake settings, the button now and then",
            30.0,
            Kind::HyModes,
        ),
    ]
}

/// Every scenario by name.
pub fn find(name: &str) -> Option<Scenario> {
    all().into_iter().chain(powertrain()).chain(whole()).chain(hybrid()).chain(track()).find(|s| s.name == name)
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
    /// Where a staged script is, and the step its current stage began at.
    stage: u32,
    mark: usize,
    /// The H-shifter's lever (game index) and where a shift in progress is going.
    lever: i32,
    lever_target: i32,
    extra: [f32; 3],
    /// `Whole::kers_held` and `Whole::drs_pulsed` of the scenario.
    kers_held: bool,
    drs_pulsed: bool,
    /// The track a track scenario drives on (the Rust port of it: only its AI line is read).
    pub track: Option<std::sync::Arc<rustyac_physics::track::Track>>,
    pub follower: Follower,
}

impl Scenario {
    /// Which track scenario this is, if it is one.
    pub fn track_kind(&self) -> Option<TrackKind> {
        match self.kind {
            Kind::Track(kind) => Some(kind),
            _ => None,
        }
    }

    pub fn driver(&self) -> Driver {
        Driver {
            track: None,
            follower: Follower::default(),
            kind: self.kind,
            kers_held: self.whole.kers_held,
            drs_pulsed: self.whole.drs_pulsed,
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
            stage: 0,
            mark: 0,
            lever: 1,
            lever_target: 1,
            extra: [0.0; 3],
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
        if self.phase_start.is_none() {
            // first gear is in: the press that asked for it is over, whatever was left of it
            self.paddle = 0;
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
        if matches!(self.kind, Kind::Shifter) {
            // the lever is in neutral while the car settles
            c.requested_gear = self.lever;
        }
        if matches!(self.kind, Kind::Settle) || car.step < SETTLE_STEPS {
            return c;
        }
        let t = (car.step - SETTLE_STEPS) as f32 * DT;
        let mut c = self.controls_of(car, t, c);
        if self.kers_held && c.gas > 0.5 {
            c.kers = true;
        }
        if self.drs_pulsed {
            c.drs = t % 1.2 < 0.03;
        }
        c
    }

    fn controls_of(&mut self, car: &CarView, t: f32, mut c: Controls) -> Controls {
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
                    // the game's own C runtime sine, so the script is the same on every PC
                    c.steer = 0.12 * rustyac_physics::math::sinf(phase);
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
            Kind::Handbrake => self.handbrake(car, t, &mut c),
            Kind::Shifter => self.shifter(car, t, &mut c),
            Kind::AutoShift => {
                // the aids do the clutch and the gears; the car starts in neutral
                if t < 5.5 {
                    c.gas = 1.0;
                } else if t < 9.0 {
                    c.brake = 0.7;
                } else {
                    c.gas = 0.6;
                }
            }
            Kind::Protect => self.protect(car, t, &mut c),
            Kind::WcDrs => {
                // a press lasts 30 ms
                let pressed = |at: f32| t >= at && t < at + 0.03;
                c.gas = 1.0;
                c.drs = pressed(3.0) || pressed(4.5) || pressed(5.5) || pressed(7.2) || pressed(8.6);
                if (6.8..7.6).contains(&t) {
                    // braking closes the wing; the press at 7.2 s must not open it
                    c.gas = 0.0;
                    c.brake = 0.6;
                }
                if t > 9.2 {
                    // a fast bend: lateral load (some cars close the wing above a limit)
                    c.steer = (0.04 * (t - 9.2)).min(0.08);
                }
            }
            Kind::WcStops => {
                if t < 4.5 {
                    c.gas = 1.0;
                } else if t < 7.5 {
                    c.brake = 1.0;
                } else if t < 10.0 {
                    c.gas = 1.0;
                    c.steer = 0.03;
                } else {
                    c.brake = 1.0;
                    c.steer = 0.06;
                }
            }
            Kind::WcPit => {
                if t < 4.0 {
                    c.gas = 1.0;
                } else if t < 5.0 {
                    // lifted
                } else if t < 7.5 {
                    c.gas = 1.0;
                } else {
                    c.brake = 0.5;
                }
            }
            Kind::WcShell => {
                // one job per moment: exactly one step
                let n = car.step - SETTLE_STEPS;
                let at = |mark: f32| n == (mark / DT) as usize;
                c.gas = 1.0;
                // still slow: a penalty given here is forgiven at once (below 35 km/h)
                if at(0.3) {
                    c.add_penalty = 4.0;
                }
                if at(1.5) {
                    c.lock_ms = 600.0;
                }
                c.headlights = (3.0..3.02).contains(&t) || (3.5..3.52).contains(&t);
                if at(4.0) {
                    c.gentle_stop = 1;
                }
                if at(4.5) {
                    c.gentle_stop = -1;
                }
                // a penalty at speed: it waits while the throttle is down ...
                if at(5.5) {
                    c.add_penalty = 3.0;
                }
                // ... and runs down while the driver lifts; a second one on top of it
                if (6.5..8.0).contains(&t) {
                    c.gas = 0.0;
                }
                if at(7.2) {
                    c.add_penalty = 2.0;
                }
                // controls locked outright (the device is not asked), then free again
                if at(8.6) {
                    c.set_locked = 1;
                }
                if at(9.3) {
                    c.set_locked = -1;
                }
            }
            Kind::Track(kind) => {
                if let Some(track) = self.track.clone() {
                    self.follower.controls(kind, &track, car, t, &mut c);
                }
            }
            Kind::HyDeploy => {
                if t < 12.0 {
                    c.gas = 1.0;
                    c.kers = t >= 1.0;
                } else if t < 15.5 {
                    c.brake = 1.0;
                } else if t < 24.0 {
                    c.gas = 1.0;
                    c.kers = true;
                } else if t < 25.0 {
                    // lifted
                } else if t < 26.0 {
                    // the button on the brakes
                    c.brake = 0.3;
                    c.kers = true;
                } else {
                    c.gas = 0.5;
                    c.kers = (t * 2.0) as i32 % 2 == 0;
                }
            }
            Kind::HyModes => {
                // one job per moment: exactly one step
                let n = car.step - SETTLE_STEPS;
                let at = |mark: f32| n == (mark / DT) as usize;
                // 4.2 s of throttle, 1.8 s of brakes, again and again
                if t % 6.0 < 4.2 {
                    c.gas = if t % 6.0 < 3.2 { 1.0 } else { 0.4 };
                } else {
                    c.brake = 0.7;
                }
                c.kers = (9.0..9.6).contains(&t) || (20.0..21.0).contains(&t);
                for (mark, level) in [(1.0, 10), (3.0, 0), (5.0, 7), (17.0, 3), (26.0, 9)] {
                    if at(mark) {
                        c.ers_recovery = level;
                    }
                }
                for k in 0..8 {
                    if at(6.0 + 1.5 * k as f32) {
                        c.ers_power = k;
                    }
                }
                if at(18.5) {
                    c.ers_heat = -1;
                }
                if at(23.0) {
                    c.ers_heat = 1;
                }
                for (mark, index) in [(2.0, 0), (11.0, 100), (22.0, 3)] {
                    if at(mark) {
                        c.engine_brake = index;
                    }
                }
            }
            Kind::WcSpirited => {
                let phase = t * 0.4 * std::f32::consts::TAU;
                let amplitude = (0.02 * t).min(0.16);
                c.steer = amplitude * rustyac_physics::math::sinf(phase);
                // throttle in bursts: 1.6 s on, 0.5 s off
                let cycle = t % 2.1;
                c.gas = if cycle < 1.6 { 1.0 } else { 0.0 };
            }
            Kind::PtRandom => {
                if self.pull_away(car, t, &mut c) {
                    let n = car.step - SETTLE_STEPS;
                    if n % 200 == 0 {
                        let r = [self.random(), self.random(), self.random(), self.random(), self.random(), self.random()];
                        self.target[0] = (r[0] * 2.0 - 1.0) * 0.2;
                        if r[3] < 0.25 {
                            self.target[1] = 0.0;
                            self.target[2] = r[2] * 0.9;
                        } else {
                            self.target[1] = 0.2 + 0.8 * r[1];
                            self.target[2] = 0.0;
                        }
                        // sometimes a dip of the clutch, sometimes a pull at the handbrake
                        self.extra[0] = if r[4] < 0.25 { r[4] * 3.0 } else { 1.0 };
                        self.extra[1] = if r[5] < 0.15 { 0.4 + r[5] * 4.0 } else { 0.0 };
                    }
                    for i in 0..3 {
                        self.noise[i] += (self.target[i] - self.noise[i]) * 0.02;
                        self.noise[i + 3] += (self.noise[i] - self.noise[i + 3]) * 0.02;
                    }
                    self.extra[2] += (self.extra[0] - self.extra[2]) * 0.05;
                    c.steer = self.noise[3] / (1.0 + car.speed * 0.03);
                    c.gas = self.noise[4].clamp(0.0, 1.0);
                    c.brake = self.noise[5].clamp(0.0, 1.0);
                    c.clutch = self.extra[2].clamp(0.0, 1.0);
                    c.hand_brake = self.extra[1].min(1.0);
                    // a paddle press every 0.75 s, whatever the revs; the gearbox decides
                    if n % 250 == 40 {
                        let r = self.random();
                        self.paddles(r < 0.45, (0.45..0.8).contains(&r), &mut c);
                    } else {
                        self.paddles(false, false, &mut c);
                    }
                    if n % 333 == 100 {
                        c.bias_clicks = if self.random() < 0.5 { 1 } else { -1 };
                    }
                    if car.speed < 6.0 {
                        c.brake = 0.0;
                        c.hand_brake = 0.0;
                        c.gas = c.gas.max(0.4);
                    }
                }
            }
        }
        c
    }

    fn since_mark(&self, car: &CarView) -> f32 {
        (car.step - self.mark) as f32 * DT
    }

    fn next_stage(&mut self, car: &CarView) {
        self.stage += 1;
        self.mark = car.step;
    }

    /// `pt_handbrake`: paddles, no clutch aid.
    fn handbrake(&mut self, car: &CarView, t: f32, c: &mut Controls) {
        match self.stage {
            0 => {
                if self.pull_away(car, t, c) {
                    self.shift_by_rpm(car, FIRST + 6, c);
                }
                if car.kmh() >= 90.0 {
                    self.paddle = 0;
                    self.next_stage(car);
                }
            }
            1 => {
                // lift, clutch in, handbrake on over 0.3 s; the foot brake joins later
                let dt = self.since_mark(car);
                c.gas = 0.0;
                c.clutch = 0.0;
                c.hand_brake = (dt / 0.3).min(1.0);
                if dt > 0.8 {
                    c.brake = 0.5;
                }
                // cockpit brake bias: three clicks forward, then five back
                let n = car.step - self.mark;
                if n == 280 || n == 300 || n == 320 {
                    c.bias_clicks = 1;
                }
                if (400..500).contains(&n) && n % 20 == 0 {
                    c.bias_clicks = -1;
                }
                if car.speed < 0.2 && dt > 1.6 {
                    self.next_stage(car);
                }
            }
            2 => {
                // standing: down through the box to reverse, clutch held in
                c.clutch = 0.0;
                c.hand_brake = 1.0;
                self.paddles(false, car.gear > 0, c);
                if car.gear == 0 && self.paddle <= 0 {
                    self.paddle = 0;
                    self.next_stage(car);
                }
            }
            3 => {
                // backwards for 1.6 s, the clutch let up over half a second
                let dt = self.since_mark(car);
                c.gas = 0.45;
                c.clutch = (dt / 0.5).min(1.0);
                if dt > 1.6 {
                    self.next_stage(car);
                }
            }
            _ => {
                // stop: foot brake first, then the handbrake with the clutch still closed
                let dt = self.since_mark(car);
                c.gas = 0.0;
                c.brake = 0.4;
                c.clutch = if dt < 0.4 { 1.0 } else { 0.0 };
                c.hand_brake = if dt > 0.2 { 1.0 } else { 0.0 };
            }
        }
    }

    /// `pt_shifter`: the H-shifter and the clutch pedal.
    fn shifter(&mut self, car: &CarView, t: f32, c: &mut Controls) {
        match self.stage {
            0 => {
                // neutral, flat out: the limiter
                c.gas = 1.0;
                if t > 0.8 {
                    self.next_stage(car);
                }
            }
            1 => {
                // clutch in, first
                c.clutch = 0.0;
                c.gas = 0.5;
                self.lever = FIRST;
                if self.since_mark(car) > 0.2 {
                    self.next_stage(car);
                }
            }
            2 => {
                let dt = self.since_mark(car);
                c.clutch = (dt / 0.6).min(1.0);
                c.gas = 0.8;
                if dt > 0.6 {
                    self.next_stage(car);
                }
            }
            3 => {
                // driving; up-shifts with a dip of the clutch
                c.gas = 1.0;
                if t > 6.0 {
                    self.lever_target = self.lever;
                    self.stage = 5;
                    self.mark = car.step;
                } else if car.road_rpm > 17_000.0 && self.lever < FIRST + 4 {
                    self.lever_target = self.lever + 1;
                    self.next_stage(car);
                }
            }
            4 => {
                let dt = self.since_mark(car);
                c.clutch = 0.0;
                c.gas = 0.0;
                if dt > 0.05 {
                    self.lever = self.lever_target;
                }
                if dt > 0.15 {
                    self.stage = 3;
                    self.mark = car.step;
                }
            }
            5 => {
                // two gears down without the clutch, flat out: the box grinds and refuses
                let dt = self.since_mark(car);
                c.gas = 1.0;
                self.lever = (self.lever_target - 2).max(FIRST);
                if dt > 0.3 {
                    self.lever = self.lever_target;
                    self.next_stage(car);
                }
            }
            6 => {
                // a lever position that does not exist, then neutral with the clutch up
                let dt = self.since_mark(car);
                c.gas = 0.0;
                self.lever = if dt < 0.05 { 12 } else { 1 };
                if dt > 0.6 {
                    self.next_stage(car);
                }
            }
            7 => {
                // clutch in, third, clutch out: engine braking
                let dt = self.since_mark(car);
                c.gas = 0.0;
                self.lever = FIRST + 2;
                c.clutch = if dt < 0.15 { 0.0 } else { ((dt - 0.15) / 0.3).min(1.0) };
                if dt > 1.2 {
                    self.next_stage(car);
                }
            }
            _ => {
                // brake to a stop in gear, clutch in near the end, then neutral
                c.gas = 0.0;
                c.brake = 0.6;
                if car.speed < 6.0 {
                    c.clutch = 0.0;
                }
                if car.speed < 0.5 {
                    self.lever = 1;
                }
            }
        }
        c.requested_gear = self.lever;
    }

    /// `pt_protect`: paddle requests the gearbox should refuse.
    fn protect(&mut self, car: &CarView, t: f32, c: &mut Controls) {
        match self.stage {
            0 => {
                // away in first; at 12,000 rpm ask for a down-shift (neutral at speed)
                if self.pull_away(car, t, c) && car.road_rpm > 12_000.0 {
                    self.paddle = 0;
                    self.next_stage(car);
                }
            }
            1 => {
                c.gas = 1.0;
                let want = self.since_mark(car) < 0.1;
                self.paddles(false, want, c);
                if self.since_mark(car) > 0.4 {
                    self.paddle = 0;
                    self.next_stage(car);
                }
            }
            2 => {
                // up to fourth
                c.gas = 1.0;
                self.shift_by_rpm(car, FIRST + 3, c);
                if car.gear == FIRST + 3 && car.road_rpm > 16_500.0 {
                    self.paddle = 0;
                    self.next_stage(car);
                }
            }
            3 => {
                // down-shifts that would over-rev the engine, for 0.7 s
                c.gas = 1.0;
                self.paddles(false, true, c);
                if self.since_mark(car) > 0.7 {
                    self.paddle = 0;
                    self.next_stage(car);
                }
            }
            4 => {
                // on to top gear, and one more up-shift there
                c.gas = 1.0;
                let top = FIRST + 6;
                let up = car.gear >= FIRST && (car.gear < top && car.road_rpm > SHIFT_UP_RPM || car.gear == top);
                self.paddles(up, false, c);
                if t > 6.5 {
                    self.paddle = 0;
                    self.next_stage(car);
                }
            }
            _ => {
                // full brakes; the down paddle is asked for all the time
                c.gas = 0.0;
                c.brake = 1.0;
                self.paddles(false, car.gear > FIRST, c);
            }
        }
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
