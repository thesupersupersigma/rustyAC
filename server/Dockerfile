# SPDX-License-Identifier: MIT OR Apache-2.0
#
# rustyAC server kit: an image that RUNS Kunos' Linux acServer without containing it.
#
#   docker build -t rustyac-acserver server/
#   docker run --rm -e AC_SERVER=spa -v /srv/rustyac/dist-server:/pack:ro \
#       -p 9600:9600/tcp -p 9600:9600/udp -p 8081:8081/tcp rustyac-acserver
#
# The image holds no Assetto Corsa file. The server pack (server/make_server_pack.py:
# acServer, the cars' data.acd, the tracks' surfaces) is mounted at /pack at run time.
# acServer is a statically linked 32-bit Go program: it needs no libraries and no AVX.

FROM debian:bookworm-slim

RUN useradd --system --create-home --home-dir /srv/ac acserver
COPY entrypoint.sh /usr/local/bin/acserver-entrypoint
RUN chmod 755 /usr/local/bin/acserver-entrypoint

USER acserver
WORKDIR /srv/ac
# AC_SERVER           the folder under /pack/cfg to run (spa, laguna_seca)
# AC_ADMIN_PASSWORD   the admin password (empty: no admin commands)
# AC_PASSWORD         a join password (empty: open)
ENV AC_SERVER=spa AC_ADMIN_PASSWORD= AC_PASSWORD=
ENTRYPOINT ["/usr/local/bin/acserver-entrypoint"]
