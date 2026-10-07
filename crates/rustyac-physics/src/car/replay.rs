// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni). Not covered by rustyAC's licenses — see LICENSING.md.

//! The test rig of the rolling chassis. Nothing in here is part of the physics.
//!
//! * [`Ground`]: the analytic road `tools/car_oracle` gives the game's tyres.
//! * [`RecordedStep`] / [`RecordedFeed`]: a [`ChassisFeed`] filled from one step of a
//!   recording of the game.
//! * [`fields`] / [`snapshot`]: every value of a step that is compared with the game, under
//!   the names the recordings use.
//! * [`Golden`]: a small excerpt of a recording (start state, feed and the game's answers per
//!   step) for `cargo test`.

use std::path::Path;

use rustyac_ode::{JointKind, Mass};

use super::body::{kind, ForceSource, TapeCall};
use super::chassis::{ChassisEnvironment, RollingChassis, StepTrace};
use super::feed::{CarControls, ChassisFeed, EngineFeed};
use super::vanilla_car::{ScriptedDevice, VanillaCar};
use crate::tyre::rig;
use crate::tyre::{RayCastResult, RayTrackCollisionProvider, SurfaceDef};
use crate::vecmath::Vec3f;

/// AC's physics step, s.
pub const DT: f32 = 0.003;
/// Names of the wheels in the recordings, in `Car::suspensions` order.
pub const WHEELS: [&str; 4] = ["lf", "rf", "lr", "rr"];
/// Names of the rigid bodies in the recordings, in creation order.
pub const BODIES: [&str; 6] = ["body", "fuel_tank", "hub_lf", "hub_rf", "hub_lr", "hub_rr"];
/// Names of a double-wishbone corner's rods in the recordings, in creation order.
pub const RODS: [&str; 5] = ["top_rear", "top_front", "bottom_rear", "bottom_front", "steer_rod"];
/// The harness's ray is 3 m long whatever length the tyre asks for.
const RAY_LENGTH: f32 = 3.0;

/// The road of the recordings: an endless plane at height 0, optionally with a raised strip.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Ground {
    Flat,
    /// `height` where `x_min <= x <= x_max` and `z_from <= z <= z_to`, else 0.
    Step { x_min: f32, x_max: f32, z_from: f32, z_to: f32, height: f32 },
}

impl Ground {
    /// The form stored in a recording's header (`ground=`).
    pub fn parse(text: &str) -> Option<Ground> {
        let mut words = text.split(' ');
        match words.next()? {
            "flat" => Some(Ground::Flat),
            "step" => {
                let mut number = || words.next()?.parse::<f32>().ok();
                Some(Ground::Step { x_min: number()?, x_max: number()?, z_from: number()?, z_to: number()?, height: number()? })
            }
            _ => None,
        }
    }

    pub fn describe(&self) -> String {
        match *self {
            Ground::Flat => "flat".to_string(),
            Ground::Step { x_min, x_max, z_from, z_to, height } => {
                format!("step {x_min:?} {x_max:?} {z_from:?} {z_to:?} {height:?}")
            }
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

impl RayTrackCollisionProvider for Ground {
    /// `track_ray_cast` of `tools/car_oracle`: straight down from `org`, one surface with
    /// full grip, the normal straight up.
    fn ray_cast(&self, org: &Vec3f, _dir: &Vec3f, _length: f32) -> Option<RayCastResult> {
        let height = self.height(org.x, org.z);
        let hit = org.y >= height && org.y - height <= RAY_LENGTH;
        hit.then(|| RayCastResult {
            surface_def: SurfaceDef::default(),
            pos: Vec3f::new(org.x, height, org.z),
            normal: Vec3f::new(0.0, 1.0, 0.0),
        })
    }
}

/// A road that is a pit lane all over (`SurfaceDef::isPitlane`): the pit limiter works on it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PitLane(pub Ground);

impl RayTrackCollisionProvider for PitLane {
    fn ray_cast(&self, org: &Vec3f, dir: &Vec3f, length: f32) -> Option<RayCastResult> {
        let mut hit = self.0.ray_cast(org, dir, length)?;
        hit.surface_def.is_pitlane = true;
        Some(hit)
    }
}

/// One value of a ported system under the name `tools/car_oracle` records it with
/// ([`BrakeModel::trace`](super::BrakeModel::trace),
/// [`DrivetrainModel::trace`](super::DrivetrainModel::trace)).
#[derive(Clone, Debug, PartialEq)]
pub struct TraceValue {
    pub name: String,
    /// `f` f32 bits, `i` integer, `d` f64 bits.
    pub kind: char,
    /// The value; an `f` or `i` value sits in the low 32 bits.
    pub word: u64,
    /// Only the recordings of the powertrain scenarios hold it.
    pub extra: bool,
}

impl TraceValue {
    pub fn f(name: &str, value: f32) -> TraceValue {
        TraceValue { name: name.to_string(), kind: 'f', word: value.to_bits() as u64, extra: false }
    }

    pub fn i(name: &str, value: i32) -> TraceValue {
        TraceValue { name: name.to_string(), kind: 'i', word: value as u32 as u64, extra: false }
    }

    pub fn d(name: &str, value: f64) -> TraceValue {
        TraceValue { name: name.to_string(), kind: 'd', word: value.to_bits(), extra: false }
    }

    pub fn extra(mut self) -> TraceValue {
        self.extra = true;
        self
    }
}

/// What the systems that are not ported left in one tyre before its step.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RecordedWheel {
    /// `Tyre::inputs.brakeTorque` (brakes, EDL)
    pub brake_torque: f32,
    /// `Tyre::inputs.handBrakeTorque` (brakes)
    pub hand_brake_torque: f32,
    /// `Tyre::inputs.electricTorque` (ERS)
    pub electric_torque: f32,
    /// `Tyre::absOverride` (ABS)
    pub abs_override: f32,
    /// `Tyre::aiMult` (AI driver)
    pub ai_mult: f32,
    /// `Tyre::driven` (drivetrain)
    pub driven: bool,
    /// Driven wheels only: `status.angularVelocity` as the drivetrain left it.
    pub angular_velocity: f32,
    /// Driven wheels only: `localWheelRotation` as the drivetrain left it, row-major.
    pub local_wheel_rotation: [f32; 16],
}

/// One force call of a system that is not ported (the wings), from the game's force tape.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RecordedCall {
    /// One of [`kind`].
    pub kind: u32,
    pub source: ForceSource,
    pub a: [f32; 3],
    pub b: [f32; 3],
}

/// Everything one step of the chassis is fed. A chassis with its own brakes ignores the
/// wheels' brake torques, one with its own drivetrain ignores `clutch`, `gear`, `engine` and
/// the driven wheels' speed and spin matrix.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RecordedStep {
    /// What the driver's device reported (`script.*`).
    pub controls: CarControls,
    /// Clicks of the cockpit brake-bias control asked for before this step.
    pub bias_clicks: i32,
    /// The device's headlight switch is held (`getAction(4)`).
    pub headlights: bool,
    /// `controls.clutch` after the automatic clutch.
    pub clutch: f32,
    /// `Drivetrain::currentGear` at the start of the step.
    pub gear: i32,
    /// The engine's values at the start of the step.
    pub engine: EngineFeed,
    /// `Engine::electronicOverride` as traction control and the pit limiter left it at the
    /// end of this step (it acts on the next step's engine).
    pub engine_electronic_override: f32,
    /// `BrakeSystem::electronicOverride` as the pit limiter left it at the end of this step.
    pub brake_electronic_override: f32,
    pub wheels: [RecordedWheel; 4],
    /// The wings' calls on the car body, in order.
    pub aero: Vec<RecordedCall>,
    /// The stability aid's torque calls on the car body (for a chassis that is fed its aids).
    pub stability: Vec<RecordedCall>,
    /// The electronic differential lock braked a wheel in this step: a chassis that is fed
    /// its aids but has its own brakes then takes the wheels' brake torques (`brake_torque`,
    /// which holds brakes plus lock) from the recording.
    pub edl_active: bool,
}

impl RecordedStep {
    /// The same step with nothing in it but what the driver did: the controls of the device
    /// and the clicks of the cockpit brake bias. This is all a whole car is given.
    pub fn driver_only(&self) -> RecordedStep {
        RecordedStep {
            controls: self.controls,
            bias_clicks: self.bias_clicks,
            headlights: self.headlights,
            ..RecordedStep::default()
        }
    }

