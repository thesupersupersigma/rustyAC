// SPDX-License-Identifier: GPL-3.0-or-later

//! Task C5g: `CSP_ORACLE_PORT=1` (with `CSP_ORACLE_BREAK=<every wrapper>p` and
//! `CSP_ORACLE_BREAK_SETUP=1`): rustyac-ode's `csp_core` fed every outermost call the car makes
//! into CSP's cphys:: wrappers as it happens, nothing copied from CSP's world. Each getter's answer
//! is checked at its return, and the port's world against CSP's at each world step's entry.

use rustyac_ode::csp_collide::Unhandled;
use rustyac_ode::csp_core::{Core, TrackRay};
use rustyac_ode::csp_step::JointKind;
use std::collections::{BTreeMap, HashMap};
use std::sync::Mutex;

/// What a call's return is checked against or tells the port.
pub(crate) enum Expect {
    Nothing,
    /// the creator returned this body / joint of the port's
    Body(usize),
    Joint(usize),
    /// the f32s the call writes at this address (and whether in the set-up)
    Out(usize, Vec<f32>, bool),
    /// an f32 returned in xmm0
    Float(f32, bool),
    /// the port's box geom: named by CSP's at the return
    Geom(usize),
    /// a mesh collider on (body, wrapper, matrix, category, collide, key): made at the return,
    /// from the trimesh data CSP built
    Mesh(usize, usize, [f32; 16], u32, u32, u32),
    /// Task C5h: a tyre raycaster of this length made (+0x92fe80): named by CSP's at the return
    Raycaster(f32),
    /// Task C5h: a track ray's out struct (address, the port's, set-up)
    Ray(usize, Option<TrackRay>, bool),
}

struct Port {
    core: Core,
    bodies: HashMap<usize, usize>,
    joints: HashMap<usize, usize>,
    /// rva: (calls, wrong answers)
    calls: BTreeMap<usize, (usize, usize)>,
    ignored: BTreeMap<usize, usize>,
    /// set-up and per step
    answers: [usize; 2],
    wrong: [usize; 2],
    worlds: usize,
    differ: usize,
    log: String,
    track: bool,
    /// Task C5h: CSP's tyre raycasters and their lengths
    raycasters: HashMap<usize, f32>,
    /// the first thing the port could not do; nothing more is done once set
    broken: Option<String>,
    /// Task C5i-a `CSP_ORACLE_LOADER`: the run's car from rustyAC's loaders, the boxes seen per key,
    /// and (boxes, boxes differing, meshes, meshes differing)
    car: Option<rustyac_physics::csp::core_car::CspCar>,
    car_boxes: HashMap<u32, usize>,
    car_count: [usize; 4],
}

static PORT: Mutex<Option<Port>> = Mutex::new(None);

/// Part 3: whether the port's rays use Embree now (`None`: the port is off).
pub(crate) fn embree_ready() -> Option<bool> {
    PORT.lock().unwrap().as_ref().map(|p| p.core.embree_ready())
}

/// The track handed to the port once, at the first ray or world step (`w` the core): its
/// trimeshes as CSP loaded them, the ray meshes in CSP's list order, Embree's settings.
unsafe fn track_in(port: &mut Port, dll: usize, w: usize) {
    if std::mem::replace(&mut port.track, true) {
        return;
    }
    let t = match crate::collision::port_track(dll, w) {
        Ok(t) => t,
        Err(e) => {
            port.broken = Some(format!("track: {}", e.0));
            return;
        }
    };
    let ids: Vec<usize> = t.geoms.iter().map(|g| g.id).collect();
    let mut rays = crate::rays::ray_meshes(dll, &ids);
    let mut t = t;
    if let Ok(how) = std::env::var("CSP_ORACLE_LOADER") {
        let mut l = loader_track();
        port.log += &loader_compare(&t, rays.as_ref().ok(), &l);
        if how == "use" {
            // the port's geoms named by CSP's pointers, list position for list position
            for (g, &id) in l.geoms.iter_mut().zip(&ids) {
                g.id = id;
            }
            (t.geoms, t.meshes, t.level, t.count, t.levels, t.cars_level, t.static_step, t.aabb_limit, t.full_box_box) = (l.geoms, l.meshes, l.level, l.count, l.levels, l.cars_level, l.static_step, l.aabb_limit, l.full_box_box);
            (t.cars_levels, t.prefilter, t.edges, t.load) = (l.cars_levels, l.prefilter, l.edges, l.load);
            rays = Ok(l.rays);
        }
    }
    port.core.set_track(t.geoms, t.meshes, t.level, t.count, t.levels, t.cars_level, t.static_step, t.aabb_limit, t.full_box_box);
    let c = &mut port.core.col;
    (c.cars_levels, c.prefilter, c.edges, c.load) = (t.cars_levels, t.prefilter, t.edges, t.load);
    let r8 = |a: usize| *((dll + a) as *const u8);
    let config = crate::rays::config();
    let embree = (std::ptr::read_unaligned((dll + 0x282_0270) as *const usize) == 0 && r8(0x280_31ff) == 1).then_some((config.as_c_str(), r8(0x280_3200) != 0));
    match rays {
        Ok(meshes) => {
            if let Err(e) = port.core.set_rays(&meshes, embree, r8(0x281_2c7f) != 0) {
                port.broken = Some(format!("Embree: {e}"));
            }
        }
        Err(e) => port.broken = Some(format!("ray meshes: {}", e.0)),
    }
}

