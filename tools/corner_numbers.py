# SPDX-License-Identifier: MIT OR Apache-2.0

"""
corner_numbers.py - the numbers of one stretch of a lap out of a telemetry CSV.

Reads a CSV written by ac_telemetry.py (real Assetto Corsa or rustyAC) or by the grip_probe
example and prints, for every pass of the stretch: speeds, gears, lateral and braking g
(averaged over 10 m), tyre loads, slip, core temperatures, pressures, ride heights, traction
control, and the largest force the tyres made per newton of load. With --pressure it also
fits the tyre pressure against the core temperature over the whole file (plain acs.exe:
0.16 psi per deg C from the static pressure at 26 deg C).

usage:
    python tools/corner_numbers.py <csv> [--from 0.947] [--to 0.985] [--mass 605]
                                   [--pressure] [--samples]

    --from / --to   the stretch along the lap, 0..1 (default: Spa's Bus Stop chicane)
    --mass          the car with its driver, without fuel, kg (default 605: the F2004);
                    fuel is counted at 0.74 kg per litre
    --pressure      fit pressure = a + b * (core temperature - 26) for each wheel
    --samples       print about forty rows of each pass

Read with care: `tc` is the traction control's slip limit while the aid is switched on (0:
off, or the car is driven by the game's AI, which switches it off); an AI-driven car has 20 %
more tyre grip than the player's whenever it is off the throttle (ai.ini [ULTRA_GRIP]).
"""
import csv
import sys

WHEELS = ["fl", "fr", "rl", "rr"]


def col(row, name, default=float("nan")):
    try:
        return float(row[name])
    except (KeyError, ValueError):
        return default


def option(args, name, default):
    return float(args[args.index(name) + 1]) if name in args else default


def pressure_fit(rows):
    print("tyre pressure against core temperature, whole file:")
    for w in WHEELS:
        xs = [col(r, "tyreCoreTemperature_" + w) for r in rows]
        ys = [col(r, "wheelsPressure_" + w) for r in rows]
        n = len(xs)
        mx, my = sum(xs) / n, sum(ys) / n
        sxx = sum((x - mx) ** 2 for x in xs)
        if sxx < 1e-9:
            print("  %s: the core temperature never changes (%.1f C, %.3f psi)" % (w, mx, my))
            continue
        k = sum((x - mx) * (y - my) for x, y in zip(xs, ys)) / sxx
        b = my - k * mx
        worst = max(abs(y - (k * x + b)) for x, y in zip(xs, ys))
        print("  %s: core %.1f..%.1f C, %.2f..%.2f psi: pressure = %.3f + %.4f * (core - 26), largest error %.3f psi" % (
            w, min(xs), max(xs), min(ys), max(ys), b + 26 * k, k, worst))
    print("  (plain acs.exe: static pressure + 0.1600 * (core - 26))")


