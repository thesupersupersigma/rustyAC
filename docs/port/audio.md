# Task 19: sound

## Resume here

State after the last commit (kept up to date with every commit):

- **Done.** Everything is on `master`, nothing is pushed. The version is 0.19.0 and the tag `v0.19.0` is on the last commit. Nothing is half done.
- What exists: `crates/rustyac-audio` (the binding to the player's FMOD DLLs with its call log, `AudioEngine`,
  the two DSP plug-ins, `CarAudioFMOD`, `TrackAudio`, the simulation's part, the golden file), the game side
  (`crates/rustyac-game/src/audio.rs`, the options in `cli.rs`, the wiring in `main.rs`), the oracle
  (`tools/audio_oracle`: `run`, `compare`, `golden`, `dsp`, `survey`; `tools/car_oracle run --audio-tape`).
- Checks to run again: section 10.
- On disk, git-ignored: the tapes of the oracle drives `oracle/audio/<car>/<scenario>.audiotape` (3 to 27 MB
  each; re-record with `re/scratch/task19/record_tapes.sh`, about ten minutes), the result rows
  `oracle/audio/results_rows.txt`, `oracle/audio/survey.md`, `oracle/audio/dsp_results.md`, the end-to-end
  recordings `oracle/audio_e2e/` with their input files `oracle/game/*_spa_*.ryin`, and the two listening
  WAVs in `re/scratch/task19/listen/`. The logs and WAVs of the comparisons were deleted after each drive.
- Scratch of this task (git-ignored): `re/scratch/task19/` (`spec_*.md` are the six briefs read from the machine
  code, `p*.py` the patch scripts, `record_tapes.sh`, `compare_all.sh`, `e2e.sh`, `stress.sh`, `fill_report.py`
  and `report_template.md`, which this report is built from).
- Section 8 is the list of what is still missing; section 9 the choices made without asking.

## 1. Plain-English summary

**rustyAC is no longer silent: it plays Assetto Corsa's own sound.** The engine, the gear shifts, the tyres,
the surfaces under the wheels, the wind, the bodywork, collisions, backfire, turbo, limiter, traction control
and the track's ambience and reverb are all AC's own FMOD events out of AC's own sound banks, started,
placed and fed by a 1:1 port of the game's sound code.

- **Nothing of FMOD or of AC's sounds is in the repository or in a release.** rustyAC loads the player's own
  `fmod64.dll` and `fmodstudio64.dll` (FMOD Studio 1.08.12) from the Assetto Corsa folder when it starts, the
  same way it loads `MSVCR120.dll` for the maths, and plays the banks where they are. The binding is written
  by hand from the 65 functions `acs.exe` imports. Without an AC install rustyAC runs silently and says so in
  one line.
- **What is ported** (section 3): `AudioEngine` with its events, reverbs and occluders, the two DSP plug-ins
  the game carries inside itself ("FMOD Distance Filter", "FMOD Gain"), `CarAudioFMOD` (about 30 events per
  car), `TrackAudio`, and the pieces of `CarAvatar`, `Sim` and `Car::getPhysicsState` the sound hangs on.
- **How it was checked** (section 4): the game's own sound code is run in a test program, loaded out of
  `acs.exe`, next to the port. Both talk to the real FMOD through one thin layer that writes down every call
  with every argument bit. The two lists must be the same.
- **Result** (section 5): on 18 recorded drives (6 cars, Spa, Monza, Magione, Imola and the flat road; whole
  laps, launches, lock-ups, grass, gravel, kerb strikes, wall hits, the limiter, three listener positions)
  **the port makes exactly the calls the game makes: 16,704,338 FMOD calls, every argument bit equal.**
  And end to end: `rustyac.exe`, replaying a drive with its own physics and its own inputs for the sound,
  writes the same call list as the game's sound code fed by the game's physics, on 7 drives, a whole
  lap of Spa among them.
- **The two DSP plug-ins are bit-exact**: 75 million samples through the game's own code and through the
  port, no difference.
- **The WAV files cannot be byte-identical, and that is FMOD, not the port.** FMOD picks samples and start
  offsets inside the events with random numbers seeded from the clock, so two runs of *the game's own code*
  give two different files. This is shown for every drive (two game runs each). Within that the port matches:
  its mix is as far from the game's as the game's own second run is (section 5.2).
- **All 123 installed cars that have a sound bank play**, the 13 mod cars with their own GUID files included;
  all load with FMOD 1.08.12. 70 car folders have no bank (DLC that is not installed): silent in AC too.
  44 track layouts play (section 6).
- **It costs nothing that matters**: 0.05 ms a frame on the display thread, FMOD's own mixer thread 2 to 3 %
  of one core, and the 333 Hz physics thread is as punctual as before (section 7).
- **One thing I could not do: listen.** The rule was to keep the speakers quiet, so every run here used
  FMOD's file writer or its no-sound output. The first run on a real sound card is yours (section 2); two
  short WAVs made by `rustyac.exe` are there to listen to first.

## 2. How to drive with sound

Sound is on by default with a window. Nothing to install: Assetto Corsa has to be where rustyAC already
finds it (Steam's library, or `AC_ROOT`).

```
target\release\rustyac.exe --track spa                          the F2004 at Spa, with sound
target\release\rustyac.exe --track magione --car bmw_m3_e30     another car
target\release\rustyac.exe --track spa --no-audio               silent, as before
target\release\rustyac.exe --track spa --volume 0.4             master volume 0.4 instead of audio.ini's MASTER
```

- Volumes come from your `Documents\Assetto Corsa\cfg\audio.ini` (`[LEVELS]` MASTER, WIND, TYRES, SURFACES,
  DIRT_BOTTOM, OPPONENTS, ENGINE, TRANSMISSION; `[SKIDS] ENTRY_POINT`), the output device from its
  `[SETTINGS] DRIVER_NAME` (yours: "Speakers (Apple Audio Device)"), channels and the on/off switch from
  the game's `system\cfg\audio_engine.ini`. Change them in AC or in Content Manager as always.
- F1 and F6 change the view and the sound with it, as in AC: the cockpit, the dash, the bonnet and the
  bumper view play the interior engine, gear and skid sounds (bonnet and bumper count as outside only when
  `Documents\Assetto Corsa\cfg\camera_manager.ini [AUDIO] BONNET_EXTERNAL` / `BUMPER_EXTERNAL` say so), the
  chase cameras the exterior ones; a car camera (F6) is inside or outside by its `EXTERNAL_SOUND` in the
  car's `cameras.ini`. In every drivable view but the dash, tyres and engine are 1.5 times louder.
- P pauses: the master volume goes to zero, as AC's pause menu does it.
- `--headless` stays silent unless one of the sound's own options below is given.

For checks, without the speakers:

```
rustyac.exe --track spa --autodrive --auto-shifter --headless --duration 40 --no-race-ini --audio-wav lap.wav
rustyac.exe --replay drive.ryin --headless --camera chase --audio-wav drive.wav --audio-log drive.log
rustyac.exe --track spa --autodrive --headless --duration 30 --audio-null
```

- `--audio-wav <file>`: the mix goes to a WAV file through FMOD's non-real-time writer instead of the sound
  card (48 kHz, 16 bit, stereo). One 1/60 s block is written per 1/60 s of *driving*, counted in physics
  steps, so the file is right whatever the PC's speed; a `--replay ... --headless` writes it as fast as it
  can compute.
- `--audio-log <file>`: every FMOD call as text.
- `--audio-null`: FMOD's no-sound output in real time (everything runs, also FMOD's own threads; for timing).
- Test switches of the oracle: `--audio-ini <file>`, `--audio-oracle-camera cockpit|chase|trackside`,
  `--audio-script <log>`.

The two listening files (made by `rustyac.exe` with `--audio-wav`, at the levels of your `audio.ini`):

| File (in `re/scratch/task19/listen/`, git-ignored) | What |
|---|---|
| `f2004_onboard_la_source_eau_rouge.wav` (20.0 s) | The F2004 from the cockpit: the end of the pit straight, braking for La Source, out of it down the hill and through Eau Rouge and Raidillon. `--track spa --autodrive --auto-shifter --camera cockpit`, seconds 20 to 40 of the run |
| `e30_chase_sliding_on_grass.wav` (17.9 s) | The BMW M3 E30 from the chase camera at Spa: away from the hot-lap start, onto the grass at 64 km/h and four seconds sliding across it with all four tyres far over the limit, back onto the road and round. The drive is the oracle's `spa_wall_slide`, replayed by `rustyac.exe --replay oracle/game/e30_spa_wall_slide.ryin --camera chase` |

## 3. What is ported

All addresses are `acs.exe`'s. The six briefs behind this (instruction level, with every FMOD call site)
are `re/scratch/task19/spec_*.md`.

| rustyAC | AC | What |
|---|---|---|
| `crates/rustyac-audio/src/fmod/raw.rs` | the import table (65 functions of `fmod64.dll`, `fmodstudio64.dll`) | The binding: the exports looked up by their decorated names in the player's DLLs, each wrapped in a function of the export's exact ABI that logs the call. Plus three exports the game does not import, for the file writer: `System::setOutput`, `System::setDSPBufferSize`, `Studio::System::flushSampleLoading` and `EventDescription::getSampleLoadingState` |
| `fmod/types.rs`, `fmod/dsp_abi.rs`, `dsp/mod.rs` | - | The FMOD 1.08 data layouts the calls need (`FMOD_3D_ATTRIBUTES`, `FMOD_GUID`, `FMOD_REVERB_PROPERTIES`, `FMOD_DSP_DESCRIPTION` 0xd8 bytes, `FMOD_DSP_PARAMETER_DESC` 0x60 bytes, the DSP state and its callbacks), written from how `acs.exe` fills and reads them |
| `engine.rs` `AudioEngine::new` | `AudioEngine::AudioEngine` 0x1401f6bf0, `getDriverInfo` 0x1401f96d0, `setDriver` 0x1401fb7f0, `listDrivers` 0x1401fa320, `fmodCallback` 0x1401f9580 | The order of creation: create (header version 0x10812), the device named in `audio.ini` looked for and set, advanced settings (command queue 0x80000; `vol0virtualvol` 0.0005), `setSoftwareChannels(MAX_CHANNELS)`, `initialize(0x800, live update, right-handed 3D)`, the two plug-ins, `setGeometrySettings(5000)`, the device-lost callback, the version, `content/sfx/GUIDs.txt`, `common.bank` |
| `parse_guids`, `lookup_guid`, `has_event`, `add_bank_ref`, `remove_bank_ref` | `parseGUIDs` 0x1401fa8c0, `lookupGUID` 0x1401fa6c0, `hasEvent` 0x1401f9c60, `addBankRef` 0x1401f8220, `removeBankRef` 0x1401fb200 | The path-to-GUID table (keys lower-cased, an unknown path gets a zero GUID), banks counted by GUID on the way in and by path on the way out |
| `create_event`, `destroy_event`, `event_*` | `AudioEvent::AudioEvent` 0x1401f7630, `~AudioEvent` 0x1401f7b30, `start` 0x1401fbf40, `stop` 0x1401fc040, `resume` 0x1401fb320, `isPlaying` 0x1401fa170, `isPaused` 0x1401fa120, `isWithinRange` 0x1401fa1c0, `setParameter` 0x1401fbc40, `set3DAttributes` 0x1401fb370 / 0x1401fb520, `setBaseVolume` 0x1401fb6d0, `setBasePitch` 0x1401fb610 | An event: `getEventByID`, `loadSampleData`, `createInstance`; every method with its finiteness tests |
| `set_listener`, `set_distance_scale`, `listener_position`, `listener_distance` | 0x1401fbb30, 0x1401fb790, 0x1401f9880, 0x1401fa610 | The listener (position = row 3, forward = minus row 2, up = row 1) and FMOD's roll-off scale |
| `update`, `ramp_volume`, `check_output_sanity`, `update_properties`, `start`, `stop`, `set_volume` | `update` 0x1401fc080, `rampVolume` 0x1401fafa0, `checkOutputSanity` 0x1401f8e00, `updateProperties` 0x1401fc310, 0x1401fbec0, 0x1401fbfc0, 0x1401fbdf0 | Per frame: the 0.9 s master ramp; for *every* live event `get3DAttributes`, `getGeometryOcclusion`, `setVolume((1 - occlusion) * base)`; FMOD's update |
| `add_cache`, `get_cached_event`, `release_cached_event`, `clear_cache` | 0x1401f8730, 0x1401f9650, 0x1401fb0d0, 0x1401f8f60 | The pool of surface sounds: twelve instances per surface, last in first out |
| `AudioReverb`, `PRESET_TABLE` | 0x1401f7860, `hearValue` 0x1401f9dd0, `setPreset` 0x1401fbd10, `setProperties` 0x1401fbd70, `presetTable` 0x1404e81c0 | Reverb spheres; the 24 presets |
| `AudioOccluder` | 0x1401f77e0, `addTriangle` 0x1401f8890, `updatePolygonAttributes` 0x1401fc260 | Occluding triangles |
| `dsp/distance_filter.rs` | fmod_distance_filter.obj: `FMODGetDSPDescription_DistanceFilter` 0x1401fc650, `process` 0x1401fc900, `updateTimeConstants` 0x1401fcf20 and the callbacks | Two one-pole low-passes and a high-pass narrowing with distance; the shared `dc` static with its sign flip per frame |
| `dsp/gain.rs` | fmod_gain.obj: `FMODGetDSPDescription_Gain` 0x1401fd240, `process` 0x1401fd3f0, `setGain` 0x1401fd590, `gain` 0x1401fd380 | Gain in dB with a 256-frame ramp and invert, with the SDK example's own slip in `gain()` |
| `car.rs` `CarAudio::new` | `CarAudioFMOD::CarAudioFMOD` 0x140062830, `MixVolumes::MixVolumes` 0x140064a40, `SkidParams::SkidParams` 0x140065110, the caller `CarAvatar::initCommonPostPhysics` 0x1400d6190 | The events in the game's order (which decides the order of everything in `update`), the optional ones, `engine.ini [TURBO_n] MAX_BOOST`, `sounds.ini [ENGINE] POSITION` |
| `render_audio`, `start_stop_events`, `set_event_volumes`, `compute_transforms`, `update_wheels`, `update_skids`, `update_surfaces`, `play_surface`, `should_use_internal_sounds` | `renderAudio` 0x1400669b0, 0x1400678c0, 0x140067330, 0x140065f20, 0x140068730, 0x140067f30, 0x140068450, 0x140066700, 0x140067810, `SmoothValue::update` 0x140067eb0, `CarAvatar::isInPit` 0x1400d8bd0 | The frame of a car's sound |
| `on_car_hit`, `on_gear_event`, `on_backfire`, `on_downshift_protection`, `destroy` | `onCarHit` 0x140066480, the gear lambda 0x140065910 with `onGearChanged` 0x140066670, the backfire lambda 0x140065750, the down-shift lambda 0x1400658a0, `~CarAudioFMOD` 0x1400653e0 | The car's events |
| `track.rs` `TrackAudio`, `Scene`, `cache_surface_sounds` | `TrackAudio::TrackAudio` 0x1401c24c0, `render` 0x1401c42a0, the destructor 0x1401c3bd0, `Node::findChildrenByPrefix` 0x14020e050, `findChildByName` 0x14020de40, `getWorldMatrix` 0x14020e190, the loop of `Sim::loadTrack` 0x14019a4c0 | Ambience emitters on the `AC_AUDIO_*` dummies, the reverbs and occluders of `data/audio_sources.ini`, the surface pool |
| `sim.rs` `BackfireParams`, `CarSound::avatar_update`, `AudioWorld` | `BackfireParams::BackfireParams` 0x1400cccb0, `checkBackfire` 0x1400d26b0, `CarAvatar::update` 0x1400db830 (its backfire test and gear trigger, `EventTriggerOnChange<int>::update` 0x1400daef0), `Sim::stepPhysicsEvent` 0x14019ebd0, the listener ranking in `Sim::update` 0x14019ef90, `ACCameraManager::setAudioDistanceScale` 0x140033e20, the order of `Game::onIdle` 0x140242730, `GameTime::update` 0x14044c250 (the 0.2 s cap) | What other classes do for the sound, and one frame in the game's order |
| `crates/rustyac-game/src/audio.rs` `capture` | `Car::getPhysicsState` 0x140270d70 (the fields the sound reads, among them `bodyWorkVolume` 0x140271922 and the tyre matrices 0x1402712de), `CarAvatar::isTcInAction` 0x1400d8de0, `CarAvatar::makeBodyMatrix` 0x1400d8ec0 (the position) | The sound's inputs after every physics step |

**The inputs** (the brief is `spec_inputs.md`). The game copies the newest finished physics step to the main
thread unblended (`PhysicsAvatar::update` 0x140122480; its "matrix smoothing" is switched off in this build),
so the sound is fed from the newest step and never from the blended picture. These were missing in rustyAC and
are now computed as the game does: the tyre matrices in the game's form (spin times the hub matrix the tyre
read in its step, at the hub's place now), `bodyWorkVolume` (how fast the four hubs move against the body
along its up axis), wheel speeds gated by lock and sleep, slip ratios, dirt, inflation, the surface a tyre
touched last with its `WAV` and grip (kept while the tyre is in the air), drivetrain speed, turbo boost and
blow-off, gear grinding, the limiter, and the physics thread's collision events (every one, queued).

