# SPDX-License-Identifier: GPL-3.0-or-later
# csp_words.py <vanilla car_words.txt> <csp car_words.txt> <acd_check --csp --pressures line>: the
# game's car (Car, 0x3ea0 bytes, 2,004 words) built by csp_oracle without and with CSP, word by word.
# Every word must be the same, apart from the words CSP itself writes (docs/csp/port_c3.md, "The
# car's memory"); of those, the ones rustyAC models must hold rustyAC's CSP-mode values: each tyre's
# static and dynamic pressure (from acd_check --csp --pressures), and Car+0x278
# (transponder.isFirstLapArmed) 0 without CSP and 1 with it, as rustyAC's CSP mode arms it for the
# oracle's race.ini (SPAWN_SET=HOTLAP_START, TYPE=1). Prints one line: IDENTICAL or DIFFERENT.
import sys

TYRES, TYRE_SIZE = 0xB28, 0x858
# word offsets (8-byte aligned) CSP writes, with what they are
CSP_ONLY = {
    0x278: "isFirstLapArmed (TimeTransponder::init detour; rustyAC's arm rule, checked below)",
    0x2D8: "colliderManager observer lastChanged = -1 (FileChangeObserver::observe detour; no file watcher in rustyAC)",
    0x3630: "screenName length (CSP's SCREEN_NAME is ui_car.json's name; the physics car has no name)",
    0x3638: "screenName capacity (same)",
    0x3DB0: "padding after Car+0x3db0: CSP's extension flag at +0x3db4",
    0x3E28: "padding at +0x3e2d (CSP zeroes it)",
    0xAE8: "AeroMap padding at +0xaec (CSP zeroes it)",
}
TYRE_ONLY = {
    0x340: "pressureStatic at +0x344 (Tyre::reset detour: the default compound's; checked against rustyAC)",
    0x348: "pressureDynamic (same)",
    0x408: "padding +0x40c: the default compound's pressure (Tyre::initCompounds detour)",
    0x4E0: "padding +0x4e1: tyre index (TyreThermalModel)",
    0x648: "padding +0x64e: CSP's compound index",
    0x828: "padding +0x829 (SCTM)",
}


def load(path):
    return [line.split() for line in open(path) if line.strip()]


def why(offset):
    if offset in CSP_ONLY:
        return CSP_ONLY[offset]
    if TYRES <= offset < TYRES + 4 * TYRE_SIZE:
        return TYRE_ONLY.get((offset - TYRES) % TYRE_SIZE)
    return None


def main():
    vanilla, csp = load(sys.argv[1]), load(sys.argv[2])
    if len(vanilla) != 2004 or len(csp) != 2004:
        print(f"DIFFERENT | {len(vanilla)} / {len(csp)} words, not 2004")
        return
    pointers = sum(1 for w in vanilla if w[1] == "ptr")
    differ = [(int(a[0], 16), a[1], b[1]) for a, b in zip(vanilla, csp) if a[1] != b[1]]
    unexplained = [f"{o:#x} {a} {b}" for o, a, b in differ if why(o) is None]
    words = {int(w[0], 16): w[1] for w in csp}
    armed = {int(w[0], 16): w[1] for w in vanilla}.get(0x278), words.get(0x278)
    # the game's pressures with CSP, as acd_check prints rustyAC's: static, dynamic per tyre
    game = []
    for i in range(4):
        tyre = TYRES + i * TYRE_SIZE
        game += [words[tyre + 0x340][:8], words[tyre + 0x348][8:]]
    port = sys.argv[3].split()[-8:]
    compared = 2004 - pointers
    if unexplained:
        print(f"DIFFERENT | {len(unexplained)} words CSP changed that are not CSP's own: {', '.join(unexplained[:4])}")
    elif game != port:
        print(f"DIFFERENT | pressures with CSP: game {' '.join(game)}, rustyAC --csp {' '.join(port)}")
    elif int(armed[0], 16) & 0xFF != 0 or int(armed[1], 16) & 0xFF != 1:
        print(f"DIFFERENT | isFirstLapArmed {armed[0]} / {armed[1]}, rustyAC's CSP mode arms it (1) and Vanilla not (0)")
    else:
        print(f"IDENTICAL | {compared} words compared ({pointers} pointers left out), {len(differ)} are CSP's own, pressures as rustyAC's")


if __name__ == "__main__":
    main()
