"""Read-only: survey an .ini file across every extracted car in cardata/.

Usage:
  ini_survey.py <file.ini> [section-regex] [key-regex]
      -> per SECTION / KEY: how many cars have it and the most common values
  ini_survey.py --files            -> every data file name and how many cars ship it
  ini_survey.py --value <file.ini> <SECTION> <KEY>   -> value per car

Section names with a numeric suffix (WING_0, WING_1, GEAR_3 ...) are folded to WING_n.
"""
import collections, os, re, sys

ROOT = os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))), "cardata")


def cars():
    return sorted(d for d in os.listdir(ROOT) if os.path.isdir(os.path.join(ROOT, d)) and not d.startswith("_"))


def read(path):
    raw = open(path, "rb").read()
    for enc in ("utf-8-sig", "latin1"):
        try:
            return raw.decode(enc)
        except UnicodeDecodeError:
            pass
    return raw.decode("latin1", "replace")


def parse(text):
    sec, out = None, []
    for line in text.splitlines():
        line = line.split(";")[0].strip()
        if not line:
            continue
        m = re.match(r"\[(.+?)\]", line)
        if m:
            sec = m.group(1).strip()
            continue
        if "=" in line and sec:
            k, v = line.split("=", 1)
            out.append((sec, k.strip(), v.strip()))
    return out


def fold(sec):
    return re.sub(r"_\d+$", "_n", sec)


def find(car, name):
    d = os.path.join(ROOT, car)
    for f in os.listdir(d):
        if f.lower() == name.lower():
            return os.path.join(d, f)
    return None


if __name__ == "__main__":
    a = sys.argv[1:]
    if not a:
        sys.exit(__doc__)
    if a[0] == "--files":
        c = collections.Counter()
        for car in cars():
            for f in os.listdir(os.path.join(ROOT, car)):
                c[re.sub(r"\d+", "#", f.lower()) if f.lower().endswith((".lut", ".rto")) else f.lower()] += 1
        for f, n in sorted(c.items(), key=lambda x: (-x[1], x[0])):
            print("%4d  %s" % (n, f))
        sys.exit()
    if a[0] == "--value":
        for car in cars():
            p = find(car, a[1])
            vals = [v for s, k, v in parse(read(p)) if s == a[2] and k == a[3]] if p else []
            print("%-40s %s" % (car, vals[0] if vals else "-"))
        sys.exit()
    name = a[0]
    srx = re.compile(a[1]) if len(a) > 1 else None
    krx = re.compile(a[2]) if len(a) > 2 else None
    have = 0
    keys = collections.OrderedDict()
    for car in cars():
        p = find(car, name)
        if not p:
            continue
        have += 1
        seen = set()
        for s, k, v in parse(read(p)):
            fs = fold(s)
            if srx and not srx.search(fs):
                continue
            if krx and not krx.search(k):
                continue
            e = keys.setdefault((fs, k), [0, collections.Counter()])
            if (fs, k) not in seen:
                e[0] += 1
                seen.add((fs, k))
            e[1][v] += 1
    print("%s: present in %d of %d cars" % (name, have, len(cars())))
    last = None
    for (s, k), (n, vals) in keys.items():
        if s != last:
            print("[%s]" % s)
            last = s
        top = ", ".join("%s (%d)" % (v[:28], c) for v, c in vals.most_common(5))
        print("  %-34s cars %3d   %s" % (k, n, top))
