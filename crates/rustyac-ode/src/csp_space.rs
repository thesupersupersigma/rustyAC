// SPDX-License-Identifier: BSD-3-Clause

//! ODE's hash space as Custom Shaders Patch 0.3.2 (build 4305) rewrote it, the track's space in
//! CSP's collision world: the table build (+0x1688ab0) and `collide2` (+0x1689bf0), the query of
//! one geom against everything in the space. Every geom gets a level (the binary exponent of its
//! largest side, at least the space's lowest level) and the cells of that size it covers; a geom
//! above the highest level (or with an endless box) goes on a list tested against every query.
//! A query walks, level by level, the cells its own box covers, or every geom of the level when
//! that is fewer, and reports a geom in the first cell both cover, so each once.
//!
//! The space hands out candidates in CSP's order; the tests (the query geom enabled, then per
//! pair: enabled, bodies, categories, [`overlap`], `AABBTest`) are the caller's, as they do not
//! change what comes next.

/// The level of a geom kept on the list every query tests.
pub const BIG: i32 = i32::MAX;

/// A geom's entry (48 bytes in CSP): its level and the cells it covers (min x, max x, min y, max
/// y, min z, max z), and which geom (its place in the space's child list).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Entry {
    pub level: i32,
    pub cells: [i32; 6],
    pub geom: usize,
}

/// A cell of a geom (20 bytes in CSP): the next in the bucket's chain, the entry, the cell.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Node {
    pub next: u32,
    pub entry: u32,
    pub cell: [i32; 3],
}

/// The tables CSP keeps from +0xe0 on.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct HashSpace {
    /// +0xe0: geoms with a level, in child order
    pub entries: Vec<Entry>,
    /// +0xf8: geoms tested against every query
    pub big: Vec<usize>,
    /// +0x110
    pub nodes: Vec<Node>,
    /// +0x128: the first node of every bucket, `u32::MAX` for none
    pub heads: Vec<u32>,
    /// +0x140: the levels in use, rising
    pub levels: Vec<i32>,
    /// +0x158: one over each level's cell size
    pub scales: Vec<f64>,
    /// +0x170
    pub mask: u32,
    /// +0x178: entry numbers, level by level
    pub order: Vec<u32>,
    /// +0x190: where each level starts in `order`, and the end
    pub ranges: Vec<u32>,
}

/// The box test of `collide2` and of ODE's `collideAABBs` (+0x16880b0): `q` (the second geom's
/// box) against `a`, as CSP compiled it: a NaN in `q`'s max side fails, one in its min side passes.
pub fn overlap(q: &[f64; 6], a: &[f64; 6]) -> bool {
    (0..3).all(|k| q[2 * k + 1] >= a[2 * k] && !(q[2 * k] > a[2 * k + 1]))
}

/// `frexp`'s exponent as the CRT gives it (-1 for inf and NaN).
fn exponent(x: f64) -> i32 {
    let bits = x.to_bits();
    let biased = ((bits >> 52) & 0x7ff) as i32;
    if biased == 0x7ff {
        -1
    } else if x == 0.0 {
        0
    } else if biased != 0 {
        biased - 1022
    } else {
        63 - (bits & 0xf_ffff_ffff_ffff).leading_zeros() as i32 - 1073
    }
}

/// `ldexp(1.0, level)`.
fn cell_size(level: i32) -> f64 {
    // shortcut: levels -1022..=1023 only (Spa uses 2..=11), the CRT's error paths when other levels come up
    assert!((-1022..=1023).contains(&level), "hash space level {level}");
    f64::from_bits(((level + 1023) as u64) << 52)
}

/// `floor(scale * v)` as an int, `None` out of int range or NaN (CSP's own floor).
fn cell(scale: f64, v: f64) -> Option<i32> {
    let x = scale * v;
    if !(x >= -2147483648.0) || x >= 2147483647.0 {
        return None;
    }
    let mut t = x as i64;
    if t as f64 != x {
        t -= x.is_sign_negative() as i64;
    }
    Some(t as i32)
}

