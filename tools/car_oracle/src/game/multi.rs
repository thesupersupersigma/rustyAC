// SPDX-License-Identifier: GPL-3.0-or-later

//! Task 24: several cars in the game's own `PhysicsEngine`, free-running from step 0.
//!
//! Every car is made with the game's `Car::Car` in one engine (so the second car has
//! `physicsGUID` 1 and so on), gets its collider mesh, a scripted device of its own, and is
//! put on the track. Each step the drivers set their devices, `PhysicsEngine::step` runs, and
//! one row goes into the recording ([`rustyac_physics::engine::record`]): what every device
//! reported, every body of every car, damage, tyres, engine, the collision switches, and the
//! contact joints of the world (with the `bounce_vel` the game's joints really got).

use rustyac_physics::car::colliders::CarColliders;
use rustyac_physics::engine::record::{contact_words, CarMeta, ContactNote, Recording};
use rustyac_physics::track::Track;

use super::*;
use crate::track_driver::Grip;

/// How one car of a scenario is driven.
#[derive(Clone, Debug)]
pub enum Drive {
    /// On the brakes.
    Parked,
    /// By the clock: from each time on, these pedals and this wheel (gas, brake, steer).
    Script(Vec<(f32, f32, f32, f32)>),
    /// Along the AI line. `lateral`: (from this time on, metres to the left of the line),
    /// eased; `pace` and `limit` (m/s) as the track scenarios have them; from `brake_at` on
    /// the car stops.
    Follow { lateral: Vec<(f32, f32)>, pace: f32, limit: f32, brake_at: f32 },
    /// Along the pit lane's line from `from` on, then straight on with this throttle until
    /// `brake_at`.
    PitLane { from: f32, then_gas: f32, brake_at: f32 },
}

/// One car of a scenario.
#[derive(Clone, Debug)]
pub struct MultiCar {
    pub name: String,
    pub position: V3,
    pub tail: V3,
    pub drive: Drive,
}

pub struct MultiScenario {
    pub name: &'static str,
    pub about: &'static str,
    pub seconds: f32,
    /// No contacts are looked for during the first steps, as at a session's start.
    pub no_collision_steps: i32,
    pub cars: fn(&Track, &[String]) -> Result<Vec<MultiCar>, String>,
    /// The cars when `--cars` names none.
    pub default_cars: &'static [&'static str],
}

const F2004: &str = "ks_ferrari_f2004";

/// A pose on the AI line at `n`, moved `back` metres against the driving direction and
/// `left` metres to the left.
fn line_pose(track: &Track, n: f32, back: f32, left: f32) -> Result<(V3, V3), String> {
    let spline = track.ai_spline.as_ref().ok_or("the track has no AI line")?;
    let n = spline.spline.wrap_position(n - back / spline.length());
    let (p, tail) = track.pose_on_ai_line_at(n).ok_or("no pose on the AI line")?;
    // forward = -tail; left of forward, level
    let (fx, fz) = (-tail.x, -tail.z);
    let l = (fx * fx + fz * fz).sqrt().max(1e-6);
    let (lx, lz) = (fz / l, -fx / l);
    let moved = rustyac_physics::vecmath::Vec3f::new(p.x + lx * left, p.y + 3.0, p.z + lz * left);
    use rustyac_physics::tyre::RayTrackCollisionProvider;
    let ground = track.ray_cast(&moved, &rustyac_physics::vecmath::Vec3f::new(0.0, -1.0, 0.0), 20.0).map(|hit| hit.pos.y).unwrap_or(p.y);
    Ok(([moved.x, ground, moved.z], [tail.x, tail.y, tail.z]))
}

fn name_of(cars: &[String], index: usize, default: &[&str]) -> String {
    cars.get(index).cloned().unwrap_or_else(|| default[index % default.len()].to_string())
}

/// The straight after Raidillon on Spa, as a share of the lap; any track's line works.
const STRAIGHT: f32 = 0.20;

