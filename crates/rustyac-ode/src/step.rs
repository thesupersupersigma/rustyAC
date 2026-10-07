// SPDX-License-Identifier: BSD-3-Clause

//! `dWorldStep`: islands (`ode/src/util.cpp`), the island stepper (`ode/src/step.cpp`) and
//! the per-body integrator `dxStepBody`.
//!
//! Stage 1 of the port: every constraint row is an equality (rods, ball joints, sliders,
//! fixed joints), so the LCP solver comes down to a plain `L*D*L^T` factor and solve. The
//! game reaches that in two ways. The stepper hands `dSolveLCP` the number of fully
//! unbounded **joints** as `nub`; for an island of one-row joints that equals the row count
//! and the solver's `nub >= n` shortcut runs, for any island with a multi-row joint (a car:
//! 21 joints, 26 rows) it is smaller and the general constructor `dLCP::dLCP` @ 0x140391cf0
//! runs, finds every remaining row unbounded, copies the lower triangle and makes the same
//! two calls. Both give the same numbers; the second leaves `A` and the right-hand side
//! untouched. Bounded rows (contacts, stops, motors) are stage 2 and have to copy the real
//! thing.
//!
//! The game runs ODE's threading interface in its single-threaded form, so the stages below
//! run one after the other in the order of the source's `allowedThreads == 1` path.

use crate::common::pad;
use crate::joint::{Info1, Info2, JointFeedback};
use crate::matrix::{factor_ldlt, solve_ldlt};
use crate::odemath::{
    dot3, invert_matrix3, multiply0_133, multiply0_331, multiply0_333, multiply2_333, safe_normalize4,
    set_cross_matrix_minus,
};
use crate::rotation::{dq_from_w, q_multiply0, r_from_q};
use crate::world::{
    Body, BodyId, JointId, World, BODY_ANGULAR_DAMPING, BODY_AUTO_DISABLE, BODY_DISABLED, BODY_FINITE_ROTATION,
    BODY_FINITE_ROTATION_AXIS, BODY_GYROSCOPIC, BODY_LINEAR_DAMPING, BODY_MAX_ANGULAR_SPEED, BODY_NO_GRAVITY,
};
use rustyac_math::{cosf, sinf, sqrtf};

/// Scratch arrays of the stepper (ODE's `dxWorldProcessMemArena`), kept between steps so
/// that a step does not allocate.
#[derive(Clone, Debug, Default)]
pub(crate) struct StepMemory {
    island_sizes: Vec<(u32, u32)>,
    bodies: Vec<BodyId>,
    joints: Vec<JointId>,
    stack: Vec<BodyId>,
    inv_i: Vec<f32>,
    joint_infos: Vec<(JointId, Info1)>,
    mindex: Vec<u32>,
    j: Vec<f32>,
    j_inv_m: Vec<f32>,
    a: Vec<f32>,
    rhs: Vec<f32>,
    cfm_or_rhs_tmp: Vec<f32>,
    d: Vec<f32>,
    cforce: Vec<f32>,
}

/// What the last step looked like from the outside: for tests and tools.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StepStats {
    /// Islands stepped.
    pub islands: u32,
    /// Constraint rows of the largest island.
    pub max_rows: u32,
}

impl World {
    /// `dWorldStep` @ 0x1403404c0: advances every enabled body by `stepsize` seconds.
    pub fn step(&mut self, stepsize: f32) -> StepStats {
        let mut memory = std::mem::take(&mut self.step_memory);
        build_islands(self, stepsize, &mut memory);
        let mut stats = StepStats::default();
        let mut body_start = 0usize;
        let mut joint_start = 0usize;
        // dxProcessIslands @ 0x1403534f0: one island after the other, in the order found
        for island in 0..memory.island_sizes.len() {
            let (bcount, jcount) = memory.island_sizes[island];
            let rows = step_island(
                self,
                &mut memory,
                body_start,
                bcount as usize,
                joint_start,
                jcount as usize,
                stepsize,
            );
            stats.islands += 1;
            stats.max_rows = stats.max_rows.max(rows);
            body_start += bcount as usize;
            joint_start += jcount as usize;
        }
        self.step_memory = memory;
        stats
    }
}