    /// The step as 32-bit words (the golden file's layout).
    pub fn to_words(&self, out: &mut Vec<u32>) {
        let c = &self.controls;
        out.extend([c.gas, c.brake, c.steer, c.clutch, c.hand_brake].map(f32::to_bits));
        out.extend([c.gear_up as u32, c.gear_dn as u32, c.drs as u32, c.kers as u32, c.requested_gear_index as u32, self.bias_clicks as u32]);
        out.push(self.clutch.to_bits());
        out.push(self.gear as u32);
        out.extend([self.engine.rpm, self.engine.gas_usage, self.engine.turbo_boost].map(f32::to_bits));
        out.extend([self.engine_electronic_override, self.brake_electronic_override].map(f32::to_bits));
        for w in &self.wheels {
            out.extend([w.brake_torque, w.hand_brake_torque, w.electric_torque, w.abs_override, w.ai_mult].map(f32::to_bits));
            out.push(w.driven as u32);
            if w.driven {
                out.push(w.angular_velocity.to_bits());
                out.extend(w.local_wheel_rotation.map(f32::to_bits));
            }
        }
        for calls in [&self.aero, &self.stability] {
            out.push(calls.len() as u32);
            for call in calls {
                out.push(call.kind);
                out.push(source_index(call.source));
                out.extend(call.a.map(f32::to_bits));
                out.extend(call.b.map(f32::to_bits));
            }
        }
        out.push(self.edl_active as u32);
        out.push(self.headlights as u32);
    }

    pub fn from_words(words: &mut impl Iterator<Item = u32>) -> Option<RecordedStep> {
        let f = |words: &mut dyn Iterator<Item = u32>| words.next().map(f32::from_bits);
        let (gas, brake, steer, pedal, hand_brake) = (f(words)?, f(words)?, f(words)?, f(words)?, f(words)?);
        let controls = CarControls {
            gas,
            brake,
            steer,
            clutch: pedal,
            hand_brake,
            gear_up: words.next()? != 0,
            gear_dn: words.next()? != 0,
            drs: words.next()? != 0,
            kers: words.next()? != 0,
            requested_gear_index: words.next()? as i32,
        };
        let bias_clicks = words.next()? as i32;
        let clutch = f(words)?;
        let gear = words.next()? as i32;
        let engine = EngineFeed { rpm: f(words)?, gas_usage: f(words)?, turbo_boost: f(words)? };
        let engine_electronic_override = f(words)?;
        let brake_electronic_override = f(words)?;
        let mut wheels = [RecordedWheel::default(); 4];
        for w in &mut wheels {
            w.brake_torque = f(words)?;
            w.hand_brake_torque = f(words)?;
            w.electric_torque = f(words)?;
            w.abs_override = f(words)?;
            w.ai_mult = f(words)?;
            w.driven = words.next()? != 0;
            if w.driven {
                w.angular_velocity = f(words)?;
                for value in &mut w.local_wheel_rotation {
                    *value = f(words)?;
                }
            }
        }
        let mut lists = [Vec::new(), Vec::new()];
        for list in &mut lists {
            let count = words.next()? as usize;
            for _ in 0..count {
                let kind = words.next()?;
                let source = source_from_index(words.next()?)?;
                let a = [f(words)?, f(words)?, f(words)?];
                let b = [f(words)?, f(words)?, f(words)?];
                list.push(RecordedCall { kind, source, a, b });
            }
        }
        let [aero, stability] = lists;
        let edl_active = words.next()? != 0;
        let headlights = words.next()? != 0;
        Some(RecordedStep {
            controls,
            bias_clicks,
            clutch,
            gear,
            engine,
            engine_electronic_override,
            brake_electronic_override,
            wheels,
            aero,
            stability,
            edl_active,
            headlights,
        })
    }
}

const SOURCES: [ForceSource; 18] = [
    ForceSource::Tyre,
    ForceSource::Surface,
    ForceSource::Spring,
    ForceSource::Damper,
    ForceSource::Bumpstop,
    ForceSource::HeaveSpring,
    ForceSource::HeaveDamper,
    ForceSource::HeaveBumpstop,
    ForceSource::Arb,
    ForceSource::AeroDrag,
    ForceSource::AeroLift,
    ForceSource::Drivetrain,
    ForceSource::Brake,
    ForceSource::Steering,
    ForceSource::Stability,
    ForceSource::Sleep,
    ForceSource::Teleport,
    ForceSource::Other,
];

fn source_index(source: ForceSource) -> u32 {
    SOURCES.iter().position(|s| *s == source).unwrap() as u32
}

fn source_from_index(index: u32) -> Option<ForceSource> {
    SOURCES.get(index as usize).copied()
}

/// One step of the chassis with a recorded feed: first the commands the game runs from its
/// queue before the step (a click of the cockpit brake bias; they do not depend on the
/// controls being polled), then [`RollingChassis::step`] with a [`RecordedFeed`].
pub fn step_recorded(chassis: &mut RollingChassis, physics_time: f64, step: &RecordedStep) {
    if step.bias_clicks != 0 {
        if let Some(brakes) = &mut chassis.brake_system {
            brakes.set_manual_front_bias(step.bias_clicks);
        }
    }
    // PhysicsEngine::stepWind runs before the cars
    chassis.env.step_wind(physics_time);
    // The game's drivetrain left the driven wheels' speed at the end of the step before; a
    // chassis with its own brakes reads it for the disc temperatures, ahead of the `edl` hook.
    if chassis.drivetrain.is_none() {
        for (tyre, wheel) in chassis.tyres.iter_mut().zip(&step.wheels) {
            if wheel.driven {
                tyre.status.angular_velocity = wheel.angular_velocity;
            }
        }
    }
    chassis.step(DT, physics_time, &mut RecordedFeed { step });
}

/// A car under test: either a chassis that is still fed some systems from a recording, or a
/// whole [`VanillaCar`] with a scripted device, which is given the driver's controls and
/// nothing else. Reads like the chassis inside.
pub enum Runner {
    Fed(RollingChassis),
    Whole(VanillaCar<ScriptedDevice>),
}

impl std::ops::Deref for Runner {
    type Target = RollingChassis;

    fn deref(&self) -> &RollingChassis {
        match self {
            Runner::Fed(chassis) => chassis,
            Runner::Whole(car) => &car.car,
        }
    }
}

impl std::ops::DerefMut for Runner {
    fn deref_mut(&mut self) -> &mut RollingChassis {
        match self {
            Runner::Fed(chassis) => chassis,
            Runner::Whole(car) => &mut car.car,
        }
    }
}

impl Runner {
    /// Is this a whole car (driver's controls only)?
    pub fn is_whole(&self) -> bool {
        matches!(self, Runner::Whole(_))
    }

    /// One step with what `step` holds. A whole car reads only `step.controls` and
    /// `step.bias_clicks`.
    pub fn step_recorded(&mut self, physics_time: f64, step: &RecordedStep) {
        match self {
            Runner::Fed(chassis) => step_recorded(chassis, physics_time, step),
            Runner::Whole(car) => {
                // what the game's command queue runs before the step
                if step.bias_clicks != 0 {
                    if let Some(brakes) = &mut car.car.brake_system {
                        brakes.set_manual_front_bias(step.bias_clicks);
                    }
                }
                car.device.controls = step.controls;
                car.device.headlights = step.headlights;
                car.step(DT, physics_time);
            }
        }
    }
}

/// A [`ChassisFeed`] that hands the chassis one recorded step.
///
/// The recordings hold the tyre's inputs as `Tyre::step` found them, so everything the ABS,
/// the ERS, the AI and (for a chassis without its own) the drivetrain write into a tyre is
/// put there in the `edl` hook, just before the suspensions and tyres run; the `drivetrain`
/// hook has nothing left to do. What the aids leave for the next step's engine and brakes is
/// written in the `aids` hook.
///
/// The recordings hold only the sum of brake and differential-lock torque, so for a chassis
/// with its own brakes and fed aids the `edl` hook writes that sum over the brakes' own torque
/// in the steps where the lock acts.
pub struct RecordedFeed<'a> {
    pub step: &'a RecordedStep,
}

