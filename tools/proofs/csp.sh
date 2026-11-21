#!/bin/sh
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Custom Shaders Patch mode (Task C3, docs/csp/port_c3.md), against the game's own code with CSP 0.3.2
# (build 4305) live in tools/csp_oracle:
#   words <car>    the game's car built with CSP and without (a copy of its data with VERSION=N), word
#                  by word: only CSP's own words differ (csp_words.py), the tyre pressures as rustyAC's
#                  CSP mode builds them (acd_check --csp --pressures), the first lap armed as it arms it
#   car <car>      the same car, the game's Car::Car (car_oracle) against the port (chassis_compare)
#   refused off|csp  every installed car through acd_check: which are refused, with which reason
#   layouts        track_survey without and with --csp: exactly the six *_fa_2026 layouts load with it
#   wav <track> <layout>  their WAV_PITCH=extended-N read by the port and by CSP's getFloat
#   air <track>|still  CSP's air density (Task C4a, docs/csp/port_c4.md): the game's car driven 60 s with
#                  CSP live, CSP's own density law, near-ground temperature, ground plane and altitude
#                  table called at every step against crates/rustyac-physics/src/csp/air.rs, bit for bit
#   ode <car>      CSP's double ODE functions and solver kernels against rustyac-ode's f64 ones (Tasks C5c, C5d),
#                  inputs from a Spa drive
#   island <car>   CSP's dxStepIsland against rustyac-ode's csp_step on that car's recorded Spa islands (Task C5d)
#   world <car> <n>  CSP's world step (auto-disable, islands, island steps) against rustyac-ode's csp_world on
#                  n cars' recorded Spa worlds, CSP workers 0 and 7 (Task C5e)
#   collision <car> <n>  CSP's collision step against rustyac-ode's csp_collision on n cars' Spa drive, then
#                  the world stepped on the port's own contacts (Task C5f)
#   core <car> <n>  CSP's cphys:: wrappers built by rustyac-ode's csp_core from the car's own calls, nothing
#                  copied: set-up, every answer and every world step; n=2 is two F2004 that touch (Task C5g)
#   row <car> lap|wall  the whole car with CSP live against rustyac --csp, step by step on one tape (Task C5i-b)
#   smoke          rustyAC's own CSP mode (Task C5i-a): both proof cars driven at Spa, the drive replayed twice
# Quick keys (tools/proofs/quick/csp.txt): the case names above.
area=csp
. "$(dirname "$0")/lib.sh"
tool car_oracle
tool chassis_compare
tool csp_oracle
build acd_check cargo build --release --locked --quiet -p rustyac-physics --bin acd_check
build track_survey cargo build --release --locked --quiet -p rustyac-physics --bin track_survey
CSP=tools/csp_oracle/target/release/csp_oracle.exe
need re/csp/fakeac/dwrite.dll "the AC folder with CSP, a junction made by Task C1 (re/csp/fakeac)"

# the cars CSP makes loadable (docs/csp/content_needs.md, acd_check), then plain cars (rss_formula_rss_3_v6 is plain since its reinstall of 2026-11-17)
extended="bdc_streetspec_350z_v4 bdc_streetspec_ae86_v4 bdc_streetspec_altezza_v4 bdc_streetspec_aristo_v4
bdc_streetspec_e36_v4 bdc_streetspec_miata_v4 bdc_streetspec_r32_v4 bdc_streetspec_rx7_v4 bdc_streetspec_s13_v4
bdc_streetspec_s14_v4 rss_formula_2010 rss_formula_hybrid_2026 rss_formula_rss_3_v6_2026 vrc_formula_lithium_2023_csp"
plain="ks_ferrari_f2004 bmw_m3_e30 bmw_z4_gt3 rss_formula_rss_3_v6"
still_refused="urd_darche_992_23 vrc_formula_alpha_2025_csp vrc_formula_alpha_2026_csp vrc_formula_beta_2024_csp"
layouts="ks_barcelona/layout_moto_fa_2026 ks_red_bull_ring/layout_gp_fa_2026 ks_silverstone/gp_fa_2026
montreal/montreal_fa_2026 monza/monza_fa_2026 spa/spa_fa_2026"

