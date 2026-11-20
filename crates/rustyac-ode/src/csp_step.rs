// SPDX-License-Identifier: BSD-3-Clause

//! ODE master 66615b8a `dxStepIsland` (`step.cpp`, one thread) in f64 as Custom Shaders Patch
//! 0.3.2 (build 4305) compiled it (+0x16c24b0, stages +0x16c26d0 .. +0x16c5370). An island is a
//! slice of bodies and the joints between them; the joints name their bodies by index.

use crate::common::{Quaternion, Vector3};
use crate::csp_contact::{self as contact, Contact};
use crate::csp_joints::dball_get_info2;
use crate::csp_lcp::{pad, solve_lcp, D_INFINITY};
use crate::joint::{ball_get_info2, fixed_get_info2, Info2};
use crate::step::step_body;
use crate::world::Body;

/// The world settings a step reads.
#[derive(Clone, Copy, Debug)]
pub struct WorldParams {
    pub gravity: [f64; 3],
    pub erp: f64,
    pub cfm: f64,
    /// `contactp.max_vel` / `contactp.min_depth`
    pub max_vel: f64,
    pub min_depth: f64,
}

#[derive(Clone, Debug)]
pub enum JointKind {
    Ball { anchor1: Vector3<f64>, anchor2: Vector3<f64>, erp: f64, cfm: f64 },
    /// master's path only (CSP's field +0x110 == -1)
    Fixed { qrel: Quaternion<f64>, offset: Vector3<f64>, erp: f64, cfm: f64 },
    DBall { anchor1: Vector3<f64>, anchor2: Vector3<f64>, erp: f64, cfm: f64, target_distance: f64 },
    /// `the_m` is written by getInfo1
    Contact { contact: Contact, reverse: bool, the_m: u32 },
}

#[derive(Clone, Debug)]
pub struct Joint {
    pub kind: JointKind,
    pub b0: usize,
    pub b1: Option<usize>,
    /// `dJointFeedback` f1, t1, f2, t2 (four values each), written by the step when set
    pub feedback: Option<[f64; 16]>,
}

/// dxBody flags the step reads.
const NO_GRAVITY: u32 = 8;
const GYROSCOPIC: u32 = 0x100;
const BODY_GYRO_MASK: u32 = GYROSCOPIC;

fn dot3(a: &[f64], sa: usize, b: &[f64], sb: usize) -> f64 {
    a[0] * b[0] + a[sa] * b[sb] + a[2 * sa] * b[2 * sb]
}

/// `dMultiply0_331`: R b
fn mul0_331(a: &[f64], b: &[f64]) -> [f64; 3] {
    [dot3(a, 1, b, 1), dot3(&a[4..], 1, b, 1), dot3(&a[8..], 1, b, 1)]
}

/// `dMultiply1_331`: Rᵀ b
fn mul1_331(a: &[f64], b: &[f64]) -> [f64; 3] {
    [dot3(a, 4, b, 1), dot3(&a[1..], 4, b, 1), dot3(&a[2..], 4, b, 1)]
}

/// `dMultiply0_333`: A B (rows by `_dMultiplyHelper0_133` = Bᵀ row)
fn mul0_333(a: &[f64], b: &[f64]) -> [f64; 12] {
    let mut r = [0.0; 12];
    for row in 0..3 {
        r[4 * row..4 * row + 3].copy_from_slice(&mul1_331(b, &a[4 * row..]));
    }
    r
}

/// `dMultiply2_333`: A Bᵀ
fn mul2_333(a: &[f64], b: &[f64]) -> [f64; 12] {
    let mut r = [0.0; 12];
    for row in 0..3 {
        r[4 * row..4 * row + 3].copy_from_slice(&mul0_331(b, &a[4 * row..]));
    }
    r
}

/// `dInvertMatrix3`: the determinant (0 leaves `dst` alone)
fn invert3(dst: &mut [f64; 12], ma: &[f64; 12]) -> f64 {
    let det = ma[0] * (ma[5] * ma[10] - ma[9] * ma[6]) - ma[1] * (ma[4] * ma[10] - ma[8] * ma[6]) + ma[2] * (ma[4] * ma[9] - ma[8] * ma[5]);
    if det == 0.0 {
        return 0.0;
    }
    let r = 1.0 / det;
    dst[0] = (ma[5] * ma[10] - ma[6] * ma[9]) * r;
    dst[1] = (ma[9] * ma[2] - ma[1] * ma[10]) * r;
    dst[2] = (ma[1] * ma[6] - ma[5] * ma[2]) * r;
    dst[4] = (ma[6] * ma[8] - ma[4] * ma[10]) * r;
    dst[5] = (ma[0] * ma[10] - ma[8] * ma[2]) * r;
    dst[6] = (ma[4] * ma[2] - ma[0] * ma[6]) * r;
    dst[8] = (ma[4] * ma[9] - ma[8] * ma[5]) * r;
    dst[9] = (ma[8] * ma[1] - ma[0] * ma[9]) * r;
    dst[10] = (ma[0] * ma[5] - ma[1] * ma[4]) * r;
    det
}