/// `dInternalHandleAutoDisabling` @ 0x140353230: counts idle steps of bodies that have the
/// auto-disable flag and puts them to sleep. (AC only sets the flag on movable track
/// objects; a car is frozen by the game's own code.)
fn handle_auto_disabling(world: &mut World, stepsize: f32) {
    let mut next = world.first_body;
    while let Some(id) = next {
        let bb = &mut world.bodies[id.0 as usize];
        next = bb.next;

        // don't freeze objects mid-air (patch 1586738)
        if bb.first_joint.is_none() {
            continue;
        }

        // nothing to do unless this body is currently enabled and has
        // the auto-disable flag set
        if bb.flags & (BODY_AUTO_DISABLE | BODY_DISABLED) != BODY_AUTO_DISABLE {
            continue;
        }

        // if sampling / threshold testing is disabled, we can never sleep.
        if bb.adis.average_samples == 0 {
            continue;
        }

        // sample the linear and angular velocity
        let counter = bb.average_counter as usize;
        if bb.average_lvel_buffer.len() <= counter {
            let samples = bb.adis.average_samples as usize;
            bb.average_lvel_buffer.resize(samples.max(counter + 1), [0.0; 3]);
            bb.average_avel_buffer.resize(samples.max(counter + 1), [0.0; 3]);
        }
        bb.average_lvel_buffer[counter] = [bb.lvel[0], bb.lvel[1], bb.lvel[2]];
        bb.average_avel_buffer[counter] = [bb.avel[0], bb.avel[1], bb.avel[2]];
        bb.average_counter += 1;

        // buffer ready test
        if bb.average_counter >= bb.adis.average_samples {
            bb.average_counter = 0; // fill the buffer from the beginning
            bb.average_ready = true; // this body is ready now for average calculation
        }

        let mut idle = false; // Assume it's in motion unless we have samples to disprove it.

        // enough samples?
        if bb.average_ready {
            idle = true; // Initial assumption: IDLE

            // the sample buffers are filled and ready for calculation
            // Store first velocity samples
            let mut average_lvel = bb.average_lvel_buffer[0];
            let mut average_avel = bb.average_avel_buffer[0];

            // If we're not in "instantaneous mode"
            if bb.adis.average_samples > 1 {
                // add remaining velocities together
                for i in 1..bb.adis.average_samples as usize {
                    for k in 0..3 {
                        average_lvel[k] += bb.average_lvel_buffer[i][k];
                        average_avel[k] += bb.average_avel_buffer[i][k];
                    }
                }

                // make average
                let r1 = 1.0f32 / bb.adis.average_samples as f32;
                for k in 0..3 {
                    average_lvel[k] *= r1;
                    average_avel[k] *= r1;
                }
            }

            // threshold test
            let av_lspeed = dot3(&average_lvel, &average_lvel);
            if av_lspeed > bb.adis.linear_average_threshold {
                idle = false; // average linear velocity is too high for idle
            } else {
                let av_aspeed = dot3(&average_avel, &average_avel);
                if av_aspeed > bb.adis.angular_average_threshold {
                    idle = false; // average angular velocity is too high for idle
                }
            }
        }

        // if it's idle, accumulate steps and time.
        // these counters won't overflow because this code doesn't run for disabled bodies.
        if idle {
            bb.adis_stepsleft -= 1;
            bb.adis_timeleft -= stepsize;
        } else {
            // Reset countdowns
            bb.adis_stepsleft = bb.adis.idle_steps;
            bb.adis_timeleft = bb.adis.idle_time;
        }

        // disable the body if it's idle for a long enough time
        if bb.adis_stepsleft <= 0 && bb.adis_timeleft <= 0.0 {
            bb.flags |= BODY_DISABLED; // set the disable flag

            // disabling bodies should also include resetting the velocity
            // should prevent jittering in big "islands"
            bb.lvel[0] = 0.0;
            bb.lvel[1] = 0.0;
            bb.lvel[2] = 0.0;
            bb.avel[0] = 0.0;
            bb.avel[1] = 0.0;
            bb.avel[2] = 0.0;
        }
    }
}

