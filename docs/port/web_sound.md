# Task 41: sound in the browser

## Resume here (Task 41b)

- Done: A.1 to A.4 and B.1 to B.4, v0.41.1 (see "Task 41b" at the end).
  Every car's sound in the page: an AI race's and two online clients' call
  logs are the desktop's once both get the same "is it playing" answers. The
  batching left Task 41's 18 logs unchanged but won no measurable frame time.
- Proofs: every area of `--quick` and `--changed v0.41.0` identical; the
  panel's unit and end-to-end tests pass.
- Next: see "Not done (Task 41b)".
- Scratch: `re/scratch/task41b/`. Disk free: 39 GB at start, 33 GB at end.

## Resume here (Task 41)

- Done: steps 1 to 5. The page plays AC's own sound banks through FMOD Studio
  2.03.15 for HTML5, served from the preview pack (never from the repository,
  the web zip or a release).
- Call log: 11 of the 18 audio oracle drives give the desktop's FMOD 1.08 log
  line for line in the page; the other 7 differ only in when some one-shot or
  looping events end (FMOD 2.03 vs 1.08, not our code, see "Call logs").
  Three drives in headless Chrome: the same bytes as the desktop's file.
- Sound: whole-drive loudness within 0.3 dB on 16 of 18 drives. The
  differences come from AC's two DSP plug-ins, which the page leaves out.
- Banks: all 139 banks of the install load, and all 2,330 events play, in
  FMOD 2.03.
- Next: v0.41.0; leftovers (see "Not done") go to Task 41b.
- Scratch: `re/scratch/task41/`. Disk free: 71 GB at start, 40 GB at end.

## How it works

- `tools/web_pack` copies `fmodstudio.js` and `fmodstudio.wasm` (Studio, the
  release build: no logging, no threads) from
  `ignored/fmod/fmodstudioapi20315html5/.../api/studio/lib/wasm/` into the
  pack's `fmod/`, plus `content/sfx/common.bank`, `content/sfx/GUIDs.txt`, every
  car's `sfx/` (bank, GUIDs.txt) and `data/sounds.ini`, and every track's
  `data/audio_sources.ini`. Without the FMOD download the pack is made as
  before and the page is silent.
- `crates/rustyac-audio/src/fmod/web.rs` (wasm only): each FMOD 1.08 function
  of `raw.rs` becomes one call, `rustyacSound.call(name, text, words)`, made by
  the same `fmod_api!` macro that builds the Windows function table. The sound
  code above the binding (engine, cars, track, camera, call log) is the
  desktop's, unchanged. `raw.rs` keeps the Windows DLL loading behind
  `cfg(windows)`; the NRT wait loops become `wait_for`, which on wasm checks
  once (the non-thread FMOD build loads synchronously).
