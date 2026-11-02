# SPDX-License-Identifier: GPL-3.0-or-later
#
# Shared by the area scripts of tools/proofs (sourced, never run): where things are, how the
# tools are built, and the tally every area prints last:
#   PROOF <area> cases=<n> identical=<n> different=<n> skipped=<n>
# An area script sets $area before sourcing this file.

repo=$(cd "$(dirname "$0")/../.." && pwd)
# in Windows form (C:/...) for the programs no MSYS path conversion reaches
repo_w=$(cd "$repo" && { pwd -W 2>/dev/null || pwd; })
ac=${AC_ROOT:-"C:/Program Files (x86)/Steam/steamapps/common/assettocorsa"}
gold=$repo/re/gold
# what a run writes: build output, deleted and remade by every run of the area
out=${PROOF_OUT:-$repo/target/proofs}/$area
rm -rf "$out"
mkdir -p "$out"
log=$out/cases.log
: > "$log"
# times.tsv: the seconds each case took (from the end of the one before), for the table of README.md
times=$out/times.tsv
: > "$times"
date +%s > "$out/last_case"  # a file: verdicts in a pipe run in a subshell
cd "$repo"
export CARGO_BUILD_JOBS=${CARGO_BUILD_JOBS:-8}

# Task 46c: no proof run reads what an earlier run wrote, nor the player's own files. rustyac.exe
# and race_proof refuse to start without folders of their own while this is set (a case that would
# use the default cfg folder fails), and every run gets fresh ones from `own` / `rustyac`.
export RUSTYAC_PROOF_RUN=1
# the kept starting copy of a player's Documents\Assetto Corsa: the setup a drive names, the
# replays of replays.sh, two saved laps (re/gold/README.md)
docs_gold=$gold/docs
export RENDER_ORACLE_DOCUMENTS=$(cygpath -w "$docs_gold")
# own: a fresh folder for one run under $out/own (its cfg/ and an empty docs/ for its Documents),
# printed in Windows form
own() {
    mkdir -p "$out/own"
    _o=$(mktemp -d "$out/own/run.XXXXXX") && mkdir -p "$_o/cfg" "$_o/docs" && cygpath -m "$_o"
}
# rustyac <exe> <args...>: that rustyac.exe (this one or an older one) on a fresh own folder
rustyac() {
    _o=$(own) && _x=$1 && shift && "$_x" --cfg-dir "$_o/cfg" --documents "$_o/docs" "$@"
}
# drive <file.ryin>: the drive as the proofs replay it (a path from the repo): one naming a file under
# the player's Documents (conditions_spa_setup: its setup) as a copy naming the kept one instead
drive() {
    grep -aqF '\Documents\Assetto Corsa\' "$1" || { echo "$1"; return; }
    mkdir -p "$out/drives"
    c=$out/drives/$(basename "$1")
    [ -f "$c" ] || DOCS=$(cygpath -w "$docs_gold") perl -0777 -pe 's/[A-Za-z]:\\Users\\[^\\\n]+\\Documents\\Assetto Corsa/$ENV{DOCS}/g' "$1" > "$c"
    echo "${c#"$repo"/}"
}
# own_print: what a run without --cfg-dir would read and write: the default own folder beside
# target/release/rustyac.exe (summary() fails the area when it changed during the run)
own_print() {
    (cd "$repo/target/release" 2>/dev/null && find cfg setups replay GhostCar ctelemetry out screenshots rustyac_cache rustyac_net.ini rustyac_controls.ini -printf '%p %s %T@\n' 2>/dev/null | sort) | sha1sum
}
own_before=$(own_print)

CO=tools/car_oracle/target/release/car_oracle.exe
CC=tools/chassis_compare/target/release/chassis_compare.exe
RO=tools/render_oracle/target/release/render_oracle.exe
AO=tools/audio_oracle/target/release/audio_oracle.exe
# PROOF_LIST_KEYS=<file> (archive_kept.sh): each kept folder the run would read goes into <file>;
# no oracle runs, nothing is built, recorded or unpacked
[ -z "$PROOF_LIST_KEYS" ] || { CO=true; CC=true; PROOF_BUILT="car_oracle chassis_compare rustyac"; }

# PROOF_QUICK=1 (run_all.sh --quick): only the cases whose key matches a whole line (a regex) of
# tools/proofs/quick/<area>.txt run; each area says what its keys look like in that file
quick_re=$out/quick.re
grep -vE '^\s*(#|$)' "$repo/tools/proofs/quick/$area.txt" > "$quick_re" 2>/dev/null
# keep <key>: whether a case runs (always, unless this is a quick run and the key is not listed)
keep() {
    [ -z "$PROOF_QUICK" ] || printf '%s
' "$1" | grep -qxE -f "$quick_re"
}

