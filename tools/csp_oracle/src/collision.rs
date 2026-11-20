// SPDX-License-Identifier: GPL-3.0-or-later

//! Task C5f part 2: CSP's collision step (+0x92c2b0) against `rustyac_ode::csp_collision` in
//! process. `CSP_ORACLE_BREAK=92c2b0q`: at the step's entry the oracle copies the world's geoms,
//! spaces, bodies and pair list into the port's [`Collision`] and runs `CspWorld::collide`; at the
//! return (a break of its own, kind 'u') it compares the contact joints CSP added (in order, every
//! word the step reads), the pair list, every box's OPCODE cache and every geom's flags, box and
//! pose. With the world recorder on (`CSP_ORACLE_ISLANDS`) the port's joints go to the recording
//! as record 6 (6, step, n, n × (id, 0x200 joint bytes, 0x80 feedback bytes), CSP's contact joint
//! layout), which `--world` steps the world with in place of CSP's.

use rustyac_ode::csp_collide::{NewJoint, Unhandled};
use rustyac_ode::csp_collision::{refresh, Collision, Geom, Shape, TrackMesh, GEOM_AABB_BAD, GEOM_POSR_BAD};
use rustyac_ode::csp_space::HashSpace;
use rustyac_ode::csp_step::WorldParams;
use rustyac_ode::csp_world::CspWorld;
use std::collections::{BTreeMap, HashMap};
use std::sync::Mutex;

unsafe fn read<T: Copy>(address: usize) -> T {
    std::ptr::read_unaligned(address as *const T)
}

/// One step between the entry and the return.
struct Open {
    world: usize,
    head: usize,
    ptrs: Vec<usize>,
    bodies: Vec<usize>,
    col: Collision,
    port: Result<Vec<NewJoint>, Unhandled>,
    step: usize,
}

#[derive(Default)]
struct State {
    steps: [u64; 2],
    compared: u64,
    joints: u64,
    differ: u64,
    unhandled: BTreeMap<&'static str, u64>,
    notes: Vec<String>,
    meshes: Vec<TrackMesh>,
    mesh_of: HashMap<usize, usize>,
    log: String,
    open: HashMap<u32, Open>,
    /// geoms seen at a step's entry with their pose / box out of date, by class
    stale: BTreeMap<(i32, u32), u64>,
    rng: u64,
    /// made-up poses: (cases, differ) per MXCSR setting
    poses: [(u64, u64); 2],
    /// cases where the only differences are a NaN against a NaN of another sign or payload
    pose_nan: [u64; 2],
}

static STATE: Mutex<Option<State>> = Mutex::new(None);

unsafe fn list(head: usize, next: usize) -> Vec<usize> {
    let mut v = Vec::new();
    let mut g = head;
    while g != 0 {
        v.push(g);
        g = read::<usize>(g + next);
    }
    v
}

fn note(st: &mut State, s: String) {
    if st.notes.len() < 20 {
        st.notes.push(s);
    }
}

