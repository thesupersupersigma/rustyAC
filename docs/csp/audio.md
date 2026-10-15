# Audio

CSP keeps AC's FMOD Studio 1.08 sound engine and its banks, but it takes over the *front end* of it: the engine wrapper
(`AudioEngine`, `AudioEvent`, `AudioReverb`, `AudioOccluder`), the per-car sound object (`CarAudioFMOD`, every method
replaced), the track sound object and the way banks and GUID files are found. On top of that it adds its own soundbanks
(`extension/sfx/csp_*.bank`), per-car and per-track audio configuration, a Lua audio API (events from banks, files and
live streams, DSPs, per-car tweaks, volume channels), a new volume app, music "currently playing" and Mumble voice chat.
Written for the lead's `docs/csp/audio.md`; all addresses are `dwrite.dll` RVAs (image base `0x180000000`) unless they
start with `0x14` (acs.exe VAs as in `docs/atlas/audio.md`).

Evidence markers: `[ini]` shipped config comment, `[sdk]` Lua SDK, `[lua]` shipped Lua, `[dll]` seen in dwrite.dll
(string / table / decompile / own disassembly, RVA given), `[log-csp]`, `[prior]`, `[interp]` my inference.
Hook categories as in `physics_car_hooks.md`: **A** records / forwards only, **B** wraps the original (original is
called), **C** replaces (original not called, or only on one branch), **D** lookup only (CSP calls the acs.exe
function through its slot).

## 1. What it does

What the player sees and hears, and what changes in the simulation (nothing in physics: audio never feeds back).

**1.1 A different engine front end** `[dll]`
* `AudioEngine::AudioEngine` runs the original constructor and then CSP re-selects the output device itself: it reads
  `cfg/audio.ini [SETTINGS] DRIVER_NAME`, walks FMOD's driver list and calls `setDriver` on the matching index (hook
  `0x1087d60` -> `0x1087d90`; strings `DRIVER_NAME`, `SETTINGS`, "FMOD::setDriver, ID="). It stores the engine pointer
  in a global (`0x1826aadc0`), marks the master volume as "unset" (`-1.0`) and mutes the master channel group until
  the first volume is set.
* **Master volume** is computed by CSP every frame (hook `0x108a250`): window-state fade (`QUIET_WHEN`, 5 per second,
  i.e. 0.2 s), the Lua multiplier (`ac.setAudioVolumeMultiplier`, `ac.onAudioVolumeCalculation` callbacks), a start /
  un-pause fade-in (a 1 s timer that `AudioEngine::stop` re-arms), then written to FMOD's master channel group only
  when it moved by more than 0.0002 (value rounded to 1e-4). AC's own 0.9 s ramp (`rampVolume`) is switched off:
  `AudioEngine::setVolume` just stores target = current. Also `CRASH_ON_0_VOLUME` (dev) crashes if the master volume
  is ever set to 0.
* **Pitch follows time**: with `ALTER_PITCH_IN_SLOW_MOTION` the master channel group's pitch is set to the sim / replay
  speed multiplier (the second global at `0x1825e7c4c`; value read from the time object `0x1826adb40 +0x130`).
* **Update path**: with `CUSTOM_UPDATE` (hidden, default 1) CSP calls `FMOD::Studio::System::update` itself instead of
  AC's `AudioEngine::update` (which does a geometry-occlusion query per event). If the update returns FMOD result
  `0x33` ("output device not available") CSP runs a device fail-over (`0x1089d30`: "Trying to fix FMOD device:
  started with ..., switching to ..., out of candidates") and, after 3 s of failure, shows the on-screen message
  "Can't process audio / Output device is not available" for 5 s. If the track has AC audio occluders the first
  `AudioOccluder` constructor clears the flag and AC's original update is used (the `.ini` says the custom update
  "doesn't work with track audio occluders") `[dll][ini]`.