/// `dxStepIsland_Stage0_Bodies` for one body: gravity was added; the world inverse inertia and
/// the implicit gyroscopic torque.
fn inertia(b: &mut Body<f64>, h: f64) -> [f64; 12] {
    let tmp = mul2_333(&b.inv_i, &b.r);
    let inv_i = mul0_333(&b.r, &tmp);
    if b.flags & BODY_GYRO_MASK != 0 && b.inv_mass > 0.0 {
        let tmp = mul2_333(&b.mass.i, &b.r);
        let i = mul0_333(&b.r, &tmp);
        let mut l = mul0_331(&i, &b.avel);
        let mut itild = [0.0; 12];
        itild[1] = l[2];
        itild[2] = -l[1];
        itild[4] = -l[2];
        itild[6] = l[0];
        itild[8] = l[1];
        itild[9] = -l[0];
        for ii in 0..12 {
            itild[ii] = itild[ii] * h + i[ii];
        }
        let hr = 1.0 / h;
        l = [l[0] * hr, l[1] * hr, l[2] * hr];
        let mut it_inv = [0.0; 12];
        if invert3(&mut it_inv, &itild) != 0.0 {
            let mut itild = mul0_333(&i, &it_inv);
            itild[0] -= 1.0;
            itild[5] -= 1.0;
            itild[10] -= 1.0;
            // CSP adds tau0 = Itild L into tacc as (two products) + (one product + tacc)
            let p = |i: usize, j: usize| itild[4 * i + j] * l[j];
            b.tacc[0] = (p(0, 2) + p(0, 1)) + (p(0, 0) + b.tacc[0]);
            b.tacc[1] = (p(1, 2) + p(1, 0)) + (p(1, 1) + b.tacc[1]);
            b.tacc[2] = (p(2, 2) + p(2, 0)) + (p(2, 1) + b.tacc[2]);
        }
    }
    inv_i
}

/// getInfo1: (m, nub)
fn info1(j: &mut Joint) -> (u32, u32) {
    match &mut j.kind {
        JointKind::Ball { .. } => (3, 3),
        JointKind::Fixed { .. } => (6, 6),
        JointKind::DBall { .. } => (1, 1),
        JointKind::Contact { contact: c, the_m, .. } => {
            let (m, nub) = contact::get_info1(c);
            *the_m = m;
            (m, nub)
        }
    }
}