/// The island builder (0x1403527a0 in `acs.exe`, `BuildIslandsAndEstimateStepperMemoryRequirements`
/// in `util.cpp`): groups the enabled bodies into sets connected by joints.
///
/// Bodies are visited along the world's body list (newest first). From each untagged enabled
/// body the island grows depth-first: the current body's joint nodes are walked (newest
/// attached joint first), every untagged enabled joint is appended to the island's joint
/// list and its other body, if untagged, is pushed on a stack (and woken up); when the node
/// list ends the last pushed body is popped and appended to the island's body list.
fn build_islands(world: &mut World, stepsize: f32, memory: &mut StepMemory) {
    // handle auto-disabling of bodies
    handle_auto_disabling(world, stepsize);

    memory.island_sizes.clear();
    memory.bodies.clear();
    memory.joints.clear();
    memory.stack.clear();

    // set all body/joint tags to 0
    let mut b = world.first_body;
    while let Some(id) = b {
        world.bodies[id.0 as usize].tag = 0;
        b = world.bodies[id.0 as usize].next;
    }
    let mut j = world.first_joint;
    while let Some(id) = j {
        world.joints[id.0 as usize].tag = 0;
        j = world.joints[id.0 as usize].next;
    }

    let mut bb_next = world.first_body;
    while let Some(bb) = bb_next {
        bb_next = world.bodies[bb.0 as usize].next;
        // get bb = the next enabled, untagged body, and tag it
        if world.bodies[bb.0 as usize].tag != 0 {
            continue;
        }
        if world.bodies[bb.0 as usize].flags & BODY_DISABLED != 0 {
            world.bodies[bb.0 as usize].tag = -1; // Not used so far (assigned to retain consistency with joints)
            continue;
        }
        world.bodies[bb.0 as usize].tag = 1;

        let body_start = memory.bodies.len();
        let joint_start = memory.joints.len();

        // tag all bodies and joints starting from bb.
        memory.bodies.push(bb);
        memory.stack.clear();
        let mut b = bb;
        loop {
            // traverse and tag all body's joints, add untagged connected bodies
            // to stack
            let mut n = world.bodies[b.0 as usize].first_joint;
            while let Some(node) = n {
                let njoint = node.joint;
                let (other_body, next) = {
                    let jn = &world.joints[njoint.0 as usize].node[node.node as usize];
                    (jn.body, jn.next)
                };
                if world.joints[njoint.0 as usize].tag == 0 {
                    if world.joints[njoint.0 as usize].is_enabled(&world.bodies) {
                        world.joints[njoint.0 as usize].tag = 1;
                        memory.joints.push(njoint);

                        // Body disabled flag is not checked here. This is how auto-enable works.
                        if let Some(nbody) = other_body {
                            let nb = &mut world.bodies[nbody.0 as usize];
                            if nb.tag <= 0 {
                                nb.tag = 1;
                                // Make sure all bodies are in the enabled state.
                                nb.flags &= !BODY_DISABLED;
                                memory.stack.push(nbody);
                            }
                        }
                    } else {
                        // Used in Step to prevent search over disabled joints
                        world.joints[njoint.0 as usize].tag = -1;
                    }
                }
                n = next;
            }

            match memory.stack.pop() {
                None => break,
                Some(popped) => {
                    b = popped; // pop body off stack
                    memory.bodies.push(b); // put body on body list
                }
            }
        }

        let bcount = (memory.bodies.len() - body_start) as u32;
        let jcount = (memory.joints.len() - joint_start) as u32;
        memory.island_sizes.push((bcount, jcount));
    }
}

