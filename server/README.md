# rustyAC server kit

Everything needed to run **Kunos' own `acServer`** (the dedicated server that ships with
Assetto Corsa) for rustyAC and real Assetto Corsa players, without any Kunos file in git.
License: `MIT OR Apache-2.0`. The full report is `docs/port/multiplayer.md`.

| File | What it is |
|---|---|
| `spa/`, `laguna_seca/` | `server_cfg.ini` + `entry_list.ini` of the two servers: Spa and Laguna Seca, six Ferrari F2004 and six BMW Z4 GT3 pick-up slots each |
| `make_server_pack.py` | copies `acServer`, the two cars' `data.acd` and the tracks' surface files from **your** install into the git-ignored `dist-server/` |
| `Dockerfile`, `entrypoint.sh` | an image that runs the Linux `acServer` from a pack mounted at `/pack` |
| `docker-compose.yml` | both servers |

## Ports

| Server | Game port (TCP **and** UDP) | HTTP port |
|---|---|---|
| Spa | 9600 | 8081 |
| Laguna Seca | 9610 | 8082 |

The game port must reach the server as raw TCP and UDP. A Cloudflare Tunnel or Traefik
route carries HTTP only, so players join over **Tailscale** (the server's tailnet address)
or through a **router port-forward** of the game ports.

## Quick start

```sh
# on the PC with Assetto Corsa
python server/make_server_pack.py            # -> dist-server/ (Kunos' files: never commit)

# on the server (the pack copied to /srv/rustyac/dist-server)
AC_PACK_DIR=/srv/rustyac/dist-server AC_ADMIN_PASSWORD=choose-one \
    docker compose -f server/docker-compose.yml up -d --build

# join
rustyac.exe --join <server address>:9600 --car ks_ferrari_f2004
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