/// The geom at `g` as the port's [`Geom`]; children of a car space are added first.
unsafe fn geom(st: &mut State, dll: usize, g: usize, body_index: &dyn Fn(usize) -> Option<usize>, geoms: &mut Vec<Geom>, ptrs: &mut Vec<usize>) -> Result<usize, Unhandled> {
    let class = read::<i32>(g + 8);
    let aabb_fn = read::<usize>(read::<usize>(g) + 0x18) - dll;
    let shape = match class {
        1 => {
            if aabb_fn != 0x16_878a0 {
                return Err(Unhandled("a box with its own AABBTest"));
            }
            Shape::Box { sides: read(g + 0xa0), tc: crate::btl::read_tc(g, 0) }
        }
        8 => {
            if aabb_fn != 0x169_3970 || (read::<usize>(g + 0xa0), read::<usize>(g + 0xa8)) != (0, 0) {
                return Err(Unhandled("a trimesh with callbacks"));
            }
            let data = read::<usize>(g + 0xc0);
            // other tree kinds (quantised) are never collided on Spa: not loaded
            let mesh = match st.mesh_of.get(&data) {
                Some(&m) => Some(m),
                None if read::<u32>(data + 0x50) == 2 => {
                    let (mesh, model) = crate::btl::load_mesh(data, &mut st.log);
                    st.meshes.push(TrackMesh { mesh, model });
                    st.mesh_of.insert(data, st.meshes.len() - 1);
                    Some(st.meshes.len() - 1)
                }
                None => None,
            };
            let posr: [f64; 16] = read(read::<usize>(g + 0x28));
            let same = |x: f64, v: f64| !matches!(x.partial_cmp(&v), Some(std::cmp::Ordering::Less | std::cmp::Ordering::Greater));
            let identity = [0usize, 1, 2, 5, 6, 8, 10, 12, 13].iter().all(|&i| same(posr[i], 0.0)) && [4usize, 9, 14].iter().all(|&i| same(posr[i], 1.0));
            let tree = read::<usize>(data + 0x60);
            let tree_test = identity && read::<u32>(data + 0x50) & 3 == 2 && tree != 0 && read::<u32>(tree + 8) != 0;
            let [c, e]: [[f64; 3]; 2] = [read(data + 0xa0), read(data + 0xc0)];
            Shape::Mesh { mesh, tc_on: read::<u8>(g + 0xc9) != 0, key: read(data + 0xf0), tree_test, bounds: [c[0], c[1], c[2], e[0], e[1], e[2]] }
        }
        11 => {
            let mut children = Vec::new();
            for k in list(read(g + 0xa8), 0x38) {
                children.push(geom(st, dll, k, body_index, geoms, ptrs)?);
            }
            Shape::Space { children }
        }
        _ => return Err(Unhandled("a geom class other than box, trimesh, simple space")),
    };
    *st.stale.entry((class, read::<u32>(g + 0xc) & 7)).or_default() += 1;
    let (mut side, body) = crate::joints::side(g);
    side.body = if body != 0 { Some(body_index(body).ok_or(Unhandled("a body not in the world"))?) } else { None };
    let posr = read::<usize>(g + 0x28);
    let offset = read::<usize>(g + 0x30);
    let (pos, r) = if posr != 0 { (read(posr), read(posr + 0x20)) } else { ([0.0; 3], [0.0; 12]) };
    // CSP's geoms are always clean here (placed by code before the step, part 7's): mark the car boxes
    // and car spaces dirty so the port places them from the bodies and the compare after the step checks it
    let dirty = match class {
        1 if body != 0 && offset != 0 => GEOM_POSR_BAD | GEOM_AABB_BAD,
        11 => GEOM_AABB_BAD,
        _ => 0,
    };
    geoms.push(Geom {
        id: g,
        side,
        flags: read::<u32>(g + 0xc) | dirty,
        category: read(g + 0x90),
        collide: read(g + 0x94),
        offset: (offset != 0).then(|| (read(offset), read(offset + 0x20))),
        pos,
        r,
        aabb: read(g + 0x60),
        shape,
    });
    ptrs.push(g);
    Ok(geoms.len() - 1)
}

