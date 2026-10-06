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
//!   `Car::step`.
//! * [`feed`]: [`ChassisFeed`], where the systems that are not ported yet (driver, engine,
//!   drivetrain, brakes, aero, aids) hand their outputs to the chassis.
//! * [`replay`]: the test rig: a feed filled from a `tools/car_oracle` recording, the road of
//!   those recordings, the list of values compared with the game, the golden-file format.

// A transcription: sums are written in the operand order of the machine code, and the float
// comparisons are the machine code's own (they decide what a NaN does).
#![allow(
    clippy::neg_cmp_op_on_partial_ord,
    clippy::assign_op_pattern,
    clippy::double_comparisons,
    clippy::nonminimal_bool,
    clippy::neg_multiply,
    clippy::identity_op,
    clippy::too_many_arguments
)]

pub mod antiroll_bar;
pub mod body;
pub mod chassis;
pub mod feed;
pub mod heave_spring;
pub mod replay;
pub mod setup;
pub mod suspension;

pub use antiroll_bar::AntirollBar;
pub use body::{DistanceJoint, FixedJoint, ForceSource, PhysicsCore, RigidBody, TapeCall};
pub use chassis::{ChassisEnvironment, RollingChassis, SteeringSystem, StepTrace};
pub use feed::{CarControls, ChassisFeed, EngineFeed};
pub use heave_spring::HeaveSpring;
pub use setup::{SetupItem, SetupManager};
pub use suspension::{Damper, SuspensionBase, SuspensionModel, SuspensionStatus, VanillaDwb};
