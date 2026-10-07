// SPDX-License-Identifier: BSD-3-Clause

//! The Dantzig LCP solver with bounds and friction: `ode/src/lcp.cpp` as compiled into
//! `acs.exe` (`dSolveLCP` @ 0x140392260, `dLCP::*` @ 0x140391cf0 …).
//!
//! Given `A` (symmetric positive definite, lower triangle used), `b`, `lo` and `hi`, it
//! finds `x` and `w` with `A*x = b + w` and for every row `i` one of
//!
//! * `x[i] = lo[i]`, `w[i] >= 0`,
//! * `x[i] = hi[i]`, `w[i] <= 0`,
//! * `lo[i] < x[i] < hi[i]`, `w[i] = 0`.
//!
//! A row with `findex[i] >= 0` is a friction row: its limits are `hi[i] * |x[findex[i]]|`
//! (the row's own `hi` is the friction coefficient, `x[findex]` the normal force found
//! before it).
//!
//! The library is the "fast" variant with row pointers (`dLCP_FAST`, `ROWPTRS`,
//! `NUB_OPTIMIZATIONS`): the rows of `A` are swapped by swapping pointers once the first
//! `nub` rows are factorised, everything else (x, b, w, lo, hi, p, state, findex) is swapped
//! in place. The index sets are kept as prefixes: rows `0..nC` are the clamped set `C`,
//! rows `nC..nC+nN` the set `N`.
//!
//! Where the compiled code differs from the source it is followed here:
//!
//! * the step that brings a `w` to zero is `(-1 / delta_w) * w`, not `-w / delta_w`;
//! * most comparisons are the inverted ones the compiler chose (`!(a >= b)` for `a < b`),
//!   and its equality tests have no parity check (`x == 0` is also true for a NaN): they
//!   decide what a NaN does;
//! * `_dLDLTRemove` divides with the CPU's approximate reciprocal (see `matrix::ldlt_remove`).

use crate::matrix::{dot, factor_ldlt, ldlt_remove, solve_l1, solve_l1t, solve_ldlt};

/// Scratch arrays of `dSolveLCP` (the stepper's arena in ODE), kept between calls.
#[derive(Clone, Debug, Default)]
pub(crate) struct LcpMemory {
    l: Vec<f32>,
    d: Vec<f32>,
    w: Vec<f32>,
    delta_w: Vec<f32>,
    delta_x: Vec<f32>,
    dell: Vec<f32>,
    ell: Vec<f32>,
    /// `Arows`: the physical row of `A` that logical row `k` lives in.
    arows: Vec<usize>,
    p: Vec<i32>,
    c: Vec<i32>,
    state: Vec<bool>,
    tmpbuf: Vec<f32>,
}

/// What the solver did, for tests and tools.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LcpStats {
    /// Rows that were not unbounded from the start.
    pub bounded_rows: u32,
    /// Passes of the inner "push x(i), w(i)" loop.
    pub pivots: u32,
    /// The solver gave up with "s <= 0".
    pub s_error: bool,
}

struct Lcp<'a> {
    n: usize,
    nskip: usize,
    nub: usize,
    n_c: usize,
    n_n: usize,
    a: &'a mut [f32],
    arows: &'a mut [usize],
    x: &'a mut [f32],
    b: &'a mut [f32],
    w: &'a mut [f32],
    lo: &'a mut [f32],
    hi: &'a mut [f32],
    l: &'a mut [f32],
    d: &'a mut [f32],
    dell: &'a mut [f32],
    ell: &'a mut [f32],
    state: &'a mut [bool],
    findex: Option<&'a mut [i32]>,
    p: &'a mut [i32],
    c: &'a mut [i32],
    tmpbuf: &'a mut Vec<f32>,
}

