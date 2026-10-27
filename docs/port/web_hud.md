# Task 40: HUD, menus and apps in the browser

## Resume here

- Done: step 0; step 1 (FW1 text on wgpu); steps 2-4 (AC's GUI in Full mode
  in the page, hidden apps, the GUI at full size under a render scale):
  11 GUI sequences give the same kgl log and control states natively on
  D3D11, natively on wgpu and in the page.
- `run_all.sh --quick` (129 / 129) and `--changed v0.39.0` (1599 / 1599)
  identical.
- Next: v0.40.0; leftovers (see "Not done") go to Task 40b.
- Scratch: `re/scratch/task40/`. Disk free: 36 GB at start, 30 GB at end.

## Step 0

- `tools/proofs/lib.sh`: multi cases with `--ai-line` no longer use the
  recording cache, like `--replay` / `--ghost` / `--telemetry`.
- `web/app.js`: High = AC's `cfg/video.ini` as shipped (AC keeps it at
  `cfg/video.ini`, not `system/cfg/`; it has no size, so 1920 x 1080 stays).
  A fresh browser has no video.ini, so it reads as High. Medium and Low keep
  their 0.39 values (the game's defaults with no video.ini, changed as before).

## Step 1: text on wgpu

How it works:

- `crates/rustyac-render/src/fw1_atlas.rs`: FW1's glyph sheets, atlas,
  geometry (quads by sheet, buffer fills) and shader constants, now shared:
  `fw1.rs` (D3D11) keeps only DirectWrite's layout and glyphs and the
  Direct3D 11 calls.
- `crates/rustyac-render/src/ttf.rs`: a TrueType reader (no font crate is
  available offline): cmap, hmtx, GPOS pair kerning, glyf outlines (simple
  and composite), DirectWrite's face choice (family, weight, italic), its
  layout of FW1's strings and a glyph rasteriser.
- `crates/rustyac-render/src/fw1_wgpu.rs`: FW1 on kgl's wgpu backend: same
  atlas, same quads, FW1's own HLSL compiled once (`fw1/simple_vs.dxbc`,
  `simple_ps.dxbc`, the same bytes d3dcompiler_43 gives at run time on
  D3D11) and translated by rustyac-dxbc like every game shader.
- The kgl call log: D3D11's FW1 never went through kgl, so its text was
  missing from the log. Both backends now write the same `fw1 …` lines
  (font made, string drawn, glyphs placed: x, y, glyph, colour; buffer
  fills; draws per sheet; sheet uploads). FW1's own kgl calls on wgpu run
  under `kgl_log::quiet` so the names of the game's objects stay the same.
- kgl_wgpu: `FW1_BLEND` (one / inverse source alpha, alpha one / zero),
  `save_bound` / `restore_bound` (FW1_RESTORESTATE), `viewport`,
  `update_texture` (UpdateSubresource of a box), R8G8B8A8_UNORM input
  elements, `set_shader` takes `&self`.

DirectWrite's layout, as found against dumps of it on this PC
(`RUSTYAC_FW1_GLYPHS=<file>` writes every glyph and layout from either
backend): baseline = y + usWinAscent; the run starts on the nearest whole
pixel and each glyph on the pixel nearest its pen; GPOS kerning only (the
old `kern` table is not used); no ligatures; trailing spaces are a run of
their own outside the alignment width; a new run where the script changes;
lines at line feeds (CR LF one break) every winAscent + winDescent (+ hhea extra).

Proof (render_oracle `gui-run --side port`, D3D11 build vs `--features
wgpu` on the GPU, 1920 x 1080; `tools/web_checks/task40/d3d_truth.sh`,
`wgpu_run.sh`, `cmp_log.py`, `glyph_close.py`):

| sequence | kgl log lines | equal (but sheet uploads) | control states |
|---|---|---|---|
| text | 236 | 2 lines differ: the CJK / Hangul / snowman string | same |
| base | 12153 | yes | same |
| widgets_a | 6801 | yes | same |
| hud_messages | 5071 | yes | same |
| hud_cars | 4872 | yes | same |
| esc_menu | 41604 | yes | same |
| esc_menu#it | 41604 | yes | same |
| pause_menu#de | 7088 | yes | same |
| pit_menu | 44940 | yes | same |
| app_essentials | 183531 | yes | same |
| app_telemetry | 206959 | yes | same |
| console | 7060 | yes | same |
| end_race_12 | 25258 | yes | same |

Layouts: every string of these sequences is placed the same (49 000+
strings, `ttf::tests::layouts_match_directwrite`) except that CJK one:
DirectWrite draws characters the game's fonts lack with a Windows font,
wgpu draws the font's missing-glyph box.

