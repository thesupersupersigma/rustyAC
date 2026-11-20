// SPDX-License-Identifier: GPL-3.0-or-later

//! Break kind `r` on CSP's contact reduction (+0x169f060): every call's contacts and limits are
//! run again through CSP's function and through `rustyac_ode::csp_reduce`, together with made-up
//! inputs (other limits, shuffled, many groups, NaN and denormal values, more than 256 contacts)
//! under both MXCSR settings, and the kept contacts are compared bit for bit.

use crate::read;
use rustyac_ode::csp_btl::ContactGeom;
use rustyac_ode::csp_reduce::{reduce, Reduce};
use std::sync::Mutex;

type Csp = extern "C" fn(usize, i32, usize, usize) -> i32;

#[derive(Default)]
struct State {
    rng: u64,
    hits: u64,
    calls: [u64; 2],
    differ: [u64; 2],
    reduced: u64,
    notes: Vec<String>,
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
        (self.next() >> 11) as f64 / (1u64 << 53) as f64 * 2.0 - 1.0
    }
}

unsafe fn load(buf: usize, n: usize) -> Vec<ContactGeom> {
    (0..n)
        .map(|i| {
            let c = buf + i * 96;
            ContactGeom {
                pos: read(c),
                normal: read(c + 32),
                depth: read(c + 64),
                g1: read(c + 72),
                g2: read(c + 80),
                side1: read(c + 88),
                side2: read(c + 92),
            }
        })
        .collect()
}

fn store(c: &[ContactGeom]) -> Vec<u8> {
    let mut b = vec![0u8; c.len() * 96];
    for (i, c) in c.iter().enumerate() {
        let o = &mut b[i * 96..][..96];
        for k in 0..3 {
            o[k * 8..][..8].copy_from_slice(&c.pos[k].to_le_bytes());
            o[32 + k * 8..][..8].copy_from_slice(&c.normal[k].to_le_bytes());
        }
        o[64..72].copy_from_slice(&c.depth.to_le_bytes());
        o[72..80].copy_from_slice(&c.g1.to_le_bytes());
        o[80..88].copy_from_slice(&c.g2.to_le_bytes());
        o[88..92].copy_from_slice(&c.side1.to_le_bytes());
        o[92..96].copy_from_slice(&c.side2.to_le_bytes());
    }
    b
}

/// Both run `c` with limits `p`; the result is noted when they differ.
fn run(st: &mut State, csp: Csp, c: &[ContactGeom], p: &Reduce, k: usize, what: &str) {
    let mut buf = store(c);
    let mut words = [0u32; 6];
    words[0] = p.per_group as u32;
    words[1] = p.groups as u32;
    words[2] = p.total as u32;
    let thr = p.same_normal.to_bits();
    words[4] = thr as u32;
    words[5] = (thr >> 32) as u32;
    let n = csp(buf.as_mut_ptr() as usize, c.len() as i32, 0, words.as_ptr() as usize) as usize;
    let mut port = c.to_vec();
    let m = reduce(&mut port, p);
    st.calls[k] += 1;
    st.reduced += (c.len() - m.min(c.len())) as u64;
    if n != m || buf[..n * 96] != store(&port)[..] {
        st.differ[k] += 1;
        if st.notes.len() < 20 {
            st.notes.push(format!("{what}: {} in, csp {n} port {m}", c.len()));
        }
    }
}

