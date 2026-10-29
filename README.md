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

**Windows**
1. Get the newest zip from the **[Releases page](https://github.com/thesupersupersigma/rustyAC/releases)** and unzip it somewhere you can write to (not `Program Files`).
2. Run **`rustyac-launcher.exe`**. It opens Assetto Corsa's own launcher menu, so you can pick a car, track and session (practice, quick race, race weekend, hotlap, online, special events, career), then press Drive.
3. `rustyac.exe` also runs on its own from the command line, or from a Content Manager `race.ini`. Run `rustyac.exe --help` for the options.

**Browser:** play at **[rustyac.com/play](https://rustyac.com/play)**. Same physics as the desktop build, compiled to WebAssembly, with keyboard and gamepad. Each release also has a `-web.zip` you can host yourself (see [docs/web.md](docs/web.md)).

---

## What works

| Area | Status |
|---|---|
| **Physics** | Bit-exact with the original: tyres, suspension, drivetrain, aero, electronics (TC, ABS, ERS/KERS, DRS), damage. **119 of 123 cars** and **44 track layouts** load. |
| **Collisions** | ODE 0.13.1 ported to Rust; car-vs-car and car-vs-track contacts, the original's thread pool (same result at any thread count). |
| **Renderer** | Assetto Corsa's own D3D11 renderer with its own shaders: lighting, shadows, sky, cars, dashboards, mirrors, smoke, skid marks, MSAA / FXAA / motion blur. Post-processing (YEBIS) is not done yet. |
| **Sound** | AC's own FMOD sound banks, every car. |
| **Racing** | AI drivers, sessions, flags, penalties, results, pit stops and pit crew, replays (your real `.acreplay` files), ghost car. |
| **UI** | The in-game HUD, menus, setup editor, pit strategy, telemetry and the player apps, pixel-identical to the game. |
| **Launcher** | AC's own launcher, with Kunos' theme from your install. |
| **Multiplayer** | Joins real Assetto Corsa servers (acServer) on desktop, and in the browser through a WebSocket gateway. |
| **Browser** | Bit-exact physics in WebAssembly, AC's own launcher, AC's renderer on WebGPU with AC's graphics settings (and a light Debug view), the HUD, menus and apps, AC's sound through FMOD, AI races and browser multiplayer. |

Not supported: Custom Shaders Patch (CSP) features. Cars that need CSP won't load, and cars handle like **vanilla** Assetto Corsa (CSP changes the physics).

Not ported on purpose: AC's **showroom** (`acShowroom.exe`, the car viewer). Use the one in your Assetto Corsa install: rustyAC reads the same content files as the original game, so any car or track that works in vanilla Assetto Corsa works in rustyAC too, and the original showroom already shows them.

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

- **Vanilla** (now): a complete 1:1 Assetto Corsa. Left: the full renderer, HUD and sound in the browser; AC's Python app API; dev and modding tools; game modes; the rest of the cameras; VR and triple screens; Content Manager support.
- **Enhanced** (later): the same game with upgraded physics (tyres, aero, soft-body crash damage), plus some chaos (yes, tnt and lava).

---

## Build from source

Windows, Rust stable:

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