# SPDX-License-Identifier: GPL-3.0-or-later
#
# run_all.sh --farm (sourced by run_all.sh, never run): the areas of areas.map's @farm line run on
# the proof farm's VM (docs/proof_farm.md) while the others run here. Needs, from the environment
# or %USERPROFILE%\.rustyac\farm.env (beside the proof lock, so every checkout reads the same one;
# never in the repo: it names the VM):
#   FARM_HOST   ssh's user@host (key login); the user is also the scheduled task's
#   FARM_SHARE  //<vm>/<share>, the VM's farm folder (repo\, ac\, home\, run\) over SMB
#   FARM_DIR    that folder on the VM (default C:\rustyac-farm)
#   FARM_SYNC   farm_sync.sh's parts sent before each run (default: programs repo ac; about
#               2 min with nothing changed, ac alone 15 s)
#   FARM_STALL  minutes with no new verdict on the VM before the farm counts as stalled (default 60)
# Everything the farm half says is in target/proofs/farm/ (its log farm.txt; the VM's status files,
# mirrored for proof_tui; fallback: "<area> <reason>" for each area that runs here instead).
farm_env=${USERPROFILE:-$HOME}/.rustyac/farm.env
_h=$FARM_HOST _s=$FARM_SHARE  # the environment wins over farm.env
[ ! -f "$farm_env" ] || . "$farm_env"
FARM_HOST=${_h:-$FARM_HOST} FARM_SHARE=${_s:-$FARM_SHARE}
farm_dir=${FARM_DIR:-'C:\rustyac-farm'}
fs=$(echo "$FARM_SHARE" | tr '\\' /)   # //vm/share, for bash
fw=$(echo "$fs" | tr / '\\')           # \\vm\share, for robocopy
fo=$out/farm
fp=$fs/repo/target/proofs              # the VM's target/proofs
# the shared queue (Task F4): every farm area's words in one list on the share (run\queue\list),
# longest first; the PC's lanes (after their own areas) and the VM's eight lanes all take the next
# word nobody took (run_all.sh's claim: a mkdir in run\queue\claim, which NTFS lets exactly one of
# them make, the VM on its own disk, the PC over SMB); a word started on one machine finishes there
farm_vm_lanes=8
farm_ssh() { ssh -o BatchMode=yes -o ConnectTimeout=15 "$FARM_HOST" "$@" < /dev/null; }
# farm_off <reason>: every farm area left runs on this PC instead
farm_off() {
    for a in $farm_areas; do echo "$a $1"; done >> "$fo/fallback"
    echo "farm off: $1" >> "$fo/farm.txt"
    farm_areas=
}
# farm_stop <reason>: at the start, the farm can't take the run: stop before any area runs (a run
# that silently goes to the PC takes hours longer), or with --farm-fallback run its areas here
farm_stop() {
    [ -z "$farm_fallback" ] || { farm_off "$1"; return; }
    echo "the farm $1; run without --farm (or with --farm-fallback to run its areas here), or set it up: docs/proof_farm.md, \"Setting up the farm\"" >&2
    exit 2
}
# back <dir under the share> <local dir> [robocopy options and file names]: from the VM to here
back() {
    _s="$fw\\$(echo "$1" | tr / '\\')"; _d=$(cygpath -w "$2"); shift 2
    MSYS2_ARG_CONV_EXCL='*' robocopy "$_s" "$_d" "$@" /R:2 /W:5 /NP /NDL /NFL /BYTES >> "$fo/back.txt" 2>&1
    [ $? -lt 8 ]
}

