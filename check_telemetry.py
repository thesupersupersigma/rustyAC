"""check_telemetry.py - sanity-check a CSV from ac_telemetry.py.
Usage: python check_telemetry.py f2004_spa_ai.csv
"""
import csv, sys, math

path = sys.argv[1]
f = open(path, newline="")
r = csv.reader(f)
head = next(r)
ix = {n: i for i, n in enumerate(head)}
num = lambda row, k: float(row[ix[k]])

rows = 0; bad = 0; pkt_gaps = 0; last_pkt = None
vmax = 0; rpm_max = 0; gears = set(); laps_seen = {}
load_sum_min = 1e9; load_sum_max = 0; tyres_out_rows = 0
dmg_max = 0; prev_lap = None; laps_done = []
first_t = last_t = None
core_t = {w: [1e9, -1e9] for w in ("fl", "fr", "rl", "rr")}
for row in r:
    rows += 1
    try:
        vals = [float(x) for x in row]
    except ValueError:
        bad += 1; continue
    if any(math.isnan(v) or math.isinf(v) for v in vals):
        bad += 1; continue
    t = num(row, "t"); first_t = t if first_t is None else first_t; last_t = t
    pkt = int(num(row, "packetId"))
    if last_pkt is not None and pkt - last_pkt > 1:
        pkt_gaps += pkt - last_pkt - 1
    last_pkt = pkt
    vmax = max(vmax, num(row, "speedKmh")); rpm_max = max(rpm_max, num(row, "rpms"))
    gears.add(int(num(row, "gear")) - 1)
    loads = sum(num(row, f"wheelLoad_{w}") for w in ("fl", "fr", "rl", "rr"))
    if num(row, "speedKmh") > 5:
        load_sum_min = min(load_sum_min, loads); load_sum_max = max(load_sum_max, loads)
    if num(row, "numberOfTyresOut") > 2: tyres_out_rows += 1
    dmg_max = max(dmg_max, max(num(row, f"carDamage_{i}") for i in range(5)))
    for w in core_t:
        c = num(row, f"tyreCoreTemperature_{w}")
        core_t[w][0] = min(core_t[w][0], c); core_t[w][1] = max(core_t[w][1], c)
    lap = int(num(row, "lap"))
    if prev_lap is not None and lap != prev_lap:
        laps_done.append(int(num(row, "lastLapMs")))
    prev_lap = lap

dur = (last_t - first_t) if rows else 0
fmt = lambda ms: f"{ms // 60000}:{(ms % 60000) / 1000:06.3f}"
print(f"file           : {path}")
print(f"columns        : {len(head)}")
print(f"rows           : {rows}  (unreadable/NaN rows: {bad})")
print(f"duration       : {dur:.1f} s  -> {rows / dur if dur else 0:.1f} rows/s")
print(f"missed packets : {pkt_gaps}  ({100 * pkt_gaps / max(rows, 1):.2f}% of rows)")
print(f"top speed      : {vmax:.1f} km/h   max rpm: {rpm_max:.0f}   gears used: {sorted(gears)}")
print(f"wheel-load sum : {load_sum_min:.0f} .. {load_sum_max:.0f} N (moving)")
print(f"tyre core temp : " + ", ".join(f"{w} {a:.0f}-{b:.0f}C" for w, (a, b) in core_t.items()))
print(f"rows >2 tyres off track: {tyres_out_rows}   max damage value: {dmg_max:.3f}")
print("lap times      : " + (", ".join(fmt(ms) for ms in laps_done) or "none recorded"))
