// SPDX-License-Identifier: GPL-3.0-or-later

//! Task C5f: CSP's hash space (`collide2` +0x1689bf0, table build +0x1688ab0) against
//! `rustyac_ode::csp_space` in process. `CSP_ORACLE_BREAK=1689bf0h`: at every entry the oracle
//! calls CSP's `collide2` itself with a recording callback (the live query, which also builds the
//! tables when the space wants it), compares the tables with rustyac-ode's build from the
//! children's boxes, and compares the reported children (in order) with the port's candidates
//! after the same per-pair tests, the trimesh `AABBTest` (+0x1693970) included. Then shadow
//! queries (a copy of the query geom with perturbed, edge and random boxes and bits), and every
//! `CSP_ORACLE_SPACE_EVERY`-th hit (default 10) a made-up space of random children built by CSP
//! and by the port and queried by both, under both MXCSR settings. `CSP_ORACLE_SPACE_N` shadow
//! queries per hit (default 24).

use rustyac_ode::csp_opcode::aabb_test;
use rustyac_ode::csp_space::{overlap, HashSpace};
use rustyac_ode::opcode::AabbNoLeafNode;
use std::collections::HashMap;
use std::sync::Mutex;

unsafe fn read<T: Copy>(address: usize) -> T {
    std::ptr::read_unaligned(address as *const T)
}

unsafe fn write<T: Copy>(address: usize, value: T) {
    std::ptr::write_unaligned(address as *mut T, value)
}

type Callback = extern "C" fn(usize, usize, usize);
type Collide2 = extern "C" fn(usize, usize, usize, Callback);
/// One query by CSP and the port: state, space, its children, the port's tables, the query geom, MXCSR slot, a label.
type Run<'a> = &'a dyn Fn(&mut State, usize, &[usize], &HashSpace, usize, usize, &dyn Fn() -> String);

extern "C" fn record(data: usize, child: usize, _query: usize) {
    unsafe { (*(data as *mut Vec<usize>)).push(child) }
}

#[derive(Default)]
struct State {
    trees: HashMap<usize, Vec<AabbNoLeafNode>>,
    rng: u64,
    hits: u64,
    calls: [u64; 2],
    differ: [u64; 2],
    reported: u64,
    tables: u64,
    tables_differ: u64,
    fakes: u64,
    first: Vec<String>,
    log: String,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);

impl State {
    fn next(&mut self) -> u64 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        self.rng
    }
    fn unit(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 52) as f64 - 1.0
    }
    fn note(&mut self, what: String) {
        if self.first.len() < 8 {
            self.first.push(what);
        }
    }
}

unsafe fn children(space: usize) -> Vec<usize> {
    let mut out = Vec::new();
    let mut g = read::<usize>(space + 0xa8);
    while g != 0 {
        out.push(g);
        g = read::<usize>(g + 0x38);
    }
    out
}

unsafe fn vec_of<T: Copy>(at: usize, size: usize) -> Vec<T> {
    let (begin, end) = (read::<usize>(at), read::<usize>(at + 8));
    (0..(end - begin) / size).map(|i| read::<T>(begin + i * size)).collect()
}

