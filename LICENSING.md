# Licensing

rustyAC is a personal Rust rewrite of Assetto Corsa (2014). It is not one work under one
license: the code I wrote myself is licensed, and the code ported from the game is not mine to
license, so it has no license at all.

Copyright (c) 2026 thesupersupersigma, for the licensed parts.

## Which part is under which license

| Part | Paths | License |
|---|---|---|
| The game, my original files | `crates/rustyac-game/` | `GPL-3.0-or-later`: [LICENSE-GPL](LICENSE-GPL) |
| Small libraries and tools I wrote | `crates/rustyac-math/`<br>the Rust tools `tools/car_oracle/`, `tools/chassis_compare/`, `tools/ode_oracle/`, `tools/sctm_oracle/`, `tools/tyre_oracle/`<br>the Python scripts `tools/*.py`, `tools/car_oracle/gen_functions.py`, `ac_telemetry.py`, `check_telemetry.py` | `MIT OR Apache-2.0`, whichever you prefer: [LICENSE-MIT](LICENSE-MIT), [LICENSE-APACHE](LICENSE-APACHE) |
| The ODE port | `crates/rustyac-ode/` | `BSD-3-Clause`: [LICENSE-ODE](crates/rustyac-ode/LICENSE-ODE) |
| The car physics | `crates/rustyac-physics/` | **None.** Derived from Assetto Corsa; no rights are granted |
| Single files ported from Assetto Corsa | listed in the next section | **None.** No rights are granted |
| Notes about the game's internals | `docs/` | None |

Everything that is not in this table has no license either.

The first line of every `.rs` file, and of every Python script named above, says which case it
is: either an `SPDX-License-Identifier` line, or the line `Ported from Assetto Corsa (acs.exe,
Kunos Simulazioni). Not covered by rustyAC's licenses`.

Two files of the game are `MIT OR Apache-2.0` instead of GPL: `crates/rustyac-game/src/dump.rs`
and `crates/rustyac-game/src/input_file.rs`. `tools/chassis_compare` compiles them in by path,
so they carry the license of the tools.

"No license" means just that: nothing in this repository gives anyone the right to copy, change
or pass on those files.

## Ported from Assetto Corsa: not covered

These are 1:1 translations of code in Kunos Simulazioni's `acs.exe`, or data that comes out of
the game. They are not mine to license, and none of rustyAC's licenses applies to them.

- `crates/rustyac-physics/`: the whole crate. The car physics, its test rigs, its tests and the
  recordings of the game in `tests/golden/`.
- `crates/rustyac-game/src/input/pad.rs`: the game's Xbox pad code.
- `crates/rustyac-game/src/input/keyboard.rs`: the game's keyboard driving code.
- `crates/rustyac-game/src/input/wheel.rs`: the game's wheel and force feedback code.
- `crates/rustyac-game/src/render/scene.rs`: the game's chase and cockpit cameras and its matrix
  routines. The car's box shapes in the same file are mine, but the file is marked as a whole.
- `tools/acd_extract.py`: the game's reader for `data.acd`.
- `tools/car_oracle/src/physics_functions.tsv`: the names and addresses of the game's physics
  functions, taken from its `acs.pdb`.
- `crates/rustyac-ode/tests/data/settle_0_300.odegold` and `slalom_3000_300.odegold`: recordings
  of the game's own car, used as test data.

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

No file of Assetto Corsa is in this repository, and none ever will be: not the executable, not
its content, cars or tracks, and not the data extracted from `.acd` archives. To use rustyAC you
point it at your own install. The oracle tools work the same way: they load `acs.exe` from your
install when they run and contain none of it.

## Trademark

Assetto Corsa is a trademark of its owners. rustyAC is not affiliated with them and is not
endorsed by them.