**Corrections to `docs/atlas/audio.md`** (the atlas was written from pseudo-C):

- The version check only prints: a DLL that is not 1.08.12 does not stop the game.
- No `.strings.bank` is ever loaded. Every event is found by GUID.
- `impact_speed` is the closing speed in km/h multiplied by 3.6 a second time (the physics already converts).
  A hit sounds from 2.0 of that unit on, about 0.15 m/s.
- `listenerDistance` of a car is not its distance but the gap to the next nearer car in the ranking;
  `listenerPriority` is the rank (cars in their pit box do not count).
- The skid's "load factor" is `load[i] / (0.125 * sum of loads)`, at least 0.2.
- Ambience emitters use the node's *local* matrix, only on nodes that are not meshes; the sections
  `[AC_AUDIO_n]` some tracks have in `audio_sources.ini` are not read.
- Every reverb zone's wet level is 0 when it is asked how much the listener hears of it (the vector move
  drops it), so the factor is always (0 + 80) * 0.01; `WET_LEVEL` still reaches FMOD.
- In the cockpit the exterior engine sound plays only inside a reverb zone, at `reverbValue` times its
  volume: that is the pit-wall echo.
- `SurfaceDef.wavPitchSpeed` (`WAV_PITCH`) is read by no sound code.
- The wind sound sits half a metre ahead of the *rear* axle outside the cockpit.

