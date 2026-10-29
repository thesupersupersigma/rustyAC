# AC's Settings in the browser (Task 44b)

## Resume here

State after the last commit (update after every commit):

- **Done and proven:** the page shows AC's own Settings tile beside Drive, with all of its tabs
  (Video with Display / Effects / View / Reflections, Audio, Controls, Profile, General), on the
  same bridge as desktop rustyAC. A scripted Settings session sends the same bridge requests and
  changes the same settings in the page as on desktop. A "rustyAC" tab inside Settings holds the
  page's own settings; the bottom-right gear is gone from the launcher and shows only during a
  drive.
- Also fixed: CI's clippy failed on `crates/rustyac-gui/src/app_drs_zones.rs:146` (a float literal
  with more digits than an f32 holds); the literal is now the f32 value it always was.
- **Released as v0.44.1.** Next: Task 44c, from "Not done (Task 44c)" at the end of this file and
  the one in `docs/port/content_tools.md`.

## In plain words

The browser page used to show only AC's Drive tile; the page's own settings sat behind a gear
in the bottom-right corner. Now the launcher has AC's real Settings tile too. Its tabs are the
theme's own and save through the same bridge as on desktop, into the page's own copy of
`Documents\Assetto Corsa\cfg\*.ini`. What a browser decides for itself (resolution, full
screen, vertical sync, frame-rate limit) is greyed out with "your browser decides this". What
the Full mode does not draw yet (the YEBIS post-processing settings) says "(not used)".

A tab called "rustyAC" at the bottom of Settings' tabs holds what only the page has: full
screen, sound volume and mute, your controls.ini (keyboard and pad speed sensitivity and the
rest), the key list, Full / Debug picture, the graphics preset and render scale, the downloaded
data and Copy diagnostics. AC's own Controls tab opens that same tab at its controls, as AC's
Controls window is a Windows program that a browser cannot show. During a drive, a small gear
in the bottom-right corner (or O) opens the rustyAC tab over the picture.

## How to use it

- In the launcher: click **Settings** (the gear tile under Drive). Video, Audio, Profile and
  General are AC's own pages; changes save at once, as in AC.