impl ChassisFeed for RecordedFeed<'_> {
    fn poll_controls(&mut self, chassis: &mut RollingChassis) {
        chassis.controls = self.step.controls;
    }

    fn get_action(&mut self, action: i32) -> bool {
        action == 4 && self.step.headlights
    }

    fn engine(&mut self, _chassis: &RollingChassis) -> EngineFeed {
        self.step.engine
    }

    fn autoclutch(&mut self, chassis: &mut RollingChassis) {
        chassis.controls.clutch = self.step.clutch;
    }

    fn current_gear(&mut self, _chassis: &RollingChassis) -> i32 {
        self.step.gear
    }

    fn brakes(&mut self, chassis: &mut RollingChassis) {
        for (tyre, wheel) in chassis.tyres.iter_mut().zip(&self.step.wheels) {
            tyre.inputs.brake_torque = wheel.brake_torque;
            tyre.inputs.hand_brake_torque = wheel.hand_brake_torque;
        }
    }

    fn edl(&mut self, chassis: &mut RollingChassis) {
        let fed_drivetrain = chassis.drivetrain.is_none();
        let fed_aids = chassis.aids.is_none();
        for (tyre, wheel) in chassis.tyres.iter_mut().zip(&self.step.wheels) {
            if fed_aids {
                tyre.inputs.electric_torque = wheel.electric_torque;
                tyre.abs_override = wheel.abs_override;
                tyre.ai_mult = wheel.ai_mult;
            }
            if fed_drivetrain {
                tyre.driven = wheel.driven;
                if wheel.driven {
                    tyre.status.angular_velocity = wheel.angular_velocity;
                    for (k, value) in wheel.local_wheel_rotation.iter().enumerate() {
                        tyre.local_wheel_rotation.m[k / 4][k % 4] = *value;
                    }
                }
            }
        }
        // the differential lock's brake torque, for a chassis with its own brakes: the
        // recording holds only the sum the tyres saw
        if fed_aids && chassis.brake_system.is_some() && self.step.edl_active {
            for (tyre, wheel) in chassis.tyres.iter_mut().zip(&self.step.wheels) {
                tyre.inputs.brake_torque = wheel.brake_torque;
            }
        }
    }

    fn aero(&mut self, chassis: &mut RollingChassis) {
        let previous = chassis.core.source;
        for call in &self.step.aero {
            chassis.core.source = call.source;
            let (a, b) = (Vec3f::new(call.a[0], call.a[1], call.a[2]), Vec3f::new(call.b[0], call.b[1], call.b[2]));
            chassis.core.apply_call(chassis.body, call.kind, &a, &b).expect("a force call from the recording");
        }
        chassis.core.source = previous;
    }

    fn drivetrain(&mut self, _chassis: &mut RollingChassis) {}

    fn aids(&mut self, chassis: &mut RollingChassis) {
        if let Some(drivetrain) = &mut chassis.drivetrain {
            drivetrain.engine_mut().base_mut().electronic_override = self.step.engine_electronic_override;
        }
        if let Some(brakes) = &mut chassis.brake_system {
            brakes.base_mut().electronic_override = self.step.brake_electronic_override;
        }
    }

    fn stability(&mut self, chassis: &mut RollingChassis) {
        let previous = chassis.core.source;
        for call in &self.step.stability {
            chassis.core.source = call.source;
            let (a, b) = (Vec3f::new(call.a[0], call.a[1], call.a[2]), Vec3f::new(call.b[0], call.b[1], call.b[2]));
            chassis.core.apply_call(chassis.body, call.kind, &a, &b).expect("a torque call from the recording");
        }
        chassis.core.source = previous;
    }
}

/// A value compared with the game.
#[derive(Clone, Debug, PartialEq)]
pub struct Field {
    /// The name in a `car_oracle` recording.
    pub name: String,
    /// `f` f32 bits, `i` integer, `d` f64 bits.
    pub kind: char,
}

fn field(out: &mut Vec<Field>, kind: char, name: String) {
    out.push(Field { name, kind });
}

fn vector(out: &mut Vec<Field>, name: &str, axes: &[&str]) {
    for axis in axes {
        field(out, 'f', format!("{name}.{axis}"));
    }
}

const XYZ: [&str; 3] = ["x", "y", "z"];
const WXYZ: [&str; 4] = ["w", "x", "y", "z"];
const NINE: [&str; 9] = ["0", "1", "2", "3", "4", "5", "6", "7", "8"];

/// The names of the joints in the recordings, in creation order.
pub fn joint_names() -> Vec<String> {
    let mut names = vec!["fuel_tank".to_string()];
    for wheel in WHEELS {
        for rod in RODS {
            names.push(format!("{wheel}.{rod}"));
        }
    }
    names
}

/// Every value [`snapshot`] returns, in its order.
pub fn fields() -> Vec<Field> {
    let mut out = Vec::new();
    field(&mut out, 'f', "car.finalSteerAngleSignal".into());
    for body in BODIES {
        field(&mut out, 'f', format!("{body}.mass"));
        vector(&mut out, &format!("{body}.inertia"), &XYZ);
        for part in ["pre", "post"] {
            vector(&mut out, &format!("{body}.{part}.pos"), &XYZ);
            vector(&mut out, &format!("{body}.{part}.q"), &WXYZ);
            vector(&mut out, &format!("{body}.{part}.R"), &NINE);
            vector(&mut out, &format!("{body}.{part}.lvel"), &XYZ);
            vector(&mut out, &format!("{body}.{part}.avel"), &XYZ);
            if part == "pre" {
                vector(&mut out, &format!("{body}.facc"), &XYZ);
                vector(&mut out, &format!("{body}.tacc"), &XYZ);
            }
        }
        field(&mut out, 'i', format!("{body}.tag"));
    }
    for (j, name) in joint_names().iter().enumerate() {
        let n = format!("joint.{name}");
        if j == 0 {
            vector(&mut out, &format!("{n}.qrel"), &WXYZ);
            vector(&mut out, &format!("{n}.offset"), &XYZ);
        } else {
            vector(&mut out, &format!("{n}.anchor1"), &XYZ);
            vector(&mut out, &format!("{n}.anchor2"), &XYZ);
        }
        field(&mut out, 'f', format!("{n}.erp"));
        field(&mut out, 'f', format!("{n}.cfm"));
        if j != 0 {
            field(&mut out, 'f', format!("{n}.distance"));
        }
        field(&mut out, 'i', format!("{n}.tag"));
        for group in ["f1", "t1", "f2", "t2"] {
            vector(&mut out, &format!("{n}.{group}"), &XYZ);
        }
    }
    for wheel in WHEELS {
        for name in rig::input_fields().into_iter().chain(rig::output_fields()) {
            field(&mut out, rig::field_kind(&name), format!("tyre.{wheel}.{name}"));
        }
        for k in 0..16 {
            field(&mut out, 'f', format!("tyre.{wheel}.in_localWheelRotation.M{}{}", k / 4 + 1, k % 4 + 1));
        }
    }
    field(&mut out, 'f', "car.speed".into());
    vector(&mut out, "car.accG", &XYZ);
    field(&mut out, 'd', "car.fuel".into());
    field(&mut out, 'f', "car.mass".into());
    field(&mut out, 'f', "car.lastFF".into());
    field(&mut out, 'f', "car.mzCurrent".into());
    field(&mut out, 'i', "car.sleepingFrames".into());
    for wheel in WHEELS {
        for name in ["travel", "damperSpeedMS", "steerTorque", "bumpStopDn"] {
            field(&mut out, 'f', format!("suspension.{wheel}.{name}"));
        }
    }
    for axle in ["front", "rear"] {
        field(&mut out, 'f', format!("heave.{axle}.travel"));
        field(&mut out, 'f', format!("arb.{axle}.k"));
    }
    field(&mut out, 'f', "car.ballastKG".into());
    field(&mut out, 'f', "car.steerLock".into());
    field(&mut out, 'f', "car.steerRatio".into());
    out
}

