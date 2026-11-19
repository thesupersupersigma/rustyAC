# SPDX-License-Identifier: MIT OR Apache-2.0
"""Plays a kept past run of tools/proofs/run_all.sh back into a folder, faster, for proof_tui to watch.

    python tools/proof_tui/replay.py <kept run> <out> [--speed 30] [--seed 1]

<kept run>: a copy of target/proofs after a run (rows/, cases/<mode>/, <area>.log, <area>/cases.log
and times.tsv). <out> gets what run_all.sh and lib.sh write, at the moments they wrote it divided by
the speed: run.status, rows/, <area>/{quick.re,last_case,now,cases.log,times.tsv}, table.md, ... The
"last run" times proof_tui guesses from (cases/<mode>/<area>.times) are the kept ones; each area plays
back slower or faster than them (a random factor per area), so the guess has to adjust. The kept run
had no DIFFERENT, ERROR or skipped case, so one of each is made from an IDENTICAL one (see INJECT).
Prints the moment the run ended (run.status's "end").
"""
import os, random, re, shutil, sys, time

INJECT = {("gui", 5): "DIFFERENT", ("net", 3): "ERROR", ("render", 4): "SKIPPED"}

args = sys.argv[1:]
speed = float(args[args.index("--speed") + 1]) if "--speed" in args else 30.0
rng = random.Random(int(args[args.index("--seed") + 1]) if "--seed" in args else 1)
src, out = [a for i, a in enumerate(args) if not a.startswith("--") and (i == 0 or not args[i - 1].startswith("--"))][:2]
repo = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "..")
lanes_line = re.search(r'^lanes=\$\{PROOF_LANES:-"([^"]*)"\}', open(os.path.join(repo, "tools/proofs/run_all.sh")).read(), re.M).group(1)
mode = "quick" if os.path.isdir(os.path.join(src, "cases/quick")) else "full"
areas = [a for a in re.split(r"[ |]", lanes_line) if os.path.isfile(os.path.join(src, a, "cases.log"))]
lanes = [[a for a in l.split() if a in areas] for l in lanes_line.split("|")]
lanes = [l for l in lanes if l]

def rd(p):
    with open(p, encoding="utf-8", errors="replace") as f:
        return f.read()
def wr(p, s, mode_="w"):
    with open(p, mode_, encoding="utf-8", newline="\n") as f:
        f.write(s)

shutil.rmtree(out, ignore_errors=True)
os.makedirs(os.path.join(out, "rows"))
os.makedirs(os.path.join(out, "cases", mode))
for a in areas:
    shutil.copy(os.path.join(src, "cases", mode, a + ".txt"), os.path.join(out, "cases", mode, a + ".txt"))
    kept = [l.split("\t", 1) for l in rd(os.path.join(src, a, "times.tsv")).splitlines() if "\t" in l]
    wr(os.path.join(out, "cases", mode, a + ".times"), "".join(f"{round(float(s) / speed)}\t{c}\n" for s, c in kept))

# every area as a list of (seconds before it, verdict line, times.tsv row or None), in play-back seconds
plays = {}
for a in areas:
    times = {}
    for l in rd(os.path.join(src, a, "times.tsv")).splitlines():
        s, _, c = l.partition("\t")
        times.setdefault(c.strip(), float(s))
    f = rng.uniform(0.75, 1.35) / speed
    steps = []
    for i, l in enumerate(rd(os.path.join(src, a, "cases.log")).splitlines()):
        m = re.match(r"^(IDENTICAL|DIFFERENT|ERROR|SKIPPED|EXPECTED) ([^|]*[^ |]) *(\|.*)?$", l)
        if not m:
            continue
        key, v = m.group(2), INJECT.get((a, i), m.group(1))
        if v != m.group(1):
            l = f"{v} {key} | made by replay.py from: {l}"
        secs = times.pop(key, None) if v != "SKIPPED" else None
        steps.append((secs * f if secs is not None else 0.0, l, key, secs is not None))
    total = float(rd(os.path.join(src, "rows", a + ".secs"))) * f
    steps.append((max(0.0, total - sum(s[0] for s in steps)), None, None, False))  # after the last case (the first one's time has the setup)
    plays[a] = steps

