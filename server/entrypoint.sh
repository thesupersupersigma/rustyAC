#!/bin/sh
# SPDX-License-Identifier: MIT OR Apache-2.0
# rustyAC server kit: lays one server's working folder out of the mounted pack and starts
# Kunos' acServer in it. The pack (/pack) is only read.
set -eu

name="${AC_SERVER:-spa}"
pack="${AC_PACK:-/pack}"
work="/srv/ac/$name"

if [ ! -f "$pack/acServer" ]; then
    echo "no acServer in $pack: mount the folder that server/make_server_pack.py made" >&2
    exit 1
fi
if [ ! -f "$pack/cfg/$name/server_cfg.ini" ]; then
    echo "no cfg/$name/server_cfg.ini in $pack (AC_SERVER=$name)" >&2
    exit 1
fi

mkdir -p "$work/cfg" "$work/results" "$work/setups"
ln -sfn "$pack/content" "$work/content"
ln -sfn "$pack/system" "$work/system"
# the binary is copied: the mount may be read-only or without the execute bit
cp "$pack/acServer" "$work/acServer"
chmod 755 "$work/acServer"
[ -f "$work/blacklist.txt" ] || : > "$work/blacklist.txt"

# the passwords come from the environment, never from git ('|' and '&' are escaped for sed)
escape() { printf '%s' "$1" | sed -e 's/[|&\\]/\\&/g'; }
admin="$(escape "${AC_ADMIN_PASSWORD:-}")"
join="$(escape "${AC_PASSWORD:-}")"
sed -e "s|@ADMIN_PASSWORD@|$admin|" -e "s|^PASSWORD=.*|PASSWORD=$join|" \
    "$pack/cfg/$name/server_cfg.ini" > "$work/cfg/server_cfg.ini"
cp "$pack/cfg/$name/entry_list.ini" "$work/cfg/entry_list.ini"

cd "$work"
exec ./acServer