/// The values of [`fields`] after a step that ran with `chassis.trace` on. An `f` or `i`
/// value sits in the low 32 bits.
pub fn snapshot(chassis: &RollingChassis) -> Vec<u64> {
    let trace: &StepTrace = chassis.trace.as_ref().expect("the step must run with a trace");
    let f = |x: f32| x.to_bits() as u64;
    let mut out = Vec::new();
    out.push(f(chassis.final_steer_angle_signal));
    for (body, pre) in chassis.core.bodies().zip(&trace.pre) {
        let post = chassis.body_trace(body);
        out.push(f(pre.mass));
        out.extend(pre.inertia.map(f));
        for (state, is_pre) in [(pre, true), (&post, false)] {
            out.extend(state.pos.map(f));
            out.extend(state.q.map(f));
            out.extend(state.r.map(f));
            out.extend(state.lvel.map(f));
            out.extend(state.avel.map(f));
            if is_pre {
                out.extend(state.facc.map(f));
                out.extend(state.tacc.map(f));
            }
        }
        out.push(chassis.core.world.body(body.id).tag as u32 as u64);
    }
    let mut joints = vec![chassis.fuel_tank_joint.id];
    for suspension in &chassis.suspensions {
        joints.extend(suspension.joints().iter().map(|joint| joint.id));
    }
    for id in joints {
        let joint = chassis.core.world.joint(id);
        match &joint.kind {
            JointKind::Fixed { qrel, offset, erp, cfm } => {
                out.extend(qrel.map(f));
                out.extend([offset[0], offset[1], offset[2], *erp, *cfm].map(f));
            }
            JointKind::DBall { anchor1, anchor2, erp, cfm, target_distance } => {
                out.extend([anchor1[0], anchor1[1], anchor1[2], anchor2[0], anchor2[1], anchor2[2]].map(f));
                out.extend([*erp, *cfm, *target_distance].map(f));
            }
            _ => unreachable!("the chassis has rods and one fixed joint"),
        }
        out.push(joint.tag as u32 as u64);
        let fb = joint.feedback.unwrap_or_default();
        for v in [fb.f1, fb.t1, fb.f2, fb.t2] {
            out.extend([v[0], v[1], v[2]].map(f));
        }
    }
    for tyre in &trace.tyres {
        out.extend(tyre.input.to_words());
        out.extend(tyre.output.iter().copied());
        for row in &tyre.wheel_rotation_in.m {
            out.extend(row.map(f));
        }
    }
    out.push(f(chassis.speed));
    out.extend([chassis.acc_g.x, chassis.acc_g.y, chassis.acc_g.z].map(f));
    out.push(chassis.fuel.to_bits());
    out.push(f(chassis.mass));
    out.push(f(chassis.last_ff));
    out.push(f(chassis.mz_current));
    out.push(chassis.sleeping_frames as u32 as u64);
    for suspension in &chassis.suspensions {
        let status = suspension.get_status();
        out.extend([status.travel, status.damper_speed_ms, suspension.get_steer_torque(), suspension.base().bump_stop_dn].map(f));
    }
    for axle in 0..2 {
        out.push(f(chassis.heave_springs[axle].travel));
        out.push(f(chassis.antiroll_bars[axle].k));
    }
    out.extend([chassis.ballast_kg, chassis.steer_lock, chassis.steer_ratio].map(f));
    out
}

/// The values of the ported brakes, engine, drivetrain and shift helpers after a step, under
/// the names of the recordings; empty for a bare chassis. With a drivetrain the list starts
/// with `Car::controls` as the helpers left them.
pub fn powertrain_trace(chassis: &RollingChassis) -> Vec<TraceValue> {
    let mut out = Vec::new();
    if let Some(drivetrain) = &chassis.drivetrain {
        let c = &chassis.controls;
        out.push(TraceValue::f("controls.steer", c.steer));
        out.push(TraceValue::f("controls.gas", c.gas));
        out.push(TraceValue::f("controls.brake", c.brake));
        out.push(TraceValue::f("controls.clutch", c.clutch));
        out.push(TraceValue::f("controls.handBrake", c.hand_brake));
        out.push(TraceValue::i("controls.gearUp", c.gear_up as i32));
        out.push(TraceValue::i("controls.gearDn", c.gear_dn as i32));
        out.push(TraceValue::i("controls.requestedGearIndex", c.requested_gear_index));
        drivetrain.trace(&mut out);
        out.push(TraceValue::f("car.waterTemperature", chassis.water.t));
        out.push(TraceValue::i("autoShift.isActive", chassis.auto_shifter.is_active as i32));
        out.push(TraceValue::i("autoBlip.isActive", chassis.auto_blip.is_active as i32));
        out.push(TraceValue::i("autoClutch.useAutoOnStart", chassis.autoclutch.use_auto_on_start as i32));
        out.push(TraceValue::i("autoClutch.useAutoOnChange", chassis.autoclutch.use_auto_on_change as i32));
        out.push(TraceValue::f("autoClutch.clutchValueSignal", chassis.autoclutch.clutch_value_signal));
        out.push(TraceValue::d("autoBlip.blipStartTime", chassis.auto_blip.blip_start_time).extra());
        out.push(TraceValue::f("autoShift.gasCutoff", chassis.auto_shifter.gas_cutoff).extra());
        out.push(TraceValue::i("autoShift.changeUpRpm", chassis.auto_shifter.change_up_rpm).extra());
        out.push(TraceValue::i("autoShift.changeDnRpm", chassis.auto_shifter.change_dn_rpm).extra());
        out.push(TraceValue::i("autoClutch.isForced", chassis.autoclutch.is_forced as i32).extra());
    }
    if let Some(brakes) = &chassis.brake_system {
        brakes.trace(&mut out);
    }
    if let Some(aero) = &chassis.aero {
        aero.trace(&mut out);
    }
    if chassis.drivetrain.is_some() {
        // the car-level glue of `Car::step` / `Car::postStep` (whole-car recordings hold them)
        out.push(TraceValue::f("car.vibrationPhase", chassis.vibration_phase).extra());
        out.push(TraceValue::f("car.slipVibrationPhase", chassis.slip_vibration_phase).extra());
        out.push(TraceValue::i("car.lightsOn", chassis.lights_on as i32).extra());
        out.push(TraceValue::i("car.isCollisionOffForPits", chassis.is_collision_off_for_pits as i32).extra());
        out.push(TraceValue::i("car.hasGridPosition", chassis.has_grid_position as i32).extra());
        let s = &chassis.slip_stream;
        for (name, v) in [("car.gridPosition", chassis.grid_position), ("car.slipStream.tip", s.tip), ("car.slipStream.dir", s.dir)] {
            for (axis, value) in ["x", "y", "z"].iter().zip([v.x, v.y, v.z]) {
                out.push(TraceValue::f(&format!("{name}.{axis}"), value).extra());
            }
        }
        out.push(TraceValue::f("car.slipStream.length", s.length).extra());
        out.push(TraceValue::d("car.lockControlsTime", chassis.lock_controls_time).extra());
        out.push(TraceValue::d("car.penaltyTimeAccumulator", chassis.penalty_time_accumulator).extra());
        out.push(TraceValue::i("car.disableMinSpeedPenaltyClear", chassis.disable_min_speed_penalty_clear as i32).extra());
        out.push(TraceValue::i("car.isGentleStopping", chassis.is_gentle_stopping as i32).extra());
        out.push(TraceValue::i("car.meshCollideMask", chassis.mesh_collide_mask as i32).extra());
        out.push(TraceValue::i("car.isControlsLocked", chassis.is_controls_locked as i32));
        out.push(TraceValue::i("car.blackFlagged", chassis.black_flagged as i32));
        out.push(TraceValue::d("car.penaltyTime", chassis.penalty_time));
    }
    if let Some(page) = &chassis.physics_page {
        // the telemetry page of this step, value by value
        for (name, kind, word) in page.named() {
            out.push(TraceValue { name: format!("page.{name}"), kind, word: word as u64, extra: false });
        }
    }
    if let Some(aids) = &chassis.aids {
        aids.trace(&mut out);
        // `Tyre::absOverride` as ABS left it for the next step
        for (tyre, wheel) in chassis.tyres.iter().zip(WHEELS) {
            out.push(TraceValue::f(&format!("abs.override.{wheel}"), tyre.abs_override).extra());
        }
    }
    out
}

/// Are two values of a field the same? A NaN equals any NaN (its sign and payload depend on
/// operand order the compiler may choose).
pub fn same_value(kind: char, expected: u64, got: u64) -> bool {
    rig::same_value(kind, expected, got)
}

