#!/usr/bin/env python3
# SPDX-License-Identifier: MIT OR Apache-2.0
"""rustyAC server kit: copies what Kunos' acServer needs out of YOUR Assetto Corsa install
into a git-ignored folder (dist-server/ by default), next to this kit's config files.

    python server/make_server_pack.py [--ac <Assetto Corsa folder>] [--out dist-server]
                                      [--server spa --server laguna_seca] [--windows]

What is copied (all of it Kunos' files: never commit the result, never publish it):
  acServer                      the Linux server (a static 32-bit Go program); with --windows
                                also acServer.exe
  system/data/surfaces.ini      the common surfaces
  content/cars/<car>/data.acd   of every car in a kit's entry list (the server checks the
                                clients' car data against it)
  content/tracks/<track>[/<layout>]/data/surfaces.ini, drs_zones.ini and models.ini
and from the kit itself:
  cfg/<name>/server_cfg.ini, entry_list.ini      one folder per server

Nothing is written anywhere else. The game's folder is only read.
"""

import argparse
import configparser
import os
import shutil
import sys

KIT = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(KIT)
USUAL = r"C:\Program Files (x86)\Steam\steamapps\common\assettocorsa"


def ini(path):
    parser = configparser.ConfigParser(interpolation=None, strict=False, comment_prefixes=(";", "#"))
    parser.optionxform = str
    with open(path, encoding="utf-8") as handle:
        parser.read_file(handle)
    return parser


def copy(src, dst, missing):
    if not os.path.isfile(src):
        missing.append(src)
        return
    os.makedirs(os.path.dirname(dst), exist_ok=True)
    shutil.copyfile(src, dst)
    print("  ", os.path.relpath(dst))


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--ac", default=os.environ.get("AC_ROOT", USUAL), help="the Assetto Corsa folder")
    ap.add_argument("--out", default=os.path.join(REPO, "dist-server"), help="where the pack goes")
    ap.add_argument("--server", action="append", help="a kit folder name (default: every one)")
    ap.add_argument("--windows", action="store_true", help="also copy acServer.exe")
    args = ap.parse_args()

    ac = args.ac
    if not os.path.isfile(os.path.join(ac, "server", "acServer")) and not os.path.isfile(os.path.join(ac, "server", "acServer.exe")):
        sys.exit(f"no acServer in {os.path.join(ac, 'server')}: give the game's folder with --ac")
    out = os.path.abspath(args.out)
    game = os.path.abspath(ac)
    if os.path.commonpath([out, game]) == game:
        sys.exit("the pack must not be written into the game's folder")
    names = args.server or sorted(
        d for d in os.listdir(KIT) if os.path.isfile(os.path.join(KIT, d, "server_cfg.ini"))
    )
    missing = []
    print("server pack ->", out)
    copy(os.path.join(ac, "server", "acServer"), os.path.join(out, "acServer"), missing)
    if args.windows:
        copy(os.path.join(ac, "server", "acServer.exe"), os.path.join(out, "acServer.exe"), missing)
    copy(os.path.join(ac, "server", "system", "data", "surfaces.ini"), os.path.join(out, "system", "data", "surfaces.ini"), missing)

    for name in names:
        kit = os.path.join(KIT, name)
        cfg = ini(os.path.join(kit, "server_cfg.ini"))
        entries = ini(os.path.join(kit, "entry_list.ini"))
        for file in ("server_cfg.ini", "entry_list.ini"):
            copy(os.path.join(kit, file), os.path.join(out, "cfg", name, file), missing)
        cars = {entries[s]["MODEL"] for s in entries.sections() if s.startswith("CAR_")}
        cars |= {c for c in cfg["SERVER"].get("CARS", "").split(";") if c}
        for car in sorted(cars):
            copy(os.path.join(ac, "content", "cars", car, "data.acd"), os.path.join(out, "content", "cars", car, "data.acd"), missing)
        track = cfg["SERVER"]["TRACK"]
        layout = cfg["SERVER"].get("CONFIG_TRACK", "")
        rel = os.path.join("content", "tracks", track, layout) if layout else os.path.join("content", "tracks", track)
        for file in ("surfaces.ini", "drs_zones.ini"):
            src = os.path.join(ac, rel, "data", file)
            if file == "drs_zones.ini" and not os.path.isfile(src):
                continue  # a track without DRS zones
            copy(src, os.path.join(out, rel, "data", file), missing)
        models = "models.ini" if not layout else f"models_{layout}.ini"
        src = os.path.join(ac, "content", "tracks", track, models)
        if os.path.isfile(src):
            copy(src, os.path.join(out, "content", "tracks", track, models), missing)

    if missing:
        print("MISSING (not copied):")
        for path in missing:
            print("  ", path)
        sys.exit(1)
    print("done. This folder holds Kunos' files: keep it off git and off public servers' web roots.")


if __name__ == "__main__":
    main()