/// One call caught at the reduction's entry: `buf`/`n` the contacts, `params` the limits.
pub(crate) unsafe fn hit(buf: usize, n: i32, params: usize, at: usize, original: u8, dll: usize, line: &mut String) {
    let mut guard = STATE.lock().unwrap();
    let st = guard.get_or_insert_with(|| State { rng: 0x2545_f491_4f6c_dd1d, ..Default::default() });
    st.hits += 1;
    let hit_no = st.hits;
    crate::set_byte(at, original);
    let csp: Csp = std::mem::transmute(dll + 0x169_f060);
    let live = load(buf, n.clamp(0, 256) as usize);
    let p = if params == 0 {
        rustyac_ode::csp_reduce::DEFAULT
    } else {
        Reduce { per_group: read(params), groups: read(params + 4), total: read(params + 8), same_normal: read(params + 16) }
    };
    line.push_str(&format!(" reduce {n} {} {} {} {}", p.per_group, p.groups, p.total, p.same_normal));
    // tag each contact so a reordering shows
    let live: Vec<_> = live.into_iter().enumerate().map(|(i, mut c)| {
        c.side2 = c.side2.wrapping_add((i as i32) << 20);
        c
    }).collect();

    let mut csr_old = 0u32;
    std::arch::asm!("stmxcsr [{}]", in(reg) &mut csr_old, options(nostack, preserves_flags));
    for (k, csr) in [0x1f80u32, 0x9fc0].into_iter().enumerate() {
        std::arch::asm!("ldmxcsr [{}]", in(reg) &csr, options(nostack, preserves_flags, readonly));
        run(st, csp, &live, &p, k, &format!("hit {hit_no} live mxcsr {csr:#x}"));
        for v in 0..16 {
            let mut c = live.clone();
            let mut q = p;
            match v % 4 {
                0 => {}
                1 => q = Reduce { per_group: (st.next() % 6) as i32 - 1, groups: (st.next() % 11) as i32 - 1, total: (st.next() % 12) as i32 - 1, same_normal: 0.5 + st.unit() * 0.5 },
                2 => q = rustyac_ode::csp_reduce::DEFAULT,
                _ => q.same_normal = [0.0, 1.0, -1.0, f64::NAN, 0.999][(st.next() % 5) as usize],
            }
            // add contacts: many normals (more than 8 groups), up to 300 in all
            let extra = [0usize, 3, 12, 40, 300][(v / 4 + (st.next() % 2) as usize) % 5];
            for e in 0..extra {
                let base = if c.is_empty() { ContactGeom { pos: [0.0; 3], normal: [0.0, 1.0, 0.0], depth: 0.01, g1: 0, g2: 0, side1: 0, side2: 0 } } else { c[(st.next() as usize) % c.len()] };
                let mut x = base;
                x.side2 = ((c.len() + e) as i32) << 20;
                for a in 0..3 {
                    x.pos[a] += st.unit() * 0.3;
                    x.normal[a] += st.unit() * if v % 3 == 0 { 1.0 } else { 0.05 };
                }
                x.depth = (x.depth + st.unit() * 0.02).abs();
                c.push(x);
            }
            // edge values: equal depths, NaN, denormals, zero, -0, inf, random bits
            for _ in 0..(v % 5) {
                if c.is_empty() {
                    break;
                }
                let i = (st.next() as usize) % c.len();
                let bits = st.next();
                let val = [f64::NAN, 0.0, -0.0, 1e-310, -1e-310, f64::INFINITY, f64::from_bits(bits), c[0].depth][(st.next() % 8) as usize];
                match st.next() % 3 {
                    0 => c[i].depth = val,
                    1 => c[i].normal[(st.next() % 3) as usize] = val,
                    _ => c[i].pos[(st.next() % 3) as usize] = val,
                }
            }
            if v % 2 == 1 {
                for i in (1..c.len()).rev() {
                    c.swap(i, (st.next() as usize) % (i + 1));
                }
            }
            run(st, csp, &c, &q, k, &format!("hit {hit_no} #{v} mxcsr {csr:#x} limits {q:?}"));
        }
    }
    std::arch::asm!("ldmxcsr [{}]", in(reg) &csr_old, options(nostack, preserves_flags, readonly));
}

pub(crate) fn finish() -> String {
    let guard = STATE.lock().unwrap();
    let Some(st) = guard.as_ref() else { return String::new() };
    let mut s = format!(
        "reduce: {} hits, mxcsr 0x1f80 {} calls {} differ, mxcsr 0x9fc0 {} calls {} differ, {} contacts dropped\n",
        st.hits, st.calls[0], st.differ[0], st.calls[1], st.differ[1], st.reduced
    );
    st.notes.iter().for_each(|n| s.push_str(&format!("  {n}\n")));
    s
}
