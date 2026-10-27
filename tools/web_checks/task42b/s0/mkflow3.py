# step 0: a race.ini drive from the page's own menu (no launcher): AC's ESC menu, a real click
# on Drive, then real keys. usage: mkflow3.py <name> <race.ini> <seconds to wait after Drive>
import json, sys
S = 're/scratch/task42b/s0'
name, race, after = sys.argv[1], open(sys.argv[2]).read(), float(sys.argv[3])
assists = open('re/gold/ai_page/assists.ini').read()
def key(code, vk, kind):
    return {"cdp": "Input.dispatchKeyEvent", "params": {"type": kind, "code": code, "key": code, "windowsVirtualKeyCode": vk}}
def mouse(kind, x, y):
    return {"cdp": "Input.dispatchMouseEvent", "params": {"type": kind, "x": x, "y": y, "button": "left" if kind != "mouseMoved" else "none", "clickCount": 1}}
hud = "JSON.stringify((({kmh,gear,in_pit_lane,camera,seconds})=>({kmh,gear,in_pit_lane,camera,seconds}))(window.rustyac.hud||{}))+' top='+((window.rustyac.hud||{}).referee||{top:[]}).top[0]+' focus='+(document.activeElement&&document.activeElement.id)"
flow = [
    {"until": "menu"},
    {"js": "(async () => { for (let i = 0; i < 600 && document.getElementById('drive').disabled; i++) await new Promise((r) => setTimeout(r, 100)); document.getElementById('drive').focus(); window.rustyac.driveRace(" + json.dumps(race) + ", " + json.dumps(assists) + "); return 'started'; })()"},
    {"until": "driving"}, {"wait": 6}, {"shot": f"{S}/{name}_menu.png"},
    key("ArrowUp", 38, "rawKeyDown"), {"wait": 3}, {"js": hud}, key("ArrowUp", 38, "keyUp"),
    mouse("mouseMoved", 50, 172), {"wait": 0.5}, mouse("mousePressed", 50, 172), {"wait": 0.2}, mouse("mouseReleased", 50, 172),
    {"wait": after}, {"shot": f"{S}/{name}_drive.png"}, {"js": hud},
    key("ArrowUp", 38, "rawKeyDown"), {"wait": 4}, {"js": hud}, key("ArrowUp", 38, "keyUp"),
    key("F1", 0x70, "rawKeyDown"), key("F1", 0x70, "keyUp"), {"wait": 1}, {"js": hud},
    {"shot": f"{S}/{name}_f1.png"},
]
json.dump(flow, open(f'{S}/{name}_flow.json', 'w'), indent=1)
