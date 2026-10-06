# rustyac-ode

The rigid-body core of rustyAC: a 1:1 Rust port of the part of **ODE 0.13.1** (Open Dynamics
Engine, single precision) that Assetto Corsa's `acs.exe` links and uses for a car that touches
nothing.

**Stage 1** (this crate today): world, bodies, mass, force and torque accumulators, the
gyroscopic term, the finite-rotation integrator, islands, auto-disable, the four joint types
the cars use (DBall = fixed-length rod, Ball, Slider, Fixed) and the equality-only path of
`dWorldStep` (Jacobian, `A = J * M^-1 * J^T`, `L*D*L^T` solve, velocity and position update).
Contacts with the pivoting LCP solver are stage 2, collision detection is stage 3.

The goal is the same bits as the game, not just the same physics. Where the machine code in
`acs.exe` differs from the ODE source (the library was built with fast floating-point
semantics: a few reciprocals in place of divisions, one vectorised sum, NaN-blind comparisons)
the machine code wins. Every function names its ODE original and its address in `acs.exe`
1.16.4 in its doc comment. Report with the details and the measurements:
[`docs/port/ode_stage1.md`](../../docs/port/ode_stage1.md).

```rust
use rustyac_ode::{Mass, World};

let mut world = World::assetto_corsa();            // gravity (0, -9.806, 0), ERP 0.3, CFM 1e-7
let chassis = world.body_create();                 // dBodyCreate
world.body_set_finite_rotation_mode(chassis, true);
world.body_set_mass(chassis, &Mass::box_total(510.0, 1.4, 0.75, 3.3));
let hub = world.body_create();
world.body_set_position(hub, 0.75, 0.0, 1.6);
let rod = world.joint_create_dball();              // dJointCreateDBall
world.joint_attach(rod, Some(chassis), Some(hub)); // dJointAttach
world.joint_set_dball_anchor1(rod, 0.3, 0.1, 1.6);
world.joint_set_dball_anchor2(rod, 0.7, 0.1, 1.6);
world.body_add_force_at_pos(hub, [0.0, 3000.0, 0.0], [0.75, -0.3, 1.6]);
world.step(0.003);                                 // dWorldStep
```

## Checks

- `cargo test -p rustyac-ode`: replays 300 steps of the game's own F2004 dropping onto the road
  (`settle`) and 300 steps of it in a slalom from checked-in excerpts of the whole-car oracle
  recordings; every body value and every joint force must be bit-identical.
- `tools/ode_oracle micro`: runs the game's own ODE (inside the mapped `acs.exe`) and this crate
  side by side on synthetic worlds, bit for bit after every step.
- `tools/ode_oracle replay`: replays all whole-car recordings, per step and as a free run.

## Licence of the original

This crate is a translation of ODE source code and keeps structure and comments of the
original. ODE is copyright (c) 2001-2007 Russell L. Smith and is used under its BSD-style
licence, reproduced in [`LICENSE-ODE`](LICENSE-ODE).
