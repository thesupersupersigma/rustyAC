# Misc: sessions, game modes, pits, penalties, Steam, outputs, car bookkeeping, utilities

Source: `acs.exe` + `acs.pdb`, read through the local index only (`re/decomp/` pseudo-C, `re/types/` layouts,
`tools/re_query.py` find / callers / callees / strings / uses / vtable / size). Shallow pass: class lists, constructors,
update / render hooks and file names; no maths was read. Scratch scripts: `re/scratch/atlas_misc/`. Nothing in the game
folder, the Ghidra project or git was modified. **Confirmed** = seen in callee lists, string literals or struct layouts;
**interpretation** = my reading of what it is for.

## 0. What is in this file and how the leftovers were found

`python tools/re_query.py coverage docs/atlas docs/map` listed only 6 object files that no document *mentions*
(SessionResultProcessor, CarRaceInfo, LapInvalidatorAvatar, DriveThroughPenaltyAvatar, ksUtils, DriverNameProvider).
"Mentioned" is weaker than "counted", so a second check was made (`re/scratch/atlas_misc/uncounted.py`): every object
file that appears in no `## 5. Size` list of the 13 other atlas files. That gives 90 object files. 50 of them are the
subject of a car-physics map in `docs/map/` (listed at the end, not counted here). The other **40 object files,
725 hand-written functions, 411,356 code bytes** are this file. The whole executable then adds up exactly:
atlas 362 objects / 6,057 functions + this file 40 / 725 + docs/map 50 / 671 = 452 objects / 7,453 functions.

| Sub-area | Objects | Functions | Code bytes | Port |
|---|---|---|---|---|
| A. Race logic and sessions | 5 | 184 | 128,998 | L |
| B. Game modes and special events | 7 | 140 | 83,024 | M |
| C. Pit stops and crew | 3 | 114 | 54,095 | M |
| D. Penalties and rules | 5 | 43 | 15,042 | S |
| E. Driving assists and push-to-pass | 2 | 27 | 6,527 | S |
| F. Steam integration | 1 | 66 | 38,068 | S |
| G. File and memory outputs | 3 | 44 | 25,504 | S |
| H. Car bookkeeping components (physics side) | 8 | 73 | 50,260 | M |
| I. Shared utilities | 6 | 34 | 9,838 | S |

All game-side classes below are `GameObject`s created in `Sim::Sim` 0x140192070 and driven by the per-frame tree walk
described in `main_loop.md` (vtable +0x08 `update`, +0x18 `renderHUD`, +0x28 `shutdown`), on the main thread.
The physics-side classes are members of `Car` and run on the physics thread at 333 Hz.

---

## A. Race logic and sessions

### A.1 What it is
`RaceManager` is the referee and the session clock. At load time it reads `race.ini`, creates the track and every car,
and builds the list of sessions (practice, qualifying, race, or one of the single-player modes). During play it decides
when a session starts, when it is over, who is in which position, who has retired, and whether the mandatory pit stop
was done. It also draws the session banner text ("FINAL LAP", "QUALIFY OVER", "PIT OPEN"). Lap times do not come from
here: the physics thread times the laps (`TimeTransponder`, sub-area H) and raises `Car::evOnLapCompleted`; the manager
listens and stores each lap in `RaceTimingServices`, which keeps one lap database per car and the sorted leaderboard.
When a session ends the manager builds a result record and raises `evOnSessionEnd`, which the end-of-session screens,
the JSON writer, Steam and the replay autosave all listen to. If the player skips a session, `SessionResultProcessor`
invents plausible results for the AI cars. Online, the server owns the sessions and the manager only mirrors what
`ACClient` reports.

### A.2 Main classes
| Class | Object / lib | Size | Role |
|---|---|---|---|
| `RaceManager` | RaceManager.obj | 0x218 | Session list, current session, start time, race-over logic, real-time positions, mandatory pit window, HUD text. Events `evOnRaceInit`, `evOnSessionEnd`, `evOnLapCompleted`. |
| `Session` | (struct) | 0x70 | `sessionType`, `isTimedRace`, `hasAdditionalLap`, `laps`, `durationMinutes`, `overtime_ms`, `spawSet`, `name`, `startTime`, `forcedPosition`, `isOver`, `leaderCompletedLaps`. |
| `RaceTimingServices` | RaceTimingServices.obj | 0xc0 | One `LapDB` (0xb0) per car: laps, best lap, splits; `LeaderboardEntry` (0x20) sorting. |
| `SessionResultProcessor` | SessionResultProcessor.obj | 0x8 | Stateless helpers that complete race / qualify / drag results for AI cars. |
| `CarRaceInfo` | CarRaceInfo.obj | 0x28 | Small per-car record owned by `CarAvatar`: spawn (grid / pit) position index, session-end handler. |
| `DriverNameProvider` | DriverNameProvider.obj | 0x1 | Only a static table `lastNames` (its initialiser and destructor are the 2 functions). |
| `OnSessionEndEvent` | (struct) | 0xe8 | `Session` + `SessionResult` (0x78), payload of `evOnSessionEnd`. |

`SessionType` enum names seen in the pseudo-C (confirmed): `Undefined`, `Pratice` (sic), `Qualify`, `Race`, `Hotlap`,
`TimeAttack`, `Drift`, `Drag`. The integer `[SESSION_n] TYPE` of race.ini is stored straight into it; the numeric values
1..7 in that order are the launcher convention (interpretation, not read from the enum record).

### A.3 Entry points
- Creation: `Sim::Sim` -> `RaceManager::RaceManager` 0x140132db0, then `RaceManager::loadSessions` 0x14013ecb0, which
  picks `initReplay` 0x14013e820, `initOnline` 0x14013d160 or `initOffline` 0x14013a6c0 (10.9 KB: prints and parses
  race.ini, loads track and cars, ballast / restrictor / skins, wind, penalties, creates `RaceTimingServices`
  0x140143c60). Helpers: `initLighting` 0x14013a3d0, `initGhostcar` 0x14013a210, `convertRemoteSession` 0x140136a50.
- Session control: `setCurrentSession` 0x140140df0 (resets laps and penalties, locks controls until the start),
  `startRace` 0x140141470 (start time = now + delay + a random +-1 s, posted to the physics thread with
  `PhysicsAvatar::executeOnPhysicsThread`), `finalizeCurrentSession` 0x140136b30 (builds results, calls
  `SessionResultProcessor::getCompleteAIResults` 0x140166c70), `skipCurrentSession` 0x140141350,
  `restartCurrentSession` 0x140140d00, `isRaceOver` 0x14013eae0.