impl Lcp<'_> {
    #[inline]
    fn at(&self, row: usize, col: usize) -> f32 {
        self.a[self.arows[row] * self.nskip + col]
    }

    #[inline]
    fn row(&self, row: usize) -> &[f32] {
        let start = self.arows[row] * self.nskip;
        &self.a[start..start + self.nskip]
    }

    /// `swapRowsAndCols` (inlined): swaps rows and columns `i1 < i2` of the lower triangle.
    fn swap_rows_and_cols(&mut self, i1: usize, i2: usize, do_fast_row_swaps: bool) {
        let nskip = self.nskip;
        let r1 = self.arows[i1] * nskip;
        let r2 = self.arows[i2] * nskip;
        for i in i1 + 1..i2 {
            let ri = self.arows[i] * nskip + i1;
            self.a[r1 + i] = self.a[ri];
            self.a[ri] = self.a[r2 + i];
        }
        self.a[r1 + i2] = self.a[r1 + i1];
        self.a[r1 + i1] = self.a[r2 + i1];
        self.a[r2 + i1] = self.a[r2 + i2];
        // swap rows, by swapping row pointers
        if do_fast_row_swaps {
            self.arows.swap(i1, i2);
        } else {
            // Only swap till i2 column to match A plain storage variant.
            for k in 0..=i2 {
                self.a.swap(r1 + k, r2 + k);
            }
        }
        // swap columns the hard way
        for j in i2 + 1..self.n {
            let rj = self.arows[j] * nskip;
            self.a.swap(rj + i1, rj + i2);
        }
    }

    /// `swapProblem` (inlined): swaps indexes `i1 <= i2` of the whole problem.
    fn swap_problem(&mut self, i1: usize, i2: usize, do_fast_row_swaps: bool) {
        if i1 == i2 {
            return;
        }
        self.swap_rows_and_cols(i1, i2, do_fast_row_swaps);
        self.x.swap(i1, i2);
        self.b.swap(i1, i2);
        self.w.swap(i1, i2);
        self.lo.swap(i1, i2);
        self.hi.swap(i1, i2);
        self.p.swap(i1, i2);
        self.state.swap(i1, i2);
        if let Some(findex) = &mut self.findex {
            findex.swap(i1, i2);
        }
    }

    /// `dLCP::dLCP` @ 0x140391cf0.
    fn init(&mut self) {
        let n = self.n;
        let nskip = self.nskip;
        for v in self.x[..n].iter_mut() {
            *v = 0.0;
        }
        // make matrix row pointers
        for k in 0..n {
            self.arows[k] = k;
        }
        // initially unpermuted
        for k in 0..n {
            self.p[k] = k as i32;
        }

        // permute the problem so that *all* the unbounded variables are at the start, i.e.
        // look for unbounded variables not included in `nub'. we can potentially push up
        // `nub' this way and get a bigger initial factorization. note that when we swap
        // rows/cols here we must not just swap row pointers, as the initial factorization
        // relies on the data being all in one chunk. variables that have findex >= 0 are
        // *not* considered to be unbounded even if lo=-inf and hi=inf - this is because
        // these limits may change during the solution process.
        for k in self.nub..n {
            if let Some(findex) = &self.findex {
                if findex[k] >= 0 {
                    continue;
                }
            }
            // ucomiss + jne, no parity test: a NaN bound counts as infinite
            if eq_or_nan(self.lo[k], f32::NEG_INFINITY) && eq_or_nan(self.hi[k], f32::INFINITY) {
                self.swap_problem(self.nub, k, false);
                self.nub += 1;
            }
        }

        // if there are unbounded variables at the start, factorize A up to that point and
        // solve for x. this puts all indexes 0..nub-1 into C.
        if self.nub > 0 {
            let nub = self.nub;
            for j in 0..nub {
                // AROW(j) is still row j here (no pointer swaps yet)
                let src = self.arows[j] * nskip;
                self.l[j * nskip..j * nskip + j + 1].copy_from_slice(&self.a[src..src + j + 1]);
            }
            factor_ldlt(self.l, self.d, nub, nskip);
            self.x[..nub].copy_from_slice(&self.b[..nub]);
            solve_ldlt(self.l, self.d, self.x, nub, nskip);
            for v in self.w[..nub].iter_mut() {
                *v = 0.0;
            }
            for k in 0..nub {
                self.c[k] = k as i32;
            }
            self.n_c = nub;
        }

        // permute the indexes > nub such that all findex variables are at the end
        if self.findex.is_some() {
            let nub = self.nub;
            let mut num_at_end = 0usize;
            let mut k = n;
            while k > nub {
                k -= 1;
                if self.findex.as_ref().unwrap()[k] >= 0 {
                    self.swap_problem(k, n - 1 - num_at_end, true);
                    num_at_end += 1;
                }
            }
        }
    }

    /// `dLCP::transfer_i_to_C` @ 0x140394550.
    fn transfer_i_to_c(&mut self, i: usize) {
        let n_c = self.n_c;
        if n_c > 0 {
            // ell,Dell were computed by solve1(). note, ell = D \ L1solve (L,A(i,C))
            let ltgt = n_c * self.nskip;
            for j in 0..n_c {
                self.l[ltgt + j] = self.ell[j];
            }
            self.d[n_c] = 1.0f32 / (self.at(i, i) - dot(self.ell, self.dell, n_c));
        } else {
            self.d[0] = 1.0f32 / self.at(i, i);
        }
        self.swap_problem(n_c, i, true);
        self.c[n_c] = n_c as i32;
        self.n_c = n_c + 1;
    }

    /// `dLCP::transfer_i_from_N_to_C` @ 0x1403941b0.
    fn transfer_i_from_n_to_c(&mut self, i: usize) {
        let n_c = self.n_c;
        if n_c > 0 {
            {
                let nub = self.nub;
                let start = self.arows[i] * self.nskip;
                // if nub>0, initial part of aptr unpermuted
                for j in 0..nub {
                    self.dell[j] = self.a[start + j];
                }
                for j in nub..n_c {
                    self.dell[j] = self.a[start + self.c[j] as usize];
                }
            }
            solve_l1(self.l, self.dell, n_c, self.nskip);
            let ltgt = n_c * self.nskip;
            for j in 0..n_c {
                let v = self.dell[j] * self.d[j];
                self.ell[j] = v;
                self.l[ltgt + j] = v;
            }
            self.d[n_c] = 1.0f32 / (self.at(i, i) - dot(self.ell, self.dell, n_c));
        } else {
            self.d[0] = 1.0f32 / self.at(i, i);
        }
        self.swap_problem(n_c, i, true);
        self.c[n_c] = n_c as i32;
        self.n_n -= 1;
        self.n_c = n_c + 1;
    }

    /// `dLCP::transfer_i_from_C_to_N` @ 0x140394030.
    fn transfer_i_from_c_to_n(&mut self, i: usize) {
        // remove a row/column from the factorization, and adjust the indexes (black magic!)
        let n_c = self.n_c;
        let mut last_idx: Option<usize> = None;
        for j in 0..n_c {
            if self.c[j] as usize == n_c - 1 {
                last_idx = Some(j);
            }
            if self.c[j] as usize == i {
                ldlt_remove(self.a, self.arows, self.c, self.l, self.d, self.n, n_c, j, self.nskip, self.tmpbuf);
                let k = match last_idx {
                    None => {
                        let mut k = j + 1;
                        while k < n_c {
                            if self.c[k] as usize == n_c - 1 {
                                break;
                            }
                            k += 1;
                        }
                        k
                    }
                    Some(k) => k,
                };
                self.c[k] = self.c[j];
                if j < n_c - 1 {
                    self.c.copy_within(j + 1..n_c, j);
                }
                break;
            }
        }
        self.swap_problem(i, n_c - 1, true);
        self.n_n += 1;
        self.n_c = n_c - 1;
    }

    /// `dLCP::pN_equals_ANC_times_qC` (inlined into `dSolveLCP`).
    fn p_n_equals_anc_times_q_c(&self, p: &mut [f32], q: &[f32]) {
        let n_c = self.n_c;
        for i in 0..self.n_n {
            p[n_c + i] = dot(self.row(i + n_c), q, n_c);
        }
    }

    /// `dLCP::pN_plusequals_ANi` @ 0x1403932f0.
    fn p_n_plusequals_an_i(&self, p: &mut [f32], i: usize, sign: i32) {
        let n_c = self.n_c;
        let start = self.arows[i] * self.nskip + n_c;
        if sign > 0 {
            for j in 0..self.n_n {
                p[n_c + j] += self.a[start + j];
            }
        } else {
            for j in 0..self.n_n {
                p[n_c + j] -= self.a[start + j];
            }
        }
    }

    /// `dLCP::solve1` @ 0x1403936f0. `a` is `delta_x`.
    fn solve1(&mut self, a: &mut [f32], tmp: &mut [f32], i: usize, dir: i32, only_transfer: bool) {
        // the `Dell' and `ell' that are computed here are saved. if index i is later added
        // to the factorization then they can be reused.
        let n_c = self.n_c;
        if n_c > 0 {
            {
                let nub = self.nub;
                let start = self.arows[i] * self.nskip;
                // if nub>0, initial part of aptr[] is guaranteed unpermuted
                for j in 0..nub {
                    self.dell[j] = self.a[start + j];
                }
                for j in nub..n_c {
                    self.dell[j] = self.a[start + self.c[j] as usize];
                }
            }
            solve_l1(self.l, self.dell, n_c, self.nskip);
            for j in 0..n_c {
                self.ell[j] = self.dell[j] * self.d[j];
            }
            if !only_transfer {
                tmp[..n_c].copy_from_slice(&self.ell[..n_c]);
                solve_l1t(self.l, tmp, n_c, self.nskip);
                if dir > 0 {
                    for j in 0..n_c {
                        a[self.c[j] as usize] = -tmp[j];
                    }
                } else {
                    for j in 0..n_c {
                        a[self.c[j] as usize] = tmp[j];
                    }
                }
            }
        }
    }
}

