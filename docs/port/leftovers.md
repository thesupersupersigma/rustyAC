# Task L1 — the leftovers before v1.0

## Resume here
- Done: (a) refused custom-filter keys, b8297a8. The frame draws without the key's effect,
  and the warning shows once a run on AC's banner (title "Post-processing filter") and in the
  log. Checked on the live exe with a custom filter that turns `[FEEDBACK] ENABLED=1` on:
  the banner shows, and the frame is pixel-identical to the default filter's apart from the
  banner rows (0..81) (`re/scratch/taskL1/shot/`, `l1_refused.jpg`).
- Done: chromatic aberration off (`[CHROMATIC_ABERRATION] ENABLED=0`) on a flat camera. The
  filter's enable is kept in `PostEffect::ca_enabled`, and the draw list without CA (VR's,
  proven in Y5d) is used when it is 0. The game: `initPostProcessing` 0x140024ca0 sets init
  flag 0x40000 only when CA is enabled; `setPostProcessing` 0x140025080 sets CA only then.
  Row `rl1_01` (test filter `rustyac_caoff`): command log and pixels IDENTICAL in all 175
  frames. Shimmer + CA off on a flat camera is not ported; it draws with CA and warns once.
- Step 1 table: written below.
- Done: the saturation key (`Sim::onKeyDown` 0x14019a940): Shift+Ctrl+numpad + / - moves
  GraphicsManager +0x11c (the lighting saturation) by 0.01 (f32), clamped to 0..2, and shows
  "Saturation: N%" (`(int)(s * 100.0f)`); nothing while the console shows. Desktop only.
  Proof: unit test `saturation_step_clamps_at_0_and_2`; the saturation itself draws as in Y7e.
- Next: step 2 in order: the FMOD crash cause, render_oracle's clippy errors, the scratch archive
  (`re/scratch/taskL1/archive_scratch.sh`, not run yet); then step 3.
- Still refused per frame (not filter keys): glare resample sizes, a glare halving with
  several taps, the heat shimmer in VR; these still skip YEBIS for the frame.

## Table

Every "Not done", open question and leftover in `docs/port/*.md` since Task 43, plus the older
open ones still true (collected in `re/scratch/taskL1/collected.md`). Items later tasks already
did are left out. **Hits**: whether a player meets it (yes / rare / dev-only). **Size**: small /
medium / big. **Plan**: *L1* = fixed in this task; *gap* = a v1.0 known gap (README list);
*after* = after v1.0 (Enhanced mode, CSP, custom-filter work).

### Post-processing (YEBIS)

