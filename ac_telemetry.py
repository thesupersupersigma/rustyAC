"""
ac_telemetry.py - log Assetto Corsa physics telemetry.  (v4)

Reads AC's documented shared memory (Local\\acpmf_physics, Local\\acpmf_graphics and
Local\\acpmf_static) READ-ONLY. It never creates the mappings, so it can't break AC's own
shared memory.

Easiest use (start this first, then load AC; it waits for you):
    python ac_telemetry.py --laps 3 --print --out e30_magione_ai.csv

Raw mode (new in v4): every packet keeps the WHOLE physics and graphics pages, bit for bit,
nothing rounded, plus the static page once:
    python ac_telemetry.py --raw --laps 3 --print --out f2004_spa.acraw
    python ac_telemetry.py --to-csv f2004_spa.acraw            (writes f2004_spa.csv)

Flags:
    --laps N        stop by itself after N completed laps (recommended)
    --duration S    safety cap in seconds (default 900 = 15 min, 0 = no cap)
    --out FILE      output name (default: ac_telemetry_YYYYmmdd_HHMMSS.csv / .acraw)
    --print         live readout in the terminal
    --raw           record the raw binary format instead of the rounded CSV
    --to-csv FILE   convert a raw recording to CSV with full-precision values
                    (also writes <out>.static.csv with the static page); no game needed

Only records while the session is LIVE (not paused, not in a replay or menu).
Wheel order in columns: fl, fr, rl, rr. No dependencies (Python 3.8+).

Raw file layout (little endian):
    header   "ACTLMRAW", u32 version (4), u32 header size, u32 physics page size (592),
             u32 graphics page size (304), u32 static page size (684), u32 record size,
             f64 start time (Unix seconds), then the static page
    records  f64 t (seconds of live driving), physics page, graphics page
Field layouts of the three pages are in docs/map/telemetry.md (from acs.pdb).
"""
import argparse
import csv
import ctypes
import os
import struct
import sys
import time
from datetime import datetime

F = ctypes.c_float
I = ctypes.c_int
W = ctypes.c_uint16  # Windows wchar_t is 2 bytes


class Physics(ctypes.Structure):
    """Kunos's SPageFilePhysics, whole struct (0x250 bytes)."""
    _pack_ = 4
    _fields_ = [
        ("packetId", I),
        ("gas", F), ("brake", F), ("fuel", F),
        ("gear", I), ("rpms", I),
        ("steerAngle", F), ("speedKmh", F),
        ("velocity", F * 3), ("accG", F * 3),
        ("wheelSlip", F * 4), ("wheelLoad", F * 4),
        ("wheelsPressure", F * 4), ("wheelAngularSpeed", F * 4),
        ("tyreWear", F * 4), ("tyreDirtyLevel", F * 4),
        ("tyreCoreTemperature", F * 4), ("camberRAD", F * 4),
        ("suspensionTravel", F * 4),
        ("drs", F), ("tc", F),
        ("heading", F), ("pitch", F), ("roll", F), ("cgHeight", F),
        ("carDamage", F * 5),
        ("numberOfTyresOut", I), ("pitLimiterOn", I),
        ("abs", F),
        ("kersCharge", F), ("kersInput", F),
        ("autoShifterOn", I),
        ("rideHeight", F * 2),
        ("turboBoost", F), ("ballast", F), ("airDensity", F),
        ("airTemp", F), ("roadTemp", F),
        ("localAngularVel", F * 3),
        ("finalFF", F),
        # --- v3 stopped here; the rest of the page follows ---
        ("performanceMeter", F),
        ("engineBrake", I),
        ("ersRecoveryLevel", I), ("ersPowerLevel", I),
        ("ersHeatCharging", I), ("ersIsCharging", I),
        ("kersCurrentKJ", F),
        ("drsAvailable", I), ("drsEnabled", I),
        ("brakeTemp", F * 4),
        ("clutch", F),
        ("tyreTempI", F * 4), ("tyreTempM", F * 4), ("tyreTempO", F * 4),
        ("isAIControlled", I),
        ("tyreContactPoint", (F * 3) * 4),
        ("tyreContactNormal", (F * 3) * 4),
        ("tyreContactHeading", (F * 3) * 4),
        ("brakeBias", F),
        ("localVelocity", F * 3),
        ("P2PActivations", I), ("P2PStatus", I),
        ("currentMaxRpm", I),
    ]


