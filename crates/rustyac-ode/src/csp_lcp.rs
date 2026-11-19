// SPDX-License-Identifier: BSD-3-Clause

//! ODE master 66615b8a `lcp.cpp` (`dxSolveLCP`, the Dantzig solver with the `dLCP` object, row
//! pointers and NUB optimisations) in f64, as Custom Shaders Patch 0.3.2 (build 4305) compiled it
//! (CSP +0x16ffe10, the generic path outlined at +0x16fffa0). The kernels are `csp_matrix`'s.
//!
//! Layout as master: `pairsbx[2 i]` = b, `pairsbx[2 i + 1]` = x; `pairslh[2 i]` = lo,
//! `pairslh[2 i + 1]` = hi; `a` is `n` rows of `nskip = pad(n)` values. `outer_w` is always null
//! in master's `step.cpp`, so `w` stays inside.

use crate::csp_matrix as cm;

const B: usize = 0;
const X: usize = 1;
const LO: usize = 0;
const HI: usize = 1;

/// CSP's `dInfinity`: 1e300, not infinity (the constants at +0x170013c / +0x170014e).
pub const D_INFINITY: f64 = 1e300;

/// `a == b` as CSP's branches test it (`ucomisd` then `jne`/`je`): NaN counts as equal.
fn ueq(a: f64, b: f64) -> bool {
    !(a < b || a > b)
}

/// `dPAD(n)`: the row skip of `a`.
pub fn pad(n: usize) -> usize {
    if n > 1 { (n + 3) & !3 } else { n }
}

/// `dxSolveLCP(memarena, n, A, pairsbx, NULL, nub, pairslh, findex)`: solves `A x = b + w` with
/// `lo <= x <= hi` and the complementarity conditions; `x` lands in `pairsbx`. `a`, `pairslh` and
/// `findex` are changed as master changes them.
pub fn solve_lcp(n: usize, a: &mut [f64], pairsbx: &mut [f64], nub: usize, pairslh: &mut [f64], findex: Option<&mut [i32]>) {
    if nub >= n {
        // dxSolveLCP_AllUnbounded
        for i in 0..n {
            pairsbx[2 * i + X] = pairsbx[2 * i + B];
            pairsbx[2 * i + B] = 0.0;
        }
        let nskip = pad(n);
        cm::factor_ldlt::<2>(a, pairsbx, n, nskip);
        solve_ldlt::<2, 2>(a, pairsbx, B, X, n, nskip);
    } else {
        Lcp::solve(n, a, pairsbx, nub, pairslh, findex);
    }
}

/// `solveEquationSystemWithLDLT<DS, BS>(L, d, b)` with `d` and `b` in `v` at `d0` / `b0`.
fn solve_ldlt<const DS: usize, const BS: usize>(l: &[f64], v: &mut [f64], d0: usize, b0: usize, n: usize, nskip: usize) {
    cm::solve_l1::<BS>(l, &mut v[b0..], n, nskip);
    for i in 0..n {
        v[b0 + i * BS] *= v[d0 + i * DS];
    }
    cm::solve_l1t::<BS>(l, &mut v[b0..], n, nskip);
}

/// The `dLCP` object and `dxSolveLCP_Generic`'s locals. `arows[i]`: where row `i` of A starts in
/// `a`. `tmp` is master's `delta_w` (the same array).
struct Lcp<'a> {
    n: usize,
    nskip: usize,
    nub: usize,
    nc: usize,
    nn: usize,
    a: &'a mut [f64],
    arows: Vec<usize>,
    pairsbx: &'a mut [f64],
    w: Vec<f64>,
    pairslh: &'a mut [f64],
    l: Vec<f64>,
    d: Vec<f64>,
    dell: Vec<f64>,
    ell: Vec<f64>,
    delta_w: Vec<f64>,
    delta_x: Vec<f64>,
    state: Vec<bool>,
    findex: Option<&'a mut [i32]>,
    p: Vec<u32>,
    c: Vec<u32>,
    tmpbuf: Vec<f64>,
}

impl<'a> Lcp<'a> {
    fn row(&self, i: usize) -> &[f64] {
        &self.a[self.arows[i]..]
    }