| Item | Source | Hits | Size | Plan |
|---|---|---|---|---|
| A custom filter's refused key gave black frames | yebis.md (Y7f) | yes | small | **L1, done** (b8297a8): banner + log once, drawn without the key |
| `[CHROMATIC_ABERRATION] ENABLED=0` drew CA on | yebis.md | rare | small | **L1, done** (VR's draw list for the flat camera, row rl1_01) |
| Heat shimmer with CA off on a flat camera | L1 | rare | medium | gap (drawn with CA, warned once) |
| Per frame: glare resample sizes the port lacks (skips YEBIS that frame) | yebis.md (Y5) | rare | medium | gap |
| Per frame: glare halving with several taps | yebis.md (Y5) | rare | medium | gap |
| Per frame: heat shimmer in VR (no CA) | yebis.md (Y3e) | rare (VR) | medium | gap |
| Custom-filter keys drawn as the shipped filters: HDR=0, tone map functions 0/3-8, tone map viewport, feedback, lens distortion, AE delay, vignette FOV, colour temperature < 4000 K, vintage afterimage / anamorphic flare | yebis.md (Y7b/Y7f) | rare | big | after (custom-filter work) |
| Airy disc (b&w, sepia at very narrow FOV / huge width) | yebis.md (Y7f) | rare | big | after |
| More than 2 DOF front levels; DOF + shafts / shimmer; mask variants other than S4 | yebis.md | rare | big | after |
| Library-shape ghosts, SHAPE_GHOST_DISTORTION / SHARPENESS / CONCENTRIC=0 | yebis.md | rare | medium | after |
| BackgroundBlurFactorNormalize not drawn | yebis.md | rare | medium | after |
| Untested: glare without AE, star counts other than 1/5/6/12, QUALITY other than the rows | yebis.md | rare | small | gap (untested, not known wrong) |
| Measured shortcuts: pool f9 list, pair clear order, teardown counted not walked, shimmer copy sampler, diaphragm aperture term, frame 0 startup not compared, one more blend/A2C set | yebis.md | dev-only | medium | after |
| Open: which +0x248 target glare[1] is; why the game fails DOF+glare resize; s1024 frames 10-13 once; extra W×H f9 in some game runs; CA weights as immediates | yebis.md | dev-only | – | after |
| rustyac.exe draws more rand() for shimmer sets than acs.exe; no shimmer row in tools/proofs | yebis.md | dev-only | small | after |
| Numpad saturation key (Shift+Ctrl+numpad + / -) | yebis.md (Y7e) | yes | small | **L1, done** |
| Saturation in the browser page | yebis.md (Y7e) | rare | medium | gap |
| Full mode in the page has no YEBIS | web_graphics.md | yes (page) | big | gap |
| Start lights use summed frame times, not the wall clock | postfx.md | rare | small | gap |

### Game, GUI and apps

| Item | Source | Hits | Size | Plan |
|---|---|---|---|---|
| Friends Leaderboard rows made but not drawn | screens_steam.md (47e) | rare | medium | gap |
| MOTION_TRACKER=RIGHT_HAND with no right controller | screens_steam.md | rare (VR) | medium | gap |
| Server FORCE_VIRTUAL_MIRROR read but not applied | screens_steam.md | rare | small | gap |
| Headless without --drawn-frames: no achievement checks | screens_steam.md | dev-only | small | after |
| Mouse pit menu online is the OnlinePit HUD lines | leftovers_43b.md | rare | medium | gap |
| Start Benchmark (BenchmarkMode not ported) | content_tools.md (44d) | rare | medium | gap |
| Photo mode: END / ray picker, FOV / roll before Start, exposure start value | content_tools.md | rare | medium | gap |
| `--screenshot` of the photo window shows FOV 0 | content_tools.md | dev-only | small | after |
| Driver model `data_<config>` save path (MODEL_CONFIG unused) | content_tools.md | dev-only | small | after |
| FormAI window, 3D splines, AUTO_FUEL_CONS, AUTO_SHIFTER copy, side spline indexes | content_tools.md | dev-only | big | after |
| AI line swap with several cars (after all cars, not inside car 0's step) | content_tools.md | rare | medium | gap |
| Kept AI lap not the live AI line at once (bestLapSpline) | content_tools.md | rare | small | gap |
| Camera editor doesn't read back its saved cameras.ini / dash_cam.ini; AUTOEXP quit after save | content_tools.md | dev-only | small | after |
| DRS zones sent by a server (0x53) decoded, not applied | content_tools.md | rare | small | gap |
| race_out.json after an online server's next race | content_tools.md | rare | small | gap |
| Console axisMode, lodDebug, editBoxes / editCMesh, observe* reloads, cockpit exposure | dev_tools.md | dev-only | medium | after |
| ACSuspension TempRun button writes nothing (write guard) | dev_tools.md | dev-only | small | after |
| Telemetry RIDE_HEIGHT_* one step late at the 2 m/s edge | dev_tools.md | rare | small | gap |
| Online: remote cars have no Car, Setup debug shows the player's car | dev_tools.md | dev-only | medium | after |
| Online tape car's delta may stay 0 in net_drive; online / replay graphics page fields not in the oracle | dev_tools.md | dev-only | small | after |
| Python: getCarTyreCompound etc. keep defaults; setCameraMode 3/4/5/7, freeCameraSetClearColor only logged; setCameraMode reads its old mode until the next frame | python_apps.md, cameras.md | rare | medium | gap |
| Delete's blur (CameraForwardYebis +0x7a0) not carried; F5 keys-and-orbit desktop only | cameras.md | rare | small | gap |
| camera_manager.ini written into rustyAC's cfg each run (open) | cameras.md | dev-only | – | after |
| Open: RACE_OVER_TIME=150 had no visible effect | content_tools.md | rare | – | gap |
| C++ plugin SDK, CSP Lua | python_apps.md | – | big | after (CSP) |

### Browser page

| Item | Source | Hits | Size | Plan |
|---|---|---|---|---|
| Page doesn't save acos.ini / gameplay.ini / exposure.ini / last.ini (GameGui::shutdown) nor other apps' shutdown files | screens_steam.md | yes (page) | medium | gap |
| Special Events sorted by name (a browser folder handle has no dates) | content_tools.md | rare | – | gap (no dates to sort by) |
| Opponent car that won't load is fatal for a join | web_fixes.md | rare | small | gap |
| No reconnect (AC has none) | web_fixes.md | rare | – | gap (as AC) |
| AC's two DSP plug-ins, track occluders, online / replay / remote sound states, pit guns, Audio app, opponents' sfx prefetch | web_sound.md | yes (page) | big | gap |
| Event lifetimes differ FMOD 1.08 vs 2.03 | web_sound.md | rare | – | gap (FMOD's) |
| No TrueType hinting, no CJK fallback fonts, no frame counts for stats apps on wgpu | web_hud.md | rare | medium | gap |
| End-of-race teleport rule not seen live, IGNORE_RESULT_TELEPORT not read, no fade before teleport | web_keys.md | rare | small | gap |
| Full mode main-thread load ≈930 ms/s; slower machines untried | web_keys.md | yes (page) | big | gap |
| View settings outside video.ini; settings change at the next drive; Medium preset rustyAC's own | web_graphics.md | rare | small | gap |
| Launcher: benchmark notice, ?launcher=0 graphics preset, no rustyAC tab icon, raw `<l18n>` in Profile filters, Clear cache / Changelog / language unexercised | web_settings.md | rare | small | gap |
| Locked look / F7 cinematic / mouse wrap not checked in the page (headless) | cameras.md | dev-only | – | after |
| Preview pack lacks career / event pictures; TA event tier not driven in the page | game_modes.md | dev-only | small | after |
| Open: frame-rate limit wording, desktop logs no ac://eventcount, tab column at ~900 px, page leave button, Chromebook stored copy, gateway idle timeout 300 s, python.ini ACTIVE default | several | rare | – | after |

### Proofs and tooling

| Item | Source | Hits | Size | Plan |
|---|---|---|---|---|
| Flaky FMOD crash in a sound proof case (fmod64.dll, frame 1408; same rerun alone passes) | prompt, web_keys.md | dev-only | medium | **L1**: find the cause (rule 20) |
| render_oracle's 41 old clippy errors | yebis.md | dev-only | small | **L1** |
| car_oracle's ~70 old lints (rustc 1.97) | screens_steam.md | dev-only | small | after |
| Old scratch folders (~11 GB) to the server | prompt | dev-only | small | **L1** (archive_kept.sh's method, not task46c) |
| Six render rows lost their made-up car data; four sound drives' .ryin lost; Task 36 lobby recording; Task 37c Race Weekend tape | proofs.md | dev-only | medium | after |
| re/gold/fresh/rec grows ~12 GB per car_oracle source change | proofs.md | dev-only | medium | after |
| Fifth lane, D: drive, keeping the game side of compares | proofs.md | dev-only | – | after |
| dumps.sh leaves out conditions_spa_setup_launch; drift drive headless at an uneven frame rate | game_modes.md | dev-only | small | after |