class Graphics(ctypes.Structure):
    """Kunos's SPageFileGraphic, whole struct (0x130 bytes)."""
    _pack_ = 4
    _fields_ = [
        ("packetId", I),
        ("status", I),          # 0 off, 1 replay, 2 live, 3 pause
        ("session", I),
        ("currentTime", W * 15), ("lastTime", W * 15),
        ("bestTime", W * 15), ("split", W * 15),
        ("completedLaps", I),
        ("position", I),
        ("iCurrentTime", I), ("iLastTime", I), ("iBestTime", I),
        ("sessionTimeLeft", F),
        ("distanceTraveled", F),
        ("isInPit", I),
        ("currentSectorIndex", I),
        ("lastSectorTime", I),
        ("numberOfLaps", I),
        ("tyreCompound", W * 33),
        ("replayTimeMultiplier", F),
        ("normalizedCarPosition", F),
        ("carCoordinates", F * 3),
        # --- v3 stopped here ---
        ("penaltyTime", F),
        ("flag", I),
        ("idealLineOn", I),
        ("isInPitLane", I),
        ("surfaceGrip", F),
        ("mandatoryPitDone", I),
        ("windSpeed", F), ("windDirection", F),
        ("timeLimitSessionLeft", I),
        ("isEscMenuVisible", I),
    ]


class Static(ctypes.Structure):
    """Kunos's SPageFileStatic, whole struct (0x2ac bytes)."""
    _pack_ = 4
    _fields_ = [
        ("smVersion", W * 15), ("acVersion", W * 15),
        ("numberOfSessions", I), ("numCars", I),
        ("carModel", W * 33), ("track", W * 33),
        ("playerName", W * 33), ("playerSurname", W * 33), ("playerNick", W * 33),
        ("sectorCount", I),
        ("maxTorque", F), ("maxPower", F),
        ("maxRpm", I),
        ("maxFuel", F),
        ("suspensionMaxTravel", F * 4), ("tyreRadius", F * 4),
        ("maxTurboBoost", F),
        ("deprecated_1", F), ("deprecated_2", F),
        ("penaltiesEnabled", I),
        ("aidFuelRate", F), ("aidTireRate", F), ("aidMechanicalDamage", F),
        ("aidAllowTyreBlankets", I),
        ("aidStability", F),
        ("aidAutoClutch", I), ("aidAutoBlip", I),
        ("hasDRS", I), ("hasERS", I), ("hasKERS", I),
        ("kersMaxJ", F),
        ("engineBrakeSettingsCount", I), ("ersPowerControllerCount", I),
        ("trackSPlineLength", F),
        ("trackConfiguration", W * 33),
        ("ersMaxJ", F),
        ("isTimedRace", I), ("hasExtraLap", I),
        ("carSkin", W * 33),
        ("reversedGridPositions", I),
        ("PitWindowStart", I), ("PitWindowEnd", I),
    ]


AC_LIVE = 2
PSIZE = ctypes.sizeof(Physics)
GSIZE = ctypes.sizeof(Graphics)
SSIZE = ctypes.sizeof(Static)
assert (PSIZE, GSIZE, SSIZE) == (0x250, 0x130, 0x2ac), "page structs do not match acs.pdb"
WHEELS = ["fl", "fr", "rl", "rr"]
AXES = ["x", "y", "z"]
# the rounded CSV keeps exactly the v3 columns: the physics page up to this field ...
CSV_LAST_PHYSICS_FIELD = "finalFF"
# ... and these graphics fields in every row
G_COLS = [("completedLaps", "lap"), ("iCurrentTime", "lapTimeMs"),
          ("iLastTime", "lastLapMs"), ("normalizedCarPosition", "trackPos")]

RAW_MAGIC = b"ACTLMRAW"
RAW_VERSION = 4
RAW_HEADER = struct.Struct("<8sIIIIIId")   # magic, version, header size, P, G, S, record, start
RAW_RECORD_HEAD = struct.Struct("<d")      # t