def main():
    args = sys.argv[1:]
    if not args or args[0] in ("-h", "--help"):
        print(__doc__)
        return
    path = args[0]
    lo, hi, mass = option(args, "--from", 0.947), option(args, "--to", 0.985), option(args, "--mass", 605.0)
    rows = list(csv.DictReader(open(path, newline="", encoding="utf-8")))
    print("%s: %d rows" % (path, len(rows)))
    if "--pressure" in args:
        pressure_fit(rows)
    passes, current = [], []
    for r in rows:
        if lo <= col(r, "trackPos") <= hi:
            current.append(r)
        elif current:
            passes.append(current)
            current = []
    if current:
        passes.append(current)
    for k, rs in enumerate(passes):
        v = [col(r, "speedKmh") for r in rs]
        if max(v) < 100:
            print("pass %d: from a standstill (never above %.0f km/h), skipped" % (k, max(v)))
            continue
        t = [col(r, "t") for r in rs]
        lat = [col(r, "accG_x") for r in rs]
        lon = [col(r, "accG_z") for r in rs]
        # the lateral and longitudinal g averaged over the last 10 m
        dist = [0.0]
        for i in range(1, len(rs)):
            dist.append(dist[-1] + v[i] / 3.6 * max(0.0, t[i] - t[i - 1]))
        j, slat, slon, mlat, mlon = 0, 0.0, 0.0, [], []
        for i in range(len(rs)):
            slat += lat[i]
            slon += lon[i]
            while dist[i] - dist[j] > 10.0:
                slat -= lat[j]
                slon -= lon[j]
                j += 1
            mlat.append(slat / (i - j + 1))
            mlon.append(slon / (i - j + 1))
        imin = min(range(len(rs)), key=lambda i: v[i])
        print("pass %d (lap %s): %.2f s" % (k, rs[0].get("lap", "?"), t[-1] - t[0]))
        print("  speed km/h: in %.1f  min %.1f (at %.4f)  out %.1f" % (v[0], v[imin], col(rs[imin], "trackPos"), v[-1]))
        print("  gear %d..%d   lateral g (10 m): left %.2f right %.2f   braking g %.2f   accelerating g %.2f" % (
            min(int(col(r, "gear")) for r in rs), max(int(col(r, "gear")) for r in rs), max(mlat), -min(mlat), -min(mlon), max(mlon)))
        for name, label in [("wheelLoad", "load N"), ("wheelSlip", "slip / peak"), ("tyreCoreTemperature", "core C"), ("wheelsPressure", "psi"),
                            ("tyreDirtyLevel", "dirt"), ("tyreWear", "wear")]:
            if name + "_fl" not in rs[0]:
                continue
            parts = []
            for w in WHEELS:
                xs = [col(r, name + "_" + w) for r in rs]
                if name == "wheelSlip":
                    # a wheel in the air has any slip
                    xs = [x for x, r in zip(xs, rs) if col(r, "wheelLoad_" + w, 1000.0) > 200.0] or xs
                parts.append("%s %.3g..%.3g (mean %.3g)" % (w, min(xs), max(xs), sum(xs) / len(xs)))
            print("  %-11s %s" % (label, "; ".join(parts)))
        for name in ["rideHeight_0", "rideHeight_1", "airDensity", "fuel", "tc", "abs", "numberOfTyresOut"]:
            if name in rs[0]:
                xs = [col(r, name) for r in rs]
                print("  %-16s %.4g..%.4g (mean %.4g)" % (name, min(xs), max(xs), sum(xs) / len(xs)))
        # what the tyres made of their load
        ratios = []
        for r in rs:
            loads = [col(r, "wheelLoad_" + w) for w in WHEELS]
            total = sum(loads)
            if total < 3000 or min(loads) < 150 or col(r, "speedKmh") < 55:
                continue
            g = (col(r, "accG_x") ** 2 + col(r, "accG_z") ** 2) ** 0.5
            ratios.append((mass + col(r, "fuel", 0.0) * 0.74) * 9.80665 * g / total)
        if ratios:
            ratios.sort()
            pick = lambda q: ratios[min(len(ratios) - 1, int(q * len(ratios)))]
            print("  force per load (mass x acceleration / sum of the tyre loads): median %.2f, 95 %% of the rows below %.2f, 99 %% below %.2f" % (
                pick(0.5), pick(0.95), pick(0.99)))
        if "--samples" in args:
            print("  pos      km/h  gear gas  brake steer   latg  long   load fl/fr/rl/rr          slip fl/fr/rl/rr")
            for i in range(0, len(rs), max(1, len(rs) // 40)):
                r = rs[i]
                print("  %.4f %6.1f %2d  %.2f %.2f %+.3f %+5.2f %+5.2f  %5.0f %5.0f %5.0f %5.0f   %5.2f %5.2f %5.2f %5.2f" % (
                    col(r, "trackPos"), v[i], int(col(r, "gear")), col(r, "gas"), col(r, "brake"), col(r, "steerAngle"), mlat[i], mlon[i],
                    col(r, "wheelLoad_fl"), col(r, "wheelLoad_fr"), col(r, "wheelLoad_rl"), col(r, "wheelLoad_rr"),
                    min(9.99, col(r, "wheelSlip_fl")), min(9.99, col(r, "wheelSlip_fr")), min(9.99, col(r, "wheelSlip_rl")), min(9.99, col(r, "wheelSlip_rr"))))


if __name__ == "__main__":
    main()
