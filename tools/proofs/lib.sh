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

# tool <name>: builds tools/<name> (its own workspace and lock file)
tool() {
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

# multi_list <file> <root> <rec>: one `car_oracle multi` a line ("scenario track cars steps [more]",
# "-" for the scenario's own cars or steps), the port at 0, 1 and 8 threads; IDENTICAL when all three
# say so and nothing says DIFFERENT. The recording is deleted after each line.
multi_list() {
    m_root=$2; m_rec=$3
    grep -vE '^\s*(#|$)' "$1" | while read -r s t cars steps more; do
        set -- --scenario "$s" --track "$t"
        [ "$cars" = - ] || set -- "$@" --cars "$cars"
        [ "$steps" = - ] || set -- "$@" --steps "$steps"
        # shellcheck disable=SC2086
        "$CO" multi "$@" $more --threads 0,1,8 --out "$m_rec/m" --root "$m_root" > "$out/multi.txt" 2>&1 < /dev/null
        name="multi $s $t $cars $more"
        n=$(grep -cE "^$s threads [0-9]+: IDENTICAL" "$out/multi.txt")
        # --replay / --ghost / --telemetry: their own verdict line must agree too. With --telemetry the
        # game's recorder hung on the lap events changes the game's referee values (v0.38.1 the same),
        # so only the telemetry's verdict counts there (as in Task 28c)
        feature=$(echo "$more" | grep -oE -- '--(replay|ghost|telemetry)' | head -n 1 | tr -d -)
        if [ "$feature" = telemetry ]; then n=3; sed -i '/threads [0-9]*: DIFFERENT/d' "$out/multi.txt"; fi
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
