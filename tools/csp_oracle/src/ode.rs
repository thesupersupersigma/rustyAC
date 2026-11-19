// SPDX-License-Identifier: GPL-3.0-or-later

//! Task C5c: rustyac-ode's f64 functions against CSP's own ODE (master 66615b8a, double) in the
//! process. `--ode <breaks.txt>`: the inputs are the values CSP's functions got on a Spa drive
//! (breakpoint dumps), then edge values and random bits. Prints one line per function:
//! "ode <name>: <n> inputs, <d> differ, <nan> NaN against NaN". NaN against NaN counts as equal
//! (docs/port/ode_stage1.md: a NaN's payload depends on operand order the compiler may swap).

use rustyac_ode::mass::Mass;
use rustyac_ode::odemath;
use rustyac_ode::rotation as rot;

type Q = [f64; 4];
type M3 = [f64; 12];

struct Csp {
    q_multiply0: extern "C" fn(*mut f64, *const f64, *const f64),
    q_multiply1: extern "C" fn(*mut f64, *const f64, *const f64),
    q_multiply3: extern "C" fn(*mut f64, *const f64, *const f64),
    r_from_q: extern "C" fn(*mut f64, *const f64),
    q_from_r: extern "C" fn(*mut f64, *const f64),
    dq_from_w: extern "C" fn(*mut f64, *const f64, *const f64),
    box_total: extern "C" fn(*mut f64, f64, f64, f64, f64),
    safe_normalize3: extern "C" fn(*mut f64) -> bool,
}

#[derive(Default)]
struct Count {
    n: u64,
    differ: u64,
    nan: u64,
    first: Vec<String>,
}

impl Count {
    fn check(&mut self, what: &dyn Fn() -> String, csp: &[f64], port: &[f64]) {
        self.n += 1;
        let mut nan = false;
        for (a, b) in csp.iter().zip(port) {
            if a.is_nan() && b.is_nan() {
                nan |= a.to_bits() != b.to_bits();
            } else if a.to_bits() != b.to_bits() {
                self.differ += 1;
                if self.first.len() < 3 {
                    self.first.push(format!("{} csp {csp:?} port {port:?}", what()));
                }
                return;
            }
        }
        self.nan += nan as u64;
    }
}

fn edge() -> Vec<f64> {
    vec![
        0.0, -0.0, 1.0, -1.0, 0.5, -0.5, 2.0, 3.0, 1e-300, -1e-300, 1e300, -1e300, 1e-160, 1e160,
        f64::from_bits(1), -f64::from_bits(1), f64::MIN_POSITIVE, f64::MAX, f64::MIN, f64::INFINITY,
        f64::NEG_INFINITY, f64::NAN, -f64::NAN, f64::EPSILON, std::f64::consts::FRAC_1_SQRT_2,
    ]
}

/// The doubles a breakpoint dump logged for `rva` (`CSP_ORACLE_BREAK=<rva>d`): 12 from field 8 on.
fn dumps(log: &str, rva: &str) -> Vec<Vec<f64>> {
    log.lines()
        .filter_map(|line| {
            let fields: Vec<&str> = line.split_whitespace().collect();
            (fields.len() == 20 && fields[2] == rva).then(|| fields[8..].iter().map(|v| v.parse::<f64>().expect("a dumped double")).collect())
        })
        .collect()
}