/// A value as text, with its bits.
pub fn describe(kind: char, word: u64) -> String {
    match kind {
        'd' => format!("{:?} ({word:#018x})", f64::from_bits(word)),
        'i' => format!("{}", word as u32 as i32),
        _ => format!("{:?} ({:#010x})", f32::from_bits(word as u32), word as u32),
    }
}

fn fnv(hash: &mut u64, bytes: &[u8]) {
    for &byte in bytes {
        *hash = (*hash ^ byte as u64).wrapping_mul(0x0000_0100_0000_01b3);
    }
}

/// One number for everything a step produced: an FNV-1a 64 hash over the [`snapshot`] words
/// and the force tape. A NaN is hashed as one canonical NaN.
pub fn step_hash(kinds: &[char], words: &[u64], tape: &[TapeCall]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    let float = |x: f32| if x.is_nan() { f32::NAN.to_bits() } else { x.to_bits() };
    for (&kind, &word) in kinds.iter().zip(words) {
        match kind {
            'd' => {
                let value = f64::from_bits(word);
                let bits = if value.is_nan() { f64::NAN.to_bits() } else { word };
                fnv(&mut hash, &bits.to_le_bytes());
            }
            'i' => fnv(&mut hash, &(word as u32).to_le_bytes()),
            _ => fnv(&mut hash, &float(f32::from_bits(word as u32)).to_le_bytes()),
        }
    }
    for call in tape {
        fnv(&mut hash, &call.body.to_le_bytes());
        fnv(&mut hash, &call.kind.to_le_bytes());
        fnv(&mut hash, &source_index(call.source).to_le_bytes());
        for v in [call.a, call.b, call.facc, call.tacc] {
            for x in v {
                fnv(&mut hash, &float(x).to_le_bytes());
            }
        }
    }
    hash
}

/// How a run of the chassis is set up to match a recording.
#[derive(Clone, Debug, PartialEq)]
pub struct RunSetup {
    /// The recording's scenario name.
    pub scenario: String,
    pub ground: Ground,
    /// `srand` seed of the game's C runtime when the car was built.
    pub seed: u32,
    /// The physics clock before the first step, ms; step `n` runs at `clock + 3 * (n + 1)`.
    pub clock_start_ms: f64,
    pub env: ChassisEnvironment,
    /// The chassis computes its brakes itself ([`RollingChassis::install_brakes`]).
    pub rust_brakes: bool,
    /// The chassis has its own engine, drivetrain and shift helpers
    /// ([`RollingChassis::install_drivetrain`]).
    pub rust_drivetrain: bool,
    /// The chassis has its own wings and DRS ([`RollingChassis::install_aero`]).
    pub rust_aero: bool,
    /// The chassis has its own driver aids ([`RollingChassis::install_aids`]).
    pub rust_aids: bool,
    /// The car writes the telemetry page ([`RollingChassis::install_telemetry`]).
    pub telemetry: bool,
    /// The whole road is a pit lane.
    pub pitlane: bool,
    /// `StabilityControl::gain`: the game's stability aid (0 = off).
    pub stability_gain: f32,
    /// The wind handed to the session: speed in m/s (0 = none) and direction in degrees.
    pub wind_speed: f32,
    pub wind_direction_deg: f32,
    /// `Car::damageZoneLevel` the car starts with (front, rear, left, right, centre).
    pub damage: [f32; 5],
    /// The "automatic clutch" driving aid, as the recording's scenario set it.
    pub auto_clutch: bool,
    /// The "automatic gearbox" driving aid.
    pub auto_shifter: bool,
}

impl RunSetup {
    /// Builds the F2004-style chassis of a recording the way the oracle built the game's car:
    /// `Car::Car`, the spawn at the origin facing +z, then the session start.
    pub fn build(&self, data_path: &Path) -> Result<RollingChassis, String> {
        let ground: Box<dyn RayTrackCollisionProvider> =
            if self.pitlane { Box::new(PitLane(self.ground)) } else { Box::new(self.ground) };
        let mut env = self.env;
        if self.wind_speed != 0.0 {
            env.set_wind(self.wind_speed, self.wind_direction_deg);
        }
        let mut chassis = RollingChassis::new(data_path, env, ground, self.seed, self.clock_start_ms)?;
        if self.rust_aero {
            chassis.install_aero()?;
        }
        if self.rust_brakes {
            chassis.install_brakes()?;
        }
        if self.rust_drivetrain {
            chassis.install_drivetrain()?;
            // the driving aids as the oracle set them (`CarAvatar::setAutoClutchEnabled`: the
            // aid switches the automatic clutch at the start, and with it the one on shifts)
            chassis.autoclutch.use_auto_on_start = self.auto_clutch;
            if self.auto_clutch {
                chassis.autoclutch.use_auto_on_change = true;
            }
            chassis.auto_shifter.is_active = self.auto_shifter;
        }
        if self.rust_aids {
            chassis.install_aids()?;
            if let Some(aids) = &mut chassis.aids {
                aids.base_mut().stability_control.gain = self.stability_gain;
            }
        }
        if self.telemetry {
            chassis.install_telemetry()?;
            // as `tools/car_oracle` sets the game's writer up: warmed up, and with a zeroed
            // car avatar behind it (so the cockpit's engine-brake setting reads 0)
            if let Some(writer) = &mut chassis.telemetry {
                writer.null_counts = 300;
                writer.engine_brake_setting = 0;
            }
        }
        chassis.core.joint_feedback = true;
        // the joints exist already: ask for their constraint forces as the oracle did
        let ids: Vec<_> = chassis.core.world.joint_ids().collect();
        for id in ids {
            chassis.core.world.joint_set_feedback(id, true);
        }
        chassis.force_rotation(&Vec3f::new(0.0, 0.0, -1.0));
        chassis.force_position(&Vec3f::new(0.0, 0.0, 0.0));
        chassis.damage_zone_level = self.damage;
        chassis.session_start()?;
        chassis.core.tape = Some(Vec::new());
        chassis.trace = Some(StepTrace::default());
        Ok(chassis)
    }

    /// Every system is computed in Rust: the car is a whole car.
    pub fn is_whole(&self) -> bool {
        self.rust_brakes && self.rust_drivetrain && self.rust_aero && self.rust_aids
    }

    /// As [`RunSetup::build`]; a whole car comes back as a [`VanillaCar`] with a scripted
    /// device, which takes nothing but the driver's controls.
    pub fn build_runner(&self, data_path: &Path) -> Result<Runner, String> {
        let chassis = self.build(data_path)?;
        if self.is_whole() {
            Ok(Runner::Whole(VanillaCar::from_chassis(chassis, ScriptedDevice::default())?))
        } else {
            Ok(Runner::Fed(chassis))
        }
    }

    /// The physics clock of a step, ms.
    pub fn time_of_step(&self, step: usize) -> f64 {
        self.clock_start_ms + (step as f64 + 1.0) * 3.0
    }
}

