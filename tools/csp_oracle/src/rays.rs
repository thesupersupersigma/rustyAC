// SPDX-License-Identifier: GPL-3.0-or-later

//! Task C5h: CSP's tyre rays against rustyac-ode's `csp_ray` (Embree 3.13.4 linked here).
//! Break kinds: `e` on `rtcIntersect1` (`+0x18d40f0`): the port intersects the same ray and the
//! whole `RTCRayHit` is compared at the return (`E`); `y` on the raycaster's cast `+0x932900`:
//! the port's cast against what CSP writes to the result (`Y`); `n` on `rtcNewDevice`
//! (`+0x18d4260`): with `CSP_ORACLE_ISA=sse2|avx|avx2` CSP's device gets `max_isa=` too; `x` on
//! the ODE cast `+0x926270`: rustyac-ode's `csp_raycast` against the same space, compared at the
//! return.

use rustyac_ode::csp_collide::Unhandled;
use rustyac_ode::csp_collision::Collision;
use rustyac_ode::csp_core::RayMesh;
use rustyac_ode::csp_ray::{Cast, RayHit, Scene, TraceMesh};
use rustyac_ode::csp_raycast::{self, OdeHit, Ray};
use std::cell::RefCell;
use std::ffi::CString;
use std::sync::Mutex;

struct Rays {
    scene: Scene,
    /// CSP's mesh list when the scene was built
    meshes: Vec<usize>,
    /// intersections (compared, wrong), casts (compared, wrong, left to ODE)
    hits: [usize; 2],
    casts: [usize; 3],
    /// ODE casts (compared, wrong, the port's unhandled with why)
    odes: [usize; 2],
    unhandled: std::collections::BTreeMap<&'static str, usize>,
    /// ray flags seen, with counts
    flags: std::collections::BTreeMap<u32, usize>,
    /// the scene's state at each cast, for where CSP switched to Embree
    ready: Vec<bool>,
    log: String,
}

/// a CSP mesh's vertices, indices and two-sided flag
type MeshData = (Vec<[f32; 3]>, Vec<u16>, bool);

static RAYS: Mutex<Option<Rays>> = Mutex::new(None);
thread_local!(static OPEN: RefCell<Vec<(usize, Pending)>> = const { RefCell::new(Vec::new()) });

enum Pending {
    Hit(usize, RayHit),
    Cast(usize, Cast),
    Ode(usize, Result<Option<OdeHit>, Unhandled>),
}

/// The ray space as the port reads it, with CSP's pointer and child count it was read at.
static SPACE: Mutex<Option<(usize, i32, Collision)>> = Mutex::new(None);

unsafe fn read<T>(a: usize) -> T {
    std::ptr::read_unaligned(a as *const T)
}

/// `max_isa=` from `CSP_ORACLE_ISA`, after CSP's "threads=1".
pub(crate) fn config() -> CString {
    match std::env::var("CSP_ORACLE_ISA") {
        Ok(isa) if !isa.is_empty() => CString::new(format!("threads=1,max_isa={isa}")).unwrap(),
        _ => c"threads=1".into(),
    }
}

/// CSP's mesh list (`+0x2885370`: array, count) and the scene the port builds from it.
pub(crate) unsafe fn list(dll: usize) -> Vec<usize> {
    let l = read::<usize>(dll + 0x288_5370);
    if l == 0 {
        return Vec::new();
    }
    (0..read::<u32>(l + 8) as usize).map(|i| read::<usize>(read::<usize>(l) + 8 * i)).collect()
}

/// A mesh object's block (+48: f32 vertices, count, u16 indices, count) and its flag (+40).
unsafe fn mesh_data(m: usize) -> MeshData {
    let b = read::<usize>(m + 48);
    let (nv, ni) = (read::<u32>(b + 8) as usize, read::<u32>(b + 24) as usize);
    let v = (0..nv).map(|i| read::<[f32; 3]>(read::<usize>(b) + 12 * i)).collect();
    let x = (0..ni).map(|i| read::<u16>(read::<usize>(b + 16) + 2 * i)).collect();
    (v, x, read::<u32>(m + 40) == 1)
}

