// SPDX-License-Identifier: BSD-3-Clause

//! Golden test of the maths routines one by one: inputs and the answers Assetto Corsa's own
//! code gave for them, compared bit for bit (a NaN in the same place counts as equal).
//!
//! `tests/data/functions.odefunc` is written by `ode_oracle matrix`, which calls each function
//! inside the game's executable. It is a sample of that run: the matrix routines for sizes 1 to
//! 24 (one well-behaved and one arbitrary matrix each), the small functions for random input and
//! for the awkward kind (signed zeros, ties, tiny and huge lengths, half turns, NaN, infinity).
//!
//! Format (little endian, 32-bit words): `"ODEFUNC1"`, then per case
//! `u32 code, u32 inputs, u32 outputs, the input words, the output words`:
//!
//! ```text
//!  1-4  dQMultiply0 … 3        in: qb[4], qc[4]                      out: qa[4]
//!  5    _dSafeNormalize3       in: a[3]                              out: flag, a[3]
//!  6    dPlaneSpace            in: n[3]                              out: p[3], q[3]
//!  7    dQfromR                in: R[12]                             out: q[4]
//!  8    dRfromQ                in: q[4]                              out: R (3x3, row-major)
//!  9    dOrthogonalizeR        in: m[12]                             out: m (3x3, row-major)
//! 10    dDQfromW               in: w[3], q[4]                        out: dq[4]
//! 11    _dFactorLDLT + _dSolveLDLT   in: n, A[n*nskip], b[n]         out: L (strict lower triangle,
//!                                                                         row by row), d[n], x[n]
//! 12    _dInvertPDMatrix       in: n, A[n*nskip]                     out: flag, inverse (n x n)
//! 13    _dIsPositiveDefinite   in: n, A[n*nskip]                     out: flag
//! 14    _dDot                  in: n, a[n], b[n]                     out: the sum
//! ```
//!
//! `nskip` is `n` rounded up to a multiple of 4 (ODE's row padding); flags are 0.0 or 1.0.

use rustyac_ode::common::pad;
use rustyac_ode::{matrix, odemath, rotation};

fn floats(words: &[u32]) -> Vec<f32> {
    words.iter().map(|&w| f32::from_bits(w)).collect()
}

fn rows9(m: &[f32; 12]) -> Vec<f32> {
    [0usize, 1, 2, 4, 5, 6, 8, 9, 10].iter().map(|&k| m[k]).collect()
}

fn answer(code: u32, input: &[u32]) -> Vec<f32> {
    let f = floats(input);
    let q = |at: usize| -> [f32; 4] { f[at..at + 4].try_into().unwrap() };
    match code {
        1 => rotation::q_multiply0(&q(0), &q(4)).to_vec(),
        2 => rotation::q_multiply1(&q(0), &q(4)).to_vec(),
        3 => rotation::q_multiply2(&q(0), &q(4)).to_vec(),
        4 => rotation::q_multiply3(&q(0), &q(4)).to_vec(),
        5 => {
            let mut a = [f[0], f[1], f[2], 5.0];
            let ok = odemath::safe_normalize3(&mut a);
            vec![ok as u8 as f32, a[0], a[1], a[2]]
        }
        6 => {
            let (mut p, mut q) = ([4.0f32; 4], [4.0f32; 4]);
            odemath::plane_space(&[f[0], f[1], f[2], 0.0], &mut p, &mut q);
            vec![p[0], p[1], p[2], q[0], q[1], q[2]]
        }
        7 => rotation::q_from_r(&f[..12].try_into().unwrap()).to_vec(),
        8 => {
            let mut r = [6.0f32; 12];
            rotation::r_from_q(&mut r, &q(0));
            rows9(&r)
        }
        9 => {
            let mut m: [f32; 12] = f[..12].try_into().unwrap();
            odemath::orthogonalize_r(&mut m);
            rows9(&m)
        }
        10 => rotation::dq_from_w(&[f[0], f[1], f[2], 0.0], &q(3)).to_vec(),
        11 => {
            let n = input[0] as usize;
            let nskip = pad(n);
            let mut a = f[1..1 + n * nskip].to_vec();
            let mut b = f[1 + n * nskip..1 + n * nskip + n].to_vec();
            let mut d = vec![7.0f32; n];
            matrix::factor_ldlt(&mut a, &mut d, n, nskip);
            matrix::solve_ldlt(&a, &d, &mut b, n, nskip);
            let mut out = Vec::new();
            for i in 0..n {
                out.extend_from_slice(&a[i * nskip..i * nskip + i]);
            }
            out.extend_from_slice(&d);
            out.extend_from_slice(&b);
            out
        }
        12 => {
            let n = input[0] as usize;
            let nskip = pad(n);
            let mut inverse = vec![3.0f32; n * nskip];
            let ok = matrix::invert_pd_matrix(&f[1..1 + n * nskip], &mut inverse, n);
            let mut out = vec![ok as u8 as f32];
            for i in 0..n {
                out.extend_from_slice(&inverse[i * nskip..i * nskip + n]);
            }
            out
        }
        13 => {
            let n = input[0] as usize;
            vec![matrix::is_positive_definite(&f[1..1 + n * pad(n)], n) as u8 as f32]
        }
        14 => {
            let n = input[0] as usize;
            vec![matrix::dot(&f[1..1 + n], &f[1 + n..1 + 2 * n], n)]
        }
        other => panic!("function code {other}"),
    }
}

#[test]
fn every_function_gives_the_games_bits() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/functions.odefunc");
    let data = std::fs::read(&path).expect("tests/data/functions.odefunc");
    assert_eq!(&data[..8], b"ODEFUNC1");
    let words: Vec<u32> = data[8..].chunks_exact(4).map(|c| u32::from_le_bytes(c.try_into().unwrap())).collect();
    let mut at = 0;
    let mut cases = [0usize; 15];
    while at < words.len() {
        let (code, inputs, outputs) = (words[at], words[at + 1] as usize, words[at + 2] as usize);
        let input = &words[at + 3..at + 3 + inputs];
        let expected = floats(&words[at + 3 + inputs..at + 3 + inputs + outputs]);
        at += 3 + inputs + outputs;
        let got = answer(code, input);
        assert_eq!(got.len(), expected.len(), "function {code}, case {}", cases[code as usize]);
        for k in 0..got.len() {
            let same = got[k].to_bits() == expected[k].to_bits() || (got[k].is_nan() && expected[k].is_nan());
            assert!(
                same,
                "function {code}, case {}, value {k}: game {:?} ({:#010x}) / Rust {:?} ({:#010x}); input {:?}",
                cases[code as usize],
                expected[k],
                expected[k].to_bits(),
                got[k],
                got[k].to_bits(),
                floats(input)
            );
        }
        cases[code as usize] += 1;
    }
    for (code, &count) in cases.iter().enumerate().skip(1) {
        assert!(count >= 30, "only {count} cases of function {code}");
    }
}
