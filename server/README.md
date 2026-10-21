# rustyAC server kit

Everything needed to run **Kunos' own `acServer`** (the dedicated server that ships with
Assetto Corsa) for rustyAC and real Assetto Corsa players, without any Kunos file in git.
License: `MIT OR Apache-2.0`. The full reports are `docs/port/multiplayer.md` (the servers,
the desktop client) and `docs/port/multiplayer_web.md` (the gateway, the browser).

| File | What it is |
|---|---|
| `spa/`, `laguna_seca/` | `server_cfg.ini` + `entry_list.ini` of the two servers: Spa and Laguna Seca, six Ferrari F2004 and six BMW Z4 GT3 pick-up slots each |
| `make_server_pack.py` | copies `acServer`, the two cars' `data.acd` and the tracks' surface files from **your** install into the git-ignored `dist-server/` |
| `Dockerfile`, `entrypoint.sh` | an image that runs the Linux `acServer` from a pack mounted at `/pack` |
| `docker-compose.yml` | both servers, and a WebSocket gateway in front of each (`crates/rustyac-gateway`: for browsers, and for desktop players behind an HTTPS-only tunnel); with the profile `panel`, the server panel and its Docker socket proxy |
| `panel/` | the server panel: a private page that starts and stops extra servers from presets, and the router of their WebSockets (below) |
| `.env.example` | every setting of the compose file, with made-up values |

## Ports

| Server | Game port (TCP **and** UDP) | HTTP port | Gateway (WebSocket) |
|---|---|---|---|
| Spa | 9600 (`SPA_GAME_PORT`) | 8081 (`SPA_HTTP_PORT`) | 9601 (`SPA_GATEWAY_PORT`) |
| Laguna Seca | 9610 (`LAGUNA_GAME_PORT`) | 8082 (`LAGUNA_HTTP_PORT`) | 9611 (`LAGUNA_GATEWAY_PORT`) |

These are the ports on the **host**. Each is a variable of the compose file with the number
above as its default: where a port is taken already, give another one, in the shell or in a
file named `.env` next to `docker-compose.yml`:

```sh
# server/.env on a host whose port 8081 belongs to another program
SPA_HTTP_PORT=8091
```

Inside the containers the numbers stay the ones of the configs.

There are two ways in:

- **Directly:** the game port must reach the server as raw TCP and UDP. A Cloudflare Tunnel
  or Traefik route carries HTTP only, so these players join over **Tailscale** (the server's
  tailnet address), the **LAN**, or a **router port-forward** of the game ports:
  `rustyac.exe --join <server address>:9600`.
- **Through the gateway:** one WebSocket per player, which an HTTPS tunnel does carry. This
  is how a browser joins, and a desktop player without Tailscale:
  `rustyac.exe --join wss://rustyac.example.com/net/spa`. The gateways listen on
  `127.0.0.1` of the host (`GATEWAY_BIND`), for the tunnel; `GATEWAY_ORIGINS` is the address
  of the site whose pages may connect. The tunnel's routes and Cloudflare Access are in
  `docs/port/multiplayer_web.md`.

## Quick start

```sh
# on the PC with Assetto Corsa
python server/make_server_pack.py            # -> dist-server/ (Kunos' files: never commit)

# on the server (the pack copied to /srv/rustyac/dist-server)
AC_PACK_DIR=/srv/rustyac/dist-server AC_ADMIN_PASSWORD=choose-one \
    docker compose -f server/docker-compose.yml up -d --build

# join
rustyac.exe --join <server address>:9600 --car ks_ferrari_f2004
rustyac.exe --join ws://<server address>:9601/net/spa --car ks_ferrari_f2004   # through the gateway (GATEWAY_BIND=0.0.0.0)
```

## Settings worth knowing

- `ADMIN_PASSWORD=@ADMIN_PASSWORD@` in the templates is filled by the container from
  `AC_ADMIN_PASSWORD`; `PASSWORD=` from `AC_PASSWORD`. Nothing secret is in git.
- `REGISTER_TO_LOBBY=0`: the servers are not listed in Kunos' lobby. Set it to 1 only when
  the game ports are reachable from the internet.
- `CLIENT_SEND_INTERVAL_HZ=18`: how often every client sends its car (the server tells each
  client in the handshake).
- The server compares the MD5 of each client's `data.acd`, of `system/data/surfaces.ini`
  and of the track's `surfaces.ini` and `models.ini` with its own copies: a client with
  changed car data is kicked. The pack must come from the same game version as the players'.