/// Part 3: the track's ray meshes for the port, in CSP's list order, each named by its ODE geom
/// (+16) among the port's track geoms (`ids`, CSP's pointers).
pub(crate) unsafe fn ray_meshes(dll: usize, ids: &[usize]) -> Result<Vec<RayMesh>, Unhandled> {
    list(dll)
        .into_iter()
        .map(|m| {
            let geom = ids.iter().position(|&g| m != 0 && g == read::<usize>(m + 16)).ok_or(Unhandled("a ray mesh off the track space"))?;
            let (vertices, indices, two_sided) = mesh_data(m);
            Ok(RayMesh { geom, vertices, indices, two_sided })
        })
        .collect()
}

/// Part 3, with the port on: CSP's Embree scene is built on a pool thread from its first ray
/// (+0x9229f0), so which rays find it ready is a race. CSP is pinned to the port's model
/// (`Core::embree_ready`): the state byte (+0x2838668) held at -1 (building) until the port's
/// first tyre ray, so CSP's own build never starts; then the build started and waited for.
unsafe fn pin(dll: usize) {
    static STARTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    let Some(ready) = crate::port::embree_ready() else { return };
    if read::<u8>(dll + 0x280_31ff) != 1 {
        return;
    }
    let state = (dll + 0x283_8668) as *mut i8;
    if !ready {
        if std::ptr::read_volatile(state) == 0 {
            std::ptr::write_volatile(state, -1);
        }
    } else if !STARTED.swap(true, std::sync::atomic::Ordering::Relaxed) {
        std::ptr::write_volatile(state, 0);
        std::mem::transmute::<usize, extern "C" fn() -> usize>(dll + 0x92_29f0)();
        let t = std::time::Instant::now();
        while std::ptr::read_volatile(state) <= 0 {
            assert!(t.elapsed().as_secs() < 120, "CSP's Embree scene not built in 120 s");
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    }
}

unsafe fn rays(dll: usize) -> std::sync::MutexGuard<'static, Option<Rays>> {
    let mut guard = RAYS.lock().unwrap();
    let meshes = list(dll);
    if guard.as_ref().is_some_and(|r| r.meshes != meshes) {
        panic!("CSP's mesh list changed after the first ray ({} meshes, then {})", guard.as_ref().unwrap().meshes.len(), meshes.len());
    }
    if guard.is_none() {
        // each mesh object's block (+48): f32 vertices, count, u16 indices, count; +40 the flag
        let data: Vec<Option<MeshData>> = meshes.iter().map(|&m| (m != 0).then(|| mesh_data(m))).collect();
        let trace: Vec<Option<TraceMesh>> = data.iter().map(|d| d.as_ref().map(|(v, x, f)| TraceMesh { vertices: v, indices: x, two_sided: *f })).collect();
        let robust = read::<u8>(dll + 0x280_3200) != 0;
        let scene = Scene::new(&config(), robust, &trace).unwrap_or_else(|e| panic!("{e}"));
        println!("Embree scene: {} meshes, robust {robust}, {:?}", meshes.len(), config());
        *guard = Some(Rays { scene, meshes, hits: [0; 2], casts: [0; 3], odes: [0; 2], unhandled: Default::default(), flags: Default::default(), ready: Vec::new(), log: String::new() });
    }
    guard
}

/// `rtcIntersect1` entered (`rdx` the context, `r8` the ray): the port's answer kept for the return.
pub(crate) unsafe fn intersect(dll: usize, r8: usize, rsp: usize) {
    // only the physics scene's: inside a cast on this thread (CSP has other Embree scenes)
    if !OPEN.with(|o| o.borrow().iter().any(|x| matches!(x.1, Pending::Cast(..)))) {
        return;
    }
    let rh = read::<RayHit>(r8);
    let mine = rays(dll).as_ref().unwrap().scene.intersect(rh.org, rh.dir, rh.tfar);
    OPEN.with(|o| o.borrow_mut().push((rsp, Pending::Hit(r8, mine))));
}