# farm_up: before any lane starts (run_all.sh's shell): can the VM take the run, and its programs
farm_up() {
    rm -rf "$fo"; mkdir -p "$fo"
    case $fs in //?*/?*) ;; *) farm_stop "isn't set up: no FARM_SHARE in $farm_env"; return ;; esac
    [ -n "$FARM_HOST" ] || { farm_stop "isn't set up: no FARM_HOST in $farm_env"; return; }
    farm_ssh exit 2> "$fo/ssh.txt" || { farm_stop "doesn't answer: ssh $FARM_HOST: $(tail -n 1 "$fo/ssh.txt")"; return; }
    [ -d "$fs/repo" ] || { farm_stop "doesn't answer: no $fs/repo"; return; }
    if farm_ssh schtasks /query /tn rustyac_farm /fo csv /nh 2> /dev/null | grep -q '"Running"'; then
        farm_stop "is busy: the VM is still running an earlier farm run"; return
    fi
    # every program the VM runs, the dumps area's old release for the newest tag included
    echo "building the farm's programs (prebuild.sh)"
    bash "$here/prebuild.sh" > "$fo/prebuild.txt" 2>&1 || { farm_stop "can't start: prebuild.sh failed ($fo/prebuild.txt)"; return; }
    farm_queue
}

# farm_queue: the queue's words: an area whose last run of this mode took more than $target s (the
# sum of cases/<mode>/<area>.times, which every run rewrites: the order improves itself) in shards
# of about that, the words longest first; an area with no times yet first, whole
farm_queue() {
    if [ "$mode" = quick ]; then target=480; else target=1800; fi
    for a in $farm_areas; do
        t=$out/cases/$mode/$a.times; [ -f "$t" ] || t=$out/cases/full/$a.times
        s=$(awk '{ s += $1 } END { print int(s) }' "$t" 2> /dev/null)
        [ "${s:-0}" -gt 0 ] || s=999999
        n=$(( (s + target - 1) / target ))
        # net.sh checks its goldens in one check.sh run (no keep()), so it can't be split
        [ "$a" != net ] && [ "$s" != 999999 ] || n=1
        if [ "$n" = 1 ]; then echo "$s $a"; else for i in $(seq "$n"); do echo "$((s / n)) $a.$i"; done; fi
    done | sort -s -k1,1nr | cut -d' ' -f2 > "$fo/queue"
    rm -rf "$fs/run/queue" && mkdir -p "$fs/run/queue/claim" && cp "$fo/queue" "$fs/run/queue/list" ||
        farm_stop "can't start: the queue on the share (run\queue) is not writable"
}