## 4. The oracle method

```
car_oracle run --audio-tape     the game's own physics: CarPhysicsState after every 1/60 s, every collision event
        |
        v  <scenario>.audiotape
audio_oracle run --side ac      acs.exe mapped into the process; its AudioEngine, TrackAudio, CarAudioFMOD run
audio_oracle run --side port    rustyac-audio on the same tape
        |                             both: FMOD imports -> rustyac_audio::fmod::raw -> the real fmod*.dll
        v                             output: FMOD_OUTPUTTYPE_WAVWRITER_NRT, 48 kHz, one 800-sample block a frame
<drive>_<side>.log + .wav       every call, every argument bit; the mix
```

- **The layer** (`fmod/raw.rs`, `fmod/log.rs`). One function per FMOD export with the export's exact ABI. For
  the game's side they are written into the import table of the mapped `acs.exe` (the loader of
  `tools/car_oracle/src/acs.rs`, shared by `#[path]`, got a way to take more stand-ins). A log line is the
  function, its arguments (floats as raw bits, structures as hex words, strings quoted, objects as numbers in
  the order FMOD first handed them out) and what FMOD gave back. Left out of the log: device names and
  counts, CPU and buffer usage, callbacks' addresses. A plug-in description is logged with its name, version,
  buffers and parameter tables.
