# Task 39: graphics settings in the browser (v0.39.0)

## Resume here

- Task 39 (v0.39.0) is done: on-demand servers join from the page and show in the theme's
  Online list (Step 0), and the gear page has AC's Options → Video with presets and a render
  scale, used by Full mode as rustyac.exe uses `--video-ini` (Steps 1–4). Proofs below.
- Not done (Task 39b): see "Not done" at the end; first the one-word `lib.sh` fix for the
  `--ai-line` cache case.
- Scratch: `re/scratch/task39/` (e2e.sh, gfx.sh, speed.sh, checks.sh, ondemand.sh and their
  outputs; `webpack/` is the Montreal + Corvette test pack, 1 GB, Kunos' content: never commit).

## Step 0: joining on-demand servers

Three reports: two were one page bug, the third was what the panel shows:

1. **`?join=x-rustyac-montreal` drove the F2004 at Spa.** The page asked for the on-demand list
   at a relative URL, `net/x/servers.json`. The site serves the page at `/play/app/`, so the
   request went to `/play/app/net/x/servers.json` (404), not the router's
   `/net/x/servers.json`. With no list, `showServers` kept the first fixed server (Spa) and
   `?join` joined it with "any free car". Local tests always passed `?ondemand=http://...`,
   which hid it. Now the URL is `/net/x/servers.json` (absolute), and a `?join` that names a
   server the list does not have says "server <id> is not running now (not in the server
   list)" and joins nothing.
2. **The theme's Online list had no Montreal**: the same missing list (the theme's
   `getserverlist` is built from it).
3. **The password**: the panel code was right (`router.ts` lists
   `password: !!preset.password` from the preset the server was *started* with). A server
   started before the password was added to its preset keeps listing `false` until it starts
   again. The panel now says it: a running server's row shows "a password to join" / "no
   password to join", and "the preset now says otherwise about the password: that takes
   effect when the server starts again" when they differ. The panel's state carries
   `password` for running servers (`main.ts`), with a unit test.

### Step 0 proofs

| Test | Where | Result |
|---|---|---|
| theme's Online list shows a listed on-demand server, with its password | `tools/proofs/browser.sh` (page at `/play/app/`, `ondemand_site.mjs`) | IDENTICAL |
| `?join` of a listed on-demand server joins that server (its track loaded first) | `browser.sh` | IDENTICAL |
| `?join` of an unlisted server joins nothing, says why | `browser.sh` | IDENTICAL |
| a running server with a password is listed with one, the password never in the list | `server/panel/test/panel.test.ts` | pass (13/13) |

End to end (`tools/web_checks/task39/e2e.sh`, all on 127.0.0.1: the panel with 38e's Docker and
Access stand-ins, the router, a local acServer copy, a made-up password; the page under
`/play/app/` and the router under `/net/x/` on one address as on rustyac.com):

| Check | Result |
|---|---|
| preset "rustyAC Montreal", 4 Corvette C7R, password: saved and started | ok; `server_cfg.ini` CARS=ks_corvette_c7r TRACK=montreal (the password goes in by the container's environment, not the file) |
| router's list through the site | `x-rustyac-montreal`, montreal, ks_corvette_c7r, `"password":true` |
| panel's running entry | `"password":true` |
| theme's Online list | Spa, Laguna Seca (fixed) and rustyAC Montreal with a password |
| `?join=x-rustyac-montreal` + the password, no car named | drives: asked for ks_corvette_c7r, "slot 0, ks_corvette_c7r at montreal, session Practice" |
| the same with a wrong password | refused: "ERROR :: Password not accepted" |
| `?join=x-not-running` | joins nothing: "server x-not-running is not running now (not in the server list)" |
| v0.38.9's page, same `?join` | the bug: went for Spa ("connecting to Spa (Ferrari F2004, BMW Z4 GT3)") |
| stop | router list empty |

## Steps 1–4: the graphics page

Gear → Picture → **Graphics** lists every setting of AC's Options → Video that lives in
`video.ini`, with the theme's names and choices (Display, Effects, View's three cockpit
checkboxes, Reflections). They are written to the page's own
`Documents\Assetto Corsa\cfg\video.ini` (the same browser store the theme writes, so the theme's
own Video page and this one see one file). Full mode reads that file at the start of each drive,
as `rustyac.exe --video-ini <file>` does (`AcOptions::video_exact`: the file's values as written,
nothing filled in).