# farm_why <reason>: the farm stopped during the run; what the VM did not finish runs here after the
# PC's lanes, which meanwhile go on taking from the queue (run_all.sh)
farm_why() { echo "$1" > "$fo/why"; echo "farm off: $1" >> "$fo/farm.txt"; }
# farm_go: the farm half, beside the PC's lanes (in the background): the copy, the VM's run, the
# results back of every word the VM finished; "<word> <reason>" in $fo/fallback for one it could not
farm_go() {
    t0=$(date +%s)
    # shellcheck disable=SC2086
    FARM_SHARE=$fs bash "$here/farm_sync.sh" ${FARM_SYNC:-programs repo ac} > "$fo/sync.txt" 2>&1 ||
        { farm_why "the copy to the VM failed ($(tail -n 1 "$fo/sync.txt"))"; return; }
    t1=$(date +%s); echo "copy: $((t1 - t0)) s; $(grep '^sync ' "$fo/sync.txt" | paste -sd ';' -)" >> "$fo/farm.txt"
    # nothing of an earlier VM run may count for this one
    mkdir -p "$fs/run" "$fp/rows" && rm -f "$fs/run/exit" "$fs/run/started" "$fs/run/new_kept.txt" "$fp"/rows/* ||
        { farm_why "the VM's run folder is not writable"; return; }
    { if [ "$mode" = quick ]; then echo --quick; else echo --full; fi
      [ -z "$PROOF_RERECORD" ] || echo --rerecord
      printf '%s\n' $farm_areas; } > "$fs/run/args"
    echo "$farm_vm_lanes" > "$fs/run/pull"
    farm_ssh "schtasks /create /tn rustyac_farm /tr \"$farm_dir\\repo\\tools\\proofs\\farm_task.cmd\" /sc once /st 23:59 /it /ru ${FARM_HOST%@*} /f && schtasks /run /tn rustyac_farm" > "$fo/task.txt" 2>&1 ||
        { farm_why "the scheduled task did not start ($(tr -d '\r' < "$fo/task.txt" | grep . | tail -n 1))"; return; }
    # wait, mirroring the VM's status files for proof_tui; bounded: the start, the share, the verdicts
    why= started= sig= last=$(date +%s) gone=0
    until [ -f "$fs/run/exit" ]; do
        sleep 30
        if [ ! -d "$fs/run" ]; then
            gone=$((gone + 1)); [ "$gone" -lt 20 ] && continue
            why="the VM's share stopped answering"; break
        fi
        gone=0
        back repo/target/proofs "$fo" run.status cases.log times.tsv now last_case quick.re '*.row' '*.secs' '*.lane' /S /LEV:2
        if [ -z "$started" ]; then
            if [ -f "$fs/run/started" ]; then started=1
            elif [ $(( $(date +%s) - t1 )) -gt 300 ]; then why="the VM's scheduled task did not start the run in 5 min (is its user logged in?)"; break; fi
        fi
        s=$(cat "$fo"/*/cases.log "$fo/run.status" 2> /dev/null | wc -l)
        if [ "$s" != "$sig" ]; then sig=$s; last=$(date +%s)
        elif [ $(( $(date +%s) - last )) -gt $(( ${FARM_STALL:-60} * 60 )) ]; then why="no new verdict on the VM for ${FARM_STALL:-60} min"; break; fi
    done
    t2=$(date +%s)
    [ -z "$why" ] || { farm_why "$why"; farm_ssh schtasks /end /tn rustyac_farm > /dev/null 2>&1; }
    back run "$fo" out.txt task.txt
    # every word the VM finished, as if it had run here (a word with no row there is the PC's, or one
    # the VM never finished: run_all.sh runs it here)
    for a in $qwords; do
        r=$fp/rows/$a.row
        [ -f "$r" ] || continue
        grep -q 'did not run' "$r" && { echo "$a it did not run there ($(tail -n 1 "$fp/$a.log" 2> /dev/null | tr -d '\r'))" >> "$fo/fallback"; continue; }
        back "repo/target/proofs/$a" "$out/$a" /MIR /XD own root rec frames drives &&
            back repo/target/proofs "$out" "$a.log" && back repo/target/proofs/rows "$out/rows" "$a.row" "$a.secs" "$a.bad" ||
            { rm -f "$out/rows/$a.row"; echo "$a its results did not come back from the VM" >> "$fo/fallback"; continue; }
        echo VM > "$out/rows/$a.where"
    done
    # the kept results the VM made in this run (its own keys: SSE2), so the PC holds every kept file;
    # only those (a whole-tree copy would bring back what archive_kept.sh moved off the PC), and each
    # folder's "done" last (a copy cut short leaves no folder cache_get would take)
    # shortcut: a run stopped on the VM lists nothing, its new kept files stay there; copy by hand if needed
    back run "$fo" new_kept.txt
    for p in -v ''; do
        tr -d '\r' < "$fo/new_kept.txt" 2> /dev/null | grep $p '/done$' | awk '{ n = split($0, s, "/")
            d = substr($0, 1, length($0) - length(s[n]) - 1); fs[d] = fs[d] " " s[n] } END { for (d in fs) print d fs[d] }' |
            while read -r d f; do
                # shellcheck disable=SC2086
                back "repo/$d" "$repo/$d" $f || echo "a kept file did not come back: $d" >> "$fo/farm.txt"
            done
    done
    n=$(grep -c . "$fo/new_kept.txt" 2> /dev/null); echo "kept files back: ${n:-0}" >> "$fo/farm.txt"
    echo "VM run: $((t2 - t1)) s; results back: $(( $(date +%s) - t2 )) s" >> "$fo/farm.txt"
}