/// The world's collision state (`w` CSP's world wrapper) as the port's.
unsafe fn state(st: &mut State, dll: usize, w: usize, bodies: &[usize]) -> Result<(Collision, Vec<usize>), Unhandled> {
    if read::<u8>(dll + 0x280_3218 + 5) == 0 {
        return Err(Unhandled("collision v1"));
    }
    if read::<u8>(w + 0x328c) != 0 || read::<u32>(w + 0x48) != 0 {
        return Err(Unhandled("deferred batches or a skipped collision"));
    }
    if read::<u8>(w + 0x3279) != 0 {
        return Err(Unhandled("batched joints"));
    }
    let v11 = read::<usize>(w + 0x20);
    let [track, cars, other, other2] = [0, 8, 0x10, 0x18].map(|o| read::<usize>(v11 + o));
    if other != 0 || other2 != 0 {
        return Err(Unhandled("a third or fourth space"));
    }
    let static_step = read::<u32>(w + 0x4c) != 0;
    if static_step && read::<u32>(w + 0x68) != 0 {
        return Err(Unhandled("car pairs kept"));
    }
    if read::<i32>(track + 8) != 12 || read::<i32>(cars + 8) != 12 {
        return Err(Unhandled("container or track not a hash space"));
    }
    let body_index = |b: usize| bodies.iter().position(|&x| x == b);
    let (mut geoms, mut ptrs) = (Vec::new(), Vec::new());
    let mut car_spaces = Vec::new();
    for s in list(read(cars + 0xa8), 0x38) {
        if read::<i32>(s + 8) != 11 {
            return Err(Unhandled("a car container child not a simple space"));
        }
        car_spaces.push(geom(st, dll, s, &body_index, &mut geoms, &mut ptrs)?);
    }
    let mut track_geoms = Vec::new();
    for t in list(read(track + 0xa8), 0x38) {
        track_geoms.push(geom(st, dll, t, &body_index, &mut geoms, &mut ptrs)?);
    }
    let aabbs: Vec<[f64; 6]> = track_geoms.iter().map(|&t| geoms[t].aabb).collect();
    let table = HashSpace::build(&aabbs, read(track + 0xd0), read(track + 0xd4));
    let index = |p: usize| ptrs.iter().position(|&x| x == p);
    let mut pairs = Vec::new();
    let base = read::<usize>(w + 0x50);
    for i in 0..read::<u32>(w + 0x58) as usize {
        let (a, b) = (read::<usize>(base + 16 * i), read::<usize>(base + 16 * i + 8));
        match (index(a), index(b)) {
            (Some(a), Some(b)) => pairs.push((a, b)),
            _ => return Err(Unhandled("a kept pair of unknown or tagged geoms")),
        }
    }
    let collider = std::mem::transmute::<usize, extern "C" fn() -> usize>(dll + 0x16c_b700)() + 0x4870;
    let col = Collision {
        geoms,
        meshes: std::mem::take(&mut st.meshes),
        cars: car_spaces,
        cars_level: read(cars + 0xb4),
        cars_count: read(cars + 0xa0),
        track: track_geoms,
        track_level: read(track + 0xb4),
        track_count: read(track + 0xa0),
        table,
        pairs,
        static_step,
        cars_dirty: read::<u8>(cars + 217) != 0,
        aabb_limit: read(dll + 0x27f_995c),
        full_box_box: read::<u8>(collider + 0x144) != 0,
        switches: crate::joints::switches(dll, &[]),
    };
    Ok((col, ptrs))
}

/// Breakpoint kind 'q': the collision step's entry, `w` CSP's world wrapper (rcx).
pub(crate) unsafe fn enter(w: usize, dll: usize, thread: u32, step: usize) {
    let mut guard = STATE.lock().unwrap();
    let st = guard.get_or_insert_with(Default::default);
    let ode = read::<usize>(w + 0x10);
    let bodies = list(read(ode + 0x28), 0x10);
    let world = CspWorld {
        params: WorldParams { gravity: [0.0; 3], erp: 0.0, cfm: 0.0, max_vel: 0.0, min_depth: 0.0 },
        bodies: bodies.iter().map(|&b| crate::island::body(&(0..crate::island::BODY / 8).map(|i| read::<u64>(b + 8 * i)).collect::<Vec<_>>())).collect(),
        joints: Vec::new(),
        nodes: Vec::new(),
    };
    st.steps[(read::<u32>(w + 0x4c) == 0) as usize] += 1;
    let (mut col, ptrs, port) = match state(st, dll, w, &bodies) {
        Ok((mut col, ptrs)) => {
            let port = world.collide(&mut col);
            (col, ptrs, port)
        }
        Err(e) => {
            *st.unhandled.entry(e.0).or_default() += 1;
            return;
        }
    };
    st.meshes = std::mem::take(&mut col.meshes);
    if let Some(&s) = col.cars.first() {
        pose_check(st, dll, col.geoms[s].id);
    }
    let head = read::<usize>(ode + 0x30);
    st.open.insert(thread, Open { world: w, head, ptrs, bodies, col, port, step });
}