- **The tape** (`car_oracle run --audio-tape`, `rustyac-audio/src/tape.rs`). The game's own
  `Car::getPhysicsState` result (0xb70 bytes) after `floor(n * 50 / 9)` physics steps for frame n (60 frames
  a second of 3 ms steps), the traction control's "in action" flag at that moment, and every
  `ACPhysicsEvent` the engine queued in between.
- **The game's side** (`tools/audio_oracle/src/harness.rs`). The game's own constructors and frame functions
  are called by address on objects laid out by hand: a `Game` with the engine, a `Sim` with the camera
  manager and the car list, a `CarAvatar` whose `physicsState` is the tape's bytes, the track's nodes with
  the game's own `Node` / `Mesh` vtables (built from the scene the port's kn5 reader finds), a
  `TrackAvatar`. Game code that ran untouched: all of `AudioEngine`, `TrackAudio`, `CarAudioFMOD`,
  `BackfireParams`, the event triggers and the handlers they call, `INIReader`, the scene searches.
- **What the harness does itself** in place of game code it cannot run (the whole `Sim`, `Game`, `CarAvatar`):
  the order of a frame (section 4.5 of `spec_inputs.md`: collision events, the ranking, the new state, the
  backfire test and the gear trigger, the listener, `TrackAudio::render`, `renderAudio`,
  `AudioEngine::update`); the hand-over of collision events to `onCarHit` (`Sim::stepPhysicsEvent`); the
  ranking of one car (`AudioEngine::listenerDistance` is the game's, the two stores are the harness's); the
  loop of `Sim::loadTrack` over the surfaces; `setAudioDistanceScale` for the session's camera (two stores
  and the game's `setDistanceScale`); `Game::Game`'s `setVolume(MASTER)`. The port's versions of exactly
  these are in `sim.rs`: they are ported from the briefs but compared only against the harness's reading of
  the same briefs.
- **The listener**: three fixed rules from the car's state, the same code on both sides: at the driver's head
  (camera mode 0, cockpit), 6 m behind and 1.8 m above (mode 2, chase), a fixed point beside the start
  (mode 3, track camera, no velocity).
- **What the layer does for a clock-less run** (both sides alike; the log shows the caller's own arguments):
  sets the output type, the block (800) and the format right after `Studio::System::create`, so that the
  game's search for the player's sound card sees the same device list on every PC; makes the Studio system
  synchronous in `initialize`; resolves a relative bank path against the game's folder (the port and the
  game hand over the same relative path); in `flushCommands` (the game calls it at the end of every car's
  constructor) waits, without mixing, until FMOD's loader thread has every event's samples; sets
  `randomSeed`; and serialises unloads at the end of a run.
- **Scripted answers.** FMOD's randomness also decides when a one-shot sound (a gear shift, a backfire, a hit)
  ends, and the game asks "is it still playing?" before it starts the next. So even two runs of the game's
  own code can make different calls (column "game twice" below). For the comparison the port is run a second
  time and told, at each such question, what the game's run was told (`log::start_scripted`): then the calls
  are equal exactly when the logic is. The free-running port is compared too.

## 5. Results

### 5.1 The FMOD call log, drive by drive

`sh re/scratch/task19/compare_all.sh` (each drive: the game's side twice, the port free, the port with the
game's answers; about a quarter of an hour).

- **Calls**: FMOD calls the game's sound code made in the drive.
- **Port = game**: the port's log, given the game's answers to the state queries, against the game's log.
  "identical" is every line equal: every call, in order, every argument bit, every value FMOD returned.
- **Game twice**: two runs of the game's own code against each other. **Port free**: the port left to itself
  against the game's first run. Where these say "differs", FMOD ended a one-shot sound a frame apart in the
  two runs.

| Car | Track | Drive | Listener | Frames | Calls | Port = game | Game twice | Port free | |
|---|---|---|---|---|---|---|---|---|---|
| F2004 | Spa | `spa_launch`: launch with wheelspin, flat out down the straight, braking | cockpit | 1440 | 534,008 | **identical** | identical | identical | ok |
| F2004 | Spa | `spa_launch`: launch with wheelspin, flat out down the straight, braking | chase | 1440 | 529,820 | **identical** | identical | identical | ok |
| F2004 | Spa | `spa_launch`: launch with wheelspin, flat out down the straight, braking | trackside | 1440 | 529,287 | **identical** | identical | identical | ok |
| F2004 | Spa | `spa_grass`: two wheels, then all four, on the grass | cockpit | 960 | 354,436 | **identical** | identical | identical | ok (1 run(s) repeated: FMOD crashed) |
| F2004 | Spa | `spa_kerb_strike`: the Bus Stop much too fast over its kerbs (collisions on) | cockpit | 960 | 357,034 | **identical** | identical | identical | ok |
| F2004 | Spa | `spa_wall_gravel`: over the run-off gravel into the barrier (collisions on) | chase | 1200 | 441,499 | **identical** | identical | identical | ok |
| F2004 | Spa | `spa_wall_high`: off at Blanchimont into the wall at speed (collisions on) | trackside | 1200 | 443,149 | **identical** | differs | differs | ok |
| F2004 | flat road | `pt_shifter`: revs to the limiter in neutral, H-shifter up- and down-shifts | cockpit | 720 | 158,406 | **identical** | identical | identical | ok |
| F2004 | flat road | `pt_protect`: paddle down-shifts the gearbox must refuse, the limiter | cockpit | 600 | 132,118 | **identical** | identical | identical | ok |
| F2004 | flat road | `brake`: flat out to 250 km/h, full brakes to a stop without ABS: locked wheels | chase | 960 | 211,860 | **identical** | identical | identical | ok |
| BMW M3 E30 | flat road | `wc_stops`: two full-throttle runs each ended by a full-pedal stop | cockpit | 780 | 166,277 | **identical** | identical | identical | ok |
| Formula Alpha 2026 | flat road | `hy_deploy`: flat out with the ERS button held until the battery is empty | cockpit | 1800 | 444,628 | **identical** | identical | identical | ok |
| Formula Alpha 2026 | Spa | `spa_hybrid_timing`: ERS on the throttle, over the lap lines (teleported between them) | chase | 2896 | 1,145,275 | **identical** | differs | differs | ok |
| Ferrari 250 GTO | Magione | `trk_kerb_strike`: the tightest corner much too fast over its kerbs (collisions on) | cockpit | 960 | 325,325 | **identical** | identical | identical | ok |
| Ferrari F40 | Monza | `trk_lap`: the first 57 s of a lap (then the oracle's driver spins the F40 off; collisions on) | cockpit | 3400 | 1,429,392 | **identical** | identical | identical | ok |
| BMW M3 E30 | Magione | `trk_lap`: a whole lap from the hot-lap start (collisions on) | chase | 7731 | 2,552,307 | **identical** | identical | identical | ok (1 run(s) repeated: FMOD crashed) |
| Ferrari 488 GT3 | Imola | `trk_lap`: a whole lap from the hot-lap start (collisions on) | cockpit | 8988 | 3,604,373 | **identical** | identical | identical | ok |
| F2004 | Spa | `trk_lap`: a whole lap from the hot-lap start (collisions on) | cockpit | 9080 | 3,345,144 | **identical** | identical | identical | ok |
| **18 drives** | | | | **46,555** | **16,704,338** | **18 identical** | 16 identical | 16 identical | |

What the drives cover: a full lap (three: Spa, Imola, Magione; and 57 s of one at Monza); a launch with wheelspin (`spa_launch`, `pt_shifter`,
`brake`); a lock-up under braking (`brake`: the F2004 without ABS from 250 km/h; `wc_stops`); grass
(`spa_grass`) and gravel (`spa_wall_gravel`); a kerb strike (`spa_kerb_strike`, `trk_kerb_strike`); a wall hit
(`spa_wall_high`, `spa_wall_gravel`); up- and down-shifts through the limiter (`pt_shifter`, `pt_protect`, the
laps); the F2004, the E30 (strut suspension, Task 17), the 250 GTO (rigid axle, Task 17), the F40 (twin
turbo, blow-off, backfire), the 488 GT3 (turbo, traction control, backfire), the Formula Alpha 2026 (plain;
ERS, its own GUID file, 20 events); Spa (1 reverb zone), Monza (5 emitters, 5 zones), Magione (2 emitters, 1
zone), Imola (7 emitters, 3 zones); cockpit, chase and track-side listeners. No installed track has an
occluder (section 6), so `AudioOccluder` is ported from the brief but ran in no comparison.

### 5.2 The WAVs

Byte-identical WAVs are not to be had: **two runs of the game's own code never gave the same file** (18 of 18
drives). The samples differ from the first sounding frame on, at full level; the call logs of the same two
runs are equal line by line. FMOD picks samples and start offsets inside the events with its own random
numbers. `FMOD_ADVANCEDSETTINGS.randomSeed` is 0 in the game ("seed from the clock"); setting it (the layer
does, for clock-less runs) did not make the mix repeat, so the Studio layer has a generator of its own that
the API of 1.08 does not reach, and patching FMOD is out of bounds.

So the WAVs are compared within FMOD's own run-to-run difference: the level of every second of the mix (RMS,
dB) is compared window by window. "Game twice" is how far two runs of the game are apart, "port" how far the
port's free run is from the game's first. The port passes when its distance is no more than twice the
game's own (or half a dB).

