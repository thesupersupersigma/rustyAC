#!/bin/bash
# SPDX-License-Identifier: MIT OR Apache-2.0
# rustyAC's Linux checklist for a tester: run it from the unpacked rustyAC folder,
#
#   ./linux_check.sh
#
# then send back the rustyac-check-<time>.tar.gz it makes. With a test build, the game files
# come with it (rustyAC-test-pack.zip, used by itself). About 10 minutes; a few steps ask you
# to drive or to answer y / n. What it collects (and nothing else): the distribution, kernel,
# CPU, RAM, graphics card and driver, audio server and the versions of the packages rustyAC
# needs; how many tracks were found (counts only, no names but missing Kunos tracks); the output
# of each step (frame rates, a screenshot of a car on Laguna Seca, the
# launcher's log without the values it sends, the devices found) and your answers. Your home
# folder, user name and computer name are replaced by ~, <user> and <host> everywhere; no file of
# your game, your settings or your replays is copied.
set -u
cd "$(dirname "$0")"
stamp=$(date +%Y%m%d-%H%M%S)
o=rustyac-check-$stamp
mkdir -p "$o"
say() { printf '\n== %s\n' "$*" | tee -a "$o/answers.txt"; }
ask() {
    local a=""
    while [[ $a != y && $a != n ]]; do read -r -p "$1 (y/n) " a; done
    echo "$1 -> $a" >> "$o/answers.txt"
}
# a step's output, bounded in time
run() {
    local name=$1 limit=$2
    shift 2
    echo "\$ $*" > "$o/$name.txt"
    timeout --kill-after=10 "$limit" "$@" >> "$o/$name.txt" 2>&1
    echo "exit $?" >> "$o/$name.txt"
}
# a test build's pack of game files beside the programs, else the game files the launcher
# remembered (a zip or extra folders included)
pack=rustyAC-test-pack.zip
remembered=${XDG_CONFIG_HOME:-$HOME/.config}/rustyAC/game_files.txt
if [[ -f $pack ]]; then
    unset AC_ROOT RUSTYAC_GAME_FILES
elif [[ -f $remembered ]]; then
    export RUSTYAC_GAME_FILES=$remembered
fi
drive=(--track ks_laguna_seca --car bmw_m3_e30)

say "1/8 the computer"
{
    grep -E '^(NAME|VERSION_ID|BUILD_ID)=' /etc/os-release
    uname -srm
    grep -m1 'model name' /proc/cpuinfo
    grep MemTotal /proc/meminfo
    echo "session: ${XDG_SESSION_TYPE:-?} ${XDG_CURRENT_DESKTOP:-?}"
    if command -v vulkaninfo > /dev/null; then vulkaninfo --summary 2> /dev/null | grep -E 'apiVersion|driverName|driverInfo|deviceName|deviceType'; else echo "vulkaninfo: not installed (vulkan-tools)"; lspci 2> /dev/null | grep -iE 'vga|3d'; fi
    command -v pactl > /dev/null && pactl info 2> /dev/null | grep -E 'Server Name|Server Version'
    if command -v pacman > /dev/null; then pacman -Q webkit2gtk-4.1 gtk3 vulkan-icd-loader mesa nvidia-utils pipewire-pulse pulseaudio 2>&1 | grep -v 'was not found'; fi
    if command -v dpkg-query > /dev/null; then dpkg-query -W libwebkit2gtk-4.1-0 libgtk-3-0 libvulkan1 mesa-vulkan-drivers 2>&1; fi
    echo "libraries not found:"; ldd ./rustyac ./rustyac-launcher | grep 'not found' || echo "  none"
} > "$o/system.txt" 2>&1
cat "$o/system.txt"

say "2/8 the game files (counts only)"
# only counts, and which of Kunos' base-game tracks are missing: never the track list (mod names)
list=$(timeout --kill-after=10 300 ./rustyac --list-tracks 2>&1)
{
    [[ -f $pack ]] && echo "the test pack"
    echo "drivable: $(grep -c '^--track ' <<< "$list")"
    echo "refused: $(grep -c ' refused: ' <<< "$list")"
    if [[ ! -f $pack ]]; then
        missing=""
        for t in $(sed -n '/^  "tracks"/,/^  "shared"/p' required/base.json | grep -o '"id": "[^"]*"' | cut -d'"' -f4 | sort -u); do
            grep -qE "^--track $t( |$)| refused: $t[ :]" <<< "$list" || missing+=" $t"
        done
        echo "Kunos base-game tracks missing:${missing:- none}"
    fi
} > "$o/tracks.txt"
cat "$o/tracks.txt"

say "3/8 the launcher: choose the game files if it asks, then Drive a short race (any car and track), drive a bit, quit the race and close the launcher"
./rustyac-launcher --log "$o/launcher_full.log" > "$o/launcher.txt" 2>&1
echo "exit $?" >> "$o/launcher.txt"
# the requests without their values (they can hold your driver name)
sed -E 's/\?[^ ]*//; s/ POST .* ->/ POST ->/' "$o/launcher_full.log" > "$o/launcher.log" 2> /dev/null
rm -f "$o/launcher_full.log"
ask "Did the launcher show Assetto Corsa's menu?"
ask "Did Drive start the race?"

say "4/8 the frame check (off screen, about a minute)"
run frame 600 ./rustyac "${drive[@]}" --autodrive --at 8 --screenshot "$o/frame_laguna.png"
tail -3 "$o/frame.txt"

say "5/8 frame times and sound: a window, the AI drives 60 s; listen"
run window 300 ./rustyac "${drive[@]}" --autodrive --windowed --duration 60
grep -E '^render:' "$o/window.txt"
ask "Did you hear the engine and the tyres, without crackling?"
ask "Did the picture look right (the car, the track, the sky, the HUD)?"

say "6/8 the devices"
run devices 60 ./rustyac --list-devices
cat "$o/devices.txt"

say "7/8 drive yourself (keyboard arrows, or your pad / wheel): 30 s or so, then close the window with its close button"
./rustyac "${drive[@]}" --windowed --ffb > "$o/hand.txt" 2>&1 &
game=$!
# the hang this checks: the end report printed, then the process does not end
while kill -0 "$game" 2> /dev/null && ! grep -q '^render:' "$o/hand.txt"; do sleep 1; done
for _ in $(seq 20); do kill -0 "$game" 2> /dev/null || break; sleep 1; done
if kill -0 "$game" 2> /dev/null; then
    echo "HANG: still running 20 s after its end report" | tee -a "$o/hand.txt"
    kill -9 "$game"
fi
wait "$game" 2> /dev/null
echo "exit $?" >> "$o/hand.txt"
ask "Did steering, throttle and brakes work?"
if grep -qE '^  (Xbox pad on XInput slot|JOY=)' "$o/devices.txt"; then
    ask "Did the pad / wheel drive the car?"
    ask "With a wheel: did force feedback pull the wheel back to the centre (not away from it)?"
fi

say "8/8 packing"
for f in "$o"/*.txt "$o"/*.log; do
    [[ -f $f ]] && sed -i -e "s#$HOME#~#g" -e "s#\b$USER\b#<user>#g" -e "s#\b$(hostname)\b#<host>#g" "$f"
done
tar -czf "$o.tar.gz" "$o" && rm -rf "$o"
echo "Done: send $(pwd)/$o.tar.gz back. It holds:"
tar -tzf "$o.tar.gz" | sed 's/^/  /'
