// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! [`PhysicsEngine`]: several cars in one world, stepped as AC's `PhysicsEngine::step`
//! @ 0x140264760 steps them.
//!
//! ```text
//! PhysicsEngine::step
//!   Track::step (the grip, from the laps of all cars), stepWind
//!   Car::stepPreCacheValues   every car, in creation order
//!   Car::step                 every car: the first `cars / workers + 1` on the calling
//!                             thread, each of the others as one task of the thread pool;
//!                             the calling thread then waits for every task
//!   PhysicsCore::step         one collision pass and one dWorldStep for all cars
//!   evOnStepCompleted         the loose objects' handlers, then Car::postStep per car
//! ```
//!
//! # One world, and a working copy per car
//!
//! All cars' bodies, joints and shapes are in the engine's one [`PhysicsCore`], made in the
//! game's order (the track's loose objects, then car after car). `Car::step` only ever
//! touches its own car's part of that world, which is what lets the game run the cars on
//! several threads. Here every car keeps a **working copy** of the world
//! ([`RollingChassis::core`]) in which its own bodies, joints and shapes are current. A step
//! runs every car on its copy, writes each car's part back into the one world (in creation
//! order), runs the collision pass and `dWorldStep` there, and hands every car its bodies'
//! new state. The copies are plain memory copies, so the numbers are the game's; and since a
//! car's step reads nothing another car's step writes, the result is the same bit for bit
//! with any number of threads, and in any order of the tasks.
//!
//! What a car reads of the others during its step is handed to it before the cars start:
//! the other cars' positions (the pit-lane rule) and wakes (the slipstream), the wind and the
//! track's grip. In the game these are live reads of values no `Car::step` writes.

#[cfg(not(target_arch = "wasm32"))]
pub mod pool;
pub mod record;

use std::sync::Arc;

use crate::car::chassis::{turn_to_car, CollisionInputs};
use crate::car::vanilla_car::DeviceFeed;
use crate::car::{ChassisEnvironment, ControlsProvider, PhysicsCore, RigidBody, RollingChassis, SlipStream};
use crate::ode::{BodyId, GeomId, JointId};
use crate::track::{DynamicTrack, Track, TrackObjectDef};
use crate::vecmath::Vec3f;

/// The driver's device of a car in a step.
pub enum DeviceRef<'a> {
    /// A device that must stay on the thread that calls [`PhysicsEngine::step`] (a real
    /// wheel, the keyboard): its car is stepped there.
    Here(&'a mut dyn ControlsProvider),
    /// A device any thread may ask (a recording, a script, an AI driver).
    Anywhere(&'a mut (dyn ControlsProvider + Send)),
}

impl DeviceRef<'_> {
    fn get(&mut self) -> &mut dyn ControlsProvider {
        match self {
            DeviceRef::Here(device) => *device,
            DeviceRef::Anywhere(device) => *device,
        }
    }
}

/// One car handed to [`PhysicsEngine::step`], in creation order.
pub struct CarSlot<'a> {
    pub car: &'a mut RollingChassis,
    pub device: DeviceRef<'a>,
}

/// What of the one world is a car's.
#[derive(Clone, Debug, Default)]
struct CarParts {
    body: Option<RigidBody>,
    bodies: Vec<BodyId>,
    joints: Vec<JointId>,
    geoms: Vec<GeomId>,
}

/// `[PHYSICS_THREADING] THREADS` of `system/cfg/assetto_corsa.ini` as
/// `PhysicsEngine::PhysicsEngine` @ 0x140262430 turns it into a number of worker threads
/// for `logical_cpus` processors (`ksGetNumberOfProcessors`): none below 4 processors or
/// with `THREADS=0`; half the processors with -1 (the game's file: "automatic"); else the
/// number as it is. (A file without the section reads as -1, a section without the key as
/// 0. The game itself cannot run with `THREADS=1`: its split `cars / workers + 1` then asks
/// for a negative number of tasks and throws; here one worker works.)
pub fn worker_threads(threads_key: i32, logical_cpus: usize) -> usize {
    if logical_cpus < 4 || threads_key == 0 {
        0
    } else if threads_key == -1 {
        logical_cpus / 2
    } else {
        threads_key.max(0) as usize
    }
}