/// Task C5i-a: the run's `--track` loaded by rustyAC's own loader (CSP mode) and built as CSP's
/// collision track (`rustyac_physics::csp::core_track`).
fn loader_track() -> rustyac_physics::csp::core_track::CspTrack {
    let folder = std::env::args().skip_while(|a| a != "--track").nth(1).expect("--track");
    rustyac_physics::csp::set_enabled(true);
    let (track, _) = rustyac_physics::track::load_track(std::path::Path::new(&folder), "").expect("rustyAC loads the track");
    rustyac_physics::csp::core_track::build(&track)
}

/// `CSP_ORACLE_LOADER`: the loader's track against CSP's (geoms but their pointers as text,
/// meshes and trees as bits, ray meshes, settings); one line, and the first differences.
fn loader_compare(t: &crate::collision::PortTrack, rays: Option<&Vec<rustyac_ode::csp_core::RayMesh>>, l: &rustyac_physics::csp::core_track::CspTrack) -> String {
    let mut out = String::new();
    let mut bad = |what: &str, i: usize, a: String, b: String, n: &mut usize| {
        *n += 1;
        if *n <= 3 {
            out += &format!("loader {what} {i}:\n  csp    {a}\n  loader {b}\n");
        }
    };
    let text = |gs: &[rustyac_ode::csp_collision::Geom], i: usize| {
        let mut g = gs[i].clone();
        g.id = 0;
        if let rustyac_ode::csp_collision::Shape::Mesh { mesh, .. } = &mut g.shape {
            // the data's index printed apart (shared data shows as a repeat)
            return format!("data {mesh:?} ") + &{ *mesh = None; crate::collision::describe(std::slice::from_ref(&g), 0, &|_| Vec::new()) };
        }
        crate::collision::describe(std::slice::from_ref(&g), 0, &|_| Vec::new())
    };
    let (mut ng, mut nm, mut nr) = (0, 0, 0);
    for i in 0..t.geoms.len().min(l.geoms.len()) {
        let (a, b) = (text(&t.geoms, i), text(&l.geoms, i));
        if a != b {
            bad("geom", i, a, b, &mut ng);
        }
    }
    for i in 0..t.meshes.len().min(l.meshes.len()) {
        let (a, b) = (&t.meshes[i], &l.meshes[i]);
        if a.mesh != b.mesh || a.model.nodes != b.model.nodes || a.model.single_node != b.model.single_node || a.model.built != b.model.built {
            let d = |m: &rustyac_ode::csp_collision::TrackMesh| format!("{} tris {} verts {} nodes {} single {} built", m.mesh.nb_tris, m.mesh.vertices.len(), m.model.nodes.len(), m.model.single_node, m.model.built);
            let what = format!("verts {} idx {} nodes {}", a.mesh.vertices == b.mesh.vertices, a.mesh.indices == b.mesh.indices, a.model.nodes == b.model.nodes);
            bad("mesh", i, d(a), d(b) + " same: " + &what, &mut nm);
        }
    }
    let rays = rays.map_or(&[][..], |r| &r[..]);
    for (i, (a, b)) in rays.iter().zip(&l.rays).enumerate() {
        if a.geom != b.geom || a.vertices != b.vertices || a.indices != b.indices || a.two_sided != b.two_sided {
            bad("ray mesh", i, format!("geom {} {} verts {} idx {} two-sided", a.geom, a.vertices.len(), a.indices.len(), a.two_sided), format!("geom {} {} verts {} idx {} two-sided same v {} i {}", b.geom, b.vertices.len(), b.indices.len(), b.two_sided, a.vertices == b.vertices, a.indices == b.indices), &mut nr);
        }
    }
    let set = |level, count, levels, cars_level, static_step, aabb_limit, full, cars_levels, prefilter, edges, load| format!("level {level} count {count} levels {levels:?} cars {cars_level} static {static_step} aabb_limit {aabb_limit} full_box_box {full} cars_levels {cars_levels:?} prefilter {prefilter} edges {edges} load {load:?}");
    let (sa, sb) = (set(t.level, t.count, t.levels, t.cars_level, t.static_step, t.aabb_limit, t.full_box_box, t.cars_levels, t.prefilter, t.edges, t.load), set(l.level, l.count, l.levels, l.cars_level, l.static_step, l.aabb_limit, l.full_box_box, l.cars_levels, l.prefilter, l.edges, l.load));
    let same = sa == sb;
    if !same {
        out += &format!("loader settings:\n  csp    {sa}\n  loader {sb}\n");
    }
    format!("loader track: geoms {}/{} ({ng} differ), meshes {}/{} ({nm} differ), ray meshes {}/{} ({nr} differ), settings {}\n", t.geoms.len(), l.geoms.len(), t.meshes.len(), l.meshes.len(), rays.len(), l.rays.len(), if same { "same" } else { "differ" }) + &out
}

