# Licensing

rustyAC is a Rust rewrite of Assetto Corsa (2014). Every source file says its license on its
first line (`SPDX-License-Identifier`). Files translated 1:1 from the game also say so on their
second line: `Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).`

Copyright (c) 2026 thesupersupersigma.

## Which part is under which license

| Part | Paths | License |
|---|---|---|
| The game, the car physics, the sound, the renderer, the game's GUI and HUD and the launcher | `crates/rustyac-game/`<br>`crates/rustyac-physics/`<br>`crates/rustyac-audio/`<br>`crates/rustyac-render/`<br>`crates/rustyac-gui/`<br>`crates/rustyac-launcher/`<br>`crates/rustyac-python/` | `GPL-3.0-or-later`: [LICENSE-GPL](LICENSE-GPL) |
| The browser preview: the wasm build, its page and its server image | `crates/rustyac-web/`<br>`web/` (but `web/fonts/`)<br>`.dockerignore` | `GPL-3.0-or-later` |
| The Selawik font for the launcher theme in the browser | `web/fonts/` | `OFL-1.1`: [web/fonts/Selawik-OFL.txt](web/fonts/Selawik-OFL.txt) |
| Tools built on the car physics, the sound or the renderer | `tools/car_oracle/`<br>`tools/chassis_compare/`<br>`tools/tyre_oracle/`<br>`tools/audio_oracle/`<br>`tools/render_oracle/`<br>`tools/web_pack/`<br>`tools/launcher_oracle/`<br>`tools/net_gold/`<br>`tools/ode_oracle/` and `tools/csp_oracle/` (both compile `tools/car_oracle/src/acs.rs`)<br>the proof and browser check scripts `tools/proofs/`, `tools/web_checks/`, the CSP fake AC folder scripts `tools/csp/` | `GPL-3.0-or-later` |
| Notes and reports | `docs/` (but its screenshots) | `GPL-3.0-or-later` |
| The screenshots in `docs/` | the `.png` / `.jpg` files under `docs/` | Not rustyAC's: pictures of Assetto Corsa's content (Kunos Simulazioni's cars, tracks and interface, as the game or rustyAC draws them), shown only to document the port. They are under none of the licenses here. |
| The project's own logos and artwork | `things/` | Copyright (c) 2026 thesupersupersigma, all rights reserved: under none of the licenses here. Ask before using them. |
| Small standalone libraries and tools | `crates/rustyac-math/`<br>`crates/rustyac-content/`<br>`crates/rustyac-dxbc/` (reads compiled shaders and writes WGSL; it contains no shader)<br>`tools/sctm_oracle/`, `tools/math_proof/`, `tools/proof_tui/`<br>the Python scripts `tools/*.py`, `ac_telemetry.py`, `check_telemetry.py`<br>the website media scripts `tools/site_media/`<br>the GitHub workflows `.github/` and the release packaging `packaging/`<br>the server kit `server/` (settings and deploy files for Kunos' `acServer`, and the server panel `server/panel/`; no Kunos file)<br>the WebSocket gateway `crates/rustyac-gateway/` (it carries the game's messages without reading them and links none of the game's code) | `MIT OR Apache-2.0`: [LICENSE-MIT](LICENSE-MIT), [LICENSE-APACHE](LICENSE-APACHE) |
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
  `Ported from MSVCR120.dll ...`. `crates/rustyac-math/src/powf4_table.rs` is the 2^(j/2048)
  table of the same runtime's vector maths (`__sse2_powf4`, which acs.exe links in): the powers of
  two at float precision. `crates/rustyac-ode/src/rcp_table.rs` is the answers of Intel's
  `rcpps` instruction, measured on a processor.
- **FMOD Studio 1.08.12** (`fmod64.dll`, `fmodstudio64.dll`), Firelight Technologies' sound
  engine, which Assetto Corsa is built on. rustyAC only calls the player's installed copy: it
  loads the two DLLs at run time from the player's own Assetto Corsa folder and plays the
  game's own sound banks from there. No FMOD DLL, header, library or sound bank is in this
  repository or in a release; the binding (`crates/rustyac-audio/src/fmod/`) is written from
  the names of the exported functions the game itself imports. Without an Assetto Corsa
  install rustyAC runs silently. FMOD is a trademark of Firelight Technologies Pty Ltd, who
  are not affiliated with rustyAC.
- **Python 3.3** (`python33.dll`, `Python33.zip`), the Python Software Foundation's
  interpreter, which Assetto Corsa runs its Python apps on. rustyAC only calls the player's
  installed copy: `crates/rustyac-python` loads the DLL at run time from the player's own
  Assetto Corsa folder (`system/x64`); no Python DLL, header, library or standard library is in
  this repository or in a release. The binding is written from the names of the 28 functions
  the game itself imports. Without the DLL rustyAC runs on without Python apps.
- **FMOD Studio 2.03.15 for HTML5** (`fmodstudio.js`, `fmodstudio.wasm`), the browser
  preview's sound engine. The website serves Firelight's unchanged files from its preview pack
  (`tools/web_pack` copies them from a local FMOD download when the pack is made); they are not
  in this repository, the web zip or a release. The page credits it: "Audio engine: FMOD Studio
  by Firelight Technologies Pty Ltd."
- **FW1FontWrapper**, copyright (c) 2011 Erik Rufelt, the library Assetto Corsa links for its
  text (DirectWrite glyphs drawn with Direct3D 11). `crates/rustyac-render/src/fw1.rs` is a
  Rust translation of the part the game uses, as the game has it (Kunos changed a few things),
  and carries the library's notice at its top, as do `fw1_atlas.rs` (its glyph sheets and
  geometry, shared by both backends) and `fw1_wgpu.rs` (the same on kgl's wgpu backend).
  `crates/rustyac-render/fw1/simple_vs.dxbc` and `simple_ps.dxbc` are the library's two HLSL
  shaders compiled once by Microsoft's shader compiler, the bytes Direct3D 11 runs. The glyphs
  on wgpu come from `ttf.rs`, rustyAC's own font reader. The library was published on CodePlex under the
  MIT License (its NuGet package still points at `fw1.codeplex.com/license`; CodePlex itself is
  offline, and the copies of the source on GitHub carry no license file).
- **Microsoft Edge WebView2**, the browser the launcher (`crates/rustyac-launcher`) shows the
  game's own launcher theme in. It is part of Windows (the WebView2 Runtime) and is never
  shipped with rustyAC; the launcher reaches it through the `webview2-com` crates (MIT), which
  carry Microsoft's WebView2 loader. The theme itself (HTML, scripts and pictures) is read from
  the player's own Assetto Corsa folder and is not in this repository.
- **Selawik 1.001**, copyright 2015 Microsoft Corporation, with Reserved Font Name Selawik,
  under the SIL Open Font License 1.1 (text in `web/fonts/Selawik-OFL.txt`, shipped beside the
  fonts in the web zip). `web/fonts/selawik-regular.ttf`, `selawik-light.ttf` and
  `selawik-bold.ttf` are built unchanged from the sources of github.com/microsoft/Selawik
  (commit 89362e8, `fontmake -o ttf`). The browser page gives them to the launcher theme only
  where the system has no Segoe UI; Selawik is not changed, so its name is kept.
- The Rust crates from crates.io (`windows`, `png`, `webview2-com`, and for the browser preview `wgpu`,
  `wasm-bindgen`, `web-sys` and what they depend on; `wgpu` and its shader validator `naga` also
  for the renderer's wgpu backend on the desktop, with `ash` and `spirv` for its Vulkan path) are
  downloaded by Cargo at build time and keep their own licenses. They are not in this repository.
  The shader proof's second software rasterizer, Google's SwiftShader (Apache-2.0), is the copy
  that comes with Chrome, copied into an ignored scratch folder on the machine that runs the
  proof; it is not in this repository and not in any release. The browser preview's build also
  downloads the programs `wasm-bindgen` and binaryen's `wasm-opt` (`web/build.py`).
- The server panel (`server/panel/`) has no runtime packages; its type check and its image use
  the npm packages `typescript` (Apache-2.0), `@types/node` (MIT) and its `undici-types` (MIT), downloaded by npm, and the
  container images `node` (MIT) and `tecnativa/docker-socket-proxy` (Apache-2.0), pulled by
  Docker. None of them is in this repository.

### Every crate in the lock files

These are all 186 third-party crates in `Cargo.lock` and the tools' own `tools/*/Cargo.lock`, by the
license each one declares on crates.io (a crate under "A OR B" may be used under either). Some
are only built for other platforms or only for the tools; none of them is in this repository.

Their full license texts and copyright notices are in [THIRD_PARTY_NOTICES.txt](THIRD_PARTY_NOTICES.txt),
which the Windows zip and the web zip both carry. `python tools/notices.py` makes it again with
[cargo-about](https://github.com/EmbarkStudios/cargo-about) 0.9.3 (`MIT OR Apache-2.0`), a
tool that is installed by hand, run on the lock files and not part of rustyAC.

- **MIT OR Apache-2.0** (121): `allocator-api2`, `android_system_properties`, `arrayvec`, `ash`, `autocfg`, `bit-set`, `bit-vec`, `bitflags`, `bumpalo`, `cassowary`, `cc`, `cfg-if`, `console_error_panic_hook`, `crc32fast`, `document-features`, `either`, `equivalent`, `errno`, `fdeflate`, `find-msvc-tools`, `flate2`, `futures-core`, `futures-task`, `futures-util`, `getrandom`, `gpu-allocator`, `half`, `hashbrown`, `heck`, `ident_case`, `indexmap`, `indoc`, `itertools`, `itoa`, `jni-sys`, `jni-sys-macros`, `jpeg-decoder`, `js-sys`, `khronos-egl`, `libc`, `litrs`, `lock_api`, `log`, `naga`, `naga-types`, `ndk-sys`, `num-traits`, `once_cell`, `parking_lot`, `parking_lot_core`, `paste`, `pin-project-lite`, `pkg-config`, `png`, `portable-atomic`, `portable-atomic-util`, `presser`, `proc-macro2`, `profiling`, `quote`, `range-alloc`, `raw-window-metal`, `renderdoc-sys`, `rustc-hash`, `rustls-pki-types`, `rustversion`, `scopeguard`, `serde`, `serde_core`, `serde_derive`, `shlex`, `signal-hook`, `signal-hook-mio`, `signal-hook-registry`, `smallvec`, `static_assertions`, `syn`, `thiserror`, `thiserror-impl`, `unicode-segmentation`, `unicode-truncate`, `unicode-width`, `version_check`, `wasm-bindgen`, `wasm-bindgen-futures`, `wasm-bindgen-macro`, `wasm-bindgen-macro-support`, `wasm-bindgen-shared`, `web-sys`, `wgpu`, `wgpu-core`, `wgpu-core-deps-wasm`, `wgpu-core-deps-windows-linux-android`, `wgpu-hal`, `wgpu-naga-bridge`, `wgpu-types`, `winapi`, `winapi-i686-pc-windows-gnu`, `winapi-x86_64-pc-windows-gnu`, `windows`, `windows-collections`, `windows-core`, `windows-future`, `windows-implement`, `windows-interface`, `windows-link`, `windows-numerics`, `windows-result`, `windows-strings`, `windows-sys`, `windows-targets`, `windows-threading`, `windows_aarch64_gnullvm`, `windows_aarch64_msvc`, `windows_i686_gnu`, `windows_i686_gnullvm`, `windows_i686_msvc`, `windows_x86_64_gnu`, `windows_x86_64_gnullvm`, `windows_x86_64_msvc`, `zeroize`.
- **MIT** (31): `castaway`, `cfg_aliases`, `compact_str`, `crossterm`, `crossterm_winapi`, `crunchy`, `darling`, `darling_core`, `darling_macro`, `dlib`, `instability`, `libm`, `lru`, `mio`, `objc2`, `objc2-encode`, `objc2-foundation`, `ordered-float`, `ratatui`, `redox_syscall`, `simd-adler32`, `slab`, `strsim`, `strum`, `strum_macros`, `tokio`, `wayland-sys`, `webview2-com`, `webview2-com-macros`, `webview2-com-sys`, `xml-rs`.
- **MIT OR Apache-2.0 OR Zlib** (9): `bytemuck`, `bytemuck_derive`, `dispatch2`, `glow`, `miniz_oxide`, `objc2-core-foundation`, `objc2-metal`, `objc2-quartz-core`, `raw-window-handle`.
- **Apache-2.0** (5): `codespan-reporting`, `gl_generator`, `glutin_wgl_sys`, `khronos_api`, `spirv`.
- **Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT** (3): `linux-raw-sys`, `rustix`, `wasi`.
- **ISC** (3): `libloading`, `rustls-webpki`, `untrusted`.
- **Zlib** (3): `foldhash`, `slotmap`, `zlib-rs`.
- **MIT OR Apache-2.0 OR BSD-2-Clause** (2): `zerocopy`, `zerocopy-derive`.
- **MIT OR Unlicense** (2): `termcolor`, `winapi-util`.
- **(MIT OR Apache-2.0) AND Unicode-3.0** (1): `unicode-ident`.
- **Apache-2.0 AND ISC** (1): `ring`.
- **Apache-2.0 OR BSL-1.0** (1): `ryu`.
- **BSD-3-Clause** (1): `subtle`.
- **CDLA-Permissive-2.0** (1): `webpki-roots`.
- **MIT OR Apache-2.0 OR 0BSD** (1): `adler2`.
- **MIT OR Apache-2.0 OR ISC** (1): `rustls`.

## FMOD (additional permission)

rustyAC plays Assetto Corsa's sound through FMOD, a closed-source audio engine by
Firelight Technologies Pty Ltd. On Windows it uses the FMOD DLLs from the player's own
Assetto Corsa install. In the browser it uses FMOD's HTML5 (WebAssembly) build, served by
the website next to the game. FMOD is not part of this repository or its releases and is
not covered by the GPL: it is under the FMOD End User License Agreement
(https://www.fmod.com/legal).

**Additional permission under GNU GPL version 3 section 7.** If you modify this Program,
or any covered work, by linking or combining it with the FMOD Engine (or a modified
version of that library), containing parts covered by the terms of the FMOD End User
License Agreement, the licensors of this Program grant you additional permission to
convey the resulting work. The FMOD Engine itself is not part of the Corresponding Source.

This permission applies to the files licensed GPL-3.0-or-later. The MIT / Apache-2.0 and
BSD-3-Clause files need no exception.

Where rustyAC plays sound through FMOD it shows the credit FMOD's licence asks for:
"Audio engine: FMOD Studio by Firelight Technologies Pty Ltd."

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
