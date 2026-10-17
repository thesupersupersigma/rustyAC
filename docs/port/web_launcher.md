# AC's launcher in the browser, keyboard steering (Task 37)

## Resume here

Task 37b (AC's launcher in the browser page):

- Part 1 working behind `?launcher=1` (the old menu is still the default): the theme from
  the preview pack in a frame (`web/sw.js` serves it, `web/theme_glue.js` runs in it,
  `web/launcher.js` holds it), its `ac://` requests answered by the desktop bridge built for
  wasm (`crates/rustyac-web/src/launcher.rs`), the player's documents in OPFS. Only the Drive
  tile shows. `tools/web_pack` packs the launcher's files (`launcher` in the manifest).
- Part 3 working (Practice proven, flow `re/scratch/task37b/flows/practice.json`): the
  theme's Start writes race.ini through the bridge, the page reloads into the in-page game
  made from race.ini + assists.ini (`crates/rustyac-web/src/race.rs`, the desktop's
  `conditions::apply` order and `rand()` draws, player only; `Game.create_race`), leaving
  writes `out/race_out.json` (the referee's) and the page reloads into the theme, told
  ACRunningEvent + ACStoppedEvent as the desktop shell does. A new player's `video.ini` gets
  the page's size (else the theme stops every Start for its hidden Options page).
- Session kinds: race.ini identical to the desktop launcher's for Practice, Hotlap, Quick Race,
  Race Weekend (`re/scratch/task37b/desk.sh` + `web.sh`); sessions with AI are refused with
  the theme's notice. Part 4 working: the theme's Online page lists servers.json's servers
  (`launcherLobby` in web/app.js), Join starts the page's multiplayer join from race.ini
  [REMOTE] (local acServer copy `re/scratch/task37b/server` + gateway, flow `online.json`).
- Next: the plain start page and the folder path, controls page, old menu removal, pad tape.
- Scratch: `re/scratch/task37b/` (test pack `pack/`, readers' notes `theme_internals.md`,
  `web_game_raceini.md`, `pad_oracle.md`). Serve: `python web/serve.py --pack
  re/scratch/task37b/pack --port 8137`; check: `node web/check.mjs --url
  "http://127.0.0.1:8137/?launcher=1" --launcher --profile target/web-check-37b`.

Task 37:

- Part 0 done (commit fd4fb66): the 20 multiplayer goldens made again under `re/gold/net/`
  (`tools/net_gold/`, see docs/port/multiplayer.md "Golden tapes"); one port bug found and
  fixed on the way (collision reports); 80 of 80 checks identical.
- Part A done: the keyboard's `stepSteer` is the game's, proven against the game in-process;
  speed sensitivity; the controls page; the launcher's cfg seeding.
- Part B.1 done: the pure-Rust ini copy (`crates/rustyac-launcher/src/profile.rs`) and the
  bridge building for wasm; proven against Windows and the desktop bridge.
- **Next (Task 37b): B.2 to B.6**, the theme in the browser page, nothing of it started (see
  "Not done" at the end). Start from `re/scratch/task37/theme_menu.md` (which of the theme's
  JS decides the menu, what Drive and Online call) and `web_now.md` (the page today, the pack,
  the game's start); the wasm bridge needs a browser `Host` and a `files::Files` on OPFS.
- Notes from reading (not committed): `re/scratch/task37/launcher_split.md`,
  `theme_menu.md`, `web_now.md`.

## Summary

- **Multiplayer goldens back.** The five tapes and 20 goldens Tasks 25–26 checked against
  were gone; they were recorded again from live sessions and now live in `re/gold/net/`,
  with the scripts to remake and check them in `tools/net_gold/`. Doing so found a port bug
  (a collision on the very step of a collision report went into the next report); fixed.
  All 20, four ways each: identical to the game.
- **Keyboard steering is the game's now.** The look-ahead along the track's AI line
  (`stepSteer`) was missing (it gave 0); it is ported 1:1 and checked against the game's own
  code run in-process: 120,000 calls, identical.
- **A speed-sensitivity setting** for the keyboard (rustyAC's own; 1 = AC exactly, 0 = speed
  doesn't limit or slow the steering) and a **controls page** to set it and the pad's, opened
  by the launcher theme's controls button.
- **The launcher fills its cfg from the player's real AC cfg** on first start (missing files
  only, the real folder only read).
- **The launcher's bridge runs without Windows**: a pure-Rust copy of Windows' ini calls
  (identical to Windows on 20,000 generated files) and a file layer, so the bridge builds for
  the browser; its wasm build answers Task 35–36's 12 request lists exactly as the desktop.
- **Not done:** the theme inside the browser page (B.2 to B.6): the browser still has the old
  menu, unchanged. See "Not done".

## How to use it

Desktop:

- Keyboard steering needs nothing: the look-ahead is on whenever the track has an AI line.
- In the launcher (`rustyac-launcher.exe`), the theme's controls button opens the controls
  page: move "Speed sensitivity" (or press "Off" for 0), Save, Back. It writes the launcher's
  `cfg\controls.ini` (`[KEYBOARD] RUSTYAC_SPEED_SENSITIVITY`, `[X360] SPEED_SENSITIVITY`, ...);
  the next drive uses it. The same key can be set by hand in `cfg\controls.ini`.
- First start of the launcher: the player's `Documents\Assetto Corsa\cfg\*.ini` that the
  launcher's own `cfg` lacks are copied in. `--player-cfg <folder>` names another folder,
  `--player-cfg none` turns it off.
- `rustyac-launcher.exe --pure-ini` uses the pure ini copy instead of Win32 (for checks).

Multiplayer goldens: `sh tools/net_gold/record.sh`, `sh tools/net_gold/gold.sh`,
`sh tools/net_gold/check.sh` (docs/port/multiplayer.md, "Golden tapes").

Browser: nothing new yet (the old menu is as it was).

## New or renamed files in the web zip

None: nothing under `web/` changed in this task.

## Disk space

44 GB free at the start, 34 GB at the end (see "End checks").

## Part 0: the multiplayer tapes

Where they live, how to rerun them and the results: docs/port/multiplayer.md, "Golden tapes:
where they live and how to make them again (Task 37)". In short: `re/gold/net/tapes/` (five
tapes), `re/gold/net/*.netgold` (20 goldens); on v0.36.0 19 of 20 were made and 76 of 76 checks
identical; the 20th showed the collision-report bug; after the fix 20 of 20 and **80 of 80
identical** (desktop, WebSocket, gateway, wasm under wasmtime 49.0.3).

## Part A: the keyboard's steering

### A.1 `KeyboardCarControl::stepSteer` (0x140111650), 1:1

Before this task the port's `step_steer` gave 0, so the keyboard's speed limit was always the
plain `π/(0.5·v+1)`. In the game, `stepSteer` looks ahead along the track's AI line
(`AISplineRecorder`'s best-lap spline, which `load` fills from `ai/fast_lane.ai`):

- with no line it puts the car on the nearest point once (`closestPointIndex`, 0 for an
  empty line) and the look-ahead point is the world's origin;
- with a line the index stays where the constructor put it (-1: the game never moves it),
  `getCarPositionOnSpline` projects the car on the segment there, and `getPointAtDistance`
  walks forward until `lookAhead` (8 m by default) is reached;
- the point in the car's own frame (`dBodyGetPosRelPoint`), `-x · steerGain`, not finite → 0,
  ±0.9, the understeer branch (never taken: nothing sets it), ±0.9.

`getKeyboardSteering` then keeps whichever is larger, that or `π/(0.5·v+1)`. The port
(`crates/rustyac-game/src/input/keyboard.rs`) follows the machine code's order of operations
(checked with `tools/disasm.py`, not the pseudo-C). Its car pose is the `CarProbe` the
keyboard already had (position and left axis of the body's world matrix); the line is the
track's, given to the live driver by `set_track` (desktop and browser).

One shortcut, commented in the code: the game's walk along the line never stops if the line is
shorter than the look-ahead; the port stops after one lap of points.

**Oracle.** `car_oracle steer-micro --seed <n> --root <folder>` builds the game's own objects
in-process: a `PhysicsEngine`'s rigid body (the game's ODE), a `KeyboardCarControl` with six
sets of settings (AC's first), and an `AISpline` loaded by the game's `AISpline::load` from a
copy of Laguna Seca's `fast_lane.ai` (2,331 points), or none. Every call puts the body
somewhere new (near the line, far from it, turned, rolled), with random speed, step, keys and
last steering, calls the game's `stepSteer` and `getKeyboardSteering`, and writes what went in
and came out (hex bits) to a tape. `keyboard_steering_check` (an example of `rustyac-game`)
plays a tape through the port.

| Tape | Calls | Workers 0 | Workers 1 | Workers 8 |
|---|---|---|---|---|
| seed 1, on the line | 20,000 | IDENTICAL | IDENTICAL | IDENTICAL (8 runs) |
| seed 1, no line | 20,000 | IDENTICAL | IDENTICAL | IDENTICAL (8 runs) |
| seed 2, on the line | 20,000 | IDENTICAL | IDENTICAL | IDENTICAL (8 runs) |
| seed 2, no line | 20,000 | IDENTICAL | IDENTICAL | IDENTICAL (8 runs) |
| seed 3, on the line | 20,000 | IDENTICAL | IDENTICAL | IDENTICAL (8 runs) |
| seed 3, no line | 20,000 | IDENTICAL | IDENTICAL | IDENTICAL (8 runs) |

The line's points the game loaded are compared with the port's `AiSpline::load` too (all the
same). Fault test (`--fault`: `STEER_GAIN` one bit off): caught at call 1 on the line, at
call 32 without one.

Found on the way: the game's `steeringMovement` reads the car's own speed
(`Car::getSpeed`, +0x3e90), not the input's; in the game they are the same number, and the
port uses the input's for both. The oracle writes the speed into its stand-in car.

### A.2 Speed sensitivity (rustyAC's own setting, not part of vanilla AC)

`controls.ini` `[KEYBOARD] RUSTYAC_SPEED_SENSITIVITY` (0 to 1, default 1) scales the speed `v`
in both places the keyboard's steering uses it: the limit `π/(0.5·v+1)` and
`steeringMovement`. 1, or the key missing, is AC's own rule bit for bit (the tapes above,
played again with the key set to 1: IDENTICAL); 0 takes the speed out (a unit test: held
steering at 80 m/s reaches full lock with 0, about π/41 with 1).

The pad already has AC's own `[X360] SPEED_SENSITIVITY`; the controls page's pad slider
writes that key. There is no pad oracle in this task: that the port reads it as the game does
is from Task 26's port, not proven again here.

### A.4 The controls page

`crates/rustyac-launcher/src/controls.html`, rustyAC's own page (no Kunos files): the
keyboard's speed sensitivity (with an "Off" button: 0) and its three steering speeds, the
pad's speed sensitivity (with "Off"), steering speed and filter. It reads and saves the
launcher's `cfg\controls.ini` through the bridge's own `getiniasjson` / `savejsontoini`. On the
desktop the theme's controls button (`ac://showcontrols`) opens it in the launcher's window;
"Back" goes home to the theme. Checked with a launcher script (off screen): the page opened, a
sensitivity of 0.4 and the pad's Off were saved (`RUSTYAC_SPEED_SENSITIVITY=0.4` and
`[X360] SPEED_SENSITIVITY=0` in the file), Back returned to the theme.

### A.5 The launcher's cfg files seeded from the player's (rustyAC's own)

The launcher's player folder is rustyac.exe's, not `Documents\Assetto Corsa`. On a window
start (not `--transcript`, so the Task 35/36 comparisons are untouched), every `.ini` of the
player's real `Documents\Assetto Corsa\cfg` that the launcher's own `cfg` lacks is copied
first, then AC's own setup runs (and merges its defaults in, as always). The real folder is
only read. `--player-cfg <folder|none>` names another one.

Checked (`re/scratch/task37/seed.sh`, a fake "real" cfg folder): first start, `controls.ini`
and `video.ini` came from it, a `.txt` did not; second start, the launcher's own changed
`controls.ini` was kept; the real `Documents\Assetto Corsa` hashed the same before and after.

## Part B.1: the bridge without Win32

### The pure-Rust copy of the ini calls

`crates/rustyac-launcher/src/profile.rs` is rustyAC's own copy of what
`GetPrivateProfileStringW` and `WritePrivateProfileStringW` do with a file of the ANSI code
page (1252): the trimming, the quotes, comments, the first matching section only, ASCII-only
case, the list mode and its short-buffer rules, where a new key or section goes, what a
deletion takes with it (and what it leaves: comment lines at a section's end), the line
endings Windows adds. Every rule came from Windows itself (PowerShell probes in
`re/scratch/task37/ini_probe*.ps1`), not from a guess.

`crates/rustyac-launcher/tests/profile.rs` generates files (sections, keys, values, quotes,
comments, blanks, odd lines, all line endings, characters of 1252 and ones outside it),
makes every read the launcher makes plus short buffers 0 to 8, then up to five random writes
and deletions, and compares buffers, answers and the files' bytes with Windows' own calls on
a real file:

| Files | Reads | Writes | Result |
|---|---|---|---|
| 3,000 (the test's default) | 77,844 | 7,387 | all the same as Windows |
| 20,000 (`PROFILE_CASES=20000`) | 519,272 | 49,575 | all the same as Windows |

Left out (28 of the 20,000 files): files Windows itself takes for UTF-16 (it guesses so for
some tiny files, such as `[` and a line feed); the copy reads every file as ANSI, as the
launcher's files are (shortcut, commented in the code).

`ini.rs` uses the copy where there is no Win32 (the browser); the desktop keeps Win32, and
`rustyac-launcher --pure-ini` switches it to the copy. Task 35 and 36's request lists through
the desktop bridge twice, Win32 and `--pure-ini` (`re/scratch/task37/pure_cmp.sh`, the 12
lists that need no server): **every answer and every file of the player's folder the same**,
except the one answer that carries a time (`getresults`' date is `race_out.json`'s write
time, made during each run).

### The bridge builds for the browser

- Every file call of the bridge goes through `files.rs`: the disk (`Disk`), or files the page
  mounts (`files::mount`; `MemFiles` is an in-memory store with Windows' rules: any case,
  `\` or `/`, folders must be made, names listed in NTFS's order).
- `lib.rs` is no longer Windows-only: the shell (WebView2), `sys.rs` (the Win32 host) and the
  online jobs' network code stay `cfg(windows)`; in the browser `online::job` has nothing to
  do (the page's own server list, Part B.5). The backup of the launcher's browser data is
  desktop-only (the browser launcher has none).
- `transcript::Fixed` is a host with the same screen, times and player everywhere, and
  `examples/transcript.rs` runs a request list with it: built for `wasm32-wasip1`, it is the
  bridge's wasm build with a real file system under it.

Task 35 and 36's 12 request lists, the desktop (`transcript.exe`, Win32 ini) against the wasm
build under wasmtime 49.0.3 (pure ini), the AC folder and the player's folder given to it as
`/acroot` and `/player` (`re/scratch/task37/wasm_cmp.sh`; the folders' names are made one
token, and `getresults`' date is left out):

| List | Answers | Player's folder |
|---|---|---|
| hotlap | 1,974 of 1,974 identical | identical |
| options | 1,843 of 1,843 | identical |
| practice | 1,956 of 1,956 | identical |
| quickrace | 1,974 of 1,974 | identical |
| raceweekend | 2,007 of 2,007 | identical |
| results | 6 of 6 | identical |
| career | 3,756 of 3,756 | identical |
| champs | 4,012 of 4,012 | identical |
| guid | 3 of 3 | identical |
| replays | 14 of 14 | identical |
| special | 3,824 of 3,824 | identical |
| videomodes | 2 of 2 | identical |

Under node's own WASI (node 26.4.0, `re/scratch/task37/wasi_node.mjs`) the same module can't
list a folder on Windows (`fd_readdir` answers ENOSYS, os error 52), so every list that reads
the content folders differs; that is node's, not the bridge's. The browser itself is Part B.2.

## End checks (nothing else changed)

On the final code (`re/scratch/task37/end_a.sh`, `end_b.sh`, `drives.sh`; v0.36.0 built into
`re/scratch/task37/tgt_old` for the comparisons, deleted afterwards):

| Check | Result |
|---|---|
| clippy `-D warnings --all-targets`, the CI line, wasm32-unknown-unknown, wasm32-wasip1 | clean |
| `cargo build` / `cargo test --release --workspace --locked` | built; 65 suites, 313 passed, 0 failed |
| `python web/build.py`, the nine tools, the .NET launcher oracle | built |
| physics dumps against v0.36.0: `spa.ryin` | the same (`c175020dafc14eeb`); state hash `af262162d7ae332b` with 0 and 8 workers, both versions |
| physics dumps against v0.36.0: `magione_ai.ryin` | the same (`726ded63cbc5848b`); state hash `b0d997d76d84e9cd` with 0 and 8 workers, both versions |
| the 20 multiplayer goldens, four ways | 80 of 80 identical (Part 0) |
| the committed multiplayer tape, desktop / browser / gateway | identical, 3,000 steps |
| plain command line, Content Manager's race.ini | run, exit 0 |
| Tasks 35 and 36's launcher answers against the .NET launcher | hotlap 1,974/1,974, options 1,843/1,843, practice 1,956/1,956, probe 59/59, quick race 1,974/1,974, race weekend 2,007/2,007, results 5/6 (the date, as before), smoke 10/10, career 3,756/3,756, championships 4,012/4,012, replays 14/14, special events 3,824/3,824, video modes 2/2; the player's folders identical; the real Documents folder unchanged |
| the keyboard tapes (Part A), 0 and 8 workers | 12 of 12 identical |
| GUI sequences of Tasks 29 to 34 (41 sequences, setup pages included) | all exit 0, 82 of 82 comparisons IDENTICAL, none different |
| recorded drives (`oracle/game/*.ryin` and Task 36's two), desktop against wasm32-wasip1 under wasmtime 49.0.3 | 158 of 158 identical at every step, 1,659,570 steps (two needed their setup file mounted: a copy of the player's `god.ini` and `re/scratch/task15/god_tc4.ini`) |

Disk: 44 GB free at the start, 34 GB at the end.

## Not done (Task 37b)

Parts B.2 to B.6 were not started: the time went to Part 0 (one port bug found in the
goldens, fixed), the ini copy's many Windows rules, and the end checks. The browser's page is
unchanged: its old menu is still the default and still works (nothing half-wired in it).

- **B.2** the launcher's theme in the page, from the preview pack (and "Open your AC folder"),
  the `.base` libraries injected as `JSIncludeDefaultLibs` does, `ac://` routed to the wasm
  bridge; `tools/web_pack` taking `launcher/themes/` (the pack's files to add are not yet
  listed).
- **B.3** only Drive (practice, quick race, race weekend, hotlap) and Online reachable. From
  the reading: `getthememodules` only feeds the module manager; the menu and Drive's tabs are
  in the theme's `index.html` and `js/ui.js`, so the page has to hide or refuse the others
  (career, special events, time attack, drift, drag, replays, options' other pages,
  `ACGotoCareerEvent` and its kind).
- **B.4** race.ini from the theme to the in-page game and back with results (the page's game
  takes no race.ini today: `rustyac-web`'s `content::setup` fixes the session).
- **B.5** Online from `web/servers.json` through the gateway, as lobby entries (the theme also
  asks servers' own HTTP ports, which the page cannot reach: a proxy is needed).
- **B.6** the old menu removed; and the controls page in the browser (A.4's browser half: the
  page is ready to be served, it takes `window.rustyacBridge`).
- Proofs 5 (in a headless Chrome; node's WASI cannot list folders on Windows), 6 (the page's
  screenshots) and the browser half of 4.

## Open questions

- The pad's `SPEED_SENSITIVITY` is AC's own key; is a pad oracle (like the keyboard's) wanted,
  to prove the slider's 0 against the game?
- The keyboard look-ahead without a track line aims at the world's origin, as the game's
  does (nothing loaded = no `fast_lane.ai`): keep it the game's, or give rustyAC its own
  fallback (the old 0)?