# The game's recordings, kept (xz) in re/gold/fresh/rec/<name>/<key>/ (made once, byte for byte
# the same every time the game records them again). PROOF_RERECORD=1 (--full, --rerecord) records
# again and holds the new recording against the kept one; nothing kept is ever replaced or deleted.
# Below PROOF_KEEP_MIN_GB (18) of free disk nothing new is kept (the run goes on, recording as before).
cache=$gold/fresh/rec
# salt_of <tool>: the oracle's own source (not its lock file, which every release changes for
# rustyAC's version) and the game's files (install_key)
# shortcut: the port crates an oracle links are not in the key (they change every task, the game's
# side only reads a few of their files); a stale result shows as a difference, and `--rerecord`
# holds every kept result against a new one.
salt_of() {
    { git ls-tree -r HEAD -- "tools/$1" | grep -v 'Cargo\.lock$'
      git diff HEAD -- "tools/$1" ":!tools/$1/Cargo.lock"; install_key; } 2>/dev/null | sha1sum | cut -c1-16
}
# install_key: the game's files every case may read: acs.exe's bytes and the names, sizes and
# times of system/, content/cars and content/tracks (once a run: run_all.sh exports it)
install_key() {
    if [ -z "$PROOF_INSTALL_KEY" ]; then
        PROOF_INSTALL_KEY=$({ sha1sum "$ac/acs.exe" | cut -c1-40
            (cd "$ac" && find system content/cars content/tracks -type f -printf '%p %s %T@\n' | sort); } | sha1sum | cut -c1-16)
        export PROOF_INSTALL_KEY
    fi
    echo "$PROOF_INSTALL_KEY"
}
cache_salt=$(salt_of car_oracle)
# cache_key <words...>: the key of one recording (its arguments, the content of any file named in them)
cache_key() {
    { echo "${key_salt:-$cache_salt}"; printf '%s
' "$@"; for a in "$@"; do [ -f "$a" ] && cat "$a"; done; } | sha1sum | cut -c1-16
}
# cache_get <dir> <into>: the kept recording into <into>; fails when there is none or when re-recording
cache_get() {
    [ -z "$PROOF_LIST_KEYS" ] || { echo "$1" >> "$PROOF_LIST_KEYS"; mkdir -p "$2"; return 0; }
    [ -z "$PROOF_RERECORD" ] && [ -f "$1/done" ] || return 1
    mkdir -p "$2"
    for f in "$1"/*.xz "$1"/*.gz; do
        [ -f "$f" ] && { unpack "$f" > "$2/$(basename "${f%.*}")" || return 1; }
    done
    return 0
}
# unpack <kept file>: its bytes (the first recordings of Task 38f were kept gzipped)
unpack() {
    case "$1" in *.gz) gunzip -c "$1" ;; *) xz -dc "$1" ;; esac
}
# cache_put <dir> <from> <what>: keeps what was just recorded; when something is kept already the
# two must be the same bytes (a DIFFERENT case when not), apart from the game's heap address in the
# header (tools/proofs/same_rec.py) and the port's own *.result.txt (it has timings)
cache_put() {
    if [ -f "$1/done" ]; then
        for f in "$2"/*; do
            [ -f "$f" ] || continue
            case "$f" in *.result.txt) continue ;; esac
            k=$(ls "$1/$(basename "$f")".* 2>/dev/null | head -n 1)
            if [ -z "$k" ] || ! { unpack "$k" > "$out/kept.tmp" && python "$repo/tools/proofs/same_rec.py" "$out/kept.tmp" "$f" > "$out/same.txt"; }; then
                verdict "re-recorded $3: $(basename "$f")" DIFFERENT "not the bytes kept in ${1#"$repo"/}: $(cat "$out/same.txt" 2>/dev/null)"
                rm -f "$out/kept.tmp"
                return
            fi
        done
        rm -f "$out/kept.tmp"
        echo "re-recorded $3: the same bytes as ${1#"$repo"/}" >> "$out/rerecord.txt"
        return
    fi
    free=$(df -Pk "$repo" | awk 'NR == 2 { print int($4 / 1048576) }')
    if [ "$free" -lt "${PROOF_KEEP_MIN_GB:-18}" ]; then
        echo "not kept (${free} GB free): $3" >> "$out/rerecord.txt"
        return
    fi
    mkdir -p "$1"
    for f in "$2"/*; do [ -f "$f" ] && xz -1 -T4 -c "$f" > "$1/$(basename "$f").xz"; done
    echo "$3" > "$1/what.txt"
    : > "$1/done"
}

# tool <name>: builds tools/<name> (its own workspace and lock file); run_all.sh builds the shared
# ones once and names them in PROOF_BUILT
tool() {
    case " $PROOF_BUILT " in *" $1 "*) return 0 ;; esac
    cargo build --release --locked --quiet --manifest-path "tools/$1/Cargo.toml" || { echo "tools/$1 does not build" >&2; exit 2; }
}
# build <name> <command...>: one build of rustyAC's own programs, skipped when run_all.sh made it
# already (PROOF_BUILT): an area never rebuilds a program another area is running
build() {
    b=$1; shift
    case " $PROOF_BUILT " in *" $b "*) return 0 ;; esac
    "$@" || { echo "$b does not build" >&2; exit 2; }
}

# verdict <case> <IDENTICAL|DIFFERENT> [detail]: one line in the tally
verdict() {
    echo "$2 $1${3:+ | $3}" | tee -a "$log"
    now=$(date +%s); printf '%s\t%s\n' $((now - $(cat "$out/last_case"))) "$1" >> "$times"; echo "$now" > "$out/last_case"
}
# skip <case> <reason>: a case that did not run, listed with its reason and counted in the tally
skip() {
    echo "SKIPPED $1 | $2" | tee -a "$log"
    date +%s > "$out/last_case"
}

# need <path> <what>: a recording the area cannot run without
need() {
    [ -e "$1" ] || { echo "missing: $1 ($2); see tools/proofs/README.md" >&2; exit 2; }
}

summary() {
    [ "$(own_print)" = "$own_before" ] || verdict "the default own folder (target/release/cfg and the rest)" DIFFERENT "a run wrote there: proof runs must have --cfg-dir and --documents"
    identical=$(grep -c '^IDENTICAL ' "$log")
    different=$(grep -c '^DIFFERENT ' "$log")
    skipped=$(grep -c '^SKIPPED ' "$log")
    echo "PROOF $area cases=$((identical + different)) identical=$identical different=$different skipped=$skipped"
    [ "$different" = 0 ] && [ $((identical + different)) -gt 0 ]
}

# multi_list <file> <root> <rec> [cache]: one `car_oracle multi` a line ("scenario track cars steps [more]",
# "-" for the scenario's own cars or steps), the port at 0, 1 and 8 threads; IDENTICAL when all three
# say so and nothing says DIFFERENT. The recording is deleted after each line. With [cache], a line
# without --replay / --ghost / --telemetry (those are checked while the game records) is recorded
# once into the cache and checked from there with `car_oracle multi-check`. Quick keys:
# "multi <scenario> <track> <cars> <more>".
multi_list() {
    m_root=$2; m_rec=$3; m_cache=$4
    grep -vE '^\s*(#|$)' "$1" | while read -r s t cars steps more; do
        name="${m_label:-multi} $s $t $cars $more"
        keep "$name" || continue
        set -- --scenario "$s" --track "$t"
        [ "$cars" = - ] || set -- "$@" --cars "$cars"
        [ "$steps" = - ] || set -- "$@" --steps "$steps"
        c=
        if [ -n "$m_cache" ] && ! echo "$more" | grep -qE -- '--(replay|ghost|telemetry|ai-line|ai-record)'; then
            # shellcheck disable=SC2086
            c=$m_cache/multi_$s/$(cache_key multi "$@" $more)
        fi
        if [ -n "$c" ] && cache_get "$c" "$m_rec/m"; then
            "$CO" multi-check "$(ls "$m_rec"/m/*.mcrec | head -n 1)" --threads 0,1,8 > "$out/multi.txt" 2>&1 < /dev/null
        else
            # shellcheck disable=SC2086
            "$CO" multi "$@" $more --threads 0,1,8 --out "$m_rec/m" --root "$m_root" > "$out/multi.txt" 2>&1 < /dev/null                 && [ -n "$c" ] && cache_put "$c" "$m_rec/m" "$name"
        fi
        n=$(grep -cE "^$s threads [0-9]+: IDENTICAL" "$out/multi.txt")
        # --replay / --ghost / --telemetry: their own verdict line must agree too
        feature=$(echo "$more" | grep -oE -- '--(replay|ghost|telemetry|ai-record)' | head -n 1 | sed 's/^--//; s/-/_/')
        if [ -n "$feature" ] && ! grep -qE "^$feature \| $s \| all \| (IDENTICAL|no lap became a ghost|no lap was finished)" "$out/multi.txt"; then n=0; fi
        if [ "$n" = 3 ] && ! grep -q 'DIFFERENT' "$out/multi.txt"; then
            verdict "$name" IDENTICAL "$(grep -m1 -oE '[0-9]+ steps, [0-9]+ values' "$out/multi.txt")"
        else
            verdict "$name" DIFFERENT "$(grep -m1 -E 'DIFFERENT|rror|panic' "$out/multi.txt")"
        fi
        cat "$out/multi.txt" >> "$out/multi_all.txt"
        rm -rf "$m_rec/m"
    done
}
