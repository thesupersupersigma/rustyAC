# The browser build: back to the launcher, reload, Low against High, frame cost (Task W1)

Four problems Super met playing the page on rustyac.com:

1. pressing Escape to go back to the launcher hangs the page;
2. reloading then fails about 99 % of the time (a new tab works);
3. the Low graphics settings are laggier than High (Full renderer);
4. the page should run faster and lighter.

Each was reproduced and measured before anything was changed, the cause found in the code, then
fixed. All runs are headless Chrome on this PC (Radeon Pro 5500M through Vulkan,
`--enable-features=Vulkan`), localhost only, the page served by `web/serve.py`.

## Resume here

- Done (steps 1 and 2 for problems 1 and 2): the hang's cause and fix (FMOD's worklet port),
  the two leaks (FMOD's bank files, the GPU device), proven over 10 trips; reloads from the
  launcher, a drive and the old hung state measured; `tools/proofs/browser.sh` runs
  `roundtrip_flow.py` (3 trips, 3 + 3 reloads) through `roundtrip_verdicts.mjs`: 4 lines, all
  IDENTICAL alone. `web/check.mjs --flow`: every step bounded, a hung page's stack, `reload`,
  `processes`, `may_fail`.
- Done (steps 3 to 5): problem 3 measured and not reproduced (Low cheaper than High here);
  problem 4's top costs, two cheap wins (bind groups kept, the call log's arguments) with the
  frame and kgl log proven the same, before / after; the missing-features list.
- Done (step 6): `web` rebased on `master` (v1.1.0), clippy clean, the browser area and
  `run_all.sh --quick` all IDENTICAL (see Checks). Ready to merge.
- Open: problem 3 on Super's machine; the GPU process still creeps a little per trip (below).
- This PC crashed (a blue screen) with a page run and a wasm clippy at once: run them one at a time.

## Checks (step 6)

After `web` was rebased on `master` (v1.1.0):

- clippy `-D warnings --all-targets`: rustyac-web for wasm32-unknown-unknown and native,
  rustyac-render native with and without `backend-wgpu`: clean. rustyac-render does not build
  for wasm32-wasip1 at all (off Windows it needs `backend-wgpu`; `replay_hash` there does not use it).
- `cargo test -p rustyac-render --lib kgl_log`: passes (the test now also holds that a line's
  names and hashes are not made without a capture).
- the browser area (`tools/proofs/browser.sh`): 237 IDENTICAL, 0 not, with the four new page
  lines (3 Esc round trips with sound, no bank file left, the GPU process under 1500 MB after
  each, 6 reloads). Two runs before that one failed only for the worktree: wasmtime does not
  follow the junctions `oracle/`, `re/` and `cardata/` out of its folders; browser.sh now gives
  their own folders too.
- `run_all.sh --quick` (1 h 16 min): physics 4, fresh 63, csp 45, sound 4, render 18, gui 29,
  python 1, replays 6, browser 15, shaders 4, launcher 1, dumps 6: all IDENTICAL. Two areas
  needed a fix first, neither in the game:
  - ai_page: its page `frameProof` ERROR'd after 120 s: W1's hang guard in `check.mjs` had
    bounded every page answer, also a `--script` that takes minutes. Now that limit is the
    run's `--timeout` outside a flow (120 s or the step's own in a flow): 9 of 9 IDENTICAL on
    run_all's retry.
  - net: the wasm way could not open `re/gold/net` through the worktree's junction
    (tools/net_gold/check.sh, as browser.sh): the area alone afterwards, 16 of 16 IDENTICAL.

These ran on `master` at 6d64b98. While they ran, `master` gained 4 commits (CSP docs, the CSP
oracle's `--drive`, and the car's short name read as `CarAvatar::initCommon`); `web` was
rebased onto them (38571b0) without conflicts, and not rebuilt or rerun there: the main
folder's proof run held the shared lock, and a build beside it would break rule 13. A rerun of
the browser area on the new base is the one check left.

