# Task S2 — the shortcut sweep

## Resume here
- Step 0 done (2e95c3a): README known gaps and `leftovers.md` rechecked, eight stale entries removed.
- Step 1 done: every `shortcut:` marker is in the table below with its verdict.
- Step 2: #18 ported, #9's options.ini key ported (its fade stays a known gap); every
  other marker in `crates/` now reads `shortcut (<verdict>, shortcuts.md #N): ...`.
- Step 3: quick lane, clippy, release v0.52.0.

## Verdicts
- **ported**: done as the game does, with its proof.
- **unreachable**: the simplified path cannot differ for anything the game or its content does;
  how it was checked is in the row.
- **known gap**: reachable and not small. The big ones are README gaps; the rest (developer
  apps, the browser's missing clocks, and *game faults*: where acs.exe crashes, hangs or reads
  past an array and rustyAC does something sane) are covered by one README line pointing here.

Totals (1 ported, 17 unreachable, 36 known gap) over the 54 markers in `crates/`; the 6 in
`tools/` only state what they leave out. Line numbers are those before this task; find a
marker now with `grep -rn "shortcuts.md #N" crates`.

## Proofs of the ported ones
- **#18 Wings app, every wing**: `FormWings::render` 0x1402edff0 loops over all of
  `car->wingsStatus`. Installed cars carry up to 23 `[WING_n]`+`[FIN_n]` (vrc_formula_alpha_2025;
  19 its CSP version, 18 vrc_2026, 15 rss_formula_hybrid_2026; Kunos cars at most 12), so
  `MAX_WINGS` is 32. The Wings app's GUI compares in the quick lane stay IDENTICAL.