pub unsafe fn run(dll: usize, breaks: &str) {
    type F3 = extern "C" fn(*mut f64, *const f64, *const f64);
    type F2 = extern "C" fn(*mut f64, *const f64);
    let f3 = |rva: usize| std::mem::transmute::<usize, F3>(dll + rva);
    let f2 = |rva: usize| std::mem::transmute::<usize, F2>(dll + rva);
    let csp = Csp {
        q_multiply0: f3(0x168_b680),
        q_multiply1: f3(0x168_b7b0),
        q_multiply3: f3(0x168_b8a0),
        r_from_q: f2(0x168_b990),
        q_from_r: f2(0x168_bae0),
        dq_from_w: f3(0x168_bca0),
        box_total: std::mem::transmute::<usize, extern "C" fn(*mut f64, f64, f64, f64, f64)>(dll + 0x168_d670),
        safe_normalize3: std::mem::transmute::<usize, extern "C" fn(*mut f64) -> bool>(dll + 0x168_aed0),
    };
    let log = std::fs::read_to_string(breaks).expect("--ode takes the breaks.txt of a drive");
    let q4 = |v: &Vec<f64>| -> Q { [v[0], v[1], v[2], v[3]] };
    let quats: Vec<Q> = dumps(&log, "0x168b990").iter().chain(&dumps(&log, "0x168b7b0")).map(q4).collect();
    let omegas: Vec<Q> = dumps(&log, "0x168bca0").iter().map(q4).collect();
    assert!(quats.len() >= 600 && omegas.len() >= 600, "breaks.txt has {} quaternions, {} angular velocities", quats.len(), omegas.len());
    println!("ode: {} quaternions, {} angular velocities from {breaks}", quats.len(), omegas.len());

    // the inputs: the drive's, every combination slot by slot from the edge values, random bits,
    // random values in [-1, 1]
    let mut seed = 0x9e37_79b9_7f4a_7c15u64;
    let mut next = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    let e = edge();
    let mut sets: Vec<(&str, Vec<[f64; 8]>)> = Vec::new();
    let drive: Vec<[f64; 8]> = (0..quats.len())
        .map(|i| {
            let (a, b, w) = (quats[i], quats[(i * 7 + 1) % quats.len()], omegas[i % omegas.len()]);
            let pick = if i < omegas.len() { [w[0], w[1], w[2], w[3]] } else { b };
            [a[0], a[1], a[2], a[3], pick[0], pick[1], pick[2], pick[3]]
        })
        .collect();
    sets.push(("drive", drive));
    sets.push(("edge", (0..400_000).map(|_| std::array::from_fn(|_| e[(next() % e.len() as u64) as usize])).collect()));
    sets.push(("bits", (0..400_000).map(|_| std::array::from_fn(|_| f64::from_bits(next()))).collect()));
    sets.push(("unit", (0..400_000).map(|_| std::array::from_fn(|_| (next() >> 11) as f64 / (1u64 << 52) as f64 - 1.0)).collect()));

    for csr in [0x1f80u32, 0x9fc0] {
        std::arch::asm!("ldmxcsr [{}]", in(reg) &csr, options(nostack, preserves_flags, readonly));
        let mut counts: Vec<(&str, Count)> = ["q_multiply0", "q_multiply1", "q_multiply3", "r_from_q", "q_from_r", "dq_from_w", "box_total", "safe_normalize3"].map(|n| (n, Count::default())).into();
        for (set, inputs) in &sets {
            for (i, x) in inputs.iter().enumerate() {
                let what = || format!("{set} #{i} {x:?}");
                let (a, b): (Q, Q) = ([x[0], x[1], x[2], x[3]], [x[4], x[5], x[6], x[7]]);
                let mut out = [0f64; 12];
                for (k, (csp_fn, port)) in [(csp.q_multiply0, rot::q_multiply0::<f64> as fn(&Q, &Q) -> Q), (csp.q_multiply1, rot::q_multiply1), (csp.q_multiply3, rot::q_multiply3)].into_iter().enumerate() {
                    csp_fn(out.as_mut_ptr(), a.as_ptr(), b.as_ptr());
                    counts[k].1.check(&what, &out[..4], &port(&a, &b));
                }
                (csp.r_from_q)(out.as_mut_ptr(), a.as_ptr());
                let mut r: M3 = [0.0; 12];
                rot::r_from_q(&mut r, &a);
                counts[3].1.check(&what, &out, &r);
                // q_from_r on the drive's rotations and on the input's eight values spread over a
                // matrix (diagonal from the first three, so every branch is reached)
                let spread: M3 = [x[0], x[3], x[4], 0.0, x[5], x[1], x[6], 0.0, x[7], x[4] - x[3], x[2], 0.0];
                for m in [r, spread] {
                    (csp.q_from_r)(out.as_mut_ptr(), m.as_ptr());
                    counts[4].1.check(&what, &out[..4], &rot::q_from_r(&m));
                }
                (csp.dq_from_w)(out.as_mut_ptr(), b.as_ptr(), a.as_ptr());
                counts[5].1.check(&what, &out[..4], &rot::dq_from_w(&b, &a));
                let mut mass = [f64::from_bits(0x7ff4_dead_beef_0001); 17];
                (csp.box_total)(mass.as_mut_ptr(), x[0], x[1], x[2], x[3]);
                let m = Mass::<f64>::box_total(x[0], x[1], x[2], x[3]);
                let mut port = [m.mass; 17];
                port[1..5].copy_from_slice(&m.c);
                port[5..].copy_from_slice(&m.i);
                counts[6].1.check(&what, &mass, &port);
                // both halves of the input, the return value as a fourth value
                for v in [[x[0], x[1], x[2]], [x[4], x[5], x[6]]] {
                    let (mut c, mut p) = ([v[0], v[1], v[2], 0.0], [v[0], v[1], v[2], 0.0]);
                    c[3] = (csp.safe_normalize3)(c.as_mut_ptr()) as u8 as f64;
                    p[3] = odemath::safe_normalize3(&mut p) as u8 as f64;
                    counts[7].1.check(&what, &c, &p);
                }
            }
        }
        for (name, c) in &counts {
            println!("ode {name} mxcsr {csr:#06x}: {} inputs, {} differ, {} NaN against NaN {:?}", c.n, c.differ, c.nan, c.first);
        }
    }
}
