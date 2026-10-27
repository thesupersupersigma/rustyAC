# Task 42b step 0b: flows that measure the page's cost in three states (check.mjs --flow).
# usage: mkperf.py <name> idle|drive  -> re/scratch/task42b/s0b/<name>_flow.json
# Each measure: CDP Performance metrics before/after a 20 s window, the page's frame
# intervals (rAF), long tasks, and the timer/rAF calls of every frame (counted by a script
# put into each document before it runs).
import json, sys
S = 're/scratch/task42b/s0b'
name, what = sys.argv[1], sys.argv[2]
COUNT = r"""
(() => {
  const c = { timeout: 0, interval: 0, raf: 0, fetch: 0, xhr: 0 };
  window.__cost = c;
  const wrap = (obj, key, field) => { const f = obj[key]; if (f) obj[key] = function (...a) { c[field]++; return f.apply(this, a); }; };
  wrap(window, 'setTimeout', 'timeout'); wrap(window, 'setInterval', 'interval');
  wrap(window, 'requestAnimationFrame', 'raf'); wrap(window, 'fetch', 'fetch');
  if (window.XMLHttpRequest) wrap(XMLHttpRequest.prototype, 'send', 'xhr');
})();
"""
WINDOW = r"""
(async () => {
  const frames = document.querySelectorAll('iframe');
  const counts = () => [window.__cost, ...[...frames].map((f) => { try { return f.contentWindow.__cost; } catch { return null; } })].map((c) => c && { ...c });
  const before = counts();
  const gaps = []; let last = performance.now(), go = true;
  const tick = (t) => { gaps.push(t - last); last = t; if (go) requestAnimationFrame(tick); };
  requestAnimationFrame(tick);
  let long = 0, longMs = 0;
  const obs = new PerformanceObserver((l) => { for (const e of l.getEntries()) { long++; longMs += e.duration; } });
  try { obs.observe({ type: 'longtask', buffered: false }); } catch {}
  await new Promise((r) => setTimeout(r, 20000));
  go = false; obs.disconnect();
  const after = counts();
  gaps.sort((a, b) => a - b);
  const pct = (p) => gaps[Math.min(gaps.length - 1, Math.floor(gaps.length * p))];
  const diff = after.map((a, i) => a && before[i] && Object.fromEntries(Object.keys(a).map((k) => [k, a[k] - before[i][k]])));
  return JSON.stringify({ state: window.rustyac && window.rustyac.state, frames: gaps.length, frame_ms_mean: gaps.reduce((s, g) => s + g, 0) / gaps.length, frame_ms_p95: pct(0.95), frame_ms_max: gaps[gaps.length - 1], long_tasks: long, long_ms: longMs, calls_per_20s: diff, heap_mb: performance.memory ? performance.memory.usedJSHeapSize / 2 ** 20 : null, wasm_mb: window.rustyac && window.rustyac.memory ? window.rustyac.memory() : null, fps: window.rustyac && window.rustyac.perf && window.rustyac.perf.fps });
})()
"""
metrics = {"cdp": "Performance.getMetrics"}
measure = [metrics, {"js": WINDOW}, metrics]
flow = [
    {"cdp": "Page.addScriptToEvaluateOnNewDocument", "params": {"source": COUNT}},
    {"cdp": "Performance.enable", "params": {"timeDomain": "timeTicks"}},
    {"cdp": "Page.reload"},
    {"wait": 2},
    {"until": "launcher"},
    {"wait": 15},
]
if what == 'idle':
    flow += measure + [{"shot": f"{S}/{name}.jpg"}]
elif what == 'back':
    # a Debug drive, then Esc (no AC GUI: back to the launcher), then the launcher measured
    flow += [
        {"js": "document.querySelector('#launcher iframe').contentWindow.focus(); 'focused'"},
        {"js": "(() => { const x = new (document.querySelector('#launcher iframe').contentWindow.XMLHttpRequest)(); x.open('GET', 'ac://start', false); x.send(); return x.responseText; })()"},
        {"until": "driving"},
        {"wait": 10},
        {"cdp": "Input.dispatchKeyEvent", "params": {"type": "rawKeyDown", "code": "Escape", "key": "Escape", "windowsVirtualKeyCode": 27}},
        {"cdp": "Input.dispatchKeyEvent", "params": {"type": "keyUp", "code": "Escape", "key": "Escape", "windowsVirtualKeyCode": 27}},
        {"until": "launcher"},
        {"wait": 10},
    ] + measure + [{"shot": f"{S}/{name}.jpg"}]
else:
    flow += [
        {"js": "document.querySelector('#launcher iframe').contentWindow.focus(); 'focused'"},
        {"js": "(() => { const x = new (document.querySelector('#launcher iframe').contentWindow.XMLHttpRequest)(); x.open('GET', 'ac://start', false); x.send(); return x.responseText; })()"},
        {"until": "driving"},
        {"wait": 15},
    ] + measure + [{"shot": f"{S}/{name}.jpg"}]
json.dump(flow, open(f'{S}/{name}_flow.json', 'w'), indent=1)