/// Task C5i-a: the run's `--car` from rustyAC's loaders (`CSP_ORACLE_LOADER` set; loaded once),
/// and whether the port uses it (`=use`).
fn loader_car(car: &mut Option<rustyac_physics::csp::core_car::CspCar>) -> Option<(&rustyac_physics::csp::core_car::CspCar, bool)> {
    let mode = std::env::var("CSP_ORACLE_LOADER").ok()?;
    if car.is_none() {
        let name = std::env::args().skip_while(|a| a != "--car").nth(1).expect("--car");
        let data = std::path::Path::new("content/cars").join(&name).join("data");
        // the game's folder (collider.kn5) is the one `--track` is in: <root>/content/tracks/<track>
        let track = std::env::args().skip_while(|a| a != "--track").nth(1).expect("--track");
        let root = std::path::Path::new(&track).ancestors().nth(3).expect("--track is <root>/content/tracks/<track>").to_path_buf();
        *car = Some(rustyac_physics::csp::core_car::build(&data, &root, &name).expect("rustyAC loads the car's colliders"));
    }
    car.as_ref().map(|c| (c, mode == "use"))
}

pub(crate) fn on() -> bool {
    std::env::var_os("CSP_ORACLE_PORT").is_some()
}

unsafe fn f32s(p: usize, n: usize) -> Vec<f32> {
    (0..n).map(|i| std::ptr::read_unaligned((p + 4 * i) as *const f32)).collect()
}

unsafe fn v3(p: usize) -> [f32; 3] {
    let v = f32s(p, 3);
    [v[0], v[1], v[2]]
}

/// A body by its game interface pointer, or by the cphys one 8 bytes in.
fn body_of(bodies: &HashMap<usize, usize>, x: usize) -> Option<usize> {
    bodies.get(&x).or_else(|| bodies.get(&x.wrapping_sub(8))).copied()
}

