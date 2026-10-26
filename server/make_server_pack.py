#!/usr/bin/env python3
# SPDX-License-Identifier: MIT OR Apache-2.0
"""rustyAC server kit: copies what Kunos' acServer needs out of YOUR Assetto Corsa install
into a git-ignored folder (dist-server/ by default), next to this kit's config files.

    python server/make_server_pack.py [--ac <Assetto Corsa folder>] [--out dist-server]
                                      [--server spa --server laguna_seca] [--windows]
                                      [--all] [--car <car>] [--track <track>[/<layout>]] [--no-check]

Without --all / --car / --track the pack holds the kit's fixed servers' cars and tracks.
--all adds every car and track layout that both plain acServer and rustyAC can run; --car and
--track add single ones to the pack that is already there. What cannot run is left out with
its reason: a DLC that is not owned (no data.acd, no models file), a car or layout made for
Custom Shaders Patch only (the check is tools/web_pack's, the browser pack's own, run with
--dry-run; --no-check skips it).

What is copied (all of it Kunos' and the mod makers' files: never commit the result, never
publish it):
  acServer                      the Linux server (a static 32-bit Go program); with --windows
                                also acServer.exe
  system/data/surfaces.ini      the common surfaces
  content/cars/<car>/data.acd   the server checks the players' car data against it
  content/tracks/<track>[/<layout>]/data/surfaces.ini, drs_zones.ini and models[_<layout>].ini
  content/weather/<id>/weather.ini   the weathers the panel offers
  index.json                    for the panel: each car's menu name and skin folder names, each
                                layout's menu name and pit boxes, the weathers, what was left out
and from the kit itself:
  cfg/<name>/server_cfg.ini, entry_list.ini      one folder per server

acServer reads no AI line (fast_lane.ai), so none is copied.
Nothing is written anywhere else. The game's folder is only read.
"""

import argparse
import configparser
import json
import os
import re
import shutil
import subprocess
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


def ui_field(path, field):
    """One field of a ui_*.json; read with a pattern because many of them are not valid JSON."""
    try:
        with open(path, encoding="utf-8", errors="replace") as handle:
            found = re.search(rf'"{field}"\s*:\s*"?([^",}}\r\n]*)', handle.read())
    except OSError:
        return ""
    return found.group(1).strip() if found else ""


def dirs(path):
    return sorted(d for d in os.listdir(path) if os.path.isdir(os.path.join(path, d))) if os.path.isdir(path) else []


def split(label):
    track, _, layout = label.partition("/")
    return track, layout


def track_files(ac, label):
    """The paths of what acServer reads of a layout; drs_zones.ini and the models file only when
    there is one (a track of one .kn5 has no models.ini)."""
    track, layout = split(label)
    rel = os.path.join("content", "tracks", track, layout) if layout else os.path.join("content", "tracks", track)
    models = os.path.join("content", "tracks", track, f"models_{layout}.ini" if layout else "models.ini")
    optional = [os.path.join(rel, "data", "drs_zones.ini"), models]
    return [os.path.join(rel, "data", "surfaces.ini")] + [f for f in optional if os.path.isfile(os.path.join(ac, f))]


def installed(ac):
    """Every car folder and every track and layout (a folder with data/surfaces.ini)."""
    cars = dirs(os.path.join(ac, "content", "cars"))
    tracks = []
    for track in dirs(os.path.join(ac, "content", "tracks")):
        base = os.path.join(ac, "content", "tracks", track)
        if os.path.isfile(os.path.join(base, "data", "surfaces.ini")):
            tracks.append(track)
        tracks += [f"{track}/{l}" for l in dirs(base) if os.path.isfile(os.path.join(base, l, "data", "surfaces.ini"))]
    return cars, tracks