/// `x == c` as compiled (`ucomiss` + `jne`, no parity test): also true when `x` is a NaN.
#[inline]
fn eq_or_nan(x: f32, c: f32) -> bool {
    x == c || x.is_nan()
}

/// `dSolveLCP` @ 0x140392260: solves the problem in place. `a` (`n` rows of `pad(n)`
/// values, lower triangle), `b`, `lo`, `hi` and `findex` are destroyed; the answer is in
/// `x`. `nub` is what the caller says about the first rows: they are unbounded.
#[allow(clippy::too_many_arguments)]
pub(crate) fn solve_lcp(
    mem: &mut LcpMemory,
    n: usize,
    a: &mut [f32],
    x: &mut [f32],
    b: &mut [f32],
    nub: usize,
    lo: &mut [f32],
    hi: &mut [f32],
    findex: Option<&mut [i32]>,
) -> LcpStats {
    let mut stats = LcpStats::default();
    let nskip = crate::common::pad(n);

    // if all the variables are unbounded then we can just factor, solve, and return
    if nub >= n {
        mem.d.clear();
        mem.d.resize(n, 0.0);
        factor_ldlt(a, &mut mem.d, n, nskip);
        solve_ldlt(a, &mem.d, b, n, nskip);
        x[..n].copy_from_slice(&b[..n]);
        return stats;
    }

    mem.l.clear();
    mem.l.resize(n * nskip, 0.0);
    mem.d.clear();
    mem.d.resize(n, 0.0);
    mem.w.clear();
    mem.w.resize(n, 0.0);
    mem.delta_w.clear();
    mem.delta_w.resize(n, 0.0);
    mem.delta_x.clear();
    mem.delta_x.resize(n, 0.0);
    mem.dell.clear();
    mem.dell.resize(n, 0.0);
    mem.ell.clear();
    mem.ell.resize(n, 0.0);
    mem.arows.clear();
    mem.arows.resize(n, 0);
    mem.p.clear();
    mem.p.resize(n, 0);
    mem.c.clear();
    mem.c.resize(n, 0);
    // for i in N, state[i] is 0 if x(i)==lo(i) or 1 if x(i)==hi(i)
    mem.state.clear();
    mem.state.resize(n, false);

    let LcpMemory { l, d, w, delta_w, delta_x, dell, ell, arows, p, c, state, tmpbuf } = mem;
    // create LCP object. note that tmp is set to delta_w to save space, this optimization
    // relies on knowledge of how tmp is used, so be careful!
    let mut lcp = Lcp {
        n,
        nskip,
        nub,
        n_c: 0,
        n_n: 0,
        a,
        arows,
        x,
        b,
        w,
        lo,
        hi,
        l,
        d,
        dell,
        ell,
        state,
        findex,
        p,
        c,
        tmpbuf,
    };
    lcp.init();
    let adj_nub = lcp.nub;
    stats.bounded_rows = (n - adj_nub) as u32;

    // loop over all indexes adj_nub..n-1. for index i, if x(i),w(i) satisfy the LCP
    // conditions then i is added to the appropriate index set. otherwise x(i),w(i) is
    // driven either +ve or -ve to force it to the valid region. as we drive x(i), x(C) is
    // also adjusted to keep w(C) at zero. while driving x(i) we maintain the LCP conditions
    // on the other variables 0..i-1. we do this by watching out for other x(i),w(i) values
    // going outside the valid region, and then switching them between index sets when that
    // happens.
    let mut hit_first_friction_index = false;
    for i in adj_nub..n {
        let mut s_error = false;
        // the index i is the driving index and indexes i+1..n-1 are "dont care", i.e. when
        // we make changes to the system those x's will be zero and we don't care what
        // happens to those w's. in other words, we only consider an (i+1)*(i+1) sub-problem
        // of A*x=b+w.

        // if we've hit the first friction index, we have to compute the lo and hi values
        // based on the values of x already computed. we have been permuting the indexes, so
        // the values stored in the findex vector are no longer valid. thus we have to
        // temporarily unpermute the x vector. for the purposes of this computation,
        // 0*infinity = 0 ... so if the contact constraint's normal force is 0, there should
        // be no tangential force applied.
        if !hit_first_friction_index && lcp.findex.as_ref().is_some_and(|f| f[i] >= 0) {
            // un-permute x into delta_w, which is not being used at the moment
            for j in 0..n {
                delta_w[lcp.p[j] as usize] = lcp.x[j];
            }
            // set lo and hi values
            let findex = lcp.findex.as_ref().unwrap();
            for k in i..n {
                let wfk = delta_w[findex[k] as usize];
                if eq_or_nan(wfk, 0.0) {
                    lcp.hi[k] = 0.0;
                    lcp.lo[k] = 0.0;
                } else {
                    let h = (wfk * lcp.hi[k]).abs();
                    lcp.hi[k] = h;
                    lcp.lo[k] = -h;
                }
            }
            hit_first_friction_index = true;
        }

        // thus far we have not even been computing the w values for indexes greater than i,
        // so compute w[i] now.
        {
            let n_c = lcp.n_c;
            let row = lcp.row(i);
            let aic = dot(row, lcp.x, n_c);
            let ain = dot(&row[n_c..], &lcp.x[n_c..], lcp.n_n);
            lcp.w[i] = aic + ain - lcp.b[i];
        }

        // if lo=hi=0 (which can happen for tangential friction when normals are 0) then the
        // index will be assigned to set N with some state. however, set C's line has zero
        // size, so the index will always remain in set N. with the "normal" switching
        // logic, if w changed sign then the index would have to switch to set C and then
        // back to set N with an inverted state. this is pointless, and also computationally
        // expensive. to prevent this from happening, we use the rule that indexes with
        // lo=hi=0 will never be checked for set changes. this means that the state for
        // these indexes may be incorrect, but that doesn't matter.

        // see if x(i),w(i) is in a valid region
        if eq_or_nan(lcp.lo[i], 0.0) && lcp.w[i] >= 0.0 {
            lcp.n_n += 1;
            lcp.state[i] = false;
        } else if eq_or_nan(lcp.hi[i], 0.0) && !(lcp.w[i] > 0.0) {
            lcp.n_n += 1;
            lcp.state[i] = true;
        } else if eq_or_nan(lcp.w[i], 0.0) {
            // this is a degenerate case. by the time we get to this test we know that
            // lo != 0, which means that lo < 0 as lo is not allowed to be +ve, and similarly
            // that hi > 0. this means that the line segment corresponding to set C is at
            // least finite in extent, and we are on it.
            // NOTE: we must call lcp.solve1() before lcp.transfer_i_to_C()
            lcp.solve1(delta_x, delta_w, i, 0, true);
            lcp.transfer_i_to_c(i);
        } else {
            // we must push x(i) and w(i)
            loop {
                stats.pivots += 1;
                // find direction to push on x(i)
                let (dir, dirf) = if lcp.w[i] <= 0.0 { (1i32, 1.0f32) } else { (-1i32, -1.0f32) };

                // compute: delta_x(C) = -dir*A(C,C)\A(C,i)
                lcp.solve1(delta_x, delta_w, i, dir, false);

                // note that delta_x[i] = dirf, but we wont bother to set it

                // compute: delta_w = A*delta_x ... note we only care about delta_w(N) and
                // delta_w(i), the rest is ignored
                lcp.p_n_equals_anc_times_q_c(delta_w, delta_x);
                lcp.p_n_plusequals_an_i(delta_w, i, dir);
                delta_w[i] = dirf * lcp.at(i, i) + dot(lcp.row(i), delta_x, lcp.n_c);

                // find largest step we can take (size=s), either to drive x(i),w(i) to the
                // valid LCP region or to drive an already-valid variable outside the valid
                // region.
                let mut cmd = 1; // index switching command
                let mut si = 0usize; // si = index to switch if cmd>3
                // (the source has -w[i] / delta_w[i]; the compiled code divides -1 first)
                let mut s = (-1.0f32 / delta_w[i]) * lcp.w[i];
                if dir > 0 {
                    if !(lcp.hi[i] >= f32::INFINITY) {
                        let s2 = (lcp.hi[i] - lcp.x[i]) * dirf; // step to x(i)=hi(i)
                        if !(s2 >= s) {
                            s = s2;
                            cmd = 3;
                        }
                    }
                } else if lcp.lo[i] > f32::NEG_INFINITY {
                    let s2 = (lcp.lo[i] - lcp.x[i]) * dirf; // step to x(i)=lo(i)
                    if !(s2 >= s) {
                        s = s2;
                        cmd = 2;
                    }
                }

                {
                    let n_c = lcp.n_c;
                    for k in 0..lcp.n_n {
                        let index_n_k = k + n_c;
                        let dw = delta_w[index_n_k];
                        let wants = if !lcp.state[index_n_k] { !(dw >= 0.0) } else { dw > 0.0 };
                        if wants {
                            // don't bother checking if lo=hi=0
                            if eq_or_nan(lcp.lo[index_n_k], 0.0) && eq_or_nan(lcp.hi[index_n_k], 0.0) {
                                continue;
                            }
                            let s2 = (-1.0f32 / dw) * lcp.w[index_n_k];
                            if !(s2 >= s) {
                                s = s2;
                                cmd = 4;
                                si = index_n_k;
                            }
                        }
                    }
                }

                {
                    for k in adj_nub..lcp.n_c {
                        let index_c_k = k;
                        let dx = delta_x[index_c_k];
                        if !(dx >= 0.0) && lcp.lo[index_c_k] > f32::NEG_INFINITY {
                            let s2 = (lcp.lo[index_c_k] - lcp.x[index_c_k]) / dx;
                            if !(s2 >= s) {
                                s = s2;
                                cmd = 5;
                                si = index_c_k;
                            }
                        }
                        if dx > 0.0 && !(lcp.hi[index_c_k] >= f32::INFINITY) {
                            let s2 = (lcp.hi[index_c_k] - lcp.x[index_c_k]) / dx;
                            if !(s2 >= s) {
                                s = s2;
                                cmd = 6;
                                si = index_c_k;
                            }
                        }
                    }
                }

                // if s <= 0 then we've got a problem. if we just keep going then we're
                // going to get stuck in an infinite loop. instead, just cross our fingers
                // and exit with the current solution.
                // (comiss s, 0 / jbe: a NaN step ends the solve too)
                if !(s > 0.0) {
                    // dMessage (d_ERR_LCP, "LCP internal error, s <= 0 (s=%.4e)"): the game
                    // prints it to stderr and goes on
                    for v in lcp.x[i..n].iter_mut() {
                        *v = 0.0;
                    }
                    for v in lcp.w[i..n].iter_mut() {
                        *v = 0.0;
                    }
                    s_error = true;
                    break;
                }

                // apply x = x + s * delta_x
                for k in 0..lcp.n_c {
                    lcp.x[k] += s * delta_x[k];
                }
                lcp.x[i] += s * dirf;

                // apply w = w + s * delta_w
                {
                    let n_c = lcp.n_c;
                    for k in 0..lcp.n_n {
                        lcp.w[n_c + k] += s * delta_w[n_c + k];
                    }
                }
                lcp.w[i] += s * delta_w[i];

                // switch indexes between sets if necessary
                match cmd {
                    1 => {
                        // done
                        lcp.w[i] = 0.0;
                        lcp.transfer_i_to_c(i);
                    }
                    2 => {
                        // done
                        lcp.x[i] = lcp.lo[i];
                        lcp.state[i] = false;
                        lcp.n_n += 1;
                    }
                    3 => {
                        // done
                        lcp.x[i] = lcp.hi[i];
                        lcp.state[i] = true;
                        lcp.n_n += 1;
                    }
                    4 => {
                        // keep going
                        lcp.w[si] = 0.0;
                        lcp.transfer_i_from_n_to_c(si);
                    }
                    5 => {
                        // keep going
                        lcp.x[si] = lcp.lo[si];
                        lcp.state[si] = false;
                        lcp.transfer_i_from_c_to_n(si);
                    }
                    _ => {
                        // keep going
                        lcp.x[si] = lcp.hi[si];
                        lcp.state[si] = true;
                        lcp.transfer_i_from_c_to_n(si);
                    }
                }

                if cmd <= 3 {
                    break;
                }
            }
        }

        if s_error {
            stats.s_error = true;
            break;
        }
    }

    // dLCP::unpermute: now we have to un-permute x and w
    delta_w[..n].copy_from_slice(&lcp.x[..n]);
    for j in 0..n {
        lcp.x[lcp.p[j] as usize] = delta_w[j];
    }
    delta_w[..n].copy_from_slice(&lcp.w[..n]);
    for j in 0..n {
        lcp.w[lcp.p[j] as usize] = delta_w[j];
    }
    stats
}
