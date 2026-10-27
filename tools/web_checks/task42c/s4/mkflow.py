# Task 42c step 4: the page (Full mode, on a local server) with F7 (free camera; the arrows
# move it), F1 (back), F4 (the fade back to your car), then full screen: Esc and Esc held.
# usage: mkflow.py <out.json> <run dir>
import json, sys
out, run = sys.argv[1], sys.argv[2]
def key(code, vk, kind="rawKeyDown", repeat=False):
    return {"cdp": "Input.dispatchKeyEvent", "params": {"type": kind, "code": code, "key": code, "windowsVirtualKeyCode": vk, "autoRepeat": repeat}}
def press(code, vk):
    return [key(code, vk), key(code, vk, "keyUp")]
state = ("JSON.stringify({fs: !!document.fullscreenElement, lock: !!(navigator.keyboard && navigator.keyboard.lock), state: window.rustyac.state,"
         " hud: window.rustyac.hud && window.rustyac.hud.kmh})")
flow = [{"until": "driving"}, {"wait": 6}, {"shot": f"{run}/0_drive.png"},
        *press("F7", 0x76), {"wait": 1}, {"shot": f"{run}/1_free.png"},
        key("ArrowUp", 0x26), {"wait": 0.3}, key("ArrowUp", 0x26, "keyUp"), {"wait": 0.5}, {"shot": f"{run}/2_free_moved.png"},
        *press("F1", 0x70), {"wait": 1}, {"shot": f"{run}/3_f1.png"},
        *press("F4", 0x73), {"shot": f"{run}/4_f4_fade.jpg"}, {"shot": f"{run}/4_f4_fade2.jpg"}, {"wait": 2}, {"shot": f"{run}/5_f4_after.png"},
        {"cdp": "Runtime.evaluate", "params": {"expression": "document.documentElement.requestFullscreen().then(() => 'full screen', (e) => String(e))", "userGesture": True, "awaitPromise": True}},
        {"wait": 1}, {"js": state},
        *press("Escape", 0x1b), {"wait": 1}, {"js": state}, {"shot": f"{run}/6_fs_esc.png"},
        *press("Escape", 0x1b), {"wait": 1},
        key("Escape", 0x1b), *[key("Escape", 0x1b, repeat=True) for _ in range(12)], {"wait": 2.5}, key("Escape", 0x1b, "keyUp"), {"wait": 1}, {"js": state}, {"shot": f"{run}/7_fs_held.png"},
        {"js": "window.rustyac.leave(); 'left'"}]
json.dump(flow, open(out, 'w'), indent=1)