def csv_physics_fields():
    out = []
    for name, typ in Physics._fields_:
        out.append((name, typ))
        if name == CSV_LAST_PHYSICS_FIELD:
            return out
    raise AssertionError("CSV_LAST_PHYSICS_FIELD is not a physics field")


def flat_names(name, typ, prefix=""):
    """Column names of one struct field (arrays are spread out)."""
    n = getattr(typ, "_length_", None)
    if n is None:
        return [prefix + name]
    inner = typ._type_
    if hasattr(inner, "_length_"):               # [4][3]: per wheel, per axis
        return [f"{prefix}{name}_{w}_{a}" for w in WHEELS for a in AXES]
    if inner is W:                               # a string: one column
        return [prefix + name]
    if n == 4:
        return [f"{prefix}{name}_{w}" for w in WHEELS]
    if n == 3:
        return [f"{prefix}{name}_{a}" for a in AXES]
    return [f"{prefix}{name}_{i}" for i in range(n)]


def columns():
    cols = ["t"] + [alias for _, alias in G_COLS] + ["posX", "posY", "posZ"]
    for name, typ in csv_physics_fields():
        cols += flat_names(name, typ)
    return cols


def row(p, g, t):
    out = [f"{t:.4f}"]
    for field, _ in G_COLS:
        v = getattr(g, field)
        out.append(round(v, 6) if isinstance(v, float) else v)
    out += [round(x, 4) for x in g.carCoordinates]
    for name, typ in csv_physics_fields():
        v = getattr(p, name)
        if hasattr(typ, "_length_"):
            out += [round(x, 5) for x in v]
        elif isinstance(v, float):
            out.append(round(v, 5))
        else:
            out.append(v)
    return out


# ---------------------------------------------------------------- raw recording -> CSV

def exact(v):
    """Text that reads back to the same 32-bit float (ints and strings pass through)."""
    return f"{v:.9g}" if isinstance(v, float) else v


def flat_values(struct_obj, name, typ):
    v = getattr(struct_obj, name)
    if not hasattr(typ, "_length_"):
        return [exact(v)]
    inner = typ._type_
    if hasattr(inner, "_length_"):
        return [exact(x) for wheel in v for x in wheel]
    if inner is W:
        text = "".join(chr(c) for c in v)
        return [text.split("\0", 1)[0]]
    return [exact(x) for x in v]


def full_columns():
    """The v3 columns first (so check_telemetry.py reads the file), then everything else."""
    cols = columns()
    seen = set(cols)
    past = False
    for name, typ in Physics._fields_:
        if past:
            cols += flat_names(name, typ)
        past = past or name == CSV_LAST_PHYSICS_FIELD
    used = {field for field, _ in G_COLS} | {"carCoordinates"}
    for name, typ in Graphics._fields_:
        if name not in used:
            cols += flat_names(name, typ, "g_")
    assert len(set(cols)) == len(cols) and seen <= set(cols)
    return cols


def full_row(p, g, t):
    out = [f"{t:.6f}"]
    out += [exact(getattr(g, field)) for field, _ in G_COLS]
    out += [exact(x) for x in g.carCoordinates]
    for name, typ in Physics._fields_:
        out += flat_values(p, name, typ)
    used = {field for field, _ in G_COLS} | {"carCoordinates"}
    for name, typ in Graphics._fields_:
        if name not in used:
            out += flat_values(g, name, typ)
    return out


def read_raw_header(f):
    head = f.read(RAW_HEADER.size)
    if len(head) < RAW_HEADER.size:
        sys.exit("not a raw telemetry file (too short)")
    magic, version, header_size, psize, gsize, ssize, record, start = RAW_HEADER.unpack(head)
    if magic != RAW_MAGIC:
        sys.exit("not a raw telemetry file (bad magic)")
    if version != RAW_VERSION or (psize, gsize, ssize) != (PSIZE, GSIZE, SSIZE):
        sys.exit(f"raw file version {version} with page sizes {psize}/{gsize}/{ssize} "
                 f"is not what this script (v{RAW_VERSION}) writes")
    static = Static.from_buffer_copy(f.read(ssize))
    f.seek(header_size)
    return static, record, start