pub fn scenarios() -> Vec<MultiScenario> {
    vec![
        MultiScenario {
            name: "mc_side_by_side",
            about: "two cars side by side down a straight, flat out, then on the brakes",
            seconds: 12.0,
            no_collision_steps: 0,
            default_cars: &[F2004, F2004],
            cars: |track, cars| {
                let mut out = Vec::new();
                for (i, left) in [1.6f32, -1.6].into_iter().enumerate() {
                    let (position, tail) = line_pose(track, STRAIGHT, 0.0, left)?;
                    out.push(MultiCar { name: name_of(cars, i, &[F2004]), position, tail, drive: Drive::Follow { lateral: vec![(0.0, left)], pace: 10.0, limit: 200.0, brake_at: 9.0 } });
                }
                Ok(out)
            },
        },
        MultiScenario {
            name: "mc_rear_end",
            about: "a car flat out into the back of a standing one",
            seconds: 9.0,
            no_collision_steps: 0,
            default_cars: &[F2004, F2004],
            cars: |track, cars| {
                let (p0, t0) = line_pose(track, STRAIGHT, 0.0, 0.0)?;
                let (p1, t1) = line_pose(track, STRAIGHT, 45.0, 0.0)?;
                Ok(vec![
                    MultiCar { name: name_of(cars, 0, &[F2004]), position: p0, tail: t0, drive: Drive::Parked },
                    MultiCar { name: name_of(cars, 1, &[F2004]), position: p1, tail: t1, drive: Drive::Follow { lateral: vec![(0.0, 0.0)], pace: 10.0, limit: 200.0, brake_at: 6.0 } },
                ])
            },
        },
        MultiScenario {
            name: "mc_side_swipe",
            about: "two cars side by side; after three seconds one turns into the other",
            seconds: 10.0,
            no_collision_steps: 0,
            default_cars: &[F2004, F2004],
            cars: |track, cars| {
                let (p0, t0) = line_pose(track, STRAIGHT, 0.0, 1.6)?;
                // (the car on the right is slower off the line: it starts eight metres ahead)
                let (p1, t1) = line_pose(track, STRAIGHT, -8.0, -1.6)?;
                Ok(vec![
                    MultiCar { name: name_of(cars, 0, &[F2004]), position: p0, tail: t0, drive: Drive::Follow { lateral: vec![(0.0, 1.6)], pace: 10.0, limit: 200.0, brake_at: 8.0 } },
                    MultiCar { name: name_of(cars, 1, &[F2004]), position: p1, tail: t1, drive: Drive::Follow { lateral: vec![(0.0, -1.6), (2.8, 2.4)], pace: 10.0, limit: 200.0, brake_at: 8.0 } },
                ])
            },
        },
        MultiScenario {
            name: "mc_pile_up",
            about: "four cars in a row leave the road to the left into the barrier, one after the other",
            seconds: 14.0,
            no_collision_steps: 0,
            default_cars: &[F2004, F2004, F2004, F2004],
            cars: |track, cars| {
                let mut out = Vec::new();
                for i in 0..4 {
                    let (position, tail) = line_pose(track, STRAIGHT, 16.0 * i as f32, 0.0)?;
                    out.push(MultiCar {
                        name: name_of(cars, i, &[F2004]),
                        position,
                        tail,
                        drive: Drive::Follow { lateral: vec![(0.0, 0.0), (2.5, 60.0)], pace: 10.0, limit: 200.0, brake_at: 9.0 },
                    });
                }
                Ok(out)
            },
        },
        MultiScenario {
            name: "mc_pit_lane",
            about: "a car parked in the pit lane; two more drive through it and out of the lane, nose to tail (the pit-lane rule)",
            seconds: 34.0,
            no_collision_steps: 0,
            default_cars: &[F2004, F2004, F2004],
            cars: |track, cars| {
                let pose = |n: f32| -> Result<(V3, V3), String> {
                    let (p, tail) = crate::track_driver::pit_pose(track, n).ok_or("the track has no pit lane line")?;
                    Ok(([p.x, p.y, p.z], [tail.x, tail.y, tail.z]))
                };
                let length = track.pit_lane_spline.as_ref().ok_or("the track has no pit lane line")?.length();
                let (p0, t0) = pose(0.30)?;
                let (p1, t1) = pose(0.30 - 14.0 / length)?;
                let (p2, t2) = pose(0.30 - 19.0 / length)?;
                Ok(vec![
                    MultiCar { name: name_of(cars, 0, &[F2004]), position: p0, tail: t0, drive: Drive::Parked },
                    MultiCar { name: name_of(cars, 1, &[F2004]), position: p1, tail: t1, drive: Drive::PitLane { from: 0.5, then_gas: 1.0, brake_at: 30.0 } },
                    MultiCar { name: name_of(cars, 2, &[F2004]), position: p2, tail: t2, drive: Drive::PitLane { from: 0.5, then_gas: 0.35, brake_at: 30.0 } },
                ])
            },
        },
        MultiScenario {
            name: "mc_field",
            about: "six cars from the grid on a lap with the line follower",
            seconds: 150.0,
            no_collision_steps: 250,
            default_cars: &[F2004, F2004, F2004, F2004, F2004, F2004],
            cars: |track, cars| grid(track, cars, &[F2004], 6, 0.78, 0.035),
        },
        MultiScenario {
            name: "mc_mixed",
            about: "four different cars from the grid: F2004, E30, a 4WD car, a live-axle car",
            seconds: 75.0,
            no_collision_steps: 250,
            default_cars: &[F2004, "bmw_m3_e30", "ks_lamborghini_sesto_elemento", "ks_ferrari_250_gto"],
            cars: |track, cars| grid(track, cars, &[F2004, "bmw_m3_e30", "ks_lamborghini_sesto_elemento", "ks_ferrari_250_gto"], 4, 0.72, 0.0),
        },
    ]
}

