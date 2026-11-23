#!/bin/bash
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Every proof of the project (see tools/proofs/README.md):
#   bash tools/proofs/run_all.sh [--full]          every area, every case (≈ the milestone run), the port
#                                                  against the game's kept results (--rerecord: the game again too)
#   bash tools/proofs/run_all.sh --quick           every area on the small samples of tools/proofs/quick/ (≈ 30 min)
#   bash tools/proofs/run_all.sh --changed <tag>   the areas the files changed since <tag> ask for
#                                                  (tools/proofs/areas.map), every case of each
#   ... [--rerecord] [--dry-run] [area ...]        record the game's recordings again / only say which
#                                                  areas would run / only these areas
#   ... --farm                                     the areas of areas.map's @farm line in shards on one queue that
#                                                  the proof farm's VM and this PC's free lanes both take from
#                                                  (tools/proofs/farm.sh, docs/proof_farm.md), the rest here;
#                                                  stops at once if the farm isn't set up or doesn't answer
#   ... --farm-fallback                            the same, but then (only then) its areas run here
# Needs: this repository, the ignored oracle/ and re/gold/ folders, the AC install (read only).
# Each area writes into target/proofs/<area> and ends with "PROOF <area> cases= identical= different= errors= skipped= expected=";
# the table below is made from those lines. Exit code 0 only when no area has a difference or an
# ERROR (a crash, running out of memory or an error exit of either side: run once more, alone, after
# all lanes, and the new verdict replaces it).
# The areas run in lanes (below); the shared programs are built first with 8 jobs, then what an
# area builds for itself with 2 jobs a lane (CARGO_BUILD_JOBS), 8 in all.
here=$(cd "$(dirname "$0")" && pwd)
repo=$(cd "$here/../.." && pwd)
all="physics fresh csp sound render gui python replays net browser ai_page shaders launcher dumps yebis_sweep"
# the lanes, each one area after another, all lanes at once: two lanes load the game's code (the
# render oracle's and the car oracle's), never two areas of one lane; the others never load it
# (a render or GUI compare keeps about 1.5 of the 16 threads busy, Task P1). PROOF_LANES: other lanes;
# a word <area>.<i> runs shard i of n of the area's cases (lib.sh's PROOF_SHARD, i from 1 to n, n the
# number of such words in the lanes and the queue, PROOF_QUEUE below), merged into one row once every
# lane is done
lanes=${PROOF_LANES:-"render gui python yebis_sweep|fresh csp sound replays launcher|physics dumps shaders|net browser ai_page"}
mode=full; tag=; want=; dry=; farm=; farm_fallback=
while [ $# -gt 0 ]; do
    case "$1" in
        --quick) mode=quick ;;
        --full) mode=full ;;
        --changed) mode=changed; tag=$2; shift; [ -n "$tag" ] || { echo "--changed needs a tag" >&2; exit 2; } ;;
        --rerecord) export PROOF_RERECORD=1 ;;
        --dry-run) dry=1 ;;
        --farm) farm=1 ;;
        --farm-fallback) farm=1; farm_fallback=1 ;;
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
    full) ;;
    changed)
        git -C "$repo" rev-parse -q --verify "$tag^{commit}" > /dev/null || { echo "no tag $tag" >&2; exit 2; }
        echo "files changed since $tag:"
        # committed since <tag> and not committed yet
        picked=$(git -C "$repo" diff --name-only "$tag" | PROOF_SINCE=$tag bash "$here/areas.sh" 2> "$out/changed.txt")
        sed 's/^/  /' "$out/changed.txt"
        areas=$(for a in $areas; do case " $picked " in *" $a "*) printf '%s ' "$a" ;; esac; done)
        ;;
esac
# the YEBIS settings sweep (≈ 1h30m) runs in --full only, or when named
[ "$mode" = full ] || [ -n "$want" ] || areas=$(echo " $areas " | sed 's/ yebis_sweep / /')
areas=$(echo $areas)
# --farm: the areas that may go to the proof farm
farm_areas=
if [ -n "$farm" ]; then
    fl=$(sed -n 's/^@farm //p' "$here/areas.map")
    farm_areas=$(for a in $areas; do case " $fl " in *" $a "*) printf '%s ' "$a" ;; esac; done)