    fn at(&self, i: usize, j: usize) -> f64 {
        self.a[self.arows[i] + j]
    }

    fn findex(&self, k: usize) -> i32 {
        self.findex.as_ref().map_or(-1, |f| f[k])
    }

    /// `swapRowsAndCols` (lower triangle only); `fast`: swap the row pointers, else the row data.
    fn swap_rows_and_cols(&mut self, i1: usize, i2: usize, fast: bool) {
        let (r1, r2) = (self.arows[i1], self.arows[i2]);
        for i in i1 + 1..i2 {
            let ri = self.arows[i];
            self.a[r1 + i] = self.a[ri + i1];
            self.a[ri + i1] = self.a[r2 + i];
        }
        self.a[r1 + i2] = self.a[r1 + i1];
        self.a[r1 + i1] = self.a[r2 + i1];
        self.a[r2 + i1] = self.a[r2 + i2];
        if fast {
            self.arows.swap(i1, i2);
        } else {
            for k in 0..=i2 {
                self.a.swap(r1 + k, r2 + k);
            }
        }
        for j in i2 + 1..self.n {
            let rj = self.arows[j];
            self.a.swap(rj + i1, rj + i2);
        }
    }

    /// `swapProblem` (`i1 < i2`).
    fn swap_problem(&mut self, i1: usize, i2: usize, swap_state: bool, fast: bool) {
        if swap_state {
            self.state.swap(i1, i2);
        }
        self.swap_rows_and_cols(i1, i2, fast);
        self.pairsbx.swap(2 * i1 + B, 2 * i2 + B);
        self.pairsbx.swap(2 * i1 + X, 2 * i2 + X);
        self.w.swap(i1, i2);
        self.pairslh.swap(2 * i1 + LO, 2 * i2 + LO);
        self.pairslh.swap(2 * i1 + HI, 2 * i2 + HI);
        self.p.swap(i1, i2);
        if let Some(f) = self.findex.as_mut() {
            f.swap(i1, i2);
        }
    }

    /// `dLCP::dLCP` then `dxSolveLCP_Generic`'s loop and `unpermute_X`.
    fn solve(n: usize, a: &'a mut [f64], pairsbx: &'a mut [f64], nub: usize, pairslh: &'a mut [f64], findex: Option<&'a mut [i32]>) {
        let nskip = pad(n);
        let mut s = Lcp {
            n,
            nskip,
            nub,
            nc: 0,
            nn: 0,
            a,
            arows: (0..n).map(|k| k * nskip).collect(),
            pairsbx,
            w: vec![0.0; n],
            pairslh,
            l: vec![0.0; nskip * n],
            d: vec![0.0; n],
            dell: vec![0.0; n],
            ell: vec![0.0; n],
            delta_w: vec![0.0; n],
            delta_x: vec![0.0; n],
            state: vec![false; n],
            findex,
            p: (0..n as u32).collect(),
            c: vec![0; n],
            tmpbuf: Vec::new(),
        };
        for k in 0..n {
            s.pairsbx[2 * k + X] = 0.0;
        }
        // every unbounded variable to the start (row data swapped, the factor reads it in one piece)
        let mut cur_nub = nub;
        for k in nub..n {
            if s.findex(k) >= 0 {
                continue;
            }
            if s.pairslh[2 * k + LO] == -D_INFINITY && s.pairslh[2 * k + HI] == D_INFINITY {
                if k != cur_nub {
                    s.swap_problem(cur_nub, k, false, false);
                }
                cur_nub += 1;
                s.nub = cur_nub;
            }
        }
        if cur_nub > 0 {
            for j in 0..cur_nub {
                let r = s.arows[j];
                s.l[j * nskip..j * nskip + j + 1].copy_from_slice(&s.a[r..r + j + 1]);
            }
            for k in 0..cur_nub {
                s.pairsbx[2 * k + X] = s.pairsbx[2 * k + B];
            }
            cm::factor_ldlt::<1>(&mut s.l, &mut s.d, cur_nub, nskip);
            cm::solve_l1::<2>(&s.l, &mut s.pairsbx[X..], cur_nub, nskip);
            for i in 0..cur_nub {
                s.pairsbx[2 * i + X] *= s.d[i];
            }
            cm::solve_l1t::<2>(&s.l, &mut s.pairsbx[X..], cur_nub, nskip);
            s.w[..cur_nub].fill(0.0);
            for k in 0..cur_nub {
                s.c[k] = k as u32;
            }
            s.nc = cur_nub;
        }
        // the findex variables to the end
        if s.findex.is_some() {
            let mut dest = n - 1;
            let mut k = n;
            while k > cur_nub {
                k -= 1;
                if s.findex(k) >= 0 {
                    if k != dest {
                        s.swap_problem(k, dest, false, true);
                    }
                    dest = dest.wrapping_sub(1);
                }
            }
        }
        s.run();
        s.unpermute_x();
    }

