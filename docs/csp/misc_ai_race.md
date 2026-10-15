# Misc A: AI drivers, race logic, replay, pits, sessions and game modes

This area is everything CSP changes about **who drives the other cars and how a session is run**: the 19 hooks on
`AIDriver`, the AI line loaders and recorder, 11 hooks on `RaceEngineer` (the AI's "what can this car do" calculator),
the `RaceManager` / `FlagManager` / timing hooks, the replay manager and its **extended replay file**, the ghost car,
comparative telemetry, the pit stop and pit crew hooks, and the Lua "new modes" host. Nothing is encrypted in this area.
Almost all of it is small wrappers around the game's own code; the one big new thing is the replay extension block
(verified byte by byte on 9 real files, section 1.5).

Evidence markers: `[ini]` shipped config comment, `[sdk]` Lua SDK, `[lua]` shipped Lua, `[dll]` read in dwrite.dll
(decompile or string table, RVA given), `[log-csp]` the owner's `custom_shaders_patch.log`, `[file]` measured on
the owner's real files (read only), `[prior]` earlier task, `[interp]` my inference. Hook category as in
`physics_car_hooks.md` section 2: A = pass-through that only records or fires an event, B = wraps the original (changes
its inputs or outputs), C = replaces it, D = lookup (CSP only calls it). "RVA" = dwrite.dll RVA, image base
0x180000000; acs.exe addresses are 0x14... VAs.

## 1. What it does

### 1.1 AI drivers (the circuit AI, `AIDriver`)

CSP does not write a new AI. It keeps AC's `AIDriver` and wraps **19** of its functions (plus 2 lookups) so that a
list of small, individually switchable fixes applies. Each fix is one byte in `.data` that the wrapper reads. The
fixes, with the shipped key that sets the byte (section 3 has the table of byte addresses):

* **Gear shifting** (`stepGears`): upshift RPM is capped at the limiter minus 100 rpm; downshift ratio is derived from
  the car's gear ratios (x0.9 for AIs below level 0.5); on slow track sections (hint speed 1..130 km/h) upshifts happen
  earlier so AIs stop revving. Keys `FIX_UPSHIFT_THRESHOLD`, `FIX_DOWNSHIFT_THRESHOLD`, `ADJUST_SHIFTING_FOR_SLOW`. [dll]
* **Pace and grip** (`stepGasBrake`, `RaceEngineer::evalLateralGFromLoads`, `evalLateralGrip`, `getOptimalBrake`):
  `FIX_LOW_LEVEL` lets an AI with level below 0.8 run the throttle/brake maths as if it were 0.8; `FIX_GRIP_MULT`
  low-passes the 1.2x tyre grip multiplier the AI gets (`Tyre::aiMult`; rate 5/s) and drops it to 1.001 while two
  or more tyres are in a certain state; `NEW_LATERAL_GRIP_ESTIMATION` replaces the lateral-g estimate from tyre
  loads (mode 1 adds braking only for three-wheelers, mode 2 for all cars); `FIX_SHORT_AXLE` clamps a short axle track to 0.25 m
  inside the old estimate; the hidden `ALTERNATIVE_OPTIMAL_BRAKE` replaces `getOptimalBrake` by a per-wheel brake
  torque sum. The AI's grip number is also multiplied by per-car extension factors (extended-physics grip multiplier,
  a 0..255 % byte for AI-driven cars, a per-car signed byte that looks like the rubber-banding offset). [dll]
* **Steering** (`stepSteer`, `getSteerAtDistance`): above 200 km/h the look-ahead base grows with speed (x speed*0.018)
  instead of being capped (`REMOVE_STEER_LOOKAHEAD_SPEED_LIMIT`); `CAUTIOUS_STEERING` shrinks the steering target
  in tight corners (corner radius below 14 m on the line payload, blend 0.03 per call). [dll]
* **Start of a race** (`stepGasBrakeStart`, `stepCarsAwareness2`): before the green light the throttle is held between
  0.4 and 0.5 instead of redlining (`START_REDUCE_REDLINING`), and the engagement/offset logic runs with a scaled
  time step so the cars leave the grid straight (`START_STRAIGHTEN_TRAJECTORY`). [dll][interp]
* **Autopilot / stuck cars** (`acquireControls`): `STOP_TELEPORTING_STUCK` pins the stuck timer so a stuck AI never
  teleports to the pits; `ACT_IMPATIENT` switches a blocked AI into an "impatient" mode (a CSP function, flag 0x400)
  when the car ahead is slower than 8 m/s and not in the pits; with no usable spline the car just brakes. [dll]
* **Smoothing that is always on**: after the original, throttle changes are rate-limited (9.45 per second when
  brake >= 0.2, otherwise 1.89 per second, only while the engine is below 96 % of the limiter) and below 2 m/s the
  steering change per call is limited to 0.003 (0.012 above 0.5 m/s). No key. [dll]
* **Visual smoothing** (`NEW_AI_SMOOTHING`): not in these hooks; done in `Car::getPhysicsState` (0x59e920) and
  `CarAvatar::setNewPhysicsState` (0x10ae290) with critically damped springs, "only visuals and audio" [ini][dll].
  Cross-reference: `physics_car_hooks.md`, `rendering_core.md`.
* **Pit and tyre decisions**: `updateTyres` is **replaced**: the compound is chosen by (a) listeners on two event
  buses (so the AI-flood can force street tyres), else (b) by the car's `ksPhysics` mode 1 -> `RaceEngineer::findTyreCompound`,
  mode 2 -> `findTyreForQualify`, otherwise the driver's preferred index; a log line "AI car: %u setting compound: %d
  for tyre:%d" is written. `onNewSession` runs the original with the car's physics GUID forced to 0 so that an AI
  driving the player's car (autopilot) also builds pit strategy. [dll][interp]
* **Alternative splines** (`stepSplineSelection`, `updateCurrentSplinePos`): an AI may be put on a CSP-made parallel
  spline ("starting from a non-AI spot", `ALT_DRAG_SPLINE`; "simpler spline for low-level AIs",
  `TRAJECTORIES_FOR_DUMMIES`; "prefer the middle when it rains", `AVOID_SIDES`); its position along it is kept in a
  per-car array so the original can be reused unchanged. [dll][interp]

What is **not** changed: the AIDriver's overall structure, the 5-step cadence, `stepSuperhuman` maths (only skipped for
CSP-controlled drivers), the genome and random draws. A CSP-driven AI lap is therefore plain AC's AI plus the
arithmetic above, **in CSP's air, on CSP's ODE** (`prior/csp_survey.md` I9, P1, P3).

### 1.2 AI lines (`AISpline`) and the recorder (`AISplineRecorder`)

* **Grid and payload caches**: CSP replaces the AI grid with a sparse one (`ALT_GRID`, cache folder `<AC>\cache\ai_grids`)
  and caches the computed payloads (`CACHE_PAYLOADS`, `<AC>\cache\ai_payloads\<track>__<hash>.bin`). Verified format
  [file]: zlib stream; after inflating: `u64 key`, `u32 count`, then `count` x 84-byte `AISplinePayload` (0x54, the
  struct of `docs/atlas/ai_drivers.md` section 2). Sizes match the `.ai` point counts exactly: Imola fast lane
  3,166 points -> 265,956 bytes = 12 + 3,166 x 84; Monaco 2017 fast lane 2,187 -> 183,720; pit lane 460 -> 38,652.
  On this PC: "Loading cached payloads for fast_lane: 13072 us", "Normals are already computed" [log-csp]. The log
  strings "Cached payload is from a different spline", "Wrong number of items (payloads=%d, loaded=%d)",
  "Failed to cache payloads for", "Computing payloads for", "Spline is too big, using linear interpolation for computing
  metadata" [dll 0x2216108..0x2216290].
* **Hooks**: `AISpline::load` is wrapped (log "Loading AI spline", records which object is `fast_lane.ai` and which is
  `pit_lane.ai` by the last 12 / 11 characters of the file name, globals 0x1826a36f8 / 0x1826a3700); `loadFast` pushes
  the file name into a per-thread slot; `initStraights` logs and times; `payloadAtPosition` and `calculateNormals` are
  **replaced** by CSP's own code when the hidden `[AI_SPLINES] EXPERIMENTAL_OPTIMIZATIONS` list asks for it (the two
  bits `CUSTOM_PAYLOAD_COMPUTE`, `CUSTOM_NORMALS_COMPUTE`, bytes 0x268a8c8 / 0x268a8c9); the normals code makes the
  same three ray casts per point as AC (centre and +-0.6 m) and writes camber (+0x10), normal (+0x18..+0x20) and grip
  (+0x40) of the 0x54 payload. Skipped entirely when the payloads came from the cache. [dll]
* **Recorder**: `AISplineRecorder::load` / `loadHints` are skipped when a per-thread flag (+0x1aa of the TLS block)
  is set (CSP loads splines itself in some paths); the constructor clears two extra bytes (+9, +10); `startRecording`
  clears the `CACHE_PAYLOADS` byte (a re-recorded line must not be cached); `step` binds the recording to car 0 when
  none was named; `save` calls the original and, if a global byte (0x182699588) is set, throws (aborts after saving:
  developer/automation case) [interp]. Lua: `ac.isAISplineRecorderPresent/Active/RecordingPitLane`,
  `getAISplineRecordedFrames`, `startAISplineRecorder(recordPitLane)`, `stopAISplineRecorder(save, callback)` [sdk].

### 1.3 The "new behaviour" module (`new_behaviour.ini`): flood, cooldown, retirement, rubber-banding, wrong way, reset

A separate CSP module (code RVA 0x1246180 .. 0x124f0fe, about 45 functions, none of them a hook; they hang on event
buses that the hooks fire) adds behaviours that **AC does not have at all**. All are offline only [ini]. What the
shipped file and the strings say (keys in section 3):

* **AI flood (Track Day)**: endless traffic. New AIs spawn ahead of the player (`SPAWN_DISTANCE`, `SPAWN_RANGE`,
  `SPAWN_MIN_DISTANCE_TO_PLAYER`), cars behind are despawned (`SPAWN_DESPAWN`), spawned cars are pushed to speed
  (`PUSH_FORCE` N / `PUSH_SPEED` km/h), tyres chosen from `PREFERRED_TYRES`, behaviour varied
  (`SHUFFLE_BEHAVIOUR`, `SPEED_LIMIT` low..high km/h), colours and driver names shuffled/hidden. Only on tracks
  longer than `MIN_TRACK_LENGTH` (7000 m). Log text "Two lane traffic spline is detected" (function 0x12489a0, 4,267
  bytes) [dll].
* **AI race cooldown lap** (default off; the owner turned it on with `LIMITER=0.67`): after the chequered flag, AIs
  drive a slow-down lap with a throttle limit and speed limit and then return to the pits and are locked there
  ("ai_race.cooldown: <n> laps remaining, locking car in pits", "... returning to pits") [dll 0x1246f20, 0x124d480].
