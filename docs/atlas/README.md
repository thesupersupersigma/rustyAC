# Whole-game atlas of acs.exe (shallow)

A one-page index of everything in `acs.exe` 1.16.4 **outside** the car physics. Each area has its
own file with the same seven headings: what it is, main classes, entry points (name + address),
what it reads from disk, size, port difficulty, open questions. The car physics itself is mapped
in depth in [`docs/map/`](../map/car.md).

Read-only work: nothing in the game folder, the Ghidra project or a running game was changed.
The atlas was written from the local index of the executable (`tools/pdb_index.py`,
`tools/xref_index.py`, `tools/bulk_decomp.py`, queried with `tools/re_query.py`). It is shallow on
purpose: classes, entry points and files, not the logic inside.

## Index

Size = hand-written functions / bytes of machine code of the object files counted in that area
(the bytes include compiler-generated helpers and template code, so they overstate the hand-written
part). Difficulty is a relative guess for a Rust rewrite (S smallest, XL largest), not a time
estimate; the reasons are in each file's section 6.

| Area | File | What it does | Size | Difficulty | Depends on |
|---|---|---|---|---|---|
| Main loop | [`main_loop.md`](main_loop.md) | `wWinMain` → `Game` loop (messages, update, render, present); `Sim` builds the whole session; the physics thread steps every 3 ms | 313 / 145 k | M | config, renderer, audio, UI, session logic, physics; every feature through `Sim::Sim` |
| Config | [`config.md`](config.md) | `INIReader` and every settings file (`system/cfg`, Documents `cfg`), `race.ini` as the launcher-to-game contract, localisation | 153 / 74 k | S | file system only (plus the `.acd` reader for packed car data) |
| Content loading | [`content_loading.md`](content_loading.md) | Car folder layout, `data.acd`, kn5 model files, textures, `CarAvatar` (the graphics/logic twin of a physics car), driver model, LODs | 350 / 138 k | L | config, renderer, audio, car physics, main loop |
| Renderer | [`renderer.md`](renderer.md) | Direct3D 11 forward renderer: `kgl` wrapper, scene graph and materials, shadows, reflections, mirrors, sky, YEBIS post-processing, VR / triple screen, particles, car visual helpers | 1,162 / 479 k | XL | main loop, content loading, config, camera, track, UI |
| Camera | [`camera.md`](camera.md) | Cockpit, chase, car-mounted, track TV, helicopter, free and start cameras; head movement, TrackIR | 296 / 124 k | M | renderer, main loop, car state snapshot, track spline, input |
| Track | [`track.md`](track.md) | Track kn5 and data files, special `AC_…` nodes (grid, pits, timing gates, movable objects), start lights, groove, lap-position splines | 208 / 122 k | M | config, renderer (kn5), physics `Track` (`docs/map/track_surface.md`), session logic, AI line |
| AI drivers | [`ai_drivers.md`](ai_drivers.md) | `AIDriver` as a car's controls provider, run inside the physics step; `fast_lane.ai` / `pit_lane.ai` format; strategy, overtaking, pit logic | 226 / 114 k | L | the whole car physics, lap-position splines, track ray cast, session info |
| Replay | [`replay.md`](replay.md) | Ring buffer of per-car frames recorded from the physics thread, playback without physics, the replay file, ghost car, comparative telemetry | 283 / 178 k | M | physics hand-off, car state snapshot, track objects, cameras, UI, session logic |
| Audio | [`audio.md`](audio.md) | FMOD Studio banks and events fed once per frame from the car state snapshot; listener from the active camera; two custom DSPs | 168 / 73 k | M | car state snapshot, camera, scene graph, collision events, FMOD runtime |
| Input and FFB hardware | [`input_ffb.md`](input_ffb.md) | DirectInput wheel, XInput pad and keyboard providers polled on the physics thread; the force-feedback chain down to DirectInput effects | 141 / 79 k | M | physics car controls (`docs/map/steering.md`), physics thread, window, config |
| Multiplayer | [`multiplayer.md`](multiplayer.md) | Online client: TCP handshake and session messages, UDP car updates, dead-reckoned remote cars, voting, chat; local UDP telemetry and command ports | 384 / 192 k | L | physics events and clock, rigid bodies for remote cars, session logic, UI, config |
| UI | [`ui.md`](ui.md) | `ksGUI` widget library, ESC / pause / setup / race-control / end-session screens, HUD, dashboard instruments, 56 built-in apps, console | 2,141 / 1,084 k | L | renderer, main loop, config, car state snapshot, setup manager, session logic, multiplayer, replay |
| Python API | [`python_api.md`](python_api.md) | Embedded Python 3.3 with the built-in `ac` module (116 functions), C++ app SDK and plugin DLL loader | 273 / 67 k | M | UI, renderer, car state snapshot, session logic, camera, replay, multiplayer |
| Everything else | [`misc.md`](misc.md) | Nine sub-areas: race logic and sessions (L), game modes (M), pit stops and crew (M), penalties (S), driving assists (S), Steam (S), file outputs (S), car bookkeeping such as lap timing and the setup table (M), shared utilities (S) | 725 / 411 k | S–L | config, main loop, content loading, track, car physics, UI |
| **Total outside car physics** | | | **6,823 / 3,280 k** | | |