- Settings Full mode does not draw yet are listed and saved but marked "not used" (Resolution,
  Rendering Mode, Fullscreen, Vertical Sync, Framerate limit, and YEBIS's Effect preset, quality,
  glare, depth of field, saturation, crepuscular rays, heat shimmer). Changing them changes the
  file only.
- **Render scale** (rustyAC only, labelled so): 50–100 % of the canvas's pixels; the browser
  scales the picture up. It is kept per browser (`localStorage`), not in `video.ini`.
- **Presets**: High = the game's own defaults with no `video.ini` (what Full mode drew before
  this task; proved identical below), Medium (shadows 1024, world detail Medium, anisotropic 2x,
  smoke Low), Low = the Hybrid picture (shadows Low 512, world detail Low, no smoke in mirrors,
  no reflections, no post-processing, no MSAA, render scale 75 %). Any change shows "Custom".
  Debug stays its own Picture choice. The choice is remembered per browser.
- **When they count**: render scale at once; the rest from the next drive (acs.exe reads
  `video.ini` only at start too), with a note saying so.
- `?graphics=low,VIDEO/SHADOW_MAP_SIZE=1024,SCALE=50` sets them from the URL (for proofs).

## Proofs

Scripts: `tools/web_checks/task39/gfx.sh` (F2004 at Magione, pit, 3 frames' kgl call log;
the page in headless Chrome on SwiftShader with `?fullproof=3&graphics=...`; native
`full_frame --video-ini` on Direct3D 11 WARP and on wgpu/SwiftShader), `gfx_ini.mjs` (the
page's own ini code makes the native side's file).

### Page == native wgpu == Direct3D 11, same `video.ini`

| Settings | Size | Page vs wgpu | wgpu vs D3D11 | Lines |
|---|---|---|---|---|
| High | 1280x720 | IDENTICAL | IDENTICAL | 15736 |
| High vs no `video.ini` at all (Full before this task) | 1280x720 | — | IDENTICAL | 15736 |
| Low (render scale 75 %) | 960x540 | IDENTICAL | IDENTICAL | 14991 |
| Low at render scale 50 % | 640x360 | IDENTICAL | IDENTICAL | 14991 |
| Custom mix: High + shadows 4096, world detail Medium, anisotropic 16x, smoke Ultra, mirror High + HQ, reflections Medium every 3 faces, hide arms | 1280x720 | IDENTICAL | IDENTICAL | 20273 |

### Each setting changes the call log (D3D11, vs the line's base)