/// `+0x932900` entered: `rcx` the result, `rdx` origin, `r8` direction, xmm3 the length, the
/// stack's first argument the ODE ray geom (or null).
pub(crate) unsafe fn cast(dll: usize, rcx: usize, rdx: usize, r8: usize, length: f32, ray: usize, rsp: usize) {
    assert!(read::<usize>(dll + 0x281_7688) == 0, "a raycast hook (+0x2817688)");
    assert!(read::<usize>(read::<usize>(dll + 0x281_7690) + 24) == 0, "a dynamic ray space");
    assert!(read::<usize>(dll + 0x282_0270) == 0, "Embree off (+0x2820270)");
    pin(dll);
    // with the port on, CSP's rays are the port's to answer (port.rs)
    if crate::port::on() {
        return;
    }
    // byte +0x28031ff: Embree on, cleared by [PHYSICS_EXPERIMENTS] ODE_BATCH_RAYCAST=1 (+0x13fdd6b)
    let embree = read::<u8>(dll + 0x280_31ff) == 1;
    let below = read::<u8>(dll + 0x281_2c7f) != 0;
    // +0x9229f0's state byte (+0x2838668): -1 while the first call's job still builds the
    // scene on a pool thread, then 1; until then every cast is ODE's
    let ready = embree && read::<i8>(dll + 0x283_8668) > 0;
    let mut guard = rays(dll);
    let r = guard.as_mut().unwrap();
    r.ready.push(ready);
    let mine = if ready { r.scene.cast(read(rdx), read(r8), length, ray != 0, below) } else { Cast::Ode };
    drop(guard);
    OPEN.with(|o| o.borrow_mut().push((rsp, Pending::Cast(rcx, mine))));
}

/// `+0x926270` entered: `ray` the ODE ray geom, `out` the result, `org` and `dir` floats.
pub(crate) unsafe fn ode(dll: usize, ray: usize, out: usize, org: usize, dir: usize, rsp: usize) {
    let space = read::<usize>(read::<usize>(read::<usize>(dll + 0x281_7690) + 16));
    let count = read::<i32>(space + 0xa0);
    let mut guard = SPACE.lock().unwrap();
    if guard.as_ref().is_none_or(|s| (s.0, s.1) != (space, count)) {
        *guard = crate::collision::port_ray_space(dll, space).ok().map(|c| (space, count, c));
    }
    let r = Ray { flags: read(ray + 0xc), category: read(ray + 0x90), collide: read(ray + 0x94), length: read(ray + 0xa0) };
    // the trimeshes' ray callbacks (+0xa0, +0xb0) the port has no answer for
    let mine = match guard.as_ref() {
        None => Err(Unhandled("the ray space")),
        Some(s) if s.2.geoms.iter().any(|g| read::<usize>(g.id + 0xb0) != 0) => Err(Unhandled("a trimesh ray callback")),
        Some(s) => csp_raycast::cast(&s.2, read(org), read(dir), &r),
    };
    drop(guard);
    *rays(dll).as_mut().unwrap().flags.entry(r.flags).or_default() += 1;
    OPEN.with(|o| o.borrow_mut().push((rsp, Pending::Ode(out, mine))));
}

