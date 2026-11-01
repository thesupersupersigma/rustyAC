# SPDX-License-Identifier: GPL-3.0-or-later
# Task 46b: a Full-mode page drive (AC's launcher, Start): Drive in the ESC menu, then F5 (the
# on-board free camera through the fade), the left button dragged across the picture (theta and
# phi), the wheel two notches, F1 back to the cockpit.
# usage: python f5_flow.py <out.json>
import json, sys
out = sys.argv[1]
def key(code, vk, kind):
    return {"cdp": "Input.dispatchKeyEvent", "params": {"type": kind, "code": code, "key": code, "windowsVirtualKeyCode": vk}}
def press(code, vk):
    return [key(code, vk, "rawKeyDown"), {"wait": 0.2}, key(code, vk, "keyUp"), {"wait": 0.3}]
def mouse(kind, x, y, button="left", **more):
    return {"cdp": "Input.dispatchMouseEvent", "params": {"type": kind, "x": x, "y": y, "button": button, "clickCount": 1, **more}}
hud = "JSON.stringify((({camera,track_camera})=>({camera,track_camera}))(window.rustyac.hud||{}))"
flow = [
    {"until": "launcher"}, {"wait": 3},
    {"js": "document.querySelector('#launcher iframe').contentWindow.focus(); 'focused'"},
    {"js": "(() => { const x = new (document.querySelector('#launcher iframe').contentWindow.XMLHttpRequest)(); x.open('GET', 'ac://start', false); x.send(); return x.responseText; })()"},
    {"until": "driving"}, {"wait": 6},
    mouse("mouseMoved", 50, 172, "none"), {"wait": 0.5}, mouse("mousePressed", 50, 172), {"wait": 0.2}, mouse("mouseReleased", 50, 172),
    {"wait": 4}, {"js": hud},
]
flow += press("F5", 0x74) + [{"wait": 2}, {"js": hud}, {"shot": out.replace(".json", "_f5.png")}]
flow += [mouse("mouseMoved", 600, 400, "none"), mouse("mousePressed", 600, 400)]
for i in range(1, 11):
    flow += [{"wait": 0.05}, mouse("mouseMoved", 600 + 20 * i, 400 - 4 * i)]
flow += [mouse("mouseReleased", 800, 360), {"wait": 1}, {"js": hud}, {"shot": out.replace(".json", "_drag.png")}]
flow += [mouse("mouseWheel", 700, 400, "none", deltaX=0, deltaY=240), {"wait": 1}, {"shot": out.replace(".json", "_wheel.png")}]
flow += press("F1", 0x70) + [{"wait": 1}, {"js": hud}, {"shot": out.replace(".json", "_f1.png")}]
json.dump(flow, open(out, "w"), indent=1)