/// A wrapper's entry: the call done on the port; what its return must show.
pub(crate) unsafe fn enter(context: usize, rva: usize, step: usize) -> Expect {
    let r = |o: usize| std::ptr::read_unaligned((context + o) as *const usize);
    let xmm = |i: usize| std::ptr::read_unaligned((context + 0x1a0 + 0x10 * i) as *const f32);
    let [rcx, rdx, r8, r9] = [0x80, 0x88, 0xb8, 0xc0].map(r);
    let stack = |i: usize| std::ptr::read_unaligned((r(0x98) + 0x28 + 8 * i) as *const usize);
    let mut guard = PORT.lock().unwrap();
    let port = guard.get_or_insert_with(|| Port {
        core: Core::new(),
        bodies: HashMap::new(),
        joints: HashMap::new(),
        calls: BTreeMap::new(),
        ignored: BTreeMap::new(),
        answers: [0; 2],
        wrong: [0; 2],
        worlds: 0,
        differ: 0,
        log: String::new(),
        track: false,
        raycasters: HashMap::new(),
        broken: None,
        car: None,
        car_boxes: HashMap::new(),
        car_count: [0; 4],
    });
    if port.broken.is_some() {
        return Expect::Nothing;
    }
    let dll = crate::DLL_BASE.load(std::sync::atomic::Ordering::Relaxed);
    let b = body_of(&port.bodies, rcx);
    let expect = match (rva, b) {
        (0x92ed80, _) => {
            assert_eq!(std::ptr::read_unaligned((rcx + 0x3298) as *const usize), 0, "a split-along body pending: not ported");
            let no_finite = *((crate::DLL_BASE.load(std::sync::atomic::Ordering::Relaxed) + 0x280_320f) as *const u8) != 0;
            Expect::Body(port.core.create_body(no_finite))
        }
        (0x92f050, _) => {
            let (b1, b2) = (body_of(&port.bodies, rdx).expect("joint body 1"), body_of(&port.bodies, r8).expect("joint body 2"));
            Expect::Joint(port.core.create_distance_joint(b1, b2, v3(r9), v3(stack(0))))
        }
        (0x92fd20, _) => {
            let (b1, b2) = (body_of(&port.bodies, rdx).expect("joint body 1"), body_of(&port.bodies, r8).expect("joint body 2"));
            Expect::Joint(port.core.create_fixed_joint(b1, b2))
        }
        (0x92fbb0, _) => {
            port.core.reseat(port.joints[&rdx], v3(r8), v3(r9));
            Expect::Nothing
        }
        (0x928310, _) => {
            port.core.set_erp_cfm(port.joints[&rcx], xmm(1), xmm(2));
            Expect::Nothing
        }
        (0x1688ab0, _) => {
            // "Prepare spaces" on the car container (the track's is never dirty)
            let v6 = std::ptr::read_unaligned((std::ptr::read_unaligned((dll + 0x281_7690) as *const usize) + 16) as *const usize);
            if rcx != std::ptr::read_unaligned((v6 + 8) as *const usize) {
                *port.ignored.entry(rva).or_default() += 1;
                return Expect::Nothing;
            }
            if let Err(e) = port.core.prepare() {
                port.broken = Some(format!("prepare: {}", e.0));
            }
            Expect::Nothing
        }
        (0x92e4a0, _) => {
            port.worlds += 1;
            track_in(port, dll, rcx);
            if port.worlds == 1 && port.broken.is_none() {
                let c = &mut port.core.col;
                c.switches = crate::joints::switches(dll, &[]);
                // the table CSP's first prepare built, before the port knew the levels; no car
                // moved since
                let aabbs: Vec<[f64; 6]> = c.cars.iter().map(|&s| c.geoms[s].aabb).collect();
                c.cars_table = rustyac_ode::csp_space::HashSpace::build(&aabbs, c.cars_levels.0, c.cars_levels.1);
                (c.cars_built, c.cars_dirty) = (true, false);
            }
            compare(port, rcx, step);
            if *((dll + 0x280_3210) as *const u8) != 0 || *((dll + 0x27f_92b6) as *const u8) != 0 {
                // the late world step's byte: this call does nothing
                Expect::Nothing
            } else {
                let r32 = |o: usize| std::ptr::read_unaligned((rcx + o) as *const u32);
                // the step's parts the car's calls never fill (deferred batches, queued contacts,
                // joints to release, the 12960 list, threads, the threaded world step)
                assert_eq!((*((rcx + 12940) as *const u8), r32(16 + 12768), r32(72)), (0, 0, 0), "deferred batches, queued contacts or skipped collision");
                let empty = |a: usize| std::ptr::read_unaligned((rcx + a) as *const usize) == std::ptr::read_unaligned((rcx + a + 8) as *const usize);
                assert!(empty(12856) && empty(12960), "joints to release or the 12960 list");
                assert!(*((dll + 0x281_7680) as *const i32) <= 0, "threads");
                assert!(*((dll + 0x280_3222) as *const u8) == 0 || *((dll + 0x267_b338) as *const u32) == 0, "the threaded world step");
                // the port steps first, so that its contact joints (record 6) come before CSP's
                // record 4 of the step
                match port.core.step(xmm(1)) {
                    Ok(joints) => {
                        let r = |a: usize| std::ptr::read_unaligned(a as *const usize);
                        let mut bodies = Vec::new();
                        let mut p = r(r(rcx + 16) + 0x28);
                        while p != 0 {
                            bodies.push(p);
                            p = r(p + 0x10);
                        }
                        crate::collision::record_joints(step, &joints, &bodies);
                    }
                    Err(e) => port.broken = Some(format!("step: {}", e.0)),
                }
                Expect::Nothing
            }
        }
        (0x92fe80, _) => Expect::Raycaster(xmm(1)),
        (0x92b970 | 0x92fac0, _) => {
            assert_eq!(std::ptr::read_unaligned((dll + 0x281_7688) as *const usize), 0, "a raycast hook (+0x2817688)");
            // the core (+0x92e4a0's rcx) is the global +0x2817690 less 16
            track_in(port, dll, std::ptr::read_unaligned((dll + 0x281_7690) as *const usize) - 16);
            let (length, tyre) = if rva == 0x92b970 { (port.raycasters.get(&rcx).copied(), true) } else { (Some(f32::from_bits(stack(0) as u32)), false) };
            let setup = step == crate::SETUP;
            match length.ok_or(Unhandled("a raycaster made before the port")).and_then(|l| port.core.cast(v3(r8), v3(r9), l, tyre)) {
                Ok(ray) => Expect::Ray(rdx, ray, setup),
                Err(e) => {
                    port.broken = Some(format!("ray: {}", e.0));
                    Expect::Nothing
                }
            }
        }
        (rva, Some(b)) => {
            let c = &mut port.core;
            match rva {
                0x937780 => c.set_mass_box(b, xmm(1), xmm(2), xmm(3), f32::from_bits(stack(0) as u32)),
                0x937460 => c.set_position(b, v3(rdx)),
                0x937050 => c.set_rotation(b, &f32s(rdx, 16).try_into().unwrap()),
                0x937910 => c.stop(b),
                0x936ea0 => c.add_local_force_at_local_pos(b, v3(rdx), v3(r8)),
                0x936f00 => c.add_force_at_local_pos(b, v3(rdx), v3(r8)),
                0x936fc0 => c.add_force_at_pos(b, v3(rdx), v3(r8)),
                0x934100 => c.add_torque(b, v3(rdx)),
                0x933710 => c.set_user_bits(b, rdx as u8, r8 as u8 != 0),
                0x933750 => c.set_mesh_collide(b, rdx as u32 as usize, r8 as u32),
                _ => {}
            }
            let setup = step == crate::SETUP;
            let out = |v: &[f32]| Expect::Out(rdx, v.to_vec(), setup);
            match rva {
                0x9348b0 => out(&c.position(b)),
                0x934e80 => out(&c.velocity(b)),
                0x934870 => out(&c.angular_velocity(b)),
                0x9346d0 => out(&c.local_velocity(b)),
                0x9347a0 => out(&c.local_angular_velocity(b)),
                0x937620 => {
                    // the wrapper's offset (byte +156) is never set by the calls seen
                    assert_eq!(*((rcx + 156) as *const u8), 0, "a body with an offset");
                    out(&c.world_matrix(b))
                }
                0x937700 => out(&c.local_inertia(b)),
                0x934ec0 => out(&c.world_to_local_normal(b, v3(r8))),
                0x937520 => out(&c.world_to_local(b, v3(r8))),
                0x9375a0 => out(&c.local_to_world(b, v3(r8))),
                0x933e30 => out(&c.point_velocity(b, v3(r8))),
                0x936db0 => out(&c.local_point_velocity(b, v3(r8))),
                0x9378d0 => Expect::Float(c.mass(b), setup),
                0x933cc0 => {
                    let (mut pos, mut size, mut min_height) = (v3(rdx), v3(r8), *((dll + 0x27f_9950) as *const f32));
                    if let Some((car, use_it)) = loader_car(&mut port.car) {
                        let key = stack(1) as u32;
                        let i = *port.car_boxes.entry(key).and_modify(|i| *i += 1).or_insert(0);
                        let (lp, ls) = car.boxes.get(i).map_or(([f32::NAN; 3], [f32::NAN; 3]), |d| ([d.centre.x, d.centre.y, d.centre.z], [d.size.x, d.size.y, d.size.z]));
                        let bits = |v: &[f32]| v.iter().map(|x| x.to_bits()).collect::<Vec<_>>();
                        port.car_count[0] += 1;
                        if bits(&pos) != bits(&lp) || bits(&size) != bits(&ls) || min_height.to_bits() != car.min_height.to_bits() {
                            port.car_count[1] += 1;
                            port.log += &format!("loader box {i} key {key}:
  csp    {pos:?} {size:?} floor {min_height}
  loader {lp:?} {ls:?} floor {}
", car.min_height);
                        }
                        if use_it {
                            (pos, size, min_height) = (lp, ls, car.min_height);
                        }
                    }
                    let c = &mut port.core;
                    match c.add_box_collider(b, pos, size, r9 as u32, stack(0) as u32, stack(1) as u32, min_height, 0) {
                        Ok(g) => Expect::Geom(g),
                        Err(e) => {
                            port.broken = Some(format!("addBoxCollider: {}", e.0));
                            Expect::Nothing
                        }
                    }
                }
                0x9337b0 => {
                    assert!(stack(0) as u32 != 0 && *((dll + 0x280_320d) as *const u8) == 0, "a mesh collider removed or convex: not ported");
                    Expect::Mesh(b, rcx, f32s(stack(1), 16).try_into().unwrap(), stack(2) as u32, stack(3) as u32, stack(4) as u32)
                }
                0x937780 | 0x937460 | 0x937050 | 0x937910 | 0x936ea0 | 0x936f00 | 0x936fc0 | 0x934100 | 0x933710 | 0x933750 => Expect::Nothing,
                _ => {
                    *port.ignored.entry(rva).or_default() += 1;
                    return Expect::Nothing;
                }
            }
        }
        _ => {
            *port.ignored.entry(rva).or_default() += 1;
            return Expect::Nothing;
        }
    };
    port.calls.entry(rva).or_default().0 += 1;
    expect
}

/// The call's return: a creator's pointer mapped, an answer checked.
pub(crate) unsafe fn leave(rva: usize, rax: usize, xmm0: usize, expect: Expect) {
    let mut guard = PORT.lock().unwrap();
    let Some(port) = guard.as_mut() else { return };
    let (ok, setup) = match expect {
        Expect::Nothing => return,
        Expect::Geom(g) => {
            port.core.col.geoms[g].id = rax;
            return;
        }
        Expect::Mesh(b, w, m, category, collide, key) => {
            let r = |a: usize| std::ptr::read_unaligned(a as *const usize);
            // the wrapper's last mesh collider (+48 vector of 16-byte entries): its geom at +16
            let geom = r(r(r(w + 56) - 16) + 16);
            match crate::collision::port_mesh(crate::DLL_BASE.load(std::sync::atomic::Ordering::Relaxed), geom) {
                Ok((mut mesh, mut shape)) => {
                    let mut m = m;
                    if let Some((car, use_it)) = loader_car(&mut port.car) {
                        port.car_count[2] += 1;
                        let l = car.mesh.as_ref().expect("the loader finds the collider mesh");
                        let bits = |v: &[f32]| v.iter().map(|x| x.to_bits()).collect::<Vec<_>>();
                        let same_mesh = mesh.as_ref().is_some_and(|a| a.mesh == l.mesh.mesh && a.model.nodes == l.mesh.model.nodes && a.model.single_node == l.mesh.model.single_node && a.model.built == l.mesh.model.built);
                        let (sa, sb) = (format!("{shape:?}"), format!("{:?}", l.shape));
                        if !same_mesh || sa != sb || bits(&m) != bits(&l.matrix) {
                            port.car_count[3] += 1;
                            let d = |t: &rustyac_ode::csp_collision::TrackMesh| format!("{} tris {} verts {} nodes", t.mesh.nb_tris, t.mesh.vertices.len(), t.model.nodes.len());
                            if let Some(a) = &mesh {
                                let dv: Vec<String> = a.mesh.vertices.iter().zip(&l.mesh.mesh.vertices).enumerate().filter(|(_, (x, y))| x != y).map(|(i, (x, y))| format!("{i}: {x:?} {y:?}")).collect();
                                port.log += &format!("loader mesh collider vertices: {} differ, indices same {}; {}
", dv.len(), a.mesh.indices == l.mesh.mesh.indices, dv.join("; "));
                            }
                            port.log += &format!("loader mesh collider (same data {same_mesh}):
  csp    {} {sa} {m:?}
  loader {} {sb} {:?}
", mesh.as_ref().map_or("none".into(), d), d(&l.mesh), l.matrix);
                        }
                        if use_it {
                            (mesh, shape, m) = (Some(rustyac_ode::csp_collision::TrackMesh { mesh: l.mesh.mesh.clone(), model: l.mesh.model.clone() }), l.shape.clone(), l.matrix);
                        }
                    }
                    port.core.add_mesh_collider(b, &m, category, collide, key, mesh, shape, geom);
                }
                Err(e) => port.broken = Some(format!("mesh collider: {}", e.0)),
            }
            return;
        }
        Expect::Body(b) => {
            port.bodies.insert(rax, b);
            return;
        }
        Expect::Raycaster(length) => {
            port.raycasters.insert(rax, length);
            return;
        }
        Expect::Ray(out, want, setup) => {
            let r = |a: usize| std::ptr::read_unaligned(a as *const usize);
            let (mesh, hit) = (r(out + 24), *((out + 32) as *const u8));
            let list = crate::rays::list(crate::DLL_BASE.load(std::sync::atomic::Ordering::Relaxed));
            let named = (mesh != 0).then(|| list.iter().position(|&m| m == mesh).unwrap_or(usize::MAX));
            let got = f32s(out, 6);
            let ok = match want {
                None => hit == 0 && mesh == 0,
                Some(w) => (hit != 0) == w.mesh.is_some() && named == w.mesh && got.iter().zip(w.point.iter().chain(&w.normal)).all(|(a, b)| a.to_bits() == b.to_bits()),
            };
            if !ok && port.wrong[0] + port.wrong[1] < 8 {
                port.log.push_str(&format!("  {rva:#x}: csp {got:?} mesh {named:?} hit {hit} port {want:?}\n"));
            }
            (ok, setup)
        }
        Expect::Joint(j) => {
            port.joints.insert(rax, j);
            return;
        }
        Expect::Out(p, want, setup) => {
            let got = f32s(p, want.len());
            let ok = got.iter().zip(&want).all(|(a, b)| a.to_bits() == b.to_bits());
            if !ok && port.wrong[0] + port.wrong[1] < 8 {
                port.log.push_str(&format!("  {rva:#x}: csp {got:?} port {want:?}\n"));
            }
            (ok, setup)
        }
        Expect::Float(want, setup) => (xmm0 as u32 == want.to_bits(), setup),
    };
    let k = usize::from(!setup);
    port.answers[k] += 1;
    if !ok {
        port.wrong[k] += 1;
        port.calls.entry(rva).or_default().1 += 1;
    }
}

/// The port's world against CSP's (`dxWorld` at core +16) at a world step's entry: every body and
/// joint in list order and each body's joint nodes.
unsafe fn compare(port: &mut Port, core: usize, step: usize) {
    let r = |a: usize| std::ptr::read_unaligned(a as *const usize);
    let world = r(core + 16);
    let list = |mut p: usize| {
        let mut v = Vec::new();
        while p != 0 {
            v.push(p);
            p = r(p + 0x10);
        }
        v
    };
    let words = |p: usize, n: usize| (0..n / 8).map(|i| r(p + 8 * i) as u64).collect::<Vec<u64>>();
    let (bodies, joints) = (list(r(world + 0x28)), list(r(world + 0x30)));
    let index = |p: u64| bodies.iter().position(|&b| b as u64 == p);
    let w = &port.core.world;
    let mut bad = Vec::new();
    if bodies.len() != w.bodies.len() || joints.len() != w.joints.len() {
        bad.push(format!("{} bodies {} joints, port {} {}", bodies.len(), joints.len(), w.bodies.len(), w.joints.len()));
    }
    for (i, (&b, pb)) in bodies.iter().zip(&w.bodies).enumerate() {
        let mut cb = crate::island::body(&words(b, 0x300));
        if cb.flags & 16 != 0 {
            let samples = cb.adis.average_samples as usize;
            let buffer = |p: usize| (0..samples).map(|s| std::array::from_fn(|k| f64::from_bits(r(p + 32 * s + 8 * k) as u64))).collect();
            (cb.average_lvel_buffer, cb.average_avel_buffer) = (buffer(r(b + 0x2a8)), buffer(r(b + 0x2b0)));
        } else {
            // never read with auto-disable off (CSP leaves them as malloc gave them)
            cb.average_lvel_buffer.clone_from(&pb.average_lvel_buffer);
            cb.average_avel_buffer.clone_from(&pb.average_avel_buffer);
        }
        if format!("{cb:?}") != format!("{pb:?}") {
            bad.push(format!("body {i}:\n    csp  {cb:?}\n    port {pb:?}"));
        }
        let mut nodes = Vec::new();
        let mut n = r(b + 0x30);
        while n != 0 {
            nodes.push((joints.iter().position(|&j| j == r(n)).expect("a node's joint in the list"), index(r(n + 8) as u64)));
            n = r(n + 0x10);
        }
        if w.nodes.get(i) != Some(&nodes) {
            bad.push(format!("body {i} nodes: csp {nodes:?} port {:?}", w.nodes.get(i)));
        }
    }
    for (i, (&j, pj)) in joints.iter().zip(&w.joints).enumerate() {
        let words = words(j, crate::island::JOINT);
        let mut cj = crate::island::joint_of(&words, &index);
        // a vector's 4th double is padding CSP never writes
        match &mut cj.kind {
            JointKind::DBall { anchor1, anchor2, .. } => (anchor1[3], anchor2[3]) = (0.0, 0.0),
            JointKind::Fixed { offset, .. } => offset[3] = 0.0,
            _ => {}
        }
        let (flags, tag) = (words[0x30 / 8] as u32, words[0x20 / 8] as u32 as i32);
        let a = format!("{:?} {:?} {:?} {flags} {tag}", cj.kind, cj.b0, cj.b1);
        let b = format!("{:?} {:?} {:?} {} {}", pj.joint.kind, pj.joint.b0, pj.joint.b1, pj.flags, pj.tag);
        if a != b {
            bad.push(format!("joint {i}:\n    csp  {a}\n    port {b}"));
        }
    }
    if port.track {
        let dll = crate::DLL_BASE.load(std::sync::atomic::Ordering::Relaxed);
        match crate::collision::port_cars(dll, core) {
            Ok((csp, pairs, car_pairs, static_step, load)) => {
                let c = &port.core.col;
                let tc = |g: usize| match &c.geoms[g].shape {
                    rustyac_ode::csp_collision::Shape::Box { tc, .. } => crate::btl::tc_bits(tc),
                    _ => Vec::new(),
                };
                let mine: Vec<String> = c.cars.iter().map(|&s| crate::collision::describe(&c.geoms, s, &tc)).collect();
                if csp.len() != mine.len() {
                    bad.push(format!("{} car spaces, port {}", csp.len(), mine.len()));
                }
                for (i, (a, b)) in csp.iter().zip(&mine).enumerate() {
                    if a != b {
                        bad.push(format!("car space {i}:
    csp  {a}
    port {b}"));
                    }
                }
                let name = |g: usize| if matches!(c.geoms[g].shape, rustyac_ode::csp_collision::Shape::Space { .. }) { 0 } else { c.geoms[g].id };
                let theirs: Vec<(usize, usize)> = pairs.iter().map(|&(a, b)| (crate::collision::space_or(a), crate::collision::space_or(b))).collect();
                let ours: Vec<(usize, usize)> = c.pairs.iter().map(|&(a, b)| (name(a), name(b))).collect();
                if theirs != ours || static_step != c.static_step {
                    bad.push(format!("pairs / static step: csp {theirs:x?} {static_step} port {ours:x?} {}", c.static_step));
                }
                let ours: Vec<(usize, usize)> = c.car_pairs.iter().map(|&(a, b)| (name(a), name(b))).collect();
                if car_pairs != ours {
                    bad.push(format!("car pairs: csp {car_pairs:x?} port {ours:x?}"));
                }
                if load != c.load && !(load.avg.is_nan() && c.load.avg.is_nan()) {
                    bad.push(format!("contact load: csp {load:?} port {:?}", c.load));
                }
            }
            Err(e) => bad.push(format!("car spaces: {}", e.0)),
        }
    }
    port.differ += bad.len();
    if !bad.is_empty() && port.log.len() < 20000 {
        port.log.push_str(&format!("  world step {} (step {step}): {} differ\n", port.worlds, bad.len()));
        bad.iter().take(6).for_each(|x| port.log.push_str(&format!("  {x}\n")));
        bad.iter().skip(6).for_each(|x| port.log.push_str(&format!("  {}\n", x.lines().next().unwrap_or(""))));
    }
}

/// "port: <n> world steps compared, <d> differ; <a> answers, <w> wrong; calls (n/wrong) ...;
/// not ported ...", then the first differences.
pub(crate) fn finish() -> String {
    let guard = PORT.lock().unwrap();
    let Some(p) = guard.as_ref() else { return String::new() };
    let calls: Vec<String> = p.calls.iter().map(|(rva, (n, bad))| format!("{rva:x} {n}/{bad}")).collect();
    let ignored: Vec<String> = p.ignored.iter().map(|(rva, n)| format!("{rva:x} {n}")).collect();
    let broken = p.broken.as_ref().map_or(String::new(), |b| format!("port stopped: {b}
"));
    let car = if p.car.is_some() { format!("loader car: boxes {} ({} differ), mesh colliders {} ({} differ)
", p.car_count[0], p.car_count[1], p.car_count[2], p.car_count[3]) } else { String::new() };
    broken + &car + &format!("port: {} world steps compared, {} differ; answers set-up {} ({} wrong), steps {} ({} wrong); calls (n/wrong) {}; not ported {}\n{}", p.worlds, p.differ, p.answers[0], p.wrong[0], p.answers[1], p.wrong[1], calls.join(" "), ignored.join(" "), p.log)
}