/// `count` cars on the first grid slots, each following the line at its own pace (the car in
/// front the fastest).
fn grid(track: &Track, cars: &[String], default: &[&str], count: usize, pace: f32, pace_step: f32) -> Result<Vec<MultiCar>, String> {
    let count = if cars.is_empty() { count } else { cars.len() };
    let spline = track.ai_spline.as_ref().ok_or("the track has no AI line")?;
    let mut out = Vec::new();
    for i in 0..count {
        let (p, tail) = track.spawn_pose("START", i).ok_or_else(|| format!("the track has no grid slot {i}"))?;
        // where the slot is beside the line: the car eases onto the line over the first seconds
        let n = spline.spline.world_to_spline(&[p.x, p.y, p.z], -1);
        let q = spline.spline.spline_to_world(n);
        let f = spline.payload_at_position(n).forward_vector;
        let l = (f[0] * f[0] + f[2] * f[2]).sqrt().max(1e-6);
        let side = (p.x - q[0]) * (f[2] / l) + (p.z - q[2]) * (-f[0] / l);
        out.push(MultiCar {
            name: name_of(cars, i, default),
            position: [p.x, p.y, p.z],
            tail: [tail.x, tail.y, tail.z],
            drive: Drive::Follow { lateral: vec![(0.0, side), (6.0 + i as f32, 0.0)], pace: pace - pace_step * i as f32, limit: 95.0, brake_at: f32::MAX },
        });
    }
    Ok(out)
}

// --- the scripted device of one car -----------------------------------------------------------
// +0x00 vtable, +0x10 gas, brake, steer, clutch, hand brake (f32), +0x24 buttons, +0x28
// requested gear, +0x2c how often it was asked
const DEV_GAS: usize = 0x10;
const DEV_BUTTONS: usize = 0x24;
const DEV_REQUESTED_GEAR: usize = 0x28;
const DEV_POLLED: usize = 0x2c;

extern "C" fn multi_acquire_controls(this: *mut u8, controls: *mut u8, _dt: f32, _input: *const f32) {
    unsafe {
        let pedals: [f32; 5] = rd(this, DEV_GAS);
        let buttons: u32 = rd(this, DEV_BUTTONS);
        wr(controls, CC_GEAR_UP, (buttons & 1 != 0) as u8);
        wr(controls, CC_GEAR_DN, (buttons & 2 != 0) as u8);
        wr(controls, CC_DRS, (buttons & 4 != 0) as u8);
        wr(controls, CC_KERS, (buttons & 8 != 0) as u8);
        wr(controls, CC_HAND_BRAKE, pedals[4]);
        wr(controls, CC_REQUESTED_GEAR_INDEX, rd::<i32>(this, DEV_REQUESTED_GEAR));
        wr(controls, CC_GAS, pedals[0]);
        wr(controls, CC_BRAKE, pedals[1]);
        wr(controls, CC_STEER, pedals[2]);
        wr(controls, CC_CLUTCH, pedals[3]);
        wr(this, DEV_POLLED, rd::<u32>(this, DEV_POLLED) + 1);
    }
}

extern "C" fn multi_get_action(_this: *mut u8, _action: i32) -> u8 {
    0
}