/// `dxStepIsland_Stage0_Joints`' ordering: the joint indices (unbounded, mixed, then LCP
/// ones; m = 0 dropped) with their (m, nub), and the "nub" master hands the solver (the count of
/// unbounded joints, not rows).
fn order(joints: &mut [Joint]) -> (Vec<(usize, u32, u32)>, u32, u32) {
    let nj = joints.len();
    let mut ji: Vec<(usize, u32, u32)> = vec![(usize::MAX, 0, 0); 2 * nj];
    let (mut unb_start, mut mix_start, mut mix_end, mut lcp_end) = (nj, nj, nj, nj);
    let mut jicurr = lcp_end as isize;
    let mut mcurr = 0;
    let mut src = 0;
    'outer: loop {
        // forward
        {
            let mut jimixend = mix_end;
            loop {
                if src == nj {
                    lcp_end = jicurr as usize;
                    break 'outer;
                }
                let j = src;
                src += 1;
                let (m, nub) = info1(&mut joints[j]);
                let cur = jicurr as usize;
                ji[cur] = (usize::MAX, m, nub);
                if m != 0 {
                    mcurr += m;
                    if nub == 0 {
                        ji[cur].0 = j;
                        jicurr += 1;
                    } else if nub < m {
                        if unb_start == mix_start {
                            unb_start -= 1;
                            mix_start -= 1;
                            ji[mix_start] = (j, m, nub);
                        } else if jimixend != cur {
                            let tmp = ji[cur];
                            ji[cur] = ji[jimixend];
                            ji[jimixend] = (j, tmp.1, tmp.2);
                            jimixend += 1;
                            jicurr += 1;
                        } else {
                            ji[cur].0 = j;
                            jicurr += 1;
                            jimixend = jicurr as usize;
                        }
                    } else {
                        unb_start -= 1;
                        ji[unb_start] = (j, m, nub);
                        lcp_end = cur;
                        mix_end = jimixend;
                        jicurr = unb_start as isize - 1;
                        break;
                    }
                }
            }
        }
        // backward
        {
            let mut jimixstart = mix_start as isize - 1;
            loop {
                if src == nj {
                    unb_start = (jicurr + 1) as usize;
                    mix_start = (jimixstart + 1) as usize;
                    break 'outer;
                }
                let j = src;
                src += 1;
                let (m, nub) = info1(&mut joints[j]);
                let cur = jicurr as usize;
                ji[cur] = (usize::MAX, m, nub);
                if m != 0 {
                    mcurr += m;
                    if nub == m {
                        ji[cur].0 = j;
                        jicurr -= 1;
                    } else if nub != 0 {
                        if mix_end == lcp_end {
                            ji[mix_end] = (j, m, nub);
                            mix_end += 1;
                            lcp_end = mix_end;
                        } else if jimixstart != jicurr {
                            let tmp = ji[cur];
                            ji[cur] = ji[jimixstart as usize];
                            ji[jimixstart as usize] = (j, tmp.1, tmp.2);
                            jimixstart -= 1;
                            jicurr -= 1;
                        } else {
                            ji[cur].0 = j;
                            jicurr -= 1;
                            jimixstart = jicurr;
                        }
                    } else {
                        ji[lcp_end] = (j, m, nub);
                        lcp_end += 1;
                        unb_start = (jicurr + 1) as usize;
                        mix_start = (jimixstart + 1) as usize;
                        jicurr = lcp_end as isize;
                        break;
                    }
                }
            }
        }
    }
    let nub = (mix_start - unb_start) as u32;
    (ji[unb_start..lcp_end].to_vec(), mcurr, nub)
}

/// What `dxSolveLCP` is handed (for checks): m, nub, A, pairsbx, pairslh, findex, and the bodies
/// (their force accumulators hold gravity and the gyroscopic torque by then).
pub struct LcpInputs<'a> {
    pub m: usize,
    pub nub: usize,
    pub a: &'a [f64],
    pub bx: &'a [f64],
    pub lh: &'a [f64],
    pub findex: &'a [i32],
    pub bodies: &'a [Body<f64>],
}

/// `dxStepIsland` for one thread: steps the bodies by `h`.
pub fn step_island(world: &WorldParams, bodies: &mut [Body<f64>], joints: &mut [Joint], h: f64) {
    step_island_probed(world, bodies, joints, h, &mut |_| {});
}

