# The browser preview: how to build it, try it and host it

rustyAC's physics also runs in a browser tab: the same Rust code compiled to WebAssembly, with a small
picture (WebGPU, or WebGL2 where the browser has no WebGPU), keyboard and gamepad. It is a preview: no sound,
not Assetto Corsa's own look, one car on the track. What it proves and what it measures is in
[docs/port/web.md](port/web.md).

The site itself holds **no Assetto Corsa file**. Cars and tracks come from one of two places:

- **your own Assetto Corsa folder**, picked in the page ("Open your Assetto Corsa folder"). The files are read
  by the page on your computer; nothing is uploaded.
- **a preview pack**: at most two cars and two tracks that you copy out of your own install with a tool and put
  next to the site. This is for a page you host for yourself (a Chromebook without the game on it). The pack is
  Kunos' content: it is never in this repository and never in a release.

## What you need

- Rust (the version in `.github/workflows/ci.yml`) and Python 3.9 or newer.
- Nothing else has to be installed by hand. `web/build.py` fetches, into `target/` inside the repository:
  - Rust's standard library for `wasm32-unknown-unknown` (with `rustup target add` where there is rustup;
    otherwise the `rust-std` archive from rust-lang.org, checked against the hash published there)
  - `wasm-bindgen` 0.2.129 and binaryen's `wasm-opt` (version_123), from their GitHub releases, checked against
    the SHA-256 hashes written in `web/build.py`. To use your own copies: `--wasm-bindgen <path> --wasm-opt <path>`.
- For the preview pack: an Assetto Corsa install on the PC that builds the pack.
- For the headless check: Node 22 or newer and Chrome or Edge.

## Build the site

```
python web/build.py
```

It builds `crates/rustyac-web` for wasm (`--release --locked`, no SIMD and no fast-math flags), runs
`wasm-bindgen` and `wasm-opt -O3`, and writes the site into `dist-web/site/`:

```
dist-web/site/index.html, app.js, style.css
dist-web/site/pkg/rustyac_web.js, rustyac_web_bg.wasm      (about 4.1 MB)
dist-web/site/LICENSE-GPL, LICENSING.md
```

`python web/build.py --no-opt` skips `wasm-opt` (quicker to build, 5.7 MB). `--zip <file>` also writes the site as
a zip with a `.sha256` next to it; that is what a release attaches as `rustyAC-vX.Y.Z-web.zip`.

`dist-web/` is ignored by git.

## Build the preview pack (optional)

Which cars and tracks go in is written in [`web/preview.toml`](../web/preview.toml):

```toml
cars = ["ks_ferrari_f2004", "bmw_z4_gt3"]
tracks = ["spa", "ks_laguna_seca"]
```

A track with layouts is written `"<track>/<layout>"`. More than two cars or two tracks are refused.

```
cargo run --release --manifest-path tools/web_pack/Cargo.toml
```

It finds your install (`AC_ROOT`, else Steam's usual place; or `-- --ac <folder>`), loads every car on every
track once with a log of every file the loaders open, and copies exactly those files into `dist-web/preview/`
(the car's `data.acd` as it is, its collider and model, the track's models, `ai` and `data` files that are
read, `system/data/surfaces.ini`), laid out as in the game. `manifest.json` lists them with sizes and SHA-256
hashes. `-- --dry-run` prints the sizes without copying. The install is only read.

With the default four it is 1.46 GB (see the table in the report): the tracks are large.

## Try it on this computer

```
python web/serve.py
```

and open `http://127.0.0.1:8080/` in Chrome or Edge. The server serves `dist-web/site/` and, when it exists,
`dist-web/preview/` at `/preview/`, to this computer only. `--port <n>` changes the port.

In the page: pick the folder (or use the pack), pick a car and a track, press **Drive**. R puts the car back
in the pits, Shift+R back on the track where it is, F1 changes the view, Esc goes back to the menu; the page
lists the rest (H shows the list while driving).

### Keys and pad

| | keyboard | Xbox pad |
|---|---|---|
| throttle, brake | Up / W, Down / S | RT, LT |
| steer | Left / A, Right / D | left stick |
| gear up, down | Space / E, Left Ctrl / Q | Y, X |
| clutch, handbrake | Left Shift, B (F9) | A, RB |
| DRS, KERS / ERS | F, K | LB, B |
| **traction control up, down** | **T, Shift+T** | D-pad up, down |
| **ABS up, down** | **Y, Shift+Y** | **Back held + D-pad up, down** |
| **brake bias forward, rearward** | **], [** | D-pad right, left |
| back to the pits | R | Back, a tap |
| back onto the track | Shift+R | Back, held 0.6 s and let go |
| next view | F1 or C | right stick press |
| new car, automatic gearbox | N, G | |
| pause, menu, the key list | P, Esc, H | Start |

- The display shows `TC 2/3`, `ABS off` (or `not fitted`) and `Bias 58.0 %` all the time, lit while the aid
  acts, and a note in the upper middle for a moment when one changes. A level goes up to its highest and then
  to off, as in the game.
- **Back is decided when you let it go**: nothing if the D-pad's up or down went down while you held it (it
  was the ABS combination), else back to the pits after less than 0.6 s, back onto the track after more.
