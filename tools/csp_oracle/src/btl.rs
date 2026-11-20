// SPDX-License-Identifier: GPL-3.0-or-later

//! Task C5f: CSP's box-trimesh collider (+0x16b1b90, called (trimesh, box)) against
//! `rustyac_ode::csp_btl` in process. `CSP_ORACLE_BREAK=16b1b90c`: at every entry the oracle calls
//! CSP's collider itself on the live box (its own contact buffer; the cache ends as the game's own
//! call leaves it), then on a shadow box (a copy of the geom with its own pose and cache) at
//! perturbed, edge and random poses under both MXCSR settings, and runs the port on the same
//! inputs with the cache read from CSP's memory. Contacts (count, pos, normal, depth, geoms,
//! sides) and the cache after the call must be the same bits. `CSP_ORACLE_BTL_N` perturbations
//! per hit (default 24).

use rustyac_ode::csp_btl::{collide_btl, ContactGeom, Mesh};
use rustyac_ode::csp_opcode::{BoxTc, ObbCache, TcSlot};
use rustyac_ode::opcode::{AabbNoLeafNode, Child, MeshInterface, Model};
use std::collections::HashMap;
use std::sync::Mutex;

unsafe fn read<T: Copy>(address: usize) -> T {
    std::ptr::read_unaligned(address as *const T)
}

#[derive(Default)]
struct State {
    meshes: HashMap<usize, (MeshInterface, Model)>,
    shadow: usize,
    shadow_posr: usize,
    rng: u64,
    hits: u64,
    calls: [u64; 2],
    differ: [u64; 2],
    nan: [u64; 2],
    contacts: u64,
    tc_hits: u64,
    first: Vec<String>,
    log: String,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);

/// The trimesh's vertices, indices and OPCODE tree from CSP's memory; the tree is also built by
/// rustyac-ode and compared node by node.
pub(crate) unsafe fn load_mesh(data: usize, log: &mut String) -> (MeshInterface, Model) {
    let iface = data + 0x68;
    let (nb_tris, nb_verts) = (read::<u32>(iface) as usize, read::<u32>(iface + 4) as usize);
    let (tri_stride, vert_stride) = (read::<u32>(iface + 8) as usize, read::<u32>(iface + 0xc) as usize);
    let (idx, verts) = (read::<usize>(iface + 0x28), read::<usize>(iface + 0x30));
    assert_eq!((tri_stride, vert_stride, read::<u8>(iface + 0x10)), (6, 12, 1), "a single-layout mesh with 16-bit indices");
    let vertices = (0..nb_verts).map(|i| read::<[f32; 3]>(verts + i * 12)).collect();
    let indices: Vec<u16> = (0..nb_tris * 3).map(|i| read::<u16>(idx + i * 2)).collect();
    let mesh = MeshInterface::new(vertices, indices);
    let model = data + 0x40;
    assert_eq!(read::<u32>(model + 0x10), 2, "a no-leaf, not quantised tree");
    let tree = read::<usize>(model + 0x20);
    let (n, base) = (read::<u32>(tree + 8) as usize, read::<usize>(tree + 0x10));
    let child = |v: u64| if v & 1 != 0 { Child::Triangle((v >> 1) as u32) } else { Child::Node(((v as usize - base) / 0x28) as u32) };
    let nodes: Vec<AabbNoLeafNode> = (0..n)
        .map(|i| {
            let a = base + i * 0x28;
            AabbNoLeafNode { center: read(a), extents: read(a + 0xc), pos: child(read(a + 0x18)), neg: child(read(a + 0x20)) }
        })
        .collect();
    let built = Model::build(&mesh);
    let first = (0..n.max(built.nodes.len())).find(|&i| nodes.get(i) != built.nodes.get(i));
    log.push_str(&format!(
        "btl mesh {data:x}: {nb_tris} triangles, {nb_verts} vertices, {n} nodes; rustyac-ode's tree {} nodes, {}\n",
        built.nodes.len(),
        match first {
            None => "identical".to_string(),
            Some(i) => format!("first differing node {i}: csp {:?} port {:?}", nodes.get(i), built.nodes.get(i)),
        }
    ));
    (mesh, Model { nodes, single_node: false, built: true, invalid_splits: 0 })
}