t0 = time.time()
events = []  # (when, what, area, step)
build = 60 / speed
for lane in lanes:
    t = t0 + build
    for a in lane:
        events.append((t, "start", a, None))
        for st in plays[a]:
            t += st[0]
            if st[1]:
                events.append((t, "case", a, st))
        events.append((t + 0.2, "end", a, None))
        t += 0.3
events.sort(key=lambda e: e[0])
lane_txt = "".join(f"lane {' '.join(l)}\n" for l in lanes)

def counts(a):
    c = {v: 0 for v in ("IDENTICAL", "DIFFERENT", "ERROR", "SKIPPED", "EXPECTED")}
    for l in rd(os.path.join(out, a, "cases.log")).splitlines():
        c[l.split(" ", 1)[0]] += 1
    return c

status_written = False
for when, what, a, st in events:
    if not status_written and when >= t0 + build:
        time.sleep(max(0, t0 + build - time.time()))
        wr(os.path.join(out, "run.status"), f"mode {mode}\nstart {int(t0)}\n{lane_txt}")
        status_written = True
    time.sleep(max(0, when - time.time()))
    d = os.path.join(out, a)
    if what == "start":
        shutil.rmtree(d, ignore_errors=True)
        os.makedirs(d)
        for n in ("cases.log", "times.tsv", "quick.re"):
            wr(os.path.join(d, n), "")
        wr(os.path.join(d, "last_case"), f"{int(time.time())}\n")
        first = next((s[2] for s in plays[a] if s[1]), "")
        wr(os.path.join(d, "now"), first + "\n")
        area_start = time.time()
        plays[a + "/start"] = area_start
    elif what == "case":
        _, line, key, timed = st
        wr(os.path.join(d, "cases.log"), line + "\n", "a")
        if timed:
            wr(os.path.join(d, "times.tsv"), f"{round(time.time() - int(rd(os.path.join(d, 'last_case'))))}\t{key}\n", "a")
        wr(os.path.join(d, "last_case"), f"{int(time.time())}\n")
        rest = [s[2] for s in plays[a] if s[1]]
        i = rest.index(key)
        if i + 1 < len(rest):
            wr(os.path.join(d, "now"), rest[i + 1] + "\n")
    else:
        c = counts(a)
        secs = round(time.time() - plays[a + "/start"])
        n = c["IDENTICAL"] + c["DIFFERENT"] + c["ERROR"]
        proof = f"PROOF {a} cases={n} identical={c['IDENTICAL']} different={c['DIFFERENT']} errors={c['ERROR']} skipped={c['SKIPPED']} expected={c['EXPECTED']}"
        wr(os.path.join(out, a + ".log"), rd(os.path.join(d, "cases.log")) + proof + "\n")
        wr(os.path.join(out, "rows", a + ".secs"), f"{secs}\n")
        wr(os.path.join(out, "rows", a + ".row"), f"| {a} | {n} | {c['IDENTICAL']} | {c['DIFFERENT']} | {c['ERROR']} | {c['SKIPPED']} | {c['EXPECTED']} | {secs // 60}m{secs % 60:02d}s |\n")

time.sleep(1)  # the ERROR cases again, alone (the made-up ERROR stays one)
secs = int(time.time() - t0)
table = "| area | cases | identical | different | errors | skipped | expected skips | time |\n|---|---|---|---|---|---|---|---|\n"
table += "".join(rd(os.path.join(out, "rows", a + ".row")) for a in areas)
table += f"| all ({mode}, wall clock) | | | | | | | {secs // 3600}h{secs % 3600 // 60:02d}m |\n"
wr(os.path.join(out, "table.md"), table)
wr(os.path.join(out, "errors.txt"), "".join(f"{a} {l[6:]}\n" for a in areas for l in rd(os.path.join(out, a, "cases.log")).splitlines() if l.startswith("ERROR ")))
wr(os.path.join(out, "skipped.txt"), "".join(l + "\n" for a in areas for l in rd(os.path.join(out, a, "cases.log")).splitlines() if l.startswith("SKIPPED ")))
wr(os.path.join(out, "vanished.txt"), "")
end = int(time.time())
wr(os.path.join(out, "run.status"), f"end {end}\n", "a")
print(f"replay: start {int(t0)} end {end} ({end - int(t0)} s, speed {speed})")
