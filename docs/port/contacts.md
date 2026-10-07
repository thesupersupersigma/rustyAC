# Task 13: the car body touches things (ODE contacts)

## Resume here

State after the last commit (kept up to date with every commit):

- **In progress.** Work is on `master`, nothing is pushed.
- Done so far (compiles, the old tests still pass, **not yet checked against the game**):
  - `crates/rustyac-ode/src/lcp.rs`: the pivoting LCP solver (`dSolveLCP`), with `ldlt_add_tl`, `ldlt_remove`,
    `remove_row_col` in `matrix.rs`.
  - `crates/rustyac-ode/src/contact.rs`: the contact joint (`getInfo1` / `getInfo2`); joint groups and joint slots
    that are used again in `joint.rs`; bounded rows in `step.rs`.
  - `crates/rustyac-ode/src/geom.rs`: moving geoms (boxes, meshes with an offset on a body), simple spaces with ODE's
    list order (a geom that gets dirty moves to the head of its space), the broad phase (`dSpaceCollide`,
    `dSpaceCollide2`), `dCollide`'s dispatch.
  - `crates/rustyac-physics/src/car/body.rs`: the game's `PhysicsCore` side (sub-spaces, `addBoxCollider`,
    `addMeshCollider`, `collisionStep`, `nearCallback`, `onCollision` with its two materials).
- **Checked against the game (bit-exact):** the contact joint, the stepper with bounded rows and the LCP solver.
  `tools\car_oracle	argetelease\car_oracle.exe collide-worlds --hybrid --steps 3000` steps eight small worlds in
  the game's own `PhysicsCore` and gives the port the game's contact joints before each `dWorldStep`: 24,000 world
  steps, 640,560 rows with limits, 712,280 pivots, 9 solver give-ups, **no difference** (`oracle/collide/worlds_hybrid.md`).
- Written, not yet checked: the car's side (`car/colliders.rs`: floor boxes and `collider.kn5`; in `car/chassis.rs`
  `step_core`, `on_collision_callback`, the mesh mask). The oracle harness is `tools/car_oracle/src/collide.rs`
  (`collide --track spa` for poses, `collide-worlds` for stepped worlds).
- Not done yet: the narrow phase (`collide_btl.rs` and `collide_ttl.rs` are stubs: `collide` and `collide-worlds`
  without `--hybrid` panic there), the car oracle with collisions and its scenarios, the comparison, the game.
- The briefs read from the machine code are in the git-ignored `re/scratch/task13/spec_*.md` (LCP, contact joint and
  stepper, dispatch; box-mesh, OPCODE's OBB collider, mesh-mesh, OPCODE's tree collider and the car's side are being
  written). Patch scripts: `re/scratch/task13/patch_*.py`.
- Next: port `dCollideBTL` + the OBB collider from `spec_btl.md` / `spec_opc_obb.md`, then the collision micro-oracle
  (`car_oracle collide`), then mesh-mesh.