# the oracle's own game cfg: CSP's RANDOMIZE_WHEEL_ANGLES=0 (rustyAC's CSP mode), a session that starts
# on the hot-lap line (CSP arms the first lap there), the keyboard (no controls message box)
cfg=$out/cfg
mkdir -p "$cfg/extension"
printf '[CONTROL]\r\nRELEASE_DPAD=1\r\n\r\n[PHYSICS_EXPERIMENTS_DONE]\r\nRANDOMIZE_WHEEL_ANGLES=0\r\n' > "$cfg/extension/general.ini"
printf '[HEADER]\r\nINPUT_METHOD=KEYBOARD\r\n' > "$cfg/controls.ini"
sed -n '/^const RACE_INI: &str = "/,/^";/p' tools/csp_oracle/src/main.rs | sed '1s/.*"//; $d; s/^SPAWN_SET=PIT$/SPAWN_SET=HOTLAP_START/' | sed 's/$/\r/' > "$cfg/race.ini"
grep -q 'SPAWN_SET=HOTLAP_START' "$cfg/race.ini" || { echo "csp_oracle's RACE_INI has no SPAWN_SET=PIT to change" >&2; exit 2; }

# oracle <run> <args...>: one csp_oracle run into $out/runs/<run> (a new folder each time)
oracle() {
    r=$out/runs/$1; shift
    rm -rf "$r"; mkdir -p "$out/runs"
    timeout 300 "$CSP" re/csp/fakeac "$r" "$cfg" "$@" > "$r.txt" 2>&1
}
# stale <car>: the install's data.acd no longer matches its copy in cardata/ (the car was reinstalled
# or updated): its cases say so instead of DIFFERENT
stale() { target/release/acd_check.exe --steps 1 --only "$1" 2>&1 | grep -q "^FAILED $1: files differ"; }

for car in $extended $plain; do
    keep "words $car" || continue
    [ -f "cardata/$car/car.ini" ] || { skip "words $car" "no cardata/$car (tools/acd_extract.py)"; continue; }
    stale "$car" && { skip "words $car" "content changed: $car, refresh cardata/"; continue; }
    # the Vanilla build reads the same data with car.ini's VERSION=extended-N as plain N
    mkdir -p "$out/plain"; rm -rf "$out/plain/$car"; cp -r "cardata/$car" "$out/plain/$car"
    sed -i 's/^VERSION=extended\(V2\)\{0,1\}-/VERSION=/' "$out/plain/$car/car.ini"
    if oracle "${car}_van" --car "$car" --car-data "$out/plain/$car" --vanilla && oracle "${car}_csp" --car "$car"; then
        port=$(target/release/acd_check.exe --steps 1 --only "$car" --csp --pressures | tr -d '' | grep -m1 '^PRESSURES ')
        line=$(python tools/proofs/csp_words.py "$out/runs/${car}_van/car_root/car_words.txt" "$out/runs/${car}_csp/car_root/car_words.txt" "$port")
        verdict "words $car" "${line%% |*}" "${line#* | }"
    else
        verdict "words $car" DIFFERENT "csp_oracle failed: $(why "$r.txt" 'error|panic|exit')"
    fi
done

for car in $extended $plain; do
    keep "car $car" || continue
    [ -f "cardata/$car/car.ini" ] || { skip "car $car" "no cardata/$car"; continue; }
    stale "$car" && { skip "car $car" "content changed: $car, refresh cardata/"; continue; }
    rm -rf "$out/rec"
    if "$CO" run --car "$car" --scenario wc_stops --out "$out/rec" --root "$out/root" > "$out/rec_$car.txt" 2>&1; then
        "$CC" run --dir "$out/rec" > "$out/cmp_$car.txt" 2>&1
        if grep -q 'bit-exact: 100 %' "$out/cmp_$car.txt"; then
            verdict "car $car" IDENTICAL "$(grep -m1 -o 'bit-exact: 100 % ([0-9/]*)' "$out/cmp_$car.txt")"
        else
            verdict "car $car" DIFFERENT "$(why "$out/cmp_$car.txt" 'first difference|bit-exact|error')"
        fi
    else
        verdict "car $car" DIFFERENT "car_oracle failed: $(why "$out/rec_$car.txt" 'error|panic|refus')"
    fi
done