/// `dxStepIsland` @ 0x1403501b0 with its stages. Returns the number of constraint rows.
fn step_island(
    world: &mut World,
    memory: &mut StepMemory,
    body_start: usize,
    nb: usize,
    joint_start: usize,
    nj_island: usize,
    stepsize: f32,
) -> u32 {
    // ------------------------------------------------------------------------------------
    // dxStepIsland_Stage0_Bodies (0x140350360)

    // number all bodies in the body list - set their tag values
    for i in 0..nb {
        let id = memory.bodies[body_start + i];
        world.bodies[id.0 as usize].tag = i as i32;
    }

    // add the gravity force to all bodies
    // since gravity does normally have only one component it's more efficient
    // to run three loops for each individual component
    for axis in 0..3 {
        let g = world.gravity[axis];
        // `ucomiss g, 0` / `je`: a zero or NaN component is skipped
        if g == 0.0 || g.is_nan() {
            continue;
        }
        for i in 0..nb {
            let b = &mut world.bodies[memory.bodies[body_start + i].0 as usize];
            if b.flags & BODY_NO_GRAVITY == 0 {
                b.facc[axis] = b.mass.mass * g + b.facc[axis];
            }
        }
    }

    // for all bodies, compute the inertia tensor and its inverse in the global
    // frame, and compute the rotational force and add it to the torque
    // accumulator. invI is a vertical stack of 3x4 matrices, one per body.
    memory.inv_i.clear();
    memory.inv_i.resize(12 * nb, 0.0);
    for i in 0..nb {
        let b = &mut world.bodies[memory.bodies[body_start + i].0 as usize];
        let inv_i_row = &mut memory.inv_i[12 * i..12 * i + 12];
        let mut tmp = [0.0f32; 12];

        // compute inverse inertia tensor in global frame
        multiply2_333(&mut tmp, &b.inv_i, &b.r);
        multiply0_333(inv_i_row, &b.r, &tmp);

        // Don't apply gyroscopic torques to bodies
        // if not flagged or the body is kinematic
        // (`comiss 0, invMass` + `jae skip`: a NaN inverse mass enters the block)
        if b.flags & BODY_GYROSCOPIC != 0 && !(0.0 >= b.inv_mass) {
            gyroscopic_torque(b, stepsize);
        }
    }

    // ------------------------------------------------------------------------------------
    // dxStepIsland_Stage0_Joints (0x140350b80)
    //
    // get m = total constraint dimension, nub = number of unbounded variables.
    // the constraints are re-ordered as follows: the purely unbounded
    // constraints, the mixed unbounded + LCP constraints, and last the purely
    // LCP constraints. joints with m=0 are inactive and are removed from the
    // joints array entirely. also number all active joints in the joint list
    // (set their tag values). inactive joints receive a tag value of -1.
    //
    // The array is twice the island's joint count and is filled from the middle: unbounded
    // joints grow towards the front (so they end up in REVERSE island order), bounded ones
    // towards the back. Stage 1 has no bounded or mixed rows.
    let mut m: u32 = 0;
    memory.joint_infos.clear();
    memory.joint_infos.resize(2 * nj_island, (JointId(0), Info1::default()));
    let mut unb_start = nj_island;
    for k in 0..nj_island {
        let id = memory.joints[joint_start + k];
        let info = world.joints[id.0 as usize].get_info1(&world.bodies);
        if info.m == 0 {
            world.joints[id.0 as usize].tag = -1;
            continue;
        }
        if info.nub != info.m {
            unimplemented!("a joint with bounded constraint rows (stage 2 of the port)");
        }
        m += info.m as u32;
        unb_start -= 1;
        memory.joint_infos[unb_start] = (id, info);
    }
    let ji_start = unb_start;
    let ji_end = nj_island;
    let nj = ji_end - ji_start;
    for (i, k) in (ji_start..ji_end).enumerate() {
        let id = memory.joint_infos[k].0;
        world.joints[id.0 as usize].tag = i as i32;
    }
    let m = m as usize;

    // ------------------------------------------------------------------------------------
    // dxStepIsland_Stage1 (0x140350eb0)
    let stepsize_recip = 1.0f32 / stepsize;
    let mskip = pad(m);
    if m > 0 {
        // mindex[i] = first row of joint i
        memory.mindex.clear();
        let mut moffs = 0u32;
        memory.mindex.push(moffs);
        for k in ji_start..ji_end {
            moffs += memory.joint_infos[k].1.m as u32;
            memory.mindex.push(moffs);
        }

        // --------------------------------------------------------------------------------
        // dxStepIsland_Stage2a (0x140351250): get jacobian data from constraints. a (2*m)x8
        // matrix will be created to store the two jacobian blocks from each constraint. it
        // has this format:
        //
        //   l l l 0 a a a 0  \    .
        //   l l l 0 a a a 0   }-- jacobian body 1 block for joint 0 (3 rows)
        //   l l l 0 a a a 0  /
        //   l l l 0 a a a 0  \    .
        //   l l l 0 a a a 0   }-- jacobian body 2 block for joint 0 (3 rows)
        //   l l l 0 a a a 0  /
        //   l l l 0 a a a 0  }--- jacobian body 1 block for joint 1 (1 row)
        //   l l l 0 a a a 0  }--- jacobian body 2 block for joint 1 (1 row)
        //   etc...
        //
        //   (lll) = linear jacobian data
        //   (aaa) = angular jacobian data
        memory.j.clear();
        memory.j.resize(2 * 8 * m, 0.0);
        memory.rhs.clear();
        memory.rhs.resize(m, 0.0);
        let cfm_len = m.max(nb * 8);
        memory.cfm_or_rhs_tmp.clear();
        memory.cfm_or_rhs_tmp.resize(cfm_len, 0.0);
        let world_erp = world.global_erp;
        for ji in 0..nj {
            let ofsi = memory.mindex[ji] as usize;
            let infom = memory.mindex[ji + 1] as usize - ofsi;
            let (j1, j2) = memory.j[2 * 8 * ofsi..2 * 8 * (ofsi + infom)].split_at_mut(8 * infom);
            let c = &mut memory.rhs[ofsi..ofsi + infom];
            let cfm = &mut memory.cfm_or_rhs_tmp[ofsi..ofsi + infom];
            for v in cfm.iter_mut() {
                *v = world.global_cfm;
            }
            let joint = &world.joints[memory.joint_infos[ji_start + ji].0 .0 as usize];
            let mut info = Info2 { j1, j2, c, cfm };
            joint.get_info2(&world.bodies, stepsize_recip, world_erp, &mut info);
            for v in info.c.iter_mut() {
                *v *= stepsize_recip;
            }
        }

        // --------------------------------------------------------------------------------
        // dxStepIsland_Stage2b (0x1403516a0)
        //
        // A starts as zero with cfm / h on the diagonal
        memory.a.clear();
        memory.a.resize(m * mskip, 0.0);
        for row in 0..m {
            memory.a[row * mskip + row] = memory.cfm_or_rhs_tmp[row] * stepsize_recip;
        }

        // compute A = J*invM*J'. first compute JinvM = J*invM. this has the same
        // format as J so we just go through the constraints in J multiplying by
        // the appropriate scalars and matrices.
        memory.j_inv_m.clear();
        memory.j_inv_m.resize(2 * 8 * m, 0.0);
        for ji in 0..nj {
            let ofsi = memory.mindex[ji] as usize;
            let infom = memory.mindex[ji + 1] as usize - ofsi;
            let joint = &world.joints[memory.joint_infos[ji_start + ji].0 .0 as usize];
            let mut src = 2 * 8 * ofsi;
            for node in 0..2 {
                let Some(body_id) = joint.node[node].body else {
                    continue;
                };
                let body = &world.bodies[body_id.0 as usize];
                let body_inv_mass = body.inv_mass;
                let tag = body.tag as usize;
                let body_inv_i = &memory.inv_i[12 * tag..12 * tag + 12];
                for _ in 0..infom {
                    for k in 0..3 {
                        memory.j_inv_m[src + k] = memory.j[src + k] * body_inv_mass;
                    }
                    let r = multiply0_133(&memory.j[src + 4..src + 7], body_inv_i);
                    memory.j_inv_m[src + 4] = r[0];
                    memory.j_inv_m[src + 5] = r[1];
                    memory.j_inv_m[src + 6] = r[2];
                    src += 8;
                }
            }
        }

        // put v/h + invM*fe into rhs_tmp (it reuses the memory of cfm)
        for bi in 0..nb {
            let b = &world.bodies[memory.bodies[body_start + bi].0 as usize];
            let inv_i_row = &memory.inv_i[12 * bi..12 * bi + 12];
            let tmp1 = &mut memory.cfm_or_rhs_tmp[8 * bi..8 * bi + 8];
            for j in 0..3 {
                tmp1[j] = b.facc[j] * b.inv_mass + b.lvel[j] * stepsize_recip;
            }
            let t = multiply0_331(inv_i_row, &b.tacc);
            for k in 0..3 {
                tmp1[4 + k] = t[k] + b.avel[k] * stepsize_recip;
            }
        }

        // --------------------------------------------------------------------------------
        // dxStepIsland_Stage2c (0x140351d20)
        //
        // now compute A = JinvM * J'. A's rows and columns are grouped by joint,
        // i.e. in the same way as the rows of J. block (i,j) of A is only nonzero
        // if joints i and j have at least one body in common.
        for ji in 0..nj {
            let ofsi = memory.mindex[ji] as usize;
            let infom = memory.mindex[ji + 1] as usize - ofsi;
            let joint_id = memory.joint_infos[ji_start + ji].0;
            for node in 0..2 {
                let Some(jb) = world.joints[joint_id.0 as usize].node[node].body else {
                    continue;
                };
                let j_inv_m_row = 2 * 8 * ofsi + node * 8 * infom;
                // compute diagonal block of A
                multiply_add2_p8r(
                    &mut memory.a,
                    ofsi * mskip + ofsi,
                    &memory.j_inv_m[j_inv_m_row..],
                    &memory.j[2 * 8 * ofsi + node * 8 * infom..],
                    infom,
                    infom,
                    mskip,
                );
                if ji == 0 {
                    continue;
                }
                let mut n = world.bodies[jb.0 as usize].first_joint;
                while let Some(nref) = n {
                    let other = &world.joints[nref.joint.0 as usize];
                    n = other.node[nref.node as usize].next;
                    // if joint was tagged as -1 then it is an inactive (m=0 or disabled)
                    // joint that should not be considered
                    let j0 = other.tag;
                    if j0 != -1 && (j0 as u32 as usize) < ji {
                        let j0 = j0 as usize;
                        let jiother_ofsi = memory.mindex[j0] as usize;
                        let jiother_infom = memory.mindex[j0 + 1] as usize - jiother_ofsi;
                        let ofsother = if other.node[1].body == Some(jb) { 8 * jiother_infom } else { 0 };
                        // set block of A
                        multiply_add2_p8r(
                            &mut memory.a,
                            ofsi * mskip + jiother_ofsi,
                            &memory.j_inv_m[j_inv_m_row..],
                            &memory.j[2 * 8 * jiother_ofsi + ofsother..],
                            infom,
                            jiother_infom,
                            mskip,
                        );
                    }
                }
            }
        }

        // compute the right hand side `rhs': rhs = c/h - J*rhs_tmp
        for ji in 0..nj {
            let ofsi = memory.mindex[ji] as usize;
            let infom = memory.mindex[ji + 1] as usize - ofsi;
            let joint = &world.joints[memory.joint_infos[ji_start + ji].0 .0 as usize];
            for node in 0..2 {
                let Some(jb) = joint.node[node].body else {
                    continue;
                };
                let tag = world.bodies[jb.0 as usize].tag as usize;
                let c = &memory.cfm_or_rhs_tmp[8 * tag..8 * tag + 8];
                let (c0, c1, c2, c4, c5, c6) = (c[0], c[1], c[2], c[4], c[5], c[6]);
                // MultiplySub0_p81
                let mut bb = 2 * 8 * ofsi + node * 8 * infom;
                for i in 0..infom {
                    let jr = &memory.j[bb..bb + 8];
                    let mut sum = jr[0] * c0;
                    sum += jr[1] * c1;
                    sum += jr[2] * c2;
                    sum += jr[4] * c4;
                    sum += jr[5] * c5;
                    sum += jr[6] * c6;
                    memory.rhs[ofsi + i] -= sum;
                    bb += 8;
                }
            }
        }

        // --------------------------------------------------------------------------------
        // dxStepIsland_Stage3 (0x140352100): solve the LCP problem and get lambda.
        // dSolveLCP @ 0x140392260 with only equality rows: factor and solve (through the
        // `nub >= n` shortcut or through dLCP::dLCP, see the top of this file).
        memory.d.clear();
        memory.d.resize(m, 0.0);
        factor_ldlt(&mut memory.a, &mut memory.d, m, mskip);
        solve_ldlt(&memory.a, &memory.d, &mut memory.rhs, m, mskip);
    }
    // lambda is now in memory.rhs

    // this will be set to the force due to the constraints
    memory.cforce.clear();
    memory.cforce.resize(nb * 8, 0.0);

    if m > 0 {
        // compute the constraint force `cforce'
        // compute cforce = J'*lambda
        for ji in 0..nj {
            let ofsi = memory.mindex[ji] as usize;
            let infom = memory.mindex[ji + 1] as usize - ofsi;
            let joint_id = memory.joint_infos[ji_start + ji].0;
            let lambda = &memory.rhs[ofsi..ofsi + infom];
            let mut feedback = JointFeedback::default();
            let wants_feedback = world.joints[joint_id.0 as usize].feedback.is_some();
            for node in 0..2 {
                let Some(body_id) = world.joints[joint_id.0 as usize].node[node].body else {
                    continue;
                };
                let tag = world.bodies[body_id.0 as usize].tag as usize;
                // Multiply1_8q1: data = J_block^T * lambda, each sum built from 0
                let jj = &memory.j[2 * 8 * ofsi + node * 8 * infom..];
                let (mut sum0, mut sum1, mut sum2, mut sum4, mut sum5, mut sum6) =
                    (0.0f32, 0.0f32, 0.0f32, 0.0f32, 0.0f32, 0.0f32);
                for (k, &c_k) in lambda.iter().enumerate() {
                    let row = &jj[8 * k..8 * k + 8];
                    sum0 += row[0] * c_k;
                    sum1 += row[1] * c_k;
                    sum2 += row[2] * c_k;
                    sum4 += row[4] * c_k;
                    sum5 += row[5] * c_k;
                    sum6 += row[6] * c_k;
                }
                let cf = &mut memory.cforce[8 * tag..8 * tag + 8];
                cf[0] += sum0;
                cf[1] += sum1;
                cf[2] += sum2;
                cf[4] += sum4;
                cf[5] += sum5;
                cf[6] += sum6;
                if node == 0 {
                    feedback.f1 = [sum0, sum1, sum2, 0.0];
                    feedback.t1 = [sum4, sum5, sum6, 0.0];
                } else {
                    feedback.f2 = [sum0, sum1, sum2, 0.0];
                    feedback.t2 = [sum4, sum5, sum6, 0.0];
                }
            }
            if wants_feedback {
                // ODE writes only the members of bodies that exist; the rest keep their old values
                let joint = &mut world.joints[joint_id.0 as usize];
                let has_second = joint.node[1].body.is_some();
                let fb = joint.feedback.as_mut().unwrap();
                fb.f1[..3].copy_from_slice(&feedback.f1[..3]);
                fb.t1[..3].copy_from_slice(&feedback.t1[..3]);
                if has_second {
                    fb.f2[..3].copy_from_slice(&feedback.f2[..3]);
                    fb.t2[..3].copy_from_slice(&feedback.t2[..3]);
                }
            }
        }
    }

    // compute the velocity update
    // add fe to cforce and multiply cforce by stepsize
    for bi in 0..nb {
        let b = &mut world.bodies[memory.bodies[body_start + bi].0 as usize];
        let inv_i_row = &memory.inv_i[12 * bi..12 * bi + 12];
        let cforce = &memory.cforce[8 * bi..8 * bi + 8];

        let body_inv_mass_mul_stepsize = stepsize * b.inv_mass;
        for j in 0..3 {
            b.lvel[j] = (cforce[j] + b.facc[j]) * body_inv_mass_mul_stepsize + b.lvel[j];
        }

        let mut data = [0.0f32; 3];
        for k in 0..3 {
            data[k] = (cforce[4 + k] + b.tacc[k]) * stepsize;
        }
        let t = multiply0_331(inv_i_row, &data);
        for k in 0..3 {
            b.avel[k] = t[k] + b.avel[k];
        }
    }

    // update the position and orientation from the new linear/angular velocity
    // (over the given timestep)
    for bi in 0..nb {
        let b = &mut world.bodies[memory.bodies[body_start + bi].0 as usize];
        step_body(b, stepsize);
    }

    // zero all force accumulators
    for bi in 0..nb {
        let b = &mut world.bodies[memory.bodies[body_start + bi].0 as usize];
        b.facc = [0.0; 4];
        b.tacc = [0.0; 4];
    }

    m as u32
}