- Per frame: `RaceManager::update` 0x140141550 (vtable +0x08: session clock, timed-race end, retire / black flag,
  `updateCarsRealTimePositions` 0x1401428b0, `updateCarsEndRace` 0x140142650, `resetInvalidStateCarsToPits`
  0x140140340), `RaceManager::renderHUD` 0x14013f960 (`getRaceText` 0x140139150, `getPracticeQualifyText`
  0x140138d80), `RaceManager::shutdown` 0x140141320.
- Callbacks: `RaceManager::onLapCompleted` 0x14013f380 -> `RaceTimingServices::onLapCompleted` 0x140145300 ->
  `updateLeaderboard` 0x140146660; `RaceTimingServices::onSectorSplit` 0x140145940; `checkMandatoryPit` 0x140136830
  (called from `CarAvatar::updateInPitlaneState` 0x1400dd6d0).
- Results: `processRaceResults` 0x140167f30, `processQualifyResults` 0x140167110, `processDragResults` 0x140166d70,
  `fetchBasicResults` 0x140166230. `CarRaceInfo::init` 0x1400e8070 is called from `CarAvatar::initCommonPostPhysics`.

### A.4 What it reads from disk
`Documents/Assetto Corsa/cfg/race.ini` (all sections, see the table in `config.md` 4.2; extra keys seen here:
`[SESSION_n] ADDITITIONAL_LAP` (sic), `STARTING_POSITION`, `[RACE] PENALTIES`, `JUMP_START_PENALTY`, `FIXED_SETUP`).
`system/cfg/assetto_corsa.ini [RACE] OVER_TIME_MULT` (constructor). `content/cars/<car>/skins/*.*` (skin check).
`system/data/se/SPECIAL_EVENT_<guid>.acd` (encrypted special event, replaces race.ini). Writes
`Documents/Assetto Corsa/out/laps.ini` in `RaceTimingServices::shutdown` 0x140145f50. Spawn sets are the names `START`
and `PIT` (track nodes, see `track.md`).

### A.5 Size
`re_query.py size RaceManager RaceTimingServices SessionResultProcessor CarRaceInfo DriverNameProvider`:
**184 functions, 128,998 bytes** (RaceManager 108 / 80,235; RaceTimingServices 40 / 16,713; SessionResultProcessor
17 / 25,999; CarRaceInfo 17 / 5,803; DriverNameProvider 2 / 248). `RaceControlSPSession` and the end-session screens
are counted in `ui.md`.

### A.6 Port difficulty: **L**
The rules themselves are plain bookkeeping, but `RaceManager` touches almost everything: `Sim`, `CarAvatar`,
`ACClient`, the physics command queue, the HUD, cameras. A Rust port should split it into a pure session state machine
(testable without graphics) and thin adapters. Needs: lap events from `TimeTransponder`, the ini reader, the car list.
No third-party SDK. Nothing off the shelf replaces it.

### A.7 Open questions
- Numeric values of `SessionType` and the exact race-over rules for timed races with an additional lap were not read.
- How the grid order of a race is taken from the previous qualifying session (`forcedPosition`, `STARTING_POSITION`).
- Who reads `DriverNameProvider::lastNames` (no code reference found by the index besides its own initialiser).
- The algorithm `SessionResultProcessor` uses to invent AI lap times (`getAverageLap` 0x140166730) was not studied.

---

## B. Game modes and special events

### B.1 What it is
The single-player modes other than a plain weekend are each one class derived from `GameMode`. `GameMode` itself holds
the list of "conditions" (objectives with a medal tier) of a special event, read from race.ini or from an encrypted
event file, and raises `evOnTierCompleted` when a tier is reached. Exactly one mode object is created per session,
chosen in `Sim::Sim`. `HotlapMode` only checks lap times against the objectives. `TimeAttack` puts checkpoints on the
track, gives a time budget and points, and applies penalties. `DragMode` runs best-of-N drag matches with a start tree
and reaction times. `DriftMode` turns the drift numbers computed on the physics side into points, combos and levels.
`RaceEvent` is the mode used for a race with objectives (finishing position). `TimeLimitedTest` is unrelated to
`GameMode`: it is a kiosk timer that ends the run after a configured time and writes the result file.

### B.2 Main classes
| Class | Object | Size | Role |
|---|---|---|---|
| `GameMode` | GameMode.obj | 0xc8 | Base: `guid`, `conditions` (`ConditionDef` 0x48: `type`, `name`, `objective`, `achieved`), `currentTier`; pure virtual `verifyConditions` at vtable +0x30. |
| `HotlapMode` | HotlapMode.obj | 0xd8 | Lap-completed lambda 0x1400beb80 compares the lap with the objectives. |
| `TimeAttack` | TimeAttack.obj | 0x2e0 | Checkpoints, time budget, points, penalties, its own HUD. |
| `DragMode` | DragMode.obj | 0x2f0 | Matches, finish-line detection, times, false start, its own HUD. |
| `DriftMode` | DriftMode.obj | 0x1248 | Points, combos, levels, bonus animations. |
| `RaceEvent` | RaceEvent.obj | 0xe0 | Race objectives (leaderboard position at race end). |
| `TimeLimitedTest` | TimeLimitedTest.obj | 0x80 | Maximum running time, then gentle stop and JSON output. |

### B.3 Entry points
- `GameMode::GameMode` 0x1400bc810 (called by the five mode constructors), `GameMode::setTier` 0x1400be990 (called by
  every `verifyConditions`), `GameMode::forceConditionAchieved` 0x1400be860 (called by `SteamInterface::addAchievements`).
- `HotlapMode::HotlapMode` 0x1400beac0. `RaceEvent::RaceEvent` 0x1401303c0, `RaceEvent::update` 0x1401306c0,
  `RaceEvent::verifyConditions` 0x140130700.
- `TimeAttack::TimeAttack` 0x1401b7e20, `TimeAttack::update` 0x1401be0b0 -> `updateTimeAttack` 0x1401be570 and
  `updatePenalties` 0x1401be2c0, `renderHUD` 0x1401bda40, `verifyConditions` 0x1401beb10, `loadOverlaysAndSectors`
  0x1401bbc20, `createAndSaveOverlays` 0x1401b9b00, `evaluateTimeFromTrackSpline` 0x1401bab80.