    fn run(&mut self) {
        let n = self.n;
        let mut hit_first_friction = false;
        let adj_nub = self.nub;
        for i in adj_nub..n {
            let mut s_error = false;
            if !hit_first_friction && self.findex(i) >= 0 {
                for j in 0..n {
                    self.delta_w[self.p[j] as usize] = self.pairsbx[2 * j + X];
                }
                for k in i..n {
                    let wfk = self.delta_w[self.findex(k) as usize];
                    if ueq(wfk, 0.0) {
                        self.pairslh[2 * k + HI] = 0.0;
                        self.pairslh[2 * k + LO] = 0.0;
                    } else {
                        self.pairslh[2 * k + HI] = (self.pairslh[2 * k + HI] * wfk).abs();
                        self.pairslh[2 * k + LO] = -self.pairslh[2 * k + HI];
                    }
                }
                hit_first_friction = true;
            }
            let (nc, nn) = (self.nc, self.nn);
            let w_prep = cm::dot::<2>(self.row(i), &self.pairsbx[X..], nc) + cm::dot::<2>(&self.row(i)[nc..], &self.pairsbx[X + 2 * nc..], nn);
            self.w[i] = w_prep - self.pairsbx[2 * i + B];
            let (lo, hi) = (self.pairslh[2 * i + LO], self.pairslh[2 * i + HI]);
            // the comparisons as CSP's branches do them, NaN included
            if ueq(lo, 0.0) && self.w[i] >= 0.0 {
                self.nn += 1;
                self.state[i] = false;
            } else if ueq(hi, 0.0) && !(self.w[i] > 0.0) {
                self.nn += 1;
                self.state[i] = true;
            } else if ueq(self.w[i], 0.0) {
                self.solve1(i, false, true);
                self.transfer_i_to_c(i);
            } else {
                loop {
                    let dir_positive = self.w[i] <= 0.0;
                    self.solve1(i, dir_positive, false);
                    let (nc, nn) = (self.nc, self.nn);
                    for k in 0..nn {
                        self.delta_w[nc + k] = cm::dot::<1>(self.row(k + nc), &self.delta_x, nc);
                    }
                    let r = self.arows[i];
                    for j in 0..nn {
                        let v = self.a[r + nc + j];
                        if dir_positive {
                            self.delta_w[nc + j] += v;
                        } else {
                            self.delta_w[nc + j] -= v;
                        }
                    }
                    let aic = cm::dot::<1>(self.row(i), &self.delta_x, nc);
                    self.delta_w[i] = if dir_positive { aic + self.at(i, i) } else { aic - self.at(i, i) };

                    let mut cmd = 1;
                    let mut si = 0;
                    let (wi, dwi) = (self.w[i], self.delta_w[i]);
                    // the compiler wrote -w / dw as (-1 / dw) * w
                    let mut s = if !ueq(dwi, 0.0) {
                        (-1.0 / dwi) * wi
                    } else if !ueq(wi, 0.0) {
                        D_INFINITY.copysign(-wi)
                    } else {
                        0.0
                    };
                    let (lo, hi, xi) = (self.pairslh[2 * i + LO], self.pairslh[2 * i + HI], self.pairsbx[2 * i + X]);
                    if dir_positive {
                        if !(hi >= D_INFINITY) {
                            let s2 = hi - xi;
                            if !(s2 >= s) {
                                s = s2;
                                cmd = 3;
                            }
                        }
                    } else if lo > -D_INFINITY {
                        let s2 = xi - lo;
                        if !(s2 >= s) {
                            s = s2;
                            cmd = 2;
                        }
                    }
                    for k in 0..nn {
                        let ik = nc + k;
                        let dw = self.delta_w[ik];
                        if if !self.state[ik] { !(dw >= 0.0) } else { dw > 0.0 } {
                            if ueq(self.pairslh[2 * ik + LO], 0.0) && ueq(self.pairslh[2 * ik + HI], 0.0) {
                                continue;
                            }
                            let s2 = (-1.0 / dw) * self.w[ik];
                            if !(s2 >= s) {
                                s = s2;
                                cmd = 4;
                                si = ik;
                            }
                        }
                    }
                    for k in adj_nub..nc {
                        let dx = self.delta_x[k];
                        let (lo, hi, x) = (self.pairslh[2 * k + LO], self.pairslh[2 * k + HI], self.pairsbx[2 * k + X]);
                        if !(dx >= 0.0) && lo > -D_INFINITY {
                            let s2 = (lo - x) / dx;
                            if !(s2 >= s) {
                                s = s2;
                                cmd = 5;
                                si = k;
                            }
                        }
                        if dx > 0.0 && !(hi >= D_INFINITY) {
                            let s2 = (hi - x) / dx;
                            if !(s2 >= s) {
                                s = s2;
                                cmd = 6;
                                si = k;
                            }
                        }
                    }
                    if !(s > 0.0) {
                        // master logs "LCP internal error, s <= 0" and gives up with x(i..) = w(i..) = 0
                        for k in i..n {
                            self.pairsbx[2 * k + X] = 0.0;
                            self.w[k] = 0.0;
                        }
                        s_error = true;
                        break;
                    }
                    for k in 0..nc {
                        self.pairsbx[2 * k + X] += s * self.delta_x[k];
                    }
                    let xi = self.pairsbx[2 * i + X];
                    self.pairsbx[2 * i + X] = if dir_positive { xi + s } else { xi - s };
                    for k in 0..nn {
                        self.w[nc + k] += s * self.delta_w[nc + k];
                    }
                    self.w[i] += s * self.delta_w[i];
                    match cmd {
                        1 => {
                            self.w[i] = 0.0;
                            self.transfer_i_to_c(i);
                        }
                        2 => {
                            self.pairsbx[2 * i + X] = self.pairslh[2 * i + LO];
                            self.state[i] = false;
                            self.nn += 1;
                        }
                        3 => {
                            self.pairsbx[2 * i + X] = self.pairslh[2 * i + HI];
                            self.state[i] = true;
                            self.nn += 1;
                        }
                        4 => {
                            self.w[si] = 0.0;
                            self.transfer_i_from_n_to_c(si);
                        }
                        _ => {
                            self.pairsbx[2 * si + X] = self.pairslh[2 * si + if cmd == 5 { LO } else { HI }];
                            self.state[si] = cmd == 6;
                            self.transfer_i_from_c_to_n(si);
                        }
                    }
                    if cmd <= 3 {
                        break;
                    }
                }
            }
            if s_error {
                break;
            }
        }
    }