/// The gyroscopic block of `dxStepIsland_Stage0_Bodies` (0x1403505b8 … 0x140350aa8): ODE
/// 0.13.1's implicit gyroscopic torque ("Stabilizing Gyroscopic Forces in Rigid Multibody
/// Simulations", Lacoursière 2006), added to the body's torque accumulator.
fn gyroscopic_torque(b: &mut Body, h: f32) {
    let mut tmp = [0.0f32; 12];
    let mut inertia = [0.0f32; 12];
    // compute inertia tensor in global frame
    multiply2_333(&mut tmp, &b.mass.i, &b.r);
    multiply0_333(&mut inertia, &b.r, &tmp);

    // Compute angular momentum
    let mut l = multiply0_331(&inertia, &b.avel);

    // Compute a new effective 'inertia tensor' for the implicit step: the cross-product
    // matrix of the angular momentum plus the old tensor scaled by the timestep.
    // Itild may not be symmetric pos-definite, but we can still use it to compute implicit
    // gyroscopic torques.
    let mut itild = [0.0f32; 12];
    set_cross_matrix_minus(&mut itild, &l, 4);
    for ii in 0..12 {
        itild[ii] = itild[ii] * h + inertia[ii];
    }

    // Scale momentum by inverse time to get a sort of "torque"
    let h_recip = 1.0f32 / h;
    l[0] *= h_recip;
    l[1] *= h_recip;
    l[2] *= h_recip;

    // Invert the pseudo-tensor (closed form)
    let mut it_inv = [0.0f32; 12];
    if invert_matrix3(&mut it_inv, &itild) != 0.0 {
        // "Divide" the original tensor by the pseudo-tensor (on the right)
        multiply0_333(&mut itild, &inertia, &it_inv);
        // Subtract an identity matrix
        itild[0] -= 1.0;
        itild[5] -= 1.0;
        itild[10] -= 1.0;

        // This new inertia matrix rotates the momentum to get a new set of torques
        // that will work correctly when applied to the old inertia matrix as explicit
        // torques with a semi-implicit update step.
        let tau0 = multiply0_331(&itild, &l);

        // Add the gyro torques to the torque accumulator
        for ii in 0..3 {
            b.tacc[ii] = tau0[ii] + b.tacc[ii];
        }
    }
}

