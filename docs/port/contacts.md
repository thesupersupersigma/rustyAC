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
  `car_oracle collide-worlds --hybrid --steps 3000` steps eight small worlds in
  the game's own `PhysicsCore` and gives the port the game's contact joints before each `dWorldStep`: 24,000 world
  steps, 640,560 rows with limits, 712,280 pivots, 9 solver give-ups, **no difference** (`oracle/collide/worlds_hybrid.md`).
- **Checked against the game (bit-exact):** box against mesh (`collide_btl.rs`, `opcode_obb.rs`) and the dispatch.
  `car_oracle collide --track spa --count 24000 --mesh-count 0 --boxes-only`: 964,908 `dCollide` pairs and 24,000
  collision passes over Spa, no difference. `car_oracle collide-worlds --only box_rest,box_slide,box_bounce,car_floor
  --steps 3000` (the port finds its own contacts): 12,000 steps, no difference.
- Written, not yet checked: the car's side (`car/colliders.rs`: floor boxes and `collider.kn5`; in `car/chassis.rs`
  `step_core`, `on_collision_callback`, the mesh mask).
- Not done yet: mesh against mesh (`collide_ttl.rs` is a stub: anything with the car's mesh near a wall panics
  there), the car oracle with collisions and its scenarios, the comparison, the game.
- The briefs read from the machine code are in the git-ignored `re/scratch/task13/spec_*.md` (LCP, contact joint and
  stepper, dispatch; box-mesh, OPCODE's OBB collider, mesh-mesh, OPCODE's tree collider and the car's side are being
  written). Patch scripts: `re/scratch/task13/patch_*.py`.
- Next: `car_oracle run --collide` with the new scenarios and `chassis_compare` for them; mesh-mesh from
  `spec_ttl.md` / `spec_opc_tree.md` when those briefs are written.
