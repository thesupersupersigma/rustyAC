# SPDX-License-Identifier: GPL-3.0-or-later
# Task W1: the page's way back to AC's launcher and its reloads, as a web/check.mjs --flow.
# usage: roundtrip_flow.py <out.json> <trips> <reloads> <car@track>
#   trips    launcher -> Start -> drive (sound) -> Esc, Esc, AC's Exit -> launcher; after each:
#            the drive's sound, FMOD's bank files left, memory, the browser's processes
#   reloads  the browser's reload from the launcher, then from a drive, each back to the launcher
# (1280 x 720: AC's ESC menu's Exit at (355, 322). The theme keeps its own car and track: the
# page's reads of race.ini after Start get car@track.)
import json, sys
out, trips, reloads, choice = sys.argv[1], int(sys.argv[2]), int(sys.argv[3]), sys.argv[4]
car, track = choice.split('@')


def key(code, vk):
    base = {"code": code, "key": code, "windowsVirtualKeyCode": vk}
    return [{"cdp": "Input.dispatchKeyEvent", "params": {"type": t, **base}} for t in ("rawKeyDown", "keyUp")]


def click(x, y):
    p = {"x": x, "y": y, "button": "left", "clickCount": 1}
    return [{"cdp": "Input.dispatchMouseEvent", "params": {"type": "mouseMoved", "x": x, "y": y}}, {"wait": 0.3},
            {"cdp": "Input.dispatchMouseEvent", "params": {"type": "mousePressed", **p}}, {"wait": 0.1},
            {"cdp": "Input.dispatchMouseEvent", "params": {"type": "mouseReleased", **p}}]


pick = {"js": r"(() => { const l = window.rustyacTheme.launcher; const read = l.read_doc.bind(l); l.read_doc = (name) => { const b = read(name);"
              r" if (!b || name.toLowerCase() !== 'cfg\\race.ini') return b; return new TextEncoder().encode(new TextDecoder().decode(b)"
              f".replace(/^MODEL=.*$/gm, 'MODEL={car}').replace(/^TRACK=.*$/gm, 'TRACK={track}').replace(/^CONFIG_TRACK=.*$/gm, 'CONFIG_TRACK=').replace(/^SKIN=.*$/gm, 'SKIN='));"
              r" }; return 'picked'; })()"}
start = {"js": "(() => { const w = document.querySelector('#launcher iframe').contentWindow; w.focus(); const x = new w.XMLHttpRequest(); x.open('GET', 'ac://start', false); x.send(); return x.responseText; })()"}
sound = {"js": "'sound ' + !!(window.rustyac.sound && window.rustyac.sound.state.studio)"}
after = {"js": "JSON.stringify({ trip: true, state: window.rustyac.state, bank_files: window.rustyac.sound ? window.rustyac.sound.state.bankFiles.size : null, js_mb: Math.round(performance.memory.usedJSHeapSize / 2 ** 20), wasm_mb: Math.round(window.rustyac.memory().wasm_mb) })"}
drive = [pick, start, {"until": "driving", "timeout": 600}, {"wait": 6}]

flow = [{"until": "launcher", "timeout": 300}, {"wait": 3}]
for _ in range(trips):
    flow += drive + [sound] + key("Escape", 27) + [{"wait": 2}] + key("Escape", 27) + [{"wait": 2}] + click(355, 322)
    flow += [{"until": "launcher", "timeout": 120}, {"wait": 3}, after, {"processes": True}]
for where in ("launcher", "drive"):
    for _ in range(reloads):
        if where == "drive":
            flow += drive
        flow += [{"reload": True, "timeout": 60}, {"until": "launcher", "timeout": 180}, {"wait": 2}]
json.dump(flow, open(out, "w"), indent=1)
