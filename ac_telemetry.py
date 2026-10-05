"""
ac_telemetry.py - log Assetto Corsa physics telemetry to CSV.  (v3)

Reads AC's documented shared memory (Local\\acpmf_physics + Local\\acpmf_graphics)
READ-ONLY. It never creates the mappings, so it can't break AC's own shared memory.

Easiest use (start this first, then load AC; it waits for you):
    python ac_telemetry.py --laps 3 --print --out e30_magione_ai.csv

Other flags:
    --laps N        stop by itself after N completed laps (recommended)
    --duration S    safety cap in seconds (default 900 = 15 min, 0 = no cap)
    --out FILE      CSV name (default: ac_telemetry_YYYYmmdd_HHMMSS.csv)
    --print         live readout in the terminal

Only records while the session is LIVE (not paused, not in a replay or menu).
Wheel order in columns: fl, fr, rl, rr. No dependencies (Python 3.8+).
"""
import argparse
import csv
import ctypes
import sys
import time
from datetime import datetime

if sys.platform != "win32":
    sys.exit("This script only runs on Windows (AC's shared memory is Windows-only).")

F = ctypes.c_float
I = ctypes.c_int
W = ctypes.c_uint16  # Windows wchar_t is 2 bytes; strings aren't needed, only spacing


class Physics(ctypes.Structure):
    """Prefix of Kunos's SPageFilePhysics struct."""
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
    ]


class Graphics(ctypes.Structure):
    """Prefix of Kunos's SPageFileGraphic struct (session/lap info)."""
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
    ]


AC_LIVE = 2
PSIZE = ctypes.sizeof(Physics)
GSIZE = ctypes.sizeof(Graphics)
WHEELS = ["fl", "fr", "rl", "rr"]
AXES = ["x", "y", "z"]
# graphics fields copied into each CSV row
G_COLS = [("completedLaps", "lap"), ("iCurrentTime", "lapTimeMs"),
          ("iLastTime", "lastLapMs"), ("normalizedCarPosition", "trackPos")]

k32 = ctypes.WinDLL("kernel32", use_last_error=True)
k32.OpenFileMappingW.restype = ctypes.c_void_p
k32.OpenFileMappingW.argtypes = [ctypes.c_uint32, ctypes.c_int, ctypes.c_wchar_p]
k32.MapViewOfFile.restype = ctypes.c_void_p
k32.MapViewOfFile.argtypes = [ctypes.c_void_p, ctypes.c_uint32, ctypes.c_uint32,
                              ctypes.c_uint32, ctypes.c_size_t]
FILE_MAP_READ = 0x0004


def open_view(name, size):
    """Map an existing AC block read-only; None if AC hasn't created it yet."""
    handle = k32.OpenFileMappingW(FILE_MAP_READ, 0, name)
    if not handle:
        return None
    return k32.MapViewOfFile(handle, FILE_MAP_READ, 0, 0, size) or None


def wait_for_ac():
    warned = False
    while True:
        pv = open_view("Local\\acpmf_physics", PSIZE)
        gv = open_view("Local\\acpmf_graphics", GSIZE)
        if pv and gv:
            return pv, gv
        if not warned:
            print("Waiting for Assetto Corsa... (start a session and get on track)")
            warned = True
        time.sleep(1)


def snap(cls, view, size):
    return cls.from_buffer_copy(ctypes.string_at(view, size))


def columns():
    cols = ["t"] + [alias for _, alias in G_COLS] + ["posX", "posY", "posZ"]
    for name, typ in Physics._fields_:
        n = getattr(typ, "_length_", None)
        if n is None:
            cols.append(name)
        elif n == 4:
            cols += [f"{name}_{w}" for w in WHEELS]
        elif n == 3:
            cols += [f"{name}_{a}" for a in AXES]
        else:
            cols += [f"{name}_{i}" for i in range(n)]
    return cols


def row(p, g, t):
    out = [f"{t:.4f}"]
    for field, _ in G_COLS:
        v = getattr(g, field)
        out.append(round(v, 6) if isinstance(v, float) else v)
    out += [round(x, 4) for x in g.carCoordinates]
    for name, typ in Physics._fields_:
        v = getattr(p, name)
        if hasattr(typ, "_length_"):
            out += [round(x, 5) for x in v]
        elif isinstance(v, float):
            out.append(round(v, 5))
        else:
            out.append(v)
    return out


def main():
    ap = argparse.ArgumentParser(description="Log Assetto Corsa physics telemetry to CSV.")
    ap.add_argument("--out", help="CSV path (default: ac_telemetry_YYYYmmdd_HHMMSS.csv)")
    ap.add_argument("--laps", type=int, default=0, help="stop after N completed laps (0 = off)")
    ap.add_argument("--duration", type=float, default=900,
                    help="safety cap in seconds of live driving (0 = no cap, default 900)")
    ap.add_argument("--print", action="store_true", help="show a live readout while logging")
    args = ap.parse_args()

    out = args.out or f"ac_telemetry_{datetime.now():%Y%m%d_%H%M%S}.csv"
    pv, gv = wait_for_ac()
    print(f"Connected. Logging to {out}")
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

    with open(out, "w", newline="") as f:
        w = csv.writer(f)
        w.writerow(columns())
        try:
            while True:
                g = snap(Graphics, gv, GSIZE)
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
                    stop_at = live_time + 2.0   # keep 2 s so the last lap time lands in the CSV
                if stop_at is not None and live_time >= stop_at:
                    reason = f"finished {args.laps} lap(s)"
                    break

                p = snap(Physics, pv, PSIZE)
                if p.packetId == last_id or p.packetId == 0:
                    time.sleep(0.0005)       # no new physics step yet
                    continue
                last_id = p.packetId
                w.writerow(row(p, g, live_time))
                count += 1

                if count == 200 and not checked:
                    checked = True
                    ok_p = 0 <= p.speedKmh < 600 and -1 <= p.gear <= 10 and 0 <= p.rpms < 30000
                    ok_g = 0 <= g.completedLaps < 10000 and 0 <= g.normalizedCarPosition <= 1.01
                    if not (ok_p and ok_g):
                        print("\nWARNING: values look wrong - struct layout may not match "
                              "your AC version. Check the CSV before trusting it.")
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


if __name__ == "__main__":
    main()