# SPDX-License-Identifier: GPL-3.0-or-later
# Task 46c: a Full-mode page drive on an AC root whose camera_onboard_free.ini has
# SPHERICAL_COORDS=0 (F5's keyboard way, re/scratch/task46c/fakeac.sh): F5, then the camera's own
# keys (Up moves it, Numpad + widens the fov, Z lowers it), the left-button look on a locked
# pointer (moved past where the cursor would stop), let go (unlocked), held again and Esc
# (unlocked). The HUD's "cinematic" field and document.pointerLockElement after each.
# usage: python look_flow.py <out.json>
import json, sys
out = sys.argv[1]
def key(code, vk, kind):
    return {"cdp": "Input.dispatchKeyEvent", "params": {"type": kind, "code": code, "key": code, "windowsVirtualKeyCode": vk}}
def press(code, vk):
    return [key(code, vk, "rawKeyDown"), {"wait": 0.2}, key(code, vk, "keyUp"), {"wait": 0.3}]
def hold(code, vk, s):
    return [key(code, vk, "rawKeyDown"), {"wait": s}, key(code, vk, "keyUp"), {"wait": 0.3}]
def mouse(kind, x, y, button="left", **more):
    return {"cdp": "Input.dispatchMouseEvent", "params": {"type": kind, "x": x, "y": y, "button": button, "clickCount": 1, **more}}
look = "JSON.stringify({cinematic: (window.rustyac.hud||{}).cinematic, locked: document.pointerLockElement === document.getElementById('view'), focus: document.hasFocus(), lock_errors: window.lockErrors, lock_changes: window.lockChanges})"
watch = "window.lockErrors = 0; window.lockChanges = 0; document.addEventListener('pointerlockerror', () => window.lockErrors++); document.addEventListener('pointerlockchange', () => window.lockChanges++); 'watching'"
flow = [
    {"until": "launcher"}, {"wait": 3},
    {"js": "document.querySelector('#launcher iframe').contentWindow.focus(); 'focused'"},
    {"js": "(() => { const x = new (document.querySelector('#launcher iframe').contentWindow.XMLHttpRequest)(); x.open('GET', 'ac://start', false); x.send(); return x.responseText; })()"},
    {"until": "driving"}, {"wait": 6},
    mouse("mouseMoved", 50, 172, "none"), {"wait": 0.5}, mouse("mousePressed", 50, 172), {"wait": 0.2}, mouse("mouseReleased", 50, 172),
    {"wait": 4}, {"js": watch}, {"js": look},
]
flow += press("F5", 0x74) + [{"wait": 2}, {"js": look}]
flow += hold("ArrowUp", 0x26, 1.5) + [{"js": look}]
flow += hold("NumpadAdd", 0x6b, 1.0) + [{"js": look}]
flow += hold("KeyZ", 0x5a, 1.0) + [{"js": look}, {"shot": out.replace(".json", "_keys.png")}]
flow += [mouse("mouseMoved", 600, 400, "none"), {"wait": 0.3}, mouse("mousePressed", 600, 400), {"wait": 0.5}, {"js": look}]
for i in range(1, 41):
    flow += [{"wait": 0.03}, mouse("mouseMoved", 600 + 40 * i, 400, "left", buttons=1)]
flow += [{"wait": 1}, {"js": look}, {"shot": out.replace(".json", "_look.png")}]
flow += [mouse("mouseReleased", 2200, 400), {"wait": 0.5}, {"js": look}]
flow += [mouse("mousePressed", 600, 400), {"wait": 0.5}, {"js": look}] + press("Escape", 0x1b) + [{"wait": 1}, {"js": look}]
json.dump(flow, open(out, "w"), indent=1)