fn cells(scale: f64, aabb: &[f64; 6]) -> Option<[i32; 6]> {
    let mut out = [0; 6];
    for k in 0..6 {
        out[k] = cell(scale, aabb[k])?;
    }
    Some(out)
}

/// The bucket hash of a cell column: x, y and the level mixed (`+ z`, masked, picks the bucket).
fn mix(x: i32, y: i32, level: i32) -> u32 {
    let a = (73_856_093i32.wrapping_mul(x) ^ 83_492_791i32.wrapping_mul(level) ^ 19_349_663i32.wrapping_mul(y)) as u32;
    let b = 2_146_121_005u32.wrapping_mul(a ^ (a >> 16));
    b ^ (b >> 15)
}

impl HashSpace {
    /// The tables for children with boxes `aabbs` (min x, max x, min y, max y, min z, max z) and
    /// the space's lowest and highest level (+0xd0, +0xd4).
    pub fn build(aabbs: &[[f64; 6]], min_level: i32, max_level: i32) -> HashSpace {
        let mut s = HashSpace::default();
        let mut total = 0usize;
        for (geom, a) in aabbs.iter().enumerate() {
            let mut level = if !(a[0] > -1e300) || a[1] >= 1e300 || !(a[2] > -1e300) || a[3] >= 1e300 || !(a[4] > -1e300) || a[5] >= 1e300 {
                BIG
            } else {
                let (dx, dy, dz) = (a[1] - a[0], a[3] - a[2], a[5] - a[4]);
                let m = if dy > dx { dy } else { dx };
                exponent(if m > dz { m } else { dz })
            };
            level = level.max(min_level);
            let found = if level <= max_level { cells(1.0 / cell_size(level), a) } else { None };
            let Some(c) = found else {
                s.big.push(geom);
                continue;
            };
            total += (c[1] - c[0] + 1) as usize * (c[3] - c[2] + 1) as usize * (c[5] - c[4] + 1) as usize;
            if let Err(at) = s.levels.binary_search(&level) {
                s.levels.insert(at, level);
            }
            s.entries.push(Entry { level, cells: c, geom });
        }
        s.scales = s.levels.iter().map(|&l| 1.0 / cell_size(l)).collect();
        for &level in &s.levels {
            s.ranges.push(s.order.len() as u32);
            s.order.extend((0..s.entries.len() as u32).filter(|&e| s.entries[e as usize].level == level));
        }
        s.ranges.push(s.order.len() as u32);
        let mut buckets = 16u32;
        while (buckets as usize) < 8 * s.entries.len() && buckets < 0x4000_0000 {
            buckets *= 2;
        }
        s.mask = buckets - 1;
        s.heads = vec![u32::MAX; buckets as usize];
        s.nodes.reserve(total);
        for (e, entry) in s.entries.iter().enumerate() {
            let c = entry.cells;
            for x in c[0]..=c[1] {
                for y in c[2]..=c[3] {
                    let m = mix(x, y, entry.level);
                    for z in c[4]..=c[5] {
                        let bucket = (s.mask & m.wrapping_add(z as u32)) as usize;
                        s.nodes.push(Node { next: s.heads[bucket], entry: e as u32, cell: [x, y, z] });
                        s.heads[bucket] = s.nodes.len() as u32 - 1;
                    }
                }
            }
        }
        s
    }

