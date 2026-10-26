# Task 42 test app: a window with every kind of control, every setter and listener of `ac`
# called with good and bad arguments, the drawing calls in a render callback; every return
# value, exception and listener call is logged.
import re
import ac
import acsys

log = open("apps/python/acapi_ui/calls.txt", "w")
frame = 0
app = 0
c = {}


class Clear(Exception):
    pass


def clear():
    # a call that failed to parse its arguments returns -1 and leaves its error set: raising and
    # catching one replaces it
    try:
        raise Clear()
    except Clear:
        pass


def show(x):
    # (no addresses: they differ between runs)
    return re.sub(r" at 0x[0-9A-Fa-f]+", "", repr(x))


def call(name, *args):
    try:
        r = getattr(ac, name)(*args)
        clear()
        log.write("%d %s%s = %s\n" % (frame, name, show(args), show(r)))
        return r
    except BaseException as e:
        log.write("%d %s%s ! %s %s\n" % (frame, name, show(args), type(e).__name__, e))
        return None


# the game keeps borrowed references to the callbacks: the app must keep them alive
listeners = []


def listener(tag):
    def f(*args):
        log.write("%d listener %s %s\n" % (frame, tag, show(args)))
    listeners.append(f)
    return f


def onRender(deltaT):
    log.write("%d render %r\n" % (frame, deltaT))
    call("glColor4f", 1, 0, 0, 0.5)
    call("glBegin", acsys.GL.Triangles)
    call("glVertex2f", 10, 10)
    call("glVertex2f", 100, 10)
    call("glVertex2f", 10, 80)
    call("glEnd")
    call("glColor3f", 0, 1, 0)
    call("glBegin", acsys.GL.Quads)
    for v in ((120, 20), (180, 20), (180, 60), (120, 60)):
        call("glVertex2f", *v)
    call("glEnd")
    for mode in (acsys.GL.Lines, acsys.GL.LineStrip):
        call("glBegin", mode)
        call("glVertex2f", 200, 20)
        call("glVertex2f", 260, 70)
        call("glVertex2f", 300, 20)
        call("glEnd")
    call("glColor4f", 0.2, 0.4, 1, 1)
    call("glQuad", 20, 100, 60, 30)
    call("glQuadTextured", 100, 100, 64, 64, c.get("tex", -1))
    call("glQuadTextured", 180, 100, 64, 64, 77)
    for bad in [("glBegin", "x"), ("glVertex2f", 1), ("glColor4f", 1, 2), ("glQuad", "a", 1, 2, 3), ("glColor3f",)]:
        call(*bad)