/// [`step_island`], `probe` seeing the solver's inputs.
pub fn step_island_probed(world: &WorldParams, bodies: &mut [Body<f64>], joints: &mut [Joint], h: f64, probe: &mut dyn FnMut(&LcpInputs)) {
    let nb = bodies.len();
    // Stage0 bodies: gravity, one component at a time
    for k in 0..3 {
        let g = world.gravity[k];
        if g != 0.0 {
            for b in bodies.iter_mut().filter(|b| b.flags & NO_GRAVITY == 0) {
                b.facc[k] += b.mass.mass * g;
            }
        }
    }
    let inv_i: Vec<[f64; 12]> = bodies.iter_mut().map(|b| inertia(b, h)).collect();

    // Stage0 joints
    let (infos, m, nub) = order(joints);
    let nj = infos.len();
    let mut tag = vec![-1i32; joints.len()];
    for (i, &(j, ..)) in infos.iter().enumerate() {
        tag[j] = i as i32;
    }
    let m = m as usize;
    let mut mindex = vec![0usize; nj + 1];
    for (i, &(_, jm, _)) in infos.iter().enumerate() {
        mindex[i + 1] = mindex[i] + jm as usize;
    }

    let recip = 1.0 / h;
    let mut lambda = vec![0.0; m];
    let mut j_all = vec![0.0; 2 * 8 * m];
    if m > 0 {
        // Stage2a: the rows
        let (mut rhs, mut cfm, mut lo, mut hi, mut findex) = (vec![0.0; m], vec![world.cfm; m], vec![-D_INFINITY; m], vec![D_INFINITY; m], vec![-1i32; m]);
        for (i, &(j, ..)) in infos.iter().enumerate() {
            let (ofs, im) = (mindex[i], mindex[i + 1] - mindex[i]);
            let (j1, j2) = j_all[16 * ofs..16 * (ofs + im)].split_at_mut(8 * im);
            let joint = &joints[j];
            let b0 = &bodies[joint.b0];
            let b1 = joint.b1.map(|k| &bodies[k]);
            let mut info = Info2 { j1, j2, c: &mut rhs[ofs..ofs + im], cfm: &mut cfm[ofs..ofs + im], lo: &mut lo[ofs..ofs + im], hi: &mut hi[ofs..ofs + im], findex: &mut findex[ofs..ofs + im] };
            match &joint.kind {
                JointKind::Ball { anchor1, anchor2, erp, cfm } => ball_get_info2(b0, b1, recip, *erp, *cfm, &mut info, anchor1, anchor2),
                JointKind::Fixed { qrel, offset, erp, cfm } => fixed_get_info2(b0, b1, recip, world.erp, qrel, offset, *erp, *cfm, &mut info),
                JointKind::DBall { anchor1, anchor2, erp, cfm, target_distance } => dball_get_info2(b0, b1, recip, anchor1, anchor2, *erp, *cfm, *target_distance, &mut info),
                JointKind::Contact { contact, reverse, the_m } => contact::get_info2(contact, *the_m, *reverse, b0, b1, recip, world.erp, world.max_vel, world.min_depth, &mut info),
            }
            for f in &mut findex[ofs..ofs + im] {
                if *f != -1 {
                    *f += ofs as i32;
                }
            }
            for r in ofs..ofs + im {
                rhs[r] *= recip;
                cfm[r] *= recip;
            }
        }

        // Stage2b: A's diagonal, J invM, rhs_tmp
        let mskip = pad(m);
        let mut a = vec![0.0; m * mskip];
        for r in 0..m {
            a[r * mskip + r] = cfm[r];
        }
        let mut jinvm = vec![0.0; 2 * 8 * m];
        for (i, &(j, ..)) in infos.iter().enumerate() {
            let (ofs, im) = (mindex[i], mindex[i + 1] - mindex[i]);
            let joint = &joints[j];
            for (side, b) in [Some(joint.b0), joint.b1].into_iter().enumerate() {
                let Some(b) = b else { continue };
                let (im_b, ii) = (bodies[b].inv_mass, &inv_i[b]);
                for r in 0..im {
                    let at = 16 * ofs + 8 * (side * im + r);
                    let src = &j_all[at..at + 8];
                    let mut d = [0.0; 8];
                    for k in 0..3 {
                        d[k] = src[k] * im_b;
                    }
                    d[4..7].copy_from_slice(&mul1_331(ii, &src[4..]));
                    jinvm[at..at + 8].copy_from_slice(&d);
                }
            }
        }
        let rhs_tmp: Vec<[f64; 6]> = bodies
            .iter()
            .zip(&inv_i)
            .map(|(b, ii)| {
                let mut t = [0.0; 6];
                for k in 0..3 {
                    t[k] = b.facc[k] * b.inv_mass + b.lvel[k] * recip;
                }
                t[3..].copy_from_slice(&mul0_331(ii, &b.tacc));
                for k in 0..3 {
                    t[3 + k] += b.avel[k] * recip;
                }
                t
            })
            .collect();

        // Stage2c: A = J invM Jᵀ (lower blocks), rhs -= J rhs_tmp
        let add = |a: &mut [f64], at: usize, jm: &[f64], jr: &[f64], rows: usize, cols: usize| {
            for r in 0..rows {
                let x = &jm[8 * r..8 * r + 8];
                for c in 0..cols {
                    let y = &jr[8 * c..8 * c + 8];
                    // CSP: A = ((AY + AX) + (AZ + A)) + ((LZ + LX) + LY)
                    let p = |k: usize| x[k] * y[k];
                    let e = &mut a[at + r * mskip + c];
                    *e = ((p(5) + p(4)) + (p(6) + *e)) + ((p(2) + p(0)) + p(1));
                }
            }
        };
        for (i, &(j, ..)) in infos.iter().enumerate() {
            let (ofs, im) = (mindex[i], mindex[i + 1] - mindex[i]);
            let joint = &joints[j];
            for (side, b) in [Some(joint.b0), joint.b1].into_iter().enumerate() {
                let Some(b) = b else { continue };
                let jm = &jinvm[16 * ofs + 8 * side * im..];
                add(&mut a, ofs * mskip + ofs, jm, &j_all[16 * ofs + 8 * side * im..], im, im);
                if i == 0 {
                    continue;
                }
                for (j0, other) in joints.iter().enumerate() {
                    let t = tag[j0];
                    if t == -1 || t as usize >= i || (other.b0 != b && other.b1 != Some(b)) {
                        continue;
                    }
                    let t = t as usize;
                    let (o_ofs, o_im) = (mindex[t], mindex[t + 1] - mindex[t]);
                    let smart = if other.b1 == Some(b) { o_im } else { 0 };
                    add(&mut a, ofs * mskip + o_ofs, jm, &j_all[(2 * o_ofs + smart) * 8..], im, o_im);
                }
            }
        }
        for (i, &(j, ..)) in infos.iter().enumerate() {
            let (ofs, im) = (mindex[i], mindex[i + 1] - mindex[i]);
            let joint = &joints[j];
            for (side, b) in [Some(joint.b0), joint.b1].into_iter().enumerate() {
                let Some(b) = b else { continue };
                let t = &rhs_tmp[b];
                for r in 0..im {
                    let x = &j_all[16 * ofs + 8 * (side * im + r)..];
                    // CSP: rhs -= ((AZ + AY) + AX) + ((LX + LZ) + LY)
                    let p = |k: usize, l: usize| x[k] * t[l];
                    rhs[ofs + r] -= ((p(6, 5) + p(5, 4)) + p(4, 3)) + ((p(0, 0) + p(2, 2)) + p(1, 1));
                }
            }
        }

        // Stage3: the LCP (lambda lands in the cfm slots)
        let mut bx: Vec<f64> = (0..m).flat_map(|r| [rhs[r], cfm[r]]).collect();
        let mut lh: Vec<f64> = (0..m).flat_map(|r| [lo[r], hi[r]]).collect();
        probe(&LcpInputs { m, nub: nub as usize, a: &a, bx: &bx, lh: &lh, findex: &findex, bodies });
        solve_lcp(m, &mut a, &mut bx, nub as usize, &mut lh, Some(&mut findex));
        for r in 0..m {
            lambda[r] = bx[2 * r + 1];
        }
    }

    // Stage4: constraint forces (each body's joints newest first), velocities, dxStepBody
    let mut links: Vec<Vec<(usize, usize)>> = vec![Vec::new(); nb];
    for (i, &(j, ..)) in infos.iter().enumerate() {
        links[joints[j].b0].push((i, 0));
        if let Some(b1) = joints[j].b1 {
            links[b1].push((i, 1));
        }
    }
    for (bi, b) in bodies.iter_mut().enumerate() {
        let mul = h * b.inv_mass;
        let acc = if links[bi].is_empty() {
            for k in 0..3 {
                b.lvel[k] += b.facc[k] * mul;
            }
            [b.tacc[0] * h, b.tacc[1] * h, b.tacc[2] * h]
        } else {
            let mut cf = [0.0; 6];
            for &(i, side) in links[bi].iter().rev() {
                let (ofs, im) = (mindex[i], mindex[i + 1] - mindex[i]);
                // CSP starts the sum from row 0's product, not +0 (all-(-0) products stay -0)
                let row = |r: usize| {
                    let x = &j_all[16 * ofs + 8 * (side * im + r)..];
                    [0, 1, 2, 4, 5, 6].map(|xk| x[xk] * lambda[ofs + r])
                };
                let mut s = row(0);
                for r in 1..im {
                    let p = row(r);
                    for k in 0..6 {
                        s[k] += p[k];
                    }
                }
                if let Some(fb) = &mut joints[infos[i].0].feedback {
                    // f1, t1 (body 0) or f2, t2 (body 1), each a dVector3
                    fb[8 * side..8 * side + 3].copy_from_slice(&s[..3]);
                    fb[8 * side + 4..8 * side + 7].copy_from_slice(&s[3..]);
                }
                for k in 0..6 {
                    cf[k] += s[k];
                }
            }
            for k in 0..3 {
                b.lvel[k] += (cf[k] + b.facc[k]) * mul;
            }
            [(cf[3] + b.tacc[0]) * h, (cf[4] + b.tacc[1]) * h, (cf[5] + b.tacc[2]) * h]
        };
        let t = mul0_331(&inv_i[bi], &acc);
        b.avel[0] += t[0];
        b.avel[1] += t[1];
        // CSP: the z row as (y product + avel) + (x product + z product)
        let ii = &inv_i[bi];
        b.avel[2] = (acc[1] * ii[9] + b.avel[2]) + (acc[0] * ii[8] + acc[2] * ii[10]);
        step_body(b, h);
        b.facc = [0.0; 4];
        b.tacc = [0.0; 4];
    }
}
