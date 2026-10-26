# Task 41: sound in the browser

## Resume here (Task 41b)

- Done: A.3 (setters kept back and sent in one crossing, occlusion answered
  in the wasm: Task 41's 18 page call logs unchanged line for line), B.1 to
  B.4 (panel: compose labels, fill the slots, Spa default, chmod step).
  A.2 (crew guns, Audio app) wired in the page, not yet proven.
- Next: A.1 other cars' sound and its proofs (AI race, 2-client online),
  A.4 controls line, report, v0.41.1.
- Scratch: `re/scratch/task41b/`. Disk free: 39 GB at start.

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
(`re/scratch/task41/all.sh`):

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

In headless Chrome (`re/scratch/task41/page_proof.sh`, `window.rustyac.soundProof`):

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
(`re/scratch/task41/wavcmp.py`). Bands: octave bands from 31 Hz, mean of
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

`re/scratch/task41/bank_check.mjs`, in node on the wasm the page loads: every
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