impl RollingChassis {
    /// Everything that changes from step to step, as 32-bit words: the bodies, the joints,
    /// the tyres and the car's own counters. What the car's files and the setup decide is not
    /// in here, so a state only fits a chassis built and set up the same way.
    pub fn save_state(&self) -> Vec<u32> {
        let mut out = Vec::new();
        let f = f32::to_bits;
        for body in self.core.bodies() {
            let b = self.core.world.body(body.id);
            out.extend([b.pos[0], b.pos[1], b.pos[2]].map(f));
            out.extend(b.q.map(f));
            out.extend(b.r.map(f));
            out.extend([b.lvel[0], b.lvel[1], b.lvel[2], b.avel[0], b.avel[1], b.avel[2]].map(f));
            out.extend([b.mass.mass, b.mass.i[0], b.mass.i[5], b.mass.i[10]].map(f));
        }
        for id in self.core.world.joint_ids() {
            match &self.core.world.joint(id).kind {
                JointKind::Fixed { qrel, offset, erp, cfm } => {
                    out.extend(qrel.map(f));
                    out.extend([offset[0], offset[1], offset[2], *erp, *cfm].map(f));
                }
                JointKind::DBall { anchor1, anchor2, erp, cfm, target_distance } => {
                    out.extend([anchor1[0], anchor1[1], anchor1[2], anchor2[0], anchor2[1], anchor2[2]].map(f));
                    out.extend([*erp, *cfm, *target_distance].map(f));
                }
                _ => unreachable!("the chassis has rods and one fixed joint"),
            }
        }
        for tyre in &self.tyres {
            for word in rig::snapshot(tyre, &[]) {
                out.push(word as u32);
                out.push((word >> 32) as u32);
            }
        }
        for suspension in &self.suspensions {
            let status = suspension.get_status();
            out.extend([status.travel, status.damper_speed_ms, suspension.get_steer_torque()].map(f));
        }
        out.extend([self.heave_springs[0].rod_length, self.heave_springs[0].travel].map(f));
        out.extend([self.heave_springs[1].rod_length, self.heave_springs[1].travel].map(f));
        let c = &self.controls;
        out.extend([c.gas, c.brake, c.steer, c.clutch].map(f));
        out.extend(
            [
                self.final_steer_angle_signal,
                self.acc_g.x,
                self.acc_g.y,
                self.acc_g.z,
                self.last_velocity.x,
                self.last_velocity.y,
                self.last_velocity.z,
                self.mz_current,
                self.last_ff,
                self.last_pure_mz_ff,
                self.last_gyro_ff,
                self.last_steer_position,
                self.flat_spot_phase,
                self.speed,
                self.fuel_pressure,
            ]
            .map(f),
        );
        out.push(self.sleeping_frames as u32);
        out.push(self.frames_to_sleep as u32);
        for value in [self.fuel.to_bits(), self.last_body_mass_update_time.to_bits(), self.physics_time.to_bits()] {
            out.push(value as u32);
            out.push((value >> 32) as u32);
        }
        // the smoothing state of the anti-roll bars' controllers (cars that have them)
        for bar in &self.antiroll_bars {
            out.extend(bar.ctrl.stages.iter().map(|stage| stage.current_value.to_bits()));
        }
        // the ported systems, when the chassis has them
        if let Some(brakes) = &self.brake_system {
            brakes.save_state(&mut out);
        }
        if let Some(drivetrain) = &self.drivetrain {
            drivetrain.save_state(&mut out);
            out.extend([c.hand_brake, self.water.t, self.water.heat_accumulator].map(f));
            out.extend([c.gear_up as u32, c.gear_dn as u32, c.kers as u32, c.requested_gear_index as u32]);
            let a = &self.autoclutch;
            out.extend([a.clutch_value_signal, a.clutch_sequence.current_time].map(f));
            out.extend([a.use_auto_on_start as u32, a.use_auto_on_change as u32, a.is_forced as u32, a.clutch_sequence.is_done as u32]);
            let curve = &a.clutch_sequence.clutch_curve;
            out.push(curve.get_count() as u32);
            out.extend(curve.references().iter().map(|x| x.to_bits()));
            out.extend(curve.values().iter().map(|x| x.to_bits()));
            let blip = self.auto_blip.blip_start_time.to_bits();
            out.extend([blip as u32, (blip >> 32) as u32, self.auto_blip.is_active as u32]);
            let s = &self.auto_shifter;
            out.extend([s.is_active as u32, s.change_up_rpm as u32, s.change_dn_rpm as u32, s.gas_cutoff.to_bits()]);
            let g = &self.gear_changer;
            out.extend([g.was_gear_up_triggered, g.was_gear_dn_triggered, g.last_gear_up, g.last_gear_dn].map(|b| b as u32));
        }
        if let Some(aero) = &self.aero {
            aero.save_state(&mut out);
            out.extend([self.air_density.to_bits(), c.drs as u32]);
        }
        if let Some(aids) = &self.aids {
            aids.save_state(&mut out);
            out.extend(self.tyres.iter().map(|tyre| tyre.abs_override.to_bits()));
        }
        // the car-level glue
        out.extend([self.vibration_phase, self.slip_vibration_phase].map(f));
        out.extend([self.lights_on, self.last_ligth_switch_state, self.is_collision_off_for_pits, self.has_grid_position].map(|b| b as u32));
        let s = &self.slip_stream;
        for v in [self.grid_position, s.tip, s.dir, s.corners[0], s.corners[1]] {
            out.extend([v.x, v.y, v.z].map(f));
        }
        out.extend([s.length.to_bits(), self.mesh_collide_mask]);
        if let Some(writer) = &self.telemetry {
            out.extend([writer.packet_id as u32, writer.null_counts as u32, writer.current_tyres_out as u32]);
            out.extend([writer.snapshot_speed, writer.ride_height[0], writer.ride_height[1]].map(f));
        }
        out
    }

