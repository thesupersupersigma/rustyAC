# kgl logs of the GUI sequences, D3D11 vs wgpu: equal except `fw1 atlas update` lines
import sys, re
d, w = sys.argv[1], sys.argv[2]
def lines(p):
    return [l for l in open(p, encoding='utf-8', errors='replace').read().split('\n') if not l.startswith('fw1 atlas update')]
a, b = lines(d), lines(w)
same = a == b
print(f'{len(a)} vs {len(b)} lines, {"SAME" if same else "DIFFERENT"}')
if not same:
    n = 0
    for i, (x, y) in enumerate(zip(a, b)):
        if x != y:
            print(i, 'd3d:', x[:160]); print(i, 'wgpu:', y[:160]); n += 1
            if n >= 6: break
