//! The maths routines on their own, each called directly in the game and in the Rust port and
//! compared bit for bit:
//!
//! * the linear algebra (`_dFactorLDLT`, `_dSolveLDLT`, `_dInvertPDMatrix`,
//!   `_dIsPositiveDefinite`, `_dDot`) on random matrices of every size from 1 to 64 rows. This
//!   reaches sizes and code paths the synthetic worlds do not (the packed sums of the Cholesky
//!   routines need 9 or more rows; the worlds only invert 3x3 inertias);
//! * the small rotation and vector functions (`dQMultiply0` … `3`, `dRfromQ`, `dQfromR`,
//!   `dDQfromW`, `_dSafeNormalize3`, `dPlaneSpace`, `dOrthogonalizeR`) on random input and on
//!   the awkward kind: signed zeros, ties, tiny and huge lengths, half-turn rotations, the
//!   threshold of `dPlaneSpace`, NaN and infinity.
//!
//! A sample of the cases is written, with the game's answers, to
//! `crates/rustyac-ode/tests/data/functions.odefunc` for the crate's own test.

use crate::acs::Acs;
use crate::micro::Rng;
use rustyac_ode::common::pad;
use rustyac_ode::{matrix, odemath, rotation};

const VA_FACTOR_LDLT: usize = 0x1_4039_88d0; // void _dFactorLDLT(float *A, float *d, int n, int nskip)
const VA_SOLVE_LDLT: usize = 0x1_4034_cdf0; // void _dSolveLDLT(const float *L, const float *d, float *b, int n, int nskip)
const VA_INVERT_PD_MATRIX: usize = 0x1_4034_bac0; // int _dInvertPDMatrix(const float *A, float *Ainv, int n, void *tmpbuf)
const VA_IS_POSITIVE_DEFINITE: usize = 0x1_4034_bce0; // int _dIsPositiveDefinite(const float *A, int n, void *tmpbuf)
const VA_DOT: usize = 0x1_4039_0530; // float _dDot(const float *a, const float *b, int n)
const VA_Q_MULTIPLY: [usize; 4] = [0x1_4034_6040, 0x1_4034_6130, 0x1_4034_6220, 0x1_4034_6310]; // void dQMultiplyN(qa, qb, qc)
const VA_R_FROM_Q: usize = 0x1_4034_6620; // void dRfromQ(dMatrix3 R, const dQuaternion q)
const VA_Q_FROM_R: usize = 0x1_4034_6400; // void dQfromR(dQuaternion q, const dMatrix3 R)
const VA_DQ_FROM_W: usize = 0x1_4034_5f70; // void dDQfromW(float dq[4], const dVector3 w, const dQuaternion q)
const VA_SAFE_NORMALIZE3: usize = 0x1_4034_b410; // int _dSafeNormalize3(dVector3 a)
const VA_PLANE_SPACE: usize = 0x1_4034_b6e0; // void dPlaneSpace(const dVector3 n, dVector3 p, dVector3 q)
const VA_ORTHOGONALIZE_R: usize = 0x1_4034_b580; // void dOrthogonalizeR(dMatrix3 m)

const MAX_N: usize = 64;
const CASES_PER_SIZE: usize = 40;
/// Matrix sizes whose first tame and first wild case go into the golden vectors.
const GOLDEN_SIZES: [usize; 17] = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 16, 17, 23, 24];
const SMALL_CASES: usize = 4000;
/// Every how many cases of a small function one goes into the golden vectors.
const GOLDEN_STRIDE: usize = 19;

/// Function codes of the golden vectors (`crates/rustyac-ode/tests/golden_functions.rs`).
const CODE_Q_MULTIPLY0: u32 = 1; // + 1, 2, 3 for dQMultiply1 … 3
const CODE_SAFE_NORMALIZE3: u32 = 5;
const CODE_PLANE_SPACE: u32 = 6;
const CODE_Q_FROM_R: u32 = 7;
const CODE_R_FROM_Q: u32 = 8;
const CODE_ORTHOGONALIZE_R: u32 = 9;
const CODE_DQ_FROM_W: u32 = 10;
const CODE_LDLT: u32 = 11;
const CODE_INVERT_PD: u32 = 12;
const CODE_IS_PD: u32 = 13;
const CODE_DOT: u32 = 14;

fn same(a: f32, b: f32) -> bool {
    a.to_bits() == b.to_bits() || (a.is_nan() && b.is_nan())
}