/// One car of the game's engine.
struct GameCar {
    car: *mut u8,
    device: *mut u8,
    /// ODE's `dxBody` of every body, in creation order.
    bodies: Vec<*mut u8>,
    suspensions: [*mut u8; 4],
    mesh_geom: *mut u8,
    drive: Drive,
    lateral: f32,
    pit_done: bool,
    grip: Grip,
}

/// The game's world with several cars.
pub struct MultiWorld<'a> {
    acs: &'a Acs,
    pub engine: *mut u8,
    cars: Vec<GameCar>,
    track: std::sync::Arc<Track>,
    objects: Vec<*mut u8>,
    pub step_index: usize,
    pub recording: Recording,
}

unsafe fn car_bodies(acs: &Acs, car: *mut u8) -> (Vec<*mut u8>, [*mut u8; 4]) {
    let ode = |wrapper: *mut u8| -> *mut u8 { rd(wrapper, RB_ODE_BODY) };
    let mut bodies = vec![ode(rd(car, CAR_BODY)), ode(rd(car, CAR_FUEL_TANK_BODY))];
    let axle: *mut u8 = rd(car, CAR_RIGID_AXLE);
    if !axle.is_null() {
        bodies.push(ode(axle));
    }
    let begin: *const *mut u8 = rd(car, CAR_SUSPENSIONS);
    let end: *const *mut u8 = rd(car, CAR_SUSPENSIONS + 8);
    assert_eq!(end.offset_from(begin), 4, "the car does not have four suspensions");
    let mut suspensions = [std::ptr::null_mut(); 4];
    for (i, slot) in suspensions.iter_mut().enumerate() {
        let suspension = *begin.add(i);
        let vtable = rd::<usize>(suspension, 0);
        let class = SUS_CLASSES.iter().find(|class| acs.va(class.vtable) == vtable).expect("one of the four suspension classes");
        if class.name == "ML" {
            // members the game never writes (see `find_bodies_and_joints`)
            for offset in [0x10, SUS_BUMP_STOP_UP, SUS_BUMP_STOP_DN, 0x2c] {
                wr(suspension, offset, 0.0f32);
            }
        }
        if let Some(hub) = class.hub {
            bodies.push(ode(rd(suspension, hub)));
        }
        if let Some(strut) = class.strut_body {
            bodies.push(ode(rd(suspension, strut)));
        }
        *slot = suspension;
    }
    (bodies, suspensions)
}

