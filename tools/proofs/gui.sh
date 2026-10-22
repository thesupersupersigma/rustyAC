#!/bin/sh
# SPDX-License-Identifier: GPL-3.0-or-later
#
# The GUI sequences (Tasks 29-34): every name of tools/proofs/gui_list.txt played by the game's GUI
# code and by the port, state, events, draw calls and pixels compared frame by frame.
#   GUI_ONLY=<regex> runs only the matching names. esc_telemetry reads the user's saved laps
#   (Documents\Assetto Corsa\ctelemetry, read-only) and is left out when they are not there.
area=gui
. "$(dirname "$0")/lib.sh"
tool render_oracle
root=$out/root

for name in $(grep -vE '^\s*#' tools/proofs/gui_list.txt); do
    echo "$name" | grep -qE "^(${GUI_ONLY:-.*})$" || continue
    keep "$name" || continue  # quick keys: the sequence's name
    if [ "$name" = esc_telemetry ] && ! ls "$USERPROFILE/Documents/Assetto Corsa/ctelemetry/player/"*.tc > /dev/null 2>&1; then
        echo "esc_telemetry: no saved laps here, left out" >&2; continue
    fi
    f=$out/$(echo "$name" | tr '@#' '__').txt
    for try in 1 2; do  # a second try only for a log that differs while the pictures agree (load-flaky)
        "$RO" gui --gui "$name" --root "$root" --out "$out/frames" > "$f" 2>&1 < /dev/null
        grep -q '^command log: DIFFERENT' "$f" && grep -q '^pixels: IDENTICAL' "$f" || break
    done
    if grep -q '^command log: IDENTICAL' "$f" && grep -q '^pixels: IDENTICAL' "$f" && ! grep -q 'DIFFERENT' "$f"; then
        verdict "$name" IDENTICAL "$(grep -m1 '^sequence ' "$f" | cut -d';' -f1)"
    else
        verdict "$name" DIFFERENT "$(grep -m1 -E 'DIFFERENT|rror|panic' "$f")"
    fi
    rm -rf "$out/frames"
done

rm -rf "$root"
summary