* **GUIDs and banks**: CSP has its own GUID table (hash of the lower-cased path, `\` turned into `/`, 256 chars max).
  `parseGUIDs` loads AC's `content/sfx/GUIDs.txt` and then also `extension/sfx/GUIDs.txt` (22 events, 23 buses, 6 VCAs,
  5 banks, see 1.2); for a car it parses `content/cars/<redirect>/sfx/GUIDs.txt` or, when the car's audio id starts
  with `S:`, `cache/shared_audio/<id>/GUIDs.txt`. `addBankRef` is re-implemented (`0x1088700`): after `content/sfx/common.bank`
  it loads `extension/sfx/csp_extras.bank` and `extension/sfx/csp_base.bank` (once), then `0x44cf00` loads
  `csp_surface_gravel.bank` and `csp_surface_ice.bank` lazily (flags `0x268a818` / `0x26a4b24`, probably Surfaces FX
  gravel / ice, which is off by default `[interp]`). `OLD_MASTER_SOUNDBANK` swaps AC's common bank for another file.
  `content/cars/nul/sfx/nul.bank` is ignored. `AudioEngine::hasEvent` answers "yes" at once for
  `event:/collisions/*/hit` names. `AudioEngine::addCache` (AC's pool of 12 instances per surface sound) is a
  no-op: surface events are created on demand (`0x111bb00`, "Create event: %s") `[dll][interp]`.
* **Shared / redirected soundbanks**: cars with `[AUDIO_REDIRECT]` use another car's bank and GUIDs; a car whose bank is
  missing can fetch a "shared soundbank" from `https://assets.acstuff.club/shared-soundbank/<id>` into
  `<AC>/cache/shared_audio/<id>/{GUIDs.txt,audio.bank}` ("Fetching missing soundbank", `0x5d7f10` / `0x5d8fd0`,
  `0x32ac90`). A car constructor "fixes" a missing car-bank GUID (`0x1118170`, log "Car bank GUID is missing!
  Setting to {...} bank:/...", persistent flag `guids_fix` in CSP's per-car state, bank fingerprint
  `BANK_FINGERPRINT` computed by `0x14858a0`) `[dll][interp]`. Not present on this PC: there is no `cache/shared_audio`.
* **FMOD initialisation is configurable** (section `[FMOD]` of `general.ini`, all hidden): the hook of
  `Studio::System::initialize` (`0x5d3980`) builds the Studio flags (LIVEUPDATE, ALLOW_MISSING_PLUGINS,
  SYNCHRONOUS_UPDATE) and the core flags (STREAM_FROM_UPDATE, MIX_FROM_UPDATE, 3D_RIGHTHANDED, CHANNEL_LOWPASS,
  CHANNEL_DISTANCEFILTER, PROFILE_ENABLE, VOL0_BECOMES_VIRTUAL, GEOMETRY_USECLOSEST, PREFER_DOLBY_DOWNMIX,
  THREAD_UNSAFE), `MAX_CHANNELS` (the value passed to `initialize`; AC uses 2048) and logs the result; the hook of
  `setAdvancedSettings` (`0x5d4a60`) overrides `RESAMPLER`, `PROFILE_PORT`, `VOL0_VIRTUAL_VOL` and dumps the whole
  struct; `setSoftwareChannels` (`0x5d38e0`) only logs. Owner's log: "[FMOD] Software channels: 256" (AC's
  `audio_engine.ini MAX_CHANNELS=256`), "[FMOD] System: 2048 channels, studio flags: 0x0, flags: 0x4"
  (3D_RIGHTHANDED), advanced settings all 0 except `vol0virtualvol: 0.0005` `[log-csp]`.
* **Performance hint**: `AudioEngine::getPerformanceStats` is wrapped to copy FMOD's CPU numbers into CSP's Render
  Stats app, which prints "FMOD is taking N% CPU. If the audio banks use compressed samples, enabling
  `DECOMPRESS_SAMPLES` in CSP General settings (Audio group) usually helps" `[dll]` (string `0x223dda0`).

**1.2 What CSP's own banks are for** `[dll][ini][lua]`
`extension/sfx/` holds 7 files, 24,077,739 bytes: `GUIDs.txt` (3,691 B, 57 lines), `csp_base.bank` (14,547,232),
`csp_extras.bank` (1,409,760), `csp_extras_rain.bank` (1,664,032), `csp_extras_sfp.bank` (822,112),
`csp_surface_gravel.bank` (4,943,936), `csp_surface_ice.bank` (686,976). Banks were not unpacked (rule); what is
known comes from `GUIDs.txt`, the event paths in dwrite.dll and the Lua that uses them.

| Bank | Loaded by | Content as far as the GUID file / code says |
|---|---|---|
| `csp_base` | `addBankRef` hook after `common.bank` (0x1089850) | the 23 mixer buses (`bus:/` + `grp_backfire_ext/int`, `grp_bodywork`, `grp_collisions`, `grp_door`, `grp_engine_ext/int`, `grp_gear_ext/int`, `grp_horn`, `grp_limiter`, `grp_skid_ext/int`, `grp_surfaces`, `grp_tractioncontrol_ext/int`, `grp_transmission`, `grp_turbo`, `grp_wheel_closed/open`, `grp_wind_closed/open`) and 6 VCAs (`vca_engine_ext/int`, `vca_skid`, `vca_surfaces`, `vca_transmission`, `vca_wind`); fallback events `event:/extension_common/{turn_signal_int, turn_signal_int__off, turn_signal_ext__off, wiper_car_int, wiper_car_ext, wiper_car_int_vintage, wiper_car_ext_vintage, handbrake_int, external_wind}` (9) |
| `csp_extras` | same | listed as `bank:/csp_extras`; no event of its own in the GUID file [interp: shared samples] |
| `csp_extras_rain` | Lua `weather/base/src/audio.lua` (`ac.loadSoundbank('extension/sfx/csp_extras_rain.bank')`) | `event:/extension_common/{rain_amb, rain_amb_thunder, rain_car_int, rain_car_ext, rain_grass, rain_gravel, rain_skid_int, rain_skid_ext}` + `event:/extension_rain/ambient` (9 events, parameters `intensity`, `thunder`, `speed`) |
| `csp_surface_gravel` / `csp_surface_ice` | `0x44cf00` / `0x44cf26` (lazily, see 1.1) | `event:/csp/surfaces/{skid, force, rocks, ice}` (4 events, used by `0xafe2f0` for the Surfaces FX physics) |
| `csp_extras_sfp` | Lua `alt/sfp1_render.luac` (the SFP tyre model's visual / audio script) with *inline* GUID text, not through `GUIDs.txt` | `event:/new_tyres/new_skid`; the script mutes the stock `Wheel*`, `SkidExt*`, `SkidInt*`, `TractionControl*`, `Limiter` events through `CarAudioTweak.setVolume` and plays its own. The inline GUIDs reuse the GUIDs of `csp_surface_ice` (`{6e129896-...}` bank, `{ea46972e-...}` event), so the two banks cannot be loaded as separate entries at the same time `[lua][interp]` |

Event-name families built in the DLL: `event:/extension_common/%s_int%s`, `.../%s_ext%s`, `.../%s%s`, `event:/cars/%ls/%ls`,
`.../%ls_int`, `event:/csp/surfaces/{force,ice,skid}`, `event:/collisions/%s/hit`, `event:/surfaces/%s`
(`0xafdbf0`, `0xafe2f0`, `0x111bb00`, `0x11204b0`).

**1.3 Per-car sound (CarAudioFMOD replaced)** `[dll]` (hooks `0x1118ba0..0x1120f50`, all category C except ctor / dtor)
* **Internal / external mix** per car from `USE_INTERIOR_SOUNDS` (0 no, 1 focused car only, 2 replace exterior events
  with interior, 3 interior logic everywhere) and `USE_EXTERIOR_SOUNDS` (0 original, 1 always without interior, 2 smart
  = roofless cars excluding open-wheelers, 3 undocumented, 4 always with interior; value 4 also reads
  `ALTER_REVERB_AMOUNT`). `shouldUseInternalSounds` returns a per-car flag held in CSP's per-car extension object
  (`+0x2f5` of `*(0x1826ae278 + carIndex*8)`). `ac.setExteriorAudioMix(0..1)` and per-car `MIX_EXTERIOR_AUDIO_OUTSIDE`
  (car `[AUDIO]`, read by `0xb0b710`) / `EXTERIOR_AUDIO_MIX` (animation key, `0x2299080`) feed the same blend.
* **Level of detail**: `renderAudio` (`0x111f810`) computes a state 0..4 per car (stored at car-audio `+0x4b3`): 4 = cut
  (rank beyond `LIMIT_AUDIO2`, default 8, doubled when a global `0x1825e981a` is set, or no events), 0 = interior
  focused, 1..3 intermediate; `LIMIT_AUDIO_FOR_OTHER_CARS` (0 off, 1 online only, 2 always) makes other cars skip
  their interior events at construction time. Per-car profiler zone `Car #%d/Audio`, `Cars/Car %d`.
* **Distance delay** (`DISTANCE_DELAY`, default 0): collisions (`onCarHit` 0x1120...) and gear changes are queued with a
  delay = distance / speed of sound (`0x1119d80`, uses `ac.getSoundSpeedMs`) when > 0.05 s; the continuous events use a
  history of source parameters (`0x1119e00/0x1119f10/0x1119f90`) `[interp]`.
* **Inputs**: `USE_ACTUAL_THROTTLE` / `USE_ACTUAL_BRAKE` / `USE_ACTUAL_TC` (throttle after shift cut-offs and curves,
  brake after ABS, TC intensity), `ADJUSTED_SKIDS` (skid pauses when the wheel does not slide or touch the ground),
  `ADJUSTED_WIND` (re-interprets `air_pressure`; new parameter `turbulence`; `WIND_FROM_OTHER_CARS` adds slipstream wind
  from other cars by speed difference), `GEARS_FIX` (hidden: gear sound lost with fast shifts), `DISABLE_BRAKES_SOUND`,
  `DISABLE_TC_SOUND`, `OPPONENTS_EXTRA_MULT` (0..3), `USE_EXTERIOR_SKIDS`. New FMOD parameter names seen next to the
  stock ones: `boost`, `bov_decay`, `decay`, `air_pressure`, `turbulence`, `dirtiness`, `brake_temperature`,
  `drivetrain_speed`, `suspension_damage`, `inflation`, `brake`, `speed`, `throttle` (`0x111da70`, `0x111e97a`,
  `0x111ed96`, `0x111f3ff`) `[dll]`.
* **Per-car configuration** (car `ext_config.ini` / CSP car configs): `[AUDIO_VOLUME]` (per event multiplier: `ENGINE_EXT,
  ENGINE_INT, GEAR_EXT, GEAR_INT, BODYWORK, WIND, DIRT, DOWN_SHIFT, HORN, GEAR_GRIND, BACKFIRE_EXT, BACKFIRE_INT,
  TRACTION_CONTROL_EXT, TRACTION_CONTROL_INT, TRANSMISSION, LIMITER`; seen in 7 installed cars), `[AUDIO_VOLUME_EXT]`,
  `[AUDIO_PITCH]`, `[AUDIO_3D_DISTANCE]` (`..._MIN/_MAX`), `[AUDIO_TRANSFORM]`, `[AUDIO_PARAMETER_TRANSFORM]` (a LUT per
  FMOD parameter, applied in the `AudioEvent::setParameter` hook; reader `0x14873e0`), `[AUDIO_DSP]` (DSP chain per event;
  "Invalid DSP type for `...`", `0x111a120`), `[AUDIO_PROPERTIES]` (e.g. `TURBO_THROTTLE`; inputs named `ENGINE_EXT`,
  `ENGINE_EXT_BOOST`, `ENGINE_INT_BOOST`, `WHEEL_BRAKE_TEMPERATURE`), `[AUDIO_REDIRECT] CARS=` / `SHARED=`,
  `[AUDIO_TWEAKS]` (`USE_ACTUAL_THROTTLE`, `TRANSMISSION_EXT_VOLUME_MULT`, `SKID_VOLUME_CHANGE_RATE`,
  `SKID_PITCH_CHANGE_RATE`, `BODYWORK_VOLUME_CHANGE_RATE`, `THROTTLE_CHANGE_RATE`; `0x14818b0`), `[AUDIO]`
  `DISABLE_FALLBACK_TURN_SIGNAL`, `DISABLE_FALLBACK_HANDBRAKE`. Cars without turn-signal / handbrake / wiper events get
  CSP's fallback events from `csp_base` (`0xafcd50`, `0xafdbf0`, "FMOD event is missing: `0xafda40`").
* **Damaged engine** (`ALTER_DAMAGED_ENGINE_AUDIO`, default 1): embedded Lua plugin `damaged_engine_audio` puts a high-pass
  DSP with a wet/dry amount driven by `engineLifeLeft` on `EngineExt` / `EngineInt` through `CarAudioTweak.setDSP` `[lua]`.
* **VR stereo**: `[VR_TWEAKS] AUDIO_STEREO_FIX` (0/1 only in VR/2 always) boosts min/max distance and volume of the engine
  events so they spread in the stereo field (`module.luac`, function `fixCarAudio`); `[AUDIO] VR_FIX` (hidden) takes the
  listener from the headset (`setListener` hook, global `0x1826a8607`) `[lua][dll]`.

**1.4 Reverb and occlusion** `[dll][ini]`
* `FORCE_REVERB` (-1 default, 0 = "fully deactivate"): when exactly 0, `AudioReverb` becomes inert (constructor stores two
  fields and returns; `enable`, `set3DAttributes`, `setProperties`, `setPreset` are no-ops; `hearValue` returns 0). Any
  other value keeps AC's reverb and wraps `set3DAttributes` (records position / radii), `setPreset` and `setProperties`
  (afterwards scales one reverb parameter by `value*0.01 + 0.8`). The switch is a registration gate on global byte
  `0x1826951d8` (= `FORCE_REVERB == 0`).
* `ALTER_REVERB` (default 1): reverb-send floor of an event 0.1 instead of 0.001, and the event constructor forces
  `reverbResponse` on for names starting `event:/collisions/`, `event:/surfaces/`, `event:/common/ambience` and ending
  `/horn`. `DO_NOT_TINKER_WITH_REVERB` (default 0) removes CSP's second reverb instance (FMOD
  `ChannelGroup::setReverbProperties(instance 1, wet)` from `setBaseVolume`, `0x108c230`) and every alteration of
  reverb values.
* `ADDITIONAL_REVERB` (default 1: more reverb with distance or behind walls on configured tracks), `SPATIAL_REVERB`
  (0..1, "geometry-based reverb, very experimental": `AudioEvent::set3DAttributes` offsets positions by a vector within
  30 m of geometry and stores a distance factor), `OCCLUDE_AUDIO2` (default 1: track visibility occluders also occlude
  audio; read into `0x1826951d6`, consumed by `CarAudioFMOD::renderAudio`).
* **Track config** `[AUDIO_DISTANT_OCCLUSION]` (`ACTIVE`, `DISTANCE`, `FADING` (two numbers), `REVERB_CONTRIBUTION`;
  shipped `tracks/generic/generic.ini` turns it on for Kunos tracks with 650 m, 0.3..0.7, 0.7; loaders `0x139bc20`,
  `0x139e4e0`), `[AUDIO_OCCLUDERS]` (`FADE_START`...), `[AUDIO_OCCLUSION_REVERB]` (a preset name such as `GENERIC`,
  `PADDEDCELL`), `[TRACK_OCCLUDER_BOX_n]` / `[TRACK_OCCLUDER_WALL_n]` (`DESCRIPTION`, `POINT_0..3`; `0x7ecd20`;
  used for render culling *and* audio; Spa has 2 boxes + 4 walls, Nordschleife 17 sections on this PC) edited with the
  CSP Debug "Track occluders" tool (saves `last_track_occluders.ini`; per occluder "audio: N%", "Reverb: N%",
  "Volume: N%"), `[AUDIO] AMBIENCE_VOLUME_CONDITION` (an expression such as `1 - 0.9 * saturate((SUN - 80) / 2)`; the shipped
  sample is commented out).

**1.5 Ambience, track events, surfaces, weather** `[dll][lua]`
* `EXTENDED_AMBIENCE` / `SHIFTED_AMBIENCE` (both default 1): embedded plugin `ambience_audio` downloads ambient MP3 packs
  from `https://acstuff.club/u/audio/<name>.zip` into `<AC>/cache/audio`, plays them with `ac.AudioEvent.fromFile`
  (night, wind, owls...; picks by sun angle and rain, with cooldown / chance) and uses `ac.setAudioEventMultiplier` to
  fade AC's `event:/common/ambience`; `SHIFTED_AMBIENCE` repositions AC's own ambience emitters (a native `__shiftAmbience`).
* Tracks can declare their own audio events in config (`0xe53c90`, 6 KB): keys `VOLUME`, `REVERB_RESPONSE`,
  `OCCLUDABLE`, `CAMERA_INTERIOR_MULT`, `CAMERA_EXTERIOR_MULT`, `CAMERA_TRACK_MULT`, `BOUND_TO_SPECTATORS`,
  `RELATIVE_TO`, `POSITION`, `DIRECTION`, `INPUT_KEY_n` / `INPUT_VALUE_n` (FMOD parameters driven by expressions),
  `SOUNDBANK`, `GUIDS`; logs "Event loaded: ..." / "Failed to create event: ...". Section name not read (see 8).
* Rain: the RainFX weather Lua (`extension/weather/base/src/audio.lua`) drives 9 events and mutes the grass / sand /
  skid events with `ac.setAudioEventMultiplier` while the track is wet. Fireworks (`lua/fireworks/holidays/fireworks_audio.lua`)
  load their own `extra_fireworks.bank`.
* Surfaces: surfaces config key `SURFACE_WAV_OVERRIDE` (log " Overriding audio: <surface>, WAV=...", `0x6fa2e0`) swaps
  the wav that picks the surface event (`grass.wav`, `gravel.wav`, `kerb.wav`, `sand.wav` ...); Surfaces FX physics
  fires `event:/csp/surfaces/*`.

**1.6 Voice chat, music, apps** `[lua][ini][dll]`
* **Mumble**: `[BASIC] ENABLED=1` of `mumble_plugin.ini` (1 key). The Lua app `internal/lua-apps/Mumble` unloads itself
  offline or when the server has no `[MUMBLE_INTEGRATION]` block (keys `HOST`, `PORT`=64738, `PASSWORD`, `CHANNEL`,
  `POSITIONAL_AUDIO`, `POSITIONAL_AUDIO_DIRECT_PTT`, `POSITIONAL_MAX_DISTANCE`=50, `MUTE_DISTANCE`, `USERNAME_PREFIX`).
  It starts the separate process `plugins/AcTools.MumbleClient.exe` (Mumble protocol + `libopus.dll`, `librnnoise.dll`,
  `libspeexdsp.dll`) with a memory-mapped-file link (struct with listener position / direction / up, audio source
  position, PTT, 64 input and 64 output devices, 256 connected users, 32 KB channel tree), and plays each remote
  speaker through an `ac.AudioEvent.fromFile({stream = {name, size}, use3D, useOcclusion, doppler...})`, i.e. a live PCM
  stream into FMOD positioned at the driver's head, with optional low/high-pass DSPs for "direct PTT" (radio). The
  embedded plugin `mumble_autoconnect` can launch the desktop Mumble client with a `mumble://` URL built from
  `race.ini` and a `[MUMBLE] AUTOCONNECT/AUTOCLOSE` server block. On this PC: "Extension `Mumble Plugin` disabled:
  available in online race only" and "App is not available" `[log-csp]`.
* **Currently playing music** (`music.ini`, 16 keys in 4 sections): Windows media session (`WIN10_LISTENER`, direct media
  access from AC or through `plugins/AcTools.CurrentlyPlaying.exe` + `libcurrentlyplaying.dll`), optional audio capture
  of the playing process for peaks / spectrum (`CAPTURE_AUDIO` 0/1/2, `DISPLAY_MODE`, `SPECTRUM_GAIN`; WASAPI
  `IAudioMeterInformation`), `PEAK_METER_MONITOR`, AIMP text file, internal players (Internet Radio app, stations
  list in `module.luac`). Result is `ac.currentlyPlaying()` (title, album, artist, track number / duration / position,
  shuffle / repeat, playback status) for car displays and apps; `ac.MusicData` needs `libcurrentlyplaying`.
* **CSP audio app** (`gui.ini APP_AC_AUDIO=1`, hook `FormVolumes::FormVolumes` `0x42c420`: CSP's form is created first,
  then the original): sliders for AC's levels (MASTER, ENGINE, OPPONENTS, WIND, DIRT_BOTTOM, TYRES, SURFACES,
  TRANSMISSION) and CSP's four extra channels (`WEATHER`, `TRACK`, `WIPERS`, `CAR_COMPONENTS`, written to
  `cfg/audio.ini [LEVELS_EXT]`, declared in `data_manifest.ini [AUDIO_LEVELS]`), output device list (MMDevice
  enumeration with icon), "extended range" (`extended_range_2`), "Link transmission volume to engine volume",
  Distance delay / Disable brakes toggles, bonnet / bumper internal-external (`camera_manager.ini`), presets saved as
  Content Manager `.cmpreset` files in `AcTools Content Manager\Presets\Audio Settings`.
* **Volume buttons** (`small_tweaks.ini [AUDIO_VOLUME_BUTTONS]`, 8 keys, two bindable buttons in `data_manifest.ini
  [INPUT_GROUP_7]`): modes SET_RESTORE, HOLD_TO_ALTER, PRESS_TO_ALTER, UP_DOWN with `STEP`, `TRANSITION_LAG`,
  `SAVE_VOLUME`; Lua, drives `ac.setAudioVolumeMultiplier`.
* **MMF media through FMOD** (`REDIRECT_MMF_THROUGH_FMOD2`, general.ini, commented, default off, "will be always enabled
  soon"): the Media Foundation player (`ui.MediaPlayer`, videos in car displays) outputs through a Media Foundation
  transform (`dx::CMFTTransformFMOD`, `dx::fmod_pcm_sink_impl`) into an FMOD callback-driven stream, so AC's volume
  mix and `QUIET_WHEN` also apply.

**1.7 Diagnostics and safety** `[dll][lua]`
* `[DEV] FMOD_LOGGING` (hidden, default 0): turns on 22 FMOD Studio hooks (EventDescription `LoadSampleData`,
  `CreateInstance`; EventInstance `SetParameterValue`, `GetParameterValue`, `SetPitch`, `Set/GetTimelinePosition`,
  `SetVolume`, `GetChannelGroup`, `Set3DAttributes`, `Set/GetProperty`, `Start`, `Stop`, `GetPlaybackState`, `Release`;
  System `GetEventByID`, `SetListenerAttributes`, `FlushCommands`, `Update`, `GetBufferUsage`, `GetCPUUsage`) which call
  the original and log failures (`audioFmodCallError`, "FMOD: Channel #"). The gate predicate is the byte
  `0x1825a2cb0` (the key). The embedded plugin `fmod_logging` adds a hotkey "Dump FMOD log" (writes the next 5 s to
  `<Documents>/Assetto Corsa/logs/fmod_<time>.log`), an output-channel / peak readout and a settings entry `[lua]`.
* `SILENCE_MALFUNCTIONS` (default 1, threshold `SILENCE_MALFUNCTIONS_THRESHOLD`=500): `module.luac` calls a native
  `fmod.limiter.configure` that silences FMOD channels producing runaway peaks ("audio glitches that sound like
  explosions") `[ini][lua]`. Native not located.
* `DISABLE_AUDIO` (`[DEV]`, `small_tweaks [SHOWROOM]` default 1, `[CUSTOM_AI]` in new_behaviour.ini): global byte
  `0x18268b1f6`; `lookupGUID`, `hasEvent`, `parseGUIDs`, `addBankRef`, `update` return at once `[dll]`.
* `os.openAudioOutput` (Lua) bypasses FMOD: it opens a WASAPI render endpoint (shared or exclusive; "second headphones,
  bass shakers, DualSense haptics") with its own thread; log prefix `[LuaAudio]` `[sdk][dll]`.

**1.8 Pit and replay** `[dll]`: CSP adds nothing pit-specific to audio (its PitCrew hooks `0x11324d0..0x1132b50` change crew
geometry / timing, not sounds; AC's `event:/common/screw` / `unscrew` stay as ported in Task 19). Replay: pitch follows
the replay speed (`ALTER_PITCH_IN_SLOW_MOTION`), `AudioEngine::start` / `stop` are replaced by two byte stores
(`+0x74`; `stop` also re-arms the fade-in), and `ReplayManager::updateReplayAudio` (0x14015bc00) is only a lookup
(slot `0x268c848`): CSP calls AC's function from its own code.

## 2. Game parts it replaces or extends

All 53 registrations of the audio group (attach.md 2.2): 47 hooks, 4 lookups on acs.exe symbols, 2 CSP-own type names
(`AC::FMOD::FMOD_RESULT`, `AC::FMOD::FMOD_DSP_RESAMPLER`, used to print FMOD results / resampler enums). Eight of the
hooks use replacement `0x2db1f0`, an empty function folded by the linker (also shared with 2 non-audio hooks): they
**delete** the original. `0x5a9180` returns 0.

### 2.1 AudioEngine (ksAudioFMOD.lib, 17 hooks; `AudioEngine::addCache` is registered twice)

| Symbol | acs.exe VA | CSP RVA (slot) | Cat | What it changes |
|---|---|---|---|---|
| `AudioEngine::AudioEngine` | 0x1401f6bf0 | 0x1087d60 -> 0x1087d90 (0x268ba80) | B | original, then DRIVER_NAME device selection, global engine pointer, master 0 |
| `AudioEngine::~AudioEngine` | 0x1401f79f0 | 0x2db1f0 (0x26aae18) | C | no-op: the engine is never destroyed |
| `AudioEngine::addBankRef` | 0x1401f8220 | 0x1089850 (0x268ba70) | C | own bank loader `0x1088700`; adds csp_extras / csp_base after common.bank, OLD_MASTER_SOUNDBANK, ignores `nul.bank`, log "Loading soundbank: " |
| `AudioEngine::addCache` x2 | 0x1401f8730 | 0x2db1f0 (0x26aadf0, 0x26ad080) | C | no-op (surface sound pool removed) |
| `AudioEngine::getPerformanceStats` | 0x1401f98f0 | 0x108aa00 (0x26aae10) | B | original, then copies CPU values into CSP globals |
| `AudioEngine::hasEvent` | 0x1401f9c60 | 0x1088e30 (0x26aadf8) | C | own table; `event:/collisions/*/hit` is always present |
| `AudioEngine::lookupGUID` | 0x1401fa6c0 | 0x1088cb0 (0x26aade8) | C | own hash table, 16-byte GUID, zero if unknown (logs once) |
| `AudioEngine::parseGUIDs` | 0x1401fa8c0 | 0x1089300 -> 0x1089600 (0x268ba78) | C | AC master list + `extension/sfx/GUIDs.txt`; per-car redirect / `S:` shared ids; log "Loading GUIDs: " |
| `AudioEngine::rampVolume` | 0x1401fafa0 | 0x2db1f0 (0x26aade0) | C | no-op |
| `AudioEngine::setDistanceScale` | 0x1401fb790 | 0x1088040 (0x26aadd8) | C | stores the float into `0x1825e7c44`; original not called |
| `AudioEngine::setListener` | 0x1401fbb30 | 0x1088240 -> 0x1088050 (0x268ba58) | C | own listener (ALIGNMENT_FIX, VR_FIX head pose) |
| `AudioEngine::setVolume` | 0x1401fbdf0 | 0x10886b0 (0x268ba50) | C | target = current = value (+ CRASH_ON_0_VOLUME check `0x10883d0`) |
| `AudioEngine::start` / `stop` | 0x1401fbec0 / 0x1401fbfc0 | 0x1089be0 / 0x1089bf0 (0x268ba68 / 0x268ba60) | C | byte store at `+0x74`; stop also fade timer `+0x48 = 1.0` |
| `AudioEngine::update` | 0x1401fc080 | 0x108a250 (0x268ba40) | C (cond.) | master volume, quiet-when, pitch, per-event callbacks, `Studio::System::update` or original when `CUSTOM_UPDATE=0` / occluders exist |
| `AudioEngine::updateProperties` | 0x1401fc310 | 0x1089cf0 (0x26aae00) | C | flush + CSP's per-event reverb send (`0x1089c00`: reverb instance 0 floor, instance 1 wet) |

### 2.2 AudioEvent, AudioReverb, AudioOccluder, AudioBankRef, TrackAudio

| Symbol | acs.exe VA | CSP RVA (slot) | Cat | What it changes |
|---|---|---|---|---|
| `AudioEvent::AudioEvent` (thin) | 0x1401f7580 | 0x108cad0 (0x26aae38) | B | original, then registers the event in CSP's pool (`0x108d8d0`, index at event `+0x44`, flags at `+0x40`: bit0 car/3D event, bit1 Lua-managed) |
| `AudioEvent::AudioEvent` (engine, name, group, reverb) | 0x1401f7580 | 0x108cd50 (0x26aae40) | B | name remap (car sharing audio: `event:/cars/` of another car), ALTER_REVERB forced reverb response |
| `AudioEvent::set3DAttributes` | 0x1401fb370 | 0x108cab0 -> 0x108c700 (0x26aae28) | C | position offset (SPATIAL_REVERB), NaN guards (`0x1cf4420`), calls FMOD directly (slot `0x268af90`) |
| `AudioEvent::setParameter` | 0x1401fbc40 | 0x108cac0 -> 0x108c4e0 (0x26aae30) | C | applies `[AUDIO_PARAMETER_TRANSFORM]` LUT by hashed name, then `EventInstance::setParameterValue` (`0x268afe8`); returns false only for result 0x4a |
| `AudioEvent::setBaseVolume` | 0x1401fb6d0 | 0x108d380 -> 0x108c230 (0x26aae50) | C | volume chain (per-event multiplier `+0x10`, interior/exterior `+0xac`, SPATIAL factor `+0xb0`, 30 %-per-frame smoothing of reverb x / y, reverb sends via `ChannelGroup::setReverbProperties`) |
| `AudioEvent::setBasePitch` | 0x1401fb610 | 0x108d390 (0x26aae58) | C | pitch x per-event pitch (`+0x14`), then `EventInstance::setPitch` |
| `AudioEvent::~AudioEvent` | 0x1401f7b30 | 0x108d3f0 (0x26aae60) | B | CSP bookkeeping when instance is 0, then original |
| `AudioEvent::isWithinRange` | 0x1401fa1c0 | lookup (0x268ba90) | D | used by `AudioEvent:isWithinRange` |
| `AudioReverb::AudioReverb` | 0x1401f7840 | 0x6a6be0 (0x26a74d8) | C (gated `FORCE_REVERB==0`) | stub constructor, no FMOD reverb |
| `AudioReverb::enable` | 0x1401f9030 | 0x2db1f0 (0x26a74e0) | C (same gate) | no-op |
| `AudioReverb::set3DAttributes` x2 | 0x1401fb5a0 | gate: 0x2db1f0 (0x26a74e8) else 0x6a6bf0 (0x26a7508) | C / B | no-op, else record then original |
| `AudioReverb::setProperties` x2 | 0x1401fbd70 | gate: 0x2db1f0 (0x26a74f0) else 0x6a6c90 (0x26a7510) | C / B | no-op, else original then scale one field |
| `AudioReverb::setPreset` x2 | 0x1401fbd10 | gate: 0x2db1f0 (0x26a74f8) else 0x6a6ce0 (0x26a7518) | C / B | same |
| `AudioReverb::hearValue` | 0x1401f9dd0 | 0x5a9180 (0x26a7500) | C (same gate) | returns 0 |
| `AudioOccluder::AudioOccluder` | 0x1401f77b0 | 0x58c100 (0x26a3768) | B | sets `0x18268a7ad = 1` and clears CUSTOM_UPDATE (`0x1826951d5`), then original |
| `AudioOccluder::~AudioOccluder`, `addTriangle`, `enable` | 0x1401f7c20, 0x1401f8890, 0x1401f8ff0 | lookups (0x268a988, 0x268a998, 0x268a990) | D | called by CSP's track-occluder code |
| `AudioBankRef::AudioBankRef` | 0x1400627c0 | 0x1087620 (0x26aada8) | B | swaps the bank path argument for CSP's transient override string (`0x1825e7be0`) when set (shared soundbank) `[interp]` |
| `TrackAudio::TrackAudio` | 0x1401c24c0 | 0x6a63e0 (0x26a74d0) | B | original, then singleton `0x18268b698` and two registered closures |

### 2.3 CarAudioFMOD (CarAudioFMOD.obj, 11 hooks)

| Symbol | acs.exe VA | CSP RVA (slot) | Cat | What it changes |
|---|---|---|---|---|
| `CarAudioFMOD::CarAudioFMOD` | 0x140062830 | 0x1118ba0 (0x26ac060) | B | profiler scope, guids_fix, shared-bank path (`cache/shared_audio/%ls/audio.bank`), original, then default state (volume 1.0 at `+0x13c`, level 1000.0 at `+0x98..0xa0`, `+0x49c..0x4a4`), an "AudioEngine is not set to create" guarded dummy event |
| `CarAudioFMOD::~CarAudioFMOD` | 0x1400653e0 | 0x1118b80 (0x26ac058) | B | original only when `0x18268b39b == 0` |
| `shouldUseInternalSounds` | 0x140067810 | 0x11193e0 (0x26ac068) | C | reads CSP per-car flag |
| `renderAudio` | 0x1400669b0 | 0x111f810 (0x26ad088) | C | LOD state, interior/exterior flags, delay history, DSP / events update |
| `onCarHit` | 0x140066480 | 0x11206e0 (0x26ad090) | C | immediate (`0x111fe80`) or delayed by distance (DISTANCE_DELAY) |
| `onGearChanged` | 0x140066670 | 0x1120970 (0x26ad098) | C | immediate (`0x1120860`) or delayed |
| `startStopEvents` | 0x1400678c0 | 0x1120a10 (0x26ad0a0) | C | zone "startStopEvents", body `0x111c9b0` + `0x111cd60` |
| `setEventVolumes` | 0x140067330 | 0x1120bc0 (0x26ad0a8) | C | zone "setEventVolumes", `0x111d5f0` |
| `computeTransforms` | 0x140065f20 | 0x1120d70 (0x26ad0c0) | C | `0x111af60`; keeps 2 args in globals `0x1826ad0b0/b8` |
| `updateSkids` | 0x140067f30 | 0x1120da0 (0x26ad0c8) | C | zone "updateSkids", `0x111b350` (gain 1.0) |
| `updateSurfaces` | 0x140068450 | 0x1120f50 (0x26ad0d0) | C | zone "updateSurfaces", `0x111b950`, then per-car notification list `0x1826974c0` |

Related non-audio-group hooks that touch sound: `Game::renderAudio` (0x140242dc0, hook `0x11132d0`, C: only when
the argument equals the game's current object, calls CSP's pre-pass `0x1112bb0` and walks CSP's own list `0x1826abfc0`
of enabled objects, zone "Audio playback" in `Game::run`), `FormVolumes::FormVolumes` (0x1402f8fd0, `0x42c420`, B),
`CarAvatar::resetMultVolume` (0x1400d9ab0, `0x10ae910`, C: resets two volume multipliers of a lazily created 0x4b8-byte
per-car record), `ReplayManager::updateReplayAudio` (D). Per-car state object: `*(0x1826ae278 + carIndex*8)` (vector
of 8-byte pointers; `+0x2e9`, `+0x2f1`, `+0x2f3`, `+0x2f5` flags, `+0x440` car config, `+0x550` audio sub-struct) `[dll]`.

### 2.4 FMOD C API surface that CSP uses directly (module-qualified names, attach.md 1.8)

| DLL | Names | Hooks | What |
|---|---|---|---|
| `fmod64` | 71 distinct: Channel 16, ChannelGroup 22, DSP 10, Sound 7, System 15, `FMOD_Memory_Initialize` 1 | 0 | looked up so CSP can drive FMOD itself: channel / group volume, pitch, 3D, cone, reverb sends, DSP add / remove / get, metering, `CreateSound`, `PlaySound`, `CreateDSP(ByType)`, `GetMasterChannelGroup`, `SetReverbProperties`, buffer sizes, drivers. (attach.md counted 69 and 1 hook; this count is from `dll_strings.tsv`.) |
| `fmodstudio64` | 29: EventInstance 17, System 7, EventDescription 3, Bank 2 | 22 (gated by `FMOD_LOGGING`) | the 22 hooked names are registered twice (hook inside the gate, lookup outside) |
| C++ mangled | 6: `System::initialize` (Studio), `setSoftwareChannels`, `setAdvancedSettings` hooked (unconditional); `loadBankFile`, `getSoftwareFormat`, `getChannelsPlaying` lookups | 3 | init flags, logging, device format |

Glue code for these: `0x5d0a70..0x5d7000` (52 functions, 25.6 KB), registration sites `0x23e4f0..0x2458f0` (133 small
functions). CSP's own FMOD helper classes (RTTI): `dx::CMFTTransformFMOD`, `dx::IMFTTransformFMOD`, `dx::fmod_pcm_sink`,
`dx::fmod_pcm_sink_impl`.

## 3. Config keys and Lua API

### 3.1 `extension/config/general.ini` (`cfg/extension/general.ini` overrides; owner has no audio override: all defaults)

`[AUDIO]` (33 keys, 6 of them commented / hidden) — shipped defaults; "dll" = reader found in `0x138ded0` / `0x1387350`:

| Key | Default | Meaning (shipped comment, own words) |
|---|---|---|
| `DECOMPRESS_SAMPLES` | 0 | decompress samples (less CPU, more RAM) (`0x1826951d4`) |
| `ALTER_PITCH_IN_SLOW_MOTION` | 1 | pitch follows slow motion (`0x1826a9165`) |
| `OLD_MASTER_SOUNDBANK` (hidden) | - | path of an old `common.bank` |
| `ALIGNMENT_FIX` / `VR_FIX` / `GEARS_FIX` (hidden) | 1 | listener orientation fix / VR stereo / frequent-shift gear fix |
| `DISTANCE_DELAY` | 0 | sound delay with distance |
| `DISABLE_BRAKES_SOUND`, `DISABLE_TC_SOUND` | 0 | silence brakes / TC |
| `OPPONENTS_EXTRA_MULT` | 1 | opponents multiplier 0..3 |
| `USE_INTERIOR_SOUNDS` | 0 | 0 / 1 focused / 2 replace exterior / 3 everywhere |
| `USE_EXTERIOR_SOUNDS` | 2 | 0 original / 1 always (no interior) / 4 always with interior / 2 smart |
| `ALTER_REVERB_AMOUNT` | 1.0 | reverb multiplier (with `USE_EXTERIOR_SOUNDS=4`) |
| `USE_EXTERIOR_SKIDS` | 0 | use exterior skid samples |
| `USE_ACTUAL_THROTTLE`, `USE_ACTUAL_BRAKE`, `USE_ACTUAL_TC` | 1 | actual values (shift cut-offs, ABS, TC) |
| `ADJUSTED_SKIDS`, `ADJUSTED_WIND` | 1 | more accurate skids / reinterpreted `air_pressure` |
| `WIND_FROM_OTHER_CARS` | 1 | 0..2 |
| `QUIET_WHEN` | IDLE | NONE / MINIMIZED / IDLE / BACKGROUND (global `0x1826951d0`; 2 = `IsIconic`, 3 = not foreground, 1 = idle flag) |
| `CUSTOM_UPDATE2` (hidden, commented; the DLL string is `CUSTOM_UPDATE`) | 1 | custom Studio update |
| `OCCLUDE_AUDIO2` (hidden) | 1 | track occluders also occlude audio |
| `FORCE_REVERB` | -1 | -1 default, 0 fully off |
| `DO_NOT_TINKER_WITH_REVERB` | 0 | no second reverb channel, no altered values |
| `ALTER_REVERB` | 1 | adds reverb to collisions and more |
| `SPATIAL_REVERB` | 0 | geometry-based reverb, 0..1 |
| `ADDITIONAL_REVERB` | 1 | extra reverb with distance / walls |
| `EXTENDED_AMBIENCE`, `SHIFTED_AMBIENCE` | 1 | CDN ambience / reposition AC ambience (embedded Lua plugin) |
| `ALTER_DAMAGED_ENGINE_AUDIO` | 1 | damaged engine DSP (embedded Lua plugin) |
| `SILENCE_MALFUNCTIONS`, `SILENCE_MALFUNCTIONS_THRESHOLD` | 1, 500 | silence runaway FMOD channels (embedded Lua; threshold hidden) |

`[FMOD]` (17 keys, all commented / hidden, shipped values are AC's): `MAX_CHANNELS` 2048, `LIVEUPDATE` 0,
`ALLOW_MISSING_PLUGINS` 0, `SYNCHRONOUS_UPDATE` 0, `STREAM_FROM_UPDATE` 0, `MIX_FROM_UPDATE` 0, `3D_RIGHTHANDED` 1,
`CHANNEL_LOWPASS` 0, `CHANNEL_DISTANCEFILTER` 0, `PROFILE_ENABLE` 0, `VOL0_BECOMES_VIRTUAL` 0, `GEOMETRY_USECLOSEST` 0,
`PREFER_DOLBY_DOWNMIX` 0, `THREAD_UNSAFE` 0, `RESAMPLER` 0, `PROFILE_PORT` 0, `VOL0_VIRTUAL_VOL` 0.0005 (all read in
`0x5d3980` / `0x5d4a60`).
Others in `general.ini`: `[OPTIMIZATIONS_CPU] LIMIT_AUDIO2=8` (commented), `LIMIT_AUDIO_FOR_OTHER_CARS=0`;
`[DEV] DISABLE_AUDIO=0`, `FMOD_LOGGING=0`, `CRASH_ON_0_VOLUME=0`; `[CONFIGURATIONS] REDIRECT_MMF_THROUGH_FMOD2` (commented);
`[BASIC]` `USE_MMF=1` media backend (MF / FFmpeg).

### 3.2 Other files

| File | Keys |
|---|---|
| `small_tweaks.ini` | `[AUDIO_VOLUME_BUTTONS]` `MODE` (SET_RESTORE), `LEVEL_0`, `LEVEL_1`, `STEP` 0.05, `TRANSITION_LAG` 0.9, `SHOW_MESSAGE` 1, `SAVE_VOLUME` 0 (+ `__HINT_BOTTOM`); `[SHOWROOM] DISABLE_AUDIO=1`; `[AUDIO_PEAK_FILTERS]` `INPUT`, `OUTPUT` (device name patterns, hidden) |
| `music.ini` | `[PEAK_METER_MONITOR] ENABLED`; `[WIN10_LISTENER]` `ENABLED`, `IGNORE_SOURCES`, `DIRECT_MEDIA_CONTROL`, `CAPTURE_AUDIO` (2), `DISPLAY_MODE`, `SPECTRUM_GAIN` 2.5, `INTEGRATE_INTERNAL_PLAYERS`; `[WIN10_LISTENER_TWEAKS]` `EXCLUDE_STOPPED`, `RECAPTURE_ON_MEDIA_PROPERTIES_CHANGE`, `LOWER_NONMUSIC_PRIORITY`, `CAPTURE_TIMEOUT_S` 60, `VERBOSE`; `[AIMP_TRACK_INFO_PLUGIN]` `ENABLED`, `PATH`, `SEPARATOR` |
| `mumble_plugin.ini` | `[BASIC] ENABLED=1` |
| `gui.ini` | `APP_AC_AUDIO=1` (new audio app) |
| `vr_tweaks.ini` | `AUDIO_STEREO_FIX` 0 |
| `car_instruments.ini` | `USE_AUDIO_PEAK2` (hidden; engine audio peak for vibrations) |
| `surfaces_fx.ini` | `[BASIC] ENABLED=0` (gravel / ice physics and their banks), `ALTER_SPECIAL_SURFACES`, `[DEV]` `DISALLOW_SURFACESFX` name seen in DLL |
| `track_adjustments.ini` | `[AUDIO] DISPLAY_DUMMIES=0` (show dummies for track audio) |
| `data_manifest.ini` | `[AUDIO_LEVELS]` `WEATHER`, `TRACK`, `WIPERS` 0.8, `CAR_COMPONENTS` (`RAIN` commented); `[INPUT_GROUP_7]` audio buttons |
| `cfg/audio.ini` (AC's, extended) | `[LEVELS]` (+ `MUSIC`, `UISOUNDS` exist but unused by acs.exe), `[LEVELS_EXT]` (CSP), `[SETTINGS] DRIVER_NAME`, `[SKIDS] ENTRY_POINT` |
| `tracks/generic/generic.ini` | `[AUDIO_DISTANT_OCCLUSION]` ACTIVE / DISTANCE 650 / FADING 0.3, 0.7 / REVERB_CONTRIBUTION 0.7; commented `[AUDIO] AMBIENCE_VOLUME_CONDITION` |
| server extra config | `[MUMBLE_INTEGRATION]` (9 keys), `[MUMBLE]` `AUTOCONNECT`, `AUTOCLOSE` (autoconnect plugin) |

### 3.3 Lua API (counted from `extension/internal/lua-sdk/*/lib.lua`)

| Group | Count | Available in |
|---|---|---|
| `ac.AudioEvent` (constructor from bank event, `.fromFile`, `.group`, `.keepAlive` deprecated) | 4 | 11 of 19 SDK modules (apps, car_scriptable_display, chaser_camera, cockpit_camera, new_modes, online_script, pfx_fireworks, pp_filters, tools, track_script, wfx_impl) |
| `ac.AudioEvent` methods: `setPosition`, `setVolumeChannel`, `setParam`, `setDistanceMin/Max`, `setConeSettings`, `setDSPParameter`, `isValid`, `isPlaying`, `isPaused`, `isWithinRange`, `setVolumeMultipliers`, `resume`, `resumeIf`, `stop`, `start`, `seek`, `getTimelinePosition`, `getDuration`, `getTransformationRaw`, `dispose`, `getDSPMetering`, `linkTo` + fields `volume`, `pitch`, `cameraInterior/Exterior/TrackMultiplier`, `inAutoLoopMode` | 23 methods | same 11 |
| `ac.CarAudioTweak`: `setVolume/getVolume`, `setPitch/getPitch`, `setDistanceMin/Max` + getters, `setTransform`, `setParameterTransform`, `setParameter/getParameter`, `setDSP`, `replaceAudioEvent` | 14 | 6 (apps, car_scriptable_display, joypad_assist, new_modes, online_script, tools) |
| `ac.loadSoundbank(soundbank, guids?)` (GUIDs file or inline text since preview370), `ac.isAudioReady`, `ac.setAudioEventMultiplier(surfaceWav, mult)` | 3 | apps, tools, new_modes, ... (11-16 of the audio functions per module) |
| Volume: `ac.getAudioVolume(channel, carIndex?, fallback?)`, `ac.setAudioVolume(channel, value, carIndex?, save?)` (custom channels with `'ns/Name'`), `ac.setAudioVolumeMultiplier`, `ac.onAudioVolumeCalculation` (deprecated) | 4 | all modules but `splashscreen` (3) |
| Mix / reverb / info: `ac.setExteriorAudioMix`, `ac.setReverbValue`, `ac.getSoundSpeedMs`, `ac.getAudioOutputDevice` | 4 | `setExteriorAudioMix` in apps, car_scriptable_display, new_modes, online_script, tools, joypad_assist |
| Music: `ac.currentlyPlaying()` -> `ac.MusicData` (14 fields) | 1 | apps, new_modes, tools, ... |
| `os.listAudioDevicesAsync`, `os.openAudioOutput` (WASAPI) | 2 | 11 modules |
| Enums: `ac.AudioChannel` 13 (`main, rain, weather, track, wipers, carComponents, wind, tyres, surfaces, dirt, engine, transmission, opponents`), `ac.CarAudioEventID` 29 (EngineExt .. Turbo = 0..16, Wheel 20..23, SkidInt 30..33, SkidExt 40..43), `ac.AudioDSP` 20 (oscillator ... threeEQ) | 62 values | - |
| `ui.MediaPlayer` audio members: `audio`, `hasAudio`, `muted`/`setMuted`, `volume`/`setVolume`, `balance`/`setBalance`, `pitch`/`setPitch`, `linkToSimulationSpeed` | 10 of 33 | UI brief |

Total audio-specific Lua functions: 4 + 23 + 14 + 3 + 4 + 4 + 1 + 2 = **55** (+ 62 enum values). Native implementers: `0x113e310`
/ `0x113e950` (soundbank + GUIDs, "[LuaFMOD] ..."), `0x1142b90` (event-name resolution `:own`, `:0`, `event:/nonexistent/%s`),
`0x11401e0` (5.7 KB: `fromFile` parameter parsing, keys `filename`, `stream`, `use3D`, `useOcclusion`, `loop`,
`insideConeAngle`, `outsideConeAngle`, `outsideVolume`, `minDistance`, `maxDistance`, `dopplerEffect`, `dsp`,
`channels`, `frequency`), `0x1144260` (creation, "Lua channel group"), `0x113f530` (DSP index / metering). The Lua side
is `lib_audio` (4,296 B bytecode, FFI struct `audioevent` with 14 `lj_audioevent_*` calls) and `lib_music` (1,437 B).

## 4. Files it reads

| File | By | Content |
|---|---|---|
| `<AC>/content/sfx/GUIDs.txt` (257,428 B), `common.bank` (43.8 MB), `common.strings.bank` | `parseGUIDs`, `addBankRef` | AC's master list and bank |
| `<AC>/extension/sfx/GUIDs.txt` + 6 `.bank` | same | see 1.2 |
| `<AC>/content/cars/<car>/sfx/GUIDs.txt`, `<car>.bank` (134 of the 204 installed car folders have `sfx/`) | car ctor | car events |
| `<AC>/cache/shared_audio/<id>/{GUIDs.txt, audio.bank}` | car ctor, `0x5d7f10` | shared / downloaded soundbanks (absent here) |
| `https://assets.acstuff.club/shared-soundbank/<id>` | `0x5d8fd0` | download |
| `https://acstuff.club/u/audio/<name>.zip` -> `<AC>/cache/audio` | `ambience_audio` plugin | ambience MP3 |
| `<Documents>/Assetto Corsa/cfg/audio.ini` (`[LEVELS]`, `[LEVELS_EXT]`, `[SETTINGS]`, `[SKIDS]`), `cfg/camera_manager.ini` | app, engine | volumes, device |
| `<AC>/system/cfg/audio_engine.ini` | AC | `ENABLE_AUDIO`, `MAX_CHANNELS` 256, `LIVE_UPDATE` |
| `extension/config/{general,small_tweaks,music,mumble_plugin,gui,vr_tweaks,surfaces_fx,track_adjustments,data_manifest}.ini` and the `cfg/extension` copies | config loaders | see 3 |
| `extension/config/{cars,tracks}/{generic,common,loaded}/*.ini`, `content/{cars,tracks}/*/extension/ext_config.ini` | per-car / per-track loaders | `[AUDIO_*]` sections, occluders |
| `<state>/last_track_occluders.ini` | CSP Debug tool | occluder editing |
| `extension/weather/base/src/audio.lua`, `extension/lua/fireworks/...` | Lua | RainFX / fireworks audio |
| `extension/internal/lua-apps/Mumble`, `plugins/AcTools.MumbleClient.exe` (375 KB), `libopus`, `librnnoise`, `libspeexdsp`, `plugins/AcTools.CurrentlyPlaying.exe` (28 KB), `libcurrentlyplaying.dll` (322 KB) | Lua, native | voice, music |
| `<Documents>/AcTools Content Manager/Presets/Audio Settings/*.cmpreset` | audio app | presets |
| Windows: MMDevice / WASAPI endpoints, media session | app, music | |
| Writes: `logs/custom_shaders_patch.log` ([FMOD] lines), `logs/fmod_<time>.log`, `cfg/audio.ini`, `cache/shared_audio`, `cache/audio` | | |

## 5. How it is switched on

* Always on with CSP (`general.ini [BASIC] ENABLED=1`); the audio front-end hooks are installed with the rest (the
  47 hook sites are plain records of the registry). Two registration-time gates exist in this group `[dll]`
  (registration sites `0x25ba46..0x25c446`, checked by disassembly): one `#if` gate record (kind 3, predicate
  `0x6a6bd0` = `FORCE_REVERB == 0`) followed by 6 `AudioReverb` hook records (`AudioReverb`, `enable`, `set3DAttributes`,
  `setProperties`, `setPreset`, `hearValue`), then a kind-5 record without predicate (an else branch) followed by 3
  `AudioReverb` records (`set3DAttributes`, `setProperties`, `setPreset` with the wrapping replacements), then the
  closing record: 6 + 3 = 9 of the 9 `AudioReverb` rows, exactly one branch is installed; and the `FMOD_LOGGING` gate
  (predicate `0x5d0a70`) around the 22 FMOD Studio logging hooks. Gate records carry an empty label (`0x21eaf8c`).
* Run-time switches are bytes read by the hooks: `DISABLE_AUDIO` (`0x18268b1f6`), `CUSTOM_UPDATE`
  (`0x1826951d5`, forced 0 by an `AudioOccluder`), `ALTER_REVERB` (`0x1825a2dbc` 0.1 / 0.001), `DO_NOT_TINKER...`
  (`0x1826951d9`), `FORCE_REVERB` (`0x1826951d8`), car audio flags `0x1825a2e04..e19`, `0x1826a8607/9164/9165`.
* Per car: `shouldUseInternalSounds` flag, `[AUDIO_*]` sections, `[AUDIO_REDIRECT]`, shared soundbank id.
* Per track: `[AUDIO_*]`, `[TRACK_OCCLUDER_*]`; generic Kunos-track distant occlusion is on by default.
* Online: server `[MUMBLE_INTEGRATION]`; the Mumble app and plugin are "available in online race only".
* On this PC (last log 12 Oct): all audio keys default; `extension/sfx` complete; Surfaces FX off (gravel / ice banks
  likely not loaded); Mumble disabled (offline); log shows "[FMOD] System: 2048 channels ... flags 0x4", "Loading GUIDs:
  content/sfx/GUIDs.txt", "Loading soundbank: content/sfx/common.bank", then a car bank; CSP's own bank loads do not
  produce a log line (only `addBankRef`'s first line is logged) `[log-csp]`.

## 6. Size

| Item | Number |
|---|---|
| audio-group registrations | 53 (47 hooks: AudioEngine 17, AudioEvent 7, AudioReverb 9, AudioOccluder 1, AudioBankRef 1, CarAudioFMOD 11, TrackAudio 1; lookups 4; type names 2) |
| no-op / constant replacements | 8 deletions (`0x2db1f0`) + 1 constant (`0x5a9180`) |
| FMOD C names | 71 + 29 + 6 = 106 (25 hooks, rest lookups) |
| CSP code, engine front end | `0x1086280..0x108e800` 141 functions, 33.7 KB |
| CSP code, car audio | `0x1118170..0x1122350` 108 functions, 40.8 KB (hook bodies 11: 21 KB incl. `renderAudio` 1.6 KB) |
| CSP code, FMOD glue | `0x5d0a70..0x5d7000` 52 functions, 25.6 KB; registration sites 133 functions 28 KB |
| CSP code, Lua audio natives | `0x113e310..0x1144c60` 43 functions, 26.8 KB |
| CSP code, car extras | `0xafb3f0..0xafec00` 35 functions, 14.2 KB (fallback events, surface events, channels) |
| other loaders | `0x139e4e0` 6.5 KB, `0xe53c90` 6.1 KB, `0x7ecd20` 4.1 KB, `0x5d7f10` 4.3 KB, `0x14818b0` 15.2 KB (car ext_config, partly audio) |
| banks / GUIDs | 7 files, 24,077,739 B; 22 events, 23 buses, 6 VCAs, 5 banks in `GUIDs.txt` |
| embedded Lua | plugins `ambience_audio` 7,184 B, `damaged_engine_audio` 771 B, `fmod_logging` 2,161 B, `mumble_autoconnect` 1,742 B (bytecode), libs `lib_audio` 4,296 B, `lib_music` 1,437 B, `sh_sim_ambience` 624 B; app `Mumble` (2 Lua files, 1,730 lines) |
| config | general.ini `[AUDIO]` 33 keys + `[FMOD]` 17; small_tweaks 8 + 2; music.ini 16; mumble 1; 11 car / track sections |
| Lua API | 55 functions + 62 enum values (3.3) |
| AudioEvent per-event extension record | `0x108d8d0` pool at `0x182695f18` (fields `+0x10` volume, `+0x14` pitch, `+0x18` pos, `+0x24/+0x28` reverb smoothing, `+0x2c..` matrix, `+0x78..` parameter-LUT hash map, `+0xac`, `+0xb0`) |

## 7. Port difficulty for rustyAC

**M for the front end and config keys, L for the car-audio replacement, XL for the extras. Overall L.** `[interp]`

* **Reusable** (`docs/port/audio.md`, `crates/rustyac-audio`): the whole FMOD layer (`fmod/raw.rs` with the exact-ABI,
  call-logging bindings of the 65 acs.exe imports), `AudioEngine`, event / reverb / occluder wrappers, `CarAudio`
  (events in the game's order), `TrackAudio`, the two DSPs, `sim.rs`, and `audio_oracle` (tapes + FMOD call log
  compare). Everything CSP changes is a *delta on the same FMOD calls*: which volume goes to which channel group,
  which reverb instance gets which wet value, which events exist.
* **Easy (S each)**: GUID table (second file, redirect / `S:` ids), bank list (csp_base / csp_extras / surfaces),
  `[FMOD]` init flags and logs, `DRIVER_NAME` re-selection, `DISABLE_AUDIO`, `QUIET_WHEN`, master volume path and
  `ALTER_PITCH_IN_SLOW_MOTION`, the no-op hooks (nothing to port: do not implement the ramp, the surface pool, the
  engine destructor), `FORCE_REVERB`, `ALTER_REVERB` name forcing, `DO_NOT_TINKER...`.
* **Medium (M)**: per-event extension records and the volume chain (`setBaseVolume`, `0x108c230`: needs one decompile),
  the second reverb instance, `[AUDIO_PARAMETER_TRANSFORM]` LUTs, `[AUDIO_VOLUME]` etc. tables, fallback
  turn-signal / handbrake / wiper events, `SURFACE_WAV_OVERRIDE`, track `[AUDIO_*]` and occluders (needs the
  FMOD geometry code AC already has), RainFX audio (a Lua script: easy once Lua exists, else ~300 lines of Rust),
  performance hint.
* **Hard (L)**: `CarAudioFMOD` replacement: LOD state machine, interior / exterior mix, distance delay history,
  actual throttle / brake / TC, adjusted skids / wind, skid / surface / wind parameter feeds: about 40 KB of CSP code
  of which only the 11 hook bodies are read here; 8 helpers (`0x111c9b0`, `0x111cd60`, `0x111d5f0`, `0x111da70`,
  `0x111e350`, `0x111b950`, `0x111b350`, `0x111af60`) must be decompiled first. Also the guids_fix / fingerprint and
  shared-soundbank logic (network, caching).
* **Needs the Lua brief**: 55 audio functions are thin over the above plus FMOD file / stream sounds
  (`CreateSound` with callbacks), DSP chains and metering; they depend on a Lua runtime (see `lua.md`).
* **XL / out of scope for the first pass**: Mumble (separate .NET client, Opus, RNNoise, speex, MMF protocol, server
  config), music "currently playing" (Windows media session, audio capture), CDN ambience pack download, WASAPI output,
  MMF-through-FMOD, `fmod.limiter` native, Internet Radio.
* **Proof** (what to compare): (a) extend the existing method: FMOD call log of the port vs a recording of *real
  AC + CSP*. The game-side harness (acs.exe mapped in a test program) cannot run CSP's hooks (CSP needs `acs.pdb`, the
  real process and DLL entry), so the CSP side must be recorded from a real run with a logging proxy in front of
  `fmod64.dll` / `fmodstudio64.dll` (rustyac-audio's layer already knows every export's ABI; CSP additionally
  resolves 106 names, the proxy must export those too); this needs a copy of the AC folder, because the real folder
  is read-only for the project. (b) scripted, cheap checks without CSP: `[FMOD]` log lines vs the port's computed
  initialise arguments for several `[FMOD]` key sets; GUID table contents and bank load order vs the log; reverb /
  occlusion keys by toggling `FORCE_REVERB`, `ALTER_REVERB`, `DO_NOT_TINKER...` in the proxy log (instance 0/1 wet
  values per event). (c) per car: tapes already exist (`car_oracle run --audio-tape`); compare parameter streams of
  engine / wind / skid events with `USE_*` options permuted. (d) FMOD's randomness and device selection stay out
  (as in Task 19: scripted answers).

## 8. Open questions and not mapped

* **`SILENCE_MALFUNCTIONS`** native `fmod.limiter.configure` and the peak natives (`fmod.peak.*`, `fmod.write`,
  `fmod.dump`) are called from embedded Lua; the native names are not plain strings in the image. The DSP they add
  (metering, `FMOD_DSP_GetMeteringInfo` lookup `0x268af28`) and the threshold semantics (500 = what unit) are unknown.
* **`USE_ACTUAL_TC`** and the non-obvious `USE_EXTERIOR_SOUNDS=3` value: the key string is not in the image (the
  ini documents it; probably read with an inline-immediate string in a function not decompiled).
* **What sets `0x1826a8606`** (the flag `addBankRef` tests before loading `csp_base` / `csp_extras`): only `0x1089600`
  (parse of AC's GUID list) and `0x107ce00` write it; I read `0x1089600`, not `0x107ce00`, and the conditions of the
  second store. The log of this PC shows no CSP bank line (no log is written for them), so whether `csp_base` was
  really loaded in that session is not proven.
* **Per-track audio event sections** (`0xe53c90`): the section name is built from the strings `AUDIO` and `EVENT`
  (probably `[AUDIO_EVENT_...]`); not confirmed. No installed track uses it.
* **Car `[AUDIO_*]` reader** `0x14818b0` (15 KB) and `0xb0b710`: key lists above come from strings; defaults and the
  exact semantics of `[AUDIO_DSP]`, `[AUDIO_PROPERTIES]`, `[AUDIO_REDIRECT] SHARED=` are not read.
* **`AudioEvent::AudioEvent` rename** (`0x108caf0`, 605 B): which condition makes a car-event name of another car
  resolve to an empty event; the relation to `[AUDIO_REDIRECT]` and `:own`.
* **Gate `0x21eaf8c`**: the label of gate records is an empty string; the meaning of kinds 3 / 4 / 5 comes from
  attach.md 1.7. In the `AudioReverb` block the kind-5 record has no predicate, so it acts as a plain else (attach.md
  calls kind 5 `#elif`); consistent with 6 + 3 records and only one branch installed.
* **`CUSTOM_UPDATE` vs `CUSTOM_UPDATE2`**: the shipped `general.ini` documents `CUSTOM_UPDATE2`, but the only string in
  the image (`0x22256f0`, used by the settings loader `0x138ded0` only while the occluder flag `0x18268a7ad` is 0) is
  `CUSTOM_UPDATE`. Either the ini key is never read under that spelling (a user value of `CUSTOM_UPDATE2=0` would then
  be ignored) or the loader appends the `2` at run time; not decided `[dll][interp]`. The same applies to the other
  hidden keys of `[AUDIO]` that have no literal in the image (`USE_ACTUAL_TC`, `ALTER_DAMAGED_ENGINE_AUDIO`,
  `EXTENDED_AMBIENCE`, `SHIFTED_AMBIENCE`, `SILENCE_MALFUNCTIONS*`: the last four are read by embedded Lua).
* **Which bank holds the fallback events**: `GUIDs.txt` maps GUID -> name only (22 events, 23 buses, 6 VCAs, 5 banks);
  it does not say which bank contains a given event. The "csp_base holds buses, VCAs and the 9 fallback events" row
  of 1.2 is therefore `[interp]` (the 14.5 MB size, the load order and the names fit it).
* `AudioEngine::update` custom path: the exact use of globals `0x1825e7c48..c5c`, `0x18268ba9c` (reverb second-instance
  scale) and the "output sanity" check were not traced; `AudioEngine::setDistanceScale` stores only: where CSP applies
  the scale to FMOD (`Set3DSettings` is not among the looked-up names) is unknown.
* **SFP bank GUID clash** (inline GUIDs equal to the ice bank's): is the SFP bank the same file under another name, or
  does FMOD tolerate two banks with the same GUIDs. Not tested (rule: banks not unpacked).
* Shared soundbank id derivation (`S:<id>`): where the id comes from (bank fingerprint? server?) and the CDN file format.
* The owner's `custom_shaders_patch.log` is one session; no FMOD error lines. Behaviour with the CSP banks absent
  (`extension/sfx` missing) not observed.
* Replay audio: only the lookup of `ReplayManager::updateReplayAudio` and the pitch rule were seen; AC's rule "sound only
  between 0.5x and 2x" (docs/port/audio.md section 8) is unchanged as far as I can tell.

## Evidence

* `re/scratch/csp1/hook_groups.tsv` (96 rows of group `audio_input`, 51 audio-class rows), `csp_hook_table.tsv`
  (rows 705..1043), `decomp_hooks/{1087620,1087d60,1088040,1088240,10886b0,1088cb0,1088e30,1089300,1089850,1089be0,
  1089bf0,1089cf0,108a250,108aa00,108cab0,108cac0,108cad0,108cd50,108d380,108d390,108d3f0,58c100,6a63e0,6a6be0,
  6a6bf0,6a6c90,6a6ce0,5a9180,1118b80,1118ba0,11193e0,111f810,11206e0,1120970,1120a10,1120bc0,1120d70,1120da0,
  1120f50,11132d0,42c420,10ae910,2db1f0}.c`, `decomp_req1/{138ded0,1387350}.c` (settings loaders, key to global map).
* Own read-only disassembly of `dwrite_copy.dll`: gate predicates `0x6a6bd0` (global `0x1826951d8`), `0x5d0a70`
  (global `0x1825a2cb0`), registration records `0x23e4f0..0x2458f0`, `0x25b900..0x25c4c0`, functions `0x1087d90`,
  `0x1088050`, `0x108d8d0`, `0x1089600`, `0x1089c00`, `0x44cf00`, `0x44cf26`, `0x5d38e0`, `0x5d3980`, `0x5d6f00`;
  xref scan of `0x1826a8605/6/7`.
* `dll_strings.tsv` ranges `0x2284100..0x2284a40`, `0x228aa80..0x228b240`, `0x221a5c0..0x221b440`, `0x224e300..0x224e660`,
  `0x2200a80..0x2201200`, `0x22b4a40..0x22b5500`, `0x2274c00..0x2274e60`, `0x22ac340..0x22ac3e0`; `dll_func_strings.tsv`.
* Embedded zips (`dll_zips_all.json`): plugins `ambience_audio`, `damaged_engine_audio`, `fmod_logging`,
  `mumble_autoconnect`, `alt/sfp1_render.luac`, `module.luac`, libs `lib_audio`, `lib_music`, `sh_sim_ambience`
  (string constants only listed; nothing copied).
* `<AC>/extension/sfx/*`, `config/{general,small_tweaks,music,mumble_plugin,gui,vr_tweaks,surfaces_fx,track_adjustments,
  data_manifest}.ini`, `config/tracks/generic/generic.ini`, `config/tracks/loaded/{spa,ks_nordschleife}.ini`,
  `internal/lua-sdk/ac_apps/lib.lua` (lines 462-489, 1654-1713, 3450-3477, 7545-7580, 9615-9770, 11543-11582,
  15290-15370, 15630-15730, 15885-15898, 16280-16310, 18325-18425), `internal/lua-apps/Mumble/*`,
  `weather/base/src/audio.lua`, `content/cars/{bdc_streetspec_*, vrc_formula_alpha_*}/extension/ext_config.ini`.
* `custom_shaders_patch.log` lines 86-117, 825-826; `docs/atlas/audio.md`, `docs/port/audio.md` sections 3, 4, 8.

## Decompile requests

1. `0x1088700` (1446 B): bank loader: FMOD load flags, DECOMPRESS_SAMPLES use, `.strings.bank` handling, "is not registered in GUIDs" path.
2. `0x1087960` (1018 B): what runs before the original `AudioEngine` constructor.
3. `0x108aa40`: engine initialisation after construction (called from the constructor hook).
4. `0x1089d30` (797 B): the device fail-over loop: candidate order, when it is entered.
5. `0x10883d0` (691 B): master volume setter, rounding, crash-on-zero.
6. `0x108c230` (setBaseVolume body, ~800 B across chunks `0x108c230..0x108c4cb`): exact volume formula, the two reverb sends.
7. `0x108c700` (set3DAttributes body, ~900 B): offset law (SPATIAL_REVERB) and what the stored distance factor does.
8. `0x1814246 50` region (`FUN_181424650`, LUT evaluator used by `setParameter`): LUT format of `[AUDIO_PARAMETER_TRANSFORM]`.
9. `0x108caf0` (605 B): the condition behind the "event of another car" rewrite.
10. `0x108ac10`, `0x108b0b0`, `0x108b550` (1177 B each): which callbacks hang on the three signal vectors (`0x182695f00/08/10`).
11. `0x108a050` (452 B) and `0x108d430` / `0x108d4e0`: per-frame housekeeping and Lua-event creation guard.
12. `0x1118170` (2565 B): guids_fix / missing car-bank GUID repair.
13. `0x111c9b0` (931 B) and `0x111cd60` (1305 B): car audio state build and start / stop rules.
14. `0x111d5f0` (287 B) and `0x111e350` (1578 B): event volumes and per-frame parameter update.
15. `0x111da70` (2264 B): wind / turbulence parameter law (ADJUSTED_WIND, WIND_FROM_OTHER_CARS).
16. `0x111b950` (updateSurfaces core) and `0x111bb00` (961 B): surface event creation / pooling.
17. `0x111b350` + `0x111af60` (1008 B): skid logic (ADJUSTED_SKIDS) and transforms.
18. `0x111a120` (1290 B): `[AUDIO_DSP]` creation and the allowed DSP types.
19. `0x1119d80`, `0x1119e00`, `0x1119f10`, `0x1119f90`: distance-delay history buffer.
20. `0x11204b0` (553 B) and `0x111fe80` (1334 B): collision / hit event selection and delayed play.
21. `0x1121430`, `0x1121200`, `0x1121a70`: interior / exterior switching and its fades.
22. `0x14873e0` (1590 B), `0x14858a0` (1190 B), `0xb0b710` (1820 B): car `AUDIO_TRANSFORM`, bank fingerprint, `MIX_EXTERIOR_AUDIO_OUTSIDE`.
23. `0x14818b0` (15 KB): the car `ext_config` loader: which audio keys and defaults.
24. `0x139e4e0` (6.5 KB) and `0x139bc20`: `[AUDIO_DISTANT_OCCLUSION]` / `[AUDIO_OCCLUSION_REVERB]` / `[AUDIO_OCCLUDERS]` parameters and how they act.
25. `0x7ecd20` (4 KB) and `0x7eb360`: track occluder sections -> `AudioOccluder` triangles, volume / reverb percentages.
26. `0xe53c90` (6 KB) and `0xe50a10`: track / car audio event section (name, `RELATIVE_TO`, input expressions).
27. `0x5d7f10` (4.3 KB), `0x5d8fd0`, `0x32ac90`: shared soundbank resolution, download, cache layout, `S:` ids.
28. `0xafcd50` (2.9 KB), `0xafdbf0` (1.5 KB), `0xafe2f0` (642 B), `0xafb690` (975 B): fallback events, `csp/surfaces` use, new volume channels.
29. `0x113e310`, `0x113e950`, `0x1142b90`, `0x11401e0`, `0x1144260`, `0x113f530`: the Lua audio natives (limits, stream format, ids).
30. `0x44cf00` / its caller of the flags `0x268a818`, `0x26a4b24`: when gravel / ice banks load.
31. `0x107ce00`: second writer of `0x1826a8605/6`.
32. `0x1112bb0` and `0x1112000`/`0x1112e60`: the CSP game-object list behind `Game::renderAudio` and the `TRACK_AUDIO` object.
33. `0x1811be450` (`0x11be450`): the CSP audio app entry that `FormVolumes::FormVolumes` calls.
34. The native that registers `fmod.limiter.configure` / `fmod.peak.*` / `fmod.dump` (find through the `__util` native table).