- `DragMode::DragMode` 0x140084a20, `DragMode::update` 0x140088c30 -> `updateDragRace` 0x140088f90,
  `onFinishLanePassed` 0x1400870e0, `computeCarTimes` 0x140086650, `newMatch` 0x140086fc0, `renderHUD` 0x140088730,
  `verifyConditions` 0x140089460.
- `DriftMode::DriftMode` 0x140089610, `modeDrifting` 0x14008a1f0 (5.4 KB, the scoring), `updateLevelUp` 0x14008ba10,
  `renderHUD` 0x14008b7c0, `verifyConditions` 0x14008bb60. Input: `DriftModeComponent` (sub-area H).
- `TimeLimitedTest::TimeLimitedTest` 0x1401becd0, `TimeLimitedTest::update` 0x1401bf1c0 (calls `Sim::writeOutputJson`
  0x14019f950 and `CarAvatar::setGentleStop`).

### B.4 What it reads from disk
`cfg/race.ini`: `[SPECIAL_EVENT] GUID`, `[CONDITION_n] TYPE, OBJECTIVE`, `[RACE] AI_LEVEL`, `[SESSION_0] MATCHES`
(drag). `system/data/se/SPECIAL_EVENT_<guid>.acd` (official events, decrypted in memory). Time attack: track
`data/overlays.ini` (`[MAIN] OVERLAYS_COUNT`, `[CHECKPOINT_n] WORLD_POSITION, OFFSET, WIDTH, HEIGHT, ORIENTATION`; the
game writes this file itself when it is missing), track `data/ideal_line.ai`, kn5 nodes `AC_TIME_n_L/R`,
`cfg/gameplay.ini [TIME_ATTACK] BASE_TIME_MULT`, the car's `data/engine.ini [TURBO_n]` (for the time estimate).
Drag: kn5 nodes `AC_OPEN_FINISH_L/R`. Textures `content/gui/timeAttack/*.png`, `content/gui/drag/*.png`,
`content/gui/drift/*.png`, `content/texture/checkpoint.png`. `cfg/time_limited_test.ini [SETTINGS] MAX_RUNNING_TIME`
and the key `JSON_ON_SESSION_END` (string seen, its section not checked).

### B.5 Size
`re_query.py size GameMode HotlapMode TimeAttack DragMode DriftMode TimeLimitedTest RaceEvent`:
**140 functions, 83,024 bytes** (TimeAttack 39 / 31,368; DragMode 37 / 23,467; GameMode 10 / 11,574; DriftMode
18 / 11,527; TimeLimitedTest 13 / 1,922; RaceEvent 12 / 1,762; HotlapMode 11 / 1,404). `AIDriverDrag` is counted in
`ai_drivers.md`, `DriftModeComponent` in sub-area H.

### B.6 Port difficulty: **M**
Each mode is self-contained scoring plus a hand-drawn HUD. Hot lap and race event are trivial; time attack is the
largest because it derives checkpoint times from the AI line and the car's power. Depends on session logic (A), lap
events, the HUD drawing layer and, for drift, the physics component. Not needed for a first drivable build.

### B.7 Open questions
- Which code calls `verifyConditions` (virtual, +0x30) and when: not in the call index.
- The list of condition `TYPE` strings and how the four medal names (`BRONZE` .. `GOLDEN GLORY`) map to tiers.
- Drift scoring formula and time-attack time budget formula were not read.

---

## C. Pit stops and crew

### C.1 What it is
`PitStop` runs the player's pit stop. It watches whether the car is in the pit lane, stopped in its own box, and then
walks a small state machine: `Idle` -> `WaitingForDriver` -> `Choosing` (the menu with tyres, fuel, repairs) ->
`Operating` (a timer counts down) -> `Finished`. The time is the sum of a fuel time, a tyre time and a repair time
whose per-unit costs come from the car's own data. When the timer ends it applies the choices to the physics car
through setup items (tyre compound, pressures, wings, fuel, repairs). It also shows a floating marker over the pit box.
`PitCrew` is the visual side for the player: animated mechanics that change the four wheels, with wheel-gun sounds.
`LollipopCrew` is the animated lollipop man placed at pit boxes by the track.

### C.2 Main classes
| Class | Object | Size | Role |
|---|---|---|---|
| `PitStop` | PitStop.obj | 0x240 | State (`PitStopState`), `pitTimer`, `fuelTime` / `repairTime` / `tyreTime`, `strategies` (`PitStopStrategy` 0xc: fuel, compound, fix body / engine / suspension), GUI buttons, events `evPitStopCommit`, `evPitStopOver`. |
| `PitStopFuel` | PitStop.obj | 0x180 | The fuel gauge control of the pit menu. |
| `PitCrew` | PitCrew.obj | 0xa8 | Crew members and tyre models per wheel, animation timing, sounds. |
| `LollipopCrew` | LollipopCrew.obj | 0xd8 | One animated figure per pit box (left and right hand variants). |

### C.3 Entry points
- `Sim::Sim` -> `PitStop::PitStop` 0x1400a55d0 (7.8 KB: builds the menu) -> `PitCrew::PitCrew` 0x1400a0b80.
- Per frame: `PitStop::update` 0x1400ab190 (state machine; `isInPitPosition` 0x1400a98c0, `isPitStopDone`
  0x1400a9b80), `PitStop::renderHUD` 0x1400aa300. Menu: `commitQuickMenuChanges` 0x1400a94b0, `getEstimatedTime`
  0x1400a9600. The lambda 0x1400a7da0 applies the choices through `SetupManager::getSetupItem` 0x1402891b0.
- Crew: `PitCrew::onPitStopBegin` 0x1400a33e0 and `onPitStopFinished` 0x1400a3b50 (handlers of the two `PitStop`
  events), `pitCrewUpdate` 0x1400a3f20, `updateCurrentPosition` 0x1400a4500, `loadCrewSkin` 0x1400a30c0.
- `TrackAvatar::initPitCrew` 0x1401ca630 -> `LollipopCrew::LollipopCrew` 0x140112bf0; `LollipopCrew::update`
  0x140113ab0, `loadCrewSkin` 0x1401136d0.
