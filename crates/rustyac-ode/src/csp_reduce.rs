// SPDX-License-Identifier: BSD-3-Clause

//! Custom Shaders Patch 0.3.2 (build 4305)'s contact reduction (+0x169f060), run on what a pair's
//! collider returned before contact joints are made. Contacts are put in up to 8 groups by
//! normal, the groups ranked by their deepest contact, and each kept group gives its deepest
//! contact and then the contacts farthest from those already kept, within per-group and total
//! limits. Every compare keeps CSP's NaN behaviour.

use crate::csp_btl::ContactGeom;

/// The limits: contacts per group, groups kept, total, and the normal dot that joins a group.
#[derive(Clone, Copy, Debug)]
pub struct Reduce {
    pub per_group: i32,
    pub groups: i32,
    pub total: i32,
    pub same_normal: f64,
}

/// CSP's default when no limits are passed.
pub const DEFAULT: Reduce = Reduce { per_group: 4, groups: 2, total: 8, same_normal: 0.95 };

fn d2(a: &[f64; 3], b: &[f64; 3]) -> f64 {
    let (x, y, z) = (a[0] - b[0], a[1] - b[1], a[2] - b[2]);
    y * y + x * x + z * z
}

/// Reduces `c` in place and returns the new count (CSP's caller then uses at least 1).
pub fn reduce(c: &mut Vec<ContactGeom>, p: &Reduce) -> usize {
    let per = p.per_group.max(1);
    let groups = p.groups.clamp(1, 8);
    let total = p.total.max(1);
    if c.is_empty() {
        return 0;
    }
    let n = c.len().min(256);
    if n as u32 <= per as u32 && n as u32 <= total as u32 {
        return n;
    }
    let mut normal = [[0f64; 3]; 8];
    let mut count = [0i32; 8];
    let mut maxd = [0f64; 8];
    let mut m = 0usize;
    let mut group = vec![0usize; n];
    for (i, ci) in c[..n].iter().enumerate() {
        let [nx, ny, nz] = ci.normal;
        // first group whose normal is close enough (a NaN dot joins none)
        let mut k = (0..m).find(|&k| ny * normal[k][1] + nx * normal[k][0] + nz * normal[k][2] >= p.same_normal).unwrap_or(m);
        if k == m {
            if m >= 8 {
                let dot = |k: usize| nx * normal[k][0] + ny * normal[k][1] + nz * normal[k][2];
                let mut best = dot(0);
                k = 0;
                for j in 1..8 {
                    if dot(j) > best {
                        best = dot(j);
                        k = j;
                    }
                }
            } else {
                normal[m] = ci.normal;
                count[m] = 0;
                maxd[m] = ci.depth;
                m += 1;
            }
        }
        group[i] = k;
        count[k] += 1;
        if ci.depth > maxd[k] {
            maxd[k] = ci.depth;
        }
    }
    // selection sort by deepest contact, deepest first, CSP's unrolled compares
    let mut order: Vec<usize> = (0..m).collect();
    for p0 in 0..m.saturating_sub(1) {
        let mut best = p0;
        let mut j = p0 + 1;
        let md = |o: &[usize], i: usize| maxd[o[i]];
        if m - 1 - p0 >= 4 {
            while j + 3 < m {
                if md(&order, j) > md(&order, best) {
                    best = j;
                }
                if md(&order, j + 1) > md(&order, best) {
                    best = j + 1;
                }
                // these two take a NaN, the others do not
                if !(md(&order, best) >= md(&order, j + 2)) {
                    best = j + 2;
                }
                if !(md(&order, best) >= md(&order, j + 3)) {
                    best = j + 3;
                }
                j += 4;
            }
        }
        while j < m {
            if md(&order, j) > md(&order, best) {
                best = j;
            }
            j += 1;
        }
        order.swap(p0, best);
    }
    let mut used = m.min(groups as usize);
    let mut quota = [0i32; 8];
    let mut sum = 0;
    for &g in &order[..used] {
        quota[g] = count[g].min(per);
        sum += quota[g];
    }
    if sum > total {
        loop {
            let mut changed = false;
            for &g in order[..used].iter().rev() {
                if sum <= total {
                    break;
                }
                if quota[g] > 1 {
                    changed = true;
                    quota[g] -= 1;
                    sum -= 1;
                }
            }
            if sum <= total {
                break;
            }
            if !changed {
                while used > 1 {
                    used -= 1;
                    sum -= quota[order[used]];
                    if sum <= total {
                        break;
                    }
                }
                break;
            }
        }
    }
    let mut keep = vec![false; n];
    let mut dist = vec![0f64; n];
    for &g in &order[..used] {
        let mut deep: Option<usize> = None;
        let mut dd = 0.0;
        for i in 0..n {
            if group[i] == g && (deep.is_none() || c[i].depth > dd) {
                deep = Some(i);
                dd = c[i].depth;
            }
        }
        let Some(d) = deep else { continue };
        keep[d] = true;
        let pd = c[d].pos;
        for i in 0..n {
            dist[i] = if group[i] != g || keep[i] { -1.0 } else { d2(&c[i].pos, &pd) };
        }
        for _ in 1..quota[g] {
            let mut far: Option<usize> = None;
            let mut fd = 0.0;
            for (i, &di) in dist.iter().enumerate() {
                if di > fd {
                    fd = di;
                    far = Some(i);
                }
            }
            let Some(f) = far else { break };
            keep[f] = true;
            dist[f] = -1.0;
            let pf = c[f].pos;
            for i in 0..n {
                if dist[i] >= 0.0 {
                    let e = d2(&c[i].pos, &pf);
                    if !(e >= dist[i]) {
                        dist[i] = e;
                    }
                }
            }
        }
    }
    let mut w = 0;
    for i in 0..n {
        if keep[i] {
            c.swap(w, i);
            w += 1;
        }
    }
    c.truncate(w);
    w
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(x: f64, nz: f64, depth: f64) -> ContactGeom {
        ContactGeom { pos: [x, 0.0, 0.0], normal: [0.0, 0.0, nz], depth, g1: 0, g2: 0, side1: 0, side2: 0 }
    }

    #[test]
    fn keeps_deepest_then_farthest() {
        let mut c: Vec<_> = (0..6).map(|i| at(i as f64, 1.0, if i == 2 { 0.5 } else { 0.1 })).collect();
        c.push(at(9.0, -1.0, 0.2));
        let p = Reduce { per_group: 2, groups: 1, total: 4, same_normal: 0.95 };
        let n = reduce(&mut c, &p);
        assert_eq!(n, 2);
        assert_eq!(c.iter().map(|c| c.pos[0]).collect::<Vec<_>>(), vec![2.0, 5.0]);
    }
}