# refused off|csp: acd_check over the install; off refuses every extended car for its VERSION (as
# acs.exe does), --csp only the COSMIC and CSP-encrypted ones, each with its reason
for mode in off csp; do
    keep "refused $mode" || continue
    flag=; [ $mode = csp ] && flag=--csp
    target/release/acd_check.exe --steps 1 --refused $flag > "$out/acd_$mode.txt" 2>&1
    got=$(grep '^  REFUSED ' "$out/acd_$mode.txt" | tr -d '\r')
    names=$(echo "$got" | awk '{sub(":", "", $2); print $2}' | sort | tr '\n' ' ')
    if [ $mode = off ]; then
        want=$(echo $extended $still_refused | tr ' ' '\n' | sort | tr '\n' ' ')
        bad=$(echo "$got" | grep -vc 'VERSION=extended')
    else
        want=$(echo $still_refused | tr ' ' '\n' | sort | tr '\n' ' ')
        bad=$(echo "$got" | grep -v 'urd_darche_992_23.*encrypted for Custom Shaders Patch by its maker' | grep -vc 'COSMIC suspension: a later rustyAC version')
    fi
    changed=$(grep -o '^FAILED [^:]*: files differ' "$out/acd_$mode.txt" | awk '{sub(":", "", $2); print $2}' | sort -u | tr '\n' ' ')
    if [ -n "$changed" ]; then
        skip "refused $mode" "content changed: ${changed% }, refresh cardata/"
    elif [ "$names" = "$want" ] && [ "$bad" = 0 ] && grep -q '^RESULT' "$out/acd_$mode.txt"; then
        verdict "refused $mode" IDENTICAL "$(echo "$names" | wc -w) refused, each with the expected reason"
    else
        verdict "refused $mode" DIFFERENT "refused: $names(want $want), $bad with another reason"
    fi
done

if keep layouts; then
    target/release/track_survey.exe > "$out/survey_off.md" 2>&1
    target/release/track_survey.exe --csp > "$out/survey_csp.md" 2>&1
    # rows that changed: each one of the six, refused before and loaded now
    changed=$(diff "$out/survey_off.md" "$out/survey_csp.md" | grep '^> ' | sed 's/^> | `\([^`]*\)`.*/\1/' | tr -d ' ' | sort | tr '\n' ' ')
    want=$(echo $layouts | tr ' ' '\n' | sort | tr '\n' ' ')
    refused_now=$(grep -cE 'fa_2026.*(\*\*refused|\*\*does not load)' "$out/survey_csp.md")
    if [ "$changed" = "$want" ] && [ "$refused_now" = 0 ] && [ "$(wc -l < "$out/survey_off.md")" = "$(wc -l < "$out/survey_csp.md")" ]; then
        verdict layouts IDENTICAL "the six *_fa_2026 layouts load with --csp, nothing else changes"
    else
        verdict layouts DIFFERENT "changed: $changed(want $want), $refused_now still refused"
    fi
fi

if echo "$layouts" | tr ' ' '\n' | while read -r l; do keep "wav ${l%/*} ${l#*/}" && exit 0; done; then
    target/release/track_survey.exe --csp --wav-pitch 2>&1 | tr -d '\r' > "$out/wav_port.txt"
fi
for l in $layouts; do
    t=${l%/*}; y=${l#*/}
    keep "wav $t $y" || continue
    file=$ac/content/tracks/$t/$y/data/surfaces.ini
    [ -f "$file" ] || { skip "wav $t $y" "not installed"; continue; }
    port=$(awk -v t="$t" -v y="$y" '$1 == t && $2 == y { print $3 " " $4 }' "$out/wav_port.txt")
    if [ -z "$port" ]; then
        verdict "wav $t $y" DIFFERENT "the port read no WAV_PITCH=extended-N"
    elif oracle "wav_$t" --surfaces "$(cygpath -w "$file")"; then
        game=$(tr -d '\r' < "$out/runs/wav_$t.txt" | awk '$2 == "WAV_PITCH" { print $1 " " $3 }')
        missing=$(echo "$port" | while read -r line; do echo "$game" | grep -qxF "$line" || echo "$line"; done)
        if [ -z "$missing" ]; then
            verdict "wav $t $y" IDENTICAL "$port"
        else
            verdict "wav $t $y" DIFFERENT "port $missing, game: $(echo "$game" | head -n 2 | tr '\n' ' ')"
        fi
    else
        verdict "wav $t $y" DIFFERENT "csp_oracle failed: $(why "$out/runs/wav_$t.txt" 'error|panic|exit')"
    fi
