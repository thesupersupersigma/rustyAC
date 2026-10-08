# SPDX-License-Identifier: MIT OR Apache-2.0

"""
compare_laps.py - lay two recorded laps over each other, corner by corner.

Input: two CSV files written by ac_telemetry.py, e.g. one recorded from real Assetto Corsa
and one from rustyAC (both fill the same shared memory, so the columns are the same).
The laps are lined up by position along the track (the `trackPos` column, 0..1), not by
time, so the table says what each car did at the same place.

    python tools/compare_laps.py ac.csv rusty.csv
    python tools/compare_laps.py ac.csv rusty.csv --plot laps.html
    python tools/compare_laps.py ac.csv rusty.csv --lap-a 2 --lap-b 1 --name-a AC --name-b rustyAC

Options:
    --lap-a N / --lap-b N   which lap of file A / B (the `lap` column); default: the fastest
                            complete lap, or the longest stretch if no lap is complete
    --name-a / --name-b     labels (default: A, B)
    --track spa|auto        the corner table: Spa's built-in one (default), or `auto`:
                            corners found from lap A's lateral g
    --corners FILE          a track's data/sections.ini (IN / OUT / TEXT) instead
    --plot FILE.html        one self-contained page: speed, lateral g, steering and the time
                            difference against track position
    --points N              points of the common grid (default 4000, about 1.75 m at Spa)

Signs and units, as the shared memory has them: speed km/h; lateral g = accG_x (negative in
a right-hand corner); steering = steerAngle, the wheel's turn as a fraction of full lock,
positive to the right; gear = the lowest gear used in the corner. "time" is the time spent
between a corner's start and end by the game's own lap clock; the last column is B minus A
(negative: B is quicker).

No dependencies (Python 3.8+).
"""
import argparse
import csv
import html
import json
import sys

# Spa (AC's `spa`): the track's own data/sections.ini, with Les Combes, Bruxelles and Fagnes
# cut into their corners where the AI's lap changes direction.
SPA = [
    ("La Source", 0.038, 0.068),
    ("Eau Rouge", 0.137, 0.154),
    ("Raidillon", 0.155, 0.197),
    ("Les Combes 1", 0.325, 0.3585),
    ("Les Combes 2", 0.3585, 0.372),
    ("Malmedy", 0.372, 0.392),
    ("Bruxelles (Rivage)", 0.411, 0.447),
    ("Speaker's Corner", 0.447, 0.490),
    ("Double Gauche (Pouhon)", 0.529, 0.602),
    ("Fagnes 1", 0.615, 0.662),
    ("Fagnes 2", 0.662, 0.685),
    ("Campus (Stavelot)", 0.693, 0.727),
    ("Courbe Paul Frere", 0.732, 0.783),
    ("Blanchimont", 0.824, 0.911),
    ("Chicane", 0.947, 0.980),
]
TRACKS = {"spa": SPA}

NEEDED = ["t", "lap", "trackPos", "speedKmh", "accG_x", "steerAngle", "gear"]