- `web/sound.js`: translates 1.08 calls to 2.03's JavaScript API
  (`getLowLevelSystem` → `getCoreSystem`, `setParameterValue` →
  `setParameterByName`, `getMaximumDistance` → `getMinMaxDistance`, a version
  answer of 1.08.12, 1.08's default listener, struct layouts as words). Banks
  come from the page's file system (`sound_read_file`) into FMOD's own.
  Answers are returned as a new array: a bank read can grow wasm memory during
  the call, which would leave a view of the old memory empty.
- `crates/rustyac-game/src/audio.rs` (`GameAudio`, the desktop's) now builds
  for the page too; the page's session feeds it each physics step and calls
  `frame` each picture, as `rustyac.exe` does.

### Browser rules

- Start: FMOD's own resume handlers, plus the page's (click, key,
  touch, pointer). Browsers keep audio silent until the first one.
- Mixer: AudioWorklet, which FMOD picks itself when the browser has it (all
  current ones). The mix runs off the main thread ("non blocking/background
  audio mixing", FMOD's HTML5 notes), so a slow frame does not starve it and
  the mixer does not stall the game loop. The fallback, ScriptProcessorNode, is
  FMOD's own choice on old browsers.
- Volume and mute: on the controls page, kept in this browser's
  `localStorage` (`rustyac.sound`), on top of AC's own levels.
- A hidden tab: `mixerSuspend`, `mixerResume` when shown again.
- Credits: "Audio engine: FMOD Studio by Firelight Technologies Pty Ltd." in
  the page's footer (FMOD EULA clause 3: the credit line names "FMOD" and
  "Firelight Technologies Pty Ltd."; no logo is required) and in
  `LICENSING.md`.

## Results

### Call logs (desktop FMOD 1.08 against page FMOD 2.03)

`audio_oracle run --side port` on the 18 drives of `tools/proofs/sound.sh`,
against `sound_proof` in node on the same wasm the page loads
(`tools/web_checks/task41/all.sh`):

| drive | lines | result |
|---|---|---|
| f2004 spa_launch cockpit | 535,451 | identical |
| f2004 spa_launch chase | 531,263 | identical |
| f2004 spa_launch trackside | 530,786 | frame 280: `gear_ext` ends earlier |
| f2004 spa_grass cockpit | 355,399 | identical |
| f2004 spa_kerb_strike cockpit | 357,997 | identical |
| f2004 spa_wall_gravel chase | 442,707 | frame 813: `collisions/track/hit` ends earlier |
| f2004 spa_wall_high trackside | 444,386 | frame 348: `gear_ext` ends earlier |
| f2004 pt_shifter cockpit | 159,129 | identical |
| f2004 pt_protect cockpit | 132,721 | identical |
| f2004 brake chase | 212,823 | identical |
| bmw_m3_e30 wc_stops cockpit | 167,060 | identical |
| vrc_formula_alpha_2026 hy_deploy cockpit | 446,443 | frame 4: `engine_int` stops by itself (restarted at 4 and 8) |
| vrc_formula_alpha_2026 spa_hybrid_timing chase | 1,148,260 | frame 80: `wind` stops by itself |
| 250_gto trk_kerb_strike cockpit | 326,288 | identical |
| ferrari_f40 trk_lap cockpit | 1,432,797 | frame 1190: `backfire_int` ends earlier |
| bmw_m3_e30 trk_lap chase | 2,560,041 | identical |
| 488_gt3 trk_lap cockpit | 3,613,364 | identical |
| f2004 trk_lap cockpit | 3,354,327 | frame 2120: `collisions/track/hit` ends earlier |

Every difference starts at an `event_get_playback_state` answer: 1.08 still
says PLAYING (0) where 2.03 says STOPPED (2) or STOPPING (4). The plug-ins are
not the cause: the desktop with them left out (a temporary switch, reverted)
gives the same log as with them on two of these drives.

In headless Chrome (`tools/web_checks/task41/page_proof.sh`, `window.rustyac.soundProof`):

| drive | lines | SHA-256 | |
|---|---|---|---|
| f2004 spa_launch cockpit | 535,451 | 58ba2697…d76e | = desktop |
| f2004 spa_grass cockpit | 355,399 | 7be1804b…fc02 | = desktop |
| f2004 brake chase | 212,823 | 09de846d…4220 | = desktop |

The desktop machine has 5 sound devices and the log lists each one, so the proof
tells the page to answer 5 (it has the browser's 1 device otherwise, 20 lines fewer).

### What it sounds like

The page's mix (taken at FMOD's master group) against the desktop's WAV
writer, same tape, aligned by whole 800-sample blocks
(`tools/web_checks/task41/wavcmp.py`). Bands: octave bands from 31 Hz, mean of
page minus desktop.

| drive | whole (dB) | per second mean / max (dB) | bands (dB) |
|---|---|---|---|
| f2004 spa_launch cockpit | -0.0 | 0.3 / 2.5 | -0.2 … +0.2 |
| f2004 spa_launch chase | -0.1 | 0.3 / 2.6 | -0.2 … +0.8 |
| f2004 spa_launch trackside | +0.3 | 2.4 / 12.7 | -0.3 … +8.1 |
| f2004 spa_grass cockpit | +0.0 | 0.7 / 2.8 | -0.9 … +1.0 |
| f2004 spa_kerb_strike cockpit | +0.2 | 0.4 / 2.5 | -0.1 … +0.7 |
| f2004 spa_wall_gravel chase | +0.0 | 0.3 / 2.5 | -0.4 … +0.4 |
| f2004 spa_wall_high trackside | +0.1 | 3.8 / 59.2 | +7.6 … +9.7 |
| f2004 pt_shifter cockpit | +0.1 | 0.8 / 2.5 | -2.1 … +1.0 |
| f2004 pt_protect cockpit | +0.0 | 1.1 / 5.8 | -2.3 … +1.9 |
| f2004 brake chase | -0.1 | 0.1 / 0.9 | -0.3 … +0.9 |
| bmw_m3_e30 wc_stops cockpit | +0.1 | 0.2 / 1.0 | -1.9 … -0.1 |
| vrc_formula_alpha_2026 hy_deploy cockpit | +1.8 | 1.7 / 3.1 | -0.4 … +2.6 |
| vrc_formula_alpha_2026 spa_hybrid_timing chase | +1.1 | 1.0 / 4.3 | +0.1 … +3.0 |
| 250_gto trk_kerb_strike cockpit | +0.1 | 0.6 / 1.6 | -1.2 … +2.3 |
| ferrari_f40 trk_lap cockpit | +0.3 | 0.5 / 4.9 | -0.2 … +0.5 |
| bmw_m3_e30 trk_lap chase | -0.0 | 0.7 / 7.5 | +0.0 … +1.5 |
| 488_gt3 trk_lap cockpit | +0.0 | 0.4 / 3.2 | -0.3 … +0.2 |
| f2004 trk_lap cockpit | -0.0 | 0.3 / 2.5 | -0.1 … +0.1 |

The desktop against itself (same drive run twice): 0.0 dB mean, 0.3 dB max.
The trackside drives are louder at the low and high ends: AC's distance filter
plug-in, which narrows the sound with distance, is missing. The mod car's
+1-2 dB: its banks use AC's Gain plug-in, also missing. The rest is FMOD's
mixer: 2.03 vs 1.08, a different resampler, and voices that end at other
times.

### Banks (step 5)

`tools/web_checks/task41/bank_check.mjs`, in node on the wasm the page loads: every
bank of the install, then every event's sample data, an instance, a start and
5 updates (`re/scratch/task41/banks.tsv`).

| | banks |
|---|---|
| in the install (`content/sfx` 2, cars 137) | 139 |
| load as the page loads them (missing plug-ins allowed) | 139 |
| load without that flag | 4 (135: `54 A requested output, dsp unit type or codec was not available.`: they name AC's plug-ins) |
| events | 2,330, all play |

### Frame rate, sound on and off

Headless Chrome, Full mode on WebGPU, f2004 at Spa, autodrive, 40 s, twice each:

| | frames/s | main thread ms/frame |
|---|---|---|
| sound on | 54.8, 53.6 | 17.1, 17.5 |
| sound off (`?sound=0`) | 62.6, 65.0 | 14.9, 14.3 |

About 2.5 ms a frame: the game's sound code and its ~370 FMOD calls a
frame through JavaScript. Physics keeps about 330 steps a second either way.

### Regression

`tools/proofs/run_all.sh` (`re/scratch/task41/quick.txt`, `changed.txt`):

| area | `--quick` | `--changed v0.40.0` |
|---|---|---|
| physics | 9 / 9 | (not changed) |
| fresh | 47 / 47 | 657 / 657 |
| sound | 4 / 4 | 22 / 22 |
| render | 10 / 10 | (not changed) |
| gui | 15 / 15 | (not changed) |
| replays | 5 / 5 | 29 / 29 |
| net | 16 / 16 | (not changed) |
| browser | 9 / 9 | 226 / 226 |
| ai_page | 3 / 3 | 3 / 3 |
| shaders | 4 / 4 | (not changed) |
| launcher | 1 / 1 | 11 / 12, then 12 / 12 |
| dumps | 6 / 6 | 223 / 223 |
| wall clock | 31 min | 4 h 25 min |

The launcher's one difference (career list, 1 answer of 1,851): the game's
handler listed the screen's display modes three times over, the port once.
That answer comes from Windows at run time; the career list alone again gave
1,851 of 1,851 (`re/scratch/task41/career_out`).

- clippy `-D warnings`: native `--all-targets`, wasm32-unknown-unknown
  (rustyac-web, rustyac-net, rustyac-audio), wasm32-wasip1: clean. Builds
  `--locked`, the tool lock files included.

## Web zip and preview pack

- Web zip: one new file, `sound.js`.
- Preview pack (a delta upload): `fmod/fmodstudio.js`, `fmod/fmodstudio.wasm`,
  `content/sfx/common.bank`, `content/sfx/GUIDs.txt`, each car's
  `sfx/*.bank` and `sfx/GUIDs.txt` (where the car has one) and
  `data/sounds.ini` (where it is a loose file), and each track's
  `data/audio_sources.ini`. For the default pack: 2.6 MB of FMOD plus about
  160 MB of banks (common.bank 44 MB, the two cars 77 and 38 MB).

## Not done (Task 41b)

- AC's two DSP plug-ins (distance filter, gain). FMOD 2.03 HTML5 corrupts its
  memory when a JavaScript plug-in with parameters is registered with Studio
  (a later bank load fails with `RuntimeError: memory access out of bounds`),
  and a Core-registered one is not usable from a bank (`BindingError: Cannot
  use deleted val`). The log still prints both registrations, as on the desktop.
- Track occluders: `Geometry::addPolygon` is not in FMOD's HTML5 build. Spa has
  none; tracks that do have them give a different log and no occlusion.
- Online, replay and remote cars' sound states, the pit crew's guns, the Audio
  app's actions in the page.
- Opponents' sfx files are not asked for before a drive (the player's car's are).
- The 2.5 ms per frame could drop with fewer JavaScript crossings (one call per frame).

## Open questions

- Event lifetimes differ between FMOD 1.08 and 2.03 (the 7 drives above). That
  is FMOD's behaviour; whether a page-only workaround is worth it is open.

# Task 41b: other cars' sound, crew guns, fewer crossings, panel fixes

## How it works (Task 41b)

- Every car: the page's sound already took every car's state (`Feed` captures
  the player, the AI cars and the other players' cars); it now starts the same
  way as the desktop, through one shared `GameAudio::start_for` (the desktop's
  `start_audio` uses it too). The cars' banks: `files_wanted` names the `sfx/`
  of each car it is asked for, and the page asks for every car of race.ini
  and every car of the server before the drive.
- AI race proof: the desktop's `--replay --headless --audio-wav` loop moved
  into `GameAudio::replay` (`crates/rustyac-game/src/audio.rs`); the page
  runs the same loop through `race_sound_proof` (wasm export, the race as
  `race.ini` + `assists.ini` + seed, as `race_proof`).
- Online proof: a sound tape. With `?soundtape=<n>` the page keeps what its
  sound was given, frame by frame (every car's state, the events, the camera,
  the listener matrix, dt, pause, focus, backfires) and its FMOD call log;
  `window.rustyac.soundTape()` hands both back. `rustyac.exe --sound-tape
  <file> --audio-wav <wav> --audio-log <log>` hears the tape with FMOD 1.08.
  A live session cannot be run twice, so the two logs come from one session.
- Crew guns and the Audio app: the page's frame does what the desktop's
  window loop does (`desktop.rs`): the picture's crew asks whether a gun
  plays, its starts and stops go to the sound, the Audio app gets the sound's
  devices and its actions (master volume, mix levels, view sounds), the
  picture's backfires go to the sound, and the sound's frame runs after the
  picture, as in the game.
- Fewer crossings: the setters whose answer is always OK for a live handle
  (`event_set_volume`, `_parameter_value`, `_3d_attributes`, `_pitch`,
  `_timeline_position`, `_paused`, `event_start`, `event_stop`,
  `studio_set_listener_attributes`) wait in the wasm and go to JavaScript in
  one `rustyacSound.batch` with the next call that needs an answer; a null
  handle is answered in the wasm (`ERR_INVALID_HANDLE`, as FMOD answers it).
  `system_get_geometry_occlusion` is answered in the wasm (the HTML5 build has
  no polygons). About 370 crossings a frame become about 60.
- Controls page: one line says the browser's sound leaves out AC's two DSP
  plug-ins and the tracks' occluders.

## Results (Task 41b)

### AI race (`tools/web_checks/task41b/ai.sh`, `ai.txt`)

The player's bmw_z4_gt3 and three AI cars (ks_ferrari_488_gt3, ks_nissan_gtr_gt3,
bmw_m3_e30) at Laguna Seca, 60 s (20,430 steps), the desktop's tape. Page:
the wasm and FMOD 2.03 HTML5 in node, every car's files from the install.

| view | page | desktop (FMOD 1.08) | desktop, page's answers |
|---|---|---|---|
| cockpit, player | 3,706,869 lines | differs from frame 1915 | IDENTICAL |
| chase, player | 3,697,208 lines | differs from frame 1899 | IDENTICAL |
| chase, AI car 2 | 3,697,782 lines | differs from frame 1846 | IDENTICAL |

The plain difference is Task 41's: an event FMOD 1.08 still calls playing has
stopped in FMOD 2.03. With the page's answers to those questions the desktop
makes the same calls line for line. All five banks load in the page:
common.bank and the four cars'.

### Online, two clients (`tools/web_checks/task41b/live.sh`, `live/summary.txt`)

Two headless Chromes join one local acServer (Spa, pickup, no lobby, made-up
admin password) through the gateway, all on 127.0.0.1: a in the f2004 for
30 s, b in the Z4 joining 8 s later for 20 s. Each keeps a sound tape; the
desktop hears it.

| client | page log | desktop (FMOD 1.08) | desktop, page's answers |
|---|---|---|---|
| a | 1,444,202 lines | differs from frame 170 | frames IDENTICAL (1,437,392 lines) |
| b | 866,841 lines | differs from frame 5 | frames IDENTICAL (860,031 lines) |

- Compared from `# frame 0` on. The setups differ, as they must: the page
  plays on the browser's sound card (it tries the 5 devices it is told of and
  sets the output format), the desktop writes a WAV with no sound card.
- The plain difference: an event the page's FMOD still plays (real-time mix)
  the desktop's calls stopping (one 1/60 s block a frame). Same class as above:
  an answer to "is it playing", not a call of ours.
- Banks: both pages load common.bank, their own car's bank and the other
  car's (ks_ferrari_f2004 and bmw_z4_gt3). The server's 12 slots give 11
  other cars to the sound, on the desktop and in the page.
- A first run with a 45 s drive: client a's answer never came back (b's
  125 MB one did). The page hands its whole log back in one DevTools answer;
  most likely too big. The drives were made shorter.

### Task 41's drives after the batching (`tools/web_checks/task41b/same41.sh`, `same41.txt`)

All 18: the page's log is the same as Task 41's page log line for line, so
the desktop comparison is Task 41's (11 identical, 7 with the known FMOD 2.03
endings).

