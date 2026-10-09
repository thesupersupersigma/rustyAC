# Task 21: AC's real renderer, part 2 (the whole car)

## Resume here

State after the last commit (kept up to date with every commit):

- **In progress.** The task text is `prompts/21_renderer_car.md`. Nothing is tagged; the version is still
  0.20.2. Do not tag until every step is done.
- Done so far:
  - the game's whole `CarPhysicsState` in `crates/rustyac-render/src/state.rs` (0xb70 bytes, the game's layout;
    the Task 20 pose files of 149 floats are still read);
  - the render oracle takes a **sequence**: `render_oracle compare --car <c> --tape <file.audiotape>
    [--tape-from <frame>] [--frames <n>]` feeds both sides the game's own car states (60 a second, written by
    `car_oracle run --audio-tape`), updates and renders every frame, and compares the command log and the
    pixels of each (`--dump <i,j>` writes those frames whole; the first frame that differs is run again and
    shown). New views `side`, `rear`, `front`; `--label <word>`; `--serial`.
  - the following cube map (`--cubemap-faces` in the oracle, `--cube-faces` in `rustyac.exe`): proven on a sequence;
  - `GLRenderer`, `NodeEvent` handlers, `CarFakeShadow` (ground shadows): proven; `rustyac.exe` fills the car state the helpers read (`view.rs` `RenderState`);
  - `TyreBlur` and `BlurredObjects` (`blur.rs`, with `Material::clone_material`): proven on two sequences;
  - `VisualDamageManager` (`damage.rs`: scratches, cracked glass at 0 until hit, body parts askew and shaking): proven on the 1M at rest (the cracked rear window is gone) and on the F2004 wall crash sequence;
  - `CarBrakeLights`, `BrakeDiscGraphics`, `DynamicCarEffects` (`lights.rs`; `graphics.crt_rand` = the C runtime's rand): proven on lights on / brake (1M), a hard stop (F2004 disc glow, 69 frames), grass (dirt, 39 frames); oracle `--set name=value[@frame]` writes over the state;
- Next: the specs of the nine items (`re/scratch/task21/spec_*.md`, written by reader agents from the
  disassembly), then port and prove item by item in this order: damage glass and damage meshes, blur meshes
  and `lods.ini` rules, lights and disc glow, the driver, `DIR_` nodes, instruments, ground shadows, skid
  marks and smoke, the following cube map, mirrors.
- On disk, git-ignored: `re/scratch/task21/` (specs, patch scripts `p_*.py` with `patchlib.py`, oracle output
  in `out/`).