| Car | Track | Drive | Listener | Length | Game twice: levels apart | Port against game: levels apart | |
|---|---|---|---|---|---|---|---|
| F2004 | Spa | `spa_launch` | cockpit | 25.6 s | 0.09 dB rms, 0.21 dB at most | 0.09 dB rms, 0.28 dB at most | ok |
| F2004 | Spa | `spa_launch` | chase | 25.6 s | 0.06 dB rms, 0.13 dB at most | 0.09 dB rms, 0.32 dB at most | ok |
| F2004 | Spa | `spa_launch` | trackside | 25.6 s | 0.06 dB rms, 0.19 dB at most | 0.04 dB rms, 0.12 dB at most | ok |
| F2004 | Spa | `spa_grass` | cockpit | 17.6 s | 0.33 dB rms, 0.96 dB at most | 0.32 dB rms, 0.86 dB at most | ok |
| F2004 | Spa | `spa_kerb_strike` | cockpit | 17.6 s | 0.16 dB rms, 0.42 dB at most | 0.13 dB rms, 0.26 dB at most | ok |
| F2004 | Spa | `spa_wall_gravel` | chase | 21.6 s | 0.87 dB rms, 2.82 dB at most | 0.37 dB rms, 0.93 dB at most | ok |
| F2004 | Spa | `spa_wall_high` | trackside | 21.6 s | 0.02 dB rms, 0.05 dB at most | 0.10 dB rms, 0.31 dB at most | ok |
| F2004 | flat road | `pt_shifter` | cockpit | 12.8 s | 0.09 dB rms, 0.26 dB at most | 0.10 dB rms, 0.17 dB at most | ok |
| F2004 | flat road | `pt_protect` | cockpit | 10.8 s | 0.06 dB rms, 0.11 dB at most | 0.12 dB rms, 0.23 dB at most | ok |
| F2004 | flat road | `brake` | chase | 16.8 s | 0.03 dB rms, 0.11 dB at most | 0.09 dB rms, 0.27 dB at most | ok |
| BMW M3 E30 | flat road | `wc_stops` | cockpit | 13.8 s | 0.04 dB rms, 0.10 dB at most | 0.04 dB rms, 0.08 dB at most | ok |
| Formula Alpha 2026 | flat road | `hy_deploy` | cockpit | 30.8 s | 0.03 dB rms, 0.14 dB at most | 0.03 dB rms, 0.13 dB at most | ok |
| Formula Alpha 2026 | Spa | `spa_hybrid_timing` | chase | 49.9 s | 0.11 dB rms, 0.42 dB at most | 0.09 dB rms, 0.24 dB at most | ok |
| Ferrari 250 GTO | Magione | `trk_kerb_strike` | cockpit | 17.4 s | 0.03 dB rms, 0.09 dB at most | 0.04 dB rms, 0.14 dB at most | ok |
| Ferrari F40 | Monza | `trk_lap` | cockpit | 58.4 s | 0.87 dB rms, 3.91 dB at most | 0.65 dB rms, 2.35 dB at most | ok |
| BMW M3 E30 | Magione | `trk_lap` | chase | 130.2 s | 0.56 dB rms, 2.95 dB at most | 0.60 dB rms, 2.87 dB at most | ok |
| Ferrari 488 GT3 | Imola | `trk_lap` | cockpit | 151.5 s | 0.32 dB rms, 1.56 dB at most | 0.28 dB rms, 1.17 dB at most | ok |
| F2004 | Spa | `trk_lap` | cockpit | 152.9 s | 0.17 dB rms, 1.09 dB at most | 0.16 dB rms, 0.97 dB at most | ok |

All WAVs of a drive have the same length to the byte. The port's mix is as close to the game's as the game's
is to itself.

### 5.3 End to end: `rustyac.exe` against the game

