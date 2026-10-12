# Task 26b: web controls settings, a personal content pack, Chrome's folder block

## Resume here

- **Done and committed:** all three problems, this report, `docs/web.md`.
  1. the Controls panel: `crates/rustyac-web/src/controls.rs`, `driver.rs`
     `WebSource::with_controls` / `set_controls`, `web.rs` `controls_*`, the page's panel
     (`web/app.js` "the Controls panel"), `selftest --set / --controls / --hold / --no-autodrive`
  2. the pack: `tools/web_pack` (no limit, `web/my_pack.toml`, `"*"`, left-out content with
     reasons, `--dry-run` sizes), `pack.rs` `pinned`, the page's ordered lists and search boxes
  3. the "Open it as a file list" button, `check.mjs --menu / --script / --pick-folder`
- **Version:** `0.26.1`, tagged `v0.26.1` (not pushed). See "Not done" for what was not rerun.
- Scratch: `re/scratch/task26b/` (`controls_web.sh` = the browser-against-desktop check,
  `panel_test.js` / `panel_test2.js` / `drive_panel.js` / `lists_test.js` = the page's own
  buttons through `check.mjs --script`, `base_v0260.txt` = v0.26.0's selftest hashes,
  `all.toml` + `all_dry.txt` = the whole-install dry run, `small.toml` = the test pack).

## Summary in plain English

Three things about the browser build were wrong on my own PC; all three are fixed.

1. **Steering.** The page drove with a built-in set of steering numbers, while `rustyac.exe`
   reads the player's own `controls.ini`. The page now has a **Controls** panel (a button in
   the menu, the **O** key while driving) with every number Assetto Corsa's pad and keyboard
   code reads from that file, under AC's own names. It can also take the whole file ("Load my
   controls.ini"): then the browser steers exactly as the desktop does with that file, keys
   and pad buttons included. A change works at once, in the middle of a drive, and is kept in
   the browser. As long as nothing is changed, the page behaves exactly as v0.26.0 did.
2. **Chrome and the game's folder.** Chrome refuses to open a folder under `Program Files`
   with its folder picker. There is now a second button, "Open it as a file list", that takes
   the same folder the way Firefox always has. In a test with Chrome 155 it took the real
   `C:\Program Files (x86)\Steam\steamapps\common\assettocorsa` (30,001 files) and the page was
   ready in under two seconds, with the same cars and tracks as the picker's way. That test
   answered the browser's file dialog by program; nobody clicked through the dialog by hand
   (see "What Chrome did").
3. **My own cars and tracks in the hosted pack.** The pack tool no longer stops at two cars
   and two tracks. A file of my own (`web/my_pack.toml`, not in git) says what goes in; `"*"`
   means everything in the install. `--dry-run` lists every car and track with its size and
   the total. Content the pack cannot use (DLC that is not owned, mods made for Custom Shaders
   Patch only, encrypted mods) is left out with a one-line reason and the build goes on;
   nothing is ever decrypted. With everything in my install the pack would be 18.9 GB: 129
   cars and 52 tracks and layouts, 29 of them mods. The page's car and track lists got a
   search box and show the default four first.

Nothing about the physics, the desktop program or multiplayer changed. The default pack
(`web/preview.toml`: two cars, two tracks) builds to a `manifest.json` that is byte for byte
the one v0.26.0's tool wrote.

## The Controls panel

Where: the menu's **Controls** section ("Steering and controls ..."), and the **O** key while
driving (Esc closes the panel and leaves the drive running). On a server too.

### The values

These are all the values `JoypadCarControl::JoypadCarControl` (0x1401d9280) and
`KeyboardCarControl::readFromIni` (0x14010ff60) read from `controls.ini`, checked against the
disassembly notes of Task 11 (`re/scratch/task11/spec_pad_keyboard.md`, sections 4 and 5), not
written from memory. "Range" is the panel's: where the class itself limits a value the range
is that limit; elsewhere it is what the formula can take (AC's launcher has its own slider
ends, which were not read: see "Open questions").