* **AI race retirement**: AIs with damage / stuck for too long retire with random messages ("forgot how to drive",
  "had enough", "needed a break", "was done waiting", "ran out of patience", "gave up", "got bored", "called it a
  day", "Driver got stuck", "%ls %ls and decided to get back to pits", " will rejoin"); `REJOIN` lets them come back,
  `PIT_WITH_DAMAGE`, `TELEPORT_WITH_DAMAGE` choose the exit (function 0x1247180, 956 bytes). [dll][ini]
* **Rubber-banding** (default off): cars behind get up to +10 % level, cars ahead up to -3 %, ramping over
  `CHANGE_TIME` s and `CHANGE_DISTANCE` m. The offset is stored per car in a signed byte array at 0x1826ab910 that
  `RaceEngineer::evalLateralGrip` reads (factor `1 + byte/300`). [ini][dll]
* **AI adaptation** (default off, "very experimental"): per-AI variance that changes over time.
* **Wrong way**: lets the player drive the wrong way in practice and in Track Day without the penalty
  (`[WRONG_WAY] ALLOW_IN_PRACTICE`, `ALLOW_IN_TRACKDAY`; per-track override `[TRACK_<name>] WRONG_WAY_ALLOW_IN_TRACKDAY`,
  hidden key `ALLOW_EVERYWHERE`). `physics.setWrongWayPenalty(bool)` is the Lua switch [sdk]. Where the check is
  skipped was not located (the loader writes into a module object, not `.data`).
* **Reset car / step back** (hidden, offline, never in a race, invalidates the lap): bindings `__EXT_CMD_RESET`
  ("Reset car (for offline practice sessions)") and `__EXT_CMD_STEP_BACK` ("Reset & step back"); the car is put back
  on the AI spline `RESET_DISTANCE` (30) m / `STEPBACK_DISTANCE` (300) m behind; messages "Reset car: ", "Taking a
  step back", "Spline is missing", "Not allowed" (functions 0x124a0d0, 0x14f2900) [dll][ini].
* **Custom AI helper** (`[CUSTOM_AI] ENABLED=0`, hidden): an external process can drive cars through the named pipe
  `\\.\pipe\AcTools.CSP.NewBehaviour.CustomAI.Helpers` and a shared-memory file mapping; the pipe serves
  `RaceEngineer` answers (`evalLateralGrip`, `getMaxSpeedFromGear`, `loads`, `aeroHint`) and logs "Car control
  provider is ready: " (functions 0xd25d90, 0xd27d00, 0xd27f90). `DISABLE_AUDIO/PYTHON_APPS/TEXTURES` strip the
  session for speed. [dll]

### 1.4 Race logic and session rules

Hooks on `RaceManager` (18 hooks, 17 lookups) mostly **fire CSP events** (so Lua / modules can react or veto) and
add a handful of rules. What changes for the player:

| Rule | Gate | What it does |
|---|---|---|
| **White flag / "final lap"** | `gui.ini [TWEAKS] FINAL_LAP_TWEAK` 1 or 3 (default 0) | `FlagManager::update` shows the white flag (internal flag 5) when the leader is on the last lap of a lap race, or when the time is up in a timed race; AC itself never sets one. Value 3 also hides the "Leader on last lap" text by making `getLeaderLapCount` answer 0 while `getRaceText` runs. [dll][ini] |
| **Chequered flag early** | `gui.ini SHOW_CHEQUERED_FLAG_EARLY` (default 0) | the chequered flag is shown as soon as the leader has finished (not only after the player), unless the black flag is up. [dll][ini] |
| **Hide race flags / yellow / blue** | `HIDE_RACE_FLAGS`; new-mode manifest `DISABLE_FLAGS=YELLOW, BLUE` | flags are not drawn / `checkForYellow`, `checkForBlue` are skipped. The two checks are also throttled to once per 310 / 315 ms. [dll] |
| **First lap armed on a hot-lap spawn** | `general.ini [PHYSICS_EXPERIMENTS] ARM_FIRST_LAP` (hidden, default 1) | true only when `race.ini [SESSION_0] SPAWN_SET=HOTLAP_START` and `TYPE` <= 2 (practice / qualifying); `TimeTransponder::init` then arms the first lap at the line. [dll] 0x138ded0 |
| **1 ms lap times** | `PRECISE_TRANSPONDER` (default 1) | byte 0x268b680 read by `TimeTransponder::step/onTimeLinePassed` (`physics_car_hooks.md`). |
| **Pit-box test 1.5 m** | `EQUALIZE_PITS_CHECK` (default 1) | `PitStop::isInsidePitArea` replaced: squared distance to the box < 2.25 (1.5 m). [dll] |
| **Offline `ALLOWED_TYRES_OUT`** | none (race.ini `[LAP_INVALIDATOR] ALLOWED_TYRES_OUT`, Content Manager writes -1) | `RaceManager::initOffline` reads the key and, if it is > 0 and the session value is still the default 2, sets it; plain AC only gets it from a server. [dll] 0x10ed7c0 |
| **Restart refused** | `small_tweaks.ini [RULES] DISABLE_SESSION_RESTART` (0), or the server rule | `restartCurrentSession` shows "Restarting session is not allowed in this race" instead. [dll][ini] |
| **No teleport to pits** | `[RULES] DISABLE_TELEPORT_TO_PITS` (0) | key read by `Esc`-menu code (cross-ref `ui_apps.md`). |
| **Black flag for worn tyres** | per-car rule object (+0x1d0), race only, not Track Day | after the player's lap, if a tyre-set wear value is at or below its limit and the race is not over, log "Wear is illegal, setting black flag" and black-flag the car. [dll] 0x10eec10, rule source [interp] |
| **Invalid-state cars back to pits** | session type < 1 [interp] | `resetInvalidStateCarsToPits` replaced: per-car timer (capped 0.5 s steps), resets cars with fewer than 2 wheels on the ground when slow, or when CSP marks them so. [dll][interp] |
| **Leader lap count memoised** | none | cached per frame (key `_DAT_1826add30`), so `getRaceText` can override it. |
| **Lap count 1 for car 0 during comparative telemetry** | none | see 1.6. |
| **Session results of AI** | CSP "new mode active" byte 0x182695041 or Track Day | `getCompleteAIResults` runs with the AI's lap count field zeroed (AIs are not scored in a new mode / track day). [dll][interp] |
| **`race_out.json` post-processing** | listeners on bus 0x182690bb0 | `JsonOutputFile::writeOutputFile` reads the freshly written `out\race_out.json`, lets listeners (new modes' `ac.endSession(message, successfulRun, results)`) rewrite it and writes it back; error text "Failed to update race results: ". [dll] |
| **TimeAttack** | none | the constructor appends one callback to the sim's callback list and replaces its invoker with CSP's (0x6a57c0): `ac.evaluateLapTime()` uses AC's time-attack estimator. [dll][interp] |
| **DragMode** | none | the constructor looks for the track object `AC_OPEN_FINISH_L`; if absent it shows "Failed to set the drag session: track is not meant for it" and offers to switch to practice instead of crashing. [dll] |

Events fired (CSP-internal buses, `[dll]`): restart (0x1826973c0), isRaceOver (0x1826973c8), setCurrentSession
(0x1826973d0), onLapCompleted (0x1826973d8), renderHUD (0x1826973e0, cancelable), checkForPenalty (0x182690bd0),
flag update (0x182690bd8), onSectorSplit (0x18268cbe8). These are what `ac.onSessionStart`, `ac.onLapCompleted(car, cb)`,
`ac.onCarJumped` and the new-mode functions ride on [sdk][interp].

**Not changed**: the referee state machine, jump start, penalty maths, drive-through, black flag logic, results
tables, `race_out.json` content. `docs/port/race_rules.md` stays valid with the additions above.

### 1.5 Replay: the extended replay file, lazy ring, loader

What the game writes is untouched: AC's version-16 replay body. CSP appends one block **after** it (verified on the
9 files in `Documents\Assetto Corsa\replay\temp`, 24-car Spa and 1-car Monaco, all files carry it: 9/9). [file]

```
[ AC replay v16 data               ]  A bytes   (A = the game's own file size)
[ CSP extra block                  ]  n - A - 31 bytes
[ marker "__AC_SHADERS_PATCH_v1__" ]  23 bytes
[ u32 A  (offset of the extra block) ]
[ u32 1  (version / flag)          ]
```

The CSP log of the owner's last session reads "Replay extra recorder: original size: 1396956" for a file with
A = 1,396,956 [log-csp][file]. Inside the extra block is a flat list of entries `u32 nameLen | name | u32 size | payload`
(no padding, parsed to the exact end on all 9 files). Entries seen, in file order:

| Entry | Payload | Meaning (verified / inferred) |
|---|---|---|
| `CONFIG_RACE` | text, 1,329..1,403 bytes | a copy of `race.ini` as the session ran (starts `[BENCHMARK]`) [file] |
| `WFX_IMPLEMENTATION`, `WFX_CONTROLLER` | 4 bytes each (`base`) | names of the WeatherFX implementation / controller [file] |
| `EXT_PERRACE_v1` / `EXT_PERRACE_v2` | 8 / 16 bytes (zeros in 8 of 9 files; v2 is `8017d06a...` once) | per-race header [interp] |
| `EXT_SESSIONDATA_v1` | 47..73 bytes | `u32 len + "Qualify"/"Practice"`, `u32 len + "PIT"/"HOTLAP_START"`, flags, floats -1.0, -1: the session record that `ac.overrideSession` also uses [file][interp] |
| `EXT_CONDITIONSVERSION_v1` | 4 bytes (`1`) | |
| `EXT_PERFRAME_v1` | zlib; inflated = **56 bytes x frames** | environment per frame (temperatures, ...) [interp] |
| `EXT_PERRACEFRAME_v1` | zlib; inflated = **16 bytes x frames** | first two floats = a time stamp falling 30 per frame (ms of the 33 Hz recording) [file][interp] |
| `EXT_PERCAR_v7:<i>` | zlib; inflated = **108 bytes x frames**, one entry per car (24 in a 24-car race) | per car per frame extras: 27 dwords, four 5-dword groups (one per wheel) then 7 dwords, half-float packed [file][interp] |
| `EXT_EXTRASTREAM_v1` (n) | `u64 key` + zlib | Lua `ac.ReplayStream` data: 24 streams of 249 bytes (1 byte per 16 frames) in the Spa files; 2 streams (8 bytes per frame, 1 byte per 16 frames) in the Monaco files [file][sdk] |
| (`EXT_EXTRABLOB_v1`) | | `ac.writeReplayBlob(key, data)`; not present in the 9 files [dll string 0x2292388] |

Arithmetic that proves the shapes (frames of each file are the game's own counts): Spa race 3,982 frames ->
PERFRAME 222,992 = 3,982 x 56; PERRACEFRAME 63,712 = 3,982 x 16; PERCAR 430,056 = 3,982 x 108; qualifying 3,218
frames -> 180,208 / 51,488 / 347,544; Monaco single car 6,254 / 3,012 / 10,143 / 4,157 frames -> 108 x frames
675,432 / 325,296 / 1,095,444 / 448,956. Compression is zlib header `78 01` (miniz, fastest); the log prints
"`EXT_PERCAR_v7:0` compressed from 659 KB to 166 KB, total frames: 6254" [log-csp]. Older section names the reader
still accepts (compat): `EXT_PERCAR_v1..v6:`, `EXT_PERRACE_v1/v2`, `EXT_PERFRAME_v1`, `EXT_PERRACEFRAME_v1`,
`EXT_CONDITIONSVERSION_v1`, `EXT_EXTRASTREAM_v1`, `EXT_EXTRABLOB_v1`, `EXT_SESSIONDATA_v1`; messages "Extended
replay: not a custom replay / unsupported version / damaged / size does not match / unknown section / file is too small
to mean anything" [dll 0x2292060..0x2292190].

Other replay behaviour:

* **Writer** (`ReplayManager::save` hook 0x11a91e0 wraps; the extension writer is 0x11a4750, 4,816 bytes, strings
  "Writing extra data: cut_in=, cut_out=, cut_size=, pivot=, recorded_frames="). A bus (0x182697810) lets listeners
  change the file name. With `LIMIT_TRACK_OBJECTS_IN_REPLAYS` the track-object recorder's frame vector is emptied
  during the original save when there are more than 50 objects.
* **Reader** (`ReplayManager::load` hook 0x11a8f70 is a **full replacement**): AC's loader is not called. CSP's
  loader (0x11a66c0 + per-car 0x11a7a70) logs "Loading replay (version N)", loads a missing car as "a pink box"
  with a message instead of crashing, checks "too many laps", "too many LB entries", "damaged frames counters",
  recognises archives ("This is not a replay, but a %ls archive") and a missing file; then 0x11a4000 / 0x11a2cb0
  read the extension. This is why the two files that crash `acs.exe` (`docs/port/replays.md`, leaderboard snapshots
  shorter than the car count) play under CSP. [dll]
* **Recording** (`ReplayRecorder::recordFrame` hook 0x11aa1b0 wraps): fires a pre-frame bus (0x1826977f8), grows the
  ring gradually (see below), after the original optionally zeroes the 4th additional-info byte, and with
  `ACCOUNT_FOR_PING` (default 1, offline cars driven by a net provider) re-times remote cars' frames by their ping;
  then asks the extension store for a 0x6c-byte slot (`0x11a25b0`) and fires bus 0x182697858 whose listener
  (0x11a0ac0) fills it. `getPhysicsState` (0x11a9cb0): with `JUMP_FIX2` a pair of recorded frames more than 30 m apart is
  not interpolated (teleports no longer smear). [dll]
* **Ring** (`setFrameSize` 0x11a1510, C): capacity per car starts at `10000 + 1730 x carIndex` frames and is grown on
  demand (by 0x11a0c00, step `10000 - 1.4 x used`) unless `GRADUAL_REPLAYS_COMMIT` / the "virtual allocation" flag
  says otherwise; the log still reports "Replay manager is created, cars: 1, frames: 95588" [log-csp] (95,588 frames
  = 47.8 min at 33.3 Hz, which `log.txt` also prints). `EXTRA_LARGE` lifts the 1 GB cap to 4 GB ("New replay max
  size"). `ReplayRecorder::~ReplayRecorder` is replaced by a bare `ret` (the ring is not freed at exit). [dll]
* **Transport**: `rewind`, `nextLap`, `previousLap`, `setNormalizedPosition` mark every car "jumped" (byte +0x2f8 of the
  per-car object), set a global to 2 and fire the replay-jump bus (0x182697840); `pause` loops instead of pausing when
  `LOOP_INSTEAD_OF_PAUSE`, and online / spectating it keeps the cursor on the live end (`ALLOW_REPLAYS_ONLINE`, instant
  replay); `startReplayMode` fires the start bus (0x182697830) and with `KEEP_CAMERA` (default 1) keeps the camera;
  `stopReplayMode` fires the stop bus (0x182697838) and truncates CSP's extension store; `getCurrentSunAngle` is
  recomputed from half floats (CSP keeps its own scene-frame buffer); `onLapCompleted` / `onStepCompleted` are guarded by
  a spin lock and, in `onStepCompleted`, fire bus 0x182697808 and fix the cursor online; `clearSaved` is handed to a
  background job ("Attempting to delete %d old replay(s)", "(skipping, too new)", age in hours). Clips (`CLIP_DURATION`
  s, `clips/` folder, "Saving clip, parallel thread / main thread") do **not** carry CSP data [ini][dll].
* **Lua**: `ac.onReplay(cb 'start'|'stop'|'jump')`, `ac.isInReplayMode`, `ac.getReplayFilename`,
  `ac.writeReplayBlob/readReplayBlob`, `ac.ReplayStream(layout, callback, frameDivisor)` (256 bytes per frame limit,
  car scripts 32), `ac.overrideReplayConditions`, `ac.tryToToggleReplay(active, rewindS)`, `ac.setReplayPosition`. [sdk]

### 1.6 Ghost car, lap database, comparative telemetry

* **Ghost recording at 1 ms** (`NEW_GHOST_RECORDING`, byte 0x268b15c, default 1): `GhostCarRecorder::onStepComplete`
  is wrapped; instead of the game's 129 ms frames only, it adds an interpolated frame exactly at the lap time (the
  body and 12 matrices are advanced by velocity x remainder) so the lap time of a ghost is exact. [dll]
* **Several ghosts** (`HAUNTED_HOUSE`, byte 0x268b15d, default 1; `HAUNTED_COMPATIBLE`): the file name function returns
  an empty name when CSP manages the files itself; `onLapCompleted` (C, 14,755 bytes: re-implemented) saves a few best
  laps with metadata; `loadGhostCarRecording` (C) checks "Track ID does not match", "Car ID does not match, failed to load
  new car", "Version %d is not supported", metadata keys `[base]`, `speedKmh`, `ambient`, `roadGrip`, `temperature`,
  `recordName`, `laptimeMs`, `autopilot`, `directionDeg`, `simulation`, `helpers`, `weather`, `trackLayout`, file
  suffix `.csp_ext` [dll 0x221c010..0x221c3a0]. User messages: "Car went off course, this lap won't be recorded",
  "Cancelling ghost record", "New ghost record". No ghost folder exists on this PC. [file]
* **Saved with the ghost**: `SAVE_PERF_METER` (0x268b15e), `SAVE_BEST_LAPTIME` (0x268b15f; `RaceTimingServices::onNewSession`
  copies the saved best lap and its splits back into the lap DB), `SAVE_TELEMETRY` (0x268b160) [dll].
* **Look**: `GhostCar::getGhostCarOpacity` replaced (distance fade, 1.0 with `USE_ORIGINAL_MATERIALS`),
  `assignGhostMaterial` (extra glow / refraction), `init3D` (a simplified model with `SIMPLIFY_GHOST_MODEL`, per-thread
  mode 1/2/4). Keys `[GHOST]`. Cross-reference `rendering_core.md`.
* **Comparative telemetry**: `ComparativeTelemetryRecorder::update` is wrapped; while it runs, `RaceManager::getLapCount`
  answers 1 (not 0) for car 0 once the transponder has data, so the first lap from a grid **is** recorded, and the
  recorded samples are cleared when the lap data is gone. `SAVE_TELEMETRY` keeps the best lap across sessions
  ("shown in main menu"). [dll][ini]

### 1.7 Pit stop and pit crew

* `PitStop::update` (wrap): while the stop is operating, the car's fuel is **raised gradually** over the fuelling
  part of the stop (`car+0x3db8` interpolated by the part's progress) instead of jumping at the end; the pit-crew
  indicator bytes (fuel / tyre / repair lamps of the pit HUD, three name compares) follow each part's progress.
  `commitChanges` / `commitQuickMenuChanges` first write the final fuel so the original sees the animated value.
  `getPitStopFuelingPercentage / RepairingPercentage / TyrePercentage` are **replaced**: progress of a part is
  `1 - clamp((offset + timer - (total - part)) / part)`, where `offset` is the start offset of the part set by the
  `Car::getPitstopTime` hook (server rule `PITS_ORDER`, `CHANGE_TYRES_PER_AXIS`; `physics_car_hooks.md` 0x5a0120).
  `isInsidePitArea` 1.5 m (above). [dll]
* `PitCrew`: the constructor reads the car's extension config `[PITCREW_TYRES] MODEL=` (custom tyre model of the crew;
  sets a loader flag during the original constructor); `onPitStopBegin` shifts each crew member's start time by the
  tyre part's offset, and for a per-axle change (`CHANGE_TYRES_PER_AXIS`) parks the members of the unchanged axle
  (start time set to a huge negative); `pitCrewUpdate` fires a cancelable bus (0x182697520) and hides those members.
  `LollipopCrew`: constructor reads `[PITCREW] MODEL=`, randomises scale (0.9..1.1) and yaw (+-0.2 rad); `update` runs
  at 0.8..1.2 x speed. [dll][interp] (no shipped file uses these keys)
* Quick pit menu (`QuickMenuPitstop` 6 hooks, `SetupPitstopPresets`) belongs to `ui_apps.md`; `ac.disableQuickMenuPitstop`,
  `ac.getPitstopSpinners`, `ac.setPitstopSpinnerValue`, `ac.setCurrentQuickPitPreset` are the Lua side [sdk].

### 1.8 New game modes (Lua, the `ac_new_modes` library)

Extension "New Modes" (log: "Extension `New Modes` disabled: not selected" on this PC [log-csp]) loads
`extension/lua/new-modes/<mode>/` when Content Manager writes `race.ini [RACE] __CM_CUSTOM_MODE`. The C++ host is
RVA 0xd25670..0xd37c67 (about 45 functions, 74 KB): manifest parser 0xd2fce0 (5,263 bytes), opponent / pit placement
0xd2d550 (5,010) and 0xd2e8f0, flag disabling 0xd2c4c0, loading screen override 0xd2c070 (`accLoadingOverride_ps.fxo`),
`ac.endSession` 0xd2b900, `ac.overrideSession` 0xd33880 (4,112), ghost 0xd335d5, settings 0xd31270, missing-script
dialog 0xd31170 / 0xd324d0 ("steam://validate/244210"). Lua size: 35 files, 438,369 bytes (drift-challenge 22 files /
221,676; p2p-1v1 5 / 102,998; revenant 48,149; rally 28,615; driver-interview 21,555; overtake 8,904; traffic 3,178;
splitscreen 2,779; walking-out 317; frozen-drive 198). The 12 SDK functions that exist **only** in this module [sdk,
computed by diffing `lib.lua` against 7 other modules]:

`ac.setStartMessage(message, icon)`, `ac.addMainMenuSettings(callback, settings, heightHint)`,
`ac.endSession(message, successfulRun, results{cancelled, place, summary, message})`, `ac.overrideSession{name, type,
spawn, order, laps, duration, timedRace, hasAdditionalLap, completedLaps, isOver, startTime, resetLaps, blockTeleports,
keepLaps}`, `ac.overrideSimTimestamp(t)`, `ac.overrideWeatherConditions(set)`, `ac.setReplayBasedGhost(target,
source, timeOffset, collisions 'disabled'|'stiff'|'pushable'|'knockable')`, `ac.disableCar(i, disable)`,
`physics.setUserControlledCar(i)` (the walking-out mechanism), and the script entry points `script.prepare(dt)`,
`script.update(dt)`, `script.drawUI()`. Flow: if the script calls `ac.setStartMessage` at load the mode enters a
preparation stage (`prepare()` per frame until it returns true) before `update()` runs.

| Mode (manifest NAME) | BASE_MODE | What it does | Needs |
|---|---|---|---|
| Drift Challenge (Neoned, 1.1) | PRACTICE, `START_TYPE=PIT`, `PENALTIES=0` | drift zones, judges, arcade scoring, per-track `.lua` zone files (ct_backwoods, drift, driftplayground, ebisu_minami, jarama_2009, ...), exported from the Drift Zones Editor | HUD (ImGui), scoring, local gates, `IMMEDIATE_START`, `DISABLE_FLAGS=YELLOW,BLUE`, `HIDE_PITCREW` |
| Driver Interview (1.0) | PRACTICE | recreation of a mission of Driver (1999) | `ALLOWED_CARS=buick gsx`, `ALLOWED_TRACKS=driver, acm_parking_interview`, forced weather / datetime / wind, six `[OPPONENTS] CAR_n` parked cars, timer |
| Frozen Drive (1.0) | HOTLAP, `PENALTIES=1` | ice everywhere, spiked tyre sets, winter | `FORCE_EXTENDED_PHYSICS=1`, `ICE_EVERYWHERE=1`, `SEASON_FORCE=WINTER`, `ALLOW_PHYSICS_ALTERATIONS=1` |
| Overtake (1.0) | TRACK_DAY, `SPEED_LIMIT=60`, `AI_LEVEL=80` | points for overtaking traffic | AI flood on, `ALLOWED_TRACKS=pits>1` |
| P2P 1v1 (0.4) | PRACTICE, `SESSION_AWARE=1`, `ALT_ONLINE_CARS=1` | two players over Steam peer-to-peer / relay | Steam API (`VERIFY_STEAM_API=1`), `libgns`, alt-online car API, chat; 100 KB of Lua |
| Rally Stage (0.1) | HOTLAP, `LAPS_COUNT=1`, `START_TYPE=START`, `JUMP_START_PENALTY=0` | single stage, copilot app | Surfaces FX, `FIX_SPAWN_POINTS=1`, `ASSOCIATED_APPS=RallyCopilot`, `FINISH_DELAY=1` |
| Revenant (0.6) | TRACK_DAY | record a lap, then a ghost with effects and **physical collisions** races you | `ac.setReplayBasedGhost`, `ALLOW_PHYSICS_ALTERATIONS=1`, `DISALLOW_AI_FLOOD=1`, `SAME_DRIVER_MODEL_ACROSS_CARS=1` |
| Splitscreen (1.0) | RACE | two local drivers, own controls preset for car 2 | `RENDER_MODE=__EXT_SPLITSCREEN`, `physics.setUserControlledCar`, two raw mice, `IMMEDIATE_START=1` |
| Traffic (1.0) | PRACTICE | thousands of simple traffic cars (`#traffic-planner-ready` tag, tool `csp-traffic-tool`) | physics API (`ALLOW_PHYSICS_ALTERATIONS=1`), track must allow scripts |
| Walking Out (1.0) | TRACK_DAY | leave the car and walk to another | `WALKING_OUT=1`, `IMMEDIATE_START=1`, driver model, `physics.setUserControlledCar` |
| `_p2p-1v1-dev` | | placeholder (description only) | |

Tools next to the modes: `lua/tools/csp-traffic-tool` (path / intersection editor + simulation, "2000 cars in less than
3 ms") and `csp-railworks-tool` [lua]. Session types the Lua enum knows: Undefined 0, Practice 1, Qualify 2, Race 3,
Hotlap 4, TimeAttack 5, Drift 6, Drag 7 [sdk].

## 2. Game parts it replaces or extends

All 158 rows of group `misc_ai_race`: **118 hooks, 40 lookups** (D). Replacement function = `decomp_hooks/<rva>.c`
(288,697 bytes of pseudo-C over 118 files, all read for this file except where marked). Slot RVAs are in
`hook_groups.tsv`. Note: `attach.md` section 2.2 lists `AIDriver 12/9`; the table says **19 hooks / 2 lookups**
(and `RaceEngineer` 11 / 2) -- the 12/9 split is a typing slip.

### 2.1 `AIDriver` (ksPhysicsAC.lib, 19 hooks, 2 lookups)

| Symbol | acs VA | CSP RVA | Cat | Change | Gate |
|---|---|---|---|---|---|
| `AIDriver::AIDriver` | 0x140298c60 | 0x107e590 | B | thread-local "current car" set; flags word (+0x136) loaded from global 0x268b93c; original; the driver's collision listener (last entry of the car's listener vector) removed; per-car CSP object linked (`ext+8 = driver`); CSP init 0x107e130 | always; flag 0x80 skips linking |
| `acquireControls` | 0x14029baa0 | 0x107f280 | B | see 1.1: stuck-timer pin, no-spline brake, pre/post events (buses 0x1826 95ee0 / 95ee8), CSP controls post-processor 0xecf0e0, `ACT_IMPATIENT`, always-on smoothing | `STOP_TELEPORTING_STUCK` (0x268ba36), `ACT_IMPATIENT` (0x26951c5) |
| `findTyreForQualify` | 0x14029c9f0 | 0x1085060 | B | result -1 -> preferred index (+0x2dc, min 0) | always |
| `generatePitlaneWait` | 0x14029cd20 | 0x107e740 | B | fires bus 0x182695ed8, original, then `updateTyres` if not yet done; clears +0xd0 for car 0 | always |
| `getLABraking` | 0x14029d650 | 0x1083ad0 | B | clamps the look-ahead target speed (+0x184) to the pit limiter speed (rules block +4, default 80 km/h) when the pit flag +0x28c is set, then to a CSP per-driver cap (+0xcc) | always |
| `getSteerAtDistance` | 0x14029de70 | 0x1085090 | B | corner-radius dependent shrink; temp-modifies +0x238 and the distance | `CAUTIOUS_STEERING` (0x26951c7) |
| `initSetup` | 0x14029e250 | 0x107e730 | A | 7-byte jump to the original | none |
| `onNewSession` | 0x14029f140 | 0x1084690 | B | original with car GUID forced to 0; afterwards CSP init 0x10842e0 for non-player cars without the ext flag 0x277 | always |
| `stepCarsAwareness2` | 0x14029f8d0 | 0x1083950 | B | car speed (+0x3e90) temporarily scaled; before the start dt x3 or x(1-0.99 m) | `START_STRAIGHTEN_TRAJECTORY` (0x268ba32) for the second part |
| `stepGasBrake` | 0x1402a08e0 | 0x1082f60 | B | level floor 0.8; `aiMult` low-pass on the 4 tyres (array 0x1825e77e0); gas request capped at +0x84; bit 0x4000 cleared | `FIX_LOW_LEVEL` (0x268ba30), `FIX_GRIP_MULT` (0x268ba39) |
| `stepGasBrakeStart` | 0x1402a1580 | 0x1083130 | B | pre-start throttle 0.4..0.5 with a pseudo-random reaction (hash of the driver pointer and the sim time) | `START_REDUCE_REDLINING` (0x268ba31) |
| `stepGears` | 0x1402a1710 | 0x1083310 | B | temp-edits upshift rpm (+0x74), downshift ratio (+0x78), car +0x170 = 1.0 around the original (scope guard restores) | `FIX_UPSHIFT_THRESHOLD` 0x268ba33, `FIX_DOWNSHIFT_THRESHOLD` 0x268ba34, `ADJUST_SHIFTING_FOR_SLOW` 0x268ba35 |
| `stepSetup` | 0x1402a2020 | 0x1084710 | B/C | stability gain (car +0x34f0) decays towards max of two wheel loads x0.995 and is clamped 0..2; original skipped when driver flag bit0 is set or the ext flag +0x277 is set (then +0x2ca4 = 1.0) | `REDUCE_STABILIZATION` (0x268ba3a); flags from `physics.setCarAutopilot` options |
| `stepSplineSelection` | 0x1402a2240 | 0x1082e30 | B | alt spline (see 1.1) via 0x1082c90; falls back to the recorded fast lane (global 0x1826a36f8) | `ALT_DRAG_SPLINE`, `TRAJECTORIES_FOR_DUMMIES`, `AVOID_SIDES` [interp] |
| `stepSteer` | 0x1402a2630 | 0x1084800 | B | look-ahead base (+0x20) x speed x 0.018 above 55.56 m/s; afterwards +0x230 scaled by byte +0x381 / 255 (always) | `REMOVE_STEER_LOOKAHEAD_SPEED_LIMIT` (0x26951c9) |
| `stepSuperhuman` | 0x1402a2a60 | 0x10847e0 | B | skipped when driver flag 0x80 | per driver |
| `stepTurbo` | 0x1402a30c0 | 0x10832f0 | B | skipped when driver flag 0x01 | per driver |
| `updateCurrentSplinePos` | 0x1402a3200 | 0x1082bf0 | B | with bit 0x200 (alt spline): pit-mode flag + saved position array 0x1826aa950 around the original | always |
| `updateTyres` | 0x1402a32f0 | 0x1084090 | **C** | see 1.1; sets +0x2e0; buses 0x182691f28, 0x182695ef0; helper 0x1083b30 sets the compound | always |
| lookups | `getCornerRadiusHint` 0x14029d0b0, `getLateralPush` 0x14029dd10 | - | D | CSP calls them (own AI code, flood) | |

The driver flags word (+0x136, global 0x268b93c) is written by a Lua binding at 0x525350 (2,764 bytes, string
`ignoreOthers`) -- the options table of `physics.setCarAutopilot` [interp]: bit0 keep settings (skip `stepSetup`,
`stepTurbo`), bit1 skip spline selection, bit7 (0x80) fully CSP-controlled (skip `stepSuperhuman`, no ext linking).

### 2.2 `AISpline`, `AISplineRecorder`, `SplineLocator`

| Symbol | acs VA | CSP RVA | Cat | Change |
|---|---|---|---|---|
| `AISpline::load` | 0x1402a8390 | 0x5864d0 | B | log "Loading AI spline", per-thread file-name slot, remembers fast / pit lane objects (0x1826a36f8 / 0x1826a3700) |
| `AISpline::loadFast` | 0x1402a83c0 | 0x585c20 | B | file-name slot; profiler tick |
| `AISpline::calculateNormals` | 0x1402a5e10 | 0x585d60 | C (when 0x268a8c9) | own 3-ray normals; skipped when payloads came from the cache |
| `AISpline::initStraights` | 0x1402a8130 | 0x586340 | B | log "Initialize straights for <file>" + timing |
| `AISpline::payloadAtPosition` | 0x1402a90a0 | 0x5855a0 | C (when 0x268a8c8) | CSP's payload interpolation (0x585610) |
| lookups | `AISpline::AISpline` 0x1402a4d70, `calculateRadius` 0x1402a6420 | | D | |
| `AISplineRecorder::AISplineRecorder` | 0x140293bf0 | 0x58a7f0 | B | clears +9, +10 |
| `::load`, `::loadHints` | 0x1402952c0, 0x140295670 | 0x58a790, 0x58a7c0 | B | skipped when the per-thread flag (+0x1aa) is set |
| `::startRecording` | 0x140296f00 | 0x58a810 | B | clears `CACHE_PAYLOADS` byte 0x26951c2 |
| `::save` | 0x140296ae0 | 0x58a820 | B | throws after saving when byte 0x182699588 is set |
| `::step` | 0x140296f60 | 0x58aa10 | B | binds car 0; sets +9 |
| `::~AISplineRecorder` | 0x140293d20 | 0x58a940 | B | profiler string |
| lookups | `beginPitLaneSpline` 0x140295020, `endPitLaneSpline` 0x140295110, `getMaxSpeedHintAtSplinePos` 0x140295270, `onLapCompleted` 0x140296760 | | D | Lua recorder API |
| `SplineLocator` lookups | `init` 0x1402ab2a0, `step` 0x1402ab5c0, `getSides` 0x1402ab130 | | D | CSP locates cars itself (flood, wrong way, DRS, `ac.getTrackAISplineSides`) [interp] |

### 2.3 `RaceEngineer` (11 hooks, 2 lookups; the AI's calculator, also used by `Car`, `AutoShifter`, `SpeedLimiter`)

| Symbol | acs VA | CSP RVA | Cat | Change |
|---|---|---|---|---|
| `evalLateralGFromLoads` | 0x14027a260 | 0x10ec0e0 | B/C | mode 0 (`NEW_LATERAL_GRIP_ESTIMATION=0`): original, with the rear axle track (+0x7c of a type-3 suspension) raised to `FIX_SHORT_AXLE_TRACK` 0.25 m during the call; modes 1, 2: **replaced** (per-wheel tyre force law, front/rear coupling by load share, braking term). Byte 0x268ba38, float 0x26951cc |
| `evalLateralGrip` | 0x14027a510 | 0x10ebff0 | B | x extension grip factor, x byte(+0x3e9a)/100 for AI-driven cars, x `1 + int8[car]/300` (rubber-banding array 0x1826ab910) |
| `getOptimalBrake` | 0x14027c320 | 0x10eb630 | C (when 0x2695097) | per-wheel brake torque curve sums (front pair averaged, rear pair averaged, divided by loads), then x extension brake factor |
| `getMaxSpeedFromGear` | 0x14027c240 | 0x10eb610 | C | CSP's drivetrain-aware version (0x10eb550) |
| `getCasterRAD` | 0x14027bb20 | 0x10eb880 | B | extended suspension type 5 returns CSP's value (deg x 0.01745) |
| `evalFrontRideHeight`, `evalRearRideHeight` | 0x14027a0f0, 0x14027a850 | 0x10ebb50, 0x10ebcf0 | C | same formula with a CSP factor; type-5 suspension through 0x1805972b0 |
| `getPointGroundHeight` | 0x14027c550 | 0x10ebeb0 | B | plane from the extension object (+0x424..0x430) when present |
| `findTyreCompound` | 0x14027b3a0 | 0x10ec5e0 | B | non-player car and result < 0 -> logs and answers 0 |
| `projectWingsDrag`, `projectWingsLift` | 0x14027ccf0, 0x14027cdc0 | 0x10ec740, 0x10ec800 | B | adds the extension wings (list at ext+0x180, area at +0x48) |
| lookups | `RaceEngineer::RaceEngineer` 0x140279bf0, `getWeightDistribution` 0x14027cb90 | | D | |

### 2.4 `RaceManager` (18 hooks, 17 lookups in this group)

| Symbol | acs VA | CSP RVA | Cat | Change |
|---|---|---|---|---|
| `RaceManager` ctor | 0x140132db0 | 0x10ecaf0 | A | stores the manager (global 0x1826aba20), registers CSP listeners (0x10eeff0) |
| `update` | 0x140141550 | 0x10ed3a0 | B | the "session paused" byte (+0xa3) is cleared during the original when spectating (0x182697c0f) |
| `startRace` | 0x140141470 | 0x10ed3f0 | B | custom-start guard (0x1826a9b10 + time compare): clears +0xe0 instead of starting |
| `updateCarsEndRace` | 0x140142650 | 0x10ed420 | B | end-of-race pass with a float threshold (0x1826fe854) forced to FLT_MAX and byte 0x1826a96dd = 1 |
| `isRaceOver` | 0x14013eae0 | 0x10ed690 | B | fires bus 0x1826973c8 with the result (listeners may override) |
| `restartCurrentSession` | 0x140140d00 | 0x10ed490 | B | fires bus 0x1826973c0; refuses with a message when `DISABLE_SESSION_RESTART` |
| `setCurrentSession` | 0x140140df0 | 0x10ed720 | B | bus 0x1826973d0 (new index, same-index flag); afterwards every car's +0x48 field reset to FLT_MAX |
| `initOffline` | 0x14013a6c0 | 0x10ed7c0 | B | sets "offline" 0x268b412, `ALLOWED_TYRES_OUT` from race.ini, new-mode init 0x31b990 when 0x2695041 |
| `initOnline`, `initReplay` | 0x14013d160, 0x14013e820 | 0x10edae0, 0x10edbd0 | B | set "online" 0x268b39f / "replay" 0x268b39e; run CSP session setup (0x10efb40) |
| `onLapCompleted` | 0x14013f380 | 0x10eec10 | B | worn-tyre black flag; bus 0x1826973d8 |
| `renderHUD` | 0x14013f960 | 0x10edc10 | B | cancelable bus 0x1826973e0 (new modes hide AC's HUD) |
| `getLapCount` | 0x140138570 | 0x10ecb60 | B | car 0 answers `DAT_1826a96dc - 1` while comparative telemetry runs |
| `getLeaderLapCount` | 0x140138a00 | 0x10ee960 | C | memoised per frame |
| `getLeaderboard` | 0x140138ab0 | 0x10ed2f0 | B | CSP-provided order when byte 0x268a7aa (`ac.setRaceScore`) |
| `getRaceText` | 0x140139150 | 0x10ee9a0 | B/C | "%d / %d" text for offline, last-lap text suppression (`FINAL_LAP_TWEAK` 3) |
| `getCarRealTimePosition` | 0x140137f90 | 0x10ed340 | B | during replay: position from the recorded frame (+0x548 +0x40, minus 1) |
| `resetInvalidStateCarsToPits` | 0x140140340 | 0x10eedd0 | C | see 1.4 |
| lookups (17) | `finalizeCurrentSession`, `getBestLap` x2 (0x14013 7c00), `getCarLeaderboardPosition`, `getCurrentLap`, `getCurrentSession`, `getCurrentSessionType`, `getHasCompletedFlag`, `getInvertedGridPositions`, `getLastLap`, `getSessionCount`, `getSessionInfo`, `getSessionTimeLeft`, `getTimetoWait`, `hasSessionStarted`, `resetMandatoryPit`, `skipCurrentSession` | | D | `data_alt_mapping.ini` also remaps `RaceManager::getBestLap` field offset 0x27C -> 0x270 |

### 2.5 Flags, timing, results, output

| Symbol | acs VA | CSP RVA | Cat | Change |
|---|---|---|---|---|
| `FlagManager::FlagManager` | 0x14009ab60 | 0x5cf740 | A | stores `this` (0x1826a49c0), CSP init 0x5cfa40 |
| `FlagManager::update` | 0x14009d140 | 0x5cf590 | B | white flag (state 5), early chequered, flag drawing 0x5cf290; bus 0x182690bd8 |
| `checkForYellow`, `checkForBlue` | 0x14009c550, 0x14009c390 | 0x5cf1f0, 0x5cf240 | B | skipped by `DISABLE_FLAGS`; run at most every 310 / 315 ms |
| `checkforPenalty` | 0x14009c9b0 | 0x5cf180 | A | sets "penalties checked" (0x1826997ee), bus 0x182690bd0 |
| `setBlueFlag` | 0x14009ce30 | 0x5cf780 | A | stores the argument (0x1825e1570) |
| `RaceTimingServices` ctor | 0x140143c60 | 0x5e64c0 | B | CSP init 0x5e62f0 |
| `RaceTimingServices::onNewSession` | 0x140145870 | 0x5e64a0 | B | `SAVE_BEST_LAPTIME`: copies saved best lap + splits into the lap DB entry of car 0 |
| `RaceTimingServices::onSectorSplit` | 0x140145940 | 0x428ce0 | A | fires bus 0x18268cbe8 before the original |
| lookups | `getLapDB` 0x140144dc0, `resetCurrentLaps` 0x140145d30 | | D | |
| `SessionResultProcessor::getCompleteAIResults` | 0x140166c70 | 0x69d800 | B | AI lap field zeroed in new modes / track day |
| `JsonOutputFile::writeOutputFile` | 0x14010ecc0 | 0x5ca090 | B | post-processing of `out\race_out.json` |
| `TimeAttack::TimeAttack` | 0x1401b7e20 | 0x6a5980 | B | callback wrapper |
| `DragMode::DragMode` | 0x140084a20 | 0x5b72d0 | B | `AC_OPEN_FINISH_L` check |
| `INIReader::INIReader` (3rd overload) | 0x1400bd740 (object `GameMode.obj`) | 0x5ecc40 | B | settings-file redirect, belongs to the config brief |

### 2.6 Replay (`ReplayManager` 16 hooks + 2 lookups, `ReplayRecorder` 5 hooks + 2 lookups)

| Symbol | acs VA | CSP RVA | Cat | Change |
|---|---|---|---|---|
| `ReplayManager::ReplayManager` | 0x140153130 | 0x11a5d60 | B | scope label, stores manager (0x1826adb40), CSP registration 0x11b00d0 |
| `::load` | 0x140157680 | 0x11a8f70 | **C** | own loader + extension reader (1.5) |
| `::save` | 0x140159d10 | 0x11a91e0 | B | logging, name bus 0x182697810, extension writer 0x11a4750 afterwards |
| `::resetReplay` | 0x1401597d0 | 0x11a99f0 | B | lazily creates the 0xd0-byte extension store (0x182705400) |
| `::startReplayMode`, `::stopReplayMode` | 0x14015b480, 0x14015b800 | 0x11a9a10, 0x11a9c00 | B | buses 0x182697830 / 0x182697838; `KEEP_CAMERA`; store truncation |
| `::onStepCompleted` | 0x140158b50 | 0x11aa7c0 | B | lock, bus 0x182697808, `LIMIT_TRACK_OBJECTS_IN_REPLAYS`, online cursor |
| `::onLapCompleted` | 0x140158a00 | 0x11aa740 | B | spin lock only |
| `::rewind`, `nextLap`, `previousLap`, `setNormalizedPosition` | 0x140159b40, 0x140158930, 0x1401592a0, 0x14015b0b0 | 0x11a1d50, 0x11a1d80, 0x11a1da0, 0x11a1d30 | B | "jumped" marks + bus 0x182697840 |
| `::pause`, `::updatePlayState` | 0x140159070, 0x14015b9a0 | 0x11a21e0, 0x11a2240 | B | loop / live-end handling |
| `::getCurrentSunAngle` | 0x140156d00 | 0x11a1dc0 | C | half-float decode of CSP's scene buffer |
| `::clearSaved` | 0x1401559f0 | 0x11ac420 | C | background deletion |
| `ReplayRecorder::recordFrame` | 0x1401609d0 | 0x11aa1b0 | B | see 1.5 |
| `::getPhysicsState` | 0x14015d850 | 0x11a9cb0 | B | `JUMP_FIX2` |
| `::save` | 0x1402 6.. (0x1401622a0) | 0x11a1550 | B | log, "recorded frames = 1" fix |
| `::setFrameSize` | 0x140162420 | 0x11a1510 | C | lazy ring |
| `::~ReplayRecorder` | 0x14015ce20 | 0x2db1f0 | C | empty stub (shared by 40 symbols) |
| lookups | `ReplayManager::play` 0x1401591f0, `updateReplayAudio` 0x14015bc00, `ReplayRecorder::getWingState` 0x14015ff00, `load` 0x140160220 | | D | |

### 2.7 Ghost, telemetry, pit

| Symbol | acs VA | CSP RVA | Cat |
|---|---|---|---|
| `GhostCar::GhostCar` | 0x140104fe0 | 0x5deae0 | B (stores `this` 0x182690c00, material flags 0x0c000000) |
| `GhostCar::getGhostCarOpacity` | 0x140105480 | 0x5deb20 | C |
| `GhostCar::assignGhostMaterial` | 0x1401053a0 | 0x5dec70 | B |
| `GhostCar::init3D` | 0x140105520 | 0x5df1b0 | B |
| `GhostCar::update` | 0x140106380 | 0x5df760 | B (7,368 bytes; 1 ms playback) [interp] |
| `GhostCarRecorder::getGhostCarFileName` | 0x140107d20 | 0x5e11e0 | C |
| `GhostCarRecorder::loadGhostCarRecording` | 0x140108c10 | 0x5e7580 | C |
| `GhostCarRecorder::onLapCompleted` | 0x1401091e0 | 0x5e4800 | C |
| `GhostCarRecorder::onStepComplete` | 0x1401093f0 | 0x5e15c0 | B |
| `ComparativeTelemetryRecorder::update` | 0x140076850 | 0x5ce890 | B (lookup `loadCTOpponent` 0x140075730 D) |
| `PitStop::PitStop` | 0x1400a55d0 | 0x68ff00 | B (helper 0x68ff30 logs "Pit stop and crew", `PIT_STOPS_SHADER_REPLACEMENT`) |
| `PitStop::update` | 0x1400ab190 | 0x690390 | B |
| `PitStop::commitChanges`, `commitQuickMenuChanges` | 0x1400a93e0, 0x1400a94b0 | 0x690750, 0x6907d0 | B |
| `PitStop::getPitStopFuelingPercentage`, `...RepairingPercentage`, `...TyrePercentage` | 0x1400a9730, 0x1400a9780, 0x1400a3070 | 0x6900d0, 0x690120, 0x690170 | C |
| `PitStop::isInsidePitArea` | 0x1400a9af0 | 0x690850 | C (when 0x268bcb2) |
| `PitCrew::PitCrew`, `onPitStopBegin`, `pitCrewUpdate` | 0x1400a0b80, 0x1400a33e0, 0x1400a3f20 | 0x11324d0, 0x1132b50, 0x1132a10 | B |
| `LollipopCrew::LollipopCrew`, `update` | 0x140112bf0, 0x140113ab0 | 0x622880, 0x6233a0 | B |

### 2.8 Non-hook CSP code that belongs to the area (RVAs, none decompiled yet)

New-behaviour module 0x1246180..0x124f0fe: config loaders 0x124a5e0 (AI_TWEAKS group, 4,470 bytes) and 0x124b760
(flood / cooldown / retirement / wrong way / reset, 4,939), cooldown 0x1246f20 / 0x124d480, retirement 0x1247180,
reset car 0x124a0d0, rubber-banding apply 0x124a390 (writes 0x1826ab910) and setter 0x124df80, traffic spline
0x12489a0, 0x12485f0, event registration 0x124cab0 (called from orchestrator stage 0x14dc880; it subscribes to
`RaceManager` buses through 0x10ec9b0 / 0x10ef350 / 0x10ef600). AI helpers 0x107e130, 0x107e370, 0x107e800 (event
call), 0x107ef50 (find blocker), 0x107efd0 (impatient), 0x1082370 / 0x1082c90 / 0x1082db0 (alt spline), 0x10842e0,
0x1083b30 (set compound). Replay extension 0x11a0000..0x11b2700. New modes 0xd25670..0xd37c67.

## 3. Config keys and Lua API

### 3.1 `extension/config/new_behaviour.ini` (78 key lines, 13 sections; owner overrides: `[AI_RACE_COOLDOWN] ENABLED=1, LIMITER=0.67`)

Defaults as shipped; "->" = byte that the hooks read.

| Section | Keys (default) |
|---|---|
| `[BASIC]` | `ENABLED=1` |
| `[AI_TWEAKS]` (17) | `NEW_LATERAL_GRIP_ESTIMATION=1` (0 off, 1 braking for three-wheelers only, 2 braking for all) -> 0x268ba38; `FIX_GRIP_MULT=1` -> 0x268ba39; `FIX_SHORT_AXLE=1`, `FIX_SHORT_AXLE_TRACK=0.25` (hidden) -> float 0x26951cc; `FIX_LOW_LEVEL=1` -> 0x268ba30; `START_REDUCE_REDLINING=1` -> 0x268ba31; `START_STRAIGHTEN_TRAJECTORY=1` -> 0x268ba32; `ACT_IMPATIENT=1` -> 0x26951c5; `TRAJECTORIES_FOR_DUMMIES=1` -> 0x26951c6; `CAUTIOUS_STEERING=1` -> 0x26951c7; `REMOVE_STEER_LOOKAHEAD_SPEED_LIMIT=1` -> 0x26951c9; `FIX_UPSHIFT_THRESHOLD=1` -> 0x268ba33; `FIX_DOWNSHIFT_THRESHOLD=0` -> 0x268ba34; `ADJUST_SHIFTING_FOR_SLOW=1` (0 off, 1 track day only, 2 always) -> 0x268ba35; `STOP_TELEPORTING_STUCK=0` -> 0x268ba36; `NEW_AI_SMOOTHING=1` -> 0x26951ca; `REDUCE_STABILIZATION=0` -> 0x268ba3a. With `[BASIC] ENABLED=0` the loader (0x138ded0) zeroes only `REMOVE_STEER...`, `START_REDUCE_REDLINING`, `ADJUST_SHIFTING_FOR_SLOW`, `STOP_TELEPORTING_STUCK`; the others keep their `.data` default 0 [dll][interp]. |
| `[RAIN]` | `AVOID_SIDES=1` -> 0x26951c3, `AVOID_PUDDLES=0` -> 0x26951c4 |
| `[AI_SPLINES]` | `ALT_GRID=1` -> 0x26951c1, `CACHE_PAYLOADS=1` -> 0x26951c2, `ALT_DRAG_SPLINE=1` -> 0x26951c8, `SAFE_NORMALS_RAYCASTING=0` -> 0x268a8cb, `EXPERIMENTAL_OPTIMIZATIONS=FIX_WRAP_INDEX, OPTIMIZED_LENGTH_COMPUTE, OPTIMIZED_FILTERING, INDEX_SEARCH, INDEX_SEARCH_CACHE, CUSTOM_PAYLOAD_COMPUTE, CUSTOM_NORMALS_COMPUTE, IMMEDIATE_NORMALS_COMPUTE` (hidden; bytes 0x268a8c8..0x268a8cc), `LINEAR_METADATA_FOR_LARGE_SPLINES=1` (hidden), `FIX_REVERSED=1` -> 0x268b1f5; master byte 0x26951c0 = `[BASIC] ENABLED` of this block [dll] |
| `[WRONG_WAY]` | `ALLOW_IN_PRACTICE=1`, `ALLOW_IN_TRACKDAY=1`, hidden `ALLOW_EVERYWHERE` |
| `[RESET_CAR]` (hidden) | `RESET_DISTANCE=30`, `STEPBACK_DISTANCE=300` |
| `[AI_FLOOD]` (17) | `ENABLED=1`, `DEBUG_MODE=0`, `MIN_TRACK_LENGTH=7000`, `HIDE_DRIVER_NAMES=1`, `PREFERRED_TYRES=Street, Street?, Semislicks, HR, Slick Hard`, `PUSH_FORCE=100000`, `PUSH_SPEED=80`, `MIN_DISTANCE_TO_AI=15`, `X_OFFSET_MULT=0`, `Y_OFFSET=0`, `SHUFFLE_BEHAVIOUR=AUTO` (ALWAYS/AUTO/NEVER), `SPEED_LIMIT=80, 160`, `SHUFFLE_COLORS=1`, `SPAWN_MIN_DISTANCE_TO_PLAYER=400, 200`, `SPAWN_DISTANCE=600, 300`, `SPAWN_RANGE=200, 100`, `SPAWN_DESPAWN=1000, 400` (pairs are "at no / at full speed or density" [interp]) |
| `[AI_RACE_COOLDOWN]` | `ENABLED=0`, `TIME_LIMIT_MIN=30`, `LIMITER=0.2`, `SPEED_LIMIT=120` |
| `[AI_RACE_RETIREMENT]` | `REJOIN=0`, `REJOIN_MESSAGE=0`, `PIT_WITH_DAMAGE=1`, `TELEPORT_WITH_DAMAGE=1` |
| `[AI_RACE_RUBBERBANDING]` | `ENABLED=0`, `CHANGE_TIME=60`, `CHANGE_DISTANCE=200`, `DECREASE_AHEAD=0.03`, `INCREASE_BEHIND=0.1` |
| `[AI_RACE_ADAPTATION]` | `ENABLED=0` |
| `[TRACK_<name>]` | `WRONG_WAY_ALLOW_IN_TRACKDAY`, `AI_FLOOD_X_OFFSET_MULT`, `AI_FLOOD_PUSH_FORCE`, `AI_FLOOD_MIN_DISTANCE_TO_PLAYER` (shipped for `simtraxx_transfagarasan_cl`) |
| `[CUSTOM_AI]` (hidden) | `ENABLED=0`, `DISABLE_AUDIO`, `DISABLE_PYTHON_APPS`, `DISABLE_TEXTURES` |

### 3.2 Other files

| File / section / key (default) | Effect (reader) |
|---|---|
| `general.ini [CONTROL] ALTERNATIVE_AI_DRIVER=1` -> 0x182695228 | autopilot toggling: `Sim::onKeyDown` -> 0x11be520 ("Activated"/"Deactivated", "Autopilot is not available in this session") |
| `[CONTROL] AI_DRIVER_KEEP_SETTINGS=1` -> 0x182695229 | "stop autopilot from altering car settings"; read by 0xff5c20 (`$SmallTweaks.AIToggle`), 0xeeb890 |
| `[PHYSICS_EXPERIMENTS] ARM_FIRST_LAP=1` (hidden) -> 0x268b682 | section 1.4 |
| `[PHYSICS_EXPERIMENTS] PRECISE_TRANSPONDER=1` -> 0x268b680; `EQUALIZE_PITS_CHECK=1` -> 0x268bcb2 | |
| `[PHYSICS_EXPERIMENTS_DONE] ALTERNATIVE_OPTIMAL_BRAKE=1` (hidden) -> 0x2695097 | `getOptimalBrake` |
| `[LAP_TIMES] KEEP_AI_LAPS=0`, `KEEP_ALTERED_LAPS=0` | "keep laps set with autopilot / with altered physics (affects ghost)"; **the key strings are not in the image (hashed)**, enforcement site not found |
| `[REPLAY]` (12) | `SPEED_UP_REPLAY_ACTIVATION` -> 0x1825a2e21, `EXTRA_LARGE` -> 0x1825a2e22 (4 GB), `LOOP_INSTEAD_OF_PAUSE=0` -> ..e23, `JUMP_FIX2=1` -> ..e24, `ACCOUNT_FOR_PING=1` -> ..e25, `KEEP_CAMERA=1` -> ..e26, `CLIP_DURATION=30` s, `ALLOW_REPLAYS_ONLINE=1`, hidden `EXT_RECORD`, `EXT_SAVE`, `EXT_LOAD` (not in the image: hashed) |
| `[GHOST_RECORDER]` (5) | `HAUNTED_HOUSE=1` -> 0x268b15d, `HAUNTED_COMPATIBLE=1`, `SAVE_PERF_METER=3` -> ..15e, `SAVE_BEST_LAPTIME=3` -> ..15f, `SAVE_TELEMETRY=1` -> ..160 |
| `[GHOST]` (7) | `USE_ORIGINAL_MATERIALS=0` -> 0x268b158, `SIMPLIFY_GHOST_MODEL=1`, `ADD_EXTRA_GLOW=0`, `EXTRA_GLOW_EMISSIVE_MULT=1`, `EXTRA_GLOW_LIGHT_MULT=1`, `REFRACTION_MULT=0`, `CAST_SHADOWS=0` |
| `[OTHER_EXPERIMENTS] NEW_GHOST_RECORDING=1` -> 0x268b15c; `ADAPTIVE_PERF_METER=1` | |
| `[OPTIMIZATIONS_MEMORY] GRADUAL_REPLAYS_COMMIT=0`; `[OPTIMIZATIONS] VIRTUAL_REPLAYS_ALLOCATION2` (commented) | replay ring |
| `small_tweaks.ini [RULES] DISABLE_TELEPORT_TO_PITS=0`, `DISABLE_SESSION_RESTART=0`; `[MISCELLANEOUS] LIMIT_TRACK_OBJECTS_IN_REPLAYS=0` -> 0x1825a2e27 | |
| `gui.ini [TWEAKS] FINAL_LAP_TWEAK=0` (0 / 1 / 3), `SHOW_CHEQUERED_FLAG_EARLY=0`, `HIDE_RACE_FLAGS=0` | -> 0x268adca / 0x268c230 / 0x268c232 / 0x268c231 |
| `race.ini [LAP_INVALIDATOR] ALLOWED_TYRES_OUT` | section 1.4 |
| `spectating.ini` | only `[BASIC] ENABLED=0` here (module off); its keys belong to the camera / online briefs |
| car `ext_config.ini [PITCREW_TYRES] MODEL=`, `[PITCREW] MODEL=` | per-car crew models [dll][interp] |
| new-mode `manifest.ini` | `[ABOUT] NAME AUTHOR VERSION DESCRIPTION`; `[CORE] SESSION_AWARE`; `[RULES] BASE_MODE (PRACTICE HOTLAP TRACK_DAY RACE) PENALTIES JUMP_START_PENALTY LAPS_COUNT START_TYPE (PIT START) SPEED_LIMIT AI_LEVEL AI_LIMIT ALLOWED_CARS ALLOWED_TRACKS (names, #tags, pits>N, @label) FORCE_WEATHER FORCE_DATETIME FORCE_WIND ALLOW_PHYSICS_ALTERATIONS`; `[DISPLAY] OPPONENT_NOUN`; `[TWEAKS] LOADING_BACKGROUND LOADING_PROGRESS IMMEDIATE_START DISABLE_FLAGS HIDE_PITCREW FORCE_EXTENDED_PHYSICS ICE_EVERYWHERE SEASON_FORCE FIX_SPAWN_POINTS ASSOCIATED_APPS FINISH_DELAY DISALLOW_AI_FLOOD SAME_DRIVER_MODEL_ACROSS_CARS RENDER_MODE WALKING_OUT VERIFY_STEAM_API ALT_ONLINE_CARS ALT_ONLINE_CAR_ID ALT_ONLINE_CHAT DYNAMIC_TRACK_OBJECTS_SEED PARAM_TEST HIDE_OBJECT_<name>`; `[OPPONENTS] CAR_n=<model>[, <skin>]`, `CAR_n_PITS=`; `[MIXIN: name]` with `@ACTIVE=$"expr"` and `@ = name`. `settings.ini [SETTINGS]` per mode, `description.txt`. [lua][dll] |
| server `[EXTRA_RULES]` keys that act on this area | `NO_BACK_TO_PITS*`, `ENFORCE_BACK_TO_PITS_PENALTY`, `MAX_METERS_WRONG_WAY`, `INVALIDATE_LAP_TIME_IN_PITS`, `PITS_ORDER`, `CHANGE_TYRES_PER_AXIS` (see `online.md`, `physics_car_hooks.md`) |

### 3.3 Lua API groups for this area (counts from `lib.lua`, `[sdk]`)

| Group | Functions | Where |
|---|---|---|
| New-mode only | 12 (listed in 1.8) | `ac_new_modes` |
| Session / lap / reset | `ac.getSession`, `getSessionName`, `getSessionSpawnSet`, `onSessionStart`, `onLapCompleted`, `onCarJumped`, `evaluateLapTime`, `exportLapTimesToCSV`, `tryToRestartSession`, `tryToSkipSession`, `tryToTeleportToPits`, `isCarResetAllowed`, `resetCar`, `markLapAsSpoiled`, `setRaceScore`, `lapTimeToString` = 16 | apps, tools, new modes, online |
| Replay | 9 (1.5) | apps, new modes, ... |
| AI recorder | 6 (1.2) | apps ... |
| Physics rules | `physics.setCarAutopilot(active, chooseTyres)`, `setTyresAIMultiplier(car, wheels, mult)`, `setPenalties(bool)`, `setWrongWayPenalty(bool)`, `setCarPenalty(type, param)` (ac.PenaltyType 0..5 None, MandatoryPits, TeleportToPits, SlowDown, BlackFlag, ReleaseBlackFlag), `markLapAsSpoiled(car)`, `overrideRacingFlag(flag)` (track scripts) = 7 | physics-enabled modules |
| Car physics scripts, AI | `ac.setAILookaheadBase`, `setAILookaheadGasBrake`, `setAIBrakeHint`, `setExtraAIGrip` (120 % default), `setAIPitStopRequest`, `setAIBrakeMistake(time)`, `setAITractionMistake(time, severity)` = 7 | `ac_car_cphys` |
| Flags | `ac.FlagType` 17 values (None, Start, Caution, Slippery, PitLaneClosed, Stop, SlowVehicle, Ambulance, ReturnToPits, MechanicalFailure, Unsportsmanlike, StopCancel, FasterCar, Finished, OneLapLeft = white, SessionSuspended, Code60) | flag textures `?textures/flags/*.png` and `content/gui/flags/whiteFlag.png` [dll 0x130e000] |
| Pits UI | `ac.disableQuickMenuPitstop`, `getPitstopSpinners`, `setPitstopSpinnerValue`, `setCurrentQuickPitPreset` | |

## 4. Files it reads

* `extension/config/new_behaviour.ini` (+ owner `cfg/extension/new_behaviour.ini`), `general.ini`, `gui.ini`, `small_tweaks.ini`
  (key by key, section 3).
* Per track: `content/tracks/<t>[/<layout>]/ai/fast_lane.ai`, `ai/pit_lane.ai`, `data/ai_hints.ini`,
  `data/side_l.csv`, `side_r.csv` (as AC); CSP caches in `<AC>\cache\ai_grids\<track>_<hash>.bin` and
  `<AC>\cache\ai_payloads\<track>__<hash>.bin` (40 files on this PC). [file]
* Per car: `data/ai.ini`, `ai_tyres.ini`, `setup.ini` (as AC); `extension/ext_config.ini` `[PITCREW_TYRES]`, `[PITCREW]`.
* `race.ini` (all sections; `[LAP_INVALIDATOR] ALLOWED_TYRES_OUT`, `[SESSION_0] SPAWN_SET/TYPE`, `[GHOST_CAR]`, `__CM_CUSTOM_MODE`),
  `replay.ini` (`MAX_SIZE_MB`, rate), `system/cfg/pitstop.ini`, `ghost_car.ini`.
* Writes: `Documents\Assetto Corsa\replay\temp\AC_<ddmmyy>-<hhmmss>_<O|Q|R>_<car>_<track>_.acreplay` (own format above),
  `replay\clips\`, `out\race_out.json` (rewritten), `GhostCar\<track>\*.ghost` + `.csp_ext`, `ctelemetry\player\`,
  `cfg\extension\state\lua\new_modes\<mode>.ini` [interp], the cache folders above.
* Network / IPC: `\\.\pipe\AcTools.CSP.NewBehaviour.CustomAI.Helpers`.

## 5. How it is switched on

* **AI tweaks, line caches, cooldown**: `new_behaviour.ini [BASIC] ENABLED=1` (default) and the per-key bytes of 3.1;
  offline only ("won't affect online" [ini]); there is no per-car gate except the driver flags of 2.1. On this PC all
  defaults are active, plus `AI_RACE_COOLDOWN ENABLED=1, LIMITER=0.67` [file].
* **Always on without a key**: the AI acquire-controls smoothing, `PitStop` fuel animation and 1.5 m box test (key
  `EQUALIZE_PITS_CHECK`, default 1), replay loader replacement, `RaceManager` events, ring growth, `~ReplayRecorder` stub.
* **Replay extension**: written for every saved replay including the autosave in `replay\temp` (9 of 9 files here);
  not for clips. `[REPLAY] EXT_RECORD / EXT_SAVE / EXT_LOAD` are commented hidden switches [ini].
* **New modes**: `race.ini [RACE] __CM_CUSTOM_MODE` (Content Manager); off here ("not selected" [log-csp]). `Walking Out`
  needs its mode ("not available in this mode" [log-csp]).
* **White flag**: `FINAL_LAP_TWEAK` 1 or 3; **early chequered**: `SHOW_CHEQUERED_FLAG_EARLY=1`; both default off.
* **Online**: the AI changes do nothing (no AI online); replay recording online is allowed by `ALLOW_REPLAYS_ONLINE`
  ("doesn't work with extra data yet" [ini]); server rules in `online.md`.

## 6. Size

| Item | Number |
|---|---|
| Rows in the group / hooks / lookups | 158 / 118 / 40 (AIDriver 19+2, AISpline 5+2, AISplineRecorder 7+4, RaceEngineer 11+2, SplineLocator 0+3, RaceManager 18+17 (+2 data lookups in `misc_core`), RaceTimingServices 3+2, ReplayManager 16+2, ReplayRecorder 5+2, GhostCar 5, GhostCarRecorder 4, ComparativeTelemetryRecorder 1+1, FlagManager 6, PitStop 8, PitCrew 3, LollipopCrew 2, SessionResultProcessor 1, JsonOutputFile 1, TimeAttack 1, DragMode 1, INIReader 1+2, ksgui::ListBoxRowData 0+1) |
| Pseudo-C of the 118 replacement functions | 288,697 bytes |
| Code of the area | AI block RVA 0x107e000..0x1086000 (about 32 KB); new-behaviour 0x1246180..0x124f0fe (36 KB, about 45 functions); replay extension 0x11a0000..0x11b2700 (74 KB, 12 decompress / compress variants); new-modes host 0xd25670..0xd37c67 (74 KB) |
| Config | `new_behaviour.ini` 78 key lines / 13 sections; about 33 keys in `general.ini`, `gui.ini`, `small_tweaks.ini` touch this area |
| Lua | 12 mode-only SDK functions, about 60 more used by modes; 35 mode `.lua` files, 438,369 bytes; 103 tool `.lua` files, 523,047 bytes |
| Replay extension on this PC | 9 files, ext block 74,503 .. 880,183 bytes each (0.3 .. 4 % of the file) |
| Payload cache | 40 `ai_payloads` + `ai_grids` files; 84 bytes per point |

## 7. Port difficulty for rustyAC

What rustyAC has today (`docs/port/ai.md`, `race_rules.md`, `pit_stops.md`, `replays.md`): the game's own AI, referee,
pit stop, crew, replay (v16), ghost and comparative telemetry, **bit-exact against plain `acs.exe`** in the oracle
(`car_oracle`, which maps `acs.exe` alone; CSP's code cannot run there). Every CSP change below is therefore a *delta
on a proven base*; the delta itself has no oracle.

| Part | Size | Reuse / what is hard | How to prove |
|---|---|---|---|
| AI tweaks, section 1.1 (19 + 11 hooks) | **M** | each is 5..60 lines of arithmetic around functions the port already has (`ai/driver.rs`, `engineer.rs`); keys are bytes | **Statistical** against a CSP-on recording, **bit-exact** only if the AI's `rand()` stream is controllable (see 8.1). Unit level: feed identical driver states through plain and tweaked paths and check the documented formulae (stepGears thresholds, aiMult low-pass, evalLateralGFromLoads mode 2) |
| Alt splines, rain/`AVOID_SIDES`, `ALT_DRAG_SPLINE` | **M-L** | needs the undecompiled 0x1082c90 family; produces new `AISpline` objects | compare AI paths (positions vs `trackPos`, speed) with a CSP-on recording on a drag-start spawn |
| Payload computation (`CUSTOM_*`), caches | **S-M** | rustyAC already computes payloads bit-exact vs plain AC (`track/ai_line.rs`); CSP's cache holds the result | **bit-exact check available now**: inflate `<AC>\cache\ai_payloads\*.bin`, compare each 84-byte payload with rustyAC's payload for the same `.ai` (tells whether CSP's custom compute equals AC's) |
| New-behaviour module (flood, cooldown, retirement, rubber-banding, reset car, wrong way) | **L** | not in the original at all; needs event hooks in the engine (`step_cars`), spawn logic, messages; flood is behaviour, not physics | behaviour tests only (spawn distances, retire reasons, speed cap = `LIMITER`, `SPEED_LIMIT`); measured on recordings |
| White flag / early chequered / `ALLOWED_TYRES_OUT` / restart refusal / 1.5 m box / `ARM_FIRST_LAP` | **S** each | `race/flags.rs` already has the White enum value ("nothing ever sets it"); `referee.rs` glue | **bit-exact** by state: shared-memory `flag` field and lap validity of a real CSP-on race vs the port replayed on the same inputs |
| `RaceManager` events / `renderHUD` cancel / `getLeaderLapCount` memo | **S** | only matters with Lua | none needed |
| Replay extension: **read** | **S** | container is flat and verified; zlib inflates (`miniz_oxide`) | **bit-exact**: parse the 9 owner files, check every entry length and the identities frames x 56 / 16 / 108 and that the walk ends exactly at `n - 31` |
| Replay extension: **write** (CSP-compatible) | **M-L** | needs the 108-byte per-car frame layout, 56/16-byte frame layouts, session record, `CONFIG_RACE`; zlib bytes need not match | compare **inflated** payloads; derive fields that are functions of the AC frame and check them against the stored values frame by frame (no CSP needed to run) |
| Own loader behaviour (dummy cars, archive detection, damaged tables) | **S** | rustyAC already tolerates the two files that crash AC | the 2 crashing files of `replays.md` (no longer in the folder: the 9 current files all play in plain AC) |
| Ring / lazy allocation / `JUMP_FIX2` / `ACCOUNT_FOR_PING` | **S** | memory policy, not output | n/a (`JUMP_FIX2` changes the blend: compare frames around a teleport) |
| Ghost 1 ms, several ghosts, `.csp_ext` metadata, saved best lap | **M** | `rustyac_physics::replay::ghost` exists; needs the metadata schema (JSON-like keys listed 1.6) | file diff of a ghost recorded by CSP on the same lap (needs a ghost: none on this PC) |
| Comparative telemetry first-lap fix | **S** | one lap-count override | compare the saved `.tc` of the first lap |
| Pit stop fuel animation, per-part progress, 1.5 m box, `PITS_ORDER` offsets | **S-M** | `race/pitstop.rs` has the stop state machine and the shares; the animation is one interpolation | fuel curve vs time in a CSP-on recording (shared-memory `fuel`) |
| Crew model keys, lollipop randomisation | **S** | `rustyac-render` has both | picture only |
| New modes (Lua host, manifests, 10 mode scripts, 12 functions) | **XL** | needs a Lua runtime (LuaJIT FFI heavy SDK), UI integration, Content Manager protocol, Steam P2P for one mode; rustyAC has no Lua | none practical; do manifests + a built-in "overtake" / "traffic" in Rust first (S-M) |
| Custom AI pipe | skip | developer / external tool | |

What would be **bit-exact against CSP** if done: everything stateful but not chaotic (flags, lap validity, replay
container and derived fields, caches, session text, pit stop timeline). What would be **statistical**: any AI lap
or race, because the AI closes the loop through the physics (`docs/port/ai.md` section 5) and CSP also changes the
physics under it (air density, ODE, mesh colliders: `prior/csp_survey.md`).

## 8. Open questions and not mapped

1. **Is a real-AC AI lap reproducible at all?** The AI draws from the C runtime `rand()` of the physics thread. If
   that thread is never `srand`ed the stream starts at the default seed, and a CSP-on run with the pool off would
   repeat exactly; then `f2004_spa_ai.csv`-style recordings could be matched bit by bit after porting the tweaks. One
   experiment: record the same AI lap twice with CSP on and compare; if identical, pin the seed in the port. Unknown.
2. `KEEP_AI_LAPS`, `KEEP_ALTERED_LAPS`, `HAUNTED_COMPATIBLE`, `EXT_RECORD/SAVE/LOAD`: key strings absent from the image
   (hashed names); consumers not found.
3. Where `[WRONG_WAY]` is enforced and what `ALLOW_EVERYWHERE` is; the per-car "wear" rule object (+0x1d0) of the
   black flag; the meaning of `0x182695041` (written by the new-modes loader 0xd2fce0 and 0xeeb890; my reading "a new
   mode is active").
4. Content of the 108-byte per-car replay frame (listener 0x11a0ac0), the 56-byte `PERFRAME` and `PERRACEFRAME`
   fields, `EXT_PERRACE_v2`. Dword counts and the shape (4 x 5 dwords + 7) are known, names are not.
5. The alt-spline picker (0x1082c90) and which of `AVOID_SIDES` / `TRAJECTORIES_FOR_DUMMIES` / `ALT_DRAG_SPLINE` it
   reads; the exact use of `0x1826a9690+4` (pit limiter speed from `[EXTRA_RULES]`, default 80 km/h) in `getLABraking`.
6. `_DAT_1826add30` is read by 350 functions; I read it as a **frame counter** (`getLeaderLapCount` memo key; the "< 10"
   test in `TimeTransponder::reset` = first ten frames). `physics_car_hooks.md` reads it as a car count. Needs one
   decompile to settle.
7. The tail of `acquireControls` (rate limiting) is unconditional; whether it also applies to the player's **own** AI
   (autopilot) -- yes by construction (same hook), not verified in a recording.
8. `RaceManager::resetInvalidStateCarsToPits` replacement: gating `*piVar5 < 1` of 0x10ee720 not understood.
9. Whether a CSP-written ghost / replay is read back by plain AC: replays append after AC's data and AC ignores the tail
   (the owner's old files were read by the game's loader per `docs/port/replays.md`), ghosts unknown.
10. Online replay extras ("doesn't work with extra data yet" [ini]).

## Evidence

* `decomp_hooks/` (118 files of this group; read: all AIDriver, AISpline*, RaceEngineer, RaceManager, PitStop/PitCrew/
  LollipopCrew, FlagManager, ReplayManager, ReplayRecorder (partly), GhostCar* (skim), RaceTimingServices, JsonOutputFile,
  DragMode, TimeAttack, SessionResultProcessor, ComparativeTelemetryRecorder), `decomp_req1/138ded0.c`, `1387350.c`
  (settings loaders), `hook_groups.tsv`, `csp_hook_table.tsv`, `dll_strings.tsv`, `dll_func_strings.tsv`, `dll_funcs.tsv`.
* Byte-level scans of `dwrite_copy.dll` (read only, stdout only): rip-relative references to key strings and to the flag
  bytes of 3.1/3.2 (function of the reader / writer), `0x124a5e0` and `0x124b760` key order, embedded replay trailer.
* Shipped config: `new_behaviour.ini`, `general.ini`, `gui.ini`, `small_tweaks.ini`, `spectating.ini`, `data_manifest.ini`,
  `data_alt_mapping.ini`; Lua SDK `ac_apps`, `ac_new_modes`, `ac_car_cphys`, `ac_track_script` `lib.lua`; `lua/new-modes/*`,
  `lua/tools/*`.
* Owner's files: `Documents\Assetto Corsa\replay\temp\*.acreplay` (9), `logs\custom_shaders_patch.log`, `logs\log.txt`,
  `cfg\race.ini`, `cfg\extension\new_behaviour.ini`, `<AC>\cache\ai_payloads\*.bin`, `content\tracks\{imola,monaco_2017}\ai\*.ai`.
* Earlier briefs: `attach.md` 1.7 / 2.2, `physics_car_hooks.md` (TimeTransponder, Car::getPitstopTime, SpeedLimiter,
  PenaltyManager), `ui_apps.md` (quick pit menu, FINAL_LAP_TWEAK), `online.md` (`[EXTRA_RULES]`), `lua.md`,
  `prior/csp_survey.md` (I9, S1..S7); `docs/atlas/ai_drivers.md`, `docs/port/ai.md`, `race_rules.md`, `pit_stops.md`,
  `replays.md`.

## Decompile requests

1. `0x11a0ac0` -- the listener on the per-frame extension bus 0x182697858: what each of the 108 bytes per car-frame is.
2. `0x11a4750` -- the extension writer: section order, per-frame field layouts, `EXT_PERRACE_v2`.
3. `0x11a2cb0` and `0x11a4000` -- extension reader dispatch and header validation (compat rules for v1..v6).
4. `0x11a66c0` and `0x11a7a70` -- CSP's replacement replay loader; what it differs from AC's (dummy cars, tables).
5. `0x11a25b0`, `0x11a0c00`, `0x11a1c80` -- extension slot allocation, lazy ring growth, replay on/off handler.
6. `0x124a5e0` -- confirm the AI-tweak key to byte map (3.1) including the hidden `FIX_SHORT_AXLE` handling.
7. `0x124b760`, `0x124cab0`, `0x124ea20` -- flood / cooldown / retirement config + event registration + the per-frame driver.
8. `0x1246f20`, `0x124d480`, `0x1247180` -- cooldown lap and retirement logic (conditions, timings, messages).
9. `0x12489a0`, `0x12485f0`, `0x1249ef0`, `0x124d810`, `0x12478b0`, `0x1247c60` -- flood spawn / despawn / traffic spline.
10. `0x124a0d0`, `0x14f2890`, `0x14f2900` -- reset car and step back; `0x124a390`, `0x124df80` -- rubber-banding apply / setter.
11. `0x1082c90`, `0x1082370`, `0x1082db0` -- alternative spline picker for AIs.
12. `0x107e130`, `0x107e370`, `0x107e800`, `0x107ef50`, `0x107efd0`, `0x10842e0`, `0x1083b30` -- AI helpers (init, blocker, impatience, tyres).
13. `0x525350` -- the autopilot Lua binding (options that fill the driver flags word 0x268b93c); `0x11be520` -- autopilot toggle.
14. `0xecf0e0` -- the per-car controls post-processor called from `acquireControls`.
15. `0x10ee720`, `0x10eebd0`, `0x10efb40` -- RaceManager session getter, leader laps, session setup.
16. `0x5cf290`, `0x130e000` -- flag drawing (white flag, code 60 and friends).
17. `0x6a57c0` -- the TimeAttack callback wrapper; `0x5e62f0`, `0x5e6760` -- saved best lap / ghost metadata load.
18. `0xd2fce0`, `0xd2d550`, `0xd33880`, `0xd2b900`, `0xd27d00`, `0xd27f90` -- new-mode manifest, opponents, `overrideSession`,
    `endSession`, custom-AI pipe.
19. `0x1811b00d0` (RVA `0x11b00d0`) -- replay manager registration of listeners.
20. `0x5864d0`'s lambda `0x5893a0` and `0x585610` -- the original-call closure of `AISpline::load` and the CSP payload computation.
