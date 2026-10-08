// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! The car, as far as it is ported: a **rolling chassis**.
//!
//! * [`body`]: AC's layer over the rigid-body library (`PhysicsCore`, `RigidBodyODE`) on
//!   `rustyac-ode`, with a force tape.
//! * [`suspension`]: the suspension slot [`SuspensionModel`] (AC's `ISuspension`) and its
//!   double-wishbone implementation [`VanillaDwb`], the [`Damper`].
//! * [`heave_spring`], [`antiroll_bar`]: the third spring and the bars of an axle.
//! * [`setup`]: the setup items that write chassis values, and the round trip through the
//!   game's setup screen at a session start.
//! * [`chassis`]: [`RollingChassis`], AC's `Car` reduced to body, fuel tank, hubs, masses,
//!   fuel, suspensions with their tyres, steering and force feedback, stepped in the order of
//!   `Car::step`, with slots for the brakes and the drivetrain.
//! * [`dynamic_controller`]: [`DynamicController`], the look-up-table formula of the
//!   `ctrl_*.ini` files.
//! * [`brakes`]: the brake slot [`BrakeModel`] and [`VanillaBrakes`] (AC's `BrakeSystem`).
//! * [`engine`]: the engine slot [`EngineModel`] and [`VanillaEngine`] (AC's `Engine`, turbos).
//! * [`drivetrain`]: the drivetrain slot [`DrivetrainModel`] and [`VanillaDrivetrain`] (AC's
//!   `Drivetrain` for two driven wheels: clutch, gearbox, differential).
//! * [`shift_assists`]: automatic clutch, throttle blip, automatic gearbox, gear changer.
//! * [`feed`]: [`ChassisFeed`], where the systems that are not ported yet (driver, aero,
//!   aids, hybrids) hand their outputs to the car.
//! * [`replay`]: the test rig: a feed filled from a `tools/car_oracle` recording, the road of
//!   those recordings, the list of values compared with the game, the golden-file format.

// A transcription: sums are written in the operand order of the machine code, the float
// comparisons are the machine code's own (they decide what a NaN does), and members are set in
// the order of the game's constructors and loaders.
#![allow(
    clippy::field_reassign_with_default,
    clippy::manual_range_contains,
    clippy::unnecessary_unwrap,
    clippy::neg_cmp_op_on_partial_ord,
    clippy::assign_op_pattern,
    clippy::double_comparisons,
    clippy::nonminimal_bool,
    clippy::neg_multiply,
    clippy::identity_op,
    clippy::too_many_arguments
)]

pub mod aero;
pub mod aids;
pub mod antiroll_bar;
pub mod body;
pub mod brakes;
pub mod chassis;
pub mod colliders;
pub mod drivetrain;
pub mod dynamic_controller;
pub mod engine;
pub mod ers;
pub mod feed;
pub mod heave_spring;
pub mod kers;
pub mod replay;
pub mod setup;
pub mod shift_assists;
pub mod suspension;
pub mod suspension_axle;
pub mod suspension_ml;
pub mod suspension_strut;
pub mod telemetry;
pub mod vanilla_car;

pub use aero::{AeroBase, AeroModel, Drs, DynamicWingController, SlipStream, VanillaAero, Wing};
pub use aids::{Abs, AidsBase, AidsModel, Edl, SpeedLimiter, StabilityControl, TractionControl, VanillaAids};
pub use antiroll_bar::AntirollBar;
pub use body::{CollisionEvent, ContactRecord, DistanceJoint, FixedJoint, ForceSource, PhysicsCore, RigidBody, Shape, TapeCall};
pub use colliders::{BoxColliderDef, CarBounds, CarColliders, ColliderMesh};
pub use brakes::{BrakeBase, BrakeModel, VanillaBrakes};
pub use chassis::{ChassisEnvironment, CockpitState, PenaltyManager, RollingChassis, SteeringSystem, StepTrace, ThermalObject};
pub use drivetrain::{DrivetrainBase, DrivetrainModel, TractionType, VanillaDrivetrain};
pub use dynamic_controller::{CarSignals, DynamicController};
pub use engine::{EngineBase, EngineModel, VanillaEngine};
pub use ers::{Ers, ErsPowerController};
pub use feed::{CarControls, ChassisFeed, EngineFeed, VibrationDef};
pub use heave_spring::HeaveSpring;
pub use kers::{Kers, KersAttachment};
pub use setup::{SetupItem, SetupManager};
pub use shift_assists::{AutoBlip, AutoShifter, Autoclutch, GearChanger};
pub use telemetry::{PhysicsPage, PhysicsPageWriter};
pub use vanilla_car::{CarControlsInput, ControlsProvider, ScriptedDevice, VanillaCar};
pub use suspension::{Damper, SuspensionBase, SuspensionModel, SuspensionStatus, SuspensionType, VanillaDwb};
pub use suspension_axle::VanillaAxle;
pub use suspension_ml::VanillaMultilink;
pub use suspension_strut::VanillaStrut;