The web zip: no file is new or renamed (the site's allowlist does not change); `sound.js` and
the wasm changed.

## How it was driven

`web/check.mjs --flow` with flows made by `re/scratch/taskW1/mkflow.py` and run by
`re/scratch/taskW1/flow.sh` (scratch, not kept). A round trip is what a player does: AC's
launcher, its Start (`ac://start`), the drive (it starts in AC's ESC menu), Esc, Esc, a real
click on the ESC menu's Exit at (355, 322) on a 1280 x 720 page, back to the launcher. The car
and track are set where the page reads race.ini after Start (the theme keeps its own choice).

New in `check.mjs --flow` for this:

- every step is bounded: a step's `timeout` (seconds), else 120 s for an answer; a page that
  does not answer fails the step with `hung: ...` and the step gets the page's JavaScript stack
  (`null`: the page does not even pause, it is busy outside script);
- `{ reload: true }` (the browser's reload; a drive's "leave the page?" is answered yes);
- `{ processes: true }` (the browser's processes by kind and their private memory, Windows);
- each step is printed on stderr as it starts, so a long flow can be watched.

## Problem 2: reloading fails

### Reproduced

10 reloads from each state (F2004 at Spa from the test pack, with sound), each to come back to
the launcher within 180 s:

| state | page | back | seconds to the launcher |
|---|---|---|---|
| the launcher | fixed | 10 of 10 | 7.7 to 33.1 |
| in a drive | fixed | 10 of 10 | 3.0 to 6.0 |
| hung at AC's Exit | before W1 | 0 of 1 (the run stops there) | never: the page still answers nothing after 180 s |
| hung, and no `beforeunload` listener (a probe) | before W1 | 0 of 1 | never |

### Cause

Problem 1. The reload is a same-site navigation, which goes to the same renderer process; its
main thread spins in FMOD's mixer and never takes the new document. The page's `beforeunload`
listener is not the cause (the probe without it stays hung too): nothing the page does can
rescue a reload once its main thread is spinning. A new tab worked because it gets a renderer
of its own.

### Fix

Problem 1's fix: the page no longer hangs, so there is no hung state to reload from. Reloads
from the launcher and from a drive came back 10 of 10 each (above).

## Problem 3: Low laggier than High

### Measured

New in the page's per-second stats (`window.rustyac.perf`, `Game::take_stats` in
crates/rustyac-web/src/web.rs) and in `check.mjs`'s report: the frame's wasm milliseconds split
into physics (`Session::advance` and the drawn frame), the picture's draw calls (`full::draw`
or the debug view's draw), sound, encode and submit (`present_full`); and the time from submit
to the GPU's "done" (`onSubmittedWorkDone`: the browser has no timestamp queries).

60 s of autodrive each, Ferrari F2004 at Spa from the test pack, sound on, 1280 x 720, the
page's presets (`?graphics=`; the page has Low, Medium and High: "max" is High with every
setting at its top, `re/scratch/taskW1/speed.sh`):

| picture | frames/s | wasm ms/frame | physics | draw | sound | submit | submit to GPU done |
|---|---|---|---|---|---|---|---|
| Full, Low | 33.6 | 28.1 | 1.9 | 21.4 | 4.5 | 0.2 | 70 ms |
| Full, Medium | 31.5 | 30.4 | 2.0 | 14.3 | 4.6 | 9.5 | 78 ms |
| Full, High | 32.2 | 29.2 | 1.8 | 23.0 | 4.1 | 0.3 | 76 ms |
| Full, max | 17.3 | 54.5 | 3.6 | 46.0 | 4.7 | 0.3 | 297 ms |
| Debug | 59.9 | 5.3 | 0.9 | 0.8 | 3.6 | 0.0 | 2 ms |

Frame pacing (every interval between two animation frames over 60 s, after 5 s of driving):

| size | preset | frames | p50 ms | p90 | p99 | max | over 50 ms |
|---|---|---|---|---|---|---|---|
| 1280 x 720 | Low | 1790 | 33.4 | 49.8 | 50.2 | 367 | 101 |
| 1280 x 720 | High | 1381 | 33.4 | 50.1 | 66.8 | 834 | 309 |
| 1920 x 1080 | Low | 2379 | 16.8 | 33.4 | 50.0 | 267 | 17 |
| 1920 x 1080 | High | 1807 | 33.4 | 33.5 | 50.2 | 484 | 104 |

At 1920 x 1080: Low 22.2 wasm ms a frame (draw 16.1) and 46 ms to the GPU's done, High 31.6
(draw 23.9) and 68 ms.

### Result: not reproduced

On this PC Low costs less than High in every number: wasm time, GPU time, pacing. What Low
does that High does not (render scale 75 % with AC's GUI drawn at the canvas's size, no
post-processing, no MSAA, no reflections, world detail 1, 512 px shadows) was read in the code
for a path that is slower: the scaled GUI keeps its full-size target between frames
(`Kgl::begin_gui_screen`), and nothing else is built per frame on Low only. No fix without a
cause: nothing was changed for problem 3. What would tell it on Super's machine:
`window.rustyac.perf` in the browser's console now gives the split and the GPU time; the same
two drives on Low and High there would show which part grows.

## Problem 4: the frame's cost

### The top costs

A 15 s CPU profile of the page's main thread while driving (DevTools' sampling profiler,
`Profiler.start`/`stop` in a `--flow`), self time by function; the function names from an
unoptimised build of the same code (`web/build.py --no-opt`, re/scratch/taskW1/site_named):