/// CSP's tables against the port's; the first difference, if any.
unsafe fn compare_tables(space: usize, kids: &[usize], port: &HashSpace) -> Option<String> {
    let index = |g: usize| kids.iter().position(|&k| k == g);
    let entries = vec_of::<[u8; 48]>(space + 0xe0, 48);
    if entries.len() != port.entries.len() {
        return Some(format!("entries csp {} port {}", entries.len(), port.entries.len()));
    }
    for (i, (e, p)) in entries.iter().zip(&port.entries).enumerate() {
        let e = e.as_ptr() as usize;
        let mine = (read::<i32>(e), read::<[i32; 6]>(e + 4), index(read::<usize>(e + 0x20)), read::<usize>(e + 0x28));
        if mine != (p.level, p.cells, Some(p.geom), i) {
            return Some(format!("entry {i}: csp {mine:?} port {p:?}"));
        }
    }
    let big: Vec<_> = vec_of::<[u8; 48]>(space + 0xf8, 48).iter().map(|e| index(read::<usize>(e.as_ptr() as usize + 0x20))).collect();
    if big != port.big.iter().map(|&g| Some(g)).collect::<Vec<_>>() {
        return Some(format!("big csp {big:?} port {:?}", port.big));
    }
    let nodes = vec_of::<[u32; 5]>(space + 0x110, 20);
    if nodes.len() < port.nodes.len() {
        return Some(format!("nodes csp {} port {}", nodes.len(), port.nodes.len()));
    }
    for (i, p) in port.nodes.iter().enumerate() {
        let n = nodes[i];
        if n != [p.next, p.entry, p.cell[0] as u32, p.cell[1] as u32, p.cell[2] as u32] {
            return Some(format!("node {i}: csp {n:?} port {p:?}"));
        }
    }
    let scales: Vec<u64> = vec_of::<f64>(space + 0x158, 8).iter().map(|s| s.to_bits()).collect();
    let checks = [
        ("heads", vec_of::<u32>(space + 0x128, 4) == port.heads),
        ("levels", vec_of::<i32>(space + 0x140, 4) == port.levels),
        ("scales", scales == port.scales.iter().map(|s| s.to_bits()).collect::<Vec<_>>()),
        ("mask", read::<u32>(space + 0x170) == port.mask),
        ("order", vec_of::<u32>(space + 0x178, 4).get(..port.order.len()) == Some(&port.order[..])),
        ("ranges", vec_of::<u32>(space + 0x190, 4) == port.ranges),
    ];
    checks.iter().find(|c| !c.1).map(|c| format!("{} differ", c.0))
}

/// The port's answer for `query` against `space`'s children: candidates from `table`, then the
/// tests CSP makes before calling back.
unsafe fn port_query(st: &mut State, dll: usize, table: &HashSpace, kids: &[usize], query: usize) -> Result<Vec<usize>, String> {
    let q_aabb: [f64; 6] = read(query + 0x60);
    let mut out = Vec::new();
    let mut err = None;
    let mut candidates = Vec::new();
    if read::<u32>(query + 0xc) & 0x30 != 0x10 {
        return Ok(out);
    }
    table.collide2(&q_aabb, |g| candidates.push(g));
    for g in candidates {
        let c = kids[g];
        let (flags, body, cat, col) = (read::<u32>(c + 0xc), read::<usize>(c + 0x18), read::<u32>(c + 0x90), read::<u32>(c + 0x94));
        let (q_body, q_cat, q_col) = (read::<usize>(query + 0x18), read::<u32>(query + 0x90), read::<u32>(query + 0x94));
        let a: [f64; 6] = read(c + 0x60);
        if flags & 0x30 != 0x10 || (body == q_body && body != 0) || (cat & q_col == 0 && col & q_cat == 0) || !overlap(&q_aabb, &a) {
            continue;
        }
        let pass = |st: &mut State, g: usize, other: usize, aabb: &[f64; 6]| -> Result<bool, String> {
            let f = read::<usize>(read::<usize>(g) + 0x18);
            if f == dll + 0x16_878a0 {
                return Ok(true);
            }
            if f != dll + 0x169_3970 {
                return Err(format!("AABBTest +{:x} not ported", f - dll));
            }
            let limit = read::<i32>(dll + 0x27f_995c);
            let data = read::<usize>(g + 0xc0);
            if limit == 0 || read::<u32>(other + 0xc) & 0x8_0000 != 0 || read::<u32>(data + 0x50) & 3 != 2 || read::<usize>(data + 0x60) == 0 || read::<u32>(read::<usize>(data + 0x60) + 8) == 0 {
                return Ok(true);
            }
            let posr: [f64; 16] = read(read::<usize>(g + 0x28));
            let same = |x: f64, v: f64| !matches!(x.partial_cmp(&v), Some(std::cmp::Ordering::Less | std::cmp::Ordering::Greater)); // ucomisd + jnz: NaN counts as equal
            let identity = [0usize, 1, 2, 5, 6, 8, 10, 12, 13].iter().all(|&i| same(posr[i], 0.0)) && [4usize, 9, 14].iter().all(|&i| same(posr[i], 1.0));
            if !identity {
                return Ok(true);
            }
            if !st.trees.contains_key(&data) {
                let (_, model) = crate::btl::load_mesh(data, &mut st.log);
                st.trees.insert(data, model.nodes);
            }
            Ok(aabb_test(&st.trees[&data], aabb, limit))
        };
        let first = pass(st, c, query, &q_aabb);
        let both = first.and_then(|ok| if ok { pass(st, query, c, &a) } else { Ok(false) });
        match both {
            Ok(true) => out.push(c),
            Ok(false) => {}
            Err(e) => err = Some(e),
        }
    }
    err.map_or(Ok(out), Err)
}

