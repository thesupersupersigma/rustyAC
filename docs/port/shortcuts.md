# Task S2b — the small shortcuts as the game does

## Resume here
- Step 2.1 done: #38 ported (and engineDamageLevel, the same pattern).
- Step 2.2 done: #34 logf ported.
- Step 2.3 done: #54 qsort ported.
- Step 2.4 done: #17, #11, #7, #12, #13, #15, #16, #24.
- Step 2.5 done: #8.
- Step 2.6 done: #22 and #30 ported; #29, #31, #32 stay known gaps (why in their rows).
- Step 1 done: #20 ported, #42 traced (as the game); #19 is a game fault (traced); the nine
  game faults are in differences.md with `difference (differences.md)` markers; the five
  browser-only markers say `browser only`.
- Step 3 done: quick lane (one fresh case fixed in the script: the e30 crash replay ran on the
  set-damage recording; rerun IDENTICAL), clippy every way clean, README gaps in three groups, v0.52.1.

## Totals
54 markers: 15 ported, 19 unreachable, 9 game fault, 5 browser only, 6 desktop gap
(#1, #9, #21, #29, #31, #32; sizes in their rows).

## Ported in this task
- **#38 bodyDamageLevelN** (and **engineDamageLevel**, alike): `lambda_e938ee::operator()`
  0x140195fb0 compares the new level with the frame's `physicsState.damageZoneLevel[i]` on the
  GUI thread (`level <= value && value != level`) and only then posts the job, whose body
  `Car::setDamageLevel(float, int)` 0x140275b50 sets the zone without comparing;
  `lambda_d21707` 0x140195e40 does the same for the engine (`value < engineLifeLeft`, the job sets
  `Engine::lifeLeft`). The port compared inside the job; now `console.rs` compares with the
  frame's `HudCar` (all five zones) and the job sets. Proof: `car_oracle run --track spa --collide
  --scenario spa_crash --car bmw_m3_e30 --set-damage 0@30=1` calls the game's job 6.6 s after
  the crash, front zone 92.9 → 1.0 (the old port refused it); `chassis_compare run` 100 %
  (12001/12001). In the `fresh` area (`crash bmw_m3_e30`).
- **#34 logf**: `MSVCR120.dll`'s `logf`, FMA3 path 0x180094560, ported into `rustyac_math::pure`
  instruction by instruction (single precision, `vfmadd…ss` as `f32::mul_add`, three 129-entry
  tables from 0x1800c0030 / 0x1800c0240 / 0x1800c0450); the pure backend (the browser, and a
  desktop without the DLL) no longer uses Rust's `ln`. Proof: `math_proof one logf`, every 32-bit
  pattern (4,294,967,296) identical to the DLL; YEBIS's auto exposure row (`r23_09`, `--track
  spa --view chase --post`, `RENDER_ORACLE_YEBIS=1`) with `RUSTYAC_MATH=pure`: command log and
  pixels IDENTICAL.
- **#54 qsort**: `MSVCR120.dll`'s `qsort` 0x180077250 (median of three, `shortsort` 0x180077550
  below nine elements, an explicit stack with the bigger half pushed) is `rustyac_math::qsort`,
  comparison for comparison; the heat shimmer sorts its particles with it and
  `Callback_QSort_CompareParticleZ_S` 0x1403fe1e0, so particles at the same depth come out in the
  game's order. Proof: `same_as_the_dll` (20,000 random arrays full of ties against the real
  DLL's `qsort`, identical) and `ties_in_the_dlls_order` (the DLL's order of one array, on any
  target); shimmer row `ry3e_01` IDENTICAL.
- **#17 replay status in shared memory**: `SharedMemoryWriter::update` 0x140186230 runs every
  drawn frame, also in a replay (status `AC_REPLAY`, the time multiplier, the avatar's
  `bodyMatrix` position for `carCoordinates` / `distanceTraveled`). An offline replay stands
  rustyAC's physics still, so the desktop now hands each drawn frame's replayed position to the
  physics thread's pause loop, which writes the page (`ShmSink::paused_tick`); before, the page
  was written once. Proof: `rustyac.exe --track spa --autodrive --duration 18 --replay-script
  replay@6` with `re/scratch/taskS2b/shm_watch.py` reading `Localcpmf_graphics`: in the
  replay `packetId` advanced 582 times at the frame rate with status 1, and `carCoordinates`
  retraced the lap from the replay's start (555 distinct positions).
- **#11 lock_animation**: `FormDriverModel::updateForces` 0x1403218f0 (called only from the six
  spinners' change handlers) sets `driverModel_HR->lockAnimation` once; the port set it every
  frame, so a LOCK_VIRTUAL_STEER car leaving the cockpit view could never clear it. Now a
  one-shot. No oracle row moves a spinner then switches view; checked by reading.
- **#7 Net monitor**: car 1's `errorVector` is now published every physics step (the rest of
  the online state stays every 64 steps).
- **#12 / #13**: no change needed, the view the apps read is `physicsState` (in a replay
  `ac_replay::view_of` puts the replayed car's world and tyre matrices in it): unreachable.
- **#15 wheel nodes**: the Suspensions app now gets the wheel nodes' world matrices from the
  scene (`CarAvatar::wheel_node_position`) right before the GUI draws, not the drawn wheels'
  matrices. render_oracle's `app_suspension` builds its own link, so this glue is checked by
  the build only.
- **#16 auto exposure to the apps**: YEBIS keeps `currentAutoExposure` as
  `CameraForwardYebis::setPostProcessing` 0x140025080 does (the tone map's exposure, or
  `GetAutoExposureAdjusted` with auto exposure on), uses 0..10 instead of the filter's
  MIN / MAX while `useAutoExposureLimits` is off, and the middle grey from the HDR app's
  target once the app moved it. The renderer hands the current value and the filter's
  MIN / MAX / TARGET to the HDR and camera editor apps' link before the GUI updates and
  takes the limits switch and target back after it draws. The apps themselves were already
  proven against the game with these values scripted (render_oracle `gui_ac_apps`); the
  YEBIS row r23_09 stays IDENTICAL (defaults unchanged).
- **#24 AUTOEXP quit**: case 9 of the calibration now ends the process with `exit(2)`
  (`SimAction::Exit(2)`) when the run was started from assetto_corsa.ini's AUTOEXP, as
  FormCamera::render 0x1402f6ff0 does; the page stops its loop instead. Checked by the build
  only (a live run takes 9 × 3 s levels with YEBIS converging).
- **#8 another car's compound**: traced. `ACClient::onMessageTCP` 0x50 posts lambda_84f96568,
  which sets the remote CarAvatar's `externalTyreCompoundShortName` (+0x100); a remote car has
  no physics car (`Sim::addNetCar` 0x140197c30 makes a NetCarStateProvider), so
  `getCurrentCompoundShortName` 0x1400d3200 returns that string, and `onRemoteLapCompleted`
  0x14004f470 writes it into the lap. The constructor leaves it empty: the port now starts
  every remote car with "" instead of tyres.ini's default, and keeps the last told name as
  before. Checked by the build (the multiplayer goldens carry no lap compounds of remote cars).
- **#22 AUTO_SHIFTER copy**: the SAVE ai lambda 0x1402df8b0 first opens the car's
  drivetrain.ini with `CarAvatar::openINI`; when it is a loose file (not from data.acd) without
  `[AUTO_SHIFTER]`, ai.ini's `[GEARS]` (`GetPrivateProfileSectionW`, read before the new
  values go in) becomes that section. The port pushes `AppAction::AiGearsToAutoShifter` before
  the ai.ini writes and the game's handler does the same into rustyAC's copy of the file;
  `profile_section` has a unit test. The GUI sequences' cars come from data.acd, so nothing
  is copied there.
- **#20 telemetry ride height**: the Telemetry app's RIDE_HEIGHT channels read the writer's
  snapshot of the step (`PhysicsPageWriter::ride_height`, made with the previous snapshot's
  speed), which is `Car::getPhysicsState`'s rideHeight, instead of recomputing with this step's
  speed. Build only.
- **#42 python.ini without ACTIVE**: traced, no change: `INIReader::getInt` 0x140235c70 reads a
  missing key's empty string as 0.
- **#30 ServicePoint idle**: the launcher's HTTP/1.1 hosts are forgotten after 100 s without a
  request (`MaxServicePointIdleTime`), so the next request sends `Connection: Keep-Alive`
  again. Build only (no proof waits 100 s between lobby requests).

# Task S2 — the shortcut sweep

## Resume here
- Step 0 done (2e95c3a): README known gaps and `leftovers.md` rechecked, eight stale entries removed.
- Step 1 done: every `shortcut:` marker is in the table below with its verdict.
- Step 2: #18 ported, #9's options.ini key ported (its fade stays a known gap); every
  other marker in `crates/` now reads `shortcut (<verdict>, shortcuts.md #N): ...`.
- Step 3 done: clippy six ways clean, quick lane 174/174 IDENTICAL (0 ERROR), released v0.52.0.

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
| 1 | `crates/rustyac-game/src/achievements.rs:780` | no avatar texture and no Friends Leaderboard rows on screen (the calls are | known gap | README gap (Friends Leaderboard). Big: Steam's friends list, avatar images as textures and the leaderboard rows' drawing (about 300 lines and a Steam-backed proof); after v1.0. |
| 2 | `crates/rustyac-game/src/achievements.rs:838` | runs only where frames are drawn (the window, or headless with --drawn-frames), | unreachable | acs.exe always draws its frames; a headless run is rustyAC's own. |
| 3 | `crates/rustyac-game/src/audio.rs:392` | the pit crew's guns and the Audio app's actions are not on it, tape them when a proof drives them. | unreachable | the sound tape is rustyAC's own proof file. |
| 4 | `crates/rustyac-game/src/conditions.rs:448` | rustyAC's own drivers (--autodrive, --ai-drive) keep the quick menu unless --pit-click scripts the clicks, as  | unreachable | rustyAC-only drivers (oracle / proof inputs). |
| 5 | `crates/rustyac-game/src/input/keyboard.rs:420` | the game never stops when no point | game fault | game fault: `controls.ini [KEYBOARD]` look-ahead loops forever when no line point is ahead (or NaN); rustyAC stops after one lap. |
| 6 | `crates/rustyac-game/src/input_file.rs:530` | an input file does not carry the special event's goals, add a key when a proof replays one | unreachable | the input file is rustyAC's own. |
| 7 | `crates/rustyac-game/src/online.rs:206` | refreshed with the rest of this state (every 64 steps), not every frame | ported (S2b) | the error vector is published every physics step. |
| 8 | `crates/rustyac-game/src/online.rs:792` | another car's compound is the one it last told of (tyres.ini's | ported (S2b) | where acs.exe's remote CarAvatar compound comes from was not traced (ACClient onMessageTCP); the last told compound is very likely the same. |
| 9 | `crates/rustyac-game/src/online.rs:970` | options.ini's IGNORE_RESULT_TELEPORT is not read and there is no fade; add them if a player asks. | known gap | IGNORE_RESULT_TELEPORT now read as `ACClient::ACClient` 0x14003a2c0 does (ported); the 4 s fade before the teleport is not. Not small: the online sim has no camera-fade channel to the picture (the referee's is off line only): a 4 s countdown in the sim, a new field through the physics thread to the picture's fade, and a live acServer race end to prove it (about 40 lines in 5 files plus the live run). |
| 10 | `crates/rustyac-game/src/render/ac.rs:43` | std's clock panics in the browser, so the load and frame times read 0 there; use the | browser only | browser: load and frame times are 0 (no clock in the page). |
| 11 | `crates/rustyac-game/src/render/ac.rs:1745` | set every frame, so a car whose LOCK_VIRTUAL_STEER clears it on board keeps it; follow updateForces calls if t | ported (S2b) | set once per spinner change, as `updateForces` does. |
| 12 | `crates/rustyac-game/src/render/ac.rs:1831` | the body's matrix of the frame's view stands for physicsState.worldMatrix; a replay's car may differ | unreachable (S2b) | the view's body is physicsState.worldMatrix, the replayed one in a replay (`ac_replay::view_of`). |
| 13 | `crates/rustyac-game/src/render/ac.rs:1842` | the frame's view stands for physicsState (worldMatrix, tyreMatrix); a replay's car may differ | unreachable (S2b) | the view's body and tyre matrices are physicsState's, the replayed ones in a replay. |
| 14 | `crates/rustyac-game/src/render/ac.rs:1882` | the statistics apps get no frame numbers on wgpu (the page has no clock in | browser only | browser only. |
| 15 | `crates/rustyac-game/src/render/gui_apps.rs:621` | the wheels' drawn matrices stand for the wheel nodes' world matrices | ported (S2b) | the wheel nodes' world matrices, read from the scene before the GUI draws. |
| 16 | `crates/rustyac-game/src/render/gui_apps.rs:760` | YEBIS's current auto exposure and the filter's [AUTO_EXPOSURE] limits are not handed to the HDR and camera editor apps (0) | ported (S2b) | HDR app: FormHDR::render 0x14032af90 graphs the camera's currentAutoExposure and the filter's limits; the port's link has 0 (needs YebisPP::updatePP readback). |
| 17 | `crates/rustyac-game/src/shm.rs:277` | written once as the replay starts, not every frame; the | ported (S2b) | the page is written every drawn frame of a replay, from the replayed car. |
| 18 | `crates/rustyac-game/src/view.rs:23` | a car with more [WING_n] + [FIN_n] shows only the first 12 in the Wings app; raise if one appears | ported | FormWings::render 0x1402edff0 draws every wing; the most [WING_n]+[FIN_n] among installed cars is 23 (vrc_formula_alpha_2025): MAX_WINGS 12 -> 32. |
| 19 | `crates/rustyac-game/src/view.rs:1054` | an online remote car has no Car here, so the player's is shown; port NetCar's setup if wanted | game fault | the game crashes: a remote CarAvatar has no RaceEngineer (only `initPhysics` 0x1400d7660 makes one; `Sim::addNetCar` uses the constructor without it) and SetupDebug::render 0x140318010 reads `[raceEngineer+8]`. |
| 20 | `crates/rustyac-game/src/view.rs:1216` | the speed of this step stands for the previous snapshot's, exact but at the 2 m/s edge | ported (S2b) | `telemetry_values` now reads the step's snapshot (`PhysicsPageWriter::ride_height`, made with the previous snapshot's speed), as getPhysicsState's rideHeight. |
| 21 | `crates/rustyac-gui/src/app_ai.rs:202` | the 3D lines of the render-finished handler (the line, the pit lane, the sides) are not drawn, port FormAI::re | known gap | AI app's 3D lines. Not small: FormAI::renderSplines 0x1402e3920 draws five splines through GLRenderer::spline 0x1401ff940 (not ported) from the AI line, its payload sides, the pit lane and the recorder's side lines, plus the steer target; about 250 lines and a 3D GUI oracle sequence. |
| 22 | `crates/rustyac-gui/src/app_ai.rs:301` | drivetrain.ini's AUTO_SHIFTER copy for a loose unencrypted | ported (S2b) | AI app SAVE: drivetrain.ini AUTO_SHIFTER copy for a loose unencrypted car. |
| 23 | `crates/rustyac-gui/src/app_ai.rs:403` | the recorder's side splines are empty here (the game fills them only when side_l/r.csv are newer than the line | unreachable | content: 10 installed tracks have side_l.csv, none newer than fast_lane.ai (the recorder writes the line, so it stays newer). |
| 24 | `crates/rustyac-gui/src/app_camera_editor.rs:847` | the game quits (exit(2)) after a run started from assetto_corsa.ini's AUTOEXP; rustyAC goes on | ported (S2b) | FormCamera::render 0x1402f6ff0 case 9: `if (isByButton == false) exit(2)`; reachable only once #16 gives the link the live auto exposure (with 0 the run never finishes), so it comes with #16. |
| 25 | `crates/rustyac-gui/src/app_suspension.rs:63` | the game's susGL is one static for every instance; one app is ever made | unreachable | one caller, crates/rustyac-game/src/render/gui_apps.rs (one app per name). |
| 26 | `crates/rustyac-gui/src/time_attack.rs:138` | the game leaves the text unwritten when no time is left (never drawn so) | game fault | game fault: the frame after Still->On, timeLeft is still 0 and timeToTimeAttackString 0x1401be000 formats an uninitialised buffer; rustyAC shows 0.000". |
| 27 | `crates/rustyac-launcher/src/files.rs:233` | the clock is a counter (no wall clock in wasm32-unknown-unknown); only the | browser only | browser: page-written files' dates come from a counter. |
| 28 | `crates/rustyac-launcher/src/files.rs:293` | files only (the launcher renames replays), not folders | unreachable | only caller is replays.rs move_file on files. |
| 29 | `crates/rustyac-launcher/src/online.rs:161` | one message for every other way the list does not parse, the lobby always sends a list | known gap | error text differs. Not small: JavaScriptSerializer's messages name the bad token and its position, which needs its parser's error paths; the lobby always sends a list. |
| 30 | `crates/rustyac-launcher/src/online.rs:169` | a ServicePoint is never let go (.NET drops it after | ported (S2b) | no ServicePoint settings. |
| 31 | `crates/rustyac-launcher/src/online.rs:176` | redirects are not followed (.NET follows them), nothing redirects here. | known gap | redirects. Not small: AllowAutoRedirect's rules (which codes, Location forms, the 50-hop limit, https targets this HTTP-only client cannot reach); the lobby and acServer never redirect. |
| 32 | `crates/rustyac-launcher/src/online.rs:311` | dotted IPv4 only, IPAddress.Parse takes more forms, the theme sends the lobby's dotted ones | known gap | only dotted IPv4 (the lobby's); a hand-typed other form IPAddress.Parse takes is refused. Not small: IPv6 needs a second UDP path and the short / octal / hex IPv4 forms need .NET's parser to refuse the same strings; the theme only sends the lobby's dotted addresses. |
| 33 | `crates/rustyac-launcher/src/profile.rs:9` | every file is read as ANSI bytes, where Windows reads a file as UTF-16 when it | unreachable | scan: 5090 .ini under the install + Documents\Assetto Corsa, none UTF-16 with BOM. |
| 34 | `crates/rustyac-math/src/lib.rs:198` | the DLL's logf is not rewritten (only YEBIS's auto exposure reads it, a | ported (S2b) | `pure::logf`, all 2^32 inputs identical to the DLL. |
| 35 | `crates/rustyac-math/src/lib.rs:471` | base 0.5 only and no rare path (y within +-100), the light shafts' | unreachable | only caller yebis.rs `powf4_half(k*a)` keeps the argument under ~30 < 100. |
| 36 | `crates/rustyac-net/src/laps.rs:67` | a sector the car missed reads 0 here; the game reads past its list then | game fault | game fault. |
| 37 | `crates/rustyac-physics/src/ai/car_ext.rs:166` | worked out again for each session, the same car on the same track). | unreachable | the result depends only on the car's and track's files, so working it out again gives the same values. |
| 38 | `crates/rustyac-physics/src/car/chassis.rs:1808` | the game compares with the frame's snapshot of the level on the GUI | ported (S2b) | compared on the GUI thread with the frame's level, the job sets. |
| 39 | `crates/rustyac-physics/src/race/shm.rs:306` | the game's list is empty unless all four tyres have as many compounds; never so in Kunos cars) | unreachable | every installed car's tyres.ini (205 cars: cardata, extracted, loose) has equal FRONT/REAR counts. |
| 40 | `crates/rustyac-physics/src/track/ai_line.rs:812` | keeps every recorded lap's lines until the track goes (a few MB a lap), drop old ones once no reader can hold  | unreachable | memory only, nothing seen. |
| 41 | `crates/rustyac-python/src/ac.rs:294` | the game returns a tuple of two empty items here (Python crashes on it) | game fault | game fault (Python crash). |
| 42 | `crates/rustyac-python/src/lib.rs:236` | a missing ACTIVE | ported (S2b) | traced: `INIReader::getInt` 0x140235c70 of a missing key reads an empty string as 0, as the port does. |
| 43 | `crates/rustyac-python/src/lib.rs:352` | the game reads the value as a str only: an exception object raised by Python | game fault | game fault. |
| 44 | `crates/rustyac-render/src/damage.rs:164` | an index outside 0..5 writes past the game's array; here it does nothing | game fault | game fault. |
| 45 | `crates/rustyac-render/src/heat_shimmer.rs:90` | the game reads past heatParticleSets[30] with more cars, here they share the last volume | game fault | game fault. |
| 46 | `crates/rustyac-render/src/kgl_wgpu.rs:1018` | no wgpu timestamp queries, upgrade if the page's profiler is wanted. | browser only | browser only. |
| 47 | `crates/rustyac-render/src/texture.rs:157` | no caller reads the format, so it is left empty here | unreachable | describe()'s callers drop the format. |
| 48 | `crates/rustyac-render/src/texture_fallback.rs:225` | CMYK as 255 - (c + k) per channel, not WIC's colour handling; no AC texture seen is CMYK | unreachable | 3156 loose jpegs + 21 jpeg textures in 1241 readable kn5: none with 4 components (24 CSP-format kn5 unreadable). |
| 49 | `crates/rustyac-render/src/ttf.rs:321` | no hinting (about | browser only | browser only. |
| 50 | `crates/rustyac-render/src/vr.rs:634` | only -1 is laid out (SteamVR's one invalid index); any other index past the array reads that too | game fault | game fault (index >= 16). |
| 51 | `crates/rustyac-render/src/yebis.rs:1610` | rows past the eighth pass would read before the game's table, no shipped star gets there | unreachable | shipped filters: Y7e sweep 10/10 IDENTICAL; custom filters are the README refused-key gap. |
| 52 | `crates/rustyac-render/src/yebis.rs:2243` | a flat camera's shimmer without CA isn't ported, drawn with CA; port it if a filter needs it | unreachable | as #51. |
| 53 | `crates/rustyac-render/src/yebis_cb.rs:1657` | the 3x3 program is not ported, the 7-tap one stands in until it is | unreachable | as #51. |
| 54 | `crates/rustyac-render/src/yebis_heat.rs:155` | msvcr120's qsort is not stable, only particles at the same depth could come out in another order | ported (S2b) | `rustyac_math::qsort`, the DLL's algorithm; tested against the DLL with ties. |
| T1 | `tools/proofs/dumps.sh:30` | files a drive names (a setup .ini) are not in the key; a changed one shows as a difference | tool limit | stated in place |
| T2 | `tools/proofs/lib.sh:85` | the port crates an oracle links are not in the key (they change every task, the game's | tool limit | stated in place |
| T3 | `tools/proofs/lib.sh:183` | gives up waiting after 30 min and starts anyway, the rerun alone catches an ERROR | tool limit | stated in place |
| T4 | `tools/proofs/run_all.sh:140` | a case whose quick key is not its name (launcher, sound's e2e) does not run | tool limit | stated in place |
| T5 | `tools/web_pack/src/main.rs:295` | the Special Events list without its pictures (144 MB), add them if the preview should show them | tool limit | stated in place |
| T6 | `tools/web_pack/src/main.rs:297` | the career's series without their pictures (617 MB with the videos), add them if the preview should show them | tool limit | stated in place |
