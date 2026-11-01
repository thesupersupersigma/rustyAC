#!/bin/bash
# Task 46b step 4: the last camera remembered. Two ESC-start runs of rustyac.exe on one cfg folder:
# the first takes [LAST_CAMERA] from the Documents given (drivable view 3), drives from the ESC
# menu (loadPreviousSessionUserCamera) and writes rustyAC's own copy at its end; the second
# reads that copy. Then the same with MODE=0 (the cockpit, the drivable view kept).
cd "$(dirname "$0")/../../.." || exit 1
D=re/scratch/task46b/last
rm -rf $D; mkdir -p $D/docs/cfg
run() { # <name> <MODE> <DRIVABLE_MODE> <fresh cfg: 1>
    [ "$4" = 1 ] && rm -rf $D/cfg && mkdir -p $D/cfg
    printf '[LAST_CAMERA]\r\nMODE=%s\r\nDRIVABLE_MODE=%s\r\n\r\n[FADE]\r\nCOLOR=0,0,0\r\n' "$2" "$3" > $D/docs/cfg/camera_manager.ini
    target/release/rustyac.exe --car bmw_m3_e30 --track magione --no-race-ini --esc-start --gui-do drive --duration 5 \
        --documents "$(cygpath -am $D/docs)" --cfg-dir "$(cygpath -am $D/cfg)" --bench-shot "$D/$1.png" > $D/$1.txt 2>&1
    echo "$1: $(grep '^CAMERA: ' $D/$1.txt) | file: $(tr -d '\r' < $D/cfg/camera_manager.ini | grep -E '^(MODE|DRIVABLE_MODE)=' | tr '\n' ' ')"
}
run first 2 3 1
run second 0 0      # Documents now says the cockpit: the own copy (2 3) wins
run cockpit 0 3 1
