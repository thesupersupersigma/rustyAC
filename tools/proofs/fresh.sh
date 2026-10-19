#!/bin/sh
# SPDX-License-Identifier: GPL-3.0-or-later
#
# The physics proofs whose recordings lived only in re/scratch, recorded again from the game's own
# code (acs.exe mapped into the oracle's process, never run) and compared on the spot. What a
# recording is written to is deleted again once compared: they are big and remade in a minute.
#   FRESH_PARTS="car conditions awd hybrid sus collide rays survey crash micro ini multi" (default: all of them)
area=fresh
. "$(dirname "$0")/lib.sh"
tool car_oracle
tool chassis_compare
parts=${FRESH_PARTS:-"car conditions awd hybrid sus collide rays survey crash micro ini multi"}
root=$out/root     # the scratch game folder the oracles build for themselves
rec=$out/rec

part() { case " $parts " in *" $1 "*) echo "== $1" >&2; return 0 ;; esac; return 1; }

# record <label> <car_oracle arguments>: one more recording into $rec/<label>
record() {
    label=$1; shift
    "$CO" "$@" --out "$rec/$label" --root "$root" >> "$out/record.txt" 2>&1 || echo "record failed: $label $*" >> "$out/record.txt"
}

# compare_dirs: chassis_compare over every folder in $rec, one case per recording, then away with them
compare_dirs() {
    for dir in "$rec"/*/; do
        [ -d "$dir" ] || continue
        name=$(basename "$dir")
        "$CC" run --dir "$dir" > "$out/cmp_$name.txt" 2>&1
        awk -v d="$name" '
            /^[a-z_0-9]+: [0-9]+ steps/ { s = $1; sub(":", "", s) }
            /bit-exact: 100 %/ && s != "" { print "IDENTICAL " d "/" s; s = "" }
            (/bit-exact: [0-9.]+ %/ && !/100 %/ || /first difference/) && s != "" { print "DIFFERENT " d "/" s; s = "" }
        ' "$out/cmp_$name.txt" | tee -a "$log"
    done
    rm -rf "$rec"
}

if part car; then  # Task 9-13: every flat-road scenario of the F2004
    record f2004 all
    compare_dirs
fi

if part conditions; then  # Task 15: weather, grip, wind and setups on Spa
    for s in spa_cold_green_wind spa_hot_optimum spa_user spa_wind_any spa_green_laps; do
        record conditions run --track spa --scenario "$s"
    done
    # the user's own setup, read where the game keeps it; god_tc4.ini is it with TRACTION_CONTROL 4
    god="$USERPROFILE/Documents/Assetto Corsa/setups/ks_ferrari_f2004/spa/god.ini"
    [ -f "$god" ] && record conditions run --track spa --scenario spa_setup --setup "$god"
    [ -f "$gold/fresh/god_tc4.ini" ] && record conditions run --track spa --scenario spa_setup_launch --setup "$gold/fresh/god_tc4.ini"
    compare_dirs
fi

if part awd; then  # Task 16: four-wheel drive
    for car in ks_lamborghini_aventador_sv ks_lamborghini_huracan_performante ks_maserati_levante ks_nissan_gtr ks_porsche_cayenne ks_porsche_macan; do
        record "awd2_$car" all --car "$car" --only pt_autoshift,wc_stops,wc_spirited
    done
    for car in ks_lamborghini_sesto_elemento ks_audi_r8_plus; do
        for s in spa_launch spa_kerbs spa_lap; do record "awd_spa_$car" run --track spa --scenario "$s" --car "$car"; done
    done
    compare_dirs
fi

if part hybrid; then  # Task 16: KERS and ERS, the installed cars and the made-up test cars
    for car in ks_ferrari_f138 ferrari_laferrari ks_ferrari_sf15t ks_ferrari_sf70h ks_mclaren_p1 vrc_formula_alpha_2026; do
        record "hy_$car" all --car "$car" --only hy_deploy,hy_modes,wc_stops
    done
    for car in ks_ferrari_f138 ks_ferrari_sf15t; do
        for s in spa_hybrid_timing spa_hybrid_lap spa_launch spa_drs_timing; do record "hy_spa_$car" run --track spa --scenario "$s" --car "$car"; done
    done
    compare_dirs
    for car in f138_kers_wheels laferrari_kers_button sesto_awd_kers r8_awd2_kers f2004_fwd_kers sf15t_ers_limits vrc_formula_lithium_2023 lithium_ers_front vrc_formula_alpha_2025; do
        record "hyt_$car" all --car "$car" --only hy_deploy,hy_modes
    done
    compare_dirs
fi

if part sus; then  # Task 17: suspensions, the eight cars of docs/port/suspensions.md
    for car in bmw_m3_e30 ks_toyota_gt86 ks_audi_sport_quattro ks_ferrari_250_gto ks_ferrari_812_superfast ks_porsche_panamera gt3_multilink formula_k; do
        for s in pt_autoshift wc_stops wc_spirited; do record "t17_$car" run --car "$car" --scenario "$s"; done
        for s in spa_launch spa_kerbs spa_eau_rouge; do record "t17_spa_$car" run --track spa --scenario "$s" --car "$car"; done
        for s in spa_wall_gravel spa_kerb_strike; do record "t17_col_$car" run --track spa --collide --scenario "$s" --car "$car"; done
        compare_dirs
    done
fi

if part collide; then  # Task 13: contacts
    "$CO" collide --track spa --root "$root" > "$out/collide.txt" 2>&1
    grep -q '^No difference' "$out/collide.txt" && verdict "collide spa" IDENTICAL || verdict "collide spa" DIFFERENT
    "$CO" collide-worlds --steps 3000 --root "$root" > "$out/collide_worlds.txt" 2>&1
    # one table row per world; the last column is the first difference ("none")
    grep -E '^\| `[a-z_]+` \|' "$out/collide_worlds.txt" | while IFS='|' read -r _ w _ _ _ _ _ first _; do
        w=$(echo "$w" | tr -d ' `')
        case "$first" in *none*) verdict "collide-worlds $w" IDENTICAL ;; *) verdict "collide-worlds $w" DIFFERENT "$first" ;; esac
    done
fi

if part rays; then  # Task 14 and 18: the road under the wheels on every track and layout that loads
    cargo build --release --locked --quiet -p rustyac-physics --bin track_survey
    target/release/track_survey.exe --list 2>/dev/null | tr -d '\r' | awk 'NF' > "$out/layouts.txt" || true
    [ -s "$out/layouts.txt" ] || printf 'magione\nspa\nks_laguna_seca\n' > "$out/layouts.txt"
    while read -r t l _; do
        "$CO" rays --track "$t" ${l:+--layout "$l"} --count 200000 --root "$root" > "$out/rays.txt" 2>&1
        if grep -q '^No difference' "$out/rays.txt"; then verdict "rays $t $l" IDENTICAL
        elif grep -qiE 'refused|not load|cannot' "$out/rays.txt"; then echo "rays $t $l: refused (not a case)" >> "$out/refused.txt"
        else verdict "rays $t $l" DIFFERENT "$(tail -n 1 "$out/rays.txt")"; fi
    done < "$out/layouts.txt"
fi

if part survey; then  # Task 18: a car on every track of survey_list.txt, then rustyac.exe replays the game's drive
    cargo build --release --locked --quiet -p rustyac-game --bin rustyac || exit 2
    grep -vE '^\s*(#|$)' tools/proofs/survey_list.txt | while read -r car t l scenarios; do
        [ "$l" = - ] && l=
        label="t18_${t}_${l}_${car}"
        for s in $scenarios; do record "$label" run --track "$t" ${l:+--layout "$l"} --scenario "$s" --car "$car" --collide; done
        # the exe replays the game's inputs: the same states as the game's recording
        "$CC" game-replay --dir "$rec/$label" --exe target/release/rustyac.exe > "$out/replay_$label.txt" 2>&1
        grep -E '^\| .(trk|spa)_[a-z_]*. \|' "$out/replay_$label.txt" | while IFS='|' read -r _ sc rest; do
            sc=$(echo "$sc" | tr -d ' `')
            case "$rest" in *"100 %"*) verdict "game-replay $label/$sc" IDENTICAL ;; *) verdict "game-replay $label/$sc" DIFFERENT "$rest" ;; esac
        done
        compare_dirs
    done
fi

if part crash; then  # Task 22b: into the barrier at Spa with damage, five suspensions; then rustyac.exe replays it
    cargo build --release --locked --quiet -p rustyac-game --bin rustyac || exit 2
    for car in ks_ferrari_f2004 bmw_m3_e30 ks_ferrari_812_superfast gt3_multilink ks_ferrari_250_gto; do
        label="t22b_$car"
        record "$label" run --track spa --collide --scenario spa_crash --car "$car"
        "$CC" game-replay spa_crash --dir "$rec/$label" --exe target/release/rustyac.exe > "$out/replay_$label.txt" 2>&1
        grep -qE '^\| .spa_crash. \|.*100 %' "$out/replay_$label.txt" && verdict "game-replay $label/spa_crash" IDENTICAL \
            || verdict "game-replay $label/spa_crash" DIFFERENT "$(grep -m1 -E 'spa_crash|rror' "$out/replay_$label.txt")"
        compare_dirs
    done
fi

if part micro; then  # Task 28: the game's referee on made-up drives, 25 seeds of each of 8 session lists
    for kind in 0 1 2 3 4 5 6 7; do
        for seed in $(seq 1 25); do
            cars=$(( 2 + (seed * 7 + kind) % 23 ))
            r=$("$CO" race-micro --seed "$seed" --kind "$kind" --count "$cars" --root "$root" 2>&1 | grep '^referee:')
            case "$r" in *IDENTICAL*) verdict "race-micro $kind/$seed/$cars" IDENTICAL ;; *) verdict "race-micro $kind/$seed/$cars" DIFFERENT "$r" ;; esac
        done
    done
fi

if part ini; then  # Task 24b and 28: race.ini read by the game and by the port (the saved runs' own files)
    find oracle/t24b oracle/t28 oracle/t28b -name '*.ini' ! -name 'laps.ini' 2>/dev/null | sort > "$out/inis.txt"
    while read -r f; do
        "$CO" race-ini "$f" --root "$root" > "$out/ini.txt" 2>&1
        if grep -q 'IDENTICAL' "$out/ini.txt" && ! grep -q 'DIFFERENT' "$out/ini.txt"; then verdict "race-ini $f" IDENTICAL
        elif grep -qE 'not a race.ini|no \[RACE\]' "$out/ini.txt"; then :
        else verdict "race-ini $f" DIFFERENT "$(grep -m1 -E 'DIFFERENT|rror' "$out/ini.txt")"; fi
    done < "$out/inis.txt"
fi

if part multi; then  # Task 28 and 28b: several cars, recorded once, the port at 0, 1 and 8 threads
    multi_list tools/proofs/multi_fresh.txt "$root" "$rec"
fi

rm -rf "$root"
summary
