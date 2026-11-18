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
- Next: problem 3 (step 1's frame-time table per preset, then the cause), problem 4 (top 5 frame
  costs, cheap wins), step 5 (the missing-features list), step 6 (browser area, wasm clippy,
  rebase on master).
- This PC crashed (a blue screen) with a page run and a wasm clippy at once: run them one at a time.

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
