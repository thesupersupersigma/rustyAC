// SPDX-License-Identifier: GPL-3.0-or-later

//! Task C5d: rustyac-ode's f64 solver kernels (`csp_matrix`) against CSP's own in the process.
//! `--solver <breaks.txt>`: matrices of every size from 1 to 40 rows, built from the Spa drive's
//! body values (symmetric positive definite, like the solver's), from edge values and from
//! random bits, under both MXCSR values. Prints one line per kernel:
//! "solver <name> mxcsr ..: <n> inputs, <d> differ, <nan> NaN against NaN".

use crate::ode::Count;
use rustyac_ode::csp_matrix as cm;

type Factor = extern "C" fn(*mut f64, *mut f64, u32, u32);
type Solve = extern "C" fn(*const f64, *mut f64, u32, u32);
type Dot = extern "C" fn(*const f64, *const f64, u32) -> f64;
type AddTl = extern "C" fn(*mut f64, *mut f64, *const f64, u32, u32, *mut f64);
type SolveLcp = extern "C" fn(*mut u64, u32, *mut f64, *mut f64, *mut f64, u32, *mut f64, *mut i32);
type Remove = extern "C" fn(*const *const f64, *const u32, *mut f64, *mut f64, u32, u32, u32, u32, *mut f64);

const NAMES: [&str; 10] = ["factor1", "factor2", "solve_l1_1", "solve_l1_2", "solve_l1t_1", "solve_l1t_2", "dot1", "dot2", "ldlt_add_tl", "ldlt_remove"];

/// The row skip dSolveLCP uses, sometimes a wider one.
fn skip(n: usize, extra: u64) -> usize {
    (if n <= 1 { n } else { (n + 3) & !3 }) + 4 * extra.is_multiple_of(3) as usize
}

/// The function at `dll + rva` as `F` (one of the fn pointer types above).
unsafe fn at<F: Copy>(dll: usize, rva: usize) -> F {
    std::mem::transmute_copy::<usize, F>(&(dll + rva))
}