pub(crate) unsafe fn hit(space: usize, query: usize, at: usize, original: u8, dll: usize, line: &mut String) {
    let mut guard = STATE.lock().unwrap();
    let st = guard.get_or_insert_with(|| State { rng: 0x2545_f491_4f6c_dd1d, ..Default::default() });
    st.hits += 1;
    let hit_no = st.hits;
    crate::set_byte(at, original);
    let csp: Collide2 = std::mem::transmute(dll + 0x168_9bf0);
    let build: extern "C" fn(usize) = std::mem::transmute(dll + 0x168_8ab0);
    let call = |space: usize, query: usize| {
        let mut got: Vec<usize> = Vec::new();
        csp(space, &mut got as *mut Vec<usize> as usize, query, record);
        got
    };

    // one query by both; `kids`/`table` describe the space
    let run = |st: &mut State, space: usize, kids: &[usize], table: &HashSpace, query: usize, k: usize, what: &dyn Fn() -> String| {
        let theirs = call(space, query);
        st.calls[k] += 1;
        st.reported += theirs.len() as u64;
        match port_query(st, dll, table, kids, query) {
            Ok(port) if port == theirs => {}
            port => {
                st.differ[k] += 1;
                let name = |l: &[usize]| l.iter().map(|g| kids.iter().position(|k| k == g).map_or("?".into(), |i| i.to_string())).collect::<Vec<_>>().join(",");
                let aabb: [f64; 6] = read(query + 0x60);
                st.note(format!("{}: aabb {aabb:?} csp [{}] port {:?}", what(), name(&theirs), port.map(|p| name(&p))));
            }
        }
    };

    // the live query (builds CSP's tables if due), then the tables
    let kids = children(space);
    let aabbs: Vec<[f64; 6]> = kids.iter().map(|&g| read(g + 0x60)).collect();
    let table = HashSpace::build(&aabbs, read(space + 0xd0), read(space + 0xd4));
    run(st, space, &kids, &table, query, 0, &|| format!("hit {hit_no} live"));
    st.tables += 1;
    if let Some(d) = compare_tables(space, &kids, &table) {
        st.tables_differ += 1;
        st.note(format!("hit {hit_no} tables: {d}"));
    }
    if hit_no == 1 {
        st.log.push_str(&format!(
            "space {space:x}: {} children, {} with a level, {} big, levels {:?}, {} nodes, {} buckets\n",
            kids.len(),
            table.entries.len(),
            table.big.len(),
            table.levels,
            table.nodes.len(),
            table.heads.len()
        ));
    }

    // shadow queries: a copy of the query geom, its box and bits changed
    let shadow = Box::into_raw(vec![0u8; 0x100].into_boxed_slice()) as *mut u8 as usize;
    std::ptr::copy_nonoverlapping(query as *const u8, shadow as *mut u8, 0x100);
    write(shadow + 0xc, read::<u32>(query + 0xc) & !4);
    let count: usize = std::env::var("CSP_ORACLE_SPACE_N").ok().and_then(|v| v.parse().ok()).unwrap_or(24);
    let every: u64 = std::env::var("CSP_ORACLE_SPACE_EVERY").ok().and_then(|v| v.parse().ok()).unwrap_or(10);
    let edge = crate::ode::edge();
    let live: [f64; 6] = read(query + 0x60);
    let mut csr_old = 0u32;
    std::arch::asm!("stmxcsr [{}]", in(reg) &mut csr_old, options(nostack, preserves_flags));
    for (k, csr) in [0x1f80u32, 0x9fc0].into_iter().enumerate() {
        std::arch::asm!("ldmxcsr [{}]", in(reg) &csr, options(nostack, preserves_flags, readonly));
        for i in 0..count {
            let b = shadow_box(st, i, &live, &aabbs, &edge);
            write(shadow + 0x60, b);
            let bits = st.next();
            let flags = read::<u32>(query + 0xc) & !4;
            write(shadow + 0xc, if i % 5 == 4 { flags ^ (bits as u32 & 0x8_0030) } else { flags });
            write(shadow + 0x90, if i % 7 == 6 { (bits >> 32) as u32 } else { read::<u32>(query + 0x90) });
            write(shadow + 0x94, if i % 7 == 6 { (bits >> 40) as u32 } else { read::<u32>(query + 0x94) });
            run(st, space, &kids, &table, shadow, k, &|| format!("hit {hit_no} shadow #{i} mxcsr {csr:#x}"));
        }
        if hit_no % every == 1 {
            fake_space(st, space, query, build, &run, k, &edge, hit_no);
        }
    }
    std::arch::asm!("ldmxcsr [{}]", in(reg) &csr_old, options(nostack, preserves_flags, readonly));
    drop(Box::from_raw(std::ptr::slice_from_raw_parts_mut(shadow as *mut u8, 0x100)));
    crate::set_byte(at, 0xcc);
    line.push_str(&format!(" | space differ {:?} tables {}", st.differ, st.tables_differ));
}

