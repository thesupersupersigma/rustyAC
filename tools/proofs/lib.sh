# SPDX-License-Identifier: GPL-3.0-or-later
#
# Shared by the area scripts of tools/proofs (sourced, never run): where things are, how the
# tools are built, and the tally every area prints last:
#   PROOF <area> cases=<n> identical=<n> different=<n>
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
cd "$repo"
export CARGO_BUILD_JOBS=${CARGO_BUILD_JOBS:-8}

CO=tools/car_oracle/target/release/car_oracle.exe
CC=tools/chassis_compare/target/release/chassis_compare.exe
RO=tools/render_oracle/target/release/render_oracle.exe
AO=tools/audio_oracle/target/release/audio_oracle.exe

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
# the oracle's own source is part of every key: a changed oracle records anew
cache_salt=$({ git rev-parse HEAD:tools/car_oracle; git diff HEAD -- tools/car_oracle; } 2>/dev/null | sha1sum | cut -c1-16)
# cache_key <words...>: the key of one recording (its arguments, the content of any file named in them)
cache_key() {
    { echo "$cache_salt"; printf '%s
' "$@"; for a in "$@"; do [ -f "$a" ] && cat "$a"; done; } | sha1sum | cut -c1-16
}
# cache_get <dir> <into>: the kept recording into <into>; fails when there is none or when re-recording
cache_get() {
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

# verdict <case> <IDENTICAL|DIFFERENT> [detail]: one line in the tally
verdict() {
    echo "$2 $1${3:+ | $3}" | tee -a "$log"
}

# need <path> <what>: a recording the area cannot run without
need() {
    [ -e "$1" ] || { echo "missing: $1 ($2); see tools/proofs/README.md" >&2; exit 2; }
}

summary() {
    identical=$(grep -c '^IDENTICAL ' "$log")
    different=$(grep -c '^DIFFERENT ' "$log")
    echo "PROOF $area cases=$((identical + different)) identical=$identical different=$different"
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
        name="multi $s $t $cars $more"
        keep "$name" || continue
        set -- --scenario "$s" --track "$t"
        [ "$cars" = - ] || set -- "$@" --cars "$cars"
        [ "$steps" = - ] || set -- "$@" --steps "$steps"
        c=
        if [ -n "$m_cache" ] && ! echo "$more" | grep -qE -- '--(replay|ghost|telemetry)'; then
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
        feature=$(echo "$more" | grep -oE -- '--(replay|ghost|telemetry)' | head -n 1 | tr -d -)
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
