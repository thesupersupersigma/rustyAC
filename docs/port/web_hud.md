# Task 40: HUD, menus and apps in the browser

## Resume here

- Done: step 0 (lib.sh skips the recording cache for `--ai-line` cases; the
  graphics page's High preset is AC's shipped `cfg/video.ini`); step 1 (FW1
  text on wgpu: same kgl log as D3D11 on 12 of 13 GUI sequences, the 13th
  differs only where DirectWrite takes CJK glyphs from a Windows font).
- Next: step 2, the GUI in Full mode in the page.
- Scratch: `re/scratch/task40/`. Disk free at start: 36 GB.

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
wgpu` on the GPU, 1920 x 1080; `re/scratch/task40/d3d_truth.sh`,
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