#[derive(Default)]
struct Tally {
    cases: usize,
    values: usize,
    different_cases: usize,
    first: Option<String>,
}

impl Tally {
    fn case(&mut self, what: &str, n: usize, game: &[f32], rust: &[f32]) {
        self.cases += 1;
        self.values += game.len();
        if let Some(k) = (0..game.len()).find(|&k| !same(game[k], rust[k])) {
            self.different_cases += 1;
            if self.first.is_none() {
                self.first = Some(format!(
                    "{what}, n = {n}, value {k}: game {:?} ({:#010x}) / Rust {:?} ({:#010x})",
                    game[k],
                    game[k].to_bits(),
                    rust[k],
                    rust[k].to_bits()
                ));
            }
        }
    }
    fn line(&self, name: &str, about: &str) -> String {
        format!(
            "| `{name}` | {about} | {} | {} | {} | {} |",
            self.cases,
            self.values,
            self.cases - self.different_cases,
            self.first.clone().unwrap_or_else(|| "none".into())
        )
    }
}

/// The golden vectors: per case the function code, the input words and the game's output.
struct Vectors {
    bytes: Vec<u8>,
    cases: usize,
}

impl Vectors {
    fn add(&mut self, code: u32, input: &[u32], output: &[f32]) {
        for w in [code, input.len() as u32, output.len() as u32] {
            self.bytes.extend_from_slice(&w.to_le_bytes());
        }
        for w in input {
            self.bytes.extend_from_slice(&w.to_le_bytes());
        }
        for v in output {
            self.bytes.extend_from_slice(&v.to_bits().to_le_bytes());
        }
        self.cases += 1;
    }
}

fn bits(values: &[f32]) -> Vec<u32> {
    values.iter().map(|v| v.to_bits()).collect()
}

/// A number that makes sums and products show their order: zero of either sign or one of
/// either sign.
fn awkward(rng: &mut Rng) -> f32 {
    [0.0, -0.0, 1.0, -1.0][rng.below(4) as usize]
}

fn quaternion(rng: &mut Rng, kind: usize) -> [f32; 4] {
    match kind {
        0 => [rng.range(-1.0, 1.0), rng.range(-1.0, 1.0), rng.range(-1.0, 1.0), rng.range(-1.0, 1.0)],
        1 => [awkward(rng), awkward(rng), awkward(rng), awkward(rng)],
        _ => {
            // some components awkward, some random
            let mut q = [0.0f32; 4];
            for v in q.iter_mut() {
                *v = if rng.chance(0.5) { awkward(rng) } else { rng.range(-2.0, 2.0) };
            }
            q
        }
    }
}

fn unit_quaternion(rng: &mut Rng) -> [f32; 4] {
    let mut q = quaternion(rng, 0);
    let n = (q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]).sqrt().max(1e-3);
    for v in q.iter_mut() {
        *v /= n;
    }
    q
}

fn rows9(m: &[f32; 12]) -> [f32; 9] {
    [m[0], m[1], m[2], m[4], m[5], m[6], m[8], m[9], m[10]]
}

