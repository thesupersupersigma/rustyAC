# Licensing

rustyAC is a Rust rewrite of Assetto Corsa (2014). Every source file says its license on its
first line (`SPDX-License-Identifier`). Files translated 1:1 from the game also say so on their
second line: `Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).`

Copyright (c) 2026 thesupersupersigma.

## Which part is under which license

| Part | Paths | License |
|---|---|---|
| The game, the car physics, the sound and the renderer | `crates/rustyac-game/`<br>`crates/rustyac-physics/`<br>`crates/rustyac-audio/`<br>`crates/rustyac-render/` | `GPL-3.0-or-later`: [LICENSE-GPL](LICENSE-GPL) |
| The browser preview: the wasm build, its page and its server image | `crates/rustyac-web/`<br>`web/`<br>`.dockerignore` | `GPL-3.0-or-later` |
| Tools built on the car physics, the sound or the renderer | `tools/car_oracle/`<br>`tools/chassis_compare/`<br>`tools/tyre_oracle/`<br>`tools/audio_oracle/`<br>`tools/render_oracle/`<br>`tools/web_pack/` | `GPL-3.0-or-later` |
| Notes and reports | `docs/` | `GPL-3.0-or-later` |
| Small standalone libraries and tools | `crates/rustyac-math/`<br>`crates/rustyac-content/`<br>`tools/ode_oracle/`, `tools/sctm_oracle/`, `tools/math_proof/`<br>the Python scripts `tools/*.py`, `ac_telemetry.py`, `check_telemetry.py`<br>the GitHub workflows `.github/` and the release packaging `packaging/` | `MIT OR Apache-2.0`: [LICENSE-MIT](LICENSE-MIT), [LICENSE-APACHE](LICENSE-APACHE) |
| The ODE port | `crates/rustyac-ode/` | `BSD-3-Clause`: [LICENSE-ODE](crates/rustyac-ode/LICENSE-ODE) |

**What the licenses mean in short:**
- **GPL-3.0-or-later:** use, change and share it freely, even for money, but anything you
  distribute that is built on it must also be GPL with its source available.
- **MIT OR Apache-2.0:** you pick one of the two. Both let you do almost anything, including
  using it in closed-source work, as long as the copyright notice stays. Apache-2.0 adds an
  explicit patent licence.
- **BSD-3-Clause:** ODE's own license, kept as it is.

GPL code may use the MIT / Apache / BSD parts; the reverse is not allowed, which is why
everything that links the car physics is GPL.

## Third-party code

- **ODE 0.13.1** (Open Dynamics Engine), copyright (c) 2001-2007 Russell L. Smith, under its
  BSD-style license. `crates/rustyac-ode` is a Rust translation of the part of ODE that the game
  uses. The license text is in `crates/rustyac-ode/LICENSE-ODE`.
- **MSVCR120.dll**, Microsoft's Visual C++ 2013 runtime. rustyAC loads it at run time from the
  user's system, when it is installed, to get the same maths results as the game. It is never
  shipped with rustyAC. Where a DLL cannot be loaded (the browser preview),
  `crates/rustyac-math/src/pure.rs` computes the same results: the runtime's algorithms for ten
  maths functions (AMD's LibM and Microsoft's C code), written again in Rust from the DLL's
  machine code, with the lookup tables' numbers in `pure_tables.rs`. Its second line says so:
  `Ported from MSVCR120.dll ...`. `crates/rustyac-ode/src/rcp_table.rs` is the answers of Intel's
  `rcpps` instruction, measured on a processor.
- **FMOD Studio 1.08.12** (`fmod64.dll`, `fmodstudio64.dll`), Firelight Technologies' sound
  engine, which Assetto Corsa is built on. rustyAC only calls the player's installed copy: it
  loads the two DLLs at run time from the player's own Assetto Corsa folder and plays the
  game's own sound banks from there. No FMOD DLL, header, library or sound bank is in this
  repository or in a release; the binding (`crates/rustyac-audio/src/fmod/`) is written from
  the names of the exported functions the game itself imports. Without an Assetto Corsa
  install rustyAC runs silently. FMOD is a trademark of Firelight Technologies Pty Ltd, who
  are not affiliated with rustyAC.
- The Rust crates from crates.io (`windows`, `png`, and for the browser preview `wgpu`,
  `wasm-bindgen`, `web-sys` and what they depend on) are downloaded by Cargo at build time and
  keep their own licenses. They are not in this repository. The browser preview's build also
  downloads the programs `wasm-bindgen` and binaryen's `wasm-opt` (`web/build.py`).

## No Assetto Corsa files

No file of Assetto Corsa is in this repository: not the executable, not its content, cars or
tracks, not its sounds (banks, `GUIDs.txt`, recordings of them), and not the data extracted
from `.acd` archives. To use rustyAC you point it at your own
install. The browser preview is the same: its site has none of the game in it, the page reads
the folder the player picks, and a "preview pack" is a copy somebody makes of their own
install's files with `tools/web_pack`, into a folder git ignores (`dist-web/`), for a page they
host for themselves. The oracle tools work the same way: they load `acs.exe` from your install when they run
and contain none of it.

## Trademark

Assetto Corsa is a trademark of its owners. rustyAC is not affiliated with them and is not
endorsed by them.