/// A box for shadow query `i`: the live one, moved, around a child, huge, edge values, cell
/// boundaries, random bits.
fn shadow_box(st: &mut State, i: usize, live: &[f64; 6], aabbs: &[[f64; 6]], edge: &[f64]) -> [f64; 6] {
    let mut b = *live;
    match i % 8 {
        0 => {}
        1 | 2 => {
            let d = [1.0, 50.0][i % 8 - 1];
            for axis in 0..3 {
                let m = st.unit() * d;
                b[2 * axis] += m;
                b[2 * axis + 1] += m;
            }
        }
        3 if !aabbs.is_empty() => {
            let a = aabbs[(st.next() % aabbs.len() as u64) as usize];
            for axis in 0..3 {
                let c = (a[2 * axis] + a[2 * axis + 1]) * 0.5 + st.unit() * (a[2 * axis + 1] - a[2 * axis]);
                let h = 0.01 + st.unit().abs() * 150.0;
                b[2 * axis] = c - h;
                b[2 * axis + 1] = c + h;
            }
        }
        4 => {
            let h = 10f64.powf(4.0 + 2.0 * st.unit().abs());
            (0..3).for_each(|axis| {
                b[2 * axis] -= h;
                b[2 * axis + 1] += h;
            });
        }
        5 => {
            let e = edge[(st.next() % edge.len() as u64) as usize];
            b[(st.next() % 6) as usize] = e;
        }
        6 => {
            let level = (st.next() % 12) as i32;
            for v in b.iter_mut() {
                *v = (*v / 2f64.powi(level)).round() * 2f64.powi(level);
            }
        }
        _ => {
            let bits = st.next();
            b[(st.next() % 6) as usize] = f64::from_bits(bits);
        }
    }
    b
}

