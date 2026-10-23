#!/bin/bash
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Self-check of areas.sh (run_all.sh --changed): made-up lists of changed files and the areas each must pick.
here=$(dirname "$0")
bad=0
check() {  # check "<expected areas>" <file>...
    want=$1; shift
    got=$(printf '%s\n' "$@" | bash "$here/areas.sh" 2>/dev/null)
    if [ "$got" = "$want" ]; then echo "ok    [$*] -> [$got]"; else echo "WRONG [$*] -> [$got], want [$want]"; bad=1; fi
}
all="physics fresh sound render gui replays net browser ai_page shaders launcher dumps"
check "" docs/port/proofs.md README.md
check "" prompts/38f_fast_proofs.md .github/workflows/release.yml
check "" crates/rustyac-physics/README.md
check "physics fresh sound replays net browser ai_page dumps" crates/rustyac-physics/src/car/tyre.rs
check "render gui shaders" crates/rustyac-render/src/fw1.rs docs/port/render.md
check "gui" crates/rustyac-gui/src/app_chat.rs
check "sound" crates/rustyac-audio/src/lib.rs
check "browser launcher" crates/rustyac-launcher/src/bridge.rs
check "browser ai_page" web/index.html crates/rustyac-web/src/controls.rs
check "net" server/panel/src/index.ts crates/rustyac-gateway/src/main.rs
check "fresh net" crates/rustyac-net/src/lib.rs re/gold/fresh/god_tc4.ini
check "physics fresh" tools/chassis_compare/src/main.rs
check "launcher" tools/proofs/launcher/career.sh
check "fresh sound replays browser ai_page launcher dumps" crates/rustyac-game/src/main.rs crates/rustyac-game/src/render/ac.rs
check "physics fresh sound replays browser ai_page launcher dumps" crates/rustyac-game/src/dump.rs
check "$all" Cargo.lock
check "$all" some/new/folder/file.rs
check "$all" tools/proofs/lib.sh docs/port/proofs.md
check "" tools/proofs/quick/gui.txt tools/proofs/areas.map
# since v0.38.3 car_oracle's lock changed only rustyAC's version; Cargo.toml without a tag still asks for all
PROOF_SINCE=v0.38.3 check "" tools/car_oracle/Cargo.lock
PROOF_SINCE=v0.38.3 check "net" crates/rustyac-gateway/Cargo.toml server/panel/src/main.ts
# a Cargo.lock change: the crates whose dependencies changed (v0.38.8: jpeg-decoder for rustyac-render,
# rustyac-render and png for rustyac-web); v0.38.3 to v0.38.4 changed only the version
lock() {  # lock "<expected crates>" <tag> <rev>
    got=$(python "$here/lock_crates.py" Cargo.lock "$2" "$3" | tr -d '\015' | tr '\n' ' ')
    if [ "$got" = "$1" ]; then echo "ok    [Cargo.lock $2..$3] -> [$got]"; else echo "WRONG [Cargo.lock $2..$3] -> [$got], want [$1]"; bad=1; fi
}
lock "" v0.38.3 v0.38.4
lock "crates/rustyac-render crates/rustyac-web " v0.38.7 v0.38.8
exit $bad