fi
echo "mode $mode: ${areas:-no area}${farm:+ (on the farm: ${farm_areas:-none})}"
[ -n "$areas" ] && [ -z "$dry" ] || exit 0

# one proof run at a time on this PC, whatever the checkout (a second one waits here)
. "$here/lock.sh"
proof_lock "$repo"

rm -f "$out"/rows/*.row "$out"/rows/*.where "$out/run.status"
# pick_lanes <areas>: each lane's words for these areas, in the lane's order; an area no lane names
# gets a lane of its own
pick_lanes() {
    p=$(echo "$lanes" | tr '|' '\n' | while read -r lane; do
        l=$(for a in $lane; do case " $1 " in *" ${a%.[0-9]*} "*) printf '%s ' "$a" ;; esac; done)
        [ -n "$l" ] && echo "$l"
    done)
    for a in $1; do echo " $p " | tr '\n' ' ' | grep -qE " $a(\.[0-9]+)? " || p="$p
$a"; done
    echo "$p" | grep .
}
# a fallback list left by an earlier --farm run must not move this run's areas (farm_up clears it on --farm)
[ -n "$farm" ] || rm -f "$out/farm/fallback"
# --farm: can the VM take its areas (tools/proofs/farm.sh; those it can't run here, from the start)
if [ -n "$farm_areas" ]; then
    . "$here/farm.sh"
    farm_up
    [ -z "$farm_areas" ] || PROOF_QUEUE=$fs/run/queue
fi
# PROOF_QUEUE=<dir> (the proof farm, Task F4): <dir>/list, words shared with another machine; every
# lane, once its own words are done, takes the next one nobody took (claim), PROOF_PULL (1) more lanes
# do nothing else; the areas of the list run only so
qwords=$( [ -z "$PROOF_QUEUE" ] || tr -d '\r' < "$PROOF_QUEUE/list")
queued=$(for w in $qwords; do echo "${w%.[0-9]*}"; done | sort -u | tr '\n' ' ')
pullers=0; [ -z "$qwords" ] || pullers=${PROOF_PULL:-1}
pc_areas=$(for a in $areas; do case " $farm_areas $queued " in *" $a "*) ;; *) printf '%s ' "$a" ;; esac; done)
picked_lanes=$(pick_lanes "$pc_areas")
# RUSTYAC_PREBUILT=1: the programs of tools/proofs/prebuild.sh (the builds below do nothing)
. "$here/prebuilt/env.sh"
jobs=${CARGO_BUILD_JOBS:-8}
export CARGO_BUILD_JOBS=$jobs
start_all=$(date +%s)

# every program more than one area runs, built once here before any area starts (the areas skip
# what PROOF_BUILT names): no area relinks an exe another lane is running, and a source changed
# during the run changes nothing in it
cd "$repo" || exit 2
built=
for t in car_oracle chassis_compare render_oracle audio_oracle; do
    case $t in
        car_oracle) uses="physics fresh csp replays" ;; chassis_compare) uses="physics fresh csp" ;;
        render_oracle) uses="render gui python fresh yebis_sweep" ;; audio_oracle) uses="sound" ;;
    esac
    for u in $uses; do
        case " $pc_areas " in *" $u "*)
            echo "building $t"
            cargo build --release --locked --quiet --manifest-path "tools/$t/Cargo.toml" && built="$built $t"
            break ;;
        esac
    done
done
case " $pc_areas " in *" fresh "*|*" sound "*|*" replays "*|*" browser "*|*" ai_page "*|*" dumps "*)
    echo "building rustyac, replay_hash"
    cargo build --release --locked --quiet -p rustyac-game --bin rustyac --bin replay_hash && built="$built rustyac replay_hash" ;;
esac
case " $pc_areas " in *" fresh "*|*" csp "*)
    echo "building track_survey"
    cargo build --release --locked --quiet -p rustyac-physics --bin track_survey && built="$built track_survey" ;;
esac
case " $pc_areas " in *" launcher "*)
    echo "building rustyac-launcher"
    cargo build --release --locked --quiet -p rustyac-launcher && built="$built rustyac-launcher" ;;
esac
# a farm area that comes back to run here finds prebuild.sh's programs built
[ -z "$farm_areas" ] || built="$built car_oracle chassis_compare render_oracle audio_oracle rustyac replay_hash track_survey rustyac-launcher"
export PROOF_BUILT=$built
# less than 12 GB of memory free: two lanes joined into one, so fewer areas run at once
free_kb=$(awk '/^MemFree:/ {print $2}' /proc/meminfo 2> /dev/null)
if [ "${free_kb:-0}" -lt 12582912 ] && [ "$(echo "$picked_lanes" | grep -c .)" -gt 2 ] && [ -z "$PROOF_LANES" ]; then
    echo "$((free_kb / 1048576)) GB free: fewer lanes"
    picked_lanes=$(echo "$picked_lanes" | paste -d' ' - -)
fi
n=$(( $(echo "$picked_lanes" | grep -c .) + pullers ))
[ "$n" -gt 0 ] || n=1
export CARGO_BUILD_JOBS=$(( jobs / n > 0 ? jobs / n : 1 ))
# run.status: what tools/proof_tui watches (the mode, the start, each lane; "end" when done); a lane
# that takes from the queue is "lane @<k>", its words in lanes/<k>.lane as it takes them
{ echo "mode $mode"; echo "start $start_all"; } > "$out/run.status"
rm -rf "$out/lanes"; mkdir -p "$out/lanes"; lane_k=0

# row <area> <PROOF line> <seconds>: the area's row of the table
row() {
    if [ -n "$2" ]; then
        c=$(echo "$2" | sed -E 's/.*cases=([0-9]+).*/\1/'); i=$(echo "$2" | sed -E 's/.*identical=([0-9]+).*/\1/'); d=$(echo "$2" | sed -E 's/.*different=([0-9]+).*/\1/')
        e=$(echo "$2" | sed -nE 's/.*errors=([0-9]+).*/\1/p'); k=$(echo "$2" | sed -nE 's/.*skipped=([0-9]+).*/\1/p'); x=$(echo "$2" | sed -nE 's/.*expected=([0-9]+).*/\1/p')
    else
        c=-; i=-; d="did not run ($(tail -n 1 "$out/$1.log"))"; e=-; k=-; x=-
    fi
    printf '| %s | %s | %s | %s | %s | %s | %s | %dm%02ds |\n' "$1" "$c" "$i" "$d" "${e:-0}" "${k:-0}" "${x:-0}" $(($3 / 60)) $(($3 % 60)) | tee "$out/rows/$1.row"
}
run_area() {
    a=$1; shard=
    # <area>.<i>: shard i of n, n the number of the area's words in the lanes and the queue
    case $a in *.[0-9]*) shard=${a##*.}/$(shards "${a%.*}" | grep -c .) ;; esac
    start=$(date +%s)
    PROOF_SHARD=$shard bash "$here/${a%.[0-9]*}.sh" > "$out/$a.log" 2>&1
    code=$?
    secs=$(( $(date +%s) - start ))
    echo "$secs" > "$out/rows/$a.secs"
    row "$a" "$(grep '^PROOF ' "$out/$a.log" | tail -n 1)" "$secs"
    [ "$code" = 0 ] || : > "$out/rows/$a.bad"
}
# merge <area>: its shards' cases, logs and times as one area (the time: the longest shard's)
merge() {
    m=$1; d=$out/$m
    sh=$(shards "$m") || return 0
    rm -rf "$d" && mkdir -p "$d"; : > "$out/$m.log"; ms=0; ran=1
    for s in $sh; do
        rm -f "$out/rows/$s.bad"  # the merged row says it (an ERROR run again alone clears only the area's)
        cat "$out/$s/cases.log" >> "$d/cases.log" 2> /dev/null; cat "$out/$s/times.tsv" >> "$d/times.tsv" 2> /dev/null
        grep -q '^PROOF ' "$out/$s.log" || ran=
        { echo "== shard $s"; cat "$out/$s.log"; } >> "$out/$m.log"
        t=$(cat "$out/rows/$s.secs"); [ "$t" -le "$ms" ] || ms=$t
    done
    echo "$ms" > "$out/rows/$m.secs"
    # a case in two shards ran outside keep(): it would be counted twice
    sed -nE 's/^(IDENTICAL|DIFFERENT|ERROR|SKIPPED|EXPECTED) ([^|]*[^ |]) *(\|.*)?$/\2/p' "$d/cases.log" | sort | uniq -d |
        sed 's/^/ERROR /; s/$/ | in two shards: a verdict outside keep()/' >> "$d/cases.log"
    l=$d/cases.log
    i=$(grep -c '^IDENTICAL ' "$l"); f=$(grep -c '^DIFFERENT ' "$l"); e=$(grep -c '^ERROR ' "$l"); k=$(grep -c '^SKIPPED ' "$l"); x=$(grep -c '^EXPECTED ' "$l")
    if [ -n "$ran" ]; then row "$m" "PROOF $m cases=$((i + f + e)) identical=$i different=$f errors=$e skipped=$k expected=$x" "$ms"
    else row "$m" "" "$ms"; fi
    [ -n "$ran" ] && [ $((f + e)) = 0 ] || : > "$out/rows/$m.bad"
}
# shards <area>: its words <area>.<i> in the lanes and the queue
shards() { echo $picked_lanes $qwords | tr ' ' '\n' | grep "^$1\.[0-9]*$"; }
# claim: the next word of the queue nobody took (none: fails); a claim is a mkdir in <queue>/claim,
# which NTFS makes for exactly one machine (the VM on its own disk, the PC over SMB); a share that
# fails otherwise (gone) ends this lane's taking, and what is left runs after (fallback)
claim() {
    [ -d "$PROOF_QUEUE/claim" ] || return 1
    for w in $qwords; do
        e=$(mkdir "$PROOF_QUEUE/claim/$w" 2>&1) && { echo "$w"; return 0; }
        case $e in *'File exists'*) ;; *) return 1 ;; esac
    done
    return 1
}
# lane <words...>: one lane, its words one after another, then (with a queue) every word it takes
lane() {
    for a in "$@"; do run_area "$a"; done
    [ -n "$qwords" ] || return 0
    while w=$(claim); do echo "$w" >> "$out/lanes/$lane_k.lane"; run_area "$w"; done
}
# run_lanes <areas> [n]: every lane of $picked_lanes and n lanes that only take from the queue, all at
# once, then the sharded areas merged
run_lanes() {
    pids=
    while read -r l; do
        [ -n "$l" ] || continue
        [ "$l" != @ ] || l=  # a lane that only takes
        lane_k=$((lane_k + 1))
        if [ -n "$qwords" ]; then echo $l > "$out/lanes/$lane_k.lane"; echo "lane @$lane_k"; else echo "lane $l"; fi >> "$out/run.status"
        # shellcheck disable=SC2086
        lane $l &
        pids="$pids $!"
    done <<LANES
$picked_lanes
$(seq "${2:-0}" | sed 's/.*/@/')
LANES
    # shellcheck disable=SC2086
    [ -z "$pids" ] || wait $pids  # a bare wait would wait for the farm half too
    for a in $1; do merge "$a"; done
}
rm -f "$out"/rows/*.bad
# the farm half beside the PC's lanes (farm.sh)
farm_pid=
if [ -n "$farm_areas" ]; then farm_go > "$out/farm/go.txt" 2>&1 & farm_pid=$!; fi
run_lanes "$pc_areas" "$pullers"
if [ -n "$farm_pid" ]; then
    echo "waiting for the farm (target/proofs/farm/farm.txt)"
    wait "$farm_pid"
    cat "$out/farm/farm.txt"
fi
if [ -n "$qwords" ] && [ -n "$farm" ]; then
    # every word of the queue with no row here: the VM did not finish it (stopped, died, its results
    # did not come back, or it never took it); here now, from a queue of only those (the shard counts
    # stay the list's), why in the table's "where"; then the queue's areas merged
    again=$out/farm/again; rm -rf "$again"; mkdir -p "$again/claim"
    left=
    for w in $qwords; do
        if [ -f "$out/rows/$w.row" ]; then mkdir "$again/claim/$w"; continue; fi
        r=$(awk -v w="$w" '$1 == w { sub(/^[^ ]+ /, ""); print; exit }' "$out/farm/fallback" 2> /dev/null)
        echo "PC (VM: ${r:-$(cat "$out/farm/why" 2> /dev/null || echo 'it did not take it')})" > "$out/rows/$w.where"
        left="$left $w"
    done
    if [ -n "$left" ]; then
        echo "running here what the farm did not finish:$left"
        PROOF_QUEUE=$again picked_lanes= run_lanes "" 4
    fi
    for a in $queued; do merge "$a"; done
# the farm areas that could not run there (a farm reason: the VM off, ssh, the share, prebuild; never
# a DIFFERENT, which is a result) run here now; why, in the table's "where"
elif [ -f "$out/farm/fallback" ]; then
    while read -r a why; do echo "PC (VM: $why)" > "$out/rows/$a.where"; done < "$out/farm/fallback"
    late=$(for a in $(cut -d' ' -f1 "$out/farm/fallback"); do case " $pc_areas " in *" $a "*) ;; *) printf '%s ' "$a" ;; esac; done)
    if [ -n "$late" ]; then
        echo "running here what the farm could not: $late"
        picked_lanes=$(pick_lanes "$late")
        run_lanes "$late"
    fi
fi

# the ERROR cases once more, each area's alone (no other lane running): the new verdict replaces the
# ERROR line; shortcut: a case whose quick key is not its name (launcher, sound's e2e) does not run
# again and stays ERROR
for a in $areas; do
    [ "$(cat "$out/rows/$a.where" 2> /dev/null)" != VM ] || continue  # the VM ran its own again, alone
    grep -q '^ERROR ' "$out/$a/cases.log" 2> /dev/null || continue
    sed -nE 's/^ERROR ([^|]*[^ |]) *(\|.*)?$/\1/p' "$out/$a/cases.log" | sed 's/[][\.*^$+?(){}|/]/\\&/g' > "$out/rows/$a.rerun"
    echo "running the $(grep -c '' "$out/rows/$a.rerun") ERROR cases of $a again, alone"
    PROOF_OUT=$out/rerun PROOF_QUICK=1 PROOF_QUICK_RE=$out/rows/$a.rerun bash "$here/$a.sh" > "$out/rerun_$a.log" 2>&1
    awk 'function key(l) { sub(/^[A-Z]+ /, "", l); sub(/ *[|].*$/, "", l); return l }
        NR == FNR { if ($0 ~ /^(IDENTICAL|DIFFERENT|ERROR|SKIPPED|EXPECTED) /) again[key($0)] = $0; next }
        /^ERROR / && (key($0) in again) { print again[key($0)] " (run again alone)"; next } { print }' \
        "$out/rerun/$a/cases.log" "$out/$a/cases.log" > "$out/rows/$a.cases" && mv "$out/rows/$a.cases" "$out/$a/cases.log"
    l=$out/$a/cases.log
    i=$(grep -c '^IDENTICAL ' "$l"); d=$(grep -c '^DIFFERENT ' "$l"); e=$(grep -c '^ERROR ' "$l"); k=$(grep -c '^SKIPPED ' "$l"); x=$(grep -c '^EXPECTED ' "$l")
    row "$a" "PROOF $a cases=$((i + d + e)) identical=$i different=$d errors=$e skipped=$k expected=$x" "$(cat "$out/rows/$a.secs")"
    rm -f "$out/rows/$a.bad"; [ $((d + e)) = 0 ] || : > "$out/rows/$a.bad"
done

secs=$(( $(date +%s) - start_all ))
table=$out/table.md
if [ -z "$farm" ]; then
    {
        echo "| area | cases | identical | different | errors | skipped | expected skips | time |"
        echo "|---|---|---|---|---|---|---|---|"
        for a in $areas; do cat "$out/rows/$a.row"; done
        printf '| all (%s, wall clock) | | | | | | | %dh%02dm |\n' "$mode" $((secs / 3600)) $((secs % 3600 / 60))
    } > "$table"
else
    # --farm: where each area ran; physics on the VM proves the pure maths path (pure.rs, the browser's),
    # the PC's runs the DLL's (docs/proof_farm.md)
    # where_of <area>: its where, or per machine its shards ("VM 1,3; PC 2")
    where_of() {
        cat "$out/rows/$1.where" 2> /dev/null && return
        s=$(shards "$1") || { echo PC; return; }
        for w in $s; do echo "$(cat "$out/rows/$w.where" 2> /dev/null || echo PC)|${w##*.}"; done |
            awk -F'|' '{ if ($1 in m) m[$1] = m[$1] "," $2; else { o[++n] = $1; m[$1] = $2 } }
                END { for (i = 1; i <= n; i++) printf "%s%s %s", i > 1 ? "; " : "", o[i], m[o[i]]; print "" }'
    }
    note=
    case "$(where_of physics)" in VM*|*"; VM "*) note="; physics on the VM proves pure.rs's maths, not MSVCR120.dll's" ;; esac
    [ ! -f "$out/farm/why" ] || note="$note; the farm stopped: $(cat "$out/farm/why"), the PC took the rest"
    {
        echo "| area | where | cases | identical | different | errors | skipped | expected skips | time |"
        echo "|---|---|---|---|---|---|---|---|---|"
        for a in $areas; do
            w=$(where_of "$a")
            awk -v a="$a" -v w="$w" '{ sub("^[|] " a " [|]", "| " a " | " w " |"); print }' "$out/rows/$a.row"
        done
        printf '| all (%s --farm, wall clock%s) | | | | | | | | %dh%02dm |\n' "$mode" "$note" $((secs / 3600)) $((secs % 3600 / 60))
    } > "$table"
fi
echo; cat "$table"
# every ERROR case (red): a crash, out of memory or an error exit, still there when run alone
for a in $areas; do grep -h '^ERROR ' "$out/$a/cases.log" 2> /dev/null | sed "s/^ERROR /$a /"; done > "$out/errors.txt"
n=$(grep -c '' "$out/errors.txt")
if [ "$n" -gt 0 ]; then printf '\n\033[31m%s cases ERROR\033[0m (%s):\n' "$n" "$out/errors.txt"; sed 's/^/  /' "$out/errors.txt"
else printf '\n0 cases ERROR\n'; fi
# every case left out, with its reason (red when there is one: nothing drops out silently)
for a in $areas; do grep -h '^SKIPPED ' "$out/$a/cases.log" 2> /dev/null; done > "$out/skipped.txt"
n=$(grep -c '' "$out/skipped.txt")
if [ "$n" -gt 0 ]; then printf '\n\033[31m%s cases skipped\033[0m (%s):\n' "$n" "$out/skipped.txt"; sed 's/^SKIPPED /  /' "$out/skipped.txt"
else printf '\n0 cases skipped\n'; fi
# every case the last run of this mode had and this one has not (red: nothing drops out silently);
# a case retired on purpose goes in tools/proofs/retired.txt ("<area> <case> | reason")
mkdir -p "$out/cases/$mode"
for a in $areas; do
    [ -f "$out/$a/cases.log" ] || continue
    sed -nE 's/^(IDENTICAL|DIFFERENT|ERROR|SKIPPED|EXPECTED) ([^|]*[^ |]) *(\|.*)?$/\2/p' "$out/$a/cases.log" | sort -u > "$out/cases/$mode/$a.new"
    [ -f "$out/cases/$mode/$a.txt" ] && sort -u -o "$out/cases/$mode/$a.txt" "$out/cases/$mode/$a.txt" && comm -23 "$out/cases/$mode/$a.txt" "$out/cases/$mode/$a.new" |
        grep -vxF -f <(sed -nE "s/^$a ([^|]*[^ |]) *\|.*/\1/p" "$here/retired.txt") | sed "s/^/$a /"
    mv "$out/cases/$mode/$a.new" "$out/cases/$mode/$a.txt"
    cp "$out/$a/times.tsv" "$out/cases/$mode/$a.times" 2> /dev/null  # for proof_tui's next ETA
done > "$out/vanished.txt"
n=$(grep -c '' "$out/vanished.txt")
if [ "$n" -gt 0 ]; then printf '\n\033[31m%s cases vanished since the last %s run\033[0m (%s):\n' "$n" "$mode" "$out/vanished.txt"; sed 's/^/  /' "$out/vanished.txt"
else printf '0 cases vanished since the last %s run\n' "$mode"; fi
echo "end $(date +%s)" >> "$out/run.status"
! ls "$out"/rows/*.bad > /dev/null 2>&1
