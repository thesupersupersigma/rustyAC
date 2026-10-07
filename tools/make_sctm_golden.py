# SPDX-License-Identifier: MIT OR Apache-2.0

"""Sample the SCTM oracle CSVs into the small checked-in golden file for `cargo test`.

Usage (from the repository root, after generating the oracle data, see docs/port/sctm.md):
  python tools/make_sctm_golden.py

Reads  oracle/sctm/ks_ferrari_f2004_<axle>_<sweep>[_<tag>].csv
Writes crates/rustyac-physics/tests/golden/sctm_f2004.csv

Rows are taken evenly spaced through each file, so the result is the same on every run.
A `case` column is put in front; it tells the test which SCTM parameter set the row needs:
  base           the F2004 Slick Soft values from tyres.ini
  cf15           base + combinedFactor 1.5
  curves         base + the three synthetic lookup tables, linear camber table
  curves_smooth  base + the three tables, smooth camber table, combinedFactor 3
"""
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SRC = ROOT / "oracle" / "sctm"
OUT = ROOT / "crates" / "rustyac-physics" / "tests" / "golden" / "sctm_f2004.csv"
CAR = "ks_ferrari_f2004"

# (file suffix, case, rows per axle)
PLAN = [
    ("lateral", "base", 10),
    ("longitudinal", "base", 10),
    ("camber", "base", 10),
    ("combined", "base", 10),
    ("random", "base", 40),
    ("random_cf15", "cf15", 8),
    ("random_curves", "curves", 8),
    ("random_curves_smooth", "curves_smooth", 8),
]


def main():
    header = None
    out = []
    for axle in ("front", "rear"):
        for suffix, case, count in PLAN:
            lines = (SRC / f"{CAR}_{axle}_{suffix}.csv").read_text(encoding="utf-8").splitlines()
            if header is None:
                header = lines[0]
            assert lines[0] == header, suffix
            rows = lines[1:]
            # evenly spaced, offset so the first and last rows of a grid are not always picked
            picks = [rows[(2 * k + 1) * len(rows) // (2 * count)] for k in range(count)]
            out += [f"{case},{row}" for row in picks]
    OUT.write_text("case," + header + "\n" + "\n".join(out) + "\n", encoding="utf-8", newline="\n")
    print(f"{len(out)} rows -> {OUT.relative_to(ROOT)}")


if __name__ == "__main__":
    main()