| Setting | Base | Result |
|---|---|---|
| Anisotropic Filtering 16x | High | DIFFERENT |
| Anti-Aliasing Samples 4x | High + post-processing | DIFFERENT (from High alone: IDENTICAL; without post-processing MSAA is only the swap chain's sample count, outside the call log) |
| World detail Minimum | High | DIFFERENT |
| Shadow Resolution Ultra | High | DIFFERENT |
| Smoke generation Off | High | DIFFERENT |
| Show smoke in mirrors | High + mirror Normal | DIFFERENT |
| Enable post-processing effects | High | DIFFERENT |
| Fast Approximate Anti-Aliasing | High + post-processing | DIFFERENT |
| Motion Blur 6 | High + post-processing | DIFFERENT |
| Mirror Resolution Normal | High | DIFFERENT |
| High Quality Mirror Reflection | High + mirror Normal | DIFFERENT |
| Reflection Quality Medium | High | DIFFERENT |
| Reflection Rendering Frequency 3 | High | DIFFERENT |
| Hide driver arms | High, cockpit camera | DIFFERENT |
| Lock virtual steering wheel | High, cockpit camera | DIFFERENT |
| Hide virtual steering wheel | High, cockpit camera | DIFFERENT |
| Render scale 75 % / 50 % | — | the page draws 960x540 / 640x360 of 1280x720 |

### Speed, 1920x1080, F2004 at Magione, 20 s autodrive (`tools/web_checks/task39/speed.sh`)

| Preset | Radeon Pro 5500M fps | ms/frame | SwiftShader fps | ms/frame |
|---|---|---|---|---|
| High | 59.9 | 15.1 | 0.9 | 21.5 |
| Medium | 72.2 | 12.3 | 1.2 | 19.6 |
| Low | 77.8 | 11.5 | 1.8 | 18.8 |
| Low at 50 % | 80.4 | 11.1 | 3.0 | 18.3 |

ms/frame is the page's own work per frame (the simulation, the renderer's calls); fps is what
the browser showed, so on SwiftShader the GPU's software work is the difference.

### `tools/proofs/run_all.sh` (v0.39.0's code)

| Area | `--quick` cases | different | `--changed v0.38.9` cases | different |
|---|---|---|---|---|
| physics | 9 | 0 | — | — |
| fresh | 47 | 0 | 657 | 1 (see below) |
| sound | 4 | 0 | 22 | 0 |
| render | 10 | 0 | — | — |
| gui | 15 | 0 | — | — |
| replays | 5 | 0 | 34 | 0 |
| net | 16 | 0 | 80 | 0 |
| browser | 9 | 0 | 226 | 0 |
| ai_page | 3 | 0 | 3 | 0 |
| shaders | 4 | 0 | — | — |
| launcher | 1 | 0 | 12 | 0 |
| dumps | 6 | 0 | 223 | 0 |
| wall clock | 0h34m | | 4h21m | |

The one different case is the proof script's, not the port's: `multi ai_solo ks_vallelunga
ks_ferrari_f2004 --layout extended_circuit --ai-line ...` took its recording from the kept
cache (`re/gold/fresh/rec/multi_ai_solo/`) and so ran only `multi-check`, which reads the AI
line's copy in the scratch root's `ai_port/`. Only a recording run writes that copy, and an
earlier case had made the track's folder again, so the port found no AI line. Run again on
its own, recorded fresh and checked against the cached recording: IDENTICAL for threads 0, 1
and 8 both ways (hash 1b2ec946b4a4a5a3). The fix (`tools/proofs/lib.sh` `multi_list`: add
`ai-line` to the `--(replay|ghost|telemetry)` cases that skip the cache) is left for 39b,
because a `lib.sh` change makes `--changed` take every area.

## Not done

For Task 39b:

- `tools/proofs/lib.sh`: `--ai-line` cases must not take a cached recording (above).
- The settings Full mode does not draw yet ("not used" on the page): YEBIS's post-processing
  look (Effect preset, Overall effect quality, Glare, Depth of field, Color saturation,
  Crepuscular rays, Heat shimmer: Task 23b's open work, desktop does not draw them either),
  and Resolution / Rendering Mode / Fullscreen / Vertical Sync / Framerate limit, which are
  the browser's (the page draws at the canvas's size and the display's rate).
- Options → View's settings outside `video.ini` (field of view, glancing, camera shake,
  `camera_onboard.ini` / `camera_manager.ini` / `gameplay.ini`): not on this page.
- Settings change at the next drive (as in acs.exe), only render scale at once. Changing
  shadows or mirrors mid-drive would need the renderer made again.
- Medium is rustyAC's own choice of values (AC ships no Medium); it is in `web/app.js`
  (`GFX_PRESETS.medium`).
- `full_frame` (the native side of the proofs) was not building for wasm32 under
  `clippy --all-targets` since v0.38.8 (it reads the picture back); it is now native only.
