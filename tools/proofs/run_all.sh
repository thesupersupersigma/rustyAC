#!/bin/bash
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Every proof of the project (see tools/proofs/README.md):
#   bash tools/proofs/run_all.sh [--full]          every area, every case (≈ the milestone run); records
#                                                  the game's recordings again and holds them against the kept ones
#   bash tools/proofs/run_all.sh --quick           every area on the small samples of tools/proofs/quick/ (≈ 30 min)
#   bash tools/proofs/run_all.sh --changed <tag>   the areas the files changed since <tag> ask for
#                                                  (tools/proofs/areas.map), every case of each
#   ... [--rerecord] [--dry-run] [area ...]        record the game's recordings again / only say which
#                                                  areas would run / only these areas
# Needs: this repository, the ignored oracle/ and re/gold/ folders, the AC install (read only).
# Each area writes into target/proofs/<area> and ends with "PROOF <area> cases= identical= different=";
# the table below is made from those lines. Exit code 0 only when no area has a difference.
# The areas that never load the game's code (net browser ai_page shaders) run beside the others,
# which run one at a time; each lane builds with 4 jobs (CARGO_BUILD_JOBS), 8 in all.
here=$(cd "$(dirname "$0")" && pwd)
repo=$(cd "$here/../.." && pwd)
all="physics fresh sound render gui python replays net browser ai_page shaders launcher dumps"
side_all="net browser ai_page shaders"
mode=full; tag=; want=; dry=
while [ $# -gt 0 ]; do
    case "$1" in
        --quick) mode=quick ;;
        --full) mode=full ;;
        --changed) mode=changed; tag=$2; shift; [ -n "$tag" ] || { echo "--changed needs a tag" >&2; exit 2; } ;;
        --rerecord) export PROOF_RERECORD=1 ;;
        --dry-run) dry=1 ;;
        -*) echo "unknown option $1" >&2; exit 2 ;;
        *) want="$want $1" ;;
    esac
    shift
done
for a in $want; do [ -f "$here/$a.sh" ] || { echo "no area $a (areas: $all)" >&2; exit 2; }; done
areas=${want:-$all}
out=${PROOF_OUT:-$repo/target/proofs}
mkdir -p "$out/rows"
case $mode in
    quick) export PROOF_QUICK=1 ;;
    full) export PROOF_RERECORD=1 ;;
    changed)
        git -C "$repo" rev-parse -q --verify "$tag^{commit}" > /dev/null || { echo "no tag $tag" >&2; exit 2; }
        echo "files changed since $tag:"
        # committed since <tag> and not committed yet
        picked=$(git -C "$repo" diff --name-only "$tag" | PROOF_SINCE=$tag bash "$here/areas.sh" 2> "$out/changed.txt")
        sed 's/^/  /' "$out/changed.txt"
        areas=$(for a in $areas; do case " $picked " in *" $a "*) printf '%s ' "$a" ;; esac; done)
        ;;
esac
areas=$(echo $areas)
echo "mode $mode: ${areas:-no area}"
[ -n "$areas" ] && [ -z "$dry" ] || exit 0

rm -f "$out"/rows/*.row
game=; side=
for a in $areas; do case " $side_all " in *" $a "*) side="$side $a" ;; *) game="$game $a" ;; esac; done
[ -n "$game" ] && [ -n "$side" ] && export CARGO_BUILD_JOBS=${CARGO_BUILD_JOBS:-4}
export CARGO_BUILD_JOBS=${CARGO_BUILD_JOBS:-8}
start_all=$(date +%s)

# the shared tools, built once here (the areas skip what PROOF_BUILT names)
cd "$repo" || exit 2
built=
for t in car_oracle chassis_compare render_oracle audio_oracle; do
    case $t in
        car_oracle) uses="physics fresh replays" ;; chassis_compare) uses="physics fresh" ;;
        render_oracle) uses="render gui python" ;; audio_oracle) uses="sound" ;;
    esac
    for u in $uses; do
        case " $areas " in *" $u "*)
            echo "building $t"
            cargo build --release --locked --quiet --manifest-path "tools/$t/Cargo.toml" && built="$built $t"
            break ;;
        esac
    done
done
export PROOF_BUILT=$built
case " $areas " in *" fresh "*|*" replays "*|*" browser "*|*" dumps "*)
    echo "building rustyac, replay_hash"
    cargo build --release --locked --quiet -p rustyac-game --bin rustyac --bin replay_hash ;;
esac

run_area() {
    a=$1
    start=$(date +%s)
    bash "$here/$a.sh" > "$out/$a.log" 2>&1
    code=$?
    secs=$(( $(date +%s) - start ))
    line=$(grep '^PROOF ' "$out/$a.log" | tail -n 1)
    if [ -n "$line" ]; then
        c=$(echo "$line" | sed -E 's/.*cases=([0-9]+).*/\1/'); i=$(echo "$line" | sed -E 's/.*identical=([0-9]+).*/\1/'); d=$(echo "$line" | sed -E 's/.*different=([0-9]+).*/\1/')
    else
        c=-; i=-; d="did not run ($(tail -n 1 "$out/$a.log"))"
    fi
    printf '| %s | %s | %s | %s | %dm%02ds |\n' "$a" "$c" "$i" "$d" $((secs / 60)) $((secs % 60)) | tee "$out/rows/$a.row"
    [ "$code" = 0 ] || : > "$out/rows/$a.bad"
}
rm -f "$out"/rows/*.bad
if [ -n "$side" ]; then
    ( for a in $side; do run_area "$a"; done ) &
fi
for a in $game; do run_area "$a"; done
wait

secs=$(( $(date +%s) - start_all ))
table=$out/table.md
{
    echo "| area | cases | identical | different | time |"
    echo "|---|---|---|---|---|"
    for a in $areas; do cat "$out/rows/$a.row"; done
    printf '| all (%s, wall clock) | | | | %dh%02dm |\n' "$mode" $((secs / 3600)) $((secs % 3600 / 60))
} > "$table"
echo; cat "$table"
! ls "$out"/rows/*.bad > /dev/null 2>&1