    /// The inverse of [`RollingChassis::save_state`].
    pub fn load_state(&mut self, words: &[u32]) -> Result<(), String> {
        let mut words = words.iter().copied();
        let mut next = || words.next().ok_or("the saved state is too short".to_string());
        macro_rules! f {
            () => {
                f32::from_bits(next()?)
            };
        }
        let bodies: Vec<_> = self.core.bodies().collect();
        for body in bodies {
            let pos = [f!(), f!(), f!()];
            let q = [f!(), f!(), f!(), f!()];
            let mut r = [0.0f32; 12];
            for value in &mut r {
                *value = f!();
            }
            let velocity = [f!(), f!(), f!(), f!(), f!(), f!()];
            let mut mass = Mass::zero();
            mass.mass = f!();
            mass.i[0] = f!();
            mass.i[5] = f!();
            mass.i[10] = f!();
            self.core.world.body_set_mass(body.id, &mass);
            let b = self.core.world.body_mut(body.id);
            b.pos[..3].copy_from_slice(&pos);
            b.q = q;
            b.r = r;
            b.lvel[..3].copy_from_slice(&velocity[..3]);
            b.avel[..3].copy_from_slice(&velocity[3..]);
            for k in 0..3 {
                b.facc[k] = 0.0;
                b.tacc[k] = 0.0;
            }
        }
        let ids: Vec<_> = self.core.world.joint_ids().collect();
        for id in ids {
            let mut v = [0.0f32; 9];
            for value in &mut v {
                *value = f!();
            }
            match &mut self.core.world.joint_mut(id).kind {
                JointKind::Fixed { qrel, offset, erp, cfm } => {
                    qrel.copy_from_slice(&v[0..4]);
                    offset[..3].copy_from_slice(&v[4..7]);
                    *erp = v[7];
                    *cfm = v[8];
                }
                JointKind::DBall { anchor1, anchor2, erp, cfm, target_distance } => {
                    anchor1[..3].copy_from_slice(&v[0..3]);
                    anchor2[..3].copy_from_slice(&v[3..6]);
                    *erp = v[6];
                    *cfm = v[7];
                    *target_distance = v[8];
                }
                _ => unreachable!("the chassis has rods and one fixed joint"),
            }
        }
        let count = rig::output_fields().len();
        for tyre in &mut self.tyres {
            let mut snapshot = Vec::with_capacity(count);
            for _ in 0..count {
                let low = next()? as u64;
                let high = next()? as u64;
                snapshot.push(low | high << 32);
            }
            rig::restore(tyre, &snapshot);
        }
        for suspension in &mut self.suspensions {
            let status = super::suspension::SuspensionStatus { travel: f!(), damper_speed_ms: f!() };
            suspension.restore_step_state(status, f!());
        }
        for heave in &mut self.heave_springs {
            heave.rod_length = f!();
            heave.travel = f!();
        }
        self.controls = CarControls { gas: f!(), brake: f!(), steer: f!(), clutch: f!(), ..CarControls::default() };
        self.final_steer_angle_signal = f!();
        self.acc_g = Vec3f::new(f!(), f!(), f!());
        self.last_velocity = Vec3f::new(f!(), f!(), f!());
        self.mz_current = f!();
        self.last_ff = f!();
        self.last_pure_mz_ff = f!();
        self.last_gyro_ff = f!();
        self.last_steer_position = f!();
        self.flat_spot_phase = f!();
        self.speed = f!();
        self.fuel_pressure = f!();
        self.sleeping_frames = next()? as i32;
        self.frames_to_sleep = next()? as i32;
        let mut double = || -> Result<f64, String> {
            let low = next()? as u64;
            let high = next()? as u64;
            Ok(f64::from_bits(low | high << 32))
        };
        self.fuel = double()?;
        self.last_body_mass_update_time = double()?;
        self.physics_time = double()?;
        for bar in &mut self.antiroll_bars {
            for stage in &mut bar.ctrl.stages {
                stage.current_value = f32::from_bits(words.next().ok_or("the saved state is too short")?);
            }
        }
        if let Some(brakes) = &mut self.brake_system {
            brakes.load_state(&mut words)?;
        }
        if let Some(drivetrain) = &mut self.drivetrain {
            drivetrain.load_state(&mut words)?;
            let mut next = || words.next().ok_or("the saved state is too short".to_string());
            self.controls.hand_brake = f32::from_bits(next()?);
            self.water.t = f32::from_bits(next()?);
            self.water.heat_accumulator = f32::from_bits(next()?);
            self.controls.gear_up = next()? != 0;
            self.controls.gear_dn = next()? != 0;
            self.controls.kers = next()? != 0;
            self.controls.requested_gear_index = next()? as i32;
            let a = &mut self.autoclutch;
            a.clutch_value_signal = f32::from_bits(next()?);
            a.clutch_sequence.current_time = f32::from_bits(next()?);
            a.use_auto_on_start = next()? != 0;
            a.use_auto_on_change = next()? != 0;
            a.is_forced = next()? != 0;
            a.clutch_sequence.is_done = next()? != 0;
            let count = next()? as usize;
            let mut numbers = Vec::with_capacity(2 * count);
            for _ in 0..2 * count {
                numbers.push(f32::from_bits(next()?));
            }
            let pairs: Vec<(f32, f32)> = (0..count).map(|k| (numbers[k], numbers[count + k])).collect();
            a.clutch_sequence.clutch_curve = crate::curve::Curve::from_pairs(&pairs);
            let low = next()? as u64;
            let high = next()? as u64;
            self.auto_blip.blip_start_time = f64::from_bits(low | high << 32);
            self.auto_blip.is_active = next()? != 0;
            let s = &mut self.auto_shifter;
            s.is_active = next()? != 0;
            s.change_up_rpm = next()? as i32;
            s.change_dn_rpm = next()? as i32;
            s.gas_cutoff = f32::from_bits(next()?);
            let g = &mut self.gear_changer;
            g.was_gear_up_triggered = next()? != 0;
            g.was_gear_dn_triggered = next()? != 0;
            g.last_gear_up = next()? != 0;
            g.last_gear_dn = next()? != 0;
        }
        if let Some(aero) = &mut self.aero {
            aero.load_state(&mut words)?;
            let mut next = || words.next().ok_or("the saved state is too short".to_string());
            self.air_density = f32::from_bits(next()?);
            self.controls.drs = next()? != 0;
        }
        if let Some(aids) = &mut self.aids {
            aids.load_state(&mut words)?;
            for tyre in &mut self.tyres {
                tyre.abs_override = f32::from_bits(words.next().ok_or("the saved state is too short".to_string())?);
            }
        }
        let mut next = || words.next().ok_or("the saved state is too short".to_string());
        self.vibration_phase = f32::from_bits(next()?);
        self.slip_vibration_phase = f32::from_bits(next()?);
        self.lights_on = next()? != 0;
        self.last_ligth_switch_state = next()? != 0;
        self.is_collision_off_for_pits = next()? != 0;
        self.has_grid_position = next()? != 0;
        let mut vectors = [Vec3f::default(); 5];
        for v in &mut vectors {
            *v = Vec3f::new(f32::from_bits(next()?), f32::from_bits(next()?), f32::from_bits(next()?));
        }
        self.grid_position = vectors[0];
        self.slip_stream.tip = vectors[1];
        self.slip_stream.dir = vectors[2];
        self.slip_stream.corners = [vectors[3], vectors[4]];
        self.slip_stream.length = f32::from_bits(next()?);
        self.mesh_collide_mask = next()?;
        if let Some(writer) = &mut self.telemetry {
            writer.packet_id = next()? as i32;
            writer.null_counts = next()? as i32;
            writer.current_tyres_out = next()? as i32;
            writer.snapshot_speed = f32::from_bits(next()?);
            writer.ride_height = [f32::from_bits(next()?), f32::from_bits(next()?)];
        }
        Ok(())
    }
}

/// Five damage levels as text, `front,rear,left,right,centre`.
pub fn parse_damage(text: &str) -> Result<[f32; 5], String> {
    let mut out = [0.0f32; 5];
    let parts: Vec<&str> = text.split(',').collect();
    if parts.len() != 5 {
        return Err(format!("damage: five numbers expected, got {text:?}"));
    }
    for (value, part) in out.iter_mut().zip(parts) {
        *value = part.trim().parse::<f32>().map_err(|e| format!("damage: {e}"))?;
    }
    Ok(out)
}

/// What the game produced in one step of a golden excerpt.
#[derive(Clone, Debug, PartialEq)]
pub struct GoldenStep {
    pub feed: RecordedStep,
    /// [`step_hash`] of the game's values.
    pub hash: u64,
    /// Position, quaternion, linear and angular velocity of the six bodies after the step
    /// (13 values each), so that a failure can say where the car is instead of only "hash".
    pub bodies: Vec<u32>,
}

/// A small excerpt of a recording for `cargo test`.
#[derive(Clone, Debug, PartialEq)]
pub struct Golden {
    /// The car's folder name in `cardata/`.
    pub car: String,
    pub setup: RunSetup,
    /// Index of the first step in the recording.
    pub first: usize,
    /// [`RollingChassis::save_state`] before the first step (empty when `first` is 0).
    pub state: Vec<u32>,
    pub steps: Vec<GoldenStep>,
}

const GOLDEN_MAGIC: &[u8; 8] = b"CHGOLD03";

/// The 13 values per body kept in a golden step.
pub fn body_words(chassis: &RollingChassis) -> Vec<u32> {
    let mut out = Vec::new();
    for body in chassis.core.bodies() {
        let b = chassis.core.world.body(body.id);
        out.extend([b.pos[0], b.pos[1], b.pos[2]].map(f32::to_bits));
        out.extend(b.q.map(f32::to_bits));
        out.extend([b.lvel[0], b.lvel[1], b.lvel[2], b.avel[0], b.avel[1], b.avel[2]].map(f32::to_bits));
    }
    out
}

impl Golden {
    pub fn to_bytes(&self) -> Vec<u8> {
        let e = &self.setup.env;
        let header = format!(
            "car={}\nscenario={}\nground={}\nseed={}\nclock_start_ms={:?}\nfirst={}\nsteps={}\nambient_temperature={:?}\n\
             road_temperature={:?}\ndynamic_grip_level={:?}\ntyre_consumption_rate={:?}\nmechanical_damage_rate={:?}\n\
             fuel_consumption_rate={:?}\nallow_tyre_blankets={}\nflat_spot_ff_gain={:?}\ngyro_wheel_gain={:?}\n\
             mz_low_speed_reduction_speed_kmh={:?}\nmz_low_speed_reduction_min_value={:?}\nff_filter={:?}\n\
             use_fake_understeer_ff={}\nis_first_car={}\nrust_brakes={}\nrust_drivetrain={}\nauto_clutch={}\n\
             auto_shifter={}\nrust_aero={}\nrust_aids={}\ntelemetry={}\npitlane={}\nstability_gain={:?}\nwind_speed={:?}\n\
             wind_direction_deg={:?}\ndamage={}\n",
            self.car,
            self.setup.scenario,
            self.setup.ground.describe(),
            self.setup.seed,
            self.setup.clock_start_ms,
            self.first,
            self.steps.len(),
            e.ambient_temperature,
            e.road_temperature,
            e.dynamic_grip_level,
            e.tyre_consumption_rate,
            e.mechanical_damage_rate,
            e.fuel_consumption_rate,
            e.allow_tyre_blankets as u8,
            e.flat_spot_ff_gain,
            e.gyro_wheel_gain,
            e.mz_low_speed_reduction_speed_kmh,
            e.mz_low_speed_reduction_min_value,
            e.ff_filter,
            e.use_fake_understeer_ff as u8,
            e.is_first_car as u8,
            self.setup.rust_brakes as u8,
            self.setup.rust_drivetrain as u8,
            self.setup.auto_clutch as u8,
            self.setup.auto_shifter as u8,
            self.setup.rust_aero as u8,
            self.setup.rust_aids as u8,
            self.setup.telemetry as u8,
            self.setup.pitlane as u8,
            self.setup.stability_gain,
            self.setup.wind_speed,
            self.setup.wind_direction_deg,
            self.setup.damage.map(|d| format!("{d:?}")).join(","),
        );
        let mut words: Vec<u32> = Vec::new();
        words.push(self.state.len() as u32);
        words.extend(&self.state);
        for step in &self.steps {
            step.feed.to_words(&mut words);
            words.push(step.hash as u32);
            words.push((step.hash >> 32) as u32);
            words.extend(&step.bodies);
        }
        let mut out = Vec::with_capacity(16 + header.len() + words.len() * 4);
        out.extend_from_slice(GOLDEN_MAGIC);
        out.extend_from_slice(&(header.len() as u32).to_le_bytes());
        out.extend_from_slice(header.as_bytes());
        for word in words {
            out.extend_from_slice(&word.to_le_bytes());
        }
        out
    }