| Name in controls.ini | Panel label | Range (step) | Default | What it does |
|---|---|---|---|---|
| `[X360] SPEED_SENSITIVITY` | Pad: speed sensitivity | 0 to 1 (0.01) | 0.5 | How much less the stick steers as the car gets faster. **0 = off**: the full lock at any speed. At 1 the full stick gives 20 degrees of steering wheel from 200 km/h on. |
| `[X360] STEER_GAMMA` | Pad: steering gamma | 1 to 4 (0.01) | 2 | The stick's curve: 1 is straight, more makes the middle of the stick gentler. The class reads anything below 1 as 1. |
| `[X360] STEER_FILTER` | Pad: steering filter | 0 to 1 (0.01) | 0.7 | Smooths the steering: each physics step makes (1 - 0.99 x filter) of its move. 0 = off, 1 = very slow. |
| `[X360] STEER_DEADZONE` | Pad: steering dead zone | 0 to 0.9 (0.01) | 0.05 | The part of the stick around its middle that does nothing. |
| `[X360] STEER_SPEED` | Pad: steering speed | 0.1 to 1 (0.01) | 0.2 | The most the steering moves per physics step (a tenth of this number of the lock). The class keeps it between 0.1 and 1. |
| `[X360] RUMBLE_INTENSITY` | Pad: rumble intensity | 0 to 1 (0.01) | 0.8 | The strength of the pad's motors on kerbs and with sliding tyres. 0 = off. |
| `[X360] STEER_THUMB` | Pad: steering stick | LEFT / RIGHT | LEFT | The stick that steers (the class takes anything but `LEFT` as the right stick). |
| `[ADVANCED] COMBINE_WITH_KEYBOARD_CONTROL` | Pad: keyboard keys too | on / off | off | While the pad drives, the `KEY` of each action (gears, DRS, KERS ...) works beside its pad button. |
| `[RUSTYAC] USE_LEGACY_GAMEPAD_CODE` | Pad: legacy speed rule | on / off | off | AC's older speed-sensitivity rule with engine rumble. In AC this is `system/cfg/assetto_corsa.ini [GAMEPAD] USE_LEGACY_CODE`, not a `controls.ini` value; rustyAC keeps it in its own section. |
| `[KEYBOARD] STEERING_SPEED` | Keys: steering speed | 0.1 to 5 (0.05) | 1.75 | How fast the steering moves while a key is held: the part of the lock per second at a standstill. |
| `[KEYBOARD] STEERING_OPPOSITE_DIRECTION_SPEED` | Keys: opposite-direction speed | 0 to 10 (0.05) | 2.5 | Times the steering speed while steering against where the wheels point. |
| `[KEYBOARD] STEER_RESET_SPEED` | Keys: return speed | 0 to 10 (0.05) | 1.8 | Times the steering speed while no key is held: how fast the wheels come back to straight. |
| `[KEYBOARD] STEER_GAIN` | Keys: look-ahead gain | 0 to 1 (0.01) | 0.18 | Read by the class for its look-ahead on the AI line. That part is not in rustyAC (see `keyboard.rs` `step_steer`): **no effect**. |
| `[KEYBOARD] LOOKAHEAD_POINTS` | Keys: look-ahead distance | 0 to 20 (1) | 6 | The same: read, **no effect** here. |

Also read by AC's keyboard class and not used here (mouse steering is not in rustyAC):
`[KEYBOARD] MOUSE_STEER`, `MOUSE_SPEED`, `MOUSE_ACCELERATOR_BRAKE`. The panel says so.

The keys and pad buttons (`[KEYBOARD] GAS / BRAKE / LEFT / RIGHT`, every action's `KEY` and
`XBOXBUTTON`) are not edited one by one in the panel; they come with a loaded file, and the
panel lists what then drives.

### The buttons