/// A return site (`rsp` after the return): the open call's answer checked.
pub(crate) unsafe fn leave(dll: usize, rsp: usize) {
    let Some(p) = OPEN.with(|o| {
        let mut o = o.borrow_mut();
        let i = o.iter().rposition(|x| x.0 == rsp - 8)?;
        o.truncate(i + 1);
        o.pop().map(|x| x.1)
    }) else {
        return;
    };
    let mut guard = rays(dll);
    let r = guard.as_mut().unwrap();
    match p {
        Pending::Hit(at, mine) => {
            let csp = read::<RayHit>(at);
            // the fields Embree writes: tfar, and on a hit Ng, u, v, primID, geomID
            let bits = |h: &RayHit| {
                let mut v = vec![h.tfar.to_bits(), h.geom_id];
                if h.geom_id != u32::MAX {
                    v.extend(h.ng.map(f32::to_bits));
                    v.extend([h.u.to_bits(), h.v.to_bits(), h.prim_id]);
                }
                v
            };
            r.hits[0] += 1;
            if bits(&csp) != bits(&mine) {
                r.hits[1] += 1;
                if r.log.len() < 8000 {
                    r.log.push_str(&format!("  hit: csp {csp:?}\n       port {mine:?}\n"));
                }
            }
        }
        Pending::Ode(out, mine) => {
            let mine = match mine {
                Ok(m) => m,
                Err(Unhandled(why)) => {
                    *r.unhandled.entry(why).or_default() += 1;
                    return;
                }
            };
            let f = |i: usize| read::<f32>(out + 4 * i).to_bits();
            let csp = ([0, 1, 2].map(f), [3, 4, 5].map(f), read::<usize>(out + 24), read::<u8>(out + 32));
            let port = match mine {
                // the geom's user data (+16), none when its low bit is set
                Some(h) => {
                    let data = SPACE.lock().unwrap().as_ref().map_or(0, |s| read::<usize>(s.2.geoms[h.geom].id + 16));
                    (h.point.map(f32::to_bits), h.normal.map(f32::to_bits), if data & 1 != 0 { 0 } else { data }, 1)
                }
                None => ([0; 3], [0, 1.0f32.to_bits(), 0], 0, 0),
            };
            r.odes[0] += 1;
            if csp != port {
                r.odes[1] += 1;
                if r.log.len() < 8000 {
                    r.log.push_str(&format!("  ode: csp {csp:x?}\n       port {port:x?}\n"));
                }
            }
        }
        Pending::Cast(out, mine) => {
            let (hit, mesh) = (read::<u8>(out + 32), read::<usize>(out + 24));
            let csp = if hit == 0 && mesh == 0 {
                Cast::Miss
            } else {
                let f = |i: usize| read::<f32>(out + 4 * i);
                let index = r.meshes.iter().position(|&m| m == mesh).map_or(u32::MAX, |i| i as u32);
                Cast::Hit { point: [f(0), f(1), f(2)], normal: [f(3), f(4), f(5)], mesh: index }
            };
            let same = match (csp, mine) {
                (_, Cast::Ode) => {
                    r.casts[2] += 1;
                    return;
                }
                (Cast::Hit { point: a, normal: b, mesh: c }, Cast::Hit { point: x, normal: y, mesh: z }) => a.map(f32::to_bits) == x.map(f32::to_bits) && b.map(f32::to_bits) == y.map(f32::to_bits) && c == z,
                (a, b) => a == b,
            };
            r.casts[0] += 1;
            if !same {
                r.casts[1] += 1;
                if r.log.len() < 8000 {
                    r.log.push_str(&format!("  cast: csp {csp:?}\n        port {mine:?}\n"));
                }
            }
        }
    }
}

/// "rays: <n> intersections (<w> wrong), <n> casts (<w> wrong, <o> left to ODE)", then the first
/// differences.
pub(crate) fn finish() -> String {
    let guard = RAYS.lock().unwrap();
    let Some(r) = guard.as_ref() else { return String::new() };
    let switches: Vec<usize> = (1..r.ready.len()).filter(|&i| r.ready[i] != r.ready[i - 1]).collect();
    format!(
        "rays: {} intersections ({} wrong), {} casts ({} wrong, {} left to ODE); Embree ready from cast {:?} of {}, switches at {switches:?}; {} ODE casts ({} wrong, unhandled {:?}), ray flags {:x?}\n{}",
        r.hits[0],
        r.hits[1],
        r.casts[0],
        r.casts[1],
        r.casts[2],
        r.ready.iter().position(|&x| x),
        r.ready.len(),
        r.odes[0],
        r.odes[1],
        r.unhandled,
        r.flags,
        r.log
    )
}
