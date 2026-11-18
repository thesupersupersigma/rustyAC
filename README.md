# rustyAC — Assetto Corsa rewritten in Rust

[![CI](https://github.com/thesupersupersigma/rustyAC/actions/workflows/ci.yml/badge.svg)](https://github.com/thesupersupersigma/rustyAC/actions/workflows/ci.yml)
[![Latest release](https://img.shields.io/github/v/release/thesupersupersigma/rustyAC)](https://github.com/thesupersupersigma/rustyAC/releases)
[![License: GPL-3.0-or-later](https://img.shields.io/badge/license-GPL--3.0--or--later-blue)](LICENSING.md)
[![Discord](https://img.shields.io/badge/discord-join-5865F2)](https://discord.gg/trvtVNWzaf)

# Important Request
## If anyone has iRacing installed on their computer, preferrably with a bunch of cars, please message `thesuper2sigma` on discord or `thesupersupersigma@gmail.com`.

**rustyAC** is an open-source rewrite of the racing simulator **Assetto Corsa** (Kunos Simulazioni, 2014) in **Rust**. It aims to be a 1:1, **bit-exact** copy of the original game, `acs.exe` 1.16: the same physics, renderer, sound, AI, multiplayer, HUD and launcher. Every part is checked against the original game's own code.

It runs on Windows with your own Assetto Corsa install, and in the browser through WebAssembly at **[rustyac.com](https://rustyac.com)**.

> Not affiliated with or endorsed by Kunos Simulazioni or 505 Games. No game files are included: rustyAC loads the cars, tracks, sounds and shaders from **your own legal copy** of Assetto Corsa.

---

## Download and play

**Windows** needs Windows 10 or 11 (64-bit, Direct3D 11) and your own **Assetto Corsa 1.16** (the Steam version); rustyAC finds it in your Steam libraries, or set `AC_ROOT`.
1. Get the newest zip from the **[Releases page](https://github.com/thesupersupersigma/rustyAC/releases)** and unzip it somewhere you can write to (not `Program Files`).
2. Run **`rustyac-launcher.exe`**. It opens Assetto Corsa's own launcher menu, so you can pick a car, track and session (practice, quick race, race weekend, hotlap, online, special events, career), then press Drive.
3. `rustyac.exe` also runs on its own from the command line, or from a Content Manager `race.ini`. Run `rustyac.exe --help` for the options; every option, URL parameter and setting is explained in [docs/flags.md](docs/flags.md).

**Browser:** play at **[rustyac.com/play/app](https://rustyac.com/play/app/)**. Same physics as the desktop build, compiled to WebAssembly, with keyboard and gamepad. Each release also has a `-web.zip` you can host yourself (see [docs/web.md](docs/web.md)).

---

## What works

| Area | Status |
|---|---|
| **Physics** | Bit-exact with the original: tyres, suspension, drivetrain, aero, electronics (TC, ABS, ERS/KERS, DRS), damage. **119 of 123 cars** and **44 track layouts** load. |
| **Collisions** | ODE 0.13.1 ported to Rust; car-vs-car and car-vs-track contacts, the original's thread pool (same result at any thread count). |
| **Renderer** | Assetto Corsa's own D3D11 renderer with its own shaders: lighting, shadows, sky, cars, dashboards, mirrors, smoke, skid marks, MSAA / FXAA / motion blur, and YEBIS post-processing (filters, tone mapping, eye adaptation, glare, light shafts, depth of field, heat shimmer) on desktop, at every video setting. |
| **Sound** | AC's own FMOD sound banks, every car. |
| **Racing** | AI drivers, sessions, flags, penalties, results, pit stops and pit crew, replays (your real `.acreplay` files), ghost car. |
| **UI** | The in-game HUD, menus, setup editor, pit strategy, telemetry and the player apps, pixel-identical to the game. |
| **Launcher** | AC's own launcher, with Kunos' theme from your install. |
| **Game modes** | Practice, quick race, race weekend, hotlap, Time Attack, Drift, Drag Race, special events and career. |
| **Cameras and hardware** | Every camera (chase, cockpit, the car's own, track and replay cameras, free camera), triple screens, VR (OpenVR and Oculus), TrackIR, force feedback, and Steam achievements (off by default). VR, triple screens, TrackIR and Steam are proven against the game's own code with stand-in devices, but haven't been tried on real hardware yet: reports welcome. |
| **Shared memory** | AC's physics, graphics and static pages, as the game writes them, so SimHub, Crew Chief, motion rigs and other telemetry tools work with rustyAC. |
| **Apps** | AC's Python app API, so Python apps from your install run, and the dev apps. |
| **Multiplayer** | Joins real Assetto Corsa servers (acServer) on desktop, and in the browser through a WebSocket gateway. |
| **Browser** | Bit-exact physics in WebAssembly, AC's own launcher, AC's renderer on WebGPU with AC's graphics settings (and a light Debug view; post-processing (YEBIS) is not available in the browser), the HUD, menus and apps, AC's sound through FMOD, AI races and browser multiplayer. |
| **CSP** | Switch and extended car data, v1.1.0. |

**Post-processing (YEBIS, desktop):** exact for every filter AC ships, at every post-processing setting, proven by a sweep of all ten filters against the game. Not yet: the **airy disc** of the b&w and sepia filters, which the game draws only with depth of field on and a replay track camera zoomed in to about 1° or less (rustyAC writes a line to its log when that happens); and **custom filters** that use keys or values no shipped filter uses: rustyAC draws those without the key's effect and names the key once on AC's message banner and in its log (the [key table](docs/port/yebis.md) lists each key's status). Both come after v1.0.

**Known gaps in v1.0** (rare; the full list with sizes and plans is in [docs/port/leftovers.md](docs/port/leftovers.md)):
- Desktop: a custom filter's heat shimmer with chromatic aberration off draws the aberration; a few glare sizes and the heat shimmer in VR skip post-processing for that frame; the Friends Leaderboard; Start Benchmark; the photo window's FOV / roll before Start; the online mouse pit menu; `MOTION_TRACKER=RIGHT_HAND` without a right controller; AI line swaps with several cars; start lights timed by frame times; the AI app's 3D lines; the 4 s fade before an online race's end sends the car to its pits; the launcher's error text for a broken server list, HTTP redirects and server addresses that are not dotted IPv4.
- Browser: post-processing and the saturation key; saving `acos.ini`, `gameplay.ini`, `exposure.ini`, `last.ini` on exit; F5's keys-and-orbit camera; AC's two sound plug-ins, track sound occluders and online / replay sound states; font hinting and CJK fonts; Full mode on slower machines; an opponent car that won't load ends the join; Special Events sorted by name.
- Game faults (where the game crashes, loops or reads memory it never set): rustyAC does something sane instead; they are listed in [docs/port/differences.md](docs/port/differences.md#crashes-and-stops-the-game-has-that-rustyac-avoids). Every code shortcut and its verdict is in [docs/port/shortcuts.md](docs/port/shortcuts.md).

Not supported: Custom Shaders Patch (CSP) features. Cars that need CSP won't load, and cars handle like **vanilla** Assetto Corsa (CSP changes the physics).

Not ported on purpose: AC's **showroom** (`acShowroom.exe`, the car viewer). Use the one in your Assetto Corsa install: rustyAC reads the same content files as the original game, so any car or track that works in vanilla Assetto Corsa works in rustyAC too, and the original showroom already shows them.

Where rustyAC behaves differently from Assetto Corsa on purpose (it never writes into the game's folder or your Documents, it avoids crashes the game has, …) is listed in [docs/port/differences.md](docs/port/differences.md).

---

## How it's verified

rustyAC isn't "inspired by" Assetto Corsa: each part is ported from the original game's code and then **proven identical**.

- **Oracles:** test tools load the original `acs.exe` into the same process and run its own functions side by side with the Rust port, comparing every value bit for bit (physics, AI, referee, pit stops, replays).
- **Renderer and UI:** every D3D11 call and every pixel (on Microsoft's WARP software renderer) is compared frame by frame against the original.
- **Sound:** every FMOD call is logged and compared.
- **Multiplayer:** network traffic is compared message by message with the original client.
- **Launcher:** a .NET harness runs the original launcher's own code and compares every answer and every file written.

Each task's proof is written up in [`docs/port/`](docs/port).

---

## Roadmap

- **Vanilla** (v1.0, done): a complete 1:1 Assetto Corsa 1.16. Fixes for the known gaps above come in 1.0.x releases.
- **CSP** (next): Custom Shaders Patch support, so CSP cars, tracks and physics work.
- **Enhanced** (after that): the same game with upgraded physics (tyres, aero, soft-body crash damage), plus some chaos (yes, tnt and lava).

---

## Build from source

Windows, Rust through [rustup](https://rustup.rs) (the exact compiler version is pinned in `rust-toolchain.toml` and installed automatically):

```
git clone https://github.com/thesupersupersigma/rustyAC
cd rustyAC
cargo build --release
```

The release and browser builds are described in [docs/release.md](docs/release.md) and [docs/web.md](docs/web.md).

---

## Contributing and community

Pull requests are welcome. Questions, bugs and ideas: the **[rustyAC Discord](https://discord.gg/trvtVNWzaf)**, or GitHub issues.

rustyAC is a hobby project, so updates come in bursts. Much of the code is written with AI help (Claude), and every port is checked against the original game rather than trusted.

---

## License

GPL-3.0-or-later for the game and the physics, MIT OR Apache-2.0 for the small standalone libraries and tools, BSD-3-Clause for the ODE port. Details, and the FMOD exception, in [LICENSING.md](LICENSING.md).

*Assetto Corsa is a trademark of Kunos Simulazioni / 505 Games. rustyAC is an independent fan project.*