/// The box's TC block (+0x16a8110's list at box+0x98, kind 0 at +0x10) as rustyac-ode's [`BoxTc`];
/// `model` is the current model pointer (a slot's +0x18 equal to it is a valid cache), 0 for each
/// slot's own trimesh's model.
pub(crate) unsafe fn read_tc(box_: usize, model: usize) -> BoxTc {
    let mut e = read::<usize>(box_ + 0x98);
    while e != 0 && read::<u32>(e + 0x10) != 0 {
        e = read::<usize>(e + 8);
    }
    let mut tc = BoxTc::default();
    if e == 0 {
        return tc;
    }
    for (i, s) in tc.slots.iter_mut().enumerate() {
        let a = e + 0x18 + i * 0x70;
        let (cur, entries) = (read::<u32>(a + 4) as usize, read::<usize>(a + 8));
        let rot: [f32; 9] = read(a + 0x38);
        *s = TcSlot {
            cache: ObbCache {
                touched: (0..cur).map(|k| read::<u32>(entries + k * 4)).collect(),
                valid: read::<usize>(a + 0x18) == if model != 0 { model } else { own_model(read(a + 0x60)) },
                fat_center: read(a + 0x20),
                fat_extents: read(a + 0x2c),
                fat_rot: [[rot[0], rot[1], rot[2]], [rot[3], rot[4], rot[5]], [rot[6], rot[7], rot[8]]],
                fat_coeff: read(a + 0x5c),
            },
            trimesh: read(a + 0x60),
            key: read(a + 0x68),
        };
    }
    tc.next = read(e + 0x1d8);
    tc
}

unsafe fn own_model(trimesh: usize) -> usize {
    if trimesh == 0 {
        return usize::MAX;
    }
    read::<usize>(trimesh + 0xc0) + 0x40
}

/// The cache as bits (NaN payloads included), for comparing.
pub(crate) fn tc_bits(tc: &BoxTc) -> Vec<u64> {
    let mut v = vec![tc.next as u64];
    for s in &tc.slots {
        let c = &s.cache;
        v.extend([s.trimesh as u64, s.key as u64, c.valid as u64, c.fat_coeff.to_bits() as u64, c.touched.len() as u64]);
        v.extend(c.touched.iter().map(|&t| t as u64));
        v.extend(c.fat_center.iter().chain(&c.fat_extents).chain(c.fat_rot.iter().flatten()).map(|f| f.to_bits() as u64));
    }
    v
}

fn bits(c: &ContactGeom) -> [u64; 11] {
    bits_nan(c, false)
}

/// With `any_nan`, every NaN is one value (a NaN's sign and payload are counted apart).
fn bits_nan(c: &ContactGeom, any_nan: bool) -> [u64; 11] {
    let f = |v: f64| if any_nan && v.is_nan() { u64::MAX } else { v.to_bits() };
    [f(c.pos[0]), f(c.pos[1]), f(c.pos[2]), f(c.normal[0]), f(c.normal[1]), f(c.normal[2]), f(c.depth), c.g1 as u64, c.g2 as u64, c.side1 as u64, c.side2 as u64]
}

unsafe fn read_contacts(buf: usize, n: usize) -> Vec<ContactGeom> {
    (0..n)
        .map(|i| {
            let a = buf + i * 96;
            ContactGeom { pos: read(a), normal: read(a + 0x20), depth: read(a + 0x40), g1: read(a + 0x48), g2: read(a + 0x50), side1: read(a + 0x58), side2: read(a + 0x5c) }
        })
        .collect()
}

type Collide = extern "C" fn(usize, usize, u32, *mut u8, i32) -> i32;