| | Full, High | share |
|---|---|---|
| 1 | bind groups made for every draw of every frame: `createBindGroup` + its wasm-bindgen wrapper + collecting them (`Kgl::end_pass`, crates/rustyac-render/src/kgl_wgpu.rs) | 27 % |
| 2 | `Kgl::vertex_buffer_map`: almost all of it the call log's FNV hash of every vertex upload, made when no log is kept (`format_args!` makes its arguments first) | 5.8 % |
| 3 | crossings from wasm into the browser's WebGPU (`wasm-to-js`) | 4.6 % |
| 4 | `Kgl::record` (a draw's state copied for the pass) | 3.5 % |
| 5 | `Scene::render` | 2.2 % |

Next were `kgl_log::name` (1.5 %, the same call-log waste as 2) and the sound bridge's calls.

| | Debug | share |
|---|---|---|
| | idle (the frame is done early: 60 frames/s) | 54 % |
| 1 | the sound bridge's `batch` (web/sound.js) | 3.7 % |
| 2 | wasm-to-js crossings | 2.7 % |
| 3 | FMOD's embind property sets (`__emval_set_property`) | 1.4 % |
| 4 | `event_get_3d_attributes` (web/sound.js) | 1.3 % |
| 5 | the bridge's text decoding (`decode`) | 1.2 % |

### The cheap wins (no pixel, no physics bit)

- **Bind groups kept.** `Kgl::end_pass` made three bind groups for every draw: the constant
  buffers', the textures' and the samplers'. The textures' and samplers' (and an empty constant
  buffers' one) are now kept by their layout and what they bind (`GroupKey`); the constant
  buffers point into each pass's new upload buffer and are still made per draw.
  shortcut: the cache is emptied whole at 4096 groups.
- **The call log's arguments.** `kgl_log::hash` and `kgl_log::name` return at once when no
  capture runs (they are only ever a log line's text; with a capture the log is as before).
  This also helps the desktop's Direct3D 11 path.

Proof of no change: the page's fixed Full frame (`?fullproof=3`, F2004 at Spa) on the old site
and the new one, Low and High: the screenshots byte for byte the same, and the kgl call logs
(17026 and 18074 lines) the same SHA-256 (re/scratch/taskW1/same.sh).

Before and after (60 s, F2004 at Spa, 1280 x 720, sound on):

| | frames/s | wasm ms/frame | draw | submit to GPU done |
|---|---|---|---|---|
| Full, Low, before | 33.6 | 28.1 | 21.4 | 70 ms |
| Full, Low, after | 47.8 | 20.9 | 14.9 | 49 ms |
| Full, High, before | 32.2 | 29.2 | 23.0 | 76 ms |
| Full, High, after | 45.9 | 20.7 | 15.3 | 53 ms |
| Debug, before | 59.9 | 5.3 | 0.8 | 2 ms |
| Debug, after | 59.7 | 7.4 | 1.1 | 2 ms |

(Debug does not use kgl: its numbers move by the run-to-run noise; it stays at the screen's 60.)
After: bind groups are 17 % of Full's main thread (from 27 %).

### The expensive ones, left

| cost | what it would take | size |
|---|---|---|
| the constant buffers' bind group per draw (17 % left) | dynamic offsets: one group per layout per pass, the offsets given at `set_bind_group`; the pipeline layouts change (`has_dynamic_offset`) and the upload's offsets must keep the 256-byte alignment | half a session, with the wgpu render proofs (`render_wgpu.sh`) |
| wasm-to-JS crossings for every WebGPU call | fewer calls: state set only when it changes between draws (`set_pipeline`, `set_vertex_buffer`, `set_viewport`), or render bundles for the static scene | a session |
| the sound bridge (3.6 to 4.9 ms a frame) | the batch is text parsed in JavaScript and FMOD's embind objects per call; a binary batch, and FMOD's mixer on the worklet's thread (a cross-origin-isolated page: COOP/COEP headers, which the site's nginx must send) | a session, plus the site's headers |
| `Kgl::record` and `Scene::render` | the scene walk itself (AC's own order is kept for the call log) | not cheap: the same code as the desktop |

## What the browser cannot do yet (step 5; for W2, nothing built here)

Checked in the page (this branch, F2004 at Spa and the launcher's BMW M3 at Laguna Seca):
**the setup screen is there**: the wrench in AC's ESC menu opens it, in a drive started from
the menu and from AC's launcher (Setup I/O, Gears, Tyres, the car's status, Load, Reset To
Default, Delete, Save). If Super sees none on rustyac.com, the site runs an older build, or the
drive was one where AC greys the wrench (out of the pits): to be looked at with him. What was
not shown working is below.

| what (desktop has it) | in the browser | what it would take | size |
|---|---|---|---|
| saving a setup by name | in one run the typed name did not show in "Save current setup as" and no setup appeared in the list (keys reach the GUI: `gui_char`); not proven either way | a page run that types into the field and saves; then that the file reaches the player's documents (`setups_dir` = the page's Documents `setups`) and survives a reload (the launcher's documents snapshot) | small (a session with the fix) |
| the setup screen's presets page, the online fixed setup | not done on the desktop either (Task 31) | the port first (desktop), then the page | a session |
| AC replays: record, save, load, watch (`.acreplay`) | none: the page records no replay (crates/rustyac-web/src/full.rs:143) | the replay recorder and player are in rustyac-game; the page needs the frame loop's replay mode and somewhere to keep files (OPFS) | 2 sessions |
| post-processing (YEBIS) and the saturation key | not in the page (README) | YEBIS's shaders on WebGPU: the DXBC translation exists (rustyac-dxbc), the passes and their targets do not | 3+ sessions |
| Python apps | not made in the page: they need the player's python33.dll | a Python in wasm (and the game's API across it) | large, several sessions |
| apps fed by the physics thread or Direct3D 11 counters: telemetry, setupdebug, volumes, ffb, eyes, gamestats, rstats | hidden (`PAGE_HIDDEN_APPS`, full.rs:108) | each its own: the page's physics feed for telemetry/setupdebug (small each); gamestats/rstats need the page's clock (shortcuts.md #10, #14) | 1 session for the lot |
| F8 screenshots | the page reads no picture back | a `mapAsync` read of the frame and a download | small |
| the benchmark | the theme says "not available in the browser version yet" (web/theme_glue.js:307) | the benchmark's replay and timing: after replays | 1 session after replays |
| saving `acos.ini`, `gameplay.ini`, `exposure.ini`, `last.ini` on exit | not saved (README) | the same as the apps' shutdown files (`shutdown_files`, Task 47d) | small |
| F5's keys-and-orbit camera | not in the page (README) | the free camera's key input in the page's loop | small |
| AC's two sound plug-ins, track sound occluders, online and replay sound states | left out (FMOD's HTML5 build runs no plug-in from a bank) | needs FMOD's HTML5 build to; occluders and states are the page's work | 1 session for what we can do |
| font hinting, CJK fonts | none (README; shortcuts.md #49) | a hinting rasteriser in ttf.rs, CJK fonts from the player's Windows fonts (the page has none) | 1 session |
| pit strategy (online) | not checked in this task | a page run on a local server copy with the strategy page | check: small |
| Special Events sorted by name, an opponent car that will not load ends the join | as the README says | small each | small |

Hardware the browser cannot reach at all (VR, TrackIR, triple screens' windows, Steam) is
left out of the list.

## Problem 1: back to the launcher hangs

### Reproduced

| source | car, track | sound | trips | result |
|---|---|---|---|---|
| the install (`?testfolder=`) | launcher's default (Lotus Elise SC, Magione) | none (a folder has no FMOD) | 10 | all back in 0.25 s |
| the install | Ferrari F2004, Spa | none | 3 | all back |
| the test pack (`re/gold/web/pack`, as the site) | Ferrari F2004, Spa | FMOD | 1 | **hung at the first Exit** |
| the test pack, `?sound=0` | Ferrari F2004, Spa | none | 3 | all back |

The hung page: its main thread at 100 % of one core for as long as it was left (2.8 h of CPU
on the first, unbounded run), the GPU process idle, no network connection open, no dialog;
`Runtime.evaluate` never answers and `Debugger.pause` never pauses (busy outside script).
On rustyac.com the page has the pack, so it always has sound: the hang is every time there.

### Cause

Markers along `returnToLauncher` (web/app.js) and every call of the sound bridge: the game is
freed (FMOD's `Studio::System::release` answers 0), the canvas is replaced, the launcher's
documents snapshot is taken (574 KB), and the page never comes back from the first `await`
after that (`saveDocuments`, web/launcher.js). At that await the queued tasks run: FMOD's
AudioWorklet node had sent mix requests, and its main-thread handler
(`port.onmessage = () => Module.FMOD_JS_MixerSlowpathFunction()`, in FMOD's own
`fmodstudio.js`) runs FMOD's mixer on the released system, which never returns. FMOD's output
close disconnects the node and closes the audio context but leaves the port's handler. With
the mix functions replaced after `release` (a probe), 16 late mix requests were counted and
the launcher came back in 0.25 s.

### Fix

`web/sound.js`, `studio_release`: after `release()`, the node's port handler is removed and the
port closed. The next drive's system makes a node of its own (FMOD is loaded once a page).

### The memory behind it

10 round trips (F2004 at Spa, sound) on the fixed page, after each trip back in the launcher:

| trip | wasm MB | JS heap MB | GPU process MB | renderer MB |
|---|---|---|---|---|
| 0 (before) | 4 | 18 | 218 | 295 |
| 1 | 1313 | 253 | 2120 | 1948 |
| 2 | 1313 | 484 | 2779 | 2326 |
| 5 | 1313 | 834 | 3512 | 2736 |
| 10 | 1313 | 1406 | 4062 | 3367 |

Sound started again on every drive. Two leaks:

- **JS heap, +115 MB a drive (only with sound).** Cause: `studio_load_bank_file`
  (web/sound.js) writes each bank into FMOD's in-memory file system as a new `bank<n>.bank`
  and never deletes it. Fix: the file goes with its bank (`bank_unload`, a failed load, and
  `studio_release`, which unloads every bank).
- **GPU process, about +200 MB a drive (with or without sound).** Cause: the page's device
  (`Gpu`, crates/rustyac-web/src/gpu.rs) is only dropped; in the browser that drops a
  JavaScript object, whose GPU memory goes when the garbage collector gets to it. Fix:
  `impl Drop for Gpu` destroys the device.

The same 10 trips with the three fixes:

| trip | wasm MB | JS heap MB | GPU process MB | renderer MB | back in the launcher |
|---|---|---|---|---|---|
| 1 | 1313 | 140 | 545 | 1755 | 7.0 s (the first) |
| 2 | 1313 | 141 | 379 | 1797 | 0.39 s |
| 5 | 1313 | 138 | 493 | 1777 | 0.42 s |
| 8 | 1313 | 451 | 629 | 1696 | 0.41 s |
| 9 | 1313 | 142 | 656 | 1796 | 0.70 s |
| 10 | 1313 | 510 | 924 | 2176 | 0.33 s |

Sound started on all 10 drives. The JS heap's 451 and 510 are the collector's timing (the next
trip is back at 140). The GPU process still creeps (545 to 924 MB over 10 trips, against 2120
to 4062 before): open, to be measured over more trips.

# W2: why Full mode falls back, which pack items it cannot draw, three small fixes

Full mode on montreal (a mod track whose CSP layout names the shader `stPerPixelNM_UVflow`)
drew the debug view on WebGPU, and the reason was only in `window.rustyac.notes`. The pack
cannot fix that: `tools/web_pack` already copies every shader of the install, and this one is
Custom Shaders Patch's, not the install's.

## Resume here (W2)

- Done: steps 1 to 3, the page check, the browser area, clippy; released as v1.1.2.
- Open: the `[debug view only]` labels and the warning before a drive of a marked pack item have
  no page check (the test pack has no marked item; montreal is 317 MB).

## Step 1: the reason on the page

When Full mode was asked for and the debug view draws, whatever the reason (no WebGPU, a shader
or file missing, an error), a banner at the top of the page says why, until it is closed
(`#fallback` in `web/index.html`, `showFallback` in `web/app.js`, the page's own style in
`web/style.css`). Its text is the notes' line that starts with "Full mode " (both reasons do),
so `window.rustyac.notes` is unchanged and says the same; `window.rustyac.fallback` holds it
too. Back in AC's launcher (Escape, Exit), the same line is the theme's own notice
(`api.notice`, recorded in `window.rustyac.launcher.notice`).

## Step 2: the pack marks what Full mode cannot draw

`web_pack` opens every KN5 of each packed car and track (and layout) with `rustyac_content::Kn5`
and lists the shaders its materials name that `system/shaders/win` lacks (both `_ps.fxo` and
`_vs.fxo`, as `Shader::get_shader` wants). The manifest's item gets
`"missing_shaders": ["stPerPixelNM_UVflow"]` (left out when empty, so a manifest without it is
the same as before); `--dry-run` prints them. The page labels a marked car or track
`[debug view only]` in its own lists, and a drive of one with Full asked shows the banner at
once: "Full mode can't draw this track (it needs Custom Shaders Patch: stPerPixelNM_UVflow): the
debug view will draw it." Nothing is refused, and Full mode is still tried (a mark can be older
than the install; the renderer's own error is the truth, and its line replaces the banner's).

Super's install, read-only (`web/my_pack.toml`: 189 cars, 56 tracks and layouts, nothing
skipped): **one item is marked, track `montreal`** (mod, 316.7 MB), missing
`stPerPixelNM_UVflow`. No car is marked.

## Step 3: three small fixes

- `web/sound.js:323` `InvalidStateError: Cannot resume a closed AudioContext` after leaving a
  drive: the click/key/visibility handler resumes only a context that is not `closed` (leaving
  a drive closes it).
- The browser's notes said "post-processing on: ... with YEBIS's filter ... from your acs.exe on
  Direct3D". The page has neither. `crates/rustyac-game/src/render/ac.rs` keeps that line only
  where YEBIS really starts (Windows, not `wgpu`); elsewhere it says "post-processing on: the
  game's post-processing frame (...), without YEBIS (its filter, tone mapping, ... run only in
  acs.exe on Direct3D 11); the light levels are those of post-processing off".
- `/favicon.ico` 404: the page links `favicon.svg`, the server panel's own icon
  (`server/panel/public/favicon.svg`, ours, MIT OR Apache-2.0), copied to `web/favicon.svg` and
  added to `web/build.py`'s site files. **New file in the web zip: `favicon.svg`**: add it to
  rustyACSite's `scripts/fetch-web-preview.mjs` allowlist. (A browser that asks
  `/favicon.ico` before reading the page still gets a 404; the page's own requests do not.)

## Checks (W2)

- `tools/proofs/browser.sh` has a new case, "Full mode's fallback banner (montreal)": the
  install served read-only (`serve.py --ac`), a drive of ks_ferrari_f2004 on montreal with
  `backend=full`; the banner is shown, says "Full mode could not start ... stPerPixelNM_UVflow",
  and its line is in the notes (`tools/proofs/fallback_page.js`). Alone it gave
  `{"shown":true,"text":"Full mode could not start, the debug view draws instead: the shader
  stPerPixelNM_UVflow is not in ac/system/shaders/win","same":true}`.
- `web_pack`'s tests (one new: a KN5 built in a temp folder, a shader with only `_ps.fxo`
  counts as missing) and `rustyac-web`'s `pack::` tests pass.
- the browser area (`tools/proofs/browser.sh`): 238 IDENTICAL, 0 not (237 of W1 plus the banner).
- clippy `-D warnings --all-targets --locked`: rustyac-web for wasm32-unknown-unknown and native,
  rustyac-game native and `replay_hash` for wasm32-wasip1, `tools/web_pack`: clean.
