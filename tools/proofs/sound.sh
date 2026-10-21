#!/bin/sh
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Task 19 (docs/port/audio.md, sections 5.1 to 5.4): the game's own sound code against the port's
# on the 18 saved drives of oracle/audio, the two DSP plug-ins, and rustyac.exe end to end on the
# drives whose input files are kept (oracle/audio_e2e, oracle/game/audio_e2e_*.ryin).
# A drive counts as identical when audio_oracle's verdict is "ok": every call to FMOD the same, the
# mix as close to the game's as the game's is to itself (FMOD's mix never repeats bit for bit).
area=sound
. "$(dirname "$0")/lib.sh"
tool audio_oracle
need oracle/audio/ks_ferrari_f2004/spa_launch.audiotape "Task 19's tapes"
cargo build --release --locked --quiet -p rustyac-game --bin rustyac || exit 2

while read -r car scenario camera; do
    for try in 1 2; do  # FMOD's mix is not repeatable: a second try only when every call log agreed
        "$AO" compare --tape "oracle/audio/$car/$scenario.audiotape" --camera "$camera" --out "$out/cmp" > "$out/row.txt" 2>&1 < /dev/null
        cat "$out/row.txt" >> "$out/rows.txt"
        row=$(grep -m1 "^$car |" "$out/row.txt")
        case "$row" in *"| identical | identical | identical |"*"| FAIL") ;; *) break ;; esac
    done
    case "$row" in *"| ok"|*"| ok ("*) verdict "$car $scenario $camera" IDENTICAL ;; *) verdict "$car $scenario $camera" DIFFERENT "${row:-$(tail -n 1 "$out/row.txt")}" ;; esac
    rm -rf "$out/cmp"
done <<ROWS
ks_ferrari_f2004 spa_launch cockpit
ks_ferrari_f2004 spa_launch chase
ks_ferrari_f2004 spa_launch trackside
ks_ferrari_f2004 spa_grass cockpit
ks_ferrari_f2004 spa_kerb_strike cockpit
ks_ferrari_f2004 spa_wall_gravel chase
ks_ferrari_f2004 spa_wall_high trackside
ks_ferrari_f2004 pt_shifter cockpit
ks_ferrari_f2004 pt_protect cockpit
ks_ferrari_f2004 brake chase
bmw_m3_e30 wc_stops cockpit
vrc_formula_alpha_2026 hy_deploy cockpit
vrc_formula_alpha_2026 spa_hybrid_timing chase
ks_ferrari_250_gto trk_kerb_strike cockpit
ferrari_f40 trk_lap cockpit
bmw_m3_e30 trk_lap chase
ks_ferrari_488_gt3 trk_lap cockpit
ks_ferrari_f2004 trk_lap cockpit
ROWS

"$AO" dsp --count 2000 > "$out/dsp.txt" 2>&1 && verdict "dsp plug-ins" IDENTICAL || verdict "dsp plug-ins" DIFFERENT "$(tail -n 1 "$out/dsp.txt")"

# end to end: the game's sound on the game's physics, then rustyac.exe replaying the same drive with
# the game's answers to its sound calls (the "scripted" column of section 5.3)
root=$out/root_e2e
while read -r scenario camera; do
    "$AO" run --side ac --tape "oracle/audio_e2e/$scenario.audiotape" --camera "$camera" --out "$out/e2e" --root "$root" > "$out/e2e.txt" 2>&1
    game_log=$(ls "$out"/e2e/*_"${scenario}_${camera}"_ac.log 2>/dev/null | head -n 1)
    target/release/rustyac.exe --replay "oracle/game/audio_e2e_$scenario.ryin" --headless --no-shm \
        --audio-wav "$out/e2e/port.wav" --audio-log "$out/e2e/port.log" --audio-oracle-camera "$camera" \
        --audio-ini "$root/cfg/audio.ini" --audio-script "$game_log" >> "$out/e2e.txt" 2>&1
    r=$(python tools/proofs/logdiff.py "$game_log" "$out/e2e/port.log" | head -n 1)
    case "$r" in IDENTICAL*) verdict "rustyac.exe $scenario $camera" IDENTICAL "$r" ;; *) verdict "rustyac.exe $scenario $camera" DIFFERENT "$r" ;; esac
    rm -rf "$out/e2e"
done <<ROWS
spa_launch cockpit
spa_launch trackside
spa_kerb_strike chase
ROWS

summary
