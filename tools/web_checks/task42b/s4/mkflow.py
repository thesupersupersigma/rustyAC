# Task 42b steps 3-4: real (trusted) keys and mouse buttons in a Full drive started from AC's
# launcher. Each browser key must reach the game, not the browser (the page stays, no reload,
# no history step); F1 / F6 change the camera; beforeunload asks during the drive and not in
# the launcher; the loading screen shows while the drive loads.
# usage: mkflow.py <out.json> <shots dir>
import json, sys
out, shots = sys.argv[1], sys.argv[2]
MODS = {'Alt': 1, 'Ctrl': 2, 'Shift': 8}
VK = {'ArrowLeft': 0x25, 'ArrowRight': 0x27, 'ArrowUp': 0x26, 'Backspace': 0x08, 'F1': 0x70, 'F3': 0x72, 'F5': 0x74, 'F6': 0x75,
      'F11': 0x7a, 'KeyR': 0x52, 'KeyF': 0x46, 'KeyP': 0x50, 'KeyS': 0x53, 'KeyD': 0x44, 'BrowserBack': 0xa6}
def press(name):
    parts = name.split('+')
    code = parts[-1]
    mods = sum(MODS[m] for m in parts[:-1])
    key = {'Backspace': 'Backspace', 'BrowserBack': 'BrowserBack'}.get(code, code[3:].lower() if code.startswith('Key') else code)
    base = {'code': code, 'key': key, 'windowsVirtualKeyCode': VK[code], 'modifiers': mods}
    return [{"cdp": "Input.dispatchKeyEvent", "params": {"type": "rawKeyDown", **base}},
            {"cdp": "Input.dispatchKeyEvent", "params": {"type": "keyUp", **base}}, {"wait": 0.4}]
def button(n):
    name = {3: 'back', 4: 'forward'}[n]
    p = {"x": 640, "y": 360, "button": name, "buttons": 8 if n == 3 else 16, "clickCount": 1}
    return [{"cdp": "Input.dispatchMouseEvent", "params": {"type": "mousePressed", **p}},
            {"cdp": "Input.dispatchMouseEvent", "params": {"type": "mouseReleased", **p}}, {"wait": 0.4}]
# what the page is: the same document (a marker on window), where, how many history entries
where = "JSON.stringify({ same: window.__mark === 42, state: window.rustyac.state, url: location.href.slice(-40), history: history.length, camera: (window.rustyac.hud || {}).camera, kmh: (window.rustyac.hud || {}).kmh })"
unload = "(() => { const e = new Event('beforeunload', { cancelable: true }); window.dispatchEvent(e); return e.defaultPrevented; })()"
seen_loading = "window.__loadingSeen = []; window.__loadingTimer = setInterval(() => { const t = document.getElementById('loading-text'); if (t && !document.getElementById('loading').hidden) { const s = t.textContent; if (window.__loadingSeen[window.__loadingSeen.length - 1] !== s) window.__loadingSeen.push(s); } }, 100); 'watching'"
flow = [{"until": "launcher"}, {"wait": 3},
        {"js": "window.__mark = 42; 'marked'"},
        {"js": "'launcher beforeunload asks: ' + " + unload},
        {"js": seen_loading},
        {"js": "document.querySelector('#launcher iframe').contentWindow.focus(); 'focused'"},
        {"js": "(() => { const x = new (document.querySelector('#launcher iframe').contentWindow.XMLHttpRequest)(); x.open('GET', 'ac://start', false); x.send(); return x.responseText; })()"},
        {"wait": 2}, {"shot": f"{shots}/loading.png"},
        {"until": "driving"},
        {"js": "clearInterval(window.__loadingTimer); JSON.stringify({ loading_lines: window.__loadingSeen.slice(0, 12), count: window.__loadingSeen.length, hidden_now: document.getElementById('loading').hidden })"},
        {"wait": 6},
        # AC's ESC menu: Drive (step 0), then the keys
        {"cdp": "Input.dispatchMouseEvent", "params": {"type": "mouseMoved", "x": 50, "y": 172}}, {"wait": 0.5},
        {"cdp": "Input.dispatchMouseEvent", "params": {"type": "mousePressed", "x": 50, "y": 172, "button": "left", "clickCount": 1}}, {"wait": 0.2},
        {"cdp": "Input.dispatchMouseEvent", "params": {"type": "mouseReleased", "x": 50, "y": 172, "button": "left", "clickCount": 1}}, {"wait": 3},
        {"js": "'drive beforeunload asks: ' + " + unload},
        {"js": "window.__blocked = []; for (const t of ['keydown', 'mouseup']) window.addEventListener(t, (e) => window.__blocked.push((e.code || 'mouse' + e.button) + (e.altKey ? '+alt' : '') + (e.ctrlKey ? '+ctrl' : '') + '=' + e.defaultPrevented)); 'recording'"},
        {"js": where}]
for name in ['Alt+ArrowLeft', 'Alt+ArrowRight', 'Backspace', 'F5', 'Ctrl+KeyR', 'F3', 'Ctrl+KeyF', 'Ctrl+KeyP', 'Ctrl+KeyS', 'Ctrl+KeyD', 'BrowserBack']:
    flow += press(name) + [{"js": f"'{name}: ' + " + where}]
flow += button(3) + [{"js": "'mouse back: ' + " + where}] + button(4) + [{"js": "'mouse forward: ' + " + where}]
flow += [{"js": "'before F1: ' + " + where}] + press('F1') + [{"js": "'F1: ' + " + where}] + press('F1') + [{"js": "'F1 again: ' + " + where}]
flow += press('F6') + [{"js": "'F6: ' + " + where}, {"shot": f"{shots}/f6.png"}]
flow += press('F11') + [{"wait": 1}, {"shot": f"{shots}/f11.png"}, {"js": "'F11: ' + " + where}]
flow += [{"cdp": "Input.dispatchKeyEvent", "params": {"type": "rawKeyDown", "code": "ArrowUp", "key": "ArrowUp", "windowsVirtualKeyCode": 0x26}}, {"wait": 4},
         {"js": "'gas: ' + " + where},
         {"cdp": "Input.dispatchKeyEvent", "params": {"type": "keyUp", "code": "ArrowUp", "key": "ArrowUp", "windowsVirtualKeyCode": 0x26}},
         {"shot": f"{shots}/end.png"}, {"js": "JSON.stringify(window.__blocked)"}]
json.dump(flow, open(out, 'w'), indent=1)
