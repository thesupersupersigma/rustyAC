// SPDX-License-Identifier: BSD-3-Clause

//! Plain physics sanity checks of the public API. These do not prove anything about bits
//! (the golden test and `tools/ode_oracle` do that); they show that the world behaves like a
//! world and that the README's example runs.

use rustyac_ode::{Mass, World};

const H: f32 = 0.003;

#[test]
fn a_free_body_falls() {
    let mut world = World::assetto_corsa();
    let b = world.body_create();
    world.body_set_finite_rotation_mode(b, true);
    world.body_set_mass(b, &Mass::box_total(10.0, 1.0, 1.0, 1.0));
    for _ in 0..1000 {
        world.step(H);
    }
    let body = world.body(b);
    // 3 s of free fall: v = g*t, y = g*t*t/2 (semi-implicit Euler is a little ahead)
    assert!((body.lvel[1] + 9.806 * 3.0).abs() < 1e-3, "velocity {}", body.lvel[1]);
    assert!((body.pos[1] + 0.5 * 9.806 * 9.0).abs() < 0.1, "height {}", body.pos[1]);
    assert_eq!(body.pos[0], 0.0);
    assert_eq!(body.facc, [0.0; 4], "the accumulators are cleared by the step");
}

#[test]
fn a_rod_to_the_world_keeps_its_length() {
    let mut world = World::assetto_corsa();
    let b = world.body_create();
    world.body_set_finite_rotation_mode(b, true);
    world.body_set_mass(b, &Mass::box_total(20.0, 0.2, 0.2, 0.2));
    world.body_set_position(b, 1.0, 0.0, 0.0);
    let rod = world.joint_create_dball();
    world.joint_attach(rod, Some(b), None);
    world.joint_set_dball_anchor1(rod, 1.0, 0.0, 0.0); // on the body
    world.joint_set_dball_anchor2(rod, 0.0, 0.0, 0.0); // in the world
    assert_eq!(world.joint_get_dball_distance(rod), 1.0);
    let mut lowest = 0.0f32;
    for _ in 0..2000 {
        world.step(H);
        let p = world.body(b).pos;
        let length = (p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt();
        assert!((length - 1.0).abs() < 2e-3, "rod length {length}");
        lowest = lowest.min(p[1]);
    }
    assert!(lowest < -0.99, "the pendulum swings through the bottom ({lowest})");
}

#[test]
fn bodies_joined_by_a_fixed_joint_move_as_one() {
    let mut world = World::assetto_corsa();
    world.set_gravity(0.0, 0.0, 0.0);
    let a = world.body_create();
    let b = world.body_create();
    for (body, x) in [(a, 0.0), (b, 1.0)] {
        world.body_set_finite_rotation_mode(body, true);
        world.body_set_mass(body, &Mass::box_total(5.0, 0.5, 0.5, 0.5));
        world.body_set_position(body, x, 0.0, 0.0);
    }
    let bolt = world.joint_create_fixed();
    world.joint_attach(bolt, Some(a), Some(b));
    world.joint_set_fixed(bolt);
    world.joint_set_feedback(bolt, true);
    for _ in 0..500 {
        world.body_add_force(a, [10.0, 0.0, 0.0]); // 10 N on 10 kg: 1 m/s2
        world.step(H);
    }
    let (va, vb) = (world.body(a).lvel[0], world.body(b).lvel[0]);
    assert!((va - 1.5).abs() < 1e-3 && (vb - 1.5).abs() < 1e-3, "velocities {va} {vb}");
    let gap = world.body(b).pos[0] - world.body(a).pos[0];
    assert!((gap - 1.0).abs() < 1e-4, "gap {gap}");
    // the bolt pulls the second body with half the force
    let pull = world.joint(bolt).feedback.unwrap().f2[0];
    assert!((pull - 5.0).abs() < 1e-2, "force on the second body {pull}");
}

#[test]
fn the_readme_example_runs() {
    let mut world = World::assetto_corsa();
    let chassis = world.body_create();
    world.body_set_finite_rotation_mode(chassis, true);
    world.body_set_mass(chassis, &Mass::box_total(510.0, 1.4, 0.75, 3.3));
    let hub = world.body_create();
    world.body_set_position(hub, 0.75, 0.0, 1.6);
    let rod = world.joint_create_dball();
    world.joint_attach(rod, Some(chassis), Some(hub));
    world.joint_set_dball_anchor1(rod, 0.3, 0.1, 1.6);
    world.joint_set_dball_anchor2(rod, 0.7, 0.1, 1.6);
    world.body_add_force_at_pos(hub, [0.0, 3000.0, 0.0], [0.75, -0.3, 1.6]);
    let stats = world.step(H);
    assert_eq!((stats.islands, stats.max_rows), (1, 1));
    assert!(world.body(hub).lvel[1] > 0.0);
}
