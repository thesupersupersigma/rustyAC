// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! An online session in the physics engine: the client and the other players' cars as the
//! engine's step-completed handlers ([`rustyac_physics::engine::StepHooks`]), in the order
//! the game registers them:
//!
//! ```text
//! PhysicsEngine::step
//!   ... the local car's step, the collision pass, dWorldStep ...
//!   ACClient::onPhysicsStep          NetSession::before_cars   the car update goes out, what
//!                                                              arrived is taken in: each
//!                                                              state to its car
//!   (the track's loose objects)
//!   Car::postStep
//!   NetCarStateProvider::step ...    NetSession::after_cars    every remote car's body is
//!                                                              put where the car is now
//! ```
//!
//! The game ([`rustyac-game`]) and the oracle (`tools/car_oracle net`, which runs the game's
//! own `ACClient` and `NetCarStateProvider` beside this) both step this.

// Comparisons are spelled the way the original branches so NaN takes the same path.
#![allow(clippy::neg_cmp_op_on_partial_ord, clippy::double_comparisons, clippy::manual_clamp, clippy::needless_range_loop)]

use std::sync::Arc;

use rustyac_physics::car::{PhysicsCore, RollingChassis};
use rustyac_physics::engine::{CarSlot, StepHooks};
use rustyac_physics::track::{Track, TrackGround};
use rustyac_physics::vecmath::Vec3f;

use crate::car_update::LocalCarState;
use crate::client::{Client, CollisionReport, Event};
use crate::net_car::{NetCarStateProvider, StepWorld};
use crate::transport::Transport;

/// `Car::getFrontWheelAngle` @ 0x140270990: the mean of the two front hubs' turn about the
/// body's vertical, radians.
#[allow(clippy::neg_cmp_op_on_partial_ord)]
pub fn front_wheel_angle(car: &RollingChassis) -> f32 {
    if car.suspensions.len() < 2 {
        return 0.0;
    }
    let turn = |wheel: usize| -> (f32, f32) {
        let hub = car.suspensions[wheel].get_hub_world_matrix(&car.core);
        let forward = Vec3f::new(-hub.m[2][0], -hub.m[2][1], -hub.m[2][2]);
        let local = car.core.world_to_local_normal(car.body, &forward);
        let (mut x, mut z) = (local.x, local.z);
        let length = rustyac_math::sqrtf(x * x + z * z);
        if length < 0.0 || length > 0.0 {
            let inv = 1.0f32 / length;
            x *= inv;
            z *= inv;
        }
        (x, z)
    };
    let angle = |x: f32, z: f32| -> f32 {
        let cosine = -z;
        let angle = if cosine > -1.0 && cosine < 1.0 { rustyac_math::acosf(cosine) } else { 0.0 };
        let sign = if x > 0.0 {
            1.0f32
        } else if x == 0.0 {
            0.0
        } else {
            -1.0
        };
        sign * angle
    };
    let (x0, z0) = turn(0);
    let (x1, z1) = turn(1);
    let first = angle(x0, z0);
    (angle(x1, z1) + first) * 0.5
}

/// What `ACClient::sendCarPosition` reads of the local car. `car`'s bodies are as
/// `dWorldStep` left them; nothing of its `Car::postStep` has run.
pub fn local_car_state(car: &RollingChassis, horn: bool) -> LocalCarState {
    let velocity = car.core.get_velocity(car.body);
    let mut state = LocalCarState {
        body_matrix: car.core.get_world_matrix(car.body),
        velocity: [velocity.x, velocity.y, velocity.z],
        speed: car.speed,
        steer: car.controls.steer,
        steer_lock: car.steer_lock,
        front_wheel_angle: front_wheel_angle(car),
        gas: car.controls.gas,
        brake: car.controls.brake,
        lights_on: car.lights_on,
        horn,
        penalty_time: car.get_penalty_time(),
        ers_is_charging: car.ers.as_ref().is_some_and(|ers| ers.is_charging),
        drs_open: car.aero.as_ref().is_some_and(|aero| aero.base().drs.is_present && aero.base().drs.is_active),
        performance_split: car.performance_split().0,
        normalized_position: car.spline_locator_data.npos,
        ..LocalCarState::default()
    };
    for (wheel, tyre) in car.tyres.iter().take(4).enumerate() {
        state.wheel_angular_velocity[wheel] = tyre.status.angular_velocity;
        state.wheel_slip_ratio[wheel] = tyre.status.slip_ratio;
    }
    if let Some(drivetrain) = &car.drivetrain {
        state.engine_rpm = drivetrain.get_engine_rpm();
        state.current_gear = drivetrain.base().current_gear;
    }
    state
}