- **rustyAC** (last tab): the page's own settings. Close (Esc) goes back to the Video tab.
- **Controls** (AC's tab): opens the rustyAC tab at "Controls".
- In a drive: the gear at the bottom right, or O, opens the rustyAC tab; Esc or O closes it.

## What changed

All in the page's glue, none in Kunos' theme files:

- `web/theme_glue.js`: the Settings tile is allowed (CSS, click blocker and the `disabled` class
  leave `data-panel=options` alone besides Drive); every other tile stays hidden and blocked.
  On load it marks the browser-decided settings (`#screenres`, `#fullscreen`, `#vsync`, the
  `launcher?SAVED/FPS_CAP` slider: disabled, faded, "your browser decides this") and the video.ini
  keys Full mode does not draw (`T.unused`, from `GFX`'s `used` column in app.js: "(not used)"),
  and adds the `rustyAC` tab (`<div data-mode="rustyac" class="nonpanel">`) to
  `#options > .tabs`. After every theme `screenChanged` it tells the page whether Settings with
  the rustyAC tab is showing (the body's `data-panel` and `rel`, which the theme sets on every
  tile and tab) and the rectangle of the theme's settings area. The tab column moves up by up to
  one and a half tabs where the extra tab would leave the window. The benchmark (it starts a
  drive of its own) shows the theme's notice "The benchmark is not available in the browser
  version yet.", for a click and for a script's `.click()` alike.
- `web/launcher.js`: `api.unused`, `api.rustyacTab(rect)` (adds the frame's offset, calls
  `env.onTab`), `api.reloadSettings()` (reloads the theme and opens Settings → rustyAC again,
  via `enterTheme`), and the request list in the form `tools/proofs/log2req.py` writes for
  desktop (`GET url` / `POST url<TAB>body`, last 4000).
- `web/app.js`: `openControls(dock)` shows the panel over the theme's area (class `docked`) or
  on its own (the drive's gear, O); `null` hides it. Close while docked clicks Settings' Video
  tab. `onControls` (AC's Controls tab) opens the rustyAC tab and scrolls to `#controls-section`.
  The gear's click handler moved out of `showCorner()`; the gear shows in `startDriving()` and
  hides wherever the HUD hides. The graphics part keeps only the preset and the render scale
  (AC's Video tab is the key-by-key editor of the same file); a preset written while Settings is
  open reloads the theme so the Video tab shows the new values.
- `web/index.html`, `web/style.css`: the panel's title is "rustyAC", Full screen moved into it,
  the gear moved out of the corner bar (fixed, bottom right, hidden but in a drive),
  `#controls.docked`, the hint texts.

### One video.ini

There is one file, the page's `Documents\Assetto Corsa\cfg\video.ini`. AC's Video tab reads and
writes it key by key through the bridge (`ac://setsetting/video?...`), as on desktop. The
rustyAC tab's preset writes its keys into the same file (`GFX_PRESETS`, as Task 39's graphics
page did) and then reloads the theme, which reads the file again; the preset reads "Custom"
when the file's keys match no preset. The render scale is the page's own (kept in the
browser, not in video.ini). Full mode reads video.ini when a drive starts, as rustyac.exe does.

## The proofs

Scripts in `tools/web_checks/task44b/` (their output went to the ignored `re/scratch/task44b/`).
Page runs: `python web/build.py --no-opt`, `python web/serve.py --port 8139 --pack re/gold/web/pack
--ac "<AC install>"`, `node web/check.mjs --flow ... --size 1600x900`.

### 1. Desktop and page, the same Settings session

`settings.txt` is one click script in `rustyac-launcher.exe --script` form: the first-run
language page (English), then `log MARK`, then Settings: two Display sliders, two Effects
check boxes, View's FOV and three check boxes, Reflections' mirror quality, two Audio sliders,
two General check boxes, Profile, Video, `log END`, AC's Controls tab. Desktop runs it as is
(empty player folder, `--offscreen 1600x900`; once whole, once only up to `log MARK` for the
folder at that point). `txt2flow.py` makes it a check.mjs flow that runs each step in the
page's theme frame, on the AC install (`?launcher=1&testfolder=/ac-test/`).

| check | desktop | page | result |
|---|---|---|---|
| bridge requests after MARK (`cmp.py`; up to `showcontrols`) | 11 | 11 | same |
| settings the session changed (`delta.py`: file, section, key, new value) | 8 | 9 | same values |
| failed steps / page errors | 0 | 0 / none | |

- The page has one more "change": `video.ini [VIDEO] ANISOTROPIC=2`. Both send
  `setsetting/video?VIDEO/ANISOTROPIC=2`; desktop's file already said 2. The two sides start from
  different files (desktop copies AC's `system/cfg`, the page seeds rustyAC's own, as since
  Task 37b), so `delta.py` counts a key one side changed when both end with the same value.
- Left out of the request compare: `eventcount` (the theme's Special Events poll, once a second:
  a timer, not a click; desktop's off-screen window logs none at all) and the cache-busting
  times. After `showcontrols`, desktop's own Controls page reads `ac://getiniasjson/cfg/controls`;
  the page's rustyAC tab has controls.ini already.
- Three of the check boxes (Effects' "Disable HDR", View's time difference and "no body") send
  no request on either side: the theme saves them only another way. Same on both.

### 2. Headless Chrome: every tab opens, saves, survives a reload

`settings.txt` again on the preview pack with the profile kept, then `reload.txt` in a new page
load: Settings shows every value the session set (anisotropic 2, shadows 512, FXAA off, FOV 60,
world-aligned on, mirror HQ on, music 0.5, UI 0.5, persist track on, replay autosave off), the
same values as just before the reload. Every tab (7) opened, no failed steps, no page errors.

### 3. Headless Chrome: the rustyAC tab and the drive (`mktab.py`)

| check | result |
|---|---|
| launcher: gear and full screen gone from the corner bar; tiles shown | `source-switch`, `source-list`; `drive`, `options` |
| a click on Career | blocked (Drive stays) |
| Start Benchmark | the notice; no drive started (`starts` 0) |
| rustyAC tab | docked over the settings area, theme's `rel` = rustyac |
| sound volume 40 + mute | `rustyac.sound` = `{"volume":0.4,"muted":true}` |
| Picture → Debug | `rustyac.picture` = auto |
| render scale 80 | `rustyac.renderScale` = 80, video.ini untouched |
| pad speed sensitivity 0.3, keyboard steering speed 1.5 | kept controls.ini has both |
| Downloaded data, key list | listed (1 group; 6 key rows) |
| Copy diagnostics | the text made; headless Chrome refuses the clipboard (the page says so) |
| Full screen | `document.fullscreenElement` set, left again |
| preset Low | video.ini ANISOTROPIC 4, SHADOW_MAP_SIZE 512, WORLD_DETAIL 1, render scale 75; theme reloaded, rustyAC tab shown again; the Video tab's sliders show the new values |
| AC's Controls tab | the rustyAC tab, scrolled to Controls |
| Close | panel hidden, Video tab |
| a drive (BMW Z4 GT3, Laguna Seca, from the launcher) | gear shown, panel hidden |
| gear | the panel on its own (not docked), focus back on the page |
| Esc / O | Esc closes the panel and the drive goes on; O opens it |
| drives started, launcher errors, page errors | 1, none, none |

### Screenshots (1600 x 900, all under 300 KB)

In `re/scratch/task44b/final/`: `settings_video_display`, `settings_video_effects`,
`settings_video_view`, `settings_video_reflections`, `settings_audio`, `settings_general`,
`settings_profile`, `settings_rustyac`, `settings_controls_to_rustyac`,
`settings_benchmark_notice`, `drive_gear`, `drive_overlay` (`.jpg`).

## Release check (v0.44.1)

`bash tools/proofs/run_all.sh --quick` (rule 14), then every case of the areas the changed web
files touch (`run_all.sh browser ai_page`, tools/proofs/areas.map).

| area | cases | identical | different |
|---|---|---|---|
| physics | 9 | 9 | 0 |
| fresh | 51 | 51 | 0 |
| sound | 4 | 4 | 0 |
| render | 12 | 12 | 0 |
| gui (the DRS zones test fix included) | 29 | 29 | 0 |
| python | 1 | 1 | 0 |
| replays | 6 | 6 | 0 |
| net | 16 | 16 | 0 |
| browser (quick) | 9 | 9 | 0 |
| ai_page (quick) | 3 | 3 | 0 |
| shaders | 4 | 4 | 0 |
| launcher | 1 | 1 | 0 |
| dumps | 6 | 6 | 0 |
| browser (every case) | 226 | 226 | 0 |
| ai_page (every case) | 3 | 3 | 0 |

CI's clippy commands (native `--workspace --locked --all-targets`, both wasm targets) with
`-D warnings`: clean. Builds with `--locked`.

## Web zip

No new or renamed files. Changed: `index.html`, `app.js`, `launcher.js`, `theme_glue.js`,
`style.css` (all named top-level files the site already has). The preview pack needs no new
files: Settings reads only what the launcher already reads (the theme, `cfg` and `system/cfg`).

## Not done (Task 44c)

1. The benchmark (Video → Display → Start Benchmark): rustyAC has no benchmark run; the page
   shows a notice.
2. With `?launcher=0` (the old page without AC's launcher) the graphics part is now only the
   preset and the render scale; the key-by-key list it had is gone (AC's Video tab replaces it).
3. The rustyAC tab has no icon above its name (the theme's tabs draw theirs from their CSS by
   `data-mode`).
4. General's "Clear cache", "Changelog" and the language setting were not exercised in the
   proofs.
5. Profile's two filter boxes show the raw `<l18n ...>` text of their first entries (the
   theme's own page, not checked on desktop).

## Open questions

- The frame-rate limit counts as "your browser decides this" (the page draws at the
  browser's own frame rate). Is that what you want, or should it be a "(not used)" setting?
- Desktop's off-screen launcher window logs no `ac://eventcount` at all, the page logs one a
  second. Not a Settings difference, but not explained either.
- At window heights near 900 px, AC's own tab column already reaches the bottom; the rustyAC tab
  moves the column up a little to fit. A shorter window cuts it, like the theme's own General tab.
