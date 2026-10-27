# Task 42b step 1: the page "b" that joins a race in progress: what it shows, a real throttle
# key, F1, then the race's end and the next race (every 20 s). usage: mkflow.py <out.json> <run dir>
import json, sys
out, run = sys.argv[1], sys.argv[2]
def key(code, vk, kind):
    return {"cdp": "Input.dispatchKeyEvent", "params": {"type": kind, "code": code, "key": code, "windowsVirtualKeyCode": vk}}
hud = ("JSON.stringify((({kmh,gear,in_pit_lane,camera,seconds,referee})=>({kmh,gear,in_pit_lane,camera,seconds,top:referee&&referee.top}))(window.rustyac.hud||{}))"
       "+' drivers='+JSON.stringify(window.rustyac.online&&window.rustyac.online.drivers.map((d)=>[d.name,d.laps]))")
flow = [{"until": "driving"}, {"wait": 8}, {"shot": f"{run}/b_join.png"}, {"js": hud},
        key("ArrowUp", 38, "rawKeyDown"), {"wait": 5}, {"js": hud}, key("ArrowUp", 38, "keyUp"),
        key("F1", 0x70, "rawKeyDown"), key("F1", 0x70, "keyUp"), {"wait": 1}, {"js": hud}, {"shot": f"{run}/b_drive.png"},
        key("ArrowDown", 40, "rawKeyDown"), {"wait": 4}, key("ArrowDown", 40, "keyUp")]
for i in range(12):
    flow += [{"wait": 20}, {"js": hud}]
    if True:
        flow.append({"shot": f"{run}/b_t{i}.png"})
flow += [key("ArrowUp", 38, "rawKeyDown"), {"wait": 4}, {"js": hud}, key("ArrowUp", 38, "keyUp"), {"shot": f"{run}/b_end.png"},
         {"js": "window.rustyac.leave(); 'left'"}]
json.dump(flow, open(out, 'w'), indent=1)
