# Task 42b: the browser keeps the game's keys

## Resume here

- Done: step 0 (Full mode drives start as on the desktop), 0b (the lag measured on four
  releases; the debug HUD's writes cut), 0c (a loading screen), 1 (joining a race in
  progress checked against the game; the end of an online race ported), 2 (the panel's
  race-only note), 3 and 4 (every AC camera key in Full mode, the browser kept away from the
  keys during a drive).
- Proofs: all identical (see "Proofs" below). Released as v0.42.1.
- Leftovers are under "Not done".
- Scratch: `re/scratch/task42b/`. Disk free: 32 GB at the start, 26 GB at the end (the old releases' builds, 4.1 GB, deleted).

## Step 0: the drive that never started

What the player saw (v0.41.1, Full mode, a drive from AC's launcher in the page): the car in
its pit box with AC's pit menu, the lollipop man's red board, brake lights; the apps could
be opened and moved, but the car did not move and F1 / C changed nothing.

Why: two things, both in the page.

1. **The keys went to the launcher.** AC's launcher runs in a frame of the page. Its Start
   (or Join) left the keyboard focus inside that frame, now hidden, so every key went to the
   hidden frame and none reached the game. The mouse still worked, which is why the apps
   could be moved. `startDriving()` now takes the focus back to the page.
2. **The drive did not begin the way the game begins it.** `rustyac.exe` (as `acs.exe`)
   begins a drive in AC's ESC menu, with the car held and the start camera turning around it,
   until the player presses Drive (`Sim::startGame` 0x14019eb00, then the fade's job
   0x140195cf0: `startRace(10000)`, controls free). The page started the session without that
   menu, and Escape went to the page's own menu instead of AC's. Now the page does what the
   desktop does: the next offline Full-mode drive begins in AC's ESC menu (unless
   `system/cfg/options.ini [OPTIONS] DISABLE_SHOW_MODE` is set, as in the game), Escape is
   AC's, and Drive frees the car. Without AC's GUI (Debug view) Drive is pressed at once, as
   rustyac.exe does with `--no-gui`.

Found on the way, in both builds: while the car is held for Drive, its spawn sequence (470
steps) ran as a burst every frame. On the desktop that made the physics thread run unpaced
and the Drive press could be lost (car still at 0 km/h); in the page it hung the drive.
The sequence now waits while the car is held (`physics_thread.rs`, `session.rs`).

Proof (headless Chrome on the GPU, real input through the DevTools protocol, page
`?backend=full`; desktop `rustyac.exe --esc-start --pit-key drive@6`):

| drive | before Drive | after Drive + throttle | F1 |
| --- | --- | --- | --- |
| launcher Start, Magione, Lotus Elise SC, pit | 0 km/h, AC's ESC menu | 65.4 km/h, out of the pit lane | chase → chase 2 |
| page menu, Laguna Seca Practice, pit | 0 km/h | 80.1 km/h | chase → chase 2 |
| page menu, Laguna Seca Quick Race, grid | 0 km/h | lights, then 80.1 km/h | chase → chase 2 |
| desktop, same ESC start, Laguna Seca | 0 km/h without Drive | 60.2 km/h | — |

## Step 0b: the lag

Measured in headless Chrome on this PC's GPU, 20 s windows, CDP `Performance.getMetrics`
plus a frame timer in the page (`re/scratch/task42b/s0b/perf.sh`). Main-thread time is ms
of work per second (1000 = the main thread never rests).

| page | launcher idle | Debug drive | Full drive |
| --- | --- | --- | --- |
| v0.37.2 | 38 ms/s, 8.3 ms frames | 276 ms/s (layout 41) | — (no Full mode) |
| v0.38.9 | 36 ms/s | 244 ms/s (layout 35) | 997 ms/s, 55 fps |
| v0.40.0 | 35 ms/s | 226 ms/s (layout 31) | 994 ms/s, 55.5 fps |
| v0.42.0 + step 0 | 36 ms/s | 210 ms/s (layout 28) | 998 ms/s, 57 fps |
| this task | 35 ms/s (also after a drive) | **167 ms/s (layout 10)** | 998 ms/s, 57 fps |

What this says:

- **No release made the page slower** on this PC; the launcher at rest costs the same since
  v0.37.2 (AC's theme animates its own frame, about 120 calls a second; nothing of rustyAC's
  runs while no drive runs: the page's frame loop stops, no timers, one fetch in 20 s).
  After a drive and Escape back to the launcher the cost is the same as before the drive.
- **The Debug drive** spent a fifth of its main-thread time laying out the HUD's text, which
  it rewrote every frame. It now writes a box only when its text changes: 210 → 167 ms/s.
- **Full mode keeps the main thread busy all the time** (about 930 ms/s of it is the wasm:
  AC's renderer, the physics and the GUI). At 55-57 fps here that is the lag a slower
  machine feels first, launcher included while a drive runs. It is not new (v0.38.9 is the
  same) and it is not a fault one change removes; see "Not done".
- Memory: the wasm heap grows to 475 MB (Debug) / 740 MB (Full) and never shrinks (wasm
  memory cannot shrink); a second drive reuses it.

## Step 0c: the loading screen

Over the blue page while a drive loads, a panel at the bottom says in plain words what
happens: "checking files: 1278 the drive needs", "downloading: file 412 of 1278, 120 MB of
446 MB, 9.5 MB/s" (or "reading the folder" / "from this browser's storage"), "loading the car
and the track, preparing the shaders", "starting the session", with a bar when the total is
known. If nothing moves for 30 s it says "Nothing has moved for N s. The page still waits for
this: …" with **Copy diagnostics** and **Back to the menu**. The launcher's own look is not
touched (the panel is the page's, above it).

Proof: 50 distinct lines seen during a launcher Start; a run with the track's files held back
(DevTools `Fetch`) showed the stall line after 43 s, and "Back to the menu" reloaded to the
launcher.

## Step 1: joining a race in progress

| what | the game's code | the page | the desktop |
| --- | --- | --- | --- |
| join while the race runs | joins (closed only with `IS_OPEN=0`: `ACP_SESSION_CLOSED`, handshakeTCP 0x140048b50); the grid slot is set only for cars in the session message's grid (onNewSession 0x14004e4a0), others keep the pit box; no control lock (setCurrentSession 0x140140df0 frees every car) | pit box, AC's pit menu and the lollipop man's board; throttle → 80 / 118 km/h out of the pits; F1 works (two live runs) | pit box, drives |
| the red sign | `LollipopCrew::update` 0x140113ab0: the board is up while the car sits in its box; it is not a refusal | same | same |
| the race ends | after the results arrive, `ACClient::update` 0x140053b30 gently stops the car (gas 0, brake 0.2), and 5 s later (lambda 0x14003dde0) puts it in its pit box with the ESC menu open and **Drive greyed out**; the next session gives Drive back (0x140196830) | was missing: now ported | was missing: now ported |

So the user's online drive had the same cause as step 0 (the keys stayed in the launcher's
frame after its Join); what they saw around the car is AC's normal pit box. What does keep
players in the pits on a race-only server is the end of each race: until the server's next
race starts, Drive is greyed out. That part was missing in rustyAC and is ported
(`online.rs` `result_frame`, `sim.rs`, the GUI's `online_heard`), with a unit test of its
timer. It was not seen live: in both live runs the racing desktop car (autodrive) never
finished its lap on an overloaded machine (see "Not done").

## Step 2: the panel

The sessions box of a race-only preset says: "Race only: players who join while the race
runs start from the pits, and when it ends everyone waits in the pits until the next race.
Add a practice session for open driving." New presets start with Practice (20 min) and a
Race (10 laps), no Qualify. Proof: `npm test` (15 / 15) and the panel in headless Chrome
(`re/scratch/task42b/s2/out/form_new.jpg`, `form_race_only.jpg`).

## Steps 3 and 4: the keys

During a drive every key and the mouse's back / forward buttons are the game's
(`preventDefault`), except F12 (the browser's developer tools). The browser's own keys are
blocked: Alt+Left / Alt+Right, Backspace, F3, F5, Ctrl+R, Ctrl+F, Ctrl+P, Ctrl+S, Ctrl+D,
the Back key, mouse buttons 4 and 5. Leaving the page during a drive asks first
(`beforeunload`). The new **Full screen** button on the page's bar goes full screen, and in
full screen during a drive the page asks the browser to give it every key
(`navigator.keyboard.lock()`, Chrome and Edge), so Esc, Ctrl+W, Ctrl+T and Ctrl+N reach the
game too (holding Esc still leaves full screen). In the launcher the browser keeps all of its
keys.

Camera keys in Full mode (as rustyac.exe): F1 or C the next view, F6 the car's own cameras,
F11 the virtual mirror; controls.ini's `ACTION_CHANGE_CAMERA` (a pad button) as before. The
controls page lists them ("Keys in a drive").

Proof (headless Chrome, real keys and mouse through DevTools, `re/scratch/task42b/s4/`): each
key above was `defaultPrevented`, the page stayed the same document (no navigation, reload or
find bar, `history.length` unchanged) and kept driving; F1 went chase → chase 2 → bonnet, F6
to the car's camera 0, F11 showed the virtual mirror; then throttle 65.5 km/h.
`beforeunload` asks during a drive and not in the launcher.

## Proofs (`tools/proofs/run_all.sh`)

`--quick`: every area identical (physics 9, net 16, fresh 47, sound 4, browser 9, render 10,
ai_page 3, shaders 4, gui 15, python 1, replays 5, launcher 1, dumps 6), 32 min.

`--changed v0.42.0` (4 h 33 min):

| area | cases | identical | different | time |
| --- | --- | --- | --- | --- |
| net | 80 | 80 | 0 | 20m49s |
| browser | 226 | 226 | 0 | 44m37s |
| ai_page | 3 | 3 | 0 | 7m34s |
| fresh | 657 | 657 | 0 | 123m23s |
| sound | 22 | 21 → 22 | 1 → 0 | 8m05s, again 3m58s |
| replays | 29 | 29 | 0 | 68m46s |
| launcher | 12 | 12 | 0 | 5m18s |
| dumps | 223 | 223 | 0 | 67m50s |

The one sound case (`spa_wall_gravel chase`) crashed inside `fmod64.dll` (FMOD's own rare
crash, a known one); run again alone, all 22 were identical.

Clippy with `-D warnings` on every target (native `--all-targets`, wasm32-unknown-unknown,
wasm32-wasip1, render with wgpu, the web crate's wasm `--all-targets`) and every `--locked`
build (workspace and the tools) pass.

## Web zip and servers

No file was added to or renamed in the site; `app.js`, `index.html`, `style.css` and the wasm
changed. Redeploy: the site (the new zip), and the panel (its `public/index.html` and
`public/panel.js`; rebuild its image).

## Not done

1. The end-of-race rule (pits, Drive greyed until the next session) was not seen live; it is
   proven by the game's code and a unit test of its timer. `options.ini`'s
   `IGNORE_RESULT_TELEPORT` is not read, and there is no fade before the teleport.
2. Full mode's main-thread load (≈ 930 ms/s of wasm at 55-57 fps on this PC): moving the
   physics to a worker or cutting the renderer's per-frame work is a task of its own.
3. (Done in Task 42c, step 4) AC's launcher frame kept running while hidden during a drive.
4. (Done in Task 42c, step 4) F4 and F7 in Full mode.
5. Old builds were measured on this PC only; a slower machine was not tried.
6. The full-screen key lock: Task 42c tried it in headless Chrome (full screen and the lock
   work, Esc reaches AC's GUI). Holding Esc to leave full screen is the browser's own and
   headless Chrome does not do it, so that part is not seen.

## Open questions

1. Does the user want Escape held in full screen to stay the way out (the browser's rule), or
   a page button too? The button exists on the page's bar.