    /// `A(i, i)`, nudged towards `dInfinity` when it equals `ell . Dell` ("a hack to avoid getting a
    /// zero in the denominator").
    fn aii_not(&self, i: usize, dot: f64) -> f64 {
        let aii = self.at(i, i);
        if !ueq(aii, dot) {
            aii
        } else if aii < D_INFINITY {
            aii.next_up()
        } else if aii > D_INFINITY {
            aii.next_down()
        } else {
            aii
        }
    }

    fn transfer_i_to_c(&mut self, i: usize) {
        let nc = self.nc;
        if nc > 0 {
            let t = self.nskip * nc;
            self.l[t..t + nc].copy_from_slice(&self.ell[..nc]);
            let dot = cm::dot::<1>(&self.ell, &self.dell, nc);
            self.d[nc] = 1.0 / (self.aii_not(i, dot) - dot);
        } else {
            self.d[0] = 1.0 / self.at(i, i);
        }
        if i != nc {
            self.swap_problem(nc, i, true, true);
        }
        self.c[nc] = nc as u32;
        self.nc = nc + 1;
    }

    /// `Dell = A(i, C)` (the first `nub` unpermuted), `Dell = L \ Dell`.
    fn load_dell(&mut self, i: usize) {
        let (nc, nub, r) = (self.nc, self.nub, self.arows[i]);
        for j in 0..nc {
            self.dell[j] = self.a[r + if j < nub { j } else { self.c[j] as usize }];
        }
        cm::solve_l1::<1>(&self.l, &mut self.dell, nc, self.nskip);
    }