def to_csv(raw_path, out):
    out = out or os.path.splitext(raw_path)[0] + ".csv"
    count = 0
    with open(raw_path, "rb") as f, open(out, "w", newline="") as o:
        static, record, start = read_raw_header(f)
        w = csv.writer(o)
        w.writerow(full_columns())
        while True:
            rec = f.read(record)
            if len(rec) < record:
                break
            (t,) = RAW_RECORD_HEAD.unpack_from(rec, 0)
            p = Physics.from_buffer_copy(rec, RAW_RECORD_HEAD.size)
            g = Graphics.from_buffer_copy(rec, RAW_RECORD_HEAD.size + PSIZE)
            w.writerow(full_row(p, g, t))
            count += 1
    static_out = os.path.splitext(out)[0] + ".static.csv"
    with open(static_out, "w", newline="") as o:
        w = csv.writer(o)
        w.writerow(["field", "value"])
        w.writerow(["recordingStartUnix", f"{start:.3f}"])
        for name, typ in Static._fields_:
            for col, value in zip(flat_names(name, typ), flat_values(static, name, typ)):
                w.writerow([col, value])
    print(f"{count} packets -> {out}\nstatic page -> {static_out}")


# ---------------------------------------------------------------- live recording

def open_views():
    """Map AC's three blocks read-only. Returns None until AC has created them."""
    k32 = ctypes.WinDLL("kernel32", use_last_error=True)
    k32.OpenFileMappingW.restype = ctypes.c_void_p
    k32.OpenFileMappingW.argtypes = [ctypes.c_uint32, ctypes.c_int, ctypes.c_wchar_p]
    k32.MapViewOfFile.restype = ctypes.c_void_p
    k32.MapViewOfFile.argtypes = [ctypes.c_void_p, ctypes.c_uint32, ctypes.c_uint32,
                                  ctypes.c_uint32, ctypes.c_size_t]
    FILE_MAP_READ = 0x0004

    def open_view(name):
        handle = k32.OpenFileMappingW(FILE_MAP_READ, 0, name)
        if not handle:
            return None
        # size 0 = the whole mapping. AC creates all three with the physics page's size, which
        # Windows rounds up to one 4096-byte page, so the longer static struct is readable too.
        return k32.MapViewOfFile(handle, FILE_MAP_READ, 0, 0, 0) or None

    views = [open_view("Local\\acpmf_" + n) for n in ("physics", "graphics", "static")]
    return views if all(views) else None


def wait_for_ac():
    warned = False
    while True:
        views = open_views()
        if views:
            return views
        if not warned:
            print("Waiting for Assetto Corsa... (start a session and get on track)")
            warned = True
        time.sleep(1)


def page(view, size):
    """One page, read twice: the game copies it without a lock, so a single read can be torn."""
    a = ctypes.string_at(view, size)
    for _ in range(4):
        b = ctypes.string_at(view, size)
        if a == b:
            break
        a = b
    return a