### Frame rate (`tools/web_checks/task41b/fps.sh`, `fps.txt`)

Task 41's drive (f2004 at Spa, autodrive, 40 s, headless Chrome, WebGPU), twice:

| page | frames/s | main thread ms/frame |
|---|---|---|
| Task 41, sound on | 58.0, 54.0 | 16.2, 17.5 |
| Task 41b, sound on | 57.8, 52.7 | 16.2, 18.1 |
| Task 41b, sound off | 66.9, 55.1 | 14.2, 17.0 |

No gain this PC can measure: the crossings were not the cost. Where the 2 ms
go instead (the game's sound code, `sound.js`'s translations, FMOD's own
work) was not measured. The runs scatter by 1 to 3 ms between twins.

### Panel (Part B)

- `npm test` (server/panel): 15 of 15, `npx tsc --noEmit` clean. New tests: a
  started server's two containers carry `com.docker.compose.project=rustyac-panel`
  and their own service name, not the image's `server` / `gateway-spa`; a new
  preset starts on Spa, the slots acServer would cut are filled over the chosen
  cars (and saving fewer cars is still allowed); the pack script prints the
  chmod step.
- The form: under the car total, when the cars add up to fewer than the max
  players, "only N of the M players can join: acServer lets on no more players
  than cars" and a button "fill the K slots" (spread round-robin over the cars).
