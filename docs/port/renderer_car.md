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
  - `ConstrainedObjectsManager` (`constrained.rs`, the `DIR_` nodes): proven on the E30 (8 nodes) and the F2004 (2) over 29 frames of a lap each;
  - `.ksanim` version 1 (`QuatPos::from_matrix`) and `AnimatedLights`: proven on the F40's pop-up lights (49 frames);
  - `SkidMarkBuffer`, `DynamicBuffer`, `CarAvatar::updateSkidMarks` (`skid.rs`; `Scene` has `RenderableObject` nodes): proven on the F2004's launch (119 frames, two marks growing);
  - the driver (`driver.rs`: both models, `driver_base_pos.knh`, skins, steering, the lever and paddle shift animations with `AnimationBlender` and the matrix lerp, head movement, visibility; `animator.rs` has mode-0 players): proven on cockpit sequences of 89 frames with gear changes on the E30, F2004 and 1M, and a far view (plain model); oracle views `onboard` and `dash` (the eyes with `GRAPHICS_OFFSET`; Task 20's `eyes` left it out and looks from above the roof), the Documents folder is a stand-in on both sides;
  - `AnalogInstruments`, `GearShiftShake`, `CarAnimations`, `RotatingObjects` (`cockpit.rs`): proven on cockpit sequences of the E30 (rev, speed, fuel, water needles with LUTs), F40 (turbo), 1M, F2004; the game has no wiper code at all;
  - - Done: digital displays (DigitalInstruments: text items, shift lights, LED series), identical on F2004, 1M, E30, F40 dash sequences; the logger now zeroes a discarded buffer (the game leaves part of GLRenderer's buffer unwritten)
  - - Done: smoke (ParticleSystem, ParticleGenerator, TyreSmoke, EngineSmoke; rand() of the main thread), identical on the F2004 launch (119 frames, --smoke 3) and grass run (89 frames, --smoke 5); the oracle has --smoke
  - - Done: exhaust flames (Flames version 2, BackfireParams and the trigger at the head of CarAvatar::update), identical on the F2004 lift-off (149 frames) and an F40 sequence (299 frames)
  - - Done: mirrors (MirrorTextureRenderer, CameraMirror, CarMirrorManager, VirtualMirrorRenderer; plain path, not HQ), identical on E30 cockpit, F2004 chase with the virtual mirror and smoke in the mirror (59 frames), 1M chase, F2004 dash; the oracle has --mirror <size> --mirror-smoke --virtual-mirror
- Next: the specs of the nine items (`re/scratch/task21/spec_*.md`, written by reader agents from the
  disassembly), then port and prove item by item in this order: damage glass and damage meshes, blur meshes
  and `lods.ini` rules, lights and disc glow, the driver, `DIR_` nodes, instruments, ground shadows, skid
  marks and smoke, the following cube map, mirrors.
- On disk, git-ignored: `re/scratch/task21/` (specs, patch scripts `p_*.py` with `patchlib.py`, oracle output
  in `out/`).