`sh re/scratch/task19/e2e.sh <scenario> <camera> [car ...]`. The game's side is as above (the game's sound
code on the game's physics). The other side is the real `rustyac.exe`: it replays the same drive from its
input file (`chassis_compare game-replay` writes it from the recording) with its own physics, takes the
sound's inputs from its own car after every step, and writes its call log (`--audio-log`, with the oracle's
listener and levels). Equal logs mean every input of the sound is bit-equal to the game's in every frame.

| Car | Drive | Listener | Frames | Log lines | `rustyac.exe` free = game | `rustyac.exe` with the game's answers = game |
|---|---|---|---|---|---|---|
| F2004 | `spa_launch` | cockpit | 1440 | 535,450 | identical | **identical** |
| F2004 | `spa_launch` | trackside | 1440 | 530,729 | differs (FMOD ended a gear-shift sound a frame apart) | **identical** |
| F2004 | `spa_kerb_strike` (collisions) | chase | 960 | 355,288 | identical | **identical** |
| BMW M3 E30 | `spa_wall_slide` (grass, the barrier; collisions) | chase | 1080 | 398,619 | identical | **identical** |
| Ferrari F40 | `spa_launch` (turbo, blow-off) | cockpit | 723 | 285,725 | identical | **identical** |
| Formula Alpha 2026 | `spa_launch` (ERS, its own GUIDs) | cockpit | 361 | 144,218 | identical | **identical** |
| F2004 | `trk_lap`: a whole lap of Spa (collisions) | cockpit | 9079 | 3,354,231 | differs from frame 8193 on (FMOD ended a one-shot sound a frame apart) | **identical** |

### 5.4 The two DSP plug-ins

`audio_oracle dsp --count 2000`: the game's own callbacks (through the description
`FMODGetDSPDescription_*` returns) and the port's, on the same random lives of an instance: parameters,
resets, blocks of 1 to 1024 frames with 1 to 12 channels of random samples (denormals, zeros and loud ones
among them).

| Plug-in | Description and parameter tables | Instances | Parameter calls | Blocks | Samples compared | Mismatches | |
|---|---|---|---|---|---|---|---|
| FMOD Distance Filter | identical | 2000 | 24217 | 22510 | 32710880 | 0 | ok |
| FMOD Gain | identical | 2000 | 28512 | 22484 | 42756351 | 0 | ok |

A NaN counts as equal to a NaN (its payload depends on operand order the compiler may choose). With
`AUDIO_DSP_FAULT=1` the port is given each float parameter with its last bit flipped: 1,079 and 955
mismatches in 300 instances, so the comparison does notice one bit.

### 5.5 The golden test

`cargo test -p rustyac-audio`: `tests/golden.rs` replays the first 240 frames (4 s) of the F2004's launch at
Spa from `tests/golden/f2004_spa_launch_cockpit_240.augold` (140 KB: the sound's inputs per frame, the 5,403
answers the game's run got, the line count and hash of the game's log) and wants the port's log to have the
game's 89,163 lines and hash `5a46f0ba78c09b10`. Without Assetto Corsa it prints NOT TESTED and passes.
`tests/logic.rs` has eight tests that need neither AC nor FMOD.
The golden file is rewritten with `audio_oracle golden --tape oracle/audio/ks_ferrari_f2004/spa_launch.audiotape
--camera cockpit --frames 240 --out-file crates/rustyac-audio/tests/golden/f2004_spa_launch_cockpit_240.augold`.

## 6. The bank survey

`audio_oracle survey` (the port with the player's FMOD 1.08.12, no-sound output; the whole table is in the
git-ignored `oracle/audio/survey.md`).

### Cars

| | Cars |
|---|---|
| Car folders in `content/cars` | 193 |
| **Play**: the bank loads in FMOD 1.08.12 and all nine events every car must have are in it (`engine_ext`, `engine_int`, `gear_ext`, `gear_int`, `bodywork`, `wind`, `wheel`, `skid_ext`, `skid_int`) | **123** |
| Bank there but refused by FMOD 1.08.12 (made with a newer FMOD Studio) | 0 |
| Bank loads but an event is missing | 0 |
| No bank (`sfx/<car>.bank` is not there): silent, in AC as well, which makes no sound object for such a car | 70 |