/// Breakpoint kind 'u': the collision step's return.
pub(crate) unsafe fn leave(thread: u32) {
    let mut guard = STATE.lock().unwrap();
    let Some(st) = guard.as_mut() else { return };
    let Some(o) = st.open.remove(&thread) else { return };
    let port = match &o.port {
        Ok(p) => p,
        Err(e) => {
            *st.unhandled.entry(e.0).or_default() += 1;
            return;
        }
    };
    let ode = read::<usize>(o.world + 0x10);
    let mut added = Vec::new();
    let mut j = read::<usize>(ode + 0x30);
    while j != 0 && j != o.head {
        added.push(j);
        j = read::<usize>(j + 0x10);
    }
    added.reverse();
    st.compared += 1;
    st.joints += added.len() as u64;
    let mut bad = Vec::new();
    if port.len() != added.len() {
        bad.push(format!("{} joints, port {}", added.len(), port.len()));
    }
    for (k, (&j, p)) in added.iter().zip(port).enumerate() {
        if let Some(d) = crate::joints::differs(k, j, p, &o.bodies) {
            bad.push(d);
            break;
        }
    }
    let w = o.world;
    let base = read::<usize>(w + 0x50);
    let theirs: Vec<(usize, usize)> = (0..read::<u32>(w + 0x58) as usize).map(|i| (read(base + 16 * i), read(base + 16 * i + 8))).collect();
    let mine: Vec<(usize, usize)> = o.col.pairs.iter().map(|&(a, b)| (o.ptrs[a], o.ptrs[b])).collect();
    if theirs != mine {
        bad.push(format!("pairs csp {} port {}", theirs.len(), mine.len()));
    }
    for (g, &p) in o.col.geoms.iter().zip(&o.ptrs) {
        let flags = read::<u32>(p + 0xc);
        let what = if flags != g.flags {
            Some(format!("flags {flags:#x} port {:#x}", g.flags))
        } else if flags & 4 == 0 && read::<[u64; 6]>(p + 0x60) != g.aabb.map(f64::to_bits) {
            Some("box".to_string())
        } else if flags & 2 == 0 && read::<usize>(p + 0x28) != 0 && (read::<[u64; 3]>(read::<usize>(p + 0x28)) != g.pos.map(f64::to_bits) || read::<[u64; 12]>(read::<usize>(p + 0x28) + 0x20) != g.r.map(f64::to_bits)) {
            Some("pose".to_string())
        } else if let Shape::Box { tc, .. } = &g.shape {
            (crate::btl::tc_bits(&crate::btl::read_tc(p, 0)) != crate::btl::tc_bits(tc)).then(|| "cache".to_string())
        } else {
            None
        };
        if let Some(what) = what {
            bad.push(format!("geom {p:#x} class {}: {what}", g.side.class));
            break;
        }
    }
    if !bad.is_empty() {
        st.differ += 1;
        let s = format!("step {} {}: {}", o.step, if o.col.static_step { "static" } else { "dynamic" }, bad.join("; "));
        note(st, s);
    }
    if std::env::var_os("CSP_ORACLE_ISLANDS").is_some() {
        let mut w = vec![6, o.step as u64, port.len() as u64];
        for (k, p) in port.iter().enumerate() {
            let mut j = vec![0u64; (crate::island::JOINT + crate::island::FEEDBACK) / 8];
            j[0] = 0x3170;
            j[0x30 / 8] = if p.reverse { 2 } else { 0 };
            j[0x40 / 8] = o.bodies[p.b0] as u64;
            j[0x58 / 8] = p.b1.map_or(0, |b| o.bodies[b]) as u64;
            j[0x68 / 8] = 1;
            for (i, x) in crate::joints::contact_words(p).into_iter().enumerate() {
                j[0xa8 / 8 + i] = x;
            }
            w.push(0x6000_0000 + k as u64);
            w.extend(j);
        }
        crate::island::push_words(&w);
    }
}