/// `MultiplyAdd2_p8r` (static in `step.cpp`): `A[p x r] += B[p x 8] * C[r x 8]^T`, where the
/// fourth and eighth value of every row of B and C are padding. `a_off` is the index of the
/// block's top-left element in `a`, whose rows are `a_skip` long.
#[inline(always)]
fn multiply_add2_p8r(a: &mut [f32], a_off: usize, b: &[f32], c: &[f32], p: usize, r: usize, a_skip: usize) {
    for i in 0..p {
        let bb = &b[8 * i..8 * i + 8];
        for j in 0..r {
            let cc = &c[8 * j..8 * j + 8];
            let mut sum = bb[0] * cc[0];
            sum += bb[1] * cc[1];
            sum += bb[2] * cc[2];
            sum += bb[4] * cc[4];
            sum += bb[5] * cc[5];
            sum += bb[6] * cc[6];
            a[a_off + i * a_skip + j] += sum;
        }
    }
}

/// `sinc` (static in `util.cpp`): sin(x)/x, with a two-term Taylor series near zero. The
/// threshold is the **double** constant 1.0e-4.
#[inline(always)]
fn sinc(x: f32) -> f32 {
    // if |x| < 1e-4 then use a taylor series expansion. this two term expansion
    // is actually accurate to one LS bit within this range if double precision
    // is being used - so don't worry!
    if (x.abs() as f64) < 1.0e-4 {
        1.0f32 - x * x * 0.166_666_67f32
    } else {
        sinf(x) / x
    }
}

