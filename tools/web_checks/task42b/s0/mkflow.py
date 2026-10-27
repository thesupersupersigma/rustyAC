# writes the flows of step 0: the launcher's Start, then real keys
import json, sys
S = 're/scratch/task42b/s0'
def key(code, vk, kind):
    return {"cdp": "Input.dispatchKeyEvent", "params": {"type": kind, "code": code, "key": code, "windowsVirtualKeyCode": vk}}
hud = "JSON.stringify((({kmh,gear,in_pit_lane,camera,seconds})=>({kmh,gear,in_pit_lane,camera,seconds}))(window.rustyac.hud||{}))+' focus='+(document.activeElement&&document.activeElement.id)+' hasFocus='+document.hasFocus()"
start = [
    {"until": "launcher"},
    {"wait": 3},
    {"js": "document.querySelector('#launcher iframe').contentWindow.focus(); 'focused'"},
    {"js": "(() => { const x = new (document.querySelector('#launcher iframe').contentWindow.XMLHttpRequest)(); x.open('GET', 'ac://start', false); x.send(); return x.responseText; })()"},
    {"until": "driving"},
    {"js": "JSON.stringify(window.rustyac.launch)"},
    {"wait": 6},
    {"shot": f"{S}/l_start.png"},
    {"js": hud},
    key("ArrowUp", 38, "rawKeyDown"), {"wait": 4}, {"js": hud}, key("ArrowUp", 38, "keyUp"),
    {"shot": f"{S}/l_gas.png"},
]
json.dump(start, open(f'{S}/l_flow.json', 'w'), indent=1)
