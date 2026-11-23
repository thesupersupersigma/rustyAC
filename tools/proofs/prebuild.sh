#!/bin/bash
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Builds every program the areas run, except the browser's and ai_page's (Chrome, the web pack:
# they stay on the PC), with the commands the areas themselves use (so the same exes), and writes
# target/prebuilt/ for a run with RUSTYAC_PREBUILT=1 on a machine with no cargo, dotnet or git
# (the proof farm, docs/proof_farm.md; the stand-ins are in tools/proofs/prebuilt/):
#   manifest.txt   sha256 and path (from the repo root) of every program and file it needs, in
#                  `sha256sum -c` form; such a run stops when one of them is not these bytes
#   src/<tool>.txt what salt_of reads from git for tools/<tool> (its tree and diff)
#   tests/<name>.exe the test programs `cargo test --test <name>` would run
#   dumps_tag      the release the dumps area holds this version against (its exe: target/proofs/old_exe)
#   bash tools/proofs/prebuild.sh
set -e
repo=$(cd "$(dirname "$0")/../.." && pwd); cd "$repo"
export CARGO_BUILD_JOBS=${CARGO_BUILD_JOBS:-4}
R="--release --locked --quiet"
p=target/prebuilt; rm -rf $p; mkdir -p $p/src $p/tests
files=

for t in car_oracle chassis_compare render_oracle audio_oracle; do
    cargo build $R --manifest-path tools/$t/Cargo.toml
    files="$files tools/$t/target/release/$t.exe"
done
cargo build $R -p rustyac-game --bin rustyac --bin replay_hash
cargo build $R -p rustyac-physics --bin track_survey --bin acd_check
cargo build $R -p rustyac-launcher
files="$files target/release/rustyac.exe target/release/replay_hash.exe target/release/track_survey.exe target/release/acd_check.exe target/release/rustyac-launcher.exe"
# the examples the areas `cargo run` (fresh, shaders) or build and start themselves (net)
cargo build $R -p rustyac-game --example start_camera_check --example steam_check --example mirror_check --example achievements_check
cargo build $R -p rustyac-dxbc --example shader_table
cargo build $R -p rustyac-render --features wgpu --example shader_proof
cargo build $R -p rustyac-net --example net_golden
CARGO_TARGET_WASM32_WASIP1_RUSTFLAGS="--sysroot $(pwd -W)/target/wasm-sysroot" cargo build $R -p rustyac-net --example net_golden --target wasm32-wasip1
for e in start_camera_check steam_check mirror_check achievements_check shader_table shader_proof net_golden; do
    files="$files target/release/examples/$e.exe"
done
files="$files target/wasm32-wasip1/release/examples/net_golden.wasm target/web-tools/wasmtime/bin/wasmtime.exe"
# shaders' `cargo test --test dxbc_reflect`
exe=$(cargo test $R -p rustyac-render --test dxbc_reflect --no-run --message-format=json |
    python -c "import json, sys; print(*[m['executable'] for m in map(json.loads, sys.stdin) if m.get('executable')])")
cp "$exe" $p/tests/dxbc_reflect.exe
files="$files $p/tests/dxbc_reflect.exe"
# the launcher area's .NET oracle (restored from NuGet here, never on the farm)
nu=$repo/target/nuget; mkdir -p "$nu/tmp"
NUGET_PACKAGES="$(cygpath -w "$nu/packages")" DOTNET_CLI_HOME="$(cygpath -w "$nu/home")" TEMP="$(cygpath -w "$nu/tmp")" TMP="$(cygpath -w "$nu/tmp")" DOTNET_CLI_TELEMETRY_OPTOUT=1 \
    sh -c 'dotnet restore --locked-mode --source https://api.nuget.org/v3/index.json tools/launcher_oracle/launcher_oracle.csproj &&
        dotnet build -c Release --no-restore tools/launcher_oracle/launcher_oracle.csproj' > $p/dotnet.txt 2>&1 || { echo "dotnet build failed ($p/dotnet.txt)" >&2; exit 2; }
files="$files $(find tools/launcher_oracle/bin/Release -type f | tr '\n' ' ')"
# the dumps area's old release (old_exe.sh builds and keeps it once per tag)
tag=$(git tag --list 'v*' --sort=-v:refname --no-contains HEAD | head -n 1)
sh tools/proofs/old_exe.sh "$tag" > /dev/null  # built once per tag: a new tag on master needs nothing by hand
echo "$tag" > $p/dumps_tag
files="$files $(find "target/proofs/old_exe/$tag" -type f | tr '\n' ' ')"

# salt_of's input for every tool (the same lines lib.sh reads from git)
for d in tools/*/Cargo.toml; do
    t=$(basename "$(dirname "$d")")
    { git ls-tree -r HEAD -- "tools/$t" | grep -v 'Cargo\.lock$'; git diff HEAD -- "tools/$t" ":!tools/$t/Cargo.lock"; } > $p/src/$t.txt
done
# shellcheck disable=SC2086
sha256sum $files > $p/manifest.txt
echo "$(grep -c '' $p/manifest.txt) files in $p/manifest.txt, $(du -csh $files | tail -n 1 | cut -f1)"