done

# air <case>: track:ambient:road:wind, the highest and the lowest table tracks installed beside Spa, an
# unstable (road warmer), a calm and a stable (road colder) case; still = the car standing
tape=oracle/game/car_random.ryin
for c in spa:spa:14:20:3 ks_black_cat_county:ks_black_cat_county:26:38:0 ks_zandvoort:ks_zandvoort:18:16:6 still:spa:14:20:3; do
    name=${c%%:*}; spec=${c#*:}
    keep "air $name" || continue
    [ -f "$tape" ] || { skip "air $name" "no $tape (car_oracle run --scenario random)"; continue; }
    flag=; [ $name = still ] && flag=--still
    if oracle "air_$name" --car ks_ferrari_f2004 --drive "$(cygpath -w "$tape")" $flag --air "$spec"; then
        run=$out/runs/air_$name.txt
        line=$(tr -d '\r' < "$run" | grep -m1 '^air: [0-9]* steps')
        if grep -q '^air: altitude table: .* all as CSP' "$run" && [ "$(grep -c '^air: grid: .* 0 differ' "$run")" = 2 ]             && echo "$line" | grep -q '^air: 20001 steps, differ: plane 0, height 0, temperature 0, density 0'; then
            verdict "air $name" IDENTICAL "${line#air: }"
        else
            verdict "air $name" DIFFERENT "$(why "$run" '^air|error|panic|fault')"
        fi
    else
        verdict "air $name" DIFFERENT "csp_oracle failed: $(why "$out/runs/air_$name.txt" 'error|panic|fault|exit')"
    fi
done

# ode <car>: CSP's double ODE (Task C5c, docs/csp/core.md): a 600-step Spa drive logs the inputs of its
# rotation functions and dxStepBody (breakpoint dumps), then rustyac-ode's f64 functions against CSP's own on those,
# edge values and random bits, at two MXCSR values (26 lines, each "0 differ"); the ball, fixed, contact and dball joint rows take the drive's bodies;
# then the solver kernels (Task C5d, csp_matrix) on 1..40-row matrices from the drive's values and dSolveLCP (csp_lcp) on step-shaped problems (24 lines, each "0 differ")
lap=oracle/game/track_spa_lap.ryin
for car in ks_ferrari_f2004 rss_formula_rss_3_v6_2026; do
    keep "ode $car" || continue
    [ -f "$lap" ] || { skip "ode $car" "no $lap"; continue; }
    if CSP_ORACLE_BREAK=168b7b0d,168b990d,168bca0d,16c61b0b oracle "ode_drive_$car" --car $car --track "$ac/content/tracks/spa" --drive "$(cygpath -w "$lap")" --workers 0 --steps 600 \
        && oracle "ode_$car" --ode "$(cygpath -aw "$out/runs/ode_drive_$car/car_root/breaks.txt")" \
        && oracle "solver_$car" --solver "$(cygpath -aw "$out/runs/ode_drive_$car/car_root/breaks.txt")"; then
        run=$out/runs/ode_$car.txt; srun=$out/runs/solver_$car.txt
        if [ "$(tr -d '\r' < "$run" | grep -c '^ode [a-z_0-9]* mxcsr 0x[0-9a-f]*: [0-9]* inputs, 0 differ')" = 26 ] \
            && [ "$(tr -d '\r' < "$srun" | grep -c '^solver [a-z_0-9]* mxcsr 0x[0-9a-f]*: [0-9]* inputs, 0 differ')" = 24 ]; then
            verdict "ode $car" IDENTICAL "11 functions + contact and dball rows + 10 solver kernels + dSolveLCP x 2 MXCSR, $(tr -d '\r' < "$run" | grep -m1 '^ode: ' | cut -d' ' -f2-6) from the drive, 0 differ"
        else
            verdict "ode $car" DIFFERENT "$(why "$run" '^ode|error|panic|fault') $(why "$srun" '^solver|error|panic|fault')"
        fi
    else
        verdict "ode $car" DIFFERENT "csp_oracle failed: $(why "$r.txt" 'error|panic|fault|exit')"
    fi
done

# island <car>: CSP's dxStepIsland (Task C5d, csp_step): a 600-step Spa drive records every island at
# dxStepIsland's entry, at dxSolveLCP's entry and after the world step (CSP_ORACLE_ISLANDS, every joint given a
# feedback block), then rustyac-ode steps each recorded island from the same state: bodies, joint feedback
# and the solver's inputs bit for bit (one line, "0 differ, solver inputs 0 differ")
for car in ks_ferrari_f2004 rss_formula_rss_3_v6_2026; do
    keep "island $car" || continue
    [ -f "$lap" ] || { skip "island $car" "no $lap"; continue; }
    bin=$out/runs/island_$car.bin; rm -f "$bin"
    if CSP_ORACLE_BREAK=16c24b0i,16c6d00w,16ffe10l CSP_ORACLE_ISLANDS="$(cygpath -aw "$bin")" CSP_ORACLE_FEEDBACK=1         oracle "island_drive_$car" --car $car --track "$ac/content/tracks/spa" --drive "$(cygpath -w "$lap")" --workers 0 --steps 600         && [ -s "$bin" ]; then
        run=$out/runs/island_$car.txt
        "$CSP" --islands "$(cygpath -aw "$bin")" > "$run" 2>&1
        line=$(tr -d '\r' < "$run" | grep -m1 '^island ')
        if echo "$line" | grep -q '^island 600 steps, 600 islands (.*): 0 differ, solver inputs 0 differ'; then
            verdict "island $car" IDENTICAL "${line#island }"
        else
            verdict "island $car" DIFFERENT "$(why "$run" '^island|^step|error|panic')"
        fi
        rm -f "$bin"
    else
        verdict "island $car" DIFFERENT "csp_oracle failed: $(why "$out/runs/island_drive_$car.txt" 'error|panic|fault|exit')"
    fi
done

# world <car> <n>: CSP's world step (Task C5e, csp_world): a 600-step Spa drive with n cars (20 m apart along
# the spawn heading) records the whole ODE world before the auto-disable / island build and after the islands
# were stepped (CSP_ORACLE_ISLANDS, records 4 and 5), once with CSP's worker count 0 and once with 7; rustyac-ode's
# CspWorld steps every recorded world (the recorded contacts fed in): every body, its auto-disable state and every
# joint feedback bit for bit in both recordings, and both recordings' after-states the same (hash)
for car in ks_ferrari_f2004 rss_formula_rss_3_v6_2026; do
    for n in 1 3; do
        keep "world $car $n" || continue
        [ -f "$lap" ] || { skip "world $car $n" "no $lap"; continue; }
        lines=
        for w in 0 7; do
            bin=$out/runs/world_${car}_${n}_$w.bin; rm -f "$bin"
            run=$out/runs/world_${car}_${n}_$w.txt
            if CSP_ORACLE_BREAK=16c7650a,92ea7dz CSP_ORACLE_ISLANDS="$(cygpath -aw "$bin")" CSP_ORACLE_FEEDBACK=1 oracle "world_drive_${car}_${n}_$w" --car $car --track "$ac/content/tracks/spa" --drive "$(cygpath -w "$lap")" --cars $n --workers $w --steps 600 && [ -s "$bin" ]; then
                "$CSP" --world "$(cygpath -aw "$bin")" > "$run" 2>&1
            else
                echo "csp_oracle failed: $(why "$out/runs/world_drive_${car}_${n}_$w.txt" 'error|panic|fault|exit')" > "$run"
            fi
            rm -f "$bin"
            lines="$lines$(tr -d '\r' < "$run" | grep -m1 '^world \|^csp_oracle failed')
"
        done
        w0=$(echo "$lines" | sed -n 1p); w7=$(echo "$lines" | sed -n 2p)
        if echo "$w0" | grep -q "^world 600 steps, $((600 * n)) islands, .*: 0 differ, after " && [ "$w0" = "$w7" ]; then
            verdict "world $car $n" IDENTICAL "${w0#world } (workers 7 the same)"
        else
            verdict "world $car $n" DIFFERENT "workers 0: $w0 / workers 7: $w7 $(why "$out/runs/world_${car}_${n}_0.txt" '^step|panic')"
        fi
    done
done

# collision <car> <n>: CSP's collision step (Task C5f, csp_collision): a 600-step Spa drive with n cars (CSP
# workers 0) where, at every step, the oracle copies CSP's geoms, spaces, bodies and kept pairs into the port's
# Collision and runs CspWorld::collide, then compares the contact joints CSP made (order and every word), the
# kept pairs, every box's OPCODE cache and every geom's flags, box and pose; 8 made-up poses a step through CSP's
# space computeAABB against the port's refresh (both MXCSR settings, NaN against NaN counted apart). The world
# is recorded with the port's joints (record 6) and CspWorld steps every recorded world on them instead of
# CSP's: every body and joint feedback bit for bit
for car in ks_ferrari_f2004 rss_formula_rss_3_v6_2026; do
    for n in 1 3; do
        keep "collision $car $n" || continue
        [ -f "$lap" ] || { skip "collision $car $n" "no $lap"; continue; }
        bin=$out/runs/collision_${car}_${n}.bin; rm -f "$bin"
        run=$out/runs/collision_${car}_${n}.txt
        drive=$out/runs/collision_drive_${car}_${n}.txt
        if CSP_ORACLE_BREAK=92c2b0q,16c7650a,92ea7dz CSP_ORACLE_ISLANDS="$(cygpath -aw "$bin")" CSP_ORACLE_FEEDBACK=1 oracle "collision_drive_${car}_${n}" --car $car --track "$ac/content/tracks/spa" --drive "$(cygpath -w "$lap")" --cars $n --workers 0 --steps 600 && [ -s "$bin" ]; then
            "$CSP" --world "$(cygpath -aw "$bin")" > "$run" 2>&1
        else
            echo "csp_oracle failed: $(why "$drive" 'error|panic|fault|exit')" > "$run"
        fi
        rm -f "$bin"
        col=$(tr -d '\r' < "$drive" | grep -m1 '^collision: ')
        wld=$(tr -d '\r' < "$run" | grep -m1 '^world \|^csp_oracle failed')
        if echo "$col" | grep -q "^collision: 300 static + 300 dynamic steps, 600 compared (.*), 0 differ, not ported {};.* 0x1f80 [0-9]* cases 0 differ .* 0x9fc0 [0-9]* cases 0 differ" &&
            echo "$wld" | grep -q "^world 600 steps (600 on the port's contacts), $((600 * n)) islands, .*: 0 differ, after "; then
            verdict "collision $car $n" IDENTICAL "$(echo "$col" | sed 's/^collision: //; s/; (class.*made-up/; made-up/') / ${wld#world }"
        else
            verdict "collision $car $n" DIFFERENT "$col / $wld $(why "$drive" '^  step|^  pose|panic')"
        fi
    done
done

# core <car> <n>: CSP's car physics core (Task C5g, csp_core): every call the car makes into CSP's cphys::
# wrappers (tools/csp_oracle/wrappers.txt, the wrapper vtables' functions) is done again on the port, which
# builds its own bodies, joints, colliders, spaces and contacts from them and nothing else: the answers
# (getters, creators) against CSP's returns and, at every world step's entry, every body, joint, car space,
# geom, kept pair and the contact load against CSP's. n=2: car 0 parked, car 1 5.5 m behind drives into it
# (1600 steps, contacts from step ~800), CSP's trimesh-trimesh collider also called on the live pair and 8
# moved shadows against csp_ttl. The world is recorded with the port's joints (record 6) and CspWorld steps
# every recorded world on them: the contact joints word for word, every body and joint bit for bit.
# C5h: the track rays too (+0x92fac0 set-up, +0x92b970 tyre): the port's own Embree scene and ODE ray,
# CSP's Embree-ready race pinned to the port's (932900y), CSP's Embree device "threads=1" (18d4260n)
# C5i-a: the port's track and car colliders are built by rustyAC's loaders (CSP_ORACLE_LOADER=use), each
# also compared with CSP's own ("loader track/car" lines, 0 differ).
for cn in "ks_ferrari_f2004 1" "rss_formula_rss_3_v6_2026 1" "ks_ferrari_f2004 2"; do
    set -- $cn; car=$1; n=$2
    keep "core $car $n" || continue
    [ -f "$lap" ] || { skip "core $car $n" "no $lap"; continue; }
    bin=$out/runs/core_${car}_${n}.bin; rm -f "$bin"
    run=$out/runs/core_${car}_${n}.txt
    drive=$out/runs/core_drive_${car}_${n}
    brk=$(sed 's/$/p/' tools/csp_oracle/wrappers.txt | paste -sd,),1688ab0p,16c7650a,92ea7dz,932900y,18d4260n
    steps=600; extra=
    [ $n = 2 ] && { steps=1600; brk=$brk,16bc1b0t; extra="CSP_ORACLE_GAP=5.5 CSP_ORACLE_PARK=1"; }
    # (a subshell: `oracle` is a function, `env` cannot run it)
    if ( [ -n "$extra" ] && export $extra; CSP_ORACLE_LOADER=use CSP_ORACLE_BREAK=$brk CSP_ORACLE_ISLANDS="$(cygpath -aw "$bin")" CSP_ORACLE_PORT=1 CSP_ORACLE_BREAK_SETUP=1 oracle "core_drive_${car}_${n}" --car $car --track "$ac/content/tracks/spa" --drive "$(cygpath -w "$lap")" --cars $n --workers 0 --steps $steps ) && [ -s "$bin" ]; then
        "$CSP" --world "$(cygpath -aw "$bin")" > "$run" 2>&1
    else
        echo "csp_oracle failed: $(why "$drive.txt" 'error|panic|fault|exit')" > "$run"
    fi
    rm -f "$bin"
    prt=$(tr -d '\r' < "$drive.txt" | grep -m1 '^port: ')
    stop=$(tr -d '\r' < "$drive.txt" | grep -m1 '^port stopped')
    ttl=$(tr -d '\r' < "$drive.txt" | grep -m1 '^ttl live ')
    wld=$(tr -d '\r' < "$run" | grep -m1 '^world \|^csp_oracle failed')
    ldr=$(tr -d '\r' < "$drive.txt" | grep '^loader ' | paste -sd' ')
    if [ -z "$stop" ] && echo "$ldr" | grep -q "^loader .*settings same" && ! echo "$ldr" | grep -q "[1-9][0-9]* differ" && echo "$prt" | grep -q "^port: $((2 * steps)) world steps compared, 0 differ; answers set-up [0-9]* (0 wrong), steps [0-9]* (0 wrong)" &&
        { [ $n = 1 ] || echo "$ttl" | grep -q "^ttl live [1-9][0-9]* calls 0 differ, shadow [1-9][0-9]* calls 0 differ"; } &&
        echo "$wld" | grep -q "^world $steps steps ($steps on the port's contacts), .*: 0 differ, after "; then
        verdict "core $car $n" IDENTICAL "$(echo "$prt" | sed 's/^port: //; s/; calls.*//') ${ttl:+/ $ttl }/ ${wld#world }"
    else
        verdict "core $car $n" DIFFERENT "$stop $ldr / $prt / $ttl / $wld $(why "$drive.txt" '^  world step|^  body|panic')"
    fi
done

# row <car> <drive>: the whole car, step by step (Task C5i-b): CSP live drives the tape (csp_oracle,
# every car state after every step, states.bin), rustyac --csp replays the same tape on the same machine
# (--physics-states), csp_oracle --compare-states compares every value of the two. Also counted: CSP's
# car collision callback +0x587cc0 (not ported). The first differing value printed skips the state
# fields only CSP's getPhysicsState hook (+0x590220) or rustyAC's view fill (the view list below)
view='tyre_surface_def|last_ff_final|body_work_volume|time_stamp|ride_height|setup_state|gear_rpm_window|tyre_thermal_states|lock_controls_time|anti_squat|caster|ground_plane|normalized_spline_position|acc_g|wheel_angular_speed'
for row in "ks_ferrari_f2004 lap 4000" "rss_formula_rss_3_v6_2026 lap 4000" "ks_ferrari_f2004 wall 6667"; do
    set -- $row; car=$1; kind=$2; steps=$3
    keep "row $car $kind" || continue
    tape=$lap; [ $kind = wall ] && tape=oracle/game/collide_spa_wall_high.ryin
    [ -f "$tape" ] || { skip "row $car $kind" "no $tape"; continue; }
    build rustyac cargo build --release --locked --quiet -p rustyac-game --bin rustyac
    r=$out/runs/row_${car}_$kind
    CSP_ORACLE_BREAK=587cc0 oracle "row_${car}_$kind" --car $car --track "$ac/content/tracks/spa" --drive "$(cygpath -w "$tape")" --workers 0 --steps $steps
    # the tape for rustyac: this car, CSP mode, cut after its first $steps steps (40 bytes a step)
    perl -0777 -pe "s/^car=.*\$/car=$car/m; s/\nend\n/\ncsp=1\nend\n/" "$tape" > "$r.ryin.full"
    head -c $((steps * 40 + $(perl -0777 -ne 'print index($_, "\nend\n") + 5' "$r.ryin.full"))) "$r.ryin.full" > "$r.ryin"
    rustyac target/release/rustyac.exe --replay "$r.ryin" --headless --no-shm --physics-threads 0 --state-hash --physics-states "$r.port.bin" > "$r.port.txt" 2>&1
    if [ -s "$r/car_root/states.bin" ] && [ -s "$r.port.bin" ]; then
        "$CSP" --compare-states "$r/car_root/states.bin" "$r.port.bin" 1 > "$r.cmp.txt" 2>&1
    else
        echo "csp_oracle failed: $(why "$r.txt" 'error|panic|fault|exit') / rustyac: $(why "$r.port.txt" 'error|panic|rustyac:')" > "$r.cmp.txt"
    fi
    cb=$(grep -c ' 0x587cc0 ' "$r/car_root/breaks.txt" 2> /dev/null)
    eq=$(tr -d '\r' < "$r.cmp.txt" | grep -m1 '^car 0: ')
    first=$(tr -d '\r' < "$r.cmp.txt" | grep '^  step ' | grep -vE "^  step [0-9]+ ($view)\[" | head -n 1 | sed 's/^  //')
    if echo "$eq" | grep -qE '^car 0: ([0-9]+) of \1 values equal at every step'; then
        verdict "row $car $kind" IDENTICAL "$steps steps: ${eq#car 0: }"
    else
        verdict "row $car $kind" DIFFERENT "$steps steps: ${eq:-$(head -n 1 "$r.cmp.txt")}; first (past the view list): $first; +0x587cc0 called ${cb:-0} times"
    fi
done

# smoke: rustyAC's own CSP mode (Task C5i-a part 2: its cars' physics in csp_core, the tyres on CSP's rays):
# both proof cars driven 10 s at Spa (the F2004 by the autodriver, the RSS beside it on its own), then the
# recording played back twice: no panic, the car still on the road (finite, under 400 km/h), one hash
if keep "smoke"; then
    build rustyac cargo build --release --locked --quiet -p rustyac-game --bin rustyac
    s=$out/runs/smoke; rm -rf "$s"; mkdir -p "$s"
    rustyac target/release/rustyac.exe --csp --headless --no-shm --no-audio --car ks_ferrari_f2004 --opponent rss_formula_rss_3_v6_2026:auto \
        --track spa --no-race-ini --spawn start --autodrive --duration 10 --record "$s/drive.ryin" > "$s/drive.txt" 2>&1
    car=$(tr -d '\r' < "$s/drive.txt" | grep -m1 '^the car after ')
    for i in 1 2; do
        rustyac target/release/rustyac.exe --replay "$s/drive.ryin" --headless --no-shm --state-hash --physics-threads 0 > "$s/replay$i.txt" 2>&1
    done
    h1=$(tr -d '\r' < "$s/replay1.txt" | grep -m1 '^state hash ' | cut -d' ' -f3-7)
    h2=$(tr -d '\r' < "$s/replay2.txt" | grep -m1 '^state hash ' | cut -d' ' -f3-7)
    if echo "$car" | grep -qE '^the car after [0-9]{4,} steps: [0-3]?[0-9]?[0-9]\.[0-9] km/h' && ! grep -qwiE 'nan|inf' "$s/drive.txt" &&
        echo "$h1" | grep -qE '^[0-9a-f]{16}: 2 cars, [0-9]{4,} steps' && [ "$h1" = "$h2" ]; then
        verdict "smoke" IDENTICAL "${car#the car after } / replayed twice: $h1"
    else
        verdict "smoke" DIFFERENT "$car / $h1 / $h2 $(why "$s/drive.txt" 'panic|error|rustyac:') $(why "$s/replay1.txt" 'panic|error|rustyac:')"
    fi
fi

summary