Glyph pixels (1674 glyphs, `glyph_close.py`): same box 81 %, mean
difference 9.6 / 255, 78 % of pixels within 8, 92.6 % within 32, the same
ink (1.007). DirectWrite runs the fonts' TrueType hinting instructions;
ttf.rs does not (a TrueType interpreter is a project of its own), so stems
fall a little softer and sometimes a row off. The sheet uploads differ with
the boxes (313 on D3D11, 301 on wgpu: a few tiny glyphs are empty here).

setup_* sequences need the AC side to prepare their car files, so they are
not in this table (the AC side runs acs.exe, which this task never runs).

## Step 2: AC's GUI in Full mode in the page

How it works:

- `rustyac-game`: `render::gui`, `gui_apps` and `console` now build for
  the page and the `wgpu` feature too (they were Windows-only glue); in
  `render::ac` the GUI pass (game screen, forms, apps bar, HUD objects,
  post-GUI fade) runs on every backend. What touched the disk directly now
  goes through the content layer (`vfs`), so the page's files are found:
  the setup editor's track list, the cars' `ui_car.json`, the help `.tag`,
  the apps' content files, `cfg` files, and the setup help
  (`rustyac_physics::data::ini::read_text_utf8`). The clock is the
  browser's in the page (`ac_replay::unix_seconds`; `SystemTime` panics in
  wasm).
- `rustyac-web/src/full.rs`: `gui_setup` gives the GUI what rustyac.exe's
  `make_ac` gives it; `GuiLoop` carries the GUI's asks back to the session
  each frame as rustyac.exe's frame loop does (pause, next / restart
  session, back to pits, the pits' lock and drive, the setup editor's jobs
  and its car, the pit strategy page, cameras, exit), and routes keys as
  rustyac.exe's window does (Home console, Tab table, Esc pause menu, the
  controls.ini Ctrl keys for apps / desktops / names / damage / restart).
- `web.rs` / `web/app.js`: a Full-mode drive has AC's GUI and HUD; the
  page's own HUD hides. Keys go to the GUI first (Esc opens AC's menu; its
  Exit leaves the drive), typed characters to the console and text boxes,
  the canvas's mouse (move, buttons, wheel) to the GUI.
- `content.rs`: the page's file list has AC's `content/gui` (textures;
  `.psd` files left out) and `system/locales`.

Proof, deviating from the prompt: the render oracle's GUI sequences (Tasks
29–34) drive the widgets on their own with an AC side next to them; they
are 6 000 lines of the oracle tied to acs.exe and the disk, so they were not
put into the page. The page's proof instead runs the whole GUI as the game
runs it: Full mode's fixed frame (`?fullproof=<frames>&fullgui=<script>`,
natively `full_frame --gui <script>`) with a `--gui-do` script, and the kgl
log ends with every control's state (name, shown, place, text, mouse,
highlight), the mouse, the desktop and the GUI's asks of the session. Each
sequence ran three ways: native D3D11, native wgpu (GPU), the page
(headless Chrome on the GPU); 1280 x 720, ks_ferrari_f2004 at magione, the
High video.ini (`tools/web_checks/task40/prove.sh`, `cmp_log.py`).

| sequence | what | script | kgl log lines | D3D11 = wgpu | wgpu = page | control states |
|---|---|---|---|---|---|---|
| hud3 | HUD with Essentials, Tyres, Pedals open | `show:essentials;show:tyres;show:pedals` | 24062 | yes | yes | same (all 3) |
| sidebar | the apps bar slid out (20 frames) | the same + `mouse:1279:360` | 115722 | yes | yes | same (all 3) |
| esc | the pause / ESC menu | `esc` | 21434 | yes | yes | same (all 3) |
| pits | the pits menu | `pits` | 22199 | yes | yes | same (all 3) |
| setup | the setup editor, Setup I/O | `pits;click:47:273` | 24395 | yes | yes | same (all 3) |
| pitlane | practice under the referee | `--sessions p10` | 22689 | yes | yes | same (all 3) |
| results | the session results | `results`, `--sessions p10` | 22349 | yes | yes | same (all 3) |
| console | the console with `help` typed | `console;type:help` | 21387 | yes | yes | same (all 3) |
| tab | the TAB table | `tab` | 21211 | yes | yes | same (all 3) |
| desktop | a second app desktop | `show:essentials;desktop;show:tyres` | 23861 | yes | yes | same (all 3) |
| names | driver names, damage | `names;damage` | 21554 | yes | yes | same (all 3) |

"=" is every line equal but the `fw1 atlas update` lines (the glyph atlas's
bytes, step 1); the control states are the log's `gui` lines (1 500+
controls each).