def record(args):
    ext = "acraw" if args.raw else "csv"
    out = args.out or f"ac_telemetry_{datetime.now():%Y%m%d_%H%M%S}.{ext}"
    pv, gv, sv = wait_for_ac()
    print(f"Connected. Logging to {out}" + (" (raw pages)" if args.raw else ""))
    if args.laps:
        print(f"Will stop by itself after {args.laps} completed lap(s).")
    print("Press Ctrl+C in THIS window to stop early "
          "(Ctrl+C inside the game toggles the AI driver instead).")

    last_id = None
    count = 0
    checked = False
    start_lap = None
    stop_at = None
    live_time = 0.0
    last_tick = None
    reason = "stopped"
    record_size = RAW_RECORD_HEAD.size + PSIZE + GSIZE
    header_size = RAW_HEADER.size + SSIZE

    with open(out, "wb" if args.raw else "w", **({} if args.raw else {"newline": ""})) as f:
        if args.raw:
            f.write(b"\0" * header_size)      # filled in once the session is live
            header_done = False
        else:
            w = csv.writer(f)
            w.writerow(columns())
        try:
            while True:
                gbytes = page(gv, GSIZE)
                g = Graphics.from_buffer_copy(gbytes)
                now = time.perf_counter()
                if g.status != AC_LIVE:
                    last_tick = None          # paused / menu / replay: don't count time
                    time.sleep(0.05)
                    continue
                if last_tick is not None:
                    live_time += now - last_tick
                last_tick = now

                if args.duration and live_time >= args.duration:
                    reason = f"hit --duration {args.duration:.0f}s"
                    break
                if start_lap is None:
                    start_lap = g.completedLaps
                if args.laps and stop_at is None and g.completedLaps - start_lap >= args.laps:
                    stop_at = live_time + 2.0   # keep 2 s so the last lap time lands in the file
                if stop_at is not None and live_time >= stop_at:
                    reason = f"finished {args.laps} lap(s)"
                    break

                pbytes = page(pv, PSIZE)
                p = Physics.from_buffer_copy(pbytes)
                if p.packetId == last_id or p.packetId == 0:
                    time.sleep(0.0005)       # no new physics step yet
                    continue
                last_id = p.packetId
                if args.raw:
                    if not header_done:
                        header_done = True
                        f.seek(0)
                        f.write(RAW_HEADER.pack(RAW_MAGIC, RAW_VERSION, header_size, PSIZE,
                                                GSIZE, SSIZE, record_size, time.time()))
                        f.write(page(sv, SSIZE))
                        f.seek(0, os.SEEK_END)
                    f.write(RAW_RECORD_HEAD.pack(live_time))
                    f.write(pbytes)
                    f.write(gbytes)
                else:
                    w.writerow(row(p, g, live_time))
                count += 1

                if count == 200 and not checked:
                    checked = True
                    ok_p = 0 <= p.speedKmh < 600 and -1 <= p.gear <= 10 and 0 <= p.rpms < 30000
                    ok_g = 0 <= g.completedLaps < 10000 and 0 <= g.normalizedCarPosition <= 1.01
                    if not (ok_p and ok_g):
                        print("\nWARNING: values look wrong - struct layout may not match "
                              "your AC version. Check the output before trusting it.")
                if count % 500 == 0:
                    f.flush()
                if args.print and count % 33 == 0:
                    print(f"\r{live_time:7.1f}s  lap {g.completedLaps - start_lap + 1}  "
                          f"{p.speedKmh:6.1f} km/h  gear {p.gear - 1:>2}  {p.rpms:5d} rpm  "
                          f"gas {p.gas:4.2f}  brake {p.brake:4.2f}  rows {count}   ",
                          end="", flush=True)
        except KeyboardInterrupt:
            reason = "Ctrl+C"

    rate = count / live_time if live_time else 0
    print(f"\nDone ({reason}): {count} rows in {live_time:.1f}s live (~{rate:.0f} Hz) -> {out}")
    if args.raw:
        print(f"Convert with: python ac_telemetry.py --to-csv {out}")


def main():
    ap = argparse.ArgumentParser(description="Log Assetto Corsa physics telemetry (v4).")
    ap.add_argument("--out", help="output path (default: ac_telemetry_YYYYmmdd_HHMMSS.csv, "
                                  ".acraw with --raw, or the raw file's name with --to-csv)")
    ap.add_argument("--laps", type=int, default=0, help="stop after N completed laps (0 = off)")
    ap.add_argument("--duration", type=float, default=900,
                    help="safety cap in seconds of live driving (0 = no cap, default 900)")
    ap.add_argument("--print", action="store_true", help="show a live readout while logging")
    ap.add_argument("--raw", action="store_true",
                    help="store the whole physics and graphics pages of every packet, unrounded "
                         "(binary), plus the static page once")
    ap.add_argument("--to-csv", metavar="RAWFILE",
                    help="convert a --raw recording to CSV with full-precision values and exit")
    args = ap.parse_args()

    if args.to_csv:
        to_csv(args.to_csv, args.out)
        return
    if sys.platform != "win32":
        sys.exit("Recording only runs on Windows (AC's shared memory is Windows-only).")
    record(args)


if __name__ == "__main__":
    main()