    /// The children a geom with box `aabb` is tested against, in CSP's order (`visit` gets the
    /// child's place in the list).
    pub fn collide2(&self, aabb: &[f64; 6], mut visit: impl FnMut(usize)) {
        self.big.iter().for_each(|&g| visit(g));
        if self.entries.is_empty() {
            return;
        }
        for (i, &level) in self.levels.iter().enumerate() {
            let (start, end) = (self.ranges[i], self.ranges[i + 1]);
            let walk = cells(self.scales[i], aabb).filter(|c| {
                ((c[1] as f64 - c[0] as f64 + 1.0) * (c[3] as f64 - c[2] as f64 + 1.0)) * (c[5] as f64 - c[4] as f64 + 1.0) < (end - start) as f64
            });
            let Some(c) = walk else {
                (start..end).for_each(|k| visit(self.entries[self.order[k as usize] as usize].geom));
                continue;
            };
            for x in c[0]..=c[1] {
                for y in c[2]..=c[3] {
                    let m = mix(x, y, level);
                    for z in c[4]..=c[5] {
                        let mut n = self.heads[(self.mask & m.wrapping_add(z as u32)) as usize];
                        while n != u32::MAX {
                            let node = &self.nodes[n as usize];
                            let e = &self.entries[node.entry as usize];
                            // a geom is reported in the first cell both boxes cover
                            if e.level == level && node.cell == [x, y, z] && x == c[0].max(e.cells[0]) && y == c[2].max(e.cells[2]) && z == c[4].max(e.cells[4]) {
                                visit(e.geom);
                            }
                            n = node.next;
                        }
                    }
                }
            }
        }
    }

    /// The pairs of the space's own collide (+0x16894f0) in CSP's order, as (child, child): each
    /// entry walks its cells on its own level and, shifted, on every higher one, and meets an
    /// entry of that level in the first cell both cover (on its own level only the later
    /// entries); then every entry against every big geom, then the big geoms among themselves.
    pub fn self_pairs(&self, mut visit: impl FnMut(usize, usize)) {
        for (i, e) in self.entries.iter().enumerate() {
            for &level in self.levels.iter().filter(|&&l| l >= e.level) {
                let c = e.cells.map(|x| x >> (level - e.level));
                for x in c[0]..=c[1] {
                    for y in c[2]..=c[3] {
                        let m = mix(x, y, level);
                        for z in c[4]..=c[5] {
                            let mut n = self.heads[(self.mask & m.wrapping_add(z as u32)) as usize];
                            while n != u32::MAX {
                                let node = &self.nodes[n as usize];
                                let (j, o) = (node.entry as usize, &self.entries[node.entry as usize]);
                                if o.level == level && node.cell == [x, y, z] && j != i && x == c[0].max(o.cells[0]) && y == c[2].max(o.cells[2]) && z == c[4].max(o.cells[4]) && (level != e.level || j > i) {
                                    visit(e.geom, o.geom);
                                }
                                n = node.next;
                            }
                        }
                    }
                }
            }
        }
        for e in &self.entries {
            self.big.iter().for_each(|&b| visit(e.geom, b));
        }
        for (k, &a) in self.big.iter().enumerate() {
            self.big[k + 1..].iter().for_each(|&b| visit(a, b));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_geom_once_and_in_level_order() {
        let small = [0.5, 1.0, 0.5, 1.0, 0.5, 1.0];
        let wide = [-40.0, 40.0, -1.0, 1.0, -1.0, 1.0];
        let s = HashSpace::build(&[wide, small, [-1e301, 0.0, 0.0, 1.0, 0.0, 1.0]], 2, 11);
        assert_eq!(s.big, vec![2]);
        assert_eq!(s.levels, vec![2, 7]);
        let mut seen = Vec::new();
        s.collide2(&[-50.0, 50.0, -2.0, 2.0, -2.0, 2.0], |g| seen.push(g));
        assert_eq!(seen, vec![2, 1, 0]);
        assert_eq!((exponent(1.0), exponent(0.75), exponent(f64::MIN_POSITIVE / 4.0)), (1, 0, -1023));
        let mut pairs = Vec::new();
        HashSpace::build(&[small, [0.75, 1.5, 0.5, 1.0, 0.5, 1.0], wide], 2, 11).self_pairs(|a, b| pairs.push((a, b)));
        assert_eq!(pairs, vec![(0, 1), (0, 2), (1, 2)]);
    }
}
