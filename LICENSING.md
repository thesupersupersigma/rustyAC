# Licensing

rustyAC is a Rust rewrite of Assetto Corsa (2014). Every source file says its license on its
first line (`SPDX-License-Identifier`). Files translated 1:1 from the game also say so on their
second line: `Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).`

Copyright (c) 2026 thesupersupersigma.

## Which part is under which license

| Part | Paths | License |
|---|---|---|
| The game and the car physics | `crates/rustyac-game/`<br>`crates/rustyac-physics/` | `GPL-3.0-or-later`: [LICENSE-GPL](LICENSE-GPL) |
| Tools built on the car physics | `tools/car_oracle/`<br>`tools/chassis_compare/`<br>`tools/tyre_oracle/` | `GPL-3.0-or-later` |
| Notes and reports | `docs/` | `GPL-3.0-or-later` |
| Small standalone libraries and tools | `crates/rustyac-math/`<br>`crates/rustyac-content/`<br>`tools/ode_oracle/`, `tools/sctm_oracle/`<br>the Python scripts `tools/*.py`, `ac_telemetry.py`, `check_telemetry.py` | `MIT OR Apache-2.0`: [LICENSE-MIT](LICENSE-MIT), [LICENSE-APACHE](LICENSE-APACHE) |
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
  shipped with rustyAC.
- The Rust crates from crates.io (`windows`, `png` and what they depend on) are downloaded by
  Cargo at build time and keep their own licenses. They are not in this repository.

## No Assetto Corsa files

No file of Assetto Corsa is in this repository: not the executable, not its content, cars or
tracks, and not the data extracted from `.acd` archives. To use rustyAC you point it at your own
install. The oracle tools work the same way: they load `acs.exe` from your install when they run
and contain none of it.

## Trademark

Assetto Corsa is a trademark of its owners. rustyAC is not affiliated with them and is not
endorsed by them.