impl<'a> MultiWorld<'a> {
    /// The current directory must be the scratch root, prepared for every car and the track.
    #[allow(clippy::too_many_arguments)]
    pub fn build(
        acs: &'a Acs,
        scenario: &MultiScenario,
        car_names: &[String],
        track_folder: &Path,
        layout: &str,
        colliders: &dyn Fn(&str) -> Result<CarColliders, String>,
        seed: u32,
    ) -> Result<MultiWorld<'a>, String> {
        unsafe {
            let engine = new_engine(acs, seed);
            let _ = std::fs::remove_file("cfg/race.ini");
            // the game's own track with its real collision categories
            let built = crate::track::GameTrack::build(acs, engine, track_folder, layout, false)?;
            if !built.surface_mismatches.is_empty() {
                return Err(format!("the game's surfaces differ from the port's: {:?}", built.surface_mismatches));
            }
            let place = |name: &str| -> Option<V3> {
                let m = built.rust.helper_nodes.iter().find(|h| h.name == name)?.local.m;
                Some([m[3][0], m[3][1], m[3][2]])
            };
            let add_time_line: extern "C" fn(*mut u8, *const V3, *const V3, i32) = std::mem::transmute(acs.va(VA_TRACK_ADD_TIME_LINE));
            let mut lines = 0;
            while let (Some(left), Some(right)) = (place(&format!("AC_TIME_{lines}_L")), place(&format!("AC_TIME_{lines}_R"))) {
                add_time_line(built.track, &left, &right, 0);
                lines += 1;
            }
            assert_eq!(lines, built.rust.time_lines.len(), "the port found other timing lines (point-to-point tracks are not for these scenarios)");
            wr(engine, PE_PENALTY_MODE, 1i32);
            wr(engine, PE_ALLOWED_TYRES_OUT, 2i32);
            let game_objects = crate::track::create_objects(acs, engine, &built.rust);
            let objects: Vec<*mut u8> = game_objects.iter().map(|o| o.body()).collect();
            let track = std::sync::Arc::new(built.rust.clone());
            let built: &'static crate::track::GameTrack = Box::leak(Box::new(built));
            let _ = built;

            let plan = (scenario.cars)(&track, car_names)?;
            let srand: extern "C" fn(u32) = std::mem::transmute(acs.crt_function(c"srand"));
            let force_rotation: extern "C" fn(*mut u8, *const V3) = std::mem::transmute(acs.va(VA_CAR_FORCE_ROTATION));
            let force_position: extern "C" fn(*mut u8, *const V3, u8) = std::mem::transmute(acs.va(VA_CAR_FORCE_POSITION));
            let set_damage_level: extern "C" fn(*mut u8, f32) = std::mem::transmute(acs.va(VA_CAR_SET_DAMAGE_LEVEL));
            let reset_suspension_damage: extern "C" fn(*mut u8) = std::mem::transmute(acs.va(VA_CAR_RESET_SUSPENSION_DAMAGE));
            let set_compound: extern "C" fn(*mut u8, i32) -> u8 = std::mem::transmute(acs.va(VA_TYRE_SET_COMPOUND));
            let init_mesh: extern "C" fn(*mut u8, *mut u8, *const [f32; 16]) = std::mem::transmute(acs.va(VA_CAR_INIT_COLLIDER_MESH));
            let ctor: extern "C" fn(*mut u8, *mut u8, *mut u8, *mut u8) -> *mut u8 = std::mem::transmute(acs.va(VA_CAR_CTOR));

            // one device class for all cars: the RTTI pointer of a real device in front so
            // that the game's dynamic_cast<AIDriver*> answers "not an AI"
            let keyboard = acs.va(VA_KEYBOARD_CONTROL_VTABLE) as *const usize;
            let table: Vec<usize> = vec![
                *keyboard.offset(-1),
                device_destructor as *const () as usize,
                multi_acquire_controls as *const () as usize,
                multi_get_action as *const () as usize,
                device_send_ff as *const () as usize,
                device_get_name as *const () as usize,
                device_get_ff_global_gain as *const () as usize,
                device_false as *const () as usize,
                device_on_auto_shifter_changed as *const () as usize,
                device_false as *const () as usize,
                device_set_vibrations as *const () as usize,
                device_set_engine_rpm as *const () as usize,
                device_false as *const () as usize,
            ];
            let table = leak(table).add(1);

            let mut cars = Vec::new();
            let mut metas = Vec::new();
            let mut geom_high = Vec::new();
            for (index, planned) in plan.iter().enumerate() {
                // each car's draws from rand() start from a known point
                let car_seed = seed.wrapping_add(1000 * (index as u32 + 1));
                srand(car_seed);
                let car = acs.alloc(CAR_SIZE);
                ctor(car, engine, wstring(acs, &planned.name), wstring(acs, ""));
                assert_eq!(rd::<u32>(car, CAR_PHYSICS_GUID), index as u32, "the car's physicsGUID is not its number");
                let (bodies, suspensions) = car_bodies(acs, car);

                // the collider mesh (CarAvatar::initPhysics: the first mesh of collider.kn5)
                let loaded = colliders(&planned.name)?;
                let real = loaded.mesh.as_ref().ok_or_else(|| format!("{}: no collider.kn5 in the game's folder", planned.name))?;
                let mesh = object(0x168);
                let mut vertices = vec![0f32; real.vertices.len() * 11];
                for (i, p) in real.vertices.iter().enumerate() {
                    vertices[i * 11..i * 11 + 3].copy_from_slice(p);
                }
                let indices = real.indices.clone();
                let mut matrix = [0f32; 16];
                for r in 0..4 {
                    for c in 0..4 {
                        matrix[4 * r + c] = real.matrix.m[r][c];
                    }
                }
                let vertex_count = vertices.len() / 11;
                let vp = leak(vertices).cast::<u8>();
                wr(mesh, 0x108, vp);
                wr(mesh, 0x110, vp.add(vertex_count * 0x2c));
                wr(mesh, 0x118, vp.add(vertex_count * 0x2c));
                let n = indices.len();
                let ip = leak(indices).cast::<u8>();
                wr(mesh, 0x120, ip);
                wr(mesh, 0x128, ip.add(n * 2));
                wr(mesh, 0x130, ip.add(n * 2));
                init_mesh(car, mesh, &matrix);
                let body: *const u8 = rd(car, CAR_BODY);
                let meshes: *const *const u8 = rd(body, 0x30);
                let mesh_geom: *mut u8 = rd(*meshes, 0x10);
                geom_high.push((mesh_geom as usize >> 32) as u32);

                let device = object(0x40);
                wr(device, 0, table);
                wr(device, DEV_GAS, [0.0f32, 0.0, 0.0, 1.0, 0.0]);
                wr(device, DEV_REQUESTED_GEAR, -1i32);
                wr(car, CAR_CONTROLS_PROVIDER, device);
                wr(car, CAR_PERFORMANCE_METER_IS_ENABLED, 0u8);
                wr(car, CAR_TELEMETRY_IS_ENABLED, 0u8);
                // the automatic clutch and gearbox (CarAvatar::setAutoClutchEnabled, the assist option)
                wr(car, CAR_AUTOCLUTCH + 0xc, 1u8);
                wr(car, CAR_AUTOCLUTCH + 0xd, 1u8);
                wr(car, CAR_AUTO_SHIFT, 1u8);

                force_rotation(car, &planned.tail);
                force_position(car, &planned.position, 1);
                set_damage_level(car, 0.0);
                reset_suspension_damage(car);
                // the session start: the default compound, the setup screen's round trip
                use rustyac_physics::data::ini::IniReader;
                let data = Path::new("content/cars").join(&planned.name).join("data");
                let tyres = IniReader::load(&data.join("tyres.ini"))?;
                let compound = tyres.get_int("COMPOUND_DEFAULT", "INDEX").unwrap_or(0);
                for wheel in 0..4 {
                    set_compound(car.add(CAR_TYRES + wheel * TYRE_SIZE), compound);
                }
                if let Ok(setup) = IniReader::load(&data.join("setup.ini")) {
                    apply_setup_screen_defaults(car, &setup);
                }
                metas.push(CarMeta {
                    name: planned.name.clone(),
                    bodies: bodies.len(),
                    seed: car_seed,
                    position: planned.position,
                    tail: planned.tail,
                    auto_clutch: true,
                    auto_shifter: true,
                    collider_mesh: true,
                });
                let lateral = match &planned.drive {
                    Drive::Follow { lateral, .. } => lateral.first().map_or(0.0, |l| l.1),
                    _ => 0.0,
                };
                cars.push(GameCar { car, device, bodies, suspensions, mesh_geom, drive: planned.drive.clone(), lateral, pit_done: false, grip: Grip::of(&planned.name) });
            }
            let core: *mut u8 = rd(engine, PE_CORE);
            wr(core, 0x60, scenario.no_collision_steps);

            let mut recording = Recording::default();
            recording.set("scenario", scenario.name);
            recording.set("about", scenario.about);
            recording.set("track", track.name.clone());
            recording.set("track_folder", track_folder.display().to_string());
            recording.set("track_layout", layout);
            recording.set("track_objects", "1");
            recording.set("track_object_count", objects.len().to_string());
            recording.set("seed", seed.to_string());
            recording.set("clock_start_ms", format!("{CLOCK_START_MS}"));
            recording.set("ambient", format!("{AMBIENT_TEMPERATURE}"));
            recording.set("road", format!("{ROAD_TEMPERATURE}"));
            recording.set("penalty_mode", "1");
            recording.set("allowed_tyres_out", "2");
            recording.set("no_collision_steps", scenario.no_collision_steps.to_string());
            // what the game's mesh contact joints get as bounce_vel when nothing better is
            // known: the upper half of the address of a car's mesh geom (all the same)
            recording.set("mesh_geom_high", geom_high.iter().map(|h| format!("{h:08x}")).collect::<Vec<_>>().join(","));
            recording.set("mesh_bounce_vel", format!("{:08x}", geom_high[0]));
            recording.set_cars(&metas);
            Ok(MultiWorld { acs, engine, cars, track, objects, step_index: 0, recording })
        }
    }

    unsafe fn view(&self, car: &GameCar) -> CarView {
        let body = car.bodies[0];
        let r: [f32; 12] = rd(body, B_R);
        let lock: f32 = rd(car.car, CAR_STEER_LOCK);
        let steer_ratio: f32 = rd(car.car, CAR_STEER_RATIO);
        CarView {
            position: v3(body, B_POS),
            forward: [r[2], r[6], r[10]],
            left: [r[0], r[4], r[8]],
            npos: if self.step_index == 0 { -1.0 } else { rd(car.car, CAR_SPLINE_LOCATOR_DATA) },
            offset: rd(car.car, CAR_SPLINE_LOCATOR + 0x24),
            max_wheel_angle: if steer_ratio != 0.0 { (lock / steer_ratio).to_radians() } else { 0.3 },
            step: self.step_index,
            speed: rd(car.car, CAR_VALUE_CACHE_SPEED),
            road_rpm: 0.0,
            gear: rd(car.car, CAR_DRIVETRAIN + 0x584),
            yaw_rate: 0.0,
            laps: rd(car.car.add(CAR_TRANSPONDER), 0x0c),
            damaged: rd::<[f32; 5]>(car.car, CAR_DAMAGE_ZONE_LEVEL)[4] > 0.0,
        }
    }

    /// The controls of every car for the next step.
    unsafe fn drive(&mut self) {
        let t = self.step_index as f32 * DT;
        for index in 0..self.cars.len() {
            let view = self.view(&self.cars[index]);
            let mut c = Controls { clutch: 1.0, requested_gear: -1, ..Controls::default() };
            let track = std::sync::Arc::clone(&self.track);
            let car = &mut self.cars[index];
            match &car.drive {
                Drive::Parked => c.brake = 1.0,
                Drive::Script(segments) => {
                    if let Some(&(_, gas, brake, steer)) = segments.iter().rev().find(|segment| t >= segment.0) {
                        c.gas = gas;
                        c.brake = brake;
                        c.steer = steer;
                    }
                }
                Drive::Follow { lateral, pace, limit, brake_at } => {
                    let target = lateral.iter().rev().find(|l| t >= l.0).map_or(0.0, |l| l.1);
                    car.lateral += (target - car.lateral) * (DT / 0.35).min(1.0);
                    crate::track_driver::follow_line(&track, &view, car.lateral, *pace, *limit, &car.grip, t, &mut c);
                    if t >= *brake_at {
                        c.gas = 0.0;
                        c.brake = 1.0;
                    }
                }
                Drive::PitLane { from, then_gas, brake_at } => {
                    if t < *from {
                        c.brake = 1.0;
                    } else if t >= *brake_at {
                        c.brake = 1.0;
                    } else if car.pit_done || !crate::track_driver::follow_pit_lane(&track, &view, &mut c) {
                        car.pit_done = true;
                        c.gas = *then_gas;
                    }
                }
            }
            let buttons = c.gear_up as u32 | (c.gear_dn as u32) << 1 | (c.drs as u32) << 2 | (c.kers as u32) << 3;
            wr(car.device, DEV_GAS, [c.gas, c.brake, c.steer, c.clutch, c.hand_brake]);
            wr(car.device, DEV_BUTTONS, buttons);
            wr(car.device, DEV_REQUESTED_GEAR, c.requested_gear);
        }
    }

    unsafe fn contacts(&self) -> Vec<ContactNote> {
        let core: *const u8 = rd(self.engine, PE_CORE);
        let world: *const u8 = rd(core, CORE_WORLD);
        let vtable = self.acs.va(0x1_4051_2d50); // dxJointContact
        let car_of = |body: *mut u8| -> i32 {
            if body.is_null() {
                -1
            } else {
                self.cars.iter().position(|car| car.bodies.contains(&body)).map_or(-2, |n| n as i32)
            }
        };
        let mut out = Vec::new();
        // the world's joint list: the newest joint first
        let mut joint: *const u8 = rd(world, 0x28);
        while !joint.is_null() {
            if rd::<usize>(joint, 0) == vtable {
                out.push(ContactNote {
                    pos: rd(joint, 0xd0),
                    normal: rd(joint, 0xe0),
                    depth: rd(joint, 0xf0),
                    car1: car_of(rd(joint, 0x40)),
                    car2: car_of(rd(joint, 0x58)),
                    reversed: rd::<u32>(joint, 0x30) & 2 != 0,
                    mode: rd(joint, 0x90),
                    mu: rd(joint, 0x94),
                    bounce: rd(joint, 0xa8),
                    bounce_vel: rd(joint, 0xac),
                    soft_cfm: rd(joint, 0xb4),
                });
            }
            joint = rd(joint, 0x10);
        }
        out
    }

    /// One step of the game's engine; the row goes into the recording.
    pub fn step(&mut self) {
        unsafe {
            self.drive();
            for car in &self.cars {
                wr(car.device, DEV_POLLED, 0u32);
            }
            let time_ms = CLOCK_START_MS + (self.step_index + 1) as f64 * 3.0;
            let step: extern "C" fn(*mut u8, f32, f64, f64) = std::mem::transmute(self.acs.va(VA_PHYSICS_ENGINE_STEP));
            step(self.engine, DT, time_ms, time_ms);
            // the sounds' queue is not read by anybody here: emptied as the game's main thread does
            let pop: unsafe extern "system" fn(*mut u8, *mut u8) -> bool =
                std::mem::transmute(self.acs.msvcp_function(c"?_Internal_pop_if_present@_Concurrent_queue_base_v4@details@Concurrency@@IEAA_NPEAX@Z"));
            let mut raw = [0u8; 0x48];
            while pop(self.engine.add(0x30), raw.as_mut_ptr()) {}

            let core: *const u8 = rd(self.engine, PE_CORE);
            let mut row: Vec<u32> = Vec::with_capacity(self.recording.row_words());
            row.extend(contact_words(&self.contacts()));
            row.push(rd::<i32>(core, 0x60) as u32);
            row.push(rd::<u32>(core, 0xa0));
            for car in &self.cars {
                let pedals: [u32; 5] = rd(car.device, DEV_GAS);
                row.extend(pedals);
                row.push(rd(car.device, DEV_BUTTONS));
                row.push(rd(car.device, DEV_REQUESTED_GEAR));
                assert_eq!(rd::<u32>(car.device, DEV_POLLED), 1, "a car's device was not asked exactly once");
                for &body in &car.bodies {
                    row.extend(rd::<[u32; 3]>(body, B_POS));
                    row.extend(rd::<[u32; 4]>(body, B_Q));
                    row.extend(rd::<[u32; 3]>(body, B_LVEL));
                    row.extend(rd::<[u32; 3]>(body, B_AVEL));
                }
                row.extend(rd::<[u32; 5]>(car.car, CAR_DAMAGE_ZONE_LEVEL));
                for &suspension in &car.suspensions {
                    // ISuspension::getDamage (slot +0x88)
                    let vtable: *const usize = rd(suspension, 0);
                    let get_damage: extern "C" fn(*mut u8) -> f32 = std::mem::transmute(*vtable.add(17));
                    row.push(get_damage(suspension).to_bits());
                }
                for wheel in 0..4 {
                    let status = car.car.add(CAR_TYRES + wheel * TYRE_SIZE + T_STATUS);
                    for offset in [S_ANGULAR_VELOCITY, 0x4, 0x2c, 0x10] {
                        row.push(rd(status, offset));
                    }
                }
                let d = car.car.add(CAR_DRIVETRAIN);
                row.extend(rd::<[u32; 2]>(d, 0x8));
                row.push(rd(d, 0x584));
                row.extend(rd::<[u32; 2]>(car.car, CAR_FUEL));
                row.push(rd(car.car, CAR_VALUE_CACHE_SPEED));
                row.push(rd(car.mesh_geom, 0x7c));
                row.push(rd::<u8>(car.car, 0x3e68) as u32);
                row.extend(rd::<[u32; 2]>(car.car, 0x3508));
                row.extend(rd::<[u32; 2]>(car.car, 0x3c30));
                row.push(rd(car.car.add(CAR_TRANSPONDER), 0x0c));
                row.push(rd(car.car, CAR_SPLINE_LOCATOR_DATA));
            }
            assert_eq!(row.len(), self.recording.row_words(), "a row of the wrong length");
            self.recording.rows.push(row);
            self.step_index += 1;
        }
    }

    /// A line about each car for the console.
    pub fn summary(&self) -> Vec<String> {
        unsafe {
            self.cars
                .iter()
                .enumerate()
                .map(|(n, car)| {
                    let p = v3(car.bodies[0], B_POS);
                    let damage: [f32; 5] = rd(car.car, CAR_DAMAGE_ZONE_LEVEL);
                    format!(
                        "car {n}: at ({:.1}, {:.1}, {:.1}), {:.0} km/h, laps {}, damage {:?}, pit ghost {}",
                        p[0],
                        p[1],
                        p[2],
                        rd::<f32>(car.car, CAR_VALUE_CACHE_SPEED) * 3.6,
                        rd::<u32>(car.car.add(CAR_TRANSPONDER), 0x0c),
                        damage,
                        rd::<u8>(car.car, 0x3e68)
                    )
                })
                .collect()
        }
    }

    pub fn object_count(&self) -> usize {
        self.objects.len()
    }
}