/// Poses and boxes from made-up bodies, offsets and sides: CSP's space `computeAABB` (+0x1688460,
/// which refreshes each stale child: +0x1686600 and the box's +0x1699680) on a copy of the car
/// space `space` and of its boxes, against the port's `refresh`, both MXCSR settings.
/// `CSP_ORACLE_POSE_N` cases per step (default 8): live values, small and big noise, edge values,
/// random bits, an empty space.
unsafe fn pose_check(st: &mut State, dll: usize, space: usize) {
    // the boxes only: a car space also holds a trimesh
    let kids: Vec<usize> = list(read(space + 0xa8), 0x38).into_iter().filter(|&k| read::<i32>(k + 8) == 1).collect();
    if kids.is_empty() {
        return;
    }
    let n_cases = std::env::var("CSP_ORACLE_POSE_N").ok().and_then(|v| v.parse().ok()).unwrap_or(8usize);
    if st.rng == 0 {
        st.rng = 0x2545_f491_4f6c_dd1d;
    }
    // shadow objects, made once per step (leaked: a few KB per step)
    let buf = |n: usize| Box::leak(vec![0u8; n].into_boxed_slice()).as_mut_ptr() as usize;
    let fs = buf(0x100);
    std::ptr::copy_nonoverlapping(space as *const u8, fs as *mut u8, 0xd0);
    let fb = buf(crate::island::BODY);
    std::ptr::copy_nonoverlapping(read::<usize>(kids[0] + 0x18) as *const u8, fb as *mut u8, crate::island::BODY);
    let fk: Vec<(usize, usize, usize)> = kids
        .iter()
        .map(|&k| {
            let (g, posr, off) = (buf(0x100), buf(0x80), buf(0x80));
            std::ptr::copy_nonoverlapping(k as *const u8, g as *mut u8, 0xc0);
            std::ptr::copy_nonoverlapping(read::<usize>(k + 0x28) as *const u8, posr as *mut u8, 0x80);
            let o = read::<usize>(k + 0x30);
            if o != 0 {
                std::ptr::copy_nonoverlapping(o as *const u8, off as *mut u8, 0x80);
            }
            (g, posr, off)
        })
        .collect();
    let compute: extern "C" fn(usize) = std::mem::transmute(dll + 0x168_8460);
    let edge = [0.0, -0.0, f64::INFINITY, f64::NEG_INFINITY, f64::NAN, 1e-310, f64::MAX, -1.0, 1e300];
    let mut csr_old = 0u32;
    std::arch::asm!("stmxcsr [{}]", in(reg) &mut csr_old, options(nostack, preserves_flags));
    // the doubles a case may change: body pos and R, each child's offset pos and R and sides
    let mut fields: Vec<usize> = (0..3).map(|i| fb + 0x138 + 8 * i).chain((0..12).map(|i| fb + 0x158 + 8 * i)).collect();
    for &(g, _, off) in &fk {
        fields.extend((0..3).map(|i| off + 8 * i).chain((0..12).map(|i| off + 0x20 + 8 * i)).chain((0..3).map(|i| g + 0xa0 + 8 * i)));
    }
    let originals: Vec<f64> = fields.iter().map(|&a| read::<f64>(a)).collect();
    for case in 0..n_cases {
        let kind = case % 6;
        for (&a, &v) in fields.iter().zip(&originals) {
            let mut next = || {
                st.rng ^= st.rng << 13;
                st.rng ^= st.rng >> 7;
                st.rng ^= st.rng << 17;
                st.rng
            };
            let u = (next() >> 11) as f64 / (1u64 << 53) as f64 - 0.5;
            let x = match kind {
                1 => v + u * 2e-3,
                2 => v + u * 2e3,
                3 if next() % 4 == 0 => edge[(next() % edge.len() as u64) as usize],
                4 => f64::from_bits(next()),
                _ => v,
            };
            std::ptr::write_unaligned(a as *mut f64, x);
        }
        let live = if kind == 5 { &fk[..0] } else { &fk[..] };
        for (k, &(g, posr, off)) in live.iter().enumerate() {
            std::ptr::write_unaligned((g + 0x18) as *mut usize, fb);
            std::ptr::write_unaligned((g + 0x28) as *mut usize, posr);
            std::ptr::write_unaligned((g + 0x30) as *mut usize, off);
            std::ptr::write_unaligned((g + 0x38) as *mut usize, live.get(k + 1).map_or(0, |x| x.0));
        }
        std::ptr::write_unaligned((fs + 0xa8) as *mut usize, live.first().map_or(0, |x| x.0));
        for (m, csr) in [0x1f80u32, 0x9fc0].into_iter().enumerate() {
            for &(g, ..) in live {
                std::ptr::write_unaligned((g + 0xc) as *mut u32, read::<u32>(g + 0xc) | 7);
            }
            // the port's view, before CSP writes
            let bodies = [crate::island::body(&(0..crate::island::BODY / 8).map(|i| read::<u64>(fb + 8 * i)).collect::<Vec<_>>())];
            let side = |class, body| rustyac_ode::csp_collide::Side { class, no_data: true, body, grip: false, special: false };
            let mut geoms: Vec<Geom> = live
                .iter()
                .map(|&(g, posr, off)| Geom {
                    id: g,
                    side: side(1, Some(0)),
                    flags: read(g + 0xc),
                    category: 0,
                    collide: 0,
                    offset: Some((read(off), read(off + 0x20))),
                    pos: read(posr),
                    r: read(posr + 0x20),
                    aabb: read(g + 0x60),
                    shape: Shape::Box { sides: read(g + 0xa0), tc: Default::default() },
                })
                .collect();
            let n = live.len();
            geoms.push(Geom { id: fs, side: side(11, None), flags: 4, category: 0, collide: 0, offset: None, pos: [0.0; 3], r: [0.0; 12], aabb: [0.0; 6], shape: Shape::Space { children: (0..n).collect() } });
            std::arch::asm!("ldmxcsr [{}]", in(reg) &csr, options(nostack, preserves_flags, readonly));
            let ok = refresh(&mut geoms, n, &bodies).is_ok();
            compute(fs);
            std::arch::asm!("ldmxcsr [{}]", in(reg) &csr_old, options(nostack, preserves_flags, readonly));
            let mut bad = (!ok).then(|| "port unhandled".to_string());
            let mut nan_only = false;
            let rows = [0usize, 1, 2, 4, 5, 6, 8, 9, 10];
            for (p, &(g, posr, _)) in geoms.iter().zip(live) {
                let theirs = (read::<u32>(g + 0xc), read::<[u64; 3]>(posr), rows.map(|i| read::<u64>(posr + 0x20 + 8 * i)), read::<[u64; 6]>(g + 0x60));
                let mine = (p.flags, p.pos.map(f64::to_bits), rows.map(|i| p.r[i].to_bits()), p.aabb.map(f64::to_bits));
                if theirs != mine {
                    let nan = |v: &[u64]| v.iter().map(|&x| if f64::from_bits(x).is_nan() { u64::MAX } else { x }).collect::<Vec<_>>();
                    let same = theirs.0 == mine.0 && [(&theirs.1[..], &mine.1[..]), (&theirs.2[..], &mine.2[..]), (&theirs.3[..], &mine.3[..])].iter().all(|(a, b)| nan(a) == nan(b));
                    if same {
                        if !nan_only && st.notes.len() < 3 {
                            st.notes.push(format!("NaN only, case {case}: box {g:x}: csp {theirs:x?} port {mine:x?}"));
                        }
                        nan_only = true;
                    } else if bad.is_none() {
                        bad = Some(format!("box {g:x}: csp {theirs:x?} port {mine:x?}"));
                    }
                }
            }
            let nan6 = |v: [u64; 6]| v.map(|x| if f64::from_bits(x).is_nan() { u64::MAX } else { x });
            if read::<[u64; 6]>(fs + 0x60) != geoms[n].aabb.map(f64::to_bits) && nan6(read(fs + 0x60)) == nan6(geoms[n].aabb.map(f64::to_bits)) {
                nan_only = true;
            } else if read::<[u64; 6]>(fs + 0x60) != geoms[n].aabb.map(f64::to_bits) && bad.is_none() {
                bad = Some(format!("space csp {:?} port {:?}", read::<[f64; 6]>(fs + 0x60), geoms[n].aabb));
            }
            st.poses[m].0 += 1;
            st.pose_nan[m] += (nan_only && bad.is_none()) as u64;
            if let Some(b) = bad {
                st.poses[m].1 += 1;
                note(st, format!("pose case {case} mxcsr {csr:#x}: {b}"));
            }
        }
    }
}