- **#9 IGNORE_RESULT_TELEPORT**: `ACClient::ACClient` 0x14003a2c0 reads `system/cfg/options.ini
  [OPTIONS] IGNORE_RESULT_TELEPORT` (`hasKey`, then `0 < getInt`); `ACClient::update` 0x140053b30
  then skips the gentle stop and the teleport. Unit test `ignore_result_teleport_is_a_positive_key`;
  the shipped file has 0, so the online cases are unchanged.

## The table
| # | where | what | verdict | how |
|---|---|---|---|---|
| 1 | `crates/rustyac-game/src/achievements.rs:780` | no avatar texture and no Friends Leaderboard rows on screen (the calls are | known gap | README gap (Friends Leaderboard). |
| 2 | `crates/rustyac-game/src/achievements.rs:838` | runs only where frames are drawn (the window, or headless with --drawn-frames), | unreachable | acs.exe always draws its frames; a headless run is rustyAC's own. |
| 3 | `crates/rustyac-game/src/audio.rs:392` | the pit crew's guns and the Audio app's actions are not on it, tape them when a proof drives them. | unreachable | the sound tape is rustyAC's own proof file. |
| 4 | `crates/rustyac-game/src/conditions.rs:448` | rustyAC's own drivers (--autodrive, --ai-drive) keep the quick menu unless --pit-click scripts the clicks, as  | unreachable | rustyAC-only drivers (oracle / proof inputs). |
| 5 | `crates/rustyac-game/src/input/keyboard.rs:420` | the game never stops when no point | known gap | game fault: `controls.ini [KEYBOARD]` look-ahead loops forever when no line point is ahead (or NaN); rustyAC stops after one lap. |
| 6 | `crates/rustyac-game/src/input_file.rs:530` | an input file does not carry the special event's goals, add a key when a proof replays one | unreachable | the input file is rustyAC's own. |
| 7 | `crates/rustyac-game/src/online.rs:206` | refreshed with the rest of this state (every 64 steps), not every frame | known gap | Net monitor's car1 error vector refreshed every 64 steps, not every step. |
| 8 | `crates/rustyac-game/src/online.rs:792` | another car's compound is the one it last told of (tyres.ini's | known gap | where acs.exe's remote CarAvatar compound comes from was not traced (ACClient onMessageTCP); the last told compound is very likely the same. |
| 9 | `crates/rustyac-game/src/online.rs:970` | options.ini's IGNORE_RESULT_TELEPORT is not read and there is no fade; add them if a player asks. | known gap | IGNORE_RESULT_TELEPORT now read as `ACClient::ACClient` 0x14003a2c0 does (ported); the 4 s fade before the teleport is not. |
| 10 | `crates/rustyac-game/src/render/ac.rs:43` | std's clock panics in the browser, so the load and frame times read 0 there; use the | known gap | browser: load and frame times are 0 (no clock in the page). |
| 11 | `crates/rustyac-game/src/render/ac.rs:1745` | set every frame, so a car whose LOCK_VIRTUAL_STEER clears it on board keeps it; follow updateForces calls if t | known gap | lock_animation set every frame, not on LOCK_VIRTUAL_STEER's updateForces calls. |
| 12 | `crates/rustyac-game/src/render/ac.rs:1831` | the body's matrix of the frame's view stands for physicsState.worldMatrix; a replay's car may differ | known gap | Wings app in a replay: the view's body matrix stands for physicsState.worldMatrix. |
| 13 | `crates/rustyac-game/src/render/ac.rs:1842` | the frame's view stands for physicsState (worldMatrix, tyreMatrix); a replay's car may differ | known gap | Suspensions app in a replay: the view stands for physicsState. |
| 14 | `crates/rustyac-game/src/render/ac.rs:1882` | the statistics apps get no frame numbers on wgpu (the page has no clock in | known gap | browser only. |
| 15 | `crates/rustyac-game/src/render/gui_apps.rs:621` | the wheels' drawn matrices stand for the wheel nodes' world matrices | known gap | Suspensions app: wheels' drawn matrices stand for the wheel nodes' world matrices. |
| 16 | `crates/rustyac-game/src/render/gui_apps.rs:760` | YEBIS's current auto exposure and the filter's [AUTO_EXPOSURE] limits are not handed to the HDR and camera editor apps (0) | known gap | HDR app: FormHDR::render 0x14032af90 graphs the camera's currentAutoExposure and the filter's limits; the port's link has 0 (needs YebisPP::updatePP readback). |
| 17 | `crates/rustyac-game/src/shm.rs:277` | written once as the replay starts, not every frame; the | known gap | replay status in shared memory written once. |
| 18 | `crates/rustyac-game/src/view.rs:23` | a car with more [WING_n] + [FIN_n] shows only the first 12 in the Wings app; raise if one appears | ported | FormWings::render 0x1402edff0 draws every wing; the most [WING_n]+[FIN_n] among installed cars is 23 (vrc_formula_alpha_2025): MAX_WINGS 12 -> 32. |
| 19 | `crates/rustyac-game/src/view.rs:1054` | an online remote car has no Car here, so the player's is shown; port NetCar's setup if wanted | known gap | setup debug app for an online remote car. |
| 20 | `crates/rustyac-game/src/view.rs:1216` | the speed of this step stands for the previous snapshot's, exact but at the 2 m/s edge | known gap | README gap (telemetry ride height). |
| 21 | `crates/rustyac-gui/src/app_ai.rs:202` | the 3D lines of the render-finished handler (the line, the pit lane, the sides) are not drawn, port FormAI::re | known gap | AI app's 3D lines. |
| 22 | `crates/rustyac-gui/src/app_ai.rs:301` | drivetrain.ini's AUTO_SHIFTER copy for a loose unencrypted | known gap | AI app SAVE: drivetrain.ini AUTO_SHIFTER copy for a loose unencrypted car. |
| 23 | `crates/rustyac-gui/src/app_ai.rs:403` | the recorder's side splines are empty here (the game fills them only when side_l/r.csv are newer than the line | unreachable | content: 10 installed tracks have side_l.csv, none newer than fast_lane.ai (the recorder writes the line, so it stays newer). |
| 24 | `crates/rustyac-gui/src/app_camera_editor.rs:847` | the game quits (exit(2)) after a run started from assetto_corsa.ini's AUTOEXP; rustyAC goes on | known gap | FormCamera::render 0x1402f6ff0 case 9: `if (isByButton == false) exit(2)`; reachable only once #16 gives the link the live auto exposure (with 0 the run never finishes), so it comes with #16. |
| 25 | `crates/rustyac-gui/src/app_suspension.rs:63` | the game's susGL is one static for every instance; one app is ever made | unreachable | one caller, crates/rustyac-game/src/render/gui_apps.rs (one app per name). |
| 26 | `crates/rustyac-gui/src/time_attack.rs:138` | the game leaves the text unwritten when no time is left (never drawn so) | known gap | game fault: the frame after Still->On, timeLeft is still 0 and timeToTimeAttackString 0x1401be000 formats an uninitialised buffer; rustyAC shows 0.000". |
| 27 | `crates/rustyac-launcher/src/files.rs:233` | the clock is a counter (no wall clock in wasm32-unknown-unknown); only the | known gap | browser: page-written files' dates come from a counter. |
| 28 | `crates/rustyac-launcher/src/files.rs:293` | files only (the launcher renames replays), not folders | unreachable | only caller is replays.rs move_file on files. |
| 29 | `crates/rustyac-launcher/src/online.rs:161` | one message for every other way the list does not parse, the lobby always sends a list | known gap | error text differs. |
| 30 | `crates/rustyac-launcher/src/online.rs:169` | a ServicePoint is never let go (.NET drops it after | known gap | no ServicePoint settings. |
| 31 | `crates/rustyac-launcher/src/online.rs:176` | redirects are not followed (.NET follows them), nothing redirects here. | known gap | redirects. |
| 32 | `crates/rustyac-launcher/src/online.rs:311` | dotted IPv4 only, IPAddress.Parse takes more forms, the theme sends the lobby's dotted ones | known gap | only dotted IPv4 (the lobby's); a hand-typed other form IPAddress.Parse takes is refused. |
| 33 | `crates/rustyac-launcher/src/profile.rs:9` | every file is read as ANSI bytes, where Windows reads a file as UTF-16 when it | unreachable | scan: 5090 .ini under the install + Documents\Assetto Corsa, none UTF-16 with BOM. |
| 34 | `crates/rustyac-math/src/lib.rs:198` | the DLL's logf is not rewritten (only YEBIS's auto exposure reads it, a | known gap | logf 1 ulp off; no proof shows it. |
| 35 | `crates/rustyac-math/src/lib.rs:471` | base 0.5 only and no rare path (y within +-100), the light shafts' | unreachable | only caller yebis.rs `powf4_half(k*a)` keeps the argument under ~30 < 100. |
| 36 | `crates/rustyac-net/src/laps.rs:67` | a sector the car missed reads 0 here; the game reads past its list then | known gap | game fault. |
| 37 | `crates/rustyac-physics/src/ai/car_ext.rs:166` | worked out again for each session, the same car on the same track). | unreachable | the result depends only on the car's and track's files, so working it out again gives the same values. |
| 38 | `crates/rustyac-physics/src/car/chassis.rs:1808` | the game compares with the frame's snapshot of the level on the GUI | known gap | damage level compared one step later at most. |
| 39 | `crates/rustyac-physics/src/race/shm.rs:306` | the game's list is empty unless all four tyres have as many compounds; never so in Kunos cars) | unreachable | every installed car's tyres.ini (205 cars: cardata, extracted, loose) has equal FRONT/REAR counts. |
| 40 | `crates/rustyac-physics/src/track/ai_line.rs:812` | keeps every recorded lap's lines until the track goes (a few MB a lap), drop old ones once no reader can hold  | unreachable | memory only, nothing seen. |
| 41 | `crates/rustyac-python/src/ac.rs:294` | the game returns a tuple of two empty items here (Python crashes on it) | known gap | game fault (Python crash). |
| 42 | `crates/rustyac-python/src/lib.rs:236` | a missing ACTIVE | known gap | a hand-edited python.ini without ACTIVE (every file AC writes has it). |
| 43 | `crates/rustyac-python/src/lib.rs:352` | the game reads the value as a str only: an exception object raised by Python | known gap | game fault. |
| 44 | `crates/rustyac-render/src/damage.rs:164` | an index outside 0..5 writes past the game's array; here it does nothing | known gap | game fault. |
| 45 | `crates/rustyac-render/src/heat_shimmer.rs:90` | the game reads past heatParticleSets[30] with more cars, here they share the last volume | known gap | game fault. |
| 46 | `crates/rustyac-render/src/kgl_wgpu.rs:1018` | no wgpu timestamp queries, upgrade if the page's profiler is wanted. | known gap | browser only. |
| 47 | `crates/rustyac-render/src/texture.rs:157` | no caller reads the format, so it is left empty here | unreachable | describe()'s callers drop the format. |
| 48 | `crates/rustyac-render/src/texture_fallback.rs:225` | CMYK as 255 - (c + k) per channel, not WIC's colour handling; no AC texture seen is CMYK | unreachable | 3156 loose jpegs + 21 jpeg textures in 1241 readable kn5: none with 4 components (24 CSP-format kn5 unreadable). |
| 49 | `crates/rustyac-render/src/ttf.rs:321` | no hinting (about | known gap | browser only. |
| 50 | `crates/rustyac-render/src/vr.rs:634` | only -1 is laid out (SteamVR's one invalid index); any other index past the array reads that too | known gap | game fault (index >= 16). |
| 51 | `crates/rustyac-render/src/yebis.rs:1610` | rows past the eighth pass would read before the game's table, no shipped star gets there | unreachable | shipped filters: Y7e sweep 10/10 IDENTICAL; custom filters are the README refused-key gap. |
| 52 | `crates/rustyac-render/src/yebis.rs:2243` | a flat camera's shimmer without CA isn't ported, drawn with CA; port it if a filter needs it | unreachable | as #51. |
| 53 | `crates/rustyac-render/src/yebis_cb.rs:1657` | the 3x3 program is not ported, the 7-tap one stands in until it is | unreachable | as #51. |
| 54 | `crates/rustyac-render/src/yebis_heat.rs:155` | msvcr120's qsort is not stable, only particles at the same depth could come out in another order | known gap | msvcr120's qsort is not stable: particles at the same depth may draw in another order. |
| T1 | `tools/proofs/dumps.sh:30` | files a drive names (a setup .ini) are not in the key; a changed one shows as a difference | tool limit | stated in place |
| T2 | `tools/proofs/lib.sh:85` | the port crates an oracle links are not in the key (they change every task, the game's | tool limit | stated in place |
| T3 | `tools/proofs/lib.sh:183` | gives up waiting after 30 min and starts anyway, the rerun alone catches an ERROR | tool limit | stated in place |
| T4 | `tools/proofs/run_all.sh:140` | a case whose quick key is not its name (launcher, sound's e2e) does not run | tool limit | stated in place |
| T5 | `tools/web_pack/src/main.rs:295` | the Special Events list without its pictures (144 MB), add them if the preview should show them | tool limit | stated in place |
| T6 | `tools/web_pack/src/main.rs:297` | the career's series without their pictures (617 MB with the videos), add them if the preview should show them | tool limit | stated in place |