- **Load my controls.ini**: pick `Documents\Assetto Corsa\cfg\controls.ini`. The bytes go to
  `ControlsIni::from_bytes` (the desktop's parser; UTF-8 or UTF-16), the same "is this a
  controls.ini" test and the same `Bindings::from_ini` as on the desktop. A file that is not a
  `controls.ini` is refused with a line of text and nothing changes.
- **Reset to AC's defaults**: back to the built-in layout, the numbers of the Default column.
- Kept in `localStorage` under `rustyac.controls` (the settings as a `controls.ini` text and
  where they came from). Every read and write is in a `try`; without storage the panel says
  that the settings last until the page is closed, and everything else works.
- **Live**: a change puts the new file into the driver's inbox; before the next physics step
  the two classes are built again from it (`WebSource::set_controls`). What the classes carry
  from step to step stays (where the keyboard's steering and throttle are, the pad's last
  state), so the wheel does not jump.

### About "AC's defaults"

Until something is changed the page uses what v0.26.0 used: rustyAC's built-in layout. Its
keyboard numbers are exactly those of AC's stock files. Its **pad** numbers are not exactly
either of the two stock files in the install, which also differ from each other:

| | built-in (the panel's default) | `cfg/controls.ini` in the install | `cfg/controllers/presets/Default.ini` |
|---|---|---|---|
| `STEER_GAMMA` | 2 | 3.01 | 2.0 |
| `STEER_FILTER` | 0.7 | 0.600999 | 0.7 |
| `SPEED_SENSITIVITY` | 0.5 | 0.5 | 0.1 |
| `STEER_DEADZONE` | 0.05 | 0.05 | 0.0 |
| `STEER_SPEED` | 0.2 | 0.2 | (missing) |
| `RUMBLE_INTENSITY` | 0.8 | 1.0 | 0.5 |

"Reset" goes to the built-in column, because that is the state every existing hash was made
with. Whether it should go to one of the stock files instead is an open question below.

For comparison, my own file has `SPEED_SENSITIVITY=0`, `STEER_GAMMA=1.4`, `STEER_FILTER=0`,
`STEER_SPEED=0.95`, `RUMBLE_INTENSITY=1`: the built-in 0.5 / 2 / 0.7 / 0.2 is what made the pad
so much heavier in the browser.

### Multiplayer and replays

The settings only change what the player's own keys and stick make of `CarControls`. Nothing
that is sent, hashed or checked on a server reads them, and a recording holds the controls
after the classes, so it plays back the same whatever the panel says.

## Proofs

1. **With the defaults nothing changed.** `selftest` of the v0.26.0 tree (built from a stash
   of this work) against the new tree, the car's whole state after the run:

   | run | v0.26.0 | now |
   |---|---|---|
   | `bmw_z4_gt3 ks_laguna_seca --steps 3000` | `6dc2082b06b7bcd6` | `6dc2082b06b7bcd6` |
   | `ks_ferrari_f2004 spa --steps 6667` | `bfe051587056e4ed` | `bfe051587056e4ed` |
   | `bmw_z4_gt3 spa --steps 4000 --keys KeyT@300,Shift+KeyT@600,KeyY@900,BracketRight@1200,BracketLeft@1500` | `77f885e8708e6831` | `77f885e8708e6831` |

   The page itself (headless Chrome 155, the real wasm build, no settings kept) gives
   `6dc2082b06b7bcd6` for the first run too. Unit test `the_defaults_are_the_built_in_layout`:
   the panel's defaults build the same `Bindings` as `default_ini()`.
2. **With a controls.ini the browser's controls are the desktop's.** Unit test
   `with_a_controls_ini_the_browser_drives_as_the_desktop_does` (`driver.rs`): a tape of 6,000
   steps (pad sweeps with triggers and every button, key presses of every key of the layout,
   the pad unplugged for a while, the speed going up and down) is fed to the browser's driver
   and to the desktop's classes, both built from the same file, and `CarControls` must be equal
   after every step (the steering to the bit) along with the device that drives. It runs for
   four files: the built-in layout; a file that changes every value of the table above with
   the right stick and other keys; the same with the newer speed rule; and, where the machine
   has one, the real `Documents\Assetto Corsa\cfg\controls.ini` (it passed with mine).
   *What "the desktop's" means here:* `LiveSource` reads the real keyboard and XInput and
   cannot be fed a tape, so the test builds the classes as `LiveSource::new` does and asks them
   as `LiveSource::acquire` does (the same class code; the choice of the device is the same
   few lines, written out in the test).
3. **The real browser with held keys and changed values** (`re/scratch/task26b/controls_web.sh`:
   the page's `&hold=38@50-3000,37@1200-1500,39@2000-2400` against `selftest --hold ...
   --no-autodrive`, Z4 GT3 at Laguna Seca, 3,000 steps):

   | case | page | desktop |
   |---|---|---|
   | default values, the keys drive | `68e4a529708af798` | `68e4a529708af798` |
   | `KEYBOARD.STEERING_SPEED=4, STEER_RESET_SPEED=0.3` | `684deba7c0e76a39` | `684deba7c0e76a39` |

   The two rows differ from each other, so the values did reach the classes in the page.
4. **The panel's own buttons** (`check.mjs --menu --script panel_test.js`, then a second visit
   with the profile kept, `panel_test2.js`): 14 rows; a slider drag and a typed number change
   the values (99 for the steering speed is kept at the range's 5) and are in `localStorage`;
   a `controls.ini` given to the file input shows gamma 1.4, steering speed 0.95 and the file's
   keys ("I / W, K / S"); a `video.ini` is refused with a line and changes nothing; Esc closes
   the panel; on the second visit the file's values are in use from the start; Reset gives the
   built-in values and removes the kept text. During a real drive (`drive_panel.js`): O opens
   the panel, two changes are taken while the car drives, Esc closes the panel and the car is
   still driving.
5. **Live rebuild**: unit test `new_settings_take_over_while_driving` (the stick held right at
   216 km/h is held back by the speed sensitivity; with it set to 0 the wheel goes on from
   where it was to full lock; the defaults again are the built-in layout again).
6. The whole test suite, clippy for the desktop and both wasm targets, and `--locked` builds:
   see the last lines of this section.

`cargo test --release --workspace --locked`: 265 passed, 0 failed, 1 ignored (54 test
programs). `cargo clippy --locked -- -D warnings`: clean for the workspace, for
`wasm32-unknown-unknown` (`rustyac-web`, `rustyac-net`), for `wasm32-wasip1` (the six crates
CI checks) and for `tools/web_pack`. (The suite ran in a second target folder,
`target/t26b`: `rustyac.exe` was running out of `target/release` at the time, and cargo
cannot replace a program that runs.)

**What proof 2 does not cover** (found by an independent read of the diff): it compares
`CarControls` and the driving device, not the extra buttons' events (traction control, ABS,
brake bias, hybrid keys), the headlights or the rumble; `STEER_GAIN` and `LOOKAHEAD_POINTS`
have no effect, so no test can see them; both sides read the file through the same
`from_file`, so the desktop's own file search (`Bindings::load`: which file is picked) is not
part of it; and the desktop side is the classes plus a copy of the few lines that pick the
device, not `LiveSource` itself.

## How to build your own pack

1. **Write the list.** Once: `cp web/my_pack.example.toml web/my_pack.toml`. Then edit
   `web/my_pack.toml` (git ignores it). The example has everything:

   ```toml
   cars = ["ks_ferrari_f2004", "bmw_z4_gt3", "*"]
   tracks = ["spa", "ks_laguna_seca", "*"]
   ```

   `"*"` = everything in the install; `"<track>/*"` = every layout of a track; a name = that
   car, track or `"<track>/<layout>"`. Each list on one line. Keep the first two of each: the
   multiplayer servers run the F2004 and the Z4 GT3 on Spa and Laguna Seca, and a browser can
   only join a server whose track and cars are in the pack.

2. **See what it comes to** (nothing is written; about a minute and a half for a whole install):

   ```
   cargo run --release --manifest-path tools/web_pack/Cargo.toml -- --dry-run
   ```

   One line per car and track with its megabytes, then `total`, then what is left out and why,
   then `mods in the pack`.

3. **Build it** (into `dist-web/preview/`; a file already there under the same name is
   written again):

   ```
   cargo run --release --manifest-path tools/web_pack/Cargo.toml
   ```

   Try it here first if you like: `python web/build.py`, `python web/serve.py`, open
   `http://127.0.0.1:8080/`.

4. **Copy it to the server.** The content first, the manifest last, so that the page never
   reads a list of files that are not there yet:

   ```
   scp -r dist-web/preview/content dist-web/preview/system tsss-server:/storage/rustyac/preview/
   scp dist-web/preview/manifest.json tsss-server:/storage/rustyac/preview/
   ssh tsss-server "chmod -R a+rX /storage/rustyac/preview"
   ```

   **The old files do not have to be removed first.** The page only ever asks for what
   `manifest.json` lists, a file of the same name is replaced by `scp`, and the browser keeps
   its own copies under each file's hash, so a changed file is fetched again by itself. Files
   of cars and tracks that are no longer in the list just stay on the disk unused. To get that
   space back, empty the folder before copying (this deletes the pack on the server, nothing
   else):

   ```
   ssh tsss-server "rm -rf /storage/rustyac/preview/content /storage/rustyac/preview/system /storage/rustyac/preview/manifest.json"
   ```

   No restart: the page asks for `preview/manifest.json` on every visit. (`docs/web.md` writes
   the folder as `/srv/rustyac/preview`; it is whatever the container's bind mount names.)

The same holds for `dist-web/preview/` on the PC: a rebuild with a shorter list leaves the
other files there. `rm -rf dist-web/preview` before the build gives a clean one.

### What a whole install comes to (the dry run of 12 October 2026)

`cars = [..., "*"]`, `tracks = [..., "*"]` on my install (203 car folders, 62 tracks and
layouts): **18,892 MB in 993 files**: 129 cars (6,697 MB) and 52 tracks and layouts
(12,196 MB). The largest cars are the `bdc_streetspec_*` drift mods (up to 344 MB each; a
Kunos car is about 40 MB); the tracks are between 157 MB (the drag strip) and 982 MB (the
Nordschleife's endurance layout). (While this task ran I installed
`bahrain_international_circuit`; the numbers are from after that.)

## What Chrome did with `webkitdirectory`

- **Setup:** Chrome 155.0.8059.39, headless, its own throw-away profile, the page's new
  "Open it as a file list" button pressed with a user gesture, the browser's file chooser
  answered through the DevTools protocol (`Page.setInterceptFileChooserDialog`, then
  `DOM.setFileInputFiles` with the folder) with the real
  `C:\Program Files (x86)\Steam\steamapps\common\assettocorsa` (`check.mjs --pick-folder`).
- **Result: it took the folder.** No refusal and no error. The input's list had **30,001
  files** (the whole install; `find` counts the same number). The page kept the 2,445 paths a
  drive can need, built its tree in 0.14 to 0.40 s and showed its lists 0.4 to 2.0 s after the
  folder was given: 133 cars, 56 tracks and layouts.
- **Same content as the picker's way:** the same folder read through the picker's code path
  (the page's test hook, `?testfolder=`) gave the same 133 cars, the same 56 tracks and
  layouts and the same 14 refusals, word for word, after every car had been checked.
- **What this did not test:** a person clicking through Chrome's own dialog. The protocol
  answers the chooser in place of the dialog, so two things a person sees were not seen: the
  folder dialog itself, and Chrome's question "Upload 30001 files to this site?". The block
  I ran into ("contains system files") belongs to the folder picker (`showDirectoryPicker`);
  I know of no such list for a file input, and the test above shows that Chrome's file-input
  code lists a folder under Program Files without complaint, but the click itself is yours to
  try. The hint under the button says what the button is for and that the browser will ask
  about an "upload"; if your Chrome refuses the folder there too, tell me and the hint gets
  rewritten.
- **A bug found on the way:** the game's own `server` folder has a `content/cars` of its own,
  and Chrome lists it before the real one. The page took the first `content/cars` it met as
  the game's folder, and so showed 86 cars without models. It now takes the highest one.
- **Speed with many files:** the browser makes the list (that part is Chrome's and took well
  under a second here); the page walks it once, keeps only car data, models, menu entries,
  tracks without their skins and sounds, and `system/data`, and reads a file's bytes only when
  the lists or a drive need them, as before. The background check of which cars load no longer
  walks the whole list once per car.

## Which mods made it, and what was left out

From the whole-install dry run (`re/scratch/task26b/all_dry.txt`).

**Mods in the pack: 19 cars, 10 tracks and layouts.**

- Cars: `bdc_streetspec_350z_v4`, `_ae86_v4`, `_altezza_v4`, `_aristo_v4`, `_e36_v4`,
  `_miata_v4`, `_r32_v4`, `_rx7_v4`, `_s13_v4`, `_s14_v4`; `vrc_1988_mclaren_mp4-4_r02`,
  `_r04`, `_r09`, `_r10`; `vrc_formula_alpha_2025`, `vrc_formula_alpha_2026`,
  `vrc_formula_beta_2024`, `vrc_formula_lithium_2023`, `vrc_formula_lithium_2023_csp`.
- Tracks: `bahrain_international_circuit` (`endurance`, `grandprix`, `inner`, `outer`,
  `paddock`, `test`), `ct_backwoods`, `montreal`, `vhe_interlagos` (`gp`, `norm`).
- `monaco_2017` is in the pack too and is not marked as a mod: its menu entry names no author
  (see "Open questions").

**Left out: 74 cars, 10 tracks and layouts.**

| What | Why (the tool's line) |
|---|---|
| 70 `ks_*` cars (`ks_abarth500_assetto_corse` ... `ks_toyota_ts040`) | not installed: the folder has neither `data.acd` nor a data folder (DLC that is not owned) |
| `urd_darche_992_23` | `suspensions.ini`: the file has no readable sections: this car's data is encrypted for Custom Shaders Patch |
| `vrc_formula_alpha_2025_csp`, `vrc_formula_alpha_2026_csp`, `vrc_formula_beta_2024_csp` | `suspensions.ini`: `COSMIC` is Custom Shaders Patch's own suspension: this car needs CSP |
| `ks_barcelona/layout_gp`, `ks_barcelona/layout_moto`, `ks_brands_hatch/gp`, `ks_brands_hatch/indy` | not installed: the layout's models file is missing: only the menu entry of a layout that is not owned (a DLC) |
| `ks_barcelona/layout_moto_fa_2026` | not installed: none of its 9 model files is there: the track it is a layout of is not installed |
| `ks_red_bull_ring/layout_gp_fa_2026`, `ks_silverstone/gp_fa_2026`, `montreal/montreal_fa_2026`, `monza/monza_fa_2026`, `spa/spa_fa_2026` | made for Custom Shaders Patch only: `acs.exe` cannot read its `data/surfaces.ini` (`WAV_PITCH=extended-0`) and would stop there |

Nothing was decrypted to find any of this out: an encrypted or CSP-only file fails in the
loader the desktop game uses, and its words become the reason. A car's model is also opened
once (as plain Assetto Corsa reads a kn5) and the car is left out if that fails or the model
carries an encryption marker; no car in my install was left out for that. The non-CSP twins of
the three refused VRC cars are in the pack.

**How the pack tool was checked**

- `web/preview.toml` (no `my_pack.toml` present): the new tool's `manifest.json` is **byte for
  byte** the one in `dist-web/preview/` from v0.26.0 (37 files, 1,455.1 MB), although the tool
  now loads each car and each track once instead of every car on every track.
- A test pack (`re/scratch/task26b/small.toml`: 18 cars and 7 track entries named, among them
  a mod car, a CSP-only car, a DLC car that is not owned, a name that does not exist, a
  CSP-only layout and `"ks_drag/*"`): built in 20 s, 15 cars and 9 tracks and layouts, 1,683 MB,
  the four unusable ones left out with their reasons. Served to headless Chrome: the lists show
  the named tracks first and the star's layouts after them by name (drag 400, 500, 1000, 2000);
  the car list has its search box ("bmw z4" leaves four cars, "formula ALPHA" one, "zzz" none
  and Drive goes grey). Three drives, 3,000 steps each, page against desktop:

  | drive | page | desktop |
  |---|---|---|
  | `vrc_formula_alpha_2025` (mod) on `ct_backwoods` (mod) | `af3b420c62c3f360` | `af3b420c62c3f360` |
  | `bmw_z4_drift` on `magione` | `f50a7cc8eb86b07b` | `f50a7cc8eb86b07b` |
  | `ks_ferrari_f2004` on `trento-bondone` | `6810af59d17711ac` | `6810af59d17711ac` |

  No file was missing in the page for any of them, so "each car alone, each track with one
  car" finds what any pair needs. The mod car on the mod track was then driven for 8 s with
  its picture (1,236 track meshes, 452 car meshes).

## Not done

- **The whole-install pack was not built or copied**, only dry-run (18.9 GB is yours to decide
  and to copy). The largest pack actually built and driven is the 15-car, 9-track test pack.
- **The long replay batches were not rerun**: `re/scratch/task20w/replay_proof.py` (160
  recordings through the desktop and wasmtime), `re/scratch/task26/proof.py` (the 80 network
  replays) and `re/scratch/task20w2/desktop_same.py` (old exe against new). They run code this
  task did not touch (a recording holds the controls after the input classes; the desktop's
  only changes are `ControlsIni::from_bytes`, split out of `load` without a change, and one
  function of the track catalogue made public). What was rerun is in "Proofs".
- **No real pad.** Headless Chrome has none, so the pad's path was checked with tapes (proof 2
  and 5), not with a hand on a stick. The rumble intensity reaches the class (same test) but
  nothing here felt it.
- **No live multiplayer run with changed settings** (see "Multiplayer and replays" for why it
  cannot matter).
- **A person's click through Chrome's own folder dialog** (see "What Chrome did").
- **Single keys and pad buttons cannot be rebound in the panel**: they come with a loaded
  `controls.ini`. A file that binds plain Shift, Ctrl or Alt (virtual keys `0x10` to `0x12`,
  not the left / right ones) to an action will not see that key in the page, which only sends
  the left / right codes.
- **The page's own keys against a loaded file.** T, Y, `[`, `]` and O give way to a file that
  drives with them. H, P, R, N, G, C, F1 and Enter do not: a file that binds one of them (AC's
  usual headlights key is H) gets both jobs on that key. The panel's layout table always
  names T and `]` / `[` for the traction control and the brake bias, also when the file took
  those keys for something else.
- **Wheels** (`[STEER]`, force feedback) are not in the panel: the browser build has no wheel.
- **Removing files of an earlier pack** is left to you (`rm -rf` as above); the tool never
  deletes anything.

## Open questions

0. **Can a car need a file only on a real track, or only on some tracks?** The tool loads each
   car alone on the flat road and each track with one car. For the default four that gives the
   very same file list as loading every pair, and three other pairs drove without a missing
   file, but it is not proven for every mod. If a drive from a big pack ever says a file is
   not in the pack, that is the place to look.

1. **Which numbers should "Reset to AC's defaults" give for the pad?** The built-in layout
   (now), the install's `cfg/controls.ini`, or `cfg/controllers/presets/Default.ini`? They
   differ (table above). I kept the built-in ones so that the default page is v0.26.0's.
2. **Slider ends.** AC's launcher (`AssettoCorsa.exe`, .NET) has its own slider ranges for
   these values; I did not read them out of it. The panel's ranges are the limits in
   `acs.exe`'s classes and, where there is none, what the formula can take. A loaded file's
   value outside a range is used as it is; only a value typed in the panel is kept inside.
3. **`monaco_2017`** is a mod but counts as Kunos' in the tool's summary: a track is taken as a
   mod when its menu entry names an author other than Kunos or it has an `extension` folder
   (the catalogue's rule since Task 18), and this one has neither. For cars the tool uses the
   game's own sound table instead, which is exact. Should there be a list of Kunos' tracks?
4. **The device at the start.** The desktop starts with the device `[HEADER] INPUT_METHOD`
   names; the page starts with the keyboard and hands over to the pad when it is touched (as
   before). With a loaded file this is the one difference left, for the moment before the
   first touch.
5. **`docs/web.md` names the server's folder `/srv/rustyac/preview`**, this task's prompt
   `/storage/rustyac/preview`. I left the how-to as it was and used the second one here.