pub(crate) fn finish() -> String {
    let guard = STATE.lock().unwrap();
    let Some(st) = guard.as_ref() else { return String::new() };
    let mut s = format!(
        "collision: {} static + {} dynamic steps, {} compared ({} joints), {} differ, not ported {:?}; (class, flags & 7) at entry {:?}; made-up poses mxcsr 0x1f80 {} cases {} differ ({} NaN-vs-NaN only), 0x9fc0 {} cases {} differ ({})\n",
        st.steps[0], st.steps[1], st.compared, st.joints, st.differ, st.unhandled, st.stale, st.poses[0].0, st.poses[0].1, st.pose_nan[0], st.poses[1].0, st.poses[1].1, st.pose_nan[1]
    );
    s.push_str(&st.log);
    st.notes.iter().for_each(|n| s.push_str(&format!("  {n}\n")));
    s
}

/// Task C5g's port: the track and the world's collision settings as CSP loaded them.
pub(crate) struct PortTrack {
    pub geoms: Vec<Geom>,
    pub meshes: Vec<TrackMesh>,
    pub level: i32,
    pub count: i32,
    pub levels: (i32, i32),
    pub cars_level: i32,
    pub static_step: bool,
    pub aabb_limit: i32,
    pub full_box_box: bool,
}

/// The port's reads keep their own mesh cache.
static PORT_STATE: Mutex<Option<State>> = Mutex::new(None);