def rustyac_check(ac, out, cars, tracks, spare):
    """web_pack --dry-run's verdict: what of `cars` / `tracks` rustyAC runs, and why the rest not.
    web_pack wants at least one of each: an empty list is asked for with `spare` (a fixed server's)."""
    conf = os.path.join(out, "check.toml")
    os.makedirs(out, exist_ok=True)
    with open(conf, "w", encoding="utf-8") as handle:
        handle.write(f"cars = {json.dumps(cars or spare[:1])}\ntracks = {json.dumps(tracks or spare[1:])}\n")
    print(f"checking {len(cars)} cars and {len(tracks)} tracks and layouts with web_pack --dry-run (a few minutes for all)")
    try:
        run = subprocess.run(
            ["cargo", "run", "--release", "--locked", "--quiet", "--manifest-path", os.path.join(REPO, "tools", "web_pack", "Cargo.toml"), "--",
             "--config", conf, "--ac", ac, "--out", os.path.join(out, "web-check"), "--dry-run"],
            capture_output=True, text=True, encoding="utf-8", errors="replace")
    finally:
        os.remove(conf)
    if run.returncode != 0:
        sys.exit(f"web_pack failed (--no-check skips this check):\n{run.stdout}{run.stderr}")
    kept, skipped = set(), []
    for line in run.stdout.splitlines():
        if line.startswith(("car ", "track ")):
            kept.add(line.split()[1].lower())
        elif line.startswith(("  car ", "  track ")):
            kind, rest = line.strip().split(" ", 1)
            names, why = rest.split(": ", 1)
            skipped += [{"kind": kind, "id": n, "reason": why} for n in names.split(", ")]
    return [c for c in cars if c.lower() in kept], [t for t in tracks if t.lower() in kept], skipped