- Keys pressed with Ctrl or Alt are left to the browser (Ctrl+T, Ctrl+W and the like cannot be taken from
  Chrome anyway), which is why the aids have plain keys here. The page only keeps the keys it uses; every
  other key (F5, F11, F12, Tab ...) does what it does in the browser.
- Left Shift is also the clutch: use Right Shift for Shift+T / Shift+Y while moving. `]` and `[` are the two
  keys right of P (by position, whatever the keyboard layout prints on them).

Useful address options: `?car=bmw_z4_gt3&track=ks_laguna_seca&go=1` (start without a click),
`&autodrive=1` (a line follower drives), `&backend=webgl` (do not use WebGPU), `&tex=512` (smaller textures),
`&msaa=1` (no anti-aliasing), `&spawn=hotlap`.

### Check it without a window

```
node web/check.mjs --url "http://127.0.0.1:8080/?go=1&autodrive=1&car=bmw_z4_gt3&track=ks_laguna_seca" --seconds 20 --screenshot shot.png
```

starts a headless Chrome with its own throw-away profile (under `target/`), lets the page drive for 20 seconds,
takes a screenshot, prints the lap timer, the frame rate and the physics steps per second, and closes the
browser again.

The folder picker needs a click, which a headless browser cannot give. Its test hook feeds the same code from
a folder served over HTTP: `python web/serve.py --ac "<your assettocorsa folder>"`, then
`...?testfolder=/ac-test/&go=1&car=bmw_m3_e30&track=magione`.

**The same physics as the desktop, checked in the browser itself:**

```
cargo run --release -p rustyac-web --example selftest -- bmw_z4_gt3 ks_laguna_seca --steps 10000
node web/check.mjs --url "http://127.0.0.1:8080/?go=1&autodrive=1&car=bmw_z4_gt3&track=ks_laguna_seca&selftest=10000"
```

Both print the car's whole state after 10,000 steps as one number (`state` / `hash`). They must be equal.

The same with key presses on the way (the aids and the brake bias change the car, so the number changes, and
it still has to be the same on both sides). The page sends each key through its own key handler:

```
cargo run --release -p rustyac-web --example selftest -- bmw_z4_gt3 ks_laguna_seca --steps 10000 --keys "KeyT@300,Shift+KeyT@600,KeyY@900,BracketRight@1200,BracketLeft@1500"
node web/check.mjs --url "http://127.0.0.1:8080/?go=1&autodrive=1&car=bmw_z4_gt3&track=ks_laguna_seca&selftest=10000&keys=KeyT@300,Shift+KeyT@600,KeyY@900,BracketRight@1200,BracketLeft@1500"
```