Of the 123 that play, the optional events: traction control 111, limiter 112, horn 91, gear grind 82,
backfire 81, transmission 60, turbo 49.
13 cars are mods with their own `sfx/GUIDs.txt`, all of them built for 1.08: `urd_darche_992_23`,
`vrc_1988_mclaren_mp4-4_r02` / `_r04` / `_r09` / `_r10`, `vrc_formula_alpha_2025` (+ `_csp`),
`vrc_formula_alpha_2026` (+ `_csp`), `vrc_formula_beta_2024` (+ `_csp`), `vrc_formula_lithium_2023` (+ `_csp`).
(The survey is about sound: four of these are cars rustyAC's physics refuses, see `docs/port/suspensions.md`.)
The 70 without a bank are Kunos DLC cars that are not installed (only their menu entry is):
`ks_abarth500_assetto_corse`, `ks_audi_r18_etron_quattro`, `ks_audi_r8_lms`, `ks_audi_r8_lms_2016`, `ks_audi_tt_cup`, `ks_audi_tt_vln`, `ks_bmw_m4`, `ks_bmw_m4_akrapovic`, `ks_ferrari_488_challenge_evo`, `ks_ferrari_488_gt3_2020`, `ks_ferrari_488_gtb`, `ks_ferrari_fxx_k`, `ks_ford_escort_mk1`, `ks_ford_gt40`, `ks_glickenhaus_scg003`, `ks_lamborghini_countach`, `ks_lamborghini_countach_s1`, `ks_lamborghini_gallardo_sl`, `ks_lamborghini_gallardo_sl_s3`, `ks_lamborghini_huracan_gt3`, `ks_lamborghini_huracan_st`, `ks_lotus_25`, `ks_lotus_3_eleven`, `ks_lotus_72d`, `ks_maserati_mc12_gt1`, `ks_mazda_mx5_cup`, `ks_mazda_mx5_nd`, `ks_mazda_rx7_spirit_r`, `ks_mazda_rx7_tuned`, `ks_mclaren_570s`, `ks_mclaren_650_gt3`, `ks_mclaren_p1_gtr`, `ks_mercedes_amg_gt3`, `ks_nissan_370z`, `ks_nissan_skyline_r34`, `ks_porsche_718_boxster_s`, `ks_porsche_718_boxster_s_pdk`, `ks_porsche_718_cayman_s`, `ks_porsche_718_spyder_rs`, `ks_porsche_908_lh`, `ks_porsche_911_carrera_rsr`, `ks_porsche_911_gt1`, `ks_porsche_911_gt3_cup_2017`, `ks_porsche_911_gt3_r_2016`, `ks_porsche_911_gt3_rs`, `ks_porsche_911_r`, `ks_porsche_911_rsr_2017`, `ks_porsche_917_30`, `ks_porsche_917_k`, `ks_porsche_918_spyder`, `ks_porsche_919_hybrid_2015`, `ks_porsche_919_hybrid_2016`, `ks_porsche_935_78_moby_dick`, `ks_porsche_962c_longtail`, `ks_porsche_962c_shorttail`, `ks_porsche_991_carrera_s`, `ks_porsche_991_turbo_s`, `ks_porsche_cayman_gt4_clubsport`, `ks_porsche_cayman_gt4_std`, `ks_praga_r1`, `ks_ruf_rt12r`, `ks_ruf_rt12r_awd`, `ks_toyota_ae86`, `ks_toyota_ae86_drift`, `ks_toyota_ae86_tuned`, `ks_toyota_celica_st185`, `ks_toyota_supra_mkiv`, `ks_toyota_supra_mkiv_drift`, `ks_toyota_supra_mkiv_tuned`, `ks_toyota_ts040`.

### Tracks

54 tracks and layouts are listed; 44 load and play; 10 are not loaded by rustyAC at all (5 are DLC layouts
that are not installed, 5 are the `*_fa_2026` layouts whose `surfaces.ini` only Custom Shaders Patch can read:
`docs/port/tracks.md`). No track has an occluder. Every surface sound the tracks name is in the game's bank.

| Track / layout | `AC_AUDIO_*` emitters | Reverb zones | Occluders | Surface sounds (a pool of 12 each) | Result |
|---|---|---|---|---|---|
| `drift` | 0 | 0 | 0 | grass, kerb, sand | plays |
| `imola` | 7 | 3 | 0 | extraturf, grass, kerb, sand | plays |
| `ks_black_cat_county / layout_int` | 15 | 15 | 0 | tyre_rolling, grass, kerb, gravel, old, sand | plays |
| `ks_black_cat_county / layout_long` | 15 | 15 | 0 | tyre_rolling, grass, kerb, gravel, old, sand | plays |
| `ks_black_cat_county / layout_short` | 15 | 15 | 0 | tyre_rolling, grass, kerb, gravel, old, sand | plays |
| `ks_drag / drag1000` | 0 | 0 | 0 | grass, kerb, sand | plays |
| `ks_drag / drag200` | 0 | 0 | 0 | grass, kerb, sand | plays |
| `ks_drag / drag2000` | 0 | 0 | 0 | grass, kerb, sand | plays |
| `ks_drag / drag400` | 0 | 0 | 0 | grass, kerb, sand | plays |
| `ks_drag / drag500` | 0 | 0 | 0 | grass, kerb, sand | plays |
| `ks_highlands / layout_drift` | 17 | 17 | 0 | grass, sand | plays |
| `ks_highlands / layout_int` | 17 | 17 | 0 | grass, sand | plays |
| `ks_highlands / layout_long` | 17 | 17 | 0 | grass, sand | plays |
| `ks_highlands / layout_short` | 17 | 17 | 0 | grass, sand | plays |
| `ks_laguna_seca` | 7 | 7 | 0 | kerb, grass, sand | plays |
| `ks_monza66 / full` | 2 | 3 | 0 | extraturf, grass, kerb, sand | plays |
| `ks_monza66 / junior` | 2 | 3 | 0 | extraturf, grass, kerb, sand | plays |
| `ks_monza66 / road` | 2 | 3 | 0 | extraturf, grass, kerb, sand | plays |
| `ks_nordschleife / endurance` | 50 | 5 | 0 | kerb, extraturf, grass, sand | plays |
| `ks_nordschleife / endurance_cup` | 50 | 5 | 0 | kerb, extraturf, grass, sand | plays |
| `ks_nordschleife / nordschleife` | 50 | 4 | 0 | kerb, grass, extraturf, sand | plays |
| `ks_nordschleife / touristenfahrten` | 50 | 3 | 0 | kerb, extraturf, grass, sand | plays |
| `ks_nurburgring / layout_gp_a` | 5 | 2 | 0 | extraturf, kerb, grass, sand | plays |
| `ks_nurburgring / layout_gp_b` | 5 | 2 | 0 | extraturf, kerb, grass, sand | plays |
| `ks_nurburgring / layout_sprint_a` | 5 | 2 | 0 | extraturf, kerb, grass, sand | plays |
| `ks_nurburgring / layout_sprint_b` | 5 | 2 | 0 | extraturf, kerb, grass, sand | plays |
| `ks_red_bull_ring / layout_gp` | 9 | 3 | 0 | kerb, grass, sand | plays |
| `ks_red_bull_ring / layout_national` | 9 | 3 | 0 | kerb, grass, sand | plays |
| `ks_silverstone / gp` | 9 | 5 | 0 | extraturf, kerb, grass, sand | plays |
| `ks_silverstone / international` | 9 | 5 | 0 | extraturf, grass, kerb, sand | plays |
| `ks_silverstone / national` | 9 | 5 | 0 | extraturf, kerb, grass, sand | plays |
| `ks_silverstone1967` | 2 | 4 | 0 | grass, kerb, sand | plays |
| `ks_vallelunga / classic_circuit` | 1 | 2 | 0 | kerb, grass, sand | plays |
| `ks_vallelunga / club_circuit` | 1 | 2 | 0 | kerb, grass, sand | plays |
| `ks_vallelunga / extended_circuit` | 1 | 2 | 0 | kerb, grass, sand | plays |
| `ks_zandvoort` | 4 | 2 | 0 | kerb, grass, sand | plays |
| `magione` | 2 | 1 | 0 | kerb, grass, sand | plays |
| `montreal` | 0 | 1 | 0 | extraturf, grass, kerb, sand | plays |
| `monza` | 5 | 5 | 0 | extraturf, kerb, grass, sand | plays |
| `mugello` | 1 | 2 | 0 | grass, kerb, sand | plays |
| `spa` | 0 | 1 | 0 | kerb, extraturf, grass, sand | plays |
| `trento-bondone` | 0 | 1 | 0 | grass, kerb, sand | plays |
| `vhe_interlagos / gp` | 0 | 2 | 0 | extraturf, kerb, grass, sand | plays |
| `vhe_interlagos / norm` | 0 | 2 | 0 | extraturf, kerb, grass, sand | plays |

## 7. Performance

On this PC (16 logical processors), `rustyac.exe --track spa --autodrive --auto-shifter --camera cockpit
--headless --duration 30 --no-race-ini --no-shm`, without sound and with `--audio-null` (FMOD's no-sound
output: all of FMOD runs in real time, its mixer and Studio threads too, only no sound card).

| | Without sound | With sound |
|---|---|---|
| Physics rate | 333.33 Hz | 333.33 Hz |
| Physics step, average / slowest | 0.105 / 0.582 ms | 0.104 / 0.813 ms |
| Step start after its due time, average / worst | 0.244 / 1.145 ms | 0.232 / 1.190 ms |
| Steps more than 1 ms late / more than a whole step | 2 / 0 | 2 / 0 |
| Sound on the display thread, per frame (average / slowest) | - | 0.046 / 0.390 ms |
| FMOD's own threads (its figures): mixer / Studio / update | - | 2.85 % / 1.00 % / 0.22 % of one core |

The physics thread's share is a copy of the sound's inputs after each step (about 0.003 ms, inside the noise
of the figures above); the lock it shares with the display thread is held for a move. E30 at Magione, chase
camera: 0.045 ms a frame, mixer 1.98 %.
Without a clock (`--audio-wav`, where the mix itself runs inside `update` on the calling thread) a frame
takes 0.4 to 0.8 ms: a 152 s lap is mixed in about 6 s.

## 8. What is missing, and where rustyAC differs from AC

- **Nobody has heard it on a sound card yet** (the speakers had to stay quiet). The real-time path is the same
  code and the same FMOD calls as the file writer's, FMOD picks the device as AC has it do, and it ran for
  minutes on the no-sound output; but a device problem would only show on the first real run.
- **One car.** `CarAudioFMOD` is built per car and the rules for other cars are ported (the ranking by
  distance, the fades from rank 2 on, the opponents' level, their smoothed throttle and brake), but with one
  car none of them is exercised, by the oracle or otherwise. Phase 4.
- **Read-only review** (three readers against the machine code, after the comparisons; their notes are
  `re/scratch/task19/review_*.md`): fixed from it are the device name's decoding, `ksTrim`'s full set of
  characters in `GUIDs.txt`, the opponents' smoothed throttle and brake, and collision events kept for the
  next frame when a frame has no time. Left as they are: the free camera's roll-off (mode 6 sets it from up
  to two camera objects rustyAC does not have), `GameTime`'s "stabilizer" (its switch was not found), what a
  tyre's surface is before it first touches the ground, and what the game reads from its stack after a
  failed FMOD call on an event that does not exist.
- **Cameras.** The listener is rustyAC's own camera matrix (cockpit, the five drivable views, the car's
  cameras), which are not AC's cameras bit for bit (no head shake, no glance); it is taken from the blended
  picture, the velocity from the unblended car as in AC. There is no track, free or helicopter camera in
  rustyAC; their sound rules (`isExternalCamera`) are ported and were run by the oracle's track-side listener.