def weather_name(wid):
    """3_clear -> Clear, 1_heavy_fog -> Heavy fog: AC's weather folders have no name of their own."""
    words = wid.split("_")
    if len(words) > 1 and words[0].isdigit():
        words = words[1:]
    return " ".join(words).capitalize()


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--ac", default=os.environ.get("AC_ROOT", USUAL), help="the Assetto Corsa folder")
    ap.add_argument("--out", default=os.path.join(REPO, "dist-server"), help="where the pack goes")
    ap.add_argument("--server", action="append", help="a kit folder name (default: every one)")
    ap.add_argument("--windows", action="store_true", help="also copy acServer.exe")
    ap.add_argument("--all", action="store_true", help="every car and track layout acServer and rustyAC can run")
    ap.add_argument("--car", action="append", default=[], help="add this car")
    ap.add_argument("--track", action="append", default=[], help="add this track or track/layout")
    ap.add_argument("--no-check", action="store_true", help="skip rustyAC's check (web_pack --dry-run)")
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

    # the fixed servers' content: must all be there
    cars, tracks = set(), set()
    for name in names:
        kit = os.path.join(KIT, name)
        cfg = ini(os.path.join(kit, "server_cfg.ini"))
        entries = ini(os.path.join(kit, "entry_list.ini"))
        for file in ("server_cfg.ini", "entry_list.ini"):
            copy(os.path.join(kit, file), os.path.join(out, "cfg", name, file), missing)
        cars |= {entries[s]["MODEL"] for s in entries.sections() if s.startswith("CAR_")}
        cars |= {c for c in cfg["SERVER"].get("CARS", "").split(";") if c}
        layout = cfg["SERVER"].get("CONFIG_TRACK", "")
        tracks.add(cfg["SERVER"]["TRACK"] + (f"/{layout}" if layout else ""))

    # what --all / --car / --track ask for: left out with a reason when it cannot run
    every_car, every_track = installed(ac)
    want_cars = [c for c in (every_car if args.all else args.car) if c not in cars]
    want_tracks = [t for t in (every_track if args.all else args.track) if t not in tracks]
    skipped = []
    for car in list(want_cars):
        if not os.path.isfile(os.path.join(ac, "content", "cars", car, "data.acd")):
            want_cars.remove(car)
            skipped.append({"kind": "car", "id": car, "reason": "no data.acd (a DLC that is not owned, or a mod without its data): acServer checks the players' cars against it"})
    for label in list(want_tracks):
        lost = [f for f in track_files(ac, label) if not os.path.isfile(os.path.join(ac, f))]
        if lost:
            want_tracks.remove(label)
            skipped.append({"kind": "track", "id": label, "reason": "not found: no data/surfaces.ini in the install"})
    if (want_cars or want_tracks) and not args.no_check:
        want_cars, want_tracks, refused = rustyac_check(ac, out, want_cars, want_tracks, [min(cars), min(tracks)])
        skipped += refused
    cars |= set(want_cars)
    tracks |= set(want_tracks)

    for car in sorted(cars):
        copy(os.path.join(ac, "content", "cars", car, "data.acd"), os.path.join(out, "content", "cars", car, "data.acd"), missing)
    for label in sorted(tracks):
        for rel in track_files(ac, label):
            copy(os.path.join(ac, rel), os.path.join(out, rel), missing)
    weathers = [w for w in dirs(os.path.join(ac, "content", "weather")) if os.path.isfile(os.path.join(ac, "content", "weather", w, "weather.ini"))]
    for w in weathers:
        copy(os.path.join(ac, "content", "weather", w, "weather.ini"), os.path.join(out, "content", "weather", w, "weather.ini"), missing)
    if missing:
        print("MISSING (not copied):")
        for path in missing:
            print("  ", path)
        sys.exit(1)

    # the index: what is in the pack now, the earlier runs' content kept (--car / --track add)
    path = os.path.join(out, "index.json")
    old = {"cars": [], "tracks": [], "skipped": []}
    if os.path.isfile(path):
        with open(path, encoding="utf-8") as handle:
            old = json.load(handle)
    index_cars = {c["model"]: c for c in old["cars"] if os.path.isfile(os.path.join(out, "content", "cars", c["model"], "data.acd"))}
    for car in cars:
        folder = os.path.join(ac, "content", "cars", car)
        index_cars[car] = {"model": car, "name": ui_field(os.path.join(folder, "ui", "ui_car.json"), "name") or car, "skins": dirs(os.path.join(folder, "skins"))}
    index_tracks = {(t["track"], t["layout"]): t for t in old["tracks"]}
    for label in tracks:
        track, layout = split(label)
        ui = os.path.join(ac, "content", "tracks", track, "ui", layout, "ui_track.json")
        pitboxes = ui_field(ui, "pitboxes")
        index_tracks[(track, layout)] = {"track": track, "layout": layout, "name": ui_field(ui, "name") or label,
                                         "pitboxes": int(pitboxes) if pitboxes.isdigit() else 0}
    packed = {("car", c) for c in index_cars} | {("track", t + (f"/{l}" if l else "")) for t, l in index_tracks}
    asked = {("car", s["id"]) for s in skipped if s["kind"] == "car"} | {("track", s["id"]) for s in skipped if s["kind"] == "track"}
    skipped += [s for s in old["skipped"] if (s["kind"], s["id"]) not in packed | asked]
    index = {
        "cars": sorted(index_cars.values(), key=lambda c: c["name"].lower()),
        "tracks": sorted(index_tracks.values(), key=lambda t: t["name"].lower()),
        "weather": [{"id": w, "name": weather_name(w)} for w in weathers],
        "skipped": sorted(skipped, key=lambda s: (s["kind"], s["id"])),
    }
    with open(path, "w", encoding="utf-8") as handle:
        json.dump(index, handle, indent=1)

    size = sum(os.path.getsize(os.path.join(d, f)) for d, _, files in os.walk(out) for f in files)
    print(f"{len(index['cars'])} cars, {len(index['tracks'])} tracks and layouts, {len(weathers)} weathers: {size / 1e6:.1f} MB")
    if index["skipped"]:
        print(f"left out ({len(index['skipped'])}):")
        for s in index["skipped"]:
            print(f"  {s['kind']} {s['id']}: {s['reason']}")
    print("done. This folder holds Kunos' files: keep it off git and off public servers' web roots.")
    # (scp -r from Windows leaves the folders drwx------, and acServer in the image runs as uid 999)
    print("after copying it to the server, there: chmod -R u=rwX,go=rX <the pack folder>")


if __name__ == "__main__":
    main()