/// The track hash space's trimeshes (`w` CSP's world wrapper), with the checks `state` makes.
pub(crate) unsafe fn port_track(dll: usize, w: usize) -> Result<PortTrack, Unhandled> {
    if read::<u8>(dll + 0x280_3218 + 5) == 0 {
        return Err(Unhandled("collision v1"));
    }
    let v11 = read::<usize>(w + 0x20);
    let [track, cars, other, other2] = [0, 8, 0x10, 0x18].map(|o| read::<usize>(v11 + o));
    if other != 0 || other2 != 0 || read::<i32>(track + 8) != 12 || read::<i32>(cars + 8) != 12 {
        return Err(Unhandled("spaces of another shape"));
    }
    if read::<u32>(w + 0x58) != 0 {
        return Err(Unhandled("kept pairs before the first step"));
    }
    let mut st = State::default();
    let (mut geoms, mut ptrs) = (Vec::new(), Vec::new());
    for t in list(read(track + 0xa8), 0x38) {
        geom(&mut st, dll, t, &|_| None, &mut geoms, &mut ptrs)?;
    }
    if geoms.iter().any(|g| g.side.body.is_some() || !matches!(g.shape, Shape::Mesh { .. })) {
        return Err(Unhandled("a track geom on a body or not a trimesh"));
    }
    let collider = std::mem::transmute::<usize, extern "C" fn() -> usize>(dll + 0x16c_b700)() + 0x4870;
    Ok(PortTrack {
        geoms,
        meshes: std::mem::take(&mut st.meshes),
        level: read(track + 0xb4),
        count: read(track + 0xa0),
        levels: (read(track + 0xd0), read(track + 0xd4)),
        cars_level: read(cars + 0xb4),
        static_step: read::<u32>(w + 0x4c) != 0,
        aabb_limit: read(dll + 0x27f_995c),
        full_box_box: read::<u8>(collider + 0x144) != 0,
    })
}