/// The client of a server and the cars of the other players.
pub struct NetSession<T: Transport> {
    pub client: Client<T>,
    /// `ACClient::netCars`: the other players' cars in the order they were made.
    pub cars: Vec<NetCarStateProvider>,
    /// The track the remote cars' wheels are put on (`rayCastProvider`).
    pub track: Option<Arc<Track>>,
    /// The driver's horn (sent with the car update).
    pub horn: bool,
    /// What the client heard, in arrival order, until somebody takes it.
    pub events: Vec<Event>,
    /// Steps so far (the clock of a tape).
    pub steps: u64,
}

impl<T: Transport> NetSession<T> {
    pub fn new(client: Client<T>) -> NetSession<T> {
        NetSession { client, cars: Vec::new(), track: None, horn: false, events: Vec::new(), steps: 0 }
    }

    /// The frame's work of every remote car (`NetCarStateProvider::update`): the game calls
    /// it once a frame with its clock; here once a step with the physics clock does the same.
    pub fn update_cars(&mut self, now: f64) {
        for car in &mut self.cars {
            car.update(now);
        }
    }

    /// The car in a slot.
    pub fn car_of(&mut self, slot: u8) -> Option<&mut NetCarStateProvider> {
        self.cars.iter_mut().find(|car| car.session_id == slot)
    }
}

impl<T: Transport> StepHooks for NetSession<T> {
    /// `ACClient::onPhysicsStep`: the client was the first to hang a handler on the engine.
    fn before_cars(&mut self, _core: &mut PhysicsCore, cars: &mut [CarSlot<'_>], physics_time: f64) {
        self.client.transport.set_tick(self.steps);
        self.steps += 1;
        let Some(slot) = cars.first() else {
            return;
        };
        let car = &*slot.car;
        // the handler ACClient hangs on the local car's collision event, which the game calls in
        // the collision pass, before this step's report can go out: the remote car that
        // was hit keeps part of the solver's velocity for a second, and every hit goes into
        // the next report (a hit of anything that is no remote car as a hit of the
        // surroundings)
        for event in &car.collision_events {
            let other = event.body.and_then(|body| self.cars.iter_mut().find(|remote| remote.body == body));
            let other_slot = match other {
                Some(remote) => {
                    remote.on_local_collision();
                    Some(remote.session_id)
                }
                None => None,
            };
            self.client.add_collision(CollisionReport {
                other_slot,
                speed: event.rel_speed,
                world_pos: [event.world_pos.x, event.world_pos.y, event.world_pos.z],
                rel_pos: [event.rel_pos.x, event.rel_pos.y, event.rel_pos.z],
            });
        }
        let horn = self.horn;
        // the wall clock of the pong and of the frame's work is the physics clock here: the
        // two run alike, and a session then repeats exactly from a tape
        self.client.on_physics_step(physics_time, physics_time, &mut || local_car_state(car, horn));
        self.client.update(physics_time);
        let send_interval = self.client.send_interval;
        for (slot, state) in self.client.take_car_states() {
            // (the first car with that slot; the player's own slot has none)
            if let Some(remote) = self.cars.iter_mut().find(|remote| remote.session_id == slot) {
                remote.on_remote_state_received(state, physics_time, send_interval);
            }
        }
        for event in self.client.take_events() {
            // ACP_CLIENT_DISCONNECTED: the car is gone at once, on this thread
            if let Event::Message(crate::messages::Message::ClientDisconnected { slot }) = &event {
                if let Some(remote) = self.cars.iter_mut().find(|remote| remote.session_id == *slot) {
                    remote.mark_disconnected();
                }
            }
            self.events.push(event);
        }
    }

    /// `NetCarStateProvider::step` of every remote car, in the order they were made.
    fn after_cars(&mut self, core: &mut PhysicsCore, cars: &mut [CarSlot<'_>], physics_time: f64) {
        let Some(slot) = cars.first() else {
            return;
        };
        let car = &*slot.car;
        let Some(track) = self.track.clone() else {
            return;
        };
        let ground = TrackGround(track);
        let local_in_pit_lane = car.is_in_pit_lane();
        let local_body = Some(car.body);
        let remote_factor = self.client.remote_factor;
        for remote in &mut self.cars {
            let mut world = StepWorld { core, ground: &ground, remote_factor, local_in_pit_lane, local_body };
            remote.step(physics_time, &mut world);
        }
    }
}
