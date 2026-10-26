# Task 42 test app: calls every data getter of `ac` with good and bad arguments and logs
# every return value or exception, every frame.
import re
import ac
import acsys

log = open("apps/python/acapi_data/calls.txt", "w")
frame = 0
ARGS = [(), (0,), (1,), (-1,), (99,), ("x",), (0, 0), (1.5,)]
GETTERS = ["getCarMinHeight", "getCarName", "getCarSkin", "getCarBallast", "getCarRestrictor",
           "getCarTyreCompound", "getCarEngineBrakeCount", "getCarPowerControllerCount", "getCarFFB",
           "isCarInPitline", "isCarInPitlane", "isCarInPit", "isConnected", "isAIControlled",
           "getDriverName", "getDriverNationCode", "getCarsCount", "getCarLeaderboardPosition",
           "getCarRealTimeLeaderboardPosition", "getLastSplits", "getCurrentSplits", "getTrackName",
           "getTrackConfiguration", "getTrackLength", "getWindSpeed", "getWindDirection", "isAcLive",
           "getServerName", "getServerIP", "getServerHttpPort", "getServerSlotsCount", "getCameraMode",
           "isCameraOnBoard", "getCameraCarCount", "getFocusedCar", "getItemCount", "getSelectedItems"]


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
    except BaseException as e:
        log.write("%d %s%s ! %s %s\n" % (frame, name, show(args), type(e).__name__, e))


# the game has no check of the car on these: a bad car crashes it
UNCHECKED = {"getCarMinHeight", "getCarLeaderboardPosition", "getCarRealTimeLeaderboardPosition",
             "getLastSplits", "getCurrentSplits", "isCameraOnBoard", "getCameraCarCount"}
# ... and these read past the wheels' arrays for a wheel of -1 or 4
BY_WHEEL = {41, 42, 55, 58}


def sweep():
    for name in GETTERS:
        for a in ARGS:
            if name in UNCHECKED and a[:1] in [(-1,), (99,), (1.5,)]:
                continue
            call(name, *a)
    for car in (0, 1, -1, 99):
        for cs in range(-1, 75):
            call("getCarState", car, cs)
            for wheel in (0, 1, 2, 3, 4, -1):
                if cs in BY_WHEEL and wheel in (4, -1) and car in (0, 1):
                    continue
                call("getCarState", car, cs, wheel)
    for a in [(), (0,), ("x", 1), (0, "x"), (0, 1.5), (0, 1, "x"), (0, 1, 2, 3)]:
        call("getCarState", *a)


def acMain(ac_version):
    log.write("acMain %r\n" % (ac_version,))
    log.write("ac %r\n" % (sorted(n for n in dir(ac) if not n.startswith("__")),))
    log.write("acsys %r\n" % (sorted((k, sorted(v for v in dir(getattr(acsys, k)) if not v.startswith("__"))) for k in dir(acsys) if not k.startswith("__")),))
    sweep()
    log.flush()
    return "acapi_data"


def acUpdate(deltaT):
    global frame
    frame += 1
    log.write("update %d %r\n" % (frame, deltaT))
    sweep()
    log.flush()


def acShutdown():
    log.write("shutdown %d\n" % frame)
    log.close()
