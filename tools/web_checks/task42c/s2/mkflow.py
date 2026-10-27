# Task 42c step 2: the page "b" (Full mode, autodriven) through a race's end: the results, its
# trip to the pit box, "Drive" refused until the next session, then "Drive" again.
# usage: mkflow.py <out.json> <run dir>
import json, sys
out, run = sys.argv[1], sys.argv[2]
hud = ("JSON.stringify((({kmh,gear,in_pit_lane,seconds,position})=>({kmh,gear,in_pit_lane,seconds,position}))(window.rustyac.hud||{}))"
       "+' log='+JSON.stringify(window.rustyac.log.filter((l)=>l.startsWith('online: the')||l.startsWith('online: a new')))")
def wait_log(text):  # resolves once the page's log has a line with `text`, then reads the HUD
    return {"js": f"new Promise((done)=>{{const t=setInterval(()=>{{if(window.rustyac.log.some((l)=>l.includes({json.dumps(text)}))){{clearInterval(t);done({hud})}}}},100)}})"}
def click(x, y):
    return [{"cdp": "Input.dispatchMouseEvent", "params": {"type": kind, "x": x, "y": y, "button": "left", "clickCount": 1}} for kind in ("mousePressed", "mouseReleased")]
drive = click(50, 172)  # the ESC menu's "Drive" at 1280 x 720
flow = [{"until": "driving"}, {"wait": 5}, {"js": "window.rustyac.notes"}, {"js": hud}, {"shot": f"{run}/b_start.png"},
        wait_log("the results came"), {"shot": f"{run}/b_results.png"},
        wait_log("goes to its pit box"), {"wait": 2}, {"js": hud}, {"shot": f"{run}/b_pits.png"},
        *drive, {"wait": 4}, {"js": hud}, {"shot": f"{run}/b_drive_refused.png"},
        wait_log("a new session"), {"wait": 2}, {"js": hud}, {"shot": f"{run}/b_new_session.png"}, *drive, *[s for _ in range(10) for s in ({"wait": 4}, {"js": hud})], {"shot": f"{run}/b_drive_again.png"},
        {"js": "window.rustyac.leave(); 'left'"}]
json.dump(flow, open(out, 'w'), indent=1)