/// AC's `PhysicsEngine` as far as several cars need it: the one world and what is stepped
/// once for all cars. The cars themselves belong to the caller.
pub struct PhysicsEngine {
    /// `PhysicsEngine::core`: the one world.
    pub core: PhysicsCore,
    /// The engine's settings and its wind; every car is built with a copy
    /// ([`PhysicsEngine::car_env`]) and gets the wind of each step.
    pub env: ChassisEnvironment,
    /// `PhysicsEngine::track`
    pub track: Option<Arc<Track>>,
    /// `Track::dynamicTrack`: the grip of the session, when race.ini has the section.
    pub dynamic_track: Option<DynamicTrack>,
    /// False: every body is a ghost (no collision pass at all).
    pub collisions_enabled: bool,
    /// `PhysicsEngine::stepCounter`
    pub step_counter: u32,
    /// What the last `dWorldStep` looked like.
    pub step_stats: rustyac_ode::StepStats,
    /// How often the collision callback ran in the last step (= new contact joints).
    pub contact_callbacks: u32,
    /// The collision callbacks of the last step, as the cars were told (for the oracles).
    pub last_events: Vec<crate::car::CollisionEvent>,
    parts: Vec<CarParts>,
    /// The core is out, in a car that is being built.
    building: bool,
    #[cfg(not(target_arch = "wasm32"))]
    pool: Option<pool::ThreadPool>,
}

