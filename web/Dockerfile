# SPDX-License-Identifier: GPL-3.0-or-later
#
# rustyAC's browser preview as a small web server image: the static site of `web/build.py`
# behind nginx. See docs/web.md, "Deploy with Coolify".
#
#   docker build -f web/Dockerfile -t rustyac-web .          (the build context is the repo root)
#   docker run --rm -p 8080:80 rustyac-web
#   docker run --rm -p 8080:80 -v /srv/rustyac/preview:/usr/share/nginx/html/preview:ro rustyac-web
#
# The image holds no Assetto Corsa file. A preview pack (tools/web_pack) is never copied in:
# it is mounted at run time at /usr/share/nginx/html/preview. Without the mount the page
# finds no pack and offers "Open your Assetto Corsa folder".
#
# Build arguments:
#   WASM_OPT=0   skip wasm-opt (`web/build.py --no-opt`): a larger wasm file (5.7 MB instead
#                of 4.1 MB before compression), the same physics. For a build machine on
#                which the downloaded wasm-opt program does not run ("Illegal instruction").

# ---- stage 1: build the site -------------------------------------------------------------
# The Rust version is the one CI builds with (RUST_TOOLCHAIN in .github/workflows/ci.yml).
FROM rust:1.97.0-slim-bookworm AS build

# python3 runs web/build.py; the certificates are for its downloads (wasm-bindgen, wasm-opt)
RUN apt-get update \
    && apt-get install -y --no-install-recommends python3 ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && rustup target add wasm32-unknown-unknown

WORKDIR /src
COPY . .

ARG WASM_OPT=1
RUN if [ "$WASM_OPT" = "0" ]; then python3 web/build.py --no-opt; else python3 web/build.py; fi \
    # compressed once here, so the server does not compress 4 MB of wasm on every request
    && cd dist-web/site \
    && for f in index.html app.js style.css pkg/rustyac_web.js pkg/rustyac_web_bg.wasm; do gzip -9 -c "$f" > "$f.gz"; done

# ---- stage 2: the server -----------------------------------------------------------------
FROM nginx:1.30.0-alpine

COPY web/nginx.conf /etc/nginx/conf.d/default.conf
# only the built site: no source, no tool, no game file
COPY --from=build /src/dist-web/site/ /usr/share/nginx/html/

EXPOSE 80
HEALTHCHECK --interval=30s --timeout=5s --start-period=5s --retries=3 \
    CMD wget -q -O /dev/null http://127.0.0.1/healthz || exit 1
