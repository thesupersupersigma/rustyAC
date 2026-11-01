# SPDX-License-Identifier: GPL-3.0-or-later
# Task 46: a Full-mode page drive (AC's launcher, Start) with AC's Default controls preset: Drive in the ESC menu, F1 to
# the cockpit, Ctrl+Numpad + twice and - once (field of view), Q and W held (glance left, right).
# usage: python cams_flow.py <out.json> <Default.ini>
import json, sys
out, controls = sys.argv[1], open(sys.argv[2]).read()
def key(code, vk, kind, mods=0):
    return {"cdp": "Input.dispatchKeyEvent", "params": {"type": kind, "code": code, "key": code, "windowsVirtualKeyCode": vk, "modifiers": mods}}
def press(code, vk, mods=0):
    return [key(code, vk, "rawKeyDown", mods), {"wait": 0.2}, key(code, vk, "keyUp", mods), {"wait": 0.3}]
def mouse(kind, x, y):
    return {"cdp": "Input.dispatchMouseEvent", "params": {"type": kind, "x": x, "y": y, "button": "left" if kind != "mouseMoved" else "none", "clickCount": 1}}
hud = "JSON.stringify((({camera,camera_settings,look,onboard_fov,head})=>({camera,camera_settings,look,onboard_fov,head}))(window.rustyac.hud||{}))"
ctrl = lambda code, vk: [key("ControlLeft", 0x11, "rawKeyDown", 2), *press(code, vk, 2), key("ControlLeft", 0x11, "keyUp")]
flow = [
    {"until": "launcher"}, {"wait": 3},
    {"js": "window.rustyac.loadControls(" + json.dumps(controls) + ", 'AC Default preset'), 'controls'"},
    {"js": "document.querySelector('#launcher iframe').contentWindow.focus(); 'focused'"},
    {"js": "(() => { const x = new (document.querySelector('#launcher iframe').contentWindow.XMLHttpRequest)(); x.open('GET', 'ac://start', false); x.send(); return x.responseText; })()"},
    {"until": "driving"}, {"wait": 6},
    mouse("mouseMoved", 50, 172), {"wait": 0.5}, mouse("mousePressed", 50, 172), {"wait": 0.2}, mouse("mouseReleased", 50, 172),
    {"wait": 4}, {"js": hud}, {"shot": out.replace(".json", "_drive.png")},
]
for _ in range(5):
    flow += press("F1", 0x70) + [{"js": hud}]
flow += [{"shot": out.replace(".json", "_f1.png")}]
flow += ctrl("NumpadAdd", 0x6b) + ctrl("NumpadAdd", 0x6b) + ctrl("NumpadSubtract", 0x6d) + [{"wait": 0.5}, {"js": hud}]
for code, vk in (("KeyQ", 0x51), ("KeyW", 0x57)):
    flow += [key(code, vk, "rawKeyDown"), {"wait": 1.5}, {"js": hud}, key(code, vk, "keyUp"), {"wait": 2}, {"js": hud}]
# the race.ini the launcher wrote (the desktop run takes the same)
flow += [{"js": "window.rustyac.launch.race_ini"}]
json.dump(flow, open(out, "w"), indent=1)