/// The slots of a step, shared with the pool's workers. Each index is taken by exactly one
/// thread ([`pool::ThreadPool::run`] hands every item out once), and only slots whose device
/// is [`DeviceRef::Anywhere`] are touched by a worker.
#[cfg(not(target_arch = "wasm32"))]
struct SharedSlots<'a, 'b>(*mut CarSlot<'b>, std::marker::PhantomData<&'a mut [CarSlot<'b>]>);

// SAFETY: see the type. A `RollingChassis` is `Send` (checked below), and so is the device
// of the slots a worker steps.
#[cfg(not(target_arch = "wasm32"))]
unsafe impl Sync for SharedSlots<'_, '_> {}

#[allow(dead_code)]
fn chassis_is_send() {
    fn is_send<T: Send>() {}
    is_send::<RollingChassis>();
}

fn step_one(slot: &mut CarSlot<'_>, dt: f32, physics_time: f64) {
    let mut feed = DeviceFeed { device: slot.device.get(), dt };
    slot.car.step_car(dt, physics_time, &mut feed);
}

impl PhysicsEngine {
    /// `PhysicsEngine::PhysicsEngine` @ 0x140262430 with `threads` worker threads
    /// ([`worker_threads`]; 0: every car on the calling thread).
    pub fn new(env: ChassisEnvironment, threads: usize) -> PhysicsEngine {
        #[cfg(target_arch = "wasm32")]
        let _ = threads;
        PhysicsEngine {
            core: PhysicsCore::new(),
            env,
            track: None,
            dynamic_track: None,
            collisions_enabled: true,
            step_counter: 0,
            step_stats: rustyac_ode::StepStats::default(),
            contact_callbacks: 0,
            last_events: Vec::new(),
            parts: Vec::new(),
            building: false,
            #[cfg(not(target_arch = "wasm32"))]
            pool: (threads > 0).then(|| pool::ThreadPool::new(threads)),
        }
    }

    /// The number of worker threads.
    pub fn threads(&self) -> usize {
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.pool.as_ref().map_or(0, |pool| pool.threads())
        }
        #[cfg(target_arch = "wasm32")]
        {
            0
        }
    }

    /// Another number of worker threads (the result of a step does not depend on it).
    pub fn set_threads(&mut self, threads: usize) {
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.pool = (threads > 0).then(|| pool::ThreadPool::new(threads));
        }
        #[cfg(target_arch = "wasm32")]
        let _ = threads;
    }

    /// The number of cars built so far: the next car's `physicsGUID`.
    pub fn car_count(&self) -> usize {
        self.parts.len()
    }

    /// The track, and with it its loose objects when `with_objects` (the game makes them
    /// while it loads the track, before any car). Call it before the first car is built.
    pub fn set_track(&mut self, track: Arc<Track>, with_objects: bool) {
        assert!(self.parts.is_empty(), "the track comes before the cars");
        if with_objects {
            self.create_track_objects(&track.objects);
        }
        self.track = Some(track);
    }

    /// `PhysicsObject::PhysicsObject` for each loose object, before any car.
    pub fn create_track_objects(&mut self, objects: &[TrackObjectDef]) {
        assert!(self.parts.is_empty(), "the track's objects come before the cars");
        for object in objects {
            self.core.create_track_object(&object.matrix, object.vertices.clone(), object.indices.clone());
        }
    }

    /// The settings a new car is built with: the engine's, and whether it is the first car.
    pub fn car_env(&self) -> ChassisEnvironment {
        ChassisEnvironment { is_first_car: self.parts.is_empty(), ..self.env }
    }

    /// Builds the next car into the one world. `build` gets the world and the car's
    /// `physicsGUID` and must do everything that makes bodies, joints or shapes:
    /// [`RollingChassis::new_in_core`] (use [`PhysicsEngine::car_env`]), the systems
    /// (`install_*`) and the collider mesh ([`RollingChassis::load_collider_mesh`] /
    /// [`RollingChassis::init_collider_mesh`]). What only moves or tunes the car (the spawn,
    /// the session start, a setup, damage) can be done on the returned car at any time: it
    /// reaches the one world with the next step.
    ///
    /// An error leaves the engine without its world: it cannot be used any more.
    pub fn build_car(&mut self, build: impl FnOnce(PhysicsCore, u32) -> Result<RollingChassis, String>) -> Result<RollingChassis, String> {
        assert!(!self.building, "a car that failed to build has taken the engine's world with it");
        let guid = self.parts.len() as u32;
        let core = std::mem::take(&mut self.core);
        let bodies_before = core.bodies().count();
        let (_, joints_before) = core.world.slot_counts();
        let geoms_before = core.world.collision.geoms.len();
        self.building = true;
        let mut car = build(core, guid)?;
        self.building = false;
        assert_eq!(car.physics_guid, guid, "the car was not built with the GUID it was given");
        // the world comes back; the car keeps a copy to work on
        self.core = car.core.clone();
        let parts = CarParts {
            body: Some(car.body),
            bodies: self.core.bodies().skip(bodies_before).map(|b| b.id).collect(),
            joints: (joints_before..self.core.world.slot_counts().1).map(JointId).collect(),
            geoms: (geoms_before..self.core.world.collision.geoms.len()).map(|g| GeomId(g as u32)).collect(),
        };
        self.parts.push(parts);
        // the car's own tape and callbacks are its own business
        car.core.contact_log = None;
        Ok(car)
    }

    /// `PhysicsEngine::setSessionInfo` @ 0x140264560, the part about collisions: the contacts
    /// are thrown away and no new ones are looked for during 250 steps (0.75 s).
    pub fn reset_collisions_for_new_session(&mut self) {
        self.core.reset_collisions();
        self.core.set_no_collision_steps(250);
    }

    /// `PhysicsEngine::setWind` @ 0x1402645a0.
    pub fn set_wind(&mut self, speed: f32, direction_deg: f32) {
        self.env.set_wind(speed, direction_deg);
    }

    /// The car bodies, in creation order.
    pub fn car_bodies(&self) -> Vec<RigidBody> {
        self.parts.iter().filter_map(|p| p.body).collect()
    }

    /// The bodies of car `index` in the one world, in creation order.
    pub fn bodies_of(&self, index: usize) -> &[BodyId] {
        &self.parts[index].bodies
    }

    /// Writes one car's part of its working copy into the one world: the state of its
    /// bodies, its joints, the bits of its shapes. A body that was put somewhere else tells
    /// its shapes, as `dBodySetPosition` does.
    fn write_back(&mut self, index: usize, car: &RollingChassis) {
        let parts = &self.parts[index];
        for &body in &parts.bodies {
            if self.core.world.copy_body_state_from(&car.core.world, body) {
                self.core.world.body_geoms_moved(body);
            }
        }
        for &joint in &parts.joints {
            self.core.world.copy_joint_state_from(&car.core.world, joint);
        }
        for &geom in &parts.geoms {
            let from = car.core.world.collision.geom(geom);
            let (category, collide) = (from.category_bits, from.collide_bits);
            let to = self.core.world.collision.geom_mut(geom);
            to.category_bits = category;
            to.collide_bits = collide;
        }
    }

    /// One step of the engine for all its cars (`cars` in creation order, all of them).
    /// `physics_time` is the clock of this step, ms.
    ///
    /// What the game's main thread queues for the physics thread runs before this: make such
    /// changes on the cars between two calls (or with [`RollingChassis::queue`], which runs
    /// them when the clock shows the new step's time).
    pub fn step(&mut self, dt: f32, physics_time: f64, cars: &mut [CarSlot<'_>]) {
        assert_eq!(cars.len(), self.parts.len(), "a step takes every car of the engine, in creation order");
        self.step_counter = self.step_counter.wrapping_add(1);

        // Track::step @ 0x140278d20: the laps of all cars rubber the track in
        if let Some(track) = &mut self.dynamic_track {
            let mut laps = 0i32;
            for slot in cars.iter() {
                laps = laps.wrapping_add(slot.car.transponder.lap_count as i32);
            }
            track.step(laps);
            self.env.dynamic_grip_level = track.dynamic_grip_level;
        }
        // PhysicsEngine::stepWind
        self.env.step_wind(physics_time);

        // what a car reads of the engine and of the other cars while it steps
        let positions: Vec<Vec3f> = cars.iter().map(|slot| slot.car.core.get_position(slot.car.body)).collect();
        let wakes: Vec<SlipStream> = cars.iter().map(|slot| slot.car.slip_stream).collect();
        for (index, slot) in cars.iter_mut().enumerate() {
            let car = &mut *slot.car;
            car.env.wind = self.env.wind;
            car.env.wind_speed = self.env.wind_speed;
            car.env.wind_direction_deg = self.env.wind_direction_deg;
            if self.dynamic_track.is_some() {
                car.env.dynamic_grip_level = self.env.dynamic_grip_level;
            }
            car.other_car_positions.clear();
            car.other_wakes.clear();
            for other in 0..positions.len() {
                if other != index {
                    car.other_car_positions.push(positions[other]);
                    car.other_wakes.push(wakes[other]);
                }
            }
        }

        // Car::stepPreCacheValues and Car::step
        self.step_cars(dt, physics_time, cars);

        // every car's part goes into the one world
        for (index, slot) in cars.iter_mut().enumerate() {
            self.write_back(index, slot.car);
            slot.car.physics_events.clear();
            slot.car.collision_events.clear();
            slot.car.contact_callbacks = 0;
        }

        // PhysicsCore::step @ 0x1402cd690: the collision pass, with the game's callback
        // after every new contact joint, then dWorldStep
        self.last_events.clear();
        self.contact_callbacks = 0;
        if self.collisions_enabled {
            let track = self.track.clone();
            let own = self.core.statics.clone();
            let statics = match &track {
                Some(track) => Some(&track.world),
                None => own.as_deref(),
            };
            let events = self.core.collision_step(statics);
            self.contact_callbacks = events.len() as u32;
            let car_bodies = self.car_bodies();
            for event in &events {
                // PhysicsEngine::onCollisionCallBack @ 0x140264020: every car is told
                let event = turn_to_car(event, &car_bodies);
                for slot in cars.iter_mut() {
                    let inputs = CollisionInputs::read(&self.core, slot.car.body, &event);
                    if inputs.is_some() {
                        slot.car.contact_callbacks += 1;
                    }
                    slot.car.on_collision_callback(&event, inputs.as_ref());
                }
                self.last_events.push(event);
            }
        }
        self.step_stats = self.core.world_step(dt);
        // PhysicsEngine::evOnStepCompleted: each loose object's handler (they were
        // registered with the track, before any car's)
        self.core.step_track_objects();

        // every car gets its bodies back, then Car::postStep in creation order
        for (index, slot) in cars.iter_mut().enumerate() {
            for &body in &self.parts[index].bodies {
                slot.car.core.world.copy_body_state_from(&self.core.world, body);
            }
            slot.car.step_stats = self.step_stats;
            let mut feed = DeviceFeed { device: slot.device.get(), dt };
            slot.car.step_completed(dt, &mut feed);
        }
    }

    /// `Car::stepPreCacheValues` and `Car::step` of every car.
    fn step_cars(&mut self, dt: f32, physics_time: f64, cars: &mut [CarSlot<'_>]) {
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(pool) = &self.pool {
            // the game's split: the first `cars / workers + 1` cars on the calling thread,
            // one task for each of the others. A car whose device must stay here stays here.
            let count = cars.len();
            let here = count / pool.threads().max(1) + 1;
            let pooled: Vec<usize> = (0..count).filter(|&i| i >= here && matches!(cars[i].device, DeviceRef::Anywhere(_))).collect();
            let shared = SharedSlots(cars.as_mut_ptr(), std::marker::PhantomData);
            let shared = &shared;
            let pooled_ref = &pooled;
            let task = move |item: usize| {
                // SAFETY: `SharedSlots`: this index is this thread's alone, and it is in
                // bounds (`pooled` holds indices below `count`).
                let slot = unsafe { &mut *shared.0.add(pooled_ref[item]) };
                step_one(slot, dt, physics_time);
            };
            let before = || {
                for index in 0..count {
                    if !pooled_ref.contains(&index) {
                        // SAFETY: as above; these indices are not in `pooled`.
                        let slot = unsafe { &mut *shared.0.add(index) };
                        step_one(slot, dt, physics_time);
                    }
                }
            };
            pool.run_with(pooled.len(), &task, before);
            return;
        }
        for slot in cars.iter_mut() {
            step_one(slot, dt, physics_time);
        }
    }
}