class Lap:
    """One lap of one file: samples with a strictly rising track position."""

    def __init__(self, number, rows, time_ms):
        self.number = number
        self.time_ms = time_ms          # the game's own lap time, None if not known
        first = rows[0]
        self.info = {k: first.get(k) for k in ("airTemp", "roadTemp", "fuel", "tc", "abs", "autoShifterOn")}
        self.pos, self.t, self.speed, self.latg, self.steer, self.gear = [], [], [], [], [], []
        self.gas, self.brake = [], []
        # the track position starts again from 0 a few metres before the timing line, where the
        # lap counter changes: the rows of a lap that begins with a stretch before the line (a
        # hot-lap start, a pit exit) are cut there, and the longer piece is the lap
        pieces, before = [[]], None
        for r in rows:
            p = float(r["trackPos"])
            if before is not None and p < before - 0.5:
                pieces.append([])
            before = p
            pieces[-1].append(r)
        rows = max(pieces, key=len)
        last = -1.0
        clock = []
        for r in rows:
            p = float(r["trackPos"])
            # the position must rise: this drops the stretch before the line on an out-lap,
            # a car standing still, and the packets between two frames (the position and the
            # lap clock are written once a frame, the rest of a row 333 times a second)
            if p <= last:
                continue
            last = p
            self.pos.append(p)
            self.t.append(float(r["t"]))
            clock.append(float(r.get("lapTimeMs") or 0.0) / 1000.0)
            self.speed.append(float(r["speedKmh"]))
            self.latg.append(float(r["accG_x"]))
            self.steer.append(float(r["steerAngle"]))
            self.gear.append(int(float(r["gear"])) - 1)
            self.gas.append(float(r.get("gas") or 0.0))
            self.brake.append(float(r.get("brake") or 0.0))
        # the lap clock starts again at the timing line, a few samples into such a piece
        head = max(1, len(clock) // 20)
        cut = min(range(head), key=lambda i: clock[i]) if clock else 0
        if cut:
            clock = clock[cut:]
            for name in ("pos", "t", "speed", "latg", "steer", "gear", "gas", "brake"):
                setattr(self, name, getattr(self, name)[cut:])
        # the game's own lap clock is the time base where it runs (the recorder's `t` is the
        # PC's clock, which drifts against the game's by some hundredths of a second a lap)
        self.game_clock = len(clock) > 10 and clock[-1] > clock[0] and all(b >= a for a, b in zip(clock, clock[1:]))
        if self.game_clock:
            self.t = clock

    @property
    def start(self):
        return self.pos[0]

    @property
    def end(self):
        return self.pos[-1]

    @property
    def complete(self):
        return len(self.pos) > 10 and self.start < 0.02 and self.end > 0.98

    @property
    def seconds(self):
        """The lap time: the game's if it is known, else from the first to the last sample."""
        if self.time_ms:
            return self.time_ms / 1000.0
        if self.game_clock and self.complete:
            # the lap clock at the last sample, carried on to the line at the speed there
            k = max(0, len(self.pos) - 6)
            rate = (self.t[-1] - self.t[k]) / (self.pos[-1] - self.pos[k])
            return self.t[-1] + (1.0 - self.pos[-1]) * rate
        return self.t[-1] - self.t[0]

    def describe(self):
        kind = "complete" if self.complete else "part: %.3f to %.3f of the track" % (self.start, self.end)
        return "lap %d (%s, %s)" % (self.number, fmt_time(self.seconds), kind)


def fmt_time(seconds):
    return "%d:%06.3f" % (int(seconds // 60), seconds % 60.0)


def read_laps(path):
    with open(path, newline="") as f:
        reader = csv.DictReader(f)
        missing = [c for c in NEEDED if c not in (reader.fieldnames or [])]
        if missing:
            sys.exit("%s: not an ac_telemetry.py CSV (no column %s)" % (path, ", ".join(missing)))
        groups = []   # (lap number, rows), in the file's order
        for row in reader:
            number = int(float(row["lap"]))
            if not groups or groups[-1][0] != number:
                groups.append((number, []))
            groups[-1][1].append(row)
    laps = []
    for i, (number, rows) in enumerate(groups):
        # the game's time of this lap: `lastLapMs` changes in the lap's last rows (at the timing
        # line, just before the lap counter does)
        time_ms = None
        if "lastLapMs" in rows[0]:
            at_start, at_end = int(float(rows[0]["lastLapMs"])), int(float(rows[-1]["lastLapMs"]))
            if at_end != at_start and at_end > 0:
                time_ms = at_end
        lap = Lap(number, rows, time_ms)
        if len(lap.pos) > 10:
            laps.append(lap)
    if not laps:
        sys.exit("%s: the track position (trackPos) never moves: recorded without a track, or the car stood still" % path)
    return laps


def choose(laps, wanted, path):
    if wanted is not None:
        for lap in laps:
            if lap.number == wanted:
                return lap
        sys.exit("%s: no lap %d (it has: %s)" % (path, wanted, ", ".join(str(l.number) for l in laps)))
    complete = [l for l in laps if l.complete]
    if complete:
        # an out-lap that starts on the line is "complete" too; the game's own time decides
        return min(complete, key=lambda l: l.seconds)
    return max(laps, key=lambda l: l.end - l.start)


def interpolate(xs, ys, grid):
    """ys at the grid's positions (xs rising); None outside xs."""
    out, j, n = [], 0, len(xs)
    for g in grid:
        if g < xs[0] or g > xs[-1]:
            out.append(None)
            continue
        while j + 1 < n and xs[j + 1] < g:
            j += 1
        if j + 1 >= n:
            out.append(ys[-1])
            continue
        x0, x1 = xs[j], xs[j + 1]
        f = 0.0 if x1 == x0 else (g - x0) / (x1 - x0)
        out.append(ys[j] + (ys[j + 1] - ys[j]) * f)
    return out


def nearest(xs, ys, grid):
    out, j, n = [], 0, len(xs)
    for g in grid:
        if g < xs[0] or g > xs[-1]:
            out.append(None)
            continue
        while j + 1 < n and xs[j + 1] <= g:
            j += 1
        out.append(ys[j])
    return out


class Trace:
    """A lap on the common grid."""

    def __init__(self, lap, grid):
        self.lap = lap
        self.t = interpolate(lap.pos, lap.t, grid)
        self.speed = interpolate(lap.pos, lap.speed, grid)
        self.latg = interpolate(lap.pos, lap.latg, grid)
        self.steer = interpolate(lap.pos, lap.steer, grid)
        self.gear = nearest(lap.pos, lap.gear, grid)


def read_sections(path):
    corners, current = [], {}
    def flush():
        if "IN" in current and "OUT" in current:
            corners.append((current.get("TEXT", "section %d" % len(corners)), float(current["IN"]), float(current["OUT"])))
    with open(path, encoding="utf-8", errors="replace") as f:
        for line in f:
            line = line.split(";")[0].strip()
            if line.startswith("["):
                flush()
                current = {}
            elif "=" in line:
                key, value = line.split("=", 1)
                current[key.strip().upper()] = value.strip()
    flush()
    if not corners:
        sys.exit("%s: no [SECTION_n] with IN and OUT" % path)
    return sorted(corners, key=lambda c: c[1])


def find_corners(grid, latg):
    """Corners from one lap's lateral g: stretches above a third of its peak (at least 0.5 g)."""
    values = [abs(v) if v is not None else 0.0 for v in latg]
    limit = max(0.5, max(values) / 3.0)
    runs, start = [], None
    for i, v in enumerate(values):
        if v >= limit and start is None:
            start = i
        elif v < limit and start is not None:
            runs.append([grid[start], grid[i - 1]])
            start = None
    if start is not None:
        runs.append([grid[start], grid[-1]])
    merged = []
    for run in runs:
        # a short dip below the limit does not end a corner
        if merged and run[0] - merged[-1][1] < 0.004:
            merged[-1][1] = run[1]
        else:
            merged.append(run)
    pad = 0.003
    return [("T%d" % (i + 1), max(0.0, a - pad), min(1.0, b + pad)) for i, (a, b) in enumerate(m for m in merged if m[1] - m[0] >= 0.004)]


def index_at(grid, position):
    step = grid[1] - grid[0]
    return max(0, min(len(grid) - 1, int(round((position - grid[0]) / step))))


def corner_numbers(trace, i0, i1):
    """What a lap did between two grid points, None if it does not cover them."""
    if trace.t[i0] is None or trace.t[i1] is None:
        return None
    r = range(i0, i1 + 1)
    apex = min(r, key=lambda i: trace.speed[i])
    peak = max(r, key=lambda i: abs(trace.latg[i]))
    steer = max(r, key=lambda i: abs(trace.steer[i]))
    return {
        "entry": trace.speed[i0],
        "min": trace.speed[apex],
        "exit": trace.speed[i1],
        "latg": trace.latg[peak],
        "steer": trace.steer[steer],
        # the lowest gear driven in (neutral between two gears does not count)
        "gear": min((trace.gear[i] for i in r if trace.gear[i] > 0), default=0),
        "time": trace.t[i1] - trace.t[i0],
    }


def table(rows, name_a, name_b):
    head = ["corner", "from", "to",
            "entry " + name_a, name_b, "min " + name_a, name_b, "diff", "exit " + name_a, name_b,
            "lat g " + name_a, name_b, "steer " + name_a, name_b, "gear " + name_a, name_b,
            "time " + name_a, name_b, "diff"]
    body = []
    for name, a, b, na, nb in rows:
        line = [name, "%.3f" % a, "%.3f" % b]
        def pair(key, fmt):
            return [fmt % na[key] if na else "-", fmt % nb[key] if nb else "-"]
        both = na is not None and nb is not None
        line += pair("entry", "%.1f")
        line += pair("min", "%.1f") + ["%+.1f" % (nb["min"] - na["min"]) if both else "-"]
        line += pair("exit", "%.1f")
        line += pair("latg", "%+.2f")
        line += pair("steer", "%+.3f")
        line += pair("gear", "%d")
        line += pair("time", "%.3f") + ["%+.3f" % (nb["time"] - na["time"]) if both else "-"]
        body.append(line)
    widths = [max(len(head[i]), max((len(l[i]) for l in body), default=0)) for i in range(len(head))]
    def fmt(line):
        return "  ".join(cell.ljust(widths[i]) if i == 0 else cell.rjust(widths[i]) for i, cell in enumerate(line))
    return "\n".join([fmt(head), "  ".join("-" * w for w in widths)] + [fmt(l) for l in body]), head, body


def conditions(lap):
    i = lap.info
    def num(key, fmt):
        try:
            return fmt % float(i[key])
        except (TypeError, ValueError):
            return "?"
    return "air %s C, road %s C, fuel at the first sample %s l, TC %s, ABS %s, automatic gearbox %s" % (
        num("airTemp", "%.0f"), num("roadTemp", "%.0f"), num("fuel", "%.1f"), num("tc", "%.2f"), num("abs", "%.2f"),
        {"1": "on", "0": "off"}.get(str(i.get("autoShifterOn")), "?"))


PAGE = r"""<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">
<title>__TITLE__</title>
<style>
:root{--surface:#fcfcfb;--page:#f9f9f7;--ink:#0b0b0b;--ink2:#52514e;--muted:#898781;--grid:#e1e0d9;--axis:#c3c2b7;--a:#2a78d6;--b:#eb6834;--band:#f0efec}
@media (prefers-color-scheme:dark){:root{--surface:#1a1a19;--page:#0d0d0d;--ink:#fff;--ink2:#c3c2b7;--muted:#898781;--grid:#2c2c2a;--axis:#383835;--a:#3987e5;--b:#d95926;--band:#242422}}
body{margin:0;padding:16px;background:var(--page);color:var(--ink);font:14px/1.45 system-ui,Segoe UI,sans-serif}
main{max-width:1280px;margin:0 auto}
h1{font-size:18px;margin:0 0 4px} p{margin:0 0 8px;color:var(--ink2)}
.legend{display:flex;gap:18px;flex-wrap:wrap;margin:10px 0}
.legend span{display:inline-flex;align-items:center;gap:6px}
.legend i{width:18px;height:0;border-top:2px solid;display:inline-block}
.legend i.b{border-top-style:dashed}
.chart{background:var(--surface);border:1px solid var(--grid);border-radius:6px;margin:0 0 10px;padding:8px 8px 2px;position:relative}
.chart h2{font-size:13px;font-weight:600;margin:0 0 2px 46px}
svg{display:block;width:100%;height:auto;overflow:visible}
svg text{fill:var(--muted);font-size:11px}
.tip{position:fixed;pointer-events:none;background:var(--surface);color:var(--ink);border:1px solid var(--axis);border-radius:6px;padding:6px 9px;font-size:12px;display:none;white-space:nowrap;box-shadow:0 2px 8px #0003;z-index:5}
.tip b{font-weight:600}.tip td{padding:0 6px 0 0}.tip td.n{text-align:right;font-variant-numeric:tabular-nums}
.sw{display:inline-block;width:10px;height:0;border-top:2px solid;vertical-align:middle;margin-right:4px}
.scroll{overflow-x:auto}
table.data{border-collapse:collapse;font-size:12px;font-variant-numeric:tabular-nums;background:var(--surface)}
table.data th,table.data td{border:1px solid var(--grid);padding:3px 7px;text-align:right;white-space:nowrap}
table.data th:first-child,table.data td:first-child{text-align:left}
table.data th{color:var(--ink2);font-weight:600}
</style></head><body><main>
<h1>__TITLE__</h1>
<p>__SUB__</p>
<div class="legend"><span><i style="border-color:var(--a)"></i>__NAME_A__</span><span><i class="b" style="border-color:var(--b)"></i>__NAME_B__</span><span style="color:var(--ink2)">shaded: the corners of the table; move the pointer over a chart for the values</span></div>
<div id="charts"></div>
<h2 style="font-size:14px">Corner table</h2>
<div class="scroll">__TABLE__</div>
<div class="tip" id="tip"></div>
</main>
<script>
const D = __DATA__;
const NS = "http://www.w3.org/2000/svg";
const W = 1200, H = 210, L = 46, R = 12, T = 16, B = 22;
const PANELS = [
  {key: "speed", title: "Speed (km/h)", digits: 1},
  {key: "latg", title: "Lateral g (negative: right-hand corner)", digits: 2, zero: true},
  {key: "steer", title: "Steering (fraction of full lock, positive: right)", digits: 3, zero: true},
  {key: "delta", title: "Time difference, " + D.nameB + " minus " + D.nameA + " (s; falling: " + D.nameB + " is gaining)", digits: 3, zero: true, single: true},
];
function el(name, attrs, parent) { const e = document.createElementNS(NS, name); for (const k in attrs) e.setAttribute(k, attrs[k]); if (parent) parent.appendChild(e); return e; }
function nice(lo, hi, n) { const span = (hi - lo) || 1, raw = span / n, p = Math.pow(10, Math.floor(Math.log10(raw))); const s = [1, 2, 5, 10].map(m => m * p).find(v => v >= raw); const out = []; for (let v = Math.ceil(lo / s) * s; v <= hi + 1e-9; v += s) out.push(+v.toFixed(10)); return out; }
const x0 = D.grid[0], x1 = D.grid[D.grid.length - 1];
const sx = p => L + (p - x0) / ((x1 - x0) || 1) * (W - L - R);
const charts = [];
for (const P of PANELS) {
  const series = P.single ? [[D.delta, "var(--ink)", ""]] : [[D.a[P.key], "var(--a)", ""], [D.b[P.key], "var(--b)", "6 3"]];
  let lo = Infinity, hi = -Infinity;
  for (const [ys] of series) for (const v of ys) if (v !== null) { lo = Math.min(lo, v); hi = Math.max(hi, v); }
  if (P.zero) { lo = Math.min(lo, 0); hi = Math.max(hi, 0); }
  if (!(hi > lo)) { hi = lo + 1; }
  const pad = (hi - lo) * 0.06; lo -= pad; hi += pad;
  const sy = v => T + (hi - v) / (hi - lo) * (H - T - B);
  const box = document.createElement("div"); box.className = "chart";
  const h = document.createElement("h2"); h.textContent = P.title; box.appendChild(h);
  const svg = el("svg", {viewBox: `0 0 ${W} ${H}`, role: "img", "aria-label": P.title}); box.appendChild(svg);
  for (const c of D.corners) { if (c[2] < x0 || c[1] > x1) continue; const a = sx(Math.max(c[1], x0)), b = sx(Math.min(c[2], x1)); el("rect", {x: a, y: T, width: Math.max(1, b - a), height: H - T - B, fill: "var(--band)"}, svg); }
  if (P === PANELS[0]) { let lastEnd = -1e9; for (const c of D.corners) { if (c[2] < x0 || c[1] > x1) continue; const mid = sx((Math.max(c[1], x0) + Math.min(c[2], x1)) / 2); const w = c[0].length * 6; if (mid - w / 2 < lastEnd + 6) continue; lastEnd = mid + w / 2; const t = el("text", {x: mid, y: T - 4, "text-anchor": "middle"}, svg); t.textContent = c[0]; } }
  for (const v of nice(lo, hi, 5)) { el("line", {x1: L, x2: W - R, y1: sy(v), y2: sy(v), stroke: v === 0 && P.zero ? "var(--axis)" : "var(--grid)", "stroke-width": 1}, svg); const t = el("text", {x: L - 6, y: sy(v) + 4, "text-anchor": "end"}, svg); t.textContent = v; }
  for (const v of nice(x0, x1, 10)) { const t = el("text", {x: sx(v), y: H - 6, "text-anchor": "middle"}, svg); t.textContent = v.toFixed(2); }
  for (const [ys, color, dash] of series) { let d = "", pen = false; ys.forEach((v, i) => { if (v === null) { pen = false; return; } d += (pen ? "L" : "M") + sx(D.grid[i]).toFixed(1) + " " + sy(v).toFixed(1); pen = true; }); const p = el("path", {d, fill: "none", stroke: color, "stroke-width": 2, "stroke-linejoin": "round"}, svg); if (dash) p.setAttribute("stroke-dasharray", dash); }
  const cross = el("line", {y1: T, y2: H - B, stroke: "var(--muted)", "stroke-width": 1, visibility: "hidden"}, svg);
  const hit = el("rect", {x: L, y: T, width: W - L - R, height: H - T - B, fill: "transparent"}, svg);
  charts.push({svg, cross, hit});
  document.getElementById("charts").appendChild(box);
}
const tip = document.getElementById("tip");
function fmt(v, d) { return v === null || v === undefined ? "-" : v.toFixed(d); }
function show(ev, chart) {
  const r = chart.svg.getBoundingClientRect(); const px = (ev.clientX - r.left) / r.width * W;
  const p = x0 + (px - L) / (W - L - R) * (x1 - x0); const i = Math.max(0, Math.min(D.grid.length - 1, Math.round((p - x0) / (D.grid[1] - D.grid[0]))));
  for (const c of charts) { c.cross.setAttribute("x1", sx(D.grid[i])); c.cross.setAttribute("x2", sx(D.grid[i])); c.cross.setAttribute("visibility", "visible"); }
  const corner = D.corners.find(c => D.grid[i] >= c[1] && D.grid[i] <= c[2]);
  let h = `<b>track position ${D.grid[i].toFixed(4)}</b>${corner ? " &middot; " + corner[0].replace(/&/g, "&amp;").replace(/</g, "&lt;") : ""}<table><tr><td></td><td class="n"><span class="sw" style="border-color:var(--a)"></span>${D.nameA}</td><td class="n"><span class="sw" style="border-color:var(--b);border-top-style:dashed"></span>${D.nameB}</td></tr>`;
  for (const [k, label, d] of [["speed", "speed km/h", 1], ["latg", "lateral g", 2], ["steer", "steering", 3], ["gear", "gear", 0]]) h += `<tr><td>${label}</td><td class="n">${fmt(D.a[k][i], d)}</td><td class="n">${fmt(D.b[k][i], d)}</td></tr>`;
  h += `</table>time difference ${D.delta[i] === null ? "-" : (D.delta[i] > 0 ? "+" : "") + D.delta[i].toFixed(3) + " s"}`;
  tip.innerHTML = h; tip.style.display = "block";
  const tw = tip.offsetWidth, th = tip.offsetHeight; let tx = ev.clientX + 14, ty = ev.clientY + 14;
  if (tx + tw > innerWidth - 4) tx = ev.clientX - tw - 14; if (ty + th > innerHeight - 4) ty = ev.clientY - th - 14;
  tip.style.left = Math.max(4, tx) + "px"; tip.style.top = Math.max(4, ty) + "px";
}
for (const c of charts) { c.hit.addEventListener("pointermove", ev => show(ev, c)); c.hit.addEventListener("pointerleave", () => { tip.style.display = "none"; for (const k of charts) k.cross.setAttribute("visibility", "hidden"); }); }
</script></body></html>
"""


def write_plot(path, grid, a, b, delta, corners, name_a, name_b, title, sub, head, body):
    # every `step`-th grid point: about 2000 points a line is as fine as a screen shows
    step = max(1, len(grid) // 2000)
    def thin(values, digits):
        return [None if v is None else round(v, digits) for v in values[::step]]
    def lap(trace):
        return {"speed": thin(trace.speed, 2), "latg": thin(trace.latg, 3), "steer": thin(trace.steer, 4), "gear": trace.gear[::step]}
    data = {
        "grid": [round(g, 6) for g in grid[::step]],
        "a": lap(a), "b": lap(b), "delta": thin(delta, 4),
        "corners": [[n, i, o] for n, i, o in corners],
        "nameA": html.escape(name_a), "nameB": html.escape(name_b),
    }
    rows = "".join("<tr>" + "".join("<td>%s</td>" % html.escape(c) for c in line) + "</tr>" for line in body)
    table_html = "<table class=\"data\"><tr>%s</tr>%s</table>" % ("".join("<th>%s</th>" % html.escape(h) for h in head), rows)
    page = (PAGE.replace("__TITLE__", html.escape(title)).replace("__SUB__", sub)
            .replace("__NAME_A__", html.escape(name_a)).replace("__NAME_B__", html.escape(name_b))
            .replace("__TABLE__", table_html)
            .replace("__DATA__", json.dumps(data, separators=(",", ":")).replace("</", "<\\/")))
    with open(path, "w", encoding="utf-8", newline="\n") as f:
        f.write(page)


def main():
    ap = argparse.ArgumentParser(description="Compare two laps recorded with ac_telemetry.py, corner by corner.")
    ap.add_argument("a", help="CSV A (the reference, e.g. real AC)")
    ap.add_argument("b", help="CSV B (e.g. rustyAC)")
    ap.add_argument("--lap-a", type=int, help="lap number in A (default: the fastest complete lap)")
    ap.add_argument("--lap-b", type=int, help="lap number in B")
    ap.add_argument("--name-a", default="A")
    ap.add_argument("--name-b", default="B")
    ap.add_argument("--track", default="spa", help="built-in corner table (%s) or auto" % ", ".join(TRACKS))
    ap.add_argument("--corners", help="a sections.ini with IN / OUT / TEXT")
    ap.add_argument("--plot", metavar="FILE.html", help="write one self-contained HTML page with the curves")
    ap.add_argument("--points", type=int, default=4000, help="points of the common grid (default 4000)")
    args = ap.parse_args()

    laps_a, laps_b = read_laps(args.a), read_laps(args.b)
    lap_a, lap_b = choose(laps_a, args.lap_a, args.a), choose(laps_b, args.lap_b, args.b)
    lo, hi = max(lap_a.start, lap_b.start), min(lap_a.end, lap_b.end)
    if hi - lo < 0.01:
        sys.exit("the two laps share no stretch of track: %s covers %.3f to %.3f, %s covers %.3f to %.3f"
                 % (args.name_a, lap_a.start, lap_a.end, args.name_b, lap_b.start, lap_b.end))
    n = max(100, args.points)
    grid = [i / n for i in range(n + 1)]
    a, b = Trace(lap_a, grid), Trace(lap_b, grid)
    shared = [i for i in range(n + 1) if a.t[i] is not None and b.t[i] is not None]
    first, last = shared[0], shared[-1]
    delta = [None] * (n + 1)
    for i in shared:
        delta[i] = (b.t[i] - b.t[first]) - (a.t[i] - a.t[first])

    if args.corners:
        corners, source = read_sections(args.corners), args.corners
    elif args.track == "auto":
        corners, source = find_corners(grid, a.latg), "found from %s's lateral g" % args.name_a
    elif args.track in TRACKS:
        corners, source = TRACKS[args.track], "built-in table of %s" % args.track
    else:
        sys.exit("no built-in corner table for '%s' (there is: %s); use --track auto or --corners <sections.ini>" % (args.track, ", ".join(TRACKS)))

    print("%s: %s, %s" % (args.name_a, args.a, lap_a.describe()))
    print("    %s" % conditions(lap_a))
    print("%s: %s, %s" % (args.name_b, args.b, lap_b.describe()))
    print("    %s" % conditions(lap_b))
    for key, what in (("airTemp", "air temperature"), ("roadTemp", "road temperature")):
        va, vb = lap_a.info.get(key), lap_b.info.get(key)
        try:
            if abs(float(va) - float(vb)) > 0.05:
                print("    NOT A FAIR PAIR: the %s differs (%s against %s C)" % (what, va, vb))
        except (TypeError, ValueError):
            pass
    print("compared from %.4f to %.4f of the track; corners: %s" % (grid[first], grid[last], source))
    print()

    rows = []
    for name, start, end in corners:
        i0, i1 = index_at(grid, start), index_at(grid, end)
        inside = i0 >= first and i1 <= last and i1 > i0
        rows.append((name, start, end, corner_numbers(a, i0, i1) if inside else None, corner_numbers(b, i0, i1) if inside else None))
    text, head, body = table(rows, args.name_a, args.name_b)
    print(text)
    print()
    in_corners = sum(nb["time"] - na["time"] for _, _, _, na, nb in rows if na and nb)
    total = delta[last]
    print("time difference over the compared stretch (%s minus %s): %+.3f s, of it in the listed corners %+.3f s, elsewhere %+.3f s"
          % (args.name_b, args.name_a, total, in_corners, total - in_corners))
    if lap_a.complete and lap_b.complete:
        print("lap times: %s %s, %s %s (%+.3f s)" % (args.name_a, fmt_time(lap_a.seconds), args.name_b, fmt_time(lap_b.seconds), lap_b.seconds - lap_a.seconds))
    both = [(name, nb["min"] - na["min"], nb["time"] - na["time"]) for name, _, _, na, nb in rows if na and nb]
    if both:
        quick = sorted(both, key=lambda r: -r[1])[:3]
        print("largest difference in minimum speed: " + "; ".join("%s %+.1f km/h" % (name, dv) for name, dv, _ in quick))

    if args.plot:
        title = "%s against %s" % (args.name_a, args.name_b)
        sub = "%s: %s, %s &middot; %s: %s, %s &middot; lined up by track position, %.3f to %.3f" % (
            html.escape(args.name_a), html.escape(args.a), html.escape(lap_a.describe()),
            html.escape(args.name_b), html.escape(args.b), html.escape(lap_b.describe()), grid[first], grid[last])
        write_plot(args.plot, grid, a, b, delta, corners, args.name_a, args.name_b, title, sub, head, body)
        print("plot: %s" % args.plot)


if __name__ == "__main__":
    main()