For scale: the executable has 7,453 hand-written functions in total. The car physics mapped in
`docs/map/` is about 670 of them (`ksPhysicsAC.lib` without the AI and bookkeeping files,
`ksPhysicsCoreODE.lib`, `ksTyreModel.lib`). Two small files are counted twice in the table
(`GearShiftShake` in input and renderer, `DriverEyesPositioner` in camera and UI: 41 functions).
After `misc.md`, `python tools/re_query.py coverage docs/atlas docs/map` reports no source file
that is not mentioned in some document.

## Third-party code

| Library | How it is linked | Size in the image | Used by |
|---|---|---|---|
| ODE 0.13.1 (rigid bodies, collision, OPCODE) | static (`ode_single.lib`) | 609 functions / 343 k | car physics (`docs/map/physics_engine.md`) |
| YEBIS 2 (post-processing) | static (`ppfx_dx11_x64.lib`), no source | 680 k | renderer |
| FW1FontWrapper (text on D3D11) | static | 42 k | UI, renderer |
| Fanatec SDK | static | 51 k | input (wheel LEDs and display only) |
| LibOVR (Oculus) | static, small stub | 8 k | renderer (VR) |
| FMOD Studio 1.08 | DLLs (`fmodstudio64.dll`, `fmod64.dll`) | — | audio |
| Direct3D 11, D3DX11, D3DCompiler, DXGI, DirectWrite | DLLs | — | renderer, UI |
| DirectInput 8, XInput | DLLs | — | input |
| Steamworks | DLL (`steam_api64.dll`) | — | misc (stats, achievements), multiplayer (player id) |
| Python 3.3 | DLL, loaded at run time | — | Python API |
| Visual C++ 2013 runtime (`MSVCR120`, `MSVCP120`) | DLLs | — | everything, including the maths functions the physics calls |

## How the areas stack

```
config ──► content loading ──► renderer ──► camera
   │              │               │
   │              └──► track ◄────┘
   ▼
main loop ──► physics thread (docs/map) ──► car state snapshot ──► audio, UI, replay, Python, multiplayer
                    ▲                                                   │
                    └──── input / AI drivers (controls providers) ◄─────┘ (session logic in misc.md)
```

A first drivable build needs, besides the car physics: config, the kn5 part of content loading,
a minimal renderer and camera, the track loader, the main loop and one input provider. Audio, UI
beyond a few read-outs, replay, AI, multiplayer and the Python API can all follow later; none of
them feeds back into the physics except the AI (which *is* the driver of its car) and the remote
cars' collision bodies in multiplayer.