/// `dxStepBody` @ 0x140353860: given a body b, apply its linear and angular rotation over
/// the time interval h, thereby adjusting its position and orientation.
pub(crate) fn step_body(b: &mut Body, mut h: f32) {
    // cap the angular velocity
    if b.flags & BODY_MAX_ANGULAR_SPEED != 0 {
        let max_ang_speed = b.max_angular_speed;
        let aspeed = dot3(&b.avel, &b.avel);
        if aspeed > max_ang_speed * max_ang_speed {
            let coef = max_ang_speed / sqrtf(aspeed);
            b.avel[0] *= coef;
            b.avel[1] *= coef;
            b.avel[2] *= coef;
        }
    }
    // end of angular velocity cap

    // handle linear velocity
    for j in 0..3 {
        b.pos[j] = h * b.lvel[j] + b.pos[j];
    }

    if b.flags & BODY_FINITE_ROTATION != 0 {
        let mut irv = [0.0f32; 3]; // infitesimal rotation vector
        let q; // quaternion for finite rotation

        if b.flags & BODY_FINITE_ROTATION_AXIS != 0 {
            // split the angular velocity vector into a component along the finite
            // rotation axis, and a component orthogonal to it.
            let k = dot3(&b.finite_rot_axis, &b.avel);
            let frv = [b.finite_rot_axis[0] * k, b.finite_rot_axis[1] * k, b.finite_rot_axis[2] * k];
            irv[0] = b.avel[0] - frv[0];
            irv[1] = b.avel[1] - frv[1];
            irv[2] = b.avel[2] - frv[2];

            // make a rotation quaternion q that corresponds to frv * h.
            // compare this with the full-finite-rotation case below.
            h *= 0.5;
            let theta = k * h;
            let s = sinc(theta) * h;
            q = [cosf(theta), frv[0] * s, frv[1] * s, frv[2] * s];
        } else {
            // make a rotation quaternion q that corresponds to w * h
            let wlen = sqrtf(b.avel[0] * b.avel[0] + b.avel[1] * b.avel[1] + b.avel[2] * b.avel[2]);
            h *= 0.5;
            let theta = wlen * h;
            let s = sinc(theta) * h;
            q = [cosf(theta), b.avel[0] * s, b.avel[1] * s, b.avel[2] * s];
        }

        // do the finite rotation
        b.q = q_multiply0(&q, &b.q);

        // do the infitesimal rotation if required
        if b.flags & BODY_FINITE_ROTATION_AXIS != 0 {
            let dq = dq_from_w(&irv, &b.q);
            for j in 0..4 {
                b.q[j] += h * dq[j];
            }
        }
    } else {
        // the normal way - do an infitesimal rotation
        let dq = dq_from_w(&b.avel, &b.q);
        for j in 0..4 {
            b.q[j] += h * dq[j];
        }
    }

    // normalize the quaternion and convert it to a rotation matrix
    safe_normalize4(&mut b.q);
    r_from_q(&mut b.r, &b.q);

    // (geoms attached to the body are told that it moved here: collision is stage 2/3)

    // damping
    if b.flags & BODY_LINEAR_DAMPING != 0 {
        let lin_threshold = b.dampingp.linear_threshold;
        let lin_speed = dot3(&b.lvel, &b.lvel);
        if lin_speed > lin_threshold {
            let k = 1.0f32 - b.dampingp.linear_scale;
            b.lvel[0] *= k;
            b.lvel[1] *= k;
            b.lvel[2] *= k;
        }
    }
    if b.flags & BODY_ANGULAR_DAMPING != 0 {
        let ang_threshold = b.dampingp.angular_threshold;
        let ang_speed = dot3(&b.avel, &b.avel);
        if ang_speed > ang_threshold {
            let k = 1.0f32 - b.dampingp.angular_scale;
            b.avel[0] *= k;
            b.avel[1] *= k;
            b.avel[2] *= k;
        }
    }
}