/// A made-up space: random children (boxes from tiny to endless, flat at edge values; random flags,
/// bodies, categories), random level limits; built by CSP and by the port, then queried by both.
#[allow(clippy::too_many_arguments)]
unsafe fn fake_space(
    st: &mut State,
    space: usize,
    query: usize,
    build: extern "C" fn(usize),
    run: Run,
    k: usize,
    edge: &[f64],
    hit_no: u64,
) {
    st.fakes += 1;
    let n = (st.next() % 80) as usize;
    let vtable = read::<usize>(query);
    let kids: Vec<usize> = (0..n).map(|_| Box::into_raw(vec![0u8; 0xc0].into_boxed_slice()) as *mut u8 as usize).collect();
    for (i, &g) in kids.iter().enumerate() {
        write(g, vtable);
        write(g + 8, 1u32);
        let r = st.next();
        write(g + 0xc, if r.is_multiple_of(9) { 0x30u32 } else { 0x10 });
        write(g + 0x18, [0usize, 0, 0, 0x1000, 0x2000][(r >> 8) as usize % 5]);
        write(g + 0x90, if r.is_multiple_of(11) { (r >> 16) as u32 & 0xf } else { u32::MAX });
        write(g + 0x94, if r.is_multiple_of(13) { (r >> 24) as u32 & 0xf } else { u32::MAX });
        let mut a = [0f64; 6];
        let size = 2f64.powf(-3.0 + 17.0 * st.unit().abs());
        for axis in 0..3 {
            let c = st.unit() * 2000.0;
            let h = size * (0.2 + st.unit().abs());
            a[2 * axis] = c - h;
            a[2 * axis + 1] = c + h;
        }
        match (r >> 32) % 16 {
            // a flat side at an edge value (upside-down boxes make CSP's node count wrap)
            0 => {
                let axis = (r >> 40) as usize % 3;
                a[2 * axis] = edge[(r >> 44) as usize % edge.len()];
                a[2 * axis + 1] = a[2 * axis];
            }
            2 => a[(r >> 40) as usize % 2] = if r >> 48 & 1 == 0 { -1e301 } else { 1e301 },
            _ => {}
        }
        write(g + 0x60, a);
        write(g + 0x38, kids.get(i + 1).copied().unwrap_or(0));
    }
    let fake = Box::into_raw(vec![0u8; 0x200].into_boxed_slice()) as *mut u8 as usize;
    std::ptr::copy_nonoverlapping(space as *const u8, fake as *mut u8, 0xa0);
    write(fake + 0xa0, n as u32);
    write(fake + 0xa8, kids.first().copied().unwrap_or(0));
    let min = (st.next() % 10) as i32 - 4;
    let max = min + (st.next() % 14) as i32;
    write(fake + 0xd0, min);
    write(fake + 0xd4, max);
    build(fake);
    let aabbs: Vec<[f64; 6]> = kids.iter().map(|&g| read(g + 0x60)).collect();
    let table = HashSpace::build(&aabbs, min, max);
    st.tables += 1;
    if let Some(d) = compare_tables(fake, &kids, &table) {
        st.tables_differ += 1;
        st.note(format!("hit {hit_no} fake space ({n} children, levels {min}..{max}) tables: {d}"));
    }
    let q = Box::into_raw(vec![0u8; 0x100].into_boxed_slice()) as *mut u8 as usize;
    std::ptr::copy_nonoverlapping(query as *const u8, q as *mut u8, 0x100);
    write(q + 0xc, 0x10u32);
    write(q + 0x18, 0x1000usize);
    for i in 0..16 {
        let b = shadow_box(st, i, &aabbs.first().copied().unwrap_or([0.0, 1.0, 0.0, 1.0, 0.0, 1.0]), &aabbs, edge);
        write(q + 0x60, b);
        run(st, fake, &kids, &table, q, k, &|| format!("hit {hit_no} fake space #{i} ({n} children, levels {min}..{max})"));
    }
    // the tables CSP allocated are left behind (a few kilobytes per made-up space)
    drop(Box::from_raw(std::ptr::slice_from_raw_parts_mut(q as *mut u8, 0x100)));
}

/// The summary, at the end of the run (also into breaks.txt).
pub(crate) fn finish() -> String {
    let guard = STATE.lock().unwrap();
    let Some(st) = guard.as_ref() else { return String::new() };
    let mut s = st.log.clone();
    for (k, csr) in ["0x1f80", "0x9fc0"].iter().enumerate() {
        s.push_str(&format!("space mxcsr {csr}: {} queries, {} differ\n", st.calls[k], st.differ[k]));
    }
    s.push_str(&format!(
        "space {} hits, {} children reported, {} table compares ({} made-up spaces), {} differ\n",
        st.hits, st.reported, st.tables, st.fakes, st.tables_differ
    ));
    st.first.iter().for_each(|f| s.push_str(&format!("space first: {f}\n")));
    s
}