/// A geom tree as text for the port's compare: every field the collision reads, children in
/// list order (a space's own pointer left out), a box's OPCODE cache.
pub(crate) fn describe(geoms: &[Geom], g: usize, tc: &dyn Fn(usize) -> Vec<u64>) -> String {
    let x = &geoms[g];
    let head = format!("flags {:#x} cat {:#x} col {:#x} aabb {:?} side {:?}", x.flags, x.category, x.collide, x.aabb, x.side);
    let pose = format!("offset {:?} pos {:?} r {:?}", x.offset, x.pos, x.r);
    match &x.shape {
        Shape::Space { children } => format!("space {head} [{}]", children.iter().map(|&k| describe(geoms, k, tc)).collect::<Vec<_>>().join("; ")),
        Shape::Box { sides, .. } => format!("box {:#x} {head} {pose} sides {sides:?} tc {:?}", x.id, tc(g)),
        Shape::Mesh { tc_on, key, tree_test, bounds, .. } => format!("mesh {:#x} {head} {pose} tc {tc_on} key {key:#x} tree {tree_test} bounds {bounds:?}", x.id),
    }
}

/// The car spaces described, the kept pairs, the static step's flag.
type Cars = (Vec<String>, Vec<(usize, usize)>, bool);

/// CSP's car spaces (`w` the world wrapper) as [`Cars`].
pub(crate) unsafe fn port_cars(dll: usize, w: usize) -> Result<Cars, Unhandled> {
    let mut guard = PORT_STATE.lock().unwrap();
    let st = guard.get_or_insert_with(Default::default);
    let ode = read::<usize>(w + 0x10);
    let bodies = list(read(ode + 0x28), 0x10);
    let body_index = |b: usize| bodies.iter().position(|&x| x == b);
    let cars = read::<usize>(read::<usize>(w + 0x20) + 8);
    let (mut geoms, mut ptrs) = (Vec::new(), Vec::new());
    let mut roots = Vec::new();
    for s in list(read(cars + 0xa8), 0x38) {
        roots.push(geom(st, dll, s, &body_index, &mut geoms, &mut ptrs)?);
    }
    // the flags as they are (`geom` marks the car geoms dirty for C5f's compare)
    for (g, &p) in geoms.iter_mut().zip(&ptrs) {
        g.flags = read::<u32>(p + 0xc);
        if matches!(g.shape, Shape::Space { .. }) {
            g.id = 0;
        }
    }
    let tc = |g: usize| crate::btl::tc_bits(&crate::btl::read_tc(ptrs[g], 0));
    let base = read::<usize>(w + 0x50);
    let pairs = (0..read::<u32>(w + 0x58) as usize).map(|i| (read(base + 16 * i), read(base + 16 * i + 8))).collect();
    Ok((roots.iter().map(|&s| describe(&geoms, s, &tc)).collect(), pairs, read::<u32>(w + 0x4c) != 0))
}

/// The trimesh CSP made at `g` (a car's mesh collider): its mesh and shape.
pub(crate) unsafe fn port_mesh(dll: usize, g: usize) -> Result<(Option<TrackMesh>, Shape), Unhandled> {
    let mut st = State::default();
    let (mut geoms, mut ptrs) = (Vec::new(), Vec::new());
    let i = geom(&mut st, dll, g, &|_| Some(0), &mut geoms, &mut ptrs)?;
    Ok((st.meshes.pop(), geoms.swap_remove(i).shape))
}

/// A geom pointer for a pair compare: a space as 0.
pub(crate) fn space_or(g: usize) -> usize {
    if unsafe { read::<i32>(g + 8) } == 11 { 0 } else { g }
}