- **No horn** (`actionsState` bit 6 is always 0: rustyAC binds no horn key), **no down-shift protection
  beep** (the handler is ported, nothing fires it yet), **no pit-crew sounds** (`event:/common/screw`).
- **No splash-screen fade and no volume app**: the master volume is set once at the start (it ramps up
  over 0.9 s as in AC); `audio.ini` is read at the start only.
- **Pause** sets the master volume to 0 as AC's pause menu does. AC's replay mode (sound only between half
  and double speed) has no counterpart: rustyAC's `--replay` drives the car again, with its sound.
- **Lost device**: AC stops with "Audio device lost". rustyAC prints a line and plays on silently.
- **A car whose bank does not load**: AC would keep the dead events in its list and read freed memory
  later; rustyAC takes them out of the list. (No installed car is such a car.)
- `CarAvatar::openINI`'s "config data folder" (`data_<config>`) is not looked for.
- On the flat road (no track) there is no ambience, no reverb and no surface sound.
- With a window, `--audio-wav` takes each 1/60 s frame's listener from the newest picture frame.

## 9. Open questions (choices made without asking)

1. **FMOD's mix is not repeatable**, so "byte-identical WAVs" became "within the game's own run-to-run
   difference", with the numbers in 5.2. If a later FMOD knowledge gives a way to seed the Studio layer,
   the layer is the place (`system_set_advanced_settings`).
2. **Scripted answers** (section 4) for the call-log comparison. Without them 16 of the 18 drives were
   equal anyway in the last run (which ones are not changes from run to run); with them all 18.
3. **FMOD 1.08.12 itself crashes now and then in the clock-less configuration**: an access violation inside
   `fmod64.dll` (+0x8fc37 or +0xf5776), about one run in a hundred on the port's side and more often on the
   game's side at the very end of a run (in `~AudioEvent` of the pooled events), with the game's own code as
   with the port. `audio_oracle compare` repeats such a run and says so in its row. Serialising the unloads
   in the layer made it rare; I did not find what is left. It was never seen in the middle of a drive with
   the no-sound real-time output, which is how the game uses FMOD. The golden test could meet it too
   (it would then fail once; run it again).
4. **The frame grid of the checks**: 60 frames a second counted in physics steps (`floor(n * 50 / 9)`), frame
   time 1/60 s, one 800-sample block each. The game's frame time is the display's.
5. **`--headless` is silent** unless `--audio-wav`, `--audio-log` or `--audio-null` is given. `--volume`
   replaces `MASTER`. `system/cfg/audio_engine.ini ENABLE_AUDIO=0` switches rustyAC's sound off too.
6. **The listening WAVs are at your own levels** (`MASTER=1`): FMOD's 16-bit writer clips 0.01 % and 0.2 %
   of their samples. With `--volume 0.5` it would not.
7. **The second listening WAV** is the E30 sliding over the grass at Spa and coming round, from an oracle
   drive, because the test driver never spins a car by itself. If you want a real spin, record one with
   `--record` and replay it with `--audio-wav`.
8. **The scene for the track's sounds** comes from the port's own kn5 reader (the game's needs Direct3D), on
   both sides of the oracle: emitter and reverb positions were not compared with the running game.
9. **The golden file** (140 KB) is in the repository: numbers of a recorded drive and a hash, no AC file.
10. The survey calls a car "plays" by its sound files alone.

## 10. Checks to run again

```
cargo build --release --workspace && cargo test --release -p rustyac-audio      golden + logic tests
cargo clippy --workspace -- -D warnings
cd tools/car_oracle && cargo build --release && cd ../audio_oracle && cargo build --release
sh re/scratch/task19/record_tapes.sh                                           the 16 tapes (10 min)
sh re/scratch/task19/compare_all.sh                                            section 5.1 / 5.2 (15 min)
tools/audio_oracle/target/release/audio_oracle.exe dsp --count 2000             section 5.4
tools/audio_oracle/target/release/audio_oracle.exe survey                       section 6
sh re/scratch/task19/e2e.sh spa_launch cockpit                                  section 5.3 (needs oracle/audio_e2e, see the script)
```