    fn transfer_i_from_n_to_c(&mut self, i: usize) {
        let nc = self.nc;
        if nc > 0 {
            self.load_dell(i);
            // CSP vectorised the dot (+0x16fefd0): columns 4k, 4k+1 into lanes a, 4k+2, 4k+3 into
            // lanes b, then (a0 + b0) + (a1 + b1), then the last nC % 4 columns in order
            let t = self.nskip * nc;
            let mut lanes = [0.0f64; 4];
            for j in 0..nc {
                let dell_j = self.dell[j];
                let ell_j = dell_j * self.d[j];
                self.l[t + j] = ell_j;
                self.ell[j] = ell_j;
                if j < nc & !3 {
                    lanes[j % 4] += ell_j * dell_j;
                }
            }
            let mut dot = if nc >= 4 { (lanes[2] + lanes[0]) + (lanes[3] + lanes[1]) } else { 0.0 };
            for j in nc & !3..nc {
                dot += self.ell[j] * self.dell[j];
            }
            self.d[nc] = 1.0 / (self.aii_not(i, dot) - dot);
        } else {
            self.d[0] = 1.0 / self.at(i, i);
        }
        if i != nc {
            self.swap_problem(nc, i, true, true);
        }
        self.c[nc] = nc as u32;
        self.nn -= 1;
        self.nc = nc + 1;
    }

    fn transfer_i_from_c_to_n(&mut self, i: usize) {
        let nc = self.nc;
        let mut last_idx = None;
        let mut j = 0;
        while j < nc {
            if self.c[j] as usize == nc - 1 {
                last_idx = Some(j);
            }
            if self.c[j] as usize == i {
                cm::ldlt_remove(self.a, &self.arows, &self.c, &mut self.l, &mut self.d, nc, j, self.nskip, &mut self.tmpbuf);
                let k = last_idx.unwrap_or_else(|| (j + 1..nc).find(|&k| self.c[k] as usize == nc - 1).expect("C holds nC - 1"));
                self.c[k] = self.c[j];
                self.c.copy_within(j + 1..nc, j);
                break;
            }
            j += 1;
        }
        if i != nc - 1 {
            self.swap_problem(i, nc - 1, true, true);
        }
        self.nn += 1;
        self.nc = nc - 1;
    }

    /// `solve1(delta_x, i, dir_positive, only_transfer)`.
    fn solve1(&mut self, i: usize, dir_positive: bool, only_transfer: bool) {
        let nc = self.nc;
        if nc == 0 {
            return;
        }
        self.load_dell(i);
        for j in 0..nc {
            self.ell[j] = self.dell[j] * self.d[j];
        }
        if !only_transfer {
            self.delta_w[..nc].copy_from_slice(&self.ell[..nc]);
            cm::solve_l1t::<1>(&self.l, &mut self.delta_w, nc, self.nskip);
            for j in 0..nc {
                let t = self.delta_w[j];
                self.delta_x[self.c[j] as usize] = if dir_positive { -t } else { t };
            }
        }
    }

    fn unpermute_x(&mut self) {
        let n = self.n;
        for j in 0..n {
            let mut k = self.p[j] as usize;
            if k != j {
                let mut x_j = self.pairsbx[2 * j + X];
                loop {
                    std::mem::swap(&mut x_j, &mut self.pairsbx[2 * k + X]);
                    let orig_k = self.p[k] as usize;
                    self.p[k] = k as u32;
                    if orig_k == j {
                        break;
                    }
                    k = orig_k;
                }
                self.pairsbx[2 * j + X] = x_j;
            }
        }
    }
}