pub unsafe fn run(dll: usize, breaks: &str) {
    let factor: [Factor; 2] = [at(dll, 0x170_0e90), at(dll, 0x170_1f60)];
    let l1: [Solve; 2] = [at(dll, 0x170_1090), at(dll, 0x170_2750)];
    let l1t: [Solve; 2] = [at(dll, 0x170_17b0), at(dll, 0x170_2e80)];
    let dot: [Dot; 2] = [at(dll, 0x16c_b540), at(dll, 0x170_3b80)];
    let add_tl: AddTl = at(dll, 0x168_ded0);
    let remove: Remove = at(dll, 0x168_e5d0);

    // the drive's values: every double of its dxStepBody calls (`CSP_ORACLE_BREAK=16c61b0b`)
    let log = std::fs::read_to_string(breaks).expect("--solver takes the breaks.txt of a drive");
    let pool: Vec<f64> = log
        .lines()
        .filter_map(|line| {
            let f: Vec<&str> = line.split_whitespace().collect();
            (f.len() == 31 && f[2] == "0x16c61b0").then(|| f[10..].iter().map(|v| v.parse::<f64>().expect("a dumped double")).collect::<Vec<_>>())
        })
        .flatten()
        .filter(|v| v.is_finite())
        .collect();
    assert!(pool.len() >= 10_000, "breaks.txt has {} body values", pool.len());
    println!("solver: {} drive values from {breaks}", pool.len());
    let mut seed = 0x9e37_79b9_7f4a_7c15u64;
    let mut next = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    let e = crate::ode::edge();
    const TRIALS: usize = 1500;

    for csr in [0x1f80u32, 0x9fc0] {
        std::arch::asm!("ldmxcsr [{}]", in(reg) &csr, options(nostack, preserves_flags, readonly));
        let mut counts: Vec<Count> = NAMES.iter().map(|_| Count::default()).collect();
        for set in ["drive", "unit", "edge", "bits"] {
            for n in 1..=40usize {
                for trial in 0..TRIALS {
                    let x = next();
                    let rs = skip(n, x);
                    let mut value = |k: u64| -> f64 {
                        match set {
                            "drive" => pool[(next() % pool.len() as u64) as usize],
                            "unit" => (next() >> 11) as f64 / (1u64 << 52) as f64 - 1.0,
                            "edge" => e[(next() % e.len() as u64) as usize],
                            _ => f64::from_bits(next() ^ k),
                        }
                    };
                    // the matrix: B*B^T + n*I for drive and unit (positive definite), raw otherwise
                    let mut a: Vec<f64> = (0..n * rs + 4).map(|k| value(k as u64)).collect();
                    if set == "drive" || set == "unit" {
                        let b = a.clone();
                        for i in 0..n {
                            for j in 0..=i {
                                let s: f64 = (0..n).map(|k| b[i * rs + k] * b[j * rs + k]).sum();
                                a[i * rs + j] = s + if i == j { n as f64 } else { 0.0 };
                                a[j * rs + i] = a[i * rs + j];
                            }
                        }
                    }
                    let what = || format!("{set} n {n} rs {rs} trial {trial}");
                    let rhs: Vec<f64> = (0..2 * n + 2).map(|k| value(k as u64)).collect();
                    // factor, then both solves on the factors (d and b interleaved for stride 2,
                    // like dSolveLCP's pairs)
                    let mut lf = [a.clone(), a.clone()];
                    for s in 0..2 {
                        let stride = s + 1;
                        let (mut ca, mut pa) = (a.clone(), a.clone());
                        let (mut cd, mut pd) = (rhs.clone(), rhs.clone());
                        factor[s](ca.as_mut_ptr(), cd.as_mut_ptr(), n as u32, rs as u32);
                        if s == 0 {
                            cm::factor_ldlt::<1>(&mut pa, &mut pd, n, rs);
                        } else {
                            cm::factor_ldlt::<2>(&mut pa, &mut pd, n, rs);
                        }
                        counts[s].check(&what, &[ca.as_slice(), &cd].concat(), &[pa.as_slice(), &pd].concat());
                        lf[s] = ca;
                        let l = if set == "drive" || set == "unit" { &lf[s] } else { &a };
                        for (k, solve) in [(2, l1[s]), (4, l1t[s])] {
                            let (mut cb, mut pb) = (rhs.clone(), rhs.clone());
                            solve(l.as_ptr(), cb.as_mut_ptr().add(stride - 1), n as u32, rs as u32);
                            let pb1 = &mut pb[stride - 1..];
                            match (k, stride) {
                                (2, 1) => cm::solve_l1::<1>(l, pb1, n, rs),
                                (2, _) => cm::solve_l1::<2>(l, pb1, n, rs),
                                (_, 1) => cm::solve_l1t::<1>(l, pb1, n, rs),
                                _ => cm::solve_l1t::<2>(l, pb1, n, rs),
                            }
                            counts[k + s].check(&what, &cb, &pb);
                        }
                        let c = dot[s](a.as_ptr(), rhs.as_ptr(), n as u32);
                        let p = if s == 0 { cm::dot::<1>(&a, &rhs, n) } else { cm::dot::<2>(&a, &rhs, n) };
                        counts[6 + s].check(&what, &[c], &[p]);
                    }
                    // add_tl on the stride-1 factors (or raw values), a from the right-hand side
                    let l = if set == "drive" || set == "unit" { &lf[0] } else { &a };
                    let d: Vec<f64> = if set == "drive" || set == "unit" { (0..n).map(|i| 1.0 / rhs[i].abs().max(0.5)).collect() } else { rhs[..n].to_vec() };
                    let (mut cl, mut pl, mut cd, mut pd) = (l.clone(), l.clone(), d.clone(), d.clone());
                    let mut w = vec![0.0; 2 * rs];
                    add_tl(cl.as_mut_ptr(), cd.as_mut_ptr(), rhs.as_ptr(), n as u32, rs as u32, w.as_mut_ptr());
                    cm::ldlt_add_tl(&mut pl, 0, &mut pd, &rhs, n, rs, &mut w);
                    counts[8].check(&what, &[cl.as_slice(), &cd].concat(), &[pl.as_slice(), &pd].concat());
                    // ldlt_remove: A is the matrix behind row pointers (rows in a shuffled order),
                    // p a permutation of n2 of its rows, r < n2
                    let n2 = 1 + (x >> 8) as usize % n;
                    let r = (x >> 20) as usize % n2;
                    let mut order: Vec<usize> = (0..n).collect();
                    for i in (1..n).rev() {
                        order.swap(i, (next() % (i as u64 + 1)) as usize);
                    }
                    let arows: Vec<usize> = order.iter().map(|&i| i * rs).collect();
                    let ptrs: Vec<*const f64> = arows.iter().map(|&o| a.as_ptr().add(o)).collect();
                    let p: Vec<u32> = order[..n2].to_vec().iter().rev().map(|&i| i as u32).collect();
                    let (mut cl, mut pl, mut cd, mut pd) = (l.clone(), l.clone(), d.clone(), d.clone());
                    let mut tmp = vec![0.0; 2 * rs + n2];
                    remove(ptrs.as_ptr(), p.as_ptr(), cl.as_mut_ptr(), cd.as_mut_ptr(), n as u32, n2 as u32, r as u32, rs as u32, tmp.as_mut_ptr());
                    cm::ldlt_remove(&a, &arows, &p, &mut pl, &mut pd, n2, r, rs, &mut tmp);
                    counts[9].check(&what, &[cl.as_slice(), &cd].concat(), &[pl.as_slice(), &pd].concat());
                }
            }
        }
        for (name, c) in NAMES.iter().zip(&counts) {
            println!("solver {name} mxcsr {csr:#06x}: {} inputs, {} differ, {} NaN against NaN {:?}", c.n, c.differ, c.nan, c.first);
        }
        lcp(at(dll, 0x16f_fe10), &pool, &mut next, csr);
    }
}

