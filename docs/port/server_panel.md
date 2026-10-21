# Task 38e: the private server panel (v0.38.4)

## Resume here

- Done and released as v0.38.4: the panel in `server/panel/` (presets, start/stop through
  the socket proxy, the `net/x/<id>` router, the Cloudflare Access check, auto-stop), its
  unit tests (8 of 8), the compose services (profile `panel`), `server/.env.example`,
  `AC_CFG` in the entrypoint, the page's list of on-demand servers (`web/app.js`), the docs
  in `server/README.md`, the end-to-end run on 127.0.0.1 and the proofs below (net, browser,
  ai_page; physics from the stopped full run).
- Not done: the panel was never run against a real Docker daemon or Cloudflare (by the
  rules); the image tags `node:24.11.1-alpine` and `tecnativa/docker-socket-proxy:0.3.0`
  were not checked online.

## What the panel is

`server/panel/` is one Node process (TypeScript run as it is: Node strips the types; no
runtime packages) with two listeners on `GATEWAY_BIND`:

- **the panel** (`PANEL_PORT`, 9680): a private page behind Cloudflare Access. Every request
  (the page too) needs a valid `Cf-Access-Jwt-Assertion` token: RS256, signed by one of the
  team's keys (`https://<team domain>/cdn-cgi/access/certs`), the application's AUD, the
  team's issuer, not expired. Anything else gets 403.
- **the router** (`PANEL_ROUTER_PORT`, 9681): public, behind the one tunnel route
  `rustyac.com/net/x/*`. `GET /net/x/servers.json` lists the on-demand servers that run;
  a WebSocket on `/net/x/<id>` is carried to that server's gateway. Every other path is 404.

A preset (a JSON file in `<PANEL_DATA_DIR>/presets/`) holds a server's choices: track and
layout, cars with counts and skins, practice / qualify / race, time of day, weather, grip,
password, max players, pickup mode, idle minutes, and the other `server_cfg.ini` keys. The
page sends it; `checkPreset` checks each field against the pack (only cars with a
`data.acd`, only tracks with a `data/surfaces.ini`) and against what an ini line can hold,
before anything is written. "New from the fixed server X" makes a preset from the pack's
`cfg/<X>/` files.

Start writes `server_cfg.ini` and `entry_list.ini` into `<PANEL_DATA_DIR>/run/<id>/`, then
creates and starts two containers from the kit's own images: `rustyac-x-<id>-acserver`
(the pack read-only at `/pack`, the run folder at `/cfg`, `AC_CFG=/cfg`; game port TCP+UDP
and the HTTP port published on every address, for desktops on the LAN / tailnet) and
`rustyac-x-<id>-gateway` (in the acServer container's network, so the server is
`127.0.0.1:<game port>` to it; its port 9601 published on `GATEWAY_BIND` only). Ports come
from three pools in `.env`, never the fixed servers' ports nor 8081 / 8082. Stop stops and
removes both. The admin password is new for each start (shown on the panel); the join
password goes in by `AC_PASSWORD`, as for the fixed servers.

Every `PANEL_POLL_SECONDS` (30) the panel asks each server's HTTP port (`/JSON|`) who is
connected; a server nobody was on for its preset's idle minutes (30) is stopped.

Docker is reached only through `tecnativa/docker-socket-proxy` with `CONTAINERS=1 POST=1`
on an internal network.

## Changes outside the panel

- `server/entrypoint.sh`: `AC_CFG`, the folder with `server_cfg.ini` and `entry_list.ini`
  (default `$AC_PACK/cfg/$AC_SERVER`, as before).
- `server/docker-compose.yml`: services `panel` and `docker-proxy`, profile `panel` (a plain
  `docker compose up -d` starts what it started before).
- `server/.env.example` (new), `.gitignore` gets `server/.env`.
- `LICENSING.md`: the panel and its npm packages and images.

## End to end on 127.0.0.1 (`re/scratch/task38e/e2e.sh`)

The panel (`node server/panel/src/main.ts`) against a Docker API stand-in that runs the
acServer copy from the AC folder's `server\` (in `re/scratch/task38e/acserver/`) and the
repo's `rustyac-gateway` as local processes, behind an Access stand-in that signs tokens
with a test key and serves its public key the way `/cdn-cgi/access/certs` does. The page is
v0.38.3's site with this task's `app.js`; a headless Chrome (`web/check.mjs`) joins.

| Check | Result |
|---|---|
| panel without a token / with a made-up token | 403 / 403 |
| panel through the Access stand-in | 200 |
| a form post (not JSON) | 403 |
| "New from the fixed server laguna_seca", idle minutes set to 2 | saved |
| a preset with a car not in the pack | refused: `car ks_mazda_mx5_cup: not in the pack` |
| start: `server_cfg.ini` vs the pack's `cfg/laguna_seca/` | only NAME and the three ports differ (pool ports 9620 / 9620 / 9630) |
| start: `entry_list.ini` vs the pack's | 0 lines differ (comments aside) |
| router `GET /net/x/servers.json` | the started server, players 0 / 12, no password, no admin password |
| router WebSocket to an unknown server / to `/net/spa` | 404 / 404 |
| the page with `?join=x-laguna_seca-x` | joined through the router, Practice, driving at 244 km/h |
| the panel's player list while the page drove | `panel38e`, idle countdown held at the full 2 min |
| the idle stop (idle minutes 2, poll every 5 s) | stopped and removed both containers 200 s after the start, about 2 min after the page left (last seen with the player ~80 s after the start); the router list empty, the run folder removed |
| the stand-in's calls for that stop | `stop` + `DELETE` of the gateway, then of the acServer; nothing else |

Screenshots: `server_panel.jpg` (43 KB, the panel with the started server; the admin
password is the run's random one) and `server_panel_join.jpg` (149 KB, the page driving on it
through the router).

![the panel](server_panel.jpg)

## Proofs, clippy, builds

`git diff --stat v0.38.3 -- crates` is empty: no Rust crate changed in this task (only
`server/`, `web/app.js`, docs, `LICENSING.md`, `.gitignore`). So, as decided for this task,
only the areas the change can reach ran:

- **net**: the multiplayer goldens four ways (the gateway and client code the panel's
  servers use; the gateway is unchanged, the area checks it still answers the same);
- **browser**: every drive desktop against wasm32-wasip1 (the page's simulation);
- **ai_page**: the Quick Race in the page (the page itself runs `web/app.js`, which changed).

The full run was started first and stopped by request after its first area (physics: 113
cases, 113 identical, 0 different, 35m11s). The other areas (fresh, sound, render, gui,
replays, shaders, launcher, dumps) were not run: they exercise crates, the desktop game and
the launcher, none of which changed; the physics dumps were not regenerated for the same
reason.

| area | cases | identical | different | time |
|---|---|---|---|---|
| physics (the stopped full run) | 113 | 113 | 0 | 35m11s |
| net | 80 | 80 | 0 | 17m46s |
| browser | 223 | 223 | 0 | 50m23s |
| ai_page | 3 | 3 | 0 | 6m44s |

Clippy `-D warnings` (re/scratch/task38e/checks.sh, CI's lists and the Task 38 extras): the
workspace with `--all-targets`; rustyac-web + rustyac-net `--lib` on wasm32-unknown-unknown;
math / ode / content / physics / net / game on wasm32-wasip1; rustyac-render `--features
wgpu --all-targets`; rustyac-web + rustyac-dxbc `--all-targets` on wasm32-unknown-unknown:
all clean. `cargo build --release --locked` and every `tools/*` manifest with `--locked`:
pass. Panel: `npx tsc --noEmit` clean, `npm test` 8 of 8.