def acMain(ac_version):
    global app
    log.write("acMain %r\n" % (ac_version,))
    app = call("newApp", "API test")
    call("newApp")
    call("newApp", 5)
    call("setTitle", app, "API test window")
    call("setSize", app, 420, 300)
    call("getSize", app)
    call("setPosition", app, 300, 200)
    call("getPosition", app)
    call("setIconPosition", app, 10, 4)
    call("setTitlePosition", app, 40, 6)
    call("setBackgroundColor", app, 0.1, 0.2, 0.3)
    call("setBackgroundOpacity", app, 0.8)
    call("drawBackground", app, 1)
    call("drawBorder", app, 1)
    call("setBackgroundTexture", app, "apps/python/gMeter/gmeterBackground.png")
    call("setBackgroundTexture", app, "")
    call("addRenderCallback", app, onRender)
    call("addOnAppActivatedListener", app, listener("activated"))
    call("addOnAppDismissedListener", app, listener("dismissed"))
    c["label"] = call("addLabel", app, "a label")
    c["button"] = call("addButton", app, "a button")
    c["check"] = call("addCheckBox", app, "a check box")
    c["textbox"] = call("addTextBox", app, "a text box")
    c["input"] = call("addTextInput", app, "an input")
    c["spinner"] = call("addSpinner", app, "a spinner")
    c["progress"] = call("addProgressBar", app, "a progress bar")
    c["list"] = call("addListBox", app, "a list")
    c["graph"] = call("addGraph", app, "a graph")
    c["tex"] = call("newTexture", "apps/python/gMeter/gmeterBackground.png")
    call("newTexture", "apps/python/nothing.png")
    y = 30
    for k in ("label", "button", "check", "textbox", "input", "spinner", "progress", "list", "graph"):
        call("setPosition", c[k], 10, y)
        call("setSize", c[k], 150, 20)
        y += 24
    call("setText", c["label"], "label text")
    call("getText", c["label"])
    call("getText", c["input"])
    call("setFontColor", c["label"], 1, 1, 0, 1)
    call("setFontSize", c["label"], 18)
    call("setFontAlignment", c["label"], "center")
    call("setFontAlignment", c["button"], "right")
    call("setFontAlignment", c["check"], "left")
    call("setFontAlignment", c["check"], "middle")
    call("initFont", 0, "Segoe UI", 0, 0)
    call("initFont", 0, "NoSuchFont", 1, 1)
    call("setCustomFont", c["label"], "Segoe UI", 0, 1)
    call("setFont", c["label"], "x")
    call("setValue", c["spinner"], 5)
    call("setRange", c["spinner"], 0, 10)
    call("setStep", c["spinner"], 2)
    call("getValue", c["spinner"])
    call("setValue", c["progress"], 0.5)
    call("setRange", c["progress"], 0, 1)
    call("getValue", c["progress"])
    call("setValue", c["check"], 1)
    call("getValue", c["check"])
    call("addItem", c["list"], "one")
    call("addItem", c["list"], "two")
    for n in ("removeItem", "getItemCount", "setItemNumberPerPage", "highlightListBoxItem", "setAllowMultiSelection", "getSelectedItems", "setAllowDeselection"):
        call(n, c["list"], 1)
    call("addSerieToGraph", c["graph"], 1, 0, 0)
    call("addValueToGraph", c["graph"], 0, 0.5)
    call("addOnClickedListener", c["button"], listener("clicked"))
    call("addOnValidateListener", c["input"], listener("validate"))
    call("addOnValueChangeListener", c["spinner"], listener("value"))
    call("addOnCheckBoxChanged", c["check"], listener("check"))
    call("addOnListBoxSelectionListener", c["list"], listener("select"))
    call("addOnListBoxDeselectionListener", c["list"], listener("deselect"))
    call("addOnChatMessageListener", app, listener("chat"))
    call("setFocus", c["input"], 1)
    call("setVisible", c["textbox"], 0)
    # bad handles and bad arguments
    for n in ("setTitle", "setSize", "getSize", "setPosition", "getPosition", "setVisible", "setText", "getText", "setValue", "getValue", "setBackgroundOpacity", "drawBorder", "setFontColor", "addLabel", "addItem", "setFocus", "addRenderCallback"):
        call(n, 0, 1, 1)
        call(n, 999, 1, 1)
        call(n, -1, 1, 1)
        call(n, "x")
        call(n)
    call("log", "a line from acapi_ui")
    # (log and console of a number: the game builds a string of a null pointer)
    call("console", "a console line from acapi_ui")
    call("console")
    log.flush()
    return "acapi_ui"


def acUpdate(deltaT):
    global frame
    frame += 1
    log.write("update %d %r\n" % (frame, deltaT))
    call("setText", c["label"], "frame %d" % frame)
    call("setValue", c["progress"], (frame % 10) / 10.0)
    call("addValueToGraph", c["graph"], 0, (frame % 7) / 7.0)
    call("getValue", c["spinner"])
    call("getValue", c["check"])
    call("getText", c["input"])
    call("getPosition", app)
    call("getSize", app)
    log.flush()


def acShutdown():
    log.write("shutdown %d\n" % frame)
    log.close()
