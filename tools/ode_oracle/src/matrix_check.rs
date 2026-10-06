//! The linear-algebra routines on their own: the game's `_dFactorLDLT`, `_dSolveLDLT`,
//! `_dInvertPDMatrix` and `_dDot` called directly on random matrices of every size from 1 to
//! 64 rows, against the Rust versions, bit for bit. This reaches sizes and code paths the
//! synthetic worlds do not (the packed sums of the Cholesky routines need 9 or more rows; the
//! worlds only invert 3x3 inertias).

use crate::acs::Acs;
use crate::micro::Rng;
use rustyac_ode::common::pad;
use rustyac_ode::matrix;

const VA_FACTOR_LDLT: usize = 0x1_4039_88d0; // void _dFactorLDLT(float *A, float *d, int n, int nskip)
const VA_SOLVE_LDLT: usize = 0x1_4034_cdf0; // void _dSolveLDLT(const float *L, const float *d, float *b, int n, int nskip)
const VA_INVERT_PD_MATRIX: usize = 0x1_4034_bac0; // int _dInvertPDMatrix(const float *A, float *Ainv, int n, void *tmpbuf)
const VA_DOT: usize = 0x1_4039_0530; // float _dDot(const float *a, const float *b, int n)

const MAX_N: usize = 64;
const CASES_PER_SIZE: usize = 40;

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

pub fn matrix_command(acs: &Acs) -> Result<String, String> {
    let factor: extern "C" fn(*mut f32, *mut f32, i32, i32) = unsafe { std::mem::transmute(acs.va(VA_FACTOR_LDLT)) };
    let solve: extern "C" fn(*const f32, *const f32, *mut f32, i32, i32) =
        unsafe { std::mem::transmute(acs.va(VA_SOLVE_LDLT)) };
    let invert: extern "C" fn(*const f32, *mut f32, i32, *mut u8) -> i32 =
        unsafe { std::mem::transmute(acs.va(VA_INVERT_PD_MATRIX)) };
    let dot: extern "C" fn(*const f32, *const f32, i32) -> f32 = unsafe { std::mem::transmute(acs.va(VA_DOT)) };

    let mut rng = Rng::new(0x6d61_7472);
    let (mut ldlt, mut inverse, mut dots) = (Tally::default(), Tally::default(), Tally::default());
    for n in 1..=MAX_N {
        let nskip = pad(n);
        for case in 0..CASES_PER_SIZE {
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

            // inverse of a positive definite matrix (and its refusal of one that is not)
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

            // dot product
            let x: Vec<f32> = (0..n).map(|_| rng.range(-10.0, 10.0)).collect();
            dots.case("dot", n, &[dot(x.as_ptr(), b.as_ptr(), n as i32)], &[matrix::dot(&x, &b, n)]);
        }
    }
    let mut table = String::from(
        "| routines | what is compared | cases | values | cases bit-exact | first difference |\n|---|---|---|---|---|---|\n",
    );
    for line in [
        ldlt.line("_dFactorLDLT` + `_dSolveLDLT", "L (strict lower triangle), d, the solution; n = 1 … 64"),
        inverse.line("_dInvertPDMatrix", "the success flag and the inverse; n = 1 … 64"),
        dots.line("_dDot", "the sum; n = 1 … 64"),
    ] {
        println!("{line}");
        table.push_str(&line);
        table.push('\n');
    }
    if ldlt.different_cases + inverse.different_cases + dots.different_cases > 0 {
        return Err("at least one matrix case differs".into());
    }
    Ok(table)
}
