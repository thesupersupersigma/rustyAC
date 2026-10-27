# Task 42c step 4: who calls timers and rAF in each document, by calling file (stack's caller),
# with the launcher shown, then during a Full drive, then back in the launcher.
# usage: mkcost.py <out flow.json> <shot prefix>
import json, sys
out, shots = sys.argv[1], sys.argv[2]  # a third argument: leave with Esc (Debug mode)
COUNT = r"""
(() => {
  const c = {};
  window.__cost = c;
  const where = () => { const l = (new Error().stack || '').split('\n')[3] || ''; const m = l.match(/(\/[^/:()]*\.(js|html))/); return m ? m[1] : l.trim().slice(0, 40); };
  const wrap = (obj, key) => { const f = obj[key]; if (f) obj[key] = function (...a) { const k = key + ' ' + where(); c[k] = (c[k] || 0) + 1; return f.apply(this, a); }; };
  for (const k of ['setTimeout', 'setInterval', 'requestAnimationFrame']) wrap(window, k);
})();
"""
WINDOW = r"""
(async () => {
  const docs = () => [['top', window], ...[...document.querySelectorAll('iframe')].map((f) => [f.id || 'iframe', f.contentWindow])];
  const take = () => Object.fromEntries(docs().map(([n, w]) => { try { return [n, { ...(w.__cost || {}) }]; } catch { return [n, null]; } }));
  const before = take();
  await new Promise((r) => setTimeout(r, 10000));
  const after = take();
  const d = {};
  for (const n of Object.keys(after)) { if (!after[n]) continue; for (const k of Object.keys(after[n])) { const v = after[n][k] - ((before[n] || {})[k] || 0); if (v) d[n + ': ' + k] = v; } }
  return JSON.stringify({ state: window.rustyac && window.rustyac.state, per_10s: d });
})()
"""
start = "(() => { const x = new (document.querySelector('#launcher iframe').contentWindow.XMLHttpRequest)(); x.open('GET', 'ac://start', false); x.send(); return x.responseText; })()"
flow = [
    {"cdp": "Page.addScriptToEvaluateOnNewDocument", "params": {"source": COUNT}},
    {"cdp": "Page.reload"},
    {"wait": 2},
    {"until": "launcher"},
    {"wait": 10},
    {"js": WINDOW},
    {"js": "document.querySelector('#launcher iframe').contentWindow.focus(); 'focused'"},
    {"js": start},
    {"until": "driving"},
    {"wait": 10},
    {"js": WINDOW},
    {"shot": shots + "_drive.jpg"},
    {"js": "JSON.stringify(window.rustyacTheme.requests.slice(-12))"},
    *([{"cdp": "Input.dispatchKeyEvent", "params": {"type": "rawKeyDown", "code": "Escape", "key": "Escape", "windowsVirtualKeyCode": 27}},
       {"cdp": "Input.dispatchKeyEvent", "params": {"type": "keyUp", "code": "Escape", "key": "Escape", "windowsVirtualKeyCode": 27}}]
      if len(sys.argv) > 3 else [{"js": "window.rustyac.leave && window.rustyac.leave(); 'left'"}]),
    {"until": "launcher"},
    {"wait": 10},
    {"js": WINDOW},
]
json.dump(flow, open(out, 'w'), indent=1)
