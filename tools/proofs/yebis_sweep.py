# SPDX-License-Identifier: GPL-3.0-or-later
#
# The rows of yebis_sweep.sh (Task Y7d), one "<name> | <render_oracle compare args> <KEY:value>..." a line:
#   x_*  every PP quality x glare x DOF x light shafts at 1280x720, default filter (6 x 6 x 6 x 2 = 432)
#   p*_* pairwise coverage of those plus heat shimmer, FXAA, AA and triple screens, at two more sizes
#   f_*  every filter AC ships (system/cfg/ppfilters), once each at AC's highest settings
# python yebis_sweep.py <AC folder>
import itertools
import os
import sys

BASE = ('--track spa --view sun --post --car ks_ferrari_f2004 --tape oracle/audio/ks_ferrari_f2004/trk_lap.audiotape'
        ' --tape-from 480 --frames 3')


def keys(pp, glare, dof, rays, heat=0):
    # DOF:<q>,5: a 5 m focus, so DOF has something to blur (the camera's dofFactor 1, Task Y6)
    return f'PP_QUALITY:{pp} GLARE:{glare} DOF:{dof}{",5" if dof else ""}{" RAYS:1" if rays else ""}{" HEAT_SHIMMER:1" if heat else ""}'


def pairwise(factors):
    """Greedy all-pairs: each next row covers the most pairs not covered yet (first such row in order)."""
    names = list(factors)
    todo = {(a, va, b, vb) for a, b in itertools.combinations(names, 2) for va in factors[a] for vb in factors[b]}
    every = [dict(zip(names, vals)) for vals in itertools.product(*factors.values())]
    rows = []
    while todo:
        def gain(row):
            return sum((a, row[a], b, row[b]) in todo for a, b in itertools.combinations(names, 2))
        best = max(every, key=gain)
        todo -= {(a, best[a], b, best[b]) for a, b in itertools.combinations(names, 2)}
        rows.append(best)
    # self-check: every pair of values of every two settings is in some row
    for a, b in itertools.combinations(names, 2):
        assert {(r[a], r[b]) for r in rows} == set(itertools.product(factors[a], factors[b])), (a, b)
    return rows


def main():
    for pp, glare, dof, rays in itertools.product(range(6), range(6), range(6), (0, 1)):
        print(f'x_pp{pp}_g{glare}_d{dof}_r{rays} | {BASE} --size 1280x720 --ppfilter default {keys(pp, glare, dof, rays)}')
    rows = pairwise({'pp': range(6), 'glare': range(6), 'dof': range(6), 'rays': (0, 1), 'heat': (0, 1),
                     'fxaa': (0, 1), 'aa': (1, 2, 4), 'triple': (0, 1)})
    for size in ('1127x633', '1920x1080'):
        for i, r in enumerate(rows):
            extra = ('--fxaa ' if r['fxaa'] else '') + f'--aa {r["aa"]} ' + ('--triple ' if r['triple'] else '')
            print(f'p{size}_{i:02d} | {BASE} --size {size} --ppfilter default {extra}{keys(r["pp"], r["glare"], r["dof"], r["rays"], r["heat"])}')
    for f in sorted(os.listdir(os.path.join(sys.argv[1], 'system/cfg/ppfilters'))):
        if f.endswith('.ini'):
            name = f[:-4]
            print(f'f_{name} | {BASE} --size 1280x720 --ppfilter {name} --fxaa --aa 4 {keys(5, 5, 5, 1, 1)}')


main()
