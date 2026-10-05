"""Read decompiler output that the kawaiidra MCP cached (~/.kawaiidra/cache).

Usage:
  re_cache.py list                 -> address, lines, qualified name of every cached function
  re_cache.py show <addr> [a] [b]  -> print code lines a..b of the function at <addr>
  re_cache.py grep <regex> [addr]  -> grep across cached code (or one function)
"""
import json, re, sys
from pathlib import Path

CACHE = Path.home() / ".kawaiidra" / "cache"


def load():
    out = {}
    for p in CACHE.rglob("*get_function_decompile*.json"):
        try:
            d = json.loads(p.read_text(encoding="utf-8"))["data"]
        except Exception:
            continue
        code = d["code"].replace("\r\n", "\n")
        out[d["address"].lower()] = {"name": d["function"], "sig": d["signature"],
                                     "code": code, "qual": qualified(code, d["function"])}
    return out


def qualified(code, name):
    """Namespace-qualified name taken from the definition line of the pseudo-C."""
    head = code.split("{", 1)[0]
    m = re.findall(r"([A-Za-z_~][\w:<>,~ \*&]*::" + re.escape(name) + r")\s*\(", head)
    if m:
        return m[-1].split()[-1].lstrip("*&")
    return name


if __name__ == "__main__":
    fns = load()
    cmd = sys.argv[1] if len(sys.argv) > 1 else "list"
    if cmd == "list":
        for a in sorted(fns):
            print(a, len(fns[a]["code"].splitlines()), fns[a]["qual"])
    elif cmd == "show":
        a = sys.argv[2].lower().replace("0x", "")
        lines = fns[a]["code"].splitlines()
        lo = int(sys.argv[3]) if len(sys.argv) > 3 else 0
        hi = int(sys.argv[4]) if len(sys.argv) > 4 else len(lines)
        for i in range(lo, min(hi, len(lines))):
            print(f"{i:4d} {lines[i]}")
    elif cmd == "grep":
        rx = re.compile(sys.argv[2])
        only = sys.argv[3].lower().replace("0x", "") if len(sys.argv) > 3 else None
        for a in sorted(fns):
            if only and a != only:
                continue
            for i, l in enumerate(fns[a]["code"].splitlines()):
                if rx.search(l):
                    print(f"{a} {fns[a]['qual']}:{i}: {l.strip()}")
