# Task 38e: the private server panel (v0.38.4)

## Resume here

- Done: the panel in `server/panel/` (presets, start/stop through the socket proxy, the
  router of `net/x/<id>`, the Cloudflare Access check), its unit tests (8, all pass), the
  compose services (profile `panel`), `server/.env.example`, `AC_CFG` in the entrypoint.
- Next: the end-to-end run on 127.0.0.1 (fake Docker API, acServer copy, gateway, headless
  page), the page's list of on-demand servers, the docs in `server/README.md`, the proofs,
  the release.

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