/// `dxSolveLCP` (CSP +0x16ffe10) against `csp_lcp::solve_lcp` on problems shaped like a step's:
/// `A = B*B^T + diag(cfm)` (B with 1 to 2n columns, so often rank deficient), the first `nub`
/// rows unbounded, then a mix of unbounded, contact normal (0, inf), friction (+-mu times an
/// earlier normal's x, findex) and bounded rows. Two lines: all unbounded (nub = n) and the rest.
unsafe fn lcp(f: SolveLcp, pool: &[f64], next: &mut impl FnMut() -> u64, csr: u32) {
    use rustyac_ode::csp_lcp::{pad, solve_lcp, D_INFINITY};
    let e = crate::ode::edge();
    let mut counts = [Count::default(), Count::default()];
    let mut arena = vec![0u64; 1 << 18];
    for set in ["drive", "unit", "edge"] {
        for n in 1..=40usize {
            for trial in 0..400 {
                let value = |next: &mut dyn FnMut() -> u64| -> f64 {
                    match set {
                        "drive" => pool[(next() % pool.len() as u64) as usize],
                        _ => (next() >> 11) as f64 / (1u64 << 52) as f64 - 1.0,
                    }
                };
                let nskip = pad(n);
                let cols = 1 + (next() % (2 * n as u64)) as usize;
                let b: Vec<f64> = (0..n * cols).map(|_| value(next)).collect();
                let mut a = vec![0.0; n * nskip];
                for i in 0..n {
                    for j in 0..=i {
                        let s: f64 = (0..cols).map(|k| b[i * cols + k] * b[j * cols + k]).sum();
                        a[i * nskip + j] = s;
                        a[j * nskip + i] = s;
                    }
                    a[i * nskip + i] += value(next).abs() * 1e-5;
                }
                let all = trial % 4 == 0;
                let nub = if all { n } else { (next() % n as u64) as usize };
                let mut bx = vec![0.0; 2 * n];
                let mut lh = vec![0.0; 2 * n];
                let mut fi = vec![-1i32; n];
                let mut normals = Vec::new();
                for i in 0..n {
                    bx[2 * i] = if set == "edge" && next().is_multiple_of(3) { e[(next() % e.len() as u64) as usize] } else { value(next) };
                    bx[2 * i + 1] = value(next);
                    let kind = if i < nub { 0 } else { next() % 4 };
                    let (lo, hi) = match kind {
                        // CSP's dInfinity, and in one row of eight true infinities
                        0 if next().is_multiple_of(8) => (f64::NEG_INFINITY, f64::INFINITY),
                        0 => (-D_INFINITY, D_INFINITY),
                        1 => (0.0, D_INFINITY),
                        2 if !normals.is_empty() => {
                            fi[i] = normals[(next() % normals.len() as u64) as usize];
                            let mu = value(next).abs();
                            (-mu, mu)
                        }
                        _ => (-value(next).abs(), value(next).abs()),
                    };
                    if kind == 1 {
                        normals.push(i as i32);
                    }
                    lh[2 * i] = lo;
                    lh[2 * i + 1] = hi;
                }
                let with_findex = trial % 8 != 1;
                let (mut ca, mut cbx, mut clh, mut cfi) = (a.clone(), bx.clone(), lh.clone(), fi.clone());
                let base = (arena.as_mut_ptr() as usize + 63) & !63;
                let mut mem = [0u64; 8];
                mem[0] = base as u64 + 64;
                f(mem.as_mut_ptr(), n as u32, ca.as_mut_ptr(), cbx.as_mut_ptr(), std::ptr::null_mut(), nub as u32, clh.as_mut_ptr(), if with_findex { cfi.as_mut_ptr() } else { std::ptr::null_mut() });
                let (mut pa, mut pbx, mut plh, mut pfi) = (a.clone(), bx.clone(), lh.clone(), fi.clone());
                solve_lcp(n, &mut pa, &mut pbx, nub, &mut plh, if with_findex { Some(&mut pfi) } else { None });
                let flat = |a: &[f64], bx: &[f64], lh: &[f64], fi: &[i32]| -> Vec<f64> { bx.iter().chain(lh).chain(a).copied().chain(fi.iter().map(|&v| v as f64)).collect() };
                counts[all as usize].check(&|| format!("{set} n {n} nub {nub} trial {trial} findex {with_findex}"), &flat(&ca, &cbx, &clh, &cfi), &flat(&pa, &pbx, &plh, &pfi));
            }
        }
    }
    for (name, c) in ["lcp", "lcp_unbounded"].iter().zip(&counts) {
        println!("solver {name} mxcsr {csr:#06x}: {} inputs, {} differ, {} NaN against NaN {:?}", c.n, c.differ, c.nan, c.first.iter().map(|s| s.chars().take(300).collect::<String>()).collect::<Vec<_>>());
    }
}