- Physics side (counted in docs/map): `Car::initPitstopTimings` 0x140274020; AI stops in `AIDriver`.

### C.4 What it reads from disk
Car `data/car.ini [PIT_STOP] TYRE_CHANGE_TIME_SEC, FUEL_LITER_TIME_SEC, BODY_REPAIR_TIME_SEC, ENGINE_REPAIR_TIME_SEC,
SUSP_REPAIR_TIME_SEC` (read by `Car`). `cfg/gameplay.ini [PIT_INDICATOR] ONLY_IN_PITLINE` (section name checked in
the user's file). `content/objects3D/
pit_indicator.kn5`. `content/gui/pitstop/*` (button and status images), car `skins/<skin>/` tyre icons.
Crew: `content/objects3D/pitcrewtyre.ini` (`TIME_TO_FINISH`, `TIME_TO_CHANGE_TYRE`, `TIME_TO_POSITION`, screw /
unscrew frame numbers, `LOOP_SPEED_MULT`), `pitcrewtyre.kn5` / `.ksanim`, `pitcrew.kn5`, `pitcrew_Left.kn5` and their
`.ksanim` idle animations, the skin's `skin.ini [CREW] SUIT, HELMET, BRAND`, `content/texture/crew_*` (with `_nm`
normal maps), FMOD events `event:/common/screw` and `unscrew`. `system/cfg/pitstop.ini` is read by `QuickMenuPitstop`
(ui.md) and by `Sim::Sim` (`[SETTINGS] USE_MOUSE_PITSTOP`).

### C.5 Size
`re_query.py size PitStop PitCrew LollipopCrew`: **114 functions, 54,095 bytes** (PitStop 57 / 28,371; PitCrew
41 / 20,744; LollipopCrew 16 / 4,980). `QuickMenuPitstop`, `PitstopOverlay`, `SetupPitstopPresets` are in `ui.md`.

### C.6 Port difficulty: **M**
The state machine and the timing are small (S). The cost is the menu (ksgui widgets) and the animated crew (kn5 +
ksanim skinned models, see renderer.md). Depends on: `CarAvatar` pit-lane state, `SetupManager` items, GUI, audio.
The crew visuals can be left out without changing behaviour.

### C.7 Open questions
- Exact formula of the total pit time (`pitRandomModifier`, `maxPitTimer`) and how repairs scale with damage.
- Whether the tyre change resets wear / temperature through setup items or through a direct call into `Tyre`.
- Online: which parts are replaced by server messages (`ACClient`) was not traced.

---

## D. Penalties and rules

### D.1 What it is
The rules are enforced on the physics thread and only shown on the main thread. On the physics side `LapInvalidator`
watches the player car for track cuts and either marks the lap as not counting or gives a "slow down" penalty, and
`PenaltyManager` holds jump-start and drive-through penalties and turns an unserved one into a black flag. Three small
HUD objects display the result: the cut / slow-down warning, the drive-through notice with laps remaining, and the
"controls are locked for N seconds" countdown. Flags in general are drawn by `FlagManager` (counted in `ui.md`).
All of this runs for the player car only (car 0) in the stock game.

### D.2 Main classes
| Class | Object / lib | Size | Role |
|---|---|---|---|
| `PenaltyManager` | ksPhysicsAC.lib | 0x48 | `pendingPenaltyType` (`eNothing`, `eDriveThrough`), `pitPenaltyLaps`, `penaltyRecords`, event `evOnPenalty`. |
| `LapInvalidator` | ksPhysicsAC.lib | 0x20 | Cut detection for the current lap, penalty-zone handling. |
| `LapInvalidatorAvatar` | LapInvalidatorAvatar.obj | 0xb8 | HUD: "CUT DETECTED", "LAP WILL NOT COUNT", "SLOW DOWN TO 35 KMH". |
| `DriveThroughPenaltyAvatar` | DriveThroughPenaltyAvatar.obj | 0x78 | HUD: "DRIVE THROUGH PENALTY", "LAPS REMAINING". |
| `LockControlsTimeAvatar` | LockControlsTimeAvatar.obj | 0x90 | HUD: "CONTROLS ARE LOCKED FOR n SECONDS". |

### D.3 Entry points
- Physics, from `Car::stepComponents` 0x1402764d0 (player car only): `LapInvalidator::step` 0x1402c0580 ->
  `onEnterPenaltyZone` 0x1402c03d0 (-> `Car::addPenalty`); `PenaltyManager::step` 0x140265e10 (-> `Car::setBlackFlag`).
  `Car::stepJumpStart` 0x140276780 -> `PenaltyManager::addJumpStartPenalty` 0x140265a90. On the finish line
  `TimeTransponder::onTimeLinePassed` calls `PenaltyManager::checkBlackFlag` 0x140265c10 and `decreasePitPenaltyLaps`
  0x140265c30. Details are in `docs/map/car_step.md` (5.3 and 5.8).
- Main thread, all created in `Sim::Sim`: `LapInvalidatorAvatar::LapInvalidatorAvatar` 0x140111990 / `renderHUD`
  0x140111c00; `DriveThroughPenaltyAvatar::DriveThroughPenaltyAvatar` 0x1400907b0 / `renderHUD` 0x140090b50;
  `LockControlsTimeAvatar::LockControlsTimeAvatar` 0x14009d8c0 / `renderHUD` 0x14009dc50.

### D.4 What it reads from disk
`content/gui/flags/penalty.png`, `content/texture/danger.png`. `system/cfg/messages.ini [PENALTY] LOCK_CONTROL`
(`LockControlsTimeAvatar`; section and key checked against the shipped file: it switches this message on or off).
Whether penalties are on comes from `race.ini [RACE] PENALTIES` and `JUMP_START_PENALTY` (sub-area A) or the server.

### D.5 Size
`re_query.py size DriveThroughPenaltyAvatar LapInvalidatorAvatar LockControlsTimeAvatar PenaltyManager LapInvalidator`:
**43 functions, 15,042 bytes** (LapInvalidatorAvatar 12 / 5,655; DriveThroughPenaltyAvatar 10 / 3,506;
LockControlsTimeAvatar 11 / 3,448; PenaltyManager 8 / 1,600; LapInvalidator 2 / 833).

### D.6 Port difficulty: **S**
Two small physics functions and three text overlays. Depends on the tyre surface flags (is a tyre on a valid surface),
lap timing and the HUD text layer. Needed early only if lap validity matters for the comparison with the real game.

### D.7 Open questions
- The cut rule itself (how many tyres out, for how long, which surfaces) was not read here; `car_step.md` lists callers.
- How the three avatars learn the state: through `CarAvatar::physicsState` or through `evOnPenalty` (not traced).

---

## E. Driving assists and push-to-pass

### E.1 What it is
`DrivingAssistManager` runs once when the session is created. It reads the player's assist choices and switches the
matching car systems on or off (ideal line, auto-blip, stability control, auto shifter, ABS, traction control, auto
clutch) and sets the global rates (damage, fuel use, tyre wear, tyre blankets, slipstream). Online, the server's
settings win for ABS, traction control and auto clutch. `PushToPassManager` gives each car its number of push-to-pass
activations at the start of every session, scaled by grid position. The assists themselves are car physics
(`docs/map/electronics.md`); these two classes only configure them.

### E.2 Main classes
| Class | Object | Size | Role |
|---|---|---|---|
| `DrivingAssistManager` | DrivingAssistManager.obj | 0x68 | Reads assists.ini once, calls `CarAvatar` setters. |
| `PushToPassManager` | PushToPassManager.obj | 0x68 | `baseActivations`, `activationCoeff`; assigns activations per car. |

### E.3 Entry points
`Sim::Sim` -> `DrivingAssistManager::DrivingAssistManager` 0x1400fbd90 (3.3 KB, does all the work);
`DrivingAssistManager::update` 0x1400fcb20 (27 bytes: keeps the ideal-line flag in sync). `Sim::Sim` ->
`PushToPassManager::PushToPassManager` 0x1400abec0 -> `assignActivations` 0x1400ac380
(`CarAvatar::setP2PActivations`, `setP2PStartingActivations`); new-session lambda 0x1400ac170 (re-assigns, then posts
to the physics thread); `Car::evOnPush2Pass` lambda 0x1400ac0d0.

### E.4 What it reads from disk
`Documents/Assetto Corsa/cfg/assists.ini [ASSISTS] IDEAL_LINE, AUTO_BLIP, STABILITY_CONTROL, AUTO_SHIFTER, ABS,
TRACTION_CONTROL, AUTO_CLUTCH, DAMAGE, FUEL_RATE, TYRE_WEAR, TYRE_BLANKETS, SLIPSTREAM`; `cfg/controls.ini [HEADER]
INPUT_METHOD` (`X360` / `KEYBOARD` change some defaults). Push-to-pass numbers come from the car's engine data through
`CarAvatar` (not a file of its own).

### E.5 Size
`re_query.py size DrivingAssistManager PushToPassManager`: **27 functions, 6,527 bytes** (12 / 4,582 and 15 / 1,945).

### E.6 Port difficulty: **S**
One ini file mapped onto setters. Depends on config (ini reader) and the car's electronics. Write it as a plain
`Assists` struct loaded with the ini crate already chosen for the project.

### E.7 Open questions
- The formula that scales push-to-pass activations with grid position (`activationCoeff`) was not read.
- What the input-method check changes exactly (interpretation: forces some aids for keyboard / pad).

---

## F. Steam integration

### F.1 What it is
`SteamInterface` is the only user of the Steamworks SDK. At process start `SteamInit` logs the player in and installs
a crash mini-dump handler. In a session the object downloads the player's statistics, updates counters (distance,
runs, games per mode, input device used), checks and unlocks achievements, and creates / uploads / downloads the
per car-and-track leaderboards used by hot lap, time attack, drift and the special events. It also fetches the
player's avatar picture. Nothing in the simulation depends on it; it listens to events from the game modes and the
session manager.

### F.2 Main classes
| Class | Object | Size | Role |
|---|---|---|---|
| `SteamInterface` | SteamInterface.obj | 0x1f8 | Stats, achievements map, current leaderboard handle, three `CCallResult` members (find / upload / download), event `evOnLeaderboardUpdate`. |
| `CarTrackComb` | SteamInterface.obj | 0x68 | A car + track + layout combination that has an official leaderboard. |

### F.3 Entry points
- `wWinMain` 0x1400c29c0 -> `SteamInit` 0x1401a7950 (`SteamAPI_Init`, user name / id, mini-dump guard).
- `Sim::Sim` -> `SteamInterface::SteamInterface` 0x1401a5d70 -> `createLeaderboards` 0x1401aa3f0.
- Per frame: `SteamInterface::update` 0x1401accd0 -> `achievementCheck` 0x1401a8df0 (and `SteamAPI_RunCallbacks`).
- Steam callbacks: `onUserStatsReceived` 0x1401ac320 -> `addAchievements` 0x1401a9410 and `updateStats` 0x1401acd20
  (-> `incrementStat` 0x1401ac190); `OnFindLeaderboard` 0x1401a7770, `OnUploadScore` 0x1401a7830, `OnDownloadScore`
  0x1401a74b0. Achievement groups: `generalAchievements` 0x1401ab5b0, `driftAchievements` 0x1401aad30,
  `timeAttackAchievements` 0x1401acb20, `hotlapAchievements` 0x1401ac130, `practiceAchievements` 0x1401ac360,
  `damageRelatedAchievements` 0x1401aabe0; `resetAchievements` 0x1401ac850.

### F.4 What it reads from disk
`cfg/race.ini [RACE] MODEL, TRACK, CONFIG_TRACK` (to pick the leaderboard), `cfg/controls.ini [HEADER] INPUT_METHOD`.
No files of its own; everything else goes through `steam_api64.dll` (`SteamUserStats`, `SteamUser`, `SteamFriends`,
`SteamUtils`, `SteamApps`). Achievement ids are literals (`AC_ONEHUNDRED`, `AC_CLEANDRIVE`, `DRIFT_LEVEL_10` ...);
leaderboard names are built from car, track and `_lb_`, `HOTLAP_`, `TIME_ATTACK_`, `DRIFT_`, `_pro`.

### F.5 Size
`re_query.py size SteamInterface`: **66 functions, 38,068 bytes**. `FriendsLeaderboardDisplayer` is UI.

### F.6 Port difficulty: **S** (or skip)
Optional for a rewrite. If wanted, the `steamworks` Rust crate covers stats, achievements and leaderboards; the game
logic is a list of thresholds. Third-party SDK: Steamworks (`steam_api64.dll`).

### F.7 Open questions
- Whether the game refuses to run when `SteamAPI_Init` fails (the string "Steam init failed" exists; the branch was
  not read). Whether ownership / DLC checks (`SteamApps`) gate content.

---

## G. File and memory outputs (other than the three shared-memory pages)

### G.1 What it is
Three small writers. `JsonOutputFile` collects the result of every session (laps, sectors, cuts, tyres, best laps,
race result, and extra blocks for drag, drift, time attack and special events) and writes one JSON file when the game
closes; launchers read it to show results. `SharedFileOut` is the old single shared-memory page kept for
compatibility. `WarningReporter` watches frame rate and CPU load and writes a small JSON report of slow moments.
The three modern shared-memory pages are in `docs/map/telemetry.md`, the UDP telemetry in `multiplayer.md`, and the
physics channel recorder (`Telemetry`) is in sub-area H.

### G.2 Main classes
| Class | Object | Size | Role |
|---|---|---|---|
| `JsonOutputFile` | JsonOutputFile.obj | 0xc0 | Session results to JSON (uses `JSONWriter`, config.md). |
| `SharedFileOut` | SharedFileOut.obj | 0x98 | Legacy page `Local\acpmf` (`SPageFile`, 0x19c bytes). |
| `WarningReporter` | WarningReporter.obj | 0x120 | Low-FPS / high-CPU event log. |

### G.3 Entry points
- `Sim::Sim` -> `JsonOutputFile::JsonOutputFile` 0x14010c0e0 (subscribes to `RaceManager::evOnSessionEnd`) ->
  `endSession` 0x14010d310 -> `offlineEndSession` 0x14010d730 or `onlineEndSession` 0x14010e1b0, `addExtraSessionInfo`
  0x14010c5e0; file written by `writeOutputFile` 0x14010ecc0 (-> `writePlayers` 0x14010edb0), called from
  `Sim::writeOutputJson` 0x14019f950 (callers: `Sim::shutdown` 0x14019e9e0 and `TimeLimitedTest::update`).
- `Sim::Sim` -> `SharedFileOut::SharedFileOut` 0x14018e7d0; per frame `SharedFileOut::update` 0x14018e970.
- `Sim::Sim` -> `WarningReporter::WarningReporter` 0x1401d64f0; per frame `update` 0x1401d7f50 (`checkProblems`
  0x1401d6e30, `checkAverage` 0x1401d6cb0); file written in `shutdown` 0x1401d70c0.

### G.4 What it reads from disk
Writes `Documents\Assetto Corsa\out\race_out.json` and `Documents\Assetto Corsa\out\warnings.json`. Reads
`cfg/warning_reporter.ini [SETTINGS] MIN_TIME_TO_REPORT_FPS_MS, MIN_TIME_TO_REPORT_CPU_MS, MAX_CPU_OCCUPANCY_PERCENT,
MIN_FPS, CPU_AVERAGE_PERCENT_FLUCTUATION, FPS_AVERAGE_PERCENT_FLUCTUATION`. `SharedFileOut` creates the file mapping
`Local\acpmf`.

### G.5 Size
`re_query.py size SharedFileOut JsonOutputFile WarningReporter`: **44 functions, 25,504 bytes** (JsonOutputFile
20 / 14,336; WarningReporter 14 / 8,248; SharedFileOut 10 / 2,920).

### G.6 Port difficulty: **S**
`serde_json` replaces `JSONWriter` and the hand-built documents; the key names are in the string list of
JsonOutputFile.obj. The legacy page can be dropped or done with the same shared-memory code as the modern pages.

### G.7 Open questions
- Exact JSON schema (nesting and types) of `race_out.json`: only the key names were collected.
- Whether any current tool still reads `Local\acpmf`.

---

## H. Car bookkeeping components (physics side)

### H.1 What it is
These are members of `Car` that push no forces: they measure, time and record. `TimeTransponder` detects the car
crossing the track's timing lines and produces lap and sector times and the lap events everything else listens to.
`PerformanceMeter` records distance-versus-time for the best lap and gives the live "delta to best". `FuelLapEvaluator`
averages fuel used per lap. `DriftModeComponent` computes drift angle, points and combos for drift mode.
`Telemetry` is a developer channel recorder that can dump a session to a file. `SetupManager` is the table of
adjustable setup values: each `SetupItem` is a name, limits and a pointer straight into a physics member, which is
how the setup screen, the pit stop and the AI change the car. `PhysicsCarStateProvider` is a two-function adapter that
lets the render side ask a local car for its state snapshot.

### H.2 Main classes
| Class | Lib | Size | Role |
|---|---|---|---|
| `TimeTransponder` | ksPhysicsAC.lib | 0x98 | `lastLap`, `bestLap`, `lapCount`, splits, `cuts`, open-track (point to point) state. |
| `PerformanceMeter` | ksPhysicsAC.lib | 0x70 | `currentLap` / `bestLap` sample vectors, `currentDiff`, `currentSpeedDiffMS`. |
| `FuelLapEvaluator` | ksPhysicsAC.lib | 0x48 | `fuelPerLap`, `totalM`, `totalLiters`. |
| `DriftModeComponent` | ksPhysicsAC.lib | 0x50 | `points`, `instantDrift`, `comboCounter`, `currentDriftAngle`, `invalid`. |
| `Telemetry` / `TelemetryChannel` | ksPhysicsAC.lib | 0x100 / 0x58 | Named channels sampled each step. |
| `SetupManager` / `SetupItem` | ksPhysicsAC.lib | 0x50 / 0x88 | Setup table, rule check (`minimumHeight_m`). |
| `PhysicsCarStateProvider` | PhysicsCarStateProvider.obj | 0x10 | `ICarPhysicsStateProvider` over a local `Car`. |

### H.3 Entry points
- All are initialised in `Car::Car` 0x14026bf00 and stepped from `Car::stepComponents` 0x1402764d0 (order in
  `docs/map/car_step.md`): `SetupManager::step` 0x14028d090, `Telemetry::step` 0x1402bf5b0, `DriftModeComponent::step`
  0x1402bff20, `PerformanceMeter::step` 0x14026b8e0, `TimeTransponder::step` 0x1402911f0 (-> `onTimeLinePassed`
  0x140290c20 -> `lap` 0x140290a50 / `split` 0x140291150), `FuelLapEvaluator::step` 0x14028d360. Telemetry, drift and
  performance meter run for car 0 only.
- Setup: `SetupManager::init` 0x140289290 -> `initItems` 0x140289570 (14 KB of registrations), `getSetupItem`
  0x1402891b0 (used by the setup screen, the pit stop and the AI), `load` 0x14028cc90 (AI setups, from
  `AIDriver::initSetup`), `isSetupRespectingRules` 0x14028cc10, `SetupItem::SetupItem` 0x1402cb170.
- Readers: `PerformanceMeter::getCurrentSplit` 0x14026b700 (shared memory, net code, `Car::getPhysicsState`);
  `Telemetry::init` 0x1402bc010, `Telemetry::save` 0x1402bf070 (called from `Car::~Car` 0x14026e0d0);
  `DriftModeComponent::init` 0x1402bfda0; `TimeTransponder::init` 0x1402907c0; `FuelLapEvaluator::init` 0x14028d2b0;
  `PhysicsCarStateProvider::getPhysicsState` 0x140122a40.

### H.4 What it reads from disk
Car `data/setup.ini` (per item: `VALUE`... `MIN`, `STEP`, `SHOW_CLICKS`) and `data/car.ini [RULES] MIN_HEIGHT`
(`SetupManager`). `system/cfg/assetto_corsa.ini [TELEMETRY] EXPORT_ENTIRE_SESSION, DEBUG_PHYSICS, DEBUG_AI`
(`Telemetry::init`); the dump is written to `Documents/Assetto Corsa/aim/telemetry_dump.act` by `Car::~Car`.
`system/cfg/tyre_smoke.ini [TRIGGERS] SLIP_LEVEL` (`DriftModeComponent::init`). `data/fuel_cons.ini [FUEL_EVAL]
KM_PER_LITER` is read by `Car::Car` for the first fuel estimate. The timing lines come from the track (`track.md`).

### H.5 Size
`re_query.py size Telemetry PerformanceMeter TimeTransponder DriftModeComponent FuelLapEvaluator SetupManager SetupItem
PhysicsCarStateProvider`: **73 functions, 50,260 bytes** (SetupManager 14 / 20,899; Telemetry 14 / 19,467;
TimeTransponder 15 / 4,073; PerformanceMeter 6 / 2,070; DriftModeComponent 4 / 1,954; FuelLapEvaluator 7 / 715;
SetupItem 2 / 539; PhysicsCarStateProvider 11 / 543). Not counted here although listed in the task: `SplineLocator`
(counted in `ai_drivers.md`), `ThermalObject` and `PhysicsObject` (`docs/map/body.md`), `TyreTester`
(`docs/map/tyre_oracle.md`), `SignalGenerator` (`camera.md`), `SharedMemoryWriter` (`docs/map/telemetry.md`).

### H.6 Port difficulty: **M**
`TimeTransponder` is small but must match the game to compare lap times (line crossing with sub-step time offset,
validity rules). `SetupManager::initItems` is long, mechanical and touches every physics system: in Rust it becomes a
table of (name, unit, multiplier, accessor) and is best written last, system by system. The rest is S. `Telemetry`
(the .act dump) can be replaced by any logging of our own.

### H.7 Open questions
- Format of `telemetry_dump.act` (binary channel dump) and whether a stock tool reads it.
- Sub-step interpolation of the line-crossing time (`PhysicsEngine::getOddTimeOffset`) was not read here.
- Full list of setup item names against the 113 cars' `setup.ini` (the per-system maps list their own items only).

---

## I. Shared utilities

### I.1 What it is
Small helpers used by many areas and owned by none. `Curve` is the look-up table class behind every `.lut` file and
every inline `(x=y|x=y)` table in the car data: it loads pairs (from disk or from the encrypted `data.acd`),
interpolates linearly or with a cubic spline. `PIDController` is a plain PID step used by the AI steering and the
(unused) active suspension. `Speed` is a unit wrapper (m/s inside). `ksUtils` are five one-line physics formulas.
`NKUtils::getSectorID` parses the number out of a track node name. `Collisions::rayMeshIntersect` is the ray against
triangle-mesh test used by mouse picking in the scene graph (not by the physics).

### I.2 Main classes
| Class / function | Lib | Size | Role |
|---|---|---|---|
| `Curve`, `CubicSpline<float,float>` | KunosSim.lib | 0x80 / 0x20 | LUT loading and interpolation; static `openedFiles` list. |
| `PIDController` | ksCommon.lib | 0x14 | `P`, `I`, `D`, `currentError`, `integral`. |
| `Speed` | ksCommon.lib | 0x4 | `fromKMH`, `fromMPH`, `fromMS`. |
| `ksCalcCamberRAD`, `ksCalcSlipAngleRAD`, `ksCalcContactPatchLength`, `ksGFromRadius`, `ksSpeedFromRadius` | ksPhysicsAC.lib (ksUtils.obj) | - | Free functions. |
| `NKUtils` | NKUtils.obj | 0x8 | `getSectorID`. |
| `Collisions` | KunosSim.lib | - | `rayMeshIntersect`. |

### I.3 Entry points
`Curve::load` 0x140206a50 (callers: `INIReader::getCurve`, `Tyre::initCompounds`, `Engine::loadINI`, `ABS::init`,
`TractionControl::init`, `Wing::Wing`, `DynamicWingController::initCommon`, `AnalogInstruments::initSpeed`) ->
`Curve::loadEncrypted` 0x140206f50; `Curve::getValue` 0x140206990, `getCubicSplineValue` 0x1402068f0,
`CubicSpline<float,float>::computeCoefficients` 0x140205c20. `PIDController::eval` 0x14044e720 (callers
`AIDriver::stepSteer`, `ActiveActuator::eval`). `Speed::fromKMH` 0x140239970. `ksCalcCamberRAD` 0x1402cafe0,
`ksCalcSlipAngleRAD` 0x1402cb080 and `ksCalcContactPatchLength` 0x1402cb040 (callers `Tyre::addTyreForces` /
`addTyreForcesV10`), `ksGFromRadius` 0x1402cb0b0 and `ksSpeedFromRadius` 0x1402cb0f0 (callers in `AIDriver`).
`NKUtils::getSectorID` 0x14018d2b0 (caller `TrackAvatar::processPhysicsNode`). `Collisions::rayMeshIntersect`
0x14044cdd0 (caller `RayPickerTraverser::onNode`).

### I.4 What it reads from disk
Only `Curve`: any `.lut` text file named by the car data (`x|y` per line), from the car folder or from `data.acd`.

### I.5 Size
`re_query.py size Curve PIDController Speed ksUtils NKUtils Collisions`: **34 functions, 9,838 bytes** (Curve
16 / 7,768; Collisions 1 / 895; NKUtils 4 / 540; ksUtils 5 / 365; PIDController 4 / 220; Speed 4 / 50).

### I.6 Port difficulty: **S**
All trivial except that `Curve` must be bit-compatible for the physics (linear interpolation, end clamping and the
cubic variant): port it by hand, about 100 lines. The three tyre formulas in `ksUtils` are already covered by the
tyre port. Ray picking can use any geometry crate.

### I.7 Open questions
- Which callers use the cubic spline path of `Curve` and which the linear one (matters for bit-exactness).
- `Curve::getValue` behaviour outside the table range was not re-read here (the per-system maps quote it).

---

## Leftover source files

Object files counted in this document (each in exactly one sub-area). Library `acs.exe` = linked directly into the
executable, not through a static library. Function counts and bytes are from `re_query.py size`.

| Object file | Library | Functions | Code bytes | Sub-area |
|---|---|---|---|---|
| RaceManager.obj | acs.exe | 108 | 80,235 | A |
| RaceTimingServices.obj | acs.exe | 40 | 16,713 | A |
| SessionResultProcessor.obj | acs.exe | 17 | 25,999 | A |
| CarRaceInfo.obj | acs.exe | 17 | 5,803 | A |
| DriverNameProvider.obj | acs.exe | 2 | 248 | A |
| TimeAttack.obj | acs.exe | 39 | 31,368 | B |
| DragMode.obj | acs.exe | 37 | 23,467 | B |
| DriftMode.obj | acs.exe | 18 | 11,527 | B |
| GameMode.obj | acs.exe | 10 | 11,574 | B |
| TimeLimitedTest.obj | acs.exe | 13 | 1,922 | B |
| RaceEvent.obj | acs.exe | 12 | 1,762 | B |
| HotlapMode.obj | acs.exe | 11 | 1,404 | B |
| PitStop.obj | acs.exe | 57 | 28,371 | C |
| PitCrew.obj | acs.exe | 41 | 20,744 | C |
| LollipopCrew.obj | acs.exe | 16 | 4,980 | C |
| LapInvalidatorAvatar.obj | acs.exe | 12 | 5,655 | D |
| DriveThroughPenaltyAvatar.obj | acs.exe | 10 | 3,506 | D |
| LockControlsTimeAvatar.obj | acs.exe | 11 | 3,448 | D |
| PenaltyManager.obj | ksPhysicsAC.lib | 8 | 1,600 | D |
| LapInvalidator.obj | ksPhysicsAC.lib | 2 | 833 | D |
| DrivingAssistManager.obj | acs.exe | 12 | 4,582 | E |
| PushToPassManager.obj | acs.exe | 15 | 1,945 | E |
| SteamInterface.obj | acs.exe | 66 | 38,068 | F |
| JsonOutputFile.obj | acs.exe | 20 | 14,336 | G |
| WarningReporter.obj | acs.exe | 14 | 8,248 | G |
| SharedFileOut.obj | acs.exe | 10 | 2,920 | G |
| SetupManager.obj | ksPhysicsAC.lib | 14 | 20,899 | H |
| Telemetry.obj | ksPhysicsAC.lib | 14 | 19,467 | H |
| TimeTransponder.obj | ksPhysicsAC.lib | 15 | 4,073 | H |
| PerformanceMeter.obj | ksPhysicsAC.lib | 6 | 2,070 | H |
| DriftModeComponent.obj | ksPhysicsAC.lib | 4 | 1,954 | H |
| FuelLapEvaluator.obj | ksPhysicsAC.lib | 7 | 715 | H |
| SetupItem.obj | ksPhysicsAC.lib | 2 | 539 | H |
| PhysicsCarStateProvider.obj | acs.exe | 11 | 543 | H |
| Curve.obj | KunosSim.lib | 16 | 7,768 | I |
| Collisions.obj | KunosSim.lib | 1 | 895 | I |
| NKUtils.obj | acs.exe | 4 | 540 | I |
| ksUtils.obj | ksPhysicsAC.lib | 5 | 365 | I |
| PIDController.obj | ksCommon.lib | 4 | 220 | I |
| Speed.obj | ksCommon.lib | 4 | 50 | I |
| **Total (40)** | | **725** | **411,356** | |

Object files that appear in no atlas Size list because a car-physics map in `docs/map/` owns them (50 objects, 671
functions, not counted above): Car, PhysicsEngine, Track, Tyre, TyreThermalModel, BrushTyreModel, BrushSlipProvider,
SCTM, TyreTester, Engine, Turbo, Drivetrain, Autoclutch, AutoBlip, AutoShifter, GearChanger, Kers, ERS, BrakeSystem,
ABS, EDL, TractionControl, StabilityControl, SpeedLimiter, DynamicController, Suspension, SuspensionStrut,
SuspensionAxle, SuspensionML, HeaveSpring, AntirollBar, Damper, ActiveActuator, SteeringSystem, FFPostProcessor,
AeroMap, Wing, DynamicWingController, DRS, DRSManager, SlipStream, CarColliderManager, PhysicsObject, ThermalObject,
SharedMemoryWriter, PhysicsCore, RigidBodyODE, CollisionMeshODE, RayCaster, ksPhysicsCoreODEFactory.

Notes for whoever merges the sizes (open questions about the atlas as a whole):
- `GearShiftShake.obj` (4 functions) is counted in both `input_ffb.md` and `renderer.md`; `DriverEyesPositioner.obj`
  (37 functions, SystemApps.lib) is counted in `camera.md` and is also one of the 56 SystemApps files summed in
  `ui.md`. Summing the Size sections of all atlas files therefore gives 41 functions more than the true total.
- The docs/map files have no Size sections; the split above treats an object as "physics map" when a map describes
  its step function. `Telemetry`, `SetupManager`, `TimeTransponder` and the other sub-area H objects are named in the
  maps' call trees but described nowhere else, so they are counted here.