pub fn matrix_command(acs: &Acs) -> Result<String, String> {
    let factor: extern "C" fn(*mut f32, *mut f32, i32, i32) = unsafe { std::mem::transmute(acs.va(VA_FACTOR_LDLT)) };
    let solve: extern "C" fn(*const f32, *const f32, *mut f32, i32, i32) =
        unsafe { std::mem::transmute(acs.va(VA_SOLVE_LDLT)) };
    let invert: extern "C" fn(*const f32, *mut f32, i32, *mut u8) -> i32 =
        unsafe { std::mem::transmute(acs.va(VA_INVERT_PD_MATRIX)) };
    let is_pd: extern "C" fn(*const f32, i32, *mut u8) -> i32 =
        unsafe { std::mem::transmute(acs.va(VA_IS_POSITIVE_DEFINITE)) };
    let dot: extern "C" fn(*const f32, *const f32, i32) -> f32 = unsafe { std::mem::transmute(acs.va(VA_DOT)) };

    let mut vectors = Vectors { bytes: b"ODEFUNC1".to_vec(), cases: 0 };
    let mut rng = Rng::new(0x6d61_7472);
    let (mut ldlt, mut inverse, mut definite, mut dots) =
        (Tally::default(), Tally::default(), Tally::default(), Tally::default());
    for n in 1..=MAX_N {
        let nskip = pad(n);
        for case in 0..CASES_PER_SIZE {
            let golden = GOLDEN_SIZES.contains(&n) && (case == 0 || case == 3);
            // a symmetric matrix; three cases in four have a dominant diagonal (well behaved,
            // positive definite), the fourth is arbitrary (huge results, failures)
            let tame = case % 4 != 3;
            let mut a = vec![0.0f32; n * nskip];
            for i in 0..n {
                for j in 0..i {
                    let v = rng.range(-1.0, 1.0);
                    a[i * nskip + j] = v;
                    a[j * nskip + i] = v;
                }
                a[i * nskip + i] = if tame { n as f32 + rng.range(0.5, 2.0) } else { rng.range(-1.0, 3.0) };
            }
            let b: Vec<f32> = (0..n).map(|_| rng.range(-50.0, 50.0)).collect();

            // L*D*L^T factor and solve. The upper triangle and the padding columns are filled
            // with rubbish first: neither side may read them.
            let mut scrambled = a.clone();
            for i in 0..n {
                for j in i + 1..nskip {
                    scrambled[i * nskip + j] = rng.range(-1e6, 1e6);
                }
            }
            let mut input = vec![n as u32];
            input.extend(bits(&scrambled));
            input.extend(bits(&b));
            let (mut a_game, mut a_rust) = (scrambled.clone(), scrambled);
            let (mut d_game, mut d_rust) = (vec![7.0f32; n], vec![7.0f32; n]);
            let (mut b_game, mut b_rust) = (b.clone(), b.clone());
            factor(a_game.as_mut_ptr(), d_game.as_mut_ptr(), n as i32, nskip as i32);
            solve(a_game.as_ptr(), d_game.as_ptr(), b_game.as_mut_ptr(), n as i32, nskip as i32);
            matrix::factor_ldlt(&mut a_rust, &mut d_rust, n, nskip);
            matrix::solve_ldlt(&a_rust, &d_rust, &mut b_rust, n, nskip);
            let mut game = Vec::new();
            let mut rust = Vec::new();
            for i in 0..n {
                game.extend_from_slice(&a_game[i * nskip..i * nskip + i]);
                rust.extend_from_slice(&a_rust[i * nskip..i * nskip + i]);
            }
            game.extend_from_slice(&d_game);
            rust.extend_from_slice(&d_rust);
            game.extend_from_slice(&b_game);
            rust.extend_from_slice(&b_rust);
            ldlt.case("factor and solve", n, &game, &rust);
            if golden {
                vectors.add(CODE_LDLT, &input, &game);
            }

            // inverse of a positive definite matrix (and its refusal of one that is not)
            let mut input = vec![n as u32];
            input.extend(bits(&a));
            let mut inv_game = vec![3.0f32; n * nskip];
            let mut inv_rust = vec![3.0f32; n * nskip];
            let ok_game = invert(a.as_ptr(), inv_game.as_mut_ptr(), n as i32, std::ptr::null_mut()) != 0;
            let ok_rust = matrix::invert_pd_matrix(&a, &mut inv_rust, n);
            let mut game = vec![ok_game as u8 as f32];
            let mut rust = vec![ok_rust as u8 as f32];
            for i in 0..n {
                game.extend_from_slice(&inv_game[i * nskip..i * nskip + n]);
                rust.extend_from_slice(&inv_rust[i * nskip..i * nskip + n]);
            }
            inverse.case("inverse", n, &game, &rust);
            if golden {
                vectors.add(CODE_INVERT_PD, &input, &game);
            }

            // the test alone
            let game = [(is_pd(a.as_ptr(), n as i32, std::ptr::null_mut()) != 0) as u8 as f32];
            definite.case("positive definite", n, &game, &[matrix::is_positive_definite(&a, n) as u8 as f32]);
            if golden {
                vectors.add(CODE_IS_PD, &input, &game);
            }

            // dot product
            let x: Vec<f32> = (0..n).map(|_| rng.range(-10.0, 10.0)).collect();
            let game = [dot(x.as_ptr(), b.as_ptr(), n as i32)];
            dots.case("dot", n, &game, &[matrix::dot(&x, &b, n)]);
            if golden {
                let mut input = vec![n as u32];
                input.extend(bits(&x));
                input.extend(bits(&b));
                vectors.add(CODE_DOT, &input, &game);
            }
        }
    }

    // the small functions
    let mut rng = Rng::new(0x736d_616c);
    let mut multiply = [Tally::default(), Tally::default(), Tally::default(), Tally::default()];
    for (k, tally) in multiply.iter_mut().enumerate() {
        let game_fn: extern "C" fn(*mut f32, *const f32, *const f32) = unsafe { std::mem::transmute(acs.va(VA_Q_MULTIPLY[k])) };
        for case in 0..SMALL_CASES {
            let (qb, qc) = (quaternion(&mut rng, case % 3), quaternion(&mut rng, (case / 3) % 3));
            let mut game = [9.0f32; 4];
            game_fn(game.as_mut_ptr(), qb.as_ptr(), qc.as_ptr());
            let rust = match k {
                0 => rotation::q_multiply0(&qb, &qc),
                1 => rotation::q_multiply1(&qb, &qc),
                2 => rotation::q_multiply2(&qb, &qc),
                _ => rotation::q_multiply3(&qb, &qc),
            };
            tally.case("quaternion product", k, &game, &rust);
            if case % GOLDEN_STRIDE == 0 {
                let mut input = bits(&qb);
                input.extend(bits(&qc));
                vectors.add(CODE_Q_MULTIPLY0 + k as u32, &input, &game);
            }
        }
    }

    let game_normalize: extern "C" fn(*mut f32) -> i32 = unsafe { std::mem::transmute(acs.va(VA_SAFE_NORMALIZE3)) };
    let mut normalize = Tally::default();
    for case in 0..SMALL_CASES {
        let mut v = [rng.range(-1.0, 1.0), rng.range(-1.0, 1.0), rng.range(-1.0, 1.0)];
        match case % 11 {
            // lengths from 1e-30 to 1e30
            0..=3 => {
                let scale = 10f32.powf(rng.range(-30.0, 30.0));
                v = v.map(|x| x * scale);
            }
            // ties for the largest component, either sign
            4 => {
                v[1] = if rng.chance(0.5) { v[0] } else { -v[0] };
                if rng.chance(0.5) {
                    v[2] = if rng.chance(0.5) { v[0] } else { -v[0] };
                }
            }
            5 => v[rng.below(3) as usize] = awkward(&mut rng),
            // zero vectors of every sign pattern, and vectors with zeros in them
            6 => {
                for x in v.iter_mut() {
                    if rng.chance(0.7) {
                        *x = if rng.chance(0.5) { 0.0 } else { -0.0 };
                    }
                }
            }
            // denormal numbers
            7 => v = v.map(|x| x * 1e-41),
            // near the largest float
            8 => v = v.map(|x| x * 3.0e38),
            // not a number, infinity
            9 => {
                let bad = [f32::NAN, f32::INFINITY, f32::NEG_INFINITY][rng.below(3) as usize];
                v[rng.below(3) as usize] = bad;
            }
            _ => {}
        }
        let mut game = [v[0], v[1], v[2], 5.0];
        let mut rust = game;
        let ok_game = game_normalize(game.as_mut_ptr()) != 0;
        let ok_rust = odemath::safe_normalize3(&mut rust);
        let game = [ok_game as u8 as f32, game[0], game[1], game[2]];
        normalize.case("normalise", case % 11, &game, &[ok_rust as u8 as f32, rust[0], rust[1], rust[2]]);
        if case % GOLDEN_STRIDE == 0 {
            vectors.add(CODE_SAFE_NORMALIZE3, &bits(&v), &game);
        }
    }

    let game_plane: extern "C" fn(*const f32, *mut f32, *mut f32) = unsafe { std::mem::transmute(acs.va(VA_PLANE_SPACE)) };
    let mut plane = Tally::default();
    for case in 0..SMALL_CASES {
        let mut n = [rng.range(-1.0, 1.0), rng.range(-1.0, 1.0), rng.range(-1.0, 1.0), 0.0];
        let length = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt().max(1e-6);
        for x in n.iter_mut().take(3) {
            *x /= length;
        }
        match case % 7 {
            // the threshold |n.z| > sqrt(1/2), which the game tests in double precision
            3 => {
                let near = (0.707_106_77f32.to_bits() as i64 + rng.below(9) as i64 - 4) as u32;
                n[2] = f32::from_bits(near) * if rng.chance(0.5) { 1.0 } else { -1.0 };
            }
            // axes, signed zeros
            4 => {
                n = [awkward(&mut rng), awkward(&mut rng), awkward(&mut rng), 0.0];
            }
            // not unit length
            5 => {
                let scale = 10f32.powf(rng.range(-3.0, 3.0));
                for x in n.iter_mut() {
                    *x *= scale;
                }
            }
            _ => {}
        }
        let (mut p_game, mut q_game) = ([4.0f32; 4], [4.0f32; 4]);
        let (mut p_rust, mut q_rust) = ([4.0f32; 4], [4.0f32; 4]);
        game_plane(n.as_ptr(), p_game.as_mut_ptr(), q_game.as_mut_ptr());
        odemath::plane_space(&n, &mut p_rust, &mut q_rust);
        let game = [p_game[0], p_game[1], p_game[2], q_game[0], q_game[1], q_game[2]];
        plane.case("plane space", case % 7, &game, &[p_rust[0], p_rust[1], p_rust[2], q_rust[0], q_rust[1], q_rust[2]]);
        if case % GOLDEN_STRIDE == 0 {
            vectors.add(CODE_PLANE_SPACE, &bits(&n[..3]), &game);
        }
    }

    let game_q_from_r: extern "C" fn(*mut f32, *const f32) = unsafe { std::mem::transmute(acs.va(VA_Q_FROM_R)) };
    let game_r_from_q: extern "C" fn(*mut f32, *const f32) = unsafe { std::mem::transmute(acs.va(VA_R_FROM_Q)) };
    let (mut q_from_r, mut r_from_q) = (Tally::default(), Tally::default());
    for case in 0..SMALL_CASES {
        let mut q = unit_quaternion(&mut rng);
        match case % 5 {
            // half turns and nearly half turns: the three branches of dQfromR for a
            // negative trace
            1 => {
                q[0] = rng.range(-1e-3, 1e-3);
                q[1 + rng.below(3) as usize] *= 5.0;
            }
            2 => {
                q = [0.0; 4];
                q[1 + rng.below(3) as usize] = if rng.chance(0.5) { 1.0 } else { -1.0 };
            }
            // not unit length, awkward numbers
            3 => q = quaternion(&mut rng, 2),
            _ => {}
        }
        let mut r_game = [6.0f32; 12];
        let mut r_rust = [6.0f32; 12];
        game_r_from_q(r_game.as_mut_ptr(), q.as_ptr());
        rotation::r_from_q(&mut r_rust, &q);
        r_from_q.case("matrix of a quaternion", case % 5, &rows9(&r_game), &rows9(&r_rust));
        if case % GOLDEN_STRIDE == 0 {
            vectors.add(CODE_R_FROM_Q, &bits(&q), &rows9(&r_game));
        }
        // back again, from the game's matrix (padding zeroed), sometimes a little off a rotation
        let mut r = r_game;
        for k in [3, 7, 11] {
            r[k] = 0.0;
        }
        if case % 4 == 3 {
            for k in [0usize, 1, 2, 4, 5, 6, 8, 9, 10] {
                r[k] += rng.range(-1e-3, 1e-3);
            }
        }
        let mut game = [6.0f32; 4];
        game_q_from_r(game.as_mut_ptr(), r.as_ptr());
        q_from_r.case("quaternion of a matrix", case % 5, &game, &rotation::q_from_r(&r));
        if case % GOLDEN_STRIDE == 0 {
            vectors.add(CODE_Q_FROM_R, &bits(&r), &game);
        }
    }

    let game_orthogonalize: extern "C" fn(*mut f32) = unsafe { std::mem::transmute(acs.va(VA_ORTHOGONALIZE_R)) };
    let mut orthogonalize = Tally::default();
    for case in 0..SMALL_CASES {
        let mut m = [0.0f32; 12];
        rotation::r_from_q(&mut m, &unit_quaternion(&mut rng));
        match case % 6 {
            // a little off, far off, anything
            0 | 1 => {
                for k in [0usize, 1, 2, 4, 5, 6, 8, 9, 10] {
                    m[k] += rng.range(-1e-3, 1e-3);
                }
            }
            2 => {
                for k in [0usize, 1, 2, 4, 5, 6, 8, 9, 10] {
                    m[k] += rng.range(-0.5, 0.5);
                }
            }
            3 => {
                for k in [0usize, 1, 2, 4, 5, 6, 8, 9, 10] {
                    m[k] = rng.range(-3.0, 3.0);
                }
            }
            // rows that cannot be repaired: a zero row, two parallel rows
            4 => {
                let row = 4 * rng.below(3) as usize;
                if rng.chance(0.5) {
                    m[row..row + 3].fill(0.0);
                } else {
                    let other = (row + 4) % 12;
                    let copy = [m[row], m[row + 1], m[row + 2]];
                    m[other..other + 3].copy_from_slice(&copy);
                }
            }
            _ => {}
        }
        let (mut game, mut rust) = (m, m);
        game_orthogonalize(game.as_mut_ptr());
        odemath::orthogonalize_r(&mut rust);
        orthogonalize.case("orthogonalise", case % 6, &rows9(&game), &rows9(&rust));
        if case % GOLDEN_STRIDE == 0 {
            vectors.add(CODE_ORTHOGONALIZE_R, &bits(&m), &rows9(&game));
        }
    }

    let game_dq: extern "C" fn(*mut f32, *const f32, *const f32) = unsafe { std::mem::transmute(acs.va(VA_DQ_FROM_W)) };
    let mut dq = Tally::default();
    for case in 0..SMALL_CASES {
        let q = quaternion(&mut rng, case % 3);
        let w = if case % 4 == 3 {
            [awkward(&mut rng), awkward(&mut rng), awkward(&mut rng), 0.0]
        } else {
            [rng.range(-30.0, 30.0), rng.range(-30.0, 30.0), rng.range(-30.0, 30.0), 0.0]
        };
        let mut game = [8.0f32; 4];
        game_dq(game.as_mut_ptr(), w.as_ptr(), q.as_ptr());
        dq.case("quaternion rate", case % 3, &game, &rotation::dq_from_w(&w, &q));
        if case % GOLDEN_STRIDE == 0 {
            let mut input = bits(&w[..3]);
            input.extend(bits(&q));
            vectors.add(CODE_DQ_FROM_W, &input, &game);
        }
    }

    let mut table = String::from(
        "| routines | what is compared | cases | values | cases bit-exact | first difference |\n|---|---|---|---|---|---|\n",
    );
    let tallies = [
        (&ldlt, "_dFactorLDLT` + `_dSolveLDLT", "L (strict lower triangle), d, the solution; n = 1 … 64"),
        (&inverse, "_dInvertPDMatrix", "the success flag and the inverse; n = 1 … 64"),
        (&definite, "_dIsPositiveDefinite", "the answer; n = 1 … 64"),
        (&dots, "_dDot", "the sum; n = 1 … 64"),
        (&multiply[0], "dQMultiply0", "qb * qc; random and signed zeros / ones"),
        (&multiply[1], "dQMultiply1", "inverse(qb) * qc"),
        (&multiply[2], "dQMultiply2", "qb * inverse(qc)"),
        (&multiply[3], "dQMultiply3", "inverse(qb) * inverse(qc)"),
        (&r_from_q, "dRfromQ", "the 3x3 matrix; unit, half-turn and arbitrary quaternions"),
        (&q_from_r, "dQfromR", "the quaternion; rotations incl. half turns, matrices a little off"),
        (&dq, "dDQfromW", "the quaternion rate"),
        (
            &normalize,
            "_dSafeNormalize3",
            "the flag and the vector; lengths 1e-30 … 1e30, ties, zeros, denormals, NaN, infinity",
        ),
        (&plane, "dPlaneSpace", "both vectors; unit normals, the sqrt(1/2) threshold, axes, other lengths"),
        (&orthogonalize, "dOrthogonalizeR", "the 3x3 matrix; near rotations, arbitrary matrices, zero and parallel rows"),
    ];
    let mut different = 0;
    for (tally, name, about) in tallies {
        let line = tally.line(name, about);
        println!("{line}");
        table.push_str(&line);
        table.push('\n');
        different += tally.different_cases;
    }
    if different > 0 {
        return Err("at least one case differs".into());
    }
    let path = crate::repo_root().join("crates/rustyac-ode/tests/data/functions.odefunc");
    std::fs::write(&path, &vectors.bytes).map_err(|e| format!("{}: {e}", path.display()))?;
    println!("wrote {} ({} cases, {} bytes)", path.display(), vectors.cases, vectors.bytes.len());
    Ok(table)
}