- server/README.md: the pack upload ends with `chmod -R u=rwX,go=rX` on the
  pack folder (Windows' `scp -r` leaves `drwx------`, acServer runs as uid 999);
  `make_server_pack.py` prints it too.

### Regression (Task 41b)

`tools/proofs/run_all.sh` on the 0.41.1 tree (`re/scratch/task41b/quick_table.md`,
`changed_table.md`):

| area | `--quick` | `--changed v0.41.0` |
|---|---|---|
| physics | 9 / 9 | (not changed) |
| fresh | 47 / 47 | 657 / 657 |
| sound | 4 / 4 | 22 / 22 |
| render | 10 / 10 | (not changed) |
| gui | 15 / 15 | (not changed) |
| replays | 5 / 5 | 29 / 29 |
| net | 16 / 16 | 80 / 80 |
| browser | 9 / 9 | 226 / 226 |
| ai_page | 3 / 3 | 3 / 3 |
| shaders | 4 / 4 | (not changed) |
| launcher | 1 / 1 | 12 / 12 |
| dumps | 6 / 6 | 223 / 223 |
| wall clock | 30 min | 3 h 51 min |

- Panel: `npm test` 15 / 15, `npx tsc --noEmit` clean. Task 38g's end-to-end
  test (`tools/web_checks/task41b/e2e.sh`: 38g's, on this build's site, since 38d's
  old wasm no longer fits `web/app.js`): presets refused and saved, the
  launcher page lists the new server after the start, a headless Chrome joins
  it through the router and drives, the server stops 118 s after the client
  left, no containers left.
- clippy `-D warnings`: native `--all-targets`, wasm32-unknown-unknown
  (rustyac-web, rustyac-net, rustyac-audio; rustyac-web and rustyac-dxbc
  `--all-targets`), wasm32-wasip1, rustyac-render with wgpu: clean. Builds
  `--locked`, the tool lock files included (car_oracle's 7 old warnings, in
  files this task did not touch).

## Web zip and preview pack (Task 41b)

- Web zip: no new or renamed files (`app.js`, `index.html`, `sound.js` and the
  wasm changed).
- Preview pack: nothing new. Task 41's pack already holds every car's `sfx/`;
  the page now asks for the other cars' too.
- Server: rebuild the panel's image (`server/panel`) and restart it; already
  running servers keep their old labels until the panel starts them again.
  The gateway's code did not change (its version number did). Packs already
  on the server: run the `chmod` once.

## Not done (Task 41b)

- Cars in replays: the page has no AC replay player, so there is nothing to
  give their sound to. The desktop's replays have it.
- The pit crew's guns and the Audio app in the page: wired as the desktop
  does it, not proven by a run. The page has no hook that drives a pit stop
  or clicks the Audio app, and the sound tape does not carry them.
- AC's two DSP plug-ins and the tracks' occluders (left out, as the prompt says;
  the controls page says so).
- The frame time with sound: about 2 ms a frame still (see "Frame rate").

## Open questions (Task 41b)

- Where the sound's 2 ms a frame go in the page (a profile of `sound.js` and
  the wasm), and whether it is worth chasing.
- The answers to "is it playing" differ between FMOD 1.08 and 2.03, and
  between a real-time mix and the WAV writer; every log difference above is
  one of them.