`selftest.presses` in the second one's output is what the display showed before and after each key. And with
real key presses while the page drives (trusted events, through the browser's own input path):

```
node web/check.mjs --url "http://127.0.0.1:8080/?go=1&autodrive=1&car=bmw_z4_gt3&track=ks_laguna_seca" --seconds 8 --keys "KeyT,Shift+KeyT,KeyY,BracketRight,BracketLeft"
```

## Host it

The site is static files: any web server or static host will do (HTTPS, or `localhost`: the folder picker and
the browser's file cache only work in a "secure context"). No special headers are needed (the physics does not
use threads). Serve `.wasm` as `application/wasm`.

- **The site only** (everybody opens their own Assetto Corsa folder): upload the content of `dist-web/site/`,
  or unpack a release's `rustyAC-vX.Y.Z-web.zip`.
- **With your preview pack**: also upload `dist-web/preview/` as a folder named `preview` next to
  `index.html`. The page looks for `preview/manifest.json`; another place is given with `?pack=<url>` (the
  other server then has to allow cross-origin requests).

  **Do not put the pack where the public can fetch it.** It is Kunos Simulazioni's content, yours to use because
  you own the game: host it for yourself (behind a login, or on your own network), as you would not hand out
  the game's folder.

The server should be able to send large files (Spa's main model is 441 MB, Laguna Seca's 359 MB); plain
static hosting does that. Compression on the fly is not needed and only slows a big model down.

A ready-made server is in the repository: `web/Dockerfile` with `web/nginx.conf`, see the next section.

## Deploy with Coolify

`web/Dockerfile` builds the site and puts it behind nginx (`web/nginx.conf`). Coolify clones the repository
and builds the image on the server itself. The image holds the site and nothing else: **no Assetto Corsa file
is ever in it.** A preview pack is a folder on the server that is mounted into the container when it runs.

What the image does:

- stage 1 is the official Rust image at the version CI uses (1.97.0) with the `wasm32-unknown-unknown` target
  and Python; it runs `python3 web/build.py`, the same command as on a PC
- stage 2 is `nginx:1.30.0-alpine` with `dist-web/site/` in `/usr/share/nginx/html`, listening on **port 80**
  (plain HTTP: the proxy in front does TLS)
- `.wasm` is sent as `application/wasm`; html, js, css and the wasm are sent gzip-compressed (compressed once
  when the image is built); **nothing under `/preview/` is compressed**
- `index.html` and `preview/manifest.json` are `no-cache` (asked for again on every visit); everything else is
  cached for five minutes, because the file names are not hashed
- byte ranges and large files work (plain `sendfile`); `/healthz` answers `ok`

### In Coolify

1. **New resource** -> *Public Repository* -> `https://github.com/thesupersupersigma/rustyAC`, branch `master`.
2. **Build Pack**: `Dockerfile`. **Base Directory**: `/`. **Dockerfile Location**: `/web/Dockerfile`.
3. **Ports Exposes**: `80`. (No port mapping to the host: Coolify's Traefik reaches the container itself.)
4. **Domains**: the address you want, for example `http://rustyac.example.com`. Write it with `http://` when a
   Cloudflare Tunnel is in front: Cloudflare does the TLS, and Traefik then neither asks for a certificate nor
   redirects to HTTPS (with `https://` here and a tunnel that talks plain HTTP to Traefik, the page ends in a
   redirect loop).
5. **Health check** (the *Healthcheck* tab): the image has its own (`/healthz`), which Coolify uses. If you
   set one by hand: path `/healthz`, port `80`, scheme `http`.
6. **Persistent Storage** -> *Add* -> **Bind Mount** (only if you want a preview pack):
   - Source Path (on the server): `/srv/rustyac/preview`
   - Destination Path (in the container): `/usr/share/nginx/html/preview`
7. **Deploy.** The first build compiles everything and takes a while on an old server; later builds too
   (the build has no cache between deployments unless Coolify keeps Docker's layer cache).

If the build stops in `wasm-opt` with `Illegal instruction`: add a build variable `WASM_OPT` = `0`
(*Environment Variables*, ticked as *Build Variable*) and deploy again. `wasm-opt` is a downloaded program
(binaryen) that only makes the wasm file smaller (4.1 MB instead of 5.7 MB before compression); without it the
site is the same. See the note on old processors below.

### The preview pack on the server

Build the pack on your PC (`cargo run --release --manifest-path tools/web_pack/Cargo.toml`, see above) and
copy the *content* of `dist-web/preview/` into the server's folder, so that
`/srv/rustyac/preview/manifest.json` exists:

```
ssh tsss-server "sudo mkdir -p /srv/rustyac/preview && sudo chown $USER /srv/rustyac/preview"
scp -r dist-web/preview/* tsss-server:/srv/rustyac/preview/
ssh tsss-server "chmod -R a+rX /srv/rustyac/preview"
```

(`a+rX`: nginx in the container is not root and has to be able to read the files.) No restart is needed: the
page asks for `preview/manifest.json` on every visit. **Without the mount, or with an empty folder, the page
finds no pack and simply offers "Open your Assetto Corsa folder".**

### Cloudflare Tunnel and Cloudflare Access

The pack is Kunos Simulazioni's content: **do not leave the site open to the public while a pack is mounted.**

1. Tunnel: in Cloudflare Zero Trust -> *Networks* -> *Tunnels* -> your tunnel -> *Public Hostname* -> add
   `rustyac.example.com` with service `http://localhost:80` (Coolify's Traefik on the server). Traefik picks
   the container by the host name, which is why the domain in Coolify must be the same name.
2. Access: Zero Trust -> *Access* -> *Applications* -> *Add an application* -> *Self-hosted*. Application
   domain `rustyac.example.com`; add a policy with action *Allow* and *Include* -> *Emails* -> your address
   (or your identity provider's group). Everybody else gets Cloudflare's login page and never reaches the
   server.
3. Leave Cloudflare's cache alone for this host: the site is small, and the pack's big files are not of a
   kind Cloudflare caches by default. If you made a "cache everything" rule for the zone, exclude
   `rustyac.example.com/preview/*`.

A track's main model is up to 441 MB in one response. A tunnel passes that through (Cloudflare's 100 MB limit
is for uploads, not downloads); the page shows its progress bar meanwhile and keeps the file in the browser's
own storage afterwards.

### Old processors (no AVX)

The server this was written for has Xeon E5620 processors, which have no AVX.

- **Running the image** needs nothing special: it is nginx serving files. The wasm is run by the visitor's
  browser, not by the server.
- **Building the image** runs rustc, cargo, `wasm-bindgen` and `wasm-opt` on the server. Rust's own programs
  are built for any x86-64 processor. `wasm-bindgen` and `wasm-opt` are the projects' release downloads
  (pinned and hash-checked in `web/build.py`); neither says that it needs AVX, but **this was not tried on a
  processor without AVX**, hence the `WASM_OPT=0` switch for `wasm-opt`. `wasm-bindgen` cannot be skipped: if
  it should not run there, build the site elsewhere (`python web/build.py`, or take a release's
  `rustyAC-vX.Y.Z-web.zip`) and serve the folder with any web server.

### Try the image on a PC with Docker

```
docker build -f web/Dockerfile -t rustyac-web .
docker run --rm -p 8080:80 rustyac-web
docker run --rm -p 8080:80 -v "<path>/dist-web/preview:/usr/share/nginx/html/preview:ro" rustyac-web
curl -I http://127.0.0.1:8080/ ; curl http://127.0.0.1:8080/healthz ; curl -I http://127.0.0.1:8080/pkg/rustyac_web_bg.wasm
```

## How the page copes with a 441 MB file

- A file is **streamed**: the page reads the response in pieces and copies each piece straight into one
  buffer inside the wasm memory. The file is never held twice, and a progress bar shows megabytes done of the
  total (the manifest knows every size before the first byte arrives).
- Only the files of the **chosen** car and track are fetched: 0.6 to 0.9 GB for one drive, not the whole pack.
- While it arrives the file is hashed (SHA-256) and compared with the manifest, and written into the
  browser's own storage for this site (OPFS), under its hash. **On the next visit it comes from there**, not
  from the network (measured: Laguna Seca's 879 MB in 2.1 s instead of 13.4 s from a local server).
- After the track's collision meshes are built and its textures are on the graphics card, the model files'
  memory is given back.
- Textures are capped at 1024 px (512 px where the device takes no compressed textures and they have to be
  unpacked), and halved again until the set fits a budget, as the desktop's debug view does.

What it costs: the browser tab needs about 1 to 1.5 GB of memory while a big track loads. A device with 4 GB of
memory may not manage Spa or Laguna Seca; a smaller track will do there.

With your own folder there is no download at all: the files are read from your disk, the same way, and the
folder is remembered (Chrome and Edge ask once more on the next visit, or not at all if you allowed the site
"on every visit").
