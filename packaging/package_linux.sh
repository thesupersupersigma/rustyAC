#!/bin/bash
# SPDX-License-Identifier: MIT OR Apache-2.0
# Packs the Linux release: rustyAC-vX.Y.Z-linux-x64.tar.gz and its .sha256, from the release build
# that is already in ${CARGO_TARGET_DIR:-target}/release, like package.ps1 does for Windows:
#
#   cargo build --release --locked -p rustyac-game --features wgpu --bin rustyac -p rustyac-launcher
#   packaging/package_linux.sh 1.2.0 <inputs> dist
#
# <inputs> holds what the repository cannot: FMOD Engine 2.03's libfmod.so.14 and
# libfmodstudio.so.14 (Firelight's, from fmod.com) and required/ (base.json and the dlc_<app>.json
# lists, made by `web_pack --required` from a full install: paths and sizes, no content). The list
# below is the whole content; the archive is read back and compared with it. No Assetto Corsa file
# is ever packed.
set -euo pipefail
version=${1:?the version, e.g. 1.2.0}
inputs=${2:?the folder with libfmod.so.14, libfmodstudio.so.14 and required/}
out=${3:-dist}
[[ $version =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || { echo "the version must look like 1.2.0, not '$version'" >&2; exit 1; }
root=$(cd "$(dirname "$0")/.." && pwd)
bin=${CARGO_TARGET_DIR:-$root/target}/release
[[ $bin = /* ]] || bin=$root/$bin
name=rustyAC-v$version-linux-x64
# source -> name in the archive (under $name/)
files=(
    "$bin/rustyac" rustyac
    "$bin/rustyac-launcher" rustyac-launcher
    "$inputs/libfmod.so.14" libfmod.so.14
    "$inputs/libfmodstudio.so.14" libfmodstudio.so.14
    "$root/README.md" README.md
    "$root/LICENSING.md" LICENSING.md
    "$root/THIRD_PARTY_NOTICES.txt" THIRD_PARTY_NOTICES.txt
    "$root/LICENSE-GPL" LICENSE-GPL
    "$root/LICENSE-MIT" LICENSE-MIT
    "$root/LICENSE-APACHE" LICENSE-APACHE
    "$root/crates/rustyac-ode/LICENSE-ODE" crates/rustyac-ode/LICENSE-ODE
    "$root/packaging/linux_check.sh" linux_check.sh
)
[[ -f $inputs/required/base.json ]] || { echo "$inputs/required/base.json is missing" >&2; exit 1; }
for list in "$inputs"/required/*.json; do
    case $(basename "$list") in base.json | dlc_*.json) files+=("$list" "required/$(basename "$list")") ;; esac
done
for ((i = 0; i < ${#files[@]}; i += 2)); do
    [[ -f ${files[i]} ]] || { echo "${files[i]} is missing (build first, or the inputs)" >&2; exit 1; }
done
mkdir -p "$out"
out=$(cd "$out" && pwd)
stage=$out/$name
rm -rf "$stage" "$out/$name.tar.gz" "$out/$name.tar.gz.sha256"
for ((i = 0; i < ${#files[@]}; i += 2)); do
    install -D -m "$([[ ${files[i + 1]} = rustyac* || ${files[i + 1]} = *.sh || ${files[i + 1]} = *.so.* ]] && echo 755 || echo 644)" "${files[i]}" "$stage/${files[i + 1]}"
done
sed -e "s/@VERSION@/$version/g" "$root/packaging/HOW_TO_RUN_LINUX.txt" > "$stage/HOW_TO_RUN.txt"
# the same bytes from the same files: sorted names, no owner, the files' times kept
tar -C "$out" --sort=name --owner=0 --group=0 --numeric-owner -czf "$out/$name.tar.gz" "$name"
rm -rf "$stage"
# read it back: exactly the expected names
expected=$( { for ((i = 1; i < ${#files[@]}; i += 2)); do echo "$name/${files[i]}"; done; echo "$name/HOW_TO_RUN.txt"; } | sort)
packed=$(tar -tzf "$out/$name.tar.gz" | grep -v '/$' | sort)
[[ $expected == "$packed" ]] || { echo "the archive does not hold what it should:" >&2; diff <(echo "$expected") <(echo "$packed") >&2; exit 1; }
(cd "$out" && sha256sum "$name.tar.gz" > "$name.tar.gz.sha256")
echo "packed $out/$name.tar.gz ($(du -h "$out/$name.tar.gz" | cut -f1))"
echo "$packed" | sed 's/^/  /'
cat "$out/$name.tar.gz.sha256"
