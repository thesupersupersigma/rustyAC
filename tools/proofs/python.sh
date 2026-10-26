#!/bin/sh
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Python apps (Task 42): AC's PythonInterface and rustyac-python load the same apps (AC's Chat and
# gMeter, the test apps of tools/render_oracle/py_apps) on the player's own python33.dll, in the
# same scripted session; draw calls, pixels, state, the console, py_log.txt and every value the
# test apps log compared frame by frame. Quick: py_quick (13 frames); full: py_apps too (69).
area=python
. "$(dirname "$0")/lib.sh"
tool render_oracle
root=$out/root

for name in py_quick py_apps; do
    keep "$name" || continue  # quick keys: the sequence's name
    f=$out/$name.txt
    "$RO" gui --gui "$name" --root "$root" --out "$out/frames" > "$f" 2>&1 < /dev/null
    if grep -q '^command log: IDENTICAL' "$f" && grep -q '^pixels: IDENTICAL' "$f" && ! grep -q 'DIFFERENT' "$f"; then
        verdict "$name" IDENTICAL "$(grep -m1 '^sequence ' "$f" | cut -d';' -f1), $(grep -c '^file .*IDENTICAL' "$f") files"
    else
        verdict "$name" DIFFERENT "$(grep -m1 -E 'DIFFERENT|rror|panic' "$f")"
    fi
    rm -rf "$out/frames"
done

rm -rf "$root"
summary
