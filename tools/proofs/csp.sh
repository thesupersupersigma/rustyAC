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
#   ode <car>      CSP's double ODE functions against rustyac-ode's f64 ones (Task C5c), inputs from a Spa drive
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
        port=$(target/release/acd_check.exe --steps 1 --only "$car" --csp --pressures | tr -d '
' | grep -m1 '^PRESSURES ')
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
# edge values and random bits, at two MXCSR values (18 lines, each "0 differ")
lap=oracle/game/track_spa_lap.ryin
for car in ks_ferrari_f2004 rss_formula_rss_3_v6_2026; do
    keep "ode $car" || continue
    [ -f "$lap" ] || { skip "ode $car" "no $lap"; continue; }
    if CSP_ORACLE_BREAK=168b7b0d,168b990d,168bca0d,16c61b0b oracle "ode_drive_$car" --car $car --track "$ac/content/tracks/spa" --drive "$(cygpath -w "$lap")" --workers 0 --steps 600 \
        && oracle "ode_$car" --ode "$(cygpath -aw "$out/runs/ode_drive_$car/car_root/breaks.txt")"; then
        run=$out/runs/ode_$car.txt
        if [ "$(tr -d '\r' < "$run" | grep -c '^ode [a-z_0-9]* mxcsr 0x[0-9a-f]*: [0-9]* inputs, 0 differ')" = 18 ]; then
            verdict "ode $car" IDENTICAL "9 functions x 2 MXCSR, $(tr -d '\r' < "$run" | grep -m1 '^ode: ' | cut -d' ' -f2-6) from the drive, 0 differ"
        else
            verdict "ode $car" DIFFERENT "$(why "$run" '^ode|error|panic|fault')"
        fi
    else
        verdict "ode $car" DIFFERENT "csp_oracle failed: $(why "$r.txt" 'error|panic|fault|exit')"
    fi
done

summary