- To run one server on Windows for a test: `python server/make_server_pack.py --windows`,
  copy `dist-server/cfg/spa/*.ini` into a `cfg` folder next to `acServer.exe`, replace
  `@ADMIN_PASSWORD@`, and start `acServer.exe` from there.

## The server panel (on-demand servers)

`panel/` is a private page (behind Cloudflare Access) that starts and stops **extra**
servers from presets, next to the fixed Spa and Laguna Seca ones, and a public **router**
that carries the browsers' WebSockets to them. The full report is `docs/port/server_panel.md`.

- **Presets** are kept in `PANEL_DATA_DIR/presets/`. Each one sets the track and layout,
  the cars (count and skins), practice / qualify / race (minutes or laps), weather, time of
  day, grip, join password, max players, pickup mode and the other `server_cfg.ini` keys.
  Only the cars and tracks in the pack can be picked. "New from the fixed server spa" copies
  a fixed server's settings.
- **Start** writes `server_cfg.ini` and `entry_list.ini` into `PANEL_DATA_DIR/run/<id>/` and
  starts two containers from the kit's own images: `rustyac-x-<id>-acserver` and
  `rustyac-x-<id>-gateway`. **Stop** stops and removes both. A server that nobody was on
  for its preset's idle minutes (default 30) stops by itself. At most `PANEL_MAX_SERVERS`
  (3) run at once.
- **Ports** come from three pools: game TCP+UDP `PANEL_GAME_PORTS` (9620-9629), acServer
  HTTP `PANEL_HTTP_PORTS` (9630-9639) and gateway `PANEL_GATEWAY_PORTS` (9640-9649). The
  panel refuses to start when a pool overlaps the fixed servers' ports or 8081 / 8082.
  The game port is published on every address (desktops join it over the LAN or tailnet);
  the gateway is published on `GATEWAY_BIND` only.
- **Joining:** browsers use the site as usual. The page lists the running on-demand
  servers (from `/net/x/servers.json`) next to `servers.json`'s, and joins
  `wss://<site>/net/x/<id>`. Desktops use
  `rustyac.exe --join <PANEL_GAME_HOST>:<game port> --car <model>`. The panel shows both
  links.
- **Docker** is reached only through `tecnativa/docker-socket-proxy` (containers only, on
  an internal network). The panel sends fixed container templates; what the page sends is
  checked field by field first.

### Setting it up

```sh
cd ~/rustyac-src/server
cp .env.example .env            # then change the values (see the comments in it)
sudo mkdir -p /srv/rustyac/panel && sudo chown 1000:1000 /srv/rustyac/panel   # PANEL_DATA_DIR
docker compose --profile panel build   # the acServer, gateway and panel images
docker compose --profile panel up -d
docker compose --profile panel logs -f panel   # "panel on 0.0.0.0:9680", "router on 0.0.0.0:9681"
```

A plain `docker compose up -d` (no profile) leaves the panel out, as before.

Cloudflare (Zero Trust):

1. **Tunnel routes** (Networks > Tunnels > the tunnel > *Public Hostname*):
   - `panel.<your domain>`, no path, service `http://<GATEWAY_BIND>:9680`;
   - `rustyac.<your domain>`, path `net/x/.*`, service `http://<GATEWAY_BIND>:9681`. Move
     it **above** the site's own `rustyac.<your domain>` entry (the first match wins), next
     to the `net/spa` and `net/laguna_seca` routes.
2. **Access application** for the panel (Access > Applications > *Add an application* >
   *Self-hosted*): application domain `panel.<your domain>`, and a policy with action
   **Allow** and *Include* > *Emails* > your own e-mail address only. Nothing else.
3. **The two values for `.env`:** `CF_ACCESS_TEAM_DOMAIN` is the team domain,
   `<team>.cloudflareaccess.com` (Settings > *Custom Pages* or *Team name and domain*);
   `CF_ACCESS_AUD` is the application's *Application Audience (AUD) Tag* (Access >
   Applications > the panel application > *Overview* / *Basic information*). Restart the
   panel after a change (`docker compose --profile panel up -d`).
4. **Test** in a private window: `https://panel.<your domain>` asks for the Access login,
   and after it shows the panel. Then open `http://<GATEWAY_BIND>:9680/` from the server
   itself: it must answer `403` (no token, no panel).

The site's own Access application (if it has one) also covers `net/x/...`, the same as
`net/spa`.