/// Breakpoint kind 'c': rcx trimesh, rdx box, r8 flags; `at` holds 0xcc, `original` its byte.
pub(crate) unsafe fn hit(mesh: usize, box_: usize, flags: u32, at: usize, original: u8, dll: usize, line: &mut String) {
    let mut guard = STATE.lock().unwrap();
    let st = guard.get_or_insert_with(|| State { rng: 0x9e37_79b9_7f4a_7c15, ..Default::default() });
    st.hits += 1;
    let data = read::<usize>(mesh + 0xc0);
    let model_ptr = data + 0x40;
    if !st.meshes.contains_key(&mesh) {
        let m = load_mesh(data, &mut st.log);
        st.meshes.insert(mesh, m);
    }
    let tc_on = read::<u8>(mesh + 0xc9) != 0;
    let key = read::<u32>(data + 0xf0);
    assert_eq!((read::<usize>(mesh + 0xa0), read::<usize>(mesh + 0xa8)), (0, 0), "no triangle callbacks");
    let collider = std::mem::transmute::<usize, extern "C" fn() -> usize>(dll + 0x16c_b700)() + 0x4870;
    let full_box_box = read::<u8>(collider + 0x144) != 0;
    if st.hits == 1 {
        st.log.push_str(&format!("btl doBoxTC {tc_on} key {key:x} fullBoxBoxTest {full_box_box}\n"));
    }
    let csp: Collide = std::mem::transmute(dll + 0x16b_1b90);
    let mesh_posr = read::<usize>(mesh + 0x28);
    let mut buf = vec![0u8; 96 * 64];

    // one call of both on the box `b` (posr at `posr`); compares contacts and the cache after
    let mut run = |st: &mut State, b: usize, posr: usize, flags: u32, what: &dyn Fn() -> String, k: usize| {
        let mut tc = read_tc(b, model_ptr);
        let tc_before = tc.clone();
        let port = {
            let (m, md) = &st.meshes[&mesh];
            let pose = Mesh { mesh: m, model: md, pos: read(mesh_posr), r: read(mesh_posr + 0x20) };
            let slot = tc_on.then(|| tc.slot(mesh, key, 1.1));
            collide_btl(&pose, &read(posr), &read(posr + 0x20), &read(b + 0xa0), flags, mesh, b, slot, full_box_box)
        };
        buf.iter_mut().for_each(|x| *x = 0);
        let n = csp(mesh, b, flags, buf.as_mut_ptr(), 96);
        let theirs = read_contacts(buf.as_ptr() as usize, n.max(0) as usize);
        let tc_after = read_tc(b, model_ptr);
        st.calls[k] += 1;
        st.contacts += theirs.len() as u64;
        if tc_on && tc_before.slots.iter().any(|s| s.trimesh == mesh && s.cache.valid) && tc_bits(&tc_after) == tc_bits(&tc_before) {
            st.tc_hits += 1;
        }
        let same = |nan: bool| theirs.len() == port.len() && theirs.iter().zip(&port).all(|(a, b)| bits_nan(a, nan) == bits_nan(b, nan));
        let same_contacts = same(true);
        st.nan[k] += (same_contacts && !same(false)) as u64;
        let same_tc = !tc_on || tc_bits(&tc_after) == tc_bits(&tc);
        if !same_contacts || !same_tc {
            st.differ[k] += 1;
            if st.first.len() < 6 {
                let i = (0..theirs.len().max(port.len())).find(|&i| theirs.get(i).map(bits) != port.get(i).map(bits));
                st.first.push(format!(
                    "{} flags {flags:x}: csp {n} port {}, first differing #{i:?}: csp {:?} port {:?}{}",
                    what(),
                    port.len(),
                    i.and_then(|i| theirs.get(i)),
                    i.and_then(|i| port.get(i)),
                    if !same_tc { format!(" cache csp {:?} port {:?}", tc_after.slots[0], tc.slots[0]) } else { String::new() }
                ));
            }
        }
    };

    // the live box, with CSP's collider callable (the 0xcc taken out meanwhile)
    crate::set_byte(at, original);
    let box_posr = read::<usize>(box_ + 0x28);
    let hit_no = st.hits;
    run(st, box_, box_posr, flags, &|| format!("hit {hit_no} live"), 0);

    // the shadow box: a copy of the geom with its own pose and cache
    if st.shadow == 0 {
        st.shadow = Box::into_raw(vec![0u8; 0x100].into_boxed_slice()) as *mut u8 as usize;
        st.shadow_posr = Box::into_raw(vec![0u8; 0x80].into_boxed_slice()) as *mut u8 as usize;
    }
    let (sh, sp) = (st.shadow, st.shadow_posr);
    let keep = read::<usize>(sh + 0x98);
    std::ptr::copy_nonoverlapping(box_ as *const u8, sh as *mut u8, 0xc0);
    *((sh + 0x98) as *mut usize) = keep;
    *((sh + 0x28) as *mut usize) = sp;
    *((sh + 0x18) as *mut usize) = 0;
    let count: usize = std::env::var("CSP_ORACLE_BTL_N").ok().and_then(|v| v.parse().ok()).unwrap_or(24);
    let edge = crate::ode::edge();
    let live: [f64; 16] = read(box_posr);
    let sides: [f64; 3] = read(box_ + 0xa0);
    let mut csr_old = 0u32;
    std::arch::asm!("stmxcsr [{}]", in(reg) &mut csr_old, options(nostack, preserves_flags));
    for (k, csr) in [0x1f80u32, 0x9fc0].into_iter().enumerate() {
        std::arch::asm!("ldmxcsr [{}]", in(reg) &csr, options(nostack, preserves_flags, readonly));
        for i in 0..count {
            let mut next = || {
                st.rng ^= st.rng << 13;
                st.rng ^= st.rng >> 7;
                st.rng ^= st.rng << 17;
                st.rng
            };
            let unit = |r: u64| (r >> 11) as f64 / (1u64 << 52) as f64 - 1.0;
            let mut p = live;
            let mut s = sides;
            let mut f = flags;
            match i % 8 {
                0 => {} // the live pose again (the cache path)
                1 | 2 => (0..3).for_each(|j| p[j] += unit(next()) * 1e-3),
                3 => {
                    (0..3).for_each(|j| p[j] += unit(next()) * 0.5);
                    (4..16).for_each(|j| p[j] += unit(next()) * 0.05);
                }
                4 => (0..3).for_each(|j| p[j] += unit(next()) * 3.0),
                5 => {
                    let e = edge[(next() % edge.len() as u64) as usize];
                    match next() % 3 {
                        0 => p[(next() % 16) as usize] = e,
                        1 => s[(next() % 3) as usize] = e,
                        _ => (0..3).for_each(|j| p[j] += e * 1e-9),
                    }
                }
                6 => {
                    f = [1, 2, 4, 16, 32, 0x8000_0004][(next() % 6) as usize];
                    (0..3).for_each(|j| p[j] += unit(next()) * 0.2);
                }
                _ => p[(next() % 16) as usize] = f64::from_bits(next()),
            }
            std::ptr::copy_nonoverlapping(p.as_ptr() as *const u8, sp as *mut u8, 0x80);
            std::ptr::copy_nonoverlapping(s.as_ptr() as *const u8, (sh + 0xa0) as *mut u8, 24);
            run(st, sh, sp, f, &|| format!("hit {hit_no} shadow #{i} mxcsr {csr:#x} pose {p:?} sides {s:?}"), k);
        }
    }
    std::arch::asm!("ldmxcsr [{}]", in(reg) &csr_old, options(nostack, preserves_flags, readonly));
    crate::set_byte(at, 0xcc);
    line.push_str(&format!(" | btl differ {:?}", st.differ));
}

/// The summary, at the end of the run (also into breaks.txt).
pub(crate) fn finish() -> String {
    let guard = STATE.lock().unwrap();
    let Some(st) = guard.as_ref() else { return String::new() };
    let mut s = st.log.clone();
    for (k, csr) in ["0x1f80", "0x9fc0"].iter().enumerate() {
        s.push_str(&format!("btl mxcsr {csr}: {} calls, {} differ, {} NaN against NaN
", st.calls[k], st.differ[k], st.nan[k]));
    }
    s.push_str(&format!("btl {} hits, {} contacts, {} cache hits\n", st.hits, st.contacts, st.tc_hits));
    st.first.iter().for_each(|f| s.push_str(&format!("btl first: {f}\n")));
    s
}
