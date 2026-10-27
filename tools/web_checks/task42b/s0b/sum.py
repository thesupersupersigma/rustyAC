# Task 42b step 0b: one line per measured run (main-thread ms per s, frames, heap, calls).
import json, sys
for path in sys.argv[1:]:
    r = json.load(open(path))
    flow = r.get('flow', [])
    bad = [s for s in flow if 'failed' in s]
    mets = [dict((m['name'], m['value']) for m in s['value']['metrics']) for s in flow if s.get('cdp') == 'Performance.getMetrics']
    win = next((json.loads(s['value']) for s in flow if s.get('js', '').startswith('\n(async () => {\n  const frames')), None)
    if bad or len(mets) < 2 or not win:
        print(path, 'FAILED', bad[:1] or flow[-1:])
        continue
    a, b = mets[0], mets[1]
    secs = b['Timestamp'] - a['Timestamp']
    per = lambda k: (b[k] - a[k]) / secs * 1000
    calls = [c for c in win['calls_per_20s'] if c]
    total = {k: sum(c[k] for c in calls) for k in calls[0]} if calls else {}
    print(f"{path}: state={win['state']} main-thread {per('TaskDuration'):.0f} ms/s (script {per('ScriptDuration'):.0f}, layout {per('LayoutDuration'):.1f}, style {per('RecalcStyleDuration'):.1f}); "
          f"frame mean {win['frame_ms_mean']:.1f} ms p95 {win['frame_ms_p95']:.1f} max {win['frame_ms_max']:.0f}; long tasks {win['long_tasks']} ({win['long_ms']:.0f} ms); "
          f"JS heap {b['JSHeapUsedSize'] / 2**20:.0f} MB, nodes {b['Nodes']:.0f}, wasm {win['wasm_mb']}; calls/20s {total}; game fps {win['fps']}")
