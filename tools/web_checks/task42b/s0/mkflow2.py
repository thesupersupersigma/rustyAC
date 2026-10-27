# step 0: the launcher's Start, AC's ESC menu, a real click on Drive, then real keys
import json, sys
S = 're/scratch/task42b/s0'
name, query_note = sys.argv[1], ''
def key(code, vk, kind):
    return {"cdp": "Input.dispatchKeyEvent", "params": {"type": kind, "code": code, "key": code, "windowsVirtualKeyCode": vk}}
def mouse(kind, x, y):
    return {"cdp": "Input.dispatchMouseEvent", "params": {"type": kind, "x": x, "y": y, "button": "left" if kind != "mouseMoved" else "none", "clickCount": 1}}
hud = "JSON.stringify((({kmh,gear,in_pit_lane,camera,seconds})=>({kmh,gear,in_pit_lane,camera,seconds}))(window.rustyac.hud||{}))+' focus='+(document.activeElement&&document.activeElement.id)"
flow = [
    {"until": "launcher"}, {"wait": 3},
    {"js": "document.querySelector('#launcher iframe').contentWindow.focus(); 'focused'"},
    {"js": "(() => { const x = new (document.querySelector('#launcher iframe').contentWindow.XMLHttpRequest)(); x.open('GET', 'ac://start', false); x.send(); return x.responseText; })()"},
    {"until": "driving"},
    {"js": "window.rustyac.drive ? JSON.stringify(window.rustyac.drive) : 'no drive'"},
    {"wait": 6}, {"shot": f"{S}/{name}_menu.png"},
    key("ArrowUp", 38, "rawKeyDown"), {"wait": 3}, {"js": hud}, key("ArrowUp", 38, "keyUp"),
    mouse("mouseMoved", 50, 172), {"wait": 0.5}, mouse("mousePressed", 50, 172), {"wait": 0.2}, mouse("mouseReleased", 50, 172),
    {"wait": 3}, {"shot": f"{S}/{name}_drive.png"},
    key("ArrowUp", 38, "rawKeyDown"), {"wait": 4}, {"js": hud}, key("ArrowUp", 38, "keyUp"),
    key("F1", 0x70, "rawKeyDown"), key("F1", 0x70, "keyUp"), {"wait": 1}, {"js": hud},
    {"shot": f"{S}/{name}_f1.png"},
]
json.dump(flow, open(f'{S}/{name}_flow.json', 'w'), indent=1)