    pub fn parse(bytes: &[u8]) -> Result<Golden, String> {
        if bytes.len() < 12 || &bytes[..8] != GOLDEN_MAGIC {
            return Err("not a chassis golden file".to_string());
        }
        let header_len = u32::from_le_bytes(bytes[8..12].try_into().unwrap()) as usize;
        let header = std::str::from_utf8(bytes.get(12..12 + header_len).ok_or("truncated header")?)
            .map_err(|e| e.to_string())?;
        let get = |key: &str| -> Result<&str, String> {
            header
                .lines()
                .find_map(|line| line.split_once('=').filter(|(k, _)| *k == key).map(|(_, v)| v))
                .ok_or(format!("the golden file has no {key}"))
        };
        let number = |key: &str| -> Result<f32, String> { get(key)?.parse::<f32>().map_err(|e| format!("{key}: {e}")) };
        let env = ChassisEnvironment {
            ambient_temperature: number("ambient_temperature")?,
            road_temperature: number("road_temperature")?,
            dynamic_grip_level: number("dynamic_grip_level")?,
            tyre_consumption_rate: number("tyre_consumption_rate")?,
            mechanical_damage_rate: number("mechanical_damage_rate")?,
            fuel_consumption_rate: number("fuel_consumption_rate")?,
            allow_tyre_blankets: get("allow_tyre_blankets")? != "0",
            flat_spot_ff_gain: number("flat_spot_ff_gain")?,
            gyro_wheel_gain: number("gyro_wheel_gain")?,
            mz_low_speed_reduction_speed_kmh: number("mz_low_speed_reduction_speed_kmh")?,
            mz_low_speed_reduction_min_value: number("mz_low_speed_reduction_min_value")?,
            ff_filter: number("ff_filter")?,
            use_fake_understeer_ff: get("use_fake_understeer_ff")? != "0",
            is_first_car: get("is_first_car")? != "0",
            ..ChassisEnvironment::default()
        };
        let setup = RunSetup {
            scenario: get("scenario")?.to_string(),
            ground: Ground::parse(get("ground")?).ok_or("bad ground")?,
            seed: get("seed")?.parse().map_err(|e| format!("seed: {e}"))?,
            clock_start_ms: get("clock_start_ms")?.parse().map_err(|e| format!("clock_start_ms: {e}"))?,
            env,
            rust_brakes: get("rust_brakes")? != "0",
            rust_drivetrain: get("rust_drivetrain")? != "0",
            rust_aero: get("rust_aero")? != "0",
            rust_aids: get("rust_aids")? != "0",
            telemetry: get("telemetry")? != "0",
            pitlane: get("pitlane")? != "0",
            stability_gain: number("stability_gain")?,
            wind_speed: number("wind_speed")?,
            wind_direction_deg: number("wind_direction_deg")?,
            damage: parse_damage(get("damage")?)?,
            auto_clutch: get("auto_clutch")? != "0",
            auto_shifter: get("auto_shifter")? != "0",
        };
        let first: usize = get("first")?.parse().map_err(|e| format!("first: {e}"))?;
        let count: usize = get("steps")?.parse().map_err(|e| format!("steps: {e}"))?;
        let mut words = bytes[12 + header_len..].chunks_exact(4).map(|c| u32::from_le_bytes(c.try_into().unwrap()));
        let state_len = words.next().ok_or("truncated")? as usize;
        let state: Vec<u32> = words.by_ref().take(state_len).collect();
        if state.len() != state_len {
            return Err("truncated state".to_string());
        }
        let mut steps = Vec::with_capacity(count);
        for index in 0..count {
            let feed = RecordedStep::from_words(&mut words).ok_or(format!("truncated at step {index}"))?;
            let low = words.next().ok_or("truncated")? as u64;
            let high = words.next().ok_or("truncated")? as u64;
            let bodies: Vec<u32> = words.by_ref().take(BODIES.len() * 13).collect();
            if bodies.len() != BODIES.len() * 13 {
                return Err(format!("truncated at step {index}"));
            }
            steps.push(GoldenStep { feed, hash: low | high << 32, bodies });
        }
        Ok(Golden { car: get("car")?.to_string(), setup, first, state, steps })
    }

    /// Builds the chassis from `data_path`, puts it into the excerpt's start state and runs
    /// every step, comparing with what the game produced. `Err` names the first step that
    /// differs.
    pub fn check(&self, data_path: &Path) -> Result<(), String> {
        let mut chassis = self.setup.build_runner(data_path)?;
        if !self.state.is_empty() {
            // the setup values reached the car in the first step of the recording
            let mut manager = std::mem::take(&mut chassis.setup_manager);
            manager.step(&mut chassis);
            chassis.setup_manager = manager;
            chassis.load_state(&self.state)?;
        }
        let kinds: Vec<char> = fields().iter().map(|field| field.kind).collect();
        for (index, step) in self.steps.iter().enumerate() {
            let number = self.first + index;
            chassis.step_recorded(self.setup.time_of_step(number), &step.feed);
            let bodies = body_words(&chassis);
            if bodies != step.bodies {
                let at = bodies.iter().zip(&step.bodies).position(|(a, b)| a != b).unwrap();
                let part = ["pos.x", "pos.y", "pos.z", "q.w", "q.x", "q.y", "q.z", "lvel.x", "lvel.y", "lvel.z", "avel.x", "avel.y", "avel.z"];
                return Err(format!(
                    "{} step {number}: {}.{} is {:?}, the game has {:?}",
                    self.setup.scenario,
                    BODIES[at / 13],
                    part[at % 13],
                    f32::from_bits(bodies[at]),
                    f32::from_bits(step.bodies[at])
                ));
            }
            // the chassis values, then those of the ported systems every recording holds
            let mut all_kinds = kinds.clone();
            let mut words = snapshot(&chassis);
            for value in powertrain_trace(&chassis).iter().filter(|value| !value.extra) {
                all_kinds.push(value.kind);
                words.push(value.word);
            }
            let hash = step_hash(&all_kinds, &words, chassis.core.tape.as_deref().unwrap_or(&[]));
            if hash != step.hash {
                return Err(format!(
                    "{} step {number}: the bodies agree with the game, but another value (suspension, tyre, joint, \
                     force call, steering, force feedback, brakes, engine, drivetrain) does not: hash {hash:#018x}, \
                     the game's {:#018x}",
                    self.setup.scenario, step.hash
                ));
            }
        }
        Ok(())
    }
}

/// A tape call's kind as text.
pub fn kind_name(call_kind: u32) -> &'static str {
    match call_kind {
        kind::ADD_FORCE_AT_POS => "addForceAtPos",
        kind::ADD_FORCE_AT_LOCAL_POS => "addForceAtLocalPos",
        kind::ADD_LOCAL_FORCE => "addLocalForce",
        kind::ADD_LOCAL_FORCE_AT_POS => "addLocalForceAtPos",
        kind::ADD_LOCAL_FORCE_AT_LOCAL_POS => "addLocalForceAtLocalPos",
        kind::ADD_TORQUE => "addTorque",
        kind::ADD_LOCAL_TORQUE => "addLocalTorque",
        kind::STOP => "stop",
        _ => "?",
    }
}