Found on the way: in Full mode the page makes the game twice (after the
renderer names its extra files) and the second make lost `?sessions=`
(since 0.38.8); fixed.

## Step 3: apps the page can't run

`full::PAGE_HIDDEN_APPS`: made but without an icon in the apps bar and never
shown (`GuiSetup::hidden_apps`; rustyac.exe hides none):

| app | why |
|---|---|
| volumes | the page's sound does not take its levels |
| ffb | a browser has no force feedback |
| eyes | the page's cameras do not take its eye point |
| gamestats, rstats | frame statistics only Direct3D 11 counts |
| telemetry, setupdebug | fed from rustyac.exe's physics thread |

AC's Python apps (`apps/python`: Chat, gMeter, system and any mod) are not
made anywhere, in the page or in rustyac.exe (Task 42).

## Step 4: the GUI at full resolution under a render scale

With AC's GUI, the page no longer shrinks the canvas for the render scale:
the 3D picture is drawn at the scale (`Game::set_render_scale`), then
`Kgl::begin_gui_screen` stretches it into a full-size screen, the 2D pass
draws there at the canvas's size, and `end_gui_screen` presents that
(kgl's wgpu backend; `AcRenderer::gui_size`). At 100 % nothing changes and
the kgl log is the same as before.

Low at 75 % (`re/scratch/task40/shots/6_low_75pct_crop.jpg`, an unscaled
corner of a 1920 x 1080 page): the track is soft, the apps' text sharp.

## Pictures and frame rates

Screenshots (`re/scratch/task40/shots/`, each under 300 KB, the page in
headless Chrome on the GPU): `1_hud_driving.jpg` (autodrive, Essentials /
Tyres / Pedals), `2_esc_menu.jpg`, `3_apps_sidebar.jpg` (the same three apps,
the bar slid out), `4_setup_editor.jpg`, `5_results.jpg`,
`6_low_75pct_crop.jpg`.

Frame rate, the page on this PC's GPU (Chrome, Vulkan, 1920 x 1080), AC's HUD
and three apps open, autodrive at magione, 40 s each, three runs
(`tools/web_checks/task40/fps.sh`; `?guido=` opens the apps):

| preset | fps, three runs | median | work ms / frame (median) |
|---|---|---|---|
| High (AC's shipped video.ini) | 63.7, 54.0, 47.7 | 54.0 | 17.8 |
| Low (render scale 75 %) | 58.6, 59.1, 58.7 | 58.7 | 16.3 |

High swings run to run (one run reached the page's ~60-64 frame cap);
Low holds near the cap.

## Proofs (`tools/proofs/run_all.sh`)

`lib.sh` changed, so `--changed v0.39.0` took every area.

| area | `--quick` cases | identical | `--changed v0.39.0` cases | identical |
|---|---|---|---|---|
| physics | 9 | 9 | 113 | 113 |
| fresh | 47 | 47 | 657 | 657 |
| sound | 4 | 4 | 22 | 22 |
| render | 10 | 10 | 107 | 107 |
| gui | 15 | 15 | 123 | 123 |
| replays | 5 | 5 | 29 | 29 |
| net | 16 | 16 | 80 | 80 |
| browser | 9 | 9 | 226 | 226 |
| ai_page | 3 | 3 | 3 | 3 |
| shaders | 4 | 4 | 4 | 4 |
| launcher | 1 | 1 | 12 | 12 |
| dumps | 6 | 6 | 223 | 223 |
| all (wall clock) | 129 (36 min) | 129 | 1599 (7 h 11 min) | 1599 |

Clippy `-D warnings` clean: native `--workspace --all-targets`, the `wgpu`
feature (`rustyac-web`, `rustyac-game`, `rustyac-render`, `--all-targets`),
wasm32-unknown-unknown (`rustyac-web`, `rustyac-net`) and wasm32-wasip1. Every
tool's lock file passes `cargo tree --locked --offline`.

## Not done (Task 40b)

- TrueType hinting (a TrueType interpreter), so wgpu's glyphs are a little
  softer than DirectWrite's.
- Fallback fonts for characters AC's fonts lack (CJK, Hangul): wgpu draws
  the missing-glyph box.
- The stats apps get no frame counts on wgpu (hidden in the page).
- The hidden apps (step 3) and AC's Python apps (Task 42).
- The render oracle's own GUI widget sequences in the page (the page is
  proved with whole-GUI `--gui-do` scripts instead).
- High's frame rate varies run to run on this PC.
- The preview content pack must be rebuilt: it now carries `content/gui`
  and `system/locales`.

